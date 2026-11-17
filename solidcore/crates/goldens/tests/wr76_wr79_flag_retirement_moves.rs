//! WR-76 and WR-79 (`ssi_trace_tangent_curves_once` and `ssi_low_angle_start_on_probation`
//! retired, part of #14246): every golden row the two retirements move is replayed and shown
//! equal to its old row up to internal state, or closer to the true intersection.
//!
//! `data/wr76_wr79_moved_rows.jsonl` is the retirements' whole diff of the goldens: one line
//! per moved row, `{"unit", "case", "class", "old", "new", "patch"}`, `old` and `new` the
//! case's golden line before and after, and `patch` the old raw values the line digests: per
//! value key `[prefix, suffix, middle]`, the old array being the new one's first `prefix`
//! values, then `middle` (hex `f64` bits), then its last `suffix` values. No other row of any
//! unit moves (the goldens job checks every row).
//!
//! Run (about two minutes):
//! `cargo test --profile goldens -p goldens --test wr76_wr79_flag_retirement_moves -- --ignored --nocapture`
//! It is `#[ignore]`d because it re-runs whole golden units; CI's goldens job already checks
//! the new rows themselves.
//!
//! # The proof (replayed row by row)
//!
//! For every row the test first runs the case on the retired code and requires its golden
//! line to be exactly `new`, then rebuilds the old raw values from the new ones and the patch
//! and requires their golden line to be exactly `old` (so the old values the certificates
//! read are the ones the old row digests). Then, by `class`:
//!
//! - **`wr79-trace-direction`** (equal): the old and new records have the same length and
//!   differ only in values that went from `+1` to `-1`, the intersector's
//!   `curve_trace_direction` that a rejected probation trace (`FindInteriorCurves` traces a
//!   start point back the other way before the loop test rejects it) leaves negated: every
//!   curve, status and count is unchanged. The test pins the slots: in
//!   `intersect.surface_intersector_p5` the only one is the state's `trace_dir` (index 7); in
//!   `intersect.adv_surface_intersector` the last is the final state's (`len - 6`) and each
//!   other is preceded by a `separate_gauss_maps` flag (`0` or `1`), as `put_state` writes.
//! - **`wr76-p4-retrace`** (closer): the unit traces from a point on its one found curve, the
//!   segment `curves[0]` on both planes. Only the `flush` output moves. Each old and new 3D
//!   curve is a B-spline whose control points (so, by the convex hull property, every point)
//!   lie within the tolerance of that segment's span and of both planes: every old curve is a
//!   retrace of the found curve. The new curves span a sub-interval of the segment the old
//!   ones span, strictly shorter (a degenerate touching point, or no curve), so the trace
//!   retraces less of a curve already found and nothing else changes.
//! - **`wr76-p5-duplicate`** (closer): the status is unchanged, the state only in its count
//!   of through points left, and the new curves are the old ones (bit for bit, in order) less
//!   some. Each dropped curve `D` lies within the tolerance of a kept curve `K`: for the line
//!   through `K`'s end points (`K` is clamped, so they are its ends), every point of `D` is
//!   within `perp(D)` of the line at a position whose projection is within `over` of the
//!   segment between `K`'s ends, and `K` (continuous, its projection running from one end to
//!   the other) has a point within `perp(K)` of the line at each projection there, so
//!   `D` is within `perp(D) + perp(K) + over` of `K`, which the test requires to be within the
//!   tolerance (`perp` the largest control point distance to the line). So each dropped curve
//!   is a second trace of a kept one.
//! - **`wr76-offset-overlap`** (closer): the offset's last intersection curve, on the circle
//!   `z = z0`, `r = R` about the `z` axis where two offset surfaces touch, ran on from the free
//!   end of an earlier curve `C` of the same circle over a further arc (about 31 degrees); now
//!   it stops at that end. Everything before the curve's record is unchanged. With the
//!   angle of the control points unwrapped along each curve (each run of four, the hull of a
//!   cubic span, spans under 90 degrees and so a convex wedge that misses the axis, where
//!   the angle is continuous), the old curve covers the angles `[a0, a1]`, the new one
//!   `[a0, b1]` with `b1 < a1`, and `C` from `b1` (its start within `1e-6` of the new end) to
//!   past `a1`. Every point of a span lies in its wedge at a radius between the smallest
//!   control point radius times `cos(width / 2)` and the largest, and between its control
//!   points' heights, so any two points of these curves at one angle are within `eps`, the
//!   spread of those bounds: the arc the new curve no longer traces is within `eps` of `C`
//!   at every point (a second trace of it), and the arc it keeps is within `eps` of the old
//!   curve. The test requires `eps` below a hundredth of the dropped arc's length.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::too_many_lines
)] // test

