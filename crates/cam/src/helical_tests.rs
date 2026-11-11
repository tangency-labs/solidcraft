//! Tests of helical boring and thread milling: the helix's Z per turn, the leads inside the
//! hole, rings for wide holes, finishing and spring passes, the hand and direction of threads,
//! written and read back with their arcs, and hostile parameters refused without panics.

use std::f64::consts::TAU;

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::gcode5::{MachineOperation, MachineProgram, Post5, emit_program};
use crate::helical::*;
use crate::indexed::IndexedOperation;
use crate::kinematics::Machine;
use crate::readback5::readback_program;
use crate::*;

fn flat(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 20.0)
}

fn thread_tool(d: f64) -> Tool {
    Tool::new(5, ToolKind::ThreadMill, d, 12.0)
}

fn heights(top: f64, bottom: f64) -> Heights {
    Heights { clearance: 20.0, retract: 5.0, top, bottom }
}

fn feeds() -> Feeds {
    Feeds { cut: 600.0, plunge: 200.0 }
}

fn bore_params(pitch: f64) -> BoreParams {
    BoreParams {
        heights: heights(0.0, -6.0),
        feeds: feeds(),
        descent: Descent::Pitch { pitch },
        climb: true,
        stepover: 2.4,
        stock_to_leave: 0.0,
        finish_passes: 0,
    }
}

fn m6(internal: bool) -> ThreadParams {
    ThreadParams {
        heights: heights(0.0, -8.0),
        feeds: feeds(),
        pitch: 1.0,
        diameter: 6.0,
        internal,
        right_hand: true,
        climb: true,
        depth: None,
        radial_passes: 1,
        spring_pass: false,
    }
}

fn hole(x: f64, y: f64, d: f64) -> Hole {
    Hole { center: Vec2::new(x, y), diameter: d }
}

/// Every arc of `path` with the point it starts from.
fn arcs(path: &Toolpath) -> Vec<(Vec3, Vec3, Vec2, bool)> {
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        if let (Move::Arc { to, center, ccw, .. }, Some(from)) = (m, pos) {
            out.push((from, *to, *center, *ccw));
        }
        if let Some(t) = m.target() {
            pos = Some(t);
        }
    }
    out
}

/// The arcs round `c` (the helix and circles, not the leads), with their radius.
fn round(path: &Toolpath, c: Vec2) -> Vec<(Vec3, Vec3, f64, bool)> {
    arcs(path).into_iter().filter(|(_, _, center, _)| center.dist(c) < 1e-9).map(|(a, b, _, ccw)| (a, b, Vec2::new(a.x, a.y).dist(c), ccw)).collect()
}

/// The farthest the tool centre gets from `c` while cutting.
fn reach(path: &Toolpath, c: Vec2) -> f64 {
    path.points(1e-4).iter().filter(|(_, cut)| *cut).map(|(p, _)| Vec2::new(p.x, p.y).dist(c)).fold(0.0, f64::max)
}

#[test]
fn a_bore_goes_down_exactly_its_pitch_each_turn_and_ends_with_a_flat_circle() {
    let tool = flat(6.0);
    let out = bore(&[hole(10.0, 5.0, 10.0)], &tool, &bore_params(0.7)).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let c = Vec2::new(10.0, 5.0);
    let helix = round(&out.path, c);
    // Every half turn goes down half the pitch, to the last; the radius is the hole's less the tool's.
    let steps: Vec<f64> = helix.iter().map(|(a, b, _, _)| a.z - b.z).collect();
    let (descent, flat_turn) = steps.split_at(steps.len() - 2);
    for s in &descent[..descent.len() - 1] {
        assert!((s - 0.35).abs() < 1e-12, "{steps:?}");
    }
    assert!((descent.iter().sum::<f64>() - 6.0).abs() < 1e-9 && flat_turn.iter().all(|s| s.abs() < 1e-12), "{steps:?}");
    assert!(helix.iter().all(|(_, _, rho, ccw)| (rho - 2.0).abs() < 1e-9 && *ccw), "climb goes counter-clockwise: {helix:?}");
    // The last moves at the bottom are a full circle back where the descent ended.
    let n = helix.len();
    assert!(helix[n - 1].1.dist(helix[n - 2].0) < 1e-9 && (helix[n - 1].1.z + 6.0).abs() < 1e-12);
    // In from the centre, out to it, never past the wall, never below the floor.
    let leads: Vec<_> = arcs(&out.path).into_iter().filter(|(_, _, center, _)| center.dist(c) > 1e-6).collect();
    assert_eq!(leads.len(), 2, "{leads:?}");
    assert!(Vec2::new(leads[0].0.x, leads[0].0.y).dist(c) < 1e-9 && Vec2::new(leads[1].1.x, leads[1].1.y).dist(c) < 1e-9);
    assert!(reach(&out.path, c) <= 2.0 + 1e-9);
    assert_eq!(out.path.stats().lowest_z, Some(-6.0));
    // Conventional milling goes the other way.
    let conv = bore(&[hole(10.0, 5.0, 10.0)], &tool, &BoreParams { climb: false, ..bore_params(0.7) }).unwrap();
    assert!(round(&conv.path, c).iter().all(|(.., ccw)| !*ccw));
}

