//! Regression test for WR-657: two chamfers meeting at a box's corner leave a vertex of
//! four plane faces whose offset planes no longer meet in one point. The offset of each
//! edge there ends at the same pair of the planes' meeting points whichever way the
//! corner splits, so an inset, or a shell open at one of the four faces, gets loop gaps
//! and no volume (or the merge refuses it). With `offset_closes_loop_gaps` and
//! `offset_splits_corner_vertices` the corner becomes the edge the offset needs.
//!
//! A 30 x 20 x 10 box with edges 0 (the top's edge at x = 30) and 9 (the top's edge at
//! y = 0) chamfered 2 (`SmApiFilletEdges`, a linear section), with edge 8 (the upright
//! edge at x = 30, y = 0) too, and with every edge chamfered. Each body is convex: the
//! closed forms are the volumes of its faces' half-spaces moved by the offset (an open
//! face stays), computed apart from the kernel.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{offset_full, planar_faces_at, report, shell, Notifications};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::error::{SolidError, SM_ERR};
use solidcore::offset::offset_executive::p2::{api_offset_brep_full, OffsetOptions};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, FaceId, Kernel, Point3};

/// Relative tolerance of the volumes (the port lands within 1e-12 of each).
const REL: f64 = 1e-9;

fn kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.offset_closes_loop_gaps = fixed;
    k.fixes.offset_splits_corner_vertices = fixed;
    k
}

/// The box with `edges` (indices into its edge list; `None`: every edge) chamfered 2.
fn chamfered(k: &mut Kernel, edges: Option<&[usize]>) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let all = bp1::edges(&b, None).unwrap();
    let picked: Vec<_> = match edges {
        Some(picks) => picks.iter().map(|&i| all[i]).collect(),
        None => all,
    };
    fillet_edges(k, &mut b, &picked, 2.0, 0, 1, 1.0).unwrap();
    b
}

/// The face whose middle (of its UV domain) is at `at`.
fn face_at(k: &Kernel, b: &Brep, at: [f64; 3]) -> FaceId {
    let found: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let m = face
                .surface
                .as_ref()
                .unwrap()
                .evaluate_point(k, face.uv_domain.mid())
                .unwrap();
            m.distance(Point3::new(at[0], at[1], at[2])) < 1e-3
        })
        .collect();
    assert_eq!(found.len(), 1, "one face at {at:?}");
    found[0]
}

/// The box's wall at x = 30.
fn x30(k: &Kernel, b: &Brep) -> FaceId {
    let faces = planar_faces_at(k, b, 0, 30.0);
    assert_eq!(faces.len(), 1, "the wall at x = 30");
    faces[0]
}

/// Picks the face a shell leaves open.
type OpenFace = fn(&Kernel, &Brep) -> FaceId;

/// The box's wall at y = 0.
fn y0(k: &Kernel, b: &Brep) -> FaceId {
    let faces = planar_faces_at(k, b, 1, 0.0);
    assert_eq!(faces.len(), 1, "the wall at y = 0");
    faces[0]
}

/// The chamfer of edge 0 (along the top at x = 30).
fn chamfer_0(k: &Kernel, b: &Brep) -> FaceId {
    face_at(k, b, [29.0, 10.0, 9.0])
}

/// The chamfer of edge 9 (along the top at y = 0).
fn chamfer_9(k: &Kernel, b: &Brep) -> FaceId {
    face_at(k, b, [15.0, 1.0, 9.0])
}

/// No loop of `b` has a gap between consecutive edges (`check_brep`).
fn no_loop_gaps(k: &mut Kernel, b: &Brep) -> bool {
    let mut b = b.clone();
    let check = check_brep(k, &mut b, CheckLevel::Level2).unwrap();
    !check.lines().iter().any(|l| l.contains("Gap"))
}

