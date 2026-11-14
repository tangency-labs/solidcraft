//! tangency-labs/solidcore#2975: the stitch reports, for every gap it closes, a
//! certified bound on the gap and the tolerance growth its closure applied; an exact
//! match's closure applies no growth.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::math::{Point2, Vec3};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p5::{stitch_and_orient_reported, StitchAndOrientOptions};
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::topology_traverser::Marks;
use solidcore::{Brep, Kernel};

/// Compare stored tolerance accounting independently of the reported bound.
fn check_accounting(entry: &solidcore::heal::stitch::p2::ClosedGap) {
    let increase = (entry.tolerance_after - entry.tolerance_before).max(0.0);
    assert!(
        entry.growth.is_finite() && entry.growth >= increase,
        "{entry:?}"
    );
    if entry.tolerance_after <= entry.tolerance_before {
        assert_eq!(entry.growth, 0.0);
    }
}

/// Adds one square planar face on the plane `z = 0` over the 1 x 1 domain whose low
/// corner is `x` on x and `y` on y.
fn add_square(k: &mut Kernel, b: &mut Brep, x: f64, y: f64) {
    let surface = Surface::from(Plane::from_normal(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ));
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let domain = Extent2d::from_points(Point2::new(x, y), Point2::new(x + 1.0, y + 1.0));
    create_face_from_surface(k, b, Some(surface), &domain, &mut fenv).unwrap();
}

#[test]
fn a_known_gap_reports_its_bound_and_the_growth_its_closure_applied() {
    let gap = 1e-5;
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_square(&mut k, &mut b, 0.0, 0.0);
    add_square(&mut k, &mut b, 1.0 + gap, 0.0);

    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("the stitch runs");
    for entry in &gaps {
        check_accounting(entry);
    }
    // The two corners glue: each reports the gap it held (the known offset the two
    // squares put between them) and the growth the glue applied — the survivor's
    // tolerance rose by the same amount, the glue's rule keeping the closed distance
    // inside the surviving tolerance (solidcore#2975).
    let corners: Vec<_> = gaps
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                solidcore::heal::stitch::p2::ClosedGapKind::Vertex { .. }
            )
        })
        .collect();
    assert_eq!(corners.len(), 2, "{gaps:?}");
    for entry in corners {
        assert!(entry.gap >= (1.0 + gap) - 1.0, "{entry:?}");
        assert!(
            (entry.gap - gap).abs() <= 1e-9 * gap.max(1.0),
            "reported {entry:?} against the known {gap:.1e}"
        );
        assert!(
            (entry.growth - gap).abs() <= 1e-9 * gap.max(1.0),
            "growth {entry:?} against the known {gap:.1e}"
        );
    }
    // Gluing the vertices does not move the two underlying edge curves. Their
    // whole-curve gap is still the original horizontal separation.
    let shared = gaps
        .iter()
        .find(|entry| {
            matches!(
                entry.kind,
                solidcore::heal::stitch::p2::ClosedGapKind::Edge { .. }
            )
        })
        .expect("the shared edge closure");
    assert!(
        shared.gap >= (1.0 + gap) - 1.0 && shared.gap < gap + 1e-12,
        "{shared:?}"
    );
}

#[test]
fn an_exact_match_reports_no_growth() {
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_square(&mut k, &mut b, 1.0, 0.0);
    add_square(&mut k, &mut b, 0.0, 0.0);

    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("the stitch runs");
    assert_eq!(
        gaps.len(),
        3,
        "two endpoint glues and their shared edge: {gaps:?}"
    );
    for gap in gaps {
        // Independently enclosing the two analytic line evaluations adds a
        // conservative rounding floor, even though the geometric boundary agrees.
        assert!(
            gap.gap.is_finite() && (0.0..=1e-12).contains(&gap.gap),
            "{gap:?}"
        );
        check_accounting(&gap);
        assert!(
            gap.growth == 0.0,
            "the exact match's closure applied no growth: {gap:?}"
        );
    }
}

