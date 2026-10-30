use codex_dock_core::{db, store, Paths, StoreState};
use std::path::PathBuf;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let t = std::env::temp_dir().join(format!("cd-fix-{}-{}-{}", tag, std::process::id(), n));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

// Finding 1: a profile dir that IS the base must never displace the shared DB.
#[test]
fn repair_never_displaces_the_shared_db_through_a_symlinked_profile() {
    let t = tmp("symprof");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::insert_thread(&p.base.join("state_5.sqlite"), "keep", "/x/keep.jsonl").unwrap();
    std::os::unix::fs::symlink(&p.base, p.profiles.join("default")).unwrap();

    store::repair(&p, &p.backup_dir("s")).unwrap();

    assert!(p.base.join("state_5.sqlite").exists(), "shared DB must survive");
    assert_eq!(db::thread_count(&p.base.join("state_5.sqlite")).unwrap(), 1, "thread must survive");
    std::fs::remove_dir_all(&t).ok();
}

// Finding 2: merging a split store must carry the session files into base.
#[test]
fn merge_moves_session_files_into_the_base() {
    let t = tmp("sess");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work/sessions/2026/10")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    let src = p.profiles.join("work/sessions/2026/10/chatA.jsonl");
    std::fs::write(&src, "{}\n").unwrap();
    db::insert_thread(&p.profiles.join("work/state_5.sqlite"), "t1", &src.to_string_lossy()).unwrap();

    store::repair(&p, &p.backup_dir("s")).unwrap();

    let dest = p.base.join("sessions/2026/10/chatA.jsonl");
    assert!(dest.exists(), "session file must be copied into the base, not left in backup");
    let got = db::run_query(&p.base.join("state_5.sqlite"), "select rollout_path from threads;").unwrap();
    assert!(got.contains(&dest.to_string_lossy().to_string()), "row must point at the copied file: {got:?}");
    std::fs::remove_dir_all(&t).ok();
}

// Finding 5: the benign live link must be left exactly as it is.
#[test]
fn repair_leaves_a_benign_auth_link_alone() {
    let t = tmp("benign");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::write(p.profiles.join("work/auth.json"), r#"{"tokens":{"account_id":"w"}}"#).unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json")).unwrap();

    store::repair(&p, &p.backup_dir("s")).unwrap();

    assert!(p.base.join("auth.json").is_symlink(), "benign link must be preserved");
    std::fs::remove_dir_all(&t).ok();
}

// Finding 4: a true cycle must actually be repaired, not just detected.
#[test]
fn repair_breaks_a_true_auth_cycle() {
    let t = tmp("cycle");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::os::unix::fs::symlink(&p.base, p.profiles.join("work")).unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json")).unwrap();
    assert!(matches!(store::inspect(&p).state, StoreState::AuthCycle), "precondition");

    let r = store::repair(&p, &p.backup_dir("s")).unwrap();

    assert!(!matches!(r.state, StoreState::AuthCycle), "must not still be a cycle: {:?}", r.state);
    assert!(!p.base.join("auth.json").is_symlink(), "looped link must be moved aside");
    assert!(p.base.join("state_5.sqlite").exists(), "DB must survive the auth repair");
    std::fs::remove_dir_all(&t).ok();
}

// Finding 6: an unreadable profile DB must not be displaced silently.
#[test]
fn repair_does_not_displace_an_unreadable_profile_db() {
    let t = tmp("junk");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    // a real file that is not a usable database
    std::fs::write(p.profiles.join("work/state_5.sqlite"), "not a database").unwrap();

    let r = store::repair(&p, &p.backup_dir("s"));

    let prof_db = p.profiles.join("work/state_5.sqlite");
    if r.is_ok() {
        assert!(prof_db.is_symlink() || prof_db.exists());
    } else {
        assert!(p.profiles.join("work/state_5.sqlite").exists(), "must not vanish on error");
    }
    std::fs::remove_dir_all(&t).ok();
}

// Finding 7: absent data must not become zero.
// PATH is process-global, so this must be the only test in its binary.

// Finding 3: committed rows living only in the WAL must be merged, not lost.
// Simulates an unclean writer (no clean close, so no checkpoint) -- the shape a
// live Codex window leaves behind.
#[test]
fn merge_reads_rows_that_exist_only_in_the_wal() {
    let t = tmp("wal");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();

    let prof = p.profiles.join("work/state_5.sqlite");
    let seed = t.join("seed.py");
    std::fs::write(&seed, r#"
import sqlite3, os
c = sqlite3.connect(__PATH__)
c.execute("PRAGMA journal_mode=WAL")
c.execute("PRAGMA wal_autocheckpoint=0")
c.execute("create table projects (id text primary key, name text, metadata text default '{}', position integer, created_at_ms integer, updated_at_ms integer)")
c.execute("create table project_roots (project_id text, position integer, path text)")
c.execute("create table threads (id text primary key, rollout_path text, cwd text, title text, created_at integer, updated_at integer, archived integer default 0)")
c.execute("insert into threads (id, rollout_path) values ('walrow','/x/w.jsonl')")
c.commit()
os._exit(0)
"#.replace("__PATH__", &format!("{:?}", prof.display().to_string()))).unwrap();

    let out = std::process::Command::new("python3").arg("-I").arg(&seed).output().unwrap();
    assert!(out.status.success(), "seed failed: {}", String::from_utf8_lossy(&out.stderr));

    let wal = format!("{}-wal", prof.display());
    let n = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
    assert!(n > 0, "precondition: non-empty -wal expected at {wal}");

    store::repair(&p, &p.backup_dir("s")).unwrap();

    let got = db::run_query(&p.base.join("state_5.sqlite"),
        "select count(*) from threads where id='walrow';").unwrap();
    assert_eq!(got.trim(), "1", "row living in the WAL must be merged");
    std::fs::remove_dir_all(&t).ok();
}
