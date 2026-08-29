# 02 — Functional Requirements

Requirements are grouped by feature area. Each requirement has a stable ID,
a priority (**P1** = must have for v1, **P2** = should have for v1, **P3** =
later release), and, where relevant, a pointer to the protocol section that
implements it.

Protocol references (`§n`) refer to
[sony-digital-paper-protocol.md](sony-digital-paper-protocol.md).

---

## 1. Device discovery and connection (CONN)

| ID | P | Requirement |
|---|---|---|
| FR-CONN-1 | P1 | The app MUST discover devices on the local network via mDNS/DNS-SD (service types `_digitalpaper._tcp.local.` and `_dp_fujitsu._tcp.local.`) and display each discovered device with model and serial number (via `GET /register/information`). (§3) |
| FR-CONN-2 | P1 | The app MUST allow the user to enter a device address manually (IPv4, IPv6 with zone ID, or hostname such as `digitalpaper.local`), for cases where mDNS discovery fails. |
| FR-CONN-3 | P1 | The app MUST support connections over Wi-Fi and Bluetooth PAN (default device address `172.25.47.1`). (§2) |
| FR-CONN-4 | P2 | The app SHOULD support USB connections: detect the device's USB CDC ACM serial port, write the mode-switch byte sequence (RNDIS on Windows, CDC/ECM on macOS/Linux), and connect to the device's IPv6 link-local address with the correct zone identifier. (§2) |
| FR-CONN-5 | P1 | The app MUST monitor connection health (periodic `GET /ping`) and reflect the state in the UI: *disconnected*, *connecting*, *connected*, *reauthenticating*. |
| FR-CONN-6 | P1 | On connection loss the app MUST retry automatically with backoff and re-authenticate transparently when the device becomes reachable again; in-flight transfers are failed cleanly and reported (see FR-TRF-10). |
| FR-CONN-7 | P2 | The app SHOULD remember multiple registered devices (keyed by serial number) and let the user switch the active device. Only one device is connected at a time. |
| FR-CONN-8 | P1 | The app MUST auto-connect on startup to the most recently used device if it is reachable. |

## 2. Registration / pairing (REG)

