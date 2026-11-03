//! Answer execution and the snapshot-driven advance policy.

use std::time::{Duration, Instant};

use crate::browser::Browser;
use crate::browser::dom;
use crate::browser::input;
use crate::model::{Answer, PageState, Question, QuestionType};
use crate::util::norm_key;
use crate::wayground::selectors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceAction {
    Wait,
    ClickNext,
    AutoAdvanced,
    RetryNext,
    HandleDialog,
    Finished,
}

/// Priorities are fixed; the sequence of events is not.
pub fn decide_advance(
    state: &PageState,
    acked: bool,
    question_changed: bool,
    next_retries: u8,
    since_ack_ms: u64,
) -> AdvanceAction {
    if question_changed {
        return AdvanceAction::AutoAdvanced;
    }
    if state.finished {
        return AdvanceAction::Finished;
    }
    if !state.dialog_text.is_empty() || state.fullscreen_prompt {
        return AdvanceAction::HandleDialog;
    }
    if !acked {
        return AdvanceAction::Wait;
    }
    if state.next_button.exists {
        if state.next_button.disabled {
            return AdvanceAction::Wait;
        }
        if since_ack_ms >= 250 {
            return AdvanceAction::ClickNext;
        }
        return AdvanceAction::Wait;
    }
    if since_ack_ms > 12_000 && state.feedback_visible && next_retries < 2 {
        return AdvanceAction::RetryNext;
    }
    AdvanceAction::Wait
}

pub fn match_indices(question: &Question, answer: &Answer) -> Vec<u32> {
    if question.options.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for value in &answer.values {
        let key = norm_key(value);
        if key.is_empty() {
            continue;
        }
        let exact = question
            .options
            .iter()
            .position(|o| norm_key(&o.text) == key);
        let idx = exact.or_else(|| {
            question.options.iter().position(|o| {
                let opt = norm_key(&o.text);
                !opt.is_empty() && (opt.contains(&key) || key.contains(&opt))
            })
        });
        if let Some(pos) = idx {
            let dom = question.options[pos].dom_index.unwrap_or(pos as u32);
            if !out.contains(&dom) {
                out.push(dom);
            }
        }
    }
    if out.is_empty() && question.type_ == QuestionType::Mcq {
        out.push(0);
    }
    out
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("gagal berinteraksi: {0}")]
    Io(String),
    #[error("tidak ada kontrol yang cocok")]
    NoControl,
    #[error("submit tidak pernah aktif")]
    SubmitDisabled,
}

const VISIBLE_PICK: &str = r#"
const all = [...document.querySelectorAll(arguments[0])];
const visible = all.filter((e) => {
  const r = e.getBoundingClientRect();
  const st = getComputedStyle(e);
  return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.display !== 'none' && !e.closest('[aria-hidden=true]');
});
const el = visible[0] || all[0];
if (!el) return null;
el.scrollIntoView({ block: 'center', inline: 'center' });
const r = el.getBoundingClientRect();
return [r.x + r.width / 2, r.y + r.height / 2, el.getAttribute('aria-selected') === 'true'];
"#;

fn pick_visible(browser: &mut Browser, css: &str) -> Option<(f64, f64, bool)> {
    let value = browser
        .mn
        .script(VISIBLE_PICK, serde_json::json!([css]))
        .ok()?;
    let arr = value.as_array()?;
    Some((
        arr.first()?.as_f64()?,
        arr.get(1)?.as_f64()?,
        arr.get(2).and_then(|v| v.as_bool()).unwrap_or(false),
    ))
}

/// Click an option and prove it became selected; escalates to real mouse events.
pub fn select_option(browser: &mut Browser, index: u32) -> Result<(), ExecError> {
    let css = selectors::option(index);
    for attempt in 0..3 {
        let Some((x, y, selected)) = pick_visible(browser, &css) else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        if selected {
            return Ok(());
        }
        match attempt {
            0 => {
                let _ = input::click_at(&mut browser.mn, x, y);
            }
            1 => {
                let _ = browser.mn.script(
                    &format!(
                        "const e=document.querySelector({}); if(e) e.click(); return !!e;",
                        dom::js_str(&css)
                    ),
                    serde_json::json!([]),
                );
            }
            _ => {
                let _ = input::click_at(&mut browser.mn, x, y);
            }
        }
        std::thread::sleep(Duration::from_millis(260));
        if pick_visible(browser, &css)
            .map(|(_, _, sel)| sel)
            .unwrap_or(false)
        {
            return Ok(());
        }
    }
    Err(ExecError::NoControl)
}

