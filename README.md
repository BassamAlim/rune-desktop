# Echo (desktop)

A deterministic password generator for Linux. One master password and a secret key become a
different password for every site; no password is ever stored. The Android app is in
`echo-android` and produces the same passwords.

`SPEC.md` is the contract for both apps. `vectors.json` is the shared test data.

## Layout

```
crates/echo-core/      the echo/v1 algorithm and the site list (no UI, no keyring)
crates/echo-desktop/   the egui app: main.rs (screens), theme.rs (look), store.rs (keyring, clipboard)
assets/                logo, in-app mark, fonts, launcher entry
scripts/mark/          the mark's geometry and the generator for every logo asset, Android included
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
cargo test -p echo-desktop -- --ignored screenshots   # render every screen to target/screenshots/
cargo test -p echo-desktop -- --ignored keyring       # write and delete a test entry in the keyring
cargo test -p echo-desktop -- --ignored clipboard     # overwrites the clipboard
```

## Where things are kept

- Keys: the Secret Service keyring (KWallet), service `echo`, account `keys`.
- Site list: `~/.local/share/echo/sites.json`. It holds names and settings, never passwords.

## Changing the algorithm

Don't, without bumping `echo/v1` to `echo/v2` in the derivation: every existing password would
change. If you do, regenerate the vectors and copy them to the Android repo:

```bash
cargo run -p echo-core --example gen-vectors > vectors.json
```
