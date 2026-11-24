//! The `intersect.surface_drop_lib_p3` rows the clamped-sliver drop fix moves, replayed
//! against main's values.
//!
//! The fix leaves out a first or last span of a dropped UV curve that clamping into the
//! domain collapses to a point (a *clamped sliver*: its polygon leaves the domain, its
//! clamped polygon is within the drop's UV tolerance and the curve piece it covers is
//! within the drop's 3D tolerance), and stretches the neighbouring span over the sliver's
//! range. Main's trim kept such a span as an out-and-back fold. For each moved row the
//! test takes main's `rec` values ([`MAIN_GEN0119`] and the rest, from a run of main's
//! drop tracer) and this branch's (run here), and checks:
//!
//! 1. main's values encode to main's committed golden line ([`MAIN_LINES`], copied from
//!    main's `intersect.surface_drop_lib_p3.jsonl`), and this branch's to the committed
//!    line;
//! 2. the two differ only in the curve that moved (the dropped curve and the record's copy
//!    of it, or the record's control polygon of a failed trace) and in the deviations the
//!    drop reports, each within the drop's tolerance;
//! 3. the moved curve is main's with exactly one end span removed: every other control
//!    point and knot is main's; the new end point is main's junction point; at most the
//!    inner control point next to it moves (the stretched span's tangent);
//! 4. the removed span of main's curve folds back on itself (it is not monotone in a
//!    coordinate), and every control point of the new curve lies in the domain.
//!
//! So each committed row is main's with the fold removed and nothing else changed beyond
//! the reported deviations, which stay within the drop's tolerance. The `face_p3`
//! row the fix moves (`own_reader_sphere_diff_3cylinders_uv_trim_f7_rebuild`, an edge
//! tolerance) is certified in `goldens::certify::trim_tolerance`, and the
//! `blend.fillet_executive_p3` rows (mass properties) in
//! `drop_clamped_sliver_moved_mass_rows.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)] // test

use goldens::case::repo_root;
use goldens::golden::{default_dir, diff, encode, read};
use goldens::{CaseContext, Output};

include!("drop_clamped_sliver_moved_rows/main_values.rs");

const UNIT: &str = "intersect.surface_drop_lib_p3";
/// The seeds the goldens run with (`--seeds` default).
const SEEDS: usize = 16;

/// Main's committed golden lines of the moved rows.
const MAIN_LINES: [&str; 4] = [
    r#"{"case":"gen0119","deferred":null,"error":null,"ints":{},"topology":null,"values":{"rec":{"blocks":[["-inf","inf","-inf"]],"n":138}}}"#,
    r#"{"case":"gen0649","deferred":null,"error":null,"ints":{},"topology":null,"values":{"rec":{"blocks":[[1088.890588438216,1088.890588438216,6455.066806365119]],"n":74}}}"#,
    r#"{"case":"gen0812","deferred":null,"error":null,"ints":{},"topology":null,"values":{"rec":{"blocks":[[4180.699273288129,4190.577912346329,209689.88911730016]],"n":94}}}"#,
    r#"{"case":"gen1306","deferred":null,"error":null,"ints":{},"topology":null,"values":{"rec":{"blocks":[["-inf","inf","-inf"]],"n":166}}}"#,
];

/// Which end span of the moved curve was removed.
#[derive(Clone, Copy, Debug)]
enum End {
    Lead,
    Tail,
}

/// Where the moved curve is.
#[derive(Clone, Copy, Debug)]
enum Place {
    /// The dropped curve (and the record's copy of it).
    Curve,
    /// The record's control polygon and knots of a failed trace.
    Polygon,
}

/// The moved rows: case, main's values, the curve that moved, the span removed.
const MOVED: [(&str, &[f64], Place, End); 4] = [
    ("gen0119", &MAIN_GEN0119, Place::Curve, End::Tail),
    ("gen0649", &MAIN_GEN0649, Place::Polygon, End::Lead),
    ("gen0812", &MAIN_GEN0812, Place::Polygon, End::Lead),
    ("gen1306", &MAIN_GEN1306, Place::Curve, End::Tail),
];

struct Reader<'a> {
    a: &'a [f64],
    i: usize,
}

impl Reader<'_> {
    fn one(&mut self) -> f64 {
        self.i += 1;
        self.a[self.i - 1]
    }
    fn many(&mut self, n: usize) -> Vec<f64> {
        self.i += n;
        self.a[self.i - n..self.i].to_vec()
    }
    fn count(&mut self) -> usize {
        let n = self.one();
        assert!(n >= 0.0 && n.fract() == 0.0, "count {n}");
        n as usize
    }
}

