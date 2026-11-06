#!/usr/bin/env bash
# Builds the desktop game for 64-bit Windows from this WSL box and packages it as an MSI.
# Usage: scripts/build_windows.sh [source dir]   (this tree by default)
#
# Build it from the same snapshot as the server it is going to play on: the rules have to match.
#
# Writes <source>/bin/windows/FrontCraft-<version>.msi, FrontCraft.msi (the same file) and latest.json,
# which the page reads to offer the download. Those three go in the site's downloads/ folder.
#
# Settings, through the environment:
#   FRONTCRAFT_VERSION  major.minor.build, where a higher build upgrades in place (minor and major under
#                       256, build under 65536). By default the workspace's major.minor and today as yyddd.
#   FRONTCRAFT_SERVER   where the main shortcut plays, wss://play.frontcraft.online by default.
#   CARGO_TARGET_DIR    <source>/bin/windows-target by default, apart from any other build.
#
# Needs: the x86_64-pc-windows-gnu Rust target, mingw-w64 (x86_64-w64-mingw32-gcc), Python 3, and on the
# Windows side the .NET SDK, which runs WiX 5. WiX is fetched once as a .NET tool into
# %LOCALAPPDATA%\FrontCraftBuild\wix, for this user only, with no installer and no admin.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
src="$(cd "${1:-$here}" && pwd)"
out="$src/bin/windows"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$src/bin/windows-target}"
# Absolute, so the exe is taken from exactly where cargo built it whatever directory this runs in.
mkdir -p "$CARGO_TARGET_DIR"
CARGO_TARGET_DIR="$(cd "$CARGO_TARGET_DIR" && pwd)"
wix_version=5.0.2
target=x86_64-pc-windows-gnu

workspace_version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$src/Cargo.toml")
version="${FRONTCRAFT_VERSION:-$(cut -d. -f1-2 <<<"$workspace_version").$(date -u +%y%j | sed 's/^0*//')}"
if ! [[ "$version" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || ((BASH_REMATCH[1] > 255 || BASH_REMATCH[2] > 255 || BASH_REMATCH[3] > 65535)); then
  echo "FRONTCRAFT_VERSION must be major.minor.build, with major and minor under 256 and build under 65536: $version" >&2
  exit 1
fi

for tool in x86_64-w64-mingw32-gcc python3 cmd.exe powershell.exe wslpath; do
  command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done
rustup target list --installed | grep -qx "$target" || { echo "missing Rust target: rustup target add $target" >&2; exit 1; }

# Source paths end up in panic messages. Without this the game ships this machine's home directory.
# The subsystem flag keeps a console window from opening beside the game.
registry="${CARGO_HOME:-$HOME/.cargo}/registry/src"
toolchains="${RUSTUP_HOME:-$HOME/.rustup}/toolchains"
echo "Building FrontCraft $version for Windows from $src"
cargo build --manifest-path "$src/Cargo.toml" -p prs_game --release --target "$target" \
  --config "profile.release.strip = 'symbols'" \
  --config "target.$target.rustflags = ['--remap-path-prefix=$registry=', '--remap-path-prefix=$toolchains=', '--remap-path-prefix=$src=', '-Clink-arg=-Wl,--subsystem,windows']"
exe="$CARGO_TARGET_DIR/$target/release/prs_game.exe"

# Anything it needs beyond what every Windows has would have to ship beside it. Nothing should.
system_dlls='^(kernel32|user32|gdi32|advapi32|shell32|ole32|oleaut32|ws2_32|bcrypt|ntdll|userenv|crypt32|secur32|ncrypt|imm32|winmm|dwmapi|uxtheme|comctl32|shlwapi|setupapi|cfgmgr32|hid|dxgi|d3d12|d3dcompiler_47|opengl32|version|propsys|powrprof|api-ms-win-.*|msvcrt|ucrtbase|winspool\.drv|comdlg32|iphlpapi|dbghelp|xinput1_4|mswsock|synchronization|ksuser|avrt|mmdevapi|runtimeobject|dcomp|d3d11|dinput8|bcryptprimitives|combase|uiautomationcore)\.dll$'
extra=$(x86_64-w64-mingw32-objdump -p "$exe" | sed -n 's/^\s*DLL Name: //p' | tr 'A-Z' 'a-z' | grep -Ev "$system_dlls" || true)
if [[ -n "$extra" ]]; then
  echo "prs_game.exe needs DLLs that are not part of Windows, and they are not packaged: $extra" >&2
  exit 1
fi

# The MSI is made on the Windows side, from a folder there, since WiX is a Windows program.
local_app=$(cd /mnt/c && cmd.exe /c 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')
tools_win="$local_app\\FrontCraftBuild"
tools=$(wslpath -u "$tools_win")
work_win="$tools_win\\work-$$"
work=$(wslpath -u "$work_win")
mkdir -p "$work"
trap 'rm -rf "$work"' EXIT
win() { (cd /mnt/c && powershell.exe -NoProfile -NonInteractive -Command "$1") | tr -d '\r'; }

if [[ ! -x "$tools/wix/wix.exe" ]]; then
  echo "Fetching WiX $wix_version as a .NET tool into $tools_win\\wix"
  win "dotnet tool install wix --version $wix_version --tool-path '$tools_win\\wix'"
fi

"$here/scripts/stage_windows.sh" "$work" "$exe" "$version" "$src"

echo "Packaging with WiX $wix_version"
win "Set-Location '$work_win'; & '$tools_win\\wix\\wix.exe' build -arch x64 -o FrontCraft.msi FrontCraft.wxs; exit \$LASTEXITCODE"
mkdir -p "$out"
cp "$work/FrontCraft.msi" "$out/FrontCraft-$version.msi"
cp "$work/FrontCraft.msi" "$out/FrontCraft.msi"
bytes=$(stat -c %s "$out/FrontCraft.msi")
printf '{ "version": "%s", "file": "FrontCraft-%s.msi", "bytes": %s }\n' "$version" "$version" "$bytes" > "$out/latest.json"
echo "Built $out/FrontCraft-$version.msi ($((bytes / 1000000)) MB)"
