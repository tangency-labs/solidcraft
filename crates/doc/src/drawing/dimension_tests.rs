use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};
use solidcraft_geom::drawing_view::NamedView;

use super::*;
use crate::drawing::Source;
use crate::drawing::view::{View, ViewKind, ViewPlace};
use crate::drawing_view::{ViewDirection, ViewOverrides};
use crate::format::DRAWING;
use crate::ids::IdSource;

const SEED: u64 = 4896;
/// The body every reference here is on, and the feature that made it.
const BODY: &str = "Box";
const FEATURE: u64 = 7;

fn corner(edges: &[&str]) -> ModelPoint {
    ModelPoint { body: BODY.into(), origin_feature: FEATURE, edges: edges.iter().map(|e| e.to_string()).collect(), centre: false, path: Vec::new() }
}

fn edge(name: &str) -> ModelEdge {
    ModelEdge { body: BODY.into(), origin_feature: FEATURE, name: name.into(), path: Vec::new() }
}

/// A drawing of one design on one A3 sheet, its front view at 1:2 at (150, 150), and on the view a
/// linear dimension between two corners, an edge's length and an angle between two edges: their
/// ids, in the order they were added.
fn measured() -> (Drawing, [u64; 3]) {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Box", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let source = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(Source { id: source });
    let (view, sheet) = (d.allocate_id(&mut id_source).unwrap(), d.sheets[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let settings = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View {
        id: view,
        sheet,
        kind,
        place: ViewPlace::At([150.0, 150.0]),
        settings,
        reference_shown_percent: 0.0,
        centre_marks: None,
    })]);
    let kinds = [
        DimensionKind::Linear {
            a: corner(&["F1:front|F1:bottom", "F1:front|F1:left"]),
            b: corner(&["F1:front|F1:bottom", "F1:front|F1:right"]),
            direction: LinearDirection::Horizontal,
            reading: Reading::Projected,
        },
        DimensionKind::Length { edge: edge("F1:front|F1:top"), direction: LinearDirection::Aligned, reading: Reading::True },
        DimensionKind::Angle { a: edge("F1:front|F1:top"), b: edge("F2:chamfer|F1:front"), reading: Reading::Projected, other_side: true },
    ];
    let texts = [TextPlace::Centred, TextPlace::At([0.0, -20.0]), TextPlace::At([12.5, 30.0])];
    let mut added = [0; 3];
    for ((kind, text), id) in kinds.into_iter().zip(texts).zip(&mut added) {
        *id = d.allocate_id(&mut id_source).unwrap();
        d.add_dimensions([Arc::new(Dimension {
            id: *id,
            view,
            kind,
            offset_mm: -10.0,
            text,
            decimals: 2,
            override_text: None,
            last_good: None,
            from_model: None,
        })]);
    }
    d.validate().unwrap();
    (d, added)
}

fn file(d: &Drawing) -> Value {
    serde_json::from_str(&d.to_json().unwrap()).unwrap()
}

/// Dimension `id` in a drawing's file.
fn dim<'a>(v: &'a mut Value, id: &str) -> &'a mut Value {
    v["dimensions"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == id).unwrap()
}

/// Why `d`'s file, changed by `edit`, is refused, or "read".
fn refused(d: &Drawing, edit: impl FnOnce(&mut Value)) -> String {
    let mut v = file(d);
    edit(&mut v);
    Drawing::from_json(&v.to_string()).map_or_else(|e| e.to_string(), |_| "read".into())
}

/// A dimension saves what it measures by name, its offset, its text's place and its decimals, and
/// nothing it leaves out. The dimensions save by id, whatever order they were added in, and read
/// back as saved, the same bytes again; a file that lists them in another order reads by id. An
/// older drawing reads with none, and a drawing with none writes none.
#[test]
fn dimensions_save_what_they_measure_and_read_back_as_saved() {
    let (d, added) = measured();
    let [linear_id, length_id, angle_id] = added.map(ids::encode);
    let view = ids::encode(d.views[0].id);
    let mut v = file(&d);
    let box_corner = |edges: [&str; 2]| json!({"body": "Box", "origin_feature": 7, "edges": edges});
    assert_eq!(
        *dim(&mut v, &linear_id),
        json!({
            "id": linear_id, "view": view,
            "kind": {"linear": {
                "a": box_corner(["F1:front|F1:bottom", "F1:front|F1:left"]),
                "b": box_corner(["F1:front|F1:bottom", "F1:front|F1:right"]),
                "direction": "horizontal", "reading": "projected"
            }},
            "offset_mm": -10.0, "text": "centred", "decimals": 2
        })
    );
    let length = dim(&mut v, &length_id).clone();
    let top_edge = json!({"body": "Box", "origin_feature": 7, "name": "F1:front|F1:top"});
    assert_eq!(length["kind"], json!({"length": {"edge": top_edge, "direction": "aligned", "reading": "true"}}));
    assert_eq!(length["text"], json!({"at": [0.0, -20.0]}));
    assert_eq!(dim(&mut v, &angle_id)["kind"]["angle"]["other_side"], json!(true));
    let mut one_side = v.clone();
    dim(&mut one_side, &angle_id)["kind"]["angle"].as_object_mut().unwrap().remove("other_side");
    let one_side = Drawing::from_json(&one_side.to_string()).unwrap();
    assert_eq!(dim(&mut file(&one_side), &angle_id)["kind"]["angle"].get("other_side"), None, "the usual side, not written");
    let listed: Vec<u64> = v["dimensions"].as_array().unwrap().iter().map(|x| ids::decode(x["id"].as_str().unwrap()).unwrap()).collect();
    let mut by_id = added.to_vec();
    by_id.sort_unstable();
    assert_eq!(listed, by_id);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
    let mut other_order = v.clone();
    other_order["dimensions"].as_array_mut().unwrap().reverse();
    assert_eq!(Drawing::from_json(&other_order.to_string()).unwrap(), d);
    let mut unsorted = d.clone();
    unsorted.dimensions.reverse();
    assert_eq!(unsorted.validate().unwrap_err().to_string(), "a drawing lists its dimensions by id");
    let mut none = d.clone();
    none.dimensions.clear();
    assert_eq!(file(&none).get("dimensions"), None, "no dimensions, none written");
    let older = json!({
        "format": DRAWING.format_string(1), "id": "0000000000002", "name": "Old", "sources": [], "language": "en",
        "sheets": [{"id": "0000000000001", "name": "S", "sheet": {"size": {"standard": "a4"}}}]
    });
    assert!(Drawing::from_json(&older.to_string()).unwrap().dimensions.is_empty());
}

