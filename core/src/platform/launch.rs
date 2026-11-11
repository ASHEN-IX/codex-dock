//! Platform-neutral Codex Desktop launch orchestration with native platform adapters.

use crate::accounts;
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::platform::process::{query_account, DiscoveryOutcome};
use crate::windows::WindowProc;
use std::path::{Path, PathBuf};

/// Launch configuration options allowing test overrides and capability enforcement settings.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Optional binary path override (e.g. for testing with mock desktop binaries).
    pub binary_override: Option<PathBuf>,
    /// Whether to enforce platform `safe_desktop_launch` capability check.
    /// In production (`launch_account`), this is always `true`. In synthetic unit tests,
    /// this may be set to `false` to verify the execution engine without official binaries.
    pub enforce_capability: bool,
    /// Whether to require a GUI window activation confirmation if window_activation capability is supported.
    /// In production on Linux this is true; in headless synthetic tests this can be false.
    pub require_window_activation: bool,
    /// Timeout in milliseconds for verifying window or process attribution post-spawn.
    /// Default is 5000 ms (100 * 50ms).
    pub startup_timeout_ms: u64,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            binary_override: None,
            enforce_capability: true,
            require_window_activation: true,
            startup_timeout_ms: 5000,
        }
    }
}

/// Inspect a macOS `.app` bundle directory to resolve its inner Mach-O executable.
pub fn resolve_macos_bundle_executable(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    let contents = path.join("Contents");
    if !contents.is_dir() {
        return None;
    }
    let macos_dir = contents.join("MacOS");
    if !macos_dir.is_dir() {
        return None;
    }

    // 1. Check Info.plist for CFBundleExecutable
    let plist_path = contents.join("Info.plist");
    if plist_path.is_file() {
        if let Ok(plist_content) = std::fs::read_to_string(&plist_path) {
            if let Some(pos) = plist_content.find("<key>CFBundleExecutable</key>") {
                let remainder = &plist_content[pos..];
                if let (Some(start), Some(end)) =
                    (remainder.find("<string>"), remainder.find("</string>"))
                {
                    if start < end {
                        let exec_name = remainder[start + 8..end].trim();
                        let exec_path = macos_dir.join(exec_name);
                        if exec_path.is_file() {
                            return Some(exec_path);
                        }
                    }
                }
            }
        }
    }

    // 2. Check bundle stem name (e.g. "Codex.app" -> "Codex")
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        let exec_path = macos_dir.join(stem);
        if exec_path.is_file() {
            return Some(exec_path);
        }
    }

    // 3. Fallback: first executable inside Contents/MacOS
    if let Ok(entries) = std::fs::read_dir(&macos_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if crate::tool::is_executable(&p) {
                return Some(p);
            }
        }
    }

    None
}

/// Resolve the Codex Desktop executable to use, verifying presence and rejecting standalone CLI binaries.
pub fn resolve_desktop_binary(options: &LaunchOptions) -> Result<PathBuf> {
    #[allow(unused_mut)]
    let mut bin = if let Some(override_path) = &options.binary_override {
        override_path.clone()
    } else {
        crate::tool::desktop()
    };

    // On macOS, if the binary path is an application bundle (.app directory),
    // resolve the inner Mach-O executable directly:
    #[cfg(target_os = "macos")]
    {
        if bin.is_dir() {
            if let Some(inner) = resolve_macos_bundle_executable(&bin) {
                bin = inner;
            }
        }
    }

    // Check executable presence:
    if !crate::tool::is_executable(&bin) {
        return Err(Error::Msg(format!(
            "Codex Desktop not found at {}. Put it on PATH or set CODEX_DESKTOP_BIN to its executable path.",
            bin.display()
        )));
    }

    // Refuse Codex CLI binaries:
    let cli_path = crate::tool::codex();
    if bin == cli_path || crate::platform::process::is_codex_cli(Some(&bin), &[]) {
        return Err(Error::Msg(format!(
            "resolved executable '{}' is the Codex CLI, not Codex Desktop; refusing to launch",
            bin.display()
        )));
    }

    Ok(bin)
}

