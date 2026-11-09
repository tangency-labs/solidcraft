//! #3094: a shell of a manifold solid reported success for a body that is no manifold
//! solid. The #1528 drilled plate (`fixtures/offset.offset_executive_p2/data/issue1528_plate.smb`,
//! a manifold solid) shelled inward by 2, open at its top face, with the convex edges
//! extended, the offset not merged (`merge_results` false) and an offset solid created
//! (the goldens case `issue1795_plate_api_shell_2_top_offset_solid_unmerged`) came back
//! with 16 faces, 40 edges and 29 vertices: Euler characteristic 5, no closed surface.
//!
//! The same through `offset_brep`: a 30 x 20 x 10 box with two edges chamfered 2 (edges 0 and
//! 9, as WR-657's tests) offset inward by 4 (merged results, extended convex edges, default
//! fixes) came back as a body that is no manifold solid.
//!
//! A shell of a manifold solid is a manifold solid or a refusal: the ops contract
//! (`finish`, docs/validity-and-tolerances.md section 4) at the kernel's entry point.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{api_shell, offset_full, planar_faces_at, shell, volume};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::offset::{OffsetFailure, Unsoundness};
use solidcore::error::Reason;
use solidcore::geom::attribute::AttributeStore;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{brep_is_closed_manifold, brep_is_manifold_solid};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel, SolidError};

fn plate(k: &mut Kernel) -> Brep {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(root.join("fixtures/offset.offset_executive_p2/data/issue1528_plate.smb"))
            .unwrap();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let fresh = Brep::new();
    let key_of = |e| fresh.owner_identity(e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut read = read_brep_from_file(k, Some(bytes), true, false, &mut env).unwrap();
    read.refresh_cached_topology();
    read
}

#[test]
fn a_shell_that_is_no_solid_is_refused() {
    let mut k = Kernel::default();
    let mut part = plate(&mut k);
    assert!(
        brep_is_manifold_solid(&part).unwrap(),
        "the input is a solid"
    );
    let open = planar_faces_at(&k, &part, 2, 27.5);
    assert_eq!(open.len(), 1, "the top face");
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -2.0,
            extend_convex_edges: true,
            merge_results: false,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: false,
    };
    let mut body = match api_shell(&mut k, &mut part, options, &open) {
        Ok(body) => body,
        Err(error) => {
            let SolidError::Failed(failure) = &error else {
                panic!("not a refusal: {error}");
            };
            assert_eq!(
                failure.reason,
                Reason::Offset(OffsetFailure::Unsound {
                    what: Unsoundness::NotManifoldSolid
                })
            );
            return;
        }
    };
    // a body reported as built must be a solid: on the base it is not
    let manifold = brep_is_manifold_solid(&body).unwrap();
    let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(
        manifold && lines.is_empty(),
        "reported as built: {} faces, {} edges, {} vertices, manifold solid {manifold}, \
         check_brep level 2 {} lines, the first {:?}",
        body.face_list().len(),
        body.edge_list().len(),
        body.vertex_list().len(),
        lines.len(),
        lines.first()
    );
}

/// The control: a valid offset and a valid shell of a manifold solid still succeed through
/// the gate, with the closed-form volumes (a 10 cube out by 1 is a 12 cube; in by 1 and
/// open at the top it is the 1000 less the 8 x 8 x 9 cavity).
#[test]
fn valid_offsets_and_shells_of_a_solid_still_succeed() {
    let mut k = Kernel::default();
    let mut cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let mut out = offset_full(&mut k, &mut cube, 1.0).unwrap();
    assert!(brep_is_manifold_solid(&out).unwrap());
    let v = volume(&k, &mut out).unwrap();
    assert!((v - 1728.0).abs() <= 1e-6 * 1728.0, "offset volume {v}");

    let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let open = planar_faces_at(&k, &cube, 2, 10.0);
    assert_eq!(open.len(), 1, "the top face");
    let mut shelled = shell(&mut k, cube, -1.0, &open).unwrap();
    assert!(brep_is_manifold_solid(&shelled).unwrap());
    let v = volume(&k, &mut shelled).unwrap();
    assert!((v - 424.0).abs() <= 1e-6 * 424.0, "shell volume {v}");
}

/// The 30 x 20 x 10 box with edges 0 and 9 chamfered 2.
fn chamfered_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let all = bp1::edges(&b, None).unwrap();
    fillet_edges(k, &mut b, &[all[0], all[9]], 2.0, 0, 1, 1.0).unwrap();
    b
}

#[test]
fn an_offset_that_is_no_solid_is_refused() {
    let mut k = Kernel::default();
    let mut part = chamfered_box(&mut k);
    assert!(
        brep_is_manifold_solid(&part).unwrap(),
        "the input is a solid"
    );
    match offset_full(&mut k, &mut part, -4.0) {
        Err(error) => {
            let SolidError::Failed(failure) = &error else {
                panic!("not a refusal: {error}");
            };
            assert_eq!(
                failure.reason,
                Reason::Offset(OffsetFailure::Unsound {
                    what: Unsoundness::NotManifoldSolid
                })
            );
        }
        Ok(body) => assert!(
            brep_is_manifold_solid(&body).unwrap(),
            "reported as built: {} faces, no manifold solid",
            body.face_list().len()
        ),
    }
}

/// The predicate the gates ask of a result, `brep_is_closed_manifold`: no shell vertex and
/// only manifold edges, the regions not counted. Two disjoint lumps pass it (as a body the
/// executive leaves with its regions unlabelled would), two lumps touching along an edge
/// do not (the edge has four faces).
#[test]
fn closed_manifold_does_not_count_regions_but_refuses_a_shared_edge() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    assert!(brep_is_closed_manifold(&a).unwrap());

    let b = create_box(&mut k, Point3::new(20.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let apart = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(apart.face_list().len(), 12, "two lumps");
    assert!(brep_is_closed_manifold(&apart).unwrap());

    let c = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let d = create_box(&mut k, Point3::new(10.0, 10.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let touching = boolean_union(&mut k, c, d).unwrap();
    assert!(!brep_is_closed_manifold(&touching).unwrap());
}
