# Fedora Copr packaging

This directory holds the RPM spec that publishes Antiphon through
[Fedora Copr](https://copr.fedorainfracloud.org), the community build
service. Copr builds the spec in a clean chroot for each supported
Fedora release and serves the results as a `dnf` repository.

- `antiphon.spec` builds both binaries from the tagged source
  tarball, renders the man pages with scdoc, and installs the
  systemd user unit.
- `../../.copr/Makefile` produces the source RPM for Copr's SCM
  method (`make srpm`).

## Installing (users)

```sh
sudo dnf copr enable donquinleone/antiphon
sudo dnf install antiphon
```

`gnupg2` is only recommended: install it yourself if you sign or
decrypt mail, since OpenPGP is opt-in per identity.

## Publishing (maintainer)

### Prerequisites

- A [Fedora Account System](https://accounts.fedoraproject.org)
  login. Creating and building a Copr project requires it; nothing
  else here does.
- The `copr-cli` tool (`sudo dnf install copr-cli`) with an API
  token saved to `~/.config/copr`. Generate the token at
  <https://copr.fedorainfracloud.org/api/> while logged in.

### One-time: create the project

Either through the web UI (New Project) or the CLI:

```sh
copr-cli create antiphon \
    --chroot fedora-41-x86_64 \
    --chroot fedora-41-aarch64 \
    --chroot fedora-42-x86_64 \
    --chroot fedora-42-aarch64 \
    --description "Modern mail client for the terminal"
```

Adjust the chroot list to the Fedora releases you want to support.

### Wire up the SCM source (recommended)

Point Copr at this repository so a rebuild is one click or one
command. In the project's Settings, Packages, New Package, choose
method **SCM** and set:

- Clone URL: `https://git.sr.ht/~donquinleone/antiphon`
- Committish: `master` (or a release tag)
- Subdirectory: leave empty
- Spec File: `dist/copr/antiphon.spec`
- Source build method: **make srpm**

Copr clones the repo, runs `make -f .copr/Makefile srpm`, and the
Makefile fetches the tagged tarball named in the spec. The built
package therefore always matches the spec's `Version`, not the
cloned commit, so bumping the spec is what ships a new release.

Trigger a build with:

```sh
copr-cli build-package antiphon --name antiphon
```

### Or build a one-off SRPM by hand

On a Fedora box with `rpmdevtools`:

```sh
make -f .copr/Makefile srpm outdir="$PWD"
copr-cli build antiphon antiphon-*.src.rpm
```

## Releasing a new version

The spec carries the version in exactly one place, its `Version`
tag, mirroring the AUR `pkgver`. To cut a release:

1. Tag the repository (`vX.Y.Z`) and let the sr.ht CI publish the
   tag archive.
2. Bump `Version:` in `antiphon.spec` and add a `%changelog`
   entry.
3. Rebuild (`copr-cli build-package antiphon --name antiphon`).

## Validation notes

The spec has not been built on this machine (macOS, no `rpmbuild`
or `mock`). Validate it on a Fedora host before the first publish:

```sh
rpmlint dist/copr/antiphon.spec
make -f .copr/Makefile srpm outdir="$PWD"   # local SRPM
mock -r fedora-42-x86_64 rebuild antiphon-*.src.rpm
```
