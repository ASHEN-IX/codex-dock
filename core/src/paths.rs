use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub base: PathBuf,
    pub profiles: PathBuf,
}

impl Paths {
    pub fn real() -> Paths {
        let home = std::env::var("HOME").expect("HOME not set");
        let h = PathBuf::from(home);
        Paths { base: h.join(".codex"), profiles: h.join(".codex-profiles") }
    }

    pub fn new(base: impl Into<PathBuf>, profiles: impl Into<PathBuf>) -> Paths {
        Paths { base: base.into(), profiles: profiles.into() }
    }

    pub fn home(&self) -> &Path {
        self.base.parent().unwrap_or(Path::new("/"))
    }

    pub fn backup_dir(&self, stamp: &str) -> PathBuf {
        self.home().join(format!("codex-dock-backup-{stamp}"))
    }
}
