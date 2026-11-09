use solidcore_record::Vec3;

use super::{RoundPath, RoundPiece, RoundSweep, RoundTool, rounded_chord};
use crate::dexel::Stock;
use crate::gcode::{CodeTable, parse};
use crate::sim::step_count;
use crate::sweep::{ArcFixedAxis, FlatTool, Sweep};
use crate::toolpath::{Program, Shape, Tool};

const R: f64 = 2.5;
const L: f64 = 10.0;
const WIDE: (f64, f64) = (-100.0, 100.0);
const BUDGET: u64 = 1 << 32;

fn ball() -> RoundTool {
    RoundTool::new(R, R, L).expect("ball")
}

fn bull(rc: f64) -> RoundTool {
    RoundTool::new(R, rc, L).expect("bull")
}

fn line(from: Vec3, to: Vec3, tool: RoundTool) -> RoundPiece {
    RoundPiece { path: RoundPath::Line { from, to }, tool }
}

fn arc(start: Vec3, end: Vec3, centre: Vec3, ccw: bool, tool: RoundTool) -> RoundPiece {
    let flat = FlatTool::new(tool.radius, tool.length, Vec3::Z).expect("flat proxy");
    let arc = ArcFixedAxis::new(start, end, centre, Vec3::Z, ccw, 0, flat).expect("arc");
    RoundPiece { path: RoundPath::Arc(arc), tool }
}

fn sweep(piece: &RoundPiece, pitch: f64) -> RoundSweep {
    piece.sweep(pitch).expect("sweep")
}

/// Sorts intervals and joins any that overlap or lie less than `gap` apart.
fn merge(mut v: Vec<(f64, f64)>, gap: f64) -> Vec<(f64, f64)> {
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 + gap => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn ray_set(s: &dyn Sweep, o: Vec3, d: Vec3, tr: (f64, f64)) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    s.ray(o, d, tr, &mut out);
    merge(out, 1e-9)
}

/// Compares two interval sets: the same count after joining gaps under 2e-3, endpoints within `tol`.
fn assert_intervals(got: &[(f64, f64)], want: &[(f64, f64)], tol: f64, what: &str) {
    let got = merge(got.to_vec(), 2e-3);
    let want = merge(want.to_vec(), 2e-3);
    assert_eq!(got.len(), want.len(), "{what}: got {got:?} want {want:?}");
    for (g, w) in got.iter().zip(&want) {
        assert!((g.0 - w.0).abs() < tol && (g.1 - w.1).abs() < tol, "{what}: got {got:?} want {want:?}");
    }
}

/// Whether `x` is inside the body of `tool` with its tip at `p`, from the definition: heights from 0 to
/// the flute length, and within the profile (the flat core, then the torus corner, then the shank).
fn inside(tool: &RoundTool, p: Vec3, x: Vec3) -> bool {
    let w = x - p;
    if w.z < 0.0 || w.z > tool.length {
        return false;
    }
    let rc = tool.corner;
    let reach = if w.z <= rc { tool.core_radius() + (rc * rc - (rc - w.z) * (rc - w.z)).max(0.0).sqrt() } else { tool.radius };
    w.x.hypot(w.y) <= reach
}

/// The removed intervals of the ray `o + t d` by the definition: the ray is tested every `dt` over
/// `t_range` against tool tips at `n_s + 1` evenly spaced positions of the path. Slow, and exact to
/// about `dt` and the path spacing: the reference the sweep is checked against.
fn brute(tool: &RoundTool, path: &dyn Fn(f64) -> Vec3, o: Vec3, d: Vec3, t_range: (f64, f64), dt: f64, n_s: usize) -> Vec<(f64, f64)> {
    let tips: Vec<Vec3> = (0..=n_s).map(|k| path(k as f64 / n_s as f64)).collect();
    let steps = ((t_range.1 - t_range.0) / dt).ceil() as usize;
    let mut out = Vec::new();
    let mut start: Option<f64> = None;
    for k in 0..=steps {
        let t = t_range.0 + k as f64 * dt;
        let x = o + d * t;
        let hit = tips.iter().any(|p| inside(tool, *p, x));
        match (hit, start) {
            (true, None) => start = Some(t),
            (false, Some(s)) => {
                out.push((s, t - dt));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, t_range.1));
    }
    out
}

