//! Platform-neutral Codex Desktop launch orchestration with native platform adapters.

use crate::accounts;
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::platform::process::{is_process_alive, query_profile, DiscoveryOutcome};
use crate::windows::WindowProc;
use std::path::{Path, PathBuf};

/// Launch configuration options allowing test overrides and capability enforcement settings.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Optional binary path override (only honored in isolated synthetic tests).
    pub binary_override: Option<PathBuf>,
    /// Whether to require a GUI window activation confirmation if window_activation capability is supported.
    /// In production on Linux this is true; in headless synthetic tests this can be false.
    pub require_window_activation: bool,
    /// Timeout in milliseconds for verifying window or process attribution post-spawn.
    /// Default is 5000 ms (100 * 50ms).
    pub startup_timeout_ms: u64,
    /// Internal flag for controlled synthetic testing.
    synthetic_test: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            binary_override: None,
            require_window_activation: true,
            startup_timeout_ms: 5000,
            synthetic_test: false,
        }
    }
}

impl LaunchOptions {
    /// Construct launch options configured for an isolated synthetic test.
    pub fn synthetic_test(bin: Option<PathBuf>) -> Self {
        Self {
            binary_override: bin,
            require_window_activation: false,
            startup_timeout_ms: 5000,
            synthetic_test: true,
        }
    }

