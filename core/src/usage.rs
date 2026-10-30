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

pub fn read(p: &Paths, account: &str, now: i64) -> Usage {
    let home = p.profiles.join(account);
    let out = std::process::Command::new("codex")
        .args(["app-server", "proxy", "account/rateLimits/read"])
        .env("CODEX_HOME", &home)
        .output();

    match out {
        Ok(o) if o.status.success() => {
            parse_rate_limits(account, &String::from_utf8_lossy(&o.stdout), now)
        }
        Ok(o) => {
            let mut u = empty(account, now);
            u.error = Some(format!(
                "app-server unavailable: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ));
            u
        }
        Err(e) => {
            let mut u = empty(account, now);
            u.error = Some(format!("could not run codex app-server: {e}"));
            u
        }
    }
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
