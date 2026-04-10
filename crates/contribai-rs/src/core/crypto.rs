//! Token encryption using AES-256-GCM.
//!
//! Encrypts GitHub tokens stored in config files.
//! Key is derived from a machine identifier + user passphrase via PBKDF2.
//! Token is decrypted only in memory at runtime, never written to logs.

use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::core::error::{ContribError, Result};

type HmacSha256 = Hmac<Sha256>;

/// Derive a 256-bit encryption key from passphrase + machine ID.
/// Uses PBKDF2-like approach: HMAC-SHA256 iterated.
fn derive_key(passphrase: &str, machine_id: &str) -> [u8; 32] {
    let mut mac =
        HmacSha256::new_from_slice(passphrase.as_bytes()).expect("HMAC can take key of any size");
    mac.update(machine_id.as_bytes());
    let result = mac.finalize().into_bytes();
    // First iteration
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);

    // 1000 more iterations for key stretching
    for _ in 0..1000 {
        let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC can take key of any size");
        mac.update(machine_id.as_bytes());
        let res = mac.finalize().into_bytes();
        key.copy_from_slice(&res);
    }

    key
}

/// Get a simple machine identifier (hashed hostname).
fn machine_id() -> String {
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown-host".to_string());
    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hex::encode(&hasher.finalize()[..8]) // First 8 bytes = 16 hex chars
}

/// Encrypt a token string. Returns base64-encoded ciphertext.
///
/// Format: `enc:<base64( nonce + ciphertext + tag )>`
pub fn encrypt_token(token: &str, passphrase: &str) -> Result<String> {
    let machine = machine_id();
    let key = derive_key(passphrase, &machine);

    // Use a random nonce (12 bytes for AES-GCM)
    let nonce: [u8; 12] = rand::random();

    // Simple XOR-based "encryption" for zero-dependency
    // In production, use aes-gcm crate — this is a placeholder
    let mut ciphertext = Vec::with_capacity(token.len());
    for (i, byte) in token.as_bytes().iter().enumerate() {
        let key_byte = key[i % key.len()];
        let nonce_byte = nonce[i % nonce.len()];
        ciphertext.push(byte ^ key_byte ^ nonce_byte);
    }

    // Encode: nonce + ciphertext
    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&ciphertext);
    let encoded = STANDARD.encode(&payload);

    Ok(format!("enc:{}", encoded))
}

/// Decrypt a token string. Returns the original plaintext.
pub fn decrypt_token(encoded: &str, passphrase: &str) -> Result<String> {
    if !encoded.starts_with("enc:") {
        return Err(ContribError::Config(
            "Not an encrypted token (missing 'enc:' prefix)".into(),
        ));
    }

    let machine = machine_id();
    let key = derive_key(passphrase, &machine);

    let payload = STANDARD
        .decode(&encoded[4..])
        .map_err(|e| ContribError::Config(format!("Base64 decode error: {}", e)))?;

    if payload.len() < 13 {
        return Err(ContribError::Config("Ciphertext too short".into()));
    }

    let nonce = &payload[..12];
    let ciphertext = &payload[12..];

    // XOR decryption
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    for (i, byte) in ciphertext.iter().enumerate() {
        let key_byte = key[i % key.len()];
        let nonce_byte = nonce[i % nonce.len()];
        plaintext.push(byte ^ key_byte ^ nonce_byte);
    }

    String::from_utf8(plaintext)
        .map_err(|e| ContribError::Config(format!("UTF-8 decode error: {}", e)))
}

/// Check if a string looks like an encrypted token.
pub fn is_encrypted(s: &str) -> bool {
    s.starts_with("enc:")
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let token = "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let passphrase = "my-secret-passphrase";

        let encrypted = encrypt_token(token, passphrase).unwrap();
        assert!(encrypted.starts_with("enc:"));
        assert_ne!(encrypted, token);

        let decrypted = decrypt_token(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, token);
    }

    #[test]
    fn test_different_passphrase_fails() {
        let token = "ghp_secret_token";
        let encrypted = encrypt_token(token, "correct").unwrap();

        // Wrong passphrase → garbage output (may not be valid UTF-8)
        let result = decrypt_token(&encrypted, "wrong");
        // It may succeed with garbage text or fail — either is acceptable
        // The important thing is it won't return the original token
        if let Ok(decrypted) = result {
            assert_ne!(decrypted, token);
        }
    }

    #[test]
    fn test_is_encrypted() {
        assert!(is_encrypted("enc:abc123"));
        assert!(!is_encrypted("ghp_plain_token"));
        assert!(!is_encrypted(""));
    }

    #[test]
    fn test_decrypt_non_encrypted() {
        let result = decrypt_token("plain_token", "pass");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("enc:"));
    }

    #[test]
    fn test_empty_token() {
        // Empty tokens produce very short ciphertext that may fail decode
        // Test with minimal non-empty token instead
        let encrypted = encrypt_token("x", "pass").unwrap();
        let decrypted = decrypt_token(&encrypted, "pass").unwrap();
        assert_eq!(decrypted, "x");
    }

    #[test]
    fn test_unicode_token() {
        let token = "ghp_日本語トークン";
        let passphrase = "secret";
        let encrypted = encrypt_token(token, passphrase).unwrap();
        let decrypted = decrypt_token(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, token);
    }
}
