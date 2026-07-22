use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ring::{digest, pbkdf2};
use std::num::NonZeroU32;

pub const PBKDF2_ITERATIONS: u32 = 100_000;

pub fn derive_key(pairing_code: &str, salt: &[u8]) -> anyhow::Result<[u8; 32]> {
    if pairing_code.is_empty() {
        anyhow::bail!("pairing code must not be empty");
    }
    if salt.len() < 16 {
        anyhow::bail!("salt must be at least 16 bytes");
    }
    let mut key = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
        salt,
        pairing_code.as_bytes(),
        &mut key,
    );
    Ok(key)
}

pub fn hash_pairing_code(pairing_code: &str, salt: &[u8]) -> anyhow::Result<String> {
    let key = derive_key(pairing_code, salt)?;
    let hash = digest::digest(&digest::SHA256, &key);
    Ok(hex::encode(hash.as_ref()))
}

pub fn encrypt(plaintext: &[u8], key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let mut ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;
    let mut out = nonce.to_vec();
    out.append(&mut ciphertext);
    Ok(out)
}

pub fn decrypt(ciphertext: &[u8], key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    if ciphertext.len() < 12 {
        anyhow::bail!("ciphertext too short");
    }
    let nonce = Nonce::from_slice(&ciphertext[..12]);
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
    cipher
        .decrypt(nonce, &ciphertext[12..])
        .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
}

pub fn generate_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    rand::fill(&mut salt);
    salt
}

pub fn generate_pairing_code() -> String {
    let mut bytes = [0u8; 8];
    rand::fill(&mut bytes);
    format!("{:08}", u64::from_le_bytes(bytes) % 100_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let salt = generate_salt();
        let code = generate_pairing_code();
        let key = derive_key(&code, &salt).unwrap();
        let plaintext = b"secret data";
        let encrypted = encrypt(plaintext, &key).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_empty_code_fails() {
        let salt = generate_salt();
        assert!(derive_key("", &salt).is_err());
    }

    #[test]
    fn test_short_salt_fails() {
        assert!(derive_key("123456", &[0u8; 8]).is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let salt = generate_salt();
        let key = derive_key("123456", &salt).unwrap();
        let encrypted = encrypt(b"x", &key).unwrap();
        let wrong_key = derive_key("654321", &salt).unwrap();
        assert!(decrypt(&encrypted, &wrong_key).is_err());
    }

    #[test]
    fn test_ciphertext_too_short() {
        let key = [0u8; 32];
        assert!(decrypt(&[0u8; 5], &key).is_err());
    }
}
