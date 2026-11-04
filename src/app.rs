//! Session orchestration: launch, join, harvest, wait, answer, finish.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::anticheat::{Metrics, pacing::Pacer};
use crate::browser::Browser;
use crate::browser::dom;
use crate::browser::focus;
use crate::config::{SessionConfig, Thinking};
use crate::events::{self, Event, Level, Phase, Progress, Summary, Tx};
use crate::model::{Answer, AnswerSource, DomOption, PageState, Question, QuestionType};
use crate::solver::agent::{self, ActionKind, AgentAction};
use crate::solver::deepseek::Solver;
use crate::store::Store;
use crate::wayground::{harvest, join, quiz, selectors};

pub struct Runtime {
    pub store: Store,
    pub metrics: Metrics,
    pub aborted: Arc<AtomicBool>,
    pub fullscreen: bool,
    pub wait_max_min: u64,
    pub agent: bool,
    pub resume: bool,
}

pub fn spawn(config: SessionConfig, app: crate::config::AppConfig, abort: Arc<AtomicBool>, tx: Tx) {
    std::thread::spawn(move || {
        let result = run(config, app, abort.clone(), tx.clone());
        if let Err(message) = result {
            events::log(&tx, Level::Error, format!("gagal: {message}"));
            let _ = tx.send(Event::Failed(message));
        }
    });
}

