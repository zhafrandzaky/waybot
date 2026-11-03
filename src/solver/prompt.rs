//! Strict solver prompt. Output must be a single JSON object.

use crate::model::{Question, QuestionType};

pub const SYSTEM: &str = "Kamu menjawab soal kuis. Jawab hanya dengan satu objek JSON, tanpa \
penjelasan di luar JSON. Skema: {\"answer\": [\"...\"], \"confidence\": 0.0-1.0, \
\"reason\": \"singkat\"}. Untuk pilihan ganda isi satu teks opsi persis seperti daftar; untuk \
jawaban ganda isi semua teks opsi yang benar; untuk isian isi teks jawaban; untuk urutan isi \
teks sesuai urutan. Jika ragu, tetap pilih yang paling masuk akal dan turunkan confidence.";

pub fn type_rule(kind: QuestionType) -> &'static str {
    match kind {
        QuestionType::Mcq | QuestionType::Poll => "Pilih tepat 1 opsi.",
        QuestionType::Msq => "Pilih semua opsi yang benar (boleh lebih dari satu).",
        QuestionType::Blank => "Isi teks jawaban singkat.",
        QuestionType::Open => "Tulis jawaban berupa satu paragraf pendek.",
        QuestionType::Dropdown => "Pilih satu teks opsi yang benar.",
        QuestionType::Reorder => "Kembalikan semua opsi dalam urutan yang benar.",
        QuestionType::Match => "Kembalikan pasangan dalam bentuk \"kiri = kanan\" sesuai urutan.",
        QuestionType::DragDrop | QuestionType::DndImage => {
            "Kembalikan teks opsi tujuan yang benar."
        }
        QuestionType::Hotspot => "Sebutkan nomor area yang benar bila ada.",
        QuestionType::Unknown => "Tebak format paling wajar dan turunkan confidence.",
    }
}

pub fn user_prompt(question: &Question, media_hint: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("Tipe soal: {}\n", question.type_.label()));
    out.push_str(&format!("Aturan: {}\n", type_rule(question.type_)));
    out.push_str(&format!("Soal: {}\n", question.plain_text()));
    if !media_hint.trim().is_empty() {
        out.push_str(&format!("Konteks gambar: {}\n", media_hint.trim()));
    }
    if !question.explain.trim().is_empty() {
        out.push_str(&format!(
            "Petunjuk (bisa membocorkan jawaban): {}\n",
            crate::util::html_to_text(&question.explain)
        ));
    }
    if !question.options.is_empty() {
        out.push_str("Opsi:\n");
        for (idx, opt) in question.options.iter().enumerate() {
            out.push_str(&format!(
                "{idx}. {}\n",
                crate::util::html_to_text(&opt.text)
            ));
        }
    }
    out
}

/// Extract the first balanced JSON object from model output.
pub fn extract_json(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escape = false;
    for (offset, ch) in raw[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..start + offset + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_inside_fence_is_found() {
        let raw = "```json\n{\"answer\": [\"A\"], \"confidence\": 0.9, \"reason\": \"x\"}\n```";
        assert_eq!(
            extract_json(raw).unwrap(),
            "{\"answer\": [\"A\"], \"confidence\": 0.9, \"reason\": \"x\"}"
        );
    }

    #[test]
    fn nested_json_and_strings_with_braces() {
        let raw = "teks {\"a\":\"{x}\",\"b\":{\"c\":1}} ekor";
        assert_eq!(
            extract_json(raw).unwrap(),
            "{\"a\":\"{x}\",\"b\":{\"c\":1}}"
        );
    }

    #[test]
    fn missing_json_returns_none() {
        assert!(extract_json("tidak ada").is_none());
    }
}
