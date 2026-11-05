//! Platform safety regression tests for Phase 1.1:
//! - Account creation atomicity and transactional rollback.
//! - Pre-existing directory preservation.
//! - Active process and database-writer repair refusal.
//! - Platform capability separation (symlinks vs shared-store compatibility).
//! - Windows and macOS isolation-only behavior.

use codex_dock_core::{accounts, db, store, Paths};
use std::fs::File;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let t = std::env::temp_dir().join(format!("cd-safety-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn preflight_preserves_existing_directory_without_overwriting_or_deleting() {
    let t = tmp("preserve");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();

    let existing = p.profiles.join("valuable");
    std::fs::create_dir_all(&existing).unwrap();
    let secret_file = existing.join("important_user_data.txt");
    std::fs::write(&secret_file, b"precious contents").unwrap();

    // 1. add_account must refuse pre-existing directory
    let res = accounts::add_account(&p, "valuable", None);
    assert!(res.is_err(), "must fail preflight when directory exists");
    assert!(
        secret_file.exists(),
        "preflight failure must NEVER delete pre-existing files"
    );
    assert_eq!(
        std::fs::read(&secret_file).unwrap(),
        b"precious contents",
        "pre-existing contents must be 100% byte-for-byte preserved"
    );

    // 2. accounts::create directly must also refuse and preserve
    let res2 = accounts::create(&p, "valuable", None);
    assert!(res2.is_err());
    assert_eq!(
        std::fs::read(&secret_file).unwrap(),
        b"precious contents",
        "direct create must never overwrite pre-existing files"
    );

    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn partial_account_creation_failure_leaves_no_residual_directory() {
    let t = tmp("partial");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();

    // Specifying a nonexistent clone source must fail before or during creation
    // and must leave NO partially initialized profile on disk.
    let res = accounts::add_account(&p, "broken", Some("nonexistent_source"));
    assert!(res.is_err());
    assert!(
        !p.profiles.join("broken").exists(),
        "failed account creation must leave no residual directory behind"
    );

    let res2 = accounts::create(&p, "broken2", Some("nonexistent_source"));
    assert!(res2.is_err());
    assert!(
        !p.profiles.join("broken2").exists(),
        "create with invalid clone source must leave no directory behind"
    );

    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn linux_add_account_links_shared_store_and_rolls_back_if_linking_fails() {
    let t = tmp("rollback");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();

    // 1. Normal successful creation on Linux links shared store
    let created = accounts::add_account(&p, "healthy", None).unwrap();
    assert_eq!(created.account.name, "healthy");
    assert!(created.shared_store, "Linux must link shared store");
    assert!(p.profiles.join("healthy/state_5.sqlite").is_symlink());

    // 2. If linking fails (e.g. base global state contains corrupted JSON causing sync_projects to fail)
    let t_fail = tmp("rollback_fail");
    let p_fail = Paths::new(t_fail.join(".codex"), t_fail.join(".codex-profiles"));
    std::fs::create_dir_all(&p_fail.base).unwrap();
    std::fs::create_dir_all(&p_fail.profiles).unwrap();
    db::create_empty(&p_fail.base.join("state_5.sqlite")).unwrap();
    // Write corrupted JSON into .codex-global-state.json so sync_projects in link_profile fails
    std::fs::write(
        p_fail.base.join(".codex-global-state.json"),
        b"{ bad unclosed json",
    )
    .unwrap();

    let res = accounts::add_account(&p_fail, "will_fail", None);
    assert!(res.is_err(), "must fail when linking cannot complete");
    assert!(
        !p_fail.profiles.join("will_fail").exists(),
        "transactional rollback must remove the newly created profile on linking failure"
    );

    std::fs::remove_dir_all(&t).ok();
    std::fs::remove_dir_all(&t_fail).ok();
}

#[test]
#[cfg(target_os = "macos")]
fn macos_uses_isolation_only_and_refuses_shared_store_operations() {
    let t = tmp("mac_iso");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();

    // add_account on macOS must succeed in isolation-only mode
    let res = accounts::add_account(&p, "isolated_mac", None).unwrap();
    assert_eq!(res.account.name, "isolated_mac");
    assert!(
        !res.shared_store,
        "macOS must remain isolated without shared-store linking"
    );
    assert!(
        p.profiles.join("isolated_mac").is_dir(),
        "profile directory must exist"
    );
    assert!(
        !p.profiles.join("isolated_mac/state_5.sqlite").exists(),
        "no shared database symlinks may be created on macOS"
    );

    // Direct shared-store operations must refuse
    assert!(store::link_profile(&p, "isolated_mac").is_err());
    assert!(store::repair(&p, &t.join("bak")).is_err());

    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "windows")]
fn windows_uses_isolation_only_and_refuses_shared_store_operations() {
    let t = tmp("win_iso");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();

    // add_account on Windows must succeed in isolation-only mode
    let res = accounts::add_account(&p, "isolated_win", None).unwrap();
    assert_eq!(res.account.name, "isolated_win");
    assert!(
        !res.shared_store,
        "Windows must remain isolated without shared-store linking"
    );
    assert!(
        p.profiles.join("isolated_win").is_dir(),
        "profile directory must exist"
    );
    assert!(
        !p.profiles.join("isolated_win/state_5.sqlite").exists(),
        "no shared database symlinks may be created on Windows"
    );

    // Direct shared-store operations must refuse
    assert!(store::link_profile(&p, "isolated_win").is_err());
    assert!(store::repair(&p, &t.join("bak")).is_err());

    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn active_writer_repair_refusal_when_windows_running() {
    let t = tmp("running_refusal");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();

    // Create a mock procfs containing an active Codex window for this profile
    let mock_proc = t.join("mock_proc");
    let pid_dir = mock_proc.join("9999");
    std::fs::create_dir_all(&pid_dir).unwrap();

    let udir = p.user_data_dir("work");
    let cmdline = format!(
        "/usr/lib/chatgpt/ChatGPT\0--user-data-dir={}\0",
        udir.display()
    );
    std::fs::write(pid_dir.join("cmdline"), cmdline.as_bytes()).unwrap();

    let bak = t.join("bak");
    let err = store::repair_with_proc_root(&p, &bak, Some(&mock_proc)).unwrap_err();
    assert!(
        err.to_string().contains("while Codex windows are running"),
        "repair must refuse when a Codex window is running: {err}"
    );
    assert!(
        !bak.exists(),
        "refused repair must not create backup or touch store"
    );

    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn active_writer_repair_refusal_when_database_locked() {
    let t = tmp("lock_refusal");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    let base_db = p.base.join("state_5.sqlite");
    db::create_empty(&base_db).unwrap();

    // Acquire an exclusive file lock on the base database
    let lock_file = File::options()
        .read(true)
        .write(true)
        .open(&base_db)
        .unwrap();
    lock_file
        .lock()
        .expect("must acquire exclusive lock in test");

    let bak = t.join("bak");
    // Synthetic empty procfs so process check passes and lock check is exercised
    let empty_proc = t.join("empty_proc");
    std::fs::create_dir_all(&empty_proc).unwrap();

    let err = store::repair_with_proc_root(&p, &bak, Some(&empty_proc)).unwrap_err();
    assert!(
        err.to_string()
            .contains("locked by an active process or writer"),
        "repair must refuse when database has an active lock: {err}"
    );
    assert!(
        !bak.exists(),
        "refused repair must not create backup or touch store"
    );

    let _ = lock_file.unlock();
    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn active_writer_repair_refusal_when_launch_lock_held() {
    let t = tmp("launch_lock_refusal");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    let work_dir = p.profiles.join("work");
    std::fs::create_dir_all(&work_dir).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();

    let launch_lock_path = work_dir.join(".desktop-launch.lock");
    let lock_file = File::create(&launch_lock_path).unwrap();
    lock_file.lock().expect("must lock launch file");

    let bak = t.join("bak");
    let empty_proc = t.join("empty_proc");
    std::fs::create_dir_all(&empty_proc).unwrap();

    let err = store::repair_with_proc_root(&p, &bak, Some(&empty_proc)).unwrap_err();
    assert!(
        err.to_string()
            .contains("locked by an active process or writer"),
        "repair must refuse when launch lock is held: {err}"
    );

    let _ = lock_file.unlock();
    std::fs::remove_dir_all(&t).ok();
}
