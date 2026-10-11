//! Regenerates vectors.json: `cargo run -p rune-core --example gen-vectors > vectors.json`
//! Only rerun when the algorithm version changes — these outputs are the contract with rune-android.

use rune_core::*;
use serde_json::json;

fn main() {
    let d = DEFAULT_SYMBOLS;
    // (master password, site, login, counter, length, classes, symbols)
    let cases = [
        ("correct horse battery staple", "github.com", "bassam", 1, 20, "luds", d),
        ("correct horse battery staple", "github.com", "bassam", 2, 20, "luds", d), // rotation
        ("correct horse battery staple", "github.com", "", 1, 20, "luds", d),       // empty login
        ("correct horse battery staple", "  GitHub.COM ", " Bassam", 1, 20, "luds", d), // normalises to case 0
        ("correct horse battery staple", "bücher.example", "jürgen", 1, 20, "luds", d), // non-ASCII
        ("correct horse battery staple", "bank.example", "bassam", 1, 4, "luds", d), // forces rejected candidates
        ("correct horse battery staple", "pin.example", "", 1, 6, "d", d),
        ("correct horse battery staple", "long.example", "", 1, 64, "lud", d),
        ("pässwörd ﬁ", "github.com", "bassam", 1, 20, "luds", d), // NFKC on the master password
        ("correct horse battery staple", "strict.example", "", 1, 16, "luds", "._-"), // canonical order is "-_."
        ("correct horse battery staple", "strict.example", "", 1, 16, "luds", "-_."), // same password as above
    ];
    let vectors: Vec<_> = cases
        .iter()
        .map(|&(pw, site, login, counter, length, classes, symbols)| {
            let mk = master_key(pw);
            let sk = site_key(&mk, site, login, counter);
            json!({
                "masterPassword": pw,
                "site": site, "login": login, "counter": counter,
                "length": length, "classes": classes, "symbols": symbols,
                "masterKey": hex::encode(mk), "fingerprint": fingerprint(&mk),
                "siteKey": hex::encode(sk),
                "password": password(&sk, length, classes, symbols).unwrap(),
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&vectors).unwrap());
}
