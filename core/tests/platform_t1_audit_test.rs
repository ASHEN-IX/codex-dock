//! Phase T1: Windows & macOS Runtime Compatibility Integration Tests.
//!
//! Verifies:
//! 1. Platform capability truthful reporting (produced, not assumed).
//! 2. Cross-platform Desktop and CLI candidate discovery.
//! 3. SQLite3 probe and version extraction.
//! 4. Profile isolation contract using disposable synthetic accounts:
//!    - CODEX_HOME propagation
//!    - --user-data-dir support & separation
//!    - Profile persistence
//!    - Credential separation without file sharing
//!    - Independent runtime state & locks
//!    - Session rollout vs projection-store isolation
//! 5. Fail-closed refusal of unsupported operations on Windows/macOS.
//! 6. Desktop execution safely blocked on headless/unsupported environments.

use codex_dock_core::audit::{self, CheckStatus};
use codex_dock_core::paths::Paths;
use codex_dock_core::platform::{self, Support};
use codex_dock_core::tool;
use codex_dock_core::windows;

#[test]
fn test_platform_capabilities_truthfulness() {
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
        assert_eq!(caps.filesystem_symlinks, Support::Yes);
        assert!(matches!(
            caps.shared_store_compatibility,
            Support::No(reason) if reason.contains("not validated on macOS")
        ));
        assert!(matches!(
            caps.native_process_discovery,
            Support::No(reason) if reason.contains("/proc")
        ));
        assert!(matches!(caps.window_activation, Support::No(_)));
        assert!(matches!(caps.safe_desktop_launch, Support::No(_)));
    }

    #[cfg(target_os = "windows")]
    {
        assert!(matches!(
            caps.filesystem_symlinks,
            Support::No(reason) if reason.contains("Developer Mode")
        ));
        assert!(matches!(
            caps.shared_store_compatibility,
            Support::No(reason) if reason.contains("not validated on Windows")
        ));
        assert!(matches!(caps.native_process_discovery, Support::No(_)));
        assert!(matches!(caps.window_activation, Support::No(_)));
        assert!(matches!(caps.safe_desktop_launch, Support::No(_)));
    }
}

#[test]
fn test_desktop_candidates_cross_platform_coverage() {
    let candidates = tool::desktop_candidates(None);
    assert!(
        !candidates.is_empty(),
        "desktop_candidates must return platform-specific probe paths"
    );

    #[cfg(target_os = "macos")]
    {
        let has_app_bundle = candidates.iter().any(|p| {
            p.to_string_lossy()
                .contains("/Applications/Codex.app/Contents/MacOS/Codex")
                || p.to_string_lossy()
                    .contains("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT")
        });
        assert!(
            has_app_bundle,
            "macOS candidates must include standard .app bundle paths"
        );
    }

    #[cfg(target_os = "windows")]
    {
        let has_exe = candidates
            .iter()
            .any(|p| p.to_string_lossy().to_ascii_lowercase().ends_with(".exe"));
        assert!(has_exe, "Windows candidates must check .exe paths");
    }

    #[cfg(target_os = "linux")]
    {
        let has_usr_bin = candidates
            .iter()
            .any(|p| p.to_string_lossy() == "/usr/bin/chatgpt");
        assert!(has_usr_bin, "Linux candidates must check /usr/bin/chatgpt");
    }
}

#[test]
fn test_codex_cli_candidates_cross_platform_coverage() {
    let candidates = tool::codex_candidates(None);
    assert!(
        !candidates.is_empty(),
        "codex_candidates must return platform-specific probe paths"
    );

    #[cfg(target_os = "macos")]
    {
        let has_app_resources = candidates.iter().any(|p| {
            p.to_string_lossy().contains("Contents/Resources/codex")
                || p.to_string_lossy().contains("/opt/homebrew/bin/codex")
        });
        assert!(
            has_app_resources,
            "macOS candidates must check app bundle resources or homebrew"
        );
    }

    #[cfg(target_os = "windows")]
    {
        let has_cmd_or_exe = candidates.iter().any(|p| {
            let s = p.to_string_lossy().to_ascii_lowercase();
            s.ends_with(".exe") || s.ends_with(".cmd") || s.ends_with(".bat")
        });
        assert!(
            has_cmd_or_exe,
            "Windows candidates must check .exe, .cmd, or .bat"
        );
    }
}

#[test]
fn test_sqlite3_probe_and_availability() {
    let locs = tool::sqlite3_standard_locations();
    let _ = locs;

    let found = tool::find_sqlite3();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // On standard Ubuntu and macOS GitHub runners, sqlite3 is pre-installed.
        if let Some(sqlite) = found {
            assert!(sqlite.exists(), "discovered sqlite3 binary path must exist");
            let ver = tool::sqlite3_version(&sqlite);
            assert!(
                ver.is_some(),
                "discovered sqlite3 must return valid version string"
            );
        }
    }

    #[cfg(target_os = "windows")]
    {
        // On clean Windows, sqlite3.exe may not be present on PATH unless bundled or installed.
        // The probe must handle absence gracefully without panicking.
        if let Some(sqlite) = found {
            let ver = tool::sqlite3_version(&sqlite);
            assert!(ver.is_some());
        }
    }
}

