//! Keep the browser window focused; never leave it blurred for long.

use std::time::Duration;

use crate::anticheat::Metrics;
use crate::browser::dom;
use crate::browser::firefox::Firefox;
use crate::browser::marionette::{Marionette, MnError};

const CHROME_FOCUS: &str = r#"
const { BrowserWindowTracker } = ChromeUtils.importESModule("resource:///modules/BrowserWindowTracker.sys.mjs");
const win = BrowserWindowTracker.getTopWindow();
if (win) { win.focus(); return true; }
return false;
"#;

pub fn ensure_focus(mn: &mut Marionette, ff: &Firefox, metrics: &mut Metrics) -> bool {
    if dom::eval_bool(mn, "return document.hasFocus();").unwrap_or(false) {
        return true;
    }
    if ff.focus_via_niri() {
        std::thread::sleep(Duration::from_millis(150));
        if dom::eval_bool(mn, "return document.hasFocus();").unwrap_or(false) {
            metrics.focus_recoveries += 1;
            return true;
        }
    }
    let mut ok = false;
    if mn.set_context_chrome().is_ok() {
        ok = mn
            .script(CHROME_FOCUS, serde_json::json!([]))
            .map(|v| v.as_bool().unwrap_or(false))
            .unwrap_or(false);
        let _ = mn.set_context_content();
    }
    if !ok {
        ok = ff.focus_via_niri();
    }
    if ok {
        metrics.focus_recoveries += 1;
        std::thread::sleep(Duration::from_millis(120));
        return dom::eval_bool(mn, "return document.hasFocus();").unwrap_or(false);
    }
    false
}

/// Wait until the page reports focus, retrying focus recovery.
pub fn wait_focus(mn: &mut Marionette, ff: &Firefox, metrics: &mut Metrics) -> bool {
    for _ in 0..12 {
        if dom::eval_bool(mn, "return document.hasFocus();").unwrap_or(false) {
            return true;
        }
        if ensure_focus(mn, ff, metrics) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

pub fn focus_button(mn: &mut Marionette) -> Result<(), MnError> {
    // Room-requested fullscreen lives behind a button labelled Fullscreen.
    if dom::exists(mn, "button[aria-label=Fullscreen]") {
        crate::browser::input::click_selector(mn, "button[aria-label=Fullscreen]")?;
    }
    Ok(())
}
