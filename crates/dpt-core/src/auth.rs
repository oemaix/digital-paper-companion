//! Session authentication (protocol §5; FR-REG-4).
//!
//! Flow: `GET /auth/nonce/{client_id}` → sign the *Base64 text* of the nonce
//! (not its decoded bytes) with RSA-PKCS#1-v1.5/SHA-256 → `PUT /auth` →
//! extract the `Credentials` cookie (the device's `Set-Cookie` is not fully
//! RFC-compliant; parse manually, protocol §10.5).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use sha2::Sha256;

use crate::Error;

/// Signs the nonce string with the client's RSA private key and returns the
/// Base64-encoded signature. The ASCII bytes of the nonce *string* are
/// signed, exactly as received (protocol §5 step 2).
pub fn sign_nonce(private_key_pem: &str, nonce: &str) -> Result<String, Error> {
    let key = RsaPrivateKey::from_pkcs8_pem(private_key_pem)
        .map_err(|e| Error::Auth(format!("cannot load private key: {e}")))?;
    let signing_key = SigningKey::<Sha256>::new(key);
    let signature = signing_key
        .try_sign(nonce.as_bytes())
        .map_err(|e| Error::Auth(format!("signing failed: {e}")))?;
    Ok(B64.encode(signature.to_bytes()))
}

/// Extracts the `Credentials` cookie value from a `Set-Cookie` header value,
/// tolerating the device's non-RFC formatting: take the part before the
/// first `; `, split on the first `=`, and use the remainder.
pub fn extract_credentials_cookie(set_cookie: &str) -> Option<String> {
    let first = set_cookie.split("; ").next()?;
    let (name, value) = first.split_once('=')?;
    if name.trim() == "Credentials" {
        Some(value.trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_non_rfc_set_cookie() {
        let hdr = "Credentials=abc.def-123; Path=/; HttpOnly";
        assert_eq!(
            extract_credentials_cookie(hdr).as_deref(),
            Some("abc.def-123")
        );
        assert_eq!(extract_credentials_cookie("Other=1; Path=/"), None);
    }
}
