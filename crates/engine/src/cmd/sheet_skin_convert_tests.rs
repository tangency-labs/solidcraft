//! Convert to Sheet Metal through the skin (#6748): no face is fixed, every face but the skipped
//! ones converts, sharp edges are cut open until picked as bends, modeled rolls keep their
//! radius, and a ring of bends is refused.

use std::f64::consts::PI;

use solidcraft_doc::{SkinConversion, SkinRule, convert_skin};
use solidcraft_geom::{Region2, Vec3};

use super::*;

/// Converts the body `name` through its skin.
fn skin(s: &Session, name: &str, rule: SkinRule, bends: &[[f64; 3]], skip: &[[f64; 3]], outward: bool) -> solidcraft_doc::Result<SkinConversion> {
    let state = s.model.state();
    let Some(mb) = state.bodies.iter().find(|b| b.name == name) else { panic!("no body {name}") };
    let v = |p: &[f64; 3]| Vec3::new(p[0], p[1], p[2]);
    convert_skin(mb, &rule, &bends.iter().map(v).collect::<Vec<_>>(), &skip.iter().map(v).collect::<Vec<_>>(), outward)
}

/// The flat pattern's size, the shorter side last.
fn size(c: &SkinConversion) -> (f64, f64) {
    let flat: Vec<Region2> = c.flat().unwrap_or_else(|e| panic!("{e}"));
    let mut b = solidcraft_geom::Aabb2::EMPTY;
    for r in &flat {
        b = b.union(&r.outer.polyline(1e-4).bounds());
    }
    let s = b.size();
    (s.x.max(s.y), s.x.min(s.y))
}

/// The flat pattern's area.
fn area(c: &SkinConversion) -> f64 {
    c.flat().unwrap_or_else(|e| panic!("{e}")).iter().map(|r| r.area()).sum()
}

const TEST: SkinRule = SkinRule { thickness: 2.0, k_factor: 0.44, bend_radius: 2.0, gap: 0.5 };

/// A channel whose bends are modeled with the rule's radius: through the skin, its round
/// corners are rolls joined to the flat walls by seams, with no pick at all, and the flat is the
/// one `sheet.convert` makes picking those corners as bends: 30 + 80 + 30 less two deductions.
#[test]
fn a_channel_converts_with_its_corners_as_rolls_to_the_bends_flat() {
    let (t, k, r) = (2.0, 0.4, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    let corners = bent_section(&mut s, &[(0.0, 30.0), (0.0, 0.0), (80.0, 0.0), (80.0, 30.0)], t, r, 60.0);
    let c = skin(&s, "Section", SkinRule { thickness: t, k_factor: k, bend_radius: r, gap: 0.5 }, &[], &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.skin.panels.len(), 5, "the web, its two corners and its two legs");
    assert!(c.thickness == t && c.development.zones.is_empty(), "a shell's own thickness, and rolls rather than bends");
    let across = 140.0 - 2.0 * deduction(std::f64::consts::FRAC_PI_2, r, t, k);
    let got = size(&c);
    assert!(same_size(got, (across, 60.0), 1e-6), "{got:?} vs {across}");
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [65, 0, 10], "bends": [corners[0].0, corners[1].0]}));
    assert!(same_size(flat_size(&mut s), got, 1e-6), "{:?} vs {got:?}", flat_size(&mut s));
}

/// A roll keeps its modeled radius, whatever the rule's: a channel rounded R 10 inside flattens
/// to its walls' flat stretches and two quarter turns at 10 + K x T, where `sheet.convert` would
/// bend it at the rule's R 2.
#[test]
fn a_roll_keeps_its_modeled_radius() {
    let (t, k, r) = (2.0, 0.4, 10.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, 2.0, 0.5);
    bent_section(&mut s, &[(0.0, 30.0), (0.0, 0.0), (80.0, 0.0), (80.0, 30.0)], t, r, 60.0);
    let c =
        skin(&s, "Section", SkinRule { thickness: t, k_factor: k, bend_radius: 2.0, gap: 0.5 }, &[], &[], false).unwrap_or_else(|e| panic!("{e}"));
    let across = 2.0 * (30.0 - (r + t)) + (80.0 - 2.0 * (r + t)) + PI * (r + k * t);
    let got = size(&c);
    assert!(same_size(got, (across, 60.0), 1e-6), "{got:?} vs {across}");
}

