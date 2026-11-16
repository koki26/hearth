//! Six-digit codes sent by email: to confirm an address, and to finish a
//! sign-in (two-step sign-in, or a new IP address). Kept in memory only, as a
//! hash, for [`LIFETIME`]; a restart just means asking for a new one.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};

pub const LIFETIME: u64 = 15 * 60;
/// Wrong guesses before a code is thrown away: 5 in a million.
pub const TRIES: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub kind: &'static str,
    pub account: u64,
    /// What the code is about: the address being confirmed, or the IP
    /// address signing in.
    pub about: String,
    hash: Vec<u8>,
    expires: u64,
    tries: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Wrong {
    /// No such code, or it ran out.
    Gone,
    /// Not the code; it may be tried again.
    NotIt,
    /// Not the code, and that was the last try.
    UsedUp,
}

#[derive(Default)]
pub struct Codes {
    by_ticket: HashMap<String, Pending>,
}

fn hash(code: &str) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, code.trim().as_bytes()).as_ref().to_vec()
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    SystemRandom::new().fill(&mut bytes).expect("the system has randomness");
    bytes
}

impl Codes {
    /// A new code for this account and kind, replacing any earlier one.
    /// Returns the ticket (handed to whoever must type the code) and the code
    /// (only ever put in the email).
    pub fn issue(&mut self, kind: &'static str, account: u64, about: &str, now: u64) -> (String, String) {
        self.by_ticket.retain(|_, p| !(p.kind == kind && p.account == account) && p.expires > now);
        let code = format!("{:06}", u32::from_be_bytes(random::<4>()) % 1_000_000);
        let ticket = URL_SAFE_NO_PAD.encode(random::<18>());
        let pending =
            Pending { kind, account, about: about.to_owned(), hash: hash(&code), expires: now + LIFETIME, tries: 0 };
        self.by_ticket.insert(ticket.clone(), pending);
        (ticket, code)
    }

    /// The code behind `ticket`, used up if `code` is it.
    pub fn check(&mut self, ticket: &str, code: &str, now: u64) -> Result<Pending, Wrong> {
        let found = self.by_ticket.get_mut(ticket).filter(|p| p.expires > now).ok_or(Wrong::Gone)?;
        Self::try_one(found, code).map(|()| self.by_ticket.remove(ticket).expect("just found")).map_err(|wrong| {
            if wrong == Wrong::UsedUp {
                self.by_ticket.remove(ticket);
            }
            wrong
        })
    }

    /// The same, found by account rather than ticket (confirming an address
    /// from the account page, where nobody holds a ticket).
    pub fn check_account(&mut self, kind: &str, account: u64, code: &str, now: u64) -> Result<Pending, Wrong> {
        let ticket = self
            .by_ticket
            .iter()
            .find(|(_, p)| p.kind == kind && p.account == account && p.expires > now)
            .map(|(t, _)| t.clone())
            .ok_or(Wrong::Gone)?;
        self.check(&ticket, code, now)
    }

    fn try_one(pending: &mut Pending, code: &str) -> Result<(), Wrong> {
        if pending.hash == hash(code) {
            return Ok(());
        }
        pending.tries += 1;
        Err(if pending.tries >= TRIES { Wrong::UsedUp } else { Wrong::NotIt })
    }

    /// Forgets every code of this account: its password or email changed.
    pub fn forget(&mut self, account: u64) {
        self.by_ticket.retain(|_, p| p.account != account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_works_once_in_time_and_five_wrong_guesses_end_it() {
        let mut codes = Codes::default();
        let (ticket, code) = codes.issue("sign-in", 7, "203.0.113.9", 0);
        assert_eq!(code.len(), 6);
        assert_eq!(codes.check(&ticket, "nope", 1), Err(Wrong::NotIt));
        let used = codes.check(&ticket, &code, 2).unwrap();
        assert_eq!((used.account, used.about.as_str()), (7, "203.0.113.9"));
        assert_eq!(codes.check(&ticket, &code, 3), Err(Wrong::Gone), "once");

        let (ticket, code) = codes.issue("sign-in", 7, "x", 0);
        assert_eq!(codes.check(&ticket, &code, LIFETIME), Err(Wrong::Gone), "in time");

        let (ticket, code) = codes.issue("verify", 8, "a@b.org", 0);
        for _ in 1..TRIES {
            assert_eq!(codes.check_account("verify", 8, "000000x", 1), Err(Wrong::NotIt));
        }
        assert_eq!(codes.check(&ticket, "000000x", 1), Err(Wrong::UsedUp));
        assert_eq!(codes.check(&ticket, &code, 1), Err(Wrong::Gone), "thrown away after the last try");

        let (old, _) = codes.issue("verify", 9, "a", 0);
        let (_, new) = codes.issue("verify", 9, "b", 0);
        assert_eq!(codes.check(&old, &new, 1), Err(Wrong::Gone), "a new code replaces the old one");
    }
}
