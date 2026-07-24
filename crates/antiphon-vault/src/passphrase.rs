use std::process::Command;

use secrecy::SecretString;

use crate::vault::VaultError;

/// Resolve a vault passphrase by running a shell command, the
/// same referenced-not-stored discipline account passwords
/// use. The secret never touches config or argv beyond the
/// command the user chose.
pub fn passphrase_command(
    command: &str,
) -> Result<SecretString, VaultError> {
    let output = Command::new("sh")
        .args(["-c", command])
        .output()
        .map_err(VaultError::Io)?;
    if !output.status.success() {
        return Err(VaultError::PassphraseCommand(format!(
            "`{command}` failed"
        )));
    }
    let secret = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if secret.is_empty() {
        return Err(VaultError::PassphraseCommand(format!(
            "`{command}` produced nothing"
        )));
    }
    Ok(SecretString::from(secret))
}
