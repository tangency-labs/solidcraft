//! Convert to Sheet Metal (#6748): no face is fixed, every face but the skipped
//! ones converts, sharp edges are cut open until picked as bends, modeled rolls keep their
//! radius, a ring of bends is refused, and the body becomes its folded sheet.

use std::f64::consts::{FRAC_PI_2, PI, SQRT_2};

use solidcraft_doc::sheet::ReliefShape;
use solidcraft_doc::{ConvertRule, SheetFaces, SurfaceConversion, convert_surfaces};
use solidcraft_geom::{Aabb3, Region2, Seg2, Vec3};

use super::*;

/// Converts the body `name` to sheet metal.
fn convert(
    s: &Session,
    name: &str,
    rule: ConvertRule,
    bends: &[[f64; 3]],
    skip: &[[f64; 3]],
    outward: bool,
) -> solidcraft_doc::Result<SurfaceConversion> {
    let state = s.model.state();
    let Some(mb) = state.bodies.iter().find(|b| b.name == name) else { panic!("no body {name}") };
    let v = |p: &[f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let skip: Vec<Vec3> = skip.iter().map(v).collect();
    convert_surfaces(mb, &fixed(rule), &bends.iter().map(v).collect::<Vec<_>>(), SheetFaces::AllBut(&skip), outward, &solidcraft_doc::IDENTITY)
}

/// The rule's values whatever the sheet's thickness.
fn fixed(rule: ConvertRule) -> impl Fn(Option<f64>) -> solidcraft_doc::Result<ConvertRule> {
    move |_| Ok(rule)
}

/// The flat pattern's size, the shorter side last.
fn size(c: &SurfaceConversion) -> (f64, f64) {
    let flat: Vec<Region2> = c.flat().unwrap_or_else(|e| panic!("{e}"));
    let mut b = solidcraft_geom::Aabb2::EMPTY;
    for r in &flat {
        b = b.union(&r.outer.polyline(1e-4).bounds());
    }
    let s = b.size();
    (s.x.max(s.y), s.x.min(s.y))
}

/// The flat pattern's area.
fn area(c: &SurfaceConversion) -> f64 {
    c.flat().unwrap_or_else(|e| panic!("{e}")).iter().map(|r| r.area()).sum()
}

/// A body's volume, integrated on its exact faces (to `solidcraft_smlib::MEASURE_ACCURACY`), so
/// however its faces happen to be split it measures alike.
fn fine_volume(b: &solidcraft_kernel::Body) -> f64 {
    solidcraft_kernel::measure_mass(b).map_or(f64::NAN, |m| m.volume)
}

/// The folded sheet's volume and box.
fn folded(c: &SurfaceConversion) -> (f64, Aabb3) {
    let b = c.solid().unwrap_or_else(|e| panic!("{e}"));
    (fine_volume(&b), b.outline_bounds())
}

const TEST: ConvertRule = ConvertRule {
    thickness: 2.0,
    k_factor: 0.44,
    bend_radius: 2.0,
    gap: 0.5,
    corner_relief: ReliefShape::Notch,
    corner_relief_size: 8.0,
    relief_width: 2.0,
    relief_depth: 1.0,
};

/// A channel whose bends are modeled with the rule's radius: converted, its round
/// corners are rolls joined to the flat walls by seams, with no pick at all, and the flat is the
/// one `sheet.convert` makes picking those corners as bends: 30 + 80 + 30 less two deductions.
#[test]
fn a_channel_converts_with_its_corners_as_rolls_to_the_bends_flat() {
    let (t, k, r) = (2.0, 0.4, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    let corners = bent_section(&mut s, &[(0.0, 30.0), (0.0, 0.0), (80.0, 0.0), (80.0, 30.0)], t, r, 60.0);
    let c =
        convert(&s, "Section", ConvertRule { thickness: t, k_factor: k, bend_radius: r, ..TEST }, &[], &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.sheet.panels.len(), 5, "the web, its two corners and its two legs");
    assert!(c.thickness == t && c.development.zones.is_empty(), "a shell's own thickness, and rolls rather than bends");
    let across = 140.0 - 2.0 * deduction(std::f64::consts::FRAC_PI_2, r, t, k);
    let got = size(&c);
    assert!(same_size(got, (across, 60.0), 1e-6), "{got:?} vs {across}");
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Section", "face": [65, 0, 10], "bends": [corners[0].0, corners[1].0]}));
    assert!(same_size(flat_size(&mut s), got, 1e-6), "{:?} vs {got:?}", flat_size(&mut s));
    // Folded, its rolls are the bends today's Convert makes of them.
    let (rolled, _) = folded(&c);
    let today = s.model.state().bodies.iter().find(|b| b.name == "Section").map_or(f64::NAN, |b| fine_volume(&b.body));
    assert!((rolled - today).abs() < 1e-9 * today, "{rolled} vs {today}");
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
    let c = convert(&s, "Section", ConvertRule { thickness: t, k_factor: k, bend_radius: 2.0, ..TEST }, &[], &[], false)
        .unwrap_or_else(|e| panic!("{e}"));
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
    let open = convert(&s, "Tray", TEST, &[], &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        (open.sheet.panels.len(), open.development.pieces.len(), open.development.zones.len()),
        (5, 5, 0),
        "the floor and four walls, no rims"
    );
    let c = convert(&s, "Tray", TEST, &BOTTOM_EDGES, &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((c.sheet.panels.len(), c.development.pieces.len(), c.development.zones.len()), (5, 1, 4));
    let skipped = convert(&s, "Tray", TEST, &BOTTOM_EDGES[1..], &[[50.0, 0.0, 15.0]], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((skipped.sheet.panels.len(), skipped.development.zones.len()), (4, 3));
    match convert(&s, "Tray", TEST, &BOTTOM_EDGES, &[[50.0, 0.0, 15.0]], false) {
        Err(e) => assert!(e.to_string().contains("is not between two faces of the sheet"), "{e}"),
        Ok(_) => panic!("a bend onto a skipped face converts"),
    }
    let got = size(&c);
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
    assert!(same_size(flat_size(&mut s), got, 1e-6), "{:?} vs {got:?}", flat_size(&mut s));
}

/// A solid converts whole: every face but the skipped top, the rule's thickness inside it, to
/// the shelled tray's flat. Drawn from the inside instead, a box the tray's cavity converted with
/// its material outside is the same metal, and so the same flat, corners and all: each bend ends
/// at the base's corner, where the bends' tangent lines cross, R + T short of each end of the
/// edge drawn outside and R short of each end of the one drawn inside.
#[test]
fn a_solid_converts_whole_with_its_material_inside_or_outside() {
    let shelled = {
        let mut s = Session::default();
        tray(&mut s, TEST.thickness);
        convert(&s, "Tray", TEST, &BOTTOM_EDGES, &[], false).unwrap_or_else(|e| panic!("{e}"))
    };
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Block"}));
    let inside = convert(&s, "Block", TEST, &BOTTOM_EDGES, &[[50.0, 30.0, 30.0]], false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((inside.sheet.panels.len(), inside.development.zones.len(), inside.thickness), (5, 4, TEST.thickness));
    // The same block with its material outside is another tray, T bigger all round, and its flat
    // is 4 T longer each way.
    let bigger = convert(&s, "Block", TEST, &BOTTOM_EDGES, &[[50.0, 30.0, 30.0]], true).unwrap_or_else(|e| panic!("{e}"));
    let grown = |(a, b): (f64, f64)| (a + 4.0 * TEST.thickness, b + 4.0 * TEST.thickness);
    assert!(same_size(size(&bigger), grown(size(&inside)), 1e-6), "{:?} vs {:?}", size(&bigger), size(&inside));
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 96, "width": 56, "height": 28, "corner": [2, 2, 2], "body_name": "Cavity"}));
    let floor = [[50.0, 2.0, 2.0], [98.0, 30.0, 2.0], [50.0, 58.0, 2.0], [2.0, 30.0, 2.0]];
    let outside = convert(&s, "Cavity", TEST, &floor, &[[50.0, 30.0, 30.0]], true).unwrap_or_else(|e| panic!("{e}"));
    let bd = deduction(std::f64::consts::FRAC_PI_2, TEST.bend_radius, TEST.thickness, TEST.k_factor);
    assert!(same_size(size(&inside), (160.0 - 2.0 * bd, 120.0 - 2.0 * bd), 1e-6), "{:?}", size(&inside));
    let short = 2.0 * (TEST.bend_radius + TEST.thickness);
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
    match convert(&s, "Tube", TEST, &corners, &[], false) {
        Err(e) => assert!(e.to_string().contains("closes a ring") && e.to_string().contains("leave one of the ring's edges out"), "{e}"),
        Ok(_) => panic!("a ring of four bends converts"),
    }
    let c = convert(&s, "Tube", TEST, &corners[..3], &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert!(c.development.loops.is_empty() && c.development.zones.len() == 3);
    // Folded: four walls 40 tall between three full-height bends, the walls at the open corner
    // cut back from it as a rip cuts them.
    let (sb, cut) = (r + t, t + 0.5 / SQRT_2);
    let walls = (100.0 - cut - sb) + (60.0 - 2.0 * sb) + (100.0 - 2.0 * sb) + (60.0 - sb - cut);
    let want = t * 40.0 * (walls + 3.0 * FRAC_PI_2 * (r + t / 2.0));
    let (v, _) = folded(&c);
    assert!((v - want).abs() < 1e-5 * want, "{v} vs {want}");
    let got = size(&c);
    // Last, since it makes the body a sheet.
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Tube", "face": [50, 0, 20], "bends": &corners[..3]}));
    assert!(same_size(flat_size(&mut s), got, 1e-6), "{:?} vs {got:?}", flat_size(&mut s));
    // Its bends end where its walls do, so today's Convert folds it to the same metal.
    let today = s.model.state().bodies.iter().find(|b| b.name == "Tube").map_or(f64::NAN, |b| fine_volume(&b.body));
    assert!((v - today).abs() < 1e-9 * today, "{v} vs {today}");
}

/// Through the commands, as the app runs them: `sheet.convert` makes the body
/// its folded sheet, which this channel already is, rolls and all, so it keeps its volume; and
/// `sheet.flat_pattern` shows its flat, its rolls counted. The flat survives saving and
/// reopening the design.
#[test]
fn a_conversion_shows_in_the_flat_pattern() {
    let (t, k, r) = (2.0, 0.4, 10.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, 2.0, 0.5);
    bent_section(&mut s, &[(0.0, 30.0), (0.0, 0.0), (80.0, 0.0), (80.0, 30.0)], t, r, 60.0);
    let section = |s: &Session| s.model.state().bodies.iter().find(|b| b.name == "Section").map_or(f64::NAN, |b| fine_volume(&b.body));
    let modeled = section(&s);
    run(&mut s, "sheet.convert", json!({"body": "Section"}));
    assert!(s.model.results.iter().all(|x| x.error.is_none()), "{:?}", s.model.results.iter().map(|x| &x.error).collect::<Vec<_>>());
    let sheet = section(&s);
    assert!((sheet - modeled).abs() < 1e-6 * modeled, "the folded sheet is the channel: {sheet} vs {modeled}");
    let across = 2.0 * (30.0 - (r + t)) + (80.0 - 2.0 * (r + t)) + PI * (r + k * t);
    let flat = run(&mut s, "sheet.flat_pattern", json!({"body": "Section"}));
    let size = |f: &Value| (f["flat_size_mm"][0].as_f64().unwrap_or(0.0), f["flat_size_mm"][1].as_f64().unwrap_or(0.0));
    let (w, h) = size(&flat);
    assert!(same_size((w.max(h), w.min(h)), (across, 60.0), 1e-6), "{flat}");
    assert!(flat["surfaces"] == json!(true) && flat["rolls"] == json!(2) && flat["bends"] == json!([]), "{flat}");
    let saved = s.doc.to_json();
    assert!(saved.contains("\"sheet_surface_convert\""), "the feature saves as a conversion");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap_or_else(|e| panic!("{e}")));
    assert_eq!(size(&run(&mut reopened, "sheet.flat_pattern", json!({"body": "Section"}))), (w, h));
}

/// Through the commands: a ring of bends is refused, naming its edge, and with one corner left
/// unpicked the flat lists the other three as bends. A face is refused by Convert, and skipped
/// faces by Thicken.
#[test]
fn a_ring_through_the_commands_is_refused_and_each_mode_takes_its_own_parameters() {
    let (t, k, r) = (2.0, 0.44, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 40, "body_name": "Tube"}));
    run(&mut s, "solid.shell", json!({"faces": [[50, 30, 40], [50, 30, 0]], "thickness": t}));
    let corners = [[100.0, 0.0, 20.0], [100.0, 60.0, 20.0], [0.0, 60.0, 20.0], [0.0, 0.0, 20.0]];
    match s.execute("sheet.convert", &json!({"body": "Tube", "bends": corners})) {
        Err(e) => assert!(e.to_string().contains("closes a ring"), "{e}"),
        Ok(v) => panic!("a ring of four bends converts: {v}"),
    }
    for bad in [
        json!({"body": "Tube", "face": [50, 0, 20]}),
        json!({"body": "Tube", "mode": "thicken", "face": [50, 0, 20], "skip": [[50, 0, 20]]}),
        json!({"body": "Tube", "mode": "thicken"}),
        json!({"body": "Tube", "mode": "thicken", "face": [50, 0, 20], "direction": "both"}),
        json!({"body": "Tube", "direction": "both"}),
        json!({"body": "Tube", "mode": "fold"}),
    ] {
        assert!(s.execute("sheet.convert", &bad).is_err(), "{bad}");
    }
    run(&mut s, "sheet.convert", json!({"body": "Tube", "bends": &corners[..3]}));
    let flat = run(&mut s, "sheet.flat_pattern", json!({"body": "Tube"}));
    let bends = flat["bends"].as_array().cloned().unwrap_or_default();
    assert!(bends.len() == 3 && bends.iter().all(|b| (b["angle_deg"].as_f64().unwrap_or(0.0) - 90.0).abs() < 1e-9), "{flat}");
}

