//! Tests of workholding: a stock in a vise on parallels (where work zero lands, the jaws and the
//! bars), a turned vise, the setup findings, clamps on the stock, the table machined itself, a
//! rotary unit, and hostile files refused.

use solidcore_record::Vec3;

use solidcraft_geom::Mesh;

use crate::foundation_tests::*;
use crate::machine_model::{ObstacleKind, Table, TablePlace, clearance};
use crate::obstacle::{ObstacleShape, Turn};
use crate::sim::{Footprint, StockShape};
use crate::tool::{Tool, ToolKind};
use crate::workholding::*;

fn table() -> Table {
    Table { size: [302.0, 175.0, 15.0], corner_radius: 5.0, cut_allowance: 0.1, holes: Vec::new() }
}

/// A coupon 101.6 × 62 × 9.525 with work zero on its top, centred.
fn coupon() -> StockShape {
    StockShape::Box { min: Vec3::new(-50.8, -31.0, -9.525), max: Vec3::new(50.8, 31.0, 0.0) }
}

fn vise(turns: u32, parallels: f64) -> Workholding {
    Workholding::from_json(&format!(
        r#"{{"format": "solidcraft.workholding", "version": 1, "holding": "vise",
            "vise": {{"model": {{"length": 175, "width": 64, "height": 64, "floor": 32, "fixed_jaw": 35.6, "moving_jaw": 50, "max_opening": 85}},
                     "center": [227, 87.5], "quarter_turns": {turns}}},
            "parallels": {{"height": {parallels}, "thickness": 3.175, "length": 152.4}}}}"#
    ))
    .unwrap()
}

#[test]
fn a_coupon_in_the_vise_on_parallels() {
    let s = build(&table(), &vise(0, 25.4), &coupon()).unwrap();
    assert!(s.findings.is_empty(), "{:?}", s.findings);
    // Centred on the vise at (227, 87.5), against the fixed jaw at the back; its top is the
    // floor 32 + the 25.4 parallels + 9.525, 2.925 above the jaws.
    let face = 87.5 + 175.0 / 2.0 - 35.6;
    close3(s.work_zero_table, Vec3::new(227.0, face - 31.0, 66.925), 1e-9);
    let jaw = s.obstacles.iter().find(|o| o.name == "the vise's fixed jaw").unwrap();
    close(jaw.max.z, -2.925, 1e-9);
    close(s.stock_above_jaws.unwrap(), 2.925, 1e-9);
    close(jaw.min.y, 31.0, 1e-9);
    let moving = s.obstacles.iter().find(|o| o.name == "the vise's moving jaw").unwrap();
    close(moving.max.y, -31.0, 1e-9);
    // Two bars along the jaws, 152.4 long, under the stock's Y faces.
    let bars: Vec<_> = s.obstacles.iter().filter(|o| o.kind == ObstacleKind::Parallels).collect();
    assert_eq!(bars.len(), 2);
    for b in &bars {
        close(b.max.x - b.min.x, 152.4, 1e-9);
        close(b.max.y - b.min.y, 3.175, 1e-9);
        close(b.max.z, -9.525, 1e-9);
    }
    assert!(bars.iter().any(|b| (b.max.y - 31.0).abs() < 1e-9) && bars.iter().any(|b| (b.min.y + 31.0).abs() < 1e-9));
    // The table is under it all: its top 66.925 below work zero.
    close(s.table.corner.z, -66.925, 1e-9);
    // Tall parallels leave nothing in the jaws.
    assert!(build(&table(), &vise(0, 30.0), &coupon()).unwrap().findings.iter().any(|f| f.code == "shallow-grip"));
    assert!(build(&table(), &vise(0, 33.0), &coupon()).unwrap().findings.iter().any(|f| f.code == "stock-not-gripped"));
}

#[test]
fn a_turned_vise_grips_the_x_faces() {
    let s = build(&table(), &vise(1, 25.4), &coupon()).unwrap();
    let jaw = s.obstacles.iter().find(|o| o.name == "the vise's fixed jaw").unwrap();
    close(jaw.max.x, -50.8, 1e-9);
    // 101.6 across the jaws is past the 85 mm opening.
    assert!(s.findings.iter().any(|f| f.code == "vise-too-narrow"));
}

#[test]
fn clamps_sit_on_the_stock() {
    let hold = Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [30, 56.5],
            "parallels": {"height": 10, "thickness": 3.175, "length": 152.4},
            "clamps": [{"center": [10, 10], "size": [40, 20], "height": 20}, {"center": [20, 87.5], "size": [40, 20], "height": 20}]}"#,
    )
    .unwrap();
    let s = build(&table(), &hold, &coupon()).unwrap();
    let c = s.obstacles.iter().find(|o| o.name == "clamp 1").unwrap();
    close(c.min.z, 0.0, 1e-9);
    close(c.max.z, 20.0, 1e-9);
    assert!(s.obstacles.iter().any(|o| o.kind == ObstacleKind::Parallels));
    let codes: Vec<&str> = s.findings.iter().map(|f| f.code.as_str()).collect();
    assert_eq!(codes, ["clamp-off-stock"], "clamp 1 misses the stock, clamp 2 is on it");
    close(s.work_zero_table.z, 10.0 + 9.525, 1e-9);
    close(s.work_zero_table.x, 30.0 + 50.8, 1e-9);
    // Over the table's edge, and no clamps at all.
    let off = Workholding::from_json(r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [250, 10]}"#).unwrap();
    let codes: Vec<String> = build(&table(), &off, &coupon()).unwrap().findings.into_iter().map(|f| f.code).collect();
    assert_eq!(codes, ["no-clamps", "stock-off-table"]);
}

