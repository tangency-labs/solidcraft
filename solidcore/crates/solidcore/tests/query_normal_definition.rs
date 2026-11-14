//! A round by its definition: its normal and point are enclosed on S, the definition, not
//! on its net, so a value the net answers is measured against S (`query::normal`).
//!
//! The body is `fork_7959c0e_round_on_a_round.rs`'s twisted wedge with its front edge
//! rounded r 3: the top is a cubic spline, so the round is its definition (the ball's
//! circle of one radius between NURBS faces, which the definition's enclosure covers).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::{create_face_from_surface, fillet_edges};
use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::p1::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::tol;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::b_spline_curve::KnotType;
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::query::normal::{certify, surface_normal, surface_point, Sense};
use solidcore::surface::procedural_blend::NetScope;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel, SolidError};

/// `TWIST_TAN`: tan 30 degrees.
const TWIST_TAN: f64 = 0.577_350_269_189_625_76;

/// The kernel SolidCraft runs: every fix on but `geometric_g2_continuity`.
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

/// The fork's `twisted_top`: cubic in u over the knots -4 (4 times), 0, 4, .., 60, 64
/// (4 times), linear in v from y -1 to 31; the control points' x Greville's, their z
/// `25 + a y`, a = -tan 30 for the first seven columns, 0 for the next six, tan 30 after.
fn twisted_top() -> Surface {
    let mut ku = vec![-4.0; 4];
    let mut uk = vec![-4.0];
    let mut um = vec![4];
    for x in (0..=60).step_by(4) {
        ku.push(f64::from(x));
        uk.push(f64::from(x));
        um.push(1);
    }
    ku.extend_from_slice(&[64.0; 4]);
    uk.push(64.0);
    um.push(4);
    let nu = ku.len() - 4;
    let mut pts = Vec::new();
    for i in 0..nu {
        let x = (ku[i + 1] + ku[i + 2] + ku[i + 3]) / 3.0;
        let a = if i < 7 {
            -TWIST_TAN
        } else if i < 13 {
            0.0
        } else {
            TWIST_TAN
        };
        for y in [-1.0, 31.0] {
            pts.push(Point3::new(x, y, 25.0 + a * y));
        }
    }
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 1,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &um,
            v_multiplicities: &[2, 2],
            u_knots: &uk,
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// `smlib_split_body`'s back part: `body` sliced (`SM_BO_SLICE`, every option off) by a
/// one-face sheet of the body's tolerance on `tool` turned over.
fn split_back(k: &mut Kernel, body: Brep, tool: &Surface) -> Brep {
    let mut s = tool.clone();
    s.swap_uv(k).unwrap();
    let mut sheet =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    let t = body.tolerance();
    solidcore::topology::brep::p2::set_tolerance(k, &mut sheet, t, false, true).unwrap();
    create_face_from_surface(k, &mut sheet, &s).unwrap();
    boolean_with_options(
        k,
        body,
        sheet,
        BooleanOperation::Slice,
        MergeOptions::new(false, false, false, false, false),
    )
    .unwrap()
    .result
}

/// The edge nearest `p`, within 0.1.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

/// The twisted wedge with its front edge rounded r 3, and its round's surface.
fn rounded_wedge(k: &mut Kernel) -> Surface {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 30.0, 50.0).unwrap();
    let mut r = split_back(k, block, &twisted_top());
    let front = pick(k, &r, [30.0, 0.0, 25.0]);
    fillet_edges(k, &mut r, &[front], 3.0, 1, 1, 1.0).unwrap();
    let rounds: Vec<Surface> = r
        .face_list()
        .iter()
        .filter_map(|&f| face2::surface(&r, f).unwrap().cloned())
        .filter(|s| matches!(s.kind(), SurfaceKind::ProceduralBlend(_)))
        .collect();
    assert_eq!(rounds.len(), 1);
    rounds.into_iter().next().unwrap()
}

/// Three points well inside the round's domain.
fn points(s: &Surface) -> Vec<Point2> {
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        unreachable!()
    };
    let d = pb.net().natural_uv_domain();
    [(0.13, 0.5), (0.41, 0.27), (0.77, 0.61)]
        .iter()
        .map(|&(a, b)| {
            Point2::new(
                d.min().x + a * (d.max().x - d.min().x),
                d.min().y + b * (d.max().y - d.min().y),
            )
        })
        .collect()
}

