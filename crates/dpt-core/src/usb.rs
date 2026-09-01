//! USB connectivity: CDC ACM serial detection and Ethernet-over-USB mode
//! switch (protocol §2.1; FR-CONN-4).
//!
//! When plugged in, the device first enumerates as a USB CDC ACM serial
//! port (e.g. `/dev/ttyACM0`, `COM5`, `/dev/cu.usbmodem…`). Writing a
//! 10-byte command to that port re-enumerates it as Ethernet-over-USB
//! (RNDIS for Windows hosts, CDC/ECM for macOS; Linux accepts both).
//! Afterwards the device is reachable via its IPv6 link-local address with
//! the host interface as zone identifier — discovery runs over mDNS on the
//! new interface (`digitalpaper.local` / `Android.local`).
//!
//! The functions here are blocking (serial I/O); call them from a blocking
//! task when inside an async runtime.

use serde::{Deserialize, Serialize};

use crate::Error;

/// Switches the device into RNDIS mode (Windows-style Ethernet-over-USB).
pub const MODE_SWITCH_RNDIS: [u8; 10] =
    [0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04];

/// Switches the device into CDC/ECM mode (macOS/Linux-style Ethernet-over-USB).
pub const MODE_SWITCH_CDC_ECM: [u8; 10] =
    [0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x01, 0x04];

/// Sony's USB vendor id (DPT-RP1 / DPT-CP1).
pub const VID_SONY: u16 = 0x054c;
/// Fujitsu's USB vendor id (Quaderno).
pub const VID_FUJITSU: u16 = 0x04c5;

/// The two Ethernet-over-USB modes (protocol §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsbNetMode {
    Rndis,
    CdcEcm,
}

impl UsbNetMode {
    /// The mode the current host OS expects (protocol §2.1): RNDIS on
    /// Windows, CDC/ECM on macOS and Linux (Linux accepts either).
    pub fn for_host_os() -> Self {
        if cfg!(windows) {
            UsbNetMode::Rndis
        } else {
            UsbNetMode::CdcEcm
        }
    }

    fn command(self) -> [u8; 10] {
        match self {
            UsbNetMode::Rndis => MODE_SWITCH_RNDIS,
            UsbNetMode::CdcEcm => MODE_SWITCH_CDC_ECM,
        }
    }
}

/// A serial port that may be a Digital Paper waiting for the mode switch.
#[derive(Debug, Clone, Serialize)]
pub struct UsbCandidate {
    /// OS port name, e.g. `/dev/ttyACM0` or `COM5`.
    pub port: String,
    /// Human-readable label (product/manufacturer when known).
    pub label: String,
    /// True when the USB vendor id matches Sony or Fujitsu.
    pub likely_digital_paper: bool,
}

/// Lists serial ports that could be a Digital Paper in CDC ACM mode. Ports
/// with a Sony/Fujitsu vendor id are flagged; other USB serial ports are
/// still listed (vendor info is not always available, e.g. without libudev).
pub fn list_candidate_ports() -> Result<Vec<UsbCandidate>, Error> {
    let ports = serialport::available_ports().map_err(serial_err)?;
    let mut out = Vec::new();
    for p in ports {
        match p.port_type {
            serialport::SerialPortType::UsbPort(usb) => {
                let likely = usb.vid == VID_SONY || usb.vid == VID_FUJITSU;
                let label = usb
                    .product
                    .or(usb.manufacturer)
                    .unwrap_or_else(|| format!("USB serial ({:04x}:{:04x})", usb.vid, usb.pid));
                out.push(UsbCandidate {
                    port: p.port_name,
                    label,
                    likely_digital_paper: likely,
                });
            }
            // Without udev metadata, ACM-style names are still worth offering.
            _ if p.port_name.contains("ttyACM") || p.port_name.contains("usbmodem") => {
                out.push(UsbCandidate {
                    port: p.port_name,
                    label: "Serial port".into(),
                    likely_digital_paper: false,
                });
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| {
        b.likely_digital_paper
            .cmp(&a.likely_digital_paper)
            .then_with(|| a.port.cmp(&b.port))
    });
    Ok(out)
}

/// Writes the mode-switch command to the given serial port (blocking). On
/// success the device drops off the serial bus and re-enumerates as a
/// network interface within a few seconds; the host must then use
/// link-local addressing on that interface (no DHCP, protocol §2.1).
pub fn switch_mode(port: &str, mode: UsbNetMode) -> Result<(), Error> {
    use std::io::Write;
    let mut sp = serialport::new(port, 9600)
        .timeout(std::time::Duration::from_secs(3))
        .open()
        .map_err(serial_err)?;
    sp.write_all(&mode.command()).map_err(Error::Io)?;
    sp.flush().map_err(Error::Io)?;
    Ok(())
}

fn serial_err(e: serialport::Error) -> Error {
    Error::Io(std::io::Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_commands_differ_only_in_mode_byte() {
        assert_eq!(UsbNetMode::Rndis.command()[8], 0x00);
        assert_eq!(UsbNetMode::CdcEcm.command()[8], 0x01);
        assert_eq!(
            UsbNetMode::Rndis.command()[..8],
            UsbNetMode::CdcEcm.command()[..8]
        );
        assert_eq!(UsbNetMode::Rndis.command()[9], 0x04);
    }
}
