#!/usr/bin/env bash
# Integration run of the LUKS2 backend on a Linux builder.
# Every vault operation goes through the luks-cycle example,
# so the shell exercises exactly the command sequences the
# Rust backend issues. Needs passwordless sudo (builds.sr.ht
# build users have it).
set -euo pipefail

readonly MAPPER='antiphon-ci-vault'
readonly MARKER='antiphon-marker-9f2c4e8a1b'
readonly PASSPHRASE='ci-throwaway-passphrase'

workdir="$(mktemp -d)"
readonly workdir
readonly container="$workdir/vault.luks"
readonly mount_dir="$workdir/mnt"

export USER="${USER:-$(id -un)}"

cycle() {
    cargo run --locked --quiet -p antiphon-vault \
        --example luks-cycle -- \
        "$1" "$container" "$MAPPER" "$mount_dir"
}

cleanup() {
    sudo -n umount "$mount_dir" 2>/dev/null || true
    sudo -n cryptsetup close "$MAPPER" 2>/dev/null || true
    rm -rf "$workdir"
}
trap cleanup EXIT

assert_status() {
    local actual
    actual="$(cycle status)"
    if [ "$actual" != "$1" ]; then
        echo "expected vault $1, got $actual" >&2
        exit 1
    fi
}

assert_status absent

printf '%s' "$PASSPHRASE" | cycle create
assert_status open
printf '%s\n' "$MARKER" > "$mount_dir/marker"

cycle lock
assert_status sealed
if grep -aq "$MARKER" "$container"; then
    echo 'marker leaked into raw container bytes' >&2
    exit 1
fi

printf '%s' "$PASSPHRASE" | cycle unlock
assert_status open
if [ "$(cat "$mount_dir/marker")" != "$MARKER" ]; then
    echo 'marker did not survive the reopen' >&2
    exit 1
fi

cycle lock
assert_status sealed

echo 'luks2 integration cycle passed'