| ID | P | Requirement |
|---|---|---|
| FR-REG-1 | P1 | The app MUST implement the full six-message registration handshake (DH group 14, PBKDF2, HMAC chaining, AES key-wrap) exactly as specified in §4, including the known pitfalls of §10 (raw `yb` bytes, 257-byte `ya`, trailing IV). |
| FR-REG-2 | P1 | The pairing UI MUST be a guided wizard: choose device → app requests PIN → user reads the PIN from the device screen and types it into the app → success/failure feedback. A failed PIN or handshake MUST be retryable without restarting the app (issuing `PUT /register/cleanup` first). |
| FR-REG-3 | P1 | On success the app MUST persist the client ID, the RSA-2048 private key and the device certificate from message M5, per the storage rules of [07-data-and-security.md](07-data-and-security.md). |
| FR-REG-4 | P1 | The app MUST establish sessions per §5 (nonce → RSA-PKCS#1-v1.5-SHA256 signature → `Credentials` cookie), tolerate the device's non-RFC `Set-Cookie` format, and refresh the session automatically when the device invalidates it. |
| FR-REG-5 | P2 | The app SHOULD offer "Forget this device", which deletes the stored credentials, certificate and per-device settings after confirmation. |
| FR-REG-6 | P2 | The app SHOULD import existing credentials from Sony's official app or `dptrp1` (`deviceid.dat` / `privatekey.dat`), so already-paired users don't need to re-pair. Default search locations per [07-data-and-security.md](07-data-and-security.md) §2; a manual file picker is the fallback. |

## 3. Device settings and status (SET)

| ID | P | Requirement |
|---|---|---|
| FR-SET-1 | P1 | The app MUST display device information: model, serial number, firmware version, MAC address, battery level and charging state, storage used/total. (§7.8) |
| FR-SET-2 | P1 | The app MUST allow reading and editing device configuration values via `/system/configs/`: owner name, date & time, timezone, date/time display format, standby timeout. Unknown keys returned by `GET /system/configs/` SHOULD be shown in an "Advanced" section as generic key/value editors. (§7.7) |
| FR-SET-3 | P1 | The app MUST offer "Set clock from this computer" writing the current UTC time to `datetime`, and SHOULD do this automatically before every sync run (see 06). |
| FR-SET-4 | P2 | The app SHOULD manage the device's Wi-Fi: toggle the radio, list stored networks, scan for visible networks, add a network (WPA-PSK or open, DHCP or static, with proxy flag), and remove stored networks. SSIDs are Base64-wrapped per §7.6. Wi-Fi passwords entered by the user are sent to the device and never stored by the app. |
| FR-SET-5 | P2 | The app SHOULD capture a screenshot of the device screen (`/system/controls/screen_shot`) and let the user save it as PNG/JPEG or copy it to the clipboard. (§7.9) |
| FR-SET-6 | P3 | The app MAY support the firmware update flow (upload, precheck, trigger; §7.10) with prominent warnings; deferred to a post-v1 release. |

## 4. Content browsing (BRW)

| ID | P | Requirement |
|---|---|---|
| FR-BRW-1 | P1 | The app MUST show the device's storage as a navigable folder tree rooted at `Document` (displayed as "System Storage" or the device name). Listing uses `GET /documents2?entry_type=all` and MUST fall back to recursive per-folder listing when the response is truncated at the ~1300-entry limit. (§7.3.2, §7.3.3, §10.8) |
| FR-BRW-2 | P1 | For every entry the app MUST display: name, type icon, size, page count, modified date, and an "unread" indicator (`is_new`). Sorting by name, date and size, plus filtering by type MUST be available. |
| FR-BRW-3 | P1 | The app MUST provide client-side search over names and paths of the cached entry list. |
| FR-BRW-4 | P1 | Notes MUST be recognizable as such (documents under the device's note folder) and reachable via a dedicated "Notes" view in addition to the plain tree. |
| FR-BRW-5 | P1 | "Preview" on a document MUST download it to a temporary location and open it with the OS default PDF viewer. Previewed files are cached per session and cleaned up on exit. |
| FR-BRW-6 | P2 | The app SHOULD offer "Open on device": open the selected document (optionally at a page number) on the device screen via `PUT /viewer/controls/open2`. (§7.5) |
| FR-BRW-7 | P1 | The app MUST list note templates (name + ID) in a dedicated templates view. (§7.4) |
| FR-BRW-8 | P2 | The entry list SHOULD be cached in memory and refreshed on demand (manual refresh button) and automatically after any mutating operation. |

## 5. Transfer and content management (TRF)

| ID | P | Requirement |
|---|---|---|
| FR-TRF-1 | P1 | The app MUST upload PDF files to any device folder: via file picker and via drag & drop from the OS file manager onto the folder view. Multi-file upload MUST be supported. |
| FR-TRF-2 | P1 | Uploading a folder from disk MUST recreate its subfolder structure on the device (folders created one level at a time, §7.3.6) and upload all contained PDFs. Non-PDF files are skipped with a per-file notice. |
| FR-TRF-3 | P1 | When an upload targets an existing device path, the app MUST ask the user: overwrite (replace content via `PUT /documents/{id}/file`), keep both (auto-renamed copy), or skip. A "apply to all" option MUST be available for batch operations. |
| FR-TRF-4 | P1 | The app MUST download any document or note to a user-chosen location, and download a folder recursively preserving structure. Drag & drop from the app to the OS file manager SHOULD be supported where the platform allows. |
| FR-TRF-5 | P1 | The app MUST support device-side operations: create folder, rename, move (with folder picker), copy, and delete for documents and folders. Deletes MUST require confirmation and state that device deletion is unrecoverable. |
| FR-TRF-6 | P1 | The app MUST upload and delete note templates (`POST /viewer/configs/note_templates` + file upload; `DELETE …/{id}`), including the camelCase `templateName` quirk. (§7.4) |
| FR-TRF-7 | P1 | All transfers MUST run asynchronously with a visible queue: per-item progress, overall progress, cancel (per item and all), and a completed/failed summary. The UI MUST stay responsive during transfers. |
| FR-TRF-8 | P2 | Transfers SHOULD be parallelized conservatively (small fixed concurrency, e.g. 2) to keep the device responsive. |
| FR-TRF-9 | P1 | The app MUST validate that uploaded files are PDFs (magic bytes `%PDF`) before uploading and refuse others with a clear message. |
| FR-TRF-10 | P1 | A transfer interrupted by connection loss MUST be marked failed (not silently partial); after reconnection the user can retry failed items from the queue. Failed uploads MUST NOT leave zero-byte ghost entries: the app deletes an entry it created if the subsequent content upload failed. |

## 6. Sync (SYN)

Detailed behavior is specified in [06-sync-specification.md](06-sync-specification.md);
these requirements define the user-facing contract.

| ID | P | Requirement |
|---|---|---|
| FR-SYN-1 | P1 | The user MUST be able to create at least one **sync pair**: a local folder plus a device subtree (default: the whole `Document` tree). |
| FR-SYN-2 | P1 | Each sync pair MUST support three modes: **two-way** (default), **mirror to computer** (device wins; local extra files deleted after confirmation policy), **mirror to device** (computer wins). |
| FR-SYN-3 | P1 | The user MUST be able to trigger a sync manually at any time (toolbar button and tray/menu action). |
| FR-SYN-4 | P1 | Scheduled sync MUST be supported: on device connect, and/or every N minutes while connected. Schedules are per sync pair. |
| FR-SYN-5 | P1 | Before applying changes, the app MUST be able to show a **preview** (dry run) listing every planned upload, download and deletion. For scheduled runs the preview is skipped, but deletions beyond a configurable threshold (default 10) MUST pause the run and ask for confirmation. |
| FR-SYN-6 | P1 | Conflicts (both sides changed since the checkpoint) MUST default to "newer side wins, loser kept as a conflict copy" — no version is ever silently discarded. See 06 §5. |
| FR-SYN-7 | P1 | Every sync run MUST produce a persistent, human-readable log entry (time, pair, actions taken, errors) viewable in the app. |
| FR-SYN-8 | P2 | The app SHOULD expose per-pair filters: include/exclude by subpath and glob pattern (e.g. exclude `Note/`). |
| FR-SYN-9 | P1 | An interrupted sync (connection loss, app quit) MUST be resumable and MUST NOT corrupt the checkpoint: the checkpoint is only advanced for actions that completed. |

## 7. Application shell (APP)

| ID | P | Requirement |
|---|---|---|
| FR-APP-1 | P1 | The app MUST persist window state, active device, and all settings across restarts. |
| FR-APP-2 | P2 | The app SHOULD minimize to the system tray / menu bar with quick actions (sync now, open app, device status) where the platform supports it. |
| FR-APP-3 | P1 | All errors surfaced to the user MUST be actionable: what failed, why (device message if any), and what to try. Raw HTTP details go to the log, not the dialog. |
| FR-APP-4 | P2 | The app SHOULD support English as the base language with an i18n mechanism ready for additional languages (see NFR-I18N). |
| FR-APP-5 | P2 | The app SHOULD check for app updates (static JSON over HTTPS) and notify; no silent auto-install on any platform without user consent. |
