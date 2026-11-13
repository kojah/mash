#!/bin/sh
# Install the latest Mash release binary:
#   curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh | sh
# Set MASH_VERSION (for example v0.8.0) to pin a release and MASH_INSTALL_DIR to
# choose the directory (default ~/.local/bin). Verifies the release checksum.
set -eu

repository=kojah/mash
directory=${MASH_INSTALL_DIR:-$HOME/.local/bin}
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  *) echo "mash: no prebuilt binary for $(uname -s) $(uname -m); build from source with cargo" >&2; exit 1 ;;
esac
if [ -n "${MASH_VERSION:-}" ]; then
  base="https://github.com/$repository/releases/download/$MASH_VERSION"
else
  base="https://github.com/$repository/releases/latest/download"
fi
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM
archive="mash-$target.tar.gz"
curl -fsSL "$base/$archive" -o "$scratch/$archive"
curl -fsSL "$base/SHA256SUMS" -o "$scratch/SHA256SUMS"
(
  cd "$scratch"
  grep " $archive\$" SHA256SUMS > expected
  if command -v sha256sum > /dev/null; then sha256sum -c expected; else shasum -a 256 -c expected; fi
) > /dev/null
tar -xzf "$scratch/$archive" -C "$scratch"
[ -f "$scratch/mash" ] && [ -f "$scratch/mash-server" ] || {
  echo 'Release archive must contain mash and mash-server.' >&2; exit 1;
}
mkdir -p "$directory"
install -m 0755 "$scratch/mash" "$directory/mash"
install -m 0755 "$scratch/mash-server" "$directory/mash-server"
echo "Installed $directory/mash and $directory/mash-server"
case ":$PATH:" in
  *":$directory:"*) ;;
  *) echo "Add $directory to your PATH." ;;
esac
echo "Next: install the local Qdrant search service that indexing and search need, then set up Mash:"
echo "  mash qdrant install"
echo "  mash init"
