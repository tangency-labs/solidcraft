//! A flange's or a hem's picked edges are saved by the model's names for them (#6748, #7990), as
//! every feature's edges are: `<face A>|<face B>`, the two faces that meet there in sorted order,
//! each named by the model after what made it. A design saved before keeps them by the sheet's own
//! names, `base:<line>:<side>` and `F<f>.<k>:<edge>:<side>`, which no edge of the model has: a
//! flange or hem that picked one fails, asking for the edge to be picked again. As one opens, each
//! such name of an edge the design says what made is the model's name for that edge:
//!
//! - `base:<line>:top` on the sheet of base flange `F<b>` is the edge where the panel's top meets
//!   its rim on the sketch line `<line>`: `F<b>:side:<line>|F<b>:top`, and `bottom` likewise.
//! - `F<f>.<k>:far:top` is the edge where the `k`th flange of edge flange or hem `F<f>` meets its
//!   far rim with its top. That flange is the one made on the feature's own `k`th edge where each
//!   edge up to it makes one flange (a hem's always, an edge flange's where none of them is split
//!   into segments), and it stands on that edge's rim, `<rim>`: the model's name for the face that
//!   meets the edge, read from the name the feature saved for it. The edge is
//!   `F<f>:far:<rim>|F<f>:top:<rim>`. `side0` and `side1` are the flange's end rims,
//!   `F<f>:end1:<rim>` and `F<f>:end0:<rim>`: the sheet ran an edge with its panel's material on
//!   the left seen from the top, and `side0` was at its start; a bend's anchor runs the edge the
//!   other way, and `end0` is at the anchor's start.
//! - The sheet a feature's names are on is its `body`, where a base flange alone keeps that name
//!   (`body_names`) and nothing converts it; else the design's one sheet, where one base flange
//!   makes every sheet in it (no other flange, conversion or recognition makes one, and no pattern,
//!   mirror or copying move copies a body).
//!
//! Every other name is left, and what picked it fails as it did: a name on a sheet the design does
//! not say; a converted sheet's flange (`F<c>.<k>`, numbered in the order the conversion reached
//! its faces), a fold's or a path flange's panel (numbered as their builds made them), and an edge
//! flange's past an edge split into segments (one flange for each run of segments with the same
//! values, which only evaluating them says); a flange's `bend`, which is no free edge; an edge
//! saved with no name; and an edge along a sketch line in line with another, which the sheet named
//! after the first of the two in the sketch, whichever the edge lay on. The last release warns
//! about the newer format number and opens the design; its sheets find none of these names and
//! take the edges at the picked points, saying so.

use serde_json::Value;

use crate::Result;

/// The features that make a sheet.
const MAKERS: [&str; 5] = ["sheet_base", "sheet_contour", "sheet_convert", "sheet_recognize", "sheet_surface_convert"];

/// The features that make a sheet of a body they convert.
const CONVERSIONS: [&str; 3] = ["sheet_convert", "sheet_recognize", "sheet_surface_convert"];

/// How deep a flange stands on flanges before it, at most, whose names are followed.
const DEPTH: usize = 64;

fn kind(f: &Value) -> Option<&str> {
    f.get("type").and_then(Value::as_str)
}

fn id(f: &Value) -> Option<u64> {
    f.get("id").and_then(Value::as_u64)
}

/// Whether feature `f` may copy a body, and a sheet body's sheet with it: a pattern, a mirror, a
/// move that copies.
fn copies(f: &Value) -> bool {
    matches!(kind(f), Some("pattern" | "mirror")) || (kind(f) == Some("move") && f.get("copy").and_then(Value::as_bool) == Some(true))
}

