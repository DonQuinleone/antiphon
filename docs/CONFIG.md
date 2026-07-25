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
date_format = "%Y-%m-%d %H:%M"  # chrono strftime
composer = "embedded"      # embedded terminal pane for the
                           # editor, or "suspend" to hand the
                           # whole screen over
list_rows = 7              # message rows when the reading
                           # pane is below; with "right" or
                           # "off" the list fills the height
                           # and this key does not apply
sidebar_width = 16         # columns, clamped to 10-40
headers = ["from", "to", "date", "subject"]
                           # headers the pager and reading
                           # pane show, in this order; any
                           # RFC 5322 name is legal, matched
                           # case-insensitively (x-mailer,
                           # message-id, ...)

[vault]
backend = "auto"           # auto | luks2 | apfs | gocryptfs
passphrase_cmd = "pass show antiphon/vault"  # unlock secret
idle_lock_minutes = 0      # seal after N client-less
                           # minutes; 0 = never
unlock = ["touchid", "yubikey", "passphrase"]

[sync]
interval_minutes = 2       # periodic daemon sync; 0 disables
idle = false               # IMAP IDLE push on each INBOX

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
`catppuccin-mocha`, `gruvbox-dark`, `gruvbox-light`,
`tokyo-night`, `nord`, `rose-pine`, `dracula`,
`solarized-dark`, `solarized-light`, `catppuccin-latte`,
`one-dark`, `everforest-dark`, `ayu-dark`, `github-dark`,
`monokai`. Truecolor terminal required.

`:theme <name>` switches the running client to a theme at
once and saves it to `theme` under `[ui]` in `config.toml`,
editing that one line (or adding it) without touching
anything else in the file; a bare `:theme` lists the names,
and an unknown name leaves the theme untouched and lists
them too.

Themes are plain files, one per theme, and yours load from
`$XDG_CONFIG_HOME/antiphon/themes/*.toml`: nineteen `#rrggbb`
colour roles plus a `name`, in exactly the format the shipped
gallery uses (crates/antiphon-ui/themes/ in the source tree
is the reference). A user file whose `name` matches a shipped
theme replaces it, and a defective file fails startup naming
the file and the key, like every other config error.

In the message list the rendered `date_format` splits at its
last space: the left part wears the theme's date colour, the
right its time colour, whatever strftime pattern you set; a
single-token format is coloured entirely as a date.

The vault seals the store at rest (see [VAULT.md](VAULT.md)):
`antiphon vault create` sets it up, `passphrase_cmd` supplies
the unlock secret to antiphond, and an absent vault leaves the
store an ordinary directory. `unlock` states a preference
order; of the three methods, `passphrase` is the one that
works today, so list it last and the others are tried first
once they exist.

### Key sequences

Single keys (`j`, `/`, `G`), modifiers (`ctrl-d`, `alt-v`),
named keys (`enter`, `esc`, `tab`, `space`, `up`, `down`), and
two-key sequences (`gg`, `,s`). Unknown action names and
unparseable sequences fail at startup naming the entry.

Movement actions take a count prefix, vi style: `4j` moves
down four, `12k` up twelve. Digits accumulate until a bound
key consumes them (capped at 9999); `esc` clears a pending
count.

Actions: `move-down`, `move-up`, `top`, `bottom`,
`half-page-down`, `half-page-up`, `open`, `back`, `quit`,
`search`, `command`, `next-account`, `previous-account`,
`sidebar-next`, `sidebar-previous`, `sidebar-open`,
`toggle-sidebar`, `cycle-reading-pane`, `sync`, `reply`,
`reply-list`, `compose`, `mark-read`, `mark-unread`,
`toggle-flagged`, `delete-message`, `mark-all-read` (`,r`:
marks every unread message the current listing's query
covers, folder or search, read in one go, not only the rows
on screen), `toggle-html` (`h`:
flip the open or previewed message between plain and html
parts), `toggle-headers` (`t`: flip the pager and reading
pane between the configured `headers` set and every header
of the message), `open-link` (`o`: a numbered picker over
the links of the open message; type the number and press
enter, or move with `j`/`k`, and the url goes to the system
opener, `open` on macOS and `xdg-open` elsewhere; `esc`
closes), `attachments` (`v`: expand the attachment drawer),
`archive` (`a`: move the message to the account's archive
folder, from the list or the pager; the row leaves at once
and the daemon replays the move against the server),
`move-to` (`c`: a picker over the account's other folders,
aliases shown where configured; `enter` moves, `esc` closes;
`:move <folder>` does the same by name, accepting an alias,
the real path, or `inbox` for the account root),
`pane-down`/`pane-up` (`J`/`K`: scroll the reading pane),
`thread-view` (`T`: pivot the flat list onto the selected
message's whole thread; `back` (`esc`) restores the listing
you came from; a `T` in the status column marks messages
whose thread has more rows in the current listing),
`help` (`?`: the keybinding cheatsheet, generated live from
these bindings with your overrides applied).

In the pager, link spans render underlined in the accent
colour. The mouse works there too: the wheel scrolls, and a
left click on a link opens it. Only `http`, `https` and
`mailto` urls are ever handed to the opener, and Antiphon
itself never fetches anything.

