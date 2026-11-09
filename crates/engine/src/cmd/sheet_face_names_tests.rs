use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A tray: a base `w` x 200 (its width a sketch dimension: the parameter returned), 1 mm thick
/// with 1 mm bends, front and back flanges 30 high in one feature, left and right in another,
/// and a 6.6 mm hole in the base: the base flange's and the two flange features' ids.
fn tray(s: &mut Session, w: f64) -> (String, u64, u64, u64) {
    run(s, "sheet.manage_rules", json!({"name": "R1", "thickness": "1 mm", "bend_radius": "1 mm", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [w, 200]}));
    let d = run(s, "sketch.dimension", json!({"entities": ["l1"], "value": w}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
    let base = s.doc.features.last().map_or(0, |f| f.id);
    run(s, "sheet.flange", json!({"edges": [[100, 0, 1], [100, 200, 1]], "height": 30}));
    let front_back = s.doc.features.last().map_or(0, |f| f.id);
    run(s, "sheet.flange", json!({"edges": [[0, 100, 1], [w, 100, 1]], "height": 30}));
    let sides = s.doc.features.last().map_or(0, |f| f.id);
    run(s, "solid.hole", json!({"position": [100, 100, 1], "diameter": 6.6}));
    let param = d["param"].as_str().or_else(|| d["name"].as_str()).map(str::to_string).unwrap_or_else(|| "d1".into());
    (param, base, front_back, sides)
}

/// Each face's name (the model's: a sheet made from surfaces has no names of its own, #7990), with
/// its centroid and cylinder radius.
fn faces(s: &mut Session) -> Vec<(Option<String>, Vec3, Option<f64>)> {
    let body = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    let r = run(s, "model.faces", json!({"body": body}));
    r["faces"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let c = &f["centroid"];
            let at = Vec3::new(c[0].as_f64().unwrap_or(f64::NAN), c[1].as_f64().unwrap_or(f64::NAN), c[2].as_f64().unwrap_or(f64::NAN));
            (f["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string), at, f["cylinder"]["radius"].as_f64())
        })
        .collect()
}

fn named<'a>(faces: &'a [(Option<String>, Vec3, Option<f64>)], name: &str) -> Vec<&'a (Option<String>, Vec3, Option<f64>)> {
    faces.iter().filter(|f| f.0.as_deref() == Some(name)).collect()
}

/// The point on the face with this name, as a command taking a face name finds it.
fn point(s: &Session, name: &str) -> Vec3 {
    solidcraft_doc::naming::named_face_point(&s.model.state(), None, name).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A sheet's faces are named by the model, after what made each (#7990): a panel's faces after
/// the feature that made it and the face of the rim it stands on (`F<flange>:top:F<base>:side:l2`,
/// the right flange on the base's sketch line `l2`), a bend's after its panel's maker, a rim after
/// what made its edge. Every face has its own name, and the names follow the tray's size.
#[test]
fn a_trays_faces_have_readable_sheet_names_that_follow_its_size() {
    let mut s = Session::default();
    let (width, b, fb, sd) = tray(&mut s, 320.0);
    let all = faces(&mut s);
    let mut names: Vec<&str> = all.iter().filter_map(|f| f.0.as_deref()).collect();
    assert_eq!(names.len(), all.len(), "every face has a name: {all:?}");
    names.sort_unstable();
    let n = names.len();
    names.dedup();
    assert_eq!(names.len(), n, "{names:?}");
    for (name, at) in [
        (format!("F{b}:top"), Vec3::new(160.0, 100.0, 1.0)),
        (format!("F{b}:bottom"), Vec3::new(160.0, 100.0, 0.0)),
        (format!("F{sd}:top:F{b}:side:l2"), Vec3::new(319.0, 100.0, 16.0)),
        (format!("F{sd}:bottom:F{b}:side:l2"), Vec3::new(320.0, 100.0, 16.0)),
        (format!("F{sd}:far:F{b}:side:l2"), Vec3::new(319.5, 100.0, 30.0)),
        (format!("F{fb}:bottom:F{b}:side:l1"), Vec3::new(160.0, 0.0, 16.0)),
    ] {
        let f = named(&all, &name);
        assert!(f.len() == 1 && f[0].1.dist(at) < 2.5, "{name}: {f:?}");
    }
    // The front flange runs on past its bend to the right one, under its corner: that stretch of
    // its edge along the bend is a rim of its own. The side flanges made it, closing the corner, so
    // it is theirs, named after the front bend's end it runs past (#7990: a face is named by the
    // feature that made it, after the faces it was made from): `past0` at the front bend's anchor
    // origin, the end its `bend.end0` is.
    let past = format!("F{sd}:past0:F{fb}:bend.end0:F{b}:side:l1");
    let f = named(&all, &past);
    assert!(f.len() == 1 && f[0].1.dist(Vec3::new(319.0, 0.5, 2.0)) < 2.5, "{past}: {f:?}");
    // A bend's two curved faces: its radius and the radius plus the thickness.
    let radius = |name: &str| named(&all, name).first().and_then(|f| f.2);
    assert_eq!(radius(&format!("F{fb}:bend.inner:F{b}:side:l1")).map(|r| (r * 1e6).round() / 1e6), Some(1.0));
    assert_eq!(radius(&format!("F{fb}:bend.outer:F{b}:side:l1")).map(|r| (r * 1e6).round() / 1e6), Some(2.0));
    // 80 mm wider: the same names, each on the same face where it now is. A name's point is
    // somewhere on its face: the right flange's top is the plane x = w − 1, the back flange's
    // far rim the strip z = 30, 199 <= y <= 200 (the flange 1 thick, its outside at y = 200).
    let right = point(&s, &format!("F{sd}:top:F{b}:side:l2"));
    assert!((right.x - 319.0).abs() < 1e-3, "{right:?}");
    run(&mut s, "parameters.change", json!({"name": width, "expression": "400"}));
    let wider = faces(&mut s);
    let mut again: Vec<&str> = wider.iter().filter_map(|f| f.0.as_deref()).collect();
    again.sort_unstable();
    assert_eq!(again, names);
    let right = point(&s, &format!("F{sd}:top:F{b}:side:l2"));
    assert!((right.x - 399.0).abs() < 1e-3, "{right:?}");
    let rim = format!("F{fb}:far:F{b}:side:l3");
    let far = point(&s, &rim);
    let st = s.model.state();
    let hit = solidcraft_doc::naming::locate_one(&st, None, &rim).unwrap();
    on_planar_face(&st.bodies[hit.body].body, hit.face, far);
    assert!((far.z - 30.0).abs() < 1e-9 && (199.0..=200.0).contains(&far.y), "the back flange's far rim: {far:?}");
}

/// `p` lies on face `face` of `body`, on its exact geometry: within rounding at the part's scale
/// of the plane the face lies on, and inside the boundary its edges trace there (each a line,
/// counted by crossings).
fn on_planar_face(body: &solidcraft_kernel::Body, face: usize, p: Vec3) {
    use solidcore_record::brep::{Curve, Loop};
    let rounding = 1e-12 * body.size().max(1.0);
    let surface = solidcraft_kernel::face_surface(body, face).unwrap();
    let on = surface.nearest(p).unwrap();
    assert!(on.dist(p) <= rounding, "{p:?} is {} off face {face}'s surface", on.dist(p));
    let solidcraft_kernel::FaceSurface::Plane { normal, .. } = surface else { panic!("face {face} is not planar: {surface:?}") };
    // Coordinates in the plane: along `x` and `y`, square to its normal.
    let x = if normal.cross(Vec3::X).len() > 0.5 { normal.cross(Vec3::X).normalized().unwrap() } else { normal.cross(Vec3::Y).normalized().unwrap() };
    let y = normal.cross(x);
    let local = |q: Vec3| ((q - p).dot(x), (q - p).dot(y));
    let record = body.record();
    let point = |v: u32| {
        let q = record.vertices[v as usize].point;
        Vec3::new(q[0], q[1], q[2])
    };
    // Crossings of the ray from `p` along +x with the face's boundary.
    let mut crossings = 0;
    for l in &record.faces[face].loops {
        let Loop::Edges { edges } = l else { continue };
        for e in edges {
            let edge = &record.edges[e.edge as usize];
            assert!(matches!(edge.parameters(), Curve::Line { .. }), "an edge of face {face} is not a line: {:?}", edge.parameters());
            let [a, b] = edge.vertices.map(|v| local(point(v)));
            let distance = {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (-(a.0 * dx + a.1 * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                (a.0 + t * dx).hypot(a.1 + t * dy)
            };
            assert!(distance > rounding, "{p:?} is on the boundary of face {face}");
            if (a.1 > 0.0) != (b.1 > 0.0) && a.0 + (b.0 - a.0) * (0.0 - a.1) / (b.1 - a.1) > 0.0 {
                crossings += 1;
            }
        }
    }
    assert_eq!(crossings % 2, 1, "{p:?} is outside face {face}'s boundary");
}

#[test]
fn a_flanges_sheet_names_hold_when_another_flange_goes() {
    let mut s = Session::default();
    let (_, b, fb, sd) = tray(&mut s, 320.0);
    let right = format!("F{sd}:top:F{b}:side:l2");
    let before = point(&s, &right);
    run(&mut s, "timeline.suppress", json!({"feature": fb, "suppressed": true}));
    let after = point(&s, &right);
    assert!((after.x - 319.0).abs() < 1e-3 && (after.z - before.z).abs() < 16.0, "{before:?} {after:?}");
    // The names of the flanges that went are gone; asking for one says so.
    let gone = format!("F{fb}:top:F{b}:side:l1");
    assert!(solidcraft_doc::naming::named_face_point(&s.model.state(), None, &gone).is_err());
    assert!(solidcraft_doc::naming::named_face_point(&s.model.state(), Some("Body2"), &right).is_err());
}

/// A fold at 45° across a strip, with a bend zone wide for the strip: about a third of the bend's
/// faces lies beyond the ends of its start line, along the bend, and they still take its names:
/// the fold's, after the face its line was drawn on.
#[test]
fn a_bend_askew_across_a_sheet_keeps_its_names() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "R10", "thickness": "1 mm", "bend_radius": "10 mm", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Strip"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [200, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Strip"}));
    let b = s.doc.features.last().map_or(0, |f| f.id);
    run(&mut s, "sheet.fold", json!({"points": [[90, -5, 1], [115, 20, 1]], "fixed": [10, 10, 1]}));
    let fold = s.doc.features.last().map_or(0, |f| f.id);
    let all = faces(&mut s);
    for (side, radius) in [("inner", 10.0), ("outer", 11.0)] {
        let name = format!("F{fold}:bend.{side}:F{b}:top");
        let f = named(&all, &name);
        assert!(f.len() == 1 && f[0].2.is_some_and(|r| (r - radius).abs() < 1e-6), "{name}: {all:?}");
    }
}

