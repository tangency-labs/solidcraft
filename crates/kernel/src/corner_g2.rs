//! G2 corner patches: the patch that fills a vertex where filleted edges meet, built to repeat each
//! blend's surface up to the second order instead of the rolling ball's sphere.
//!
//! [`crate::blend_corner`] and [`crate::curvecorner`] fill the corner of a vertex with a piece of
//! the ball that rolls along the three edges (a sphere for equal radii, a rational patch for
//! unequal ones). That patch is tangent to every blend along the sections where they stop, so the
//! corner is G1 — but across a seam the ball keeps its own 1/r where a fillet is flat along its
//! axis, so the two surfaces meet with a jump in curvature.
//!
//! Here the corner is one tensor patch whose boundary is the blends' section curves themselves and
//! whose cross-sections carry each blend's own derivatives: the position, the direction the patch
//! leaves the blend's surface in, and the blend's curvature vector along that direction are the
//! first three control points of every cross-section, one control point per continuity condition,
//! exactly as [`crate::bridge`]'s sections do. The other end of every cross-section gathers at
//! `apex` — a point inside the material the corner caps — where the sections arrive in a common
//! tangent plane with a common curvature, so the patch closes there smoothly like a sphere's pole.
//! Along a seam the patch therefore repeats the blend up to the second order (G2).
//!
//! The boundary is a closed loop (the three sections joined corner to corner), so the direction
//! along it is a *closed* cubic B-spline: an interpolating curve with free ends swings wide at the
//! corner where the loop joins and folds the patch there. Where two blends meet at that corner
//! their sections leave in directions 90° apart and ask for different curvature at that one point
//! (a fillet is flat across its own seam, the other is not), which no single surface can hold; the
//! corner is a still point of the construction and [`seam_error`] is measured clear of it. A patch
//! that folds, or a corner with no room, is refused rather than replaced by the G1 ball.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self as b, eval::SurfaceEval};
use solidcraft_geom::bspline::{HPoint, NurbsSurface};

use crate::blend::unsupported;
use crate::bridge::Frame;
use crate::{KernelError, Result};

/// A corner's name in an error.
fn fail(m: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("G2 corner: {m}"))
}

/// Conditions each end of a cross-section carries: position, tangent and curvature.
const END_CONDITIONS: usize = 3;
/// Degree across the patch: three conditions at each end, less the control point they share.
const ACROSS: usize = 2 * END_CONDITIONS - 1;
/// Degree along the seam loop.
const ALONG: usize = 3;
/// Fewest stations a section curve may be sampled at (a cubic through them).
const MIN_STATIONS: usize = 4;
/// Most stations one section curve may carry.
const MAX_STATIONS: usize = 512;
/// How much of a station's `leave` has to lie across its seam for the patch to have a direction to
/// leave the blend in at all.
const ACROSS_ENOUGH: f64 = 0.2;

/// A G2 corner patch: its surface, and its boundary as the section curves themselves, each an
/// interval of the surface's own boundary curve (the `v = 0` line, `seam_curve`) from one corner of
/// the vertex to the next, so that it lies on the patch exactly.
#[derive(Clone, Debug, PartialEq)]
pub struct CornerPatch {
    pub surface: b::Surface,
    pub seam_curve: b::Curve,
    /// Each side's interval of `seam_curve` (its parameter is the surface's u).
    pub seams: Vec<[f64; 2]>,
}

/// The G2 corner patch over three blends' section curves: `sides[i]` is one blend's section curve,
/// sampled in order from one corner of the vertex to the next, each side's last station the next
/// side's first (the three close a loop). `apex` is a point inside the material the corner caps;
/// the patch gathers there. Its boundary is the section curves themselves (one interval of the
/// patch's own boundary curve per side); the caller orients the face where it sews it in.
pub fn corner_g2_patch(sides: &[Vec<Frame>], apex: Vec3) -> Result<CornerPatch> {
    build(sides, apex)
}