/// A shelled tray with no picks is cut open at every sharp edge: five pieces, no bends. Its four
/// floor edges picked as bends fold it to the flat `sheet.convert` makes from the same picks, its
/// corners cut open. A skipped wall is left out.
#[test]
fn a_shell_is_cut_open_until_its_edges_are_picked_as_bends() {
    let mut s = Session::default();
    rule(&mut s, "Test", TEST.thickness, TEST.k_factor, TEST.bend_radius, TEST.gap);
    tray(&mut s, TEST.thickness);
    let open = skin(&s, "Tray", TEST, &[], &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((open.skin.panels.len(), open.development.pieces.len(), open.development.zones.len()), (5, 5, 0), "the floor and four walls, no rims");
    let c = skin(&s, "Tray", TEST, &BOTTOM_EDGES, &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((c.skin.panels.len(), c.development.pieces.len(), c.development.zones.len()), (5, 1, 4));
    let skipped = skin(&s, "Tray", TEST, &BOTTOM_EDGES[1..], &[[50.0, 0.0, 15.0]], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((skipped.skin.panels.len(), skipped.development.zones.len()), (4, 3));
    match skin(&s, "Tray", TEST, &BOTTOM_EDGES, &[[50.0, 0.0, 15.0]], false) {
        Err(e) => assert!(e.to_string().contains("is not between two faces of the sheet"), "{e}"),
        Ok(_) => panic!("a bend onto a skipped face converts"),
    }
    let got = size(&c);
    run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
    assert!(same_size(flat_size(&mut s), got, 1e-6), "{:?} vs {got:?}", flat_size(&mut s));
}

/// A solid converts whole: every face but the skipped top, the rule's thickness inside it, to
/// the shelled tray's flat. Drawn from the inside instead, a box the tray's cavity converted with
/// its material outside is the same metal, and so the same flat, corners and all: each bend ends
/// where the rip planes at its corners cut it, R + T + gap / sqrt 2 short of each end of the edge
/// drawn outside, whichever side the sheet is drawn from.
#[test]
fn a_solid_converts_whole_with_its_material_inside_or_outside() {
    let shelled = {
        let mut s = Session::default();
        tray(&mut s, TEST.thickness);
        skin(&s, "Tray", TEST, &BOTTOM_EDGES, &[], false).unwrap_or_else(|e| panic!("{e}"))
    };
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Block"}));
    let inside = skin(&s, "Block", TEST, &BOTTOM_EDGES, &[[50.0, 30.0, 30.0]], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((inside.skin.panels.len(), inside.development.zones.len(), inside.thickness), (5, 4, TEST.thickness));
    // The same block with its material outside is another tray, T bigger all round, and its flat
    // is 4 T longer each way.
    let bigger = skin(&s, "Block", TEST, &BOTTOM_EDGES, &[[50.0, 30.0, 30.0]], true).unwrap_or_else(|e| panic!("{e}"));
    let grown = |(a, b): (f64, f64)| (a + 4.0 * TEST.thickness, b + 4.0 * TEST.thickness);
    assert!(same_size(size(&bigger), grown(size(&inside)), 1e-6), "{:?} vs {:?}", size(&bigger), size(&inside));
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 96, "width": 56, "height": 28, "corner": [2, 2, 2], "body_name": "Cavity"}));
    let floor = [[50.0, 2.0, 2.0], [98.0, 30.0, 2.0], [50.0, 58.0, 2.0], [2.0, 30.0, 2.0]];
    let outside = skin(&s, "Cavity", TEST, &floor, &[[50.0, 30.0, 30.0]], true).unwrap_or_else(|e| panic!("{e}"));
    let bd = deduction(std::f64::consts::FRAC_PI_2, TEST.bend_radius, TEST.thickness, TEST.k_factor);
    assert!(same_size(size(&inside), (160.0 - 2.0 * bd, 120.0 - 2.0 * bd), 1e-6), "{:?}", size(&inside));
    let short = 2.0 * (TEST.bend_radius + TEST.thickness) + std::f64::consts::SQRT_2 * TEST.gap;
    let want = [60.0 - short, 60.0 - short, 100.0 - short, 100.0 - short];
    for (what, c) in [("shelled", &shelled), ("inside", &inside), ("outside", &outside)] {
        let mut lengths: Vec<f64> = c.development.zones.iter().map(|z| z.near[0].dist(z.near[1])).collect();
        lengths.sort_by(f64::total_cmp);
        assert!(lengths.iter().zip(want).all(|(l, w)| (l - w).abs() < 1e-9), "{what}: {lengths:?} vs {want:?}");
        assert!(same_size(size(c), size(&shelled), 1e-6), "{what}: {:?} vs {:?}", size(c), size(&shelled));
        assert!((area(c) - area(&shelled)).abs() < 1e-6, "{what}: {} vs {}", area(c), area(&shelled));
    }
}

/// A square tube converted with all four of its corners as bends closes a ring: refused, naming
/// the ring's edges. With three, the fourth corner is cut open, and the flat is the one
/// `sheet.convert` makes from the same picks.
#[test]
fn a_ring_of_bends_is_refused_and_opens_where_a_corner_is_left_unpicked() {
    let (t, k, r) = (2.0, 0.44, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 40, "body_name": "Tube"}));
    run(&mut s, "solid.shell", json!({"faces": [[50, 30, 40], [50, 30, 0]], "thickness": t}));
    let corners = [[100.0, 0.0, 20.0], [100.0, 60.0, 20.0], [0.0, 60.0, 20.0], [0.0, 0.0, 20.0]];
    match skin(&s, "Tube", TEST, &corners, &[], false) {
        Err(e) => assert!(e.to_string().contains("closes a ring") && e.to_string().contains("leave one of the ring's edges out"), "{e}"),
        Ok(_) => panic!("a ring of four bends converts"),
    }
    let c = skin(&s, "Tube", TEST, &corners[..3], &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert!(c.development.loops.is_empty() && c.development.zones.len() == 3);
    let got = size(&c);
    // Last, since it makes the body a sheet.
    run(&mut s, "sheet.convert", json!({"body": "Tube", "face": [50, 0, 20], "bends": &corners[..3]}));
    assert!(same_size(flat_size(&mut s), got, 1e-6), "{:?} vs {got:?}", flat_size(&mut s));
}

