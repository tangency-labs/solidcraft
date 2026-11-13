//! Regression test for WR-531 (the fork's tangency-labs/smlib@d5dadaa, capi test
//! `shelled_box_with_a_drilled_hole`): a box with a blind drilled hole (r 3, a 118
//! degree tip) shelled 2 mm inward, open at a side and, separately, at the top. The
//! hole's tip is a cone's apex whose seam is its only edge: the extended offset builds
//! no offset there, so the cone's offset ends open on a circle. Pinned SMLib returns
//! that non-manifold offset as a success (`SmOffsetExecutive.cpp:255-262`). Here the
//! default shell refuses it with or without WR-454's `shell_empty_cavity_fails` check,
//! retaining the input. With `offset_point_vertex_caps` the apex's offset is a sphere
//! zone of radius 2 and the shell is a closed solid.
//!
//! The successful result must pass the level-2 check (`check_brep`) clean.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell, volume, Notifications};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::error::offset::{OffsetFailure, Unsoundness};
use solidcore::error::Reason;
use solidcore::math::{Point3, Vec3, PI};
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel, SolidError};

/// A 40 x 40 x 20 box with a hole r 3 drilled 10 deep from the top at (20, 20), its
/// tip a 118 degree cone.
fn drilled_box(k: &mut Kernel) -> Brep {
    let (r, top, depth) = (3.0, 20.0, 10.0);
    let tip = r / 59.0_f64.to_radians().tan();
    let bottom = top - depth;
    let at = |x: f64, z: f64| Point3::new(20.0 + x, 20.0, z);
    let profile = [
        (at(0.0, bottom - tip), at(r, bottom)),
        (at(r, bottom), at(r, top + 1.0)),
        (at(r, top + 1.0), at(0.0, top + 1.0)),
        (at(0.0, top + 1.0), at(0.0, bottom - tip)),
    ];
    let curves: Vec<Curve> = profile
        .iter()
        .map(|&(a, b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let curves: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let drill = create_rotational_sweep(
        k,
        &curves,
        at(0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        360.0,
        true,
    )
    .unwrap();
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    boolean_difference(k, block, drill).unwrap()
}

/// The drilled box shelled 2 inward, open at the top or at x = 0, with WR-531's `fixed`
/// and WR-454's `refuse_open`.
fn shelled(fixed: bool, refuse_open: bool, open_top: bool) -> solidcore::Result<(Kernel, Brep)> {
    let mut k = Kernel::default();
    k.fixes.offset_point_vertex_caps = fixed;
    k.fixes.shell_empty_cavity_fails = refuse_open;
    let part = drilled_box(&mut k);
    let open = if open_top {
        planar_faces_at(&k, &part, 2, 20.0)
    } else {
        planar_faces_at(&k, &part, 0, 0.0)
    };
    assert_eq!(open.len(), 1, "the open face");
    let b = shell(&mut k, part, -2.0, &open)?;
    Ok((k, b))
}

/// The level-2 check's failure count and report lines.
fn check(k: &mut Kernel, b: &mut Brep) -> (usize, Vec<String>) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    (c.failure_count(), c.lines())
}

fn spheres(b: &Brep) -> usize {
    b.face_list()
        .iter()
        .filter(|&&f| {
            b.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Sphere(_)))
        })
        .count()
}

#[test]
fn wr_531_drilled_hole_shells_with_a_sphere_cap() {
    for open_top in [false, true] {
        for refuse_open in [false, true] {
            let case = format!("open_top {open_top} refuse_open {refuse_open}");
            let (mut k, mut b) = shelled(true, refuse_open, open_top).unwrap();
            assert!(
                brep_is_manifold_solid(&b).unwrap(),
                "{case}: not a closed solid"
            );
            assert_eq!(spheres(&b), 1, "{case}: one cap on a sphere");
            let (failed, lines) = check(&mut k, &mut b);
            assert_eq!(failed, 0, "{case}: {lines:?}");
        }
    }
}

/// Without a sphere cap the cone's offset stays open at the tip. The shell
/// refuses it and retains the input, regardless of the legacy WR-454 flag.
#[test]
fn wr_531_without_a_tip_cap_the_shell_is_refused_and_keeps_the_input() {
    for open_top in [false, true] {
        for refuse_open in [false, true] {
            let case = format!("open_top={open_top} refuse_open={refuse_open}");
            let mut k = Kernel::default();
            k.fixes.shell_empty_cavity_fails = refuse_open;
            let mut part = drilled_box(&mut k);
            let open = if open_top {
                planar_faces_at(&k, &part, 2, 20.0)
            } else {
                planar_faces_at(&k, &part, 0, 0.0)
            };
            assert_eq!(open.len(), 1);
            let mut notifications = Notifications::new(&mut part);
            let key = part.key();
            let tolerance = part.tolerance();
            let ids = (
                part.face_list().to_vec(),
                part.edge_list().to_vec(),
                part.vertex_list().to_vec(),
            );
            let points: Vec<_> = part
                .vertices()
                .iter()
                .map(|(id, v)| (id, v.point, v.tolerance))
                .collect();
            let outcome = OffsetExecutive::shell_brep(
                &mut k,
                part,
                ShellOptions {
                    offset: OffsetOptions {
                        distance: -2.0,
                        extend_convex_edges: true,
                        merge_results: true,
                    },
                    create_offset_solid: false,
                    merge_sheet_side_walls: false,
                },
                &open,
                &mut notifications.context(),
            );
            let error = outcome.result.as_ref().unwrap_err();
            let SolidError::Failed(failure) = error else {
                panic!("{case}: not a typed refusal: {error}");
            };
            assert_eq!(
                failure.operation, "offset::offset_executive::p2::shell_brep",
                "{case}: {error:?}"
            );
            assert_eq!(
                failure.reason,
                Reason::Offset(OffsetFailure::Unsound {
                    what: Unsoundness::NotManifoldSolid
                }),
                "{case}: {error:?}"
            );
            assert!(!outcome.input_ownership_transferred());
            let mut retained = outcome.retained_input.unwrap();
            assert_eq!(retained.key(), key);
            assert_eq!(retained.tolerance(), tolerance);
            assert_eq!(
                (
                    retained.face_list().to_vec(),
                    retained.edge_list().to_vec(),
                    retained.vertex_list().to_vec()
                ),
                ids
            );
            assert_eq!(
                retained
                    .vertices()
                    .iter()
                    .map(|(id, v)| (id, v.point, v.tolerance))
                    .collect::<Vec<_>>(),
                points
            );
            // Inspect the returned caches without refreshing away an inconsistency.
            assert!(
                solidcore::topology::integrity::check(&retained).is_ok(),
                "{case}"
            );
            assert!(brep_is_manifold_solid(&retained).unwrap(), "{case}");
            assert_eq!(
                spheres(&retained),
                0,
                "{case}: the original has no sphere cap"
            );
            for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
                let checked = check_brep(&mut k, &mut retained, level).unwrap();
                assert!(
                    checked.is_valid(),
                    "{case}, {level:?}: {:?}",
                    checked.lines()
                );
            }
            let v = volume(&k, &mut retained).unwrap();
            let tip = 3.0 / 59.0_f64.to_radians().tan();
            let exact = 40.0 * 40.0 * 20.0 - PI * 3.0 * 3.0 * (10.0 + tip / 3.0);
            // Original block less the cylindrical bore and its conical drill tip.
            assert!((v - exact).abs() <= 1e-9 * exact, "{case}: {v}");
        }
    }
}
