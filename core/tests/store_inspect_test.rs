#![cfg(unix)]

use codex_dock_core::{db, store, Paths, StoreState};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let t = std::env::temp_dir().join(format!("cd-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn detects_shared_store_by_inode() {
    let t = tmp("insp");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    std::os::unix::fs::symlink(
        p.base.join("state_5.sqlite"),
        p.profiles.join("work/state_5.sqlite"),
    )
    .unwrap();
    let r = store::inspect(&p);
    assert!(matches!(r.state, StoreState::Shared), "got {:?}", r.state);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn detects_auth_cycle() {
    let t = tmp("cycle");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    // A true cycle: base/auth.json -> <profile>/auth.json, and the profile
    // directory itself resolves back into the base, so following auth.json loops.
    std::fs::remove_dir_all(p.profiles.join("work")).unwrap();
    std::os::unix::fs::symlink(&p.base, p.profiles.join("work")).unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json"))
        .unwrap();
    let r = store::inspect(&p);
    assert!(
        matches!(r.state, StoreState::AuthCycle),
        "got {:?}",
        r.state
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn detects_nested_profile_dir() {
    let t = tmp("nest");
    let p = Paths::new(t.join(".codex"), t.join(".codex"));
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    let r = store::inspect(&p);
    assert!(
        matches!(r.state, StoreState::Nested { .. }),
        "got {:?}",
        r.state
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn detects_split_store_with_threads() {
    let t = tmp("split");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    db::insert_thread(&p.profiles.join("work/state_5.sqlite"), "t1", "/x/y.jsonl").unwrap();
    let r = store::inspect(&p);
    assert!(
        matches!(
            r.state,
            StoreState::Split {
                threads_in_profile: 1
            }
        ),
        "got {:?}",
        r.state
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn not_configured_when_base_db_missing() {
    let t = tmp("none");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    let r = store::inspect(&p);
    assert!(
        matches!(r.state, StoreState::NotConfigured),
        "got {:?}",
        r.state
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn symlinked_auth_into_a_real_profile_is_not_a_cycle() {
    // The live setup: base/auth.json -> work/auth.json where work is a REAL dir.
    let t = tmp("authok");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::write(
        p.profiles.join("work/auth.json"),
        r#"{"tokens":{"account_id":"w"}}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json"))
        .unwrap();
    let r = store::inspect(&p);
    assert!(
        !matches!(r.state, StoreState::AuthCycle),
        "false positive: {:?}",
        r.state
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn profile_symlinked_to_base_is_a_real_cycle() {
    // The genuinely dangerous shape: the profile DIR points back at the base.
    let t = tmp("authcyc");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::remove_dir_all(&p.base).ok();
    std::fs::create_dir_all(&p.base).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::write(p.base.join("auth.json.tmp"), "{}").unwrap();
    std::fs::remove_file(p.base.join("auth.json.tmp")).unwrap();
    std::os::unix::fs::symlink(&p.base, p.profiles.join("work")).unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json"))
        .unwrap();
    let r = store::inspect(&p);
    assert!(
        matches!(r.state, StoreState::AuthCycle),
        "expected cycle, got {:?}",
        r.state
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn a_profile_symlinked_to_base_is_shared_not_split() {
    // The live setup: profiles/default -> ~/.codex. It is the same database,
    // so it must not be mistaken for a split store.
    let t = tmp("deflink");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::insert_thread(&p.base.join("state_5.sqlite"), "t1", "/x.jsonl").unwrap();
    std::os::unix::fs::symlink(&p.base, p.profiles.join("default")).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    std::os::unix::fs::symlink(
        p.base.join("state_5.sqlite"),
        p.profiles.join("work/state_5.sqlite"),
    )
    .unwrap();
    let r = store::inspect(&p);
    assert!(matches!(r.state, StoreState::Shared), "got {:?}", r.state);
    std::fs::remove_dir_all(&t).ok();
}