/// A Bezier-spline B-spline curve as the runner writes it.
#[derive(Clone, Debug, PartialEq)]
struct Curve {
    degree: f64,
    points: Vec<Vec<f64>>,
    knots: Vec<f64>,
    mults: Vec<f64>,
    interval: Vec<f64>,
}

fn curve(r: &mut Reader) -> Curve {
    let degree = r.one();
    let n = r.count();
    let points = (0..n).map(|_| r.many(4)).collect();
    let nk = r.count();
    let knots = r.many(nk);
    let mults = r.many(nk);
    let interval = r.many(2);
    Curve {
        degree,
        points,
        knots,
        mults,
        interval,
    }
}

fn curves(r: &mut Reader) -> Vec<Curve> {
    let n = r.count();
    (0..n).map(|_| curve(r)).collect()
}

/// The record of a drop (the runner's `put_record`), split into the parts the test reads.
#[derive(Clone, Debug, PartialEq)]
struct Record {
    /// Status, call, path, context, domain, interval and tolerance.
    head: Vec<f64>,
    /// The largest drop-to-surface argument, and the deviation.
    deviations: [f64; 2],
    /// The flags, with the largest drop-to-surface distance (`[5]`).
    flags: Vec<f64>,
    /// Singularities, poles, pole normals and solutions.
    rest: Vec<f64>,
    polygon: Vec<Vec<f64>>,
    knots: Vec<f64>,
    curves: Vec<Curve>,
}

impl Record {
    fn domain(&self) -> [f64; 4] {
        [self.head[4], self.head[5], self.head[6], self.head[7]]
    }
    fn tol(&self) -> f64 {
        self.head[10]
    }
}

fn record(r: &mut Reader) -> Record {
    let head = r.many(11);
    let deviations = [r.one(), r.one()];
    let flags = r.many(9);
    let mut rest = r.many(2);
    for width in [3, 3, 2] {
        let n = r.count();
        rest.push(n as f64);
        rest.extend(r.many(width * n));
    }
    let n = r.count();
    let polygon = (0..n).map(|_| r.many(3)).collect();
    let n = r.count();
    let knots = r.many(n);
    let curves = curves(r);
    Record {
        head,
        deviations,
        flags,
        rest,
        polygon,
        knots,
        curves,
    }
}

/// A drop's `rec` values: status, the reported distances, the curves and the record.
#[derive(Clone, Debug, PartialEq)]
struct Drop {
    status: f64,
    distances: [f64; 2],
    curves: Vec<Curve>,
    record: Record,
}

fn decode(a: &[f64]) -> Drop {
    let mut r = Reader { a, i: 0 };
    let status = r.one();
    let (distances, curves) = if status == 0.0 {
        ([r.one(), r.one()], curves(&mut r))
    } else {
        r.many(3);
        ([0.0; 2], Vec::new())
    };
    let record = record(&mut r);
    assert_eq!(r.i, a.len(), "trailing values");
    Drop {
        status,
        distances,
        curves,
        record,
    }
}

fn in_domain(p: &[f64], d: [f64; 4]) -> bool {
    d[0] <= p[0] && p[0] <= d[2] && d[1] <= p[1] && p[1] <= d[3]
}

/// Whether the points are monotone (non-decreasing or non-increasing) in coordinate `c`.
fn monotone(points: &[Vec<f64>], c: usize) -> bool {
    let up = points.windows(2).all(|w| w[0][c] <= w[1][c]);
    let down = points.windows(2).all(|w| w[0][c] >= w[1][c]);
    up || down
}

/// Checks that `new` is `old` with its `end` span removed (module docs, 3 and 4), on the
/// polygon and distinct knots of a Bezier spline.
fn one_span_removed(
    old: &[Vec<f64>],
    old_knots: &[f64],
    new: &[Vec<f64>],
    new_knots: &[f64],
    end: End,
    domain: [f64; 4],
) {
    let spans = old_knots.len() - 1;
    assert_eq!(
        old.len(),
        3 * spans + 1,
        "main's polygon is a Bezier spline"
    );
    let removed = match end {
        End::Lead => {
            if spans == 1 {
                assert!(new.is_empty() && new_knots.is_empty(), "{new:?}");
            } else {
                assert_eq!(new_knots[0], old_knots[0]);
                assert_eq!(new_knots[1..], old_knots[2..]);
                assert_eq!(new.len(), old.len() - 3);
                assert_eq!(new[0], old[3], "the new start is main's junction point");
                assert_eq!(new[2..], old[5..]);
            }
            &old[..4]
        }
        End::Tail => {
            assert!(spans > 1);
            let last = old_knots.len() - 1;
            assert_eq!(new_knots[..last - 1], old_knots[..last - 1]);
            assert_eq!(new_knots[last - 1], old_knots[last]);
            assert_eq!(new.len(), old.len() - 3);
            let junction = 3 * (spans - 1);
            assert_eq!(new[..junction - 1], old[..junction - 1]);
            assert_eq!(
                new[junction], old[junction],
                "the new end is main's junction point"
            );
            &old[junction..]
        }
    };
    assert!(
        !monotone(removed, 0) || !monotone(removed, 1),
        "the removed span folds back: {removed:?}"
    );
    for p in new {
        assert!(in_domain(p, domain), "{p:?} outside {domain:?}");
    }
}

