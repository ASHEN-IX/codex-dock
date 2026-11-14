//! Platform-neutral Codex Desktop launch orchestration with native platform adapters.

use crate::accounts;
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::platform::process::{is_process_alive, query_profile, DiscoveryOutcome};
use crate::windows::WindowProc;
use std::path::{Path, PathBuf};

/// Launch configuration. Production callers always enforce platform capabilities.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Optional desktop executable path. This never bypasses platform capabilities.
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
    #[cfg(any(test, debug_assertions, feature = "synthetic-tests"))]
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
    // The historical default alias may point to the base home. Other aliases
    // must remain inside the managed root, even before account canonicalization.
    if name == "default" {
        let target = std::fs::canonicalize(p.profiles.join(name))?;
        let base = std::fs::canonicalize(&p.base)?;
        let profiles = std::fs::canonicalize(&p.profiles)?;
        if target != base && (!target.starts_with(&profiles) || target == profiles) {
            return Err(Error::Msg(
                "account 'default' escapes managed profiles root".into(),
            ));
        }
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
        let is_test_env = cfg!(any(test, debug_assertions, feature = "synthetic-tests"));
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
    // Hold this lock throughout startup; checking then unlocking races a new repair.
    let _repair_guard = crate::platform::fs::LockGuard::try_acquire(
        &repair_lock_path,
        "cannot launch desktop: Codex Dock repair",
    )?;

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
    match std::fs::read_to_string(&pid_file) {
        Ok(content) => {
            // Only a verified desktop record may be cleared automatically. An exited
            // launcher can still have descendants that have not become discoverable.
            let recorded_pid = content.trim().parse::<u32>().ok();
            if recorded_pid.is_none_or(is_process_alive) {
                return Err(Error::Msg(format!(
                    "previous desktop startup for account '{name}' is still running or starting, or indeterminate; refusing duplicate launch. Inspect processes before removing '{}'",
                    pid_file.display()
                )));
            }
            std::fs::remove_file(&pid_file)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::Io(e)),
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

    // Persist uncertainty BEFORE spawning. Failure to publish this record must
    // prevent launch, including crashes between spawn and verification.
    use std::io::Write;
    let mut record = crate::platform::fs::create_new_private(&pid_file)?;
    record.write_all(b"indeterminate\n")?;
    record.sync_all()?;
    drop(record);
    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            // No child was created: this is a confirmed launch failure.
            std::fs::remove_file(&pid_file)?;
            return Err(Error::Io(e));
        }
    };
    let launcher_pid = child.id();
    let mut child = OwnedChild(Some(child));

    // An exit (even nonzero) cannot prove that a launcher left no descendants.
    // Keep polling after either exit status and retain uncertainty on timeout.
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(options.startup_timeout_ms);
    let mut child_exited = None;
    let mut discovered_pid = None;
    let result = loop {
        if child_exited.is_none() {
            child_exited = child.0.as_mut().expect("owned child").try_wait()?;
        }
        if let DiscoveryOutcome::ManagedProfile(proc) = query_profile(p, name) {
            if proc.pid == launcher_pid && child_exited.is_none() {
                child_exited = child.0.as_mut().expect("owned child").try_wait()?;
            }
            // Exclude a reaped launcher and processes that vanished during scan.
            if !(proc.pid == launcher_pid && child_exited.is_some()) && is_process_alive(proc.pid) {
                discovered_pid = Some(proc.pid);
                #[allow(unused_mut)]
                let mut activated = true;
                if options.require_window_activation
                    && crate::platform::capabilities().window_activation
                        == crate::platform::Support::Yes
                {
                    #[cfg(target_os = "linux")]
                    {
                        activated = crate::windows::focus_pid(proc.pid);
                    }
                }
                if activated {
                    // Writing a verified PID must succeed before reporting success.
                    std::fs::write(&pid_file, proc.pid.to_string())?;
                    break Ok(WindowProc {
                        pid: proc.pid,
                        account: name.to_string(),
                        user_data_dir: proc.user_data_dir.unwrap_or_else(|| p.user_data_dir(name)),
                        launcher_pid: (proc.pid != launcher_pid).then_some(launcher_pid),
                    });
                }
            }
        }
        if start.elapsed() >= timeout {
            let detail = match (discovered_pid, child_exited) {
                (Some(pid), _) => format!("process {pid} remains running in background; no window could be shown"),
                (_, Some(status)) => format!("launcher exited before opening a window ({status}); delayed handoff cannot be excluded"),
                _ => format!("spawned process {launcher_pid} is still running"),
            };
            break Err(Error::Msg(format!(
                "Codex startup for '{name}' is indeterminate after {}ms: {detail}. Retry is blocked until startup is verified or processes are inspected and '{}' is removed",
                options.startup_timeout_ms, pid_file.display()
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };

    // 9. Release launch lock guard before exiting function
    drop(launch_guard);

    result
}

/// Reap on every exit path, including verification and record-write errors.
/// Dropping a Child alone neither waits nor terminates it. Never kill a process
/// merely because attribution or activation failed.
struct OwnedChild(Option<std::process::Child>);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
