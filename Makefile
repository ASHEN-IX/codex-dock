# Codex Dock
#
#   make test      core crate test suite (works offline, no toolchain fetch)
#   make cli       build the headless CLI
#   make setup     install the Tauri CLI (network, once)
#   make app       build and run the desktop app
#   make status    inspect the live shared store

CARGO ?= cargo

.PHONY: test cli setup app status clean

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

clean:
	$(CARGO) clean
