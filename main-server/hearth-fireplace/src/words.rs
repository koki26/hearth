//! Words nobody may put in a handle, a name, pronouns, a bio or a public
//! Flame's name.
//!
//! Text is folded before it is matched: lower case, common look-alikes
//! (`4`→a, `0`→o, `$`→s...) turned back into letters, everything but letters
//! dropped and repeated letters squeezed, so `F.u_u-u©k` and `sh1iit` are
//! caught. A host adds their own words to `data/blocked-words.txt`, one per
//! line.
//!
//! shortcut: substring matching on a short built-in English list; it misses
//! other languages and creative spelling, upgrade to a maintained list or a
//! classifier when people get around it.

use std::path::Path;

/// Matched anywhere in the folded text, so each must not hide inside common
/// words ("class", "therapist").
const BUILT_IN: &[&str] = &[
    "fuck", "fck", "shit", "cunt", "nigger", "nigga", "faggot", "retard", "whore", "slut", "bitch",
    "bastard", "porn", "hentai", "molest", "nazi", "hitler", "wank", "jizz", "dildo", "penis", "vagina",
    "pussy", "nude", "naked", "blowjob", "handjob", "killyourself", "tranny", "chink", "kike", "wetback",
    "onlyfans", "pedophile", "paedophile",
];

/// Only wrong on their own, never inside another word ("grape", "Essex",
/// "document", "torpedo", "spice").
const WHOLE: &[&str] = &[
    "ass", "fag", "fags", "nig", "coon", "dick", "twat", "hoe", "cum", "anal", "sex", "rape", "cock", "pedo",
    "spic", "kys", "tits", "boobs", "dyke", "gook",
];

pub struct Words {
    blocked: Vec<String>,
}

impl Words {
    /// The built-in list and the host's file, if there is one.
    pub fn open(data_dir: &Path) -> Self {
        let mut blocked: Vec<String> = BUILT_IN.iter().map(|w| fold(w, true)).collect();
        if let Ok(text) = std::fs::read_to_string(data_dir.join("blocked-words.txt")) {
            blocked.extend(text.lines().map(|w| fold(w, true)).filter(|w| w.len() >= 3));
        }
        Self { blocked }
    }

    /// Whether `text` has none of the words in it.
    pub fn is_clean(&self, text: &str) -> bool {
        let folded = fold(text, true);
        if self.blocked.iter().any(|w| folded.contains(w.as_str())) {
            return false;
        }
        !text
            .split(|c: char| !c.is_alphanumeric())
            .map(|word| fold(word, false))
            .any(|word| WHOLE.contains(&word.as_str()))
    }
}

/// `squeeze` drops repeated letters, for matching inside longer text.
fn fold(text: &str, squeeze: bool) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        let c = match c {
            '4' | '@' | 'à' | 'á' | 'â' | 'ä' | 'å' => 'a',
            '8' => 'b',
            '3' | 'è' | 'é' | 'ê' | 'ë' | '€' => 'e',
            '1' | '!' | '|' | 'í' | 'ì' | 'ï' => 'i',
            '0' | 'ò' | 'ó' | 'ö' => 'o',
            '5' | '$' => 's',
            '7' | '+' => 't',
            'ù' | 'ú' | 'ü' | 'µ' => 'u',
            '©' => 'c',
            c if c.is_ascii_lowercase() => c,
            _ => continue,
        };
        if !squeeze || !out.ends_with(c) {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disguised_words_are_caught_and_ordinary_ones_are_not() {
        let words = Words { blocked: BUILT_IN.iter().map(|w| fold(w, true)).collect() };
        for bad in ["fuck", "F.u_u-u©k", "sh1iit", "xX_P0RN_Xx", "big ass", "Dick", "n1gg4", "R4PE"] {
            assert!(!words.is_clean(bad), "{bad:?} got through");
        }
        for good in ["Ada Lovelace", "she/her", "class assassin", "Dickens fan", "grape", "Essex", "document",
            "torpedo", "therapist", "peacock", "Bob", "Max xx", "skyscraper", "as good as"]
        {
            assert!(words.is_clean(good), "{good:?} was refused");
        }
    }
}
