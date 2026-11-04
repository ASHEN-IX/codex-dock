use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub records: HashMap<String, Record>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub attempted_at: i64,
    pub next_at: Option<i64>,
    pub reply: Option<String>,
    pub error: Option<String>,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(|e| Error::Msg(format!("Invalid auto-ping settings: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let temporary = path.with_extension("json.tmp");
        let mut file = std::fs::OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(&temporary)?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| Error::Msg(e.to_string()))?;
        file.write_all(&json)?;
        file.sync_all()?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn due(&self, id: &str, now: i64) -> bool {
        self.enabled && self.records.get(id).map_or(true, |r| r.next_at.is_some_and(|t| t <= now))
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled && !self.enabled {
            // Explicitly turning the feature back on permits retrying failures.
            // Successful pings with an unknown reset remain paused.
            self.records.retain(|_, r| r.error.is_none() && r.reply.is_some());
        }
        self.enabled = enabled;
    }

    pub fn complete(&mut self, id: &str, now: i64, reply: Option<String>, error: Option<String>, reset: Option<i64>) {
        self.records.insert(id.to_owned(), Record {
            attempted_at: now,
            next_at: if reply.is_some() { reset.filter(|t| *t > now) } else { None },
            reply, error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_in_and_reported_expiry_control_sending_not_future_reset_metadata_alone() {
        let mut s = Settings::default();
        assert!(!s.due("account", 100));
        s.set_enabled(true);
        assert!(s.due("account", 100));
        s.complete("account", 100, Some("pong".into()), None, Some(200));
        assert!(!s.due("account", 199));
        assert!(s.due("account", 200));
        s.set_enabled(false);
        assert!(!s.due("account", 201));
    }

    #[test]
    fn unknown_reset_and_failed_requests_do_not_repeat_automatically() {
        let mut s = Settings::default();
        s.set_enabled(true);
        s.complete("unknown", 100, Some("pong".into()), None, None);
        s.complete("failed", 100, None, Some("network failure".into()), Some(200));
        s.records.insert("interrupted".into(), Record::default());
        assert!(!s.due("unknown", 99999));
        assert!(!s.due("failed", 99999));
        assert!(!s.due("interrupted", 99999));
        s.set_enabled(false); s.set_enabled(true);
        assert!(s.due("failed", 99999));
        assert!(s.due("interrupted", 99999));
        assert!(!s.due("unknown", 99999));
    }

    #[test]
    fn restart_does_not_reping_a_successful_account() {
        let path = std::env::temp_dir().join(format!("dock-auto-settings-{}.json", std::process::id()));
        let mut s = Settings::default(); s.set_enabled(true);
        s.complete("a", 100, Some("pong".into()), None, Some(200));
        s.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert!(loaded.enabled); assert!(!loaded.due("a", 150)); assert!(loaded.due("a", 200));
        std::fs::remove_file(path).unwrap();
    }
}
