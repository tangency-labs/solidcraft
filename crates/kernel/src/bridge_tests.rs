//! Tests of the bridge core: the support surface's shape is read off a real body, and a bridge's
//! continuity is measured back off the surface it built.

use super::*;
use crate::{box_solid, cylinder};

/// The shape of a bridge's cross-section at the parameter `u`, measured off the finished face (not
/// from the construction): position, unit tangent, curvature vector and the arclength derivative
/// of that curvature vector at the start of the cross-section. The samples run inwards from the
/// boundary, so nothing is asked of the surface outside its own domain.
fn section_at(body: &Body, u: f64) -> (Vec3, Vec3, Vec3, Vec3) {
    let (surf, [_, [v0, _]]) = the_face(body).unwrap();
    let h = 1e-3;
    let q: Vec<Vec3> = (0..5).map(|k| surf.point([u, v0 + k as f64 * h]).unwrap()).collect();
    let (f0, f1, f2, f3, f4) = (q[0], q[1], q[2], q[3], q[4]);
    let d1 = (f0 * -25.0 + f1 * 48.0 + f2 * -36.0 + f3 * 16.0 + f4 * -3.0) * (1.0 / (12.0 * h));
    let d2 = (f0 * 35.0 + f1 * -104.0 + f2 * 114.0 + f3 * -56.0 + f4 * 11.0) * (1.0 / (12.0 * h * h));
    let d3 = (f0 * -5.0 + f1 * 18.0 + f2 * -24.0 + f3 * 14.0 + f4 * -3.0) * (1.0 / (2.0 * h * h * h));
    let (t, bend, bend_change) = shape(d1, d2, d3).unwrap();
    (f0, t, bend, bend_change)
}

/// The same at the end of the cross-section (`v = v1`), with the samples running back inwards and
/// the odd derivatives turned round.
fn section_end_at(body: &Body, u: f64) -> (Vec3, Vec3, Vec3, Vec3) {
    let (surf, [_, [_, v1]]) = the_face(body).unwrap();
    let h = 1e-3;
    let q: Vec<Vec3> = (0..5).map(|k| surf.point([u, v1 - k as f64 * h]).unwrap()).collect();
    let (f0, f1, f2, f3, f4) = (q[0], q[1], q[2], q[3], q[4]);
    let d1 = (f0 * -25.0 + f1 * 48.0 + f2 * -36.0 + f3 * 16.0 + f4 * -3.0) * (-1.0 / (12.0 * h));
    let d2 = (f0 * 35.0 + f1 * -104.0 + f2 * 114.0 + f3 * -56.0 + f4 * 11.0) * (1.0 / (12.0 * h * h));
    let d3 = (f0 * -5.0 + f1 * 18.0 + f2 * -24.0 + f3 * 14.0 + f4 * -3.0) * (-1.0 / (2.0 * h * h * h));
    let (t, bend, bend_change) = shape(d1, d2, d3).unwrap();
    (f0, t, bend, bend_change)
}

