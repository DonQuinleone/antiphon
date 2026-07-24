//! Configuration schema and strict TOML loading.
//!
//! Layout under `$XDG_CONFIG_HOME/antiphon` (resolved by this
//! crate on both Linux and macOS, never Library/Application
//! Support): `config.toml`, an `accounts/` directory with one
//! file per account, and an optional `local.toml` of
//! per-machine overrides loaded last. Parsing is strict:
//! unknown keys fail with file, line and the nearest valid key.
//! No secrets ever appear in config; commands and keyring
//! references stand in for them.

mod account;
mod error;
mod load;
mod schema;
mod xdg;

pub use account::{
    Account, AccountFile, Graph, Identity, Imap, Oauth, OauthProvider,
    Rule, Smtp,
};
pub use error::ConfigError;
pub use load::{Loaded, NamedAccount, load};
pub use schema::{
    Config, Notifications, ReadingPane, SavedSearch, Ui, Unlock, Vault,
    VaultBackend,
};
pub use xdg::{Dirs, resolve};
