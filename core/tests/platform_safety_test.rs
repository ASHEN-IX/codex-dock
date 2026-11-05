//! Platform safety regression tests for Phase 1.1:
//! - Account creation atomicity and transactional rollback.
//! - Pre-existing directory preservation.
//! - Active process and database-writer repair refusal.
//! - Platform capability separation (symlinks vs shared-store compatibility).
//! - Windows and macOS isolation-only behavior.

#[cfg(target_os = "linux")]
use codex_dock_core::db;
use codex_dock_core::{accounts, store, windows, Paths};
#[cfg(target_os = "linux")]
use std::fs::File;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let t = std::env::temp_dir().join(format!("cd-safety-{}-{}-{}", tag, std::process::id(), id));
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
fn active_sqlite_transaction_refuses_mutating_repair_and_preserves_database() {
    let t = tmp("sqlite_active_tx");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    let base_db = p.base.join("state_5.sqlite");
    db::create_empty(&base_db).unwrap();
    db::insert_thread(&base_db, "initial_thread", "/x/init.jsonl").unwrap();

    // Enable WAL mode
    let _ = db::run_query(&base_db, "PRAGMA journal_mode=WAL;").unwrap();

    // Open a real background SQLite process that holds an active write transaction
    // with BEGIN IMMEDIATE and an uncommitted INSERT
    let script = format!(
        "import sqlite3, time\n\
         conn = sqlite3.connect('{}')\n\
         conn.execute('BEGIN IMMEDIATE;')\n\
         conn.execute(\"INSERT INTO threads (id, rollout_path) VALUES ('tx_thread', '/x/tx.jsonl');\")\n\
         print('READY', flush=True)\n\
         time.sleep(2)\n\
         conn.rollback()\n\
         conn.close()",
        base_db.display()
    );
    let mut child = std::process::Command::new("python3")
        .arg("-c")
        .arg(&script)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("must spawn python3 holding transaction");

    use std::io::{BufRead, BufReader};
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("READY"));

    let bak = t.join("bak");
    let empty_proc = t.join("empty_proc");
    std::fs::create_dir_all(&empty_proc).unwrap();

    let err = store::repair_with_proc_root(&p, &bak, Some(&empty_proc)).unwrap_err();
    assert!(
        err.to_string()
            .contains("locked by an active SQLite connection or transaction")
            || err.to_string().contains("database is locked")
            || err.to_string().contains("open file handle"),
        "repair must refuse when a real SQLite connection holds an active transaction: {err}"
    );

    assert!(
        !bak.exists(),
        "refused repair must not create backup or touch store"
    );

    let _ = child.wait();

    // Verify the database is 100% intact with original data
    assert_eq!(db::thread_count(&base_db).unwrap(), 1);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn concurrent_repair_and_launch_are_mutually_exclusive() {
    let t = tmp("dock_concurrency");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();

    // 1. Hold .dock-repair.lock
    let repair_lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(p.base.join(".dock-repair.lock"))
        .unwrap();
    repair_lock.lock().unwrap();

    // 2. Simultaneous repair must be refused
    let bak = t.join("bak");
    let empty_proc = t.join("empty_proc");
    std::fs::create_dir_all(&empty_proc).unwrap();
    let err = store::repair_with_proc_root(&p, &bak, Some(&empty_proc)).unwrap_err();
    assert!(
        err.to_string()
            .contains("another Codex Dock repair operation is currently running"),
        "expected concurrent repair refusal, got: {err}"
    );

    // 3. Launch must be refused while repair lock is held
    accounts::create(&p, "launch_target", None).unwrap();
    let launch_err = windows::launch(&p, "launch_target").unwrap_err();
    assert!(
        launch_err
            .to_string()
            .contains("Codex Dock repair is currently in progress"),
        "expected launch refusal during repair, got: {launch_err}"
    );

    let _ = repair_lock.unlock();
    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn repair_refuses_when_proc_root_missing_or_unreadable() {
    let t = tmp("proc_unreadable");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    let bak = t.join("bak");

    // 1. None proc_root
    let err1 = store::repair_with_proc_root(&p, &bak, None).unwrap_err();
    assert!(err1
        .to_string()
        .contains("process discovery root is unavailable"));

    // 2. Non-existent proc_root
    let non_existent = t.join("does_not_exist");
    let err2 = store::repair_with_proc_root(&p, &bak, Some(&non_existent)).unwrap_err();
    assert!(
        err2.to_string().contains("does not exist") || err2.to_string().contains("failed to read"),
        "got: {err2}"
    );

    // 3. Unreadable proc_root (mode 0000)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let no_perm = t.join("no_perm");
        std::fs::create_dir_all(&no_perm).unwrap();
        std::fs::set_permissions(&no_perm, std::fs::Permissions::from_mode(0o000)).unwrap();
        let err3 = store::repair_with_proc_root(&p, &bak, Some(&no_perm)).unwrap_err();
        assert!(
            err3.to_string().contains("failed to read")
                || err3.to_string().contains("cannot read")
                || err3.to_string().contains("Permission denied"),
            "got: {err3}"
        );
        let _ = std::fs::set_permissions(&no_perm, std::fs::Permissions::from_mode(0o700));
    }
    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn real_linux_process_scanner_succeeds_against_actual_procfs() {
    let real_proc = std::path::Path::new("/proc");
    assert!(real_proc.is_dir());
    let res = windows::scan_running(real_proc);
    assert!(
        res.is_ok(),
        "scan_running against actual /proc must succeed: {:?}",
        res.err()
    );
    let procs = res.unwrap();
    for proc in procs {
        assert!(proc.pid > 0);
    }
}

#[test]
fn account_creation_and_cloning_reject_path_traversal() {
    let t = tmp("traversal_reject");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();

    // Traversal in name
    for bad in ["../evil", "../../etc", "/root", "a/b", "c\\d", "..", "."] {
        assert!(
            accounts::add_account(&p, bad, None).is_err(),
            "must reject bad name: {bad}"
        );
        assert!(
            accounts::create(&p, bad, None).is_err(),
            "must reject bad name: {bad}"
        );
    }

    // Traversal in clone_from
    accounts::create(&p, "valid_account", None).unwrap();
    for bad_src in ["../etc", "../../root", "/tmp", "a/b", "..", "."] {
        assert!(
            accounts::add_account(&p, "child", Some(bad_src)).is_err(),
            "must reject bad clone_from: {bad_src}"
        );
        assert!(
            accounts::create(&p, "child2", Some(bad_src)).is_err(),
            "must reject bad clone_from: {bad_src}"
        );
    }
    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(unix)]
fn cloning_refuses_symlinks_escaping_source_profile_root() {
    let t = tmp("symlink_escape");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();

    // Create outside secret directory
    let outside = t.join("outside_secrets");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.key"), b"CONFIDENTIAL").unwrap();

    // Create source profile with malicious symlink in skills/ pointing outside
    let src_dir = p.profiles.join("src_with_link");
    std::fs::create_dir_all(src_dir.join("skills")).unwrap();
    std::os::unix::fs::symlink(&outside, src_dir.join("skills/escaped_dir")).unwrap();

    // Creation must fail and refuse to clone the escaping symlink
    let res = accounts::add_account(&p, "target", Some("src_with_link"));
    assert!(res.is_err(), "must refuse to clone escaping symlink");
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("escapes source profile root"),
        "expected escape error, got: {err_msg}"
    );

    // Target directory must not exist on disk
    assert!(!p.profiles.join("target").exists());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn simultaneous_creation_attempts_are_synchronized() {
    let t = tmp("concurrent_create");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();

    use std::sync::{Arc, Barrier};
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = Vec::new();

    for _ in 0..4 {
        let p_clone = Paths::new(p.base.clone(), p.profiles.clone());
        let b = barrier.clone();
        handles.push(std::thread::spawn(move || {
            b.wait();
            accounts::create(&p_clone, "same_name", None)
        }));
    }

    let mut successes = 0;
    for h in handles {
        if h.join().unwrap().is_ok() {
            successes += 1;
        }
    }
    assert_eq!(successes, 1, "exactly one concurrent creation must succeed");
    assert!(p.profiles.join("same_name").is_dir());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn rollback_aborts_without_deleting_target_if_ownership_token_is_missing_or_mismatched() {
    let t = tmp("ownership_rollback");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();

    // Create a target profile directory simulating external creation or token loss
    let target = p.profiles.join("precious_target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("important.txt"), b"do not delete").unwrap();

    // If ownership token is tampered with or absent, attempting rollback must not delete it
    let token_file = target.join(".dock-creation-token");
    assert!(!token_file.exists());

    // When add_account sees pre-existing directory, it fails preflight without deleting
    assert!(accounts::add_account(&p, "precious_target", None).is_err());
    assert!(target.join("important.txt").exists());
    assert_eq!(
        std::fs::read(target.join("important.txt")).unwrap(),
        b"do not delete"
    );

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
        err.to_string().contains("launch is currently in progress")
            || err
                .to_string()
                .contains("locked by an active process or writer"),
        "repair must refuse when launch lock is held: {err}"
    );

    let _ = lock_file.unlock();
    std::fs::remove_dir_all(&t).ok();
}
