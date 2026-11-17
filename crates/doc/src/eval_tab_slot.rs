//! The Tab and Slot feature (#17422): tabs on flat plates that fit slots cut in the flat plates
//! they stand on. This file grows with the doc layer's slice 3b: the picks' checks first, then the
//! settings, the joint planes, the joints' order and the cuts.

use crate::PlatePick;

/// The first key that names more than one of `tab_plates` and `slot_plates` together, or a pick
/// with no key (a key is minted on load, so that is a damaged design). Keys name every tool of a
/// pick's joints: two picks with one key would share their names.
// The eval arm calls it from the next commit of slice 3b.
#[allow(dead_code)]
pub(crate) fn bad_plate_key(tab_plates: &[PlatePick], slot_plates: &[PlatePick]) -> Option<String> {
    let mut seen = std::collections::BTreeSet::new();
    for p in tab_plates.iter().chain(slot_plates) {
        let Some(k) = p.key else {
            return Some(format!("the pick of {} has no key", p.body.0));
        };
        if !seen.insert(k) {
            return Some(format!("two plate picks share key {k}"));
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn pick(key: Option<u32>, body: &str) -> PlatePick {
        PlatePick { key, body: crate::names::BodyName(body.into()), near: None }
    }

    #[test]
    fn tab_slot_bad_plate_key_finds_duplicates_and_missing_keys() {
        assert_eq!(bad_plate_key(&[pick(Some(1), "A")], &[pick(Some(2), "B")]), None);
        assert!(bad_plate_key(&[pick(Some(1), "A")], &[pick(Some(1), "B")]).unwrap().contains("key 1"));
        assert!(bad_plate_key(&[pick(Some(1), "A"), pick(Some(1), "C")], &[]).is_some());
        assert!(bad_plate_key(&[pick(None, "A")], &[]).unwrap().contains("no key"));
    }
}
