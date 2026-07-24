#!/bin/sh
# Antiphon installer: curl -fsSL https://antiphon.net/install.sh | sh
# POSIX sh, because this runs before anything is installed.
set -eu

REPO="https://git.sr.ht/~donquinleone/antiphon"
TAP="donquinleone/antiphon"
TAP_URL="https://git.sr.ht/~donquinleone/homebrew-antiphon"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
fail() { printf 'antiphon install: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 \
        || fail "$1 is required to install; $2"
}

macos_install() {
    need brew "install Homebrew first: https://brew.sh"
    say "adding the Antiphon tap and installing (notmuch and"
    say "gnupg arrive as dependencies)"
    brew tap "$TAP" "$TAP_URL"
    brew install antiphon
}

latest_tag() {
    git ls-remote --tags --refs --sort=-v:refname "$REPO" \
        | head -n 1 | sed 's|.*refs/tags/||'
}

linux_install() {
    need git "install it from your distribution"
    need curl "install it from your distribution"
    arch="$(uname -m)"
    case "$arch" in
        x86_64 | aarch64) ;;
        *) fail "no prebuilt binary for $arch; run \
'cargo install --git $REPO antiphon antiphond --locked'" ;;
    esac
    tag="$(latest_tag)"
    [ -n "$tag" ] || fail "no release has been published yet"
    tarball="antiphon-$tag-$arch-linux-gnu.tar.gz"
    url="$REPO/refs/download/$tag/$tarball"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    say "downloading $tarball"
    curl -fsSL -o "$tmp/$tarball" "$url"
    curl -fsSL -o "$tmp/$tarball.sha256" "$url.sha256"
    (cd "$tmp" && sha256sum -c "$tarball.sha256" >/dev/null) \
        || fail "checksum mismatch; refusing to install"
    mkdir -p "$BIN_DIR"
    tar -xzf "$tmp/$tarball" -C "$tmp"
    install -m 755 "$tmp/antiphon" "$tmp/antiphond" "$BIN_DIR/"
    say "installed antiphon and antiphond to $BIN_DIR"
    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *) say "note: add $BIN_DIR to your PATH" ;;
    esac
    check_runtime_deps
}

check_runtime_deps() {
    for tool in notmuch gpg; do
        command -v "$tool" >/dev/null 2>&1 && continue
        say "note: $tool is required at runtime; install it \
from your distribution (e.g. apt/dnf/pacman install $tool)"
    done
}

main() {
    case "$(uname -s)" in
        Darwin) macos_install ;;
        Linux) linux_install ;;
        *) fail "unsupported platform $(uname -s); build from \
source: $REPO" ;;
    esac
    say ""
    say "run 'antiphon setup' to configure your first account"
}

main "$@"
