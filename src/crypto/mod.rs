//! Cryptographic primitives for Phase 3.
//!
//! This module formalizes path-based key derivation with HKDF-SHA256 and
//! authenticated payload encryption with ChaCha20-Poly1305.

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

pub const DERIVED_KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

pub struct PathCrypto;

impl PathCrypto {
    pub fn path_hash(path: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(path.as_bytes());
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }

    pub fn derive_object_key(master_key: &[u8], salt: &[u8], object_path: &str) -> [u8; 32] {
        let info = Self::path_hash(object_path);
        let hk = Hkdf::<Sha256>::new(Some(salt), master_key);
        let mut okm = [0u8; DERIVED_KEY_SIZE];
        hk.expand(&info, &mut okm)
            .expect("HKDF expand to 32 bytes must succeed");
        okm
    }

    pub fn encrypt_in_place(
        key: &[u8; DERIVED_KEY_SIZE],
        nonce: &[u8; NONCE_SIZE],
        aad: &[u8],
        payload: &mut [u8],
    ) -> [u8; TAG_SIZE] {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        let tag = cipher
            .encrypt_in_place_detached(Nonce::from_slice(nonce), aad, payload)
            .expect("encryption should succeed for valid key/nonce");
        let mut out = [0u8; TAG_SIZE];
        out.copy_from_slice(&tag);
        out
    }

    pub fn decrypt_in_place(
        key: &[u8; DERIVED_KEY_SIZE],
        nonce: &[u8; NONCE_SIZE],
        aad: &[u8],
        payload: &mut [u8],
        tag: &[u8; TAG_SIZE],
    ) -> bool {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        cipher
            .decrypt_in_place_detached(Nonce::from_slice(nonce), aad, payload, Tag::from_slice(tag))
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_derived_keys_are_path_bound() {
        let master = [7u8; 32];
        let salt = [9u8; 32];
        let k1 = PathCrypto::derive_object_key(&master, &salt, "/bucket/a");
        let k2 = PathCrypto::derive_object_key(&master, &salt, "/bucket/b");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let master = [1u8; 32];
        let salt = [2u8; 32];
        let key = PathCrypto::derive_object_key(&master, &salt, "/obj/42");
        let nonce = [3u8; NONCE_SIZE];
        let aad = b"metadata";
        let mut payload = *b"phase3-payload";

        let tag = PathCrypto::encrypt_in_place(&key, &nonce, aad, &mut payload);
        assert_ne!(&payload, b"phase3-payload");

        let ok = PathCrypto::decrypt_in_place(&key, &nonce, aad, &mut payload, &tag);
        assert!(ok);
        assert_eq!(&payload, b"phase3-payload");
    }

    #[test]
    fn test_decrypt_fails_with_wrong_path_key() {
        let master = [5u8; 32];
        let salt = [6u8; 32];
        let key_ok = PathCrypto::derive_object_key(&master, &salt, "/x");
        let key_bad = PathCrypto::derive_object_key(&master, &salt, "/y");
        let nonce = [4u8; NONCE_SIZE];
        let mut payload = *b"secret-payload";
        let original = payload;

        let tag = PathCrypto::encrypt_in_place(&key_ok, &nonce, b"", &mut payload);
        let mut tampered = payload;
        let ok = PathCrypto::decrypt_in_place(&key_bad, &nonce, b"", &mut tampered, &tag);
        assert!(!ok);
        assert_ne!(tampered, original);
    }
}
