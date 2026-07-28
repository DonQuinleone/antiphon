antiphond(1)

# NAME

antiphond - sync daemon for the antiphon mail client

# SYNOPSIS

*antiphond*++
*antiphond* [*-V*|*--version*]

# DESCRIPTION

antiphond is an ordinary foreground process: it does not fork,
it logs to stdout and stderr, and it takes no configuration of
its own beyond what *antiphon*(1)'s configuration files supply.
Run with no arguments to start it; it stays in the foreground
until stopped, which makes it a first-class citizen under any
service supervisor and trivially scriptable everywhere else.

Each sync pass fetches new mail per folder, sends queued
outbound mail from the crash-safe outbox, files sent and
delivered mail per the account's rules, uploads drafts, and
posts desktop notifications. Passes run on the configured
*[sync] interval_minutes*, whenever a client asks over the IPC
socket, and, with *[sync] idle = true*, whenever an IMAP IDLE
push arrives. A *UIDVALIDITY* change on a folder refetches it
outright rather than trusting local state.

Nothing has to run antiphond explicitly: when *antiphon* finds
no daemon reachable it starts one itself, in its own process
group, and that daemon outlives the terminal. Disable this with
*[daemon] autostart = false*. A supervisor is still the better
home for a daemon you want running before the first client
launch, restarted on failure, and stopped at logout; see
*DAEMON.md* for launchd, systemd, dinit, runit and desktop
autostart units, all provided under *dist/* in the source tree.

# OPTIONS

*-V*, *--version*
	Print the version and exit.

Any other argument is rejected with an "unknown argument"
message; antiphond takes no other flags.

# SIGNALS

*SIGHUP*, *SIGINT*, *SIGTERM*
	Each stops antiphond gracefully: it finishes the current
	loop iteration, seals the vault if one is mounted, and
	exits. All three are handled identically; there is no
	separate reload behaviour on *SIGHUP*. Any of a
	supervisor's default stop signals is safe to use.
	Configuration reloads happen over IPC instead:
	*antiphon*(1) requests one automatically after an
	account is added, edited or removed, so a running
	daemon picks up account changes without a restart.

# FILES

antiphond reads the same configuration and store as
*antiphon*(1); see *antiphon*(1) FILES. It additionally binds
the IPC socket that *antiphon*(1) and *antiphon-sendmail*(1)
connect to:

_$XDG_RUNTIME_DIR/antiphon/antiphond.sock_
	Preferred socket location, falling back in order to
	_$XDG_STATE_HOME/antiphon/antiphond.sock_ and
	_~/.local/state/antiphon/antiphond.sock_ when
	*XDG_RUNTIME_DIR* is unset.

# EXIT STATUS

*0*
	Clean shutdown on *SIGHUP*, *SIGINT* or *SIGTERM*.

*2*
	An unrecognised argument was given.

# SEE ALSO

*antiphon*(1), *antiphon-sendmail*(1)

# AUTHORS

Maintained by DonQuinleone <don@donquinleone.com>. Bugs go to
the tracker at https://todo.sr.ht/~donquinleone/antiphon;
patches go to the mailing list
<~donquinleone/antiphon-devel@lists.sr.ht> via *git-send-email*(1).
