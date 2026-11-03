//! Full-screen TUI. Keyboard only; input is locked while the quiz runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crossterm::event::{self, Event as Key, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap};

use crate::config::{AppConfig, Cli, OcrMode, SessionConfig, Thinking};
use crate::events::{Event, Level, Phase, Progress, Summary};
use crate::util::{fmt_duration, parse_duration, valid_display_name, valid_room_code};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Setup,
    Confirm,
    Lobby,
    Quiz,
    Summary,
}

#[derive(Default)]
struct AdvFields {
    base_url: String,
    model: String,
    effort: String,
    ocr: String,
    profile: String,
    jeda_min: String,
    jeda_maks: String,
}

struct App {
    screen: Screen,
    focus: usize,
    advanced_open: bool,
    room: String,
    name: String,
    duration: String,
    adv: AdvFields,
    thinking: Thinking,
    vision: bool,
    fullscreen: bool,
    errors: Vec<String>,
    app_cfg: AppConfig,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    abort: Arc<AtomicBool>,
    phase: Phase,
    logs: Vec<(Level, String)>,
    progress: Option<Progress>,
    questions: Option<(u32, u32, u32, u32, u32)>,
    metrics: String,
    summary: Option<Summary>,
    abort_confirm: bool,
    escape_armed: bool,
    no_color: bool,
    model_checking: bool,
    pending: Option<SessionConfig>,
}

const FIELDS: usize = 13;

pub fn run(cli: Cli, app_cfg: AppConfig) -> std::io::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App {
        screen: Screen::Setup,
        focus: 0,
        advanced_open: false,
        room: cli.room.clone().unwrap_or_default(),
        name: cli.name.clone().unwrap_or_default(),
        duration: cli.duration.clone().unwrap_or_default(),
        adv: AdvFields {
            base_url: app_cfg.base_url.clone(),
            model: app_cfg.model.clone(),
            effort: app_cfg.reasoning_effort.clone().unwrap_or_default(),
            ocr: app_cfg.ocr.describe(),
            profile: app_cfg
                .profile
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            jeda_min: format!("{:.1}", app_cfg.jeda_min_ms as f64 / 1000.0),
            jeda_maks: format!("{:.1}", app_cfg.jeda_maks_ms as f64 / 1000.0),
        },
        thinking: app_cfg.thinking,
        vision: app_cfg.vision,
        fullscreen: !cli.no_fullscreen,
        errors: Vec::new(),
        app_cfg: app_cfg.clone(),
        tx,
        rx,
        abort: Arc::new(AtomicBool::new(false)),
        phase: Phase::Setup,
        logs: Vec::new(),
        progress: None,
        questions: None,
        metrics: String::new(),
        summary: None,
        abort_confirm: false,
        escape_armed: false,
        no_color: std::env::var_os("NO_COLOR").is_some(),
        model_checking: false,
        pending: None,
    };

    // Flag-driven start: skip the setup screen when everything needed is present.
    if let Some(cfg) = SessionConfig::from_cli(&cli, &app_cfg) {
        let errors = cfg.validate();
        if errors.is_empty() {
            if cli.yes {
                app.start(cfg);
            } else {
                app.pending = Some(cfg);
                app.screen = Screen::Confirm;
            }
        } else {
            app.errors = errors;
        }
    }

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, &cli);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    cli: &Cli,
) -> std::io::Result<()> {
    loop {
        app.drain_events();
        terminal.draw(|frame| ui(frame, app))?;
        if app.screen == Screen::Summary {
            if event::poll(Duration::from_millis(250))?
                && let Key::Key(KeyEvent {
                    code,
                    kind: KeyEventKind::Press,
                    ..
                }) = event::read()?
            {
                match code {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        app.reset();
                    }
                    _ => {}
                }
            }
            continue;
        }
        if !event::poll(Duration::from_millis(120))? {
            continue;
        }
        let Key::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            match app.screen {
                Screen::Quiz | Screen::Lobby => app.abort_confirm = true,
                _ => return Ok(()),
            }
            continue;
        }
        if app.abort_confirm {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    app.abort.store(true, Ordering::Relaxed);
                    app.abort_confirm = false;
                    app.log(Level::Warn, "permintaan batal dikirim");
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.abort_confirm = false,
                _ => {}
            }
            continue;
        }
        match app.screen {
            Screen::Setup => app.setup_key(key, cli),
            Screen::Confirm => app.confirm_key(key),
            Screen::Lobby | Screen::Quiz => {
                if key.code == KeyCode::F(2) {
                    // Log panel is always visible in the dashboard.
                }
            }
            Screen::Summary => {}
        }
        if app.screen == Screen::Summary {
            continue;
        }
    }
}

