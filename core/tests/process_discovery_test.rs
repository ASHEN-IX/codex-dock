//! Native process discovery verification tests.
//!
//! Verifies:
//! - Process enumeration and identity.
//! - Process termination during scanning (churn / race tolerance).
//! - Multiple simultaneous processes.
//! - Unknown profile handling without false attribution.
//! - Accurate process-to-profile association when evidence is present.
//! - Electron helper filtering.
//! - Explicit outcomes: ManagedProfile, UnknownProfile, NoCodexProcess, Unsupported.
//! - Synthetic tests are explicitly marked and distinct from real desktop execution.

use codex_dock_core::accounts::Account;
use codex_dock_core::paths::Paths;
use codex_dock_core::platform::process::{
    self, attribute_process, extract_user_data_dir, is_candidate_codex_name, is_electron_helper,
    DiscoveredProcess, DiscoveryOutcome, ProfileAttribution,
};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn test_paths(tag: &str) -> (Paths, PathBuf) {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "cd-proc-test-{}-{}-{}",
        tag,
        std::process::id(),
        nonce
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let p = Paths::new(root.join(".codex"), root.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    (p, root)
}

#[test]
fn native_process_discovery_is_supported_on_major_platforms() {
    let supported = process::supported();
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        assert!(
            supported,
            "native process discovery must be supported on {}",
            std::env::consts::OS
        );
        let method = process::discovery_method();
        assert!(!method.is_empty());
        assert_ne!(method, "unsupported");
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(!supported);
    }
}

