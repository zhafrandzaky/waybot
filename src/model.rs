//! Domain model for questions, answers and observed page state.

use serde::{Deserialize, Serialize};

use crate::util::html_to_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QuestionType {
    Mcq,
    Msq,
    Blank,
    Open,
    Dropdown,
    Reorder,
    Match,
    DragDrop,
    DndImage,
    Hotspot,
    Poll,
    Unknown,
}

impl QuestionType {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_uppercase().as_str() {
            "MCQ" => Self::Mcq,
            "MSQ" => Self::Msq,
            "BLANK" => Self::Blank,
            "OPEN" => Self::Open,
            "DROPDOWN" => Self::Dropdown,
            "REORDER" => Self::Reorder,
            "MATCH" => Self::Match,
            "DRAGNDROP" | "DRAG_DROP" => Self::DragDrop,
            "DND_IMAGE" | "DNDIMAGE" => Self::DndImage,
            "HOTSPOT" => Self::Hotspot,
            "POLL" => Self::Poll,
            _ => Self::Unknown,
        }
    }

    pub fn is_choice(self) -> bool {
        matches!(self, Self::Mcq | Self::Msq | Self::Poll)
    }

    pub fn is_text(self) -> bool {
        matches!(self, Self::Blank | Self::Open | Self::Dropdown)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mcq => "MCQ",
            Self::Msq => "MSQ",
            Self::Blank => "BLANK",
            Self::Open => "OPEN",
            Self::Dropdown => "DROPDOWN",
            Self::Reorder => "REORDER",
            Self::Match => "MATCH",
            Self::DragDrop => "DRAGNDROP",
            Self::DndImage => "DND_IMAGE",
            Self::Hotspot => "HOTSPOT",
            Self::Poll => "POLL",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub src: String,
    #[serde(default)]
    pub alt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionItem {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub media: Vec<Media>,
    /// Index as rendered by the client (`data-cy="option-<index>"`).
    #[serde(default)]
    pub dom_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    #[serde(default)]
    pub option_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: QuestionType,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub explain: String,
    #[serde(default)]
    pub media: Vec<Media>,
    #[serde(default)]
    pub options: Vec<OptionItem>,
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub time_ms: u64,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub raw: serde_json::Value,
}

impl Question {
    pub fn plain_text(&self) -> String {
        html_to_text(&self.text)
    }

    pub fn option_texts(&self) -> Vec<String> {
        self.options.iter().map(|o| html_to_text(&o.text)).collect()
    }

    pub fn recompute_hash(&mut self) {
        self.hash = crate::util::hash_question(&self.text, &self.option_texts());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerSource {
    Key,
    Explain,
    Ai,
    Heuristic,
}

impl AnswerSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Explain => "explain",
            Self::Ai => "ai",
            Self::Heuristic => "heuristik",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub question_id: String,
    /// Chosen option texts (choice types) or free text values (text types).
    pub values: Vec<String>,
    pub confidence: f32,
    pub source: AnswerSource,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub low_confidence: bool,
}

impl Answer {
    pub fn new(question_id: &str, values: Vec<String>, source: AnswerSource) -> Self {
        let confidence = match source {
            AnswerSource::Key => 0.99,
            AnswerSource::Explain => 0.8,
            AnswerSource::Ai => 0.6,
            AnswerSource::Heuristic => 0.25,
        };
        Self {
            question_id: question_id.to_string(),
            values,
            confidence,
            source,
            reason: String::new(),
            low_confidence: confidence < 0.5,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.iter().all(|v| v.trim().is_empty())
    }
}

/// What the page currently shows. Built purely from a DOM snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageState {
    pub url: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub question_number: Option<u32>,
    #[serde(default)]
    pub total_questions: Option<u32>,
    #[serde(default)]
    pub question_id: Option<String>,
    #[serde(default)]
    pub question_text: String,
    #[serde(default)]
    pub question_type_hint: Option<String>,
    #[serde(default)]
    pub options: Vec<DomOption>,
    #[serde(default)]
    pub inputs: Vec<DomInput>,
    #[serde(default)]
    pub next_button: DomButton,
    #[serde(default)]
    pub submit_button: DomButton,
    #[serde(default)]
    pub feedback_visible: bool,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub waiting_for_host: bool,
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub interstitial: Option<String>,
    #[serde(default)]
    pub dialog_text: String,
    #[serde(default)]
    pub error_text: String,
    #[serde(default)]
    pub fullscreen_prompt: bool,
    #[serde(default)]
    pub login_popup: bool,
    #[serde(default)]
    pub media: Vec<Media>,
    #[serde(default)]
    pub body_excerpt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomOption {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub msq: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomInput {
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub cy: Option<String>,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub editable: bool,
    #[serde(default)]
    pub css: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomButton {
    #[serde(default)]
    pub exists: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub label: String,
}

impl PageState {
    /// Stable identity even when the container lacks `data-quesid`.
    /// Order matters: container id, question number, option fingerprint, text, body.
    pub fn effective_id(&self) -> Option<String> {
        if let Some(id) = &self.question_id {
            return Some(id.clone());
        }
        if let Some(number) = self.question_number {
            return Some(format!("n{number}"));
        }
        if !self.options.is_empty() {
            let mut texts: Vec<String> = self.options.iter().map(|o| o.text.clone()).collect();
            texts.sort();
            return Some(format!(
                "o{}",
                &crate::util::hash_question("", &texts)[..12]
            ));
        }
        let text = crate::util::norm_key(&self.question_text);
        if !text.is_empty() {
            return Some(format!(
                "t{}",
                &crate::util::hash_question(&text, &[])[..12]
            ));
        }
        let body = crate::util::norm_key(&self.body_excerpt);
        if body.is_empty() {
            None
        } else {
            Some(format!(
                "b{}",
                &crate::util::hash_question(&body, &[])[..12]
            ))
        }
    }

    pub fn has_question(&self) -> bool {
        self.effective_id().is_some() && (!self.options.is_empty() || !self.inputs.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_id_prefers_container_id() {
        let state = PageState {
            question_id: Some("abc".into()),
            question_number: Some(3),
            question_text: "x".into(),
            ..Default::default()
        };
        assert_eq!(state.effective_id().as_deref(), Some("abc"));
    }

    #[test]
    fn effective_id_uses_number_then_options() {
        let numbered = PageState {
            question_number: Some(7),
            ..Default::default()
        };
        assert_eq!(numbered.effective_id().as_deref(), Some("n7"));

        let options = PageState {
            options: vec![DomOption {
                index: 0,
                text: "A".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(options.effective_id().unwrap().starts_with('o'));
    }

    #[test]
    fn effective_id_falls_back_to_text() {
        let state = PageState {
            question_text: "Apa itu CPU?".into(),
            ..Default::default()
        };
        assert!(state.effective_id().unwrap().starts_with('t'));
    }

    #[test]
    fn effective_id_last_resort_body() {
        let state = PageState {
            body_excerpt: "1 / 10 Question text: x".into(),
            ..Default::default()
        };
        assert!(state.effective_id().unwrap().starts_with('b'));
    }
}
