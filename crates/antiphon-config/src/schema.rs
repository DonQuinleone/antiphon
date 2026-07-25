use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub ui: Ui,
    pub vault: Vault,
    pub sync: Sync,
    pub daemon: DaemonConfig,
    pub notifications: Notifications,
    pub keys: BTreeMap<String, String>,
    pub saved_searches: Vec<SavedSearch>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sync {
    pub interval_minutes: u32,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DaemonConfig {
    pub autostart: bool,
}

impl Default for DaemonConfig {
    fn default() -> DaemonConfig {
        DaemonConfig { autostart: true }
    }
}

impl Default for Sync {
    fn default() -> Sync {
        Sync {
            interval_minutes: DEFAULT_SYNC_INTERVAL_MINUTES,
        }
    }
}

const DEFAULT_SYNC_INTERVAL_MINUTES: u32 = 5;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ui {
    pub theme: String,
    pub reading_pane: ReadingPane,
    pub date_format: String,
    pub composer: Composer,
    pub list_rows: u16,
    pub sidebar_width: u16,
    pub headers: Vec<String>,
}

impl Default for Ui {
    fn default() -> Ui {
        Ui {
            theme: "vespers".to_string(),
            reading_pane: ReadingPane::Below,
            date_format: "%Y-%m-%d %H:%M".to_string(),
            composer: Composer::Embedded,
            list_rows: DEFAULT_LIST_ROWS,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            headers: DEFAULT_HEADERS
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        }
    }
}

const DEFAULT_LIST_ROWS: u16 = 7;
const DEFAULT_SIDEBAR_WIDTH: u16 = 16;
const DEFAULT_HEADERS: [&str; 4] = ["from", "to", "date", "subject"];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Composer {
    #[default]
    Embedded,
    Suspend,
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
    pub passphrase_cmd: Option<String>,
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
            passphrase_cmd: None,
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
