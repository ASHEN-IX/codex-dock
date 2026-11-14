# Phase T2.2: Safe Native Desktop Launching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement safe, isolated Codex Desktop launching on Windows and macOS building on T2.1 native process discovery while preserving Linux functionality and maintaining strict isolation-only storage.

**Architecture:** 
1. Correct the two Windows discovery edge cases (strict `ERROR_NO_MORE_FILES` termination and absolute-only `UNICODE_STRING` buffer pointers).
2. Build a platform-neutral launch orchestration module (`core/src/platform/launch.rs`) with native platform execution adapters for Linux (setsid detachment), macOS (direct Mach-O execution with pristine environment), and Windows (`CREATE_NEW_PROCESS_GROUP`, `CommandLineToArgvW` argument safety).
3. Integrate strict lifecycle controls: account validation, lock guards, process pre-flight deduplication via `query_account`, CLI vs Desktop binary differentiation, and post-spawn verification.
4. Keep `safe_desktop_launch` disabled in production on Windows and macOS until official proprietary binaries are validated, providing synthetic integration tests to prove the launch engine natively.

**Tech Stack:** Rust (edition 2021), Tauri 2, Win32 (`Process32FirstW`, `Process32NextW`, `CREATE_NEW_PROCESS_GROUP`), Unix/macOS (`setsid`, Mach-O bundle resolution, `std::process::Command`), `codex-dock-core`.

**Spec:** User prompt requirements for Codex Dock Phase T2.2 (Prerequisites, Launch architecture, Native process execution, Runtime verification, Safety boundaries, Delivery).

## Global Constraints

- Never mutate real user profiles, credentials, or session databases during automated tests.
- Maintain `shared_store_compatibility: Support::No` on Windows and macOS.
- Maintain `window_activation: Support::No` on Windows and macOS until T2.3.
- Mutating repair remains disabled.
- Profile removal remains disabled on Windows and macOS.
- Keep `safe_desktop_launch: Support::No(...)` in production capabilities on Windows and macOS when official binaries are unverified; mark T2.2 delivery as CONDITIONAL GO.
- Never use shell interpolation (`sh -c` or `cmd /c`) to launch Codex Desktop; pass arguments and environment variables directly.
- Standalone Codex CLI executables must never be launched as Codex Desktop.

## Review Focus

1. **Confusing Codex CLI with Codex Desktop:** If `CODEX_DESKTOP_BIN` or PATH resolves to the CLI binary, launching it as Desktop would fail or run CLI command loops. *Test: Verify `is_codex_cli` rejection during binary resolution.*
2. **Launch Deduplication & Ambiguity:** Launching when an account is already running or when discovery is incomplete/ambiguous. *Test: Verify `query_account` gating for `ManagedProfile`, `UnknownProfile`, and `InspectionIncompleteOrDenied`.*
3. **Environment & Argument Contamination on macOS:** Using `open -a` strips or mangles `CODEX_HOME` and `--user-data-dir`. *Test: Direct Mach-O bundle execution with isolated environment.*
4. **Console/Terminal Binding on Windows:** Launching from CLI tying the desktop process lifetime to the console or Ctrl+C signals. *Test: Verify `CREATE_NEW_PROCESS_GROUP` and detached stdio.*
5. **Lifecycle Lock Racing:** Concurrent launches or launch during repair. *Test: Verify `.dock-repair.lock` and `.desktop-launch.lock` contention handling.*

---

### Task 1: Fix Outstanding Windows Discovery Edge Cases (Phase 0)

**Files:**
- Modify: `core/src/platform/process/windows.rs:69-89`
- Modify: `core/src/platform/process.rs:128-135`
- Test: `core/tests/process_discovery_test.rs:395-480`

- [ ] **Step 1: Write failing tests for the two edge cases in `core/tests/process_discovery_test.rs`**
  - Add test asserting that `parse_unicode_string_buffer` rejects buffers containing relative offsets (non-absolute pointers).
  - Add test asserting that `MockSnapshotEnumerator` treating error code `0` on `Process32FirstW`/`NextW` failure marks the scan as incomplete with a warning, not as clean completion.
