# The vault

Antiphon keeps everything sensitive, the Maildir, the notmuch
index, OAuth tokens and account state, inside an encrypted
vault. The guarantee is simple: **sealed at rest, open in
session, ciphertext only off the machine**. When the vault is
locked, nothing but ciphertext exists on disk; a backup tool
(restic, rclone, a cloud sync) copying the store copies
ciphertext and never sees your mail.

Antiphon contains no cryptography of its own. It orchestrates
the platform's own tools to create, unlock and lock the vault,
and enforces when to lock. The keys, ciphers and on-disk format
belong to the underlying tool, audited software in wide use,
not to anything Antiphon invented.

## Where it sits

The store lives at `$XDG_DATA_HOME/antiphon/store`. The vault
mounts its decrypted view at exactly that path, so nothing else
in Antiphon needs to know whether a vault is present: the store
is at the same place either way. Until you set one up, that
path is an ordinary directory; `antiphon doctor` tells you
which it is.

## Backends

Chosen in `config.toml` under `[vault] backend`; `auto` picks
the right one for the platform.

| Backend     | Platform        | Notes                        |
| ----------- | --------------- | ---------------------------- |
| `apfs`      | macOS           | Encrypted APFS sparse image  |
| `luks2`     | Linux           | LUKS2 container, dm-crypt     |
| `gocryptfs` | any (rootless)  | FUSE, needs no admin rights  |

- **apfs** stores the vault as an encrypted sparse image
  (`hdiutil`, AES-256), mounted with the system's own disk
  machinery. No administrator rights, no third-party kernel
  extension.
- **luks2** is the Linux-native choice: a LUKS2 container driven
  by `cryptsetup`, ext4 inside. Opening and mounting need root,
  so the daemon runs those steps through a narrow, documented
  `sudo` allowance rather than prompting.
- **gocryptfs** is the portable reserve: a FUSE filesystem that
  works anywhere without administrator rights.

## Unlocking

The passphrase is the floor: every vault has one and it always
works. The daemon unlocks headlessly using `passphrase_cmd`, a
command that prints the passphrase, referenced the same way
account passwords are and never stored in config.

```toml
[vault]
backend = "auto"
passphrase_cmd = "pass show antiphon/vault"
idle_lock_minutes = 0        # 0 = open until logout or suspend
unlock = ["touchid", "yubikey", "passphrase"]
```

The `unlock` list governs the interactive fast paths as the
hardware backends land; `touchid` and `yubikey` are declared
but not yet wired. However the vault opens, the secret reaches
the underlying tool over a private channel (standard input or a
controlled helper), never on a command line another process
could read.

## Locking

The vault opens once, when the daemon starts, and stays open so
mail keeps arriving while the client is closed. antiphond seals
it again on a clean stop (SIGINT or SIGTERM), so a graceful
shutdown leaves ciphertext at rest rather than an open mount.

## Setting it up

```bash
# set backend and passphrase_cmd in [vault] first, then:
antiphon vault create          # create and mount the vault
antiphon doctor --init-store   # lay out the store inside it
antiphond &                    # unlocks on start, seals on stop
antiphon
```

`antiphon vault create` refuses to shadow an existing plain
store, so migrate an old unencrypted store by moving it aside,
creating the vault, and copying the mail back in while it is
mounted. `antiphon doctor` reports the vault's state (open,
sealed or absent) and every resolved path. FIDO2 and PGP
smartcard keyslot enrolment is named in the design and awaits
hardware testing.