/// What reading refuses, each rule by name, on both sides of every cap: the view a dimension is
/// on, the names, the feature and the path of its references, a corner's edges, its direction and
/// reading, its offset and text place, its decimals and its text, its id, how many there are, and
/// fields it doesn't know.
#[test]
fn the_dimensions_rules_are_kept_on_reading() {
    let (d, added) = measured();
    let [linear_id, length_id, angle_id] = added.map(ids::encode);
    let rule = |id: &str, rule: &str| format!("damaged drawing: dimension {id}: {rule}");
    let nothing = ids::encode(99);
    let on = format!("it is on view {nothing}, which the drawing doesn't have");
    assert_eq!(refused(&d, |v| dim(v, &linear_id)["view"] = json!(nothing)), rule(&linear_id, &on));
    // Names: of the body, an edge and a corner's edges, 1 to 512 bytes without control characters.
    let names = "a name in a reference is 1 to 512 bytes, without control characters";
    let body = |name: &str| refused(&d, |v| dim(v, &length_id)["kind"]["length"]["edge"]["body"] = json!(name));
    let edge_name = |name: &str| refused(&d, |v| dim(v, &angle_id)["kind"]["angle"]["b"]["name"] = json!(name));
    let corner_edge = |name: &str| refused(&d, |v| dim(v, &linear_id)["kind"]["linear"]["a"]["edges"][1] = json!(name));
    for fits in ["x".repeat(512), "é".repeat(256), "F1:front|F1:top".into()] {
        assert_eq!([body(&fits), edge_name(&fits), corner_edge(&fits)], ["read", "read", "read"], "{} bytes", fits.len());
    }
    for broken in ["x".repeat(513), "é".repeat(256) + "a", "x".repeat(1_000_000), String::new(), "two\nlines".into(), "nul\0".into()] {
        let expected = [rule(&length_id, names), rule(&angle_id, names), rule(&linear_id, names)];
        assert_eq!([body(&broken), edge_name(&broken), corner_edge(&broken)], expected, "{} bytes", broken.len());
    }
    // A corner is where 2 to 16 named edges meet.
    let edges = |n: usize| {
        refused(&d, |v| dim(v, &linear_id)["kind"]["linear"]["b"]["edges"] = json!((0..n).map(|i| format!("F1:e{i}")).collect::<Vec<_>>()))
    };
    let meet = rule(&linear_id, "a corner is where 2 to 16 named edges meet");
    assert_eq!([edges(0), edges(1), edges(2), edges(16), edges(17)], [meet.clone(), meet.clone(), "read".into(), "read".into(), meet]);
    // A centre is of one named circle or arc, and saves as `centre`.
    let centred = |n: usize| {
        let mut v = file(&d);
        let b = &mut dim(&mut v, &linear_id)["kind"]["linear"]["b"];
        b["centre"] = json!(true);
        b["edges"] = json!((0..n).map(|i| format!("F3:e{i}")).collect::<Vec<_>>());
        let read = Drawing::from_json(&v.to_string()).map_err(|e| e.to_string())?;
        Ok::<_, String>(dim(&mut file(&read), &linear_id)["kind"]["linear"]["b"].clone())
    };
    let one = Err(rule(&linear_id, "a centre is of one named circle or arc"));
    assert_eq!([centred(0), centred(2)], [one.clone(), one]);
    assert_eq!(centred(1), Ok(json!({"body": "Box", "origin_feature": 7, "edges": ["F3:e0"], "centre": true})));
    // The feature that made the body, and the occurrence path.
    let feature = rule(&length_id, "a reference names the feature that made its body");
    assert_eq!(refused(&d, |v| dim(v, &length_id)["kind"]["length"]["edge"]["origin_feature"] = json!(0)), feature);
    let path = |p: Vec<u64>| refused(&d, |v| dim(v, &angle_id)["kind"]["angle"]["a"]["path"] = json!(p));
    let deep = rule(&angle_id, "an occurrence path is at most 64 occurrences, none of them 0");
    assert_eq!([path(vec![3; 64]), path(vec![3; 65]), path(vec![3, 0]), path(vec![])], ["read".into(), deep.clone(), deep, "read".to_string()]);
    // A direction to measure along that isn't zero, and a true length measured aligned.
    let along = |a: Value| refused(&d, |v| dim(v, &length_id)["kind"]["length"]["direction"] = json!({"along": a}));
    let zero = rule(&length_id, "the direction to measure along is zero or not finite");
    assert_eq!(along(json!([0.0, 0.0])), zero);
    assert!(along(json!([1e308, 1e308])).ends_with("is out of range"), "the file's own number check, before the record's");
    // A whole number passes the file's check but would save as one past it: the record refuses it.
    assert_eq!(along(json!([u64::MAX, 0])), rule(&length_id, "the direction to measure along is at most 1000000000 each way"));
    assert_eq!(along(json!([1_000_000_000, 1])), rule(&length_id, "a true length is measured aligned"), "1e9 each way passes");
    assert_eq!(along(json!([0.0, 2.0])), rule(&length_id, "a true length is measured aligned"), "along, read true");
    let projected_along = refused(&d, |v| {
        dim(v, &length_id)["kind"]["length"]["direction"] = json!({"along": [0.0, 2.0]});
        dim(v, &length_id)["kind"]["length"]["reading"] = json!("projected");
    });
    assert_eq!(projected_along, "read");
    let true_horizontal = refused(&d, |v| dim(v, &linear_id)["kind"]["linear"]["reading"] = json!("true"));
    assert_eq!(true_horizontal, rule(&linear_id, "a true length is measured aligned"));
    // On paper: the offset and the text's place.
    let paper = rule(&angle_id, "its offset and its text's place are at most 5000 mm on paper");
    let offset = |mm: f64| refused(&d, |v| dim(v, &angle_id)["offset_mm"] = json!(mm));
    let text = |at: [f64; 2]| refused(&d, |v| dim(v, &angle_id)["text"] = json!({"at": at}));
    assert_eq!([offset(5000.0), offset(-5000.0), offset(5000.001), offset(-5000.001)], ["read".into(), "read".into(), paper.clone(), paper.clone()]);
    assert_eq!([text([5000.0, -5000.0]), text([5000.001, 0.0]), text([0.0, -5000.001])], ["read".into(), paper.clone(), paper]);
    // Decimals, and the text shown instead of the number.
    let decimals = |n: u64| refused(&d, |v| dim(v, &angle_id)["decimals"] = json!(n));
    assert_eq!([decimals(0), decimals(6), decimals(7)], ["read".into(), "read".into(), rule(&angle_id, "it shows at most 6 decimals")]);
    assert!(decimals(256).starts_with("damaged drawing: "), "past a u8");
    let instead = |text: &str| refused(&d, |v| dim(v, &linear_id)["override_text"] = json!(text));
    let text_rule = rule(&linear_id, "the text shown instead of its number is 1 to 1024 bytes");
    assert_eq!(
        [instead("≈80"), instead(&"x".repeat(1024)), instead(""), instead(&"x".repeat(1025))],
        ["read".into(), "read".into(), text_rule.clone(), text_rule]
    );
    // Its id is the drawing's: never another object's.
    let view_id = ids::encode(d.views[0].id);
    assert_eq!(refused(&d, |v| dim(v, &length_id)["id"] = json!(view_id)), format!("damaged drawing: two objects have the id {view_id}"));
    // Fields it doesn't know, at every level.
    let unknown: [fn(&mut Value, &str); 5] = [
        |v, id| dim(v, id)["label"] = json!("A"),
        |v, id| dim(v, id)["kind"]["linear"]["tolerance"] = json!(0.1),
        |v, id| dim(v, id)["kind"]["linear"]["a"]["point"] = json!([0.0, 0.0, 0.0]),
        |v, id| dim(v, id)["text"] = json!({"at": [1.0, 2.0], "angle": 3.0}),
        |v, id| dim(v, id)["kind"] = json!({"baseline": {}}),
    ];
    for (i, edit) in unknown.into_iter().enumerate() {
        assert!(refused(&d, |v| edit(v, &linear_id)).starts_with("damaged drawing: "), "{i}");
    }
    // At most MAX_DIMENSIONS dimensions.
    let many = |n: usize| {
        let mut v = file(&d);
        let one = dim(&mut v, &linear_id).clone();
        let copies = (1..=n as u64).map(|i| {
            let mut copy = one.clone();
            copy["id"] = json!(ids::encode(i));
            copy
        });
        v["dimensions"] = Value::Array(copies.collect());
        Drawing::from_json(&v.to_string()).map(|d| d.dimensions.len()).map_err(|e| e.to_string())
    };
    assert_eq!(many(MAX_DIMENSIONS), Ok(MAX_DIMENSIONS));
    assert_eq!(many(MAX_DIMENSIONS + 1), Err(format!("damaged drawing: a drawing has at most {MAX_DIMENSIONS} dimensions")));
}

