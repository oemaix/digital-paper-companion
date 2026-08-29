//! Key derivation (protocol §4.5).
//!
//! `derivedKey = PBKDF2-HMAC-SHA256(ZZ, n1‖mac‖n2, 10000, 48)`
//! split into `authKey = derivedKey[0..32]` (HMAC key) and
//! `keyWrapKey = derivedKey[32..48]` (AES-128 key).

use hmac::{Hmac, Mac};
use sha2::Sha256;

const ITERATIONS: u32 = 10_000;

/// Derived key material for a registration session.
pub struct DerivedKeys {
    pub auth_key: [u8; 32],
    pub key_wrap_key: [u8; 16],
}

/// Derives the session keys from the shared secret and the three nonces.
pub fn derive(zz: &[u8], n1: &[u8], mac: &[u8], n2: &[u8]) -> DerivedKeys {
    let mut salt = Vec::with_capacity(n1.len() + mac.len() + n2.len());
    salt.extend_from_slice(n1);
    salt.extend_from_slice(mac);
    salt.extend_from_slice(n2);

    let mut dk = [0u8; 48];
    pbkdf2::pbkdf2_hmac::<Sha256>(zz, &salt, ITERATIONS, &mut dk);

    let mut auth_key = [0u8; 32];
    auth_key.copy_from_slice(&dk[0..32]);
    let mut key_wrap_key = [0u8; 16];
    key_wrap_key.copy_from_slice(&dk[32..48]);
    DerivedKeys {
        auth_key,
        key_wrap_key,
    }
}

/// HMAC-SHA256 over `msg` with `key` (32-byte digest). Shared by the
/// handshake message chaining (protocol §4.7) and the key-wrap authenticator.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_splits_48_bytes() {
        let keys = derive(&[0u8; 256], &[1u8; 16], &[2u8; 6], &[3u8; 16]);
        // Deterministic for fixed inputs; just assert the split sizes and
        // that the two halves differ.
        assert_ne!(&keys.auth_key[..16], &keys.key_wrap_key[..]);
    }

    #[test]
    fn hmac_matches_known_vector() {
        // RFC 4231 test case 1: key = 0x0b*20, data = "Hi There".
        let key = [0x0bu8; 20];
        let out = hmac_sha256(&key, b"Hi There");
        let expected =
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";
        assert_eq!(hex(&out), expected);
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
