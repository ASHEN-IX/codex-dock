#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_dock_core::{accounts, store, tray, usage, windows, Paths};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
fn account_list() -> Result<serde_json::Value, String> {
    let p = Paths::real();
    let rows: Vec<_> = accounts::list(&p)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "name": a.name,
                "loggedIn": a.logged_in,
                "accountId": a.account_id,
            })
        })
        .collect();
    Ok(serde_json::json!(rows))
}

#[tauri::command]
fn store_status() -> Result<serde_json::Value, String> {
    let p = Paths::real();
    let r = store::inspect(&p);
    Ok(serde_json::json!({
        "state": format!("{:?}", r.state),
        "threads": r.threads,
        "projects": r.projects,
    }))
}

#[tauri::command]
fn usage_all() -> serde_json::Value {
    let p = Paths::real();
    let rows: Vec<_> = accounts::list(&p)
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            let u = usage::read(&p, &a.name, now());
            serde_json::json!({
                "account": u.account,
                "fiveHour": u.five_hour_percent,
                "weekly": u.weekly_percent,
                "credits": u.credits,
                "plan": u.plan,
                "checkedAt": u.checked_at,
                "error": u.error,
            })
        })
        .collect();
    serde_json::json!(rows)
}

#[tauri::command]
fn open_window(account: String) -> Result<u32, String> {
    let p = Paths::real();
    // Focus an existing window rather than spawning a duplicate.
    if let Some(existing) = windows::running(std::path::Path::new("/proc"))
        .into_iter()
        .find(|w| w.account == account)
    {
        return Ok(existing.pid);
    }
    windows::launch(&p, &account).map(|w| w.pid).map_err(|e| e.to_string())
}

#[tauri::command]
fn repair_store() -> Result<serde_json::Value, String> {
    let p = Paths::real();
    let stamp = now().to_string();
    let bak = p.backup_dir(&stamp);
    let r = store::repair(&p, &bak).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "state": format!("{:?}", r.state),
        "threads": r.threads,
        "backup": bak.display().to_string(),
    }))
}

fn refresh_tray(app: &tauri::AppHandle) {
    let p = Paths::real();
    let usages: Vec<_> = accounts::list(&p)
        .unwrap_or_default()
        .into_iter()
        .map(|a| usage::read(&p, &a.name, now()))
        .collect();
    let color = usage::worst_color(&usages);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tray::title(&usages)));
        if let Ok(img) = tauri::image::Image::from_bytes(include_bytes!(
            "../assets/tray-neutral.png"
        )) {
            let _ = tray.set_icon(Some(img));
        }
    }
    let _ = color;
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let _tray = TrayIconBuilder::with_id("main")
                .tooltip("Codex Dock")
                .on_tray_icon_event(|tray, _event| {
                    refresh_tray(tray.app_handle());
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            account_list,
            store_status,
            usage_all,
            open_window,
            repair_store
        ])
        .run(tauri::generate_context!())
        .expect("error while running Codex Dock");
}
