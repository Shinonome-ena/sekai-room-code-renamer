use regex_lite::Regex;
use std::sync::LazyLock;

static HEAD_FIVE_DIGIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{5})\s+(.+)$").unwrap());
static TAIL_FIVE_DIGIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*\s)?(\d{5})$").unwrap());

const GROUP_NAME_MAX_LEN: usize = 60;

/// end=false 车牌在前，true 在后。返回 (新群名, 是否截断过)。
pub fn compute_new_name(old_name: &str, new_code: &str, end: bool) -> (String, bool) {
    let base_name = if !end {
        if let Some(caps) = HEAD_FIVE_DIGIT_RE.captures(old_name) {
            caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default()
        } else if old_name.len() == 5 && old_name.bytes().all(|b| b.is_ascii_digit()) {
            String::new()
        } else {
            old_name.to_string()
        }
    } else if let Some(caps) = TAIL_FIVE_DIGIT_RE.captures(old_name) {
        caps.get(1).map(|m| m.as_str().trim_end().to_string()).unwrap_or_default()
    } else {
        old_name.to_string()
    };

    let join = |base: &str| {
        if base.is_empty() {
            new_code.to_string()
        } else if !end {
            format!("{new_code} {base}")
        } else {
            format!("{base} {new_code}")
        }
    };

    let new_name = join(&base_name);
    if new_name.chars().count() <= GROUP_NAME_MAX_LEN {
        return (new_name, false);
    }

    let overflow = new_name.chars().count() - GROUP_NAME_MAX_LEN;
    let base_len = base_name.chars().count();
    let trimmed_base = if base_len > overflow {
        if !end {
            base_name.chars().take(base_len - overflow).collect()
        } else {
            base_name.chars().skip(overflow).collect()
        }
    } else {
        String::new()
    };
    (join(&trimmed_base), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_no_old_code() {
        let (name, _) = compute_new_name("娱乐群", "66666", false);
        assert_eq!(name, "66666 娱乐群");
    }

    #[test]
    fn start_replace_old_code() {
        let (name, _) = compute_new_name("12345 娱乐群", "66666", false);
        assert_eq!(name, "66666 娱乐群");
    }

    #[test]
    fn end_no_old_code() {
        let (name, _) = compute_new_name("娱乐群", "66666", true);
        assert_eq!(name, "娱乐群 66666");
    }

    #[test]
    fn end_replace_old_code() {
        let (name, _) = compute_new_name("娱乐群 12345", "66666", true);
        assert_eq!(name, "娱乐群 66666");
    }

    #[test]
    fn empty_base() {
        let (name, _) = compute_new_name("12345", "66666", false);
        assert_eq!(name, "66666");
    }

    #[test]
    fn start_keeps_trailing_number() {
        let (name, _) = compute_new_name("推车abc 126860", "66666", false);
        assert_eq!(name, "66666 推车abc 126860");
    }

    #[test]
    fn start_trims_overflow() {
        let base = "车".repeat(80);
        let (name, trimmed) = compute_new_name(&base, "66666", false);
        assert_eq!(name.chars().count(), 60);
        assert!(trimmed && name.starts_with("66666 "));
    }

    #[test]
    fn end_trims_overflow() {
        let base = "车".repeat(80);
        let (name, trimmed) = compute_new_name(&base, "66666", true);
        assert_eq!(name.chars().count(), 60);
        assert!(trimmed && name.ends_with(" 66666"));
    }

    #[test]
    fn no_trim_when_within_limit() {
        let (name, trimmed) = compute_new_name("短群名", "66666", false);
        assert_eq!(name, "66666 短群名");
        assert!(!trimmed);
    }
}
