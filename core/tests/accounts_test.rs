use codex_dock_core::{accounts, Paths};
use std::path::{Path, PathBuf};

fn fixture(root: &Path, name: &str, auth: Option<&str>) -> PathBuf {
    let d = root.join(".codex-profiles").join(name);
    std::fs::create_dir_all(&d).unwrap();
    if let Some(a) = auth { std::fs::write(d.join("auth.json"), a).unwrap(); }
    d
}

fn tmp(tag: &str) -> PathBuf {
    let t = std::env::temp_dir().join(format!("cd-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

#[test]
fn lists_profiles_with_account_id() {
    let t = tmp("acc");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    fixture(&t, "work", Some(r#"{"tokens":{"account_id":"9c50"}}"#));
    fixture(&t, "personal", Some(r#"{"tokens":{"account_id":"a423"}}"#));
    let mut got = accounts::list(&p).unwrap();
    got.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].name, "personal");
    assert_eq!(got[0].account_id.as_deref(), Some("a423"));
    assert!(got[0].logged_in);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn malformed_auth_is_logged_out_not_an_error() {
    let (id, logged_in) = accounts::auth_info(Path::new("/nonexistent/auth.json"));
    assert!(id.is_none());
    assert!(!logged_in);
}

#[test]
fn dot_entries_are_not_listed_as_accounts() {
    let t = tmp("dot");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    fixture(&t, "work", Some(r#"{"tokens":{"account_id":"a"}}"#));
    std::fs::write(t.join(".codex-profiles/.active"), "work").unwrap();
    let got = accounts::list(&p).unwrap();
    assert_eq!(got.len(), 1);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn default_alias_is_one_account_and_named_profile_wins() {
    let t = tmp("alias");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    fixture(&t, "default", Some(r#"{"tokens":{"account_id":"same"}}"#));
    fixture(&t, "work", Some(r#"{"tokens":{"account_id":"same"}}"#));
    fixture(&t, "secondary", Some(r#"{"tokens":{"account_id":"other"}}"#));
    assert_eq!(accounts::list(&p).unwrap().iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["secondary", "work"]);
    assert_eq!(accounts::canonical_name(&p, "default").unwrap(), "work");
    assert_eq!(accounts::profiles(&p).unwrap().len(), 3);
    std::fs::remove_dir_all(t).unwrap();
}

#[test]
fn separate_signed_out_profiles_are_not_collapsed() {
    let t = tmp("signed-out");
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    fixture(&t, "first", None); fixture(&t, "second", None);
    assert_eq!(accounts::list(&p).unwrap().len(), 2);
    std::fs::remove_dir_all(t).unwrap();
}
