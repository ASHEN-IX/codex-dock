#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
arch=$(uname -m)
target_dir=${CARGO_TARGET_DIR:-target}
release="$target_dir/release"
output="$root/dist/codex-dock-linux-$arch.tar.gz"
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT HUP INT TERM
bundle="$stage/codex-dock"
install -d "$bundle/bin" "$bundle/share/applications" "$root/dist"
for binary in codex-dock codex-dock-cli; do
  install -m755 "$release/$binary" "$bundle/bin/$binary"
done
for size in 32 48 64 128 256; do
  icons="$bundle/share/icons/hicolor/${size}x${size}/apps"
  install -d "$icons"
  install -m644 "src-tauri/icons/${size}x${size}.png" "$icons/codex-dock.png"
done
install -m644 packaging/codex-dock.desktop "$bundle/share/applications/codex-dock.desktop"
install -m755 packaging/install-bundle.sh "$bundle/install.sh"
install -m644 packaging/INSTALL.md "$bundle/INSTALL.md"
licenses="$bundle/share/licenses/codex-dock"
install -d "$licenses"
for notice in LICENSE NOTICE THIRD-PARTY-NOTICES.md THIRD-PARTY-NOTICES.txt; do
  install -m644 "$notice" "$licenses/$notice"
done
cp -R third-party/native "$licenses/native"
# All notice files have ordinary read-only data modes in archives.
find "$licenses" -type d -exec chmod 755 {} +
find "$licenses" -type f -exec chmod 644 {} +
tar -czf "$output" -C "$stage" codex-dock
(cd "$root/dist" && sha256sum "$(basename "$output")" > "$(basename "$output").sha256")
printf 'Built %s\n' "$output"
