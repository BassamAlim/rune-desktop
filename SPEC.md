# Rune — spec v1

Deterministic password generator. Same inputs, same password; Rune's passwords are never stored.
The one exception is temporary: a site's old password, sealed (§3.1), until it is changed to Rune's.

Two private GitHub repos, personal use only:

- `rune-android` — Kotlin/Compose app, sideloaded APK, minSdk 30 (Android 11)
- `rune-desktop` — Rust app for Linux

Both implement the algorithm below independently and must pass the same `vectors.json`.

## 1. Secrets

| Secret | What | Where it lives |
|---|---|---|
| Master password | Memorised by the user | Nowhere. Discarded after key derivation. |
| Master key | Derived from the master password | On each device, encrypted (see §4) |

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
    salt     = "rune/v1/master",   # fixed: Rune has one user, so there is no crowd to precompute against
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
- Set and opened only after an unlock: on Android the site screen holds `sealKey` (never the master key) while the password is shown and zeroes it on leaving.
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
- Master key stored in the Secret Service keyring (KWallet on KDE) via the `keyring` crate. Setup is the same as Android's (generated or own). Anyone at the unlocked desktop session can open Rune — the keyring is the lock.
- Copy flags the password so clipboard managers (Klipper) keep it out of history, and clears it after 30 s if it is still what is on the clipboard.
- Site list at `$XDG_DATA_HOME/rune/sites.json`. Export and import through the system file dialog.
- Import also takes a Bitwarden export (unencrypted `.json`), for moving in. Each login becomes a site named by its first web address (or its name, for an app or an entry with no address), with its username, any `androidapp://` addresses as linked apps, and its password kept sealed (§3.1) until switched to Rune's. An account already in Rune is left alone. Left behind on purpose: notes (the site list isn't encrypted), TOTP seeds, cards and identities. Desktop only, where the master key is at hand; sync (§3.3) carries the result to the phone.
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
- The clipboard is readable by other apps while the password is on it; prefer autofill.
