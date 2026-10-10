//! Echo `echo/v1` algorithm. See SPEC.md §2 — this file must match it byte for byte.

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
const SYMBOLS: &str = "!@#$%^&*-_=+?";

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
pub fn master_key(master_password: &str, secret_key: &Key) -> Key {
    let params = Params::new(65536, 3, 4, Some(32)).expect("fixed params are valid");
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(nfkc(master_password).as_bytes(), secret_key, &mut out)
        .expect("fixed params are valid");
    out
}

/// §2.2
pub fn site_key(master_key: &Key, site: &str, login: &str, counter: u32) -> Key {
    let site = normalize(site);
    let login = normalize(login);
    hmac(
        master_key,
        &[
            b"echo/v1",
            &(site.len() as u32).to_be_bytes(),
            site.as_bytes(),
            &(login.len() as u32).to_be_bytes(),
            login.as_bytes(),
            &counter.to_be_bytes(),
        ],
    )
}

/// §2.3 — `classes` is any non-empty subset of "luds" (lower, upper, digits, symbols).
pub fn password(site_key: &Key, length: usize, classes: &str) -> Result<String, String> {
    if !(4..=64).contains(&length) {
        return Err(format!("length {length} not in 4..=64"));
    }
    if classes.is_empty() || classes.chars().any(|c| !"luds".contains(c)) {
        return Err(format!("classes {classes:?} must be a non-empty subset of \"luds\""));
    }
    // Fixed order regardless of how `classes` is written.
    let enabled: Vec<&[u8]> = [('l', LOWER), ('u', UPPER), ('d', DIGITS), ('s', SYMBOLS)]
        .iter()
        .filter(|(c, _)| classes.contains(*c))
        .map(|(_, set)| set.as_bytes())
        .collect();
    let charset = enabled.concat();
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
        if enabled.iter().all(|set| candidate.iter().any(|c| set.contains(c))) {
            return Ok(String::from_utf8(candidate).expect("charset is ASCII"));
        }
    }
}

/// Approximate strength of a policy in bits (ignores the small loss from requiring every class).
pub fn entropy_bits(length: usize, classes: &str) -> f64 {
    let n: usize = [('l', LOWER), ('u', UPPER), ('d', DIGITS), ('s', SYMBOLS)]
        .iter()
        .filter(|(c, _)| classes.contains(*c))
        .map(|(_, set)| set.len())
        .sum();
    if n == 0 { 0.0 } else { length as f64 * (n as f64).log2() }
}

/// §2.4
pub fn fingerprint(master_key: &Key) -> String {
    let h = hmac(master_key, &[b"echo/v1/check"]);
    format!("{:02x}{:02x}", h[0], h[1])
}

/// §1 — secret key as text: RFC 4648 Base32, no padding, shown in groups of 4.
pub fn encode_secret_key(key: &Key) -> String {
    let text = data_encoding::BASE32_NOPAD.encode(key);
    let groups: Vec<&str> = (0..text.len()).step_by(4).map(|i| &text[i..i + 4]).collect();
    groups.join(" ")
}

/// Accepts any case and ignores spaces, dashes and other separators.
pub fn decode_secret_key(text: &str) -> Option<Key> {
    let text: String = text.chars().filter(char::is_ascii_alphanumeric).collect();
    let bytes = data_encoding::BASE32_NOPAD.decode(text.to_ascii_uppercase().as_bytes()).ok()?;
    bytes.try_into().ok()
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
    /// Android package names linked to this site; carried through untouched on desktop.
    #[serde(default)]
    pub apps: Vec<String>,
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
            let secret: Key = hex::decode(s("secretKey")).unwrap().try_into().unwrap();
            let mk = master_key(s("masterPassword"), &secret);
            let sk = site_key(&mk, s("site"), s("login"), v["counter"].as_u64().unwrap() as u32);
            assert_eq!(hex::encode(mk), s("masterKey"));
            assert_eq!(fingerprint(&mk), s("fingerprint"));
            assert_eq!(encode_secret_key(&secret), s("secretKeyText"));
            assert_eq!(decode_secret_key(&s("secretKeyText").to_lowercase().replace(' ', "-")), Some(secret));
            assert_eq!(hex::encode(sk), s("siteKey"));
            let pw = password(&sk, v["length"].as_u64().unwrap() as usize, s("classes")).unwrap();
            assert_eq!(pw, s("password"), "{v}");
        }
    }

    #[test]
    fn rejects_bad_secret_key_text() {
        assert_eq!(decode_secret_key(""), None);
        assert_eq!(decode_secret_key("AAAA AAAA"), None); // too short
        assert_eq!(decode_secret_key(&"1".repeat(52)), None); // not Base32
    }

    #[test]
    fn rejects_bad_policy() {
        let sk = [0u8; 32];
        assert!(password(&sk, 3, "luds").is_err());
        assert!(password(&sk, 65, "luds").is_err());
        assert!(password(&sk, 20, "").is_err());
        assert!(password(&sk, 20, "lx").is_err());
    }

    #[test]
    fn save_then_load() {
        let dir = std::env::temp_dir().join(format!("echo-core-test-{}", std::process::id()));
        let path = dir.join("nested/sites.json");
        assert_eq!(SiteList::load(&path).unwrap().sites, vec![]);
        let list = SiteList {
            version: 1,
            sites: vec![Site {
                site: "github.com".into(), login: "bassam".into(), counter: 2,
                length: 20, classes: "luds".into(), apps: vec!["com.github.android".into()],
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
            length: 20, classes: "luds".into(), apps: vec![],
        };
        let mut mine = SiteList { version: 1, sites: vec![site("a", 3), site("b", 1)] };
        let counts = mine.merge(SiteList { version: 1, sites: vec![site("a", 2), site(" B", 2), site("c", 1)] });
        assert_eq!(counts, (1, 1));
        assert_eq!(entropy_bits(20, "luds").round(), 125.0);
        assert_eq!(mine.sites, vec![site("a", 3), site("b", 2), site("c", 1)]);
    }
}
