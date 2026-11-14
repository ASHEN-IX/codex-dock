# Cross-platform roadmap

Status of Phase 1: **architecture assessed, adapters landed, Linux unchanged,
Windows/macOS explicitly NOT supported yet.** Support claims in this document
are separated into Verified (evidence on this machine or in CI), Partial
(evidence exists but incomplete), and Blocked (cannot be claimed without work
that has not been done). Compilation passing on a platform is never treated as
runtime support.

## 1. Scope rules (inherited from the Phase 1 assignment)

- Preserve every existing profile, credential and chat. Nothing in Phase 1
  migrates, deletes, or rewrites user data; `store::repair` semantics on Linux
  are byte-for-byte the previous behaviour.
- No real credentials in tests: every fixture uses synthetic
  `{"tokens":{"account_id":"..."}}` JSON in temp directories.
- No automatic releases, no publishing, no website work in this phase.
- Keep the Rust core + Tauri 2 architecture; add no dependencies.

## 2. OS-dependency inventory (traceable)

Every platform-sensitive operation in the tree, and where it lives now:

| # | Concern | Where | Pre-Phase-1 shape | Now |
|---|---|---|---|---|
| 1 | Home directory | `paths.rs` `Paths::real` | `$HOME` env, `expect` panic | `platform::fs::home_dir()` — `$HOME` first (Linux behaviour preserved), then the std home fallback; clear panic message |
| 2 | Electron config root | `paths.rs` `user_data_dir` | hardcoded `$HOME/.config` | `platform::fs::config_dir_at(home)` — Linux unchanged, macOS `Library/Application Support`, Windows `%APPDATA%` |
| 3 | Symlink shared store | `store.rs` `link_profile`, `repair`, `db` merge | `std::os::unix::fs::symlink` | `platform::fs::create_link`; gated by `links_support()`; refuse-before-touch on unsupported platforms |
| 4 | File identity (inode sharing proof) | `db.rs` `inode`, `store.rs` | `MetadataExt::ino` | `platform::fs::file_identity()` — `Some(ino)` on Unix, `None` = "cannot verify" elsewhere |
| 5 | Private files (0600) | `automation.rs`, `store.rs` merge writer, `ping.rs` | `OpenOptionsExt::mode(0o600)` | `create_private` / `create_new_private` / `open_private` |
| 6 | Private dirs (0700) | `ping.rs` workspace | `DirBuilderExt` | `create_private_dir` |
| 7 | Executable-bit check | `tool.rs` | `PermissionsExt` | `platform::fs::is_executable` |
| 8 | Running-window detection | `windows.rs` | hardcoded `/proc` reads | `platform::process::proc_root()`; `running()` refuses on platforms without a process table |
| 9 | Window activation | `windows.rs` `focus_pid` | qdbus6 (KWin) then wmctrl (X11) | unchanged code, but reachable only when `process_table: Yes` |
| 10 | Desktop binary discovery | `tool.rs` | PATH scan for `chatgpt`/`codex-desktop`/`ChatGPT`, `CODEX_DESKTOP_BIN` | unchanged on Unix; extension-aware on other platforms; override unchanged |
| 11 | `codex` CLI discovery | `tool.rs` `codex()` | PATH + HOME fallbacks + hardcoded Linux paths | unchanged; `/opt/homebrew/bin/codex` already anticipates macOS |
| 12 | Launch stamp | CLI `stamp()` | shells out to `date` | `date` preserved on Unix (identical output), UTC fallback elsewhere |
| 13 | Ping timeout/transport | `ping.rs`, `usage.rs` | std process + `File::try_lock` | unchanged — std is cross-platform |
| 14 | Login child termination | `src-tauri/main.rs` | `kill -TERM` | `kill -TERM` on Unix; `taskkill` elsewhere (documented as weaker: no graceful signal) |
| 15 | Process env reading | `windows.rs::running_for` | `/proc/PID/environ` | reachable only via the procfs root |
| 16 | Tray / menu | Tauri + `tray.rs` | pure RGBA raster + Tauri | unchanged; Tauri provides per-OS tray |
| 17 | Single instance | `tauri-plugin-single-instance` | plugin supports Linux + Windows + macOS (checked upstream source) | unchanged |
| 18 | DB access | `db.rs` | shells out to the `sqlite3` CLI binary | unchanged; **packaging must deliver `sqlite3`** (see §8) |
| 19 | Backup stamps / backup dir | `paths.rs`, CLI | dated dir under home | unchanged |
| 20 | Settings file | `automation.rs` | JSON, atomic rename | unchanged; Windows lacks atomic-replace-through-rename (risk R7) |

## 3. Platform abstraction & capability model

`core/src/platform` is the single seam layer:

