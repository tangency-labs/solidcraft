//! Formed beads on a sheet (#6748): a bead is a form on a planar panel, kept in the panel's chart,
//! built in the panel's frame and sewn with its faces in the folded solid, which the model names
//! after the bead feature and its centreline (#7990).

use solidcore_record::Vec2;

use super::*;

const T: f64 = 1.0;

/// A plate 100 x 400 x 1 thickened from its top (the sheet's surface, z = 1; the material
/// below), R 1.
fn plate() -> Session {
    let mut s = Session::default();
    rule(&mut s, "Test", T, 0.44, 1.0, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 400, "height": T, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Plate", "face": [50, 200, T]}));
    s
}

/// A sketch `at` mm up the z axis with one line from `a` to `b`.
fn line(s: &mut Session, name: &str, at: f64, a: [f64; 2], b: [f64; 2]) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": at, "name": name}));
    run(s, "sketch.line", json!({"points": [a, b], "ids": ["c1"]}));
    run(s, "sketch.finish", json!({}));
}

/// A bead 2 deep and 10 wide (45°, R 1 at the base and the crown, 6 long ramps) along the
/// sketch's line.
fn bead(s: &mut Session, sketch: &str, flip: bool) -> Value {
    run(
        s,
        "sheet.bead",
        json!({"sketch": sketch, "curves": ["c1"], "depth": 2, "width": 10, "base_radius": 1, "crown_radius": 1, "end_length": 6, "flip": flip}),
    )
}

/// The plate's volume, finely tessellated, its shells and faces, and its bounding box's lowest and
/// highest z.
fn measured(s: &Session) -> (f64, usize, usize, f64, f64) {
    let b = s.model.state().bodies.iter().find(|b| b.name == "Plate").map(|b| b.body.clone()).expect("the plate");
    let m = solidcraft_kernel::measure(&b).expect("measured");
    (b.volume_at(1e-4).unwrap_or(f64::NAN), m.shells, b.face_count(), m.bbox.min.z, m.bbox.max.z)
}

/// A bead is sewn into its plate as one closed solid with its 26 faces, out of the face its
/// sketch is on or, flipped, out of the other one; 100 mm more of its straight part adds
/// t·(S − 2·xf)·100, S being the mid-surface length of its cross-section and 2·xf its
/// footprint's width.
#[test]
fn a_bead_is_sewn_into_its_panel_and_its_straight_part_adds_its_cross_section() {
    let (rb, rc, t, d, w, a) = (1.0, 1.0, T, 2.0, 10.0, std::f64::consts::FRAC_PI_4);
    let ro = rc + t;
    let half = (a / 2.0).tan();
    let xf = w / 2.0 + rb * half;
    let uc = rb * half + d / a.tan() + ro * half;
    let flank = Vec2::new(rb * a.sin(), rb * (1.0 - a.cos())).dist(Vec2::new(uc - ro * a.sin(), d - ro + ro * a.cos()));
    let mid = 2.0 * ((rb + t / 2.0) * a + flank + (rc + t / 2.0) * a) + 2.0 * (xf - uc);
    let added = t * (mid - 2.0 * xf) * 100.0;
    let with = |len: f64, flip: bool| {
        let mut s = plate();
        line(&mut s, "Beads", T, [50.0, 20.0], [50.0, 20.0 + len]);
        bead(&mut s, "Beads", flip);
        s
    };
    let (v0, ..) = measured(&plate());
    let (v360, shells, faces, z0, z1) = measured(&with(360.0, false));
    let (v260, ..) = measured(&with(260.0, false));
    eprintln!("plate {v0:.4}, bead 360 {v360:.4}, bead 260 {v260:.4}; added {:.4}, expected {added:.4}", v360 - v260);
    assert_eq!((shells, faces), (1, 6 + 26));
    // Out of the sheet's surface (the top, z = 1): the crown 2 above it.
    assert!((z1 - 3.0).abs() < 1e-6 && z0.abs() < 1e-6, "{z0} … {z1}");
    assert!((v360 - v260 - added).abs() < 1e-3 * added, "added {} vs {added}", v360 - v260);
    // Flipped: out of the other face (z = 0), the same volume.
    let (vd, sd, fd, zd0, zd1) = measured(&with(360.0, true));
    assert_eq!((sd, fd), (1, 32));
    assert!((vd - v360).abs() < 1e-6 * v360 && (zd0 + 2.0).abs() < 1e-6 && (zd1 - 1.0).abs() < 1e-6, "{vd} {zd0} … {zd1}");
}