/// The angle between two vectors, in degrees.
fn angle_deg(a: Vec3, b: Vec3) -> f64 {
    (a.normalized().unwrap().dot(b.normalized().unwrap())).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Two planes 10 mm apart: the lower one leaves along +y, the upper one has no curvature, and the
/// direction between them is +z — an S-shaped bridge.
fn two_planes(count: usize) -> (Vec<Frame>, Vec<Frame>) {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for i in 0..count {
        let y = 10.0 * i as f64 / (count - 1) as f64;
        a.push(Frame { p: Vec3::new(0.0, y, 0.0), leave: Vec3::new(0.0, 1.0, 0.0), bend: Vec3::ZERO, bend_change: Vec3::ZERO });
        b.push(Frame { p: Vec3::new(0.0, y, 10.0), leave: Vec3::new(0.0, -1.0, 0.0), bend: Vec3::ZERO, bend_change: Vec3::ZERO });
    }
    (a, b)
}

/// Two parallel circular cylinders of radius `r`, axes along x at z = r, 20 mm apart in y: their
/// lowest lines are the two boundaries. Each leaves towards the other along ∓y, bends up towards
/// its own axis (curvature 1/r), and its curvature does not change as it goes round — what does
/// change is purely tangential.
fn valley_between_cylinders(count: usize, r: f64) -> (Vec<Frame>, Vec<Frame>) {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for i in 0..count {
        let y = 10.0 * i as f64 / (count - 1) as f64;
        a.push(Frame {
            p: Vec3::new(0.0, y, 0.0),
            leave: Vec3::new(0.0, 1.0, 0.0),
            bend: Vec3::new(0.0, 0.0, 1.0 / r),
            bend_change: Vec3::new(0.0, -1.0 / (r * r), 0.0),
        });
        b.push(Frame {
            p: Vec3::new(0.0, y + 20.0, 0.0),
            leave: Vec3::new(0.0, -1.0, 0.0),
            bend: Vec3::new(0.0, 0.0, 1.0 / r),
            bend_change: Vec3::new(0.0, 1.0 / (r * r), 0.0),
        });
    }
    (a, b)
}

/// The edges of `body`'s face `face` chained into one loop (its outer loop, as it runs).
fn loop_of(body: &Body, face: usize) -> Chain {
    let r = body.record();
    let solidcraft_geom::brep::Loop::Edges { edges } = &r.faces[face].loops[0] else { panic!("a vertex loop") };
    edges.iter().map(|u| (u.edge as usize, u.forward)).collect()
}

/// The face of `body` (its index) whose surface contains `at`.
fn face_at(body: &Body, at: Vec3) -> usize {
    let r = body.record();
    r.faces.iter().position(|f| SurfaceEval::new(&f.surface).unwrap().project(at, None, 1e-6).is_some()).unwrap()
}

#[test]
fn frames_read_a_plane_and_a_cylinder_wall() {
    // A 40 x 30 x 20 box: the top face's rim leaves along +x, a plane with no curvature.
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let top = face_at(&b, Vec3::new(20.0, 15.0, 20.0));
    let rim = loop_of(&b, top);
    assert!(chain_closes(&b, &rim));
    let f = frames(&b, top, &rim, Vec3::new(1.0, 0.0, 0.0), 9).unwrap();
    assert_eq!(f.len(), 9);
    for s in &f {
        assert!((s.p.z - 20.0).abs() < 1e-9, "{s:?}");
        assert!(angle_deg(s.leave, Vec3::new(1.0, 0.0, 0.0)) < 1e-6, "{s:?}");
        assert!(s.bend.len() < 1e-6 && s.bend_change.len() < 1e-6, "{s:?}");
    }
    // The chain's picks work the same way: the rim's own points pick the rim's own edges.
    let picked = chain(&b, &[Vec3::new(20.0, 0.0, 20.0), Vec3::new(40.0, 15.0, 20.0)]).unwrap();
    assert_eq!(picked.len(), 2);

    // A cylinder's wall: along the axis it is straight, and a leave direction that is normal to it
    // has no tangent plane to work in.
    let c = cylinder(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 10.0, 30.0).unwrap();
    let wall = face_at(&c, Vec3::new(10.0, 0.0, 15.0));
    let ring = loop_of(&c, wall);
    let f = frames(&c, wall, &ring, Vec3::new(0.0, 0.0, 1.0), 5).unwrap();
    for s in &f {
        assert!(s.bend.len() < 1e-6, "the axis direction is straight: {s:?}");
    }
    // A leave direction that is normal to the support surface has no tangent plane to work in.
    assert!(frames(&b, top, &rim, Vec3::new(0.0, 0.0, 1.0), 9).is_err(), "a face's own normal is no direction to leave in");
    // On the cylinder's wall only a direction with a tangential part is usable.
    assert!(frames(&c, wall, &ring, Vec3::new(1.0, 0.0, 0.0), 5).is_ok());
}

#[test]
fn a_g1_bridge_meets_both_boundaries_tangentially() {
    let (a, b) = two_planes(9);
    let body = bridge(&a, &b, [Continuity::G1, Continuity::G1], [0.0, 0.0]).unwrap();
    for u in [0.0, 0.5, 1.0] {
        let (_, t, _, _) = section_at(&body, u);
        assert!(angle_deg(t, Vec3::new(0.0, 1.0, 0.0)) < 0.01, "the bridge leaves along the tangent: {u}");
    }
    // The boundary itself is on each support: the first and last rows of the grid.
    let grid = sample_grid(&body, 9, 7).unwrap();
    for (row, frame) in grid.iter().zip(&a) {
        assert!(row[0].dist(frame.p) < 1e-9, "{:?} vs {:?}", row[0], frame.p);
    }
    for (row, frame) in grid.iter().zip(&b) {
        assert!(row[6].dist(frame.p) < 1e-9, "{:?} vs {:?}", row[6], frame.p);
    }
}

#[test]
fn g2_and_g3_bridges_repeat_the_support_shape() {
    let (a, b) = valley_between_cylinders(9, 10.0);
    // A G2 bridge to two cylinders: each end holds the cylinder's own curvature (1/10 towards its
    // axis) and leaves along its tangent.
    let g2 = bridge(&a, &b, [Continuity::G2, Continuity::G2], [0.0, 0.0]).unwrap();
    for (i, u) in [(0usize, 0.0), (4, 0.5), (8, 1.0)] {
        let (p, t, bend, _) = section_at(&g2, u);
        assert!(p.dist(a[i].p) < 1e-9, "the boundary is on the cylinder: {p:?}");
        assert!(angle_deg(t, Vec3::new(0.0, 1.0, 0.0)) < 0.01, "leaves along the tangent: {u}");
        assert!((bend.dist(a[i].bend) / a[i].bend.len()) < 1e-3, "curvature {bend:?} vs {:?}", a[i].bend);
    }
    // The far end, read back to front.
    let (p, t, bend, _) = section_end_at(&g2, 1.0);
    assert!(p.dist(b[8].p) < 1e-9, "the far boundary is on its cylinder: {p:?}");
    assert!(angle_deg(t, Vec3::new(0.0, 1.0, 0.0)) < 0.01, "arrives along the far tangent");
    assert!((bend.dist(b[8].bend) / b[8].bend.len()) < 1e-3, "curvature {bend:?} vs {:?}", b[8].bend);

    // A G2 bridge leaves the change of curvature free; asking for G3 pins its normal part to the
    // cylinder's own (zero: a cylinder's curvature does not change as you go round it).
    let (_, _, _, free) = section_at(&g2, 0.0);
    let g3 = bridge(&a, &b, [Continuity::G3, Continuity::G3], [0.0, 0.0]).unwrap();
    let (_, _, bend3, held) = section_at(&g3, 0.0);
    assert!((bend3.dist(a[0].bend) / a[0].bend.len()) < 1e-3, "G3 still holds the curvature: {bend3:?}");
    let along_normal = |v: Vec3| v.dot(Vec3::new(0.0, 0.0, 1.0));
    assert!(along_normal(held).abs() < 1e-3, "a G3 bridge holds the change of curvature: {held:?}");
    assert!(along_normal(free).abs() > 1e-3, "a G2 bridge is free to change it: {free:?}");
}

#[test]
fn crown_shapes_the_middle_without_touching_the_ends() {
    let (a, b) = valley_between_cylinders(11, 10.0);
    let base = bridge(&a, &b, [Continuity::G2, Continuity::G2], [0.0, 0.0]).unwrap();
    let (_, t0, bend0, _) = section_at(&base, 0.0);
    let mut heights = Vec::new();
    for crown in [-0.5, 0.0, 1.0, 2.0] {
        let body = bridge(&a, &b, [Continuity::G2, Continuity::G2], [crown, crown]).unwrap();
        let grid = sample_grid(&body, 11, 9).unwrap();
        heights.push(grid[5][4].z);
        // The ends hold in every case: the same boundary points, the same tangent, and the same
        // seam curvature as the uncrowned bridge.
        let (_, t, bend, _) = section_at(&body, 0.0);
        assert!(angle_deg(t, t0) < 0.01, "crown {crown} left the tangent alone");
        assert!(bend.dist(bend0) < 1e-9, "crown {crown} left the curvature alone: {bend:?} vs {bend0:?}");
        assert!(grid[5][0].dist(a[5].p) < 1e-9 && grid[5][8].dist(b[5].p) < 1e-9, "crown {crown} left the boundary alone");
    }
    let steps: Vec<f64> = heights.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.iter().all(|d| d.abs() > 1e-6 && d.signum() == steps[0].signum()), "a fuller crown moves the middle one way: {heights:?}");
}

