use codex_dock_core::usage::{self, Color, Usage};

fn stub(pct: Option<f64>) -> Usage {
    Usage {
        account: "x".into(),
        five_hour_percent: pct,
        weekly_percent: None,
        credits: None,
        plan: None,
        checked_at: 0,
        error: None,
    }
}

#[test]
fn parses_rate_limits_response() {
    let json = r#"{"five_hour_limit_percent":87.5,"weekly_limit_percent":40,
                   "balance_usage_credits":12.5,"plan_type":"pro"}"#;
    let u = usage::parse_rate_limits("work", json, 1000);
    assert_eq!(u.five_hour_percent, Some(87.5));
    assert_eq!(u.weekly_percent, Some(40.0));
    assert_eq!(u.credits, Some(12.5));
    assert_eq!(u.plan.as_deref(), Some("pro"));
    assert_eq!(u.checked_at, 1000);
    assert!(u.error.is_none());
}

#[test]
fn missing_fields_stay_none_never_zero() {
    let u = usage::parse_rate_limits("work", r#"{"weekly_limit_percent":40}"#, 5);
    assert_eq!(u.five_hour_percent, None, "absent must not become 0");
    assert_eq!(u.credits, None);
    assert_eq!(u.plan, None);
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
