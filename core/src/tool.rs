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

fn find_desktop(
    override_bin: Option<&std::ffi::OsStr>,
    paths: Option<&std::ffi::OsStr>,
) -> PathBuf {
    if let Some(bin) = override_bin.filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    if let Some(paths) = paths {
        for dir in std::env::split_paths(paths) {
            for name in ["chatgpt", "codex-desktop", "ChatGPT"] {
                let candidate = dir.join(name);
                if is_executable(&candidate) {
                    return candidate;
                }
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
}
