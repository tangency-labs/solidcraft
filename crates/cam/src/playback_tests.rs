//! Tests of playing a program: the cutting model against closed forms through the player (a
//! pocket's floor and walls, a ball slot's profile, a groove round a bar), seeking forward and
//! back through keyframes to the same stock, seeking by time, line and operation, what is found
//! where (a rapid in the stock, a gouge, cutting with the spindle stopped, the shank rubbing, the
//! holder on the stock, a machine part and the cutter against a clamp), and the display meshes.

use std::sync::Arc;

use solidcore_record::Vec3;

use crate::findings::Severity;
use crate::foundation_tests::*;
use crate::kinematics::Machine;
use crate::machine_model::{MachineModel, ObstacleKind};
use crate::machine_time::{DwellUnit, MachineRates};
use crate::playback::{Budget, MoveKind, OpsGiven, Playback, PlaybackSetup, Timeline};
use crate::sim::{SimParams, SimState, StockShape, plan_sim, simulate};
use crate::stock_mesh::{CLASS_TOOL, CLASS_UNCUT, display_step, tile_mesh};
use crate::tool_mesh::tool_meshes;
use crate::workholding::{Workholding, build};
use crate::*;

fn flat(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 20.0)
}

fn program(moves: &str) -> String {
    format!("%\nG90 G94 G91.1 G17\nG21\nT1 M6\nS12000 M3\nG0 X0. Y0.\nG43 Z10. H1\n{moves}\nM5\nG28 G91 Z0.\nG90\nM30\n%\n")
}

fn stock() -> StockShape {
    StockShape::Box { min: Vec3::new(0.0, 0.0, -10.0), max: Vec3::new(40.0, 40.0, 0.0) }
}

fn rates() -> MachineRates {
    MachineRates { rapid_xyz_mm_min: 6000.0, ..MachineRates::default() }
}

/// A player of `text` with `tools` on `stock` against `part`, keyframes every `visits` cells.
fn player(text: &str, machine: &Machine, tools: &[Tool], stock: &StockShape, part: Surface, setup: Option<PlaybackSetup>, visits: f64) -> Playback {
    let plan = Arc::new(plan_sim(text, machine, tools, stock, &SimParams { tolerance: 0.01, resolution: Some(0.1) }).unwrap());
    let timeline = Arc::new(Timeline::build(&plan, text, &rates(), DwellUnit::Seconds, None).unwrap());
    let setup = setup.unwrap_or(PlaybackSetup { tools: tools.to_vec(), ..PlaybackSetup::default() });
    Playback::new(plan, timeline, Arc::new(part), setup, Budget { keyframe_visits: visits, keyframe_bytes: 64 << 20 }, true)
}

fn height(p: &Playback, x: f64, y: f64) -> f64 {
    let crate::sim::Grid::Heights { x0, y0, dx, dy, .. } = *p.plan().grid() else { panic!() };
    p.state().value(p.plan(), ((x - x0) / dx).floor() as usize, ((y - y0) / dy).floor() as usize).unwrap()
}

fn pocket() -> String {
    let mut moves = String::from("G0 X13. Y13.\nG0 Z1.\nG1 Z-3. F200\nG1 X27. F800\nY27.\nX13.\nY13.\n");
    let mut y = 13.0;
    while y < 27.0 {
        moves.push_str(&format!("G1 Y{y:.3}\nX27.\nG1 Y{:.3}\nX13.\n", y + 2.0));
        y += 4.0;
    }
    moves.push_str("G0 Z10.\n");
    program(&moves)
}

/// Whether two stocks are the same to rounding.
fn same(a: &crate::sim::StockGrid, b: &crate::sim::StockGrid) -> bool {
    match (a, b) {
        (crate::sim::StockGrid::Heights { heights: x, .. }, crate::sim::StockGrid::Heights { heights: y, .. }) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| (p - q).abs() < 1e-9)
        }
        _ => false,
    }
}

#[test]
fn a_pocket_played_has_its_floor_and_walls_and_seeking_back_gives_the_same_stock() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0));
    let text = pocket();
    let whole =
        simulate(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), &part, &SimParams { tolerance: 0.01, resolution: Some(0.1) }).unwrap();
    // Whole moves, keyframes every 20 000 cells: back and forth to the same stock, bit for bit.
    let mut p = player(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), part.clone(), None, 20_000.0);
    let n = p.plan().moves();
    p.seek(n, 0.0, None).unwrap();
    assert!(p.keyframes() > 2, "{}", p.keyframes());
    assert_eq!(p.state().grid(p.plan()), whole.stock);
    let g = p.generation();
    p.seek(n / 3, 1.0, None).unwrap();
    assert!(p.generation() > g);
    assert_eq!(p.at(), (n / 3, 1.0));
    p.seek(n, 0.0, None).unwrap();
    assert_eq!(p.state().grid(p.plan()), whole.stock);
    // The floor and the walls, exactly.
    assert_eq!(height(&p, 20.0, 20.0), -3.0);
    assert_eq!(height(&p, 10.05, 20.0), -3.0);
    assert_eq!(height(&p, 9.95, 20.0), 0.0);
    assert_eq!(height(&p, 20.0, 29.95), -3.0);
    assert_eq!(height(&p, 20.0, 30.05), 0.0);
    assert_eq!(height(&p, 10.25, 10.25), 0.0);
    // Nothing wrong with this program.
    assert!(p.findings().is_empty(), "{:?}", p.findings().events());
    // A frame's worth at a time (parts of moves too): the same stock, to rounding.
    let mut q = player(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), part, None, 20_000.0);
    let mut frames = 0;
    while !q.seek(n / 2, 0.5, Some(5_000.0)).unwrap().done {
        frames += 1;
        assert!(frames < 10_000);
    }
    assert!(frames > 2, "the budget split the seek: {frames}");
    assert_eq!(q.at(), (n / 2, 0.5));
    while !q.seek(n, 0.0, Some(5_000.0)).unwrap().done {}
    assert!(same(&q.state().grid(q.plan()), &whole.stock));
    // The display: tiles of the stock, coloured by the tool that cut them last.
    let step = display_step(p.plan(), 1024);
    let tile = tile_mesh(p.plan(), p.state(), 12, 12, step);
    assert!(!tile.triangles.is_empty() && tile.tri_face.len() == tile.triangles.len());
    assert!(tile.tri_face.contains(&CLASS_TOOL));
    assert!(tile_mesh(p.plan(), p.state(), 0, 0, step).tri_face.iter().all(|c| *c == CLASS_UNCUT));
}

