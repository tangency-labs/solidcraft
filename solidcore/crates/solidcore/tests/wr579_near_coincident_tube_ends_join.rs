//! WR-579 (tangency-labs/solidcore#1517; the fork's tangency-labs/smlib@9eec159): a bicycle
//! frame's seat tube (r 16, from (0, 0, 300) towards (-150, 0, 820)) ends 3.7e-4 short of
//! where the top tube (r 16, from (-150, 0, 820) to (600, 0, 860)) starts. Where the two
//! walls touch, the seat's end circle runs within tolerance of the top tube's wall for a
//! stretch and crosses it there too: the crossing and the stretch's ends fall in one proto
//! vertex as three vertex definitions too far apart to combine, and upstream's
//! `SmProtoEdge::Resolve` keeps the stretch's edge whole past the crossing's vertex. The
//! union then failed. WR-579's split of that edge at the crossing's vertex and WR-533's
//! split of a coincident definition across a split point are both always on; WR-533's
//! alone already joins this input.
//!
//! The fork builds each tube with its C API's extrude, the circle starting on the side the
//! walls touch (the top tube's seam at -y, the seat's at +y). This builds them as
//! `SmApiCreateCylinder` (seam at its frame's +X) turned about Z to put the seam there,
//! about Y onto the axis, and moved to the base, as the `boolean.api_brep` fixture
//! `issue_1517_tube_ends.json` does on both sides.
//!
//! Expected: one manifold solid, its volume between the larger tube's and both together,
//! and its check is clean, in either order of the tubes. On the side of the seat's seam
//! its end circle meets the top tube's start circle (the corner), 8.6e-5 on its own seam
//! vertex, and 2.9e-4 further it crosses the top tube's wall: three points farther apart
//! than their tolerances (1e-5 each), so three vertices, where the capture distance once
//! made them one (docs/booleans.md, "One point or two"; the WR-579 split was for that one
//! proto vertex).
//! Between the seam vertex and the crossing the intersection curve (the top tube's wall
//! cut by the seat's end plane, an ellipse of radii 71 and 16) runs 4e-9 inside the seat's
//! end circle: it is that circle's piece, proved by enclosure, and no second edge is made
//! beside it ("One edge or two"; upstream's second edge gave `SmFace` tests 33 and 34).
//! The fork's still
//! fails on a separate defect: an ellipse piece 8.6e-5 long (the top tube's wall cut by the
//! seat's end plane) whose STEP inversion set points near the arc's start to the start, so
//! its NURB read 2.1e-5 off its ellipse (`SmEllipse` test 2). That was
//! `SmExtent1d::ContainsPeriodicValue`, fixed for #1518 (WR-509).
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
use solidcore::math::det;

