use codex_dock_core::Paths;

#[test]
fn real_paths_point_at_codex_and_profiles() {
    let p = Paths::real();
    assert!(p.base.ends_with(".codex"));
    assert!(p.profiles.ends_with(".codex-profiles"));
}

#[test]
fn backup_dir_is_dated_under_home() {
    let p = Paths::new("/tmp/x/.codex", "/tmp/x/.codex-profiles");
    let b = p.backup_dir("20261030-160949");
    assert!(b.ends_with("codex-dock-backup-20261030-160949"));
    assert_eq!(b.to_string_lossy(), "/tmp/x/codex-dock-backup-20261030-160949");
}
