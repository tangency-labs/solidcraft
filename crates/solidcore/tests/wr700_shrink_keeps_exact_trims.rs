//! WR-700 (the fork's tangency-labs/smlib@6f811b8): `SmFace::ShrinkGeometry` keeps a
//! face's trim curves where its surface's trim keeps the surface's parameterization
//! (`SmSurface::TrimKeepsParameterization`), and WR-702 (@d80ea93): a shrink bound within
//! 64 ulp of a knot ends on that knot.
//!
//! The cases are the fork's capi `SHRINK_CASES` (5c13ed7, 9245ff7). Each builds a face
//! on the sub-domain `[u0, u1] x [v0, v1]` of a surface, bounded by the surface's own
//! iso curves (exact B-spline extractions) and their UV lines, made with the trim curves
//! kept (`pbOptCrvOnSurf`). Then the face is shrunk. Where the trim keeps the map
//! (a B-spline patch; a cylinder trimmed along its axis), the trim curves stay as given and
//! lie on their edges to 1e-12. Everywhere else (a cylinder trimmed in its angle, a
//! torus along its generator, a plane), and with the flag off, they're removed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData};
use solidcore::nurbs::b_spline_surface::p2::IsoCurveOptions;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::edgeuse::p1::uv_trim_curve;
use solidcore::topology::face::p1::shrink_geometry;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::Brep;
use solidcore::Kernel;

fn key_of(entity: EntityRef) -> ObjectRef {
    let (type_code, index) = match entity {
        EntityRef::Face(f) => (1, f.index()),
        EntityRef::Edge(e) => (2, e.index()),
        EntityRef::Edgeuse(e) => (3, e.index()),
        EntityRef::Vertex(v) => (4, v.index()),
        EntityRef::Vertexuse(v) => (5, v.index()),
        EntityRef::Loop(l) => (6, l.index()),
        EntityRef::Loopuse(l) => (7, l.index()),
        EntityRef::Faceuse(f) => (8, f.index()),
        EntityRef::Shell(s) => (9, s.index()),
        EntityRef::Region(r) => (10, r.index()),
        EntityRef::Brep(_) => (11, 0),
    };
    ObjectRef {
        type_code: 0,
        key: ObjectKey(type_code * 1_000_000 + u64::from(index)),
    }
}

/// The B-spline surface under `s` (`SmBSplineSurface::CreateIsoParametricCurve` is
/// called on it in the fork's case).
fn root(s: &Surface) -> &BSplineSurface {
    match s.kind() {
        SurfaceKind::BSpline(b) => b,
        SurfaceKind::Cone(c) => c.rev().base(),
        SurfaceKind::Torus(t) => t.base().base(),
        SurfaceKind::Plane(p) => p.base(),
        _ => panic!("kind not used here"),
    }
}

/// How many of the face's 8 trim curves the shrink kept as given, how many it removed,
/// and how far the kept ones lie off their edges.
struct Shrunk {
    kept: usize,
    removed: usize,
    off: f64,
    /// The trimmed surface's spans along u.
    spans_u: usize,
}

