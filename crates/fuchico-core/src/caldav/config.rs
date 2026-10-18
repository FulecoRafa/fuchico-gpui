use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(all(not(target_os = "linux"), not(test)))]
use keyring::{Entry as KeyringEntry, Error as KeyringError};
#[cfg(all(target_os = "linux", not(test)))]
use keyring_core::{api::CredentialStoreApi, Entry as KeyringEntry, Error as KeyringError};

use crate::fs::file::write_atomic;

/// Matches `tauri.conf.json`'s `identifier` -- the keyring service name
/// under which every account's app-specific password is stored, keyed by
/// that account's `id`.
#[cfg_attr(test, allow(dead_code))]
const KEYRING_SERVICE: &str = "dev.fuleco.fuchico";

/// A saved CalDAV account. Never carries a password -- that lives in the OS
/// keyring, looked up by `id` via [`load_password`].
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Account {
    pub id: String,
    #[serde(rename = "serverUrl")]
    pub server_url: String,
    pub username: String,
}

/// A folder opted into sync against one calendar of one account. `folder_path`
/// is a canonical absolute path (see `fs::to_canon`); the corresponding
/// `.fuchico-sync.json` sidecar lives inside that folder.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FolderLink {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "calendarHref")]
    pub calendar_href: String,
    #[serde(rename = "calendarDisplayName")]
    pub calendar_display_name: String,
    #[serde(rename = "folderPath")]
    pub folder_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub links: Vec<FolderLink>,
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("caldav").join("config.json")
}

/// `data_dir` is the app data dir (see [`crate::paths::app_data_dir`]).
pub fn load(data_dir: &Path) -> Result<Config, String> {
    let path = config_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|e| format!("corrupt caldav config at {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("failed to read {}: {e}", path.display())),
    }
}

pub fn save(data_dir: &Path, config: &Config) -> Result<(), String> {
    let path = config_path(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    write_atomic(&path, &json).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// Creates an entry with this app's fixed service name and maps store errors
/// to plain strings for Tauri IPC. Linux uses the kernel keyring directly, so
/// it works without a running Secret Service; other platforms use their native
/// store through `keyring`'s compatibility API.
#[cfg(all(target_os = "linux", not(test)))]
fn keyring_entry(account_id: &str) -> Result<KeyringEntry, String> {
    let store = linux_keyutils_keyring_store::Store::new()
        .map_err(|e| format!("kernel keyring unavailable: {e}"))?;
    store
        .build(KEYRING_SERVICE, account_id, None)
        .map_err(|e| format!("kernel keyring unavailable: {e}"))
}

#[cfg(all(not(target_os = "linux"), not(test)))]
fn keyring_entry(account_id: &str) -> Result<KeyringEntry, String> {
    KeyringEntry::new(KEYRING_SERVICE, account_id).map_err(|e| format!("keyring unavailable: {e}"))
}

pub fn save_password(account_id: &str, password: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        test_keyring::set(account_id, password);
        Ok(())
    }
    #[cfg(not(test))]
    keyring_entry(account_id)?
        .set_password(password)
        .map_err(|e| format!("failed to save password to keyring: {e}"))
}

pub fn load_password(account_id: &str) -> Result<String, String> {
    #[cfg(test)]
    {
        test_keyring::get(account_id)
            .ok_or_else(|| "failed to read password from keyring: No matching entry found in secure storage".to_string())
    }
    #[cfg(not(test))]
    keyring_entry(account_id)?
        .get_password()
        .map_err(|e| format!("failed to read password from keyring: {e}"))
}

/// Missing credential is not an error here -- deleting an account whose
/// password lookup already failed (e.g. keyring was cleared out-of-band)
/// should still succeed in removing the account record.
pub fn delete_password(account_id: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        test_keyring::remove(account_id);
        Ok(())
    }
    #[cfg(not(test))]
    match keyring_entry(account_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(format!("failed to delete password from keyring: {e}")),
    }
}

/// Unit tests must never touch the user's real OS keychain, so test builds
/// keep passwords in a process-wide map instead. Account ids are fresh
/// UUIDs, so parallel tests don't collide.
#[cfg(test)]
pub(crate) mod test_keyring {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn store() -> &'static Mutex<HashMap<String, String>> {
        static S: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        S.get_or_init(Default::default)
    }

    pub fn set(id: &str, pw: &str) {
        store().lock().unwrap().insert(id.to_string(), pw.to_string());
    }

    pub fn get(id: &str) -> Option<String> {
        store().lock().unwrap().get(id).cloned()
    }

    pub fn remove(id: &str) {
        store().lock().unwrap().remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_account(id: &str) -> Account {
        Account {
            id: id.to_string(),
            server_url: "https://caldav.icloud.com".to_string(),
            username: "user@example.com".to_string(),
        }
    }

    fn sample_link(account_id: &str) -> FolderLink {
        FolderLink {
            account_id: account_id.to_string(),
            calendar_href: "https://caldav.icloud.com/123/calendars/home/".to_string(),
            calendar_display_name: "Reminders".to_string(),
            folder_path: "/Users/x/notes".to_string(),
        }
    }

    #[test]
    fn config_round_trips_through_serde() {
        let mut config = Config::default();
        config.accounts.push(sample_account("acct-1"));
        config.links.push(sample_link("acct-1"));

        let json = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.accounts.len(), 1);
        assert_eq!(parsed.accounts[0].id, "acct-1");
        assert_eq!(parsed.links.len(), 1);
        assert_eq!(parsed.links[0].calendar_display_name, "Reminders");
    }

    #[test]
    fn config_never_serializes_a_password_field() {
        let mut config = Config::default();
        config.accounts.push(sample_account("acct-1"));
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.to_lowercase().contains("password"));
    }

    #[test]
    fn empty_config_deserializes_from_missing_fields() {
        let parsed: Config = serde_json::from_str("{}").unwrap();
        assert!(parsed.accounts.is_empty());
        assert!(parsed.links.is_empty());
    }
}
