//! Capability honesty checks that compile on every target OS. The runtime
//! content differs per platform, but the shape of the guarantee does not: a
//! capability that is absent must be reported as `No(reason)`, and callers
//! must refuse instead of pretending.

use codex_dock_core::platform::{self, Support};

#[test]
fn absent_capabilities_carry_a_reason_and_refuse_functions_state_so() {
    let caps = platform::capabilities();
    for (name, support) in [
        ("shared_store_links", caps.shared_store_links),
        ("process_table", caps.process_table),
        ("window_activation", caps.window_activation),
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
        assert_eq!(caps.shared_store_links, Support::Yes);
        assert_eq!(caps.process_table, Support::Yes);
        assert_eq!(caps.window_activation, Support::Yes);
    }
    #[cfg(target_os = "macos")]
    {
        assert_eq!(caps.shared_store_links, Support::Yes);
        assert!(matches!(caps.process_table, Support::No(_)));
        assert!(matches!(caps.window_activation, Support::No(_)));
    }
    #[cfg(target_os = "windows")]
    {
        // Phase 1 promises only isolation on Windows: everything below must
        // read as absent, never half-claimed.
        assert!(matches!(caps.shared_store_links, Support::No(_)));
        assert!(matches!(caps.process_table, Support::No(_)));
        assert!(matches!(caps.window_activation, Support::No(_)));
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(matches!(caps.shared_store_links, Support::No(_)));
        assert!(matches!(caps.process_table, Support::No(_)));
        assert!(matches!(caps.window_activation, Support::No(_)));
    }
}
