use codex_dock_core::usage::{self, Cache, Color, Usage};

fn stub(pct: Option<f64>) -> Usage {
    let mut u = Usage::empty("x", 0);
    u.five_hour_percent = pct;
    u
}

/// The exact shape `account/rateLimits/read` returned on this machine.
const RATE_LIMITS: &str = r#"{
  "ordinaryUsageAllowed": true,
  "rateLimits": {
    "limitId": "codex",
    "primary":   { "usedPercent": 55, "windowDurationMins": 300,   "resetsAt": 1793466378 },
    "secondary": { "usedPercent": 58, "windowDurationMins": 10080, "resetsAt": 1793796584 },
    "credits":   { "hasCredits": false, "unlimited": false, "balance": "0" },
    "planType": "plus"
  },
  "accountId": "9c5012f1-36d0-4810-85f7-6ce5427814bd"
}"#;

#[test]
fn parses_the_real_rate_limits_shape() {
    let u = usage::parse_rate_limits("work", RATE_LIMITS, 1000);
    assert_eq!(u.five_hour_percent, Some(55.0), "primary.usedPercent");
    assert_eq!(u.weekly_percent, Some(58.0), "secondary.usedPercent");
    assert_eq!(u.credits, Some(0.0), "credits.balance is a string");
    assert_eq!(u.plan.as_deref(), Some("plus"));
    assert_eq!(u.resets_at, Some(1793466378));
    assert_eq!(u.checked_at, 1000);
    assert!(u.error.is_none());
}

#[test]
fn missing_fields_stay_none_never_zero() {
    let u = usage::parse_rate_limits("work", r#"{"rateLimits":{"secondary":{"usedPercent":40}}}"#, 5);
    assert_eq!(u.five_hour_percent, None, "absent must not become 0");
    assert_eq!(u.credits, None);
    assert_eq!(u.plan, None);
}

#[test]
fn parses_the_usage_summary_shape() {
    let json = r#"{"summary":{"lifetimeTokens":755580136,"peakDailyTokens":128662242,
                   "longestRunningTurnSec":15868,"currentStreakDays":12,"longestStreakDays":12},
                   "dailyUsageBuckets":[{"startDate":"2026-09-20","tokens":828528}]}"#;
    let u = usage::parse_usage_summary("work", json, 1);
    assert_eq!(u.lifetime_tokens, Some(755_580_136));
    assert_eq!(u.peak_daily_tokens, Some(128_662_242));
    assert_eq!(u.current_streak_days, Some(12));
}

#[test]
fn parses_the_account_shape() {
    let json = r#"{"account":{"type":"chatgpt","email":"a@b.c","planType":"plus"},
                   "requiresOpenaiAuth":true}"#;
    assert_eq!(
        usage::parse_account("work", json, 1).plan.as_deref(),
        Some("plus")
    );
}

#[test]
fn malformed_json_yields_error_and_no_numbers() {
    let u = usage::parse_rate_limits("work", "not json", 7);
    assert!(u.error.is_some());
    assert_eq!(u.five_hour_percent, None);
    assert_eq!(u.checked_at, 7);
}

#[test]
fn worst_color_uses_the_highest_five_hour_percent() {
    assert_eq!(usage::worst_color(&[stub(Some(10.0)), stub(Some(70.0))]), Color::Amber);
    assert_eq!(usage::worst_color(&[stub(Some(10.0)), stub(Some(90.0))]), Color::Red);
    assert_eq!(usage::worst_color(&[stub(Some(10.0))]), Color::Green);
    assert_eq!(usage::worst_color(&[stub(None)]), Color::Neutral);
    assert_eq!(usage::worst_color(&[]), Color::Neutral);
}

#[test]
fn color_boundaries_are_exact() {
    assert_eq!(usage::worst_color(&[stub(Some(59.9))]), Color::Green);
    assert_eq!(usage::worst_color(&[stub(Some(60.0))]), Color::Amber);
    assert_eq!(usage::worst_color(&[stub(Some(84.9))]), Color::Amber);
    assert_eq!(usage::worst_color(&[stub(Some(85.0))]), Color::Red);
}

