use crate::usage::{worst_color, Color, Usage};

#[derive(Debug, Clone, PartialEq)]
pub struct MenuRow {
    pub label: String,
    pub account: String,
}

/// What a menu row is for. The shell renders each kind differently: headers
/// and separators are inert, the rest are clickable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Header,
    Account,
    Separator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuEntry {
    pub label: String,
    /// Empty for rows that are not clickable.
    pub account: String,
    pub kind: RowKind,
    pub enabled: bool,
}

pub fn title(usages: &[Usage]) -> String {
    let n = usages.len();
    if n == 0 {
        return "Codex Dock — no accounts".to_string();
    }
    let running = usages.iter().filter(|u| u.error.is_none()).count();
    format!("Codex Dock — {n} accounts, {running} reporting")
}

fn pct(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.1}%"),
        None => "—".to_string(),
    }
}

/// How old a reading is, for the "last checked Nm ago" suffix.
pub fn age_label(checked_at: i64, now: i64) -> String {
    let secs = now.saturating_sub(checked_at).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

pub fn menu_rows(usages: &[Usage]) -> Vec<MenuRow> {
    usages
        .iter()
        .map(|u| {
            let plan = u.plan.as_deref().unwrap_or("—");
            MenuRow {
                label: format!(
                    "{} · {} 5h {} · {}",
                    u.account,
                    plan,
                    pct(u.five_hour_percent),
                    pct(u.weekly_percent)
                ),
                account: u.account.clone(),
            }
        })
        .collect()
}

/// Menu rows that say how stale a retained reading is, so the user is never
/// shown an old number as if it were current.
pub fn menu_rows_at(usages: &[Usage], now: i64) -> Vec<MenuRow> {
    usages
        .iter()
        .map(|u| {
            let plan = u.plan.as_deref().unwrap_or("—");
            let mut label = format!(
                "{} · {} 5h {} · {}",
                u.account,
                plan,
                pct(u.five_hour_percent),
                pct(u.weekly_percent)
            );
            if u.stale {
                label.push_str(&format!(" · last checked {}", age_label(u.checked_at, now)));
            } else if u.error.is_some() {
                label.push_str(" · unavailable");
            }
            MenuRow {
                label,
                account: u.account.clone(),
            }
        })
        .collect()
}

pub fn icon_name(color: Color) -> &'static str {
    match color {
        Color::Green => "tray-green.png",
        Color::Amber => "tray-amber.png",
        Color::Red => "tray-red.png",
        Color::Neutral => "tray-neutral.png",
    }
}

/// An RGBA image, row-major, ready for the tray.
#[derive(Debug, Clone, PartialEq)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Icon {
    /// Alpha at a pixel, for tests that check the ring is drawn where expected.
    pub fn alpha_at(&self, x: u32, y: u32) -> u8 {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i + 3]
    }
}

pub fn color_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Green => (0x3f, 0xb9, 0x50),
        Color::Amber => (0xd2, 0x99, 0x22),
        Color::Red => (0xf8, 0x51, 0x49),
        Color::Neutral => (0x8b, 0x94, 0x9e),
    }
}

/// Draw a usage gauge: a ring whose filled arc is the worst 5-hour percentage,
/// in the worst account's colour. A flat dot carries no information; the ring
/// lets you read "how close to the limit" from the panel without opening the
/// menu.
///
/// The arc starts at 12 o'clock and runs clockwise. The unfilled remainder is
/// drawn dim so the gauge stays legible on both light and dark panels.
pub fn gauge_icon(percent: Option<f64>, color: Color, size: u32) -> Icon {
    let (r, g, b) = color_rgb(color);
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let c = (size as f64 - 1.0) / 2.0;
    let outer = size as f64 / 2.0 - 0.5;
    let inner = (outer - (size as f64 * 0.19).max(2.0)).max(1.0);

    // The filled fraction. Neutral (no reading) draws the ring entirely dim so
    // it never implies a real percentage.
    let frac = match (percent, color) {
        (Some(p), _) => (p / 100.0).clamp(0.0, 1.0),
        (None, _) => 0.0,
    };

    // 2x2 supersampling keeps the ring smooth at 22px.
    let samples = 4.0;
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f64; 3];
            let mut cov = 0f64;
            for sy in 0..2 {
                for sx in 0..2 {
                    let px = x as f64 + (sx as f64 + 0.5) / 2.0;
                    let py = y as f64 + (sy as f64 + 0.5) / 2.0;
                    let dx = px - c;
                    let dy = py - c;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist > outer || dist < inner {
                        continue;
                    }
                    // Angle from 12 o'clock, clockwise, as a 0..1 fraction.
                    let mut a = dx.atan2(-dy);
                    if a < 0.0 {
                        a += std::f64::consts::TAU;
                    }
                    let pos = a / std::f64::consts::TAU;
                    let on = pos <= frac;
                    let (cr, cg, cb) = if on {
                        (r as f64, g as f64, b as f64)
                    } else {
                        // A neutral track. Tinting it with the fill colour made
                        // the ring read as two shades of the same colour
                        // instead of "this much is used".
                        (0x4a as f64, 0x4a as f64, 0x4a as f64)
                    };
                    acc[0] += cr;
                    acc[1] += cg;
                    acc[2] += cb;
                    cov += 1.0;
                }
            }
            if cov > 0.0 {
                let alpha = (cov / samples * 255.0).round() as u8;
                let i = ((y * size + x) * 4) as usize;
                rgba[i] = (acc[0] / cov).round() as u8;
                rgba[i + 1] = (acc[1] / cov).round() as u8;
                rgba[i + 2] = (acc[2] / cov).round() as u8;
                rgba[i + 3] = alpha;
            }
        }
    }
    Icon {
        width: size,
        height: size,
        rgba,
    }
}

