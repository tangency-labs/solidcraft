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
//! Run (about ten seconds):
//! `cargo test --profile goldens -p goldens --test wr76_wr79_flag_retirement_moves -- --include-ignored --nocapture`
//! The replay, `moved_rows_are_equal_or_closer`, is `#[ignore]`d because it re-runs whole
//! golden units; CI's goldens job already checks the new rows themselves. The file's other
//! three tests run by default: `shortened_trace_ends_on_the_found_curve` (the offset trace's
//! end, `gen0696`), `trace_keeps_a_curve_beside_the_found_segment` (the strict span test,
//! `intersect.surface_intersector_p4` `gen1081`) and
//! `trace_stopped_at_its_start_on_the_found_curve_flushes_nothing` (the lone start point,
//! p4 `gen0500`); each fails with its fix reverted.
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
//!   curve, status and count is unchanged. The test pins the slots by decoding the record:
//!   in `intersect.surface_intersector_p5` the only one is the state's `trace_dir` (index
//!   7); in `intersect.adv_surface_intersector` the record is walked op by op (each op's
//!   code, status and the values its arm writes, as the unit writes them) and every moved
//!   value must be the direction slot of a state (`put_state`'s value 8 of 14).
//! - **`wr76-p4-retrace`** (closer): the unit traces from a point on its one found curve, the
//!   segment `curves[0]` on both planes. Only the `flush` output moves. Each old and new 3D
//!   curve is a B-spline whose control points (so, by the convex hull property, every point)
//!   lie within the tolerance of that segment's line, of its span and of both planes: every
//!   old curve is a retrace of the found curve. The new curves span a sub-interval of the
//!   segment the old ones span, strictly shorter (or no curve), so the trace retraces less of
//!   a curve already found; the planes cross along a line (`planes_cross`: `sin^2` of their
//!   angle has a positive certified lower bound), so they touch nowhere, no new curve is a
//!   touching point, and every new curve type is one the old flush gave.
//!   (In `gen0153` the new curve's control hull still spans 1.7e-4 of the segment, 118
//!   tolerances: the curve's ends are exactly start points 0 and 2, the points the case adds
//!   to the curve itself with `add_curve`, and the trace from point 3 adds none. The curve
//!   itself spans about 1.12e-4 along the segment against 1.0e-4 between points 0 and 2:
//!   between its ends it bulges past each of them by about 6e-6, four tolerances, along the
//!   case's own `cpv` tangents at those points.)
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
//!   about the `z` axis where the offset of `toruswvy.smb` (a surface of revolution) touches
//!   itself, ran on from the free end of an earlier curve `C` of the same circle over a
//!   further arc (about 31 degrees); now it stops at that end, within the tolerance of `C`'s
//!   start. Everything before the curve's record is unchanged. The circle has no closed form
//!   (the profile is a rational B-spline), so the curves are bounded against the reference
//!   circle through `C`'s start (`z = z0`, `r = R`): each curve is refined by knot insertion
//!   into Bezier pieces (each knot span cut in 16), and per piece the Bernstein coefficients
//!   of `x^2 + y^2 - R^2` (degree 6) and of `z - z0` bound every point's distance to that
//!   circle (`circle_bound`, rounding and the refinement's own error counted). With the
//!   control point angles unwrapped (each piece within a convex wedge that misses the axis),
//!   the old curve covers `[a0, a1]`, the new one `[a0, b1]`, `b1` strictly inside, and `C`
//!   runs from `b1` to past `a1`. Over the dropped angles `[b1, a1]` both the old curve and
//!   `C` are within the tolerance of the circle, so within twice the tolerance of each other
//!   at every angle (same plane band, radii each within the bound of `R`): the dropped arc was
//!   a second trace of `C`. The new curve's certified bound to the circle is within the
//!   tolerance and no larger than the old curve's bound (bounds compared, not the curves'
//!   exact distances).
//!
//! Every computed distance, position and bound is compared at the far side of its rounding
//! bound, `gamma(k) M` (`gamma(k) = k u / (1 - k u)`, `M` the sum of the magnitudes of the
//! chain's terms; each chain's `k` is derived at its function):
//!
//! | Chain | `k` |
//! |-------|-----|
//! | distance of a point to a segment's line (`Segment::place`) | 5 on the vector, 6 on the norm |
//! | position along the segment, `t * len` (`Segment::place`) | 16 |
//! | distance to a plane (`PlaneOf::distance`) | 4 on the value, 15 times the normal's conditioning |
//! | `perp(D) + perp(K) + over` (`check_p5`) | 2 |
//! | Boehm refinement of a curve, per coordinate (`bezier_pieces`) | 14 per insertion |
//! | Bernstein coefficient of `x^2 + y^2 - R^2` (`circle_bound`) | 10 |
//! | distance between two points (end gaps) | 6 |
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
use solidcore::math::det;
use solidcore::nurbs::math::gamma;

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

/// `|x|` summed over the components.
fn abs_sum(a: P3) -> f64 {
    a[0].abs() + a[1].abs() + a[2].abs()
}

