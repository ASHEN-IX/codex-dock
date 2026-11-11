use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// The oldest Codex CLI known to advertise every `exec` flag pings rely on.
/// Documented capability floor, surfaced in error messages; never trusted as
/// the actual gate — [`exec_supports`] probes the installed CLI directly so
/// builds between releases and version scheme changes are detected honestly.
pub const MIN_PING_CLI_VERSION: (u64, u64, u64) = (0, 166, 0);

/// Reads the CLI's version banner, e.g. `codex-cli 0.164.0` → `(0, 164, 0)`.
/// `None` means the binary could not be run or printed something other than a
/// recognizable version; callers treat that as "unknown", never as a match.
pub fn codex_version(binary: &Path) -> Option<(u64, u64, u64)> {
    let mut attempts = 0;
    let out = loop {
        match Command::new(binary).arg("--version").output() {
            Ok(out) => break out,
            Err(e) if attempts < 5 && e.raw_os_error() == Some(26) => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    };
    if !out.status.success() {
        return None;
    }
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

/// Parses `codex-cli 0.164.0`, a bare `0.164.0`, or `0.164.0-nightly (abc)`.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    for line in text.lines() {
        for token in line.split_whitespace() {
            let core = token.split(['-', '+']).next().unwrap_or("");
            // Tolerate a single-letter product prefix such as v0.4.1.
            let core = core.strip_prefix(['v', 'V']).unwrap_or(core);
            let mut it = core.split('.');
            let major = it.next().and_then(|s| s.parse().ok());
            let minor = it.next().and_then(|s| s.parse().ok());
            let patch = it.next().and_then(|s| s.parse().ok());
            if it.next().is_some() {
                continue;
            }
            if let (Some(ma), Some(mi), Some(pa)) = (major, minor, patch) {
                return Some((ma, mi, pa));
            }
        }
    }
    None
}

/// Whether `codex exec` advertises every flag in `required`. Returns
/// `Ok(false)` for an honest "this CLI is too old"; `Err` only when the CLI
/// cannot be queried at all, so a caller cannot mistake a broken spawn for
/// support. The probe prints help text only and never contacts a network.
pub fn exec_supports(binary: &Path, required: &[&str]) -> Result<bool> {
    let mut attempts = 0;
    let out = loop {
        match Command::new(binary).args(["exec", "--help"]).output() {
            Ok(out) => break out,
            Err(e) if attempts < 5 && e.raw_os_error() == Some(26) => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(Error::Msg(format!("could not query the Codex CLI: {e}"))),
        }
    };
    if !out.status.success() {
        return Err(Error::Msg(
            "running `codex exec --help` failed; the Codex CLI may be broken".into(),
        ));
    }
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    Ok(required.iter().all(|f| text.contains(f)))
}

/// Refuses, with a message naming the missing surface, anything the installed
/// CLI cannot do. Unqueryable CLIs also refuse rather than spending allowance
/// on a guess.
pub fn require_exec_support(binary: &Path, required: &[&str]) -> Result<()> {
    if exec_supports(binary, required)? {
        return Ok(());
    }
    let (ma, mi, pa) = MIN_PING_CLI_VERSION;
    let version = codex_version(binary)
        .map(|(a, b, c)| format!("{a}.{b}.{c}"))
        .unwrap_or_else(|| "unknown".into());
    Err(Error::Msg(format!(
        "Codex CLI {version} does not advertise the flags required here ({}). \
         Pings need `codex exec` with --ignore-user-config, which came in {ma}.{mi}.{pa}.",
        required.join(" "),
    )))
}

/// Distribution-independent desktop discovery. The override is one executable
/// path, never a shell command; it also supports locally installed AppImages.
pub fn desktop() -> PathBuf {
    find_desktop(
        std::env::var_os("CODEX_DESKTOP_BIN").as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
}

/// Fallback desktop binary path if none found in PATH or standard locations.
pub fn default_desktop_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Applications/Codex.app/Contents/MacOS/Codex")
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
            PathBuf::from(local_app_data)
                .join("Programs")
                .join("Codex")
                .join("Codex.exe")
        } else {
            PathBuf::from("Codex.exe")
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        PathBuf::from("/usr/bin/chatgpt")
    }
}

