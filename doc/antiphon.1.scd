antiphon(1)

# NAME

antiphon - a modern mail client for the terminal

# SYNOPSIS

*antiphon* [_command_]

# DESCRIPTION

Antiphon is a terminal mail client backed by a local Maildir
indexed by notmuch. It never talks to a mail server itself;
*antiphond*(1) does that work in the background, so the
interface stays responsive whatever the network is doing.

Run with no arguments to open the client. If no daemon is
reachable, antiphon starts one before opening (see
*antiphond*(1) and the *daemon.autostart* key). If no message
store exists yet, antiphon reports the missing path and exits;
run *antiphon doctor --init-store* to create one, or *antiphon
setup* for a guided first run.

Once open, the interface is a sidebar, a message list and a
reading pane, entirely keyboard driven and fully rebindable.
Press *?* at any time for the live keybinding cheatsheet. The
full key reference, theme gallery and every configuration key
live in the *CONFIG.md* guide bundled with the project (see
FILES).

# COMMANDS

*doctor* [--init-store]
	Check the local setup: configuration, accounts, the
	message store, the vault, the daemon, and the notmuch and
	gpg tools. Prints one line per check and exits non-zero if
	any check fails. *--init-store* creates the message store
	first, before running the checks.

*vault create*
	Create and unlock the encrypted vault at the configured
	store path, using *[vault] passphrase_cmd* for the
	passphrase. Fails if a store or vault already exists there.

*oauth login* _account_
	Run the configured provider's sign-in flow for _account_
	(an account name from *accounts/\*.toml*) and store the
	resulting grants. Requires an *[oauth]* table with a
	*client_id* on that account, unless the matching
	environment variable overrides it:
	*ANTIPHON_MS_CLIENT_ID* for Microsoft accounts,
	*ANTIPHON_GOOGLE_CLIENT_ID* for Google accounts.

*oauth status* _account_
	Show the grants stored for _account_ and how long each
	access token has left before it needs a refresh.

*export* [_account_] *-o* _path_ [*-r* _recipient_]... [*-p*]
	Export _account_'s Maildir, or every configured account's
	when _account_ is omitted, as a tar.gz archive encrypted
	with age. Each archive lands under the *-o*/*--output*
	directory as _name-YYYY-MM-DD.tar.gz.age_ (UTC date); with
	a single account, an *--output* that is not an existing
	directory names the archive file itself. Encrypt to one or
	more age public keys with *-r*/*--recipient*, or to a
	passphrase prompted on the terminal with
	*-p*/*--passphrase*; exactly one of the two must be
	chosen. After writing, each archive is re-opened to
	confirm its age header parses, and a summary line reports
	the account, file count, bytes and destination. The same
	export runs from inside the client as *:export* _account_
	_path_, encrypting to the *[export] recipients* keys from
	*config.toml*; the passphrase mode is command-line only.

*view* _archive_ [*-i* _identity-file_]... [*-p*]
	Open an archive written by *export* read-only, without
	needing the account it came from. Decrypt with one or more
	age identity files via *-i*/*--identity*, or with a
	passphrase prompted on the terminal via
	*-p*/*--passphrase*; exactly one of the two must be
	chosen. The archive is unpacked and indexed into a
	throwaway store under
	_$XDG_CACHE_HOME/antiphon/view/<stem>_ (the archive file
	name without its extensions, which also names the account
	in the client), then the normal interface opens over it:
	reading, search, threading and attachments all work, while
	anything that would change mail answers "read-only archive
	view". No daemon is started or contacted. Reopening the
	same archive reuses the unpacked store, so it is instant;
	on exit the store's path is printed together with the *rm
	-r* command that removes it.

*setup*
	Interactive first-time setup: asks for an address and
	server details, stores the mail and vault secrets, writes
	*config.toml* and an account file, creates the vault,
	initialises the store, and starts the daemon. Refuses to
	run if *config.toml* already exists.

*sendmail* [_args_]
	A sendmail-compatible entry point for *git-send-email*(1)
	and similar tools. See *antiphon-sendmail*(1).

# OPTIONS

*-h*, *--help*
	Print usage information.

*-V*, *--version*
	Print the version and exit.

# FILES

_$XDG_CONFIG_HOME/antiphon/config.toml_
	Global settings: UI, vault, sync, daemon, notifications and
	key bindings. Defaults to _~/.config/antiphon_ when
	*XDG_CONFIG_HOME* is unset. *accounts.order* lists account
	names to place first, in that order; unlisted accounts
	follow alphabetically by file name, and the first account
	is the primary: selected on startup and the unified view's
	compose From. Shift-J/K in the settings Accounts tab edit
	the same list.

_$XDG_CONFIG_HOME/antiphon/local.toml_
	Per-machine overrides, loaded after *config.toml* and
	merged over it key by key.

_$XDG_CONFIG_HOME/antiphon/accounts/\*.toml_
	One file per mail account: IMAP, SMTP, identities, filing
	rules and, optionally, OAuth and Microsoft Graph settings.
	*folder_order* lists sidebar folder names (as the sidebar
	shows them, *inbox* included) to place first, in that
	order; the rest stay alphabetical. *folders_hidden* lists
	folders to drop from the sidebar; a hidden folder stays
	synced and searchable. The settings Folders tab edits both:
	Shift-J/K reorder, *h* hides and unhides. *folders_unsynced*
	lists folders the daemon skips entirely (matched on the
	maildir-relative name, case-insensitively; the inbox is
	never excludable); see *antiphond*(1).

_$XDG_CONFIG_HOME/antiphon/signatures/_, _templates/_, _pgp/_
	Signature files, compose templates and the trusted OpenPGP
	certificate directory, each referenced by name from the
	configuration; see *PGP.md* for the last of these.

_$XDG_DATA_HOME/antiphon/store_
	The Maildir, notmuch index, operation log, outbox and sync
	state, sealed inside the vault where one is configured.
	Defaults to _~/.local/share/antiphon/store_.

_$XDG_CACHE_HOME/antiphon/view/_
	Throwaway stores unpacked by *view*, one directory per
	archive, safe to remove at any time. Defaults to
	_~/.cache/antiphon/view_.

_$XDG_STATE_HOME/antiphon/antiphond.log_
	Log of a daemon started automatically by the client.
	Defaults to _~/.local/state/antiphon/antiphond.log_.

Parsing every configuration file is strict: an unknown key
fails startup naming the file, the line, and the nearest valid
key.

# EXIT STATUS

*0*
	Success.

*1*
	Failure: a resolvable path is missing, the configuration
	does not parse, a subcommand's own precondition failed, or
	*doctor* found at least one failing check.

# SEE ALSO

*antiphond*(1), *antiphon-sendmail*(1)

# AUTHORS

Maintained by DonQuinleone <don@donquinleone.com>. Bugs go to
the tracker at https://todo.sr.ht/~donquinleone/antiphon;
patches go to the mailing list
<~donquinleone/antiphon-devel@lists.sr.ht> via *git-send-email*(1).