fn shrunk_face_trims(k: &mut Kernel, srf: Surface, d: &Extent2d) -> Shrunk {
    let mut b = Brep::new_with_infinite_region().unwrap();
    b.set_tolerance(1e-6);
    let (u0, u1, v0, v1) = (d.min().x, d.max().x, d.min().y, d.max().y);
    // counter-clockwise: v = v0 (u up), u = u1 (v up), v = v1 (u down), u = u0 (v down)
    let iso = [
        SurfParamType::V,
        SurfParamType::U,
        SurfParamType::V,
        SurfParamType::U,
    ];
    let at = [v0, u1, v1, u0];
    let from = [(u0, v0), (u1, v0), (u0, v1), (u0, v0)];
    let to = [(u1, v0), (u1, v1), (u1, v1), (u0, v1)];
    let mut curves = Vec::new();
    let mut uvs = Vec::new();
    let mut orients = Vec::new();
    for i in 0..4 {
        let c = root(&srf)
            .create_iso_parametric_curve(
                k,
                iso[i],
                at[i],
                IsoCurveOptions {
                    domain: Some(*d),
                    uv_iso_curve: false,
                },
            )
            .unwrap()
            .curve;
        let ivl = c.natural_interval();
        let cps = [
            Point3::new(from[i].0, from[i].1, 0.0),
            Point3::new(to[i].0, to[i].1, 0.0),
        ];
        let uv = BSplineCurve::create_canonical(&CanonicalData {
            dim: 2,
            degree: 1,
            control_points: &cps,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[2, 2],
            knots: &[ivl.min(), ivl.max()],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap();
        curves.push(Some(Curve::from(c)));
        uvs.push(Curve::from(uv));
        orients.push(if i < 2 {
            OrientType::Same
        } else {
            OrientType::Opposite
        });
    }
    let natural = srf.natural_uv_domain_with_nurb(k).unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let region = b.infinite_region();
    let made = make_face_with_curves(
        k,
        &mut b,
        region,
        FaceCurvesInput {
            loops: vec![4],
            curves_3d: Some(curves),
            uv_curves: Some(uvs),
            orientations: orients,
            loop_points: Vec::new(),
        },
        srf,
        &natural,
        OrientType::Same,
        Some(true),
        &|f| key_of(EntityRef::Face(f)),
        &mut env,
    )
    .unwrap();
    let face = made.face.unwrap();
    let eus: Vec<_> = b.edgeuses().ids().collect();
    assert_eq!(eus.len(), 8);
    let given: Vec<Option<Curve>> = eus
        .iter()
        .map(|&eu| uv_trim_curve(&b, eu).unwrap().0.cloned())
        .collect();
    assert!(
        given.iter().all(Option::is_some),
        "the face keeps the given trims"
    );

    shrink_geometry(k, &mut b, face, &key_of).unwrap();

    let surface = b.face(face).unwrap().surface.clone().unwrap();
    let mut out = Shrunk {
        kept: 0,
        removed: 0,
        off: 0.0,
        spans_u: surface.knots(SurfParamType::U, None).unwrap().0.len() - 1,
    };
    for (i, &eu) in eus.iter().enumerate() {
        let r = b.edgeuse(eu).unwrap();
        let Some(now) = uv_trim_curve(&b, eu).unwrap().0 else {
            out.removed += 1;
            continue;
        };
        let was = given[i].as_ref().unwrap();
        let e = b.edge(r.edge.unwrap()).unwrap();
        let curve = e.curve.as_ref().unwrap();
        let mut same = now.natural_interval() == was.natural_interval();
        for j in 0..=32 {
            let t = e.interval.evaluate(f64::from(j) / 32.0);
            let c = curve.evaluate_point(k, t).unwrap();
            let w = now.evaluate_point(k, t).unwrap();
            same &= w == was.evaluate_point(k, t).unwrap();
            let q = surface.evaluate_point(k, Point2::new(w.x, w.y)).unwrap();
            out.off = out.off.max(c.distance(q));
        }
        if same {
            out.kept += 1;
        }
    }
    out
}

fn kernel(on: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.face_trim_curves_kept_where_exact = on;
    k
}

/// A bicubic B-spline of two spans each way, a hill (the fork's `shrink_bspline_patch`).
fn bspline_patch() -> Surface {
    let mut net = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let z = if i == 2 && j == 2 {
                4.0
            } else {
                0.3 * f64::from(i) * f64::from(j)
            };
            net.push(Point3::new(2.5 * f64::from(i), 2.5 * f64::from(j), z));
        }
    }
    let mult = [4, 1, 4];
    let unique = [0.0, 0.5, 1.0];
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 3,
            control_points: &net,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &mult,
            v_multiplicities: &mult,
            u_knots: &unique,
            v_knots: &unique,
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// A quarter cylinder r 2, height 10, its generator a line (the fork's cylinder cases).
fn quarter_cylinder(k: &Kernel) -> Surface {
    let mut c = Cone::new_cylinder_bounded(
        Point3::ZERO,
        Vec3::X,
        Vec3::Y,
        2.0,
        0.0,
        90.0,
        10.0,
        false,
        false,
        false,
    );
    c.make_nurb(k).unwrap();
    Surface::from(c)
}

fn sub(n: &Extent2d, u: (f64, f64), v: (f64, f64)) -> Extent2d {
    let (du, dv) = (n.max().x - n.min().x, n.max().y - n.min().y);
    Extent2d::from_points(
        Point2::new(n.min().x + u.0 * du, n.min().y + v.0 * dv),
        Point2::new(n.min().x + u.1 * du, n.min().y + v.1 * dv),
    )
}

#[test]
fn bspline_patch_keeps_its_trims() {
    let d = Extent2d::from_points(Point2::new(0.2, 0.3), Point2::new(0.7, 0.9));
    let on = shrunk_face_trims(&mut kernel(true), bspline_patch(), &d);
    assert_eq!((on.kept, on.removed), (8, 0));
    assert!(on.off <= 1e-12, "off {}", on.off);
    let off = shrunk_face_trims(&mut kernel(false), bspline_patch(), &d);
    assert_eq!(off.removed, 8, "upstream removes every trim curve");
}

#[test]
fn cylinder_trimmed_along_its_axis_keeps_its_trims() {
    let mut k = kernel(true);
    let s = quarter_cylinder(&k);
    let n = s.natural_uv_domain_with_nurb(&k).unwrap();
    let d = sub(&n, (0.0, 1.0), (0.2, 0.7));
    let on = shrunk_face_trims(&mut k, s, &d);
    assert_eq!((on.kept, on.removed), (8, 0));
    assert!(on.off <= 1e-12, "off {}", on.off);

    let mut k = kernel(false);
    let s = quarter_cylinder(&k);
    assert_eq!(shrunk_face_trims(&mut k, s, &d).removed, 8);
}

#[test]
fn reparameterizing_trims_remove_the_trim_curves() {
    // the cylinder trimmed in its angle: its arc is rebuilt over the smaller angle
    let mut k = kernel(true);
    let s = quarter_cylinder(&k);
    let n = s.natural_uv_domain_with_nurb(&k).unwrap();
    let d = sub(&n, (0.2, 0.7), (0.0, 1.0));
    assert_eq!(shrunk_face_trims(&mut k, s, &d).removed, 8);

    // a torus R 5, r 1, trimmed along its generator (a circle)
    let origin = Axis2Placement::new(Point3::ZERO, Vec3::X, Vec3::Y);
    let t = Torus::create_canonical(&k, &origin, 5.0, 1.0, TorusOptions::default(), None).unwrap();
    let s = Surface::from(t);
    let n = s.natural_uv_domain_with_nurb(&k).unwrap();
    let d = sub(&n, (0.0, 1.0), (0.1, 0.4));
    assert_eq!(shrunk_face_trims(&mut k, s, &d).removed, 8);

    // a plane: its trim adjusts its STEP domain
    let s = Surface::from(Plane::create_canonical(&origin).unwrap());
    let n = s.natural_uv_domain_with_nurb(&k).unwrap();
    let d = sub(&n, (0.2, 0.6), (0.3, 0.8));
    assert_eq!(shrunk_face_trims(&mut k, s, &d).removed, 8);
}

#[test]
fn bound_a_rounding_step_past_a_knot_ends_on_the_knot() {
    // the hill's patch whose bound in u sits one rounding step past its knot at 0.5 (the
    // fork's `shrink_bound_a_rounding_step_past_a_knot`): with WR-702 the bound is 0.5 and
    // the patch has the one span [0.2, 0.5] along u
    let d = Extent2d::from_points(Point2::new(0.2, 0.3), Point2::new(0.5_f64.next_up(), 0.9));
    let mut k = kernel(true);
    k.fixes.face_shrink_bound_on_knot = true;
    let on = shrunk_face_trims(&mut k, bspline_patch(), &d);
    assert_eq!((on.kept, on.removed, on.spans_u), (8, 0, 1));
    assert!(on.off <= 1e-12, "off {}", on.off);

    // without it the surface keeps a sliver span past 0.5
    let off = shrunk_face_trims(&mut kernel(true), bspline_patch(), &d);
    assert_eq!(off.spans_u, 2, "upstream keeps the sliver span");
}