#[test]
fn the_table_itself_and_a_rotary_unit() {
    let hold =
        Workholding::from_json(r#"{"format": "solidcraft.workholding", "version": 1, "holding": "table", "table_zero": [302, 0, 0]}"#).unwrap();
    let plate = StockShape::Box { min: Vec3::new(-302.0, 0.0, -15.0), max: Vec3::new(0.0, 175.0, 0.0) };
    let s = build(&table(), &hold, &plate).unwrap();
    assert!(s.obstacles.is_empty() && s.findings.is_empty());
    close3(s.table.corner, Vec3::new(-302.0, 0.0, 0.0), 1e-12);
    let hold = Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "rotary",
            "rotary": {"axis": [60, 70], "headstock": {"min": [10, 20, 0], "max": [80, 100, 110]},
                       "chuck": {"diameter": 80, "length": 40, "face_x": 120, "side": "minus_x"},
                       "tailstock": {"min": [250, 40, 0], "max": [290, 80, 90]}}}"#,
    )
    .unwrap();
    let bar = StockShape::Cylinder { x_min: 0.0, x_max: 100.0, radius: 15.0 };
    let s = build(&table(), &hold, &bar).unwrap();
    // A chuck on the -X side: work zero on the A axis at the chuck's face (the bar's low end
    // there), the chuck reaching back from it towards -X.
    close3(s.work_zero_table, Vec3::new(120.0, 60.0, 70.0), 1e-12);
    let chuck = s.obstacles.iter().find(|o| o.name == "the rotary unit's chuck").unwrap();
    close3(chuck.max, Vec3::new(0.0, 40.0, 40.0), 1e-12);
    assert_eq!(s.obstacles.iter().filter(|o| o.kind == ObstacleKind::Rotary).count(), 3);
    // A rotary unit holds a bar the A axis runs through (a square one too), not a plate below
    // the axis.
    assert!(build(&table(), &hold, &coupon()).is_err());
    let square = StockShape::Box { min: Vec3::new(0.0, -15.0, -15.0), max: Vec3::new(100.0, 15.0, 15.0) };
    close3(build(&table(), &hold, &square).unwrap().work_zero_table, Vec3::new(120.0, 60.0, 70.0), 1e-12);
}

/// The usual layout: the unit and its chuck at the table's +X end, the bar standing out towards
/// -X with its +X end in the jaws; work zero on the A axis.
#[test]
fn a_rotary_unit_holds_the_bar_from_the_plus_x_side_by_default() {
    let hold = Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "rotary",
            "rotary": {"axis": [57, 65], "headstock": {"min": [222, 12, 0], "max": [300, 102, 160]},
                       "chuck": {"diameter": 80, "length": 45, "face_x": 180,
                                 "jaws": {"radial": 12, "axial": 15}}}}"#,
    )
    .unwrap();
    // The bar: 30 mm across, its free end at X -50, 20 mm of it in the jaws (to X 20).
    let bar = StockShape::Cylinder { x_min: -50.0, x_max: 20.0, radius: 15.0 };
    let s = build(&table(), &hold, &bar).unwrap();
    // The bar's +X end at the chuck's face: work zero 20 mm towards -X from it.
    close3(s.work_zero_table, Vec3::new(160.0, 57.0, 65.0), 1e-12);
    // The chuck's body reaches from its face towards +X, into the unit.
    let chuck = s.obstacles.iter().find(|o| o.name == "the rotary unit's chuck").unwrap();
    close3(chuck.min, Vec3::new(20.0, -40.0, -40.0), 1e-12);
    close3(chuck.max, Vec3::new(65.0, 40.0, 40.0), 1e-12);
    // The jaws stand 15 mm out of the face towards the work, 12 mm beyond the bar, never inside it:
    // the ring from the bar's surface (r 15) to r 27.
    let jaws: Vec<_> = s.obstacles.iter().filter(|o| o.name == "the rotary unit's jaws").collect();
    assert_eq!(jaws.len(), 1);
    for j in &jaws {
        assert!(j.min.x >= 5.0 - 1e-9 && j.max.x <= 20.0 + 1e-9, "{j:?}");
        assert!(j.min.y >= -27.0 - 1e-9 && j.max.y <= 27.0 + 1e-9 && j.min.z >= -27.0 - 1e-9 && j.max.z <= 27.0 + 1e-9, "{j:?}");
    }
    let meets = |x: f64, y: f64, z0: f64, z1: f64| jaws.iter().any(|j| j.shape.meets(Footprint::Circle { x, y, r: 1.0 }, z0, z1, None, 1e-6));
    // Inside the bar: no jaw. Above the bar in the jaws: a tool coming down there meets a jaw.
    assert!(!meets(12.0, 0.0, -10.0, 10.0));
    assert!(meets(12.0, 0.0, 20.0, 30.0));
    // Beyond their reach (r 27), or off their X span: nothing.
    assert!(!meets(12.0, 0.0, 28.0, 40.0));
    assert!(!meets(25.0, 0.0, 20.0, 30.0));
    assert!(s.findings.is_empty(), "{:?}", s.findings);
    // How the bar is gripped, for the setup sheet: its +X end at the face, 15 mm in the jaws,
    // 55 mm standing out of them to its free end, work zero 20 mm out from the face.
    let g = s.bar_grip.unwrap();
    for (got, want) in [(g.face_x, 20.0), (g.held_end_x, 20.0), (g.free_end_x, -50.0), (g.in_jaws, 15.0), (g.stands_out, 55.0), (g.zero_out, 20.0)] {
        close(got, want, 1e-9);
    }
    // Work zero given: a bar that does not reach the jaws is not held.
    let mut short = hold.clone();
    if let Some(r) = short.rotary.as_mut() {
        r.zero_x = Some(100.0);
    }
    let s = build(&table(), &short, &bar).unwrap();
    assert_eq!(s.findings.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(), ["stock-not-gripped"]);
    // ... and one with 2 mm in the jaws is held only just.
    if let Some(r) = short.rotary.as_mut() {
        r.zero_x = Some(147.0);
    }
    let s = build(&table(), &short, &bar).unwrap();
    assert_eq!(s.findings.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(), ["shallow-grip"]);
    // The face at work X 33: the bar's end 13 mm short of it, 2 mm in the jaws (X 18 to 20).
    let g = s.bar_grip.unwrap();
    for (got, want) in [(g.face_x, 33.0), (g.held_end_x, 20.0), (g.in_jaws, 2.0), (g.stands_out, 68.0), (g.zero_out, 33.0)] {
        close(got, want, 1e-9);
    }
    // Jaws of no size are refused, and so is a side that is not a side.
    for bad in [r#""jaws": {"radial": 0, "axial": 15}"#, r#""side": "up""#] {
        let text = format!(
            r#"{{"format": "solidcraft.workholding", "version": 1, "holding": "rotary",
                "rotary": {{"axis": [57, 65], "headstock": {{"min": [222, 12, 0], "max": [300, 102, 160]}},
                           "chuck": {{"diameter": 80, "length": 45, "face_x": 180, {bad}}}}}}}"#
        );
        assert!(Workholding::from_json(&text).is_err(), "{bad}");
    }
}

