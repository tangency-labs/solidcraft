//! Display normals at a pole of higher order (a boundary node with no normal): the exact limit
//! normal per triangle where the kernel gives it, else the mean of the triangles' normals, on
//! the planner's layout either way, named in the plan's stats.

#![allow(clippy::panic, clippy::indexing_slicing)] // test helpers

mod pole_faces;

use pole_faces::{Kind, L};
use solidcore_record::Vec3;
use solidcore_tess::{FacePlan, Mesh, MeshTolerance, PlanStats, PoleRule, TessPlan, plan};

/// The body of `k` planned at display tolerance for its size.
fn planned(k: Kind) -> (TessPlan, PlanStats) {
    let b = k.body();
    if let Err(e) = b.validate() {
        panic!("{}: {e:?}", k.name());
    }
    let size = L * 3f64.sqrt();
    match plan(&b, &MeshTolerance::display_of(size), size) {
        Ok(p) => p,
        Err(e) => panic!("{}: {e}", k.name()),
    }
}

/// Face `face`'s triangles in the mesh, in the plan's order.
fn face_triangles(m: &Mesh, face: u32) -> Vec<[u32; 3]> {
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| **f == face)
        .map(|(t, _)| *t)
        .collect()
}

/// The angle between `a` and `b` (rad), exact to rounding at any angle (`atan2` of the cross
/// and dot products; `acos` of the dot would lose half the digits near 0).
fn angle(a: Vec3, b: [f64; 3]) -> f64 {
    let b = Vec3::new(b[0], b[1], b[2]);
    let cross = Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    );
    let sin = (cross.x * cross.x + cross.y * cross.y + cross.z * cross.z).sqrt();
    sin.atan2(a.x * b.x + a.y * b.y + a.z * b.z)
}

/// The parameters of `f`'s pole nodes.
fn pole_nodes(f: &FacePlan) -> Vec<u32> {
    f.poles.iter().map(|p| p.node).collect()
}

/// Faces 1 to 3 (poles of order two and three) take the limit rule on both faces: every pole
/// node is named with it, nothing is averaged, the faces are meshed on the plan's triangles, and
/// each triangle's copy of the pole has the closed-form limit normal at its own point of the
/// pole (the mean of its other corners along the collapsed parameter), to rounding.
#[test]
fn a_pole_of_order_two_or_three_has_the_exact_limit_normal_per_triangle() {
    for k in Kind::LIMIT {
        let (p, stats) = planned(k);
        assert!(
            stats.unmeshed.is_empty(),
            "{}: {:?}",
            k.name(),
            stats.unmeshed
        );
        assert!(
            stats.averaged_poles.is_empty(),
            "{}: {:?}",
            k.name(),
            stats.averaged_poles
        );
        let (m, refused) = p.evaluate();
        assert!(refused.is_empty(), "{}: {refused:?}", k.name());
        let mut copies = 0;
        let mut worst = 0.0f64;
        for f in &p.faces {
            let poles = pole_nodes(f);
            assert!(
                !poles.is_empty(),
                "{} face {}: no pole named",
                k.name(),
                f.face
            );
            assert!(
                f.poles
                    .iter()
                    .all(|q| matches!(q.rule, PoleRule::Limit { .. })),
                "{} face {}: {:?}",
                k.name(),
                f.face,
                f.poles
            );
            let tris = face_triangles(&m, f.face);
            assert_eq!(
                tris.len(),
                f.triangles.len(),
                "{} face {}",
                k.name(),
                f.face
            );
            let sign = if f.flip { -1.0 } else { 1.0 };
            let mut used = Vec::new();
            for (t, mt) in f.triangles.iter().zip(&tris) {
                for (c, &node) in t.iter().enumerate() {
                    if !poles.contains(&node) {
                        continue;
                    }
                    // One copy per triangle: no index is shared at the pole.
                    assert!(!used.contains(&mt[c]), "{}: a pole copy shared", k.name());
                    used.push(mt[c]);
                    let others: Vec<[f64; 2]> = t
                        .iter()
                        .filter(|j| !poles.contains(j))
                        .map(|&j| f.uv[j as usize])
                        .collect();
                    let v = others.iter().map(|q| q[1]).sum::<f64>() / others.len() as f64;
                    let exact = k.limit(f.face as usize, [0.0, v]).map(|x| x * sign);
                    let n = m.normals[mt[c] as usize];
                    worst = worst.max(angle(n, exact));
                    copies += 1;
                }
            }
        }
        assert!(copies > 0, "{}", k.name());
        assert!(worst < 1e-12, "{}: {worst:e} rad off the limit", k.name());
    }
}

