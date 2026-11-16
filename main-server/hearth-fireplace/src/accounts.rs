//! Who people are, and who their friends are.
//!
//! One file, `data/people.json`, read into memory at boot and written back
//! whole whenever anything changes. That is a deliberate choice at this size:
//! a friendship touches two accounts, and one file written atomically is the
//! only way it can never be half true in one direction. Thousands of accounts
//! are a few hundred kilobytes; when that stops being true the thing to reach
//! for is a database, not a cleverer file.
//!
//! The write is the Flame's (`server/hearth-flame/src/store.rs`): beside, then
//! fsync, then rename, so a server killed mid-write leaves the previous file
//! rather than half of this one.
//!
//! Passwords are hashed by the caller, on a blocking thread, and arrive here
//! as a PHC string. This file never sees a password and never logs one.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Friends somebody may have, and requests they may be sitting on. Checked
/// before anything is written, the way the Flame's store checks its quotas.
pub const MAX_FRIENDS: usize = 256;
pub const MAX_PENDING: usize = 64;
/// A handle is a login. Narrower than a display name on purpose: two handles
/// that look the same are a way to be somebody else.
pub const MIN_HANDLE: usize = 3;
pub const MAX_HANDLE: usize = 20;
/// Long enough to be worth hashing, short enough that argon2 cannot be asked
/// to chew on a megabyte. No other rule: a rule about punctuation is how you
/// get `Password1!` from everybody.
pub const MIN_PASSWORD: usize = 8;
pub const MAX_PASSWORD: usize = 128;

/// Why something was not done. Each maps to one HTTP status in `api.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    BadHandle,
    HandleTaken,
    NoSuchAccount,
    Yourself,
    AlreadyFriends,
    TooManyFriends,
    TooManyRequests,
    Full,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BadHandle => "a handle is 3 to 20 letters, digits, - or _, starting with a letter",
            Self::HandleTaken => "that handle is taken",
            Self::NoSuchAccount => "nobody here goes by that handle",
            Self::Yourself => "you are already yourself",
            Self::AlreadyFriends => "you are already friends",
            Self::TooManyFriends => "that is as many friends as an account may have",
            Self::TooManyRequests => "that account has too many requests waiting",
            Self::Full => "this Fireplace is full",
        })
    }
}

/// One account. **Every field added after this must be `#[serde(default)]`
/// and nothing here may be `deny_unknown_fields`**: a rolled-back binary has
/// to be able to read a file a newer one wrote, or a bad deploy becomes lost
/// accounts.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: u64,
    /// As it was typed, for showing. Matching is always case-insensitive.
    pub handle: String,
    /// argon2id, in PHC form -- which carries its own parameters, so they can
    /// be raised later and old accounts rehashed as people sign in.
    pub password: String,
    pub created: u64,
    /// Bumping this makes every token issued before it worthless. The whole
    /// of revocation, in one number.
    #[serde(default = "one")]
    pub token_epoch: u64,
    #[serde(default = "yes")]
    pub share_location: bool,
    #[serde(default)]
    pub friends: Vec<u64>,
    #[serde(default)]
    pub incoming: Vec<u64>,
    #[serde(default)]
    pub outgoing: Vec<u64>,
}

fn one() -> u64 {
    1
}
fn yes() -> bool {
    true
}

/// What the file holds. `version` is what lets a later shape be recognised
/// instead of guessed at.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Book {
    version: u32,
    next_id: u64,
    accounts: Vec<Account>,
}

pub struct Accounts {
    path: PathBuf,
    next_id: u64,
    /// Ordered, so the file is stable and a diff of two saves means something.
    by_id: BTreeMap<u64, Account>,
    /// Lowercased handle -> id.
    by_handle: HashMap<String, u64>,
}