/// The flat pattern lists the bead where its panel lies in the flat, on the blank as the flat
/// pattern draws it (the blank stays flat), and the DXF draws its centreline and footprint on
/// FORM_UP or FORM_DOWN, with a note.
#[test]
fn a_bead_is_listed_in_the_flat_pattern_and_drawn_in_the_dxf() {
    let mut s = plate();
    line(&mut s, "Beads", T, [50.0, 40.0], [50.0, 360.0]);
    bead(&mut s, "Beads", false);
    let f = flat(&mut s);
    let beads = f["beads"].as_array().cloned().unwrap_or_default();
    assert_eq!(beads.len(), 1, "{f}");
    let b = &beads[0];
    assert_eq!(b["direction"], "up");
    let len = |p: &Value, q: &Value| {
        let (px, py, qx, qy) = (
            p[0].as_f64().unwrap_or(f64::NAN),
            p[1].as_f64().unwrap_or(f64::NAN),
            q[0].as_f64().unwrap_or(f64::NAN),
            q[1].as_f64().unwrap_or(f64::NAN),
        );
        ((px - qx).powi(2) + (py - qy).powi(2)).sqrt()
    };
    assert!((len(&b["start"], &b["end"]) - 320.0).abs() < 1e-9, "{b}");
    assert_eq!(b["footprint"].as_array().map_or(0, Vec::len), 4);
    // The blank is the plate: 100 x 400, no hole for the bead.
    assert!(f["cutouts"].as_array().is_some_and(Vec::is_empty), "{f}");
    // The bead lies on the blank as the flat pattern reads it: inside its outline's box, with
    // its centreline down the middle.
    let xy = |p: &Value| (p[0].as_f64().unwrap_or(f64::NAN), p[1].as_f64().unwrap_or(f64::NAN));
    let outline: Vec<(f64, f64)> = f["outline"].as_array().into_iter().flatten().flat_map(|l| l.as_array().into_iter().flatten().map(xy)).collect();
    let (x0, x1) = outline.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (y0, y1) = outline.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    assert!((x1 - x0 - 100.0).abs() < 1e-6 && (y1 - y0 - 400.0).abs() < 1e-6, "{f}");
    let corners: Vec<(f64, f64)> = b["footprint"].as_array().into_iter().flatten().map(xy).chain([xy(&b["start"]), xy(&b["end"])]).collect();
    assert!(corners.iter().all(|&(x, y)| x > x0 && x < x1 && y > y0 && y < y1), "{b} on {f}");
    assert!([xy(&b["start"]), xy(&b["end"])].iter().all(|p| (p.0 - (x0 + x1) / 2.0).abs() < 1e-6), "{b}");
    let dxf = run(&mut s, "sheet.export_dxf", json!({"bend_notes": true}))["dxf"].as_str().unwrap_or_default().to_string();
    assert!(dxf.contains("FORM_UP") && !dxf.contains("FORM_DOWN"), "layers");
    assert!(dxf.contains("BEAD UP D2 W10"), "note");
}

