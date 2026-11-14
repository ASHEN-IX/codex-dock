//! Platform seams for everything the OS decides for us.
//!
//! Phase 1 rule: a feature the code cannot implement for a platform is
//! *reported as unsupported*, never silently faked. Compilation passing on a
//! platform is not evidence that a feature works there, so every support
//! decision lives behind [`capabilities`] and callers must refuse when a
//! capability is absent.
//!
//! Current support matrix (verified, not assumed):
//!
//! | Capability | Linux | macOS | Windows |
//! |---|---|---|---|
//! | filesystem symlinks | yes | yes | no (needs Developer Mode) |
//! | shared-store compatibility | yes | no (unvalidated; isolated) | no (unvalidated; isolated) |
//! | native process discovery | yes (/proc) | yes (libproc) | yes (Toolhelp32) |
//! | window activation | yes (KWin/wmctrl) | no | no |
//! | safe desktop launch | yes | no | no |
//!
//! Behaviours that are plain std and identical everywhere (temp directories,
//! file locks via `File::try_lock`, atomic writes, JSON settings) are *not*
//! advertised as capabilities; only the seams with real shadow of a doubt
//! differences are.

pub mod activation;
pub mod fs;
pub mod launch;
pub mod process;

use crate::error::{Error, Result};

/// Whether the current platform implements a feature.
///
/// `No` carries the reason so a refusal can explain itself instead of leaving
/// the user with a UI that silently claims success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Yes,
    No(&'static str),
}

/// What the running platform can do today. Produced, not assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Filesystem symlink availability: whether the OS and current privilege level
    /// support creating filesystem symbolic links.
    pub filesystem_symlinks: Support,
    /// Validated shared-store compatibility: whether sharing the Codex SQLite database
    /// and session rollouts across concurrent profile windows has been runtime-validated.
    pub shared_store_compatibility: Support,
    /// Native process discovery: whether running Codex Desktop processes/windows can be
    /// discovered via OS process tables or system APIs.
    pub native_process_discovery: Support,
    /// Window activation: whether an existing window can be raised to the front
    /// (e.g. via KWin D-Bus or wmctrl).
    pub window_activation: Support,
    /// Safe desktop launching: whether Codex Desktop can be safely launched with
    /// per-profile user data arguments and monitored on this platform.
    pub safe_desktop_launch: Support,
}

impl Capabilities {
    /// Backward-compatible alias for shared-store link operations.
    pub fn shared_store_links(&self) -> Support {
        self.shared_store_compatibility
    }

    /// Backward-compatible alias for process table discovery.
    pub fn process_table(&self) -> Support {
        self.native_process_discovery
    }
}

/// The platform's measured capabilities. This is a cfg decision per OS, kept
/// conservative: anything untested is `No` until a native runner proves it.
pub fn capabilities() -> Capabilities {
    #[cfg(target_os = "linux")]
    {
        Capabilities {
            filesystem_symlinks: Support::Yes,
            shared_store_compatibility: Support::Yes,
            native_process_discovery: Support::Yes,
            window_activation: Support::Yes,
            safe_desktop_launch: Support::Yes,
        }
    }
    #[cfg(target_os = "macos")]
    {
        Capabilities {
            filesystem_symlinks: Support::Yes,
            shared_store_compatibility: Support::No(
                "shared-store runtime compatibility is not validated on macOS yet; profiles stay isolated",
            ),
            native_process_discovery: Support::Yes,
            window_activation: Support::No(
                "native macOS window activation requires official Desktop runtime validation; production activation remains disabled",
            ),
            safe_desktop_launch: Support::No(
                "desktop launching requires verified official macOS application bundle execution; runtime verification pending",
            ),
        }
    }
    #[cfg(target_os = "windows")]
    {
        Capabilities {
            filesystem_symlinks: Support::No(
                "creating symlinks needs Developer Mode or an elevated process",
            ),
            shared_store_compatibility: Support::No(
                "shared-store runtime compatibility is not validated on Windows yet; profiles stay isolated",
            ),
            native_process_discovery: Support::Yes,
            window_activation: Support::No(
                "native Windows window activation requires official Desktop runtime validation; production activation remains disabled",
            ),
            safe_desktop_launch: Support::No(
                "desktop launching requires verified official Windows process execution; runtime verification pending",
            ),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Capabilities {
            filesystem_symlinks: Support::No("this platform is untested"),
            shared_store_compatibility: Support::No("this platform is untested"),
            native_process_discovery: Support::No("this platform is untested"),
            window_activation: Support::No("this platform is untested"),
            safe_desktop_launch: Support::No("this platform is untested"),
        }
    }
}

/// Constructive form of [`capabilities`]: refuse a feature this platform does
/// not have, with a message the UI and the CLI can show verbatim.
pub fn require(support: Support, feature: &str) -> Result<()> {
    match support {
        Support::Yes => Ok(()),
        Support::No(reason) => Err(Error::Msg(format!(
            "{feature} is not supported here: {reason}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_capability_carries_a_reason_when_absent() {
        let caps = capabilities();
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
            if let Support::No(reason) = support {
                assert!(
                    !reason.trim().is_empty(),
                    "{name} must explain its absence instead of failing silently"
                );
            }
        }
    }
}
