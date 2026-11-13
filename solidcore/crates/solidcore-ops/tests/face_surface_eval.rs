//! `face_surface::FaceSurface`: a body face's exact surface and its point, normal and nearest
//! point, against closed forms.
//!
//! - The round of a bore's rim (the plane z = 10 square to a Z bore of radius 5, a round of 0.3):
//!   the torus whose centre circle has radius 5.3 at z = 9.7 and whose tube is 0.3. Every point
//!   is on it, every normal is its normal and every nearest point is its nearest point, to 1e-12.
//! - The round at a boss's root (a Z boss of radius 5 on the plane z = 10, a round of 0.3): the
//!   torus of centre circle radius 5.3 at z = 10.3 and tube 0.3, the same way.
//! - A variable round (a general procedural blend): the nearest point on its definition S of a
//!   point of its net is within the certified bound, its residual along S's normal; a point off
//!   S along its normal has that point for its nearest.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test

mod common;

use common::{all_fixes, edge_at, vertex_at};
use solidcore_ops::blend::{blend, fillet_variable, Blend, Corners};
use solidcore_ops::boolean::boolean;
use solidcore_ops::face_surface::{Exact, FaceSurface};
use solidcore_ops::{make_box, make_cylinder, Body, BooleanOp, Error};
use solidcore_record::brep::{Brep as Record, Surface};

const TOL: f64 = 1e-12;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// The record of `b` and the indices of its faces kept as their blend definition.
fn blend_faces(b: &Body) -> (Record, Vec<usize>) {
    let (record, _) = b.to_record().unwrap();
    let faces = record
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| matches!(f.surface, Surface::Blend { .. }))
        .map(|(i, _)| i)
        .collect();
    (record, faces)
}

/// The (u, v) at fractions `(a, c)` of the face's parameter box.
fn at(s: &FaceSurface, a: f64, c: f64) -> [f64; 2] {
    let [[u0, u1], [v0, v1]] = s.domain();
    [u0 + a * (u1 - u0), v0 + c * (v1 - v0)]
}

/// The interior fractions sampled.
const FRACTIONS: [f64; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];

/// A torus about the Z axis through `centre`: its centre circle of radius `major` in
/// z = `centre[2]`, its tube `minor`.
struct Torus {
    centre: [f64; 3],
    major: f64,
    minor: f64,
}

impl Torus {
    /// The point of the centre circle nearest `p` (off the axis).
    fn spine(&self, p: [f64; 3]) -> [f64; 3] {
        let d = [p[0] - self.centre[0], p[1] - self.centre[1], 0.0];
        add(self.centre, scale(d, self.major / length(d)))
    }

    /// The point of the torus nearest `p` (off the axis and the centre circle).
    fn nearest(&self, p: [f64; 3]) -> [f64; 3] {
        let c = self.spine(p);
        let d = sub(p, c);
        add(c, scale(d, self.minor / length(d)))
    }
}

/// Every round face of `b` is the torus `t`: the exact surface it reports, and its points,
/// normals and nearest points, to [`TOL`].
fn check_round(b: &Body, t: &Torus) {
    let (record, faces) = blend_faces(b);
    assert!(!faces.is_empty(), "the round kept no definition");
    for face in faces {
        let s = FaceSurface::of(&record, face).unwrap();
        let Exact::Torus {
            centre,
            axis,
            major,
            minor,
        } = *s.exact()
        else {
            panic!("face {face}: {:?}, not a torus", s.exact());
        };
        assert!(length(sub(centre, t.centre)) <= TOL, "centre {centre:?}");
        assert!(
            axis[0].abs() <= TOL && axis[1].abs() <= TOL && (axis[2].abs() - 1.0).abs() <= TOL,
            "axis {axis:?}"
        );
        assert!((major - t.major).abs() <= TOL, "major {major}");
        assert!((minor - t.minor).abs() <= TOL, "minor {minor}");
        // the face's normal is the torus's, on one side throughout
        let mut side = 0.0;
        for a in FRACTIONS {
            for c in FRACTIONS {
                let uv = at(&s, a, c);
                let p = s.point(uv).unwrap();
                let off = sub(p, t.spine(p));
                assert!(
                    (length(off) - t.minor).abs() <= TOL,
                    "{uv:?}: {p:?} is {} off the torus",
                    length(off) - t.minor
                );
                let n = s.normal(uv).unwrap();
                let m = scale(off, 1.0 / length(off));
                if side == 0.0 {
                    side = dot(n, m).signum();
                }
                assert!(
                    length(sub(n, scale(m, side))) <= TOL,
                    "{uv:?}: normal {n:?}, the torus's {m:?}"
                );
                // a point 0.1 off along the normal, from a guess beside the point
                let q = add(p, scale(n, 0.1));
                let guess = at(&s, a + 0.04, c - 0.04);
                let near = s.closest_point(q, guess).unwrap();
                let want = t.nearest(q);
                assert!(
                    length(sub(near.point, want)) <= TOL,
                    "{uv:?}: nearest {:?}, the torus's {want:?}",
                    near.point
                );
                assert!((near.distance - length(sub(q, want))).abs() <= TOL);
                assert!(length(sub(s.point(near.uv).unwrap(), near.point)) <= TOL);
                // the same inputs give the same answer, bit for bit
                assert_eq!(s.closest_point(q, guess).unwrap(), near);
            }
        }
    }
}

