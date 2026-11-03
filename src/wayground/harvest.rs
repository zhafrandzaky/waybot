//! Question harvesting and answer-key extraction.

use serde_json::Value;

use crate::browser::Browser;
use crate::events::{Level, Tx, log};
use crate::model::{Answer, AnswerSource, Media, OptionItem, Question, QuestionType, Target};
use crate::util::{html_to_text, truncate_chars};
use crate::wayground::urls;

#[derive(Debug, thiserror::Error)]
pub enum HarvestError {
    #[error("roomHash tidak ditemukan")]
    NoRoomHash,
    #[error("getQuestions gagal: {0}")]
    Questions(String),
    #[error("daftar soal kosong")]
    Empty,
    #[error("respons tidak dikenali")]
    Shape,
}

pub struct Harvest {
    pub questions: Vec<Question>,
    pub answers: Vec<Answer>,
    pub keyed: u32,
    pub explained: u32,
}

/// Pull the network log recorded by the preload extension.
fn net_log(browser: &mut Browser) -> Vec<Value> {
    let value = browser
        .mn
        .script(
            "return (window.__waybotNet || []).slice(-300);",
            serde_json::json!([]),
        )
        .ok()
        .unwrap_or(Value::Array(vec![]));
    serde_json::from_value(value).unwrap_or_default()
}

fn find_field(text: &str, key: &str) -> Option<String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    find_first_json(&compact, &format!("\"{key}\":\""))
}

