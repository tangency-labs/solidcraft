//! `SmFeatureExecutive::DoDefeature` of face 2 of `Defeature/defillet.smb` (the
//! `blend.feature_executive` case `single_face_defillet_2`). Extending loop edge 14's
//! neighbour, face 8 (the fillet), XORs its extended surface with a copy of the face.
//! Upstream's fitted extension folds over itself there: eight edge-definition imprints fail
//! at `MakeEdgeInFace`'s `FindVertexSector` checks, which upstream `IntersectInsertRelate`
//! ignores and goes on past (`SmTopologyIntersector.cpp:1233`; defined, since
//! `MakeEdgeInFace` makes both checks before any edit), and its patch for that loop keeps no
//! face.
//!
//! On the exact extension (`surface::extend`, Tangent) the surface does not fold: the XOR
//! gives two faces, and `sm_TrimPatches` keeps the one bounded by the loop circle (radius
//! 0.7 round the boss axis at z 2), so the loop's own patch has one face where the C++ has
//! none. Neither loop's patch fills its void alone, so both are joined into the first
//! loop's patch, and the defeatured body is the C++ reference's (the difftest compares its
//! whole graph and geometry: 8 faces, 17 edges, 11 vertices). Only the unused loop patch's
//! face count moved (recorded in the difftest's known mismatches).
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
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
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

    // the second loop's one patch face is bounded by the loop circle
    let pid = ex.features()[0].loops[1].base_patch.unwrap();
    let patch = ex.patch(pid).unwrap();
    let on_circle = bp1::edges(patch, None).unwrap().into_iter().any(|e| {
        let r = patch.edge(e).unwrap();
        let c = r.curve.as_ref().unwrap();
        (0..=8).all(|i| {
            let p = c
                .evaluate_point(&k, r.interval.evaluate(f64::from(i) / 8.0))
                .unwrap();
            let rho = (p.x - 1.0).hypot(p.y - 1.0);
            (rho - 0.7).abs() < 1e-12 && (p.z - 2.0).abs() < 1e-12
        })
    });
    assert!(
        on_circle,
        "the loop patch is not bounded by the loop circle"
    );

    // the defeatured body is the C++ reference's
    let base = ex.base_brep();
    let counts = (
        bp1::faces(base, None).unwrap().len(),
        bp1::edges(base, None).unwrap().len(),
        bp1::vertices(base, None).unwrap().len(),
    );
    assert_eq!(counts, (8, 17, 11), "faces, edges, vertices");
}

/// Per loop, the base and feature patch face counts on the exact extension: the C++
/// reference's [(4, 4), (0, 0)] but for the second loop's own (joined, unused) patch.
const EXPECTED: &[(i64, i64)] = &[(4, 4), (1, 1)];
