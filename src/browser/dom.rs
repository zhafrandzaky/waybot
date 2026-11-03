//! DOM observation. One snapshot script owns every selector assumption.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::browser::Browser;
use crate::browser::marionette::{Marionette, MnError};
use crate::model::PageState;

pub const SNAPSHOT_SCRIPT: &str = r#"
const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
const num = (sel) => { const e = document.querySelector(sel); const n = e ? parseInt(norm(e.innerText), 10) : NaN; return Number.isFinite(n) ? n : null; };
const qc = document.querySelector('.quiz-container[data-quesid]');
const body = document.body ? document.body.innerText : '';
const submit = document.querySelector('button[data-cy=submit-button]');
const nextCandidates = [...document.querySelectorAll('div,button,a')].filter(e => e.children.length === 0 && /^\s*(next question|next|proceed|lanjut)\s*$/i.test(norm(e.innerText)));
const next = nextCandidates[0] || null;
const opts = [...document.querySelectorAll('button.option[data-cy^=option-], button[data-cy^=option-]')].filter((e) => {
  const r = e.getBoundingClientRect();
  const st = getComputedStyle(e);
  return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.display !== 'none' && !e.closest('[aria-hidden=true]');
}).map(e => {
  const cy = e.getAttribute('data-cy') || '';
  const idx = parseInt(cy.replace('option-', ''), 10);
  const aria = e.getAttribute('aria-label') || '';
  const text = aria.includes(',') ? aria.slice(aria.indexOf(',') + 1) : norm(e.innerText);
  return { index: Number.isFinite(idx) ? idx : -1, text, selected: e.getAttribute('aria-selected') === 'true', disabled: !!e.disabled, msq: String(e.className).includes('is-msq') };
});
const inputs = [...document.querySelectorAll('input[data-cy=fib-text-input], input[data-cy=dropdown-input], input:not([type=hidden]):not([type=checkbox]):not([type=radio]), textarea, [contenteditable=true], [role=textbox]')].map(e => ({
  tag: e.tagName, cy: e.getAttribute('data-cy'), placeholder: e.getAttribute('placeholder') || '', value: e.value || norm(e.innerText), editable: e.getAttribute('contenteditable') === 'true' || !e.disabled, css: e.getAttribute('data-cy') ? ('input[data-cy=' + e.getAttribute('data-cy') + ']') : (e.getAttribute('contenteditable') === 'true' ? '[contenteditable=true]' : e.tagName.toLowerCase())
}));
const media = qc ? [...qc.querySelectorAll('img')].map(i => ({ src: i.currentSrc || i.src || '', alt: i.getAttribute('alt') || '' })) : [];
const dialog = document.querySelector('[role=dialog], .dialog, .modal');
const explanationNode = [...document.querySelectorAll('div,section')].find(e => /answer explanation/i.test(norm(e.innerText)) && norm(e.innerText).length < 1200);
let qtext = qc ? norm((qc.querySelector('[data-cy=question-text], .question-text, .question-title, [class*=question-text]') || {}).innerText || '') : '';
if (!qtext && qc) {
  qtext = (qc.innerText || '').split('\n').map((l) => norm(l)).find((l) => l.length > 3 && !/^\d+\s*\/?\s*\d*$/.test(l) && !/^submit$/i.test(l)) || '';
}
return {
  url: location.href,
  focused: document.hasFocus(),
  visible: document.visibilityState === 'visible',
  question_id: qc ? qc.getAttribute('data-quesid') : (document.querySelector('[data-quesid]') ? document.querySelector('[data-quesid]').getAttribute('data-quesid') : null),
  question_number: num('[data-testid=current-question-number]'),
  total_questions: num('[data-testid=total-question-number]'),
  question_text: qtext,
  question_type_hint: qc ? (qc.className || '') : null,
  options: opts,
  inputs,
  media,
  next_button: { exists: !!next, disabled: next ? (next.getAttribute('aria-disabled') === 'true' || !!next.disabled) : true, label: next ? norm(next.innerText) : '' },
  submit_button: { exists: !!submit, disabled: submit ? !!submit.disabled : true, label: submit ? norm(submit.innerText) : '' },
  feedback_visible: /answer explanation|correct answer|jawaban benar/i.test(body),
  explanation: explanationNode ? norm(explanationNode.innerText).slice(0, 1200) : '',
  waiting_for_host: /waiting for the host|menunggu host/i.test(body),
  finished: /game over|you finished|see results|leaderboard|skip to summary|survey|summary|ringkasan|akurasi|accuracy/i.test(body),
  interstitial: /next up|who's up next|selanjutnya/i.test(body) ? 'interstitial' : null,
  dialog_text: dialog ? norm(dialog.innerText).slice(0, 500) : '',
  error_text: /room not found|incomplete game already exists|expired/i.test(body) ? norm(body).slice(0, 400) : '',
  fullscreen_prompt: [...document.querySelectorAll('button')].some(b => /fullscreen/i.test(b.getAttribute('aria-label') || '')),
  login_popup: /login ke wayground|login to wayground/i.test(body) && body.length < 4000,
  body_excerpt: norm(body).slice(0, 3000)
};
"#;

