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
	*client_id* on that account.

*oauth status* _account_
	Show the grants stored for _account_ and how long each
	access token has left before it needs a refresh.

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
	*XDG_CONFIG_HOME* is unset.

_$XDG_CONFIG_HOME/antiphon/local.toml_
	Per-machine overrides, loaded after *config.toml* and
	merged over it key by key.

_$XDG_CONFIG_HOME/antiphon/accounts/\*.toml_
	One file per mail account: IMAP, SMTP, identities, filing
	rules and, optionally, OAuth and Microsoft Graph settings.

_$XDG_CONFIG_HOME/antiphon/signatures/_, _templates/_, _pgp/_
	Signature files, compose templates and the trusted OpenPGP
	certificate directory, each referenced by name from the
	configuration; see *PGP.md* for the last of these.

_$XDG_DATA_HOME/antiphon/store_
	The Maildir, notmuch index, operation log, outbox and sync
	state, sealed inside the vault where one is configured.
	Defaults to _~/.local/share/antiphon/store_.

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