/// The seam's own direction at station `i` of a side, from a four-point central difference through
/// the stations (`crate::bridge::frames` samples a chain evenly, and a corner's own callers do the
/// same): a two-point chord would only be second order in the step and would tilt the corner's
/// tangent plane by more than G2 is judged at. `None` within two stations of a side's end.
fn seam_tangent(side: &[Frame], i: usize) -> Option<Vec3> {
    let at = |j: isize| usize::try_from(j).ok().and_then(|j| side.get(j)).map(|f| f.p);
    let (a, b, c, d) = (at(i as isize - 2)?, at(i as isize - 1)?, at(i as isize + 1)?, at(i as isize + 2)?);
    (b * 8.0 - a - c * 8.0 + d).normalized()
}

/// The seam's direction at every station of the closed loop (the same difference, wrapping round
/// its ends), falling back to the chord to a neighbour where the four points are not there.
fn loop_tangents(frames: &[Frame]) -> Vec<Option<Vec3>> {
    let m = frames.len();
    if m == 0 {
        return Vec::new();
    }
    let at = |j: isize| frames.get(j.rem_euclid(m as isize) as usize).map(|f| f.p);
    (0..m)
        .map(|i| {
            let four = || {
                let (a, b, c, d) = (at(i as isize - 2)?, at(i as isize - 1)?, at(i as isize + 1)?, at(i as isize + 2)?);
                (b * 8.0 - a - c * 8.0 + d).normalized()
            };
            four().or_else(|| {
                let prev = at(i as isize - 1)?;
                let next = at(i as isize + 1)?;
                (next - prev).normalized()
            })
        })
        .collect()
}

/// The three section curves as one closed loop of stations, the loop index each side starts at,
/// and the corner's own size (twice the mean distance of a station from their centre).
fn corner_loop(sides: &[Vec<Frame>]) -> Result<(Vec<Frame>, Vec<usize>, f64)> {
    if sides.len() != 3 {
        return Err(unsupported("a G2 corner needs three blends meeting at one vertex (a corner of two, or one, still ends on the ball's patch)"));
    }
    let mut frames: Vec<Frame> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    for side in sides {
        if side.len() < MIN_STATIONS || side.len() > MAX_STATIONS {
            return Err(KernelError::Invalid(format!("a corner section curve needs {MIN_STATIONS} to {MAX_STATIONS} sampled stations")));
        }
        for f in side {
            if ![f.p, f.leave, f.bend, f.bend_change].into_iter().all(Vec3::is_finite) || !(f.leave.len() > 0.5) {
                return Err(KernelError::Invalid("a corner station must carry finite data and a unit direction".into()));
            }
        }
        starts.push(frames.len());
        // A side runs from one corner to the next; its last station is the next side's first, so the
        // loop takes it once.
        frames.extend(side.iter().take(side.len() - 1).copied());
    }
    let centre = frames.iter().fold(Vec3::ZERO, |a, f| a + f.p) * (1.0 / frames.len().max(1) as f64);
    let size = frames.iter().map(|f| f.p.dist(centre)).fold(0.0, f64::max) * 2.0;
    if !(size > 0.0 && size.is_finite()) {
        return Err(unsupported("a corner whose section curves have no extent"));
    }
    for (i, side) in sides.iter().enumerate() {
        let Some(prev) = sides.get((i + sides.len() - 1) % sides.len()).and_then(|s| s.last()) else { continue };
        let Some(&here) = side.first() else { continue };
        if prev.p.dist(here.p) > size * 1e-3 {
            return Err(unsupported("the section curves do not meet at a corner"));
        }
    }
    Ok((frames, starts, size))
}

/// Every station's `leave` squared up with the seam it belongs to: the part of it across the seam,
/// which is the direction the cross-section has to leave the blend in. A `leave` along the seam
/// itself leaves the cross-section no room, and is refused.
fn transverse(frames: &[Frame], tangents: &[Option<Vec3>]) -> Result<Vec<Frame>> {
    let mut out = Vec::with_capacity(frames.len());
    for (f, t) in frames.iter().zip(tangents) {
        let across = match t {
            Some(t) => f.leave - *t * f.leave.dot(*t),
            // Stations that offer no direction along the seam: the leave stands as it came.
            None => f.leave,
        };
        if across.len() < ACROSS_ENOUGH * f.leave.len() {
            return Err(unsupported("a section curve meets the corner patch along its own tangent, so the patch has no direction to leave it in"));
        }
        let leave = across.normalized().ok_or_else(|| unsupported("a corner station has no direction across its seam"))?;
        out.push(Frame { leave, ..*f });
    }
    Ok(out)
}