/// Where the stock is kept along X and Y too (a part with faces under its own material along
/// Z), the view draws what those lines hold that the heights cannot: a slot an earlier setup cut
/// across the part under the stock's top shows its ceiling and its floor; without the lines the
/// column is drawn solid.
#[test]
fn the_stock_view_shows_the_air_the_side_lines_keep_under_material() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -6.0));
    let text = program(
        "G0 X20. Y20.
G0 Z5.",
    );
    let plan = plan_sim(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), &SimParams { tolerance: 0.01, resolution: Some(0.5) }).unwrap();
    // The slot, y under 20 and z from -6 to -4, cut by the earlier setup.
    let carried = |p: Vec3| !(p.y < 20.0 && p.z > -6.0 && p.z < -4.0);
    // Triangles over the slot facing down at its ceiling and up at its floor, and any facing
    // down at that height past its edge.
    let faces = |state: &SimState| {
        let (tx, ty) = plan.grid().tiles();
        let (mut down, mut up, mut past) = (0usize, 0usize, 0usize);
        for tj in 0..ty {
            for ti in 0..tx {
                let m = tile_mesh(&plan, state, ti, tj, 1);
                for t in &m.triangles {
                    let [a, b, c] = t.map(|i| m.positions[i as usize]);
                    let (u, v) = (b - a, c - a);
                    let n = Vec3::new(0.0, 0.0, u.x * v.y - u.y * v.x);
                    let mid = Vec3::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0, (a.z + b.z + c.z) / 3.0);
                    if n.z < 0.0 && (mid.z + 4.0).abs() < 0.1 {
                        if mid.y < 20.0 { down += 1 } else { past += 1 }
                    }
                    if n.z > 0.0 && (mid.z + 6.0).abs() < 0.1 && mid.y < 20.0 {
                        up += 1;
                    }
                }
            }
        }
        (down, up, past)
    };
    assert_eq!(faces(&SimState::new(&plan)), (0, 0, 0));
    let mut kept = SimState::new(&plan);
    assert!(kept.carve_sides(&plan, &carried, &part));
    // Every display cell over the slot (80 × 40 of them, half a millimetre each), two
    // triangles each, at its ceiling and at its floor; none past it.
    assert_eq!(faces(&kept), (2 * 80 * 40, 2 * 80 * 40, 0));
}

#[test]
fn a_ball_slot_played_in_pieces_has_the_ball_profile() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -4.0));
    let ball = Tool::new(1, ToolKind::BallEndmill, 6.0, 20.0);
    let text = program("G0 X5. Y20.\nG0 Z1.\nG1 Z-3. F200\nG1 X35. F800\nG0 Z10.");
    let mut p = player(&text, &Machine::three_axis(), std::slice::from_ref(&ball), &stock(), part, None, 1.0e9);
    // The slot in ten pieces of its move, then the rest.
    let slot = p.timeline().at_line(11).unwrap();
    for k in 1..=10 {
        p.seek(slot, k as f64 * 0.1, None).unwrap();
    }
    p.seek(usize::MAX, 0.0, None).unwrap();
    for v in [0.05, 0.95, 1.55, 2.05, 2.55, 2.95] {
        let want = -3.0 + 3.0 - (9.0f64 - v * v).sqrt();
        close(height(&p, 20.0, 20.0 + v), want, 1e-9);
        close(height(&p, 20.0, 20.0 - v), want, 1e-9);
    }
    assert_eq!(height(&p, 20.0, 23.05), 0.0);
    // The timeline: a plunge, a feed, rapids, times in order.
    let t = p.timeline();
    assert_eq!(t.moves.get(slot).map(|m| m.kind), Some(MoveKind::Feed));
    assert_eq!(t.moves.get(slot - 1).map(|m| m.kind), Some(MoveKind::Plunge));
    assert!(t.moves.windows(2).all(|w| w[0].t1 <= w[1].t0 + 1e-12));
    // 30 mm at 800 mm/min.
    close(t.moves[slot].t1 - t.moves[slot].t0, 30.0 / 800.0 * 60.0, 1e-9);
    // By time: the middle of the slot.
    let mid = 0.5 * (t.moves[slot].t0 + t.moves[slot].t1);
    let (m, f) = t.at_time(mid);
    assert_eq!(m, slot);
    close(f, 0.5, 1e-9);
}

