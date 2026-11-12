//! Tests of face mills and chamfer mills: their tool-list names, the tapered cutter (its drop
//! against triangles held against brute force, grown and shrunk), the chamfer operation's
//! width measured from the G-code it writes, the gouge check with a chamfer mill (chamfers,
//! traces and spots), and what a chamfer mill and a face mill are refused for.

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use super::*;
use crate::collision::gouges;
use crate::foundation_tests::{block, close};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    let pts = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
    Region2 { outer: Loop2::polygon(&pts), holes: vec![] }
}

/// A chamfer mill `d` across with a tip `tip` across and an included angle `angle` (degrees).
fn chamfer_mill(d: f64, tip: f64, angle: f64) -> Tool {
    let mut t = Tool::new(7, ToolKind::Chamfer, d, 10.0);
    t.tip_diameter = Some(tip);
    t.angle_deg = Some(angle);
    t
}

fn face_mill(d: f64, corner: Option<f64>) -> Tool {
    let mut t = Tool::new(1, ToolKind::FaceMill, d, 6.0);
    t.corner_radius = corner;
    t
}

/// A small, deterministic random number generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// The highest tip height at which `c` on the vertical through `(x, y)` touches the triangle,
/// by brute force: the tip height touching a point of the triangle is its height less the
/// cutter's height at its distance from the axis. Sampled on a grid round the axis (distance
/// and angle, the tip's rim and the top rim among the distances, where the cutter's profile
/// bends), the best few samples are each refined with ever finer grids round them.
fn brute_drop(c: &Cutter, t: &[Vec3; 3], x: f64, y: f64) -> Option<f64> {
    let n = (t[1] - t[0]).cross(t[2] - t[0]);
    if n.z.abs() < 1e-6 * n.len() {
        return None;
    }
    let rim = c.flat_radius();
    let eval = |d: f64, th: f64| -> Option<f64> {
        if !(0.0..=c.radius).contains(&d) {
            return None;
        }
        let (px, py) = (x + d * th.cos(), y + d * th.sin());
        // Barycentric coordinates seen from above.
        let (ax, ay, bx, by, qx, qy) = (t[1].x - t[0].x, t[1].y - t[0].y, t[2].x - t[0].x, t[2].y - t[0].y, px - t[0].x, py - t[0].y);
        let det = ax * by - ay * bx;
        let (u, v) = ((qx * by - qy * bx) / det, (ax * qy - ay * qx) / det);
        if u < 0.0 || v < 0.0 || u + v > 1.0 {
            return None;
        }
        let z = t[0].z + (t[1].z - t[0].z) * u + (t[2].z - t[0].z) * v;
        c.height(d).map(|h| z - h)
    };
    let (nd, na) = (120, 360);
    let mut samples: Vec<(f64, f64, f64)> = Vec::new();
    for i in 0..=nd + 1 {
        let d = if i == nd + 1 { rim } else { c.radius * i as f64 / nd as f64 };
        for j in 0..na {
            let th = std::f64::consts::TAU * j as f64 / na as f64;
            if let Some(z) = eval(d, th) {
                samples.push((d, th, z));
            }
        }
    }
    samples.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut best = f64::NEG_INFINITY;
    for &(d0, th0, z0) in samples.iter().take(6) {
        let (mut d, mut th, mut z) = (d0, th0, z0);
        let (mut wd, mut wa) = (2.0 * c.radius / nd as f64, 2.0 * std::f64::consts::TAU / na as f64);
        for _ in 0..90 {
            let k = 6;
            for a in -k..=k {
                let tt = th + wa * a as f64 / k as f64;
                let ds = (-k..=k).map(|b| d + wd * b as f64 / k as f64).chain([rim, c.radius]).filter(|dd| (dd - d).abs() <= wd * 1.0001);
                for dd in ds.collect::<Vec<_>>() {
                    if let Some(zz) = eval(dd, tt)
                        && zz > z
                    {
                        (d, th, z) = (dd, tt, zz);
                    }
                }
            }
            wd *= 0.7;
            wa *= 0.7;
        }
        best = best.max(z);
    }
    // The boundary, where a grid round the axis is weakest, sampled along each edge.
    for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
        let at = |s: f64| {
            let r = p.lerp(q, s.clamp(0.0, 1.0));
            c.height((r.x - x).hypot(r.y - y)).map(|h| r.z - h)
        };
        let m = 20_000;
        let Some((mut s, mut z)) =
            (0..=m).filter_map(|i| at(i as f64 / m as f64).map(|z| (i as f64 / m as f64, z))).max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        let mut w = 2.0 / m as f64;
        for _ in 0..80 {
            for k in -10..=10 {
                let ss = (s + w * k as f64 / 10.0).clamp(0.0, 1.0);
                if let Some(zz) = at(ss)
                    && zz > z
                {
                    (s, z) = (ss, zz);
                }
            }
            w *= 0.7;
        }
        best = best.max(z);
    }
    best.is_finite().then_some(best)
}

