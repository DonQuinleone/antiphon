use std::path::Path;

use sequoia_openpgp::Cert;
use sequoia_openpgp::cert::CertParser;
use sequoia_openpgp::parse::Parse;

const CERT_EXTENSIONS: [&str; 2] = ["asc", "pgp"];

#[derive(Debug, Clone, Default)]
pub struct Keyring {
    certs: Vec<Cert>,
}

impl Keyring {
    /// Loads every trusted public cert from a directory of `.asc`
    /// and `.pgp` files. A missing directory yields an empty
    /// keyring rather than an error, so an unconfigured client
    /// simply cannot verify anything.
    pub fn from_dir(path: impl AsRef<Path>) -> Keyring {
        let Ok(entries) = std::fs::read_dir(path.as_ref()) else {
            return Keyring::default();
        };
        let mut certs = Vec::new();
        for entry in entries.flatten() {
            load_file(&entry.path(), &mut certs);
        }
        Keyring { certs }
    }

    pub fn certs(&self) -> &[Cert] {
        &self.certs
    }

    pub fn is_empty(&self) -> bool {
        self.certs.is_empty()
    }
}

fn load_file(path: &Path, certs: &mut Vec<Cert>) {
    if !has_cert_extension(path) {
        return;
    }
    let Ok(parser) = CertParser::from_file(path) else {
        return;
    };
    certs.extend(parser.flatten());
}

fn has_cert_extension(path: &Path) -> bool {
    let Some(extension) = path.extension() else {
        return false;
    };
    CERT_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}