#[test]
fn hostile_workholding_is_refused() {
    let bad = [
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "vise"}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps"}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "table"}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "rotary"}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "glue"}"#,
        r#"{"format": "other", "version": 1, "holding": "table", "table_zero": [0, 0, 0]}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "table", "table_zero": [0, 0, 1e300]}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [0, 0], "clamps": [{"center": [0, 0], "size": [-1, 1], "height": 1}]}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "vise", "vise": {"model": {"length": 10, "width": 64, "height": 64, "floor": 32, "fixed_jaw": 6, "moving_jaw": 5, "max_opening": 85}, "center": [0, 0]}}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "vise", "vise": {"model": {"length": 175, "width": 64, "height": 30, "floor": 32, "fixed_jaw": 6, "moving_jaw": 5, "max_opening": 85}, "center": [0, 0]}}"#,
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "table", "table_zero": [0, 0, 0], "surprise": 1}"#,
        "not json",
    ];
    for (k, text) in bad.iter().enumerate() {
        assert!(Workholding::from_json(text).is_err(), "case {k} was taken");
    }
    let many = format!(
        r#"{{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [0, 0], "clamps": [{}]}}"#,
        vec![r#"{"center": [0, 0], "size": [1, 1], "height": 1}"#; 101].join(",")
    );
    assert!(Workholding::from_json(&many).is_err());
}

/// A rotary unit as parts: the cover with its lower corner cut away is two boxes, a ring
/// (a home flag) turns with A behind the chuck, and the jaws are four blocks with gaps between.
fn unit_in_parts() -> Workholding {
    Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "rotary",
            "rotary": {"axis": [57, 61],
                       "plate": {"min": [220, 2, 0], "max": [300, 112, 20]},
                       "chuck": {"diameter": 80, "length": 76, "face_x": 141.5,
                                 "jaws": {"count": 4, "radial": 33, "axial": 16, "width": 12, "angle_deg": 0}},
                       "parts": [{"name": "the housing", "box": {"min": [217.5, 22, 20], "max": [300, 92, 98]}},
                                 {"name": "the cover's top", "box": {"min": [204, 2, 106], "max": [338, 112, 190]}},
                                 {"name": "the cover's back", "box": {"min": [294, 2, 29], "max": [338, 112, 106]}},
                                 {"name": "the spindle pulley", "cylinder": {"diameter": 92, "x": [308, 328]}}],
                       "rings": [{"name": "the home flag", "diameter": 102, "x": [197.5, 199.5]}]}}"#,
    )
    .unwrap()
}

