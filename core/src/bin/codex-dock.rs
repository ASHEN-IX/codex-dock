use codex_dock_core::{accounts, store, usage, windows, Paths, StoreState};

fn stamp() -> String {
    let out = std::process::Command::new("date").arg("+%Y%m%d-%H%M%S").output();
    out.ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "backup".to_string())
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
        StoreState::Split { threads_in_profile } => format!("split ({threads_in_profile} threads in a profile)"),
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
    println!("\naccounts:");
    for a in &accts {
        let id = a.account_id.as_deref().unwrap_or("—");
        println!("  {:<12} {} ({})", a.name, if a.logged_in { "logged in" } else { "signed out" }, id);
    }
    if matches!(r.state, StoreState::Shared) {
        0
    } else {
        1
    }
}

fn cmd_usage(p: &Paths, with_usage: bool) -> i32 {
    let accts = accounts::list(p).unwrap_or_default();
    println!("{:<12} {:<8} {:>7} {:>7} {:>9}", "account", "plan", "5-hour", "weekly", "credits");
    for a in &accts {
        let u = if with_usage { Some(usage::read(p, &a.name, now())) } else { None };
        match u {
            Some(u) => {
                let pct = |v: Option<f64>| v.map(|x| format!("{x:.1}%")).unwrap_or_else(|| "—".into());
                println!(
                    "{:<12} {:<8} {:>7} {:>7} {:>9}",
                    a.name,
                    u.plan.as_deref().unwrap_or("—"),
                    pct(u.five_hour_percent),
                    pct(u.weekly_percent),
                    u.credits.map(|c| format!("{c:.1}")).unwrap_or_else(|| "—".into())
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let p = Paths::real();
    let code = match args.first().map(|s| s.as_str()) {
        Some("status") | None => cmd_status(&p),
        Some("list") => {
            for a in accounts::list(&p).unwrap_or_default() {
                println!("{}\t{}", a.name, a.account_id.unwrap_or_default());
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
                    println!("launched {} (pid {})", w.account, w.pid);
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
        Some("running") => {
            for w in windows::running(std::path::Path::new("/proc")) {
                println!("{}\t{}\tpid {}", w.account, w.user_data_dir.display(), w.pid);
            }
            0
        }
        Some(other) => {
            eprintln!("unknown command '{other}'");
            eprintln!("commands: status | list | usage | repair | launch <name> | running");
            1
        }
    };
    std::process::exit(code);
}