#[test]
fn test_profile_isolation_six_invariants_synthetic() {
    let temp_root = std::env::temp_dir().join(format!("cd-iso-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();

    let base_dir = temp_root.join(".codex");
    let profiles_dir = temp_root.join(".codex-profiles");
    let p = Paths::new(&base_dir, &profiles_dir);

    // Invariant 1: CODEX_HOME propagation
    let cmd_work = windows::command_for(&p, "work");
    let cmd_personal = windows::command_for(&p, "personal");

    let work_home = cmd_work
        .env
        .iter()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| v.as_str());
    let personal_home = cmd_personal
        .env
        .iter()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| v.as_str());

    assert_eq!(
        work_home,
        Some(p.profiles.join("work").to_str().unwrap()),
        "CODEX_HOME must point directly to profile directory"
    );
    assert_eq!(
        personal_home,
        Some(p.profiles.join("personal").to_str().unwrap()),
        "CODEX_HOME must point directly to profile directory"
    );
    assert_ne!(
        work_home, personal_home,
        "profiles must receive distinct CODEX_HOME environments"
    );

    // Invariant 2: --user-data-dir support and isolation
    let work_user_data = cmd_work
        .args
        .iter()
        .find(|a| a.starts_with("--user-data-dir="))
        .unwrap();
    let personal_user_data = cmd_personal
        .args
        .iter()
        .find(|a| a.starts_with("--user-data-dir="))
        .unwrap();

    assert_ne!(
        work_user_data, personal_user_data,
        "different accounts must have isolated user-data directories"
    );
    assert!(work_user_data.contains("Codex-work"));
    assert!(personal_user_data.contains("Codex-personal"));

    // Check OS-specific path conventions
    #[cfg(target_os = "macos")]
    {
        assert!(work_user_data.contains("Library/Application Support"));
    }
    #[cfg(target_os = "windows")]
    {
        assert!(work_user_data.contains("AppData") && work_user_data.contains("Roaming"));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        assert!(work_user_data.contains(".config"));
    }

    // Invariant 3: Profile persistence
    let _r_work = codex_dock_core::accounts::add_account(&p, "work", None).unwrap();
    let _r_personal = codex_dock_core::accounts::add_account(&p, "personal", None).unwrap();

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        assert!(
            !_r_work.shared_store,
            "macOS and Windows must use isolation-only mode"
        );
        assert!(
            !_r_personal.shared_store,
            "macOS and Windows must use isolation-only mode"
        );
    }
    #[cfg(target_os = "linux")]
    {
        assert!(
            _r_work.shared_store,
            "Linux defaults to shared-store creation"
        );
    }

    let accts = codex_dock_core::accounts::list(&p).unwrap();
    assert!(accts.iter().any(|a| a.name == "work"));
    assert!(accts.iter().any(|a| a.name == "personal"));

    // Invariant 4: Credential separation without sharing authentication files
    let work_auth = p.profiles.join("work").join("auth.json");
    let personal_auth = p.profiles.join("personal").join("auth.json");

    std::fs::write(&work_auth, r#"{"tokens":{"account_id":"user_work_token"}}"#).unwrap();
    std::fs::write(
        &personal_auth,
        r#"{"tokens":{"account_id":"user_personal_token"}}"#,
    )
    .unwrap();

    let bak = p.backup_dir("audit-test");
    codex_dock_core::accounts::materialise_auth(&p.profiles.join("work"), &bak).unwrap();
    codex_dock_core::accounts::materialise_auth(&p.profiles.join("personal"), &bak).unwrap();

    // Verify auth.json files are independent regular files
    let meta_work = std::fs::symlink_metadata(&work_auth).unwrap();
    let meta_personal = std::fs::symlink_metadata(&personal_auth).unwrap();
    assert!(
        meta_work.is_file() && !meta_work.file_type().is_symlink(),
        "work auth.json must be a regular file"
    );
    assert!(
        meta_personal.is_file() && !meta_personal.file_type().is_symlink(),
        "personal auth.json must be a regular file"
    );

    // Overwrite work credentials and ensure personal credentials are untouched
    std::fs::write(
        &work_auth,
        r#"{"tokens":{"account_id":"user_work_UPDATED"}}"#,
    )
    .unwrap();
    let (personal_id, ok_p) = codex_dock_core::accounts::auth_info(&personal_auth);
    let (work_id, ok_w) = codex_dock_core::accounts::auth_info(&work_auth);

    assert!(ok_p && ok_w);
    assert_eq!(work_id, Some("user_work_UPDATED".into()));
    assert_eq!(
        personal_id,
        Some("user_personal_token".into()),
        "mutating work credentials must never affect personal credentials"
    );

    // Invariant 5: Independent runtime state and locks
    let lock_work = codex_dock_core::accounts::acquire_login_lock(&p, "work");
    assert!(lock_work.is_ok(), "should acquire work login lock");
    let lock_personal = codex_dock_core::accounts::acquire_login_lock(&p, "personal");
    assert!(
        lock_personal.is_ok(),
        "locking work must not prevent locking personal"
    );
    drop(lock_work);
    drop(lock_personal);

    // Invariant 6: Session rollout and projection-store isolation
    let work_sessions = p.profiles.join("work").join("sessions");
    std::fs::create_dir_all(&work_sessions).unwrap();
    let work_rollout = work_sessions.join("rollout-work-1.jsonl");
    std::fs::write(
        &work_rollout,
        "{\"type\":\"turn\",\"text\":\"secret work\"}\n",
    )
    .unwrap();

    let work_proj = p.profiles.join("work").join("thread_history_1.sqlite");
    std::fs::write(&work_proj, "PROJECTION_DATA").unwrap();

    let personal_sessions = p.profiles.join("personal").join("sessions");
    assert!(
        !personal_sessions.join("rollout-work-1.jsonl").exists(),
        "work session rollouts must not appear in personal sessions"
    );
    assert!(
        !p.profiles
            .join("personal")
            .join("thread_history_1.sqlite")
            .exists(),
        "work projection stores must not appear in personal profile"
    );

    std::fs::remove_dir_all(&temp_root).ok();
}

