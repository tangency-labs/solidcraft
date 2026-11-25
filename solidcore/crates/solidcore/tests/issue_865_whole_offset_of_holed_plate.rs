//! tangency-labs/solidcore#865 (regression): the ladder 09/011 part fails shelled closed
//! (1001) and its `SmApiOffsetBrepFull` is a body of volume 0, in the SMLib fork. The
//! fork traced both to #977 (`RemoveExcessFaces` before the merge), whose closed-shell
//! case the Rust port covers with `issue_977_closed_shell_excess_faces_after_merge`.
//! The ladder part is not public; this probe takes the whole offset of #977's plate
//! (30 x 20 x 6, an r1.5 through hole 1 from the front side) 1.5 inward: a manifold solid of
//! the closed form's volume (plan/findings/offset.offset_executive_p2--issue-400-865-shell-probes.md).
//! #977's excess faces are found after the merge and the offset faces' crossing loops
//! (#854, WR-345) are resolved, both always.
//!
//! The inward offset is the plate shrunk by 1.5 (27 x 17 x 3) less the hole's offset
//! (r 3 about (15, 2.5)), which crosses the front's offset at y = 1.5.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;
#[path = "support/volume_leak.rs"]
mod volume_leak;

use core::f64::consts::PI;

use offset_fixture::{offset_full, report, volume};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::error::offset::{LoopCrossing, OffsetFailure};
use solidcore::error::Reason;
use solidcore::geom::attribute::AttributeStore;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::offset::offset_executive::p2::{
    api_offset_brep_full, shell_brep_simple, OffsetOptions, ShellOptions,
};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

fn plate(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 6.0).unwrap();
    let hole = create_cylinder(k, Point3::new(15.0, 2.5, -1.0), 1.5, 8.0).unwrap();
    boolean_difference(k, plate, hole).unwrap()
}

fn want() -> f64 {
    let t: f64 = 1.5;
    let r = 1.5 + t;
    let d = 2.5 - t; // the hole offset's centre from the front's offset
    let segment = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let disc_inside = PI * r * r - segment;
    ((30.0 - 2.0 * t) * (20.0 - 2.0 * t) - disc_inside) * (6.0 - 2.0 * t)
}

fn run() -> bool {
    let mut k = Kernel::default();
    let mut b = plate(&mut k);
    let result = offset_full(&mut k, &mut b, -1.5);
    report(
        &k,
        "issue_865 whole offset -1.5",
        result,
        Some(want()),
        1e-6,
    )
}

#[test]
fn issue_865_whole_offset_with_fixes() {
    assert!(run());
}

/// The plate with a second through hole, r 1 about (15, 12), well clear of everything:
/// shrunk 1.5 its offset (r 2.5) crosses no other loop. On the offset faces z = 1.5 and
/// z = 4.5 its loop is one closed edge that no crossing cuts, beside the first hole's
/// loop, which crosses the front's. That whole edge is one stretch from its vertex round
/// to the same vertex; it was taken for a stretch too short to build ("three loops cross
/// at one point", `SM_ERR_BAD_INTERSECTIONS`) and the offset refused.
///
/// Closed form: [`want`] less the second hole's offset disc over the height 3.
#[test]
fn an_uncut_hole_beside_a_crossing_is_kept_whole() {
    let mut k = Kernel::default();
    let first = plate(&mut k);
    let second = create_cylinder(&mut k, Point3::new(15.0, 12.0, -1.0), 1.0, 8.0).unwrap();
    let mut b = boolean_difference(&mut k, first, second).unwrap();
    let mut result = offset_full(&mut k, &mut b, -1.5).unwrap();
    result.refresh_cached_topology();
    result.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut result, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(brep_is_manifold_solid(&result).unwrap());
    let want = want() - PI * 2.5 * 2.5 * 3.0;
    let got = volume(&k, &mut result).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "volume {got}, closed form {want}"
    );
}

/// The whole offset of [`plate`], results not merged: the offset only stitches its faces,
/// so the hole's offset wall (r 3) and the front's (y = 1.5), which pass through each
/// other, are never cut, whether the floor and the top are rebuilt or not. That body
/// was returned as an offset: the top and floor keep loops that cross (`SmFace` test
/// #38). It is refused, naming the face whose offset's loops cross.
#[test]
fn crossing_loops_without_the_merge_are_refused() {
    let mut k = Kernel::default();
    let mut b = plate(&mut k);
    let e = unmerged_offset(&mut k, &mut b, -1.5).unwrap_err();
    // the API's own failure (1001) keeps the refusal as its cause
    let cause = e.root_cause();
    let Some(Reason::Offset(OffsetFailure::LoopsCross {
        face: Some(_),
        crossing: LoopCrossing::Unmerged,
    })) = cause.failure().map(|f| &f.reason)
    else {
        panic!("{e:?}");
    };
    assert_eq!(cause.sm_status(), Some(1027));
}

