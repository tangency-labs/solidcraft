//! Clone audit (follow-up to tangency-labs/solidcore#1235): the offset surfaces of the
//! constant-radius and constant-distance fillet solvers made from an edge use share the
//! faces' surfaces
//! (`SmOffsetSurface(d, *pFace->GetSurface())` keeps `m_pSurface(&rSurface)`). The port
//! copies the surface, and the copy constructor's knot fix-up (`sm_FixupKnotVector`)
//! spreads near-coincident knots, so the offset's base was other geometry than the
//! face's when the face surface carries knots closer than `SM_EFF_ZERO_PARAM`.
#![allow(clippy::unwrap_used)] // test helpers
use solidcore::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
use solidcore::blend::fillet_standard_solver::p1::{
    constant_radius_from_edgeuse, ConstantDistanceFs,
};
use solidcore::geom::vector2d::Point2;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::EdgeuseId;
use solidcore::{Brep, Kernel};

/// The face surface's u knots, or of the offset's base.
fn u_knots(s: &Surface) -> Vec<f64> {
    // (empty for another kind, which no raw-knot vector contains)
    match s.kind() {
        SurfaceKind::BSpline(b) => b.gw_nurb().unwrap().u_knots().to_vec(),
        _ => Vec::new(),
    }
}

/// A 10-unit box whose two faces at its first edge are bilinear B-splines over the
/// planes' own parametrisations, with interior u knots 1e-10 apart (kept as given: a
/// borrowed NURB); the edge use and the faces' u knot vectors.
fn raw_knot_box(k: &mut Kernel) -> (Brep, EdgeuseId, Vec<Vec<f64>>) {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let e = b.edges().ids().next().unwrap();
    let eu = eu1::primary_edgeuse(&b, e).unwrap().unwrap();
    let radial = eu1::radial(&b, eu).unwrap().unwrap();

    // Both faces at the edge become bilinear B-splines over the plane's own
    // parametrisation, with interior u knots 1e-10 apart (kept as given: a borrowed NURB).
    let mut raw = Vec::new();
    for use_ in [eu, radial] {
        let f = eu1::face(&b, use_).unwrap().unwrap();
        let plane = b.face(f).unwrap().surface.clone().unwrap();
        let d = plane.natural_uv_domain_with_nurb(k).unwrap();
        let (u0, u1, v0, v1) = (d.min().x, d.max().x, d.min().y, d.max().y);
        let m = 0.5 * (u0 + u1);
        let us = [u0, m, m + 1e-10, u1];
        let net: Vec<Vec<CPoint>> = us
            .iter()
            .map(|&u| {
                [v0, v1]
                    .iter()
                    .map(|&v| {
                        let p = plane.evaluate_point(k, Point2::new(u, v)).unwrap();
                        CPoint::new(p.x, p.y, p.z, NOW)
                    })
                    .collect()
            })
            .collect();
        let knots_u = vec![u0, u0, m, m + 1e-10, u1, u1];
        let nurb = NurbSurface::new(net, 1, 1, knots_u.clone(), vec![v0, v0, v1, v1]).unwrap();
        let s = Surface::from(BSplineSurface::from_nurb(nurb, true));
        assert_eq!(u_knots(&s), knots_u);
        b.face_mut(f).unwrap().surface = Some(s);
        raw.push(knots_u);
    }

    (b, eu, raw)
}

#[test]
fn constant_radius_offset_bases_keep_the_face_surfaces_knots() {
    let mut k = Kernel::default();
    let (mut b, eu, raw) = raw_knot_box(&mut k);
    let mut pseudo = FilletBrep::new();
    let fs = constant_radius_from_edgeuse(
        &k,
        &mut b,
        &mut pseudo,
        FilletSolverId(0),
        1e-6,
        0.5,
        0.03,
        1.0,
        eu,
        None,
    )
    .unwrap();
    // the offsets' bases are the faces' surfaces, knots exactly as they are
    for rail in 0..2 {
        let base = fs.surface(rail).unwrap().base_surface();
        assert!(raw.contains(&u_knots(base)), "{:?}", u_knots(base));
    }
}

#[test]
fn constant_distance_offset_bases_keep_the_face_surfaces_knots() {
    let mut k = Kernel::default();
    let (mut b, eu, raw) = raw_knot_box(&mut k);
    let mut pseudo = FilletBrep::new();
    let fs = ConstantDistanceFs::from_edgeuse(
        &k,
        &mut b,
        &mut pseudo,
        FilletSolverId(0),
        1e-6,
        0.5,
        0.03,
        1.0,
        eu,
    )
    .unwrap();
    for rail in 0..2 {
        let slot = fs.solver.surface(rail).unwrap().unwrap();
        let base = slot.surface.base_surface();
        assert!(raw.contains(&u_knots(base)), "{:?}", u_knots(base));
    }
}