A message with attachments shows a one-line drawer at the
pager's bottom, e.g. `2 attachments: report.pdf, photo.jpg`,
truncated to the width. `v` expands it into a list: `j`/`k`
select, `s` saves the decoded bytes to a prompted path
(prefilled with the sent filename, `~` expands), `v` writes
a temporary copy and hands it to the system opener, and
`esc` collapses the drawer again.

`next-account` (`gt`) and `previous-account` (`gT`) cycle the
view scope: unified, then each account in turn. `sidebar-next`
(`ctrl-n`) and `sidebar-previous` (`ctrl-p`) move the sidebar
highlight over the unified view, the accounts (each with its
folders nested beneath it), four built-in searches (`all`,
`inbox`, `unread`, `flagged`) and the `[[saved_searches]]`
from config, in that order; `sidebar-open` (`ctrl-o`) applies
the highlighted entry. Account entries set the scope; saved
searches run their query inside the current scope. On startup
the `all` search is selected, so everything in scope is
listed.

Folder entries list one folder of one account: `inbox` is the
account's root maildir, and every other folder is discovered
from the store's maildir tree on each refresh, nested IMAP
folders included (`lists/aerc`, `inbox/accounts`). Opening a
folder scopes the view to its account and shows only that
folder's messages. A folder holding unread mail steps out of
the muted rank and carries its unread count beside the name,
refreshed with the rest of the sidebar every couple of
seconds.

`[folder_names]` gives folders friendlier sidebar names
(`INBOX/Accounts` shown as `accounts`); the alias also works
wherever a folder name is typed. The left side is the folder
path as the store knows it: lowercase, `/`-separated.

`reply-list` (`L`) replies to the mailing list a message came
from: `Mail-Followup-To` wins when the author set one,
otherwise the `List-Post` mailto address is used. A
`List-Post: NO` list refuses with a status message, and a list
without any `List-Post` header falls back to reply-all with a
warning naming the recipient count rather than guessing an
address.

`:accept`, `:tentative` and `:decline` answer the calendar
invite of the open message: each opens an ordinary compose to
the organiser with the RFC 5546 reply attached as a calendar
part, so nothing is sent before the review screen's y. The
organiser's copy updates your attendance when their client
processes the part, which the big providers all do.

`:unsubscribe` acts on the current message's
`List-Unsubscribe` header. An RFC 8058 one-click entry asks
for confirmation naming the list, then hands the POST to
antiphond, which requires https and performs it off the serve
loop (the client never touches the network). A mailto
entry opens a
compose prefilled with the address, subject and body from the
URI. A web-only entry displays the URL for you to open;
nothing is ever fetched automatically.

## accounts/*.toml

```toml
[account]
name = "personal"
maildir = "personal"       # reserved; the store folder is
                           # the account name for now
archive = "archive"        # where `a` files mail, as the
                           # folder appears in the sidebar;
                           # this is also the default

[folder_names]             # sidebar aliases; typed folder
                           # names accept either side
"inbox/accounts" = "accounts"

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

[oauth]
provider = "google"        # google | microsoft
client_id = "..."          # optional

[graph]
send = false               # send via Microsoft Graph rather
                           # than SMTP
```

Identity `match` patterns allow one `*`, only in the local
part; `antiphon doctor` validates every pattern and names any
bad one. On reply, the most specific match wins (literal, then
plus-pattern, then catch-all) and the From is the delivered
address verbatim.

## Composing

A compose (`compose`, `reply`, `reply-list`, `:template`,
`:unsubscribe`) opens a fields stage, aerc style: To, Cc, Bcc
and Subject are structured fields edited in place, and From
cycles through every configured identity with `left`/`right`
or `space` rather than taking free text; replies preselect the
identity the resolution engine picked. `tab`/`shift-tab` move
between fields, `enter` advances and, on From, opens `$EDITOR`
on the body alone; headers never pass through the editor.
`ctrl-e` (or `ctrl-h`) toggles between the fields and the
running editor, both ways, so one chord covers the round
trip. `esc` backs out of the fields stage (to the review
screen once one exists, else out of the compose). The stage's
keys are always spelt out in the status line.

Leaving the editor lands on a review screen, Mutt style,
showing the recipients, subject, seal plan, attachments and
the first body lines; nothing is sent until confirmed there.
Its keys:

    y    send (seal per the plan, queue for antiphond)
    e    edit the body again (ctrl-e works too)
    h    edit the header fields again (ctrl-h works too)
    a    add an attachment (path prompt, ~ expands)
    d    remove the selected attachment
    j/k  move the attachment selection
    s    toggle signing for this message
    x    toggle encryption for this message
    q    close, asking first whether to save a draft
    ^c   stays on review; nothing is discarded

Attachments are read in full when added, so a bad path fails
at the prompt (named error, then re-asked) rather than at
send time. The message ships as multipart/mixed with base64
parts; the content type is inferred from the file extension,
falling back to application/octet-stream, and signing or
encryption wraps the whole multipart per RFC 3156.

`q` writes the compose, fields, plan and attachment paths
included, to a file under `store/drafts/` and names it in the
statusline; `:resume <path>` reopens it on the fields stage
exactly as saved (a vanished attachment file is reported and
dropped).

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
directory's `pgp/`; see [PGP.md](PGP.md)). If a cert is
missing, signing fails, or the agent refuses, nothing is
sent: the compose stays on the review screen and the
statusline names the problem. Received
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
