# Sync (planned paid feature): storage format and API contract

xtermium keeps everything encrypted on disk (`src/vault.rs`, `src/config.rs`). Sync uploads those same encrypted files. The server stores and returns opaque bytes and can't read sessions, hosts, users or tags. This document fixes the format the client writes, and the API a sync server has to offer. The client is `src/sync.rs` (Settings > Sync); there's no server code here.

The client belongs in this repository, under its Apache-2.0 license. The server is proprietary, lives elsewhere, and is not covered by this repository's license.

## What a device has

All files are in the data folder (`~/.xtermium`, or `XTERMIUM_HOME`).

| File | Synced | Contents |
|---|---|---|
| `sessions.xtm` | yes | sessions, folders (with tags), logins, tunnels, snippets, saved layouts and workspaces, sealed under the data key |
| `known.xtm` | yes | trusted SSH host keys and RDP server keys, sealed under the data key |
| `recovery.key` | yes | the data key, sealed under a key derived from the recovery passphrase |
| `settings.xtm` | no | this device's look and defaults (including the default SSH password) |
| `sync.xtm` | no | the server revision this device last matched, and a digest of what was in sync then |

The **data key** is 32 random bytes. Each device keeps it in its OS keychain under service `xtermium` and account `data-key-<first 8 bytes of SHA-256 of the config dir path, hex>`.

## Formats

All integers are little-endian.

**Sealed file** (`*.xtm`): `XTM1` | nonce (24 bytes) | XChaCha20-Poly1305 ciphertext and tag. The associated data is the file's name without extension (`sessions`, `known`, `settings`), so a file can't be swapped for another. The plaintext is UTF-8 TOML. A fresh random nonce is used for every write.

**Recovery key** (`recovery.key`), TOML:

```toml
salt = "<16 bytes, hex>"
m_cost = 65536      # Argon2id memory, KiB
t_cost = 3          # Argon2id passes (parallelism 1, output 32 bytes)
nonce = "<24 bytes, hex>"
sealed_key = "<XChaCha20-Poly1305(data key), associated data 'recovery', hex>"
```

**Folder export** (`*.xtermium`, not synced): `XTMX` | salt (16) | m_cost (u32) | t_cost (u32) | nonce (24) | ciphertext and tag. The key comes from Argon2id over the export passphrase, the associated data is `XTMX`, and the plaintext is a TOML `Store` holding only that folder's sessions and folders.

Readers must refuse Argon2 parameters outside m_cost 8 MiB–1 GiB and t_cost 1–10, so a hostile file can't exhaust memory.

## Why this is post-quantum safe

