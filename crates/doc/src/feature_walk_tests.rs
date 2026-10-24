//! Golden records of what the walks over a feature report, for every variant (and every fillet
//! style, pattern, hole and construction definition):
//! - expressions: `inputs` (label, expression, kind, in order: the order names the `dN`
//!   parameters) and `expressions` (every expression, placement points included). Each
//!   expression field holds its own name, so a moved, missing or relabelled input shows;
//! - references: the sketches a feature is built from, and the fields `remap_refs` and
//!   `remap_bodies` rewrite when everything is copied under new ids and names;
//! - geometry: the fields `transform_geometry` moves, the face points, the profile selections and
//!   the placement point.
//!
//! The records live in `feature_walk_tests/*.txt`; to record an intended change, run the tests
//! with `FEATURE_WALK_BLESS=1`.

mod fixtures;

use std::collections::BTreeMap;

use fixtures::{changed, fixtures, with_points, with_refs};

use crate::FeatureKind;

/// Per fixture: its inputs, one per line (`label = expression (kind)`), then every expression.
fn record() -> String {
    let mut out = String::new();
    for (name, v) in fixtures() {
        let kind: FeatureKind = serde_json::from_value(v).unwrap_or_else(|e| panic!("{name}: {e}"));
        out += &format!("{name}\n");
        let inputs = kind.inputs();
        if !inputs.is_empty() {
            out += "  inputs:\n";
            for (label, e, k) in inputs {
                out += &format!("    {label} = {e} ({k:?})\n");
            }
        }
        let expressions = kind.expressions();
        if !expressions.is_empty() {
            out += &format!("  expressions: {}\n", expressions.join(", "));
        }
    }
    out
}

