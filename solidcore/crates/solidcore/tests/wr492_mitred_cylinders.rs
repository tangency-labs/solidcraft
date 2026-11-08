//! WR-492: `SmMerge::MakeFaceuse` returns `SM_ERR` (1001) when the edge `MergeEdge` matches
//! has no end at the mate of the other edge's vertex. Two r 2.75 cylinders whose axes meet at
//! a small turn, each cut back to the mitre plane that halves it, meet face to face there; their
//! union fails for turns up to about 0.3 degrees (C++ SMLib v1.0.1 the same: 0.1, 0.25 and 0.3
//! fail, 0.4 and up succeed). SolidCraft's tube corners (a52 `lead-out-minus`) are the
//! repro. No fix yet: the record WR-492 says why.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_analytic_circle, create_line_segment};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::proto_topology::p1::api_transform::{
    transform_independent_object, TransformObject,
};
use solidcore::{Brep, Kernel};

const RADIUS: f64 = 2.75;
const SIZE: f64 = 260.077_476_451_483_79;

fn unit(v: Vec3) -> Vec3 {
    v / v.length()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// A cylinder along `axis` through `at`, from `before` behind it to `after` ahead of it.
fn cylinder(k: &mut Kernel, at: Vec3, axis: Vec3, before: f64, after: f64) -> Brep {
    let x = unit(axis.cross(Vec3::X));
    let c = at - axis * before;
    let mut circle =
        Curve::from(create_analytic_circle(k, Point3::new(0.0, 0.0, 0.0), RADIUS).unwrap());
    let placement = Axis2Placement::new(c, x, axis.cross(x));
    transform_independent_object(
        k,
        Some(TransformObject::Curve(&mut circle)),
        &placement,
        None,
    )
    .unwrap();
    create_linear_sweep(k, &[Some(&circle)], axis, before + after, true).unwrap()
}

/// The box `size` wide on the far side of the plane through `at` with normal `n`.
fn half_space(k: &mut Kernel, at: Vec3, n: Vec3) -> Brep {
    let u = unit(n.cross(if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }));
    let v = n.cross(u);
    let h = SIZE * 0.5;
    let c = [
        at + (u + v) * h,
        at + (v - u) * h,
        at - (u + v) * h,
        at + (u - v) * h,
    ];
    let square: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(c[i], c[(i + 1) % 4]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = square.iter().map(Some).collect();
    create_linear_sweep(k, &refs, n, SIZE, true).unwrap()
}

/// Two cylinders `turn` degrees apart (axes symmetric about X), each cut back to the mitre
/// plane, and the volumes of the two pieces; their union's result.
fn mitred_union(turn: f64) -> (solidcore::Result<Brep>, Kernel, f64) {
    let mut k = Kernel::default();
    let at = Vec3::new(23.5802, -12.5892, 51.2134);
    let a = turn.to_radians() * 0.5;
    let (ta, tb) = (
        Vec3::new(a.cos(), a.sin(), 0.0),
        Vec3::new(a.cos(), -a.sin(), 0.0),
    );
    let m = unit(ta + tb);
    let ca = cylinder(&mut k, at, ta, 10.0, 12.0);
    let hs = half_space(&mut k, at, m);
    let mut ca = boolean_difference(&mut k, ca, hs).unwrap();
    let cb = cylinder(&mut k, at, tb, 10.0, 12.0);
    let hs = half_space(&mut k, at, -m);
    let mut cb = boolean_difference(&mut k, cb, hs).unwrap();
    let pieces = volume(&k, &mut ca) + volume(&k, &mut cb);
    let r = boolean_union(&mut k, ca, cb);
    (r, k, pieces)
}

#[test]
fn small_turns_fail_like_cpp() {
    for turn in [0.1, 0.25, 0.3] {
        let (r, ..) = mitred_union(turn);
        let e = r.unwrap_err().to_string();
        assert!(
            e.contains("make_faceuse") && e.contains("1001"),
            "turn {turn}: {e}"
        );
    }
}

#[test]
fn larger_turns_unite_the_pieces() {
    for turn in [0.4, 0.5, 1.0, 5.0, 90.0] {
        let (r, k, pieces) = mitred_union(turn);
        let mut b = r.unwrap();
        assert!(is_manifold_solid(&b).unwrap(), "turn {turn}");
        // The pieces only touch on the mitre plane, so the union's volume is their sum.
        let v = volume(&k, &mut b);
        assert!(
            (v - pieces).abs() <= 1e-6 * pieces,
            "turn {turn}: {v} against {pieces}"
        );
    }
}
