//! USB connectivity: CDC ACM serial detection and Ethernet-over-USB mode
//! switch (protocol §2; FR-CONN-4, priority P2 — implemented in Phase 3).
//!
//! Writing a magic byte sequence to the device's serial port switches it
//! into a network mode; afterwards the device is reachable via its IPv6
//! link-local address (zone identifier required).

/// Switches the device into RNDIS mode (Windows-style Ethernet-over-USB).
pub const MODE_SWITCH_RNDIS: [u8; 10] = [0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04];

/// Switches the device into CDC/ECM mode (macOS/Linux-style Ethernet-over-USB).
pub const MODE_SWITCH_CDC_ECM: [u8; 10] = [0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x01, 0x04];

// TODO(FR-CONN-4): serial port enumeration + mode switch via `serialport`
// (crate dependency added when this lands; needs libudev, already in shell.nix).
