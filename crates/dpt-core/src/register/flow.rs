//! The six-message handshake state machine M1…M6 (protocol §4.3, §4.7).
//!
//! Pure (no I/O): each step consumes the previous device message and
//! produces the next client message, so the whole flow is testable against
//! recorded fixtures. The HTTP transport lives in the parent module.

// TODO(FR-REG-1): RegistrationFlow { new(m1) -> m2, on_m3(pin) -> m4,
// on_m5() -> m6 + Registration } with full HMAC chain verification.