#[test]
fn native_process_scan_runs_cleanly_on_current_host() {
    let (p, cleanup) = test_paths("scan-clean");
    let report =
        process::scan_processes(&p).expect("scan_processes must succeed on supported platform");
    // Should return a valid report with non-negative count
    assert!(report.processes.iter().all(|proc| proc.pid > 0));
    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn synthetic_tests_are_explicitly_distinguished_from_real_desktop() {
    // Tests using synthetic helper processes must not be reported as verification
    // against the real Codex Desktop application.
    let real_desktop_opt_in = std::env::var("CODEX_DOCK_TEST_REAL_DESKTOP").is_ok();
    if !real_desktop_opt_in {
        let (p, cleanup) = test_paths("synth-distinction");
        let audit = codex_dock_core::audit::audit_platform(&p);
        assert!(
            audit
                .process_discovery_audit
                .synthetic_vs_real_note
                .contains("Synthetic tests verify"),
            "audit report must explicitly identify synthetic verification scope"
        );
        assert!(
            audit
                .process_discovery_audit
                .synthetic_vs_real_note
                .contains("real Codex Desktop launch and window control remain unverified"),
            "audit report must not conflate synthetic tests with real desktop verification"
        );
        std::fs::remove_dir_all(cleanup).ok();
    }
}

#[test]
fn candidate_codex_name_detection_matches_supported_executables() {
    assert!(is_candidate_codex_name("Codex"));
    assert!(is_candidate_codex_name("codex"));
    assert!(is_candidate_codex_name("Codex.exe"));
    assert!(is_candidate_codex_name("CODEX.EXE"));
    assert!(is_candidate_codex_name("ChatGPT"));
    assert!(is_candidate_codex_name("chatgpt"));
    assert!(is_candidate_codex_name("ChatGPT.exe"));
    assert!(is_candidate_codex_name("codex-desktop"));
    assert!(is_candidate_codex_name("codex-desktop.exe"));

    // Unrelated processes must NEVER match as candidate
    assert!(!is_candidate_codex_name("bash"));
    assert!(!is_candidate_codex_name("chrome"));
    assert!(!is_candidate_codex_name("firefox"));
    assert!(!is_candidate_codex_name("explorer.exe"));
    assert!(!is_candidate_codex_name("svchost.exe"));
    assert!(!is_candidate_codex_name("systemd"));
}

#[test]
fn electron_helper_detection_identifies_secondary_surfaces() {
    // Helper executables
    assert!(is_electron_helper("Codex Helper", &[]));
    assert!(is_electron_helper("ChatGPT Helper (Renderer)", &[]));
    assert!(is_electron_helper("codex-helper.exe", &[]));

    // Command-line flag detection
    let main_args = vec![
        "/usr/bin/chatgpt".into(),
        "--user-data-dir=/tmp/test".into(),
    ];
    assert!(!is_electron_helper("chatgpt", &main_args));

    let renderer_args = vec![
        "/usr/bin/chatgpt".into(),
        "--type=renderer".into(),
        "--user-data-dir=/tmp/test".into(),
    ];
    assert!(is_electron_helper("chatgpt", &renderer_args));

    let gpu_args = vec!["Codex.exe".into(), "--type=gpu-process".into()];
    assert!(is_electron_helper("Codex.exe", &gpu_args));

    let utility_args = vec!["Codex".into(), "--type=utility".into()];
    assert!(is_electron_helper("Codex", &utility_args));
}

#[test]
fn extract_user_data_dir_parses_various_flag_formats() {
    let args1 = vec![
        "app".to_string(),
        "--user-data-dir=/home/user/.config/Codex-work".to_string(),
    ];
    assert_eq!(
        extract_user_data_dir(&args1),
        Some(PathBuf::from("/home/user/.config/Codex-work"))
    );

    let args2 = vec![
        "app".to_string(),
        "--user-data-dir".to_string(),
        "/home/user/.config/Codex-personal".to_string(),
    ];
    assert_eq!(
        extract_user_data_dir(&args2),
        Some(PathBuf::from("/home/user/.config/Codex-personal"))
    );

    // Quoted paths (Windows style or spaces)
    let args3 = vec![
        "app.exe".to_string(),
        "--user-data-dir=\"C:\\Users\\User\\AppData\\Roaming\\Codex-special\"".to_string(),
    ];
    assert_eq!(
        extract_user_data_dir(&args3),
        Some(PathBuf::from(
            "C:\\Users\\User\\AppData\\Roaming\\Codex-special"
        ))
    );

    let args4 = vec!["app".to_string(), "--other-flag".to_string()];
    assert_eq!(extract_user_data_dir(&args4), None);
}

#[test]
fn profile_attribution_enforces_safety_and_rejects_false_attribution() {
    let (p, cleanup) = test_paths("attribution");
    let managed = vec![
        Account {
            name: "work".to_string(),
            path: p.profiles.join("work"),
            account_id: Some("id-work".to_string()),
            logged_in: true,
        },
        Account {
            name: "personal".to_string(),
            path: p.profiles.join("personal"),
            account_id: Some("id-personal".to_string()),
            logged_in: true,
        },
    ];

    // 1. Valid user-data-dir matches managed profile:
    let udd_work = p.user_data_dir("work");
    let attr1 = attribute_process(Some(&udd_work), None, &managed);
    assert_eq!(attr1, ProfileAttribution::Known("work".to_string()));

    // 2. Valid CODEX_HOME matches managed profile:
    let home_personal = p.profiles.join("personal");
    let attr2 = attribute_process(None, Some(&home_personal), &managed);
    assert_eq!(attr2, ProfileAttribution::Known("personal".to_string()));

    // 3. Unmanaged profile name must NOT be falsely attributed:
    let udd_unmanaged = PathBuf::from("/tmp/Codex-unmanaged");
    let attr3 = attribute_process(Some(&udd_unmanaged), None, &managed);
    assert_eq!(attr3, ProfileAttribution::Unknown);

    // 4. Missing arguments or generic user data dir must NOT be assigned to default or any account:
    let attr4 = attribute_process(None, None, &managed);
    assert_eq!(attr4, ProfileAttribution::Unknown);

    let udd_bare = PathBuf::from("/tmp/Codex");
    let attr5 = attribute_process(Some(&udd_bare), None, &managed);
    assert_eq!(attr5, ProfileAttribution::Unknown);

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn query_account_reports_explicit_discovery_outcomes() {
    let (p, cleanup) = test_paths("query-outcomes");

    // With no processes running for this test profile:
    let outcome = process::query_account(&p, "nonexistent-profile");
    let is_explicit = matches!(
        outcome,
        DiscoveryOutcome::NoCodexProcess | DiscoveryOutcome::UnknownProfile(_)
    );
    assert!(is_explicit, "outcome must be an explicit discovery variant");

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn process_churn_and_exit_races_do_not_fail_scan() {
    // Repeatedly scan while churning background processes to verify race handling:
    let stop = Arc::new(AtomicBool::new(false));
    let churn = Arc::clone(&stop);

    let churner = std::thread::spawn(move || {
        while !churn.load(Ordering::Relaxed) {
            #[cfg(unix)]
            let child = Command::new("true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            #[cfg(windows)]
            let child = Command::new("cmd")
                .args(["/C", "exit 0"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();

            if let Ok(mut c) = child {
                let _ = c.wait();
            }
        }
    });

    let (p, cleanup) = test_paths("churn");
    let mut failures = Vec::new();
    for _ in 0..100 {
        if let Err(e) = process::scan_processes(&p) {
            failures.push(e.to_string());
        }
    }
    stop.store(true, Ordering::Relaxed);
    churner.join().unwrap();

    assert!(
        failures.is_empty(),
        "process churn must not cause scan errors: {:?}",
        failures
    );

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn multiple_simultaneous_helper_processes_are_attributed_independently() {
    let (p, cleanup) = test_paths("multi-proc");
    let managed = vec![
        Account {
            name: "alpha".to_string(),
            path: p.profiles.join("alpha"),
            account_id: None,
            logged_in: true,
        },
        Account {
            name: "beta".to_string(),
            path: p.profiles.join("beta"),
            account_id: None,
            logged_in: true,
        },
    ];

    let udd_alpha = p.user_data_dir("alpha");
    let udd_beta = p.user_data_dir("beta");
    let udd_gamma = p.user_data_dir("gamma"); // unmanaged

    let p1 = DiscoveredProcess {
        pid: 1001,
        exe_path: Some(PathBuf::from("/bin/codex")),
        attribution: attribute_process(Some(&udd_alpha), None, &managed),
        user_data_dir: Some(udd_alpha),
    };
    let p2 = DiscoveredProcess {
        pid: 1002,
        exe_path: Some(PathBuf::from("/bin/codex")),
        attribution: attribute_process(Some(&udd_beta), None, &managed),
        user_data_dir: Some(udd_beta),
    };
    let p3 = DiscoveredProcess {
        pid: 1003,
        exe_path: Some(PathBuf::from("/bin/codex")),
        attribution: attribute_process(Some(&udd_gamma), None, &managed),
        user_data_dir: Some(udd_gamma),
    };

    assert_eq!(
        p1.attribution,
        ProfileAttribution::Known("alpha".to_string())
    );
    assert_eq!(
        p2.attribution,
        ProfileAttribution::Known("beta".to_string())
    );
    assert_eq!(p3.attribution, ProfileAttribution::Unknown);

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn windows_native_scan_conversion_preserves_structure() {
    let (p, cleanup) = test_paths("scan-native-conv");
    let native_windows = codex_dock_core::windows::running_native(&p);
    // Should be a valid vector without panics
    for win in &native_windows {
        assert!(win.pid > 0);
        assert!(!win.account.is_empty());
    }
    std::fs::remove_dir_all(cleanup).ok();
}