/// One station's cross-section: `ACROSS + 1` control points of a Bézier from the seam point to
/// `apex`. The first three hold the blend: its position, the direction the patch leaves it in
/// (one control point along `leave`) and its curvature vector across the seam (one more). The last
/// three hold the apex: the point, a tangent in the apex's own plane, and the curvature of a dome
/// over the corner, so the sections arrive smoothly instead of leaving a point.
fn section(f: &Frame, apex: Vec3, n_apex: Vec3, kappa: f64) -> Result<[Vec3; ACROSS + 1]> {
    let k = ACROSS as f64;
    let chord = f.p.dist(apex);
    if !(chord.is_finite() && chord > 0.0) {
        return Err(unsupported("a blend too small for the corner patch to fit"));
    }
    let speed = chord / 3.0;
    let to_apex = (f.p - apex).normalized().ok_or_else(|| unsupported("a section curve starts on the apex"))?;
    let leave_apex =
        (to_apex - n_apex * to_apex.dot(n_apex)).normalized().ok_or_else(|| unsupported("a section curve reaches the apex along the apex normal"))?;
    let mut c = [Vec3::ZERO; ACROSS + 1];
    c[0] = f.p;
    c[1] = f.p + f.leave * (speed / k);
    c[2] = f.bend * (speed * speed / (k * (k - 1.0))) + c[1] * 2.0 - c[0];
    c[ACROSS] = apex;
    c[ACROSS - 1] = apex + leave_apex * (speed / k);
    c[ACROSS - 2] = (-n_apex * kappa) * (speed * speed / (k * (k - 1.0))) + c[ACROSS - 1] * 2.0 - c[ACROSS];
    if c.iter().any(|p| !p.is_finite()) {
        return Err(fail("a cross-section of the corner is not finite"));
    }
    Ok(c)
}