/// The removed volume of a straight move at depth `depth` below the top (z = 0) of the stock, by Cavalieri
/// and Steiner: the body's volume in the stock, plus the move length times the body's projection, the
/// area of its width profile. The body is clipped at the top, and is convex, so the sweep is exact.
fn straight_volume(tool: &RoundTool, path_len: f64, depth: f64) -> f64 {
    let n = 200_000;
    let top = depth.min(tool.length);
    let dh = top / n as f64;
    let rc = tool.corner;
    let (mut vol, mut area) = (0.0, 0.0);
    for k in 0..n {
        let h = (k as f64 + 0.5) * dh;
        let reach = if h <= rc { tool.core_radius() + (rc * rc - (rc - h) * (rc - h)).max(0.0).sqrt() } else { tool.radius };
        vol += std::f64::consts::PI * reach * reach * dh;
        area += 2.0 * reach * dh;
    }
    vol + path_len * area
}

#[test]
fn bad_round_tools_are_refused_with_a_reason() {
    assert!(RoundTool::new(0.0, 0.0, L).expect_err("zero radius").contains("radius"));
    assert!(RoundTool::new(R, 0.0, L).expect_err("zero corner").contains("corner radius"));
    assert!(RoundTool::new(R, 3.0, L).expect_err("corner above the radius").contains("corner radius"));
    assert!(RoundTool::new(R, 1.0, 0.0).expect_err("zero flute").contains("flute"));
    assert!(RoundTool::new(f64::NAN, 1.0, L).is_err());
    assert!(RoundTool::new(R, f64::INFINITY, L).is_err());
    assert!(RoundTool::new(R, 1.0, f64::NAN).is_err());
    assert!(RoundTool::new(R, R, L).is_ok(), "a ball");
    assert!(RoundTool::new(R, 1.0, L).is_ok(), "a bull-nose");
}

#[test]
fn a_static_ball_is_a_hemisphere_under_its_shank() {
    // Tip at the origin, ball centre at height R: a Z ray at lateral distance 1 is inside from the
    // hemisphere's surface, z = R - sqrt(R^2 - 1), up to the flute top.
    let s = sweep(&line(Vec3::ZERO, Vec3::ZERO, ball()), 0.1);
    let got = ray_set(&s, Vec3::new(1.0, 0.0, -20.0), Vec3::Z, WIDE);
    let floor = R - (R * R - 1.0f64).sqrt();
    let want = [(floor + 20.0, L + 20.0)];
    assert_intervals(&got, &want, 1e-9, "static ball");
}

#[test]
fn a_static_bull_has_a_flat_floor_and_a_fillet_of_its_corner_radius() {
    // Corner radius 1 on R 2.5: the flat floor is a disc of radius 1.5, the fillet rises to the wall at
    // height 1 on the radius 2.5. The floor is the lower edge of the removed interval along a Z ray.
    let rc = 1.0;
    let s = sweep(&line(Vec3::ZERO, Vec3::ZERO, bull(rc)), 0.1);
    for rho in [0.5, 1.0, 1.4, 2.0, 2.4] {
        let got = ray_set(&s, Vec3::new(rho, 0.0, -20.0), Vec3::Z, WIDE);
        let a = R - rc;
        let floor = if rho <= a { 0.0 } else { rc - (rc * rc - (rho - a) * (rho - a)).sqrt() };
        assert_intervals(&got, &[(floor + 20.0, L + 20.0)], 1e-9, &format!("bull floor at {rho}"));
    }
}

#[test]
fn a_short_flute_stops_the_body_at_its_length() {
    // Flute 0.5 under a corner radius of 1: the body ends at height 0.5, the fillet starts above the tip.
    let short = RoundTool::new(R, 1.0, 0.5).expect("tool");
    let s = sweep(&line(Vec3::ZERO, Vec3::ZERO, short), 0.1);
    // The ray starts at z = -20, so a height z is the parameter z + 20.
    assert_intervals(&ray_set(&s, Vec3::new(1.0, 0.0, -20.0), Vec3::Z, WIDE), &[(20.0, 20.5)], 1e-9, "core under the flute top");
    let fillet = 1.0 - (1.0 - 0.25f64).sqrt();
    assert_intervals(&ray_set(&s, Vec3::new(2.0, 0.0, -20.0), Vec3::Z, WIDE), &[(fillet + 20.0, 20.5)], 1e-9, "fillet under the flute top");
}

