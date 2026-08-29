//! AES-128-CBC key wrapping with 8-byte HMAC authenticator (protocol §4.6).
//!
//! Wire format quirk: the IV is transmitted *after* the ciphertext.

// TODO(FR-REG-1): wrap(auth_key, wrap_key, data) / unwrap(...) with
// test vectors captured from the reference implementation (NFR-QLT-2).
