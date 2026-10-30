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

pub fn icon_name(color: Color) -> &'static str {
    match color {
        Color::Green => "tray-green.png",
        Color::Amber => "tray-amber.png",
        Color::Red => "tray-red.png",
        Color::Neutral => "tray-neutral.png",
    }
}
