use codex_dock_core::tray;
use codex_dock_core::usage::{Color, Usage};

fn u(acct: &str, pct: Option<f64>, plan: Option<&str>) -> Usage {
    let mut u = Usage::empty(acct, 0);
    u.five_hour_percent = pct;
    u.plan = plan.map(|s| s.to_string());
    u
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

#[test]
fn age_label_reads_naturally() {
    assert_eq!(tray::age_label(1000, 1000), "just now");
    assert_eq!(tray::age_label(1000, 1030), "just now");
    assert_eq!(tray::age_label(1000, 1180), "3m ago");
    assert_eq!(tray::age_label(1000, 1000 + 7200), "2h ago");
    assert_eq!(tray::age_label(1000, 1000 + 172800), "2d ago");
}

#[test]
fn a_stale_row_says_when_it_was_last_checked() {
    let mut u = u("work", Some(55.0), Some("plus"));
    u.checked_at = 1000;
    u.stale = true;
    let rows = tray::menu_rows_at(&[u], 1000 + 300);
    assert!(rows[0].label.contains("55.0"), "keeps the real number");
    assert!(rows[0].label.contains("last checked 5m ago"), "got {}", rows[0].label);
}

#[test]
fn a_fresh_row_has_no_staleness_suffix() {
    let rows = tray::menu_rows_at(&[u("work", Some(55.0), Some("plus"))], 1000);
    assert!(!rows[0].label.contains("last checked"));
    assert!(!rows[0].label.contains("unavailable"));
}