/// The base flange whose sheet the edges feature `f` picked lie on, as the saved design says: the
/// base flange that alone keeps the name of `f`'s `body`, where nothing converts that body; else
/// the design's base flange, where it makes every sheet of the design and no body is copied.
fn base_of<'a>(features: &'a [Value], f: &Value) -> Option<&'a Value> {
    let base = |x: &Value| kind(x) == Some("sheet_base");
    if let Some(body) = f.get("body").and_then(Value::as_str).filter(|b| !b.is_empty()) {
        let keeps = |x: &&Value| x.get("body_names").and_then(Value::as_array).is_some_and(|n| n.iter().any(|n| n.as_str() == Some(body)));
        let converts = |x: &Value| kind(x).is_some_and(|k| CONVERSIONS.contains(&k)) && x.get("body").and_then(Value::as_str) == Some(body);
        let kept: Vec<&Value> = features.iter().filter(keeps).collect();
        if let [one] = kept.as_slice()
            && base(one)
            && !features.iter().any(converts)
        {
            return Some(*one);
        }
    }
    let makers: Vec<&Value> = features.iter().filter(|x| kind(x).is_some_and(|k| MAKERS.contains(&k))).collect();
    match makers.as_slice() {
        [one] if base(one) && !features.iter().any(copies) => Some(*one),
        _ => None,
    }
}

/// The lines of sketch feature `sketch` that named a base flange's edges (not its construction
/// lines): each one's id and its ends as the sketch saved them.
fn lines(features: &[Value], sketch: u64) -> Vec<(String, [f64; 2], [f64; 2])> {
    let Some(s) = features.iter().find(|f| kind(f) == Some("sketch") && id(f) == Some(sketch)).and_then(|f| f.get("sketch")) else {
        return Vec::new();
    };
    let points = s.get("points").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let at = |c: &Value, end: &str| -> Option<[f64; 2]> {
        let pos = points.get(usize::try_from(c.get(end)?.as_u64()?).ok()?)?.get("pos")?;
        Some([pos.get(0)?.as_f64()?, pos.get(1)?.as_f64()?])
    };
    s.get("curves")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| kind(c) == Some("line") && c.get("construction").and_then(Value::as_bool) != Some(true))
        .filter_map(|c| Some((c.get("id")?.as_str()?.to_string(), at(c, "a")?, at(c, "b")?)))
        .collect()
}

/// Whether the sketch line `line` is one of `lines` and no other lies in line with it: the sheet
/// named an edge of its base after the first line in the sketch whose line both its ends lay on
/// (within 1e-6 mm), so beside another in line (the two sides of a notch) the name says neither.
fn alone(lines: &[(String, [f64; 2], [f64; 2])], line: &str) -> bool {
    let Some((_, a, b)) = lines.iter().find(|(id, _, _)| id == line) else { return false };
    // How far `p` lies off the line through `s` and `e`.
    let off = |p: [f64; 2], s: [f64; 2], e: [f64; 2]| {
        let (tx, ty) = (e[0] - s[0], e[1] - s[1]);
        ((p[0] - s[0]) * ty - (p[1] - s[1]) * tx).abs() / tx.hypot(ty)
    };
    let along = |p: [f64; 2], q: [f64; 2], s: [f64; 2], e: [f64; 2]| off(p, s, e) < 1e-6 && off(q, s, e) < 1e-6;
    (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-9
        && lines.iter().filter(|(id, _, _)| id != line).all(|(_, p, q)| !along(*p, *q, *a, *b) && !along(*a, *b, *p, *q))
}

/// The sheet of base flange `base` made from its sketch's `lines`, with the design's `features`
/// that bent it.
struct Sheet<'a> {
    features: &'a [Value],
    base: u64,
    lines: Vec<(String, [f64; 2], [f64; 2])>,
}

