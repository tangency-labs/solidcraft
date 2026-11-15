//! WR-598 (the SolidCraft fork's tangency-labs/smlib@829b982): `DeleteTopologicalVertex`
//! between a line edge and an edge whose curve is a curve on a B-spline surface along a
//! UV line of constant u (the seam an extrusion leaves on its wall). Upstream casts the
//! `SmCrvOnSurf` copy to `SmBSplineCurve` and dereferences the null; the port leaves the
//! vertex in place (`SM_ERR`, nothing changed), and with
//! `Kernel::fixes.delete_vertex_joins_seam_iso_curve` joins the two edges into one over
//! the surface's iso curve.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p5;
use solidcore::topology::{edge, vertex, Brep, EdgeId, VertexId};

/// A line edge from the origin to (0, 0, 1) and a curve-on-surface edge from (0, 0, 1) to
/// (0, 0, 2) on the bilinear patch x in [-1, 1], z in [1, 2], along u = 0.5; the shared
/// vertex glued. Returns the B-rep, the line edge, the curve-on-surface edge and the
/// shared vertex.
fn line_and_seam(k: &mut Kernel) -> (Brep, EdgeId, EdgeId, VertexId) {
    let mut b = Brep::new_with_infinite_region().unwrap();
    let line = Curve::from(Line::from_points(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 3));
    let line_ivl = line.natural_interval();
    let e_line = p5::create_wire_edge_from_curve(k, &mut b, line, line_ivl).unwrap();

    let patch = BSplineSurface::create_bilinear_surface(
        Vec3::new(-1.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 1.0),
        Vec3::new(-1.0, 0.0, 2.0),
        Vec3::new(1.0, 0.0, 2.0),
    )
    .unwrap();
    let uv = Curve::from(Line::from_points(
        Vec3::new(0.5, 0.0, 0.0),
        Vec3::new(0.5, 1.0, 0.0),
        2,
    ));
    let cos = CrvOnSurf::new(k, uv, Surface::from(patch), None, 2).unwrap();
    let seam = Curve::from(cos);
    let seam_ivl = seam.natural_interval();
    assert!(seam
        .evaluate_point(k, seam_ivl.min())
        .unwrap()
        .closer_than(1e-12, Vec3::new(0.0, 0.0, 1.0)));
    let e_seam = p5::create_wire_edge_from_curve(k, &mut b, seam, seam_ivl).unwrap();

    let v_line = edge::end_vertex(&b, e_line).unwrap().unwrap();
    let v_seam = edge::start_vertex(&b, e_seam).unwrap().unwrap();
    p5::glue_vertices(k, &mut b, Some(v_line), Some(v_seam), None).unwrap();
    assert_eq!(vertex::edges(&b, v_line, None).unwrap().len(), 2);
    (b, e_line, e_seam, v_line)
}

/// The joined edge runs from the origin to (0, 0, 2), its curve a B-spline kind.
fn assert_joined(k: &Kernel, b: &Brep, survivor: EdgeId) {
    let s = edge::start_vertex(b, survivor).unwrap().unwrap();
    let e = edge::end_vertex(b, survivor).unwrap().unwrap();
    let mut ends = [b.vertex(s).unwrap().point, b.vertex(e).unwrap().point];
    ends.sort_by(|a, c| a.z.total_cmp(&c.z));
    assert!(ends[0].closer_than(1e-12, Vec3::ZERO), "{ends:?}");
    assert!(
        ends[1].closer_than(1e-12, Vec3::new(0.0, 0.0, 2.0)),
        "{ends:?}"
    );
    let curve = b.edge(survivor).unwrap().curve.as_ref().unwrap();
    assert!(curve.is_kind_of(solidcore::curve::curve::p2::BSPLINE_CURVE_TYPE));
    let ivl: Extent1d = b.edge(survivor).unwrap().interval;
    let mid = curve.evaluate_point(k, ivl.mid()).unwrap();
    assert!(mid.closer_than(1e-9, Vec3::new(0.0, 0.0, 1.0)), "{mid:?}");
}

#[test]
fn wr598_seam_curve_deleted_joins_with_fix_and_stays_without() {
    for fixed in [false, true] {
        let mut k = Kernel::default();
        k.fixes.delete_vertex_joins_seam_iso_curve = fixed;
        let (mut b, e_line, e_seam, v) = line_and_seam(&mut k);
        let r = p5::delete_topological_vertex(&mut k, &mut b, true, Some(v), Some(e_seam));
        if fixed {
            r.unwrap();
            assert_joined(&k, &b, e_line);
        } else {
            assert!(r.is_err(), "{r:?}");
            assert_eq!(vertex::edges(&b, v, None).unwrap().len(), 2);
            assert!(b.edge(e_seam).unwrap().curve.is_some());
        }
    }
}

#[test]
fn wr598_line_deleted_into_seam_curve_joins_with_fix_and_stays_without() {
    for fixed in [false, true] {
        let mut k = Kernel::default();
        k.fixes.delete_vertex_joins_seam_iso_curve = fixed;
        let (mut b, e_line, e_seam, v) = line_and_seam(&mut k);
        let r = p5::delete_topological_vertex(&mut k, &mut b, true, Some(v), Some(e_line));
        if fixed {
            r.unwrap();
            assert_joined(&k, &b, e_seam);
        } else {
            assert!(r.is_err(), "{r:?}");
            assert_eq!(vertex::edges(&b, v, None).unwrap().len(), 2);
        }
    }
}

/// A B-spline line surviving the join of an `SmLine` (both `SmBSplineCurve` kinds, which
/// upstream joins at the pin too): the line's NURB is made for `JoinWith`, as upstream's
/// `GetOrCreateGwNurbPointer` makes it, so the two edges become one.
#[test]
fn wr598_bspline_survivor_joins_a_line_without_its_nurb() {
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    let mut k = Kernel::default();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let line = Curve::from(Line::from_points(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 3));
    let ivl = line.natural_interval();
    let e_line = p5::create_wire_edge_from_curve(&mut k, &mut b, line, ivl).unwrap();
    let bs = Curve::from(
        BSplineCurve::create_line_segment(3, Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 2.0))
            .unwrap(),
    );
    let ivl = bs.natural_interval();
    let e_bs = p5::create_wire_edge_from_curve(&mut k, &mut b, bs, ivl).unwrap();
    let v = edge::end_vertex(&b, e_line).unwrap().unwrap();
    let v2 = edge::start_vertex(&b, e_bs).unwrap().unwrap();
    p5::glue_vertices(&mut k, &mut b, Some(v), Some(v2), None).unwrap();
    p5::delete_topological_vertex(&mut k, &mut b, true, Some(v), Some(e_line)).unwrap();
    assert_joined(&k, &b, e_bs);
}
