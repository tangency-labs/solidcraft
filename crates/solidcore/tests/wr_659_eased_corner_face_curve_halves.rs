//! WR-659 (the fork's tangency-labs/smlib@4702889): at a set-back corner whose rounds ease
//! (`FilletSession::set_corner_ease`, the fork's `e74bbd3`), each face curve of the
//! corner's hole (on a box face, between two rounds' contact lines, split at its middle)
//! bounds the N-sided patch as that curve's own half: one quintic span, curvature-continuous
//! with its contact line and with the other half. Upstream bounds the patch faces by the
//! patch surfaces' iso-curves, which `N_FillNSidedHole` fits to the halves only to
//! tolerance, in its surfaces' degree and knots (degree 6 over 7 spans). The fork's case:
//! SolidCraft's G2 corner on the 60 x 40 x 20 box, its three edges at (60, 40, 20) rounded
//! r 5, set back and eased (`smlib_blend_setback_open_eased`; capi test
//! `corner_setback_eased_face_curves_are_quintics`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::{edge, vertex, Brep, EdgeId};
use solidcore::{Kernel, VertexId};

const CORNER: [f64; 3] = [60.0, 40.0, 20.0];

fn at_point(b: &Brep, v: VertexId, p: [f64; 3]) -> bool {
    let q = vertex::point(b, v).unwrap();
    (q.x - p[0]).abs() + (q.y - p[1]).abs() + (q.z - p[2]).abs() < 1e-6
}

/// The box with its three edges at `CORNER` rounded r 5 by the curvature-continuous blend,
/// the corner set back `setback` and eased `ease`, under `halves`
/// (`fillet_corner_face_curve_halves`); and the corner patch's faces.
fn build(setback: f64, ease: f64, halves: bool) -> (Kernel, Brep, Vec<solidcore::FaceId>) {
    let mut k = Kernel::default();
    k.fixes.fillet_setback_from_start_vertex = true;
    k.fixes.fillet_corner_face_curve_halves = halves;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let edges: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            [s, t]
                .into_iter()
                .flatten()
                .any(|v| at_point(&b, v, CORNER))
        })
        .collect();
    assert_eq!(edges.len(), 3);
    let patch;
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.set_mark_corner_patches(true);
        s.set_blend_curve_at_thirds(true);
        s.prepare_constant_radius_edges(&mut k, &edges, 5.0, 2, 2, 1.0)
            .unwrap();
        s.create_corners(&k, None).unwrap();
        let corners = s.fillet_corners().unwrap();
        let found: Vec<_> = corners
            .iter()
            .copied()
            .filter(|&id| {
                s.corner(id)
                    .unwrap()
                    .vertex
                    .is_some_and(|v| at_point(s.target(), v, CORNER))
            })
            .collect();
        assert_eq!(found.len(), 1);
        s.set_corner_setback(found[0], setback).unwrap();
        s.set_corner_ease(found[0], ease).unwrap();
        s.do_filleting(&mut k, None).unwrap();
        patch = s
            .corner_patch_faces()
            .unwrap()
            .into_iter()
            .map(|(_, f)| f)
            .collect();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    // the session-level calls leave the port's cached topology fields to their caller: the
    // public fillet API refreshes them before it returns
    // (`blend::fillet_executive::api`), and so does this caller of the session
    b.refresh_cached_topology();
    (k, b, patch)
}

/// The edges of the patch's faces that a planar box face shares: the face curves' halves.
fn face_curve_halves(b: &Brep, patch: &[solidcore::FaceId]) -> Vec<EdgeId> {
    let mut out = Vec::new();
    for e in p1::edges(b, None).unwrap() {
        let faces = edge::faces(b, e, None).unwrap();
        let on_patch = faces.iter().any(|f| patch.contains(f));
        let on_plane = faces.iter().any(|&f| {
            matches!(
                b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Plane(_)
            )
        });
        if on_patch && on_plane {
            out.push(e);
        }
    }
    out
}

