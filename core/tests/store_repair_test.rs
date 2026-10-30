use codex_dock_core::{accounts, db, store, Paths, StoreState};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let t = std::env::temp_dir().join(format!("cd-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn repair_breaks_auth_cycle_and_links_store() {
    let t = tmp("rep");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    std::fs::write(p.profiles.join("work/auth.json"), r#"{"tokens":{"account_id":"abc"}}"#).unwrap();
    // The benign live shape: base/auth.json -> a REAL profile's auth.json.
    // It resolves fine, so repair must leave it exactly as it is.
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json")).unwrap();

    let bak = p.backup_dir("stamp");
    let r = store::repair(&p, &bak).unwrap();
    assert!(matches!(r.state, StoreState::Shared), "got {:?}", r.state);
    assert!(p.base.join("auth.json").is_symlink(), "benign link must be preserved");
    let (id, ok) = codex_dock_core::accounts::auth_info(&p.base.join("auth.json"));
    assert_eq!(id.as_deref(), Some("abc"), "and still resolve to the account");
    assert!(ok);
    assert_eq!(
        db::inode(&p.base.join("state_5.sqlite")).unwrap(),
        db::inode(&p.profiles.join("work/state_5.sqlite")).unwrap()
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn repair_is_idempotent() {
    let t = tmp("idem");
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

#[test]
fn repair_merges_split_store_and_preserves_original() {
    let t = tmp("merge");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    db::insert_thread(&p.profiles.join("work/state_5.sqlite"), "t1", "/x/one.jsonl").unwrap();

    let bak = p.backup_dir("stamp");
    let r = store::repair(&p, &bak).unwrap();
    assert!(matches!(r.state, StoreState::Shared), "got {:?}", r.state);
    assert_eq!(db::thread_count(&p.base.join("state_5.sqlite")).unwrap(), 1);
    // the profile's old file was preserved, not deleted
    assert!(bak.join("work/old-state_5.sqlite").exists(), "original must be kept");
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn repair_refuses_when_profiles_dir_is_nested() {
    let t = tmp("nested");
    let p = Paths::new(t.join(".codex"), t.join(".codex"));
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    let bak = p.backup_dir("stamp");
    assert!(store::repair(&p, &bak).is_err());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn repair_repoints_rollout_paths_into_base() {
    let t = tmp("repoint");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    let roll = format!("{}/work/sessions/y.jsonl", p.profiles.to_string_lossy());
    db::insert_thread(&p.profiles.join("work/state_5.sqlite"), "t2", &roll).unwrap();

    let bak = p.backup_dir("stamp");
    store::repair(&p, &bak).unwrap();
    let got = db::run_query(&p.base.join("state_5.sqlite"), "select rollout_path from threads;").unwrap();
    let base = p.base.to_string_lossy().to_string();
    assert!(got.contains(&format!("{base}/sessions/y.jsonl")), "got {got:?}");
    assert!(!got.contains(".codex-profiles"), "profile segment must be gone: {got:?}");
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn repair_keeps_own_auth_per_profile() {
    let t = tmp("auth");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    accounts::create(&p, "work", None).unwrap();
    std::fs::write(p.profiles.join("work/auth.json"), r#"{"tokens":{"account_id":"w1"}}"#).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    store::repair(&p, &p.backup_dir("s")).unwrap();
    let (id, _) = accounts::auth_info(&p.profiles.join("work/auth.json"));
    assert_eq!(id.as_deref(), Some("w1"));
    std::fs::remove_dir_all(&t).ok();
}
