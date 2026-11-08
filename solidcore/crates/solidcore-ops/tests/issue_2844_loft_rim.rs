//! #2844: the round and the chamfer of a ruled loft's circular rim (solidcraft#12034, the
//! kernel case `fillets/12034-round-loft-square-to-circle-circular-rim`). The loft, degree 1,
//! runs from the square of side 30 at z = 0 (four lines) to the circle r 10 at z = 30 (four
//! quarter arcs, each centred on a side's direction): six faces, the top a plane disc. Each wall
//! is the ruled patch from a side to its arc; two walls meet along a straight side edge from a
//! square's corner to an arcs' joint, tangent to each other only at that joint.
//!
//! At the middle of a side (y = 0 for the side x = 15) the loft is symmetric, so the round's
//! section there is the ball's circle in that plane, between the disc and the wall's middle
//! ruling from (15, 0, 0) to (10, 0, 30). They meet at 90 degrees + atan(1/6) inside the
//! material, so the ball of radius r stands k r inside the rim, k = cot of half that angle =
//! (sqrt 37 - 1) / 6, its centre at (10 - k r, 0, 30 - r). A chamfer's setbacks are its
//! distance at its edge's middle (its offsets are taken there), so its chord there joins
//! (10 - d, 0, 30) to the point d down the ruling.
//!
//! Its own test binary, on the kernel's default fixes: none is set. The last test rounds a copy
//! of the loft on a kernel with every fix on, the round built as its definition.
#![allow(clippy::disallowed_methods)] // closed-form oracles, not ops geometry
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

mod common;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use common::{edge_at, kernel_copy, rel, volume};
use solidcore::blend::definition_check::BlendFaceLog;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::Point3;
use solidcore::query::api_queries::{edge_closest_point, surface_closest_point};
use solidcore::topology::{edge, face, EdgeId};
use solidcore::{Brep, Kernel, Vec3};
use solidcore_ops::sweep::{loft, Curve};
use solidcore_ops::{blend, Blend, Body, CheckLevel, Corners, PointClass};

/// The sides' and arcs' mixed area M: the loft's section at height 30 s is
/// C = (1 - s) L(t) + s A(t), L a side and A its arc at the same parameter, so its area,
/// half the integral of C x C', is (1 - s)^2 900 + 2 s (1 - s) M + s^2 100 pi with
/// M = 1/4 of the integral of L x A' + A x L' round the loop, by the four-fold symmetry the
/// integral over one side. A is the arc's rational quadratic form, control points 10 (cos 45,
/// -sin 45), 10 / cos 45 (1, 0), 10 (cos 45, sin 45) with weights 1, cos 45, 1, and L the side
/// x = 15 from y = -15 to 15. Simpson's rule on 2000 panels: 537.671338762226, within 1e-12 of
/// it on 200 (the loft's volume below checks it against the kernel's to 1e-9).
fn mixed() -> f64 {
    let c = FRAC_PI_4.cos();
    let p = [[10.0 * c, -10.0 * c], [10.0 / c, 0.0], [10.0 * c, 10.0 * c]];
    let w = [1.0, c, 1.0];
    let integrand = |t: f64| {
        let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
        let db = [-2.0 * (1.0 - t), 2.0 - 4.0 * t, 2.0 * t];
        let dot = |u: [f64; 3], i: usize| (0..3).map(|j| u[j] * w[j] * p[j][i]).sum::<f64>();
        let ww = (0..3).map(|j| b[j] * w[j]).sum::<f64>();
        let dww = (0..3).map(|j| db[j] * w[j]).sum::<f64>();
        let a = [dot(b, 0) / ww, dot(b, 1) / ww];
        let da = [
            (dot(db, 0) * ww - dot(b, 0) * dww) / (ww * ww),
            (dot(db, 1) * ww - dot(b, 1) * dww) / (ww * ww),
        ];
        let (l, dl) = ([15.0, -15.0 + 30.0 * t], [0.0, 30.0]);
        l[0] * da[1] - l[1] * da[0] + a[0] * dl[1] - a[1] * dl[0]
    };
    let n = 2000;
    let sum: f64 = (0..=n)
        .map(|i| {
            let weight = if i == 0 || i == n {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            weight * integrand(f64::from(i) / f64::from(n))
        })
        .sum();
    sum / (3.0 * f64::from(n))
}

/// The loft's volume, 30 times the mean of that area: 10 (900 + M + 100 pi).
fn loft_volume() -> f64 {
    10.0 * (900.0 + mixed() + 100.0 * PI)
}

/// How much of the loft lies in the band rho >= 10 - s, z >= 30 - s: every wall leans in, so
/// a round of radius s or a chamfer of distance s takes nothing outside it.
fn band(s: f64) -> f64 {
    let s0 = 1.0 - s / 30.0;
    let square = 900.0 * (1.0 - s0).powi(3) / 3.0;
    let mixed = 2.0 * mixed() * (1.0 / 6.0 - s0 * s0 / 2.0 + s0.powi(3) / 3.0);
    let circle = 100.0 * PI * (1.0 - s0.powi(3)) / 3.0;
    30.0 * (square + mixed + circle) - s * PI * (10.0 - s).powi(2)
}

/// The loft: the sides paired with the arcs centred on their directions.
fn loft_body() -> Body {
    let side = |a: [f64; 2], b: [f64; 2]| Curve::Line {
        start: [a[0], a[1], 0.0],
        end: [b[0], b[1], 0.0],
    };
    let square = vec![
        side([15.0, -15.0], [15.0, 15.0]),
        side([15.0, 15.0], [-15.0, 15.0]),
        side([-15.0, 15.0], [-15.0, -15.0]),
        side([-15.0, -15.0], [15.0, -15.0]),
    ];
    let circle = (0..4)
        .map(|q| Curve::Arc {
            center: [0.0, 0.0, 30.0],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 10.0,
            start: f64::from(q) * FRAC_PI_2 - FRAC_PI_4,
            end: f64::from(q) * FRAC_PI_2 + FRAC_PI_4,
        })
        .collect();
    let b = loft(&[square, circle], 1, true).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert!(rel(volume(&b), loft_volume()) < 1e-9, "{}", volume(&b));
    b
}

/// `p` turned `q` quarters about the z axis.
fn turned(p: [f64; 3], q: i32) -> [f64; 3] {
    let (s, c) = (f64::from(q) * FRAC_PI_2).sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]]
}

