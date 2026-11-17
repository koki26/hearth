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
    BadDisplayName,
    BadProfile,
    BadGroups,
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
            Self::BadDisplayName => "a display name is 1 to 32 bytes, with no control characters",
            Self::BadProfile => "pronouns are up to 24 bytes and a bio up to 300, with no control characters",
            Self::BadGroups => "a group needs a name of 1 to 24 bytes, used once, and only known permissions",
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
    /// What Flames and friends see. Empty means the handle. Unlike a handle
    /// it may change and need not be unique: it is a name, not a login.
    #[serde(default)]
    pub display_name: String,
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
    /// Optional, and only ever for resets and security notices
    /// (`docs/decisions.md` D63). Skipped when absent, so an account without
    /// one is written exactly as it always was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<Email>,
    /// SHA-256 of each unused recovery code. The codes are 80 random bits, so
    /// a fast hash is enough; argon2 is for things people choose.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovery: Vec<String>,
    /// When the terms and privacy notice were accepted, as proof of it.
    /// 0 for accounts made before sign-up asked.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub terms_accepted: u64,
    /// Asked for an email when the terms or privacy notice change. Off until
    /// turned on, off again whenever they like, and only ever sent to a
    /// confirmed address.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub policy_emails: bool,
    /// Where it signed in from, for security: a new place has to be confirmed
    /// by email. Each entry is forgotten [`PLACE_DAYS`] after it was last seen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<Seen>,
    /// When the display name was changed, within the last [`RENAME_WINDOW`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renamed: Vec<u64>,
    /// When the uploaded picture was set, which is also its cache key. 0: none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub avatar: u64,
    /// Mirrors "is the owner or in a group", so an older binary reading this
    /// file still knows who its admins are.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub admin: bool,
    /// The one account that runs this Fireplace: every permission, and the
    /// only one that makes groups and puts people in them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner: bool,
    /// The name of a group in `Book::groups`; empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Shown on the public profile.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pronouns: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bio: String,
    /// When a picture waiting for review was uploaded; 0 for none. Nobody
    /// else sees it until it is approved and becomes `avatar`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub avatar_pending: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ban: Option<Ban>,
    /// Two-step sign-in: a code by email on every sign-in, not only from a
    /// new IP address.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub two_factor: bool,
    /// Where it is signed in: one per sign-in, each ended on its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<Session>,
}

/// One sign-in, kept while its refresh token could still be used.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: String,
    /// "Hearth game", "Firefox on Windows": what the device called itself.
    pub client: String,
    pub ip: String,
    pub started: u64,
    pub last: u64,
}

/// At most this many sessions per account; the one used longest ago goes.
const MAX_SESSIONS: usize = 20;

/// One place an account signed in from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Seen {
    pub ip: String,
    pub first: u64,
    pub last: u64,
    /// Confirmed by the owner, by the link sent to their email, or the place
    /// the account was made from.
    pub confirmed: bool,
}

/// A ban on an account or an address. `until` 0 is for good.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ban {
    pub until: u64,
    pub reason: String,
    /// The admin's handle.
    pub by: String,
    pub at: u64,
}

impl Ban {
    pub fn active(&self, now: u64) -> bool {
        self.until == 0 || now < self.until
    }
}

/// How long a sign-in place is kept after it was last used.
pub const PLACE_DAYS: u64 = 90;
const PLACE_SECONDS: u64 = PLACE_DAYS * 24 * 60 * 60;
/// At most this many places per account; the oldest goes first.
const MAX_PLACES: usize = 30;
/// Display names change at most [`RENAMES`] times in this long.
pub const RENAME_WINDOW: u64 = 30 * 24 * 60 * 60;
pub const RENAMES: usize = 2;

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// An address and whether its owner has clicked the link sent to it. Nothing
/// else about it is kept.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Email {
    pub address: String,
    pub verified: bool,
    /// When it was added, so an unverified one can be forgotten.
    pub since: u64,
}

/// An address nobody confirms is deleted after this long.
pub const UNVERIFIED_SECONDS: u64 = 7 * 24 * 60 * 60;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<Group>,
}

/// A set of permissions the owner hands out under one name, which is also
/// the badge its members wear.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub perms: Vec<String>,
}