/// The worst 5-hour percentage across accounts, or `None` when no account has
/// a reading. The gauge and the colour must agree, so both derive from this.
pub fn worst_percent(usages: &[Usage]) -> Option<f64> {
    usages
        .iter()
        .filter_map(|u| u.five_hour_percent)
        .fold(None::<f64>, |acc, p| Some(acc.map_or(p, |a| a.max(p))))
}

/// Icon for a set of readings: ring filled to the worst percentage, in the
/// worst colour.
pub fn icon_for(usages: &[Usage], size: u32) -> Icon {
    gauge_icon(worst_percent(usages), worst_color(usages), size)
}

/// A compact 10-cell text gauge, e.g. `▓▓▓▓▓▓▓░░░`. Native menus cannot draw
/// progress bars, so the bar has to be characters — but it reads far faster
/// than a bare number.
pub fn text_bar(percent: Option<f64>, cells: usize) -> String {
    let Some(p) = percent else {
        return "░".repeat(cells);
    };
    let filled = ((p / 100.0).clamp(0.0, 1.0) * cells as f64).round() as usize;
    format!(
        "{}{}",
        "▓".repeat(filled.min(cells)),
        "░".repeat(cells.saturating_sub(filled))
    )
}

/// Human reset time, e.g. `resets in 2h 15m`.
pub fn reset_label(resets_at: Option<i64>, now: i64) -> Option<String> {
    let r = resets_at?;
    let secs = r.saturating_sub(now);
    if secs <= 0 {
        return Some("resetting now".to_string());
    }
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    Some(if d > 0 {
        format!("resets in {d}d {h}h")
    } else if h > 0 {
        format!("resets in {h}h {m}m")
    } else {
        format!("resets in {m}m")
    })
}

fn state_word(u: &Usage) -> &'static str {
    if u.stale {
        "stale"
    } else if u.error.is_some() {
        "unavailable"
    } else if u.five_hour_percent.is_none() && u.weekly_percent.is_none() {
        "no reading"
    } else {
        "live"
    }
}

/// The tray menu: a header, then one two-line block per account.
///
/// Native menus have no progress-bar widget, so each account gets a header
/// line and a gauge line. Keeping the account name and its numbers on
/// separate lines is what makes the list scannable — the old single line
/// packed five values into one and was unreadable at menu width.
pub fn menu_entries(usages: &[Usage], now: i64) -> Vec<MenuEntry> {
    menu_entries_with(usages, now, &|_| true)
}

/// The same menu, told which accounts are signed in. A signed-out account is
/// listed but cannot be opened usefully: opening it would show Codex's own
/// sign-in screen under a name that reads as ready. `signed_in` is a predicate
/// so the caller's account list stays the single source of truth.
pub fn menu_entries_with(
    usages: &[Usage],
    now: i64,
    signed_in: &dyn Fn(&str) -> bool,
) -> Vec<MenuEntry> {
    let mut out = Vec::new();
    if usages.is_empty() {
        out.push(MenuEntry {
            label: "No accounts yet".to_string(),
            account: String::new(),
            kind: RowKind::Header,
            enabled: false,
        });
        return out;
    }

    out.push(MenuEntry {
        label: title(usages),
        account: String::new(),
        kind: RowKind::Header,
        enabled: false,
    });
    out.push(MenuEntry {
        label: String::new(),
        account: String::new(),
        kind: RowKind::Separator,
        enabled: false,
    });

    for (i, u) in usages.iter().enumerate() {
        let plan = u.plan.as_deref().unwrap_or("—");
        let logged = signed_in(&u.account);
        let state = state_word(u);
        let badge = if !logged {
            "  (sign in)".to_string()
        } else if state == "live" {
            String::new()
        } else {
            format!("  ({state})")
        };
        out.push(MenuEntry {
            label: format!("{}. {}  ·  {plan}{badge}", i + 1, u.account),
            account: u.account.clone(),
            kind: RowKind::Account,
            enabled: logged,
        });
        // The gauge and detail lines carry no account: only the name line opens
        // a window. Assigning them one made the shell give every row the same
        // `open:<account>` id, so clicking the numbers of a signed-out account
        // opened the window the name line had just refused to open.
        out.push(MenuEntry {
            label: format!(
                "   5h {} {}     week {} {}",
                text_bar(u.five_hour_percent, 10),
                pct(u.five_hour_percent),
                text_bar(u.weekly_percent, 10),
                pct(u.weekly_percent)
            ),
            account: String::new(),
            kind: RowKind::Header,
            enabled: false,
        });
        let mut detail = Vec::new();
        if let Some(r) = reset_label(u.resets_at, now) {
            detail.push(r);
        }
        if u.stale {
            detail.push(format!("last checked {}", age_label(u.checked_at, now)));
        } else if u.error.is_some() {
            detail.push("open a window to refresh".to_string());
        }
        if let Some(c) = u.credits {
            detail.push(format!("{c:.0} credits"));
        }
        if !detail.is_empty() {
            out.push(MenuEntry {
                label: format!("   {}", detail.join(" · ")),
                account: String::new(),
                kind: RowKind::Header,
                enabled: false,
            });
        }
        out.push(MenuEntry {
            label: String::new(),
            account: String::new(),
            kind: RowKind::Separator,
            enabled: false,
        });
    }
    out
}
