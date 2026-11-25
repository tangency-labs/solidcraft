//! Regression test for WR-657: two chamfers meeting at a box's corner leave a vertex of
//! four plane faces whose offset planes no longer meet in one point. The offset of each
//! edge there ends at the same pair of the planes' meeting points whichever way the
//! corner splits, so an inset, or a shell open at one of the four faces, gets loop gaps
//! and no volume (or the merge refuses it). The corner becomes the edge the offset
//! needs (the default path since #2981).
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
use solidcore::error::offset::{OffsetFailure, Unsoundness};
use solidcore::error::Reason;
use solidcore::geom::attribute::AttributeStore;
use solidcore::mass::integrator::properties::face_area;
use solidcore::nurbs::knots::Side;
use solidcore::offset::offset_executive::p2::{
    api_offset_brep_full, local_operation_simple, OffsetOptions,
};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2 as bp2;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p3 as eu3;
use solidcore::topology::integrity;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, FaceId, Kernel, Point3, SolidError, Vec3};

/// Relative tolerance of the volumes (the port lands within 1e-12 of each).
const REL: f64 = 1e-9;

// The closed forms of the bodies whose corners are mitres (three chamfers meeting), each
// the volume of the convex polytope of the faces' half-spaces moved by the offset, an open
// face kept (`wr_657_closed_forms_replay` recomputes each apart from the kernel).

/// Edges 0, 9 and 8 chamfered, offset 1.
const THREE_OFFSET_OUT: f64 = 8240.319371877506;
/// Edges 0, 9 and 8 chamfered, offset -1.
const THREE_OFFSET_IN: f64 = 3980.1213203435586;
/// Edges 0, 9 and 8 chamfered, shelled 1 inward open at the wall at x = 30.
const THREE_SHELL_IN_X30: f64 = 1807.9006853824353;
/// Edges 0, 9 and 8 chamfered, shelled 1 outward open at the wall at x = 30.
const THREE_SHELL_OUT_X30: f64 = 2156.802272360098;
/// Edges 0, 9 and 8 chamfered, shelled 1 outward open at edge 8's chamfer.
const THREE_SHELL_OUT_C8: f64 = 2306.5861949340533;
/// Every edge chamfered, offset 1.
const ALL_OFFSET_OUT: f64 = 7669.145453154059;
/// Every edge chamfered, offset -1.
const ALL_OFFSET_IN: f64 = 3832.9705627484773;
/// Every edge chamfered, shelled 1 outward open at the wall at x = 30.
const ALL_SHELL_OUT_X30: f64 = 1961.2435773258048;
/// Every edge chamfered, shelled 1 inward open at the wall at x = 30.
const ALL_SHELL_IN_X30: f64 = 1675.5783549120974;
/// Every edge chamfered, shelled 1 inward open at the top (z = 10).
const ALL_SHELL_IN_Z10: f64 = 1392.1468974070212;
/// Every edge chamfered, shelled 1 outward open at the chamfer along the top at x = 30.
const ALL_SHELL_OUT_C0: f64 = 2014.992807766137;
/// Every edge chamfered, shelled 1 outward open at the upright chamfer at x = 30, y = 0.
const ALL_SHELL_OUT_C8: f64 = 2061.561350261062;

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

/// The box's top, z = 10.
fn z10(k: &Kernel, b: &Brep) -> FaceId {
    face_in_plane(k, b, [0.0, 0.0, 1.0], 10.0)
}

/// The box's wall at y = 0.
fn y0(k: &Kernel, b: &Brep) -> FaceId {
    let faces = planar_faces_at(k, b, 1, 0.0);
    assert_eq!(faces.len(), 1, "the wall at y = 0");
    faces[0]
}

/// The plane face of `b` lying in the plane `n . x = h`.
fn face_in_plane(k: &Kernel, b: &Brep, n: [f64; 3], h: f64) -> FaceId {
    let found: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            if !s.is_kind_of(Plane::TYPE).unwrap() {
                return false;
            }
            let p = s.evaluate_point(k, face.uv_domain.mid()).unwrap();
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            ((n[0] * p.x + n[1] * p.y + n[2] * p.z - h) / l).abs() < 1e-6
        })
        .collect();
    assert_eq!(found.len(), 1, "one face in the plane {n:?} . x = {h}");
    found[0]
}