#[test]
fn an_interior_vertex_split_reports_the_original_curve_gap() {
    use solidcore::heal::stitch::p2::ClosedGapKind;
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let gap = 1e-5;
    add_square(&mut k, &mut b, 0.0, 0.0);
    add_square(&mut k, &mut b, 0.5, 1.0 + gap);
    let records =
        stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let splits: Vec<_> = records
        .iter()
        .filter(|r| matches!(r.kind, ClosedGapKind::Split { .. }))
        .collect();
    assert_eq!(
        splits.len(),
        2,
        "both ends of the partial shared boundary: {records:?}"
    );
    for entry in splits {
        assert!(
            entry.gap >= (1.0 + gap) - 1.0 && entry.gap < gap + 1e-12,
            "{entry:?}"
        );
        check_accounting(entry);
    }
}

/// Two adjacent squares, with one shared edge represented by an exact cubic
/// Hermite line. Stitching can compare it; this certificate does not support it.
fn unsupported_shared_edge() -> (Kernel, Brep) {
    use solidcore::curve::hermite_curve::HermiteCurve;
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_square(&mut k, &mut b, 0.0, 0.0);
    add_square(&mut k, &mut b, 1.0, 0.0);
    b.refresh_cached_topology();
    let ends: Vec<_> = b
        .edge_list()
        .iter()
        .copied()
        .map(|e| {
            let data = b.edge(e).unwrap();
            let c = data.curve.as_ref().unwrap();
            let a = c.evaluate_point(&k, data.interval.min()).unwrap();
            let z = c.evaluate_point(&k, data.interval.max()).unwrap();
            (e, a, z)
        })
        .collect();
    let edge = ends
        .iter()
        .find(|(e, a, z)| {
            ends.iter()
                .any(|(other, p, q)| e != other && ((a == p && z == q) || (a == q && z == p)))
        })
        .expect("the exactly shared boundary")
        .0;
    let data = b.edge(edge).unwrap();
    let old = data.curve.as_ref().unwrap();
    let a = old.evaluate_point(&k, data.interval.min()).unwrap();
    let z = old.evaluate_point(&k, data.interval.max()).unwrap();
    let mut curve: solidcore::curve::curve::Curve = HermiteCurve::new(a, z - a, z, z - a, 3).into();
    curve.set_owner(
        old.owner(),
        Some(solidcore::topology::brep::p2::brep_ref(&b)),
    );
    b.edge_mut(edge).unwrap().curve = Some(curve);
    b.edge_mut(edge).unwrap().interval =
        solidcore::geom::extent1d::Extent1d::from_min_max(0.0, 1.0);
    (k, b)
}

/// A deterministic binary record checks topology, geometry and stored tolerances;
/// explicit id-list assertions below also cover the original arena handles.
fn binary(k: &Kernel, b: &Brep) -> Vec<u8> {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::topology::brep_data_write::FromBrepEnv;
    use solidcore::topology::topology::NotifyRecords;
    let attributes = AttributeStore::new();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &attributes,
        records: &records,
        key_of: &solidcore::topology::loopuse::notify_ref,
        curve_key: &|_| ObjectKey(0),
        uv_curve_key: &|_| ObjectKey(0),
        surface_key: &|_| ObjectKey(0),
    };
    solidcore::topology::api_files::write_brep_to_file(k, Some(b), &env, false).unwrap()
}