/// A found segment's line, a plane of the case, and the rounding bounds of the distances and
/// positions the certificates compute against them. Each bound is `gamma(k) * M`
/// (`solidcore::nurbs::math::gamma`, Higham's `k u / (1 - k u)`), `M` the sum of the
/// magnitudes of the expression's terms, and each comparison is `value + error <= bound`;
/// the rounding of the error terms themselves is below a part in `1e15` of them.
struct Segment {
    a: P3,
    b: P3,
    d: P3,
    len: f64,
}

/// Where a point lies against a segment: its parameter `t` over `a -> b`, its distance to the
/// line, and the rounding bounds of the distance and of `t * len`.
struct Placed {
    t: f64,
    dist: f64,
    dist_err: f64,
    along_err: f64,
}

impl Segment {
    fn new(a: P3, b: P3) -> Self {
        let d = sub(b, a);
        Self {
            a,
            b,
            d,
            len: norm(d),
        }
    }

    /// `t = (p - a).d / len^2` and the distance `|p - (a + t d)|`.
    ///
    /// - Distance: `a + t d` is a point of the exact line for the computed `t`, so the exact
    ///   distance is at most its distance from `p`. Each component of `p - (a + t d)` is four
    ///   roundings (`b - a`, `t *`, `a +`, `p -`) on terms of size `|p_i| + |a_i| + |t|
    ///   (|a_i| + |b_i|)`: `gamma(5)` of their sum covers the vector; the norm (three
    ///   products, two sums, a square root) adds `gamma(6)` of itself.
    /// - Along: `(p - a).d` is five roundings on `sum (|p_i| + |a_i|)(|a_i| + |b_i|) = S`,
    ///   `len^2` nine relative to itself, the division one, and `t * len` (or `(t - 1) *
    ///   len`) two more with `len` itself four: `gamma(16) (S / len + (|t| + 1) len)` bounds
    ///   the error of `t * len` and of `(t - 1) * len`.
    fn place(&self, p: P3) -> Placed {
        let (a, b, d, len) = (self.a, self.b, self.d, self.len);
        let t = dot(sub(p, a), d) / (len * len);
        let foot = [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]];
        let dist = norm(sub(p, foot));
        let m: f64 = (0..3)
            .map(|i| p[i].abs() + a[i].abs() + t.abs() * (a[i].abs() + b[i].abs()))
            .sum();
        let s: f64 = (0..3)
            .map(|i| (p[i].abs() + a[i].abs()) * (a[i].abs() + b[i].abs()))
            .sum();
        Placed {
            t,
            dist,
            dist_err: gamma(6) * dist + gamma(5) * m,
            along_err: gamma(16) * (s / len + (t.abs() + 1.0) * len),
        }
    }
}

/// A plane of the case: its origin, its computed unit normal, and `kappa = |x||y| / |x x y|`
/// of its axes, the conditioning of that normal.
struct PlaneOf {
    o: P3,
    n: P3,
    kappa: f64,
}

impl PlaneOf {
    /// `|(p - o).n|` and its rounding bound. The cross product is two roundings per
    /// component on terms below `|x||y|`, so its direction is off by at most `2 gamma(2)
    /// kappa`; the unit normal adds `gamma(6)`; together `gamma(10) kappa` per unit length
    /// of `p - o`. The dot product is four roundings on `sum |p_i - o_i||n_i|`: `gamma(15)
    /// kappa sum (|p_i| + |o_i|)` covers both.
    fn distance(&self, p: P3) -> (f64, f64) {
        let v = dot(sub(p, self.o), self.n).abs();
        let m = abs_sum(p) + abs_sum(self.o);
        (v, gamma(4) * v + gamma(15) * self.kappa * m)
    }
}

/// The computed parameter range `(t0, t1)` (over the segment `a -> b`) of `curves`' control
/// points and the largest rounding bound `e` of a `t` among them (the exact range lies in
/// `[t0 - e, t1 + e]` and covers `[t0 + e, t1 - e]`), after requiring every control point
/// within `tol` of the segment's line, of its span and of both planes, rounding counted.
fn on_found_segment(
    curves: &[Spline],
    seg: &Segment,
    planes: &[PlaneOf; 2],
    tol: f64,
) -> Option<(f64, f64, f64)> {
    let len = seg.len;
    let mut range: Option<(f64, f64, f64)> = None;
    for c in curves {
        c.check_plain();
        for p in c.points() {
            let pl = seg.place(p);
            assert!(
                pl.dist + pl.dist_err <= tol,
                "a control point off the found segment's line"
            );
            assert!(
                -pl.t * len + pl.along_err <= tol && (pl.t - 1.0) * len + pl.along_err <= tol,
                "a control point past the found segment"
            );
            for plane in planes {
                let (v, e) = plane.distance(p);
                assert!(v + e <= tol, "a control point off a plane");
            }
            let e = pl.along_err / len;
            range = Some(match range {
                None => (pl.t, pl.t, e),
                Some((lo, hi, m)) => (lo.min(pl.t), hi.max(pl.t), m.max(e)),
            });
        }
    }
    range
}

