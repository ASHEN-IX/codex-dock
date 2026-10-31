use crate::usage::{Color, Usage};

#[derive(Debug, Clone, PartialEq)]
pub struct MenuRow {
    pub label: String,
    pub account: String,
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
                label: format!("{} · {} 5h {} · {}", u.account, plan, pct(u.five_hour_percent), pct(u.weekly_percent)),
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
            MenuRow { label, account: u.account.clone() }
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
