//! Rolling-ball blends along a closed chain of smooth edges between two smooth groups of faces
//! of any surfaces (a branch pipe meeting a main pipe, a boss on a cylinder). A ball rolls along
//! the edge touching both sides; its centre traces the spine and its contacts the curves where
//! the blend meets each side. The blend face is a rational B-spline through the circular
//! cross-sections (exact arcs at the samples), the side faces are trimmed back to the contact
//! curves, and seams that met the edge (lines, or arcs of an end face) are shortened to the
//! contacts.
//!
//! The ball's size along the chain is one of [`Size`]: one radius all along (the ordinary
//! fillet), a chord width across the blend (each section spans the requested width from contact
//! to contact, so its radius is `chord / (2 sin(ψ/2))` for the `ψ` between the two outward
//! normals where the ball touches), or a radius law along the chain (a [`RadiusSpline`] whose
//! positions are the fraction of the chain's arc length, zero only at an end, where the blend
//! closes to the edge's end point as the straight-edge fillet does).

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::blend::too_large;
use crate::blend_variable::RadiusSpline;
use crate::body::{Solid, from_p3, p3};
use crate::record::Rebuild;
use crate::{KernelError, Result};

fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

/// Samples per edge for the contact curves and the blend surface.
const SAMPLES: usize = 24;
const MAX_SAMPLES: usize = 369;
const MAX_CHAIN_SAMPLES: usize = 100_000;
/// Sections past a slanted end of an open chain.
const EXTENSION: usize = 8;
/// `1 + n1·n2` below this: a knife edge, where the faces' normals are opposite and the ball's
/// centre is found by sliding along the faces rather than from their normals.
const KNIFE: f64 = 1e-4;
/// An end face this close to square-on-to-its-sides' normal counts as tangent to a side.
const TANGENT_END: f64 = 1e-6;
/// Times the run past a tangent end is halved when the ball does not fit along it (and the run
/// past a curved end is shortened when its sections wrap round).
const TANGENT_RETRIES: usize = 8;
/// The longest step between the centres of neighbouring sections past an open chain's end, as a
/// multiple of the step along the end tangent: longer, and the sections have wrapped round a
/// curved edge's ring of centres.
const MAX_STEP_STRETCH: f64 = 3.0;
/// Slides of the ball's centre along a knife edge's faces, each 1.6 times the last.
const KNIFE_STEPS: usize = 40;
/// Corrections of the radius a chord fillet makes to the width it measures across the blend.
const CHORD_STEPS: usize = 12;
/// A section this close to a point is closed: the blend ends at the edge's end point (a radius
/// that has run to zero there).
const CLOSED: f64 = 1e-7;
/// Parameters an edge is sampled at to measure the arc length a radius law is read off.
const LENGTH_STEPS: usize = 64;

/// The ball's size along the edge chain.
#[derive(Clone, Copy)]
enum Size<'a> {
    /// One radius all along.
    Constant(f64),
    /// The width across the blend, from contact to contact, at every station.
    Chord(f64),
    /// The radius at the station's arc-length fraction along the chain, 0 at the front and 1 at
    /// the back of its edge.
    Law(&'a RadiusSpline),
}

struct Fit {
    surface: mt::Surface,
    contacts: [mt::Curve; 2],
    /// Parameter range belonging to this edge (the fit includes neighbouring sections).
    range: (f64, f64),
    /// The part of the fit that is the chain's own run, as fractions of its parameter range.
    span: (f64, f64),
}

struct Guide {
    sections: Vec<Section>,
    /// The size law's parameter (the arc-length fraction along the chain) at each section: the
    /// radius between the samples is read off the law there, not off the spine's own length.
    law: Vec<f64>,
    lengths: Vec<f64>,
    /// The part of the guide that is the chain itself, as fractions of its length: the sections
    /// run on past the ends of the chain (only so that the fit can be cut back there) and the fit
    /// is checked for accuracy on the chain alone.
    span: (f64, f64),
}

impl Guide {
    /// A guide whose law parameter is the section's own fraction of it (the size is one radius,
    /// which needs no law).
    fn new(sections: Vec<Section>) -> Result<Self> {
        let den = sections.len().max(2) - 1;
        let law = (0..sections.len()).map(|i| i as f64 / den as f64).collect();
        Self::with_law(sections, law)
    }

    fn with_law(sections: Vec<Section>, law: Vec<f64>) -> Result<Self> {
        if sections.len() != law.len() {
            return Err(unsupported("degenerate rolling-ball spine"));
        }
        let mut lengths = vec![0.0];
        let mut total = 0.0;
        for pair in sections.windows(2) {
            let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else { continue };
            total += a.c.dist(b.c);
            lengths.push(total);
        }
        if !total.is_finite() || total <= 1e-12 {
            return Err(unsupported("degenerate rolling-ball spine"));
        }
        Ok(Self { sections, law, lengths, span: (0.0, 1.0) })
    }

    /// The centre and tangent at `along` of the spine.
    fn anchor(&self, along: f64) -> Option<(Vec3, Vec3)> {
        self.anchor_at(along).map(|(centre, tangent, _)| (centre, tangent))
    }

    /// [`Guide::anchor`] with the size law's parameter at that station.
    fn anchor_at(&self, along: f64) -> Option<(Vec3, Vec3, f64)> {
        if self.lengths.len() < 2 || !along.is_finite() || !(0.0..=1.0).contains(&along) {
            return None;
        }
        let distance = self.lengths.last()? * along;
        let hi = self.lengths.partition_point(|length| *length < distance).clamp(1, self.lengths.len().checked_sub(1)?);
        let (a, b) = (self.sections.get(hi - 1)?, self.sections.get(hi)?);
        let (lo, hi_length) = (self.lengths.get(hi - 1)?, self.lengths.get(hi)?);
        let span = hi_length - lo;
        if span <= 1e-12 {
            return None;
        }
        let fraction = (distance - lo) / span;
        let law = self.law.get(hi - 1)? * (1.0 - fraction) + self.law.get(hi)? * fraction;
        Some((a.c + (b.c - a.c) * fraction, (b.c - a.c).normalized()?, law))
    }
}

/// One interpolation for both the face and its boundary curves. Fitting the contacts
/// independently gives different end conditions and leaves gaps between the same faces.
fn fit_sections(sections: &[Vec<Section>], k: usize, closed: bool, round: bool) -> Result<Fit> {
    let fail = |m: &str| KernelError::Failed(format!("curve blend: {m}"));
    let n = sections.len();
    let list = sections.get(k).ok_or_else(|| fail("section"))?;
    let mut before = Vec::new();
    if closed || k > 0 {
        let prev = sections.get((k + n - 1) % n).ok_or_else(|| fail("section"))?;
        before.extend(prev.iter().rev().skip(1).take(2).rev().copied());
    }
    let mut after = Vec::new();
    if closed || k + 1 < n {
        let next = sections.get((k + 1) % n).ok_or_else(|| fail("section"))?;
        after.extend(next.iter().skip(1).take(2).copied());
    }
    fit_strip(list, &before, &after, round)
}

/// [`fit_sections`] with the neighbouring sections given: best spaced as the strip's own, since
/// the interpolation's parameters are uniform.
fn fit_strip(list: &[Section], before: &[Section], after: &[Section], round: bool) -> Result<Fit> {
    use mt::Cut;
    let fail = |m: &str| KernelError::Failed(format!("curve blend: {m}"));
    let mut ext = Vec::with_capacity(list.len() + before.len() + after.len());
    ext.extend(before.iter().copied());
    ext.extend(list.iter().copied());
    ext.extend(after.iter().copied());
    let before = before.len();
    let ctrl: Vec<[mt::Vector4; 3]> = ext.iter().map(|s| section_controls(s, round)).collect::<Option<_>>().ok_or_else(|| fail("section"))?;
    let mut rows = Vec::with_capacity(3);
    let mut vknots = None;
    for j in 0..3 {
        let xyz: Vec<Vec3> = ctrl.iter().filter_map(|c| c.get(j)).map(|c| Vec3::new(c.x, c.y, c.z)).collect();
        let ww: Vec<Vec3> = ctrl.iter().filter_map(|c| c.get(j)).map(|c| Vec3::new(c.w, 0.0, 0.0)).collect();
        let bx = crate::build::interpolate_cubic(&xyz).ok_or_else(|| fail("blend surface"))?;
        let bw = crate::build::interpolate_cubic(&ww).ok_or_else(|| fail("blend surface"))?;
        vknots = Some(bx.knot_vec().clone());
        rows.push(bx.control_points().iter().zip(bw.control_points()).map(|(p, w)| mt::Vector4::new(p.x, p.y, p.z, w.x)).collect::<Vec<_>>());
    }
    let vk = vknots.ok_or_else(|| fail("knots"))?;
    let den = ext.len().saturating_sub(1) as f64;
    if den <= 0.0 {
        return Err(fail("empty sections"));
    }
    let range = (before as f64 / den, (before + list.len().saturating_sub(1)) as f64 / den);
    let contact = |row: usize| -> Result<mt::Curve> {
        let points = rows.get(row).cloned().ok_or_else(|| fail("contact row"))?;
        let bs = mt::BSplineCurve::try_new(vk.clone(), points).map_err(|e| fail(&e.to_string()))?;
        let mut curve = mt::NurbsCurve::new(bs);
        if range.0 > 0.0 {
            curve = curve.cut(range.0);
        }
        if range.1 < 1.0 {
            let _ = curve.cut(range.1);
        }
        Ok(mt::Curve::NurbsCurve(curve))
    };
    let contacts = [contact(0)?, contact(2)?];
    let bs = mt::BSplineSurface::try_new((mt::KnotVec::bezier_knot(2), vk), rows).map_err(|e| fail(&e.to_string()))?;
    Ok(Fit { surface: mt::Surface::NurbsSurface(mt::NurbsSurface::new(bs)), contacts, range, span: (0.0, 1.0) })
}

/// Compare the fitted strip with a solved circular section between interpolation nodes.
/// This checks the supporting contacts as well as the middle of the fillet band.
fn section_fits(fit: &Fit, section: &Section, along: f64, round: bool, sigma: f64, tolerance: f64) -> bool {
    use mt::{ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    let Some(ctrl) = section_controls(section, round) else { return false };
    let v = fit.range.0 + (fit.range.1 - fit.range.0) * along;
    for u in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let [a, b, c] = ctrl;
        let h = a * (1.0 - u) * (1.0 - u) + b * (2.0 * u * (1.0 - u)) + c * u * u;
        if !h.w.is_finite() || h.w <= 1e-12 {
            return false;
        }
        let exact = Vec3::new(h.x / h.w, h.y / h.w, h.z / h.w);
        // The input intersection edge may use piecewise leader-curve parameters. A
        // parameter-speed change is not a geometric error: compare nearest points, with
        // contacts constrained to the corresponding boundary and all probes to the strip.
        let (probe_u, probe_v) = if u == 0.0 || u == 1.0 {
            let Some(contact) = fit.contacts.get(usize::from(u == 1.0)) else { return false };
            let Some(parameter) = contact.search_nearest_parameter(p3(exact), v, 32) else { return false };
            (u, parameter.clamp(fit.range.0, fit.range.1))
        } else {
            let Some((pu, pv)) = fit.surface.search_nearest_parameter(p3(exact), Some((u, v)), 32) else { return false };
            (pu.clamp(0.0, 1.0), pv.clamp(fit.range.0, fit.range.1))
        };
        let actual = from_p3(fit.surface.subs(probe_u, probe_v));
        if !actual.is_finite() || actual.dist(exact) > tolerance {
            return false;
        }
        if round && (u == 0.0 || u == 1.0) {
            let Some(want) = ((section.c - exact) * sigma).normalized() else { return false };
            let normal = fit.surface.normal(probe_u, probe_v);
            let Some(normal) = Vec3::new(normal.x, normal.y, normal.z).normalized() else { return false };
            if normal.dot(want).abs() < 0.5f64.to_radians().cos() {
                return false;
            }
        }
    }
    true
}

/// Where a blend strip run on past a chain end meets the end face: the contacts' parameters
/// there and the end curve from contact 1 to contact 2 (fitted through points where the
/// strip's cross-section curves meet the face's surface, and checked against both).
fn end_cap(fit: &Fit, g: &mt::Face, at: &Section, seams: [Option<&mt::Edge>; 2], round: bool, tolerance: f64) -> Result<(f64, f64, mt::Curve)> {
    use mt::{ParametricSurface, SearchNearestParameter};
    let fail = || unsupported("the blend's end on its end face");
    let (surf, gs) = (&fit.surface, g.surface());
    let mid = if round { at.c + ((at.q1 - at.c) + (at.q2 - at.c)).normalized().ok_or_else(fail)? * at.c.dist(at.q1) } else { (at.q1 + at.q2) * 0.5 };
    let (_, v_start) = surf.search_nearest_parameter(p3(mid), None, 100).ok_or_else(fail)?;
    for m in [16usize, 32, 64] {
        // From the middle out to both contacts.
        let half = m / 2;
        let u_mid = half as f64 / m as f64;
        let v_mid = meet_surface(surf, &gs, u_mid, v_start, tolerance).ok_or_else(fail)?;
        let to_1 = march(surf, &gs, (u_mid, v_mid), 0.0, half, tolerance).ok_or_else(fail)?;
        let to_2 = march(surf, &gs, (u_mid, v_mid), 1.0, m - half, tolerance).ok_or_else(fail)?;
        let uv: Vec<(f64, f64)> = to_1.iter().rev().chain(to_2.iter().skip(1)).copied().collect();
        let mut pts: Vec<Vec3> = uv.iter().map(|(u, v)| from_p3(surf.subs(*u, *v))).collect();
        snap_to_seams(&mut pts, seams, tolerance);
        if let Some(bs) = fit_on_both(surf, &gs, &pts, &uv, tolerance) {
            let (Some(t1), Some(t2)) = (uv.first().map(|x| x.1), uv.last().map(|x| x.1)) else { return Err(fail()) };
            // Both contacts are cut there: strictly inside the strip's own part.
            let inside = |t: f64| t > fit.range.0 + 1e-6 && t < fit.range.1 - 1e-6;
            if !(inside(t1) && inside(t2)) {
                return Err(too_large("the blend is larger than the neighbouring faces (site 309)"));
            }
            return Ok((t1, t2, mt::Curve::BSplineCurve(bs)));
        }
    }
    Err(fail())
}

/// Where the strip's curve along it at `u` meets the surface `gs`: its `v` there, by Newton from
/// `v` (S(u, v) = G(a, b) for v, a, b).
fn meet_surface(surf: &mt::Surface, gs: &mt::Surface, u: f64, v: f64, tolerance: f64) -> Option<f64> {
    use mt::{ParametricSurface, SearchNearestParameter};
    let mut v = v;
    let (mut a, mut b) = gs.search_nearest_parameter(surf.subs(u, v), None, 100)?;
    for _ in 0..60 {
        let f = from_p3(surf.subs(u, v)) - from_p3(gs.subs(a, b));
        if !f.is_finite() {
            return None;
        }
        if f.len() <= tolerance * 1e-3 {
            return (-1e-9..=1.0 + 1e-9).contains(&v).then_some(v.clamp(0.0, 1.0));
        }
        let (sv, ga, gb) = (surf.vder(u, v), gs.uder(a, b), gs.vder(a, b));
        let m = [[sv.x, -ga.x, -gb.x], [sv.y, -ga.y, -gb.y], [sv.z, -ga.z, -gb.z]];
        let [dv, da, db] = crate::polyhedron::solve3_pub(m, [-f.x, -f.y, -f.z])?;
        // Keep each step within the strip's parameters.
        let k = if dv.abs() > 0.1 { 0.1 / dv.abs() } else { 1.0 };
        v += dv * k;
        a += da * k;
        b += db * k;
    }
    None
}

/// The strip's curves along it from `u0` (whose `v0` already meets the surface `gs`) to `u1`, in
/// `steps` steps, each where it meets `gs`: `(u, v)` at each, every search starting where the
/// last ended.
fn march(surf: &mt::Surface, gs: &mt::Surface, (u0, v0): (f64, f64), u1: f64, steps: usize, tolerance: f64) -> Option<Vec<(f64, f64)>> {
    let mut out = Vec::with_capacity(steps + 1);
    out.push((u0, v0));
    let mut last = v0;
    for i in 1..=steps {
        let u = u0 + (u1 - u0) * i as f64 / steps as f64;
        last = meet_surface(surf, gs, u, last, tolerance)?;
        out.push((u, last));
    }
    Some(out)
}

/// The cubic through `pts` (the strip's points at `uv`, where they meet the surface `gs`), if it
/// keeps within `tolerance` of both the strip and `gs` halfway between them.
fn fit_on_both(surf: &mt::Surface, gs: &mt::Surface, pts: &[Vec3], uv: &[(f64, f64)], tolerance: f64) -> Option<mt::BSplineCurve<mt::Point3>> {
    use mt::{ParametricCurve, ParametricSurface, SearchNearestParameter};
    let bs = crate::build::interpolate_cubic(pts)?;
    let m = pts.len().checked_sub(1)?;
    let fits = (0..m).all(|i| {
        let q = bs.subs((i as f64 + 0.5) / m as f64);
        let hint = uv.get(i).zip(uv.get(i + 1)).map(|(a, b)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5));
        let on_g = gs.search_nearest_parameter(q, None, 100).is_some_and(|(a, b)| from_p3(gs.subs(a, b)).dist(from_p3(q)) <= tolerance);
        let on_s = surf.search_nearest_parameter(q, hint, 100).is_some_and(|(a, b)| from_p3(surf.subs(a, b)).dist(from_p3(q)) <= tolerance);
        on_g && on_s
    });
    fits.then_some(bs)
}

/// A round's end across two end faces: on the first from contact 1 to the crossing, on the second
/// from the crossing to contact 2 (none where the crossing is that contact).
type RoundEnds = (Option<mt::Curve>, Option<mt::Curve>);

