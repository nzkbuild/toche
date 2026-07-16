//! Utility functions for text processing. Vendored from RTK.

use lazy_static::lazy_static;
use regex::Regex;

/// Strip ANSI escape codes (colors, styles) from a string.
pub fn strip_ansi(text: &str) -> String {
    lazy_static! {
        static ref ANSI_RE: Regex = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    }
    ANSI_RE.replace_all(text, "").to_string()
}

/// Truncates a string to `max_len` characters, appending `...` if needed.
/// Unicode-safe (counts chars, not bytes).
pub fn truncate(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len < 3 {
        "...".to_string()
    } else {
        format!("{}...", s.chars().take(max_len - 3).collect::<String>())
    }
}

/// Format a token count with K/M suffix for human-readable output.
pub fn format_tokens(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_removes_color_codes() {
        assert_eq!(strip_ansi("\x1b[31mError\x1b[0m"), "Error");
    }

    #[test]
    fn test_strip_ansi_leaves_plain_text() {
        assert_eq!(strip_ansi("Hello World"), "Hello World");
    }

    #[test]
    fn test_truncate_short_string_unchanged() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_long_string_gets_ellipsis() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_unicode_safe() {
        assert_eq!(truncate("日本語テスト", 4), "日...");
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1_500), "1.5K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }
}
