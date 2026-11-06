#!/usr/bin/env bash
# Builds the web client. Usage: scripts/build_web.sh [output dir], bin/web by default.
# FRONTCRAFT_ONLINE=1 is for a page that prs_server serves: it offers playing on the server it came from, and
# otherwise it just plays alone.
# Serve it with python3 -m http.server -d bin/web 8080, or scripts/serve_web.py to use the .br and
# .gz files this writes next to the originals.
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:-bin/web}"

version=$(grep -A1 'name = "wasm-bindgen"' Cargo.lock | grep version | cut -d'"' -f2)
if [[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $version" ]]; then
  echo "Installing wasm-bindgen-cli $version to match Cargo.lock"
  cargo install wasm-bindgen-cli --version "$version" --locked
fi

# Source paths end up in panic messages and log call sites. Without this the site ships this
# machine's home directory. It lives here because .cargo/config.toml cannot read $HOME.
registry="${CARGO_HOME:-$HOME/.cargo}/registry/src"
toolchains="${RUSTUP_HOME:-$HOME/.rustup}/toolchains"
mkdir -p bin "$(dirname "$out")"

# Work in a staging directory so a site being served never sees half a build.
staging=$(mktemp -d "$out.staging.XXXXXX")
trap 'rm -rf "$staging"' EXIT

# Every build in bin/ writes the same prs_game.wasm, whichever tree it came from. Hold this lock until
# this run has its own copy, so that it cannot publish what another run built.
(
  flock 9
  cargo build -p prs_game --target wasm32-unknown-unknown --profile wasm-release \
    --config "target.wasm32-unknown-unknown.rustflags = ['--remap-path-prefix=$registry=', '--remap-path-prefix=$toolchains=', '--remap-path-prefix=$PWD=']"
  cp "${CARGO_TARGET_DIR:-bin}/wasm32-unknown-unknown/wasm-release/prs_game.wasm" "$staging/prs_game.wasm"
) 9> bin/build_web.lock
wasm-bindgen "$staging/prs_game.wasm" \
  --out-dir "$staging" --target web --no-typescript --remove-name-section --remove-producers-section
rm "$staging/prs_game.wasm"
wasm="$staging/prs_game_bg.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz "$wasm" -o "$wasm"
else
  echo "warning: wasm-opt not found, so the wasm is about 10% larger. Install binaryen." >&2
fi

# The loading screen needs the real size, because the Content-Length of a compressed response
# does not match the bytes it reads. The id stops browsers pairing a cached script with a new wasm.
sed -e "s/__WASM_BYTES__/$(stat -c %s "$wasm")/" \
  -e "s/__BUILD_ID__/$(sha256sum "$wasm" | cut -c1-12)/" \
  -e "s/__ONLINE__/${FRONTCRAFT_ONLINE:-}/" web/index.html > "$staging/index.html"

# The footstep tuner at /tune/, on the same synth as the game.
cargo build -p prs_sound_lab --target wasm32-unknown-unknown --profile wasm-release \
  --config "target.wasm32-unknown-unknown.rustflags = ['--remap-path-prefix=$registry=', '--remap-path-prefix=$toolchains=', '--remap-path-prefix=$PWD=']"
cp -r web/tune "$staging/tune"
wasm-bindgen "${CARGO_TARGET_DIR:-bin}/wasm32-unknown-unknown/wasm-release/prs_sound_lab.wasm" \
  --out-dir "$staging/tune" --target web --no-typescript --remove-producers-section

# Fetched by the game itself with ?mod=name.
cp -r mods "$staging/mods"

# Linked from the page: every crate the game is built from, with its licence.
scripts/third_party_licenses.sh "$staging/THIRD_PARTY_LICENSES.html"

for file in "$wasm" "$staging/prs_game.js"; do
  gzip -9 -k "$file"
  if command -v brotli >/dev/null; then
    brotli -q 11 -k "$file"
  else
    echo "warning: brotli not found, so there is no $(basename "$file").br." >&2
  fi
done

# Runs publishing to the same place take turns, so what is served all comes from one of them.
(
  flock 9
  mkdir -p "$out"
  rm -rf "$out"/*.br "$out"/*.gz "$out/mods" "$out/tune" "$out/snippets"
  mv -f "$staging"/* "$out"/
) 9> "$out.lock"
du -h "$out"/prs_game_bg.wasm*
