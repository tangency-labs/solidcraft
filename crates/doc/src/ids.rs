//! Object ids (#1017, #3890): random, so objects added side by side, on two branches of a
//! repository or in two copies of a file, never take the same id, and no counter changes with
//! every addition. Drawings use them from their first format; designs keep their numbered ids
//! until they move over too (#626 Phase 3).
//!
//! - In memory an id is a nonzero `u64`. 0 is never one (it stands for a design's root
//!   component).
//! - An [`IdSource`] gives them: splitmix64 from a seed, each draw checked against the ids in
//!   use and drawn again when it is one. [`IdSource::seeded`] gives the same ids for the same
//!   seed, for tests and replays, and still a new one in a file that seed made;
//!   [`IdSource::from_entropy`] is what a session uses.
//! - In a file an id is 13 characters of Crockford's base32, lowercase: digits and letters without
//!   i, l, o and u, the first `0` to `f` (`0000000000001`, `fzzzzzzzzzzzz`). [`encode`] writes it
//!   and [`decode`] reads only that spelling, so an id has one form in a file's text. A field
//!   holding an id says `#[serde(with = "crate::ids")]`.

use std::collections::BTreeSet;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Deserializer, Serializer};

use crate::{DocError, Result};

/// The characters of an id in a file: 64 bits in 5-bit digits, the first holding the top four.
pub const ID_CHARS: usize = 13;
/// Crockford's base32 digits, lowercase.
const DIGITS: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
/// splitmix64's step: odd, so the state passes every `u64` before it comes round again.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// Gives ids from a seed.
#[derive(Clone, Debug)]
pub struct IdSource {
    state: u64,
}

impl IdSource {
    /// The same ids for the same seed, for tests and replays.
    pub fn seeded(seed: u64) -> IdSource {
        IdSource { state: seed }
    }

    /// Ids another run gives only by chance: seeded from the standard library's hash keys (random
    /// where the platform gives it randomness), the time, and a count of sources made, so two
    /// sources made together differ.
    pub fn from_entropy() -> IdSource {
        static MADE: AtomicU64 = AtomicU64::new(0);
        let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        IdSource::seeded(RandomState::new().hash_one((nanos, MADE.fetch_add(1, Ordering::Relaxed))))
    }

    /// The next id that isn't 0 and isn't `in_use`. A run never draws a number twice, so at most
    /// one draw per id in use, and one 0, come before a free one.
    pub fn next(&mut self, in_use: &BTreeSet<u64>) -> Result<u64> {
        for _ in 0..in_use.len().saturating_add(2) {
            let id = self.draw();
            if id != 0 && !in_use.contains(&id) {
                return Ok(id);
            }
        }
        Err(DocError::Invalid("no free id".into()))
    }

