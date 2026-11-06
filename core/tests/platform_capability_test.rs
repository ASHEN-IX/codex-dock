//! Capability honesty checks that compile on every target OS. The runtime
//! content differs per platform, but the shape of the guarantee does not: a
//! capability that is absent must be reported as `No(reason)`, and callers
//! must refuse instead of pretending.

use codex_dock_core::platform::{self, Support};

#[test]
fn absent_capabilities_carry_a_reason_and_refuse_functions_state_so() {
    let caps = platform::capabilities();
    for (name, support) in [
        ("filesystem_symlinks", caps.filesystem_symlinks),
        (
            "shared_store_compatibility",
            caps.shared_store_compatibility,
        ),
        ("native_process_discovery", caps.native_process_discovery),
        ("window_activation", caps.window_activation),
        ("safe_desktop_launch", caps.safe_desktop_launch),
    ] {
        match support {
            Support::Yes => {
                assert!(platform::require(support, name).is_ok());
            }
            Support::No(reason) => {
                let err = platform::require(support, name).unwrap_err();
                assert!(
                    err.to_string().contains(reason),
                    "the refusal must quote the reason: {err}"
                );
            }
        }
    }
}

#[test]
fn platform_expectations_are_pinned_explicitly() {
    let caps = platform::capabilities();
    #[cfg(target_os = "linux")]
    {
        assert_eq!(caps.filesystem_symlinks, Support::Yes);
        assert_eq!(caps.shared_store_compatibility, Support::Yes);
        assert_eq!(caps.native_process_discovery, Support::Yes);
        assert_eq!(caps.window_activation, Support::Yes);
        assert_eq!(caps.safe_desktop_launch, Support::Yes);
    }
    #[cfg(target_os = "macos")]
    {
        // Filesystem symlinks exist on macOS, but multi-profile database sharing
        // is NOT runtime validated, so shared store is explicitly reported No.
        assert_eq!(caps.filesystem_symlinks, Support::Yes);
        assert!(matches!(caps.shared_store_compatibility, Support::No(_)));
        assert!(matches!(caps.native_process_discovery, Support::No(_)));
        assert!(matches!(caps.window_activation, Support::No(_)));
        assert!(matches!(caps.safe_desktop_launch, Support::No(_)));
    }
    #[cfg(target_os = "windows")]
    {
        // Phase 1 promises only isolation on Windows: everything below must
        // read as absent, never half-claimed.
        assert!(matches!(caps.filesystem_symlinks, Support::No(_)));
        assert!(matches!(caps.shared_store_compatibility, Support::No(_)));
        assert!(matches!(caps.native_process_discovery, Support::No(_)));
        assert!(matches!(caps.window_activation, Support::No(_)));
        assert!(matches!(caps.safe_desktop_launch, Support::No(_)));
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(matches!(caps.filesystem_symlinks, Support::No(_)));
        assert!(matches!(caps.shared_store_compatibility, Support::No(_)));
        assert!(matches!(caps.native_process_discovery, Support::No(_)));
        assert!(matches!(caps.window_activation, Support::No(_)));
        assert!(matches!(caps.safe_desktop_launch, Support::No(_)));
    }
}

#[test]
#[cfg(target_os = "windows")]
fn windows_refuses_linking_and_repair_before_mutation() {
    let t = std::env::temp_dir().join(format!("cd-win-refuse-{}", std::process::id()));
    let p = codex_dock_core::Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    assert!(codex_dock_core::store::link_profile(&p, "work").is_err());
    assert!(codex_dock_core::store::repair(&p, &t.join("bak")).is_err());
}

#[test]
#[cfg(target_os = "macos")]
fn macos_refuses_linking_and_repair_before_mutation() {
    let t = std::env::temp_dir().join(format!("cd-mac-refuse-{}", std::process::id()));
    let p = codex_dock_core::Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    assert!(codex_dock_core::store::link_profile(&p, "work").is_err());
    assert!(codex_dock_core::store::repair(&p, &t.join("bak")).is_err());
}
