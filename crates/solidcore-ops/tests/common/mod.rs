//! Helpers the fillet test binaries share: the fix set, entity indices read back through the
//! kernel, and bodies the kernel builds turned into ops bodies.
#![allow(dead_code, clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::query::api_queries::{edge_closest_point, vertex_get_point};
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::{Brep, EdgeId, FaceId, Vec3};
use solidcore_ops::{make_box, session, Body};

/// The body `f` builds on a kernel with every fix on, as an ops body (through its `.smb` bytes).
pub fn built(f: impl FnOnce(&mut Kernel) -> solidcore::Result<Brep>) -> Body {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let b = f(&mut k).unwrap();
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &loopuse::notify_ref,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    Body::from_smb(
        write_brep_to_file(&k, Some(&b), &env, false).unwrap(),
        false,
    )
    .unwrap()
}

pub fn all_fixes() {
    session::set_fixes(UpstreamFixes::NAMES.iter().copied()).unwrap();
}

/// The 30 x 20 x 10 block at the origin.
pub fn block() -> Body {
    make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap()
}

pub const ABC: f64 = 6000.0;

pub fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1.0)
}

pub fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

/// `b` as the kernel reads it back from its `.smb` bytes (the same entity order), with a
/// kernel to query it: every fix on, as [`all_fixes`] runs the session, so a round kept as
/// its definition reads back.
pub fn kernel_copy(b: &Body) -> (Kernel, Brep) {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let bytes = b.to_smb(false).unwrap();
    let brep = read_brep_from_file(&mut k, Some(bytes), false, false, &mut env).unwrap();
    (k, brep)
}

/// The index of the edge of `b` through `p`.
pub fn edge_at(b: &Body, p: [f64; 3]) -> usize {
    let (k, brep) = kernel_copy(b);
    let q = Vec3::new(p[0], p[1], p[2]);
    brep.edge_list()
        .iter()
        .position(|&e| {
            edge_closest_point(&k, &brep, e, q)
                .unwrap()
                .closest
                .distance
                < 1e-9
        })
        .unwrap()
}

/// The index of the vertex of `b` at `p`.
pub fn vertex_at(b: &Body, p: [f64; 3]) -> usize {
    let (_, brep) = kernel_copy(b);
    brep.vertex_list()
        .iter()
        .position(|&v| {
            let q = vertex_get_point(&brep, v).unwrap();
            (q.x - p[0]).abs() + (q.y - p[1]).abs() + (q.z - p[2]).abs() < 1e-9
        })
        .unwrap()
}
