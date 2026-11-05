#![cfg(unix)]

use codex_dock_core::{db, store, Paths};

#[test]
fn missing_sqlite_is_reported_not_faked_as_zero() {
    let t = std::env::temp_dir().join(format!("cd-nosql-{}", std::process::id()));
    std::fs::remove_dir_all(&t).ok();
    std::fs::create_dir_all(t.join(".codex")).unwrap();
    std::fs::create_dir_all(t.join(".codex-profiles/work")).unwrap();
    let p = Paths::new(t.join(".codex"), t.join(".codex-profiles"));
    db::create_empty(&p.base.join("state_5.sqlite")).unwrap();

    let real = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", "");

    let r = store::inspect(&p);

    std::env::set_var("PATH", real);
    assert!(
        r.error.is_some(),
        "must report it could not read the store; got state {:?}, threads {}",
        r.state,
        r.threads
    );
    std::fs::remove_dir_all(&t).ok();
}