/// The shapes of corner relief a rule can ask for.
const SHAPES: [(&str, ReliefShape); 4] =
    [("notch", ReliefShape::Notch), ("round", ReliefShape::Round), ("rectangular", ReliefShape::Rectangular), ("tear", ReliefShape::Tear)];

/// Today's Convert of a box `size` big at `corner`, shelled 2 thick with its top open, its floor
/// edges bent under the test rule with a corner relief of `shape` 8 across: the folded sheet's
/// volume.
fn todays_tray(shape: &str, corner: [f64; 3], size: [f64; 3]) -> f64 {
    let mut s = Session::default();
    relief_rule(&mut s, shape);
    let ([x, y, z], [l, w, h]) = (corner, size);
    run(&mut s, "solid.box", json!({"length": l, "width": w, "height": h, "corner": corner, "body_name": "Tray"}));
    run(&mut s, "solid.shell", json!({"faces": [[x + l / 2.0, y + w / 2.0, z + h]], "thickness": 2.0}));
    let floor = [[x + l / 2.0, y, z], [x + l, y + w / 2.0, z], [x + l / 2.0, y + w, z], [x, y + w / 2.0, z]];
    let out = run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Tray", "face": [x + l / 2.0, y + w / 2.0, z], "bends": floor}));
    assert!(out["warning"].is_null(), "{shape}: {out}");
    s.model.state().bodies.iter().find(|b| b.name == "Tray").map_or(f64::NAN, |b| fine_volume(&b.body))
}

