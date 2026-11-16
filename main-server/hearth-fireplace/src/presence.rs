//! Where somebody is: which Flame, which ember.
//!
//! In memory only. It is worthless after a restart -- everybody's client
//! says where it is again within a minute -- and writing it would mean a disk
//! write per heartbeat, for ever, for nothing.
//!
//! It comes from the player's own client, never from a Flame. A Flame that
//! had to tell the Fireplace who was in it would put this server in the path
//! of every match, which `docs/TRUST.md` refuses. The cost is that a client
//! can claim to be somewhere it is not -- it can only lie about itself, and
//! the lie is "I am in the forest" rather than anything anybody can act on.
//!
//! What it says is still checked (`usable_flame`): the address goes into a
//! friend's client and from there into `connect_to_server`, so it is a trust
//! boundary even though the worst case is small.

use std::collections::HashMap;

/// How long a report is good for. A client that stops saying anything -- it
/// crashed, it was unplugged, the player took the headset off and walked
/// away -- goes quiet on its own, with no goodbye needed.
pub const TTL_SECONDS: u64 = 120;
/// The longest address we will carry. A host and a port, nothing more.
pub const MAX_FLAME: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Where {
    /// `host:port` of the Flame, as the client would type it.
    pub flame: String,
    /// Which ember on that Flame.
    pub ember: u32,
    pub ember_name: String,
    pub world: String,
    /// When this was last said, in seconds since the epoch.
    pub at: u64,
}

#[derive(Default)]
pub struct Presence(HashMap<u64, Where>);

impl Presence {
    pub fn report(&mut self, account: u64, now: Where) {
        self.0.insert(account, now);
    }

    pub fn clear(&mut self, account: u64) {
        self.0.remove(&account);
    }

    /// Where an account is, or `None` if it has gone quiet. Staleness is
    /// judged on the way out rather than swept by a timer: there is no
    /// background task to get wrong, and an entry nobody asks about costs a
    /// few dozen bytes.
    pub fn of(&self, account: u64, now: u64) -> Option<&Where> {
        self.0.get(&account).filter(|seen| now.saturating_sub(seen.at) < TTL_SECONDS)
    }

    /// Drops what has gone stale. Called when somebody signs in, which is
    /// often enough to keep the map the size of the people actually playing.
    pub fn sweep(&mut self, now: u64) {
        self.0.retain(|_, seen| now.saturating_sub(seen.at) < TTL_SECONDS);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Is this something we are willing to hand to another player's client? It
/// ends up in `connect_to_server`, so: short, printable, one colon, and
/// nothing that could be a second instruction.
pub fn usable_flame(flame: &str) -> bool {
    if flame.is_empty() || flame.len() > MAX_FLAME {
        return false;
    }
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']' | '_');
    flame.chars().all(ok) && flame.contains(|c: char| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere(at: u64) -> Where {
        Where {
            flame: "192.168.1.20:7777".into(),
            ember: 3,
            ember_name: "the loft".into(),
            world: "forest".into(),
            at,
        }
    }

    #[test]
    fn presence_goes_stale_without_a_heartbeat() {
        let mut here = Presence::default();
        here.report(7, somewhere(1_000));
        assert_eq!(here.of(7, 1_000).map(|w| w.ember), Some(3));
        assert!(here.of(7, 1_000 + TTL_SECONDS - 1).is_some(), "still here a moment before");
        assert!(here.of(7, 1_000 + TTL_SECONDS).is_none(), "and gone after");
        assert_eq!(here.len(), 1, "gone, but not yet swept");
        here.sweep(1_000 + TTL_SECONDS);
        assert!(here.is_empty());
        assert!(here.of(99, 1_000).is_none(), "somebody who never said anything");
    }

    #[test]
    fn leaving_clears_it_at_once() {
        let mut here = Presence::default();
        here.report(7, somewhere(1_000));
        here.clear(7);
        assert!(here.of(7, 1_000).is_none());
    }

    #[test]
    fn an_address_we_would_not_hand_to_a_friend_is_refused() {
        for bad in [
            "",
            "with space:7777",
            "line\nbreak:7777",
            "quote\"s:7777",
            "x".repeat(MAX_FLAME + 1).as_str(),
            "::::",
        ] {
            assert!(!usable_flame(bad), "{bad:?} should not be allowed");
        }
        for good in ["192.168.1.20:7777", "hearth.example.com:7777", "[::1]:7777", "localhost"] {
            assert!(usable_flame(good), "{good:?} should be allowed");
        }
    }
}