/// Two lines at 45° read 45°, and 135° on the other side, whichever way each runs: the angle
/// between their directions is 45° or 135° by how the kernel runs the edges.
#[test]
fn an_angle_between_lines_reads_the_same_whichever_way_they_run() {
    let (acute, obtuse) = (45f64.to_radians(), 135f64.to_radians());
    for between in [acute, obtuse] {
        assert!((between_lines(between, false) - acute).abs() < 1e-12, "{between}");
        assert!((between_lines(between, true) - obtuse).abs() < 1e-12, "{between}");
    }
}

/// A radial dimension saves its edge, and `diameter` only when it is one; its edge keeps an edge's
/// rules, and it has no reading.
#[test]
fn a_radial_dimension_saves_its_edge_and_whether_it_is_a_diameter() {
    let (mut d, _) = measured();
    let (view, id) = (d.views[0].id, d.allocate_id(&mut IdSource::seeded(SEED + 1)).unwrap());
    let radial = |diameter| DimensionKind::Radial { edge: edge("F3:hole|F1:front"), diameter };
    let kind = radial(true);
    d.add_dimensions([Arc::new(Dimension {
        id,
        view,
        kind,
        offset_mm: 5.0,
        text: TextPlace::Centred,
        decimals: 1,
        override_text: None,
        last_good: None,
        from_model: None,
    })]);
    d.validate().unwrap();
    let radial_id = ids::encode(id);
    let mut v = file(&d);
    let hole = json!({"body": "Box", "origin_feature": 7, "name": "F3:hole|F1:front"});
    assert_eq!(dim(&mut v, &radial_id)["kind"], json!({"radial": {"edge": hole, "diameter": true}}));
    assert_eq!(Drawing::from_json(&v.to_string()).unwrap(), d);
    dim(&mut v, &radial_id)["kind"]["radial"].as_object_mut().unwrap().remove("diameter");
    let radius = Drawing::from_json(&v.to_string()).unwrap();
    assert_eq!(radius.dimension(id).map(|r| &r.kind), Some(&radial(false)));
    assert_eq!(dim(&mut file(&radius), &radial_id)["kind"]["radial"].get("diameter"), None, "a radius, not written");
    let names = format!("damaged drawing: dimension {radial_id}: a name in a reference is 1 to 512 bytes, without control characters");
    assert_eq!(refused(&d, |v| dim(v, &radial_id)["kind"]["radial"]["edge"]["name"] = json!("")), names);
    assert!(refused(&d, |v| dim(v, &radial_id)["kind"]["radial"]["reading"] = json!("true")).starts_with("damaged drawing: "));
}