#[test]
fn across_a_bull_the_rounded_part_is_a_disc_at_its_height() {
    // At height 0.5 the rounded part is the disc of radius a + m, m = sqrt(r^2 - (h - r)^2): the core
    // fills the hole that the torus alone would leave, so one chord, centred on the tip.
    let (a, rc, h) = (R - 1.0, 1.0f64, 0.5f64);
    let m = (rc * rc - (h - rc) * (h - rc)).sqrt();
    let got = rounded_chord(Vec3::new(-10.0, 0.0, h), Vec3::X, Vec3::ZERO, &bull(rc)).expect("a chord");
    assert!((got.0 - (10.0 - (a + m))).abs() < 1e-9 && (got.1 - (10.0 + (a + m))).abs() < 1e-9, "{got:?}");
}

#[test]
fn across_a_bull_the_shank_and_core_make_one_chord() {
    // The same ray through the whole tool at height 0.5: the flat core covers the hole, so the removed
    // chord is the full outer radius a + sqrt(r^2 - (r - h)^2), one interval.
    let tool = bull(1.0);
    let s = sweep(&line(Vec3::ZERO, Vec3::ZERO, tool), 0.1);
    let reach = (R - 1.0) + (1.0 - 0.25f64).sqrt();
    let half = (reach * reach - 1.0).sqrt();
    let got = ray_set(&s, Vec3::new(-10.0, 1.0, 0.5), Vec3::X, WIDE);
    assert_intervals(&got, &[(10.0 - half, 10.0 + half)], 1e-9, "bull chord");
}

#[test]
fn a_ball_along_a_line_matches_the_definition() {
    // Rays against the definition, a ball moving along X at height -3 (the tip) over x = 0..10.
    let tool = ball();
    let (from, to) = (Vec3::new(0.0, 0.0, -3.0), Vec3::new(10.0, 0.0, -3.0));
    let s = sweep(&line(from, to, tool), 0.05);
    let path = |f: f64| from + (to - from) * f;
    for (o, d, tr) in [
        (Vec3::new(5.0, 1.0, -12.0), Vec3::Z, (-12.0, 12.0)),
        (Vec3::new(-5.0, 0.5, -1.0), Vec3::X, (-5.0, 20.0)),
        (Vec3::new(3.0, -2.0, -5.0), Vec3::Z, (-12.0, 12.0)),
    ] {
        let want = brute(&tool, &path, o, d, tr, 0.005, 2000);
        assert_intervals(&ray_set(&s, o, d, tr), &want, 0.04, &format!("ball line ray {o:?} {d:?}"));
    }
}

#[test]
fn a_bull_along_an_arc_matches_the_definition() {
    // A quarter arc (radius 6, tip at height -3) with a bull-nose of corner radius 1: X, Y and Z rays
    // against the definition.
    let tool = bull(1.0);
    let (start, end, centre) = (Vec3::new(6.0, 0.0, -3.0), Vec3::new(0.0, 6.0, -3.0), Vec3::new(0.0, 0.0, -3.0));
    let piece = arc(start, end, centre, true, tool);
    let s = sweep(&piece, 0.05);
    let flat = FlatTool::new(R, L, Vec3::Z).expect("flat");
    let reference = ArcFixedAxis::new(start, end, centre, Vec3::Z, true, 0, flat).expect("arc");
    let path = |f: f64| reference.point(f);
    for (o, d, tr) in [
        (Vec3::new(-10.0, 3.0, -2.0), Vec3::X, (-10.0, 20.0)),
        (Vec3::new(3.0, -10.0, -2.5), Vec3::Y, (-10.0, 20.0)),
        (Vec3::new(4.0, 4.0, -15.0), Vec3::Z, (-15.0, 5.0)),
    ] {
        let want = brute(&tool, &path, o, d, tr, 0.01, 2000);
        assert_intervals(&ray_set(&s, o, d, tr), &want, 0.04, &format!("bull arc ray {o:?} {d:?}"));
    }
}