pub fn snapshot(mn: &mut Marionette) -> Result<PageState, MnError> {
    mn.set_timeout(Duration::from_secs(8));
    let value = mn.script(SNAPSHOT_SCRIPT, Value::Array(vec![]));
    mn.set_timeout(Duration::from_secs(60));
    let value = value?;
    serde_json::from_value(value).map_err(|_| MnError::Protocol)
}

pub fn eval_bool(mn: &mut Marionette, script: &str) -> Result<bool, MnError> {
    mn.set_timeout(Duration::from_secs(8));
    let out = mn.script(script, Value::Array(vec![]));
    mn.set_timeout(Duration::from_secs(60));
    Ok(out?.as_bool().unwrap_or(false))
}

pub fn wait_for<F>(mn: &mut Marionette, timeout: Duration, mut check: F) -> Result<bool, MnError>
where
    F: FnMut(&PageState) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(state) = snapshot(mn)
            && check(&state)
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Ensure a CSS element exists, returning its element id.
#[allow(dead_code)]
pub fn find_visible(mn: &mut Marionette, css: &str) -> Option<String> {
    mn.find(css).ok()
}

pub fn eval_text(browser: &mut Browser) -> String {
    eval_string(&mut browser.mn, "return String(document.location.href + ' :: ' + (document.body ? document.body.innerText.slice(0, 200) : ''));").unwrap_or_default()
}

pub fn eval_string(mn: &mut Marionette, script: &str) -> Result<String, MnError> {
    mn.set_timeout(Duration::from_secs(8));
    let out = mn.script(script, Value::Array(vec![]));
    mn.set_timeout(Duration::from_secs(60));
    Ok(out?.as_str().unwrap_or_default().to_string())
}

/// Poll for an element, tolerating transient script timeouts.
pub fn wait_exists(mn: &mut Marionette, css: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if exists(mn, css) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

pub fn exists(mn: &mut Marionette, css: &str) -> bool {
    eval_bool(
        mn,
        &format!("return !!document.querySelector({});", js_str(css)),
    )
    .unwrap_or(false)
}

/// Close the optional "login with Google" popup so it cannot swallow clicks.
pub fn dismiss_login(mn: &mut Marionette) -> bool {
    let script = r#"
const dlg = [...document.querySelectorAll('div,section')].find((e) => /login ke wayground|login to wayground/i.test(e.innerText || '') && (e.innerText || '').length < 500);
if (!dlg) return false;
const btn = [...dlg.querySelectorAll('button')].find((b) => /close|tutup|dismiss/i.test(b.getAttribute('aria-label') || '') || /^[x×✕]$/i.test((b.innerText || '').trim()));
if (btn) { btn.click(); return true; }
const r = dlg.getBoundingClientRect();
const top = document.elementFromPoint(r.right - 14, r.top + 14);
if (top) { top.click(); return true; }
return false;
"#;
    eval_bool(mn, script).unwrap_or(false)
}

/// Dismiss generic UI blockers (cookie banners, generic modals). Returns the label used.
pub fn dismiss_obstacles(mn: &mut Marionette) -> Option<String> {
    let script = r#"
const pack = document.querySelector('[role=dialog], .modal, [class*=cookie], [class*=consent], [class*=popup], [class*=overlay]');
if (!pack) return null;
const visible = (e) => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e); return r.width > 4 && r.height > 4 && s.visibility !== 'hidden' && s.display !== 'none'; };
const buttons = [...pack.querySelectorAll('button'), ...document.querySelectorAll('[class*=cookie] button, [class*=consent] button')].filter(visible);
const label = /^(accept|accept all|setuju|got it|mengerti|ok|oke|continue|lanjutkan|yes|ya)$/i;
const btn = buttons.find((b) => label.test((b.innerText || '').trim()));
const close = buttons.find((b) => /close|tutup|dismiss/i.test(b.getAttribute('aria-label') || ''));
const target = btn || close;
if (!target) return null;
const text = (target.innerText || target.getAttribute('aria-label') || '').trim();
target.click();
return text || 'tutup';
"#;
    let value = mn.script(script, serde_json::json!([])).ok()?;
    value.as_str().map(str::to_string)
}