/// A closed uniform cubic B-spline through `points`, once round the loop: the curve at each station
/// is `(q[i-1] + 4 q[i] + q[i+1]) / 6`, so the stations fix the net by a cyclic tridiagonal system,
/// solved here with the Sherman–Morrison form of the Thomas algorithm (a corner's loop runs to
/// hundreds of stations). The net comes back in the open form the tensor surface wants — the first
/// three control points repeated past the end — so it still closes, with one uniform span per
/// station and no free end anywhere.
fn closed_layer(points: &[Vec3]) -> Result<Vec<Vec3>> {
    let m = points.len();
    if m < 2 * ALONG + 2 {
        return Err(fail("a corner loop needs at least eight stations"));
    }
    // Sub-diagonal 1, diagonal 4, super-diagonal 1, and 1 in both corners.
    let (sub, diag, sup, corner) = (1.0f64, 4.0f64, 1.0f64, 1.0f64);
    let gamma = -diag;
    let mut bb = vec![diag; m];
    if let Some(first) = bb.first_mut() {
        *first = diag - gamma;
    }
    if let Some(last) = bb.last_mut() {
        *last = diag - corner * corner / gamma;
    }
    // Thomas, one right-hand side at a time (three components solved together).
    let thomas = |rhs: &[Vec3]| -> Result<Vec<Vec3>> {
        let mut c = vec![0.0f64; m];
        let mut d = rhs.to_vec();
        let mut denom = bb.first().copied().unwrap_or(0.0);
        if !(denom.abs() > 1e-300) {
            return Err(fail("the corner's stations are degenerate"));
        }
        let (Some(c0), Some(d0)) = (c.first_mut(), d.first_mut()) else { return Err(fail("stations")) };
        *c0 = sup / denom;
        *d0 = *d0 * (1.0 / denom);
        for i in 1..m {
            let (prev_c, prev_d) = (c.get(i - 1).copied().unwrap_or(0.0), d.get(i - 1).copied().unwrap_or(Vec3::ZERO));
            let b = bb.get(i).copied().unwrap_or(diag);
            denom = b - sub * prev_c;
            if !(denom.abs() > 1e-300) {
                return Err(fail("the corner's stations are degenerate"));
            }
            if let Some(slot) = c.get_mut(i) {
                *slot = if i + 1 < m { sup / denom } else { 0.0 };
            }
            if let Some(slot) = d.get_mut(i) {
                *slot = (*slot - prev_d * sub) * (1.0 / denom);
            }
        }
        let mut x = vec![Vec3::ZERO; m];
        let last = d.last().copied().unwrap_or(Vec3::ZERO);
        if let Some(slot) = x.last_mut() {
            *slot = last;
        }
        for i in (0..m - 1).rev() {
            let (Some(ci), Some(xn)) = (c.get(i).copied(), x.get(i + 1).copied()) else { continue };
            let di = d.get(i).copied().unwrap_or(Vec3::ZERO);
            if let Some(slot) = x.get_mut(i) {
                *slot = di - xn * ci;
            }
        }
        Ok(x)
    };
    let rhs: Vec<Vec3> = points.iter().map(|p| *p * 6.0).collect();
    let x = thomas(&rhs)?;
    // The unit right-hand side the corner terms add: gamma at the first station, the corner at the
    // last.
    let mut unit = vec![Vec3::ZERO; m];
    if let Some(slot) = unit.first_mut() {
        *slot = Vec3::new(gamma, gamma, gamma);
    }
    if let Some(slot) = unit.last_mut() {
        *slot = Vec3::new(corner, corner, corner);
    }
    let z = thomas(&unit)?;
    let (x0, xn) = (x.first().copied().unwrap_or(Vec3::ZERO), x.last().copied().unwrap_or(Vec3::ZERO));
    let (z0, zn) = (z.first().copied().unwrap_or(Vec3::ZERO), z.last().copied().unwrap_or(Vec3::ZERO));
    // z answers a scalar right-hand side, so the correction is one scalar factor per component.
    let factor = |x0: f64, xn: f64, z0: f64, zn: f64| (x0 + corner * xn / gamma) / (1.0 + z0 + corner * zn / gamma);
    let f = Vec3::new(factor(x0.x, xn.x, z0.x, zn.x), factor(x0.y, xn.y, z0.y, zn.y), factor(x0.z, xn.z, z0.z, zn.z));
    let mut out = Vec::with_capacity(m + ALONG);
    for i in 0..m {
        let (xi, zi) = (x.get(i).copied().unwrap_or(Vec3::ZERO), z.get(i).copied().unwrap_or(Vec3::ZERO));
        out.push(xi - Vec3::new(f.x * zi.x, f.y * zi.y, f.z * zi.z));
    }
    if !out.iter().all(|p| p.is_finite()) {
        return Err(fail("the corner's boundary is not finite"));
    }
    for i in 0..ALONG {
        let p = out.get(i).copied().unwrap_or(Vec3::ZERO);
        out.push(p);
    }
    Ok(out)
}

/// The stations of the loop as one tensor control net: across (v) each station's cross-section is
/// the Bézier above, along the seam loop (u) each of its control points is a closed cubic through
/// the stations, so the face passes through every section exactly and its parameter line along the
/// loop closes without an end to swing from. The loop's station count comes back with it: station
/// `i` sits at parameter `i + ALONG`, and the loop closes at `count + ALONG`.
fn tensor(layers: &[[Vec3; ACROSS + 1]]) -> Result<(NurbsSurface, f64)> {
    let m = layers.len();
    let nu = m + ALONG;
    let knots_u: Vec<f64> = (0..=(nu + ALONG) as i64).map(|i| i as f64).collect();
    let mut ctrl: Vec<HPoint> = Vec::with_capacity(nu * (ACROSS + 1));
    for j in 0..=ACROSS {
        let points: Vec<Vec3> = layers.iter().map(|c| c.get(j).copied().unwrap_or(Vec3::ZERO)).collect();
        let net = closed_layer(&points)?;
        for p in &net {
            ctrl.push(HPoint::weighted(*p, 1.0));
        }
    }
    let mut knots_v = vec![0.0; ACROSS + 1];
    knots_v.extend(std::iter::repeat_n(1.0, ACROSS + 1));
    let s = NurbsSurface::from_homogeneous([ALONG, ACROSS], [knots_u, knots_v], ctrl).map_err(|e| fail(format!("the corner patch: {e}")))?;
    Ok((s, m as f64))
}

