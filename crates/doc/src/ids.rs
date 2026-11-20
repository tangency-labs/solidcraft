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
//! - In a file an id is 13 characters of Crockford's base32, lowercase: [`encode`] writes it and
//!   [`decode`] reads only that spelling ([`solidcraft_geom::ids`], where data the geometry crate
//!   defines can use it too). A field holding an id says `#[serde(with = "crate::ids")]`.

use std::collections::BTreeSet;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

pub use solidcraft_geom::ids::{ID_CHARS, decode, deserialize, encode, serialize};

use crate::{DocError, Result};

/// `#[serde(with = "crate::ids::option")]`: a field holding an id, or nothing.
pub mod option {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(id: &Option<u64>, s: S) -> std::result::Result<S::Ok, S::Error> {
        id.map(super::encode).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<u64>, D::Error> {
        let read = |text: String| super::decode(&text).ok_or_else(|| D::Error::custom(format!("`{text}` is not an id")));
        Option::<String>::deserialize(d)?.map(read).transpose()
    }
}

/// One 128-bit process epoch, shared by document identities and history tokens.
fn process_epoch() -> &'static str {
    static RUN: OnceLock<String> = OnceLock::new();
    RUN.get_or_init(|| {
        let started = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let first = RandomState::new().hash_one(started);
        let second = RandomState::new().hash_one((started, first));
        format!("{first:016x}{second:016x}")
    })
}

/// A design identity: one random run identity followed by a process-wide serial.
/// Persisted in the document, so opening it again preserves its identity.
pub fn new_document_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("{}-{}", process_epoch(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Read a bounded opaque identity before it can be retained or copied by host
/// observers. The spelling is open to other native-document writers.
pub fn deserialize_document_id<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    deserialize_token_part(d, "document identity")
}

/// Read a bounded opaque committed-state revision.
pub fn deserialize_design_revision<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    deserialize_token_part(d, "design revision")
}

fn deserialize_token_part<'de, D: serde::Deserializer<'de>>(d: D, kind: &str) -> std::result::Result<String, D::Error> {
    use serde::{Deserialize, de::Error};

    let identity = String::deserialize(d)?;
    if identity.is_empty() || identity.len() > 128 || !identity.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
        return Err(D::Error::custom(format!("a {kind} must contain 1 to 128 printable ASCII characters")));
    }
    Ok(identity)
}

/// The shared revision allocator used by committed states and design observations.
/// The counter is outside undo snapshots and document files. Its numeric values
/// alone are process-local observations; persistent tokens include the epoch.
pub fn next_design_revision() -> u64 {
    static NEXT: OnceLock<AtomicU64> = OnceLock::new();
    NEXT.get_or_init(|| {
        let seed = IdSource::from_entropy().seed();
        // Keep revisions exactly representable by JSON clients, with another
        // 2^52 increments available before reaching JavaScript's integer limit.
        AtomicU64::new((seed & ((1_u64 << 52) - 1)).max(1))
    })
    .fetch_add(1, Ordering::Relaxed)
}

/// A committed state names both the process epoch and one generation from the
/// existing shared allocator. Reopening preserves the entire token; a later edit
/// uses this process's epoch, including when generations overlap across runs.
pub fn new_design_revision() -> String {
    format!("{}:{}", process_epoch(), next_design_revision())
}

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

    /// Where the source stands: [`IdSource::seeded`] with it gives the ids this one gives next
    /// (a replay's start).
    pub fn seed(&self) -> u64 {
        self.state
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

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> BTreeSet<u64> {
        BTreeSet::new()
    }

    /// Identity and revision metadata survive serialization, while independent new
    /// documents and later allocated states receive distinct tokens.
    #[test]
    fn design_identity_and_revision_survive_serialization() {
        let first = crate::Document::new("First");
        let second = crate::Document::new("Second");
        assert_ne!(first.document_id, second.document_id);
        assert_ne!(first.revision, second.revision);
        let reopened = crate::Document::from_json(&first.to_json()).unwrap();
        assert_eq!((&reopened.document_id, &reopened.revision), (&first.document_id, &first.revision));
        assert_ne!(new_design_revision(), first.revision);
        assert_ne!(new_design_revision(), second.revision);
        let (epoch, generation) = first.revision.split_once(':').unwrap();
        assert_eq!(epoch.len(), 32);
        assert!(epoch.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(generation.parse::<u64>().is_ok());
    }

    #[test]
    fn document_identity_rejects_hostile_values_and_defaults_when_missing() {
        let mut value = serde_json::to_value(crate::Document::new("Identity")).unwrap();
        for invalid in
            ["".to_string(), "x".repeat(129), "line\nfeed".to_string(), "nul\0byte".to_string(), "café".to_string(), "del\u{7f}".to_string()]
        {
            value["document_id"] = serde_json::json!(invalid);
            assert!(crate::Document::from_json(&value.to_string()).is_err(), "invalid identity was accepted");
        }
        value["document_id"] = serde_json::json!("foreign opaque id:0");
        assert_eq!(crate::Document::from_json(&value.to_string()).unwrap().document_id, "foreign opaque id:0");
        value.as_object_mut().unwrap().remove("document_id");
        let legacy = crate::Document::from_json(&value.to_string()).unwrap();
        assert!(!legacy.document_id.is_empty());
        assert!(legacy.document_id.len() <= 128);
    }

    #[test]
    fn design_revision_rejects_hostile_values_and_defaults_when_missing() {
        let mut value = serde_json::to_value(crate::Document::new("Revision")).unwrap();
        for invalid in ["".to_string(), "x".repeat(129), "line\nfeed".to_string(), "nul\0byte".to_string(), "café".to_string()] {
            value["revision"] = serde_json::json!(invalid);
            assert!(crate::Document::from_json(&value.to_string()).is_err(), "invalid revision was accepted");
        }
        value["revision"] = serde_json::json!("foreign opaque revision:7");
        assert_eq!(crate::Document::from_json(&value.to_string()).unwrap().revision, "foreign opaque revision:7");
        value.as_object_mut().unwrap().remove("revision");
        assert!(!crate::Document::from_json(&value.to_string()).unwrap().revision.is_empty());
    }

    /// Every id a source gives is written and read back as itself.
    #[test]
    fn a_sources_ids_are_written_and_read_back() {
        let mut source = IdSource::seeded(1);
        for _ in 0..10_000 {
            let id = source.next(&none()).unwrap();
            assert_eq!(decode(&encode(id)), Some(id), "{id:#x}");
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
