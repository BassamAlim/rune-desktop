# Rune — spec v1

Deterministic password generator. Same inputs, same password; Rune's passwords are never stored.
The one exception is temporary: a site's old password, sealed (§3.1), until it is changed to Rune's.

Two GitHub repos:

- [`rune-android`](https://github.com/BassamAlim/rune-android) — Kotlin/Compose app, sideloaded APK, minSdk 30 (Android 11)
- `rune-desktop` — Rust app for Linux

Both implement the algorithm below independently and must pass the same `vectors.json`.

## 1. Secrets

| Secret | What | Where it lives |
|---|---|---|
| Master password | Memorised by the user | Nowhere. Discarded after key derivation. |
| Master key | Derived from the master password | On the phone, encrypted (§4.1). Never kept on the desktop, which asks the phone (§3.4). |

The master password is the only secret, so a new device needs only it. It is also the only
defence once a site leaks one of its passwords: the attacker can then guess master passwords
offline at Argon2id cost (§8). Setup therefore offers a generated one by default:

- **Generated:** 6 words picked uniformly (OS randomness, rejection sampling) from the EFF large
  wordlist (7776 words, `rune-core/words.txt`, copied to Android as `res/raw/words.txt`), joined
  by single spaces: ≈ 77.5 bits. The user types it back once; case and spacing are ignored for
  that check only — the derivation uses the generated text.
- **Own:** typed twice, with a zxcvbn estimate shown as the time to guess it at ~10⁵ guesses/s.
  Weak passwords are warned about, never refused.

Fingerprint unlock means the master password is rarely typed, and forgetting it loses everything.
So on Android, once it hasn't been typed for 14 days, the next unlock asks for it instead of the
fingerprint ("practice"). It can be skipped for that unlock; the next one asks again.

## 2. Algorithm (`rune/v1`)

All strings are UTF-8 after NFKC normalisation. The master password is then also cleaned:
split on runs of ASCII whitespace (U+0009–000D and U+0020), empty pieces dropped, joined with
single spaces. Case is kept. `u32be(x)` is a 4-byte big-endian integer.

### 2.1 Master key (once, at setup)

```
masterKey = Argon2id(
    password = masterPassword,
    salt     = "rune/v1/master",   # fixed, with no per-user part: a new device needs only the master password (§8)
    m = 65536 KiB (64 MiB), t = 3, p = 4, version = 0x13,
    length   = 32)
```

Parameters are fixed by the `rune/v1` tag. Changing them means `rune/v2`.

### 2.2 Site key

```
site  = lowercase(trim(siteName))
login = lowercase(trim(loginName))      # may be empty

siteKey = HMAC-SHA256(masterKey,
    "rune/v1" ‖ u32be(len(site)) ‖ site ‖ u32be(len(login)) ‖ login ‖ u32be(counter))
```

`counter` starts at 1. Bump it to rotate one password.

### 2.3 Password

Policy per site: `length` (4–64, default 16), enabled character classes (default all four), and
the symbols the site allows (default `!@#$%^&*-_=+?`).

```
lower   = abcdefghijklmnopqrstuvwxyz
upper   = ABCDEFGHIJKLMNOPQRSTUVWXYZ
digits  = 0123456789
allSymbols = !@#$%^&*-_=+?"'(),./:;<>[\]`{|}~     # the defaults, then the rest of ASCII punctuation
symbols = the characters of allSymbols that the site allows, in allSymbols order
          (any order, repeats ignored; anything else is an error; empty is an error when symbols are on)
charset = enabled classes concatenated in the order above; n = len(charset)

stream  = HMAC-SHA256(siteKey, u32be(0)) ‖ HMAC-SHA256(siteKey, u32be(1)) ‖ …

nextChar():  take next byte b from stream;
             if b >= 256 - (256 mod n): discard and repeat;
             return charset[b mod n]

loop:
    candidate = length × nextChar()
    if candidate contains at least one character from every enabled class: return it
    otherwise discard it and continue with the same stream
```

Rejection sampling keeps the output uniform over all valid passwords. Because the default symbols
lead `allSymbols` in their original order, a site with the default set gets the same alphabet,
and the same passwords, as before per-site symbols existed.
Default policy: 75-character alphabet × 16 ≈ 100 bits, beyond any guessing even offline, and
within the 16-character cap some sites (often banks) impose. Each site stores its own length, so
the default only applies to sites added after it changes.

### 2.4 Key fingerprint

```
fingerprint = first 2 bytes of HMAC-SHA256(masterKey, "rune/v1/check"), shown as 4 hex digits
```

Stored on the device and shown at setup. Used to reject a mistyped master password on
re-entry and to confirm that phone and desktop derived the same key.

## 3. Site list

Not secret, but required: a lost counter or policy means a lost password.

```json
{
  "version": 1,
  "sites": [
    { "site": "github.com", "login": "bassam", "counter": 1,
      "length": 16, "classes": "luds", "symbols": "!@#$%^&*-_=+?",
      "apps": ["com.github.android"], "note": "username: bassam_m", "modified": 1760300000 }
  ],
  "deleted": [ { "site": "old.example", "login": "", "modified": 1760300100 } ]
}
```

- `symbols` is stored in canonical order and may be absent (files from before it existed): absent means the default.
- `note` is free text kept with the login (e.g. a username when the login is an email), stored as typed. Not secret, not part of the password; absent means empty.
- `sealed` is a site's own password kept until it is changed to Rune's (§3.1); absent or empty for most sites.

### 3.1 Kept passwords

For moving in from another password manager: a site keeps its existing password until the user
changes it on the site to Rune's, then switches, which deletes it.

```
sealKey = HMAC-SHA256(masterKey, "rune/v1/seal")
sealed  = hex( nonce ‖ AES-256-GCM(sealKey, nonce, UTF-8 password, aad = "rune/v1/password") )
```

- `nonce` is 12 random bytes, fresh for every seal; the GCM tag is 16 bytes.
- Not bound to the site or login, so renaming an entry keeps it readable. Editing a site carries it through untouched.
- While kept, it is the site's password everywhere (shown, copied, autofilled); Rune's is offered beside it for the site's change-password form.
- Set and opened only after an unlock: on Android the site screen holds `sealKey` (never the master key) while the password is shown and zeroes it on leaving. The desktop asks the phone to do both (§3.4).
- Both apps assert one vector (`seal_vector` in rune-core, `SealTest` on Android): master key `[7; 32]`, nonce `01..0c`, password `Hunter2 – ünïcode`.
- One JSON file in app-private storage; the same format is the export/import file.
- `modified` is when the entry last changed, in seconds since 1970; absent (older files) means 0. `deleted` remembers deletes; absent means none.
- Export and import are manual, through the system file picker. Import merges (§3.2).
- The same file moves the list between phone and desktop, and so does sync (§3.3).

### 3.2 Merging two lists

One rule for import and for sync, per `(site, login)` after normalising both:

- The entry with the larger `modified` wins whole. At a tie the higher counter wins; at a full tie the list being merged into keeps its own.
- The counter of the result is the higher of the two, whichever entry won: a counter never goes down, because a lost higher counter is a lost password.
- A delete removes an entry whose `modified` is older than it. An entry changed in the same second or later stays, and the delete is dropped.
- Every save stamps `modified = max(now, what it replaces + 1)`, counting a remembered delete of the same name, so a change always beats what it changed even on a device whose clock runs behind. A delete is stamped the same way. Renaming an entry is a delete of the old name and a save of the new.

Merging A into B and B into A gives the same list, and merging again changes nothing, so two devices settle after one exchange.

### 3.3 Sync over the local network

The phone and the desktop keep their lists the same when both are on one network. There is no server and no pairing: both already hold the master key.

```
syncKey = HMAC-SHA256(masterKey, "rune/v1/sync")
message = nonce ‖ AES-256-GCM(syncKey, nonce, UTF-8 JSON site list, aad = "rune/v1/sync")
```

1. **Find.** The phone sends the UDP datagram `rune-sync/1 <fingerprint>` (§2.4) to the broadcast address of each network it is on, port 47825. A desktop holding a key with that fingerprint sends the same bytes back; any other stays silent.
2. **Exchange.** The phone connects to that address on TCP 47825 and sends its list as one `message`, prefixed by its length as a 4-byte big-endian integer (at most 4 MiB). The desktop opens it, merges it into its own list (§3.2), saves, and answers with the merged list in the same framing. The phone merges the answer into its own.

- Only a device with the same master password can produce or open a message. One with another password gets no answer and changes nothing.
- A replayed or reflected message is harmless: merging an old list, or one's own, changes nothing (§3.2).
- The phone syncs when the app comes to the front, after every change, and on request. The desktop only answers, for as long as it is running, including minimised: the merge is done on the file by the listener, not by the window.
- The phone keeps `syncKey` in app-private storage so sync needs no unlock. It is one-way from the master key and opens no password (kept ones are sealed under a different key, §3.1); it protects only the list in transit, which the phone already stores in the clear.
- On Android 17+ the phone needs the `ACCESS_LOCAL_NETWORK` permission, asked for on "Sync now".
- Both apps assert one message (`sync_messages_open_only_with_the_same_master` in rune-core, `SyncTest` on Android).

### 3.4 Approval from the phone

The desktop never keeps the master key. Whatever needs it (a site's password, the previous one,
a kept one, sealing a password to keep) is asked of the phone, which shows what is asked and
answers after its own unlock (§4.1). It rides on the sync connection (§3.3); `rune/v1` is
unchanged, so no password changes.

```
commit    = SHA-256("rune/v1/commit" ‖ desktopPub)
code      = first 4 bytes of SHA-256("rune/v1/code" ‖ desktopPub ‖ phonePub), big-endian, mod 10⁶, as 6 digits
answerKey = HKDF-SHA256(ikm = X25519(own private, other public), salt = desktopPub ‖ phonePub, info = "rune/v1/answer")
frame     = 4-byte big-endian length ‖ nonce ‖ AES-256-GCM(syncKey, nonce, UTF-8 JSON, aad)
box       = hex( nonce ‖ AES-256-GCM(answerKey, nonce, UTF-8 JSON, aad = "rune/v1/box") )
```

`desktopPub` and `phonePub` are fresh X25519 public keys (RFC 7748, 32 bytes, hex in JSON, like `commit`),
made for one request and dropped with their private keys when it ends. `id` is 16 random bytes,
hex. Every frame has a fresh random 12-byte nonce and is at most 4 MiB. The desktop's frames are
sealed with `syncKey` and aad `rune/v1/request`, the phone's with `syncKey` and `rune/v1/answer`;
what is secret inside them goes in a `box`.

1. **Ask.** The desktop says "Approve on your phone". A request waits until answered, cancelled,
   or 2 minutes pass. One at a time: a new one replaces it.
2. **Notice.** While a request waits, the desktop answers the probe with `rune-sync/1 <fingerprint> ask`
   instead of the plain probe. While the app is in front, the phone probes every 2 s and syncs when
   the answer ends in ` ask`. A probe answer isn't authenticated: a forged `ask` only causes a sync.
3. **Hand over.** After the merged list (§3.3), on the same connection, the desktop sends the
   request, or `{}` when there is none. A phone that doesn't know this frame just closes the
   connection; the request keeps waiting.
   - `{"id", "kind": "show", "site", "login", "commit"}`, for the site card;
   - `{"id", "kind": "keep" | "import", "count", "commit"}`, for passwords the phone should seal.
4. **Agree a code.** The phone answers `{"id", "pub": phonePub}`; the desktop then sends
   `{"id", "pub": desktopPub}`, and the phone checks it against `commit`. Both show `code`.
   Committing first means neither side can pick its key after seeing the other's, so a
   man-in-the-middle can't make the two codes match except by a 1-in-10⁶ chance.
5. **Approve.** The phone shows the request and the code, and unlocks as for any password (§4.1).
   The user checks the code is the desktop's. Declining answers `{"id", "declined": true}`. The
   connection stays open while the prompt is up, within the 2 minutes.
6. **Answer.**
   - `show`: the phone looks the entry up in its own list, just merged, and answers `{"id", "box"}`
     over `{"password", "previous", "kept"}`: Rune's password, the one for counter − 1 when the
     counter is above 1, and the kept password opened (§3.1) when there is one; the last two are
     absent otherwise. The settings (counter, length, classes, symbols) are the phone's, never the
     request's, so a request reveals only what the phone would show for that entry. An entry the
     phone doesn't have answers `{"id", "missing": true}`.
   - `keep` / `import`: the phone answers `{"id", "ready": true}`. The passwords go the other way,
     so here the desktop is the one that must trust the code: it asks "Does your phone show
     <code>?" and only then sends `{"id", "box"}` over a list of `{"site", "password"}`, each `site` an
     entry in the site-list format (§3). The phone seals each (§3.1), saves, answers
     `{"id", "added", "already"}`, and syncs, which brings the result to the desktop. `keep` is one
     existing entry and sets its kept password; `import` adds the sites not yet in the list and
     leaves those already there alone (`already`).

- What malware on the desktop can get: the answers to requests made while it is there, never the
  master key. A request it makes on its own shows its site on the phone, unasked.
- A stolen `syncKey` on the same network reads which sites are asked for, never the answers.
  Posing as either side, or sitting between them, shows codes that don't match.
- The phone answers only in front, behind a prompt, and holds the master key only while answering.
- **Without the phone**, the desktop offers the master password instead: it derives the master key,
  checks the fingerprint (§2.4), does the same work itself and zeroes the key at once. Setup works
  the same way: the master password is typed once, and only `syncKey` and the fingerprint are kept.
- Both apps assert one approval (`approval_vector` in rune-core, `ApprovalTest` on Android): X25519
  private keys `[1; 32]` (desktop) and `[2; 32]` (phone), giving `commit`, `code` (`437923`) and `answerKey`;
  confirmed by a third implementation (Python `cryptography`).

## 4. Android

Structured like the owner's other apps: `core/` (crypto, vault, data, nav, ui, autofill) and
`features/<feature>/` with `Screen`, `ViewModel`, `UiState`, `Domain`. Hilt, Navigation Compose
with type-safe routes, one `StateFlow<UiState>` per ViewModel.

### 4.1 Key storage

- Master key encrypted with an AES-256-GCM Android Keystore key:
  `setUserAuthenticationRequired(true)`, `setUserAuthenticationParameters(0, AUTH_BIOMETRIC_STRONG)`,
  StrongBox if available, TEE otherwise.
- Every use shows a `BiometricPrompt` bound to the decrypt `Cipher` via `CryptoObject`. The decrypted key is used for one operation and zeroed.
- Fallback when biometrics fail or the key is invalidated by new enrolment: enter the master password → re-derive → check fingerprint. No device-PIN fallback.
- A phone with no fingerprint enrolled still works: the master key is simply not stored, and the master password is asked for each time. Fingerprint unlock is turned on (or back on) from Settings.
- Stored alongside: the key fingerprint, and when the master password was last typed (for practice, §1).

### 4.2 Screens

1. **Setup** — a generated passphrase typed back, or your own master password twice; then the fingerprint to compare with other devices; then the fingerprint sheet that seals the master key.
2. **Sites** — searchable list of names and logins. Holds no secrets, so it has no lock.
3. **Site** — opening it asks for the fingerprint; the password is shown masked with reveal and copy, and forgotten when the screen is left. When the counter is above 1, the previous password (counter − 1) can be copied too, for a site's change-password form; it comes from the same unlock.
4. **Editor** — site, login, counter, length, classes, allowed symbols; delete behind a confirmation.
5. **Settings** — autofill, your key, fingerprint unlock, export/import, reset.
6. **Approval** — a sheet over whatever is in front when the desktop asks (§3.4): what is asked (the site and login, or how many passwords to keep), the code, then the unlock. Approve or decline.

### 4.3 Autofill

- `AutofillService`. The service never holds a password: every suggestion is a locked dataset that opens `AutofillActivity`, which authenticates, derives the password and returns the filled dataset.
- Web: only a known browser (a fixed package list) is believed about which domain it is showing. A page matches a saved site when the domain equals the site or ends with `.` + site, and the site name contains a dot.
- Apps: match the package name against a site's `apps`. The link is made when the user picks a site for that app and authenticates.
- A "choose in Rune" suggestion is always offered. In a browser it also offers to add the page's domain as a new site with default settings.
- Nothing is saved from forms.

### 4.4 Clipboard

- Copy sets `ClipDescription.EXTRA_IS_SENSITIVE` and clears the clipboard after 30 s. Android does not let a background app read the clipboard, so the clear is unconditional.

### 4.5 Hardening

- `INTERNET` only to fetch each site's `https://<site>/favicon.ico` and homepage, for its avatar and a display name (both apps do this; the stored name and the password never change), and to sync with the desktop on the local network (§3.3). The request carries the site name and nothing else; no secret or derived value is ever sent. `FLAG_SECURE` on both activities. `allowBackup="false"` and everything excluded from device transfer.

### 4.6 Stack

Kotlin, Jetpack Compose (Material 3), Hilt, Navigation Compose, DataStore, `androidx.biometric`,
`kotlinx.serialization`, Bouncy Castle (`Argon2BytesGenerator`). HMAC and AES-GCM from the platform.

## 5. Desktop (Rust)

- One Cargo workspace: `rune-core` (library: algorithm + site list, no I/O beyond the JSON file) and `rune-desktop` (GUI binary).
- GUI: `eframe`/`egui`, native Wayland. Screens: setup, sites (list with the selected site's password in a card below, and the previous password to copy when the counter is above 1), site editor, settings.
- Rune opens locked, and locks again after 5 idle minutes (wall-clock, so sleep counts, and checked while minimised) or from the lock button. A finger on the sensor (fprintd, via `fprintd-verify`) or the master password unlocks it. This only hides the list from someone at the screen: a match releases nothing secret, the master password is checked against the fingerprint (§2.4) and dropped, and `sites.json` stays readable on disk. Sync carries on while locked.
- The site card's password comes from the phone (§3.4, `show`): one approval gives the password, the previous one and the kept one, held while the card is open and forgotten on leaving it or on locking. Typing a site's old password to keep it is a `keep` request.
- The master key is never stored. The Secret Service keyring (KWallet on KDE, via the `keyring` crate) holds `syncKey` ‖ key fingerprint: for sync and approvals (§3.3, §3.4), and to reject a mistyped master password in setup and the fallback. Setup is the same as Android's (generated or own). A keyring entry holding the master key itself (an older Rune) is replaced by this on first launch.
- Copy flags the password so clipboard managers (Klipper) keep it out of history, and clears it after 30 s if it is still what is on the clipboard.
- Site list at `$XDG_DATA_HOME/rune/sites.json`. Export and import through the system file dialog.
- Import also takes a Bitwarden export (unencrypted `.json`), for moving in. Each login becomes a site named by its first web address (or its name, for an app or an entry with no address), with its username, any `androidapp://` addresses as linked apps, and its password kept sealed (§3.1) until switched to Rune's. An account already in Rune is left alone. Left behind on purpose: notes (the site list isn't encrypted), TOTP seeds, cards and identities. Read on the desktop, where the export file is, and sealed by the phone as one `import` request (§3.4); sync brings the result back.
- This repo owns `SPEC.md` and `vectors.json`; `cargo run -p rune-core --example gen-vectors` regenerates the vectors. `rune-android` holds a copy of `vectors.json` only.

## 5a. Design

One dark theme shared by both apps: warm near-black surfaces (`#0D0B09`, `#181512`, `#231F1A`),
one amber accent (`#FFB020`), Space Grotesk for the interface and JetBrains Mono for passwords
and keys.

The mark is ᚱ (Raido), the Elder Futhark R, drawn as round-capped strokes. `scripts/mark/assets.py`
holds its geometry and writes every asset from it: the desktop SVGs and raster, and the Android
vector drawables.

## 6. Tests

- `vectors.json`: fixed (master password, site, login, counter, policy) → expected master key, site key, password, fingerprint. Generated by the Rust core, checked into both repos, asserted by both.
- Before the vectors are frozen, one vector is confirmed by a throwaway third implementation (Python, `argon2-cffi` + stdlib `hmac`), so the two apps can't simply agree on the same mistake.
- Must include: empty login, non-ASCII site name, a policy that forces a rejected candidate, counter 2.
- Sealing, sync and approval each have one shared vector, asserted by both apps (§3.1, §3.3, §3.4).

## 7. Not in v1

- Sync beyond the local network (§3.3), multiple profiles, storing passwords for good (a kept one, §3.1, is until it is switched).
- Inline (keyboard) autofill suggestions; the dropdown is used.
- A light theme.
- Credential Manager provider (Android 14+) — add when apps you use need it.
- Public Suffix List domain matching — add if suffix matching proves wrong for a site you use.
- Desktop CLI and browser extension.

## 8. Known limits

- Changing or forgetting the master password changes or loses every password.
- An attacker with one site password can guess the master password offline at Argon2id cost: a generated passphrase is out of reach, a weak own password is not.
- The master key's salt is fixed (§2.1), so the same master password gives the same master key for everyone, and one Argon2id guess can be tried against every user whose site password has leaked. Rune was designed for one user; with many, a weak own master password is more exposed than a per-user salt would leave it. Changing this means `rune/v2`.
- The clipboard is readable by other apps while the password is on it; prefer autofill.
- Copying a password on the desktop needs the phone at hand, on the same network. The master-password fallback works without it, but puts the master key on the desktop for that moment.
- Malware running as the user on the desktop sees every password asked for while it is there (§3.4). Only the phone keeps the master key out of its reach.
