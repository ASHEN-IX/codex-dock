# Codex Dock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a desktop app that manages multiple Codex accounts, launches each in its own Codex Desktop window over one shared chat store, and reports per-account usage with a tray indicator.

**Architecture:** A workspace with a pure-Rust `core` crate holding all logic (accounts, shared-store verify/repair, window launching, usage) and thin front-ends over it: a CLI binary for headless use and a Tauri shell added last. All filesystem work takes an injected `Paths`, so every unit is tested against a temp directory.

**Tech Stack:** Rust 1.99.1, `serde`/`serde_json` (both cached offline), `std::process::Command` for the `sqlite3` binary, Tauri 2 for the shell.

**Spec:** `docs/superpowers/specs/2026-10-30-codex-dock-design.md`

## Global Constraints

- Only `serde` and `serde_json` may be added as dependencies to `core`. No `rusqlite` (not cached) — query SQLite through the `sqlite3` binary.
- Never delete files. Every mutation is move-then-link; displaced files go to `~/codex-dock-backup-<timestamp>/`.
- Per profile, only these are shared by symlink into `~/.codex`: `state_5.sqlite`, `sessions`, `archived_sessions`, `memories_1.sqlite`, `history.jsonl`.
- Per profile, these are never linked: `auth.json`, `config.toml`, `.codex-global-state.json`.
- Never render a fabricated usage value. Absent data is `None` plus a timestamp, never `0`.
- Tray colours: green `< 60`, amber `< 85`, red otherwise, on the worst `five_hour_percent`.
- First run against an already-shared store must write nothing.

## Review Focus

- Profile dir nested inside `~/.codex` → would recurse when linked; `inspect` must report `Nested` and `repair` must refuse.
- `~/.codex/auth.json` symlinked into a profile that links back → infinite resolution; must be materialised, not followed.
- A profile DB with threads while the shared DB also has threads → silent data loss if overwritten; must merge and preserve the original file.
- An account never opened → its app-server socket is absent; usage must read as unavailable, not as zero percent.
- A malformed or empty `auth.json` → must list the account as not logged in rather than failing the whole listing.
- `sqlite3` missing from `PATH` → every DB read must surface a clear error instead of a panic.

---

### Task 1: Core crate skeleton and injected paths

**Files:**
- Create: `Cargo.toml` (workspace), `core/Cargo.toml`, `core/src/lib.rs`, `core/src/paths.rs`
- Test: `core/src/paths.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: nothing.
- Produces: `Paths { base: PathBuf, profiles: PathBuf }` with `Paths::real() -> Paths`, `Paths::new(base: impl Into<PathBuf>, profiles: impl Into<PathBuf>) -> Paths`, `Paths::backup_dir(&self, stamp: &str) -> PathBuf`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn real_paths_point_at_codex_and_profiles() {
    let p = Paths::real();
    assert!(p.base.ends_with(".codex"));
    assert!(p.profiles.ends_with(".codex-profiles"));
}

#[test]
fn backup_dir_is_dated_under_home() {
    let p = Paths::new("/tmp/x/.codex", "/tmp/x/.codex-profiles");
    let b = p.backup_dir("20261030-160949");
    assert!(b.ends_with("codex-dock-backup-20261030-160949"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p codex-dock-core paths --offline`
Expected: FAIL — `Paths` not defined.

- [ ] **Step 3: Implement**

Workspace `Cargo.toml` with members `core` (add `src-tauri` in Task 8). `Paths::real()` reads `$HOME`; `backup_dir` returns `$HOME/codex-dock-backup-<stamp>`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p codex-dock-core paths --offline`
Expected: PASS, 2 tests.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml core && git commit -m "feat: core crate with injected paths"
```

---

### Task 2: Account listing and login state

**Files:**
- Create: `core/src/accounts.rs`, `core/src/error.rs`
- Modify: `core/src/lib.rs`
- Test: `core/src/accounts.rs` (inline)

**Interfaces:**
- Consumes: `Paths`.
- Produces: `Error` (enum with `Io(std::io::Error)`, `Msg(String)`); `Account { name: String, path: PathBuf, account_id: Option<String>, logged_in: bool }`; `accounts::list(&Paths) -> Result<Vec<Account>, Error>`; `accounts::auth_info(path: &Path) -> (Option<String>, bool)`.

