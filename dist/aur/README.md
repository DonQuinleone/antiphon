# AUR packaging

`antiphon/PKGBUILD` and `.SRCINFO` here are the source for the `antiphon`
AUR package. They build both binaries with cargo, generate the
man pages with scdoc, and install the systemd user unit
alongside them. `notmuch` is a hard dependency (antiphond shells
out to it for indexing and tagging); `gnupg` is `optdepends`
only, since OpenPGP signing and decryption are opt-in per
identity and the rest of the client works without it.

`pkgver` and `sha256sums` in both files are placeholders until
v1.0.0 is tagged; the tarball they point at does not exist yet.

## Release-time steps

Run these from a machine with `makepkg`, once `v$pkgver` is
tagged and pushed to
<https://git.sr.ht/~donquinleone/antiphon>:

1. Clone the AUR package repository (first time only):
   `git clone ssh://aur@aur.archlinux.org/antiphon.git`.
2. Copy this directory's `antiphon/PKGBUILD` into the clone, or edit the
   clone's copy directly.
3. Bump `pkgver` to the released version and reset `pkgrel=1`
   (bump `pkgrel` instead, on a re-package of the same
   `pkgver`).
4. Run `updpkgsums` in the clone to fetch the real tarball and
   replace the placeholder `sha256sums` with its checksum.
5. Regenerate the metadata: `makepkg --printsrcinfo
   >.SRCINFO`.
6. Build and check the package locally: `makepkg -si` (or
   `namcap antiphon/PKGBUILD` and `namcap antiphon-$pkgver-$pkgrel-*.pkg.tar.zst`
   once built, to catch anything the guidelines require).
7. Commit `antiphon/PKGBUILD` and `.SRCINFO` in the AUR clone and push:
   `git commit -m "..." && git push`.
8. Copy the same bump back into this repository's
   `dist/aur/antiphon/PKGBUILD` and `dist/aur/.SRCINFO`, so they stay the
   record of what is actually published.

`yay -S antiphon` or `paru -S antiphon` picks the package up
from the AUR once step 7 lands.

## antiphon-git

`antiphon-git/PKGBUILD` tracks master and can sit on the AUR
before any release exists: `provides=(antiphon)`,
`conflicts=(antiphon)`, `pkgver()` derives from git describe,
and `SKIP` is correct for a VCS source. Push it to
`ssh://aur@aur.archlinux.org/antiphon-git.git` with its own
`.SRCINFO` (`makepkg --printsrcinfo > .SRCINFO`); it needs no
release-time bumps, only guideline hygiene when the build
inputs change.
