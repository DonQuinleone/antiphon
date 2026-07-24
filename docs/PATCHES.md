# Patches and git send-email

Antiphon treats patch mail as a first-class citizen: patches
render highlighted in the pager, a thread exports as a series
`git am` understands, and Antiphon itself can be the sendmail
that `git send-email` speaks to, so a series goes out through
the same durable outbox as every other message.

## Reading patches

A message is treated as a patch when its subject carries a
`[PATCH ...]` tag or its body contains a unified diff. The
pager colours additions sage and removals rubric, with file
and hunk headers distinct; trailing prose is never
misclassified by a stray leading `+`.

## Applying a series

- `:save-patches <path>` writes the current thread's patch
  series to `<path>` as an mbox in `[PATCH n/m]` order: cover
  letter first, re-rolls resolved to the highest version,
  replies without diffs excluded.
- `:apply <repo-dir>` does the same into a temporary file and
  runs `git am` in that repository. On failure the status line
  shows git's last words and the repository is left in git's
  own `am` state (`git am --continue` / `--abort` as usual).

## Sending with git send-email

Point git at Antiphon once:

```bash
git config --global sendemail.sendmailCmd "antiphon sendmail"
```

Then `git send-email 000*.patch` works as normal. Each message
is matched to an account by its From address (against your
identity `match` patterns), queued durably in the outbox, and
handed to antiphond for SMTP delivery with a sent copy filed;
if no daemon is running, one is started, unlocking the vault
on the way. The command exits non-zero without queueing when
no account matches or the store is unavailable, so git stops
rather than half-sending a series.

The classic sendmail flags are honoured or tolerated (`-i`,
`-f` envelope sender, recipients as arguments); recipients
come from git, not from message headers.
