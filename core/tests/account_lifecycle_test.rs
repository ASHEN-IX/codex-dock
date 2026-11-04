use codex_dock_core::{accounts, db, store, Paths, StoreState};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let t = std::env::temp_dir().join(format!("cd-{}-{}", tag, std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(&t).unwrap();
    t
}

fn setup(tag: &str) -> (PathBuf, Paths) {
    let t = tmp(tag);
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    std::fs::create_dir_all(&p.base).unwrap();
    std::fs::create_dir_all(&p.profiles).unwrap();
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();
    std::fs::create_dir_all(p.base.join("sessions")).unwrap();
    std::fs::write(p.base.join("history.jsonl"), "").unwrap();
    (t, p)
}

#[test]
fn a_new_account_shares_the_store_immediately() {
    let (t, p) = setup("link");
    accounts::create(&p, "fresh", None).unwrap();
    store::link_profile(&p, "fresh").unwrap();

    // Same inode through the new profile path: this is what makes the new
    // window show the same chats. Presence alone would not be enough.
    assert_eq!(
        db::inode(&p.base.join("state_5.sqlite")).unwrap(),
        db::inode(&p.profiles.join("fresh/state_5.sqlite")).unwrap()
    );
    assert!(p.profiles.join("fresh/sessions").is_symlink());
    assert_eq!(store::inspect(&p).state, StoreState::Shared);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn a_new_account_keeps_its_own_credentials_and_config() {
    let (t, p) = setup("personal");
    accounts::create(&p, "fresh", None).unwrap();
    store::link_profile(&p, "fresh").unwrap();

    // Sharing these would put every window on the same login.
    for own in ["auth.json", "config.toml"] {
        assert!(
            !p.profiles.join("fresh").join(own).is_symlink(),
            "{own} must stay per-account"
        );
    }
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn linking_twice_does_not_disturb_the_links() {
    let (t, p) = setup("idem");
    accounts::create(&p, "fresh", None).unwrap();
    store::link_profile(&p, "fresh").unwrap();
    let first = db::inode(&p.profiles.join("fresh/state_5.sqlite")).unwrap();
    store::link_profile(&p, "fresh").unwrap();
    assert_eq!(
        first,
        db::inode(&p.profiles.join("fresh/state_5.sqlite")).unwrap()
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn linking_skips_paths_the_base_store_does_not_have() {
    let (t, p) = setup("partial");
    accounts::create(&p, "fresh", None).unwrap();
    store::link_profile(&p, "fresh").unwrap();
    // No memories_1.sqlite in this base: a dangling link would make the new
    // window write into nothing.
    assert!(!p.profiles.join("fresh/memories_1.sqlite").exists());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn linking_an_unknown_account_is_refused() {
    let (t, p) = setup("unknown");
    assert!(store::link_profile(&p, "nope").is_err());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn removing_an_account_also_parks_its_window_data() {
    let (t, p) = setup("rmdata");
    accounts::create(&p, "gone", None).unwrap();
    store::link_profile(&p, "gone").unwrap();
    let udd = p.user_data_dir("gone");
    std::fs::create_dir_all(udd.join("Cache")).unwrap();
    std::fs::write(udd.join("Cookies"), "x").unwrap();

    let bak = p.backup_dir("stamp");
    accounts::remove(&p, "gone", &bak).unwrap();
    accounts::remove_user_data_dir(&p, "gone", &bak).unwrap();

    assert!(!udd.exists(), "window data must leave the config dir");
    assert!(bak.join("gone/auth.json").exists() || bak.join("gone").is_dir());
    assert!(
        bak.join("Codex-gone-userdata/Cookies").exists(),
        "and be recoverable"
    );
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn removing_window_data_that_never_existed_is_not_an_error() {
    let (t, p) = setup("nodata");
    let bak = p.backup_dir("stamp");
    accounts::remove_user_data_dir(&p, "never-opened", &bak).unwrap();
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn the_shared_store_survives_a_removal() {
    let (t, p) = setup("survive");
    db::insert_thread(&p.base.join("state_5.sqlite"), "t1", "/x").unwrap();
    accounts::create(&p, "a", None).unwrap();
    accounts::create(&p, "b", None).unwrap();
    store::link_profile(&p, "a").unwrap();
    store::link_profile(&p, "b").unwrap();

    let bak = p.backup_dir("stamp");
    accounts::remove(&p, "a", &bak).unwrap();
    accounts::remove_user_data_dir(&p, "a", &bak).unwrap();

    // The other account still sees the chats.
    assert_eq!(store::inspect(&p).state, StoreState::Shared);
    assert_eq!(
        db::thread_count(&p.profiles.join("b/state_5.sqlite")).unwrap(),
        1
    );
    std::fs::remove_dir_all(&t).ok();
}

/// The parser the shell uses to show the device code. Kept honest against the
/// real `codex login --device-auth` banner, ANSI escapes and all.
#[test]
fn signing_in_cannot_write_through_into_another_account() {
    // The live layout: base/auth.json -> profiles/work/auth.json, with
    // profiles/default a link to the base. So default/auth.json and work/auth.json
    // are the same file. `codex login` writes through a symlink, which would
    // replace work's credentials when signing in to default.
    let (t, p) = setup("protect");
    accounts::create(&p, "work", None).unwrap();
    std::os::unix::fs::symlink(p.base.clone(), p.profiles.join("default")).unwrap();
    std::fs::write(
        p.profiles.join("work/auth.json"),
        r#"{"tokens":{"account_id":"WORK-real"}}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(p.profiles.join("work/auth.json"), p.base.join("auth.json"))
        .unwrap();

    let bak = p.backup_dir("stamp");
    // The profile path itself is a link to the base, so materialise through the
    // resolved directory the way the shell does.
    let default_dir = std::fs::canonicalize(p.profiles.join("default")).unwrap();
    assert!(accounts::materialise_auth(&default_dir, &bak).unwrap());

    let auth = p.base.join("auth.json");
    assert!(!auth.is_symlink(), "login must not write through the link");
    assert!(auth.is_file());
    // It starts from the credentials it already had, so nothing is lost.
    let (id, ok) = accounts::auth_info(&auth);
    assert_eq!(id.as_deref(), Some("WORK-real"));
    assert!(ok);
    // And the other account's file is no longer the same file.
    assert_ne!(
        db::inode(&auth).unwrap(),
        db::inode(&p.profiles.join("work/auth.json")).unwrap(),
        "the two accounts must be independent after materialising"
    );
    // The displaced link is kept for the record.
    assert!(bak.join("auth.json.link").is_symlink());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn materialising_a_real_auth_file_changes_nothing() {
    let (t, p) = setup("alreadyreal");
    accounts::create(&p, "work", None).unwrap();
    std::fs::write(
        p.profiles.join("work/auth.json"),
        r#"{"tokens":{"account_id":"x"}}"#,
    )
    .unwrap();
    let bak = p.backup_dir("stamp");
    assert!(!accounts::materialise_auth(&p.profiles.join("work"), &bak).unwrap());
    assert!(p.profiles.join("work/auth.json").is_file());
    // A snapshot is taken even for a real file: login deletes auth.json as it
    // starts, so the only way back from a cancelled sign-in is that copy.
    assert!(bak.join("auth.json.pre-login").is_file());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn materialising_without_credentials_is_not_an_error() {
    let (t, p) = setup("noauth");
    accounts::create(&p, "fresh", None).unwrap();
    let bak = p.backup_dir("stamp");
    assert!(!accounts::materialise_auth(&p.profiles.join("fresh"), &bak).unwrap());
    assert!(!accounts::restore_auth(&p.profiles.join("fresh"), &bak).unwrap());
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn an_abandoned_sign_in_gets_its_credentials_back() {
    // `codex login` deletes auth.json when it starts and writes a new one only
    // on success. Without a restore, cancelling the dialog would log the
    // account out for good.
    let (t, p) = setup("restore");
    accounts::create(&p, "work", None).unwrap();
    let auth = p.profiles.join("work/auth.json");
    std::fs::write(&auth, r#"{"tokens":{"account_id":"KEEP"}}"#).unwrap();

    let bak = p.backup_dir("stamp");
    accounts::materialise_auth(&p.profiles.join("work"), &bak).unwrap();
    // What login does as it starts:
    std::fs::remove_file(&auth).unwrap();
    assert!(!accounts::auth_info(&auth).1, "signed out mid-flow");

    assert!(accounts::restore_auth(&p.profiles.join("work"), &bak).unwrap());
    let (id, ok) = accounts::auth_info(&auth);
    assert_eq!(id.as_deref(), Some("KEEP"));
    assert!(ok);
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn a_completed_sign_in_is_never_overwritten_by_the_snapshot() {
    let (t, p) = setup("keepnew");
    accounts::create(&p, "work", None).unwrap();
    let auth = p.profiles.join("work/auth.json");
    std::fs::write(&auth, r#"{"tokens":{"account_id":"OLD"}}"#).unwrap();
    let bak = p.backup_dir("stamp");
    accounts::materialise_auth(&p.profiles.join("work"), &bak).unwrap();

    // Login succeeded and wrote the new account.
    std::fs::write(&auth, r#"{"tokens":{"account_id":"NEW"}}"#).unwrap();
    assert!(!accounts::restore_auth(&p.profiles.join("work"), &bak).unwrap());
    let (id, _) = accounts::auth_info(&auth);
    assert_eq!(id.as_deref(), Some("NEW"));
    std::fs::remove_dir_all(&t).ok();
}

#[test]
fn device_code_parsing_survives_the_real_banner() {
    let raw = "Welcome to Codex [v\x1b[90m0.164.0\x1b[0m]\n\n\
Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
1. Open this link in your browser and sign in to your account\n\
\x1b[94mhttps://auth.openai.com/codex/device\x1b[0m\n\n\
2. Enter this one-time code \x1b[90m(expires in 15 minutes)\x1b[0m\n\
\x1b[94mS6ZU-7HYU9\x1b[0m\n";
    let (code, url) = codex_dock_core::login::parse_device_code(raw);
    assert_eq!(code.as_deref(), Some("S6ZU-7HYU9"));
    assert_eq!(url.as_deref(), Some("https://auth.openai.com/codex/device"));
}

#[test]
fn a_banner_without_a_code_yields_nothing_rather_than_a_wrong_code() {
    let raw = "Open https://auth.openai.com/codex/device and look at your terminal";
    let (code, url) = codex_dock_core::login::parse_device_code(raw);
    assert_eq!(code, None);
    assert_eq!(url.as_deref(), Some("https://auth.openai.com/codex/device"));
}
