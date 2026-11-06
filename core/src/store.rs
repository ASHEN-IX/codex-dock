use crate::db;
use crate::paths::Paths;
use std::path::{Path, PathBuf};

pub const SHARED: [&str; 5] = [
    "state_5.sqlite",
    "sessions",
    "archived_sessions",
    "memories_1.sqlite",
    "history.jsonl",
];

#[derive(Debug, Clone, PartialEq)]
pub enum StoreState {
    Shared,
    NotConfigured,
    Split { threads_in_profile: usize },
    AuthCycle,
    Nested { profile: String },
}

#[derive(Debug, Clone)]
pub struct StoreReport {
    pub state: StoreState,
    pub threads: usize,
    pub projects: Vec<String>,
    pub inodes: Vec<(String, u64)>,
    /// Why the store could not be read, when it could not be. Never a fake zero.
    pub error: Option<String>,
}

pub fn inspect(p: &Paths) -> StoreReport {
    let base_db = p.base.join("state_5.sqlite");

    let mut accounts = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&p.profiles) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') && e.path().is_dir() {
                accounts.push(name);
            }
        }
    }
    accounts.sort();

    // Cycle: base/auth.json cannot be resolved because following it loops.
    // That is exactly the dangerous shape (base/auth.json -> profile/auth.json
    // where the profile directory resolves back into the base). Two benign
    // shapes must not be reported: base/auth.json -> <real profile>/auth.json
    // (the live setup) and a profile directory that is a symlink to the base.
    let base_auth = p.base.join("auth.json");
    if base_auth.is_symlink() && std::fs::canonicalize(&base_auth).is_err() {
        return report(StoreState::AuthCycle, &base_db);
    }

    // Nested: the profiles dir lives inside (or is) the base store.
    if let (Ok(a), Ok(b)) = (
        std::fs::canonicalize(&p.profiles),
        std::fs::canonicalize(&p.base),
    ) {
        if a.starts_with(&b) {
            return report(
                StoreState::Nested {
                    profile: first(&accounts),
                },
                &base_db,
            );
        }
    }

    if !base_db.exists() {
        return report(StoreState::NotConfigured, &base_db);
    }

    // Split: a profile has a DB holding threads that is NOT the shared file.
    // Comparing identities (not paths) is what makes a profile that is itself a
    // symlink to the base read as shared rather than split.
    let base_ino = crate::platform::fs::file_identity(&base_db);
    for name in &accounts {
        let prof_db = p.profiles.join(name).join("state_5.sqlite");
        if !prof_db.exists() {
            continue;
        }
        if let (Some(b), Some(pi)) = (base_ino, crate::platform::fs::file_identity(&prof_db)) {
            if b == pi {
                continue;
            }
        }
        if prof_db.is_symlink() {
            continue;
        }
        if let Ok(n) = db::thread_count(&prof_db) {
            if n > 0 {
                return report(
                    StoreState::Split {
                        threads_in_profile: n,
                    },
                    &base_db,
                );
            }
        }
    }

    // Do not claim health we did not verify: if the DB cannot be read, say so.
    let probe = report(StoreState::Shared, &base_db);
    if probe.error.is_some() {
        return probe;
    }
    probe
}

/// Give a freshly created profile the shared store, so the new account's window
/// shows the same chats as every other window.
///
/// Nothing is moved: a new profile has no data of its own, so there is nothing
/// to preserve. Missing shared paths are skipped rather than created, which
/// keeps this a no-op on a machine that has not been set up yet.
pub fn link_profile(p: &Paths, name: &str) -> Result<()> {
    crate::platform::fs::require_shared_store("shared-store linking")?;
    crate::platform::fs::require_links("shared-store linking")?;
    let dir = p.profiles.join(name);
    if !dir.is_dir() {
        return Err(Error::Msg(format!("account '{name}' does not exist")));
    }
    for item in SHARED {
        let shared = p.base.join(item);
        if !shared.exists() {
            continue;
        }
        let target = dir.join(item);
        if target.is_symlink() || target.exists() {
            continue;
        }
        crate::platform::fs::create_link(&shared, &target)?;
    }
    sync_projects(p, name)
}

