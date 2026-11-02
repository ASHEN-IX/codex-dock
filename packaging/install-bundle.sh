#!/bin/sh
# Install application files only. Never read or modify Codex profiles or chats.
set -eu
bundle=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
prefix=${1:-"$HOME/.local"}
case "$prefix" in
  /*) ;;
  *) echo 'Installation prefix must be an absolute path.' >&2; exit 1 ;;
esac
for binary in codex-dock codex-dock-cli; do
  test -f "$bundle/bin/$binary" || { echo "Missing binary: $binary" >&2; exit 1; }
done
install -d "$prefix/bin" "$prefix/share/applications"
# Preserve the previous executables so an update can be rolled back.
previous=''
for binary in codex-dock codex-dock-cli; do
  if test -e "$prefix/bin/$binary"; then
    if test -z "$previous"; then
      install -d "$prefix/share/codex-dock"
      previous=$(mktemp -d "$prefix/share/codex-dock/previous-XXXXXXXX")
    fi
    cp -p "$prefix/bin/$binary" "$previous/$binary"
  fi
done
staged=''
trap 'test -z "$staged" || rm -f -- "$staged"' EXIT HUP INT TERM
for binary in codex-dock codex-dock-cli; do
  staged=$(mktemp "$prefix/bin/.$binary-XXXXXXXX")
  install -m755 "$bundle/bin/$binary" "$staged"
  mv -f -- "$staged" "$prefix/bin/$binary"
  staged=''
done
for size in 32 48 64 128 256; do
  icons="share/icons/hicolor/${size}x${size}/apps"
  install -d "$prefix/$icons"
  install -m644 "$bundle/$icons/codex-dock.png" "$prefix/$icons/codex-dock.png"
done
install -m644 "$bundle/share/applications/codex-dock.desktop" "$prefix/share/applications/codex-dock.desktop"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$prefix/share/applications" || true
fi
printf 'Installed Codex Dock to %s\nEnsure %s/bin is on PATH.\n' "$prefix" "$prefix"
if test -n "$previous"; then printf 'Previous executables saved in %s\n' "$previous"; fi
