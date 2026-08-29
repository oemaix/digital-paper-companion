# 03 — Non-Functional Requirements

## 1. Platforms and packaging (NFR-PLT)

| ID | Requirement |
|---|---|
| NFR-PLT-1 | Supported operating systems: Windows 10 (1809+) and 11 (x64, arm64), macOS 12+ (Intel and Apple Silicon, universal binary), Linux x64 with glibc ≥ 2.31 and WebKitGTK 4.1 (Ubuntu 22.04+, Fedora, Debian 12+, Arch). |
| NFR-PLT-2 | Distribution formats: `.msi`/`.exe` (Windows), `.dmg` (macOS), `.deb`, `.rpm` and AppImage (Linux). All produced by Tauri's bundler from one CI pipeline. |
| NFR-PLT-3 | No external runtime requirements (no Java, Python, or .NET). The installer size SHOULD stay under 25 MB per platform. |
| NFR-PLT-4 | Windows and macOS binaries SHOULD be code-signed; macOS builds notarized. (Requires certificates; see roadmap.) |
| NFR-PLT-5 | The app MUST work fully offline except for the device connection itself and the optional update check. |

## 2. Performance (NFR-PRF)

| ID | Requirement |
|---|---|
| NFR-PRF-1 | Cold start to interactive UI in < 2 s on reference hardware (2020 mid-range laptop). |
| NFR-PRF-2 | Full entry listing of a device with 1 000 entries renders in < 3 s on Wi-Fi; subsequent navigation within the cached tree is instant (< 50 ms). Lists MUST be virtualized; 10 000 entries must not degrade scrolling. |
| NFR-PRF-3 | Transfer throughput is limited by the device (~1–3 MB/s observed); the app MUST NOT add more than 10 % overhead versus a raw HTTP client, and MUST stream file bodies (no full-file buffering in memory). |
| NFR-PRF-4 | A no-change sync of 1 000 files completes in < 10 s. Change detection is metadata-only (no content hashing of unchanged files). |
| NFR-PRF-5 | Idle memory footprint < 200 MB including webview; the Rust core alone < 50 MB during a large sync. |
| NFR-PRF-6 | All device I/O runs off the UI thread. No UI action may block longer than 100 ms. |

## 3. Reliability (NFR-REL)

| ID | Requirement |
|---|---|
| NFR-REL-1 | No operation sequence — including killing the app mid-transfer or mid-sync — may corrupt local state files (config, credentials, checkpoints). All state files are written atomically (write-temp-then-rename). |
| NFR-REL-2 | Downloads are written to a temporary file and renamed into place only when complete and size-verified; partial downloads never appear at the destination path. |
| NFR-REL-3 | The app MUST tolerate device idiosyncrasies documented in protocol §10: string-typed JSON scalars, non-RFC `Set-Cookie`, 1300-entry listing truncation, transient registration failures. |
| NFR-REL-4 | Every device API call has a timeout (connect 5 s; request 30 s; file transfer stall detection 60 s without progress) and a defined retry policy (idempotent GETs: 2 retries with backoff; mutations: no automatic retry). |
| NFR-REL-5 | Crash of a background task (sync, transfer) MUST NOT crash the app; the failure is reported in the UI and log. |

## 4. Security (NFR-SEC)

Details and rationale in [07-data-and-security.md](07-data-and-security.md).

| ID | Requirement |
|---|---|
| NFR-SEC-1 | Client credentials (RSA private key, client ID) are stored in the OS keychain where available (Windows Credential Manager, macOS Keychain, Secret Service on Linux), with an encrypted file fallback. Never in plain text, never in logs. |
| NFR-SEC-2 | TLS to the device uses **certificate pinning** against the certificate obtained during registration; global TLS verification is never simply disabled. If pinning fails, the user is warned and must explicitly accept the new certificate (device may have been reset). |
| NFR-SEC-3 | All cryptographic operations use audited Rust crates (e.g. RustCrypto family / `ring`); no hand-rolled primitives beyond the protocol's specified composition. |
| NFR-SEC-4 | The app makes no network connections other than to the device, except the optional, user-disableable update check. No telemetry. |
| NFR-SEC-5 | Wi-Fi passphrases entered for the device are forwarded and immediately discarded; they are never persisted or logged by the app. |
| NFR-SEC-6 | Tauri IPC surface is minimal and typed; the webview cannot issue arbitrary HTTP requests or file operations outside the exposed commands (Tauri capability/permission configuration). |

## 5. Usability and accessibility (NFR-UX)

| ID | Requirement |
|---|---|
| NFR-UX-1 | Every long-running operation shows progress and can be cancelled. |
| NFR-UX-2 | The UI follows the design system in [05-ui-ux-specification.md](05-ui-ux-specification.md): light and dark themes following the OS setting, with manual override. |
| NFR-UX-3 | Full keyboard operability of primary flows (browse, upload, download, sync). Standard shortcuts per platform (Cmd on macOS, Ctrl elsewhere). |
| NFR-UX-4 | Interactive elements carry accessible names/roles (webview exposes them to OS screen readers); color is never the only carrier of meaning; contrast meets WCAG 2.1 AA. |
| NFR-UX-5 | Destructive actions (device delete, mirror-mode deletions) require explicit confirmation and name the affected items or counts. |

## 6. Internationalization (NFR-I18N)

| ID | Requirement |
|---|---|
| NFR-I18N-1 | All user-facing strings live in locale resource files; English (`en`) is the source language. German (`de`) is the first translation target. |
| NFR-I18N-2 | Dates, times and file sizes are formatted per the OS locale. |
| NFR-I18N-3 | Device file names are Unicode; the app MUST handle NFC/NFD normalization differences between device paths and local filesystems (notably macOS), per sync spec §4.1. |

## 7. Maintainability and quality (NFR-QLT)

| ID | Requirement |
|---|---|
| NFR-QLT-1 | The protocol implementation is a separate Rust library crate (`dpt-core`) with no Tauri dependency, unit-tested against recorded fixtures, and usable by third parties (e.g. a future CLI). |
| NFR-QLT-2 | The registration handshake and key-wrap functions have test vectors (captured from a real device or the `dptrp1` reference implementation) exercised in CI. |
| NFR-QLT-3 | The sync engine has a property-based/scenario test suite covering the decision table in 06 §5, runnable against an in-memory fake device. |
| NFR-QLT-4 | CI builds and tests on all three OS targets on every merge; releases are tagged and reproducible. |
| NFR-QLT-5 | Structured logging (`tracing`) with rotating log files; a "Reveal log file" action exists in the UI for bug reports. Log level configurable. |
| NFR-QLT-6 | License: open source (MIT or Apache-2.0 dual license, matching the Rust ecosystem convention). No Sony trademarks in app name or icon. |
