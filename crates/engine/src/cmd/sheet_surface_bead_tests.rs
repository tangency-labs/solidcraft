//! Formed beads on a sheet made from surfaces (#6748): a bead is a form on a planar panel, kept
//! in the panel's chart, built in the panel's frame and sewn with its faces in the folded solid.

use solidcraft_geom::Vec2;

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

/// The flat pattern lists the bead where its panel lies in the flat (the blank stays flat), and
/// the DXF draws its centreline and footprint on FORM_UP or FORM_DOWN, with a note.
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
    let dxf = run(&mut s, "sheet.export_dxf", json!({"bend_notes": true}))["dxf"].as_str().unwrap_or_default().to_string();
    assert!(dxf.contains("FORM_UP") && !dxf.contains("FORM_DOWN"), "layers");
    assert!(dxf.contains("BEAD UP D2 W10"), "note");
}

/// A bead whose footprint meets the sheet's edge or a bend, one on a sheet drawn from a sketch,
/// and one not on the sheet's faces are refused, each with its reason.
#[test]
fn beads_that_cannot_be_pressed_are_refused() {
    let mut s = plate();
    line(&mut s, "Edge", T, [3.0, 40.0], [3.0, 360.0]);
    let e = s.execute("sheet.bead", &json!({"sketch": "Edge", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("meets the sheet's edge"), "{e}");
    line(&mut s, "Off", T + 5.0, [50.0, 40.0], [50.0, 360.0]);
    let e = s.execute("sheet.bead", &json!({"sketch": "Off", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("planar panel of a sheet made from surfaces"), "{e}");
    // A flange on the plate's front edge: a bead running into its bend.
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    line(&mut s, "Bend", T, [50.0, 1.0], [50.0, 200.0]);
    let e = s.execute("sheet.bead", &json!({"sketch": "Bend", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("meets a bend") || e.contains("meets the sheet's edge"), "{e}");
    // A sheet drawn from a sketch is not made from surfaces.
    let mut f = Session::default();
    rule(&mut f, "Test", T, 0.44, 1.0, 0.5);
    run(&mut f, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
    run(&mut f, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 400]}));
    run(&mut f, "sketch.finish", json!({}));
    run(&mut f, "sheet.flange", json!({"sketch": "Blank"}));
    line(&mut f, "Beads", T, [50.0, 40.0], [50.0, 360.0]);
    let e = f.execute("sheet.bead", &json!({"sketch": "Beads", "curves": ["c1"], "depth": 2, "width": 10})).unwrap_err().to_string();
    assert!(e.contains("planar panel of a sheet made from surfaces"), "{e}");
}
