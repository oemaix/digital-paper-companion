# 04 — Technical Architecture

## 1. Overview

The application is a **Tauri 2** desktop app with three layers:

```
┌───────────────────────────────────────────────────────────┐
│  Frontend (webview)                                       │
│  TypeScript · React · Vite · Tailwind CSS                 │
│  Views, state, i18n — no protocol or filesystem logic     │
├──────────────────── Tauri IPC (commands/events) ──────────┤
│  App layer (Rust, crate `dpt-app`)                        │
│  Tauri commands, event emission, task orchestration,      │
│  settings store, credential store, transfer queue,        │
│  sync scheduler, tray integration                         │
├───────────────────────────────────────────────────────────┤
│  Core library (Rust, crate `dpt-core`, no Tauri deps)     │
│  discovery · registration crypto · session auth ·         │
│  typed API client · sync engine · USB mode switch         │
└───────────────────────────────────────────────────────────┘
                    HTTPS :8443 / HTTP :8080
                             │
                        DPT device
```

Guiding rules:

- **All protocol, crypto, filesystem and sync logic lives in Rust.** The
  frontend renders state and sends user intents.
- **`dpt-core` is UI-agnostic and independently testable** (NFR-QLT-1). It
  could later power a CLI.
- The webview is locked down via Tauri capabilities: only the declared
  commands are callable; no generic fs/http/shell access (NFR-SEC-6).

## 2. Repository layout

```
digital-paper-companion/
├── docs/                       # this documentation
├── crates/
│   └── dpt-core/               # protocol + sync library
│       └── src/
│           ├── discovery.rs    # mDNS browsing, manual address probe
│           ├── usb.rs          # CDC ACM detection + mode-switch bytes
│           ├── register/       # §4 handshake: dh.rs, kdf.rs, wrap.rs, flow.rs
│           ├── auth.rs         # §5 nonce signing, cookie handling
│           ├── client.rs       # authenticated HTTPS client (pinning, retries)
│           ├── api/            # typed endpoints: entries.rs, templates.rs,
│           │                   #   system.rs, wifi.rs, viewer.rs
│           ├── model.rs        # Entry, DeviceInfo, … (serde types)
│           └── sync/           # engine per docs/06: snapshot.rs, plan.rs,
│                               #   apply.rs, checkpoint.rs
├── src-tauri/                  # Tauri application crate `dpt-app`
│   │                           # (directory name fixed by the Tauri CLI)
│   ├── tauri.conf.json
│   ├── capabilities/           # webview permission grants (minimal)
│   ├── icons/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── commands/           # IPC command handlers per domain
│       ├── state.rs            # AppState: connection, caches, queues
│       ├── transfers.rs        # transfer queue worker
│       ├── scheduler.rs        # sync scheduling
│       ├── credentials.rs      # keychain integration
│       └── settings.rs         # settings persistence
├── src/                        # frontend (React + TS)
│   ├── views/                  # per-screen components (see docs/05)
│   ├── components/             # design-system components
│   ├── ipc/                    # generated/typed command + event bindings
│   ├── stores/                 # UI state (device, entries, transfers, sync)
│   └── i18n/
├── shell.nix                   # Nix dev environment (Rust, Node, Tauri libs)
└── package.json, Cargo.toml (workspace), CI config
```

## 3. Technology choices

| Concern | Choice | Rationale |
|---|---|---|
| Shell | **Tauri 2** | Small bundles, native webviews, Rust backend, mature bundler for all three OS |
| Frontend | **React + TypeScript + Vite** | Large ecosystem, good virtual-list and DnD libraries; any team member can contribute |
| Styling | **Tailwind CSS** + small component library per docs/05 | Fast iteration, consistent theming, easy dark mode |
| HTTP | `reqwest` (rustls) | Streaming bodies, per-client TLS config for pinning |
| TLS pinning | `rustls` custom `ServerCertVerifier` comparing against the stored M5 certificate | NFR-SEC-2 |
| Crypto | RustCrypto crates: `sha2`, `hmac`, `pbkdf2`, `aes` + `cbc`, `rsa`; DH group 14 via `num-bigint` modpow | Audited, pure Rust, covers protocol Appendix A |
| mDNS | `mdns-sd` | Pure Rust service browsing |
| USB serial | `serialport` | Port enumeration + writing the mode-switch bytes |
| Keychain | `keyring` | Uniform API over Credential Manager / Keychain / Secret Service |
| Async | `tokio` | Backbone for I/O, queues, scheduler |
| Logging | `tracing` + `tracing-appender` | NFR-QLT-5 |
| Frontend state | `zustand` (or equivalent light store) | Simple event-driven sync with backend |

## 4. Core library design (`dpt-core`)

### 4.1 Connection lifecycle

```rust
pub struct DeviceClient { /* base URLs, reqwest client, session cookie */ }

impl DeviceClient {
    /// Probe an address: GET :8080/register/information
    pub async fn probe(addr: &DeviceAddr) -> Result<DeviceInfo>;

    /// One-time pairing (§4). `pin` is requested via callback mid-flow.
    pub async fn register(
        addr: &DeviceAddr,
        pin_provider: impl PinProvider,
    ) -> Result<Registration>;   // { client_id, private_key_pem, device_cert_pem }

    /// Open an authenticated session (§5) with stored credentials.
    pub async fn connect(addr: &DeviceAddr, creds: &Credentials) -> Result<Self>;

    pub async fn ping(&self) -> Result<()>;
}
```

- `connect` builds a `reqwest::Client` whose TLS verifier pins the stored
  device certificate.
- Every API method transparently re-authenticates once on a 401-class
  response before failing (FR-REG-4).
