use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use antiphon_config::{Composer, Dirs, ReadingPane};
use antiphon_pgp::{Keyring, Signature, SignatureStatus};
use antiphon_store::MessageSummary;
use antiphon_ui::Theme;

use super::app::{App, DEFAULT_QUERY, View};
use super::commands::FrameStats;
use super::crypto::{ComposeCrypto, PgpPlan};
use super::identity::ComposeIdentity;
use super::scope::ViewScope;
use super::settings::{AccountSummary, SettingsState, SettingsTab};
use super::sidebar::{self, AccountEntry};

pub(super) fn app_with_messages(count: usize) -> App {
    let messages = (0..count)
        .map(|index| MessageSummary {
            id: format!("m{index}"),
            thread_id: String::new(),
            subject: String::new(),
            from: String::new(),
            to: String::new(),
            date_unix: index as i64,
            tags: Vec::new(),
            unread: index % 2 == 0,
            path: std::path::PathBuf::new(),
        })
        .collect();
    App {
        accounts: Vec::new(),
        scope: ViewScope::Unified,
        account_entries: Vec::new(),
        saved_searches: Vec::new(),
        sidebar_entries: Vec::new(),
        sidebar_selected: 0,
        active_search: None,
        messages,
        total_messages: count as u32,
        selected: 0,
        view: View::List,
        sync_progress: None,
        pager_body: String::new(),
        pager_patch: Vec::new(),
        pager_signature: Signature::none(),
        pager_invite: Vec::new(),
        pager_scroll: 0,
        pager_raw: Vec::new(),
        pager_html: false,
        pager_headers: Vec::new(),
        pager_headers_all: Vec::new(),
        pager_rendered: antiphon_render::RenderedBody::default(),
        pager_attachments: Vec::new(),
        pager_images: Vec::new(),
        inline_images: true,
        image_view: None,
        link_picker: None,
        folder_picker: None,
        account_form: None,
        folder_alias_edit: None,
        drawer_open: false,
        drawer_selected: 0,
        header_names: antiphon_config::Ui::default().headers,
        headers_all: false,
        preview_scroll: 0,
        preview_html: false,
        help: false,
        help_scroll: 0,
        key_bindings: antiphon_core::Keymap::default()
            .bindings()
            .iter()
            .map(|(action, text)| (text.clone(), action.to_string()))
            .collect(),
        preview: None,
        own_addresses: Vec::new(),
        archive_folders: Vec::new(),
        trash_folders: Vec::new(),
        folder_aliases: Vec::new(),
        contacts: Vec::new(),
        pending_rsvp: None,
        keyring: Keyring::default(),
        reading_pane: ReadingPane::Below,
        accounts_bar: antiphon_config::AccountsBar::Sidebar,
        sidebar: true,
        list_rows: antiphon_config::Ui::default().list_rows,
        sidebar_width: antiphon_config::Ui::default().sidebar_width,
        theme: Theme::vespers(),
        config_path: PathBuf::new(),
        dirs: Dirs {
            config: PathBuf::new(),
            state: PathBuf::new(),
            cache: PathBuf::new(),
            data: PathBuf::new(),
        },
        sync_interval_minutes: antiphon_config::Sync::default()
            .interval_minutes,
        sync_idle: false,
        settings: None,
        oauth_flow: None,
        auth_failures: Vec::new(),
        date_format: String::new(),
        notice: None,
        prompt: None,
        current_query: DEFAULT_QUERY.to_string(),
        pending_ops: Vec::new(),
        pending_template: None,
        pending_resume: None,
        pending_patches: None,
        pending_export: None,
        export_recipients: Vec::new(),
        pending_sign: None,
        pending_encrypt: None,
        pending_one_click: None,
        pending_unsub_post: None,
        thread_return: None,
        pending_unsubscribe: None,
        frame_stats: FrameStats::default(),
        composer: Composer::Embedded,
        compose: None,
        editor: None,
        editor_return: View::List,
        requery: false,
        read_only: false,
        quit: false,
    }
}

pub(super) fn app_with_accounts(names: &[&str]) -> App {
    app_with_folders(
        &names
            .iter()
            .map(|name| (*name, &[][..]))
            .collect::<Vec<_>>(),
    )
}

pub(super) fn app_with_folders(accounts: &[(&str, &[&str])]) -> App {
    let mut app = app_with_messages(1);
    app.accounts = accounts
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    let entries: Vec<AccountEntry> = accounts
        .iter()
        .map(|(name, folders)| AccountEntry {
            name: (*name).to_string(),
            folders: folders
                .iter()
                .map(|folder| (*folder).to_string())
                .collect(),
            ..AccountEntry::default()
        })
        .collect();
    app.sidebar_entries = sidebar::entries(&entries, &[]);
    app.account_entries = entries;
    app
}

