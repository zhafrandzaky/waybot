//! Join flow through the real UI.

use std::time::Duration;

use crate::browser::Browser;
use crate::browser::dom;
use crate::browser::input;
use crate::events::{Level, Tx, log};
use crate::wayground::{selectors, urls};

#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("{0}")]
    Page(String),
    #[error("form join tidak muncul")]
    NoForm,
    #[error("room ditolak: {0}")]
    Rejected(String),
    #[error("layar nama tidak muncul")]
    NoNameScreen,
    #[error("gagal masuk lobby")]
    NoLobby,
    #[error("butuh login akun (loginRequired)")]
    LoginRequired,
}

pub fn join(
    browser: &mut Browser,
    code: &str,
    name: &str,
    resume: bool,
    tx: &Tx,
) -> Result<(), JoinError> {
    log(tx, Level::Info, "membuka wayground.com/join");
    browser
        .navigate(urls::JOIN)
        .map_err(|e| JoinError::Page(e.to_string()))?;

    // Wait until the document is at least interactive.
    let loaded = dom::wait_for(&mut browser.mn, Duration::from_secs(25), |s| {
        !s.url.is_empty()
    })
    .unwrap_or(false);
    if !loaded {
        return Err(JoinError::Page("halaman tidak termuat".into()));
    }

    // The SPA renders asynchronously ("Connecting…" first).
    let appeared = dom::wait_exists(
        &mut browser.mn,
        selectors::JOIN_INPUT,
        Duration::from_secs(30),
    );
    if appeared {
        // happy path
    } else if let Some(target) = resolve_join_input(browser) {
        log(
            tx,
            Level::Warn,
            format!("selector kode diganti ke {target}"),
        );
    } else {
        let body = dom::snapshot(&mut browser.mn)
            .map(|s| s.body_excerpt)
            .unwrap_or_default();
        return Err(JoinError::Page(format!(
            "form join tidak muncul | {} | {}",
            url_now(browser),
            crate::util::truncate_chars(&body, 300)
        )));
    }

    // Fullscreen right at the URL, before any join interaction.
    browser.enter_fullscreen();

    let code_field =
        resolve_join_input(browser).unwrap_or_else(|| selectors::JOIN_INPUT.to_string());
    log(tx, Level::Info, format!("mengetik kode di {code_field}"));
    input::type_selector(&mut browser.mn, &code_field, code)
        .map_err(|e| JoinError::Page(e.to_string()))?;

    let join_button = if dom::exists(&mut browser.mn, selectors::JOIN_BUTTON) {
        selectors::JOIN_BUTTON.to_string()
    } else {
        "[data-cy=joinGame-button], button[type=submit]".to_string()
    };
    let typed = browser
        .mn
        .script(
            "const e=document.querySelector(arguments[0]); return e ? e.value : '';",
            serde_json::json!([code_field]),
        )
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    if typed.replace(' ', "") != code {
        log(
            tx,
            Level::Warn,
            format!("kode tidak masuk ({typed:?}); ulang"),
        );
        input::type_selector(&mut browser.mn, &code_field, code)
            .map_err(|e| JoinError::Page(e.to_string()))?;
    }
    // Wait until the join button is actually enabled.
    for _ in 0..20 {
        let enabled = browser
            .mn
            .script(
                "const b=document.querySelector(arguments[0]); return b ? (!b.disabled && b.getAttribute('aria-disabled') !== 'true') : false;",
                serde_json::json!([join_button]),
            )
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if enabled {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    log(tx, Level::Info, "klik join");
    input::click_selector(&mut browser.mn, &join_button)
        .map_err(|e| JoinError::Page(e.to_string()))?;

    // Wait for one of: name screen, rejection, resume dialog.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(JoinError::NoNameScreen);
        }
        let state = dom::snapshot(&mut browser.mn).map_err(|e| JoinError::Page(e.to_string()))?;
        crate::events::file_log(
            "join-poll",
            &format!(
                "{} | qid={:?} wait={} err={:?} dlg={:?}",
                state.url,
                state.question_id,
                state.waiting_for_host,
                state.error_text,
                state.dialog_text
            ),
        );
        if state.error_text.to_lowercase().contains("room not found") {
            return Err(JoinError::Rejected(state.error_text));
        }
        if has_resume(&state) {
            log(
                tx,
                Level::Warn,
                "nama sudah dipakai di room ini; memilih resume",
            );
            click_resume(browser);
            break;
        }
        if dom::exists(&mut browser.mn, selectors::NAME_INPUT) {
            break;
        }
        // Variant: the name is remembered from an earlier attempt.
        if state.body_excerpt.contains("Resume attempt")
            || state.body_excerpt.contains("Start Over")
        {
            break;
        }
        if state.waiting_for_host || state.has_question() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    if dom::exists(&mut browser.mn, selectors::NAME_INPUT) {
        log(tx, Level::Info, "mengisi nama");
        input::type_selector(&mut browser.mn, selectors::NAME_INPUT, name)
            .map_err(|e| JoinError::Page(e.to_string()))?;
        std::thread::sleep(Duration::from_millis(300));
        input::click_selector(&mut browser.mn, selectors::START_BUTTON)
            .map_err(|e| JoinError::Page(e.to_string()))?;
    } else if let Some(custom) = resolve_name_input(browser) {
        log(
            tx,
            Level::Warn,
            format!("selector nama diganti ke {custom}"),
        );
        input::type_selector(&mut browser.mn, &custom, name)
            .map_err(|e| JoinError::Page(e.to_string()))?;
        std::thread::sleep(Duration::from_millis(300));
        if let Some(button) = resolve_start_button(browser) {
            input::click_selector(&mut browser.mn, &button)
                .map_err(|e| JoinError::Page(e.to_string()))?;
        }
    } else {
        log(
            tx,
            Level::Info,
            "nama sudah diingat browser; mencoba Resume attempt",
        );
        if resume {
            ensure_name(browser, name, tx);
            let _ = click_text_button(browser, &["resume attempt", "resume", "start", "lanjut"]);
        } else {
            // Fresh run: discard the stale attempt, then join with the typed name.
            let _ = click_text_button(browser, &["start over", "mulai ulang"]);
            std::thread::sleep(Duration::from_millis(700));
            ensure_name(browser, name, tx);
            let _ = click_text_button(browser, &["start", "lanjut", "resume attempt", "resume"]);
        }
        let mut tried_url = false;
        let mut entered = false;
        for step in 0..15 {
            std::thread::sleep(Duration::from_millis(1000));
            let Ok(state) = dom::snapshot(&mut browser.mn) else {
                continue;
            };
            if state.url.contains("/join/game/") {
                entered = true;
                break;
            }
            if state.waiting_for_host {
                break;
            }
            if step == 4 && !tried_url {
                tried_url = true;
                if let Some(url) = game_url_from(browser) {
                    log(tx, Level::Warn, format!("fallback ke URL game {url}"));
                    let _ = browser.navigate(&url);
                }
            } else if state.url.contains("/pre-game/") {
                if resume {
                    let _ = click_text_button(browser, &["resume attempt", "resume", "start"]);
                } else {
                    let _ = click_text_button(browser, &["start over", "mulai ulang"]);
                    std::thread::sleep(Duration::from_millis(500));
                    ensure_name(browser, name, tx);
                    let _ = click_text_button(browser, &["start", "lanjut"]);
                }
            }
        }
        if entered {
            log(tx, Level::Info, "masuk halaman game");
            if let Ok(state) = dom::snapshot(&mut browser.mn)
                && !state.body_excerpt.contains(name)
            {
                log(
                    tx,
                    Level::Warn,
                    format!("nama {:?} tidak terlihat di halaman game", name),
                );
            }
        }
    }

    // Resume dialog can appear after Start; give it a moment to render.
    let mut resumed = false;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        let state = dom::snapshot(&mut browser.mn).map_err(|e| JoinError::Page(e.to_string()))?;
        if has_resume(&state) {
            log(
                tx,
                Level::Warn,
                "dialog resume muncul setelah Start; memilih YES",
            );
            click_resume(browser);
            resumed = true;
            break;
        }
        if state.waiting_for_host || state.has_question() {
            break;
        }
    }
    if !resumed {
        log(tx, Level::Info, "tidak ada dialog resume");
    }
    let state = dom::snapshot(&mut browser.mn).map_err(|e| JoinError::Page(e.to_string()))?;
    if state.error_text.to_lowercase().contains("login") {
        return Err(JoinError::LoginRequired);
    }

    // Lobby, or already inside a question.
    let ready = dom::wait_for(&mut browser.mn, Duration::from_secs(40), |s| {
        s.waiting_for_host || s.has_question() || s.finished || s.url.contains("/join/game/")
    })
    .map_err(|e| JoinError::Page(e.to_string()))?;
    if !ready {
        return Err(JoinError::NoLobby);
    }
    log(tx, Level::Info, "berhasil masuk room");
    Ok(())
}