Nothing uses public-key cryptography, which is what quantum computers break (Shor's algorithm against RSA and elliptic curves). XChaCha20-Poly1305 with 256-bit keys and Argon2id keep about 128-bit security even under Grover's algorithm. A new device gets the data key from `recovery.key` and the passphrase, so devices never need a key exchange, and the server never holds anything a quantum attacker could unwrap.

Transport (HTTPS) protects metadata only. The server should still offer hybrid post-quantum TLS (X25519MLKEM768) where the TLS stack supports it.

## API

Base URL `https://account.xtermium.com`, fixed in the app (`XTERMIUM_SYNC_URL` overrides it for development); JSON unless noted. Refusals carry `{"error": "<message for the person>"}`, which the client shows as it is. The server is proprietary and lives in its own repository; this is the contract both sides read.

### Account

There is no password. A person signs in with an account they already have at Google, GitHub or Discord, in their browser; the app never sees those credentials, and the server keeps no password. The desktop app uses the OAuth device flow (RFC 8628): it asks the server for a code, opens the system browser on the server's page, and polls until the person approves there. No local listener and no deep link. The recovery passphrase stays separate and never leaves the device. Sync is on exactly while signed in; turning it off signs out.

| Method and path | Body | Response |
|---|---|---|
| `POST /auth/device/code` | `{"client_id": "xtermium-desktop"}` | `200 {"device_code": "...", "user_code": "ABCD2345", "verification_uri": "https://account.xtermium.com/device", "verification_uri_complete": "https://account.xtermium.com/device?user_code=ABCD2345", "expires_in": 600, "interval": 5}` |
| `POST /auth/device/token` | `{"grant_type": "urn:ietf:params:oauth:grant-type:device_code", "device_code": "...", "client_id": "xtermium-desktop"}` | `200 {"access_token": "...", "token_type": "Bearer"}` once the person approved; until then `400 {"error": "authorization_pending"}`. Also `400` with `"slow_down"` (poll 5 s slower), `"expired_token"` (ask for a new code), `"access_denied"` (the person refused), `"invalid_grant"` (unknown or already used code) |
| `GET /v1/account` | | `200 {"email": "...", "name": "...", "image": "<url or null>", "pro": {"active": true, "plan": "monthly" or "yearly" or null, "until": <unix seconds or null>, "status": "<Creem's status or null>"}}` |
| `POST /v1/account/portal` | | `200 {"url": "..."}`: Creem's customer portal (cancel, change the card, invoices); `404` when the account has no subscription yet |
| `POST /auth/revoke-sessions` | | `200`; every session of the account ends ("sign out everywhere") |
| `POST /v1/account/logout` | | `204`; the token stops working. The client forgets it whatever the answer |

- The app shows the code large and opens `verification_uri_complete`; it polls every `interval` seconds and adds 5 s on `slow_down`. A code is good for 10 minutes and single use: a second `token` call after a success gets `invalid_grant`.
- `access_token` is the bearer token for every other call. The app keeps it in `settings.xtm`, which is sealed with the data key like every other file, and leaves it out of backups, exports and troubleshooting bundles.
- Sessions last 90 days, extended by a day whenever used; `401` (or `403`) on any call means signing in again, when it is the server's own JSON `{"error": …}`; the same status as an HTML page is Cloudflare's bot block or challenge, and the app treats it like `503` (try later) rather than signing out.
- Adding a second sign-in provider, or removing one, happens on `https://account.xtermium.com/account` in the browser. Pro is bought at `https://account.xtermium.com/checkout/monthly` or `/checkout/yearly` (the page signs the person in first, then opens Creem's checkout).

### Vault

`Authorization: Bearer <token>` on every call. Uploading needs Pro: `PUT` answers `402 {"error": "..."}` without it, and the client shows the message as it is with a link to get Pro. `GET` and `DELETE` work without Pro, so an account whose Pro ended can still get its data onto a new device and can still delete its cloud copy.

| Method and path | Body | Response |
|---|---|---|
| `GET /v1/vault` | | `200 {"revision": n, "files": {"sessions": "<base64>", "known": "<base64>", "meta": "<base64>"}, "recovery": "<recovery.key text>", "updated": {"at": <unix seconds>, "ip": "<address>", "location": "<city, country>"}, "quota": {"used": <bytes>, "total": <bytes>}}`, or `404` when the account has no vault yet |
| `PUT /v1/vault` | `{"base": n, "files": {...}, "recovery": "..."}` | `200 {"revision": n+1, "quota": {...}}`, `402` without Pro, `409 {"revision": m}` when `base` isn't the current revision, or `413 {"error": "..."}` when a file is over its cap or the upload would go past the account's storage |
| `DELETE /v1/vault` | | `204`; removes everything stored for the account |

- **Revisions:** an integer that grows by one with every accepted `PUT`. The server stores whole files and never merges. Every accepted `PUT` is a new, immutable revision on the server; the last 10 are kept so the operator can roll a bad sync back.
- **`meta`:** a sealed file (associated data `meta`, TOML `device`, `os`, `at`) naming the device that uploaded the revision. Stored and returned like the other files; older uploads may lack it.
- **`updated`:** what the server saw of the `PUT` that made the current revision: when, from which IP address, and the city and country it maps to. All fields optional.
- **`quota`:** the account's storage, bytes held and bytes allowed. `used` counts the current revision's raw files; `total` is 10 MiB. The client shows it as a bar in the account menu and in Settings › Account and sync, warns from 95% that past 100% nothing more syncs, and at 100% that nothing does. Pictures used as session and folder icons are what takes the room: they travel inside `sessions`.
- **Caps:** `sessions` up to 10 MiB, `known` 1 MiB, `meta` 16 KiB, `recovery` 4 KiB; a `PUT` body up to 14 MiB. Over any of them, or over the total: `413`, and nothing is stored.
- **Retried `PUT`:** the same request sent again after a timeout answers `200` with the same revision; no second revision is made.
- **Conflicts:** when this device and the server both changed since the last sync, nothing is merged. The client shows both sides (last change, device, OS, and for the server's copy the IP address and location from `updated`) and the person keeps one: "this device's" puts with the current revision as `base`, "the cloud's" replaces the local files. A `409` during that put means a third change arrived: the client looks again.
- **First device:** if `GET` returns 404, upload the local files and `recovery.key`. Sync can't be turned on until a recovery passphrase is set.
- **New device:** after `GET`, ask for the recovery passphrase, unwrap the data key, store it in the keychain, and write the files.
- **When the client syncs:** at start and on "Sync now". When a sync fails with `503`, with Cloudflare's own `502`, `504` or `520`–`530` (the server or its tunnel is down), or with a network error, it retries on its own with exponential backoff and jitter (30 s, doubling, capped at 30 minutes); after a day of failing it stops and shows "Syncing issue, try later…". "Sync now" or a success resets that. The app keeps working with its local data throughout. If only one side changed since the last sync, its copy replaces the other's whole, so deletions carry over; if both did, the person chooses (above).
- **Joining with data of its own:** the person chooses between the device's data and the account's, the same way; either way every local file, settings included, is then encrypted again under the account's data key.
- **`429`:** too many requests; `Retry-After` says how many seconds to wait. `GET /v1/vault` is limited to 120 an hour per account: `recovery.key` rides along in every answer and allows offline passphrase guessing, so the client also requires passphrases of at least 12 characters.
- **`503`:** the server is away (`Retry-After: 30`; minutes, hours at worst). Never a bare 500.
