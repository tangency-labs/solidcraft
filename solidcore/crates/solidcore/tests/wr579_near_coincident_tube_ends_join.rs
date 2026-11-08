//! WR-579 (tangency-labs/solidcore#1517; the fork's tangency-labs/smlib@9eec159): a bicycle
//! frame's seat tube (r 16, from (0, 0, 300) towards (-150, 0, 820)) ends 3.7e-4 short of
//! where the top tube (r 16, from (-150, 0, 820) to (600, 0, 860)) starts. Where the two
//! walls touch, the seat's end circle runs within tolerance of the top tube's wall for a
//! stretch and crosses it there too: the crossing and the stretch's ends fall in one proto
//! vertex as three vertex definitions too far apart to combine, and upstream's
//! `SmProtoEdge::Resolve` keeps the stretch's edge whole past the crossing's vertex. The
//! union then fails.
//!
//! The fork builds each tube with its C API's extrude, the circle starting on the side the
//! walls touch (the top tube's seam at -y, the seat's at +y). This builds them as
//! `SmApiCreateCylinder` (seam at its frame's +X) turned about Z to put the seam there,
//! about Y onto the axis, and moved to the base, as the `boolean.api_brep` fixture
//! `issue_1517_tube_ends.json` does on both sides.
//!
//! Expected with `proto_edge_splits_at_captured_vertices`: one manifold solid, its volume
//! between the larger tube's and both together. Its check still fails on a separate
//! defect, as the fork's does: an ellipse piece 8.6e-5 long (the top tube's wall cut by the
//! seat's end plane) whose NURB lies 2.1e-5 off its ellipse (`SmEllipse` test 2;
//! `SmExtent1d::ContainsPeriodicValue`, #1518). That is the only failure allowed.
//!
//! WR-741: a plane that nearly holds a cylinder's axis meets it in an ellipse of radii 1.8e7
//! and 8, a line to 6e-11 over its 2.3e-4 degrees, whose analytic form carries rounding of
//! about `rx` times an ulp of its angle (4.6e-9); `SmEllipse`'s tests 1 and 2 held it to
//! `100 SM_EFF_ZERO (1 + |p|)` and refused exact results. The cases below are SolidCraft's
//! records of the cut (`periph_case_R`'s notched tool `t24`, a cone `e3` and the part's
//! lower slab), not rebuilt here: the verdict on this ellipse turns on its last bits.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean, boolean_union};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// A tube of radius 16 and `length` along Z (`SmApiCreateCylinder` at the origin), turned
/// `z_deg` about Z (its seam off +X) and `y_deg` about Y (its axis onto the tube's), then
/// moved to `origin`: the fixture's `moved` cylinder.
fn tube(k: &mut Kernel, origin: [f64; 3], length: f64, z_deg: f64, y_deg: f64) -> Brep {
    let mut b = create_cylinder(k, Point3::ZERO, 16.0, length).unwrap();
    let o = Point3::ZERO;
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, Vec3::Z, z_deg).unwrap();
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, Vec3::Y, y_deg).unwrap();
    api_translate(k, Some(TransformObject::Brep(&mut b)), Vec3::from(origin)).unwrap();
    b
}

/// The top tube: (-150, 0, 820) to (600, 0, 860), its seam at -y.
fn top(k: &mut Kernel) -> Brep {
    tube(
        k,
        [-150.0, 0.0, 820.0],
        751.0659092250161,
        -90.0,
        86.94711748520757,
    )
}