- The registration flow is implemented as a pure state machine
  (`register/flow.rs`) whose crypto steps take byte slices in and out — unit
  testable against fixtures without network (NFR-QLT-2).

### 4.2 Typed API surface

All endpoint wrappers return typed models; the string-typed JSON scalars of
the device (protocol §6.1) are converted at the serde boundary with dedicated
deserializers (`string_bool`, `string_u64`, `string_date`). Key methods:

- `list_all_entries()` — implements the 1300-entry fallback internally and
  always returns the complete tree (FR-BRW-1).
- `download_document(id) -> impl Stream<Bytes>` and
  `upload_document(parent_id, name, body: impl Stream) -> EntryId` — both
  streaming (NFR-PRF-3); upload performs the two-step create+put and deletes
  the created entry if the content upload fails (FR-TRF-10).
- Folder create/delete, move/rename/copy, template CRUD, system
  configs/status, Wi-Fi management, viewer open, screenshot — one method per
  protocol §7 endpoint.

### 4.3 Sync engine

Implemented exactly per [06-sync-specification.md](06-sync-specification.md).
Structure:

```
sync::snapshot   — build LocalTree / RemoteTree / Checkpoint views
sync::plan       — pure function (3 views + mode + filters) -> Vec<Action>
sync::apply      — executes actions via DeviceClient + std::fs, streams
                   progress events, advances checkpoint per completed action
```

`plan` being a pure function is the linchpin for testing (NFR-QLT-3): the
scenario suite feeds synthetic trees and asserts on the produced actions.

## 5. App layer (`dpt-app`)

### 5.1 State

A single `AppState` (behind `tokio::sync::RwLock`/actors) holds:

- `connection: ConnectionState` — `Disconnected | Connecting | Connected(DeviceClient) | Reauthenticating`
- `entry_cache: Option<EntryTree>` + refresh generation counter
- `transfer_queue: TransferQueue` — ordered jobs with status, executed by a
  worker pool (concurrency 2, FR-TRF-8)
- `sync_state: per-pair status (idle/running/progress/last result)`
- `settings`, `known_devices`

A background *connection supervisor* task owns reconnection with exponential
backoff and pings every 15 s while connected (FR-CONN-5/6), emitting state
events.

### 5.2 IPC contract

Commands (frontend → Rust), grouped; all return `Result<T, AppError>` where
`AppError` is `{ code, message, detail? }` rendered per FR-APP-3:

| Domain | Commands |
|---|---|
| discovery | `start_discovery`, `stop_discovery`, `probe_address(addr)` |
| pairing | `start_registration(addr)`, `submit_pin(pin)`, `cancel_registration`, `forget_device(serial)`, `import_credentials(paths)` |
| connection | `connect(serial)`, `disconnect`, `get_connection_state` |
| entries | `refresh_entries`, `get_entries`, `create_folder`, `rename_entry`, `move_entries`, `copy_entries`, `delete_entries`, `preview_document(id)`, `open_on_device(id, page)` |
| transfers | `enqueue_uploads(items)`, `enqueue_downloads(items, dest)`, `cancel_transfer(id)`, `retry_transfer(id)`, `clear_finished` |
| templates | `list_templates`, `upload_template(name, path)`, `delete_template(id)` |
| system | `get_device_status`, `get_configs`, `set_config(key, value)`, `set_clock_now`, `wifi_*`, `take_screenshot(dest?)` |
| sync | `list_sync_pairs`, `upsert_sync_pair(cfg)`, `delete_sync_pair(id)`, `plan_sync(id)` (dry run), `run_sync(id)`, `cancel_sync(id)`, `get_sync_log(id)` |

Events (Rust → frontend), pushed via Tauri events:

- `connection:changed`, `discovery:device-found/-lost`
- `registration:pin-required`, `registration:finished`
- `entries:invalidated`
- `transfer:updated` (job snapshot), `transfer:queue-drained`
- `sync:progress`, `sync:confirmation-required` (mass-deletion gate, FR-SYN-5), `sync:finished`

The TypeScript side of this contract is generated (e.g. via `specta`/
`tauri-specta`) so frontend and backend cannot drift.

### 5.3 Filesystem access policy

The webview never touches the filesystem. File/folder pickers use the Tauri
dialog plugin; drag & drop uses Tauri's native file-drop events, which
deliver OS paths to Rust. Downloads, previews and sync I/O happen entirely in
the Rust layer.

## 6. Cross-cutting concerns

- **Errors:** `dpt-core` uses `thiserror` enums per domain
  (`RegistrationError`, `ApiError { status, device_message }`,
  `SyncError`); `dpt-app` maps them to user-presentable `AppError`s and logs
  full detail.
- **Cancellation:** every long task (transfer, sync, discovery) owns a
  `CancellationToken`; cancellation is cooperative and prompt (< 1 s).
- **Temp files:** previews in the OS temp dir under an app-scoped folder,
  cleared on exit (FR-BRW-5); download/sync writes use `*.part` +
  atomic rename (NFR-REL-2).
- **Time:** the device clock is set (FR-SET-3) before syncs; all internal
  timestamps are UTC `chrono::DateTime<Utc>`.

## 7. Build, CI, release

- Cargo workspace + npm workspace; `tauri build` produces all bundles
  (NFR-PLT-2).
- CI matrix (GitHub Actions): lint (`clippy`, `eslint`), unit tests
  (`dpt-core` incl. crypto vectors and sync scenarios), frontend tests,
  `tauri build` per OS. Releases on tags upload signed artifacts.
- A minimal **fake device server** (Rust, in `dpt-core` dev-dependencies)
  implements enough of §4–§7 of the protocol for integration tests of
  registration, listing, transfer and sync without hardware.
