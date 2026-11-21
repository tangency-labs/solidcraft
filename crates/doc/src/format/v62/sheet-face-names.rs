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

use std::collections::BTreeMap;

use serde_json::Value;

use crate::Result;

/// What made the panel `base` of a sheet body, as the saved design says: a base flange (its id,
/// and the ids of its sketch's lines, which named the panel's rims), or a conversion with bend
/// edges (its id, and the model's name for the face it fixed).
enum Base {
    Flange { feature: u64, lines: Vec<String> },
    Converted { feature: u64, face: String },
}

fn kind(f: &Value) -> Option<&str> {
    f.get("type").and_then(Value::as_str)
}

fn id(f: &Value) -> Option<u64> {
    f.get("id").and_then(Value::as_u64)
}

/// The ids of the lines of sketch feature `sketch` (construction lines name no rim).
fn lines(features: &[Value], sketch: u64) -> Vec<String> {
    let curves = features.iter().find(|f| kind(f) == Some("sketch") && id(f) == Some(sketch)).and_then(|f| f.pointer("/sketch/curves"));
    curves
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| c.get("type").and_then(Value::as_str) == Some("line") && c.get("construction").and_then(Value::as_bool) != Some(true))
        .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// What made the panel `base` of each body the design says it of: a body whose name only a base
/// flange keeps (`body_names`) and no conversion converts; a body one conversion with bend edges
/// converts, with the model's name for its fixed face, and whose name no base flange keeps. A
/// body two features could have made is neither.
fn bases(v: &Value) -> BTreeMap<String, Base> {
    let features = v.get("features").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    // The features keeping each body's name, and the conversions of each body.
    let mut keepers: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    let mut conversions: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for f in features {
        for name in f.get("body_names").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            keepers.entry(name).or_default().push(f);
        }
        if kind(f) == Some("sheet_convert")
            && let Some(body) = f.get("body").and_then(Value::as_str)
        {
            conversions.entry(body).or_default().push(f);
        }
    }
    let mut out = BTreeMap::new();
    for (body, kept) in &keepers {
        if let ([f], None) = (kept.as_slice(), conversions.get(body))
            && kind(f) == Some("sheet_base")
            && let (Some(feature), Some(sketch)) = (id(f), f.get("sketch").and_then(Value::as_u64))
        {
            out.insert(body.to_string(), Base::Flange { feature, lines: lines(features, sketch) });
        }
    }
    for (body, converted) in &conversions {
        let flange = keepers.get(body).is_some_and(|kept| kept.iter().any(|f| kind(f) == Some("sheet_base")));
        if let [f] = converted.as_slice()
            && !flange
            && f.get("bends").and_then(Value::as_array).is_some_and(|b| !b.is_empty())
            && let (Some(feature), Some(face)) = (id(f), f.pointer("/face_names/0").and_then(Value::as_str))
            && !face.is_empty()
            && !face.starts_with('?')
        {
            out.insert(body.to_string(), Base::Converted { feature, face: face.to_string() });
        }
    }
    out
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
fn renamed(name: &str, bases: &BTreeMap<String, Base>) -> Option<String> {
    let (body, "base", side) = parts(name)? else { return None };
    match bases.get(body)? {
        Base::Flange { feature, lines } => match side.strip_prefix("rim.") {
            None if matches!(side, "top" | "bottom") => Some(format!("F{feature}:{side}")),
            Some(line) if lines.iter().any(|l| l == line) => Some(format!("F{feature}:side:{line}")),
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
fn faces(v: &mut Value, rename: &dyn Fn(&str) -> Option<String>) {
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
                    faces(x, rename);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| faces(x, rename)),
        _ => {}
    }
}

pub fn step(v: &mut Value) -> Result<()> {
    let made = bases(v);
    if !made.is_empty() {
        faces(v, &|name| renamed(name, &made));
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
}
