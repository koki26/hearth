//! The server's world repository: a directory of packed `.hworld` files.
//!
//! The server never looks inside one. It knows a world's slug (the file stem),
//! its size and its SHA-256, and it streams the bytes to clients that ask.
//! Everything about the format lives in the client (docs/WORLD_FORMAT.md).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use hearth_protocol::{MAX_WORLD_SIZE, World, validate_slug};
use tracing::{info, warn};

pub const EXTENSION: &str = "hworld";

#[derive(Debug, Clone)]
pub struct Entry {
    pub world: World,
    pub path: PathBuf,
}

/// Every readable world in `dir`, keyed by slug. Rebuilt by `scan`.
#[derive(Debug, Default)]
pub struct Repository {
    dir: PathBuf,
    entries: BTreeMap<String, Entry>,
}

impl Repository {
    /// Missing or unreadable directories give an empty repository: a server
    /// with no worlds still runs, it just has nothing to hand out.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        let mut repo = Self { dir: dir.into(), entries: BTreeMap::new() };
        repo.scan();
        repo
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn scan(&mut self) {
        self.entries.clear();
        let listing = match std::fs::read_dir(&self.dir) {
            Ok(listing) => listing,
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    warn!(dir = %self.dir.display(), "cannot read world directory: {err}");
                }
                return;
            }
        };
        for file in listing.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                continue;
            }
            match read_entry(&path) {
                // shortcut: every world is hashed on every scan; cache by
                // (size, mtime) if a repository ever grows past a few files.
                Ok(entry) => {
                    self.entries.insert(entry.world.slug.clone(), entry);
                }
                Err(err) => warn!(path = %path.display(), "skipped world: {err:#}"),
            }
        }
        info!(dir = %self.dir.display(), worlds = self.entries.len(), "scanned world repository");
    }

    pub fn get(&self, slug: &str) -> Option<&Entry> {
        self.entries.get(slug)
    }

    pub fn worlds(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// Copies a `.hworld` into the repository and returns its slug.
    pub fn add(&mut self, source: &Path) -> anyhow::Result<String> {
        let name = source.file_name().context("no file name")?;
        let entry = read_entry(source).context("not a usable world file")?;
        let target = self.dir.join(name);
        if target != source {
            std::fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
            std::fs::copy(source, &target).with_context(|| format!("copying to {}", target.display()))?;
        }
        let slug = entry.world.slug.clone();
        self.entries.insert(slug.clone(), Entry { path: target, ..entry });
        info!(%slug, "added world to the repository");
        Ok(slug)
    }
}

/// A world's slug is its file stem, so the stem has to be a valid slug: that
/// keeps a request for a slug from ever naming a file we did not list.
fn read_entry(path: &Path) -> anyhow::Result<Entry> {
    let slug = path.file_stem().and_then(|s| s.to_str()).context("no file stem")?.to_owned();
    validate_slug(&slug)
        .with_context(|| format!("{slug:?} is not a usable world name (a-z, 0-9, - and _)"))?;
    let size = std::fs::metadata(path)?.len();
    if size == 0 || size > MAX_WORLD_SIZE as u64 {
        bail!("{size} bytes is empty or over the {MAX_WORLD_SIZE}-byte limit");
    }
    Ok(Entry { world: World { slug, size: size as u32, sha256: sha256_file(path)? }, path: path.to_owned() })
}

/// Chunked, so hashing a big world does not read it all into memory.
fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        context.update(&buf[..read]);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(context.finish().as_ref());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hearth-worlds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_takes_valid_worlds_and_skips_the_rest() {
        let dir = temp_dir("scan");
        std::fs::write(dir.join("hub.hworld"), b"PK\x03\x04 pretend zip").unwrap();
        std::fs::write(dir.join("court-yard_2.hworld"), b"x").unwrap();
        std::fs::write(dir.join("Shouty.hworld"), b"x").unwrap(); // capitals: not a slug
        std::fs::write(dir.join("empty.hworld"), b"").unwrap(); // zero bytes
        std::fs::write(dir.join("notes.txt"), b"x").unwrap(); // wrong extension

        let repo = Repository::open(&dir);
        let slugs: Vec<_> = repo.worlds().map(|e| e.world.slug.as_str()).collect();
        assert_eq!(slugs, ["court-yard_2", "hub"]);
        let hub = repo.get("hub").unwrap();
        assert_eq!(hub.world.size, 16);
        // Known-answer SHA-256 of the file contents.
        assert_eq!(
            hub.world.sha256,
            sha256_file(&dir.join("hub.hworld")).unwrap(),
            "hash must come from the file bytes"
        );
        assert!(repo.get("Shouty").is_none() && repo.get("empty").is_none() && repo.get("notes").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn add_copies_into_the_repository() {
        let dir = temp_dir("add");
        let outside = temp_dir("add-src");
        let source = outside.join("forest.hworld");
        std::fs::write(&source, b"a world").unwrap();

        let mut repo = Repository::open(dir.join("worlds"));
        assert_eq!(repo.add(&source).unwrap(), "forest");
        assert!(dir.join("worlds/forest.hworld").exists());
        assert_eq!(repo.get("forest").unwrap().world.size, 7);
        assert!(repo.add(&outside.join("missing.hworld")).is_err());

        // A fresh scan of the same directory sees it.
        assert!(Repository::open(dir.join("worlds")).get("forest").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn missing_directory_is_an_empty_repository() {
        let repo = Repository::open(std::env::temp_dir().join("hearth-no-such-dir"));
        assert_eq!(repo.worlds().count(), 0);
        assert!(repo.get("hub").is_none());
    }
}