/// Everything a group can be allowed. The owner has all of them, and alone
/// manages groups.
pub const PERMS: &[&str] = &["view", "ban", "delete", "sign_out", "moderate", "review", "reports", "flames"];
const MAX_GROUPS: usize = 20;
pub const MAX_PRONOUNS: usize = 24;
pub const MAX_BIO: usize = 300;

pub struct Accounts {
    path: PathBuf,
    next_id: u64,
    /// Ordered, so the file is stable and a diff of two saves means something.
    by_id: BTreeMap<u64, Account>,
    /// Lowercased handle -> id.
    by_handle: HashMap<String, u64>,
    groups: Vec<Group>,
}

impl Accounts {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join("people.json");
        let mut store =
            Self { path, next_id: 1, by_id: BTreeMap::new(), by_handle: HashMap::new(), groups: Vec::new() };
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
        store.groups = book.groups;
        for account in book.accounts {
            store.by_handle.insert(account.handle.to_lowercase(), account.id);
            store.next_id = store.next_id.max(account.id + 1);
            store.by_id.insert(account.id, account);
        }
        tracing::info!(accounts = store.by_id.len(), "read the people file");
        store.migrate_admins();
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
                display_name: String::new(),
                password: hashed,
                created: now,
                token_epoch: 1,
                share_location: true,
                friends: Vec::new(),
                incoming: Vec::new(),
                outgoing: Vec::new(),
                email: None,
                recovery: Vec::new(),
                terms_accepted: now,
                policy_emails: false,
                seen: Vec::new(),
                renamed: Vec::new(),
                avatar: 0,
                admin: false,
                owner: false,
                group: String::new(),
                pronouns: String::new(),
                bio: String::new(),
                avatar_pending: 0,
                ban: None,
                two_factor: false,
                sessions: Vec::new(),
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

    pub fn set_policy_emails(&mut self, me: u64, on: bool) -> Result<(), Refused> {
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        if account.policy_emails != on {
            account.policy_emails = on;
            self.save();
        }
        Ok(())
    }

    /// Handle and confirmed address of everybody who asked to hear about
    /// changes to the terms or privacy notice.
    pub fn policy_recipients(&self) -> Vec<(String, String)> {
        self.by_id
            .values()
            .filter(|a| a.policy_emails)
            .filter_map(|a| a.email.as_ref().filter(|e| e.verified).map(|e| (a.handle.clone(), e.address.clone())))
            .collect()
    }

    pub fn set_display_name(&mut self, me: u64, name: &str) -> Result<(), Refused> {
        let name = name.trim();
        if hearth_protocol::validate_name(name).is_err() {
            return Err(Refused::BadDisplayName);
        }
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        account.display_name = name.to_owned();
        self.save();
        Ok(())
    }

    /// Back to the handle.
    pub fn clear_display_name(&mut self, me: u64) -> Result<(), Refused> {
        self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?.display_name.clear();
        self.save();
        Ok(())
    }

    /// A new password signs out everywhere: the epoch moves, so every token
    /// minted under the old one stops working. Returns the new epoch.
    pub fn set_password(&mut self, me: u64, hashed: String) -> Result<u64, Refused> {
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        account.password = hashed;
        account.token_epoch += 1;
        account.sessions.clear();
        let epoch = account.token_epoch;
        self.save();
        Ok(epoch)
    }

    /// A new address, unverified, replacing whatever was there -- which is
    /// deleted at once, not kept aside. `None` removes it. Returns the old
    /// one, if it was verified, so it can be told.
    pub fn set_email(&mut self, me: u64, address: Option<&str>, now: u64) -> Result<Option<String>, Refused> {
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        let new = address.map(|a| Email { address: a.to_owned(), verified: false, since: now });
        let old = std::mem::replace(&mut account.email, new);
        self.save();
        Ok(old.filter(|e| e.verified).map(|e| e.address))
    }

    /// True when `address` is the one waiting on this account, now verified.
    pub fn verify_email(&mut self, me: u64, address: &str) -> bool {
        let Some(email) = self.by_id.get_mut(&me).and_then(|a| a.email.as_mut()) else {
            return false;
        };
        if !email.address.eq_ignore_ascii_case(address) {
            return false;
        }
        if !email.verified {
            email.verified = true;
            self.save();
        }
        true
    }

