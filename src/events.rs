//! Events from the session worker to the UI.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Setup,
    Confirming,
    Launching,
    Joining,
    Harvesting,
    WaitingStart,
    Answering,
    Finishing,
    Done,
    Failed,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::Confirming => "Konfirmasi",
            Self::Launching => "Membuka Firefox",
            Self::Joining => "Join room",
            Self::Harvesting => "Mengambil soal",
            Self::WaitingStart => "Menunggu host",
            Self::Answering => "Menjawab",
            Self::Finishing => "Menyelesaikan",
            Self::Done => "Selesai",
            Self::Failed => "Gagal",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub index: u32,
    pub total: u32,
    pub question: String,
    pub answer: String,
    pub source: String,
    pub status: String,
    pub remaining_ms: u64,
    pub mode: String,
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub answered: u32,
    pub total: u32,
    pub correct: u32,
    pub unknown: u32,
    pub elapsed_ms: u64,
    pub metrics: String,
    pub session_dir: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    Log(Level, String),
    Phase(Phase),
    Joined,
    QuestionsReady {
        total: u32,
        keyed: u32,
        explained: u32,
        ai: u32,
        low: u32,
    },
    Ready,
    Started {
        number: u32,
        total: u32,
    },
    Progress(Progress),
    Metrics(String),
    ModelCheck(Result<Vec<String>, String>),
    Finished(Summary),
    Failed(String),
}

pub type Tx = std::sync::mpsc::Sender<Event>;

static LOG_FILE: std::sync::OnceLock<std::sync::Mutex<Option<std::fs::File>>> =
    std::sync::OnceLock::new();

/// Mirror every event into a plain file so headless runs stay inspectable.
pub fn set_log_file(path: &std::path::Path) {
    let file = std::fs::File::create(path).ok();
    let cell = LOG_FILE.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut slot) = cell.lock() {
        *slot = file;
    }
}

pub fn file_log(kind: &str, message: &str) {
    use std::io::Write;
    if let Some(cell) = LOG_FILE.get()
        && let Ok(mut slot) = cell.lock()
        && let Some(file) = slot.as_mut()
    {
        let _ = writeln!(file, "{} {kind} {message}", crate::store::now_ms());
    }
}

pub fn log(tx: &Tx, level: Level, message: impl Into<String>) {
    let message = message.into();
    file_log(
        "log",
        &format!(
            "{} {}",
            match level {
                Level::Info => "INFO",
                Level::Warn => "WARN",
                Level::Error => "ERR ",
            },
            message
        ),
    );
    let _ = tx.send(Event::Log(level, message));
}

pub fn phase(tx: &Tx, phase: Phase) {
    file_log("phase", phase.label());
    let _ = tx.send(Event::Phase(phase));
}
