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
    std::fs::write(&base, r#"{"local-projects":{"p1":{"name":"wie-challenge"}},
        "electron-persisted-atom-state":{"local-document-identity":{"accountId":"BASE"}}}"#).unwrap();
    std::fs::write(&prof, r#"{"local-projects":{},
        "electron-persisted-atom-state":{"local-document-identity":{"accountId":"MINE"}}}"#).unwrap();

    store::merge_global_state(&base, &prof, &t.join("bak")).unwrap();

    let out: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prof).unwrap()).unwrap();
    assert_eq!(out["electron-persisted-atom-state"]["local-document-identity"]["accountId"], "MINE",
        "the window's own account must never be overwritten");
    assert!(out["local-projects"]["p1"].is_object(), "projects must be merged in");
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn merge_preserves_existing_project_entries() {
    let t = tmp("preserve");
    let base = t.join("base.json");
    let prof = t.join("prof.json");
    std::fs::write(&base, r#"{"local-projects":{"shared":{"name":"from-base"}}}"#).unwrap();
    std::fs::write(&prof, r#"{"local-projects":{"shared":{"name":"already-here"},"own":{"name":"mine"}}}"#).unwrap();

    store::merge_global_state(&base, &prof, &t.join("bak")).unwrap();

    let out: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prof).unwrap()).unwrap();
    assert_eq!(out["local-projects"]["shared"]["name"], "already-here", "local entry wins");
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

    store::repair(&p, &p.backup_dir("s")).unwrap();

    let out: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(p.profiles.join("work/.codex-global-state.json")).unwrap()).unwrap();
    assert!(out["local-projects"]["shared"].is_object(), "projects synced");
    assert_eq!(out["electron-persisted-atom-state"]["local-document-identity"]["accountId"], "MINE");
    std::fs::remove_dir_all(&t).ok();
}