/// A chain saves its points in order and its direction, and keeps its rules on reading: 3 to
/// [`MAX_CHAIN_POINTS`] points, each with a point's rules, a direction to measure along that isn't
/// zero, no reading (it reads projected) and no text instead of its members' numbers.
#[test]
fn a_chain_saves_its_points_in_order_and_keeps_its_rules() {
    let (mut d, _) = measured();
    let (view, id) = (d.views[0].id, d.allocate_id(&mut IdSource::seeded(SEED + 1)).unwrap());
    let hole = |n: u64| {
        let rim = format!("F{n}:hole|F1:front");
        ModelPoint { centre: true, ..corner(&[rim.as_str()]) }
    };
    let points = vec![corner(&["F1:front|F1:bottom", "F1:front|F1:left"]), hole(3), hole(4), hole(5)];
    let kind = DimensionKind::Chain { points, direction: LinearDirection::Horizontal };
    d.add_dimensions([Arc::new(Dimension {
        id,
        view,
        kind,
        offset_mm: 8.0,
        text: TextPlace::Centred,
        decimals: 1,
        override_text: None,
        last_good: None,
        from_model: None,
    })]);
    d.validate().unwrap();
    let chain_id = ids::encode(id);
    let mut v = file(&d);
    let first = json!({"body": "Box", "origin_feature": 7, "edges": ["F1:front|F1:bottom", "F1:front|F1:left"]});
    let centre = |n: u64| json!({"body": "Box", "origin_feature": 7, "edges": [format!("F{n}:hole|F1:front")], "centre": true});
    let saved = json!({"chain": {"points": [first, centre(3), centre(4), centre(5)], "direction": "horizontal"}});
    assert_eq!(dim(&mut v, &chain_id)["kind"], saved);
    assert_eq!(Drawing::from_json(&v.to_string()).unwrap(), d);
    let rule = |rule: &str| format!("damaged drawing: dimension {chain_id}: {rule}");
    let count = |n: usize| {
        refused(&d, |v| {
            let points = &mut dim(v, &chain_id)["kind"]["chain"]["points"];
            let one = points[1].clone();
            *points = json!(vec![one; n]);
        })
    };
    let three_to = rule(&format!("a chain has 3 to {MAX_CHAIN_POINTS} points"));
    let counts = [count(0), count(2), count(3), count(MAX_CHAIN_POINTS), count(MAX_CHAIN_POINTS + 1)];
    assert_eq!(counts, [three_to.clone(), three_to.clone(), "read".into(), "read".into(), three_to]);
    let two_edges = refused(&d, |v| dim(v, &chain_id)["kind"]["chain"]["points"][2]["edges"] = json!(["F3:a", "F3:b"]));
    assert_eq!(two_edges, rule("a centre is of one named circle or arc"));
    let zero = refused(&d, |v| dim(v, &chain_id)["kind"]["chain"]["direction"] = json!({"along": [0.0, 0.0]}));
    assert_eq!(zero, rule("the direction to measure along is zero or not finite"));
    let instead = refused(&d, |v| dim(v, &chain_id)["override_text"] = json!("60"));
    assert_eq!(instead, rule("a chain shows its members' numbers, never a text instead"));
    let reading = refused(&d, |v| dim(v, &chain_id)["kind"]["chain"]["reading"] = json!("projected"));
    assert!(reading.starts_with("damaged drawing: "), "{reading}");
}