- `Support::Yes | No(reason)` — every missing feature carries its reason.
- `capabilities()` — per-OS matrix, produced not assumed, distinguishing filesystem primitives from validated application compatibility:

  | Capability | Linux | macOS | Windows |
  |---|---|---|---|
  | `filesystem_symlinks` | Yes | Yes (POSIX symlinks available) | No (needs Developer Mode / elevation) |
  | `shared_store_compatibility` | Yes (validated on Linux) | No (unvalidated runtime; profiles stay isolated) | No (unvalidated runtime; profiles stay isolated) |
  | `native_process_discovery` | Yes (procfs `/proc`) | Yes (native `libproc` & `sysctl`; synthetic verified, real desktop unverified) | Yes (Win32 Toolhelp32 & `QueryFullProcessImageNameW`; synthetic verified, real desktop unverified) |
  | `window_activation` | Yes (KWin/wmctrl) | No (native adapter implemented; official runtime validation pending) | No (native adapter implemented; official runtime validation pending) |
  | `safe_desktop_launch` | Yes (verified desktop exec) | No (official application bundle runtime validation pending) | No (official desktop runtime validation pending) |

- `require(support, feature)` turns `No` into a user-readable refusal. Refusing
  call sites:
  - `store::link_profile`: requires `shared_store_compatibility` and `filesystem_symlinks`.
  - `store::repair`: requires `shared_store_compatibility`, `filesystem_symlinks`, and `native_process_discovery` (refuses fail-closed if writers cannot be reliably excluded).
  - `windows::launch`: requires `safe_desktop_launch` and `native_process_discovery`.
  - `accounts::add_account`: preflights inputs, executes transactional rollback on failure without touching pre-existing directories, and uses **isolation-only mode** on macOS and Windows without attempting unverified shared-store operations.
- `present?`/`absent` confusion is forbidden: an empty `read_dir` used to be
  indistinguishable from "no procfs"; procfs is now `Option`-gated.

## 4. Desktop-integration audit against the installed desktop

Live evidence on the development machine (2026-11-05, read-only inspection):

- Codex Desktop (OpenAI "Owl" Electron shell) `26.1023.50603` at
  `/usr/lib/chatgpt/ChatGPT` via launcher `/usr/bin/chatgpt`; `owl-app.ini`
  declares `UserDataDirectoryName=Codex`; the Electron binary contains
  `--user-data-dir` handling; `~/.config/Codex-{work,secondary,default}` exist
  and were written within the last hours → **per-window `--user-data-dir` +
  `CODEX_HOME` isolation is still honored by this desktop build**. Verified.
- Codex CLI `codex-cli 0.164.0`; `~/.codex/version.json` advertises an
  available `0.165.0` → CLI surface can move under us between releases.
- The shared-store inventory in `store.rs` (`SHARED`) is **out of date with
  what the installed desktop writes**, and the layout is a *moving target*:
  the Phase 1 audit (2026-11-05 morning) and the T0 re-audit (same day, later)
  differ. Current `~/.codex` additionally holds (vs `SHARED`):
  `thread_history_1.sqlite` (+wal/shm), `goals_1.sqlite`, `logs_2.sqlite`,
  `memories_1.sqlite`, `queue_1.sqlite`, `session_index.jsonl`,
  `archived_sessions/`, `shell_snapshots/`, `thread-writer-locks/`,
  `rollout-migrations/`, `attachments/`, `ambient-suggestions/`,
  `app-server-daemon/`, `ipc/`, `replays/`, `node_repl/`, `generated_images/`,
  `computer-use/`, `external_agent_session_imports.json`,
  `project-metadata-locks/`. New stores may appear in any desktop release;
  capability T0→T5 must re-audit instead of freezing this list.
- Schema inspection on **copies** (never on the live files): bases
  `secondary=163MB/24,277 items`, `work=113MB/8,251`, base `150MB/33,133`.
  `thread_history_1.sqlite` is a *projection* store (`thread_items`,
  `thread_turns`, `thread_history_projection_state` with
  `next_rollout_byte_offset/ordinal` per thread; sqlx migrations v8) built from
  the shared `sessions/` rollouts.
- Consequence: the source-of-truth chats (rollouts) remain shared, but the
  projection store is **per-profile and diverging**. Whether windows present
  identical history despite divergent projections is unverifiable from files
  alone → classified Partial; a runtime observation task is staged (§6, T1).
- `state_5.sqlite` (the store `store.rs` links, counts, and repairs) remains
  linked inode-equal across base/profiles and `PRAGMA quick_check` passes as
  documented → the repair/link machinery is still valid for the files it
  claims.
- Windows/macOS equivalents of all of the above: **unverified**. The Windows
  desktop build is a separate Electron target with its own store layout; macOS
  likewise. Dock must not assume flag parity on those platforms.

### 4.1 Store-classification of the observed layout

Each file in `~/.codex` classified by its role, from schema inspection of
byte-copies (authoritativeness is *assigned*, never invented: a class is only
stated when the schema or live behavior shows which file stands behind it).

