antiphon-sendmail(1)

# NAME

antiphon-sendmail - sendmail-compatible entry point for git
send-email and friends

# SYNOPSIS

*antiphon sendmail* [*-i*|*-oi*] [*-f* _address_] [*-F* _name_]++
[*-t*] [*--*] _recipient_...

This is the *sendmail* subcommand of *antiphon*(1), not a
separate binary; point tools that expect a sendmail command at
*"antiphon sendmail"* as a single string, for example:

	git config --global sendemail.sendmailCmd "antiphon sendmail"

# DESCRIPTION

antiphon-sendmail reads one RFC 5322 message from standard
input, matches it to a configured account by its envelope or
*From* address, and queues it durably in that account's outbox
for *antiphond*(1) to send over SMTP, filing a sent copy on
delivery. If no daemon is reachable one is started first,
unlocking the vault on the way. Once queued, it asks the daemon
to drain the outbox immediately and waits for that pass; if the
daemon cannot be reached the message stays queued and the next
scheduled sync sends it.

The account match runs the same identity resolution as replies:
the *From* address (or *-f* envelope sender, if given) is
checked against every configured identity's *match* patterns
across all accounts, and the first account with a matching
identity is used. There is no default account and no
guessing: an unmatched address is refused rather than sent from
the wrong place.

Recipients come from the command line, as git and other
sendmail callers provide them, never by re-parsing the message
headers.

# OPTIONS

*-i*, *-oi*
	Accepted and ignored (dot-stuffing is not needed on this
	path).

*-f* _address_
	Set the envelope-sender address used to resolve the
	account. Without it, the message's *From* header is used.

*-F* _name_
	Accepted and ignored.

*-t*
	Accepted and ignored (recipients are always taken from the
	command line, not the headers).

*--*
	End of options; anything after is a recipient even if it
	begins with a hyphen.

Any other flag is reported to stderr and otherwise ignored.

# EXIT STATUS

Exit codes follow *sysexits.h*, since git send-email and most
other callers only distinguish zero from non-zero, but the
classic meanings help everyone else:

*0*
	The message was queued (*EX_OK*).

*64*
	Usage error: no recipients were given, or a flag needing a
	value did not get one (*EX_USAGE*).

*67*
	No configured identity matches the resolved *From* address
	(*EX_NOUSER*).

*75*
	A transient failure: reading standard input failed, the
	daemon could not be started, or queueing the message failed
	(*EX_TEMPFAIL*).

*78*
	The home directory could not be resolved, or the
	configuration failed to load (*EX_CONFIG*).

# FILES

See *antiphon*(1) FILES for the configuration and store
locations that govern account matching and queueing.

# SEE ALSO

*antiphon*(1), *antiphond*(1), *git-send-email*(1)

# AUTHORS

Maintained by DonQuinleone <don@donquinleone.com>. Bugs go to
the tracker at https://todo.sr.ht/~donquinleone/antiphon;
patches go to the mailing list
<~donquinleone/antiphon-devel@lists.sr.ht> via *git-send-email*(1).