/// The flat of the tray a box l x w x h drawn outside makes under the test rule with notched
/// corners: its floor between the bends' tangent lines, R + T in from the sharps; its bends BA
/// wide, meeting at the floor's corners; its walls cut back g/√2 + T from each corner.
fn notched_tray_flat(l: f64, w: f64, h: f64) -> f64 {
    let (t, r, k, gap) = (TEST.thickness, TEST.bend_radius, TEST.k_factor, TEST.gap);
    let (s, ba, cut) = (r + t, FRAC_PI_2 * (r + k * t), t + gap / SQRT_2);
    (l - 2.0 * s) * (w - 2.0 * s) + ba * 2.0 * ((l - 2.0 * s) + (w - 2.0 * s)) + (h - s) * 2.0 * ((l - 2.0 * cut) + (w - 2.0 * cut))
}

/// The centers of the flat's round reliefs of radius `r`: its arcs of that radius, one center
/// for each.
fn relief_centers(c: &SurfaceConversion, r: f64) -> Vec<solidcraft_geom::Vec2> {
    let mut centers: Vec<solidcraft_geom::Vec2> = Vec::new();
    for region in c.flat().unwrap_or_else(|e| panic!("{e}")) {
        for seg in std::iter::once(&region.outer).chain(&region.holes).flat_map(|l| &l.segs) {
            if let solidcraft_geom::Seg2::Arc { center, radius, .. } = *seg
                && (radius - r).abs() < 1e-6
                && !centers.iter().any(|q| q.dist(center) < 1e-6)
            {
                centers.push(center);
            }
        }
    }
    centers
}

