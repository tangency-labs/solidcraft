//! An object id's text form (#1017, #3890): in memory an id is a nonzero `u64`, and in a file it is
//! 13 characters of Crockford's base32, lowercase: digits and letters without i, l, o and u, the
//! first `0` to `f` (`0000000000001`, `fzzzzzzzzzzzz`). [`encode`] writes it and [`decode`] reads
//! only that spelling, so an id has one form in a file's text. A field holding an id says
//! `#[serde(with = "solidcraft_geom::ids")]`, or [`option`] for one that may be none.
//!
//! It is here, at the bottom, so data this crate defines and a drawing saves (a revision record's
//! rows) writes its ids as every drawing id is written. `solidcraft_doc::ids` gives the ids.

use serde::{Deserialize, Deserializer, Serializer};

/// The characters of an id in a file: 64 bits in 5-bit digits, the first holding the top four.
pub const ID_CHARS: usize = 13;
/// Crockford's base32 digits, lowercase.
const DIGITS: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

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
pub fn serialize<S: Serializer>(id: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&encode(*id))
}

/// See [`serialize`].
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let text = String::deserialize(d)?;
    decode(&text).ok_or_else(|| serde::de::Error::custom(format!("an id is {ID_CHARS} characters of lowercase Crockford base32, not all 0")))
}

/// serde's `with` for a field that holds an id or none: none is written as `null`.
pub mod option {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(id: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
        match id {
            Some(id) => super::serialize(id, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        #[derive(Deserialize)]
        struct Id(#[serde(with = "crate::ids")] u64);
        Ok(Option::<Id>::deserialize(d)?.map(|Id(id)| id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// An id or none: none is `null`, an id its text; anything else is refused.
    #[test]
    fn an_optional_id_is_null_or_its_text() {
        #[derive(Debug, PartialEq, serde::Serialize, Deserialize)]
        struct Holder(#[serde(with = "option")] Option<u64>);
        for (held, text) in [(Holder(None), "null"), (Holder(Some(32)), "\"0000000000010\"")] {
            assert_eq!(serde_json::to_string(&held).unwrap(), text);
            assert_eq!(serde_json::from_str::<Holder>(text).unwrap(), held);
        }
        for bad in ["\"0000000000000\"", "32", "\"\""] {
            assert!(serde_json::from_str::<Holder>(bad).is_err(), "{bad}");
        }
    }
}
