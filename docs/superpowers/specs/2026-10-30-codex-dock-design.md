# Codex Dock — Design

**Date:** 2026-10-30
**Status:** Approved design, pending implementation plan

## Purpose

A desktop app for people running more than one Codex account on one machine. It
manages accounts, launches each in its own Codex Desktop window, and reports
usage per account — all windows reading and writing one shared chat store.

The user's words: *"add accounts and launch each one in a codex page independent
with the same chats"*, plus per-account usage and a tray icon next to Bluetooth
showing which accounts are on and their limits.

## What already exists

Verified on this machine before designing:

- `~/.local/bin/codex-profile` (675 lines, Python): create/delete/rename profiles,
  login, set API key, install desktop launchers, launch Codex. Account plumbing
  largely exists and its logic informs this design.
- A shared store is already live: `~/.codex-profiles/{secondary,work}/{state_5.sqlite,sessions,...}`
  are symlinks into `~/.codex`. Same inode (`412784`) through all three paths,
  WAL mode, 139 threads, `PRAGMA quick_check` = `ok` on each.
- Each profile keeps its own `auth.json`, so each window stays on its own account.

## Key constraints discovered

1. **Codex Desktop is a closed Electron binary** (`/usr/lib/chatgpt/ChatGPT`). Its
   chat UI cannot be embedded in another app.
2. **The app-server exposes a Unix control socket only** — no HTTP. Supported via
   `codex app-server proxy`.
3. **Usage is server-side**, not in local files. It is reachable through the
   app-server JSON-RPC methods documented below.
4. **No npm/crates access from the agent sandbox.** Tauri CLI must be installed by
   the user (`cargo install tauri-cli`).
5. **KDE Plasma on Wayland**: tray = StatusNotifierItem (Tauri-supported).

## Scope

In scope:
- Add / remove / sign in to accounts.
- Launch each account in its own Codex Desktop window.
- Verify and repair the shared store, with backup.
- Per-account usage (5-hour %, weekly %, credits, plan, reset time).
- Tray icon with worst-case colour and a full per-account list.

Out of scope (explicitly):
- Rendering Codex chats inside this app. Not possible; see constraint 1.
- Copying or syncing threads between stores. There is one store; copying would
  break the design.
- Cloud-synced project chats. They stay tied to the account that created them.

## Architecture

```
┌─ Codex Dock (Tauri window) ─────┐
│ Accounts                        │   spawn ──▶ Codex Desktop (account A)
│  ● work      logged in  ▶ Open  │   spawn ──▶ Codex Desktop (account B)
│  ● secondary 87% 5h     ▶ Open  │
│  + Add account                  │   all windows ──▶ ~/.codex/state_5.sqlite
│ Store: shared ✓ 139 threads     │                   (one file, one inode)
└─────────────────────────────────┘
            ▲
      StatusNotifierItem tray: worst-case colour + per-account menu
```

### Units

Each unit has one job, a narrow interface, and is testable without a GUI.

| Unit | Responsibility | Depends on |
|---|---|---|
| `accounts` | list / create / delete profile; report which account owns it and whether it is logged in | profiles dir |
| `store` | verify and repair the shared layout; refuse unsafe mutations | profiles dir, `~/.codex` |
| `windows` | launch Codex for an account; list running windows and their account | `accounts` |
| `usage` | query per-account rate limits and usage via JSON-RPC; cache | app-server sockets |
| `app` | Tauri commands, HTML UI, tray | all of the above |

The backend is shared with a small CLI entry point so `store` and `usage` can be
exercised headless.

## Shared store rules

`store` is the only unit allowed to mutate the layout. Every mutation is
**move-then-link**; nothing is deleted.

Shared via symlink into `~/.codex`:
`state_5.sqlite`, `sessions/`, `archived_sessions/`, `memories_1.sqlite`,
`history.jsonl`.

Kept per profile (never linked):
`auth.json`, `config.toml`, `.codex-global-state.json`.