#[path = "support/volume_leak.rs"]
mod volume_leak;
use solidcore::boolean::api_brep::{boolean, boolean_union};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::mass::integrator::properties::{brep_volume, face_area, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p1::faces;
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

/// The union, the seat tube first when `seat_first`: the result's volume when it is one
/// manifold solid whose check is clean, else what went wrong.
fn join(seat_first: bool) -> Result<f64, String> {
    let mut k = Kernel::default();
    let (t, s) = (top(&mut k), seat(&mut k));
    let (a, b) = if seat_first { (s, t) } else { (t, s) };
    let mut r = boolean_union(&mut k, a, b).map_err(|e| format!("union: {e:?}"))?;
    if !is_manifold_solid(&r).unwrap() {
        return Err("not a manifold solid".into());
    }
    let lines = check_brep(&mut k, &mut r, CheckLevel::Level2)
        .unwrap()
        .lines();
    println!("check (seat first {seat_first}): {lines:?}");
    // any report fails: #1518's ellipse line, the one the fork still has, is gone with
    // WR-509's fix
    if !lines.is_empty() {
        return Err(format!("check: {lines:?}"));
    }
    Ok(volume(&k, &mut r))
}

/// The stretch's edge is split at the crossing's vertex (WR-579), and `UpdateEdges` splits
/// the coincident definition that runs across that split point (WR-533). In either order of
/// the tubes, `join` accepts only one manifold solid whose level-2 check reports nothing,
/// and its volume lies strictly between the larger tube's and both together; the two
/// volumes agree to 1e-6.
#[test]
fn wr579_near_coincident_tube_ends_join() {
    let mut k = Kernel::default();
    let (mut t, mut s) = (top(&mut k), seat(&mut k));
    let (vt, vs) = (volume(&k, &mut t), volume(&k, &mut s));
    let mut volumes = Vec::new();
    for seat_first in [false, true] {
        let v = join(seat_first).unwrap();
        assert!(
            v > vt.max(vs) && v < vt + vs,
            "seat first {seat_first}: union volume {v}, tubes {vt} and {vs}"
        );
        volumes.push(v);
    }
    // one body whatever the order
    let (lo, hi) = volumes
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    assert!(hi - lo <= 1e-6 * hi, "volumes {volumes:?}");
}

/// Closed-form enclosures of the union's volume and of its six face areas, sorted, as
/// `(low, high)`. The two axes lie in the plane y = 0 and meet at the top tube's start
/// `P = (-150, 0, 820)`; the seat's axis runs through `P` and the seat ends `delta` (3.7e-4)
/// short of it. With `t1`, `t2` the axial coordinates from `P` (the top tube is `t1` in
/// `[0, L1]`, the seat `t2` in `[-|P - (0, 0, 300)|, -delta]`), the overlap of the solids is the
/// bicylinder (both infinite cylinders of radius `r`) cut to `t1 >= 0`, `t2 <= -delta`; both
/// tubes reach far past the bicylinder (`|t| <= 2 r / sin a` there, 33 against 541 and 751).
///
/// - Cut by a plane parallel to both axes at height `z`, the bicylinder is a rhombus of two
///   strips of half-width `w = sqrt(r^2 - z^2)` crossing at the axes' angle `a`, so its
///   volume is `16 r^3 / (3 sin a)`. The quarter `t1 >= 0`, `t2 <= 0` is cut from every rhombus by
///   the same two lines through its centre, so it is the same fraction `f` of each
///   (`rhombus_quarter`): `V_q = f 16 r^3 / (3 sin a)`.
/// - On the top tube's wall at angle `th` the cut is the rhombus's edge at offset
///   `s = r sin th` with `w = |s|`, of length `2 |s| / sin a`; the quarter keeps the same fraction
///   `g` of the edge at each offset sign (`edge_quarter`), and the wall element is `r dth dt1`,
///   so the wall inside the other tube is `4 r^2 (g+ + g-) / sin a`; likewise for the seat.
/// - A cap meets the other cylinder in an ellipse that holds the whole disc (`|cos a| < 1`),
///   so the other tube covers half of it, up to the strip `delta` moves the dividing line.
/// - `delta` moves each figure by at most a slab: `delta pi r^2` of volume, a band
///   `2 pi r delta` of the seat's wall and `2 pi r delta / |cos a|` of the top's, and a strip
///   of `2 r delta / sin a` (top cap) or `2 r delta |cos a| / sin a` (seat cap) across a cap.
fn union_enclosure() -> ((f64, f64), [(f64, f64); 6]) {
    use std::f64::consts::PI;
    let r: f64 = 16.0;
    let (l1, l2) = (751.0659092250161_f64, 541.2019951093923_f64);
    let delta = det::hypot(150.0_f64, 520.0) - l2;
    let unit = |x: f64, z: f64| {
        let n = det::hypot(x, z);
        [x / n, z / n]
    };
    let (a1, a2) = (unit(750.0, 40.0), unit(-150.0, 520.0));
    let dot = |u: [f64; 2], v: [f64; 2]| u[0] * v[0] + u[1] * v[1];
    let normal = |a: [f64; 2]| [-a[1], a[0]];
    let (n1, n2) = (normal(a1), normal(a2));
    let cos_a = dot(a1, a2);
    let sin_a = (a1[0] * a2[1] - a1[1] * a2[0]).abs();
    // the quarter t1 >= 0, t2 <= 0 of the unit rhombus |n1.x| <= 1, |n2.x| <= 1
    let keep = |poly: Vec<[f64; 2]>, n: [f64; 2], c: f64| -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (fp, fq) = (dot(n, p) - c, dot(n, q) - c);
            if fp <= 0.0 {
                out.push(p);
            }
            if fp * fq < 0.0 {
                let t = fp / (fp - fq);
                out.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
        }
        out
    };
    let shoelace = |poly: &[[f64; 2]]| {
        let mut a = 0.0;
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            a += p[0] * q[1] - q[0] * p[1];
        }
        0.5 * a.abs()
    };
    let neg = |n: [f64; 2]| [-n[0], -n[1]];
    let mut rhombus = vec![
        [-100.0, -100.0],
        [100.0, -100.0],
        [100.0, 100.0],
        [-100.0, 100.0],
    ];
    for n in [n1, n2] {
        rhombus = keep(keep(rhombus, n, 1.0), neg(n), 1.0);
    }
    let quarter = keep(keep(rhombus.clone(), neg(a1), 0.0), a2, 0.0);
    let rhombus_quarter = shoelace(&quarter) / shoelace(&rhombus);
    // the fraction of the rhombus edge n.x = s (s = +-1) that the quarter keeps
    let edge_quarter = |n: [f64; 2], a: [f64; 2], other: [f64; 2], s: f64| {
        let (c0, c1) = (s * dot(other, n), dot(other, a));
        let (mut lo, mut hi) = ((-1.0 - c0) / c1, (1.0 - c0) / c1);
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        let full = hi - lo;
        for (b, sign) in [(a1, 1.0), (a2, -1.0)] {
            let (b0, b1) = (s * dot(b, n), dot(b, a));
            let at = -b0 / b1;
            if sign * b1 > 0.0 {
                lo = lo.max(at);
            } else {
                hi = hi.min(at);
            }
        }
        (hi - lo).max(0.0) / full
    };
    let g1 = edge_quarter(n1, a1, n2, 1.0) + edge_quarter(n1, a1, n2, -1.0);
    let g2 = edge_quarter(n2, a2, n1, 1.0) + edge_quarter(n2, a2, n1, -1.0);

    let disc = PI * r * r;
    let v_q = rhombus_quarter * 16.0 * det::powi(r, 3) / (3.0 * sin_a);
    let v_lo = disc * (l1 + l2) - v_q;
    let volume = (v_lo, v_lo + delta * disc);
    let top_wall = 2.0 * PI * r * l1 - 4.0 * r * r * g1 / sin_a;
    let seat_wall = 2.0 * PI * r * l2 - 4.0 * r * r * g2 / sin_a;
    let half = disc / 2.0;
    let top_cap = (half, half + 2.0 * r * delta / sin_a);
    let seat_strip = 2.0 * r * delta * cos_a.abs() / sin_a;
    let mut areas = [
        (half - seat_strip, half + seat_strip),
        top_cap,
        (disc, disc),
        (disc, disc),
        (seat_wall, seat_wall + 2.0 * PI * r * delta),
        (top_wall, top_wall + 2.0 * PI * r * delta / cos_a.abs()),
    ];
    // the two half caps overlap as ranges; sorted values pair with ranges sorted by midpoint
    areas.sort_by(|p, q| (p.0 + p.1).total_cmp(&(q.0 + q.1)));
    (volume, areas)
}