/// A cylinder, a sphere or a cone dimension saves its face by name, and the face keeps a reference's rules.
#[test]
fn a_cylinder_sphere_or_cone_dimension_saves_its_face() {
    let face = ModelFace { body: BODY.into(), origin_feature: FEATURE, name: "F4:boss".into(), path: vec![3] };
    let kinds = [
        ("cylinder", DimensionKind::Cylinder { face: face.clone() }),
        ("sphere", DimensionKind::Sphere { face: face.clone() }),
        ("cone", DimensionKind::Cone { face: face.clone() }),
    ];
    for (key, kind) in kinds {
        let (mut d, _) = measured();
        let (view, id) = (d.views[0].id, d.allocate_id(&mut IdSource::seeded(SEED + 1)).unwrap());
        d.add_dimensions([Arc::new(Dimension {
            id,
            view,
            kind,
            offset_mm: 6.0,
            text: TextPlace::Centred,
            decimals: 1,
            override_text: None,
            last_good: None,
            from_model: None,
        })]);
        d.validate().unwrap();
        let encoded = ids::encode(id);
        let mut v = file(&d);
        let saved = json!({key: {"face": {"body": "Box", "origin_feature": 7, "name": "F4:boss", "path": [3]}}});
        assert_eq!(dim(&mut v, &encoded)["kind"], saved);
        assert_eq!(Drawing::from_json(&v.to_string()).unwrap(), d);
        let rule = |rule: &str| format!("damaged drawing: dimension {encoded}: {rule}");
        let changed = |field: &str, value: Value| refused(&d, |v| dim(v, &encoded)["kind"][key]["face"][field] = value);
        assert_eq!(changed("name", json!("")), rule("a name in a reference is 1 to 512 bytes, without control characters"));
        assert_eq!(changed("origin_feature", json!(0)), rule("a reference names the feature that made its body"));
        assert_eq!(changed("path", json!([0])), rule("an occurrence path is at most 64 occurrences, none of them 0"));
        assert!(changed("edges", json!(["F4:boss"])).starts_with("damaged drawing: "), "{key}: a face has no edges");
    }
}

/// A sphere's size reads SØ or SR, the diameter sign as a diameter's (ISO 129-1).
#[test]
fn a_spheres_label_is_sr_or_s_diameter() {
    assert_eq!(reading_label(LabelKind::SphericalDiameter, 20.0, "mm", 2), "SØ20.00");
    assert_eq!(reading_label(LabelKind::SphericalRadius, 10.0, "mm", 1), "SR10.0");
    assert_eq!(reading_label(LabelKind::Diameter, 20.0, "mm", 0), "Ø20", "unchanged");
}

/// A dimension between two cylinders' silhouettes saves each one's face and side, and its
/// direction; each face keeps a reference's rules, a side is near or far, and it reads as the view
/// shows it (no `reading`).
#[test]
fn a_silhouettes_dimension_saves_each_face_and_side() {
    let (mut d, _) = measured();
    let (view, id) = (d.views[0].id, d.allocate_id(&mut IdSource::seeded(SEED + 1)).unwrap());
    let face = |name: &str| ModelFace { body: BODY.into(), origin_feature: FEATURE, name: name.into(), path: Vec::new() };
    let (a, b) = (Silhouette { face: face("F4:boss"), side: SilhouetteSide::Near }, Silhouette { face: face("F5:pin"), side: SilhouetteSide::Far });
    let kind = DimensionKind::Silhouettes { a, b, direction: LinearDirection::Vertical };
    d.add_dimensions([Arc::new(Dimension {
        id,
        view,
        kind,
        offset_mm: 6.0,
        text: TextPlace::Centred,
        decimals: 1,
        override_text: None,
        last_good: None,
        from_model: None,
    })]);
    d.validate().unwrap();
    let silhouettes = ids::encode(id);
    let mut v = file(&d);
    let face = |name: &str| json!({"body": "Box", "origin_feature": 7, "name": name});
    let saved = json!({"silhouettes": {"a": {"face": face("F4:boss"), "side": "near"}, "b": {"face": face("F5:pin"), "side": "far"}, "direction": "vertical"}});
    assert_eq!(dim(&mut v, &silhouettes)["kind"], saved);
    assert_eq!(Drawing::from_json(&v.to_string()).unwrap(), d);
    let rule = |rule: &str| format!("damaged drawing: dimension {silhouettes}: {rule}");
    let changed = |edit: &dyn Fn(&mut Value)| refused(&d, |v| edit(&mut dim(v, &silhouettes)["kind"]["silhouettes"]));
    assert_eq!(changed(&|k| k["b"]["face"]["name"] = json!("")), rule("a name in a reference is 1 to 512 bytes, without control characters"));
    assert_eq!(changed(&|k| k["a"]["face"]["origin_feature"] = json!(0)), rule("a reference names the feature that made its body"));
    assert_eq!(changed(&|k| k["direction"] = json!({"along": [0.0, 0.0]})), rule("the direction to measure along is zero or not finite"));
    let malformed: [fn(&mut Value); 3] = [|k| k["a"]["side"] = json!("left"), |k| k["reading"] = json!("true"), |k| k["b"]["edges"] = json!([])];
    for edit in malformed {
        let why = changed(&edit);
        assert!(why.starts_with("damaged drawing: "), "{why}");
    }
}

/// A direction along a picked line saves as its plane coordinates and reads back as the same
/// direction: what W19a measures along.
#[test]
fn a_direction_along_a_line_saves_as_its_coordinates() {
    let along = LinearDirection::Along(Vec2::new(0.6, 0.8));
    assert_eq!(serde_json::to_value(along).unwrap(), json!({"along": [0.6, 0.8]}));
    assert_eq!(serde_json::from_value::<LinearDirection>(json!({"along": [0.6, 0.8]})).unwrap(), along);
    assert_eq!(serde_json::to_value(Reading::True).unwrap(), json!("true"));
}

/// A last good value saves only when there is one, reads back as saved, and an older drawing reads
/// with none.
#[test]
fn a_last_good_value_saves_only_when_there_is_one() {
    let (mut d, [linear, ..]) = measured();
    let id = ids::encode(linear);
    assert_eq!(dim(&mut file(&d), &id).get("last_good"), None, "none: not written");
    let at = d.dimension_index(linear).unwrap();
    Arc::make_mut(&mut d.dimensions[at]).last_good = Some(80.5);
    let mut v = file(&d);
    assert_eq!(dim(&mut v, &id)["last_good"], json!(80.5));
    let read = Drawing::from_json(&v.to_string()).unwrap();
    assert_eq!(read, d);
    assert_eq!(read.to_json().unwrap(), d.to_json().unwrap(), "the same bytes again");
    dim(&mut v, &id).as_object_mut().unwrap().remove("last_good");
    assert_eq!(Drawing::from_json(&v.to_string()).unwrap().dimension(linear).unwrap().last_good, None);
}

