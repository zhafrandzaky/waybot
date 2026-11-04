//! Configuration: `.env` defaults, CLI overrides, validation.

use std::path::PathBuf;

use clap::Parser;

use crate::util::{parse_duration, valid_display_name, valid_room_code};

#[derive(Debug, Parser)]
#[command(
    name = "waybot",
    version,
    about = "Otomatisasi sesi kuis Wayground lewat Firefox nyata"
)]
pub struct Cli {
    /// Kode room (3-11 alfanumerik). Melewati layar Setup.
    #[arg(long)]
    pub room: Option<String>,
    /// Nama tampilan pemain. Melewati layar Setup.
    #[arg(long)]
    pub name: Option<String>,
    /// Durasi total `mm:ss`. Melewati layar Setup.
    #[arg(long)]
    pub duration: Option<String>,
    /// Model solver (menimpa DEEPSEEK_MODEL).
    #[arg(long)]
    pub model: Option<String>,
    /// Base URL OpenAI-compatible (menimpa DEEPSEEK_BASE_URL).
    #[arg(long = "base-url")]
    pub base_url: Option<String>,
    /// auto | on | off
    #[arg(long)]
    pub thinking: Option<String>,
    /// reasoning effort: minimal | low | medium | high
    #[arg(long = "reasoning-effort")]
    pub reasoning_effort: Option<String>,
    /// Aktifkan pembacaan gambar lewat model vision.
    #[arg(long)]
    pub vision: bool,
    /// Matikan vision (menang atas .env).
    #[arg(long = "no-vision")]
    pub no_vision: bool,
    /// auto | off | path ke biner OCR
    #[arg(long)]
    pub ocr: Option<String>,
    /// Minta fullscreen bila room memintanya (default aktif).
    #[arg(long)]
    pub fullscreen: bool,
    /// Jangan masuk fullscreen.
    #[arg(long = "no-fullscreen")]
    pub no_fullscreen: bool,
    /// Direktori profil Firefox.
    #[arg(long)]
    pub profile: Option<String>,
    /// Jeda minimum antar soal, detik.
    #[arg(long = "jeda-min")]
    pub jeda_min: Option<f64>,
    /// Jeda maksimum antar soal, detik.
    #[arg(long = "jeda-maks")]
    pub jeda_maks: Option<f64>,
    /// Lewati layar konfirmasi (butuh room, nama, durasi).
    #[arg(long)]
    pub yes: bool,
    /// Lanjutkan sesi room yang sama bila masih segar (default: sesi baru).
    #[arg(long)]
    pub resume: bool,
    /// Matikan agen halaman AI (default aktif sebagai cadangan).
    #[arg(long = "no-agent")]
    pub no_agent: bool,
    /// Batas menunggu host, menit (0 = tanpa batas).
    #[arg(long = "wait-max")]
    pub wait_max: Option<u64>,
    /// Cetak konfigurasi efektif lalu keluar.
    #[arg(long = "print-config")]
    pub print_config: bool,
}

pub fn parse_args() -> Cli {
    Cli::parse()
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub vision_model: String,
    pub thinking: Thinking,
    pub reasoning_effort: Option<String>,
    pub temperature: f32,
    pub profile: Option<PathBuf>,
    pub ocr: OcrMode,
    pub vision: bool,
    pub resume: bool,
    pub agent: bool,
    pub wait_max_min: u64,
    pub jeda_min_ms: u64,
    pub jeda_maks_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thinking {
    Auto,
    On,
    Off,
}

impl Thinking {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Some(Self::Auto),
            "on" | "true" | "1" => Some(Self::On),
            "off" | "false" | "0" => Some(Self::Off),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrMode {
    Auto,
    Off,
    Path(String),
}

impl OcrMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Self::Auto,
            "off" | "none" => Self::Off,
            other => Self::Path(other.to_string()),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Auto => "auto".into(),
            Self::Off => "off".into(),
            Self::Path(p) => p.clone(),
        }
    }
}