#[test]
fn face_and_chamfer_mills_read_check_and_name_themselves() {
    let face: Tool = serde_json::from_str(r#"{"number":1,"kind":"face_mill","diameter":10,"flute_length":6,"corner_radius":0.2}"#).unwrap();
    face.validate().unwrap();
    assert_eq!(face.kind.label(), "face mill");
    assert_eq!(face.cutter().unwrap(), Cutter::bull(5.0, 0.2).unwrap());
    assert_eq!(face_mill(10.0, None).cutter().unwrap(), Cutter::flat(5.0).unwrap());
    assert_eq!(face_mill(10.0, Some(0.0)).cutter().unwrap(), Cutter::flat(5.0).unwrap());
    assert!(face_mill(10.0, Some(5.0)).validate().is_err(), "a face mill's corner is below its radius");
    assert!(face_mill(10.0, Some(-0.1)).validate().is_err());
    assert!(face_mill(10.0, Some(f64::NAN)).validate().is_err());

    let ch: Tool = serde_json::from_str(r#"{"number":7,"kind":"chamfer","diameter":4,"flute_length":10,"tip_diameter":0.5,"angle_deg":90}"#).unwrap();
    ch.validate().unwrap();
    assert_eq!(ch.kind.label(), "chamfer mill");
    let c = ch.cutter().unwrap();
    let cone = c.cone.unwrap();
    close(cone.tip_radius, 0.25, 1e-12);
    close(cone.half_angle, std::f64::consts::FRAC_PI_4, 1e-12);
    close(c.rim_height(), 1.75, 1e-12);
    assert_eq!(serde_json::to_value(&ch).unwrap()["kind"], "chamfer");
    assert_eq!(serde_json::to_value(&face).unwrap()["kind"], "face_mill");

    let mut bad = chamfer_mill(4.0, 0.0, 90.0);
    bad.angle_deg = None;
    assert!(bad.validate().is_err() && bad.cutter().is_err(), "a chamfer mill needs its angle");
    for angle in [0.0, 0.5, 179.5, 180.0, f64::NAN, f64::INFINITY, -90.0] {
        assert!(chamfer_mill(4.0, 0.0, angle).validate().is_err(), "angle {angle}");
    }
    for tip in [4.0, 5.0, -0.1, f64::NAN] {
        assert!(chamfer_mill(4.0, tip, 90.0).validate().is_err(), "tip {tip}");
    }
    let mut flat = Tool::new(2, ToolKind::FlatEndmill, 6.0, 20.0);
    flat.angle_deg = Some(90.0);
    assert!(flat.validate().is_err(), "an angle is only for a chamfer mill");
    // Contours and pockets refuse a tapered side.
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let cut =
        Cut { heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -1.0 }, feeds: Feeds { cut: 500.0, plunge: 100.0 }, stepdown: 1.0 };
    assert!(contour(&[rect(0.0, 0.0, 10.0, 10.0)], &ch, &params, &cut).is_err());
}

#[test]
fn the_tool_list_names_face_and_chamfer_mills() {
    let post = Post {
        tool_list: true,
        upper_case_comments: true,
        trailing_decimal: true,
        comment_chars: Some(" .,=_-".into()),
        operation_name_only: true,
        ..Post::rs274()
    };
    let feeds = Feeds { cut: 1600.0, plunge: 50.0 };
    let face = face_mill(10.0, Some(0.2));
    let cut = Cut { heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -0.05 }, feeds, stepdown: 1.0 };
    // A face mill ramps in: it cannot plunge.
    let params = PocketParams { stepover: 7.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 2f64.to_radians() };
    let facing = pocket(&[rect(0.0, 0.0, 30.0, 20.0)], &face, &params, &cut).unwrap();
    let mill = chamfer_mill(4.0, 0.0, 90.0);
    let ch = ChamferParams {
        width: 0.5,
        tip_offset: 0.2,
        side: Side::Outside,
        climb: true,
        tolerance: 0.01,
        clearance: 10.0,
        retract: 2.0,
        top: 0.0,
        feeds,
    };
    let edge = chamfer(&[rect(0.0, 0.0, 30.0, 20.0)], &mill, &ch).unwrap();
    let program = Program {
        name: "tools".into(),
        operations: vec![
            Operation { name: "face".into(), tool: face, spindle_rpm: 18000.0, coolant: false, path: facing },
            Operation { name: "deburr".into(), tool: mill, spindle_rpm: 20000.0, coolant: false, path: edge },
        ],
        work_offset: None,
    };
    let text = emit(&program, &post).unwrap();
    assert!(text.contains("(T1 D=10. CR=0.2 - ZMIN=-0.05 - FACE MILL)"), "{text}");
    assert!(text.contains("(T7 D=4. CR=0. - ZMIN=-0.7 - CHAMFER MILL)"), "{text}");
}

#[test]
fn the_tapered_drop_matches_brute_force_on_random_triangles() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut compared, mut faces, mut steep) = (0, 0, 0);
    for case in 0..1500 {
        let radius = rng.range(0.5, 4.0);
        let tip = if case % 4 == 0 { 0.0 } else { rng.range(0.0, 0.8) * radius };
        let half = rng.range(10.0, 80.0).to_radians();
        let c = Cutter::tapered(radius, tip, half).unwrap();
        let mut corner = || Vec3::new(rng.range(-5.0, 5.0), rng.range(-5.0, 5.0), rng.range(-3.0, 3.0));
        let t = [corner(), corner(), corner()];
        let (x, y) = (rng.range(-4.0, 4.0), rng.range(-4.0, 4.0));
        let exact = drop_triangle(&c, &t, x, y);
        let Some(brute) = brute_drop(&c, &t, x, y) else { continue };
        let exact = exact.unwrap_or_else(|| panic!("case {case}: brute force touches at {brute}, the drop-cutter misses"));
        // The drop-cutter's answer is a point the cutter really touches, so never below the
        // brute force's best; and brute force finds the true maximum to well under 1e-6.
        assert!((exact - brute).abs() <= 1e-6, "case {case}: {c:?} {t:?} at ({x}, {y}): drop {exact}, brute force {brute}");
        compared += 1;
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.z.abs() > 0.5 * n.len() {
            faces += 1;
        } else {
            steep += 1;
        }
    }
    assert!(compared > 600 && faces > 100 && steep > 100, "compared {compared} ({faces} gentle, {steep} steep)");
}

