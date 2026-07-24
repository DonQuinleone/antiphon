<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)"
            srcset="assets/banner-dark.svg">
    <img src="assets/banner-light.svg" width="480"
         alt="antiphon: a modern mail client for the terminal">
  </picture>
</p>

[![builds.sr.ht status](https://builds.sr.ht/~donquinleone/antiphon.svg)](https://builds.sr.ht/~donquinleone/antiphon)

[Source](https://git.sr.ht/~donquinleone/antiphon) |
[Mailing list](https://lists.sr.ht/~donquinleone/antiphon-devel) |
[Tracker](https://todo.sr.ht/~donquinleone/antiphon) |
[Builds](https://builds.sr.ht/~donquinleone/antiphon)

> Pre-alpha, built milestone by milestone against
> [DESIGN.md](DESIGN.md). Already working: `antiphon doctor`
> (setup preflight with `--init-store`), the themed client over
> a local notmuch store (list, pager, live search, vim-flavoured
> rebindable keys), and `antiphond` syncing a plain-auth IMAP
> account with client mutations landing in a crash-safe
> operation log. Not yet: composing, OAuth accounts, the vault,
> and everything else the design defers to later milestones.

Antiphon is a Rust TUI mail client built for people who live in
the terminal and refuse to choose between speed, security and
civilised e-mail. Local Maildir as the source of truth, indexed
by notmuch, synced by a separate daemon (antiphond) so the UI
never blocks on the network, and the whole store sealed inside a
platform-native encrypted vault at rest.

Planned for v1 (see [DESIGN.md](DESIGN.md) for the full scope):

- Multiple accounts with unified views: IMAP, Microsoft 365
  (OAuth2, Graph send) and Google Workspace (OAuth2)
- Full-text search over hundreds of thousands of messages,
  always scoped to the accounts in view
- Plaintext-first composing in your own editor, embedded in the
  client; format=flowed; per-identity signatures
- OpenPGP signing and encryption via Sequoia and gpg-agent,
  including smartcards
- Mailing lists done properly: reply-to-list, patch rendering,
  one-key unsubscribe
- Encrypted vault (LUKS2, encrypted APFS, or gocryptfs) with
  passphrase, Touch ID and YubiKey unlock
- Vim-flavoured, fully rebindable keys; themeable, with a
  gallery of familiar schemes

## Building

Requires stable Rust (MSRV 1.95).

```bash
git clone https://git.sr.ht/~donquinleone/antiphon
cd antiphon
cargo build --workspace
```

## Contributing

Development happens on SourceHut: patches go to the
[mailing list](https://lists.sr.ht/~donquinleone/antiphon-devel)
with `git send-email`, and bugs to the
[tracker](https://todo.sr.ht/~donquinleone/antiphon); see
[CONTRIBUTING.md](CONTRIBUTING.md) for the workflow. The
[GitHub repository](https://github.com/DonQuinleone/antiphon)
is a read-only mirror.

## Licence

GPL-3.0-or-later. See [COPYING](COPYING).