#[test]
fn read_parses_a_jsonl_transcript_end_to_end() {
    let transcript = concat!(
        r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex-dock/0.164.0"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"result":{"rateLimits":{"primary":{"usedPercent":42,"resetsAt":99},"planType":"pro"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"result":{"account":{"planType":"pro"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":4,"result":{"summary":{"lifetimeTokens":123}}}"#,
        "\n"
    );
    let fake = |_home: &std::path::Path| Ok(transcript.to_string());
    let u = usage::read_with("work", std::path::Path::new("/tmp"), 99, &fake);
    assert_eq!(u.five_hour_percent, Some(42.0));
    assert_eq!(u.plan.as_deref(), Some("pro"));
    assert_eq!(u.lifetime_tokens, Some(123));
    assert_eq!(u.checked_at, 99);
    assert!(u.error.is_none());
}

#[test]
fn read_with_reports_transport_failure_without_inventing_numbers() {
    let fake = |_h: &std::path::Path| Err("socket refused".to_string());
    let u = usage::read_with("work", std::path::Path::new("/tmp"), 5, &fake);
    assert!(u.error.as_deref().unwrap_or("").contains("socket refused"));
    assert_eq!(u.five_hour_percent, None);
    assert_eq!(u.weekly_percent, None);
}

#[test]
fn unreachable_app_server_says_how_to_fix_it() {
    let fake = |_h: &std::path::Path| Err("connect failed".to_string());
    let u = usage::read_with("work", std::path::Path::new("/tmp"), 1, &fake);
    let msg = u.error.unwrap_or_default();
    assert!(msg.contains("connect failed"), "keeps the cause: {msg}");
    assert!(
        msg.contains("open a Codex window") || msg.contains("daemon"),
        "must tell the user what to do, got: {msg}"
    );
}

#[test]
fn cache_serves_a_reading_until_it_expires() {
    let mut c = Cache::new(60);
    let mut u = stub(Some(55.0));
    u.account = "work".into();
    u.checked_at = 1000;
    c.put(u);
    assert!(c.get("work", 1030).is_some(), "still fresh at +30s");
    assert!(c.get("work", 1060).is_none(), "expired at +60s");
}

#[test]
fn cache_keeps_the_last_value_after_expiry() {
    let mut c = Cache::new(60);
    let mut u = stub(Some(55.0));
    u.account = "work".into();
    u.checked_at = 1000;
    c.put(u);
    assert!(c.get("work", 9999).is_none());
    assert_eq!(c.last("work").and_then(|u| u.five_hour_percent), Some(55.0));
}

#[test]
fn a_failed_refresh_retains_the_previous_numbers_and_marks_them_stale() {
    let mut prev = stub(Some(55.0));
    prev.account = "work".into();
    prev.weekly_percent = Some(58.0);
    prev.checked_at = 1000;

    let mut fresh = Usage::empty("work", 2000);
    fresh.error = Some("app-server unavailable".into());

    let kept = usage::retain_last(fresh, Some(&prev));
    assert_eq!(kept.five_hour_percent, Some(55.0), "must not fall back to 0");
    assert_eq!(kept.weekly_percent, Some(58.0));
    assert_eq!(kept.checked_at, 1000, "age is measured from the real reading");
    assert!(kept.stale);
    assert!(kept.error.is_some(), "the cause is still reported");
}

#[test]
fn a_good_refresh_replaces_the_retained_value() {
    let prev = stub(Some(55.0));
    let mut fresh = stub(Some(70.0));
    fresh.checked_at = 2000;
    let got = usage::retain_last(fresh, Some(&prev));
    assert_eq!(got.five_hour_percent, Some(70.0));
    assert!(!got.stale);
}

#[test]
fn retain_last_without_history_returns_the_fresh_row() {
    let mut fresh = Usage::empty("work", 5);
    fresh.error = Some("down".into());
    let got = usage::retain_last(fresh, None);
    assert_eq!(got.five_hour_percent, None);
    assert!(!got.stale);
}