#[test]
fn a_groove_round_a_bar_is_as_deep_as_the_tool_went() {
    let machine = Machine::four_axis_a();
    let bar = StockShape::Cylinder { x_min: -10.0, x_max: 10.0, radius: 10.0 };
    let part = cylinder_x(6.0, -15.0, 15.0, 64);
    let text = "G21\nT1 M6\nS12000 M3\nG0 X0. Y0. Z30. A0.\nG1 Z8. F100\nG1 A360. F2000\nG0 Z30.\n";
    let mut p = player(text, &machine, &[flat(4.0)], &bar, part, None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let crate::sim::Grid::Radii { x0, dx, nx, nt, .. } = *p.plan().grid() else { panic!() };
    for k in 0..nt {
        for i in 0..nx {
            let x = x0 + (i as f64 + 0.5) * dx;
            let r = p.state().value(p.plan(), i, k).unwrap();
            if x.abs() < 1.5 {
                // Within half the tolerance of the depth, all the way round.
                assert!((8.0 - 1e-9..=8.005 + 1e-9).contains(&r), "x {x} k {k}: {r}");
            } else if x.abs() > 2.1 {
                assert_eq!(r, 10.0, "x {x}");
            }
        }
    }
    assert!(p.findings().is_empty(), "{:?}", p.findings().events());
}

#[test]
fn what_goes_wrong_is_found_at_its_move_and_place() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    // A rapid through the stock at line 9, then a cut 1 mm into the part at line 12.
    let text = program("G0 X5. Y20.\nG0 Z-2.\nG0 X10.\nG1 Z-6. F200\nG1 X30. F800\nG0 Z10.");
    let mut p = player(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), part.clone(), None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let ev = p.findings().events();
    let rapid = ev.iter().find(|e| e.code == "rapid-in-stock").unwrap();
    assert_eq!((rapid.line, rapid.severity), (9, Severity::Error));
    close(rapid.depth, 2.0, 1e-9);
    let gouge = ev.iter().find(|e| e.code == "gouge").unwrap();
    assert_eq!(gouge.line, 11);
    close(gouge.depth, 1.0, 0.02);
    assert!(gouge.at.unwrap().z < -5.0);
    // Findings so far grow as the tool goes.
    assert!(p.findings().merged_until(p.timeline().at_line(9).unwrap()).is_empty());
    assert!(p.findings().merged_until(p.timeline().at_line(9).unwrap() + 1).has("rapid-in-stock"));

    // With the spindle stopped, and the shank rubbing.
    let short = Tool::new(1, ToolKind::FlatEndmill, 6.0, 2.0);
    let text = "G21\nT1 M6\nS12000 M3\nG0 X5. Y20. Z1.\nG1 Z-4. F200\nG1 X15. F800\nM5\nG1 X25.\nG0 Z10.\n";
    let mut p = player(text, &Machine::three_axis(), std::slice::from_ref(&short), &stock(), part.clone(), None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let merged = p.findings().merged();
    assert!(merged.has("cut-spindle-off") && merged.has("shank-rubs"), "{:?}", merged.list());
    assert_eq!(p.findings().events().iter().find(|e| e.code == "cut-spindle-off").unwrap().line, 8);

    // A holder (Ø20) 10 mm above the tip, the tool plunged 12 deep: the holder comes down on it.
    let held =
        Tool { stickout: Some(10.0), holder: vec![HolderSection { diameter: 20.0, length: 20.0 }], ..Tool::new(1, ToolKind::FlatEndmill, 6.0, 10.0) };
    let text = program("G0 X20. Y20.\nG0 Z1.\nG1 Z-12. F200\nG0 Z10.");
    let mut p = player(&text, &Machine::three_axis(), std::slice::from_ref(&held), &stock(), Surface::new(Vec::new()).unwrap(), None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let hit = p.findings().events().iter().find(|e| e.code == "holder-hits-stock").unwrap();
    assert_eq!(hit.line, 10);
    assert!(hit.at.unwrap().z > -2.0 - 1e-9);
    // Not when it goes in only as far as the stickout.
    let text = program("G0 X20. Y20.\nG0 Z1.\nG1 Z-9.9 F200\nG0 Z10.");
    let mut p = player(&text, &Machine::three_axis(), std::slice::from_ref(&held), &stock(), Surface::new(Vec::new()).unwrap(), None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    assert!(!p.findings().merged().has("holder-hits-stock"));
}

/// A machine with a plate on the spindle reaching 40 mm out in +X from the axis, from the nose
/// down to 5 mm below it, on a 300 × 200 table.
fn plate_machine() -> MachineModel {
    MachineModel::from_json(
        r#"{"format": "solidcraft.machine_model", "version": 1, "name": "test mill",
            "spindle": {"nose_gap": 0, "nose_max_above_table": 120},
            "table": {"size": [300, 200, 15], "cut_allowance": 0.1},
            "parts": [{"name": "the side plate", "code": "side-plate", "mount": "spindle",
                       "shape": {"box": {"min": [10, -10, -5], "max": [40, 10, 50]}}}]}"#,
    )
    .unwrap()
}

#[test]
fn a_machine_part_and_the_cutter_against_a_clamp() {
    // The stock clamped at (100, 80) on the table, a clamp 20 high on its +X end.
    let hold = Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [100, 80],
            "clamps": [{"center": [140, 100], "size": [20, 20], "height": 20}]}"#,
    )
    .unwrap();
    let model = plate_machine();
    let scene = build(&model.table, &hold, &stock()).unwrap();
    // Work zero at the stock's top front-left: the table top 10 below it.
    close3(scene.work_zero_table, Vec3::new(100.0, 80.0, 10.0), 1e-12);
    let clamp = scene.obstacles.iter().find(|o| o.kind == ObstacleKind::Clamp).unwrap();
    close3(clamp.min, Vec3::new(30.0, 10.0, 0.0), 1e-12);
    close3(clamp.max, Vec3::new(50.0, 30.0, 20.0), 1e-12);
    // 20 mm of stickout and no holder: the nose is 20 above the tip, the plate from 15 above it.
    let tool = Tool { stickout: Some(20.0), ..Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0) };
    let setup = |m: Option<MachineModel>| PlaybackSetup {
        tools: vec![tool.clone()],
        model: m.map(Arc::new),
        scene: Some(Arc::new(scene.clone())),
        deliberate: Vec::new(),
        hidden: None,
    };
    // Along Y at X5, Z1: the plate reaches X45, over the clamp, its bottom at Z16 below its top (20).
    let text = program("G0 X5. Y-20.\nG0 Z1.\nG1 Y60. F800\nG0 Z60.");
    let mut p = player(
        &text,
        &Machine::three_axis(),
        std::slice::from_ref(&tool),
        &stock(),
        Surface::new(Vec::new()).unwrap(),
        Some(setup(Some(plate_machine()))),
        1.0e9,
    );
    p.seek(usize::MAX, 0.0, None).unwrap();
    let hit = p.findings().events().iter().find(|e| e.code == "side-plate-hits-clamp").unwrap();
    assert_eq!((hit.line, hit.severity), (10, Severity::Error));
    // ... and nothing without the machine model.
    let mut p =
        player(&text, &Machine::three_axis(), std::slice::from_ref(&tool), &stock(), Surface::new(Vec::new()).unwrap(), Some(setup(None)), 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    assert!(!p.findings().merged().has("side-plate-hits-clamp"));
    // The cutter through the clamp, and into the table past its allowance.
    let text = program("G0 X40. Y-20.\nG0 Z5.\nG1 Y60. F800\nG0 Z60.\nG0 X-20. Y-20.\nG1 Z-10.5 F200\nG0 Z60.");
    let mut p =
        player(&text, &Machine::three_axis(), std::slice::from_ref(&tool), &stock(), Surface::new(Vec::new()).unwrap(), Some(setup(None)), 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let merged = p.findings().merged();
    assert!(merged.has("cutter-hits-clamp"), "{:?}", merged.list());
    // The nose above the spindle's travel at Z60: 60 + 20 + 10 = 90 above the table... within 120;
    // at Z95 it is 125.
    assert!(!merged.has("z-travel"));
    let text = program("G0 X-20. Y-20.\nG0 Z95.");
    let mut p = player(
        &text,
        &Machine::three_axis(),
        std::slice::from_ref(&tool),
        &stock(),
        Surface::new(Vec::new()).unwrap(),
        Some(setup(Some(plate_machine()))),
        1.0e9,
    );
    p.seek(usize::MAX, 0.0, None).unwrap();
    assert!(p.findings().merged().has("z-travel"));
    // An error in the simulation as in the job's check: the program cannot run as written.
    let z = p.findings().merged().list().into_iter().find(|f| f.code == "z-travel").unwrap();
    assert_eq!(z.severity, Severity::Error);
}

/// The check a job runs before it is written finds what the playback finds against the
/// workholding, the table and the spindle's travel, at the same moves, without cutting the
/// stock: once per kind and tool, with how many moves have it.
#[test]
fn the_clearance_check_finds_what_the_playback_finds() {
    let hold = Workholding::from_json(
        r#"{"format": "solidcraft.workholding", "version": 1, "holding": "clamps", "stock_at": [100, 80],
            "clamps": [{"center": [140, 100], "size": [20, 20], "height": 20}]}"#,
    )
    .unwrap();
    let model = plate_machine();
    let scene = build(&model.table, &hold, &stock()).unwrap();
    let tool = Tool { stickout: Some(20.0), ..Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0) };
    // Through the clamp twice, into the table, and up past the spindle's travel.
    let text = program("G0 X40. Y-20.\nG0 Z5.\nG1 Y60. F800\nG1 Y-20.\nG0 Z60.\nG0 X-20. Y-20.\nG1 Z-10.5 F200\nG0 Z95.");
    let plan =
        plan_sim(&text, &Machine::three_axis(), std::slice::from_ref(&tool), &stock(), &SimParams { tolerance: 0.01, resolution: Some(1.0e3) })
            .unwrap();
    let hits = crate::playback::clearance_check(&plan, std::slice::from_ref(&tool), Some(&model), Some(&scene), &|_| 0);
    let codes: Vec<&str> = hits.iter().map(|h| h.first.code.as_str()).collect();
    for code in ["cutter-hits-clamp", "cutter-into-table", "z-travel"] {
        assert!(codes.contains(&code), "{code}: {codes:?}");
    }
    let clamp = hits.iter().find(|h| h.first.code == "cutter-hits-clamp").unwrap();
    assert_eq!((clamp.tool, clamp.moves, clamp.first.line, clamp.first.severity), (1, 2, 10, Severity::Error));
    let z = hits.iter().find(|h| h.first.code == "z-travel").unwrap();
    assert_eq!(z.first.severity, Severity::Error);
    assert!((z.highest - 95.0).abs() < 1e-9, "{}", z.highest);
    // Grouped by line (each its own operation), the two moves through the clamp are two hits.
    let apart = crate::playback::clearance_check(&plan, std::slice::from_ref(&tool), None, Some(&scene), &|line| line);
    let clamps: Vec<(usize, usize)> = apart.iter().filter(|h| h.first.code == "cutter-hits-clamp").map(|h| (h.group, h.moves)).collect();
    assert_eq!(clamps, [(10, 1), (11, 1)]);
    // The playback finds the same codes, at the same first lines.
    let setup =
        PlaybackSetup { tools: vec![tool.clone()], model: Some(Arc::new(model)), scene: Some(Arc::new(scene)), deliberate: Vec::new(), hidden: None };
    let mut p = player(&text, &Machine::three_axis(), std::slice::from_ref(&tool), &stock(), Surface::new(Vec::new()).unwrap(), Some(setup), 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    for h in &hits {
        let e = p.findings().events().iter().find(|e| e.code == h.first.code).unwrap_or_else(|| panic!("{}", h.first.code));
        assert_eq!((e.line, e.at_move), (h.first.line, h.first.at_move), "{}", h.first.code);
    }
    // Neither a model nor a workholding: nothing to check.
    assert!(crate::playback::clearance_check(&plan, std::slice::from_ref(&tool), None, None, &|_| 0).is_empty());
}

