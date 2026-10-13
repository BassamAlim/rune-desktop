//! The `rune/v1` algorithm. See SPEC.md §2 — this file must match it byte for byte.

use argon2::{Algorithm, Argon2, Params, Version};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use std::path::Path;
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;

pub type Key = [u8; 32];

/// §2.3 — the length a new site starts with.
pub const DEFAULT_LENGTH: usize = 16;

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

/// §2.1 — NFKC, then runs of ASCII whitespace become one space and the ends are trimmed, so a
/// stray or doubled space can't silently change every password. Case is kept.
fn normalize_master(master_password: &str) -> String {
    nfkc(master_password)
        .split(|c| matches!(c, '\t'..='\r' | ' '))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// §2.1 — slow (64 MiB, ~1 s). Run once at setup.
pub fn master_key(master_password: &str) -> Key {
    let params = Params::new(65536, 3, 4, Some(32)).expect("fixed params are valid");
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(normalize_master(master_password).as_bytes(), b"rune/v1/master", &mut out)
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

/// §3.1: the key that seals passwords kept for sites not yet switched to Rune's.
pub fn seal_key(master_key: &Key) -> Key {
    hmac(master_key, &[b"rune/v1/seal"])
}

const SEAL_AAD: &[u8] = b"rune/v1/password";
const SYNC_AAD: &[u8] = b"rune/v1/sync";

/// AES-256-GCM as nonce ‖ ciphertext ‖ tag. `nonce` must be fresh and random for every call.
fn aead_seal(key: &Key, aad: &[u8], data: &[u8], nonce: [u8; 12]) -> Vec<u8> {
    use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("32-byte key"));
    let mut data = data.to_vec();
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad), &mut data)
        .expect("the data fits in one GCM message");
    [nonce.as_slice(), &data].concat()
}

/// `None` on a wrong key or anything damaged or edited.
fn aead_open(key: &Key, aad: &[u8], bytes: &[u8]) -> Option<Vec<u8>> {
    use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
    if bytes.len() < 12 + 16 {
        return None;
    }
    let (nonce, data) = bytes.split_at(12);
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("32-byte key"));
    let mut data = data.to_vec();
    let plain = key.open_in_place(Nonce::try_assume_unique_for_key(nonce).ok()?, Aad::from(aad), &mut data).ok()?;
    Some(plain.to_vec())
}

/// §3.1: AES-256-GCM under `seal_key`, as hex of nonce ‖ ciphertext ‖ tag. `nonce` must be
/// fresh and random for every call.
pub fn seal(seal_key: &Key, password: &str, nonce: [u8; 12]) -> String {
    hex::encode(aead_seal(seal_key, SEAL_AAD, password.as_bytes(), nonce))
}

/// §3.1: the password [`seal`] sealed. Fails on a wrong key or a damaged or edited field.
pub fn open(seal_key: &Key, sealed: &str) -> Result<String, String> {
    let bytes = hex::decode(sealed).map_err(|_| "not a sealed password".to_string())?;
    let plain = aead_open(seal_key, SEAL_AAD, &bytes)
        .ok_or_else(|| "the stored password can't be opened with this master password".to_string())?;
    String::from_utf8(plain).map_err(|_| "not a sealed password".to_string())
}

/// §3.3: what two devices with the same master password prove to each other, and encrypt with.
pub fn sync_key(master_key: &Key) -> Key {
    hmac(master_key, &[b"rune/v1/sync"])
}

/// §3.3: a site list as one sync message.
pub fn sync_seal(sync_key: &Key, list: &SiteList, nonce: [u8; 12]) -> Vec<u8> {
    let json = serde_json::to_vec(list).expect("a site list is always JSON");
    aead_seal(sync_key, SYNC_AAD, &json, nonce)
}

/// §3.3: the list in a sync message. Fails for a device with a different master password.
pub fn sync_open(sync_key: &Key, message: &[u8]) -> Result<SiteList, String> {
    let json = aead_open(sync_key, SYNC_AAD, message).ok_or("not from a device with this master password")?;
    let list: SiteList = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
    if list.version != 1 {
        return Err(format!("unsupported version {}", list.version));
    }
    Ok(list)
}

/// §3.4: an X25519 public key, made fresh for one approval.
pub fn x25519_public(private: &Key) -> Key {
    let mut base = [0u8; 32];
    base[0] = 9;
    x25519_dalek::x25519(*private, base)
}

