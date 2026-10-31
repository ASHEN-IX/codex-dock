use crate::paths::Paths;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub account: String,
    /// 5-hour window used percent (`rateLimits.primary.usedPercent`).
    pub five_hour_percent: Option<f64>,
    /// Weekly window used percent (`rateLimits.secondary.usedPercent`).
    pub weekly_percent: Option<f64>,
    /// Credit balance. `None` when the plan does not meter credits.
    pub credits: Option<f64>,
    pub plan: Option<String>,
    pub checked_at: i64,
    pub error: Option<String>,
    /// When the 5-hour window resets (unix seconds).
    pub resets_at: Option<i64>,
    pub lifetime_tokens: Option<i64>,
    pub peak_daily_tokens: Option<i64>,
    pub current_streak_days: Option<i64>,
    /// True when this row is a retained previous reading, not a fresh one.
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Green,
    Amber,
    Red,
    Neutral,
}

impl Usage {
    pub fn empty(account: &str, now: i64) -> Usage {
        Usage {
            account: account.to_string(),
            five_hour_percent: None,
            weekly_percent: None,
            credits: None,
            plan: None,
            checked_at: now,
            error: None,
            resets_at: None,
            lifetime_tokens: None,
            peak_daily_tokens: None,
            current_streak_days: None,
            stale: false,
        }
    }
}

fn num(v: &serde_json::Value, path: &[&str]) -> Option<f64> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    match cur {
        serde_json::Value::Number(n) => n.as_f64(),
        // `credits.balance` is a decimal string in the wire format.
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn int(v: &serde_json::Value, path: &[&str]) -> Option<i64> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_i64()
}

fn text(v: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_str().map(|s| s.to_string())
}

/// Parse the `result` of `account/rateLimits/read`.
///
/// Shape verified live (codex 0.164.0):
/// `{ rateLimits: { primary: { usedPercent, windowDurationMins, resetsAt },
///                  secondary: { usedPercent, ... },
///                  credits: { hasCredits, balance }, planType } }`
pub fn parse_rate_limits(account: &str, json: &str, now: i64) -> Usage {
    let mut u = Usage::empty(account, now);
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            u.error = Some(format!("could not parse usage response: {e}"));
            return u;
        }
    };
    let rl = v.get("rateLimits").unwrap_or(&v);
    u.five_hour_percent = num(rl, &["primary", "usedPercent"]);
    u.weekly_percent = num(rl, &["secondary", "usedPercent"]);
    u.credits = num(rl, &["credits", "balance"]);
    u.plan = text(rl, &["planType"]);
    u.resets_at = int(rl, &["primary", "resetsAt"]);
    u
}

/// Parse the `result` of `account/read`; only the plan is used here, and only
/// when rate limits did not supply one.
pub fn parse_account(account: &str, json: &str, now: i64) -> Usage {
    let mut u = Usage::empty(account, now);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        u.plan = text(&v, &["account", "planType"]);
    }
    u
}

