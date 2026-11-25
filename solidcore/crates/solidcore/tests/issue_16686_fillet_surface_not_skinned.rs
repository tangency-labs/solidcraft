//! tangency-labs/solidcraft#16686: when a round's trace cannot skin its fillet surface, the
//! flush notes the solver's status (`SurfSkinningFailure`) and stops before the rails get
//! their curves. Upstream's `CalcFilletGeom` then returns success with every trace point
//! processed, and a later step fails on the rails it never made (a null pointer, 1006, was
//! how the goldens case `sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1` ended). The
//! solver's geometry step now fails, as `CreateFilletBrep` reports a failed `CalcFilletGeom`:
//! `SM_FILERR_EDGE_PROBLEM` (4) with status 10 at the edge. That refusal's own sequences
//! are pinned by `fillet_solver::p4`'s `UnmadeGeometry` and `c4_trace` unit tests.
//!
//! The case was the goldens one `sweep5-sphere_diff_3cylinders-e29-r1.0`:
//! `Fillet/sphere_diff_3cylinders.smb`, edge 29 rounded r 1 (circular cross section, G1,
//! thumbweight 1), which failed to skin. Its supports are two cylinders whose offsets meet
//! in a rational class only within the body's tolerance (offset radii 23.00935363769532
//! and ...313, axes tilted 1.2e-7), and the exact round is now made on a spine proved
//! within that tolerance of both offsets (`surface::implicit::spine_near_offsets`), so the
//! round no longer reaches the skinning: it builds. The test now pins that build at r 1,
//! 0.5 and 0.1: no error note, every round face recertified, one face more, the B-rep check
//! reporting exactly the input's own findings (three seam warnings of the read body, none
//! new), and material only removed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// An upstream test file read by the Rust reader, arena-named.
fn read(k: &mut Kernel, rel: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(rel);
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
    Some(b)
}

/// The body's volume, integrated to 1e-11.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

#[test]
fn the_round_whose_surface_could_not_be_skinned_builds_on_its_exact_class() {
    for r in [1.0, 0.5, 0.1] {
        let mut k = Kernel::default();
        let Some(mut b) = read(&mut k, "Fillet/sphere_diff_3cylinders.smb") else {
            return;
        };
        let before = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        let faces = p1::faces(&b, None).unwrap().len();
        let v0 = volume(&k, &mut b);
        let edge = p1::edges(&b, None).unwrap()[29];
        {
            let mut s = FilletSession::new(&mut k, &mut b).unwrap();
            s.prepare_constant_radius_edges(&mut k, &[edge], r, 1, 1, 1.0)
                .unwrap();
            let result = s.do_fillet(&mut k);
            assert!(result.is_ok(), "r {r}: {result:?}");
            assert_eq!(s.error_info().code, 0, "r {r}: {:?}", s.error_info());
        }
        // every round face's exact class proved again on the result
        solidcore::surface::procedural_blend::recertify_faces(&k, &b).unwrap();
        // the session leaves the port's cached topology to its caller, as the API's
        // fillets refresh it before they return
        b.refresh_cached_topology();
        assert_eq!(p1::faces(&b, None).unwrap().len(), faces + 1, "r {r}");
        let after = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert_eq!(after.lines(), before.lines(), "r {r}");
        // a convex edge's round only removes material: the drop is more than the two
        // integrations' requested accuracy (relative, so taken at the larger volume)
        let v = volume(&k, &mut b);
        assert!(v0 - v > 2e-11 * v0, "r {r}: {v0} -> {v}");
    }
}