#[test]
fn operations_tool_changes_and_seeking_by_them() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let text = "G21\nT2 M6\nS10000 M3\nG0 X10. Y10. Z5.\nG1 Z-1. F100\nG0 Z5.\nT7 M6\nS10000 M3\nG0 X30. Y30. Z5.\nG1 Z-1. F100\nG0 Z10.\n";
    let tools = [Tool { number: 2, ..flat(2.0) }, Tool { number: 7, ..flat(10.0) }];
    let plan = Arc::new(plan_sim(text, &Machine::three_axis(), &tools, &stock(), &SimParams { tolerance: 0.01, resolution: Some(0.2) }).unwrap());
    // By tool change when no operations are given.
    let t = Timeline::build(&plan, text, &rates(), DwellUnit::Seconds, None).unwrap();
    assert_eq!(t.ops.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["T2", "T7"]);
    assert_eq!(t.tool_changes.iter().map(|c| (c.at_move, c.tool)).collect::<Vec<_>>(), [(0, 2), (3, 7)]);
    // By the job's operations, one per block.
    let names = ["drill a".to_string(), "drill b".to_string()];
    let owners = [0, 0, 0, 1, 1, 1];
    let t = Timeline::build(&plan, text, &rates(), DwellUnit::Seconds, Some(OpsGiven { names: &names, owners: &owners })).unwrap();
    assert_eq!(t.ops.iter().map(|o| (o.name.as_str(), o.first, o.end)).collect::<Vec<_>>(), [("drill a", 0, 3), ("drill b", 3, 6)]);
    assert_eq!(t.at_line(8), Some(2));
    assert_eq!(t.at_line(10), Some(4));
    let mut p = Playback::new(
        plan,
        Arc::new(t),
        Arc::new(part),
        PlaybackSetup { tools: tools.to_vec(), ..PlaybackSetup::default() },
        Budget::default(),
        true,
    );
    let second = p.timeline().ops[1].first;
    p.seek(second, 0.0, None).unwrap();
    assert_eq!(p.tool(p.plan().step(second).unwrap().slot).map(|t| t.number), Some(7));
    close3(p.pose_now().unwrap().tip, Vec3::new(10.0, 10.0, 5.0), 1e-9);
    p.seek(second, 1.0, None).unwrap();
    close3(p.pose_now().unwrap().tip, Vec3::new(30.0, 30.0, 5.0), 1e-9);
}

