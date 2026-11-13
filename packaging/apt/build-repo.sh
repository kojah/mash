#!/usr/bin/env bash
# Add one release's .deb packages to a signed apt repository and refresh it.
#
#   packaging/apt/build-repo.sh VERSION DIST REPO
#
# DIST holds the release archives (mash-<target>.tar.gz); REPO is the published
# tree (the gh-pages `apt/` directory), kept across releases so older versions
# stay installable. Signs with the default secret key in $GNUPGHOME, or
# $APT_SIGNING_KEY when set. Writes the built .deb files to DIST as well.
set -euo pipefail

version=${1#v}
dist=$(realpath "$2")
repo=$3
mkdir -p "$repo"
repo=$(realpath "$repo")
signing=(--batch --yes)
[[ -n "${APT_SIGNING_KEY:-}" ]] && signing+=(--local-user "$APT_SIGNING_KEY")

scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT

for pair in amd64:x86_64-unknown-linux-gnu arm64:aarch64-unknown-linux-gnu; do
  arch=${pair%%:*}
  target=${pair#*:}
  root="$scratch/$arch"
  mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/doc/mash"
  tar -xzf "$dist/mash-$target.tar.gz" -C "$root/usr/bin" mash mash-server
  chmod 755 "$root/usr/bin/mash" "$root/usr/bin/mash-server"
  cp "$(dirname "$0")/copyright" "$root/usr/share/doc/mash/copyright"
  cat > "$root/DEBIAN/control" <<CONTROL
Package: mash
Version: $version
Architecture: $arch
Maintainer: kojah <mash-release@users.noreply.github.com>
Homepage: https://github.com/kojah/mash
Section: utils
Priority: optional
Description: Search your coding agents' past conversations
 Mash indexes Claude Code, Codex, Copilot, Cursor, OpenCode and Antigravity
 chat histories locally and searches them from a JSON CLI and an MCP server.
 Also installs mash-server, the optional service for sharing an index between
 machines. Run "mash qdrant install", then "mash init", after installing.
CONTROL
  deb="mash_${version}_${arch}.deb"
  dpkg-deb --root-owner-group --build "$root" "$dist/$deb" >/dev/null
  mkdir -p "$repo/pool/main/m/mash"
  cp "$dist/$deb" "$repo/pool/main/m/mash/$deb"
done

cd "$repo"
for arch in amd64 arm64; do
  directory="dists/stable/main/binary-$arch"
  mkdir -p "$directory"
  apt-ftparchive --arch "$arch" packages pool > "$directory/Packages"
  gzip -9nkf "$directory/Packages"
done
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=mash \
  -o APT::FTPArchive::Release::Label=mash \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
  -o APT::FTPArchive::Release::Components=main \
  release dists/stable > "$scratch/Release"
mv "$scratch/Release" dists/stable/Release
gpg "${signing[@]}" --clearsign -o dists/stable/InRelease dists/stable/Release
gpg "${signing[@]}" --detach-sign --armor -o dists/stable/Release.gpg dists/stable/Release
gpg "${signing[@]}" --export ${APT_SIGNING_KEY:+"$APT_SIGNING_KEY"} > mash-archive-keyring.gpg
echo "apt repository updated with mash $version in $repo"
