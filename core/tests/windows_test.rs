use codex_dock_core::{windows, Paths};

#[test]
fn command_targets_profile_home_and_own_user_data_dir() {
    let p = Paths::new("/home/u/.codex", "/home/u/.codex-profiles");
    let l = windows::command_for(&p, "work");
    assert_eq!(l.bin, "/usr/bin/chatgpt");
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
