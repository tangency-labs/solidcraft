//! #18162: a lazily built UV trim raises its edge's tolerance by the trim's proved distance
//! from the edge curve, not by the drop's reported error, which counts the fit tolerance (the
//! edge's own tolerance) and so nearly doubled the tolerance on every refit.
//!
//! #20940: the step that builds those trims for a whole body, `create_missing_uv_trim_curves`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::curve_surface_gap::trim_image_gap_bound;
use solidcore::topology::brep::create_missing_uv_trim_curves;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::edgeuses;
use solidcore::topology::brep::p5::create_wire_edge_from_curve;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, edgeuse, EdgeuseId};
use solidcore::{Brep, EdgeId, Kernel, Vec3};

fn fillet1019(k: &mut Kernel) -> Brep {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/fillet1019.smb");
    // (an unreadable fixture fails: the test must never pass without running)
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    b
}

/// The largest proved distance between `e`'s curve and the image of any of its UV trims
/// (`trim_image_gap_bound` at a thousandth of the edge's tolerance), with the number of trims.
fn trims_bound(k: &Kernel, b: &Brep, e: EdgeId) -> (f64, usize) {
    let rec = b.edge(e).unwrap();
    let ivl = rec.interval;
    let curve = rec.curve.as_ref().unwrap();
    let floor = 1e-3 * edge::tolerance(b, e).unwrap();
    let mut worst = 0.0_f64;
    let mut n = 0;
    for &eu in &rec.edgeuses {
        let Some(f) = edgeuse::p1::face(b, eu).unwrap() else {
            continue;
        };
        let (Some(trim), _) = edgeuse::p1::uv_trim_curve(b, eu).unwrap() else {
            continue;
        };
        let surface = b.face(f).unwrap().surface.as_ref().unwrap();
        let g = trim_image_gap_bound(k, surface, curve, trim, [ivl.min(), ivl.max()], floor)
            .unwrap()
            .expect("the trim's distance is enclosed");
        worst = worst.max(g);
        n += 1;
    }
    (worst, n)
}

/// The end points of base edge 13 after the defeature (arena id 13 on Linux): from the corner
/// of the two cylinders at the base face's edge, (-24.917, -19.856, 5.523), down to the fillet's
/// end, (-20.337, -25.398, 0.023).
const BASE_EDGE_13: [[f64; 3]; 2] = [
    [-24.916821880874167, -19.8561528145681, 5.522751173158113],
    [-20.337332681118223, -25.39800174742429, 0.022840131454544],
];

/// The one edge of `b` whose two end points are `ends` (in either order) to 1e-4 (the ends lie
/// 9 apart, the edge's tolerance is 5e-4): an edge found by its geometry, not its arena id,
/// which another platform's float path may number otherwise.
fn edge_between(b: &Brep, ends: [[f64; 3]; 2]) -> EdgeId {
    let near = |p: Point3, q: [f64; 3]| (p - Point3::new(q[0], q[1], q[2])).length() <= 1e-4;
    let found: Vec<EdgeId> = bp1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let pts: Vec<Point3> = edge::vertices(b, e, None)
                .unwrap()
                .into_iter()
                .flatten()
                .map(|v| b.vertex(v).unwrap().point)
                .collect();
            let [p, q] = pts[..] else {
                return false;
            };
            (near(p, ends[0]) && near(q, ends[1])) || (near(p, ends[1]) && near(q, ends[0]))
        })
        .collect();
    assert_eq!(found.len(), 1, "edges between {ends:?}: {found:?}");
    found[0]
}

/// fillet1019, feature face 6 defeatured (`single_face_fillet1019_6`): base edge 13 lies on
/// two cylinders, with lazily built trims on both. Each refit raised it from its own
/// tolerance (1.37e-3 before the fix). Now its tolerance covers the proved distance of its
/// trims `B` and is at most twice it: twice the bound at the raise, which is computed at a
/// coarser floor (a thousandth of the tolerance then) and so is up to a few per cent above `B`.
#[test]
fn a_lazy_trim_raises_its_edge_by_its_proved_distance() {
    let mut k = Kernel::default();
    let mut b = fillet1019(&mut k);
    let face = bp1::faces(&b, None).unwrap()[6];
    {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        ex.add_feature(
            &mut k,
            &[face],
            RebuildBrepType::ExtendSurface,
            RebuildBrepType::FromOtherBrep,
        )
        .unwrap()
        .unwrap();
        let kept = ex.do_defeature(&mut k).unwrap();
        assert!(kept.is_empty(), "{kept:?}");
    }
    let e = edge_between(&b, BASE_EDGE_13);
    let tol = edge::tolerance(&b, e).unwrap();
    let (bound, trims) = trims_bound(&k, &b, e);
    assert_eq!(trims, 4, "both edge uses of both faces have trims");
    assert!(
        tol >= bound,
        "tolerance {tol:e} below the proved distance {bound:e}"
    );
    assert!(
        tol <= 2.05 * bound,
        "tolerance {tol:e} past twice the proved distance {bound:e}"
    );
}

