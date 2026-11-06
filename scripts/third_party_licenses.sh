#!/usr/bin/env bash
# Writes THIRD_PARTY_LICENSES.html: every crate the game is built from, with its licence text, made by cargo-about
# from Cargo.lock and the crates' own licence files. Every package and the web build carry it.
# Usage: scripts/third_party_licenses.sh <output file> [source dir]   (this tree by default)
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$(dirname "$1")"
out="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
cd "${2:-$here}"
version=0.9.3
if [[ "$(cargo about --version 2>/dev/null)" != "cargo-about $version" ]]; then
  echo "Installing cargo-about $version"
  cargo install cargo-about --version "$version" --locked --features cli
fi
cargo about generate --manifest-path crates/prs_game/Cargo.toml -c "$here/scripts/licences/about.toml" \
  "$here/scripts/licences/about.hbs" -o "$out"
