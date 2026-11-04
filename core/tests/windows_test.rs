use codex_dock_core::{windows, Paths};

#[test]
fn command_targets_profile_home_and_own_user_data_dir() {
    let p = Paths::new("/home/u/.codex", "/home/u/.codex-profiles");
    let l = windows::command_for(&p, "work");
    assert_eq!(l.bin, codex_dock_core::tool::desktop().to_string_lossy());
    assert_eq!(l.env, vec![("CODEX_HOME".to_string(), "/home/u/.codex-profiles/work".to_string())]);
    assert!(l.args.join(" ").contains("--user-data-dir=/home/u/.config/Codex-work"));
}

#[test]
fn every_account_gets_a_distinct_user_data_dir() {
    let p = Paths::new("/h/.codex", "/h/.codex-profiles");
    let a = windows::command_for(&p, "work").args.join(" ");
    let b = windows::command_for(&p, "secondary").args.join(" ");
    assert_ne!(a, b);
}

#[test]
fn running_maps_user_data_dir_back_to_account() {
    let root = std::env::temp_dir().join(format!("cd-proc-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let d = root.join("4242");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("cmdline"), "/usr/bin/chatgpt\0--user-data-dir=/home/u/.config/Codex-work\0").unwrap();
    let got = windows::running(&root);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].account, "work");
    assert_eq!(got[0].pid, 4242);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
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
    assert_eq!(got.len(), 1); assert_eq!(got[0].pid, 1);
    assert_eq!(got[0].user_data_dir.to_str().unwrap(), "/home/a user/.config/Codex-work");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_default_process_is_reused_for_the_same_named_account() {
    let root = std::env::temp_dir().join(format!("cd-legacy-{}", std::process::id()));
    let p = Paths::new(root.join(".codex"), root.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    let auth = r#"{"tokens":{"account_id":"same"}}"#;
    std::fs::write(p.base.join("auth.json"), auth).unwrap();
    std::fs::write(p.profiles.join("work/auth.json"), auth).unwrap();
    let proc = root.join("proc/42"); std::fs::create_dir_all(&proc).unwrap();
    std::fs::write(proc.join("cmdline"), "/usr/lib/chatgpt/ChatGPT\0").unwrap();
    let got = windows::running_for(&p, &root.join("proc"));
    assert_eq!(got.len(), 1); assert_eq!(got[0].account, "work"); assert_eq!(got[0].pid, 42);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
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
    assert_eq!(got[0].user_data_dir.to_str().unwrap(), "/home/a user/.config/Codex-personal");
    std::fs::remove_dir_all(root).unwrap();
}
