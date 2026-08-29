# Sony Digital Paper (DPT-RP1 / DPT-CP1) Communication Protocol

This document is a complete, implementation-independent specification of the
network protocol spoken by Sony Digital Paper devices (DPT-RP1, DPT-CP1 and
compatible devices such as the Fujitsu Quaderno). It was reverse-engineered
from the behaviour of Sony's official Digital Paper App and is the protocol
implemented by the `dptrp1` Python library in this repository. It contains
everything needed to write a compatible client in any programming language.

**Contents**

1. [Overview](#1-overview)
2. [Transports and connectivity](#2-transports-and-connectivity)
3. [Device discovery](#3-device-discovery)
4. [Registration (pairing) protocol](#4-registration-pairing-protocol)
5. [Session authentication](#5-session-authentication)
6. [General API conventions](#6-general-api-conventions)
7. [Endpoint reference](#7-endpoint-reference)
   - [7.1 Unauthenticated endpoints](#71-unauthenticated-endpoints-http-port-8080)
   - [7.2 Authentication](#72-authentication)
   - [7.3 Documents and folders](#73-documents-and-folders)
   - [7.4 Note templates](#74-note-templates)
   - [7.5 Viewer control](#75-viewer-control)
   - [7.6 Wi-Fi](#76-wi-fi)
   - [7.7 System configuration](#77-system-configuration)
   - [7.8 System status](#78-system-status)
   - [7.9 Screenshots](#79-screenshots)
   - [7.10 Firmware update](#710-firmware-update)
8. [File uploads (multipart format)](#8-file-uploads-multipart-format)
9. [Error handling](#9-error-handling)
10. [Implementation checklist and pitfalls](#10-implementation-checklist-and-pitfalls)
11. [Appendix A: Cryptographic primitives](#appendix-a-cryptographic-primitives)
12. [Appendix B: Client-side sync (informative)](#appendix-b-client-side-sync-informative)

---

## 1. Overview

A Digital Paper device runs two HTTP servers:

| Server | Scheme | Port | Purpose |
|---|---|---|---|
| Registration server | plain HTTP | **8080** | Pairing (registration), device information, API version. No authentication. |
| Main API server | HTTPS (TLS) | **8443** | All device functionality. Requires a paired client and an authenticated session. |

The high-level lifecycle of a client is:

1. **Discover** the device's IP address (mDNS, fixed Bluetooth address, or
   user-supplied address).
2. **Register** (pair) once per client. This is an interactive, PIN-confirmed
   key-exchange over HTTP port 8080. It yields a *client ID* (a UUID chosen by
   the client) and a client-generated *RSA-2048 private key*, which the device
   remembers. It also returns a PEM certificate issued by the device's
   built-in CA.
3. **Authenticate** at the start of every session: fetch a nonce, sign it with
   the RSA private key, and exchange it for a session cookie named
   `Credentials`.
4. **Call the REST API** on HTTPS port 8443, sending the `Credentials` cookie
   with every request.

All request and response bodies (except raw file content and screenshots) are
JSON, encoded as UTF-8. Binary values inside JSON are Base64-encoded (standard
alphabet, with padding).

---

## 2. Transports and connectivity

The device is reachable over any IP transport:

- **Wi-Fi** — device and client on the same network. The device's IP address
  is shown in the device's Wi-Fi settings when tapping the connected network.
- **Bluetooth (PAN)** — the device typically has the fixed address
  `172.25.47.1`.
- **USB (Ethernet-over-USB)** — the device enumerates as a USB CDC ACM serial
  device (e.g. `/dev/ttyACM0` on Linux). Writing a magic byte sequence to that
  serial port switches it into a network mode:

  | Byte sequence written to serial port | Resulting mode |
  |---|---|
  | `01 00 00 01 00 00 00 01 00 04` | RNDIS (Windows-style Ethernet-over-USB) |
  | `01 00 00 01 00 00 00 01 01 04` | CDC/ECM (Mac-style Ethernet-over-USB) |

  In these modes the device uses an **IPv6 link-local address** and announces
  itself over mDNS as `digitalpaper.local` (Sony) or `Android.local` (Fujitsu
  Quaderno). The device does **not** run a DHCP server; the host side should
  use link-local addressing. When using the IPv6 link-local address, a zone
  (scope) identifier is required, e.g.
  `https://[fe80::xxxx:xxxx:xxxx:xxxx%usb0]:8443/...`. See
  [linux-ethernet-over-usb.md](linux-ethernet-over-usb.md) for details.

### TLS

The main API server presents a certificate signed by the device's own CA. The
per-device server certificate is delivered to the client during registration
(message M5, see below). Clients either:

- disable TLS certificate verification (what this library does), or
- pin/trust the certificate obtained during registration.

There is no client-certificate authentication at the TLS layer; the protocol
authenticates via the nonce-signing scheme in section 5.

---

## 3. Device discovery

The device advertises itself via **mDNS / DNS-SD** with these service types:

| Service type | Vendor |
|---|---|
| `_digitalpaper._tcp.local.` | Sony DPT-RP1 / DPT-CP1 |
| `_dp_fujitsu._tcp.local.` | Fujitsu Quaderno |

The mDNS service record contains the device's IP address(es) and a port
(observed to be **8080**, the registration server). Discovery only works for a
few minutes after the device's Wi-Fi setting is switched on.

To identify a discovered device, request (plain HTTP, no authentication):

```
GET http://{addr}:{mdns_port}/register/information
```

The JSON response includes at least a `serial_number` field, which can be used
to select a specific device when several are present. Discovery can also be
skipped entirely by resolving the hostnames `digitalpaper.local` /
`Android.local` via mDNS, or by letting the user supply an address.

---

## 4. Registration (pairing) protocol

Registration is a one-time, six-message authenticated key-exchange carried
over **plain HTTP on port 8080**. It combines:

- an anonymous **Diffie-Hellman** exchange (RFC 3526 group 14),
- key derivation via **PBKDF2-HMAC-SHA256**,
- mutual authentication via a **PIN displayed on the device screen**, and
- **HMAC-SHA256** integrity chaining of every message.

Its outcome:

- The client invents a **client ID** (a random UUIDv4 string) and generates an
  **RSA-2048 key pair** (public exponent 65537).
- The device stores the client ID and the client's RSA *public* key.
- The device sends the client a **PEM X.509 certificate** (the device's server
  certificate issued by its on-device CA), which may be used for TLS pinning.

The client must persist the client ID and the RSA private key (PEM); they are
the long-term credentials used by section 5. (Sony's Digital Paper App stores
them as `deviceid.dat` and `privatekey.dat`.)

### 4.1 Endpoints used

All bodies are JSON (`Content-Type: application/json`). All binary values are
Base64 strings.

| Step | Request | Body | Response |
|---|---|---|---|
| 0 | `PUT /register/cleanup` | none | `204 No Content` — aborts/cleans any half-finished registration |
| 1 | `POST /register/pin` | none | **M1** (JSON) — device shows a PIN on its screen |
| 2 | `POST /register/hash` | **M2** | **M3** |
| 3 | `POST /register/ca` | **M4** | **M5** |
| 4 | `POST /register` | **M6** | success status (registration committed) |
| 5 | `PUT /register/cleanup` | none | `204 No Content` |

If any parameter is malformed the device answers `403` with a message like
"Bad parameters for registration process".

### 4.2 Notation

- `‖` denotes byte-string concatenation.
- `HMAC(k, m)` is HMAC-SHA256 with key `k` over message `m` (32-byte output).
- `n1` — 16-byte nonce chosen by the **device** (sent in M1).
- `n2` — 16-byte random nonce chosen by the **client**.
- `mac` — an opaque byte string sent by the device in M1 (its identifier /
  MAC-address blob). The client never interprets it, only echoes and hashes it.
- `yb` — device's DH public key, **exact bytes as received** (see 4.4).
- `ya` — client's DH public key, encoded as described in 4.4.
- `PIN` — the digits shown on the device display, entered by the user,
  encoded as UTF-8.

### 4.3 Message flow

```
Client                                                Device
  |--- PUT  /register/cleanup ------------------------->|
  |--- POST /register/pin ----------------------------->|  (device displays PIN)
  |<-- M1 { a:n1, b:mac, c:yb } ------------------------|
  |    derive keys (4.4, 4.5)                           |
  |--- M2 { a:n1, b:n2, c:mac, d:ya, e:m2hmac } ------->|  POST /register/hash
  |<-- M3 { a:n2, b:eHash, e:m3hmac } ------------------|
  |    verify M3; user enters PIN                       |
  |--- M4 { a:n1, b:rHash, d:wrappedRs, e:m4hmac } ---->|  POST /register/ca
  |<-- M5 { a:n2, d:wrappedEsCert, e:m5hmac } ----------|
  |    verify M5; unwrap cert; verify eHash (PIN check) |
  |    generate RSA-2048 key pair + client_id (UUIDv4)  |
  |--- M6 { a:n1, d:wrappedDIDKPUBC, e:m6hmac } ------->|  POST /register
  |<-- success -----------------------------------------|
  |--- PUT  /register/cleanup ------------------------->|
```

### 4.4 Diffie-Hellman exchange

- **Group:** RFC 3526 MODP group 14 (2048-bit prime `p`, generator `g = 2`).
- **Client private key:** `a` = 256-bit (32-byte) random integer.
- **Client public key:** `A = g^a mod p`.
- **Encoding of `ya` (client public key on the wire):** a single `0x00` byte
  followed by `A` as a 256-byte big-endian integer — i.e. **257 bytes total**.
  This mimics Java's `BigInteger.toByteArray()` sign byte.
- **Decoding of `yb` (device public key, field `c` of M1):** Base64-decode and
  keep the **raw bytes exactly as received** for all HMAC computations. The
  device is a Java implementation and produces `yb` via
  `BigInteger.toByteArray()`, which prepends a `0x00` sign byte whenever the
  most significant bit is set — so `yb` is 257 bytes rather than 256 roughly
  half the time. Re-encoding `yb` to a fixed length makes the client's HMACs
  disagree with the device's, and the device then rejects M2 with HTTP 403.
  Only the shared-secret computation uses the integer interpretation:
  `yb_int = big-endian-integer(yb)`.
- **Shared secret:** `ZZ = yb_int^a mod p`, encoded as a **256-byte**
  big-endian integer. (Recommended: validate `2 ≤ yb_int ≤ p−2` and
  `yb_int^((p−1)/2) mod p = 1` per NIST SP 800-56.)

### 4.5 Key derivation

```
derivedKey = PBKDF2-HMAC-SHA256(
                 password   = ZZ                (256 bytes),
                 salt       = n1 ‖ mac ‖ n2,
                 iterations = 10000,
                 dkLen      = 48 bytes)

authKey    = derivedKey[0..31]    (32 bytes, HMAC key)
keyWrapKey = derivedKey[32..47]   (16 bytes, AES-128 key)
```

### 4.6 Key wrapping (`wrap` / `unwrap`)

Sensitive payloads are wrapped with AES-128-CBC plus an 8-byte key-wrap
authenticator (KWA):

```
wrap(data):
    kwa       = HMAC(authKey, data)[0..7]          # first 8 bytes
    iv        = 16 random bytes
    wrapped   = AES-128-CBC-Encrypt(keyWrapKey, iv,
                    PKCS7-pad(data ‖ kwa, blocksize 16))
    return wrapped ‖ iv                            # NOTE: IV is APPENDED

unwrap(blob):
    iv        = last 16 bytes of blob
    plaintext = PKCS7-unpad(AES-128-CBC-Decrypt(keyWrapKey, iv,
                    blob without last 16 bytes))
    kwa       = last 8 bytes of plaintext
    data      = plaintext without last 8 bytes
    check kwa == HMAC(authKey, data)[0..7]
    return data
```

Note the unusual detail that the IV is transmitted **after** the ciphertext,
not before.

### 4.7 Message contents and HMAC chaining

All fields are Base64 strings in a flat JSON object with single-letter keys.
Every message carries an HMAC (field `e`) computed with `authKey`; each HMAC
covers material from the previous message, chaining the whole handshake.

**M1** (device → client, response to `POST /register/pin`):

| Field | Value |
|---|---|
| `a` | `n1` — device nonce (16 bytes) |
| `b` | `mac` — opaque device identifier bytes |
| `c` | `yb` — device DH public key (256 or 257 bytes, see 4.4) |

**M2** (client → device, body of `POST /register/hash`):

| Field | Value |
|---|---|
| `a` | `n1` (echoed) |
| `b` | `n2` — fresh 16-byte client nonce |
| `c` | `mac` (echoed) |
| `d` | `ya` — client DH public key (257 bytes) |
| `e` | `m2hmac = HMAC(authKey, n1 ‖ mac ‖ yb ‖ n1 ‖ n2 ‖ mac ‖ ya)` |

**M3** (device → client, response to `POST /register/hash`):

| Field | Value |
|---|---|
| `a` | `n2` — client must verify it equals its own `n2` |
| `b` | `eHash` — device's PIN commitment (32 bytes, verified in step M5) |
| `e` | `m3hmac` — client must verify: `HMAC(authKey, n1 ‖ n2 ‖ mac ‖ ya ‖ m2hmac ‖ n2 ‖ eHash)` |

At this point the user reads the **PIN** from the device display. The client
computes:

```
psk       = HMAC(authKey, UTF8(PIN))
rs        = 16 random bytes
rHash     = HMAC(authKey, rs ‖ psk ‖ yb ‖ ya)
wrappedRs = wrap(rs)
```

**M4** (client → device, body of `POST /register/ca`):

| Field | Value |
|---|---|
| `a` | `n1` |
| `b` | `rHash` |
| `d` | `wrappedRs` |
| `e` | `m4hmac = HMAC(authKey, n2 ‖ eHash ‖ m3hmac ‖ n1 ‖ rHash ‖ wrappedRs)` |

**M5** (device → client, response to `POST /register/ca`):

| Field | Value |
|---|---|
| `a` | `n2` — verify against client's `n2` |
| `d` | `wrappedEsCert` — wrapped payload, see below |
| `e` | `m5hmac` — verify: `HMAC(authKey, n1 ‖ rHash ‖ wrappedRs ‖ m4hmac ‖ n2 ‖ wrappedEsCert)` |

The client unwraps `d`:

```
esCert = unwrap(wrappedEsCert)
es     = esCert[0..15]        # device's 16-byte secret nonce
cert   = esCert[16..]         # PEM-encoded X.509 certificate (UTF-8 text)
```

and verifies the device's PIN knowledge (mutual authentication):

```
eHash == HMAC(authKey, es ‖ psk ‖ yb ‖ ya)      # must hold, else abort
```

The client then generates its long-term identity:

```
rsaKey          = new RSA-2048 key pair, public exponent 65537
keyPubC         = public key, PEM-encoded ("-----BEGIN PUBLIC KEY-----", UTF-8 bytes)
client_id       = random UUIDv4 as lowercase ASCII string, e.g.
                  "6fa4b2c1-…", UTF-8 bytes
wrappedDIDKPUBC = wrap( UTF8(client_id) ‖ keyPubC )
```

**M6** (client → device, body of `POST /register`):

| Field | Value |
|---|---|
| `a` | `n1` |
| `d` | `wrappedDIDKPUBC` |
| `e` | `m6hmac = HMAC(authKey, n2 ‖ wrappedEsCert ‖ m5hmac ‖ n1 ‖ wrappedDIDKPUBC)` |

On success the device permanently associates `client_id` with the client's
RSA public key. Finish with `PUT /register/cleanup`.

**Persist:** `client_id` (string) and the RSA private key (PEM). Optionally
persist `cert` for TLS pinning.

---

## 5. Session authentication

Every session on the HTTPS API (port 8443) starts with a nonce-signature
exchange:

**Step 1 — fetch a nonce** (no authentication required):

```
GET https://{addr}:8443/auth/nonce/{client_id}
→ 200 { "nonce": "<base64 string>" }
```

**Step 2 — sign the nonce.** Sign the ASCII bytes of the nonce string exactly
as received (i.e. the Base64 text itself, *not* its decoded bytes) using
**RSA PKCS#1 v1.5 with SHA-256** and the private key from registration.
Base64-encode the 256-byte signature.

**Step 3 — exchange for a cookie:**

```
PUT https://{addr}:8443/auth
Content-Type: application/json

{ "client_id": "<client_id>", "nonce_signed": "<base64 RSA signature>" }
```

On success the response carries a header such as:

```
Set-Cookie: Credentials=<token>; Path=/; ...
```

> Note: the device's `Set-Cookie` format is not fully RFC-compliant and some
> HTTP libraries' cookie jars fail to parse it. If so, extract the value
> manually: take the part before the first `; `, split on `=`, and use the
> value.

**Step 4 — use the cookie.** Send `Cookie: Credentials=<token>` with every
subsequent request. There is no per-request signing. The cookie is valid until
the device invalidates it (e.g. reboot or a new authentication); re-run steps
1–3 to refresh.

`GET /ping` returns `2xx` when the session is valid and can be used to test
authentication.

---

## 6. General API conventions

Unless noted otherwise, everything below is on **HTTPS port 8443** and
requires the `Credentials` cookie.

### 6.1 Requests and responses

- Request bodies are JSON, `Content-Type: application/json`.
- Responses are JSON, except file/screenshot downloads (raw bytes).
- Success statuses are `200` (with body) or `204` (no body).
- Errors return a non-2xx status with a JSON body containing at least a
  `"message"` field (see section 9).
- **All JSON values are strings**, including numbers and booleans:
  `"file_size": "12345"`, `"is_new": "false"`, `"dhcp": "true"`.

### 6.2 The entry model (documents and folders)

The device exposes a single tree of *entries*. The root folder is named
**`Document`** (displayed as "System Storage" on the device). Every path
starts with `Document/`, e.g. `Document/Papers/file.pdf`.

Entries are identified by an `entry_id` — an opaque UUID string assigned by
the device. Path-based operations first resolve a path to an ID (7.3.1).

An entry object contains (fields observed; folders omit file-specific ones):

| Field | Type/format | Description |
|---|---|---|
| `entry_id` | UUID string | Unique ID, used in URLs |
| `entry_name` | string | File or folder name |
| `entry_path` | string | Full path, e.g. `Document/x/y.pdf` |
| `entry_type` | `"document"` or `"folder"` | |
| `parent_folder_id` | UUID string | ID of containing folder |
| `created_date` | `YYYY-MM-DDTHH:MM:SSZ` (UTC) | |
| `modified_date` | `YYYY-MM-DDTHH:MM:SSZ` (UTC) | Documents only |
| `reading_date` | `YYYY-MM-DDTHH:MM:SSZ` (UTC) | Last-read time, may be absent |
| `file_size` | numeric string | Bytes, documents only |
| `file_revision` | string, e.g. `a21ea4b1c368.2.0` | Changes on modification |
| `mime_type` | string, e.g. `application/pdf` | Documents only |
| `title` | string | PDF metadata title |
| `total_page` | numeric string | Page count |
| `is_new` | `"true"` / `"false"` | Unread flag |
| `document_source` | string | Origin marker, may be empty/absent |

The device stores **PDF files only**.

### 6.3 Path encoding in URLs

When a path is embedded in a URL (only the resolve endpoint, 7.3.1), it is
encoded like an HTML form value (`application/x-www-form-urlencoded` style):
every character except letters, digits and `-_.~` is percent-encoded and
spaces become `+` (Python's `quote_plus`). Example:
`Document/My Folder/a b.pdf` → `Document%2FMy+Folder%2Fa+b.pdf`.

---

## 7. Endpoint reference

### 7.1 Unauthenticated endpoints (HTTP, port 8080)

| Method & path | Description |
|---|---|
| `GET /register/information` | Device information JSON; includes `serial_number` (and further model/firmware fields). Also available authenticated on port 8443. |
| `GET /api_version` | `{ "value": "<api version string>" }` |
| `PUT /register/cleanup`, `POST /register/pin`, `POST /register/hash`, `POST /register/ca`, `POST /register` | Registration protocol, see section 4. |

### 7.2 Authentication

| Method & path | Body | Response |
|---|---|---|
| `GET /auth/nonce/{client_id}` | — | `{ "nonce": "<base64>" }` |
| `PUT /auth` | `{ "client_id": "...", "nonce_signed": "<base64>" }` | `Set-Cookie: Credentials=...` |
| `GET /ping` | — | 2xx if the session is valid |

### 7.3 Documents and folders

#### 7.3.1 Resolve a path to an entry

```
GET /resolve/entry/path/{form-encoded path}
```

Returns the full entry object (6.2) for the given path, or a non-2xx status
with `{"message": ...}` if the path does not exist. This is the standard way
to obtain an `entry_id` from a human-readable path.

#### 7.3.2 List all entries

```
GET /documents2?entry_type=all[&fields=field1,field2,...]
```

Response:

```json
{ "count": 123, "entry_list": [ { ...entry... }, ... ] }
```

- Lists **every document and folder on the device** in one call.
- Without `entry_type=all` the listing is restricted to documents.
- The optional `fields` parameter (comma-separated entry field names, e.g.
  `fields=entry_path,modified_date,entry_type`) reduces each entry to those
  fields, shrinking the response.
- **Limit:** the device returns at most ~1300 entries in `entry_list` even
  when `count` is larger. Detect this by comparing `count` with the length of
  `entry_list`; if they differ, fall back to recursive per-folder listing
  (7.3.3). No pagination mechanism is known.

#### 7.3.3 List folder contents

```
GET /folders/{folder_id}/entries
GET /folders/{folder_id}/entries2
```

Both return `{ "entry_list": [ ...entries... ] }` with the direct children of
the folder. (`entries2` is the variant used for recursive traversal; both
behave equivalently for this purpose.)

#### 7.3.4 Download a document

```
GET /documents/{document_id}/file
```

Response body is the raw PDF bytes.

#### 7.3.5 Upload a document

Uploading is a two-step process:

1. **Create the document entry** (skip if overwriting an existing document —
   resolve its ID instead and go straight to step 2):

   ```
   POST /documents2
   { "file_name": "name.pdf",
     "parent_folder_id": "<folder entry_id>",
     "document_source": "" }
   → 200 { "document_id": "<new id>", ... }
   ```

2. **Send the file content:**

   ```
   PUT /documents/{document_id}/file
   Content-Type: multipart/form-data          (see section 8)
   ```

   Uploading to an existing document's `/file` endpoint replaces its content.

#### 7.3.6 Create a folder

```
POST /folders2
{ "folder_name": "New Folder", "parent_folder_id": "<parent entry_id>" }
```

Parent folders must exist; create them one level at a time.

#### 7.3.7 Delete

```
DELETE /documents/{document_id}     # delete a document
DELETE /folders/{folder_id}         # delete a folder
```

#### 7.3.8 Move / rename a document

```
PUT /documents/{document_id}
{ "parent_folder_id": "<target folder id>" }          # move
{ "parent_folder_id": "...", "file_name": "new.pdf" } # move and/or rename
```

#### 7.3.9 Copy a document

```
POST /documents/{document_id}/copy
{ "parent_folder_id": "<target folder id>" }          # copy
{ "parent_folder_id": "...", "file_name": "copy.pdf" }# copy with new name
```

### 7.4 Note templates

Templates are blank-note backgrounds managed separately from documents.

| Method & path | Body / Notes |
|---|---|
| `GET /viewer/configs/note_templates` | → `{ "template_list": [ { "template_name": "...", "note_template_id": "..." }, ... ] }` |
| `POST /viewer/configs/note_templates` | `{ "templateName": "<name>", "document_source": "" }` → `{ "note_template_id": "<id>" }`. Note the **camelCase** key `templateName`, unlike the snake_case used elsewhere. |
| `PUT /viewer/configs/note_templates/{note_template_id}/file` | multipart file upload (section 8) with the template PDF |
| `DELETE /viewer/configs/note_templates/{note_template_id}` | delete a template |

### 7.5 Viewer control

Open a document on the device's screen:

```
PUT /viewer/controls/open2
{ "document_id": "<entry_id>", "page": 2 }
```

`page` is 1-based; page 1 is the front page.

### 7.6 Wi-Fi

**SSIDs are Base64-encoded** (UTF-8 bytes of the network name) in access-point
listing/registration payloads.

| Method & path | Description |
|---|---|
| `GET /system/configs/wifi_accesspoints` | Stored access points: `{ "aplist": [ { "ssid": "<base64>", "security": "...", ... }, ... ] }` |
| `POST /system/controls/wifi_accesspoints/scan` | Performs a scan and returns visible networks in the same `aplist` format |
| `PUT /system/controls/wifi_accesspoints/register` | Add/configure a network, body below |
| `DELETE /system/configs/wifi_accesspoints/{ssid}/{security}` | Remove a stored network. `ssid` is the plain (URL-encoded) network name; `security` as below |
| `GET /system/configs/wifi` | → `{ "value": "on" | "off" }` |
| `PUT /system/configs/wifi` | `{ "value": "on" }` or `{ "value": "off" }` — switch the Wi-Fi radio |

Body of `PUT /system/controls/wifi_accesspoints/register` (all values are
strings):

```json
{
  "ssid":           "<base64 of UTF-8 SSID>",
  "security":       "psk",
  "passwd":         "<passphrase, empty if open>",
  "dhcp":           "true",
  "static_address": "",
  "gateway":        "",
  "network_mask":   "",
  "dns1":           "",
  "dns2":           "",
  "proxy":          "false"
}
```

- `security`: `"psk"` (WPA/WPA2 personal) or `"nonsec"` (open network); other
  values may exist for enterprise modes.
- `dhcp`: `"true"` for automatic addressing. With `"false"`, fill
  `static_address`, `gateway`, `network_mask` (prefix length as string, e.g.
  `"24"`), `dns1`, `dns2`.
- `proxy`: `"true"`/`"false"`.

### 7.7 System configuration

Configuration values live under `/system/configs/`.

```
GET /system/configs/
```

returns all settings as a JSON object mapping each key to a setting object.
Each individual setting is written with:

```
PUT /system/configs/{key}
{ "value": <new value> }
```

and read with `GET /system/configs/{key}` → `{ "value": ... }`.

Known keys:

| Key | Value |
|---|---|
| `datetime` | UTC time as `YYYY-MM-DDTHH:MM:SSZ` (write to set the device clock) |
| `timezone` | Time zone identifier |
| `date_format` | Display date format |
| `time_format` | Display time format (12/24 h) |
| `timeout_to_standby` | Minutes until standby |
| `owner` | Owner name string |
| `wifi` | `"on"` / `"off"` (see 7.6) |

The full authoritative key list for a given firmware is whatever
`GET /system/configs/` returns; a generic client can round-trip that object
by PUT-ting each key back.

### 7.8 System status

Read-only values under `/system/status/`:

| Method & path | Response |
|---|---|
| `GET /system/status/storage` | JSON with storage figures (total/available capacity) |
| `GET /system/status/battery` | JSON with battery state (level, charging status, …) |
| `GET /system/status/firmware_version` | `{ "value": "<version>" }` |
| `GET /system/status/mac_address` | `{ "value": "<mac>" }` |
| `GET /register/information` | Device info incl. `serial_number` (also unauthenticated on port 8080) |

### 7.9 Screenshots

| Method & path | Response |
|---|---|
| `GET /system/controls/screen_shot` | Current screen as **PNG** bytes |
| `GET /system/controls/screen_shot2?query=jpeg` | Current screen as **JPEG** bytes |

### 7.10 Firmware update

Three-step process:

1. **Upload the firmware package** (multipart, section 8; the official
   client uses the filename `FwUpdater.pkg`):

   ```
   PUT /system/controls/update_firmware/file
   ```

2. **Precheck:**

   ```
   GET /system/controls/update_firmware/precheck
   → { "battery": "ok", "image_file": "ok", ... }
   ```

   Proceed only if both `battery` and `image_file` are `"ok"`.

3. **Trigger the update** (device reboots into the updater):

   ```
   PUT /system/controls/update_firmware
   ```

---

## 8. File uploads (multipart format)

All file-content uploads (`PUT .../file` endpoints) use
`multipart/form-data` with a **single form part named `file`**:

```
PUT /documents/{id}/file HTTP/1.1
Cookie: Credentials=...
Content-Type: multipart/form-data; boundary=----XYZ

------XYZ
Content-Disposition: form-data; name="file"; filename="{form-encoded filename}"
Content-Type: application/octet-stream

<raw PDF bytes>
------XYZ--
```

The `filename` in the part header is form-encoded like paths (6.3; spaces as
`+`, non-ASCII percent-encoded). The part's `Content-Type` is not meaningful
to the device.

---

## 9. Error handling

- Failed requests return a non-2xx HTTP status and (for API errors) a JSON
  body with a human-readable `"message"` field, e.g. resolving a nonexistent
  path yields an error status with a message describing the failure.
- During registration, malformed cryptographic parameters produce
  `403 Forbidden` with "Bad parameters for registration process". The most
  common cause is re-encoding `yb` instead of using the device's exact bytes
  (see 4.4). Registration can also fail transiently; retrying after a few
  seconds (after `PUT /register/cleanup`) often succeeds.
- Requests without a valid `Credentials` cookie are rejected; re-authenticate
  (section 5) and retry.

---

## 10. Implementation checklist and pitfalls

1. **TLS:** the API server's certificate is self-signed by the device CA —
   disable verification or pin the certificate from M5.
2. **Never re-encode `yb`:** HMAC the device's DH public key exactly as
   received (may be 256 or 257 bytes).
3. **Encode `ya` as 257 bytes:** `0x00` + 256-byte big-endian value.
4. **IV position:** the AES key-wrap puts the IV *after* the ciphertext.
5. **Cookie parsing:** the device's `Set-Cookie` header may not parse with
   strict cookie jars — extract `Credentials=<value>` manually.
6. **Sign the nonce string**, not its Base64-decoded bytes, with
   RSA-SHA256 (PKCS#1 v1.5).
7. **All JSON scalars are strings** (`"true"`, `"1234"`, …).
8. **1300-entry limit** on `GET /documents2` — compare `count` against the
   returned list length and fall back to per-folder traversal if truncated.
9. **Paths in URLs** use form encoding (spaces as `+`), applied to the whole
   path including its `/` separators (only the resolve endpoint takes paths).
10. **IPv6 zone identifiers:** with link-local addresses
    (`fe80::…%usb0`), ensure your HTTP stack does not percent-encode the `%`
    of the zone id in a way the transport layer cannot resolve.
11. **Root folder is `Document`** — it always exists and cannot be deleted;
    build all paths beneath it.
12. **Set the clock**: writing `/system/configs/datetime` before comparing
    `modified_date` values avoids drift issues when synchronizing.

---

## Appendix A: Cryptographic primitives

| Primitive | Parameters |
|---|---|
| Diffie-Hellman | RFC 3526 MODP group 14: 2048-bit prime, generator 2; client private key 256-bit random |
| KDF | PBKDF2-HMAC-SHA256, 10 000 iterations, 48-byte output |
| MAC | HMAC-SHA256 (full 32-byte digests, except the 8-byte KWA truncation) |
| Symmetric cipher | AES-128-CBC with PKCS#7 padding (key wrapping only) |
| Client identity | RSA-2048, e = 65537; signatures RSASSA-PKCS1-v1_5 with SHA-256 |
| Encoding | Base64 (standard alphabet with padding) for all binary values in JSON |

RFC 3526 group 14 prime (hex):

```
FFFFFFFF FFFFFFFF C90FDAA2 2168C234 C4C6628B 80DC1CD1 29024E08
8A67CC74 020BBEA6 3B139B22 514A0879 8E3404DD EF9519B3 CD3A431B
302B0A6D F25F1437 4FE1356D 6D51C245 E485B576 625E7EC6 F44C42E9
A637ED6B 0BFF5CB6 F406B7ED EE386BFB 5A899FA5 AE9F2411 7C4B1FE6
49286651 ECE45B3D C2007CB8 A163BF05 98DA4836 1C55D39A 69163FA8
FD24CF5F 83655D23 DCA3AD96 1C62F356 208552BB 9ED52907 7096966D
670C354E 4ABC9804 F1746C08 CA18217C 32905E46 2E36CE3B E39E772C
180E8603 9B2783A2 EC07A28F B5C55DF0 6F4C52C9 DE2BCBF6 95581718
3995497C EA956AE5 15D22618 98FA0510 15728E5A 8AACAA68 FFFFFFFF
FFFFFFFF
```

## Appendix B: Client-side sync (informative)

The `sync` feature of this repository is **not part of the device protocol**;
it is implemented entirely client-side on top of the endpoints above. For
reference, it works as follows:

- A *checkpoint* file (`.sync` in the local sync folder) stores the remote
  entry list (paths, types, `modified_date`) as of the previous sync.
- On each run the client gathers three views: checkpoint, current remote tree
  (7.3.2/7.3.3), and the local file tree, normalizing paths to NFC Unicode
  form with `/` separators.
- Per file it compares `modified_date` (remote) and local mtime against the
  checkpoint to classify: upload, download, delete-local, delete-remote, or
  conflict (both changed; the newer side wins).
- Folders present in the checkpoint but missing on one side are deleted on
  the other; new folders are created as needed.
- After applying changes, the fresh remote entry list is written back as the
  new checkpoint.