#[test]
fn certificate_failure_propagates_and_rolls_back_earlier_closures_with_flags_off_or_on() {
    use solidcore::heal::stitch::p1::{DefaultStitchCallback, Stitch};
    use solidcore::heal::stitch::p2::{ClosedGapKind, MinGapRequest};
    use solidcore::SolidError;

    // The unwrapped stitch demonstrates that failure really occurs AFTER the two
    // vertex glues. This is not merely a no-edit input-validation test.
    let (mut k, mut b) = unsupported_shared_edge();
    let mut callback = DefaultStitchCallback::new();
    let mut stitch = Stitch::new(&mut callback, b.tolerance() * 4.0, true, true);
    stitch.fast_edge_compare = true;
    stitch.report_gaps = Some(Vec::new());
    let failure = stitch
        .do_stitching(&mut k, &mut b, None, None, MinGapRequest::default())
        .unwrap_err();
    assert!(
        matches!(
            failure,
            SolidError::Unsupported {
                operation: "heal::stitch::bounds",
                ..
            }
        ),
        "{failure:?}"
    );
    let records = stitch.report_gaps.as_ref().unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r.kind, ClosedGapKind::Vertex { .. }))
            .count(),
        2,
        "{records:?}"
    );
    b.refresh_cached_topology();
    assert_eq!(
        b.vertex_list().len(),
        6,
        "the direct stitch has already edited the operand"
    );

    for statuses in [false, true] {
        let (mut k, mut b) = unsupported_shared_edge();
        k.fixes.brep_edit_statuses_pass_on = statuses;
        let before = binary(&k, &b);
        let ids = (
            b.face_list().to_vec(),
            b.edge_list().to_vec(),
            b.vertex_list().to_vec(),
        );
        let result = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default());
        assert!(
            matches!(
                result,
                Err(SolidError::Unsupported {
                    operation: "heal::stitch::bounds",
                    ..
                })
            ),
            "flag={statuses}: {result:?}"
        );
        assert_eq!(
            binary(&k, &b),
            before,
            "flag={statuses}: the full operand must be restored"
        );
        assert_eq!(
            (
                b.face_list().to_vec(),
                b.edge_list().to_vec(),
                b.vertex_list().to_vec()
            ),
            ids
        );
    }
}

/// The edge closures of a report, with their kept and deleted edges.
fn edge_closures(
    gaps: &[solidcore::heal::stitch::p2::ClosedGap],
) -> Vec<(
    solidcore::heal::stitch::p2::ClosedGap,
    solidcore::topology::EdgeId,
    solidcore::topology::EdgeId,
)> {
    gaps.iter()
        .filter_map(|entry| match entry.kind {
            solidcore::heal::stitch::p2::ClosedGapKind::Edge { kept, deleted } => {
                Some((*entry, kept, deleted))
            }
            _ => None,
        })
        .collect()
}

/// The largest distance between two curves at the same parameter, over 1001
/// stations of `[t0, t1]`, in floating point. It only checks that a case is built
/// with the gap it means to have: its evaluation rounds by about 1e-16 of the
/// curves' size, so it is no lower bound at that level, and no certificate.
fn sampled_distance(
    k: &Kernel,
    a: &solidcore::curve::curve::Curve,
    b: &solidcore::curve::curve::Curve,
    t0: f64,
    t1: f64,
) -> f64 {
    (0..=1000)
        .map(|i| {
            let t = t0 + (t1 - t0) * f64::from(i) / 1000.0;
            (a.evaluate_point(k, t).unwrap() - b.evaluate_point(k, t).unwrap()).length()
        })
        .fold(0.0, f64::max)
}

/// Adds a half-cylinder face of radius 2 about z, from height `z` to `z + 1`. Its
/// edges at the two heights are circles (`SmCircle`) over the same parameters.
fn add_half_cylinder(k: &mut Kernel, b: &mut Brep, z: f64) {
    use solidcore::surface::cone::Cone;
    let cylinder = Cone::new_cylinder_bounded(
        solidcore::math::Point3::new(0.0, 0.0, z),
        Vec3::X,
        Vec3::Y,
        2.0,
        0.0,
        180.0,
        1.0,
        false,
        false,
        true,
    );
    let surface = Surface::from(cylinder);
    let domain = surface.natural_uv_domain_with_nurb(k).unwrap();
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, b, Some(surface), &domain, &mut fenv).unwrap();
}