/// A dimension brought in from the model saves which model dimension it shows, a sketch's constraint
/// or a feature's parameter, and reads back as saved; one with none writes none, and an older drawing
/// reads with none. `showing` finds the dimension that shows a model dimension, on any view, and a
/// file a hand edit doubled one in still opens, `showing` giving the first.
#[test]
fn a_dimension_saves_which_model_dimension_it_shows() {
    let (mut d, [linear, length, _]) = measured();
    let (linear_id, length_id) = (ids::encode(linear), ids::encode(length));
    assert_eq!(dim(&mut file(&d), &linear_id).get("from_model"), None, "none: not written");
    let width = ModelDimension::Sketch { sketch: 3, constraint: "c12".into() };
    let depth = ModelDimension::Feature { feature: 7, param: "Distance".into() };
    assert_eq!((d.showing(&width), d.showing(&depth)), (None, None));
    let at = |d: &Drawing, id| d.dimension_index(id).unwrap();
    let i = at(&d, linear);
    Arc::make_mut(&mut d.dimensions[i]).from_model = Some(width.clone());
    let j = at(&d, length);
    Arc::make_mut(&mut d.dimensions[j]).from_model = Some(depth.clone());
    let mut v = file(&d);
    assert_eq!(dim(&mut v, &linear_id)["from_model"], json!({"sketch": {"sketch": 3, "constraint": "c12"}}));
    assert_eq!(dim(&mut v, &length_id)["from_model"], json!({"feature": {"feature": 7, "param": "Distance"}}));
    let read = Drawing::from_json(&v.to_string()).unwrap();
    assert_eq!(read, d);
    assert_eq!(read.to_json().unwrap(), d.to_json().unwrap(), "the same bytes again");
    assert_eq!((read.showing(&width), read.showing(&depth)), (Some(linear), Some(length)));
    // Another constraint of the sketch, or the same id in another sketch, is another model dimension.
    assert_eq!(read.showing(&ModelDimension::Sketch { sketch: 3, constraint: "c13".into() }), None);
    assert_eq!(read.showing(&ModelDimension::Sketch { sketch: 4, constraint: "c12".into() }), None);
    assert_eq!(read.showing(&ModelDimension::Feature { feature: 3, param: "c12".into() }), None);
    // An older drawing has none.
    dim(&mut v, &linear_id).as_object_mut().unwrap().remove("from_model");
    assert_eq!(Drawing::from_json(&v.to_string()).unwrap().dimension(linear).unwrap().from_model, None);
    // Doubled by hand: it opens, and the first dimension by id shows it.
    let mut doubled = file(&d);
    dim(&mut doubled, &length_id)["from_model"] = json!({"sketch": {"sketch": 3, "constraint": "c12"}});
    let read = Drawing::from_json(&doubled.to_string()).unwrap();
    assert_eq!(read.showing(&width), Some(linear.min(length)));
}

/// A model dimension's constraint id or parameter label is 1 to 256 bytes without control
/// characters, its kind one of the two, and nothing else is in it; a file with any other is refused.
#[test]
fn a_model_dimensions_rules_are_kept_on_reading() {
    let (d, [linear, ..]) = measured();
    let id = ids::encode(linear);
    let rule = format!("damaged drawing: dimension {id}: the model dimension it shows is named in 1 to 256 bytes, without control characters");
    let shows = |from: Value| refused(&d, |v| dim(v, &id)["from_model"] = from);
    let sketch = |name: &str| shows(json!({"sketch": {"sketch": 3, "constraint": name}}));
    let feature = |name: &str| shows(json!({"feature": {"feature": 7, "param": name}}));
    for fits in ["x".repeat(256), "é".repeat(128), "c1".into()] {
        assert_eq!([sketch(&fits), feature(&fits)], ["read", "read"], "{} bytes", fits.len());
    }
    for bad in [String::new(), "x".repeat(257), "c\n1".into(), "c\u{7}".into()] {
        assert_eq!([sketch(&bad), feature(&bad)], [rule.clone(), rule.clone()], "{bad:?}");
    }
    let damaged = [
        json!({"sketch": {"sketch": 3}}),
        json!({"sketch": {"sketch": -1, "constraint": "c1"}}),
        json!({"sketch": {"sketch": 3, "constraint": "c1", "param": "Distance"}}),
        json!({"view": {"view": 3, "name": "c1"}}),
        json!("sketch"),
        json!({"sketch": {"sketch": 3, "constraint": "c1"}, "feature": {"feature": 7, "param": "Distance"}}),
    ];
    for from in damaged {
        assert!(shows(from.clone()).starts_with("damaged drawing: "), "{from}");
    }
    // Any feature id reads: one the design no longer has only leaves its model dimension unshown.
    assert_eq!(shows(json!({"feature": {"feature": u64::MAX, "param": "Distance"}})), "read");
}