/// Parse the `result` of `account/usage/read`.
///
/// Shape verified live: `{ summary: { lifetimeTokens, peakDailyTokens,
/// currentStreakDays, ... }, dailyUsageBuckets: [...] }`
pub fn parse_usage_summary(account: &str, json: &str, now: i64) -> Usage {
    let mut u = Usage::empty(account, now);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        u.lifetime_tokens = int(&v, &["summary", "lifetimeTokens"]);
        u.peak_daily_tokens = int(&v, &["summary", "peakDailyTokens"]);
        u.current_streak_days = int(&v, &["summary", "currentStreakDays"]);
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

/// Keep the last good numbers when a refresh fails, and mark them stale.
/// The spec forbids rendering a fabricated 0%; a retained value with its age
/// is the honest alternative.
pub fn retain_last(fresh: Usage, previous: Option<&Usage>) -> Usage {
    let Some(prev) = previous else { return fresh };
    if fresh.error.is_none() && fresh.five_hour_percent.is_some() {
        return fresh;
    }
    if prev.five_hour_percent.is_none() && prev.weekly_percent.is_none() {
        return fresh;
    }
    Usage {
        account: fresh.account.clone(),
        five_hour_percent: fresh.five_hour_percent.or(prev.five_hour_percent),
        weekly_percent: fresh.weekly_percent.or(prev.weekly_percent),
        credits: fresh.credits.or(prev.credits),
        plan: fresh.plan.clone().or_else(|| prev.plan.clone()),
        checked_at: prev.checked_at,
        error: fresh.error.clone(),
        resets_at: fresh.resets_at.or(prev.resets_at),
        lifetime_tokens: fresh.lifetime_tokens.or(prev.lifetime_tokens),
        peak_daily_tokens: fresh.peak_daily_tokens.or(prev.peak_daily_tokens),
        current_streak_days: fresh.current_streak_days.or(prev.current_streak_days),
        stale: true,
    }
}

/// A 60s cache shared by the window and the tray so they cannot disagree.
#[derive(Debug)]
pub struct Cache {
    ttl: i64,
    entries: HashMap<String, Usage>,
}

impl Cache {
    pub fn new(ttl: i64) -> Cache {
        Cache { ttl, entries: HashMap::new() }
    }

    pub fn get(&self, account: &str, now: i64) -> Option<&Usage> {
        self.entries
            .get(account)
            .filter(|u| now.saturating_sub(u.checked_at) < self.ttl)
    }

    pub fn put(&mut self, u: Usage) {
        self.entries.insert(u.account.clone(), u);
    }

    pub fn last(&self, account: &str) -> Option<&Usage> {
        self.entries.get(account)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Transport seam: fetch every reply needed for one account in a single
/// app-server session, returning the raw newline-delimited transcript.
pub type Transport<'a> =
    dyn Fn(&std::path::Path) -> std::result::Result<String, String> + 'a;

pub fn read_with(
    account: &str,
    home: &std::path::Path,
    now: i64,
    transport: &Transport<'_>,
) -> Usage {
    let raw = match transport(home) {
        Ok(r) => r,
        Err(e) => {
            let mut u = Usage::empty(account, now);
            u.error = Some(format!(
                "app-server unavailable ({e}) — open a Codex window for this account, or run \
                 `codex app-server daemon start` with CODEX_HOME set to its profile"
            ));
            return u;
        }
    };

    // Ids are fixed by `proxy_request`: 2 rate limits, 3 account, 4 usage.
    let mut u = match crate::rpc::decode_response(&raw, 2) {
        Ok(v) => parse_rate_limits(account, &v.to_string(), now),
        Err(e) => {
            let mut u = Usage::empty(account, now);
            u.error = Some(e.to_string());
            return u;
        }
    };
    if let Ok(v) = crate::rpc::decode_response(&raw, 3) {
        if u.plan.is_none() {
            u.plan = parse_account(account, &v.to_string(), now).plan;
        }
    }
    if let Ok(v) = crate::rpc::decode_response(&raw, 4) {
        let s = parse_usage_summary(account, &v.to_string(), now);
        u.lifetime_tokens = s.lifetime_tokens;
        u.peak_daily_tokens = s.peak_daily_tokens;
        u.current_streak_days = s.current_streak_days;
    }
    u
}

pub fn read(p: &Paths, account: &str, now: i64) -> Usage {
    let home = p.profiles.join(account);
    let transport = |home: &std::path::Path| -> std::result::Result<String, String> {
        proxy_request(home)
    };
    read_with(account, &home, now, &transport)
}

/// One `codex app-server` session: initialize, then the three reads. The
/// server speaks newline-delimited JSON on stdio, which is the transport the
/// Codex clients themselves use.
fn proxy_request(home: &std::path::Path) -> std::result::Result<String, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let mut child = Command::new("codex")
        .args(["app-server"])
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not run codex app-server: {e}"))?;

    {
        let stdin = child.stdin.as_mut().ok_or("no stdin")?;
        let init = crate::rpc::encode_request_with(
            1,
            "initialize",
            r#"{"clientInfo":{"name":"codex-dock","version":"0.1.0"}}"#,
        );
        stdin
            .write_all(init.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;
        stdin
            .write_all(crate::rpc::encode_notification("initialized").as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;
        for (id, method) in [
            (2u64, "account/rateLimits/read"),
            (3, "account/read"),
            (4, "account/usage/read"),
        ] {
            stdin
                .write_all(crate::rpc::encode_request(id, method).as_bytes())
                .map_err(|e| format!("write failed: {e}"))?;
        }
        stdin.flush().ok();
    }

    // Read until every id we asked for has been seen, so a slow first reply
    // cannot truncate the rest. `usage/read` is the slowest and optional.
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut transcript = String::new();
    let mut seen = [false; 5];
    let mut line = String::new();
    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            break;
        }
        transcript.push_str(&line);
        for id in 2..=4usize {
            if !seen[id] && line.contains(&format!("\"id\":{id}")) {
                seen[id] = true;
            }
        }
        line.clear();
        if seen[2] && seen[3] && seen[4] {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();

    if transcript.trim().is_empty() {
        return Err("app-server returned no output".to_string());
    }
    Ok(transcript)
}
