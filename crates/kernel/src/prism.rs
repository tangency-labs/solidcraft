//! Booleans of planar regions in 2D (an arrangement of their lines, arcs and free-form
//! curves), the extrusion of planar material from them, and the test of whether a record
//! surface is a wall swept along a direction.

use std::borrow::Cow;

use solidcraft_geom::brep::Surface;
use solidcraft_geom::{Aabb2, Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::body::Body;
use crate::{BoolOp, KernelError, Result};

const MAX_PLANAR_SEGMENTS: usize = 8192;
const MAX_PLANAR_EVENTS: usize = 131_072;
const MAX_PLANAR_PAIRS: usize = 16_000_000;

/// Free-form boundary segments (conics, Béziers) take part in planar booleans as lines and
/// arcs within this distance (mm); loops that come out whole get their exact segments back.
pub const PLANAR_FIT_TOL: f64 = 1e-3;

/// Boolean of planar regions bounded by straight lines, circular arcs and free-form segments.
/// Lines and arcs stay analytic. Free-form loops are arranged as tangent arcs within
/// [`PLANAR_FIT_TOL`]: a loop the operation leaves whole comes back exact, one it cuts keeps
/// the arcs. Disjoint results and retained islands are returned as separate regions.
pub fn planar_regions_boolean(a: &[Region2], b: &[Region2], op: BoolOp, tolerance: f64) -> Result<Vec<Region2>> {
    crate::guard("planar region boolean", || {
        if !tolerance.is_finite() || !(1e-9..=1.0).contains(&tolerance) {
            return Err(KernelError::Invalid("planar boolean tolerance must be finite and between 1e-9 and 1 mm".into()));
        }
        let mut exact: Vec<(Loop2, Loop2)> = Vec::new();
        let mut pieces = 0usize;
        let (a, b) = (fit_free_form(a, &mut exact, &mut pieces)?, fit_free_form(b, &mut exact, &mut pieces)?);
        let (a, b): (&[Region2], &[Region2]) = (&a, &b);
        let loops = || a.iter().chain(b).flat_map(|r| std::iter::once(&r.outer).chain(&r.holes));
        let mut edges = 0usize;
        let mut samples = 0usize;
        for lp in loops() {
            edges = edges.saturating_add(lp.segs.len());
            if edges > MAX_PLANAR_SEGMENTS || lp.segs.is_empty() || !lp.is_closed(tolerance * 10.0) {
                return Err(KernelError::Invalid("planar boolean needs closed loops within its geometry budget".into()));
            }
            for s in &lp.segs {
                let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9;
                let valid = match *s {
                    Seg2::Line { a, b } => finite(a) && finite(b) && a.dist(b) > 1e-12,
                    Seg2::Arc { center, radius, start, sweep } => {
                        finite(center)
                            && radius.is_finite()
                            && radius > 0.0
                            && radius < 1e9
                            && start.is_finite()
                            && sweep.is_finite()
                            && sweep.abs() > 1e-12
                            && sweep.abs() <= std::f64::consts::TAU + 1e-9
                    }
                    _ => false,
                };
                if !valid {
                    return Err(KernelError::Invalid("planar boolean supports finite nondegenerate lines and circular arcs".into()));
                }
                samples = samples.saturating_add(s.polyline(1e-3).len());
                if samples > 65_536 {
                    return Err(KernelError::Invalid("planar boolean sampled geometry exceeds its budget".into()));
                }
            }
        }
        let out = boolean2d(a, b, op, tolerance)
            .ok_or_else(|| KernelError::Failed("planar region arrangement failed or exceeded its bounded processing budget".into()))?;
        Ok(restore_exact_loops(out, &exact, tolerance))
    })
}

/// Regions with their free-form loops fitted by lines and arcs; `exact` records each fitted
/// loop with the loop it stands for, `pieces` counts the fitted segments. Regions of lines and
/// arcs alone are passed on as they are.
fn fit_free_form<'a>(regions: &'a [Region2], exact: &mut Vec<(Loop2, Loop2)>, pieces: &mut usize) -> Result<Cow<'a, [Region2]>> {
    if regions.iter().all(Region2::is_line_arc) {
        return Ok(Cow::Borrowed(regions));
    }
    let mut fit = |lp: &Loop2| -> Result<Loop2> {
        if lp.is_line_arc() {
            return Ok(lp.clone());
        }
        let fitted = lp.fit_arcs(PLANAR_FIT_TOL).filter(|f| {
            *pieces = pieces.saturating_add(f.segs.len());
            *pieces <= MAX_PLANAR_SEGMENTS
        });
        let fitted = fitted.ok_or_else(|| {
            KernelError::Invalid("planar boolean: a free-form boundary is not finite or takes more arcs than the geometry budget".into())
        })?;
        exact.push((fitted.clone(), lp.clone()));
        Ok(fitted)
    };
    let fitted: Result<Vec<Region2>> =
        regions.iter().map(|r| Ok(Region2 { outer: fit(&r.outer)?, holes: r.holes.iter().map(&mut fit).collect::<Result<_>>()? })).collect();
    fitted.map(Cow::Owned)
}

/// Output loops that are a fitted free-form loop, whole (from any start, either way round), get
/// the exact loop back, running the same way.
fn restore_exact_loops(regions: Vec<Region2>, exact: &[(Loop2, Loop2)], tol: f64) -> Vec<Region2> {
    if exact.is_empty() {
        return regions;
    }
    let near = |p: Vec2, q: Vec2| p.dist(q) <= tol * 10.0;
    let same_run = |l: &Loop2, f: &Loop2| -> bool {
        let n = f.segs.len();
        let Some(first) = l.segs.first() else { return false };
        (0..n).any(|k| {
            f.segs.get(k).is_some_and(|s| near(s.start(), first.start()))
                && l.segs.iter().enumerate().all(|(i, s)| {
                    f.segs.get((k + i) % n).is_some_and(|g| near(g.start(), s.start()) && near(g.end(), s.end()) && near(g.mid(), s.mid()))
                })
        })
    };
    let restore = |l: Loop2| -> Loop2 {
        for (fitted, original) in exact {
            if fitted.segs.len() != l.segs.len() {
                continue;
            }
            if same_run(&l, fitted) {
                return original.clone();
            }
            if same_run(&l, &fitted.reversed()) {
                return original.reversed();
            }
        }
        l
    };
    regions.into_iter().map(|r| Region2 { outer: restore(r.outer), holes: r.holes.into_iter().map(&restore).collect() }).collect()
}

/// Extrude normalized planar material as one logical compound, retaining every disjoint
/// region and its holes. Planar arrangement certifies separation before shells are joined.
pub fn extrude_planar_regions(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Body> {
    crate::guard("planar material extrusion", || {
        // Validate aggregate inputs first; an arrangement compares opposite operands,
        // so regions in the same operand must be normalized through balanced unions.
        planar_regions_boolean(regions, &[], BoolOp::Union, 1e-7)?;
        let mut groups: Vec<Vec<Region2>> = regions.iter().cloned().map(|r| vec![r]).collect();
        let mut remaining_pairs = 32_000_000usize;
        while groups.len() > 1 {
            let mut next = Vec::new();
            let mut iter = groups.into_iter();
            while let Some(a) = iter.next() {
                if let Some(b) = iter.next() {
                    let count = |rs: &[Region2]| {
                        rs.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).fold(0usize, |n, l| n.saturating_add(l.segs.len()))
                    };
                    remaining_pairs = remaining_pairs
                        .checked_sub(count(&a).saturating_mul(count(&b)))
                        .ok_or_else(|| KernelError::Invalid("planar extrusion arrangement exceeds its work budget".into()))?;
                    next.push(planar_regions_boolean(&a, &b, BoolOp::Union, 1e-7)?);
                } else {
                    next.push(a);
                }
            }
            groups = next;
        }
        let normalized = groups.pop().unwrap_or_default();
        if normalized.is_empty() {
            return Err(KernelError::Invalid("empty planar material extrusion".into()));
        }
        // The separation test knows lines and arcs: free-form loops take part as their arcs.
        let fitted = fit_free_form(&normalized, &mut Vec::new(), &mut 0)?;
        for (i, a) in fitted.iter().enumerate() {
            for b in fitted.iter().skip(i + 1) {
                for s in std::iter::once(&a.outer).chain(&a.holes).flat_map(|l| &l.segs) {
                    for t in std::iter::once(&b.outer).chain(&b.holes).flat_map(|l| &l.segs) {
                        remaining_pairs = remaining_pairs
                            .checked_sub(1)
                            .ok_or_else(|| KernelError::Invalid("planar extrusion separation exceeds its work budget".into()))?;
                        let endpoints_touch = [s.start(), s.end()].iter().any(|p| t.point_at(closest_param(t, *p)).dist(*p) <= 1e-7)
                            || [t.start(), t.end()].iter().any(|p| s.point_at(closest_param(s, *p)).dist(*p) <= 1e-7);
                        if endpoints_touch || !crossings(s, t, 1e-7).is_empty() {
                            return Err(KernelError::Invalid("planar material pieces touch at a boundary".into()));
                        }
                    }
                }
            }
        }
        let pieces = crate::extrude(plane, &normalized, start, end)?;
        crate::ops::join_pieces(pieces)?.ok_or_else(|| KernelError::Failed("empty planar material extrusion".into()))
    })
}

/// What a section's segment is, through [`boolean2d_tagged`]: the free-form segment (by
/// number) the arc it is was fitted to, so the result's pieces of it get the exact curve back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tag {
    free: Option<usize>,
}

/// A segment that is no fitted arc.
const BLANK: Tag = Tag { free: None };

/// Regions with a tag for each segment: per region, per loop (outer, then holes), per segment.
#[derive(Clone, Debug, Default, PartialEq)]
struct Section {
    regions: Vec<Region2>,
    tags: Vec<Vec<Vec<Tag>>>,
}

impl Section {
    /// Regions none of whose segments is a fitted arc.
    fn untagged(regions: &[Region2]) -> Section {
        let tags = regions.iter().map(|r| std::iter::once(&r.outer).chain(&r.holes).map(|l| vec![BLANK; l.segs.len()]).collect()).collect();
        Section { regions: regions.to_vec(), tags }
    }

    /// The tag of segment `s` of loop `l` of region `r` (blank where missing).
    fn tag(&self, r: usize, l: usize, s: usize) -> Tag {
        self.tags.get(r).and_then(|t| t.get(l)).and_then(|t| t.get(s)).copied().unwrap_or(BLANK)
    }

