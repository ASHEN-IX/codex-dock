use std::path::{Path, PathBuf};

/// Distribution-independent desktop discovery. The override is one executable
/// path, never a shell command; it also supports locally installed AppImages.
pub fn desktop() -> PathBuf {
    find_desktop(
        std::env::var_os("CODEX_DESKTOP_BIN").as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
}

fn find_desktop(override_bin: Option<&std::ffi::OsStr>, paths: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(bin) = override_bin.filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    if let Some(paths) = paths {
        for dir in std::env::split_paths(paths) {
            for name in ["chatgpt", "codex-desktop", "ChatGPT"] {
                let candidate = dir.join(name);
                if is_executable(&candidate) { return candidate; }
            }
        }
    }
    PathBuf::from("/usr/bin/chatgpt")
}

/// Where `codex` normally lives when it was installed by the desktop bundle.
const FALLBACKS: [&str; 4] = [
    "/usr/local/bin/codex",
    "/usr/bin/codex",
    "/usr/lib/chatgpt/resources/codex",
    "/opt/homebrew/bin/codex",
];

/// Locate the `codex` executable.
///
/// A GUI launch (launcher entry, autostart) does not always inherit the shell's
/// `PATH`, so a bare `"codex"` spawn can fail with "not found" even though the
/// terminal finds it. Resolve it once, here, instead of hoping.
pub fn codex() -> PathBuf {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("codex");
            if is_executable(&candidate) {
                return candidate;
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let candidate = Path::new(&home).join(".local/bin/codex");
        if is_executable(&candidate) {
            return candidate;
        }
    }
    for candidate in FALLBACKS {
        if is_executable(Path::new(candidate)) {
            return PathBuf::from(candidate);
        }
    }
    // Let the spawn fail and surface its error rather than guessing wrongly.
    PathBuf::from("codex")
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_discovery_supports_custom_paths_and_alternative_binary_names() {
        use std::ffi::OsStr;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("cd-desktop-tool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("codex-desktop");
        std::fs::write(&binary, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_desktop(None, Some(dir.as_os_str())), binary);
        let custom = OsStr::new("/home/a user/Apps/Codex.AppImage");
        assert_eq!(find_desktop(Some(custom), Some(dir.as_os_str())), PathBuf::from(custom));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_without_the_execute_bit_is_not_the_binary() {
        let dir = std::env::temp_dir().join(format!("cd-tool-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("codex");
        std::fs::write(&f, "#!/bin/sh\n").unwrap();
        assert!(!is_executable(&f));

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&f));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_path_is_not_executable() {
        assert!(!is_executable(Path::new("/nonexistent/codex")));
    }
}