- [ ] **Step 1: Write the failing test**

```rust
fn fixture(dir: &Path, name: &str, auth: Option<&str>) -> PathBuf {
    let d = dir.join(".codex-profiles").join(name);
    std::fs::create_dir_all(&d).unwrap();
    if let Some(a) = auth { std::fs::write(d.join("auth.json"), a).unwrap(); }
    d
}

#[test]
fn lists_profiles_with_account_id() {
    let t = std::env::temp_dir().join(format!("cd-acc-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    fixture(&t, "work", Some(r#"{"tokens":{"account_id":"9c50"}}"#));
    fixture(&t, "personal", Some(r#"{"tokens":{"account_id":"a423"}}"#));
    let mut got = accounts::list(&p).unwrap();
    got.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].name, "personal");
    assert_eq!(got[0].account_id.as_deref(), Some("a423"));
    assert!(got[0].logged_in);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn malformed_auth_is_logged_out_not_an_error() {
    let (id, logged_in) = accounts::auth_info(Path::new("/nonexistent/auth.json"));
    assert!(id.is_none());
    assert!(!logged_in);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p codex-dock-core accounts --offline`
Expected: FAIL — module not defined.

- [ ] **Step 3: Implement**

`auth_info` parses `auth.json` and returns `tokens.account_id`; any io/parse failure yields `(None, false)`. `list` skips entries starting with `.` and resolves symlinks.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p codex-dock-core accounts --offline`
Expected: PASS, 2 tests.

- [ ] **Step 5: Commit**

```bash
git add core && git commit -m "feat: list accounts and detect login state"
```

---

### Task 3: Create and remove accounts

**Files:**
- Modify: `core/src/accounts.rs`
- Test: `core/src/accounts.rs` (inline)

**Interfaces:**
- Consumes: `accounts::list`.
- Produces: `accounts::create(&Paths, name: &str, clone_from: Option<&str>) -> Result<Account, Error>`; `accounts::remove(&Paths, name: &str, backup: &Path) -> Result<PathBuf, Error>` returning where the profile was moved.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn create_rejects_duplicate_and_bad_names() {
    let t = std::env::temp_dir().join(format!("cd-create-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    accounts::create(&p, "work", None).unwrap();
    assert!(accounts::create(&p, "work", None).is_err());
    assert!(accounts::create(&p, "bad name!", None).is_err());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn remove_moves_the_profile_never_deletes() {
    let t = std::env::temp_dir().join(format!("cd-rm-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    accounts::create(&p, "work", None).unwrap();
    let bak = p.backup_dir("stamp");
    let moved = accounts::remove(&p, "work", &bak).unwrap();
    assert!(moved.exists(), "profile must survive in the backup");
    assert!(!t.join(".codex-profiles/work").exists());
    std::fs::remove_dir_all(&t).ok();
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p codex-dock-core create --offline; cargo test -p codex-dock-core remove --offline`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement**