fn submit(browser: &mut Browser) -> Result<(), ExecError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if dom::exists(&mut browser.mn, selectors::SUBMIT_BUTTON) {
            let disabled = browser
                .mn
                .script(
                    "const b=document.querySelector(arguments[0]); return b ? b.disabled : true;",
                    serde_json::json!([selectors::SUBMIT_BUTTON]),
                )
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !disabled {
                input::click_selector(&mut browser.mn, selectors::SUBMIT_BUTTON)
                    .map_err(|e| ExecError::Io(e.to_string()))?;
                return Ok(());
            }
        }
        if Instant::now() > deadline {
            return Err(ExecError::SubmitDisabled);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Some types auto-submit as soon as a choice is made.
fn submit_soft(browser: &mut Browser) -> Result<(), ExecError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if dom::exists(&mut browser.mn, selectors::SUBMIT_BUTTON) {
            let disabled = browser
                .mn
                .script(
                    "const b=document.querySelector(arguments[0]); return b ? b.disabled : true;",
                    serde_json::json!([selectors::SUBMIT_BUTTON]),
                )
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !disabled {
                input::click_selector(&mut browser.mn, selectors::SUBMIT_BUTTON)
                    .map_err(|e| ExecError::Io(e.to_string()))?;
                return Ok(());
            }
        }
        if Instant::now() > deadline {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

pub fn execute(
    browser: &mut Browser,
    question: &Question,
    state: &PageState,
    answer: &Answer,
) -> Result<(), ExecError> {
    match question.type_ {
        QuestionType::Mcq | QuestionType::Poll => {
            let indices = match_indices(question, answer);
            let index = *indices.first().ok_or(ExecError::NoControl)?;
            select_option(browser, index)?;
            return submit_soft(browser);
        }
        QuestionType::Msq => {
            let indices = match_indices(question, answer);
            if indices.is_empty() {
                return Err(ExecError::NoControl);
            }
            for index in indices {
                select_option(browser, index)?;
                std::thread::sleep(Duration::from_millis(crate::util::jitter_ms(120, 320)));
            }
        }
        QuestionType::Blank | QuestionType::Dropdown => {
            let css = state
                .inputs
                .first()
                .map(|i| i.css.clone())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| {
                    "input[data-cy=fib-text-input], input[data-cy=dropdown-input]".into()
                });
            let text = answer.values.first().cloned().unwrap_or_default();
            input::type_selector(&mut browser.mn, &css, &text)
                .map_err(|e| ExecError::Io(e.to_string()))?;
        }
        QuestionType::Open => {
            let text = answer.values.join(" ");
            let css = state
                .inputs
                .first()
                .map(|i| {
                    if !i.css.is_empty() {
                        i.css.clone()
                    } else if i.tag == "DIV" || i.editable {
                        selectors::OPEN_EDITOR.to_string()
                    } else {
                        "textarea".to_string()
                    }
                })
                .unwrap_or_else(|| selectors::OPEN_EDITOR.to_string());
            if css.contains("contenteditable") || css.contains("ProseMirror") || css == "div" {
                input::type_contenteditable(&mut browser.mn, &css, &text)
                    .map_err(|e| ExecError::Io(e.to_string()))?;
            } else {
                input::type_selector(&mut browser.mn, &css, &text)
                    .map_err(|e| ExecError::Io(e.to_string()))?;
            }
        }
        QuestionType::Reorder => {
            // Click items in answer order; Wayground moves clicked items into place.
            for value in &answer.values {
                let script = format!(
                    "const items=[...document.querySelectorAll('[data-testid^=reorder-option-]')]; const el=items.find(e=>e.innerText.trim().toLowerCase().includes({})); if(el){{el.click(); return true;}} return false;",
                    dom::js_str(&value.to_lowercase())
                );
                let _ = browser.mn.script(&script, serde_json::json!([]));
                std::thread::sleep(Duration::from_millis(crate::util::jitter_ms(150, 300)));
            }
        }
        QuestionType::Match => {
            for pair in &answer.values {
                let mut parts = pair.split('=');
                let left = parts.next().unwrap_or("").trim();
                let right = parts.next().unwrap_or("").trim();
                for needle in [left, right] {
                    if needle.is_empty() {
                        continue;
                    }
                    let script = format!(
                        "const el=[...document.querySelectorAll('[data-testid=match-option-card-text]')].find(e=>e.innerText.trim().toLowerCase().includes({})); if(el){{el.click(); return true;}} return false;",
                        dom::js_str(&needle.to_lowercase())
                    );
                    let _ = browser.mn.script(&script, serde_json::json!([]));
                    std::thread::sleep(Duration::from_millis(crate::util::jitter_ms(120, 260)));
                }
            }
        }
        QuestionType::DragDrop | QuestionType::DndImage | QuestionType::Unknown => {
            let indices = match_indices(question, answer);
            if let Some(index) = indices.first() {
                select_option(browser, *index)?;
                return submit_soft(browser);
            } else {
                return Err(ExecError::NoControl);
            }
        }
        QuestionType::Hotspot => {
            input::click_at(&mut browser.mn, 320.0, 360.0)
                .map_err(|e| ExecError::Io(e.to_string()))?;
            return submit_soft(browser);
        }
    }
    submit(browser)
}

/// Click Next, tolerating `<div>` buttons.
pub fn click_next(browser: &mut Browser) -> Result<(), ExecError> {
    let script = r#"
const el=[...document.querySelectorAll('div,button,a')].find(e=>e.children.length===0 && /^\s*(next question|next|proceed|lanjut)\s*$/i.test((e.innerText||'').trim()));
if (!el) return false;
el.scrollIntoView({block:'center'});
el.click();
return true;
"#;
    let ok = browser
        .mn
        .script(script, serde_json::json!([]))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ExecError::NoControl)
    }
}