/// A round run on past a chain end that two planar end faces close (`ga` reached by contact 1,
/// `gb` by contact 2, meeting in a straight seam through the end vertex `p` along `w`): where the
/// round crosses the seam's line, and its end on each face.
struct SplitRound {
    /// The contacts' parameters where they are cut.
    t1: f64,
    t2: f64,
    /// Where the round crosses the seam's line, as a fraction of `w` from `p` (in (0, 1) where
    /// the seam is shortened, below 0 where it runs on past the vertex).
    fraction: f64,
    /// The crossing, on the round.
    crossing: Vec3,
    /// The end on `ga` from contact 1 to the crossing, and on `gb` from the crossing to contact
    /// 2. None where the crossing is that contact itself (the seam's line runs on along that
    /// side's own edge with the end face: a wall's end flush with the face beneath it).
    on_a: Option<mt::Curve>,
    on_b: Option<mt::Curve>,
}

/// Where the line through `p` along `w` crosses the round `surf` within its own part: `(u, v, λ)`
/// with S(u, v) = p + w λ. First on the circle of the section `at` (exact for a straight edge
/// between planes), then on the fitted surface by Newton. A crossing within a hair of a contact
/// is that contact, where the line passes it within `tolerance`.
fn crossing_on_round(surf: &mt::Surface, at: &Section, (p, t, w): (Vec3, Vec3, Vec3), tolerance: f64) -> Option<(f64, f64, f64)> {
    use mt::{ParametricSurface, SearchNearestParameter};
    // |perp(p + w λ - c)| = r, a point of the arc from q1 to q2.
    let perp = |x: Vec3| x - t * x.dot(t);
    let r = at.c.dist(at.q1);
    let (d0, dw) = (perp(p - at.c), perp(w));
    let (qa, qb, qc) = (dw.dot(dw), 2.0 * d0.dot(dw), d0.dot(d0) - r * r);
    let disc = qb * qb - 4.0 * qa * qc;
    if !(qa > 1e-300 && disc >= 0.0 && r > 0.0) {
        return None;
    }
    let (a1, a2) = (perp(at.q1 - at.c), perp(at.q2 - at.c));
    let angle = |x: Vec3, y: Vec3| (x.dot(y) / (x.len() * y.len()).max(1e-300)).clamp(-1.0, 1.0).acos();
    let span = angle(a1, a2);
    let guess = [(-qb - disc.sqrt()) / (2.0 * qa), (-qb + disc.sqrt()) / (2.0 * qa)]
        .into_iter()
        .map(|l| (l, perp(p + w * l - at.c)))
        .map(|(l, y)| (l, angle(a1, y) + angle(y, a2) - span))
        .filter(|(_, off)| *off <= 1e-6)
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(l, _)| l)?;
    let (mut u, mut v) = surf.search_nearest_parameter(p3(p + w * guess), None, 100)?;
    let mut lambda = guess;
    let mut found = false;
    for _ in 0..60 {
        let f = from_p3(surf.subs(u, v)) - (p + w * lambda);
        if !f.is_finite() {
            return None;
        }
        if f.len() <= tolerance * 1e-3 {
            found = true;
            break;
        }
        let (su, sv) = (surf.uder(u, v), surf.vder(u, v));
        let m = [[su.x, sv.x, -w.x], [su.y, sv.y, -w.y], [su.z, sv.z, -w.z]];
        let [du, dv, dl] = crate::polyhedron::solve3_pub(m, [-f.x, -f.y, -f.z])?;
        let k = if du.abs().max(dv.abs()) > 0.1 { 0.1 / du.abs().max(dv.abs()) } else { 1.0 };
        u += du * k;
        v += dv * k;
        lambda += dl * k;
    }
    let edge = (tolerance * 10.0 / (r * span).max(1e-300)).min(1e-3);
    if !found || !(-edge..=1.0 + edge).contains(&u) || !(-1e-9..=1.0 + 1e-9).contains(&v) {
        return None;
    }
    // Near a contact: on it, if the line passes it closely enough; else where it crossed.
    let snapped = [(0.0, u < edge), (1.0, u > 1.0 - edge)].into_iter().find(|x| x.1).map(|x| x.0);
    Some(snapped.and_then(|at_u| crossing_on_contact(surf, at_u, v, (p, w), tolerance)).unwrap_or((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0), lambda)))
}

/// Where the line through `p` along `w`, lying in (or within `tolerance` of) the plane of the side
/// whose contact runs at `u` (0 or 1), meets that contact: `(u, v, λ)` with S(u, v) nearest
/// p + w λ, by Gauss-Newton on `v` and `λ` from `v0`, if they come within `tolerance`.
fn crossing_on_contact(surf: &mt::Surface, u: f64, v0: f64, (p, w): (Vec3, Vec3), tolerance: f64) -> Option<(f64, f64, f64)> {
    use mt::ParametricSurface;
    let w2 = w.len2();
    if !(w2 > 1e-300) {
        return None;
    }
    let mut v = v0;
    let mut lambda = (from_p3(surf.subs(u, v)) - p).dot(w) / w2;
    for _ in 0..60 {
        let f = from_p3(surf.subs(u, v)) - (p + w * lambda);
        if !f.is_finite() {
            return None;
        }
        if f.len() <= tolerance * 1e-3 {
            break;
        }
        let sv = surf.vder(u, v);
        let a = Vec3::new(sv.x, sv.y, sv.z);
        let (aa, ab, bb) = (a.dot(a), -a.dot(w), w2);
        let det = aa * bb - ab * ab;
        if !(det.abs() > 1e-300) {
            return None;
        }
        let (ra, rb) = (-a.dot(f), w.dot(f));
        let dv = (ra * bb - rb * ab) / det;
        let dl = (aa * rb - ab * ra) / det;
        let k = if dv.abs() > 0.1 { 0.1 / dv.abs() } else { 1.0 };
        v += dv * k;
        lambda += dl * k;
        // Settled: the line misses the contact by what is left.
        if (dv * k).abs() <= 1e-14 && (dl * k).abs() * w2.sqrt() <= tolerance * 1e-6 {
            break;
        }
    }
    let miss = (from_p3(surf.subs(u, v)) - (p + w * lambda)).len();
    (miss <= tolerance && (-1e-9..=1.0 + 1e-9).contains(&v)).then_some((u, v.clamp(0.0, 1.0), lambda))
}

/// [`SplitRound`] for the round `fit`, whose section at the end vertex is `at` (centre on the
/// strip's axis along `t` there). The crossing is found on the section's circle and then on the
/// fitted surface itself; each end is fitted through points where the strip's curves along it
/// meet that face, and checked against both.
fn split_round_cap(
    fit: &Fit,
    (ga, gb): (&mt::Face, &mt::Face),
    at: &Section,
    (p, t, w): (Vec3, Vec3, Vec3),
    seams: [Option<&mt::Edge>; 2],
    tolerance: f64,
) -> Result<SplitRound> {
    use mt::{ParametricSurface, SearchNearestParameter};
    let fail = || unsupported("the round does not cross the edge between the end faces");
    let surf = &fit.surface;
    let along = w.normalized().ok_or_else(fail)?;
    // A seam whose line lies in a side's plane (that side runs on into the end face along it: a
    // plate's slanted side under a wall's end flush with it) touches the round at that side's
    // contact instead of crossing it there: within `tolerance` of the plane as far as the round
    // reaches from the vertex.
    let reach = at.c.dist(p) + at.c.dist(at.q1);
    let in_side = [(0.0, at.q1), (1.0, at.q2)]
        .into_iter()
        .find(|(_, q)| (*q - at.c).normalized().is_some_and(|n| n.dot(along).abs() * reach <= tolerance))
        .and_then(|(u, q)| surf.search_nearest_parameter(p3(q), None, 100).and_then(|(_, v)| crossing_on_contact(surf, u, v, (p, w), tolerance)));
    let (u, v, lambda) = match in_side {
        Some(found) => found,
        None => crossing_on_round(surf, at, (p, t, w), tolerance).ok_or_else(fail)?,
    };
    let crossing = from_p3(surf.subs(u, v));
    // One end, between the crossing at `u` and the contact at `to` (0 or 1) on face `g`, from
    // contact 1's side to contact 2's: the curve and its `v` at the contact.
    let piece = |g: &mt::Face, to: f64, seam: Option<&mt::Edge>| -> Result<Option<(mt::Curve, f64)>> {
        if (u - to).abs() < 1e-12 {
            return Ok(None);
        }
        let gs = g.surface();
        for m in [16usize, 32, 64] {
            // From the crossing out to the contact.
            let mut uv = march(surf, &gs, (u, v), to, m, tolerance).ok_or_else(fail)?;
            let mut pts: Vec<Vec3> = uv.iter().map(|(a, b)| from_p3(surf.subs(*a, *b))).collect();
            let at_contact = uv.last().map(|x| x.1).ok_or_else(fail)?;
            if to == 0.0 {
                pts.reverse();
                uv.reverse();
                snap_to_seams(&mut pts, [seam, None], tolerance);
            } else {
                snap_to_seams(&mut pts, [None, seam], tolerance);
            }
            if let Some(bs) = fit_on_both(surf, &gs, &pts, &uv, tolerance) {
                return Ok(Some((mt::Curve::BSplineCurve(bs), at_contact)));
            }
        }
        Err(fail())
    };
    let a = piece(ga, 0.0, seams[0])?;
    let b = piece(gb, 1.0, seams[1])?;
    let (t1, t2) = (a.as_ref().map_or(v, |x| x.1), b.as_ref().map_or(v, |x| x.1));
    // Both contacts are cut there: strictly inside the strip's own part.
    let inside = |t: f64| t > fit.range.0 + 1e-6 && t < fit.range.1 - 1e-6;
    if !(inside(t1) && inside(t2)) {
        return Err(too_large("the blend is larger than the neighbouring faces (split round end)"));
    }
    Ok(SplitRound { t1, t2, fraction: lambda, crossing, on_a: a.map(|x| x.0), on_b: b.map(|x| x.0) })
}

/// The ends of an end curve (contact 1 to contact 2) that land within `tolerance` of an end of
/// their side's seam are that point exactly: the blend and the faces there meet at one vertex.
fn snap_to_seams(pts: &mut [Vec3], seams: [Option<&mt::Edge>; 2], tolerance: f64) {
    let snap = |p: &mut Vec3, seam: Option<&mt::Edge>| {
        for q in seam.into_iter().flat_map(|e| [from_p3(e.front().point()), from_p3(e.back().point())]) {
            if p.dist(q) <= tolerance {
                *p = q;
            }
        }
    };
    if let Some(first) = pts.first_mut() {
        snap(first, seams[0]);
    }
    if let Some(last) = pts.last_mut() {
        snap(last, seams[1]);
    }
}

/// Where a blend strip run on past a chain end meets an end face that lies along the edge and is
/// tangent to one side there (a wall running out into the face, an earlier round that touches a
/// side along its contact): the end curve from contact 1 to contact 2, the same result as
/// [`end_cap`]. The strip touches the face along the tangent side's contact, so a Newton solve
/// for the crossing is singular there.
///
/// Both ends of the curve are where a contact meets a seam, which is a plain crossing: the
/// tangent side's contact leaves the end face where it crosses the seam of that side with the
/// face, the other contact where it crosses the other side's seam. Between those stations each
/// cross-section crosses the face once, a simple root. Either station may lie before or after
/// the vertex's section: the curve runs inwards along the edge or out along the extension.
fn tangent_cap(fit: &Fit, g: &mt::Face, tangent_side: u8, seams: [Option<&mt::Edge>; 2], tolerance: f64) -> Result<(f64, f64, mt::Curve)> {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface, SearchNearestParameter};
    let fail = || unsupported("the blend's end on its end face");
    let surf = &fit.surface;
    let (lo, hi) = (fit.range.0.min(fit.range.1), fit.range.0.max(fit.range.1));
    let (u_t, u_o, seam_t, seam_o) = if tangent_side == 1 { (0.0, 1.0, seams[0], seams[1]) } else { (1.0, 0.0, seams[1], seams[0]) };
    let (seam_t, seam_o) = (seam_t.ok_or_else(fail)?, seam_o.ok_or_else(fail)?);
    // The station where the strip's contact `u` crosses `seam`.
    let crossing = |u: f64, seam: &mt::Edge| -> Option<f64> {
        let curve = seam.curve();
        let (a, b) = curve.range_tuple();
        let polyline: Vec<Vec3> = (0..=1024).map(|i| from_p3(curve.subs(a + (b - a) * f64::from(i) / 1024.0))).collect();
        let dist = |v: f64| -> f64 {
            let x = from_p3(surf.subs(u, v));
            polyline.windows(2).map(|w| x.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min)
        };
        let n = 400;
        let step = (hi - lo) / f64::from(n);
        let (mut best, mut best_d) = (lo, f64::INFINITY);
        for i in 0..=n {
            let v = lo + step * f64::from(i);
            let d = dist(v);
            if d < best_d {
                (best, best_d) = (v, d);
            }
        }
        let (mut x0, mut x1) = ((best - step).max(lo), (best + step).min(hi));
        let phi = 0.618_033_988_749_894_9;
        for _ in 0..80 {
            let (c, d) = (x1 - phi * (x1 - x0), x0 + phi * (x1 - x0));
            if dist(c) < dist(d) {
                x1 = d;
            } else {
                x0 = c;
            }
        }
        let v = 0.5 * (x0 + x1);
        (dist(v) <= tolerance).then_some(v)
    };
    let (rt, ra) = (crossing(u_t, seam_t), crossing(u_o, seam_o));
    let v_t = rt.ok_or_else(fail)?;
    let v_a = ra.ok_or_else(|| too_large("the blend is larger than the neighbouring faces (site 387)"))?;
    let inside = |t: f64| t > lo + 1e-6 && t < hi - 1e-6;
    if !(inside(v_a) && inside(v_t)) || (v_a - v_t).abs() < 1e-9 {
        return Err(too_large("the blend is larger than the neighbouring faces (site 390)"));
    }
    // How far the strip's point (u, v) is outside the end face (negative: inside the body).
    let outside = |u: f64, v: f64| -> Option<f64> {
        let x = from_p3(surf.subs(u, v));
        let (q, n, _) = project(g, x, None)?;
        Some((x - q).dot(n))
    };
    // Root of `f` between `lo` and `hi`, `f(lo)` and `f(hi)` of different sign.
    let bisect = |f: &dyn Fn(f64) -> Option<f64>, mut x0: f64, mut x1: f64| -> Option<f64> {
        let f0 = f(x0)?;
        for _ in 0..60 {
            let mid = 0.5 * (x0 + x1);
            let fm = f(mid)?;
            if fm.abs() <= tolerance * 1e-6 {
                return Some(mid);
            }
            if fm.is_sign_negative() == f0.is_sign_negative() {
                x0 = mid;
            } else {
                x1 = mid;
            }
        }
        Some(0.5 * (x0 + x1))
    };
    for m in [16usize, 32, 64, 128] {
        // The curve at stations v_i from the crossing contact to the tangent one, as (u, v, point).
        let mut stations: Vec<(f64, f64, Vec3)> = Vec::with_capacity(m + 1);
        for i in 0..=m {
            // Clustered at both ends, where the curve can meet a contact tangentially (a square
            // root in the station) and a uniform spacing would fit it poorly.
            let v = v_a + (v_t - v_a) * 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / m as f64).cos());
            let u = if i == 0 {
                u_o
            } else if i == m {
                u_t
            } else {
                let at_t = outside(u_t, v).ok_or_else(fail)?;
                let at_o = outside(u_o, v).ok_or_else(fail)?;
                // No crossing where the contacts are on one side: the curve is at the tangent one.
                if at_t * at_o >= 0.0 { u_t } else { bisect(&|x| outside(x, v), u_o, u_t).ok_or_else(fail)? }
            };
            stations.push((u, v, from_p3(surf.subs(u, v))));
        }
        let mut pts: Vec<Vec3> = stations.iter().map(|x| x.2).collect();
        // The curve runs from contact 1 to contact 2.
        if u_o == 1.0 {
            pts.reverse();
        }
        snap_to_seams(&mut pts, seams, tolerance);
        let bs = crate::build::interpolate_cubic(&pts).ok_or_else(fail)?;
        let gs = g.surface();
        let fits = stations.windows(2).enumerate().all(|(i, w)| {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { return false };
            let t = if u_o == 1.0 { 1.0 - (i as f64 + 0.5) / m as f64 } else { (i as f64 + 0.5) / m as f64 };
            let q = p3(from_p3(bs.subs(t)));
            let hint = (0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
            let on_g = gs.search_nearest_parameter(q, None, 100).is_some_and(|(x, y)| from_p3(gs.subs(x, y)).dist(from_p3(q)) <= tolerance);
            let on_s = surf.search_nearest_parameter(q, Some(hint), 100).is_some_and(|(x, y)| from_p3(surf.subs(x, y)).dist(from_p3(q)) <= tolerance);
            on_g && on_s
        });
        if fits {
            let (t1, t2) = if u_o == 0.0 { (v_a, v_t) } else { (v_t, v_a) };
            return Ok((t1, t2, mt::Curve::BSplineCurve(bs)));
        }
    }
    Err(fail())
}

/// One cross-section of the blend: ball centre and its contacts with side 1 and side 2.
#[derive(Clone, Copy, Debug)]
struct Section {
    c: Vec3,
    q1: Vec3,
    q2: Vec3,
}

/// The third condition of a section's solve, in place of staying in the plane square to the edge: a contact held at a seam.
#[derive(Clone, Copy)]
enum Extra<'a> {
    /// Side `k`'s contact lies in this plane (a point on it, its normal): beside a straight seam.
    Plane(usize, Vec3, Vec3),
    /// Side `k`'s contact lies on this curved seam (a lofted wall's seam between its two halves).
    Seam(usize, &'a mt::Edge),
}

/// The seam's point nearest `x` and its unit tangent there.
fn nearest_on_seam(seam: &mt::Edge, x: Vec3) -> Option<(Vec3, Vec3)> {
    use mt::{BoundedCurve, ParametricCurve, SearchNearestParameter};
    let curve = seam.curve();
    let (lo, hi) = curve.range_tuple();
    let at = |t: f64| from_p3(curve.subs(t));
    // The nearest of a few samples starts the search: a long seam holds the point's neighbourhood
    // far from its ends.
    let hint = (0..=64).map(|i| lo + (hi - lo) * i as f64 / 64.0).min_by(|a, b| at(*a).dist(x).total_cmp(&at(*b).dist(x)))?;
    let t = curve.search_nearest_parameter(p3(x), hint, 100)?.clamp(lo, hi);
    let d = curve.der(t);
    Some((at(t), Vec3::new(d.x, d.y, d.z).normalized()?))
}