    /// The tags of region `r`'s loops, as long as its loops (blank where missing).
    fn region_tags(&self, r: usize) -> Vec<Vec<Tag>> {
        self.regions
            .get(r)
            .map(|reg| {
                std::iter::once(&reg.outer)
                    .chain(&reg.holes)
                    .enumerate()
                    .map(|(l, lp)| (0..lp.segs.len()).map(|s| self.tag(r, l, s)).collect())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every segment in order (regions, then loops) with its tag.
    fn segments(&self) -> (Vec<Seg2>, Vec<Tag>) {
        let mut out = (Vec::new(), Vec::new());
        for (r, reg) in self.regions.iter().enumerate() {
            for (l, lp) in std::iter::once(&reg.outer).chain(&reg.holes).enumerate() {
                for (s, seg) in lp.segs.iter().enumerate() {
                    out.0.push(*seg);
                    out.1.push(self.tag(r, l, s));
                }
            }
        }
        out
    }

    /// Loops oriented: outers counter-clockwise, holes clockwise; the tags following their
    /// segments.
    fn oriented(&self) -> Section {
        let turn = |lp: &Loop2, tags: Vec<Tag>, reverse: bool| -> (Loop2, Vec<Tag>) {
            if reverse { (lp.reversed(), tags.into_iter().rev().collect()) } else { (lp.clone(), tags) }
        };
        let mut out = Section::default();
        for (r, reg) in self.regions.iter().enumerate() {
            let mut tags = self.region_tags(r).into_iter();
            let (outer, outer_tags) = turn(&reg.outer, tags.next().unwrap_or_default(), reg.outer.signed_area() < 0.0);
            let mut loop_tags = vec![outer_tags];
            let mut holes = Vec::new();
            for h in &reg.holes {
                let (hole, hole_tags) = turn(h, tags.next().unwrap_or_default(), h.signed_area() > 0.0);
                holes.push(hole);
                loop_tags.push(hole_tags);
            }
            out.regions.push(Region2 { outer, holes });
            out.tags.push(loop_tags);
        }
        out
    }
}

/// Certify that a curved wall is swept along `d` (unit) from its surface record: a cylinder or
/// a revolved line about an axis along `d`, or a spline net whose rows run along `d` (either
/// parameter) with weights that separate into one factor per parameter. Seam vertices alone
/// cannot establish this: a half-cylinder's seam normals can both be perpendicular to a
/// transverse direction, silently filling its hole. A plane is no curved wall.
pub(crate) fn wall_along(surface: &Surface, d: Vec3, tol: f64, remaining: &mut usize) -> bool {
    let v3 = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let along = |a: Vec3| a.normalized().is_some_and(|a| a.cross(d).len() < 1e-9);
    let points: Vec<Vec<(Vec3, f64)>> = match surface {
        Surface::Cylinder { frame, .. } => return along(v3(frame.x_axis).cross(v3(frame.y_axis))),
        Surface::PlacedRevolution { profile, axis, matrix, .. } => {
            let solidcraft_geom::brep::Curve::Line { start, end } = &**profile else { return false };
            let (Some(a), Some(m)) = (v3(*axis).normalized(), (v3(*end) - v3(*start)).normalized()) else { return false };
            if a.cross(m).len() > 1e-9 {
                return false; // A conical meridian is not a cylindrical wall.
            }
            // The axis placed by the column-major matrix (a direction: no translation).
            let c = |k: usize| matrix[0][k] * a.x + matrix[1][k] * a.y + matrix[2][k] * a.z;
            return c(3).abs() < 1e-12 && along(Vec3::new(c(0), c(1), c(2)));
        }
        Surface::Spline { points, .. } => points.iter().map(|r| r.iter().map(|p| (v3(*p), 1.0)).collect()).collect(),
        Surface::RationalSpline { homogeneous, .. } => {
            homogeneous.iter().map(|r| r.iter().map(|p| (Vec3::new(p[0] / p[3], p[1] / p[3], p[2] / p[3]), p[3])).collect()).collect()
        }
        _ => return false,
    };
    let (rows, cols) = (points.len(), points.first().map_or(0, Vec::len));
    let count = rows.saturating_mul(cols);
    if rows < 2 || cols < 2 || count > 16_384 || points.iter().any(|r| r.len() != cols) {
        return false;
    }
    let Some(left) = remaining.checked_sub(count.saturating_mul(2)) else { return false };
    *remaining = left;
    if points.iter().flatten().any(|(p, w)| !w.is_finite() || *w <= 0.0 || !p.is_finite()) {
        return false;
    }
    let max_weight = points.iter().flatten().map(|p| p.1).fold(0.0, f64::max);
    if points.iter().flatten().any(|p| p.1 / max_weight < 1e-12) {
        return false;
    }
    // Either parameter can be the extrusion direction. Projected positions must
    // be invariant in that parameter; rational weights must also separate into
    // a product of one factor per parameter, otherwise mixed weights bend the wall.
    for transpose in [false, true] {
        let (groups, length) = if transpose { (cols, rows) } else { (rows, cols) };
        let at = |i: usize, j: usize| if transpose { points[j][i] } else { points[i][j] };
        let mut valid = true;
        let mut extended = false;
        for i in 0..groups {
            let (base, base_w) = at(i, 0);
            for j in 0..length {
                let (p, w) = at(i, j);
                let delta = p - base;
                let magnitude = [p.x.abs(), p.y.abs(), p.z.abs(), base.x.abs(), base.y.abs(), base.z.abs()].into_iter().fold(1.0, f64::max);
                if delta.cross(d).len() > tol * 1e-3 + magnitude * f64::EPSILON * 128.0 {
                    valid = false;
                    break;
                }
                extended |= delta.dot(d).abs() > tol * 1e-3;
                let lhs = w / max_weight * (at(0, 0).1 / max_weight);
                let rhs = base_w / max_weight * (at(0, j).1 / max_weight);
                if (lhs - rhs).abs() > 1e-12 * lhs.abs().max(rhs.abs()) {
                    valid = false;
                    break;
                }
            }
            if !valid {
                break;
            }
        }
        if valid && extended {
            return true;
        }
    }
    false
}

/// Points where two segments cross (parameters on each), lines and arcs only.
///
/// Curves that touch within the tolerance (a line tangent to an arc, two circles tangent inside
/// or outside) meet at one point, taken from the centres: the foot of the centre on the line,
/// or the point on the line of centres. The intersection formulas' two roots there are rounding
/// noise spread along the curves by its square root (a 1e-12 mm error in a distance puts them
/// ±2e-6 mm from a 2 mm arc's tangent point), close enough to both curves to pass for two
/// crossings; when the noise falls the other way there is no root and the contact is missed.
fn crossings(s: &Seg2, t: &Seg2, tol: f64) -> Vec<(f64, f64)> {
    let mut pts: Vec<Vec2> = Vec::new();
    match (*s, *t) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => {
            let (r, q) = (b - a, d - c);
            let den = r.cross(q);
            // Lines lying on one another within the tolerance do not cross: their ends split
            // them. A hair from parallel they still meet at a point, anywhere along the overlap,
            // and a split there leaves a piece shorter than the tolerance or a vertex that
            // neither outline has.
            let off = |p: Vec2, o: Vec2, dir: Vec2| (p - o).cross(dir).abs() / dir.len().max(1e-300);
            let collinear = off(c, a, r) < tol && off(d, a, r) < tol || off(a, c, q) < tol && off(b, c, q) < tol;
            if den.abs() > 1e-300 && !collinear {
                let u = (c - a).cross(q) / den;
                pts.push(a + r * u);
            }
        }
        (Seg2::Line { a, b }, Seg2::Arc { center, radius, .. }) | (Seg2::Arc { center, radius, .. }, Seg2::Line { a, b }) => {
            let dir = b - a;
            let f = a - center;
            let (qa, qb, qc) = (dir.dot(dir), 2.0 * f.dot(dir), f.dot(f) - radius * radius);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa > 1e-300 {
                let foot = a + dir * (-qb / (2.0 * qa));
                if (foot.dist(center) - radius).abs() < tol {
                    pts.push(foot);
                } else if disc >= -tol * tol * qa {
                    let sd = disc.max(0.0).sqrt();
                    for u in [(-qb - sd) / (2.0 * qa), (-qb + sd) / (2.0 * qa)] {
                        pts.push(a + dir * u);
                    }
                }
            }
        }
        (Seg2::Arc { center: c0, radius: r0, .. }, Seg2::Arc { center: c1, radius: r1, .. }) => {
            let dd = c0.dist(c1);
            if dd > 1e-12 && dd <= r0 + r1 + tol && dd >= (r0 - r1).abs() - tol {
                // Circles of nearly one radius about nearly one centre lie on each other for a
                // long way rather than touch at a point (centres 1000 tolerances apart: within
                // the tolerance of each other over 5° of arc): those keep the general formula.
                let inside = (dd - (r0 - r1).abs()).abs() < tol && dd > 1e3 * tol;
                if (dd - (r0 + r1)).abs() < tol || inside {
                    pts.push(c0 + (c1 - c0) * (if inside && r1 > r0 { -r0 } else { r0 } / dd));
                } else {
                    let a = (r0 * r0 - r1 * r1 + dd * dd) / (2.0 * dd);
                    let h = (r0 * r0 - a * a).max(0.0).sqrt();
                    let m = c0 + (c1 - c0) * (a / dd);
                    let perp = (c1 - c0).perp() * (1.0 / dd);
                    pts.push(m + perp * h);
                    pts.push(m - perp * h);
                }
            }
        }
        _ => {}
    }
    let mut out = Vec::new();
    for p in pts {
        let (u, v) = (closest_param(s, p), closest_param(t, p));
        if s.point_at(u).dist(p) < tol && t.point_at(v).dist(p) < tol {
            out.push((u, v));
        }
    }
    out
}

/// Ray parity against analytic lines/arcs. Fragment classification must not depend on a
/// display chord tolerance: a tiny tool can sit between a circular boundary and its chords.
/// Loops are closed only within the chaining tolerance (an arrangement's own output joins its
/// pieces within `10 · tol`), so a gap where one line ends and the next begins is closed by a
/// straight connector: a ray through the gap would otherwise slip out of the loop without a
/// crossing and flip the parity of points level with the corner. Arcs decide their own ends
/// within an angular tolerance (below), so joints with an arc keep that rule.
fn inside_loop(lp: &Loop2, p: Vec2) -> bool {
    // A ray through a vertex or grazing an arc's top or bottom is decided by the half-open
    // rule, which needs both curves meeting there to agree on that point exactly. Arcs read
    // back from a solid (a patterned hole's two halves) can disagree by a few ulps, so one half
    // counts the touch and the other misses it. Move the ray off such points: a shift far below
    // any modelling tolerance cannot change the answer for a point that is not on the loop.
    let scale = 1.0 + p.x.abs().max(p.y.abs());
    // A point clear of the loop's box (by more than the ray shifts below) is outside it: skip
    // the costly tests, which a region with a thousand cut-outs runs for every probe (#732).
    if let Some(b) = loop_box(lp)
        && !b.inflated(scale * 1e-6).contains(p)
    {
        return false;
    }
    // A point on the loop keeps the half-open answer.
    if lp.segs.iter().any(|s| s.point_at(closest_param(s, p)).dist(p) < scale * 1e-9) {
        return inside_loop_ray(lp, p);
    }
    let grazes = |q: Vec2| {
        let near = |y: f64| (y - q.y).abs() < scale * 1e-9;
        lp.segs.iter().any(|s| {
            near(s.start().y)
                || match *s {
                    Seg2::Arc { center, radius, .. } => near(center.y + radius) || near(center.y - radius),
                    _ => false,
                }
        })
    };
    let ray = [0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0].into_iter().map(|k| Vec2::new(p.x, p.y + k * scale * 1e-7)).find(|q| !grazes(*q)).unwrap_or(p);
    inside_loop_ray(lp, ray)
}

/// A box holding a loop of lines and arcs (an arc's whole circle); None for other curves.
fn loop_box(lp: &Loop2) -> Option<Aabb2> {
    let mut b = Aabb2::EMPTY;
    for seg in &lp.segs {
        b = b.union(&match *seg {
            Seg2::Line { a, b } => Aabb2 { min: Vec2::new(a.x.min(b.x), a.y.min(b.y)), max: Vec2::new(a.x.max(b.x), a.y.max(b.y)) },
            Seg2::Arc { center, radius, .. } => Aabb2 { min: center, max: center }.inflated(radius.abs()),
            _ => return None,
        });
    }
    (!b.is_empty() && b.min.is_finite() && b.max.is_finite()).then_some(b)
}

/// [`inside_loop`] with the ray along +x from `p` taken as given.
fn inside_loop_ray(lp: &Loop2, p: Vec2) -> bool {
    let mut odd = false;
    let nexts = lp.segs.iter().skip(1).chain(lp.segs.first());
    for (seg, next) in lp.segs.iter().zip(nexts) {
        if let (Seg2::Line { b: a, .. }, Seg2::Line { a: b, .. }) = (*seg, *next)
            && (a.y > p.y) != (b.y > p.y)
            && a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) > p.x
        {
            odd = !odd;
        }
        match *seg {
            Seg2::Line { a, b } => {
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                    if x > p.x {
                        odd = !odd;
                    }
                }
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let y = (p.y - center.y) / radius;
                if !(-1.0..=1.0).contains(&y) {
                    continue;
                }
                let a = y.asin();
                let root = (1.0 - y * y).max(0.0).sqrt();
                let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-12;
                for (i, (angle, x)) in [(a, center.x + radius * root), (std::f64::consts::PI - a, center.x - radius * root)].into_iter().enumerate() {
                    // A tangent has one geometric root. Interior tangencies contribute zero
                    // parity; a minimum at an endpoint follows the same half-open rule as lines.
                    if root == 0.0 && i == 1 {
                        continue;
                    }
                    if x <= p.x {
                        continue;
                    }
                    let mut delta = if sweep > 0.0 { angle - start } else { start - angle };
                    delta = delta.rem_euclid(std::f64::consts::TAU);
                    if (delta - std::f64::consts::TAU).abs() < 1e-12 {
                        delta = 0.0;
                    }
                    if delta > sweep.abs() + 1e-12 {
                        continue;
                    }
                    let dy = angle.cos() * sweep;
                    let tangent = dy.abs() < sweep.abs() * 1e-12;
                    let at_start = delta < 1e-12;
                    let at_end = (delta - sweep.abs()).abs() < 1e-12;
                    let crosses = if full {
                        !tangent
                    } else if at_start {
                        dy > 0.0 && !tangent || tangent && y < 0.0
                    } else if at_end {
                        dy < 0.0 && !tangent || tangent && y < 0.0
                    } else {
                        !tangent
                    };
                    if crosses {
                        odd = !odd;
                    }
                }
            }
            _ => {
                // Private prism callers only supply lines/arcs. Keep a controlled fallback
                // for future internal callers; the public helper rejects other segment kinds.
                let points = seg.polyline(1e-7);
                for pair in points.windows(2) {
                    if let [a, b] = pair
                        && (a.y > p.y) != (b.y > p.y)
                        && a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) > p.x
                    {
                        odd = !odd;
                    }
                }
            }
        }
    }
    odd
}

