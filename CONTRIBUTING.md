# Contributing to antiphon

antiphon is developed on SourceHut with the e-mail-based patch
workflow; there are no pull requests. The
[GitHub repository](https://github.com/DonQuinleone/antiphon)
is a read-only mirror.

- Source: https://git.sr.ht/~donquinleone/antiphon
- Patches and discussion:
  https://lists.sr.ht/~donquinleone/antiphon-devel
- Bugs and feature requests:
  https://todo.sr.ht/~donquinleone/antiphon
- Builds: https://builds.sr.ht/~donquinleone/antiphon

Antiphon is deliberate about scope: plaintext-first mail,
local-first storage, encryption at rest, and no bespoke
crypto or protocol parsing. Before proposing a feature,
consider whether it fits that shape; if it cuts against it,
open a ticket to argue the case rather than sending a patch.

## Sending patches

```bash
git config sendemail.to \
    '~donquinleone/antiphon-devel@lists.sr.ht'
git send-email --annotate origin/master..HEAD
```

New to `git send-email`? The tutorial at
[git-send-email.io](https://git-send-email.io) covers setup,
and [git.sr.ht's web UI](https://git.sr.ht) offers a
"Prepare a patchset" alternative that needs no local mail
configuration. Revise a series after review with
`git send-email -v2 --annotate origin/master..HEAD`.

## Ground rules for patches

- The default branch is master.
- Commit messages are kernel style: a short imperative subject
  (prefixed with the area touched, e.g. `config:`, `store:`),
  a blank line, then a body explaining why, wrapped at 72
  columns. Keep commits small and bisectable; a series should
  be reviewable commit by commit.
- Everything wraps at 72 columns: code (rustfmt is configured
  for it), comments, prose and commit messages.
- Comments are a last resort; write one only for a non-obvious
  constraint the code cannot carry. No section labels, no
  restating the code.
- Guard clauses over nesting; functions do one thing; no magic
  numbers; meaningful names.
- New behaviour lands with a test.
- Prose is British English, without em-dashes.
- Dependencies are pinned to exact versions; a bump is its own
  commit explaining why.

## Before sending

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI on [builds.sr.ht](https://builds.sr.ht/~donquinleone/antiphon)
runs these on Arch (stable toolchain, plus the query
performance smoke test) and Alpine (MSRV floor and musl);
macOS is tested by the maintainer before releases. Green CI is
a precondition for review, not a substitute for it.

## Licence

By contributing you agree to license your work under
GPL-3.0-or-later, the project licence.
