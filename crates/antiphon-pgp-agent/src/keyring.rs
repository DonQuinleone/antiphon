use std::path::Path;
use std::process::Command;

use sequoia_openpgp::Cert;
use sequoia_openpgp::cert::CertParser;
use sequoia_openpgp::parse::Parse;

use crate::error::AgentError;

pub(crate) fn export_certs(
    gnupg_home: Option<&Path>,
) -> Result<Vec<Cert>, AgentError> {
    let mut command = Command::new("gpg");
    if let Some(home) = gnupg_home {
        command.arg("--homedir").arg(home);
    }
    command.args(["--batch", "--quiet", "--export"]);

    let output = command.output().map_err(|error| {
        AgentError::Keyring(format!("running gpg failed: {error}"))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgentError::Keyring(stderr.trim().to_string()));
    }
    if output.stdout.is_empty() {
        return Ok(Vec::new());
    }

    let parser = CertParser::from_bytes(&output.stdout)
        .map_err(|error| AgentError::Keyring(format!("{error:#}")))?;
    Ok(parser.flatten().collect())
}
