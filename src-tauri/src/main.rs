#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_dock_core::login::{parse_device_code, strip_ansi};
use codex_dock_core::{accounts, store, tray, usage, windows, Paths};
use std::sync::Mutex;
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

/// One cache for the whole app, shared by the window commands and the tray
/// thread so the two can never show different numbers for the same account.
struct UsageCache(usage::Shared);

struct AutoPingState {
    settings: Mutex<codex_dock_core::automation::Settings>,
    path: std::path::PathBuf,
}

#[tauri::command]
fn auto_ping_settings(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let state = app.state::<AutoPingState>();
    let settings = state
        .settings
        .lock()
        .map_err(|_| "Auto-ping settings unavailable")?;
    serde_json::to_value(&*settings).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_auto_ping(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AutoPingState>();
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Auto-ping settings unavailable")?;
    let mut updated = settings.clone();
    updated.set_enabled(enabled);
    updated.save(&state.path).map_err(|e| e.to_string())?;
    *settings = updated;
    drop(settings);
    if enabled {
        let handle = app.clone();
        std::thread::spawn(move || auto_ping_tick(&handle));
    }
    Ok(())
}

fn auto_ping_tick(app: &tauri::AppHandle) {
    let p = Paths::real();
    for account in accounts::list(&p).unwrap_or_default() {
        let Some(id) = account.account_id.as_ref().filter(|_| account.logged_in) else {
            continue;
        };
        let state = app.state::<AutoPingState>();
        if !state
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .enabled
        {
            return;
        }
        let usage = app
            .state::<UsageCache>()
            .0
            .get_or_read(&p, &account.name, now);
        if usage.stale
            || usage.error.is_some()
            || usage.five_hour_percent.is_none()
            || usage.five_hour_percent.is_some_and(|n| n >= 100.0)
            || usage.weekly_percent.is_some_and(|n| n >= 100.0)
        {
            continue;
        }
        {
            let mut settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
            if !settings.enabled {
                return;
            }
            if !settings.due(id, now()) {
                continue;
            }
            settings.records.insert(
                id.clone(),
                codex_dock_core::automation::Record {
                    attempted_at: now(),
                    ..Default::default()
                },
            );
            // Reserve durably before sending: interrupted requests never loop.
            if let Err(e) = settings.save(&state.path) {
                settings.enabled = false;
                eprintln!("Auto-ping paused: could not save settings: {e}");
                return;
            }
        }
        let _ = perform_ping(app, &account.name);
    }
}

const CACHE_TTL_SECS: i64 = 60;

/// Namespaced so the tray's D-Bus object path cannot collide with another
/// libappindicator app's generic `tray_icon_tray_app_main` export.
const TRAY_ID: &str = "codex-dock-tray";

/// 32px source; Plasma scales it down for the panel. Rendering larger than
/// 22px keeps the ring smooth on HiDPI panels.
const TRAY_ICON_SIZE: u32 = 32;

/// The dashboard window. Closing it hides it rather than quitting: the tray is
/// the app's home, and a dock that disappears should stop reporting usage.
const MAIN_WINDOW: &str = "main";

/// A sign-in in progress, keyed by account. Device-code login takes as long as
/// it takes a human to approve it, so the window polls this instead of the
/// command blocking for minutes.
#[derive(Default)]
struct LoginState(Mutex<std::collections::HashMap<String, Login>>);

struct Login {
    code: Option<String>,
    url: Option<String>,
    done: bool,
    error: Option<String>,
    /// The `codex login` child, so Cancel can stop it. Without this the child
    /// outlives the dialog and the profile sits with no auth.json until it
    /// gives up.
    pid: Option<u32>,
}

impl Login {
    fn starting() -> Login {
        Login {
            code: None,
            url: None,
            done: false,
            error: None,
            pid: None,
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Create an account, point it at the shared store, and start device sign-in.
#[tauri::command]
fn add_account(name: String, clone_from: Option<String>) -> Result<serde_json::Value, String> {
    let p = Paths::real();
    let account = accounts::create(&p, &name, clone_from.as_deref()).map_err(|e| e.to_string())?;
    store::link_profile(&p, &name).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "name": account.name,
        "loggedIn": account.logged_in,
    }))
}

/// Spawn `codex login --device-auth` under the account's CODEX_HOME and record
/// the code it prints. The child keeps running until the user approves, so its
/// completion is what flips the account to signed in.
///
/// Returns immediately: the wait happens on a worker thread, because blocking
/// here would freeze the window for as long as the approval takes.
#[tauri::command]
fn start_login(name: String, app: tauri::AppHandle) -> Result<(), String> {
    let p = Paths::real();
    let home = p.profiles.join(&name);
    if !home.is_dir() {
        return Err(format!("account '{name}' does not exist"));
    }
    {
        let state = app.state::<LoginState>();
        let mut map = state
            .0
            .lock()
            .map_err(|_| "login state poisoned".to_string())?;
        // Adopt the in-flight sign-in instead of starting a second one: the
        // dialog can be reopened, and two `codex login` children would race for
        // the same auth.json.
        let pending = map.get(&name).map(|l| !l.done).unwrap_or(false);
        if pending {
            return Ok(());
        }
        map.insert(name.clone(), Login::starting());
    }
    std::thread::spawn(move || run_login(app, name, home));
    Ok(())
}

fn run_login(app: tauri::AppHandle, name: String, home: std::path::PathBuf) {
    use std::io::{BufRead, BufReader};

    // `codex login` rewrites auth.json through a symlink. The live layout links
    // ~/.codex/auth.json -- and with it the `default` profile -- at
    // work/auth.json, so signing in here without this step would replace another
    // account's credentials.
    let backup = Paths::real().backup_dir(&now().to_string());
    if let Err(e) = accounts::materialise_auth(&home, &backup) {
        set_login_error(
            &app,
            &name,
            format!("could not protect the existing credentials: {e}"),
        );
        return;
    }

    let child = std::process::Command::new(codex_dock_core::tool::codex())
        .args(["login", "--device-auth"])
        .env("CODEX_HOME", &home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            set_login_error(&app, &name, format!("could not run codex login: {e}"));
            return;
        }
    };
    set_login_pid(&app, &name, Some(child.id()));

    let Some(stdout) = child.stdout.take() else {
        set_login_error(&app, &name, "no output from codex login".to_string());
        return;
    };
    let stderr = child.stderr.take();

    // Drain stderr on its own thread *first*: reading stdout to EOF before
    // touching stderr deadlocks as soon as the child fills the 64 KB stderr
    // pipe, which is exactly what happens when login fails verbosely.
    let err_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let err_sink = std::sync::Arc::clone(&err_text);
    if let Some(err) = stderr {
        std::thread::spawn(move || {
            let sink = err_sink;
            for line in BufReader::new(err).lines().map_while(|l| l.ok()) {
                if let Ok(mut s) = sink.lock() {
                    s.push_str(&line);
                    s.push('\n');
                }
            }
        });
    }

    // Read the banner so the window can show the code, then keep draining so the
    // pipe cannot fill and stall the child.
    let reader = BufReader::new(stdout);
    let mut seen = String::new();
    let mut captured = None;
    for line in reader.lines().map_while(|l| l.ok()) {
        seen.push_str(&line);
        seen.push('\n');
        if captured.is_none() {
            let (code, url) = parse_device_code(&seen);
            if code.is_some() {
                captured = Some((code.clone(), url.clone()));
                set_login_code(&app, &name, code, url);
            }
        }
    }

    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    let (_, logged_in) = accounts::auth_info(&home.join("auth.json"));
    // A cancelled or failed sign-in leaves the profile with no auth.json at all,
    // because login deletes it as it starts. Put the old credentials back so the
    // account is not silently signed out by opening the dialog.
    let restored = if ok && logged_in {
        false
    } else {
        accounts::restore_auth(&home, &backup).unwrap_or(false)
    };
    let succeeded = (ok && logged_in) || restored;
    let mut diagnostics = seen;
    if let Ok(s) = err_text.lock() {
        diagnostics.push_str(&s);
    }

    let state = app.state::<LoginState>();
    let mut guard = match state.0.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(login) = guard.get_mut(&name) {
        login.done = true;
        login.pid = None;
        login.error = if succeeded {
            None
        } else if diagnostics.trim().is_empty() {
            Some("sign-in did not complete".to_string())
        } else {
            Some(last_meaningful_line(&diagnostics))
        };
    }
    drop(guard);
    // A new credential means the numbers belong to a different account now.
    app.state::<UsageCache>().0.forget(&name);
}

