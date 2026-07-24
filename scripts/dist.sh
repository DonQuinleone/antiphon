#!/usr/bin/env bash
set -euo pipefail

# Builds release tarballs for the host platform into dist/,
# named from git describe, with SHA256SUMS. Safe to re-run.

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(git describe --tags --always --dirty)"
host="$(rustc -vV | sed -n 's/host: //p')"
name="antiphon-${version}-${host}"
staging="dist/${name}"

cargo build --release --workspace --locked

rm -rf "$staging"
mkdir -p "$staging"
cp target/release/antiphon target/release/antiphond \
    README.md COPYING "$staging/"

tar -czf "dist/${name}.tar.gz" -C dist "$name"
rm -rf "$staging"

(cd dist && shasum -a 256 ./*.tar.gz > SHA256SUMS)
echo "wrote dist/${name}.tar.gz"