/// Control for the refusal above, and the real defect still caught beside it: the hole
/// moved to (15, 4.6), so its offset (r 3) reaches y = 1.6, 0.1 clear of the front's
/// offset (y = 1.5), and no loops cross. Unmerged, the offset is still built: a valid
/// solid of the closed form, the plate shrunk by 1.5 less the hole's offset disc.
#[test]
fn loops_that_do_not_cross_still_offset_without_the_merge() {
    let mut k = Kernel::default();
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 6.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(15.0, 4.6, -1.0), 1.5, 8.0).unwrap();
    let mut b = boolean_difference(&mut k, plate, hole).unwrap();
    let mut result = unmerged_offset(&mut k, &mut b, -1.5).unwrap();
    result.refresh_cached_topology();
    result.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut result, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(brep_is_manifold_solid(&result).unwrap());
    let want = (27.0 * 17.0 - PI * 3.0 * 3.0) * 3.0;
    let got = volume(&k, &mut result).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "volume {got}, closed form {want}"
    );
}

/// `SmApiOffsetBrepFull` with extended convex edges and results not merged.
fn unmerged_offset(k: &mut Kernel, part: &mut Brep, distance: f64) -> solidcore::Result<Brep> {
    let mut notifications = offset_fixture::Notifications::new(part);
    api_offset_brep_full(
        k,
        Some(part),
        OffsetOptions {
            distance,
            extend_convex_edges: true,
            merge_results: false,
        },
        &mut notifications.context(),
    )
}

/// Upstream's `Fillet/cyl_on_box_w_hole.smb` (`_tan_hole`, `_tan_hole2`): a box with a
/// cylinder standing on it and a through hole near a side, given in `.smb` text.
fn cyl_on_box(k: &mut Kernel, name: &str, tolerance: f64) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet")
        .join(name);
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
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    b.set_tolerance(tolerance);
    Some(b)
}

/// The `offset.offset_executive_p2` goldens cases `pe_Fillet_cyl_on_box_*_d0_*_ext` (and
/// `fuzz00035`, the first with no approximation tolerance): `DoSolidOffset` of the whole
/// part by the distance, face `face` given as the shell face, extended convex edges and
/// results not merged. `None` without the pinned upstream checkout.
struct CylOnBox {
    file: &'static str,
    face: usize,
    distance: f64,
    tolerance: f64,
    approx: f64,
    inset: bool,
}

const HOLE: (&str, f64, f64) = ("cyl_on_box_w_hole.smb", 13.1556, 0.001_777_777_811_686);
const TAN_HOLE: (&str, f64, f64) = ("cyl_on_box_w_tan_hole.smb", 11.1404, 0.001_367_821_375_529);
const TAN_HOLE2: (&str, f64, f64) = ("cyl_on_box_w_tan_hole2.smb", 10.0, 0.001_626_666_692_098);

