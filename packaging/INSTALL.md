# Install a binary package

Codex Dock runs without its Git checkout. You still need Codex Desktop, the
Codex CLI, SQLite, GTK3, WebKitGTK 4.1, and an AppIndicator runtime installed.
The raw Linux archive uses the packager's host ABI; choose a compatible
distribution and CPU architecture. Use the CI-built formats below for broader
distribution coverage. No source checkout is needed to run an installed app.

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

## AppImage, Debian and RPM packages

The primary distribution path is the Linux CI workflow, which builds on Ubuntu
22.04 for an older glibc baseline and also compiles/tests on Ubuntu 24.04.
Download the workflow artifacts from the fork's **Actions → Linux packages** page,
or use release assets once a publisher attaches them to a release.

| Distribution family | Format | Install or launch |
| --- | --- | --- |
| Ubuntu, Debian, Mint, Pop!_OS | `.deb` | `sudo apt install ./codex-dock_*.deb` |
| Fedora, RHEL derivatives | `.rpm` | `sudo dnf install ./codex-dock-*.rpm` |
| openSUSE | `.rpm` | `sudo zypper install ./codex-dock-*.rpm` |
| Other compatible Linux systems | `.AppImage` | `chmod +x ./Codex*.AppImage`, then run it |
| Arch and derivatives | `.pkg.tar.zst` | See the optional Arch package below |

The AppImage still depends on compatible host components such as glibc and
may need FUSE. Extracting with `--appimage-extract` is an alternative when FUSE
is unavailable. Formats and build jobs are not a guarantee that every distro,
architecture, compositor or older release has been runtime tested. The current
CI builds x86_64; ARM64 needs a separate native build and validation. Flatpak
support is not implemented: it needs explicit access to host Codex executables
and profile files, so a sandbox manifest cannot safely be guessed.

Codex Desktop is discovered on PATH as `chatgpt`, `codex-desktop` or `ChatGPT`.
For another installation location, set `CODEX_DESKTOP_BIN` to one executable
path, for example `/home/you/Applications/Codex.AppImage`. It is not a shell
command and does not accept embedded arguments. Ensure the variable reaches
the GUI process as well as your shell. Codex Desktop and CLI must be installed
separately on the host.

## Arch Linux and derivatives (optional)

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
make release        # AppImage + Debian + RPM via the Tauri CLI
make package-linux  # dist/*.tar.gz and SHA-256 checksum
make package-arch   # also dist/*.pkg.tar.zst; requires makepkg
```

These commands build release binaries with the committed Cargo lockfile.
`package-arch` wraps the binaries in a package; it skips build-host dependency
checking, but the generated package declares runtime dependencies for pacman.
Only application binaries, launcher, icons, installer and this guide enter the
bundle. Publish the files from `dist/` as release assets after checking them on
the target distribution.
