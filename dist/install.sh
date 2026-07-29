#!/bin/sh
# Antiphon installer: curl -fsSL https://antiphon.net/install.sh | sh
# POSIX sh, because this runs before anything is installed.
# Package managers first, prebuilt binaries second, and the
# compiler only when nothing else can serve this machine.
set -eu

REPO="https://git.sr.ht/~donquinleone/antiphon"
TAP="donquinleone/antiphon"
TAP_URL="https://git.sr.ht/~donquinleone/homebrew-antiphon"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
fail() { printf 'antiphon install: %s\n' "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

macos_install() {
    have brew \
        || fail "install Homebrew first: https://brew.sh"
    say "adding the Antiphon tap and installing (notmuch"
    say "arrives as a dependency)"
    brew tap "$TAP" "$TAP_URL"
    brew install antiphon
}

arch_install() {
    for helper in paru yay; do
        have "$helper" || continue
        say "installing from the AUR with $helper"
        "$helper" -S --needed antiphon
        return 0
    done
    say "no AUR helper found; the manual route is:"
    say "    git clone https://aur.archlinux.org/antiphon.git"
    say "    cd antiphon && makepkg -si"
    return 1
}

fedora_install() {
    have dnf || return 1
    [ -r /etc/os-release ] || return 1
    # shellcheck disable=SC1091
    . /etc/os-release
    case "${ID:-}:${ID_LIKE:-}" in
        *fedora*) ;;
        *) return 1 ;;
    esac
    say "enabling the Antiphon Copr and installing (needs sudo)"
    sudo dnf copr enable -y "$TAP" \
        && sudo dnf install -y antiphon
}

nix_install() {
    have nix || return 1
    say "installing with nix from the flake"
    nix profile install "git+$REPO" \
        --extra-experimental-features 'nix-command flakes'
}

latest_tag() {
    git ls-remote --tags --refs --sort=-v:refname "$REPO" \
        | head -n 1 | sed 's|.*refs/tags/||'
}

tarball_install() {
    have git || return 1
    have curl || return 1
    arch="$(uname -m)"
    case "$arch" in
        x86_64) ;;
        *) return 1 ;;
    esac
    tag="$(latest_tag)"
    [ -n "$tag" ] || return 1
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

compile_install() {
    have cargo || fail "nothing on this machine can install \
Antiphon without building it, and cargo is missing; install \
Rust (https://rustup.rs) and notmuch, then re-run"
    say "building from source with cargo (the last resort)"
    cargo install --git "$REPO" --locked antiphon
    cargo install --git "$REPO" --locked antiphond
    check_runtime_deps
}

check_runtime_deps() {
    for tool in notmuch gpg; do
        have "$tool" && continue
        say "note: $tool is wanted at runtime; install it \
from your distribution (e.g. apt/dnf/pacman install $tool)"
    done
}

linux_install() {
    if have pacman; then
        arch_install && return 0
        say "falling back past the AUR"
    fi
    fedora_install && return 0
    nix_install && return 0
    tarball_install && return 0
    compile_install
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