#[test]
fn tools_are_drawn_from_their_profile() {
    let held = Tool {
        stickout: Some(30.0),
        holder: vec![HolderSection { diameter: 34.0, length: 4.0 }, HolderSection { diameter: 34.0, length: 18.0 }],
        ..Tool::new(1, ToolKind::BallEndmill, 6.0, 12.0)
    };
    let m = tool_meshes(&held, 24).unwrap();
    let top = |mesh: &solidcraft_geom::Mesh| mesh.positions.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
    let low = |mesh: &solidcraft_geom::Mesh| mesh.positions.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    close(low(&m.cutting), 0.0, 1e-12);
    close(top(&m.cutting), 12.0, 1e-12);
    close(top(&m.shank), 30.0, 1e-12);
    close(top(&m.holder), 52.0, 1e-12);
    // The ball's points are on its sphere.
    assert!(
        m.cutting.positions.iter().filter(|p| p.z < 3.0 - 1e-9).all(|p| ((p.x * p.x + p.y * p.y + (p.z - 3.0).powi(2)).sqrt() - 3.0).abs() < 1e-9)
    );
    let thread = Tool { thread_pitch: Some(1.5), tooth_height: Some(0.8), ..Tool::new(2, ToolKind::ThreadMill, 8.0, 9.0) };
    assert!(
        tool_meshes(&thread, 16).unwrap().cutting.positions.len()
            > tool_meshes(&Tool::new(3, ToolKind::FlatEndmill, 8.0, 9.0), 16).unwrap().cutting.positions.len()
    );
}

/// Every kind of tool the view draws, each with its closed-form height above the tip at a
/// distance from its axis (written here from the tools' sizes, not taken from the simulation).
type HeightAt = Box<dyn Fn(f64) -> f64>;

fn kinds() -> Vec<(Tool, HeightAt)> {
    let r = 3.0;
    let bull = 1.0;
    let drill = 1.0 / (118f64.to_radians() * 0.5).tan();
    let face_corner = 0.8;
    let corner = move |c: f64| move |d: f64| if d <= r - c { 0.0 } else { c - (c * c - (d - (r - c)).powi(2)).max(0.0).sqrt() };
    vec![
        (Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0), Box::new(|_d: f64| 0.0)),
        (Tool::new(2, ToolKind::BallEndmill, 6.0, 20.0), Box::new(move |d: f64| r - (r * r - d * d).max(0.0).sqrt())),
        (Tool { corner_radius: Some(bull), ..Tool::new(3, ToolKind::BullNose, 6.0, 20.0) }, Box::new(corner(bull))),
        (Tool { angle_deg: Some(90.0), ..Tool::new(4, ToolKind::Chamfer, 6.0, 20.0) }, Box::new(|d: f64| d)),
        (Tool { point_angle_deg: Some(118.0), ..Tool::new(5, ToolKind::Drill, 6.0, 20.0) }, Box::new(move |d: f64| drill * d)),
        (Tool { corner_radius: Some(face_corner), ..Tool::new(6, ToolKind::FaceMill, 6.0, 20.0) }, Box::new(corner(face_corner))),
    ]
}

