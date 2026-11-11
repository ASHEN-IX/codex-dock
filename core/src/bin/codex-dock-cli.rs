use codex_dock_core::{accounts, store, usage, windows, Paths, StoreState};

fn stamp() -> String {
    // On Unix `date` has always produced the local-time stamp backups carry;
    // keep that exact behaviour on the platforms where it works. Elsewhere a
    // filesystem-safe UTC stamp is honest about being UTC.
    #[cfg(unix)]
    {
        let out = std::process::Command::new("date")
            .arg("+%Y%m%d-%H%M%S")
            .output();
        out.ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "backup".to_string())
    }
    #[cfg(not(unix))]
    {
        codex_dock_core::platform::fs::utc_stamp().unwrap_or_else(|| "backup".to_string())
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn state_label(s: &StoreState) -> String {
    match s {
        StoreState::Shared => "shared".into(),
        StoreState::NotConfigured => "not configured".into(),
        StoreState::Split { threads_in_profile } => {
            format!("split ({threads_in_profile} threads in a profile)")
        }
        StoreState::AuthCycle => "auth cycle".into(),
        StoreState::Nested { profile } => format!("nested ({profile})"),
    }
}

fn cmd_status(p: &Paths) -> i32 {
    let r = store::inspect(p);
    println!("store:    {}", state_label(&r.state));
    println!("threads:  {}", r.threads);
    println!("projects: {}", r.projects.join(", "));
    let accts = accounts::list(p).unwrap_or_default();
    let signed_in = accts.iter().filter(|a| a.logged_in).count();
    println!("{}", status_counts_line(accts.len(), signed_in));
    if let Some(e) = &r.error {
        println!("unreadable: {e}");
    }
    status_exit(matches!(r.state, StoreState::Shared), r.error.as_ref())
}

/// `Shared` only means healthy when the store's contents were actually read:
/// an error overrides the state, because a store we could not open must never
/// exit as if everything were fine.
fn status_exit(is_shared: bool, error: Option<&String>) -> i32 {
    if is_shared && error.is_none() {
        0
    } else {
        1
    }
}

/// The diagnostic renderer accepts only counts, never credential-bearing models.
fn status_counts_line(configured: usize, signed_in: usize) -> String {
    format!("accounts: {configured} configured, {signed_in} signed in")
}

/// Process IDs are enough to identify windows without exporting account
/// metadata or per-window paths into captured terminal logs.
fn running_line(window: &windows::WindowProc) -> String {
    format!("pid {}", window.pid)
}

fn cmd_running(p: &Paths, account: Option<&str>) -> i32 {
    if let codex_dock_core::platform::Support::No(reason) =
        codex_dock_core::platform::capabilities().native_process_discovery
    {
        eprintln!("no running windows to report: {reason}");
        return 1;
    }
    let filter = match account
        .map(|name| accounts::canonical_name(p, name))
        .transpose()
    {
        Ok(name) => name,
        Err(e) => {
            eprintln!("could not select profile: {e}");
            return 1;
        }
    };
    for window in windows::running_native(p) {
        if filter.as_ref().is_none_or(|name| *name == window.account) {
            println!("{}", running_line(&window));
        }
    }
    0
}

fn cmd_usage(p: &Paths, with_usage: bool) -> i32 {
    let accts = accounts::list(p).unwrap_or_default();
    println!(
        "{:<12} {:<8} {:>7} {:>7} {:>9}",
        "account", "plan", "5-hour", "weekly", "credits"
    );
    for a in &accts {
        let u = if with_usage {
            Some(usage::read(p, &a.name, now()))
        } else {
            None
        };
        match u {
            Some(u) => {
                let pct =
                    |v: Option<f64>| v.map(|x| format!("{x:.1}%")).unwrap_or_else(|| "—".into());
                println!(
                    "{:<12} {:<8} {:>7} {:>7} {:>9}",
                    a.name,
                    u.plan.as_deref().unwrap_or("—"),
                    pct(u.five_hour_percent),
                    pct(u.weekly_percent),
                    u.credits
                        .map(|c| format!("{c:.1}"))
                        .unwrap_or_else(|| "—".into())
                );
                if let Some(e) = u.error {
                    println!("  ! {e}");
                }
            }
            None => println!("{:<12} {:<8} {:>7} {:>7} {:>9}", a.name, "—", "—", "—", "—"),
        }
    }
    0
}

fn cmd_add(p: &Paths, name: &str, clone_from: Option<&str>) -> i32 {
    let res = match accounts::add_account(p, name, clone_from) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("could not add account '{name}': {e}");
            return 1;
        }
    };
    let profile_path = p.profiles.join(name);
    println!("created '{name}' ({})", profile_path.display());
    if res.shared_store {
        println!("store:   linked to the shared database");
    } else {
        println!("store:   isolated profile (shared store not supported on this platform)");
    }
    println!("sign in: codex-dock login {name}");
    0
}

fn cmd_remove(p: &Paths, name: &str) -> i32 {
    let bak = p.backup_dir(&stamp());
    match accounts::remove(p, name, &bak) {
        Ok(moved) => {
            println!("removed '{}' -> {}", name, moved.display());
            println!("backup:  {}", bak.display());
            0
        }
        Err(codex_dock_core::Error::PartialRemoval {
            profile_backup,
            message,
        }) => {
            eprintln!(
                "partial removal: profile moved to '{}', but window data stayed behind: {message}",
                profile_backup.display()
            );
            println!("backup:  {}", bak.display());
            1
        }
        Err(e) => {
            eprintln!("could not remove '{name}': {e}");
            1
        }
    }
}

