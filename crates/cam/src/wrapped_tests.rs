//! Tests of pockets and contours wrapped onto a cylinder: floors at the exact radius, walls
//! offset at each level's own radius, a slot measured in the written G-code, hostile input.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::foundation_tests::close;
use crate::gcode5::{JointWords, MachineOperation, MachineProgram, Post5, emit_program, parse5};
use crate::kinematics::{AxisLetter, Machine};
use crate::wrapped::{MAX_WRAP_LEVELS, WrapContourParams, WrapCut, WrapPocketParams, wrap_contour, wrap_pocket};
use crate::*;

const R: f64 = 10.0;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    Region2 { outer: Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]), holes: Vec::new() }
}

fn cut(bottom: f64, stepdown: f64) -> WrapCut {
    WrapCut {
        radius: R,
        top: 0.0,
        bottom,
        stepdown,
        clearance: 5.0,
        retract: 1.0,
        feeds: Feeds { cut: 600.0, plunge: 150.0 },
        tolerance: 0.01,
        max_angle_step: DEFAULT_MAX_ANGLE_STEP,
    }
}

fn pocket_params(bottom: f64, stepdown: f64) -> WrapPocketParams {
    WrapPocketParams { cut: cut(bottom, stepdown), stepover: 1.5, climb: true, stock_to_leave: 0.0, ramp_angle: 0.0 }
}

fn flat(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 15.0)
}

/// Distance from the X axis.
fn radius(p: Vec3) -> f64 {
    p.y.hypot(p.z)
}

/// The tips of the cutting moves (not the plunges), as `(x, θ, ρ)`.
fn cuts(path: &Toolpath5, feed: f64) -> Vec<(f64, f64, f64)> {
    path.moves
        .iter()
        .filter_map(|m| match *m {
            Move5::Feed { to, feed: f } if f == feed => Some((to.tip.x, to.tip.y.atan2(to.tip.z), radius(to.tip))),
            _ => None,
        })
        .collect()
}

/// Every pose's tool axis points straight out from the rotary axis through its tip.
fn radial(path: &Toolpath5) {
    for p in path.poses() {
        let r = radius(p.tip);
        assert!(p.axis.x.abs() <= 1e-12 && (p.axis.y - p.tip.y / r).abs() <= 1e-9 && (p.axis.z - p.tip.z / r).abs() <= 1e-9, "{p:?}");
    }
}

#[test]
fn a_wrapped_pocket_floor_is_at_the_exact_radius() {
    // 10 mm along X, 15 mm of arc round a cylinder of radius 10, 2 mm deep in 1 mm levels.
    let path = wrap_pocket(&[rect(-5.0, 0.0, 5.0, 15.0)], &flat(4.0), &pocket_params(-2.0, 1.0)).unwrap();
    verify5(&path, DEFAULT_MAX_ANGLE_STEP).unwrap();
    radial(&path);
    assert!(path.stats().max_axis_step <= DEFAULT_MAX_ANGLE_STEP + 1e-12);
    let c = cuts(&path, 600.0);
    let lowest = c.iter().map(|t| t.2).fold(f64::INFINITY, f64::min);
    close(lowest, 8.0, 1e-9);
    // Every cutting tip is on one of the two levels, exactly.
    for &(_, _, rho) in &c {
        assert!((rho - 9.0).abs() <= 1e-9 || (rho - 8.0).abs() <= 1e-9, "a tip at radius {rho}");
    }
    // Nothing anywhere comes inside the floor: plunges stop on it.
    assert!(path.poses().all(|p| radius(p.tip) >= 8.0 - 1e-9));
    // The rapids go round at radius 15 and never inside it.
    let tips: Vec<Vec3> = path.poses().map(|p| p.tip).collect();
    assert!(tips.iter().any(|t| (radius(*t) - 15.0).abs() <= 1e-9));
}

/// The levels below the first are entered from the top of the material, not by a rapid down
/// to the level above: that is cleared only where its own cuts ran, and a level's first cut
/// (or one after a hop) may start beyond them, in the stock. Every rapid stays out at the top.
#[test]
fn no_wrapped_rapid_comes_below_the_top_of_the_material() {
    // Two pockets side by side along X (a hop between them at every level), 3 mm deep in
    // 0.75 mm levels.
    let regions = [rect(-12.0, 0.0, -4.0, 12.0), rect(4.0, 0.0, 12.0, 12.0)];
    let params = pocket_params(-3.0, 0.75);
    let contour = WrapContourParams { cut: params.cut, side: Side::Inside, climb: true, stock_to_leave: 0.0, ramp_angle: 0.0 };
    for path in [wrap_pocket(&regions, &flat(4.0), &params).unwrap(), wrap_contour(&regions, &flat(4.0), &contour).unwrap()] {
        let rapids: Vec<f64> = path.moves.iter().filter_map(|m| if let Move5::Rapid { to } = m { Some(radius(to.tip)) } else { None }).collect();
        assert!(rapids.len() >= 8, "{} rapids", rapids.len());
        let lowest = rapids.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(lowest >= R - 1e-9, "a rapid down to radius {lowest}, inside the uncut cylinder of radius {R}");
        // Every level is still cut, down to the floor.
        let deepest = cuts(&path, 600.0).iter().map(|t| t.2).fold(f64::INFINITY, f64::min);
        close(deepest, R - 3.0, 1e-9);
    }
}