impl Sheet<'_> {
    /// The model's names for the two faces that meet at the edge the sheet named `name`, as the
    /// feature at `at` in the design saved it: the rim along it, and its panel's cap on its side.
    /// None where the saved design does not say which they are.
    fn faces(&self, name: &str, at: usize, depth: usize) -> Option<(String, String)> {
        let (edge, side) = name.rsplit_once(':')?;
        if !matches!(side, "top" | "bottom") || depth > DEPTH {
            return None;
        }
        let b = self.base;
        if let Some(line) = edge.strip_prefix("base:") {
            return alone(&self.lines, line).then(|| (format!("F{b}:side:{line}"), format!("F{b}:{side}")));
        }
        let (tag, rim) = edge.split_once(':')?;
        let (f, k) = tag.strip_prefix('F')?.split_once('.')?;
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
        if !digits(f) || !digits(k) {
            return None;
        }
        let (f, k): (u64, usize) = (f.parse().ok()?, k.parse().ok()?);
        // The flange's end rims are named from its bend's anchor, which runs the edge the other way.
        let rim = match rim {
            "far" => "far",
            "side0" => "end1",
            "side1" => "end0",
            _ => return None,
        };
        // The feature that made the flange, before the one that picked its edge, on the same sheet.
        let made = self.features.get(..at)?.iter().position(|x| id(x) == Some(f))?;
        let feature = self.features.get(made)?;
        if base_of(self.features, feature).is_some_and(|x| id(x) != Some(b)) {
            return None;
        }
        // Its `k`th flange is the one made on its own `k`th edge where each edge up to it makes one.
        let edges = feature.get("edges").and_then(Value::as_array)?;
        if k >= edges.len() {
            return None;
        }
        let whole = |e: &Value| e.get("segments").and_then(Value::as_array).is_none_or(Vec::is_empty);
        let one_each = match kind(feature) {
            Some("sheet_hem") => true,
            Some("sheet_flange") => edges.iter().take(k + 1).all(whole),
            _ => false,
        };
        if !one_each {
            return None;
        }
        let picked = feature.get("edge_names")?.get(k)?.as_str()?;
        let (on, _) = self.faces(picked, made, depth + 1)?;
        Some((format!("F{f}:{rim}:{on}"), format!("F{f}:{side}:{on}")))
    }

    /// The model's name for the edge the sheet named `name`, as the feature at `at` saved it.
    fn renamed(&self, name: &str, at: usize) -> Option<String> {
        let (rim, cap) = self.faces(name, at, 0)?;
        let mut two = [rim, cap];
        two.sort();
        Some(two.join("|"))
    }
}

