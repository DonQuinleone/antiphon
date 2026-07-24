# Releasing Antiphon

The whole distribution story hangs off git tags on the
canonical repository; versions are never written anywhere
else.

## Cutting a release

1. Gate green on master, then tag and push:
   `git tag -s v1.0.0 -m "..." && git push origin v1.0.0`.
2. `.builds/release.yml` fires on the tag: it builds release
   binaries `--locked`, packages
   `antiphon-<tag>-x86_64-linux-gnu.tar.gz` (binaries, service
   units, licence, README) with a sha256 sidecar, and uploads
   both as tag artefacts on git.sr.ht via the job's
   `git.sr.ht/OBJECTS:RW` grant. aarch64 joins when a second
   manifest targets SourceHut's arm builders.
3. Update the Homebrew tap
   (`https://git.sr.ht/~donquinleone/homebrew-antiphon`): the
   formula points at the tag archive
   (`.../archive/<tag>.tar.gz`), carries its sha256, declares
   `depends_on "notmuch"` and `"gnupg"` plus a rust build
   dependency, and builds both binaries from source. The tap
   is seeded at v1.0; the formula's version and checksum are
   stamped from the tag at release time, never written ahead
   of it.

## The one-command installer

`dist/install.sh` is served as
`https://antiphon.net/install.sh` from v1.0 (the apex domain
points at SourceHut pages alongside the landing page; the
docs live at docs.antiphon.net). Until then it ships in the
repository only. Behaviour:

- macOS: adds the tap and `brew install antiphon`, which
  resolves notmuch and gnupg properly.
- Linux (x86_64/aarch64): resolves the latest tag with
  `git ls-remote`, downloads the tarball and checksum from the
  tag's artefacts, verifies, installs to
  `${XDG_BIN_HOME:-~/.local/bin}`, and names any missing
  runtime dependency (notmuch, gpg) rather than guessing at
  package managers.
- Anything else: pointed at `cargo install --locked` from the
  repository.

Native packages (AUR first, then other repositories we can
maintain ourselves) supersede the tarball path per platform as
they land; the script prefers them as they do.

## Publishing gates

Nothing is published before v1.0: the installer URL, the tap
and the docs site all go live together with the landing page.
The release CI can run on a pre-1.0 test tag at any time to
prove the pipeline; artefacts on a test tag are harmless.
