//! OpenAI-compatible chat client (Venice / DeepSeek), provider-agnostic.

use std::time::Duration;

use serde_json::{Value, json};

use crate::config::Thinking;
use crate::model::{Answer, AnswerSource, Question};
use crate::solver::prompt;

#[derive(Debug, thiserror::Error)]
pub enum SolverError {
    #[error("API key solver belum diisi")]
    MissingKey,
    #[error("HTTP {0}")]
    Http(u16),
    #[error("jaringan: {0}")]
    Network(String),
    #[error("respons model tidak valid: {0}")]
    Invalid(String),
}

pub struct Solver {
    client: reqwest::Client,
    base_url: String,
    key: String,
    model: String,
    vision_model: String,
    thinking: Thinking,
    reasoning_effort: Option<String>,
    temperature: f32,
}

/// Capability probe result for the model list.
pub type ModelList = Vec<String>;

impl Solver {
    pub fn new(
        base_url: &str,
        key: &str,
        model: &str,
        vision_model: &str,
        thinking: Thinking,
        reasoning_effort: Option<String>,
        temperature: f32,
    ) -> Result<Self, SolverError> {
        if key.trim().is_empty() {
            return Err(SolverError::MissingKey);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .user_agent("waybot/0.1")
            .build()
            .map_err(|e| SolverError::Network(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            key: key.to_string(),
            model: model.to_string(),
            vision_model: vision_model.to_string(),
            thinking,
            reasoning_effort,
            temperature,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model
    }

    /// Model for vision calls; falls back to the text model.
    pub fn vision_or_text(&self) -> &str {
        if self.vision_model.is_empty() {
            &self.model
        } else {
            &self.vision_model
        }
    }

    /// Raw chat call used by the page agent.
    pub async fn raw_chat(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, SolverError> {
        let resp = self
            .client
            .post(self.url("/chat/completions"))
            .bearer_auth(&self.key)
            .json(body)
            .send()
            .await
            .map_err(|e| SolverError::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(SolverError::Invalid(format!(
                "{status}: {}",
                crate::util::truncate_chars(&text, 200)
            )));
        }
        resp.json()
            .await
            .map_err(|e| SolverError::Invalid(e.to_string()))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub async fn list_models(&self) -> Result<ModelList, SolverError> {
        let resp = self
            .client
            .get(self.url("/models"))
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(|e| SolverError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(SolverError::Http(resp.status().as_u16()));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| SolverError::Invalid(e.to_string()))?;
        let mut out = Vec::new();
        if let Some(items) = body.get("data").and_then(|d| d.as_array()) {
            for item in items {
                if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                    out.push(id.to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn build_body(
        &self,
        question: &Question,
        media_hint: &str,
        images: &[String],
        variant: u8,
    ) -> Value {
        let user_text = prompt::user_prompt(question, media_hint);
        let content = if images.is_empty() {
            json!(user_text)
        } else {
            let mut parts = vec![json!({"type": "text", "text": user_text})];
            for url in images {
                parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
            }
            json!(parts)
        };
        let model = if images.is_empty() || self.vision_model.is_empty() {
            self.model.clone()
        } else {
            self.vision_model.clone()
        };
        let mut body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": prompt::SYSTEM},
                {"role": "user", "content": content}
            ],
            "temperature": self.temperature,
            "max_tokens": 1200
        });
        if variant < 2 {
            body["response_format"] = json!({"type": "json_object"});
        }
        if variant == 0 {
            match self.thinking {
                Thinking::On => {
                    body["thinking"] = json!({"type": "enabled"});
                    body["chat_template_kwargs"] = json!({"enable_thinking": true});
                }
                Thinking::Off => {
                    body["thinking"] = json!({"type": "disabled"});
                    body["chat_template_kwargs"] = json!({"enable_thinking": false});
                }
                Thinking::Auto => {}
            }
            if let Some(effort) = &self.reasoning_effort {
                body["reasoning_effort"] = json!(effort);
            }
        }
        body
    }

    pub async fn solve(
        &self,
        question: &Question,
        media_hint: &str,
        images: &[String],
    ) -> Result<Answer, SolverError> {
        if self.key.trim().is_empty() {
            return Err(SolverError::MissingKey);
        }
        let mut last: Option<SolverError> = None;
        for variant in 0..3u8 {
            let body = self.build_body(question, media_hint, images, variant);
            let resp = self
                .client
                .post(self.url("/chat/completions"))
                .bearer_auth(&self.key)
                .json(&body)
                .send()
                .await
                .map_err(|e| SolverError::Network(e.to_string()))?;
            let status = resp.status();
            if status.is_success() {
                let payload: Value = resp
                    .json()
                    .await
                    .map_err(|e| SolverError::Invalid(e.to_string()))?;
                return parse_answer(&payload, question);
            }
            let code = status.as_u16();
            let text = resp.text().await.unwrap_or_default();
            last = Some(if code == 400 || code == 422 {
                SolverError::Invalid(format!(
                    "{code}: {}",
                    crate::util::truncate_chars(&text, 200)
                ))
            } else {
                SolverError::Http(code)
            });
            // Only parameter/schema problems are worth retrying.
            if !(code == 400 || code == 422) {
                break;
            }
        }
        Err(last.unwrap_or_else(|| SolverError::Invalid("gagal".into())))
    }
}

fn parse_answer(payload: &Value, question: &Question) -> Result<Answer, SolverError> {
    let content = payload
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .ok_or_else(|| SolverError::Invalid("tanpa content".into()))?;
    let raw = prompt::extract_json(content)
        .ok_or_else(|| SolverError::Invalid("JSON tidak ditemukan".into()))?;
    let value: Value =
        serde_json::from_str(raw).map_err(|e| SolverError::Invalid(e.to_string()))?;
    let values: Vec<String> = match value.get("answer") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) => s.trim().to_string(),
                other => other.to_string(),
            })
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => vec![s.trim().to_string()],
        _ => Vec::new(),
    };
    if values.is_empty() {
        return Err(SolverError::Invalid("answer kosong".into()));
    }
    let confidence = value
        .get("confidence")
        .and_then(|c| c.as_f64())
        .unwrap_or(0.5) as f32;
    let mut answer = Answer::new(&question.id, values, AnswerSource::Ai);
    answer.confidence = confidence.clamp(0.0, 1.0);
    answer.low_confidence = answer.confidence < 0.5;
    answer.reason = value
        .get("reason")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();
    Ok(answer)
}

/// Wrap raw bytes as a data URL for vision requests.
pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OptionItem, QuestionType};