/// §3.4: what the desktop sends before it shows its public key.
pub fn approval_commit(desktop_pub: &Key) -> Key {
    sha256(&[b"rune/v1/commit", desktop_pub])
}

/// §3.4: the six digits both screens show.
pub fn approval_code(desktop_pub: &Key, phone_pub: &Key) -> String {
    let h = sha256(&[b"rune/v1/code", desktop_pub, phone_pub]);
    format!("{:06}", u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000)
}

/// §3.4: the key for what's secret in an approval. `None` when the other side's key is one
/// that would make the shared secret all zeros.
pub fn answer_key(own_private: &Key, other_pub: &Key, desktop_pub: &Key, phone_pub: &Key) -> Option<Key> {
    let mut shared = x25519_dalek::x25519(*own_private, *other_pub);
    if shared == [0; 32] {
        return None;
    }
    // HKDF-SHA256 (RFC 5869); one block is the 32 bytes wanted.
    let prk = hmac(&[desktop_pub.as_slice(), phone_pub].concat(), &[&shared]);
    shared.fill(0);
    Some(hmac(&prk, &[b"rune/v1/answer", &[1]]))
}

fn sha256(parts: &[&[u8]]) -> Key {
    use sha2::Digest;
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub const REQUEST_AAD: &[u8] = b"rune/v1/request";
pub const ANSWER_AAD: &[u8] = b"rune/v1/answer";
const BOX_AAD: &[u8] = b"rune/v1/box";

/// §3.4: one frame's sealed bytes (without the length), under `sync_key`. `aad` says which side
/// sent it: [`REQUEST_AAD`] or [`ANSWER_AAD`].
pub fn frame_seal(sync_key: &Key, aad: &[u8], frame: &Frame, nonce: [u8; 12]) -> Vec<u8> {
    aead_seal(sync_key, aad, &serde_json::to_vec(frame).expect("a frame is always JSON"), nonce)
}

pub fn frame_open(sync_key: &Key, aad: &[u8], bytes: &[u8]) -> Result<Frame, String> {
    let json = aead_open(sync_key, aad, bytes).ok_or("not from a device with this master password")?;
    serde_json::from_slice(&json).map_err(|e| e.to_string())
}

/// §3.4: a secret inside a frame, under `answer_key`, as hex.
pub fn box_seal<T: Serialize>(answer_key: &Key, value: &T, nonce: [u8; 12]) -> String {
    let mut json = serde_json::to_vec(value).expect("always JSON");
    let sealed = hex::encode(aead_seal(answer_key, BOX_AAD, &json, nonce));
    json.fill(0);
    sealed
}

pub fn box_open<T: serde::de::DeserializeOwned>(answer_key: &Key, sealed: &str) -> Result<T, String> {
    let bytes = hex::decode(sealed).map_err(|_| "not a box".to_string())?;
    let mut json = aead_open(answer_key, BOX_AAD, &bytes).ok_or("the box doesn't open with this key")?;
    let value = serde_json::from_slice(&json).map_err(|e| e.to_string());
    json.fill(0);
    value
}

/// §3.4: every frame of an approval, either way. Fields a frame doesn't use are left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// "show", "keep" or "import".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// Hex, like `pub`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub commit: String,
    #[serde(rename = "pub", skip_serializing_if = "String::is_empty")]
    pub public: String,
    #[serde(rename = "box", skip_serializing_if = "String::is_empty")]
    pub sealed: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub declined: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already: Option<usize>,
}

/// §3.4 `show`: what the phone reveals for one entry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Revealed {
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept: Option<String>,
}

impl Drop for Revealed {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.password.zeroize();
        self.previous.zeroize();
        self.kept.zeroize();
    }
}

/// §3.4 `keep` / `import`: a site and the password the phone should seal for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToKeep {
    pub site: Site,
    pub password: String,
}

impl Drop for ToKeep {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.password.zeroize();
    }
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
    /// Free text kept with the login, e.g. the username when the login is an email. Not secret,
    /// not part of the password; absent in files written before it existed.
    #[serde(default)]
    pub note: String,
    /// §3.1: the site's own password, sealed, until it is changed to Rune's. Empty for most.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sealed: String,
    /// §3.2: when this entry was last changed, in seconds since 1970. Zero in older files.
    #[serde(default)]
    pub modified: u64,
}

/// §3.2: an entry that was deleted, remembered so the delete reaches the other device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deleted {
    pub site: String,
    #[serde(default)]
    pub login: String,
    pub modified: u64,
}