Name rule: `[A-Za-z0-9_-]+`. `create` makes the directory and copies `config.toml`/`rules`/`skills` from `clone_from` when given. `remove` uses `std::fs::rename` into the backup dir.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p codex-dock-core --offline`
Expected: PASS, all tests.

- [ ] **Step 5: Commit**

```bash
git add core && git commit -m "feat: create and remove accounts with backup"
```

---

### Task 4: Store inspection (detect the unsafe states)

**Files:**
- Create: `core/src/store.rs`, `core/src/db.rs`
- Modify: `core/src/lib.rs`
- Test: `core/src/store.rs` (inline)

**Interfaces:**
- Consumes: `Paths`.
- Produces: `SHARED: [&str; 5]`; `StoreState { Shared, NotConfigured, Split { threads_in_profile: usize }, AuthCycle, Nested { profile: String } }`; `StoreReport { state: StoreState, threads: usize, projects: Vec<String>, inodes: Vec<(String, u64)> }`; `store::inspect(&Paths) -> StoreReport`; `db::thread_count(path: &Path) -> Result<usize, Error>`; `db::quick_check(path: &Path) -> Result<bool, Error>`; `db::inode(path: &Path) -> Result<u64, Error>`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn detects_shared_store_by_inode() {
    let t = std::env::temp_dir().join(format!("cd-insp-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    std::os::unix::fs::symlink(&p.base.join("state_5.sqlite"),
        p.profiles.join("work/state_5.sqlite")).unwrap();
    let r = store::inspect(&p);
    assert!(matches!(r.state, StoreState::Shared));
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn detects_auth_cycle() {
    let t = std::env::temp_dir().join(format!("cd-cycle-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::write(p.profiles.join("work/auth.json"), "{}").unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"),
        p.base.join("auth.json")).unwrap();
    std::os::unix::fs::symlink(&p.base, p.profiles.join("work/link")).unwrap();
    let r = store::inspect(&p);
    assert!(matches!(r.state, StoreState::AuthCycle), "got {:?}", r.state);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn detects_nested_profile_dir() {
    let t = std::env::temp_dir().join(format!("cd-nest-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex"));
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    let r = store::inspect(&p);
    assert!(matches!(r.state, StoreState::Nested { .. }));
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn detects_split_store_with_threads() {
    let t = std::env::temp_dir().join(format!("cd-split-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    db::insert_thread(&p.profiles.join("work/state_5.sqlite"), "t1", "/x/y.jsonl").unwrap();
    let r = store::inspect(&p);
    assert!(matches!(r.state, StoreState::Split { threads_in_profile: 1 }), "got {:?}", r.state);
    std::fs::remove_dir_all(&t).ok();
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p codex-dock-core store --offline`
Expected: FAIL — modules not defined.

- [ ] **Step 3: Implement**

`db` shells out to `sqlite3` and maps a missing binary to `Error::Msg("sqlite3 not found in PATH")`. `db::create_empty` and `db::insert_thread` are test helpers (`#[cfg(test)]` or `pub` with a doc note) creating the minimal `threads`/`projects`/`project_roots` schema. `inspect` checks, in order: nested dir, auth cycle, split store, shared, not configured.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p codex-dock-core store --offline`
Expected: PASS, 4 tests. The cycle test must not hang — a bound of one `readlink -f` is enough.

- [ ] **Step 5: Commit**

```bash
git add core && git commit -m "feat: inspect shared store and detect unsafe states"
```

---

### Task 5: Store repair

**Files:**
- Modify: `core/src/store.rs`
- Test: `core/src/store.rs` (inline)

**Interfaces:**
- Consumes: `store::inspect`, `db::*`.
- Produces: `store::repair(&Paths, backup: &Path) -> Result<StoreReport, Error>`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn repair_breaks_auth_cycle_and_links_store() {
    let t = std::env::temp_dir().join(format!("cd-rep-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    std::fs::write(p.profiles.join("work/auth.json"), r#"{"tokens":{"account_id":"abc"}}"#).unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json")).unwrap();

    let bak = p.backup_dir("stamp");
    let r = store::repair(&p, &bak).unwrap();
    assert!(matches!(r.state, StoreState::Shared));
    assert!(!p.base.join("auth.json").is_symlink(), "auth.json must be materialised");
    assert_eq!(db::inode(&p.base.join("state_5.sqlite")).unwrap(),
               db::inode(&p.profiles.join("work/state_5.sqlite")).unwrap());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn repair_is_idempotent() {
    let t = std::env::temp_dir().join(format!("cd-idem-{}", std::process::id()));
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    let bak = p.backup_dir("stamp");
    store::repair(&p, &bak).unwrap();
    let before = db::inode(&p.profiles.join("work/state_5.sqlite")).unwrap();
    store::repair(&p, &bak).unwrap();
    assert_eq!(before, db::inode(&p.profiles.join("work/state_5.sqlite")).unwrap());
    std::fs::remove_dir_all(&t).ok();
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p codex-dock-core repair --offline`
Expected: FAIL — `repair` not defined.

- [ ] **Step 3: Implement**

