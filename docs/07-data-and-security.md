# 07 — Data Storage and Security

## 1. Local data inventory

All app data lives in the platform-standard locations resolved by Tauri:

| Data | Location | Format |
|---|---|---|
| App settings (theme, language, window state, behavior) | config dir¹ `settings.json` | JSON, atomic writes |
| Known devices registry (serial, name, model, last addresses, cert fingerprint) | config dir `devices.json` | JSON |
| **Client credentials per device** (client ID + RSA private key) | OS keychain (see §2) | PEM/UUID in keychain entry |
| Pinned device certificate per device | config dir `certs/{serial}.pem` | PEM (public data) |
| Sync pair configurations | config dir `sync-pairs.json` | JSON |
| Sync checkpoints | data dir² `checkpoints/{pair_id}.json` | JSON (schema in 06 §7) |
| Sync run history | data dir `sync-history/{pair_id}.jsonl` | JSON lines, capped at 100 runs |
| Logs | log dir³ `dpc.log` (rotated, 7 files × 5 MB) | text (`tracing`) |
| Preview cache | OS temp `dpc-preview/` | PDFs, purged on exit |

¹ e.g. `%APPDATA%/digital-paper-companion`, `~/Library/Application Support/…`,
`~/.config/digital-paper-companion`
² platform data dir (may equal config dir on Windows/Linux)
³ platform log dir (`~/Library/Logs/…` on macOS)

Rules:

- Every JSON file carries a `"version"` field; readers migrate forward and
  never destroy files they can't parse (rename to `*.bad` and recreate).
- All writes are write-temp-then-rename (NFR-REL-1).
- "Forget this device" (FR-REG-5) removes the keychain entry, the pinned
  cert, the device registry record, and (after separate confirmation) the
  device's sync pairs and checkpoints. Local synced files are never touched.

## 2. Credential handling (NFR-SEC-1)

The pairing output (protocol §4) is the crown jewel: whoever holds the
client ID and RSA-2048 private key has full read/write access to the device
whenever reachable.

- **Primary store:** OS keychain via the `keyring` crate — one entry per
  device, service `digital-paper-companion`, account `dpt:{serial}`, secret
  = JSON `{ "client_id": …, "private_key_pem": … }`.
  - Windows: Credential Manager. macOS: Keychain. Linux: Secret Service
    (GNOME Keyring / KWallet).
- **Fallback (headless Linux / no Secret Service):** encrypted file
  `credentials/{serial}.enc` in the config dir — ChaCha20-Poly1305 with a
  key stored in `credentials/.key` (mode `0600`). This is obfuscation at
  the same trust level as the user account, equivalent to what Sony's app
  provided (`privatekey.dat` was plaintext); the UI settings page states
  which store is in use.
- Credentials never appear in logs, error messages, IPC payloads to the
  webview, or crash reports. The webview receives only the device serial
  and display name.
- **Import (FR-REG-6):** reads Sony's/`dptrp1`'s `deviceid.dat` +
  `privatekey.dat`, validates the key parses and authenticates against the
  device, then stores through the normal path. The source files are left
  untouched; the UI suggests deleting them.

## 3. TLS policy (NFR-SEC-2)

- The device presents a self-signed certificate issued by its on-device CA;
  the PEM is delivered during pairing (M5) and stored per device.
- The HTTPS client uses a custom verifier that accepts **exactly** the
  stored certificate (byte-equal DER comparison). Hostname verification is
  skipped (the cert isn't issued for the varying IPs) — the pin itself is
  the identity proof.
- Pin mismatch → connection refused, user dialog explaining that the device
  identity changed (likely factory reset) with the option to re-pair; the
  new cert is only accepted through a completed re-registration.
- Registration traffic on port 8080 is plain HTTP **by device design**;
  its security derives from the PIN-authenticated key exchange, not the
  transport. The wizard advises pairing on a trusted network.

## 4. Threat model (summary)

In scope:

| Threat | Mitigation |
|---|---|
| Theft of credentials at rest | OS keychain; encrypted fallback; no plaintext copies (§2) |
| Network attacker impersonating the device (MitM on 8443) | Certificate pinning (§3) |
| MitM during initial pairing on a hostile network | PIN-based mutual authentication in the handshake itself (protocol §4); UI guidance to pair on trusted networks |
| Malicious/compromised webview content escalating to system access | Tauri capability lockdown: no fs/http/shell APIs exposed; only typed commands (NFR-SEC-6); CSP with no remote content |
| Data loss through app bugs | Atomic writes, `*.part` downloads, conflict-copy policy, mass-deletion guard (06) |
| Leaking data to third parties | No telemetry; only device traffic + optional update check (NFR-SEC-4) |

Out of scope (documented, not defended):

- An attacker with the user's OS account (can read the keychain anyway).
- Physical access to an unlocked device (the device has its own lock
  feature, independent of this app).
- The device's own firmware security.

## 5. Privacy

- No analytics, no crash uploaders, no accounts.
- The optional update check requests a static JSON manifest over HTTPS and
  sends no identifiers beyond standard HTTP headers; it is on by default
  with a first-run notice and a settings toggle (FR-APP-5).
- Document contents are only ever transferred between the user's disk and
  the user's device.

## 6. Supply chain and release integrity

- Rust dependencies audited with `cargo audit`/`cargo deny` in CI; frontend
  with `npm audit` gating on high severity.
- Release artifacts are built in CI from tagged commits, checksummed
  (SHA-256 published alongside), and signed where certificates are
  available (NFR-PLT-4). Tauri's updater signature mechanism is used if
  in-app updates are enabled post-v1.
