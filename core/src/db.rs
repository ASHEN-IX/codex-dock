use crate::error::{Error, Result};
use std::path::Path;
use std::process::Command;

fn run(db: &Path, sql: &str) -> Result<String> {
    let out = Command::new("sqlite3").arg(db).arg(sql).output().map_err(|e| {
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
    out.trim().parse::<usize>().map_err(|_| Error::Msg(format!("unexpected sqlite output: {out:?}")))
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
    Ok(out.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect())
}

pub fn quick_check(db: &Path) -> Result<bool> {
    let out = run(db, "PRAGMA quick_check;")?;
    Ok(out.trim() == "ok")
}

pub fn inode(path: &Path) -> Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(path)?.ino())
}

const SCHEMA: &str = "create table if not exists projects (id text primary key, name text not null, metadata text not null default '{}', position integer not null, created_at_ms integer not null, updated_at_ms integer not null);
create table if not exists project_roots (project_id text not null, position integer not null, path text not null);
create table if not exists threads (id text primary key, rollout_path text not null, cwd text, title text, created_at integer, updated_at integer, archived integer not null default 0);";

pub fn create_empty(db: &Path) -> Result<()> {
    run(db, SCHEMA)?;
    Ok(())
}

pub fn insert_thread(db: &Path, id: &str, rollout: &str) -> Result<()> {
    run(db, &format!(
        "insert into threads (id, rollout_path) values ('{id}', '{rollout}');"
    ))?;
    Ok(())
}
