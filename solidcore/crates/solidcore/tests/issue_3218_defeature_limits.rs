//! #3218: a defeature that keeps a feature on its base names why its patch is not built
//! ([`DefeatureFailure`], carried by the refusal's [`Reason::Defeature`] at the open loop's
//! edge, and by each [`KeptFeature::limit`]). Each limit is decided where the patch's
//! construction refuses, from what it found there:
//!
//! - `LoopNotPlanar`: a planar cap asked for a cylinder fillet's loop (two lines and two arcs
//!   on the cylinder), which no plane holds;
//! - `ExtensionRefused`: the faces along the loop asked to be extended by 0;
//! - `TangentNeighbours`: `fillet1019`'s fillet that ends on another, tangent, fillet: the
//!   patch's piece on the wall is cut from that fillet's extension only along an edge the
//!   kernel proves smooth, so nothing bounds it there;
//! - `NotEnclosed`: a box face, whose neighbours, extended, run past it.
//!
//! The models are the upstream test files, read by the Rust reader when the pinned upstream
//! checkout is present.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::feature_executive::{FeatureExecutive, KeptFeature, RebuildBrepType};
use solidcore::error::defeature::DefeatureFailure;
use solidcore::error::{Reason, SM_ERR};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// An upstream test file, read and arena-named as the goldens reader does.
fn read(k: &mut Kernel, file: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
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

/// Face `face` of `file` defeatured alone with base rebuild `base_rb` (and the base extension
/// distance `ext`, when given): the refusal's limit, checked to be at an edge with the
/// status `SM_ERR`, and the base unchanged.
fn limit(
    file: &str,
    face: usize,
    base_rb: RebuildBrepType,
    ext: Option<f64>,
) -> Option<DefeatureFailure> {
    let mut k = Kernel::default();
    let mut b = read(&mut k, file)?;
    let faces = bp1::faces(&b, None).unwrap();
    let before = faces.len();
    let r = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        ex.add_feature(
            &mut k,
            &[faces[face]],
            base_rb,
            RebuildBrepType::ExtendSurface,
        )
        .unwrap()
        .unwrap();
        if let Some(d) = ext {
            ex.set_base_ext_dist(d);
        }
        ex.do_defeature(&mut k)
    };
    assert_eq!(
        bp1::faces(&b, None).unwrap().len(),
        before,
        "{file} {face}: base changed"
    );
    match &r {
        Err(e @ SolidError::Failed(f)) => {
            assert!(matches!(f.entity, Some(EntityRef::Edge(_))), "{r:?}");
            assert_eq!(e.sm_status(), Some(SM_ERR), "{r:?}");
            match f.reason {
                Reason::Defeature(d) => Some(d),
                _ => panic!("{file} {face}: {r:?}"),
            }
        }
        other => panic!("{file} {face}: a refusal naming its limit, got {other:?}"),
    }
}

#[test]
fn a_planar_cap_on_a_cylinder_fillet_s_loop_is_refused_as_not_planar() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::PlanarCap,
        None,
    ) else {
        return;
    };
    assert_eq!(l, DefeatureFailure::LoopNotPlanar);
}

#[test]
fn an_extension_by_nothing_is_refused_as_the_extension_s() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(0.0),
    ) else {
        return;
    };
    assert_eq!(l, DefeatureFailure::ExtensionRefused { distance: 0.0 });
}

#[test]
fn a_fillet_ending_on_a_tangent_fillet_is_refused_as_tangent_neighbours() {
    let Some(l) = limit(
        "Fillet/fillet1019.smb",
        3,
        RebuildBrepType::ExtendSurface,
        None,
    ) else {
        return;
    };
    let within = Kernel::default().tolerances.ang_tol_deg;
    assert_eq!(
        l,
        DefeatureFailure::TangentNeighbours { within_deg: within }
    );
}

#[test]
fn a_box_face_is_refused_as_not_enclosed() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        0,
        RebuildBrepType::ExtendSurface,
        None,
    ) else {
        return;
    };
    assert!(matches!(l, DefeatureFailure::NotEnclosed { .. }), "{l:?}");
}

/// A kept feature carries its limit too: of the two fillets of `cyl_on_box_w_fillet` one
/// comes off and the other is kept, naming why.
#[test]
fn a_kept_feature_names_its_limit() {
    let mut k = Kernel::default();
    let Some(mut b) = read(&mut k, "Fillet/cyl_on_box_w_fillet.smb") else {
        return;
    };
    let faces = bp1::faces(&b, None).unwrap();
    let kept: Vec<KeptFeature> = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        for f in [3, 8] {
            ex.add_feature(
                &mut k,
                &[faces[f]],
                RebuildBrepType::ExtendSurface,
                RebuildBrepType::ExtendSurface,
            )
            .unwrap()
            .unwrap();
        }
        ex.do_defeature(&mut k).unwrap()
    };
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].limit.is_some(), "{kept:?}");
}
