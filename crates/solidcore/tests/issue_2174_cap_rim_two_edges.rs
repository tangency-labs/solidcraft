//! tangency-labs/solidcore#2174: thickening the r 10 sphere's face above z = 5, taken
//! alone as a sheet, when its rim is two half circles placed against the sphere's seam
//! differently from #1502's tests (`issue_1502_z5_cap_thicken.rs`).
//!
//! Each rim half lifts onto the whole offset circle, a closed curve. `TrimExtendedEdge`
//! takes the half's first trim point where its start vertex goes; for the second it
//! refuses only the first point's parameter. Here the only solution at the second vertex
//! is the coincident range the other half's offset (the same circle) gives, and its far
//! end is the first point's twin across the circle's seam: another parameter, the same
//! point. One half got the whole circle, the other nothing (a zero-length interval), so
//! the offset loop had a gap: with `offset_closes_loop_gaps` the shell fails with
//! `SM_ERR_OFFSET_EDGE_VANISHES` (1043), and at the defaults it returns a non-manifold
//! body (volume -627.4 out, 0 in).
//!
//! `Kernel::fixes.offset_open_edge_second_trim_point` (WR-673; the fork's
//! tangency-labs/smlib@7ee0876) refuses that twin by point. The second trim point is then
//! not found, the half's offset edge is left to the merge, and the cap thickens to the
//! sector between radii 10 and 11 (out) or 9 and 10 (in) over the cap's solid angle pi:
//! V = pi/3 (11^3 - 10^3) = 331 pi/3 and pi/3 (10^3 - 9^3) = 271 pi/3. The results are a
//! manifold solid, clean at `check_brep` level 2, within 1e-6 of those (the issue's
//! tolerance; they come within 1.3e-8).
//!
//! Sheets: the issue's repro (the ball intersected with a box over z >= 5 turned 90
//! degrees about z; rim vertices (+-8.66, 0, 5), seam end (8.66, 0, 5)), the ball turned
//! 90 degrees less a box below z = 5 (rim vertices (8.66, 0, 5) and (0, 8.66, 5)), and
//! SolidCraft's sheet as it wrote it (`data/issue_2174_solidcraft_cap_sheet.smb`, which
//! failed in `relate` with 1001 out).
//!
//! The pinned C++ on the repro's sheet (the `issue2174` fixture of
//! `offset.offset_executive_p2`): inward it agrees with the port's default; outward it
//! crashes (SIGSEGV), so that direction has no difftest case.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{remove_faces, transform};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, FaceId};
use solidcore::SolidError;

/// The issue's flags: every fix on except `geometric_g2_continuity`, with or without
/// this one.
fn issue_kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    f.offset_open_edge_second_trim_point = fixed;
    k.fixes = f;
    k
}

/// The defaults and this fix alone.
fn only_this_fix() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.offset_open_edge_second_trim_point = true;
    k
}

/// The quarter turn about z.
fn quarter_turn() -> Axis2Placement {
    Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
    )
}

/// `c` with its one flat face removed: the spherical sheet alone.
fn without_flat_face(k: &mut Kernel, mut c: Brep) -> Brep {
    let flat: Vec<FaceId> = c
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            matches!(
                c.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Plane(_)
            )
        })
        .collect();
    assert_eq!(flat.len(), 1);
    c.set_editing_enabled(true);
    remove_faces(k, &mut c, true, &flat).unwrap();
    c.refresh_cached_topology();
    assert_eq!(c.face_list().len(), 1);
    c
}

/// The issue's repro: the ball intersected with a box over z >= 5 turned 90 degrees about
/// z. Its rim is two half circles meeting at (8.66, 0, 5) and (-8.66, 0, 5); its seam runs
/// from (8.66, 0, 5) to the pole.
fn intersected_cap(k: &mut Kernel) -> Brep {
    let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let mut above = create_box(k, Point3::new(-15.0, -15.0, 5.0), 30.0, 30.0, 10.0).unwrap();
    transform(k, &mut above, &quarter_turn(), None, false).unwrap();
    let c = boolean_intersection(k, ball, above).unwrap();
    without_flat_face(k, c)
}

/// The ball turned 90 degrees about z less a box below z = 5: rim vertices (8.66, 0, 5)
/// and (0, 8.66, 5), seam end (0, 8.66, 5).
fn turned_cap(k: &mut Kernel) -> Brep {
    let mut ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    transform(k, &mut ball, &quarter_turn(), None, false).unwrap();
    let below = create_box(k, Point3::new(-15.0, -15.0, -15.0), 30.0, 30.0, 20.0).unwrap();
    let c = boolean_difference(k, ball, below).unwrap();
    without_flat_face(k, c)
}