fn inside(regions: &[Region2], p: Vec2) -> bool {
    regions.iter().any(|r| inside_loop(&r.outer, p) && !r.holes.iter().any(|h| inside_loop(h, p)))
}

/// Call only after proving the point is at least `tol` from the other boundary.
/// This sub-tolerance interior probe cannot cross that boundary, and avoids casting
/// exactly through fitted arc/line endpoints whose heights differ by roundoff.
fn inside_clear(regions: &[Region2], point: Vec2, tol: f64) -> bool {
    inside(regions, point + Vec2::new(tol * 0.173, tol * 0.317))
}

/// Analytic projections avoid iterative searches at every planar boundary probe.
/// Other curve types keep the shared solver introduced on main.
fn closest_param(segment: &Seg2, point: Vec2) -> f64 {
    match *segment {
        Seg2::Line { a, b } => {
            let direction = b - a;
            let length2 = direction.dot(direction);
            if length2 > 1e-300 && length2.is_finite() { ((point - a).dot(direction) / length2).clamp(0.0, 1.0) } else { 0.0 }
        }
        Seg2::Arc { center, radius, start, sweep } if radius > 0.0 && sweep.abs() > 1e-12 && sweep.abs() <= std::f64::consts::TAU + 1e-9 => {
            let offset = point - center;
            let delta = ((offset.y.atan2(offset.x) - start) * sweep.signum()).rem_euclid(std::f64::consts::TAU);
            if delta <= sweep.abs() {
                (delta / sweep.abs()).clamp(0.0, 1.0)
            } else if segment.start().dist(point) <= segment.end().dist(point) {
                0.0
            } else {
                1.0
            }
        }
        _ => segment.closest_param(point),
    }
}

/// The bounding box of a line or an arc, grown by twice the tolerance (and by the rounding of
/// its own corners). Whatever lies within the tolerance of the segment lies in the box. `None`
/// for other segments and for values that are not finite: no box, and nothing is skipped for it.
fn seg_box(s: &Seg2, tol: f64) -> Option<Aabb2> {
    let mut pts = vec![s.start(), s.end()];
    match *s {
        Seg2::Line { .. } => {}
        Seg2::Arc { center, radius, start, sweep } => {
            // The axis extremes the arc passes.
            for k in 0..4 {
                let ang = k as f64 * std::f64::consts::FRAC_PI_2;
                let delta =
                    if sweep >= 0.0 { (ang - start).rem_euclid(std::f64::consts::TAU) } else { (start - ang).rem_euclid(std::f64::consts::TAU) };
                if delta <= sweep.abs() {
                    pts.push(center + Vec2::from_angle(ang) * radius);
                }
            }
        }
        _ => return None,
    }
    let g = tol * 2.0 + 1e-12 * pts.iter().fold(0.0, |m: f64, p| m.max(p.x.abs()).max(p.y.abs()));
    (pts.iter().all(|p| p.is_finite()) && g.is_finite()).then(|| pts.iter().copied().collect::<Aabb2>().inflated(g))
}

fn boxes_meet(a: Option<Aabb2>, b: Option<Aabb2>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.meets(&b),
        _ => true,
    }
}

fn box_holds(b: Option<Aabb2>, p: Vec2) -> bool {
    b.is_none_or(|b| b.contains(p))
}

/// Distance from `p` to the nearest of `segs` whose box holds it: infinite when none does (no
/// segment is within the tolerance of `p` then).
fn near_dist(segs: &[Seg2], boxes: &[Option<Aabb2>], p: Vec2) -> f64 {
    segs.iter().zip(boxes).filter(|(_, b)| box_holds(**b, p)).map(|(s, _)| s.point_at(closest_param(s, p)).dist(p)).fold(f64::INFINITY, f64::min)
}

/// Direction of the first of `segs` through `p` (within `tol`), as it runs.
fn near_dir(segs: &[Seg2], boxes: &[Option<Aabb2>], p: Vec2, tol: f64) -> Option<Vec2> {
    let (s, t) = segs
        .iter()
        .zip(boxes)
        .filter(|(_, b)| box_holds(**b, p))
        .map(|(s, _)| (s, closest_param(s, p)))
        .find(|(s, t)| s.point_at(*t).dist(p) < tol)?;
    s.derivative(t).normalized()
}

/// 2D boolean of regions bounded by lines and arcs.
fn boolean2d(a: &[Region2], b: &[Region2], op: BoolOp, tol: f64) -> Option<Vec<Region2>> {
    boolean2d_tagged(&Section::untagged(a), &Section::untagged(b), op, tol).map(|s| s.regions)
}

/// [`boolean2d`] of sections. Free-form segments (a font's curves) take part as arcs within
/// [`PLANAR_FIT_TOL`]; each piece of one in the result is the exact curve again, ending where
/// it exactly meets the segment next to it (see [`exact_again`]).
fn boolean2d_tagged(a: &Section, b: &Section, op: BoolOp, tol: f64) -> Option<Section> {
    let line_arc = |s: &Section| s.regions.iter().all(Region2::is_line_arc);
    if line_arc(a) && line_arc(b) {
        return boolean2d_arcs(a, b, op, tol);
    }
    let mut sources = Vec::new();
    let (fa, fb) = (fitted(a, &mut sources, PLANAR_FIT_TOL)?, fitted(b, &mut sources, PLANAR_FIT_TOL)?);
    let out = boolean2d_arcs(&fa, &fb, op, tol)?;
    Some(exact_again(out, &sources, tol))
}

/// `s` with each free-form segment replaced by lines and arcs within `fit`, tagged with its
/// number in `sources` (where it is put).
fn fitted(s: &Section, sources: &mut Vec<Seg2>, fit: f64) -> Option<Section> {
    let mut out = Section::default();
    let mut pieces = 0usize;
    for (r, reg) in s.regions.iter().enumerate() {
        let mut loops = Vec::new();
        let mut tags = Vec::new();
        for (l, lp) in std::iter::once(&reg.outer).chain(&reg.holes).enumerate() {
            let (mut segs, mut ts) = (Vec::new(), Vec::new());
            for (i, seg) in lp.segs.iter().enumerate() {
                if seg.is_line_or_arc() {
                    segs.push(*seg);
                    ts.push(s.tag(r, l, i));
                    continue;
                }
                let arcs = seg.fit_arcs(fit)?;
                pieces = pieces.checked_add(arcs.len()).filter(|n| *n <= MAX_PLANAR_SEGMENTS)?;
                let n = arcs.len();
                ts.extend(std::iter::repeat_n(Tag { free: Some(sources.len()) }, n));
                segs.extend(arcs);
                sources.push(*seg);
            }
            loops.push(Loop2 { segs });
            tags.push(ts);
        }
        let mut loops = loops.into_iter();
        out.regions.push(Region2 { outer: loops.next()?, holes: loops.collect() });
        out.tags.push(tags);
    }
    Some(out)
}

/// A loop's piece in [`exact_again`]: a run of arcs fitted to free-form segment `src`, standing
/// for its exact curve from parameter `t0` to `t1` (`t1 < t0`: run backwards), from `a` to `b`;
/// or a line or arc as it is.
#[derive(Clone, Copy)]
enum Piece {
    Exact { src: usize, t0: f64, t1: f64, a: Vec2, b: Vec2, tag: Tag },
    Plain(Seg2, Tag),
}

impl Piece {
    fn end(&self) -> Vec2 {
        match self {
            Piece::Exact { b, .. } => *b,
            Piece::Plain(s, _) => s.end(),
        }
    }

    /// The piece ending at `at` (an exact curve: at its parameter `t`, when given).
    fn set_end(&mut self, t: Option<f64>, at: Vec2) {
        match self {
            Piece::Exact { t1, b, .. } => {
                if let Some(t) = t {
                    *t1 = t;
                }
                *b = at;
            }
            Piece::Plain(s, _) => *s = seg_with_ends(s, s.start(), at),
        }
    }