/// System and user-level candidate locations for Codex Desktop across platforms.
pub fn desktop_candidates(paths: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(paths) = paths {
        for dir in std::env::split_paths(paths) {
            #[cfg(target_os = "windows")]
            let names = [
                "codex.exe",
                "chatgpt.exe",
                "ChatGPT.exe",
                "codex-desktop.exe",
                "Codex.exe",
                "codex",
                "chatgpt",
            ];
            #[cfg(target_os = "macos")]
            let names = ["chatgpt", "codex-desktop", "ChatGPT", "Codex"];
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            let names = ["chatgpt", "codex-desktop", "ChatGPT", "Codex"];

            for name in names {
                candidates.push(dir.join(name));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Codex.app/Contents/MacOS/Codex",
        ));
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
        ));
        if let Some(home) = std::env::var_os("HOME").filter(|s| !s.is_empty()) {
            let h = PathBuf::from(home);
            candidates.push(h.join("Applications/Codex.app/Contents/MacOS/Codex"));
            candidates.push(h.join("Applications/ChatGPT.app/Contents/MacOS/ChatGPT"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
            let lad = PathBuf::from(local_app_data);
            candidates.push(lad.join("Programs").join("Codex").join("Codex.exe"));
            candidates.push(lad.join("Programs").join("chatgpt").join("ChatGPT.exe"));
            candidates.push(lad.join("Codex").join("Codex.exe"));
            candidates.push(lad.join("ChatGPT").join("ChatGPT.exe"));
        }
        if let Some(pf) = std::env::var_os("ProgramFiles").filter(|s| !s.is_empty()) {
            let pfb = PathBuf::from(pf);
            candidates.push(pfb.join("Codex").join("Codex.exe"));
            candidates.push(pfb.join("ChatGPT").join("ChatGPT.exe"));
        }
        if let Some(pf86) = std::env::var_os("ProgramFiles(x86)").filter(|s| !s.is_empty()) {
            let pfb86 = PathBuf::from(pf86);
            candidates.push(pfb86.join("Codex").join("Codex.exe"));
            candidates.push(pfb86.join("ChatGPT").join("ChatGPT.exe"));
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        candidates.push(PathBuf::from("/usr/bin/chatgpt"));
        candidates.push(PathBuf::from("/usr/local/bin/chatgpt"));
        candidates.push(PathBuf::from("/opt/chatgpt/chatgpt"));
    }

    candidates
}

pub fn find_desktop(
    override_bin: Option<&std::ffi::OsStr>,
    paths: Option<&std::ffi::OsStr>,
) -> PathBuf {
    if let Some(bin) = override_bin.filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    for candidate in desktop_candidates(paths) {
        if is_executable(&candidate) {
            return candidate;
        }
    }
    default_desktop_path()
}

/// Where `codex` normally lives when it was installed by the desktop bundle.
#[cfg(unix)]
const FALLBACKS: [&str; 4] = [
    "/usr/local/bin/codex",
    "/usr/bin/codex",
    "/usr/lib/chatgpt/resources/codex",
    "/opt/homebrew/bin/codex",
];

/// Candidate locations for the `codex` CLI across platforms.
pub fn codex_candidates(paths: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(paths) = paths {
        for dir in std::env::split_paths(paths) {
            #[cfg(target_os = "windows")]
            let names = ["codex.exe", "codex.cmd", "codex.bat", "codex"];
            #[cfg(not(target_os = "windows"))]
            let names = ["codex"];

            for name in names {
                candidates.push(dir.join(name));
            }
        }
    }

    #[cfg(unix)]
    {
        if let Some(home) = std::env::var_os("HOME").filter(|s| !s.is_empty()) {
            candidates.push(Path::new(&home).join(".local/bin/codex"));
        }
        for fb in FALLBACKS {
            candidates.push(PathBuf::from(fb));
        }
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/codex",
        ));
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex",
        ));
        if let Some(home) = std::env::var_os("HOME").filter(|s| !s.is_empty()) {
            let h = PathBuf::from(home);
            candidates.push(h.join("Applications/Codex.app/Contents/Resources/codex"));
            candidates.push(h.join("Applications/ChatGPT.app/Contents/Resources/codex"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(user_profile) = std::env::var_os("USERPROFILE").filter(|s| !s.is_empty()) {
            let up = PathBuf::from(user_profile);
            candidates.push(up.join(".cargo").join("bin").join("codex.exe"));
        }
        if let Some(app_data) = std::env::var_os("APPDATA").filter(|s| !s.is_empty()) {
            let ad = PathBuf::from(app_data);
            candidates.push(ad.join("npm").join("codex.cmd"));
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
            let lad = PathBuf::from(local_app_data);
            candidates.push(
                lad.join("Programs")
                    .join("Codex")
                    .join("resources")
                    .join("codex.exe"),
            );
            candidates.push(
                lad.join("Programs")
                    .join("chatgpt")
                    .join("resources")
                    .join("codex.exe"),
            );
        }
    }

    candidates
}

pub fn default_codex_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from("codex.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("codex")
    }
}

