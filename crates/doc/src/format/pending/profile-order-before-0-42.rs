//! A pick of sketch profiles by index may save `order: "exact_crossings"` (`{"type": "indices",
//! "indices": [k], "order": "exact_crossings"}`): its numbers mean the numbering builds before
//! 0.42.0 gave the profiles. Profiles of the same size (the lenses where equal circles cross a
//! larger one) are told apart only by the round-off in their areas, and 0.42.0 changed that
//! round-off where two arcs of different circles meet (#16607), so the same index named another
//! region of the same size (#19163). Every index pick of a design saved by 0.41.0 or earlier
//! (format 55 and older) gets the field as the design opens; designs of format 56 and later mean
//! this build's numbering and are left as they are. A pick made or edited later saves no field.
//! The last release opens a design that has the field with a warning, keeps the field it cannot
//! read, and numbers the profiles its own way.

use serde_json::Value;

use crate::Result;

/// The first format saved by a build that closes arc corners as 0.42.0 does.
const ARC_CORNERS_ON_BOTH: u32 = 56;

pub fn step(v: &mut Value) -> Result<()> {
    // The file's own format is still in it: upgrades never rewrite `format`, and a file without
    // one is format 1.
    let version = match v.get("format").and_then(Value::as_str) {
        Some(format) => crate::format::version_of(format),
        None => Some(1),
    };
    if version.is_none_or(|n| n >= ARC_CORNERS_ON_BOTH) {
        return Ok(());
    }
    for feature in v.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        mark_index_picks(feature, 0);
    }
    Ok(())
}

/// Give every profile pick by index in `v` (`{"type": "indices", "indices": [...]}`) the older
/// numbering. Nesting past a design's own depth is not followed.
fn mark_index_picks(v: &mut Value, depth: usize) {
    if depth > 32 {
        return;
    }
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("indices") && o.get("indices").is_some_and(Value::is_array) {
                o.entry("order").or_insert_with(|| Value::String("exact_crossings".into()));
                return;
            }
            o.values_mut().for_each(|x| mark_index_picks(x, depth + 1));
        }
        Value::Array(a) => a.iter_mut().for_each(|x| mark_index_picks(x, depth + 1)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::json;

    fn design(format: &str) -> serde_json::Value {
        json!({"format": format, "features": [
            {"type": "sketch", "id": 1},
            {"type": "extrude", "sketch": 1, "profiles": {"type": "indices", "indices": [3]}},
            {"type": "extrude", "sketch": 1, "profiles": {"type": "curves", "loops": [["c1"]]}},
            {"type": "loft", "sections": [{"sketch": 1, "profiles": {"type": "indices", "indices": [0]}}]},
            {"type": "fillet"},
        ]})
    }

    fn orders(v: &serde_json::Value) -> Vec<serde_json::Value> {
        let f = &v["features"];
        vec![f[1]["profiles"]["order"].clone(), f[2]["profiles"]["order"].clone(), f[3]["sections"][0]["profiles"]["order"].clone()]
    }

    #[test]
    fn only_index_picks_saved_before_format_56_keep_the_older_numbering() {
        for older in ["solidcraft/48", "solidcraft/55"] {
            let mut v = design(older);
            super::step(&mut v).unwrap();
            assert_eq!(orders(&v), [json!("exact_crossings"), json!(null), json!("exact_crossings")], "{older}");
            assert_eq!(v["features"][4], json!({"type": "fillet"}));
        }
        for newer in ["solidcraft/56", "solidcraft/63", "solidcraft/65"] {
            let mut v = design(newer);
            let before = v.clone();
            super::step(&mut v).unwrap();
            assert_eq!(v, before, "{newer}");
        }
        let mut untagged = json!({"features": [{"type": "extrude", "profiles": {"type": "indices", "indices": [1]}}]});
        super::step(&mut untagged).unwrap();
        assert_eq!(untagged["features"][0]["profiles"]["order"], json!("exact_crossings"));
        let mut empty = json!({"format": "solidcraft/40"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"format": "solidcraft/40"}));
    }
}
