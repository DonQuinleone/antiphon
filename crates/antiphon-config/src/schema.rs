use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub ui: Ui,
    pub vault: Vault,
    pub sync: Sync,
    pub daemon: DaemonConfig,
    pub accounts: Accounts,
    pub notifications: Notifications,
    pub keys: BTreeMap<String, String>,
    pub saved_searches: Vec<SavedSearch>,
    pub export: Export,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Export {
    pub recipients: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Accounts {
    pub order: Vec<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sync {
    pub interval_minutes: u32,
    pub idle: bool,
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
            idle: false,
        }
    }
}

const DEFAULT_SYNC_INTERVAL_MINUTES: u32 = 2;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ui {
    pub theme: String,
    pub reading_pane: ReadingPane,
    pub accounts_bar: AccountsBar,
    pub date_format: String,
    pub composer: Composer,
    pub list_rows: u16,
    pub sidebar_width: u16,
    pub headers: Vec<String>,
    pub inline_images: bool,
}

impl Default for Ui {
    fn default() -> Ui {
        Ui {
            theme: "vespers".to_string(),
            reading_pane: ReadingPane::Below,
            accounts_bar: AccountsBar::Sidebar,
            date_format: "%Y-%m-%d %H:%M".to_string(),
            composer: Composer::Embedded,
            list_rows: DEFAULT_LIST_ROWS,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            headers: DEFAULT_HEADERS
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            inline_images: true,
        }
    }
}

const DEFAULT_LIST_ROWS: u16 = 7;
const DEFAULT_SIDEBAR_WIDTH: u16 = 16;
const DEFAULT_HEADERS: [&str; 4] = ["from", "to", "date", "subject"];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountsBar {
    #[default]
    Sidebar,
    Tabs,
}

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
            // Touch ID and a YubiKey both unlock once enrolled,
            // but each needs that one-time setup, so the default
            // lists only the passphrase; opt in with `touchid`
            // or `yubikey` after enrolling.
            unlock: vec![Unlock::Passphrase],
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
    /// Folders whose new mail raises a notification; empty
    /// watches every folder. Defaults to the inbox alone, so
    /// filing and sent mail stay quiet.
    pub folders: Vec<String>,
    pub sound: bool,
    pub speech: bool,
}

impl Default for Notifications {
    fn default() -> Notifications {
        Notifications {
            enabled: true,
            folders: vec!["INBOX".to_string()],
            sound: false,
            speech: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSearch {
    pub name: String,
    pub query: String,
}