#[test]
fn the_tapered_drop_touches_planes_at_the_tip_rim_or_the_top_rim() {
    let c = Cutter::tapered(3.0, 0.5, 45f64.to_radians()).unwrap();
    // A level plane: the flat tip sits on it.
    let level = [Vec3::new(-20.0, -20.0, 2.0), Vec3::new(20.0, -20.0, 2.0), Vec3::new(0.0, 20.0, 2.0)];
    close(drop_triangle(&c, &level, 1.0, 1.0).unwrap(), 2.0, 1e-12);
    // A 30° slope (gentler than the side): the tip's rim, 0.5 uphill, touches.
    let m = 30f64.to_radians().tan();
    let gentle = [Vec3::new(-20.0, -20.0, -20.0 * m), Vec3::new(20.0, -20.0, 20.0 * m), Vec3::new(20.0, 20.0, 20.0 * m)];
    close(drop_triangle(&c, &gentle, 10.0, -5.0).unwrap(), 10.5 * m, 1e-9);
    // A 60° slope (steeper than the side): the top rim, 3 uphill, touches; the cone is 2.5 high.
    let m = 60f64.to_radians().tan();
    let steep = [Vec3::new(-20.0, -20.0, -20.0 * m), Vec3::new(20.0, -20.0, 20.0 * m), Vec3::new(20.0, 20.0, 20.0 * m)];
    close(drop_triangle(&c, &steep, 10.0, -5.0).unwrap(), 13.0 * m - 2.5, 1e-9);
    // A level edge passing 1 mm from the axis meets the side where it is 1 out: 0.5 up the cone.
    let ridge = [Vec3::new(-10.0, 1.0, 4.0), Vec3::new(10.0, 1.0, 4.0), Vec3::new(0.0, 8.0, -10.0)];
    close(drop_triangle(&c, &ridge, 0.0, 0.0).unwrap(), 3.5, 1e-9);
    // Hostile cutters are refused.
    for (r, tip, half) in [(3.0, 3.0, 0.7), (3.0, -0.1, 0.7), (3.0, 0.0, 0.0), (3.0, 0.0, 1.6), (f64::NAN, 0.0, 0.7), (3.0, 0.0, f64::NAN)] {
        assert!(Cutter::tapered(r, tip, half).is_err(), "{r} {tip} {half}");
    }
    assert!(Cutter { radius: 3.0, corner: 0.5, cone: c.cone }.validate().is_err(), "a tapered cutter has no corner");
}

/// The tip height of `c`'s profile at distance `d` as a point in the plane through the axis.
fn profile(c: &Cutter, d: f64) -> Vec2 {
    Vec2::new(d, c.height(d).unwrap_or(f64::NAN))
}

#[test]
fn a_grown_cone_holds_the_offset_and_a_shrunk_one_stays_inside() {
    for (tip, deg) in [(0.0, 45.0), (0.4, 45.0), (1.0, 30.0), (0.2, 70.0), (2.5, 45.0)] {
        let c = Cutter::tapered(3.0, tip, f64::to_radians(deg)).unwrap();
        let by = 0.3;
        // Every point of the offset surface is at or above the grown cutter, its tip `by` lower.
        let g = c.grown(by).unwrap();
        let (sa, ca) = f64::to_radians(deg).sin_cos();
        let mut offset: Vec<Vec2> = Vec::new();
        for k in 0..=600 {
            let f = k as f64 / 600.0;
            // The flat tip, its rim, the side and the top rim, each moved out along its normals.
            offset.push(Vec2::new(tip * f, -by));
            let th = f * (std::f64::consts::FRAC_PI_2 - deg.to_radians());
            offset.push(Vec2::new(tip, 0.0) + Vec2::new(th.sin(), -th.cos()) * by);
            offset.push(profile(&c, tip + (3.0 - tip) * f) + Vec2::new(ca, -sa) * by);
            let th = f * deg.to_radians();
            offset.push(profile(&c, 3.0) + Vec2::new(th.cos() * ca + th.sin() * sa, th.cos() * -sa + th.sin() * ca) * by);
        }
        for q in offset {
            if q.x <= g.radius {
                assert!(g.height(q.x).unwrap() - by <= q.y + 1e-9, "tip {tip} {deg}°: offset {q:?} below the grown cutter");
            }
        }
        // Every point of the shrunk cutter is at least `by` inside the cutter (or it is the
        // side, moved in at most `by`).
        let (s, lift) = c.shrunk(by).unwrap();
        for k in 0..=600 {
            let d = s.radius * k as f64 / 600.0;
            let q = Vec2::new(d, s.height(d).unwrap() + lift);
            let nearest = (0..=3000).map(|j| profile(&c, 3.0 * j as f64 / 3000.0).dist(q)).fold(f64::INFINITY, f64::min);
            let above = q.y >= c.height(d).unwrap();
            assert!(above && nearest >= by - 2e-3, "tip {tip} {deg}°: shrunk point {q:?} is {nearest} from the cutter");
        }
    }
}

fn chamfer_params(width: f64, side: Side) -> ChamferParams {
    ChamferParams {
        width,
        tip_offset: DEFAULT_TIP_OFFSET,
        side,
        climb: true,
        tolerance: 0.01,
        clearance: 10.0,
        retract: 2.0,
        top: 0.0,
        feeds: Feeds { cut: 800.0, plunge: 100.0 },
    }
}

