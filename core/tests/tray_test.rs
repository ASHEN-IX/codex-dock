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
    let title = tray::title(&[
        u("work", Some(10.0), Some("pro")),
        u("secondary", Some(20.0), None),
    ]);
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
    assert!(
        rows[0].label.contains("last checked 5m ago"),
        "got {}",
        rows[0].label
    );
}

#[test]
fn a_fresh_row_has_no_staleness_suffix() {
    let rows = tray::menu_rows_at(&[u("work", Some(55.0), Some("plus"))], 1000);
    assert!(!rows[0].label.contains("last checked"));
    assert!(!rows[0].label.contains("unavailable"));
}

#[test]
fn text_bar_fills_proportionally() {
    assert_eq!(tray::text_bar(Some(0.0), 10), "░░░░░░░░░░");
    assert_eq!(tray::text_bar(Some(50.0), 10), "▓▓▓▓▓░░░░░");
    assert_eq!(tray::text_bar(Some(100.0), 10), "▓▓▓▓▓▓▓▓▓▓");
    // Clamped, never longer than the track.
    assert_eq!(tray::text_bar(Some(140.0), 10).chars().count(), 10);
}

#[test]
fn text_bar_without_a_reading_is_all_empty() {
    let bar = tray::text_bar(None, 10);
    assert_eq!(bar, "░░░░░░░░░░");
    assert!(!bar.contains('▓'), "must not imply a value");
}

#[test]
fn reset_label_counts_down_naturally() {
    assert_eq!(
        tray::reset_label(Some(1000 + 2 * 3600 + 15 * 60), 1000).as_deref(),
        Some("resets in 2h 15m")
    );
    assert_eq!(
        tray::reset_label(Some(1000 + 45 * 60), 1000).as_deref(),
        Some("resets in 45m")
    );
    assert_eq!(
        tray::reset_label(Some(1000 + 86400 + 3600), 1000).as_deref(),
        Some("resets in 1d 1h")
    );
    assert_eq!(
        tray::reset_label(Some(900), 1000).as_deref(),
        Some("resetting now")
    );
    assert_eq!(tray::reset_label(None, 1000), None);
}

#[test]
fn gauge_is_a_ring_not_a_filled_disc() {
    let icon = tray::gauge_icon(Some(50.0), Color::Green, 32);
    assert_eq!((icon.width, icon.height), (32, 32));
    assert_eq!(icon.rgba.len(), 32 * 32 * 4);
    // The centre is hollow, the ring edge is drawn.
    assert_eq!(icon.alpha_at(16, 16), 0, "centre must be transparent");
    assert!(icon.alpha_at(16, 1) > 200, "top of the ring must be opaque");
}

#[test]
fn gauge_fill_tracks_the_percentage() {
    // 12 o'clock is the arc start, so a small fill colours the top but not the
    // bottom; a full one colours both.
    let low = tray::gauge_icon(Some(10.0), Color::Green, 32);
    let full = tray::gauge_icon(Some(100.0), Color::Green, 32);
    let top_alpha = low.alpha_at(16, 1);
    let bottom_alpha = low.alpha_at(16, 30);
    assert!(top_alpha > 0, "top of a 10% ring is filled");
    assert!(bottom_alpha > 0, "the dim track is still drawn");

    // Compare colour saturation of the track vs the filled arc: the filled arc
    // is the pure colour, the track is washed out toward grey.
    let idx = |x: u32, y: u32| ((y * 32 + x) * 4) as usize;
    let top_r = full.rgba[idx(16, 1)];
    let low_top_r = low.rgba[idx(16, 1)];
    assert_eq!(top_r, low_top_r, "same colour where both are filled");
    let full_bottom_r = full.rgba[idx(16, 30)];
    let low_bottom_r = low.rgba[idx(16, 30)];
    assert!(
        full_bottom_r < low_bottom_r,
        "a full ring draws its own colour at the bottom ({full_bottom_r}) \
         where a 10% ring draws the washed-out track ({low_bottom_r})"
    );
}

#[test]
fn gauge_with_no_reading_draws_only_the_dim_track() {
    let icon = tray::gauge_icon(None, Color::Neutral, 32);
    let idx = |x: u32, y: u32| ((y * 32 + x) * 4) as usize;
    let top = icon.rgba[idx(16, 1)];
    let bottom = icon.rgba[idx(16, 30)];
    assert_eq!(top, bottom, "an unknown percentage fills nothing");
}