#[test]
fn every_kind_of_tool_cuts_its_own_profile_along_lines_and_arcs() {
    let kinds = kinds();
    let tools: Vec<Tool> = kinds.iter().map(|(t, _)| t.clone()).collect();
    let wide = StockShape::Box { min: Vec3::new(0.0, 0.0, -10.0), max: Vec3::new(80.0, 80.0, 0.0) };
    // Tool k: a straight slot along X at y = 6 + 12 k, 2 mm deep, and a half circle of radius 8
    // about (60, 6 + 12 k), 1.5 mm deep, going round through -Y.
    let mut text = String::from("G21 G90 G17\n");
    for (k, t) in tools.iter().enumerate() {
        let y = 6.0 + 12.0 * k as f64;
        text.push_str(&format!(
            "T{n} M6\nS12000 M3\nG0 X5. Y{y:.3} Z5.\nG1 Z-2. F200\nG1 X35. F600\nG0 Z5.\nG0 X68. Y{y:.3}\nG1 Z-1.5 F200\nG2 X52. Y{y:.3} I-8. J0. F600\nG0 Z5.\n",
            n = t.number
        ));
    }
    text.push_str("M5\nM30\n");
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(80.0, 80.0, -9.0));
    let mut p = player(&text, &Machine::three_axis(), &tools, &wide, part, None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    for (k, (t, h)) in kinds.iter().enumerate() {
        let y = 6.0 + 12.0 * k as f64;
        for i in 0..60 {
            let d = -2.95 + 0.1 * f64::from(i);
            let want = (-2.0 + h(d.abs())).min(0.0);
            let got = height(&p, 20.05, y + d);
            assert!((got - want).abs() < 1e-9, "{:?} line, {d:+.2} mm off the path: {got} (want {want})", t.kind);
        }
        // Across the arc where it is lowest (straight below its centre).
        for i in 0..60 {
            let off = -2.95 + 0.1 * f64::from(i);
            let want = (-1.5 + h(off.abs())).min(0.0);
            let got = height(&p, 60.05, y - 8.0 + off);
            assert!((got - want).abs() < 2e-3, "{:?} arc, {off:+.2} mm off the path: {got} (want {want})", t.kind);
        }
        // Outside its reach the stock is whole.
        assert_eq!(height(&p, 20.05, y + 3.15), 0.0, "{:?}", t.kind);
    }
}

#[test]
fn every_kind_of_tool_is_drawn_with_its_own_profile_and_size() {
    for (t, h) in kinds() {
        let m = tool_meshes(&t, 48).unwrap();
        let r = t.diameter * 0.5;
        let widest = m.cutting.positions.iter().map(|p| p.x.hypot(p.y)).fold(0.0, f64::max);
        assert!((widest - r).abs() < 1e-9, "{:?}: drawn {} mm across, is {} mm", t.kind, 2.0 * widest, t.diameter);
        let lowest = m.cutting.positions.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
        assert!(lowest.abs() < 1e-12, "{:?}: the tip is drawn at {lowest}, not at the origin", t.kind);
        // Every point is on the cutting end's surface, on the side, or on the top's centre.
        for p in &m.cutting.positions {
            let rho = p.x.hypot(p.y);
            let on_end = (p.z - h(rho)).abs() < 1e-9;
            let on_side = (rho - r).abs() < 1e-9 && p.z >= h(r) - 1e-9;
            let top = rho < 1e-9 && (p.z - t.flute_length).abs() < 1e-9;
            assert!(on_end || on_side || top, "{:?}: a point at r {rho}, z {} is off the tool", t.kind, p.z);
        }
        assert!(m.cutting.positions.iter().all(|p| p.z <= t.flute_length + 1e-9), "{:?}", t.kind);
    }
}

/// The reference cut: when `SOLIDCRAFT_CAM_REFERENCE_CUT` names a folder with `program.nc` (five
/// tools: T1 flat, T2 ball, T3 bull nose r1, T4 90 degree chamfer, T5 118 degree drill, all 6 mm)
/// cut on a 40 x 40 x 10 block from (0, 0, -10), and `heights.txt` (the stock heights an
/// independent heightfield simulator gave on the same 0.1 mm grid: `x0 y0 cell nx ny`, then one
/// height per cell, row by row), the simulation's stock is held against it cell by cell.
#[test]
fn the_stock_matches_an_independent_simulators_cut() {
    let Some(dir) = std::env::var_os("SOLIDCRAFT_CAM_REFERENCE_CUT").map(std::path::PathBuf::from) else {
        eprintln!("SOLIDCRAFT_CAM_REFERENCE_CUT is not set: no reference cut to compare with");
        return;
    };
    let text = std::fs::read_to_string(dir.join("program.nc")).unwrap();
    let reference = std::fs::read_to_string(dir.join("heights.txt")).unwrap();
    let mut words = reference.split_whitespace().map(|w| w.parse::<f64>().unwrap());
    let mut next = || words.next().unwrap();
    let (x0, y0, cell, nx, ny) = (next(), next(), next(), next() as usize, next() as usize);
    let want: Vec<f64> = words.collect();
    assert_eq!((nx, ny, want.len()), (400, 400, 160_000), "the reference is on the same 0.1 mm grid");
    let tools = vec![
        Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0),
        Tool::new(2, ToolKind::BallEndmill, 6.0, 20.0),
        Tool { corner_radius: Some(1.0), ..Tool::new(3, ToolKind::BullNose, 6.0, 20.0) },
        Tool { angle_deg: Some(90.0), ..Tool::new(4, ToolKind::Chamfer, 6.0, 20.0) },
        Tool { point_angle_deg: Some(118.0), ..Tool::new(5, ToolKind::Drill, 6.0, 20.0) },
    ];
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -9.0));
    let mut p = player(&text, &Machine::three_axis(), &tools, &stock(), part, None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let at = |i: i64, j: i64| (i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < ny).then(|| want[j as usize * nx + i as usize]);
    let mut diffs: Vec<f64> = Vec::new();
    let mut beside_walls = 0usize;
    for j in 0..ny as i64 {
        for i in 0..nx as i64 {
            let Some(r) = at(i, j) else { continue };
            // The reference stamps the tool every 0.9 cell, so a cell beside a wall sees the
            // wall at the stamp's offset; those cells are left out.
            if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(a, b)| at(i + a, j + b).is_some_and(|q| (q - r).abs() > 0.15)) {
                beside_walls += 1;
                continue;
            }
            let ours = height(&p, x0 + (i as f64 + 0.5) * cell, y0 + (j as f64 + 0.5) * cell);
            diffs.push(ours - r);
        }
    }
    diffs.sort_by(f64::total_cmp);
    let q = |f: f64| diffs[((diffs.len() - 1) as f64 * f) as usize];
    eprintln!(
        "reference cut: {} cells compared ({beside_walls} beside walls left out); ours - reference: min {:.4}, 1% {:.4}, median {:.4}, 99% {:.4}, max {:.4}",
        diffs.len(),
        q(0.0),
        q(0.01),
        q(0.5),
        q(0.99),
        q(1.0)
    );
    // The reference stamps points, so it is never deeper than an exact sweep by more than its
    // f32 rounding, and lags it by at most the scallop between its stamps.
    assert!(q(1.0) < 0.01, "the simulation leaves material the reference cut: {} mm", q(1.0));
    assert!(-q(0.0) < 0.06, "the simulation cuts deeper than the reference: {} mm", -q(0.0));
}

