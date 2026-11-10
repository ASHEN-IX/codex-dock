use codex_dock_core::Paths;

#[test]
fn real_paths_point_at_codex_and_profiles() {
    let p = Paths::real();
    assert!(p.base.ends_with(".codex"));
    assert!(p.profiles.ends_with(".codex-profiles"));
}

#[test]
fn backup_dir_is_dated_under_one_shared_root() {
    let p = Paths::new("/tmp/x/.codex", "/tmp/x/.codex-profiles");
    let b = p.backup_dir("20261030-160949");
    assert_eq!(b, p.home().join("codex-dock-backups/20261030-160949"));
    assert_eq!(b.parent(), Some(p.backup_root().as_path()));
}

#[test]
fn backups_never_overwrite_each_other() {
    let p = Paths::new("/tmp/x/.codex", "/tmp/x/.codex-profiles");
    assert_ne!(p.backup_dir("a"), p.backup_dir("b"));
    assert_eq!(p.backup_dir("a").parent(), p.backup_dir("b").parent());
}
