#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
archive="codex-dock-linux-$(uname -m).tar.gz"
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT HUP INT TERM
cp "$root/dist/$archive" "$stage/$archive"
checksum=$(sha256sum "$stage/$archive" | cut -d ' ' -f1)
sed "s/sha256sums=('SKIP')/sha256sums=('$checksum')/" "$root/packaging/arch/PKGBUILD" > "$stage/PKGBUILD"
cd "$stage"
PKGDEST="$root/dist" makepkg --force --nodeps
printf 'Built Arch package in %s/dist\n' "$root"
