# Configuration reference

This documents the configuration Antiphon understands today;
keys arrive here in the same commit that makes them real.
Everything lives under `$XDG_CONFIG_HOME/antiphon` (defaulting
to `~/.config/antiphon` on Linux and macOS alike):

    config.toml          global settings
    accounts/*.toml      one file per account
    local.toml           per-machine overrides, loaded last
    signatures/<name>    signature files referenced by name
    templates/<name>     compose templates referenced by name

Templates named `new` and `reply`, when present, shape fresh
composes and replies automatically; `:template <name>` opens a
fresh compose from any named template. Tokens `{from}`,
`{name}`, `{date}` and `{quoted}` expand; unknown braces pass
through.

Parsing is strict: an unknown key fails with the file, the
line, and the nearest valid key. Empty or relative XDG
variables are ignored per the specification. No secrets belong
in any of these files; passwords come from `password_cmd`.

`local.toml` holds the same keys as `config.toml` and wins
where both set a value; defects in it are reported against
`local.toml`.

## config.toml

```toml
[ui]
theme = "vespers"          # or a gallery name, see below
reading_pane = "below"     # below | right | off
date_format = "%d %b %H:%M"  # chrono strftime
composer = "embedded"      # embedded terminal pane for the
                           # editor, or "suspend" to hand the
                           # whole screen over

[vault]
backend = "auto"           # auto | luks2 | apfs | gocryptfs
passphrase_cmd = "pass show antiphon/vault"  # unlock secret
idle_lock_minutes = 0      # seal after N client-less
                           # minutes; 0 = never
unlock = ["touchid", "yubikey", "passphrase"]

[sync]
interval_minutes = 5       # periodic daemon sync; 0 disables

[daemon]
autostart = true           # client starts antiphond if absent

[notifications]
enabled = true

[keys]
# action = "key sequence"; every action is rebindable
half-page-down = "ctrl-d"
sync = ",s"

[[saved_searches]]
name = "unread"
query = "tag:unread"
```

Gallery themes: `vespers` (default), `kanagawa-wave`,
`catppuccin-mocha`, `gruvbox-dark`, `tokyo-night`, `nord`,
`rose-pine`. Truecolor terminal required.

The vault seals the store at rest (see [VAULT.md](VAULT.md)):
`antiphon vault create` sets it up, `passphrase_cmd` supplies
the unlock secret to antiphond, and an absent vault leaves the
store an ordinary directory. `touchid` and `yubikey` in
`unlock` are declared but not yet wired.

### Key sequences

Single keys (`j`, `/`, `G`), modifiers (`ctrl-d`, `alt-v`),
named keys (`enter`, `esc`, `tab`, `space`, `up`, `down`), and
two-key sequences (`gg`, `,s`). Unknown action names and
unparseable sequences fail at startup naming the entry.

Actions: `move-down`, `move-up`, `top`, `bottom`,
`half-page-down`, `half-page-up`, `open`, `back`, `quit`,
`search`, `command`, `next-account`, `previous-account`,
`sidebar-next`, `sidebar-previous`, `sidebar-open`,
`toggle-sidebar`, `cycle-reading-pane`, `sync`, `reply`,
`compose`, `mark-read`, `mark-unread`, `toggle-flagged`,
`delete-message`.

`next-account` (`gt`) and `previous-account` (`gT`) cycle the
view scope: unified, then each account in turn. `sidebar-next`
(`ctrl-n`) and `sidebar-previous` (`ctrl-p`) move the sidebar
highlight over the unified view, the accounts, three built-in
searches (`inbox`, `unread`, `flagged`) and the
`[[saved_searches]]` from config, in that order;
`sidebar-open` (`ctrl-o`) applies the highlighted entry.
Account entries set the scope; saved searches run their query
inside the current scope.

## accounts/*.toml

```toml
[account]
name = "personal"
maildir = "personal"       # parsed but not yet honoured; the
                           # store folder is the account name

[imap]
host = "imap.example.com"
port = 993                 # optional, defaults to 993
user = "you@example.com"
password_cmd = "pass show mail/personal"

[smtp]
host = "smtp.example.com"
port = 587                 # optional
user = "you@example.com"   # optional
password_cmd = "..."       # optional

[[identity]]
address = "you@example.com"
name = "Your Name"         # optional display name
signature = "personal"     # optional; a file in signatures/
match = [
    "you@example.com",     # literal
    "you+*@example.com",   # plus-addressing
    "*@you.example.com",   # catch-all domain
]
pgp_sign = false           # sign mail from this identity
# optional; the full fingerprint (gpg --fingerprint shows it),
# otherwise the key is picked by the identity address
pgp_key = "8F0EA48BF8BE9D3B9E1B2B9C6E5F0D3A1C2B4D5E"

[[rules]]
match_list = "~lists/somewhere"   # or match_sender
move_to = "lists/somewhere"       # or tag

[oauth]                    # M8; parsed today, unused yet
provider = "google"        # google | microsoft
client_id = "..."          # optional

[graph]                    # M8; parsed today, unused yet
send = false
```

Identity `match` patterns allow one `*`, only in the local
part; `antiphon doctor` validates every pattern and names any
bad one. On reply, the most specific match wins (literal, then
plus-pattern, then catch-all) and the From is the delivered
address verbatim.

## OpenPGP at compose time

`pgp_sign` sets the default for mail sent from that identity;
signing is off unless enabled. Before composing, `:sign`,
`:nosign`, `:encrypt` and `:noencrypt` override the default
for the next message only, and the compose statusline shows
the resulting plan (`[sign]`, `[sign+encrypt]`). Signing goes
through gpg-agent, so pinentry and smartcards work as they do
for gpg; the signer is the `pgp_key` fingerprint or, absent
that, the agent key whose user ID carries the identity
address.

Encryption requires a cert for every To and Cc address in the
trusted keyring (`.asc`/`.pgp` files under the config
directory's `pgp/`; see [PGP.md](PGP.md)). If a cert is missing, signing fails, or
the agent refuses, nothing is sent: the message stays in
`store/drafts/` and the statusline names the problem. Received
`multipart/encrypted` mail is decrypted through gpg-agent when
opened, and any signature inside is verified against the same
keyring.

## Machine state and data

State (nothing precious) goes to `$XDG_STATE_HOME/antiphon`,
caches to `$XDG_CACHE_HOME/antiphon`, and the message store,
index, operation log, outbox and sync state live under
`$XDG_DATA_HOME/antiphon/store`, the path the vault mounts
over from M6. `antiphon doctor` reports every resolved path
and `--init-store` creates or repairs the store.
