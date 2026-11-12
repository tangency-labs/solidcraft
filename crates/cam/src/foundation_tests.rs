//! Tests of frames, cutters, surfaces, the drop-cutter and multi-axis toolpaths, and the test
//! surfaces they share.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use super::*;

pub(crate) fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} is not within {tol} of {b}");
}

pub(crate) fn close3(a: Vec3, b: Vec3, tol: f64) {
    assert!(a.dist(b) <= tol, "{a:?} is not within {tol} of {b:?}");
}

/// A square `[-s, s]²` at height `z`, as two triangles facing up.
pub(crate) fn plane(s: f64, z: f64) -> Surface {
    let p = |x: f64, y: f64| Vec3::new(x, y, z);
    Surface::new(vec![[p(-s, -s), p(s, -s), p(s, s)], [p(-s, -s), p(s, s), p(-s, s)]]).unwrap()
}

/// The plane `z = x·tan(slope)` over `[-s, s]²`.
pub(crate) fn slope(s: f64, slope: f64) -> Surface {
    let m = slope.tan();
    let p = |x: f64, y: f64| Vec3::new(x, y, x * m);
    Surface::new(vec![[p(-s, -s), p(s, -s), p(s, s)], [p(-s, -s), p(s, s), p(-s, s)]]).unwrap()
}

