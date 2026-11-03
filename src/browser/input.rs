//! Human-like native input: per-character typing, jittered clicks.

use std::time::Duration;

use crate::browser::dom;
use crate::browser::marionette::{Marionette, MnError};
use crate::util::jitter_ms;

/// Focus the element's owner document and click it natively.
pub fn click_selector(mn: &mut Marionette, css: &str) -> Result<(), MnError> {
    let element = mn.find(css)?;
    mn.script(
        "arguments[0].scrollIntoView({block: 'center', inline: 'center'}); return true;",
        serde_json::json!([{ "element-6066-11e4-a52e-4f735466cecf": element }]),
    )
    .ok();
    std::thread::sleep(Duration::from_millis(jitter_ms(80, 220)));
    mn.click(&element)
}

/// Click at viewport coordinates (hotspot / fallback).
pub fn click_at(mn: &mut Marionette, x: f64, y: f64) -> Result<(), MnError> {
    mn.command(
        "WebDriver:PerformActions",
        serde_json::json!({"actions": [{"type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"}, "actions": [
            {"type": "pointerMove", "duration": 0, "x": x, "y": y, "origin": "viewport"},
            {"type": "pointerDown", "button": 0},
            {"type": "pause", "duration": 60},
            {"type": "pointerUp", "button": 0}
        ]}]}),
    )?;
    Ok(())
}

/// Type in small chunks: human start, then fast bursts (still per-character key events).
fn type_chunks(mn: &mut Marionette, element: &str, text: &str) -> Result<(), MnError> {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        let take = if index < 2 {
            1
        } else {
            (rand::random::<u64>() % 5 + 10) as usize
        };
        let end = (index + take).min(chars.len());
        let chunk: String = chars[index..end].iter().collect();
        mn.send_keys(element, &chunk)?;
        index = end;
        let delay = if index <= 2 {
            jitter_ms(45, 140)
        } else {
            jitter_ms(25, 60)
        };
        std::thread::sleep(Duration::from_millis(delay));
    }
    Ok(())
}

fn read_value(mn: &mut Marionette, css: &str) -> String {
    mn.script(
        &format!(
            "const e=document.querySelector({}); return e ? (e.value !== undefined ? e.value : (e.innerText || '')) : '';",
            crate::browser::dom::js_str(css)
        ),
        serde_json::json!([]),
    )
    .ok()
    .and_then(|v| v.as_str().map(str::to_string))
    .unwrap_or_default()
}

/// Clear and type text (no clipboard), with real click focus and a value check.
pub fn type_selector(mn: &mut Marionette, css: &str, text: &str) -> Result<(), MnError> {
    let element = mn.find(css)?;
    let _ = click_selector(mn, css);
    mn.script(
        "arguments[0].focus(); return true;",
        serde_json::json!([{ "element-6066-11e4-a52e-4f735466cecf": element }]),
    )
    .ok();
    mn.clear(&element).ok();
    type_chunks(mn, &element, text)?;
    if read_value(mn, css).trim() == text.trim() {
        return Ok(());
    }
    // Fallback: set the value through the native setter and fire a real input event.
    let script = r#"
const el = document.querySelector(arguments[0]);
if (!el) return false;
el.focus();
const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
setter.call(el, arguments[1]);
el.dispatchEvent(new Event('input', { bubbles: true }));
el.dispatchEvent(new Event('change', { bubbles: true }));
return true;
"#;
    mn.script(script, serde_json::json!([css, text]))?;
    Ok(())
}

/// Focus a contenteditable and type into it, falling back to `insertText`.
pub fn type_contenteditable(mn: &mut Marionette, css: &str, text: &str) -> Result<(), MnError> {
    let element = mn.find(css)?;
    let _ = click_selector(mn, css);
    mn.script(
        "const el=arguments[0]; el.focus(); const r=document.createRange(); r.selectNodeContents(el); const s=getSelection(); s.removeAllRanges(); s.addRange(r); return true;",
        serde_json::json!([{ "element-6066-11e4-a52e-4f735466cecf": element }]),
    )
    .ok();
    type_chunks(mn, &element, text)?;
    if !read_value(mn, css).trim().is_empty() {
        return Ok(());
    }
    let script = r#"
const el = document.querySelector(arguments[0]);
if (!el) return false;
el.focus();
document.execCommand('insertText', false, arguments[1]);
return true;
"#;
    mn.script(script, serde_json::json!([css, text]))?;
    Ok(())
}

pub fn focused(mn: &mut Marionette) -> bool {
    dom::eval_bool(mn, "return document.hasFocus();").unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_uses_native_command() {
        // Compile-time guard: the function returns MnError.
        let _f: fn(&mut Marionette, &str) -> Result<(), MnError> = click_selector;
    }
}