/// Compact, AI-readable page snapshot with actionable handles.
pub fn agent_snapshot(mn: &mut Marionette) -> Result<Value, MnError> {
    let script = r#"
const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
const visible = (e) => { const r = e.getBoundingClientRect(); const st = getComputedStyle(e); return r.width > 3 && r.height > 3 && st.visibility !== 'hidden' && st.display !== 'none'; };
const css = (e) => {
  const cy = e.getAttribute('data-cy');
  if (cy) return '[data-cy="' + cy + '"]';
  if (e.className && String(e.className).includes('ProseMirror')) return '.ProseMirror';
  return e.tagName.toLowerCase();
};
const qc = document.querySelector('.quiz-container[data-quesid]') || document.querySelector('[data-quesid]');
const num = (sel) => { const e = document.querySelector(sel); const n = e ? parseInt(norm(e.innerText), 10) : NaN; return Number.isFinite(n) ? n : null; };
return {
  url: location.href,
  qid: qc ? qc.getAttribute('data-quesid') : null,
  number: num('[data-testid=current-question-number]'),
  total: num('[data-testid=total-question-number]'),
  text: qc ? norm((qc.querySelector('[data-cy=question-text], .question-text, .question-title') || {}).innerText || '') : '',
  options: [...document.querySelectorAll('button.option[data-cy^=option-], button[data-cy^=option-]')].filter(visible).map((e) => ({
    index: parseInt((e.getAttribute('data-cy') || '').replace('option-', ''), 10),
    text: norm((e.getAttribute('aria-label') || '').split(',').slice(1).join(',') || e.innerText),
    selected: e.getAttribute('aria-selected') === 'true',
    disabled: !!e.disabled
  })),
  inputs: [...document.querySelectorAll('input:not([type=hidden]):not([type=checkbox]):not([type=radio]),textarea,[contenteditable=true],[role=textbox]')].filter(visible).map((e) => ({
    tag: e.tagName, selector: css(e), placeholder: e.getAttribute('placeholder') || '', value: (e.value || norm(e.innerText)).slice(0, 200)
  })),
  buttons: [...document.querySelectorAll('button,[role=button],[data-cy=submit-button],[data-cy=start-game-button]')].filter(visible).slice(0, 25).map((e) => ({
    text: norm(e.innerText).slice(0, 40), cy: e.getAttribute('data-cy'), aria: e.getAttribute('aria-label') || '', disabled: !!e.disabled
  })),
  submit: (() => { const b = document.querySelector('button[data-cy=submit-button]'); return b ? { exists: true, disabled: !!b.disabled } : { exists: false, disabled: true }; })(),
  next: (() => { const n = [...document.querySelectorAll('div,button,a')].find((e) => e.children.length === 0 && /^\s*(next question|next|proceed|lanjut)\s*$/i.test((e.innerText || '').trim())); return n ? { exists: true, disabled: n.getAttribute('aria-disabled') === 'true' } : { exists: false, disabled: true }; })(),
  feedback: /answer explanation|correct answer|jawaban benar/i.test(document.body ? document.body.innerText : ''),
  dialog: norm((document.querySelector('[role=dialog], .modal') || {}).innerText || '').slice(0, 200),
  body: norm(document.body ? document.body.innerText : '').slice(0, 700)
};
"#;
    mn.script(script, Value::Array(vec![]))
}

pub fn js_str(input: &str) -> String {
    serde_json::to_string(input).unwrap_or_else(|_| "\"\"".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_has_anchor_selectors() {
        for needle in [
            "quiz-container[data-quesid]",
            "data-cy=submit-button",
            "current-question-number",
            "option-",
        ] {
            assert!(SNAPSHOT_SCRIPT.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn js_string_escapes() {
        assert_eq!(js_str("a\"b"), "\"a\\\"b\"");
    }
}
