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

/// `nurb_cone.smb` with edge 5 rounded r 0.5: the largest control point count of an
/// edge curve (the round's rails) and the volume.
fn rounded_cone(fix: bool) -> Option<(usize, f64)> {
    let mut k = Kernel::default();
    k.fixes.fillet_tangent_point_one_sided_step = fix;
    let mut b = read(&mut k, "Fillet/nurb_cone.smb")?;
    let e = p1::edges(&b, None).unwrap()[5];
    fillet_edges(&mut k, &mut b, &[e], 0.5, 1, 1, 1.0).unwrap();
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
    Some((most, volume(&k, &mut b)))
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
