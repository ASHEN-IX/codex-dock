//! Platform runtime audit, executable discovery, and profile isolation validation.
//!
//! Provides safe, opt-in diagnostics that:
//! - Discover installed desktop and CLI executables across Linux, macOS, and Windows.
//! - Report capabilities truthfully without premature claims.
//! - Validate per-profile isolation using disposable synthetic fixtures.
//! - Refuse destructive operations and avoid touching live user data.

use crate::error::Result;
use crate::paths::Paths;
use crate::platform::{self, Capabilities, Support};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static AUDIT_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Blocked(&'static str),
    Skipped(&'static str),
}

impl CheckStatus {
    pub fn is_pass(&self) -> bool {
        matches!(self, CheckStatus::Pass)
    }

    pub fn label(&self) -> String {
        match self {
            CheckStatus::Pass => "PASS".to_string(),
            CheckStatus::Fail => "FAIL".to_string(),
            CheckStatus::Blocked(reason) => format!("BLOCKED ({reason})"),
            CheckStatus::Skipped(reason) => format!("SKIPPED ({reason})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub name: String,
    pub path: Option<PathBuf>,
    pub found: bool,
    pub version: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathsAudit {
    pub home: PathBuf,
    pub config_root: PathBuf,
    pub base_store: PathBuf,
    pub profiles_dir: PathBuf,
    pub sample_user_data_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageAudit {
    pub shared_store_supported: bool,
    pub shared_store_reason: Option<String>,
    pub isolation_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationAudit {
    pub codex_home_propagation: CheckStatus,
    pub user_data_dir_isolation: CheckStatus,
    pub credential_separation: CheckStatus,
    pub rollout_projection_isolation: CheckStatus,
    pub safe_refusal_unsupported_ops: CheckStatus,
    pub overall: CheckStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDiscoveryAudit {
    pub status: CheckStatus,
    pub method: String,
    pub scan_completed: bool,
    pub running_processes_count: usize,
    pub confirmed_desktop_count: usize,
    pub candidate_desktop_count: usize,
    pub confident_profile_count: usize,
    pub unknown_profile_count: usize,
    pub restrictions: Vec<String>,
    pub synthetic_vs_real_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformAudit {
    pub os: String,
    pub arch: String,
    pub capabilities: Capabilities,
    pub desktop_probe: ProbeResult,
    pub cli_probe: ProbeResult,
    pub sqlite3_probe: ProbeResult,
    pub paths_audit: PathsAudit,
    pub storage_audit: StorageAudit,
    pub isolation_audit: IsolationAudit,
    pub process_discovery_audit: ProcessDiscoveryAudit,
    pub desktop_runtime_status: CheckStatus,
}

/// Execute a non-destructive runtime audit of the current platform environment.
pub fn audit_platform(p: &Paths) -> PlatformAudit {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let capabilities = platform::capabilities();

    // 1. Probe Desktop binary
    let desktop_bin = crate::tool::desktop();
    let desktop_found = platform::fs::is_executable(&desktop_bin);
    let desktop_probe = ProbeResult {
        name: "Codex Desktop".to_string(),
        path: if desktop_found {
            Some(desktop_bin.clone())
        } else {
            None
        },
        found: desktop_found,
        version: None,
        details: if desktop_found {
            Some(format!("executable at {}", desktop_bin.display()))
        } else {
            Some(format!(
                "not found (checked PATH and standard platform paths; default fallback: {})",
                desktop_bin.display()
            ))
        },
    };

    // 2. Probe Codex CLI binary
    let cli_bin = crate::tool::codex();
    let cli_found = platform::fs::is_executable(&cli_bin);
    let cli_version = if cli_found {
        crate::tool::codex_version(&cli_bin).map(|(ma, mi, pa)| format!("{ma}.{mi}.{pa}"))
    } else {
        None
    };
    let ping_support = if cli_found {
        crate::tool::exec_supports(
            &cli_bin,
            &["--ephemeral", "--ignore-user-config", "--ignore-rules"],
        )
        .ok()
    } else {
        None
    };
    let cli_details = match (cli_found, &cli_version, ping_support) {
        (true, Some(ver), Some(true)) => {
            Some(format!("v{ver} (exec supports ping isolation flags)"))
        }
        (true, Some(ver), Some(false)) => Some(format!(
            "v{ver} (exec lacks --ignore-user-config; ping operations unsupported)"
        )),
        (true, Some(ver), None) => Some(format!("v{ver} (query failed)")),
        (true, None, _) => Some("executable found (unversioned banner)".to_string()),
        (false, _, _) => Some(format!(
            "not found (checked PATH and standard platform paths; default fallback: {})",
            cli_bin.display()
        )),
    };
    let cli_probe = ProbeResult {
        name: "Codex CLI".to_string(),
        path: if cli_found { Some(cli_bin) } else { None },
        found: cli_found,
        version: cli_version,
        details: cli_details,
    };

    // 3. Probe SQLite3 binary
    let sqlite_path = crate::tool::find_sqlite3();
    let sqlite_found = sqlite_path.is_some();
    let sqlite_ver = sqlite_path
        .as_ref()
        .and_then(|path| crate::tool::sqlite3_version(path));
    let sqlite3_probe = ProbeResult {
        name: "sqlite3 CLI".to_string(),
        path: sqlite_path.clone(),
        found: sqlite_found,
        version: sqlite_ver.clone(),
        details: if let Some(path) = &sqlite_path {
            Some(format!(
                "found at {} ({})",
                path.display(),
                sqlite_ver.as_deref().unwrap_or("unknown version")
            ))
        } else {
            Some("not found in PATH or standard system paths (required for store inspect/diagnostics on Windows)".to_string())
        },
    };

    // 4. Paths Audit
    let paths_audit = PathsAudit {
        home: platform::fs::home_dir(),
        config_root: platform::fs::config_dir_at(p.home()),
        base_store: p.base.clone(),
        profiles_dir: p.profiles.clone(),
        sample_user_data_dir: p.user_data_dir("sample-audit"),
    };

    // 5. Storage Audit
    let (shared_store_supported, shared_store_reason) =
        match capabilities.shared_store_compatibility {
            Support::Yes => (true, None),
            Support::No(reason) => (false, Some(reason.to_string())),
        };
    let storage_audit = StorageAudit {
        shared_store_supported,
        shared_store_reason,
        isolation_mode: !shared_store_supported,
    };

    // 6. Synthetic Isolation Audit
    let isolation_audit = run_synthetic_isolation_check().unwrap_or(IsolationAudit {
        codex_home_propagation: CheckStatus::Fail,
        user_data_dir_isolation: CheckStatus::Fail,
        credential_separation: CheckStatus::Fail,
        rollout_projection_isolation: CheckStatus::Fail,
        safe_refusal_unsupported_ops: CheckStatus::Fail,
        overall: CheckStatus::Fail,
    });

    // 7. Native Process Discovery Audit
    let discovery_supported = matches!(capabilities.native_process_discovery, Support::Yes);
    let (
        disc_status,
        scan_completed,
        proc_count,
        confirmed_count,
        candidate_count,
        conf_count,
        unk_count,
        restrictions,
    ) = if discovery_supported {
        match platform::process::scan_processes(p) {
            Ok(report) => {
                let proc_count = report.processes.len();
                let confirmed_count = report
                    .processes
                    .iter()
                    .filter(|proc| {
                        matches!(
                            proc.identity,
                            platform::process::ProcessIdentity::ConfirmedDesktop
                        )
                    })
                    .count();
                let candidate_count = report
                    .processes
                    .iter()
                    .filter(|proc| {
                        matches!(
                            proc.identity,
                            platform::process::ProcessIdentity::CandidateDesktop
                        )
                    })
                    .count();
                let conf_count = report
                    .processes
                    .iter()
                    .filter(|proc| {
                        matches!(
                            proc.attribution,
                            platform::process::ProfileAttribution::Known(_)
                        )
                    })
                    .count();
                let unk_count = report
                    .processes
                    .iter()
                    .filter(|proc| {
                        matches!(
                            proc.attribution,
                            platform::process::ProfileAttribution::Unknown
                        )
                    })
                    .count();
                (
                    CheckStatus::Pass,
                    report.is_complete,
                    proc_count,
                    confirmed_count,
                    candidate_count,
                    conf_count,
                    unk_count,
                    report.warnings,
                )
            }
            Err(e) => (
                CheckStatus::Fail,
                false,
                0,
                0,
                0,
                0,
                0,
                vec![format!("scan error: {e}")],
            ),
        }
    } else {
        (
            CheckStatus::Blocked("native process discovery unsupported on this platform"),
            false,
            0,
            0,
            0,
            0,
            0,
            Vec::new(),
        )
    };

    let process_discovery_audit = ProcessDiscoveryAudit {
        status: disc_status,
        method: platform::process::discovery_method().to_string(),
        scan_completed,
        running_processes_count: proc_count,
        confirmed_desktop_count: confirmed_count,
        candidate_desktop_count: candidate_count,
        confident_profile_count: conf_count,
        unknown_profile_count: unk_count,
        restrictions,
        synthetic_vs_real_note: "Synthetic tests verify native process discovery, helper-process filtering, and error handling; official proprietary Codex Desktop application launch and runtime behavior remain unverified and gated behind safe_desktop_launch.".to_string(),
    };

    // 8. Desktop Runtime Status
    let desktop_runtime_status = if desktop_found {
        if capabilities.safe_desktop_launch == Support::Yes {
            CheckStatus::Pass
        } else {
            CheckStatus::Blocked(
                "desktop binary found, but platform safe_desktop_launch capability is unsupported",
            )
        }
    } else {
        CheckStatus::Blocked("Codex Desktop binary not installed on this host")
    };

    PlatformAudit {
        os,
        arch,
        capabilities,
        desktop_probe,
        cli_probe,
        sqlite3_probe,
        paths_audit,
        storage_audit,
        isolation_audit,
        process_discovery_audit,
        desktop_runtime_status,
    }
}

/// Run an in-memory / temporary-directory synthetic isolation validation.
///
/// Validates:
/// - `CODEX_HOME` propagation.
/// - `--user-data-dir` separation across accounts.
/// - `auth.json` credential separation without sharing.
/// - Session rollout and projection isolation.
/// - Safe refusal of unsupported shared-store operations.
pub fn run_synthetic_isolation_check() -> Result<IsolationAudit> {
    let count = AUDIT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!(
        "cd-audit-iso-{}-{}-{}",
        std::process::id(),
        nonce,
        count
    ));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root)?;

    let base_dir = temp_root.join(".codex");
    let profiles_dir = temp_root.join(".codex-profiles");
    std::fs::create_dir_all(&base_dir)?;
    std::fs::create_dir_all(&profiles_dir)?;
    let p = Paths::new(&base_dir, &profiles_dir);

    // 1. Verify CODEX_HOME propagation
    let cmd_alpha = crate::windows::command_for(&p, "alpha");
    let env_home = cmd_alpha
        .env
        .iter()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| v.clone());
    let expected_home = p.profiles.join("alpha").display().to_string();
    let codex_home_ok = env_home == Some(expected_home);

    // 2. Verify --user-data-dir isolation
    let cmd_beta = crate::windows::command_for(&p, "beta");
    let flag_prefix = "--user-data-dir=";
    let alpha_arg = cmd_alpha.args.iter().find(|a| a.starts_with(flag_prefix));
    let beta_arg = cmd_beta.args.iter().find(|a| a.starts_with(flag_prefix));
    let user_data_ok = match (alpha_arg, beta_arg) {
        (Some(a), Some(b)) => a != b && a.contains("Codex-alpha") && b.contains("Codex-beta"),
        _ => false,
    };

    // 3. Verify Credential separation
    let add_alpha = crate::accounts::add_account(&p, "alpha", None);
    let add_beta = crate::accounts::add_account(&p, "beta", None);
    let mut cred_ok = add_alpha.is_ok() && add_beta.is_ok();
    if cred_ok {
        let auth_alpha = p.profiles.join("alpha").join("auth.json");
        let auth_beta = p.profiles.join("beta").join("auth.json");
        let _ = std::fs::write(&auth_alpha, r#"{"tokens":{"account_id":"user_alpha"}}"#);
        let _ = std::fs::write(&auth_beta, r#"{"tokens":{"account_id":"user_beta"}}"#);
        let bak_alpha = p.backup_dir(&format!("audit-test-alpha-{count}"));
        let bak_beta = p.backup_dir(&format!("audit-test-beta-{count}"));
        let _ = crate::accounts::materialise_auth(&p.profiles.join("alpha"), &bak_alpha);
        let _ = crate::accounts::materialise_auth(&p.profiles.join("beta"), &bak_beta);

        let (id_a, is_file_a) = match std::fs::symlink_metadata(&auth_alpha) {
            Ok(m) => (
                crate::accounts::auth_info(&auth_alpha).0,
                m.is_file() && !m.file_type().is_symlink(),
            ),
            Err(_) => (None, false),
        };
        let (id_b, is_file_b) = match std::fs::symlink_metadata(&auth_beta) {
            Ok(m) => (
                crate::accounts::auth_info(&auth_beta).0,
                m.is_file() && !m.file_type().is_symlink(),
            ),
            Err(_) => (None, false),
        };

        // Mutate alpha's credential file
        let _ = std::fs::write(
            &auth_alpha,
            r#"{"tokens":{"account_id":"user_alpha_MUTATED"}}"#,
        );
        let id_b_after = crate::accounts::auth_info(&auth_beta).0;

        cred_ok = is_file_a
            && is_file_b
            && id_a == Some("user_alpha".into())
            && id_b == Some("user_beta".into())
            && id_b_after == Some("user_beta".into());
    }

    // 4. Verify Rollout / Projection isolation
    let alpha_sessions = p.profiles.join("alpha").join("sessions");
    std::fs::create_dir_all(&alpha_sessions)?;
    let rollout_file = alpha_sessions.join("rollout-1.jsonl");
    std::fs::write(&rollout_file, "{\"type\":\"turn\",\"text\":\"test\"}\n")?;
    let proj_file = p.profiles.join("alpha").join("thread_history_1.sqlite");
    std::fs::write(&proj_file, "SYNTHETIC_SQLITE")?;

    let beta_sessions = p.profiles.join("beta").join("sessions");
    let rollout_leak = beta_sessions.join("rollout-1.jsonl").exists();
    let proj_leak = p
        .profiles
        .join("beta")
        .join("thread_history_1.sqlite")
        .exists();
    let base_rollout_leak = p.base.join("sessions").join("rollout-1.jsonl").exists();
    let rollout_ok = !rollout_leak && !proj_leak && !base_rollout_leak;

    // 5. Verify safe refusal of unsupported shared-store operations
    let caps = crate::platform::capabilities();
    let safe_refusal_ok = if let crate::platform::Support::No(_) = caps.shared_store_compatibility {
        crate::store::link_profile(&p, "alpha").is_err()
    } else {
        true
    };

    let _ = std::fs::remove_dir_all(&temp_root);

    let all_passed = codex_home_ok && user_data_ok && cred_ok && rollout_ok && safe_refusal_ok;

    Ok(IsolationAudit {
        codex_home_propagation: if codex_home_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        user_data_dir_isolation: if user_data_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        credential_separation: if cred_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        rollout_projection_isolation: if rollout_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        safe_refusal_unsupported_ops: if safe_refusal_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        overall: if all_passed {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
    })
}

/// Format the platform audit into a clean, sanitized text report.
pub fn format_audit_report(audit: &PlatformAudit) -> String {
    let mut out = String::new();
    out.push_str("=================================================================\n");
    out.push_str(" Codex Dock — Phase T1 Platform Runtime Compatibility Audit\n");
    out.push_str("=================================================================\n\n");

    out.push_str(&format!("OS Platform:    {} ({})\n", audit.os, audit.arch));
    out.push_str(&format!(
        "Storage Mode:   {}\n\n",
        if audit.storage_audit.isolation_mode {
            "Isolation Mode (profiles fully independent; shared store disabled)"
        } else {
            "Shared-Store Mode (symlinked SQLite database)"
        }
    ));

    out.push_str("--- Platform Capabilities ---\n");
    for (name, sup) in [
        (
            "filesystem symlinks",
            audit.capabilities.filesystem_symlinks,
        ),
        (
            "shared-store compatibility",
            audit.capabilities.shared_store_compatibility,
        ),
        (
            "native process discovery",
            audit.capabilities.native_process_discovery,
        ),
        ("window activation", audit.capabilities.window_activation),
        (
            "safe desktop launch",
            audit.capabilities.safe_desktop_launch,
        ),
    ] {
        match sup {
            Support::Yes => out.push_str(&format!("  {:<30} YES (supported)\n", name)),
            Support::No(reason) => out.push_str(&format!("  {:<30} NO ({reason})\n", name)),
        }
    }
    out.push('\n');

    out.push_str("--- Tool Discovery ---\n");
    for probe in [&audit.desktop_probe, &audit.cli_probe, &audit.sqlite3_probe] {
        out.push_str(&format!(
            "  {:<18} [{}] {}\n",
            probe.name,
            if probe.found { "FOUND" } else { "MISSING" },
            probe.details.as_deref().unwrap_or("none")
        ));
    }
    out.push('\n');

    out.push_str("--- Filesystem Paths ---\n");
    out.push_str(&format!(
        "  Home directory:     {}\n",
        audit.paths_audit.home.display()
    ));
    out.push_str(&format!(
        "  Config root:        {}\n",
        audit.paths_audit.config_root.display()
    ));
    out.push_str(&format!(
        "  Base directory:     {}\n",
        audit.paths_audit.base_store.display()
    ));
    out.push_str(&format!(
        "  Profiles directory: {}\n",
        audit.paths_audit.profiles_dir.display()
    ));
    out.push_str(&format!(
        "  User-data template: {}\n\n",
        audit.paths_audit.sample_user_data_dir.display()
    ));

    out.push_str("--- Synthetic Profile Isolation Check ---\n");
    out.push_str(&format!(
        "  CODEX_HOME propagation:          {}\n",
        audit.isolation_audit.codex_home_propagation.label()
    ));
    out.push_str(&format!(
        "  --user-data-dir isolation:       {}\n",
        audit.isolation_audit.user_data_dir_isolation.label()
    ));
    out.push_str(&format!(
        "  Credential separation:           {}\n",
        audit.isolation_audit.credential_separation.label()
    ));
    out.push_str(&format!(
        "  Rollout/projection isolation:    {}\n",
        audit.isolation_audit.rollout_projection_isolation.label()
    ));
    out.push_str(&format!(
        "  Safe refusal on unsupported:     {}\n",
        audit.isolation_audit.safe_refusal_unsupported_ops.label()
    ));
    out.push_str(&format!(
        "  Overall Isolation Status:        {}\n\n",
        audit.isolation_audit.overall.label()
    ));

    out.push_str("--- Native Process Discovery ---\n");
    out.push_str(&format!(
        "  Discovery Status:                {}\n",
        audit.process_discovery_audit.status.label()
    ));
    out.push_str(&format!(
        "  Discovery Method:                {}\n",
        audit.process_discovery_audit.method
    ));
    out.push_str(&format!(
        "  Scan Completion:                 {}\n",
        if audit.process_discovery_audit.scan_completed {
            "COMPLETE"
        } else {
            "INCOMPLETE (inspection denied or restricted)"
        }
    ));
    out.push_str(&format!(
        "  Running Codex Processes:         {} ({} confirmed desktop, {} candidate)\n",
        audit.process_discovery_audit.running_processes_count,
        audit.process_discovery_audit.confirmed_desktop_count,
        audit.process_discovery_audit.candidate_desktop_count
    ));
    out.push_str(&format!(
        "  Profile Association:             {} confident, {} unknown\n",
        audit.process_discovery_audit.confident_profile_count,
        audit.process_discovery_audit.unknown_profile_count
    ));
    if audit.process_discovery_audit.restrictions.is_empty() {
        out.push_str("  Restrictions:                    none\n");
    } else {
        out.push_str("  Restrictions:\n");
        for r in &audit.process_discovery_audit.restrictions {
            out.push_str(&format!("    - {r}\n"));
        }
    }
    out.push_str(&format!(
        "  Verification Scope:              {}\n\n",
        audit.process_discovery_audit.synthetic_vs_real_note
    ));

    out.push_str("--- Real Desktop Execution Status ---\n");
    out.push_str(&format!(
        "  Desktop Launch Status:           {}\n",
        audit.desktop_runtime_status.label()
    ));
    out.push_str("=================================================================\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_isolation_check_passes_on_current_host() {
        let res = run_synthetic_isolation_check().expect("isolation check should execute");
        assert!(
            res.overall.is_pass(),
            "synthetic isolation check must pass: {res:?}"
        );
        assert!(res.codex_home_propagation.is_pass());
        assert!(res.user_data_dir_isolation.is_pass());
        assert!(res.credential_separation.is_pass());
        assert!(res.rollout_projection_isolation.is_pass());
        assert!(res.safe_refusal_unsupported_ops.is_pass());
    }

    #[test]
    fn audit_report_formatting_is_sanitized_and_non_empty() {
        let p = Paths::real();
        let report = audit_platform(&p);
        let text = format_audit_report(&report);
        assert!(text.contains("Platform Runtime Compatibility Audit"));
        assert!(text.contains(&report.os));
        assert!(text.contains(&report.arch));
        assert!(text.contains("Overall Isolation Status"));
        // Never disclose private credentials
        assert!(!text.contains("OPENAI_API_KEY"));
        assert!(!text.contains("CODEX_API_KEY"));
        assert!(!text.contains("sensitive-provider-identifier"));
    }
}