/// Converted, a body becomes one sheet however it was drawn: the shelled
/// tray, the block drawn from outside and its cavity drawn from inside with the material out
/// fold to one solid, whatever the rule's corner relief, and the block with its material outside
/// to the tray a box T bigger all round makes. Notched, that is the solid today's Convert makes,
/// and the flanged tray's flat.
#[test]
fn a_body_folds_into_one_sheet_however_it_is_drawn() {
    let top = [[50.0, 30.0, 30.0]];
    let cavity_floor = [[50.0, 2.0, 2.0], [98.0, 30.0, 2.0], [50.0, 58.0, 2.0], [2.0, 30.0, 2.0]];
    let mut shelled = Session::default();
    tray(&mut shelled, TEST.thickness);
    let mut block = Session::default();
    run(&mut block, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Block"}));
    let mut cavity = Session::default();
    run(&mut cavity, "solid.box", json!({"length": 96, "width": 56, "height": 28, "corner": [2, 2, 2], "body_name": "Cavity"}));
    for (name, shape) in SHAPES {
        let rule = ConvertRule { corner_relief: shape, ..TEST };
        let drawn = [
            ("shelled", convert(&shelled, "Tray", rule, &BOTTOM_EDGES, &[], false)),
            ("inside", convert(&block, "Block", rule, &BOTTOM_EDGES, &top, false)),
            ("outside", convert(&cavity, "Cavity", rule, &cavity_floor, &top, true)),
        ];
        let mut one: Option<(f64, f64)> = None;
        for (what, c) in drawn {
            let c = c.unwrap_or_else(|e| panic!("{name} {what}: {e}"));
            assert!(c.notes.is_empty(), "{name} {what}: {:?}", c.notes);
            let (v, a) = (folded(&c).0, area(&c));
            let (v0, a0) = *one.get_or_insert((v, a));
            // A torn end folds to chains within 1e-7 mm of its curve, each drawing's a little
            // differently.
            let within = if shape == ReliefShape::Tear { 1e-8 } else { 1e-9 };
            assert!((v - v0).abs() < within * v0 && (a - a0).abs() < 1e-6 * a0, "{name} {what}: {v} {a} vs {v0} {a0}");
        }
        let bigger = convert(&block, "Block", rule, &BOTTOM_EDGES, &top, true).unwrap_or_else(|e| panic!("{name} bigger: {e}"));
        if shape == ReliefShape::Notch {
            for (what, c, volume, flat) in [
                (
                    "shelled",
                    convert(&shelled, "Tray", rule, &BOTTOM_EDGES, &[], false),
                    todays_tray(name, [0.0; 3], [100.0, 60.0, 30.0]),
                    notched_tray_flat(100.0, 60.0, 30.0),
                ),
                ("bigger", Ok(bigger), todays_tray(name, [-2.0; 3], [104.0, 64.0, 32.0]), notched_tray_flat(104.0, 64.0, 32.0)),
            ] {
                let c = c.unwrap_or_else(|e| panic!("{what}: {e}"));
                let (v, _) = folded(&c);
                assert!((v - volume).abs() < 1e-9 * volume, "{what}: folded {v} vs today's {volume}");
                assert!((area(&c) - flat).abs() < 1e-6 * flat, "{what}: flat {} vs {flat}", area(&c));
            }
        }
    }
}

/// A corner relief belongs to the corner, not to any sheet round it: in a tray whose walls are
/// larger than its floor, as in a shallow one, each of the four corners gets a round relief
/// centered where its two bends' center lines cross, BA / 2 out from the floor's tangent lines
/// both ways, so their centers stand 92 + BA by 52 + BA apart.
#[test]
fn a_corner_relief_belongs_to_the_corner_whichever_sheet_is_largest() {
    let ba = FRAC_PI_2 * (TEST.bend_radius + TEST.k_factor * TEST.thickness);
    let rule = ConvertRule { corner_relief: ReliefShape::Round, ..TEST };
    for height in [30.0, 120.0] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": height, "body_name": "Tray"}));
        run(&mut s, "solid.shell", json!({"faces": [[50, 30, height]], "thickness": TEST.thickness}));
        let c = convert(&s, "Tray", rule, &BOTTOM_EDGES, &[], false).unwrap_or_else(|e| panic!("{height}: {e}"));
        assert!(c.notes.is_empty() && c.sheet.corners.len() == 4, "{height}: {:?} {:?}", c.notes, c.sheet.corners);
        let centers = relief_centers(&c, rule.corner_relief_size / 2.0);
        assert_eq!(centers.len(), 4, "{height}: {centers:?}");
        let mut b = solidcraft_geom::Aabb2::EMPTY;
        for p in &centers {
            b = b.union(&solidcraft_geom::Aabb2 { min: *p, max: *p });
        }
        let got = (b.size().x.max(b.size().y), b.size().x.min(b.size().y));
        assert!(same_size(got, (92.0 + ba, 52.0 + ba), 1e-6), "{height}: {got:?}");
        folded(&c);
    }
}

/// A plate with a wall along its whole edge y = 0 and one on its edge x = 100 from y = 0 to 30
/// only: the shorter wall bends only where it stands, and where it ends beside more of the
/// plate the plate gets the rule's bend relief. Notched where the walls meet, the bends are 96
/// and 26 long, the flat is a flanged sheet's, and the folded sheet holds the metal that flat
/// folds to.
#[test]
fn a_shorter_wall_bends_where_it_stands_beside_its_bend_relief() {
    let (t, r, k) = (2.0, 2.0, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let mut s = Session::default();
    relief_rule(&mut s, "notch");
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 2, "body_name": "Part"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 2, "height": 30, "operation": "join", "targets": ["Part"]}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 30, "height": 30, "corner": [98, 0, 0], "operation": "join", "targets": ["Part"]}));
    let bends = [[50.0, 0.0, 0.0], [100.0, 15.0, 0.0]];
    let c = convert(&s, "Part", TEST, &bends, &[], false).unwrap_or_else(|e| panic!("{e}"));
    assert!(c.notes.is_empty(), "{:?}", c.notes);
    let mut lengths: Vec<f64> = c.development.zones.iter().map(|z| z.near[0].dist(z.near[1])).collect();
    lengths.sort_by(f64::total_cmp);
    assert!(lengths.len() == 2 && (lengths[0] - 26.0).abs() < 1e-9 && (lengths[1] - 96.0).abs() < 1e-9, "{lengths:?}");
    // The plate less the setbacks and the bend relief (2 by 5), the walls 26 high from the
    // corner's closed end, and the bends.
    let cut = t + 0.5 / SQRT_2;
    let flat = 100.0 * 56.0 - 4.0 * 26.0 - 10.0 + 26.0 * ((100.0 - cut) + (30.0 - cut)) + ba * 122.0;
    assert!((area(&c) - flat).abs() < 1e-6 * flat, "{} vs {flat}", area(&c));
    // A sheet's metal is its thickness times its middle surface's area. The flat is drawn at the
    // neutral surface, so each bend's middle runs t (1/2 - k) wider than its zone per radian.
    let (v, _) = folded(&c);
    let want = t * (flat + FRAC_PI_2 * t * (0.5 - k) * 122.0);
    assert!((v - want).abs() < 1e-8 * want, "{v} vs {want}");
}

/// An icosahedron, edge 50, as a solid body.
fn icosahedron() -> (solidcraft_doc::ModelBody, Vec<Vec3>, Vec<[usize; 3]>) {
    use solidcraft_kernel::{EdgeSpec, FaceSpec, SurfSpec};
    let g = (1.0 + 5f64.sqrt()) / 2.0;
    let mut v = Vec::new();
    for a in [-1.0, 1.0] {
        for b in [-g, g] {
            v.extend([Vec3::new(0.0, a, b), Vec3::new(a, b, 0.0), Vec3::new(b, 0.0, a)]);
        }
    }
    let v: Vec<Vec3> = v.into_iter().map(|p| p * 25.0).collect();
    let close = |a: Vec3, b: Vec3| (a.dist(b) - 50.0).abs() < 1e-6;
    let mut faces = Vec::new();
    for i in 0..12 {
        for j in i + 1..12 {
            for k in j + 1..12 {
                if close(v[i], v[j]) && close(v[j], v[k]) && close(v[i], v[k]) {
                    let n = (v[j] - v[i]).cross(v[k] - v[i]);
                    faces.push(if n.dot(v[i] + v[j] + v[k]) > 0.0 { [i, j, k] } else { [i, k, j] });
                }
            }
        }
    }
    assert_eq!(faces.len(), 20);
    let specs: Vec<FaceSpec> = faces
        .iter()
        .map(|f| FaceSpec {
            loops: vec![(0..3).map(|e| EdgeSpec::Line { a: v[f[e]], b: v[f[(e + 1) % 3]] }).collect()],
            surface: SurfSpec::Plane,
            probe: None,
        })
        .collect();
    let body = solidcraft_kernel::sew(&specs).unwrap_or_else(|e| panic!("{e}"));
    (solidcraft_doc::ModelBody::new("Ico".into(), body, 0), v, faces)
}