/// Checks one stitch's single edge closure: the shared edge is of `kind`, the bound
/// holds the curves' distance at every station and lies within `slack` of the
/// closed-form `gap`.
fn check_edge_closure(
    b: &Brep,
    gaps: &[solidcore::heal::stitch::p2::ClosedGap],
    kind: &str,
    gap: f64,
    slack: f64,
) {
    for entry in gaps {
        check_accounting(entry);
    }
    let edges = edge_closures(gaps);
    assert_eq!(edges.len(), 1, "{gaps:?}");
    let (entry, kept, _) = edges[0];
    let data = b.edge(kept).unwrap();
    let curve = data.curve.as_ref().unwrap();
    assert_eq!(curve.type_string(), kind);
    assert!(
        entry.gap >= gap - slack && entry.gap <= gap + slack,
        "{kind}: reported {entry:?} against the closed-form {gap:e}"
    );
}

#[test]
fn stacked_cylinder_faces_report_the_circle_gap() {
    let gap = 1e-6;
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_half_cylinder(&mut k, &mut b, 0.0);
    add_half_cylinder(&mut k, &mut b, 1.0 + gap);
    b.refresh_cached_topology();
    // The two circles at the shared height, before the glue moves anything.
    let circles: Vec<_> = b
        .edge_list()
        .iter()
        .filter_map(|&e| {
            let data = b.edge(e).unwrap();
            let c = data.curve.as_ref().unwrap();
            let z = c.evaluate_point(&k, data.interval.min()).unwrap().z;
            (c.type_string() == "SmCircle_TYPE" && z > 0.5 && z < 1.5)
                .then(|| (c.clone(), data.interval))
        })
        .collect();
    assert_eq!(circles.len(), 2);
    let witness = sampled_distance(
        &k,
        &circles[0].0,
        &circles[1].0,
        circles[0].1.min(),
        circles[0].1.max(),
    );
    // Closed form: the circles differ by the translation between the heights
    // as stored, (1 + gap) - 1, exact in floating point (Sterbenz).
    let exact = (1.0 + gap) - 1.0;
    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("circles have a certified enclosure");
    let edges = edge_closures(&gaps);
    assert_eq!(edges.len(), 1, "{gaps:?}");
    let entry = edges[0].0;
    assert!((witness - exact).abs() <= 1e-14, "{witness:e}");
    // Both nets hold the same x and y and the stored heights: the exact gap.
    assert!(
        entry.gap >= exact && entry.gap - exact <= 1e-15,
        "{entry:?} against the closed-form {exact:e}"
    );
    check_edge_closure(&b, &gaps, "SmCircle_TYPE", exact, 1e-15);
}

#[test]
fn stacked_cylinder_faces_that_match_exactly_report_no_growth() {
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_half_cylinder(&mut k, &mut b, 0.0);
    add_half_cylinder(&mut k, &mut b, 1.0);
    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("circles have a certified enclosure");
    assert_eq!(edge_closures(&gaps).len(), 1, "{gaps:?}");
    for entry in &gaps {
        check_accounting(entry);
        assert_eq!(entry.growth, 0.0, "{entry:?}");
    }
    let (entry, _, _) = edge_closures(&gaps)[0];
    // The same control net at the same parameters: the bound is exactly zero.
    assert_eq!(entry.gap, 0.0, "{entry:?}");
}

/// Adds the planar faces bounded by the closed loops of `curves` on `z = 0`.
fn add_planar(k: &mut Kernel, b: &mut Brep, curves: &[solidcore::curve::curve::Curve]) {
    let region = b.infinite_region();
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let faces = solidcore::topology::brep::p8::create_planar_faces_with_3d_curves(
        k, b, region, curves, 0.01, &mut fenv,
    )
    .unwrap();
    assert_eq!(faces.len(), 1);
}