    /// Accounts whose **verified** address is this one. Several may share an
    /// address; refusing that would tell a stranger it is in use.
    pub fn by_verified_email(&self, address: &str) -> Vec<&Account> {
        self.by_id
            .values()
            .filter(|a| {
                a.email.as_ref().is_some_and(|e| e.verified && e.address.eq_ignore_ascii_case(address))
            })
            .collect()
    }

    /// Deletes every address that was not verified in time.
    pub fn forget_unverified(&mut self, now: u64) {
        let mut changed = false;
        for account in self.by_id.values_mut() {
            if account
                .email
                .as_ref()
                .is_some_and(|e| !e.verified && now.saturating_sub(e.since) > UNVERIFIED_SECONDS)
            {
                account.email = None;
                changed = true;
            }
            let places = account.seen.len();
            account.seen.retain(|s| now.saturating_sub(s.last) <= PLACE_SECONDS);
            changed |= account.seen.len() != places;
        }
        if changed {
            self.save();
        }
    }

    /// Whether `ip` is a place this account may sign in from without asking:
    /// one it confirmed, or any place at all while it has none confirmed yet
    /// (accounts from before places were kept).
    pub fn place_is_known(&self, me: u64, ip: &str) -> bool {
        self.by_id.get(&me).is_some_and(|a| {
            !a.seen.iter().any(|s| s.confirmed) || a.seen.iter().any(|s| s.confirmed && s.ip == ip)
        })
    }

    /// Notes a sign-in from `ip`; `confirmed` marks it as the owner's.
    pub fn remember_place(&mut self, me: u64, ip: &str, now: u64, confirmed: bool) {
        let Some(account) = self.by_id.get_mut(&me) else {
            return;
        };
        match account.seen.iter_mut().find(|s| s.ip == ip) {
            Some(place) => {
                place.last = now;
                place.confirmed |= confirmed;
            }
            None => account.seen.push(Seen { ip: ip.to_owned(), first: now, last: now, confirmed }),
        }
        account.seen.sort_by_key(|s| std::cmp::Reverse(s.last));
        account.seen.truncate(MAX_PLACES);
        self.save();
    }

    /// How many display name changes are left now, and when the next one
    /// frees up if none are.
    pub fn renames_left(&self, me: u64, now: u64) -> (usize, Option<u64>) {
        let Some(account) = self.by_id.get(&me) else {
            return (0, None);
        };
        let recent: Vec<u64> =
            account.renamed.iter().copied().filter(|t| now.saturating_sub(*t) < RENAME_WINDOW).collect();
        let left = RENAMES.saturating_sub(recent.len());
        (left, (left == 0).then(|| recent.iter().min().map_or(now, |t| t + RENAME_WINDOW)))
    }

    /// Counts one display name change against the monthly allowance.
    pub fn count_rename(&mut self, me: u64, now: u64) {
        if let Some(account) = self.by_id.get_mut(&me) {
            account.renamed.retain(|t| now.saturating_sub(*t) < RENAME_WINDOW);
            account.renamed.push(now);
            self.save();
        }
    }

    pub fn set_avatar(&mut self, me: u64, version: u64) -> Result<(), Refused> {
        self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?.avatar = version;
        self.save();
        Ok(())
    }

    pub fn set_two_factor(&mut self, me: u64, on: bool) -> Result<(), Refused> {
        self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?.two_factor = on;
        self.save();
        Ok(())
    }

    /// From before owners and groups: the oldest admin becomes the owner and
    /// the others go into an "Admin" group that can do everything.
    fn migrate_admins(&mut self) {
        if self.by_id.values().any(|a| a.owner) {
            return;
        }
        let Some(first) = self.by_id.values().find(|a| a.admin).map(|a| a.id) else {
            return;
        };
        self.by_id.get_mut(&first).expect("found").owner = true;
        let others: Vec<u64> =
            self.by_id.values().filter(|a| a.admin && !a.owner && a.group.is_empty()).map(|a| a.id).collect();
        if !others.is_empty() && !self.groups.iter().any(|g| g.name == "Admin") {
            self.groups.push(Group { name: "Admin".into(), perms: PERMS.iter().map(|p| (*p).to_owned()).collect() });
        }
        for id in others {
            self.by_id.get_mut(&id).expect("listed").group = "Admin".into();
        }
        tracing::info!(owner = first, "the first admin is now the Fireplace owner");
        self.save();
    }

