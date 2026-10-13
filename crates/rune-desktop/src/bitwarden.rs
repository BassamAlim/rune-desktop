//! Moving in from Bitwarden: its unencrypted JSON export becomes Rune sites that keep their
//! existing passwords (SPEC §3.1) until each is changed to Rune's.

use rune_core::{DEFAULT_LENGTH, DEFAULT_SYMBOLS, Key, Site, normalize, seal};
use serde::Deserialize;
use zeroize::Zeroize;

#[derive(Deserialize)]
struct Export {
    encrypted: bool,
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    #[serde(rename = "type")]
    kind: u8,
    name: Option<String>,
    login: Option<Login>,
}

#[derive(Deserialize)]
struct Login {
    username: Option<String>,
    password: Option<String>,
    uris: Option<Vec<Uri>>,
}

#[derive(Deserialize)]
struct Uri {
    uri: Option<String>,
}

/// Bitwarden's item type for a login; the others are notes, cards and identities.
const LOGIN: u8 = 1;

pub struct Converted {
    /// One per account, each with its password sealed. Not yet stamped or checked against the
    /// list they are going into.
    pub sites: Vec<Site>,
    pub not_logins: usize,
    pub no_password: usize,
    pub duplicates: usize,
}

/// `Ok(None)` when `text` isn't a Bitwarden export at all, so the caller can try it as a Rune
/// site list. An encrypted export is refused: only Bitwarden can open those.
///
/// Taken from each login: its first web address as the site (or its name, for an app or an
/// entry with no address), any `androidapp://` addresses as linked apps, the username, and the
/// password, sealed. Left behind on purpose: notes (they can hold secrets, and the site list
/// isn't encrypted), TOTP seeds, cards and identities.
pub fn convert(text: &str, seal_key: &Key, mut nonce: impl FnMut() -> [u8; 12]) -> Result<Option<Converted>, String> {
    let Ok(export) = serde_json::from_str::<Export>(text) else {
        return Ok(None);
    };
    if export.encrypted {
        return Err("That Bitwarden export is encrypted. Export again and choose the format \".json\", not \".json (Encrypted)\".".to_string());
    }

    let mut converted = Converted { sites: vec![], not_logins: 0, no_password: 0, duplicates: 0 };
    for item in export.items {
        let Some(login) = item.login.filter(|_| item.kind == LOGIN) else {
            converted.not_logins += 1;
            continue;
        };
        let Some(mut password) = login.password.filter(|p| !p.is_empty()) else {
            converted.no_password += 1;
            continue;
        };

        let uris: Vec<String> = login.uris.into_iter().flatten().filter_map(|u| u.uri).collect();
        let apps: Vec<String> =
            uris.iter().filter_map(|u| u.strip_prefix("androidapp://")).map(str::to_owned).collect();
        let host = uris.iter().filter(|u| !u.starts_with("androidapp://")).map(|u| host(u)).find(|h| !h.is_empty());
        let site = host.unwrap_or_else(|| normalize(item.name.as_deref().unwrap_or_default()));
        let username = normalize(login.username.as_deref().unwrap_or_default());

        if site.is_empty() || converted.sites.iter().any(|s| s.site == site && s.login == username) {
            converted.duplicates += 1;
        } else {
            converted.sites.push(Site {
                site,
                login: username,
                counter: 1,
                length: DEFAULT_LENGTH,
                classes: "luds".to_string(),
                symbols: DEFAULT_SYMBOLS.to_string(),
                apps,
                note: String::new(),
                sealed: seal(seal_key, &password, nonce()),
                modified: 0,
            });
        }
        password.zeroize();
    }
    Ok(Some(converted))
}

/// "https://www.github.com:443/login?x" is "github.com": what Rune calls the site.
fn host(uri: &str) -> String {
    let rest = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = host.rsplit_once(':').filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit())).map_or(host, |(h, _)| h);
    let host = normalize(host);
    host.strip_prefix("www.").map(str::to_owned).unwrap_or(host)
}
