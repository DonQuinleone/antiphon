<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)"
            srcset="assets/banner-dark.svg">
    <img src="assets/banner-light.svg" width="480"
         alt="antiphon: a modern mail client for the terminal">
  </picture>
</p>

> Pre-alpha. Nothing here is usable yet; the design is settled
> and the code is being built milestone by milestone. Read
> [DESIGN.md](DESIGN.md) for the full plan.

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
git clone https://github.com/DonQuinleone/antiphon
cd antiphon
cargo build --workspace
```

## Contributing

Development happens on
[GitHub](https://github.com/DonQuinleone/antiphon), where issues
and pull requests are welcome; see
[CONTRIBUTING.md](CONTRIBUTING.md) for the ground rules. The
repository is mirrored at
[git.sr.ht/~donquinleone/antiphon](https://git.sr.ht/~donquinleone/antiphon).

## Licence

GPL-3.0-or-later. See [COPYING](COPYING).