    pub fn any_owner(&self) -> bool {
        self.by_id.values().any(|a| a.owner)
    }

    /// Makes this account the owner, and nobody else.
    pub fn set_owner(&mut self, me: u64) -> Result<(), Refused> {
        self.must_exist(me)?;
        for account in self.by_id.values_mut() {
            account.owner = account.id == me;
            account.admin = account.owner || !account.group.is_empty();
        }
        self.save();
        Ok(())
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Replaces every group. Members of a group that is gone lose it.
    pub fn set_groups(&mut self, groups: Vec<Group>) -> Result<(), Refused> {
        let mut seen = std::collections::HashSet::new();
        let good = groups.len() <= MAX_GROUPS
            && groups.iter().all(|g| {
                (1..=24).contains(&g.name.len())
                    && g.name == g.name.trim()
                    && !g.name.chars().any(char::is_control)
                    && seen.insert(g.name.to_lowercase())
                    && g.perms.iter().all(|p| PERMS.contains(&p.as_str()))
            });
        if !good {
            return Err(Refused::BadGroups);
        }
        for account in self.by_id.values_mut() {
            if !account.group.is_empty() && !groups.iter().any(|g| g.name == account.group) {
                account.group.clear();
                account.admin = account.owner;
            }
        }
        self.groups = groups;
        self.save();
        Ok(())
    }

    /// Puts an account in a group, or with "" in none.
    pub fn set_group(&mut self, me: u64, group: &str) -> Result<(), Refused> {
        if !group.is_empty() && !self.groups.iter().any(|g| g.name == group) {
            return Err(Refused::BadGroups);
        }
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        account.group = group.to_owned();
        account.admin = account.owner || !group.is_empty();
        self.save();
        Ok(())
    }

    /// Whether this account may do `perm`.
    pub fn can(&self, me: u64, perm: &str) -> bool {
        self.by_id.get(&me).is_some_and(|a| {
            a.owner || self.groups.iter().any(|g| g.name == a.group && g.perms.iter().any(|p| p == perm))
        })
    }

    /// What `me` may do, so the pages show only those tools.
    pub fn perms_of(&self, me: u64) -> Vec<&'static str> {
        PERMS.iter().copied().filter(|p| self.can(me, p)).collect()
    }

    /// Owner 2, a group member 1, anybody else 0: nobody acts on an account
    /// ranked as high as their own.
    pub fn rank(&self, me: u64) -> u8 {
        self.by_id.get(&me).map_or(0, |a| if a.owner { 2 } else { u8::from(!a.group.is_empty()) })
    }

    pub fn set_profile(&mut self, me: u64, pronouns: &str, bio: &str) -> Result<(), Refused> {
        let (pronouns, bio) = (pronouns.trim(), bio.trim());
        if pronouns.len() > MAX_PRONOUNS
            || bio.len() > MAX_BIO
            || pronouns.chars().any(char::is_control)
            || bio.chars().any(|c| c.is_control() && c != '\n')
        {
            return Err(Refused::BadProfile);
        }
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        account.pronouns = pronouns.to_owned();
        account.bio = bio.to_owned();
        self.save();
        Ok(())
    }

    pub fn set_avatar_pending(&mut self, me: u64, version: u64) -> Result<(), Refused> {
        self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?.avatar_pending = version;
        self.save();
        Ok(())
    }

    /// Bans (or with `None` unbans) and, banning, signs it out everywhere.
    pub fn set_ban(&mut self, me: u64, ban: Option<Ban>) -> Result<(), Refused> {
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        if ban.is_some() {
            account.token_epoch += 1;
            account.sessions.clear();
        }
        account.ban = ban;
        self.save();
        Ok(())
    }

    /// Every token issued so far stops working.
    pub fn sign_out_everywhere(&mut self, me: u64) -> Result<(), Refused> {
        let account = self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?;
        account.token_epoch += 1;
        account.sessions.clear();
        self.save();
        Ok(())
    }