/// The 30 x 30 x 10 block at the origin.
fn plate() -> Body {
    make_box([0.0; 3], [30.0, 30.0, 10.0]).unwrap()
}

/// A point of the circle of radius 5 about (15, 15) at height `z`, off any seam.
fn on_rim(z: f64) -> [f64; 3] {
    let a = std::f64::consts::FRAC_PI_4;
    [15.0 + 5.0 * a.cos(), 15.0 + 5.0 * a.sin(), z]
}

#[test]
fn a_bore_rims_round_is_its_torus() {
    all_fixes();
    let bore = make_cylinder([15.0, 15.0, 0.0], 5.0, 10.0).unwrap();
    let b = boolean(plate(), bore, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let e = edge_at(&b, on_rim(10.0));
    blend(&b, Blend::Round, &[e], &[0.3], &[], Corners::Filled).unwrap();
    check_round(
        &b,
        &Torus {
            centre: [15.0, 15.0, 9.7],
            major: 5.3,
            minor: 0.3,
        },
    );
}

#[test]
fn a_boss_roots_round_is_its_torus() {
    all_fixes();
    let boss = make_cylinder([15.0, 15.0, 10.0], 5.0, 5.0).unwrap();
    let b = boolean(plate(), boss, BooleanOp::Union).unwrap().unwrap();
    let e = edge_at(&b, on_rim(10.0));
    blend(&b, Blend::Round, &[e], &[0.3], &[], Corners::Filled).unwrap();
    check_round(
        &b,
        &Torus {
            centre: [15.0, 15.0, 10.3],
            major: 5.3,
            minor: 0.3,
        },
    );
}

#[test]
fn a_variable_rounds_definition_is_within_its_bound_of_its_net() {
    all_fixes();
    // the round of radius 2 to 5 along the block's edge x = 10, z = 30 (tests/blend.rs)
    let b = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
    let e = edge_at(&b, [10.0, 10.0, 30.0]);
    let start = vertex_at(&b, [10.0, 0.0, 30.0]);
    fillet_variable(&b, e, start, 1, &[0.0, 0.0, 1.0, 1.0], &[2.0, 5.0]).unwrap();
    let (record, faces) = blend_faces(&b);
    assert!(!faces.is_empty(), "the round kept no definition");
    for face in faces {
        let s = FaceSurface::of(&record, face).unwrap();
        let Exact::Blend { bound } = *s.exact() else {
            panic!("face {face}: {:?}, not a procedural blend", s.exact());
        };
        assert!(bound.is_finite() && bound >= 0.0, "bound {bound}");
        // the same face as its net alone: its NURBS, in the same (u, v)
        let mut bare = record.clone();
        let f = &mut bare.faces[face];
        f.surface = f.nurbs.take().unwrap();
        let net = FaceSurface::of(&bare, face).unwrap();
        assert_eq!(*net.exact(), Exact::Nurbs);
        assert_eq!(net.domain(), s.domain());
        for a in FRACTIONS {
            for c in FRACTIONS {
                let uv = at(&s, a, c);
                // a point of the net: S's nearest is within the bound, along S's normal
                let n_point = net.point(uv).unwrap();
                let near = s.closest_point(n_point, uv).unwrap();
                assert!(
                    near.distance <= bound,
                    "{uv:?}: the net is {} off S, its bound {bound}",
                    near.distance
                );
                let r = sub(n_point, near.point);
                let n = s.normal(near.uv).unwrap();
                let along = sub(r, scale(n, dot(r, n)));
                assert!(
                    length(along) <= TOL,
                    "{uv:?}: residual {r:?} is {} off S's normal",
                    length(along)
                );
                // a point 0.05 off S along its normal has that point for its nearest (the
                // round's radius is 2 at least)
                let p = s.point(uv).unwrap();
                let q = add(p, scale(s.normal(uv).unwrap(), 0.05));
                let near = s.closest_point(q, at(&s, a + 0.04, c - 0.04)).unwrap();
                assert!(
                    length(sub(near.point, p)) <= 1e-9,
                    "{uv:?}: nearest {:?}, want {p:?}",
                    near.point
                );
                assert!((near.distance - 0.05).abs() <= 1e-9);
            }
        }
    }
}

#[test]
fn a_face_past_the_records_is_refused() {
    all_fixes();
    let (record, _) = plate().to_record().unwrap();
    let past = record.faces.len();
    assert!(matches!(
        FaceSurface::of(&record, past),
        Err(Error::Argument)
    ));
}
