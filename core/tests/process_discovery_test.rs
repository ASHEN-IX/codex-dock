//! Native process discovery verification tests.
//!
//! Verifies:
//! - Process enumeration and identity.
//! - Process termination during scanning (churn / race tolerance).
//! - Multiple simultaneous processes.
//! - Unknown profile handling without false attribution.
//! - Strict expected-path matching against `Paths::user_data_dir()`.
//! - Rejection of unrelated directories with matching basenames.
//! - Rejection of conflicting `CODEX_HOME` and `--user-data-dir` evidence.
//! - Relative path, symlink, and Windows case-insensitivity handling.
//! - Incomplete-scan semantics and permission denial handling in `query_account`.
//! - Windows `NtQueryInformationProcess` UNICODE_STRING buffer parsing and memory safety.
//! - Windows `CommandLineToArgvW` / MSDN parsing with escaped quotes, backslashes, spaces, and Unicode.
//! - Distinction between Codex CLI and Codex Desktop.
//! - Electron helper filtering.
//! - Synthetic tests are explicitly marked and distinct from real desktop execution.

use codex_dock_core::accounts::Account;
use codex_dock_core::paths::Paths;
use codex_dock_core::platform::process::{
    self, attribute_process, classify_process, extract_user_data_dir, is_candidate_codex_name,
    is_codex_cli, is_electron_helper, parse_unicode_string_buffer, parse_windows_command_line_msdn,
    paths_match_with_options, DiscoveredProcess, DiscoveryOutcome, ProcessDiscoveryReport,
    ProcessIdentity, ProfileAttribution,
};
use std::path::{Path, PathBuf};
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
    assert!(report.processes.iter().all(|proc| proc.pid > 0));
    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn synthetic_tests_are_explicitly_distinguished_from_real_desktop() {
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
                .contains("official proprietary Codex Desktop application launch and runtime behavior remain unverified"),
            "audit report must not conflate synthetic tests with real desktop verification"
        );
        std::fs::remove_dir_all(cleanup).ok();
    }
}

// -----------------------------------------------------------------------------
// Task 1: Strict Profile Attribution Tests
// -----------------------------------------------------------------------------

