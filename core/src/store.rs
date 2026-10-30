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

    // Nested: the profiles dir lives inside the base store.
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(&p.profiles), std::fs::canonicalize(&p.base)) {
        if a.starts_with(&b) {
            return report(StoreState::Nested { profile: first(&accounts) }, &base_db);
        }
    }

    // Auth cycle: base auth.json resolves into a profile that resolves back to base.
    let base_auth = p.base.join("auth.json");
    if base_auth.is_symlink() {
        if let Ok(target) = std::fs::canonicalize(&base_auth) {
            for name in &accounts {
                if let Ok(prof) = std::fs::canonicalize(p.profiles.join(name)) {
                    if target.starts_with(&prof) {
                        return report(StoreState::AuthCycle, &base_db);
                    }
                }
            }
        }
    }

    if !base_db.exists() {
        return report(StoreState::NotConfigured, &base_db);
    }

    // Split: a profile has its own real DB holding threads.
    for name in &accounts {
        let prof_db = p.profiles.join(name).join("state_5.sqlite");
        if prof_db.exists() && !prof_db.is_symlink() {
            if let Ok(n) = db::thread_count(&prof_db) {
                if n > 0 {
                    return report(StoreState::Split { threads_in_profile: n }, &base_db);
                }
            }
        }
    }

    report(StoreState::Shared, &base_db)
}

fn first(v: &[String]) -> String {
    v.first().cloned().unwrap_or_default()
}

fn report(state: StoreState, base_db: &std::path::Path) -> StoreReport {
    let threads = db::thread_count(base_db).unwrap_or(0);
    let projects = db::project_names(base_db).unwrap_or_default();
    StoreReport { state, threads, projects, inodes: Vec::new() }
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
    let base_auth = p.base.join("auth.json");
    if base_auth.is_symlink() {
        if let Ok(target) = std::fs::canonicalize(&base_auth) {
            let mut cyclic = false;
            if let Ok(rd) = std::fs::read_dir(&p.profiles) {
                for e in rd.flatten() {
                    if let Ok(prof) = std::fs::canonicalize(e.path()) {
                        if target.starts_with(&prof) {
                            cyclic = true;
                        }
                    }
                }
            }
            if cyclic {
                let bytes = std::fs::read(&target)?;
                std::fs::copy(&target, backup.join("base-auth.json"))?;
                let link = p.base.join("auth.json");
                let stale = backup.join("base-auth.json.link");
                std::fs::create_dir_all(backup)?;
                std::fs::rename(&link, &stale)?;
                std::fs::write(&link, bytes)?;
            }
        }
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

    for (name, dir) in &profile_dirs {
        let prof_db = dir.join("state_5.sqlite");
        if prof_db.exists() && !prof_db.is_symlink() {
            if let Ok(n) = db::thread_count(&prof_db) {
                if n > 0 {
                    db::merge_from(&p.base.join("state_5.sqlite"), &prof_db, dir, &p.base)?;
                }
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
                move_aside(&target, backup, &format!("{name}/old-{item}"))?;
            }
            std::os::unix::fs::symlink(&shared, &target)?;
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