#[test]
fn test_storage_compatibility_safe_refusal_on_unsupported_platforms() {
    let temp_root = std::env::temp_dir().join(format!("cd-refusal-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();

    let p = Paths::new(temp_root.join(".codex"), temp_root.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();

    let caps = platform::capabilities();
    if let Support::No(reason) = caps.shared_store_compatibility {
        // Direct link_profile must refuse fail-closed
        let err = codex_dock_core::store::link_profile(&p, "unsupported-acct").unwrap_err();
        assert!(
            err.to_string().contains(reason),
            "link_profile must refuse with platform capability reason"
        );

        // Mutating repair must refuse fail-closed
        let bak = p.backup_dir("refuse-repair");
        let rep_err = codex_dock_core::store::repair(&p, &bak).unwrap_err();
        assert!(
            rep_err.to_string().contains("not supported")
                || rep_err.to_string().contains("disabled"),
            "repair must refuse on unsupported platforms"
        );
    }

    std::fs::remove_dir_all(&temp_root).ok();
}

#[test]
fn test_desktop_launch_safely_refused_on_unsupported_platforms() {
    let temp_root = std::env::temp_dir().join(format!("cd-launch-refuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();

    let p = Paths::new(temp_root.join(".codex"), temp_root.join(".codex-profiles"));
    std::fs::create_dir_all(p.profiles.join("test-account")).unwrap();

    let caps = platform::capabilities();
    if caps.safe_desktop_launch != Support::Yes || caps.native_process_discovery != Support::Yes {
        let res = windows::launch(&p, "test-account");
        assert!(
            res.is_err(),
            "windows::launch must fail-closed on platforms lacking required capabilities"
        );
        let err_str = res.unwrap_err().to_string();
        assert!(
            err_str.contains("desktop launch is not supported here"),
            "error must clearly communicate missing capability refusal: {err_str}"
        );
    }

    std::fs::remove_dir_all(&temp_root).ok();
}

#[test]
fn test_real_desktop_launch_explicitly_blocked_on_ci() {
    // Unless explicitly opted in on a physical machine with Codex Desktop installed,
    // real desktop execution tests must be marked as BLOCKED rather than assumed.
    let opt_in = std::env::var("CODEX_DOCK_TEST_REAL_DESKTOP").is_ok();
    if !opt_in {
        let status = CheckStatus::Blocked(
            "real desktop launch requires physical machine with installed Codex Desktop and display server",
        );
        assert!(!status.is_pass());
        assert!(status.label().contains("BLOCKED"));
    }
}

#[test]
fn test_audit_platform_generates_truthful_report() {
    let p = Paths::real();
    let audit_data = audit::audit_platform(&p);

    assert!(!audit_data.os.is_empty());
    assert!(!audit_data.arch.is_empty());
    assert!(audit_data.isolation_audit.overall.is_pass());

    let formatted = audit::format_audit_report(&audit_data);
    assert!(formatted.contains("Phase T1 Platform Runtime Compatibility Audit"));
    assert!(formatted.contains("CODEX_HOME propagation:          PASS"));
    assert!(formatted.contains("Credential separation:"));
    assert!(formatted.contains("PASS"));
}