    /// splitmix64: its mixing is one-to-one, so a run never gives one number twice.
    fn draw(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// `id` as a file writes it: [`ID_CHARS`] base32 digits, the most significant first.
pub fn encode(id: u64) -> String {
    (0..ID_CHARS).rev().map(|k| DIGITS.get(((id >> (5 * k)) & 31) as usize).map_or('0', |&d| char::from(d))).collect()
}

/// The id `text` writes, if it is one in [`encode`]'s spelling: anything else, 0 included, is not.
pub fn decode(text: &str) -> Option<u64> {
    if text.len() != ID_CHARS {
        return None;
    }
    // A first digit above `f` overflows the 64 bits.
    let id = text.bytes().try_fold(0u64, |id, b| id.checked_mul(32)?.checked_add(DIGITS.iter().position(|&d| d == b)? as u64))?;
    (id != 0).then_some(id)
}

/// serde's `with` for an id field: written as [`encode`] writes it, read as [`decode`] reads it.
pub fn serialize<S: Serializer>(id: &u64, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&encode(*id))
}

/// See [`serialize`].
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    let text = String::deserialize(d)?;
    decode(&text).ok_or_else(|| serde::de::Error::custom(format!("an id is {ID_CHARS} characters of lowercase Crockford base32, not all 0")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> BTreeSet<u64> {
        BTreeSet::new()
    }

    /// Every boundary of the spelling: 1, the last value of one digit and the first of two, the
    /// top digit alone and the largest id, and splitmix64's first output for seed 0.
    #[test]
    fn an_id_is_written_as_13_digits_and_read_back() {
        let pinned = [
            (1, "0000000000001"),
            (31, "000000000000z"),
            (32, "0000000000010"),
            (1 << 60, "1000000000000"),
            (u64::MAX, "fzzzzzzzzzzzz"),
            (0xe220_a839_7b1d_cdaf, "e485875xhvkdf"),
        ];
        for (id, text) in pinned {
            assert_eq!((encode(id), decode(text)), (text.to_string(), Some(id)), "{id:#x}");
        }
        let mut source = IdSource::seeded(1);
        for _ in 0..10_000 {
            let id = source.next(&none()).unwrap();
            assert_eq!(decode(&encode(id)), Some(id), "{id:#x}");
        }
    }

    /// Only [`encode`]'s spelling is an id: the wrong length, 0, a first digit past `f`, capitals
    /// (case-sensitive, so an id has one spelling to search for), the letters Crockford leaves
    /// out, and anything not ASCII.
    #[test]
    fn only_the_written_spelling_is_an_id() {
        let not_ids = [
            "",
            "000000000001",
            "00000000000001",
            "0000000000000",
            "g000000000000",
            "zzzzzzzzzzzzz",
            "Fzzzzzzzzzzzz",
            "000000000000A",
            "000000000000i",
            "000000000000l",
            "000000000000o",
            "000000000000u",
            "00000000000é",
            "000000000000-",
            " 000000000001",
            "000000000001\n",
        ];
        for text in not_ids {
            assert_eq!(decode(text), None, "{text:?}");
        }
    }

    /// A seed gives the same ids every time (splitmix64's published first outputs for seed 0), and
    /// a run never gives one twice.
    #[test]
    fn a_seeded_source_repeats_its_ids_and_never_gives_one_twice() {
        let mut zero = IdSource::seeded(0);
        assert_eq!([zero.draw(), zero.draw(), zero.draw()], [0xe220_a839_7b1d_cdaf, 0x6e78_9e6a_a1b9_65f4, 0x06c4_5d18_8009_454f]);
        let run = |seed| {
            let mut source = IdSource::seeded(seed);
            (0..10_000).map(|_| source.next(&none()).unwrap()).collect::<Vec<_>>()
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8));
        assert_eq!(run(7).into_iter().collect::<BTreeSet<_>>().len(), 10_000);
    }

    /// A draw that is in use, or 0, is drawn again: a file the same seed made, reopened with that
    /// seed, collides on every one of its ids in turn and still gets a new one.
    #[test]
    fn a_taken_id_or_0_is_drawn_again() {
        let mut seven = IdSource::seeded(7);
        let made: Vec<u64> = (0..101).map(|_| seven.draw()).collect();
        let in_use: BTreeSet<u64> = made.iter().take(100).copied().collect();
        assert_eq!(IdSource::seeded(7).next(&in_use).unwrap(), made[100]);
        // The state just before 0's: its first draw is 0, and the next is seed 0's first.
        let before_zero = 0u64.wrapping_sub(GAMMA);
        assert_eq!(IdSource::seeded(before_zero).draw(), 0);
        assert_eq!(IdSource::seeded(before_zero).next(&none()).unwrap(), IdSource::seeded(0).draw());
    }

    /// Two sources made together still give different ids.
    #[test]
    fn sources_from_entropy_differ() {
        let (mut a, mut b) = (IdSource::from_entropy(), IdSource::from_entropy());
        assert_ne!(a.next(&none()).unwrap(), b.next(&none()).unwrap());
    }
}
