#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_dock_core::{accounts, store, tray, usage, windows, Paths};
use tauri::tray::TrayIconBuilder;

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
        "error": r.error,
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

fn refresh_tray(
    app: &tauri::AppHandle,
    rebuild_menu: &dyn Fn() -> tauri::Result<tauri::menu::Menu<tauri::Wry>>,
) {
    let p = Paths::real();
    let usages: Vec<_> = accounts::list(&p)
        .unwrap_or_default()
        .into_iter()
        .map(|a| usage::read(&p, &a.name, now()))
        .collect();

    let color = usage::worst_color(&usages);
    let icon_bytes: &[u8] = match color {
        usage::Color::Green => include_bytes!("../icons/tray-green.png"),
        usage::Color::Amber => include_bytes!("../icons/tray-amber.png"),
        usage::Color::Red => include_bytes!("../icons/tray-red.png"),
        usage::Color::Neutral => include_bytes!("../icons/tray-neutral.png"),
    };

    if let Some(tray) = app.tray_by_id("main") {
        // `tray::icon_name` is the single source for these names and is unit
        // tested; the match above picks the bytes for the same colour.
        if let Ok(img) = tauri::image::Image::from_bytes(icon_bytes) {
            let _ = tray.set_icon(Some(img));
        }
        let _ = tray.set_tooltip(Some(tray::title(&usages)));

        match rebuild_menu() {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => eprintln!("codex-dock: could not rebuild tray menu: {e}"),
        }
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Rebuilt on every refresh: a menu built once would show stale limits.
            let app_for_menu = app.handle().clone();
            let build_menu = move || -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
                let menu = tauri::menu::Menu::new(&app_for_menu)?;
                let p = Paths::real();
                let usages: Vec<_> = accounts::list(&p)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| usage::read(&p, &a.name, now()))
                    .collect();
                for row in tray::menu_rows(&usages) {
                    let item = tauri::menu::MenuItem::with_id(
                        &app_for_menu,
                        format!("open:{}", row.account),
                        row.label,
                        true,
                        None::<&str>,
                    )?;
                    menu.append(&item)?;
                }
                Ok(menu)
            };
            let menu = build_menu()?;
            let menu_for_refresh = std::sync::Arc::new(build_menu);

            let _tray = TrayIconBuilder::with_id("main")
                .tooltip("Codex Dock")
                .menu(&menu)
                .on_menu_event(|_app, event| {
                    if let Some(account) = event.id().0.strip_prefix("open:") {
                        let p = Paths::real();
                        let _ = windows::launch(&p, account);
                    }
                })
                .on_tray_icon_event(|tray, _event| {
                    // click handler has no access to build_menu; the 60s tick does the work
                    let _ = tray;
                })
                .build(app)?;

            // The spec's 60s tick: keep window and tray from disagreeing.
            {
                let h = app.handle().clone();
                let rebuild = std::sync::Arc::clone(&menu_for_refresh);
                std::thread::spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    refresh_tray(&h, rebuild.as_ref());
                });
            }

            refresh_tray(app.handle(), menu_for_refresh.as_ref());
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