/// A sphere of radius `r` about `c`, `n` bands from pole to pole, wound outwards, with exact
/// vertex normals.
pub(crate) fn sphere_mesh(c: Vec3, r: f64, n: usize) -> Mesh {
    let (rings, segs) = (n, 2 * n);
    let mut m = Mesh::default();
    for i in 0..=rings {
        let phi = PI * i as f64 / rings as f64;
        for j in 0..segs {
            let th = TAU * j as f64 / segs as f64;
            let d = Vec3::new(phi.sin() * th.cos(), phi.sin() * th.sin(), phi.cos());
            m.positions.push(c + d * r);
            m.normals.push(d);
        }
    }
    let at = |i: usize, j: usize| (i * segs + j % segs) as u32;
    for i in 0..rings {
        for j in 0..segs {
            let (a, b, cc, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            m.triangles.push([a, b, cc]);
            m.triangles.push([a, cc, d]);
            m.tri_face.push(0);
            m.tri_face.push(0);
        }
    }
    m
}

pub(crate) fn sphere(c: Vec3, r: f64, n: usize) -> Surface {
    Surface::from_mesh(&sphere_mesh(c, r, n)).unwrap()
}

/// The outside of a cylinder of radius `r` about the X axis from `x0` to `x1`, `n` facets
/// round, with exact vertex normals.
pub(crate) fn cylinder_x(r: f64, x0: f64, x1: f64, n: usize) -> Surface {
    let mut m = Mesh::default();
    for j in 0..n {
        let th = TAU * j as f64 / n as f64;
        let d = Vec3::new(0.0, th.cos(), th.sin());
        for x in [x0, x1] {
            m.positions.push(Vec3::new(x, 0.0, 0.0) + d * r);
            m.normals.push(d);
        }
    }
    for j in 0..n {
        let k = (j + 1) % n;
        let (a, b, c, d) = ((2 * j) as u32, (2 * j + 1) as u32, (2 * k + 1) as u32, (2 * k) as u32);
        m.triangles.push([a, d, c]);
        m.triangles.push([a, c, b]);
        m.tri_face.push(0);
        m.tri_face.push(0);
    }
    Surface::from_mesh(&m).unwrap()
}

/// `t` wound so that its normal points along `outward`.
pub(crate) fn facing(t: [Vec3; 3], outward: Vec3) -> [Vec3; 3] {
    if (t[1] - t[0]).cross(t[2] - t[0]).dot(outward) < 0.0 { [t[0], t[2], t[1]] } else { t }
}

/// A round bar of radius `r` about X from `x0` to `x1` with a flat at height `h` along Z over
/// its whole length, closed at its ends, wound outwards.
pub(crate) fn flatted_bar(r: f64, h: f64, x0: f64, x1: f64) -> Surface {
    let edge = (h / r).acos();
    let mut angles: Vec<f64> = (0..720).map(|k| -PI + TAU * k as f64 / 720.0).filter(|a| (a.abs() - edge).abs() > 1e-6).collect();
    angles.extend([-edge, edge]);
    angles.sort_by(f64::total_cmp);
    let at = |x: f64, a: f64| {
        let rho = if a.abs() <= edge + 1e-12 { h / a.cos() } else { r };
        Vec3::new(x, rho * a.sin(), rho * a.cos())
    };
    let mut tris = Vec::new();
    let n = angles.len();
    for k in 0..n {
        let (a, b) = (angles[k], angles[(k + 1) % n]);
        let out = Vec3::new(0.0, (0.5 * (a + b)).sin(), (0.5 * (a + b)).cos());
        let (p, q, rr, s) = (at(x0, a), at(x1, a), at(x1, b), at(x0, b));
        tris.push(facing([p, q, rr], out));
        tris.push(facing([p, rr, s], out));
        for (x, sign) in [(x0, -1.0), (x1, 1.0)] {
            tris.push(facing([Vec3::new(x, 0.0, 0.0), at(x, a), at(x, b)], Vec3::new(sign, 0.0, 0.0)));
        }
    }
    Surface::new(tris).unwrap()
}

/// A square bar from -20 to 20 in Y and Z, `x0` to `x1` along X, with a slot 10 wide and 5 deep
/// along its whole length in its +Z face, wound outwards (its ends open: no ray of a simulation
/// reaches them).
pub(crate) fn slotted_bar(x0: f64, x1: f64) -> Surface {
    let section = [(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (5.0, 20.0), (5.0, 15.0), (-5.0, 15.0), (-5.0, 20.0), (-20.0, 20.0)];
    let mut tris = Vec::new();
    for k in 0..section.len() {
        let ((y0, z0), (y1, z1)) = (section[k], section[(k + 1) % section.len()]);
        // The section runs counter-clockwise seen from +X: outwards is to the right of each edge.
        let out = Vec3::new(0.0, z1 - z0, -(y1 - y0));
        let (p, q, r, s) = (Vec3::new(x0, y0, z0), Vec3::new(x1, y0, z0), Vec3::new(x1, y1, z1), Vec3::new(x0, y1, z1));
        tris.push(facing([p, q, r], out));
        tris.push(facing([p, r, s], out));
    }
    Surface::new(tris).unwrap()
}

/// A closed box from `lo` to `hi`, wound outwards.
pub(crate) fn block(lo: Vec3, hi: Vec3) -> Surface {
    let p = |i: usize| Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut tris = Vec::new();
    for q in quads {
        tris.push([p(q[0]), p(q[1]), p(q[2])]);
        tris.push([p(q[0]), p(q[2]), p(q[3])]);
    }
    Surface::new(tris).unwrap()
}

/// The frame tilted from the world by `a` about X then `b` about the turned Y, at `origin`.
pub(crate) fn tilted(origin: Vec3, a: f64, b: f64) -> Frame {
    let x = Vec3::new(1.0, 0.0, 0.0);
    let y = rotate(Vec3::new(0.0, 1.0, 0.0), x, a);
    let z = rotate(Vec3::new(0.0, 0.0, 1.0), x, a);
    Frame { origin, x: rotate(x, y, b), y, z: rotate(z, y, b) }
}

#[test]
fn frames_go_both_ways() {
    let f = tilted(Vec3::new(3.0, -2.0, 7.0), 0.4, -1.1);
    assert!(f.is_orthonormal());
    let p = Vec3::new(1.5, 2.5, -4.0);
    close3(f.to_world(f.to_local(p)), p, 1e-12);
    close3(f.dir_to_world(f.dir_to_local(p)), p, 1e-12);
    let inv = f.inverse();
    close3(inv.to_world(p), f.to_local(p), 1e-12);
    let g = tilted(Vec3::new(-1.0, 0.0, 2.0), -0.3, 0.8);
    close3(f.compose(&g).to_world(p), f.to_world(g.to_world(p)), 1e-12);
    let z = Frame::from_z(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)).unwrap();
    assert!(z.is_orthonormal());
    assert!(Frame::from_z(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)).is_err());
    assert!(Frame::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 2.0)).is_err());
    close(angle_about(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 5.0), Vec3::new(0.0, 0.0, 1.0)).unwrap(), FRAC_PI_2, 1e-12);
    assert!(angle_about(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)).is_none());
    close3(rotate_about(Vec3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), PI), Vec3::new(0.0, 0.0, 0.0), 1e-12);
}