/// Locate the `codex` executable.
///
/// A GUI launch (launcher entry, autostart) does not always inherit the shell's
/// `PATH`, so a bare `"codex"` spawn can fail with "not found" even though the
/// terminal finds it. Resolve it once, here, instead of hoping.
pub fn codex() -> PathBuf {
    let override_bin = std::env::var_os("CODEX_CLI_BIN");
    if let Some(bin) = override_bin.filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    let paths = std::env::var_os("PATH");
    for candidate in codex_candidates(paths.as_deref()) {
        if is_executable(&candidate) {
            return candidate;
        }
    }
    default_codex_path()
}

/// Standard system locations where sqlite3 CLI binary might reside.
pub fn sqlite3_standard_locations() -> Vec<PathBuf> {
    let mut locs = Vec::new();
    #[cfg(target_os = "macos")]
    {
        locs.push(PathBuf::from("/usr/bin/sqlite3"));
        locs.push(PathBuf::from("/opt/homebrew/bin/sqlite3"));
        locs.push(PathBuf::from("/usr/local/bin/sqlite3"));
    }
    #[cfg(target_os = "linux")]
    {
        locs.push(PathBuf::from("/usr/bin/sqlite3"));
        locs.push(PathBuf::from("/usr/local/bin/sqlite3"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(pd) = std::env::var_os("ProgramData").filter(|s| !s.is_empty()) {
            locs.push(
                PathBuf::from(pd)
                    .join("chocolatey")
                    .join("bin")
                    .join("sqlite3.exe"),
            );
        }
        if let Some(lad) = std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
            locs.push(
                PathBuf::from(lad)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join("sqlite3.exe"),
            );
        }
        if let Some(up) = std::env::var_os("USERPROFILE").filter(|s| !s.is_empty()) {
            locs.push(
                PathBuf::from(up)
                    .join("scoop")
                    .join("shims")
                    .join("sqlite3.exe"),
            );
        }
    }
    locs
}

/// Discover sqlite3 CLI binary across PATH and standard platform locations.
pub fn find_sqlite3() -> Option<PathBuf> {
    find_sqlite3_with_paths(std::env::var_os("PATH").as_deref())
}

pub fn find_sqlite3_with_paths(paths: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(paths) = paths {
        for dir in std::env::split_paths(paths) {
            #[cfg(target_os = "windows")]
            let names = ["sqlite3.exe", "sqlite3.bat", "sqlite3.cmd", "sqlite3"];
            #[cfg(not(target_os = "windows"))]
            let names = ["sqlite3"];

            for name in names {
                let candidate = dir.join(name);
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    sqlite3_standard_locations()
        .into_iter()
        .find(|loc| is_executable(loc))
}

/// Query sqlite3 version string (e.g. "3.45.1").
pub fn sqlite3_version(binary: &Path) -> Option<String> {
    let out = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().next().map(|s| s.to_string())
}

pub(crate) fn is_executable(path: &Path) -> bool {
    crate::platform::fs::is_executable(path)
}

#[cfg(all(test, unix))]
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
        assert_eq!(
            find_desktop(Some(custom), Some(dir.as_os_str())),
            PathBuf::from(custom)
        );
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

    #[test]
    fn cli_version_banner_is_parsed_faithfully() {
        assert_eq!(parse_version("codex-cli 0.164.0"), Some((0, 164, 0)));
        assert_eq!(parse_version("0.166.0\n"), Some((0, 166, 0)));
        assert_eq!(
            parse_version("codex-cli 0.165.1-nightly (build abc7)"),
            Some((0, 165, 1))
        );
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        // Not a version at all must never masquerade as one.
        assert_eq!(parse_version("hello world"), None);
        assert_eq!(parse_version("codex-cli zero.one.two"), None);
    }

    #[test]
    fn cli_capability_probe_reports_what_the_binary_advertises() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("cd-tool-cap-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        // Prints help that deliberately lacks --ignore-user-config.
        let old = dir.join("old-codex");
        std::fs::write(
            &old,
            "#!/bin/sh\nif [ \"$1\" = exec ]; then echo 'usage: codex exec [--ephemeral] [--ignore-rules]'; exit 0; fi\necho 'codex-cli 0.100.0'; exit 0\n",
        )
        .unwrap();
        let newer = dir.join("new-codex");
        std::fs::write(
            &newer,
            "#!/bin/sh\nif [ \"$1\" = exec ]; then echo '[--ephemeral] [--ignore-user-config] [--ignore-rules]'; exit 0; fi\necho 'codex-cli 0.170.0'; exit 0\n",
        )
        .unwrap();
        let broken = dir.join("broken-codex");
        std::fs::write(&broken, "#!/bin/sh\nexit 3\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        for f in [&old, &newer, &broken] {
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let want = ["--ephemeral", "--ignore-user-config", "--ignore-rules"];
        assert!(!exec_supports(&old, &want).unwrap());
        assert!(require_exec_support(&old, &want)
            .unwrap_err()
            .to_string()
            .contains("0.100.0"));
        assert!(exec_supports(&newer, &want).unwrap());
        assert!(require_exec_support(&newer, &want).is_ok());
        // A CLI that cannot be queried is an error, never silent support.
        assert!(exec_supports(&broken, &want).is_err());
        assert!(exec_supports(Path::new("/nonexistent/codex"), &want).is_err());
        std::fs::remove_dir_all(dir).unwrap();
        Ok(())
    }

    #[test]
    fn candidates_are_non_empty_and_valid_paths() {
        let dc = desktop_candidates(None);
        assert!(!dc.is_empty(), "desktop candidates should produce paths");
        let cc = codex_candidates(None);
        assert!(!cc.is_empty(), "codex candidates should produce paths");
        let def_d = default_desktop_path();
        assert!(!def_d.as_os_str().is_empty());
        let def_c = default_codex_path();
        assert!(!def_c.as_os_str().is_empty());
    }

    #[test]
    fn sqlite3_probe_checks_standard_locations_or_path() {
        let locs = sqlite3_standard_locations();
        // locs can be empty or populated depending on platform, but should not panic
        let _ = locs;
        if let Some(sqlite) = find_sqlite3() {
            let ver = sqlite3_version(&sqlite);
            assert!(ver.is_some(), "found sqlite3 must return version string");
        }
    }
}
