//! Blend refusals as data (`SolidError::BlendRefused`, `blend::blend_refusal`) and the
//! tangent chain query, each beside a case the kernel builds.
//!
//! - The tangent chain of a tabbed plate's R2 corner arc (#2176's plate): the arc runs on
//!   tangentially into the straight top edge and the tab's side, and from them into the other
//!   corner arc and side; the tab's sides end where they meet the R25 rim at an angle. A box
//!   edge runs on into nothing.
//! - An edge whose faces meet tangentially all along it (the plate's seam where a corner's
//!   cylinder runs into a flat side) has no blend: `EdgeNotBlendable { reason: Smooth }`,
//!   status `SM_ERR_INVALID_INPUT` as before.
//! - A round too small to change the part: across a
//!   crease whose faces turn by delta, a round of radius r lies r (sec(delta/2) - 1) inside
//!   the edge; below the edge's tolerance it is refused with the smallest radius that rounds
//!   it, tolerance / (sec(delta/2) - 1).
//! - A setback asked for that does not fit on the corner's edges, or stops short of where the
//!   rails cross (#2310's box corner): `SetbackTooLong { largest }`, `SetbackTooShort { least }`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::blend_refusal::{round_too_small, tangent_chain, BlendRefusal, NotBlendable};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::curve::curve::Curve;
use solidcore::error::{SolidError, SM_ERR, SM_ERR_INVALID_INPUT};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::{edge, Brep, EdgeId, VertexId};
use std::f64::consts::PI;

/// A point on the plate's R2 corner arc at x > 0.
const CORNER: [f64; 3] = [3.414_213_56, 32.525_324_7, 2.0];
/// A point on the top R25 rim.
const RIM: [f64; 3] = [0.0, -25.0, 2.0];
/// A point on the seam where the corner's cylinder runs into the tab's flat side.
const SEAM: [f64; 3] = [4.0, 280.0 / 9.0, 1.0];

/// Every opt-in fix on except `geometric_g2_continuity`, then `set` applied.
fn kernel(set: impl FnOnce(&mut UpstreamFixes)) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    set(&mut f);
    k.fixes = f;
    k
}

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

fn arc(c: (f64, f64), r: f64, from: f64, to: f64) -> Curve {
    Curve::from(
        create_arc(
            Vec3::new(c.0, c.1, 0.0),
            r,
            from.to_degrees(),
            to.to_degrees(),
        )
        .unwrap(),
    )
}