/// A bead whose footprint meets the sheet's edge or a bend, one wholly inside another bead's
/// footprint, and one not on the sheet's faces, are refused, each with its reason; one on a sheet
/// drawn from a sketch (a base flange) is pressed as on a converted one.
#[test]
fn beads_that_cannot_be_pressed_are_refused() {
    let mut s = plate();
    line(&mut s, "Edge", T, [3.0, 40.0], [3.0, 360.0]);
    let e = s.execute("sheet.bead", &json!({"sketch": "Edge", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("meets the sheet's edge"), "{e}");
    line(&mut s, "Off", T + 5.0, [50.0, 40.0], [50.0, 360.0]);
    let e = s.execute("sheet.bead", &json!({"sketch": "Off", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("planar panel of a sheet"), "{e}");
    // A flange on the plate's front edge: a bead running into its bend.
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    line(&mut s, "Bend", T, [50.0, 1.0], [50.0, 200.0]);
    let e = s.execute("sheet.bead", &json!({"sketch": "Bend", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("meets a bend") || e.contains("meets the sheet's edge"), "{e}");
    // A sheet drawn from a sketch.
    let mut f = Session::default();
    rule(&mut f, "Test", T, 0.44, 1.0, 0.5);
    run(&mut f, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
    run(&mut f, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 400]}));
    run(&mut f, "sketch.finish", json!({}));
    run(&mut f, "sheet.flange", json!({"sketch": "Blank"}));
    line(&mut f, "Beads", T, [50.0, 40.0], [50.0, 360.0]);
    if let Err(e) = f.execute("sheet.bead", &json!({"sketch": "Beads", "curves": ["c1"], "depth": 2, "width": 10})) {
        panic!("a bead on a base flange: {e}");
    }
    let faces = f.model.state().bodies.first().map_or(0, |b| b.body.face_count());
    assert_eq!(faces, 6 + 26);
    // A bead wholly inside a wider one's footprint, where no edge of either crosses the other's.
    let mut w = plate();
    line(&mut w, "Wide", T, [50.0, 40.0], [50.0, 360.0]);
    run(&mut w, "sheet.bead", json!({"sketch": "Wide", "curves": ["c1"], "depth": 2, "width": 30}));
    line(&mut w, "Inside", T, [50.0, 100.0], [50.0, 300.0]);
    let e = w.execute("sheet.bead", &json!({"sketch": "Inside", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("meets another bead"), "{e}");
    // A bead wholly inside a pocket (a recess 0.3 into the surface), clear of its edges.
    let mut p = plate();
    run(&mut p, "sketch.create", json!({"plane": "XY", "offset": T, "name": "Pocket"}));
    run(&mut p, "sketch.rectangle.two_point", json!({"p0": [20, 100], "p1": [80, 300]}));
    run(&mut p, "sketch.finish", json!({}));
    run(&mut p, "solid.extrude", json!({"sketch": "Pocket", "distance": 0.3, "direction": "negative", "operation": "cut"}));
    line(&mut p, "InPocket", T, [50.0, 140.0], [50.0, 260.0]);
    let e = p.execute("sheet.bead", &json!({"sketch": "InPocket", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("meets a recess"), "{e}");
}

/// Each face of the plate the model names, by name, with its centroid.
fn named_faces(s: &mut Session) -> Vec<(String, [f64; 3])> {
    let r = run(s, "model.faces", json!({"body": "Plate"}));
    r["faces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let c = &f["centroid"];
            let at = [0, 1, 2].map(|i| c[i].as_f64().unwrap_or(f64::NAN));
            f["name"].as_str().filter(|n| !n.is_empty()).map(|n| (n.to_string(), at))
        })
        .collect()
}

/// Presses a bead 2 deep and 10 wide along `curves` of `sketch`: the prefix its faces' names
/// start with, its id's (`F<id>:`).
fn named_bead(s: &mut Session, sketch: &str, curves: &[&str]) -> String {
    let r = run(s, "sheet.bead", json!({"sketch": sketch, "curves": curves, "depth": 2, "width": 10}));
    format!("F{}:", r["feature"].as_u64().unwrap_or_default())
}

/// The names of the plate's faces that start with `prefix`, sorted.
fn bead_names(s: &mut Session, prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = named_faces(s).into_iter().map(|(n, _)| n).filter(|n| n.starts_with(prefix)).collect();
    names.sort();
    names
}

/// A bead's 26 faces are the bead feature's, each named after the centreline's sketch curve and
/// what the face is: its crown, and the base bend, flank and crown bend on its left and right
/// (seen from the sheet's surface, from the centreline's start to its end) and the base bend,
/// ramp and crown bend at its start and end, each on the skin that continues the sheet's surface
/// (`top`) or the other one (`bottom`).
#[test]
fn a_beads_faces_are_named_by_its_feature_after_its_centreline() {
    for flip in [false, true] {
        let mut s = plate();
        // Along +y at x = 50 on the surface (z = 1), seen from above: its left is toward −x.
        line(&mut s, "Beads", T, [50.0, 40.0], [50.0, 360.0]);
        let r = bead(&mut s, "Beads", flip);
        let prefix = format!("F{}:", r["feature"].as_u64().unwrap_or_default());
        let faces = named_faces(&mut s);
        let rib: Vec<&(String, [f64; 3])> = faces.iter().filter(|(n, _)| n.starts_with(&prefix)).collect();
        let names: std::collections::BTreeSet<&str> = rib.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!((rib.len(), names.len()), (26, 26), "flip {flip}: {faces:?}");
        let at = |role: &str| -> [f64; 3] {
            let hits: Vec<[f64; 3]> = rib.iter().filter(|(n, _)| *n == format!("{prefix}{role}:c1")).map(|(_, c)| *c).collect();
            assert_eq!(hits.len(), 1, "flip {flip}, {role}: {rib:?}");
            hits[0]
        };
        // Up out of the surface, the crown's top skin 2 above it and its bottom a thickness lower;
        // flipped, down out of the other face (z = 0), the crown's bottom skin 2 below it.
        let (top, bottom) = if flip { (-1.0, -2.0) } else { (3.0, 2.0) };
        assert!((at("crown.top")[2] - top).abs() < 1e-6 && (at("crown.bottom")[2] - bottom).abs() < 1e-6, "flip {flip}: {rib:?}");
        for skin in ["top", "bottom"] {
            for piece in ["base_bend", "flank", "crown_bend"] {
                let (l, r) = (at(&format!("left.{piece}.{skin}")), at(&format!("right.{piece}.{skin}")));
                assert!(l[0] < 50.0 && r[0] > 50.0, "flip {flip}, {piece} {skin}: {rib:?}");
            }
            for piece in ["base_bend", "ramp", "crown_bend"] {
                let (a, b) = (at(&format!("start.{piece}.{skin}")), at(&format!("end.{piece}.{skin}")));
                assert!(a[1] < 200.0 && b[1] > 200.0, "flip {flip}, {piece} {skin}: {rib:?}");
            }
        }
    }
}

/// A bead's faces keep their names when the sheet is unfolded (the blank stays flat, so they are
/// not there) and folded again, and when another bead is pressed beside it; and one bead's names
/// do not depend on the other centrelines of its feature.
#[test]
fn a_beads_names_survive_unfolding_refolding_and_other_beads() {
    let mut s = plate();
    line(&mut s, "Beads", T, [50.0, 40.0], [50.0, 360.0]);
    let rib = named_bead(&mut s, "Beads", &["c1"]);
    let before = bead_names(&mut s, &rib);
    assert_eq!(before.len(), 26, "{before:?}");
    run(&mut s, "sheet.unfold", json!({}));
    assert!(bead_names(&mut s, &rib).is_empty(), "the flat blank has no bead");
    run(&mut s, "sheet.refold", json!({}));
    assert_eq!(bead_names(&mut s, &rib), before);
    line(&mut s, "More", T, [20.0, 40.0], [20.0, 360.0]);
    let more = named_bead(&mut s, "More", &["c1"]);
    assert_eq!(bead_names(&mut s, &more).len(), 26);
    assert_eq!(bead_names(&mut s, &rib), before, "the first bead's names after a second bead");
    // Two centrelines in one feature: the bead along c1 is named as when it was alone (the same
    // feature ids in both designs: the plate's, the sketch's, then the bead's).
    let mut two = plate();
    run(&mut two, "sketch.create", json!({"plane": "XY", "offset": T, "name": "Beads"}));
    run(&mut two, "sketch.line", json!({"points": [[50, 40], [50, 360]], "ids": ["c1"]}));
    run(&mut two, "sketch.line", json!({"points": [[20, 40], [20, 360]], "ids": ["c2"]}));
    run(&mut two, "sketch.finish", json!({}));
    let both = named_bead(&mut two, "Beads", &["c1", "c2"]);
    assert_eq!(both, rib);
    let names = bead_names(&mut two, &both);
    assert_eq!(names.len(), 52, "{names:?}");
    let c1: Vec<String> = names.into_iter().filter(|n| n.ends_with(":c1")).collect();
    assert_eq!(c1, before);
}

/// A fold across the plate takes a bead on its side that moves with it: the bead keeps its 26
/// faces and their names, now on the folded part. A fold whose bend zone runs through a bead is
/// refused.
#[test]
fn a_fold_takes_a_bead_with_its_side_and_refuses_one_through_it() {
    let fold = json!({"points": [[0, 200, T], [100, 200, T]], "fixed": [50, 20, T]});
    let mut s = plate();
    line(&mut s, "Beads", T, [50.0, 260.0], [50.0, 360.0]);
    let r = bead(&mut s, "Beads", false);
    let prefix = format!("F{}:", r["feature"].as_u64().unwrap_or_default());
    let before = bead_names(&mut s, &prefix);
    assert_eq!(before.len(), 26, "{before:?}");
    run(&mut s, "sheet.fold", fold.clone());
    assert_eq!(bead_names(&mut s, &prefix), before);
    // Folded square, the part beyond y = 200 stands up or hangs down: the crown with it.
    let crown = named_faces(&mut s).into_iter().find(|(n, _)| *n == format!("{prefix}crown.top:c1")).map(|(_, c)| c);
    assert!(crown.is_some_and(|c| c[2].abs() > 50.0), "{crown:?}");
    // One shell, with the bead's 26 faces more than the same plate folded without it.
    let mut bare = plate();
    run(&mut bare, "sheet.fold", fold.clone());
    let (.., bare_faces, _, _) = measured(&bare);
    let (_, shells, faces, ..) = measured(&s);
    assert_eq!((shells, faces), (1, bare_faces + 26));
    let mut x = plate();
    line(&mut x, "Beads", T, [50.0, 150.0], [50.0, 250.0]);
    bead(&mut x, "Beads", false);
    let e = x.execute("sheet.fold", &fold).unwrap_err().to_string();
    assert!(e.contains("runs through a bead"), "{e}");
}

/// A mirrored copy of a beaded plate has its bead where the mirror puts it: a bead off the
/// plate's middle, at x = 30, is at x = −30 on the copy across the YZ plane.
#[test]
fn a_mirrored_copy_has_its_bead_where_the_mirror_puts_it() {
    let mut s = plate();
    line(&mut s, "Beads", T, [30.0, 40.0], [30.0, 360.0]);
    bead(&mut s, "Beads", false);
    run(&mut s, "solid.mirror", json!({"bodies": ["Plate"], "plane": "YZ"}));
    let copy = s.model.state().bodies.iter().map(|b| b.name.clone()).find(|n| n != "Plate").unwrap_or_default();
    let faces = run(&mut s, "model.faces", json!({"body": copy}));
    let centroids: Vec<[f64; 3]> =
        faces["faces"].as_array().into_iter().flatten().map(|f| [0, 1, 2].map(|i| f["centroid"][i].as_f64().unwrap_or(f64::NAN))).collect();
    assert_eq!(centroids.len(), 6 + 26, "{faces}");
    // The crown's top, 2 above the surface.
    let crowns: Vec<&[f64; 3]> = centroids.iter().filter(|c| (c[2] - 3.0).abs() < 1e-6).collect();
    assert!(crowns.len() == 1 && (crowns[0][0] + 30.0).abs() < 1e-6, "{crowns:?}");
}

/// Clearance is decided on the exact outline: a bead a hair (5e-4 mm) clear of the plate's
/// straight edge is pressed, with the margin's warning. A centreline across a hole between its
/// ends lies on no panel, so no sheet takes it.
#[test]
fn a_bead_a_hair_clear_of_an_edge_is_pressed_and_one_across_a_hole_is_not() {
    let xf = 5.0 + (std::f64::consts::FRAC_PI_8).tan();
    let x = xf + 5e-4;
    let mut s = plate();
    line(&mut s, "Near", T, [x, 40.0], [x, 360.0]);
    let r = bead(&mut s, "Near", false);
    assert!(r["warning"].as_str().is_some_and(|w| w.contains("the sheet's edge")), "{r}");
    let (_, shells, faces, ..) = measured(&s);
    assert_eq!((shells, faces), (1, 6 + 26));
    let mut h = plate();
    run(&mut h, "sketch.create", json!({"plane": "XY", "offset": T, "name": "Hole"}));
    run(&mut h, "sketch.circle.center", json!({"center": [50, 200], "radius": 5}));
    run(&mut h, "sketch.finish", json!({}));
    run(&mut h, "solid.extrude", json!({"sketch": "Hole", "through_all": true, "direction": "symmetric", "operation": "cut"}));
    line(&mut h, "Across", T, [50.0, 40.0], [50.0, 360.0]);
    let e = h.execute("sheet.bead", &json!({"sketch": "Across", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("centreline must lie on a planar panel"), "{e}");
}

/// #20417 for beads: a sheet in a component moved 200 along x takes a bead whose sketch lies on
/// it in the world, the sketch drawn in the moved component (its local x = 50) or at the root
/// (world x = 250), the bead added at the root, with the sheet named or not. One sketched at
/// world x = 50 lies on no sheet.
#[test]
fn a_bead_on_a_sheet_in_a_moved_component_goes_where_its_sketch_lies() {
    let moved = || {
        let mut s = Session::default();
        run(&mut s, "sheet.manage_rules", json!({"name": "Plate", "active": true, "thickness": "2 mm", "k_factor": 0.4, "bend_radius": "2 mm"}));
        run(&mut s, "component.create", json!({"name": "P", "grounded": false}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [120, 60]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Base", "name": "Plate"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "occurrence.move", json!({"occurrence": "P", "translate": [200, 0, 0]}));
        run(&mut s, "component.capture_position", json!({}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        s
    };
    let along = |s: &mut Session, name: &str, x: f64| {
        run(s, "sketch.create", json!({"plane": "XY", "offset": 2, "name": name}));
        run(s, "sketch.line", json!({"points": [[x, 5], [x, 55]], "ids": ["c1"]}));
        run(s, "sketch.finish", json!({}));
    };
    let press = |s: &mut Session, sketch: &str, body: Option<&str>| {
        let mut p = json!({"sketch": sketch, "curves": ["c1"], "depth": 2, "width": 10});
        if let Some(b) = body {
            p["body"] = json!(b);
        }
        s.execute("sheet.bead", &p)
    };
    for named in [false, true] {
        // Sketched in the moved component, at its own x = 50; the bead added at the root.
        let mut s = moved();
        let body = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
        run(&mut s, "component.activate", json!({"component": "P"}));
        along(&mut s, "InP", 50.0);
        run(&mut s, "component.activate", json!({"component": "root"}));
        if let Err(e) = press(&mut s, "InP", named.then_some(body.as_str())) {
            panic!("named {named}, sketched in the component: {e}");
        }
        // Sketched at the root, on the sheet where it lies (world x = 250), and off it (x = 50).
        let mut s = moved();
        along(&mut s, "On", 250.0);
        if let Err(e) = press(&mut s, "On", named.then_some(body.as_str())) {
            panic!("named {named}, sketched at the root: {e}");
        }
        along(&mut s, "Off", 50.0);
        assert!(press(&mut s, "Off", named.then_some(body.as_str())).is_err(), "named {named}: off the sheet");
    }
}

/// Two sheets in one place, one with a hole on the centreline away from its middle: the one that
/// holds the whole centreline takes the bead, and the other is no candidate, so the bead is not
/// refused as lying on two sheets.
#[test]
fn of_two_sheets_the_one_holding_the_whole_centreline_takes_the_bead() {
    let mut s = Session::default();
    rule(&mut s, "Test", T, 0.44, 1.0, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 400, "height": T, "body_name": "A"}));
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "A", "face": [50, 200, T]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": T, "name": "Hole"}));
    run(&mut s, "sketch.circle.center", json!({"center": [50, 120], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Hole", "through_all": true, "direction": "symmetric", "operation": "cut"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 400, "height": T, "body_name": "B", "operation": "new"}));
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "B", "face": [50, 200, T]}));
    line(&mut s, "Beads", T, [50.0, 40.0], [50.0, 360.0]);
    let r = bead(&mut s, "Beads", false);
    assert!(r["changed_bodies"].as_array().is_some_and(|b| b.iter().any(|n| n == "B") && !b.iter().any(|n| n == "A")), "{r}");
    let faces = |name: &str| s.model.state().bodies.iter().find(|b| b.name == name).map_or(0, |b| b.body.face_count());
    assert_eq!(faces("B"), 6 + 26);
}