/// The rim's four arcs, each found at its middle.
fn rim(b: &Body) -> Vec<usize> {
    (0..4)
        .map(|q| edge_at(b, turned([10.0, 0.0, 30.0], q)))
        .collect()
}

/// k: the disc and a wall's middle ruling inside the material, cot of half their angle.
fn inset() -> f64 {
    (37.0_f64.sqrt() - 1.0) / 6.0
}

/// `p`, turned by each quarter about the z axis (to each side's middle or each joint), lies
/// where `class` says.
fn at_each(b: &Body, p: [f64; 3], class: PointClass, what: &str) {
    for q in 0..4 {
        let got = b.classify_point(turned(p, q)).unwrap();
        assert_eq!(got, class, "{what}, quarter {q}");
    }
}

/// The distance from `p` to the nearest of `b`'s faces' surfaces (untrimmed).
fn to_nearest_surface(b: &Body, p: [f64; 3]) -> f64 {
    let (k, brep) = kernel_copy(b);
    let q = Vec3::new(p[0], p[1], p[2]);
    brep.face_list()
        .iter()
        .map(|&f| {
            let s = face::p2::surface(&brep, f).unwrap().unwrap();
            surface_closest_point(&k, s, q).map_or(f64::INFINITY, |c| c.closest.distance)
        })
        .fold(f64::INFINITY, f64::min)
}

/// A crease: an edge lying in a joint's plane (through the z axis at 45 + 90 q degrees) with
/// both ends above z = 25, so not the loft's side edge, which runs down to z = 0. Its quarter,
/// its ends, its largest distance from that plane over 33 points and its tolerance: the exact
/// crease lies within that of the stored curve.
struct Crease {
    quarter: i32,
    ends: [Point3; 2],
    off_plane: f64,
    tolerance: f64,
}

fn creases(k: &Kernel, brep: &Brep) -> Vec<Crease> {
    let mut out = Vec::new();
    for &e in brep.edge_list() {
        let curve = edge::curve(brep, e).unwrap().unwrap();
        let ivl = edge::interval(brep, e).unwrap();
        let at = |t: f64| curve.evaluate_point(k, ivl.evaluate(t)).unwrap();
        let ends = [at(0.0), at(1.0)];
        if ends.iter().any(|p| p.z < 25.0) {
            continue;
        }
        for quarter in 0..4 {
            let a = FRAC_PI_4 + f64::from(quarter) * FRAC_PI_2;
            // the joint's plane, on the joint's side of the axis
            let off = |p: Point3| (-a.sin() * p.x + a.cos() * p.y).abs();
            let mid = at(0.5);
            if a.cos() * mid.x + a.sin() * mid.y <= 0.0 {
                continue;
            }
            let off_plane = (0..=32)
                .map(|i| off(at(f64::from(i) / 32.0)))
                .fold(0.0, f64::max);
            if off_plane < 1e-3 {
                out.push(Crease {
                    quarter,
                    ends,
                    off_plane,
                    tolerance: edge::tolerance(brep, e).unwrap(),
                });
            }
        }
    }
    out
}

