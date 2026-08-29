//! AES-128-CBC key wrapping with an 8-byte HMAC authenticator (protocol §4.6).
//!
//! ```text
//! wrap(data)   = AES-128-CBC-Enc(keyWrapKey, iv, PKCS7(data ‖ kwa)) ‖ iv
//! kwa          = HMAC(authKey, data)[0..8]
//! ```
//! Wire-format quirk: the IV is transmitted **after** the ciphertext.

use aes::Aes128;
use cbc::{Decryptor, Encryptor};
use cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use rand::RngCore;

use super::kdf::hmac_sha256;
use crate::Error;

type Aes128CbcEnc = Encryptor<Aes128>;
type Aes128CbcDec = Decryptor<Aes128>;

const IV_LEN: usize = 16;
const KWA_LEN: usize = 8;

/// Wraps `data`, appending a random IV after the ciphertext.
pub fn wrap(auth_key: &[u8], key_wrap_key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let kwa = &hmac_sha256(auth_key, data)[..KWA_LEN];
    let mut plaintext = Vec::with_capacity(data.len() + KWA_LEN);
    plaintext.extend_from_slice(data);
    plaintext.extend_from_slice(kwa);

    let mut iv = [0u8; IV_LEN];
    rand::thread_rng().fill_bytes(&mut iv);

    let ciphertext = Aes128CbcEnc::new(key_wrap_key.into(), &iv.into())
        .encrypt_padded_vec_mut::<Pkcs7>(&plaintext);

    let mut out = ciphertext;
    out.extend_from_slice(&iv);
    out
}

/// Reverses [`wrap`], verifying the key-wrap authenticator.
pub fn unwrap(auth_key: &[u8], key_wrap_key: &[u8; 16], blob: &[u8]) -> Result<Vec<u8>, Error> {
    if blob.len() < IV_LEN + IV_LEN {
        return Err(Error::Crypto("wrapped blob too short".into()));
    }
    let (ciphertext, iv) = blob.split_at(blob.len() - IV_LEN);

    let plaintext = Aes128CbcDec::new(key_wrap_key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| Error::Crypto("AES-CBC unpad failed".into()))?;

    if plaintext.len() < KWA_LEN {
        return Err(Error::Crypto("unwrapped plaintext too short".into()));
    }
    let (data, kwa) = plaintext.split_at(plaintext.len() - KWA_LEN);
    let expected = &hmac_sha256(auth_key, data)[..KWA_LEN];
    if kwa != expected {
        return Err(Error::Crypto("key-wrap authenticator mismatch".into()));
    }
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_unwrap_roundtrip() {
        let auth_key = [7u8; 32];
        let wrap_key = [9u8; 16];
        let data = b"the quick brown fox jumps over the lazy dog";
        let blob = wrap(&auth_key, &wrap_key, data);
        // IV is appended, so the blob is longer than the padded ciphertext.
        assert!(blob.len() >= data.len() + KWA_LEN + IV_LEN);
        let back = unwrap(&auth_key, &wrap_key, &blob).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn tampered_authenticator_is_rejected() {
        let auth_key = [7u8; 32];
        let wrap_key = [9u8; 16];
        let blob = wrap(&auth_key, &wrap_key, b"secret");
        // Decrypt with a different auth key -> KWA mismatch.
        let other = [8u8; 32];
        assert!(unwrap(&other, &wrap_key, &blob).is_err());
    }
}
