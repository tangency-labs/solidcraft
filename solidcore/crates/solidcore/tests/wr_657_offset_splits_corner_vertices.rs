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
use solidcore::{Brep, FaceId, Kernel, Point3, Vec3};

/// Relative tolerance of the volumes (the port lands within 1e-12 of each).
const REL: f64 = 1e-9;

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
    for (d, want) in [(1.0, 8238.532227977406), (-1.0, 3979.9694554143525)] {
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
    // open at the wall at x = 30 (1 each way) and at the corner's triangle (1 outward)
    let mut k = Kernel::default();
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
    let mut k = Kernel::default();
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
    let mut k = Kernel::default();
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
        let mut k = Kernel::default();
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
/// only (the body is no manifold solid): the default four-face split detects a
/// vanishing edge before the final manifold-result guard, and refuses it by name.
#[test]
fn wr_657_fully_chamfered_box_inset_open_at_a_chamfer_is_refused() {
    let mut k = Kernel::default();
    let b = chamfered(&mut k, None);
    let open = face_at(&k, &b, [29.0, 10.0, 9.0]);
    match shell(&mut k, b, -1.0, &[open]) {
        Err(e) => assert_eq!(e.sm_status(), Some(1043)),
        other => panic!("SM_ERR_OFFSET_EDGE_VANISHES expected: {other:?}"),
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