/// Two planar faces on `z = 0` meeting along an open conic arc: the first bounded
/// by `make(0)` and its chord, the second by `make(-gap)` (the same arc moved by
/// `gap` along -x) and a box to its left. Returns the stitch's report, the B-rep
/// and the stations' witness distance between the two arcs.
fn split_by_arc(
    make: impl Fn(&Kernel, f64) -> solidcore::curve::curve::Curve,
    gap: f64,
) -> (Vec<solidcore::heal::stitch::p2::ClosedGap>, Brep, f64) {
    use solidcore::curve::line::Line;
    use solidcore::math::Point3;
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let first = make(&k, 0.0);
    let second = make(&k, -gap);
    let t = first.natural_interval();
    let witness = sampled_distance(&k, &first, &second, t.min(), t.max());
    let p0 = first.evaluate_point(&k, t.min()).unwrap();
    let p1 = first.evaluate_point(&k, t.max()).unwrap();
    let q0 = second.evaluate_point(&k, t.min()).unwrap();
    let q1 = second.evaluate_point(&k, t.max()).unwrap();
    add_planar(
        &mut k,
        &mut b,
        &[first, Line::from_points(p1, p0, 3).into()],
    );
    let left = -10.0;
    let a = Point3::new(left, q1.y, 0.0);
    let c = Point3::new(left, q0.y, 0.0);
    add_planar(
        &mut k,
        &mut b,
        &[
            second,
            Line::from_points(q1, a, 3).into(),
            Line::from_points(a, c, 3).into(),
            Line::from_points(c, q0, 3).into(),
        ],
    );
    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("the arc has a certified enclosure");
    (gaps, b, witness)
}

/// The closed-form gap is the translation `gap` between the two definitions. Each
/// stored net rounds its control points by a few ulps of the arc's size (about 3,
/// so under 1e-15 each), so the stored arcs' gap is within 1e-14 of `gap`; the
/// bound must hold it and stay within 1e-12 of it.
fn check_arc_split(make: impl Fn(&Kernel, f64) -> solidcore::curve::curve::Curve, kind: &str) {
    let gap = 1e-6;
    let (gaps, b, witness) = split_by_arc(make, gap);
    assert!(
        (witness - gap).abs() <= 1e-14,
        "{kind}: the stations' distance {witness:e} is the translation"
    );
    let (entry, _, _) = edge_closures(&gaps)[0];
    assert!(entry.gap >= gap - 1e-14, "{kind}: {entry:?}");
    check_edge_closure(&b, &gaps, kind, gap, 1e-12);
}

#[test]
fn faces_split_by_a_hyperbola_report_its_gap() {
    use solidcore::curve::hyperbola::Hyperbola;
    use solidcore::geom::extent1d::Extent1d;
    check_arc_split(
        |_, dx| {
            Hyperbola::new(
                solidcore::math::Point3::new(dx, 0.0, 0.0),
                Vec3::X,
                Vec3::Y,
                Extent1d::from_min_max(-1.0, 1.0),
                2.0,
                1.0,
                3,
            )
            .into()
        },
        "SmHyperbola_TYPE",
    );
}

#[test]
fn faces_split_by_a_parabola_report_its_gap() {
    use solidcore::curve::parabola::Parabola;
    use solidcore::geom::extent1d::Extent1d;
    check_arc_split(
        |_, dx| {
            Parabola::new(
                solidcore::math::Point3::new(dx, 0.0, 0.0),
                Vec3::X,
                Vec3::Y,
                Extent1d::from_min_max(-1.0, 1.0),
                0.5,
                3,
            )
            .into()
        },
        "SmParabola_TYPE",
    );
}

#[test]
fn faces_split_by_an_ellipse_report_its_gap() {
    use solidcore::curve::ellipse::Ellipse;
    use solidcore::geom::extent1d::Extent1d;
    check_arc_split(
        |k, dx| {
            Ellipse::new(
                k,
                solidcore::math::Point3::new(dx, 0.0, 0.0),
                Vec3::X,
                Vec3::Y,
                Extent1d::from_min_max(-60.0, 60.0),
                2.0,
                1.0,
                &solidcore::curve::ellipse::EllipseOptions::default(),
            )
            .unwrap()
            .into()
        },
        "SmEllipse_TYPE",
    );
}

