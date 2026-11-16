//! Per-player storage that lives on the server (docs/TRUST.md).
//!
//! A creator needs somewhere to keep a score, a key, a door somebody opened.
//! It cannot live on the client, because the first patched client rewrites
//! it. So it lives here, and the rules that make that safe are all in one
//! place:
//!
//! * A client only ever writes **its own** data. There is no addressing
//!   anybody else, so there is nothing to abuse.
//! * Every key is **namespaced by the world** the player is in, by the server
//!   and not by the client, so one creator cannot read or clobber another.
//! * **Quotas** on key count and value size, checked before anything touches
//!   the disk.
//!
//! One file per player, named after a hash of their name so nothing a player
//! can type ever reaches the filesystem. Lines are `world/key<TAB>hex`, which
//! needs no dependency to parse and stays readable when something goes wrong.
//!
//! Keyed on the player's per-Flame id from their Fireplace ticket
//! (`ticket.rs`), never on a typed name, so nobody can claim anybody else's
//! data. Guests have no id and so no store at all.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use hearth_protocol::{MAX_VALUE_LEN, validate_key, validate_slug};
use tracing::warn;

/// How many keys one player may keep, across every world. Generous for a
/// score and a few flags, mean enough that nobody fills a disk.
pub const MAX_KEYS: usize = 256;

/// Why a write was refused. The client is told and carries on: going over a
/// quota is usually a bug in a world, not an attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    BadKey,
    TooBig,
    TooMany,
}

pub struct Store {
    root: PathBuf,
    /// Loaded on the first touch and kept, because a player reads their own
    /// data far more often than they write it.
    loaded: HashMap<String, Player>,
}

#[derive(Default)]
struct Player {
    name: String,
    values: HashMap<String, Vec<u8>>,
    dirty: bool,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), loaded: HashMap::new() }
    }

    /// What a player has under `key` in `world`, or empty for nothing.
    pub fn get(&mut self, name: &str, world: &str, key: &str) -> Vec<u8> {
        if validate_key(key).is_err() || validate_slug(world).is_err() {
            return Vec::new();
        }
        let player = self.player(name);
        player.values.get(&namespaced(world, key)).cloned().unwrap_or_default()
    }

    /// Writes one value, or deletes it when `value` is empty. The checks are
    /// here rather than at the call site so there is exactly one way in.
    pub fn put(&mut self, name: &str, world: &str, key: &str, value: Vec<u8>) -> Result<(), Refused> {
        if validate_key(key).is_err() || validate_slug(world).is_err() {
            return Err(Refused::BadKey);
        }
        if value.len() > MAX_VALUE_LEN {
            return Err(Refused::TooBig);
        }
        let full = namespaced(world, key);
        let player = self.player(name);
        if value.is_empty() {
            if player.values.remove(&full).is_some() {
                player.dirty = true;
            }
            return Ok(());
        }
        if !player.values.contains_key(&full) && player.values.len() >= MAX_KEYS {
            return Err(Refused::TooMany);
        }
        if player.values.get(&full) == Some(&value) {
            return Ok(()); // Writing the same thing again is not a change.
        }
        player.values.insert(full, value);
        player.dirty = true;
        Ok(())
    }

    /// Writes out whatever changed. Called when a player leaves and when the
    /// server stops, rather than on every key, so a world that counts
    /// something every frame does not count it onto the disk every frame.
    pub fn flush(&mut self, name: &str) {
        let Some(player) = self.loaded.get_mut(name) else {
            return;
        };
        if !player.dirty {
            return;
        }
        if let Err(err) = write_file(&self.root, name, player) {
            warn!(player = name, "could not save stored data: {err}");
            return;
        }
        player.dirty = false;
    }

    pub fn flush_all(&mut self) {
        let names: Vec<String> = self.loaded.keys().cloned().collect();
        for name in names {
            self.flush(&name);
        }
    }

    /// Lets a player go once their data is safely down.
    pub fn forget(&mut self, name: &str) {
        self.flush(name);
        self.loaded.remove(name);
    }

    fn player(&mut self, name: &str) -> &mut Player {
        if !self.loaded.contains_key(name) {
            let loaded = read_file(&self.root, name).unwrap_or_else(|err| {
                warn!(player = name, "could not read stored data, starting empty: {err}");
                Player::default()
            });
            self.loaded.insert(name.to_owned(), Player { name: name.to_owned(), ..loaded });
        }
        self.loaded.get_mut(name).expect("just inserted")
    }
}

/// The world a key belongs to is prepended by the server. The client never
/// names the namespace, which is the whole reason one world cannot reach
/// into another.
fn namespaced(world: &str, key: &str) -> String {
    format!("{world}/{key}")
}

