<p align="center">
  <img src="assets/logo.svg" width="96" alt="Rune logo" />
</p>

<h1 align="center">Rune</h1>

<p align="center">
  <b>A deterministic password generator for Linux.</b><br />
  One master password becomes a different password for every site. No password is ever stored.
</p>

![Setup, the site list with a password, the site editor and settings](docs/images/screens.png)

<sub>Rendered from the app's own screens by its screenshot test.</sub>

The Android app is in [`rune-android`](https://github.com/BassamAlim/rune-android) and produces
the same passwords. `SPEC.md` is the contract for both apps; `vectors.json` is the shared test
data.

## Features

- **Nothing to steal.** Passwords are worked out from your master password and the site's
  name. They are never written to disk, and the master key isn't kept on this computer at all.
- **Approved on your phone.** Showing a password asks Rune on your phone, which shows the same
  six-digit code and asks for your fingerprint. Without the phone, type the master password
  instead; it's used once and forgotten.
- **Sites.** A searchable list of site names and logins. Two logins on one site get two
  different passwords.
- **Per-site settings.** Length, which kinds of character, and a counter you raise to get a
  new password for that site.
- **Careful copy.** A copied password is kept out of clipboard history and cleared after 30
  seconds if it is still on the clipboard.
- **Same passwords on your phone.** Set both apps up with the same master password; the
  four-character fingerprint shows they match.
- **Export and import.** The site list is one JSON file, shared with the Android app.
- **Move in from Bitwarden.** Import its unencrypted `.json` export and every login keeps its password, sealed by your phone, until you change it to Rune's.

## Layout

```
crates/rune-core/      the rune/v1 algorithm and the site list (no UI, no keyring)
crates/rune-desktop/   the egui app: main.rs (screens), theme.rs (look), store.rs (keyring, clipboard,
                       sync and approvals from the phone)
assets/                logo, in-app mark, fonts, launcher entry
scripts/mark/          the mark and the generator for every logo asset, Android included
scripts/install.sh     build a release binary and install it for the current user
```

## Commands

```bash
cargo run                       # run the app
cargo test --workspace          # all tests
scripts/install.sh              # install binary, launcher entry and icon into ~/.local
```

Three tests are ignored by default because they touch the real machine:

```bash
cargo test -p rune-desktop -- --ignored screenshots   # render every screen to target/screenshots/
cargo test -p rune-desktop -- --ignored keyring       # write and delete a test entry in the keyring
cargo test -p rune-desktop -- --ignored clipboard     # overwrites the clipboard
```

## Where things are kept

- Keys: the Secret Service keyring (KWallet), service `rune`, account `keys`: the sync key and
  fingerprint only, never the master key. Revealed passwords are forgotten after 5 idle minutes.
- Site list: `~/.local/share/rune/sites.json`. It holds names and settings, never passwords.

## Changing the algorithm

Don't, without bumping `rune/v1` to `rune/v2` in the derivation: every existing password would
change. If you do, regenerate the vectors and copy them to the Android repo:

```bash
cargo run -p rune-core --example gen-vectors > vectors.json
```