/// A pole of order four (`N_0` to `N_3` vanish, the kernel refuses its limit) keeps the
/// planner's layout: every face is meshed with exactly the plan's triangles, the pole stays one
/// vertex per node with the area-weighted mean of its triangles' normals, and the plan's stats
/// name the pole on each face with the kernel's reason.
#[test]
fn a_pole_of_order_four_is_meshed_with_its_triangles_mean_normal_and_named() {
    let (p, stats) = planned(Kind::Quartic);
    assert!(stats.unmeshed.is_empty(), "{:?}", stats.unmeshed);
    let named: Vec<usize> = stats.averaged_poles.iter().map(|a| a.face).collect();
    assert_eq!(named, vec![0, 1], "{:?}", stats.averaged_poles);
    for a in &stats.averaged_poles {
        assert_eq!(a.point, Vec3::new(0.0, 0.0, 0.0), "{a:?}");
        assert!(
            !a.uv.is_empty() && a.uv.iter().all(|q| q[0] == 0.0),
            "{a:?}"
        );
        assert!(a.reason.contains("a pole of order above three"), "{a:?}");
    }
    let (m, refused) = p.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    for f in &p.faces {
        assert!(
            !f.poles.is_empty() && f.poles.iter().all(|q| q.rule == PoleRule::Averaged),
            "face {}: {:?}",
            f.face,
            f.poles
        );
        let tris = face_triangles(&m, f.face);
        assert_eq!(tris.len(), f.triangles.len(), "face {}", f.face);
        let poles = pole_nodes(f);
        for &node in &poles {
            // The node's own vertex in every triangle at it: not split.
            let at: Vec<u32> = f
                .triangles
                .iter()
                .zip(&tris)
                .flat_map(|(t, mt)| {
                    t.iter()
                        .zip(mt)
                        .filter(|(j, _)| **j == node)
                        .map(|(_, i)| *i)
                })
                .collect();
            assert!(!at.is_empty(), "face {} node {node}", f.face);
            assert!(
                at.iter().all(|i| *i == at[0]),
                "face {} node {node} split",
                f.face
            );
            // The mean of the face's triangles' normals at the pole point, weighted by area.
            let mut sum = [0.0f64; 3];
            for mt in &tris {
                let q = mt.map(|i| m.positions[i as usize]);
                if q.contains(&Vec3::new(0.0, 0.0, 0.0)) {
                    let (a, b) = (q[1] - q[0], q[2] - q[0]);
                    sum[0] += a.y * b.z - a.z * b.y;
                    sum[1] += a.z * b.x - a.x * b.z;
                    sum[2] += a.x * b.y - a.y * b.x;
                }
            }
            let n = m.normals[at[0] as usize];
            assert!(angle(n, sum) < 1e-12, "face {} node {node}: {n:?}", f.face);
            // A display normal near the surface's own limit (−e_x, oriented).
            let sign = if f.flip { -1.0 } else { 1.0 };
            assert!(
                angle(n, [-sign, 0.0, 0.0]) < 10f64.to_radians(),
                "face {}: {n:?}",
                f.face
            );
        }
    }
}

/// The cone whose apex is of third order (both first derivatives vanish there) is planned like
/// the other faces, 592 triangles a quarter, and meshed whole: its apex takes the limit rule.
#[test]
fn the_cone_apex_of_third_order_is_meshed_on_its_592_triangles() {
    let (p, stats) = planned(Kind::Cone);
    assert!(stats.unmeshed.is_empty(), "{:?}", stats.unmeshed);
    let (m, refused) = p.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    for f in &p.faces {
        assert_eq!(f.triangles.len(), 592, "face {}", f.face);
        assert_eq!(face_triangles(&m, f.face).len(), 592, "face {}", f.face);
    }
}

/// A cone on one spline closed along its turn, its face across the interval's end: the plan
/// holds the parameters in the interval, so a triangle there has corners near both ends (0.98
/// and 0.02). Its foot is between them, not on the far side of the pole: every copy has the
/// closed-form limit normal at its own point of the pole, to rounding.
#[test]
fn a_pole_across_a_closed_spline_s_end_has_the_limit_normal_between_its_corners() {
    let b = pole_faces::closed_cone();
    if let Err(e) = b.validate() {
        panic!("closed cone: {e:?}");
    }
    let size = L * 3f64.sqrt();
    let (p, stats) = match plan(&b, &MeshTolerance::display_of(size), size) {
        Ok(p) => p,
        Err(e) => panic!("closed cone: {e}"),
    };
    assert!(stats.unmeshed.is_empty(), "{:?}", stats.unmeshed);
    assert!(
        stats.averaged_poles.is_empty(),
        "{:?}",
        stats.averaged_poles
    );
    let (m, refused) = p.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    let (mut across, mut worst) = (0, 0.0f64);
    for f in &p.faces {
        let poles = pole_nodes(f);
        assert!(
            !poles.is_empty()
                && f.poles
                    .iter()
                    .all(|q| matches!(q.rule, PoleRule::Limit { .. })),
            "face {}: {:?}",
            f.face,
            f.poles
        );
        let tris = face_triangles(&m, f.face);
        assert_eq!(tris.len(), f.triangles.len(), "face {}", f.face);
        let sign = if f.flip { -1.0 } else { 1.0 };
        for (t, mt) in f.triangles.iter().zip(&tris) {
            for (c, &node) in t.iter().enumerate() {
                if !poles.contains(&node) {
                    continue;
                }
                let mut vs: Vec<f64> = t
                    .iter()
                    .filter(|j| !poles.contains(j))
                    .map(|&j| f.uv[j as usize][1])
                    .collect();
                // Corners near both ends of the turn: the low ones a turn on.
                let (lo, hi) = vs
                    .iter()
                    .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
                if hi - lo > 0.5 {
                    across += 1;
                    for v in &mut vs {
                        if *v < 0.5 {
                            *v += 1.0;
                        }
                    }
                }
                let th = pole_faces::closed_angle(vs.iter().sum::<f64>() / vs.len() as f64);
                let r = std::f64::consts::FRAC_1_SQRT_2;
                let exact = [-th.cos() * r, -th.sin() * r, r].map(|x| x * sign);
                worst = worst.max(angle(m.normals[mt[c] as usize], exact));
            }
        }
    }
    assert!(
        across > 0,
        "no triangle at the pole lies across the interval's end"
    );
    assert!(worst < 1e-12, "{worst:e} rad off the limit");
}