- [ ] **Step 2: Run test suite to verify the test fails or exposes the edge case**
  - Run: `cargo test --test process_discovery_test`
- [ ] **Step 3: Correct Win32 snapshot enumeration termination in `core/src/platform/process/windows.rs`**
  - In `LiveSnapshotEnumerator::first` and `next`: replace `if err == ERROR_NO_MORE_FILES || err == 0` with `if err == ERROR_NO_MORE_FILES`.
  - Ensure error code `0` on a `FALSE` return from `Process32FirstW`/`Process32NextW` is returned as `Err(0)` to trigger incomplete scan semantics.
- [ ] **Step 4: Remove synthetic relative pointer fallback in `core/src/platform/process.rs`**
  - In `parse_unicode_string_buffer`: remove `else if ptr_val >= header_size && ptr_val <= buf.len()`.
  - Require `ptr_val >= base_addr` and `offset = ptr_val.checked_sub(base_addr)?`.
  - Update `core/tests/process_discovery_test.rs` fixtures to use absolute pointers into the allocated buffer.
- [ ] **Step 5: Run tests, clippy, and formatting**
  - Run: `cargo fmt --all -- --check`
  - Run: `cargo clippy --all-targets -- -D warnings`
  - Run: `cargo test --all`
- [ ] **Step 6: Commit Phase 0 fixes**
  - Commit message: `fix(windows): enforce strict snapshot termination and absolute unicode pointers`

---

### Task 2: Platform-Neutral Launch Architecture & Core Launch Engine (Phase 1 & Phase 2)

**Files:**
- Create: `core/src/platform/launch.rs`
- Modify: `core/src/platform/mod.rs`
- Modify: `core/src/windows.rs`
- Modify: `core/src/tool.rs`
- Test: `core/tests/desktop_launch_test.rs`

- [ ] **Step 1: Write failing tests in `core/tests/desktop_launch_test.rs`**
  - Test account validation (invalid segment, non-existent account).
  - Test repair lock contention (launch refused when `.dock-repair.lock` exists).
  - Test launch lock contention (second launch blocked while first launch lock held).
  - Test rejection when Codex CLI binary is supplied instead of Codex Desktop.
  - Test rejection when `query_account` reports `ManagedProfile` (duplicate prevention).
  - Test rejection when `query_account` reports `UnknownProfile` or `InspectionIncompleteOrDenied`.
- [ ] **Step 2: Run `cargo test --test desktop_launch_test` to confirm failure**
- [ ] **Step 3: Implement `core/src/platform/launch.rs`**
  - Define `LaunchOptions` (allowing binary override and synthetic test flags).
  - Define `LaunchSpec`: validated account name, binary path, args (`--user-data-dir=...`), env (`CODEX_HOME=...`), runtime paths.
  - Implement `resolve_desktop_binary`: verify file exists, is executable, and is NOT a Codex CLI binary (`!is_codex_cli`).
  - Implement `validate_launch_preconditions`:
    - Canonicalize account name and check safe segment.
    - Check `.dock-repair.lock`.
    - Acquire `.desktop-launch.lock`.
    - Check `query_account`: fail-closed on `ManagedProfile`, `UnknownProfile`, `InspectionIncompleteOrDenied`, and `Unsupported`.
  - Implement native process spawning per OS:
    - Unix/Linux: `std::process::Command`, `setsid()` pre-exec detachment, null stdio.
    - macOS: Resolve `.app` bundle Mach-O executable, `setsid()` pre-exec detachment, null stdio, no `open -a`.
    - Windows: `std::process::Command`, `CREATE_NEW_PROCESS_GROUP`, null stdio, safe argument formatting.
  - Implement post-spawn verification:
    - Poll `query_account` for confident attribution.
    - Check `child.try_wait()` for early exit failure.
    - Return `WindowProc`.
    - Spawn thread to reap child on eventual exit.
- [ ] **Step 4: Connect `core/src/windows.rs` to `platform::launch`**
  - Forward `windows::launch(p, name)` through `platform::launch::launch_desktop(p, name)`.
  - Maintain Linux backward compatibility.