/// A 10 cube with an r 2 hole through it along z (planes and a cylinder), then every other
/// UV trim curve removed, then one wire edge added beside it (a straight segment, whose two
/// edge uses have no face use). Returns the body and the wire edge's uses.
fn holed_cube_with_missing_trims_and_a_wire(k: &mut Kernel) -> (Brep, Vec<EdgeuseId>) {
    let cube = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let tool = create_cylinder(k, Point3::new(5.0, 5.0, -1.0), 2.0, 12.0).unwrap();
    let mut b = boolean_difference(k, cube, tool).unwrap();
    let holders: Vec<EdgeuseId> = edgeuses(&b, None)
        .unwrap()
        .into_iter()
        .filter(|&eu| {
            edgeuse::p1::uv_trim_curve_pointer(&b, eu)
                .unwrap()
                .is_some()
        })
        .collect();
    for eu in holders.into_iter().step_by(2) {
        edgeuse::p1::set_uv_trim_curve(
            &mut b,
            eu,
            edgeuse::p1::UvTrimCurveArg::Null,
            0.0,
            true,
            None,
        )
        .unwrap();
    }
    let segment = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &[Vec3::new(20.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 5.0)],
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let wire = create_wire_edge_from_curve(
        k,
        &mut b,
        Curve::from(segment),
        Extent1d::from_min_max(0.0, 1.0),
    )
    .unwrap();
    let wire_uses = b.edge(wire).unwrap().edgeuses.clone();
    (b, wire_uses)
}

/// The edge uses of `b` on a face, split into those with a UV trim curve (their own or their
/// mate's) and those without.
fn face_uses_by_trim(b: &Brep) -> (Vec<EdgeuseId>, Vec<EdgeuseId>) {
    edgeuses(b, None)
        .unwrap()
        .into_iter()
        .filter(|&eu| edgeuse::p1::faceuse(b, eu).unwrap().is_some())
        .partition(|&eu| edgeuse::p1::uv_trim_curve(b, eu).unwrap().0.is_some())
}

/// #20940: `create_missing_uv_trim_curves` on a body where some edge uses on faces have a UV
/// trim curve and some have none, with a wire edge beside them. Afterwards every edge use
/// on a face has one, the ones that had one hold the same curve, the wire edge's uses are as
/// they were, and a second call leaves the whole body as it was.
#[test]
fn every_edge_use_on_a_face_gets_its_missing_uv_trim_curve() {
    let mut k = Kernel::default();
    let (mut b, wire_uses) = holed_cube_with_missing_trims_and_a_wire(&mut k);
    assert_eq!(wire_uses.len(), 2, "a wire edge has two uses");
    for &eu in &wire_uses {
        assert!(edgeuse::p1::faceuse(&b, eu).unwrap().is_none());
    }
    let (with, without) = face_uses_by_trim(&b);
    assert!(!with.is_empty(), "some edge uses keep their trims");
    assert!(!without.is_empty(), "some edge uses have no trim");
    let trim_of = |b: &Brep, eu: EdgeuseId| {
        format!(
            "{:?}",
            edgeuse::p1::uv_trim_curve(b, eu).unwrap().0.unwrap()
        )
    };
    let kept_before: Vec<String> = with.iter().map(|&eu| trim_of(&b, eu)).collect();
    let wire_before: Vec<String> = wire_uses
        .iter()
        .map(|&eu| format!("{:?}", b.edgeuse(eu).unwrap()))
        .collect();

    create_missing_uv_trim_curves(&k, &mut b).unwrap();

    let (with_after, without_after) = face_uses_by_trim(&b);
    assert_eq!(without_after, Vec::<EdgeuseId>::new());
    assert_eq!(with_after.len(), with.len() + without.len());
    let kept_after: Vec<String> = with.iter().map(|&eu| trim_of(&b, eu)).collect();
    assert_eq!(
        kept_after, kept_before,
        "a trim that was there is not rebuilt"
    );
    let wire_after: Vec<String> = wire_uses
        .iter()
        .map(|&eu| format!("{:?}", b.edgeuse(eu).unwrap()))
        .collect();
    assert_eq!(
        wire_after, wire_before,
        "a wire edge's uses are not touched"
    );
    for &eu in &wire_uses {
        assert!(edgeuse::p1::uv_trim_curve_pointer(&b, eu)
            .unwrap()
            .is_none());
    }

    let once = format!("{b:?}");
    create_missing_uv_trim_curves(&k, &mut b).unwrap();
    assert!(format!("{b:?}") == once, "a second call changed the body");
}