#[test]
fn a_ramp_angle_sets_the_pitch_by_the_radius_and_is_capped() {
    let tool = flat(6.0);
    let angle = 2f64.to_radians();
    let p = BoreParams { descent: Descent::Ramp { angle, max_pitch: 10.0 }, ..bore_params(1.0) };
    let out = bore(&[hole(0.0, 0.0, 10.0)], &tool, &p).unwrap();
    let first = round(&out.path, Vec2::ZERO)[0];
    assert!((first.0.z - first.1.z - TAU * 2.0 * angle.tan() * 0.5).abs() < 1e-12, "{first:?}");
    let capped = bore(&[hole(0.0, 0.0, 10.0)], &tool, &BoreParams { descent: Descent::Ramp { angle, max_pitch: 0.2 }, ..p }).unwrap();
    let first = round(&capped.path, Vec2::ZERO)[0];
    assert!((first.0.z - first.1.z - 0.1).abs() < 1e-12, "{first:?}");
}

#[test]
fn a_hole_wider_than_two_tool_diameters_is_opened_in_rings_from_the_centre() {
    let tool = flat(6.0);
    let out = bore(&[hole(0.0, 0.0, 30.0)], &tool, &bore_params(1.0)).unwrap();
    let mut radii: Vec<f64> = round(&out.path, Vec2::ZERO).iter().map(|(.., rho, _)| *rho).collect();
    radii.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert!(radii.len() >= 4, "{radii:?}");
    assert!(radii[0] < 3.0, "the first ring cuts the centre: {radii:?}");
    assert!(radii.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] <= 2.4 + 1e-9), "{radii:?}");
    assert!((radii[radii.len() - 1] - 12.0).abs() < 1e-9 && reach(&out.path, Vec2::ZERO) <= 12.0 + 1e-9);
    // Two tool diameters or less: one ring.
    let one = bore(&[hole(0.0, 0.0, 12.0)], &tool, &bore_params(1.0)).unwrap();
    let mut radii: Vec<f64> = round(&one.path, Vec2::ZERO).iter().map(|(.., rho, _)| *rho).collect();
    radii.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert_eq!(radii.len(), 1);
}

#[test]
fn finishing_passes_go_round_at_the_full_diameter_after_roughing_leaves_stock() {
    let tool = flat(6.0);
    let p = BoreParams { stock_to_leave: 0.3, finish_passes: 2, ..bore_params(1.0) };
    let out = bore(&[hole(0.0, 0.0, 10.0)], &tool, &p).unwrap();
    let mut radii: Vec<f64> = round(&out.path, Vec2::ZERO).iter().map(|(.., rho, _)| *rho).collect();
    radii.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert!((radii[0] - 1.7).abs() < 1e-9 && (radii[1] - 2.0).abs() < 1e-9 && radii.len() == 2, "{radii:?}");
    // Three helices down: roughing and the two finishing passes.
    let bottoms = round(&out.path, Vec2::ZERO).iter().filter(|(a, b, ..)| a.z > -6.0 + 1e-9 && (b.z + 6.0).abs() < 1e-12).count();
    assert_eq!(bottoms, 3);
    verify(&out.path, &p.heights).unwrap();
}