#[test]
fn cutter_profiles() {
    let flat = Cutter::flat(5.0).unwrap();
    let ball = Cutter::ball(5.0).unwrap();
    let bull = Cutter::bull(5.0, 1.0).unwrap();
    assert_eq!(flat.height(4.9), Some(0.0));
    assert_eq!(flat.height(5.1), None);
    close(ball.height(3.0).unwrap(), 1.0, 1e-12);
    close(ball.height(5.0).unwrap(), 5.0, 1e-12);
    assert_eq!(bull.height(3.9), Some(0.0));
    close(bull.height(5.0).unwrap(), 1.0, 1e-12);
    assert!(Cutter::bull(5.0, 6.0).is_err());
    assert!(Cutter::flat(f64::NAN).is_err());
    let grown = ball.grown(0.5).unwrap();
    assert!(grown.is_ball() && (grown.radius - 5.5).abs() < 1e-12);
    assert!(ball.grown(-1.0).is_err());

    let mut t = Tool::new(3, ToolKind::BullNose, 10.0, 20.0);
    assert!(t.validate().is_err(), "a bull nose needs its corner radius");
    t.corner_radius = Some(1.5);
    t.validate().unwrap();
    assert_eq!(t.cutter().unwrap(), Cutter::bull(5.0, 1.5).unwrap());
    t.corner_radius = Some(5.0);
    assert!(t.validate().is_err(), "a bull nose's corner is below its radius");
    assert!(Tool::new(1, ToolKind::Drill, 6.0, 30.0).cutter().is_err());
    assert_eq!(Tool::new(1, ToolKind::BallEndmill, 6.0, 20.0).cutter().unwrap(), Cutter::ball(3.0).unwrap());
    // A chamfer mill needs its included angle; its tip is below its diameter.
    let mut c = Tool::new(2, ToolKind::Chamfer, 12.0, 10.0);
    assert!(c.validate().is_err() && c.cutter().is_err());
    (c.angle_deg, c.tip_diameter) = (Some(90.0), Some(1.0));
    c.validate().unwrap();
    assert_eq!(c.cutter().unwrap(), Cutter::tapered(6.0, 0.5, FRAC_PI_4).unwrap());
    c.tip_diameter = Some(12.0);
    assert!(c.validate().is_err());
    let mut flat = Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0);
    flat.angle_deg = Some(90.0);
    assert!(flat.validate().is_err(), "only a chamfer mill has an included angle");
    // An old tool without the new fields still reads, and is written as before.
    let json = r#"{"number":1,"kind":"flat_endmill","diameter":6.0,"flute_length":20.0}"#;
    let old: Tool = serde_json::from_str(json).unwrap();
    assert!(old.corner_radius.is_none() && old.tip_diameter.is_none() && old.angle_deg.is_none());
    assert!(!serde_json::to_string(&old).unwrap().contains("corner_radius"));
}

#[test]
fn drop_onto_a_flat_and_a_slope() {
    let p = plane(50.0, 3.0);
    for c in [Cutter::flat(4.0).unwrap(), Cutter::ball(4.0).unwrap(), Cutter::bull(4.0, 1.0).unwrap()] {
        close(drop_z(&p, &c, 7.0, -3.0).unwrap(), 3.0, 1e-9);
        // Half over the edge still sits on the face's edge.
        close(drop_z(&p, &c, 51.0, 0.0).unwrap(), 3.0 - c.height(1.0).unwrap(), 1e-9);
        assert!(drop_z(&p, &c, 60.0, 0.0).is_none());
    }
    let th: f64 = 0.5;
    let s = slope(100.0, th);
    let (r, x) = (4.0, 10.0);
    // A ball's centre is r from the plane: its tip r/cos − r above the plane under its axis.
    let ball = Cutter::ball(r).unwrap();
    close(drop_z(&s, &ball, x, 2.0).unwrap(), x * th.tan() + r / th.cos() - r, 1e-9);
    // A flat end rests on its rim's highest point.
    let flat = Cutter::flat(r).unwrap();
    close(drop_z(&s, &flat, x, 2.0).unwrap(), (x + r) * th.tan(), 1e-9);
    // A bull nose: its corner circle's centre r_c from the plane.
    let (rc, a) = (1.0, 3.0);
    let bull = Cutter::bull(r, rc).unwrap();
    close(drop_z(&s, &bull, x, 2.0).unwrap(), (rc + th.sin() * (x + a)) / th.cos() - rc, 1e-9);
}