fn set_login_pid(app: &tauri::AppHandle, name: &str, pid: Option<u32>) {
    let state = app.state::<LoginState>();
    let mut guard = match state.0.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(login) = guard.get_mut(name) {
        login.pid = pid;
    }
}

/// Stop a sign-in the user backed out of, and put the old credentials back.
///
/// Without this, Cancel only stopped the polling: the child kept waiting for an
/// approval that would never come, with the profile's auth.json already deleted
/// by `codex login`. The worker restores the snapshot when it sees the child
/// exit non-zero, so signalling is enough.
#[tauri::command]
fn cancel_login(name: String, app: tauri::AppHandle) -> Result<(), String> {
    let pid = {
        let state = app.state::<LoginState>();
        let guard = match state.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.get(&name).and_then(|l| l.pid)
    };
    if let Some(pid) = pid {
        // SIGTERM, not SIGKILL: the CLI gets to clean up and exit non-zero,
        // which is what triggers the restore in the worker.
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
    Ok(())
}

fn set_login_code(app: &tauri::AppHandle, name: &str, code: Option<String>, url: Option<String>) {
    let state = app.state::<LoginState>();
    let mut guard = match state.0.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(login) = guard.get_mut(name) {
        login.code = code;
        login.url = url;
    }
}

fn set_login_error(app: &tauri::AppHandle, name: &str, error: String) {
    let state = app.state::<LoginState>();
    let mut guard = match state.0.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(login) = guard.get_mut(name) {
        login.done = true;
        login.error = Some(error);
    }
}

fn last_meaningful_line(s: &str) -> String {
    strip_ansi(s)
        .lines()
        .map(|l| l.trim().to_string())
        .rfind(|l| !l.is_empty() && !l.starts_with('─'))
        .unwrap_or_else(|| "sign-in did not complete".to_string())
}

/// Where the window polls for the code and the outcome.
#[tauri::command]
fn login_status(name: String, state: tauri::State<'_, LoginState>) -> serde_json::Value {
    let map = match state.0.lock() {
        Ok(m) => m,
        Err(p) => p.into_inner(),
    };
    match map.get(&name) {
        Some(l) => serde_json::json!({
            "pending": true,
            "code": l.code,
            "url": l.url,
            "done": l.done,
            "error": l.error,
            "running": l.pid.is_some(),
        }),
        None => serde_json::json!({ "pending": false }),
    }
}

/// Remove an account: its profile and its Electron window data both move to the
/// backup directory, never to the bin.
#[tauri::command]
fn delete_account(name: String) -> Result<serde_json::Value, String> {
    let p = Paths::real();
    let stamp = now().to_string();
    let bak = p.backup_dir(&stamp);
    let moved = accounts::remove(&p, &name, &bak).map_err(|e| e.to_string())?;
    accounts::remove_user_data_dir(&p, &name, &bak).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "moved": moved.display().to_string(),
        "backup": bak.display().to_string(),
    }))
}