#[test]
fn icon_for_uses_the_worst_percentage_and_colour() {
    let usages = vec![u("a", Some(10.0), None), u("b", Some(90.0), None)];
    assert_eq!(tray::worst_percent(&usages), Some(90.0));
    let icon = tray::icon_for(&usages, 32);
    assert_eq!(icon.rgba.len(), 32 * 32 * 4);
    // 90% is red, so the filled arc is the red channel's 0xf8.
    let idx = ((32 + 16) * 4) as usize;
    assert_eq!(icon.rgba[idx], 0xf8, "worst account drives the colour");
}

#[test]
fn menu_groups_each_account_over_several_lines() {
    let mut w = u("work", Some(55.0), Some("plus"));
    w.weekly_percent = Some(58.0);
    w.resets_at = Some(1000 + 3600);
    let entries = tray::menu_entries(&[w], 1000);

    let labelled: Vec<_> = entries
        .iter()
        .filter(|e| !e.label.trim().is_empty())
        .collect();
    assert!(
        labelled.len() >= 3,
        "header, name and numbers each get their own line: {labelled:?}"
    );
    let lines: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == tray::RowKind::Account)
        .collect();
    assert_eq!(lines.len(), 1, "only the name line is an account action");

    let name_line = &lines[0].label;
    assert!(name_line.contains("work"), "got {name_line}");
    assert!(name_line.contains("plus"));
    assert!(
        !name_line.contains("55.0"),
        "numbers are not crammed into the name line"
    );

    let gauge_line = entries
        .iter()
        .find(|e| e.label.contains("5h "))
        .map(|e| e.label.clone())
        .expect("a gauge line");
    assert!(gauge_line.contains("▓"), "has a text gauge: {gauge_line}");
    assert!(gauge_line.contains("55.0"));
    assert!(gauge_line.contains("58.0"));

    // Exactly one row per account opens it; the numbers must not, or a
    // signed-out account could be opened through its gauge line.
    let openable: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == tray::RowKind::Account)
        .collect();
    assert_eq!(openable.len(), 1, "one clickable row per account");
    assert_eq!(openable[0].account, "work");
    assert!(openable[0].enabled);
}

#[test]
fn menu_starts_with_a_header_and_has_a_separator() {
    let entries = tray::menu_entries(&[u("work", Some(10.0), None)], 1000);
    assert_eq!(entries[0].kind, tray::RowKind::Header);
    assert!(!entries[0].enabled, "headers are inert");
    assert!(entries.iter().any(|e| e.kind == tray::RowKind::Separator));
}

#[test]
fn menu_marks_stale_accounts_in_their_header() {
    let mut w = u("work", Some(55.0), Some("plus"));
    w.stale = true;
    w.checked_at = 1000;
    let entries = tray::menu_entries(&[w], 1000 + 300);
    let name_line = entries.iter().find(|e| e.label.contains("work")).unwrap();
    assert!(name_line.label.contains("stale"), "got {}", name_line.label);
    let detail = entries
        .iter()
        .find(|e| e.label.contains("last checked"))
        .expect("stale rows say how old they are");
    assert!(detail.label.contains("5m ago"));
}

#[test]
fn empty_account_list_says_so() {
    let entries = tray::menu_entries(&[], 1000);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].label.to_lowercase().contains("no accounts"));
}

#[test]
fn a_signed_out_account_is_listed_but_not_openable() {
    let entries = tray::menu_entries_with(
        &[u("fresh", None, None), u("work", Some(10.0), Some("plus"))],
        1000,
        &|name| name == "work",
    );
    let fresh = entries
        .iter()
        .find(|e| e.account == "fresh")
        .expect("listed");
    assert!(
        !fresh.enabled,
        "opening it would only show Codex's sign-in screen"
    );
    assert!(fresh.label.contains("sign in"), "got {}", fresh.label);

    let work = entries
        .iter()
        .find(|e| e.account == "work")
        .expect("listed");
    assert!(work.enabled);
    assert!(!work.label.contains("sign in"), "got {}", work.label);
}

#[test]
fn a_signed_out_account_never_reads_as_zero_percent() {
    let entries = tray::menu_entries_with(&[u("fresh", None, None)], 1000, &|_| false);
    let text: String = entries.iter().map(|e| e.label.clone()).collect();
    assert!(!text.contains("0%"), "got {text}");
}