fn default_symbols() -> String {
    DEFAULT_SYMBOLS.into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteList {
    pub version: u32,
    pub sites: Vec<Site>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<Deleted>,
}

impl SiteList {
    /// A missing file is an empty list; anything else unreadable is an error, so a damaged
    /// file is never silently replaced.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { version: 1, sites: vec![], deleted: vec![] });
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

    /// Adds `site`, or replaces the entry at `replacing`. The list stays sorted, a second entry
    /// for the same site and login is refused, and the change is stamped `now` (§3.2) so it
    /// wins over older copies on other devices. Renaming leaves a delete for the old name.
    pub fn put(&mut self, mut site: Site, replacing: Option<usize>, now: u64) -> Result<(), String> {
        let same = |a: &Site, b: &Site| a.site == b.site && a.login == b.login;
        if self.sites.iter().enumerate().any(|(i, other)| Some(i) != replacing && same(other, &site)) {
            return Err("That site and login already exist.".to_string());
        }
        let old = replacing.map(|i| self.sites.remove(i));
        // Later than everything it replaces, even if this device's clock runs behind.
        let before = old.iter().map(|o| o.modified).chain(self.deleted_at(&site.site, &site.login)).max();
        site.modified = before.map_or(now, |b| now.max(b + 1));
        if let Some(old) = old.filter(|old| !same(old, &site)) {
            self.forget(&old, now);
        }
        self.deleted.retain(|d| !(d.site == site.site && d.login == site.login));
        self.sites.push(site);
        self.sites.sort_by(|a, b| (&a.site, &a.login).cmp(&(&b.site, &b.login)));
        Ok(())
    }

    /// Deletes the entry at `index`, remembering the delete (§3.2).
    pub fn remove(&mut self, index: usize, now: u64) {
        let old = self.sites.remove(index);
        self.forget(&old, now);
    }

    fn forget(&mut self, old: &Site, now: u64) {
        self.deleted.retain(|d| !(d.site == old.site && d.login == old.login));
        self.deleted.push(Deleted { site: old.site.clone(), login: old.login.clone(), modified: now.max(old.modified + 1) });
    }

    fn deleted_at(&self, site: &str, login: &str) -> Option<u64> {
        self.deleted.iter().find(|d| d.site == site && d.login == login).map(|d| d.modified)
    }

    /// §3.2: merges `other` in, for import and sync. Per site and login the newer change wins
    /// (at a tie, the higher counter, then this list's), a counter never goes down, and a
    /// delete beats anything older than it. Merging the result back the other way changes
    /// nothing, so two devices settle. Returns how many sites were added and how many changed.
    pub fn merge(&mut self, other: SiteList) -> (usize, usize) {
        use std::collections::BTreeMap;
        type Key = (String, String);
        let key = |site: &str, login: &str| -> Key { (normalize(site), normalize(login)) };

        let before: BTreeMap<Key, Site> =
            std::mem::take(&mut self.sites).into_iter().map(|s| (key(&s.site, &s.login), s)).collect();
        let mut sites = before.clone();
        for mut theirs in other.sites {
            let k = key(&theirs.site, &theirs.login);
            (theirs.site, theirs.login) = k.clone();
            match sites.get_mut(&k) {
                None => {
                    sites.insert(k, theirs);
                }
                Some(mine) => {
                    let counter = mine.counter.max(theirs.counter);
                    if (theirs.modified, theirs.counter) > (mine.modified, mine.counter) {
                        *mine = theirs;
                    }
                    mine.counter = counter;
                }
            }
        }

        let mut deleted: BTreeMap<Key, u64> = BTreeMap::new();
        for d in std::mem::take(&mut self.deleted).into_iter().chain(other.deleted) {
            let at = deleted.entry(key(&d.site, &d.login)).or_default();
            *at = (*at).max(d.modified);
        }
        // A site changed in the same second as its delete stays: nothing is lost to a tie.
        sites.retain(|k, site| deleted.get(k).is_none_or(|at| *at <= site.modified));
        deleted.retain(|k, _| !sites.contains_key(k));

        let added = sites.keys().filter(|k| !before.contains_key(*k)).count();
        let updated = sites.iter().filter(|(k, site)| before.get(*k).is_some_and(|old| old != *site)).count();
        self.sites = sites.into_values().collect();
        self.deleted = deleted.into_iter().map(|((site, login), modified)| Deleted { site, login, modified }).collect();
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
    fn seal_vector() {
        let key = seal_key(&[7; 32]);
        let nonce = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let sealed = seal(&key, "Hunter2 – ünïcode", nonce);
        // Android asserts the same hex: both apps must open each other's files.
        assert_eq!(
            sealed,
            "0102030405060708090a0b0c21bcf4402847d8fc368013cfceee4958760fa13ce5252a5544c5d679c31f12209598356c53"
        );
        assert_eq!(open(&key, &sealed).unwrap(), "Hunter2 – ünïcode");
        assert!(open(&seal_key(&[8; 32]), &sealed).is_err());
        let mut edited = sealed.clone();
        edited.replace_range(30..32, if &sealed[30..32] == "00" { "01" } else { "00" });
        assert!(open(&key, &edited).is_err());
        assert!(open(&key, "zz").is_err());
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
                note: "username: bassam_m".into(),
                sealed: String::new(),
                modified: 0,
            }],
            deleted: vec![],
        };
        list.save(&path).unwrap();
        assert_eq!(SiteList::load(&path).unwrap(), list);
        // Files from before notes existed still load.
        std::fs::write(&path, r#"{"version":1,"sites":[{"site":"a","counter":1,"length":16,"classes":"l"}]}"#).unwrap();
        assert_eq!(SiteList::load(&path).unwrap().sites[0].note, "");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(SiteList::load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn site(name: &str, counter: u32, modified: u64) -> Site {
        Site {
            site: name.into(), login: String::new(), counter,
            length: 20, classes: "luds".into(), symbols: DEFAULT_SYMBOLS.into(), apps: vec![],
            note: String::new(), sealed: String::new(), modified,
        }
    }

    fn list(sites: Vec<Site>) -> SiteList {
        SiteList { version: 1, sites, deleted: vec![] }
    }

    #[test]
    fn merge_higher_counter_wins() {
        let mut mine = list(vec![site("a", 3, 0), site("b", 1, 0)]);
        let counts = mine.merge(list(vec![site("a", 2, 0), site(" B", 2, 0), site("c", 1, 0)]));
        assert_eq!(counts, (1, 1));
        assert_eq!(entropy_bits(20, "luds", DEFAULT_SYMBOLS).round(), 125.0);
        assert_eq!(mine.sites, vec![site("a", 3, 0), site("b", 2, 0), site("c", 1, 0)]);
    }

    #[test]
    fn merge_newer_change_wins_but_a_counter_never_goes_down() {
        let noted = |note: &str, counter, modified| Site { note: note.into(), ..site("a", counter, modified) };
        let mut mine = list(vec![noted("old", 3, 10)]);
        assert_eq!(mine.merge(list(vec![noted("new", 1, 20)])), (0, 1));
        assert_eq!(mine.sites, vec![noted("new", 3, 20)]);

        // The other device, merging the other way, lands on the same list.
        let mut theirs = list(vec![noted("new", 1, 20)]);
        theirs.merge(list(vec![noted("old", 3, 10)]));
        assert_eq!(theirs, mine);
        assert_eq!(theirs.clone().merge(mine.clone()), (0, 0));
    }

    #[test]
    fn deletes_and_renames_travel() {
        let mut phone = list(vec![site("a", 1, 10), site("b", 1, 10)]);
        let mut desktop = phone.clone();

        phone.remove(0, 20);
        phone.put(site("c", 1, 0), Some(0), 20).unwrap(); // "b" renamed to "c"
        assert_eq!(phone.sites.iter().map(|s| s.site.as_str()).collect::<Vec<_>>(), ["c"]);

        desktop.merge(phone.clone());
        assert_eq!(desktop, phone);

        // Added again later, it comes back everywhere, and the delete is forgotten.
        desktop.put(site("a", 1, 0), None, 15).unwrap();
        assert_eq!(desktop.sites[0].modified, 21, "later than the delete, whatever the clock says");
        phone.merge(desktop.clone());
        assert_eq!(phone, desktop);
        assert!(phone.deleted.iter().all(|d| d.site != "a"));

        assert!(phone.put(site("c", 1, 0), None, 30).is_err(), "no second entry for one account");
    }

    const SYNC_VECTOR: &str = "09090909090909090909090932c1ac9a31895ff6306e6f7139af6044bbaa5e2cd5af2f8be8722941db67734bc975fc8e3619d2fb92cd3ff5a4c822cd5e61945fc01acfc418438359b6d0a1381a3d6f376ef1a852c31477de98766e8aade92939144510ff34acca16cb79a7f6c37bff0d819279badb8e64b6f4d221b33f864606b5381b087f1baec866e03cc83fc6d59caa6d89de6db37d88967e9e2fd719d1e02c5ed656cf2247636d4f77c55f738f73";

    #[test]
    fn approval_vector() {
        let (desktop, phone) = ([1; 32], [2; 32]);
        let (desktop_pub, phone_pub) = (x25519_public(&desktop), x25519_public(&phone));
        let key = answer_key(&desktop, &phone_pub, &desktop_pub, &phone_pub).unwrap();
        assert_eq!(answer_key(&phone, &desktop_pub, &desktop_pub, &phone_pub).unwrap(), key, "both sides agree");
        assert!(answer_key(&desktop, &[0; 32], &desktop_pub, &phone_pub).is_none(), "a low-order key is refused");
        // Android's ApprovalTest asserts the same values.
        let got = [
            hex::encode(desktop_pub),
            hex::encode(phone_pub),
            hex::encode(approval_commit(&desktop_pub)),
            approval_code(&desktop_pub, &phone_pub),
            hex::encode(key),
        ];
        assert_eq!(got, APPROVAL_VECTOR);

        let revealed = Revealed { password: "a".into(), previous: Some("b".into()), kept: None };
        let sealed = box_seal(&key, &revealed, [3; 12]);
        assert_eq!(box_open::<Revealed>(&key, &sealed).unwrap(), revealed);
        assert!(box_open::<Revealed>(&[0; 32], &sealed).is_err());

        // Android's ApprovalTest opens this request and seals the same box, byte for byte.
        let request = Frame {
            id: "ab".into(),
            kind: "show".into(),
            site: Some("github.com".into()),
            login: Some(String::new()),
            commit: hex::encode(approval_commit(&desktop_pub)),
            ..Default::default()
        };
        let revealed = Revealed { password: "pw".into(), previous: None, kept: Some("old".into()) };
        assert_eq!(hex::encode(frame_seal(&sync_key(&[7; 32]), REQUEST_AAD, &request, [4; 12])), APPROVAL_FRAME);
        assert_eq!(box_seal(&key, &revealed, [3; 12]), APPROVAL_BOX);

        let frame = Frame { id: "ab".into(), kind: "show".into(), login: Some(String::new()), ..Default::default() };
        let bytes = frame_seal(&[5; 32], REQUEST_AAD, &frame, [4; 12]);
        assert_eq!(frame_open(&[5; 32], REQUEST_AAD, &bytes).unwrap(), frame);
        assert!(frame_open(&[5; 32], ANSWER_AAD, &bytes).is_err(), "a request can't pass as an answer");
    }

    /// desktopPub, phonePub, commit, code, answerKey. Confirmed by Python's `cryptography`.
    const APPROVAL_VECTOR: [&str; 5] = [
        "a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209",
        "ce8d3ad1ccb633ec7b70c17814a5c76ecd029685050d344745ba05870e587d59",
        "96fd0426e04d80775bd47312d4508dda42e8d2f44d38b55e2f27c08f41016cd2",
        "437923",
        "b0b00aa39da3bb7e44274a34529acae4ad457effe3404d6030e4d5e10fa8f7fa",
    ];

    const APPROVAL_FRAME: &str = "040404040404040404040404d589631975cf50fdef031efa339185e19bb9dcb8089757d17e0e4a3b5c8df9a62fa4ec1ca0f46299cf1397ed9d5300ce29b621cd794f9e3783461e81594fda122756b51a21b71f5846820ff7c64f1c9a4c5e65db49ec72c4837899b922817c93592f153390220f0b7c773ec7a4d1f0d5829d24386d5e62b49d25469dd9867f0bf7c0491813789c336e1cc694cc62b5b17463fd49";
    const APPROVAL_BOX: &str = "030303030303030303030303260ef21ae3530030473a4b88650ce2721eb11da272f4fcdede823fc98cdf2dfb41c1ef0a9cd497a7f24af9e892d3";

    #[test]
    fn sync_messages_open_only_with_the_same_master() {
        let key = sync_key(&[7; 32]);
        let sent = list(vec![site("a", 1, 10)]);
        let message = sync_seal(&key, &sent, [9; 12]);
        assert_eq!(sync_open(&key, &message).unwrap(), sent);
        assert!(sync_open(&sync_key(&[8; 32]), &message).is_err());
        // Android's SyncTest opens this exact message.
        assert_eq!(hex::encode(&message), SYNC_VECTOR);
    }
}
