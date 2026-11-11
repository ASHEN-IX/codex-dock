use codex_dock_core::paths::Paths;
use codex_dock_core::platform::launch::{
    launch_account_with_options, resolve_desktop_binary, resolve_macos_bundle_executable,
    LaunchOptions,
};
use codex_dock_core::windows;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LAUNCH_MUTEX: Mutex<()> = Mutex::new(());

struct TestDir(PathBuf);

impl TestDir {
    fn new(prefix: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cd-launch-{}-{}-{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup_test_env(test_name: &str) -> (Paths, TestDir) {
    let tmp = TestDir::new(test_name);
    let base = tmp.path().join("base");
    let profiles = tmp.path().join("profiles");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&profiles).unwrap();
    let paths = Paths::new(base, profiles);
    (paths, tmp)
}

fn create_mock_account(p: &Paths, name: &str) -> PathBuf {
    let prof_dir = p.profiles.join(name);
    fs::create_dir_all(&prof_dir).unwrap();
    prof_dir
}

fn get_mock_desktop_binary(dir: &Path, fail: bool) -> PathBuf {
    let mut target_dir = std::env::current_exe().expect("current_exe");
    target_dir.pop(); // remove deps binary
    if target_dir.file_name().and_then(|s| s.to_str()) == Some("deps") {
        target_dir.pop(); // remove "deps" folder to get target/debug
    }

    let bin_name = if cfg!(windows) {
        "mock-desktop.exe"
    } else {
        "mock-desktop"
    };
    let src_bin = target_dir.join(bin_name);

    let target_name = match (cfg!(windows), fail) {
        (true, false) => "Codex.exe",
        (true, true) => "Codex-fail.exe",
        (false, false) => "codex-desktop",
        (false, true) => "codex-desktop-fail",
    };
    let dest_bin = dir.join(target_name);

    if src_bin.is_file() {
        fs::copy(&src_bin, &dest_bin).expect("copy mock desktop");
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = if fail {
                r#"#!/bin/sh
exit 1
"#
            } else {
                r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in
        --user-data-dir=*)
            udir="${arg#--user-data-dir=}"
            echo "ARGS:$@" > "$udir/launch_record.txt"
            echo "CODEX_HOME:$CODEX_HOME" >> "$udir/launch_record.txt"
            ;;
    esac
done
sleep 3
"#
            };
            fs::write(&dest_bin, script).unwrap();
            let mut perms = fs::metadata(&dest_bin).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest_bin, perms).unwrap();
        }
        #[cfg(windows)]
        {
            panic!("mock-desktop.exe must be compiled at {}", src_bin.display());
        }
    }
    dest_bin
}

fn create_mock_cli_script(dir: &Path, name: &str) -> PathBuf {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin_path = bin_dir.join(name);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = r#"#!/bin/sh
echo "codex-cli 0.166.0"
"#;
        fs::write(&bin_path, script).unwrap();
        let mut perms = fs::metadata(&bin_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin_path, perms).unwrap();
    }
    #[cfg(windows)]
    {
        let script = r#"@echo off
echo codex-cli 0.166.0
"#;
        fs::write(&bin_path, script).unwrap();
    }
    bin_path
}

#[test]
fn launch_rejects_invalid_and_traversal_account_names() {
    let (paths, _tmp) = setup_test_env("invalid-names");

    let opts = LaunchOptions {
        enforce_capability: false,
        ..Default::default()
    };

    let err = launch_account_with_options(&paths, "../escape", &opts).unwrap_err();
    assert!(err.to_string().contains("invalid account name"));

    let err = launch_account_with_options(&paths, "has/slash", &opts).unwrap_err();
    assert!(err.to_string().contains("invalid account name"));

    let err = launch_account_with_options(&paths, "", &opts).unwrap_err();
    assert!(err.to_string().contains("cannot be empty") || err.to_string().contains("invalid"));
}

#[test]
fn launch_rejects_non_existent_account() {
    let (paths, _tmp) = setup_test_env("nonexistent");

    let opts = LaunchOptions {
        enforce_capability: false,
        ..Default::default()
    };

    let err = launch_account_with_options(&paths, "nonexistent", &opts).unwrap_err();
    assert!(
        err.to_string().contains("unknown account") || err.to_string().contains("does not exist"),
        "expected missing account error, got: {err}"
    );
}

