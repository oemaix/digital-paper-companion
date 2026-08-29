//! Diffie-Hellman exchange, RFC 3526 MODP group 14 (protocol §4.4).

// TODO(FR-REG-1): group-14 keypair generation (256-bit private key),
// `ya` wire encoding (0x00 ‖ 256-byte big-endian), shared-secret
// computation from raw `yb` bytes, NIST SP 800-56 validation.
