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

`url` and `sha256` point at a `v1.0.0` tag that does not exist
yet; both are placeholders.

## Seeding the tap

Once `v1.0.0` is tagged:

1. Create the `homebrew-antiphon` repository on SourceHut and
   push this file to it as `Formula/antiphon.rb`.
2. Compute the real checksum:

   ```bash
   curl -fsSL \
       https://git.sr.ht/~donquinleone/antiphon/archive/v1.0.0.tar.gz \
       | shasum -a 256
   ```

3. Replace the placeholder `sha256` (and `url`, if the version
   ever needs bumping later) with that value.
4. `brew install --build-from-source
   ./Formula/antiphon.rb` locally to confirm it builds and the
   `test do` block passes, then commit and push.
5. Copy the same bump back into this repository's
   `dist/homebrew/Formula/antiphon.rb`, so it stays the record
   of what is actually published.

From then on, `brew tap donquinleone/antiphon
https://git.sr.ht/~donquinleone/homebrew-antiphon && brew
install antiphon` installs it, and a version bump is just steps
2-5 again against the new tag.