/// A fold keeps the side it was made with, and so do that side's faces their names (#6748). A
/// 100 x 60 plate (the default rule: 2.5 thick, 2.5 bend radius, k 0.44) folded at x = 60, a
/// parameter, with no fixed point: the larger side, left of the line drawn along +y as seen from
/// the top (the sheet's surface), stays put and the 40 mm side stands up. Moved to x = 40, the
/// line keeps its left side put, now the smaller one, and the 60 mm side stands up.
#[test]
fn a_fold_keeps_its_side_and_that_sides_names_when_its_line_moves() {
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = std::f64::consts::FRAC_PI_2 * (r + k * t);
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "x_fold", "expression": "60 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": t, "name": "FoldLine"}));
    run(&mut s, "sketch.line", json!({"points": [[60, 0], [60, 60]], "ids": ["fl"], "connect": false}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "fl", "mode": "vertical"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "fl.start"], "type": "horizontal", "value": "x_fold"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.fold", json!({"sketch": "FoldLine", "curve": "fl"}));
    let stays = |s: &Session| match s.doc.features.last().map(|f| &f.kind) {
        Some(solidcraft_doc::FeatureKind::SheetFold { stays, .. }) => *stays,
        _ => None,
    };
    assert_eq!(stays(&s), Some(solidcraft_doc::FoldSide::Left));
    // With the line at x, the right side stands up: its outer face at the zone's start, x − BA/2,
    // plus R + T, its leg 100 − x − BA/2 tall above the bend; the left side lies flat from x = 0.
    // The faces that stay are the left side's flat faces short of the zone (its top, bottom and
    // three rims), the names they carry: each named, no two alike.
    let staying = |s: &mut Session, x: f64| {
        let b = run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"].clone();
        assert!(b["min"][0].as_f64().is_some_and(|v| v.abs() < 1e-6), "{x}: {b}");
        assert!(b["max"][0].as_f64().is_some_and(|v| (v - (x - ba / 2.0 + r + t)).abs() < 1e-6), "{x}: {b}");
        assert!(b["max"][2].as_f64().is_some_and(|v| (v - (t + r + 100.0 - x - ba / 2.0)).abs() < 1e-6), "{x}: {b}");
        let all = faces(s);
        let flat: Vec<_> = all.iter().filter(|f| f.2.is_none() && f.1.z < t + 1e-6 && f.1.x < x - ba / 2.0 - 1e-3).collect();
        let mut names: Vec<String> = flat.iter().filter_map(|f| f.0.clone()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!((flat.len(), names.len()), (5, 5), "{x}: {flat:?}");
        names
    };
    let before = staying(&mut s, 60.0);
    run(&mut s, "parameters.change", json!({"name": "x_fold", "expression": "40 mm"}));
    assert_eq!(stays(&s), Some(solidcraft_doc::FoldSide::Left));
    assert_eq!(staying(&mut s, 40.0), before);
}

/// The faces' names, by where each is (its centroid, to a hundredth of a millimetre).
fn names_by_place(s: &mut Session) -> std::collections::BTreeMap<[i64; 3], String> {
    faces(s).into_iter().filter_map(|(name, at, _)| Some(([at.x, at.y, at.z].map(|v| (v * 100.0).round() as i64), name?))).collect()
}

/// #7990: a flange feature's picks in another order make the same flanges, and every face of the
/// sheet has the name it had: a panel is keyed by what it was made on, never by the order of the
/// picks, and its faces are named after that.
#[test]
fn reordering_a_flanges_picks_renames_no_face() {
    let made = |edges: Value| {
        let mut s = Session::default();
        run(&mut s, "sheet.manage_rules", json!({"name": "R1", "thickness": "1 mm", "bend_radius": "1 mm", "active": true}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [200, 150]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
        run(&mut s, "sheet.flange", json!({"edges": edges, "height": 20}));
        names_by_place(&mut s)
    };
    // Two flanges on opposite edges, so that no corner between them depends on the order.
    let one = made(json!([[100, 0, 1], [100, 150, 1]]));
    let other = made(json!([[100, 150, 1], [100, 0, 1]]));
    assert!(!one.is_empty());
    assert_eq!(one, other);
}

/// #7990: a panel that goes (its flange suppressed, the hole cut through the base after it kept)
/// leaves every other face with the name it had: no name is a place among panels or faces.
#[test]
fn a_panel_that_goes_leaves_every_other_faces_name() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "R1", "thickness": "1 mm", "bend_radius": "1 mm", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [200, 150]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    run(&mut s, "sheet.flange", json!({"edges": [[100, 0, 1]], "height": 20}));
    let front = s.doc.features.last().map_or(0, |f| f.id);
    run(&mut s, "sheet.flange", json!({"edges": [[100, 150, 1]], "height": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 75], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Hole", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    let before: std::collections::BTreeSet<String> = faces(&mut s).into_iter().filter_map(|f| f.0).collect();
    run(&mut s, "timeline.suppress", json!({"feature": front, "suppressed": true}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results.iter().filter_map(|r| r.error.clone()).collect::<Vec<_>>());
    let after: std::collections::BTreeSet<String> = faces(&mut s).into_iter().filter_map(|f| f.0).collect();
    let kept: Vec<&String> = before.iter().filter(|n| !n.starts_with(&format!("F{front}:"))).collect();
    let lost: Vec<&&String> = kept.iter().filter(|n| !after.contains(**n)).collect();
    assert!(lost.is_empty(), "renamed when the front flange went: {lost:?}");
}

/// The names of a Ø10 hole's two half walls cut through a 100 x 60 plate at (50, 30), by which
/// side of the hole's center (along y) each lies.
fn hole_halves(s: &mut Session) -> (String, String) {
    let all = faces(s);
    let walls: Vec<_> = all.iter().filter(|f| f.2.is_some_and(|r| (r - 5.0).abs() < 1e-6)).collect();
    assert_eq!(walls.len(), 2, "{all:?}");
    let name = |up: bool| walls.iter().find(|f| (f.1.y > 30.0) == up).and_then(|f| f.0.clone()).unwrap_or_default();
    (name(true), name(false))
}

/// #7990, change 4: a hole's two halves are numbered in the order they start along its sketch
/// circle (from where the circle starts, round the way it runs), so the half above its center
/// is `#1` whether the sheet's frame is mirrored or not, and stays so through an unfold and a
/// refold. Numbered by where the halves lie (the display mesh's spread), a mirror could swap them.
#[test]
fn a_holes_halves_keep_their_numbers_when_the_sheet_is_mirrored() {
    for flip in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Plate", "flip": flip}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
        let circle = run(&mut s, "sketch.circle.center", json!({"center": [50, 30], "radius": 5}));
        let curve = circle["curves"][0].as_str().unwrap_or("c1").to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Hole", "through_all": true, "operation": "cut", "direction": "symmetric"}));
        let cut = s.doc.features.last().map_or(0, |f| f.id);
        let want = (format!("F{cut}:side:{curve}#1"), format!("F{cut}:side:{curve}#2"));
        assert_eq!(hole_halves(&mut s), want, "flip {flip}");
        run(&mut s, "sheet.unfold", json!({}));
        assert_eq!(hole_halves(&mut s), want, "flip {flip}, unfolded");
        run(&mut s, "sheet.refold", json!({}));
        assert_eq!(hole_halves(&mut s), want, "flip {flip}, refolded");
    }
}

/// #7990: unfolding, cutting a hole into the flat and refolding keeps every face's name: the flat
/// is built of the folded sheet's faces, each keeping its name, and the refold gives them back.
/// The hole's walls are the cut's, after its sketch circle.
#[test]
fn unfold_cut_and_refold_keep_every_name() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Plate"}));
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    let folded: std::collections::BTreeSet<String> = faces(&mut s).into_iter().filter_map(|f| f.0).collect();
    run(&mut s, "sheet.unfold", json!({}));
    let flat: std::collections::BTreeSet<String> = faces(&mut s).into_iter().filter_map(|f| f.0).collect();
    assert_eq!(flat, folded, "the flat's faces are the folded sheet's, laid flat");
    run(&mut s, "sketch.create", json!({"plane": {"face": [70, 30, 2.5]}, "name": "Hole"}));
    let circle = run(&mut s, "sketch.circle.center", json!({"center": [70, 30], "radius": 4}));
    let curve = circle["curves"][0].as_str().unwrap_or("c1").to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Hole", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    let cut = s.doc.features.last().map_or(0, |f| f.id);
    run(&mut s, "sheet.refold", json!({}));
    let refolded: std::collections::BTreeSet<String> = faces(&mut s).into_iter().filter_map(|f| f.0).collect();
    let lost: Vec<&String> = folded.iter().filter(|n| !refolded.contains(*n)).collect();
    assert!(lost.is_empty(), "renamed by the unfold, cut and refold: {lost:?}");
    let walls: Vec<&String> = refolded.difference(&folded).collect();
    assert!(!walls.is_empty() && walls.iter().all(|n| n.starts_with(&format!("F{cut}:side:{curve}#"))), "{walls:?}");
}