/// The chamfer's width across the top as the written moves cut it: along the straight cuts at
/// the bottom on the +X side of `wall_x`, the cone's radius at the top less the axis's distance
/// from the wall.
fn width_from_gcode(text: &str, tool: &Tool, wall_x: f64, top: f64, outside: bool) -> f64 {
    let c = tool.cutter().unwrap();
    let cone = c.cone.unwrap();
    let moves = gcode::parse(text).unwrap();
    let bottom = moves.iter().filter_map(Move::target).map(|p| p.z).fold(f64::INFINITY, f64::min);
    let side: Vec<Vec3> = moves
        .iter()
        .filter(|m| matches!(m, Move::Feed { .. }))
        .filter_map(Move::target)
        .filter(|p| (p.z - bottom).abs() < 1e-9 && p.x > wall_x * 0.5 && p.y.abs() <= wall_x + 1e-9)
        .collect();
    assert!(!side.is_empty(), "{text}");
    let at_top = cone.tip_radius + (top - bottom) * cone.half_angle.tan();
    let widths: Vec<f64> = side.iter().map(|p| if outside { at_top - (p.x - wall_x) } else { at_top - (wall_x - p.x) }).collect();
    let w = widths.iter().sum::<f64>() / widths.len() as f64;
    assert!(widths.iter().all(|v| (v - w).abs() < 1e-9), "{widths:?}");
    w
}

#[test]
fn a_chamfer_is_as_wide_as_asked_measured_from_its_gcode() {
    let post = Post::rs274();
    for (tool, width) in [(chamfer_mill(6.0, 0.0, 90.0), 1.0), (chamfer_mill(6.0, 0.5, 90.0), 2.0), (chamfer_mill(8.0, 1.0, 60.0), 1.2)] {
        for side in [Side::Outside, Side::Inside] {
            let p = chamfer_params(width, side);
            let g = chamfer_geometry(&tool, width, p.tip_offset).unwrap();
            let path = chamfer(&[rect(-10.0, -10.0, 10.0, 10.0)], &tool, &p).unwrap();
            let text = emit(
                &Program {
                    name: "chamfer".into(),
                    operations: vec![Operation { name: "chamfer".into(), tool: tool.clone(), spindle_rpm: 20000.0, coolant: false, path }],
                    work_offset: None,
                },
                &post,
            )
            .unwrap();
            let measured = width_from_gcode(&text, &tool, 10.0, 0.0, side == Side::Outside);
            close(measured, width, 1e-3);
            // One pass, at the depth the geometry says.
            let half = f64::to_radians(tool.angle_deg.unwrap() * 0.5);
            close(g.depth, width / half.tan() + DEFAULT_TIP_OFFSET, 1e-12);
            let bottom = gcode::parse(&text).unwrap().iter().filter_map(Move::target).map(|q| q.z).fold(f64::INFINITY, f64::min);
            close(bottom, -g.depth, 1e-3);
        }
    }
}

#[test]
fn a_chamfer_the_cone_cannot_reach_is_refused() {
    let tool = chamfer_mill(6.0, 0.0, 90.0);
    // The cone ends 3 out; with the tip 0.2 below the chamfer the axis is 0.2 out.
    let g = chamfer_geometry(&tool, 2.8, 0.2).unwrap();
    close(g.max_width, 2.8, 1e-12);
    let e = chamfer_geometry(&tool, 2.9, 0.2).unwrap_err().to_string();
    assert!(e.contains("2.8"), "{e}");
    assert!(chamfer_geometry(&tool, 0.5, 3.5).is_err(), "a tip that deep reaches nothing");
    let mut short = tool.clone();
    short.flute_length = 1.0;
    assert!(chamfer_geometry(&short, 1.0, 0.2).is_err(), "beyond the flutes");
    assert!(chamfer_geometry(&Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0), 1.0, 0.2).is_err(), "not a chamfer mill");
    for (w, t) in [(f64::NAN, 0.2), (0.0, 0.2), (-1.0, 0.2), (f64::INFINITY, 0.2), (1.0, -0.1), (1.0, f64::NAN), (1.0, 1e300)] {
        assert!(chamfer_geometry(&tool, w, t).is_err(), "{w} {t}");
    }
    let region = [rect(-10.0, -10.0, 10.0, 10.0)];
    assert!(chamfer(&region, &tool, &chamfer_params(1.0, Side::On)).is_err());
    for top in [f64::NAN, f64::INFINITY, 1e300] {
        assert!(chamfer(&region, &tool, &ChamferParams { top, ..chamfer_params(1.0, Side::Outside) }).is_err(), "top {top}");
    }
    assert!(chamfer(&region, &tool, &ChamferParams { retract: -5.0, ..chamfer_params(1.0, Side::Outside) }).is_err());
    assert!(chamfer(&[rect(-0.1, -0.1, 0.1, 0.1)], &tool, &chamfer_params(1.0, Side::Inside)).is_err(), "too small a hole");
    // A pointed tip with no tip offset runs on the edge.
    let on = chamfer(&region, &tool, &ChamferParams { tip_offset: 0.0, ..chamfer_params(1.0, Side::Outside) }).unwrap();
    assert!(on.moves.iter().filter_map(Move::target).any(|p| (p.x - 10.0).abs() < 1e-9 && (p.z + 1.0).abs() < 1e-9));
}

/// A closed box `[-s, s]² × [-h, 0]` with its top edges chamfered `w` at 45°.
fn chamfered_block(s: f64, h: f64, w: f64) -> Surface {
    let top = |x: f64, y: f64| Vec3::new(x * (s - w), y * (s - w), 0.0);
    let mid = |x: f64, y: f64| Vec3::new(x * s, y * s, -w);
    let low = |x: f64, y: f64| Vec3::new(x * s, y * s, -h);
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let mut tris = vec![[top(-1.0, -1.0), top(1.0, -1.0), top(1.0, 1.0)], [top(-1.0, -1.0), top(1.0, 1.0), top(-1.0, 1.0)]];
    tris.push([low(-1.0, -1.0), low(1.0, 1.0), low(1.0, -1.0)]);
    tris.push([low(-1.0, -1.0), low(-1.0, 1.0), low(1.0, 1.0)]);
    for k in 0..4 {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        tris.push([top(a.0, a.1), mid(a.0, a.1), mid(b.0, b.1)]);
        tris.push([top(a.0, a.1), mid(b.0, b.1), top(b.0, b.1)]);
        tris.push([mid(a.0, a.1), low(a.0, a.1), low(b.0, b.1)]);
        tris.push([mid(a.0, a.1), low(b.0, b.1), mid(b.0, b.1)]);
    }
    Surface::new(tris).unwrap()
}

