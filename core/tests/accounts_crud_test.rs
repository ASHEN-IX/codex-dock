use codex_dock_core::{accounts, Paths};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let t = std::env::temp_dir().join(format!("cd-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn create_rejects_duplicate_and_bad_names() {
    let t = tmp("create");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    accounts::create(&p, "work", None).unwrap();
    assert!(accounts::create(&p, "work", None).is_err());
    assert!(accounts::create(&p, "bad name!", None).is_err());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn create_clones_config_from_source() {
    let t = tmp("clone");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    accounts::create(&p, "base", None).unwrap();
    std::fs::write(t.join(".codex-profiles/base/config.toml"), "model = \"x\"\n").unwrap();
    accounts::create(&p, "copy", Some("base")).unwrap();
    let got = std::fs::read_to_string(t.join(".codex-profiles/copy/config.toml")).unwrap();
    assert!(got.contains("model"));
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn remove_moves_the_profile_never_deletes() {
    let t = tmp("rm");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    accounts::create(&p, "work", None).unwrap();
    let bak = p.backup_dir("stamp");
    let moved = accounts::remove(&p, "work", &bak).unwrap();
    assert!(moved.exists(), "profile must survive in the backup");
    assert!(!t.join(".codex-profiles/work").exists());
    std::fs::remove_dir_all(&t).ok();
}