/// A last good value is from 0 to the largest number a drawing saves; a file with any other is
/// refused, and so is a record built with one that JSON can't hold.
#[test]
fn a_last_good_value_keeps_its_rule() {
    let (d, [linear, ..]) = measured();
    let id = ids::encode(linear);
    let with = |value: Value| refused(&d, |v| dim(v, &id)["last_good"] = value);
    for good in [json!(0.0), json!(-0.0), json!(5e-324), json!(80), json!(MAX_REAL)] {
        assert_eq!(with(good.clone()), "read", "{good}");
    }
    for bad in [json!(-5e-324), json!(-1.0)] {
        assert!(with(bad.clone()).contains("last good"), "{bad}: {}", with(bad.clone()));
    }
    // Past every drawing's largest number: the file's own check refuses it first.
    let past = MAX_REAL * (1.0 + f64::EPSILON);
    for bad in [json!(past), json!(f64::MAX), json!(-f64::MAX)] {
        assert_ne!(with(bad.clone()), "read", "{bad}");
    }
    let mut direct = d.clone();
    Arc::make_mut(&mut direct.dimensions[0]).last_good = Some(past);
    assert!(direct.validate().is_err());
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut direct = d.clone();
        Arc::make_mut(&mut direct.dimensions[0]).last_good = Some(bad);
        assert!(direct.validate().is_err(), "{bad}");
    }
}

/// What dimensions last read goes into their records by drawing and dimension, leaving out a
/// dimension or a drawing no longer there and a value the record would refuse. A drawing with no
/// new value stays shared with the document it was copied from, and that document keeps none.
#[test]
fn keep_last_good_keeps_only_what_a_record_may_save() {
    let (first, [a, b, c]) = measured();
    let second = Drawing { id: first.id + 1, ..first.clone() };
    let (one, two) = (first.id, second.id);
    let mut doc = Document::new("Box");
    doc.drawings = vec![Arc::new(first), Arc::new(second)];
    let before = doc.clone();
    let values = BTreeMap::from([
        ((one, a), 80.0),
        ((one, b), MAX_REAL),
        ((one, c), MAX_REAL * 2.0),
        ((one, 1), 5.0),
        ((one, u64::MAX), 5.0),
        ((two + 1, a), 5.0),
    ]);
    doc.keep_last_good(&values);
    let read = |doc: &Document, drawing: usize, id| doc.drawings[drawing].dimension(id).unwrap().last_good;
    assert_eq!([read(&doc, 0, a), read(&doc, 0, b), read(&doc, 0, c)], [Some(80.0), Some(MAX_REAL), None]);
    assert_eq!([read(&doc, 1, a), read(&doc, 1, b), read(&doc, 1, c)], [None; 3], "another drawing's dimensions, with the same ids");
    assert!(Arc::ptr_eq(&doc.drawings[1], &before.drawings[1]), "no new value: still shared");
    assert_eq!(read(&before, 0, a), None, "the document it was copied from keeps none");
    doc.drawings.iter().for_each(|d| d.validate().unwrap());
    let kept = doc.clone();
    doc.keep_last_good(&values);
    assert!(Arc::ptr_eq(&doc.drawings[0], &kept.drawings[0]), "the same values again: still shared");
}

