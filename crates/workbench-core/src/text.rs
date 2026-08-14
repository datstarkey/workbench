//! UTF-8-safe string helpers.

/// The longest prefix of `s` that fits within `max_bytes`, never splitting a
/// character.
///
/// Rust's `&s[..n]` panics when `n` lands inside a multi-byte character, so any
/// slice at a fixed byte offset is a latent crash on the first non-ASCII input —
/// an em dash in a Bash command, an accented word in a `gh` response. Use this
/// for every "trim this to N bytes for display" case.
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorter_than_limit_is_unchanged() {
        assert_eq!(truncate_bytes("hello", 80), "hello");
    }

    #[test]
    fn exactly_at_limit_is_unchanged() {
        let s = "a".repeat(80);
        assert_eq!(truncate_bytes(&s, 80), s);
    }

    #[test]
    fn ascii_truncates_at_the_limit() {
        let s = "a".repeat(100);
        assert_eq!(truncate_bytes(&s, 80).len(), 80);
    }

    /// The exact shape of the reported panic: an em dash straddling byte 80.
    #[test]
    fn backs_off_a_split_multibyte_char() {
        let s = format!("{}—{}", "a".repeat(79), "b".repeat(40));
        assert!(!s.is_char_boundary(80));
        assert_eq!(truncate_bytes(&s, 80), "a".repeat(79));
    }

    #[test]
    fn every_limit_over_a_multibyte_string_is_a_valid_prefix() {
        let s = "é🎉漢字—ok".repeat(10);
        for limit in 0..=s.len() {
            let out = truncate_bytes(&s, limit);
            assert!(out.len() <= limit);
            assert!(s.starts_with(out));
        }
    }

    #[test]
    fn zero_limit_yields_empty() {
        assert_eq!(truncate_bytes("🎉", 0), "");
    }

    #[test]
    fn limit_inside_the_only_char_yields_empty() {
        assert_eq!(truncate_bytes("🎉", 2), "");
    }
}