/// `e`'s curve's degree and number of spans.
fn degree_and_spans(k: &Kernel, b: &Brep, e: EdgeId) -> (usize, usize) {
    let c = edge::curve(b, e).unwrap().unwrap();
    let (knots, _) = c.knots(k, None).unwrap();
    (c.degree().unwrap(), knots.len().saturating_sub(1))
}

/// The curvature of `e`'s curve at its end on `v`.
fn end_curvature(k: &Kernel, b: &Brep, e: EdgeId, v: VertexId) -> f64 {
    let ivl = edge::interval(b, e).unwrap();
    let t = if edge::start_vertex(b, e).unwrap() == Some(v) {
        ivl.min()
    } else {
        ivl.max()
    };
    let curve = edge::curve(b, e).unwrap().unwrap();
    let d = curve.evaluate(k, t, 2, true, true).unwrap();
    let speed = d[1].length();
    d[1].cross(d[2]).length() / (speed * speed * speed)
}

/// Flag on: every face curve half is one span of degree at most 5; where two halves meet
/// their curvatures agree, at its other end (on its straight contact line) it is flat; the
/// result is a manifold solid.
fn assert_quintic_halves(setback: f64, ease: f64) {
    let (k, b, patch) = build(setback, ease, true);
    assert!(
        is_manifold_solid(&b).unwrap(),
        "setback {setback}, ease {ease}"
    );
    if let Err(e) = solidcore::topology::integrity::check(&b) {
        panic!("setback {setback}, ease {ease}: integrity {e:?}");
    }
    let halves = face_curve_halves(&b, &patch);
    assert!(
        halves.len() >= 6,
        "setback {setback}, ease {ease}: {halves:?}"
    );
    for &e in &halves {
        let (degree, spans) = degree_and_spans(&k, &b, e);
        assert!(
            degree <= 5 && spans == 1,
            "setback {setback}, ease {ease}, edge {e:?}: degree {degree}, {spans} spans"
        );
        let (s, t) = edge::start_end_vertices(&b, e).unwrap();
        for v in [s, t].into_iter().flatten() {
            let kappa = end_curvature(&k, &b, e, v);
            let other = halves
                .iter()
                .copied()
                .find(|&o| o != e && edge::vertices(&b, o, None).unwrap().contains(&Some(v)));
            match other {
                Some(o) => {
                    let kappa_o = end_curvature(&k, &b, o, v);
                    assert!(
                        (kappa - kappa_o).abs() <= 1e-9 * (1.0 + kappa.abs()),
                        "setback {setback}, ease {ease}: halves {e:?} {o:?} at {v:?}: {kappa} vs {kappa_o}"
                    );
                }
                None => assert!(
                    kappa <= 1e-9,
                    "setback {setback}, ease {ease}: edge {e:?} at its contact line: {kappa}"
                ),
            }
        }
    }
}

/// Off (upstream's MakeFaceBrep): a face curve half is the patch's iso-curve, fitted in its
/// surfaces' degree (6).
#[test]
fn wr_659_off_a_face_curve_half_is_the_fitted_iso_curve() {
    let (k, b, patch) = build(10.0, 3.0, false);
    let halves = face_curve_halves(&b, &patch);
    assert!(!halves.is_empty());
    let degrees: Vec<(usize, usize)> = halves
        .iter()
        .map(|&e| degree_and_spans(&k, &b, e))
        .collect();
    assert!(degrees.iter().any(|&(d, _)| d == 6), "{degrees:?}");
}

#[test]
fn wr_659_on_each_face_curve_half_is_its_own_quintic() {
    assert_quintic_halves(10.0, 3.0);
}

/// The fork's test ranges, where the setback and ease fit on the 20 edge with the round.
#[test]
fn wr_659_on_over_setbacks_and_eases() {
    for setback in [8.0, 10.0, 12.0] {
        for ease in [0.5, 1.25, 3.0] {
            assert_quintic_halves(setback, ease);
        }
    }
}
