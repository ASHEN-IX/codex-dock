use crate::platform;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub base: PathBuf,
    pub profiles: PathBuf,
}

impl Paths {
    pub fn real() -> Paths {
        let h = platform::fs::home_dir();
        Paths {
            base: h.join(".codex"),
            profiles: h.join(".codex-profiles"),
        }
    }

    pub fn new(base: impl Into<PathBuf>, profiles: impl Into<PathBuf>) -> Paths {
        Paths {
            base: base.into(),
            profiles: profiles.into(),
        }
    }

    pub fn home(&self) -> &Path {
        self.base.parent().unwrap_or(Path::new("/"))
    }

    /// One directory per operation, all under a single `codex-dock-backups`
    /// root: every backup stays isolated (a later operation cannot overwrite
    /// an earlier one) while the home directory gains one entry, not one per
    /// backup.
    pub fn backup_root(&self) -> PathBuf {
        self.home().join("codex-dock-backups")
    }

    pub fn backup_dir(&self, stamp: &str) -> PathBuf {
        self.backup_root().join(stamp)
    }

    /// Codex Desktop's per-window Electron profile. Each account gets its own
    /// so two windows cannot share cookies or window state.
    pub fn user_data_dir(&self, name: &str) -> PathBuf {
        platform::fs::config_dir_at(self.home()).join(format!("Codex-{name}"))
    }
}