pub fn step(v: &mut Value) -> Result<()> {
    let Some(features) = v.get("features").and_then(Value::as_array) else { return Ok(()) };
    // Every name is read as the design saved it before any is rewritten: a flange's name follows
    // the name saved for the edge of the flange it stands on.
    let mut renamed: Vec<(usize, usize, String)> = Vec::new();
    for (at, f) in features.iter().enumerate() {
        if !matches!(kind(f), Some("sheet_flange" | "sheet_hem")) {
            continue;
        }
        let Some(names) = f.get("edge_names").and_then(Value::as_array) else { continue };
        let Some(base) = base_of(features, f) else { continue };
        let (Some(b), Some(sketch)) = (id(base), base.get("sketch").and_then(Value::as_u64)) else { continue };
        let sheet = Sheet { features, base: b, lines: lines(features, sketch) };
        for (k, name) in names.iter().enumerate() {
            if let Some(new) = name.as_str().and_then(|n| sheet.renamed(n, at)) {
                renamed.push((at, k, new));
            }
        }
    }
    for (at, k, new) in renamed {
        if let Some(slot) = v.pointer_mut(&format!("/features/{at}/edge_names/{k}")) {
            *slot = Value::String(new);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::{Value, json};

    /// A sketch feature of straight lines, each `(id, start, end, construction)`, between points
    /// saved where the lines end.
    fn sketch(id: u64, lines: &[(&str, [f64; 2], [f64; 2], bool)]) -> Value {
        let mut points = vec![json!({"id": "origin", "pos": [0.0, 0.0], "fixed": true})];
        let mut curves = Vec::new();
        for (line, a, b, construction) in lines {
            points.push(json!({"id": format!("{line}.start"), "pos": a, "fixed": false}));
            points.push(json!({"id": format!("{line}.end"), "pos": b, "fixed": false}));
            curves.push(json!({"id": line, "type": "line", "a": points.len() - 2, "b": points.len() - 1, "construction": construction}));
        }
        json!({"id": id, "name": format!("Sketch{id}"), "type": "sketch", "plane": {"type": "origin", "name": "XY"}, "sketch": {"points": points, "curves": curves, "constraints": []}})
    }

    /// A 100 x 60 rectangle from the origin, its lines `l1` along the front (y = 0) to `l4`.
    const RECTANGLE: [(&str, [f64; 2], [f64; 2], bool); 4] = [
        ("l1", [0.0, 0.0], [100.0, 0.0], false),
        ("l2", [100.0, 0.0], [100.0, 60.0], false),
        ("l3", [100.0, 60.0], [0.0, 60.0], false),
        ("l4", [0.0, 60.0], [0.0, 0.0], false),
    ];

    /// The `edge_names` of each feature once the step has run on a design of `features`.
    fn upgraded(features: Value) -> Vec<Value> {
        let mut v = json!({"features": features});
        super::step(&mut v).unwrap();
        v["features"].as_array().unwrap().iter().map(|f| f["edge_names"].clone()).collect()
    }

    #[test]
    fn a_flanges_and_a_hems_edges_take_the_models_names() {
        let mut lines = RECTANGLE.to_vec();
        lines.push(("l5", [0.0, 30.0], [100.0, 30.0], true));
        let split = json!({"point": [0, 30, 1], "segments": [{"from": 0, "to": 0.5}, {"from": 0.5, "to": 1, "height": "5"}]});
        let names = upgraded(json!([
            sketch(1, &lines),
            {"id": 2, "name": "BaseFlange1", "type": "sheet_base", "sketch": 1, "profiles": {"type": "all"}},
            {"id": 3, "name": "EdgeFlange1", "type": "sheet_flange", "edges": [[50, 0, 1], [100, 30, 0]], "edge_names": ["base:l1:top", "base:l2:bottom"]},
            {"id": 4, "name": "Hem1", "type": "sheet_hem", "edges": [[50, 1, 20], [100, 0, 10], [100, 60, 10]],
             "edge_names": ["F3.0:far:top", "F3.1:side0:bottom", "F3.1:side1:top"]},
            {"id": 5, "name": "EdgeFlange2", "type": "sheet_flange", "edges": [[50, -3, 15]], "edge_names": ["F4.0:far:bottom"]},
            {"id": 6, "name": "EdgeFlange3", "type": "sheet_flange", "edges": [split, [50, 60, 1]], "edge_names": ["base:l4:top", "base:l3:top"]},
            {"id": 7, "name": "Hem2", "type": "sheet_hem", "edges": [[0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0]],
             "edge_names": ["F6.1:far:top", "F6.0:far:top", "F3.0:bend:top", "F9.0:far:top", "base:l5:top", "base:l9:top", "", "F2.0:far:top"]},
            {"id": 8, "name": "Fillet1", "type": "fillet", "edge_names": ["base:l1:top"]},
            {"id": 9, "name": "EdgeFlange4", "type": "sheet_flange", "edges": [[50, 0, 0]], "edge_names": ["base:l1:bottom"]},
        ]));
        // The base flange's top and bottom where they meet its rims on the sketch's lines.
        assert_eq!(names[2], json!(["F2:side:l1|F2:top", "F2:bottom|F2:side:l2"]));
        assert_eq!(names[8], json!(["F2:bottom|F2:side:l1"]));
        // Flange F3.0 is the one on F3's first edge, the base's rim on l1, F3.1 the one on its
        // second, the rim on l2: the first's far rim with its top, and the second's ends, `side0`
        // the model's `end1`, `side1` its `end0`.
        assert_eq!(
            names[3],
            json!(["F3:far:F2:side:l1|F3:top:F2:side:l1", "F3:bottom:F2:side:l2|F3:end1:F2:side:l2", "F3:end0:F2:side:l2|F3:top:F2:side:l2"])
        );
        // A flange on the hem's far edge: the hem stands on F3's far rim, as its own name says.
        assert_eq!(names[4], json!(["F4:bottom:F3:far:F2:side:l1|F4:far:F3:far:F2:side:l1"]));
        // A split edge's own name is the edge's.
        assert_eq!(names[5], json!(["F2:side:l4|F2:top", "F2:side:l3|F2:top"]));
        // Left: F6's flanges from its split edge on (it makes one for each run of equal segments),
        // a bend, a flange of a feature after the hem, a construction line, a line the sketch does
        // not have, no name, and a flange of the base flange, which bends none; and a fillet's
        // edges, which are no sheet's.
        assert_eq!(
            names[6],
            json!(["F6.1:far:top", "F6.0:far:top", "F3.0:bend:top", "F9.0:far:top", "base:l5:top", "base:l9:top", "", "F2.0:far:top"])
        );
        assert_eq!(names[7], json!(["base:l1:top"]));
        assert_eq!(names[0], Value::Null);
    }

    #[test]
    fn names_on_a_sheet_the_design_does_not_say_or_along_a_notch_are_left() {
        let base = |id: u64, sketch: u64, body: Option<&str>| {
            let mut f = json!({"id": id, "name": format!("BaseFlange{id}"), "type": "sheet_base", "sketch": sketch, "profiles": {"type": "all"}});
            if let Some(body) = body {
                f["body_names"] = json!([body]);
            }
            f
        };
        let flange = |id: u64, body: Option<&str>, name: &str| {
            let mut f = json!({"id": id, "name": format!("EdgeFlange{id}"), "type": "sheet_flange", "edges": [[50, 0, 1]], "edge_names": [name]});
            if let Some(body) = body {
                f["body"] = json!(body);
            }
            f
        };
        // Two sheets: a flange on the body whose name only base flange 2 keeps is on F2's sheet, and
        // so is the flange on that flange; one that names no body, or a body whose name no feature
        // keeps (base flange 4's), could be on either.
        let names = upgraded(json!([
            sketch(1, &RECTANGLE),
            base(2, 1, Some("Plate")),
            sketch(3, &RECTANGLE),
            base(4, 3, None),
            flange(5, Some("Plate"), "base:l1:top"),
            flange(6, None, "base:l1:top"),
            flange(7, Some("Body2"), "base:l1:top"),
            flange(8, Some("Plate"), "F5.0:far:top"),
        ]));
        assert_eq!(names[4], json!(["F2:side:l1|F2:top"]));
        assert_eq!(names[5], json!(["base:l1:top"]));
        assert_eq!(names[6], json!(["base:l1:top"]));
        assert_eq!(names[7], json!(["F5:far:F2:side:l1|F5:top:F2:side:l1"]));
        // One base flange, and a mirror that may have copied its body: the body's name says the
        // sheet, no name does not.
        let names = upgraded(json!([
            sketch(1, &RECTANGLE),
            base(2, 1, Some("Plate")),
            {"id": 3, "name": "Mirror1", "type": "mirror", "bodies": ["Plate"]},
            flange(4, None, "base:l1:top"),
            flange(5, Some("Plate"), "base:l1:top"),
        ]));
        assert_eq!((&names[3], &names[4]), (&json!(["base:l1:top"]), &json!(["F2:side:l1|F2:top"])));
        // A front edge notched between x = 40 and 60: l1 and l5 lie in line, and the sheet named
        // the edges along both after l1. The notch's own sides and the others are each alone.
        let notched = [
            ("l1", [0.0, 0.0], [40.0, 0.0], false),
            ("l2", [40.0, 0.0], [40.0, 20.0], false),
            ("l3", [40.0, 20.0], [60.0, 20.0], false),
            ("l4", [60.0, 20.0], [60.0, 0.0], false),
            ("l5", [60.0, 0.0], [100.0, 0.0], false),
            ("l6", [100.0, 0.0], [100.0, 60.0], false),
            ("l7", [100.0, 60.0], [0.0, 60.0], false),
            ("l8", [0.0, 60.0], [0.0, 0.0], false),
        ];
        let names = upgraded(json!([
            sketch(1, &notched),
            base(2, 1, None),
            {"id": 3, "name": "EdgeFlange3", "type": "sheet_flange", "edges": [[80, 0, 1], [100, 30, 1], [50, 20, 1]],
             "edge_names": ["base:l1:top", "base:l6:top", "base:l3:top"]},
        ]));
        assert_eq!(names[2], json!(["base:l1:top", "F2:side:l6|F2:top", "F2:side:l3|F2:top"]));
        // A converted sheet: its flanges were numbered in the order the conversion reached its
        // faces, and the design's one sheet is no base flange's.
        let names = upgraded(json!([
            {"id": 1, "name": "Box1", "type": "box", "body_names": ["Tray"]},
            {"id": 2, "name": "C1", "type": "sheet_convert", "body": "Tray", "face": [50, 30, 0], "bends": [[50, 0, 0]], "edge_names": ["F1:-y|F1:-z"]},
            flange(3, Some("Tray"), "F2.0:far:top"),
            flange(4, None, "F2.0:far:top"),
        ]));
        assert_eq!(names[1], json!(["F1:-y|F1:-z"]), "a conversion's bend edges are its body's");
        assert_eq!((&names[2], &names[3]), (&json!(["F2.0:far:top"]), &json!(["F2.0:far:top"])));
    }
}