/// SolidCraft's sheet, written by SolidCraft (binary `.smb`).
fn solidcraft_cap(k: &mut Kernel) -> Brep {
    let fresh = Brep::new();
    let mut store = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let key_of = |e| fresh.owner_identity(e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let bytes = include_bytes!("data/issue_2174_solidcraft_cap_sheet.smb").to_vec();
    let mut b = read_brep_from_file(k, Some(bytes), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    assert_eq!(b.face_list().len(), 1);
    b
}

/// `SmApiShellBrepFull` thickening the sheet by `distance` (a solid, extended convex
/// edges, merged results), as the issue calls it.
fn thicken(k: &mut Kernel, sheet: &Brep, distance: f64) -> solidcore::Result<Brep> {
    let options = ShellOptions {
        offset: OffsetOptions {
            distance,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    shell_brep_simple(k, sheet, options, &[]).map(|mut b| {
        b.refresh_cached_topology();
        b
    })
}

/// The shell's sector over the cap's solid angle pi.
fn want(distance: f64) -> f64 {
    if distance > 0.0 {
        PI / 3.0 * (11.0_f64.powi(3) - 10.0_f64.powi(3))
    } else {
        PI / 3.0 * (10.0_f64.powi(3) - 9.0_f64.powi(3))
    }
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The thickened sheet is a manifold solid, clean at level 2, of the sector's volume.
fn assert_sound(mut k: Kernel, sheet: fn(&mut Kernel) -> Brep, distance: f64) {
    let s = sheet(&mut k);
    let mut r = thicken(&mut k, &s, distance).unwrap_or_else(|e| panic!("{distance}: {e}"));
    assert!(is_manifold_solid(&r).unwrap(), "{distance}: not manifold");
    let lines = check_brep(&mut k, &mut r, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{distance}: {lines:?}");
    let v = volume(&k, &mut r);
    let w = want(distance);
    assert!(
        (v - w).abs() <= 1e-6 * w,
        "{distance}: volume {v} vs {w} ({:e} relative)",
        (v - w).abs() / w
    );
}

#[test]
fn intersected_cap_thickens_out() {
    assert_sound(issue_kernel(true), intersected_cap, 1.0);
}

#[test]
fn intersected_cap_thickens_in() {
    assert_sound(issue_kernel(true), intersected_cap, -1.0);
}

#[test]
fn turned_cap_thickens_out() {
    assert_sound(issue_kernel(true), turned_cap, 1.0);
}

#[test]
fn turned_cap_thickens_in() {
    assert_sound(issue_kernel(true), turned_cap, -1.0);
}

#[test]
fn solidcraft_cap_thickens_out() {
    assert_sound(issue_kernel(true), solidcraft_cap, 1.0);
}

#[test]
fn solidcraft_cap_thickens_in() {
    assert_sound(issue_kernel(true), solidcraft_cap, -1.0);
}

/// The fix alone, on the defaults, is enough.
#[test]
fn intersected_cap_thickens_with_this_fix_alone() {
    assert_sound(only_this_fix(), intersected_cap, 1.0);
    assert_sound(only_this_fix(), intersected_cap, -1.0);
}

/// Upstream behaviour, with every other fix on: the loop gap is refused (1043).
#[test]
fn flag_off_refuses_the_gap() {
    for distance in [1.0, -1.0] {
        let mut k = issue_kernel(false);
        let s = intersected_cap(&mut k);
        match thicken(&mut k, &s, distance) {
            Err(SolidError::Upstream { code, .. }) => assert_eq!(code, 1043, "{distance}"),
            other => panic!("{distance}: {other:?}"),
        }
    }
}

/// Upstream behaviour at the defaults (the pinned C++ inward): a body that is no manifold
/// solid.
#[test]
fn defaults_give_no_manifold_solid() {
    for distance in [1.0, -1.0] {
        let mut k = Kernel::default();
        let s = intersected_cap(&mut k);
        let r = thicken(&mut k, &s, distance).unwrap();
        assert!(!is_manifold_solid(&r).unwrap(), "{distance}");
    }
}

/// Writes the repro's sheet as the ASCII `.smb` of the `issue2174` fixture of
/// `offset.offset_executive_p2` (data flows Rust to C++ only).
#[test]
#[ignore = "generator: writes the #2174 fixture file"]
fn write_issue_2174_operand() {
    use solidcore::geom::object::ObjectRef;
    use solidcore::topology::api_files::write_brep_to_file;
    use solidcore::topology::brep_data_write::FromBrepEnv;
    use solidcore::topology::{EdgeId, EdgeuseId};
    let mut k = Kernel::default();
    let b = intersected_cap(&mut k);
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let fresh = Brep::new();
    let key_of = |e| -> ObjectRef { fresh.owner_identity(e) };
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bytes = write_brep_to_file(&k, Some(&b), &env, true).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path =
        root.join("fixtures/offset.offset_executive_p2/data/issue2174_cap_two_rim_edges.smb");
    std::fs::write(&path, &bytes).unwrap();
}
