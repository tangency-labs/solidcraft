//! A sheet names none of its faces itself (#6748, #7990): the model names a sheet's faces after
//! what made each, as it names every feature's. A design saved before keeps a face of a sheet by
//! the sheet's own name for it, `sheet:<body>:<panel>:<side>`: the face a sketch or a plane is
//! attached to (also beneath an offset or a rotation), or a feature picked. As one opens, each
//! such name of a face the design says what made is the model's name for that face. The panel of
//! a base flange's sheet (`base`) is the base flange `F<b>`'s: `base:top` is `F<b>:top`,
//! `base:bottom` is `F<b>:bottom`, and its rim along the base sketch's line `l1`, `base:rim.l1`,
//! is `F<b>:side:l1`. The panel of a sheet converted with bend edges by `F<c>` is the face it
//! fixed, which the conversion keeps by the model's name for it, `<face>`: `base:bottom` is that
//! face, `<face>`, and `base:top`, a thickness into the material, `F<c>:bottom:<face>`. Every other
//! name is left, and what is attached by one reports a lost attachment, to be picked again: a
//! flange's panel (`F<f>.<k>`, the `k`th the feature made) or an untagged one (`#<i>`), a rim
//! named by its place in the outline (`rim.edge<k>`) and a contour flange's first panel are named
//! by an order, the unfolded sheet (`flat`) by where on it a face lies, and a plate conversion
//! kept its face by a point alone: the design keeps none of these as a name the model gives. The
//! last release warns about the newer format number and opens the design.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{DocError, Result};

/// Maximum total `body_names` entries inspected across all features.
const MAX_BODY_NAMES: usize = 200_000;
/// Maximum total base-sketch line ids indexed across all distinct base sketches.
const MAX_TOTAL_SKETCH_LINES: usize = 1_000_000;
/// Maximum JSON recursion depth when rewriting face names.
const MAX_DEPTH: usize = 128;

/// What made the panel `base` of a sheet body, as the saved design says: a base flange (its id,
/// and its base sketch's id whose lines are stored once in [`Bases::sketch_lines`]), or a
/// conversion with bend edges (its id, and the model's name for the face it fixed).
enum Base {
    Flange { feature: u64, sketch: u64 },
    Converted { feature: u64, face: String },
}

enum Keeper<'a> {
    One(&'a Value),
    Many { has_base: bool },
}

enum Conversion<'a> {
    One(&'a Value),
    Many,
}

struct Bases {
    bodies: HashMap<String, Base>,
    sketch_lines: HashMap<u64, HashSet<String>>,
}

fn kind(f: &Value) -> Option<&str> {
    f.get("type").and_then(Value::as_str)
}

fn id(f: &Value) -> Option<u64> {
    f.get("id").and_then(Value::as_u64)
}

/// The ids of the lines of sketch feature `sketch` (construction lines name no rim).
fn lines_of(sketch: Option<&Value>) -> Result<HashSet<String>> {
    let Some(curves) = sketch.and_then(|f| f.pointer("/sketch/curves")).and_then(Value::as_array) else {
        return Ok(HashSet::new());
    };
    if curves.len() > solidcraft_sketch::CEILING_CURVES {
        return Err(DocError::Invalid("damaged design: sketch too large".into()));
    }
    Ok(curves
        .iter()
        .filter(|c| c.get("type").and_then(Value::as_str) == Some("line") && c.get("construction").and_then(Value::as_bool) != Some(true))
        .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_string))
        .collect())
}

