//! Everything that touches the machine outside the window: keyring, clipboard, paths, randomness.

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use arboard::{Clipboard, SetExtLinux};
use eframe::egui;
use rune_core::Key;
use zeroize::Zeroize;

const SERVICE: &str = "rune";
pub const ACCOUNT: &str = "keys";
pub const CLIPBOARD_CLEAR_AFTER: Duration = Duration::from_secs(30);

pub struct Keys {
    pub master: Key,
}

impl Drop for Keys {
    fn drop(&mut self) {
        self.master.zeroize();
    }
}

/// Stored as one 32-byte keyring secret: the master key.
pub fn load_keys(account: &str) -> Result<Option<Keys>, String> {
    let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
    match entry.get_secret() {
        Ok(mut bytes) => {
            let keys = match <[u8; 32]>::try_from(bytes.as_slice()) {
                Ok(master) => Ok(Some(Keys { master })),
                // 64 bytes is the older secret key ‖ master key layout.
                Err(_) => Err("the stored keys are from an older Rune; set it up again".to_string()),
            };
            bytes.zeroize();
            keys
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_keys(account: &str, keys: &Keys) -> Result<(), String> {
    keyring::Entry::new(SERVICE, account)
        .and_then(|entry| entry.set_secret(&keys.master))
        .map_err(|e| e.to_string())
}

pub fn delete_keys(account: &str) -> Result<(), String> {
    match keyring::Entry::new(SERVICE, account).and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn random_u16() -> std::io::Result<u16> {
    let mut bytes = [0u8; 2];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

/// A site's favicon, from `https://<site>/favicon.ico` and nowhere else, kept in the cache dir.
/// `None` while it is being fetched (`done` runs when it lands) or after the network failed;
/// `Some(None)` when the site has no usable one, or isn't an address at all.
pub fn favicon(site: &str, done: impl FnOnce() + Send + 'static) -> Option<Option<egui::ColorImage>> {
    const MAX_SIDE: u32 = 512;

    let keep = |bytes: Vec<u8>| -> Option<Vec<u8>> {
        let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?;
        let (w, h) = reader.into_dimensions().ok()?;
        if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
            return None;
        }
        let mut png = Vec::new();
        image::load_from_memory(&bytes)
            .ok()?
            .thumbnail(72, 72)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .ok()?;
        Some(png)
    };
    let bytes = cached(site, "icons", "favicon.ico", 512 * 1024, keep, done)?;
    Some(bytes.and_then(|bytes| {
        let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
        Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &rgba))
    }))
}

/// The name a site gives itself on its homepage ("GitHub" for github.com), cached like
/// [`favicon`] and with the same `None` / `Some(None)` meanings. Display only: the stored name,
/// which the password is made from, never changes.
pub fn site_name(site: &str, done: impl FnOnce() + Send + 'static) -> Option<Option<String>> {
    let keep = |bytes: Vec<u8>| site_title(&String::from_utf8_lossy(&bytes)).map(String::into_bytes);
    let bytes = cached(site, "names", "", 256 * 1024, keep, done)?;
    Some(bytes.and_then(|bytes| String::from_utf8(bytes).ok()))
}

/// What `https://<site>/<path>` gave, reduced by `keep` and cached under `folder`. `None` while
/// it is being fetched or after the network failed; `Some(None)` when there is nothing usable
/// (remembered as an empty file) or the site is a plain name.
fn cached(
    site: &str,
    folder: &str,
    path: &str,
    limit: u64,
    keep: impl FnOnce(Vec<u8>) -> Option<Vec<u8>> + Send + 'static,
    done: impl FnOnce() + Send + 'static,
) -> Option<Option<Vec<u8>>> {
    // Each file is asked for at most once per run, so an offline machine doesn't ask every frame.
    static ASKED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

    let host = site.split('/').next().unwrap_or_default().to_owned();
    // "bank" or "my laptop" is a name, not an address: nothing to ask.
    if !host.contains('.') || host.contains(char::is_whitespace) {
        return Some(None);
    }
    let file = cache_dir().join(folder).join(&host);
    if let Ok(bytes) = std::fs::read(&file) {
        return Some((!bytes.is_empty()).then_some(bytes));
    }

    if ASKED.lock().unwrap().get_or_insert_default().insert(file.clone()) {
        let url = format!("https://{host}/{path}");
        std::thread::spawn(move || {
            // A network failure writes nothing, so the next run tries again.
            if let Some(body) = download(&url, limit) {
                let kept = if body.is_empty() { None } else { keep(body) };
                let _ = std::fs::create_dir_all(file.parent().unwrap());
                let partial = PathBuf::from(format!("{}.part", file.display()));
                if std::fs::write(&partial, kept.unwrap_or_default()).is_ok() {
                    let _ = std::fs::rename(&partial, &file);
                }
            }
            done();
        });
    }
    None
}

/// `None` when the network failed; empty when the site answered with an error.
fn download(url: &str, limit: u64) -> Option<Vec<u8>> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).build();
    let response = match agent.get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(..)) => return Some(Vec::new()),
        Err(ureq::Error::Transport(_)) => return None,
    };
    let mut bytes = Vec::new();
    response.into_reader().take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The name a homepage gives itself: `og:site_name`, else `application-name`, else the title up