`.codex-global-state.json` is per-window UI state, so it is merged rather than
shared: the `local-projects` map and `app-server-projects-migration-by-host` are
copied in with `setdefault` semantics, while `local-document-identity` (the
window's account) is preserved. This is what makes the sidebar list the same
projects in every window without changing who is logged in.

Three unsafe states are refused, each discovered during this work:

1. **auth cycle** — `~/.codex/auth.json` resolves into a profile that links back
   to `~/.codex`. Materialise it as a real file first, moving the stale link to
   the backup.
2. **nested profile** — a profile directory inside `~/.codex`. Refuse; would
   recurse.
3. **split store** — a profile `state_5.sqlite` is a real file containing threads
   while the shared DB also has threads. Never overwrite: merge with
   `INSERT OR IGNORE`, repoint `rollout_path`, move the old file to the backup.

**Verification, not assumption.** After repair, open the DB through each profile
path and confirm identical inode and `PRAGMA quick_check` = `ok`. The UI displays
that result; it never shows an unearned checkmark.

Backups: `~/codex-dock-backup-<timestamp>/` holding a tar per profile plus any
displaced file.

## Usage

JSON-RPC over each profile's app-server control socket (`codex app-server proxy`
under that profile's `CODEX_HOME`):

| Method | Fields used |
|---|---|
| `account/rateLimits/read` | `five_hour_limit_percent`, `weekly_limit_percent`, `balance_usage_credits`, reset time |
| `account/usage/read` | `lifetime_tokens`, `peak_daily_tokens`, `current_streak_days`, `daily_usage_buckets` |
| `account/read` | `chatgpt_account_id`, `chatgpt_plan_type` |

Corroborating response headers seen in the binary: `x-codex-credits-balance`,
`x-codex-rate-limit-reached-type`, `x-codex-active-limit`.

Behaviour:
- If no app-server is running for an account, start one on demand for the read.
- Cache 60s, shared by window and tray so they cannot disagree.
- On failure keep the last value, greyed, with "last checked Nm ago". Never
  render a fabricated 0%.

**Tray**: single StatusNotifierItem. Colour = worst `five_hour_limit_percent`
across accounts (green < 60, amber < 85, red otherwise). Menu lists every account
with its 5-hour and weekly bars and an "Open window" action.

Tray icon rendering: a 22×22 PNG generated at build time for each colour state,
shipped with the app. No runtime image library, no SVG in the tray (StatusNotifier
on Plasma does not reliably render SVG). If no account has a valid reading, the
icon uses a neutral grey variant and the tooltip says "usage unavailable".

## Failure handling

| Failure | Behaviour |
|---|---|
| app-server socket absent | start one; on failure show "usage unavailable", keep last value greyed |
| account not logged in | listed with "Sign in"; usage columns `—` |
| store broken | banner + "Repair"; runs move-then-link and re-verifies |
| window already open for an account | focus it instead of spawning a duplicate (match on `--user-data-dir`) |

## Testing

Pure Rust units with injected paths, run against a temp `CODEX_HOME`.

Must-have regression tests, each reproducing a real trap from this work:
1. cyclic `auth.json` is detected and materialised, not followed.
2. nested profile directory is refused.
3. split store with threads on both sides merges and preserves the old file.
4. repair is idempotent — running twice changes nothing the second time.
5. rolled-out `rollout_path` is repointed to the shared store.

Usage parsing is tested against recorded JSON-RPC fixtures, not the live API.

## Build prerequisites

`cargo install tauri-cli` (user-run, network), `webkit2gtk-4.1`, `gtk3`,
`libsoup3` — all present on this machine. Rust 1.99.1, Node 24.20.0 available.

## Migration from the current setup

The shared store is already applied and healthy, so first run must be a no-op:
`store` verifies, reports "shared, 139 threads, up to date", and writes nothing.
Codex Dock does not re-run the shell procedure; it adopts the existing layout.

Detection is by inode: a profile is already shared when its `state_5.sqlite`
resolves to the same inode as `~/.codex/state_5.sqlite`. Anything else is a split
store and follows the merge rule.

The one-time script `~/Downloads/share-codex-store.sh` stays as the headless
equivalent and is not invoked by the app.