impl App {
    fn log(&mut self, level: Level, message: impl Into<String>) {
        self.logs.push((level, message.into()));
        if self.logs.len() > 400 {
            self.logs.remove(0);
        }
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Log(level, message) => self.log(level, message),
                Event::Phase(phase) => {
                    self.phase = phase;
                    if phase == Phase::Answering {
                        self.screen = Screen::Quiz;
                    }
                    if phase == Phase::Done {
                        self.screen = Screen::Summary;
                    }
                }
                Event::Joined => self.log(Level::Info, "bergabung ke room"),
                Event::QuestionsReady {
                    total,
                    keyed,
                    explained,
                    ai,
                    low,
                } => {
                    self.questions = Some((total, keyed, explained, ai, low));
                }
                Event::Ready => {
                    self.screen = Screen::Lobby;
                    self.phase = Phase::WaitingStart;
                }
                Event::Progress(progress) => self.progress = Some(progress),
                Event::Metrics(line) => self.metrics = line,
                Event::Finished(summary) => {
                    self.summary = Some(summary);
                    self.screen = Screen::Summary;
                }
                Event::Failed(message) => {
                    self.log(Level::Error, message.clone());
                    self.summary = Some(Summary::default());
                    self.screen = Screen::Summary;
                    self.phase = Phase::Failed;
                }
                Event::ModelCheck(result) => self.model_checked(result),
            }
        }
    }

    fn reset(&mut self) {
        self.screen = Screen::Setup;
        self.phase = Phase::Setup;
        self.progress = None;
        self.summary = None;
        self.questions = None;
        self.metrics.clear();
        self.logs.clear();
        self.errors.clear();
        self.abort = Arc::new(AtomicBool::new(false));
        self.focus = 0;
    }

    fn setup_key(&mut self, key: KeyEvent, _cli: &Cli) {
        self.errors.clear();
        match key.code {
            KeyCode::Esc => {
                let filled =
                    !self.room.is_empty() || !self.name.is_empty() || !self.duration.is_empty();
                if filled && !self.escape_armed {
                    self.escape_armed = true;
                    self.log(Level::Info, "Esc lagi untuk keluar");
                } else {
                    std::process::exit(0);
                }
            }
            KeyCode::F(3) => {
                self.advanced_open = !self.advanced_open;
                if !self.advanced_open && self.focus >= 3 {
                    self.focus = 2;
                }
            }
            KeyCode::F(1) => {
                self.log(
                    Level::Info,
                    "Tab pindah · Enter validasi · Esc kosongkan/keluar · F3 lanjutan",
                );
            }
            KeyCode::Tab => {
                self.advance_focus(1);
            }
            KeyCode::BackTab => {
                self.advance_focus(-1);
            }
            KeyCode::Down => {
                self.advance_focus(1);
            }
            KeyCode::Up => {
                self.advance_focus(-1);
            }
            KeyCode::Enter => {
                self.submit_setup();
            }
            KeyCode::Char(' ') if self.is_toggle() => {
                self.toggle();
            }
            KeyCode::Left if self.focus == 5 => {
                self.thinking = match self.thinking {
                    Thinking::Auto => Thinking::Off,
                    Thinking::On => Thinking::Auto,
                    Thinking::Off => Thinking::Off,
                };
            }
            KeyCode::Right if self.focus == 5 => {
                self.thinking = match self.thinking {
                    Thinking::Auto => Thinking::On,
                    Thinking::On => Thinking::On,
                    Thinking::Off => Thinking::Auto,
                };
            }
            KeyCode::Backspace => {
                self.field_mut().pop();
                self.escape_armed = false;
            }
            KeyCode::Char(ch) => {
                if self.is_toggle() {
                    self.toggle();
                    return;
                }
                let field = self.field_mut();
                if field.chars().count() < 120 {
                    field.push(ch);
                }
                self.escape_armed = false;
            }
            _ => {}
        }
    }

    fn confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if self.model_checking {
                    return;
                }
                let Some(cfg) = self.pending.clone() else {
                    return;
                };
                if self.app_cfg.api_key.trim().is_empty() {
                    self.pending = None;
                    self.start(cfg);
                    return;
                }
                self.check_model(cfg);
            }
            KeyCode::Esc => {
                self.screen = Screen::Setup;
                if let Some(cfg) = self.pending.take() {
                    self.room = cfg.room;
                    self.name = cfg.name;
                    self.duration = fmt_duration(cfg.duration_ms);
                }
            }
            _ => {}
        }
    }

    fn check_model(&mut self, cfg: SessionConfig) {
        self.model_checking = true;
        let key = self.app_cfg.key();
        let base = cfg.base_url.clone();
        let model = cfg.model.clone();
        let vision = cfg.vision;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Vec<String>, String> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                let solver = crate::solver::deepseek::Solver::new(
                    &base,
                    &key,
                    &model,
                    if vision { &model } else { "" },
                    Thinking::Auto,
                    None,
                    0.2,
                )
                .map_err(|e| e.to_string())?;
                runtime
                    .block_on(solver.list_models())
                    .map_err(|e| e.to_string())
            })();
            let _ = tx.send(Event::ModelCheck(result));
        });
    }

    fn is_toggle(&self) -> bool {
        self.advanced_open && matches!(self.focus, 7 | 9)
    }

    fn toggle(&mut self) {
        match self.focus {
            7 => self.vision = !self.vision,
            9 => self.fullscreen = !self.fullscreen,
            _ => {}
        }
    }

    fn advance_focus(&mut self, delta: i32) {
        let max = if self.advanced_open { FIELDS - 1 } else { 2 };
        let next = self.focus as i32 + delta;
        self.focus = if next < 0 {
            max
        } else if next as usize > max {
            0
        } else {
            next as usize
        };
    }

    fn field_mut(&mut self) -> &mut String {
        match self.focus {
            0 => &mut self.room,
            1 => &mut self.name,
            2 => &mut self.duration,
            3 => &mut self.adv.base_url,
            4 => &mut self.adv.model,
            6 => &mut self.adv.effort,
            8 => &mut self.adv.ocr,
            10 => &mut self.adv.profile,
            11 => &mut self.adv.jeda_min,
            _ => &mut self.adv.jeda_maks,
        }
    }

    fn submit_setup(&mut self) {
        self.errors.clear();
        if !valid_room_code(&self.room) {
            self.errors.push("kode room 3-11 alfanumerik".into());
        }
        if !valid_display_name(&self.name) {
            self.errors.push("nama 1-60 karakter".into());
        }
        let duration = parse_duration(&self.duration);
        if !matches!(duration, Some(d) if (30_000..=3_600_000).contains(&d)) {
            self.errors
                .push("durasi mm:ss antara 00:30 dan 60:00".into());
        }
        let jeda_min = parse_seconds(&self.adv.jeda_min);
        let jeda_maks = parse_seconds(&self.adv.jeda_maks);
        if !self.advanced_open
            && (self.adv.jeda_min.trim().is_empty() || self.adv.jeda_maks.trim().is_empty())
        {
            self.errors
                .push("jeda di panel lanjutan belum diisi".into());
        }
        if let (Some(min), Some(max)) = (jeda_min, jeda_maks)
            && min > max
        {
            self.errors.push("jeda minimum > maksimum".into());
        }
        if self.adv.base_url.trim().is_empty() {
            self.errors.push("base URL kosong".into());
        }
        if self.adv.model.trim().is_empty() {
            self.errors.push("model kosong".into());
        }
        if !self.errors.is_empty() {
            return;
        }
        let cfg = SessionConfig {
            room: self.room.trim().to_uppercase(),
            name: self.name.trim().to_string(),
            duration_ms: duration.unwrap_or(180_000),
            model: self.adv.model.trim().to_string(),
            base_url: self.adv.base_url.trim().to_string(),
            thinking: self.thinking,
            reasoning_effort: if self.adv.effort.trim().is_empty() {
                None
            } else {
                Some(self.adv.effort.trim().to_string())
            },
            vision: self.vision,
            ocr: OcrMode::parse(&self.adv.ocr),
            fullscreen: self.fullscreen,
            profile: if self.adv.profile.trim().is_empty() {
                None
            } else {
                Some(self.adv.profile.trim().into())
            },
            jeda_min_ms: (jeda_min.unwrap_or(2.5) * 1000.0) as u64,
            jeda_maks_ms: (jeda_maks.unwrap_or(8.0) * 1000.0) as u64,
        };
        let cfg = SessionConfig {
            room: cfg.room.to_lowercase(),
            ..cfg
        };
        self.pending = Some(cfg.clone());
        self.screen = Screen::Confirm;
        if self.app_cfg.api_key.is_empty() {
            self.log(
                Level::Warn,
                "API key belum diset; jawaban hanya dari kunci/explain/heuristik",
            );
        }
    }

    fn model_checked(&mut self, result: Result<Vec<String>, String>) {
        self.model_checking = false;
        let Some(cfg) = self.pending.clone() else {
            return;
        };
        match result {
            Ok(models) => {
                if models.iter().any(|m| m == &cfg.model) {
                    self.start(cfg);
                } else {
                    self.errors = vec![format!(
                        "model '{}' tidak ada di provider ({} model tersedia)",
                        cfg.model,
                        models.len()
                    )];
                    self.screen = Screen::Setup;
                }
            }
            Err(e) => {
                self.errors = vec![format!("gagal memeriksa model: {e}")];
                self.screen = Screen::Setup;
            }
        }
    }

    fn start(&mut self, cfg: SessionConfig) {
        if self.app_cfg.api_key.is_empty() {
            // Still start: keys/explain/heuristics can carry the session.
        }
        self.phase = Phase::Launching;
        self.screen = Screen::Lobby;
        self.log(
            Level::Info,
            format!("mulai room {} sebagai {}", cfg.room, cfg.name),
        );
        crate::app::spawn(
            cfg,
            self.app_cfg.clone(),
            self.abort.clone(),
            self.tx.clone(),
        );
    }
}