/// Nearest point of a face's surface and the outward normal there.
pub(crate) fn project(f: &mt::Face, p: Vec3, hint: Option<(f64, f64)>) -> Option<(Vec3, Vec3, (f64, f64))> {
    use mt::{ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    let s = f.oriented_surface();
    let uv = s.search_nearest_parameter(p3(p), hint, 100).or_else(|| s.search_nearest_parameter(p3(p), None, 100))?;
    let q = from_p3(s.subs(uv.0, uv.1));
    let n = s.normal(uv.0, uv.1);
    Some((q, Vec3::new(n.x, n.y, n.z).normalized()?, uv))
}

/// Whether `uv` lies within the parameters of the face's surface (unbounded directions pass).
pub(crate) fn in_range(f: &mt::Face, uv: (f64, f64)) -> bool {
    use mt::ParametricSurface;
    use std::ops::Bound;
    let inside = |range: (Bound<f64>, Bound<f64>), x: f64| {
        let slack = |a: f64, b: f64| (b - a).abs() * 1e-6 + 1e-9;
        match range {
            (Bound::Included(a) | Bound::Excluded(a), Bound::Included(b) | Bound::Excluded(b)) => x >= a - slack(a, b) && x <= b + slack(a, b),
            (Bound::Included(a) | Bound::Excluded(a), Bound::Unbounded) => x >= a - 1e-9,
            (Bound::Unbounded, Bound::Included(b) | Bound::Excluded(b)) => x <= b + 1e-9,
            (Bound::Unbounded, Bound::Unbounded) => true,
        }
    };
    let (ru, rv) = f.oriented_surface().parameter_range();
    uv.0.is_finite() && uv.1.is_finite() && inside(ru, uv.0) && inside(rv, uv.1)
}

/// Solve for the ball touching both sides near `p`. `extra` adds a third condition (a plane the
/// side-k contact must lie in, or a curved seam it must lie on); without it the centre stays in the
/// plane through `p` square to `t`. `sigma` is +1 when the ball sits outside the material (concave edge), −1 inside.
/// `cusp` is ±1 at a knife edge (the faces' normals opposite: both leave the edge the same way,
/// along `cusp · n1 × t`, and the ball lies where the cusp has opened to its width), else 0.
#[allow(clippy::too_many_arguments)]
fn solve(f1: &[&mt::Face], f2: &[&mt::Face], p: Vec3, t: Vec3, r: f64, sigma: f64, cusp: f64, extra: Option<Extra<'_>>, tol: f64) -> Option<Section> {
    solve_from(f1, f2, p, t, r, sigma, cusp, extra, tol, None)
}

/// [`solve`], started from the centre `start` when given (the corner builder's balls at a knife
/// edge start beside the last one).
#[allow(clippy::too_many_arguments)]
fn solve_from(
    f1: &[&mt::Face],
    f2: &[&mt::Face],
    p: Vec3,
    t: Vec3,
    r: f64,
    sigma: f64,
    cusp: f64,
    extra: Option<Extra<'_>>,
    tol: f64,
    start: Option<Vec3>,
) -> Option<Section> {
    // The nearest face of a side (several faces on one smooth surface group). Each face's
    // search starts from its last answer: the centre moves little between iterations. An
    // answer outside the surface's parameters, or farther than that last point (the search
    // ran away or settled on the far side), is searched again cold.
    let last = std::cell::RefCell::new(HashMap::<mt::FaceID, ((f64, f64), Vec3)>::new());
    let near = |fs: &[&mt::Face], c: Vec3| -> Option<(Vec3, Vec3)> {
        fs.iter()
            .filter_map(|f| {
                let prev = last.borrow().get(&f.id()).copied();
                let warm =
                    prev.and_then(|(uv, q)| project(f, c, Some(uv)).filter(|(x, _, uv)| in_range(f, *uv) && x.dist(c) <= q.dist(c) + tol * 1e-3));
                let (q, n, uv) = match warm {
                    Some(x) => x,
                    None => project(f, c, None)?,
                };
                last.borrow_mut().insert(f.id(), (uv, q));
                Some((q, n))
            })
            .min_by(|a, b| a.0.dist(c).total_cmp(&b.0.dist(c)))
    };
    let (q1, n1) = near(f1, p)?;
    let (q2, n2) = near(f2, p)?;
    let _ = (q1, q2);
    let den = 1.0 + n1.dot(n2);
    let mut c = if let Some(c) = start {
        c
    } else if cusp != 0.0 && den.abs() < KNIFE {
        // The normals are (nearly) opposite: `(n1 + n2) / den` has no direction. The centre is r
        // inside face 1 and slides along it, away from the edge, until it is r from face 2 too.
        let w = n1.cross(t).normalized()? * cusp;
        let centre = |s: f64| -> Option<Vec3> {
            let (qa, na) = near(f1, p + w * s)?;
            Some(qa + na * (sigma * r))
        };
        let gap = |s: f64| -> Option<f64> {
            let c = centre(s)?;
            let (q2, n2) = near(f2, c)?;
            Some((c - q2).dot(n2) - sigma * r)
        };
        let (mut lo, mut g_lo) = (0.0, gap(0.0)?);
        let mut hi = None;
        let mut s = r * 0.05;
        for _ in 0..KNIFE_STEPS {
            let Some(g) = gap(s) else { break };
            if g == 0.0 || g.is_sign_negative() != g_lo.is_sign_negative() {
                hi = Some(s);
                break;
            }
            (lo, g_lo) = (s, g);
            s *= 1.6;
        }
        let mut hi = hi?;
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            match gap(mid) {
                Some(g) if g.is_sign_negative() == g_lo.is_sign_negative() => lo = mid,
                Some(_) => hi = mid,
                None => break,
            }
        }
        centre(0.5 * (lo + hi))?
    } else {
        if den.abs() < 1e-9 {
            return None;
        }
        p + (n1 + n2) * (sigma * r / den)
    };
    for _ in 0..60 {
        let (q1, n1) = near(f1, c)?;
        let (q2, n2) = near(f2, c)?;
        let g1 = (c - q1).dot(n1) - sigma * r;
        let g2 = (c - q2).dot(n2) - sigma * r;
        // Third row: stay in the section plane, or put a contact on the seam plane.
        let (row3, g3) = match extra {
            None => (t, (c - p).dot(t)),
            Some(Extra::Plane(k, o, m)) => {
                let q = if k == 1 { q1 } else { q2 };
                (m, (q - o).dot(m))
            }
            Some(Extra::Seam(k, seam)) => {
                // The plane through the seam's nearest point to the contact, square to that side's
                // surface, taken afresh each step: it holds the contact on the curved seam as the
                // steps close in.
                let (q, n) = if k == 1 { (q1, n1) } else { (q2, n2) };
                let (on_seam, d) = nearest_on_seam(seam, q)?;
                let m = d.cross(n).normalized()?;
                (m, (q - on_seam).dot(m))
            }
        };
        let a = [[n1.x, n1.y, n1.z], [n2.x, n2.y, n2.z], [row3.x, row3.y, row3.z]];
        let b = [-g1, -g2, -g3];
        let x = crate::polyhedron::solve3_pub(a, b)?;
        let dc = Vec3::new(x[0], x[1], x[2]);
        c += dc;
        if dc.len() < tol * 1e-3 && g1.abs() < tol && g2.abs() < tol && g3.abs() < tol {
            let (q1, _) = near(f1, c)?;
            let (q2, _) = near(f2, c)?;
            return Some(Section { c, q1, q2 });
        }
    }
    None
}

/// Contacts of a convex chamfer in a cross-section normal to a straight edge. The caller
/// checks that sweeping the section stays on both supports within the fitting tolerance.
pub(crate) fn straight_chamfer_contacts(f1: &mt::Face, f2: &mt::Face, p: Vec3, d: Vec3, r: f64, tolerance: f64) -> Option<(Vec3, Vec3)> {
    let section = solve(&[f1], &[f2], p, d, r, -1.0, 0.0, None, tolerance)?;
    Some((section.q1, section.q2))
}

/// The section whose two contacts are `chord` apart: the ball's radius is `chord / (2 sin(ψ/2))`
/// for the `ψ` between the two outward normals where it touches, `|q1 − q2| = 2 r sin(ψ/2)`. The
/// normals turn as the ball grows against curved faces, so the width is measured on the solved
/// section and the radius corrected until it matches (the width is proportional to the radius,
/// so the corrections converge quickly). The normals at `p` give the first radius, exact where
/// both faces are planar; `None` where the faces meet too nearly tangentially to span a chord.
#[allow(clippy::too_many_arguments)]
fn solve_chord(
    f1: &[&mt::Face],
    f2: &[&mt::Face],
    p: Vec3,
    t: Vec3,
    chord: f64,
    sigma: f64,
    cusp: f64,
    extra: Option<Extra<'_>>,
    tol: f64,
) -> Option<(Section, f64)> {
    let normal = |fs: &[&mt::Face]| fs.iter().filter_map(|f| project(f, p, None)).min_by(|a, b| a.0.dist(p).total_cmp(&b.0.dist(p))).map(|x| x.1);
    let (n1, n2) = (normal(f1)?, normal(f2)?);
    let half = (n1.dot(n2).clamp(-1.0, 1.0).acos() / 2.0).sin();
    if !(half > 1e-3) {
        return None;
    }
    let mut r = chord / (2.0 * half);
    for _ in 0..CHORD_STEPS {
        if !(r.is_finite() && r > 0.0 && r < 1e9) {
            return None;
        }
        let s = solve(f1, f2, p, t, r, sigma, cusp, extra, tol)?;
        let width = s.q1.dist(s.q2);
        if !width.is_finite() || width <= 0.0 {
            return None;
        }
        if (width - chord).abs() <= (chord * 1e-9).max(tol * 0.5) {
            return Some((s, r));
        }
        r *= chord / width;
    }
    None
}

/// The rolling ball at one station, sized by the blend's law.
struct Sizer<'a> {
    size: Size<'a>,
    /// The tolerance the section's equations are solved at.
    tol: f64,
}

impl Sizer<'_> {
    /// The section at `p` (tangent `t`, the size law's parameter `s`) holding the third
    /// condition `extra` when given, with the radius it was solved at.
    #[allow(clippy::too_many_arguments)]
    fn section(
        &self,
        f1: &[&mt::Face],
        f2: &[&mt::Face],
        p: Vec3,
        t: Vec3,
        s: f64,
        sigma: f64,
        cusp: f64,
        extra: Option<Extra<'_>>,
    ) -> Option<(Section, f64)> {
        match self.size {
            Size::Constant(r) => solve(f1, f2, p, t, r, sigma, cusp, extra, self.tol).map(|x| (x, r)),
            Size::Chord(chord) => solve_chord(f1, f2, p, t, chord, sigma, cusp, extra, self.tol),
            Size::Law(law) => {
                let r = law_beyond(law, s)?;
                if !(r.is_finite() && (0.0..1e6).contains(&r)) {
                    return None;
                }
                solve(f1, f2, p, t, r, sigma, cusp, extra, self.tol).map(|x| (x, r))
            }
        }
    }

    /// What a station the ball does not fit at is refused with: a chord too large is about the
    /// size asked for, anything else about where the ball is.
    fn refusal(&self) -> KernelError {
        match self.size {
            Size::Chord(_) => too_large("the blend is larger than the neighbouring faces (site 704)"),
            _ => unsupported("the ball doesn't fit along the edge"),
        }
    }
}

/// Whether the strip's curvature jumps where two runs of sections meet (each three sections from
/// the joint outward, the joint's first): for the centres or either contact, the difference of
/// the circles through them would move the middle section by more than a tenth of `tolerance`.
fn bends(a: [&Section; 3], b: [&Section; 3], tolerance: f64) -> bool {
    let curvature = |p: [Vec3; 3]| {
        let (x, y, z) = (p[1] - p[0], p[2] - p[1], p[2] - p[0]);
        let d = x.len() * y.len() * z.len();
        if d > 1e-300 { 2.0 * x.cross(y).len() / d } else { 0.0 }
    };
    let h = a[0].c.dist(a[1].c);
    [|s: &Section| s.c, |s: &Section| s.q1, |s: &Section| s.q2]
        .iter()
        .any(|f| (curvature(a.map(f)) - curvature(b.map(f))).abs() * h * h > tolerance * 0.1)
}

/// Whether the section is a point: a blend whose radius has run to zero, closing to the edge's
/// end point there.
fn closed_section(s: &Section) -> bool {
    s.c.dist(s.q1) <= CLOSED && s.c.dist(s.q2) <= CLOSED
}

/// Homogeneous control points of the circular (or straight) cross-section from q1 to q2.
fn section_controls(s: &Section, round: bool) -> Option<[mt::Vector4; 3]> {
    let h = |p: Vec3, w: f64| mt::Vector4::new(p.x * w, p.y * w, p.z * w, w);
    if !round {
        let m = (s.q1 + s.q2) * 0.5;
        return Some([h(s.q1, 1.0), h(m, 1.0), h(s.q2, 1.0)]);
    }
    if closed_section(s) {
        // A closed section: the arc has shrunk to the edge's end point, all three controls
        // there, so the fitted strips of the neighbouring faces meet at that vertex.
        return Some([h(s.c, 1.0), h(s.c, 1.0), h(s.c, 1.0)]);
    }
    let (a, b) = (s.q1 - s.c, s.q2 - s.c);
    let theta = a.normalized()?.dot(b.normalized()?).clamp(-1.0, 1.0).acos();
    if !(theta > 1e-6 && theta < std::f64::consts::PI - 1e-6) {
        return None;
    }
    let half = (theta / 2.0).cos();
    let bis = (a + b).normalized()?;
    let m = s.c + bis * (a.len() / half);
    Some([h(s.q1, 1.0), h(m, half), h(s.q2, 1.0)])
}

/// Where to start the ball at a knife edge through `p` (direction `t`, the sides running on from
/// it along `into`): the first point along them, from the edge, where they are a diameter
/// apart, midway.
fn knife_start(o1: &[&mt::Face], o2: &[&mt::Face], p: Vec3, t: Vec3, into: Vec3, r: f64) -> Option<Vec3> {
    let w = (into - t * into.dot(t)).normalized()?;
    (1..=256).find_map(|k| {
        let x = p + w * (r * k as f64 / 8.0);
        let a = o1.iter().filter_map(|f| project(f, x, None)).min_by(|a, b| a.0.dist(x).total_cmp(&b.0.dist(x)))?;
        let b = o2.iter().filter_map(|f| project(f, x, None)).min_by(|a, b| a.0.dist(x).total_cmp(&b.0.dist(x)))?;
        let c = (a.0 + b.0) * 0.5;
        (a.0.dist(b.0) >= 2.0 * r).then(|| c - t * (c - p).dot(t))
    })
}

/// Whether two faces meeting at no angle at `pm` (the first's outward normal `n1`, running on
/// from the edge along `into`) hold the material between them: a little way in, the second is
/// behind the first.
fn knife_convex(f1: &mt::Face, f2: &mt::Face, pm: Vec3, into: Vec3, r: f64) -> Option<bool> {
    let x = pm + into * r;
    let (a, b) = (project(f1, x, None)?, project(f2, x, None)?);
    Some((b.0 - a.0).dot(a.1) < 0.0)
}