fn cyl_on_box_case(
    (file, distance, tolerance): (&'static str, f64, f64),
    face: usize,
    inset: bool,
) -> CylOnBox {
    CylOnBox {
        file,
        face,
        distance,
        tolerance,
        approx: tolerance,
        inset,
    }
}

impl CylOnBox {
    fn label(&self) -> String {
        format!(
            "{} f{} {} approx {}",
            self.file,
            self.face,
            if self.inset { "in" } else { "out" },
            self.approx
        )
    }

    /// The offset's outcome and the executive's offset B-rep after it.
    fn run(&self) -> Option<(Kernel, solidcore::Result<Brep>, Option<Brep>)> {
        let mut k = Kernel::default();
        let mut part = cyl_on_box(&mut k, self.file, self.tolerance)?;
        let shell_face = part.face_list()[self.face];
        let mut exec =
            OffsetExecutive::new(OffsetOperation::SolidOffset, self.distance, self.approx);
        exec.set_inset(self.inset);
        exec.set_extend_convex_edges(true);
        exec.set_merge_results(false);
        exec.set_shell_faces(&[shell_face]);
        let mut notifications = offset_fixture::Notifications::new(&mut part);
        let result = exec.do_solid_offset(
            &mut k,
            Some(&mut part),
            SolidOffsetOptions::default(),
            &mut notifications.context(),
        );
        let faces = exec
            .offset_brep()
            .map(|b| b.clone_standing_for_self().unwrap());
        Some((k, result, faces))
    }
}

/// The eleven goldens cases whose loops cross with results not merged. Each is refused as
/// [`LoopCrossing::Unmerged`], and the offset faces it would have stitched (the
/// executive's offset B-rep, kept as it was when refused) fail `check_brep`'s `SmFace`
/// test #38 (Intersecting Face Loops): the stitch only joins edges, so the body it made of
/// them had that face too. Main returned that body (status 1000).
#[test]
fn upstream_holes_crossing_the_sides_unmerged_are_refused() {
    let mut cases = vec![
        cyl_on_box_case(HOLE, 0, true),
        cyl_on_box_case(HOLE, 1, true),
        cyl_on_box_case(HOLE, 2, true),
        cyl_on_box_case(HOLE, 2, false),
        cyl_on_box_case(TAN_HOLE, 0, false),
        cyl_on_box_case(TAN_HOLE, 1, false),
        cyl_on_box_case(TAN_HOLE, 2, false),
        cyl_on_box_case(TAN_HOLE2, 0, true),
        cyl_on_box_case(TAN_HOLE2, 1, true),
        cyl_on_box_case(TAN_HOLE2, 2, true),
    ];
    let mut fuzz = cyl_on_box_case(HOLE, 0, true);
    fuzz.approx = 0.0;
    cases.push(fuzz);
    for case in &cases {
        let Some((mut k, result, faces)) = case.run() else {
            return;
        };
        let label = case.label();
        let e = result.err().unwrap_or_else(|| panic!("{label}: built"));
        let Some(Reason::Offset(OffsetFailure::LoopsCross {
            face: Some(_),
            crossing: LoopCrossing::Unmerged,
        })) = e.root_cause().failure().map(|f| &f.reason)
        else {
            panic!("{label}: {e:?}");
        };
        let mut faces = faces.unwrap_or_else(|| panic!("{label}: no offset faces"));
        faces.use_arena_naming().unwrap();
        let lines = check_brep(&mut k, &mut faces, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(
            lines.iter().any(|l| l.contains("Test #38")),
            "{label}: {lines:?}"
        );
    }
}

/// The real defect still caught beside the refusals above, and the cases next to them
/// still built: the other eight configurations of the same three parts, whose loops do
/// not cross, offset unmerged as before into valid manifold solids (`check_brep` Level 2
/// clean) of the volumes main gives them, each moved by at most the old integrator's leak
/// through the tolerant edges' gaps that `brep_volume` now closes (#16735).
#[test]
fn upstream_holes_clear_of_the_sides_still_offset_unmerged() {
    let cases = [
        (cyl_on_box_case(HOLE, 0, false), 7_290_094.858_016_346),
        (cyl_on_box_case(HOLE, 1, false), 7_561_236.426_576_405),
        (cyl_on_box_case(TAN_HOLE, 0, true), 859_170.668_806_394_1),
        (cyl_on_box_case(TAN_HOLE, 1, true), 804_072.097_376_684_1),
        (cyl_on_box_case(TAN_HOLE, 2, true), 701_583.266_256_480_5),
        (cyl_on_box_case(TAN_HOLE2, 0, false), 5_075_909.523_810_437),
        (cyl_on_box_case(TAN_HOLE2, 1, false), 5_213_791.753_009_426),
        (cyl_on_box_case(TAN_HOLE2, 2, false), 5_314_487.782_083_446),
    ];
    for (case, main_volume) in &cases {
        let Some((mut k, result, _)) = case.run() else {
            return;
        };
        let label = case.label();
        let mut b = result.unwrap_or_else(|e| panic!("{label}: {e:?}"));
        b.use_arena_naming().unwrap();
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{label}: {lines:?}");
        assert!(brep_is_manifold_solid(&b).unwrap(), "{label}");
        let v = volume(&k, &mut b).unwrap();
        eprintln!("{label}: volume {v}, main {main_volume}");
        volume_leak::assert_moved_by_the_old_leak(&k, &mut b, *main_volume, v, 1e-6);
    }
}

/// An open sheet bent in an L: the floor z = 0 (x 0..20) and the wall x = 20 (z 0..10),
/// both 10 deep in y, the floor with an r 1 through hole about (`hole_x`, 5).
fn l_sheet(k: &mut Kernel, hole_x: f64) -> Brep {
    let floor = Curve::from(
        create_line_segment(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 0.0, 0.0)).unwrap(),
    );
    let wall = Curve::from(
        create_line_segment(Point3::new(20.0, 0.0, 0.0), Point3::new(20.0, 0.0, 10.0)).unwrap(),
    );
    let sheet = create_linear_sweep(
        k,
        &[Some(&floor), Some(&wall)],
        Vec3::new(0.0, 1.0, 0.0),
        10.0,
        false,
    )
    .unwrap();
    let hole = create_cylinder(k, Point3::new(hole_x, 5.0, -3.0), 1.0, 6.0).unwrap();
    boolean_difference(k, sheet, hole).unwrap()
}

/// The L sheet's hole 0.5 from the bend (about (18.5, 5)), offset 2 into the L. The
/// floor's offset (z = 2) ends where it meets the wall's (x = 18), so the hole's loop
/// crosses the floor offset's outer loop. With results not merged it is refused as
/// [`LoopCrossing::Unmerged`], as every unmerged offset whose face's loops cross is: body
/// type does not tell whether two walls pass through each other there (an open sheet with
/// a hole through a face of its own has the same two walls as a solid). Merged, it is
/// built: `check_brep` Level 2 clean, of the closed-form area, the floor 18 x 10 less the
/// hole's segment beyond x = 18 (chord 0.5 from the centre), plus the wall 8 x 10.
#[test]
fn an_open_sheet_with_crossing_loops_offsets_only_merged() {
    let offset = |k: &mut Kernel, merge_results: bool| {
        let mut sheet = l_sheet(k, 18.5);
        let mut notifications = offset_fixture::Notifications::new(&mut sheet);
        api_offset_brep_full(
            k,
            Some(&mut sheet),
            OffsetOptions {
                distance: 2.0,
                extend_convex_edges: true,
                merge_results,
            },
            &mut notifications.context(),
        )
    };
    let mut k = Kernel::default();
    let e = offset(&mut k, false).unwrap_err();
    let cause = e.root_cause();
    let Some(Reason::Offset(OffsetFailure::LoopsCross {
        face: Some(_),
        crossing: LoopCrossing::Unmerged,
    })) = cause.failure().map(|f| &f.reason)
    else {
        panic!("{e:?}");
    };
    assert_eq!(cause.sm_status(), Some(1027));

    let segment = (0.5_f64).acos() - 0.5 * (1.0_f64 - 0.25).sqrt();
    let want = 18.0 * 10.0 - segment + 8.0 * 10.0;
    let mut k = Kernel::default();
    let mut result = offset(&mut k, true).unwrap();
    result.refresh_cached_topology();
    result.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut result, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let got = brep_area(&k, &mut result, 1e-9).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "area {got}, closed form {want}"
    );
}

