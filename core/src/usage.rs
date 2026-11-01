use crate::paths::Paths;
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

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
///
/// `Shared` behind an `Arc` because the lock must never be held across a read:
/// one app-server round trip takes seconds, and a refresh of three accounts
/// would freeze the window and the tray tick for the whole time.
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

/// A cache that can be shared across threads, and the single place a reading is
/// refreshed. Holding the lock only to look up or store a value keeps the UI
/// responsive while an account is slow to answer.
#[derive(Debug)]
pub struct Shared {
    inner: Mutex<Cache>,
    /// Serialises refreshes of one account so the window and the tray tick
    /// cannot both spawn an app-server for it.
    refreshes: Mutex<HashMap<String, Arc<(Mutex<bool>, Condvar)>>>,
}

impl Shared {
    pub fn new(ttl: i64) -> Shared {
        Shared { inner: Mutex::new(Cache::new(ttl)), refreshes: Mutex::new(HashMap::new()) }
    }

    /// A reading from the last `ttl` seconds, if one was recorded.
    pub fn get(&self, account: &str, now: i64) -> Option<Usage> {
        self.lock().get(account, now).cloned()
    }

    /// The last reading of any age, fresh or retained.
    pub fn last(&self, account: &str) -> Option<Usage> {
        self.lock().last(account).cloned()
    }

    /// Serve from the cache when fresh, otherwise read and store. Concurrent
    /// callers for the same account wait for the first one instead of each
    /// spawning their own app-server.
    pub fn get_or_read(&self, p: &Paths, account: &str, now_now: impl Fn() -> i64) -> Usage {
        let now = now_now();
        if let Some(hit) = self.get(account, now) {
            return hit;
        }
        let gate = {
            let mut map = self.refreshes.lock().unwrap_or_else(|e| e.into_inner());
            Arc::clone(map.entry(account.to_string()).or_default())
        };
        let (busy, cv) = &*gate;
        let mut started = busy.lock().unwrap_or_else(|e| e.into_inner());
        while *started {
            // Another thread is already reading: wait, then re-check the cache.
            let (guard, _) = cv
                .wait_timeout(started, Duration::from_secs(30))
                .unwrap_or_else(|e| e.into_inner());
            started = guard;
            if !*started {
                break;
            }
        }
        let fresh_now = now_now();
        if let Some(hit) = self.get(account, fresh_now) {
            return hit;
        }
        *started = true;
        drop(started);

        let previous = self.last(account);
        let fresh = read(p, account, fresh_now);
        let kept = retain_last(fresh, previous.as_ref());
        self.lock().put(kept.clone());

        {
            let mut started = busy.lock().unwrap_or_else(|e| e.into_inner());
            *started = false;
        }
        cv.notify_all();
        kept
    }

    /// Drop an account's cached reading so the next refresh is a real one.
    pub fn forget(&self, account: &str) {
        self.lock().entries.remove(account);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
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
    use std::io::Write;

    let mut child = Command::new(crate::tool::codex())
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
    let transcript = match read_replies(&mut child, &[2, 3, 4], REPLY_TIMEOUT) {
        Ok(t) => t,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let _ = child.kill();
    let _ = child.wait();

    if transcript.trim().is_empty() {
        return Err("app-server returned no output".to_string());
    }
    Ok(transcript)
}

/// How long to wait for the replies. `account/usage/read` is the slowest and
/// genuinely optional: a timeout still leaves the rate-limit numbers usable.
const REPLY_TIMEOUT: Duration = Duration::from_secs(25);

/// Read stdout on a worker thread while watching the clock, so a wedged
/// app-server cannot hang the caller (and, with it, the tray tick) forever.
fn read_replies(
    child: &mut Child,
    want: &[u64],
    timeout: Duration,
) -> std::result::Result<String, String> {
    use std::io::{BufRead, BufReader};

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(line.clone()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let mut transcript = String::new();
    let mut seen: Vec<u64> = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            // Keep whatever arrived: rate limits may be complete even when the
            // slower usage summary is not.
            if transcript.trim().is_empty() {
                return Err("app-server did not answer in time".to_string());
            }
            return Ok(transcript);
        }
        let line = match rx.recv_timeout(left) {
            Ok(l) => l,
            Err(_) => continue,
        };
        transcript.push_str(&line);
        for id in want {
            if !seen.contains(id) && line.contains(&format!("\"id\":{id}")) {
                seen.push(*id);
            }
        }
        if want.iter().all(|id| seen.contains(id)) {
            return Ok(transcript);
        }
    }
}
