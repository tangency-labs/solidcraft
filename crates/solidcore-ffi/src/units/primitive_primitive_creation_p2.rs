//! Checked native `primitive.primitive_creation_p2` fixtures: the public
//! `SmPrimitiveCreation` entry points on a fresh native B-rep.
//!
//! Shim: `harness/cpp/shim/primitive_primitive_creation_p2.cpp`. Curves are p1's
//! `[kind, x, y, z, a, b, c]` records (kind 0 a B-spline line, kind 1 a quadratic
//! circular arc centred on xyz, radius a, angles b..c degrees). Frames are
//! `origin, x axis, y axis` (nine values). Layouts (`edit` is the B-rep's initial
//! editing flag):
//!
//! | op | entry point | data |
//! |---:|---|---|
//! | 1 | partial `CreateCone` | `edit h base top start end frame` |
//! | 2 | partial `CreateSphere` | `edit r start end frame` |
//! | 3 | partial `CreateTorus` | `edit major minor start end frame` |
//! | 4 | `CreateSkinPrimitive` (ownership report) | `edit approx degree cap profiles counts... curves...` |
//! | 5 | `CreateCurveSweepFromEdges`, no edges | `edit approx cap translational center perpendicular to_profile path` |
//! | 6 | `CreateCurveSweepFromFaces`, no faces | as op 5 |
//! | 7 | `CreatePipeSweep` | `edit radius approx cap translational path` |
//! | 8 | `CreateTaperExtrude` | `edit height draft caps nx ny nz corner curves...` |
//! | 9 | static `CreateSweepAlongPlanarPath` | `tol cap_start cap_end move profiles paths curves...` |
//!
//! Inputs upstream reads undefined memory for are rejected here: a taper with fewer
//! than two curves (UB-111 for none; one curve can make the sliver test read past
//! the split curve array, the dereference behind UB-112), a curve shorter than
//! `2e-5` (UB-112) or curves its chaining deletes as duplicates and reads again
//! (UB-50, both chaining passes, the reversed one where upstream may take it; with the
//! end cap at the offset end, a closed chain whose short curve meets a long one at a
//! corner sharper than `3e-5` radians, where upstream may reverse it), with the start cap
//! curves that close a loop while an end meets no other end or more than one (UB-247,
//! use after free), a planar path without path curves (`rPathCurves[0]`), skin
//! profile counts past the curves (`r3DCurves[lCount]`), a partial cone or torus that
//! sweeps a profile edge shorter than the zone tolerance (UB-215, use after free;
//! only where the swept face's degenerate-surface test does not certainly refuse it
//! first), and
//! a sphere, torus or cone a few zone tolerances across, at any turn (HANG-35).
//! Values are finite with magnitude at most `1e6`, frames nondegenerate, lists at
//! most 32 curves.

#![allow(clippy::indexing_slicing)] // fixed fixture layout validated before indexing

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_primitive_primitive_creation_p2_eval(
            op: i32,
            data: *const f64,
            n: i64,
            output: *mut f64,
            cap: i64,
            length: *mut i64,
        ) -> i32;
    }
}

const MAX_CURVES: usize = 32;

fn bad() -> FfiError {
    FfiError::InvalidArgument("primitive p2 layout or geometry".into())
}
fn integer(v: f64, max: f64) -> bool {
    v.is_finite() && v.fract() == 0.0 && (0.0..=max).contains(&v)
}
fn flag(v: f64) -> bool {
    integer(v, 1.0)
}
/// A buildable curve record (as p1's fixtures).
fn curve(v: &[f64]) -> bool {
    let [kind, x, y, z, a, b, c] = v else {
        return false;
    };
    if *kind == 0.0 {
        (a - x).powi(2) + (b - y).powi(2) + (c - z).powi(2) > 0.0
    } else {
        *kind == 1.0
            && *a >= 1e-15
            && (-360.0..=360.0).contains(b)
            && (-360.0..=360.0).contains(c)
            && c > b
            && c - b <= 360.0
    }
}
/// The extent of a curve record: its chord for a line, its radius for an arc.
fn extent(v: &[f64]) -> f64 {
    match v {
        [kind, x, y, z, a, b, c] if *kind == 0.0 => {
            ((a - x).powi(2) + (b - y).powi(2) + (c - z).powi(2)).sqrt()
        }
        [_, _, _, _, a, ..] => *a,
        _ => 0.0,
    }
}
fn curves(data: &[f64]) -> Option<Vec<&[f64]>> {
    let (records, rest) = data.as_chunks::<7>();
    if !rest.is_empty() || records.len() > MAX_CURVES {
        return None;
    }
    let records: Vec<&[f64]> = records.iter().map(<[f64; 7]>::as_slice).collect();
    records.iter().all(|r| curve(r)).then_some(records)
}
fn frame(f: &[f64]) -> bool {
    let [_, _, _, ax, ay, az, bx, by, bz] = f else {
        return false;
    };
    let cross = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];
    cross.iter().map(|v| v * v).sum::<f64>() > 1e-12
}

/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1e-12;
/// `SM_ZONE_TOL_3D`, the zone tolerance of the fresh native B-rep.
const ZONE_TOL: f64 = 1e-5;
/// `SM_ZONE_TOL_3D / 10`, the default 3D tolerance of `SmSurface::IsDegenerateCurve`
/// and `SmBSplineSurface::IsDegeneratePoint`.
const DEGENERATE_TOL: f64 = 1e-6;
/// Rounding margin of the degenerate-face predicate: a face counts as refused only
/// when its largest sampled value is below the tolerance by this share of it plus
/// [`REFUSAL_ABS_MARGIN`] times the size of its coordinates (native values are
/// evaluated from control points at that size, so their rounding grows with it).
const REFUSAL_REL_MARGIN: f64 = 1e-6;
/// See [`REFUSAL_REL_MARGIN`].
const REFUSAL_ABS_MARGIN: f64 = 1e-13;
/// From this turn (degrees) up, p3's `CreateRotationalSweep` builds analytic faces
/// instead of sweeping the profile edges; p2's full turns are among them.
const ANALYTIC_TURN: f64 = 359.99999;
/// HANG-35 ends here: every probe from this size up returned in milliseconds, while a
/// sphere of `1e-4` over 359 degrees took 20 s (fuzz allows 20 s per case).
const HANG_MAX: f64 = 1.5e-4;