    fn q() -> Question {
        Question {
            id: "q".into(),
            type_: QuestionType::Mcq,
            text: "Soal?".into(),
            explain: String::new(),
            media: vec![],
            options: vec![OptionItem {
                id: "o".into(),
                text: "A".into(),
                media: vec![],
                dom_index: Some(0),
            }],
            targets: vec![],
            time_ms: 1000,
            hash: String::new(),
            raw: Value::Null,
        }
    }

    #[test]
    fn payload_omits_thinking_when_auto() {
        let solver = Solver::new("https://x/v1", "k", "m", "", Thinking::Auto, None, 0.2).unwrap();
        let body = solver.build_body(&q(), "", &[], 0);
        assert!(body.get("thinking").is_none());
        assert!(body.get("chat_template_kwargs").is_none());
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn payload_carries_thinking_when_on() {
        let solver = Solver::new(
            "https://x/v1",
            "k",
            "m",
            "",
            Thinking::On,
            Some("low".into()),
            0.2,
        )
        .unwrap();
        let body = solver.build_body(&q(), "", &[], 0);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "low");
    }

    #[test]
    fn parse_valid_answer() {
        let payload =
            json!({"choices":[{"message":{"content":"{\"answer\":[\"A\"],\"confidence\":0.9}"}}]});
        let answer = parse_answer(&payload, &q()).unwrap();
        assert_eq!(answer.values, vec!["A"]);
        assert!(!answer.low_confidence);
    }

    #[test]
    fn parse_fenced_answer() {
        let payload = json!({"choices":[{"message":{"content":"```json\n{\"answer\":\"A\",\"confidence\":0.4}\n```"}}]});
        let answer = parse_answer(&payload, &q()).unwrap();
        assert!(answer.low_confidence);
    }

    #[test]
    fn missing_content_errors() {
        assert!(parse_answer(&json!({}), &q()).is_err());
    }

    #[test]
    fn data_url_prefixes_base64() {
        assert!(data_url("image/png", b"ab").starts_with("data:image/png;base64,"));
    }
}
