//! Rounds whose fillet geometry works on an original face's own surface (upstream's
//! `pFace->GetSurface()`, which keeps its face as owner): the face classifications and
//! trimmed surface caches upstream makes through that owner run with the target B-rep
//! at hand.
//!
//! - `SmFilletEdge::CalcCrossSection` on the original face: `sm_CreateUVCurve`'s point
//!   drops classify on the face (`pFace->PointClassify`), the top edges of
//!   `filleted_box.smb` (`prog_test` `smFilletTest.cpp:1176`, difftest
//!   `big-rad-filleted_box-r*-top-edges`). Its vertical edges are rounded R1: a round of
//!   the top loop up to 1 builds, a larger one is refused (#2546, WR-739).
//! - `SmFilletEdge::CalcFilletIntSideFace` against the side face's surface:
//!   `sm_SrfSrfIntersection`'s general intersector makes the face's trimmed cache,
//!   difftest `stubs4-shell_fillet2-e1-r0.5`.
//!
//! Before the face context was wired it stopped at `Unported`
//! (`TrimSrfCache::build_tree_with_face`, `surface.trim_srf_cache`). The expected outcome is
//! the C++ reference's (difftest compares it exactly). The filleted box's top edges r 2
//! (`big-rad-filleted_box-r2.0-top-edges`), which the pinned 2x2 corner's big-radius
//! conversion built, are the blend corner's Join now, which does not build them (#2400:
//! with every fix it refuses them as the ball not fitting, naming radius 1).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p1, p10::TopologyEnv, p7};
use solidcore::topology::database_io::FileType;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel, SolidError};

/// The difftest own reader's identities: one distinct key per object.
fn key_of(e: EntityRef) -> ObjectRef {
    use EntityRef as E;
    let (type_code, kind, i) = match e {
        E::Brep(x) => (16011, 0u64, x.index()),
        E::Region(x) => (16012, 1, x.index()),
        E::Shell(x) => (16013, 2, x.index()),
        E::Faceuse(x) => (16022, 3, x.index()),
        E::Face(x) => (16002, 4, x.index()),
        E::Loopuse(x) => (16004, 5, x.index()),
        E::Loop(x) => (16003, 6, x.index()),
        E::Edgeuse(x) => (16005, 7, x.index()),
        E::Edge(x) => (16016, 8, x.index()),
        E::Vertexuse(x) => (16006, 9, x.index()),
        E::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(((kind + 1) << 40) | u64::from(i)),
    }
}

/// Reads the upstream test file and rounds `edges` (by index) with radius `r`
/// (`SmApiFilletEdges`, circular cross section): the result and the face count after
/// it. `None` without the upstream tree (CI's test job has none; the difftest job runs
/// the same cases through `blend.fillet_executive_p3`).
fn round(file: &str, r: f64, edges: &[usize]) -> Option<(Result<(), SolidError>, usize)> {
    round_body(file, r, edges).map(|(result, faces, _)| (result, faces))
}

/// Reads the upstream test file `file` (arena-named). `None` without the upstream tree
/// (CI's test job has none), failing loudly when `SOLIDCORE_REQUIRE_UPSTREAM` is set.
fn read_body(file: &str) -> Option<(Kernel, Brep)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut k = Kernel::default();
    let mut body = Brep::new_with_infinite_region().unwrap();
    let mut attributes = AttributeStore::default();
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    p7::read_from_file(&mut k, &mut body, bytes, FileType::Ascii, false, &mut env).unwrap();
    body.use_arena_naming().unwrap();
    Some((k, body))
}

/// [`round`] with the volume the round removes (`None` when it fails).
fn round_body(
    file: &str,
    r: f64,
    edges: &[usize],
) -> Option<(Result<(), SolidError>, usize, Option<f64>)> {
    let (mut k, mut body) = read_body(file)?;
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let before = brep_volume(&k, &mut body, 1e-6, &attrs).unwrap();
    let all = p1::edges(&body, None).unwrap();
    let selected: Vec<_> = edges.iter().map(|&i| all[i]).collect();
    let result = fillet_edges(&mut k, &mut body, &selected, r, 1, 1, 1.0);
    let removed = result
        .is_ok()
        .then(|| before - brep_volume(&k, &mut body, 1e-6, &attrs).unwrap());
    Some((result, p1::faces(&body, None).unwrap().len(), removed))
}

/// The top loop of `filleted_box.smb` (a 10 cube whose vertical edges are rounded R1):
/// four straight edges of length 8 and four R1 arcs.
const TOP_LOOP: [usize; 8] = [3, 6, 9, 12, 15, 17, 20, 23];

/// The volume a round of radius `r <= 1` takes off the top loop: on each straight edge the
/// section `r^2 (1 - pi/4)` along 8, on each arc that section turned a quarter about the
/// R1 axis at its centroid's radius `1 - r (10 - 3 pi) / (12 - 3 pi)` (Pappus).
fn top_loop_removed(r: f64) -> f64 {
    use std::f64::consts::PI;
    let section = r * r * (1.0 - PI / 4.0);
    let centroid = 1.0 - r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    4.0 * 8.0 * section + 4.0 * (PI / 2.0) * section * centroid
}