/// A player names their own file, so the name never reaches the filesystem:
/// it is hashed. Anything else means thinking about every separator, every
/// reserved device name and every unicode trick, for ever.
fn file_for(root: &Path, name: &str) -> PathBuf {
    // ring is already here for the world hashes, so no new dependency.
    let digest = ring::digest::digest(&ring::digest::SHA256, name.as_bytes());
    root.join(format!("{}.txt", to_hex(digest.as_ref())))
}

fn read_file(root: &Path, name: &str) -> std::io::Result<Player> {
    let path = file_for(root, name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Player::default()),
        Err(err) => return Err(err),
    };
    let mut values = HashMap::new();
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((key, hex)) = line.split_once('\t') else {
            continue;
        };
        let Some(value) = from_hex(hex) else { continue };
        if value.len() <= MAX_VALUE_LEN && values.len() < MAX_KEYS {
            values.insert(key.to_owned(), value);
        }
    }
    Ok(Player { name: name.to_owned(), values, dirty: false })
}

fn write_file(root: &Path, name: &str, player: &Player) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let path = file_for(root, name);
    // Written beside and renamed, so a server killed mid-write leaves the
    // previous file rather than half of this one.
    let temporary = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&temporary)?;
        writeln!(file, "# {}", player.name.replace(['\n', '\r'], " "))?;
        let mut keys: Vec<&String> = player.values.keys().collect();
        keys.sort(); // Stable on disk, so a diff of two saves means something.
        for key in keys {
            writeln!(file, "{key}\t{}", to_hex(&player.values[key]))?;
        }
        file.sync_all()?;
    }
    std::fs::rename(&temporary, &path)
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2).map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own. Keyed on the test name as well as the
    /// process, because these run in parallel and sharing one directory means
    /// each test deleting the others out from under them.
    fn temp(who: &str) -> PathBuf {
        let at = std::env::temp_dir().join(format!("hearth-store-{}-{who}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        at
    }

    #[test]
    fn a_value_survives_a_restart() {
        let root = temp("a_value_survives_a_restart");
        let mut store = Store::new(&root);
        store.put("Ada", "hub", "score", vec![7]).unwrap();
        store.forget("Ada");

        // A whole new Store, as if the server had been restarted.
        let mut again = Store::new(&root);
        assert_eq!(again.get("Ada", "hub", "score"), vec![7]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn one_world_cannot_read_or_clobber_another() {
        let mut store = Store::new(temp("one_world_cannot_read_or_clobber_another"));
        store.put("Ada", "hub", "score", vec![1]).unwrap();
        store.put("Ada", "arena", "score", vec![2]).unwrap();
        assert_eq!(store.get("Ada", "hub", "score"), vec![1]);
        assert_eq!(store.get("Ada", "arena", "score"), vec![2]);
        // And one player cannot see another, even under the same key.
        assert!(store.get("Bob", "hub", "score").is_empty());
    }

    #[test]
    fn quotas_are_enforced_before_anything_is_written() {
        let mut store = Store::new(temp("quotas_are_enforced_before_anything_is_written"));
        assert_eq!(store.put("Ada", "hub", "big", vec![0; MAX_VALUE_LEN + 1]), Err(Refused::TooBig));
        assert_eq!(store.put("Ada", "hub", "../escape", vec![1]), Err(Refused::BadKey));
        assert_eq!(store.put("Ada", "../hub", "score", vec![1]), Err(Refused::BadKey));

        for i in 0..MAX_KEYS {
            store.put("Ada", "hub", &format!("key_{i}"), vec![1]).unwrap();
        }
        assert_eq!(store.put("Ada", "hub", "one_too_many", vec![1]), Err(Refused::TooMany));
        // Overwriting one that already exists is still fine at the limit.
        assert_eq!(store.put("Ada", "hub", "key_0", vec![2]), Ok(()));
    }

    #[test]
    fn an_empty_value_deletes() {
        let mut store = Store::new(temp("an_empty_value_deletes"));
        store.put("Ada", "hub", "score", vec![9]).unwrap();
        store.put("Ada", "hub", "score", Vec::new()).unwrap();
        assert!(store.get("Ada", "hub", "score").is_empty());
    }

    #[test]
    fn a_name_never_reaches_the_filesystem() {
        let root = Path::new("/tmp/hearth");
        // Whatever somebody types, the file is a hash under the one directory.
        for name in ["Ada", "../../etc/passwd", "con", "a\\b", "..", "x/y"] {
            let path = file_for(root, name);
            assert_eq!(path.parent(), Some(root), "{name:?} escaped the directory");
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(file.ends_with(".txt") && file.len() == 68, "{name:?} gave {file:?}");
        }
    }

    #[test]
    fn hex_round_trips_and_rejects_rubbish() {
        for bytes in [vec![], vec![0], vec![0xde, 0xad, 0xbe, 0xef], vec![255; 64]] {
            assert_eq!(from_hex(&to_hex(&bytes)), Some(bytes));
        }
        assert_eq!(from_hex("abc"), None); // Odd length.
        assert_eq!(from_hex("zz"), None);
    }
}