/// R25 disc with a tab (sides x = +-4, R2 corners, top y = 280/9 + 2), 2 thick (#2176).
fn plate(k: &mut Kernel) -> Brep {
    let t = (4.0_f64 / 25.0).acos();
    let y = (625.0_f64 - 16.0).sqrt();
    let cy = 280.0 / 9.0;
    let curves = [
        arc((0.0, 0.0), 25.0, PI - t - 2.0 * PI, t),
        line((4.0, y), (4.0, cy)),
        arc((2.0, cy), 2.0, 0.0, PI / 2.0),
        line((2.0, cy + 2.0), (-2.0, cy + 2.0)),
        arc((-2.0, cy), 2.0, PI / 2.0, PI),
        line((-4.0, cy), (-4.0, y)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true).unwrap()
}

/// The slope of [`crease`]'s second floor face: its faces turn by `atan(SLOPE)` across the
/// edge at x = 10.
const SLOPE: f64 = 0.0875;

/// A 20 x 10 block, 5 thick, whose floor turns up by `atan(SLOPE)` (5.0 degrees) at x = 10:
/// the profile (0, 0), (10, 0), (20, 10 SLOPE), (20, 10), (0, 10) swept along z.
fn crease(k: &mut Kernel) -> Brep {
    let curves = [
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (20.0, 10.0 * SLOPE)),
        line((20.0, 10.0 * SLOPE), (20.0, 10.0)),
        line((20.0, 10.0), (0.0, 10.0)),
        line((0.0, 10.0), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 5.0, true).unwrap()
}

/// The z of both ends of `e`.
fn end_heights(b: &Brep, e: EdgeId) -> [f64; 2] {
    let at = |v: Option<VertexId>| b.vertex(v.unwrap()).unwrap().point.z;
    [
        at(edge::start_vertex(b, e).unwrap()),
        at(edge::end_vertex(b, e).unwrap()),
    ]
}

/// The corner arc's chain is the tab's five top edges: both corner arcs, the straight top
/// and both sides, each on the top face; the rim, which meets the sides at an angle, is not
/// in it.
#[test]
fn the_corner_arc_runs_on_round_the_tab() {
    let mut k = kernel(|_| {});
    let mut b = plate(&mut k);
    let arc = pick(&k, &b, CORNER);
    let chain = tangent_chain(&k, &mut b, &[arc], 0.5_f64.to_radians()).unwrap();
    assert_eq!(chain.len(), 5, "{chain:?}");
    assert_eq!(chain[0], arc);
    for &e in &chain {
        assert_eq!(
            end_heights(&b, e),
            [2.0, 2.0],
            "edge {e:?} is off the top face"
        );
    }
    let tab = [
        [4.0, 29.0, 2.0],
        [0.0, 280.0 / 9.0 + 2.0, 2.0],
        [-3.414_213_56, 32.525_324_7, 2.0],
        [-4.0, 29.0, 2.0],
    ];
    for p in tab {
        assert!(chain.contains(&pick(&k, &b, p)), "{p:?} not in the chain");
    }
    assert!(!chain.contains(&pick(&k, &b, RIM)));
    // the chain of any of its edges is the same five
    let side = pick(&k, &b, tab[3]);
    let again = tangent_chain(&k, &mut b, &[side, side], 0.5_f64.to_radians()).unwrap();
    assert_eq!(again.len(), 5);
    assert!(chain.iter().all(|e| again.contains(e)));
    // an angle that is no tangency is refused
    assert!(tangent_chain(&k, &mut b, &[arc], 0.0).is_err());
}

/// A box edge runs on into no other: the edges at its ends turn a right angle.
#[test]
fn a_box_edge_is_its_own_chain() {
    let mut k = kernel(|_| {});
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let e = pick(&k, &b, [5.0, 0.0, 0.0]);
    assert_eq!(
        tangent_chain(&k, &mut b, &[e], 0.5_f64.to_radians()).unwrap(),
        vec![e]
    );
}

/// The seam where the corner's cylinder runs into the tab's flat side has no blend: its faces
/// meet tangentially all along it. The arc beside it prepares.
#[test]
fn an_edge_between_tangent_faces_has_no_blend() {
    let mut k = kernel(|_| {});
    let mut b = plate(&mut k);
    let seam = pick(&k, &b, SEAM);
    let arc = pick(&k, &b, CORNER);
    let circular = [FilletSurfaceGeneratorType::Circular];
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        let err = s
            .prepare_per_edge_constant_radius(&mut k, &[arc, seam], &[0.5], &circular, 1, 1.0)
            .unwrap_err();
        assert_eq!(
            err,
            SolidError::BlendRefused {
                operation: "blend::fillet_executive::session::edge_refusals",
                code: SM_ERR_INVALID_INPUT,
                refusal: BlendRefusal::EdgeNotBlendable {
                    edge: seam,
                    reason: NotBlendable::Smooth,
                },
            }
        );
        assert_eq!(err.sm_status(), Some(SM_ERR_INVALID_INPUT));
    }
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(&mut k, &[arc], &[0.5], &circular, 1, 1.0)
            .unwrap();
        assert!(matches!(
            s.blendable_edgeuse(&k, seam),
            Err(SolidError::BlendRefused {
                refusal: BlendRefusal::EdgeNotBlendable {
                    reason: NotBlendable::Smooth,
                    ..
                },
                ..
            })
        ));
    }
}

/// The crease's faces turn by atan(SLOPE): a round of radius r lies r (sec(turn/2) - 1)
/// inside the edge. One whose depth is below the edge's tolerance is refused with the
/// smallest radius that rounds it; that radius, a hair more, is not; a round of 0.5 builds.
#[test]
fn a_round_within_the_tolerance_of_its_edge_is_refused() {
    let turn = SLOPE.atan();
    let reach = 1.0 / (0.5 * turn).cos() - 1.0;
    let mut k = kernel(|_| {});
    let mut b = crease(&mut k);
    let e = pick(&k, &b, [10.0, 0.0, 2.5]);
    let tolerance = b.edge(e).unwrap().tolerance.max(b.tolerance());
    let smallest = tolerance / reach;
    let small = 0.5 * smallest;
    let Some(BlendRefusal::RoundTooSmall {
        edge,
        turn: found,
        tolerance: tol,
        smallest_radius,
    }) = round_too_small(&k, &mut b, e, small).unwrap()
    else {
        panic!("not refused");
    };
    assert_eq!(edge, e);
    assert!((found - turn).abs() <= 1e-12, "turn {found}, atan {turn}");
    assert_eq!(tol, tolerance);
    assert!(
        (smallest_radius - smallest).abs() <= 1e-9 * smallest,
        "{smallest_radius} {smallest}"
    );
    assert_eq!(
        round_too_small(&k, &mut b, e, 1.001 * smallest).unwrap(),
        None
    );
    // through the preparation, with the fix: refused, the status SM_ERR
    {
        let circular = [FilletSurfaceGeneratorType::Circular];
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        let err = s
            .prepare_per_edge_constant_radius(&mut k, &[e], &[small], &circular, 1, 1.0)
            .unwrap_err();
        assert!(
            matches!(
                err,
                SolidError::BlendRefused {
                    code: SM_ERR,
                    refusal: BlendRefusal::RoundTooSmall { .. },
                    ..
                }
            ),
            "{err:?}"
        );
    }
    let faces = b.face_list().len();
    fillet_edges(&mut k, &mut b, &[e], 0.5, 1, 1, 1.0).unwrap();
    assert_eq!(b.face_list().len(), faces + 1);
}

