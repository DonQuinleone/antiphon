# Migrating from NeoMutt (or Weremutt)

Antiphon has no config importer by design: a muttrc encodes
years of workarounds for problems Antiphon does not have. This
guide maps the concepts so the move is an afternoon, not a
project.

## The mental model shift

NeoMutt talks to your IMAP server (or reads a Maildir someone
else syncs) and does everything in one process. Antiphon
splits that: `antiphond` owns the network and syncs every
account into one local store, indexed by notmuch, sealed in an
encrypted vault at rest; the `antiphon` client only ever reads
the local store, so nothing in the UI waits on a server.

Practical consequences:

- No `$folder`, `$spoolfile` or mailboxes list: every folder
  of every account is synced and appears in the sidebar; the
  unified view merges accounts.
- No `$imap_user`/`$imap_pass`: accounts live one-per-file in
  `accounts/*.toml`, and passwords come from `password_cmd`
  (the same `pass show ...` command your muttrc probably
  already calls).
- Search is notmuch query syntax everywhere, always scoped to
  the accounts in view; virtual folders map to
  `[[saved_searches]]`.

## Concept map

| NeoMutt                       | Antiphon                        |
| ----------------------------- | ------------------------------- |
| `~/.muttrc` / `neomuttrc`     | `config.toml` + `accounts/*.toml` |
| account hooks (`folder-hook`) | one file per account            |
| `$imap_pass` / `$smtp_pass`   | `password_cmd` per account      |
| mbsync/offlineimap + notmuch  | antiphond (built in)            |
| `$sendmail` / msmtp           | `[smtp]` per account; daemon sends from a durable outbox |
| `$record` (Sent copy)         | automatic sent-copy filing      |
| `$editor`                     | `$EDITOR` in an embedded pane   |
| `send-hook`/`reply-hook` identity tricks | `[[identity]]` with `match` patterns |
| `$pgp_default_key`, crypt hooks | `pgp_sign` + `pgp_key` per identity |
| sidebar patch                 | built in                        |
| `color` commands              | a theme (`vespers` default, gallery available) |
| `macro`/`bind`                | `[keys]` table, vim-flavoured defaults |
| virtual-mailboxes (notmuch)   | `[[saved_searches]]`            |

## The move, step by step

1. Build and check the ground: `antiphon doctor` names every
   missing piece (notmuch, gpg, `$EDITOR`, config).
2. Write one `accounts/<name>.toml` per account: host, user,
   `password_cmd`, identities with `match` patterns for your
   aliases. Strict parsing means a typo fails loudly with the
   file and line, so iterate with `antiphon doctor`.
3. Set up the vault (recommended; see
   [VAULT.md](VAULT.md)): configure `[vault] passphrase_cmd`,
   then `antiphon vault create` and
   `antiphon doctor --init-store`.
4. Start `antiphon`: it launches the daemon, which performs
   the initial sync. A large mailbox takes a while on the
   first pass; the client is usable as mail lands.
5. Port your PGP trust: export the correspondents you verify
   into the keyring directory (see [PGP.md](PGP.md)):
   `gpg --armor --export alice@example.com >
   ~/.config/antiphon/pgp/alice.asc`.
6. Recreate your virtual folders as `[[saved_searches]]` and
   any list filing as `[[rules]]` entries.
7. Run both clients side by side for a week; the store is
   yours (plain Maildir + notmuch under the mount), so nothing
   locks you in. Then delete the muttrc.

## What does not carry over

- HTML composing: Antiphon is plaintext-first, permanently.
- Local folders outside the synced accounts: import them into
  a folder of an account (copy the Maildir files in while the
  vault is mounted, then let the daemon index and upload).
- Scoring, limit-pattern muscle memory: use notmuch queries
  and saved searches instead.
