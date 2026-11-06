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
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

/// An RAII guard managing the lifetime of an advisory file lock.
///
/// Synchronization protocol:
/// - Lock files are persistent on disk and opened private (mode 0600 on Unix).
/// - Locks are acquired non-blockingly using `File::try_lock()`.
/// - Dropping the guard releases the kernel lock via `File::unlock()`.
/// - Lock files are NEVER unlinked or removed during normal release, preventing
///   inode-replacement races between independent processes.
#[derive(Debug)]
pub struct LockGuard {
    file: std::fs::File,
    path: std::path::PathBuf,
    label: String,
}

impl LockGuard {
    pub fn try_acquire(path: &Path, label: &str) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = open_private(path).map_err(|e| {
            crate::error::Error::Msg(format!(
                "cannot open {label} lock file '{}': {e}",
                path.display()
            ))
        })?;
        file.try_lock()
            .map_err(|_| crate::error::Error::Msg(format!("{label} is currently in progress")))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            label: label.to_string(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file(&self) -> &std::fs::File {
        &self.file
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        // NEVER remove the lock file: it must remain persistent on disk
        // to prevent inode-replacement races.
    }
}

/// Atomically publish/rename `from` to `to`, guaranteeing that `to` is not replaced
/// if it already exists (even if `to` is an empty directory).
pub fn atomic_rename_no_replace(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_from = CString::new(from.as_os_str().as_bytes()).map_err(|e| {
            crate::error::Error::Msg(format!("invalid path '{}': {e}", from.display()))
        })?;
        let c_to = CString::new(to.as_os_str().as_bytes()).map_err(|e| {
            crate::error::Error::Msg(format!("invalid path '{}': {e}", to.display()))
        })?;

        const AT_FDCWD: std::ffi::c_int = -100;
        const RENAME_NOREPLACE: std::ffi::c_uint = 1;

        extern "C" {
            fn renameat2(
                olddirfd: std::ffi::c_int,
                oldpath: *const std::ffi::c_char,
                newdirfd: std::ffi::c_int,
                newpath: *const std::ffi::c_char,
                flags: std::ffi::c_uint,
            ) -> std::ffi::c_int;
        }

        let ret = unsafe {
            renameat2(
                AT_FDCWD,
                c_from.as_ptr(),
                AT_FDCWD,
                c_to.as_ptr(),
                RENAME_NOREPLACE,
            )
        };

        if ret == 0 {
            return Ok(());
        }

        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(17) || err.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(crate::error::Error::Msg(format!(
                "destination '{}' already exists",
                to.display()
            )));
        }
        Err(crate::error::Error::Io(err))
    }

    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_from = CString::new(from.as_os_str().as_bytes()).map_err(|e| {
            crate::error::Error::Msg(format!("invalid path '{}': {e}", from.display()))
        })?;
        let c_to = CString::new(to.as_os_str().as_bytes()).map_err(|e| {
            crate::error::Error::Msg(format!("invalid path '{}': {e}", to.display()))
        })?;

        const AT_FDCWD: std::ffi::c_int = -2;
        const RENAME_EXCL: std::ffi::c_uint = 0x00000004;

        extern "C" {
            fn renameatx_np(
                olddirfd: std::ffi::c_int,
                oldpath: *const std::ffi::c_char,
                newdirfd: std::ffi::c_int,
                newpath: *const std::ffi::c_char,
                flags: std::ffi::c_uint,
            ) -> std::ffi::c_int;
        }

        let ret = unsafe {
            renameatx_np(
                AT_FDCWD,
                c_from.as_ptr(),
                AT_FDCWD,
                c_to.as_ptr(),
                RENAME_EXCL,
            )
        };

        if ret == 0 {
            return Ok(());
        }

        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(17) || err.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(crate::error::Error::Msg(format!(
                "destination '{}' already exists",
                to.display()
            )));
        }
        Err(crate::error::Error::Io(err))
    }

    #[cfg(target_os = "windows")]
    {
        if to.exists() || std::fs::symlink_metadata(to).is_ok() {
            return Err(crate::error::Error::Msg(format!(
                "destination '{}' already exists",
                to.display()
            )));
        }
        // std::fs::rename on Windows does not replace existing directories
        match std::fs::rename(from, to) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(
                crate::error::Error::Msg(format!("destination '{}' already exists", to.display())),
            ),
            Err(e) => Err(crate::error::Error::Io(e)),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (from, to);
        Err(crate::error::Error::Msg(
            "atomic no-replace publication is not supported on this platform".into(),
        ))
    }
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