/// An icosahedron converted whole, its faces bent along a spanning tree of them and every other
/// edge cut open. A triangle's sides slant across its bends' tangent lines, and its bends still
/// run their whole edges, to the corners: where two of them meet on a face a round relief is cut,
/// ten times, and the sheet folds.
#[test]
fn an_icosahedron_bends_whole_edges_to_its_corners() {
    let (mb, v, faces) = icosahedron();
    let mut seen = vec![false; faces.len()];
    seen[0] = true;
    let mut queue = std::collections::VecDeque::from([0usize]);
    let mut picks = Vec::new();
    while let Some(f) = queue.pop_front() {
        for (g, other) in faces.iter().enumerate() {
            let shared: Vec<usize> = faces[f].iter().copied().filter(|p| other.contains(p)).collect();
            if !seen[g] && shared.len() == 2 {
                seen[g] = true;
                queue.push_back(g);
                picks.push((v[shared[0]] + v[shared[1]]) * 0.5);
            }
        }
    }
    let c = convert_surfaces(
        &mb,
        &fixed(ConvertRule { corner_relief: ReliefShape::Round, ..TEST }),
        &picks,
        SheetFaces::AllBut(&[]),
        false,
        &solidcraft_doc::IDENTITY,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(c.notes.is_empty(), "{:?}", c.notes);
    assert_eq!((c.sheet.panels.len(), c.development.zones.len(), c.sheet.corners.len()), (20, 19, 10));
    let (v, _) = folded(&c);
    assert!(v.is_finite() && v > 0.0, "{v}");
}

/// A tombstone as a solid: a plate `r` either side of x = 0 from `y0` up to y = 23, topped by a
/// half round of radius `r` about (0, 23), standing from `z0` to `z1`.
fn tombstone_body(name: &str, r: f64, y0: f64, z0: f64, z1: f64) -> solidcraft_doc::ModelBody {
    use solidcraft_kernel::{EdgeSpec, FaceSpec, SurfSpec};
    let at = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    // The outline counter-clockwise from above: the bottom, the right side, the half round, the
    // left side.
    let outline = |z: f64| {
        vec![
            EdgeSpec::Line { a: at(-r, y0, z), b: at(r, y0, z) },
            EdgeSpec::Line { a: at(r, y0, z), b: at(r, 23.0, z) },
            EdgeSpec::Arc { a: at(r, 23.0, z), b: at(-r, 23.0, z), mid: at(0.0, 23.0 + r, z) },
            EdgeSpec::Line { a: at(-r, 23.0, z), b: at(-r, y0, z) },
        ]
    };
    let (bottom, top) = (outline(z0), outline(z1));
    let mut faces = vec![
        FaceSpec { loops: vec![top.clone()], surface: SurfSpec::Plane, probe: None },
        FaceSpec { loops: vec![bottom.iter().rev().map(EdgeSpec::reversed).collect()], surface: SurfSpec::Plane, probe: None },
    ];
    for (low, high) in bottom.iter().zip(&top) {
        let ((a, b), (c, d)) = (ends(low), ends(high));
        let surface = match low {
            EdgeSpec::Arc { a, b, mid } => SurfSpec::Extruded { a: *a, b: *b, mid: *mid, dir: Vec3::new(0.0, 0.0, z1 - z0) },
            _ => SurfSpec::Plane,
        };
        let probe = matches!(low, EdgeSpec::Arc { .. }).then(|| (at(0.0, 23.0 + r, (z0 + z1) / 2.0), Vec3::Y));
        faces.push(FaceSpec {
            loops: vec![vec![low.clone(), EdgeSpec::Line { a: b, b: d }, high.reversed(), EdgeSpec::Line { a: c, b: a }]],
            surface,
            probe,
        });
    }
    let body = solidcraft_kernel::sew(&faces).unwrap_or_else(|e| panic!("{e}"));
    solidcraft_doc::ModelBody::new(name.into(), body, 0)
}

fn ends(e: &solidcraft_kernel::EdgeSpec) -> (Vec3, Vec3) {
    match e {
        solidcraft_kernel::EdgeSpec::Line { a, b } | solidcraft_kernel::EdgeSpec::Arc { a, b, .. } => (*a, *b),
        solidcraft_kernel::EdgeSpec::Chain { ctrl, .. } => (ctrl.first().copied().unwrap_or_default(), ctrl.last().copied().unwrap_or_default()),
    }
}

/// A tombstone 55 wide, 46 to the top of its sides and topped by a half round, 36 deep, converted
/// whole with 2.5 mm steel inside, bent along two edges of its base. Its rips run every way: along
/// the half round's rims, where the plates meet the roll, as well as straight. Each panel is cut
/// back along each rip as the rip runs, so the base and the lid keep their half rounds, cut back as
/// arcs: the flat is the base with the walls round it, and the lid. The same metal drawn from inside, its cavity
/// converted with the material outside, unfolds and folds alike.
#[test]
fn a_tombstone_rips_along_its_round_as_along_its_sides() {
    let rule = ConvertRule {
        thickness: 2.5,
        k_factor: 0.44,
        bend_radius: 2.5,
        gap: 2.5,
        corner_relief: ReliefShape::Notch,
        corner_relief_size: 10.0,
        relief_width: 0.1,
        relief_depth: 1.25,
    };
    let outside = tombstone_body("Outside", 27.5, -23.0, 0.0, 36.0);
    let inside = tombstone_body("Inside", 25.0, -20.5, 2.5, 33.5);
    let drawn = [
        (
            "outside",
            convert_surfaces(
                &outside,
                &fixed(rule),
                &[Vec3::new(0.0, -23.0, 0.0), Vec3::new(27.5, 0.0, 0.0)],
                SheetFaces::AllBut(&[]),
                false,
                &solidcraft_doc::IDENTITY,
            ),
        ),
        (
            "inside",
            convert_surfaces(
                &inside,
                &fixed(rule),
                &[Vec3::new(0.0, -20.5, 2.5), Vec3::new(25.0, 0.0, 2.5)],
                SheetFaces::AllBut(&[]),
                true,
                &solidcraft_doc::IDENTITY,
            ),
        ),
    ];
    let mut one: Option<(f64, f64)> = None;
    for (what, c) in drawn {
        let c = c.unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(c.notes.is_empty(), "{what}: {:?}", c.notes);
        let regions = c.flat().unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(regions.len(), 2, "{what}: the base with its walls, and the lid");
        let free = regions.iter().flat_map(|r| r.outer.segs.iter().chain(r.holes.iter().flat_map(|h| &h.segs))).filter(|s| !s.is_line_or_arc());
        assert_eq!(free.count(), 0, "{what}: the half rounds' rims cut back as the arcs they are");
        // No edge in pieces: on no panel does the outline run straight on from an edge into more
        // of the same edge, as where a rim's cut once carried on along a side's. (Two edges that
        // only line up, as the walls' tops either side of a seam, are two.)
        for flat in c.development.panels.iter().flatten() {
            for (lp, from) in std::iter::once(&flat.region.outer).chain(&flat.region.holes).zip(&flat.origins) {
                let n = lp.segs.len();
                for (k, (p, o)) in lp.segs.iter().zip(from).enumerate() {
                    let (Some(q), Some(next)) = (lp.segs.get((k + 1) % n), from.get((k + 1) % n)) else { continue };
                    if let (Seg2::Line { a, b }, Seg2::Line { b: c, .. }) = (p, q)
                        && o == next
                    {
                        let bent = (*b - *a).normalized().zip((*c - *b).normalized()).is_none_or(|(u, v)| u.cross(v).abs() > 1e-9 || u.dot(v) < 0.0);
                        assert!(bent, "{what}: {o:?} in pieces at {b:?}");
                    }
                }
            }
        }
        let (v, a) = (folded(&c).0, area(&c));
        let (v0, a0) = *one.get_or_insert((v, a));
        assert!((v - v0).abs() < 1e-8 * v0 && (a - a0).abs() < 1e-9 * a0, "{what}: {v} {a} vs {v0} {a0}");
    }
}

/// A rolled surface needs no flat face, and nothing about it is primary. A half pipe of radius
/// 20, 30 long, drawn as a surface, rolls out to its neutral width; a whole pipe, smooth all
/// round, is ripped open at one seam (where its walk closes) and rolls out less the rip's gap,
/// cut on the roll.
#[test]
fn a_pipe_rolls_out_from_its_surface_alone() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Round"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Round", "distance": "30 mm", "name": "Pipe", "body_name": "Pipe"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
    let arc = run(&mut s, "sketch.arc.center_point", json!({"center": [100, 0], "start": [120, 0], "sweep": 180}));
    let curve = arc["created"]["curves"][0].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Arc", "curves": [curve], "distance": "30 mm", "name": "Half", "body_name": "Half"}));
    let neutral = 20.0 - (1.0 - TEST.k_factor) * TEST.thickness;
    for (name, width) in [("Half", PI * neutral), ("Pipe", 2.0 * PI * neutral - TEST.gap * neutral / 20.0)] {
        let c = convert(&s, name, TEST, &[], &[], false).unwrap_or_else(|e| panic!("{name}: {e}"));
        let flat = c.flat().unwrap_or_else(|e| panic!("{name}: {e}"));
        let [one] = flat.as_slice() else { panic!("{name}: one piece: {flat:?}") };
        assert!((one.area() - width * 30.0).abs() < 1e-6 * width * 30.0, "{name}: {} vs {}", one.area(), width * 30.0);
        let (v, _) = folded(&c);
        let outer = 30.0 * width / neutral * 20.0;
        assert!(v > 0.9 * outer * TEST.thickness && v < outer * TEST.thickness, "{name}: folded {v}");
    }
}

