//! The infinite-region ray fire of a near-flat B-rep (#1570, `mat0076`): mirroring
//! `Solids/revolution.smb` after a rotation and a scale of about `5.6e-11` along one
//! axis fires rays whose face line solves fetch each face's surface cache. On a
//! flattened face the trimmed cache cannot be built (`Evaluate1stDerivatives` finds no
//! non-zero tangent while implanting a vertex use), so `smsurf_GetSurfaceCache` falls
//! back to a plain `SmSurfaceCache`, whose `GetTree` builds no face boundary. The ray
//! fire built that fallback again as a trimmed cache, failed the same way, and every
//! ray of the search failed: `FindAndSetInfiniteRegion` returned `SM_ERR` where the
//! C++ reference succeeds.
//!
//! The steps are those `SmBrep::Transform(const double[4][4])` takes for the `mat0076`
//! matrix (its `V` and `Uᵀ` transforms and mirrors), through the placement transform
//! and the mirror alone. The solid is the upstream test file, read when the pinned
//! upstream checkout is present (as the difftest SMS tests do).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::object::ObjectRef;
use solidcore::math::Vec3;
use solidcore::topology::brep::mirror::mirror;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p5::transform;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

fn revolution(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Solids/revolution.smb");
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
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.refresh_cached_topology();
    Some(b)
}

#[test]
fn issue_1570_mirror_of_a_flattened_revolution_finds_its_infinite_region() {
    let mut k = Kernel::default();
    let Some(mut b) = revolution(&mut k) else {
        return;
    };
    let origin = Vec3::new(0.0, 0.0, 0.0);
    let v = Axis2Placement::new(
        origin,
        Vec3::new(
            9.748731915882214e-11,
            0.9296172572952511,
            -0.368526464367018,
        ),
        Vec3::new(
            1.2211649548646432e-11,
            0.3685264643670182,
            0.929617257295251,
        ),
    );
    let scale_v = Vec3::new(1.0, 0.9999999999999998, 1.0000000000000002);
    let ut = Axis2Placement::new(
        origin,
        Vec3::new(0.3372020359976084, 0.9361822322780193, -0.0992855220362577),
        Vec3::new(0.8593263568435076, -0.2630021228950323, 0.4386206741442671),
    );
    let scale_ut = Vec3::new(5.635977911045567e-11, 0.8270541083737828, 1.02452826451937);

    transform(&mut k, &mut b, &v, Some(scale_v), true).unwrap();
    mirror(&mut k, &mut b, &v).unwrap();
    transform(&mut k, &mut b, &ut, Some(scale_ut), true).unwrap();
    // the infinite-region search of this mirror needs the subdivision surfaces
    mirror(&mut k, &mut b, &ut).unwrap();
    assert!(b.infinite_region().is_some());
}