#[test]
fn a_ball_plunge_leaves_a_hemispherical_dimple() {
    // Plunge from z = 5 to a tip at z = -3. The ball centre ends at z = -0.5, so the dimple's floor is
    // z = -0.5 - sqrt(R^2 - rho^2) at lateral distance rho, and it is R deep below the centre.
    let plunge = sweep(&line(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -3.0), ball()), 0.05);
    let zc = -3.0 + R;
    for rho in [0.0, 1.0, 2.0, 2.4] {
        let got = ray_set(&plunge, Vec3::new(rho, 0.0, -20.0), Vec3::Z, WIDE);
        let floor = zc - (R * R - rho * rho).sqrt();
        assert_intervals(&got, &[(floor + 20.0, 5.0 + L + 20.0)], 0.02, &format!("dimple at {rho}"));
    }
    // Beyond the shank's radius there is nothing: the rim is the shank's wall.
    assert!(ray_set(&plunge, Vec3::new(R + 0.1, 0.0, -20.0), Vec3::Z, WIDE).is_empty(), "outside the shank");
}

#[test]
fn a_ball_plunge_removes_its_analytic_volume() {
    // A hemisphere below the centre level (2/3 pi R^3), and a cylinder of radius R from there to the top.
    let mut stock = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(60.0, 20.0, 0.0), 0.1, BUDGET).expect("stock");
    let before = stock.volume();
    let plunge = line(Vec3::new(30.0, 10.0, 5.0), Vec3::new(30.0, 10.0, -3.0), ball());
    stock.apply(&sweep(&plunge, 0.1), 1).expect("cut");
    let zc = -3.0 + R;
    let want = 2.0 / 3.0 * std::f64::consts::PI * R * R * R + std::f64::consts::PI * R * R * (0.0 - zc);
    let removed = before - stock.volume();
    assert!((removed - want).abs() < 0.02 * want, "removed {removed} mm^3, analytic {want} mm^3");
}

#[test]
fn a_ball_slot_has_a_semicircular_floor() {
    // A slot along X at y = 10, the tip at z = -4: the floor at lateral offset d is
    // z = -4 + R - sqrt(R^2 - d^2), so it is -4 on the centre line and rises to the slot's wall at d = R.
    // The probes sit midway between two samples (pitch 0.1), not on one.
    let s = sweep(&line(Vec3::new(10.0, 10.0, -4.0), Vec3::new(50.0, 10.0, -4.0), ball()), 0.1);
    for d in [0.0, 0.5, 1.0, 1.5, 2.0, 2.4] {
        let got = ray_set(&s, Vec3::new(30.05, 10.0 + d, -20.0), Vec3::Z, WIDE);
        let floor = -4.0 + R - (R * R - d * d).sqrt();
        assert_intervals(&got, &[(floor + 20.0, -4.0 + L + 20.0)], 0.01, &format!("slot floor at {d}"));
    }
}

#[test]
fn a_ball_slot_is_exact_along_an_x_ray() {
    // An X ray one millimetre above the tip, one millimetre off the centre line: the swept disc is radius 2
    // there (the ball's section at that height), so the chord runs sqrt(3) past each end of the slot.
    let s = sweep(&line(Vec3::new(10.0, 10.0, -4.0), Vec3::new(50.0, 10.0, -4.0), ball()), 0.1);
    let got = ray_set(&s, Vec3::new(-5.0, 11.0, -3.0), Vec3::X, WIDE);
    let half = 3.0f64.sqrt();
    assert_intervals(&got, &[(10.0 - half + 5.0, 50.0 + half + 5.0)], 1e-6, "x chord");
}