#[test]
fn an_undersize_hole_is_refused_by_name_and_a_tight_one_warned_of() {
    let tool = flat(6.0);
    let e = bore(&[hole(0.0, 0.0, 10.0), hole(20.0, 0.0, 6.0)], &tool, &bore_params(0.5)).unwrap_err();
    let msg = e.to_string();
    assert!(msg.contains("hole 2 at (20, 0)") && msg.contains("too small"), "{msg}");
    assert!(bore(&[hole(0.0, 0.0, 4.0)], &tool, &bore_params(0.5)).is_err());
    let out = bore(&[hole(0.0, 0.0, 7.0), hole(20.0, 0.0, 10.0)], &tool, &bore_params(0.5)).unwrap();
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    assert!(out.warnings[0].contains("hole 1") && out.warnings[0].contains("chips"), "{:?}", out.warnings);
    // Stock that leaves the tool no room is refused too.
    let e = bore(&[hole(0.0, 0.0, 6.2)], &tool, &BoreParams { stock_to_leave: 0.2, finish_passes: 1, ..bore_params(0.5) }).unwrap_err();
    assert!(e.to_string().contains("stock"), "{e}");
}

#[test]
fn several_holes_are_linked_above_the_retract_height() {
    let tool = flat(4.0);
    let holes = [hole(0.0, 0.0, 8.0), hole(30.0, 0.0, 6.0), hole(30.0, 20.0, 8.0)];
    let p = bore_params(0.5);
    let out = bore(&holes, &tool, &p).unwrap();
    verify(&out.path, &p.heights).unwrap();
    for h in &holes {
        let rho = h.diameter * 0.5 - 2.0;
        assert!(round(&out.path, h.center).iter().all(|(.., r, _)| (r - rho).abs() < 1e-9));
    }
    assert!(matches!(out.path.moves.last(), Some(Move::Rapid { to }) if (to.z - 20.0).abs() < 1e-12));
}

#[test]
fn an_m6_internal_thread_rises_exactly_one_pitch_a_turn_counter_clockwise() {
    let tool = thread_tool(4.5);
    let p = m6(true);
    let out = thread_mill(&[Vec2::new(5.0, 5.0)], &tool, &p).unwrap();
    verify(&out.path, &p.heights).unwrap();
    let c = Vec2::new(5.0, 5.0);
    let helix = round(&out.path, c);
    // The leads rise a quarter pitch each, so the helix runs from -7.75 to -0.25.
    assert_eq!(helix.len(), 15, "seven and a half turns of half-turn arcs: {helix:?}");
    for (k, (a, b, rho, ccw)) in helix.iter().enumerate() {
        assert!(*ccw, "a right-hand internal thread climb milled goes counter-clockwise");
        assert!((b.z - a.z - 0.5).abs() < 1e-12, "half a pitch up each half turn");
        assert!((b.z - (-7.75 + 0.5 * (k + 1) as f64)).abs() < 1e-12);
        assert!((rho - 0.75).abs() < 1e-9, "the tip at the major diameter: {rho}");
    }
    // The core inside the teeth reaches the minor diameter, no farther.
    let check = thread_check_tool(&tool, &p).unwrap();
    let core = check.cutter().unwrap();
    assert!((core.radius + reach(&out.path, c) - (3.0 - p.thread_depth())).abs() < 1e-9, "{core:?}");
    assert!((p.thread_depth() - 0.5413).abs() < 1e-4);
    // Left-hand goes down, conventional reverses the turn as well.
    let lh = thread_mill(&[c], &tool, &ThreadParams { right_hand: false, ..p }).unwrap();
    assert!(round(&lh.path, c).iter().all(|(a, b, _, ccw)| *ccw && b.z < a.z));
    let conv = thread_mill(&[c], &tool, &ThreadParams { climb: false, ..p }).unwrap();
    assert!(round(&conv.path, c).iter().all(|(a, b, _, ccw)| !*ccw && b.z < a.z));
}

#[test]
fn an_external_thread_comes_in_from_outside_and_goes_down_clockwise() {
    let tool = thread_tool(8.0);
    let p = ThreadParams { radial_passes: 3, spring_pass: true, ..m6(false) };
    let out = thread_mill(&[Vec2::ZERO], &tool, &p).unwrap();
    verify(&out.path, &p.heights).unwrap();
    let helix = round(&out.path, Vec2::ZERO);
    assert!(helix.iter().all(|(a, b, _, ccw)| !*ccw && (a.z - b.z - 0.5).abs() < 1e-12), "{helix:?}");
    let mut radii: Vec<f64> = helix.iter().map(|(.., rho, _)| *rho).collect();
    radii.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let depth = p.thread_depth();
    assert_eq!(radii.len(), 3, "the spring pass repeats the last radius: {radii:?}");
    assert!((radii[2] - (3.0 - depth + 4.0)).abs() < 1e-9 && radii[0] > radii[1], "{radii:?}");
    assert_eq!(helix.len(), 4 * 16);
    // Every move outside the thread's circle: the tool's tip never inside the minor diameter.
    let inner = path_min_reach(&out.path);
    assert!(inner >= 3.0 - depth + 4.0 - 1e-9, "{inner}");
}

