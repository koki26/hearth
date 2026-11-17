//! Players reporting each other: a picture, a name, a bio, behaviour.
//!
//! Kept in `data/reports.json` until somebody with the `reports` permission
//! deals with one; dealt with, it is deleted, not archived. Written whole and
//! renamed into place, like the people file.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// What a report can be about: id and the words shown for it.
pub const REASONS: &[(&str, &str)] = &[
    ("picture", "Inappropriate picture"),
    ("name", "Offensive name"),
    ("bio", "Offensive bio or pronouns"),
    ("harassment", "Harassment or bullying"),
    ("hate", "Hate speech"),
    ("cheating", "Cheating"),
    ("spam", "Spam or scams"),
    ("underage", "May be under the minimum age"),
    ("other", "Something else"),
];
pub const MAX_DETAILS: usize = 500;
/// Waiting at once, so a flood cannot fill the disk.
pub const MAX_OPEN: usize = 5000;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub id: u64,
    pub reporter: u64,
    pub target: u64,
    pub reason: String,
    #[serde(default)]
    pub details: String,
    pub at: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct File {
    next_id: u64,
    reports: Vec<Report>,
}

pub struct Reports {
    path: PathBuf,
    file: File,
}

/// Why a report was not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    BadReason,
    TooLong,
    AlreadyReported,
    Full,
}

impl Reports {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join("reports.json");
        let file = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("{} is not a reports file", path.display()))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
        };
        Ok(Self { path, file })
    }

    pub fn file(&mut self, reporter: u64, target: u64, reason: &str, details: &str, now: u64) -> Result<u64, Refused> {
        let details = details.trim();
        if !REASONS.iter().any(|(id, _)| *id == reason) {
            return Err(Refused::BadReason);
        }
        if details.len() > MAX_DETAILS || details.chars().any(|c| c.is_control() && c != '\n') {
            return Err(Refused::TooLong);
        }
        // Once per reason until it is dealt with: saying it again adds nothing.
        if self.file.reports.iter().any(|r| r.reporter == reporter && r.target == target && r.reason == reason) {
            return Err(Refused::AlreadyReported);
        }
        if self.file.reports.len() >= MAX_OPEN {
            return Err(Refused::Full);
        }
        self.file.next_id += 1;
        let id = self.file.next_id;
        self.file.reports.push(Report { id, reporter, target, reason: reason.to_owned(), details: details.to_owned(), at: now });
        self.save();
        Ok(id)
    }

    pub fn open_reports(&self) -> &[Report] {
        &self.file.reports
    }

    pub fn filed_by(&self, reporter: u64) -> Vec<&Report> {
        self.file.reports.iter().filter(|r| r.reporter == reporter).collect()
    }

    /// Dealt with: gone. Returns it, for the log.
    pub fn resolve(&mut self, id: u64) -> Option<Report> {
        let at = self.file.reports.iter().position(|r| r.id == id)?;
        let report = self.file.reports.remove(at);
        self.save();
        Some(report)
    }

    /// An account was deleted: nothing it filed or that was filed about it stays.
    pub fn forget(&mut self, account: u64) {
        let before = self.file.reports.len();
        self.file.reports.retain(|r| r.reporter != account && r.target != account);
        if self.file.reports.len() != before {
            self.save();
        }
    }

    fn save(&self) {
        let write = || -> Result<()> {
            use std::io::Write;
            let temporary = self.path.with_extension("tmp");
            let mut out = std::fs::File::create(&temporary)?;
            out.write_all(&serde_json::to_vec_pretty(&self.file)?)?;
            out.sync_all()?;
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        };
        if let Err(err) = write() {
            tracing::error!(path = %self.path.display(), "could not write the reports file: {err:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_kept_once_until_dealt_with_and_goes_with_the_account() {
        let dir = std::env::temp_dir().join(format!("hearth-reports-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut reports = Reports::open(&dir).unwrap();
        let id = reports.file(1, 2, "picture", "rude", 10).unwrap();
        assert_eq!(reports.file(1, 2, "picture", "again", 11), Err(Refused::AlreadyReported));
        assert_eq!(reports.file(1, 2, "nonsense", "", 11), Err(Refused::BadReason));
        reports.file(3, 2, "name", "", 12).unwrap();
        let reports_again = Reports::open(&dir).unwrap();
        assert_eq!(reports_again.open_reports().len(), 2, "kept across a restart");
        assert_eq!(reports.resolve(id).map(|r| r.reporter), Some(1));
        reports.forget(2);
        assert!(reports.open_reports().is_empty());
    }
}