#[test]
fn wr_657_two_chamfers_shell_inward_open_at_a_wall() {
    // the reported case: shelled 1 inward open at the wall at x = 30
    let want = 1806.1797017478002;
    let shell_x30 = |fixed: bool| {
        let mut k = kernel(fixed);
        let b = chamfered(&mut k, Some(&[0, 9]));
        let open = x30(&k, &b);
        let result = shell(&mut k, b, -1.0, &[open]);
        (k, result)
    };
    let (k, result) = shell_x30(true);
    assert!(report(
        &k,
        "wr_657 two chamfers shell -1 x30",
        result,
        Some(want),
        REL
    ));
    // upstream (the pinned C++ too): Ok, no closed solid, volume 0
    let (k, result) = shell_x30(false);
    assert!(!report(
        &k,
        "wr_657 two chamfers shell -1 x30 default",
        result,
        Some(want),
        REL
    ));
}

#[test]
fn wr_657_two_chamfers_shells_at_each_corner_face() {
    // the other faces at the corner: the wall at y = 0 and the two chamfers, each
    // shelled 1 outward; the wall at y = 0 1 inward
    let cases: [(OpenFace, f64, f64); 4] = [
        (y0, -1.0, 1745.3218373715308),
        (y0, 1.0, 2053.4784027704327),
        (chamfer_0, 1.0, 2272.843698672228),
        (chamfer_9, 1.0, 2226.2751561773057),
    ];
    for (i, (open, d, want)) in cases.into_iter().enumerate() {
        let mut k = kernel(true);
        let b = chamfered(&mut k, Some(&[0, 9]));
        let open = open(&k, &b);
        let result = shell(&mut k, b, d, &[open]);
        assert!(report(
            &k,
            &format!("wr_657 two chamfers shell {d} case {i}"),
            result,
            Some(want),
            REL
        ));
    }
}