#[test]
fn the_gouge_check_holds_a_chamfer_mill_to_the_chamfer() {
    let tool = chamfer_mill(6.0, 0.5, 90.0);
    let cutter = tool.cutter().unwrap();
    let path = chamfer(&[rect(-10.0, -10.0, 10.0, 10.0)], &tool, &chamfer_params(1.0, Side::Outside)).unwrap();
    let path5 = Toolpath5::from_3axis(&path, 0.001).unwrap();
    // The modelled chamfer: the cone lies on it and its tip stays clear of the wall below.
    let modelled = chamfered_block(10.0, 10.0, 1.0);
    let hits = gouges(&modelled, &cutter, &path5, 0.01, 0.2).unwrap();
    assert!(hits.is_empty(), "{hits:?}");
    // A sharp block: the cone cuts its edge 1 mm wide, deeper than the tolerance.
    let sharp = block(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 0.0));
    let hits = gouges(&sharp, &cutter, &path5, 0.01, 0.2).unwrap();
    assert!(hits.len() > 3, "{hits:?}");
    let worst = hits.iter().map(|h| h.distance).fold(0.0, f64::max);
    assert!(worst > 0.5 && worst < 1.3, "{worst}");
    // A chamfer 0.1 mm wider than modelled is found too.
    let wider = chamfer(&[rect(-10.0, -10.0, 10.0, 10.0)], &tool, &chamfer_params(1.1, Side::Outside)).unwrap();
    let hits = gouges(&modelled, &cutter, &Toolpath5::from_3axis(&wider, 0.001).unwrap(), 0.01, 0.2).unwrap();
    assert!(!hits.is_empty());
    // And the drop-cutter puts the cone on the chamfer: dropped beside the +X wall, the tip
    // stops at the depth the chamfer operation runs at.
    let g = chamfer_geometry(&tool, 1.0, DEFAULT_TIP_OFFSET).unwrap();
    close(drop_z(&modelled, &cutter, 10.0 + g.offset, 0.0).unwrap(), -g.depth, 1e-9);
}

/// Twice the signed area of the closed run of `points` seen from above: positive
/// anticlockwise.
fn signed_area(points: &[Vec3]) -> f64 {
    points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum()
}

#[test]
fn a_chamfer_runs_climb_or_conventional_as_a_contour_does() {
    // With a clockwise spindle climb milling keeps the wall on the right of the travel: round a
    // part clockwise, round an opening anticlockwise. The same with the axis on the edge (no
    // offset: a pointed tip and no tip offset) as off it.
    let tool = chamfer_mill(6.0, 0.0, 90.0);
    for tip_offset in [0.0, DEFAULT_TIP_OFFSET] {
        for (side, climb, anticlockwise) in
            [(Side::Outside, true, false), (Side::Outside, false, true), (Side::Inside, true, true), (Side::Inside, false, false)]
        {
            let p = ChamferParams { tip_offset, climb, ..chamfer_params(1.0, side) };
            let path = chamfer(&[rect(-10.0, -10.0, 10.0, 10.0)], &tool, &p).unwrap();
            let g = chamfer_geometry(&tool, 1.0, tip_offset).unwrap();
            let bottom: Vec<Vec3> = path
                .moves
                .iter()
                .filter(|m| matches!(m, Move::Feed { .. } | Move::Arc { .. }))
                .filter_map(Move::target)
                .filter(|q| (q.z + g.depth).abs() < 1e-9)
                .collect();
            assert!(bottom.len() >= 4, "{side:?} {climb}: {path:?}");
            let area = signed_area(&bottom);
            assert_eq!(area > 0.0, anticlockwise, "{side:?} climb {climb} tip offset {tip_offset}: area {area}");
            // Every cut at the bottom is the axis's offset from the edge, outside or inside.
            let want = if side == Side::Outside { g.offset } else { -g.offset };
            for q in &bottom {
                let out = q.x.abs().max(q.y.abs()) - 10.0;
                assert!((out - want).abs() <= 1e-6 || (side == Side::Outside && out < want), "{side:?} {tip_offset}: {q:?}");
            }
        }
    }
}

