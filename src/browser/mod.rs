//! Browser control surface: launch, tab management, focus, screenshots.

pub mod dom;
pub mod extension;
pub mod firefox;
pub mod focus;
pub mod input;
pub mod marionette;

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::anticheat::{BlurWatch, Metrics};
use firefox::Firefox;
use marionette::{Marionette, MnError};

#[allow(dead_code)]
pub struct Browser {
    pub mn: Marionette,
    pub ff: Firefox,
    pub handle: String,
    pub data_root: PathBuf,
    blur: BlurWatch,
}

impl Browser {
    pub fn launch(
        profile: Option<&Path>,
        data_root: &Path,
        fullscreen: bool,
    ) -> Result<Self, String> {
        let profile = firefox::profile_dir(profile, data_root).map_err(|e| e.to_string())?;
        let ff = firefox::launch(&profile, data_root, 1280, 860).map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut mn = loop {
            match Marionette::connect(ff.port, Duration::from_secs(60)) {
                Ok(client) => break client,
                Err(e) => {
                    if std::time::Instant::now() > deadline {
                        return Err(format!("Marionette tidak siap: {e}"));
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        };
        mn.new_session()
            .map_err(|e| format!("sesi Marionette: {e}"))?;
        crate::events::file_log("step", "new_session ok");
        let _ = mn.command(
            "WebDriver:SetTimeouts",
            serde_json::json!({"pageLoad": 30_000, "script": 30_000, "implicit": 5_000}),
        );

        // Temporary preload extension sets navigator.webdriver=false.
        match extension::build(data_root) {
            Ok(xpi) => match mn.install_addon(&xpi.to_string_lossy()) {
                Ok(value) => crate::events::file_log("extension", &format!("terpasang {value}")),
                Err(e) => crate::events::file_log("extension", &format!("GAGAL: {e}")),
            },
            Err(e) => crate::events::file_log("extension", &format!("build gagal: {e}")),
        }

        // Reuse the window Firefox already opened: one tab per session, no extra.
        let handle = mn
            .command("WebDriver:GetWindowHandles", serde_json::json!({}))
            .ok()
            .and_then(|value| {
                let list = value
                    .get("value")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .or_else(|| value.as_array().cloned())?;
                list.first().and_then(|v| v.as_str()).map(str::to_string)
            });
        let handle = match handle {
            Some(handle) => handle,
            None => mn.new_tab().map_err(|e| format!("tab baru: {e}"))?,
        };
        mn.switch_window(&handle).map_err(|e| e.to_string())?;
        crate::events::file_log("step", &format!("tab {handle}"));

        // One window only: about:blank and the driver session each open one.
        let mut browser = Self {
            mn,
            ff,
            handle,
            data_root: data_root.to_path_buf(),
            blur: BlurWatch::new(),
        };
        browser.collapse_windows();
        let _ = fullscreen;
        Ok(browser)
    }

    fn current_handle(&mut self) -> String {
        self.mn
            .command("WebDriver:GetWindowHandle", serde_json::json!({}))
            .ok()
            .and_then(|v| {
                v.get("value")
                    .and_then(|x| x.as_str())
                    .map(str::to_string)
                    .or_else(|| v.as_str().map(str::to_string))
            })
            .unwrap_or_else(|| self.handle.clone())
    }

    fn all_handles(&mut self) -> Vec<String> {
        let Ok(value) = self
            .mn
            .command("WebDriver:GetWindowHandles", serde_json::json!({}))
        else {
            return Vec::new();
        };
        let list = value
            .get("value")
            .and_then(|v| v.as_array())
            .cloned()
            .or_else(|| value.as_array().cloned())
            .unwrap_or_default();
        list.iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    }

    /// Keep exactly one window, the one in use.
    pub fn collapse_windows(&mut self) {
        for pass in 0..3 {
            std::thread::sleep(Duration::from_millis(if pass == 0 { 900 } else { 500 }));
            let current = self.current_handle();
            let handles = self.all_handles();
            crate::events::file_log(
                "handles",
                &format!("pass{pass} cur={current} all={handles:?}"),
            );
            if handles.len() <= 1 {
                return;
            }
            for extra in handles.iter().filter(|h| **h != current) {
                let _ = self.mn.switch_window(extra);
                let _ = self
                    .mn
                    .command("WebDriver:CloseWindow", serde_json::json!({}));
                crate::events::file_log("step", &format!("tutup jendela {extra}"));
            }
            let _ = self.mn.switch_window(&current);
        }
    }

    /// Real compositor fullscreen (equivalent to Mod+Shift+F in niri).
    pub fn enter_fullscreen(&mut self) -> bool {
        self.collapse_windows();
        let title = self
            .mn
            .script("return String(document.title);", serde_json::json!([]))
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let closed = self.ff.close_foreign_windows(&title);
        if closed > 0 {
            crate::events::file_log("windows", &format!("tutup {closed} jendela lewat niri"));
        }
        self.ff.focus_via_niri();
        std::thread::sleep(Duration::from_millis(120));
        let mut ok = self.ff.fullscreen_via_niri();
        if !ok {
            let script = r#"
const { BrowserWindowTracker } = ChromeUtils.importESModule("resource:///modules/BrowserWindowTracker.sys.mjs");
const win = BrowserWindowTracker.getTopWindow();
if (!win) return false;
win.fullscreen = true;
return !!win.fullscreen;
"#;
            if self.mn.set_context_chrome().is_ok() {
                ok = self
                    .mn
                    .script(script, serde_json::json!([]))
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let _ = self.mn.set_context_content();
            }
        }
        std::thread::sleep(Duration::from_millis(900));
        crate::events::file_log(
            "fullscreen",
            &format!("niri={ok} window={}", self.ff.window_snapshot()),
        );
        ok
    }

    /// Reattach to the live Firefox without opening another tab/window.
    pub fn reconnect(&mut self) -> Result<(), String> {
        let text = std::fs::read_to_string(self.data_root.join("browser.json"))
            .map_err(|e| format!("state browser: {e}"))?;
        let port = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("port").and_then(|p| p.as_u64()))
            .ok_or("port tidak diketahui")? as u16;
        let mut mn = Marionette::connect(port, Duration::from_secs(20))
            .map_err(|e| format!("reconnect: {e}"))?;
        mn.new_session().map_err(|e| format!("sesi baru: {e}"))?;
        let _ = mn.command(
            "WebDriver:SetTimeouts",
            serde_json::json!({"pageLoad": 30_000, "script": 30_000, "implicit": 5_000}),
        );
        let handles = {
            let out = mn
                .command("WebDriver:GetWindowHandles", serde_json::json!({}))
                .ok();
            out.and_then(|v| {
                let list = v
                    .get("value")
                    .and_then(|x| x.as_array())
                    .cloned()
                    .or_else(|| v.as_array().cloned())?;
                Some(
                    list.iter()
                        .filter_map(|h| h.as_str().map(str::to_string))
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default()
        };
        // Pick the window that already shows Wayground.
        let mut keep: Option<String> = None;
        for handle in &handles {
            let _ = mn.switch_window(handle);
            let url = mn
                .script(
                    "return String(document.location.href);",
                    serde_json::json!([]),
                )
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            if url.contains("wayground.com") {
                keep = Some(handle.clone());
                break;
            }
        }
        if let Some(handle) = keep.clone().or_else(|| handles.first().cloned()) {
            let _ = mn.switch_window(&handle);
        }
        self.mn = mn;
        // Keep one window; the driver's fresh session window may still be open.
        let title = self
            .mn
            .script("return String(document.title);", serde_json::json!([]))
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let closed = self.ff.close_foreign_windows(&title);
        crate::events::file_log("reconnect", &format!("ok, tutup {closed} jendela"));
        Ok(())
    }

    pub fn current_url(&mut self) -> String {
        self.mn
            .script(
                "return String(document.location.href);",
                serde_json::json!([]),
            )
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), MnError> {
        crate::events::file_log("nav", &format!("mulai {url}"));
        self.mn.set_timeout(Duration::from_secs(15));
        let first = self.mn.navigate(url);
        self.mn.set_timeout(Duration::from_secs(60));
        // A load timeout is not fatal as long as the document is the right origin.
        let here = self
            .mn
            .script("return document.location.href;", serde_json::json!([]))
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        crate::events::file_log("nav", &format!("selesai {:?} di {here}", first.is_err()));
        if here.starts_with(url) || here.contains("wayground.com") {
            return Ok(());
        }
        self.mn.script(
            "document.location.href = arguments[0]; return true;",
            serde_json::json!([url]),
        )?;
        std::thread::sleep(Duration::from_secs(3));
        Ok(())
    }

    /// Focus check + recovery, feeding blur metrics.
    pub fn tick_focus(&mut self, metrics: &mut Metrics, manual: bool) -> bool {
        let focused = dom::eval_bool(&mut self.mn, "return document.hasFocus();").unwrap_or(false);
        if let Some(ms) = self.blur.update(focused, metrics, manual)
            && ms > 2500
        {
            focus::ensure_focus(&mut self.mn, &self.ff, metrics);
        }
        focused
    }

    pub fn ensure_focus(&mut self, metrics: &mut Metrics) -> bool {
        focus::wait_focus(&mut self.mn, &self.ff, metrics)
    }

    pub fn screenshot(&mut self, label: &str, store: &crate::store::Store) {
        if let Ok(bytes) = self.mn.screenshot() {
            let _ = store.save_screenshot(label, &bytes);
        }
    }

    /// Full viewport screenshot, native size, lossless PNG (quality 100).
    pub fn screenshot_data_url(&mut self) -> Option<String> {
        let bytes = self.mn.screenshot().ok()?;
        Some(crate::solver::deepseek::data_url("image/png", &bytes))
    }

    pub fn webdriver_flag(&mut self) -> Option<bool> {
        dom::eval_bool(&mut self.mn, "return navigator.webdriver === false;").ok()
    }

    /// In-page fetch returning status and body even on failure.
    pub fn fetch_raw(
        &mut self,
        url: &str,
        method: &str,
        body: Option<&str>,
    ) -> Option<(u16, String)> {
        let script = r#"
const url = arguments[0], method = arguments[1], body = arguments[2];
const done = arguments[arguments.length - 1];
(async () => {
  try {
    const init = { method, credentials: 'include', headers: { 'content-type': 'application/json' } };
    if (body) init.body = body;
    const r = await fetch(url, init);
    const txt = await r.text();
    done({ status: r.status, text: txt });
  } catch (e) { done({ status: 0, text: String(e) }); }
})();
"#;
        let result = self
            .mn
            .async_script(script, serde_json::json!([url, method, body]))
            .ok()?;
        let status = result.get("status").and_then(|s| s.as_u64()).unwrap_or(0) as u16;
        let text = result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        Some((status, text))
    }

    /// In-page fetch, serialised as JSON. Runs with the page's own cookies.
    pub fn fetch_json(
        &mut self,
        url: &str,
        method: &str,
        body: Option<&str>,
    ) -> Option<serde_json::Value> {
        let script = r#"
const url = arguments[0], method = arguments[1], body = arguments[2];
const done = arguments[arguments.length - 1];
(async () => {
  try {
    const init = { method, credentials: 'include', headers: { 'content-type': 'application/json' } };
    if (body) init.body = body;
    const r = await fetch(url, init);
    const txt = await r.text();
    done({ status: r.status, text: txt });
  } catch (e) { done({ status: 0, text: String(e) }); }
})();
"#;
        let result = self
            .mn
            .async_script(script, serde_json::json!([url, method, body]))
            .ok()?;
        let status = result.get("status").and_then(|s| s.as_u64()).unwrap_or(0);
        let text = result.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if status >= 400 || text.is_empty() {
            return None;
        }
        serde_json::from_str(text).ok()
    }
}