#[test]
fn drop_onto_an_edge() {
    // A wall in the plane x = 0 whose top edge is level at z = 10.
    let w = |y: f64, z: f64| Vec3::new(0.0, y, z);
    let wall = Surface::new(vec![[w(-50.0, 0.0), w(50.0, 0.0), w(50.0, 10.0)], [w(-50.0, 0.0), w(50.0, 10.0), w(-50.0, 10.0)]]).unwrap();
    let (r, rc) = (5.0, 2.0);
    for d in [0.0, 1.0, 2.5, 3.5, 4.5, 4.99] {
        let ball = drop_z(&wall, &Cutter::ball(r).unwrap(), d, 7.0).unwrap();
        close(ball, 10.0 + (r * r - d * d).sqrt() - r, 1e-9);
        let bull = drop_z(&wall, &Cutter::bull(r, rc).unwrap(), -d, 7.0).unwrap();
        let a = r - rc;
        let want = if d <= a { 10.0 } else { 10.0 + (rc * rc - (d - a) * (d - a)).sqrt() - rc };
        close(bull, want, 1e-9);
        close(drop_z(&wall, &Cutter::flat(r).unwrap(), d, 7.0).unwrap(), 10.0, 1e-9);
    }
    assert!(drop_z(&wall, &Cutter::ball(r).unwrap(), 5.5, 7.0).is_none());
}

#[test]
fn drop_onto_a_sphere_is_within_its_chords() {
    let (rs, r) = (20.0, 3.0);
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), rs, 64);
    // The facets lie inside the sphere by at most its chord error.
    let chord = rs * (1.0 - (PI / 64.0).cos()) * 2.0;
    let ball = Cutter::ball(r).unwrap();
    for rho in [0.0, 5.0, 12.0, 19.0, 22.5] {
        let want = ((rs + r) * (rs + r) - rho * rho).sqrt() - r;
        let got = drop_z(&s, &ball, rho * 0.6, rho * 0.8).unwrap();
        assert!(got <= want + 1e-9 && got >= want - chord, "ρ {rho}: {got} vs {want}");
    }
}

#[test]
fn drop_along_any_axis_agrees_with_dropping_down() {
    let s = sphere(Vec3::new(1.0, 2.0, -3.0), 15.0, 32);
    let f = tilted(Vec3::new(5.0, -4.0, 2.0), 0.7, -0.4);
    // The same surface turned into a tilted frame, dropped along that frame's Z.
    let turned = Surface::new(s.triangles().iter().map(|t| t.map(|p| f.to_world(p))).collect()).unwrap();
    for c in [Cutter::flat(4.0).unwrap(), Cutter::ball(4.0).unwrap(), Cutter::bull(4.0, 1.5).unwrap()] {
        for (x, y) in [(0.0, 0.0), (6.0, -3.0), (-12.0, 9.0)] {
            let down = drop_z(&s, &c, x, y).unwrap();
            let along = drop_along(&turned, &c, f.to_world(Vec3::new(x, y, 0.0)), f.z).unwrap().unwrap();
            close(along, down, 1e-7);
            // The same with the axis given through another point on the line.
            let higher = drop_along(&turned, &c, f.to_world(Vec3::new(x, y, 40.0)), f.z * 3.0).unwrap().unwrap();
            close(higher, down - 40.0, 1e-7);
        }
    }
    let ball = Cutter::ball(4.0).unwrap();
    assert!(drop_along(&turned, &ball, f.to_world(Vec3::new(100.0, 0.0, 0.0)), f.z).unwrap().is_none());
    assert!(drop_along(&turned, &ball, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)).is_err());
    assert!(drop_along(&turned, &ball, Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)).is_err());
    // From underneath, the tool stops below the sphere.
    let under = drop_along(&s, &ball, Vec3::new(1.0, 2.0, -3.0), Vec3::new(0.0, 0.0, -1.0)).unwrap().unwrap();
    close(under, 15.0, 0.1);
}

