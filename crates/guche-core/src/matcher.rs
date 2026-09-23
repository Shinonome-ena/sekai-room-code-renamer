use regex_lite::Regex;
use std::sync::LazyLock;

static FIVE_DIGIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{5}$").unwrap());
static FUZZY_FIVE_DIGIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{5})").unwrap());

/// fuzzy=false 严格（整串 5 位）；fuzzy=true 宽松（开头 5 位，第 6 位不能是数字）
pub fn match_message(text: &str, fuzzy: bool) -> Option<String> {
    if !fuzzy {
        return FIVE_DIGIT_RE.is_match(text).then(|| text.to_string());
    }
    let trimmed = text.trim();
    FUZZY_FIVE_DIGIT_RE.captures(trimmed).and_then(|cap| {
        let m = cap.get(1)?;
        let after = &trimmed[m.end()..];
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        Some(m.as_str().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_exact() {
        assert_eq!(match_message("12345", false), Some("12345".into()));
    }

    #[test]
    fn strict_rejects_extra() {
        assert_eq!(match_message("12345a", false), None);
        assert_eq!(match_message(" 12345", false), None);
    }

    #[test]
    fn fuzzy_extracts_prefix() {
        assert_eq!(match_message("66666 test", true), Some("66666".into()));
        assert_eq!(match_message("12345abc", true), Some("12345".into()));
        assert_eq!(match_message("12345 🦐288+", true), Some("12345".into()));
    }

    #[test]
    fn fuzzy_exact_5_digits_only() {
        assert_eq!(match_message("12345", true), Some("12345".into()));
    }

    #[test]
    fn fuzzy_rejects_6_digits() {
        assert_eq!(match_message("123456", true), None);
        assert_eq!(match_message("123456abc", true), None);
    }

    #[test]
    fn fuzzy_rejects_non_digit_start() {
        assert_eq!(match_message("abc12345", true), None);
    }
}