/// Whatever a read hands over, at every boundary and over random bit patterns, the saved record is
/// one a file can open: a design is never lost to its last good values.
#[test]
fn keep_last_good_never_writes_a_record_a_file_refuses() {
    let (d, [linear, ..]) = measured();
    let id = d.id;
    let mut doc = Document::new("Box");
    doc.drawings = vec![Arc::new(d)];
    let tricky = [0.0, -0.0, 5e-324, MAX_REAL, MAX_REAL * (1.0 + f64::EPSILON), -1.0, 1e12, f64::MAX, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    let mut bits = 0x9E37_79B9_7F4A_7C15_u64;
    let random = std::iter::repeat_with(|| {
        bits = bits.wrapping_mul(0x5851_F42D_4C95_7F2D).wrapping_add(1);
        f64::from_bits(bits)
    });
    for value in tricky.into_iter().chain(random.take(2000)) {
        doc.keep_last_good(&BTreeMap::from([((id, linear), value)]));
        let saved = doc.drawings[0].to_json().unwrap();
        let read = Drawing::from_json(&saved).unwrap_or_else(|e| panic!("{value}: {e}"));
        assert_eq!(read.dimension(linear).unwrap().last_good, doc.drawings[0].dimension(linear).unwrap().last_good, "{value}");
    }
}

/// A lost edge's leads, from its body's face names: the blend face that rounds it and a piece of that
/// face (a later cut's), then the edges they bound, then the faces of the feature that made it, at most
/// 16, no face twice. Another fillet's face rounds another edge, so it leads nowhere. A `~k` on the
/// name finds the same blend; a name no blend rounds leads to its makers' faces only, as a face's
/// does, and a name that doesn't read, or whose feature made nothing here, leads nowhere.
#[test]
fn lost_leads_list_the_blend_then_its_edges_then_the_makers_faces() {
    let blends = ["F5:blend:F1:+z|F1:-y", "F5:blend:F1:+z|F1:-y#[F1:+x,F7:+z]", "F6:blend:F1:+x|F1:+z"];
    let mut faces: Vec<String> = ["F1:+z", "F1:-y", "F1:+x"].into_iter().chain(blends).map(String::from).collect();
    faces.extend((0..40).map(|k| format!("F1:face:{k}")));
    let leads = Leads::new(BODY, &faces);
    // Edges 0, 1, 3 and 5 bound the blend or its piece; 2 and 4 don't.
    let edge_faces = [vec![0, 3], vec![1, 3], vec![0, 2], vec![3, 4], vec![2, 5], vec![4, 1]];
    let lead_face = |index: usize| Candidate::Face { body: BODY.into(), index };
    let lead_edge = |index: usize| Candidate::Edge { body: BODY.into(), index };
    let box_faces = |n: usize| [0, 1, 2].into_iter().chain(6..).take(n).map(lead_face);
    let mut expected = vec![lead_face(3), lead_face(4), lead_edge(0), lead_edge(1), lead_edge(3), lead_edge(5)];
    expected.extend(box_faces(10));
    let found = leads.for_edge("F1:+z|F1:-y", &edge_faces);
    assert_eq!(found, expected);
    assert_eq!(found.len(), MAX_CANDIDATES);
    assert_eq!(leads.for_edge("F1:+z|F1:-y~2", &edge_faces), expected, "a numbered edge's blend");
    let makers: Vec<Candidate> = box_faces(MAX_CANDIDATES).collect();
    assert_eq!(leads.for_edge("F1:-y|F1:+x", &edge_faces), makers, "no blend rounds it");
    assert_eq!(leads.for_face("F1:+z"), makers, "a face's");
    assert!(leads.for_edge("F1:+z|", &edge_faces).is_empty(), "a name that doesn't read");
    assert!(leads.for_edge("F9:+z|F9:-y", &edge_faces).is_empty(), "a feature that made nothing here");
}

/// Quarter arcs of radius `r` round `c` in the plane z = 0, from `from` degrees, as a tessellated edge.
fn arc_at(c: [f64; 2], r: f64, from: f64) -> (usize, Vec<Vec3>) {
    let at = |d: f64| Vec3::new(c[0] + r * (from + d).to_radians().cos(), c[1] + r * (from + d).to_radians().sin(), 0.0);
    (0, (0..=9).map(|i| at(f64::from(i) * 10.0)).collect())
}

/// Pieces are one circle only when every one lies on it: another centre or radius, however near, is
/// another circle, and a straight piece isn't a circle. Pieces too short to read a circle from alone
/// are read together, so short arcs of one circle are it and of two circles aren't; a piece of only
/// two points could be a chord, and no pieces are none.
#[test]
fn pieces_are_one_circle_only_when_they_all_lie_on_it() {
    let c = [40.0, 15.0];
    let first = arc_at(c, 6.0, 0.0);
    let found = one_circle(&[first.clone(), arc_at(c, 6.0, 180.0)]).unwrap();
    assert!(found.dist(Vec3::new(40.0, 15.0, 0.0)) < 1e-9, "{found:?}");
    assert!(one_circle(std::slice::from_ref(&first)).is_some());
    assert!(one_circle(&[first.clone(), arc_at([40.0 + 1e-9, 15.0], 6.0, 180.0)]).is_some(), "rounding is the same circle");
    assert!(one_circle(&[first.clone(), arc_at([40.0 + 1e-3, 15.0], 6.0, 180.0)]).is_none(), "another centre");
    assert!(one_circle(&[first.clone(), arc_at(c, 6.0 + 1e-3, 180.0)]).is_none(), "another radius");
    let above: Vec<Vec3> = arc_at(c, 6.0, 180.0).1.iter().map(|p| Vec3::new(p.x, p.y, 5.0)).collect();
    assert!(one_circle(&[first.clone(), (1, above)]).is_none(), "the same circle moved along its axis");
    let tilted: Vec<Vec3> = arc_at(c, 6.0, 180.0).1.iter().map(|p| Vec3::new(40.0, p.y, p.x - 40.0)).collect();
    assert!(one_circle(&[first.clone(), (1, tilted)]).is_none(), "round another axis");
    let line: Vec<Vec3> = (0..5).map(|i| Vec3::new(f64::from(i), 0.0, 0.0)).collect();
    assert!(one_circle(&[first.clone(), (1, line)]).is_none(), "a straight piece");
    assert!(one_circle(&[first.clone(), (1, first.1[..3].to_vec())]).is_some(), "too short to read, but on it");
    let off: Vec<Vec3> = first.1[..3].iter().map(|p| Vec3::new(p.x + 1e-3, p.y, p.z)).collect();
    assert!(one_circle(&[first.clone(), (1, off)]).is_none(), "too short to read, and off it");
    assert!(one_circle(&[first.clone(), (1, first.1[..2].to_vec())]).is_none(), "two points could be a chord");
    let short = |c: [f64; 2]| (1, arc_at(c, 6.0, 180.0).1[..3].to_vec());
    let three = (0, first.1[..3].to_vec());
    assert!(one_circle(&[three.clone(), short(c)]).is_some(), "short arcs of one circle, read together");
    assert!(one_circle(&[three, short([45.0, 15.0])]).is_none(), "short arcs of two circles of one radius");
    assert!(one_circle(&[]).is_none());
}

/// Faces lie on one cone when they share its apex, its axis's way and its half-angle, within the
/// cylinder's margins: a double cone's other half (the same apex and axis line, facing the other
/// way) is another surface, and so is a cone whose half-angle or apex is off by more.
#[test]
fn faces_lie_on_one_cone_by_its_apex_axis_and_half_angle() {
    let cone = |apex: Vec3, axis: Vec3, half_angle: f64| Curved::Cone(Cone { apex, axis, half_angle });
    let a = cone(Vec3::ZERO, Vec3::Z, 0.5);
    assert!(a.same(&cone(Vec3::new(1e-7, 0.0, 0.0), Vec3::Z, 0.5 + 1e-6), 1e-6));
    assert!(!a.same(&cone(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), 0.5), 1e-6), "a double cone's other half");
    assert!(!a.same(&cone(Vec3::ZERO, Vec3::Z, 0.5 + 1e-3), 1e-6), "another half-angle");
    assert!(!a.same(&cone(Vec3::new(1e-3, 0.0, 0.0), Vec3::Z, 0.5), 1e-6), "another apex");
    assert!(!a.same(&Curved::Sphere(Sphere { center: Vec3::ZERO, radius: 1.0 }), 1e-6));
}
