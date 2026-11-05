//! File-system seams: home/config roots, executable checks, link creation,
//! private-file creation and file identity.
//!
//! Each cfg branch keeps the previous Linux implementation verbatim; other
//! platforms get the smallest honest equivalent, or a refusal.

use crate::error::Result;
use std::path::Path;

/// The user's home directory. `$HOME` still wins on Unix, so an injected test
/// environment keeps behaving exactly as before; the fallback graph exists for
/// GUI processes and non-Unix platforms, where `HOME` is not guaranteed.
pub fn home_dir() -> std::path::PathBuf {
    #[cfg(unix)]
    {
        if let Some(home) = std::env::var_os("HOME").filter(|s| !s.is_empty()) {
            return std::path::PathBuf::from(home);
        }
    }
    #[cfg(windows)]
    {
        if let Some(home) = std::env::var_os("USERPROFILE").filter(|s| !s.is_empty()) {
            return std::path::PathBuf::from(home);
        }
        if let (Some(drive), Some(path)) = (
            std::env::var_os("HOMEDRIVE").filter(|s| !s.is_empty()),
            std::env::var_os("HOMEPATH").filter(|s| !s.is_empty()),
        ) {
            let mut p = std::path::PathBuf::from(drive);
            p.push(path);
            return p;
        }
    }
    #[allow(deprecated)]
    std::env::home_dir().expect("HOME not set")
}

/// Root for per-user application data, where Electron looks by default.
///
/// Parameterised on the home the paths live under, so injected test paths keep
/// their `.config` inside the fixture instead of migrating to a real `$HOME`.
pub fn config_dir() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA").filter(|s| !s.is_empty()) {
            return std::path::PathBuf::from(appdata);
        }
    }
    config_dir_at(&home_dir())
}

/// The same root, anchored at an explicit home.
pub fn config_dir_at(home: &std::path::Path) -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library").join("Application Support")
    }
    #[cfg(target_os = "windows")]
    {
        if home == home_dir() {
            if let Some(appdata) = std::env::var_os("APPDATA").filter(|s| !s.is_empty()) {
                return std::path::PathBuf::from(appdata);
            }
        }
        home.join("AppData").join("Roaming")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".config")
    }
}

/// Whether a path is an existing executable. On Unix this is the metadata
/// execute-bit check we already ship; file modes do not exist on Windows, so
/// a regular file with an executable-style extension is the best available
/// signal and `CODEX_DESKTOP_BIN` remains the override of last resort.
pub fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("exe") | Some("bat") | Some("cmd") | Some("lnk")
        ) && std::fs::metadata(path)
            .map(|m| m.is_file())
            .unwrap_or(false)
    }
}

/// Create `link` as a symlink pointing at `target`.
///
/// Never used directly for the shared store: go through
/// [`links_support`] first, because a platform can return `No`.
pub fn create_link(target: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link);
        Err(crate::error::Error::Msg(
            "symlink creation is not supported on this platform".into(),
        ))
    }
}

/// Capability check for OS-level symbolic links.
pub fn links_support() -> crate::platform::Support {
    crate::platform::capabilities().filesystem_symlinks
}

/// Capability check for validated multi-profile shared store operations.
pub fn shared_store_support() -> crate::platform::Support {
    crate::platform::capabilities().shared_store_compatibility
}

/// Refuse politely when symlink creation is not supported on this platform.
pub fn require_links(feature: &str) -> Result<()> {
    crate::platform::require(links_support(), feature)
}

/// Refuse politely when shared store operations are not validated on this platform.
pub fn require_shared_store(feature: &str) -> Result<()> {
    crate::platform::require(shared_store_support(), feature)
}

/// A stable per-file identity for proving two paths are the same file.
/// Unix exposes the inode; platforms without a usable identity report `None`
/// and *every* caller must treat that as "cannot verify", not "files differ".
pub fn file_identity(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| m.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Create-or-truncate a file readable only by the current user. On Unix the
/// mode is enforced at creation, too late to matter otherwise.
pub fn create_private(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

/// Create a brand-new file, failing if it exists, private to the current user.
/// Used by the atomic-rename writer in `store::merge_global_state`.
pub fn create_new_private(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

/// Open an existing-or-new file without truncating, for lock files.
pub fn open_private(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

/// A filesystem-safe `YYYYMMDD-HHMMSS` stamp in UTC, for the rare platform
/// where the `date` utility is not available. Backup stamps disambiguate
/// directories only; callers already fall back to `"backup"` on failure.
pub fn utc_stamp() -> Option<String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Inverse civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    Some(format!("{year:04}{month:02}{d:02}-{h:02}{m:02}{s:02}"))
}

/// Create a directory private to the current user.
pub fn create_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::platform::{require, Support};

    #[test]
    fn require_reports_the_reason_for_an_absent_feature() {
        let err = require(Support::No("needs Developer Mode"), "shared-store linking").unwrap_err();
        assert!(
            err.to_string().contains("Developer Mode"),
            "the reason must reach the user: {err}"
        );
        assert!(require(Support::Yes, "anything").is_ok());
    }
}