/// The union's volume lies in a certified enclosure (`enclose_volume`) at most `5e-6` of
/// it wide that holds the golden row's volume, and its volume and face areas lie in their
/// closed-form enclosures ([`union_enclosure`]), widened by the integrators' requested accuracies (the goldens'
/// `1e-6` for volume, `1e-4` for areas, relative) and `1e-12` relative for rounding in the
/// enclosures themselves. Six faces: both walls, both far caps and both cut caps.
#[test]
fn wr533_tube_ends_union_measures_lie_in_their_closed_forms() {
    let mut k = Kernel::default();
    let (t, s) = (top(&mut k), seat(&mut k));
    let mut r = boolean_union(&mut k, t, s).unwrap();
    let ((v_lo, v_hi), area_ranges) = union_enclosure();
    let within = |x: f64, lo: f64, hi: f64, rel: f64| {
        let slack = (rel + 1e-12) * hi.abs();
        x >= lo - slack && x <= hi + slack
    };
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut r, 1e-6, &attrs).unwrap();
    println!("volume {v} in [{v_lo}, {v_hi}]");
    assert!(
        within(v, v_lo, v_hi, 1e-6),
        "volume {v} outside [{v_lo}, {v_hi}]"
    );
    // the certified volume (`enclose_volume`, rounding counted) at most 5e-6 of it wide
    // holds the `boolean.api_brep` row `issue1517_tube_ends_union_measured`'s volume, and
    // meets the closed-form window. 5e-6 rather than 1e-7: this body's enclosure has a floor
    // of 2.84 (2.8e-6 of the volume; the strip cells along its trims that no cut narrows),
    // so 1e-7 is out of its reach; the closed-form window, 2.9e-7 wide, is the tighter one.
    let golden = 1_032_433.136_484_584_2;
    let e = enclose_volume(&k, &mut r, 5e-6 * golden).unwrap();
    println!(
        "enclosure [{}, {}] width {} ({:?})",
        e.lo,
        e.hi,
        e.width(),
        e.stop
    );
    assert!(e.width() <= 5e-6 * golden, "enclosure {e:?}");
    assert!(
        e.lo <= golden && golden <= e.hi,
        "golden {golden} outside {e:?}"
    );
    assert!(
        e.lo <= v_hi && v_lo <= e.hi,
        "enclosure {e:?} misses [{v_lo}, {v_hi}]"
    );
    let mut areas: Vec<f64> = faces(&r, None)
        .unwrap()
        .into_iter()
        .map(|f| face_area(&k, &mut r, f, 1e-4).unwrap())
        .collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas.len(), 6, "faces: {areas:?}");
    for (a, (lo, hi)) in areas.iter().zip(area_ranges) {
        println!("area {a} in [{lo}, {hi}]");
        assert!(within(*a, lo, hi, 1e-4), "area {a} outside [{lo}, {hi}]");
    }
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
/// top tube's start. The fixes are the default kernel's, or every one when `all`;
/// returns the check's labels.
fn join_with_gap(gap: f64, all: bool) -> Vec<String> {
    let mut k = Kernel::default();
    if all {
        k.fixes = UpstreamFixes::all();
    }
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

/// Joins #1529's tubes at `gap` with every fix on when `all`. Every test the check
/// reports must also be in `off`, upstream's pinned report (WR-575's span removal,
/// measured with the retired flag off), and at 1e-5 the circle's `SmEllipse` #1/#2 must
/// be gone (WR-575's own case). The report must also be exactly `expected` (pinned from
/// the run that retired the flag). Returns what went wrong.
fn flag_row(gap: f64, all: bool, off: &[String], expected: &[String]) -> Vec<String> {
    let mut failures = Vec::new();
    let now = join_with_gap(gap, all);
    println!("gap {gap} all {all}: upstream {off:?}, now {now:?}");
    if now != expected {
        failures.push(format!(
            "gap {gap} all {all}: now {now:?}, expected {expected:?}"
        ));
    }
    if !is_sub_multiset(&now, off) {
        failures.push(format!(
            "gap {gap} all {all}: now {now:?} adds to upstream's {off:?}"
        ));
    }
    if gap == 1e-5 && now.iter().any(|x| x.starts_with("SmEllipse")) {
        failures.push(format!(
            "gap {gap} all {all}: now {now:?} keeps the ellipse"
        ));
    }
    failures
}

/// tangency-labs/solidcore#2424: WR-575 keeps a conic's short end span, and where that
/// span is a pole (#2090) the trim ends at the interior knot. When the edge is split
/// there, both pieces must end at the same parameter, and the edge's UV trims with them.
/// Expected: at every gap the check reports no test upstream's doesn't (no `SmEdgeuse`
/// #25/#26), and at 1e-5 it still drops the circle's `SmEllipse` #1/#2. Upstream's
/// reports (measured with the retired flag off) are pinned. Every fix on at 1e-6 is
/// [`wr575_every_fix_at_1e_6_adds_no_check_failure`].
#[test]
fn wr575_tube_ends_join_adds_no_check_failure_at_any_gap() {
    // (gap, every fix on, upstream's report). At 1e-5 upstream's reports changed with
    // WR-751 (#2621), `MergeIntoObject` splitting the two end circles where they pass
    // nearest their points (3.1e-6 -> 2.3e-6 and 2.8e-6 -> 2.2e-6 off): one gap pair
    // (7.77e-6 against 7.5e-6) goes, and each split now falls a few micrometres from the
    // circle's half-turn knot, so both pieces' short end spans fail `SmEllipse` #1/#2
    // (1.8e-6 and 1.2e-6; one piece, 1.5e-6, before): WR-575's defect, which its fix
    // clears.
    // Face test 36 is gone from every row since #13295: it named the joined tube's seam
    // edge ending at a vertex a few micrometres across the seam, inside the vertex's own
    // tolerance, with no edge beside the seam there: a corner, not a sliver.
    // (gap, every fix on, upstream's report, the report now: nothing. Of the two gap
    // pairs of the default kernel at 1e-5, the 7.77e-6 at the -y vertex went when WR-453's
    // continuation became the only path: the intersection edge that stopped 2.77e-6 short
    // of its proto vertex is continued onto it, and its end now lies 5e-6 from the vertex,
    // as the circle edges there do. The other, an edge use ending 8.15e-6 from that
    // vertex, over half its tolerance 1.5e-5, went when every vertex a definition made
    // was given the tolerance that covers the ends of its edges, not only an exactly
    // placed one (docs/booleans.md, "What a vertex stands for").)
    let rows = [
        (1e-5, false, report(2, 2, 0), report(0, 0, 0)),
        (1e-6, false, report(0, 1, 0), report(0, 0, 0)),
        (1e-7, false, report(0, 0, 0), report(0, 0, 0)),
        (1e-5, true, report(0, 2, 0), report(0, 0, 0)),
        (1e-7, true, report(0, 0, 0), report(0, 0, 0)),
    ];
    let failures: Vec<String> = rows
        .iter()
        .flat_map(|(gap, all, off, now)| flag_row(*gap, *all, off, now))
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
    // (face test 36 gone since #13295, as in the rows above)
    let failures = flag_row(1e-6, true, &report(0, 1, 0), &report(0, 0, 0));
    assert!(failures.is_empty(), "{failures:#?}");
}

/// SolidCraft's profile: every fix on.
fn solidcraft() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
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
    // 100808.551793 until `brep_volume` closed tolerant edges' gaps (#16735)
    assert!((v - 100_808.552_462_565).abs() < 1e-5, "{v}");
    volume_leak::assert_moved_by_the_old_leak(&k, &mut r, 100_808.551_793, v, 1e-9);
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