fn find_first_json(body: &str, needle: &str) -> Option<String> {
    let idx = body.find(needle)?;
    let rest = &body[idx + needle.len()..];
    let end = rest.find(['"', ',', '}']).unwrap_or(rest.len());
    let value = &rest[..end];
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub fn join_identifiers(
    browser: &mut Browser,
    code: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let entries = net_log(browser);
    let _ = std::fs::write(
        "/tmp/opencode/netlog.json",
        serde_json::to_string_pretty(&entries).unwrap_or_default(),
    );
    let mut hash = None;
    let mut player = None;
    let mut quiz = None;
    for entry in entries {
        let url = entry.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let body = entry.get("body").and_then(|b| b.as_str()).unwrap_or("");
        let res = entry.get("res").and_then(|b| b.as_str()).unwrap_or("");
        if url.contains("checkRoom") && hash.is_none() {
            hash = find_first_json(res, "\"hash\":\"");
        }
        if (url.contains("/v5/join") || url.contains("/v5/rejoin"))
            && player.is_none()
                && let Ok(value) = serde_json::from_str::<Value>(body) {
                    player = value
                        .pointer("/player/id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                }
        if quiz.is_none() {
            for text in [res, body] {
                if let Some(found) = find_field(text, "quizId")
                    && found.len() == 24 && found.chars().all(|c| c.is_ascii_hexdigit()) {
                        quiz = Some(found);
                        break;
                    }
            }
        }
    }
    // checkRoom is unauthenticated; call it from the page if the hook missed it.
    if hash.is_none() {
        let body = serde_json::json!({"roomCode": code}).to_string();
        if let Some(value) = browser.fetch_json(&urls::check_room(), "POST", Some(&body)) {
            hash = value
                .pointer("/room/hash")
                .and_then(|h| h.as_str())
                .map(str::to_string);
        }
    }
    (hash, player, quiz)
}

pub fn harvest(
    browser: &mut Browser,
    code: &str,
    name: &str,
    tx: &Tx,
) -> Result<Harvest, HarvestError> {
    let (hash, player, mut quiz_id) = join_identifiers(browser, code);
    let hash = hash.ok_or(HarvestError::NoRoomHash)?;
    let player_id = player.unwrap_or_else(|| name.to_string());
    log(
        tx,
        Level::Info,
        format!("roomHash {hash} · player {player_id}"),
    );

    if quiz_id.is_none()
        && let Some(found) = decrypt_quiz_id(browser) {
            log(tx, Level::Info, format!("quizId dari URL: {found}"));
            quiz_id = Some(found);
        }
    if quiz_id.is_none() {
        for path in [
            format!("{}/_gameapi/main/public/v1/games/{hash}/quiz", urls::ORIGIN),
            format!(
                "{}/_gameapi/main/public/v1/games/{hash}/questions",
                urls::ORIGIN
            ),
            format!("{}/_quizserver/main/v2/quiz/{hash}", urls::ORIGIN),
            format!("{}/_quizserver/main/v2/quiz-by-game/{hash}", urls::ORIGIN),
        ] {
            if let Some((status, text)) = browser.fetch_raw(&path, "GET", None) {
                log(
                    tx,
                    Level::Info,
                    format!(
                        "probe {status} {path} :: {}",
                        crate::util::truncate_chars(&text, 160)
                    ),
                );
            }
        }
        if let Some(info) = browser.fetch_json(&urls::game_info(&hash), "GET", None) {
            let text = info.to_string();
            quiz_id = find_field(&text, "quizId")
                .or_else(|| {
                    info.pointer("/room/quizId")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .or_else(|| {
                    info.pointer("/quiz/_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                });
        }
    }
    log(tx, Level::Info, format!("quizId {:?}", quiz_id));

    let body = serde_json::json!({"roomHash": hash, "playerId": player_id}).to_string();
    let payload = browser
        .fetch_json(&urls::get_questions(), "POST", Some(&body))
        .ok_or_else(|| HarvestError::Questions("tidak ada respons".into()))?;

    let raw_map = payload
        .get("questions")
        .or_else(|| payload.pointer("/data/questions"))
        .ok_or(HarvestError::Shape)?;

    let mut questions = parse_questions(raw_map)?;
    if questions.is_empty() {
        return Err(HarvestError::Empty);
    }

    // Answer keys come from the authenticated quiz document.
    let key_map = quiz_id
        .as_deref()
        .and_then(|id| browser.fetch_json(&urls::quiz_data(id), "GET", None))
        .map(parse_key_map)
        .unwrap_or_default();

    let mut answers = Vec::new();
    let mut keyed = 0;
    let mut explained = 0;
    for question in &mut questions {
        question.recompute_hash();
        if let Some(values) = key_map.get(&question.id)
            && !values.is_empty()
        {
            keyed += 1;
            let mut answer = Answer::new(&question.id, values.clone(), AnswerSource::Key);
            answer.confidence = 0.99;
            answers.push(answer);
            continue;
        }
        if let Some(answer) = crate::solver::from_explain(question) {
            explained += 1;
            answers.push(answer);
        }
    }

    log(
        tx,
        Level::Info,
        format!(
            "{} soal diambil; kunci {} · explain {}",
            questions.len(),
            keyed,
            explained
        ),
    );
    Ok(Harvest {
        questions,
        answers,
        keyed,
        explained,
    })
}

/// The game URL carries an AES blob (key "game", fields split by "|-$"): roomHash, playerId, quizId.
fn decrypt_quiz_id(browser: &mut Browser) -> Option<String> {
    crate::events::file_log("quizid", "mulai decrypt");
    let script = r#"
const done = arguments[arguments.length - 1];
(async () => {
  try {
    const segs = location.pathname.split('/').filter(Boolean);
    let blob = segs.map(decodeURIComponent).find((s) => s.length > 40 && !/^(running|start|pre-game|game|join)$/.test(s));
    if (!blob) return done('NOBLOB:' + location.pathname);
    if (/%[0-9A-Fa-f]{2}/.test(blob)) { try { blob = decodeURIComponent(blob); } catch (e) {} }
    const mod = await import('https://cf.quizizz.com/app-bundle/join/upload/js/crypto-js.CuYbf-_W.js');
    const C = mod.default || mod;
    const aes = C.A || (C.default && C.default.A);
    if (!aes) return done('ERR:no-aes');
    const hex = aes.decrypt(blob, 'game').toString();
    done('HEX:' + hex);
  } catch (e) { done('ERR:' + String(e)); }
})();
"#;
    let value = match browser.mn.async_script(script, serde_json::json!([])) {
        Ok(value) => value,
        Err(e) => {
            crate::events::file_log("quizid", &format!("script: {e}"));
            return None;
        }
    };
    crate::events::file_log("quizid", &format!("hasil: {value:?}"));
    let text = match value.as_str() {
        Some(text) => text,
        None => {
            crate::events::file_log("quizid", &format!("nilai: {value}"));
            return None;
        }
    };
    if !text.starts_with("HEX:") {
        crate::events::file_log("quizid", text);
        return None;
    }
    let bytes = match hex::decode(&text[4..]) {
        Ok(bytes) => bytes,
        Err(e) => {
            crate::events::file_log("quizid", &format!("hex: {e} len={}", text.len()));
            return None;
        }
    };
    let plain = match String::from_utf8(bytes) {
        Ok(plain) => plain,
        Err(_) => {
            crate::events::file_log("quizid", "bukan utf8");
            return None;
        }
    };
    let parts: Vec<&str> = plain.split("|-$").collect();
    let quiz = parts.get(2).map(|s| s.trim().to_string())?;
    if quiz.len() == 24 && quiz.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(quiz)
    } else {
        crate::events::file_log("quizid", &format!("parts={parts:?}"));
        None
    }
}

/// Map of question id -> correct option text(s) from quiz data.
pub fn fetch_key_map(
    browser: &mut Browser,
    tx: &Tx,
) -> std::collections::HashMap<String, Vec<String>> {
    let Some(quiz_id) = decrypt_quiz_id(browser) else {
        return Default::default();
    };
    log(tx, Level::Info, format!("quizId {quiz_id}"));
    match browser.fetch_json(&urls::quiz_data(&quiz_id), "GET", None) {
        Some(payload) => parse_key_map(payload),
        None => {
            log(
                tx,
                Level::Warn,
                format!("quiz data {quiz_id} gagal diambil"),
            );
            Default::default()
        }
    }
}

fn parse_questions(raw: &Value) -> Result<Vec<Question>, HarvestError> {
    let items: Vec<&Value> = match raw {
        Value::Object(map) => map.values().collect(),
        Value::Array(list) => list.iter().collect(),
        _ => return Err(HarvestError::Shape),
    };
    let mut out = Vec::new();
    for item in items {
        let id = item
            .get("_id")
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let structure = item.get("structure").cloned().unwrap_or(Value::Null);
        let type_raw = item
            .get("type")
            .and_then(|t| t.as_str())
            .or_else(|| structure.get("kind").and_then(|k| k.as_str()))
            .unwrap_or("");
        let text = structure
            .pointer("/query/text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let explain = structure
            .pointer("/explain/text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let media = parse_media(structure.pointer("/query/media"));
        let options = structure
            .get("options")
            .and_then(|o| o.as_array())
            .map(|list| {
                list.iter()
                    .enumerate()
                    .map(|(idx, opt)| OptionItem {
                        id: opt
                            .get("id")
                            .or_else(|| opt.get("_id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        text: opt
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string(),
                        media: parse_media(opt.get("media")),
                        dom_index: Some(idx as u32),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let targets = structure
            .get("targets")
            .and_then(|t| t.as_array())
            .map(|list| {
                list.iter()
                    .map(|target| Target {
                        id: target
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        option_ids: target
                            .get("optionId")
                            .and_then(|o| o.as_array())
                            .map(|ids| {
                                ids.iter()
                                    .filter_map(|i| i.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let time_ms = item.get("time").and_then(|t| t.as_u64()).unwrap_or(30_000);
        out.push(Question {
            id,
            type_: QuestionType::parse(type_raw),
            text: html_to_text(&text),
            explain: html_to_text(&explain),
            media,
            options,
            targets,
            time_ms,
            hash: String::new(),
            raw: item.clone(),
        });
    }
    Ok(out)
}

fn parse_media(raw: Option<&Value>) -> Vec<Media> {
    raw.and_then(|m| m.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|m| {
                    let src = m.get("src").or_else(|| m.get("url"))?.as_str()?.to_string();
                    Some(Media {
                        src,
                        alt: m
                            .get("alt")
                            .and_then(|a| a.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_key_map(payload: Value) -> std::collections::HashMap<String, Vec<String>> {
    let mut map = std::collections::HashMap::new();
    let questions = payload
        .pointer("/data/quiz/info/questions")
        .or_else(|| payload.pointer("/quiz/info/questions"))
        .or_else(|| payload.get("questions"));
    let Some(questions) = questions else {
        return map;
    };
    let items: Vec<&Value> = match questions {
        Value::Array(list) => list.iter().collect(),
        Value::Object(obj) => obj.values().collect(),
        _ => return map,
    };
    for item in items {
        let Some(id) = item.get("_id").and_then(|i| i.as_str()) else {
            continue;
        };
        let structure = item.get("structure").cloned().unwrap_or(Value::Null);
        let options = structure
            .get("options")
            .and_then(|o| o.as_array())
            .cloned()
            .unwrap_or_default();
        let text_at = |idx: usize| -> Option<String> {
            options
                .get(idx)
                .and_then(|o| o.get("text"))
                .and_then(|t| t.as_str())
                .map(html_to_text)
        };
        let by_id = |option_id: &str| -> Option<String> {
            options
                .iter()
                .find(|o| {
                    o.get("id").and_then(|i| i.as_str()) == Some(option_id)
                        || o.get("_id").and_then(|i| i.as_str()) == Some(option_id)
                })
                .and_then(|o| o.get("text"))
                .and_then(|t| t.as_str())
                .map(html_to_text)
        };
        let answer = structure.get("answer");
        let values: Vec<String> = match answer {
            Some(Value::Number(n)) => text_at(n.as_u64().unwrap_or(0) as usize)
                .into_iter()
                .collect(),
            Some(Value::Array(list)) => {
                let mut out = Vec::new();
                for entry in list {
                    match entry {
                        Value::Number(n) => {
                            if let Some(text) = text_at(n.as_u64().unwrap_or(0) as usize) {
                                out.push(text);
                            }
                        }
                        Value::Object(obj) => {
                            if let Some(ids) = obj.get("optionId").and_then(|o| o.as_array()) {
                                for id in ids {
                                    if let Some(text) = id.as_str().and_then(by_id) {
                                        out.push(text);
                                    }
                                }
                            }
                        }
                        Value::String(s) => out.push(s.clone()),
                        _ => {}
                    }
                }
                out
            }
            Some(Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        };
        if !values.is_empty() {
            map.insert(id.to_string(), values);
        }
    }
    map
}

#[allow(dead_code)]
pub fn describe(question: &Question) -> String {
    truncate_chars(&question.plain_text(), 80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_map_reads_numbers_and_blank_ids() {
        let payload = serde_json::json!({"data":{"quiz":{"info":{"questions":[
            {"_id":"q1","structure":{"answer":1,"options":[{"id":"a","text":"A"},{"id":"b","text":"B"}]}},
            {"_id":"q2","structure":{"answer":[{"targetId":"t","optionId":["o2"]}],"options":[{"id":"o1","text":"X"},{"id":"o2","text":"Y"}]}}
        ]}}}});
        let map = parse_key_map(payload);
        assert_eq!(map.get("q1").unwrap(), &vec!["B".to_string()]);
        assert_eq!(map.get("q2").unwrap(), &vec!["Y".to_string()]);
    }

    #[test]
    fn question_parsing_handles_dict_shape() {
        let raw = serde_json::json!({"q1":{"_id":"q1","type":"MCQ","time":30000,"structure":{
            "query":{"text":"<p>2+2?</p>","media":[]},
            "options":[{"id":"a","text":"3"},{"id":"b","text":"4"}],
            "explain":{"text":"Empat."}
        }}});
        let parsed = parse_questions(&raw).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].plain_text(), "2+2?");
        assert_eq!(parsed[0].options.len(), 2);
        assert_eq!(parsed[0].explain, "Empat.");
    }
}