impl Accounts {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join("people.json");
        let mut store = Self { path, next_id: 1, by_id: BTreeMap::new(), by_handle: HashMap::new() };
        let text = match std::fs::read_to_string(&store.path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(store),
            Err(err) => {
                return Err(err).with_context(|| format!("could not read {}", store.path.display()));
            }
        };
        // A file that cannot be read is not an empty file: starting fresh
        // here would quietly orphan everybody and then overwrite them.
        let book: Book = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a people file", store.path.display()))?;
        store.next_id = book.next_id.max(1);
        for account in book.accounts {
            store.by_handle.insert(account.handle.to_lowercase(), account.id);
            store.next_id = store.next_id.max(account.id + 1);
            store.by_id.insert(account.id, account);
        }
        tracing::info!(accounts = store.by_id.len(), "read the people file");
        Ok(store)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&Account> {
        self.by_id.get(&id)
    }

    pub fn by_handle(&self, handle: &str) -> Option<&Account> {
        self.by_handle.get(&handle.to_lowercase()).and_then(|id| self.by_id.get(id))
    }

    /// Makes an account. The password arrives already hashed.
    pub fn create(&mut self, handle: &str, hashed: String, now: u64, max: usize) -> Result<u64, Refused> {
        let handle = handle.trim();
        if !handle_is_usable(handle) {
            return Err(Refused::BadHandle);
        }
        if self.by_handle.contains_key(&handle.to_lowercase()) {
            return Err(Refused::HandleTaken);
        }
        if self.by_id.len() >= max {
            return Err(Refused::Full);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.by_handle.insert(handle.to_lowercase(), id);
        self.by_id.insert(
            id,
            Account {
                id,
                handle: handle.to_owned(),
                password: hashed,
                created: now,
                token_epoch: 1,
                share_location: true,
                friends: Vec::new(),
                incoming: Vec::new(),
                outgoing: Vec::new(),
            },
        );
        self.save();
        Ok(id)
    }

    /// Asks to be friends. Answering a request that is already waiting the
    /// other way makes you friends instead, which is what anybody expects
    /// when two people add each other at the same time.
    pub fn request(&mut self, me: u64, them: u64) -> Result<bool, Refused> {
        if me == them {
            return Err(Refused::Yourself);
        }
        self.must_exist(them)?;
        let mine = self.by_id.get(&me).ok_or(Refused::NoSuchAccount)?;
        if mine.friends.contains(&them) {
            return Err(Refused::AlreadyFriends);
        }
        if mine.incoming.contains(&them) {
            self.accept(me, them)?;
            return Ok(true);
        }
        if mine.outgoing.contains(&them) {
            return Ok(false); // Asking twice is not an error, it is patience.
        }
        if mine.outgoing.len() >= MAX_PENDING {
            return Err(Refused::TooManyRequests);
        }
        if self.by_id[&them].incoming.len() >= MAX_PENDING {
            return Err(Refused::TooManyRequests);
        }
        self.by_id.get_mut(&me).expect("checked").outgoing.push(them);
        self.by_id.get_mut(&them).expect("checked").incoming.push(me);
        self.save();
        Ok(false)
    }

    /// Both sides are written in one save, so friendship can never be half
    /// true in one direction.
    pub fn accept(&mut self, me: u64, them: u64) -> Result<(), Refused> {
        self.must_exist(them)?;
        let mine = self.by_id.get(&me).ok_or(Refused::NoSuchAccount)?;
        if mine.friends.contains(&them) {
            return Err(Refused::AlreadyFriends);
        }
        if !mine.incoming.contains(&them) {
            return Err(Refused::NoSuchAccount);
        }
        if mine.friends.len() >= MAX_FRIENDS || self.by_id[&them].friends.len() >= MAX_FRIENDS {
            return Err(Refused::TooManyFriends);
        }
        for (a, b) in [(me, them), (them, me)] {
            let account = self.by_id.get_mut(&a).expect("checked");
            account.incoming.retain(|id| *id != b);
            account.outgoing.retain(|id| *id != b);
            account.friends.push(b);
        }
        self.save();
        Ok(())
    }

    /// Unfriend, decline, or take back a request: all the same button, from
    /// either side, whichever of the three states you are in.
    pub fn remove(&mut self, me: u64, them: u64) -> Result<(), Refused> {
        self.must_exist(them)?;
        if !self.by_id.contains_key(&me) {
            return Err(Refused::NoSuchAccount);
        }
        for (a, b) in [(me, them), (them, me)] {
            let account = self.by_id.get_mut(&a).expect("checked");
            account.friends.retain(|id| *id != b);
            account.incoming.retain(|id| *id != b);
            account.outgoing.retain(|id| *id != b);
        }
        self.save();
        Ok(())
    }

    pub fn set_sharing(&mut self, me: u64, on: bool) -> Result<(), Refused> {
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        if account.share_location == on {
            return Ok(()); // Nothing changed, so nothing is written.
        }
        account.share_location = on;
        self.save();
        Ok(())
    }

    fn must_exist(&self, id: u64) -> Result<(), Refused> {
        if self.by_id.contains_key(&id) { Ok(()) } else { Err(Refused::NoSuchAccount) }
    }

    /// Written beside and renamed, so a server killed mid-write leaves the
    /// previous file rather than half of this one. Failures are logged and
    /// swallowed: there is nothing useful to tell the player, and the next
    /// write will try again.
    fn save(&self) {
        let book =
            Book { version: 1, next_id: self.next_id, accounts: self.by_id.values().cloned().collect() };
        if let Err(err) = write_atomically(&self.path, &book) {
            tracing::error!(path = %self.path.display(), "could not write the people file: {err:#}");
        }
    }
}

fn write_atomically(path: &Path, book: &Book) -> Result<()> {
    use std::io::Write;
    let json = serde_json::to_vec_pretty(book)?;
    let temporary = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path)?;
    Ok(())
}

