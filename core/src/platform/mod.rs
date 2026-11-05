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
//! | shared-store links | yes (symlink) | yes (symlink) | no (needs Developer Mode; untested) |
//! | process table (`/proc`) | yes | no (no `/proc`) | no |
//! | window activation (KWin/wmctrl) | yes | no | no |
//!
//! Behaviours that are plain std and identical everywhere (temp directories,
//! file locks via `File::try_lock`, atomic writes, JSON settings) are *not*
//! advertised as capabilities; only the seams with real shadow of a doubt
//! differences are.

pub mod fs;
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
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    /// Symlink-style sharing of the Codex store between profiles.
    pub shared_store_links: Support,
    /// Enumerating running Codex Desktop windows.
    pub process_table: Support,
    /// Raising an existing window instead of spawning a duplicate.
    pub window_activation: Support,
}

/// The platform's measured capabilities. This is a cfg decision per OS, kept
/// conservative: anything untested is `No` until a native runner proves it.
pub fn capabilities() -> Capabilities {
    #[cfg(target_os = "linux")]
    {
        let links = Support::Yes;
        let process_table = Support::Yes;
        let window_activation = Support::Yes;
        Capabilities {
            shared_store_links: links,
            process_table,
            window_activation,
        }
    }
    #[cfg(target_os = "macos")]
    {
        Capabilities {
            shared_store_links: Support::Yes,
            process_table: Support::No(
                "process inspection reads /proc and that is Linux-only; macOS support is planned",
            ),
            window_activation: Support::No(
                "window activation targets KWin/wmctrl; macOS support is planned",
            ),
        }
    }
    #[cfg(target_os = "windows")]
    {
        Capabilities {
            shared_store_links: Support::No(
                "creating symlinks needs Developer Mode or an elevated process; profiles stay isolated",
            ),
            process_table: Support::No("process inspection is not implemented for Windows yet"),
            window_activation: Support::No("window activation is not implemented for Windows yet"),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Capabilities {
            shared_store_links: Support::No("this platform is untested"),
            process_table: Support::No("this platform is untested"),
            window_activation: Support::No("this platform is untested"),
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
            ("shared_store_links", caps.shared_store_links),
            ("process_table", caps.process_table),
            ("window_activation", caps.window_activation),
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
