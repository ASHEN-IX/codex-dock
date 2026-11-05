//! Names coming from the outside (UI input, CLI arguments) must never be able
//! to step outside the profile or config directories. These checks hold on
//! every platform, so they are deliberately not gated on a target OS.

use codex_dock_core::{accounts, windows, Paths};

fn tmp(tag: &str) -> std::path::PathBuf {
    let t = std::env::temp_dir().join(format!("cd-traverse-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn create_never_materialises_a_traversal_name() {
    let t = tmp("create");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();

    for bad in ["../escape", "..", ".", "a/../b", "sub/dir"] {
        assert!(
            accounts::create(&p, bad, None).is_err(),
            "'{bad}' must be refused"
        );
    }
    let after = std::fs::read_dir(&p.profiles).unwrap().count();
    assert_eq!(after, 0, "no directory may be created inside profiles");
    assert!(
        !t.join("escape").exists(),
        "no directory may be created outside profiles either"
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn remove_and_window_data_removal_refuse_traversal_names() {
    let t = tmp("remove");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();
    let bak = t.join("bak");

    assert!(accounts::remove(&p, "../another", &bak).is_err());
    assert!(accounts::remove(&p, "a\\b", &bak).is_err());
    assert!(accounts::remove_user_data_dir(&p, "../another", &bak).is_err());
    assert!(!bak.exists(), "a refused name must move nothing anywhere");
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn launching_a_traversal_name_cannot_place_a_lock_file_outside() {
    let t = tmp("launch");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.profiles).unwrap();
    let outside = t.join("escape");
    std::fs::create_dir_all(&outside).unwrap();
    // Even though no account literally named like this can exist, the launch
    // path reaches the profile join before any spawn; the guard has to catch
    // the crafted input without creating lock files anywhere else.
    assert!(windows::launch(&p, "../escape").is_err());
    assert!(
        std::fs::read_dir(&outside).unwrap().count() == 0,
        "no lock file or directory may appear outside the profile area"
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn the_user_data_dir_for_a_valid_name_stays_inside_the_config_root() {
    let t = tmp("contain");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    let dir = windows::user_data_dir(&p, "工作 语料");
    assert!(
        dir.starts_with(p.home()),
        "user data must stay under this Paths' home, got {}",
        dir.display()
    );
    std::fs::remove_dir_all(&t).ok();
}
