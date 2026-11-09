//! Issue #2867 (nightly fuzz `query.face_p4/fuzz00209`): a cylinder face (radius
//! 1.7697804728715845, UV domain `[0, 2π] × [0, 4.514774976811207]`) trimmed by two
//! one-line UV loops, the first ending at v = 4.5e300. Its 3D trim reaches a coordinate
//! of 4.5e300.
//!
//! Classifying the face's loops (`SetProps_Stage2` then `ClassifyLoops`, as the healer
//! does through `HealLoopHandle`) ran for 48 s: the curve coincidence check along the
//! trim split the curve until its call cap (`MAX_COINCIDENCE_CALLS`), each call on
//! unbounded boxes. Since #2552 the check refuses a curve whose box has a coordinate
//! past `limits::MAX_COORDINATE_MAGNITUDE` (1e150) at its first call, so the refusal
//! comes after one call, whatever the magnitude. Below the bound the same face is
//! classified, and the time does not grow with the end's magnitude (measured in release:
//! stage 2 takes 0.83 s at 1e10 and 0.45 s at 1e140).
//!
//! Without #2552's bound the props stage fails with the call cap's error instead, after
//! 31 s in release; the first test tells the two apart by the error, not by a timing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::TAU;

use solidcore::curve::curve::Curve;
use solidcore::error::{EntityRef, SolidError};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::heal::face_props::{
    ClassifyLoopsOut, ContainmentType, FaceProps, HealerOpType, LoopProps, LoopuseType, PropStage,
};
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::face::p4::classify_loops;
use solidcore::surface::cone::Cone;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1::HealLoopHandle;
use solidcore::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, FaceId};
use solidcore::Kernel;

/// The cylinder's height: the v extent of the face's UV domain.
const HEIGHT: f64 = 4.514774976811207;

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

fn uv_line(a: [f64; 2], b: [f64; 2]) -> Curve {
    Curve::from(
        BSplineCurve::create_line_segment(
            2,
            Point3::new(a[0], a[1], 0.0),
            Point3::new(b[0], b[1], 0.0),
        )
        .unwrap(),
    )
}

/// The fuzz case's face, with its first loop's line ending at v = `end` (in units of
/// the cylinder's height; the fuzz case has 1e300).
fn face(k: &mut Kernel, end: f64) -> (Brep, FaceId) {
    let srf = Surface::from(
        Cone::create_canonical_cylinder(k, &Axis2Placement::default(), 1.7697804728715845).unwrap(),
    );
    let dom = Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(TAU, HEIGHT));
    let uv = vec![
        uv_line([0.0, 0.35403041722463596 * HEIGHT], [TAU, end * HEIGHT]),
        uv_line(
            [TAU, 0.30845071908772786 * HEIGHT],
            [0.0, 0.30845071908772786 * HEIGHT],
        ),
    ];
    let mut b = Brep::new_with_infinite_region().unwrap();
    solidcore::topology::brep::p2::set_tolerance(k, &mut b, 1e-7, false, true).unwrap();
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
            loops: vec![1, 1],
            curves_3d: None,
            uv_curves: Some(uv),
            orientations: vec![OrientType::Same, OrientType::Same],
            loop_points: Vec::new(),
        },
        srf,
        &dom,
        OrientType::Same,
        None,
        &|f| key_of(EntityRef::Face(f)),
        &mut env,
    )
    .unwrap();
    (b, made.face.unwrap())
}

/// `SetProps_Stage3`'s loop props on top of `props` (stage 2 done or failed), as the
/// healer builds them before it calls `ClassifyLoops`.
fn stage3_loop_props(k: &Kernel, b: &Brep, face: FaceId, props: &mut FaceProps<HealLoopHandle>) {
    props.reset(PropStage::Stage3);
    props.prop_stage = PropStage::Stage3A;
    let loops = solidcore::topology::face::p2::loops(b, face, false, None).unwrap();
    let mut loop_props = vec![LoopProps::new(); loops.len()];
    let mut closed = 0;
    for (lp, &l) in loop_props.iter_mut().zip(&loops) {
        lp.set_props(k, b, l, props).unwrap();
        if lp.loopuse_type == LoopuseType::Vertexuse {
            continue;
        }
        if lp.closed_3d.is_true() || lp.closed_ptrs.is_true() {
            closed += 1;
        } else {
            lp.containment = ContainmentType::WireLoop;
        }
    }
    props.loop_props = loop_props;
    props.closed_loop_cnt = closed;
}

/// Stage 2 props of the face with its first line ending at `end`, then its loops
/// classified: stage 2's result and `ClassifyLoops`' result.
fn classify(end: f64) -> (solidcore::Result<()>, solidcore::Result<ClassifyLoopsOut>) {
    let mut k = Kernel::default();
    let (mut b, face) = face(&mut k, end);
    let mut props: FaceProps<HealLoopHandle> = FaceProps::for_face(face, 0);
    let stage2 = props.set_props_stage2_mut(&k, &mut b, face, 0, HealerOpType::All);
    stage3_loop_props(&k, &b, face, &mut props);
    let mut out = ClassifyLoopsOut::default();
    let mut scratch: CurveClassification<HealLoopHandle> = CurveClassification::new(&k);
    let mut loops = Vec::new();
    let classified =
        classify_loops(&k, &b, face, &mut loops, &mut out, &mut scratch, &mut props).map(|()| out);
    (stage2, classified)
}

#[test]
fn a_trim_ending_at_1e300_is_refused_at_the_first_coincidence_check() {
    let (stage2, classified) = classify(1e300);
    match stage2 {
        Err(SolidError::InvalidInput { operation, reason }) => {
            assert_eq!(operation, "curve::curve::p6::global_coincidence_checker");
            // the coordinate bound, checked at the check's first call, and not the call
            // cap (`MAX_COINCIDENCE_CALLS`) the check reached after 48 s before #2552
            assert_eq!(
                reason,
                "coincidence check of a curve with a coordinate beyond 1e150"
            );
        }
        other => panic!("stage 2 props of a 1e300 trim: {other:?}"),
    }
    // the loops are still classified, on the props stage 2 left
    classified.unwrap();
}

#[test]
fn a_trim_ending_below_the_bound_is_classified() {
    let (stage2, classified) = classify(1e140);
    stage2.unwrap();
    let out = classified.unwrap();
    assert_eq!(out.classify_loop_ios.len(), 2, "{out:?}");
}