    /// A new sign-in; its id goes into the tokens. Sessions nobody could
    /// still refresh are dropped on the way.
    pub fn start_session(&mut self, me: u64, client: &str, ip: &str, now: u64) -> String {
        let Some(account) = self.by_id.get_mut(&me) else {
            return String::new();
        };
        account.sessions.retain(|s| now.saturating_sub(s.last) < crate::token::REFRESH_SECONDS);
        account.sessions.sort_by_key(|s| std::cmp::Reverse(s.last));
        account.sessions.truncate(MAX_SESSIONS - 1);
        let mut bytes = [0u8; 9];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).expect("randomness");
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        account.sessions.push(Session { id: id.clone(), client: client.to_owned(), ip: ip.to_owned(), started: now, last: now });
        self.save();
        id
    }

    /// Whether `sid` is still a session of this account.
    pub fn has_session(&self, me: u64, sid: &str) -> bool {
        self.by_id.get(&me).is_some_and(|a| a.sessions.iter().any(|s| s.id == sid))
    }

    /// A refresh: the session was used now, from here.
    pub fn touch_session(&mut self, me: u64, sid: &str, ip: &str, now: u64) -> bool {
        let Some(session) = self.by_id.get_mut(&me).and_then(|a| a.sessions.iter_mut().find(|s| s.id == sid)) else {
            return false;
        };
        session.last = now;
        session.ip = ip.to_owned();
        self.save();
        true
    }

    /// Ends one session, or with `keep`, every one but that.
    pub fn end_sessions(&mut self, me: u64, only: Option<&str>, keep: Option<&str>) -> usize {
        let Some(account) = self.by_id.get_mut(&me) else {
            return 0;
        };
        let before = account.sessions.len();
        account.sessions.retain(|s| only.is_some_and(|o| o != s.id) || keep.is_some_and(|k| k == s.id));
        let ended = before - account.sessions.len();
        if ended > 0 {
            self.save();
        }
        ended
    }

    pub fn all(&self) -> impl Iterator<Item = &Account> {
        self.by_id.values()
    }

    /// Replaces every recovery code with these hashes.
    pub fn set_recovery(&mut self, me: u64, hashes: Vec<String>) -> Result<(), Refused> {
        self.by_id.get_mut(&me).ok_or(Refused::NoSuchAccount)?.recovery = hashes;
        self.save();
        Ok(())
    }

    /// Spends one recovery code. True if it was one of this account's.
    pub fn spend_recovery(&mut self, me: u64, hash: &str) -> bool {
        let Some(account) = self.by_id.get_mut(&me) else {
            return false;
        };
        let before = account.recovery.len();
        account.recovery.retain(|h| h != hash);
        let spent = account.recovery.len() < before;
        if spent {
            self.save();
        }
        spent
    }

    /// Gone for good, and out of everybody else's lists in the same save, so
    /// nobody is left friends with an id that means nothing. The handle is
    /// free again afterwards.
    pub fn delete(&mut self, me: u64) -> Result<(), Refused> {
        let account = self.by_id.remove(&me).ok_or(Refused::NoSuchAccount)?;
        self.by_handle.remove(&account.handle.to_lowercase());
        for other in self.by_id.values_mut() {
            other.friends.retain(|id| *id != me);
            other.incoming.retain(|id| *id != me);
            other.outgoing.retain(|id| *id != me);
        }
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
        let book = Book {
            version: 1,
            next_id: self.next_id,
            accounts: self.by_id.values().cloned().collect(),
            groups: self.groups.clone(),
        };
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
impl Account {
    /// What everybody else sees.
    pub fn display(&self) -> &str {
        if self.display_name.is_empty() { &self.handle } else { &self.display_name }
    }

    /// The badge on the profile and over the head: kind and words.
    pub fn badge(&self) -> Option<(&'static str, &str)> {
        if self.owner {
            Some(("owner", "Fireplace owner"))
        } else if !self.group.is_empty() {
            Some(("staff", &self.group))
        } else {
            None
        }
    }
}

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

/// Whole years between a `YYYY-MM-DD` birth date and `now` (UTC seconds),
/// or `None` for a date that is malformed, not real, in the future or over
/// 130 years ago. Only ever used to answer "old enough?": never stored.
pub fn age_from_birthdate(birthdate: &str, now: u64) -> Option<u32> {
    let mut parts = birthdate.trim().splitn(3, '-');
    let mut next = |len: usize| parts.next().filter(|p| p.len() == len && p.bytes().all(|b| b.is_ascii_digit()));
    let (y, m, d): (u32, u32, u32) = (next(4)?.parse().ok()?, next(2)?.parse().ok()?, next(2)?.parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d == 0 || d > days_in[m as usize - 1] {
        return None;
    }
    let (ty, tm, td) = civil_date(now);
    if (y, m, d) > (ty, tm, td) {
        return None;
    }
    let age = ty - y - u32::from((m, d) > (tm, td));
    (age <= 130).then_some(age)
}

/// A Unix time as people read it: `2026-11-16 14:05 UTC`.
pub fn when(at: u64) -> String {
    let (y, m, d) = civil_date(at);
    format!("{y}-{m:02}-{d:02} {:02}:{:02} UTC", at % 86_400 / 3600, at % 3600 / 60)
}

/// Year, month and day of a Unix time in UTC (Howard Hinnant's algorithm).
fn civil_date(now: u64) -> (u32, u32, u32) {
    let z = (now / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as u32;
    (y, m, d)
}

/// How many recovery codes an account holds when they are (re)made.
pub const RECOVERY_CODES: usize = 8;

/// Eight codes as they are shown, once, and their hashes as they are kept.
/// 80 random bits each, in hex, grouped so they can be read out and typed.
pub fn new_recovery_codes() -> (Vec<String>, Vec<String>) {
    let random = ring::rand::SystemRandom::new();
    let codes: Vec<String> = (0..RECOVERY_CODES)
        .map(|_| {
            let mut bytes = [0u8; 10];
            ring::rand::SecureRandom::fill(&random, &mut bytes).expect("the system has randomness");
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("{}-{}-{}-{}", &hex[..5], &hex[5..10], &hex[10..15], &hex[15..])
        })
        .collect();
    let hashes = codes.iter().map(|c| hash_recovery(c)).collect();
    (codes, hashes)
}

/// Dashes, spaces and capitals do not matter: a code is typed by hand.
pub fn hash_recovery(code: &str) -> String {
    let clean: String =
        code.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect();
    ring::digest::digest(&ring::digest::SHA256, clean.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_turns_over_on_the_birthday_and_not_a_day_before() {
        let day = |y: i64, m: i64, d: i64| {
            // Days from 1970-01-01, counted the slow way, as an independent check.
            let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            let mut n = (1970..y).map(|y| if leap(y) { 366 } else { 365 }).sum::<i64>();
            let months = [31, if leap(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            n += months[..(m - 1) as usize].iter().sum::<i64>() + d - 1;
            (n * 86_400 + 3600) as u64
        };
        assert_eq!(civil_date(day(2026, 11, 16)), (2026, 11, 16));
        assert_eq!(civil_date(day(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(age_from_birthdate("2010-11-16", day(2026, 11, 16)), Some(16));
        assert_eq!(age_from_birthdate("2010-11-17", day(2026, 11, 16)), Some(15));
        assert_eq!(age_from_birthdate("2008-02-29", day(2026, 2, 28)), Some(17));
        assert_eq!(age_from_birthdate("2008-02-29", day(2026, 3, 1)), Some(18));
        for wrong in ["", "2010-1-16", "2010-13-01", "2011-02-29", "2010-04-31", "2030-01-01", "1800-01-01", "abcd-ef-gh"] {
            assert_eq!(age_from_birthdate(wrong, day(2026, 11, 16)), None, "{wrong}");
        }
    }

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

    #[test]
    fn a_display_name_changes_and_a_deleted_account_leaves_no_trace() {
        let mut store = Accounts::open(&temp("display-delete")).unwrap();
        let (ada, bob) = (make(&mut store, "ada"), make(&mut store, "bob"));
        assert_eq!(store.get(ada).unwrap().display(), "ada", "the handle until one is chosen");
        store.set_display_name(ada, "  Ada Lovelace ").unwrap();
        assert_eq!(store.get(ada).unwrap().display(), "Ada Lovelace");
        assert_eq!(store.set_display_name(ada, "   "), Err(Refused::BadDisplayName));
        assert_eq!(store.set_display_name(ada, &"x".repeat(33)), Err(Refused::BadDisplayName));

        assert_eq!(store.set_password(ada, "$argon2id$new".into()), Ok(2), "everybody signed out");

        store.request(ada, bob).unwrap();
        store.accept(bob, ada).unwrap();
        store.delete(ada).unwrap();
        assert!(store.get(ada).is_none() && store.by_handle("ada").is_none());
        assert!(store.get(bob).unwrap().friends.is_empty(), "not friends with nobody");
        make(&mut store, "ada"); // The handle is free again.
    }

    #[test]
    fn an_old_people_file_still_reads_and_an_account_without_email_is_written_as_before() {
        let dir = temp("old-file");
        let old =
            r#"{"version":1,"next_id":2,"accounts":[{"id":1,"handle":"ada","password":"x","created":1}]}"#;
        std::fs::write(dir.join("people.json"), old).unwrap();
        let mut store = Accounts::open(&dir).unwrap();
        assert!(store.get(1).unwrap().email.is_none() && store.get(1).unwrap().recovery.is_empty());
        store.set_sharing(1, false).unwrap();
        let written = std::fs::read_to_string(dir.join("people.json")).unwrap();
        assert!(!written.contains("email") && !written.contains("recovery"), "{written}");
    }

    #[test]
    fn the_oldest_admin_becomes_the_owner_and_groups_decide_the_rest() {
        let dir = temp("owner");
        let old = r#"{"version":1,"next_id":4,"accounts":[
            {"id":1,"handle":"ada","password":"x","created":1},
            {"id":2,"handle":"bob","password":"x","created":1,"admin":true},
            {"id":3,"handle":"cy","password":"x","created":1,"admin":true}]}"#;
        std::fs::write(dir.join("people.json"), old).unwrap();
        let mut store = Accounts::open(&dir).unwrap();
        assert!(store.get(2).unwrap().owner && !store.get(3).unwrap().owner);
        assert_eq!(store.get(3).unwrap().group, "Admin");
        assert!(store.can(3, "ban") && !store.can(1, "ban"));
        assert_eq!((store.rank(2), store.rank(3), store.rank(1)), (2, 1, 0));

        let mods = Group { name: "Mods".into(), perms: vec!["review".into()] };
        store.set_groups(vec![mods.clone()]).unwrap();
        assert_eq!(store.get(3).unwrap().group, "", "the Admin group is gone, and so is its member's place in it");
        assert!(!store.get(3).unwrap().admin);
        store.set_group(1, "Mods").unwrap();
        assert!(store.can(1, "review") && !store.can(1, "ban") && store.get(1).unwrap().admin);
        assert_eq!(store.set_group(1, "Nobody"), Err(Refused::BadGroups));
        let twice = vec![mods.clone(), Group { name: "mods".into(), perms: vec![] }];
        assert_eq!(store.set_groups(twice), Err(Refused::BadGroups), "a name is used once");
        let unknown = vec![Group { name: "X".into(), perms: vec!["everything".into()] }];
        assert_eq!(store.set_groups(unknown), Err(Refused::BadGroups));

        let store = Accounts::open(&dir).unwrap();
        assert_eq!(store.groups(), &[mods][..], "kept across a restart");
        assert_eq!(store.get(1).unwrap().group, "Mods");
    }

    #[test]
    fn an_unverified_address_is_forgotten_after_a_week_and_a_verified_one_is_not() {
        let mut store = Accounts::open(&temp("unverified")).unwrap();
        let (ada, bob) = (make(&mut store, "ada"), make(&mut store, "bob"));
        store.set_email(ada, Some("ada@example.org"), 0).unwrap();
        store.set_email(bob, Some("bob@example.org"), 0).unwrap();
        assert!(store.by_verified_email("ada@example.org").is_empty(), "not until it is verified");
        assert!(store.verify_email(bob, "BOB@example.org"));
        store.forget_unverified(UNVERIFIED_SECONDS);
        assert!(store.get(ada).unwrap().email.is_some(), "a week is a week");
        store.forget_unverified(UNVERIFIED_SECONDS + 1);
        assert!(store.get(ada).unwrap().email.is_none(), "deleted, not hidden");
        assert_eq!(store.by_verified_email("bob@example.org").len(), 1);
    }
}