#[test]
fn cross_section_on_the_original_face_drops_points_with_the_face() {
    for r in [0.5, 0.9, 1.0] {
        let Some((result, faces, removed)) = round_body("Fillet/filleted_box.smb", r, &TOP_LOOP)
        else {
            return;
        };
        result.unwrap();
        assert_eq!(faces, 18, "r {r}");
        let want = top_loop_removed(r);
        let removed = removed.unwrap();
        assert!((removed - want).abs() < 1e-6, "r {r}: {removed} vs {want}");
    }
}

/// #2546 (WR-739): a round of the top loop wider than the R1 rounds puts its rail on the
/// top face past each round's axis (radius 1 - r < 0): its section turned about the axis
/// would be a spindle torus through its own axis. Refused, naming 1. Upstream and the
/// port before built an 18-face body whose corner faces cross themselves (the difftest's
/// `big-rad-filleted_box-r2.0-top-edges` and `-r3.0-`, parked on WR-739). The ball's
/// regularity (`BallIsRegular`, WR-696) finds it first: the R1 rounds curve tighter than
/// the ball, which does not fit them (`BallDoesNotFit`), naming the same 1.
#[test]
fn a_round_wider_than_the_r1_rounds_is_refused_naming_1() {
    for r in [1.01, 2.0, 3.0] {
        let Some((result, _, _)) = round_body("Fillet/filleted_box.smb", r, &TOP_LOOP) else {
            return;
        };
        let Err(SolidError::FilletRefused { reason, fit, .. }) = result else {
            panic!("r {r}: {result:?}");
        };
        assert_eq!(reason, Some(NoRoomReason::BallDoesNotFit), "r {r}");
        assert!((fit.size - 1.0).abs() < 1e-12, "r {r}: {}", fit.size);
    }
}

#[test]
fn side_face_intersection_makes_the_face_trimmed_cache() {
    let Some((result, _)) = round("Fillet/shell_fillet2.smb", 0.5, &[1]) else {
        return;
    };
    // the C++ reference fails the round with SM_FAILURE (1001)
    assert!(
        result
            .as_ref()
            .err()
            .and_then(solidcore::error::upstream_status)
            == Some(1001),
        "{result:?}"
    );
}

/// Rounds of a sheet's edges (`blend.fillet_executive_p3`'s `stubs4-*` cases): a sheet's
/// edges bound no material, so no end section runs off its end face under material and
/// the cliff overflow's end-section check passes them by. They build, as they do without
/// that check (on the fork the check's `IsConcaveEdge` fails on a sheet, and the round
/// with it).
#[test]
fn sheet_rounds_build() {
    for (file, edge, faces) in [
        ("Fillet/shell_fillet.smb", 0, 4),
        ("Fillet/shell_fillet.smb", 3, 4),
        ("Fillet/shell_fillet.smb", 4, 4),
        ("Fillet/coons_01_03_29.smb", 0, 5),
        ("Fillet/test5.smb", 2, 5),
        ("Fillet/sphere_plane.smb", 3, 3),
    ] {
        let Some((mut k, mut body)) = read_body(file) else {
            return;
        };
        let before = brep_area(&k, &mut body, 1e-6).unwrap();
        let all = p1::edges(&body, None).unwrap();
        fillet_edges(&mut k, &mut body, &[all[edge]], 0.5, 1, 1, 1.0)
            .unwrap_or_else(|e| panic!("{file} e{edge}: {e:?}"));
        assert_eq!(
            p1::faces(&body, None).unwrap().len(),
            faces,
            "{file} e{edge}"
        );
        let report = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
        assert!(
            report.lines().is_empty(),
            "{file} e{edge}: {:?}",
            report.lines()
        );
        // a round of a crease turning by phi trades the two flanks it cuts away, each
        // r tan(phi / 2) wide, for an arc r phi wide: it always takes area off a sheet
        let after = brep_area(&k, &mut body, 1e-9).unwrap();
        assert!(after < before, "{file} e{edge}: area {before} -> {after}");
        if file == "Fillet/shell_fillet.smb" {
            let want = shell_corner_area(0.5);
            assert!(
                (after - want).abs() < 1e-6 * want,
                "e{edge}: area {after} vs {want}"
            );
        }
    }
}

