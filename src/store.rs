//! Per-session storage. Atomic writes, secrets never persisted.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::model::{Answer, Question};
use crate::util::truncate_chars;

#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn create(room: &str, name: &str) -> io::Result<Self> {
        let root = data_root()?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let dir = root
            .join("sessions")
            .join(format!("{}-{stamp}", sanitize(room)));
        fs::create_dir_all(dir.join("screenshots"))?;
        fs::create_dir_all(dir.join("snapshots"))?;
        let store = Self { dir };
        store.write_json(
            "meta.json",
            &serde_json::json!({
                "room": room,
                "name": truncate_chars(name, 60),
                "started_at": stamp,
                "app_version": env!("CARGO_PKG_VERSION")
            }),
        )?;
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn write_json<T: Serialize>(&self, name: &str, value: &T) -> io::Result<()> {
        let target = self.dir.join(name);
        let tmp = target.with_extension("tmp");
        let data = serde_json::to_vec_pretty(value)?;
        fs::write(&tmp, data)?;
        fs::rename(tmp, target)
    }

    pub fn write_text(&self, name: &str, text: &str) -> io::Result<()> {
        let target = self.dir.join(name);
        let tmp = target.with_extension("tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, target)
    }

    pub fn append_event(&self, kind: &str, payload: serde_json::Value) {
        let line = serde_json::json!({
            "t": now_ms(),
            "kind": kind,
            "payload": payload
        });
        let path = self.dir.join("events.ndjson");
        if let Ok(mut file) = File::options().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }

    pub fn save_questions(&self, questions: &[Question]) -> io::Result<()> {
        self.write_json("questions.raw.json", &questions)
    }

    pub fn save_answers(&self, answers: &[Answer]) -> io::Result<()> {
        let owned = answers.to_vec();
        self.write_json("answers.json", &owned)?;
        let mut md = String::from("# Jawaban\n\n");
        for answer in answers {
            md.push_str(&format!(
                "- `{}` [{} {:.2}] {}\n",
                answer.question_id,
                answer.source.label(),
                answer.confidence,
                answer.values.join(" | ")
            ));
        }
        self.write_text("answers.md", &md)
    }

    pub fn save_screenshot(&self, label: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let safe = format!("{}-{}.png", now_ms(), sanitize(label));
        let path = self.dir.join("screenshots").join(safe);
        fs::write(&path, bytes)?;
        Ok(path)
    }

    pub fn save_snapshot(&self, label: &str, value: &serde_json::Value) -> io::Result<()> {
        let safe = format!("{}-{}.json", now_ms(), sanitize(label));
        self.write_json(&format!("snapshots/{safe}"), value)
    }
}

pub fn data_root() -> io::Result<PathBuf> {
    if let Ok(dir) = std::env::var("WAYBOT_DATA_DIR") {
        let path = PathBuf::from(dir);
        fs::create_dir_all(&path)?;
        return Ok(path);
    }
    let home = std::env::var("HOME").map_err(|_| io::Error::other("HOME tidak ada"))?;
    let path = PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("waybot");
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