/// The seat tube: from (0, 0, 300) towards (-150, 0, 820), 3.7e-4 short, its seam at +y.
fn seat(k: &mut Kernel) -> Brep {
    tube(
        k,
        [0.0, 0.0, 300.0],
        541.2019951093923,
        90.0,
        -16.09081634885217,
    )
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// #1518's report, the one check failure the joined body still has: the ellipse piece
/// 8.6e-5 long (the top tube's wall cut by the seat's end plane) whose NURB lies 2.1e-5
/// off its ellipse (`SmExtent1d::ContainsPeriodicValue`, a separate defect the fork's
/// commit also leaves). The numbers are deterministic.
/// Its tolerance is upstream's point term plus the rounding the ellipse's evaluation carries
/// (WR-741).
const ELLIPSE_1518: &str = "SmEllipse::Test #2 Bad Nurb to STEP Approx: \
Dist(NurbPt, EvalSTEP(STEPInversion(NurbPt)) greater than tol \
(offset=2.1419823e-05, tolerance=8.2100185e-08) [owner SmEllipse]";

/// The union with the fix set to `fixed`: the result's volume when it is one manifold
/// solid whose check reports exactly #1518's ellipse ([`ELLIPSE_1518`]), else what went
/// wrong.
fn join(fixed: bool) -> Result<f64, String> {
    let mut k = Kernel::default();
    k.fixes.proto_edge_splits_at_captured_vertices = fixed;
    let (t, s) = (top(&mut k), seat(&mut k));
    let mut r = boolean_union(&mut k, t, s).map_err(|e| format!("union: {e:?}"))?;
    if !is_manifold_solid(&r).unwrap() {
        return Err("not a manifold solid".into());
    }
    let lines = check_brep(&mut k, &mut r, CheckLevel::Level2)
        .unwrap()
        .lines();
    println!("check (fix {fixed}): {lines:?}");
    // exactly #1518's line, once; any other report fails
    if lines != [ELLIPSE_1518] {
        return Err(format!("check: {lines:?}"));
    }
    Ok(volume(&k, &mut r))
}

#[test]
fn wr579_near_coincident_tube_ends_join() {
    let unfixed = join(false);
    println!("flag off: {unfixed:?}");
    assert!(unfixed.is_err(), "upstream's union succeeded: {unfixed:?}");

    let v = join(true).unwrap();
    let mut k = Kernel::default();
    let (mut t, mut s) = (top(&mut k), seat(&mut k));
    let (vt, vs) = (volume(&k, &mut t), volume(&k, &mut s));
    assert!(
        v > vt.max(vs) && v < vt + vs,
        "union volume {v}, tubes {vt} and {vs}"
    );
}

/// The `SmClass::Test #n` labels of the check's lines, sorted, so reports compare as
/// multisets of failed tests.
fn labels(lines: &[String]) -> Vec<String> {
    let mut v: Vec<String> = lines
        .iter()
        .map(|l| l.splitn(3, ' ').take(2).collect::<Vec<_>>().join(" "))
        .collect();
    v.sort();
    v
}

/// #1529's union (tangency-labs/solidcore#2424): the seat tube stops `gap` short of the
/// top tube's start. The fixes are the default kernel's, or every one when `all`, with
/// WR-575's `bspline_trim_keeps_rational_end_spans` set to `keeps`; returns the check's
/// labels.
fn join_with_gap(gap: f64, all: bool, keeps: bool) -> Vec<String> {
    let mut k = Kernel::default();
    if all {
        k.fixes = UpstreamFixes::all();
    }
    k.fixes.bspline_trim_keeps_rational_end_spans = keeps;
    let full = (150.0_f64 * 150.0 + 520.0 * 520.0).sqrt();
    let t = top(&mut k);
    let s = tube(
        &mut k,
        [0.0, 0.0, 300.0],
        full - gap,
        90.0,
        -16.09081634885217,
    );
    let mut r = boolean_union(&mut k, t, s).unwrap();
    assert!(
        is_manifold_solid(&r).unwrap(),
        "gap {gap}: not a manifold solid"
    );
    labels(
        &check_brep(&mut k, &mut r, CheckLevel::Level2)
            .unwrap()
            .lines(),
    )
}

/// Whether every label of `sub` is in `all`, as many times.
fn is_sub_multiset(sub: &[String], all: &[String]) -> bool {
    let mut rest = all.to_vec();
    sub.iter().all(|l| match rest.iter().position(|r| r == l) {
        Some(i) => {
            rest.remove(i);
            true
        }
        None => false,
    })
}

/// The check labels of the five reports below, sorted: `gaps` pairs of `SmBrep` gaps
/// (#0/#2), `ellipses` pairs of a circle piece's `SmEllipse` #1/#2, and one or two
/// `SmFace` #36.
fn report(gaps: usize, ellipses: usize, seams: usize) -> Vec<String> {
    let mut v = Vec::new();
    for _ in 0..gaps {
        v.extend(["SmBrep::Test #0", "SmBrep::Test #2"].map(String::from));
    }
    for _ in 0..ellipses {
        v.extend(["SmEllipse::Test #1", "SmEllipse::Test #2"].map(String::from));
    }
    v.extend(std::iter::repeat_n("SmFace::Test #36".to_string(), seams));
    v.sort();
    v
}

/// Joins #1529's tubes at `gap` with every fix on when `all`, with WR-575's flag off and
/// on: the flag-off report must be `expected_off`, the flag-on one may report no test the
/// flag-off one doesn't, and at 1e-5 it must drop the circle's `SmEllipse` #1/#2 (WR-575's
/// own case). Returns what went wrong.
fn flag_row(gap: f64, all: bool, expected_off: &[String]) -> Vec<String> {
    let mut failures = Vec::new();
    let off = join_with_gap(gap, all, false);
    let on = join_with_gap(gap, all, true);
    println!("gap {gap} all {all}: off {off:?}, on {on:?}");
    if off != expected_off {
        failures.push(format!("gap {gap} all {all}: flag off {off:?}"));
    }
    if !is_sub_multiset(&on, &off) {
        failures.push(format!(
            "gap {gap} all {all}: flag on {on:?} adds to {off:?}"
        ));
    }
    if gap == 1e-5 && on.iter().any(|x| x.starts_with("SmEllipse")) {
        failures.push(format!(
            "gap {gap} all {all}: flag on {on:?} keeps the ellipse"
        ));
    }
    failures
}

/// tangency-labs/solidcore#2424: WR-575's flag keeps a conic's short end span, and where
/// that span is a pole (#2090) the trim ends at the interior knot. When the edge is split
/// there, both pieces must end at the same parameter, and the edge's UV trims with them.
/// Expected: at every gap the flag-on check reports no test the flag-off one doesn't (no
/// `SmEdgeuse` #25/#26), and at 1e-5 it still drops the circle's `SmEllipse` #1/#2. The
/// flag-off reports are pinned as controls. Every fix on at 1e-6 is
/// [`wr575_every_fix_at_1e_6_adds_no_check_failure`].
#[test]
fn wr575_tube_ends_join_adds_no_check_failure_at_any_gap() {
    // (gap, every fix on, the flag-off report). At 1e-5 the flag-off controls changed with
    // WR-751 (#2621), `MergeIntoObject` splitting the two end circles where they pass
    // nearest their points (3.1e-6 -> 2.3e-6 and 2.8e-6 -> 2.2e-6 off): one gap pair
    // (7.77e-6 against 7.5e-6) goes, and each split now falls a few micrometres from the
    // circle's half-turn knot, so both pieces' short end spans fail `SmEllipse` #1/#2
    // (1.8e-6 and 1.2e-6; one piece, 1.5e-6, before): WR-575's defect, which its flag
    // clears, as the flag-on rows show.
    let rows = [
        (1e-5, false, report(2, 2, 2)),
        (1e-6, false, report(0, 1, 1)),
        (1e-7, false, report(0, 0, 1)),
        (1e-5, true, report(0, 2, 2)),
        (1e-7, true, report(0, 0, 1)),
    ];
    let failures: Vec<String> = rows
        .iter()
        .flat_map(|(gap, all, off)| flag_row(*gap, *all, off))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// #2424's row with every fix on at 1e-6. WR-575 keeps the ellipse's NURB exact, so
/// WR-680's exact UV trim is built for it, and the ellipse's knot falls within a pole
/// (1.4e-7) of where it crosses a joint of the cylinder circle's segments: the trim must
/// not keep the 2e-9-long span between them, which check test #15 reports as
/// `SmBSplineCurve` #15 (tangency-labs/solidcore#2427).
#[test]
fn wr575_every_fix_at_1e_6_adds_no_check_failure() {
    let failures = flag_row(1e-6, true, &report(0, 1, 1));
    assert!(failures.is_empty(), "{failures:#?}");
}

/// SolidCraft's profile, every fix on but `fillet_refuse_rounds_breaking_through` (its
/// `HELD_BACK`).
fn solidcraft() -> Kernel {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.fixes.fillet_refuse_rounds_breaking_through = false;
    k
}

/// `b` checks clean at Level 2.
fn checks_clean(k: &mut Kernel, b: &mut Brep) {
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
}

/// A body as SolidCraft records it (binary SMB), read under its naming.
fn read_notify_named(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// `t24` and `e3`'s cone as SolidCraft main 4bdffef06 (on solidcore 3df8a06b5) passes them to
/// the cut `e3`, written where its check refused the cut. `t24` is SolidCraft's 198 x 105.5 x
/// 71.5 box minus the half-space `t25` and minus the notch `t26`, a rectangle 8 wide turned
/// about its side on the clipped box's slanted face. The cone is the trapezoid (0, -55),
/// (24, -55), (19, 16.5), (0, 16.5) turned about world Z through (0, -5.8).
const TOOL: &[u8] = include_bytes!("data/issue_2539_e3_tool.smb");
const CONE: &[u8] = include_bytes!("data/issue_2539_e3_cone.smb");
/// `periph_case_R` before its last cut (SolidCraft's record, as above) cut down by solidcore to
/// its part below z = -14, where `t24` takes its corner: 18 faces, written by
/// `write_brep_to_file`. The whole part's record is 338 KB.
const PART: &[u8] = include_bytes!("data/issue_2539_periph_case_lower.smb");

fn fine_volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// `t24` itself: its slanted face meets the notch in the near-line ellipse, which checks
/// clean (upstream's tolerance refused it: `SmEllipse` test 2, 3.97e-9 against 3.81e-9).
#[test]
fn wr741_the_notched_tool_checks_clean() {
    let mut k = solidcraft();
    let mut tool = read_notify_named(&mut k, TOOL);
    assert!(is_manifold_solid(&tool).unwrap());
    checks_clean(&mut k, &mut tool);
}

/// `e3`: the cone minus `t24` is one closed solid of 8 faces, the cone's 104300.353 less
/// what the tool takes, and checks clean (upstream's tolerance refused it on the ellipse it
/// keeps from `t24`, as above).
#[test]
fn wr741_e3_cone_minus_the_notched_tool() {
    let mut k = solidcraft();
    let cone = read_notify_named(&mut k, CONE);
    let tool = read_notify_named(&mut k, TOOL);
    let mut r = boolean(&mut k, cone, tool, BooleanOperation::Difference)
        .unwrap()
        .result;
    assert!(is_manifold_solid(&r).unwrap());
    assert_eq!(r.face_list().len(), 8);
    let v = fine_volume(&k, &mut r);
    assert!((v - 100_808.551_793).abs() < 1e-5, "{v}");
    checks_clean(&mut k, &mut r);
}

/// `periph_case_R`'s lower part minus `t24` is one closed solid that loses exactly what it
/// shares with `t24`, and checks clean (upstream's tolerance refused it on the ellipse the cut
/// trims, `SmEllipse` tests 1 and 2 at 4.64e-9 against 1.7e-9, as it refused the whole part's
/// cut, 72170.402 to 70319.173).
#[test]
fn wr741_periph_case_minus_the_notched_tool() {
    let mut k = solidcraft();
    let (part, tool) = (
        read_notify_named(&mut k, PART),
        read_notify_named(&mut k, TOOL),
    );
    let mut common = boolean(&mut k, part, tool, BooleanOperation::Intersection)
        .unwrap()
        .result;
    let shared = fine_volume(&k, &mut common);
    let mut part = read_notify_named(&mut k, PART);
    let before = fine_volume(&k, &mut part);
    let tool = read_notify_named(&mut k, TOOL);
    let mut r = boolean(&mut k, part, tool, BooleanOperation::Difference)
        .unwrap()
        .result;
    assert!(is_manifold_solid(&r).unwrap());
    let after = fine_volume(&k, &mut r);
    // the cut takes `t24`'s corner of the slab, 1532.499 measured, whatever the intersection
    // says: two disjoint classifications cannot pass
    assert!(before - after > 1500.0, "{before} - {after}");
    assert!(
        (before - after - shared).abs() < 1e-4,
        "{before} - {after} against {shared}"
    );
    checks_clean(&mut k, &mut r);
}
