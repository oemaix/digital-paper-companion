//! Key derivation (protocol §4.5).
//!
//! PBKDF2-HMAC-SHA256(password = ZZ, salt = n1‖mac‖n2, 10 000 iterations,
//! 48 bytes) → authKey (32 bytes) ‖ keyWrapKey (16 bytes).

// TODO(FR-REG-1): derive_keys(zz, n1, mac, n2) -> (AuthKey, KeyWrapKey)