/// Per fixture: the sketches it is built from, and the fields that copying with every sketch,
/// feature and body renamed rewrites (`remap_refs`: sketches and features; `remap_bodies`).
fn ref_record() -> String {
    let sketches: BTreeMap<u64, u64> = (1..=8).map(|i| (i, i + 100)).collect();
    let names = BTreeMap::from([("F".to_string(), "F2".to_string()), ("CP".to_string(), "CP2".to_string())]);
    let bodies = BTreeMap::from([("B".to_string(), "B2".to_string())]);
    let mut out = String::new();
    for (name, v) in fixtures() {
        let kind: FeatureKind = serde_json::from_value(with_refs(name, v)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let before = serde_json::to_value(&kind).unwrap();
        let mut refs = kind.clone();
        refs.remap_refs(&sketches, &names);
        let mut by_refs = Vec::new();
        changed(&before, &serde_json::to_value(&refs).unwrap(), "", &mut by_refs);
        let mut body = kind.clone();
        body.remap_bodies(&bodies);
        let mut by_bodies = Vec::new();
        changed(&before, &serde_json::to_value(&body).unwrap(), "", &mut by_bodies);
        out += &format!("{name}\n");
        let sketches = kind.input_sketches();
        if !sketches.is_empty() {
            out += &format!("  sketches: {sketches:?}\n");
        }
        if !by_refs.is_empty() {
            out += &format!("  remap_refs: {}\n", by_refs.join(", "));
        }
        if !by_bodies.is_empty() {
            out += &format!("  remap_bodies: {}\n", by_bodies.join(", "));
        }
    }
    out
}

/// A variant with a field that refers to a sketch (a `SketchId`, sketch points or loft
/// sections) must report it from `input_sketches`, or deleting the sketch leaves the feature
/// broken. New variants are caught by the walk's exhaustive match; this catches a
/// sketch field added to an existing variant (its fixture has to set it).
/// Per fixture (with every reference and position set): the fields `transform_geometry` changes
/// under a turn and a shift (every point, direction and plane it maps), the face points that name
/// faces, the profile selections with their sketches, and a primitive's placement point.
fn geometry_record() -> String {
    // x → y → z → x, then (1, 2, 3): every point and every direction of the fixtures changes.
    let m = [[0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
    let mut out = String::new();
    for (name, v) in fixtures() {
        let kind: FeatureKind = serde_json::from_value(with_points(name, with_refs(name, v))).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut moved = kind.clone();
        moved.transform_geometry(&m);
        let mut paths = Vec::new();
        changed(&serde_json::to_value(&kind).unwrap(), &serde_json::to_value(&moved).unwrap(), "", &mut paths);
        out += &format!("{name}\n");
        if !paths.is_empty() {
            out += &format!("  moves: {}\n", paths.join(", "));
        }
        let faces: Vec<String> = kind.face_points().iter().map(|p| format!("[{}, {}, {}]", p.x, p.y, p.z)).collect();
        if !faces.is_empty() {
            out += &format!("  face points: {}\n", faces.join(", "));
        }
        let profiles: Vec<String> = kind
            .profile_sels()
            .iter()
            .map(|(sketch, sel)| format!("{sketch} {}", serde_json::to_value(sel).unwrap()["type"].as_str().unwrap_or("?")))
            .collect();
        if !profiles.is_empty() {
            out += &format!("  profiles: {}\n", profiles.join(", "));
        }
        if let Some(p) = kind.point_expr() {
            out += &format!("  placement: {}\n", p.iter().map(|e| e.as_str()).collect::<Vec<_>>().join(", "));
        }
    }
    out
}

/// Compares `got` with the golden file `name` in `feature_walk_tests/`, line by line. With
/// `FEATURE_WALK_BLESS` set it rewrites the file instead, for an intended change (say why in the
/// pull request).
fn check_golden(name: &str, want: &str, got: &str) {
    if std::env::var_os("FEATURE_WALK_BLESS").is_some() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/feature_walk_tests").join(name);
        std::fs::write(&path, got).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        return;
    }
    for (line, (want, got)) in want.lines().zip(got.lines()).enumerate() {
        assert_eq!(want, got, "{name} line {}; rerun with FEATURE_WALK_BLESS=1 if the change is intended", line + 1);
    }
    assert_eq!(want.lines().count(), got.lines().count(), "{name}: the record has a different number of lines");
}

/// Recorded from the hand-written `inputs_mut` and `expressions` the walk replaced.
const GOLDEN_EXPRS: &str = include_str!("feature_walk_tests/golden_exprs.txt");

/// Recorded from the hand-written `transform_geometry`, `face_points`, `profile_sels` and
/// `point_expr` the walk replaced.
const GOLDEN_GEOMETRY: &str = include_str!("feature_walk_tests/golden_geometry.txt");

/// Recorded from the hand-written `input_sketches`, `remap_refs` and `remap_bodies` the walk
/// replaced, with two fixes: a coil's `targets` and a surface trim's plane are remapped too.
const GOLDEN_REFS: &str = include_str!("feature_walk_tests/golden_refs.txt");

#[test]
fn input_sketches_lists_every_variant_with_a_sketch_field() {
    let src = include_str!("document.rs").replace("\r\n", "\n");
    let body = src.split("pub enum FeatureKind {").nth(1).and_then(|s| s.split("\n}\n").next()).unwrap();
    let mut with_sketch = Vec::new();
    let mut variant = String::new();
    for line in body.lines() {
        let t = line.trim();
        if line.starts_with("    ") && !line.starts_with("     ") && t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            variant = t.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_lowercase();
            continue;
        }
        let Some((_, ty)) = t.strip_prefix("pub ").unwrap_or(t).split_once(':') else { continue };
        let refers = ty.contains("SketchId") || ty.contains("SketchPoints") || ty.contains("LoftSection");
        if refers && !t.starts_with("//") && !with_sketch.contains(&variant) {
            with_sketch.push(variant.clone());
        }
    }
    assert!(with_sketch.len() >= 10, "the FeatureKind parse found too few sketch users: {with_sketch:?}");
    for v in with_sketch {
        let uses = fixtures().into_iter().filter(|(_, f)| f["type"].as_str().unwrap().replace('_', "") == v).any(|(name, f)| {
            let kind: FeatureKind = serde_json::from_value(with_refs(name, f)).unwrap();
            !kind.input_sketches().is_empty()
        });
        assert!(uses, "FeatureKind::{v} has a sketch field but input_sketches reports no sketch for its fixtures");
    }
}

#[test]
fn geometry_matches_the_golden_record() {
    check_golden("golden_geometry.txt", GOLDEN_GEOMETRY, &geometry_record());
}

#[test]
fn references_match_the_golden_record() {
    check_golden("golden_refs.txt", GOLDEN_REFS, &ref_record());
}

#[test]
fn construction_names_follow_renames_in_every_definition() {
    use crate::walk::{Ref, Walk};
    use serde_json::json;

    let axis = json!({"type": "axis", "name": "Old"});
    let point = json!({"type": "construct_point", "name": "Old"});
    let plane = json!({"type": "plane", "plane": {"type": "construction", "name": "Old"}});
    let definitions = [
        json!({"type": "construction_axis", "def": {"type": "through_face", "face": axis}}),
        json!({"type": "construction_axis", "def": {"type": "normal_to_face", "face": plane, "point": point}}),
        json!({"type": "construction_axis", "def": {"type": "two_planes", "a": plane, "b": plane}}),
        json!({"type": "construction_axis", "def": {"type": "two_points", "a": point, "b": point}}),
        json!({"type": "construction_axis", "def": {"type": "edge", "edge": axis}}),
        json!({"type": "construction_point", "def": {"type": "at", "point": point}}),
        json!({"type": "construction_point", "def": {"type": "two_edges", "a": axis, "b": axis}}),
        json!({"type": "construction_point", "def": {"type": "three_planes", "a": plane, "b": plane, "c": plane}}),
        json!({"type": "construction_point", "def": {"type": "center", "of": axis}}),
        json!({"type": "construction_point", "def": {"type": "edge_and_plane", "edge": axis, "plane": plane}}),
        json!({"type": "construction_point", "def": {"type": "along_path", "path": axis, "t": "0.5"}}),
    ];
    let planes = [
        json!({"type": "midplane", "a": plane, "b": plane}),
        json!({"type": "two_edges", "a": axis, "b": axis}),
        json!({"type": "three_points", "a": point, "b": point, "c": point}),
        json!({"type": "tangent", "face": plane, "at": [0, 0, 0]}),
        json!({"type": "along_path", "path": axis, "t": "0.5"}),
        json!({"type": "perpendicular", "plane": plane, "line": axis}),
    ];
    let wrapped = planes.into_iter().map(|plane| {
        json!({"type": "construction_plane", "plane": {"type": "flipped", "base": {
            "type": "offset", "distance": "10", "base": {"type": "at_angle", "angle": "20 deg",
            "axis_origin": [0, 0, 0], "axis_dir": [1, 0, 0], "base": plane}}}})
    });
    let names = BTreeMap::from([("Old".into(), "New".into()), ("New".into(), "Wrong".into())]);
    for definition in definitions.into_iter().chain(wrapped) {
        let mut kind: FeatureKind = serde_json::from_value(definition.clone()).unwrap();
        let expected_count = definition.to_string().matches("\"Old\"").count();
        let mut seen = Vec::new();
        kind.walk_refs(&mut |r| {
            if let Ref::Feature(name) = r {
                seen.push(name.to_string());
            }
        });
        assert_eq!(seen, vec!["Old"; expected_count], "{definition}");
        kind.remap_refs(&BTreeMap::new(), &names);
        let expected: serde_json::Value = serde_json::from_str(&definition.to_string().replace("\"Old\"", "\"New\"")).unwrap();
        assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::to_value(serde_json::from_value::<FeatureKind>(expected).unwrap()).unwrap());
    }
}

