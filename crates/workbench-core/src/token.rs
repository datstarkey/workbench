//! Bearer tokens guarding the control-plane server's listeners.

use anyhow::Context;

/// Shortest token any listener accepts.
pub const MIN_TOKEN_LEN: usize = 32;

/// 32 bytes from the OS CSPRNG, hex-encoded (64 characters).
pub fn generate() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).context("OS random number generator unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether `token` is strong enough to guard a listener: at least
/// [`MIN_TOKEN_LEN`] characters and no whitespace (so a padded or blank value
/// can't slip through).
pub fn is_strong(token: &str) -> bool {
    token.len() >= MIN_TOKEN_LEN && !token.chars().any(char::is_whitespace)
}

/// Length-independent constant-time comparison, so a secret isn't leaked
/// through timing.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_strong_hex_and_unique() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(is_strong(&a));
        assert_ne!(a, b);
    }

    #[test]
    fn weak_tokens_are_rejected() {
        assert!(!is_strong(""));
        assert!(!is_strong("   "));
        assert!(!is_strong("secret"));
        assert!(!is_strong(&"a".repeat(MIN_TOKEN_LEN - 1)));
        assert!(!is_strong(&format!("{} ", "a".repeat(MIN_TOKEN_LEN))));
        assert!(is_strong(&"a".repeat(MIN_TOKEN_LEN)));
    }
}