fn plane_of(s: &Value) -> PlaneOf {
    assert_eq!(s["kind"], "plane");
    let (x, y) = (p3(&s["x"]), p3(&s["y"]));
    let n = cross(x, y);
    let l = norm(n);
    PlaneOf {
        o: p3(&s["o"]),
        n: [n[0] / l, n[1] / l, n[2] / l],
        kappa: norm(x) * norm(y) / l,
    }
}

/// A closed interval holding an exact real. Each operation rounds its ends to nearest and then
/// widens them one step outward (`next_down`, `next_up`), which covers the rounding's half
/// step (also at a binade edge and in the subnormal range), so the exact result of the same
/// operation on any reals in the operands lies inside.
#[derive(Clone, Copy)]
struct Iv {
    lo: f64,
    hi: f64,
}

impl Iv {
    fn exact(x: f64) -> Self {
        assert!(x.is_finite());
        Self { lo: x, hi: x }
    }
    fn out(lo: f64, hi: f64) -> Self {
        assert!(lo.is_finite() && hi.is_finite());
        Self {
            lo: lo.next_down(),
            hi: hi.next_up(),
        }
    }
    fn add(self, o: Self) -> Self {
        Self::out(self.lo + o.lo, self.hi + o.hi)
    }
    fn sub(self, o: Self) -> Self {
        Self::out(self.lo - o.hi, self.hi - o.lo)
    }
    fn mul(self, o: Self) -> Self {
        let p = [
            self.lo * o.lo,
            self.lo * o.hi,
            self.hi * o.lo,
            self.hi * o.hi,
        ];
        Self::out(
            p.iter().copied().fold(f64::INFINITY, f64::min),
            p.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn sqr(self) -> Self {
        let (a, b) = (self.lo.abs(), self.hi.abs());
        if self.lo <= 0.0 && self.hi >= 0.0 {
            Self::out(0.0, a.max(b) * a.max(b)).max0()
        } else {
            Self::out(a.min(b) * a.min(b), a.max(b) * a.max(b))
        }
    }
    fn max0(self) -> Self {
        Self {
            lo: self.lo.max(0.0),
            hi: self.hi,
        }
    }
}

fn iv_cross(a: [Iv; 3], b: [Iv; 3]) -> [Iv; 3] {
    [
        a[1].mul(b[2]).sub(a[2].mul(b[1])),
        a[2].mul(b[0]).sub(a[0].mul(b[2])),
        a[0].mul(b[1]).sub(a[1].mul(b[0])),
    ]
}

fn iv_norm2(a: [Iv; 3]) -> Iv {
    a[0].sqr().add(a[1].sqr()).add(a[2].sqr())
}

/// A certified lower bound on `sin^2` of the angle between the planes `a` and `b` of a p4 case,
/// computed in interval arithmetic from the inputs' exact axes: with `N = x x y` per plane,
/// `sin^2 = |N_a x N_b|^2 / (|N_a|^2 |N_b|^2)`. The test requires it positive, so the exact
/// planes are not parallel and cross along a line.
fn planes_cross(a: &Value, b: &Value) -> f64 {
    let normal = |s: &Value| {
        assert_eq!(s["kind"], "plane");
        let (x, y) = (p3(&s["x"]), p3(&s["y"]));
        iv_cross(x.map(Iv::exact), y.map(Iv::exact))
    };
    let (na, nb) = (normal(a), normal(b));
    let num = iv_norm2(iv_cross(na, nb));
    let den = iv_norm2(na).mul(iv_norm2(nb));
    assert!(den.hi > 0.0, "a degenerate plane");
    let sin2 = (num.lo / den.hi).next_down().max(0.0);
    assert!(
        sin2 > 0.0,
        "the planes are not certified to cross (|N_a x N_b|^2 in [{:e}, {:e}])",
        num.lo,
        num.hi
    );
    sin2
}

/// `TsectCurveType::Touching`'s code in the record.
const TOUCHING: f64 = 1.0;

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
    let seg = Segment::new(p3(&curves[0]["a"]), p3(&curves[0]["b"]));
    let planes = [plane_of(&inputs["a"]), plane_of(&inputs["b"])];
    let tol = od.tol;
    assert_eq!(tol, nd.tol);
    let (oc, otypes) = od.flush.unwrap();
    let (nc, ntypes) = nd.flush.unwrap();
    // the planes cross along a line (certified), so they touch nowhere: no touching point,
    // and no type main did not give
    let sin2 = planes_cross(&inputs["a"], &inputs["b"]);
    assert!(
        !ntypes.contains(&TOUCHING),
        "a touching point between transversal planes: {ntypes:?}"
    );
    assert!(
        ntypes.iter().all(|t| otypes.contains(t)),
        "new types {ntypes:?} not among the old {otypes:?}"
    );
    let len = seg.len;
    // the new curves' exact range (widened by its rounding bound) lies within the old one's
    // and is strictly shorter than the old curves' exact range (narrowed by its own)
    let (o0, o1, eo) = on_found_segment(&oc, &seg, &planes, tol).expect("an old curve");
    let new_len = match on_found_segment(&nc, &seg, &planes, tol) {
        None => 0.0,
        Some((n0, n1, en)) => {
            assert!(
                n0 - en >= o0 - eo && n1 + en <= o1 + eo,
                "new spans more of the found curve"
            );
            n1 - n0 + 2.0 * en
        }
    };
    assert!(
        new_len < o1 - o0 - 2.0 * eo,
        "new retraces no less of the found curve"
    );
    let cps = |cs: &[Spline]| cs.iter().map(|c| c.cps.len()).collect::<Vec<_>>();
    format!(
        "planes cross (sin^2 >= {sin2:.3e}); {} old curve(s) {otypes:?} ({:?} control points) retrace {:.3e} of the found segment, {} new {ntypes:?} ({:?}) retrace {:.3e} (tol {tol:.3e})",
        oc.len(),
        cps(&oc),
        (o1 - o0) * len,
        nc.len(),
        cps(&nc),
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

/// How far `d` can be from `k`, rounding counted: see the module docs. The line and the
/// positions along it are those of [`Segment::place`] for the segment between `k`'s ends, and
/// each distance and position is taken at its rounding bound's far side.
fn duplicate_bound(d: &Spline, k: &Spline) -> f64 {
    let kp = k.points();
    let seg = Segment::new(kp[0], kp[kp.len() - 1]);
    let perp = |p: P3| {
        let pl = seg.place(p);
        pl.dist + pl.dist_err
    };
    let perp_k = kp.iter().map(|p| perp(*p)).fold(0.0, f64::max);
    let mut perp_d: f64 = 0.0;
    let mut over: f64 = 0.0;
    for p in d.points() {
        let pl = seg.place(p);
        perp_d = perp_d.max(pl.dist + pl.dist_err);
        over = over
            .max(-pl.t * seg.len + pl.along_err)
            .max((pl.t - 1.0) * seg.len + pl.along_err);
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
        // (the bound's two sums round by `gamma(2)` of it)
        assert!(
            best + gamma(2) * best <= tol,
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

/// Values `put_state` writes in `intersect.adv_surface_intersector`'s record, and the offset
/// of `curve_trace_direction` among them (closest value, two UVs, two nodes, the
/// `separate_gauss_maps` flag, then the direction).
const ADV_STATE_LEN: usize = 14;
const ADV_STATE_DIR: usize = 8;
/// Values `put_point` writes (`POINT_LEN` of the unit).
const ADV_POINT_LEN: usize = 80;
/// Values per solution `put_solutions` writes: type, variable count, start value, start
/// parameters (`MAX_PARAMS` = 10), end value, end parameters, two node indices.
const ADV_SOLUTION_LEN: usize = 26;
/// The op status of an op the runner does not run.
const ADV_REJECTED: f64 = -2.0;

/// The record offsets of every `curve_trace_direction` the adv record holds: per
/// `run_op` arm (op code, then its values) every `put_state` copy's, and the final
/// state's. Decodes the whole record and requires it consumed, so every offset is a
/// structural one.
fn adv_direction_slots(rec: &[f64], inputs: &Value) -> Vec<usize> {
    let mut r = Reader::new(rec);
    let mut slots = Vec::new();
    let state = |r: &mut Reader<'_>, slots: &mut Vec<usize>| {
        slots.push(r.i + ADV_STATE_DIR);
        r.n(ADV_STATE_LEN);
    };
    let solutions = |r: &mut Reader<'_>| {
        let n = r.g() as usize;
        r.n(n * ADV_SOLUTION_LEN);
    };
    let mut run_ops = true;
    if inputs["trees"].is_null() && inputs["build_trees"] == true {
        if r.g() == 0.0 {
            for _ in 0..2 {
                let n = r.g() as usize;
                r.n(n * 10);
            }
        } else {
            run_ops = false;
        }
    }
    if run_ops {
        for op in inputs["ops"].as_array().unwrap() {
            let (code, kind) = match op["op"].as_str().unwrap() {
                "state" => (1.0, 0),
                "set" => (2.0, 1),
                "push" => (3.0, 1),
                "sing" => (4.0, 2),
                "dupin" => (5.0, 3),
                "solve" => (6.0, 4),
                "best_sub" => (7.0, 5),
                "gauss" => (8.0, 6),
                "local" => (9.0, 7),
                "interior" => (10.0, 8),
                "solve_branch" => (11.0, 9),
                other => panic!("adv op {other}"),
            };
            assert_eq!(r.g(), code, "op code");
            let status = r.g();
            match kind {
                0 => {
                    assert_eq!(status, 0.0);
                    state(&mut r, &mut slots);
                }
                1 => {}
                2 => {
                    if status != ADV_REJECTED {
                        r.n(ADV_POINT_LEN);
                    }
                }
                3 => {
                    r.n(27);
                    if status == 0.0 {
                        r.n(14);
                        for _ in 0..2 {
                            let n = r.g() as usize;
                            r.n(n * 6);
                        }
                    }
                }
                4 => solutions(&mut r),
                5 => {
                    r.n(2);
                }
                6 => {
                    r.n(1);
                }
                7 => {
                    if status != ADV_REJECTED {
                        r.n(1);
                        solutions(&mut r);
                        state(&mut r, &mut slots);
                    }
                }
                8 => {
                    r.n(1);
                    state(&mut r, &mut slots);
                }
                _ => {
                    solutions(&mut r);
                    state(&mut r, &mut slots);
                }
            }
        }
    }
    state(&mut r, &mut slots);
    assert_eq!(r.i, rec.len(), "adv record not consumed");
    assert_eq!(
        slots.last(),
        Some(&(rec.len() - ADV_STATE_LEN + ADV_STATE_DIR))
    );
    slots
}

fn check_trace_direction(unit: &str, inputs: &Value, old: &Output, new: &Output) -> String {
    assert_eq!(old.values.len(), new.values.len());
    let mut flips = 0;
    for (key, o) in &old.values {
        let n = &new.values[key];
        assert_eq!(o.len(), n.len(), "{key}: length moved");
        let adv_slots = (unit == "intersect.adv_surface_intersector").then(|| {
            let s = adv_direction_slots(n, inputs);
            assert_eq!(s, adv_direction_slots(o, inputs), "{key}: layouts differ");
            s
        });
        for (i, (x, y)) in o.iter().zip(n).enumerate() {
            if x.to_bits() == y.to_bits() {
                continue;
            }
            assert!(
                *x == 1.0 && *y == -1.0,
                "{key}[{i}]: {x} -> {y} is not a direction flip"
            );
            match unit {
                // the status, then the state (decoded: `p5_decode` consumes the record),
                // whose `curve_trace_direction` is its seventh value
                "intersect.surface_intersector_p5" => {
                    p5_decode(n, true);
                    assert_eq!(i, 7, "not the trace_dir slot");
                }
                "intersect.adv_surface_intersector" => assert!(
                    adv_slots.as_ref().is_some_and(|s| s.contains(&i)),
                    "{key}[{i}]: not a put_state curve_trace_direction slot"
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

/// A circle about the `z` axis: `z = z0`, `x^2 + y^2 = r^2`.
#[derive(Clone, Copy)]
struct Circle {
    r: f64,
    z0: f64,
}

/// The circle about the `z` axis through `c`'s start point. The touching circle has no
/// closed form here (`toruswvy.smb` is a rational B-spline surface of revolution with a
/// wavy profile, and the circle is where its offset by `dist` touches itself), so the
/// curves are certified against this reference instead: two curves each within `b` of it
/// at one angle are within `2 b` of each other there (same height band, radii within `b`
/// of `r`), whatever the true circle is.
fn reference_circle(c: &Spline) -> Circle {
    let p = c.points()[0];
    Circle {
        r: det::hypot(p[0], p[1]),
        z0: p[2],
    }
}

/// The offset case's approximation tolerance (`approx`, the intersection curves' fit
/// tolerance).
fn offset_tol(inputs: &Value) -> f64 {
    inputs["approx"].as_f64().unwrap()
}

/// Bisections of every knot span before the Bernstein bounds are read (each span is cut
/// into `2^SUBDIVIDE` pieces).
const SUBDIVIDE: u32 = 4;

/// `c` refined by Boehm knot insertion into cubic Bezier pieces, each knot span cut into
/// `2^SUBDIVIDE` equal pieces: the pieces' control points (four each) and the coordinate
/// rounding bound `e` of every refined control point.
///
/// Each insertion forms `(1 - a) p + a q` with `a = (t - u_i) / (u_{i+3} - u_i)` in `[0, 1]`:
/// `a` is off by at most `gamma(3)` and `1 - a` by `gamma(4)`, the products and sum round by
/// `gamma(2) (|p| + |q|)` more, so each coordinate gains at most `gamma(4) (|p| + |q|) +
/// gamma(3) |q - p| <= gamma(14) M`, and the inputs' own errors pass through a convex
/// combination unamplified. So after `L` insertions every coordinate is within
/// `gamma(14 L) M` of the exact refined control point's (`M` the largest control point
/// coordinate magnitude; refined points stay in the hull), and by the partition of unity the
/// refined curve is within `e = sqrt(3) gamma(14 L) M` of `c` at every parameter.
fn bezier_pieces(c: &Spline) -> (Vec<[P3; 4]>, f64) {
    let deg = 3;
    assert_eq!(c.deg, deg);
    let mut pts = c.points();
    let mut knots = c.knots.clone();
    let m = pts
        .iter()
        .flat_map(|p| p.iter().map(|x| x.abs()))
        .fold(0.0, f64::max);
    // the parameters to insert: interior cuts of every span, then each interior knot up to
    // multiplicity `deg`
    let mut ins = Vec::new();
    let n = 1u32 << SUBDIVIDE;
    for w in knots.windows(2) {
        if w[1] > w[0] {
            for j in 1..n {
                let t = w[0] + (w[1] - w[0]) * f64::from(j) / f64::from(n);
                ins.extend([t; 3]);
            }
        }
    }
    let (k0, k1) = (knots[0], knots[knots.len() - 1]);
    let mut distinct: Vec<f64> = knots.clone();
    distinct.dedup();
    for u in distinct.iter().copied().filter(|u| *u > k0 && *u < k1) {
        let mult = knots.iter().filter(|x| **x == u).count();
        for _ in mult..deg {
            ins.push(u);
        }
    }
    for t in &ins {
        let t = *t;
        // the span `[u_s, u_{s+1})` holding `t`
        let s = knots
            .iter()
            .rposition(|u| *u <= t)
            .unwrap()
            .min(pts.len() - 1);
        let mut np = Vec::with_capacity(pts.len() + 1);
        np.extend_from_slice(&pts[..=s - deg]);
        for i in s - deg + 1..=s {
            let a = (t - knots[i]) / (knots[i + deg] - knots[i]);
            let (p, q) = (pts[i - 1], pts[i]);
            np.push([
                (1.0 - a) * p[0] + a * q[0],
                (1.0 - a) * p[1] + a * q[1],
                (1.0 - a) * p[2] + a * q[2],
            ]);
        }
        np.extend_from_slice(&pts[s..]);
        pts = np;
        knots.insert(s + 1, t);
    }
    assert_eq!((pts.len() - 1) % deg, 0, "not Bezier pieces");
    let pieces = (0..(pts.len() - 1) / deg)
        .map(|i| [pts[3 * i], pts[3 * i + 1], pts[3 * i + 2], pts[3 * i + 3]])
        .collect();
    let e = 3f64.sqrt() * gamma(14 * ins.len()) * m;
    (pieces, e)
}

/// The unwrapped angles of `pts` about the `z` axis, starting near `start`; each step is
/// under 90 degrees, so each Bezier piece's control points span a convex wedge that misses
/// the axis (asserted) and the piece lies in it.
fn unwrapped(pts: &[P3], start: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len());
    let mut prev = start;
    for p in pts {
        let a = det::atan2(p[1], p[0]);
        let k = ((prev - a) / std::f64::consts::TAU).round();
        let a = a + k * std::f64::consts::TAU;
        assert!(
            (a - prev).abs() < std::f64::consts::FRAC_PI_4 || out.is_empty(),
            "an angle step of 45 degrees or more"
        );
        out.push(a);
        prev = a;
    }
    out
}

/// A certified bound on the distance from `c` to the circle over the Bezier pieces whose
/// control point wedge meets the angles `[lo, hi]` (all of `c` for infinite ones), with `c`
/// unwrapped from `start`; and the angles of `c`'s first and last control points.
///
/// Per piece, with `f = x^2 + y^2 - r^2` (degree 6 in Bernstein form, coefficients
/// `c_k = sum_{i+j=k} C(3,i) C(3,j) / C(6,k) (x_i x_j + y_i y_j) - r^2`), every point is
/// within `max |c_k| / r` of the circle's cylinder (`|rho - r| = |f| / (rho + r)`) and within
/// `max |z_i - z0|` of its plane, so within their sum of the circle. Rounding: each `c_k`
/// is at most seven products and sums on `M_k = sum w (|x_i x_j| + |y_i y_j|) + r^2`, so
/// within `gamma(10) M_k`; the division and the heights add `gamma(2)` of themselves; and
/// the refinement adds its `e` twice (radially and in height).
fn circle_bound(c: &Spline, circle: Circle, start: f64, lo: f64, hi: f64) -> (f64, f64, f64) {
    c.check_plain();
    let pts = c.points();
    let ends = unwrapped(&pts, start);
    let (a_first, a_last) = (ends[0], ends[ends.len() - 1]);
    let (pieces, e) = bezier_pieces(c);
    // the pieces' angles, unwrapped along the refined control polygon from `c`'s start
    let flat: Vec<P3> = pieces.iter().flat_map(|q| q.iter().copied()).collect();
    let angles = unwrapped(&flat, a_first);
    let binom = |n: usize, k: usize| -> f64 {
        (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
    };
    let r2 = circle.r * circle.r;
    let mut worst: f64 = 0.0;
    for (i, q) in pieces.iter().enumerate() {
        let a = &angles[4 * i..4 * i + 4];
        let (w0, w1) = (
            a.iter().copied().fold(f64::INFINITY, f64::min),
            a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        assert!(w1 - w0 < std::f64::consts::FRAC_PI_2, "a wide piece");
        if w1 < lo || w0 > hi {
            continue;
        }
        let mut fmax: f64 = 0.0;
        for k in 0..=6usize {
            let (mut v, mut mag) = (0.0, r2);
            for i in k.saturating_sub(3)..=k.min(3) {
                let j = k - i;
                let w = binom(3, i) * binom(3, j) / binom(6, k);
                let (x, y) = (q[i][0] * q[j][0], q[i][1] * q[j][1]);
                v += w * (x + y);
                mag += w * (x.abs() + y.abs());
            }
            let ck = v - r2;
            fmax = fmax.max(ck.abs() + gamma(10) * mag);
        }
        let dr = fmax / circle.r;
        let dz = q
            .iter()
            .map(|p| (p[2] - circle.z0).abs())
            .fold(0.0, f64::max);
        let d = (dr + dz) * (1.0 + gamma(3)) + 2.0 * e;
        worst = worst.max(d);
    }
    (worst, a_first, a_last)
}

/// The moved curve's header in the offset record, its old and new curves, and the earlier
/// curve `C` whose start the new curve's end meets (with that gap).
struct OffsetMove {
    oc: Spline,
    nc: Spline,
    c: Spline,
    gap: f64,
}

fn offset_move(old: &Output, new: &Output) -> OffsetMove {
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
    let earlier: Vec<Spline> = (0..h).filter_map(|i| offset_curve_at(o, i)).collect();
    let np = nc.points();
    let end = np[np.len() - 1];
    let (gap, c) = earlier
        .iter()
        .map(|c| (norm(sub(c.points()[0], end)), c))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
    OffsetMove {
        oc: oc.clone(),
        nc: nc.clone(),
        c: c.clone(),
        gap,
    }
}

fn check_offset(inputs: &Value, old: &Output, new: &Output) -> String {
    let tol = offset_tol(inputs);
    let OffsetMove { oc, nc, c, gap } = offset_move(old, new);
    let circle = reference_circle(&c);
    let (op, np) = (oc.points(), nc.points());
    assert_eq!(op[0], np[0], "the curve's start moved");
    // (the distance's three differences, squares, sums and root round by `gamma(6)` of it)
    assert!(
        gap + gamma(6) * gap <= tol,
        "the new end is {gap:.3e} from every earlier curve's start (tol {tol:.3e})"
    );
    let all = (f64::NEG_INFINITY, f64::INFINITY);
    let start = det::atan2(op[0][1], op[0][0]);
    let (old_all, a0, a1) = circle_bound(&oc, circle, start, all.0, all.1);
    let (new_all, _, b1) = circle_bound(&nc, circle, start, all.0, all.1);
    let (_, c0, c1) = circle_bound(&c, circle, b1, all.0, all.1);
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
    // the new curve's certified bound to the circle is within the tolerance and no larger
    // than main's curve's (both are upper bounds with their rounding already counted, so
    // they are compared as they are)
    assert!(
        new_all <= tol && new_all <= old_all,
        "the new curve's bound to the circle is {new_all:.3e}, main's {old_all:.3e} (tol {tol:.3e})"
    );
    // over the angles the new curve no longer traces, main's curve and `C` are both within
    // the tolerance of the circle: the dropped arc was a second trace of `C`
    let (lo, hi) = if s > 0.0 { (b1, a1) } else { (a1, b1) };
    let (old_dropped, _, _) = circle_bound(&oc, circle, start, lo, hi);
    let (c_dropped, _, _) = circle_bound(&c, circle, b1, lo, hi);
    assert!(
        old_dropped <= tol && c_dropped <= tol,
        "over the dropped arc main's curve is {old_dropped:.3e} and C {c_dropped:.3e} from the circle (tol {tol:.3e})"
    );
    format!(
        "dropped {:.3} deg: main's curve within {old_dropped:.3e} and C within {c_dropped:.3e} of the circle there; whole curve {old_all:.3e} -> {new_all:.3e}; new end {gap:.3e} from C's start (tol {tol:.3e})",
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
                "wr79-trace-direction" => {
                    check_trace_direction(unit.id(), &case.inputs, &old, &new)
                }
                "wr76-p4-retrace" => check_p4(&case.inputs, &old, &new),
                "wr76-p5-duplicate" => check_p5(&case.inputs, &old, &new),
                "wr76-offset-overlap" => check_offset(&case.inputs, &old, &new),
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

/// One golden case's inputs and its output on this code.
fn run_case(unit: &str, name: &str) -> (Value, Output) {
    let u = registry().into_iter().find(|u| u.id() == unit).unwrap();
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let cases = u.cases(&ctx).unwrap();
    let case = cases.iter().find(|c| c.name == name).unwrap();
    (case.inputs.clone(), u.run_rust(case).unwrap())
}

/// WR-76: a near-tangent trace that reaches the free end of a curve already found stops
/// there, and its shortened last step ends on that curve's start. In `gen0696` the circle
/// where two offset surfaces touch is traced in two curves; the second used to run on over
/// about 31 degrees of the first (a second trace of it). A first-order shortened step left
/// its end 7e-4 short of the first's start; the secant corrections bring it within the
/// tolerance.
#[test]
fn shortened_trace_ends_on_the_found_curve() {
    let (inputs, out) = run_case("offset.offset_geometry_creation", "gen0696");
    let tol = offset_tol(&inputs);
    let v = &out.values["state"];
    let curves: Vec<Spline> = (0..v.len()).filter_map(|i| offset_curve_at(v, i)).collect();
    let last = curves.last().unwrap().points();
    let (start, end) = (last[0], last[last.len() - 1]);
    // the last curve is open: it stops at another curve's start
    assert!(
        norm(sub(start, end)) > 1.0,
        "the last curve closed on itself"
    );
    let gap = curves[..curves.len() - 1]
        .iter()
        .map(|c| norm(sub(c.points()[0], end)))
        .fold(f64::INFINITY, f64::min);
    // (the distance rounds by `gamma(6)` of itself)
    assert!(
        gap + gamma(6) * gap <= tol,
        "the last curve ends {gap:e} from every other curve's start (tol {tol:e})"
    );
}

/// The point of a plane input at `uv` (`o + u su X + v sv Y`; the test requires the axes
/// unit and perpendicular, so `SetCanonical` keeps them).
fn plane_point(plane: &Value, uv: &Value) -> P3 {
    let (o, x, y) = (p3(&plane["o"]), p3(&plane["x"]), p3(&plane["y"]));
    assert!(
        (norm(x) - 1.0).abs() <= 4.0 * f64::EPSILON && (norm(y) - 1.0).abs() <= 4.0 * f64::EPSILON
    );
    assert!(
        dot(x, y).abs() <= 4.0 * f64::EPSILON,
        "the axes are not perpendicular"
    );
    let sc = plane["scale"].as_array().unwrap();
    let (su, sv) = (sc[0].as_f64().unwrap(), sc[1].as_f64().unwrap());
    let (u, v) = (uv[0].as_f64().unwrap() * su, uv[1].as_f64().unwrap() * sv);
    [
        o[0] + u * x[0] + v * y[0],
        o[1] + u * x[1] + v * y[1],
        o[2] + u * x[2] + v * y[2],
    ]
}

/// WR-76: a trace near a found curve that is not on it keeps its curve. In
/// `intersect.surface_intersector_p4` case `gen1081` two planes cross along a line, the
/// found curve is a segment in plane A about 0.96 off plane B (5 tolerances, within the
/// ten-times-the-tolerance end acceptance of `IsPointOnCurve`), and the trace (from start
/// point 1, backwards) runs along the line through start point 0 to the edge `u = 0` of both
/// domains. The "already traced" test reads a span as on a found curve only when its start
/// and every sample are within the tolerance itself: with the acceptance a span was taken
/// for the segment and no curve was flushed.
///
/// Closed form: the line between `A(u0, v0)` (start point 0's `uv` on A) and `A(0, v0)`;
/// the test requires exactly one flushed curve, a crossing one, every control point within
/// the tolerance of both planes and of that chord, its ends within the tolerance of the two
/// points, and some control point farther than the tolerance from the found segment's line.
#[test]
fn trace_keeps_a_curve_beside_the_found_segment() {
    let (inputs, out) = run_case("intersect.surface_intersector_p4", "gen1081");
    let ops = inputs["ops"].as_array().unwrap();
    let d = p4_decode(&out.values["rec"], ops);
    let tol = d.tol;
    let found = &inputs["curves"][0];
    let found = Segment::new(p3(&found["a"]), p3(&found["b"]));
    let planes = [plane_of(&inputs["a"]), plane_of(&inputs["b"])];
    let uv0 = &ops
        .iter()
        .find(|o| o["op"] == "cpv" && o["i"].as_u64() == Some(0))
        .unwrap()["uvs"][0];
    let p0 = plane_point(&inputs["a"], uv0);
    // the domains' `u = 0` edge (both are `[0, 0, w, w]`)
    assert_eq!(inputs["dom1"][0], 0.0);
    assert_eq!(inputs["dom1"], inputs["dom2"]);
    let edge = plane_point(&inputs["a"], &serde_json::json!([0.0, uv0[1]]));
    let chord = Segment::new(p0, edge);
    let (flushed, types) = d.flush.unwrap();
    assert_eq!(types, [2.0], "one crossing curve");
    assert_eq!(flushed.len(), 1);
    let c = &flushed[0];
    c.check_plain();
    let pts = c.points();
    let mut far: f64 = 0.0;
    for p in &pts {
        for plane in &planes {
            let (v, e) = plane.distance(*p);
            assert!(v + e <= tol, "a control point off a plane");
        }
        let pl = chord.place(*p);
        assert!(
            pl.dist + pl.dist_err <= tol,
            "a control point off the chord"
        );
        let f = found.place(*p);
        far = far.max(f.dist - f.dist_err);
    }
    assert!(far > tol, "the curve lies on the found segment's line");
    let gap = |a: P3, b: P3| {
        let g = norm(sub(a, b));
        g + gamma(6) * g
    };
    let ends = gap(pts[0], p0).max(gap(pts[pts.len() - 1], edge));
    assert!(
        ends <= tol,
        "the curve's ends are {ends:e} from the closed form's"
    );
}

/// WR-76: a trace that stops at its lone start point, because its first span lies on a curve
/// already found, flushes nothing there. In `intersect.surface_intersector_p4` case `gen0500`
/// two planes cross at a right angle, the found curve is a segment of their line, and the
/// trace starts from a point of that segment: its first span lies on the segment, so the
/// trace stops with the start point as its only point. Main's flush emitted that point as a
/// touching point (its uniqueness test reads only the found curves' ends), which two planes
/// crossing along a line cannot have. The test certifies that the planes cross
/// (`planes_cross`) and requires a flush with no curve at all.
#[test]
fn trace_stopped_at_its_start_on_the_found_curve_flushes_nothing() {
    let (inputs, out) = run_case("intersect.surface_intersector_p4", "gen0500");
    let sin2 = planes_cross(&inputs["a"], &inputs["b"]);
    let d = p4_decode(&out.values["rec"], inputs["ops"].as_array().unwrap());
    let (flushed, types) = d.flush.expect("the case flushes");
    assert!(
        !types.contains(&TOUCHING),
        "a touching point between planes that cross (sin^2 >= {sin2:e}): {types:?}"
    );
    assert!(
        types.is_empty() && flushed.is_empty(),
        "the flush gave {} curve(s) {types:?}",
        flushed.len()
    );
}