fn close_to(a: Vec3, b: Vec3, what: &str) {
    assert!((a.x - b.x).abs().max((a.y - b.y).abs()).max((a.z - b.z).abs()) < 1e-9, "{what}: {a:?}, want {b:?}");
}

#[test]
fn a_turn_of_plus_90_brings_the_works_plus_y_up_to_the_tool() {
    let bar = StockShape::Cylinder { x_min: -10.0, x_max: 10.0, radius: 10.0 };
    let text = "G21\nT1 M6\nS12000 M3\nG0 X0. Y0. Z30. A90.\nG1 Z8. F100\nG0 Z30.\n";
    for reversed in [false, true] {
        let machine = Machine::four_axis_a_with(Vec3::ZERO, reversed, None);
        let mut p = player(text, &machine, &[flat(4.0)], &bar, cylinder_x(5.0, -15.0, 15.0, 64), None, 1.0e9);
        let plunge = p.timeline().at_line(5).unwrap();
        p.seek(plunge, 1.0, None).unwrap();
        // The side that comes up under the tool: +Y, or -Y on a table wired the other way.
        let up = if reversed { -1.0 } else { 1.0 };
        let pose = p.pose_now().unwrap();
        close_to(pose.tip, Vec3::new(0.0, 8.0 * up, 0.0), "the tip");
        close_to(pose.axis, Vec3::new(0.0, up, 0.0), "the tool axis");
        // The spindle's own frame against the work: X stays X, its Z is the tool axis, its +Y
        // (the machine's back) is the work's -Z (or +Z).
        let [x, y, z] = p.head_frame_now();
        close_to(x, Vec3::X, "the spindle's X");
        close_to(y, Vec3::new(0.0, 0.0, -up), "the spindle's Y");
        close_to(z, pose.axis, "the spindle's Z");
        // The bed under the bar is now on the side away from the tool.
        close_to(p.bed_point_now(Vec3::new(5.0, 0.0, -20.0)), Vec3::new(5.0, -20.0 * up, 0.0), "the bed");
        // The cut is on that side of the bar, and only there.
        assert!(!p.state().contains(Vec3::new(0.0, 9.0 * up, 0.0)), "cut under the tool");
        for q in [Vec3::new(0.0, 0.0, 9.0), Vec3::new(0.0, 0.0, -9.0), Vec3::new(0.0, -9.0 * up, 0.0)] {
            assert!(p.state().contains(q), "{q:?} is still there");
        }
    }
    // A 3-axis machine's spindle is square to the work.
    let p =
        player(&program("G1 Z-1. F100"), &Machine::three_axis(), &[flat(4.0)], &stock(), block(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)), None, 1.0e9);
    assert_eq!(p.head_frame_now(), [Vec3::X, Vec3::Y, Vec3::Z]);
}