#[test]
fn the_bridge_refuses_what_it_cannot_hold() {
    let (a, b) = two_planes(9);
    // Different station counts.
    assert!(bridge(&a[..6], &b, [Continuity::G1, Continuity::G1], [0.0, 0.0]).is_err());
    // A crown outside its range, and a crown that is not a number.
    for crown in [f64::NAN, 5.0, -1.0, f64::INFINITY] {
        assert!(bridge(&a, &b, [Continuity::G1, Continuity::G1], [crown, crown]).is_err(), "{crown}");
    }
    // Boundaries that meet: no room for a bridge.
    let same: Vec<Frame> = a.iter().map(|f| Frame { p: f.p, ..*f }).collect();
    assert!(bridge(&a, &same, [Continuity::G1, Continuity::G1], [0.0, 0.0]).is_err());
    // A station that is not a number.
    let mut bad = a.clone();
    bad[3].leave = Vec3::new(f64::NAN, 0.0, 0.0);
    assert!(bridge(&bad, &b, [Continuity::G1, Continuity::G1], [0.0, 0.0]).is_err());
    // Fewer than two stations.
    assert!(bridge(&a[..1], &b[..1], [Continuity::G1, Continuity::G1], [0.0, 0.0]).is_err());
    // G4 is past the bridge's Hermite rows: refused by the one check, not built as G3.
    let e = bridge(&a, &b, [Continuity::G4, Continuity::G1], [0.0, 0.0]).unwrap_err().to_string();
    assert!(e.contains("not supported yet") && e.contains("g4"), "{e}");
    // Every order from G0 to G3 builds.
    for (x, y) in [(Continuity::G0, Continuity::G0), (Continuity::G1, Continuity::G0), (Continuity::G3, Continuity::G1)] {
        let body = bridge(&a, &b, [x, y], [0.0, 0.0]).unwrap();
        let (_, t, _, _) = section_at(&body, 0.0);
        if x != Continuity::G0 {
            assert!(angle_deg(t, Vec3::new(0.0, 1.0, 0.0)) < 0.01, "{x:?}");
        }
    }
}