/// The rim of a fresh loft blended by `kind` of size `size`: one closed valid solid with less
/// than the loft and more than the loft less the band; it and its volume.
fn blended(kind: Blend, size: f64) -> (Body, f64) {
    let b = loft_body();
    let edges = rim(&b);
    if let Err(e) = blend(&b, kind, &edges, &[size], &[], Corners::Filled) {
        panic!("{kind:?} {size}: {e}");
    }
    assert!(
        b.check(CheckLevel::All).unwrap().is_valid(),
        "{kind:?} {size}"
    );
    assert_eq!(b.solid_count().unwrap(), 1, "{kind:?} {size}");
    let v = volume(&b);
    let loft = loft_volume();
    assert!(v < loft && loft - v < band(size), "{kind:?} {size}: {v}");
    (b, v)
}

/// What every rounding of the rim leaves at the old joints, the arcs' meeting points: their
/// corner taken, the side edges below the round untouched.
fn joints(b: &Body) {
    let (s, c) = FRAC_PI_4.sin_cos();
    // 0.01 inside the rim and below the disc, in the plane of a side edge: material before
    at_each(
        b,
        [9.99 * c, 9.99 * s, 29.99],
        PointClass::Outside,
        "corner at a joint",
    );
    // on the side edge from (15, 15, 0) to (10 cos 45, 10 sin 45, 30), at z = 25
    let x = 15.0 - 25.0 / 30.0 * (15.0 - 10.0 * c);
    at_each(
        b,
        [x, x, 25.0],
        PointClass::OnBoundary,
        "side edge below the round",
    );
}