    /// The piece starting at `at` (an exact curve: at its parameter `t`, when given).
    fn set_start(&mut self, t: Option<f64>, at: Vec2) {
        match self {
            Piece::Exact { t0, a, .. } => {
                if let Some(t) = t {
                    *t0 = t;
                }
                *a = at;
            }
            Piece::Plain(s, _) => *s = seg_with_ends(s, at, s.end()),
        }
    }
}

/// `s` running from `a` to `b` instead (points on it or a hair off it): a line between them, an
/// arc of the same circle, a free-form segment with those end control points.
fn seg_with_ends(s: &Seg2, a: Vec2, b: Vec2) -> Seg2 {
    match *s {
        Seg2::Line { .. } => Seg2::Line { a, b },
        Seg2::Arc { center, radius, start, sweep } => {
            let new_start = (a - center).angle();
            let end = start + sweep;
            // The same way round, through the same part of the circle.
            let shift = |x: f64, near: f64| x + ((near - x) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
            let s0 = shift(new_start, start);
            let s1 = shift((b - center).angle(), end);
            Seg2::Arc { center, radius, start: s0, sweep: s1 - s0 }
        }
        Seg2::Cubic { p1, p2, .. } => Seg2::Cubic { p0: a, p1, p2, p3: b },
        Seg2::Conic { apex, w, .. } => Seg2::Conic { a, apex, b, w },
        Seg2::Bezier { n, mut p } => {
            if let Some(x) = p.first_mut() {
                *x = a;
            }
            if let Some(x) = p.get_mut(n as usize) {
                *x = b;
            }
            Seg2::Bezier { n, p }
        }
    }
}

/// The exact piece of `src` from parameter `t0` to `t1` (either way round), from `a` to `b`.
fn trimmed(src: &Seg2, t0: f64, t1: f64, a: Vec2, b: Vec2) -> Seg2 {
    let (lo, hi) = (t0.min(t1), t0.max(t1));
    let mut s = *src;
    if lo > 1e-12 {
        s = s.split_at(lo).1;
    }
    if hi < 1.0 - 1e-12 {
        // Splitting a conic renormalises its weights, which moves its parameters: the second
        // split is found on the piece.
        let at = closest_param(&s, src.point_at(hi));
        if at < 1.0 - 1e-12 {
            s = s.split_at(at).0;
        }
    }
    if t0 <= t1 { seg_with_ends(&s, a, b) } else { seg_with_ends(&s.reversed(), a, b) }
}

/// Where curve `p` near its parameter `t` meets `q` near its parameter `u`, by Newton's method:
/// their parameters there, or `None` when the crossing doesn't settle within `near` of `p(t)`.
fn meet(p: &Seg2, t: f64, q: &Seg2, u: f64, near: f64) -> Option<(f64, f64)> {
    let (mut t, mut u) = (t, u);
    let start = p.point_at(t);
    let scale = 1.0 + start.x.abs().max(start.y.abs());
    for _ in 0..50 {
        let f = p.point_at(t) - q.point_at(u);
        if f.len() < 1e-14 * scale {
            break;
        }
        // P(t) - Q(u) = f; the step solves [P'(t), -Q'(u)] (dt, du) = -f.
        let (dp, dq) = (p.derivative(t), q.derivative(u));
        let det = -dp.cross(dq);
        if !(det.abs() > 1e-300 && det.is_finite()) {
            return None;
        }
        t = (t + f.cross(dq) / det).clamp(0.0, 1.0);
        u = (u + f.cross(dp) / det).clamp(0.0, 1.0);
    }
    let (x, y) = (p.point_at(t), q.point_at(u));
    (x.dist(y) < 1e-9 * scale && x.dist(start) < near).then_some((t, u))
}

/// The whole line or circle a line or arc lies on (as a segment reaching past it), what an exact
/// crossing with a curve is looked for on.
fn carrier(s: &Seg2) -> Option<Seg2> {
    match *s {
        Seg2::Line { a, b } => Some(Seg2::Line { a: a - (b - a), b: b + (b - a) }),
        Seg2::Arc { center, radius, start, sweep } => {
            let turn = sweep.signum() * std::f64::consts::PI;
            Some(Seg2::Arc { center, radius, start: start + sweep * 0.5 - turn, sweep: 2.0 * turn })
        }
        _ => None,
    }
}

/// The exact curves back in the result of a boolean of [`fitted`] sections: each run of arcs
/// fitted to one free-form segment becomes that segment's exact piece, and where a run ends on
/// another segment (a cut across the curve) both end at their exact crossing. Where that
/// crossing can't be found they end where the arrangement put them, within
/// [`PLANAR_FIT_TOL`] of it.
fn exact_again(out: Section, sources: &[Seg2], tol: f64) -> Section {
    let near = (PLANAR_FIT_TOL + tol) * 10.0;
    let mut res = Section::default();
    for (r, reg) in out.regions.iter().enumerate() {
        let tags = out.region_tags(r);
        let mut loops = Vec::new();
        let mut loop_tags = Vec::new();
        for (l, lp) in std::iter::once(&reg.outer).chain(&reg.holes).enumerate() {
            let (segs, ts) = exact_loop(lp, tags.get(l).map(Vec::as_slice).unwrap_or(&[]), sources, near);
            // A sliver the arrangement found where a curve's arcs nearly touch another part of
            // the outline (at the waist of a 3) closes up to nothing on the exact curves: it
            // encloses nothing and would make edges with no length, so it is left out.
            if segs.iter().map(Seg2::length).sum::<f64>() <= near {
                if l == 0 {
                    break;
                }
                continue;
            }
            loops.push(Loop2 { segs });
            loop_tags.push(ts);
        }
        let mut loops = loops.into_iter();
        let Some(outer) = loops.next() else { continue };
        res.regions.push(Region2 { outer, holes: loops.collect() });
        res.tags.push(loop_tags);
    }
    res
}

/// One loop of [`exact_again`]: its segments and their tags.
fn exact_loop(lp: &Loop2, tags: &[Tag], sources: &[Seg2], near: f64) -> (Vec<Seg2>, Vec<Tag>) {
    let n = lp.segs.len();
    let tag = |_: usize| BLANK;
    let free = |i: usize| tags.get(i).and_then(|t| t.free);
    let plain = || (lp.segs.clone(), (0..n).map(tag).collect::<Vec<_>>());
    // Start at the first segment of a run (a loop that is one run stays as it is).
    let Some(first) = (0..n).find(|&i| free(i).is_none() || free(i) != free((i + n - 1) % n)) else { return plain() };
    let mut pieces: Vec<Piece> = Vec::new();
    let mut k = 0;
    while k < n {
        let i = (first + k) % n;
        let Some(seg) = lp.segs.get(i) else { return plain() };
        let Some((f, src)) = free(i).and_then(|f| sources.get(f).map(|s| (f, s))) else {
            pieces.push(Piece::Plain(*seg, tag(i)));
            k += 1;
            continue;
        };
        let mut j = k;
        while j + 1 < n && free((first + j + 1) % n) == Some(f) {
            j += 1;
        }
        let Some(last) = lp.segs.get((first + j) % n) else { return plain() };
        // A run reaching the curve's end is the curve to its end, exactly: the same curve the
        // walls over it are read from, which a parameter found by projection (a hair short of 1)
        // would cut short into a different one.
        let at = |p: Vec2| {
            if p.dist(src.start()) < near {
                0.0
            } else if p.dist(src.end()) < near {
                1.0
            } else {
                closest_param(src, p)
            }
        };
        let (t0, t1) = (at(seg.start()), at(last.end()));
        if (t1 - t0).abs() < 1e-9 {
            // A sliver of the curve: its arcs as they are, or, shorter than the tolerance (where
            // a curve lies on another, as when the same letters are cut again), nothing: its
            // neighbours meet where it was, and as an edge it would have no length.
            let length: f64 = (k..=j).filter_map(|m| lp.segs.get((first + m) % n)).map(Seg2::length).sum();
            if length > near {
                for m in k..=j {
                    let q = (first + m) % n;
                    pieces.push(Piece::Plain(*lp.segs.get(q).unwrap_or(seg), tag(q)));
                }
            }
        } else {
            pieces.push(Piece::Exact { src: f, t0, t1, a: src.point_at(t0), b: src.point_at(t1), tag: BLANK });
        }
        k = j + 1;
    }
    // Each joint at an exact curve: both sides end at the same point, the exact crossing where
    // the curve is cut, its own end point where it ends.
    if pieces.is_empty() {
        return plain();
    }
    let m = pieces.len();
    let inner = |t: f64| t > 1e-9 && t < 1.0 - 1e-9;
    for i in 0..m {
        let (Some(p), Some(q)) = (pieces.get(i).copied(), pieces.get((i + 1) % m).copied()) else { continue };
        let joint = p.end();
        let src = |f: usize| sources.get(f);
        let found = match (p, q) {
            (Piece::Exact { src: f, t1, .. }, Piece::Exact { src: g, t0, .. }) if f != g && (inner(t1) || inner(t0)) => {
                src(f).zip(src(g)).and_then(|(x, y)| meet(x, t1, y, t0, near).map(|(a, b)| (Some(a), Some(b), x.point_at(a))))
            }
            (Piece::Exact { src: f, t1, .. }, Piece::Plain(s, _)) if inner(t1) => src(f)
                .zip(carrier(&s))
                .and_then(|(x, c)| meet(x, t1, &c, closest_param(&c, joint), near).map(|(a, _)| (Some(a), None, x.point_at(a)))),
            (Piece::Plain(s, _), Piece::Exact { src: f, t0, .. }) if inner(t0) => src(f)
                .zip(carrier(&s))
                .and_then(|(x, c)| meet(x, t0, &c, closest_param(&c, joint), near).map(|(a, _)| (None, Some(a), x.point_at(a)))),
            _ => None,
        };
        // No crossing to refine (or none found): both end where the curve does.
        let found = found.or(match (p, q) {
            (Piece::Exact { b, .. }, _) => Some((None, None, b)),
            (_, Piece::Exact { a, .. }) => Some((None, None, a)),
            _ => None,
        });
        let Some((x, y, at)) = found else { continue };
        if let Some(p) = pieces.get_mut(i) {
            p.set_end(x, at);
        }
        if let Some(q) = pieces.get_mut((i + 1) % m) {
            q.set_start(y, at);
        }
    }
    let mut segs = Vec::with_capacity(m);
    let mut ts = Vec::with_capacity(m);
    for p in pieces {
        match p {
            Piece::Exact { src, t0, t1, a, b, tag } => match sources.get(src) {
                Some(s) => {
                    segs.push(trimmed(s, t0, t1, a, b));
                    ts.push(tag);
                }
                None => return plain(),
            },
            Piece::Plain(s, t) => {
                segs.push(s);
                ts.push(t);
            }
        }
    }
    // The loop closed exactly: where a segment ends a hair from where the next starts (a font's
    // nearly round curve read as an arc ends a few hundredths of a micron off the curve after
    // it), the one that is not an arc moves its end onto the arc's (an arc's ends stay on its
    // circle). Two arcs are left as they meet, as a boolean of lines and arcs leaves them.
    let m = segs.len();
    for i in 0..m {
        let j = (i + 1) % m;
        let (Some(x), Some(y)) = (segs.get(i).copied(), segs.get(j).copied()) else { continue };
        let gap = x.end().dist(y.start());
        if gap == 0.0 || gap > near {
            continue;
        }
        let arc = |s: &Seg2| matches!(s, Seg2::Arc { .. });
        if !arc(&y) {
            if let Some(s) = segs.get_mut(j) {
                *s = seg_with_ends(&y, x.end(), y.end());
            }
        } else if !arc(&x)
            && let Some(s) = segs.get_mut(i)
        {
            *s = seg_with_ends(&x, x.start(), y.start());
        }
    }
    (segs, ts)
}

/// The boolean of sections of lines and arcs alone ([`boolean2d_tagged`]).
fn boolean2d_arcs(a: &Section, b: &Section, op: BoolOp, tol: f64) -> Option<Section> {
    let count =
        |rs: &[Region2]| rs.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).fold(0usize, |n, lp| n.saturating_add(lp.segs.len()));
    let (na, nb) = (count(&a.regions), count(&b.regions));
    if na.saturating_add(nb) > MAX_PLANAR_SEGMENTS || na.saturating_mul(nb) > MAX_PLANAR_PAIRS {
        return None;
    }
    let (a, b) = (a.oriented(), b.oriented());
    let ((sa, ta), (sb, tb)) = (a.segments(), b.segments());
    let (a, b) = (a.regions, b.regions);
    if sa.iter().chain(&sb).any(|s| !matches!(s, Seg2::Line { .. } | Seg2::Arc { .. })) {
        return None;
    }
    // Only segments whose bounding boxes meet can cross, touch or lie on each other: the other
    // pairs skip the exact tests (a closest-point search each).
    let (box_a, box_b): (Vec<_>, Vec<_>) = (sa.iter().map(|s| seg_box(s, tol)).collect(), sb.iter().map(|s| seg_box(s, tol)).collect());
    let apart = |i: usize, j: usize| !boxes_meet(box_a.get(i).copied().flatten(), box_b.get(j).copied().flatten());
    // Split every segment where it crosses the other side's boundary.
    let mut cut_a: Vec<Vec<f64>> = vec![Vec::new(); sa.len()];
    let mut cut_b: Vec<Vec<f64>> = vec![Vec::new(); sb.len()];
    let mut events = 0usize;
    for (i, s) in sa.iter().enumerate() {
        for (j, t) in sb.iter().enumerate() {
            if apart(i, j) {
                continue;
            }
            for (u, v) in crossings(s, t, tol) {
                events += 2;
                if events > MAX_PLANAR_EVENTS {
                    return None;
                }
                cut_a.get_mut(i)?.push(u);
                cut_b.get_mut(j)?.push(v);
            }
        }
    }
    // Ends of the other side's segments lying on a segment split it too (touching corners).
    for (i, s) in sa.iter().enumerate() {
        for (j, t) in sb.iter().enumerate() {
            if apart(i, j) {
                continue;
            }
            for p in [t.start(), t.end()] {
                let u = closest_param(s, p);
                if s.point_at(u).dist(p) < tol {
                    events += 1;
                    if events > MAX_PLANAR_EVENTS {
                        return None;
                    }
                    cut_a.get_mut(i)?.push(u);
                }
            }
        }
    }
    for (j, t) in sb.iter().enumerate() {
        for (i, s) in sa.iter().enumerate() {
            if apart(i, j) {
                continue;
            }
            for p in [s.start(), s.end()] {
                let v = closest_param(t, p);
                if t.point_at(v).dist(p) < tol {
                    events += 1;
                    if events > MAX_PLANAR_EVENTS {
                        return None;
                    }
                    cut_b.get_mut(j)?.push(v);
                }
            }
        }
    }
    // The loops below are chained by joining pieces whose ends lie within `near` of each other,
    // so to them a piece shorter than that runs from a point back to the same point: a cut that
    // would leave one, at either end of what is left of the segment, is not made. Two edges
    // 0.57° apart that share a corner up to rounding cross a hundred times that rounding short
    // of it; split there, the sliver between the crossing and the corner was taken for a
    // second way out of the corner and closed as a loop of its own.
    let near = tol * 10.0;
    let pieces = |s: &Seg2, cuts: &mut Vec<f64>| -> Vec<Seg2> {
        cuts.sort_by(f64::total_cmp);
        let mut out = Vec::new();
        let mut rest = *s;
        let mut done = 0.0;
        for &c in cuts.iter() {
            if c <= done + 1e-9 || c >= 1.0 - 1e-9 {
                continue;
            }
            let local = (c - done) / (1.0 - done);
            let (x, y) = rest.split_at(local);
            if x.length() > near && y.length() > near {
                out.push(x);
                rest = y;
                done = c;
            }
        }
        out.push(rest);
        out
    };
    let mut kept: Vec<(Seg2, Tag)> = Vec::new();
    // A's pieces.
    for (i, s) in sa.iter().enumerate() {
        if cut_a.get(i)?.len() > MAX_PLANAR_SEGMENTS {
            return None;
        }
        let split = pieces(s, cut_a.get_mut(i)?);
        let tag = ta.get(i).copied().unwrap_or(BLANK);
        for p in split {
            let m = p.point_at(0.5);
            let on = near_dist(&sb, &box_b, m) < tol;
            let keep = if on {
                let same = near_dir(&sb, &box_b, m, tol).is_some_and(|d| d.dot(p.derivative(0.5)) > 0.0);
                match op {
                    BoolOp::Union | BoolOp::Intersect => same,
                    BoolOp::Cut => !same,
                }
            } else {
                let ins = inside_clear(&b, m, tol);
                match op {
                    BoolOp::Union | BoolOp::Cut => !ins,
                    BoolOp::Intersect => ins,
                }
            };
            if keep {
                if kept.len() >= MAX_PLANAR_SEGMENTS {
                    return None;
                }
                kept.push((p, tag));
            }
        }
    }
    // B's pieces (coincident ones were decided with A's).
    for (j, t) in sb.iter().enumerate() {
        if cut_b.get(j)?.len() > MAX_PLANAR_SEGMENTS {
            return None;
        }
        let split = pieces(t, cut_b.get_mut(j)?);
        let tag = tb.get(j).copied().unwrap_or(BLANK);
        for p in split {
            let m = p.point_at(0.5);
            if near_dist(&sa, &box_a, m) < tol {
                continue;
            }
            let ins = inside_clear(&a, m, tol);
            if kept.len() >= MAX_PLANAR_SEGMENTS {
                return None;
            }
            match op {
                BoolOp::Union if !ins => kept.push((p, tag)),
                BoolOp::Intersect if ins => kept.push((p, tag)),
                BoolOp::Cut if ins => kept.push((p.reversed(), tag)),
                _ => {}
            }
        }
    }
    // Chain into loops. Where several kept pieces leave the point a loop has reached (outlines
    // pinching at a vertex on an edge, a circle tangent to a side, two corners touching), the
    // loop takes the one keeping the material on its left most tightly, so that pieces of
    // material meeting only at that point close as loops of their own; returning to its start,
    // a loop closes only when that is the tightest turn too. A loop that still passes a point
    // twice (a hole touching the outline) is split there into simple loops.
    let mut loops: Vec<(Loop2, Vec<Tag>)> = Vec::new();
    let mut used = vec![false; kept.len()];
    for start in 0..kept.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let (seg, tag) = *kept.get(start)?;
        let (mut segs, mut tags) = (vec![seg], vec![tag]);
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let first = seg.start();
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > kept.len() + 2 {
                return None;
            }
            let last = *segs.last()?;
            let end = last.end();
            let full_circle =
                segs.first().is_some_and(|s| matches!(s, Seg2::Arc { sweep, .. } if (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9));
            let closes = end.dist(first) < near && (segs.len() > 1 || full_circle);
            let leaving: Vec<usize> =
                (0..kept.len()).filter(|k| !used.get(*k).copied().unwrap_or(true) && kept[*k].0.start().dist(end) < near).collect();
            let next = match leaving.as_slice() {
                [] if end.dist(first) < near => break,
                [] => return None,
                [only] if !closes => *only,
                _ => {
                    // The loop's own first piece stands for closing it.
                    let mut out: Vec<Seg2> = leaving.iter().map(|k| kept[*k].0).collect();
                    if closes {
                        out.push(*segs.first()?);
                    }
                    let pick = tightest_left(&last, &out)?;
                    match leaving.get(pick) {
                        Some(k) => *k,
                        None => break,
                    }
                }
            };
            let (seg, tag) = *kept.get(next)?;
            if let Some(u) = used.get_mut(next) {
                *u = true;
            }
            segs.push(seg);
            tags.push(tag);
        }
        for (l, t) in simple_loops(Loop2 { segs }, tags, near) {
            if l.signed_area().abs() > tol * tol {
                loops.push((l, t));
            }
        }
    }
    // Outers (counter-clockwise) take the holes they contain, the smallest first.
    let (mut outers, holes): (Vec<(Loop2, Vec<Tag>)>, Vec<(Loop2, Vec<Tag>)>) = loops.into_iter().partition(|(l, _)| l.signed_area() > 0.0);
    outers.sort_by(|x, y| x.0.signed_area().abs().total_cmp(&y.0.signed_area().abs()));
    let mut out = Section::default();
    for (o, tags) in outers {
        out.regions.push(Region2 { outer: o, holes: Vec::new() });
        out.tags.push(vec![tags]);
    }
    for (h, tags) in holes {
        let p = h.segs.first()?.point_at(0.5);
        let r = out.regions.iter().position(|r| inside_loop(&r.outer, p) && r.outer.signed_area().abs() > h.signed_area().abs())?;
        out.regions.get_mut(r)?.holes.push(h);
        out.tags.get_mut(r)?.push(tags);
    }
    Some(out)
}