#[test]
fn hostile_chamfer_params_are_refused_without_panics() {
    let tool = chamfer_mill(6.0, 0.0, 90.0);
    let region = [rect(-10.0, -10.0, 10.0, 10.0)];
    let p = chamfer_params(1.0, Side::Outside);
    let bad = [
        ChamferParams { tolerance: f64::NAN, ..p },
        ChamferParams { tolerance: -1.0, ..p },
        ChamferParams { tolerance: 0.0, ..p },
        ChamferParams { clearance: f64::NAN, ..p },
        ChamferParams { clearance: f64::INFINITY, ..p },
        ChamferParams { retract: f64::NAN, ..p },
        ChamferParams { clearance: 1.0, retract: 2.0, ..p },
        ChamferParams { feeds: Feeds { cut: f64::NAN, plunge: 100.0 }, ..p },
        ChamferParams { feeds: Feeds { cut: 800.0, plunge: 0.0 }, ..p },
        ChamferParams { feeds: Feeds { cut: -1.0, plunge: 100.0 }, ..p },
        ChamferParams { width: f64::MIN_POSITIVE, tip_offset: f64::NAN, ..p },
    ];
    for b in bad {
        assert!(chamfer(&region, &tool, &b).is_err(), "{b:?}");
    }
    // Hostile tools: no angle, angles out of range, a tip as wide as the tool, NaN.
    let mut tools = Vec::new();
    for (angle, tip) in
        [(None, Some(0.0)), (Some(0.0), None), (Some(180.0), None), (Some(f64::NAN), None), (Some(90.0), Some(6.0)), (Some(90.0), Some(f64::NAN))]
    {
        let mut t = chamfer_mill(6.0, 0.0, 90.0);
        t.angle_deg = angle;
        t.tip_diameter = tip;
        tools.push(t);
    }
    let mut t = chamfer_mill(6.0, 0.0, 90.0);
    t.diameter = f64::NAN;
    tools.push(t);
    for t in &tools {
        assert!(t.validate().is_err(), "{t:?}");
        assert!(chamfer(&region, t, &p).is_err(), "{t:?}");
    }
    // An empty region and degenerate or huge profiles are refused, not panicked on.
    assert!(chamfer(&[], &tool, &p).is_err());
    assert!(chamfer(&[rect(0.0, 0.0, 0.0, 0.0)], &tool, &p).is_err());
    assert!(chamfer(&[rect(-1e300, -1e300, 1e300, 1e300)], &tool, &p).is_err());
    // The cutter's own checks hold on hostile shapes.
    for (r, tip, half) in
        [(3.0, 3.0, 0.5), (3.0, -0.1, 0.5), (3.0, 0.0, 0.0), (3.0, 0.0, 1.6), (3.0, 0.0, f64::NAN), (f64::NAN, 0.0, 0.5), (0.0, 0.0, 0.5)]
    {
        assert!(Cutter::tapered(r, tip, half).is_err(), "{r} {tip} {half}");
    }
    let c = Cutter::tapered(3.0, 0.0, 0.5).unwrap();
    for by in [f64::NAN, -1.0, f64::INFINITY] {
        assert!(c.grown(by).is_err() && c.shrunk(by).is_err(), "{by}");
    }
}

#[test]
fn roughing_refuses_a_chamfer_mill_and_a_face_mill_ramps_in() {
    // A chamfer mill's point leaves ridges between roughing passes a diameter apart, which the
    // roughing takes as cut: refused, on three axes and on four. A face mill cannot plunge into
    // the stock, so it neither roughs nor enters a contour or a pocket without a ramp.
    let cone = chamfer_mill(6.0, 0.0, 90.0);
    let face = face_mill(10.0, Some(0.2));
    let s = block(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 0.0));
    let rough = RoughingParams {
        heights: Heights { clearance: 10.0, retract: 2.0, top: 1.0, bottom: -5.0 },
        feeds: Feeds { cut: 800.0, plunge: 100.0 },
        stepdown: 1.0,
        stepover: 2.4,
        angle: 0.0,
        boundary: Boundary { rect: Some([Vec2::new(-15.0, -15.0), Vec2::new(15.0, 15.0)]), region: None },
        stock_to_leave: 0.25,
        step: 1.0,
        tolerance: 0.01,
        zigzag: true,
    };
    assert!(roughing(&s, &Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0), &rough).is_ok());
    for t in [&cone, &face] {
        let e = roughing(&s, t, &rough).unwrap_err().to_string();
        assert!(e.contains("rough with an end mill"), "{e}");
    }
    let drum = crate::foundation_tests::cylinder_x(10.0, -20.0, 20.0, 64);
    let rr = RotaryRoughingParams {
        pattern: RotaryPattern::Lines { angle_step: 0.2, zigzag: true },
        stock_radius: 15.0,
        clearance: 25.0,
        retract: 18.0,
        floor: 8.0,
        stepdown: 2.0,
        feeds: Feeds { cut: 800.0, plunge: 100.0 },
        range: Some([-20.0, 20.0]),
        step: 1.0,
        tolerance: 0.01,
        stock_to_leave: 0.5,
        max_angle_step: DEFAULT_MAX_ANGLE_STEP,
    };
    for t in [&cone, &face] {
        let e = rotary_roughing(&drum, t, &rr).unwrap_err().to_string();
        assert!(e.contains("rough with an end mill"), "{e}");
    }
    // A face mill ramps into a pocket or a contour; straight down it is refused.
    let region = [rect(-10.0, -10.0, 10.0, 10.0)];
    let cut =
        Cut { heights: Heights { clearance: 10.0, retract: 2.0, top: 0.5, bottom: 0.0 }, feeds: Feeds { cut: 800.0, plunge: 50.0 }, stepdown: 0.5 };
    let pp = PocketParams { stepover: 7.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let e = pocket(&region, &face, &pp, &cut).unwrap_err().to_string();
    assert!(e.contains("ramp"), "{e}");
    assert!(pocket(&region, &face, &PocketParams { ramp_angle: 2f64.to_radians(), ..pp }, &cut).is_ok());
    let cp = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(contour(&region, &face, &cp, &cut).unwrap_err().to_string().contains("ramp"));
    assert!(contour(&region, &face, &ContourParams { ramp_angle: 2f64.to_radians(), ..cp }, &cut).is_ok());
    // Without corner radii a face mill is refused the same way; a flat end mill still plunges.
    assert!(pocket(&region, &face_mill(10.0, None), &pp, &cut).is_err());
    assert!(pocket(&region, &Tool::new(1, ToolKind::FlatEndmill, 10.0, 20.0), &pp, &cut).is_ok());
}

