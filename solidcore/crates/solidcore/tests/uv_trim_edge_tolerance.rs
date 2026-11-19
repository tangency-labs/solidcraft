//! #18162: a lazily built UV trim raises its edge's tolerance by the trim's proved distance
//! from the edge curve, not by the drop's reported error, which counts the fit tolerance (the
//! edge's own tolerance) and so nearly doubled the tolerance on every refit.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::geom::attribute::AttributeStore;
use solidcore::math::Point3;
use solidcore::query::curve_surface_gap::trim_image_gap_bound;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, edgeuse};
use solidcore::{Brep, EdgeId, Kernel};

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
