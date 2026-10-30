use codex_dock_core::tray;
use codex_dock_core::usage::{Color, Usage};

fn u(acct: &str, pct: Option<f64>, plan: Option<&str>) -> Usage {
    Usage {
        account: acct.into(),
        five_hour_percent: pct,
        weekly_percent: None,
        credits: None,
        plan: plan.map(|s| s.to_string()),
        checked_at: 0,
        error: None,
    }
}

#[test]
fn tray_title_names_running_accounts() {
    let title = tray::title(&[u("work", Some(10.0), Some("pro")), u("secondary", Some(20.0), None)]);
    assert!(title.contains("2"), "should show the count: {title}");
}

#[test]
fn menu_lists_every_account_with_its_percent() {
    let rows = tray::menu_rows(&[u("work", Some(87.5), Some("pro"))]);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].label.contains("work"));
    assert!(rows[0].label.contains("87.5"), "got {}", rows[0].label);
    assert!(rows[0].label.contains("pro"));
}

#[test]
fn menu_marks_a_missing_reading_as_unavailable_not_zero() {
    let rows = tray::menu_rows(&[u("work", None, None)]);
    assert!(rows[0].label.contains("—"), "got {}", rows[0].label);
    assert!(!rows[0].label.contains("0%"));
}

#[test]
fn icon_name_matches_the_worst_colour() {
    assert_eq!(tray::icon_name(Color::Green), "tray-green.png");
    assert_eq!(tray::icon_name(Color::Amber), "tray-amber.png");
    assert_eq!(tray::icon_name(Color::Red), "tray-red.png");
    assert_eq!(tray::icon_name(Color::Neutral), "tray-neutral.png");
}