/// Detach the child process from the controlling terminal / console session.
fn detach_from_controlling_terminal(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe. `pre_exec` runs in the forked
        // child before it execs, creating a new session so the desktop survives launcher exit.
        unsafe {
            cmd.pre_exec(|| {
                extern "C" {
                    fn setsid() -> i32;
                }
                if setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

/// Safely launch Codex Desktop for a managed profile.
pub fn launch_account(p: &Paths, name: &str) -> Result<WindowProc> {
    launch_account_with_options(p, name, &LaunchOptions::default())
}

/// Safely launch Codex Desktop for a managed profile with explicit options.
pub fn launch_account_with_options(
    p: &Paths,
    name: &str,
    options: &LaunchOptions,
) -> Result<WindowProc> {
    if !accounts::safe_segment(name) {
        return Err(Error::Msg(format!("invalid account name '{name}'")));
    }
    let canonical = accounts::canonical_name(p, name)?;
    let name = canonical.as_str();

    let profile_dir = if name == "default" {
        p.base.clone()
    } else {
        p.profiles.join(name)
    };

    if !profile_dir.is_dir() {
        return Err(Error::Msg(format!("account '{name}' does not exist")));
    }

    if options.enforce_capability {
        crate::platform::require(
            crate::platform::capabilities().safe_desktop_launch,
            "desktop launch",
        )?;
        crate::platform::require(
            crate::platform::capabilities().native_process_discovery,
            "desktop launch",
        )?;
    }

    // 1. Check if a Codex Dock repair is currently active; refuse launch to prevent conflicting writes
    let repair_lock_path = p.base.join(".dock-repair.lock");
    if repair_lock_path.is_file() {
        if let Ok(file) = crate::platform::fs::open_private(&repair_lock_path) {
            if file.try_lock().is_err() {
                return Err(Error::Msg(
                    "cannot launch desktop: Codex Dock repair is currently in progress".to_string(),
                ));
            }
            let _ = file.unlock();
        }
    }

    // 2. Acquire profile launch lock
    let launch_lock_path = profile_dir.join(".desktop-launch.lock");
    let launch_guard = crate::platform::fs::LockGuard::try_acquire(
        &launch_lock_path,
        &format!("profile '{name}' launch"),
    )?;

    // 3. Pre-flight process discovery: avoid duplicate launch and refuse ambiguous or incomplete states
    match query_account(p, name) {
        DiscoveryOutcome::ManagedProfile(proc) => {
            if crate::platform::capabilities().window_activation == crate::platform::Support::Yes {
                #[cfg(target_os = "linux")]
                {
                    if crate::windows::focus_pid(proc.pid) {
                        return Ok(WindowProc {
                            pid: proc.pid,
                            account: name.to_string(),
                            user_data_dir: proc
                                .user_data_dir
                                .unwrap_or_else(|| p.user_data_dir(name)),
                        });
                    }
                    return Err(Error::Msg(format!(
                        "Codex for '{name}' is running (pid {}) but its window could not be restored. Quit that Codex process, then open the account again.",
                        proc.pid
                    )));
                }
                #[cfg(not(target_os = "linux"))]
                {
                    return Err(Error::Msg(format!(
                        "Codex for '{name}' is already running (pid {})",
                        proc.pid
                    )));
                }
            } else {
                return Err(Error::Msg(format!(
                    "Codex for '{name}' is already running (pid {})",
                    proc.pid
                )));
            }
        }
        DiscoveryOutcome::UnknownProfile(proc) => {
            return Err(Error::Msg(format!(
                "cannot launch desktop: an ambiguous or unmanaged Codex process is running (pid {})",
                proc.pid
            )));
        }
        DiscoveryOutcome::InspectionIncompleteOrDenied(reason) => {
            return Err(Error::Msg(format!(
                "cannot safely launch desktop: process scan was incomplete ({reason})"
            )));
        }
        DiscoveryOutcome::Unsupported(reason) => {
            return Err(Error::Msg(format!(
                "cannot safely launch desktop: native process discovery is unsupported ({reason})"
            )));
        }
        DiscoveryOutcome::NoCodexProcess => {
            // Clean absence confirmed; proceed with launch
        }
    }

    // 4. Resolve desktop executable and reject CLI
    let bin = resolve_desktop_binary(options)?;

    // 5. Ensure independent user-data-dir exists with secure permissions
    let user_data = p.user_data_dir(name);
    std::fs::create_dir_all(&user_data)?;

    // 6. Synchronize project metadata if a managed profile
    if name != "default" {
        crate::store::sync_projects(p, name)?;
    }

    // 7. Spawn native desktop process without shell interpolation
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg(format!("--user-data-dir={}", user_data.display()));
    cmd.env("CODEX_HOME", profile_dir.as_os_str());
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    detach_from_controlling_terminal(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Msg(format!(
                "Codex Desktop not found at {}. Put it on PATH or set CODEX_DESKTOP_BIN to its executable path.",
                bin.display()
            ))
        } else {
            Error::Io(e)
        }
    })?;

    // 8. Startup verification loop: wait for process discovery to attribute the process
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(options.startup_timeout_ms);
    let mut verified_proc: Option<WindowProc> = None;

    while start.elapsed() < timeout {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(Error::Msg(format!(
                    "Codex for '{name}' exited before opening a window ({status})"
                )));
            }
        }

        if let DiscoveryOutcome::ManagedProfile(proc) = query_account(p, name) {
            if crate::platform::capabilities().window_activation == crate::platform::Support::Yes
                && options.require_window_activation
            {
                #[cfg(target_os = "linux")]
                {
                    if crate::windows::focus_pid(proc.pid) {
                        verified_proc = Some(WindowProc {
                            pid: proc.pid,
                            account: name.to_string(),
                            user_data_dir: proc
                                .user_data_dir
                                .unwrap_or_else(|| p.user_data_dir(name)),
                        });
                        break;
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    verified_proc = Some(WindowProc {
                        pid: proc.pid,
                        account: name.to_string(),
                        user_data_dir: proc.user_data_dir.unwrap_or_else(|| p.user_data_dir(name)),
                    });
                    break;
                }
            } else {
                verified_proc = Some(WindowProc {
                    pid: proc.pid,
                    account: name.to_string(),
                    user_data_dir: proc.user_data_dir.unwrap_or_else(|| p.user_data_dir(name)),
                });
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let result = match verified_proc {
        Some(w) => Ok(w),
        None => Err(Error::Msg(format!(
            "Codex started for '{name}', but no window could be shown. Check whether Codex Desktop is running in the background."
        ))),
    };

    // 9. Release launch lock guard before exiting function
    drop(launch_guard);

    // 10. Spawn detached reaper thread to reap child when it eventually exits
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    result
}