fn cmd_login(p: &Paths, name: &str) -> i32 {
    let home = match accounts::validate_login_target(p, name) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("cannot sign in to '{name}': {e}");
            return 1;
        }
    };
    let _login_guard = match accounts::acquire_login_lock(p, name) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("cannot sign in to '{name}': {e}");
            return 1;
        }
    };
    // Same protection as the window: never let login write through a symlink
    // into another account's credentials.
    let bak = p.backup_dir(&stamp());
    match accounts::materialise_auth(&home, &bak) {
        Ok(true) => println!("kept a real auth.json (previous link in {})", bak.display()),
        Ok(false) => {}
        Err(e) => {
            eprintln!("could not protect the existing credentials: {e}");
            return 1;
        }
    }
    // Inherit stdio: the device code and the waiting happen in the terminal.
    let status = std::process::Command::new(codex_dock_core::tool::codex())
        .args(["login", "--device-auth"])
        .env("CODEX_HOME", &home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .status();
    match status {
        Ok(s) if s.success() => {
            let (_, ok) = accounts::auth_info(&home.join("auth.json"));
            if ok {
                println!("'{name}' signed in");
                0
            } else {
                eprintln!("login exited cleanly but no credentials were written");
                1
            }
        }
        Ok(_) => {
            match accounts::restore_auth(&home, &bak) {
                Ok(true) => eprintln!("login did not complete; previous credentials restored"),
                _ => eprintln!("login did not complete"),
            }
            1
        }
        Err(e) => {
            let _ = accounts::restore_auth(&home, &bak);
            eprintln!("could not run codex login: {e}");
            1
        }
    }
}

fn cmd_audit(p: &Paths) -> i32 {
    let audit = codex_dock_core::audit::audit_platform(p);
    println!("{}", codex_dock_core::audit::format_audit_report(&audit));
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let p = Paths::real();
    let code = match args.first().map(|s| s.as_str()) {
        Some("status") | None => cmd_status(&p),
        Some("doctor") | Some("audit") => cmd_audit(&p),
        Some("list") => {
            for a in accounts::list(&p).unwrap_or_default() {
                println!("{}", a.name);
            }
            0
        }
        Some("usage") => cmd_usage(&p, true),
        Some("repair") => {
            let bak = p.backup_dir(&stamp());
            match store::repair(&p, &bak) {
                Ok(r) => {
                    println!("store:    {}", state_label(&r.state));
                    println!("threads:  {}", r.threads);
                    for (name, ino) in &r.inodes {
                        println!("  {name}: inode {ino}");
                    }
                    println!("backup:   {}", bak.display());
                    0
                }
                Err(e) => {
                    eprintln!("repair failed: {e}");
                    1
                }
            }
        }
        Some("launch") => match args.get(1) {
            Some(name) => match windows::launch(&p, name) {
                Ok(w) => {
                    println!("opened window ({})", running_line(&w));
                    0
                }
                Err(e) => {
                    eprintln!("launch failed: {e}");
                    1
                }
            },
            None => {
                eprintln!("usage: codex-dock launch <account>");
                1
            }
        },
        Some("running") => cmd_running(&p, args.get(1).map(String::as_str)),
        Some("add") => match args.get(1) {
            Some(name) => cmd_add(&p, name, args.get(2).map(|s| s.as_str())),
            None => {
                eprintln!("usage: codex-dock add <name> [clone-from]");
                1
            }
        },
        Some("remove") | Some("rm") => match args.get(1) {
            Some(name) => cmd_remove(&p, name),
            None => {
                eprintln!("usage: codex-dock remove <name>");
                1
            }
        },
        Some("login") => match args.get(1) {
            Some(name) => cmd_login(&p, name),
            None => {
                eprintln!("usage: codex-dock login <name>");
                1
            }
        },
        Some(other) => {
            eprintln!("unknown command '{other}'");
            eprintln!(
                "commands: status | list | usage | add <name> [clone] | remove <name> | \
                 login <name> | doctor | repair (disabled) | launch <name> | running [name]"
            );
            1
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_store_with_an_unread_error_never_exits_as_shared_and_healthy() {
        let healthy = status_exit(true, None);
        let corrupt_shape = status_exit(true, Some(&"not a database".to_string()));
        let not_configured = status_exit(false, None);
        assert_eq!(healthy, 0);
        assert_eq!(corrupt_shape, 1, "Shared with an error is not a pass");
        assert_eq!(not_configured, 1);
    }

    #[test]
    fn routine_status_does_not_disclose_account_metadata() {
        let account = accounts::Account {
            name: "work".into(),
            path: PathBuf::from("/home/private-user/.codex-profiles/work"),
            account_id: Some("sensitive-provider-identifier".into()),
            logged_in: true,
        };
        let accounts = [account];
        let line = status_counts_line(
            accounts.len(),
            accounts.iter().filter(|a| a.logged_in).count(),
        );
        assert!(line.contains("1 configured, 1 signed in"));
        assert!(!line.contains("work"));
        assert!(!line.contains("sensitive-provider-identifier"));
        assert!(!line.contains("private-user"));
    }

    #[test]
    fn running_output_does_not_disclose_process_account_metadata() {
        let window = windows::WindowProc {
            pid: 42,
            account: "private-account-label".into(),
            user_data_dir: PathBuf::from("/home/private-user/.config/Codex-private-account-label"),
        };
        let line = running_line(&window);
        assert!(line.contains("42"));
        assert!(!line.contains("private-account-label"));
        assert!(!line.contains("private-user"));
        assert!(!line.contains("Codex-"));
    }
}