Order: materialise cyclic `auth.json` (copy target to a real file, move the old link into backup); merge split stores with `INSERT OR IGNORE` on `projects`, `project_roots`, `threads`, then `UPDATE threads SET rollout_path = replace(...)`; move each non-link shared path into backup and `symlink`; finish by calling `inspect` and verifying equal inodes plus `quick_check` on every profile path. Refuse (return `Err`) when `Nested`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p codex-dock-core --offline`
Expected: PASS, all tests.

- [ ] **Step 5: Commit**

```bash
git add core && git commit -m "feat: repair shared store with backup and verification"
```

---

### Task 6: Window launching

**Files:**
- Create: `core/src/windows.rs`
- Modify: `core/src/lib.rs`
- Test: `core/src/windows.rs` (inline)

**Interfaces:**
- Consumes: `Paths`, `accounts::list`.
- Produces: `WindowProc { pid: u32, account: String, user_data_dir: PathBuf }`; `windows::command_for(&Paths, name: &str) -> Launcher` where `Launcher { bin: String, args: Vec<String>, env: Vec<(String, String)> }` (pure, so it is testable); `windows::launch(&Paths, name: &str) -> Result<WindowProc, Error>`; `windows::running(proc_root: &Path) -> Vec<WindowProc>`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn command_targets_profile_home_and_own_user_data_dir() {
    let p = Paths::new("/home/u/.codex", "/home/u/.codex-profiles");
    let l = windows::command_for(&p, "work");
    assert_eq!(l.bin, "/usr/bin/chatgpt");
    assert_eq!(l.env,
        vec![("CODEX_HOME".to_string(), "/home/u/.codex-profiles/work".to_string())]);
    assert!(l.args.join(" ").contains("--user-data-dir=/home/u/.config/Codex-work"));
}

#[test]
fn every_account_gets_a_distinct_user_data_dir() {
    let p = Paths::new("/h/.codex", "/h/.codex-profiles");
    let a = windows::command_for(&p, "work").args.join(" ");
    let b = windows::command_for(&p, "secondary").args.join(" ");
    assert_ne!(a, b);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p codex-dock-core windows --offline`
Expected: FAIL — module not defined.

- [ ] **Step 3: Implement**

`command_for` returns a `Launcher` with `bin = "/usr/bin/chatgpt"`, `args = ["--user-data-dir=<home>/.config/Codex-<name>"]`, and `env = [("CODEX_HOME", "<profiles>/<name>")]`. `launch` applies `env` to the `Command` and returns the child pid. `running` reads `/proc/*/cmdline`, matches `--user-data-dir=`, and maps the trailing name back to an account.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p codex-dock-core --offline`
Expected: PASS, all tests.

- [ ] **Step 5: Commit**

```bash
git add core && git commit -m "feat: launch one Codex window per account"
```

---

### Task 7: Usage via app-server JSON-RPC

**Files:**
- Create: `core/src/usage.rs`
- Modify: `core/src/lib.rs`
- Test: `core/src/usage.rs` (inline, fixture-driven)

**Interfaces:**
- Consumes: `Paths`.
- Produces: `Usage { account: String, five_hour_percent: Option<f64>, weekly_percent: Option<f64>, credits: Option<f64>, plan: Option<String>, checked_at: i64, error: Option<String> }`; `Color { Green, Amber, Red, Neutral }`; `usage::parse_rate_limits(account: &str, json: &str, now: i64) -> Usage`; `usage::worst_color(&[Usage]) -> Color`; `usage::read(&Paths, account: &str, now: i64) -> Usage`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_rate_limits_response() {
    let json = r#"{"five_hour_limit_percent":87.5,"weekly_limit_percent":40,
                   "balance_usage_credits":12.5,"plan_type":"pro"}"#;
    let u = usage::parse_rate_limits("work", json, 1000);
    assert_eq!(u.five_hour_percent, Some(87.5));
    assert_eq!(u.weekly_percent, Some(40.0));
    assert_eq!(u.credits, Some(12.5));
    assert_eq!(u.plan.as_deref(), Some("pro"));
    assert_eq!(u.checked_at, 1000);
    assert!(u.error.is_none());
}

#[test]
fn missing_fields_stay_none_never_zero() {
    let u = usage::parse_rate_limits("work", r#"{"weekly_limit_percent":40}"#, 5);
    assert_eq!(u.five_hour_percent, None, "absent must not become 0");
}

#[test]
fn worst_color_uses_the_highest_five_hour_percent() {
    let mk = |p: Option<f64>| behavior_stub(p);
    assert_eq!(usage::worst_color(&[mk(Some(10.0)), mk(Some(70.0))]), Color::Amber);
    assert_eq!(usage::worst_color(&[mk(Some(10.0)), mk(Some(90.0))]), Color::Red);
    assert_eq!(usage::worst_color(&[mk(Some(10.0))]), Color::Green);
    assert_eq!(usage::worst_color(&[mk(None)]), Color::Neutral);
}
```