/// What made the panel `base` of each body the design says it of: a body whose name only a base
/// flange keeps (`body_names`) and no conversion converts; a body one conversion with bend edges
/// converts, with the model's name for its fixed face, and whose name no base flange keeps. A
/// body two features could have made is neither.
fn bases(v: &Value) -> Result<Bases> {
    let features = v.get("features").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    if features.len() > crate::document::MAX_FEATURES {
        return Err(DocError::Invalid("document too large".into()));
    }
    // The features keeping each body's name, the conversions of each body, and each sketch by id.
    let mut keepers: HashMap<&str, Keeper<'_>> = HashMap::new();
    let mut conversions: HashMap<&str, Conversion<'_>> = HashMap::new();
    let mut sketches: HashMap<u64, &Value> = HashMap::new();
    let mut total_body_names = 0usize;
    for f in features {
        let is_base = kind(f) == Some("sheet_base");
        if kind(f) == Some("sketch")
            && let Some(sid) = id(f)
        {
            sketches.entry(sid).or_insert(f);
        }
        if let Some(names) = f.get("body_names").and_then(Value::as_array) {
            total_body_names = total_body_names.saturating_add(names.len());
            if total_body_names > MAX_BODY_NAMES {
                return Err(DocError::Invalid("too many body names in design".into()));
            }
            for name in names.iter().filter_map(Value::as_str) {
                match keepers.entry(name) {
                    Entry::Vacant(entry) => {
                        entry.insert(Keeper::One(f));
                    }
                    Entry::Occupied(mut entry) => {
                        let has_base = match entry.get() {
                            Keeper::One(prev) => kind(prev) == Some("sheet_base") || is_base,
                            Keeper::Many { has_base } => *has_base || is_base,
                        };
                        entry.insert(Keeper::Many { has_base });
                    }
                }
            }
        }
        if kind(f) == Some("sheet_convert")
            && let Some(body) = f.get("body").and_then(Value::as_str)
        {
            match conversions.entry(body) {
                Entry::Vacant(entry) => {
                    entry.insert(Conversion::One(f));
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(Conversion::Many);
                }
            }
        }
    }
    let mut bodies = HashMap::new();
    let mut sketch_lines: HashMap<u64, HashSet<String>> = HashMap::new();
    let mut total_lines = 0usize;
    for (&body, kept) in &keepers {
        if let Keeper::One(f) = kept
            && !conversions.contains_key(body)
            && kind(f) == Some("sheet_base")
            && let (Some(feature), Some(sketch)) = (id(f), f.get("sketch").and_then(Value::as_u64))
        {
            if let Entry::Vacant(entry) = sketch_lines.entry(sketch) {
                let set = lines_of(sketches.get(&sketch).copied())?;
                total_lines = total_lines.saturating_add(set.len());
                if total_lines > MAX_TOTAL_SKETCH_LINES {
                    return Err(DocError::Invalid("too many base sketch lines during format upgrade".into()));
                }
                entry.insert(set);
            }
            bodies.insert(body.to_string(), Base::Flange { feature, sketch });
        }
    }
    for (&body, converted) in &conversions {
        let flange = keepers.get(body).is_some_and(|kept| match kept {
            Keeper::One(f) => kind(f) == Some("sheet_base"),
            Keeper::Many { has_base } => *has_base,
        });
        if let Conversion::One(f) = converted
            && !flange
            && f.get("bends").and_then(Value::as_array).is_some_and(|b| !b.is_empty())
            && let (Some(feature), Some(face)) = (id(f), f.pointer("/face_names/0").and_then(Value::as_str))
            && !face.is_empty()
            && !face.starts_with('?')
        {
            bodies.insert(body.to_string(), Base::Converted { feature, face: face.to_string() });
        }
    }
    Ok(Bases { bodies, sketch_lines })
}

/// The body, panel and side of a sheet's own name for a face, `sheet:<body>:<panel>:<side>`: a
/// body's name may hold a colon, a panel's and a side's never do.
fn parts(name: &str) -> Option<(&str, &str, &str)> {
    let rest = name.strip_prefix("sheet:")?;
    let (head, side) = rest.rsplit_once(':')?;
    let (body, panel) = head.rsplit_once(':')?;
    (!body.is_empty() && !panel.is_empty() && !side.is_empty()).then_some((body, panel, side))
}