/// Import sidebar metadata before an account window starts. Never call this
/// against a running desktop window, which owns an in-memory copy of the file.
pub fn sync_projects(p: &Paths, name: &str) -> Result<()> {
    let profile = p.profiles.join(name);
    if !profile.is_dir() {
        return Err(Error::Msg(format!("account '{name}' does not exist")));
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    merge_global_state(
        &p.base.join(".codex-global-state.json"),
        &profile.join(".codex-global-state.json"),
        &p.backup_dir(&format!("projects-{nonce}")).join(name),
    )
}

fn first(v: &[String]) -> String {
    v.first().cloned().unwrap_or_default()
}

fn report(state: StoreState, base_db: &std::path::Path) -> StoreReport {
    let (threads, projects, error) = if !base_db.exists() {
        (0, Vec::new(), None)
    } else {
        match (db::thread_count(base_db), db::project_names(base_db)) {
            (Ok(t), Ok(p)) => (t, p, None),
            (Err(e), _) | (_, Err(e)) => (0, Vec::new(), Some(e.to_string())),
        }
    };
    StoreReport {
        state,
        threads,
        projects,
        inodes: Vec::new(),
        error,
    }
}

use crate::error::{Error, Result};

fn move_aside(path: &Path, backup: &Path, label: &str) -> Result<()> {
    if !path.exists() && !path.is_symlink() {
        return Ok(());
    }
    let dest = backup.join(label);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(path, &dest)?;
    Ok(())
}

fn ensure_unlocked(path: &Path, label: &str) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .or_else(|_| std::fs::OpenOptions::new().read(true).open(path))
        .map_err(|e| Error::Msg(format!("cannot open {label} to verify locks: {e}")))?;

    file.try_lock().map_err(|_| {
        Error::Msg(format!(
            "cannot repair: {label} is locked by an active process or writer"
        ))
    })?;
    let _ = file.unlock();
    Ok(())
}