#[test]
fn a_round_by_its_definition_is_certified_on_s_and_its_net_measured_against_s() {
    let mut k = solidcraft_kernel();
    let s = rounded_wedge(&mut k);
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        unreachable!()
    };
    let bound = pb.bound();
    // the largest distance between the net's normal and S's evaluated one, and the largest
    // bound the net's normal is given against S: a net measured against itself would be
    // given rounding (about 1e-15) where the two differ by microns
    let (mut apart, mut given) = (0.0_f64, 0.0_f64);
    for uv in points(&s) {
        // S's normal: the definition's (outside every net scope), proved. The definition's
        // enclosure at a point is as tight as its feet's Krawczyk boxes: about 1e-10 at two
        // of these points, 2e-4 at the third (the evaluator's value agrees with a finite
        // difference to 1e-6 there: the width is the enclosure's, not an error)
        let n = surface_normal(&k, &s, uv).unwrap();
        assert!(n.bound <= 1e-3, "{n:?}");
        // the other sense is refused, not turned
        assert!(certify(&k, &s, uv, -n.value, Sense::Surface).is_err());
        assert_eq!(
            certify(&k, &s, uv, -n.value, Sense::Reversed)
                .unwrap()
                .bound,
            n.bound
        );
        // S's point encloses the definition's evaluation
        let p = surface_point(&k, &s, uv).unwrap();
        let e = s.evaluate_point(&k, uv).unwrap();
        for (c, x) in p.iter().zip([e.x, e.y, e.z]) {
            assert!(c.lo - 1e-9 <= x && x <= c.hi + 1e-9, "{c:?} vs {x}");
        }
        // the net's normal, inside a net scope, measured against S: its bound covers its
        // distance from S's normal
        let on_net = {
            let _scope = NetScope::new(&k);
            s.evaluate_normal(&k, uv, Side::Left, Side::Left).unwrap()
        };
        let m = certify(&k, &s, uv, on_net, Sense::Surface).unwrap();
        assert!(
            m.bound + n.bound >= (on_net - n.value).length(),
            "{m:?} {n:?}"
        );
        apart = apart.max((on_net - n.value).length());
        given = given.max(m.bound);
        // and the net's point is within the net's certified bound of S's enclosure
        let q = {
            let _scope = NetScope::new(&k);
            s.evaluate_point(&k, uv).unwrap()
        };
        let far = p
            .iter()
            .zip([q.x, q.y, q.z])
            .map(|(c, x)| (x - c.hi).max(c.lo - x).max(0.0))
            .fold(0.0_f64, f64::max);
        assert!(far <= bound + 1e-12, "{far} vs bound {bound}");
    }
    assert!(apart > 1e-9, "the net and S differ here: {apart}");
    assert!(given >= apart, "{given} vs {apart}");
}

#[test]
fn a_definition_off_its_net_s_domain_is_refused() {
    let mut k = solidcraft_kernel();
    let s = rounded_wedge(&mut k);
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        unreachable!()
    };
    let d = pb.net().natural_uv_domain();
    let off = Point2::new(d.max().x + 1.0, d.min().y);
    assert!(matches!(
        surface_point(&k, &s, off),
        Err(SolidError::Unsupported { .. })
    ));
}

/// The faces the definition rolls on are enclosed piece by piece over the feet's boxes,
/// so no station invariant is needed: at the net's knot lines along s, the stations, where a
/// foot sits on a face's knot line and its box straddles it, the point is still enclosed (a
/// check of the feet against one piece refused 67 of the 188 the net then had), and so is
/// every point between two stations; each enclosure holds the evaluator's point. The net
/// had 181 inner knots since #2690 (the certificate accepts a level by each uv parameter's
/// own error, so some spans certify without being halved), 169 since #13984 (each proved
/// feet's box is tightened before the Taylor solve, so more spans certify unhalved), 164
/// since #14405 (the arc's rows over a piece are bounded by Taylor models too), and 113
/// since #14405's feet (the feet over a piece are bounded by Taylor models too).
#[test]
fn every_station_and_every_point_between_is_enclosed() {
    let mut k = solidcraft_kernel();
    let s = rounded_wedge(&mut k);
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        unreachable!()
    };
    let net = pb.net().gw_nurb().unwrap();
    let d = pb.net().natural_uv_domain();
    let (_, _, swapped) = pb.definition_map();
    let (knots, lo, hi) = if swapped {
        (net.v_knots(), d.min().y, d.max().y)
    } else {
        (net.u_knots(), d.min().x, d.max().x)
    };
    let t = if swapped {
        0.5 * (d.min().x + d.max().x)
    } else {
        0.5 * (d.min().y + d.max().y)
    };
    let at = |s_: f64| {
        if swapped {
            Point2::new(t, s_)
        } else {
            Point2::new(s_, t)
        }
    };
    let mut inner: Vec<f64> = knots
        .iter()
        .copied()
        .filter(|&x| x > lo && x < hi)
        .collect();
    inner.dedup();
    assert_eq!(inner.len(), 113);
    let mut points = Vec::new();
    for w in inner.windows(2) {
        points.push(w[0]);
        points.push(0.5 * (w[0] + w[1]));
    }
    for x in points {
        let uv = at(x);
        let p = surface_point(&k, &s, uv).unwrap_or_else(|e| panic!("{x}: {e:?}"));
        let e = s.evaluate_point(&k, uv).unwrap();
        for (c, v) in p.iter().zip([e.x, e.y, e.z]) {
            assert!(c.lo - 1e-9 <= v && v <= c.hi + 1e-9, "{x}: {c:?} vs {v}");
        }
        let n = surface_normal(&k, &s, uv).unwrap_or_else(|e| panic!("{x}: {e:?}"));
        assert!(n.bound <= 1e-2, "{x}: {n:?}");
    }
}
