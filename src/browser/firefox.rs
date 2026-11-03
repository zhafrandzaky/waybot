//! Firefox process launch with a dedicated profile and Marionette port.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("firefox gagal dijalankan: {0}")]
    Spawn(String),
    #[error("Marionette tidak siap dalam {0:?}")]
    NotReady(Duration),
    #[error("io: {0}")]
    Io(String),
}

pub struct Firefox {
    child: Child,
    pub port: u16,
    pub profile: PathBuf,
    _tmp_profile: bool,
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(2828)
}

pub fn profile_dir(requested: Option<&Path>, data_root: &Path) -> std::io::Result<PathBuf> {
    if let Some(path) = requested {
        fs::create_dir_all(path)?;
        return Ok(path.to_path_buf());
    }
    // Persistent profile: stable uid/cookies, no first-run dialogs on every run.
    let dir = data_root.join("profile");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write_prefs(profile: &Path, port: u16, width: u32, height: u32) -> std::io::Result<()> {
    let prefs = format!(
        r#"user_pref("marionette.port", {port});
user_pref("browser.startup.page", 0);
user_pref("browser.startup.homepage", "about:blank");
user_pref("browser.newtabpage.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.tabs.warnOnClose", false);
user_pref("browser.warnOnQuit", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("datareporting.healthreport.uploadEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("toolkit.telemetry.enabled", false);
user_pref("extensions.autoDisableScopes", 0);
user_pref("xpinstall.signatures.required", false);
user_pref("extensions.webextensions.remote", true);
user_pref("extensions.enabledScopes", 15);
user_pref("signon.rememberSignons", false);
user_pref("ui.key.menuAccessKeyFocuses", false);
user_pref("browser.uidensity", 0);
user_pref("layout.css.devPixelsPerPx", "1.0");
user_pref("browser.window.width", {width});
user_pref("browser.window.height", {height});
"#
    );
    fs::write(profile.join("user.js"), prefs)
}

fn pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Kill a leftover Firefox that still holds our profile.
fn kill_stale(profile: &Path, data_root: &Path) {
    let state = data_root.join("browser.json");
    if let Ok(text) = fs::read_to_string(&state)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(pid) = value.get("pid").and_then(|p| p.as_u64())
                && pid_alive(pid as u32) {
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                    std::thread::sleep(Duration::from_millis(600));
                }
    let needle = profile.to_string_lossy().to_string();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            let Ok(cmdline) = fs::read(format!("/proc/{pid}/cmdline")) else {
                continue;
            };
            let cmd = String::from_utf8_lossy(&cmdline);
            if cmd.contains("firefox") && cmd.contains(&needle) {
                let _ = Command::new("kill").arg(pid.to_string()).status();
            }
        }
    }
    let _ = fs::remove_file(profile.join("lock"));
    let _ = fs::remove_file(profile.join(".parentlock"));
    std::thread::sleep(Duration::from_millis(400));
}

pub fn launch(
    profile: &Path,
    data_root: &Path,
    width: u32,
    height: u32,
) -> Result<Firefox, LaunchError> {
    kill_stale(profile, data_root);
    let port = free_port();
    write_prefs(profile, port, width, height).map_err(|e| LaunchError::Io(e.to_string()))?;

    let child = Command::new("firefox")
        .arg("--marionette")
        .arg("--remote-allow-system-access")
        .arg("--new-instance")
        .arg("--no-remote")
        .arg("--profile")
        .arg(profile)
        .arg("--width")
        .arg(width.to_string())
        .arg("--height")
        .arg(height.to_string())
        .env("MOZ_CRASHREPORTER_DISABLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(data_root.join("firefox.err")).unwrap_or_else(|_| {
                std::fs::File::create("/tmp/opencode/firefox.err").expect("stderr")
            }),
        ))
        .spawn()
        .map_err(|e| LaunchError::Spawn(e.to_string()))?;

    let _ = fs::write(
        data_root.join("browser.json"),
        serde_json::json!({"pid": child.id(), "port": port}).to_string(),
    );
    Ok(Firefox {
        child,
        port,
        profile: profile.to_path_buf(),
        _tmp_profile: false,
    })
}