fn url_now(browser: &mut Browser) -> String {
    dom::eval_text(browser)
}

fn resolve_join_input(browser: &mut Browser) -> Option<String> {
    if dom::exists(&mut browser.mn, selectors::JOIN_INPUT) {
        return Some(selectors::JOIN_INPUT.to_string());
    }
    for candidate in [
        "input[name=gamecode]",
        "input[placeholder*='code' i]",
        "input[type=text]",
    ] {
        if dom::exists(&mut browser.mn, candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn resolve_name_input(browser: &mut Browser) -> Option<String> {
    if dom::exists(&mut browser.mn, selectors::NAME_INPUT) {
        return Some(selectors::NAME_INPUT.to_string());
    }
    for candidate in ["input[name=name]", "input[placeholder*='name' i]"] {
        if dom::exists(&mut browser.mn, candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn resolve_start_button(browser: &mut Browser) -> Option<String> {
    if dom::exists(&mut browser.mn, selectors::START_BUTTON) {
        return Some(selectors::START_BUTTON.to_string());
    }
    for candidate in ["button[data-cy=start-game-button]", "button[type=submit]"] {
        if dom::exists(&mut browser.mn, candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn name_field(browser: &mut Browser) -> Option<String> {
    let script = r#"
const visible = (e) => { const r = e.getBoundingClientRect(); const st = getComputedStyle(e); return r.width > 4 && r.height > 4 && st.visibility !== 'hidden' && st.display !== 'none'; };
const el = [...document.querySelectorAll('input[type=text], input:not([type])')]
  .filter(visible)
  .find((e) => !(e.getAttribute('data-cy') || '').includes('gamecode'));
if (!el) return null;
const cy = el.getAttribute('data-cy');
if (cy) return 'input[data-cy="' + cy + '"]';
const name = el.getAttribute('name');
if (name) return 'input[name="' + name + '"]';
return 'input[type=text]';
"#;
    browser
        .mn
        .script(script, serde_json::json!([]))
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
}

/// Force the typed name onto the pre-game screen, even when the browser remembers one.
fn ensure_name(browser: &mut Browser, name: &str, tx: &Tx) {
    for attempt in 0..3 {
        if let Some(css) = name_field(browser) {
            if input::type_selector(&mut browser.mn, &css, name).is_ok() {
                let value = browser
                    .mn
                    .script(
                        "const e=document.querySelector(arguments[0]); return e ? e.value : '';",
                        serde_json::json!([css]),
                    )
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                log(tx, Level::Info, format!("nama diisi ulang: {value:?}"));
                if value.trim() == name {
                    return;
                }
            }
        } else {
            // The name may be shown as text; click it to reveal the editor.
            let _ = browser.mn.script(
                "const el=[...document.querySelectorAll('div,span,button')].find(e=>/your wayground name is|nama/i.test((e.innerText||'')) && (e.innerText||'').length<80); if(el){el.click(); return true;} return false;",
                serde_json::json!([]),
            );
            std::thread::sleep(Duration::from_millis(500));
        }
        if attempt == 2 {
            log(
                tx,
                Level::Warn,
                format!(
                    "field nama tidak ditemukan; memakai nama tersimpan browser (bisa berbeda dari {name:?})"
                ),
            );
        }
    }
}

fn game_url_from(browser: &mut Browser) -> Option<String> {
    let path = browser
        .mn
        .script(
            "return String(location.pathname + location.search);",
            serde_json::json!([]),
        )
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))?;
    let blob = path
        .split('/')
        .find(|s| s.len() > 40 && !s.starts_with("start") && !s.contains("pre-game"))?;
    Some(format!(
        "https://wayground.com/join/game/{blob}?gameType=live"
    ))
}

pub fn click_text_button(browser: &mut Browser, labels: &[&str]) -> Result<(), String> {
    let script = r#"
const labels = arguments[0];
const visible = (e) => { const r = e.getBoundingClientRect(); const st = getComputedStyle(e); return r.width > 4 && r.height > 4 && st.visibility !== 'hidden' && st.display !== 'none'; };
const candidates = [...document.querySelectorAll('button,a,div[role=button],[data-cy=start-game-button]')].filter(visible);
const btn = candidates.find((b) => {
  const text = ((b.innerText || '') + ' ' + (b.getAttribute('aria-label') || '')).trim().toLowerCase();
  return labels.some((l) => text === l || text.startsWith(l + ' ')) && !b.disabled && b.getAttribute('aria-disabled') !== 'true';
});
if (!btn) return false;
btn.click();
window.__waybotClicked = ((btn.innerText || '') + '|' + (btn.getAttribute('data-cy') || '')).slice(0, 60);
return true;
"#;
    let ok = browser
        .mn
        .script(script, serde_json::json!([labels]))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err("tombol Start tidak ditemukan".into())
    }
}

fn has_resume(state: &crate::model::PageState) -> bool {
    let dialog = state.dialog_text.to_lowercase();
    let body = state.body_excerpt.to_lowercase();
    dialog.contains("already exists") || body.contains("incomplete game already exists")
}

fn click_resume(browser: &mut Browser) {
    let yes = r#"
const buttons = [...document.querySelectorAll('button')];
const yes = buttons.find(b => /^\s*(yes|ya|resume)\s*$/i.test((b.innerText || '').trim()));
if (yes) { yes.click(); return true; }
return false;
"#;
    let _ = browser.mn.script(yes, serde_json::json!([]));
    std::thread::sleep(Duration::from_millis(1500));
}