impl AppConfig {
    pub fn load(cli: &Cli) -> Result<Self, String> {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default();
        let base_url = cli
            .base_url
            .clone()
            .or_else(|| std::env::var("DEEPSEEK_BASE_URL").ok())
            .unwrap_or_else(|| "https://api.venice.ai/api/v1".to_string());
        let model = cli
            .model
            .clone()
            .or_else(|| std::env::var("DEEPSEEK_MODEL").ok())
            .unwrap_or_else(|| "deepseek-v4-1-flash-1017-uncensored".to_string());
        let vision_model = std::env::var("DEEPSEEK_VISION_MODEL").unwrap_or_default();
        let thinking = cli
            .thinking
            .clone()
            .or_else(|| std::env::var("DEEPSEEK_THINKING").ok())
            .and_then(|v| Thinking::parse(&v))
            .unwrap_or(Thinking::Auto);
        let reasoning_effort = cli
            .reasoning_effort
            .clone()
            .or_else(|| std::env::var("DEEPSEEK_REASONING_EFFORT").ok())
            .filter(|v| !v.trim().is_empty());
        let temperature = std::env::var("DEEPSEEK_TEMPERATURE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.2)
            .clamp(0.0, 1.0);
        let profile = cli
            .profile
            .clone()
            .or_else(|| std::env::var("WAYBOT_PROFILE").ok())
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);
        let ocr = OcrMode::parse(
            cli.ocr
                .clone()
                .or_else(|| std::env::var("WAYBOT_OCR").ok())
                .as_deref()
                .unwrap_or("auto"),
        );
        let vision = if cli.no_vision {
            false
        } else if cli.vision {
            true
        } else {
            std::env::var("WAYBOT_VISION")
                .map(|v| {
                    !matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "0" | "false" | "off"
                    )
                })
                .unwrap_or(!vision_model.is_empty())
        };
        let wait_max_min = cli
            .wait_max
            .or_else(|| {
                std::env::var("WAYBOT_WAIT_MAX_MIN")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(0);
        let jeda_min_ms = (cli
            .jeda_min
            .or_else(|| {
                std::env::var("WAYBOT_JEDA_MIN")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(2.5)
            .clamp(1.0, 15.0)
            * 1000.0) as u64;
        let jeda_maks_ms = (cli
            .jeda_maks
            .or_else(|| {
                std::env::var("WAYBOT_JEDA_MAKS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(8.0)
            .clamp(1.0, 15.0)
            * 1000.0) as u64;
        if jeda_min_ms > jeda_maks_ms {
            return Err("jeda minimum lebih besar dari jeda maksimum".into());
        }
        if base_url.trim().is_empty() {
            return Err("base URL solver kosong".into());
        }
        if model.trim().is_empty() {
            return Err("model solver kosong".into());
        }
        Ok(Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            vision_model,
            thinking,
            reasoning_effort,
            temperature,
            profile,
            ocr,
            vision,
            resume: cli.resume,
            agent: !cli.no_agent,
            wait_max_min,
            jeda_min_ms,
            jeda_maks_ms,
        })
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub fn key(&self) -> String {
        self.api_key.clone()
    }
}

/// Values typed at the Setup screen (or supplied by flags).
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub room: String,
    pub name: String,
    pub duration_ms: u64,
    pub model: String,
    pub base_url: String,
    pub thinking: Thinking,
    pub reasoning_effort: Option<String>,
    pub vision: bool,
    pub ocr: OcrMode,
    pub fullscreen: bool,
    pub profile: Option<PathBuf>,
    pub jeda_min_ms: u64,
    pub jeda_maks_ms: u64,
}

impl SessionConfig {
    pub fn from_cli(cli: &Cli, app: &AppConfig) -> Option<Self> {
        let room = cli.room.clone()?;
        let name = cli.name.clone()?;
        let duration_ms = parse_duration(cli.duration.as_deref()?)?;
        Some(Self {
            room,
            name,
            duration_ms,
            model: app.model.clone(),
            base_url: app.base_url.clone(),
            thinking: app.thinking,
            reasoning_effort: app.reasoning_effort.clone(),
            vision: app.vision,
            ocr: app.ocr.clone(),
            fullscreen: !cli.no_fullscreen,
            profile: app.profile.clone(),
            jeda_min_ms: app.jeda_min_ms,
            jeda_maks_ms: app.jeda_maks_ms,
        })
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !valid_room_code(&self.room) {
            errors.push("kode room harus 3-11 karakter alfanumerik".into());
        }
        if !valid_display_name(&self.name) {
            errors.push("nama harus 1-60 karakter tanpa karakter kontrol".into());
        }
        if !(30_000..=3_600_000).contains(&self.duration_ms) {
            errors.push("durasi harus antara 00:30 dan 60:00".into());
        }
        if self.model.trim().is_empty() {
            errors.push("model wajib dipilih".into());
        }
        if self.jeda_min_ms > self.jeda_maks_ms {
            errors.push("jeda minimum > jeda maksimum".into());
        }
        errors
    }

    pub fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SessionConfig {
        SessionConfig {
            room: "794275".into(),
            name: "Budi".into(),
            duration_ms: 180_000,
            model: "m".into(),
            base_url: "https://example.test/v1".into(),
            thinking: Thinking::Auto,
            reasoning_effort: None,
            vision: false,
            ocr: OcrMode::Auto,
            fullscreen: false,
            profile: None,
            jeda_min_ms: 2500,
            jeda_maks_ms: 8000,
        }
    }

    #[test]
    fn valid_config_passes() {
        assert!(base().is_valid());
    }

    #[test]
    fn bad_room_and_duration_fail() {
        let mut c = base();
        c.room = "7".into();
        c.duration_ms = 10_000;
        let errs = c.validate();
        assert_eq!(errs.len(), 2);
    }

    #[test]
    fn thinking_parsing() {
        assert_eq!(Thinking::parse("off"), Some(Thinking::Off));
        assert_eq!(Thinking::parse("ON"), Some(Thinking::On));
        assert_eq!(Thinking::parse("nope"), None);
    }
}