#[test]
fn a_ball_slot_removes_its_analytic_volume() {
    let mut stock = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(60.0, 20.0, 0.0), 0.1, BUDGET).expect("stock");
    let before = stock.volume();
    let piece = line(Vec3::new(10.0, 10.0, -4.0), Vec3::new(50.0, 10.0, -4.0), ball());
    stock.apply(&sweep(&piece, 0.1), 1).expect("cut");
    let removed = before - stock.volume();
    let want = straight_volume(&ball(), 40.0, 4.0);
    assert!((removed - want).abs() < 0.02 * want, "removed {removed} mm^3, analytic {want} mm^3");
}

#[test]
fn a_bull_slot_has_a_flat_floor_and_fillets() {
    // Corner radius 1: a flat floor of width 2 (R - r) = 3, then fillets of radius 1 up to the wall.
    let s = sweep(&line(Vec3::new(10.0, 10.0, -4.0), Vec3::new(50.0, 10.0, -4.0), bull(1.0)), 0.1);
    let a = R - 1.0;
    for d in [0.0, 1.0, 1.4, 2.0, 2.4] {
        let got = ray_set(&s, Vec3::new(30.0, 10.0 + d, -20.0), Vec3::Z, WIDE);
        let floor = if d <= a { -4.0 } else { -4.0 + 1.0 - (1.0 - (d - a) * (d - a)).sqrt() };
        let z = got.first().map_or(f64::NAN, |g| g.0 - 20.0);
        assert!((z - floor).abs() < 0.01, "bull slot floor at {d}: got {z} want {floor}");
    }
}

#[test]
fn a_bull_slot_removes_its_analytic_volume() {
    let mut stock = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(60.0, 20.0, 0.0), 0.1, BUDGET).expect("stock");
    let before = stock.volume();
    let piece = line(Vec3::new(10.0, 10.0, -4.0), Vec3::new(50.0, 10.0, -4.0), bull(1.0));
    stock.apply(&sweep(&piece, 0.1), 1).expect("cut");
    let removed = before - stock.volume();
    let want = straight_volume(&bull(1.0), 40.0, 4.0);
    assert!((removed - want).abs() < 0.02 * want, "removed {removed} mm^3, analytic {want} mm^3");
}

#[test]
fn a_full_circle_ball_pocket_has_an_analytic_floor_on_every_radius() {
    // A ball round a circle of radius 8, the tip at z = -4: the floor at distance rho from the centre is
    // -4 + R - sqrt(R^2 - (rho - 8)^2), with the ball's centre on the circle.
    let c = Vec3::new(0.0, 0.0, -4.0);
    let start = Vec3::new(8.0, 0.0, -4.0);
    let piece = arc(start, start, c, true, ball());
    let s = sweep(&piece, 0.1);
    for rho in [8.0, 6.5, 5.6, 9.2] {
        let got = ray_set(&s, Vec3::new(rho, 0.0, -20.0), Vec3::Z, WIDE);
        let floor = -4.0 + R - (R * R - (rho - 8.0) * (rho - 8.0)).sqrt();
        let z = got.first().map_or(f64::NAN, |g| g.0 - 20.0);
        assert!((z - floor).abs() < 0.01, "pocket floor at {rho}: got {z} want {floor}");
    }
}

#[test]
fn the_sampled_rounded_part_is_within_half_a_ray_spacing_of_the_fine_sweep() {
    // With the pitch at the ray spacing h, the floor of a ball slot is at most h / 2 normal to the
    // surface from the fine sweep: vertically, that is h / 2 times sqrt(1 + slope^2).
    // Probes sit midway between two coarse samples (x = 30.1 with the samples at 30.0 and 30.2), where the
    // error is largest. The X rays run along the move, one height above the tip.
    let h = 0.2;
    let path = line(Vec3::new(10.0, 10.0, -4.0), Vec3::new(50.0, 10.0, -4.0), ball());
    let coarse = sweep(&path, h);
    let fine = sweep(&path, 0.005);
    for d in [0.0, 0.5, 1.0, 1.5, 2.0] {
        let o = Vec3::new(30.0 + h / 2.0, 10.0 + d, -20.0);
        let a = ray_set(&coarse, o, Vec3::Z, WIDE);
        let b = ray_set(&fine, o, Vec3::Z, WIDE);
        let slope = if d < R { d / (R * R - d * d).sqrt() } else { f64::INFINITY };
        let bound = 0.5 * h * (1.0 + slope * slope).sqrt() + 1e-9;
        let (la, lb) = (a.first().map_or(f64::NAN, |g| g.0), b.first().map_or(f64::NAN, |g| g.0));
        assert!((la - lb).abs() <= bound, "floor at offset {d}: coarse {la} fine {lb} bound {bound}");
    }
    for d in [0.0, 0.5, 1.0, 1.5] {
        let o = Vec3::new(-5.0, 10.0 + d, -3.0);
        let a = ray_set(&coarse, o, Vec3::X, WIDE);
        let b = ray_set(&fine, o, Vec3::X, WIDE);
        let (la, lb) = (a.first().map_or(f64::NAN, |g| g.0), b.first().map_or(f64::NAN, |g| g.0));
        assert!((la - lb).abs() <= 0.5 * h + 1e-9, "x ray at offset {d}: coarse {la} fine {lb}");
        assert_eq!(a.len(), 1, "one chord along the slot at offset {d}: {a:?}");
    }
}