/// Through the commands, as the app runs them: `sheet.convert` with `skin: true` leaves the body
/// as modeled, and `sheet.flat_pattern` shows the skin's flat, its rolls counted. The flat
/// survives saving and reopening the design.
#[test]
fn a_skin_conversion_shows_in_the_flat_pattern() {
    let (t, k, r) = (2.0, 0.4, 10.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, 2.0, 0.5);
    bent_section(&mut s, &[(0.0, 30.0), (0.0, 0.0), (80.0, 0.0), (80.0, 30.0)], t, r, 60.0);
    let solid = volume(&mut s);
    run(&mut s, "sheet.convert", json!({"body": "Section", "skin": true}));
    assert!(s.model.results.iter().all(|x| x.error.is_none()), "{:?}", s.model.results.iter().map(|x| &x.error).collect::<Vec<_>>());
    assert!((volume(&mut s) - solid).abs() < 1e-9 * solid, "the body stays as modeled");
    let across = 2.0 * (30.0 - (r + t)) + (80.0 - 2.0 * (r + t)) + PI * (r + k * t);
    let flat = run(&mut s, "sheet.flat_pattern", json!({"body": "Section"}));
    let size = |f: &Value| (f["flat_size_mm"][0].as_f64().unwrap_or(0.0), f["flat_size_mm"][1].as_f64().unwrap_or(0.0));
    let (w, h) = size(&flat);
    assert!(same_size((w.max(h), w.min(h)), (across, 60.0), 1e-6), "{flat}");
    assert!(flat["skin"] == json!(true) && flat["rolls"] == json!(2) && flat["bends"] == json!([]), "{flat}");
    let saved = s.doc.to_json();
    assert!(saved.contains("\"sheet_skin_convert\""), "the feature saves as a conversion through the skin");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap_or_else(|e| panic!("{e}")));
    assert_eq!(size(&run(&mut reopened, "sheet.flat_pattern", json!({"body": "Section"}))), (w, h));
}

/// Through the commands: a ring of bends is refused, naming its edge, and with one corner left
/// unpicked the flat lists the other three as bends. A fixed face is refused through the skin,
/// and skipped faces or a material side without it.
#[test]
fn a_ring_through_the_commands_is_refused_and_the_skins_parameters_are_its_own() {
    let (t, k, r) = (2.0, 0.44, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 40, "body_name": "Tube"}));
    run(&mut s, "solid.shell", json!({"faces": [[50, 30, 40], [50, 30, 0]], "thickness": t}));
    let corners = [[100.0, 0.0, 20.0], [100.0, 60.0, 20.0], [0.0, 60.0, 20.0], [0.0, 0.0, 20.0]];
    match s.execute("sheet.convert", &json!({"body": "Tube", "bends": corners, "skin": true})) {
        Err(e) => assert!(e.to_string().contains("closes a ring"), "{e}"),
        Ok(v) => panic!("a ring of four bends converts: {v}"),
    }
    for bad in [
        json!({"body": "Tube", "face": [50, 0, 20], "skin": true}),
        json!({"body": "Tube", "face": [50, 0, 20], "skip": [[50, 0, 20]]}),
        json!({"body": "Tube", "face": [50, 0, 20], "direction": "outside"}),
        json!({"body": "Tube", "skin": true, "direction": "both"}),
    ] {
        assert!(s.execute("sheet.convert", &bad).is_err(), "{bad}");
    }
    run(&mut s, "sheet.convert", json!({"body": "Tube", "bends": &corners[..3], "skin": true}));
    let flat = run(&mut s, "sheet.flat_pattern", json!({"body": "Tube"}));
    let bends = flat["bends"].as_array().cloned().unwrap_or_default();
    assert!(bends.len() == 3 && bends.iter().all(|b| (b["angle_deg"].as_f64().unwrap_or(0.0) - 90.0).abs() < 1e-9), "{flat}");
}