#[allow(dead_code)]
impl Firefox {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn window_snapshot(&self) -> String {
        let Some(id) = self.niri_window_id() else {
            return "?".into();
        };
        let output = Command::new("niri")
            .args(["msg", "--json", "windows"])
            .output();
        let Ok(output) = output else {
            return "?".into();
        };
        let Ok(text) = String::from_utf8(output.stdout) else {
            return "?".into();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return "?".into();
        };
        let Some(list) = value.as_array() else {
            return "?".into();
        };
        list.iter()
            .find(|w| w.get("id").and_then(|i| i.as_u64()) == Some(id))
            .map(|w| {
                format!(
                    "tile={:?} size={:?}",
                    w.pointer("/layout/tile_size"),
                    w.pointer("/layout/window_size")
                )
            })
            .unwrap_or_else(|| "?".into())
    }

    /// Close windows of our own process that are not the title we control.
    pub fn close_foreign_windows(&self, keep_title: &str) -> usize {
        let output = Command::new("niri")
            .args(["msg", "--json", "windows"])
            .output();
        let Ok(output) = output else { return 0 };
        let Ok(text) = String::from_utf8(output.stdout) else {
            return 0;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return 0;
        };
        let Some(list) = value.as_array() else {
            return 0;
        };
        let is_wayground = |w: &&serde_json::Value| {
            w.get("title")
                .and_then(|t| t.as_str())
                .map(|t| t.contains("Wayground") || t.contains("Quizizz"))
                .unwrap_or(false)
        };
        let mut mine: Vec<&serde_json::Value> = list
            .iter()
            .filter(|w| w.get("app_id").and_then(|a| a.as_str()) == Some("firefox"))
            .filter(is_wayground)
            .collect();
        // Only consider windows of the launcher's process when it is identifiable.
        let same_pid: Vec<&serde_json::Value> = mine
            .iter()
            .copied()
            .filter(|w| w.get("pid").and_then(|p| p.as_u64()) == Some(self.pid() as u64))
            .collect();
        if same_pid.len() > 1 {
            mine = same_pid;
        }
        if mine.len() <= 1 {
            return 0;
        }
        let keep = mine
            .iter()
            .find(|w| w.get("title").and_then(|t| t.as_str()) == Some(keep_title))
            .or_else(|| {
                mine.iter()
                    .find(|w| w.get("is_focused").and_then(|f| f.as_bool()) == Some(true))
            })
            .and_then(|w| w.get("id"))
            .and_then(|i| i.as_u64());
        let Some(keep) = keep else { return 0 };
        let mut closed = 0;
        for w in mine {
            let Some(id) = w.get("id").and_then(|i| i.as_u64()) else {
                continue;
            };
            if id == keep {
                continue;
            }
            let _ = Command::new("niri")
                .args(["msg", "action", "close-window", "--id", &id.to_string()])
                .status();
            closed += 1;
        }
        std::thread::sleep(Duration::from_millis(500));
        closed
    }

    pub fn fullscreen_via_niri(&self) -> bool {
        let Some(id) = self.niri_window_id() else {
            return false;
        };
        Command::new("niri")
            .args([
                "msg",
                "action",
                "fullscreen-window",
                "--id",
                &id.to_string(),
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn niri_window_id(&self) -> Option<u64> {
        let output = Command::new("niri")
            .args(["msg", "--json", "windows"])
            .output()
            .ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
        let windows = value.as_array()?;
        windows
            .iter()
            .find(|w| {
                w.get("app_id").and_then(|a| a.as_str()) == Some("firefox")
                    && w.get("pid").and_then(|p| p.as_u64()) == Some(self.pid() as u64)
            })
            .or_else(|| {
                windows
                    .iter()
                    .find(|w| w.get("app_id").and_then(|a| a.as_str()) == Some("firefox"))
            })
            .and_then(|w| w.get("id"))
            .and_then(|i| i.as_u64())
    }

    pub fn focus_via_niri(&self) -> bool {
        let Some(id) = self.niri_window_id() else {
            return false;
        };
        Command::new("niri")
            .args(["msg", "action", "focus-window", "--id", &id.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

impl Drop for Firefox {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
