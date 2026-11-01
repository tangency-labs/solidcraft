//! tangency-labs/solidcore#2175: a chamfer whose rail leaves its face before the end of
//! the filleted edge, where the edge meets the face's side edge tangentially, was not
//! refused with `fillet_refuse_rails_leaving_faces` (WR-302): `do_filleting` returned
//! `Ok` with an open body.
//!
//! The body is a block with an arc side (17.5 high) less a ball r 5 at (2.5, 22.5, 5).
//! The ball's rim on the back face y = 22.5 touches the bottom face z = 0 at
//! (2.5, 22.5, 0). The rim edge from (2.5, 22.5, 10) through (7.5, 22.5, 5) to
//! (2.5, 22.5, 0) is chamfered 0.5 (a linear section, bevelled corners). Each rail,
//! untrimmed, is a closed circle. On the ball's face the rail (radius 4.979 about
//! (2.5, 22.045, 5)) leaves the face past the top end, at (-0.207, 22.045, 9.179), runs
//! round the far side of the circle, and comes back onto it past the bottom end, at
//! (-0.207, 22.045, 0.821). `RailsFitTheirFaces` skipped that middle off-face run (the
//! #1580 skip of a run wholly past an end) because both of its ends lie past an end of
//! the edge, though not the same one. The fork (tangency-labs/smlib at 5afca53, the same
//! chamfer through `SmFilletExecutive`, rails identical to the port's with the flag)
//! judges the run and refuses the chamfer with `SM_FILERR_NO_ROOM` (8): "a rail leaves
//! the face it rolls on, and the fillet ends on more than one face". Pinned SMLib returns
//! success with the open body (14 faces, not a manifold solid), as the port does without
//! the flag.
//!
//! The #1580 skip now asks for both ends of the run on or beyond the cross section at the
//! same end of the filleted edge.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::create_sphere;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use solidcore::topology::Brep;
use solidcore::topology::EdgeId;
use std::f64::consts::PI;

/// The edge nearest `p`.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// A block with an arc side, a ball r 5 at (2.5, 22.5, 5) cut from its back face y = 22.5.
fn body(k: &mut Kernel) -> Brep {
    let (c, r) = ((24.0625, 11.25), 26.5625);
    let a = |p: (f64, f64)| (p.1 - c.1).atan2(p.0 - c.0);
    let (s, mut e) = (a((0.0, 22.5)), a((0.0, 0.0)));
    while e < s {
        e += 2.0 * PI;
    }
    let curves = [
        line((0.0, 0.0), (47.5, 0.0)),
        line((47.5, 0.0), (47.5, 22.5)),
        line((47.5, 22.5), (27.5, 22.5)),
        line((27.5, 22.5), (27.5, 12.5)),
        line((27.5, 12.5), (15.0, 12.5)),
        line((15.0, 12.5), (15.0, 22.5)),
        line((15.0, 22.5), (0.0, 22.5)),
        Curve::from(
            create_arc(Vec3::new(c.0, c.1, 0.0), r, s.to_degrees(), e.to_degrees()).unwrap(),
        ),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let block = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 17.5, true).unwrap();
    let ball = create_sphere(k, Point3::new(2.5, 22.5, 5.0), 5.0).unwrap();
    boolean_difference(k, block, ball).unwrap()
}

/// distance * tan(a / 2), a the angle between the directions into the faces at the
/// edge's midpoint (SolidCraft's chamfer setback to radius).
fn chamfer_radius(k: &Kernel, b: &mut Brep, e: EdgeId, distance: f64) -> f64 {
    let u0 = edge::blend_edgeuse(k, b, e).unwrap().unwrap();
    let u1 = eu1::radial(b, u0).unwrap().unwrap();
    let t = b.edge(e).unwrap().interval.evaluate(0.5);
    let o = BinormalOptions::default();
    let e0 = evaluate_binormal(k, b, u0, t, false, &o).unwrap();
    let e1 = evaluate_binormal(k, b, u1, t, false, &o).unwrap();
    let c = e0
        .binormal
        .unitize()
        .unwrap()
        .dot(e1.binormal.unitize().unwrap())
        .clamp(-1.0, 1.0);
    distance * ((1.0 - c) / (1.0 + c)).sqrt()
}

/// The chamfer 0.5 of the rim edge ending at the bottom face, as SolidCraft drives it.
/// `Err` holds the error info code; `Ok` the face count after stitching and whether the
/// result is a manifold solid.
fn chamfer(k: &mut Kernel) -> Result<(usize, bool), i32> {
    let mut b = body(k);
    assert_eq!(b.face_list().len(), 12);
    let e = pick(k, &b, [3.1526309873924174, 22.5, 0.04277569594852188]);
    let r = chamfer_radius(k, &mut b, e, 0.5);
    assert!((r - 0.5).abs() < 1e-9, "the faces meet square: {r}");
    let done = {
        let mut s = FilletSession::new(k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius_with_second(
            k,
            &[e],
            &[r],
            &[r],
            &[FilletSurfaceGeneratorType::Linear],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(k, None).unwrap();
        for id in s.fillet_corners().unwrap() {
            s.set_corner_bevel(id, true).unwrap();
        }
        s.do_filleting(k, None).map_err(|_| s.error_info().code)
    };
    done?;
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    Ok((b.face_list().len(), is_manifold_solid(&b).unwrap()))
}

/// SolidCraft's fixes: every `UpstreamFixes` field on except `geometric_g2_continuity`.
fn solidcraft_kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

#[test]
fn chamfer_past_the_bottom_is_no_room_with_solidcraft_fixes() {
    let mut k = solidcraft_kernel();
    assert_eq!(chamfer(&mut k), Err(FilletErrorType::NoRoom as i32));
}

#[test]
fn chamfer_past_the_bottom_is_no_room_with_the_flag() {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    assert_eq!(chamfer(&mut k), Err(FilletErrorType::NoRoom as i32));
}

/// Without the flag the port keeps pinned SMLib's result: success, an open body of 14
/// faces.
#[test]
fn chamfer_past_the_bottom_builds_the_open_body_by_default() {
    let mut k = Kernel::default();
    assert_eq!(chamfer(&mut k), Ok((14, false)));
}