| Class | Files | Evidence / behavior |
|---|---|---|
| **Authoritative (shared, source of truth)** | `sessions/*.jsonl` rollout files; `archived_sessions/` (aged rollouts); `state_5.sqlite` threads tables; `auth.json`; `history.jsonl`; `config.toml` | Rollouts are the transcripts the desktop writes on every turn; archived rollouts remain evidence of past chats even after a window forgets them; `state_5` is the store `repair` links and `PRAGMA quick_check`s. These are what the shared store already hard-links today. |
| **Derived projection (per-profile, built FROM rollouts)** | `thread_history_1.sqlite` (`thread_items`, `thread_turns`, `thread_history_projection_state` with `next_rollout_byte_offset/ordinal`, sqlx migrations v8); `session_index.jsonl` | Pure projections of the rollout stream: the projection state records a byte offset + ordinal *into the rollout file*, so it can be rebuilt. Bases observed diverging across profiles (§4) — divergence is display-level, not data loss. Repairing/rebuilding is possible because rollouts exist; making these shared is **not** proposed until T1 proves the desktop tolerates it. |
| **Per-window mutable (unmanaged)** | `logs_2.sqlite`, `queue_1.sqlite`, `goals_1.sqlite`, `memories_1.sqlite`, `sqlite/codex-dev.db` (+wal/shm: `automations`, `automation_runs`, `local_thread_catalog_*`, schema v34 — appeared after the Phase 1 audit; role and authority **TBD**, must go through the same evidence process before any linking), `shell_snapshots/`, `attachments/`, `ambient-suggestions/`, `app-server-daemon/`, `app-server-control/`, `rollout-migrations/`, `ipc/` (unix socket), `replays/`, `node_repl/`, `generated_images/`, `computer-use/`, `external_agent_session_imports.json`, `thread-writer-locks/`, `project-metadata-locks/` | High-frequency write targets and per-window coordination state. Dock deliberately does not link or copy these; every window owns its own. |
| **Cache/ephemeral (unmanaged)** | `cache/`, `log/`, `models_cache.json`, `tmp/`, `installation_id`, temp dirs, caches inside per-window `Codex-*` config dirs | Regenerable; out of scope. |

**Concurrent-write hazards observed or inferred (documented, not fixed in
Phase 1) — all live in the *shared* class, which is why the standing rule
"never repair/merge while windows are running" exists:**

1. `state_5.sqlite` + wal/shm: multiple windows holding `CODEX_HOME` on the
   same inode write concurrently; SQLite WAL tolerates same-file multi-writer
   access (that is its job), but a *file displacement* under open writers does
   not — hence repair's move-aside ordering matters and refuses nothing else.
2. Rollout-file growth while a projection reader scans: `rollout-migrations/`
   and `next_rollout_byte_offset` suggest incremental re-reading of the same
   rollout file from two processes (window + any projection builder); a
   repair-time merge writing a rollout while a window tail-reads it could
   corrupt the projection state rather than the rollout itself.
3. `history.jsonl` appends from N windows: append-only writes share one
   descriptor target; interleaving is tolerated by line-oriented readers, but
   truncation/merge is not — repair must never rewrite it (it does not; merge
   covers `state_5` + project sidebars only).
4. `thread-writer-locks/` shows the desktop itself fences projection writers;
   Dock being lock-aware is future work (T3 acceptance couples repair with
   running-window detection).

No new shared-database assumptions were introduced; this classification only
restates the Phase 1 audit so T1 has a checklist to verify per OS.

## 5. Per-OS storage strategy (safe defaults)

- **Linux (today, unchanged):** link-based shared store, verified by inode and
  `PRAGMA quick_check`; move-then-link with backups; refuse nested/cyclic
  layouts. Account creation uses transactional rollback on linking failure.
  Mutating repair enforces fail-closed protection against active Codex windows
  and database writer locks.
- **Windows (Phase 1 / 1.1: isolation-only):** each `CODEX_HOME` is a complete,
  independent store. Account creation operates in isolation-only mode without
  running unverified shared-store operations. Direct `link_profile` and `repair`
  refuse with capability reasons before any mutation. No shared mutable SQLite
  file is touched.
- **macOS (Phase 1 / 1.1: isolation-only):** filesystem symlinks exist, but
  creating symlinks is **never interpreted as evidence that multi-profile SQLite
  sharing is safe**. Account creation operates strictly in isolation-only mode.
  Direct shared-store linking and repair refuse fail-closed until runtime
  compatibility is proven.
- Used-space writes (`logs_2`, `queue_1`, `goals_1`) are per-profile by
  observed live layout; making them shared is **not** proposed: they are
  high-frequency write targets and concurrent multi-process writes to one
  SQLite file across N windows is exactly what the assignment excludes until
  validated.
- Rollback rule for any future storage migration (unchanged): backups
  `~/codex-dock-backups/<stamp>/` hold the displaced originals; repair is
  idempotent and displaces nothing when re-run (`repair_displaced...` test).

## 6. Staged plan, acceptance criteria