fn path_min_reach(path: &Toolpath) -> f64 {
    path.points(1e-4).iter().map(|(p, _)| Vec2::new(p.x, p.y).len()).fold(f64::INFINITY, f64::min)
}

#[test]
fn a_multi_form_thread_mill_turns_once_per_span_of_its_teeth() {
    let mut tool = thread_tool(4.5);
    tool.thread_pitch = Some(1.0);
    tool.flute_length = 4.0;
    let p = m6(true);
    let out = thread_mill(&[Vec2::ZERO], &tool, &p).unwrap();
    let helix = round(&out.path, Vec2::ZERO);
    // 8 mm of thread (7.5 at full depth, inside the leads), 4 rows of teeth: two turns, from
    // -7.75 and from -3.75.
    assert_eq!(helix.len(), 4, "{helix:?}");
    assert!((helix[0].0.z + 7.75).abs() < 1e-12 && (helix[2].0.z + 3.75).abs() < 1e-12, "{helix:?}");
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    // Teeth longer than the thread are warned of: they cut thread beyond its top.
    let long = Tool { flute_length: 12.0, ..tool.clone() };
    let warned = thread_mill(&[Vec2::ZERO], &long, &p).unwrap();
    assert!(warned.warnings.len() == 1 && warned.warnings[0].contains("beyond its top"), "{:?}", warned.warnings);
    assert!(helix.iter().all(|(a, b, ..)| (b.z - a.z - 0.5).abs() < 1e-12));
    tool.thread_pitch = Some(1.25);
    assert!(thread_mill(&[Vec2::ZERO], &tool, &p).unwrap_err().to_string().contains("1.25"));
}

#[test]
fn a_thread_mill_too_big_for_the_minor_diameter_or_a_wrong_tool_is_refused() {
    let p = m6(true);
    let e = thread_mill(&[Vec2::ZERO], &thread_tool(5.0), &p).unwrap_err();
    assert!(e.to_string().contains("minor diameter"), "{e}");
    assert!(thread_mill(&[Vec2::ZERO], &flat(4.0), &p).unwrap_err().to_string().contains("thread mill"));
    let mut short = thread_tool(4.5);
    short.tooth_height = Some(0.3);
    assert!(thread_mill(&[Vec2::ZERO], &short, &p).unwrap_err().to_string().contains("shorter"));
    // A thread mill does not bore, pocket or finish surfaces.
    assert!(bore(&[hole(0.0, 0.0, 10.0)], &thread_tool(4.5), &bore_params(0.5)).is_err());
    let square =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]), holes: vec![] };
    let cut = Cut { heights: heights(0.0, -1.0), feeds: feeds(), stepdown: 1.0 };
    let side = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(contour(&[square], &thread_tool(4.5), &side, &cut).unwrap_err().to_string().contains("thread mill"));
}

#[test]
fn a_thread_mill_is_a_tool_kind_with_its_pitch_and_tooth_height() {
    let mut t: Tool = serde_json::from_value(
        serde_json::json!({"number": 7, "kind": "thread_mill", "diameter": 4.5, "flute_length": 10, "thread_pitch": 1.0, "tooth_height": 0.6}),
    )
    .unwrap();
    t.validate().unwrap();
    assert_eq!(t.kind, ToolKind::ThreadMill);
    assert_eq!(t.kind.label(), "thread mill");
    assert!((t.cutter().unwrap().radius - 2.25).abs() < 1e-12, "checked whole, its teeth and all");
    assert_eq!(serde_json::to_value(&t).unwrap()["kind"], "thread_mill");
    t.tooth_height = Some(2.25);
    assert!(t.validate().is_err(), "teeth as deep as the radius leave no core");
    t.tooth_height = None;
    t.thread_pitch = Some(11.0);
    assert!(t.validate().is_err(), "rows of teeth farther apart than the flutes are long");
    let mut flat_with_pitch = flat(6.0);
    flat_with_pitch.thread_pitch = Some(1.0);
    assert!(flat_with_pitch.validate().is_err());
}