pub fn handle_unknown(browser: &mut Browser, store: &crate::store::Store, label: &str) {
    if let Ok(state) = dom::snapshot(&mut browser.mn) {
        let value = serde_json::to_value(&state).unwrap_or_default();
        let _ = store.save_snapshot(label, &value);
    }
    browser.screenshot(label, store);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DomButton, DomOption, OptionItem, Question};
    use std::collections::HashMap;

    fn state_with(next: bool, disabled: bool) -> PageState {
        PageState {
            next_button: DomButton {
                exists: next,
                disabled,
                label: "Next question".into(),
            },
            ..Default::default()
        }
    }

    fn question(opts: &[&str]) -> Question {
        Question {
            id: "q".into(),
            type_: QuestionType::Mcq,
            text: "t".into(),
            explain: String::new(),
            media: vec![],
            options: opts
                .iter()
                .enumerate()
                .map(|(i, t)| OptionItem {
                    id: format!("o{i}"),
                    text: (*t).into(),
                    media: vec![],
                    dom_index: Some(i as u32),
                })
                .collect(),
            targets: vec![],
            time_ms: 1000,
            hash: String::new(),
            raw: serde_json::Value::Null,
        }
    }

    #[test]
    fn auto_advance_wins_over_next_button() {
        assert_eq!(
            decide_advance(&state_with(true, false), true, true, 0, 500),
            AdvanceAction::AutoAdvanced
        );
    }

    #[test]
    fn next_clicks_only_after_ack_and_stability() {
        assert_eq!(
            decide_advance(&state_with(true, false), false, false, 0, 900),
            AdvanceAction::Wait
        );
        assert_eq!(
            decide_advance(&state_with(true, false), true, false, 0, 100),
            AdvanceAction::Wait
        );
        assert_eq!(
            decide_advance(&state_with(true, false), true, false, 0, 400),
            AdvanceAction::ClickNext
        );
        assert_eq!(
            decide_advance(&state_with(true, true), true, false, 0, 400),
            AdvanceAction::Wait
        );
    }

    #[test]
    fn stuck_feedback_triggers_bounded_retry() {
        let mut state = state_with(false, true);
        state.feedback_visible = true;
        assert_eq!(
            decide_advance(&state, true, false, 0, 13_000),
            AdvanceAction::RetryNext
        );
        assert_eq!(
            decide_advance(&state, true, false, 2, 13_000),
            AdvanceAction::Wait
        );
    }

    #[test]
    fn question_matching_uses_text_then_falls_back() {
        let q = question(&["Keyboard", "Monitor"]);
        let mut answer = Answer::new("q", vec!["monitor".into()], crate::model::AnswerSource::Ai);
        assert_eq!(match_indices(&q, &answer), vec![1]);
        answer.values = vec!["tidak ada".into()];
        assert_eq!(match_indices(&q, &answer), vec![0]);
    }

    #[test]
    fn msq_keeps_multiple_indices() {
        let mut q = question(&["A", "B", "C"]);
        q.type_ = QuestionType::Msq;
        let answer = Answer::new(
            "q",
            vec!["A".into(), "C".into()],
            crate::model::AnswerSource::Key,
        );
        assert_eq!(match_indices(&q, &answer), vec![0, 2]);
    }

    #[test]
    fn dom_index_mapping_prefers_rendered_index() {
        let mut q = question(&["A", "B"]);
        q.options[0].dom_index = Some(3);
        let answer = Answer::new("q", vec!["A".into()], crate::model::AnswerSource::Key);
        assert_eq!(match_indices(&q, &answer), vec![3]);
        let _: HashMap<u32, u32> = HashMap::new();
        let _ = DomOption::default();
    }
}