/// The folded body's exact volume.
fn body_volume(s: &Session, name: &str) -> f64 {
    s.model.state().bodies.iter().find(|b| b.name == name).map_or(f64::NAN, |b| fine_volume(&b.body))
}

/// A plate 100 x 60 x `t` thickened from its top under the test rule.
fn thickened_plate(t: f64) -> Session {
    let mut s = Session::default();
    rule(&mut s, "Test", t, TEST.k_factor, TEST.bend_radius, TEST.gap);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": t, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Plate", "face": [50, 30, t]}));
    s
}

/// An edge flange on a sheet made from surfaces is a new panel bent from a free edge (#6748): on
/// a plate 100 x 60 x 2 thickened from its top, a flange 20 high from its outer sharp at 90° on
/// the front edge rises away from the material, up, its outside flush with the plate's front face.
/// Flat: 60 + 20 less one deduction, 100 wide, one bend 100 long; folded, t (flat + θ t (1/2 − k)
/// L) by the neutral surface's length through the bend.
#[test]
fn an_edge_flange_bends_from_a_free_edge_of_a_thickened_plate() {
    let (t, k, r) = (TEST.thickness, TEST.k_factor, TEST.bend_radius);
    let mut s = thickened_plate(t);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, t]], "height": 20}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 1 && rows[0].0 == 90 && (rows[0].2 - 100.0).abs() < 1e-6, "{rows:?}");
    let across = 60.0 + 20.0 - deduction(FRAC_PI_2, r, t, k);
    assert!(same_size(flat_size(&mut s), (100.0, across), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    let (lo, hi) = bbox(&mut s);
    // The outer sharp is the plate's bottom front edge: the flange's top is 20 above it.
    let want = ([0.0, 0.0, 0.0], [100.0, 60.0, 20.0]);
    assert!(lo.iter().zip(want.0).chain(hi.iter().zip(want.1)).all(|(a, b)| (a - b).abs() < 1e-6), "{lo:?} {hi:?}");
    let expect = t * (100.0 * across + FRAC_PI_2 * t * (0.5 - k) * 100.0);
    let v = body_volume(&s, "Plate");
    assert!((v - expect).abs() < 1e-6 * expect, "{v} vs {expect}");
    // Unfolded it is the flat, one sheet thick; refolded, the flange again.
    run(&mut s, "sheet.unfold", json!({}));
    assert!((body_volume(&s, "Plate") - t * 100.0 * across).abs() < 1e-6 * expect, "{}", body_volume(&s, "Plate"));
    run(&mut s, "sheet.refold", json!({}));
    assert!((body_volume(&s, "Plate") - v).abs() < 1e-9 * v);
    // The bend line is no free edge.
    assert!(s.execute("sheet.flange", &json!({"edges": [[50, 0, t]], "height": 20})).is_err());
}

/// A hem on a sheet made from surfaces (#6748): picked on the plate's top, it turns half a turn
/// at the front edge back under the plate, its leg the gap 0.5 below it (so the bend's inside
/// radius is 0.25), 10 to its outer face: its leg is 10 − (0.25 + t). Flat: 60 + π (0.25 + K T)
/// + leg.
#[test]
fn a_hem_turns_back_under_a_thickened_plate() {
    let (t, k, gap) = (TEST.thickness, TEST.k_factor, 0.5);
    let mut s = thickened_plate(t);
    run(&mut s, "sheet.hem", json!({"edges": [[50, 0, t]], "length": 10, "gap": gap}));
    // The gap is the space under the leg: the bend's inside radius is half of it.
    let r = gap / 2.0;
    let leg = 10.0 - (r + t);
    let across = 60.0 + PI * (r + k * t) + leg;
    assert!(same_size(flat_size(&mut s), (100.0, across), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 1 && rows[0].0 == 180 && (rows[0].2 - 100.0).abs() < 1e-6, "{rows:?}");
    let (lo, hi) = bbox(&mut s);
    // The bend's axis lies r + t below the top: its outside reaches r + t in front, and the leg
    // lies gap under the plate, its underside gap + t below it.
    let want = ([0.0, -(r + t), -(gap + t)], [100.0, 60.0, t]);
    assert!(lo.iter().zip(want.0).chain(hi.iter().zip(want.1)).all(|(a, b)| (a - b).abs() < 1e-6), "{lo:?} {hi:?}");
    let expect = t * (100.0 * across + PI * t * (0.5 - k) * 100.0);
    let v = body_volume(&s, "Plate");
    assert!((v - expect).abs() < 1e-6 * expect, "{v} vs {expect}");
}

/// Flanges meeting at a corner of the panel they stand on close it (#6748): their surfaces meet
/// along the line where their planes cross, a rip parts them there, and their bends end with the
/// corner's relief. Flanges 20 high on the front and right edges of a plate 100 x 60 thickened
/// from its top are the same metal as a box 100 x 60 x 20 converted from its floor, front and
/// right side with the two floor edges bent: they give the same flat, the same bends and the
/// same folded body, whether the flanges come in one feature or two, and with a round corner
/// relief the flanges ask for as with the rule's.
#[test]
fn flanges_meeting_at_a_corner_close_it_as_converting_the_same_metal_does() {
    let t = TEST.thickness;
    let round = json!({"corner_relief_shape": "round", "corner_relief": 6});
    for (what, flanges, relief) in [
        ("one feature", vec![json!({"edges": [[50, 0, t], [100, 30, t]], "height": 20})], None),
        ("two features", vec![json!({"edges": [[50, 0, t]], "height": 20}), json!({"edges": [[100, 30, t]], "height": 20})], None),
        (
            "a round relief",
            vec![json!({"edges": [[50, 0, t], [100, 30, t]], "height": 20, "corner_relief_shape": "round", "corner_relief": 6})],
            Some(&round),
        ),
    ] {
        let mut flanged = thickened_plate(t);
        for f in flanges {
            run(&mut flanged, "sheet.flange", f);
        }
        let mut boxed = Session::default();
        rule(&mut boxed, "Test", t, TEST.k_factor, TEST.bend_radius, TEST.gap);
        if relief.is_some() {
            run(&mut boxed, "sheet.manage_rules", json!({"name": "Test", "corner_relief_shape": "round", "corner_relief": "6 mm"}));
        }
        run(&mut boxed, "solid.box", json!({"length": 100, "width": 60, "height": 20, "body_name": "Plate"}));
        run(
            &mut boxed,
            "sheet.convert",
            json!({"body": "Plate", "skip": [[50, 30, 20], [50, 60, 10], [0, 30, 10]], "bends": [[50, 0, 0], [100, 30, 0]]}),
        );
        let (got, want) = (flat_size(&mut flanged), flat_size(&mut boxed));
        assert!(same_size(got, want, 1e-6), "{what}: {got:?} vs {want:?}");
        let rows = |s: &mut Session| {
            let mut r = bend_rows(s);
            r.sort_by(|x, y| x.2.total_cmp(&y.2));
            r
        };
        let (got, want) = (rows(&mut flanged), rows(&mut boxed));
        assert!(got.len() == want.len() && got.iter().zip(&want).all(|(a, b)| a.0 == b.0 && (a.2 - b.2).abs() < 1e-6), "{what}: {got:?} vs {want:?}");
        let (got, want) = (bbox(&mut flanged), bbox(&mut boxed));
        assert!(got.0.iter().zip(want.0).chain(got.1.iter().zip(want.1)).all(|(a, b)| (a - b).abs() < 1e-6), "{what}: {got:?} vs {want:?}");
        let (got, want) = (body_volume(&flanged, "Plate"), body_volume(&boxed, "Plate"));
        assert!((got - want).abs() < 1e-6 * want, "{what}: {got} vs {want}");
        assert_eq!(rips(&flanged), 1, "{what}");
    }
}

/// How many rips the sheet made from surfaces has.
fn rips(s: &Session) -> usize {
    let st = s.model.state();
    st.surface_sheets.first().map_or(0, |k| k.sheet.joints.iter().filter(|j| matches!(j.kind, solidcraft_develop::JointKind::Rip(_))).count())
}

/// Four flanges round a plate make a tray (#6748), each corner closed: the same as a box 100 x
/// 60 x 20 converted with its top left off and its floor edges bent.
#[test]
fn four_flanges_round_a_plate_make_the_tray_converting_a_box_makes() {
    let t = TEST.thickness;
    let mut flanged = thickened_plate(t);
    run(&mut flanged, "sheet.flange", json!({"edges": [[50, 0, t], [100, 30, t], [50, 60, t], [0, 30, t]], "height": 20}));
    let mut boxed = Session::default();
    rule(&mut boxed, "Test", t, TEST.k_factor, TEST.bend_radius, TEST.gap);
    run(&mut boxed, "solid.box", json!({"length": 100, "width": 60, "height": 20, "body_name": "Plate"}));
    run(&mut boxed, "sheet.convert", json!({"body": "Plate", "skip": [[50, 30, 20]], "bends": BOTTOM_EDGES}));
    let (got, want) = (flat_size(&mut flanged), flat_size(&mut boxed));
    assert!(same_size(got, want, 1e-6), "{got:?} vs {want:?}");
    let (got, want) = (body_volume(&flanged, "Plate"), body_volume(&boxed, "Plate"));
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
    let (got, want) = (flat(&mut flanged)["cutouts"].clone(), flat(&mut boxed)["cutouts"].clone());
    assert_eq!(got, want);
    assert_eq!(rips(&flanged), 4);
}

/// The flat, bends, place and volume of two sheets agree, and the first has `rips` rips.
fn same_sheet(what: &str, flanged: &mut Session, boxed: &mut Session, rips_here: usize) {
    let (got, want) = (flat_size(flanged), flat_size(boxed));
    assert!(same_size(got, want, 1e-6), "{what}: {got:?} vs {want:?}");
    let rows = |s: &mut Session| {
        let mut r = bend_rows(s);
        r.sort_by(|x, y| x.2.total_cmp(&y.2));
        r
    };
    let (got, want) = (rows(flanged), rows(boxed));
    assert!(got.len() == want.len() && got.iter().zip(&want).all(|(a, b)| a.0 == b.0 && (a.2 - b.2).abs() < 1e-6), "{what}: {got:?} vs {want:?}");
    let (got, want) = (bbox(flanged), bbox(boxed));
    assert!(got.0.iter().zip(want.0).chain(got.1.iter().zip(want.1)).all(|(a, b)| (a - b).abs() < 1e-6), "{what}: {got:?} vs {want:?}");
    let (got, want) = (body_volume(flanged, "Plate"), body_volume(boxed, "Plate"));
    assert!((got - want).abs() < 1e-6 * want, "{what}: {got} vs {want}");
    assert_eq!(rips(flanged), rips_here, "{what}");
}

/// A flange meets a bend Thicken made as it meets another flange's (#6748): an L thickened from
/// its outside (a floor 100 x 60 and a wall 20 high along its back), flanged 20 up its right edge
/// toward the material, closes the corner with the wall, as converting a box 100 x 60 x 20 from
/// its floor, back and right side does.
#[test]
fn a_flange_meets_a_bend_thicken_made_at_a_corner() {
    let t = TEST.thickness;
    let mut flanged = Session::default();
    rule(&mut flanged, "Test", t, TEST.k_factor, TEST.bend_radius, TEST.gap);
    run(&mut flanged, "solid.box", json!({"length": 100, "width": 60, "height": t, "body_name": "Plate"}));
    run(&mut flanged, "solid.box", json!({"corner": [0, 60.0 - t, 0], "length": 100, "width": t, "height": 20, "operation": "join"}));
    run(&mut flanged, "sheet.convert", json!({"mode": "thicken", "body": "Plate", "face": [50, 30, 0], "bends": [[50, 60, 0]]}));
    run(&mut flanged, "sheet.flange", json!({"edges": [[100, 30, 0]], "height": 20, "flip": true}));
    let mut boxed = Session::default();
    rule(&mut boxed, "Test", t, TEST.k_factor, TEST.bend_radius, TEST.gap);
    run(&mut boxed, "solid.box", json!({"length": 100, "width": 60, "height": 20, "body_name": "Plate"}));
    run(
        &mut boxed,
        "sheet.convert",
        json!({"body": "Plate", "skip": [[50, 30, 20], [50, 0, 10], [0, 30, 10]], "bends": [[50, 60, 0], [100, 30, 0]]}),
    );
    same_sheet("an L and a flange", &mut flanged, &mut boxed, 1);
}

/// Flanges at 45° close their corners along the slanted lines where their planes cross (#6748):
/// four flanges 40 √2 along their outer faces round a plate 100 x 60 are the same metal as a box
/// extruded 40 from a 100 x 60 rectangle with a 45° outward taper, converted with its top left
/// off and its floor edges bent. (Extruded 20, Convert would take that box for a plate 20
/// thick: its top, 140 x 100, is more than six times as wide.)
#[test]
fn flanges_at_45_degrees_close_their_corners_as_a_tapered_box_converts() {
    let t = TEST.thickness;
    let mut flanged = thickened_plate(t);
    let edges = json!([[50, 0, t], [100, 30, t], [50, 60, t], [0, 30, t]]);
    run(&mut flanged, "sheet.flange", json!({"edges": edges, "height": 40.0 * SQRT_2, "angle": "45 deg"}));
    let mut boxed = Session::default();
    rule(&mut boxed, "Test", t, TEST.k_factor, TEST.bend_radius, TEST.gap);
    run(&mut boxed, "sketch.create", json!({"plane": "XY", "name": "Floor"}));
    run(&mut boxed, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut boxed, "sketch.finish", json!({}));
    run(&mut boxed, "solid.extrude", json!({"sketch": "Floor", "distance": 40, "taper": "45 deg", "body_name": "Plate"}));
    run(&mut boxed, "sheet.convert", json!({"body": "Plate", "skip": [[50, 30, 40]], "bends": BOTTOM_EDGES}));
    same_sheet("45° flanges", &mut flanged, &mut boxed, 4);
}

/// A flange trims the side of a wall it meets to the line where the two cross (#6748), whatever
/// the wall's side was: a box tapered 45° outward, 40 high on a 100 x 60 floor, converted from
/// its floor and front wall (whose sides lean out with the taper), flanged 40 straight up its
/// right edge, cuts the front wall back to the flange's plane. The same metal is that box cut
/// square at its right and back, converted from its floor, front and right side.
#[test]
fn a_flange_trims_the_leaning_side_of_a_converted_wall_it_meets() {
    let t = TEST.thickness;
    let tapered = |s: &mut Session| {
        rule(s, "Test", t, TEST.k_factor, TEST.bend_radius, TEST.gap);
        run(s, "sketch.create", json!({"plane": "XY", "name": "Floor"}));
        run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(s, "sketch.finish", json!({}));
        run(s, "solid.extrude", json!({"sketch": "Floor", "distance": 40, "taper": "45 deg", "body_name": "Plate"}));
    };
    let mut flanged = Session::default();
    tapered(&mut flanged);
    run(
        &mut flanged,
        "sheet.convert",
        json!({"body": "Plate", "skip": [[50, 30, 40], [50, 70, 10], [-10, 30, 10], [110, 30, 10]], "bends": [[50, 0, 0]]}),
    );
    run(&mut flanged, "sheet.flange", json!({"edges": [[100, 30, 0]], "height": 40, "flip": true}));
    let mut boxed = Session::default();
    tapered(&mut boxed);
    run(&mut boxed, "solid.box", json!({"corner": [100, -100, -10], "length": 200, "width": 300, "height": 100, "operation": "cut"}));
    run(&mut boxed, "solid.box", json!({"corner": [-100, 60, -10], "length": 300, "width": 200, "height": 100, "operation": "cut"}));
    run(
        &mut boxed,
        "sheet.convert",
        json!({"body": "Plate", "skip": [[30, 30, 40], [30, 60, 10], [-10, 30, 10]], "bends": [[50, 0, 0], [100, 30, 0]]}),
    );
    same_sheet("a leaning wall and a flange", &mut flanged, &mut boxed, 1);
}
