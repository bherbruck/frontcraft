#!/usr/bin/env bash
# Packs a built prs_game for Linux as a tarball with the mods and a README.
# Usage: scripts/package_linux.sh <prs_game> <version> <out dir> [source dir]
# Writes <out dir>/FrontCraft-<version>-linux-x86_64.tar.gz, which unpacks into FrontCraft/.
#
#   FRONTCRAFT_SERVER   where play-online.sh plays, wss://play.frontcraft.online by default.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
exe="$1"
version="$2"
out="$(mkdir -p "$3" && cd "$3" && pwd)"
src="$(cd "${4:-$here}" && pwd)"
server="${FRONTCRAFT_SERVER:-wss://play.frontcraft.online}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
game="$work/FrontCraft"
mkdir -p "$game/mods"
install -m 755 "$exe" "$game/frontcraft"
cp "$src/mods/"*.toml "$game/mods/"

# The mods folder is found from where the game runs, so each script starts it from here.
for script in "play-online.sh:--server $server" "shooting-range.sh:--map range"; do
  cat > "$game/${script%%:*}" <<EOF
#!/bin/sh
cd "\$(dirname "\$0")" && exec ./frontcraft ${script#*:} "\$@"
EOF
  chmod 755 "$game/${script%%:*}"
done

cat > "$game/README.txt" <<EOF
FrontCraft $version for Linux (x86_64)

  ./play-online.sh      play online at $server
  ./shooting-range.sh   practise alone on the shooting range

Or run ./frontcraft yourself with --server <address> or --map range.

Mods: each .toml file in mods/ is one. To play with one, set PRS_MOD to its name, for example
  PRS_MOD=arcade ./shooting-range.sh
A server decides the rules for itself, so online the server's rules win; mods are for playing alone.

Needs a GPU with Vulkan drivers (Mesa or the vendor's), ALSA (libasound2) and libudev, which desktop
distributions have. Settings are saved in ~/.config/prs/settings.txt.
EOF

tarball="$out/FrontCraft-$version-linux-x86_64.tar.gz"
tar -C "$work" --owner=0 --group=0 -czf "$tarball" FrontCraft
echo "Built $tarball ($(($(stat -c %s "$tarball") / 1000000)) MB)"
