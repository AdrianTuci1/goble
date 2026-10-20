use base64::Engine as _;
use rand::Rng as _;
use sha2::{Digest, Sha256};

/// Hash a PIN for storage/comparison. Only this digest is kept in memory.
pub fn hash_pin(pin: &str) -> [u8; 32] {
    let digest = Sha256::digest(pin.as_bytes());
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&digest);
    hash
}

/// Compare a candidate PIN against a stored hash without an early exit.
pub fn verify_pin(pin_hash: &[u8; 32], candidate: &str) -> bool {
    constant_time_eq(pin_hash, &hash_pin(candidate))
}

/// Generate a short, human-typable PIN. Ambiguous characters are omitted so a
/// PIN read off a terminal is entered the same way.
pub fn generate_pin() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    (0..8)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect()
}

/// Generate an opaque bearer/session token (256 bits, URL-safe).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Constant-time comparison for secrets.
///
/// The length itself is not secret for fixed-size hashes and generated tokens;
/// the contents are compared without short-circuiting on the first mismatch.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in a.iter().zip(b) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_verification_accepts_only_the_original_pin() {
        let hash = hash_pin("correct-horse");
        assert!(verify_pin(&hash, "correct-horse"));
        assert!(!verify_pin(&hash, "correct-hors"));
        assert!(!verify_pin(&hash, "Correct-horse"));
    }

    #[test]
    fn generated_pins_are_short_and_unambiguous() {
        let pin = generate_pin();
        assert_eq!(pin.len(), 8);
        assert!(pin
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
        assert!(!pin.contains(['I', 'O', '0', '1']));
    }

    #[test]
    fn tokens_are_random_and_url_safe() {
        let first = generate_token();
        let second = generate_token();
        assert_ne!(first, second);
        assert_eq!(first.len(), 43);
        assert!(first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
