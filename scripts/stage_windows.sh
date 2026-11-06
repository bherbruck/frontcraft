#!/usr/bin/env bash
# Lays out everything the MSI is made from in one folder: the game, the mods, the icon and FrontCraft.wxs.
# scripts/build_windows.sh and the release workflow both use it, so the two installers are the same.
# Usage: scripts/stage_windows.sh <folder> <prs_game.exe> <version> [source dir]
# Then, in that folder: wix build -arch x64 -o FrontCraft.msi FrontCraft.wxs
#
#   FRONTCRAFT_SERVER   where the main shortcut plays, wss://play.frontcraft.online by default.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
work="$1"
exe="$2"
version="$3"
src="$(cd "${4:-$here}" && pwd)"
server="${FRONTCRAFT_SERVER:-wss://play.frontcraft.online}"
# Never change this: it is what lets a new MSI replace an installed older one.
upgrade_code="A93AF7B1-5A22-472B-8C33-80A6FACD2482"

mkdir -p "$work/mods"
cp "$exe" "$work/FrontCraft.exe"
cp "$src/mods/"*.toml "$work/mods/"
cat > "$work/mods/README.txt" <<'EOF'
Mods for FrontCraft. Each .toml file here is one. To play with one, start FrontCraft.exe with PRS_MOD set to
its name, for example PRS_MOD=arcade for arcade.toml. A server decides the rules for itself, so online the
server's rules win; mods are for playing alone.
EOF
# Windows has a python3 that only points at the Store, so take whichever one runs.
python=python3
"$python" -c '' 2>/dev/null || python=python
"$python" "$here/scripts/make_icons.py" "$work" ico

mod_files=""
for mod in "$work/mods/"*; do
  name=$(basename "$mod")
  mod_files+="          <File Id=\"mod_${name//[^A-Za-z0-9]/_}\" Source=\"mods\\$name\" KeyPath=\"no\" />"$'\n'
done

# Per user, into %LOCALAPPDATA%\Programs\FrontCraft, so it installs without asking for admin and the folder it runs in is the player's own,
# which is where settings live and mods go. Everything in a user's profile is found again by a value in
# HKCU, which is what Windows Installer wants for that.
cat > "$work/FrontCraft.wxs" <<EOF
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="FrontCraft" Manufacturer="FrontCraft" Version="$version" UpgradeCode="$upgrade_code" Scope="perUser" Compressed="yes" Language="1033" Codepage="1252">
    <MajorUpgrade AllowSameVersionUpgrades="yes" DowngradeErrorMessage="A newer FrontCraft is already installed." />
    <MediaTemplate EmbedCab="yes" CompressionLevel="high" />
    <Icon Id="FrontCraftIcon" SourceFile="frontcraft.ico" />
    <Property Id="ARPPRODUCTICON" Value="FrontCraftIcon" />
    <Property Id="ARPURLINFOABOUT" Value="https://play.frontcraft.online" />
    <Property Id="ARPNOMODIFY" Value="1" />

    <StandardDirectory Id="LocalAppDataFolder">
      <Directory Id="UserPrograms" Name="Programs">
      <Directory Id="INSTALLFOLDER" Name="FrontCraft">
        <Component Id="Game" Guid="E556783A-9D28-4E96-A8F1-EB82BE87F094">
          <File Id="FrontCraftExe" Source="FrontCraft.exe" KeyPath="no" />
          <RegistryValue Root="HKCU" Key="Software\FrontCraft" Name="InstallFolder" Type="string" Value="[INSTALLFOLDER]" KeyPath="yes" />
          <RemoveFolder Id="RemoveInstallFolder" On="uninstall" />
          <!-- Only goes if nothing else is in it. -->
          <RemoveFolder Id="RemoveUserPrograms" Directory="UserPrograms" On="uninstall" />
        </Component>
        <Directory Id="ModsFolder" Name="mods">
          <Component Id="Mods" Guid="B2F995B9-3D83-4747-8F67-1635AA39850B">
$mod_files          <RegistryValue Root="HKCU" Key="Software\FrontCraft" Name="Mods" Type="integer" Value="1" KeyPath="yes" />
            <RemoveFolder Id="RemoveModsFolder" On="uninstall" />
          </Component>
        </Directory>
      </Directory>
      </Directory>
    </StandardDirectory>

    <StandardDirectory Id="ProgramMenuFolder">
      <Component Id="Shortcuts" Guid="*">
        <Shortcut Id="PlayOnline" Name="FrontCraft" Description="Play FrontCraft online" Target="[INSTALLFOLDER]FrontCraft.exe" Arguments="--server $server" WorkingDirectory="INSTALLFOLDER" Icon="FrontCraftIcon" />
        <Shortcut Id="PlayRange" Name="FrontCraft &#8211; Shooting Range" Description="Practise alone on the shooting range" Target="[INSTALLFOLDER]FrontCraft.exe" Arguments="--map range" WorkingDirectory="INSTALLFOLDER" Icon="FrontCraftIcon" />
        <RegistryValue Root="HKCU" Key="Software\FrontCraft" Name="Shortcuts" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </StandardDirectory>

    <Feature Id="Main">
      <ComponentRef Id="Game" />
      <ComponentRef Id="Mods" />
      <ComponentRef Id="Shortcuts" />
    </Feature>
  </Package>
</Wix>
EOF