/// `box_diff_cyl_big_rad.smb` (a 200 x 198.2 x 40 box less a cylinder hole of radius
/// `R` at `(52, 26.2)`, depth 29.3) with its hole's rim (edge 12) rounded r 28: the
/// round's rail runs past the box's `x = 100` face (`R + r > 48`), a cliff overflow
/// built on the faces' continuations, and the box keeps whatever of the torus band
/// lies inside it (difftest `big-radius-box_diff_cyl_big_rad-r28-edge12`).
///
/// The removed volume is the band `h(rho) = r - sqrt(r^2 - (R + r - rho)^2)` deep at
/// radius `rho` in `[R, R + r]` from the axis, over the angle `theta(rho)` of that circle
/// inside the box (`2 pi`, less `2 acos(48 / rho)` past `x = 100`). `h` and `theta`
/// both fall as `rho` grows, so on each step `[a, b]` the integrand `h theta rho` is
/// at least `h(b) theta(b) a` and at most `h(a) theta(a) b`: summed, they enclose it.
#[test]
fn a_rim_round_past_the_box_face_removes_the_band_inside_the_box() {
    use std::f64::consts::PI;
    let Some((mut k, mut body)) = read_body("Fillet/box_diff_cyl_big_rad.smb") else {
        return;
    };
    // the file's control points (homogeneous over weight)
    let (cx, cy) = (52.000_003_814_697_266_f64, 26.222_219_467_163_086_f64);
    let big_r = 76.713_672_637_939_453 - cx;
    let depth = 29.333_333_969_116_211;
    let (x0, x1, y, z) = (
        -100.0,
        100.0,
        99.111_114_501_953_125,
        40.000_007_629_394_531,
    );
    let r = 28.0;
    let c = x1 - cx;
    assert!(big_r + r > c && cy + big_r + r < y && cy - big_r - r > -y);
    assert!(cx - big_r - r > x0 && r < depth);
    let v0 = (x1 - x0) * (2.0 * y) * z - PI * big_r * big_r * depth;
    let h = |rho: f64| {
        let t = big_r + r - rho;
        r - (r * r - t * t).max(0.0).sqrt()
    };
    let theta = |rho: f64| {
        if rho <= c {
            2.0 * PI
        } else {
            2.0 * PI - 2.0 * (c / rho).acos()
        }
    };
    let n = 2_000_000;
    let w = r / f64::from(n);
    let (mut lo, mut hi, mut a) = (0.0, 0.0, big_r);
    for i in 1..=n {
        let b = big_r + f64::from(i) * w;
        lo += h(b) * theta(b) * a * w;
        hi += h(a) * theta(a) * b * w;
        a = b;
    }
    // the rounding of 2e6 sums (each well under 1e-12 relative) is far below this
    let margin = 1e-3;
    let attrs_store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &attrs_store,
        key_of: &|_| ObjectKey(0),
    };
    let all = p1::edges(&body, None).unwrap();
    fillet_edges(&mut k, &mut body, &[all[12]], r, 1, 1, 1.0).unwrap();
    let report = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
    assert!(report.lines().is_empty(), "{:?}", report.lines());
    let v = brep_volume(&k, &mut body, 1e-9, &attrs).unwrap();
    let (v_lo, v_hi) = (v0 - hi - margin, v0 - lo + margin);
    assert!(v_lo <= v && v <= v_hi, "V {v} outside [{v_lo}, {v_hi}]");
}

/// The area of `shell_fillet.smb` (three 10 x 10 squares meeting square at a corner)
/// after a round of radius `r <= 10` of one of the edges at the corner: the two flanks
/// lose a strip `r` wide along the edge, the quarter cylinder `pi r / 2 x 10` comes in,
/// and the third square, across the round's end, loses the spandrel `r^2 (1 - pi / 4)`
/// between the corner and the arc.
fn shell_corner_area(r: f64) -> f64 {
    use std::f64::consts::PI;
    300.0 - 2.0 * 10.0 * r + PI / 2.0 * r * 10.0 - r * r * (1.0 - PI / 4.0)
}

/// A round of a sheet's edge as wide as the faces beside it (`shell_fillet.smb`): up to
/// 10 its rails stay on the faces and it builds; past 10 they leave the faces, and with no
/// material for a cliff overflow to cut it is refused, naming 10. Main stopped with "the
/// fillet body is not built" (the rail-fit judgement's `IsConcaveEdge` fails on a
/// sheet's edge, as on the fork).
#[test]
fn a_sheet_round_wider_than_its_faces_is_refused_naming_their_width() {
    for r in [5.0, 9.5] {
        let Some((mut k, mut body)) = read_body("Fillet/shell_fillet.smb") else {
            return;
        };
        let all = p1::edges(&body, None).unwrap();
        fillet_edges(&mut k, &mut body, &[all[0]], r, 1, 1, 1.0).unwrap();
        assert_eq!(p1::faces(&body, None).unwrap().len(), 4, "r {r}");
        let report = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
        assert!(report.lines().is_empty(), "r {r}: {:?}", report.lines());
        let (area, want) = (
            brep_area(&k, &mut body, 1e-9).unwrap(),
            shell_corner_area(r),
        );
        assert!(
            (area - want).abs() < 1e-6 * want,
            "r {r}: area {area} vs {want}"
        );
    }
    for r in [11.0, 12.0, 15.0] {
        let Some((mut k, mut body)) = read_body("Fillet/shell_fillet.smb") else {
            return;
        };
        let all = p1::edges(&body, None).unwrap();
        let result = fillet_edges(&mut k, &mut body, &[all[0]], r, 1, 1, 1.0);
        let Err(SolidError::FilletRefused { fit, .. }) = result else {
            panic!("r {r}: {result:?}");
        };
        assert!((fit.size - 10.0).abs() < 1e-9, "r {r}: {}", fit.size);
        assert_eq!(p1::faces(&body, None).unwrap().len(), 3, "r {r}");
    }
}
