//! Page agent: the model reads the live page and picks the next action.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::{Answer, Question};
use crate::solver::deepseek::{Solver, SolverError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    ClickOption,
    ClickText,
    Type,
    Submit,
    Next,
    Dismiss,
    Wait,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub action: ActionKind,
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub doubt: Option<bool>,
}

const SYSTEM: &str = "Kamu operator UI kuis. Baca snapshot halaman dan pilih SATU aksi berikutnya. \
Index opsi mengikuti atribut data-cy dan bisa tidak berurutan; pakai index yang benar-benar ada di snapshot. Balas hanya JSON: {\"action\":\"...\",\"index\":n,\"text\":\"...\",\"selector\":\"...\",\"reason\":\"...\"}. \
Aksi: click_option (pakai index opsi), click_text (pakai text tombol), type (pakai selector + text), \
submit, next, dismiss, wait, done. Dasarnya baca screenshot. Isi \"doubt\": true HANYA jika bacaan screenshot meragukan (gambar buram/kecil, teks terpotong, opsi tidak jelas). Jika doubt true, pakai index opsi yang cocok dengan jawaban awal; jika bacaan jelas, ikuti screenshot. Aturan: isi/pilih dulu, baru submit; MCQ biasanya auto-submit \
tanpa tombol; kalau jawaban sudah terkirim dan ada Next, klik next; jangan ulangi aksi yang sudah \
tampak berhasil; jangan menebak selector acak; kalau halaman belum siap, wait.";

pub async fn plan(
    solver: &Solver,
    snapshot: &Value,
    question: Option<&Question>,
    answer: Option<&Answer>,
    history: &[String],
    images: &[String],
    suggested: &[u32],
) -> Result<AgentAction, SolverError> {
    let mut user = String::new();
    if !history.is_empty() {
        user.push_str("Riwayat aksi (terbaru di bawah):\n");
        for line in history.iter().rev().take(6).rev() {
            user.push_str(&format!("- {line}\n"));
        }
        user.push_str("Jangan ulangi aksi yang sudah berhasil; kalau aksi terakhir gagal, pilih cara lain.\n\n");
    }
    user.push_str("Snapshot halaman:\n");
    user.push_str(&snapshot.to_string());
    if let Some(question) = question {
        user.push_str(&format!(
            "\n\nSoal target: [{}] {}\n",
            question.type_.label(),
            question.text
        ));
        if !question.options.is_empty() {
            user.push_str("Opsi API berindeks:\n");
            for (idx, opt) in question.options.iter().enumerate() {
                user.push_str(&format!("{idx}. {}\n", opt.text));
            }
        }
    }
    if let Some(answer) = answer {
        user.push_str(&format!(
            "\nJawaban yang sudah disiapkan {} (sumber {}, keyakinan {:.2}): {}\n",
            if answer.source.label() == "key" || answer.source.label() == "explain" {
                "RESMI"
            } else {
                ""
            },
            answer.source.label(),
            answer.confidence,
            answer.values.join(" | ")
        ));
    }
    if !suggested.is_empty() {
        user.push_str(&format!(
            "Index opsi yang cocok dengan jawaban awal: {suggested:?} (hanya dipakai bila doute=true).\n"
        ));
    }
    let content = if images.is_empty() {
        json!(user)
    } else {
        let mut parts = vec![json!({"type": "text", "text": user})];
        for image in images {
            parts.push(json!({"type": "image_url", "image_url": {"url": image}}));
        }
        json!(parts)
    };
    let model = if images.is_empty() {
        solver.model_id()
    } else {
        solver.vision_or_text()
    };
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": content}
        ],
        "temperature": 0.1,
        "max_tokens": 900,
        "thinking": {"type": "disabled"},
        "response_format": {"type": "json_object"}
    });
    let payload = solver.raw_chat(&body).await?;
    let content = payload
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .ok_or_else(|| SolverError::Invalid("tanpa content".into()))?;
    let raw = crate::solver::prompt::extract_json(content)
        .ok_or_else(|| SolverError::Invalid("bukan JSON".into()))?;
    serde_json::from_str(raw).map_err(|e| SolverError::Invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_parses_from_json() {
        let action: AgentAction =
            serde_json::from_str(r#"{"action":"click_option","index":2,"reason":"pilih"}"#)
                .unwrap();
        assert_eq!(action.action, ActionKind::ClickOption);
        assert_eq!(action.index, Some(2));
    }

    #[test]
    fn unknown_action_fails() {
        assert!(serde_json::from_str::<AgentAction>(r#"{"action":"drag"}"#).is_err());
    }
}