use std::collections::BTreeMap;

use goldens::case::repo_root;
use goldens::{golden, registry, CaseContext, Output};
use serde_json::Value;

/// The moved rows (see the module docs).
const INVENTORY: &str = include_str!("data/wr76_wr79_moved_rows.jsonl");

/// The goldens' default number of generated cases per unit (the seeds they were blessed with).
const SEEDS: usize = 16;

/// The values of a stored `TsectPnt` in `intersect.surface_intersector_p4`'s record.
const POINT_LEN: usize = 80;

type P3 = [f64; 3];

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}
fn p3(v: &Value) -> P3 {
    let a = v.as_array().unwrap();
    [
        a[0].as_f64().unwrap(),
        a[1].as_f64().unwrap(),
        a[2].as_f64().unwrap(),
    ]
}

/// A B-spline as the records write it.
#[derive(Debug, Clone, PartialEq)]
struct Spline {
    deg: usize,
    cps: Vec<[f64; 4]>,
    knots: Vec<f64>,
}

impl Spline {
    fn points(&self) -> Vec<P3> {
        self.cps.iter().map(|c| [c[0], c[1], c[2]]).collect()
    }
    /// Non-rational (no weights: `f64::MIN`) and clamped, so its ends are its end control
    /// points and the hull property holds per span of `deg + 1` control points.
    fn check_plain(&self) {
        assert!(
            self.cps.iter().all(|c| c[3] == f64::MIN),
            "a rational curve"
        );
        let d = self.deg;
        let k = &self.knots;
        assert_eq!(k.len(), self.cps.len() + d + 1, "knot count");
        assert!(
            k[..=d].iter().all(|x| *x == k[0]),
            "not clamped at the start"
        );
        assert!(
            k[k.len() - d - 1..].iter().all(|x| *x == k[k.len() - 1]),
            "not clamped at the end"
        );
    }
}

struct Reader<'a> {
    v: &'a [f64],
    i: usize,
}

impl<'a> Reader<'a> {
    fn new(v: &'a [f64]) -> Self {
        Self { v, i: 0 }
    }
    fn g(&mut self) -> f64 {
        let x = self.v[self.i];
        self.i += 1;
        x
    }
    fn n(&mut self, n: usize) -> &'a [f64] {
        let x = &self.v[self.i..self.i + n];
        self.i += n;
        x
    }
    fn done(&self) -> bool {
        self.i >= self.v.len()
    }
    /// `type dim knot_type form has [n deg m cps[(n+1) x 4] knots[m+1]] ivl[2]`.
    fn spline(&mut self) -> Option<Spline> {
        let head = self.n(5);
        let s = if head[4] == 1.0 {
            let n = self.g() as usize;
            let deg = self.g() as usize;
            let m = self.g() as usize;
            let cps = (0..=n)
                .map(|_| {
                    let c = self.n(4);
                    [c[0], c[1], c[2], c[3]]
                })
                .collect();
            let knots = self.n(m + 1).to_vec();
            Some(Spline { deg, cps, knots })
        } else {
            None
        };
        self.n(2);
        s
    }
}

// ---- intersect.surface_intersector_p4 ------------------------------------------------------

