//! How often one address, or one handle, may try something.
//!
//! Fixed windows in a map. Not a token bucket and not per-route middleware:
//! the three things worth limiting here are sign-in, making an account and
//! asking to be friends, and a window that resets is easy to reason about
//! when somebody writes in to say they are locked out.
//!
//! It lives in memory, so a restart forgives everybody. That is the right
//! trade at this size: the alternative is writing a counter to disk on every
//! failed password, which is a way to be attacked rather than a defence.

use std::collections::HashMap;

/// Making accounts: ten an hour from one address, which is a household or a
/// LAN party rather than a bot farm. Configurable, because only the person
/// running a Fireplace knows which of those they are expecting.
pub const ACCOUNTS_PER_HOUR: u32 = 10;
/// Signing in: ten tries in five minutes, counted per address **and** per
/// handle, so somebody spread across a botnet still cannot grind one account.
pub const SIGN_INS: u32 = 10;
pub const SIGN_IN_WINDOW: u64 = 5 * 60;
pub const HOUR: u64 = 60 * 60;
/// Asking to be friends: enough for anybody, few enough that the button is
/// not a way to fill somebody's page.
pub const REQUESTS_PER_HOUR: u32 = 60;
/// When the map is bigger than this, the stale windows are dropped. A number
/// rather than a timer: there is no task to get wrong, and the cost is paid
/// by whoever made it big.
const SWEEP_AT: usize = 10_000;

#[derive(Default)]
pub struct Limiter {
    /// key -> (tries in this window, when the window started)
    windows: HashMap<String, (u32, u64)>,
}

impl Limiter {
    /// Counts one try and says whether it is allowed. A refused try still
    /// counts: otherwise the limit is only a limit on successes.
    pub fn allow(&mut self, key: &str, limit: u32, window: u64, now: u64) -> bool {
        if self.windows.len() > SWEEP_AT {
            self.windows.retain(|_, (_, started)| now.saturating_sub(*started) < window);
        }
        let entry = self.windows.entry(key.to_owned()).or_insert((0, now));
        if now.saturating_sub(entry.1) >= window {
            *entry = (0, now);
        }
        entry.0 += 1;
        entry.0 <= limit
    }

    /// For the test and for the log line: how many tries are on this key.
    pub fn tries(&self, key: &str) -> u32 {
        self.windows.get(key).map(|(n, _)| *n).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_many_sign_in_tries_are_refused_until_the_window_turns_over() {
        let mut limits = Limiter::default();
        for try_number in 1..=SIGN_INS {
            assert!(limits.allow("ip:1.2.3.4", SIGN_INS, SIGN_IN_WINDOW, 100), "try {try_number}");
        }
        assert!(!limits.allow("ip:1.2.3.4", SIGN_INS, SIGN_IN_WINDOW, 100), "one too many");
        // Somebody else is unaffected.
        assert!(limits.allow("ip:5.6.7.8", SIGN_INS, SIGN_IN_WINDOW, 100));
        // And the window turns over.
        assert!(limits.allow("ip:1.2.3.4", SIGN_INS, SIGN_IN_WINDOW, 100 + SIGN_IN_WINDOW));
        assert_eq!(limits.tries("ip:1.2.3.4"), 1, "a fresh window counts from one");
    }

    #[test]
    fn a_big_map_is_swept_rather_than_grown_for_ever() {
        let mut limits = Limiter::default();
        for i in 0..=SWEEP_AT {
            limits.allow(&format!("ip:{i}"), 100, 60, 0);
        }
        // Long after every window has turned over, one more call clears them.
        limits.allow("ip:fresh", 100, 60, 10_000);
        assert!(limits.tries("ip:0") == 0, "the old windows are gone");
    }
}
