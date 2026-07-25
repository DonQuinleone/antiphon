<p align="center">
  <img src="https://git.sr.ht/~donquinleone/antiphon/blob/master/assets/banner.svg"
       width="480"
       alt="antiphon: a modern mail client for the terminal">
</p>

<p align="center">
  <a href="https://builds.sr.ht/~donquinleone/antiphon"><img
    src="https://builds.sr.ht/~donquinleone/antiphon.svg"
    alt="build status"></a>
</p>

Antiphon is a mail client for people who live in the terminal
and refuse to choose between speed, security and civilised
e-mail.

Your mail lives in a local Maildir, indexed by notmuch, so
search across a hundred thousand messages returns before you
finish blinking. A separate daemon does all the talking to
your servers, so the interface never stutters waiting on a
network. The entire store sits inside an encrypted vault that
seals when you walk away, and OpenPGP runs through Sequoia
and your own gpg-agent, keyring and smartcard included.

## A look around

<img src="https://git.sr.ht/~donquinleone/antiphon/blob/master/assets/screenshots/list.png"
     alt="The message list: sidebar with unread counts, status
          markers, threading marks and the reading pane"
     width="820">

<img src="https://git.sr.ht/~donquinleone/antiphon/blob/master/assets/screenshots/pager.png"
     alt="Reading a message: generated keybar, headers, and
          the attachment drawer expanded"
     width="820">

<img src="https://git.sr.ht/~donquinleone/antiphon/blob/master/assets/screenshots/compose.png"
     alt="Composing: structured header fields with contact
          completion in a popover"
     width="820">

## Highlights

- **Local first.** Maildir plus notmuch full-text search,
  always scoped to the accounts in view. Instant everything,
  even offline; flags, moves and sends queue durably and
  replay when the network returns.
- **Sealed at rest.** The store, index, tokens and state live
  inside an encrypted vault (encrypted APFS on macOS, LUKS2 or
  gocryptfs on Linux). Back it up with anything; the copy is
  ciphertext.
- **A daemon that behaves.** antiphond syncs on a timer, sends
  from a crash-safe outbox, files your sent mail, uploads
  drafts, applies your filing rules and posts desktop
  notifications. It answers instantly even mid-sync, and it
  runs under launchd, systemd, dinit, runit or plain
  `antiphon` launch.
- **OpenPGP without ceremony.** Verification against a keyring
  you curate; signing and decryption through gpg-agent, so
  pinentry and your smartcard work exactly as they do
  everywhere else. Sign-inside-encrypt, per identity or per
  message.
- **Patches are first-class.** Unified diffs render
  highlighted, a thread saves as a series `git am`
  understands, and `antiphon sendmail` slots straight into
  `git send-email`.
- **Composing that respects you.** Header fields first, your
  own `$EDITOR` for the body, a review screen before anything
  leaves, attachments included. Plaintext always;
  format=flowed; no HTML composing, ever.
- **Your keys, your colours.** Vim-flavoured and fully
  rebindable (`?` shows the live cheatsheet), themed with a
  truecolor palette and a gallery of familiar schemes.

## Getting started

Antiphon is pre-1.0 and moving quickly. Expect rough edges,
and expect them to be fixed fast.

You need Rust (stable), `notmuch` and `gnupg`:

```bash
brew install notmuch gnupg            # macOS
```

```bash
sudo apt install notmuch gnupg        # Debian, Ubuntu
```

```bash
sudo dnf install notmuch gnupg2       # Fedora
```

```bash
sudo pacman -S notmuch gnupg          # Arch
```

Build and install both binaries:

```bash
git clone https://git.sr.ht/~donquinleone/antiphon
cd antiphon
cargo install --path antiphon --locked
cargo install --path antiphond --locked
```

Then let the wizard do the rest:

```bash
antiphon setup
```

It asks for your address and server details, stores your
secrets, creates the vault, and starts the daemon. On macOS,
secrets go into your Keychain; on Linux, you provide a
command that prints each secret, so any manager works:

```
pass show mail/personal
```

```
secret-tool lookup service mail account personal
```

Antiphon never stores a password itself, only the command
that produces it. When setup finishes, run `antiphon` and
watch your mailbox fill. Press `?` at any time for the key
cheatsheet.

## Documentation

| Guide | Covers |
| ----- | ------ |
| [Configuration](docs/CONFIG.md) | every key, strictly parsed |
| [The vault](docs/VAULT.md) | encryption at rest, unlocking, locking |
| [OpenPGP](docs/PGP.md) | verification, signing, encryption |
| [Running the daemon](docs/DAEMON.md) | launchd, systemd, dinit, runit, autostart |
| [Patches](docs/PATCHES.md) | reading, applying and sending patch series |
| [Migrating from NeoMutt](docs/MIGRATING.md) | the concept map and the move |

## Contributing

Development happens on SourceHut: patches go to the
[mailing list](https://lists.sr.ht/~donquinleone/antiphon-devel)
with `git send-email`, bugs to the
[tracker](https://todo.sr.ht/~donquinleone/antiphon). See
[CONTRIBUTING.md](CONTRIBUTING.md) for the workflow. The
[GitHub repository](https://github.com/DonQuinleone/antiphon)
is a read-only mirror.

## Licence

GPL-3.0-or-later. See [COPYING](COPYING).
