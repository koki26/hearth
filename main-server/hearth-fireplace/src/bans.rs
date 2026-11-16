//! Addresses an admin has refused, in `data/bans.json`. Read on every request
//! before anything else is done for it, so it is a map in memory and the file
//! is only for restarts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::accounts::Ban;

pub struct IpBans {
    path: PathBuf,
    bans: BTreeMap<String, Ban>,
}

impl IpBans {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join("bans.json");
        let bans = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("{} is damaged", path.display()))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
        };
        Ok(Self { path, bans })
    }

    /// The ban on `ip`, if one is in force.
    pub fn of(&self, ip: &str, now: u64) -> Option<&Ban> {
        self.bans.get(ip).filter(|b| b.active(now))
    }

    pub fn set(&mut self, ip: &str, ban: Ban) {
        self.bans.insert(ip.to_owned(), ban);
        self.save();
    }

    pub fn remove(&mut self, ip: &str) -> bool {
        let gone = self.bans.remove(ip).is_some();
        if gone {
            self.save();
        }
        gone
    }

    /// Bans in force; ones that ran out are dropped, address and all.
    pub fn list(&mut self, now: u64) -> &BTreeMap<String, Ban> {
        let before = self.bans.len();
        self.bans.retain(|_, b| b.active(now));
        if self.bans.len() != before {
            self.save();
        }
        &self.bans
    }

    fn save(&self) {
        let temporary = self.path.with_extension("tmp");
        let written = serde_json::to_vec_pretty(&self.bans)
            .map_err(anyhow::Error::from)
            .and_then(|json| Ok(std::fs::write(&temporary, json)?))
            .and_then(|()| Ok(std::fs::rename(&temporary, &self.path)?));
        if let Err(err) = written {
            tracing::error!(path = %self.path.display(), "could not write the bans file: {err:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ban_holds_until_it_runs_out_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("hearth-bans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut bans = IpBans::open(&dir).unwrap();
        let ban = |until| Ban { until, reason: "spam".into(), by: "root".into(), at: 1 };
        bans.set("203.0.113.9", ban(100));
        bans.set("198.51.100.1", ban(0));
        assert!(bans.of("203.0.113.9", 99).is_some() && bans.of("203.0.113.9", 100).is_none());
        let mut again = IpBans::open(&dir).unwrap();
        assert!(again.of("198.51.100.1", u64::MAX).is_some(), "for good, and kept");
        assert_eq!(again.list(200).len(), 1, "the one that ran out is dropped");
        assert!(again.remove("198.51.100.1") && again.of("198.51.100.1", 0).is_none());
    }
}