/// The same sheet thickened by 2 into the L (`create_offset_solid`), results not merged:
/// refused as [`LoopCrossing::Unmerged`] like its plain offset (the same refusal, no branch
/// of its own). The hole moved to (10, 5), well clear of the bend, it thickens unmerged into
/// a valid manifold solid of the closed-form volume: the L's two slabs (20 x 10 x 2 and
/// 2 x 10 x 8) less the hole's r 1 x 2 cylinder.
#[test]
fn a_thickened_sheet_with_crossing_loops_unmerged_is_refused() {
    let thicken = |k: &mut Kernel, hole_x: f64| {
        let sheet = l_sheet(k, hole_x);
        let options = ShellOptions {
            offset: OffsetOptions {
                distance: 2.0,
                extend_convex_edges: true,
                merge_results: false,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: false,
        };
        shell_brep_simple(k, &sheet, options, &[])
    };
    let mut k = Kernel::default();
    let e = thicken(&mut k, 18.5).unwrap_err();
    let cause = e.root_cause();
    let Some(Reason::Offset(OffsetFailure::LoopsCross {
        face: Some(_),
        crossing: LoopCrossing::Unmerged,
    })) = cause.failure().map(|f| &f.reason)
    else {
        panic!("{e:?}");
    };
    assert_eq!(cause.sm_status(), Some(1027));

    let mut k = Kernel::default();
    let mut solid = thicken(&mut k, 10.0).unwrap();
    solid.refresh_cached_topology();
    solid.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut solid, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(brep_is_manifold_solid(&solid).unwrap());
    let want = 20.0 * 10.0 * 2.0 + 2.0 * 10.0 * 8.0 - PI * 2.0;
    let got = volume(&k, &mut solid).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "volume {got}, closed form {want}"
    );
}