- [ ] **Step 5: Run tests, clippy, and formatting**
  - Run: `cargo fmt --all -- --check`
  - Run: `cargo clippy --all-targets -- -D warnings`
  - Run: `cargo test --all`
- [ ] **Step 6: Commit Task 2 changes**
  - Commit message: `feat(platform): implement platform-neutral desktop launch architecture`

---

### Task 3: Comprehensive Native Integration & Isolation Tests (Phase 3 & Phase 4)

**Files:**
- Modify: `core/tests/desktop_launch_test.rs`
- Modify: `core/tests/platform_capability_test.rs`

- [ ] **Step 1: Write integration tests with synthetic desktop helper processes**
  - Test launching two independent profiles (`alpha` and `beta`): verify distinct `CODEX_HOME` and `--user-data-dir` received by mock processes.
  - Test process termination during startup: mock binary that exits immediately with code 1 causes launch failure.
  - Test existing process duplicate prevention: when mock binary for `alpha` is running, second launch for `alpha` is rejected.
  - Test launch cancellation and lock cleanup: if launch fails, lock file is cleanly released.
  - Test credential and profile isolation: verify no cross-profile file contamination.
- [ ] **Step 2: Run all tests locally**
  - Run: `cargo test --all`
- [ ] **Step 3: Run Clippy and formatting**
  - Run: `cargo fmt --all -- --check`
  - Run: `cargo clippy --all-targets -- -D warnings`
- [ ] **Step 4: Commit Task 3 test suite**
  - Commit message: `test(platform): add synthetic native desktop launch verification tests`

---

### Task 4: Documentation, Roadmap, and Capability Status (Phase 4 & Phase 5)

**Files:**
- Modify: `docs/roadmap/cross-platform.md`
- Modify: `core/src/audit.rs`
- Modify: `core/src/platform/mod.rs`

- [ ] **Step 1: Audit and verify capabilities matrix**
  - Confirm `safe_desktop_launch` is `Support::Yes` on Linux, `Support::No(...)` on Windows and macOS.
  - Confirm `window_activation` is `Support::Yes` on Linux, `Support::No(...)` on Windows and macOS.
  - Confirm `shared_store_compatibility` is `Support::Yes` on Linux, `Support::No(...)` on Windows and macOS.
- [ ] **Step 2: Update roadmap in `docs/roadmap/cross-platform.md`**
  - Document T2.2 launch architecture, native adapters, and synthetic test results.
  - Explicitly document that official proprietary Codex Desktop runtime behavior remains unverified on Windows/macOS.
- [ ] **Step 3: Update `core/src/audit.rs` diagnostics**
  - Ensure doctor diagnostics describe the conditional state accurately.
- [ ] **Step 4: Run full workspace test suite and format check**
  - Run: `cargo fmt --all -- --check`
  - Run: `cargo clippy --all-targets -- -D warnings`
  - Run: `cargo test --all`
- [ ] **Step 5: Commit documentation and capability adjustments**
  - Commit message: `docs(roadmap): document T2.2 safe desktop launch architecture and conditional status`

---

### Task 5: Remote CI Matrix Verification & Delivery (Phase 5)

- [ ] **Step 1: Push branch to GitHub**
  - Push `cross-platform/t2-safe-desktop-launch` to origin.
- [ ] **Step 2: Open Pull Request against `main`**
  - Use `gh pr create` with comprehensive description detailing architecture, safety boundaries, and test results.
- [ ] **Step 3: Monitor all CI workflows across Windows, macOS, and Linux runners**
  - Verify `Cross-platform` (Native gate Windows, macOS, Linux).
  - Verify `CI` (Ubuntu 22.04 & 24.04).
  - Verify `Security` (CodeQL Rust, JS, Python, Actions, cargo audit).
  - Verify `Linux packages` (Ubuntu 22.04 & 24.04).
- [ ] **Step 4: Prepare delivery report**
  - Report exact commit SHA, PR number and URL, CI verification matrix, and CONDITIONAL GO verdict.