#[test]
fn each_level_is_offset_at_its_own_radius() {
    let r = 2.0;
    let len = 15.0;
    let path = wrap_pocket(&[rect(-5.0, 0.0, 5.0, len)], &flat(2.0 * r), &pocket_params(-3.0, 1.5)).unwrap();
    let c = cuts(&path, 600.0);
    for rho in [8.5, 7.0] {
        let at: Vec<&(f64, f64, f64)> = c.iter().filter(|t| (t.2 - rho).abs() <= 1e-9).collect();
        assert!(!at.is_empty(), "no cut at radius {rho}");
        let (lo, hi) = at.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t.1), hi.max(t.1)));
        // The walls at θ = 0 and θ = 15/R: the centre is r of arc length in at this radius.
        close(lo * rho, r, 0.01);
        close((len / R - hi) * rho, r, 0.01);
        let (xlo, xhi) = at.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t.0), hi.max(t.0)));
        close(xlo, -5.0 + r, 1e-9);
        close(xhi, 5.0 - r, 1e-9);
    }
    // Offsetting once at the cylinder would put the deepest centres r/R round instead of r/ρ:
    // further out than the wall allows by r·(1/7 − 1/10)·7 = 0.6 mm. They are not.
    let deepest = c.iter().filter(|t| (t.2 - 7.0).abs() <= 1e-9).map(|t| t.1).fold(f64::INFINITY, f64::min);
    assert!(deepest * 7.0 > r - 0.01, "{deepest}");
}

/// The slot as the controller runs it: X, the radius (Z, the table having turned the tip under
/// the tool) and A, unwrapped to arc lengths at the radius it was cut at.
#[test]
fn a_wrapped_slot_measures_its_width_in_the_written_program() {
    let w = 6.0;
    let d = 4.0;
    // A slot w wide along X, a quarter turn round, 1.5 mm deep.
    let quarter = FRAC_PI_2 * R;
    let p = pocket_params(-1.5, 0.75);
    let path = wrap_pocket(&[rect(-w / 2.0, 0.0, w / 2.0, quarter)], &flat(d), &p).unwrap();
    let machine = Machine::four_axis_a();
    let post = Post5::linuxcnc_4axis();
    let op = Operation5 { name: "slot".into(), tool: flat(d), spindle_rpm: 12000.0, coolant: false, path, plunge_feed: None };
    let program = MachineProgram { name: "slot".into(), operations: vec![MachineOperation::Continuous(op)], work_offset: None };
    let text = emit_program(&program, &post, &machine).unwrap();
    assert!(text.contains("G93"), "cuts that turn A are in inverse time");
    let blocks = parse5(&text, &[AxisLetter::A]).unwrap();
    // The cut tips: (x, a, z) of every feed at the floor.
    let floor = R - 1.5;
    let mut xs = (f64::INFINITY, f64::NEG_INFINITY);
    let mut arcs = (f64::INFINITY, f64::NEG_INFINITY);
    let mut lowest = f64::INFINITY;
    for b in &blocks {
        if let JointWords::Feed { linear, rotary, .. } = b {
            let a = rotary.first().copied().unwrap();
            // The tool is vertical over the axis: the tip is at Y 0, Z the radius.
            assert!(linear.y.abs() <= 1e-3, "{linear:?}");
            lowest = lowest.min(linear.z);
            if (linear.z - floor).abs() <= 2e-3 {
                xs = (xs.0.min(linear.x), xs.1.max(linear.x));
                arcs = (arcs.0.min(a * linear.z), arcs.1.max(a * linear.z));
            }
        }
    }
    close(lowest, floor, 1e-3);
    // Across X the slot is the tool's width plus the centres' travel.
    close(xs.1 - xs.0 + d, w, 2e-3);
    // Round, at the floor radius, the quarter turn's arc length there.
    close(arcs.1 - arcs.0 + d, FRAC_PI_2 * floor, 0.02);
}

#[test]
fn a_wrapped_contour_follows_the_slot_wall_at_every_level() {
    let r = 1.5;
    let p = WrapContourParams { cut: cut(-2.0, 1.0), side: Side::Inside, climb: true, stock_to_leave: 0.0, ramp_angle: 0.0 };
    let path = wrap_contour(&[rect(0.0, -6.0, 20.0, 6.0)], &flat(2.0 * r), &p).unwrap();
    verify5(&path, DEFAULT_MAX_ANGLE_STEP).unwrap();
    radial(&path);
    let c = cuts(&path, 600.0);
    for rho in [9.0, 8.0] {
        let at: Vec<&(f64, f64, f64)> = c.iter().filter(|t| (t.2 - rho).abs() <= 1e-9).collect();
        let (lo, hi) = at.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t.1), hi.max(t.1)));
        // The walls at θ = ±0.6 rad; the centres r of arc length inside them at this radius.
        close((0.6 + lo) * rho, r, 0.01);
        close((0.6 - hi) * rho, r, 0.01);
    }
    // An outside contour runs round the profile instead.
    let out = WrapContourParams { side: Side::Outside, ..p };
    let path = wrap_contour(&[rect(0.0, -6.0, 20.0, 6.0)], &flat(2.0 * r), &out).unwrap();
    let hi = cuts(&path, 600.0).iter().filter(|t| (t.2 - 8.0).abs() <= 1e-9).map(|t| t.1).fold(f64::NEG_INFINITY, f64::max);
    close((hi - 0.6) * 8.0, r, 0.01);
}