#[test]
fn a_bore_and_a_thread_are_written_with_helical_arcs_and_read_back() {
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    let bored = bore(&[hole(10.0, 0.0, 10.0)], &flat(6.0), &bore_params(0.5)).unwrap();
    let threaded = thread_mill(&[Vec2::new(-10.0, 0.0)], &thread_tool(4.5), &m6(true)).unwrap();
    let op = |name: &str, tool: Tool, path: Toolpath| {
        MachineOperation::Indexed(IndexedOperation { name: name.into(), tool, spindle_rpm: 18000.0, coolant: false, rotary: vec![0.0], path })
    };
    let program = MachineProgram {
        name: "holes".into(),
        operations: vec![op("bore", flat(6.0), bored.path), op("thread", thread_tool(4.5), threaded.path)],
        work_offset: None,
    };
    let text = emit_program(&program, &post, &machine).unwrap();
    let rb = readback_program(&text, &program, &post, &machine).unwrap();
    assert!(rb.within(), "{rb:?}");
    let word = |l: &str, c: char| l.split_whitespace().any(|w| w.starts_with(c));
    let helical = text.lines().filter(|l| !l.starts_with('(') && word(l, 'Z') && word(l, 'I')).count();
    assert!(helical > 20, "{text}");
}

#[test]
fn hostile_bores_and_threads_are_refused_without_panics() {
    let tool = flat(6.0);
    let h = hole(0.0, 0.0, 10.0);
    let nan = f64::NAN;
    let bores = [
        (vec![], bore_params(0.5)),
        (vec![hole(nan, 0.0, 10.0)], bore_params(0.5)),
        (vec![hole(1e300, 0.0, 10.0)], bore_params(0.5)),
        (vec![hole(0.0, 0.0, nan)], bore_params(0.5)),
        (vec![hole(0.0, 0.0, f64::INFINITY)], bore_params(0.5)),
        (vec![hole(0.0, 0.0, -10.0)], bore_params(0.5)),
        (vec![h], bore_params(0.0)),
        (vec![h], bore_params(nan)),
        (vec![h], bore_params(1e-9)),
        (vec![h], bore_params(1e9)),
        (vec![h], BoreParams { descent: Descent::Ramp { angle: 0.0, max_pitch: 1.0 }, ..bore_params(0.5) }),
        (vec![h], BoreParams { descent: Descent::Ramp { angle: 1.6, max_pitch: 1.0 }, ..bore_params(0.5) }),
        (vec![h], BoreParams { descent: Descent::Ramp { angle: 0.01, max_pitch: nan }, ..bore_params(0.5) }),
        (vec![h], BoreParams { stepover: 0.0, ..bore_params(0.5) }),
        (vec![h], BoreParams { stepover: 7.0, ..bore_params(0.5) }),
        (vec![h], BoreParams { stock_to_leave: -1.0, ..bore_params(0.5) }),
        (vec![h], BoreParams { stock_to_leave: nan, ..bore_params(0.5) }),
        (vec![h], BoreParams { finish_passes: 1000, ..bore_params(0.5) }),
        (vec![h], BoreParams { heights: heights(0.0, 0.0), ..bore_params(0.5) }),
        (vec![h], BoreParams { heights: Heights { clearance: nan, ..heights(0.0, -6.0) }, ..bore_params(0.5) }),
        (vec![h], BoreParams { heights: heights(0.0, -1e5), ..bore_params(0.001) }),
        (vec![h], BoreParams { feeds: Feeds { cut: 0.0, plunge: 1.0 }, ..bore_params(0.5) }),
        (vec![hole(0.0, 0.0, 1e7)], BoreParams { stepover: 0.01, ..bore_params(0.5) }),
        (vec![h; MAX_HOLES + 1], bore_params(0.5)),
    ];
    for (holes, p) in &bores {
        assert!(bore(holes, &tool, p).is_err(), "{holes:?} {p:?}");
    }
    let tm = thread_tool(4.5);
    let threads = [
        (vec![], m6(true)),
        (vec![Vec2::new(nan, 0.0)], m6(true)),
        (vec![Vec2::ZERO], ThreadParams { pitch: 0.0, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { pitch: nan, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { pitch: 1e9, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { pitch: 0.01, heights: heights(0.0, -1e5), ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { diameter: nan, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { diameter: -6.0, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { diameter: 2.0, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { depth: Some(0.0), ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { depth: Some(nan), ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { depth: Some(3.0), ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { depth: Some(2.5), diameter: 60.0, ..m6(false) }),
        (vec![Vec2::ZERO], ThreadParams { radial_passes: 0, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { radial_passes: 1000, ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { heights: heights(0.0, 0.0), ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { heights: heights(0.0, -0.4), ..m6(true) }),
        (vec![Vec2::ZERO], ThreadParams { feeds: Feeds { cut: nan, plunge: 1.0 }, ..m6(true) }),
    ];
    for (centers, p) in &threads {
        assert!(thread_mill(centers, &tm, p).is_err(), "{centers:?} {p:?}");
    }
}

/// Every arc of `path` with the point it starts from and its feed.
fn arc_feeds(path: &Toolpath) -> Vec<(Vec3, Vec3, Vec2, f64)> {
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        if let (Move::Arc { to, center, feed, .. }, Some(from)) = (m, pos) {
            out.push((from, *to, *center, *feed));
        }
        if let Some(t) = m.target() {
            pos = Some(t);
        }
    }
    out
}

/// The fastest any arc of `path` goes down (mm/min): its feed times the share of its length
/// that is Z.
fn fastest_descent(path: &Toolpath) -> f64 {
    arc_feeds(path)
        .iter()
        .filter(|(a, b, ..)| b.z < a.z)
        .map(|(a, b, c, feed)| {
            let (u, v) = (Vec2::new(a.x, a.y) - *c, Vec2::new(b.x, b.y) - *c);
            // Every arc here is at most a half turn, so the angle between its ends is its sweep.
            let sweep = (u.x * v.y - u.y * v.x).atan2(u.x * v.x + u.y * v.y).abs();
            let len = ((sweep * u.len()).powi(2) + (a.z - b.z).powi(2)).sqrt();
            feed * (a.z - b.z) / len
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_bores_helix_goes_down_no_faster_than_the_plunge_feed() {
    let tool = flat(6.0);
    // A steep helix close round the axis is nearly a plunge: at the cutting feed it would go
    // down almost three times faster than the plunge feed allows.
    let steep = bore(&[hole(0.0, 0.0, 6.2)], &tool, &bore_params(3.0)).unwrap();
    let fastest = fastest_descent(&steep.path);
    assert!(fastest <= feeds().plunge + 1e-6, "goes down at {fastest} mm/min");
    // So does the steepest ramp angle, and a gentle helix keeps the cutting feed.
    let ramp = BoreParams { descent: Descent::Ramp { angle: 45f64.to_radians(), max_pitch: 20.0 }, ..bore_params(1.0) };
    let ramped = bore(&[hole(0.0, 0.0, 10.0)], &tool, &ramp).unwrap();
    assert!(fastest_descent(&ramped.path) <= feeds().plunge + 1e-6);
    let gentle = bore(&[hole(0.0, 0.0, 10.0)], &tool, &bore_params(0.5)).unwrap();
    assert!(arc_feeds(&gentle.path).iter().all(|(.., f)| (*f - feeds().cut).abs() < 1e-9));
}

/// How far (in turns) the cutting points of `path` farther than `near` from `c` are off the
/// thread's helix through `(angle 0, z0)`, rising counter-clockwise for a right-hand thread.
fn off_helix(path: &Toolpath, c: Vec2, z0: f64, pitch: f64, right_hand: bool, near: f64) -> f64 {
    let s = if right_hand { 1.0 } else { -1.0 };
    path.points(1e-4)
        .iter()
        .filter(|(p, cut)| *cut && Vec2::new(p.x, p.y).dist(c) > near)
        .map(|(p, _)| {
            let d = Vec2::new(p.x, p.y) - c;
            let turns = (p.z - z0) / pitch - s * d.y.atan2(d.x) / TAU;
            (turns - turns.round()).abs()
        })
        .fold(0.0, f64::max)
}

#[test]
fn an_internal_threads_leads_follow_its_helix() {
    // The teeth meet the wall wherever the tool centre is farther from the axis than the
    // minor diameter less the tool: in the leads as well as on the helix. There they must cut
    // the same helix, or the leads in and out notch the thread's flanks.
    let c = Vec2::new(5.0, 5.0);
    let p = m6(true);
    let tool = thread_tool(4.5);
    let first = 3.0 - p.thread_depth() - 2.25;
    for params in [p, ThreadParams { right_hand: false, ..p }, ThreadParams { climb: false, ..p }, ThreadParams { radial_passes: 3, ..p }] {
        let out = thread_mill(&[c], &tool, &params).unwrap();
        verify(&out.path, &params.heights).unwrap();
        let z0 = round(&out.path, c)[0].0.z;
        let off = off_helix(&out.path, c, z0, 1.0, params.right_hand, first);
        assert!(off < 1e-6, "{off} turns off the helix: {params:?}");
        assert!(out.path.stats().lowest_z.is_some_and(|z| z >= -8.0 - 1e-12));
    }
    // A multi-form tool's turns join on the same helix.
    let mut multi = thread_tool(4.5);
    multi.thread_pitch = Some(1.0);
    multi.flute_length = 3.0;
    let out = thread_mill(&[c], &multi, &p).unwrap();
    let z0 = round(&out.path, c)[0].0.z;
    assert!(off_helix(&out.path, c, z0, 1.0, true, first) < 1e-6);
}

#[test]
fn a_multi_form_thread_mills_teeth_stop_within_a_pitch_of_the_top() {
    // 5 mm of thread with 4 rows of teeth: the second turn must not start a whole span above
    // the first, or its upper teeth cut 3 mm of thread above the top.
    let mut tool = thread_tool(4.5);
    tool.thread_pitch = Some(1.0);
    tool.flute_length = 4.0;
    let p = ThreadParams { heights: heights(0.0, -5.0), ..m6(true) };
    let out = thread_mill(&[Vec2::ZERO], &tool, &p).unwrap();
    let helix = round(&out.path, Vec2::ZERO);
    let highest_tip = helix.iter().map(|(a, b, ..)| a.z.max(b.z)).fold(f64::NEG_INFINITY, f64::max);
    // The top row of teeth stands three pitches above the tip.
    assert!(highest_tip + 3.0 <= 1.0 + 1e-9, "the teeth reach {} mm", highest_tip + 3.0);
    // And every pitch of the thread from the bottom up is still cut: the turns meet or overlap.
    let mut starts: Vec<f64> = helix.iter().filter(|(a, ..)| a.x > 0.0 && a.y.abs() < 1e-9).map(|(a, ..)| a.z).collect();
    starts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert!(starts.len() >= 2 && starts.windows(2).all(|w| w[1] - w[0] <= 4.0 + 1e-9 && w[1] > w[0]), "{starts:?}");
}

#[test]
fn a_thread_mill_is_checked_whole_outside_thread_milling() {
    // Only thread milling may take a thread mill as the core inside its teeth; anything else
    // that checks with `cutter` (the 5-axis strategies, a wrapped path) must see its teeth.
    let mut t = thread_tool(4.5);
    t.tooth_height = Some(0.6);
    t.validate().unwrap();
    assert!((t.cutter().unwrap().radius - 2.25).abs() < 1e-12);
    let p = m6(true);
    let check = thread_check_tool(&t, &p).unwrap();
    check.validate().unwrap();
    assert!((check.cutter().unwrap().radius - (2.25 - p.thread_depth())).abs() < 1e-12);
    assert_eq!(check.number, t.number);
    // The shank and holder above the flutes are the thread mill's.
    assert_eq!(check.envelopes(), t.envelopes());
    let shanked = Tool { shank_diameter: Some(4.0), stickout: Some(20.0), ..t.clone() };
    assert_eq!(thread_check_tool(&shanked, &p).unwrap().envelopes(), shanked.envelopes());
}

/// A bore always goes down along a helix, so a face mill may bore; a chamfer mill's tapered
/// side would leave the wall uncut and is refused.
#[test]
fn a_face_mill_bores_and_a_chamfer_mill_is_refused() {
    let mut face = Tool::new(1, ToolKind::FaceMill, 10.0, 6.0);
    face.corner_radius = Some(0.8);
    assert!(bore(&[hole(0.0, 0.0, 20.0)], &face, &bore_params(1.0)).is_ok());
    let mut chamfer = Tool::new(7, ToolKind::Chamfer, 10.0, 10.0);
    chamfer.angle_deg = Some(90.0);
    let e = bore(&[hole(0.0, 0.0, 20.0)], &chamfer, &bore_params(1.0)).unwrap_err();
    assert!(e.to_string().contains("a chamfer mill's side is tapered"), "{e}");
}