fn parse_seconds(input: &str) -> Option<f64> {
    let value: f64 = input.trim().replace(',', ".").parse().ok()?;
    if (1.0..=15.0).contains(&value) {
        Some(value)
    } else {
        None
    }
}

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 80 || area.height < 24 {
        let text = Paragraph::new("Terminal terlalu kecil — butuh minimal 80x24.")
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(text, area);
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(6),
    ])
    .split(area);
    header(frame, chunks[0], app);
    match app.screen {
        Screen::Setup => setup(frame, chunks[1], app),
        Screen::Confirm => confirm(frame, chunks[1], app),
        Screen::Lobby => dashboard(frame, chunks[1], app, true),
        Screen::Quiz => dashboard(frame, chunks[1], app, false),
        Screen::Summary => summary(frame, chunks[1], app),
    }
    log_panel(frame, chunks[2], app);
    if app.abort_confirm {
        popup(frame, area, "Batalkan sesi? y/n");
    }
}

fn style(app: &App, color: Color) -> Style {
    if app.no_color {
        Style::default()
    } else {
        Style::default().fg(color)
    }
}

fn header(frame: &mut Frame, area: Rect, app: &App) {
    let title = Line::from(vec![
        Span::styled(" Waybot ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("· "),
        Span::styled(app.phase.label(), style(app, Color::Cyan)),
    ]);
    let state = if app.abort.load(Ordering::Relaxed) {
        "membatalkan…"
    } else {
        match app.screen {
            Screen::Setup => "Tab pindah · F3 lanjutan · Enter mulai · Esc keluar",
            Screen::Confirm => "Enter lanjut · Esc kembali",
            Screen::Lobby => "menunggu host — Ctrl-C batal",
            Screen::Quiz => "menjawab — Ctrl-C batal",
            Screen::Summary => "R ulangi · Esc keluar",
        }
    };
    let block = Block::default().borders(Borders::ALL);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            title,
            Line::from(Span::styled(state, style(app, Color::Gray))),
        ]))
        .block(block),
        area,
    );
}