/// Of the pieces in `out` leaving the point where `incoming` ends, the one turning most to the
/// left: the largest counter-clockwise angle from the way back along `incoming`. A loop with
/// its material on its left that always takes it encloses the least, so material meeting
/// another piece only at a point closes there as a loop of its own.
///
/// Directions are read a short way along each piece (the same distance on all, a twentieth of
/// the shortest), not from tangents: a circle tangent to a line leaves the point along the
/// line, and only its curvature says which side it turns to. The pieces of an arrangement
/// don't cross, and none turns more than a twentieth of its sweep within that distance, so the
/// order the points lie in is the order the pieces leave in.
fn tightest_left(incoming: &Seg2, out: &[Seg2]) -> Option<usize> {
    let shortest = out.iter().map(Seg2::length).fold(incoming.length(), f64::min);
    let h = shortest * 0.05;
    if !(h > 0.0) || !h.is_finite() {
        return None;
    }
    let at = incoming.end();
    let back = incoming.point_at(1.0 - h / incoming.length()) - at;
    let turn = |s: &Seg2| {
        let d = s.point_at(h / s.length()) - at;
        let a = back.cross(d).atan2(back.dot(d));
        if a < 0.0 { a + std::f64::consts::TAU } else { a }
    };
    (0..out.len()).max_by(|x, y| turn(&out[*x]).total_cmp(&turn(&out[*y])))
}