/// to its first separator ("GitHub · Build and ship software" is "GitHub").
pub fn site_title(html: &str) -> Option<String> {
    // ASCII lowercasing keeps byte offsets, so positions found in `lower` index `html` too.
    let lower = html.to_ascii_lowercase();

    let meta = |key: &str| -> Option<String> {
        lower.match_indices("<meta").find_map(|(start, _)| {
            let end = start + lower[start..].find('>')?;
            let tag = &lower[start..end];
            let named = ["property", "name"]
                .iter()
                .any(|attr| tag.contains(&format!("{attr}=\"{key}\"")) || tag.contains(&format!("{attr}='{key}'")));
            if !named {
                return None;
            }
            let at = start + tag.find("content=")? + "content=".len();
            let quote = html[at..].chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let value = &html[at + 1..];
            Some(decode_entities(&value[..value.find(quote)?]))
        })
    };
    let title = || -> Option<String> {
        let open = lower.find("<title")?;
        let start = open + lower[open..].find('>')? + 1;
        let end = start + lower[start..].find("</title")?;
        let title = decode_entities(&html[start..end]);
        let first = [" | ", " - ", " · ", " — ", " – ", ": ", ". "]
            .iter()
            .fold(title.as_str(), |t, sep| t.split(sep).next().unwrap_or(t))
            .to_owned();
        // A bot check or error page names itself, not the site.
        let walls = ["just a moment", "attention required", "access denied", "error", "403", "404"];
        (!walls.iter().any(|w| first.trim().to_lowercase().starts_with(w))).then_some(first)
    };

    let name = meta("og:site_name").or_else(|| meta("application-name")).or_else(title)?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (1..=40).contains(&name.chars().count()).then_some(name)
}

fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// "my bank" is "My Bank"; an address like "github.com" stays as it is.
pub fn title_case(site: &str) -> String {
    if site.contains('.') {
        return site.to_owned();
    }
    site.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or(String::new(), |first| first.to_uppercase().chain(chars).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".cache")))
        .unwrap_or_default()
        .join("rune")
}

pub fn sites_path() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_default()
        .join("rune/sites.json")
}

/// Puts a secret on the clipboard, flagged so clipboard managers (Klipper) keep it out of history.
pub fn copy_secret(clipboard: &mut Clipboard, text: &str) -> Result<(), arboard::Error> {
    clipboard.set().exclude_from_history().text(text)
}

/// Clears the clipboard after the delay, unless something else has been copied since.
/// A thread rather than a UI timer: a hidden Wayland window may not get repainted.
pub fn clear_clipboard_later(mut text: String) {
    std::thread::spawn(move || {
        std::thread::sleep(CLIPBOARD_CLEAR_AFTER);
        if let Ok(mut clipboard) = Clipboard::new()
            && clipboard.get_text().is_ok_and(|current| current == text)
        {
            let _ = clipboard.clear();
        }
        text.zeroize();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes to the real session keyring"]
    fn keyring_roundtrip() {
        let account = "test";
        let keys = Keys { master: [2; 32] };
        save_keys(account, &keys).unwrap();
        let loaded = load_keys(account).unwrap().unwrap();
        assert_eq!(loaded.master, keys.master);
        delete_keys(account).unwrap();
        assert!(load_keys(account).unwrap().is_none());
        delete_keys(account).unwrap(); // deleting twice is fine
    }

    #[test]
    #[ignore = "overwrites the real clipboard"]
    fn clipboard_secret_is_flagged_and_readable() {
        let mut clipboard = Clipboard::new().unwrap();
        copy_secret(&mut clipboard, "rune-clipboard-test").unwrap();
        assert_eq!(clipboard.get_text().unwrap(), "rune-clipboard-test");
        let types = std::process::Command::new("wl-paste").arg("--list-types").output().unwrap();
        let types = String::from_utf8_lossy(&types.stdout).into_owned();
        assert!(types.contains("x-kde-passwordManagerHint"), "offered types: {types}");
        clipboard.clear().unwrap();
    }
}