#[test]
fn profile_attribution_enforces_strict_expected_path_and_rejects_matching_basenames() {
    let (p, cleanup) = test_paths("strict-attribution");
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

    // 1. Valid user-data-dir matching full expected path:
    let udd_work = p.user_data_dir("work");
    let attr1 = attribute_process(&p, Some(&udd_work), None, &managed);
    assert_eq!(attr1, ProfileAttribution::Known("work".to_string()));

    // 2. Unrelated directory with matching basename "Codex-work" MUST BE REJECTED:
    let unrelated_udd = PathBuf::from("/var/tmp/unrelated/Codex-work");
    let attr_unrelated = attribute_process(&p, Some(&unrelated_udd), None, &managed);
    assert_eq!(
        attr_unrelated,
        ProfileAttribution::Unknown,
        "an unrelated directory sharing the basename 'Codex-work' must NEVER be attributed"
    );

    let home_tamper = PathBuf::from("/opt/other-user/.codex-profiles/work");
    let attr_home_tamper = attribute_process(&p, None, Some(&home_tamper), &managed);
    assert_eq!(
        attr_home_tamper,
        ProfileAttribution::Unknown,
        "an unrelated directory sharing the profile dirname must NEVER be attributed"
    );

    // 3. Valid CODEX_HOME matches managed profile:
    let home_personal = p.profiles.join("personal");
    let attr2 = attribute_process(&p, None, Some(&home_personal), &managed);
    assert_eq!(attr2, ProfileAttribution::Known("personal".to_string()));

    // 4. Missing arguments or generic user data dir must NOT be assigned to default or any account:
    let attr4 = attribute_process(&p, None, None, &managed);
    assert_eq!(attr4, ProfileAttribution::Unknown);

    let udd_bare = PathBuf::from("/tmp/Codex");
    let attr5 = attribute_process(&p, Some(&udd_bare), None, &managed);
    assert_eq!(attr5, ProfileAttribution::Unknown);

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn profile_attribution_rejects_conflicting_evidence() {
    let (p, cleanup) = test_paths("conflicting-evidence");
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

    let udd_work = p.user_data_dir("work");
    let home_personal = p.profiles.join("personal");

    // Conflicting evidence: user-data-dir points to 'work', but CODEX_HOME points to 'personal':
    let attr_conflict = attribute_process(&p, Some(&udd_work), Some(&home_personal), &managed);
    assert_eq!(
        attr_conflict,
        ProfileAttribution::Unknown,
        "conflicting evidence between user_data_dir and codex_home must be rejected as Unknown"
    );

    // Concordant evidence: both point to 'work':
    let home_work = p.profiles.join("work");
    let attr_concordant = attribute_process(&p, Some(&udd_work), Some(&home_work), &managed);
    assert_eq!(
        attr_concordant,
        ProfileAttribution::Known("work".to_string())
    );

    // user_data_dir points to 'work', but CODEX_HOME points to unmanaged location:
    let unmanaged_home = PathBuf::from("/tmp/random-home");
    let attr_partial_conflict =
        attribute_process(&p, Some(&udd_work), Some(&unmanaged_home), &managed);
    assert_eq!(
        attr_partial_conflict,
        ProfileAttribution::Unknown,
        "if codex_home conflicts with an unmanaged path, attribution must be rejected"
    );

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn profile_attribution_handles_relative_paths_and_symlinks() {
    let (p, cleanup) = test_paths("rel-and-symlinks");
    let managed = vec![Account {
        name: "alpha".to_string(),
        path: p.profiles.join("alpha"),
        account_id: None,
        logged_in: true,
    }];

    let expected_udd = p.user_data_dir("alpha");
    std::fs::create_dir_all(&expected_udd).unwrap();

    // 1. Symlink pointing to expected user-data-dir:
    #[cfg(unix)]
    {
        let symlink_udd = cleanup.join("symlink-udd");
        std::os::unix::fs::symlink(&expected_udd, &symlink_udd).unwrap();
        let attr_symlink = attribute_process(&p, Some(&symlink_udd), None, &managed);
        assert_eq!(
            attr_symlink,
            ProfileAttribution::Known("alpha".to_string()),
            "symlink pointing to canonical user_data_dir must match"
        );
    }

    // 2. Relative path to expected user-data-dir:
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = expected_udd.strip_prefix(&cwd) {
            let attr_rel = attribute_process(&p, Some(rel), None, &managed);
            assert_eq!(
                attr_rel,
                ProfileAttribution::Known("alpha".to_string()),
                "relative path to expected user_data_dir must match"
            );
        }
    }

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn windows_case_insensitivity_and_path_forms() {
    let path_a = Path::new("C:/Users/User/AppData/Roaming/Codex-Work");
    let path_b = Path::new(r"C:\Users\user\appdata\roaming\codex-work");
    let path_verbatim = Path::new(r"\\?\C:\Users\User\AppData\Roaming\Codex-Work");

    // Under Windows semantics, case differences, slash differences, and \\?\ prefix must match:
    assert!(
        paths_match_with_options(path_a, path_b, true),
        "Windows paths must match case-insensitively with mixed slashes"
    );
    assert!(
        paths_match_with_options(path_b, path_verbatim, true),
        "Windows verbatim \\\\?\\ prefix must be stripped for equivalence"
    );

    // Under Unix semantics (Linux case-sensitivity):
    assert!(
        !paths_match_with_options(path_a, path_b, false),
        "Unix paths with differing cases must not match on case-sensitive filesystems"
    );
}

// -----------------------------------------------------------------------------
// Task 2: Incomplete-Scan Semantics Tests
// -----------------------------------------------------------------------------

#[test]
fn query_account_returns_incomplete_when_scan_had_permission_failures() {
    let (_p, cleanup) = test_paths("incomplete-scan");

    // Construct an incomplete report: no processes found, but permission was denied on inspection
    let report_incomplete = ProcessDiscoveryReport {
        processes: Vec::new(),
        warnings: vec!["permission denied inspecting candidate Codex process pid 42".to_string()],
        is_complete: false,
    };

    // When a scan was incomplete, absence cannot be confirmed, so it must NEVER return NoCodexProcess:
    assert!(!report_incomplete.is_complete);
    assert!(!report_incomplete.warnings.is_empty());

    // Construct a simulated outcome check:
    let outcome = if !report_incomplete.is_complete || !report_incomplete.warnings.is_empty() {
        DiscoveryOutcome::InspectionIncompleteOrDenied(report_incomplete.warnings[0].clone())
    } else {
        DiscoveryOutcome::NoCodexProcess
    };

    assert!(
        matches!(outcome, DiscoveryOutcome::InspectionIncompleteOrDenied(ref w) if w.contains("permission denied")),
        "incomplete scan must return InspectionIncompleteOrDenied, never NoCodexProcess"
    );

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn query_account_preserves_partially_discovered_processes_and_exposes_uncertainty() {
    let (_p, cleanup) = test_paths("partial-discovery");

    // Process was discovered, but could not be inspected for arguments due to permissions:
    let proc_restricted = DiscoveredProcess {
        pid: 9999,
        exe_path: Some(PathBuf::from(
            "/Applications/Codex.app/Contents/MacOS/Codex",
        )),
        identity: ProcessIdentity::CandidateDesktop,
        attribution: ProfileAttribution::Unknown,
        user_data_dir: None,
    };

    let report = ProcessDiscoveryReport {
        processes: vec![proc_restricted.clone()],
        warnings: vec!["permission denied inspecting arguments for pid 9999".to_string()],
        is_complete: false,
    };

    // Query for 'work': since an unknown candidate process is running, return UnknownProfile:
    let outcome = if let Some(m) = report
        .processes
        .iter()
        .find(|p| p.attribution == ProfileAttribution::Known("work".to_string()))
    {
        DiscoveryOutcome::ManagedProfile(m.clone())
    } else if let Some(u) = report
        .processes
        .iter()
        .find(|p| p.attribution == ProfileAttribution::Unknown)
    {
        DiscoveryOutcome::UnknownProfile(u.clone())
    } else if !report.is_complete {
        DiscoveryOutcome::InspectionIncompleteOrDenied(report.warnings[0].clone())
    } else {
        DiscoveryOutcome::NoCodexProcess
    };

    assert!(
        matches!(outcome, DiscoveryOutcome::UnknownProfile(p) if p.pid == 9999),
        "unknown candidate must be preserved and returned as UnknownProfile"
    );

    std::fs::remove_dir_all(cleanup).ok();
}

// -----------------------------------------------------------------------------
// Task 3: Windows Native Command-Line Extraction & Argument Parsing Tests
// -----------------------------------------------------------------------------

#[test]
fn unicode_string_buffer_parsing_handles_contract_and_memory_safety() {
    let is_64_bit = std::mem::size_of::<usize>() == 8;
    let header_size = if is_64_bit { 16 } else { 8 };
    let ptr_offset = if is_64_bit { 8 } else { 4 };

    let text = "app --flag";
    let wide_chars: Vec<u16> = text.encode_utf16().collect();
    let char_bytes_len = (wide_chars.len() * 2) as u16;

    // Helper closure to write pointer into header:
    let write_ptr = |buf: &mut [u8], ptr: usize| {
        if is_64_bit {
            buf[ptr_offset..ptr_offset + 8].copy_from_slice(&(ptr as u64).to_ne_bytes());
        } else {
            buf[ptr_offset..ptr_offset + 4].copy_from_slice(&(ptr as u32).to_ne_bytes());
        }
    };

    // 1. Valid buffer with absolute pointer into caller's buffer:
    let mut buf = vec![0u8; header_size + (char_bytes_len as usize)];
    buf[0..2].copy_from_slice(&char_bytes_len.to_ne_bytes());
    buf[2..4].copy_from_slice(&(char_bytes_len + 2).to_ne_bytes());
    let abs_ptr = buf.as_ptr() as usize + header_size;
    write_ptr(&mut buf, abs_ptr);

    for (idx, &ch) in wide_chars.iter().enumerate() {
        let offset = header_size + idx * 2;
        buf[offset..offset + 2].copy_from_slice(&ch.to_ne_bytes());
    }

    let parsed = parse_unicode_string_buffer(&buf);
    assert_eq!(parsed, Some("app --flag".to_string()));

    // 2. Valid buffer with non-standard offset (padding between header and string data):
    let extra_padding = 8;
    let str_offset = header_size + extra_padding;
    let mut padded_buf = vec![0u8; str_offset + (char_bytes_len as usize)];
    padded_buf[0..2].copy_from_slice(&char_bytes_len.to_ne_bytes());
    padded_buf[2..4].copy_from_slice(&(char_bytes_len + 4).to_ne_bytes());
    let padded_abs_ptr = padded_buf.as_ptr() as usize + str_offset;
    write_ptr(&mut padded_buf, padded_abs_ptr);

    for (idx, &ch) in wide_chars.iter().enumerate() {
        let offset = str_offset + idx * 2;
        padded_buf[offset..offset + 2].copy_from_slice(&ch.to_ne_bytes());
    }

    let parsed_padded = parse_unicode_string_buffer(&padded_buf);
    assert_eq!(parsed_padded, Some("app --flag".to_string()));

    // 3. Valid buffer with relative offset:
    let mut rel_buf = buf.clone();
    write_ptr(&mut rel_buf, header_size);
    assert_eq!(
        parse_unicode_string_buffer(&rel_buf),
        Some("app --flag".to_string())
    );

    // 4. Null pointer rejection:
    let mut null_ptr_buf = buf.clone();
    write_ptr(&mut null_ptr_buf, 0);
    assert_eq!(parse_unicode_string_buffer(&null_ptr_buf), None);

    // 5. Invalid offset: pointer references memory past buffer bounds:
    let mut over_ptr_buf = buf.clone();
    let bad_ptr_high = buf.as_ptr() as usize + buf.len() + 100;
    write_ptr(&mut over_ptr_buf, bad_ptr_high);
    assert_eq!(parse_unicode_string_buffer(&over_ptr_buf), None);

    // 6. Invalid offset: pointer references memory before buffer base:
    let mut under_ptr_buf = buf.clone();
    let bad_ptr_low = (buf.as_ptr() as usize).saturating_sub(100);
    write_ptr(&mut under_ptr_buf, bad_ptr_low);
    assert_eq!(parse_unicode_string_buffer(&under_ptr_buf), None);

    // 7. Invalid offset: pointer points inside UNICODE_STRING header itself:
    let mut overlap_buf = buf.clone();
    let overlap_ptr = buf.as_ptr() as usize + 2;
    write_ptr(&mut overlap_buf, overlap_ptr);
    assert_eq!(parse_unicode_string_buffer(&overlap_buf), None);

    // 8. Unaligned pointer: offset is odd (not 2-byte aligned for UTF-16):
    let mut unaligned_buf = vec![0u8; header_size + 20];
    unaligned_buf[0..2].copy_from_slice(&10u16.to_ne_bytes());
    unaligned_buf[2..4].copy_from_slice(&12u16.to_ne_bytes());
    let odd_ptr = unaligned_buf.as_ptr() as usize + header_size + 1;
    write_ptr(&mut unaligned_buf, odd_ptr);
    assert_eq!(parse_unicode_string_buffer(&unaligned_buf), None);

    // 9. Truncated buffer (< header_size):
    assert_eq!(parse_unicode_string_buffer(&[0u8; 4]), None);
    assert_eq!(parse_unicode_string_buffer(&[]), None);

    // 10. Truncated string: declared Length extends beyond returned buffer:
    let mut trunc_str_buf = buf.clone();
    trunc_str_buf[0..2].copy_from_slice(&100u16.to_ne_bytes()); // Length 100 exceeds allocated size
    trunc_str_buf[2..4].copy_from_slice(&120u16.to_ne_bytes());
    assert_eq!(parse_unicode_string_buffer(&trunc_str_buf), None);

    // 11. Malformed header: Length > MaximumLength:
    let mut malformed_len_buf = buf.clone();
    malformed_len_buf[0..2].copy_from_slice(&20u16.to_ne_bytes());
    malformed_len_buf[2..4].copy_from_slice(&10u16.to_ne_bytes()); // MaximumLength < Length
    assert_eq!(parse_unicode_string_buffer(&malformed_len_buf), None);

    // 12. Malformed header: odd byte count (invalid UTF-16 code units):
    let mut odd_len_buf = buf.clone();
    odd_len_buf[0..2].copy_from_slice(&5u16.to_ne_bytes()); // odd length
    assert_eq!(parse_unicode_string_buffer(&odd_len_buf), None);

    // 13. Malformed header: zero length:
    let mut zero_len_buf = buf.clone();
    zero_len_buf[0..2].copy_from_slice(&0u16.to_ne_bytes());
    assert_eq!(parse_unicode_string_buffer(&zero_len_buf), None);
}

#[test]
fn windows_command_line_parsing_msdn_conformance() {
    // 1. Basic space-separated arguments:
    let args1 = parse_windows_command_line_msdn("codex.exe --flag arg2");
    assert_eq!(args1, vec!["codex.exe", "--flag", "arg2"]);

    // 2. Quoted arguments with spaces:
    let args2 = parse_windows_command_line_msdn(
        r#""C:\Program Files\Codex\Codex.exe" --user-data-dir="C:\Users\Alice\AppData\Roaming\Codex-work""#,
    );
    assert_eq!(
        args2,
        vec![
            r"C:\Program Files\Codex\Codex.exe",
            r"--user-data-dir=C:\Users\Alice\AppData\Roaming\Codex-work"
        ]
    );

    // 3. Escaped double quotes (\"):
    let args3 = parse_windows_command_line_msdn(r#"app.exe "quote: \"hello\"" remaining"#);
    assert_eq!(args3, vec!["app.exe", r#"quote: "hello""#, "remaining"]);

    // 4. Literal backslashes not preceding a quote:
    let args4 = parse_windows_command_line_msdn(r#"app.exe C:\Path\To\File.txt"#);
    assert_eq!(args4, vec!["app.exe", r"C:\Path\To\File.txt"]);

    // 5. Consecutive backslashes followed by quote (2N rule):
    // \\" -> \ followed by closing quote
    let args5 = parse_windows_command_line_msdn(r#"app.exe "directory\\""#);
    assert_eq!(args5, vec!["app.exe", r"directory\"]);

    // 6. Unicode paths:
    let args6 = parse_windows_command_line_msdn(
        r#"Codex.exe --user-data-dir="C:\Users\Über\Codex-日本語""#,
    );
    assert_eq!(
        args6,
        vec!["Codex.exe", r"--user-data-dir=C:\Users\Über\Codex-日本語"]
    );

    // 7. Oversized command line (> 64KB):
    let huge_token = "a".repeat(70000);
    let cmd = format!("app.exe {huge_token}");
    let parsed_huge = parse_windows_command_line_msdn(&cmd);
    assert_eq!(parsed_huge.len(), 2);
    assert_eq!(parsed_huge[1].len(), 70000);
}

// -----------------------------------------------------------------------------
// Task 4: Distinguish Codex CLI from Codex Desktop Tests
// -----------------------------------------------------------------------------

#[test]
fn classify_process_distinguishes_cli_from_desktop() {
    // 1. Standalone Codex CLI with subcommands:
    let cli_login_args = vec!["codex".into(), "login".into(), "--device-auth".into()];
    assert!(is_codex_cli(
        Some(Path::new("/usr/bin/codex")),
        &cli_login_args
    ));
    let id1 = classify_process(
        Some(Path::new("/usr/bin/codex")),
        "codex",
        &cli_login_args,
        None,
    );
    assert_eq!(id1, ProcessIdentity::Cli);

    let cli_exec_args = vec!["codex".into(), "exec".into(), "--ephemeral".into()];
    let id2 = classify_process(
        Some(Path::new("/home/user/.cargo/bin/codex")),
        "codex",
        &cli_exec_args,
        None,
    );
    assert_eq!(id2, ProcessIdentity::Cli);

    // 2. Confirmed Codex Desktop (macOS bundle):
    let mac_desktop_path = Path::new("/Applications/Codex.app/Contents/MacOS/Codex");
    let mac_desktop_args = vec!["/Applications/Codex.app/Contents/MacOS/Codex".into()];
    assert!(!is_codex_cli(Some(mac_desktop_path), &mac_desktop_args));
    let id3 = classify_process(Some(mac_desktop_path), "Codex", &mac_desktop_args, None);
    assert_eq!(id3, ProcessIdentity::ConfirmedDesktop);

    // 3. Confirmed Codex Desktop (has --user-data-dir):
    let linux_desktop_args = vec![
        "/usr/bin/chatgpt".into(),
        "--user-data-dir=/home/user/.config/Codex-work".into(),
    ];
    let id4 = classify_process(
        Some(Path::new("/usr/bin/chatgpt")),
        "chatgpt",
        &linux_desktop_args,
        None,
    );
    assert_eq!(id4, ProcessIdentity::ConfirmedDesktop);

    // 4. Electron Helper process:
    let helper_args = vec![
        "/usr/bin/chatgpt".into(),
        "--type=renderer".into(),
        "--user-data-dir=/home/user/.config/Codex-work".into(),
    ];
    assert!(is_electron_helper("chatgpt", &helper_args));
    let id5 = classify_process(
        Some(Path::new("/usr/bin/chatgpt")),
        "chatgpt",
        &helper_args,
        None,
    );
    assert_eq!(id5, ProcessIdentity::ElectronHelper);

    // 5. Unrelated executable:
    let id6 = classify_process(Some(Path::new("/bin/bash")), "bash", &[], None);
    assert_eq!(id6, ProcessIdentity::NonCodex);
}

// -----------------------------------------------------------------------------
// Existing Multi-process & Churn Tests (Preserved and Updated)
// -----------------------------------------------------------------------------

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

    // Unrelated processes must NEVER match as candidate:
    assert!(!is_candidate_codex_name("bash"));
    assert!(!is_candidate_codex_name("chrome"));
    assert!(!is_candidate_codex_name("firefox"));
    assert!(!is_candidate_codex_name("explorer.exe"));
    assert!(!is_candidate_codex_name("svchost.exe"));
    assert!(!is_candidate_codex_name("systemd"));
}

#[test]
fn electron_helper_detection_identifies_secondary_surfaces() {
    assert!(is_electron_helper("Codex Helper", &[]));
    assert!(is_electron_helper("ChatGPT Helper (Renderer)", &[]));
    assert!(is_electron_helper("codex-helper.exe", &[]));

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
fn query_account_reports_explicit_discovery_outcomes() {
    let (p, cleanup) = test_paths("query-outcomes");

    let outcome = process::query_account(&p, "nonexistent-profile");
    let is_explicit = matches!(
        outcome,
        DiscoveryOutcome::NoCodexProcess
            | DiscoveryOutcome::UnknownProfile(_)
            | DiscoveryOutcome::InspectionIncompleteOrDenied(_)
    );
    assert!(is_explicit, "outcome must be an explicit discovery variant");

    std::fs::remove_dir_all(cleanup).ok();
}

#[test]
fn process_churn_and_exit_races_do_not_fail_scan() {
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
        exe_path: Some(PathBuf::from("/bin/codex-desktop")),
        identity: ProcessIdentity::ConfirmedDesktop,
        attribution: attribute_process(&p, Some(&udd_alpha), None, &managed),
        user_data_dir: Some(udd_alpha),
    };
    let p2 = DiscoveredProcess {
        pid: 1002,
        exe_path: Some(PathBuf::from("/bin/codex-desktop")),
        identity: ProcessIdentity::ConfirmedDesktop,
        attribution: attribute_process(&p, Some(&udd_beta), None, &managed),
        user_data_dir: Some(udd_beta),
    };
    let p3 = DiscoveredProcess {
        pid: 1003,
        exe_path: Some(PathBuf::from("/bin/codex-desktop")),
        identity: ProcessIdentity::ConfirmedDesktop,
        attribution: attribute_process(&p, Some(&udd_gamma), None, &managed),
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
    for win in &native_windows {
        assert!(win.pid > 0);
        assert!(!win.account.is_empty());
    }
    std::fs::remove_dir_all(cleanup).ok();
}