fn p4_ref(r: &mut Reader<'_>) {
    if r.g() == -1.0 {
        r.n(POINT_LEN);
    }
}
fn p4_list(r: &mut Reader<'_>) {
    let n = r.g() as usize;
    for _ in 0..n {
        p4_ref(r);
    }
}
fn p4_state(r: &mut Reader<'_>) {
    r.n(13);
    for _ in 0..3 {
        p4_list(r);
    }
    let n = r.g() as usize;
    r.n(n * POINT_LEN);
}
fn p4_curves(r: &mut Reader<'_>) -> Vec<Spline> {
    let n = r.g() as usize;
    (0..n).map(|_| r.spline().unwrap()).collect()
}

/// The record's pieces: per op (and the final state) its value range, and the flush's 3D
/// curves and types.
struct P4 {
    pieces: Vec<(String, std::ops::Range<usize>)>,
    flush: Option<(Vec<Spline>, Vec<f64>)>,
    tol: f64,
}

fn p4_decode(rec: &[f64], ops: &[Value]) -> P4 {
    let mut r = Reader::new(rec);
    let mut pieces = Vec::new();
    let mut flush = None;
    for op in ops {
        let kind = op["op"].as_str().unwrap();
        let at = r.i;
        r.g();
        match kind {
            "state" => {
                r.g();
                p4_state(&mut r);
            }
            "point" | "cpv" => {
                r.g();
                r.n(POINT_LEN);
            }
            "add_curve" | "add_start" | "add_through" | "trace" | "members" | "ends" | "ext" => {
                r.g();
            }
            "flush" | "fic" => {
                r.g();
                let c3 = p4_curves(&mut r);
                p4_curves(&mut r);
                p4_curves(&mut r);
                let nt = r.g() as usize;
                let types = r.n(nt).to_vec();
                let nd = r.g() as usize;
                r.n(nd);
                if kind == "flush" {
                    flush = Some((c3, types));
                }
            }
            "dpi" | "dli" => {
                if r.g() == 0.0 {
                    for _ in 0..3 {
                        if r.g() == 1.0 {
                            r.spline();
                        }
                    }
                    r.n(2);
                }
            }
            other => panic!("op {other}"),
        }
        pieces.push((kind.to_owned(), at..r.i));
    }
    let at = r.i;
    let tol = rec[at];
    p4_state(&mut r);
    pieces.push(("final".to_owned(), at..r.i));
    assert_eq!(r.i, rec.len(), "record not consumed");
    P4 { pieces, flush, tol }
}

/// The parameter range `[t0, t1]` (over the segment `a -> b`) of `curves`, after requiring
/// every control point within `tol` of the segment's line, of its span and of both planes.
fn on_found_segment(
    curves: &[Spline],
    a: P3,
    b: P3,
    planes: &[(P3, P3); 2],
    tol: f64,
) -> Option<(f64, f64)> {
    let d = sub(b, a);
    let len = norm(d);
    let mut range: Option<(f64, f64)> = None;
    for c in curves {
        c.check_plain();
        for p in c.points() {
            let t = dot(sub(p, a), d) / (len * len);
            let foot = [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]];
            assert!(
                norm(sub(p, foot)) <= tol,
                "a control point off the found segment's line"
            );
            assert!(
                t * len >= -tol && (t - 1.0) * len <= tol,
                "a control point past the found segment"
            );
            for (o, n) in planes {
                assert!(
                    dot(sub(p, *o), *n).abs() <= tol,
                    "a control point off a plane"
                );
            }
            range = Some(match range {
                None => (t, t),
                Some((lo, hi)) => (lo.min(t), hi.max(t)),
            });
        }
    }
    range
}

fn plane_of(s: &Value) -> (P3, P3) {
    assert_eq!(s["kind"], "plane");
    let n = cross(p3(&s["x"]), p3(&s["y"]));
    let l = norm(n);
    (p3(&s["o"]), [n[0] / l, n[1] / l, n[2] / l])
}