#[test]
fn a_planar_conic_disc_in_a_holed_face_reports_its_gap() {
    use solidcore::curve::conic::{Conic, ConicType};
    use solidcore::curve::curve::Curve;
    use solidcore::curve::line::Line;
    use solidcore::math::{Point2, Point3};
    let gap = 1e-6;
    let conic = |dx: f64| -> Curve {
        let mut c = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        c.transform_2d(0.0, Point2::new(dx, 0.0));
        c.into()
    };
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let (first, second) = (conic(0.0), conic(-gap));
    let t = first.natural_interval();
    let witness = sampled_distance(&k, &first, &second, t.min(), t.max());
    assert!((witness - gap).abs() <= 1e-14, "{witness:e}");
    add_planar(&mut k, &mut b, &[first]);
    let r = |x: f64, y: f64| Point3::new(x, y, 0.0);
    add_planar(
        &mut k,
        &mut b,
        &[
            second,
            Line::from_points(r(-5.0, -5.0), r(5.0, -5.0), 3).into(),
            Line::from_points(r(5.0, -5.0), r(5.0, 5.0), 3).into(),
            Line::from_points(r(5.0, 5.0), r(-5.0, 5.0), 3).into(),
            Line::from_points(r(-5.0, 5.0), r(-5.0, -5.0), 3).into(),
        ],
    );
    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("the planar conic has a certified enclosure");
    let (entry, _, _) = edge_closures(&gaps)[0];
    // The conic's own numbers: a, b and theta equal, origins 0 and -gap, so the
    // curves differ by exactly (gap, 0) at every parameter.
    assert!(entry.gap >= gap, "{entry:?}");
    check_edge_closure(&b, &gaps, "SmConic_TYPE", gap, 1e-12);
}