/// Per edge of `b`: its tolerance and which of its edge uses have a UV trim curve.
fn trims_and_tolerances(b: &Brep) -> Vec<(f64, Vec<bool>)> {
    let edges = solidcore::topology::brep::p1::edges(b, None).unwrap();
    edges
        .iter()
        .map(|&e| {
            let r = b.edge(e).unwrap();
            let trims = r
                .edgeuses
                .iter()
                .map(|&eu| b.edgeuse(eu).unwrap().uv_trim_curve.is_some())
                .collect();
            (r.tolerance, trims)
        })
        .collect()
}

/// The too-small check is a query: a round it refuses leaves the body as it was. Its
/// evaluations build the UV trim curves an edge use lacks and refit the edge's tolerance;
/// on the plate, ten of its 46 edge uses without one got one from the check alone, which
/// on the caller's body moved every later result (#2718's sweep: edge tolerances of 1.03e-5
/// where upstream keeps 1e-5).
#[test]
fn a_round_refused_as_too_small_leaves_the_body_as_it_was() {
    let mut k = kernel(|_| {});
    let mut b = plate(&mut k);
    let edges = solidcore::topology::brep::p1::edges(&b, None).unwrap();
    let mut refused = 0;
    for &e in &edges {
        let before = trims_and_tolerances(&b);
        // the session's preparation: the API's own floor (8 tolerances) is above the
        // smallest radius of a right-angle edge (tolerance / (sec(pi/4) - 1))
        let circular = [FilletSurfaceGeneratorType::Circular];
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        let err = s
            .prepare_per_edge_constant_radius(&mut k, &[e], &[1e-6], &circular, 1, 1.0)
            .unwrap_err();
        drop(s);
        if matches!(
            err,
            SolidError::BlendRefused {
                refusal: BlendRefusal::RoundTooSmall { .. },
                ..
            }
        ) {
            refused += 1;
            assert_eq!(
                before,
                trims_and_tolerances(&b),
                "edge {e:?}: the refused round changed the body"
            );
        }
    }
    assert!(refused >= 10, "{refused} refused");
}

#[test]
fn a_setback_asked_that_does_not_fit_or_stops_short_is_refused() {
    let at = [[30.0, 40.0, 20.0], [60.0, 20.0, 20.0], [60.0, 40.0, 10.0]];
    let corner = Point3::new(60.0, 40.0, 20.0);
    let run = |setback: f64| -> (Result<(), SolidError>, VertexId) {
        let mut k = kernel(|f| {
            f.fillet_exact_round_classes = false;
            f.face_swap_uv_transposes_trims = false;
        });
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
        let edges: Vec<EdgeId> = at.iter().map(|&p| pick(&k, &b, p)).collect();
        let v = b
            .vertices()
            .ids()
            .find(|&v| b.vertex(v).unwrap().point.distance(corner) < 1e-9)
            .unwrap();
        let xs = vec![FilletSurfaceGeneratorType::Circular; 3];
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(&mut k, &edges, &[5.0; 3], &xs, 1, 1.0)
            .unwrap();
        s.add_set_back_vertex(v);
        s.set_set_back_fill(v, setback, 0.0, false);
        s.create_corners(&k, None).unwrap();
        for c in s.fillet_corners().unwrap() {
            if s.corner(c).unwrap().vertex == Some(v) {
                s.set_corner_setback(c, setback).unwrap();
            }
        }
        (s.do_filleting(&mut k, None), v)
    };
    let (r, v) = run(20.0);
    let err = r.unwrap_err();
    assert_eq!(err.sm_status(), Some(SM_ERR));
    assert!(
        matches!(
            err,
            SolidError::BlendRefused {
                refusal: BlendRefusal::SetbackTooLong { vertex, largest },
                ..
            } if vertex == v && largest == 20.0
        ),
        "{err:?}"
    );
    let (r, v) = run(0.0);
    let err = r.unwrap_err();
    assert!(
        matches!(
            err,
            SolidError::BlendRefused {
                refusal: BlendRefusal::SetbackTooShort { vertex, least },
                ..
            } if vertex == v && (least - 5.0).abs() <= 1e-9
        ),
        "{err:?}"
    );
    run(8.0).0.unwrap();
}
