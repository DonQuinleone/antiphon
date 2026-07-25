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

<img src="https://git.sr.ht/~donquinleone/antiphon/blob/master/assets/screenshots/themes.gif"
     alt="The message list cycling through all seventeen
          shipped themes: two accounts in the sidebar, unread
          counts, status markers and the reading pane"
     width="820">

Seventeen themes ship, and yours is one TOML file away; the
website shows [the rest of the interface](https://antiphon.net).

## Why Antiphon

Terminal mail today usually means picking a UI, then bolting on
your own sync, your own send queue, and often your own crypto,
one tool at a time. Antiphon is built the other way round: the
daemon, the store, the vault and the OpenPGP integration are one
project, tested and shipped together, so "does sending survive
a crash offline" and "is my mail encrypted at rest" are answered
once, for everyone, rather than depending on how you wired your
own pipeline.

**NeoMutt** is the deepest, most configurable terminal client
there is, with decades of muttrc wisdom behind it. Antiphon
trades that depth for a process split in two: antiphond owns
the network so the UI never blocks on it, and the store lives
inside an encrypted vault at rest, which NeoMutt (one process,
a plain on-disk Maildir, no vault) does not attempt.

**aerc** is the closest cousin: asynchronous IMAP and JMAP keep
its UI from locking up, and it is a genuinely pleasant terminal
client. It is also deliberately Unix-shaped: sending offline
means pairing it with `msmtpq` or a similar queue yourself, and
there is no vault either. Antiphon ships a durable outbox and
an encrypted store as part of the one system, so offline work
always replays and the store is never plaintext on disk.

**Thunderbird and other GUI clients** cover more protocols and
more calendars than Antiphon ever will, and are the right choice
if you want a mouse-driven, all-in-one mail app. They also run
outside the terminal, are considerably heavier, and search
without notmuch's tag-based, whole-corpus speed.

**alot and the notmuch-frontend pipeline** (offlineimap or
mbsync, notmuch, msmtp, a pile of hooks) is a well-worn, capable
setup, and notmuch's search is exactly what Antiphon is built
on too. The difference is coherence: three tools with three
failure modes and three config languages become one binary and
one daemon, with a single strict config and one operation log
responsible for getting mail there and back.

Across all four, the shape is the same: Antiphon is one
coherent local-first system, not an assembled pipeline. The UI
never touches the network. A durable operation log means
offline work replays instead of vanishing. The store is
encrypted at rest. OpenPGP goes through your existing gpg-agent,
keyring and smartcard rather than reinventing them. And
configuration is strict enough to fail at the first typo, named
by file and line, rather than three defaults deep into a
mystery.

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

## Installation

Antiphon is pre-1.0 and moving quickly. Expect rough edges,
and expect them to be fixed fast. The AUR release package,
the Homebrew tap and the installer's release path go live
with v1.0.0.

### One command (Linux and macOS)

```bash
curl -fsSL https://antiphon.net/install.sh | sh
```

This resolves the latest release, verifies its checksum, and
installs both binaries to `${XDG_BIN_HOME:-~/.local/bin}` on
Linux, or adds the Homebrew tap on macOS; it names any
missing runtime dependency rather than guessing at a package
manager.

### Arch Linux

```bash
yay -S antiphon        # released version, from v1.0.0
yay -S antiphon-git    # tracks master today
```

Building the PKGBUILD by hand works the same way it does for
any AUR package:

```bash
git clone https://aur.archlinux.org/antiphon-git.git
cd antiphon-git
makepkg -si
```

Either route pulls in `notmuch`, and installs the man pages
and the systemd user unit alongside the binaries; `gnupg` is
an optional dependency, needed only if you sign or decrypt
with OpenPGP.

### Nix

```bash
nix profile install \
    git+https://git.sr.ht/~donquinleone/antiphon
# or try it without installing:
nix run git+https://git.sr.ht/~donquinleone/antiphon
```

The flake ships the package (binaries, man pages, the systemd
user unit) and a dev shell (`nix develop`).

### Other Linux distributions

Grab the release tarball for your architecture from the
[refs page](https://git.sr.ht/~donquinleone/antiphon/refs)
(x86_64-linux-gnu and aarch64-linux-musl, each with a sha256
sidecar), verify, and drop both binaries on your PATH; or
build from source below. You need `notmuch` at runtime:

```bash
sudo apt install notmuch gnupg        # Debian, Ubuntu
sudo dnf install notmuch gnupg2       # Fedora
```

### macOS (Homebrew)

```bash
brew tap donquinleone/antiphon \
    https://git.sr.ht/~donquinleone/homebrew-antiphon
brew install antiphon
```

Resolves `notmuch` and Rust automatically; `gnupg` is a
caveat, not a hard dependency, for the same reason as above.

### Building from source

```bash
git clone https://git.sr.ht/~donquinleone/antiphon
cd antiphon
cargo install --path antiphon --locked
cargo install --path antiphond --locked
```

You need Rust (stable), `notmuch` and, for OpenPGP, `gnupg`:

```bash
sudo pacman -S notmuch gnupg          # Arch
sudo apt install notmuch gnupg        # Debian, Ubuntu
sudo dnf install notmuch gnupg2       # Fedora
brew install notmuch gnupg            # macOS
```

## Getting started

Three commands take you from nothing to reading mail:

```bash
antiphon setup      # account, secrets, vault, store, daemon
antiphon doctor     # confirm everything resolved cleanly
antiphon            # open the client
```

`setup` asks for your address and server details, stores your
secrets, creates the vault, initialises the store, and starts
the daemon. On macOS, secrets go into your Keychain; on Linux,
you provide a command that prints each secret, so any manager
works (`pass show mail/personal`, `secret-tool lookup ...`).
Antiphon never stores a password itself, only the command that
produces it. Press `?` at any time for the key cheatsheet.

## Documentation

Full documentation moves to
[docs.antiphon.net](https://docs.antiphon.net) with v1.0.0;
until then, it lives here in the repository:

| Guide | Covers |
| ----- | ------ |
| [User guide](https://docs.antiphon.net/guide/getting-started/) | getting started through security |
| [Configuration](https://docs.antiphon.net/customise/configuration/) | every key, strictly parsed |
| [Appearance](https://docs.antiphon.net/customise/appearance/) | seventeen themes and your own |
| [Sync and the daemon](https://docs.antiphon.net/guide/sync/) | supervisors on every platform |
| [Migrating from NeoMutt](https://docs.antiphon.net/guide/migrating/) | the concept map and the move |
| [Developer guide](https://docs.antiphon.net/develop/architecture/) | architecture, building, contributing |

Man pages for `antiphon(1)`, `antiphond(1)` and
`antiphon-sendmail(1)` are in [doc/](doc/); the AUR and
Homebrew packages install them alongside the binaries.

## Contributing

Development happens on SourceHut: patches go to the
[mailing list](https://lists.sr.ht/~donquinleone/antiphon-devel)
with `git send-email`, bugs to the
[tracker](https://todo.sr.ht/~donquinleone/antiphon). See
[CONTRIBUTING.md](CONTRIBUTING.md) for the workflow. The
[GitHub repository](https://github.com/DonQuinleone/antiphon)
is a read-only mirror.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