#[test]
fn surfaces_from_meshes() {
    let m = sphere_mesh(Vec3::new(0.0, 0.0, 0.0), 10.0, 16);
    let s = Surface::from_mesh(&m).unwrap();
    assert!(!s.is_empty() && s.len() <= m.triangles.len(), "the pole's slivers are left out");
    let b = s.bounds();
    close(b.max.z, 10.0, 1e-9);
    // A ray from far above meets the top.
    let (t, i) = s.raycast(Vec3::new(0.3, 0.2, 50.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
    let hit = Vec3::new(0.3, 0.2, 50.0 - t);
    assert!(hit.z > 9.5);
    let n = s.normal_at(i, hit).unwrap();
    assert!(n.z > 0.99, "the blended normal points out: {n:?}");
    let mut bad = m.clone();
    bad.triangles.push([0, 1, 999_999]);
    assert!(Surface::from_mesh(&bad).is_err());
    assert!(Surface::new(vec![[Vec3::new(f64::INFINITY, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0)]]).is_err());
    let f = tilted(Vec3::new(1.0, 1.0, 1.0), 0.3, 0.2);
    let local = s.in_frame(&f).unwrap();
    close3(local.bounds().center(), f.to_local(Vec3::new(0.0, 0.0, 0.0)), 0.5);
    // Geometry helpers.
    let tri = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0), Vec3::new(0.0, 4.0, 0.0)];
    close3(nearest_on_triangle(Vec3::new(1.0, 1.0, 5.0), &tri), Vec3::new(1.0, 1.0, 0.0), 1e-12);
    close3(nearest_on_triangle(Vec3::new(-2.0, -2.0, 0.0), &tri), tri[0], 1e-12);
    let w = barycentric(&tri, Vec3::new(1.0, 2.0, 0.0)).unwrap();
    close(w[0] + w[1] + w[2], 1.0, 1e-12);
}

#[test]
fn multiaxis_paths_from_3axis_ones() {
    let square =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 20.0), Vec2::new(0.0, 20.0)]), holes: vec![] };
    let tool = Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0);
    let cut =
        Cut { heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -3.0 }, feeds: Feeds { cut: 600.0, plunge: 200.0 }, stepdown: 1.5 };
    let path =
        contour(&[square], &tool, &ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.05 }, &cut)
            .unwrap();
    let p5 = Toolpath5::from_3axis(&path, 0.001).unwrap();
    verify5(&p5, 1e-9).unwrap();
    let (a, b) = (path.stats(), p5.stats());
    close(b.cut_length, a.cut_length, 0.05);
    close(b.cut_minutes, a.cut_minutes, 0.001);
    assert_eq!(b.max_tilt, 0.0);
    // Tilted, the tips are the tilted frame's and the axis its Z.
    let f = tilted(Vec3::new(10.0, 0.0, 5.0), 0.5, 0.0);
    let t5 = Toolpath5::from_frame(&path, &f, 0.001).unwrap();
    close(t5.stats().max_tilt, 0.5, 1e-12);
    for (pose, local) in t5.poses().zip(p5.poses()) {
        close3(pose.tip, f.to_world(local.tip), 1e-9);
        close3(pose.axis, f.z, 1e-12);
    }
    // The checks catch what must not leave.
    let start = Move5::Rapid { to: Pose::vertical(Vec3::new(0.0, 0.0, 10.0)) };
    let tilt = |a: f64| Pose::new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(a.sin(), 0.0, a.cos()));
    assert!(verify5(&Toolpath5 { moves: vec![start, Move5::Feed { to: tilt(0.2), feed: 100.0 }] }, 0.1).is_err());
    verify5(&Toolpath5 { moves: vec![start, Move5::Feed { to: tilt(0.05), feed: 100.0 }] }, 0.1).unwrap();
    assert!(verify5(&Toolpath5 { moves: vec![Move5::Feed { to: tilt(0.0), feed: 100.0 }] }, 0.1).is_err());
    let bad = Pose::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 2.0));
    assert!(verify5(&Toolpath5 { moves: vec![Move5::Rapid { to: bad }] }, 0.1).is_err());
    assert!(verify5(&Toolpath5 { moves: vec![start, Move5::Feed { to: tilt(0.0), feed: 0.0 }] }, 0.1).is_err());
    // Turning evenly between two directions.
    let (x, z) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let mid = slerp(z, x, 0.5);
    close3(mid, Vec3::new(1.0, 0.0, 1.0).normalized().unwrap(), 1e-12);
    close((slerp(z, -z, 0.5).len() - 1.0).abs(), 0.0, 1e-12);
    let json = serde_json::to_string(&t5).unwrap();
    assert_eq!(serde_json::from_str::<Toolpath5>(&json).unwrap(), t5);
}