fn field_line(app: &App, index: usize, label: &str, value: &str, hint: &str) -> Line<'static> {
    let active = app.focus == index && app.screen == Screen::Setup;
    let marker = if active { "▸" } else { " " };
    let mut style = Style::default();
    if active {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(vec![
        Span::styled(format!(" {marker} {label:<11}"), style),
        Span::raw(format!("[{value}]")),
        Span::styled(format!("  {hint}"), Style::default().fg(Color::DarkGray)),
    ])
}

fn setup(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        field_line(app, 0, "Kode Room", &app.room, "3-11 alfanumerik"),
        field_line(app, 1, "Nama", &app.name, "1-60 karakter"),
        field_line(app, 2, "Durasi", &app.duration, "mm:ss, 00:30-60:00"),
        Line::raw(""),
    ];
    if app.advanced_open {
        lines.push(Line::styled(
            " Lanjutan",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(field_line(
            app,
            3,
            "Base URL",
            &app.adv.base_url,
            &app.app_cfg.base_url,
        ));
        lines.push(field_line(
            app,
            4,
            "Model",
            &app.adv.model,
            "divalidasi ke /models",
        ));
        lines.push(field_line(
            app,
            5,
            "Thinking",
            crate::app::thinking_label(app.thinking),
            "←/→ ubah",
        ));
        lines.push(field_line(
            app,
            6,
            "Reasoning",
            &app.adv.effort,
            "kosong = default",
        ));
        lines.push(field_line(
            app,
            7,
            "Vision",
            if app.vision { "aktif" } else { "nonaktif" },
            "Spasi ubah",
        ));
        lines.push(field_line(app, 8, "OCR", &app.adv.ocr, "auto/off/path"));
        lines.push(field_line(
            app,
            9,
            "Fullscreen",
            if app.fullscreen { "ya" } else { "tidak" },
            "Spasi ubah",
        ));
        lines.push(field_line(
            app,
            10,
            "Profil FF",
            &app.adv.profile,
            "kosong = sementara",
        ));
        lines.push(field_line(app, 11, "Jeda min", &app.adv.jeda_min, "detik"));
        lines.push(field_line(
            app,
            12,
            "Jeda maks",
            &app.adv.jeda_maks,
            "detik",
        ));
    } else {
        lines.push(Line::styled(
            " F3 untuk opsi lanjutan (model, thinking, vision, jeda, profil)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    if !app.errors.is_empty() {
        lines.push(Line::raw(""));
        for err in &app.errors {
            lines.push(Line::styled(
                format!(" ! {err}"),
                Style::default().fg(Color::Red),
            ));
        }
    }
    if app.api_status() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            " API key: absen — jawaban dari kunci/explain/heuristik",
            Style::default().fg(Color::Yellow),
        ));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(" Setup "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

impl App {
    fn api_status(&self) -> bool {
        self.app_cfg.api_key.is_empty()
    }
}

fn confirm(frame: &mut Frame, area: Rect, app: &App) {
    let Some(cfg) = &app.pending else {
        frame.render_widget(Paragraph::new("tidak ada konfigurasi"), area);
        return;
    };
    let lines = vec![
        Line::raw(""),
        Line::styled(
            " Jendela Firefox akan dibuka. Biarkan fokus di browser.",
            Style::default().fg(Color::Yellow),
        ),
        Line::raw(""),
        Line::raw(format!(" Room      : {}", cfg.room)),
        Line::raw(format!(" Nama      : {}", cfg.name)),
        Line::raw(format!(" Durasi    : {}", fmt_duration(cfg.duration_ms))),
        Line::raw(format!(" Model     : {} ({})", cfg.model, cfg.base_url)),
        Line::raw(format!(
            " Thinking  : {}",
            crate::app::thinking_label(cfg.thinking)
        )),
        Line::raw(format!(
            " Vision    : {} · OCR {}",
            if cfg.vision { "aktif" } else { "nonaktif" },
            cfg.ocr.describe()
        )),
        Line::raw(format!(
            " Jeda      : {:.1}s - {:.1}s · fullscreen {}",
            cfg.jeda_min_ms as f64 / 1000.0,
            cfg.jeda_maks_ms as f64 / 1000.0,
            if cfg.fullscreen { "ya" } else { "tidak" }
        )),
        Line::raw(""),
        Line::styled(
            " Enter: lanjut   Esc: kembali",
            Style::default().fg(Color::Cyan),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(" Konfirmasi "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn dashboard(frame: &mut Frame, area: Rect, app: &App, lobby: bool) {
    let rows = Layout::vertical([Constraint::Length(5), Constraint::Min(4)]).split(area);
    let (total, keyed, explained, ai, low) = app.questions.unwrap_or((0, 0, 0, 0, 0));
    let ratio = 0.0;
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(if lobby {
            " Menunggu host "
        } else {
            " Progres "
        }))
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(ratio)
        .label(if lobby {
            format!("SIAP — SILAKAN START QUIZ DI BROWSER · {total} soal · kunci {keyed} · explain {explained} · ai {ai} · low {low}")
        } else {
            app.progress
                .as_ref()
                .map(|p| {
                    format!(
                        "soal {}/{} · {} · sisa {}",
                        p.index,
                        p.total,
                        p.source,
                        fmt_duration(p.remaining_ms)
                    )
                })
                .unwrap_or_else(|| "menunggu soal".into())
        });
    frame.render_widget(gauge, rows[0]);

    let mut lines: Vec<Line> = Vec::new();
    if let Some(progress) = &app.progress {
        lines.push(Line::styled(
            format!(" Soal: {}", progress.question),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw(format!(" Jawaban: {}", progress.answer)));
        lines.push(Line::raw(format!(
            " Status: {} · mode: {}",
            progress.status, progress.mode
        )));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        format!(" Anti-deteksi: {}", app.metrics),
        Style::default().fg(Color::DarkGray),
    ));
    if lobby {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            " Tunggu hingga host menekan Start. Jangan pindahkan fokus jendela.",
            Style::default().fg(Color::Yellow),
        ));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(" Status "))
            .wrap(Wrap { trim: false }),
        rows[1],
    );
}

fn summary(frame: &mut Frame, area: Rect, app: &App) {
    let s = app.summary.clone().unwrap_or_default();
    let mut lines = vec![
        Line::styled(
            format!(" {} / {} soal dijawab", s.answered, s.total),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::raw(format!(
            " Metrik: {}",
            if s.metrics.is_empty() {
                &app.metrics
            } else {
                &s.metrics
            }
        )),
        Line::raw(format!(" Sesi  : {}", s.session_dir)),
        Line::raw(""),
    ];
    if app.phase == Phase::Failed {
        lines.push(Line::styled(
            " Sesi gagal — lihat log.",
            Style::default().fg(Color::Red),
        ));
    }
    lines.push(Line::styled(
        " R: ulangi   Esc: keluar",
        Style::default().fg(Color::Cyan),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(" Ringkasan "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn log_panel(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .logs
        .iter()
        .rev()
        .take(area.height.saturating_sub(2) as usize)
        .rev()
        .map(|(level, message)| {
            let color = match level {
                Level::Info => Color::Gray,
                Level::Warn => Color::Yellow,
                Level::Error => Color::Red,
            };
            ListItem::new(Line::styled(message.clone(), style(app, color)))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Log ")),
        area,
    );
}

fn popup(frame: &mut Frame, area: Rect, text: &str) {
    let width = 36.min(area.width);
    let height = 3;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(x, y, width, height);
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_parsing_bounds() {
        assert_eq!(parse_seconds("2.5"), Some(2.5));
        assert_eq!(parse_seconds("0.5"), None);
        assert_eq!(parse_seconds("20"), None);
        assert_eq!(parse_seconds(""), None);
    }

    #[test]
    fn toggle_only_on_toggle_fields() {
        let (tx, rx) = std::sync::mpsc::channel();
        let app_cfg = AppConfig {
            api_key: String::new(),
            base_url: "b".into(),
            model: "m".into(),
            vision_model: String::new(),
            thinking: Thinking::Auto,
            reasoning_effort: None,
            temperature: 0.2,
            profile: None,
            ocr: OcrMode::Auto,
            vision: false,
            agent: false,
            wait_max_min: 0,
            jeda_min_ms: 2500,
            jeda_maks_ms: 8000,
        };
        let mut app = App {
            screen: Screen::Setup,
            focus: 7,
            advanced_open: false,
            room: String::new(),
            name: String::new(),
            duration: String::new(),
            adv: AdvFields::default(),
            thinking: Thinking::Auto,
            vision: false,
            fullscreen: false,
            errors: vec![],
            app_cfg,
            tx,
            rx,
            abort: Arc::new(AtomicBool::new(false)),
            phase: Phase::Setup,
            logs: vec![],
            progress: None,
            questions: None,
            metrics: String::new(),
            summary: None,
            abort_confirm: false,
            escape_armed: false,
            no_color: true,
            model_checking: false,
            pending: None,
        };
        assert!(!app.is_toggle());
        app.advanced_open = true;
        assert!(app.is_toggle());
        app.toggle();
        assert!(app.vision);
    }
}
