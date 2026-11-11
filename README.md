# Codex Dock

Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md),
[security reporting](SECURITY.md), and the [community conduct](CODE_OF_CONDUCT.md).
CI tests and builds the Linux app without using real account credentials.
Maintainer settings and draft releases are documented in
[docs/maintaining.md](docs/maintaining.md).

A desktop app for people running more than one Codex account on one machine. It
lists accounts, launches each in its own Codex Desktop window, and reports
per-account usage from a tray icon — all windows sharing one chat store.

## Why

Codex Desktop keeps a single profile per `CODEX_HOME`. To run two accounts at
once you need separate homes for credentials but one database for chats, so the
same threads appear in every window. Codex Dock manages that split for you.

## Install

To install a prebuilt package without cloning this repository or installing Rust,
see [binary package installation](packaging/INSTALL.md). Contributors can build
AppImage, Debian and RPM bundles with `make release`, a Linux archive with
`make package-linux`, or an optional Arch package with `make package-arch`. These package targets do not change account or chat data.
Prebuilt assets must be supplied by a release publisher; these commands alone
do not publish them.

Building from source requires Rust, `sqlite3` on `PATH`, and WebKitGTK 4.1 +
GTK3 development libraries. See the installation guide for distribution details.

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

`make release` builds AppImage, Debian and RPM bundles. The Linux packages
workflow builds on Ubuntu 22.04 and tests the core on Ubuntu 22.04 and 24.04.
Artifacts need distribution-specific runtime validation before declaring support.

### Running as an installed app

- **Ping** sends one minimal message for a signed-in account to open an idle
  usage window, then refreshes the server's usage and reset time. It uses
  allowance. API-key profiles are excluded. An explicit click sends one message,
  even if a usage reset time already exists. The actual model reply confirms
  delivery. This is a background request; use **Open** to open a desktop window.
  It never resets an active limit or invents a five-hour countdown. Missing or
  delayed reset times remain unconfirmed until a fresh server reading arrives.
  Requires Codex CLI 0.166 or later with `exec --ignore-user-config` support.
  Pings run in a temporary directory, with ephemeral history, read-only sandbox,
  ignored user config/rules and disabled shell, hooks, plugins and delegation.
  Manual Ping has a one-minute cooldown per account, a 90-second timeout,
  and no automatic retries. A failed/timed-out request may still use
  allowance. Existing projects, config and saved chats are not used as context.

- **Auto-ping** is a separate saved setting, off by default. Enabling it sends
  one background ping to each eligible account that Dock has not pinged for the
  current period. While Dock is running (including docked in the tray), it checks
  every minute and sends again only after a successfully recorded server reset
  time has passed. Successful manual pings count too, including after a restart.
  Accounts with unavailable usage, no five-hour limit, or exhausted allowance
  are skipped. Unknown reset times, failures and interrupted requests pause the
  account; use manual Ping, or toggle the option off and on to retry failures.
  Turning it off stops future automatic requests; an in-flight ping may finish.
  Each row shows its last reply or error. This uses allowance, and does not open
  desktop windows or reset an existing usage period.

- **Closing the window docks it.** The tray keeps ticking and keeps the usage
  numbers live; the process keeps running. Quit from the tray menu.
- **One instance.** Launching the app again (the shortcut, the launcher, the
  tray's *Open Codex Dock*) raises the running window instead of starting a
  second tray icon.
- **Left-click the tray** to open the dashboard; right-click for the menu.
- **Opening an account** raises its Codex window if it is already open, and
  only launches a new one otherwise. Dashboard, tray, and CLI use the same
  serialized launch path, including windows opened outside Dock. On KDE,
  opening a window switches to its virtual desktop. A running process without
  an accessible window is reported as an error instead of a successful open.
- **One row per named profile.** Named profiles remain visible and open their
  own windows even when their current sign-in identities match. A matching
  `default` alias is hidden in favor of a named profile; all profile directories
  and credentials remain on disk.
- **Sign-in opens your browser.** Keep Dock's device-code dialog open, enter
  its code in the browser, and approve the connection. Logging in to the
  ChatGPT website alone does not save credentials for a Dock profile.

The headless CLI needs no GUI stack:

```bash
make cli
./target/debug/codex-dock-cli status
```

## CLI

| Command | What it does |
|---|---|
| `status` | Store health, thread/project counts, accounts and login state |
| `list` | Local profile names; provider account IDs are omitted |
| `usage` | Per-account plan, 5-hour %, weekly %, credits |
| `launch <name>` | Open Codex Desktop for one account |
| `running [name]` | PIDs of open windows, optionally filtered by a local profile name |
| `repair` | Verify and repair the shared store, writing a backup (disabled in Phase 1.1; use `status` for read-only diagnostics) |

`status` against a healthy store writes nothing.

Routine CLI output omits provider account IDs and per-window data paths so they
do not end up in captured terminal logs. `status` summarizes configured/signed-in
account counts; use `list` to request local profile names. `running` prints process IDs; use
`codex-dock-cli running work` to select one local profile without printing its
process-derived account metadata. `launch` reports the opened window's PID.

## How the shared store works

Per profile, these are symlinked into `~/.codex` so every window reads the same
data:

`state_5.sqlite`, `sessions`, `archived_sessions`, `memories_1.sqlite`,
`history.jsonl`

These stay per profile, so each window keeps its own account:

`auth.json`, `config.toml`, `.codex-global-state.json`

New profiles import existing local projects and sidebar order automatically.
Existing profiles import missing projects before their next fresh launch; close
the account window first. Opening an already-running window only focuses it.
An import backs up an existing state file before replacing it atomically, and
does not copy another account's identity or migration flags.

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
`~/codex-dock-backups/<timestamp>/`, one directory per operation under a
single root.

Mutating shared-store repair is disabled in Phase 1.1 to protect against concurrent-writer
hazards and WAL coordination races while Codex windows may be open or background tasks running.
Read-only diagnostics (`status` / `store::inspect` / `db::quick_check`) run in strict `-readonly`
mode without mutating database or WAL bytes. Repair verifies by **inode** through every profile
path and runs `PRAGMA quick_check`. The UI only shows a checkmark when that verification passed.

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

149 tests. The ones worth knowing about reproduce real traps: auth cycles,
nested profiles, split stores, repair idempotency, `rollout_path` repointing,
absent usage fields staying `None`, stale readings never becoming zero, path
traversal and Unicode/space names, and per-platform capability reporting
("`platform_test` and `platform_capability_test` refuse silently-unsupported
features; cross-OS guarantees are pinned in CI on all three runners").

## Build note

Codex Desktop is a closed Electron binary, so its chat UI cannot be embedded
here. Codex Dock launches it and shares its store; it does not render chats.
