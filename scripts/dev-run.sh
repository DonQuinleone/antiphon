#!/usr/bin/env bash
#
# Run the locally-built Antiphon from this working tree, for
# trying changes that are not in the Homebrew build yet. It
# rebuilds first, so you never run a stale binary.
#
#   scripts/dev-run.sh [args...]         run the dev `antiphon`
#   scripts/dev-run.sh daemon [args...]  run the dev `antiphond`
#   scripts/dev-run.sh build             build only, then stop
#
# Examples:
#   scripts/dev-run.sh vault yubikey-enrol
#   scripts/dev-run.sh daemon

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

build() {
    echo "==> building antiphon + antiphond (release)" >&2
    (cd "$repo_root" && cargo build --release -p antiphon -p antiphond)
}

run_client() {
    build
    exec "$repo_root/target/release/antiphon" "$@"
}

run_daemon() {
    build
    echo "==> running dev antiphond; Ctrl-C to stop" >&2
    exec "$repo_root/target/release/antiphond" "$@"
}

case "${1:-}" in
    "")
        run_client
        ;;
    build)
        build
        ;;
    daemon)
        shift
        run_daemon "$@"
        ;;
    client)
        shift
        run_client "$@"
        ;;
    *)
        run_client "$@"
        ;;
esac
