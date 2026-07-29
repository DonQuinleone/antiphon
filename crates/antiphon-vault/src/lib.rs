pub mod apfs;
pub mod fido2;
pub mod gocryptfs;
pub mod luks2;
pub mod passphrase;
pub mod select;
pub mod system;
pub mod touchid;
pub mod unlock;
pub mod vault;

pub use apfs::ApfsVault;
pub use gocryptfs::GocryptfsVault;
pub use luks2::Luks2Vault;
pub use passphrase::passphrase_command;
pub use select::select_backend;
pub use system::{Invocation, RunOutput, System, SystemRunner};
pub use unlock::{
    PassphraseCmdSource, SecretSource, TouchidSource, YubikeySource,
    enrol_touchid, enrol_yubikey, resolve_passphrase,
};
pub use vault::{
    Auth, CreateOptions, DEFAULT_VAULT_BYTES, Mounted, Vault,
    VaultError, VaultStatus,
};
