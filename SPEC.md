# Rune — spec v1

Deterministic password generator. Same inputs, same password; no passwords are ever stored.

Two private GitHub repos, personal use only:

- `rune-android` — Kotlin/Compose app, sideloaded APK, minSdk 30 (Android 11)
- `rune-desktop` — Rust app for Linux

Both implement the algorithm below independently and must pass the same `vectors.json`.

## 1. Secrets

| Secret | What | Where it lives |
|---|---|---|
| Master password | Memorised by the user | Nowhere. Discarded after key derivation. |
| Secret key | 32 random bytes generated at first setup | On each device; backed up once by the user as Base32 text |
| Master key | Derived from the two above | On each device, encrypted (see §4) |

The secret key is what makes a leaked site password useless for guessing the master password.
A new device needs both the master password and the secret key.

Secret key as text: RFC 4648 Base32, no padding (52 characters), shown in groups of 4.
Input ignores case, spaces and dashes. The desktop also shows the key as a QR code (the same text);
the phone's setup screen scans it on-device, so the key never has to be typed on the phone.

## 2. Algorithm (`rune/v1`)

All strings are UTF-8 after NFKC normalisation. `u32be(x)` is a 4-byte big-endian integer.

### 2.1 Master key (once, at setup)

```
masterKey = Argon2id(
    password = masterPassword,
    salt     = secretKey,          # 32 bytes
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

Policy per site: `length` (4–64, default 20), enabled character classes (default all four), and
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
Default policy: 75-character alphabet × 20 ≈ 124 bits.

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
      "length": 20, "classes": "luds", "symbols": "!@#$%^&*-_=+?",
      "apps": ["com.github.android"] }
  ]
}
```

- `symbols` is stored in canonical order and may be absent (files from before it existed): absent means the default.
- One JSON file in app-private storage; the same format is the export/import file.
- Export and import are manual, through the system file picker. Import merges by `(site, login)`; the higher counter wins.
- The same file moves the list between phone and desktop.

## 4. Android

Structured like the owner's other apps: `core/` (crypto, vault, data, nav, ui, autofill) and
`features/<feature>/` with `Screen`, `ViewModel`, `UiState`, `Domain`. Hilt, Navigation Compose
with type-safe routes, one `StateFlow<UiState>` per ViewModel.

### 4.1 Key storage

- Master key encrypted with an AES-256-GCM Android Keystore key:
  `setUserAuthenticationRequired(true)`, `setUserAuthenticationParameters(0, AUTH_BIOMETRIC_STRONG)`,
  StrongBox if available, TEE otherwise.
- Every use shows a `BiometricPrompt` bound to the decrypt `Cipher` via `CryptoObject`. The decrypted key is used for one operation and zeroed.
- Secret key encrypted with a second Keystore key that needs no user authentication, so the master-password fallback works.
- Fallback when biometrics fail or the key is invalidated by new enrolment: enter the master password → re-derive → check fingerprint. No device-PIN fallback.
- A phone with no fingerprint enrolled still works: the master key is simply not stored, and the master password is asked for each time. Fingerprint unlock is turned on (or back on) from Settings.
- Showing the secret key in Settings is gated by a plain presence check (fingerprint or screen lock), not by a key: the app can already read it.

### 4.2 Screens

1. **Setup** — master password twice, or join an existing key by typing the secret key; then the fingerprint and the secret key to write down; then the fingerprint sheet that seals the master key.
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

- No `INTERNET` permission. `CAMERA` only for scanning the secret key at setup, decoded on the phone by zxing. `FLAG_SECURE` on both activities. `allowBackup="false"` and everything excluded from device transfer.

### 4.6 Stack

Kotlin, Jetpack Compose (Material 3), Hilt, Navigation Compose, DataStore, `androidx.biometric`,
`kotlinx.serialization`, Bouncy Castle (`Argon2BytesGenerator`). HMAC and AES-GCM from the platform.

## 5. Desktop (Rust)

- One Cargo workspace: `rune-core` (library: algorithm + site list, no I/O beyond the JSON file) and `rune-desktop` (GUI binary).
- GUI: `eframe`/`egui`, native Wayland. Screens: setup, sites (list with the selected site's password in a card below, and the previous password to copy when the counter is above 1), site editor, settings.
- Master key and secret key stored in the Secret Service keyring (KWallet on KDE) via the `keyring` crate; secret key entered once as Base32 text from the phone. Anyone at the unlocked desktop session can open Rune — the keyring is the lock.
- Copy flags the password so clipboard managers (Klipper) keep it out of history, and clears it after 30 s if it is still what is on the clipboard.
- Site list at `$XDG_DATA_HOME/rune/sites.json`. Export and import through the system file dialog.
- This repo owns `SPEC.md` and `vectors.json`; `cargo run -p rune-core --example gen-vectors` regenerates the vectors. `rune-android` holds a copy of `vectors.json` only.

## 5a. Design

One dark theme shared by both apps: warm near-black surfaces (`#0D0B09`, `#181512`, `#231F1A`),
one amber accent (`#FFB020`), Space Grotesk for the interface and JetBrains Mono for passwords
and keys.

The mark is ᚱ (Raido), the Elder Futhark R, drawn as round-capped strokes. `scripts/mark/assets.py`
holds its geometry and writes every asset from it: the desktop SVGs and raster, and the Android
vector drawables.

## 6. Tests

- `vectors.json`: fixed (master password, secret key, site, login, counter, policy) → expected master key, site key, password, fingerprint. Generated by the Rust core, checked into both repos, asserted by both.
- Before the vectors are frozen, one vector is confirmed by a throwaway third implementation (Python, `argon2-cffi` + stdlib `hmac`), so the two apps can't simply agree on the same mistake.
- Must include: empty login, non-ASCII site name, a policy that forces a rejected candidate, counter 2.

## 7. Not in v1

- Sync of any kind, multiple profiles, storing passwords Rune didn't generate.
- Inline (keyboard) autofill suggestions; the dropdown is used.
- A light theme.
- Credential Manager provider (Android 14+) — add when apps you use need it.
- Public Suffix List domain matching — add if suffix matching proves wrong for a site you use.
- Desktop CLI and browser extension.

## 8. Known limits

- Changing the master password or losing the secret key changes every password.
- An attacker with the secret key **and** one site password can guess the master password offline at Argon2id cost.
- The clipboard is readable by other apps while the password is on it; prefer autofill.