/// The model's name for the face of a sheet the sheet's own name `name` names, where the design
/// says what made its panel (see the note above); none for any other name.
fn renamed(name: &str, bases: &Bases) -> Option<String> {
    let (body, "base", side) = parts(name)? else { return None };
    match bases.bodies.get(body)? {
        Base::Flange { feature, sketch } => match side.strip_prefix("rim.") {
            None if matches!(side, "top" | "bottom") => Some(format!("F{feature}:{side}")),
            Some(line) if bases.sketch_lines.get(sketch).is_some_and(|lines| lines.contains(line)) => Some(format!("F{feature}:side:{line}")),
            _ => None,
        },
        Base::Converted { feature, face } => match side {
            "bottom" => Some(face.clone()),
            "top" => Some(format!("F{feature}:bottom:{face}")),
            _ => None,
        },
    }
}

/// `n` through `rename`, where it is a name `rename` gives another.
fn rename_in(n: &mut Value, rename: &dyn Fn(&str) -> Option<String>) {
    if let Value::String(s) = n
        && let Some(r) = rename(s.as_str())
    {
        *s = r;
    }
}

/// Each name of a face in `v` through `rename`: a feature's `face_names`, and the `name` of every
/// face reference (tagged `"type": "face"`, picked `at` a point: a sketch's or a plane's face,
/// also beneath an offset or a rotation, and a construction reference's).
fn faces(v: &mut Value, rename: &dyn Fn(&str) -> Option<String>, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(DocError::Invalid("damaged design: nested too deeply".into()));
    }
    match v {
        Value::Object(o) => {
            let reference = o.get("type").and_then(Value::as_str) == Some("face") && o.contains_key("at");
            for (key, x) in o.iter_mut() {
                if reference && key == "name" {
                    rename_in(x, rename);
                } else if key == "face_names"
                    && let Value::Array(names) = x
                {
                    names.iter_mut().for_each(|n| rename_in(n, rename));
                } else {
                    faces(x, rename, depth + 1)?;
                }
            }
        }
        Value::Array(a) => {
            for x in a.iter_mut() {
                faces(x, rename, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn step(v: &mut Value) -> Result<()> {
    let made = bases(v)?;
    if !made.bodies.is_empty() {
        faces(v, &|name| renamed(name, &made), 0)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::{Value, json};

    #[test]
    fn the_faces_of_a_base_flanges_panel_and_a_converted_fixed_face_take_the_models_names() {
        let face = |name: &str| -> Value { json!({"type": "face", "plane": {}, "at": [0, 0, 0], "name": name}) };
        let curves = json!([
            {"id": "l1", "type": "line", "a": 1, "b": 2, "construction": false},
            {"id": "l2", "type": "line", "a": 2, "b": 3, "construction": true},
            {"id": "a1", "type": "arc", "a": 3, "b": 1},
        ]);
        let picked = json!(["sheet:Plate:base:rim.l1", "sheet:Plate:base:rim.l2", "sheet:Plate:base:rim.edge0", "sheet:Plate:F6.0:top", ""]);
        let mut v = json!({
            "features": [
                {"id": 1, "name": "Base", "type": "sketch", "plane": {"type": "origin", "name": "XY"}, "sketch": {"curves": curves}},
                {"id": 2, "name": "Flange1", "type": "sheet_base", "sketch": 1, "body_names": ["Plate"]},
                {"id": 3, "name": "Top", "type": "sketch", "plane": face("sheet:Plate:base:top")},
                {"id": 4, "name": "Below", "type": "sketch", "plane": {"type": "offset", "base": face("sheet:Plate:base:bottom"), "distance": "5"}},
                {"id": 5, "name": "Hole1", "type": "hole", "face_names": picked},
                {"id": 6, "name": "Flange2", "type": "sheet_flange", "edge_names": ["base:l1:top"]},
                {"id": 7, "name": "Box1", "type": "box", "body_names": ["Tray"]},
                {"id": 8, "name": "C1", "type": "sheet_convert", "body": "Tray", "face": [50, 30, 0], "bends": [[50, 0, 0]], "face_names": ["F7:-z"]},
                {"id": 9, "name": "sheet:Plate:base:top", "type": "construction_plane", "plane": face("sheet:Tray:base:bottom")},
                {"id": 10, "name": "Inside", "type": "sketch", "plane": face("sheet:Tray:base:top")},
                {"id": 11, "name": "Flange3", "type": "sheet_base", "sketch": 1, "body_names": ["Twice"]},
                {"id": 12, "name": "Join1", "type": "join", "body_names": ["Twice"]},
                {"id": 13, "name": "Convert2", "type": "sheet_convert", "body": "Slab", "face": [0, 0, 1]},
                {"id": 14, "name": "Late", "type": "sketch", "plane": face("sheet:Twice:base:top")},
                {"id": 15, "name": "OnSlab", "type": "sketch", "plane": face("sheet:Slab:base:top")},
            ],
        });
        super::step(&mut v).unwrap();
        let f = &v["features"];
        // Plate is base flange 2's: its panel's caps, and its rim on the sketch's line l1 (not on
        // construction line l2, nor one named by its place in the outline, nor a flange's panel).
        assert_eq!(f[2]["plane"]["name"], json!("F2:top"));
        assert_eq!(f[3]["plane"]["base"]["name"], json!("F2:bottom"));
        assert_eq!(f[4]["face_names"], json!(["F2:side:l1", "sheet:Plate:base:rim.l2", "sheet:Plate:base:rim.edge0", "sheet:Plate:F6.0:top", ""]));
        assert_eq!(f[5]["edge_names"], json!(["base:l1:top"]), "an edge's name is no face's");
        // Tray is converted 8's from its face F7:-z: that face, and a thickness into the material
        // the conversion's cap after it.
        assert_eq!(f[8]["plane"]["name"], json!("F7:-z"));
        assert_eq!(f[8]["name"], json!("sheet:Plate:base:top"), "a feature's own name is no face's");
        assert_eq!(f[9]["plane"]["name"], json!("F8:bottom:F7:-z"));
        assert_eq!(f[7]["face_names"], json!(["F7:-z"]));
        // Left: a body two features keep the name of, and a plate converted from a face it kept no
        // name for.
        assert_eq!(f[13]["plane"]["name"], json!("sheet:Twice:base:top"));
        assert_eq!(f[14]["plane"]["name"], json!("sheet:Slab:base:top"));
    }

    #[test]
    fn many_body_names_share_base_sketch_lines_without_cloning_and_caps_reject_crafted_files() {
        let curves: Vec<Value> = (0..2_000).map(|i| json!({"id": format!("l{i}"), "type": "line", "a": 1, "b": 2, "construction": false})).collect();
        let body_names: Vec<Value> = (0..2_000).map(|i| json!(format!("Plate{i}"))).collect();
        let mut v = json!({
            "features": [
                {"id": 1, "name": "Base", "type": "sketch", "plane": {"type": "origin", "name": "XY"}, "sketch": {"curves": curves}},
                {"id": 2, "name": "Flange1", "type": "sheet_base", "sketch": 1, "body_names": body_names},
                {"id": 3, "name": "Hole1", "type": "hole", "face_names": ["sheet:Plate0:base:rim.l0", "sheet:Plate1999:base:rim.l1999", "sheet:Plate1999:base:rim.missing"]},
            ],
        });
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][2]["face_names"], json!(["F2:side:l0", "F2:side:l1999", "sheet:Plate1999:base:rim.missing"]));

        // Oversized body_names array is rejected cleanly.
        let too_many_bodies = vec![json!("B"); super::MAX_BODY_NAMES + 1];
        let mut bloated = json!({
            "features": [{"id": 1, "type": "sheet_base", "sketch": 1, "body_names": too_many_bodies}],
        });
        assert!(super::step(&mut bloated).is_err());

        // Oversized feature list is rejected cleanly.
        let too_many_features = vec![json!({"id": 1, "type": "box"}); crate::document::MAX_FEATURES + 1];
        let mut oversized = json!({"features": too_many_features});
        assert!(super::step(&mut oversized).is_err());
    }
}