### T0 — Phase 1 & Phase 1.1 Final Stabilization (merged on main)
Adapters + capability gates + Linux-parity tests + three-OS CI gates + platform safety corrections:
- **Persistent advisory synchronization locks:** `LockGuard` RAII guards prevent inode-replacement races; lock files (`.create-{name}.lock`, `.dock-repair.lock`, `.desktop-launch.lock`, `.login-{name}.lock`) remain persistent across releases.
- **Native atomic no-replace publication:** `atomic_rename_no_replace` uses `renameat2(RENAME_NOREPLACE)` on Linux and `renameatx_np(RENAME_EXCL)` on macOS, preventing TOCTOU races and pre-existing directory clobbering.
- **Disabled mutating shared-store repair:** Mutating repair is disabled across CLI, GUI, and public APIs (`Error::MaintenanceUnavailable`) to prevent active-writer hazards; read-only diagnostics (`store::inspect`, `db::quick_check`) use `sqlite3 -readonly` and guarantee byte preservation.
- **Account lifecycle data integrity:** Account creation enforces path containment and rollback safety; `accounts::remove` synchronizes with creation and launch locks, verifies no running windows, and moves profiles and user data to dated backup directories rather than deleting.
- **Lifecycle synchronization:** Login and account removal are synchronized via persistent cross-process `.login-{name}.lock`. Removal refuses fail-closed while login is in-flight.
- **Isolation-only runtime behavior for macOS and Windows:** Unvalidated platforms remain in isolation-only mode without executing unverified shared-store operations.
- Acceptance: 154 core tests pass on Linux; native gates pass on ubuntu/macos/windows runners. Merged into `main` via PR #8 (`9ae6eeb`).

### T1 — Windows & macOS Runtime Compatibility Audit (this milestone)

**Status:** Completed audit, diagnostic validation harness, and synthetic isolation verification.
**Storage Contract:** macOS and Windows remain strictly in **Isolation-Only Mode** (`shared_store_compatibility: Support::No`). Shared-store linking is not enabled.

#### 1. Platform Compatibility Matrix