pub(super) fn app_with_settings(accounts: &[&str]) -> App {
    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Accounts,
        accounts: accounts
            .iter()
            .map(|name| AccountSummary {
                name: (*name).to_string(),
                account_name: (*name).to_string(),
                address: format!("{name}@example.com"),
                host: format!("imap.{name}.example.com"),
                oauth: None,
            })
            .collect(),
        account_selected: 0,
        pending_delete: None,
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: None,
        folders: Vec::new(),
        folder_selected: 0,
    });
    app.view = View::Settings;
    app
}

pub(super) fn tester_identity() -> ComposeIdentity {
    ComposeIdentity {
        name: Some("Tester".to_string()),
        address: "tester@example.com".to_string(),
        signature: Some("Kind regards\n".to_string()),
        pgp_sign: false,
        pgp_key: None,
    }
}

pub(super) const TEST_USER_ID: &str =
    "Antiphon Test <antiphon-test@example.com>";
pub(super) const TEST_ADDRESS: &str = "antiphon-test@example.com";
pub(super) const BODY: &str = "A body line for the pager round trip.";

pub(super) const PLAIN: &str = concat!(
    "From: Antiphon Test <antiphon-test@example.com>\r\n",
    "To: Antiphon Test <antiphon-test@example.com>\r\n",
    "Subject: sealed\r\n",
    "MIME-Version: 1.0\r\n",
    "Content-Type: text/plain; charset=\"utf-8\"\r\n",
    "\r\n",
    "A body line for the pager round trip.\r\n",
);

pub(super) fn plan(sign: bool, encrypt: bool) -> ComposeCrypto {
    ComposeCrypto {
        plan: PgpPlan { sign, encrypt },
        key: None,
        address: TEST_ADDRESS.to_string(),
    }
}

pub(super) struct TempDir {
    pub(super) path: PathBuf,
}

impl TempDir {
    pub(super) fn new() -> TempDir {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name =
            format!("antiphon-crypto-{}-{nonce}", std::process::id());
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(super) struct EphemeralHome {
    dir: TempDir,
    pub(super) fingerprint: String,
}

impl EphemeralHome {
    pub(super) fn new() -> Option<EphemeralHome> {
        if !gpg_usable() {
            eprintln!(
                "SKIP: no usable gpg CLI; live gpg-agent \
                 test not run"
            );
            return None;
        }
        let dir = TempDir::new();
        restrict_permissions(&dir.path);
        let mut home = EphemeralHome {
            dir,
            fingerprint: String::new(),
        };
        home.gpg(&[
            "--quick-gen-key",
            TEST_USER_ID,
            "ed25519",
            "cert,sign",
            "never",
        ]);
        home.fingerprint = home.primary_fingerprint();
        let fingerprint = home.fingerprint.clone();
        home.gpg(&[
            "--quick-add-key",
            &fingerprint,
            "cv25519",
            "encr",
            "never",
        ]);
        Some(home)
    }

    pub(super) fn path(&self) -> &Path {
        &self.dir.path
    }

    fn gpg(&self, args: &[&str]) -> Vec<u8> {
        let output = Command::new("gpg")
            .arg("--homedir")
            .arg(self.path())
            .args([
                "--batch",
                "--pinentry-mode",
                "loopback",
                "--passphrase",
                "",
            ])
            .args(args)
            .output()
            .expect("running gpg");
        assert!(
            output.status.success(),
            "gpg {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn primary_fingerprint(&self) -> String {
        let listing = self.gpg(&["--list-keys", "--with-colons"]);
        let listing = String::from_utf8_lossy(&listing);
        listing
            .lines()
            .find(|line| line.starts_with("fpr:"))
            .and_then(|line| line.split(':').nth(9))
            .expect("a fingerprint in the gpg listing")
            .to_string()
    }

    pub(super) fn keyring(&self) -> (TempDir, Keyring) {
        let exported = self.gpg(&["--export"]);
        let dir = TempDir::new();
        std::fs::write(dir.path.join("test.pgp"), exported).unwrap();
        let keyring = Keyring::from_dir(&dir.path);
        (dir, keyring)
    }
}

impl Drop for EphemeralHome {
    fn drop(&mut self) {
        let _ = Command::new("gpgconf")
            .arg("--homedir")
            .arg(&self.dir.path)
            .args(["--kill", "all"])
            .status();
    }
}

fn gpg_usable() -> bool {
    let gpg = Command::new("gpg").arg("--version").output();
    let gpgconf = Command::new("gpgconf").arg("--version").output();
    matches!(gpg, Ok(out) if out.status.success())
        && matches!(gpgconf, Ok(out) if out.status.success())
}

fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(0o700),
    )
    .expect("restricting GNUPGHOME permissions");
}

pub(super) fn assert_good_signature(
    signature: &antiphon_pgp::Signature,
    context: &str,
) {
    let SignatureStatus::Good { signer, .. } = &signature.status else {
        panic!("{context}: expected Good, got other");
    };
    assert_eq!(signer, TEST_USER_ID, "{context}");
}
