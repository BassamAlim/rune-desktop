//! Regenerates vectors.json: `cargo run -p echo-core --example gen-vectors > vectors.json`
//! Only rerun when the algorithm version changes — these outputs are the contract with echo-android.

use echo_core::*;
use serde_json::json;

fn main() {
    let secret: Key = core::array::from_fn(|i| i as u8);
    // (master password, site, login, counter, length, classes)
    let cases = [
        ("correct horse battery staple", "github.com", "bassam", 1, 20, "luds"),
        ("correct horse battery staple", "github.com", "bassam", 2, 20, "luds"), // rotation
        ("correct horse battery staple", "github.com", "", 1, 20, "luds"),       // empty login
        ("correct horse battery staple", "  GitHub.COM ", " Bassam", 1, 20, "luds"), // normalises to case 0
        ("correct horse battery staple", "bücher.example", "jürgen", 1, 20, "luds"), // non-ASCII
        ("correct horse battery staple", "bank.example", "bassam", 1, 4, "luds"), // forces rejected candidates
        ("correct horse battery staple", "pin.example", "", 1, 6, "d"),
        ("correct horse battery staple", "long.example", "", 1, 64, "lud"),
        ("pässwörd ﬁ", "github.com", "bassam", 1, 20, "luds"), // NFKC on the master password
    ];
    let vectors: Vec<_> = cases
        .iter()
        .map(|&(pw, site, login, counter, length, classes)| {
            let mk = master_key(pw, &secret);
            let sk = site_key(&mk, site, login, counter);
            json!({
                "masterPassword": pw, "secretKey": hex::encode(secret),
                "secretKeyText": encode_secret_key(&secret),
                "site": site, "login": login, "counter": counter,
                "length": length, "classes": classes,
                "masterKey": hex::encode(mk), "fingerprint": fingerprint(&mk),
                "siteKey": hex::encode(sk),
                "password": password(&sk, length, classes).unwrap(),
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&vectors).unwrap());
}
