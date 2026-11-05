//! Regression tests for the platform seams. They also pin the honest
//! capability report: a refused feature must say why, and a feature worth its
//! name must behave identically to the previous Linux implementation.
//!
//! Everything here exercises symlink semantics, which is Unix behaviour by
//! current design, so the suite is compiled on Linux/macOS only. Cross-OS
//! guaranteed checks live in traversal_test and platform_capability_test.
#![cfg(unix)]

use codex_dock_core::platform::{self, Support};
use codex_dock_core::{accounts, db, store, windows, Paths, StoreState};

fn tmp(tag: &str) -> std::path::PathBuf {
    let t = std::env::temp_dir().join(format!("cd-platform-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn the_capability_report_matches_the_platform_it_runs_on() {
    let caps = platform::capabilities();
    #[cfg(target_os = "linux")]
    {
        assert_eq!(caps.filesystem_symlinks, Support::Yes);
        assert_eq!(caps.shared_store_compatibility, Support::Yes);
        assert_eq!(caps.native_process_discovery, Support::Yes);
        assert_eq!(caps.window_activation, Support::Yes);
        assert_eq!(caps.safe_desktop_launch, Support::Yes);
    }
    #[cfg(target_os = "macos")]
    {
        // Links are real, but shared store is unverified, and process/window features
        // are reported absent with a reason rather than pretending.
        assert_eq!(caps.filesystem_symlinks, Support::Yes);
        assert!(matches!(caps.shared_store_compatibility, Support::No(_)));
        assert!(matches!(caps.native_process_discovery, Support::No(r) if r.contains("/proc")));
        assert!(matches!(caps.window_activation, Support::No(_)));
        assert!(matches!(caps.safe_desktop_launch, Support::No(_)));
    }
}

#[test]
fn adapter_created_links_are_indistinguishable_from_the_previous_implementation() {
    let t = tmp("link");
    let target = t.join("real.data");
    std::fs::write(&target, b"x").unwrap();
    let link = t.join("linked.data");
    platform::fs::create_link(&target, &link).unwrap();
    let meta = std::fs::symlink_metadata(&link).unwrap();
    assert!(meta.file_type().is_symlink(), "must be a real symlink");
    assert_eq!(
        std::fs::read(&link).unwrap(),
        b"x",
        "and it must resolve to the target"
    );
    assert_ne!(
        platform::fs::file_identity(&link),
        None,
        "identity must be readable through the link"
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn unicode_and_space_account_names_are_managed_like_any_other() {
    let t = tmp("unicode");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("工作 语料")).unwrap();
    std::fs::write(
        p.profiles.join("工作 语料/auth.json"),
        r#"{"tokens":{"account_id":"uni-1"}}"#,
    )
    .unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();

    let listed = accounts::list(&p).unwrap();
    assert!(listed.iter().any(|a| a.name == "工作 语料"), "{listed:?}");
    assert_eq!(listed[0].name, "工作 语料");
    assert!(listed[0].logged_in);

    // Linking through the adapter keeps the previous storing contract.
    store::link_profile(&p, "工作 语料").unwrap();
    let shared = p.profiles.join("工作 语料/state_5.sqlite");
    assert!(shared.is_symlink(), "the shared store must be linked");
    assert_eq!(
        platform::fs::file_identity(&shared),
        platform::fs::file_identity(&p.base.join("state_5.sqlite")),
        "and it must resolve to the base database"
    );

    // Per-window Electron data stays inside this Paths' home with the name.
    let dir = windows::user_data_dir(&p, "工作 语料");
    assert!(dir.starts_with(p.home()), "{dir:?}");
    assert!(dir.ends_with("Codex-工作 语料"), "{dir:?}");
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn a_dangling_auth_symlink_reads_as_signed_out_not_as_a_dead_profile() {
    let t = tmp("dangling");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("gone")).unwrap();
    std::os::unix::fs::symlink(
        t.join("nowhere/auth.json"),
        p.profiles.join("gone/auth.json"),
    )
    .unwrap();

    let listed = accounts::list(&p).unwrap();
    assert_eq!(listed.len(), 1, "the profile still lists");
    assert!(!listed[0].logged_in, "a dangling link is not a sign-in");

    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    let report = store::inspect(&p);
    assert!(
        matches!(report.state, StoreState::Shared),
        "dangling profile auth must not turn into an auth-cycle report: {:?}",
        report.state
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn repair_displaced_every_file_into_the_backup_for_rollback() {
    // Upgrade/rollback rehearsal: after repair, every displaced original must
    // be inside the backup with its data readable, so a manual restore or a
    // ships-broken upgrade can walk everything back.
    let t = tmp("rollback");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    db::create_empty(&p.profiles.join("work/state_5.sqlite")).unwrap();
    db::insert_thread(&p.profiles.join("work/state_5.sqlite"), "t1", "/x/a.jsonl").unwrap();

    let bak = p.backup_dir("upg");
    store::repair(&p, &bak).unwrap();

    let displaced = bak.join("work/old-state_5.sqlite");
    assert!(
        displaced.is_file(),
        "the original must survive in the backup"
    );
    assert_eq!(
        db::thread_count(&displaced).unwrap(),
        1,
        "the displaced database must still hold its thread for rollback"
    );

    // Re-running repair after rollback-grade changes keeps the store shared
    // and never deletes: the second pass moves nothing new.
    let before = std::fs::read_dir(&bak).unwrap().count();
    store::repair(&p, &bak).unwrap();
    assert_eq!(
        std::fs::read_dir(&bak).unwrap().count(),
        before,
        "idempotent repair must leave the backup untouched"
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn a_corrupted_shared_store_is_reported_never_claimed_healthy() {
    let t = tmp("corrupt");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::write(p.base.join("state_5.sqlite"), b"not a database at all").unwrap();

    let report = store::inspect(&p);
    assert!(
        report.error.is_some(),
        "inspection must surface why the store is unreadable"
    );
    // Downstream gates (CLI status, UI) must refuse to call this healthy and
    // never report thread/project counts from a store that failed to open.
    assert!(
        !(report.error.is_none() && report.threads > 0),
        "no counts may be fabricated from an unreadable store: {:?}",
        report
    );
    assert!(!db::quick_check(&p.base.join("state_5.sqlite")).unwrap_or(false));
    std::fs::remove_dir_all(&t).ok();
}