fn check_proc_file_handles_and_locks(proc_root: &Path, target_paths: &[PathBuf]) -> Result<()> {
    if !crate::platform::process::supported() {
        return Ok(());
    }

    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    // Build target representations: canonical paths and (dev, ino, maj, min)
    struct TargetInfo<'a> {
        path: &'a PathBuf,
        canonical: PathBuf,
        #[cfg(unix)]
        dev: u64,
        #[cfg(unix)]
        ino: u64,
        #[cfg(unix)]
        maj: u32,
        #[cfg(unix)]
        min: u32,
    }

    let mut targets: Vec<TargetInfo> = Vec::new();
    for path in target_paths {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        #[cfg(unix)]
        if let Ok(meta) = path.metadata() {
            let dev = meta.dev();
            let ino = meta.ino();
            let maj = ((dev >> 8) & 0xfff) as u32;
            let min = ((dev & 0xff) | ((dev >> 12) & 0xfff00)) as u32;
            targets.push(TargetInfo {
                path,
                canonical,
                dev,
                ino,
                maj,
                min,
            });
            continue;
        }
        #[cfg(not(unix))]
        targets.push(TargetInfo { path, canonical });
    }

    if targets.is_empty() {
        return Ok(());
    }

    let my_pid = std::process::id();

    // 1. Check /proc/locks for POSIX/OFD/FLOCK locks
    let locks_path = proc_root.join("locks");
    if locks_path.is_file() {
        let locks_data = std::fs::read_to_string(&locks_path).map_err(|e| {
            Error::Msg(format!(
                "cannot read '{}' to verify active locks: {e}",
                locks_path.display()
            ))
        })?;
        for line in locks_data.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Line format: e.g. "1: POSIX ADVISORY WRITE 1234 08:01:19542 0 EOF"
            if parts.len() >= 6 {
                let pid_str = parts[4];
                let pid = pid_str.parse::<u32>().unwrap_or(0);
                if pid == my_pid {
                    continue;
                }
                let maj_min_ino: Vec<&str> = parts[5].split(':').collect();
                if maj_min_ino.len() == 3 {
                    #[cfg(unix)]
                    if let (Ok(maj), Ok(min), Ok(ino)) = (
                        u32::from_str_radix(maj_min_ino[0], 16),
                        u32::from_str_radix(maj_min_ino[1], 16),
                        maj_min_ino[2].parse::<u64>(),
                    ) {
                        if let Some(t) = targets
                            .iter()
                            .find(|t| t.ino == ino && t.maj == maj && t.min == min)
                        {
                            return Err(Error::Msg(format!(
                                "cannot repair: active lock detected in process table on '{}' (inode {ino}, pid {pid})",
                                t.path.display()
                            )));
                        }
                    }
                }
            }
        }
    }

    // 2. Scan open file descriptors in /proc/*/fd/*
    let rd = std::fs::read_dir(proc_root).map_err(|e| {
        Error::Msg(format!(
            "failed to read process directory '{}': {e}",
            proc_root.display()
        ))
    })?;
    for entry in rd {
        let entry = entry.map_err(|e| Error::Msg(format!("failed to read process entry: {e}")))?;
        let pid: u32 = match entry.file_name().to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if pid == my_pid {
            continue;
        }
        let fd_dir = entry.path().join("fd");
        let fd_rd = match std::fs::read_dir(&fd_dir) {
            Ok(r) => r,
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                continue;
            }
            Err(e) => {
                return Err(Error::Msg(format!(
                    "failed to read file descriptors for pid {pid}: {e}"
                )))
            }
        };
        for fd_entry in fd_rd.flatten() {
            if let Ok(link) = std::fs::read_link(fd_entry.path()) {
                let link_str = link.to_string_lossy();
                for t in &targets {
                    let c_str = t.canonical.to_string_lossy();
                    let p_str = t.path.to_string_lossy();
                    if link_str == c_str
                        || link_str == p_str
                        || link_str.starts_with(&format!("{c_str} "))
                        || link_str.starts_with(&format!("{p_str} "))
                    {
                        return Err(Error::Msg(format!(
                            "cannot repair: process pid {pid} has an open file handle to '{}'",
                            t.path.display()
                        )));
                    }
                }
            }
            #[cfg(unix)]
            if let Ok(meta) = fd_entry.metadata() {
                if meta.is_file() {
                    let dev = meta.dev();
                    let ino = meta.ino();
                    if let Some(t) = targets.iter().find(|t| t.dev == dev && t.ino == ino) {
                        return Err(Error::Msg(format!(
                            "cannot repair: process pid {pid} has open file handle to inode {ino} ('{}')",
                            t.path.display()
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

thread_local! {
    static MUTATING_REPAIR_THREAD_COUNTER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Internal helper to enable mutating repair for regression tests or offline maintenance.
#[doc(hidden)]
pub fn enable_mutating_repair_for_test(enabled: bool) {
    MUTATING_REPAIR_THREAD_COUNTER.with(|c| {
        if enabled {
            c.set(c.get() + 1);
        } else {
            c.set(c.get().saturating_sub(1));
        }
    });
}

pub fn repair(p: &Paths, backup: &Path) -> Result<StoreReport> {
    repair_with_proc_root(p, backup, crate::platform::process::proc_root().as_deref())
}

struct RepairLocks {
    locks: Vec<crate::platform::fs::LockGuard>,
}

pub fn repair_with_proc_root(
    p: &Paths,
    backup: &Path,
    proc_root: Option<&Path>,
) -> Result<StoreReport> {
    // Architectural decision (Phase 1.1 Final Stabilization):
    // Mutating shared-store repair is disabled by default for normal users to prevent
    // active-writer hazards and data corruption. Read-only diagnostics ('store::inspect')
    // remain fully available.
    let allowed = MUTATING_REPAIR_THREAD_COUNTER.with(|c| c.get() > 0)
        || std::env::var("CODEX_DOCK_ENABLE_MUTATING_REPAIR").as_deref() == Ok("1");

    if !allowed {
        return Err(Error::MaintenanceUnavailable(
            "mutating shared-store repair is disabled in this milestone to protect database integrity against concurrent writers. Use 'store status' for read-only diagnostics.".to_string(),
        ));
    }

    // 1. Refuse before touching anything if shared store is not validated
    crate::platform::fs::require_shared_store("shared-store repair")?;
    crate::platform::fs::require_links("shared-store repair")?;

    // 2. Refuse if reliable process & writer discovery is unavailable
    if let crate::platform::Support::No(reason) =
        crate::platform::capabilities().native_process_discovery
    {
        return Err(Error::Msg(format!(
            "cannot safely repair: reliable process and writer detection is unavailable on this platform ({reason}); active writers cannot be excluded"
        )));
    }

    // 3. Reject missing or unreadable process-discovery root
    let root = match proc_root {
        Some(r) => {
            if !r.exists() {
                return Err(Error::Msg(format!(
                    "cannot safely repair: process discovery root '{}' does not exist",
                    r.display()
                )));
            }
            if !r.is_dir() {
                return Err(Error::Msg(format!(
                    "cannot safely repair: process discovery root '{}' is not a directory",
                    r.display()
                )));
            }
            if let Err(e) = std::fs::read_dir(r) {
                return Err(Error::Msg(format!(
                    "cannot safely repair: failed to read process discovery root '{}': {e}",
                    r.display()
                )));
            }
            r
        }
        None => {
            return Err(Error::Msg(
                "cannot safely repair: process discovery root is unavailable; active processes and writers cannot be verified".to_string()
            ));
        }
    };

    // 4. Synchronize Dock operations: acquire and hold global repair lock and all profile launch locks
    let repair_lock_path = p.base.join(".dock-repair.lock");
    let repair_guard = crate::platform::fs::LockGuard::try_acquire(
        &repair_lock_path,
        "another Codex Dock repair operation",
    )?;

    let mut held_locks = RepairLocks {
        locks: vec![repair_guard],
    };

    let base_launch_lock = p.base.join(".desktop-launch.lock");
    let base_guard =
        crate::platform::fs::LockGuard::try_acquire(&base_launch_lock, "base profile launch")?;
    held_locks.locks.push(base_guard);

    let mut profile_dirs = Vec::new();
    let base_canonical = std::fs::canonicalize(&p.base).ok();
    if let Ok(rd) = std::fs::read_dir(&p.profiles) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') && e.path().is_dir() {
                let prof_canonical = std::fs::canonicalize(e.path()).ok();
                let is_base = base_canonical.is_some() && base_canonical == prof_canonical;
                if !is_base {
                    let prof_launch_lock = e.path().join(".desktop-launch.lock");
                    let prof_guard = crate::platform::fs::LockGuard::try_acquire(
                        &prof_launch_lock,
                        &format!("desktop launch for profile '{name}'"),
                    )?;
                    held_locks.locks.push(prof_guard);
                }
                profile_dirs.push((name, e.path()));
            }
        }
    }
    profile_dirs.sort();

    // 5. Fail-closed if active Codex windows or processes are running for this store
    let running = crate::windows::scan_running(root)?;
    let config_root = crate::platform::fs::config_dir_at(p.home());
    let active_for_store: Vec<String> = running
        .into_iter()
        .filter(|w| {
            if !w.user_data_dir.as_os_str().is_empty() && w.user_data_dir.starts_with(&config_root)
            {
                return true;
            }
            let env =
                std::fs::read(root.join(w.pid.to_string()).join("environ")).unwrap_or_default();
            env.split(|b| *b == 0)
                .find_map(|v| v.strip_prefix(b"CODEX_HOME="))
                .map(|v| PathBuf::from(String::from_utf8_lossy(v).into_owned()))
                .map(|h| h.starts_with(&p.base) || h.starts_with(&p.profiles))
                .unwrap_or(false)
        })
        .map(|w| format!("pid {} ({})", w.pid, w.account))
        .collect();
    if !active_for_store.is_empty() {
        return Err(Error::Msg(format!(
            "cannot repair shared store while Codex windows are running (found: {}); close all Codex windows first",
            active_for_store.join(", ")
        )));
    }

    // 6. Fail-closed if active database writers, locks, or open file handles are detected
    let mut candidate_files = Vec::new();
    let base_db = p.base.join("state_5.sqlite");
    if base_db.is_file() {
        candidate_files.push(base_db.clone());
        let base_wal = p.base.join("state_5.sqlite-wal");
        if base_wal.is_file() {
            candidate_files.push(base_wal);
        }
        let base_shm = p.base.join("state_5.sqlite-shm");
        if base_shm.is_file() {
            candidate_files.push(base_shm);
        }
    }

    for (_name, prof_dir) in &profile_dirs {
        let prof_db = prof_dir.join("state_5.sqlite");
        if prof_db.is_file() && !prof_db.is_symlink() {
            candidate_files.push(prof_db.clone());
            let prof_wal = prof_dir.join("state_5.sqlite-wal");
            if prof_wal.is_file() {
                candidate_files.push(prof_wal);
            }
            let prof_shm = prof_dir.join("state_5.sqlite-shm");
            if prof_shm.is_file() {
                candidate_files.push(prof_shm);
            }
        }
    }

    // 6a. Scan /proc for open file handles or /proc/locks
    check_proc_file_handles_and_locks(root, &candidate_files)?;

    // 6b. Verify SQLite exclusive lock using SQLite's own engine (detects active transactions)
    if base_db.is_file() {
        crate::db::verify_sqlite_exclusive(&base_db)?;
    }
    for (_name, prof_dir) in &profile_dirs {
        let prof_db = prof_dir.join("state_5.sqlite");
        if prof_db.is_file() && !prof_db.is_symlink() {
            crate::db::verify_sqlite_exclusive(&prof_db)?;
        }
    }

    // 6c. Check OS file-level try_lock as defense-in-depth
    for f in &candidate_files {
        ensure_unlocked(f, &f.display().to_string())?;
    }

    let writer_locks_dir = p.base.join("thread-writer-locks");
    if writer_locks_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&writer_locks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    ensure_unlocked(&path, &format!("thread writer lock '{}'", path.display()))?;
                }
            }
        }
    }
    profile_dirs.sort();

    let current = inspect(p);

    // Nested profiles dir can only be fixed by the user; linking would recurse.
    if let StoreState::Nested { .. } = current.state {
        return Err(Error::Msg(
            "profiles directory is inside the base store; move it out before repairing".into(),
        ));
    }

    // Create backup directory only after all fail-closed preflight checks pass
    std::fs::create_dir_all(backup)?;

    // 1. Break a cyclic auth.json: materialise a real file, keep the old link.
    // A genuine cycle is exactly the case where the link cannot be resolved.
    // The benign live shape (base/auth.json -> <real profile>/auth.json)
    // resolves fine and must be left untouched.
    let base_auth = p.base.join("auth.json");
    if base_auth.is_symlink() && std::fs::canonicalize(&base_auth).is_err() {
        let link = p.base.join("auth.json");
        let stale = backup.join("base-auth.json.link");
        std::fs::create_dir_all(backup)?;
        std::fs::rename(&link, &stale)?;
        // No credential can be recovered from a loop; leave the base logged
        // out rather than inventing one. The profile's own auth.json is intact.
    }

    let base_db = p.base.join("state_5.sqlite");
    let base_real = std::fs::canonicalize(&p.base).ok();

    for (name, dir) in &profile_dirs {
        // A profile directory that resolves to the base IS the shared store.
        // Linking inside it, or moving anything out of it, would destroy it.
        let dir_real = std::fs::canonicalize(dir).ok();
        let is_base_itself = match (&dir_real, &base_real) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if is_base_itself {
            continue;
        }

        let prof_db = dir.join("state_5.sqlite");
        let prof_real = std::fs::canonicalize(&prof_db).ok();
        let points_at_base_db = match (&prof_real, std::fs::canonicalize(&base_db).ok()) {
            (Some(a), Some(b)) => *a == b,
            _ => false,
        };

        if prof_db.exists() && !prof_db.is_symlink() && !points_at_base_db {
            // An unreadable profile DB is never displaced: the user's data may
            // be in a schema we do not understand. Merge only what we can read.
            let n = db::thread_count(&prof_db)?;
            if n > 0 {
                db::merge_from(&base_db, &prof_db, dir, &p.base)?;
                db::copy_sessions(dir, &p.base)?;
            }
            move_aside(&prof_db, backup, &format!("{name}/old-state_5.sqlite"))?;
        }

        for item in SHARED {
            let shared = p.base.join(item);
            if !shared.exists() {
                continue;
            }
            let target = dir.join(item);
            if target.is_symlink() {
                std::fs::remove_file(&target)?;
            } else if target.exists() {
                if item == "state_5.sqlite" && points_at_base_db {
                    continue;
                }
                move_aside(&target, backup, &format!("{name}/old-{item}"))?;
            }
            crate::platform::fs::create_link(&shared, &target)?;
        }

        // Sidebars should list the same projects in every window without
        // touching which account each window is signed in as.
        let base_state = p.base.join(".codex-global-state.json");
        let prof_state = dir.join(".codex-global-state.json");
        if base_state.exists() {
            merge_global_state(&base_state, &prof_state, &backup.join(name))?;
        }

        for side in ["state_5.sqlite-wal", "state_5.sqlite-shm"] {
            let target = dir.join(side);
            if target.exists() && !target.is_symlink() {
                move_aside(&target, backup, &format!("{name}/old-{side}"))?;
            }
        }
    }

    // 3. Verify: same inode through every path, and the DB is intact.
    let base_db = p.base.join("state_5.sqlite");
    let mut inodes = Vec::new();
    if base_db.exists() {
        if !db::quick_check(&base_db)? {
            return Err(Error::Msg(
                "shared database failed PRAGMA quick_check".into(),
            ));
        }
        let base_ino = db::inode(&base_db)?;
        inodes.push(("base".to_string(), base_ino));
        for (name, dir) in &profile_dirs {
            let prof_db = dir.join("state_5.sqlite");
            if prof_db.exists() {
                let ino = db::inode(&prof_db)?;
                if ino != base_ino {
                    return Err(Error::Msg(format!(
                        "profile '{name}' is not sharing the base database after repair"
                    )));
                }
                inodes.push((name.clone(), ino));
            }
        }
    }

    let mut final_report = inspect(p);
    final_report.inodes = inodes;
    Ok(final_report)
}

/// Merge the shared project list into a per-window desktop state file.
/// The window's own `local-document-identity` is never touched: that is what
/// keeps each window on its own account while showing the same projects.
pub fn merge_global_state(base: &Path, target: &Path, backup: &Path) -> Result<()> {
    if !base.exists() {
        return Ok(());
    }
    let src: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(base)?)
        .map_err(|e| Error::Msg(format!("bad base state json: {e}")))?;
    let mut dst: serde_json::Value = if target.exists() {
        serde_json::from_str(&std::fs::read_to_string(target)?)
            .map_err(|e| Error::Msg(format!("bad profile state json: {e}")))?
    } else {
        serde_json::json!({})
    };
    if !src.is_object() || !dst.is_object() {
        return Err(Error::Msg("desktop state must be a JSON object".into()));
    }

    // Migration flags and server project IDs belong to each profile. Copying
    // those can mark a new account as migrated before it imports local projects.
    for key in ["local-projects", "thread-project-assignments"] {
        let Some(src_map) = src.get(key).and_then(|v| v.as_object()) else {
            continue;
        };
        if dst.get(key).is_some_and(|v| !v.is_object()) {
            return Err(Error::Msg(format!("profile {key} must be an object")));
        }
        let mut merged = dst
            .get(key)
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for (k, v) in src_map {
            merged.entry(k.clone()).or_insert_with(|| v.clone());
        }
        dst[key] = serde_json::Value::Object(merged);
    }

    if let Some(order) = src.get("project-order").and_then(|v| v.as_array()) {
        if dst.get("project-order").is_some_and(|v| !v.is_array()) {
            return Err(Error::Msg("profile project-order must be an array".into()));
        }
        let mut merged = dst
            .get("project-order")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for id in order {
            if !merged.contains(id) {
                merged.push(id.clone());
            }
        }
        dst["project-order"] = serde_json::Value::Array(merged);
    }

    // Sidebar presentation only; never the identity block.
    if dst
        .get("electron-persisted-atom-state")
        .is_some_and(|v| !v.is_object())
    {
        return Err(Error::Msg("profile atom state must be an object".into()));
    }
    if let (Some(s_atom), Some(d_atom)) = (
        src.get("electron-persisted-atom-state")
            .and_then(|v| v.as_object()),
        Some(
            dst.get("electron-persisted-atom-state")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default(),
        ),
    ) {
        let mut atoms = d_atom;
        for k in [
            "flat-project-sidebar-preferences-v1",
            "new-task-project-by-computer",
        ] {
            if let Some(v) = s_atom.get(k) {
                atoms.entry(k.to_string()).or_insert_with(|| v.clone());
            }
        }
        dst["electron-persisted-atom-state"] = serde_json::Value::Object(atoms);
    }

    let text = serde_json::to_string_pretty(&dst)
        .map_err(|e| Error::Msg(format!("could not serialise state: {e}")))?;
    if target.exists() && std::fs::read_to_string(target)? == text {
        return Ok(());
    }
    if target.exists() {
        std::fs::create_dir_all(backup)?;
        std::fs::copy(target, backup.join("global-state.json"))?;
    }
    // Write alongside the destination and rename, so readers never see partial JSON.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = target.with_file_name(format!(
        ".codex-global-state-{}-{nonce}.tmp",
        std::process::id()
    ));
    use std::io::Write;
    let result = (|| -> Result<()> {
        let mut file = crate::platform::fs::create_new_private(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    Ok(())
}
