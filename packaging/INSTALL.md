# Install a binary package

Codex Dock runs without its Git checkout. You still need Codex Desktop, the
Codex CLI, SQLite, GTK3, WebKitGTK 4.1, and an AppIndicator runtime installed.
The Linux bundle is built on the packager's system; use a compatible distribution
and CPU architecture. It is not a universally compatible AppImage.

## Arch Linux / Omarchy

Install the downloaded `.pkg.tar.zst` file with:

```sh
sudo pacman -U ./codex-dock-0.1.0-1-x86_64.pkg.tar.zst
```

Pacman resolves dependencies from configured repositories. No Git or Rust is
needed to install the binary package. The package owns only `/usr/bin` and
`/usr/share` files; it has no install hooks or profile migration step.

If an older copy exists in `~/.local/bin`, it can take precedence over `/usr/bin`.
Keep using that copy until ready to switch; then quit Dock, move the two old
executables and `~/.local/share/applications/codex-dock.desktop` to a backup,
and launch `/usr/bin/codex-dock`. Account data stays in place.

## User installation without root

Download the Linux archive and its checksum file into the same directory:

```sh
sha256sum -c codex-dock-linux-x86_64.tar.gz.sha256
tar -xzf codex-dock-linux-x86_64.tar.gz
sh codex-dock/install.sh
```

The installer defaults to `~/.local`. An absolute install prefix may be passed
as its first argument. Ensure its `bin` directory is on PATH. Launch Codex Dock
from your application menu or run `codex-dock`. The extracted bundle can then
be removed.

Before updating, quit Dock through its tray menu. The installer saves existing
executables under `<prefix>/share/codex-dock/previous-*`. To roll back, quit
Dock and copy both saved executables back into `<prefix>/bin`.

## Account and chat data

Installing or updating either package does not create, repair, migrate, or remove
profiles. Codex Dock continues using `~/.codex`, `~/.codex-profiles`, and
`~/.config/Codex-*`. These directories must never be included in release assets.

For a recoverable backup, quit Dock and all Codex windows, then preserve those
directories together, including symlinks and file permissions. Treat backups as
sensitive because they contain credentials and chats. Removing the app package
does not remove these directories. Store repair is a separate action inside Dock.

## For contributors

```sh
make package-linux  # dist/*.tar.gz and SHA-256 checksum
make package-arch   # also dist/*.pkg.tar.zst; requires makepkg
```

These commands build release binaries with the committed Cargo lockfile.
`package-arch` wraps the binaries in a package; it skips build-host dependency
checking, but the generated package declares runtime dependencies for pacman.
Only application binaries, launcher, icons, installer and this guide enter the
bundle. Publish the files from `dist/` as release assets after checking them on
the target distribution.
