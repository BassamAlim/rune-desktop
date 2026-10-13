# AGENTS.md

Rune for Linux: a deterministic password generator. One master password becomes a
different password for every site; passwords are derived, never stored (except a site's old
password, sealed under the master key until it's switched to Rune's; SPEC §3.1). `README.md`
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
- **Every change to the site list goes through `SiteList::put` / `remove`** (rune-core), which
  stamp it (SPEC §3.2). A change that skips them loses to older copies when devices sync.
- **An unreadable site list stops the app** (`Screen::Fatal`) rather than starting empty:
  counters can't be recomputed, so overwriting the file loses passwords.
- **Machine access goes through `store.rs`**: keyring, clipboard, paths, randomness, and all
  networking: each site's `/favicon.ico` and homepage (its logo and display name, cached under
  `~/.cache/rune/icons` and `names`), and the listener that answers the phone's sync (SPEC §3.3)
  and hands it requests to approve (§3.4).
  That listener merges into `sites.json` itself, so sync works while the window is minimised;
  the window reloads when told. `main.rs`
  stays screens only, which keeps the screens testable with `egui_kittest`.

`bitwarden.rs` turns a Bitwarden export into sites with kept passwords (SPEC §5). It is the one
place plain-text passwords from outside are read: they go straight to the phone to seal (an
`import` request), zeroize what was read, and never write notes, TOTP seeds or anything else
from the export to the site list.

**The desktop never keeps the master key** (SPEC §3.4). Anything that needs it is a request to
the phone (`SyncListener::ask`), or, without the phone, the master password typed for that one
action: `App::answer_here` does the phone's work and the key is zeroed straight after. Don't add
a path that holds the master key longer, or stores it.

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
