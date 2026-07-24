# Contributing to antiphon

Development happens on
[GitHub](https://github.com/DonQuinleone/antiphon); issues and
pull requests are welcome there. The
[SourceHut repository](https://git.sr.ht/~donquinleone/antiphon)
is a read-only mirror.

Antiphon is pre-alpha and built milestone by milestone against
[DESIGN.md](DESIGN.md). Before proposing a feature, check the
design document: if it is listed as deferred or out of scope,
open an issue to argue the case rather than a pull request.

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

CI runs exactly these on Linux and macOS, plus a release build;
green CI is a precondition for review, not a substitute for it.

## Licence

By contributing you agree to license your work under
GPL-3.0-or-later, the project licence.
