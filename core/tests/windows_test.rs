use codex_dock_core::{windows, Paths};

#[test]
fn command_targets_profile_home_and_own_user_data_dir() {
    let p = Paths::new("/home/u/.codex", "/home/u/.codex-profiles");
    let l = windows::command_for(&p, "work");
    assert_eq!(l.bin, codex_dock_core::tool::desktop().to_string_lossy());
    assert_eq!(
        l.env,
        vec![(
            "CODEX_HOME".to_string(),
            p.profiles.join("work").display().to_string(),
        )]
    );
    let expected = format!("--user-data-dir={}", p.user_data_dir("work").display());
    assert!(l.args.join(" ").contains(&expected));
    #[cfg(target_os = "linux")]
    assert!(l
        .args
        .join(" ")
        .contains("--user-data-dir=/home/u/.config/Codex-work"));
}

#[test]
fn every_account_gets_a_distinct_user_data_dir() {
    let p = Paths::new("/h/.codex", "/h/.codex-profiles");
    let a = windows::command_for(&p, "work").args.join(" ");
    let b = windows::command_for(&p, "secondary").args.join(" ");
    assert_ne!(a, b);
}

#[test]
#[cfg(not(target_os = "linux"))]
fn procfs_window_enumeration_is_empty_when_unsupported() {
    let root = std::env::temp_dir().join(format!("cd-proc-unsupported-{}", std::process::id()));
    assert!(windows::running(&root).is_empty());
}

#[test]
#[cfg(target_os = "linux")]
fn running_maps_user_data_dir_back_to_account() {
    let root = std::env::temp_dir().join(format!("cd-proc-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let d = root.join("4242");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("cmdline"),
        "/usr/bin/chatgpt\0--user-data-dir=/home/u/.config/Codex-work\0",
    )
    .unwrap();
    let got = windows::running(&root);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].account, "work");
    assert_eq!(got[0].pid, 4242);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn processes_without_our_flag_are_ignored() {
    let root = std::env::temp_dir().join(format!("cd-proc2-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let d = root.join("7");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("cmdline"), "/usr/bin/other\0--foo\0").unwrap();
    assert!(windows::running(&root).is_empty());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn ignores_electron_helpers_and_unrelated_apps_and_keeps_space_in_paths() {
    let root = std::env::temp_dir().join(format!("cd-proc-filter-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (pid, cmd) in [
        (1, "/usr/lib/chatgpt/ChatGPT\0--user-data-dir=/home/a user/.config/Codex-work\0"),
        (2, "/usr/lib/chatgpt/ChatGPT\0--type=renderer\0--user-data-dir=/home/a user/.config/Codex-work\0"),
        (3, "/usr/bin/other\0--user-data-dir=/home/a user/.config/Codex-work\0"),
    ] {
        let dir = root.join(pid.to_string()); std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cmdline"), cmd).unwrap();
    }
    let got = windows::running(&root);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].pid, 1);
    assert_eq!(
        got[0].user_data_dir.to_str().unwrap(),
        "/home/a user/.config/Codex-work"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn legacy_default_process_is_reused_for_the_same_named_account() {
    let root = std::env::temp_dir().join(format!("cd-legacy-{}", std::process::id()));
    let p = Paths::new(root.join(".codex"), root.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    let auth = r#"{"tokens":{"account_id":"same"}}"#;
    std::fs::write(p.base.join("auth.json"), auth).unwrap();
    std::fs::write(p.profiles.join("work/auth.json"), auth).unwrap();
    let proc = root.join("proc/42");
    std::fs::create_dir_all(&proc).unwrap();
    std::fs::write(proc.join("cmdline"), "/usr/lib/chatgpt/ChatGPT\0").unwrap();
    let got = windows::running_for(&p, &root.join("proc"));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].account, "work");
    assert_eq!(got[0].pid, 42);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn matching_sign_in_identities_do_not_redirect_named_profile_windows() {
    let root = std::env::temp_dir().join(format!("cd-named-windows-{}", std::process::id()));
    let p = Paths::new(root.join(".codex"), root.join(".codex-profiles"));
    let auth = r#"{"tokens":{"account_id":"same"}}"#;
    for (pid, name) in [(42, "work"), (43, "third")] {
        std::fs::create_dir_all(p.profiles.join(name)).unwrap();
        std::fs::write(p.profiles.join(name).join("auth.json"), auth).unwrap();
        let proc = root.join("proc").join(pid.to_string());
        std::fs::create_dir_all(&proc).unwrap();
        std::fs::write(
            proc.join("cmdline"),
            format!(
                "/usr/lib/chatgpt/ChatGPT\0--user-data-dir={}\0",
                p.user_data_dir(name).display()
            ),
        )
        .unwrap();
        std::fs::write(
            proc.join("environ"),
            format!("CODEX_HOME={}\0", p.profiles.join(name).display()),
        )
        .unwrap();
    }
    let got = windows::scan_running_for(&p, &root.join("proc")).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!((got[0].pid, got[0].account.as_str()), (42, "work"));
    assert_eq!((got[1].pid, got[1].account.as_str()), (43, "third"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn a_process_exiting_mid_scan_is_transient_not_a_broken_process_table() {
    // Reading /proc/<pid>/cmdline for a pid that was reaped after read_dir
    // listed it fails with ESRCH ("No such process"), which std reports as
    // ErrorKind::Uncategorized. Treating that as a fatal scan error made
    // `running`, `launch`, `remove` and `repair` fail closed on ordinary
    // process churn. Spawn and reap children while scanning to provoke it.
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let stop = Arc::new(AtomicBool::new(false));
    let churn = Arc::clone(&stop);
    let churner = std::thread::spawn(move || {
        while !churn.load(Ordering::Relaxed) {
            let child = Command::new("true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if let Ok(mut c) = child {
                let _ = c.wait();
            }
        }
    });

    let mut failures = Vec::new();
    for _ in 0..300 {
        if let Err(e) = windows::scan_running(std::path::Path::new("/proc")) {
            failures.push(e.to_string());
        }
    }
    stop.store(true, Ordering::Relaxed);
    churner.join().unwrap();

    assert!(
        failures.is_empty(),
        "a dying process must not break the scan; {} of 300 failed, first: {}",
        failures.len(),
        failures[0]
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_missing_process_root_is_still_a_loud_failure() {
    // The transient-process tolerances above must not swallow a genuinely
    // unreadable process table: callers rely on the error to refuse.
    let missing = std::env::temp_dir().join(format!("cd-proc-missing-{}", std::process::id()));
    let err = windows::scan_running(&missing).unwrap_err().to_string();
    assert!(err.contains("does not exist"), "{err}");
}

#[test]
#[cfg(target_os = "linux")]
fn rewritten_electron_titles_still_detect_running_accounts_and_ignore_helpers() {
    let root = std::env::temp_dir().join(format!("cd-flat-proc-{}", std::process::id()));
    for (pid, title) in [
        (42, "/usr/lib/chatgpt/ChatGPT --ozone-platform=wayland --user-data-dir=/home/a user/.config/Codex-personal"),
        (43, "/usr/lib/chatgpt/ChatGPT --type=renderer --user-data-dir=/home/a user/.config/Codex-personal"),
    ] {
        let dir = root.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cmdline"), title).unwrap();
    }
    let got = windows::running(&root);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].pid, 42);
    assert_eq!(got[0].account, "personal");
    assert_eq!(
        got[0].user_data_dir.to_str().unwrap(),
        "/home/a user/.config/Codex-personal"
    );
    std::fs::remove_dir_all(root).unwrap();
}