#[test]
fn a_rotary_unit_of_several_parts_leaves_the_cut_corner_free() {
    let bar = StockShape::Cylinder { x_min: -60.0, x_max: 16.0, radius: 11.0 };
    let s = build(&table(), &unit_in_parts(), &bar).unwrap();
    close3(s.work_zero_table, Vec3::new(125.5, 57.0, 61.0), 1e-12);
    // No headstock box: each part is its own obstacle, by its name.
    assert!(s.obstacles.iter().all(|o| o.name != "the rotary unit's headstock"));
    for name in ["the housing", "the cover's top", "the cover's back", "the spindle pulley", "the home flag", "the rotary unit's chuck"] {
        assert!(s.obstacles.iter().any(|o| o.name == name), "{name}");
    }
    let tool = Tool::new(1, ToolKind::FlatEndmill, 6.0, 15.0);
    let table = TablePlace { corner: Vec3::ZERO - s.work_zero_table, size: [302.0, 175.0, 15.0] };
    let at = |x: f64, y: f64, z: f64| Vec3::new(x, y, z) - s.work_zero_table;
    let hit = |p: Vec3| clearance(&tool, None, Some(&table), &s.obstacles, &[p], &[], 0.5, 0.01);
    // Under the cut corner (beside the housing, below the cover's front edge): free. The one box
    // round it all blocked it.
    assert!(hit(at(210.0, 10.0, 40.0)).is_empty());
    // In the cover's back, and into the housing: named in the message.
    let h = hit(at(300.0, 10.0, 40.0));
    assert_eq!(h.iter().map(|h| h.code.as_str()).collect::<Vec<_>>(), ["cutter-hits-rotary"]);
    assert!(h[0].message.contains("the cover's back"), "{}", h[0].message);
    assert!(hit(at(230.0, 57.0, 90.0)).iter().any(|h| h.message.contains("the housing")));
    // The home flag sweeps r 51 at X 197.5..199.5: a tool 45 above the axis meets it, 55 above does not.
    assert!(hit(at(198.5, 57.0, 61.0 + 45.0)).iter().any(|h| h.message.contains("the home flag")));
    assert!(hit(at(198.5, 57.0, 61.0 + 55.0)).iter().all(|h| !h.message.contains("the home flag")));
}

#[test]
fn jaw_blocks_turn_with_a_and_leave_their_gaps_free() {
    let bar = StockShape::Cylinder { x_min: -60.0, x_max: 16.0, radius: 11.0 };
    let s = build(&table(), &unit_in_parts(), &bar).unwrap();
    let jaws: Vec<_> = s.obstacles.iter().filter(|o| o.name == "the rotary unit's jaws").collect();
    assert_eq!(jaws.len(), 4);
    assert!(jaws.iter().all(|j| j.turns()));
    let tool = Tool { stickout: Some(20.0), ..Tool::new(1, ToolKind::FlatEndmill, 3.0, 10.0) };
    let table = TablePlace { corner: Vec3::ZERO - s.work_zero_table, size: [302.0, 175.0, 15.0] };
    // A Ø3 cutter coming down at 45° round the bar, over the jaws' X span (work X 0..16), to
    // r 20: between two jaws at A 0.
    let r = 20.0;
    let tip = Vec3::new(8.0, -r * std::f64::consts::FRAC_1_SQRT_2, r * std::f64::consts::FRAC_1_SQRT_2);
    let check = |a_deg: Option<f64>| {
        let turn = a_deg.map(|a| Turn { angle: a.to_radians(), center: [0.0, 0.0] });
        clearance(&tool, None, Some(&table), &s.obstacles, &[tip], &[turn], 0.5, 0.01)
    };
    let is_jaw = |h: &Vec<crate::machine_model::ClearanceHit>| h.iter().any(|h| h.message.contains("jaws"));
    // A 0: the gap. A 45: a jaw turned under the tool (the first jaw points up, +Z, at A 0, and
    // turns towards -Y with a positive turn about +X).
    assert!(!is_jaw(&check(Some(0.0))), "{:?}", check(Some(0.0)));
    assert!(is_jaw(&check(Some(45.0))), "{:?}", check(Some(45.0)));
    // Where A is not known, the ring they sweep.
    assert!(is_jaw(&check(None)));
    // Straight above the bar at A 0: the first jaw.
    let top = Vec3::new(8.0, 0.0, 20.0);
    let h = clearance(&tool, None, Some(&table), &s.obstacles, &[top], &[Some(Turn { angle: 0.0, center: [0.0, 0.0] })], 0.5, 0.01);
    assert!(is_jaw(&h));
}

/// A mesh "L": a 40 x 40 x 10 slab with a 10 x 40 x 40 wall at its +X end, as triangles.
fn l_mesh(at: Vec3) -> Mesh {
    let mut m = Mesh::default();
    let mut add_box = |lo: Vec3, hi: Vec3| {
        let base = m.positions.len() as u32;
        for k in 0..8u32 {
            m.positions.push(
                at + Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z }),
            );
        }
        for [a, b, c, d] in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            m.triangles.push([base + a, base + b, base + c]);
            m.triangles.push([base + a, base + c, base + d]);
        }
    };
    add_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 40.0, 10.0));
    add_box(Vec3::new(30.0, 0.0, 10.0), Vec3::new(40.0, 40.0, 40.0));
    m
}