#[test]
fn drop_onto_test_shapes() {
    let b = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let ball = Cutter::ball(3.0).unwrap();
    close(drop_z(&b, &ball, 0.0, 0.0).unwrap(), 5.0, 1e-9);
    // Past the side, the ball rolls over the top edge.
    close(drop_z(&b, &ball, 12.0, 0.0).unwrap(), 5.0 + (9.0f64 - 4.0).sqrt() - 3.0, 1e-9);
    // A cylinder about X: dropped radially at any angle, the ball stops r_cyl + r from the axis.
    let c = cylinder_x(20.0, -50.0, 50.0, 256);
    for a in [0.0, 0.7, 2.0, -2.5] {
        let dir = Vec3::new(0.0, f64::cos(a), f64::sin(a));
        let t = drop_along(&c, &ball, Vec3::new(5.0, 0.0, 0.0), dir).unwrap().unwrap();
        assert!((20.0 - 0.01..=20.0 + 1e-9).contains(&t), "{t}");
    }
}

/// Random triangles against a brute-force drop: the highest tip touching any of a fine grid of
/// points on the triangle. The drop must never be below it (a gouge) and only above it by the
/// grid's own coarseness.
#[test]
fn drop_triangle_matches_brute_force() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = |lo: f64, hi: f64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        lo + (hi - lo) * ((seed >> 11) as f64 / (1u64 << 53) as f64)
    };
    let cutters = [Cutter::flat(3.0).unwrap(), Cutter::ball(3.0).unwrap(), Cutter::bull(3.0, 1.0).unwrap()];
    let n = 120;
    for _ in 0..150 {
        let t = [0, 1, 2].map(|_| Vec3::new(rnd(-6.0, 6.0), rnd(-6.0, 6.0), rnd(-4.0, 4.0)));
        let (x, y) = (rnd(-4.0, 4.0), rnd(-4.0, 4.0));
        for c in &cutters {
            let mut brute = f64::NEG_INFINITY;
            for i in 0..=n {
                for j in 0..=(n - i) {
                    let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
                    let p = t[0] + (t[1] - t[0]) * u + (t[2] - t[0]) * v;
                    if let Some(h) = c.height((p.x - x).hypot(p.y - y)) {
                        brute = brute.max(p.z - h);
                    }
                }
            }
            let got = drop_triangle(c, &t, x, y);
            if !brute.is_finite() {
                // Only a sliver of the triangle may be under the cutter, between grid points.
                continue;
            }
            let got = got.expect("the cutter reaches the triangle");
            assert!(got >= brute - 1e-9, "gouge: {got} below {brute} for {c:?} at ({x}, {y}) on {t:?}");
            assert!(got <= brute + 0.35, "{got} far above {brute} for {c:?} at ({x}, {y}) on {t:?}");
        }
    }
}

