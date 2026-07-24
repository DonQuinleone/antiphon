pub mod apfs;
pub mod gocryptfs;
pub mod luks2;
pub mod passphrase;
pub mod select;
pub mod system;
pub mod vault;

pub use apfs::ApfsVault;
pub use gocryptfs::GocryptfsVault;
pub use luks2::Luks2Vault;
pub use passphrase::passphrase_command;
pub use select::select_backend;
pub use system::{Invocation, RunOutput, System, SystemRunner};
pub use vault::{
    Auth, CreateOptions, DEFAULT_VAULT_BYTES, Mounted, Vault,
    VaultError, VaultStatus,
};
