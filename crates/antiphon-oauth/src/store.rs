use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use secrecy::ExposeSecret;
use serde::Serialize;

use crate::{OauthError, Provider, TokenSet};

const TOKEN_FILE_MODE: u32 = 0o600;
const STORE_DIR_MODE: u32 = 0o700;

pub struct TokenStore {
    dir: PathBuf,
}

#[derive(Serialize)]
struct StoredTokenSet<'a> {
    access_token: &'a str,
    refresh_token: &'a str,
    expires_at_unix: u64,
    scope: &'a str,
    client_id: &'a str,
    provider: Provider,
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant: Option<&'a str>,
}

impl TokenStore {
    pub fn open(
        dir: impl Into<PathBuf>,
    ) -> Result<TokenStore, OauthError> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(store_error)?;
        fs::set_permissions(
            &dir,
            Permissions::from_mode(STORE_DIR_MODE),
        )
        .map_err(store_error)?;
        Ok(TokenStore { dir })
    }

    pub fn save(
        &self,
        name: &str,
        tokens: &TokenSet,
    ) -> Result<(), OauthError> {
        validate_name(name)?;
        let stored = StoredTokenSet {
            access_token: tokens.access_token.expose_secret(),
            refresh_token: tokens.refresh_token.expose_secret(),
            expires_at_unix: tokens.expires_at_unix,
            scope: &tokens.scope,
            client_id: &tokens.client_id,
            provider: tokens.provider,
            tenant: tokens.tenant.as_deref(),
        };
        let json = serde_json::to_vec_pretty(&stored)
            .map_err(|error| OauthError::Store(error.to_string()))?;
        let temp_path = self.temp_path(name);
        write_new_file(&temp_path, &json).map_err(store_error)?;
        fs::rename(&temp_path, self.token_path(name))
            .map_err(store_error)?;
        File::open(&self.dir)
            .and_then(|dir| dir.sync_all())
            .map_err(store_error)
    }

    pub fn load(&self, name: &str) -> Result<TokenSet, OauthError> {
        validate_name(name)?;
        let raw = match fs::read_to_string(self.token_path(name)) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(OauthError::NoStoredToken(
                    name.to_string(),
                ));
            }
            Err(error) => return Err(store_error(error)),
        };
        serde_json::from_str(&raw).map_err(|error| {
            OauthError::Store(format!(
                "token file for {name} is unreadable: {error}"
            ))
        })
    }

    /// Deletes the named grant (and any half-written temp
    /// file); an absent grant is fine, so revoking twice is a
    /// no-op.
    pub fn remove(&self, name: &str) -> Result<(), OauthError> {
        validate_name(name)?;
        for path in [self.temp_path(name), self.token_path(name)] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(store_error(error)),
            }
        }
        File::open(&self.dir)
            .and_then(|dir| dir.sync_all())
            .map_err(store_error)
    }

    fn token_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    fn temp_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json.tmp"))
    }
}

fn write_new_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(TOKEN_FILE_MODE)
        .open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

fn validate_name(name: &str) -> Result<(), OauthError> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
        });
    if !plain {
        return Err(OauthError::BadGrantName(name.to_string()));
    }
    Ok(())
}

fn store_error(error: std::io::Error) -> OauthError {
    OauthError::Store(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use secrecy::{ExposeSecret, SecretString};

    use super::TokenStore;
    use crate::{OauthError, Provider, TokenSet};

    fn sample() -> TokenSet {
        TokenSet {
            access_token: SecretString::from("access-secret"),
            refresh_token: SecretString::from("refresh-secret"),
            expires_at_unix: 1_800_000_000,
            scope: "https://mail.google.com/".to_string(),
            client_id: "client-1".to_string(),
            provider: Provider::Google,
            tenant: None,
        }
    }

    #[test]
    fn save_is_atomic_and_private() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        store.save("personal", &sample()).expect("save");

        let token_path = dir.path().join("personal.json");
        let temp_path = dir.path().join("personal.json.tmp");
        assert!(token_path.exists());
        assert!(!temp_path.exists());

        let mode = fs::metadata(&token_path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);

        let raw =
            fs::read_to_string(&token_path).expect("read token file");
        assert!(raw.contains("\"access-secret\""));
        assert!(raw.contains("\"refresh-secret\""));
        assert!(raw.contains("\"google\""));
    }

    #[test]
    fn load_round_trips_a_saved_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        let saved = sample();
        store.save("work", &saved).expect("save");

        let loaded = store.load("work").expect("load");
        assert_eq!(
            loaded.access_token.expose_secret(),
            saved.access_token.expose_secret()
        );
        assert_eq!(
            loaded.refresh_token.expose_secret(),
            saved.refresh_token.expose_secret()
        );
        assert_eq!(loaded.expires_at_unix, saved.expires_at_unix);
        assert_eq!(loaded.scope, saved.scope);
        assert_eq!(loaded.client_id, saved.client_id);
        assert_eq!(loaded.provider, saved.provider);
    }

    #[test]
    fn save_overwrites_an_existing_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        store.save("acct", &sample()).expect("first save");
        let mut rotated = sample();
        rotated.refresh_token = SecretString::from("rotated-secret");
        store.save("acct", &rotated).expect("second save");

        let loaded = store.load("acct").expect("load");
        assert_eq!(
            loaded.refresh_token.expose_secret(),
            "rotated-secret"
        );
    }

    #[test]
    fn missing_grant_is_reported_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        let error = store.load("absent").expect_err("missing");
        assert!(matches!(
            error,
            OauthError::NoStoredToken(name) if name == "absent"
        ));
    }

    #[test]
    fn remove_deletes_only_the_named_grant() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        store.save("work-imap", &sample()).expect("save imap");
        store.save("work-graph", &sample()).expect("save graph");

        store.remove("work-imap").expect("remove");
        assert!(matches!(
            store.load("work-imap"),
            Err(OauthError::NoStoredToken(_))
        ));
        assert!(store.load("work-graph").is_ok());
    }

    #[test]
    fn removing_an_absent_grant_is_a_no_op() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        store.remove("never-stored").expect("absent is fine");
        store.remove("never-stored").expect("and idempotent");
    }

    #[test]
    fn remove_clears_a_stale_temp_file_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        store.save("acct", &sample()).expect("save");
        let temp = dir.path().join("acct.json.tmp");
        fs::write(&temp, b"half-written").expect("stale temp");

        store.remove("acct").expect("remove");
        assert!(!temp.exists());
        assert!(!dir.path().join("acct.json").exists());
    }

    #[test]
    fn remove_rejects_path_like_grant_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        let error = store.remove("../escape").expect_err("rejected");
        assert!(matches!(error, OauthError::BadGrantName(_)));
    }

    #[test]
    fn rejects_path_like_grant_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        let error =
            store.save("../escape", &sample()).expect_err("rejected");
        assert!(matches!(error, OauthError::BadGrantName(_)));
    }

    #[test]
    fn debug_output_never_leaks_secrets() {
        let printed = format!("{:?}", sample());
        assert!(!printed.contains("access-secret"));
        assert!(!printed.contains("refresh-secret"));
    }
}
