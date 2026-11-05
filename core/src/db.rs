use crate::error::{Error, Result};
use std::path::Path;
use std::process::Command;

fn run(db: &Path, sql: &str) -> Result<String> {
    let out = Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Msg("sqlite3 not found in PATH".into())
            } else {
                Error::Io(e)
            }
        })?;
    if !out.status.success() {
        return Err(Error::Msg(format!(
            "sqlite3 failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn scalar(db: &Path, sql: &str) -> Result<usize> {
    let out = run(db, sql)?;
    out.trim()
        .parse::<usize>()
        .map_err(|_| Error::Msg(format!("unexpected sqlite output: {out:?}")))
}

pub fn thread_count(db: &Path) -> Result<usize> {
    if !db.exists() {
        return Ok(0);
    }
    scalar(db, "select count(*) from threads;")
}

pub fn project_names(db: &Path) -> Result<Vec<String>> {
    if !db.exists() {
        return Ok(Vec::new());
    }
    let out = run(db, "select name from projects order by position;")?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

pub fn quick_check(db: &Path) -> Result<bool> {
    let out = run(db, "PRAGMA quick_check;")?;
    Ok(out.trim() == "ok")
}

/// Proactively verify that SQLite can acquire an EXCLUSIVE lock on the database.
///
/// In SQLite Unix VFS (as well as Windows and macOS VFS), setting `PRAGMA locking_mode = EXCLUSIVE`
/// and initiating an `EXCLUSIVE` transaction requires SQLite to acquire exclusive locks
/// on the database and WAL/SHM coordination structures. If another connection has an active
/// transaction or open writer handle, SQLite will return `SQLITE_BUSY` ("database is locked").
pub fn verify_sqlite_exclusive(db: &Path) -> Result<()> {
    if !db.exists() {
        return Ok(());
    }
    let out = Command::new("sqlite3")
        .arg("-cmd")
        .arg(".timeout 50")
        .arg(db)
        .arg("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; COMMIT;")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Msg("sqlite3 not found in PATH".into())
            } else {
                Error::Io(e)
            }
        })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::Msg(format!(
            "database '{}' is locked by an active SQLite connection or transaction: {stderr}",
            db.display()
        )));
    }
    Ok(())
}

pub fn inode(path: &Path) -> Result<u64> {
    match crate::platform::fs::file_identity(path) {
        Some(ino) => Ok(ino),
        // Never claim files differ when identity cannot be read: treat it as
        // an unread problem so nothing is displaced on a guess.
        None => Err(Error::Msg(
            "file identity is not available on this platform".into(),
        )),
    }
}

const SCHEMA: &str = "create table if not exists projects (id text primary key, name text not null, metadata text not null default '{}', position integer not null, created_at_ms integer not null, updated_at_ms integer not null);
create table if not exists project_roots (project_id text not null, position integer not null, path text not null);
create table if not exists threads (id text primary key, rollout_path text not null, cwd text, title text, created_at integer, updated_at integer, archived integer not null default 0);";

pub fn create_empty(db: &Path) -> Result<()> {
    run(db, SCHEMA)?;
    Ok(())
}

pub fn insert_thread(db: &Path, id: &str, rollout: &str) -> Result<()> {
    run(
        db,
        &format!("insert into threads (id, rollout_path) values ('{id}', '{rollout}');"),
    )?;
    Ok(())
}

pub fn run_query(db: &Path, sql: &str) -> Result<String> {
    run(db, sql)
}

pub fn merge_from(base_db: &Path, src: &Path, profile_dir: &Path, base_dir: &Path) -> Result<()> {
    let sql = format!(
        // NO immutable=1: with WAL, committed rows can live only in the -wal
        // file, and an immutable attach silently skips them -- merging nothing
        // while the caller then moves the database aside.
        "ATTACH 'file:{}?mode=ro' AS src;
         INSERT OR IGNORE INTO projects SELECT * FROM src.projects;
         INSERT OR IGNORE INTO project_roots SELECT * FROM src.project_roots;
         INSERT OR IGNORE INTO threads SELECT * FROM src.threads;
         DETACH src;",
        src.display()
    );
    run(base_db, &sql)?;

    let from = profile_dir.to_string_lossy().to_string();
    let to = base_dir.to_string_lossy().to_string();
    let repoint = format!(
        "UPDATE threads SET rollout_path = replace(rollout_path, '{from}/', '{to}/')
         WHERE rollout_path LIKE '{from}/%';"
    );
    run(base_db, &repoint)?;
    Ok(())
}

/// Copy a profile's session rollouts into the base so merged rows resolve.
pub fn copy_sessions(profile_dir: &Path, base_dir: &Path) -> Result<()> {
    let src = profile_dir.join("sessions");
    if !src.exists() || src.is_symlink() {
        return Ok(());
    }
    let dest = base_dir.join("sessions");
    copy_tree(&src, &dest)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