fn run(
    config: SessionConfig,
    app: crate::config::AppConfig,
    aborted: Arc<AtomicBool>,
    tx: Tx,
) -> Result<(), String> {
    let data_root = crate::store::data_root().map_err(|e| format!("folder data: {e}"))?;
    let store =
        Store::create(&config.room, &config.name).map_err(|e| format!("folder sesi: {e}"))?;
    let mut rt = Runtime {
        store,
        metrics: Metrics::default(),
        aborted,
        fullscreen: config.fullscreen,
        wait_max_min: app.wait_max_min,
        agent: config.agent,
        resume: config.resume,
    };
    events::set_log_file(&rt.store.dir().join("app.log"));
    events::file_log("run", "mulai");
    events::phase(&tx, Phase::Launching);
    events::log(
        &tx,
        Level::Info,
        format!("sesi: {}", rt.store.dir().display()),
    );

    let mut browser = Browser::launch(config.profile.as_deref(), &data_root, config.fullscreen)
        .map_err(|e| format!("browser: {e}"))?;
    events::log(
        &tx,
        Level::Info,
        format!("Firefox pid {}", browser.ff.pid()),
    );

    // Shim in case the extension was not installed in time.
    let _ = browser
        .mn
        .script(crate::browser::extension::RUNTIME_SHIM, json!([]));

    browser.ensure_focus(&mut rt.metrics);
    let webdriver_ok = browser.webdriver_flag();
    rt.metrics.webdriver_ok = webdriver_ok;
    events::log(
        &tx,
        if webdriver_ok == Some(true) {
            Level::Info
        } else {
            Level::Warn
        },
        format!("navigator.webdriver === false: {webdriver_ok:?}"),
    );

    events::phase(&tx, Phase::Joining);
    let resumed = try_resume(&mut browser, &config, &data_root, &tx, &mut rt.metrics);
    if !resumed
        && let Err(e) = join::join(&mut browser, &config.room, &config.name, config.resume, &tx)
    {
        browser.screenshot("join-fail", &rt.store);
        let _ = browser
            .mn
            .script(
                "return document.body ? document.body.innerText.slice(0, 2000) : '';",
                json!([]),
            )
            .map(|v| {
                rt.store
                    .write_text("join-fail.txt", v.as_str().unwrap_or_default())
            });
        return Err(e.to_string());
    }
    let _ = tx.send(Event::Joined);
    remember_session(&data_root, &config, &browser.current_url());

    // Fullscreen is requested by the room; never leave it on our own.
    if rt.fullscreen || dom::exists(&mut browser.mn, selectors::FULLSCREEN_BUTTON) {
        let _ = focus::focus_button(&mut browser.mn);
        rt.metrics.fullscreen_entered = true;
    }

    events::phase(&tx, Phase::Harvesting);
    let harvested = harvest::harvest(&mut browser, &config.room, &config.name, &tx)
        .map_err(|e| format!("harvest: {e}"))?;
    rt.store
        .save_questions(&harvested.questions)
        .map_err(|e| format!("simpan soal: {e}"))?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {e}"))?;

    // Solver: everything is answered before the host starts.
    let solver = if app.has_key() {
        match Solver::new(
            &config.base_url,
            &app.api_key,
            &config.model,
            &app.vision_model,
            config.thinking,
            config.reasoning_effort.clone(),
            app.temperature,
        ) {
            Ok(solver) => {
                if let Err(e) = runtime.block_on(solver.list_models()) {
                    events::log(&tx, Level::Warn, format!("daftar model: {e}"));
                }
                Some(solver)
            }
            Err(e) => {
                events::log(&tx, Level::Warn, format!("solver nonaktif: {e}"));
                None
            }
        }
    } else {
        events::log(
            &tx,
            Level::Warn,
            "API key tidak ada; pakai kunci/explain/heuristik",
        );
        None
    };

    let mut answers: Vec<Answer> = harvested.answers.clone();
    let mut low = 0u32;
    let mut ai = 0u32;
    for question in harvested.questions.clone() {
        if answers.iter().any(|a| a.question_id == question.id) {
            continue;
        }
        let mut answer = match &solver {
            Some(solver) => {
                let (hint, images) = media_context(&mut browser, &question);
                match runtime.block_on(solver.solve(&question, &hint, &images)) {
                    Ok(answer) => answer,
                    Err(e) => {
                        events::log(
                            &tx,
                            Level::Warn,
                            format!("solver soal {}: {e}", question.id),
                        );
                        crate::solver::heuristic(&question)
                    }
                }
            }
            None => crate::solver::heuristic(&question),
        };
        if answer.low_confidence {
            low += 1;
        }
        if answer.source == AnswerSource::Ai {
            ai += 1;
        }
        answer.reason = answer.reason.clone();
        answers.push(answer);
        std::thread::sleep(Duration::from_millis(120));
    }
    rt.store
        .save_answers(&answers)
        .map_err(|e| format!("simpan jawaban: {e}"))?;
    let _ = tx.send(Event::QuestionsReady {
        total: harvested.questions.len() as u32,
        keyed: harvested.keyed,
        explained: harvested.explained,
        ai,
        low,
    });

    // Wait for the host.
    events::phase(&tx, Phase::WaitingStart);
    events::log(&tx, Level::Info, "SIAP — silakan start quiz di browser");
    let _ = tx.send(Event::Ready);
    let started = wait_start(&mut browser, &mut rt, &tx, &harvested.questions)?;
    if !started {
        browser.keep_open();
        let _ = tx.send(Event::Finished(summary(&rt, 0, 0)));
        return Ok(());
    }

    events::phase(&tx, Phase::Answering);
    // Official keys only become reachable once the game URL is live.
    let key_map = harvest::fetch_key_map(&mut browser, &tx);
    if !key_map.is_empty() {
        let mut replaced = 0u32;
        for question in &harvested.questions {
            if let Some(values) = key_map.get(&question.id)
                && let Some(slot) = answers.iter_mut().find(|a| a.question_id == question.id)
            {
                *slot = Answer::new(&question.id, values.clone(), AnswerSource::Key);
                replaced += 1;
            }
        }
        if replaced > 0 {
            let _ = rt.store.save_answers(&answers);
            events::log(&tx, Level::Info, format!("{replaced} kunci resmi dipakai"));
        }
    }
    let total = harvested.questions.len() as u32;
    let pacer = Pacer::new(
        config.duration_ms,
        total,
        config.jeda_min_ms,
        config.jeda_maks_ms,
    );
    let mut pacer = pacer;
    let started_at = Instant::now();
    let mut answered = 0u32;
    let mut current_id: Option<String> = None;
    let mut cursor = 0usize;
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut attempts: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    let mut pregame_started_over = false;
    let mut max_seen: u32 = 0;
    let mut answered_fingerprints: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut last_dbg = Instant::now();
    let mut last_html = Instant::now();
    let mut last_agent = Instant::now() - Duration::from_secs(10);
    let mut last_qid: Option<String> = None;
    let mut same_since = Instant::now();
    let mut last_nudge = Instant::now() - Duration::from_secs(10);
    let mut last_answer_at = Instant::now();
    let mut end_seen: Option<Instant> = None;

    let mut overall = Instant::now() + Duration::from_millis(config.duration_ms);
    loop {
        if rt.aborted.load(Ordering::Relaxed) {
            events::log(&tx, Level::Warn, "dibatalkan pengguna");
            break;
        }
        if Instant::now() > overall + Duration::from_secs(10) {
            events::log(&tx, Level::Warn, "batas waktu total tercapai");
            break;
        }
        rt.store
            .append_event("tick", json!({"elapsed": started_at.elapsed().as_millis()}));
        let state = match dom::snapshot(&mut browser.mn) {
            Ok(state) => state,
            Err(e) => {
                events::log(
                    &tx,
                    Level::Warn,
                    format!("snapshot: {e}; mencoba reconnect"),
                );
                let mut ok = false;
                for _ in 0..5 {
                    std::thread::sleep(Duration::from_millis(800));
                    if browser.reconnect().is_ok() {
                        ok = true;
                        break;
                    }
                }
                events::log(
                    &tx,
                    if ok { Level::Info } else { Level::Error },
                    if ok {
                        "koneksi Marionette pulih"
                    } else {
                        "reconnect gagal"
                    },
                );
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        browser.tick_focus(&mut rt.metrics, false);
        if let Some(number) = state.question_number {
            max_seen = max_seen.max(number);
        }
        if last_dbg.elapsed() > Duration::from_secs(3) {
            last_dbg = Instant::now();
            events::file_log(
                "state",
                &format!(
                    "{} qid={:?} opts={} next={} fb={} wait={} body={}",
                    state.url,
                    state.question_id,
                    state.options.len(),
                    state.next_button.exists,
                    state.feedback_visible,
                    state.waiting_for_host,
                    crate::util::truncate_chars(&state.body_excerpt, 120)
                ),
            );
        }

        if state.finished
            && !state.waiting_for_host
            && state.question_number.is_none()
            && last_answer_at.elapsed() > Duration::from_secs(5)
        {
            events::log(&tx, Level::Info, "quiz selesai di browser");
            break;
        }
        if let Some(label) = dom::dismiss_obstacles(&mut browser.mn) {
            events::file_log("obstacle", &label);
            std::thread::sleep(Duration::from_millis(350));
            continue;
        }
        if state.login_popup && dom::dismiss_login(&mut browser.mn) {
            events::file_log("login", "popup login ditutup");
            std::thread::sleep(Duration::from_millis(400));
            continue;
        }
        if state.fullscreen_prompt && rt.fullscreen {
            let _ = focus::focus_button(&mut browser.mn);
        }
        if !state.dialog_text.is_empty() {
            events::log(&tx, Level::Warn, format!("dialog: {}", state.dialog_text));
            let _ = browser.mn.script(
                "const b=[...document.querySelectorAll('button')].find(x=>/^(ok|yes|ya|resume|continue)$/i.test((x.innerText||'').trim())); if(b)b.click(); return !!b;",
                json!([]),
            );
            std::thread::sleep(Duration::from_millis(600));
            continue;
        }

        let mut resolved_id = state.effective_id();
        // Body-hash ids are unstable during transitions; reuse the recent question id.
        if let (Some(id), Some(current)) = (resolved_id.clone(), current_id.clone())
            && id.starts_with('b')
            && same_since.elapsed() < Duration::from_secs(6)
        {
            resolved_id = Some(current);
        }
        let Some(qid) = resolved_id else {
            if last_answer_at.elapsed() > Duration::from_secs(15) && state.finished {
                events::log(
                    &tx,
                    Level::Info,
                    "tidak ada soal baru; sesi dianggap selesai",
                );
                break;
            }
            if (!state.options.is_empty() || !state.inputs.is_empty())
                && solver.is_some()
                && rt.agent
                && last_agent.elapsed() > Duration::from_secs(4)
            {
                last_agent = Instant::now();
                events::log(&tx, Level::Warn, "soal tanpa id; agen membaca halaman");
                if let Some(solver) = &solver {
                    let mut idle_done = false;
                    run_agent(
                        &mut browser,
                        solver,
                        &runtime,
                        None,
                        None,
                        &tx,
                        &rt.store,
                        &mut rt.metrics,
                        None,
                        None,
                        &mut idle_done,
                    );
                    last_agent = Instant::now();
                    let empty = state.options.is_empty() && state.inputs.is_empty();
                    if state.finished
                        && empty
                        && !state.waiting_for_host
                        && state.dialog_text.is_empty()
                    {
                        let since = *end_seen.get_or_insert_with(Instant::now);
                        if since.elapsed() > Duration::from_secs(4) {
                            events::log(&tx, Level::Info, "halaman akhir stabil; sesi selesai");
                            break;
                        }
                    } else {
                        end_seen = None;
                    }
                    if idle_done {
                        events::log(
                            &tx,
                            Level::Info,
                            "agen selesai di layar ini; lanjut memantau",
                        );
                    }
                }
            } else if last_html.elapsed() > Duration::from_secs(3) {
                last_html = Instant::now();
                if let Ok(html) = browser
                    .mn
                    .script(
                        "const c=document.querySelector('[data-quesid], .quiz-container'); return c ? c.outerHTML.slice(0, 1800) : 'NO-CONTAINER';",
                        json!([]),
                    )
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                {
                    events::file_log("html", &html);
                }
            }
            std::thread::sleep(Duration::from_millis(400));
            continue;
        };
        if state.url.contains("/pre-game/") {
            if last_nudge.elapsed() > Duration::from_secs(4) {
                last_nudge = Instant::now();
                if config.resume {
                    let _ = crate::wayground::join::click_text_button(
                        &mut browser,
                        &["resume attempt", "resume", "start", "lanjut"],
                    );
                } else if !pregame_started_over {
                    pregame_started_over = true;
                    events::file_log("pregame", "start over sekali");
                    let _ = crate::wayground::join::click_text_button(
                        &mut browser,
                        &["start over", "mulai ulang"],
                    );
                    std::thread::sleep(Duration::from_millis(800));
                    crate::wayground::join::ensure_name(&mut browser, &config.name, &tx);
                    let _ = crate::wayground::join::click_text_button(
                        &mut browser,
                        &["start", "lanjut"],
                    );
                } else {
                    crate::wayground::join::ensure_name(&mut browser, &config.name, &tx);
                    let _ = crate::wayground::join::click_text_button(
                        &mut browser,
                        &["start", "lanjut"],
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(600));
            continue;
        }
        if !state.url.contains("/join/game/") {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        pregame_started_over = false;
        if state.options.is_empty() && state.inputs.is_empty() {
            std::thread::sleep(Duration::from_millis(400));
            continue;
        }
        // Fingerprint guards against id churn during transitions.
        let mut fp_parts: Vec<String> = state
            .options
            .iter()
            .map(|o| crate::util::norm_key(&o.text))
            .collect();
        fp_parts.sort();
        let fingerprint = format!(
            "{}|{}",
            crate::util::norm_key(&state.question_text),
            fp_parts.join("||")
        );
        if answered_fingerprints.contains(&fingerprint) {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        if current_id.as_deref() == Some(qid.as_str()) || visited.contains(&qid) {
            // Watchdog: the same screen for too long is a stall, not progress.
            if same_since.elapsed() > Duration::from_secs(20)
                && last_nudge.elapsed() > Duration::from_secs(5)
            {
                last_nudge = Instant::now();
                same_since = Instant::now();
                events::log(
                    &tx,
                    Level::Warn,
                    format!("macet di soal {qid}; dorong maju"),
                );
                if state.next_button.exists {
                    let _ = quiz::click_next(&mut browser);
                } else if !state.options.is_empty() || !state.inputs.is_empty() {
                    visited.remove(&qid);
                    current_id = None;
                } else if rt.agent
                    && let Some(solver) = &solver
                {
                    let mut nudge_done = false;
                    run_agent(
                        &mut browser,
                        solver,
                        &runtime,
                        None,
                        None,
                        &tx,
                        &rt.store,
                        &mut rt.metrics,
                        Some(&qid),
                        None,
                        &mut nudge_done,
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }

        if last_qid.as_deref() != Some(qid.as_str()) {
            last_qid = Some(qid.clone());
            same_since = Instant::now();
        }
        current_id = Some(qid.clone());
        visited.insert(qid.clone());
        // Harvest order is not guaranteed to equal the live quiz order: match text first.
        let shown = {
            let base = crate::util::norm_key(&state.question_text);
            if base.is_empty() {
                let marker = "question text:";
                let lower = state.body_excerpt.to_lowercase();
                match lower.find(marker) {
                    Some(pos) => crate::util::norm_key(&state.body_excerpt[pos + marker.len()..]),
                    None => String::new(),
                }
            } else {
                base
            }
        };
        let shown_short: String = shown.chars().take(80).collect();
        let by_text = harvested
            .questions
            .iter()
            .find(|q| {
                let key = crate::util::norm_key(&q.text);
                !key.is_empty()
                    && !shown_short.is_empty()
                    && (shown.contains(&key) || key.contains(&shown_short))
            })
            .cloned();
        let shown_opts: std::collections::BTreeSet<String> = state
            .options
            .iter()
            .map(|o| crate::util::norm_key(&o.text))
            .collect();
        let by_options = harvested
            .questions
            .iter()
            .find(|q| {
                !shown_opts.is_empty()
                    && q.options.len() == state.options.len()
                    && q.options
                        .iter()
                        .all(|o| shown_opts.contains(&crate::util::norm_key(&o.text)))
            })
            .cloned();
        let by_index = state
            .question_number
            .and_then(|n| harvested.questions.get(n.saturating_sub(1) as usize))
            .cloned();
        let mut question = match harvested
            .questions
            .iter()
            .find(|q| q.id == qid)
            .cloned()
            .or(by_text)
            .or(by_options)
            .or(by_index)
        {
            Some(q) => q,
            None => {
                events::log(
                    &tx,
                    Level::Warn,
                    format!("soal {qid} tidak ada di daftar; dibentuk dari DOM"),
                );
                question_from_state(&state)
            }
        };
        if question.id.is_empty() {
            question.id = qid.clone();
        }
        // The host may pause between questions; guarantee headroom per question.
        overall = overall.max(Instant::now() + Duration::from_secs(30));
        let existing = answers
            .iter()
            .find(|a| a.question_id == question.id)
            .cloned();
        let answer = match existing {
            Some(a) => a,
            None => {
                let (hint, images) = media_context(&mut browser, &question);
                let mut a = match &solver {
                    Some(solver) => runtime
                        .block_on(solver.solve(&question, &hint, &images))
                        .unwrap_or_else(|_| crate::solver::heuristic(&question)),
                    None => crate::solver::heuristic(&question),
                };
                a.question_id = question.id.clone();
                answers.push(a.clone());
                a
            }
        };

        if rt.aborted.load(Ordering::Relaxed) {
            break;
        }

        // Human pause, budget aware.
        let delay = pacer.next_delay();
        events::log(
            &tx,
            Level::Info,
            format!("jeda {} ms sebelum soal {}", delay, question.id),
        );
        if !rt.agent {
            let mut waited = 0u64;
            while waited < delay && !rt.aborted.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                waited += 100;
            }
        }

        let _ = tx.send(Event::Progress(Progress {
            index: state.question_number.unwrap_or((cursor + 1) as u32),
            total: state.total_questions.unwrap_or(total),
            question: crate::util::truncate_chars(&question.plain_text(), 90),
            answer: answer.values.join(" | "),
            source: answer.source.label().to_string(),
            status: "mengirim".into(),
            remaining_ms: overall
                .saturating_duration_since(Instant::now())
                .as_millis() as u64,
            mode: mode_label(&state),
        }));

        browser.ensure_focus(&mut rt.metrics);
        let _ = dom::dismiss_obstacles(&mut browser.mn);
        let _ = dom::dismiss_login(&mut browser.mn);
        let took = Instant::now();

        // The page agent leads when enabled: it reads the page, so unfamiliar
        // layouts do not need a hard-coded branch. Deterministic clicks stay as
        // the instant shortcut, and as fallback if the agent cannot act.
        let mut agent_acted = false;
        let mut agent_done = false;
        if let (Some(solver), true) = (&solver, rt.agent) {
            agent_acted = run_agent(
                &mut browser,
                solver,
                &runtime,
                Some(&question),
                Some(&answer),
                &tx,
                &rt.store,
                &mut rt.metrics,
                Some(&qid),
                Some(delay),
                &mut agent_done,
            );
        }
        // `done` from a per-question agent only means "this question is handled";
        // the session ends only on a stable end page (checked at the loop top).
        if agent_done {
            events::log(&tx, Level::Info, "agen menandai soal selesai");
        }
        let looks_done = state.options.iter().any(|o| o.selected) || state.feedback_visible;
        if !agent_acted && !looks_done {
            let executed = quiz::execute(&mut browser, &question, &state, &answer);
            if let Err(e) = executed {
                events::log(&tx, Level::Warn, format!("eksekusi {}: {e}", question.id));
                events::file_log(
                    "exec-fail",
                    &format!(
                        "type={:?} ans={:?} opts={:?} submit={:?} body={}",
                        question.type_,
                        answer.values,
                        state.options,
                        state.submit_button,
                        crate::util::truncate_chars(&state.body_excerpt, 300)
                    ),
                );
                quiz::handle_unknown(&mut browser, &rt.store, "exec-error");
                let fallback = crate::solver::heuristic(&question);
                let _ = quiz::execute(&mut browser, &question, &state, &fallback);
            }
        }

        let action_ms = took.elapsed().as_millis() as u64;
        // Record the attempt immediately: grading happens at submit, not at ack.
        rt.store.append_event(
            "answer",
            json!({"qid": question.id, "source": answer.source.label(), "action_ms": action_ms}),
        );
        events::file_log(
            "timing",
            &format!("qid={} action_ms={action_ms} attempt", question.id),
        );
        if !fingerprint.is_empty() {
            answered_fingerprints.insert(fingerprint.clone());
        }

        let mut acked = wait_ack(&mut browser, &qid, &mut rt.metrics);
        rt.store.append_event(
            "ack",
            json!({"qid": question.id, "acked": acked, "answer_ms": took.elapsed().as_millis()}),
        );
        if !acked
            && rt.agent
            && !agent_acted
            && let Some(solver) = &solver
        {
            events::log(
                &tx,
                Level::Warn,
                "deterministik gagal; serahkan ke agen halaman",
            );
            if run_agent(
                &mut browser,
                solver,
                &runtime,
                Some(&question),
                Some(&answer),
                &tx,
                &rt.store,
                &mut rt.metrics,
                Some(&qid),
                None,
                &mut agent_done,
            ) {
                acked = wait_ack(&mut browser, &qid, &mut rt.metrics);
            }
        }
        pacer.record_answered(took.elapsed().as_millis() as u64);
        answered += 1;
        cursor += 1;
        rt.store.append_event(
            "result",
            json!({"qid": question.id, "acked": acked, "answer_ms": took.elapsed().as_millis()}),
        );
        events::file_log(
            "timing",
            &format!(
                "qid={} action_ms={action_ms} answer_ms={} acked={acked}",
                question.id,
                took.elapsed().as_millis()
            ),
        );

        if acked {
            last_answer_at = Instant::now();
            advance(&mut browser, &qid, &mut rt.metrics, &rt.store, &rt.aborted);
        } else {
            let tries = attempts.entry(qid.clone()).or_insert(0);
            *tries += 1;
            answered_fingerprints.remove(&fingerprint);
            if *tries < 3 {
                visited.remove(&qid);
                current_id = None;
                events::log(
                    &tx,
                    Level::Warn,
                    format!("jawaban {qid} belum terkirim; ulangi ({tries}/3)"),
                );
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        let _ = tx.send(Event::Metrics(rt.metrics.summary()));
    }

    events::phase(&tx, Phase::Finishing);
    browser.keep_open();
    rt.store.save_answers(&answers).map_err(|e| e.to_string())?;
    rt.store
        .write_json(
            "result.json",
            &json!({
                "answered": answered,
                "total": total,
                "metrics": rt.metrics.summary(),
                "elapsed_ms": started_at.elapsed().as_millis() as u64
            }),
        )
        .ok();
    browser.screenshot("final", &rt.store);
    events::phase(&tx, Phase::Done);
    let _ = tx.send(Event::Finished(summary(&rt, answered, total)));
    Ok(())
}

fn media_context(browser: &mut Browser, question: &Question) -> (String, Vec<String>) {
    let mut hint = String::new();
    for media in &question.media {
        if !media.alt.trim().is_empty() {
            hint.push_str(&media.alt);
            hint.push(' ');
        }
    }
    let mut images = Vec::new();
    if !question.media.is_empty() {
        for media in question.media.iter().take(2) {
            if let Some(data) = page_image(browser, &media.src) {
                images.push(data);
            }
        }
    }
    (hint, images)
}

fn page_image(browser: &mut Browser, src: &str) -> Option<String> {
    let script = r#"
const url = arguments[0]; const done = arguments[arguments.length - 1];
(async () => {
  try {
    const r = await fetch(url, {credentials: 'include'});
    const blob = await r.blob();
    const reader = new FileReader();
    reader.onload = () => done(reader.result);
    reader.readAsDataURL(blob);
  } catch (e) { done(null); }
})();
"#;
    let value = browser.mn.async_script(script, json!([src])).ok()?;
    value.as_str().map(str::to_string)
}

#[allow(clippy::too_many_arguments)]
fn run_agent(
    browser: &mut Browser,
    solver: &Solver,
    runtime: &tokio::runtime::Runtime,
    question: Option<&Question>,
    answer: Option<&Answer>,
    tx: &Tx,
    store: &Store,
    metrics: &mut Metrics,
    current: Option<&str>,
    first_delay_ms: Option<u64>,
    done_flag: &mut bool,
) -> bool {
    let before = current.map(str::to_string);
    let mut history: Vec<String> = Vec::new();
    let mut invalid = 0u8;
    let mut last_action = String::new();

    for step in 0..10 {
        let Ok(state) = dom::snapshot(&mut browser.mn) else {
            std::thread::sleep(Duration::from_millis(400));
            continue;
        };
        if before.is_some() && state.effective_id().as_deref() != before.as_deref() {
            events::file_log("agent", "soal berubah");
            return true;
        }
        let Ok(snapshot) = dom::agent_snapshot(&mut browser.mn) else {
            std::thread::sleep(Duration::from_millis(400));
            continue;
        };
        // Vision on the first step (and on retries) so the model sees the page.
        let mut images: Vec<String> = Vec::new();
        if step == 0 || invalid > 0 || history.len() > 4 {
            if let Some(question) = question {
                for media in question.media.iter().take(2) {
                    if let Some(data) = page_image(browser, &media.src) {
                        images.push(data);
                    }
                }
            }
            if let Some(shot) = browser.screenshot_data_url() {
                images.push(shot);
            }
        }
        let suggested: Vec<u32> = match (question, answer) {
            (Some(question), Some(answer)) => quiz::match_indices(question, answer),
            _ => Vec::new(),
        };
        let plan_start = Instant::now();
        let planned = match first_delay_ms.filter(|_| step == 0) {
            Some(delay) => runtime.block_on(async {
                tokio::join!(
                    tokio::time::sleep(std::time::Duration::from_millis(delay)),
                    agent::plan(
                        solver, &snapshot, question, answer, &history, &images, &suggested
                    )
                )
                .1
            }),
            None => runtime.block_on(agent::plan(
                solver, &snapshot, question, answer, &history, &images, &suggested,
            )),
        };
        events::file_log(
            "vision",
            &format!(
                "step{step} {} ms gambar={}",
                plan_start.elapsed().as_millis(),
                images.len()
            ),
        );
        let action = match planned {
            Ok(action) => action,
            Err(e) => {
                events::log(tx, Level::Warn, format!("agen: {e}"));
                history.push(format!("model error: {e}"));
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };

        if let Err(reason) = validate_action(&snapshot, &action) {
            invalid += 1;
            let line = format!("{:?} tidak valid: {reason}", action.action);
            events::file_log("agent", &format!("step{step} {line}"));
            history.push(line);
            if invalid >= 4 {
                return false;
            }
            continue;
        }

        let mut action = action;
        if let Some(index) = action.index
            && matches!(action.action, ActionKind::ClickOption)
            && !suggested.is_empty()
        {
            if action.doubt.unwrap_or(false) && !suggested.contains(&index) {
                // Doubtful reading: the pre-joined answer wins for this question.
                let exists = snapshot
                    .get("options")
                    .and_then(|o| o.as_array())
                    .map(|list| {
                        list.iter().any(|o| {
                            o.get("index").and_then(|i| i.as_u64()) == Some(suggested[0] as u64)
                        })
                    })
                    .unwrap_or(false);
                if exists {
                    events::file_log(
                        "agent",
                        &format!("ragu: {index} -> {} dari jawaban awal", suggested[0]),
                    );
                    action.index = Some(suggested[0]);
                }
            } else if !suggested.contains(&index) {
                events::file_log(
                    "agent",
                    &format!(
                        "menyimpang dari jawaban awal: pilih {index}, seharusnya {suggested:?}"
                    ),
                );
            }
        }
        let signature = format!(
            "{:?}:{:?}:{:?}:{:?}",
            action.action, action.index, action.selector, action.text
        );
        if signature == last_action {
            history.push(format!("ulangi {signature} (pastikan halaman berubah)"));
        }
        last_action = signature.clone();

        events::file_log(
            "agent",
            &format!(
                "step{step} {:?} idx={:?} sel={:?} ({})",
                action.action,
                action.index,
                action.selector,
                action.reason.clone().unwrap_or_default()
            ),
        );
        let finished = apply_action(browser, &action, store, metrics);
        if matches!(action.action, ActionKind::Done) {
            *done_flag = true;
        }
        history.push(format!("{signature} -> dieksekusi"));
        std::thread::sleep(Duration::from_millis(500));
        if finished {
            return true;
        }
        if let Ok(state) = dom::snapshot(&mut browser.mn) {
            if state.feedback_visible || state.next_button.exists {
                return true;
            }
            if before.is_some() && state.effective_id().as_deref() != before.as_deref() {
                return true;
            }
        }
    }
    false
}

fn validate_action(snapshot: &serde_json::Value, action: &AgentAction) -> Result<(), String> {
    match action.action {
        ActionKind::ClickOption => {
            let index = action.index.ok_or("index kosong")?;
            let exists = snapshot
                .get("options")
                .and_then(|o| o.as_array())
                .map(|list| {
                    list.iter()
                        .any(|o| o.get("index").and_then(|i| i.as_u64()) == Some(index as u64))
                })
                .unwrap_or(false);
            if exists {
                Ok(())
            } else {
                Err(format!("opsi {index} tidak ada"))
            }
        }
        ActionKind::Type => {
            let selector = action.selector.as_deref().ok_or("selector kosong")?;
            let exists = snapshot
                .get("inputs")
                .and_then(|i| i.as_array())
                .map(|list| {
                    list.iter()
                        .any(|i| i.get("selector").and_then(|s| s.as_str()) == Some(selector))
                })
                .unwrap_or(false);
            if exists {
                Ok(())
            } else {
                Err(format!("input {selector} tidak ada"))
            }
        }
        ActionKind::Submit => {
            let exists = snapshot
                .pointer("/submit/exists")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let disabled = snapshot
                .pointer("/submit/disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !exists {
                Err("tombol submit tidak ada".into())
            } else if disabled {
                Err("submit masih nonaktif".into())
            } else {
                Ok(())
            }
        }
        ActionKind::Next => {
            let exists = snapshot
                .pointer("/next/exists")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if exists {
                Ok(())
            } else {
                Err("tombol next tidak ada".into())
            }
        }
        ActionKind::ClickText => {
            if action.text.as_deref().unwrap_or("").trim().is_empty() {
                Err("text kosong".into())
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn apply_action(
    browser: &mut Browser,
    action: &AgentAction,
    store: &Store,
    metrics: &mut Metrics,
) -> bool {
    match action.action {
        ActionKind::Done => true,
        ActionKind::Wait => false,
        ActionKind::ClickOption => match action.index {
            Some(index) => {
                let _ = quiz::select_option(browser, index);
                false
            }
            None => false,
        },
        ActionKind::ClickText => {
            if let Some(text) = &action.text {
                let _ = browser.mn.script(
                    "const t=arguments[0].toLowerCase(); const el=[...document.querySelectorAll('button,a,[role=button]')].find(e=>((e.innerText||'').trim().toLowerCase())===t && !e.disabled); if(el){el.click(); return true;} return false;",
                    json!([text.to_lowercase()]),
                );
            }
            false
        }
        ActionKind::Type => {
            let text = action.text.clone().unwrap_or_default();
            let selector = action.selector.clone().unwrap_or_default();
            if selector.contains("ProseMirror") {
                let _ = crate::browser::input::type_contenteditable(&mut browser.mn, &selector, &text);
            } else if !selector.is_empty() {
                let _ = crate::browser::input::type_selector(&mut browser.mn, &selector, &text);
            }
            false
        }
        ActionKind::Submit => {
            if let Ok(true) = browser
                .mn
                .script(
                    "const b=document.querySelector('button[data-cy=submit-button]'); if(b && !b.disabled){b.click(); return true;} return false;",
                    json!([]),
                )
                .map(|v| v.as_bool().unwrap_or(false))
            {
                return false;
            }
            false
        }
        ActionKind::Next => {
            let _ = quiz::click_next(browser);
            true
        }
        ActionKind::Dismiss => {
            let _ = dom::dismiss_login(&mut browser.mn);
            let _ = dom::dismiss_obstacles(&mut browser.mn);
            false
        }
    }
    .then_some(())
    .map(|_| action.action == ActionKind::Done || action.action == ActionKind::Next)
    .unwrap_or(false)
    || matches!(action.action, ActionKind::Done | ActionKind::Next)
        || {
            if matches!(action.action, ActionKind::Submit | ActionKind::ClickOption | ActionKind::Type) {
                let _ = metrics;
                let _ = store;
            }
            false
        }
}

fn mode_label(state: &PageState) -> String {
    if state.next_button.exists {
        "review+next".into()
    } else if state.feedback_visible {
        "feedback".into()
    } else if state.interstitial.is_some() {
        "interstisial".into()
    } else {
        "auto".into()
    }
}

fn question_from_state(state: &PageState) -> Question {
    let options: Vec<crate::model::OptionItem> = state
        .options
        .iter()
        .map(|o| crate::model::OptionItem {
            id: format!("dom-{}", o.index),
            text: o.text.clone(),
            media: vec![],
            dom_index: Some(o.index),
        })
        .collect();
    let kind = if state
        .question_type_hint
        .as_deref()
        .unwrap_or("")
        .contains("is-msq")
        || state.options.iter().any(|o| o.msq)
    {
        QuestionType::Msq
    } else if state.inputs.iter().any(|i| i.editable) && options.is_empty() {
        if state.inputs.iter().any(|i| i.tag == "DIV") {
            QuestionType::Open
        } else {
            QuestionType::Blank
        }
    } else {
        QuestionType::Mcq
    };
    let mut question = Question {
        id: state.effective_id().unwrap_or_default(),
        type_: kind,
        text: state.question_text.clone(),
        explain: state.explanation.clone(),
        media: state.media.clone(),
        options,
        targets: vec![],
        time_ms: 30_000,
        hash: String::new(),
        raw: serde_json::Value::Null,
    };
    question.recompute_hash();
    question
}

fn wait_start(
    browser: &mut Browser,
    rt: &mut Runtime,
    tx: &Tx,
    _questions: &[Question],
) -> Result<bool, String> {
    let deadline = if rt.wait_max_min == 0 {
        None
    } else {
        Some(Instant::now() + Duration::from_secs(rt.wait_max_min * 60))
    };
    let mut last_beat = Instant::now();
    loop {
        if rt.aborted.load(Ordering::Relaxed) {
            return Ok(false);
        }
        if let Some(deadline) = deadline
            && Instant::now() > deadline
        {
            return Ok(false);
        }
        if last_beat.elapsed() > Duration::from_secs(30) {
            last_beat = Instant::now();
            events::log(tx, Level::Info, "masih menunggu host…");
        }
        if let Some(label) = dom::dismiss_obstacles(&mut browser.mn) {
            events::file_log("obstacle", &label);
        }
        let state = match dom::snapshot(&mut browser.mn) {
            Ok(state) => state,
            Err(_) => {
                let _ = browser.reconnect();
                std::thread::sleep(Duration::from_millis(800));
                continue;
            }
        };
        browser.tick_focus(&mut rt.metrics, false);
        let _ = tx.send(Event::Metrics(rt.metrics.summary()));
        if state.url.contains("/pre-game/") {
            if rt.resume {
                let _ = crate::wayground::join::click_text_button(
                    browser,
                    &["resume attempt", "resume", "start", "lanjut"],
                );
            } else {
                let _ = crate::wayground::join::click_text_button(browser, &["start", "lanjut"]);
            }
        }
        if state.has_question() || state.finished {
            events::log(tx, Level::Info, "quiz dimulai");
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn wait_ack(browser: &mut Browser, qid: &str, metrics: &mut Metrics) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        browser.tick_focus(metrics, false);
        if let Ok(state) = dom::snapshot(&mut browser.mn)
            && (state.question_id.as_deref() != Some(qid)
                || state.feedback_visible
                || state.next_button.exists
                || (state.submit_button.exists
                    && state.submit_button.disabled
                    && state.options.iter().all(|o| o.selected)))
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn advance(
    browser: &mut Browser,
    qid: &str,
    metrics: &mut Metrics,
    store: &Store,
    aborted: &Arc<AtomicBool>,
) {
    let mut retries = 0u8;
    let mut last_ack = Instant::now();
    let mut clicked = 0u8;
    loop {
        if aborted.load(Ordering::Relaxed) {
            return;
        }
        let Ok(state) = dom::snapshot(&mut browser.mn) else {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        };
        browser.tick_focus(metrics, false);
        let changed = state.question_id.as_deref() != Some(qid);
        let action = quiz::decide_advance(
            &state,
            true,
            changed,
            retries,
            last_ack.elapsed().as_millis() as u64,
        );
        match action {
            quiz::AdvanceAction::AutoAdvanced | quiz::AdvanceAction::Finished => {
                store.append_event("advance", json!({"qid": qid, "kind": "auto"}));
                return;
            }
            quiz::AdvanceAction::ClickNext | quiz::AdvanceAction::RetryNext => {
                if clicked >= 3 {
                    store.append_event("advance", json!({"qid": qid, "kind": "stuck"}));
                    return;
                }
                clicked += 1;
                if quiz::click_next(browser).is_ok() {
                    store.append_event("advance", json!({"qid": qid, "kind": "click"}));
                    let changed_at = Instant::now();
                    while changed_at.elapsed() < Duration::from_secs(5) {
                        if let Ok(s) = dom::snapshot(&mut browser.mn)
                            && s.question_id.as_deref() != Some(qid)
                        {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    retries += 1;
                    last_ack = Instant::now();
                } else {
                    clicked -= 1;
                    retries += 1;
                    last_ack = Instant::now();
                }
            }
            quiz::AdvanceAction::HandleDialog => {
                let _ = browser.mn.script(
                    "const b=[...document.querySelectorAll('button')].find(x=>/^(ok|yes|ya|continue)$/i.test((x.innerText||'').trim())); if(b)b.click(); return !!b;",
                    json!([]),
                );
                std::thread::sleep(Duration::from_millis(500));
            }
            quiz::AdvanceAction::Wait => {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        // Never spin forever on a room that stopped sending questions.
        if last_ack.elapsed() > Duration::from_secs(45) {
            return;
        }
    }
}

fn last_session_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join("last-session.json")
}

fn remember_session(root: &std::path::Path, config: &SessionConfig, url: &str) {
    if !(url.contains("/join/game/") || url.contains("/join/pre-game/")) {
        return;
    }
    let value = json!({
        "room": config.room,
        "name": config.name,
        "url": url,
        "t": crate::store::now_ms()
    });
    let _ = std::fs::write(last_session_path(root), value.to_string());
}

fn try_resume(
    browser: &mut Browser,
    config: &SessionConfig,
    root: &std::path::Path,
    tx: &Tx,
    metrics: &mut Metrics,
) -> bool {
    let Ok(text) = std::fs::read_to_string(last_session_path(root)) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let room_ok = value.get("room").and_then(|v| v.as_str()) == Some(config.room.as_str());
    let fresh = value
        .get("t")
        .and_then(|v| v.as_u64())
        .map(|t| crate::store::now_ms().saturating_sub(t) < 15 * 60 * 1000)
        .unwrap_or(false);
    let url = value.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let playable = url.contains("/join/game/") || url.contains("/join/pre-game/");
    if !room_ok || !fresh || !playable {
        return false;
    }
    events::log(tx, Level::Info, format!("resume sesi sebelumnya: {url}"));
    if browser.navigate(url).is_err() {
        return false;
    }
    let path = browser
        .mn
        .script("return String(location.pathname);", json!([]))
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    if path == "/join" {
        return false;
    }
    browser.enter_fullscreen();
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(state) = dom::snapshot(&mut browser.mn) {
            if state.dialog_text.to_lowercase().contains("already exists")
                || state
                    .body_excerpt
                    .to_lowercase()
                    .contains("incomplete game already exists")
            {
                let _ = browser.mn.script(
                    r#"const b=[...document.querySelectorAll('button')].find(x=>/^\s*yes\s*$/i.test((x.innerText||'').trim())); if(b)b.click(); return !!b;"#,
                    json!([]),
                );
            }
            if state.waiting_for_host || state.has_question() {
                events::log(tx, Level::Info, "resume berhasil");
                return true;
            }
            if state.error_text.to_lowercase().contains("room not found") {
                return false;
            }
        }
        browser.tick_focus(metrics, false);
        std::thread::sleep(Duration::from_millis(400));
    }
    false
}

fn summary(rt: &Runtime, answered: u32, total: u32) -> Summary {
    Summary {
        answered,
        total,
        correct: 0,
        unknown: 0,
        elapsed_ms: 0,
        metrics: rt.metrics.summary(),
        session_dir: rt.store.dir().display().to_string(),
    }
}

/// Exposed for the TUI footer.
pub fn thinking_label(t: Thinking) -> &'static str {
    match t {
        Thinking::Auto => "auto",
        Thinking::On => "on",
        Thinking::Off => "off",
    }
}

pub fn _dom_option_default() -> DomOption {
    DomOption::default()
}
