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
pub use load::{
    Loaded, NamedAccount, load, signature_text, template_text,
};
pub use schema::{
    Composer, Config, Notifications, ReadingPane, SavedSearch, Sync,
    Ui, Unlock, Vault, VaultBackend,
};
pub use xdg::{Dirs, resolve};