/// The distance from `p` to the joint's side edge from (15, 15, 0) to (10 cos 45, 10 sin 45,
/// 30), turned by `quarter`.
fn off_side_edge(p: Point3, quarter: i32) -> f64 {
    let c = FRAC_PI_4.cos();
    let a = turned([15.0, 15.0, 0.0], quarter);
    let b = turned([10.0 * c, 10.0 * c, 30.0], quarter);
    let u = Vec3::new(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    let v = Vec3::new(p.x - a[0], p.y - a[1], p.z - a[2]);
    v.cross(u).length() / u.length()
}

/// The rounds mitred at each joint (`docs/generic-blend.md` §3.2, the Turn): one crease per
/// joint, in the joint's plane, the plane of symmetry between the two rounds, from the disc
/// (where both rounds are tangent to it) down to the loft's side edge, less than a radius below
/// the rim. The joint's plane holds the exact crease by symmetry, so the stored one lies within
/// the edge's tolerance of it, as its ends lie within it of the disc and the side edge.
fn mitred(b: &Body, r: f64) {
    let (k, brep) = kernel_copy(b);
    mitred_in(&k, &brep, r);
}

fn mitred_in(k: &Kernel, brep: &Brep, r: f64) {
    let cs = creases(k, brep);
    let mut quarters: Vec<i32> = cs.iter().map(|c| c.quarter).collect();
    quarters.sort_unstable();
    assert_eq!(quarters, [0, 1, 2, 3], "r {r}: one crease per joint");
    for c in &cs {
        assert!(
            c.off_plane <= c.tolerance,
            "r {r}: crease {} off its plane by {} (tolerance {})",
            c.quarter,
            c.off_plane,
            c.tolerance
        );
        let (top, low) = if c.ends[0].z > c.ends[1].z {
            (c.ends[0], c.ends[1])
        } else {
            (c.ends[1], c.ends[0])
        };
        assert!(
            (top.z - 30.0).abs() <= c.tolerance,
            "r {r}: crease {} top {top:?}",
            c.quarter
        );
        let side = off_side_edge(low, c.quarter);
        assert!(
            side <= c.tolerance && low.z < 30.0 && low.z > 30.0 - r,
            "r {r}: crease {} ends at {low:?}, {side} off the side edge",
            c.quarter
        );
    }
}

#[test]
fn the_round_of_the_rim_is_one_solid_with_the_balls_section_at_each_sides_middle() {
    let k = inset();
    let n = (k * k + 1.0).sqrt();
    let s37 = 37.0_f64.sqrt();
    let mut volumes = Vec::new();
    for r in [0.5, 1.0, 2.0] {
        let (b, v) = blended(Blend::Round, r);
        let c = [10.0 - k * r, 0.0, 30.0 - r];
        // along the ball's radius towards the rim's corner (10, 0, 30)
        let along = |t: f64| [c[0] + t * r * k / n, 0.0, c[2] + t * r / n];
        let m = along(1.0);
        // the section's middle: the default round's section is a polynomial approximation of
        // the ball's circle, 5.2e-5 r off it there (measured, the same on every side)
        for q in 0..4 {
            let d = to_nearest_surface(&b, turned(m, q));
            assert!(d < 1e-4 * r, "the section's middle, quarter {q}: {d}");
        }
        at_each(
            &b,
            [c[0], 0.0, 30.0],
            PointClass::OnBoundary,
            "the disc's rail",
        );
        let wall = [c[0] + 6.0 * r / s37, 0.0, c[2] + r / s37];
        at_each(&b, wall, PointClass::OnBoundary, "the wall's rail");
        at_each(&b, along(0.5), PointClass::Inside, "inside the ball");
        let corner = [(m[0] + 10.0) / 2.0, 0.0, (m[2] + 30.0) / 2.0];
        at_each(
            &b,
            corner,
            PointClass::Outside,
            "between the section and the rim",
        );
        joints(&b);
        mitred(&b, r);
        volumes.push(v);
    }
    // a larger ball takes more
    assert!(
        volumes[0] > volumes[1] && volumes[1] > volumes[2],
        "{volumes:?}"
    );
}

#[test]
fn the_chamfer_of_the_rim_is_one_solid_with_its_chord_at_each_sides_middle() {
    let s37 = 37.0_f64.sqrt();
    for d in [1.0, 2.0] {
        let (b, _) = blended(Blend::Chamfer, d);
        let disc = [10.0 - d, 0.0, 30.0];
        let wall = [10.0 + d / s37, 0.0, 30.0 - 6.0 * d / s37];
        let mid = [(disc[0] + wall[0]) / 2.0, 0.0, (disc[2] + wall[2]) / 2.0];
        at_each(&b, disc, PointClass::OnBoundary, "the disc's setback");
        at_each(&b, wall, PointClass::OnBoundary, "the wall's setback");
        at_each(&b, mid, PointClass::OnBoundary, "the chord's middle");
        // across the chord from the rim's corner: material; towards it: taken
        let inside = [1.5 * mid[0] - 5.0, 0.0, 1.5 * mid[2] - 15.0];
        at_each(&b, inside, PointClass::Inside, "beyond the chord");
        let corner = [(mid[0] + 10.0) / 2.0, 0.0, (mid[2] + 30.0) / 2.0];
        at_each(
            &b,
            corner,
            PointClass::Outside,
            "between the chord and the rim",
        );
        joints(&b);
        mitred(&b, d);
    }
}

/// On a kernel with every fix on (`UpstreamFixes::all()`, as `kernel_copy` reads), the round and
/// the chamfer of the rim make exactly their four blend faces, of the blend's own kind and no
/// corner face (the faces the session logs, `blend::definition_check`): each joint is the
/// Turn's Mitre, not a patch. The rounds mitre at the joints there too.
///
/// Not checked here: whether those faces are definitions. A round of one arc of this rim, where
/// no corner is classified, makes bare B-spline faces too; that is the loft's walls, not the
/// joints.
#[test]
fn the_rims_blends_make_only_their_faces_with_every_fix() {
    for (xsect, size, kind) in [(1, 0.5, "round"), (1, 2.0, "round"), (0, 1.0, "chamfer")] {
        let (mut k, mut b) = kernel_copy(&loft_body());
        k.blend_face_log = Some(BlendFaceLog::default());
        let edges: Vec<EdgeId> = (0..4)
            .map(|q| {
                let p = turned([10.0, 0.0, 30.0], q);
                let p = Vec3::new(p[0], p[1], p[2]);
                *b.edge_list()
                    .iter()
                    .find(|&&e| edge_closest_point(&k, &b, e, p).unwrap().closest.distance < 1e-9)
                    .unwrap()
            })
            .collect();
        fillet_edges(&mut k, &mut b, &edges, size, xsect, 1, 1.0).unwrap();
        let made: Vec<String> = k
            .blend_face_log
            .as_ref()
            .unwrap()
            .last()
            .iter()
            .map(|m| m.kind.name())
            .collect();
        assert_eq!(made, [kind; 4], "section {xsect} {size}: faces made");
        mitred_in(&k, &b, size);
    }
}
