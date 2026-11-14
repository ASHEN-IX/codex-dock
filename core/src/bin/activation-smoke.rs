//! Opt-in activation of an existing disposable profile; never launches Desktop.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 || args[0] != "--confirm-disposable" {
        eprintln!("usage: activation-smoke --confirm-disposable BASE PROFILES ACCOUNT");
        std::process::exit(2);
    }
    let Some(account) = args[3].to_str() else {
        std::process::exit(2);
    };
    let paths = codex_dock_core::paths::Paths::new(&args[1], &args[2]);
    match codex_dock_core::platform::activation::testing::activate_native_profile(&paths, account) {
        Ok(window) => println!(
            "verified foreground window {} for PID {}",
            window.id, window.pid
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn main() {
    eprintln!("This opt-in harness targets Windows/macOS; Linux keeps its existing focus adapter.");
    std::process::exit(2);
}
