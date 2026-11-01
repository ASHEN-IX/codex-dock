# Codex Dock

A desktop app for people running more than one Codex account on one machine. It
lists accounts, launches each in its own Codex Desktop window, and reports
per-account usage from a tray icon — all windows sharing one chat store.

## Why

Codex Desktop keeps a single profile per `CODEX_HOME`. To run two accounts at
once you need separate homes for credentials but one database for chats, so the
same threads appear in every window. Codex Dock manages that split for you.

## Install

Requires Rust, `sqlite3` on `PATH`, and `webkit2gtk-4.1` + `gtk3` for the shell.

```bash
make setup     # installs the Tauri CLI (network, once)
make app       # build and run the desktop app
```

To install it as a real desktop app — launcher entry, hicolor icons, and
`codex-dock` on `PATH`, no root required:

```bash
make install     # -> ~/.local/bin/codex-dock + app launcher
make autostart   # start it docked when you log in (optional)
make uninstall   # removes the binary, launcher, and autostart entry
```

`make release` builds distributable bundles (`deb` + `AppImage`) instead.

### Running as an installed app

- **Closing the window docks it.** The tray keeps ticking and keeps the usage
  numbers live; the process keeps running. Quit from the tray menu.
- **One instance.** Launching the app again (the shortcut, the launcher, the
  tray's *Open Codex Dock*) raises the running window instead of starting a
  second tray icon.
- **Left-click the tray** to open the dashboard; right-click for the menu.
- **Opening an account** raises its Codex window if it is already open, and
  only launches a new one otherwise. Dashboard, tray, and CLI use the same
  serialized launch path, including windows opened outside Dock.
- **One row per account.** Profiles with the same signed-in account identity
  appear once. A named profile takes precedence over a `default` alias; all
  profile directories and credentials remain on disk.

The headless CLI needs no GUI stack:

```bash
make cli
./target/debug/codex-dock-cli status
```

## CLI

| Command | What it does |
|---|---|
| `status` | Store health, thread/project counts, accounts and login state |
| `list` | One line per account: name and account id |
| `usage` | Per-account plan, 5-hour %, weekly %, credits |
| `launch <name>` | Open Codex Desktop for one account |
| `running` | Windows currently open, mapped back to accounts |
| `repair` | Verify and repair the shared store, writing a backup |

`status` against a healthy store writes nothing.

## How the shared store works

Per profile, these are symlinked into `~/.codex` so every window reads the same
data:

`state_5.sqlite`, `sessions`, `archived_sessions`, `memories_1.sqlite`,
`history.jsonl`

These stay per profile, so each window keeps its own account:

`auth.json`, `config.toml`, `.codex-global-state.json`

`.codex-global-state.json` is merged rather than shared: the project list is
copied in with `setdefault` semantics while `local-document-identity` (which
account the window is on) is preserved.

### Three unsafe states, refused or repaired

| State | Detection | Action |
|---|---|---|
| **auth cycle** | `~/.codex/auth.json` resolves into a profile that links back | materialise as a real file, keep the stale link in the backup |
| **nested profile** | a profile directory inside `~/.codex` | refuse — linking would recurse |
| **split store** | a profile DB has threads the shared DB also has | merge with `INSERT OR IGNORE`, repoint `rollout_path`, preserve the old file |

Every mutation is move-then-link. Nothing is deleted; displaced files go to
`~/codex-dock-backup-<timestamp>/`.

Repair verifies by **inode** through every profile path and runs
`PRAGMA quick_check`. The UI only shows a checkmark when that verification
passed.

## Usage

Read over the app-server's JSON-RPC on stdio, one session per account:

| Method | Fields used |
|---|---|
| `account/rateLimits/read` | `rateLimits.primary.usedPercent` (5-hour), `secondary.usedPercent` (weekly), `credits.balance`, `planType`, `primary.resetsAt` |
| `account/read` | `account.planType` |
| `account/usage/read` | `summary.lifetimeTokens`, `peakDailyTokens`, `currentStreakDays` |

The transport is **newline-delimited JSON** (one object per line), not LSP
framing. `initialize` must carry `clientInfo`, and an `initialized`
notification follows it.

Readings are cached for 60 seconds in one cache shared by the window and the
tray, so they cannot disagree. If a refresh fails the last good numbers are
kept and labelled with their age — a value is never fabricated as 0%.

Tray colour is the worst 5-hour percentage across accounts: green `< 60`,
amber `< 85`, red otherwise, neutral when no account has a reading.

The tray icon itself is a **ring gauge**, not a coloured dot: the filled arc is
that same worst percentage, so you can read how close you are to the limit
without opening anything. The window header shows the same ring, and each
account gets filled bars on a shared scale.

The menu repeats the layout in two lines per account — name and plan, then a
ten-cell text gauge for the 5-hour and weekly windows — because native menus
cannot draw a progress bar. Reset time, staleness, and credits follow on a
third line when they exist.

## Layout

```
core/       all logic: accounts, store, db, windows, usage, tray, rpc
core/src/bin/codex-dock-cli.rs   headless front-end
src-tauri/  Tauri shell: commands, tray, 60s tick
src/        the window UI
docs/       design spec and implementation plan
```

Every unit takes an injected `Paths`, so tests run against a temp directory
rather than your real `~/.codex`.

## Tests

```bash
make test     # cargo test -p codex-dock-core
```

78 tests. The ones worth knowing about reproduce real traps: auth cycles,
nested profiles, split stores, repair idempotency, `rollout_path` repointing,
absent usage fields staying `None`, and stale readings never becoming zero.

## Build note

Codex Desktop is a closed Electron binary, so its chat UI cannot be embedded
here. Codex Dock launches it and shares its store; it does not render chats.
