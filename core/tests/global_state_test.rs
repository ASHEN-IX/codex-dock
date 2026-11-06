use codex_dock_core::store;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let t = std::env::temp_dir().join(format!("cd-gs-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn merge_keeps_the_windows_own_account_identity() {
    let t = tmp("identity");
    let base = t.join("base.json");
    let prof = t.join("prof.json");
    std::fs::write(
        &base,
        r#"{"local-projects":{"p1":{"name":"wie-challenge"}},
        "electron-persisted-atom-state":{"local-document-identity":{"accountId":"BASE"}}}"#,
    )
    .unwrap();
    std::fs::write(
        &prof,
        r#"{"local-projects":{},
        "electron-persisted-atom-state":{"local-document-identity":{"accountId":"MINE"}}}"#,
    )
    .unwrap();

    store::merge_global_state(&base, &prof, &t.join("bak")).unwrap();

    let out: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&prof).unwrap()).unwrap();
    assert_eq!(
        out["electron-persisted-atom-state"]["local-document-identity"]["accountId"], "MINE",
        "the window's own account must never be overwritten"
    );
    assert!(
        out["local-projects"]["p1"].is_object(),
        "projects must be merged in"
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn merge_preserves_existing_project_entries() {
    let t = tmp("preserve");
    let base = t.join("base.json");
    let prof = t.join("prof.json");
    std::fs::write(
        &base,
        r#"{"local-projects":{"shared":{"name":"from-base"}}}"#,
    )
    .unwrap();
    std::fs::write(
        &prof,
        r#"{"local-projects":{"shared":{"name":"already-here"},"own":{"name":"mine"}}}"#,
    )
    .unwrap();

    store::merge_global_state(&base, &prof, &t.join("bak")).unwrap();

    let out: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&prof).unwrap()).unwrap();
    assert_eq!(
        out["local-projects"]["shared"]["name"], "already-here",
        "local entry wins"
    );
    assert_eq!(out["local-projects"]["own"]["name"], "mine");
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn merge_is_a_noop_when_the_state_files_are_missing() {
    let t = tmp("missing");
    let r = store::merge_global_state(&t.join("nope.json"), &t.join("nope2.json"), &t.join("bak"));
    assert!(r.is_ok(), "missing files must not error: {r:?}");
    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn repair_syncs_projects_but_keeps_each_windows_account() {
    use codex_dock_core::{db, Paths};
    let t = tmp("repair");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(p.profiles.join("work")).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::write(p.base.join(".codex-global-state.json"),
        r#"{"local-projects":{"shared":{"name":"wie-challenge"}},"electron-persisted-atom-state":{"local-document-identity":{"accountId":"BASE"}}}"#).unwrap();
    std::fs::write(p.profiles.join("work/.codex-global-state.json"),
        r#"{"local-projects":{},"electron-persisted-atom-state":{"local-document-identity":{"accountId":"MINE"}}}"#).unwrap();

    store::sync_projects(&p, "work").unwrap();

    let out: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(p.profiles.join("work/.codex-global-state.json")).unwrap(),
    )
    .unwrap();
    assert!(
        out["local-projects"]["shared"].is_object(),
        "projects synced"
    );
    assert_eq!(
        out["electron-persisted-atom-state"]["local-document-identity"]["accountId"],
        "MINE"
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
#[cfg(target_os = "linux")]
fn a_new_profile_imports_projects_without_base_identity_or_migration_flags() {
    use codex_dock_core::{accounts, Paths};
    let t = tmp("fresh-projects");
    let p = Paths::new(t.join("base"), t.join("profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::write(
        p.base.join(".codex-global-state.json"),
        r#"{
        "local-projects":{"p1":{"name":"existing"}},
        "project-order":["p1"],
        "thread-project-assignments":{"thread1":"p1"},
        "app-server-projects-migration-by-host":{"host":{"done":true}},
        "authSecret":"do-not-copy",
        "electron-persisted-atom-state":{
            "local-document-identity":{"accountId":"BASE"},
            "flat-project-sidebar-preferences-v1":{"collapsed":false}}
    }"#,
    )
    .unwrap();
    accounts::create(&p, "new", None).unwrap();
    store::link_profile(&p, "new").unwrap();
    let target = p.profiles.join("new/.codex-global-state.json");
    let out: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(out["local-projects"]["p1"]["name"], "existing");
    assert_eq!(out["project-order"], serde_json::json!(["p1"]));
    assert_eq!(out["thread-project-assignments"]["thread1"], "p1");
    assert!(out.get("app-server-projects-migration-by-host").is_none());
    assert!(out.get("authSecret").is_none());
    assert!(out["electron-persisted-atom-state"]
        .get("local-document-identity")
        .is_none());
    let before = std::fs::read(&target).unwrap();
    store::sync_projects(&p, "new").unwrap();
    assert_eq!(
        before,
        std::fs::read(&target).unwrap(),
        "repeat import is idempotent"
    );
    std::fs::remove_dir_all(t).unwrap();
}

#[test]
fn missing_project_keys_are_added_and_local_order_and_identity_win() {
    let t = tmp("missing-keys");
    let base = t.join("base.json");
    let prof = t.join("prof.json");
    std::fs::write(&base, r#"{"local-projects":{"p1":{}},"project-order":["p1","own"],"app-server-projects-migration-by-host":{"BASE":{}}}"#).unwrap();
    let original = r#"{"project-order":["own"],"app-server-projects-migration-by-host":{"MINE":{}},"electron-persisted-atom-state":{"local-document-identity":{"accountId":"MINE"}}}"#;
    std::fs::write(&prof, original).unwrap();
    store::merge_global_state(&base, &prof, &t.join("bak")).unwrap();
    let out: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&prof).unwrap()).unwrap();
    assert!(out["local-projects"]["p1"].is_object());
    assert_eq!(out["project-order"], serde_json::json!(["own", "p1"]));
    assert_eq!(
        out["electron-persisted-atom-state"]["local-document-identity"]["accountId"],
        "MINE"
    );
    assert!(out["app-server-projects-migration-by-host"]
        .get("BASE")
        .is_none());
    assert_eq!(
        std::fs::read_to_string(t.join("bak/global-state.json")).unwrap(),
        original
    );
    std::fs::remove_dir_all(t).unwrap();
}

#[test]
fn malformed_profile_state_is_not_replaced() {
    let t = tmp("bad-state");
    let base = t.join("base.json");
    let prof = t.join("prof.json");
    std::fs::write(&base, r#"{"local-projects":{"p1":{}}}"#).unwrap();
    for original in ["[]", "not json", r#"{"local-projects":[]}"#] {
        std::fs::write(&prof, original).unwrap();
        assert!(store::merge_global_state(&base, &prof, &t.join("bak")).is_err());
        assert_eq!(std::fs::read_to_string(&prof).unwrap(), original);
    }
    std::fs::remove_dir_all(t).unwrap();
}