/// A radius-10 tube along z from `z0` to `z0 + 30`, turned `turn` radians about its axis (which
/// moves where its rim's seam sits), with the end face at `open_at` taken off: a surface body
/// whose rim there is a closed chain.
fn open_tube(z0: f64, turn: f64, open_at: f64) -> Body {
    let c = cylinder(Vec3::new(0.0, 0.0, z0), Vec3::Z, 10.0, 30.0).unwrap();
    let c = crate::transform(&c, Vec3::ZERO, Vec3::ZERO, Vec3::Z, turn).unwrap();
    let (_, rest) = crate::surfaces::unstitch(&c, &[Vec3::new(0.0, 0.0, open_at)]).unwrap();
    rest.unwrap()
}

/// Points round a radius-10 rim at height `z`.
fn rim_at(z: f64) -> Vec<Vec3> {
    (0..8).map(|k| f64::from(k) * std::f64::consts::FRAC_PI_4 + 0.1).map(|a| Vec3::new(10.0 * a.cos(), 10.0 * a.sin(), z)).collect()
}

/// How far a blend between two coaxial rims turns round the axis: the most, over its stations,
/// of the angle between where a cross-section starts and where it ends (zero for straight
/// rulings parallel to the axis).
fn worst_turn(body: &Body) -> f64 {
    let grid = sample_grid(body, 33, 9).unwrap();
    grid.iter()
        .map(|row| {
            let (a, b) = (row[0], row[row.len() - 1]);
            angle_deg(Vec3::new(a.x, a.y, 0.0), Vec3::new(b.x, b.y, 0.0))
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_blend_between_closed_rims_lines_their_seams_up() {
    // Two coaxial tubes 10 apart, the upper one turned a quarter round: its rim's seam is 90°
    // from the lower rim's. The blend between them is the cylinder wall between: every ruling
    // straight up the axis, none turning round it.
    let low = open_tube(0.0, 0.0, 30.0);
    let high = open_tube(40.0, std::f64::consts::FRAC_PI_2, 40.0);
    for orders in [[Continuity::G0, Continuity::G0], [Continuity::G1, Continuity::G1], [Continuity::G2, Continuity::G2]] {
        let (made, gap) = blend_between(&low, &rim_at(30.0), &high, &rim_at(40.0), orders, [0.0, 0.0], None, None, 65, false).unwrap();
        let turn = worst_turn(&made);
        // The boundary is a cubic through the stations on the rim: between them it strays by the
        // interpolation error of a circle at 65 stations (about 2e-5 mm here).
        assert!(gap < 1e-4, "{orders:?}: the boundary stays on the rims: {gap}");
        assert!(turn < 0.1, "{orders:?}: the blend turns {turn}° round the axis");
        // The wall between: radius 10 everywhere (G1 and G2 repeat a straight wall).
        for row in sample_grid(&made, 17, 9).unwrap() {
            for p in row {
                assert!((Vec3::new(p.x, p.y, 0.0).len() - 10.0).abs() < 1e-5, "{orders:?}: off the wall at {p:?}");
            }
        }
    }
}

#[test]
fn a_closed_chain_blends_only_to_a_closed_chain() {
    // The tube's closed rim against one open edge of a box's top.
    let tube = open_tube(0.0, 0.0, 30.0);
    let b = box_solid(Vec3::new(-20.0, 30.0, 30.0), Vec3::new(20.0, 40.0, 40.0)).unwrap();
    let err = blend_between(&tube, &rim_at(30.0), &b, &[Vec3::new(0.0, 30.0, 40.0)], [Continuity::G1; 2], [0.0, 0.0], None, None, 33, false);
    assert!(matches!(&err, Err(KernelError::Invalid(m)) if m.contains("closed chain")), "{err:?}");
    let err = blend_between(&b, &[Vec3::new(0.0, 30.0, 40.0)], &tube, &rim_at(30.0), [Continuity::G1; 2], [0.0, 0.0], None, None, 33, false);
    assert!(matches!(&err, Err(KernelError::Invalid(m)) if m.contains("closed chain")), "{err:?}");
}

#[test]
fn a_blend_between_offset_strips_is_the_cubic_hermite_strip() {
    // Two 40-long boxes 10 apart in y, the second standing 5 higher: the blend from the first's
    // top rim (y = 10, z = 10) to the second's (y = 20, z = 15), G1 at both ends. Every
    // cross-section is the cubic Bézier that leaves each top face along it, at the speed chord/3
    // (chord = |(10, 5)|): control points (10, 10), (10 + chord/9, 10), (20 - chord/9, 15),
    // (20, 15) in (y, z), the same at every x.
    let a = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 10.0, 10.0)).unwrap();
    let b = box_solid(Vec3::new(0.0, 20.0, 5.0), Vec3::new(40.0, 30.0, 15.0)).unwrap();
    let (made, gap) =
        blend_between(&a, &[Vec3::new(20.0, 10.0, 10.0)], &b, &[Vec3::new(20.0, 20.0, 15.0)], [Continuity::G1; 2], [0.0, 0.0], None, None, 9, false)
            .unwrap();
    assert!(gap < 1e-9, "straight rims: the boundary is on them exactly: {gap}");
    let chord = 125f64.sqrt();
    let control = [(10.0, 10.0), (10.0 + chord / 9.0, 10.0), (20.0 - chord / 9.0, 15.0), (20.0, 15.0)];
    let bezier = |t: f64| {
        let w = [(1.0 - t).powi(3), 3.0 * t * (1.0 - t).powi(2), 3.0 * t * t * (1.0 - t), t.powi(3)];
        control.iter().zip(w).fold((0.0, 0.0), |(y, z), ((cy, cz), w)| (y + cy * w, z + cz * w))
    };
    let across = 11;
    for row in sample_grid(&made, 9, across).unwrap() {
        for (j, p) in row.iter().enumerate() {
            let (y, z) = bezier(j as f64 / (across - 1) as f64);
            assert!((p.y - y).abs() < 1e-9 && (p.z - z).abs() < 1e-9, "at v = {j}/{}: {p:?} vs the Hermite strip ({y}, {z})", across - 1);
        }
    }
    // G1 at both seams: the blend's normal is the top faces' (±z) all along each rim.
    let (surf, [[u0, u1], [v0, v1]]) = the_face(&made).unwrap();
    for i in 0..=16 {
        let u = u0 + (u1 - u0) * i as f64 / 16.0;
        for v in [v0, v1] {
            let (_, su, sv) = surf.derivs(u, v).unwrap();
            let n = su.cross(sv);
            let worst = angle_deg(Vec3::new(n.x, n.y, n.z), Vec3::Z).min(angle_deg(Vec3::new(n.x, n.y, n.z), Vec3::Z * -1.0));
            assert!(worst < 0.01, "at u = {u}, v = {v}: the normal is {worst}° off the top faces'");
        }
    }
}

#[test]
fn a_picked_seam_sets_where_a_closed_blend_starts() {
    // The same coaxial rims as above, but the upper chain told to start a quarter round from where
    // the lower one starts: the blend then turns a quarter round the axis, as asked.
    let low = open_tube(0.0, 0.0, 30.0);
    let high = open_tube(40.0, 0.0, 40.0);
    let free = blend_between(&low, &rim_at(30.0), &high, &rim_at(40.0), [Continuity::G1; 2], [0.0, 0.0], None, None, 65, false).unwrap().0;
    assert!(worst_turn(&free) < 0.1, "unpicked: no turn ({}°)", worst_turn(&free));
    let first = sample_grid(&free, 2, 2).unwrap()[0][0];
    let quarter = Vec3::new(-first.y, first.x, 40.0);
    let seams = Some([Vec3::new(first.x, first.y, 30.0), quarter]);
    let turned = blend_between(&low, &rim_at(30.0), &high, &rim_at(40.0), [Continuity::G1; 2], [0.0, 0.0], None, seams, 65, false).unwrap().0;
    let turn = worst_turn(&turned);
    assert!((turn - 90.0).abs() < 0.1, "the picked seams turn the blend a quarter round: {turn}°");
    // Seam points that are not numbers are refused.
    let bad = Some([Vec3::new(f64::NAN, 0.0, 30.0), quarter]);
    assert!(blend_between(&low, &rim_at(30.0), &high, &rim_at(40.0), [Continuity::G1; 2], [0.0, 0.0], None, bad, 65, false).is_err());
}

#[test]
fn a_blend_between_offset_tubes_reads_stations_on_the_walls_seams() {
    // A radius-10 tube and a radius-6 one standing 8 off its axis: the blend leaves each rim at an
    // angle, so a station on the vertex where a wall's two halves meet is at a corner of one
    // half's parameters. It is read on the other half, and the blend builds at every order.
    let low = open_tube(0.0, 0.0, 30.0);
    let high = cylinder(Vec3::new(8.0, 0.0, 50.0), Vec3::Z, 6.0, 25.0).unwrap();
    let (_, high) = crate::surfaces::unstitch(&high, &[Vec3::new(8.0, 0.0, 50.0)]).unwrap();
    let high = high.unwrap();
    let rim6: Vec<Vec3> = rim_at(50.0).into_iter().map(|p| Vec3::new(8.0 + p.x * 0.6, p.y * 0.6, 50.0)).collect();
    for orders in [[Continuity::G1; 2], [Continuity::G2; 2], [Continuity::G3; 2]] {
        let made = blend_between(&low, &rim_at(30.0), &high, &rim6, orders, [0.0, 0.0], None, None, 41, false);
        let (_, gap) = made.unwrap_or_else(|e| panic!("{orders:?}: {e:?}"));
        assert!(gap < 1e-3, "{orders:?}: {gap}");
    }
}