#[test]
fn launch_refuses_when_repair_is_in_progress() {
    let (paths, _tmp) = setup_test_env("repair-active");
    create_mock_account(&paths, "work");

    // Acquire repair lock
    let repair_lock = paths.base.join(".dock-repair.lock");
    let _lock = codex_dock_core::platform::fs::LockGuard::try_acquire(&repair_lock, "repair")
        .expect("acquire repair lock");

    let opts = LaunchOptions {
        enforce_capability: false,
        ..Default::default()
    };

    let err = launch_account_with_options(&paths, "work", &opts).unwrap_err();
    assert!(
        err.to_string().contains("repair is currently in progress"),
        "expected repair active error, got: {err}"
    );
}

#[test]
fn launch_refuses_concurrent_launch_for_same_profile() {
    let (paths, _tmp) = setup_test_env("concurrent-launch");
    let prof_dir = create_mock_account(&paths, "work");

    // Simulate concurrent launch already holding profile launch lock
    let launch_lock = prof_dir.join(".desktop-launch.lock");
    let _lock = codex_dock_core::platform::fs::LockGuard::try_acquire(
        &launch_lock,
        "concurrent launch lock",
    )
    .expect("acquire launch lock");

    let opts = LaunchOptions {
        enforce_capability: false,
        ..Default::default()
    };

    let err = launch_account_with_options(&paths, "work", &opts).unwrap_err();
    assert!(
        err.to_string().contains("profile 'work' launch")
            || err.to_string().contains("lock contention"),
        "expected lock contention error, got: {err}"
    );
}

#[test]
fn launch_refuses_codex_cli_executable_as_desktop() {
    let (paths, tmp) = setup_test_env("cli-rejection");
    create_mock_account(&paths, "work");

    #[cfg(unix)]
    let cli_name = "codex";
    #[cfg(windows)]
    let cli_name = "codex.exe";

    let cli_path = create_mock_cli_script(tmp.path(), cli_name);

    let opts = LaunchOptions {
        binary_override: Some(cli_path.clone()),
        enforce_capability: false,
        ..Default::default()
    };

    let err = launch_account_with_options(&paths, "work", &opts).unwrap_err();
    assert!(
        err.to_string().contains("Codex CLI, not Codex Desktop"),
        "expected CLI rejection error, got: {err}"
    );
}

#[test]
fn resolve_desktop_binary_checks_presence_and_bundle_structure() {
    let tmp = TestDir::new("bundle-test");
    let bundle_dir = tmp.path().join("Codex.app");
    let macos_dir = bundle_dir.join("Contents").join("MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    let exec_path = macos_dir.join("Codex");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&exec_path, "#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&exec_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exec_path, perms).unwrap();
    }
    #[cfg(windows)]
    {
        fs::write(&exec_path, "@echo off\n").unwrap();
    }

    // Write Info.plist
    let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>Codex</string>
</dict>
</plist>"#;
    fs::write(bundle_dir.join("Contents").join("Info.plist"), plist).unwrap();

    let resolved = resolve_macos_bundle_executable(&bundle_dir);
    assert_eq!(resolved, Some(exec_path.clone()));

    let opts = LaunchOptions {
        binary_override: Some(exec_path.clone()),
        enforce_capability: false,
        ..Default::default()
    };
    let bin = resolve_desktop_binary(&opts).unwrap();
    assert_eq!(bin, exec_path);
}

#[test]
fn windows_launch_entrypoint_delegates_to_platform_launch() {
    let (paths, _tmp) = setup_test_env("windows-launch");
    create_mock_account(&paths, "work");

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let err = windows::launch(&paths, "work").unwrap_err();
        assert!(
            err.to_string().contains("safe_desktop_launch")
                || err.to_string().contains("desktop launch is not supported"),
            "expected capability refusal on Windows/macOS, got: {err}"
        );
    }

    #[cfg(target_os = "linux")]
    {
        let fake_bin = paths.base.join("fake-desktop-bin");
        std::env::set_var("CODEX_DESKTOP_BIN", &fake_bin);
        let err = windows::launch(&paths, "work").unwrap_err();
        std::env::remove_var("CODEX_DESKTOP_BIN");

        assert!(
            err.to_string().contains("Codex Desktop not found"),
            "expected desktop not found error, got: {err}"
        );
    }
}