/// A closed loop split where it passes a point twice (within `near`) into loops that pass
/// every point once, with their segments' tags. Counter-clockwise pieces are material, the
/// others holes in it: a hole touching the outline at one point, or two pieces of material
/// meeting at a corner, come out as two loops sharing that point.
fn simple_loops(lp: Loop2, tags: Vec<Tag>, near: f64) -> Vec<(Loop2, Vec<Tag>)> {
    let mut todo = vec![(lp.segs, tags)];
    let mut out = Vec::new();
    // Each split leaves two shorter loops, so the work is bounded by the segment count.
    while let Some((segs, tags)) = todo.pop() {
        let n = segs.len();
        let repeat = (0..n).find_map(|i| {
            let p = segs.get(i)?.start();
            (i + 1..n).find(|j| segs.get(*j).is_some_and(|s| s.start().dist(p) < near)).map(|j| (i, j))
        });
        match repeat {
            Some((i, j)) if j > i => {
                let part = |v: &[Seg2], r: std::ops::Range<usize>| v.get(r).unwrap_or(&[]).to_vec();
                let part_t = |v: &[Tag], r: std::ops::Range<usize>| v.get(r).unwrap_or(&[]).to_vec();
                let inner = (part(&segs, i..j), part_t(&tags, i..j));
                let outer = ([part(&segs, j..n), part(&segs, 0..i)].concat(), [part_t(&tags, j..n), part_t(&tags, 0..i)].concat());
                todo.push(inner);
                todo.push(outer);
            }
            _ => out.push((Loop2 { segs }, tags)),
        }
    }
    out
}

