//! Answer generation: official key, explain hint, AI, heuristic fallback.

pub mod agent;
pub mod deepseek;
pub mod prompt;

use std::collections::HashMap;

use crate::model::{Answer, AnswerSource, Question, QuestionType};
use crate::util::{norm_key, truncate_chars};

/// Heuristic picker: highest token overlap with the question text and explanation.
pub fn heuristic(question: &Question) -> Answer {
    let mut best: Option<(usize, String)> = None;
    if question.options.is_empty() {
        return Answer::new(&question.id, vec![String::new()], AnswerSource::Heuristic);
    }
    let hint = format!("{} {}", question.plain_text(), question.explain).to_lowercase();
    let hint_tokens: Vec<&str> = hint.split_whitespace().collect();
    let initials: String = hint_tokens
        .iter()
        .filter_map(|t| t.chars().next())
        .collect();
    for opt in &question.options {
        let text = crate::util::html_to_text(&opt.text);
        let key = norm_key(&text);
        if key.is_empty() {
            continue;
        }
        let mut score = key
            .split_whitespace()
            .filter(|t| t.len() >= 3 && hint_tokens.contains(t))
            .count();
        if hint.contains(&key) {
            score += 3;
        }
        let compact = key.replace(' ', "");
        if compact.len() >= 2 && hint_tokens.iter().any(|t| t.starts_with(&compact)) {
            score += 2;
        }
        if compact.len() >= 2 && initials.starts_with(&compact) {
            score += 3;
        }
        if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
            best = Some((score, text));
        }
    }
    let chosen = best.map(|(_, t)| t).unwrap_or_default();
    let mut answer = Answer::new(&question.id, vec![chosen], AnswerSource::Heuristic);
    answer.low_confidence = true;
    answer.reason = "kemiripan token lokal".into();
    answer
}

/// Derive an answer from `explain` when it leaks the correct option text.
pub fn from_explain(question: &Question) -> Option<Answer> {
    let explain = question.explain.trim();
    if explain.is_empty() || question.options.is_empty() {
        return None;
    }
    let lower = norm_key(explain);
    let mut hits: Vec<(usize, String)> = Vec::new();
    for opt in &question.options {
        let text = crate::util::html_to_text(&opt.text);
        let key = norm_key(&text);
        if key.len() > 2 && lower.contains(&key) {
            hits.push((key.len(), text));
        }
    }
    if hits.is_empty() {
        return None;
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.0));
    let values: Vec<String> = if question.type_ == QuestionType::Msq {
        hits.into_iter().map(|(_, t)| t).collect()
    } else {
        vec![hits.remove(0).1]
    };
    let mut answer = Answer::new(&question.id, values, AnswerSource::Explain);
    answer.confidence = 0.75;
    Some(answer)
}

#[derive(Debug, Default)]
pub struct AnswerCache {
    map: HashMap<String, Answer>,
}

impl AnswerCache {
    pub fn get(&self, hash: &str) -> Option<&Answer> {
        self.map.get(hash)
    }

    pub fn put(&mut self, hash: &str, answer: Answer) {
        self.map.insert(hash.to_string(), answer);
    }
}

pub fn preview(answer: &Answer) -> String {
    truncate_chars(&answer.values.join(" | "), 90)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OptionItem, Question};

    fn q(kind: QuestionType, text: &str, explain: &str, opts: &[&str]) -> Question {
        Question {
            id: "q1".into(),
            type_: kind,
            text: text.into(),
            explain: explain.into(),
            media: vec![],
            options: opts
                .iter()
                .enumerate()
                .map(|(i, t)| OptionItem {
                    id: format!("o{i}"),
                    text: (*t).into(),
                    media: vec![],
                    dom_index: Some(i as u32),
                })
                .collect(),
            targets: vec![],
            time_ms: 30_000,
            hash: String::new(),
            raw: serde_json::Value::Null,
        }
    }

    #[test]
    fn explain_leak_is_used() {
        let question = q(
            QuestionType::Mcq,
            "Manakah perangkat output?",
            "Monitor merupakan perangkat output.",
            &["Keyboard", "Monitor", "Mouse"],
        );
        let answer = from_explain(&question).unwrap();
        assert_eq!(answer.values, vec!["Monitor"]);
        assert_eq!(answer.source, AnswerSource::Explain);
    }

    #[test]
    fn explain_without_leak_returns_none() {
        let question = q(QuestionType::Mcq, "2+2?", "", &["3", "4"]);
        assert!(from_explain(&question).is_none());
    }

    #[test]
    fn heuristic_picks_overlapping_option() {
        let question = q(
            QuestionType::Mcq,
            "Central Processing Unit adalah",
            "",
            &["Monitor", "CPU", "Printer"],
        );
        let answer = heuristic(&question);
        assert_eq!(answer.values, vec!["CPU"]);
        assert!(answer.low_confidence);
    }

    #[test]
    fn blank_without_options_still_answers() {
        let question = q(QuestionType::Blank, "Ibukota Indonesia?", "", &[]);
        assert!(heuristic(&question).values[0].is_empty());
    }
}
