# AGENTS.md

Rune for Linux: a deterministic password generator. One master password becomes a
different password for every site; passwords are derived, never stored. `README.md`
has the layout, commands and where data lives.

This repo is the **contract owner** for both apps. `SPEC.md` (algorithm, site-list format,
security model) and `vectors.json` live here; the sibling `rune-android` repo holds a copy of
the vectors and must produce identical passwords. A change to either file is a change to both
apps: copy `vectors.json` to `rune-android/app/src/test/resources/` in the same piece of work.
The passphrase wordlist `crates/rune-core/words.txt` is shared the same way, as
`rune-android/app/src/main/res/raw/words.txt`.

## Rules that matter

- **`rune-core` is frozen by `vectors.json`.** Any output change changes every password the
  user has. The vectors test passes unchanged; a real algorithm change means `rune/v2`, new
  vectors from `cargo run -p rune-core --example gen-vectors`, and the same change in Android.
- **Secrets are zeroized after use.** The master password, confirmation and derived passwords
  are wiped with `zeroize` once consumed; a new field holding a secret gets the same treatment.
- **An unreadable site list stops the app** (`Screen::Fatal`) rather than starting empty:
  counters can't be recomputed, so overwriting the file loses passwords.
- **Machine access goes through `store.rs`**: keyring, clipboard, paths, randomness. `main.rs`
  stays screens only, which keeps the screens testable with `egui_kittest`.

## Design

`theme.rs` is the only source of colour, radius and spacing, and its helpers
(`primary_button`, `card`, `field`, …) are how screens are built. The palette is shared with
Android: warm near-black surfaces, one amber accent, one filled accent button per screen,
destructive actions outlined in `DANGER`. Icons are Phosphor via `theme::icon`.

The mark is ᚱ, the R rune. `scripts/mark/assets.py` holds its geometry and regenerates every
logo asset in both repos (needs ImageMagick and `rune-android` next to this repo); edit the
shape there, never the generated files.

## Checking UI changes

After changing a screen, render them all and look at the result:
`cargo test -p rune-desktop -- --ignored screenshots` writes `target/screenshots/`.
