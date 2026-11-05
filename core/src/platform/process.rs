//! Process seams: enumerating running Codex Desktop windows and the procfs
//! root they are discovered through.
//!
//! `running()`/`running_for()` already accept an injected `proc_root`, which
//! keeps them testable; what was missing is an honest answer for platforms
//! without procfs. On Linux this module changes nothing: procfs is exactly
//! what was hardcoded before.

use std::path::PathBuf;

/// procfs root on Linux, `None` anywhere without one (macOS, Windows, BSD,
/// and every other untested platform). A `None` is a refusal: callers must not
/// read a fake path, because a missing directory currently reads as "no
/// running windows", which would be a lie.
pub fn proc_root() -> Option<PathBuf> {
    if cap_supported() {
        Some(PathBuf::from("/proc"))
    } else {
        None
    }
}

/// Whether the process table can be inspected on this platform. Genuinely
/// linux-only until a native implementation exists on another OS.
pub fn supported() -> bool {
    cap_supported()
}

fn cap_supported() -> bool {
    matches!(
        crate::platform::capabilities().process_table,
        crate::platform::Support::Yes
    )
}
