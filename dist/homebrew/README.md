# Homebrew tap

`Formula/antiphon.rb` here is the seed for the tap repository
at <https://git.sr.ht/~donquinleone/homebrew-antiphon>. It
builds both binaries from source with cargo and renders the
scdoc man pages at build time; `notmuch` is a build and runtime
dependency, `rust` and `scdoc` are build-only. `gnupg` is
deliberately not a `depends_on`: Homebrew removed optional
dependencies from the formula DSL, and OpenPGP support is
opt-in per identity, so the formula notes it in `caveats`
instead of forcing the install on everyone.

The tap is live: `brew tap donquinleone/antiphon
https://git.sr.ht/~donquinleone/homebrew-antiphon && brew
install antiphon` installs the current release. `url` and
`sha256` here mirror the published formula, updated at each
release.

## Bumping the formula

The version bump is one step of cutting a release; see the
release guide at <https://docs.antiphon.net> for the full
process. In short: point `url` at the new `vX.Y.Z` tag
archive, set `sha256` to `curl -fsSL
https://git.sr.ht/~donquinleone/antiphon/archive/vX.Y.Z.tar.gz
| shasum -a 256`, validate (`ruby -c`, or a full `brew install
--build-from-source`), push to the tap, and mirror the same
change back here so this file stays the record of what is
published.
