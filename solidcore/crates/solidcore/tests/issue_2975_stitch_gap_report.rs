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