/// The distinct knots of a curve; its multiplicities must be a Bezier spline's.
fn bezier_knots(c: &Curve) -> &[f64] {
    let n = c.mults.len();
    assert!(n >= 2 && c.mults[0] == 4.0 && c.mults[n - 1] == 4.0);
    assert!(c.mults[1..n - 1].iter().all(|&m| m == 3.0), "{:?}", c.mults);
    assert_eq!(c.degree, 3.0);
    &c.knots
}

fn check_curve(old: &Curve, new: &Curve, end: End, domain: [f64; 4]) {
    assert_eq!(new.degree, old.degree);
    assert_eq!(new.interval, old.interval);
    let (ok, nk) = (bezier_knots(old), bezier_knots(new));
    one_span_removed(&old.points, ok, &new.points, nk, end, domain);
}

fn this_branch(case: &str) -> Output {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let runner = goldens::registry()
        .into_iter()
        .find(|u| u.id() == UNIT)
        .unwrap();
    let c = runner
        .cases(&ctx)
        .unwrap()
        .into_iter()
        .find(|c| c.name == case)
        .unwrap_or_else(|| panic!("no case {case}"));
    runner.run_rust(&c).unwrap()
}

/// 1: main's values are main's lines, and this branch's run is the committed line, each
/// as the goldens compare them ([`diff`], the rule a bless keeps a line by).
#[test]
fn main_values_are_mains_lines() {
    let golden = read(&default_dir(), UNIT).unwrap();
    for ((case, main, _, _), line) in MOVED.iter().zip(MAIN_LINES) {
        let want: serde_json::Value = serde_json::from_str(line).unwrap();
        let got = encode(case, &Output::new().values("rec", main.to_vec()));
        assert_eq!(
            diff(&want, &got, &[]),
            None,
            "{case}: main's values are not main's line"
        );
        let ours = encode(case, &this_branch(case));
        let committed = golden.get(*case).unwrap();
        assert_eq!(
            diff(committed, &ours, &[]),
            None,
            "{case}: not the committed line"
        );
        assert!(
            diff(&want, &ours, &[]).is_some(),
            "{case}: the row did not move"
        );
    }
}

/// 2 to 4: each moved row is main's with one folded end span removed.
#[test]
fn each_moved_row_is_mains_without_a_folded_end_span() {
    for (case, main, place, end) in MOVED {
        let old = decode(main);
        let out = this_branch(case);
        let new = decode(&out.values["rec"]);
        let domain = old.record.domain();
        let tol = old.record.tol();
        let mut expect = old.clone();
        match place {
            Place::Curve => {
                assert_eq!(old.curves.len(), 1, "{case}");
                assert_eq!(new.curves.len(), 1, "{case}");
                check_curve(&old.curves[0], &new.curves[0], end, domain);
                assert_eq!(new.record.curves, new.curves, "{case}: the record's copy");
                expect.curves = new.curves.clone();
                expect.record.curves = new.record.curves.clone();
                // the drop reports what the stretched span's test measured
                for d in new.distances.iter().chain(&new.record.deviations) {
                    assert!(*d <= tol, "{case}: {d} past {tol}");
                }
                assert!(new.record.flags[5] <= tol, "{case}");
                expect.distances = new.distances;
                expect.record.deviations = new.record.deviations;
                expect.record.flags[5] = new.record.flags[5];
            }
            Place::Polygon => {
                assert_ne!(old.record.head[0], 0.0, "{case}: a failed trace's record");
                one_span_removed(
                    &old.record.polygon,
                    &old.record.knots,
                    &new.record.polygon,
                    &new.record.knots,
                    end,
                    domain,
                );
                expect.record.polygon = new.record.polygon.clone();
                expect.record.knots = new.record.knots.clone();
            }
        }
        assert_eq!(new, expect, "{case}: something else moved");
    }
}