#[test]
fn two_independent_profiles_receive_isolated_arguments_and_environment() {
    let _lock = LAUNCH_MUTEX.lock().unwrap();
    let (paths, tmp) = setup_test_env("multi-profile");
    let alpha_dir = create_mock_account(&paths, "alpha");
    let beta_dir = create_mock_account(&paths, "beta");

    let mock_bin = get_mock_desktop_binary(tmp.path(), false);

    let opts = LaunchOptions {
        binary_override: Some(mock_bin.clone()),
        enforce_capability: false,
        require_window_activation: false,
        startup_timeout_ms: 3000,
    };

    let alpha_proc = launch_account_with_options(&paths, "alpha", &opts)
        .expect("launch alpha with mock desktop");
    assert_eq!(alpha_proc.account, "alpha");

    let alpha_record_file = paths.user_data_dir("alpha").join("launch_record.txt");
    let alpha_content = fs::read_to_string(&alpha_record_file).expect("read alpha log");
    assert!(
        alpha_content.contains(&format!(
            "--user-data-dir={}",
            paths.user_data_dir("alpha").display()
        )),
        "expected alpha user data dir, got: {alpha_content}"
    );
    assert!(
        alpha_content.contains(&format!("CODEX_HOME:{}", alpha_dir.display())),
        "expected alpha CODEX_HOME, got: {alpha_content}"
    );

    // Launch Beta
    let beta_proc =
        launch_account_with_options(&paths, "beta", &opts).expect("launch beta with mock desktop");
    assert_eq!(beta_proc.account, "beta");

    let beta_record_file = paths.user_data_dir("beta").join("launch_record.txt");
    let beta_content = fs::read_to_string(&beta_record_file).expect("read beta log");
    assert!(
        beta_content.contains(&format!(
            "--user-data-dir={}",
            paths.user_data_dir("beta").display()
        )),
        "expected beta user data dir, got: {beta_content}"
    );
    assert!(
        beta_content.contains(&format!("CODEX_HOME:{}", beta_dir.display())),
        "expected beta CODEX_HOME, got: {beta_content}"
    );

    // Strict Isolation Verification:
    assert_ne!(
        paths.user_data_dir("alpha"),
        paths.user_data_dir("beta"),
        "user data directories must be completely distinct"
    );
    assert_ne!(alpha_proc.pid, beta_proc.pid, "pids must be distinct");

    // Clean up mock processes:
    terminate_pid(alpha_proc.pid);
    terminate_pid(beta_proc.pid);
}

#[cfg(unix)]
fn terminate_pid(pid: u32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        let _ = kill(pid as i32, 9);
    }
}

#[cfg(windows)]
fn terminate_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
}

#[test]
fn early_process_exit_during_startup_is_detected_and_cleans_up_lock() {
    let _lock = LAUNCH_MUTEX.lock().unwrap();
    let (paths, tmp) = setup_test_env("early-exit");
    create_mock_account(&paths, "work");

    let fail_bin = get_mock_desktop_binary(tmp.path(), true);

    let opts = LaunchOptions {
        binary_override: Some(fail_bin),
        enforce_capability: false,
        require_window_activation: false,
        startup_timeout_ms: 2000,
    };

    let err = launch_account_with_options(&paths, "work", &opts).unwrap_err();

    assert!(
        err.to_string().contains("exited before opening a window"),
        "expected premature exit error, got: {err}"
    );

    // Verify lock file is released and not left locked:
    let launch_lock = paths.profiles.join("work").join(".desktop-launch.lock");
    let lock_guard =
        codex_dock_core::platform::fs::LockGuard::try_acquire(&launch_lock, "verify lock release");
    assert!(
        lock_guard.is_ok(),
        "launch lock must be released after failed launch"
    );
}

#[test]
fn existing_process_detection_prevents_duplicate_launch() {
    let _lock = LAUNCH_MUTEX.lock().unwrap();
    let (paths, tmp) = setup_test_env("duplicate-prevention");
    create_mock_account(&paths, "work");

    let mock_bin = get_mock_desktop_binary(tmp.path(), false);

    let opts = LaunchOptions {
        binary_override: Some(mock_bin),
        enforce_capability: false,
        require_window_activation: false,
        startup_timeout_ms: 2000,
    };

    let first_launch =
        launch_account_with_options(&paths, "work", &opts).expect("first launch should succeed");
    assert_eq!(first_launch.account, "work");

    // Second launch attempt while first is still alive:
    let second_launch = launch_account_with_options(&paths, "work", &opts);

    // Depending on whether window_activation is Yes or No:
    // If Yes and focus fails: "running ... but its window could not be restored"
    // If No: "already running"
    // In either case, it must NOT spawn a new duplicate process!
    match second_launch {
        Ok(proc) => {
            // Only allowed if window focus succeeded and returned the SAME existing pid!
            assert_eq!(
                proc.pid, first_launch.pid,
                "re-launch must return existing process, never a duplicate"
            );
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("is running") || msg.contains("already running"),
                "expected existing process detection error, got: {msg}"
            );
        }
    }

    terminate_pid(first_launch.pid);
}