/// The nearest point of a face's (oriented) surface to `x`, its outward normal and parameters;
/// `last`: where the last search on it ended (the points asked about come in small steps).
fn nearest_on(surface: &mt::Surface, last: &std::cell::Cell<Option<(f64, f64)>>, x: Vec3) -> Option<(Vec3, Vec3, (f64, f64))> {
    use mt::{ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    // The nearest of the search from the last answer, a fresh one (the first time, also from
    // a grid over the surface's parameters: a fresh search on a conic can settle on its far
    // side) and either kept within the parameters (past them a conic runs on round).
    let (ru, rv) = surface.try_range_tuple();
    let ((u0, u1), (v0, v1)) = (ru.unwrap_or((f64::NEG_INFINITY, f64::INFINITY)), rv.unwrap_or((f64::NEG_INFINITY, f64::INFINITY)));
    let inside = |uv: (f64, f64)| uv.0 >= u0 - 1e-9 && uv.0 <= u1 + 1e-9 && uv.1 >= v0 - 1e-9 && uv.1 <= v1 + 1e-9;
    let warm = last.get().and_then(|hint| surface.search_nearest_parameter(p3(x), Some(hint), 20));
    let mut found: Vec<(f64, f64)> = warm.into_iter().collect();
    if !warm.is_some_and(inside) {
        found.extend(surface.search_nearest_parameter(p3(x), None, 100));
    }
    if last.get().is_none() && [u0, u1, v0, v1].iter().all(|x| x.is_finite()) {
        for i in 0..5 {
            for j in 0..5 {
                let hint = (u0 + (u1 - u0) * (i as f64 + 0.5) / 5.0, v0 + (v1 - v0) * (j as f64 + 0.5) / 5.0);
                found.extend(surface.search_nearest_parameter(p3(x), Some(hint), 50));
            }
        }
    }
    let clamped: Vec<(f64, f64)> = found.iter().map(|(u, v)| (u.clamp(u0, u1), v.clamp(v0, v1))).collect();
    found.extend(clamped);
    let d = |uv: &(f64, f64)| from_p3(surface.subs(uv.0, uv.1)).dist(x);
    let uv = found.into_iter().filter(|uv| d(uv).is_finite()).min_by(|a, b| d(a).total_cmp(&d(b)))?;
    last.set(Some(uv));
    let n = surface.normal(uv.0, uv.1);
    Some((from_p3(surface.subs(uv.0, uv.1)), Vec3::new(n.x, n.y, n.z).normalized()?, uv))
}

/// The strip with parameters in proportion to its lengths across and along (the same surface).
/// The mesher triangulates a face in its parameters: on a unit square for a strip a few
/// millimetres wide and tens long, the points of a slanted end fan out into triangles so long
/// along the curve that they turn over.
fn in_proportion(surface: &mt::Surface) -> Result<mt::Surface> {
    use mt::ParametricSurface;
    let fail = || KernelError::Failed("curve blend: strip parameters".into());
    let mt::Surface::NurbsSurface(ns) = surface else { return Err(fail()) };
    let length = |at: &dyn Fn(f64) -> Vec3| (0..32).map(|i| at(i as f64 / 32.0).dist(at((i + 1) as f64 / 32.0))).sum::<f64>();
    let across = length(&|u| from_p3(surface.subs(u, 0.5)));
    let along = length(&|v| from_p3(surface.subs(0.5, v)));
    if !(across.is_finite() && along.is_finite() && across > 1e-9 && along > 1e-9) {
        return Err(fail());
    }
    let bs = ns.non_rationalized();
    let (mut uk, mut vk) = bs.knot_vecs().clone();
    uk.transform(across, 0.0);
    vk.transform(along, 0.0);
    let bs = mt::BSplineSurface::try_new((uk, vk), bs.control_points().clone()).map_err(|_| fail())?;
    Ok(mt::Surface::NurbsSurface(mt::NurbsSurface::new(bs)))
}

/// A chain edge the ball turns about instead of rolling along: an arc of the blend's radius on a
/// planar side (`side`), curving round the ball (a rounded corner rounded again). The ball's
/// centre stays at `c`, it touches the planar side only at the arc's centre `o`, and the blend
/// there is a piece of sphere (of cone, for a chamfer) about the axis through `c` along `axis`.
#[derive(Clone, Copy, Debug)]
struct Pivot {
    c: Vec3,
    o: Vec3,
    axis: Vec3,
    side: usize,
}

/// The circle an edge lies on (centre, radius, axis), if it is an arc of one within `tol`.
pub(crate) fn circle_of(e: &mt::Edge, tol: f64) -> Option<(Vec3, f64, Vec3)> {
    use mt::{BoundedCurve, ParametricCurve};
    let curve = e.oriented_curve();
    let (t0, t1) = curve.range_tuple();
    let at = |f: f64| from_p3(curve.subs(t0 + (t1 - t0) * f));
    let (a, u, v) = (at(0.0), at(0.5) - at(0.0), at(1.0) - at(0.0));
    let w = u.cross(v);
    if w.len2() < 1e-24 {
        return None;
    }
    let o = a + (v * u.len2() - u * v.len2()).cross(w) * (1.0 / (2.0 * w.len2()));
    let (rho, axis) = (o.dist(a), w.normalized()?);
    (0..=8).map(|i| at(i as f64 / 8.0)).all(|p| (p.dist(o) - rho).abs() <= tol && (p - o).dot(axis).abs() <= tol).then_some((o, rho, axis))
}

/// The surface the section (homogeneous controls from contact 1 to contact 2) sweeps turning by
/// `angle` about the axis through `c` along the unit `axis`: rational quadratic arcs in v, a
/// quarter turn at most each, so it is exact.
fn revolve(ctrl: [mt::Vector4; 3], c: Vec3, axis: Vec3, angle: f64) -> Option<mt::Surface> {
    let pieces = ((angle.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).clamp(1, 8);
    let step = angle / pieces as f64;
    let half = (step / 2.0).cos();
    if !(half.is_finite() && half > 1e-6) {
        return None;
    }
    let rot = |x: Vec3, phi: f64| {
        let (s, co) = phi.sin_cos();
        x * co + axis.cross(x) * s + axis * (axis.dot(x) * (1.0 - co))
    };
    let h = |p: Vec3, w: f64| mt::Vector4::new(p.x * w, p.y * w, p.z * w, w);
    let mut rows = Vec::with_capacity(3);
    for q in ctrl {
        if !(q.w.is_finite() && q.w > 1e-12) {
            return None;
        }
        let x = Vec3::new(q.x / q.w, q.y / q.w, q.z / q.w) - c;
        let along = axis * axis.dot(x);
        let mut row = vec![h(c + x, q.w)];
        for i in 0..pieces {
            let a0 = step * i as f64;
            row.push(h(c + along + rot(x - along, a0 + step / 2.0) * (1.0 / half), q.w * half));
            row.push(h(c + rot(x, a0 + step), q.w));
        }
        rows.push(row);
    }
    let mut knots = vec![0.0; 3];
    for i in 1..pieces {
        let k = i as f64 / pieces as f64;
        knots.extend([k, k]);
    }
    knots.extend([1.0; 3]);
    let bs = mt::BSplineSurface::try_new((mt::KnotVec::bezier_knot(2), mt::KnotVec::from(knots)), rows).ok()?;
    Some(mt::Surface::NurbsSurface(mt::NurbsSurface::new(bs)))
}

/// Whether the edge is straight: a line, or a curve on the segment between its ends within
/// `tol` (an earlier blend's contact along a straight edge is a fitted curve).
pub(crate) fn straight(e: &mt::Edge, tol: f64) -> bool {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.curve();
    if matches!(c, mt::Curve::Line(_)) {
        return true;
    }
    let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
    let (t0, t1) = c.range_tuple();
    a.dist(b) > tol && (0..=16).all(|i| from_p3(c.subs(t0 + (t1 - t0) * i as f64 / 16.0)).dist_to_segment(a, b) <= tol)
}

/// Consuming both seams of a side can leave its old boundary and a fitted contact on the
/// same curve (#1113). They are distinct topological edges but enclose no usable face. Keep
/// genuine two-edge faces (a lens or lune); refuse only coincident traces, in both directions
/// since the curves need not have the same parameterisation.
fn collapsed_boundary(edges: &[mt::Edge], tolerance: f64) -> bool {
    use mt::{BoundedCurve, ParametricCurve};
    let [a, b] = edges else { return false };
    [(a, b), (b, a)].into_iter().all(|(edge, other)| {
        let curve = edge.curve();
        let (lo, hi) = curve.range_tuple();
        (0..=16).all(|k| {
            let p = from_p3(curve.subs(lo + (hi - lo) * f64::from(k) / 16.0));
            crate::blend::on_edge(other, p, tolerance).is_some()
        })
    })
}

/// A curved seam (an arc of an end face) shortened to the blend's contacts: the part of its
/// curve between the new ends `a` and `b` (`None` keeps that end), which must lie on it, in
/// order, short of each other.
pub(crate) fn trim_curved_seam(seam: &mt::Edge, a: Option<mt::Vertex>, b: Option<mt::Vertex>, size: f64) -> Result<mt::Edge> {
    // A closed seam (a full circle starting and ending at one vertex) has no "between": which
    // end a contact replaces, and which way round the kept piece runs, are both ambiguous.
    if seam.front() == seam.back() {
        return Err(unsupported("a closed curved edge where the blend ends"));
    }
    use mt::{BoundedCurve, Cut, SearchNearestParameter};
    let mut curve = seam.curve();
    let (lo, hi) = curve.range_tuple();
    let at = |v: &Option<mt::Vertex>, end: f64| -> Result<f64> {
        let Some(v) = v else { return Ok(end) };
        // Start from the nearest of a few samples: from the far end of a long arc
        // the search can settle on the opposite side of the circle.
        let q = from_p3(v.point());
        let hint = (0..=32)
            .map(|i| lo + (hi - lo) * i as f64 / 32.0)
            .min_by(|x, y| {
                let d = |t: f64| from_p3(mt::ParametricCurve::subs(&curve, t)).dist(q);
                d(*x).total_cmp(&d(*y))
            })
            .unwrap_or(end);
        let t = curve.search_nearest_parameter(v.point(), hint, 100).ok_or_else(|| unsupported("the blend is larger than the neighbouring faces"))?;
        if !(t.is_finite() && from_p3(mt::ParametricCurve::subs(&curve, t)).dist(from_p3(v.point())) <= size * 1e-5) {
            return Err(too_large("the blend is larger than the neighbouring faces (site 938)"));
        }
        Ok(t)
    };
    let (ta, tb) = (at(&a, lo)?, at(&b, hi)?);
    let margin = (hi - lo) * 1e-9;
    if ta < lo - margin || tb > hi + margin || tb - ta < (hi - lo) * 1e-6 {
        return Err(too_large("the blend is larger than the neighbouring faces (site 945)"));
    }
    if tb < hi {
        let _ = curve.cut(tb);
    }
    let piece = if ta > lo { curve.cut(ta) } else { curve };
    let a = a.unwrap_or_else(|| seam.front().clone());
    let b = b.unwrap_or_else(|| seam.back().clone());
    mt::Edge::try_new(&a, &b, piece).map_err(|e| KernelError::Failed(format!("curve blend: seam: {e}")))
}

/// A monotone curve's parameter read off a table of its values at uniform parameters (`None` for
/// an empty table): the fraction of the way through it.
fn sampled(table: &[f64], along: f64) -> Option<f64> {
    if table.len() < 2 || !along.is_finite() {
        return None;
    }
    let x = along.clamp(0.0, 1.0) * (table.len() - 1) as f64;
    let i = (x.floor() as usize).min(table.len() - 2);
    let f = x - i as f64;
    Some(table.get(i)? * (1.0 - f) + table.get(i + 1)? * f)
}

/// The arc length of `edge` from its front to its back at each of `LENGTH_STEPS + 1` uniform
/// parameters, accumulated. `None` for an edge that has no length (or runs back over itself).
fn edge_lengths(edge: &mt::Edge) -> Option<Vec<f64>> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = edge.oriented_curve();
    let (a, b) = c.range_tuple();
    let mut last = from_p3(c.subs(a));
    let mut total = 0.0;
    let mut out = Vec::with_capacity(LENGTH_STEPS + 1);
    out.push(0.0);
    for i in 1..=LENGTH_STEPS {
        let p = from_p3(c.subs(a + (b - a) * i as f64 / LENGTH_STEPS as f64));
        total += last.dist(p);
        last = p;
        out.push(total);
    }
    (total.is_finite() && total > 1e-12).then_some(out)
}

/// The radius a law gives at the chain fraction `s`, continued past 0 and 1 at the law's own end
/// slope. The ball runs on past a chain end (to be cut back there by its end face), and holding
/// the end radius would kink the strip exactly where the edge begins: a kink a cubic fit spreads
/// back over the whole blend. Never below a fifth of the end radius, so that a law running down
/// steeply still has a ball to run on with.
fn law_beyond(law: &RadiusSpline, s: f64) -> Option<f64> {
    if (0.0..=1.0).contains(&s) {
        return Some(law.eval(s));
    }
    let (end, step) = if s < 0.0 { (0.0, s) } else { (1.0, s - 1.0) };
    let h = 1e-3;
    let slope = (law.eval(end + h) - law.eval(end)) / h;
    let r = law.eval(end);
    let r = (r + slope * step).max(0.2 * r);
    (r.is_finite() && r > 0.0).then_some(r)
}

/// A chain end where the round merges with an earlier round of the other kind
/// ([`crate::merge_corner`]): the strip runs on to the section through `L` and ends on `Γ_X`, the
/// earlier round `g` ends on `Γ_g`, and the band `R` between them closes the corner.
struct MergeEnd {
    /// The chain's vertex, its edge there and the earlier round's face.
    k: usize,
    ke: usize,
    g: usize,
    /// The side (1 or 2) that is `T`.
    top: u8,
    zone: crate::merge_corner::Zone,
    end: crate::merge_corner::MixedEnd,
}

impl MergeEnd {
    /// Whether the later round (this chain's) adds material: it is `A`, the earlier `B`.
    fn later_adds(&self) -> bool {
        !self.end.earlier_adds
    }

    /// Where the later round's contact with the face beside the chain meets `L`.
    fn q_x(&self) -> Vec3 {
        if self.later_adds() { self.zone.q_w } else { self.zone.q_s }
    }

    /// The later round's edge with the band at `t` (`P*` at 0, `q_X` at 1).
    fn gamma_x(&self, t: f64) -> Vec3 {
        if self.later_adds() { self.zone.gamma_a(t) } else { self.zone.gamma_b(t) }
    }
}

/// The merge at the end `k` of the chain (end face `fi`, tangent to the chain's side `side`), if
/// the end face is an earlier round of the other kind ending flush with a wall's end, planned or
/// refused where the corner does not fit. `end_faces`: the faces the chain ends on, by vertex
/// (the first reached by side 1's contact); `tols`: the chain's tolerance and the fit's.
#[allow(clippy::too_many_arguments)]
fn merge_end(
    faces: &[mt::Face],
    (side1, side2): (&[usize], &[usize]),
    chain: &[mt::Edge],
    verts: &[mt::Vertex],
    end_faces: &[(usize, usize)],
    (k, fi, side): (usize, usize, u8),
    convex: bool,
    fixed: Option<f64>,
    (tol, fit_tolerance): (f64, f64),
) -> Result<Option<MergeEnd>> {
    let ke = if k == 0 { 0 } else { chain.len().saturating_sub(1) };
    let (top, other) = if side == 1 { (side1.get(ke), side2.get(ke)) } else { (side2.get(ke), side1.get(ke)) };
    let (Some(t), Some(x), Some(v), Some(g), Some(terminal)) =
        (top.and_then(|i| faces.get(*i)), other.and_then(|i| faces.get(*i)), verts.get(k), faces.get(fi), chain.get(ke))
    else {
        return Ok(None);
    };
    let found = crate::guard("curve blend end", || Ok(crate::merge_corner::mixed_end(faces, v, t, x, g, fit_tolerance))).ok().flatten();
    let Some(end) = found.filter(|m| m.earlier_adds == convex) else { return Ok(None) };
    let refuse = |why: &str| KernelError::TooLarge(format!("{}: {why}", crate::blend::MIXED_END));
    let r = fixed.ok_or_else(|| refuse("not with a chord width or a radius law"))?;
    let p = from_p3(v.point());
    // The chain runs straight back from its end as far as its last edge is straight; its contact
    // with `T` as far as the plane the chain ends on there, if it ends there on a plane (on into
    // the next edge, the contact's straight run ends square to the edge, as the edge's does).
    let far = if terminal.front() == v { terminal.back() } else { terminal.front() };
    let is_straight = straight(terminal, tol * 10.0);
    let reach = if is_straight { from_p3(far.point()) } else { p };
    let kf = if k == 0 { 1 } else { k.saturating_sub(1) };
    let ends: Vec<usize> = end_faces.iter().filter(|(kk, _)| *kk == kf).map(|(_, f)| *f).collect();
    let on = match ends[..] {
        [one] => Some(one),
        [first, second] => Some(if side == 1 { first } else { second }),
        _ => None,
    };
    let end_plane = on.filter(|_| is_straight).and_then(|i| faces.get(i)).and_then(crate::blend::plane_normal).map(|n| (from_p3(far.point()), n));
    let zone = end.plan(p, r, crate::merge_corner::Run { reach, end: end_plane })?;
    Ok(Some(MergeEnd { k, ke, g: fi, top: side, zone, end }))
}

/// Where the strip `fit`, run on to the section through `L`, ends at the merge `m`: its contacts'
/// parameters at `P*` (on `T`) and at `q_X` (where its other contact meets `L`), and `Γ_X` from
/// contact 1 to contact 2. `Γ_X` is the exact curve: the strip is checked to hold it within
/// `tolerance` along its length, between its own contacts and within its own part.
fn merge_cap(fit: &Fit, m: &MergeEnd, tolerance: f64) -> Result<(f64, f64, mt::Curve)> {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface, SearchNearestParameter};
    let refuse = |why: &str| KernelError::TooLarge(format!("{}: {why}", crate::blend::MIXED_END_SIZE));
    let off = || refuse("the merged corner does not lie on the round");
    let (lo, hi) = (fit.range.0.min(fit.range.1), fit.range.0.max(fit.range.1));
    let inside = |t: f64| t > lo + 1e-6 && t < hi - 1e-6;
    // The contacts' parameters where `P*` and `q_X` lie on them.
    let on_contact = |row: usize, q: Vec3| -> Option<f64> {
        let curve = fit.contacts.get(row)?;
        let (a, b) = curve.range_tuple();
        let hint = (0..=64)
            .map(|i| a + (b - a) * f64::from(i) / 64.0)
            .min_by(|x, y| from_p3(curve.subs(*x)).dist(q).total_cmp(&from_p3(curve.subs(*y)).dist(q)))?;
        let t = curve.search_nearest_parameter(p3(q), hint, 100)?;
        (inside(t) && from_p3(curve.subs(t)).dist(q) <= tolerance).then_some(t)
    };
    let (row_t, row_x) = if m.top == 1 { (0, 1) } else { (1, 0) };
    let t_top = on_contact(row_t, m.zone.p_star).ok_or_else(off)?;
    let t_x = on_contact(row_x, m.q_x()).ok_or_else(off)?;
    // `Γ_X` on the strip: its points, and halfway between them, near the strip within its part.
    let surf = &fit.surface;
    let mut hint: Option<(f64, f64)> = None;
    for i in 0..=64 {
        let x = m.gamma_x(f64::from(i) / 64.0);
        let (u, v) = surf.search_nearest_parameter(p3(x), hint, 100).or_else(|| surf.search_nearest_parameter(p3(x), None, 100)).ok_or_else(off)?;
        let near = from_p3(surf.subs(u, v)).dist(x) <= tolerance;
        if !(near && (-1e-6..=1.0 + 1e-6).contains(&u) && v > lo - 1e-9 && v < hi + 1e-9) {
            return Err(off());
        }
        hint = Some((u, v));
    }
    let curve = if m.later_adds() { m.zone.curve_a()? } else { m.zone.curve_b()? };
    // From contact 1 to contact 2: from `P*` where `T` is side 1.
    let curve = if m.top == 1 {
        curve
    } else {
        crate::guard("merge corner end", || {
            let mut c = curve.clone();
            c.invert();
            Ok(c)
        })?
    };
    let (t1, t2) = if m.top == 1 { (t_top, t_x) } else { (t_x, t_top) };
    Ok((t1, t2, mt::Curve::NurbsCurve(curve)))
}

/// Blend the closed chain of edges (ids in any order) with radius `r`; with what it built from
/// `solid` (each blend face in place of its chain edge).
pub(crate) fn curve_blend(solid: &Solid, ids: &[mt::EdgeID], size: f64, r: f64, round: bool) -> Result<(Solid, Rebuild)> {
    curve_blend_with_limit(solid, ids, size, r, round, MAX_SAMPLES)
}

