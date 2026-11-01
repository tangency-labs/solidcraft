//! Issue #2979: a split along the plane of a face of the body. The section is bounded in part by
//! that face's rim, but only where the material beyond the rim lies across the plane; and a
//! fillet tangent to the face only touches the plane, so it must not be traced along its edge.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use solidcraft_geom::{Plane, Vec3};

use crate::{Body, BoolOp, SplitTool, boolean, box_solid, fillet, measure, split_body};

const X: f64 = -3.5;

/// A block ending in a face at x = X, a smaller block standing on part of that face, and the
/// block's edge at y = 0 on that face rounded (radius 1, tangent to the face).
fn stepped() -> Body {
    let big = box_solid(Vec3::new(-10.0, 0.0, -25.0), Vec3::new(X, 50.0, 27.7)).unwrap();
    let small = box_solid(Vec3::new(X, 30.0, 7.5), Vec3::new(5.0, 50.0, 27.7)).unwrap();
    let joined = boolean(&big, &small, BoolOp::Union).unwrap().expect("a solid");
    fillet(&joined, &[Vec3::new(X, 0.0, 0.0)], 1.0).unwrap()
}

fn volumes(parts: &[Body]) -> Vec<f64> {
    let mut v: Vec<f64> = parts.iter().map(|p| measure(p).unwrap().volume).collect();
    v.sort_by(f64::total_cmp);
    v
}

fn plane() -> Plane {
    Plane::from_normal(Vec3::new(X, 0.0, 0.0), Vec3::X).unwrap()
}

fn halves(parts: &[Body]) {
    let rounded = (1.0 - PI / 4.0) * 52.7;
    let want = [8.5 * 20.0 * 20.2, 6.5 * 50.0 * 52.7 - rounded];
    let got = volumes(parts);
    assert_eq!(got.len(), 2, "{got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < w * 1e-3, "{got:?}, expected {want:?}");
    }
    for p in parts {
        assert!(p.validity().is_empty(), "{:?}", p.validity());
    }
}

#[test]
fn a_plane_along_a_step_face_splits_at_the_step() {
    let b = stepped();
    halves(&split_body(&b, &SplitTool::Plane(plane())).unwrap());
}

#[test]
fn the_step_face_itself_splits_at_the_step() {
    let b = stepped();
    let face = (0..b.face_count())
        .find(|i| matches!(crate::face_surface(&b, *i), Some(crate::FaceSurface::Plane { normal, point }) if normal.x > 0.999 && (point.x - X).abs() < 1e-9))
        .expect("the step face");
    halves(&split_body(&b, &SplitTool::Face { body: &b, face }).unwrap());
}

#[test]
fn a_plane_along_a_rounded_end_face_leaves_the_block_whole() {
    let block = box_solid(Vec3::new(-10.0, 0.0, -25.0), Vec3::new(X, 50.0, 27.7)).unwrap();
    let b = fillet(&block, &[Vec3::new(X, 0.0, 0.0)], 1.0).unwrap();
    let parts = split_body(&b, &SplitTool::Plane(plane())).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].face_count(), b.face_count());
}
