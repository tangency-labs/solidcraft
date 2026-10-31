//! `SmFeatureExecutive::DoDefeature` of face 2 of `Defeature/defillet.smb` (the
//! `blend.feature_executive` case `single_face_defillet_2`). Extending loop edge 14's
//! neighbour, face 8, XORs its extended B-spline surface with a copy of the face. In
//! that boolean, eight edge-definition imprints fail at `MakeEdgeInFace`'s
//! `FindVertexSector` checks: the extended surface folds over itself, so the
//! intersection UV curves sit on the other preimage of the face's vertices. Upstream
//! `IntersectInsertRelate` ignores those failures and goes on (`SmTopologyIntersector.cpp:1233`),
//! which is defined because `MakeEdgeInFace` makes both checks before any edit. The port
//! used to stop there as for UB-242's half-edited B-reps, so the XOR kept 7 faces instead
//! of 10 and the feature's patches differed. The C++ reference gives the values asserted
//! here.
//!
//! The model is the upstream test file, read by the Rust reader when the pinned upstream
//! checkout is present (as the difftest does).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

/// `Defeature/defillet.smb`, read and arena-named as the difftest reader does.
fn defillet(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Defeature/defillet.smb");
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

/// Per loop of the one feature: base and feature patch face counts (-1 for none).
fn patch_faces(ex: &FeatureExecutive<'_>) -> Vec<(i64, i64)> {
    let count = |p: Option<solidcore::blend::feature_executive::PatchId>| {
        p.and_then(|p| ex.patch(p))
            .map_or(-1, |b| bp1::faces(b, None).unwrap().len() as i64)
    };
    let feature = &ex.features()[0];
    feature
        .loops
        .iter()
        .map(|l| (count(l.base_patch), count(l.feat_patch)))
        .collect()
}

#[test]
fn defeature_goes_on_past_imprints_without_a_vertex_sector() {
    let mut k = Kernel::default();
    let Some(mut b) = defillet(&mut k) else {
        return;
    };
    let face = bp1::faces(&b, None).unwrap()[2];
    let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
    ex.add_feature(
        &mut k,
        &[face],
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::FromOtherBrep,
    )
    .unwrap()
    .unwrap();
    ex.do_defeature(&mut k).unwrap();
    let got = patch_faces(&ex);
    eprintln!("patch faces per loop: {got:?}");
    assert_eq!(got, EXPECTED);
}

/// The C++ reference's patch face counts (difftest `single_face_defillet_2`).
const EXPECTED: &[(i64, i64)] = &[(4, 4), (0, 0)];