#[test]
fn a_holder_on_a_bar_is_named_as_the_holder() {
    // No plain shank: the flutes end in the holder, whose bottom comes down into the bar.
    let held = Tool { holder: vec![HolderSection { diameter: 10.0, length: 10.0 }], ..Tool::new(1, ToolKind::FlatEndmill, 4.0, 5.0) };
    let bar = StockShape::Cylinder { x_min: -10.0, x_max: 10.0, radius: 10.0 };
    let text = "G21\nT1 M6\nS12000 M3\nG0 X0. Y0. Z30. A0.\nG1 Z4. F100\nG0 Z30.\n";
    let mut p = player(text, &Machine::four_axis_a(), std::slice::from_ref(&held), &bar, cylinder_x(2.0, -15.0, 15.0, 64), None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let codes: Vec<&str> = p.findings().events().iter().map(|e| e.code.as_str()).collect();
    assert!(codes.contains(&"holder-hits-stock"), "{codes:?}");
    assert!(!codes.contains(&"shank-hits-stock"), "{codes:?}");
    let holder = p.findings().events().iter().find(|e| e.code == "holder-hits-stock").map(|e| e.message.clone());
    assert!(holder.as_deref().is_some_and(|m| m.contains("10 mm")), "{holder:?}");
}

/// 3+1 in the view: a slot in the side of a square bar (A at 90°) whose last pass goes 0.3 mm
/// into the slot's wall. The rays from the A axis through that wall run beside the tool, inside
/// the material, so only the stock that holds air inside its rays sees the cut; the view finds
/// it at the move that cut it, about as deep as it is, and the simulation's report agrees.
#[test]
fn an_indexed_slot_cut_into_its_wall_is_found_in_the_view() {
    let tool = flat(6.0);
    let machine = Machine::four_axis_a();
    let frame = crate::indexed::frame_for_index(&machine, &[90f64.to_radians()]).unwrap();
    let part = Surface::new(slotted_bar(-20.0, 20.0).triangles().iter().map(|t| t.map(|p| frame.to_world(p))).collect()).unwrap();
    let stock = StockShape::Box { min: Vec3::new(-20.0, -20.0, -20.0), max: Vec3::new(20.0, 20.0, 20.0) };
    let mut text = String::from("G21\nT1 M6\nS12000 M3\nG0 X-25 Y-2 Z30 A90\n");
    for y in [-2.0, 0.0, 2.3] {
        text += &format!("G0 X-25 Y{y}\nG1 Z15.1 F300\nG1 X25 F600\nG0 Z30\n");
    }
    let wall_line = text.lines().position(|l| l == "G0 X-25 Y2.3").unwrap() + 3;
    let mut p = player(&text, &machine, std::slice::from_ref(&tool), &stock, part.clone(), None, 1.0e9);
    p.seek(usize::MAX, 0.0, None).unwrap();
    let gouges: Vec<_> = p.findings().events().iter().filter(|e| e.code == "gouge").collect();
    assert!(!gouges.is_empty(), "no gouge found: {:?}", p.findings().merged().list());
    assert!(
        gouges.iter().all(|g| g.line == wall_line),
        "{:?} (the pass into the wall is line {wall_line})",
        gouges.iter().map(|g| g.line).collect::<Vec<_>>()
    );
    let r = simulate(&text, &machine, std::slice::from_ref(&tool), &stock, &part, &SimParams { tolerance: 0.01, resolution: Some(0.1) }).unwrap();
    // Measured in 3D at the cells the rays sample, a little under the 0.3 the tool's side
    // reaches (0.27 in the view, 0.28 in the report).
    close(gouges[0].depth, 0.3, 0.05);
    close(r.gouge_worst, 0.3, 0.05);
}

/// The spindle head's body (the machine model's exact parts that move with the tool) is checked
/// against the part the way the holder is: a side plate low enough to reach a wall of the part
/// beside the cut is found at the move that brings it there, and nothing where it stays above.
#[test]
fn the_head_is_checked_against_the_part() {
    let model = plate_machine();
    let tool = Tool { stickout: Some(20.0), ..Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0) };
    // A wall of the part at X27, from Y0 to Y50, up to Z20.
    let a = |y: f64, z: f64| Vec3::new(27.0, y, z);
    let wall = Surface::new(vec![[a(0.0, -10.0), a(50.0, -10.0), a(50.0, 20.0)], [a(0.0, -10.0), a(50.0, 20.0), a(0.0, 20.0)]]).unwrap();
    let body = [crate::playback::HeadBody { surface: &wall, code: "part", noun: "the part" }];
    let plan_of = |text: &str| {
        plan_sim(text, &Machine::three_axis(), std::slice::from_ref(&tool), &stock(), &SimParams { tolerance: 0.01, resolution: Some(1.0e3) })
            .unwrap()
    };
    // Along Y at X5, Z1: the plate (X15 to X45, from 15 above the tip) passes over the wall's top.
    let plan = plan_of(&program("G0 X5. Y-20.\nG0 Z1.\nG1 Y60. F800\nG0 Z60."));
    let hits = crate::playback::head_check(&plan, std::slice::from_ref(&tool), &model, None, &body, &|_| 0);
    assert_eq!(hits.iter().map(|h| (h.first.code.as_str(), h.first.line, h.moves)).collect::<Vec<_>>(), [("side-plate-hits-part", 10, 1)]);
    assert_eq!(hits[0].first.severity, Severity::Error);
    assert!(hits[0].first.message.contains("the side plate runs into the part"), "{}", hits[0].first.message);
    // At Z6 the plate's bottom (21) clears the wall's top: nothing.
    let plan = plan_of(&program("G0 X5. Y-20.\nG0 Z6.\nG1 Y60. F800\nG0 Z60."));
    assert!(crate::playback::head_check(&plan, std::slice::from_ref(&tool), &model, None, &body, &|_| 0).is_empty());
    // No bodies: nothing to check.
    assert!(crate::playback::head_check(&plan, std::slice::from_ref(&tool), &model, None, &[], &|_| 0).is_empty());
}

/// On a part turned with A the head check passes over a move whose head stays farther from the
/// A axis than any of the part reaches, as it does over one above the part's top with the work
/// not turned (each tip taken one at a time was the 4th-axis suggestion's slowest check), and
/// still finds the head where it comes into the turned part.
#[test]
fn the_head_check_passes_over_moves_clear_of_a_turned_part() {
    let model = plate_machine();
    let tool = Tool { stickout: Some(20.0), ..Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0) };
    // A bar of radius 10 along X on the A axis: its box reaches 14.1 from the axis.
    let bar = cylinder_x(10.0, -50.0, 50.0, 48);
    let body = [crate::playback::HeadBody { surface: &bar, code: "part", noun: "the part" }];
    let check = |moves: &str| {
        let plan = plan_sim(
            &program(moves),
            &Machine::four_axis_a(),
            std::slice::from_ref(&tool),
            &StockShape::Cylinder { x_min: -50.0, x_max: 50.0, radius: 10.0 },
            &SimParams { tolerance: 0.01, resolution: Some(1.0e3) },
        )
        .unwrap();
        crate::playback::HEAD_TIPS.with(|n| n.set(0));
        let hits = crate::playback::head_check(&plan, std::slice::from_ref(&tool), &model, None, &body, &|_| 0);
        (hits.iter().map(|h| h.first.code.clone()).collect::<Vec<_>>(), crate::playback::HEAD_TIPS.with(|n| n.get()))
    };
    // The plate's bottom 15 above a tip at Z2 (17) is beyond the bar's reach turned any way.
    assert_eq!(
        check(
            "G0 X-20. Y0. A30.
G0 Z2.
G1 X20. A90. F800
G0 Z60."
        ),
        (vec![], 0)
    );
    // At Z-8 the plate's bottom (7) is in the bar however it is turned.
    let (codes, tips) = check(
        "G0 X-20. Y0. A30.
G0 Z-8.
G1 X20. A60. F800
G0 Z60.",
    );
    assert_eq!(codes, ["side-plate-hits-part"]);
    assert!(tips > 0);
}