#[test]
fn a_profile_with_arcs_and_islands_is_cut_within_the_tolerance() {
    // A disc of radius 8 unrolled, with a round island of radius 2: lines and arcs.
    let disc = Region2 { outer: Loop2::circle(Vec2::new(0.0, 10.0), 8.0), holes: vec![Loop2::circle(Vec2::new(0.0, 10.0), 2.0)] };
    let p = pocket_params(-1.0, 1.0);
    let path = wrap_pocket(&[disc], &flat(3.0), &p).unwrap();
    verify5(&path, DEFAULT_MAX_ANGLE_STEP).unwrap();
    // Unrolled at the level's radius every centre is 1.5 inside the disc and outside the island.
    for (x, th, rho) in cuts(&path, 600.0) {
        let q = Vec2::new(x, th * rho);
        let c = Vec2::new(0.0, 10.0 * rho / R);
        // Round the cylinder the level is ρ/R of the drawn profile: the disc and the island are
        // ellipses there, within circles of their radii along x and outside those of their
        // radii scaled by ρ/R.
        let e = (q - c).len();
        assert!(e <= 8.0 - 1.5 + 0.01 && e >= 2.0 * rho / R + 1.5 - 0.01, "{q:?} at {rho}");
    }
}

#[test]
fn hostile_wrapped_operations_are_refused() {
    let tool = flat(4.0);
    let slot = [rect(-5.0, 0.0, 5.0, 15.0)];
    let with = |f: &dyn Fn(&mut WrapPocketParams)| {
        let mut p = pocket_params(-2.0, 1.0);
        f(&mut p);
        p
    };
    let cases = [
        with(&|p| p.cut.radius = f64::NAN),
        with(&|p| p.cut.radius = 0.0),
        with(&|p| p.cut.radius = 1e300),
        with(&|p| p.cut.bottom = -10.0),
        with(&|p| p.cut.bottom = f64::NEG_INFINITY),
        with(&|p| p.cut.top = 2.0),
        with(&|p| p.cut.retract = 6.0),
        with(&|p| p.cut.clearance = 2000.0),
        with(&|p| p.cut.stepdown = 0.0),
        with(&|p| p.cut.stepdown = f64::NAN),
        with(&|p| p.cut.stepdown = 2.0 / (MAX_WRAP_LEVELS as f64 + 1.0)),
        with(&|p| p.cut.tolerance = 0.0),
        with(&|p| p.cut.max_angle_step = 0.0),
        with(&|p| p.cut.max_angle_step = 1.0),
        with(&|p| p.cut.feeds.cut = -1.0),
        with(&|p| p.stepover = 0.0),
        with(&|p| p.stepover = 5.0),
        with(&|p| p.ramp_angle = 3.0),
        with(&|p| p.stock_to_leave = f64::NAN),
    ];
    for p in &cases {
        assert!(wrap_pocket(&slot, &tool, p).is_err(), "{p:?}");
    }
    let p = pocket_params(-2.0, 1.0);
    // More than once round, nothing to cut, points that are not numbers, a drill, a tool too wide.
    assert!(wrap_pocket(&[rect(-5.0, 0.0, 5.0, TAU * R + 1.0)], &tool, &p).is_err());
    assert!(wrap_pocket(&[], &tool, &p).is_err());
    assert!(wrap_pocket(&[rect(-5.0, 0.0, f64::NAN, 15.0)], &tool, &p).is_err());
    assert!(wrap_pocket(&[rect(-5.0, 0.0, 5.0, 1e300)], &tool, &p).is_err());
    assert!(wrap_pocket(&slot, &Tool::new(1, ToolKind::Drill, 4.0, 15.0), &p).is_err());
    assert!(wrap_pocket(&slot, &flat(12.0), &p).is_err());
    let degenerate = Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)]), holes: Vec::new() };
    assert!(wrap_pocket(&[degenerate], &tool, &p).is_err());
    let c = WrapContourParams { cut: cut(-2.0, 1.0), side: Side::Outside, climb: true, stock_to_leave: 0.0, ramp_angle: 0.0 };
    assert!(wrap_contour(&[rect(-5.0, 0.0, 5.0, TAU * R - 2.0)], &tool, &c).is_err(), "round the outside it overlaps itself");
    assert!(wrap_contour(&slot, &tool, &WrapContourParams { cut: WrapCut { radius: -1.0, ..c.cut }, ..c }).is_err());
}