#[test]
fn wr_657_two_chamfers_shell_outward_has_no_loop_gaps() {
    // upstream's volume is right (2152.620538, the C++ too) but two loops have a gap
    // of sqrt(2)
    let want = 2152.6205383941633;
    let mut k = kernel(true);
    let b = chamfered(&mut k, Some(&[0, 9]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, 1.0, &[open]).unwrap();
    assert!(no_loop_gaps(&mut k, &result), "loop gaps with the opt-in");
    assert!(report(
        &k,
        "wr_657 two chamfers shell +1 x30",
        Ok(result),
        Some(want),
        REL
    ));
    let mut k = kernel(false);
    let b = chamfered(&mut k, Some(&[0, 9]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, 1.0, &[open]).unwrap();
    assert!(!no_loop_gaps(&mut k, &result), "upstream leaves loop gaps");
}

#[test]
fn wr_657_three_chamfers_offsets() {
    for (d, want) in [(1.0, 8238.532227977406), (-1.0, 3979.9694554143525)] {
        let mut k = kernel(true);
        let mut b = chamfered(&mut k, Some(&[0, 9, 8]));
        let result = offset_full(&mut k, &mut b, d);
        assert!(report(
            &k,
            &format!("wr_657 three chamfers offset {d}"),
            result,
            Some(want),
            REL
        ));
    }
    // upstream: Ok with loop gaps and volume 0 at +1, refused (1001) at -1
    let mut k = kernel(false);
    let mut b = chamfered(&mut k, Some(&[0, 9, 8]));
    let result = offset_full(&mut k, &mut b, 1.0);
    assert!(!report(
        &k,
        "wr_657 three chamfers offset +1 default",
        result,
        Some(8238.532227977406),
        REL
    ));
}

#[test]
fn wr_657_three_chamfers_shells() {
    // open at the wall at x = 30 (1 each way) and at the corner's triangle (1 outward)
    let mut k = kernel(true);
    let b = chamfered(&mut k, Some(&[0, 9, 8]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, -1.0, &[open]);
    assert!(report(
        &k,
        "wr_657 three chamfers shell -1 x30",
        result,
        Some(1807.385883644974),
        REL
    ));
    let mut k = kernel(true);
    let b = chamfered(&mut k, Some(&[0, 9, 8]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, 1.0, &[open]);
    assert!(report(
        &k,
        "wr_657 three chamfers shell +1 x30",
        result,
        Some(2155.9012941491765),
        REL
    ));
    let mut k = kernel(true);
    let b = chamfered(&mut k, Some(&[0, 9, 8]));
    let open = face_at(&k, &b, [28.0 + 2.0 / 3.0, 4.0 / 3.0, 8.0 + 2.0 / 3.0]);
    let result = shell(&mut k, b, 1.0, &[open]);
    assert!(report(
        &k,
        "wr_657 three chamfers shell +1 corner",
        result,
        Some(2337.769119345813),
        REL
    ));
}

#[test]
fn wr_657_fully_chamfered_box() {
    for (d, want) in [(1.0, 7654.848301953257), (-1.0, 3831.755643314821)] {
        let mut k = kernel(true);
        let mut b = chamfered(&mut k, None);
        let result = offset_full(&mut k, &mut b, d);
        assert!(report(
            &k,
            &format!("wr_657 chamfered box offset {d}"),
            result,
            Some(want),
            REL
        ));
    }
    // open at the wall at x = 30, a corner's triangle and the chamfer along the top at
    // x = 30, each way (the inset open at a chamfer is refused below)
    let cases: [([f64; 3], f64, f64); 5] = [
        ([30.0, 10.0, 5.0], 1.0, 1953.157755548379),
        ([30.0, 10.0, 5.0], -1.0, 1671.4599410124183),
        (
            [28.0 + 2.0 / 3.0, 4.0 / 3.0, 8.0 + 2.0 / 3.0],
            1.0,
            2076.7518599883315,
        ),
        (
            [28.0 + 2.0 / 3.0, 4.0 / 3.0, 8.0 + 2.0 / 3.0],
            -1.0,
            1730.7591584226357,
        ),
        ([29.0, 10.0, 9.0], 1.0, 2009.1803204583593),
    ];
    for (at, d, want) in cases {
        let mut k = kernel(true);
        let b = chamfered(&mut k, None);
        let open = face_at(&k, &b, at);
        let result = shell(&mut k, b, d, &[open]);
        assert!(report(
            &k,
            &format!("wr_657 chamfered box shell {d} at {at:?}"),
            result,
            Some(want),
            REL
        ));
    }
}

/// An inset of a chamfer's width leaves the cavity meeting the open chamfer along a line
/// only (the body is no manifold solid): refused, not returned.
#[test]
fn wr_657_fully_chamfered_box_inset_open_at_a_chamfer_is_refused() {
    let mut k = kernel(true);
    let b = chamfered(&mut k, None);
    let open = face_at(&k, &b, [29.0, 10.0, 9.0]);
    match shell(&mut k, b, -1.0, &[open]) {
        Err(SolidError::Upstream { code, .. }) => assert_eq!(code, SM_ERR),
        other => panic!("SM_ERR expected: {other:?}"),
    }
}

/// #2092: a sphere of radius 10 offset by -10 (convex edges not extended) leaves nothing.
/// With both flags on, the empty body comes back as with either off: a result with no
/// faces is not refused as a broken solid.
#[test]
fn issue_2092_sphere_offset_to_its_centre_is_empty() {
    for flags in [(false, false), (true, false), (false, true), (true, true)] {
        let mut k = Kernel::default();
        k.fixes.offset_closes_loop_gaps = flags.0;
        k.fixes.offset_splits_corner_vertices = flags.1;
        let mut ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
        let mut n = Notifications::new(&mut ball);
        let options = OffsetOptions {
            distance: -10.0,
            extend_convex_edges: false,
            merge_results: true,
        };
        let out = api_offset_brep_full(&mut k, Some(&mut ball), options, &mut n.context())
            .unwrap_or_else(|e| panic!("flags {flags:?}: {e:?}"));
        assert!(out.face_list().is_empty(), "flags {flags:?}: no faces");
    }
}