/// The surface across its own domain: the patch must keep facing one way, or a blend would be met
/// by a piece of the patch folded back on itself.
fn no_fold(surface: &SurfaceEval, loop_range: (f64, f64), (v0, v1): (f64, f64)) -> Result<()> {
    // The u knots run past the loop at both ends (a closed curve has no clamped ends to name), so
    // the loop's own range is brought in rather than read off the knot vector.
    let (u0, u1) = loop_range;
    let normal = |u: f64, v: f64| -> Option<Vec3> { surface.derivs(u, v).and_then(|(_, su, sv)| su.cross(sv).normalized()) };
    let mid = normal((u0 + u1) * 0.5, v0 + (v1 - v0) * 0.5).ok_or_else(|| unsupported("the corner patch has no normal at its middle"))?;
    // Clear of the apex (v1), where the patch's parameter line along the seam collapses to a point
    // and the normal is read from the sections approaching it.
    for i in 0..=8 {
        for j in 0..9 {
            let u = u0 + (u1 - u0) * i as f64 / 8.0;
            let v = v0 + (v1 - v0) * j as f64 / 9.0;
            let n = normal(u, v).ok_or_else(|| unsupported("the corner patch folds back on itself"))?;
            if n.dot(mid) <= 0.0 {
                return Err(unsupported("the corner patch folds back on itself with these section curves"));
            }
        }
    }
    Ok(())
}