#[test]
fn a_ball_on_a_g18_arc_matches_the_definition() {
    // An arc in the XZ plane (normal Y) from (8, 0, -3) over the top (0, 0, 5) to (-8, 0, -3): X, Y and Z rays
    // against the definition. The pitch is 0.05; the reference takes 800 tip positions along the arc.
    let tool = ball();
    let (start, end, centre) = (Vec3::new(8.0, 0.0, -3.0), Vec3::new(-8.0, 0.0, -3.0), Vec3::new(0.0, 0.0, -3.0));
    let flat = FlatTool::new(R, L, Vec3::Z).expect("flat");
    let reference = ArcFixedAxis::new(start, end, centre, Vec3::Y, false, 0, flat).expect("G18 arc");
    let piece = RoundPiece { path: RoundPath::Arc(reference), tool };
    let s = sweep(&piece, 0.05);
    let path = |f: f64| reference.point(f);
    for (o, d, tr) in [
        (Vec3::new(-20.0, 0.5, 6.5), Vec3::X, (-20.0, 20.0)),
        (Vec3::new(3.0, 0.5, -20.0), Vec3::Z, (-20.0, 20.0)),
        (Vec3::new(0.0, -20.0, 6.0), Vec3::Y, (-20.0, 20.0)),
    ] {
        let want = brute(&tool, &path, o, d, tr, 0.01, 800);
        assert_intervals(&ray_set(&s, o, d, tr), &want, 0.05, &format!("G18 ray {o:?} {d:?}"));
    }
}

#[test]
fn a_long_ball_move_along_x_leaves_no_slivers() {
    // A 50 mm move at height -3, pitch 0.05. A ray 3e-5 above the tip is cut only in a band narrower than
    // half a pitch under each sample's sphere. Joined along the move, each ray keeps one interval, and the
    // stock takes the cut without exceeding its segment cap.
    let tool = ball();
    let h = 0.05;
    let s = sweep(&line(Vec3::new(0.0, 0.0, -3.0), Vec3::new(50.0, 0.0, -3.0), tool), h);
    let z = -3.0 + 3e-5;
    let mut worst = 0;
    for k in -6..=6 {
        let y = f64::from(k) * 0.0031;
        worst = worst.max(ray_set(&s, Vec3::new(-5.0, y, z), Vec3::X, WIDE).len());
    }
    assert!(worst <= 1, "a ray has {worst} intervals");
    // The same move on a stock whose ray grid holds the probe height: the cut is taken, not refused.
    let grid_z = -2.975;
    let tip_z = grid_z - 3e-5;
    let mut stock = Stock::new_box(Vec3::new(-4.0, -3.0, -6.0), Vec3::new(54.0, 3.0, 0.0), h, BUDGET).expect("stock");
    let moved = line(Vec3::new(0.0, 0.0, tip_z), Vec3::new(50.0, 0.0, tip_z), tool);
    stock.apply(&sweep(&moved, h), 1).expect("the cut is taken");
}