#[test]
fn a_workholding_mesh_is_checked_triangle_by_triangle_and_drawn() {
    let hold = Workholding::from_json(r#"{"format": "solidcraft.workholding", "version": 1, "holding": "table", "table_zero": [0, 0, 0]}"#).unwrap();
    let plate = StockShape::Box { min: Vec3::new(0.0, 0.0, -1.0), max: Vec3::new(10.0, 10.0, 0.0) };
    let mesh = LoadedMesh {
        name: "the angle plate".into(),
        frame: MeshFrame::Table,
        check: true,
        color: None,
        kind: ObstacleKind::Fixture,
        mesh: l_mesh(Vec3::new(100.0, 50.0, 0.0)),
    };
    let s = build_with(&table(), &hold, &plate, std::slice::from_ref(&mesh)).unwrap();
    let o = s.obstacles.iter().find(|o| o.name == "the angle plate").unwrap();
    assert!(matches!(o.shape, ObstacleShape::Mesh(_)));
    close3(o.min, Vec3::new(100.0, 50.0, 0.0), 1e-12);
    close3(o.max, Vec3::new(140.0, 90.0, 40.0), 1e-12);
    assert_eq!(s.meshes.len(), 1);
    let tool = Tool::new(1, ToolKind::FlatEndmill, 6.0, 15.0);
    let hit = |p: Vec3| clearance(&tool, None, None, &s.obstacles, &[p], &[], 0.5, 0.01);
    // Above the slab, in front of the wall: inside the mesh's box, but clear of it.
    assert!(hit(Vec3::new(110.0, 70.0, 12.0)).is_empty());
    // Down into the slab, and into the wall.
    assert_eq!(hit(Vec3::new(110.0, 70.0, 8.0)).iter().map(|h| h.code.as_str()).collect::<Vec<_>>(), ["cutter-hits-fixture"]);
    // The wall is at X 130: a Ø6 cutter at X 126 is clear of it, at X 128 in it.
    assert!(hit(Vec3::new(126.0, 70.0, 20.0)).is_empty());
    assert!(!hit(Vec3::new(128.0, 70.0, 20.0)).is_empty());
    assert!(hit(Vec3::new(128.0, 70.0, 20.0))[0].message.contains("the angle plate"));
    // A zone inside the slab that crosses none of its triangles is inside it all the same.
    let grid = crate::obstacle::MeshGrid::new(&l_mesh(Vec3::ZERO), Vec3::ZERO).unwrap();
    assert!(grid.meets(Footprint::Circle { x: 10.0, y: 20.0, r: 1.0 }, 3.0, 6.0, 0.01));
    assert!(!grid.meets(Footprint::Circle { x: 10.0, y: 20.0, r: 1.0 }, 12.0, 20.0, 0.01));
    assert!(grid.meets(Footprint::Rect { x0: 32.0, x1: 38.0, y0: 10.0, y1: 12.0 }, 20.0, 30.0, 0.01));
    // A mesh drawn only is not checked; a NaN or an empty mesh that is checked is refused.
    let drawn = LoadedMesh { check: false, ..mesh.clone() };
    let s = build_with(&table(), &hold, &plate, &[drawn]).unwrap();
    assert!(s.obstacles.is_empty() && s.meshes.len() == 1);
    let mut bad = mesh.clone();
    bad.mesh.positions[0].x = f64::NAN;
    assert!(build_with(&table(), &hold, &plate, &[bad]).is_err());
    let empty = LoadedMesh { mesh: Mesh::default(), ..mesh };
    assert!(build_with(&table(), &hold, &plate, &[empty]).is_err());
}

#[test]
fn workholding_meshes_and_new_parts_are_validated() {
    let base = r#""format": "solidcraft.workholding", "version": 1, "holding": "table", "table_zero": [0, 0, 0]"#;
    let ok = Workholding::from_json(&format!(
        r#"{{{base}, "meshes": [{{"path": "unit.stl", "frame": "rotary"}}, {{"path": "plate.step", "check": false}}]}}"#
    ))
    .unwrap();
    assert!(!ok.meshes[0].checked() && !ok.meshes[1].checked());
    let rotary = |inner: &str| {
        format!(
            r#"{{"format": "solidcraft.workholding", "version": 1, "holding": "rotary",
                "rotary": {{"axis": [57, 61], "chuck": {{"diameter": 80, "length": 50, "face_x": 140}}, {inner}}}}}"#
        )
    };
    let bad = [
        format!(r#"{{{base}, "meshes": [{{"path": "unit.stl", "frame": "rotary", "check": true}}]}}"#),
        format!(r#"{{{base}, "meshes": [{{"path": ""}}]}}"#),
        format!(r#"{{{base}, "meshes": [{{"path": "a.stl", "color": [2, 0, 0]}}]}}"#),
        format!(r#"{{{base}, "meshes": [{{"path": "a.stl", "transform": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 1e300]]}}]}}"#),
        format!(r#"{{{base}, "meshes": [{}]}}"#, vec![r#"{"path": "a.stl"}"#; 65].join(",")),
        rotary(r#""parts": [{"name": "both", "box": {"min": [0, 0, 0], "max": [1, 1, 1]}, "cylinder": {"diameter": 1, "x": [0, 1]}}]"#),
        rotary(r#""parts": [{"name": "neither"}]"#),
        rotary(r#""parts": [{"name": "", "box": {"min": [0, 0, 0], "max": [1, 1, 1]}}]"#),
        rotary(r#""parts": [{"name": "flat", "box": {"min": [0, 0, 0], "max": [1, 1, 0]}}]"#),
        rotary(r#""parts": [{"name": "nan", "cylinder": {"diameter": -1, "x": [0, 1]}}]"#),
        rotary(r#""rings": [{"diameter": 10, "x": [5, 1]}]"#),
        rotary(r#""rings": [{"diameter": 10, "x": [1, 5], "turns": true}]"#),
    ];
    for (k, text) in bad.iter().enumerate() {
        assert!(Workholding::from_json(text).is_err(), "case {k} was taken");
    }
    let jaws = |j: &str| {
        format!(
            r#"{{"format": "solidcraft.workholding", "version": 1, "holding": "rotary",
                "rotary": {{"axis": [57, 61], "chuck": {{"diameter": 80, "length": 50, "face_x": 140, "jaws": {{"radial": 10, "axial": 10, {j}}}}}}}}}"#
        )
    };
    assert!(Workholding::from_json(&jaws(r#""width": 12, "angle_deg": 45"#)).is_ok());
    assert!(Workholding::from_json(&jaws(r#""width": 0"#)).is_err());
    assert!(Workholding::from_json(&jaws(r#""angle_deg": 1e9"#)).is_err());
}

#[test]
fn a_workholding_file_from_a_newer_build_is_read_with_its_unknown_keys_named() {
    let text = r#"{"format": "solidcraft.workholding", "version": 1, "holding": "rotary", "colour_scheme": "blue",
        "rotary": {"axis": [57, 61], "chuck": {"diameter": 80, "length": 50, "face_x": 140, "key_sockets": 3,
                   "jaws": {"radial": 10, "axial": 10, "steps": [1, 2]}},
                   "parts": [{"name": "the cover", "box": {"min": [0, 0, 0], "max": [1, 1, 1]}, "sheet": 1.5}]}}"#;
    // A job's own workholding refuses them (a typo must fail) ...
    assert!(Workholding::from_json(text).is_err());
    // ... a file reads, naming each.
    let (w, ignored) = Workholding::from_json_lenient(text).unwrap();
    assert_eq!(w.rotary.as_ref().map(|r| r.parts.len()), Some(1));
    let mut ignored = ignored;
    ignored.sort();
    assert_eq!(ignored, ["colour_scheme", "rotary.chuck.jaws.steps", "rotary.chuck.key_sockets", "rotary.parts[0].sheet"]);
    // A value of the wrong kind is still refused.
    assert!(Workholding::from_json_lenient(&text.replace(r#""length": 50"#, r#""length": "long""#)).is_err());
    // A known file reads with nothing left out.
    let plain = r#"{"format": "solidcraft.workholding", "version": 1, "holding": "table", "table_zero": [0, 0, 0]}"#;
    assert!(Workholding::from_json_lenient(plain).unwrap().1.is_empty());
}

#[test]
fn the_meshes_of_a_workholding_share_one_budget() {
    // Each of these meshes is within the limits on its own (7000 triangles over the whole of its
    // grid: about 12 M grid entries), but not both together: a file naming 64 of them would
    // have asked for gigabytes.
    let mut m = Mesh {
        positions: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 10.0), Vec3::new(0.0, 100.0, 10.0)],
        ..Mesh::default()
    };
    for k in 0..7000u32 {
        m.triangles.push(if k % 2 == 0 { [0, 1, 2] } else { [0, 2, 3] });
    }
    assert!(crate::obstacle::MeshGrid::new(&m, Vec3::ZERO).is_ok());
    let hold = Workholding::from_json(r#"{"format": "solidcraft.workholding", "version": 1, "holding": "table", "table_zero": [0, 0, 0]}"#).unwrap();
    let plate = StockShape::Box { min: Vec3::new(0.0, 0.0, -1.0), max: Vec3::new(10.0, 10.0, 0.0) };
    let one = LoadedMesh { name: "a ramp".into(), frame: MeshFrame::Table, check: true, color: None, kind: ObstacleKind::Fixture, mesh: m };
    assert!(build_with(&table(), &hold, &plate, std::slice::from_ref(&one)).is_ok());
    // (The scene itself is not printed: it holds every triangle.)
    let e = build_with(&table(), &hold, &plate, &[one.clone(), one]).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("too many"), "refused: `{e}`");
}

/// A clearing carried out past a flat's open sides stops short of the jaws: the stock's
/// rectangle seen down a frame turned about X is cut back at the limit on the chuck's side, on
/// whichever local axis runs along work X, and left whole down a frame looking along X.
#[test]
fn a_clearing_past_open_sides_stops_short_of_the_jaws() {
    use solidcore_record::Vec2;
    let rect = [Vec2::new(-82.0, -16.0), Vec2::new(25.0, 16.0)];
    let flat =
        crate::Frame { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 0.5, 0.866), z: Vec3::new(0.0, -0.866, 0.5) };
    let r = crate::workholding::short_of_jaws(rect, &flat, ChuckSide::PlusX, 3.0);
    assert_eq!(r, Some([Vec2::new(-82.0, -16.0), Vec2::new(3.0, 16.0)]));
    let r = crate::workholding::short_of_jaws(rect, &flat, ChuckSide::MinusX, -70.0);
    assert_eq!(r, Some([Vec2::new(-70.0, -16.0), Vec2::new(25.0, 16.0)]));
    // Work X along local -Y, origin moved: work X = 5 - v.
    let turned =
        crate::Frame { origin: Vec3::new(5.0, 0.0, 0.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(-1.0, 0.0, 0.0), z: Vec3::new(0.0, 0.0, 1.0) };
    let rect = [Vec2::new(-16.0, -20.0), Vec2::new(16.0, 87.0)];
    let r = crate::workholding::short_of_jaws(rect, &turned, ChuckSide::PlusX, 3.0);
    assert_eq!(r, Some([Vec2::new(-16.0, 2.0), Vec2::new(16.0, 87.0)]));
    // Looking along X: nothing to cut back.
    let end =
        crate::Frame { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0), z: Vec3::new(1.0, 0.0, 0.0) };
    assert_eq!(crate::workholding::short_of_jaws(rect, &end, ChuckSide::PlusX, 3.0), Some(rect));
    // Nothing left on the free side, or a limit that is no number.
    assert_eq!(crate::workholding::short_of_jaws([Vec2::new(5.0, -1.0), Vec2::new(9.0, 1.0)], &flat, ChuckSide::PlusX, 3.0), None);
    assert_eq!(crate::workholding::short_of_jaws(rect, &flat, ChuckSide::PlusX, f64::NAN), None);
}

/// A 155 x 85 plate (a 150 x 80 part in it) clamped to a fixture plate's tapped grid: clamps go
/// on grid holes across two opposite edges, their noses on the 2.5 mm margin, clear of where the
/// tool reaches; with the tool over the whole stock no placement is clear.
#[test]
fn clamps_placed_on_the_grid_clear_of_the_toolpaths() {
    let hold = Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [30, 56.5],
            "clamp_grid": {"origin": [8.125, -1.4], "pitch": [19.05, 19.05], "count": [16, 11]},
            "clamps": [{"center": [20, 87.5], "size": [40, 20], "height": 20}]}"#,
    )
    .unwrap();
    let stock = [[-77.5, -42.5], [77.5, 42.5]];
    let part = [[-75.0, -40.0], [75.0, 40.0]];
    // A window pocket and four corner holes, grown by the holder's radius.
    let keep = [[[-50.0, -25.0], [50.0, 25.0]], [[-90.0, -60.0], [-50.0, -20.0]], [[50.0, 20.0], [90.0, 60.0]]];
    let clamps = place_clamps(&hold, stock, part, &keep).unwrap();
    assert!(clamps.len() >= 2, "{clamps:?}");
    let shift = [30.0 + 77.5, 56.5 + 42.5];
    for c in &clamps {
        let lo = [c.center[0] - c.size[0] / 2.0 - shift[0], c.center[1] - c.size[1] / 2.0 - shift[1]];
        let hi = [c.center[0] + c.size[0] / 2.0 - shift[0], c.center[1] + c.size[1] / 2.0 - shift[1]];
        // On the stock, but not on the part.
        assert!(hi[0] > stock[0][0] && lo[0] < stock[1][0] && hi[1] > stock[0][1] && lo[1] < stock[1][1], "{c:?}");
        assert!(lo[0] >= part[1][0] - 1e-9 || hi[0] <= part[0][0] + 1e-9 || lo[1] >= part[1][1] - 1e-9 || hi[1] <= part[0][1] + 1e-9, "{c:?}");
        assert!(keep.iter().all(|k| hi[0] <= k[0][0] || lo[0] >= k[1][0] || hi[1] <= k[0][1] || lo[1] >= k[1][1]), "{c:?}");
        // Bolted in a hole of the grid: the strap's centre line on a grid column or row.
        let on = |v: f64, o: f64| ((v - o) / 19.05 - ((v - o) / 19.05).round()).abs() < 1e-9;
        assert!(on(c.center[0], 8.125) || on(c.center[1], -1.4), "{c:?}");
    }
    // Two opposite edges.
    let edge =
        |c: &Clamp| (c.size[0] > c.size[1], c.center[if c.size[0] > c.size[1] { 0 } else { 1 }] > shift[if c.size[0] > c.size[1] { 0 } else { 1 }]);
    let sides: Vec<(bool, bool)> = clamps.iter().map(edge).collect();
    assert!(sides.iter().any(|a| sides.iter().any(|b| a.0 == b.0 && a.1 != b.1)), "{sides:?}");
    // The tool everywhere (a facing over the whole stock): nowhere is clear.
    assert_eq!(place_clamps(&hold, stock, part, &[[[-90.0, -60.0], [90.0, 60.0]]]), None);
    // No margin outside the part: nothing to clamp.
    assert_eq!(place_clamps(&hold, stock, stock, &[]), None);
    // A hostile grid is refused.
    let bad = r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [0, 0], "clamp_grid": {"origin": [0, 0], "pitch": [0, 1], "count": [4, 4]}}"#;
    assert!(Workholding::from_json(bad).is_err());
}

/// Clamps keep clear of each cut, not of a box around an operation's cuts: one drilling over
/// the four corner holes of a 150 x 80 part leaves room for clamps between the holes, where a
/// box around all four holes leaves none.
#[test]
fn clamps_keep_clear_of_each_cut_not_the_operations_box() {
    let hold = Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [30, 56.5],
            "clamp_grid": {"origin": [8.125, -1.4], "pitch": [19.05, 19.05], "count": [16, 11]},
            "clamps": [{"center": [20, 87.5], "size": [40, 20], "height": 20}]}"#,
    )
    .unwrap();
    let stock = [[-77.5, -42.5], [77.5, 42.5]];
    let part = [[-75.0, -40.0], [75.0, 40.0]];
    let down = Vec3::new(0.0, 0.0, 1.0);
    let at = |x: f64, y: f64, z: f64| crate::Pose::new(Vec3::new(x, y, z), down);
    let mut moves = Vec::new();
    for (x, y) in [(-70.0, -35.0), (70.0, -35.0), (70.0, 35.0), (-70.0, 35.0)] {
        moves.push(crate::Move5::Rapid { to: at(x, y, 5.0) });
        moves.push(crate::Move5::Rapid { to: at(x, y, 1.0) });
        moves.push(crate::Move5::Feed { to: at(x, y, -5.0), feed: 100.0 });
        moves.push(crate::Move5::Rapid { to: at(x, y, 5.0) });
        moves.push(crate::Move5::Dwell { seconds: 0.1 });
    }
    let runs = cut_runs(&moves);
    assert_eq!(runs.len(), 4, "{runs:?}");
    assert_eq!(runs[0], [[-70.0, -35.0, -5.0], [-70.0, -35.0, 1.0]]);
    // Each hole grown by a holder's radius and the clearance.
    let grow = |r: &[[f64; 3]; 2]| [[r[0][0] - 9.0, r[0][1] - 9.0], [r[1][0] + 9.0, r[1][1] + 9.0]];
    let each: Vec<[[f64; 2]; 2]> = runs.iter().map(grow).collect();
    assert!(place_clamps(&hold, stock, part, &each).is_some_and(|c| c.len() >= 2));
    // The operation's box spans the stock's corners: no clamp is clear of it.
    assert_eq!(place_clamps(&hold, stock, part, &[[[-79.0, -44.0], [79.0, 44.0]]]), None);
    // Runs that overlap are one box; nothing cut, no box.
    let pass = [
        crate::Move5::Feed { to: at(0.0, 0.0, -1.0), feed: 100.0 },
        crate::Move5::Rapid { to: at(0.0, 0.0, 2.0) },
        crate::Move5::Rapid { to: at(1.0, 0.0, 2.0) },
        crate::Move5::Feed { to: at(1.0, 0.0, -1.0), feed: 100.0 },
        crate::Move5::Feed { to: at(-1.0, 0.0, -1.0), feed: 100.0 },
    ];
    assert_eq!(cut_runs(&pass), vec![[[-1.0, 0.0, -1.0], [1.0, 0.0, 2.0]]]);
    assert!(cut_runs(&moves[..2]).is_empty());
}

#[test]
fn the_grip_is_weighed_against_the_deepest_side_cut() {
    // A bracket stock standing 29 mm above jaws that grip 4.13 mm of it, cut 27 mm down its
    // side: the reach above the jaws is 27, a third of it is 9 mm of grip, so 4.13 warns.
    let deep = SideCut { operation: "outline".into(), depth: 27.0 };
    let short = grip_for_cut(4.13, 29.0, &deep, false).unwrap();
    assert_eq!(short.code, "grip-short-for-cut");
    assert_eq!(short.severity, crate::findings::Severity::Warning);
    assert!(short.message.contains("`outline`") && short.message.contains("9 mm"), "{}", short.message);
    assert!(short.message.contains("step jaws") && short.message.contains("parallels"), "{}", short.message);
    assert!(!short.message.contains("  "), "{}", short.message);
    // Gripped 10 mm deep, the same cut is held.
    assert!(grip_for_cut(10.0, 29.0, &deep, false).is_none());
    // In steel the jaws grip half the reach: 10 mm is short of 13.5.
    assert!(grip_for_cut(10.0, 29.0, &deep, true).is_some());
    // The reach is only what stands above the jaws: a stock 6 mm proud cut 27 mm deep asks 2 mm.
    assert!(grip_for_cut(4.13, 6.0, &deep, false).is_none());
    // A light side cut (a 3 mm facing step) does not ask for more.
    assert!(grip_for_cut(4.13, 29.0, &SideCut { operation: "face".into(), depth: 3.0 }, false).is_none());
    // Nothing known, nothing said; hostile numbers never warn or panic.
    for (g, a, d) in
        [(0.0, 29.0, 27.0), (4.0, -1.0, 27.0), (4.0, 29.0, 0.0), (f64::NAN, 29.0, 27.0), (4.0, f64::INFINITY, 27.0), (4.0, 29.0, f64::NAN)]
    {
        assert!(grip_for_cut(g, a, &SideCut { operation: "x".into(), depth: d }, false).is_none(), "{g} {a} {d}");
    }
}

#[test]
fn the_scene_says_how_much_of_the_stock_the_jaws_grip() {
    // The coupon on 25.4 parallels on a floor at 32, jaws 64 high: 6.6 mm of its 9.525 gripped.
    let s = build(&table(), &vise(0, 25.4), &coupon()).unwrap();
    close(s.stock_grip.unwrap(), 64.0 - 32.0 - 25.4, 1e-9);
    // Above the jaws: nothing gripped, nothing to weigh.
    assert!(build(&table(), &vise(0, 33.0), &coupon()).unwrap().stock_grip.is_none());
}
