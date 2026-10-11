//! The `rune/v1` algorithm. See SPEC.md §2 — this file must match it byte for byte.

use argon2::{Algorithm, Argon2, Params, Version};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use std::path::Path;
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;

pub type Key = [u8; 32];

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
/// The default symbols, then the rest of ASCII punctuation: the fixed order a site's symbols
/// take in the charset (§2.3). Keeping the defaults first keeps default passwords unchanged.
pub const DEFAULT_SYMBOLS: &str = "!@#$%^&*-_=+?";
const ALL_SYMBOLS: &str = "!@#$%^&*-_=+?\"'(),./:;<>[\\]`{|}~";

fn nfkc(s: &str) -> String {
    s.nfkc().collect()
}

/// §2.2 — how site and login names are canonicalised before derivation.
pub fn normalize(s: &str) -> String {
    nfkc(s).trim().to_lowercase()
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> Key {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// §2.1 — slow (64 MiB, ~1 s). Run once at setup.
pub fn master_key(master_password: &str) -> Key {
    let params = Params::new(65536, 3, 4, Some(32)).expect("fixed params are valid");
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(nfkc(master_password).as_bytes(), b"rune/v1/master", &mut out)
        .expect("fixed params are valid");
    out
}

/// §1 — the EFF large wordlist: 7776 words, so each is log2(7776) ≈ 12.9 bits.
const WORDS: &str = include_str!("../words.txt");
pub const PASSPHRASE_WORDS: usize = 6;

/// §1 — a generated master password: `PASSPHRASE_WORDS` words, each picked uniformly using
/// `random`, which must return uniformly random u16s (from the OS).
pub fn passphrase(mut random: impl FnMut() -> u16) -> String {
    let words: Vec<&str> = WORDS.lines().collect();
    let limit = 65536 - 65536 % words.len() as u32;
    (0..PASSPHRASE_WORDS)
        .map(|_| loop {
            let r = random() as u32;
            if r < limit {
                break words[r as usize % words.len()];
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Strength of a generated passphrase in bits.
pub fn passphrase_bits() -> f64 {
    PASSPHRASE_WORDS as f64 * (WORDS.lines().count() as f64).log2()
}

/// §2.2
pub fn site_key(master_key: &Key, site: &str, login: &str, counter: u32) -> Key {
    let site = normalize(site);
    let login = normalize(login);
    hmac(
        master_key,
        &[
            b"rune/v1",
            &(site.len() as u32).to_be_bytes(),
            site.as_bytes(),
            &(login.len() as u32).to_be_bytes(),
            login.as_bytes(),
            &counter.to_be_bytes(),
        ],
    )
}

/// §2.3 — a site's symbols in canonical order. Any ASCII punctuation, any order, repeats allowed.
pub fn canonical_symbols(symbols: &str) -> Result<String, String> {
    if let Some(c) = symbols.chars().find(|&c| !ALL_SYMBOLS.contains(c)) {
        return Err(format!("{c:?} is not an ASCII symbol"));
    }
    Ok(ALL_SYMBOLS.chars().filter(|&c| symbols.contains(c)).collect())
}

fn charset(classes: &str, symbols: &str) -> Result<Vec<String>, String> {
    if classes.is_empty() || classes.chars().any(|c| !"luds".contains(c)) {
        return Err(format!("classes {classes:?} must be a non-empty subset of \"luds\""));
    }
    let symbols = canonical_symbols(symbols)?;
    if classes.contains('s') && symbols.is_empty() {
        return Err("symbols are on but none are allowed".into());
    }
    // Fixed order regardless of how `classes` is written.
    Ok([('l', LOWER), ('u', UPPER), ('d', DIGITS), ('s', symbols.as_str())]
        .iter()
        .filter(|(c, _)| classes.contains(*c))
        .map(|(_, set)| set.to_string())
        .collect())
}

/// §2.3 — `classes` is any non-empty subset of "luds" (lower, upper, digits, symbols);
/// `symbols` is the site's symbol set, used when `classes` has `s`.
pub fn password(site_key: &Key, length: usize, classes: &str, symbols: &str) -> Result<String, String> {
    if !(4..=64).contains(&length) {
        return Err(format!("length {length} not in 4..=64"));
    }
    let enabled = charset(classes, symbols)?;
    let charset = enabled.concat().into_bytes();
    let n = charset.len();
    let limit = 256 - 256 % n;

    let mut stream = (0u32..).flat_map(|i| hmac(site_key, &[&i.to_be_bytes()]));
    loop {
        let candidate: Vec<u8> = stream
            .by_ref()
            .filter(|&b| (b as usize) < limit)
            .map(|b| charset[b as usize % n])
            .take(length)
            .collect();
        if enabled.iter().all(|set| candidate.iter().any(|c| set.as_bytes().contains(c))) {
            return Ok(String::from_utf8(candidate).expect("charset is ASCII"));
        }
    }
}

/// Approximate strength of a policy in bits (ignores the small loss from requiring every class).
pub fn entropy_bits(length: usize, classes: &str, symbols: &str) -> f64 {
    match charset(classes, symbols) {
        Ok(sets) => length as f64 * (sets.concat().len() as f64).log2(),
        Err(_) => 0.0,
    }
}

/// §2.4
pub fn fingerprint(master_key: &Key) -> String {
    let h = hmac(master_key, &[b"rune/v1/check"]);
    format!("{:02x}{:02x}", h[0], h[1])
}

/// §3
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Site {
    pub site: String,
    #[serde(default)]
    pub login: String,
    pub counter: u32,
    pub length: usize,
    pub classes: String,
    /// The symbols this site accepts; absent in files written before it existed.
    #[serde(default = "default_symbols")]
    pub symbols: String,
    /// Android package names linked to this site; carried through untouched on desktop.
    #[serde(default)]
    pub apps: Vec<String>,
}

fn default_symbols() -> String {
    DEFAULT_SYMBOLS.into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteList {
    pub version: u32,
    pub sites: Vec<Site>,
}

impl SiteList {
    /// A missing file is an empty list; anything else unreadable is an error, so a damaged
    /// file is never silently replaced.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { version: 1, sites: vec![] });
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let list: Self = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if list.version != 1 {
            return Err(format!("{}: unsupported version {}", path.display(), list.version));
        }
        Ok(list)
    }

    /// Writes to a temporary file and renames it, so a crash can't leave a half-written list.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("json.tmp");
        let write = || {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
            std::fs::rename(&tmp, path)
        };
        write().map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Import: merge by `(site, login)`; the higher counter wins.
    /// Returns how many sites were added and how many were replaced.
    pub fn merge(&mut self, other: SiteList) -> (usize, usize) {
        let (mut added, mut updated) = (0, 0);
        for mut s in other.sites {
            s.site = normalize(&s.site);
            s.login = normalize(&s.login);
            match self.sites.iter_mut().find(|m| m.site == s.site && m.login == s.login) {
                Some(mine) if s.counter > mine.counter => {
                    *mine = s;
                    updated += 1;
                }
                Some(_) => {}
                None => {
                    self.sites.push(s);
                    added += 1;
                }
            }
        }
        self.sites.sort_by(|a, b| (&a.site, &a.login).cmp(&(&b.site, &b.login)));
        (added, updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors() {
        let vectors: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../vectors.json")).unwrap();
        assert!(!vectors.is_empty());
        for v in &vectors {
            let s = |k: &str| v[k].as_str().unwrap();
            let mk = master_key(s("masterPassword"));
            let sk = site_key(&mk, s("site"), s("login"), v["counter"].as_u64().unwrap() as u32);
            assert_eq!(hex::encode(mk), s("masterKey"));
            assert_eq!(fingerprint(&mk), s("fingerprint"));
            assert_eq!(hex::encode(sk), s("siteKey"));
            let pw = password(&sk, v["length"].as_u64().unwrap() as usize, s("classes"), s("symbols")).unwrap();
            assert_eq!(pw, s("password"), "{v}");
        }
    }

    #[test]
    fn passphrase_is_six_words_from_the_list() {
        let mut n = 0u16;
        // Starts in the rejected range: 62208.. never picks a word.
        let mut values = [65535u16, 62208].into_iter().chain(std::iter::repeat_with(move || { n += 1111; n }));
        let phrase = passphrase(|| values.next().unwrap());
        let words: Vec<&str> = phrase.split(' ').collect();
        assert_eq!(words.len(), 6);
        assert_eq!(words[0], WORDS.lines().nth(1111).unwrap());
        assert!(words.iter().all(|w| WORDS.lines().any(|l| l == *w)));
        assert_eq!(passphrase_bits().round(), 78.0);
    }

    #[test]
    fn rejects_bad_policy() {
        let sk = [0u8; 32];
        let d = DEFAULT_SYMBOLS;
        assert!(password(&sk, 3, "luds", d).is_err());
        assert!(password(&sk, 65, "luds", d).is_err());
        assert!(password(&sk, 20, "", d).is_err());
        assert!(password(&sk, 20, "lx", d).is_err());
        assert!(password(&sk, 20, "luds", "").is_err());
        assert!(password(&sk, 20, "luds", "é").is_err());
        assert!(password(&sk, 20, "lud", "").is_ok()); // symbols off: the set doesn't matter
        assert_eq!(canonical_symbols("~.-!!").unwrap(), "!-.~");
    }

    #[test]
    fn save_then_load() {
        let dir = std::env::temp_dir().join(format!("rune-core-test-{}", std::process::id()));
        let path = dir.join("nested/sites.json");
        assert_eq!(SiteList::load(&path).unwrap().sites, vec![]);
        let list = SiteList {
            version: 1,
            sites: vec![Site {
                site: "github.com".into(), login: "bassam".into(), counter: 2,
                length: 20, classes: "luds".into(), symbols: "-_".into(), apps: vec!["com.github.android".into()],
            }],
        };
        list.save(&path).unwrap();
        assert_eq!(SiteList::load(&path).unwrap(), list);
        std::fs::write(&path, "{ not json").unwrap();
        assert!(SiteList::load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn merge_higher_counter_wins() {
        let site = |name: &str, counter| Site {
            site: name.into(), login: String::new(), counter,
            length: 20, classes: "luds".into(), symbols: DEFAULT_SYMBOLS.into(), apps: vec![],
        };
        let mut mine = SiteList { version: 1, sites: vec![site("a", 3), site("b", 1)] };
        let counts = mine.merge(SiteList { version: 1, sites: vec![site("a", 2), site(" B", 2), site("c", 1)] });
        assert_eq!(counts, (1, 1));
        assert_eq!(entropy_bits(20, "luds", DEFAULT_SYMBOLS).round(), 125.0);
        assert_eq!(mine.sites, vec![site("a", 3), site("b", 2), site("c", 1)]);
    }
}