#[test]
fn a_round_tool_arc_is_cut_in_pieces_of_at_most_two_millimetres() {
    // A quarter arc of radius 50 is 78.5 mm: 40 pieces of 2 mm for a round tool, 4 quarter turns for a flat one.
    let text = "T0 M6\nG0 X50 Y0 Z5\nG1 Z-3\nG3 X0 Y50 I-50 J0 F100";
    let ball_program = Program {
        tools: vec![Tool { id: 0, shape: Shape::Ball, diameter: 5.0, flute_length: 10.0, shank: Vec::new(), holder: Vec::new(), stickout: 30.0 }],
        ..parse(text, &CodeTable::default())
    };
    let flat_program = Program {
        tools: vec![Tool { id: 0, shape: Shape::Flat, diameter: 5.0, flute_length: 10.0, shank: Vec::new(), holder: Vec::new(), stickout: 30.0 }],
        ..parse(text, &CodeTable::default())
    };
    // Moves: the first rapid (no start point), the plunge, then the arc. A flat tool cuts a quarter turn in one step.
    assert_eq!(step_count(&ball_program, 2), 40);
    assert_eq!(step_count(&flat_program, 2), 1);
}

#[test]
fn a_round_tool_move_of_three_metres_is_cut_without_a_diagnostic() {
    // 1024 pieces of about 2.9 mm each; the part past the 60 mm stock cuts nothing and is not refused.
    let program = Program {
        tools: vec![Tool { id: 0, shape: Shape::Ball, diameter: 5.0, flute_length: 10.0, shank: Vec::new(), holder: Vec::new(), stickout: 30.0 }],
        ..parse("T0 M6\nG0 X0 Y0 Z5\nG1 Z-3\nG1 X3000 F100", &CodeTable::default())
    };
    assert_eq!(step_count(&program, 2), 1024);
    let mut stock = Stock::new_box(Vec3::new(0.0, -10.0, -10.0), Vec3::new(60.0, 10.0, 0.0), 0.5, BUDGET).expect("stock");
    let diags = crate::sim::simulate(&program, &mut stock).expect("simulate");
    assert!(diags.is_empty(), "{diags:?}");
    assert!(stock.volume() < 60.0 * 20.0 * 10.0, "the move cut the stock");
}

#[test]
fn a_round_piece_too_long_to_sample_is_reported_and_skipped() {
    // A 1e7 mm feed makes pieces of about 9.8 km, too long to sample every 0.1 mm within the sample guard.
    // (A tiny resolution is not the way to reach it: the stock budget refuses the grid first.) The plunge
    // before it is cut; the feed is reported and not cut; the run is not aborted.
    let program = Program {
        tools: vec![Tool { id: 0, shape: Shape::Ball, diameter: 5.0, flute_length: 10.0, shank: Vec::new(), holder: Vec::new(), stickout: 30.0 }],
        ..parse("T0 M6\nG0 X10 Y10 Z5\nG1 Z-2 F100\nG1 X10000000 F300", &CodeTable::default())
    };
    let mut stock = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(60.0, 20.0, 0.0), 0.1, BUDGET).expect("stock");
    let diags = crate::sim::simulate(&program, &mut stock).expect("the run goes on");
    assert!(diags.iter().any(|d| d.msg.contains("too long to sample")), "{diags:?}");
    assert!(stock.volume() < 60.0 * 20.0 * 10.0, "the plunge was cut");
}

#[test]
fn sweep_bounds_contain_the_swept_volume() {
    let piece = arc(Vec3::new(6.0, 0.0, -3.0), Vec3::new(0.0, 6.0, -3.0), Vec3::new(0.0, 0.0, -3.0), true, bull(1.0));
    let b = sweep(&piece, 0.1).bounds();
    assert!(b.min.x <= -R + 1e-9 && b.max.x >= 6.0 + R - 1e-9, "x {b:?}");
    assert!(b.min.y <= -R + 1e-9 && b.max.y >= 6.0 + R - 1e-9, "y {b:?}");
    assert!(b.max.z >= -3.0 + L - 1e-9, "z {b:?}");
}

#[test]
fn a_bad_pitch_is_refused() {
    let piece = line(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), ball());
    assert!(piece.sweep(0.0).is_err());
    assert!(piece.sweep(f64::NAN).is_err());
}