type V3 = [f64; 3];
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn along(a: V3, s: f64, b: V3) -> V3 {
    [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn unit(a: V3) -> V3 {
    let n = norm(a);
    [a[0] / n, a[1] / n, a[2] / n]
}
fn max_dim(a: V3) -> f64 {
    a[0].abs().max(a[1].abs()).max(a[2].abs())
}

/// A cone profile line: its length, whether it is the axis line, and its ends as
/// (signed distance from the axis along the start x axis, height).
type ConeLine = (f64, bool, [(f64, f64); 2]);

/// The partial cone's profile lines as upstream builds them (side, base radius,
/// axis, top radius; a radius within the scaled zero is left out), each with its
/// length and whether it is the axis line, or `None` when the cone fails before
/// sweeping them. The frame is checked nondegenerate first.
fn cone_lines(d: &[f64]) -> Option<Vec<ConeLine>> {
    let (h, base, top, start, f) = (d[1], d[2], d[3], d[4], &d[6..15]);
    if h < EFF_ZERO || (d[5] - start).abs() < EFF_ZERO {
        return None;
    }
    let origin = [f[0], f[1], f[2]];
    let x = unit([f[3], f[4], f[5]]);
    let z = unit(cross(x, [f[6], f[7], f[8]]));
    let (sin, cos) = start.to_radians().sin_cos();
    let x_start = along([0.0; 3], cos, x);
    let x_start = along(x_start, sin, cross(z, x));
    let top_base = along(origin, h, z);
    let start_pt = along(origin, base, x_start);
    let end_pt = along(top_base, top, x_start);
    let tol = EFF_ZERO * (1.0 + max_dim(end_pt) + max_dim(start_pt));
    let (side, axis) = (norm(sub(end_pt, start_pt)), norm(sub(top_base, origin)));
    if side <= tol || axis <= tol {
        return None;
    }
    // ends as (signed distance along the start x axis, height)
    let lines: Vec<ConeLine> = [
        (side, false, [(base, 0.0), (top, h)]),
        (norm(sub(start_pt, origin)), false, [(0.0, 0.0), (base, 0.0)]),
        (axis, true, [(0.0, 0.0), (0.0, h)]),
        (norm(sub(top_base, end_pt)), false, [(top, h), (0.0, h)]),
    ]
    .into_iter()
    .filter(|(l, _, _)| *l > tol)
    .collect();
    (lines.len() >= 3).then_some(lines)
}

/// The `(u, v)` fractions of the domain `SmSurface::IsDegenerateCurve` evaluates the
/// first derivatives at: corners, edge midlines, diagonals and centre (`u` is the
/// sweep, `v` the profile curve).
const DEGENERATE_SAMPLES: [(f64, f64); 13] = [
    (0.0, 0.0),
    (1.0, 1.0),
    (0.0, 1.0),
    (1.0, 0.0),
    (0.5, 0.333),
    (0.5, 0.666),
    (0.333, 0.5),
    (0.666, 0.5),
    (0.333, 0.333),
    (0.666, 0.666),
    (0.333, 0.666),
    (0.666, 0.333),
    (0.5, 0.5),
];

/// The sweep circle `SmBSplineSurface::CreateSurfOfRevolution` builds for a turn
/// (`CreateCircleSegment(r, 0, turn, SM_CO_QUADRATIC)`, i.e. `N_CreateCircArc`):
/// the angle in degrees after `N_ConicData` normalises it (a turn below `1e-6`
/// degrees, or a negative one, wraps round to a nearly or fully closed circle) and
/// its span count. `None` for an angle the circle constructor refuses.
fn sweep_circle(turn: f64) -> Option<(f64, usize)> {
    if !(-360.0..=360.0).contains(&turn) {
        return None;
    }
    let tol = 1.0e-6;
    let mut ae = turn;
    while ae < tol {
        ae += 360.0;
    }
    while ae > 360.0 - tol {
        ae -= 360.0;
    }
    if ae < tol {
        ae = 360.0;
    }
    let spans = if ae < 91.0 {
        1
    } else if ae < 181.0 {
        2
    } else if ae < 271.0 {
        3
    } else {
        4
    };
    Some((ae, spans))
}

/// The unit quadratic circle of `theta` degrees in `spans` equal double-knotted spans
/// on `[0, 1]` at `u`: the angle of the point (radians) and the speed `|C'(u)|`.
fn circle_at(u: f64, theta: f64, spans: usize) -> (f64, f64) {
    let n = spans as f64;
    let phi = (theta / n).to_radians();
    let k = (u * n).floor().clamp(0.0, n - 1.0);
    let t = u * n - k;
    let w = (phi / 2.0).cos();
    let p = [[1.0, 0.0], [1.0, (phi / 2.0).tan()], [phi.cos(), phi.sin()]];
    let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t];
    let db = [-2.0 * (1.0 - t), 2.0 * (1.0 - 2.0 * t) * w, 2.0 * t];
    let weight = b[0] + b[1] + b[2];
    let dweight = db[0] + db[1] + db[2];
    let mut c = [0.0; 2];
    let mut dc = [0.0; 2];
    for (i, (ci, dci)) in c.iter_mut().zip(dc.iter_mut()).enumerate() {
        let num = b[0] * p[0][i] + b[1] * p[1][i] + b[2] * p[2][i];
        let dnum = db[0] * p[0][i] + db[1] * p[1][i] + db[2] * p[2][i];
        *ci = num / weight;
        *dci = (dnum * weight - num * dweight) / (weight * weight);
    }
    (c[1].atan2(c[0]) + k * phi, n * (dc[0] * dc[0] + dc[1] * dc[1]).sqrt())
}

/// A profile curve revolved by `CreateSurfOfRevolution` about the frame's z axis
/// from its start x axis.
struct Revolution {
    origin: V3,
    x: V3,
    y: V3,
    z: V3,
    theta: f64,
    spans: usize,
}

impl Revolution {
    /// The revolution of a frame `f` (nine values) turned to `start` degrees, over
    /// `turn` degrees; `None` when the sweep circle is refused.
    fn new(f: &[f64], start: f64, turn: f64) -> Option<Self> {
        let (theta, spans) = sweep_circle(turn)?;
        let x = unit([f[3], f[4], f[5]]);
        let z = unit(cross(x, [f[6], f[7], f[8]]));
        let (sin, cos) = start.to_radians().sin_cos();
        let x = along(along([0.0; 3], cos, x), sin, cross(z, x));
        Some(Self { origin: [f[0], f[1], f[2]], x, y: cross(z, x), z, theta, spans })
    }

    /// The point `rho` from the axis at height `h`, turned by `angle` radians.
    fn point(&self, rho: f64, h: f64, angle: f64) -> V3 {
        let (sin, cos) = angle.sin_cos();
        along(along(along(self.origin, h, self.z), rho * cos, self.x), rho * sin, self.y)
    }

    /// Whether `SweepEdgeHigh`'s degenerate-surface test certainly refuses the
    /// revolved face (`SM_ERR_INVALID_INPUT` before `MakeFaceWithCurves`). `net` is
    /// the profile's control points and `at(v)` its point and speed `(rho, h, |C'|)`
    /// on its `[0, 1]` domain, both as (distance from the axis, height).
    ///
    /// The surface is the profile turned rigidly, so at `(u, v)` `|Sv| = |C'(v)|` and
    /// `|Su| = |rho(v)| * s(u)`, `s` the sweep circle's speed. `IsDegenerateCurve`
    /// refuses it when `|Su|` or `|Sv|` is at most `tol` at all thirteen samples,
    /// `tol = max(1e-6, SM_EFF_ZERO * (1 + max dimension of the profile ends))`
    /// (its Su-parallel-to-Sv branch cannot apply: `Su` is around the axis, `Sv` in a
    /// plane through it). `IsDegeneratePoint` (`sm_IsNSrfDegenerate`) refuses it when
    /// the control net's box is at most `max(1e-6, 1000 * SM_EFF_ZERO * (1 + its max
    /// dimension))` along each axis; the net is each profile control point's sweep
    /// circle (a point within the scaled zero of the axis stays put).
    fn refuses(&self, net: &[(f64, f64)], at: impl Fn(f64) -> (f64, f64, f64)) -> bool {
        let ((r0, h0, _), (r1, h1, _)) = (at(0.0), at(1.0));
        let ends = max_dim(self.point(r0, h0, 0.0)).max(max_dim(self.point(r1, h1, 0.0)));
        let tol = DEGENERATE_TOL.max(EFF_ZERO * (1.0 + ends));
        let (mut su, mut sv) = (0.0f64, 0.0f64);
        for &(u, v) in &DEGENERATE_SAMPLES {
            let (rho, _, speed) = at(v);
            sv = sv.max(speed);
            su = su.max(rho.abs() * circle_at(u, self.theta, self.spans).1);
        }
        let phi = (self.theta / self.spans as f64).to_radians();
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        let mut add = |p: V3| {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        };
        for &(rho, h) in net {
            let on_axis = self.point(rho, h, 0.0);
            if rho.abs() < EFF_ZERO * (1.0 + max_dim(on_axis)) {
                add(on_axis);
                continue;
            }
            for k in 0..=2 * self.spans {
                let f = if k % 2 == 0 { 1.0 } else { 1.0 / (phi / 2.0).cos() };
                add(self.point(rho * f, h, k as f64 * phi / 2.0));
            }
        }
        let size = max_dim(lo).max(max_dim(hi));
        let point_tol = DEGENERATE_TOL.max(1000.0 * EFF_ZERO * (1.0 + size));
        let extent = (0..3).map(|i| hi[i] - lo[i]).fold(0.0, f64::max);
        let margin = |t: f64| t * (1.0 - REFUSAL_REL_MARGIN) - REFUSAL_ABS_MARGIN * (1.0 + size);
        su <= margin(tol) || sv <= margin(tol) || extent <= margin(point_tol)
    }
}

/// UB-215: a turn below [`ANALYTIC_TURN`] sweeps the profile edges one face at a
/// time (`SmTopologySweep::SweepEdgeHigh`). When the degenerate-surface test lets a
/// face through, `MakeFaceWithCurves` deletes every trimming curve whose
/// `ApproximateLength` is below the zone tolerance, the profile curve's swept copy
/// among them, and on success `pOrigEdgeCurve->GetOwner()` reads it after the free.
/// A sphere's line lies on the axis and its arc's face fails the degenerate test
/// first, so spheres never reach it; a cone's axis line sweeps to no face.
///
/// The guard refuses a cone with a non-axis line, or a torus with a minor circle,
/// shorter than the zone tolerance (`2 pi minor` for the circle, as before) whose
/// face [`Revolution::refuses`] does not certainly refuse. Its remaining
/// over-rejections: faces within the rounding margin of the degenerate test, a
/// short line behind a face refused earlier in the sweep, and tori whose face build
/// fails before the read (`SM_ERR`, native probes: most minor radii from about
/// `6.3e-7`).
fn sweeps_a_short_edge(op: i32, d: &[f64]) -> bool {
    match op {
        1 => {
            d[5] - d[4] < ANALYTIC_TURN
                && cone_lines(d).is_some_and(|lines| {
                    let sweep = Revolution::new(&d[6..15], d[4], d[5] - d[4]);
                    lines.iter().any(|&(l, axis, [(ra, ha), (rb, hb)])| {
                        !axis
                            && l < ZONE_TOL
                            && !sweep.as_ref().is_some_and(|s| {
                                s.refuses(&[(ra, ha), (rb, hb)], |v| {
                                    ((1.0 - v) * ra + v * rb, (1.0 - v) * ha + v * hb, l)
                                })
                            })
                    })
                })
        }
        3 => {
            let (major, minor) = (d[1], d[2]);
            d[4] - d[3] < ANALYTIC_TURN
                && minor > 0.0
                && std::f64::consts::TAU * minor < ZONE_TOL
                && !Revolution::new(&d[5..14], d[3], d[4] - d[3]).is_some_and(|s| {
                    // the minor circle: frame x -z, y -(start x), from -180 degrees
                    // when the radii are equal; a full circle in four spans
                    let b0 = if minor == major { -std::f64::consts::PI } else { 0.0 };
                    let net: Vec<(f64, f64)> = (0..9)
                        .map(|k| {
                            let f = if k % 2 == 0 { 1.0 } else { std::f64::consts::SQRT_2 };
                            let (sin, cos) = (b0 + f64::from(k) * std::f64::consts::FRAC_PI_4).sin_cos();
                            (major - minor * f * sin, -minor * f * cos)
                        })
                        .collect();
                    s.refuses(&net, |v| {
                        let (angle, speed) = circle_at(v, 360.0, 4);
                        let (sin, cos) = (b0 + angle).sin_cos();
                        (major - minor * sin, -minor * cos, minor * speed)
                    })
                })
        }
        _ => false,
    }
}

/// The UB-215 torus guard before it was narrowed to faces the degenerate test lets
/// through: any minor circle shorter than the zone tolerance. [`generator_refuses`]
/// keeps it so the generated cases stay the same draws.
fn short_torus_circle_v1(op: i32, d: &[f64]) -> bool {
    op == 3 && d[4] - d[3] < ANALYTIC_TURN && d[2] > 0.0 && std::f64::consts::TAU * d[2] < ZONE_TOL
}

/// The UB-215 cone guard before it was narrowed to the lines that can reach the freed
/// curve: any profile line, the axis line included, shorter than the zone tolerance.
/// [`generator_refuses`] keeps it so the generated cases stay the same draws.
fn short_cone_line_v1(op: i32, d: &[f64]) -> bool {
    op == 1
        && d[5] - d[4] < ANALYTIC_TURN
        && cone_lines(d).is_some_and(|l| l.iter().any(|&(l, _, _)| l < ZONE_TOL))
}

/// Whether the difftest generator draws again for this input: [`validate`] refuses
/// it, or the UB-215 guard as it was before it was narrowed refused it (the
/// generated cases must stay the same draws; the narrowed range is covered by
/// fixtures).
#[must_use]
pub fn generator_refuses(op: i32, data: &[f64]) -> bool {
    validate(op, data).is_err() || short_cone_line_v1(op, data) || short_torus_circle_v1(op, data)
}

/// HANG-35: a closed shell a few zone tolerances across takes minutes to place in
/// its region (the ray fire of `FindInfiniteRegion`), whether it is an analytic face
/// or a stitched sweep, at any turn. Cones that fail first (no height or turn, a
/// partial profile that is not built) and zero radii are kept.
fn builds_a_tiny_closed_shell(op: i32, d: &[f64]) -> bool {
    let size = match op {
        1 => {
            let turn = d[5] - d[4];
            let full = (turn - 360.0).abs() < 360.0 * EFF_ZERO;
            if d[1] < EFF_ZERO || turn.abs() < EFF_ZERO || !(full || cone_lines(d).is_some()) {
                return false;
            }
            d[1].max(d[2].abs()).max(d[3].abs())
        }
        2 if d[1] != 0.0 => d[1].abs(),
        3 if d[1] > 0.0 && d[2] > 0.0 => d[1] + d[2],
        _ => return false,
    };
    (ZONE_TOL / 2.0..HANG_MAX).contains(&size)
}

fn dist2(a: V3, b: V3) -> f64 {
    let d = sub(a, b);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// A curve record's start, end and mid-parameter points (an arc's mid parameter is
/// its mid angle).
fn points(v: &[f64]) -> [V3; 3] {
    match v {
        [kind, x, y, z, a, b, c] if *kind == 0.0 => {
            let (start, end) = ([*x, *y, *z], [*a, *b, *c]);
            [start, end, along(start, 0.5, sub(end, start))]
        }
        [_, x, y, z, r, b, c] => {
            let at = |deg: f64| {
                let (sin, cos) = deg.to_radians().sin_cos();
                [x + r * cos, y + r * sin, *z]
            };
            [at(*b), at(*c), at((b + c) / 2.0)]
        }
        _ => [[0.0; 3]; 3],
    }
}

/// A curve in a chain: its start, end and mid points in chain direction, the record it
/// came from, and whether the chain runs it backwards.
type Link = ([V3; 3], usize, bool);

/// One `SmCurve::FixChainCurvesList` pass as the taper calls it (`dTol = 2 * ZONE_TOL`,
/// deleting duplicates), on the curves' start, end and mid points: the chains in
/// upstream's order, their curves in its order and direction, or `None` when it
/// deletes one (UB-50). A curve is a duplicate when it matches the ends of the chain
/// built so far, not another curve: a square with a repeated side deletes nothing,
/// while a line from a chain's end back to its start, through its last link's mid
/// point, is deleted.
fn chain(curves: Vec<Link>) -> Option<Vec<Vec<Link>>> {
    let tol2 = (2.0 * ZONE_TOL).powi(2);
    let mut left: Vec<Option<Link>> = curves.into_iter().map(Some).collect();
    let mut chains = Vec::new();
    for kk in 0..left.len() {
        let Some(first) = left[kk].take() else {
            continue;
        };
        let mut chained = vec![first];
        let ([mut start, mut end, mid], _, _) = first;
        let (mut mid_start, mut mid_end) = (mid, mid);
        loop {
            let (mut best, mut next) = (f64::MAX, None);
            for (j, slot) in left.iter().enumerate() {
                let Some(([s1, e1, m1], _, _)) = *slot else {
                    continue;
                };
                let (s1e, e1e) = (dist2(s1, end), dist2(e1, end));
                let (e1s, s1s) = (dist2(e1, start), dist2(s1, start));
                // the four links in upstream's order; a later, closer one wins
                if s1e < tol2 && s1e < best {
                    if e1s < tol2 && dist2(mid_end, m1) < tol2 {
                        return None;
                    }
                    (best, next) = (s1e, Some((j, 1)));
                }
                if e1e < tol2 && e1e < best {
                    if s1s < tol2 && dist2(mid_end, m1) < tol2 {
                        return None;
                    }
                    (best, next) = (e1e, Some((j, 2)));
                }
                if e1s < tol2 && e1s < best {
                    if s1e < tol2 && dist2(mid_start, m1) < tol2 {
                        return None;
                    }
                    (best, next) = (e1s, Some((j, 3)));
                }
                if s1s < tol2 && s1s < best {
                    if e1e < tol2 && dist2(mid_start, m1) < tol2 {
                        return None;
                    }
                    (best, next) = (s1s, Some((j, 4)));
                }
            }
            let Some((j, link)) = next else {
                break;
            };
            let Some(([s1, e1, m1], record, backwards)) = left[j].take() else {
                break;
            };
            // 1 and 2 add at the end, 3 and 4 before the start; 2 and 4 reverse it
            let flip = link % 2 == 0;
            let (s1, e1) = if flip { (e1, s1) } else { (s1, e1) };
            let entry = ([s1, e1, m1], record, backwards != flip);
            if link <= 2 {
                chained.push(entry);
                (end, mid_end) = (e1, m1);
            } else {
                chained.insert(0, entry);
                (start, mid_start) = (s1, m1);
            }
        }
        chains.push(chained);
    }
    Some(chains)
}

/// The sharpest corner the offset end cap may not survive (radians), where a short curve
/// meets one more than [`SHORT_AGAINST_LONG`] times longer. Probed natively at caps 2:
/// slivers whose short link meets the long side at up to 1e-6 crash (1e-4 wide on a 100
/// side, 1e-3 on 1000), from 3e-6 they run, at heights 1 to 100 and drafts from -45 to 45
/// degrees; the margin is tenfold. A needle's tip, where two long sides meet even more
/// sharply, ran in every probe.
const SHARPEST_CAPPED_CORNER: f64 = 3e-5;

/// How much longer the other curve at a corner is when the short one makes the corner
/// count: links of up to 5e-4 of the long side crashed, 1e-3 ran; twentyfold margin.
const SHORT_AGAINST_LONG: f64 = 100.0;

/// A record's unit tangents at its start and end, in chain direction: a line's
/// direction, or an arc's (it turns from its start angle to its end angle about +Z of
/// its frame) at those angles. `None` for a line without length.
fn end_tangents(v: &[f64], backwards: bool) -> Option<(V3, V3)> {
    let (start, end) = match v {
        [kind, x, y, z, a, b, c] if *kind == 0.0 => {
            let d = sub([*a, *b, *c], [*x, *y, *z]);
            let length = norm(d);
            if length.is_nan() || length <= 0.0 {
                return None;
            }
            let u = [d[0] / length, d[1] / length, d[2] / length];
            (u, u)
        }
        [_, _, _, _, _, b, c] => {
            let sense = if c >= b { 1.0 } else { -1.0 };
            let at = |deg: f64| {
                let (sin, cos) = deg.to_radians().sin_cos();
                [-sin * sense, cos * sense, 0.0]
            };
            (at(*b), at(*c))
        }
        _ => return None,
    };
    let back = |t: V3| [-t[0], -t[1], -t[2]];
    Some(if backwards {
        (back(end), back(start))
    } else {
        (start, end)
    })
}

/// The sharpest corner of a chain that closes (to the chaining tolerance) where a curve
/// meets one more than [`SHORT_AGAINST_LONG`] times longer (lengths between their ends),
/// in radians: the angle between the way back along the arriving curve and the leaving
/// curve (0 where the chain turns straight back), the closing joint too. `None` for a
/// chain that stays open or has no such corner; 0 when a tangent cannot be taken.
fn sharpest_corner(chain: &[Link], records: &[&[f64]]) -> Option<f64> {
    let (Some(&([start, ..], _, _)), Some(&([_, end, _], _, _))) = (chain.first(), chain.last())
    else {
        return None;
    };
    if dist2(start, end) >= (2.0 * ZONE_TOL).powi(2) {
        return None;
    }
    let tangents: Option<Vec<(V3, V3)>> = chain
        .iter()
        .map(|&(_, record, backwards)| records.get(record).and_then(|r| end_tangents(r, backwards)))
        .collect();
    let Some(tangents) = tangents else {
        return Some(0.0);
    };
    let lengths: Vec<f64> = chain
        .iter()
        .map(|&([s, e, _], _, _)| norm(sub(e, s)))
        .collect();
    let mut sharpest = None;
    for (k, &(_, arriving)) in tangents.iter().enumerate() {
        let next = (k + 1) % tangents.len();
        let (Some(&(leaving, _)), Some(&a), Some(&b)) =
            (tangents.get(next), lengths.get(k), lengths.get(next))
        else {
            return Some(0.0);
        };
        if a.max(b) <= SHORT_AGAINST_LONG * a.min(b) {
            continue;
        }
        let back = [-arriving[0], -arriving[1], -arriving[2]];
        let cos =
            (back[0] * leaving[0] + back[1] * leaving[1] + back[2] * leaving[2]).clamp(-1.0, 1.0);
        let angle = cos.acos();
        sharpest = Some(sharpest.map_or(angle, |s: f64| s.min(angle)));
    }
    sharpest
}

/// The points `SmCurve::ComputeCrvsNormal` evaluates on a record, in chain direction:
/// a line (`CreateLineSegment`, two control points) at its ends, an arc
/// (`CreateCircleSegment`, quadratic) at six even parameters, taken here at six even
/// angles (its mid parameter is its mid angle; the other four lie within about a degree).
fn samples(v: &[f64], backwards: bool) -> Vec<V3> {
    let mut points = match v {
        [kind, x, y, z, a, b, c] if *kind == 0.0 => vec![[*x, *y, *z], [*a, *b, *c]],
        [_, x, y, z, r, b, c] => (0..=5)
            .map(|i| {
                let (sin, cos) = (b + (c - b) * f64::from(i) / 5.0).to_radians().sin_cos();
                [x + r * cos, y + r * sin, *z]
            })
            .collect(),
        _ => Vec::new(),
    };
    if backwards {
        points.reverse();
    }
    points
}

/// Whether upstream may reverse the chained curves: `CreateTaperExtrude` takes its second
/// pass when their computed normal is within 1e-4 radians of antiparallel to the supplied
/// one (`SmPrimitiveCreation.cpp:3693-3713`). The normal is `ComputeCrvsNormal`'s sum of
/// the cross products of consecutive sample chords, plus the closing corner when the
/// last point equals the first. It is false only when that sum surely points along
/// `normal`: a turn inside an arc keeps its sign however the arc is sampled, and a corner
/// between two lines is exact, while a corner at an arc within 30 degrees of straight or
/// of turning back, and the closing corner of a chain that ends where it starts, may take
/// either sign; those count against it at full size, and every other term at a fifth of
/// its size.
fn may_reverse(chains: &[Vec<Link>], records: &[&[f64]], normal: &[f64]) -> bool {
    let (Some(&nx), Some(&ny), Some(&nz)) = (normal.first(), normal.get(1), normal.get(2)) else {
        return true;
    };
    let length = (nx * nx + ny * ny + nz * nz).sqrt();
    if length.is_nan() || length <= 0.0 {
        return true;
    }
    let n = [nx / length, ny / length, nz / length];
    let dot = |a: V3| a[0] * n[0] + a[1] * n[1] + a[2] * n[2];
    // each chord with the curve it belongs to and whether that curve is an arc
    let mut chords: Vec<(V3, usize, bool)> = Vec::new();
    let (mut first, mut last) = (None, None);
    for (k, &(_, record, backwards)) in chains.iter().flatten().enumerate() {
        let Some(r) = records.get(record) else {
            return true;
        };
        let arc = r.first() != Some(&0.0);
        let points = samples(r, backwards);
        first = first.or_else(|| points.first().copied());
        last = points.last().copied().or(last);
        for pair in points.windows(2) {
            chords.push((sub(pair[1], pair[0]), k, arc));
        }
    }
    let turn = |a: V3, b: V3| {
        let (la, lb) = (norm(a), norm(b));
        if la > 0.0 && lb > 0.0 {
            ((a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (la * lb))
                .clamp(-1.0, 1.0)
                .acos()
        } else {
            0.0
        }
    };
    let near = 30f64.to_radians();
    let (mut sure, mut slack) = (0.0, 0.0);
    for pair in chords.windows(2) {
        let ((v1, k1, arc1), (v2, k2, arc2)) = (pair[0], pair[1]);
        let t = cross(v1, v2);
        let angle = turn(v1, v2);
        let unsure =
            k1 != k2 && (arc1 || arc2) && (angle < near || angle > std::f64::consts::PI - near);
        if unsure {
            slack += norm(t);
        } else {
            sure += dot(t);
            slack += 0.2 * norm(t);
        }
    }
    // the closing corner, which upstream adds only when the last point equals the first:
    // possible only when they are within the chaining tolerance here
    let closes =
        matches!((first, last), (Some(a), Some(b)) if dist2(a, b) < (2.0 * ZONE_TOL).powi(2));
    if let (true, Some(&(end, ..)), Some(&(start, ..))) = (closes, chords.last(), chords.first()) {
        slack += norm(cross(end, start));
    }
    sure <= slack
}

/// Ends this near meet: the tolerance the taper chains its curves with (`2 * ZONE_TOL`), and
/// the shortest curve kept here, so no curve's own two ends meet.
const JOIN: f64 = 2.0 * ZONE_TOL;

/// The root of `i` in a union-find over `root`, halving the path on the way.
fn find_root(root: &mut [usize], mut i: usize) -> usize {
    while root[i] != i {
        root[i] = root[root[i]];
        i = root[i];
    }
    i
}

/// UB-247 in `CreateTaperExtrude`'s start cap (`caps` 1 or 3): `CreatePlanarFaceWith3DCurves`
/// needs each curve end to meet exactly one other (`SmBrep.cpp:3800-3801`). Otherwise it can
/// fail after building edges on the input curves, and the taper, taking the failure for a
/// missing cap, deletes curves those edges still use (SIGSEGV in `SmEdgeuse::~SmEdgeuse`).
/// Probed natively, every crash had a closed loop among the curves and an end that met no
/// other end or more than one (a tail into a square, two squares at a corner, a spike from a
/// corner); curves that close no loop fail first and harmlessly, and so do clean loops, a
/// self-crossing bow-tie too. A repeat of another curve (the same ends either way and the same
/// mid point) is left out first: a square with a repeated or reversed side ran in every probe.
/// So, where the curves close a loop, every end must meet exactly one other within [`JOIN`].
fn start_cap_is_unsafe(records: &[&[f64]]) -> bool {
    let joined = |a: V3, b: V3| dist2(a, b) < JOIN * JOIN;
    let all: Vec<[V3; 3]> = records.iter().map(|r| points(r)).collect();
    let mut kept: Vec<[V3; 3]> = Vec::with_capacity(all.len());
    for &[s, e, m] in &all {
        let repeat = kept.iter().any(|&[s1, e1, m1]| {
            joined(m, m1) && ((joined(s, s1) && joined(e, e1)) || (joined(s, e1) && joined(e, s1)))
        });
        if !repeat {
            kept.push([s, e, m]);
        }
    }
    let ends: Vec<V3> = kept.iter().flat_map(|&[s, e, _]| [s, e]).collect();
    let loose = ends.iter().enumerate().any(|(i, &p)| {
        ends.iter()
            .enumerate()
            .filter(|&(j, &q)| j != i && joined(p, q))
            .count()
            != 1
    });
    if !loose {
        return false;
    }
    // a loop closes when a curve joins two ends already connected (union-find over the ends)
    let mut root: Vec<usize> = (0..ends.len()).collect();
    for i in 0..ends.len() {
        for j in i + 1..ends.len() {
            if joined(ends[i], ends[j]) {
                let (a, b) = (find_root(&mut root, i), find_root(&mut root, j));
                root[a] = b;
            }
        }
    }
    let mut curve_root: Vec<usize> = (0..ends.len()).collect();
    for k in 0..kept.len() {
        let (a, b) = (find_root(&mut root, 2 * k), find_root(&mut root, 2 * k + 1));
        let (a, b) = (find_root(&mut curve_root, a), find_root(&mut curve_root, b));
        if a == b {
            return true;
        }
        curve_root[a] = b;
    }
    false
}

/// UB-50 in `CreateTaperExtrude`: it first chains its curves. With a supplied normal
/// it may reverse them all and chain again, depending on their computed normal; that
/// second pass is checked unless the computed normal surely points along the supplied
/// one ([`may_reverse`]), so a few inputs it would not reverse are refused too. The end
/// cap at the offset end (`caps` 2 or 3) chains the trimmed offset curves through a copy
/// of their list, so a curve deleted there is freed again at cleanup; which offsets
/// coincide cannot be told without building them, so with that cap, where upstream may
/// reverse the curves (every such probe that crashed did), a closed chain with a short
/// curve meeting a long one at a corner sharper than [`SHARPEST_CAPPED_CORNER`] (the
/// slivers and back-and-forth loops of those probes) is refused. The offsets' chaining before the caps
/// (`SmPrimitiveCreation.cpp:3884`) is not checked.
fn taper_is_unsafe(caps: f64, normal: &[f64], records: &[&[f64]]) -> bool {
    let links = records
        .iter()
        .enumerate()
        .map(|(i, r)| (points(r), i, false))
        .collect();
    let Some(chains) = chain(links) else {
        return true;
    };
    let supplied = normal.iter().map(|v| v * v).sum::<f64>() >= 0.00001;
    let reversed = || {
        chains
            .iter()
            .flatten()
            .map(|&([s, e, m], record, backwards)| ([e, s, m], record, !backwards))
            .collect()
    };
    let reverses = supplied && may_reverse(&chains, records, normal);
    ((caps == 1.0 || caps == 3.0) && start_cap_is_unsafe(records))
        || (reverses && chain(reversed()).is_none())
        || (caps >= 2.0
            && reverses
            && chains
                .iter()
                .any(|c| sharpest_corner(c, records).is_some_and(|a| a < SHARPEST_CAPPED_CORNER)))
}

/// Validate the layout, flags, counts and the inputs upstream cannot run safely
/// (also used by the Rust runner).
pub fn validate(op: i32, data: &[f64]) -> Result<(), FfiError> {
    if data.is_empty() || !data.iter().all(|v| v.is_finite() && v.abs() <= 1e6) {
        return Err(bad());
    }
    let ok = match op {
        1 => data.len() == 15 && flag(data[0]) && frame(&data[6..15]),
        2 => data.len() == 13 && flag(data[0]) && frame(&data[4..13]),
        3 => data.len() == 14 && flag(data[0]) && frame(&data[5..14]),
        4 => {
            data.len() >= 5
                && flag(data[0])
                && data[1] >= 0.0
                && integer(data[2], 3.0)
                && data[2] >= 1.0
                && flag(data[3])
                && integer(data[4], MAX_CURVES as f64)
                && {
                    let profiles = data[4] as usize;
                    data.len() >= 5 + profiles && {
                        let counts = &data[5..5 + profiles];
                        let total: f64 = counts.iter().sum();
                        counts.iter().all(|c| integer(*c, MAX_CURVES as f64))
                            && curves(&data[5 + profiles..])
                                .is_some_and(|c| total <= c.len() as f64)
                    }
                }
        }
        5 | 6 => {
            data.len() == 14
                && flag(data[0])
                && data[1] >= 0.0
                && data[2..7].iter().all(|v| flag(*v))
                && curve(&data[7..14])
        }
        7 => {
            data.len() == 12
                && flag(data[0])
                && data[2] >= 0.0
                && flag(data[3])
                && flag(data[4])
                && curve(&data[5..12])
        }
        8 => {
            data.len() >= 8
                && flag(data[0])
                && integer(data[3], 3.0)
                && integer(data[7], 2.0)
                && curves(&data[8..]).is_some_and(|c| {
                    c.len() >= 2
                        && c.iter().all(|r| extent(r) >= 2e-5)
                        && !taper_is_unsafe(data[3], &data[4..7], &c)
                })
        }
        9 => {
            data.len() >= 6
                && data[0] > 0.0
                && data[1..4].iter().all(|v| flag(*v))
                && integer(data[4], MAX_CURVES as f64)
                && integer(data[5], MAX_CURVES as f64)
                && data[5] >= 1.0
                && data.len() == 6 + 7 * (data[4] as usize + data[5] as usize)
                && curves(&data[6..]).is_some()
        }
        _ => false,
    };
    // the layout is checked first: the guards index it
    if ok && !sweeps_a_short_edge(op, data) && !builds_a_tiny_closed_shell(op, data) {
        Ok(())
    } else {
        Err(bad())
    }
}

/// Run the native entry point: `status edit doing_boolean` map counts, start, side and
/// end face counts, then the op's extra values.
pub fn eval(op: i32, data: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, data)?;
    call_with_buffer(64, |output, length| {
        // SAFETY: layout, counts, flags and curve records checked above; all
        // input/output buffers remain live for the complete synchronous call.
        unsafe {
            sys::sc_primitive_primitive_creation_p2_eval(
                op,
                data.as_ptr(),
                data.len() as i64,
                output.as_mut_ptr(),
                output.len() as i64,
                &raw mut *length,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undefined_native_inputs_are_rejected() {
        // a taper of one curve, or of a curve shorter than 2e-5
        let line = [0., 0., 0., 0., 1., 0., 0.];
        let short = [0., 0., 0., 0., 1e-5, 0., 0.];
        let mut taper = vec![0., 1., 5., 3., 0., 0., 0., 0.];
        taper.extend(line);
        assert!(validate(8, &taper).is_err());
        taper.extend(short);
        assert!(validate(8, &taper).is_err());
        // a planar path without path curves
        let mut path = vec![1e-5, 1., 1., 1., 1., 0.];
        path.extend(line);
        assert!(validate(9, &path).is_err());
        // skin profile counts past the curves
        let mut skin = vec![0., 0., 1., 0., 2., 2., 1.];
        skin.extend(line);
        skin.extend(line);
        assert!(validate(4, &skin).is_err());
    }

    fn taper(caps: f64, normal_z: f64, curves: &[[f64; 7]]) -> Result<(), FfiError> {
        taper_at(5., caps, normal_z, curves)
    }

    fn taper_at(draft: f64, caps: f64, normal_z: f64, curves: &[[f64; 7]]) -> Result<(), FfiError> {
        let mut d = vec![0., 1., draft, caps, 0., 0., normal_z, 0.];
        d.extend(curves.iter().flatten());
        validate(8, &d)
    }
    fn line(s: [f64; 3], e: [f64; 3]) -> [f64; 7] {
        [0., s[0], s[1], s[2], e[0], e[1], e[2]]
    }

    #[test]
    fn taper_curves_its_chaining_deletes_are_rejected() {
        // UB-50, each segfaulting natively: two identical lines, a line and its
        // reverse, two identical arcs
        let x = line([0., 0., 0.], [1., 0., 0.]);
        let back = line([1., 0., 0.], [0., 0., 0.]);
        let arc = [1., 0., 0., 0., 1., 0., 180.];
        for caps in [0., 3.] {
            assert!(taper(caps, 1., &[x, x]).is_err());
            assert!(taper(caps, 1., &[x, back]).is_err());
            assert!(taper(caps, 1., &[arc, arc]).is_err());
        }
        // and a line from a chain's end back to its start through its last link's
        // mid point, a repeat of no curve; 2.5e-5 off that mid point it runs
        let tiny = [
            line([0., 0., 0.], [3e-5, 0., 0.]),
            line([3e-5, 0., 0.], [1., 0., 0.]),
            back,
        ];
        for z in [1., -1., 0.] {
            assert!(taper(1., z, &tiny).is_err());
        }
        let short = [
            line([0., 0., 0.], [5e-5, 0., 0.]),
            line([5e-5, 0., 0.], [1., 0., 0.]),
            back,
        ];
        for z in [1., -1., 0.] {
            assert!(taper(1., z, &short).is_ok());
        }
        // a line and its reverse that the first pass chains apart: the reversed second
        // pass deletes one, and segfaults natively under the normal the curves wind
        // against (-Z), at every cap. Under +Z and a computed normal upstream does not
        // take that pass and runs; the guard tells that from the curves' normal
        let apart = [
            line([0., 1., 0.], [0., 0., 0.]),
            line([1., 1., 0.], [0., 1., 0.]),
            line([0., 1., 0.], [1., 1., 0.]),
        ];
        for caps in [0., 1., 2., 3.] {
            assert!(taper(caps, -1., &apart).is_err());
            assert!(taper(caps, 1., &apart).is_ok());
            assert!(taper(caps, 0., &apart).is_ok());
        }
        // a half circle with its chord both ways: the same under -Z (segfaults natively),
        // and it runs under +Z and a computed normal; with one chord either way it runs
        // under every normal
        let (ab, ba) = (
            line([1., 0., 0.], [-1., 0., 0.]),
            line([-1., 0., 0.], [1., 0., 0.]),
        );
        for caps in [0., 1., 2., 3.] {
            for curves in [[arc, ab, ba], [arc, ba, ab]] {
                assert!(taper(caps, -1., &curves).is_err());
                assert!(taper(caps, 1., &curves).is_ok());
                assert!(taper(caps, 0., &curves).is_ok());
            }
            for z in [-1., 0., 1.] {
                assert!(taper(caps, z, &[arc, ab]).is_ok());
                assert!(taper(caps, z, &[arc, ba]).is_ok());
            }
        }
        // a link reversed as it joins decides the next duplicate test: these chain
        // apart and run natively (a replay that kept the link's direction deletes)
        let turned = [
            line([0.5, 0., 0.], [0., 1., 0.]),
            line([0.5, 0., 0.], [1., 1., 0.]),
            line([0., 1., 0.], [0.5, 0., 0.]),
        ];
        assert!(taper(1., 0., &turned).is_ok());
        // the end cap at the offset end chains the offsets: the short link's
        // back-and-forth loop at side 100 under -Z frees a curve twice natively (caps 2
        // and 3, not 0 and 1). A loop of no area may be reversed under either normal, so
        // with that cap it is refused under both, the short link at side 1 under +Z too,
        // which runs. The open chain of the line and its reverse is not tested, and runs
        let far = [
            line([0., 0., 0.], [5e-5, 0., 0.]),
            line([5e-5, 0., 0.], [100., 0., 0.]),
            line([100., 0., 0.], [0., 0., 0.]),
        ];
        for caps in [2., 3.] {
            assert!(taper(caps, -1., &far).is_err());
            assert!(taper(caps, 1., &short).is_err());
        }
        assert!(taper(1., -1., &far).is_ok());
        assert!(taper(3., 0., &apart).is_ok());
        // a sliver whose 1e-3 link meets the 100 side at 1e-6 radians segfaults natively
        // with that cap under -Z at a -24.9 degree draft (from 3e-6 it runs): refused
        // where upstream may reverse the curves, kept under +Z, where it runs, and
        // without the cap
        let sliver = [
            line([0., 0., 0.], [1e-3, 0., 0.]),
            line([1e-3, 0., 0.], [100., 1e-4, 0.]),
            line([100., 1e-4, 0.], [0., 0., 0.]),
        ];
        for caps in [2., 3.] {
            assert!(taper_at(-24.9, caps, -1., &sliver).is_err());
            assert!(taper_at(-24.9, caps, 1., &sliver).is_ok());
            assert!(taper_at(-24.9, caps, 0., &sliver).is_ok());
        }
        assert!(taper_at(-24.9, 1., -1., &sliver).is_ok());
        // a triangle 5e-5 tall on a side of 1 runs natively with both caps under every
        // normal, and is kept; so is a needle whose two long sides meet more sharply
        let low = [
            line([0., 0., 0.], [1., 0., 0.]),
            line([1., 0., 0.], [0.5, 5e-5, 0.]),
            line([0.5, 5e-5, 0.], [0., 0., 0.]),
        ];
        let needle = [
            line([0., 0., 0.], [1e-3, 0., 0.]),
            line([1e-3, 0., 0.], [100., 1., 0.]),
            line([100., 1., 0.], [0., 0., 0.]),
        ];
        for caps in [2., 3.] {
            for z in [-1., 0., 1.] {
                assert!(taper(caps, z, &low).is_ok());
                assert!(taper_at(-24.9, caps, z, &needle).is_ok());
            }
        }
        // a square with a repeated or reversed side deletes nothing in either pass,
        // keeps its area and runs natively with both caps, as do two halves of a
        // circle and a half circle's chord
        let [p0, p1, p2, p3] = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
        let square = [line(p0, p1), line(p1, p2), line(p2, p3), line(p3, p0)];
        for side in [
            line(p1, p2),
            line(p2, p1),
            line(p2, p3),
            line(p0, p1),
            line(p3, p0),
        ] {
            for z in [1., -1., 0.] {
                let mut curves = square.to_vec();
                curves.push(side);
                assert!(taper(3., z, &curves).is_ok());
            }
        }
        assert!(taper(3., 1., &[arc, [1., 0., 0., 0., 1., 180., 360.]]).is_ok());
        assert!(taper(3., 1., &[arc, line([-1., 0., 0.], [1., 0., 0.])]).is_ok());
    }

    #[test]
    fn a_start_cap_on_a_loop_with_a_loose_or_crowded_end_is_rejected() {
        // UB-247: each segfaults natively with the start cap (caps 1 and 3) under one normal
        // or another (fuzz00880's tail into a unit square ending at its corner, the same tail
        // stopping 1e-3 short and two squares at a corner under +Z and a computed normal, a
        // spike into the square from a corner under -Z), and is refused under every normal
        let ln = |a: [f64; 2], b: [f64; 2]| line([a[0], a[1], 0.], [b[0], b[1], 0.]);
        let square = [
            ln([1., 0.], [1., 1.]),
            ln([1., 1.], [0., 1.]),
            ln([0., 1.], [0., 0.]),
            ln([0., 0.], [1., 0.]),
        ];
        let with = |first: &[[f64; 7]], last: &[[f64; 7]]| [first, &square[..], last].concat();
        let lollipop = with(&[ln([0., 43.], [1., 0.])], &[]);
        let short_tail = with(&[ln([0., 10.], [1.001, 0.])], &[]);
        let spike = with(&[], &[ln([1., 0.], [0.3, 0.6])]);
        let other = [
            ln([2., 1.], [2., 2.]),
            ln([2., 2.], [1., 2.]),
            ln([1., 2.], [1., 1.]),
            ln([1., 1.], [2., 1.]),
        ];
        let corner = with(&[], &other);
        // a half circle across a side shares its ends but not its mid point, so it is no
        // repeat: it segfaults natively under every normal, bulging out or in
        let across = with(&[], &[[1., 0.5, 0., 0., 0.5, 180., 360.]]);
        for curves in [&lollipop, &short_tail, &spike, &corner, &across] {
            for caps in [1., 3.] {
                for z in [-1., 0., 1.] {
                    assert!(taper(caps, z, curves).is_err());
                }
            }
            // without the start cap they run natively and are kept
            for caps in [0., 2.] {
                assert!(taper(caps, 1., curves).is_ok());
            }
        }
        // kept with the start cap, each running natively: the square, a self-crossing
        // bow-tie, curves that close no loop (an open crossing, a tail into an open C),
        // two squares apart, and a square with a repeated or reversed side
        let bowtie = [
            ln([0., 0.], [1., 1.]),
            ln([1., 1.], [1., 0.]),
            ln([1., 0.], [0., 1.]),
            ln([0., 1.], [0., 0.]),
        ];
        let open_x = [
            ln([0., 0.], [1., 1.]),
            ln([1., 1.], [1., 0.]),
            ln([1., 0.], [0., 1.]),
        ];
        let open_c = [ln([0., 43.], [1., 0.]), square[0], square[1], square[2]];
        let apart: Vec<[f64; 7]> = [
            &square[..],
            &other.map(|r| [r[0], r[1] + 1., r[2], r[3], r[4] + 1., r[5], r[6]]),
        ]
        .concat();
        let repeated = with(&[], &[square[0]]);
        let reversed = with(&[], &[ln([1., 1.], [1., 0.])]);
        for curves in [
            &square[..],
            &bowtie,
            &open_x,
            &open_c,
            &apart,
            &repeated,
            &reversed,
        ] {
            for caps in [1., 3.] {
                assert!(taper(caps, 1., curves).is_ok());
                assert!(taper(caps, 0., curves).is_ok());
            }
        }
        // a third end needs to be within the chaining tolerance to crowd a corner: 4e-5 off
        // it is loose, so the lollipop is still refused (it segfaults natively too); a
        // separate square 4e-5 off the corner leaves every end of both squares met by
        // exactly one other and is kept (it runs)
        let near = with(&[ln([0., 43.], [1. + 4e-5, 0.])], &[]);
        assert!(taper(1., 1., &near).is_err());
        let off = other.map(|r| [r[0], r[1] + 4e-5, r[2], r[3], r[4] + 4e-5, r[5], r[6]]);
        assert!(taper(1., 1., &[&square[..], &off].concat()).is_ok());
    }

    #[test]
    fn a_corner_at_an_arc_near_turning_back_counts_against_the_normal() {
        // a shallow arc and its chord turn back at both ends (within 30 degrees), where the
        // sampled chords may not match upstream's: under its own normal the sum is not
        // sure, so the second pass stays checked; a half circle's corners are square, so
        // under its own normal it is sure
        let chained = |records: &[&[f64]]| {
            let links = records
                .iter()
                .enumerate()
                .map(|(i, r)| (points(r), i, false))
                .collect();
            chain(links).unwrap_or_default()
        };
        let shallow_arc = [1., 0., 0., 0., 1., 0., 20.];
        let (s, e) = ((20f64).to_radians().cos(), (20f64).to_radians().sin());
        let shallow_chord = line([s, e, 0.], [1., 0., 0.]);
        let shallow: [&[f64]; 2] = [&shallow_arc, &shallow_chord];
        assert!(may_reverse(&chained(&shallow), &shallow, &[0., 0., 1.]));
        let half_arc = [1., 0., 0., 0., 1., 0., 180.];
        let half_chord = line([-1., 0., 0.], [1., 0., 0.]);
        let half: [&[f64]; 2] = [&half_arc, &half_chord];
        assert!(!may_reverse(&chained(&half), &half, &[0., 0., 1.]));
        assert!(may_reverse(&chained(&half), &half, &[0., 0., -1.]));
    }

    fn primitive(op: i32, sizes: &[f64], start: f64, end: f64) -> Result<(), FfiError> {
        let mut d = vec![0.];
        d.extend(sizes);
        d.extend([start, end, 0., 0., 0., 1., 0., 0., 0., 1., 0.]);
        validate(op, &d)
    }

    #[test]
    fn short_swept_edges_are_rejected_and_early_exits_kept() {
        // UB-215 at its native edges: a 9e-6 cone line segfaults, 1e-5 does not
        assert!(primitive(1, &[1., 1., 9e-6], 0., 90.).is_err());
        assert!(primitive(1, &[2e-6, 1., 1.], 0., 90.).is_err());
        assert!(primitive(1, &[1., 1., 1e-5], 0., 90.).is_ok());
        // below 1e-6 the swept face is degenerate and refused first (1007 natively):
        // top, base and side lines, at 90 and 359 degrees
        assert!(primitive(1, &[1., 1., 9.9e-7], 0., 90.).is_ok());
        assert!(primitive(1, &[1., 1., 1e-9], 0., 90.).is_ok());
        assert!(primitive(1, &[1., 5e-7, 1.], -45., 314.).is_ok());
        assert!(primitive(1, &[9.9e-7, 1., 1.], 0., 90.).is_ok());
        // at 1e-6 rounding decides (it runs at unit size): refused, as just above
        assert!(primitive(1, &[1., 1., 1e-6], 0., 90.).is_err());
        assert!(primitive(1, &[1., 1., 1.01e-6], 0., 90.).is_err());
        assert!(primitive(1, &[1.01e-6, 1., 1.], 0., 90.).is_err());
        // the axis line sweeps to no face: a short axis alone runs (1025 natively)
        assert!(primitive(1, &[2e-6, 1., 2.], 0., 90.).is_ok());
        assert!(primitive(1, &[9e-6, 1., 2.], 0., 90.).is_ok());
        // a minor circle of 2 pi 1e-6 segfaults, 2 pi 2e-6 does not
        assert!(primitive(3, &[0., 1e-6], 0., 90.).is_err());
        assert!(primitive(3, &[1., 2e-6], 0., 90.).is_ok());
        // failures before the sweep stay compared: no height, no turn, a zero side,
        // a radius within the scaled zero (not built), a zero minor radius
        assert!(primitive(1, &[0., 1., 1.], 0., 90.).is_ok());
        assert!(primitive(1, &[1., 1., 1e-6], 30., 30.).is_ok());
        assert!(primitive(1, &[1e-12, 1., 1.], 0., 90.).is_ok());
        assert!(primitive(1, &[1., 1., 1e-13], 0., 90.).is_ok());
        assert!(primitive(3, &[1., 0.], 0., 90.).is_ok());
        // analytic faces sweep no edges; spheres never reach the freed curve
        assert!(primitive(1, &[1., 1., 1e-6], 0., 360.).is_ok());
        assert!(primitive(2, &[1e-6], 0., 90.).is_ok());
    }

    fn placed(op: i32, sizes: &[f64], start: f64, end: f64, origin: [f64; 3]) -> bool {
        let mut d = vec![0.];
        d.extend(sizes);
        d.extend([start, end]);
        d.extend(origin);
        d.extend([1., 0., 0., 0., 1., 0.]);
        validate(op, &d).is_ok()
    }

    #[test]
    fn the_ub215_guard_follows_the_native_degenerate_face_test() {
        // native boundaries (refused below, a freed-curve read above), bisected to 12
        // digits: the line's own |Sv| = L against 1e-6 ...
        let o = [0., 0., 0.];
        assert!(placed(1, &[1., 1., 0.999_998e-6], 0., 90., o));
        assert!(!placed(1, &[1., 1., 1.000_001e-6], 0., 90., o));
        // ... the sweep's |Su| at small turns (5.73685662283e-6 at 10 degrees,
        // 1.93185165258e-6 at 30) ...
        assert!(placed(1, &[1., 1., 5.736_8e-6], 0., 10., o));
        assert!(!placed(1, &[1., 1., 5.736_87e-6], 0., 10., o));
        assert!(placed(1, &[1., 1., 1.931_8e-6], 0., 30., o));
        assert!(!placed(1, &[1., 1., 1.931_86e-6], 0., 30., o));
        assert!(placed(1, &[1., 1., 9e-6], 0., 0.01, o));
        // ... where a turn below 1e-6 degrees (or a negative one) sweeps a nearly
        // closed circle (`N_ConicData`): a 1e-9 degree turn segfaults
        assert!(!placed(1, &[1., 1., 2e-6], 0., 1e-9, o));
        assert!(!placed(1, &[1., 1., 2e-6], 0., -30., o));
        assert!(placed(1, &[1., 1., 9.9e-7], 0., -30., o));
        // ... the control net's point-sized box (1000 * SM_EFF_ZERO * (1 + size)): 1,000
        // away it decides at 1.00100004374e-6 natively ...
        let far = [1000., -700., 300.];
        assert!(placed(1, &[1., 1., 1.0008e-6], 0., 90., far));
        assert!(!placed(1, &[1., 1., 1.0012e-6], 0., 90., far));
        // ... and refuses every line in the band from about 1e4 away
        assert!(placed(1, &[1., 1., 2e-6], 0., 90., [1e5, 0., 0.]));
        assert!(placed(1, &[1., 1., 9e-6], 0., 270., [9e5, -9e5, 9e5]));
        assert!(!placed(1, &[1., 1., 9e-6], 0., 270., [1e3, 0., 0.]));
        // the torus' minor circle: max |Sv| = 6.5073 minor (1.53673519e-7 natively at
        // major radii 0.01 and 1), at major radius 0 its |Su| decides with the turn
        assert!(placed(3, &[0.01, 1.536_7e-7], 0., 90., o));
        assert!(!placed(3, &[0.01, 1.536_8e-7], 0., 90., o));
        assert!(placed(3, &[1., 1.536_7e-7], 0., 1e-7, o));
        assert!(!placed(3, &[1., 1.536_8e-7], 0., 1e-7, o));
        assert!(placed(3, &[0., 3.969e-7], 0., -30., o));
        assert!(!placed(3, &[0., 3.971e-7], 0., -30., o));
        assert!(placed(3, &[0., 4.936e-7], 0., 359., o));
        assert!(!placed(3, &[0., 4.937e-7], 0., 359., o));
        // a point-sized torus far out is refused natively
        assert!(placed(3, &[2e-4, 1e-6], 0., 90., [1e6, 0., 0.]));
    }

    #[test]
    fn the_generator_keeps_the_earlier_ub215_guard() {
        let cone = |sizes: [f64; 3]| {
            let mut d = vec![0.];
            d.extend(sizes);
            d.extend([0., 90., 0., 0., 0., 1., 0., 0., 0., 1., 0.]);
            d
        };
        for sizes in [[1., 1., 9.9e-7], [2e-6, 1., 2.]] {
            assert!(validate(1, &cone(sizes)).is_ok());
            assert!(generator_refuses(1, &cone(sizes)));
        }
        assert!(!generator_refuses(1, &cone([1., 1., 0.5])));
        assert!(generator_refuses(1, &cone([1., 1., 9e-6])));
        let torus = [0., 0.01, 1.5e-7, 0., 90., 0., 0., 0., 1., 0., 0., 0., 1., 0.];
        assert!(validate(3, &torus).is_ok());
        assert!(generator_refuses(3, &torus));
    }

    #[test]
    fn tiny_closed_shells_are_rejected() {
        // HANG-35: a sphere of 1e-5 takes 87 s, 1e-4 over 359 degrees 20 s; 4e-6 and
        // 1.5e-4 take milliseconds, at any turn
        assert!(primitive(2, &[1e-5], 0., 360.).is_err());
        assert!(primitive(2, &[5e-5], 30., 390.000_000_003_6).is_err());
        assert!(primitive(2, &[1e-4], 0., 359.).is_err());
        assert!(primitive(2, &[1.5e-4], 0., 359.).is_ok());
        assert!(primitive(2, &[4e-6], 0., 360.).is_ok());
        assert!(primitive(1, &[2e-5, 2e-5, 2e-5], 0., 359.).is_err());
        assert!(primitive(1, &[1.5e-4, 1.5e-4, 1.5e-4], 0., 359.).is_ok());
        assert!(primitive(3, &[1e-5, 1e-5], 0., 360.).is_err());
        assert!(primitive(3, &[1., 1e-5], 0., 360.).is_ok());
        // a swept torus is stitched and placed the same way: 3e-6 by 3e-6 over 359
        // degrees takes minutes, 2e-6 by 2e-6 does not
        assert!(primitive(3, &[3e-6, 3e-6], 0., 359.).is_err());
        assert!(primitive(3, &[2e-6, 2e-6], 0., 359.).is_ok());
        // failures before any face is built stay compared
        assert!(primitive(2, &[0.], 0., 360.).is_ok());
        assert!(primitive(3, &[0., 1e-5], 0., 360.).is_ok());
        assert!(primitive(1, &[2e-5, 2e-5, 2e-5], 30., 30.).is_ok());
        assert!(primitive(1, &[1e-13, 2e-5, 2e-5], 0., 360.).is_ok());
    }
}
