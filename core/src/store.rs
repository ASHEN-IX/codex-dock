use crate::db;
use crate::paths::Paths;

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
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(&p.profiles), std::fs::canonicalize(&p.base)) {
        if a.starts_with(&b) {
            return report(StoreState::Nested { profile: first(&accounts) }, &base_db);
        }
    }

    if !base_db.exists() {
        return report(StoreState::NotConfigured, &base_db);
    }

    // Split: a profile has a DB holding threads that is NOT the shared file.
    // Comparing inodes (not paths) is what makes a profile that is itself a
    // symlink to the base read as shared rather than split.
    let base_ino = db::inode(&base_db).ok();
    for name in &accounts {
        let prof_db = p.profiles.join(name).join("state_5.sqlite");
        if !prof_db.exists() {
            continue;
        }
        if let (Some(b), Ok(pi)) = (base_ino, db::inode(&prof_db)) {
            if b == pi {
                continue;
            }
        }
        if prof_db.is_symlink() {
            continue;
        }
        if let Ok(n) = db::thread_count(&prof_db) {
            if n > 0 {
                return report(StoreState::Split { threads_in_profile: n }, &base_db);
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
        std::os::unix::fs::symlink(&shared, &target)?;
    }
    Ok(())
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
    StoreReport { state, threads, projects, inodes: Vec::new(), error }
}

use crate::error::{Error, Result};
use std::path::Path;

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

pub fn repair(p: &Paths, backup: &Path) -> Result<StoreReport> {
    std::fs::create_dir_all(backup)?;

    let current = inspect(p);

    // Nested profiles dir can only be fixed by the user; linking would recurse.
    if let StoreState::Nested { .. } = current.state {
        return Err(Error::Msg(
            "profiles directory is inside the base store; move it out before repairing".into(),
        ));
    }

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

    // 2. Per-profile: merge a split store, then link every shared path.
    let mut profile_dirs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&p.profiles) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') && e.path().is_dir() {
                profile_dirs.push((name, e.path()));
            }
        }
    }
    profile_dirs.sort();

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
            std::os::unix::fs::symlink(&shared, &target)?;
        }

        // Sidebars should list the same projects in every window without
        // touching which account each window is signed in as.
        let base_state = p.base.join(".codex-global-state.json");
        let prof_state = dir.join(".codex-global-state.json");
        if base_state.exists() && prof_state.exists() {
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
            return Err(Error::Msg("shared database failed PRAGMA quick_check".into()));
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
    if !base.exists() || !target.exists() {
        return Ok(());
    }
    let src: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(base)?)
        .map_err(|e| Error::Msg(format!("bad base state json: {e}")))?;
    let mut dst: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(target)?)
        .map_err(|e| Error::Msg(format!("bad profile state json: {e}")))?;

    std::fs::create_dir_all(backup)?;
    std::fs::copy(target, backup.join("global-state.json"))?;

    for key in ["local-projects", "app-server-projects-migration-by-host"] {
        let (Some(src_map), Some(dst_map)) = (
            src.get(key).and_then(|v| v.as_object()),
            dst.get(key).and_then(|v| v.as_object()),
        ) else {
            continue;
        };
        let mut merged = dst_map.clone();
        for (k, v) in src_map {
            merged.entry(k.clone()).or_insert_with(|| v.clone());
        }
        dst[key] = serde_json::Value::Object(merged);
    }

    // Sidebar presentation only; never the identity block.
    if let (Some(s_atom), Some(d_atom)) = (
        src.get("electron-persisted-atom-state").and_then(|v| v.as_object()),
        dst.get("electron-persisted-atom-state").and_then(|v| v.as_object()).cloned(),
    ) {
        let mut atoms = d_atom;
        for k in ["flat-project-sidebar-preferences-v1", "new-task-project-by-computer"] {
            if let Some(v) = s_atom.get(k) {
                atoms.entry(k.to_string()).or_insert_with(|| v.clone());
            }
        }
        dst["electron-persisted-atom-state"] = serde_json::Value::Object(atoms);
    }

    let text = serde_json::to_string_pretty(&dst)
        .map_err(|e| Error::Msg(format!("could not serialise state: {e}")))?;
    std::fs::write(target, text)?;
    Ok(())
}