/// A drop is the highest of every triangle's to the last bit, and a drop from a hint is the
/// plain drop to the last bit whatever the hint (a little below the
/// answer, on it, above it, wild or not a number), for every cutter shape on a wavy sheet and a
/// sphere, and a hint a little below the answer opens about half the triangles.
#[test]
fn hinted_drops_are_the_plain_drops_and_open_fewer_triangles() {
    use crate::dropcutter::{OPENED, drop_z_from};
    let n = 60;
    let at = |i: usize, j: usize| {
        let (x, y) = (-15.0 + 0.5 * i as f64, -15.0 + 0.5 * j as f64);
        Vec3::new(x, y, 3.0 * (x / 5.0).sin() * (y / 7.0).cos() + 0.02 * ((i * 7 + j * 13) % 5) as f64)
    };
    let mut tris = Vec::new();
    for i in 0..n {
        for j in 0..n {
            tris.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            tris.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    let surfaces = [Surface::new(tris).unwrap(), sphere(Vec3::new(0.0, 0.0, 0.0), 12.0, 40)];
    let cutters = [Cutter::flat(3.0).unwrap(), Cutter::ball(3.0).unwrap(), Cutter::bull(3.0, 1.0).unwrap(), Cutter::tapered(3.0, 0.5, 0.5).unwrap()];
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = |lo: f64, hi: f64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        lo + (hi - lo) * ((seed >> 11) as f64 / (1u64 << 53) as f64)
    };
    let points: Vec<(f64, f64)> = (0..600).map(|_| (rnd(-17.0, 17.0), rnd(-17.0, 17.0))).collect();
    let (mut plain_opened, mut hinted_opened) = (0, 0);
    for s in &surfaces {
        for c in &cutters {
            for &(x, y) in &points {
                OPENED.with(|k| k.set(0));
                let plain = drop_z(s, c, x, y);
                plain_opened += OPENED.with(|k| k.replace(0));
                // Every triangle opened, from the surface straight under the axis.
                let top = s.bounds().max.z + 1.0;
                let under = s.raycast(Vec3::new(x, y, top), Vec3::new(0.0, 0.0, -1.0)).map(|(t, _)| top - t);
                let every = s.triangles().iter().filter_map(|t| drop_triangle(c, t, x, y)).fold(under, |b, z| Some(b.map_or(z, |b| b.max(z))));
                assert_eq!(every.map(f64::to_bits), plain.map(f64::to_bits), "{c:?} at ({x}, {y}), every triangle");
                let z = plain.unwrap_or(0.0);
                OPENED.with(|k| k.set(0));
                let near = drop_z_from(s, c, x, y, z - 1e-3);
                hinted_opened += OPENED.with(|k| k.replace(0));
                assert_eq!(plain.map(f64::to_bits), near.map(f64::to_bits), "{c:?} at ({x}, {y}), hint just below");
                for hint in [z, z + 1e-9, z + 0.5, z - 5.0, rnd(-50.0, 50.0), f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    let hinted = drop_z_from(s, c, x, y, hint);
                    assert_eq!(plain.map(f64::to_bits), hinted.map(f64::to_bits), "{c:?} at ({x}, {y}), hint {hint}");
                }
            }
        }
    }
    println!("triangles opened: {plain_opened} plain, {hinted_opened} from a hint 1 µm below");
    assert!(hinted_opened * 3 < plain_opened * 2, "{hinted_opened} opened from a hint, {plain_opened} without");
}

/// A bull nose passes over the edges whose higher end, less the cutter's height at the edge's
/// nearest distance from the axis, cannot beat the tip already found: every drop stays the
/// same to the last bit, on a wavy sheet and a sphere, for every cutter shape, and a third of
/// the bull-nose edge searches are passed over.
#[test]
fn edge_bound_keeps_every_drop_and_skips_most_edge_searches() {
    use crate::dropcutter::{EDGE_SEARCHES, NO_EDGE_BOUND};
    let n = 60;
    let at = |i: usize, j: usize| {
        let (x, y) = (-15.0 + 0.5 * i as f64, -15.0 + 0.5 * j as f64);
        Vec3::new(x, y, 3.0 * (x / 5.0).sin() * (y / 7.0).cos() + 0.02 * ((i * 7 + j * 13) % 5) as f64)
    };
    let mut tris = Vec::new();
    for i in 0..n {
        for j in 0..n {
            tris.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            tris.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    let surfaces = [Surface::new(tris).unwrap(), sphere(Vec3::new(0.0, 0.0, 0.0), 12.0, 40)];
    let cutters = [Cutter::flat(3.0).unwrap(), Cutter::ball(3.0).unwrap(), Cutter::bull(3.0, 1.0).unwrap(), Cutter::bull(5.0, 0.5).unwrap()];
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = |lo: f64, hi: f64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        lo + (hi - lo) * ((seed >> 11) as f64 / (1u64 << 53) as f64)
    };
    let points: Vec<(f64, f64)> = (0..1500).map(|_| (rnd(-16.0, 16.0), rnd(-16.0, 16.0))).collect();
    let (mut before, mut after) = (0, 0);
    for s in &surfaces {
        for c in &cutters {
            for &(x, y) in &points {
                NO_EDGE_BOUND.with(|f| f.set(true));
                EDGE_SEARCHES.with(|k| k.set(0));
                let old = drop_z(s, c, x, y);
                let old_along = drop_along(s, c, Vec3::new(x, y, 0.0), Vec3::new(0.1, -0.2, 1.0)).unwrap();
                before += EDGE_SEARCHES.with(|k| k.replace(0));
                NO_EDGE_BOUND.with(|f| f.set(false));
                let new = drop_z(s, c, x, y);
                let new_along = drop_along(s, c, Vec3::new(x, y, 0.0), Vec3::new(0.1, -0.2, 1.0)).unwrap();
                after += EDGE_SEARCHES.with(|k| k.replace(0));
                assert_eq!(old.map(f64::to_bits), new.map(f64::to_bits), "{c:?} at ({x}, {y})");
                assert_eq!(old_along.map(f64::to_bits), new_along.map(f64::to_bits), "{c:?} along, at ({x}, {y})");
            }
        }
    }
    println!("bull-nose edge searches: {before} without the bound, {after} with it");
    assert!(before > 1000, "the bull noses searched edges: {before}");
    assert!(after * 20 <= before * 13, "the bound passes over a third of the edge searches: {after} of {before}");
}

/// How long a drop takes on a fine sphere (run with `--ignored --nocapture` in release).
#[test]
#[ignore]
fn drop_speed() {
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 30.0, 136);
    let c = Cutter::ball(3.0).unwrap();
    let b = Cutter::bull(3.0, 1.0).unwrap();
    for (name, c) in [("ball", c), ("bull", b)] {
        let start = std::time::Instant::now();
        let mut k = 0;
        for i in 0..60 {
            for j in 0..60 {
                if drop_z(&s, &c, -30.0 + i as f64, -30.0 + j as f64).is_some() {
                    k += 1;
                }
            }
        }
        println!("{name}: {} triangles, {k} drops, {:.1} µs per drop", s.len(), start.elapsed().as_secs_f64() * 1e6 / 3600.0);
    }
}

/// A wall leaning by a hair, far from the origin: the face test must not accept a contact
/// outside the face and lift the tip far above it.
#[test]
fn near_vertical_faces_far_from_the_origin() {
    for (o, k) in [(100.0, 1e-11), (100.0, 1e-10), (500.0, 1e-9), (500.0, 1e-8), (0.0, 2e-12), (300.0, 1e-9)] {
        let t = [Vec3::new(o, o, 0.0), Vec3::new(o, o + 10.0, 0.0), Vec3::new(o + k * 10.0, o, 10.0)];
        for c in [Cutter::flat(3.0).unwrap(), Cutter::ball(3.0).unwrap(), Cutter::bull(3.0, 1.0).unwrap()] {
            for x in [o - 3.0 + 4.4531e-9, o - 2.5, o - 1.0] {
                if let Some(z) = drop_triangle(&c, &t, x, o + 9.9) {
                    assert!(z <= 10.0 + 1e-9, "tip {z} above the wall's top for {c:?} at x {x}");
                }
            }
        }
    }
    // A hint almost along Z is refused rather than giving a frame that is not square.
    let z = Vec3::new(0.36, 0.48, 0.8);
    assert!(Frame::new(Vec3::new(0.0, 0.0, 0.0), z, z * 1000.0 + Vec3::new(0.8, -0.6, 0.0) * 1e-9).is_err());
    let f = Frame::new(Vec3::new(0.0, 0.0, 0.0), z, z + Vec3::new(0.8, -0.6, 0.0) * 1e-3).unwrap();
    assert!(f.is_orthonormal());
}

/// A ray in a triangle's plane (to rounding) meets it nowhere off the triangle: one along an
/// edge of an end face, as tessellated, was taken to meet it millimetres off it.
#[test]
fn a_ray_in_a_triangles_plane_meets_it_nowhere_off_it() {
    let tri = [
        Vec3::new(30.0, 4.242640687119286, 4.242640687119286),
        Vec3::new(30.0, 2.1213203435596437, 2.1213203435596437),
        Vec3::new(30.000000000000004, 3.283370607972145, 3.0089100015804924),
    ];
    let axis = Vec3::new(0.0, 0.7071067811865475, 0.7071067811865477);
    let tip = Vec3::new(30.0, 4.270647634931788, 4.27064763493179);
    for reach in [10.0, 50.0, 97.3, 120.0, 150.0] {
        let o = tip + axis * reach;
        if let Some(at) = ray_triangle(o, -axis, &tri) {
            let p = o - axis * at;
            let off = p.dist(nearest_on_triangle(p, &tri));
            assert!(off < 1e-9, "from {reach} mm out: met {} mm off the triangle", off);
        }
    }
    // A ray through it at a slant still meets it.
    let (o, d) = (Vec3::new(40.0, 3.2158, 3.1243), Vec3::new(-1.0, 0.0, 0.0));
    assert!(ray_triangle(o, d, &tri).is_some_and(|at| (at - 10.0).abs() < 1e-9));
}
