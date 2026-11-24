//! tangency-labs/solidcore#1476: shelling a swept bar failed with 1001. SolidCraft rig
//! case shell/566: an 8 x 5 rectangle swept (by the C++ sweep) along a 30 mm line, a
//! quarter arc of r 30 and a 30 mm line (14 faces: 12 B-spline or extrusion walls, two
//! planar caps), shelled 0.5 inward open at its start cap (z = 0). The operand is the
//! rig's working copy right before the shell (`data/issue_1476_swept_bar.smd`).
//!
//! Two causes, both fixed unconditionally:
//! - `OffsetExecutive::replace_implicit_offsets` took only a plain B-spline base;
//!   upstream's `SM_CAST_PTR(SmBSplineSurface, pBase)` takes every subclass, here a
//!   straight wall's `SmSurfOfExtrusion`. Both tests fail with 1001 without it.
//! - With SolidCraft's opt-in fixes on (the issue's configuration), the bend walls'
//!   offset curves (125 and 127 cubic segments) need 37 625 and 38 227 samples at
//!   1e-5, above the former fixed HANG-44 cap of 16 384; the bound is now
//!   input-scaled (`nurbs::crv_offset::offset_sample_bound`). The second test fails
//!   with 1001 without it. With the defaults the shell takes another path and never
//!   samples that many.
//!
//! Expected: one solid, V = 40 L - 28 (L - 0.5) = 1299.48668 with L = 60 + 15 pi, the ideal
//! shape's. With SolidCraft's fixes the port gives the closed B-rep's volume, 1299.48310,
//! origin-independent since #16735, pinned to 1e-9; the operand's own volume is 1.2e-2 below
//! its closed form. (The earlier pin, 1299.48231, integrated one wall clamped, its trims
//! 1.5e-6 past its plane's domain. #15946, closing the faces
//! independently, pins 1299.4830955384505, 2e-15 from this pin.) With the kernel's own
//! defaults the volume is checked against the closed form only.
//!
//! The shell ends with `make_edges_on_analytics_exact` (WR-545, once the
//! `shell_edges_on_analytics_exact` flag, which the SolidCraft rig ran on). It intersected
//! the walls' face-owned surfaces without their faces and the shell failed with
//! `Unported(topology.face_p2)`; it now goes through the result's face services. With the
//! kernel's defaults two marched edges between the inner planar walls become lines; with
//! SolidCraft's fixes none does; the volumes stay as above.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;
#[path = "support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};
use solidcraft_fixes::solidcraft_fixes;

