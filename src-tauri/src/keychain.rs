//! API keys live in the macOS Keychain, never in settings.json. Service is the
//! bundle id; one entry per translation provider.

const SERVICE: &str = "com.speakly.app";

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &format!("translation.{provider}"))
        .map_err(|e| format!("keychain: {e}"))
}

pub fn set_key(provider: &str, key: &str) -> Result<(), String> {
    entry(provider)?
        .set_password(key.trim())
        .map_err(|e| format!("keychain write: {e}"))
}

pub fn get_key(provider: &str) -> Result<Option<String>, String> {
    match entry(provider)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keychain read: {e}")),
    }
}

pub fn delete_key(provider: &str) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keychain delete: {e}")),
    }
}

/// `{present, last4}` for the UI — the key itself never round-trips.
pub fn key_status(provider: &str) -> Result<(bool, String), String> {
    Ok(match get_key(provider)? {
        Some(k) => {
            let last4: String = k
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            (true, last4)
        }
        None => (false, String::new()),
    })
}
