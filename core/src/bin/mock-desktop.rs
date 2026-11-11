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
