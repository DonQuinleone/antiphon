use std::collections::BTreeMap;

use serde::Deserialize;

/// Global settings from `config.toml`, after any `local.toml`
/// overrides.
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub ui: Ui,
    pub vault: Vault,
    pub notifications: Notifications,
    pub keys: BTreeMap<String, String>,
    pub saved_searches: Vec<SavedSearch>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ui {
    pub theme: String,
    pub reading_pane: ReadingPane,
    pub date_format: String,
}

impl Default for Ui {
    fn default() -> Ui {
        Ui {
            theme: "vespers".to_string(),
            reading_pane: ReadingPane::Below,
            date_format: "%d %b %H:%M".to_string(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadingPane {
    #[default]
    Below,
    Right,
    Off,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Vault {
    pub backend: VaultBackend,
    pub idle_lock_minutes: u32,
    pub unlock: Vec<Unlock>,
}

impl Default for Vault {
    fn default() -> Vault {
        Vault {
            backend: VaultBackend::Auto,
            idle_lock_minutes: 0,
            unlock: vec![
                Unlock::Touchid,
                Unlock::Yubikey,
                Unlock::Passphrase,
            ],
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultBackend {
    #[default]
    Auto,
    Luks2,
    Apfs,
    Gocryptfs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unlock {
    Touchid,
    Yubikey,
    Passphrase,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Notifications {
    pub enabled: bool,
}

impl Default for Notifications {
    fn default() -> Notifications {
        Notifications { enabled: true }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSearch {
    pub name: String,
    pub query: String,
}