/// The chamfer of edge 8 (upright at x = 30, y = 0): x - y = 28.
fn chamfer_8(k: &Kernel, b: &Brep) -> FaceId {
    face_in_plane(k, b, [1.0, -1.0, 0.0], 28.0)
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
    let mut b = b.clone_standing_for_self().unwrap();
    let check = check_brep(k, &mut b, CheckLevel::Level2).unwrap();
    !check.lines().iter().any(|l| l.contains("Gap"))
}

#[test]
fn wr_657_two_chamfers_shell_inward_open_at_a_wall() {
    // the reported case: shelled 1 inward open at the wall at x = 30
    let want = 1806.1797017478002;
    let mut k = Kernel::default();
    let b = chamfered(&mut k, Some(&[0, 9]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, -1.0, &[open]);
    assert!(report(
        &k,
        "wr_657 two chamfers shell -1 x30",
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
        let mut k = Kernel::default();
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
    let mut k = Kernel::default();
    let b = chamfered(&mut k, Some(&[0, 9]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, 1.0, &[open]).unwrap();
    assert!(
        no_loop_gaps(&mut k, &result),
        "loop gaps on the default path"
    );
    assert!(report(
        &k,
        "wr_657 two chamfers shell +1 x30",
        Ok(result),
        Some(want),
        REL
    ));
}

#[test]
fn wr_657_three_chamfers_offsets() {
    for (d, want) in [(1.0, THREE_OFFSET_OUT), (-1.0, THREE_OFFSET_IN)] {
        let mut k = Kernel::default();
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
}

#[test]
fn wr_657_three_chamfers_shells() {
    // open at the wall at x = 30 (1 each way) and at edge 8's chamfer, which runs into the
    // mitre corner (1 outward)
    let mut k = Kernel::default();
    let b = chamfered(&mut k, Some(&[0, 9, 8]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, -1.0, &[open]);
    assert!(report(
        &k,
        "wr_657 three chamfers shell -1 x30",
        result,
        Some(THREE_SHELL_IN_X30),
        REL
    ));
    let mut k = Kernel::default();
    let b = chamfered(&mut k, Some(&[0, 9, 8]));
    let open = x30(&k, &b);
    let result = shell(&mut k, b, 1.0, &[open]);
    assert!(report(
        &k,
        "wr_657 three chamfers shell +1 x30",
        result,
        Some(THREE_SHELL_OUT_X30),
        REL
    ));
    let mut k = Kernel::default();
    let b = chamfered(&mut k, Some(&[0, 9, 8]));
    let open = chamfer_8(&k, &b);
    let result = shell(&mut k, b, 1.0, &[open]);
    assert!(report(
        &k,
        "wr_657 three chamfers shell +1 chamfer 8",
        result,
        Some(THREE_SHELL_OUT_C8),
        REL
    ));
}

#[test]
fn wr_657_fully_chamfered_box() {
    for (d, want) in [(1.0, ALL_OFFSET_OUT), (-1.0, ALL_OFFSET_IN)] {
        let mut k = Kernel::default();
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
    // open at the wall at x = 30 each way, the top inward, and outward at the chamfer along
    // the top at x = 30 and the upright chamfer at x = 30, y = 0, both into a mitre corner
    // (the inset open at a chamfer is refused below)
    let cases: [(OpenFace, f64, f64); 5] = [
        (x30, 1.0, ALL_SHELL_OUT_X30),
        (x30, -1.0, ALL_SHELL_IN_X30),
        (z10, -1.0, ALL_SHELL_IN_Z10),
        (chamfer_0, 1.0, ALL_SHELL_OUT_C0),
        (chamfer_8, 1.0, ALL_SHELL_OUT_C8),
    ];
    for (i, (open, d, want)) in cases.into_iter().enumerate() {
        let mut k = Kernel::default();
        let b = chamfered(&mut k, None);
        let open = open(&k, &b);
        let result = shell(&mut k, b, d, &[open]);
        assert!(report(
            &k,
            &format!("wr_657 chamfered box shell {d} case {i}"),
            result,
            Some(want),
            REL
        ));
    }
}

/// An inset of a chamfer's width leaves the cavity meeting the open chamfer along a line
/// only (the body is no manifold solid; `wr_657_closed_forms_replay` shows the line): the
/// manifold-result guard refuses it by name. (With the old triangular corner facets the
/// four-face split met a vanishing edge first, `SM_ERR_OFFSET_EDGE_VANISHES`; at the mitre
/// corners there is no vertex of four faces.)
#[test]
fn wr_657_fully_chamfered_box_inset_open_at_a_chamfer_is_refused() {
    let mut k = Kernel::default();
    let b = chamfered(&mut k, None);
    let open = face_at(&k, &b, [29.0, 10.0, 9.0]);
    match shell(&mut k, b, -1.0, &[open]) {
        Err(SolidError::Failed(failure)) => assert_eq!(
            failure.reason,
            Reason::Offset(OffsetFailure::Unsound {
                what: Unsoundness::NotManifoldSolid
            })
        ),
        other => panic!("a refusal as no manifold solid expected: {other:?}"),
    }
}

/// An offset asked not to merge leaves its overlapping faces (`OffsetBrep`, `bDoSelfInt`), so a
/// result that is no closed solid is what was asked for: a 10 mm cube inset 1, convex edges
/// not extended, still comes back with faces on the default split path.
#[test]
fn wr_657_unmerged_offset_is_not_refused() {
    let faces = || {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let mut n = Notifications::new(&mut b);
        let options = OffsetOptions {
            distance: -1.0,
            extend_convex_edges: false,
            merge_results: false,
        };
        api_offset_brep_full(&mut k, Some(&mut b), options, &mut n.context())
            .unwrap_or_else(|e| panic!("{e:?}"))
            .face_list()
            .len()
    };
    assert!(faces() > 0);
}

/// #2092: a sphere of radius 10 offset by -10 (convex edges not extended) leaves nothing.
/// The splits are the only path since #2981: an empty body still comes back as itself, no
/// broken-solid refusal on a result with no faces.
#[test]
fn issue_2092_sphere_offset_to_its_centre_is_empty() {
    let mut k = Kernel::default();
    let mut ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let mut n = Notifications::new(&mut ball);
    let options = OffsetOptions {
        distance: -10.0,
        extend_convex_edges: false,
        merge_results: true,
    };
    let out = api_offset_brep_full(&mut k, Some(&mut ball), options, &mut n.context())
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(out.face_list().is_empty(), "no faces");
}

/// The result's validity, as no first-pass artefact: the first L0, L1 and L2 checks each on
/// an independent unchecked copy of `b`, then three L2 checks on one unchecked copy (the
/// first check makes that copy's face UV trims, the later ones grade them). The body `b`
/// itself is never checked, so checking primes no copy but the ones made here.
fn assert_checks(k: &mut Kernel, b: &Brep) {
    for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
        let mut fresh = b.clone_standing_for_self().unwrap();
        let check = check_brep(k, &mut fresh, level).unwrap();
        assert!(check.is_valid(), "{level:?}: {:?}", check.lines());
    }
    let mut repeated = b.clone_standing_for_self().unwrap();
    for pass in 0..3 {
        let check = check_brep(k, &mut repeated, CheckLevel::Level2).unwrap();
        assert!(
            check.is_valid(),
            "L2 pass {}: {:?}",
            pass + 1,
            check.lines()
        );
    }
}

/// The distance of `p` (in the plane z = `height`) to the circle r 5 about the origin at
/// that height: a full turn's translated boundary.
fn off_the_translated_circle(p: Point3, height: f64) -> f64 {
    assert!((p.z - height).abs() <= 1e-9, "{p:?} leaves the plane");
    ((p.x * p.x + p.y * p.y).sqrt() - 5.0).abs()
}

/// The `topology.brep_local_operation` fixture `disk.smb` (case `gen0001`'s input): one
/// lamina face bounded by the rational quadratic upper semicircle r 5 about the origin
/// and its diameter, in the plane z = 0.
fn disk_sheet(k: &mut Kernel) -> Option<Brep> {
    sheet(k, "disk.smb")
}

/// The named upstream sheet fixture (`upstream/TestFiles/pt_TestFiles`).
fn sheet(k: &mut Kernel, file: &str) -> Option<Brep> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("upstream/TestFiles/pt_TestFiles").join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
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
    let mut b = read_brep_from_file(k, Some(bytes), true, false, &mut env).unwrap();
    b.refresh_cached_topology();
    Some(b)
}

/// The plane `distance` along the face's own normal, on one of three placements of the
/// same geometric plane: 0, `Plane::from_normal`'s canonical frame (the difftest
/// runner's: axes +y and -x); 1, the canonical X and Y axes; 2, that frame with its
/// origin shifted by the exact in-plane vector 3x + 4y. All three describe one plane.
fn moved_plane(k: &Kernel, b: &Brep, face: FaceId, distance: f64, frame: usize) -> Surface {
    let record = b.face(face).unwrap();
    let own = record.surface.as_ref().unwrap();
    let uv = record.uv_domain.evaluate(0.5, 0.5);
    let p = own.evaluate_point(k, uv).unwrap();
    let n = own.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
    let origin = Point3::new(
        p.x + n.x * distance,
        p.y + n.y * distance,
        p.z + n.z * distance,
    );
    let reference = Plane::from_normal(origin, n);
    if frame == 0 {
        return Surface::from(reference);
    }
    // the same plane, the same way round, on other axes: the disk's own normal is +-z, so
    // (X, Y) spans +z and (Y, X) -z
    assert!(
        n.x.abs() <= 1e-12 && n.y.abs() <= 1e-12,
        "the disk's normal is +-z"
    );
    let (x, y) = if n.z > 0.0 {
        (Vec3::X, Vec3::Y)
    } else {
        (Vec3::Y, Vec3::X)
    };
    let origin = if frame == 1 {
        origin
    } else {
        origin + x * 3.0 + y * 4.0
    };
    Surface::from(Plane::new(
        origin,
        x,
        y,
        reference.uv_scale(),
        &reference.step_uv_domain(),
    ))
}

/// The result's boundary points: every edge use's 3D point at the normalized parameters
/// `k / 8`, read from the body as the operation returned it. The callers' checks run on
/// copies of the body, so the body read here has not been graded and carries no UV trim
/// the checks would have made; this sampling makes none either
/// (`ok_to_make_uv_trim_curve` false).
fn boundary_points(k: &mut Kernel, b: &mut Brep) -> Vec<Point3> {
    let mut out = Vec::new();
    for u in bp2::edgeuses(b, None).unwrap() {
        for i in 0..=8 {
            let t = f64::from(i) / 8.0;
            out.push(
                eu3::normalized_evaluate(k, b, u, t, false, false, false)
                    .unwrap()
                    .point,
            );
        }
    }
    out
}

/// The distance from `p` (which must lie in the plane z = `height`) to the semicircle r 5
/// about the origin and its diameter, both at that height: the disk face's boundary
/// translated along its normal.
fn off_the_translated_disk_boundary(p: Point3, height: f64) -> f64 {
    assert!((p.z - height).abs() <= 1e-9, "{p:?} leaves the plane");
    let (x, y) = (p.x, p.y);
    let arc = if y >= 0.0 {
        ((x * x + y * y).sqrt() - 5.0).abs()
    } else {
        // below the diameter: the nearest end of the semicircle
        ((x - 5.0).powi(2) + y * y)
            .sqrt()
            .min(((x + 5.0).powi(2) + y * y).sqrt())
    };
    let dx = (x.abs() - 5.0).max(0.0);
    arc.min((dx * dx + y * y).sqrt())
}

/// #2981: `disk.smb`'s lone lamina face moved onto a parallel plane comes back as the
/// face's own boundary translated along the normal (the rational semicircle and diameter
/// at the plane's height), on any placement of that plane, and stays valid on repeated
/// checks. A plane's canonical frame (`Plane::from_normal`) is not the face's, and lifting
/// the face's UV trims through it turned the boundary about the normal: the difftest's
/// `gen0001` came back as the mapped circle with three bridge segments, and failed its
/// second check. The rational control points have positive weights, so their convex hull
/// bounds the translated boundary (x in [-5, 5], y in [0, 5]) as a whole curve.
#[test]
fn disk_sheet_parallel_plane_move_keeps_its_boundary() {
    const HEIGHT: f64 = 0.9196834024495337;
    let mut runs: Vec<Vec<Point3>> = Vec::new();
    for frame in 0..3 {
        let mut k = Kernel::default();
        let Some(mut b) = disk_sheet(&mut k) else {
            return;
        };
        assert_eq!(b.face_list().len(), 1, "frame {frame}: one lamina face");
        let face = b.face_list()[0];
        let plane = moved_plane(&k, &b, face, HEIGHT, frame);
        local_operation_simple(&mut k, &mut b, &[face], vec![plane]).unwrap();
        // the same loop, not a mapped copy with bridge edges: the semicircle and diameter
        assert_eq!(b.face_list().len(), 1, "frame {frame}: one face");
        assert_eq!(
            b.edge_list().len(),
            2,
            "frame {frame}: the arc and diameter"
        );
        assert_eq!(
            b.vertex_list().len(),
            2,
            "frame {frame}: the arc's two ends"
        );
        assert_eq!(
            bp2::edgeuses(&b, None).unwrap().len(),
            4,
            "frame {frame}: two edges on each of the lamina face's two sides"
        );
        // the body the operation returned links up as it stands; then validity is no
        // first-pass artefact (assert_checks reads copies, leaving this body untouched)
        integrity::check(&b).unwrap();
        assert_checks(&mut k, &b);
        let points = boundary_points(&mut k, &mut b);
        let mut worst = 0.0_f64;
        for &p in &points {
            assert!(
                (-5.0 - 1e-9..=5.0 + 1e-9).contains(&p.x) && (-1e-9..=5.0 + 1e-9).contains(&p.y),
                "frame {frame}: {p:?} outside the translated control hull"
            );
            worst = worst.max(off_the_translated_disk_boundary(p, HEIGHT));
        }
        assert!(worst <= 1e-9, "frame {frame}: boundary off by {worst}");
        for &v in b.vertex_list() {
            let p = b.vertex(v).unwrap().point;
            assert!(
                (p.x.abs() - 5.0).abs() <= 1e-9 && p.y.abs() <= 1e-9,
                "frame {frame}: {p:?} is not a translated arc end"
            );
            assert!(
                off_the_translated_disk_boundary(p, HEIGHT) <= 1e-9,
                "frame {frame}: vertex {p:?} off the translated boundary"
            );
        }
        runs.push(points);
    }
    // the three placements describe one plane: the boundary may not depend on which came
    for (i, a) in runs.iter().enumerate() {
        for (j, c) in runs.iter().enumerate().skip(i + 1) {
            assert_eq!(a.len(), c.len(), "frames {i} and {j}: sample counts");
            let worst = a
                .iter()
                .zip(c)
                .map(|(p, q)| (*p - *q).length())
                .fold(0.0_f64, f64::max);
            assert!(worst <= 1e-12, "frames {i} and {j} differ by {worst}");
        }
    }
}

/// #2981: `plane_circ.smb`'s full circle (one closed edge, seam vertex (0, 5, 0), the
/// face's own map `(u, v) -> (v, u + 5, 0)`) moved onto a parallel plane comes back as the
/// same circle translated along the normal, its seam at `(0, 5, z0)`. The old verbatim UV
/// lift through `Plane::from_normal`'s frame carried the *UV* circle (centre (-5, 0)) to the
/// given plane and put the seam at `(0, 0, z)`: the seam vertex is the witness, and it is
/// not a first-check artefact (the returned body is checked for integrity as it stands,
/// then the first L0/L1/L2 checks run on unchecked copies, and three L2 checks on one
/// copy, before the boundary is read).
#[test]
fn circle_sheet_parallel_plane_move_keeps_its_seam() {
    const OFFSET: f64 = 0.3239936032947275; // the difftest case gen0027
    let mut k = Kernel::default();
    let Some(mut b) = sheet(&mut k, "plane_circ.smb") else {
        return;
    };
    assert_eq!(b.face_list().len(), 1, "one lamina face");
    let face = b.face_list()[0];
    let plane = moved_plane(&k, &b, face, OFFSET, 0);
    let z0 = match plane.kind() {
        SurfaceKind::Plane(p) => p.is_planar().0.z,
        _ => panic!("the replacement is a plane"),
    };
    local_operation_simple(&mut k, &mut b, &[face], vec![plane]).unwrap();
    assert_eq!(b.face_list().len(), 1, "one face");
    assert_eq!(b.edge_list().len(), 1, "the full turn");
    assert_eq!(b.vertex_list().len(), 1, "the seam");
    // the body the operation returned links up as it stands; then validity is no
    // first-pass artefact (assert_checks reads copies, leaving this body untouched)
    integrity::check(&b).unwrap();
    assert_checks(&mut k, &b);
    let seam = b.vertex(b.vertex_list()[0]).unwrap().point;
    assert!(
        seam.distance(Point3::new(0.0, 5.0, z0)) <= 1e-9,
        "the seam is at {seam:?}, expected (0, 5, {z0})"
    );
    let points = boundary_points(&mut k, &mut b);
    let mut worst = 0.0_f64;
    for &p in &points {
        worst = worst.max(off_the_translated_circle(p, z0));
    }
    assert!(
        worst <= 1e-9,
        "the boundary is off the translated circle by {worst}"
    );
    let face = b.face_list()[0];
    let area = face_area(&k, &mut b, face, 1e-9).unwrap();
    let want = std::f64::consts::PI * 25.0;
    assert!(
        (area - want).abs() <= 1e-9 * want,
        "area {area}, expected {want}"
    );
}

/// A half-space `n . x <= h` (`n` need not be unit).
type HalfSpace = ([f64; 3], f64);

/// The volume of the bounded convex polytope `hs` cuts out, apart from the kernel: its
/// vertices are the feasible meeting points of three planes; each face's polygon, ordered
/// by angle about its centroid, has area `A_i`, and the volume is sum `h_i A_i / 3` over the
/// unit-normal planes (the cone from the origin over each face).
fn polytope_volume(hs: &[HalfSpace]) -> f64 {
    let (unit, points) = polytope_vertices(hs);
    let mut volume = 0.0;
    for &(n, h) in &unit {
        let mut face = on_plane(&points, n, h);
        if face.len() < 3 {
            continue;
        }
        let c = face.iter().fold(Vec3::new(0.0, 0.0, 0.0), |s, p| s + *p) / face.len() as f64;
        let u = face[0] - c;
        let u = u / u.length();
        let w = n.cross(u);
        let angle = |p: &Vec3| (*p - c).dot(w).atan2((*p - c).dot(u));
        face.sort_by(|p, q| angle(p).total_cmp(&angle(q)));
        let area: f64 = (0..face.len())
            .map(|i| 0.5 * (face[i] - c).cross(face[(i + 1) % face.len()] - c).dot(n))
            .sum();
        volume += h * area / 3.0;
    }
    volume
}

/// The vertices of `hs` on the unit-normal plane `n . x = h`.
fn on_plane(points: &[Vec3], n: Vec3, h: f64) -> Vec<Vec3> {
    points
        .iter()
        .copied()
        .filter(|p| (n.dot(*p) - h).abs() < 1e-9)
        .collect()
}

/// The half-spaces with unit normals, and the polytope's vertices: the feasible meeting
/// points of three planes.
fn polytope_vertices(hs: &[HalfSpace]) -> (Vec<(Vec3, f64)>, Vec<Vec3>) {
    let unit: Vec<(Vec3, f64)> = hs
        .iter()
        .map(|&(n, h)| {
            let n = Vec3::new(n[0], n[1], n[2]);
            let l = n.length();
            (n / l, h / l)
        })
        .collect();
    let mut points: Vec<Vec3> = Vec::new();
    for i in 0..unit.len() {
        for j in i + 1..unit.len() {
            for m in j + 1..unit.len() {
                let (a, ha) = unit[i];
                let (b, hb) = unit[j];
                let (c, hc) = unit[m];
                let det = a.dot(b.cross(c));
                if det.abs() < 1e-12 {
                    continue;
                }
                let p = (b.cross(c) * ha + c.cross(a) * hb + a.cross(b) * hc) / det;
                let feasible = unit.iter().all(|&(n, h)| n.dot(p) <= h + 1e-9);
                if feasible && !points.iter().any(|q| (*q - p).length() < 1e-9) {
                    points.push(p);
                }
            }
        }
    }
    (unit, points)
}

/// The faces of the 30 x 20 x 10 box by outward axis: x-, x+, y-, y+, z-, z+.
const BOX_FACES: [HalfSpace; 6] = [
    ([-1.0, 0.0, 0.0], 0.0),
    ([1.0, 0.0, 0.0], 30.0),
    ([0.0, -1.0, 0.0], 0.0),
    ([0.0, 1.0, 0.0], 20.0),
    ([0.0, 0.0, -1.0], 0.0),
    ([0.0, 0.0, 1.0], 10.0),
];

/// The box with the edges between the face pairs `edges` (indices into [`BOX_FACES`])
/// chamfered 2: each chamfer the plane through the lines 2 from the edge on both faces,
/// `(n_a + n_b) . x <= h_a + h_b - 2`. Every corner where three chamfers meet is their mitre.
fn chamfered_half_spaces(edges: &[(usize, usize)]) -> Vec<HalfSpace> {
    let mut hs = BOX_FACES.to_vec();
    for &(a, b) in edges {
        let ((na, ha), (nb, hb)) = (BOX_FACES[a], BOX_FACES[b]);
        hs.push(([na[0] + nb[0], na[1] + nb[1], na[2] + nb[2]], ha + hb - 2.0));
    }
    hs
}

/// Every face moved `d` along its outward normal but `keep` (an open face stays).
fn moved(hs: &[HalfSpace], d: f64, keep: Option<usize>) -> Vec<HalfSpace> {
    hs.iter()
        .enumerate()
        .map(|(i, &(n, h))| {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            (n, if Some(i) == keep { h } else { h + d * l })
        })
        .collect()
}

/// A shell `d` open at face `open`: the material between the body and its offset with the
/// open face kept.
fn shell_volume(hs: &[HalfSpace], d: f64, open: usize) -> f64 {
    let body = polytope_volume(hs);
    let offset = polytope_volume(&moved(hs, d, Some(open)));
    if d > 0.0 {
        offset - body
    } else {
        body - offset
    }
}

/// The closed forms behind every volume pinned above, replayed row by row against the
/// literal each test pins (to 1e-12 relative). The two-chamfer rows (whose corner is a
/// vertex of four planes) check the construction itself; the three-chamfer and fully
/// chamfered rows are the mitre corners (#18920: the chamfer-corner event is the only
/// chamfer corner, so a corner of three chamfers is the box cut by their planes).
#[test]
fn wr_657_closed_forms_replay() {
    // indices into `BOX_FACES`: x+, y-, z+
    let (xp, ym, zp) = (1, 2, 5);
    let two = chamfered_half_spaces(&[(xp, zp), (ym, zp)]);
    let three = chamfered_half_spaces(&[(xp, zp), (ym, zp), (xp, ym)]);
    let mut all_edges = Vec::new();
    for a in 0..6 {
        for b in a + 1..6 {
            if a / 2 != b / 2 {
                all_edges.push((a, b));
            }
        }
    }
    let all = chamfered_half_spaces(&all_edges);
    // the chamfers' indices in each list: the box's six faces come first
    let (two_c0, two_c9) = (6, 7);
    let three_c8 = 8;
    let all_c = |a: usize, b: usize| 6 + all_edges.iter().position(|&e| e == (a, b)).unwrap();
    let rows: Vec<(&str, f64, f64)> = vec![
        (
            "two, shell -1 x30",
            shell_volume(&two, -1.0, xp),
            1806.1797017478002,
        ),
        (
            "two, shell -1 y0",
            shell_volume(&two, -1.0, ym),
            1745.3218373715308,
        ),
        (
            "two, shell +1 y0",
            shell_volume(&two, 1.0, ym),
            2053.4784027704327,
        ),
        (
            "two, shell +1 chamfer 0",
            shell_volume(&two, 1.0, two_c0),
            2272.843698672228,
        ),
        (
            "two, shell +1 chamfer 9",
            shell_volume(&two, 1.0, two_c9),
            2226.2751561773057,
        ),
        (
            "two, shell +1 x30",
            shell_volume(&two, 1.0, xp),
            2152.6205383941633,
        ),
        (
            "three, offset +1",
            polytope_volume(&moved(&three, 1.0, None)),
            THREE_OFFSET_OUT,
        ),
        (
            "three, offset -1",
            polytope_volume(&moved(&three, -1.0, None)),
            THREE_OFFSET_IN,
        ),
        (
            "three, shell -1 x30",
            shell_volume(&three, -1.0, xp),
            THREE_SHELL_IN_X30,
        ),
        (
            "three, shell +1 x30",
            shell_volume(&three, 1.0, xp),
            THREE_SHELL_OUT_X30,
        ),
        (
            "three, shell +1 chamfer 8",
            shell_volume(&three, 1.0, three_c8),
            THREE_SHELL_OUT_C8,
        ),
        (
            "all, offset +1",
            polytope_volume(&moved(&all, 1.0, None)),
            ALL_OFFSET_OUT,
        ),
        (
            "all, offset -1",
            polytope_volume(&moved(&all, -1.0, None)),
            ALL_OFFSET_IN,
        ),
        (
            "all, shell +1 x30",
            shell_volume(&all, 1.0, xp),
            ALL_SHELL_OUT_X30,
        ),
        (
            "all, shell -1 x30",
            shell_volume(&all, -1.0, xp),
            ALL_SHELL_IN_X30,
        ),
        (
            "all, shell -1 z10",
            shell_volume(&all, -1.0, zp),
            ALL_SHELL_IN_Z10,
        ),
        (
            "all, shell +1 chamfer x30 z10",
            shell_volume(&all, 1.0, all_c(xp, zp)),
            ALL_SHELL_OUT_C0,
        ),
        (
            "all, shell +1 chamfer x30 y0",
            shell_volume(&all, 1.0, all_c(xp, ym)),
            ALL_SHELL_OUT_C8,
        ),
    ];
    for (label, closed, pinned) in rows {
        eprintln!("{label}: closed form {closed:.13} pinned {pinned:.13}");
        assert!(
            (closed - pinned).abs() <= 1e-12 * pinned,
            "{label}: {closed} vs {pinned}"
        );
    }

    // the refused inset open at the chamfer along the top at x = 30: the cavity (every face
    // moved in 1 but that chamfer) meets the chamfer's plane in a segment, not a face,
    // the line x = 29, z = 9 (`..._inset_open_at_a_chamfer_is_refused`)
    let c0 = all_c(xp, zp);
    let (unit, points) = polytope_vertices(&moved(&all, -1.0, Some(c0)));
    let (n, h) = unit[c0];
    let touch = on_plane(&points, n, h);
    assert_eq!(touch.len(), 2, "{touch:?}");
    for p in &touch {
        assert!(
            (p.x - 29.0).abs() < 1e-12 && (p.z - 9.0).abs() < 1e-12,
            "{p:?}"
        );
    }
    assert!((touch[0] - touch[1]).length() > 1.0, "{touch:?}");
}

/// #3655's rig (`fixtures/solidcraft/3655-chamfer-all-box-edges-pick-order.json`): the
/// 40 x 30 x 12.5 box with every edge chamfered 2.5 is the box cut by its twelve chamfer
/// planes, 15000 - 0.5 * 2.5^2 * 330 + 6 * 2.5^3 = 14062.5, the rig's expectation.
#[test]
fn issue_3655_mitred_box_closed_form() {
    let (a, b, c, d) = (40.0, 30.0, 12.5, 2.5);
    let faces: [HalfSpace; 6] = [
        ([-1.0, 0.0, 0.0], 0.0),
        ([1.0, 0.0, 0.0], a),
        ([0.0, -1.0, 0.0], 0.0),
        ([0.0, 1.0, 0.0], b),
        ([0.0, 0.0, -1.0], 0.0),
        ([0.0, 0.0, 1.0], c),
    ];
    let mut hs = faces.to_vec();
    for i in 0..6 {
        for j in i + 1..6 {
            if i / 2 != j / 2 {
                let ((ni, hi), (nj, hj)) = (faces[i], faces[j]);
                hs.push(([ni[0] + nj[0], ni[1] + nj[1], ni[2] + nj[2]], hi + hj - d));
            }
        }
    }
    let closed = a * b * c - 0.5 * d * d * (4.0 * (a + b + c)) + 6.0 * d * d * d;
    assert_eq!(closed, 14062.5);
    let v = polytope_volume(&hs);
    assert!((v - closed).abs() <= 1e-12 * closed, "{v} vs {closed}");
}
