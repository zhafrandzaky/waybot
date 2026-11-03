//! Anti-cheat accounting: pacing budget and detection metrics.

pub mod pacing;

use std::time::Instant;

#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct Metrics {
    pub webdriver_ok: Option<bool>,
    pub blur_count: u32,
    pub blur_total_ms: u64,
    pub manual_blur: u32,
    pub resize_count: u32,
    pub paste_count: u32,
    pub fullscreen_entered: bool,
    pub fullscreen_preserved: bool,
    pub focus_recoveries: u32,
    pub untrusted_events: u32,
}

impl Metrics {
    pub fn record_blur(&mut self, manual: bool) {
        self.blur_count += 1;
        if manual {
            self.manual_blur += 1;
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "webdriver={} blur={} (manual={}) blur_total={}ms resize={} paste={} fokus_pulih={}",
            match self.webdriver_ok {
                Some(true) => "false",
                Some(false) => "true",
                None => "?",
            },
            self.blur_count,
            self.manual_blur,
            self.blur_total_ms,
            self.resize_count,
            self.paste_count,
            self.focus_recoveries
        )
    }
}

#[derive(Debug, Clone)]
pub struct BlurWatch {
    started: Option<Instant>,
}

impl Default for BlurWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl BlurWatch {
    pub fn new() -> Self {
        Self { started: None }
    }

    /// Returns the completed blur duration when focus comes back.
    pub fn update(&mut self, focused: bool, metrics: &mut Metrics, manual: bool) -> Option<u64> {
        match (focused, self.started) {
            (true, Some(start)) => {
                let ms = start.elapsed().as_millis() as u64;
                metrics.blur_total_ms += ms;
                self.started = None;
                Some(ms)
            }
            (false, None) => {
                self.started = Some(Instant::now());
                metrics.record_blur(manual);
                None
            }
            _ => None,
        }
    }
}
