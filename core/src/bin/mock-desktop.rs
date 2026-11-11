#[allow(clippy::zombie_processes)]
fn spawn_detached_mock(args_to_pass: &[String]) {
    let exe = std::env::current_exe().expect("current_exe");
    let target_name = if cfg!(windows) {
        "Codex.exe"
    } else {
        "codex-desktop"
    };
    let target_bin = exe.with_file_name(target_name);
    let bin_to_run = if target_bin.is_file() {
        target_bin
    } else {
        exe
    };

    let mut cmd = std::process::Command::new(bin_to_run);
    cmd.arg("--mock-child");
    for arg in args_to_pass {
        cmd.arg(arg);
    }
    if let Ok(ch) = std::env::var("CODEX_HOME") {
        cmd.env("CODEX_HOME", ch);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
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
    let _ = cmd.spawn().expect("spawn detached mock child");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_name = exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if exe_name.contains("fail") || args.iter().any(|a| a == "--exit-failure") {
        std::process::exit(1);
    }

    let is_child = args.iter().any(|a| a == "--mock-child");

    if !is_child {
        if args.iter().any(|a| a == "--slow-launcher") || exe_name.contains("slow-launcher") {
            std::thread::sleep(std::time::Duration::from_millis(350));
            let forwarded: Vec<String> = args
                .into_iter()
                .skip(1)
                .filter(|a| a != "--slow-launcher")
                .collect();
            spawn_detached_mock(&forwarded);
            std::process::exit(0);
        }

        if args.iter().any(|a| a == "--handoff") || exe_name.contains("launcher") {
            let forwarded: Vec<String> = args
                .into_iter()
                .skip(1)
                .filter(|a| a != "--handoff")
                .collect();
            spawn_detached_mock(&forwarded);
            std::process::exit(0);
        }
    }

    // Automatically record launch arguments and CODEX_HOME into launch_record.txt inside user_data_dir:
    for arg in &args {
        if let Some(user_data) = arg.strip_prefix("--user-data-dir=") {
            let log_file = std::path::Path::new(user_data).join("launch_record.txt");
            let content = format!(
                "ARGS:{}\nCODEX_HOME:{}\n",
                args.join(" "),
                std::env::var("CODEX_HOME").unwrap_or_default()
            );
            let _ = std::fs::write(log_file, content);
        }
    }

    // Stay alive for 10 seconds so process discovery can detect and attribute the running process:
    std::thread::sleep(std::time::Duration::from_millis(10000));
}