#[cfg(test)]
mod planar_tests {
    use super::*;
    #[test]
    fn shared_arc_line_endpoint_roundoff_does_not_open_a_union() {
        let a = Region2 { outer: Loop2::polygon(&[Vec2::new(-2., 0.), Vec2::new(2., 0.), Vec2::new(2., 2.), Vec2::new(-2., 2.)]), holes: vec![] };
        let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
        for epsilon in [-4e-14, 4e-14] {
            let b = Region2 {
                outer: Loop2 {
                    segs: vec![
                        line((1.15, epsilon), (-1.15, -epsilon)),
                        Seg2::Arc { center: Vec2::new(-1.35, epsilon), radius: 0.2, start: 0., sweep: -std::f64::consts::FRAC_PI_2 },
                        line((-1.35, -0.2), (-2.4, -0.2)),
                        line((-2.4, -0.2), (-2.4, -1.)),
                        line((-2.4, -1.), (2.4, -1.)),
                        line((2.4, -1.), (2.4, -0.2)),
                        line((2.4, -0.2), (1.35, -0.2)),
                        Seg2::Arc {
                            center: Vec2::new(1.35, -epsilon),
                            radius: 0.2,
                            start: -std::f64::consts::FRAC_PI_2,
                            sweep: -std::f64::consts::FRAC_PI_2,
                        },
                    ],
                },
                holes: vec![],
            };
            let result = planar_regions_boolean(std::slice::from_ref(&a), std::slice::from_ref(&b), BoolOp::Union, 1e-5).unwrap();
            assert_eq!(result.len(), 1);
            assert!(result[0].outer.is_closed(1e-4));
            assert!((result[0].area() - a.area() - b.area()).abs() < 1e-6);
        }
    }
    #[test]
    fn analytic_projection_keeps_clamped_lines_and_wrapped_arcs() {
        let line = Seg2::Line { a: Vec2::new(2., 3.), b: Vec2::new(5., 7.) };
        for (point, expected) in [(Vec2::new(-1., -1.), 0.), (Vec2::new(3.5, 5.), 0.5), (Vec2::new(8., 11.), 1.)] {
            assert!((closest_param(&line, point) - expected).abs() < 1e-12);
        }
        for start in [-7., -0.5, 3., 7.] {
            for sweep in [-std::f64::consts::TAU, -5., -0.2, 0.2, 5., std::f64::consts::TAU] {
                let arc = Seg2::Arc { center: Vec2::new(2., -3.), radius: 4., start, sweep };
                for index in 0..128 {
                    let angle = index as f64 * std::f64::consts::TAU / 128.;
                    let point = Vec2::new(2., -3.) + Vec2::new(angle.cos(), angle.sin()) * 6.;
                    let nearest = arc.point_at(closest_param(&arc, point));
                    // Includes both signs, sweeps over pi, seam crossings and endpoint clamps.
                    for sample in 0..=128 {
                        assert!(nearest.dist(point) <= arc.point_at(sample as f64 / 128.).dist(point) + 1e-10);
                    }
                }
                for t in [0., 0.25, 0.5, 0.75, 1.] {
                    let point = arc.point_at(t);
                    assert!(arc.point_at(closest_param(&arc, point)).dist(point) < 1e-10);
                }
            }
        }
    }
    fn rectangle(a: Vec2, b: Vec2) -> Region2 {
        Region2 { outer: Loop2::polygon(&[a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]), holes: vec![] }
    }
    #[test]
    fn curved_prism_wall_requires_positive_separable_rational_weights() {
        let surface = |mixed: bool| {
            let weight = if mixed { 2.0 } else { 1.0 };
            Surface::RationalSpline {
                u_knots: vec![0., 0., 1., 1.],
                v_knots: vec![0., 0., 1., 1.],
                homogeneous: vec![
                    vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 10.0, 1.0]],
                    vec![[5.0, 0.0, 0.0, 1.0], [5.0 * weight, 0.0, 10.0 * weight, weight]],
                ],
            }
        };
        assert!(wall_along(&surface(false), Vec3::Z, 1e-5, &mut 1000));
        assert!(!wall_along(&surface(true), Vec3::Z, 1e-5, &mut 1000), "parallel Euclidean controls alone do not certify a rational sweep");
        assert!(!wall_along(&surface(false), Vec3::Y, 1e-5, &mut 1000));
        assert!(!wall_along(&surface(false), Vec3::Z, 1e-5, &mut 0));
    }
    #[test]
    fn compound_planar_extrusion_normalizes_overlap_and_retains_islands() {
        let plane = Plane::named("XY").unwrap();
        let a = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = rectangle(Vec2::new(5.0, 0.0), Vec2::new(15.0, 10.0));
        let body = extrude_planar_regions(&plane, &[a.clone(), b], 0.0, 2.0).unwrap();
        assert!((crate::measure(&body).unwrap().volume - 300.0).abs() < 1e-6);
        let inner = Region2 { outer: Loop2::circle(Vec2::new(5.0, 5.0), 2.0), holes: vec![] };
        let body = extrude_planar_regions(&plane, &[a.clone(), inner.clone()], 0.0, 2.0).unwrap();
        assert!((crate::measure(&body).unwrap().volume - 200.0).abs() < 1e-6);
        let annulus = Region2 { outer: a.outer, holes: vec![Loop2::circle(Vec2::new(5.0, 5.0), 3.0)] };
        let body = extrude_planar_regions(&plane, &[annulus, inner], 0.0, 2.0).unwrap();
        let expected = (100.0 - std::f64::consts::PI * 5.0) * 2.0;
        assert!((crate::measure(&body).unwrap().volume - expected).abs() < 0.1);
        assert_eq!(body.shell_count(), 2);
        let a = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = rectangle(Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0));
        assert!(extrude_planar_regions(&plane, &[a, b], 0.0, 2.0).is_err());
        let circles = [Vec2::ZERO, Vec2::new(10.0, 0.0)].map(|center| Region2 { outer: Loop2::circle(center, 5.0), holes: vec![] });
        assert!(extrude_planar_regions(&plane, &circles, 0.0, 2.0).is_err());
    }

    /// An ellipse about `c` with semi-axes `a` (along x) and `b`, as four exact conics.
    fn ellipse(c: Vec2, a: f64, b: f64) -> Loop2 {
        let corners = [c + Vec2::new(a, 0.0), c + Vec2::new(0.0, b), c - Vec2::new(a, 0.0), c - Vec2::new(0.0, b)];
        let w = std::f64::consts::FRAC_1_SQRT_2;
        Loop2 {
            segs: (0..4).map(|i| Seg2::Conic { a: corners[i], apex: corners[i] + corners[(i + 1) % 4] - c, b: corners[(i + 1) % 4], w }).collect(),
        }
    }

    #[test]
    fn free_form_cut_outs_stay_exact_when_whole_and_follow_arcs_when_cut() {
        let plate = rectangle(Vec2::ZERO, Vec2::new(100.0, 60.0));
        let hole = Region2 { outer: ellipse(Vec2::new(40.0, 30.0), 15.0, 8.0), holes: vec![] };
        let out = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&hole), BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].holes.len(), 1);
        // The whole ellipse comes back as its four exact conics, running as a hole (clockwise):
        // the plate less π·15·8.
        let h = &out[0].holes[0];
        assert_eq!(h.segs.len(), 4);
        assert!(h.segs.iter().all(|s| hole.outer.segs.contains(&s.reversed())), "{h:?}");
        assert!(h.signed_area() < 0.0);
        let exact = 6000.0 - std::f64::consts::PI * 15.0 * 8.0;
        assert!((out[0].area() - exact).abs() < 1e-9, "{} vs {exact}", out[0].area());
        // What the cut takes is the ellipse, whole and exact, counter-clockwise.
        let taken = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&hole), BoolOp::Intersect, 1e-7).unwrap();
        assert!(taken.len() == 1 && taken[0].outer.segs.iter().all(|s| hole.outer.segs.contains(s)), "{taken:?}");
        // An ellipse centred on the plate's edge makes a notch of half the ellipse: its part
        // inside follows arcs within the fit tolerance (times half the perimeter, 37 mm).
        let over = Region2 { outer: ellipse(Vec2::new(100.0, 30.0), 15.0, 8.0), holes: vec![] };
        let out = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&over), BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].holes.is_empty() && out[0].outer.is_line_arc());
        let notched = 6000.0 - std::f64::consts::PI * 15.0 * 8.0 / 2.0;
        assert!((out[0].area() - notched).abs() < PLANAR_FIT_TOL * 37.0, "{} vs {notched}", out[0].area());
        // Two ellipses that overlap join into one outline of arcs; apart, both stay exact.
        let beside = |dx: f64| Region2 { outer: ellipse(Vec2::new(40.0 + dx, 30.0), 15.0, 8.0), holes: vec![] };
        let joined = planar_regions_boolean(std::slice::from_ref(&hole), &[beside(20.0)], BoolOp::Union, 1e-7).unwrap();
        assert!(joined.len() == 1 && joined[0].outer.is_line_arc(), "{joined:?}");
        let apart = planar_regions_boolean(std::slice::from_ref(&hole), &[beside(40.0)], BoolOp::Union, 1e-7).unwrap();
        assert!(apart.len() == 2 && apart.iter().all(|r| r.outer.segs.len() == 4 && !r.outer.is_line_arc()), "{apart:?}");
        // The flat material extrudes with the exact elliptic wall: four conic faces, two caps.
        let body = extrude_planar_regions(&Plane::XY, &planar_regions_boolean(&[plate], &[hole], BoolOp::Cut, 1e-7).unwrap(), 0.0, 2.0).unwrap();
        assert_eq!(body.face_count(), 4 + 4 + 2);
        let v = crate::measure(&body).unwrap().volume;
        assert!((v - exact * 2.0).abs() < 1e-3 * exact, "{v}");
    }

    #[test]
    fn free_form_loops_that_cannot_be_followed_are_refused() {
        let plate = rectangle(Vec2::ZERO, Vec2::new(100.0, 60.0));
        let mut bad = ellipse(Vec2::new(40.0, 30.0), 15.0, 8.0);
        bad.segs[1] = Seg2::Conic { a: bad.segs[1].start(), apex: Vec2::new(f64::NAN, 0.0), b: bad.segs[1].end(), w: 1.0 };
        let e = planar_regions_boolean(std::slice::from_ref(&plate), &[Region2 { outer: bad, holes: vec![] }], BoolOp::Cut, 1e-7).unwrap_err();
        assert!(e.to_string().contains("free-form"), "{e}");
        // More fitted arcs than the arrangement's segment budget.
        let many: Vec<Region2> = (0..400).map(|i| Region2 { outer: ellipse(Vec2::new(40.0 * i as f64, 0.0), 15.0, 8.0), holes: vec![] }).collect();
        assert!(planar_regions_boolean(&[plate], &many, BoolOp::Cut, 1e-7).is_err());
    }

    #[test]
    fn segment_boxes_hold_what_lies_within_the_tolerance() {
        // An arc of radius 10 about the origin from 45° to 135° passes the top of its circle:
        // x within ±10/√2, y from 10/√2 to 10, grown by twice the tolerance.
        let r = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
        let arc = Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: std::f64::consts::FRAC_PI_4, sweep: std::f64::consts::FRAC_PI_2 };
        for s in [arc, arc.reversed()] {
            let Aabb2 { min: lo, max: hi } = seg_box(&s, 1e-3).unwrap();
            assert!(lo.dist(Vec2::new(-r - 2e-3, r - 2e-3)) < 1e-9 && hi.dist(Vec2::new(r + 2e-3, 10.0 + 2e-3)) < 1e-9, "{lo:?} {hi:?}");
        }
        let Aabb2 { min: lo, max: hi } = seg_box(&Seg2::Line { a: Vec2::new(3.0, -1.0), b: Vec2::new(-2.0, 4.0) }, 0.5).unwrap();
        assert!(lo.dist(Vec2::new(-3.0, -2.0)) < 1e-9 && hi.dist(Vec2::new(4.0, 5.0)) < 1e-9, "{lo:?} {hi:?}");
        // A line 3 tolerances above the arc's top meets its box (each is grown by two); one
        // 5 tolerances above does not. No box (a free-form segment, a value that is not
        // finite) hides nothing.
        let level = |y: f64| seg_box(&Seg2::Line { a: Vec2::new(-1.0, y), b: Vec2::new(1.0, y) }, 1e-3);
        assert!(boxes_meet(seg_box(&arc, 1e-3), level(10.003)) && !boxes_meet(seg_box(&arc, 1e-3), level(10.005)));
        let free = ellipse(Vec2::ZERO, 2.0, 1.0).segs[0];
        assert!(seg_box(&free, 1e-3).is_none() && seg_box(&Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::X }, 1e-3).is_none());
        assert!(boxes_meet(None, level(500.0)) && box_holds(None, Vec2::new(500.0, 500.0)));
    }

    #[test]
    fn many_small_cut_outs_far_apart_cut_as_before() {
        // 400 round holes in a strip: almost every pair of segments is far apart.
        let plate = rectangle(Vec2::ZERO, Vec2::new(2000.0, 20.0));
        let holes: Vec<Region2> =
            (0..400).map(|i| Region2 { outer: Loop2::circle(Vec2::new(2.5 + 5.0 * i as f64, 10.0), 1.8), holes: vec![] }).collect();
        let out = planar_regions_boolean(&[plate], &holes, BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].holes.len(), 400);
        let exact = 40_000.0 - 400.0 * std::f64::consts::PI * 1.8 * 1.8;
        assert!((out[0].area() - exact).abs() < 1e-6, "{}", out[0].area());
    }

    #[test]
    fn analytic_circle_classification_respects_signed_sweeps_and_seams() {
        for seam in [0.0, 0.13, std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_2] {
            for reversed in [false, true] {
                let lp = Loop2::circle_from(Vec2::ZERO, 10.0, seam);
                let lp = if reversed { lp.reversed() } else { lp };
                for p in [Vec2::ZERO, Vec2::new(9.9998, 0.0), Vec2::new(0.0, 9.9998), Vec2::new(0.0, -9.9998)] {
                    assert!(inside_loop(&lp, p), "{seam} {reversed} {p:?}");
                }
                for p in [Vec2::new(11.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(0.0, -10.0)] {
                    assert!(!inside_loop(&lp, p), "{seam} {reversed} {p:?}");
                }
            }
        }
        let half = Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
                Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(0.0, -10.0) },
            ],
        };
        for lp in [half.clone(), half.reversed()] {
            assert!(inside_loop(&lp, Vec2::new(5.0, 0.0)));
            assert!(!inside_loop(&lp, Vec2::new(-5.0, 0.0)));
            assert!(!inside_loop(&lp, Vec2::new(-5.0, -10.0)));
        }
        // #307: a circle read back as two halves whose centres disagree by a few ulps, split
        // at its bottom. A ray grazing that bottom must not count one half's touch only.
        let halves = Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::new(0.0, 25.0), radius: 5.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
                Seg2::Arc {
                    center: Vec2::new(2.9e-15, 25.000000000000004),
                    radius: 4.99999999999999,
                    start: std::f64::consts::FRAC_PI_2,
                    sweep: std::f64::consts::PI,
                },
            ],
        };
        for lp in [halves.clone(), halves.reversed()] {
            for x in [-30.0, -5.0, -0.1] {
                assert!(!inside_loop(&lp, Vec2::new(x, 20.0)), "{x}");
                assert!(!inside_loop(&lp, Vec2::new(x, 30.0)), "{x}");
            }
            assert!(inside_loop(&lp, Vec2::new(0.0, 25.0)));
        }
    }

    /// Loops are closed only within the chaining tolerance. A ray level with one end of such a
    /// gap slipped through it without a crossing, so a point beside the loop counted as inside:
    /// a sheet shrunk by a parameter change (sketch solver residuals of 1e-13 mm) lost a bend
    /// zone that way and its unfold failed.
    #[test]
    fn ray_parity_closes_the_gaps_chaining_leaves() {
        let gap = 1e-13;
        let square = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) },
                Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 10.0) },
                Seg2::Line { a: Vec2::new(10.0, 10.0), b: Vec2::new(0.0, 10.0) },
                Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(0.0, 5.0 + gap) },
                Seg2::Line { a: Vec2::new(0.0, 5.0), b: Vec2::new(0.0, 0.0) },
            ],
        };
        for lp in [square.clone(), square.reversed()] {
            assert!(!inside_loop(&lp, Vec2::new(-5.0, 5.0)));
            assert!(inside_loop(&lp, Vec2::new(5.0, 5.0)));
            assert!(!inside_loop(&lp, Vec2::new(15.0, 5.0)));
        }
        // A neighbour with an edge level with the gap joins the square instead of being
        // dropped as if it lay inside it (which left the outline open: no result at all).
        let a = Region2 { outer: square, holes: vec![] };
        let b = rectangle(Vec2::new(-4.0, 2.0), Vec2::new(0.0, 5.0));
        let union = planar_regions_boolean(&[a], &[b], BoolOp::Union, 1e-7).unwrap();
        assert_eq!(union.len(), 1);
        assert!((union[0].area().abs() - 112.0).abs() < 1e-9, "{}", union[0].area());
    }
    /// Two edges lying on one another within the tolerance, a hair from parallel, meet at a
    /// point somewhere along their overlap. Taken for a crossing, that point split the square's
    /// edge once more: 5e-8 mm before its end, which left an edge shorter than the tolerance in
    /// the result, or in the middle of a straight edge. The ends of an overlap are what split
    /// the edges.
    #[test]
    fn overlapping_lines_a_hair_from_parallel_do_not_cross() {
        let square = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        // A quadrilateral from x = 4 to x = 14 with one edge along the square's bottom edge,
        // tilted by 1e-9 rad about (meet, 0), and the opposite edge at y = far.
        let quad = |meet: f64, far: f64| {
            let on = |x: f64| Vec2::new(x, (meet - x) * 1e-9);
            let mut corners = [on(4.0), on(14.0), Vec2::new(14.0, far), Vec2::new(4.0, far)];
            if far < 0.0 {
                corners.reverse();
            }
            Region2 { outer: Loop2::polygon(&corners), holes: vec![] }
        };
        let lengths = |r: &Region2| {
            let mut all: Vec<f64> = r.outer.segs.iter().map(Seg2::length).collect();
            all.sort_by(f64::total_cmp);
            all
        };
        let same = |got: &[f64], want: &[f64]| got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6);
        for meet in [10.0 - 5e-8, 7.0] {
            // Below the square, against its bottom edge: cut away, it leaves the square, the
            // bottom edge split only at x = 4, where the quadrilateral's corner lies on it.
            let below = quad(meet, -5.0);
            let cut = planar_regions_boolean(std::slice::from_ref(&square), std::slice::from_ref(&below), BoolOp::Cut, 1e-7).unwrap();
            assert_eq!(cut.len(), 1, "{meet}");
            assert!(same(&lengths(&cut[0]), &[4.0, 6.0, 10.0, 10.0, 10.0]), "{meet}: cut edges {:?}", lengths(&cut[0]));
            assert!((cut[0].area() - 100.0).abs() < 1e-6, "{meet}: {}", cut[0].area());
            // Over the square, flush with that edge and reaching past it to x = 14: the
            // union's outline runs along y = 0 once, in the pieces 0..4, 4..10 and 10..14.
            let flush = quad(meet, 5.0);
            let union = planar_regions_boolean(std::slice::from_ref(&square), std::slice::from_ref(&flush), BoolOp::Union, 1e-7)
                .unwrap_or_else(|e| panic!("{meet}: union {e}"));
            assert_eq!(union.len(), 1, "{meet}");
            assert!(same(&lengths(&union[0]), &[4.0, 4.0, 4.0, 5.0, 5.0, 6.0, 10.0, 10.0]), "{meet}: union edges {:?}", lengths(&union[0]));
            assert!((union[0].area() - 120.0).abs() < 1e-6, "{meet}: {}", union[0].area());
            // The two edges themselves, either way round.
            let (edge, tilted) = (square.outer.segs[0], flush.outer.segs[0]);
            assert!(crossings(&edge, &tilted, 1e-7).is_empty() && crossings(&tilted, &edge, 1e-7).is_empty(), "{meet}");
        }
        // Lines further apart than the tolerance at their ends still cross: tilted by 1e-3 rad
        // about (7, 0), the edge from (4, 3e-3) to (14, -7e-3) meets the bottom edge at x = 7.
        let (edge, steep) = (square.outer.segs[0], Seg2::Line { a: Vec2::new(4.0, 3e-3), b: Vec2::new(14.0, -7e-3) });
        let hits = crossings(&edge, &steep, 1e-7);
        assert!(hits.len() == 1 && (hits[0].0 - 0.7).abs() < 1e-12 && (hits[0].1 - 0.3).abs() < 1e-12, "{hits:?}");
    }

    #[test]
    fn tiny_tool_between_circle_and_display_chord_is_not_lost() {
        let circle = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] };
        let sampled = circle.outer.polyline(1e-3);
        let direction = (sampled[0] + sampled[1]).normalized().unwrap();
        let at = direction * 9.9998;
        let half = Vec2::new(0.000025, 0.000025);
        let square = rectangle(at - half, at + half);
        assert!(square.outer.segs.iter().all(|s| s.start().len() < 10.0));
        assert!(!circle.contains(at), "fixture must lie outside the display chord");
        let cut = planar_regions_boolean(std::slice::from_ref(&circle), std::slice::from_ref(&square), BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(cut.len(), 1);
        assert_eq!(cut[0].holes.len(), 1);
        assert!((cut[0].holes[0].signed_area().abs() - 2.5e-9).abs() < 1e-13);
        let intersect = planar_regions_boolean(&[circle], &[square], BoolOp::Intersect, 1e-7).unwrap();
        assert_eq!(intersect.len(), 1);
        assert!((intersect[0].area() - 2.5e-9).abs() < 1e-13);
    }
    /// A 200 x 37 plate turned by `theta` about the origin, and the fillet region of one of its
    /// corners: an arc of radius `r` tangent to both edges there, closed by two lines that meet
    /// `reach` out past the corner.
    fn plate_and_corner_fillet(theta: f64, r: f64, reach: f64) -> (Region2, Region2) {
        let (sn, cs) = theta.sin_cos();
        let rot = |x: f64, y: f64| Vec2::new(x * cs - y * sn, x * sn + y * cs);
        let (w, lo, hi) = (200.0, -40.25, -3.25);
        let plate = Region2 { outer: Loop2::polygon(&[rot(0.0, hi), rot(0.0, lo), rot(w, lo), rot(w, hi)]), holes: vec![] };
        let arc = Seg2::Arc { center: rot(w - r, lo + r), radius: r, start: theta, sweep: -std::f64::consts::FRAC_PI_2 };
        let out = rot(w + reach, lo - reach);
        let cut =
            Region2 { outer: Loop2 { segs: vec![arc, Seg2::Line { a: arc.end(), b: out }, Seg2::Line { a: out, b: arc.start() }] }, holes: vec![] };
        (plate, cut)
    }

    /// A contact between a line and an arc that touch is one point. The quadratic's two roots
    /// there are rounding noise: a 1e-12 mm error in the centre's distance from the line moves
    /// them 2e-6 mm either way along it, and the cut then ran that far along the plate's edge
    /// past the arc's end before turning onto the arc: an edge a few millionths of a millimetre
    /// long in the result. Which turns of the plate do that is a matter of rounding, so every
    /// one is tried.
    #[test]
    fn an_arc_tangent_to_the_edges_it_ends_on_leaves_no_hairline_edges() {
        for k in 0..=360 {
            let theta = (k as f64 * 0.25).to_radians();
            for r in [2.0, 0.5, 25.0] {
                let (plate, cut) = plate_and_corner_fillet(theta, r, 0.4);
                let out = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&cut), BoolOp::Cut, 1e-7)
                    .unwrap_or_else(|e| panic!("{k} r {r}: {e}"));
                assert_eq!(out.len(), 1, "{k} r {r}");
                let lp = &out[0].outer;
                let shortest = lp.segs.iter().map(Seg2::length).fold(f64::MAX, f64::min);
                let arcs = lp.segs.iter().filter(|s| matches!(s, Seg2::Arc { .. })).count();
                assert!(
                    out[0].holes.is_empty() && lp.segs.len() == 5 && arcs == 1 && shortest > 0.5,
                    "{k} r {r}: {} edges, shortest {shortest:e}",
                    lp.segs.len()
                );
                let want = 200.0 * 37.0 - r * r * (1.0 - std::f64::consts::FRAC_PI_4);
                assert!((out[0].area() - want).abs() < 1e-9, "{k} r {r}: {} vs {want}", out[0].area());
                // What the cut takes is the corner's fillet region, whole.
                let taken = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&cut), BoolOp::Intersect, 1e-7).unwrap();
                assert!(taken.len() == 1 && taken[0].outer.segs.len() == 3, "{k} r {r}: {taken:?}");
                assert!((taken[0].area() - r * r * (1.0 - std::f64::consts::FRAC_PI_4)).abs() < 1e-9, "{k} r {r}");
            }
        }
    }

    /// A 55 x 30 plate whose bottom edge bends up by `slope` over its last 5 mm before the
    /// corner at (100, 50), and a strip 3 wide below the straight 50 mm, as a sheet's bend zone
    /// lies against its base: the strip's corners on that edge rounded to the nanometre, as a
    /// flange keeps them, and moved by `off`. Both turned by `theta` about the origin.
    fn plate_and_strip_by_a_bent_edge(theta: f64, slope: f64, off: Vec2) -> (Region2, Region2) {
        let (sn, cs) = theta.sin_cos();
        let rot = |x: f64, y: f64| Vec2::new(x * cs - y * sn, x * sn + y * cs);
        let nm = |p: Vec2| Vec2::new((p.x * 1e9).round() / 1e9, (p.y * 1e9).round() / 1e9) + off;
        let plate = [rot(95.0, 50.0 + 5.0 * slope), rot(100.0, 50.0), rot(150.0, 50.0), rot(150.0, 80.0), rot(95.0, 80.0)];
        let strip = [nm(rot(100.0, 50.0)), rot(100.0, 47.0), rot(150.0, 47.0), nm(rot(150.0, 50.0))];
        (Region2 { outer: Loop2::polygon(&plate), holes: vec![] }, Region2 { outer: Loop2::polygon(&strip), holes: vec![] })
    }

    /// The bent piece of the plate's edge, a hair from the strip's edge, crosses it short of
    /// the corner they share: the corners a nanometre apart at most, the crossing up to a
    /// hundred times that before it at a slope of 1 in 100, a thousand at 1 in 1000. A split
    /// there left a sliver of edge shorter than the distance within which the loops join their
    /// pieces; the chaining took it for a second way out of the corner, and a loop of that one
    /// sliver is open, its area not zero away from the origin: a hole no outline held (no
    /// result at all) or a region of its own. The union is the plate and the strip, whole.
    #[test]
    fn a_bent_edge_crossing_its_neighbour_by_their_corner_leaves_no_sliver() {
        let union_is_whole = |(plate, strip): (Region2, Region2), what: &str| {
            let want = plate.area() + strip.area();
            for (a, b) in [(&plate, &strip), (&strip, &plate)] {
                let out = planar_regions_boolean(std::slice::from_ref(a), std::slice::from_ref(b), BoolOp::Union, 1e-7)
                    .unwrap_or_else(|e| panic!("{what}: {e}"));
                assert_eq!(out.len(), 1, "{what}: {out:?}");
                let lp = &out[0].outer;
                let shortest = lp.segs.iter().map(Seg2::length).fold(f64::MAX, f64::min);
                assert!(out[0].holes.is_empty() && lp.segs.len() == 7 && shortest > 2.9, "{what}: {} edges, shortest {shortest:e}", lp.segs.len());
                assert!((out[0].area() - want).abs() < 1e-6, "{what}: {} vs {want}", out[0].area());
            }
        };
        for k in 0..360 {
            for slope in [0.01, 0.001] {
                union_is_whole(plate_and_strip_by_a_bent_edge((k as f64 * 0.25).to_radians(), slope, Vec2::ZERO), &format!("{k} slope {slope}"));
            }
        }
        // Corners a few nanometres apart, any way round: the sliver is then longer than the
        // tolerance (1e-7 with the strip 1e-9 into the plate) but shorter than the joining
        // distance (1e-6).
        for k in 0..16 {
            for d in [1e-9, 4e-9] {
                let off = Vec2::from_angle(k as f64 * std::f64::consts::TAU / 16.0) * d;
                union_is_whole(plate_and_strip_by_a_bent_edge(0.0, 0.01, off), &format!("off {off:?}"));
            }
        }
    }

    /// Touching circles meet at one point on the line of their centres, inside and outside.
    #[test]
    fn touching_lines_and_arcs_meet_at_one_point() {
        let full = |c: Vec2, r: f64| Seg2::Arc { center: c, radius: r, start: 0.3, sweep: std::f64::consts::TAU - 1e-3 };
        for k in 0..=360 {
            let dir = Vec2::from_angle((k as f64 * 0.25).to_radians());
            let c = Vec2::new(151.3, -37.7);
            for (r0, r1, d) in [(2.0, 3.0, 5.0), (7.0, 3.0, 4.0), (3.0, 7.0, 4.0), (250.0, 2.0, 248.0)] {
                let hits = crossings(&full(c, r0), &full(c + dir * d, r1), 1e-7);
                let touch = c + dir * if r1 > r0 && d < r1 { -r0 } else { r0 };
                let pts: Vec<Vec2> = hits.iter().map(|(u, _)| full(c, r0).point_at(*u)).collect();
                assert!(!pts.is_empty() && pts.iter().all(|p| p.dist(touch) < 1e-9), "{k} {r0} {r1}: {pts:?} vs {touch:?}");
            }
            // A line touching a circle, the circle's side of it and the far side by rounding.
            for r in [2.0, 250.0] {
                let foot = c + dir * r;
                let line = Seg2::Line { a: foot - dir.perp() * 120.0, b: foot + dir.perp() * 78.0 };
                let hits = crossings(&line, &full(c, r), 1e-7);
                let pts: Vec<Vec2> = hits.iter().map(|(u, _)| line.point_at(*u)).collect();
                assert!(!pts.is_empty() && pts.iter().all(|p| p.dist(foot) < 1e-9), "{k} {r}: {pts:?} vs {foot:?}");
            }
        }
        // Real crossings stay two points: a chord 0.01 mm deep in a 2 mm circle.
        let line = Seg2::Line { a: Vec2::new(-5.0, 1.99), b: Vec2::new(5.0, 1.99) };
        let hits = crossings(&line, &full(Vec2::ZERO, 2.0), 1e-7);
        let half = (4.0f64 - 1.99 * 1.99).sqrt();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|(u, _)| (line.point_at(*u).x.abs() - half).abs() < 1e-12), "{hits:?}");
    }

    #[test]
    fn planar_region_api_rejects_hostile_values_and_budgets() {
        let plate = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        for tolerance in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(planar_regions_boolean(std::slice::from_ref(&plate), &[], BoolOp::Union, tolerance).is_err());
        }
        let mut bad = plate.clone();
        bad.outer.segs[0] = Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::X };
        assert!(planar_regions_boolean(&[bad], &[], BoolOp::Union, 1e-7).is_err());
        let many = vec![plate; MAX_PLANAR_SEGMENTS / 4 + 1];
        assert!(planar_regions_boolean(&many, &[], BoolOp::Union, 1e-7).is_err());
    }
}