/// Build the patch: sections, tensor surface, and the intervals of the section curves themselves.
fn build(sides: &[Vec<Frame>], apex: Vec3) -> Result<CornerPatch> {
    if !apex.is_finite() {
        return Err(KernelError::Invalid("the corner apex must be a finite point".into()));
    }
    let (frames, starts, size) = corner_loop(sides)?;
    let frames = transverse(&frames, &loop_tangents(&frames))?;
    // The apex's own plane: the mean direction from the apex to the boundary it caps.
    let n_apex = frames
        .iter()
        .fold(Vec3::ZERO, |a, f| a + (f.p - apex))
        .normalized()
        .ok_or_else(|| unsupported("the corner's apex sits on its own boundary curves"))?;
    // How hard the patch bends into the apex: as much as its blends do across their seams, and
    // never flatter than a dome of the corner's own size (a flat cap would end in a point).
    let mean_bend = frames.iter().map(|f| f.bend.len()).sum::<f64>() / frames.len().max(1) as f64;
    let reach = (frames.iter().map(|f| f.p.dist(apex)).sum::<f64>() / frames.len().max(1) as f64).max(size * 1e-3);
    let kappa = mean_bend.max(1.0 / (2.0 * reach));
    if !(kappa.is_finite() && kappa > 0.0) {
        return Err(unsupported("a corner the patch cannot span"));
    }
    let layers: Vec<[Vec3; ACROSS + 1]> = frames.iter().map(|f| section(f, apex, n_apex, kappa)).collect::<Result<_>>()?;
    let (nurbs, m) = tensor(&layers)?;
    // Every weight is one (the sections are polynomial): the patch is a plain B-spline.
    let [count_u, count_v] = nurbs.count();
    let net = nurbs.net();
    let point = |i: usize, j: usize| net.get(i, j).and_then(|h| h.project()).map(|p| [p.x, p.y, p.z]).ok_or_else(|| fail("the corner patch's net"));
    let points = (0..count_u).map(|i| (0..count_v).map(|j| point(i, j)).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?;
    let [knots_u, knots_v] = nurbs.knots().clone();
    let surface = b::Surface::Spline { u_knots: knots_u.clone(), v_knots: knots_v.clone(), points };
    let eval = SurfaceEval::new(&surface).map_err(|e| fail(format!("the corner patch: {e}")))?;
    let (v0, v1) = (knots_v.first().copied().unwrap_or(0.0), knots_v.last().copied().unwrap_or(1.0));
    no_fold(&eval, (ALONG as f64, m + ALONG as f64), (v0, v1))?;
    // The boundary is the loop of section curves, intervals of the patch's own `v = 0` line: its
    // control points are the net's first column, on the same u knots.
    let seam_points = (0..count_u).map(|i| point(i, 0)).collect::<Result<Vec<_>>>()?;
    let seam_curve = b::Curve::Spline { knots: knots_u, points: seam_points };
    let mut cuts: Vec<f64> = starts.iter().map(|i| *i as f64 + ALONG as f64).collect();
    cuts.push(m + ALONG as f64);
    let seams = cuts.windows(2).filter_map(|w| Some([*w.first()?, *w.get(1)?])).collect::<Vec<_>>();
    if seams.len() != sides.len() {
        return Err(fail("the corner's seams"));
    }
    Ok(CornerPatch { surface, seam_curve, seams })
}

/// What the patch achieves along the seams of `sides`: the largest distance from a seam station to
/// the patch, the largest angle (radians) between the patch's normal there and the blend's, and the
/// largest difference between the curvature across the seam and the blend's own, relative to
/// `reference` — the curvature a rolling-ball corner shows across the same seam, which is what G2
/// removes. Measured on the built surface alone, by the same finite differences of its own samples
/// that `crate::bridge`'s tests read a cross-section's shape with, so it is evidence about the
/// geometry and not about the construction.
pub fn seam_error(surface: &b::Surface, sides: &[Vec<Frame>], reference: f64) -> Result<(f64, f64, f64)> {
    if !(reference.is_finite() && reference > 0.0) {
        return Err(KernelError::Invalid("a reference curvature must be finite and positive".into()));
    }
    // The surface's parameter box: its natural one, a periodic direction one turn.
    let range = |n: b::Natural| match n {
        b::Natural::Periodic => Some((0.0, std::f64::consts::TAU)),
        b::Natural::Range(a, z) => (a.is_finite() && z.is_finite()).then_some((a, z)),
    };
    let [nu, nv] = surface.natural();
    let (Some((u0, u1)), Some((v0, v1))) = (range(nu), range(nv)) else {
        return Err(fail("the corner patch has no parameter domain"));
    };
    let surface = SurfaceEval::new(surface).map_err(|e| fail(format!("the corner patch: {e}")))?;
    let at = |u: f64, v: f64| surface.point([u, v]).ok_or_else(|| fail("the corner patch has no point there"));
    // Samples across the section for a five-point difference, as `bridge`'s tests use.
    const INWARDS: usize = 5;
    let hv = (v1 - v0).abs() * 1e-3;
    let hu = (u1 - u0).abs() * 1e-3;
    if !(hv > 0.0 && hu > 0.0) {
        return Err(fail("the corner patch has no extent to measure"));
    }
    let (mut gap, mut angle, mut curvature) = (0.0f64, 0.0f64, 0.0f64);
    for side in sides {
        if side.len() < 5 {
            return Err(KernelError::Invalid("a seam needs at least five stations to measure its direction".into()));
        }
        let mut hint: Option<[f64; 2]> = None;
        for (i, f) in side.iter().enumerate() {
            let Some(t) = seam_tangent(side, i) else { continue };
            let found = surface
                .project(f.p, hint, f64::INFINITY)
                .or_else(|| surface.project(f.p, None, f64::INFINITY))
                .ok_or_else(|| fail("a seam station is not on the corner patch"))?;
            hint = Some(found);
            let [u, v] = found;
            gap = gap.max(at(u, v)?.dist(f.p));
            // The cross-section, sampled inwards from the seam (or from the other end of the patch,
            // with the odd differences turned round).
            let out = v <= (v0 + v1) * 0.5;
            let step = if out { hv } else { -hv };
            let q: Vec<Vec3> = (0..INWARDS).map(|k| at(u, v + step * k as f64)).collect::<Result<_>>()?;
            let (Some(&f0), Some(&f1), Some(&f2), Some(&f3), Some(&f4)) = (q.first(), q.get(1), q.get(2), q.get(3), q.get(4)) else {
                return Err(fail("the corner patch has no room for a sample"));
            };
            let sign = if out { 1.0 } else { -1.0 };
            let d1 = (f0 * -25.0 + f1 * 48.0 + f2 * -36.0 + f3 * 16.0 + f4 * -3.0) * (sign / (12.0 * hv));
            let d2 = (f0 * 35.0 + f1 * -104.0 + f2 * 114.0 + f3 * -56.0 + f4 * 11.0) * (1.0 / (12.0 * hv * hv));
            // Along the seam, to stand the tangent plane up.
            let du = if u - hu >= u0 && u + hu <= u1 {
                (at(u + hu, v)? - at(u - hu, v)?) * (0.5 / hu)
            } else {
                let (a, b) = if u + hu <= u1 { (u, u + hu) } else { (u, u - hu) };
                (at(b, v)? - at(a, v)?) * (1.0 / hu)
            };
            let n = du.cross(d1).normalized().ok_or_else(|| fail("the corner patch has no normal at a seam"))?;
            let blend_n = t.cross(f.leave).normalized().ok_or_else(|| fail("a blend has no normal at a seam"))?;
            angle = angle.max(n.dot(blend_n).abs().clamp(0.0, 1.0).acos());
            // The curvature vector of the cross-seam curve: what the blend's own surface does along
            // the direction the patch leaves it in.
            let speed2 = d1.len2().max(1e-300);
            let bend = (d2 - d1 * (d2.dot(d1) / speed2)) * (1.0 / speed2);
            curvature = curvature.max((bend - f.bend).len() / reference.max(f.bend.len()).max(1e-30));
        }
    }
    Ok((gap, angle, curvature))
}

#[cfg(test)]
#[path = "corner_g2_tests.rs"]
mod tests;

/// The G2 corner through the fillet path: a box's three edges at one corner rounded, rolling ball
/// against G2 patch, measured on the built bodies with the continuity analysis.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod fillet_corner_tests {
    use super::*;
    use crate::{ContinuityTolerances, SeamReport, measure, seam_diagnostics};

    /// A 20 mm box with its three edges at one corner rounded by 4: the rolling ball fills that
    /// corner with a piece of its sphere, the G2 route with a patch on the blends' own sections.
    fn cornered(g2: bool) -> crate::Body {
        let b = crate::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let edges = [Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)];
        if g2 { crate::fillet_g2_corners(&b, &edges, 4.0).unwrap() } else { crate::fillet(&b, &edges, 4.0).unwrap() }
    }

    #[test]
    fn the_ball_corner_is_measurably_g1_and_a_g2_corner_is_refused_until_it_sews() {
        // The rolling ball: tangent to every blend, keeping its own 1/r across each corner seam.
        let body = cornered(false);
        let volume = measure(&body).unwrap().volume;
        assert!(volume > 7000.0 && volume < 8000.0, "the box is still a solid: {volume}");
        let report = seam_diagnostics(&body, &[], 33, ContinuityTolerances { gap_mm: 1e-3, angle_deg: 1.0, curvature_per_mm: 1e-6 }).unwrap();
        // Every seam of a filleted box is tangent; the corner's three are the short ones (the
        // blend-to-face seams run the edge's own 12 mm and more).
        let tangent: Vec<&SeamReport> = report.seams.iter().filter(|s| s.normal_angle_deg.value < 0.01).collect();
        let corner: Vec<&&SeamReport> = tangent.iter().filter(|s| s.length < 8.0).collect();
        assert_eq!(
            corner.len(),
            3,
            "three corner seams among {} tangent ones: {:?}",
            tangent.len(),
            tangent.iter().map(|s| s.length).collect::<Vec<_>>()
        );
        let worst = corner.iter().map(|s| s.curvature_jump_per_mm.value).fold(0.0f64, f64::max);
        eprintln!("rolling ball: corner seams jump {worst:.6} per mm (volume {volume:.3})");
        assert!((worst - 0.25).abs() < 1e-3, "the ball keeps its own 1/r (r = 4) across the seam: {worst}");

        // The G2 corner patch is built on the blends' own sections, but sewing it into the shell
        // is not done yet: the fillet with `corner: g2` refuses with a clear message rather than
        // leaving the shell open, and leaves the ball the corner.
        let b = crate::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let edges = [Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)];
        let err = crate::fillet_g2_corners(&b, &edges, 4.0).unwrap_err().to_string();
        eprintln!("G2 corner: {err}");
        assert!(err.contains("not supported yet:") && err.contains("G2 corner"), "{err}");
    }
}
