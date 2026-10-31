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

67 tests. The ones worth knowing about reproduce real traps: auth cycles,
nested profiles, split stores, repair idempotency, `rollout_path` repointing,
absent usage fields staying `None`, and stale readings never becoming zero.

## Build note

Codex Desktop is a closed Electron binary, so its chat UI cannot be embedded
here. Codex Dock launches it and shares its store; it does not render chats.
