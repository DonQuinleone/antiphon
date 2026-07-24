# Packaging

Distribution templates and tooling ahead of the first release
(M10 in DESIGN.md). Status of each piece:

- `scripts/dist.sh`: works today; builds host-platform release
  tarballs into `dist/` with SHA256SUMS, versioned from
  `git describe`.
- `PKGBUILD` (Arch) and `homebrew/antiphon.rb`: complete except
  the `@VERSION@` and `@SHA256@` tokens, which only a tagged
  release can fill; the release pipeline substitutes them and
  publishes to the AUR and the tap; source archives come from
  git.sr.ht, where release artefacts attach to the tag.
- `nfpm.yaml`: deb and rpm packaging via nfpm; usable now
  against a local release build with `VERSION` and `ARCH` set.

Nothing here is wired into CI until there is a release to cut;
the release tooling (tag-triggered, run from a trusted machine)
arrives at M10 per DESIGN.md section 11.
