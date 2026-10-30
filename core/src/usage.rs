use crate::paths::Paths;

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub account: String,
    pub five_hour_percent: Option<f64>,
    pub weekly_percent: Option<f64>,
    pub credits: Option<f64>,
    pub plan: Option<String>,
    pub checked_at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Green,
    Amber,
    Red,
    Neutral,
}

fn num(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

pub fn parse_rate_limits(account: &str, json: &str, now: i64) -> Usage {
    let mut u = Usage {
        account: account.to_string(),
        five_hour_percent: None,
        weekly_percent: None,
        credits: None,
        plan: None,
        checked_at: now,
        error: None,
    };
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(v) => {
            u.five_hour_percent = num(&v, "five_hour_limit_percent");
            u.weekly_percent = num(&v, "weekly_limit_percent");
            u.credits = num(&v, "balance_usage_credits");
            u.plan = v.get("plan_type").and_then(|p| p.as_str()).map(|s| s.to_string());
        }
        Err(e) => u.error = Some(format!("could not parse usage response: {e}")),
    }
    u
}

pub fn worst_color(usages: &[Usage]) -> Color {
    let worst = usages
        .iter()
        .filter_map(|u| u.five_hour_percent)
        .fold(None::<f64>, |acc, p| Some(acc.map_or(p, |a| a.max(p))));
    match worst {
        None => Color::Neutral,
        Some(p) if p < 60.0 => Color::Green,
        Some(p) if p < 85.0 => Color::Amber,
        Some(_) => Color::Red,
    }
}

/// Transport seam: given the profile home and method, return the raw framed
/// bytes from the app-server, or a transport error string.
pub type Transport<'a> = dyn Fn(&std::path::Path, &str) -> std::result::Result<String, String> + 'a;

pub fn read_with(account: &str, home: &std::path::Path, now: i64, transport: &Transport<'_>) -> Usage {
    let raw = match transport(home, "account/rateLimits/read") {
        Ok(r) => r,
        Err(e) => {
            let mut u = empty(account, now);
            u.error = Some(format!("app-server unavailable: {e}"));
            return u;
        }
    };
    match crate::rpc::decode_response(&raw, 2) {
        Ok(v) => parse_rate_limits(account, &v.to_string(), now),
        Err(e) => {
            let mut u = empty(account, now);
            u.error = Some(e.to_string());
            u
        }
    }
}

pub fn read(p: &Paths, account: &str, now: i64) -> Usage {
    let home = p.profiles.join(account);
    let transport = |home: &std::path::Path, method: &str| -> std::result::Result<String, String> {
        proxy_request(home, method)
    };
    read_with(account, &home, now, &transport)
}

fn proxy_request(home: &std::path::Path, method: &str) -> std::result::Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let sock = home.join("ipc").join("ipc.sock");
    let mut child = Command::new("codex")
        .args(["app-server", "proxy", "--sock"])
        .arg(&sock)
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run codex app-server proxy: {e}"))?;

    {
        let stdin = child.stdin.as_mut().ok_or("no stdin")?;
        let init = crate::rpc::encode_request(1, "initialize");
        let call = crate::rpc::encode_request(2, method);
        stdin
            .write_all(format!("{init}{call}").as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;
    }

    let out = child.wait_with_output().map_err(|e| format!("wait failed: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if text.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(err.trim().lines().last().unwrap_or("no output").to_string());
    }
    Ok(text)
}

fn empty(account: &str, now: i64) -> Usage {
    Usage {
        account: account.to_string(),
        five_hour_percent: None,
        weekly_percent: None,
        credits: None,
        plan: None,
        checked_at: now,
        error: None,
    }
}
