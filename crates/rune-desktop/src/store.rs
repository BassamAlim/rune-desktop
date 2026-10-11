//! Everything that touches the machine outside the window: keyring, clipboard, paths, randomness.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use arboard::{Clipboard, SetExtLinux};
use rune_core::Key;
use zeroize::Zeroize;

const SERVICE: &str = "rune";
pub const ACCOUNT: &str = "keys";
pub const CLIPBOARD_CLEAR_AFTER: Duration = Duration::from_secs(30);

pub struct Keys {
    pub secret: Key,
    pub master: Key,
}

impl Drop for Keys {
    fn drop(&mut self) {
        self.secret.zeroize();
        self.master.zeroize();
    }
}

/// Stored as one 64-byte keyring secret: secret key ‖ master key.
pub fn load_keys(account: &str) -> Result<Option<Keys>, String> {
    let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
    match entry.get_secret() {
        Ok(mut bytes) => {
            let keys = match bytes.as_slice().split_at_checked(32) {
                Some((secret, master)) if master.len() == 32 => Ok(Some(Keys {
                    secret: secret.try_into().unwrap(),
                    master: master.try_into().unwrap(),
                })),
                _ => Err("the stored Rune keys are corrupt".to_string()),
            };
            bytes.zeroize();
            keys
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_keys(account: &str, keys: &Keys) -> Result<(), String> {
    let mut bytes = [keys.secret, keys.master].concat();
    let result = keyring::Entry::new(SERVICE, account)
        .and_then(|entry| entry.set_secret(&bytes))
        .map_err(|e| e.to_string());
    bytes.zeroize();
    result
}

pub fn delete_keys(account: &str) -> Result<(), String> {
    match keyring::Entry::new(SERVICE, account).and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn random_key() -> std::io::Result<Key> {
    let mut key = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut key)?;
    Ok(key)
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
        let keys = Keys { secret: [1; 32], master: [2; 32] };
        save_keys(account, &keys).unwrap();
        let loaded = load_keys(account).unwrap().unwrap();
        assert_eq!((loaded.secret, loaded.master), (keys.secret, keys.master));
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
