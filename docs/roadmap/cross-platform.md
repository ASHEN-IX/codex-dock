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
- `capabilities()` — per-OS matrix, produced not assumed:

  | Capability | Linux | macOS | Windows |
  |---|---|---|---|
  | `shared_store_links` | Yes | Yes (symlinks exist, but **untested** — see §5) | No (needs Developer Mode / elevation; Phase 1 refuses) |
  | `process_table` | Yes (procfs) | No ("reads /proc ... Linux-only") | No (not implemented) |
  | `window_activation` | Yes (KWin/wmctrl) | No | No |

- `require(support, feature)` turns `No` into a user-readable refusal. Refusing
  call sites: `store::link_profile`, `store::repair` (both refuse **before
  moving anything**), `windows::launch` (cannot detect existing windows → would
  duplicate them), CLI `running` (prints the reason, exits 1).
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
  layouts. No concurrent unvalidated writes were introduced; merging still
  has the standing rule *"never run against a running desktop window"* (docs +
  code comment).
- **Windows (Phase 1: isolation-only):** each `CODEX_HOME` is a complete,
  independent store. `link_profile`/`repair` refuse with capability reasons
  before any mutation. No shared mutable SQLite file is touched. When (and
  only when) a Windows runtime test proves store layout + privilege
  unblocked symlink creation, links can be enabled behind the same gate.
- **macOS (Phase 1: isolation-only):** symlinks exist and *would* work, but
  without a process table `launch` cannot dedupe windows, so the shared-store
  path stays deliberately off until T2 (§6) proves the desktop keeps per-home
  stores.
- Used-space writes (`logs_2`, `queue_1`, `goals_1`) are per-profile by
  observed live layout; making them shared is **not** proposed: they are
  high-frequency write targets and concurrent multi-process writes to one
  SQLite file across N windows is exactly what the assignment excludes until
  validated.
- Rollback rule for any future storage migration (unchanged): backups
  `~/codex-dock-backup-<stamp>/` hold the displaced originals; repair is
  idempotent and displaces nothing when re-run (`repair_displaced...` test).

## 6. Staged plan, acceptance criteria

### T0 — Phase 1 (this repo state)
Adapters + capability gates + Linux-parity tests + three-OS CI gates.
Acceptance: all core tests pass on Linux; native gates (fmt/test/clippy/check)
pass on ubuntu/macos/windows runners; no user data touched anywhere.

### T1 — Verify desktop storeparity on Windows/macOS (manual, capability-gated)
On disposable accounts: confirm which `CODEX_HOME` files the desktop writes on
each OS, whether `--user-data-dir` is honored, and whether chats (rollouts →
projections) behave like Linux. Evidence into this document.
Acceptance: a table like §4 per OS, or an explicit "unsupported" entry.

### T2 — Native process enumeration where it is worth it
Only after T1: macOS (NSWorkspace / `ps` fallback) and Windows
(`NtQuerySystemInformation` or `EnumProcesses`) to enable dedup-launch and
focus. Behind `platform::process`, capability still refused until tests pass
on native CI.
Acceptance: `running` finds a launched window on the OS; `launch` deduplicates.

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
cargo test --locked -p codex-dock-core              # 132 tests, Linux PASS
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