fn show_dashboard(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
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
async fn usage_all(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = Paths::real();
        let state = app.state::<UsageCache>();
        let mut rows = Vec::new();
        for account in accounts::list(&p).unwrap_or_default() {
            let u = state.0.get_or_read(&p, &account.name, now);
            rows.push(usage_json(&u, now()));
        }
        serde_json::json!(rows)
    })
    .await
    .map_err(|e| format!("Usage refresh failed: {e}"))
}

fn usage_json(u: &usage::Usage, now: i64) -> serde_json::Value {
    serde_json::json!({
        "account": u.account,
        "fiveHour": u.five_hour_percent,
        "weekly": u.weekly_percent,
        "credits": u.credits,
        "plan": u.plan,
        "checkedAt": u.checked_at,
        "resetsAt": u.resets_at,
        "lifetimeTokens": u.lifetime_tokens,
        "peakDailyTokens": u.peak_daily_tokens,
        "streakDays": u.current_streak_days,
        "stale": u.stale,
        "age": tray::age_label(u.checked_at, now),
        "error": u.error,
    })
}

#[tauri::command]
async fn ping_account(account: String, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || perform_ping(&app, &account))
        .await
        .map_err(|e| format!("Ping worker failed: {e}"))?
}

fn perform_ping(app: &tauri::AppHandle, account: &str) -> Result<serde_json::Value, String> {
    let p = Paths::real();
    let cache = app.state::<UsageCache>();
    let identity = accounts::list(&p)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.name == account && a.logged_in)
        .and_then(|a| a.account_id)
        .ok_or("Ping requires a signed-in ChatGPT account, not an API-key profile.")?;
    let started = now();
    // A future reset time is quota metadata, not evidence of a running
    // desktop window or a previously delivered ping. An explicit click sends.
    let result = codex_dock_core::ping::send(&p, account);
    cache.0.forget(account);
    let after = cache.0.get_or_read(&p, account, now);
    let state = app.state::<AutoPingState>();
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Auto-ping settings unavailable")?;
    let mut warning = None;
    let newer_success = result.is_err()
        && settings
            .records
            .get(&identity)
            .is_some_and(|r| r.reply.is_some() && r.attempted_at >= started);
    if !newer_success {
        let reset = if after.error.is_none() && !after.stale {
            after.resets_at
        } else {
            None
        };
        settings.complete(
            &identity,
            now(),
            result.as_ref().ok().cloned(),
            result.as_ref().err().map(|e| e.to_string()),
            reset,
        );
        if let Err(e) = settings.save(&state.path) {
            warning = Some(format!("Could not save ping status: {e}"));
        }
    }
    let reply = result.map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"sent": true, "reply": reply, "usage": usage_json(&after, now()), "warning": warning}),
    )
}