#[test]
fn construction_reference_walk_keeps_its_depth_limit_across_geometry() {
    use crate::construct::GeoRef;
    use crate::walk::{Ref, Walk};

    let mut plane = crate::PlaneRef::Construction { name: "TooDeep".into() };
    for _ in 0..80 {
        plane = crate::PlaneRef::Perpendicular { plane: Box::new(GeoRef::Plane { plane }), line: Box::new(GeoRef::Axis { name: "Side".into() }) };
    }
    let mut seen = Vec::new();
    plane.walk_refs(&mut |r| {
        if let Ref::Feature(name) = r {
            seen.push(name.to_string());
        }
    });
    assert!(!seen.is_empty() && seen.len() < 64);
    assert!(seen.iter().all(|name| name == "Side"));
    let mut changed = 0;
    plane.walk_refs_mut(&mut |r| {
        if let Ref::Feature(name) = r {
            assert_eq!(name.as_ref(), "Side");
            *name = "Renamed".into();
            changed += 1;
        }
    });
    assert_eq!(changed, seen.len());
}

#[test]
fn inputs_and_expressions_match_the_golden_record() {
    check_golden("golden_exprs.txt", GOLDEN_EXPRS, &record());
}

#[test]
fn the_record_covers_every_variant() {
    let src = include_str!("document.rs").replace("\r\n", "\n");
    let body = src.split("pub enum FeatureKind {").nth(1).and_then(|s| s.split("\n}\n").next()).unwrap();
    let names: Vec<String> = fixtures().into_iter().map(|(_, v)| v["type"].as_str().unwrap().replace('_', "")).collect();
    for line in body.lines().filter(|l| l.starts_with("    ") && !l.starts_with("     ")) {
        let t = line.trim();
        if t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let variant = t.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_lowercase();
            assert!(names.contains(&variant), "FeatureKind::{t} has no fixture in feature_walk_tests");
        }
    }
}