    /// Set startup verification timeout in milliseconds.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.startup_timeout_ms = timeout_ms;
        self
    }

    /// Configure whether GUI window activation is required.
    pub fn with_window_activation(mut self, require: bool) -> Self {
        self.require_window_activation = require;
        self
    }

    /// Whether this configuration is explicitly isolated for synthetic testing.
    pub fn is_synthetic_test(&self) -> bool {
        self.synthetic_test
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
pub fn launch_desktop(p: &Paths, name: &str) -> Result<WindowProc> {
    launch_desktop_with_options(p, name, &LaunchOptions::default())
}

/// Safely launch Codex Desktop for a managed profile with explicit options.
pub fn launch_desktop_with_options(
    p: &Paths,
    name: &str,
    options: &LaunchOptions,
) -> Result<WindowProc> {
    if !accounts::safe_segment(name) {
        return Err(Error::Msg(format!("invalid account name '{name}'")));
    }
    let canonical = accounts::canonical_name(p, name)?;
    let name = canonical.as_str();

    // Verify managed profile containment and ensure symlinks cannot escape profiles root
    let profile_dir = if name == "default" {
        let canonical_base = std::fs::canonicalize(&p.base).map_err(|e| {
            Error::Msg(format!(
                "account 'default' base directory '{}' could not be resolved: {e}",
                p.base.display()
            ))
        })?;
        if !canonical_base.is_dir() {
            return Err(Error::Msg(format!(
                "account 'default' directory '{}' is not a directory",
                p.base.display()
            )));
        }
        canonical_base
    } else {
        let target = p.profiles.join(name);
        if !target.is_dir() {
            return Err(Error::Msg(format!("account '{name}' does not exist")));
        }
        let canonical_profiles = std::fs::canonicalize(&p.profiles).map_err(|e| {
            Error::Msg(format!(
                "failed to canonicalize profiles root '{}': {e}",
                p.profiles.display()
            ))
        })?;
        let canonical_target = std::fs::canonicalize(&target).map_err(|e| {
            Error::Msg(format!(
                "failed to canonicalize account directory '{}': {e}",
                target.display()
            ))
        })?;
        if !canonical_target.starts_with(&canonical_profiles)
            || canonical_target == canonical_profiles
        {
            return Err(Error::Msg(format!(
                "account '{name}' directory '{}' escapes managed profiles root",
                target.display()
            )));
        }
        canonical_target
    };

    // Capability enforcement: ordinary release callers CANNOT bypass safe_desktop_launch
    if options.is_synthetic_test() {
        let is_test_env = cfg!(any(test, debug_assertions))
            || std::env::var_os("CODEX_DOCK_SYNTHETIC_TEST").is_some();
        if !is_test_env {
            return Err(Error::Msg(
                "synthetic test mode cannot be used to bypass platform capabilities in production release builds".to_string(),
            ));
        }
    } else {
        crate::platform::require(
            crate::platform::capabilities().safe_desktop_launch,
            "desktop launch",
        )?;
        crate::platform::require(
            crate::platform::capabilities().native_process_discovery,
            "desktop launch",
        )?;
    }

    // 1. Check if a Codex Dock repair is currently active; refuse launch to prevent conflicting writes.
    // Errors opening the repair lock file must NEVER be silently treated as absence.
    let repair_lock_path = p.base.join(".dock-repair.lock");
    if repair_lock_path.exists() {
        let file = crate::platform::fs::open_private(&repair_lock_path).map_err(|e| {
            Error::Msg(format!(
                "cannot verify repair lock '{}': {e}",
                repair_lock_path.display()
            ))
        })?;
        if file.try_lock().is_err() {
            return Err(Error::Msg(
                "cannot launch desktop: Codex Dock repair is currently in progress".to_string(),
            ));
        }
        let _ = file.unlock();
    }

    // 2. Acquire profile launch lock
    let launch_lock_path = profile_dir.join(".desktop-launch.lock");
    let launch_guard = crate::platform::fs::LockGuard::try_acquire(
        &launch_lock_path,
        &format!("profile '{name}' launch"),
    )?;

    // 3. Pre-flight process discovery: avoid duplicate launch and refuse ambiguous or incomplete states
    match query_profile(p, name) {
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
                            launcher_pid: None,
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
            // Clean absence confirmed; verify .desktop-launch.pid to prevent duplicate launch on retry
        }
    }

    let pid_file = profile_dir.join(".desktop-launch.pid");
    if pid_file.is_file() {
        if let Ok(content) = std::fs::read_to_string(&pid_file) {
            if let Ok(recorded_pid) = content.trim().parse::<u32>() {
                if is_process_alive(recorded_pid) {
                    return Err(Error::Msg(format!(
                        "a previously launched Codex process (pid {recorded_pid}) is still running or starting for account '{name}'; refusing duplicate launch"
                    )));
                }
            }
        }
        let _ = std::fs::remove_file(&pid_file);
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

    let launcher_pid = child.id();
    let _ = std::fs::write(&pid_file, launcher_pid.to_string());

    // 8. Startup verification loop: wait for process discovery to attribute the process
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(options.startup_timeout_ms);
    let mut verified_proc: Option<WindowProc> = None;
    let mut child_exited: Option<std::process::ExitStatus> = None;

    while start.elapsed() < timeout {
        if child_exited.is_none() {
            if let Some(status) = child.try_wait()? {
                child_exited = Some(status);
                if !status.success() {
                    // Check if discovery already attributed a running process despite launcher non-zero exit:
                    if let DiscoveryOutcome::ManagedProfile(proc) = query_profile(p, name) {
                        let wp = WindowProc {
                            pid: proc.pid,
                            account: name.to_string(),
                            user_data_dir: proc
                                .user_data_dir
                                .unwrap_or_else(|| p.user_data_dir(name)),
                            launcher_pid: (proc.pid != launcher_pid).then_some(launcher_pid),
                        };
                        let _ = std::fs::write(&pid_file, proc.pid.to_string());
                        verified_proc = Some(wp);
                        break;
                    } else {
                        let _ = std::fs::remove_file(&pid_file);
                        return Err(Error::Msg(format!(
                            "Codex for '{name}' exited before opening a window ({status})"
                        )));
                    }
                }
                // status.success(): clean handoff initiated; keep polling discovery
            }
        }

        if let DiscoveryOutcome::ManagedProfile(proc) = query_profile(p, name) {
            if crate::platform::capabilities().window_activation == crate::platform::Support::Yes
                && options.require_window_activation
            {
                #[cfg(target_os = "linux")]
                {
                    if crate::windows::focus_pid(proc.pid) {
                        let wp = WindowProc {
                            pid: proc.pid,
                            account: name.to_string(),
                            user_data_dir: proc
                                .user_data_dir
                                .unwrap_or_else(|| p.user_data_dir(name)),
                            launcher_pid: (proc.pid != launcher_pid).then_some(launcher_pid),
                        };
                        let _ = std::fs::write(&pid_file, proc.pid.to_string());
                        verified_proc = Some(wp);
                        break;
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let wp = WindowProc {
                        pid: proc.pid,
                        account: name.to_string(),
                        user_data_dir: proc.user_data_dir.unwrap_or_else(|| p.user_data_dir(name)),
                        launcher_pid: (proc.pid != launcher_pid).then_some(launcher_pid),
                    };
                    let _ = std::fs::write(&pid_file, proc.pid.to_string());
                    verified_proc = Some(wp);
                    break;
                }
            } else {
                let wp = WindowProc {
                    pid: proc.pid,
                    account: name.to_string(),
                    user_data_dir: proc.user_data_dir.unwrap_or_else(|| p.user_data_dir(name)),
                    launcher_pid: (proc.pid != launcher_pid).then_some(launcher_pid),
                };
                let _ = std::fs::write(&pid_file, proc.pid.to_string());
                verified_proc = Some(wp);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let result = match verified_proc {
        Some(w) => Ok(w),
        None => {
            if let Some(status) = child_exited {
                let _ = std::fs::remove_file(&pid_file);
                if !status.success() {
                    Err(Error::Msg(format!(
                        "Codex for '{name}' exited before opening a window ({status})"
                    )))
                } else {
                    Err(Error::Msg(format!(
                        "Codex launcher for '{name}' (pid {launcher_pid}) completed handoff, but no desktop process was discovered within {}ms",
                        options.startup_timeout_ms
                    )))
                }
            } else {
                // Child process is still running in background!
                if let DiscoveryOutcome::ManagedProfile(proc) = query_profile(p, name) {
                    let _ = std::fs::write(&pid_file, proc.pid.to_string());
                    Err(Error::Msg(format!(
                        "Codex started for '{name}' (pid {}), but no window could be shown within {}ms. Process remains running in background.",
                        proc.pid,
                        options.startup_timeout_ms
                    )))
                } else {
                    Err(Error::Msg(format!(
                        "Codex startup for '{name}' is indeterminate: spawned process (pid {launcher_pid}) is still running, but verification timed out after {}ms",
                        options.startup_timeout_ms
                    )))
                }
            }
        }
    };

    // 9. Release launch lock guard before exiting function
    drop(launch_guard);

    // 10. Spawn detached reaper thread to reap child when it eventually exits
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    result
}
