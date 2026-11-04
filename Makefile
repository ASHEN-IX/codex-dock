# Codex Dock
#
#   make test      core crate test suite (works offline, no toolchain fetch)
#   make cli       build the headless CLI
#   make setup     install the Tauri CLI (network, once)
#   make app       build and run the desktop app
#   make status    inspect the live shared store
#   make install   build release, install to ~/.local, add launcher + autostart
#   make uninstall remove everything install added

CARGO ?= cargo
PREFIX ?= $(HOME)/.local
BIN     = codex-dock
DESKTOP = $(PREFIX)/share/applications/codex-dock.desktop
AUTOSTART = $(HOME)/.config/autostart/codex-dock.desktop

.PHONY: test cli setup app status release package-linux package-arch install autostart uninstall clean check-core check

check-core:
	$(CARGO) fmt --all -- --check
	$(CARGO) test --locked -p codex-dock-core
	python3 scripts/check-ui.py

check: check-core
	$(CARGO) clippy --locked --workspace --all-targets -- -D warnings
	$(CARGO) build --locked --workspace

test:
	$(CARGO) test -p codex-dock-core

cli:
	$(CARGO) build -p codex-dock-core
	@echo "built: target/debug/codex-dock-cli"

setup:
	$(CARGO) install tauri-cli --version "^2"

app:
	$(CARGO) tauri dev

status: cli
	@./target/debug/codex-dock-cli status

release:
	$(CARGO) tauri build --bundles appimage,deb,rpm

# Downloadable binaries: end users do not need Git, Cargo, or the source tree.
package-linux:
	$(CARGO) build --locked --release -p codex-dock -p codex-dock-core
	sh packaging/package-linux.sh

package-arch: package-linux
	sh packaging/package-arch.sh

# Installs the release binary under ~/.local, so no root is needed and the
# launcher's Exec path stays valid.
install:
	$(CARGO) build --release -p codex-dock -p codex-dock-core
	install -d $(PREFIX)/bin
	install -m755 target/release/$(BIN) $(PREFIX)/bin/$(BIN)
	install -m755 target/release/codex-dock-cli $(PREFIX)/bin/codex-dock-cli
	install -d $(PREFIX)/share/icons/hicolor
	for s in 32 48 64 128 256; do \
	  install -d $(PREFIX)/share/icons/hicolor/$${s}x$${s}/apps; \
	  install -m644 src-tauri/icons/$${s}x$${s}.png \
	    $(PREFIX)/share/icons/hicolor/$${s}x$${s}/apps/$(BIN).png; \
	done
	install -d $(PREFIX)/share/applications
	install -m644 packaging/codex-dock.desktop $(DESKTOP)
	@command -v update-desktop-database >/dev/null && \
	  update-desktop-database $(PREFIX)/share/applications 2>/dev/null || true
	@echo "installed: $(PREFIX)/bin/$(BIN)"
	@echo "launcher:  $(DESKTOP)"
	@echo "run 'make autostart' to start it at login"

autostart:
	install -d $(dir $(AUTOSTART))
	install -m644 packaging/codex-dock.desktop $(AUTOSTART)
	@echo "autostart: $(AUTOSTART)"

uninstall:
	rm -f $(PREFIX)/bin/$(BIN) $(PREFIX)/bin/codex-dock-cli $(DESKTOP) $(AUTOSTART)
	rm -f $(PREFIX)/share/icons/hicolor/*/apps/$(BIN).png
	@echo "removed"

clean:
	$(CARGO) clean