/// Two adjacent sheets whose shared curves are lifted `gap` off their planes, stitched with
/// `stitch_tol`. The edges carry the zone `edge_zone`, the faces 0.002 and the vertices 0.006.
/// Returns the body and the number of edges the stitch sewed.
fn lifted_sheets(k: &mut Kernel, gap: f64, stitch_tol: f64, edge_zone: f64) -> (Brep, usize) {
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::heal::gap::{EdgeEdgeGap, EdgeFaceGap, VertexEdgeGap, VertexFaceGap};
    use solidcore::topology::brep::p4::stitch_faces;

    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_square(k, &mut b, 0.0, 0.0);
    add_square(k, &mut b, 1.0, 0.0);
    b.set_tolerance(1e-5);
    for f in b.face_list().to_vec() {
        b.face_mut(f).unwrap().tolerance = 0.002;
    }
    for v in b.vertex_list().to_vec() {
        b.vertex_mut(v).unwrap().tolerance = 0.006;
    }
    let translation = Axis2Placement::new(
        Vec3::new(0.0, 0.0, gap),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    // add_square's domain is in the plane's frame, which need not align
    // its u coordinate with world x. Select the shared u = 1 edge in that frame.
    let surface = b.face(b.face_list()[0]).unwrap().surface.as_ref().unwrap();
    let shared_start = surface.evaluate_point(k, Point2::new(1.0, 0.0)).unwrap();
    let shared_end = surface.evaluate_point(k, Point2::new(1.0, 1.0)).unwrap();
    let mut lifted = 0;
    for e in b.edge_list().to_vec() {
        let data = b.edge_mut(e).unwrap();
        data.tolerance = edge_zone;
        let curve = data.curve.as_mut().unwrap();
        let a = curve.evaluate_point(k, data.interval.min()).unwrap();
        let z = curve.evaluate_point(k, data.interval.max()).unwrap();
        if (a.distance(shared_start) < 1e-12 && z.distance(shared_end) < 1e-12)
            || (a.distance(shared_end) < 1e-12 && z.distance(shared_start) < 1e-12)
        {
            curve.transform(k, &translation, None).unwrap();
            lifted += 1;
        }
    }
    assert_eq!(lifted, 2);
    // The fixture edits the stored geometry directly, so clear any gaps the
    // face constructor measured before that edit.
    for eu in b.edgeuses().iter().map(|(id, _)| id).collect::<Vec<_>>() {
        let data = b.edgeuse_mut(eu).unwrap();
        data.max_edge_face_gap = EdgeFaceGap::default();
        data.edge_ccw_edge_gap = EdgeEdgeGap::default();
    }
    for vu in b.vertexuses().iter().map(|(id, _)| id).collect::<Vec<_>>() {
        let data = b.vertexuse_mut(vu).unwrap();
        data.vertex_edge_gap = VertexEdgeGap::default();
        data.vertex_face_gap = VertexFaceGap::default();
    }
    let sewed = stitch_faces(k, &mut b, stitch_tol, None, false)
        .map_or(0, |outcome| outcome.stitched_edges);
    b.refresh_cached_topology();
    // `add_square` names each surface's owner by its arena identity, while `new_brep`
    // checks under the notification scheme: relabel so the check sees the owners.
    b.use_arena_naming().unwrap();
    if sewed == 1 {
        assert_eq!(b.edge_list().len(), 7);
        assert_eq!(b.vertex_list().len(), 6);
    }
    (b, usize::try_from(sewed).unwrap_or(usize::MAX))
}

/// A stitch tolerance of 0.01 sews gaps up to it, growing the sewn edge's zone to cover the gap
/// (at most the gap plus the larger zone), and the body is then valid. A gap past the stitch
/// tolerance and past the summed edge zones is not sewn, and the check reports it.
#[test]
fn a_stitch_sews_a_gap_within_its_tolerance_and_leaves_a_larger_one() {
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};

    let mut k = Kernel::default();
    for gap in [0.004, 0.008] {
        let (mut b, sewed) = lifted_sheets(&mut k, gap, 0.01, 0.005);
        assert_eq!(sewed, 1, "gap {gap}");
        let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(report.is_valid(), "gap {gap}: {:?}", report.lines());
        let grown = b
            .edge_list()
            .iter()
            .map(|&e| b.edge(e).unwrap().tolerance)
            .fold(0.0, f64::max);
        assert!(grown >= gap, "gap {gap}: zone {grown}");
        assert!(grown <= gap + 0.005 + 1e-12, "gap {gap}: zone {grown}");
    }
    let (mut b, sewed) = lifted_sheets(&mut k, 0.012, 0.01, 0.005);
    assert_eq!(sewed, 0);
    let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(!report.is_valid());
}

/// A gap inside the sum of the incident local zones (here the 0.02 edge zone) is sewn without
/// growing any zone, the body is valid, and the explicit repair API keeps its stricter targets.
#[test]
fn a_gap_inside_the_summed_local_zones_is_sewn_without_growing_them() {
    use solidcore::geom::assert_array::AssertArray;
    use solidcore::topology::brep::validation::{
        check_brep, validate_and_update_tolerances, CheckLevel,
    };

    let mut k = Kernel::default();
    k.fixes.validate_tolerances_fixes_reported_gaps = true;
    let (mut b, sewed) = lifted_sheets(&mut k, 0.001, 1e-4, 0.02);
    assert_eq!(sewed, 1);
    let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(report.is_valid(), "{:?}", report.lines());
    assert_eq!(b.tolerance(), 1e-5);
    for &e in b.edge_list() {
        assert_eq!(b.edge(e).unwrap().tolerance, 0.02);
    }
    for &v in b.vertex_list() {
        assert_eq!(b.vertex(v).unwrap().tolerance, 0.006);
    }
    // Checking validity must not silently repair the model.
    let mut repair = AssertArray::default();
    let gaps = validate_and_update_tolerances(&mut k, &mut b, true, Some(&mut repair), 2).unwrap();
    assert!((gaps.max_vertex_edge_gap - 0.001).abs() < 1e-12);
}
