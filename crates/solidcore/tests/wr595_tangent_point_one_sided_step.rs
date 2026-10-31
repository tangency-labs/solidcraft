//! WR-595 (the fork's tangency-labs/smlib@f8fd83e): `SmFilletSolver::ComputePointValues`
//! sets a tangent point's curve tangent by the central difference of a step forward and a
//! step back, though the intersection curve ends or branches there; where the step back
//! finds a near-tangent crossing beyond the point, the tangent is off the curve and the
//! trace from the point creeps on in small steps, piling up rail control points.
//! `fixes.fillet_tangent_point_one_sided_step` takes the forward step alone there.
//!
//! The fork commit has no test. The case is the upstream test file `Fillet/nurb_cone.smb`,
//! its edge 5 rounded r 0.5 (difftest case `stubs4-degenerate-nurb_cone-e5-r0.5` of
//! `blend.fillet_executive_p3`): its trace starts at a tangent point where both steps
//! find the intersection, and the round's two rails come out with 310 control points
//! each upstream, 286 with the fix, the volume the same to 5e-9.
//!
//! The flag alone turns the same file's edge 8 at r 0.5, which upstream builds, into a
//! failure: its trace starts at a tangent point along the edge's end tangent, and the
//! forward step alone along it misses the centre curve that the central difference
//! found. The fork has the next fix for that, tangency-labs/smlib@68fb916 (WR-603,
//! `fixes.fillet_trace_from_tangent_point_toward_next`): with both on, as in the fork,
//! edge 8 builds upstream's body and edge 5's rails have 181 control points.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::CurveKind;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edge;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

/// An upstream test file read by the Rust reader, arena-named.
fn read(k: &mut Kernel, rel: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(rel);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// `nurb_cone.smb` with edge `edge` rounded r 0.5 under `fixes.fillet_tangent_point_one_sided_step
/// = fix` and `fixes.fillet_trace_from_tangent_point_toward_next = toward_next`: the largest
/// control point count of an edge curve (the round's rails) and the volume, or `Err` when
/// the round fails.
fn round_cone_edge(
    edge: usize,
    fix: bool,
    toward_next: bool,
) -> Option<solidcore::Result<(usize, f64)>> {
    let mut k = Kernel::default();
    k.fixes.fillet_tangent_point_one_sided_step = fix;
    k.fixes.fillet_trace_from_tangent_point_toward_next = toward_next;
    let mut b = read(&mut k, "Fillet/nurb_cone.smb")?;
    let e = p1::edges(&b, None).unwrap()[edge];
    if let Err(err) = fillet_edges(&mut k, &mut b, &[e], 0.5, 1, 1, 1.0) {
        return Some(Err(err));
    }
    let ids: Vec<_> = b.edges().ids().collect();
    let most = ids
        .into_iter()
        .filter_map(|id| edge::curve(&b, id).unwrap())
        .filter_map(|c| match c.kind() {
            CurveKind::BSpline(s) => Some(s.control_points().unwrap().len()),
            _ => None,
        })
        .max()
        .unwrap();
    Some(Ok((most, volume(&k, &mut b))))
}

/// Edge 5 rounded r 0.5 with only WR-595's flag set as given.
fn rounded_cone(fix: bool) -> Option<(usize, f64)> {
    round_cone_edge(5, fix, false).map(|r| r.unwrap())
}

#[test]
fn trace_from_a_tangent_point_takes_the_forward_step_with_the_fix() {
    let Some((upstream, v_upstream)) = rounded_cone(false) else {
        return;
    };
    let (fixed, v_fixed) = rounded_cone(true).unwrap();
    // upstream (flag off): the central difference at the start
    assert_eq!(upstream, 310);
    // the fix: the forward difference, fewer rail points
    assert_eq!(fixed, 286);
    assert!(
        (v_fixed - v_upstream).abs() < 1e-7 * v_upstream,
        "{v_fixed} {v_upstream}"
    );
}

/// Edge 8 at r 0.5: upstream builds it; WR-595's flag alone fails it; with WR-603's flag
/// as well, as the fork has both, it builds upstream's body again, and edge 5's rails have 181
/// control points (310 upstream, 286 with WR-595's alone).
#[test]
fn edge_8_builds_with_the_fix_and_the_trace_toward_the_next_point() {
    let Some(upstream) = round_cone_edge(8, false, false) else {
        return;
    };
    let (_, v_upstream) = upstream.unwrap();
    // WR-595's flag alone: the round fails (`SM_ERR` from the session's finish)
    assert!(round_cone_edge(8, true, false).unwrap().is_err());
    // with WR-603 too, as in the fork: upstream's body
    let (_, v_both) = round_cone_edge(8, true, true).unwrap().unwrap();
    assert!(
        (v_both - v_upstream).abs() <= 1e-9 * v_upstream,
        "{v_both} {v_upstream}"
    );
    // edge 5 with both: fewer rail points still (upstream 310, WR-595 alone 286)
    let (fixed, v5) = round_cone_edge(5, true, true).unwrap().unwrap();
    let (_, v5_upstream) = round_cone_edge(5, false, false).unwrap().unwrap();
    assert_eq!(fixed, 181);
    assert!(
        (v5 - v5_upstream).abs() < 1e-7 * v5_upstream,
        "{v5} {v5_upstream}"
    );
}