#[test]
fn the_gouge_check_follows_a_tall_cone_as_its_axis_turns() {
    // A 10° engraving cone 2 mm across is 11.4 mm tall. Turned 90° about its tip, its side 6 mm
    // up sweeps through a thin fin 11° round from the start; sampled for points only a radius
    // up the tool, the samples (every 22.5°) step over the fin.
    let c = Cutter::tapered(1.0, 0.0, f64::to_radians(5.0)).unwrap();
    let th = f64::to_radians(11.0);
    let u = Vec3::new(th.sin(), 0.0, th.cos());
    let y = Vec3::new(0.0, 1.0, 0.0);
    let fin =
        Surface::new(vec![[u * 4.0 - y * 0.3, u * 8.0 - y * 0.3, u * 8.0 + y * 0.3], [u * 4.0 - y * 0.3, u * 8.0 + y * 0.3, u * 4.0 + y * 0.3]])
            .unwrap();
    let tip = Vec3::new(0.0, 0.0, 0.0);
    // Standing at the fin's angle, the cone is deep in it.
    let at_fin = Toolpath5 { moves: vec![Move5::Rapid { to: Pose::new(tip, u) }, Move5::Feed { to: Pose::new(tip, u), feed: 100.0 }] };
    assert!(!gouges(&fin, &c, &at_fin, 0.01, 0.5).unwrap().is_empty());
    let turn = Toolpath5 {
        moves: vec![
            Move5::Rapid { to: Pose::new(tip, Vec3::new(0.0, 0.0, 1.0)) },
            Move5::Feed { to: Pose::new(tip, Vec3::new(1.0, 0.0, 0.0)), feed: 100.0 },
        ],
    };
    assert!(!gouges(&fin, &c, &turn, 0.01, 0.5).unwrap().is_empty(), "the turn through the fin is a gouge");
}

#[test]
fn the_band_check_allows_the_chamfer_it_is_told_to_cut_and_nothing_beyond() {
    // A chamfer the model does not show: the cone cuts the sharp edge as deep as the chamfer
    // band, which the band check allows; a profile off the part's edge, or a chamfer wider than
    // the band it is told to cut, still gouges.
    let sharp = block(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 0.0));
    for tool in [chamfer_mill(6.0, 0.0, 90.0), chamfer_mill(6.0, 0.5, 90.0), chamfer_mill(8.0, 1.0, 60.0)] {
        let g = chamfer_geometry(&tool, 1.0, DEFAULT_TIP_OFFSET).unwrap();
        let path = chamfer(&[rect(-10.0, -10.0, 10.0, 10.0)], &tool, &chamfer_params(1.0, Side::Outside)).unwrap();
        let path5 = Toolpath5::from_3axis(&path, 0.001).unwrap();
        assert!(!gouges(&sharp, &tool.cutter().unwrap(), &path5, 0.01, 0.2).unwrap().is_empty(), "the plain check finds the chamfer");
        let hits = band_gouges(&sharp, &tool, &path5, g.height, true, 0.01, 0.2).unwrap();
        assert!(hits.is_empty(), "{hits:?}");
        // The modelled chamfer passes too.
        assert!(band_gouges(&chamfered_block(10.0, 10.0, 1.0), &tool, &path5, g.height, true, 0.01, 0.2).unwrap().is_empty());
        // 0.3 mm wider than the band: found, about as deep as the cone goes past it.
        let wider = chamfer(&[rect(-10.0, -10.0, 10.0, 10.0)], &tool, &chamfer_params(1.3, Side::Outside)).unwrap();
        let hits = band_gouges(&sharp, &tool, &Toolpath5::from_3axis(&wider, 0.001).unwrap(), g.height, true, 0.01, 0.2).unwrap();
        let worst = hits.iter().map(|h| h.distance).fold(0.0, f64::max);
        let half = f64::to_radians(tool.angle_deg.unwrap() * 0.5);
        assert!(worst > 0.5 * 0.3 * half.cos() && worst < 0.3 + 0.05, "{worst}");
        // Reported where the tool really runs, not where the check lifted it.
        let lowest = hits.iter().map(|h| h.pose.tip.z).fold(f64::INFINITY, f64::min);
        assert!(lowest < -1.0, "{lowest}");
        // A profile 2 mm inside the edge: the tip runs below the top face.
        let inset = chamfer(&[rect(-8.0, -8.0, 8.0, 8.0)], &tool, &chamfer_params(1.0, Side::Outside)).unwrap();
        let hits = band_gouges(&sharp, &tool, &Toolpath5::from_3axis(&inset, 0.001).unwrap(), g.height, true, 0.01, 0.2).unwrap();
        assert!(hits.iter().any(|h| h.distance > 0.1), "{hits:?}");
    }
    // A band of 0 is the plain check; hostile bands are refused.
    let tool = chamfer_mill(6.0, 0.0, 90.0);
    let path = chamfer(&[rect(-10.0, -10.0, 10.0, 10.0)], &tool, &chamfer_params(1.0, Side::Outside)).unwrap();
    let path5 = Toolpath5::from_3axis(&path, 0.001).unwrap();
    assert!(!band_gouges(&sharp, &tool, &path5, 0.0, true, 0.01, 0.2).unwrap().is_empty());
    for band in [-1.0, f64::NAN, f64::INFINITY, 1e300] {
        assert!(band_gouges(&sharp, &tool, &path5, band, true, 0.01, 0.2).is_err(), "{band}");
    }
}

/// A spot drilled with `tool` at the origin, from the top at Z 0 down to `bottom`.
fn spot(tool: &Tool, bottom: f64) -> Result<Toolpath> {
    use crate::drill::{Cycle, DrillParams, drill};
    let params = DrillParams {
        cycle: Cycle::Simple,
        heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom },
        feed: 100.0,
        to_full_diameter: false,
        stay_at_retract: false,
        part_top: None,
    };
    drill(&[Vec2::new(0.0, 0.0)], tool, &params)
}