/// Letters, digits, `-` and `_`, starting with a letter. No dots, no
/// separators, nothing that could be read as a path -- and nothing that could
/// pass for somebody else's name at a glance.
pub fn handle_is_usable(handle: &str) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    handle.len() >= MIN_HANDLE
        && handle.len() <= MAX_HANDLE
        && handle.starts_with(|c: char| c.is_ascii_alphabetic())
        && handle.chars().all(ok)
}

pub fn password_is_usable(password: &str) -> bool {
    (MIN_PASSWORD..=MAX_PASSWORD).contains(&password.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(who: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hearth-fireplace-{}-{who}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make(store: &mut Accounts, handle: &str) -> u64 {
        store.create(handle, "$argon2id$fake".into(), 1, 100).unwrap()
    }

    #[test]
    fn an_account_survives_a_restart() {
        let dir = temp("restart");
        let id = {
            let mut store = Accounts::open(&dir).unwrap();
            make(&mut store, "Ada")
        };
        let store = Accounts::open(&dir).unwrap();
        let account = store.get(id).expect("still there");
        assert_eq!(account.handle, "Ada", "the capitals somebody chose are kept");
        assert_eq!(store.by_handle("ADA").map(|a| a.id), Some(id), "however it is typed");
        assert_eq!(account.token_epoch, 1);
        assert!(account.share_location);
    }

    #[test]
    fn a_handle_is_taken_whatever_its_case() {
        let mut store = Accounts::open(&temp("case")).unwrap();
        make(&mut store, "Ada");
        assert_eq!(store.create("ada", "x".into(), 1, 100), Err(Refused::HandleTaken));
        assert_eq!(store.create("ADA", "x".into(), 1, 100), Err(Refused::HandleTaken));
        assert!(store.create("Ada2", "x".into(), 1, 100).is_ok());
    }

    #[test]
    fn a_handle_never_reaches_the_filesystem() {
        // It cannot: the file is one file with a fixed name. These are
        // refused anyway, because a handle is a login and half of these are
        // somebody trying something.
        for bad in
            ["../../etc/passwd", "a/b", "..", "", "ab", "a".repeat(21).as_str(), "1ada", "ada!", " ada"]
        {
            assert!(!handle_is_usable(bad), "{bad:?} should not be a handle");
        }
        for good in ["ada", "Ada_2", "a-b-c", "x".repeat(20).as_str()] {
            assert!(handle_is_usable(good), "{good:?} should be a handle");
        }
        let mut store = Accounts::open(&temp("paths")).unwrap();
        assert_eq!(store.create("../../etc/passwd", "x".into(), 1, 100), Err(Refused::BadHandle));
        assert!(store.is_empty(), "and nothing was written");
    }

    #[test]
    fn friends_are_mutual_or_they_are_not_friends() {
        let dir = temp("mutual");
        let mut store = Accounts::open(&dir).unwrap();
        let (ada, bob) = (make(&mut store, "ada"), make(&mut store, "bob"));

        assert_eq!(store.request(ada, bob), Ok(false));
        assert_eq!(store.get(ada).unwrap().outgoing, vec![bob]);
        assert_eq!(store.get(bob).unwrap().incoming, vec![ada]);
        assert!(store.get(ada).unwrap().friends.is_empty(), "asking is not being");

        store.accept(bob, ada).unwrap();
        assert_eq!(store.get(ada).unwrap().friends, vec![bob]);
        assert_eq!(store.get(bob).unwrap().friends, vec![ada]);
        assert!(store.get(bob).unwrap().incoming.is_empty(), "the request is spent");

        // And it is still mutual after a restart.
        let store = Accounts::open(&dir).unwrap();
        assert_eq!(store.get(ada).unwrap().friends, vec![bob]);
        assert_eq!(store.get(bob).unwrap().friends, vec![ada]);
    }

    #[test]
    fn adding_somebody_who_already_asked_makes_you_friends() {
        let mut store = Accounts::open(&temp("crossed")).unwrap();
        let (ada, bob) = (make(&mut store, "ada"), make(&mut store, "bob"));
        store.request(ada, bob).unwrap();
        // Bob adds Ada rather than answering: the same thing, said twice.
        assert_eq!(store.request(bob, ada), Ok(true));
        assert_eq!(store.get(ada).unwrap().friends, vec![bob]);
        assert_eq!(store.get(bob).unwrap().friends, vec![ada]);
    }

    #[test]
    fn one_button_undoes_a_friendship_from_either_side_and_either_state() {
        let mut store = Accounts::open(&temp("remove")).unwrap();
        let (ada, bob) = (make(&mut store, "ada"), make(&mut store, "bob"));

        store.request(ada, bob).unwrap();
        store.remove(bob, ada).unwrap(); // Declined.
        assert!(store.get(ada).unwrap().outgoing.is_empty() && store.get(bob).unwrap().incoming.is_empty());

        store.request(ada, bob).unwrap();
        store.remove(ada, bob).unwrap(); // Taken back.
        assert!(store.get(bob).unwrap().incoming.is_empty());

        store.request(ada, bob).unwrap();
        store.accept(bob, ada).unwrap();
        store.remove(ada, bob).unwrap(); // Unfriended.
        assert!(store.get(ada).unwrap().friends.is_empty() && store.get(bob).unwrap().friends.is_empty());
    }

    #[test]
    fn quotas_and_nonsense_are_refused_before_anything_is_written() {
        let mut store = Accounts::open(&temp("quotas")).unwrap();
        let ada = make(&mut store, "ada");
        assert_eq!(store.request(ada, ada), Err(Refused::Yourself));
        assert_eq!(store.request(ada, 999), Err(Refused::NoSuchAccount));
        assert_eq!(store.accept(ada, 999), Err(Refused::NoSuchAccount));
        assert_eq!(store.create("bob", "x".into(), 1, 1), Err(Refused::Full), "the Fireplace is full");

        // Pending requests are capped, because a thousand of them is a way to
        // make somebody's friends page useless.
        let mut store = Accounts::open(&temp("pending")).unwrap();
        let ada = make(&mut store, "ada");
        for i in 0..MAX_PENDING {
            let other = make(&mut store, &format!("friend{i}"));
            store.request(other, ada).unwrap();
        }
        let one_too_many = make(&mut store, "lastone");
        assert_eq!(store.request(one_too_many, ada), Err(Refused::TooManyRequests));
    }

    #[test]
    fn a_file_that_cannot_be_read_stops_the_server_rather_than_replacing_it() {
        let dir = temp("broken");
        std::fs::write(dir.join("people.json"), "{ this is not json").unwrap();
        assert!(Accounts::open(&dir).is_err(), "better to refuse to start than to overwrite everybody");
    }
}