/// [`curve_blend`] with the blend `chord` wide across, from contact to contact, at every station.
pub(crate) fn curve_blend_chord(solid: &Solid, ids: &[mt::EdgeID], size: f64, chord: f64, round: bool) -> Result<(Solid, Rebuild)> {
    curve_blend_sized(solid, ids, size, Size::Chord(chord), round, MAX_SAMPLES)
}

/// [`curve_blend`] with the radius following `law` along the chain (its positions the fraction of
/// the chain's arc length, from the front of its first edge to the back of its last).
pub(crate) fn curve_blend_law(solid: &Solid, ids: &[mt::EdgeID], size: f64, law: &RadiusSpline, round: bool) -> Result<(Solid, Rebuild)> {
    curve_blend_sized(solid, ids, size, Size::Law(law), round, MAX_SAMPLES)
}

fn curve_blend_with_limit(solid: &Solid, ids: &[mt::EdgeID], size: f64, r: f64, round: bool, max_samples: usize) -> Result<(Solid, Rebuild)> {
    curve_blend_sized(solid, ids, size, Size::Constant(r), round, max_samples)
}

/// Blend the chain of edges (ids in any order) sized by `size`: one radius, a chord width, or a
/// radius law. With what it built from `solid` (each blend face in place of its chain edge).
fn curve_blend_sized(solid: &Solid, ids: &[mt::EdgeID], size: f64, size_law: Size<'_>, round: bool, max_samples: usize) -> Result<(Solid, Rebuild)> {
    if !(size.is_finite() && size > 0.0) {
        return Err(KernelError::Invalid("curve blend size and radius must be finite and positive".into()));
    }
    match size_law {
        Size::Constant(r) | Size::Chord(r) => {
            if !(r.is_finite() && r > 1e-6 && r < 1e6) {
                return Err(KernelError::Invalid("curve blend size and radius must be finite and positive".into()));
            }
        }
        Size::Law(law) => {
            let knots = law.knots.len() == law.radii.len() + law.degree + 1;
            if law.radii.len() < 2 || !knots || law.radii.iter().any(|r| !(r.is_finite() && (0.0..1e6).contains(r))) {
                return Err(KernelError::Invalid("a radius law along the edge must be finite and non-negative".into()));
            }
            // A law that runs to zero closes the blend at the edge's end point. The fitted strip
            // pinches to a point there, and the sections' own interpolation cannot follow the last
            // span of it: the straight-edge fillet, which builds that closure exactly, is the way.
            if law.eval(0.0) <= CLOSED || law.eval(1.0) <= CLOSED {
                return Err(unsupported("a radius law that runs to zero along a curved edge"));
            }
        }
    }
    if !(4..=MAX_SAMPLES).contains(&max_samples) {
        return Err(KernelError::Invalid("curve blend section limit is outside the supported range".into()));
    }
    let tol = (size * 1e-7).max(1e-9);
    let shells = solid.boundaries();
    let (si, shell) = shells
        .iter()
        .enumerate()
        .find(|(_, sh)| sh.edge_iter().any(|e| ids.contains(&e.id())))
        .ok_or_else(|| KernelError::Failed("edge not found".into()))?;
    let faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    // Order the chain: edges as stored (absolute direction), each starting where the last ended.
    let mut chain: Vec<mt::Edge> = Vec::new();
    let all: Vec<mt::Edge> = {
        let mut seen = std::collections::HashSet::new();
        shell.edge_iter().filter(|e| ids.contains(&e.id()) && seen.insert(e.id())).map(|e| e.absolute_clone()).collect()
    };
    // An open chain starts at a vertex only one of its edges touches.
    let uses = |v: &mt::Vertex| all.iter().filter(|e| e.front() == v || e.back() == v).count();
    let first = match all.iter().find_map(|e| {
        if uses(e.front()) == 1 {
            Some(e.clone())
        } else if uses(e.back()) == 1 {
            Some(e.inverse())
        } else {
            None
        }
    }) {
        Some(e) => e,
        None => all.first().cloned().ok_or_else(|| KernelError::Failed("no edges".into()))?,
    };
    chain.push(first.clone());
    while chain.len() < all.len() {
        let end = chain.last().map(|e| e.back().clone()).ok_or_else(|| KernelError::Failed("chain".into()))?;
        let next = all
            .iter()
            .find(|e| !chain.iter().any(|c| c.id() == e.id()) && (*e.front() == end || *e.back() == end))
            .ok_or_else(|| unsupported("the edges don't form one chain"))?;
        chain.push(if *next.front() == end { next.clone() } else { next.inverse() });
    }
    let closed = chain.first().map(|e| e.front().clone()) == chain.last().map(|e| e.back().clone());
    let n = chain.len();
    // The chain's vertices (n for a closed chain, n + 1 for an open one).
    let mut verts: Vec<mt::Vertex> = chain.iter().map(|e| e.front().clone()).collect();
    if !closed && let Some(e) = chain.last() {
        verts.push(e.back().clone());
    }
    let nv = verts.len();
    // Faces of each edge, split into the two sides.
    let faces_of = |e: &mt::Edge| -> Vec<usize> {
        (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.edge_iter().any(|x| x.id() == e.id()))).collect()
    };
    let mut side1: Vec<usize> = Vec::new();
    let mut side2: Vec<usize> = Vec::new();
    for (k, e) in chain.iter().enumerate() {
        let fs = faces_of(e);
        let [a, b] = fs[..] else { return Err(unsupported("an edge without two faces")) };
        if k == 0 {
            side1.push(a);
            side2.push(b);
            continue;
        }
        // The side-1 face of this edge is the one whose normal matches side 1 at the joint.
        let p = from_p3(e.front().point());
        let nrm = |fi: usize| faces.get(fi).and_then(|f| project(f, p, None)).map(|x| x.1);
        let prev1 = side1.last().copied().and_then(nrm);
        let (na, nb) = (nrm(a), nrm(b));
        let a_is_1 = match (prev1, na, nb) {
            (Some(x), Some(y), Some(z)) => x.dot(y) >= x.dot(z),
            _ => return Err(KernelError::Failed("blend: normals".into())),
        };
        if a_is_1 {
            side1.push(a);
            side2.push(b);
        } else {
            side1.push(b);
            side2.push(a);
        }
    }
    let s1: Vec<&mt::Face> = side1.iter().filter_map(|i| faces.get(*i)).collect();
    let s2: Vec<&mt::Face> = side2.iter().filter_map(|i| faces.get(*i)).collect();
    // The faces the ball may touch near edges `ks` of the chain: their own, on each side. (The
    // surface of another face of a side, such as a wall's plane past the bend it runs into,
    // may pass nearer than the face the ball actually rolls on.)
    let edge_sides = |ks: &[usize]| -> (Vec<&mt::Face>, Vec<&mt::Face>) {
        let pick = |side: &[usize]| {
            let mut out: Vec<usize> = Vec::new();
            for k in ks {
                if let Some(fi) = side.get(*k)
                    && !out.contains(fi)
                {
                    out.push(*fi);
                }
            }
            out.iter().filter_map(|i| faces.get(*i)).collect::<Vec<_>>()
        };
        (pick(&side1), pick(&side2))
    };
    // The chain's edges at vertex k.
    let vertex_edges = |k: usize| -> Vec<usize> {
        if closed {
            vec![(k + n - 1) % n, k % n]
        } else if k == 0 {
            vec![0]
        } else if k >= n {
            vec![n - 1]
        } else {
            vec![k - 1, k]
        }
    };
    // Convex or concave, from the first edge as its side-1 face runs it.
    let e0 = chain.first().ok_or_else(|| KernelError::Failed("chain".into()))?;
    let f1 = s1.first().ok_or_else(|| KernelError::Failed("side".into()))?;
    let in_f1 = f1.boundary_iters().into_iter().flatten().find(|e| e.id() == e0.id()).ok_or_else(|| KernelError::Failed("edge use".into()))?;
    let (pm, t1) = {
        use mt::{BoundedCurve, ParametricCurve};
        let c = in_f1.oriented_curve();
        let (a, b) = c.range_tuple();
        let m = (a + b) * 0.5;
        let d = c.der(m);
        (from_p3(c.subs(m)), Vec3::new(d.x, d.y, d.z).normalized().ok_or_else(|| KernelError::Failed("tangent".into()))?)
    };
    let (n1, n2) = match (project(f1, pm, None), s2.first().and_then(|f| project(f, pm, None))) {
        (Some(x), Some(y)) => (x.1, y.1),
        _ => return Err(KernelError::Failed("blend: normals".into())),
    };
    // A knife edge (opposite normals): both faces leave the edge along `w`; the second one lies
    // below the first (against its outward normal) when the cusp is material, above when it is
    // a gap. The cross product of the normals, which tells the other edges apart, is zero here.
    let knife = 1.0 + n1.dot(n2) < KNIFE;
    // The one radius, where the size is one radius (a chord or a law has none).
    let fixed = match size_law {
        Size::Constant(r) => Some(r),
        _ => None,
    };
    // The blend's scale where the size itself is not what matters (the knife edge's probe): the
    // radius asked for, or the law's middle value.
    let scale = match size_law {
        Size::Constant(r) | Size::Chord(r) => r,
        Size::Law(law) => law.eval(0.5).max(1e-6),
    };
    let (convex, cusp) = if knife {
        let w = n1.cross(t1).normalized().ok_or_else(|| KernelError::Failed("blend: cusp direction".into()))?;
        // Second order: from the point of face 1 a little way along `w`, face 2's nearest point.
        let probe = project(f1, pm + w * scale, None)
            .and_then(|(qa, na, _)| s2.first().and_then(|f| project(f, qa, None)).map(|(qb, _, _)| (qb - qa).dot(na)))
            .filter(|depth| depth.abs() > size * 1e-9)
            .ok_or_else(|| unsupported("a knife edge whose faces lie flat on each other"))?;
        // Whether the chain runs along the edge as face 1 uses it decides which way `w` points
        // from the chain's tangent.
        (probe < 0.0, if in_f1.front() == e0.front() { 1.0 } else { -1.0 })
    } else {
        (t1.dot(n1.cross(n2)) > 0.0, 0.0)
    };
    let sigma = if convex { -1.0 } else { 1.0 };
    if n1.dot(n2) > 1.0 - 1e-6 {
        return Err(unsupported("blending a smooth edge"));
    }
    // The chain's arc length, in fractions of its whole, at each station: where the size is one
    // radius it is not read, but the sections take their parameter from it all the same.
    let tables: Vec<Vec<f64>> = chain.iter().map(|e| edge_lengths(e).unwrap_or_else(|| vec![0.0; LENGTH_STEPS + 1])).collect();
    let total: f64 = tables.iter().filter_map(|t| t.last().copied()).sum();
    if !matches!(size_law, Size::Constant(_)) && !(total.is_finite() && total > 1e-12) {
        return Err(unsupported("a zero-length edge of the chain"));
    }
    let total = if total > 1e-12 { total } else { 1.0 };
    let mut cum: Vec<f64> = Vec::with_capacity(n + 1);
    let mut acc = 0.0;
    for table in &tables {
        cum.push(acc);
        acc += table.last().copied().unwrap_or(0.0);
    }
    cum.push(acc);
    // A law that runs around a closed chain meets itself: its two ends must agree there.
    if closed
        && let Size::Law(law) = size_law
        && (law.eval(0.0) - law.eval(1.0)).abs() > 1e-9 * law.radii.iter().copied().fold(1.0_f64, f64::max)
    {
        return Err(KernelError::Invalid("a closed edge chain runs back into itself: give the same radius law at both ends".into()));
    }
    // The law's parameter at `along` of edge `k` (its front to its back).
    let frac_at = |k: usize, along: f64| -> f64 {
        let (Some(table), Some(lo)) = (tables.get(k), cum.get(k)) else { return if k >= n { 1.0 } else { 0.0 } };
        let s = sampled(table, along).unwrap_or(0.0);
        ((lo + s) / total).clamp(0.0, 1.0)
    };
    let sizer = Sizer { size: size_law, tol: tol * 100.0 };
    // Arcs on a planar side that curve round the ball: its centre runs round them at their
    // radius less the blend's. Less than nothing is too large; nothing is a pivot.
    let mut pivots: Vec<Option<Pivot>> = vec![None; n];
    'pivots: for (k, e) in chain.iter().enumerate() {
        // A size that varies along the edge has no one radius to compare a corner's arc with.
        let Some(r) = fixed else { break 'pivots };
        let Some((o, rho, _)) = circle_of(e, tol * 1e3) else { continue };
        for side in [1, 2] {
            let Some(f) = (if side == 1 { side1.get(k) } else { side2.get(k) }).and_then(|i| faces.get(*i)) else { continue };
            let mt::Surface::Plane(pl) = f.oriented_surface() else { continue };
            let Some(np) = Vec3::new(pl.normal().x, pl.normal().y, pl.normal().z).normalized() else { continue };
            // Into the face (left of the edge as the face runs it, about its outward normal).
            let Some(used) = f.boundary_iters().into_iter().flatten().find(|x| x.id() == e.id()) else { continue };
            let Some((pm, tm)) = ({
                use mt::{BoundedCurve, ParametricCurve};
                let c = used.oriented_curve();
                let (a, b) = c.range_tuple();
                let d = c.der((a + b) * 0.5);
                Vec3::new(d.x, d.y, d.z).normalized().map(|t| (from_p3(c.subs((a + b) * 0.5)), t))
            }) else {
                continue;
            };
            if (o - pm).dot(np.cross(tm)) <= 0.0 {
                continue;
            }
            if rho < r - tol * 1e3 {
                return Err(too_large("the blend is larger than a corner radius (site 1257)"));
            }
            if (rho - r).abs() > tol * 1e3 {
                continue;
            }
            // The ball sits on the planar side at the arc's centre; the other side must be a round
            // about the same axis, the ball touching it all along.
            let c = o + np * (sigma * r);
            let (a1, a2) = edge_sides(&[k]);
            let other = if side == 1 { a2 } else { a1 };
            let reach = other.iter().filter_map(|f| project(f, c, None)).map(|x| x.0.dist(c)).fold(f64::INFINITY, f64::min);
            if (reach - r).abs() <= size * 1e-4 {
                if let Some(slot) = pivots.get_mut(k) {
                    *slot = Some(Pivot { c, o, axis: np, side });
                }
                break;
            }
        }
    }
    if pivots.iter().all(Option::is_some) {
        return Err(unsupported("rounding again only rounded corners of the blend's radius"));
    }
    // Seams at the chain's vertices: edges there that aren't in the chain, and their side.
    let chain_ids: Vec<mt::EdgeID> = chain.iter().map(|e| e.id()).collect();
    let mut seams: Vec<Vec<(usize, mt::Edge)>> = Vec::with_capacity(nv);
    // At an open chain's ends: the end face (planar, square to the edge there).
    let mut end_faces: Vec<(usize, usize)> = Vec::new(); // (vertex index, face index)
    // A strip on planar sides can end across two planar faces (an earlier chamfer, or a wall's
    // end flush with the face beneath it). Their shared seam ends where the strip crosses it:
    // shortened to it, or (a round) run on to it past the vertex.
    let mut split_ends: Vec<(usize, usize, usize, mt::Edge)> = Vec::new();
    // Chain ends whose end face is not a plane square to the edge: (vertex, face, point, tangent,
    // the side (1 or 2) the end face is tangent to when it lies along the edge there).
    let mut slanted: Vec<(usize, usize, Vec3, Vec3, Option<u8>)> = Vec::new();
    for (k, v) in verts.iter().enumerate() {
        let at_end = !closed && (k == 0 || k == nv - 1);
        let (k1, k2) = {
            let ei = if k == nv - 1 && !closed { n - 1 } else { k.min(n - 1) };
            (side1.get(ei).copied().unwrap_or(usize::MAX), side2.get(ei).copied().unwrap_or(usize::MAX))
        };
        let mut found: Vec<(usize, mt::Edge)> = Vec::new();
        for (fi, f) in faces.iter().enumerate() {
            if !f.vertex_iter().any(|x| x == *v) {
                continue;
            }
            let on_side = side1.contains(&fi) || side2.contains(&fi);
            if !on_side {
                if !at_end || end_faces.iter().filter(|(kk, _)| *kk == k).count() >= 2 {
                    return Err(unsupported("a vertex of the edge chain where other faces meet"));
                }
                end_faces.push((k, fi));
                continue;
            }
            for x in f.edge_iter() {
                if chain_ids.contains(&x.id()) || !(*x.front() == *v || *x.back() == *v) || found.iter().any(|(_, y)| y.id() == x.id()) {
                    continue;
                }
                let side = if fi == k1 || (side1.contains(&fi) && fi != k2) { 1 } else { 2 };
                found.push((side, x.absolute_clone()));
            }
        }
        if at_end && !end_faces.iter().any(|(kk, _)| *kk == k) {
            return Err(unsupported("an open chain of curved edges must end at a face"));
        }
        let ends: Vec<usize> = end_faces.iter().filter(|(kk, _)| *kk == k).map(|(_, fi)| *fi).collect();
        if let [ga, gb] = ends[..] {
            let terminal = chain.get(if k == 0 { 0 } else { n - 1 }).ok_or_else(|| unsupported("chain end"))?;
            let planar = [ga, gb, k1, k2].iter().all(|i| faces.get(*i).and_then(crate::blend::plane_normal).is_some());
            if !planar || !straight(terminal, tol * 10.0) {
                return Err(unsupported("a vertex of the edge chain where other faces meet"));
            }
            let shares = |g: usize, side: usize| {
                faces
                    .get(g)
                    .zip(faces.get(side))
                    .is_some_and(|(g, s)| g.edge_iter().any(|e| (e.front() == v || e.back() == v) && s.edge_iter().any(|x| x.id() == e.id())))
            };
            let (ga, gb) = if shares(ga, k1) && shares(gb, k2) {
                (ga, gb)
            } else if shares(gb, k1) && shares(ga, k2) {
                (gb, ga)
            } else {
                return Err(unsupported("the end faces do not meet both sides of the chain"));
            };
            let seam = faces
                .get(ga)
                .and_then(|a| {
                    faces.get(gb).and_then(|b| a.edge_iter().find(|e| (e.front() == v || e.back() == v) && b.edge_iter().any(|x| x.id() == e.id())))
                })
                .ok_or_else(|| unsupported("the end faces have no shared seam"))?;
            if !straight(&seam, tol * 10.0) {
                return Err(unsupported("a curved seam between the end faces"));
            }
            // The first face is the one reached by side 1's contact.
            end_faces.retain(|(kk, _)| *kk != k);
            end_faces.extend([(k, ga), (k, gb)]);
            split_ends.push((k, ga, gb, seam.absolute_clone()));
        }
        // At most one seam per side.
        if found.iter().filter(|x| x.0 == 1).count() > 1 || found.iter().filter(|x| x.0 == 2).count() > 1 {
            return Err(unsupported("a vertex of the edge chain where several seams meet"));
        }
        seams.push(found);
    }
    // Sections: at each vertex (a contact on the seam there), then along each edge.
    let tangent_at = |e: &mt::Edge, s: f64| -> Option<(Vec3, Vec3)> {
        use mt::{BoundedCurve, ParametricCurve};
        let c = e.oriented_curve();
        let (a, b) = c.range_tuple();
        let t = a + (b - a) * s;
        let d = c.der(t);
        Some((from_p3(c.subs(t)), Vec3::new(d.x, d.y, d.z).normalized()?))
    };
    let mut vsec: Vec<Section> = Vec::with_capacity(nv);
    // The radius each vertex section was solved at (the closed end of a law: zero).
    let mut vradius: Vec<f64> = Vec::with_capacity(nv);
    for k in 0..nv {
        let (p, t) = if k < n {
            tangent_at(chain.get(k).ok_or_else(|| KernelError::Failed("edge".into()))?, 0.0)
        } else {
            tangent_at(chain.get(n - 1).ok_or_else(|| KernelError::Failed("edge".into()))?, 1.0)
        }
        .ok_or_else(|| KernelError::Failed("tangent".into()))?;
        let here = seams.get(k).cloned().unwrap_or_default();
        let at_end = end_faces.iter().any(|(kk, _)| *kk == k);
        // Next to a pivot the ball is at its centre, touching the other side square above the
        // vertex.
        let pivoting: Vec<Pivot> = [(k < n).then_some(k), if k > 0 { Some(k - 1) } else { closed.then_some(n - 1) }]
            .into_iter()
            .flatten()
            .filter_map(|i| pivots.get(i).copied().flatten())
            .collect();
        if let Some(pv) = pivoting.first().copied() {
            if at_end || pivoting.iter().any(|x| x.c.dist(pv.c) > tol * 1e3) {
                return Err(unsupported("a rounded corner of the blend's radius where the edge chain ends or turns"));
            }
            let q = pv.c + (p - pv.o);
            vsec.push(if pv.side == 1 { Section { c: pv.c, q1: pv.o, q2: q } } else { Section { c: pv.c, q1: q, q2: pv.o } });
            vradius.push(fixed.unwrap_or_default());
            continue;
        }
        // The law's parameter at this vertex: the front of its edge, or the back of the last
        // edge at an open chain's end.
        let law_s = if k < n { frac_at(k, 0.0) } else { frac_at(n - 1, 1.0) };
        if at_end {
            // A plane square to the edge holds the blend's end section. Any other end face: the
            // blend runs on past the end and is cut back where it meets that face.
            let (_, fi) = end_faces.iter().find(|(kk, _)| *kk == k).copied().unwrap_or_default();
            let g = faces.get(fi).ok_or_else(|| KernelError::Failed("end face".into()))?;
            let square = match g.oriented_surface() {
                mt::Surface::Plane(pl) => {
                    let m = pl.normal();
                    Vec3::new(m.x, m.y, m.z).normalized().is_some_and(|ng| ng.dot(t).abs() >= 1.0 - 1e-6)
                }
                _ => false,
            };
            // A radius that has run to zero closes the blend at the vertex itself: it does not run
            // on past the end, and the end face keeps the corner it had there.
            let closed_end = matches!(size_law, Size::Law(law) if law.eval(law_s) <= CLOSED);
            if (!square || split_ends.iter().any(|e| e.0 == k)) && !closed_end {
                slanted.push((k, fi, p, t, None));
            }
        }
        let extra = match here.first().filter(|_| !at_end) {
            None => None,
            // A curved seam holds the contact on itself (the chain runs on smoothly across it: a
            // lofted wall's two halves).
            Some((side, seam)) if !straight(seam, tol * 10.0) => Some(Extra::Seam(*side, seam)),
            Some((side, seam)) => {
                let side = *side;
                // The plane through the seam square to that side's surface.
                let o = from_p3(seam.front().point());
                let d = (from_p3(seam.back().point()) - o).normalized().ok_or_else(|| KernelError::Failed("seam".into()))?;
                let fs = if side == 1 { &s1 } else { &s2 };
                // The side's normal at the joint: from its face nearest the joint (any face of a
                // longer side is no guide here).
                let nn = fs
                    .iter()
                    .filter_map(|f| project(f, p, None))
                    .min_by(|a, b| a.0.dist(p).total_cmp(&b.0.dist(p)))
                    .map(|x| x.1)
                    .ok_or_else(|| KernelError::Failed("seam normal".into()))?;
                Some(Extra::Plane(side, o, d.cross(nn).normalized().ok_or_else(|| KernelError::Failed("seam plane".into()))?))
            }
        };
        // On the faces of the edge from this vertex alone (the last edge's at an open chain's
        // end): the other edge's faces are untrimmed here and can run on near the ball where it no
        // longer is (a wall past the round at a corner, which a ball rolling round it pokes
        // through).
        let (a1, a2) = edge_sides(&[k.min(n - 1)]);
        let (s, r) = sizer.section(&a1, &a2, p, t, law_s, sigma, cusp, extra).ok_or_else(|| sizer.refusal())?;
        // A seam on the other side too must pass through that side's contact.
        if let Some((side, seam)) = here.get(1).filter(|_| !at_end) {
            let q = if *side == 1 { s.q1 } else { s.q2 };
            let (a, b) = (from_p3(seam.front().point()), from_p3(seam.back().point()));
            if !straight(seam, tol * 10.0) || q.dist_to_segment(a, b) > tol * 1e3 {
                return Err(unsupported("seams on both sides of a corner of the edge chain"));
            }
        }
        vsec.push(s);
        vradius.push(r);
    }
    // Uniform refinement preserves a common spacing across neighbouring patches. Work is
    // capped: a difficult strip returns an error instead of accepting an inaccurate fit.
    let fit_tolerance = (size * 1e-5).clamp(1e-5, 0.001);
    // Past a slanted end: sections of the same rolling ball beyond the end vertex (far to near
    // at the chain's start, near to far at its end), far enough to cross the end face.
    let mut beyond: Vec<(usize, Vec<Section>, Vec<f64>)> = Vec::new();
    // Ends where the round merges with an earlier round of the other kind.
    let mut merges: Vec<MergeEnd> = Vec::new();
    for (k, fi, p, t, tangent) in slanted.iter_mut() {
        let g = faces.get(*fi).ok_or_else(|| KernelError::Failed("end face".into()))?;
        let at = vsec.get(*k).copied().ok_or_else(|| KernelError::Failed("section".into()))?;
        let (_, ng, _) = project(g, *p, None).ok_or_else(|| unsupported("the blend's end on its end face"))?;
        let cos = split_ends
            .iter()
            .find(|e| e.0 == *k)
            .and_then(|(_, _, gb, _)| faces.get(*gb))
            .and_then(crate::blend::plane_normal)
            .map_or(ng.dot(*t).abs(), |nb| ng.dot(*t).abs().min(nb.dot(*t).abs()));
        // An end face along the edge and tangent to one side there (a wall that runs out into
        // the face): the blend meets it along a curve that touches that side's contact at the
        // vertex. Any other end face that close to the edge is refused.
        let (e1, e2) = edge_sides(&vertex_edges(*k));
        *tangent = if cos < TANGENT_END {
            [(1u8, &e1), (2u8, &e2)].into_iter().find_map(|(side, faces)| {
                faces.iter().filter_map(|f| project(f, *p, None)).any(|(_, n, _)| n.dot(ng).abs() > 1.0 - TANGENT_END).then_some(side)
            })
        } else {
            None
        };
        if cos < 0.15 && tangent.is_none() {
            return Err(unsupported("an end face almost along the edge"));
        }
        let out = if *k == 0 { -*t } else { *t };
        // An earlier round of the other kind ending flush with a wall's end, tangent to the side
        // the chain shares with it ([`crate::merge_corner`]): the two rounds merge there.
        let merge = match *tangent {
            Some(side) if round => {
                merge_end(&faces, (&side1, &side2), &chain, &verts, &end_faces, (*k, *fi, side), convex, fixed, (tol, fit_tolerance))?
            }
            _ => None,
        };
        let width = at.q1.dist(*p).max(at.q2.dist(*p)).max(at.c.dist(*p)) + vradius.get(*k).copied().unwrap_or(scale);
        // At a merge the strip runs on past the end to the section through `L`, and as far again
        // as past any other end.
        let reach = match &merge {
            Some(m) => (m.zone.v - *p).dot(out).max(0.0) + 1.5 * width,
            None => 1.5 * width / if tangent.is_some() { 1.0 } else { cos },
        };
        merges.extend(merge);
        // The run on past the end only gives the fit its context (the end face cuts it back), but
        // it keeps the law's end slope: the radius is read at the distance past the end.
        let law_s = |d: f64| if *k == 0 { -d / total } else { 1.0 + d / total };
        // Past a tangent end the straight run along the tangent leaves the curved spine: where
        // the ball no longer fits, the run is shortened (it only gives the fit its context).
        let mut list = Vec::with_capacity(EXTENSION);
        let mut laws = Vec::with_capacity(EXTENSION);
        let mut reach = reach;
        // Past the end of a curved edge the planes square to its end tangent cut the ring of
        // centres further and further round it, until a step jumps across the ring (#1255: a
        // knuckle's cap circle with a round as wide as 0.7 of its radius). The run is shortened
        // then, as long as one of its sections still lies wholly past the end face.
        let ng_out = if ng.dot(out) < 0.0 { -ng } else { ng };
        let past = |sec: &Section| [sec.c, sec.q1, sec.q2].iter().all(|x| (*x - *p).dot(ng_out) > tol);
        // A run shortened when its centres jump across the ring (a knuckle's cap circle, #1255)
        // stands if no full-length run works; a size law's own end slope is read at each station's
        // distance past the end.
        let mut fallback: Option<Vec<(f64, Section)>> = None;
        for attempt in 0..TANGENT_RETRIES {
            list.clear();
            laws.clear();
            let step = reach / EXTENSION as f64;
            let sections = (1..=EXTENSION).map(|j| {
                let d = step * j as f64;
                let anchor = *p + out * d;
                let s = law_s(d);
                sizer.section(&e1, &e2, anchor, *t, s, sigma, cusp, None).map(|(section, _)| (s, section))
            });
            match sections.collect::<Option<Vec<_>>>() {
                Some(all) => {
                    let smooth = tangent.is_some()
                        || std::iter::once(at.c).chain(all.iter().map(|(_, s)| s.c)).collect::<Vec<_>>().windows(2).all(|w| match w {
                            [a, b] => a.dist(*b) <= MAX_STEP_STRETCH * step,
                            _ => true,
                        });
                    if smooth {
                        // A shortened run must still cross the end face; else the first,
                        // full-length run stands.
                        if fallback.is_none() || all.iter().any(|(_, s)| past(s)) {
                            list = all.iter().map(|(_, s)| *s).collect();
                            laws = all.iter().map(|(s, _)| *s).collect();
                        }
                        break;
                    }
                    fallback.get_or_insert(all);
                }
                None if tangent.is_none() || fallback.is_some() || attempt + 1 == TANGENT_RETRIES => break,
                None => {}
            }
            reach *= if tangent.is_some() { 0.5 } else { 0.75 };
        }
        if list.is_empty() {
            match fallback {
                // The shortened run stands when the ring's step jumped, whatever its size.
                Some(all) => {
                    list = all.iter().map(|(_, s)| *s).collect();
                    laws = all.iter().map(|(s, _)| *s).collect();
                }
                // A size asked for beyond what the faces around the end hold is refused as such;
                // one radius past the end is the rolling ball's own failure to fit.
                None => {
                    return Err(match size_law {
                        Size::Constant(_) => unsupported("the ball doesn't fit past the end of the edge"),
                        _ => too_large("the blend is larger than the neighbouring faces (site 1472)"),
                    });
                }
            }
        }
        if *k == 0 {
            list.reverse();
            laws.reverse();
        }
        beyond.push((*k, list, laws));
    }
    // At a seam the contact lies on the seam plane, which need not be square to the
    // original edge. Its ball centre therefore need not lie in the edge's normal plane.
    // Interpolate that plane's displacement so the interior sections tend to the exact
    // seam sections, rather than leaving a finite jump as sampling gets finer.
    let offsets: Vec<(f64, f64)> = chain
        .iter()
        .enumerate()
        .map(|(k, edge)| {
            let (p0, t0) = tangent_at(edge, 0.0).ok_or_else(|| KernelError::Failed("tangent".into()))?;
            let (p1, t1) = tangent_at(edge, 1.0).ok_or_else(|| KernelError::Failed("tangent".into()))?;
            let start = vsec.get(k).ok_or_else(|| KernelError::Failed("section".into()))?;
            let end = vsec.get((k + 1) % nv).ok_or_else(|| KernelError::Failed("section".into()))?;
            Ok(((start.c - p0).dot(t0), (end.c - p1).dot(t1)))
        })
        .collect::<Result<_>>()?;
    let section_at = |k: usize, edge: &mt::Edge, along: f64| -> Result<(Section, f64)> {
        let (p, t) = tangent_at(edge, along).ok_or_else(|| KernelError::Failed("tangent".into()))?;
        let (start, end) = offsets.get(k).copied().ok_or_else(|| KernelError::Failed("section offset".into()))?;
        let anchor = p + t * (start * (1.0 - along) + end * along);
        let (a1, a2) = edge_sides(&[k]);
        sizer.section(&a1, &a2, anchor, t, frac_at(k, along), sigma, cusp, None).ok_or_else(|| sizer.refusal())
    };
    let mut samples = SAMPLES.min(max_samples);
    let (esec, fits) = loop {
        if n.checked_mul(samples).is_none_or(|count| count > MAX_CHAIN_SAMPLES) {
            return Err(unsupported("curve blend exceeds the section fitting limit"));
        }
        // No guide along a pivot: its sections are the two at its ends.
        let mut guides: Vec<Option<Guide>> = Vec::with_capacity(n);
        for (k, e) in chain.iter().enumerate() {
            if pivots.get(k).copied().flatten().is_some() {
                guides.push(None);
                continue;
            }
            let mut list = vec![*vsec.get(k).ok_or_else(|| KernelError::Failed("section".into()))?];
            let mut law = vec![frac_at(k, 0.0)];
            let (mut front_beyond, mut back_beyond) = (0usize, 0usize);
            for j in 1..samples - 1 {
                let along = j as f64 / (samples - 1) as f64;
                let (section, _) = section_at(k, e, along)?;
                list.push(section);
                law.push(frac_at(k, along));
            }
            list.push(*vsec.get((k + 1) % nv).ok_or_else(|| KernelError::Failed("section".into()))?);
            law.push(frac_at(k, 1.0));
            for (kv, ext, exts) in &beyond {
                if *kv == k && k == 0 {
                    list.splice(0..0, ext.iter().copied());
                    law.splice(0..0, exts.iter().copied());
                    front_beyond = ext.len();
                } else if *kv == k + 1 && k + 1 == nv - 1 {
                    list.extend(ext.iter().copied());
                    law.extend(exts.iter().copied());
                    back_beyond = ext.len();
                }
            }
            let mut guide = Guide::with_law(list, law)?;
            // The chain's own run: the spliced sections at either end are context for the fit
            // only (the blend is cut back where it meets the end face there).
            let last = guide.lengths.last().copied().unwrap_or(0.0);
            let at = |i: usize| if last > 0.0 { guide.lengths.get(i).copied().unwrap_or(0.0) / last } else { 0.0 };
            guide.span = (at(front_beyond), at(guide.sections.len().saturating_sub(1 + back_beyond)));
            guides.push(Some(guide));
        }
        // Intersection curves inherit their leader's piecewise parameter speed. Fitting
        // those parameters creates oscillations even on a geometrically smooth spine.
        // Resample by distance along the rolling centres, projecting each interpolated
        // centre back onto the exact two offset surfaces before it becomes a section.
        let uniform_section = |k: usize, along: f64| -> Result<Section> {
            let guide = guides.get(k).and_then(Option::as_ref).ok_or_else(|| KernelError::Failed("spine guide".into()))?;
            if along == 0.0 {
                return guide.sections.first().copied().ok_or_else(|| KernelError::Failed("section".into()));
            }
            if along == 1.0 {
                return guide.sections.last().copied().ok_or_else(|| KernelError::Failed("section".into()));
            }
            let (anchor, tangent, law_s) = guide.anchor_at(along).ok_or_else(|| unsupported("degenerate rolling-ball spine"))?;
            let (a1, a2) = edge_sides(&[k]);
            sizer
                .section(&a1, &a2, anchor, tangent, law_s, sigma, cusp, None)
                .map(|(section, _)| section)
                .ok_or_else(|| unsupported("the ball doesn't fit along the resampled spine"))
        };
        let esec: Vec<Vec<Section>> = (0..n)
            .map(|k| match guides.get(k) {
                Some(Some(_)) => (0..samples).map(|j| uniform_section(k, j as f64 / (samples - 1) as f64)).collect::<Result<Vec<_>>>(),
                _ => Ok(vec![
                    *vsec.get(k).ok_or_else(|| KernelError::Failed("section".into()))?,
                    *vsec.get((k + 1) % nv).ok_or_else(|| KernelError::Failed("section".into()))?,
                ]),
            })
            .collect::<Result<_>>()?;
        // Each strip's neighbouring sections at its own spacing (edges of different lengths
        // have differently spaced samples, which makes a uniform interpolation overshoot).
        let length = |k: usize| guides.get(k).and_then(Option::as_ref).and_then(|g| g.lengths.last().copied()).unwrap_or(0.0);
        let rolls = |k: usize| guides.get(k).is_some_and(Option::is_some);
        let mut fits: Vec<Option<Fit>> = Vec::with_capacity(n);
        for k in 0..n {
            // A pivot's surface is made whole (`revolve`); a strip next to one ends on the joint's
            // section on its own.
            if !rolls(k) {
                fits.push(None);
                continue;
            }
            let h = length(k) / (samples - 1) as f64;
            // A neighbour shorter than two of the strip's steps lends none: its own sections are
            // spaced far closer (a small round beside a long line), and borrowing them made the
            // strip overshoot and fold back on itself past the joint, which its faces' meshes
            // then folded with (#1667). The strip ends on the joint's section on its own.
            let spaced = |nb: usize, from_end: bool| -> Result<Vec<Section>> {
                let l = length(nb);
                if !(l > 2.0 * h && h > 0.0) {
                    return Ok(Vec::new());
                }
                let mut out = Vec::with_capacity(2);
                for j in [1.0, 2.0] {
                    out.push(uniform_section(nb, if from_end { 1.0 - j * h / l } else { j * h / l })?);
                }
                if from_end {
                    out.reverse();
                }
                Ok(out)
            };
            let (prev, next) = ((k + n - 1) % n, (k + 1) % n);
            let list = esec.get(k).ok_or_else(|| KernelError::Failed("section".into()))?;
            // Not across a joint where the strip's curvature jumps (a line or a wide arc into a
            // small round): a spline smooth through it spreads the jump over the sections beside
            // it, and the error falls only with the square of their spacing. The strip then ends
            // on the joint's section on its own, which the next one shares.
            let smooth = |near: [Option<&Section>; 3], beyond: [Option<&Section>; 3]| match (near, beyond) {
                ([Some(a), Some(b), Some(c)], [Some(x), Some(y), Some(z)]) => !bends([a, b, c], [x, y, z], fit_tolerance),
                _ => false,
            };
            let before = match (closed || k > 0) && rolls(prev) {
                true => {
                    let b = spaced(prev, true)?;
                    if smooth([list.first(), list.get(1), list.get(2)], [list.first(), b.get(1), b.first()]) { b } else { Vec::new() }
                }
                false => Vec::new(),
            };
            let after = match (closed || k + 1 < n) && rolls(next) {
                true => {
                    let a = spaced(next, false)?;
                    let m = list.len();
                    let tail = [list.last(), m.checked_sub(2).and_then(|i| list.get(i)), m.checked_sub(3).and_then(|i| list.get(i))];
                    if smooth(tail, [list.last(), a.first(), a.get(1)]) { a } else { Vec::new() }
                }
                false => Vec::new(),
            };
            let mut fit = crate::guard("fit curve blend", || fit_strip(list, &before, &after, round))?;
            fit.span = guides.get(k).and_then(Option::as_ref).map(|g| g.span).unwrap_or((0.0, 1.0));
            fits.push(Some(fit));
        }
        let mut accurate = true;
        'edges: for k in 0..n {
            let Some(Some(fit)) = fits.get(k) else { continue };
            for j in 0..samples - 1 {
                for fraction in [0.25, 0.5, 0.75] {
                    let along = (j as f64 + fraction) / (samples - 1) as f64;
                    // A size that varies along the edge reaches past its ends only for the fit's
                    // context (the end face cuts that away), so it is not held to the tolerance
                    // there. One radius keeps the check it always had, the whole strip.
                    if !matches!(size_law, Size::Constant(_))
                        && fits.get(k).and_then(Option::as_ref).is_some_and(|f| along < f.span.0 || along > f.span.1)
                    {
                        continue;
                    }
                    let section = uniform_section(k, along)?;
                    if !crate::guard("check curve blend fit", || Ok(section_fits(fit, &section, along, round, sigma, fit_tolerance)))? {
                        accurate = false;
                        break 'edges;
                    }
                }
            }
        }
        if accurate {
            break (esec, fits);
        }
        if samples >= max_samples {
            return Err(KernelError::Failed("curve blend: could not fit the rolling-ball surface within the accuracy and section limits".into()));
        }
        samples = (2 * samples - 1).min(max_samples);
    };
    // Where each extended blend meets its slanted end face: (vertex, edge, cut parameters of
    // the two contacts, the end curve from contact 1 to contact 2).
    let mut caps: Vec<(usize, usize, f64, f64, mt::Curve)> = Vec::new();
    // Ends across two end faces: (vertex, the seam between them, where the blend crosses it, and
    // a round's end on each face, contact 1 to the crossing and on to contact 2; a chamfer's
    // ends are lines).
    let mut split_caps: Vec<(usize, mt::Edge, Vec3, Option<RoundEnds>)> = Vec::new();
    for (k, fi, _, _, tangent) in &slanted {
        let ke = if *k == 0 { 0 } else { n - 1 };
        let fit = fits.get(ke).and_then(Option::as_ref).ok_or_else(|| KernelError::Failed("curve blend fit".into()))?;
        let g = faces.get(*fi).ok_or_else(|| KernelError::Failed("end face".into()))?;
        let at = vsec.get(*k).copied().ok_or_else(|| KernelError::Failed("section".into()))?;
        let seam = |s: u8| seams.get(*k).and_then(|l| l.iter().find(|(x, _)| *x as u8 == s)).map(|(_, e)| e);
        let both = [seam(1), seam(2)];
        if let Some((_, ga, gb, middle)) = split_ends.iter().find(|e| e.0 == *k) {
            use mt::{BSplineCurve, KnotVec, ParametricCurve, SearchNearestParameter};
            let fail = || {
                unsupported(if round {
                    "the round does not cross the edge between the end faces"
                } else {
                    "the chamfer does not cross both end faces"
                })
            };
            let old = verts.get(*k).ok_or_else(fail)?;
            let p = from_p3(old.point());
            let (_, t) = tangent_at(chain.get(ke).ok_or_else(fail)?, if *k == 0 { 0.0 } else { 1.0 }).ok_or_else(fail)?;
            let far = if middle.front() == old { middle.back() } else { middle.front() };
            if round {
                use mt::BoundedCurve;
                let w = from_p3(far.point()) - p;
                let (fa, fb) = (faces.get(*ga).ok_or_else(fail)?, faces.get(*gb).ok_or_else(fail)?);
                let end = crate::guard("curve blend split end", || split_round_cap(fit, (fa, fb), &at, (p, t, w), both, fit_tolerance))?;
                // The round crosses the seam short of its far end (two bevels at the end of a rim),
                // or its line past the vertex, which the seam then runs on to: a round adding
                // material at a wall's foot, or one taking it away along a plate's top edge that
                // runs into a wall standing on the plate (the wall's face runs on down to it).
                let crosses = end.fraction < 1.0 - 1e-6 && end.fraction.abs() > 1e-6;
                // The round's end, and the seam run on past the vertex, are new edges of the end
                // faces: they keep clear of the faces' other edges (a hole beside the end, the
                // face's far side), sampled finely between their own ends.
                let mut runs: Vec<Vec<Vec3>> = [&end.on_a, &end.on_b]
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        let (lo, hi) = c.range_tuple();
                        (0..=128).map(|i| from_p3(c.subs(lo + (hi - lo) * i as f64 / 128.0))).collect()
                    })
                    .collect();
                if end.fraction < 0.0 {
                    runs.push((0..=128).map(|i| p + (end.crossing - p) * (i as f64 / 128.0)).collect());
                }
                let clear = runs.iter().all(|run| {
                    let gap = run.windows(2).map(|x| x.first().zip(x.get(1)).map_or(0.0, |(a, b)| a.dist(*b))).fold(fit_tolerance * 10.0, f64::max);
                    let inner = run.get(2..run.len().saturating_sub(2)).unwrap_or(&[]);
                    [fa, fb].iter().all(|g| {
                        g.edge_iter()
                            .filter(|e| e.front() != old && e.back() != old)
                            .all(|e| inner.iter().all(|q| crate::blend::on_edge(&e, *q, gap).is_none()))
                    })
                });
                if !crosses || !clear {
                    return Err(too_large("the blend is larger than the neighbouring faces (split round end)"));
                }
                let first = end.on_a.as_ref().map_or(end.crossing, |c| from_p3(c.front()));
                let last = end.on_b.as_ref().map_or(end.crossing, |c| from_p3(c.back()));
                // The end's own curve where it lies on one face; across both only its ends are
                // used (the blend and the end faces take the two curves, `split_arcs`).
                let curve = match (&end.on_a, &end.on_b) {
                    (Some(c), None) | (None, Some(c)) => c.clone(),
                    _ => mt::Curve::BSplineCurve(
                        BSplineCurve::try_new(KnotVec::from(vec![0.0, 0.0, 0.5, 1.0, 1.0]), vec![p3(first), p3(end.crossing), p3(last)])
                            .map_err(|e| KernelError::Failed(format!("round end: {e}")))?,
                    ),
                };
                caps.push((*k, ke, end.t1, end.t2, curve));
                split_caps.push((*k, middle.clone(), end.crossing, Some((end.on_a, end.on_b))));
                continue;
            }
            let hit = |q: Vec3, fi: usize| -> Option<Vec3> {
                let normal = faces.get(fi).and_then(crate::blend::plane_normal)?;
                let den = normal.dot(t);
                (den.abs() > 1e-6).then(|| q - t * (normal.dot(q - p) / den))
            };
            let a = hit(at.q1, *ga).ok_or_else(fail)?;
            let b = hit(at.q2, *gb).ok_or_else(fail)?;
            let m = (at.q2 - at.q1).cross(t).normalized().ok_or_else(fail)?;
            let u = from_p3(far.point()) - p;
            let den = m.dot(u);
            if den.abs() < tol {
                return Err(fail());
            }
            let fraction = m.dot(at.q1 - p) / den;
            let c = p + u * fraction;
            let across = (c - at.q1).dot(at.q2 - at.q1) / (at.q2 - at.q1).len2();
            if !(fraction > 1e-6 && fraction < 1.0 - 1e-6 && across > 1e-6 && across < 1.0 - 1e-6) {
                return Err(fail());
            }
            let parameter = |row: usize, q: Vec3| -> Option<f64> {
                let curve = fit.contacts.get(row)?;
                let v = curve.search_nearest_parameter(p3(q), None, 100)?;
                (v > fit.range.0 + 1e-6 && v < fit.range.1 - 1e-6 && from_p3(curve.subs(v)).dist(q) <= fit_tolerance).then_some(v)
            };
            let (ta, tb) = (parameter(0, a).ok_or_else(fail)?, parameter(1, b).ok_or_else(fail)?);
            let curve = BSplineCurve::try_new(KnotVec::from(vec![0.0, 0.0, 0.5, 1.0, 1.0]), vec![p3(a), p3(c), p3(b)])
                .map_err(|e| KernelError::Failed(format!("chamfer end: {e}")))?;
            caps.push((*k, ke, ta, tb, mt::Curve::BSplineCurve(curve)));
            split_caps.push((*k, middle.clone(), c, None));
            continue;
        }
        // A merge with an earlier round of the other kind ends on the band's edge `Γ_X`.
        if let Some(m) = merges.iter().find(|m| m.k == *k) {
            let cap = crate::guard("curve blend merge end", || merge_cap(fit, m, fit_tolerance))?;
            caps.push((*k, ke, cap.0, cap.1, cap.2));
            continue;
        }
        let cap = crate::guard("curve blend end", || match tangent {
            Some(side) => tangent_cap(fit, g, *side, both, fit_tolerance),
            None => end_cap(fit, g, &at, both, round, fit_tolerance),
        })?;
        caps.push((*k, ke, cap.0, cap.1, cap.2));
    }
    // Both ends of one edge cut back: the contacts must keep a piece between them.
    if let [(_, 0, s1, s2, _), (_, 0, e1, e2, _)] = &caps[..]
        && n == 1
        && !(s1 + 1e-6 < *e1 && s2 + 1e-6 < *e2)
    {
        return Err(too_large("the blend is larger than the neighbouring faces (site 1668)"));
    }
    crate::guard("curve blend", || {
        let fail = |m: &str| KernelError::Failed(format!("curve blend: {m}"));
        // Vertices and arcs at the chain's vertices.
        let mut v1: Vec<mt::Vertex> = vsec.iter().map(|s| builder::vertex(p3(s.q1))).collect();
        let mut v2: Vec<mt::Vertex> = vsec.iter().map(|s| builder::vertex(p3(s.q2))).collect();
        for (k, _, _, _, curve) in &caps {
            use mt::{BoundedCurve, ParametricCurve};
            let (lo, hi) = curve.range_tuple();
            if let (Some(a), Some(b)) = (v1.get_mut(*k), v2.get_mut(*k)) {
                *a = builder::vertex(curve.subs(lo));
                *b = builder::vertex(curve.subs(hi));
            }
        }
        // A closed section (a radius that has run to zero) has one vertex for both contacts, so
        // the blend face's wire and the side faces' meet there.
        for (k, s) in vsec.iter().enumerate() {
            if !closed_section(s) {
                continue;
            }
            if let Some(v) = v1.get(k).cloned()
                && let Some(slot) = v2.get_mut(k)
            {
                *slot = v;
            }
        }
        // Along a pivot the planar side's contact is one point: one vertex at both its ends.
        for (k, pv) in pivots.iter().enumerate() {
            let Some(pv) = pv else { continue };
            let vs = if pv.side == 1 { &mut v1 } else { &mut v2 };
            if let Some(v) = vs.get(k).cloned()
                && let Some(slot) = vs.get_mut((k + 1) % nv)
            {
                *slot = v;
            }
        }
        // A merge's band, on `P*`, the later round's `q_X` (both new) and the earlier round's `u`:
        // the strip's contacts end at `P*` and `q_X`, and its end is the band's edge `Γ_X`.
        let mut bands: HashMap<usize, crate::merge_corner::Band> = HashMap::new();
        for m in &merges {
            let p_star = builder::vertex(p3(m.zone.p_star));
            let q_x = builder::vertex(p3(m.q_x()));
            let (q_w, q_s) = if m.later_adds() { (&q_x, &m.end.u) } else { (&m.end.u, &q_x) };
            let band = m.zone.band(&p_star, q_w, q_s, fit_tolerance)?;
            let (on_top, on_other) = if m.top == 1 { (&mut v1, &mut v2) } else { (&mut v2, &mut v1) };
            if let (Some(a), Some(b)) = (on_top.get_mut(m.k), on_other.get_mut(m.k)) {
                *a = p_star;
                *b = q_x;
            }
            bands.insert(m.k, band);
        }
        // The earlier rounds' ends at merges are left out (the band's edges `Γ_g` take their place).
        let merged: Vec<mt::EdgeID> = merges.iter().map(|m| m.end.cap.id()).collect();
        // A chain end (a cap, or the section at a square end) on the far end of its seam uses that
        // seam up: that vertex is the end.
        let mut consumed: HashMap<mt::EdgeID, mt::Vertex> = HashMap::new();
        for (k, _) in &end_faces {
            if bands.contains_key(k) {
                continue;
            }
            for (side, seam) in seams.get(*k).into_iter().flatten() {
                let Some(old) = verts.get(*k) else { continue };
                let far = if seam.front() == old { seam.back() } else { seam.front() };
                let slot = if *side == 1 { v1.get_mut(*k) } else { v2.get_mut(*k) };
                if let Some(nv) = slot
                    && far != old
                    && from_p3(far.point()).dist(from_p3(nv.point())) <= fit_tolerance
                {
                    *nv = far.clone();
                    consumed.insert(seam.id(), far.clone());
                }
            }
        }
        // The section's own radius, where the blend is sized by a law or a chord width (`r` is the
        // one radius of a constant blend, exactly as before).
        let rad = |s: &Section| match size_law {
            Size::Constant(r) => r,
            Size::Chord(_) | Size::Law(_) => s.c.dist(s.q1),
        };
        let mut arcs: Vec<Option<mt::Edge>> = Vec::with_capacity(nv);
        // The pieces of an end across two end faces, and the vertex where they meet the seam.
        let mut split_arcs: HashMap<usize, (Vec<mt::Edge>, mt::Vertex)> = HashMap::new();
        for (k, s) in vsec.iter().enumerate() {
            let (a, b) = (v1.get(k).ok_or_else(|| fail("vertex"))?, v2.get(k).ok_or_else(|| fail("vertex"))?);
            // A closed section has no arc to run between the contacts: the blend meets itself
            // there, at the edge's end point.
            let e = if closed_section(s) {
                None
            } else if round {
                let m = s.c + ((s.q1 - s.c) + (s.q2 - s.c)).normalized().ok_or_else(|| fail("arc"))? * rad(s);
                Some(builder::circle_arc(a, b, p3(m)))
            } else {
                Some(builder::line(a, b))
            };
            let merge = merges.iter().find(|m| m.k == k).zip(bands.get(&k));
            let e = match (merge, caps.iter().find(|c| c.0 == k)) {
                // The band's edge with the later round, from `P*` to `q_X`: from contact 1 to 2.
                (Some((m, band)), _) => {
                    let gamma = if m.later_adds() { &band.gamma_a } else { &band.gamma_b };
                    Some(if m.top == 1 { gamma.clone() } else { gamma.inverse() })
                }
                (None, Some((_, _, _, _, curve))) => Some(mt::Edge::try_new(a, b, curve.clone()).map_err(|e| fail(&e.to_string()))?),
                (None, None) => e,
            };
            match split_caps.iter().find(|cap| cap.0 == k) {
                Some((_, _, c, None)) => {
                    let v = builder::vertex(p3(*c));
                    split_arcs.insert(k, (vec![builder::line(a, &v), builder::line(&v, b)], v));
                }
                // A round's end crossing the seam at a contact meets it at that contact's vertex.
                Some((_, _, c, Some((on_a, on_b)))) => {
                    let v = match (on_a, on_b) {
                        (None, _) => a.clone(),
                        (_, None) => b.clone(),
                        _ => builder::vertex(p3(*c)),
                    };
                    let mut parts = Vec::with_capacity(2);
                    if let Some(curve) = on_a {
                        parts.push(mt::Edge::try_new(a, &v, curve.clone()).map_err(|e| fail(&e.to_string()))?);
                    }
                    if let Some(curve) = on_b {
                        parts.push(mt::Edge::try_new(&v, b, curve.clone()).map_err(|e| fail(&e.to_string()))?);
                    }
                    split_arcs.insert(k, (parts, v));
                }
                None => {}
            }
            arcs.push(e);
        }
        // Outward: toward the ball centre on a concave edge, away on a convex one.
        let oriented = |mut surface: mt::Surface, list: &[Section]| -> Result<mt::Surface> {
            use mt::{ParametricSurface3D, SearchNearestParameter};
            let s = list.get(list.len() / 2).ok_or_else(|| fail("section"))?;
            let mid = s.c + ((s.q1 - s.c) + (s.q2 - s.c)).normalized().ok_or_else(|| fail("mid"))? * rad(s);
            if let Some((u, v)) = surface.search_nearest_parameter(p3(mid), None, 100) {
                let nn = surface.normal(u, v);
                let want = (s.c - mid) * sigma;
                if Vec3::new(nn.x, nn.y, nn.z).dot(want) < 0.0 {
                    surface = mt::Invertible::inverse(&surface);
                }
            }
            Ok(surface)
        };
        // Contact curves and blend faces per edge.
        // (None: the planar side's contact along a pivot, a point.)
        let mut c1: Vec<Option<mt::Edge>> = Vec::with_capacity(n);
        let mut c2: Vec<Option<mt::Edge>> = Vec::with_capacity(n);
        let mut blends: Vec<mt::Surface> = Vec::with_capacity(n);
        for (k, list) in esec.iter().enumerate() {
            let k1 = (k + 1) % nv;
            let (a1, b1, a2, b2) = (v1.get(k), v1.get(k1), v2.get(k), v2.get(k1));
            let (Some(a1), Some(b1), Some(a2), Some(b2)) = (a1, b1, a2, b2) else { return Err(fail("vertex")) };
            if let Some(pv) = pivots.get(k).copied().flatten() {
                // The section at the start turned about the axis to the one at the end; the other
                // side's contact is the circle its end runs on.
                let (s0, s1e) = (list.first().ok_or_else(|| fail("section"))?, list.last().ok_or_else(|| fail("section"))?);
                let (qa, qb) = if pv.side == 1 { (s0.q2, s1e.q2) } else { (s0.q1, s1e.q1) };
                let (a, b) = (qa - pv.c, qb - pv.c);
                let angle = pv.axis.dot(a.cross(b)).atan2(a.dot(b));
                let (sn, cs) = (angle / 2.0).sin_cos();
                let mid = pv.c + a * cs + pv.axis.cross(a) * sn + pv.axis * (pv.axis.dot(a) * (1.0 - cs));
                let surface = section_controls(s0, round).and_then(|ctrl| revolve(ctrl, pv.c, pv.axis, angle)).ok_or_else(|| fail("pivot"))?;
                let (from, to) = if pv.side == 1 { (a2, b2) } else { (a1, b1) };
                let arc = builder::circle_arc(from, to, p3(mid));
                let (e1, e2) = if pv.side == 1 { (None, Some(arc)) } else { (Some(arc), None) };
                c1.push(e1);
                c2.push(e2);
                blends.push(oriented(surface, list)?);
                continue;
            }
            let fit = fits.get(k).and_then(Option::as_ref).ok_or_else(|| fail("fit"))?;
            let [cv1, cv2] = &fit.contacts;
            // Contacts past a slanted end stop at the end curve.
            let (mut cv1, mut cv2) = (cv1.clone(), cv2.clone());
            for (kv, _, t1, t2, _) in caps.iter().filter(|c| c.1 == k) {
                use mt::Cut;
                if *kv == k + 1 {
                    let _ = cv1.cut(*t1);
                    let _ = cv2.cut(*t2);
                }
            }
            for (kv, _, t1, t2, _) in caps.iter().filter(|c| c.1 == k) {
                use mt::Cut;
                if *kv == k && k == 0 {
                    cv1 = cv1.cut(*t1);
                    cv2 = cv2.cut(*t2);
                }
            }
            c1.push(Some(mt::Edge::new(a1, b1, cv1)));
            c2.push(Some(mt::Edge::new(a2, b2, cv2)));
            blends.push(oriented(fit.surface.clone(), list)?);
        }
        // Rebuild the side faces: chain edges become contact curves, seams start at contacts.
        let mut subst: HashMap<mt::EdgeID, (Option<mt::Edge>, Option<mt::Edge>)> = HashMap::new();
        for (k, e) in chain.iter().enumerate() {
            let (Some(x), Some(y)) = (c1.get(k), c2.get(k)) else { return Err(fail("contact")) };
            subst.insert(e.id(), (x.clone(), y.clone()));
        }
        // Seams get new ends at the contacts (an edge between two chain vertices at both).
        let mut ends: HashMap<mt::EdgeID, (mt::Edge, Option<mt::Vertex>, Option<mt::Vertex>)> = HashMap::new();
        for (k, sm) in seams.iter().enumerate().flat_map(|(k, l)| l.iter().map(move |x| (k, x))) {
            let (side, seam) = sm;
            let v = verts.get(k).cloned().ok_or_else(|| fail("vertex"))?;
            let nv = if *side == 1 { v1.get(k) } else { v2.get(k) }.ok_or_else(|| fail("vertex"))?.clone();
            let slot = ends.entry(seam.id()).or_insert_with(|| (seam.clone(), None, None));
            if *seam.front() == v {
                slot.1 = Some(nv);
            } else {
                slot.2 = Some(nv);
            }
        }
        let mut seam_subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        for (k, seam, _, _) in &split_caps {
            let old = verts.get(*k).ok_or_else(|| fail("end vertex"))?;
            let v = split_arcs.get(k).map(|(_, v)| v).ok_or_else(|| fail("end seam"))?;
            seam_subst.insert(seam.id(), crate::blend::retarget(seam, old, v, fit_tolerance)?);
        }
        // At a merge: the earlier round's contact with `T` shortened to `P*`, and `E`'s edge with the
        // face beside the chain to `q_X` (the band's edge on `L` takes the rest).
        for m in &merges {
            let v = verts.get(m.k).ok_or_else(|| fail("vertex"))?;
            let top = if m.top == 1 { v1.get(m.k) } else { v2.get(m.k) }.ok_or_else(|| fail("vertex"))?;
            let other = if m.top == 1 { v2.get(m.k) } else { v1.get(m.k) }.ok_or_else(|| fail("vertex"))?;
            seam_subst.insert(m.end.contact.id(), crate::blend::retarget(&m.end.contact, v, top, fit_tolerance)?);
            seam_subst.insert(m.end.end_side.id(), crate::blend::retarget(&m.end.end_side, &m.end.u, other, fit_tolerance)?);
        }
        for (id, (seam, a, b)) in ends {
            if consumed.contains_key(&id) || merges.iter().any(|m| m.end.contact.id() == id || m.end.cap.id() == id) {
                continue;
            }
            // At an end of an open chain the blend may stop past the seam's old end (a slanted end
            // face, or one turning back past the side, as a box side beside a boss joined to it):
            // the seam then runs on along its line or circle, within its planar face.
            let one_end = match (&a, &b) {
                (Some(nv), None) => Some((seam.front().clone(), nv.clone())),
                (None, Some(nv)) => Some((seam.back().clone(), nv.clone())),
                _ => None,
            };
            if let Some((old, nv)) = one_end
                && end_faces.iter().any(|(k, _)| verts.get(*k) == Some(&old))
            {
                let on_tol = (size * 1e-5).max(1e-6);
                let q = from_p3(nv.point());
                if crate::blend::on_edge(&seam, q, on_tol).is_none() {
                    let fits = faces
                        .iter()
                        .filter(|f| f.edge_iter().any(|e| e.id() == id))
                        .any(|f| crate::blend::plane_normal(f).is_some() && crate::blend::extension_fits(f, from_p3(old.point()), q, tol));
                    if !fits || !crate::blend::on_extension(&seam, q, on_tol) {
                        return Err(too_large("the blend is larger than the neighbouring faces (site 1853)"));
                    }
                }
                seam_subst.insert(id, crate::blend::retarget(&seam, &old, &nv, on_tol)?);
                continue;
            }
            if !matches!(seam.curve(), mt::Curve::Line(_)) {
                seam_subst.insert(id, trim_curved_seam(&seam, a, b, size)?);
                continue;
            }
            // The new ends must stay on the seam, in order, short of each other.
            let (p0, p1) = (from_p3(seam.front().point()), from_p3(seam.back().point()));
            let at = |v: &mt::Vertex| {
                let q = from_p3(v.point());
                let d = p1 - p0;
                ((q - p0).dot(d) / d.len2().max(1e-300), (q - p0 - d * ((q - p0).dot(d) / d.len2().max(1e-300))).len())
            };
            let (ta, oa) = a.as_ref().map(at).unwrap_or((0.0, 0.0));
            let (tb, ob) = b.as_ref().map(at).unwrap_or((1.0, 0.0));
            if oa > size * 1e-5 || ob > size * 1e-5 || ta < -1e-9 || tb > 1.0 + 1e-9 || tb - ta < 1e-6 {
                return Err(too_large("the blend is larger than the neighbouring faces (site 1873)"));
            }
            let a = a.unwrap_or_else(|| seam.front().clone());
            let b = b.unwrap_or_else(|| seam.back().clone());
            seam_subst.insert(id, builder::line(&a, &b));
        }
        let mut out: Vec<mt::Face> = Vec::new();
        let mut rec = Rebuild::default();
        rec.pieces(&seam_subst);
        for (fi, f) in faces.iter().enumerate() {
            let corners: Vec<usize> = end_faces.iter().filter(|(_, g)| *g == fi).map(|(k, _)| *k).collect();
            // An earlier round at a merge: its contact with `T` runs to `P*`, its end on the face
            // beside the chain gives way to `Γ_g` from `P*` to `u`. The end face `E`: its edge on
            // `L` with that face stops at `q_X`, the band's edge on `L` running on to `u`.
            let earlier: Vec<&MergeEnd> = merges.iter().filter(|m| m.g == fi).collect();
            let ended: Vec<&MergeEnd> = merges.iter().filter(|m| m.end.end == f.id()).collect();
            if !earlier.is_empty() || !ended.is_empty() {
                if corners.iter().any(|k| !bands.contains_key(k)) || (!earlier.is_empty() && !ended.is_empty()) {
                    return Err(unsupported("an earlier round that ends at a merged corner and elsewhere on the edge"));
                }
                let connectors: Vec<mt::Edge> = earlier
                    .iter()
                    .filter_map(|m| bands.get(&m.k).map(|b| if m.later_adds() { b.gamma_b.clone() } else { b.gamma_a.clone() }))
                    .chain(ended.iter().filter_map(|m| bands.get(&m.k).map(|b| b.on_l.clone())))
                    .collect();
                let rebuilt = crate::blend::rebuild_face_dropping(f, &seam_subst, &merged, &connectors)?;
                rec.face(&rebuilt, f);
                out.push(rebuilt);
                continue;
            }
            if !corners.is_empty() {
                if corners.iter().any(|k| split_arcs.contains_key(k)) {
                    let connectors: Vec<mt::Edge> = corners
                        .iter()
                        .flat_map(|k| split_arcs.get(k).map(|x| x.0.clone()).unwrap_or_else(|| arcs.get(*k).cloned().flatten().into_iter().collect()))
                        .collect();
                    // A seam the round used up (its contact at the seam's far end) is left out.
                    let used_up: Vec<mt::EdgeID> = consumed.keys().copied().collect();
                    let rebuilt = crate::blend::rebuild_face_dropping(f, &seam_subst, &used_up, &connectors)?;
                    rec.face(&rebuilt, f);
                    out.push(rebuilt);
                    continue;
                }
                // The end face (one or both ends of the chain): its two edges at a chain end now
                // stop at the contacts, with the blend's end arc between them.
                let mut wires = Vec::new();
                for w in f.absolute_boundaries() {
                    let mut es: Vec<mt::Edge> = Vec::new();
                    for e in w.edge_iter() {
                        let ne = match seam_subst.get(&e.id()) {
                            Some(s) => {
                                if e.front() == e.absolute_front() {
                                    s.clone()
                                } else {
                                    s.inverse()
                                }
                            }
                            None => e.clone(),
                        };
                        // Arriving at an old corner: continue along that end's arc. A seam the
                        // blend used up is left out, and the arc starts at its far end.
                        let arrives = corners.iter().copied().find(|k| verts.get(*k).is_some_and(|v| *e.back() == *v));
                        let used_up = consumed.contains_key(&e.id());
                        if !used_up {
                            es.push(ne.clone());
                        }
                        if let Some(k) = arrives {
                            // A closed section has no blend arc at the corner: the blend ends at
                            // the vertex itself and the end face keeps the corner it had.
                            let (Some(arc), Some(a1), Some(a2)) = (arcs.get(k).and_then(Option::as_ref), v1.get(k), v2.get(k)) else {
                                continue;
                            };
                            let to = if used_up { e.front() } else { ne.back() };
                            let corner = if to == a1 {
                                arc.clone()
                            } else if to == a2 {
                                arc.inverse()
                            } else {
                                return Err(fail("end face corner"));
                            };
                            es.push(corner);
                        }
                    }
                    wires.push(mt::Wire::from(es));
                }
                let rebuilt = crate::heal::absolute_face(f, wires).ok_or_else(|| fail("an end face could not be rebuilt"))?;
                rec.face(&rebuilt, f);
                out.push(rebuilt);
                continue;
            }
            let on1 = side1.contains(&fi);
            let on2 = side2.contains(&fi);
            if !on1 && !on2 && !f.edge_iter().any(|e| seam_subst.contains_key(&e.id())) {
                out.push(f.clone());
                continue;
            }
            let mut wires = Vec::new();
            for w in f.absolute_boundaries() {
                let mut es = Vec::new();
                for e in w.edge_iter() {
                    let ne = if let Some((x, y)) = subst.get(&e.id()) {
                        // A point along a pivot: the contacts beside it meet there.
                        let Some(base) = (if on1 { x } else { y }) else { continue };
                        // Same direction as the chain edge, or reversed.
                        let chain_e = chain.iter().find(|c| c.id() == e.id()).ok_or_else(|| fail("edge"))?;
                        if e.front() == chain_e.front() { base.clone() } else { base.inverse() }
                    } else if let Some(s) = seam_subst.get(&e.id()) {
                        if e.front() == e.absolute_front() { s.clone() } else { s.inverse() }
                    } else if consumed.contains_key(&e.id()) || merged.contains(&e.id()) {
                        continue;
                    } else {
                        e.clone()
                    };
                    es.push(ne);
                }
                if collapsed_boundary(&es, fit_tolerance) {
                    return Err(too_large("the blend would consume a neighbouring face"));
                }
                wires.push(mt::Wire::from(es));
            }
            let rebuilt = crate::heal::absolute_face(f, wires).ok_or_else(|| fail("a side face could not be rebuilt"))?;
            rec.face(&rebuilt, f);
            out.push(rebuilt);
        }
        // Blend faces: each runs its side-1 contact the other way from side 1's face.
        for (k, bf) in blends.iter().enumerate() {
            let (Some(e1), Some(e2)) = (c1.get(k), c2.get(k)) else {
                return Err(fail("blend"));
            };
            let ak = arcs.get(k).and_then(Option::as_ref);
            let ak1 = arcs.get((k + 1) % nv).and_then(Option::as_ref);
            let f1i = side1.get(k).ok_or_else(|| fail("side"))?;
            let f1 = faces.get(*f1i).ok_or_else(|| fail("side"))?;
            let chain_e = chain.get(k).ok_or_else(|| fail("edge"))?;
            let used_fwd = f1
                .boundary_iters()
                .into_iter()
                .flatten()
                .find(|e| e.id() == chain_e.id())
                .map(|e| e.front() == chain_e.front())
                .ok_or_else(|| fail("edge use"))?;
            // Side 1 runs the contact forward (as the chain) when it ran the edge forward.
            // A pivot's planar contact is a point: three sides. A closed section has no arc: two.
            let edges: Vec<mt::Edge> = if used_fwd {
                [e1.as_ref().map(|e| e.inverse()), ak.cloned(), e2.clone(), ak1.map(|e| e.inverse())]
            } else {
                [e1.clone(), ak1.cloned(), e2.as_ref().map(|e| e.inverse()), ak.map(|e| e.inverse())]
            }
            .into_iter()
            .flatten()
            .collect();
            // An arc the chamfer crossed is two lines through the crossing: the blend meets the
            // shortened seam there.
            let wire: mt::Wire = edges
                .into_iter()
                .flat_map(|e| {
                    let split = [(k, ak), ((k + 1) % nv, ak1)]
                        .into_iter()
                        .filter_map(|(i, arc)| arc.map(|arc| (i, arc.clone())))
                        .find(|(_, arc)| arc.id() == e.id())
                        .and_then(|(i, arc)| split_arcs.get(&i).map(|parts| (arc, &parts.0)));
                    match split {
                        Some((arc, parts)) if e.orientation() == arc.orientation() => parts.clone(),
                        Some((_, parts)) => parts.iter().rev().map(|p| p.inverse()).collect(),
                        None => vec![e],
                    }
                })
                .collect();
            let face = mt::Face::try_new(vec![wire], bf.clone()).map_err(|e| fail(&e.to_string()))?;
            rec.blend(&face, [chain_e.id()]);
            out.push(face);
        }
        // The bands at merges, made by this blend in place of its chain's end edge and the
        // earlier round's end, the edges meeting at the corner.
        for m in &merges {
            let (Some(band), Some(chain_e)) = (bands.get(&m.k), chain.get(m.ke)) else { return Err(fail("merge band")) };
            rec.blend(&band.face, [chain_e.id(), m.end.cap.id()]);
            out.push(band.face.clone());
        }
        let mut shells = solid.boundaries().clone();
        if let Some(sh) = shells.get_mut(si) {
            *sh = out.into();
        }
        Ok((Solid::try_new(shells).map_err(|e| fail(&format!("invalid solid: {e}")))?, rec))
    })
}

#[path = "curvecorner.rs"]
mod corner;
pub(crate) use corner::{corner_blend_style, param_on};
// The planar corner builder the G2 corner work left in place of `corner_blend_style`'s sphere.
#[allow(unused_imports)]
pub(crate) use corner::corner_blend;

#[cfg(test)]
#[path = "curveblend_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "curveblend_law_tests.rs"]
mod law_tests;