/// `SmBrep::ReadFromFile` of the rig's operand.
fn operand(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_1476_swept_bar.smd").to_vec();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// The shell of the operand (`SmApiShellBrepFull(b, -0.5, TRUE, TRUE, TRUE, [start
/// cap])`), with its run time.
fn shell(fixes: &UpstreamFixes) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.clone_from(fixes);
    let mut part = operand(&mut k);
    assert_eq!(part.face_list().len(), 14);
    let start = planar_faces_at(&k, &part, 2, 0.0);
    assert_eq!(start.len(), 1);
    let mut notifications = Notifications::new(&mut part);
    let clock = std::time::Instant::now();
    let result = OffsetExecutive::shell_brep(
        &mut k,
        part,
        ShellOptions {
            offset: OffsetOptions {
                distance: -0.5,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        &start,
        &mut notifications.context(),
    )
    .result;
    eprintln!("issue_1476 shell: {:.2?}", clock.elapsed());
    (k, result)
}

/// Checks the shell and returns its number of line edges.
fn check(fixes: &UpstreamFixes, want: Option<f64>) -> usize {
    check_validated(fixes, want, false)
}

/// [`check`]; with `validate`, the shell also passes `check_brep` at every level
/// (`Level2` runs every test, SolidCraft's `CheckLevel::All`).
fn check_validated(fixes: &UpstreamFixes, want: Option<f64>, validate: bool) -> usize {
    let (mut k, result) = shell(fixes);
    let mut b = result.unwrap();
    b.refresh_cached_topology();
    assert!(brep_is_manifold_solid(&b).unwrap());
    if validate {
        for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
            let checked = check_brep(&mut k, &mut b, level).unwrap();
            assert!(checked.is_valid(), "{level:?}: {:?}", checked.lines());
        }
    }
    let v = volume(&k, &mut b).unwrap();
    let kinds: Vec<&str> = b
        .edge_list()
        .iter()
        .map(
            |&e| match b.edge(e).unwrap().curve.as_ref().map(Curve::kind) {
                Some(CurveKind::Line(_)) => "line",
                Some(CurveKind::Ellipse(_)) => "conic",
                Some(_) => "other",
                None => "none",
            },
        )
        .collect();
    eprintln!(
        "issue_1476 shell: {} faces, volume {v}, edges: {} lines, {} conics, {} other",
        b.face_list().len(),
        kinds.iter().filter(|&&k| k == "line").count(),
        kinds.iter().filter(|&&k| k == "conic").count(),
        kinds.iter().filter(|&&k| k == "other").count(),
    );
    let analytic =
        40.0 * (60.0 + 15.0 * std::f64::consts::PI) - 28.0 * (59.5 + 15.0 * std::f64::consts::PI);
    assert!((v - analytic).abs() <= 1e-5 * analytic, "{v} vs {analytic}");
    if let Some(want) = want {
        assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    }
    kinds.iter().filter(|&&k| k == "line").count()
}

/// SolidCraft's configuration (which also keeps an inset's cavity regions, now the
/// default): the shell is the closed form and passes `check_brep` at every level. Slow in
/// debug builds, as below.
#[test]
#[ignore = "slow in debug builds (two 38 000-sample curve offsets)"]
fn issue_1476_swept_bar_shells_with_solidcraft_fixes_validates() {
    assert_eq!(check_validated(&solidcraft_fixes(), None, true), 16);
}

/// About 6 s at opt-level 3 but over 4 min in a debug build (the two 38 000-sample
/// offsets' knot removal): run with `--ignored` (`--profile difftest`). The default
/// test below keeps the extrusion fix live; `nurbs::crv_offset`'s unit tests keep the
/// bound live.
///
/// The shell ends with `make_edges_on_analytics_exact` (WR-545; the rig's run of shell/566
/// failed with `Unported(topology.face_p2)` before it reached the faces through the result's
/// face services): the shell builds and no edge changes. The inner straight walls are
/// certified nets (WR-718) that do not give the recognised plane's points at the same
/// parameters, so the merge keeps them as B-splines (`CopyAndAddAnalytics`, WR-450); no
/// marched edge then lies between two analytic faces, and the shell has the
/// lines and the volume it had without the pass (with `N_SrfOffset`'s fits two edges became
/// lines). The fork (tangency-labs/smlib 68fb9162) gives V = 1299.4823.
#[test]
#[ignore = "slow in debug builds (two 38 000-sample curve offsets)"]
fn issue_1476_swept_bar_shells_with_solidcraft_fixes() {
    assert_eq!(check(&solidcraft_fixes(), Some(1299.4830955384534)), 16);
}

/// The default kernel, which keeps an inset's cavity regions (WR-667). The merge leaves one
/// finite region (deduced: before #17064 the one-name fixture shelled with this selection, which
/// the verdicts' point-solve cache would have refused), which `make_manifold` keeps whatever its cavity
/// verdict, so the verdict's point solves over the operand are not run (#17393).
///
/// The shell ends with `make_edges_on_analytics_exact` (WR-545), which
/// intersects the result's face-owned surfaces (the extrusion walls) with their faces'
/// trimmed caches, as the fork's `GlobalSurfaceIntersect` reaches them through
/// `SmSurface::GetFace`; without the face context it failed with
/// `Unported(topology.face_p2)` (SolidCraft rig case shell/566). Two marched edges
/// between the inner planar walls become lines; the volume does not change. The two
/// inner straight walls are certified nets (WR-718), bicubic and not recognised as
/// extrusions when merged, so their edges with the end cap stay marched (`N_SrfOffset`'s
/// fit was recognised, and those two became lines as well). The volume moves by 1.2e-10
/// relative, within the pin.
#[test]
fn issue_1476_swept_bar_shells_with_edges_on_analytics_exact() {
    assert_eq!(check(&UpstreamFixes::default(), None), 18);
}

/// The shell's volume is the closed B-rep's, origin-independent (#16735; adapted from
/// #15946's test by @bherbruck). One wall's trims leave its plane's analytic domain by
/// 1.5e-6 in u: integrated clamped while its strips start on the continued trims, the
/// volume moved with the origin's z. About the box's middle and 100 above and below it,
/// and moved 1e3 up about the world origin, the volume agrees within the integration's own
/// error scale ([`integration_scale`]) of the two volumes compared.
#[test]
fn issue_1476_volume_is_origin_independent() {
    use solidcore::mass::integrator::properties::{
        brep_properties, MassPropertyBehavior, PropertyAttributes,
    };
    use solidcore::math::{Point3, Vec3};
    use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
    use solidcore::topology::proto_topology::p1::api_transform::{api_translate, TransformObject};
    const ACCURACY: f64 = 1e-9;
    let (mut k, result) = shell(&UpstreamFixes::default());
    let mut b = result.unwrap();
    b.refresh_cached_topology();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = |k: &Kernel, b: &mut Brep, origin: Point3| -> f64 {
        brep_properties(
            k,
            b,
            ACCURACY,
            origin,
            MassPropertyBehavior::default(),
            &attrs,
        )
        .unwrap()
        .volume
    };
    let mid = calculate_bounding_box(&k, &mut b, BoundingBoxOptions::default())
        .unwrap()
        .mid();
    let got: Vec<_> = [0.0, 100.0, -100.0]
        .iter()
        .map(|&dz| {
            let origin = mid + Vec3::new(0.0, 0.0, dz);
            (
                volume(&k, &mut b, origin),
                integration_scale(&k, &mut b, origin, ACCURACY),
            )
        })
        .collect();
    for (i, (vi, ei)) in got.iter().enumerate() {
        for (vj, ej) in got.iter().skip(i + 1) {
            let spread = (vi - vj).abs();
            assert!(spread <= ei + ej, "spread {spread:e} above {:e}", ei + ej);
        }
    }
    let here = volume(&k, &mut b, Point3::ZERO);
    let eh = integration_scale(&k, &mut b, Point3::ZERO, ACCURACY);
    api_translate(
        &mut k,
        Some(TransformObject::Brep(&mut b)),
        Vec3::new(0.0, 0.0, 1e3),
    )
    .unwrap();
    let there = volume(&k, &mut b, Point3::ZERO);
    let et = integration_scale(&k, &mut b, Point3::ZERO, ACCURACY);
    assert!(
        (here - there).abs() <= eh + et,
        "{here} moved 1e3 up: {there} (allowed {:e})",
        eh + et
    );
}

/// The integration's own error scale for a body's volume about `origin` at `accuracy`: what
/// `brep_properties` holds each face's volume to (`boundary_properties`' scale for the
/// volume component: the area scale `estimated / 10`, `estimated` the body box's largest
/// size squared over 100, times a length, the larger of `10 sqrt(estimated)` and the face's
/// distance from the origin; taken here at the body box's farthest point, which no face is
/// farther than), summed over the faces. An estimate the integrator steps by, not a proved
/// bound (ARGUED).
fn integration_scale(
    k: &Kernel,
    b: &mut Brep,
    origin: solidcore::math::Point3,
    accuracy: f64,
) -> f64 {
    use solidcore::geom::extent3d::Extent3d;
    use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
    let bbox = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default()).unwrap();
    let size = bbox.max_dimension();
    let estimated = size * size / 100.0;
    let far = bbox.max_distance(&Extent3d::from_point(origin));
    let length = (10.0 * estimated.sqrt()).max(far);
    accuracy * (estimated / 10.0) * length * b.face_list().len() as f64
}
