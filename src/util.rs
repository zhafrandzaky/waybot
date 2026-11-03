//! Pure helpers: text normalisation, hashing, jitter.

use rand::RngExt;
use sha2::{Digest, Sha256};

/// Strip tags and decode the handful of entities Wayground emits.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    normalize_ws(&decoded)
}

pub fn normalize_ws(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalised comparison key: lowercased, alphanumeric-ish only.
pub fn norm_key(input: &str) -> String {
    normalize_ws(&html_to_text(input))
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn hash_question(text: &str, options: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(norm_key(text).as_bytes());
    for opt in options {
        hasher.update(b"\x1f");
        hasher.update(norm_key(opt).as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Random delay in `[min, max]` milliseconds.
pub fn jitter_ms(min_ms: u64, max_ms: u64) -> u64 {
    if max_ms <= min_ms {
        return min_ms;
    }
    rand::rng().random_range(min_ms..=max_ms)
}

pub fn truncate_chars(input: &str, max: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// Validate a room code: 3-11 alphanumeric characters.
pub fn valid_room_code(code: &str) -> bool {
    let len = code.chars().count();
    (3..=11).contains(&len) && code.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Validate a display name: 1-60 chars, trimmed, no control characters.
pub fn valid_display_name(name: &str) -> bool {
    let trimmed = name.trim();
    let len = trimmed.chars().count();
    (1..=60).contains(&len) && !trimmed.chars().any(|c| c.is_control())
}

/// Parse `mm:ss` (or `h:mm:ss`) into milliseconds.
pub fn parse_duration(input: &str) -> Option<u64> {
    let parts: Vec<&str> = input.trim().split(':').collect();
    let secs: u64 = match parts.as_slice() {
        [m, s] => m.parse::<u64>().ok()? * 60 + s.parse::<u64>().ok()?,
        [h, m, s] => {
            h.parse::<u64>().ok()? * 3600 + m.parse::<u64>().ok()? * 60 + s.parse::<u64>().ok()?
        }
        _ => return None,
    };
    Some(secs * 1000)
}

pub fn fmt_duration(ms: u64) -> String {
    let total = ms / 1000;
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_stripped_and_decoded() {
        assert_eq!(html_to_text("<p>CPU &amp; RAM</p><br/>x"), "CPU & RAM x");
    }

    #[test]
    fn norm_key_ignores_case_and_punctuation() {
        assert_eq!(
            norm_key("  Central  Processing-Unit "),
            "central processing unit"
        );
    }

    #[test]
    fn hash_is_stable_and_option_sensitive() {
        let a = hash_question("Apa itu CPU?", &["unit".into(), "monitor".into()]);
        let b = hash_question("<p>Apa itu CPU?</p>", &["Unit".into(), "Monitor".into()]);
        assert_eq!(a, b);
        let c = hash_question("Apa itu CPU?", &["monitor".into(), "unit".into()]);
        assert_ne!(a, c);
    }

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration("03:00"), Some(180_000));
        assert_eq!(parse_duration("1:02:03"), Some(3_723_000));
        assert_eq!(parse_duration("90"), None);
        assert_eq!(fmt_duration(180_000), "03:00");
    }

    #[test]
    fn validation_rules() {
        assert!(valid_room_code("794275"));
        assert!(!valid_room_code("79"));
        assert!(!valid_room_code("79427-5"));
        assert!(valid_display_name("Budi"));
        assert!(!valid_display_name("   "));
    }

    #[test]
    fn jitter_stays_in_range() {
        for _ in 0..50 {
            let v = jitter_ms(10, 20);
            assert!((10..=20).contains(&v));
        }
    }
}
