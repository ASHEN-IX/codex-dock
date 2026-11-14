//! Runs with default features in both debug and release builds.
#[test]
fn ordinary_options_enforce_production_capabilities() {
    use codex_dock_core::{
        paths::Paths,
        platform::{
            self,
            launch::{launch_desktop_with_options, LaunchOptions},
        },
    };
    let root = std::env::temp_dir().join(format!("cd-production-gate-{}", std::process::id()));
    let p = Paths::new(root.join("base"), root.join("profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    std::env::set_var("CODEX_DOCK_SYNTHETIC_TEST", "1");
    let mut options = LaunchOptions::default();
    options.binary_override = Some(root.join("missing-desktop"));
    options.require_window_activation = false;
    options.startup_timeout_ms = 0;
    assert!(!options.is_synthetic_test());
    if matches!(
        platform::capabilities().safe_desktop_launch,
        platform::Support::No(_)
    ) {
        for result in [
            launch_desktop_with_options(&p, "work", &options),
            platform::launch::launch_desktop(&p, "work"),
            codex_dock_core::windows::launch(&p, "work"),
        ] {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("desktop launch is not supported here"));
        }
    }
    std::env::remove_var("CODEX_DOCK_SYNTHETIC_TEST");
    std::fs::remove_dir_all(root).unwrap();
}