/// solidcraft#18278: a Ø10 x 10 cylinder with three Ø1 x 4 blind holes in its top on a circle
/// of radius 1.5, shelled closed with a 0.95 wall. The holes' offsets (r 1.45) overlap their
/// neighbours but do not reach the middle (1.45 < 1.5), so on the void's ceiling (z = 9.05)
/// the stretches of the three offsets facing the middle close round an island. That loop
/// was made a hole of the face around the ring, wound the outer way: the shell was built
/// and refused by the check ("Face Loop has bad orientation"). The island is a face of its
/// own, and the result a valid solid of the closed form.
///
/// Closed form: the body less the void, a cylinder r 4.05 from z = 0.95 to 9.05 less the
/// three offset discs (pairwise lenses only: none covers the middle) from z = 5.05 up.
#[test]
fn a_ring_of_holes_whose_offsets_close_round_an_island_shells_closed() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap();
    let centres = [
        (1.5, 0.0),
        (-0.75, 1.299_038_105_676_658),
        (-0.75, -1.299_038_105_676_658),
    ];
    for (x, y) in centres {
        let hole = create_cylinder(&mut k, Point3::new(x, y, 6.0), 0.5, 4.5).unwrap();
        b = boolean_difference(&mut k, b, hole).unwrap();
    }
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -0.95,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    let mut result =
        solidcore::offset::thin_features::shell_brep(&mut k, &b, options, &[]).unwrap();
    result.refresh_cached_topology();
    result.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut result, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(brep_is_manifold_solid(&result).unwrap());
    let r: f64 = 1.45;
    let d = 1.5 * 3.0_f64.sqrt(); // between two hole centres
    let lens = 2.0 * r * r * (d / (2.0 * r)).acos() - 0.5 * d * (4.0 * r * r - d * d).sqrt();
    let discs = 3.0 * PI * r * r - 3.0 * lens;
    let void = PI * 4.05 * 4.05 * 8.1 - discs * 4.0;
    let body = PI * 25.0 * 10.0 - 3.0 * PI * 0.25 * 4.0;
    let want = body - void;
    let got = volume(&k, &mut result).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "volume {got}, closed form {want}"
    );
}

/// [`a_ring_of_holes_whose_offsets_close_round_an_island_shells_closed`] on a Ø4.3 cylinder:
/// the holes' offsets (r 1.45) now cover the whole of the ceiling's outer loop (r 1.2), so no
/// stretch of it is left, and only the island stands. It was not built (no face round it to
/// hold it), and the shell was refused as not a manifold solid. The island is a face of its
/// own all the same.
///
/// Closed form: the body less the void, a cylinder r 1.2 from z = 0.95 to 5.05 and, from there
/// to 9.05, the island: the triangle of the three offsets' inner crossings less the segment
/// each offset cuts off it.
#[test]
fn a_ring_of_holes_whose_offsets_cover_the_outer_loop_keeps_the_island() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 2.15, 10.0).unwrap();
    let centres = [
        (1.5, 0.0),
        (-0.75, 1.299_038_105_676_658),
        (-0.75, -1.299_038_105_676_658),
    ];
    for (x, y) in centres {
        let hole = create_cylinder(&mut k, Point3::new(x, y, 6.0), 0.5, 4.5).unwrap();
        b = boolean_difference(&mut k, b, hole).unwrap();
    }
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -0.95,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    let mut result =
        solidcore::offset::thin_features::shell_brep(&mut k, &b, options, &[]).unwrap();
    result.refresh_cached_topology();
    result.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut result, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(brep_is_manifold_solid(&result).unwrap());
    let r: f64 = 1.45;
    let d = 1.5 * 3.0_f64.sqrt(); // between two hole centres
                                  // the inner crossing of two offsets, from the axis, and the side between two crossings
    let tip = 0.75 - (r * r - 0.25 * d * d).sqrt();
    let side = tip * 3.0_f64.sqrt();
    let angle = 2.0 * (0.5 * side / r).asin();
    let island = 0.25 * 3.0_f64.sqrt() * side * side - 1.5 * r * r * (angle - angle.sin());
    let void = PI * 1.2 * 1.2 * 4.1 + island * 4.0;
    let body = PI * 2.15 * 2.15 * 10.0 - 3.0 * PI * 0.25 * 4.0;
    let want = body - void;
    let got = volume(&k, &mut result).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "volume {got}, closed form {want}"
    );
}