| Property | Linux (Ubuntu) | macOS (Darwin) | Windows (Win32) |
| :--- | :--- | :--- | :--- |
| **Desktop Executable Locations** | `/usr/bin/chatgpt`, `/usr/local/bin/chatgpt`, AppImage | `/Applications/Codex.app/Contents/MacOS/Codex`, `/Applications/ChatGPT.app/Contents/MacOS/ChatGPT` | `%LOCALAPPDATA%\Programs\Codex\Codex.exe`, `%ProgramFiles%\Codex\Codex.exe` |
| **CLI Executable Locations** | `/usr/bin/codex`, `~/.local/bin/codex`, `/usr/lib/chatgpt/resources/codex` | `/usr/local/bin/codex`, `/opt/homebrew/bin/codex`, `Codex.app/Contents/Resources/codex` | `%USERPROFILE%\.cargo\bin\codex.exe`, `%APPDATA%\npm\codex.cmd`, `Codex\resources\codex.exe` |
| **Default Config / User-Data Root** | `~/.config/Codex-{name}` | `~/Library/Application Support/Codex-{name}` | `%APPDATA%\Codex-{name}` (`AppData/Roaming`) |
| **Base Profile Directory** | `~/.codex` | `~/.codex` | `%USERPROFILE%\.codex` |
| **Managed Profiles Root** | `~/.codex-profiles/` | `~/.codex-profiles/` | `%USERPROFILE%\.codex-profiles\` |
| **`sqlite3` System Availability** | Pre-installed (`/usr/bin/sqlite3`) | Pre-installed (`/usr/bin/sqlite3`) | **MISSING by default** (requires bundling or external winget/choco) |
| **Filesystem Symlinks** | POSIX symlinks (`Support::Yes`) | POSIX symlinks (`Support::Yes`) | Requires Developer Mode / elevation (`Support::No`) |
| **Shared Store Compatibility** | `Support::Yes` (validated on Linux) | `Support::No` (unvalidated multi-writer hazards) | `Support::No` (mandatory NTFS locking & no unprivileged file symlinks) |
| **Native Process Discovery** | `Support::Yes` (`/proc` procfs) | `Support::Yes` (`libproc` & `sysctl`) | `Support::Yes` (Win32 Toolhelp32 & `QueryFullProcessImageNameW`) |
| **Window Activation** | `Support::Yes` (KWin / wmctrl) | `Support::No` (official runtime validation pending) | `Support::No` (official runtime validation pending) |
| **Safe Desktop Launch** | `Support::Yes` (direct spawn + procfs sync) | `Support::No` (official runtime validation pending) | `Support::No` (official runtime validation pending) |

#### 2. Storage Compatibility Analysis & Hazards

- **macOS Analysis:**
  - APFS supports POSIX symlinks without elevation. However, macOS Codex Desktop runs inside a hardened runtime / App Sandbox (`com.apple.security.app-sandbox`). Traversing symlinks outside application container directories risks sandbox violations.
  - SQLite WAL mode on macOS uses POSIX advisory file locks. Multi-process concurrent access from separate Electron instances frequently triggers `SQLITE_BUSY` during checkpointing.
  - Derived projection divergence (`thread_history_1.sqlite` with byte offsets into `sessions/*.jsonl`) means sharing rollouts while projections diverge leads to inconsistent chat rendering.
  - **Decision:** macOS remains in **Isolation-Only Mode** (`Support::No`).

- **Windows Analysis:**
  - NTFS enforces mandatory file locking on open write handles. Two Codex Desktop instances opening `state_5.sqlite` or WAL files concurrently encounter `ERROR_SHARING_VIOLATION` (error 32).
  - Windows NTFS has no unprivileged file symbolic links; creating symlinks requires `SeCreateSymbolicLinkPrivilege` (Developer Mode or Admin elevation).
  - SQLite WAL shared memory mapping (`.shm` file) requires identical security descriptors across processes, failing across different integrity levels.
  - Windows does not ship `sqlite3.exe` on `%PATH%`.
  - **Decision:** Windows remains in **Isolation-Only Mode** (`Support::No`).

#### 3. Verified Synthetic Profile Isolation (6 Invariants)

Verified across Linux, macOS, and Windows via `core/tests/platform_t1_audit_test.rs` and `audit::run_synthetic_isolation_check()`:
1. **`CODEX_HOME` Propagation:** `command_for` sets `CODEX_HOME` pointing strictly to `p.profiles.join(name)`. Profiles never receive base directory or cross-profile paths.
2. **`--user-data-dir` Isolation:** Per-account user-data arguments adhere strictly to OS conventions (`.config` on Linux, `Library/Application Support` on macOS, `AppData/Roaming` on Windows). No two profiles share cookies, caches, or session storage.
3. **Profile Persistence:** `accounts::add_account` creates the profile directory in isolation mode; `accounts::list` discovers all configured profiles.
4. **Credential Separation:** `auth.json` in each profile is a regular, independent file (never symlinked or hardlinked). Overwriting one account's credentials leaves other accounts untouched.
5. **Independent Runtime State:** Lifecycle locks (`.login-{name}.lock`, `.create-{name}.lock`) are strictly per-profile; holding a lock on profile A does not block operations on profile B.
6. **Rollout & Projection Isolation:** Session transcripts (`sessions/*.jsonl`) and projection stores (`thread_history_1.sqlite`) created in profile A remain local to profile A and never leak into profile B or base.

#### 4. Opt-In Diagnostic Validation Tool

Added `codex-dock-cli doctor` (or `audit`) to core and CLI:
- Evaluates system capabilities truthfully.
- Probes installed Desktop, CLI, and SQLite3 binaries with version detection.
- Reports filesystem roots and user-data templates.
- Runs non-destructive synthetic profile isolation tests in a temporary sandbox.
- Sanitizes output (zero credential or private user information disclosure).

#### 5. Manual Smoke-Test Runbook

- **macOS Manual Smoke Test:**
  1. Install official Codex Desktop / ChatGPT into `/Applications/Codex.app`.
  2. Verify CLI discovery: `codex-dock doctor`.
  3. Create disposable account: `codex-dock add test-isolated`.
  4. Verify profile creation in `~/.codex-profiles/test-isolated` with `auth.json` and `config.toml`.
  5. Verify that `codex-dock launch test-isolated` refuses fail-closed until Phase T2.
  6. Sign in via `codex-dock login test-isolated` and verify credentials persist in profile `auth.json` only.
  7. Clean up: `codex-dock remove test-isolated`.

- **Windows Manual Smoke Test:**
  1. Install official Codex Desktop into `%LOCALAPPDATA%\Programs\Codex\Codex.exe`.
  2. Install `sqlite3.exe` (via `winget install SQLite.SQLite` or bundle).
  3. Verify discovery: `codex-dock doctor`.
  4. Create disposable account: `codex-dock add test-isolated`.
  5. Verify profile creation in `%USERPROFILE%\.codex-profiles\test-isolated`.
  6. Verify launch refusal fail-closed (`desktop launch is not supported here...`).
  7. Sign in via `codex-dock login test-isolated`.
  8. Clean up: `codex-dock remove test-isolated`.

#### 6. Explicit Blockers for Production Windows/macOS Support

1. **`sqlite3` Dependency on Windows:** Production installer must bundle SQLite3 or Dock core must embed SQLite (via `rusqlite` / `sqlx`).
2. **Phase T2 Native Process Discovery:** Windows Win32 process enumeration (`EnumProcesses` / `CreateToolhelp32Snapshot`) and macOS process scanning (`libproc` / `kinfo_proc`) must be implemented before safe launch deduplication can be enabled.
3. **Phase T2 Window Management:** Cocoa AppleScript / NSWorkspace window activation on macOS and Win32 `SetForegroundWindow` on Windows.
4. **Code Signing:** Windows Authenticode signing and macOS Apple Developer ID notarization required before binary distribution.

#### 7. Phase T2 Implementation Recommendations

- **T2.1 (Native Process Discovery):** Implement native process scanning on Windows (Toolhelp32 snapshots, `NtQueryInformationProcess`) and macOS (`libproc`, `sysctl`). [Completed]
- **T2.2 (Safe Native Desktop Launching):** Implement platform-neutral desktop launch interface with native Windows/macOS adapters, strict argument and environment isolation, lifecycle locks, and deduplication. [Completed - Conditional GO]
- **T2.3 (Native Window Activation):** Implement window activation via Cocoa / AppleScript on macOS and Win32 `SetForegroundWindow` on Windows.
- **Maintain Invariants:** Keep Windows and macOS in isolation-only mode throughout Phase T2. Ensure all 6 isolation invariants remain strictly enforced.

### T2 — Native process enumeration where it is worth it

#### T2.1 — Native Process Discovery (Windows & macOS)
**Status:** Completed native process discovery on Windows and macOS.
- **Windows Implementation:** Documented Win32 APIs: `CreateToolhelp32Snapshot`, `Process32FirstW`, `Process32NextW`, `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`, and `QueryFullProcessImageNameW`. Dynamic resolution of `NtQueryInformationProcess(ProcessCommandLineInformation = 60)` with memory-safe `UNICODE_STRING` buffer validation and `CommandLineToArgvW` / MSDN parsing.
- **macOS Implementation:** Native `libproc` (`proc_listpids`, `proc_pidpath`, `proc_name`) and `sysctl(KERN_PROCARGS2)` for `argv` and `env` extraction. Identifies `.app` bundles (`/Codex.app/Contents/MacOS/Codex`).
- **Safety & Attribution Invariants:**
  - Strict profile attribution: processes are matched against the full expected path returned by `Paths::user_data_dir()`, normalizing slashes, Windows case-insensitivity, and verbatim prefixes. Matching basenames alone (e.g. `/tmp/unrelated/Codex-<name>`) are strictly rejected.
  - Rejection of conflicting evidence: processes with conflicting `CODEX_HOME` and `--user-data-dir` are safely categorized as `ProfileAttribution::Unknown`.
  - Incomplete scan semantics: permission denials and relevant inspection failures prevent false `NoCodexProcess` outcomes; `query_account` returns `InspectionIncompleteOrDenied`.
  - CLI vs Desktop distinction: standalone Codex CLI commands (`codex login`, `codex exec`) and Electron helpers are cleanly distinguished from Codex Desktop.
  - Candidate Codex processes with missing, unreadable, or unmanaged arguments are classified as `ProfileAttribution::Unknown` (NEVER falsely assigned to "default" or arbitrary accounts).
  - Explicit discovery outcomes: `ManagedProfile`, `UnknownProfile`, `NoCodexProcess`, `InspectionIncompleteOrDenied`, and `Unsupported`.
  - Process churn, rapid exit (`ESRCH` / `ERROR_INVALID_PARAMETER`), and permission barriers (`EPERM` / `ERROR_ACCESS_DENIED`) handled gracefully without false scan failure or data leaks.
  - Capability `native_process_discovery` enabled (`Support::Yes`) on Linux, macOS, and Windows.
  - Verification Scope: CI tests verify synthetic helper processes and native OS discovery mechanics; proprietary official Codex Desktop runtime behavior remains unverified on Windows/macOS. Profile removal, desktop launching, and shared store remain safely gated and disabled until independently verified.

#### T2.2 — Safe Native Desktop Launching (Windows & macOS)
**Status:** Completed launch architecture, process handoff, safe startup timeout, and native execution adapters with synthetic verification; CONDITIONAL GO pending real official Codex Desktop validation.
- **Platform-Neutral Launch Architecture (`core/src/platform/launch.rs`):**
  - Unified launch interface (`launch_desktop`, `launch_desktop_with_options`, `resolve_desktop_binary`).
  - Preconditions and containment: canonical account name validation, safe path segment enforcement (`accounts::safe_segment`), and profile directory containment verification (ensuring profiles cannot escape the managed profiles root via symlinks while preserving legitimate Linux `default` aliases).
  - Lifecycle lock protection: holds the repair lock throughout startup (`.dock-repair.lock`, refusing launch if locked or inaccessible, never treating open errors as absence) and acquires profile-specific launch locks (`.desktop-launch.lock`) to prevent concurrent race conditions.
  - Process pre-flight gating: calls `query_profile` and verifies `.desktop-launch.pid` before spawning to refuse duplicate launches (`ManagedProfile`), ambiguous unmanaged processes (`UnknownProfile`), incomplete scans (`InspectionIncompleteOrDenied`), or in-flight starting instances.
  - CLI vs Desktop distinction: prevents accidental execution of the standalone Codex CLI as Codex Desktop (`is_codex_cli` check and CLI binary comparison).
  - Process detachment and execution:
    - Linux: `std::process::Command` with `pre_exec` `setsid()` session leader detachment.
    - macOS: Direct Mach-O executable resolution inside `.app` application bundles (reading `CFBundleExecutable` from `Info.plist`), bypassing `open -a` to guarantee 100% environment (`CODEX_HOME`) and argument (`--user-data-dir`) fidelity without Launch Services sanitization.
    - Windows: `std::process::Command` with `CREATE_NEW_PROCESS_GROUP`, Win32 argument escaping, and isolated environment propagation.
  - Post-spawn attribution verification loop with process handoff support:
    - Always reports the PID of the verified discovered desktop process, never an exited launcher.
    - Preserves the initial launcher PID separately in `WindowProc.launcher_pid` when a handoff occurs.
    - Reaps the initial spawned child asynchronously to prevent zombie accumulation.
  - Safe startup timeout and lifecycle safety:
    - Distinguishes confirmed spawn failures (no child created) from indeterminate verification. Even a nonzero launcher exit can leave delayed descendants, so it is not proof of absence.
    - Detects invisible-but-running background processes when window activation is required.
    - Preserves processes where ownership is uncertain rather than killing unrelated instances.
    - Writes a private persistent `indeterminate` startup record before spawn; read/write errors fail closed. Only successful verification replaces it with a desktop PID. Retries refuse an uncertain record even after the launcher exits. After manually confirming that no desktop or delayed descendants remain, remove the profile’s `.desktop-launch.pid` to recover; no process is automatically killed. A verified PID record is cleared automatically only after that PID is no longer alive.
- **Synthetic Native Verification:**
  - Automated integration tests (`core/tests/desktop_launch_test.rs`) verify multi-profile launching (`alpha` and `beta`) with independent `CODEX_HOME` and `--user-data-dir` propagation.
  - Verified launcher handoff, exited launcher PID exclusion, slow startup indeterminate handling, crash retry refusal and explicit recovery, delayed handoff after zero/nonzero launcher exit, duplicate launch prevention, early exit cleanup, launch lock release, and CLI binary rejection.
- **Production Capability Gate & Verification Scope:**
  - `LaunchOptions` API enforces platform capabilities in production release builds; ordinary callers cannot bypass `safe_desktop_launch: Support::No`. The synthetic constructor exists only in debug/test builds or with the explicit non-default `synthetic-tests` Cargo feature for isolated release tests. Environment variables cannot enable it in an ordinary release binary. Native CI separately tests normal release entry points with default features.
  - `safe_desktop_launch` capability remains set to `Support::No` on Windows and macOS in production capabilities until official proprietary Codex Desktop runtime execution is verified on native hardware.
  - `shared_store_compatibility` remains disabled (`Support::No`); profiles remain strictly isolated.
  - Profile removal on Windows and macOS remains disabled until end-to-end lifecycle safety is established.

#### T2.3 — Native Window Activation (Windows & macOS)
Native adapters and injectable policy tests are implemented; official runtime validation remains pending. No production capability is enabled.

- `platform::activation::activate_profile` gates production calls, requires a complete native process report, rejects unknown/candidate/helper identities and multiple main PIDs for the requested account, and selects only standard windows owned by that verified PID.
- Windows enumerates top-level HWNDs using `EnumWindows` and `GetWindowThreadProcessId`; excludes owned dialogs, tool/no-activate windows, non-Electron classes, and small helper surfaces. It restores hidden/minimized windows with `ShowWindow`, requests activation with `SetForegroundWindow`, and verifies the actual `GetForegroundWindow` result and non-cloaked DWM visibility. It never attaches input queues or overrides foreground restrictions.
- macOS resolves `NSRunningApplication` by verified PID, enumerates Accessibility windows, requires standard window roles and `AXUIElementGetPid` ownership, then unhides/unminimizes and raises the selected window. Success requires both AppKit active state and live system Accessibility focused-application/window evidence, corroborated by an on-screen layer-zero Quartz window with the same PID and uniquely matching bounds. Missing permissions and metadata fail closed without opening a permission prompt.
- Quartz window information was evaluated: it can corroborate owner PIDs, but a CGWindowID has no public reliable mapping to an actionable AX window reference. AX ownership and standard-window roles are authoritative for selecting the actionable reference; on-screen Quartz owner/bounds metadata corroborates visibility. Multiple AX or Quartz windows with matching bounds are indeterminate and cannot produce success. No private AX/Quartz bridge or title/bundle-name inference is used.
- Process evidence and window ownership are rechecked before action and throughout bounded foreground verification. A visible modal/dialog surface blocks activation rather than redirecting it. Failed activation never falls back to launch.
- Linux keeps the existing KWin/X11 focus implementation. T2.2 launch locks, startup records, and retry behavior remain unchanged.
- `window_activation`, `safe_desktop_launch`, and `shared_store_compatibility` stay `Support::No` on Windows/macOS, including after synthetic CI passes. Debug or explicitly feature-enabled testing uses an isolated API; default-feature release callers cannot bypass the gates through it.
- See [the opt-in smoke protocol](../testing/native-window-activation.md) for disposable-account validation and the exact commands. CI policy tests and compilation are not evidence of official Desktop runtime behavior.

### T3 — Linking enablement per OS (shared store)
Windows only if symlink privilege is validated (Developer Mode test); macOS
after T1. Rollout with backups, idempotent repair, and the split-merge path
tested on-OS. Keep `Concurrent multi-writer` rule: repair still refuses while
windows run (planned addition).
Acceptance: synthetic split-detector + repair tests pass on the OS runner;
manual smoke with disposable profiles.

### T4 — Packaging (see §8) and signed releases, per OS, gated by manual smoke.
### T5 — Usage/ping surface per OS — re-verify `codex` CLI flags on each OS
CLI version drift (0.164 → 0.165+) can change `exec --ignore-user-config` or
app-server RPC shapes. On Linux this is now machine-checked before every ping
(help-based capability probe), which also tolerates CLIs older/newer than the
documented 0.166 floor; usage reads do not have an equivalent probe yet.
Acceptance: usage/ping verified per OS with disposable accounts, no fabricated
readings.

## 7. Risk register

| ID | Risk | Impact | Mitigation / status |
|---|---|---|---|
| R1 | Store inventory drift (§4): desktop writes more stores than `SHARED` links | users report "chats differ between windows" | Documented as Partial; T1 verification; no code change until understood |
| R2 | CLI version drift (`exec` / app-server flags) | usage/ping silently break | pings now probe `codex exec --help` and refuse **before recording a cooldown or spending allowance** when the required flag surface is missing or unqueryable (`tool::require_exec_support`); app-server RPC shapes still only refused by failing honestly (T5 re-verification per release) |
| R3 | Windows symlink privilege | sharing silently degrades | capability refusal; isolation is the supported Windows shape in Phase 1 |
| R4 | `/proc` semantics differ (mac) | duplicate windows / wrong focus | process table `No` on mac; launch refuses honestly |
| R5 | `thread_history` projection divergence on same thread | inconsistent history display | T1; only rollouts are authoritative today |
| R6 | `sqlite3` CLI missing on user systems | every store operation fails with a clear error | packaging dependency lists (§8); error message already explicit |
| R7 | Windows atomic-rename semantics | auto-ping settings races | document; Phase 1 Windows not released |
| R8 | Auth-cycle/nested traps reused from Linux onto mac | repair could mangle layouts | repair refuses before mutating when links unsupported; synthetic tests are Unix-gated until T1 |
| R9 | License absence (§9) | legal block on any release | BLOCKED, owner decision required |
| R10 | Archived `rollback` of upgrades | user confusion | backup-first displacement + idempotency tests (existing) |

## 8. Packaging requirements

Current (Linux, verified in-repo): AppImage + deb + rpm via `cargo tauri
build`, tar.gz portable bundle, Arch package, `SHA256SUMS`, Ubuntu 22.04 ABI
baseline, verified checksums workflow, draft-only releases.

### Windows
- Tauri bundle targets `nsis` and `msi` (WiX): NSIS preferred for per-user
  installs, MSI for enterprise. Both must be produced from native CI
  (`windows-latest`), not from Linux cross-compilation.
- Code signing is a hard requirement for distribution: Authenticode
  (`signtool`) with an Extended Validation or Azure Trusted Signing
  certificate; unsigned installers trip SmartScreen and lose the store
  check. Sign the `.exe` and both installers, timestamp the signature.
- Ship `sqlite3` as a bundled DLL/CLI or declare DLL dependency + PATH.
- Windows Defender/SmartScreen first-run telemetry is a manual smoke step.
- Checksums: `SHA256SUMS` for every artifact, created on the build runner.

### macOS
- `.dmg` produced on `macos-latest` (lipo of arm64 + x86_64 when universal
  binaries exist; today the app is x86_64-only — choose native runners per
  arch, do not claim arm64 until built and tested on it).
- Gatekeeper requires: Developer ID Application **and** Installer
  certificates, `codesign --options runtime` (hardened runtime), and
  notarization (`notarytool`/staple) applied to the DMG. Non-notarized DMGs
  open only through system-policy exceptions — do not ship that way.
- Tauri updater (if enabled later) requires signing of update bundles with
  pinned keys; keep it disabled until the release process settles.
- Bundle `sqlite3` binary (or use system `/usr/bin/sqlite3` — present on
  macOS, but pinned against SIP changes; bundling is safer).

### All platforms
- Release security posture is inherited from the Linux release workflow:
  read-only build jobs; a separate narrowly-scoped draft job; pinned action
  SHAs; `sha256sum` manifests; draft-first, publish-after-review. Extend that
  workflow only after native packaging works.
- Add SBOM/transparency artifacts only if maintainers commit to reviewing
  them; avoid tooling for its own sake.
- No secrets in build logs; signing keys as GitHub environment secrets with
  a human approving the environment for drafts.

## 9. Licensing / release-readiness audit

- **No `LICENSE` file exists in the repository.** `docs/maintaining.md` already
  documents this as a required decision, and this audit confirms it: until an
  owner picks and applies a license (including copyright lines), *any* public
  release of binaries is legally unclarified. **BLOCKED — owner decision.**
- Dependency license check: core is MIT/Apache-compatible serde only;
  Tauri/wry/licensed under MIT/Apache — no known redistribution blocker, but a
  shipped-binary notice file (THIRD-PARTY-NOTICES) should be generated once a
  license exists.
- Readiness beyond licensing:
  - CI: PASS (Linux), Cross-OS CI gates: proposed in `cross-platform.yml`.
  - Windows/macOS runtime support: BLOCKED (T1/T2/T3).
  - Signed releases: BLOCKED (needs certificates + process; no paid
    infrastructure exists in the repo today).
  - Docs/installer claims: Linux packaging accurate; no cross-platform pages
    exist yet — good, since nothing may be promised.

## 10. Verification commands (run at Phase 1 completion)

```bash
cargo fmt --all -- --check
cargo test --locked -p codex-dock-core              # 149 tests, Linux PASS
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release --workspace          # Linux PASS
python3 scripts/check-ui.py
go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12   # workflows, clean
```

Cross-OS native gates (`cross-platform.yml`, pull_request + dispatch):
`cargo fmt`, `cargo test -p codex-dock-core`, `cargo clippy -D warnings`,
`cargo check -p codex-dock` on ubuntu-24.04, macos-latest, windows-latest.
These gates *cannot* validate on this machine (only the linux-gnu standard
library is installed locally), so the first green cross-OS run is a required
acceptance item for T0 — pending until the workflow runs on GitHub.