fn check_p4(inputs: &Value, old: &Output, new: &Output) -> String {
    let ops = inputs["ops"].as_array().unwrap();
    let (o, n) = (&old.values["rec"], &new.values["rec"]);
    let (od, nd) = (p4_decode(o, ops), p4_decode(n, ops));
    assert_eq!(od.pieces.len(), nd.pieces.len());
    for ((ko, ro), (kn, rn)) in od.pieces.iter().zip(&nd.pieces) {
        assert_eq!(ko, kn);
        if ko != "flush" {
            assert_eq!(o[ro.clone()], n[rn.clone()], "op {ko} moved");
        }
    }
    let curves = inputs["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0]["kind"], "segment");
    let (a, b) = (p3(&curves[0]["a"]), p3(&curves[0]["b"]));
    let planes = [plane_of(&inputs["a"]), plane_of(&inputs["b"])];
    let tol = od.tol;
    assert_eq!(tol, nd.tol);
    let (oc, _) = od.flush.unwrap();
    let (nc, ntypes) = nd.flush.unwrap();
    let len = norm(sub(b, a));
    let (o0, o1) = on_found_segment(&oc, a, b, &planes, tol).expect("an old curve");
    let slack = tol / len;
    let new_len = match on_found_segment(&nc, a, b, &planes, tol) {
        None => 0.0,
        Some((n0, n1)) => {
            assert!(
                n0 >= o0 - slack && n1 <= o1 + slack,
                "new spans more of the found curve"
            );
            n1 - n0
        }
    };
    assert!(new_len < o1 - o0, "new retraces no less of the found curve");
    format!(
        "{} old curve(s) retrace {:.3e} of the found segment, {} new ({ntypes:?}) retrace {:.3e} (tol {tol:.3e})",
        oc.len(),
        (o1 - o0) * len,
        nc.len(),
        new_len * len
    )
}

// ---- intersect.surface_intersector_p5 ------------------------------------------------------

struct P5 {
    status: f64,
    state: Vec<f64>,
    /// Each curve's value range in the record, and its 3D curve.
    curves: Vec<(std::ops::Range<usize>, Spline)>,
}

fn p5_decode(rec: &[f64], has_state: bool) -> P5 {
    let mut r = Reader::new(rec);
    let status = r.g();
    let state = if has_state {
        r.n(15).to_vec()
    } else {
        Vec::new()
    };
    let mut curves = Vec::new();
    if !r.done() {
        let n = r.g() as usize;
        for _ in 0..n {
            let at = r.i;
            let c3 = r.spline().unwrap();
            for _ in 0..2 {
                if r.g() == 1.0 {
                    r.spline();
                }
            }
            r.n(2);
            curves.push((at..r.i, c3));
        }
    }
    assert_eq!(r.i, rec.len(), "record not consumed");
    P5 {
        status,
        state,
        curves,
    }
}

/// How far `d` can be from `k`: see the module docs.
fn duplicate_bound(d: &Spline, k: &Spline) -> f64 {
    let kp = k.points();
    let (k0, k1) = (kp[0], kp[kp.len() - 1]);
    let dir = sub(k1, k0);
    let len = norm(dir);
    let u = [dir[0] / len, dir[1] / len, dir[2] / len];
    let place = |p: P3| {
        let t = dot(sub(p, k0), u);
        let foot = [k0[0] + t * u[0], k0[1] + t * u[1], k0[2] + t * u[2]];
        (t, norm(sub(p, foot)))
    };
    let perp_k = kp.iter().map(|p| place(*p).1).fold(0.0, f64::max);
    let mut perp_d: f64 = 0.0;
    let mut over: f64 = 0.0;
    for p in d.points() {
        let (t, q) = place(p);
        perp_d = perp_d.max(q);
        over = over.max(-t).max(t - len);
    }
    perp_d + perp_k + over
}

fn check_p5(inputs: &Value, old: &Output, new: &Output) -> String {
    let has_state = !inputs["trees"].is_null();
    assert!(has_state, "a moved row without the intersector state");
    let (o, n) = (&old.values["rec"], &new.values["rec"]);
    let (od, nd) = (p5_decode(o, true), p5_decode(n, true));
    assert_eq!(od.status, nd.status);
    for (i, (x, y)) in od.state.iter().zip(&nd.state).enumerate() {
        assert!(i == 14 || x == y, "state value {i} moved");
    }
    let tol = od.state[8];
    // the new curves are the old ones less some, in order
    let mut kept = Vec::new();
    let mut j = 0;
    for (rn, _) in &nd.curves {
        while j < od.curves.len() && o[od.curves[j].0.clone()] != n[rn.clone()] {
            j += 1;
        }
        assert!(j < od.curves.len(), "a new curve that is not an old one");
        kept.push(j);
        j += 1;
    }
    let mut worst: f64 = 0.0;
    let mut dropped = 0;
    for (i, (_, d)) in od.curves.iter().enumerate() {
        if kept.contains(&i) {
            continue;
        }
        dropped += 1;
        d.check_plain();
        let best = kept
            .iter()
            .map(|&k| {
                od.curves[k].1.check_plain();
                duplicate_bound(d, &od.curves[k].1)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(
            best <= tol,
            "dropped curve {i} is {best:.3e} from every kept curve (tol {tol:.3e})"
        );
        worst = worst.max(best / tol);
    }
    assert!(dropped > 0);
    format!(
        "{dropped} of {} curves dropped, each within {worst:.3} tol of a kept one; through points left {} -> {}",
        od.curves.len(),
        od.state[14],
        nd.state[14]
    )
}

// ---- WR-79 -------------------------------------------------------------------------------

fn check_trace_direction(unit: &str, old: &Output, new: &Output) -> String {
    assert_eq!(old.values.len(), new.values.len());
    let mut flips = 0;
    for (key, o) in &old.values {
        let n = &new.values[key];
        assert_eq!(o.len(), n.len(), "{key}: length moved");
        for (i, (x, y)) in o.iter().zip(n).enumerate() {
            if x.to_bits() == y.to_bits() {
                continue;
            }
            assert!(
                *x == 1.0 && *y == -1.0,
                "{key}[{i}]: {x} -> {y} is not a direction flip"
            );
            match unit {
                "intersect.surface_intersector_p5" => assert_eq!(i, 7, "not the trace_dir slot"),
                "intersect.adv_surface_intersector" => assert!(
                    i == o.len() - 6 || n[i - 1] == 0.0 || n[i - 1] == 1.0,
                    "{key}[{i}]: not after a separate_gauss_maps flag"
                ),
                _ => panic!("{unit}"),
            }
            flips += 1;
        }
    }
    assert!(flips > 0);
    format!("{flips} trace direction value(s) +1 -> -1, nothing else")
}

// ---- offset.offset_geometry_creation -----------------------------------------------------

/// The 3D B-spline whose header (`14033 3 1 n 3 n+4`, as the offset record writes a cubic
/// non-rational curve) starts at `i`, if one does.
fn offset_curve_at(v: &[f64], i: usize) -> Option<Spline> {
    let h = v.get(i..i + 6)?;
    if !(h[0] == 14033.0 && h[1] == 3.0 && h[2] == 1.0 && h[4] == 3.0 && h[5] == h[3] + 4.0) {
        return None;
    }
    let n = h[3] as usize;
    let st = i + 6;
    let cps = (0..=n)
        .map(|j| {
            let c = v.get(st + 4 * j..st + 4 * j + 4)?;
            Some([c[0], c[1], c[2], c[3]])
        })
        .collect::<Option<Vec<_>>>()?;
    let knots = v.get(st + 4 * (n + 1)..st + 4 * (n + 1) + n + 5)?.to_vec();
    Some(Spline { deg: 3, cps, knots })
}

/// Per span (four control points) of `c`: the unwrapped angle range, the least radius
/// (times `cos(width / 2)`) and the largest, and the height range; plus the unwrapped
/// angle of every control point.
struct Wedges {
    angles: Vec<f64>,
    spans: Vec<[f64; 6]>,
}

fn wedges(c: &Spline, start: f64) -> Wedges {
    let pts = c.points();
    let mut angles = Vec::with_capacity(pts.len());
    let mut prev = start;
    for p in &pts {
        let a = p[1].atan2(p[0]);
        let k = ((prev - a) / std::f64::consts::TAU).round();
        let a = a + k * std::f64::consts::TAU;
        assert!((a - prev).abs() < std::f64::consts::FRAC_PI_4 || angles.is_empty());
        angles.push(a);
        prev = a;
    }
    let spans = (0..pts.len() - 3)
        .map(|j| {
            let w = &pts[j..j + 4];
            let a = &angles[j..j + 4];
            let (a0, a1) = (
                a.iter().copied().fold(f64::INFINITY, f64::min),
                a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            );
            assert!(
                a1 - a0 < std::f64::consts::FRAC_PI_2,
                "a span wedge of 90 degrees or more"
            );
            let r: Vec<f64> = w.iter().map(|p| p[0].hypot(p[1])).collect();
            let rmin = r.iter().copied().fold(f64::INFINITY, f64::min) * ((a1 - a0) / 2.0).cos();
            let rmax = r.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let z0 = w.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
            let z1 = w.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
            [a0, a1, rmin, rmax, z0, z1]
        })
        .collect();
    Wedges { angles, spans }
}

/// The radius and height spread over the spans of `ws` whose angles meet `[lo, hi]`.
fn spread(ws: &[&Wedges], lo: f64, hi: f64) -> f64 {
    let mut b = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    for w in ws {
        for s in &w.spans {
            if s[1] < lo || s[0] > hi {
                continue;
            }
            b[0] = b[0].min(s[2]);
            b[1] = b[1].max(s[3]);
            b[2] = b[2].min(s[4]);
            b[3] = b[3].max(s[5]);
        }
    }
    (b[1] - b[0]) + (b[3] - b[2])
}

fn check_offset(old: &Output, new: &Output) -> String {
    let (o, n) = (&old.values["state"], &new.values["state"]);
    let same = o
        .iter()
        .zip(n)
        .take_while(|(x, y)| x.to_bits() == y.to_bits())
        .count();
    // the moved curve's header is the last one before the first moved value
    let h = (0..=same)
        .rev()
        .find(|&i| offset_curve_at(o, i).is_some())
        .unwrap();
    let (oc, nc) = (
        offset_curve_at(o, h).unwrap(),
        offset_curve_at(n, h).unwrap(),
    );
    oc.check_plain();
    nc.check_plain();
    let (op, np) = (oc.points(), nc.points());
    assert_eq!(op[0], np[0], "the curve's start moved");
    // the earlier curve the new end meets
    let earlier: Vec<Spline> = (0..h).filter_map(|i| offset_curve_at(o, i)).collect();
    let end = np[np.len() - 1];
    let (gap, c) = earlier
        .iter()
        .map(|c| (norm(sub(c.points()[0], end)), c))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
    assert!(
        gap <= 1e-6,
        "the new end is {gap:.3e} from every earlier curve's start"
    );
    c.check_plain();
    let ow = wedges(&oc, op[0][1].atan2(op[0][0]));
    let nw = wedges(&nc, ow.angles[0]);
    let (a0, a1) = (ow.angles[0], ow.angles[ow.angles.len() - 1]);
    let b1 = nw.angles[nw.angles.len() - 1];
    let cw = wedges(c, b1);
    let (c0, c1) = (cw.angles[0], cw.angles[cw.angles.len() - 1]);
    // all three run the same way, the new one stopping where the earlier one starts
    let s = (a1 - a0).signum();
    assert!(
        s * (b1 - a0) > 0.0 && s * (a1 - b1) > 0.0,
        "the new curve is not a part of the old one"
    );
    assert!(
        s * (c1 - c0) > 0.0 && s * (c1 - a1) >= 0.0,
        "the earlier curve does not cover the dropped arc"
    );
    let (lo, hi) = if s > 0.0 { (b1, a1) } else { (a1, b1) };
    let radius = op[0][0].hypot(op[0][1]);
    let arc = (a1 - b1).abs() * radius;
    let eps_dropped = spread(&[&ow, &cw], lo, hi);
    let (klo, khi) = if s > 0.0 { (a0, b1) } else { (b1, a0) };
    let eps_kept = spread(&[&ow, &nw], klo, khi);
    assert!(
        eps_dropped < arc / 100.0 && eps_kept < arc / 100.0,
        "the curves are not one circle's traces"
    );
    format!(
        "dropped arc {:.3} deg ({arc:.3}) within {eps_dropped:.3e} of the earlier curve, kept arc within {eps_kept:.3e} of the old one, new end {gap:.3e} from the earlier curve's start",
        (a1 - b1).abs().to_degrees()
    )
}

// ---- driver -------------------------------------------------------------------------------

fn patched(new: &Output, patch: &Value) -> Output {
    let mut o = new.clone();
    for (key, p) in patch.as_object().unwrap() {
        let p = p.as_array().unwrap();
        let (pre, suf) = (
            p[0].as_u64().unwrap() as usize,
            p[1].as_u64().unwrap() as usize,
        );
        let mid: Vec<f64> = p[2]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| f64::from_bits(u64::from_str_radix(h.as_str().unwrap(), 16).unwrap()))
            .collect();
        let n = &new.values[key];
        let mut v = n[..pre].to_vec();
        v.extend(mid);
        v.extend_from_slice(&n[n.len() - suf..]);
        o.values.insert(key.clone(), v);
    }
    o
}

#[test]
#[ignore = "re-runs five golden units; see the module docs"]
fn moved_rows_are_equal_or_closer() {
    let mut rows: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    for line in INVENTORY.lines() {
        let r: Value = serde_json::from_str(line).unwrap();
        rows.entry(r["unit"].as_str().unwrap().to_owned())
            .or_default()
            .insert(r["case"].as_str().unwrap().to_owned(), r);
    }
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let mut checked = 0usize;
    for unit in registry() {
        let Some(moved) = rows.get(unit.id()) else {
            continue;
        };
        let cases = unit.cases(&ctx).unwrap();
        for name in moved.keys() {
            assert!(
                cases.iter().any(|c| &c.name == name),
                "{}: no case {name}",
                unit.id()
            );
        }
        for case in cases.iter().filter(|c| moved.contains_key(&c.name)) {
            let r = &moved[&case.name];
            let new = unit.run_rust(case).unwrap();
            assert_eq!(
                golden::encode(&case.name, &new),
                r["new"],
                "{} {}: the retired code does not give the new row",
                unit.id(),
                case.name
            );
            let old = patched(&new, &r["patch"]);
            assert_eq!(
                golden::encode(&case.name, &old),
                r["old"],
                "{} {}: the patch does not give the old row",
                unit.id(),
                case.name
            );
            let class = r["class"].as_str().unwrap();
            let what = match class {
                "wr79-trace-direction" => check_trace_direction(unit.id(), &old, &new),
                "wr76-p4-retrace" => check_p4(&case.inputs, &old, &new),
                "wr76-p5-duplicate" => check_p5(&case.inputs, &old, &new),
                "wr76-offset-overlap" => check_offset(&old, &new),
                other => panic!("class {other}"),
            };
            println!("{} {} [{class}]: {what}", unit.id(), case.name);
            checked += 1;
        }
    }
    let total: usize = rows.values().map(BTreeMap::len).sum();
    println!("{checked} of {total} moved rows replayed");
    assert_eq!(checked, total);
}