`behavior_stub` is a two-line helper in the test module building a `Usage` with only the percent set.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p codex-dock-core usage --offline`
Expected: FAIL — module not defined.

- [ ] **Step 3: Implement**

`parse_rate_limits` uses `serde_json::Value` and maps absent keys to `None`. `read` shells out to `codex app-server proxy` under the account's `CODEX_HOME`, sends `account/rateLimits/read`, and on any failure returns a `Usage` with `error: Some(...)` and all numeric fields `None`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p codex-dock-core --offline`
Expected: PASS, all tests.

- [ ] **Step 5: Commit**

```bash
git add core && git commit -m "feat: read per-account usage over app-server RPC"
```

---

### Task 8: CLI binary

**Files:**
- Create: `core/src/main.rs`
- Test: none (manual smoke over tested units)

**Interfaces:**
- Consumes: every unit above.
- Produces: `codex-dock list | status | repair | launch <name> | usage`.

- [ ] **Step 1: Implement**

Subcommands dispatch to the units and print a table. `status` prints the `StoreReport` plus per-account usage. No new logic.

- [ ] **Step 2: Smoke-test by hand**

Run: `cargo run -q --offline -- status`
Expected: reports `Shared`, 139 threads, 3 projects, and both accounts logged in. This must write nothing.

- [ ] **Step 3: Commit**

```bash
git add core && git commit -m "feat: headless CLI over the core units"
```

---

### Task 9: Tauri shell and tray

**Files:**
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`, `src/index.html`, `assets/tray-{green,amber,red,neutral}.png`
- Modify: `Cargo.toml` (workspace members)

**Requires network.** Run `cargo install tauri-cli --version "^2"` first; the Tauri crates are not in the local cache.

**Interfaces:**
- Consumes: all `core` units.
- Produces: window listing accounts with plan, 5-hour %, weekly %, and an Open button; tray with worst-case colour and per-account menu.

- [ ] **Step 1: Install the CLI (user-run)**

```bash
cargo install tauri-cli --version "^2"
```

- [ ] **Step 2: Add the shell**

`main.rs` exposes Tauri commands that each call one `core` function and return serialised results. The UI calls `list`, `status`, `usage`, `open`. The tray is built from `usage::worst_color` and re-rendered on the same 60s tick as the window.

- [ ] **Step 3: Generate tray icons**

Four 22×22 solid-circle PNGs, one per colour (`#3fb950`, `#d29922`, `#f85149`, `#8b949e`). Commit them; no runtime image dependency.

- [ ] **Step 4: Verify**

Run: `cargo tauri dev`
Expected: the window lists both accounts; the tray shows an amber or red dot; clicking an account opens its Codex window; a chat created there appears after reopening the other window.

- [ ] **Step 5: Commit**

```bash
git add src-tauri src assets Cargo.toml && git commit -m "feat: Tauri shell with account list and usage tray"
```

---

## Self-Review

**Spec coverage:** accounts (Tasks 2–3), shared-store verify/repair + backup + inode/`quick_check` proof (Tasks 4–5), window launching and duplicate focus (Task 6), usage per account and tray colour (Task 7, 9), `.codex-global-state.json` merge (Task 5 repair step), no-op first run (Task 8 smoke test), fake-value ban (Task 7 `None` tests), build prerequisite (Task 9 Step 1). No spec section lacks a task.

**Gaps found and fixed:** the spec's global-state merge had no home — added to Task 5. The "focus an already-open window" rule was in the spec's failure table but no task — added to Task 6's `running`.
