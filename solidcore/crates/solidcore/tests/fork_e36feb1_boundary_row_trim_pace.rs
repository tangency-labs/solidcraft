//! The fork's own regression test for WR-543 (tangency-labs/smlib@e36feb1,
//! tangency-labs/solidcore#1383, capi test `flat_roundtrip` `BoundaryRowTrimPace`),
//! ported. A one-face sheet: a 10 x 10 mm rational bicubic patch with a crease (a triple
//! u knot) at u = 0.08. Edge 0 runs along the patch's v = 0 row, creased with it at
//! u = 0.08, on its own parameter (its control points at s^0.8 of the way along each line
//! for their Greville values s), and starts 0.05 mm past the patch's u = 0 side, on the
//! row's extension. The sheet is given without UV trims (as SolidCraft hands SMLib a
//! face: `data/fork_wr543_boundary_row_sheet.smd`, written by the fork's
//! `smlib_brep_import` of the test's flat arrays, read here without its trims), and
//! `GetOrCreateUVTrimCurve` makes them.
//!
//! At the edge's tolerance the drop does not take the start: it comes back as pieces on
//! the patch, split at the crease. Upstream stretches the longer piece (96% of the edge)
//! onto the whole edge, so the trim runs at the wrong pace, 0.85 mm off the edge, against
//! a tolerance of 5e-5. With `uv_trim_joins_drop_pieces` the pieces that meet are joined
//! on their own parameters: the trim lifts onto the edge within the edge's tolerance
//! (as the fork's test requires), and the edge stays whole.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::topology::brep::p10::{
    make_topology_from_data, MakeTopologyOptions, TopologyEnv, HEALER_VERSION,
};
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use solidcore::topology::face::p2 as face;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, Brep, EdgeId};
use solidcore::{EntityRef, Kernel};

const SHEET: &[u8] = include_bytes!("data/fork_wr543_boundary_row_sheet.smd");

/// SolidCraft's topology identities (type code, entity index).
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(((kind + 1) << 32) | u64::from(i)),
    }
}

/// The sheet without UV trims, then each edge use's trim made, with WR-543's `fixed`.
fn sheet(fixed: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.uv_trim_joins_drop_pieces = fixed;
    let mut data = BrepData::read_from_file(&k, SHEET.to_vec(), FileType::Ascii).unwrap();
    data.set_healer_version(HEALER_VERSION);
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut store = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let next_key = (data.uv_curves.len() + data.curves_3d.len() + data.surfaces.len()) as u64 + 1;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key,
    };
    let options = MakeTopologyOptions {
        make_uv_trim_curves: false,
        add_analytics: false,
        force_same_tol: true,
        check_uv_trim_curves: false,
        always_heal: false,
        healer_enabled: true,
    };
    make_topology_from_data(&mut k, &mut b, &mut data, &mut env, options).unwrap();
    let uses: Vec<_> = b
        .edges()
        .ids()
        .flat_map(|e| edge::edgeuses(&b, e, None, None).unwrap())
        .collect();
    for eu in uses {
        let made = get_or_create_uv_trim_curve(&k, &mut b, eu, true, false, true).unwrap();
        made.status.unwrap();
    }
    (k, b)
}

/// For each edge: its interval, the largest distance (129 samples) between its trim
/// lifted onto the face and the edge's point at the same parameter, and its tolerance.
fn trim_fits(k: &Kernel, b: &Brep) -> Vec<(EdgeId, [f64; 2], f64, f64)> {
    let mut fits = Vec::new();
    for e in b.edges().ids() {
        let curve = edge::curve(b, e).unwrap().unwrap();
        let iv = edge::interval(b, e).unwrap();
        for eu in edge::edgeuses(b, e, None, None).unwrap() {
            let Some(f) = eu1::face(b, eu).unwrap() else {
                continue;
            };
            let surface = face::surface(b, f).unwrap().unwrap();
            let trim = eu1::uv_trim_curve(b, eu).unwrap().0.unwrap();
            let trim_iv = trim.natural_interval();
            let mut gap = 0.0_f64;
            for i in 0..=128 {
                let t = iv.evaluate(f64::from(i) / 128.0);
                let uv = trim
                    .evaluate_point(k, t.clamp(trim_iv.min(), trim_iv.max()))
                    .unwrap();
                let p = surface.evaluate_point(k, Point2::new(uv.x, uv.y)).unwrap();
                gap = gap.max(p.distance(curve.evaluate_point(k, t).unwrap()));
            }
            fits.push((e, [iv.min(), iv.max()], gap, b.edge(e).unwrap().tolerance));
        }
    }
    fits
}

#[test]
fn fork_e36feb1_every_trim_lifts_onto_its_edge_with_the_fix() {
    let (k, b) = sheet(true);
    let fits = trim_fits(&k, &b);
    assert!(!fits.is_empty());
    for (e, range, gap, tolerance) in fits {
        assert_eq!(range, [0.0, 1.0], "{e:?}: the edge stays whole");
        assert!(
            gap <= tolerance,
            "{e:?}: its trim {gap} off it, tolerance {tolerance}"
        );
    }
}

/// Upstream stretches one piece onto the whole edge: the boundary row's trim runs 0.85 mm
/// off its edge.
#[test]
fn fork_e36feb1_the_boundary_row_trim_runs_off_its_edge_by_default() {
    let (k, b) = sheet(false);
    let worst = trim_fits(&k, &b)
        .into_iter()
        .map(|(_, _, gap, tolerance)| gap - tolerance)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        worst > 0.5,
        "the worst trim exceeds its tolerance by {worst}"
    );
}