#[tauri::command]
fn open_window(account: String) -> Result<u32, String> {
    let p = Paths::real();
    // Opening a signed-out profile only shows Codex's own sign-in screen under a
    // name that reads as ready.
    let logged_in = accounts::list(&p)
        .unwrap_or_default()
        .into_iter()
        .any(|a| a.name == account && a.logged_in);
    if !logged_in {
        return Err(format!(
            "'{account}' is not signed in yet — sign in from Codex Dock first"
        ));
    }
    windows::launch(&p, &account)
        .map(|w| w.pid)
        .map_err(|e| e.to_string())
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
    let cache = &app.state::<UsageCache>().0;
    let usages: Vec<_> = accounts::list(&p)
        .unwrap_or_default()
        .into_iter()
        .map(|a| cache.get_or_read(&p, &a.name, now))
        .collect();

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        // A gauge ring filled to the worst percentage: the colour alone said
        // "fine/warning/bad" but not "how close to the limit".
        let icon = tray::icon_for(&usages, TRAY_ICON_SIZE);
        let img = tauri::image::Image::new_owned(icon.rgba, icon.width, icon.height);
        let _ = tray.set_icon(Some(img));
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
    let mut builder = tauri::Builder::default();

    // One dock per session. A second launch (double-clicking the shortcut again)
    // raises the running window instead of starting a duplicate tray icon.
    builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        show_dashboard(app);
    }));

    builder
        .manage(UsageCache(usage::Shared::new(CACHE_TTL_SECS)))
        .manage(LoginState::default())
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("auto-ping.json");
            let settings = codex_dock_core::automation::Settings::load(&path).unwrap_or_else(|e| {
                eprintln!("Auto-ping disabled: {e}");
                Default::default()
            });
            app.manage(AutoPingState {
                settings: Mutex::new(settings),
                path,
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                auto_ping_tick(&handle);
                std::thread::sleep(std::time::Duration::from_secs(60));
            });
            // Rebuilt on every refresh: a menu built once would show stale limits.
            let app_for_menu = app.handle().clone();
            let build_menu = move || -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
                let menu = tauri::menu::Menu::new(&app_for_menu)?;
                let p = Paths::real();
                let accounts = accounts::list(&p).unwrap_or_default();
                let cache = &app_for_menu.state::<UsageCache>().0;
                let usages: Vec<_> = accounts
                    .iter()
                    .map(|a| {
                        cache
                            .last(&a.name)
                            .unwrap_or_else(|| usage::Usage::empty(&a.name, 0))
                    })
                    .collect();
                let signed_in: Vec<String> = accounts
                    .iter()
                    .filter(|a| a.logged_in)
                    .map(|a| a.name.clone())
                    .collect();
                let is_in = |name: &str| signed_in.iter().any(|n| n == name);
                for (i, entry) in tray::menu_entries_with(&usages, now(), &is_in)
                    .into_iter()
                    .enumerate()
                {
                    match entry.kind {
                        tray::RowKind::Separator => {
                            menu.append(&tauri::menu::PredefinedMenuItem::separator(
                                &app_for_menu,
                            )?)?;
                        }
                        tray::RowKind::Header => {
                            // Disabled items render as plain text: the native
                            // equivalent of a section heading.
                            let item = tauri::menu::MenuItem::with_id(
                                &app_for_menu,
                                format!("header:{i}"),
                                entry.label,
                                false,
                                None::<&str>,
                            )?;
                            menu.append(&item)?;
                        }
                        tray::RowKind::Account => {
                            let item = tauri::menu::MenuItem::with_id(
                                &app_for_menu,
                                format!("open:{}", entry.account),
                                entry.label,
                                entry.enabled,
                                None::<&str>,
                            )?;
                            menu.append(&item)?;
                        }
                    }
                }
                menu.append(&tauri::menu::PredefinedMenuItem::separator(&app_for_menu)?)?;
                menu.append(&tauri::menu::MenuItem::with_id(
                    &app_for_menu,
                    "open-dock",
                    "Open Codex Dock",
                    true,
                    None::<&str>,
                )?)?;
                menu.append(&tauri::menu::MenuItem::with_id(
                    &app_for_menu,
                    "add-account",
                    "Add account…",
                    true,
                    None::<&str>,
                )?)?;
                menu.append(&tauri::menu::MenuItem::with_id(
                    &app_for_menu,
                    "quit",
                    "Quit Codex Dock",
                    true,
                    None::<&str>,
                )?)?;
                Ok(menu)
            };
            let menu = build_menu()?;
            let menu_for_refresh = std::sync::Arc::new(build_menu);

            let _tray = TrayIconBuilder::with_id(TRAY_ID)
                .tooltip("Codex Dock")
                .menu(&menu)
                .on_menu_event(|_app, event| {
                    let id = event.id().0.as_str();
                    if id == "quit" {
                        _app.exit(0);
                        return;
                    }
                    if id == "open-dock" {
                        show_dashboard(_app);
                        return;
                    }
                    if id == "add-account" {
                        show_dashboard(_app);
                        if let Some(w) = _app.get_webview_window(MAIN_WINDOW) {
                            let _ = w.emit("add-account", ());
                        }
                        return;
                    }
                    if let Some(account) = id.strip_prefix("open:") {
                        let p = Paths::real();
                        // Same rule as the window button: raise, never duplicate.
                        if let Err(e) = windows::launch(&p, account) {
                            eprintln!("codex-dock: could not open {account}: {e}");
                        }
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // Left click opens the dashboard; the menu keeps the numbers.
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_dashboard(tray.app_handle());
                    }
                })
                .build(app)?;

            // The spec's 60s tick: keep window and tray from disagreeing.
            {
                let h = app.handle().clone();
                let rebuild = std::sync::Arc::clone(&menu_for_refresh);
                std::thread::spawn(move || loop {
                    refresh_tray(&h, rebuild.as_ref());
                    std::thread::sleep(std::time::Duration::from_secs(60));
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // "Closed" means "docked": hide it, keep the tray and the tick.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            account_list,
            add_account,
            start_login,
            login_status,
            cancel_login,
            delete_account,
            store_status,
            usage_all,
            ping_account,
            auto_ping_settings,
            set_auto_ping,
            open_window,
            repair_store
        ])
        .run(tauri::generate_context!())
        .expect("error while running Codex Dock");
}