#[test]
fn a_chamfer_mill_spots_holes_and_the_band_check_allows_the_spot() {
    // A drilling operation takes a chamfer mill (a spot drill) as it takes a drill. The spot's
    // band (its depth) lets the true cone cut the top it is told to; a spot deeper than the band
    // does not pass.
    let tool = chamfer_mill(6.0, 0.0, 60.0);
    assert!(crate::drill::check_drilling_tool(&tool).is_ok());
    assert!(crate::drill::check_drilling_tool(&Tool::new(1, ToolKind::BallEndmill, 6.0, 20.0)).is_err());
    let path = spot(&tool, -0.4).unwrap();
    let cuts: Vec<Vec3> = path.moves.iter().filter(|m| matches!(m, Move::Feed { .. })).filter_map(Move::target).collect();
    assert!(cuts.iter().all(|p| (p.z + 0.4).abs() < 1e-9 && p.x.abs() < 1e-12 && p.y.abs() < 1e-12), "{cuts:?}");
    let flat = block(Vec3::new(-20.0, -20.0, -10.0), Vec3::new(20.0, 20.0, 0.0));
    let p5 = Toolpath5::from_3axis(&path, 0.001).unwrap();
    assert!(!gouges(&flat, &tool.cutter().unwrap(), &p5, 0.01, 0.2).unwrap().is_empty(), "the plain check finds the spot");
    assert!(band_gouges(&flat, &tool, &p5, 0.4, false, 0.01, 0.2).unwrap().is_empty());
    assert!(!band_gouges(&flat, &tool, &p5, 0.2, false, 0.01, 0.2).unwrap().is_empty());
}

#[test]
fn a_spot_deeper_than_the_cone_is_refused() {
    // Below its cone a chamfer mill cuts with its straight side, a hole as wide as the tool:
    // not a spot, and deeper than any band check could hold (the band lets the tool cut
    // everything down to its depth, a wall it breaks into below the face included).
    let tool = chamfer_mill(6.0, 0.0, 90.0); // its cone is 3 mm high
    assert!(spot(&tool, -3.0).is_ok());
    let e = spot(&tool, -4.0).unwrap_err().to_string();
    assert!(e.contains("cone") && e.contains("3 mm"), "{e}");
    // A tipped 60° cone: (4 - 0.5) / tan 30° high.
    let tipped = chamfer_mill(8.0, 1.0, 60.0);
    let high = 3.5 / 30f64.to_radians().tan();
    assert!(spot(&tipped, -(high - 0.01)).is_ok());
    assert!(spot(&tipped, -(high + 0.01)).is_err());
}

#[test]
fn a_chamfer_mill_traces_a_line_to_a_depth() {
    // V-engraving: a contour on the profile with a chamfer mill runs its tip along the line at
    // the depth asked; beside the line (outside or inside) it is still refused.
    let tool = chamfer_mill(6.0, 0.0, 60.0);
    let region = [rect(-10.0, -5.0, 10.0, 5.0)];
    let cut =
        Cut { heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -0.4 }, feeds: Feeds { cut: 600.0, plunge: 100.0 }, stepdown: 1.0 };
    let on = ContourParams { side: Side::On, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let path = contour(&region, &tool, &on, &cut).unwrap();
    let cuts: Vec<Vec3> = path.moves.iter().filter(|m| matches!(m, Move::Feed { .. })).filter_map(Move::target).collect();
    assert!(cuts.iter().all(|p| p.z >= -0.4 - 1e-9), "{cuts:?}");
    assert!(cuts.iter().any(|p| (p.x - 10.0).abs() < 1e-9 && (p.z + 0.4).abs() < 1e-9), "{cuts:?}");
    for side in [Side::Outside, Side::Inside] {
        let e = contour(&region, &tool, &ContourParams { side, ..on }, &cut).unwrap_err().to_string();
        assert!(e.contains("tapered"), "{e}");
    }
    // Deeper than its flutes: refused.
    let deep = Cut { heights: Heights { bottom: -11.0, ..cut.heights }, ..cut };
    assert!(contour(&region, &tool, &on, &deep).is_err());
    // The trace's band (its depth) lets the true cone cut the top it is told to; a path that
    // goes deeper than the band does not pass.
    let flat = block(Vec3::new(-20.0, -20.0, -10.0), Vec3::new(20.0, 20.0, 0.0));
    let p5 = Toolpath5::from_3axis(&path, 0.001).unwrap();
    assert!(band_gouges(&flat, &tool, &p5, 0.4, false, 0.01, 0.2).unwrap().is_empty());
    assert!(!band_gouges(&flat, &tool, &p5, 0.2, false, 0.01, 0.2).unwrap().is_empty());
}

#[test]
fn a_trace_deeper_than_the_cone_is_refused() {
    // Below its cone a chamfer mill cuts with its straight side, a slot as wide as the tool: not
    // a V-trace, and deeper than any band check could hold.
    let tool = chamfer_mill(6.0, 0.0, 90.0); // its cone is 3 mm high
    let region = [rect(-10.0, -5.0, 10.0, 5.0)];
    let on = ContourParams { side: Side::On, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let cut = |top: f64, bottom: f64| Cut {
        heights: Heights { clearance: 10.0, retract: 2.0, top, bottom },
        feeds: Feeds { cut: 600.0, plunge: 100.0 },
        stepdown: 1.0,
    };
    assert!(contour(&region, &tool, &on, &cut(0.0, -3.0)).is_ok());
    let e = contour(&region, &tool, &on, &cut(0.0, -3.5)).unwrap_err().to_string();
    assert!(e.contains("cone") && e.contains("3 mm"), "{e}");
    // A top above the face makes the band no deeper than the cone either.
    assert!(contour(&region, &tool, &on, &cut(2.0, -1.5)).is_err());
    // A tipped 60° cone: (4 - 0.5) / tan 30° high.
    let tipped = chamfer_mill(8.0, 1.0, 60.0);
    let high = 3.5 / 30f64.to_radians().tan();
    assert!(contour(&region, &tipped, &on, &cut(0.0, -(high - 0.01))).is_ok());
    assert!(contour(&region, &tipped, &on, &cut(0.0, -(high + 0.01))).is_err());
}
