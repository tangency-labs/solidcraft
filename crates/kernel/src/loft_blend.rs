//! Multi-section blends: one face that runs through many cross-sections and still leaves the
//! first and last of them the way a support surface wants.
//!
//! The sections run in order along the face's **u**, each sampled at the same `samples` stations
//! along the face's **v**. Section `i` sits at `u = i/(n−1)`, and the face passes through it
//! exactly: its v-curve there is that section's own cross-section curve through its stations.
//! Along v the stations are interpolated by a B-spline of degree `min(3, samples − 1)` — the same
//! interpolation [`crate::bridge`] builds its boundary curves with — so every station of every
//! section lies on the face.
//!
//! Along u the face is a B-spline whose knots are clamped at both ends and repeated at every
//! interior section, so every span is one Bézier; a span's leading control rows are the Hermite
//! rows of `bridge::section` — position, tangent (`leave`), curvature (`bend`) and change of
//! curvature (`bend_change`), scaled to that span's parameter interval. The degree is the largest
//! `c + c′ − 1` over the spans (`c` conditions at the section the span leaves, `c′` at the one it
//! arrives at), and a span whose two ends carry fewer conditions than that is elevated exactly,
//! which moves no point of the curve. A blend of two sections is therefore the [`crate::bridge`]
//! of those two boundaries, only with the two parameters the other way round.
//!
//! **The first and last section** take their tangent, curvature and change of curvature from the
//! caller's frames, exactly as a bridge takes them from a support surface's boundary, and hold
//! them to the requested [`Continuity`] (G0 up to G3). The `leave` direction of the last section's
//! frames points back into the blend, towards the interior, as a bridge's far boundary does.
//!
//! **Interior sections** take nothing from the caller's frames but their positions: the run
//! direction's tangent there is the secant of the two neighbouring sections' stations, its
//! curvature their second difference and its change of curvature their third difference, all
//! along the run. The two spans that meet at such a section are given exactly those rows, so they
//! agree in ∂/∂u, ∂²/∂u² and ∂³/∂u³ there and the joint is **C³** in the run parameter (hence G³)
//! whenever a fourth station is there to take the third difference over — that is, whenever the
//! blend runs through at least four sections. A blend of exactly three sections has only its one
//! interior section's two neighbours to read, so no third difference can be taken: the interior
//! section then keeps position, tangent and curvature only and its joint is **C²** (G²). Dropping
//! interior conditions, never the surface, is also how this blend stays within
//! [`MAX_SPLINE_DEGREE`]: a section family asks for at most `c + c′ − 1 ≤ 7` at the four conditions
//! a section can carry today, and a caller that ever asked for more would lose interior
//! conditions (change of curvature first, then curvature, then tangent) rather than fail.
//!
//! `crown` scales how fast the face leaves the first and the last section (0 the plain blend, up
//! to 3 fuller, down to −0.9 flatter), exactly as a bridge's crown does. It shapes the middle of
//! each end span and leaves every joint's conditions alone.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::bspline::{CurveN, HPoint, MAX_SPLINE_DEGREE, NurbsSurface};

use crate::body::Body;
use crate::bridge::{Continuity, Frame};
use crate::{KernelError, Result};

/// Most cross-sections one blend may run through.
pub const MAX_BLEND_SECTIONS: usize = 50;
/// Fewest stations one cross-section may be sampled at (a straight line across it).
pub const MIN_BLEND_STATIONS: usize = 2;
/// Most stations one cross-section may be sampled at. The control net (and the interpolation that
/// builds it) grows with the station count, and no cross-section curve needs more than this.
pub const MAX_BLEND_STATIONS: usize = 257;

/// How many rows of the control net one interpolation solve carries at a time: a row is three
/// coordinates, so this many rows are solved together.
const ROWS_PER_SOLVE: usize = 16;

/// A blend surface through `sections`, in order, that leaves the first and the last of them with
/// the continuity their frames ask for.
///
/// Each section is one cross-section: `samples` stations (`samples` must match every section's
/// length, so that the stations correspond from one section to the next). Section `i` is
/// interpolated at `u = i/(n−1)`, and its stations run along `v`, so the face of the body holds
/// every station of every section. `orders[0]` and `orders[1]` are what the face holds at the
/// first and the last section, read from their frames; the interior sections join the face
/// smoothly by finite differences along the run (see the module comment for the order each joint
/// holds). `crown` scales how fast the face leaves the first (`crown[0]`) and the last
/// (`crown[1]`) section and must be within −0.9 and 3.
///
/// Refused, without ever panicking: fewer than 2 or more than [`MAX_BLEND_SECTIONS`] sections,
/// sections of unequal station counts or a `samples` below [`MIN_BLEND_STATIONS`] or above
/// [`MAX_BLEND_STATIONS`], a frame that is not finite or a `leave` with no direction, a crown
/// outside its range, and two neighbouring sections that meet at a station (no room for a blend).
pub fn loft_blend(sections: &[Vec<Frame>], orders: [Continuity; 2], crown: [f64; 2], samples: usize) -> Result<Body> {
    for g in orders {
        g.built_by(crate::continuity_order::Construction::Bridge)?;
    }
    let blend = Blend::new(sections, orders, crown, samples)?;
    let net = blend.net()?;
    crate::surfaces::tensor_sheet(&net)
}

/// The frames of one cross-section picked as an edge chain of a body, sampled at `stations`
/// stations: the chain itself, with the neighbouring face's tangent plane, curvature vector and
/// change of curvature read along `leave` — the direction the blend runs on from this section —
/// exactly as [`crate::bridge::frames`] reads a support surface's own boundary.
///
/// Hand the result to [`loft_blend`] as one section. The face the section repeats is the body's
/// face that holds the whole chain and faces the run best; `leave` must have a part in that face's
/// tangent plane (a direction normal to it has no tangent plane to leave in). The last section's
/// `leave` runs back into the blend, towards the section before it.
pub fn section_frames(body: &Body, edges: &[Vec3], leave: Vec3, stations: usize) -> Result<Vec<Frame>> {
    let chain = crate::bridge::chain(body, edges)?;
    let leave =
        leave.normalized().filter(|l| l.is_finite()).ok_or_else(|| KernelError::Invalid("the leave direction must be a finite vector".into()))?;
    let r = body.record();
    let &(first, _) = chain.first().ok_or_else(|| KernelError::Failed("loft blend: no chain".into()))?;
    let ends = r.edges.get(first).map(|e| e.vertices).ok_or_else(|| KernelError::Failed("loft blend: an edge reference".into()))?;
    let point = |v: u32| r.vertices.get(v as usize).map(|x| Vec3::new(x.point[0], x.point[1], x.point[2]));
    let (Some(a), Some(b)) = (point(ends[0]), point(ends[1])) else { return Err(KernelError::Failed("loft blend: a vertex reference".into())) };
    let mid = (a + b) * 0.5;
    // The face the chain lies on that the blend leaves through: the one whose tangent plane holds
    // as much of `leave` as possible.
    let bounds = |f: usize, e: usize| r.edges.get(e).is_some_and(|x| x.uses.iter().any(|u| u.face as usize == f));
    let mut best: Option<(f64, usize)> = None;
    for (f, face) in r.faces.iter().enumerate() {
        if !chain.iter().all(|(e, _)| bounds(f, *e)) {
            continue;
        }
        let Ok(surf) = SurfaceEval::new(face.parameters()) else { continue };
        let Some((_, su, sv)) = surf.project(mid, None, f64::INFINITY).and_then(|uv| surf.derivs(uv[0], uv[1])) else { continue };
        let n = su.cross(sv).normalized().unwrap_or(Vec3::ZERO);
        let score = (leave - n * leave.dot(n)).len();
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, f));
        }
    }
    let (score, face) = best.ok_or_else(|| KernelError::Invalid("no face of the body holds the picked edges".into()))?;
    if score < 0.2 {
        return Err(KernelError::Invalid("the direction towards the next section lies in no face at the picked edges".into()));
    }
    crate::bridge::frames(body, face, &chain, leave, stations)
}

/// What the face has to do at one section: where it is, and the run direction's first, second and
/// third derivatives at each of the section's stations. The third is carried only when the
/// section holds a change-of-curvature condition.
struct Shape {
    p: Vec<Vec3>,
    d1: Vec<Vec3>,
    d2: Vec<Vec3>,
    d3: Option<Vec<Vec3>>,
}

/// The blend's inputs, validated, with the conditions and the run direction's degree worked out.
struct Blend<'a> {
    sections: &'a [Vec<Frame>],
    crown: [f64; 2],
    samples: usize,
    run: usize,
    /// The run parameter interval between two neighbouring sections: `1/(run − 1)`.
    step: f64,
    degree: usize,
    /// Conditions held at each section, one per control row of the spans that end there.
    conditions: Vec<usize>,
}

impl<'a> Blend<'a> {
    fn new(sections: &'a [Vec<Frame>], orders: [Continuity; 2], crown: [f64; 2], samples: usize) -> Result<Blend<'a>> {
        if !(MIN_BLEND_STATIONS..=MAX_BLEND_STATIONS).contains(&samples) {
            return Err(KernelError::Invalid(format!(
                "a blend needs {MIN_BLEND_STATIONS} to {MAX_BLEND_STATIONS} stations per cross-section, not {samples}"
            )));
        }
        let run = sections.len();
        if !(2..=MAX_BLEND_SECTIONS).contains(&run) {
            return Err(KernelError::Invalid(format!("a blend runs through 2 to {MAX_BLEND_SECTIONS} cross-sections, not {run}")));
        }
        for c in crown {
            if !c.is_finite() || !(-0.9..=3.0).contains(&c) {
                return Err(KernelError::Invalid("a blend's crown must be between -0.9 and 3".into()));
            }
        }
        for (i, section) in sections.iter().enumerate() {
            if section.len() != samples {
                return Err(KernelError::Invalid(format!(
                    "every cross-section of a blend must be sampled at the same {samples} stations (section {i} has {})",
                    section.len()
                )));
            }
            for (s, f) in section.iter().enumerate() {
                if ![f.p, f.leave, f.bend, f.bend_change].into_iter().all(Vec3::is_finite) {
                    return Err(KernelError::Invalid(format!("the frame at section {i}, station {s} is not finite")));
                }
                if f.leave.normalized().is_none() {
                    return Err(KernelError::Invalid(format!("the leave direction at section {i}, station {s} is zero")));
                }
            }
        }
        // The face runs from one section to the next: where two of them meet at a station there is
        // no room for a blend between them.
        for i in 0..run - 1 {
            for s in 0..samples {
                if !(point(sections, i, s)?.dist(point(sections, i + 1, s)?) > 1e-9) {
                    return Err(KernelError::Invalid(format!("sections {} and {} meet at station {s}: there is no room for a blend", i + 1, i + 2)));
                }
            }
        }
        // The ends hold what their orders ask for. An interior section would like all four
        // conditions (position, tangent, curvature, change of curvature); where no fourth station
        // is there to take the third difference over it keeps the three its neighbours can give,
        // and its joint is C² instead of C³.
        let mut conditions = vec![2usize; run];
        if let Some(slot) = conditions.first_mut() {
            *slot = orders[0].conditions();
        }
        if let Some(slot) = conditions.last_mut() {
            *slot = orders[1].conditions();
        }
        for i in 1..run.saturating_sub(1) {
            if let Some(slot) = conditions.get_mut(i) {
                *slot = if third_difference_available(i, run) { 4 } else { 3 };
            }
        }
        // The run direction's degree is the largest `c + c′ − 1` of a span. Four conditions a
        // section is at most what a section can carry, so this fits today; should a caller ever
        // ask for more, the interior joints give up conditions rather than the blend failing.
        let mut degree = span_degree(&conditions);
        while degree > MAX_SPLINE_DEGREE {
            let next = (1..run.saturating_sub(1))
                .filter(|&i| conditions.get(i).is_some_and(|&c| c > 1))
                .max_by_key(|&i| conditions.get(i).copied().unwrap_or(0));
            let Some(i) = next else { break };
            if let Some(c) = conditions.get_mut(i) {
                *c = c.saturating_sub(1);
            }
            degree = span_degree(&conditions);
        }
        if degree == 0 || degree > MAX_SPLINE_DEGREE {
            return Err(KernelError::Failed("loft blend: the requested continuity needs a degree above the kernel's".into()));
        }
        Ok(Blend { sections, crown, samples, run, step: 1.0 / (run - 1) as f64, degree, conditions })
    }

    /// What the face has to do at section `i`: the run direction's derivatives at every station.
    fn shape(&self, i: usize) -> Result<Shape> {
        let samples = self.samples;
        let p: Vec<Vec3> = (0..samples).map(|s| point(self.sections, i, s)).collect::<Result<Vec<_>>>()?;
        let conditions = self.conditions.get(i).copied().unwrap_or(1);
        let mut d1 = vec![Vec3::ZERO; samples];
        let mut d2 = vec![Vec3::ZERO; samples];
        let mut d3 = (conditions >= 4).then(|| vec![Vec3::ZERO; samples]);
        if i == 0 {
            for s in 0..samples {
                let f = frame(self.sections, 0, s)?;
                let leave = f.leave.normalized().ok_or_else(|| KernelError::Failed("loft blend: the leave direction".into()))?;
                // The speed a bridge gives a boundary: a third of the station's chord to the
                // section it leaves for, the crown, over this span's parameter interval.
                let speed = f.p.dist(point(self.sections, 1, s)?) / 3.0 * (1.0 + self.crown[0]) / self.step;
                if let (Some(v1), Some(v2)) = (d1.get_mut(s), d2.get_mut(s)) {
                    *v1 = leave * speed;
                    *v2 = f.bend * (speed * speed);
                }
            }
        } else if i + 1 == self.run {
            for s in 0..samples {
                let f = frame(self.sections, i, s)?;
                let leave = f.leave.normalized().ok_or_else(|| KernelError::Failed("loft blend: the leave direction".into()))?;
                let speed = f.p.dist(point(self.sections, i - 1, s)?) / 3.0 * (1.0 + self.crown[1]) / self.step;
                if let (Some(v1), Some(v2)) = (d1.get_mut(s), d2.get_mut(s)) {
                    // The run leaves the last section outwards; the caller's frames there point
                    // back into it, so the odd derivatives turn round.
                    *v1 = leave * -speed;
                    *v2 = f.bend * (speed * speed);
                }
            }
        } else {
            let h = self.step;
            for s in 0..samples {
                let before = point(self.sections, i - 1, s)?;
                let here = point(self.sections, i, s)?;
                let after = point(self.sections, i + 1, s)?;
                if let (Some(v1), Some(v2)) = (d1.get_mut(s), d2.get_mut(s)) {
                    *v1 = (after - before) * (1.0 / (2.0 * h));
                    *v2 = (after - here * 2.0 + before) * (1.0 / (h * h));
                }
            }
        }
        // The change of curvature: the caller's bend_change at an end, a third difference of the
        // neighbouring sections at an interior one.
        if let Some(third) = d3.as_mut() {
            for s in 0..samples {
                let (Some(&a), Some(&b)) = (d1.get(s), d2.get(s)) else {
                    return Err(KernelError::Failed("loft blend: a section's derivatives".into()));
                };
                let value = if i == 0 {
                    let f = frame(self.sections, 0, s)?;
                    third_derivative(a, b, f.bend_change)?
                } else if i + 1 == self.run {
                    let f = frame(self.sections, i, s)?;
                    -third_derivative(-a, b, f.bend_change)?
                } else {
                    self.third_difference(i, s)?
                };
                if let Some(slot) = third.get_mut(s) {
                    *slot = value;
                }
            }
        }
        Ok(Shape { p, d1, d2, d3 })
    }

    /// The third difference of the sections' stations at an interior section: the estimate of how
    /// the curvature vector changes along the run. `i` is an interior section, and the neighbours
    /// reach far enough for a stencil (see [`third_difference_available`]).
    fn third_difference(&self, i: usize, s: usize) -> Result<Vec3> {
        let h = self.step;
        let cube = h * h * h;
        let before = i.checked_sub(2);
        let value = match (before, i + 2 < self.run) {
            // Both sides: the symmetric five-point difference.
            (Some(k), true) => {
                (point(self.sections, i + 2, s)? - point(self.sections, i + 1, s)? * 2.0 + point(self.sections, i - 1, s)? * 2.0
                    - point(self.sections, k, s)?)
                    * (1.0 / (2.0 * cube))
            }
            // Only ahead: the forward four-point stencil at the second of them.
            (None, true) => {
                (point(self.sections, i + 2, s)? - point(self.sections, i + 1, s)? * 3.0 + point(self.sections, i, s)? * 3.0
                    - point(self.sections, i - 1, s)?)
                    * (1.0 / cube)
            }
            // Only behind: the same the other way round.
            (Some(k), false) => {
                (point(self.sections, k, s)? - point(self.sections, i - 1, s)? * 3.0 + point(self.sections, i, s)? * 3.0
                    - point(self.sections, i + 1, s)?)
                    * (1.0 / cube)
            }
            (None, false) => return Err(KernelError::Failed("loft blend: no fourth station for a change of curvature".into())),
        };
        if !value.is_finite() {
            return Err(KernelError::Failed("loft blend: the sections' third difference is not finite".into()));
        }
        Ok(value)
    }

    /// The Bézier rows of the span from section `j` to section `j + 1`: one control row per
    /// condition at each end, elevated to the run direction's degree so the whole face is one
    /// tensor surface. Row `k` of the span is control row `j·degree + k` of the face.
    fn span(&self, j: usize, left: &Shape, right: &Shape) -> Result<Vec<Vec<Vec3>>> {
        let (Some(&kl), Some(&kr)) = (self.conditions.get(j), self.conditions.get(j + 1)) else {
            return Err(KernelError::Failed("loft blend: a section with no conditions".into()));
        };
        let m = kl.saturating_add(kr).saturating_sub(1);
        if !(1..=MAX_SPLINE_DEGREE).contains(&m) {
            return Err(KernelError::Failed("loft blend: a span's degree".into()));
        }
        let k = m as f64;
        let delta = self.step;
        let one = delta / k;
        let two = if m >= 2 { delta * delta / (k * (k - 1.0)) } else { 0.0 };
        let three = if m >= 3 { delta * delta * delta / (k * (k - 1.0) * (k - 2.0)) } else { 0.0 };
        let mut out = vec![vec![Vec3::ZERO; self.samples]; m + 1];
        let missing = || KernelError::Failed("loft blend: a section's derivatives".into());
        for s in 0..self.samples {
            // The near end: one row per condition, position first.
            let p = *left.p.get(s).ok_or_else(missing)?;
            put(&mut out, 0, s, p)?;
            if kl >= 2 {
                let d1 = *left.d1.get(s).ok_or_else(missing)?;
                let q1 = p + d1 * one;
                put(&mut out, 1, s, q1)?;
                if kl >= 3 {
                    let d2 = *left.d2.get(s).ok_or_else(missing)?;
                    let q2 = d2 * two + q1 * 2.0 - p;
                    put(&mut out, 2, s, q2)?;
                    if kl >= 4 {
                        let d3 = left.d3.as_ref().and_then(|d| d.get(s)).copied().ok_or_else(missing)?;
                        put(&mut out, 3, s, d3 * three + q2 * 3.0 - q1 * 3.0 + p)?;
                    }
                }
            }
            // The far end, its own conditions read backwards.
            let e = *right.p.get(s).ok_or_else(missing)?;
            put(&mut out, m, s, e)?;
            if kr >= 2 {
                let d1 = *right.d1.get(s).ok_or_else(missing)?;
                let r1 = e - d1 * one;
                put(&mut out, m - 1, s, r1)?;
                if kr >= 3 {
                    let d2 = *right.d2.get(s).ok_or_else(missing)?;
                    let r2 = d2 * two + r1 * 2.0 - e;
                    put(&mut out, m - 2, s, r2)?;
                    if kr >= 4 {
                        let d3 = right.d3.as_ref().and_then(|d| d.get(s)).copied().ok_or_else(missing)?;
                        put(&mut out, m - 3, s, r2 * 3.0 - r1 * 3.0 + e - d3 * three)?;
                    }
                }
            }
        }
        // A span whose two ends carry fewer conditions than the run direction's degree is
        // elevated: the same curve, written in more control rows.
        while out.len().saturating_sub(1) < self.degree {
            out = elevate(&out);
        }
        Ok(out)
    }

    /// The run direction's knots: clamped at both ends, repeated `degree` times at every interior
    /// section — so the face interpolates each section and every span in between is one Bézier.
    fn run_knots(&self) -> Vec<f64> {
        let degree = self.degree;
        let mut knots = vec![0.0; degree + 1];
        for i in 1..self.run.saturating_sub(1) {
            knots.extend(std::iter::repeat_n(i as f64 / (self.run - 1) as f64, degree));
        }
        knots.extend(std::iter::repeat_n(1.0, degree + 1));
        knots
    }

    /// The face's control net: u runs with the sections, v along each section's curve.
    fn net(&self) -> Result<NurbsSurface> {
        let shapes: Vec<Shape> = (0..self.run).map(|i| self.shape(i)).collect::<Result<Vec<_>>>()?;
        // One span per pair of neighbouring sections. The first row of a span is the last row of
        // the one before it — the shared section — so only the first span writes it.
        let mut rows: Vec<Vec<Vec3>> = Vec::with_capacity((self.run - 1) * self.degree + 1);
        for j in 0..self.run.saturating_sub(1) {
            let (Some(left), Some(right)) = (shapes.get(j), shapes.get(j + 1)) else {
                return Err(KernelError::Failed("loft blend: a section's shape".into()));
            };
            for row in self.span(j, left, right)?.into_iter().skip(usize::from(j > 0)) {
                rows.push(row);
            }
        }
        let count_u = rows.len();
        // Along each section's curve the rows are the interpolation through that section's
        // stations: the same parameters and degree for every row, so every row has one knot
        // vector. Rows are solved in batches of `ROWS_PER_SOLVE` (three coordinates each), which
        // shares one elimination between them.
        let params: Vec<f64> = (0..self.samples).map(|s| s as f64 / (self.samples - 1) as f64).collect();
        let degree_v = 3.min(self.samples - 1);
        let mut ctrl = vec![HPoint::ZERO; count_u * self.samples];
        let mut knots_v: Option<Vec<f64>> = None;
        for (batch, chunk) in rows.chunks(ROWS_PER_SOLVE).enumerate() {
            const WIDTH: usize = 3 * ROWS_PER_SOLVE;
            let mut points: Vec<[f64; WIDTH]> = Vec::with_capacity(self.samples);
            for s in 0..self.samples {
                let mut q = [0.0f64; WIDTH];
                for (r, row) in chunk.iter().enumerate() {
                    if let Some(p) = row.get(s) {
                        for (slot, value) in q.iter_mut().skip(3 * r).zip([p.x, p.y, p.z]) {
                            *slot = value;
                        }
                    }
                }
                points.push(q);
            }
            let curve = CurveN::<WIDTH>::interpolate(&params, &points, degree_v).map_err(|e| KernelError::Failed(format!("loft blend: {e}")))?;
            if knots_v.is_none() {
                knots_v = Some(curve.knots.clone());
            }
            for (s, c) in curve.ctrl.iter().enumerate() {
                for (r, xyz) in c.chunks(3).enumerate() {
                    let i = batch * ROWS_PER_SOLVE + r;
                    // Coordinates past the net's last row are this batch's own padding: their
                    // controls are not part of the surface.
                    if i >= count_u {
                        break;
                    }
                    if let [x, y, z] = *xyz {
                        let Some(slot) = s.checked_mul(count_u).and_then(|base| base.checked_add(i)).and_then(|k| ctrl.get_mut(k)) else {
                            continue;
                        };
                        *slot = HPoint::weighted(Vec3::new(x, y, z), 1.0);
                    }
                }
            }
        }
        let knots_v = knots_v.ok_or_else(|| KernelError::Failed("loft blend: no row could be interpolated".into()))?;
        NurbsSurface::from_homogeneous([self.degree, degree_v], [self.run_knots(), knots_v], ctrl)
            .map_err(|e| KernelError::Failed(format!("loft blend: {e}")))
    }
}

/// The frame at section `i`, station `s`.
fn frame(sections: &[Vec<Frame>], i: usize, s: usize) -> Result<&Frame> {
    sections.get(i).and_then(|section| section.get(s)).ok_or_else(|| KernelError::Failed("loft blend: a section is missing a station".into()))
}

/// The point of section `i` at station `s`.
fn point(sections: &[Vec<Frame>], i: usize, s: usize) -> Result<Vec3> {
    frame(sections, i, s).map(|f| f.p)
}

/// `rows[k][s] = v`, an error when the net has no such row or station.
fn put(rows: &mut [Vec<Vec3>], k: usize, s: usize, v: Vec3) -> Result<()> {
    match rows.get_mut(k).and_then(|row| row.get_mut(s)) {
        Some(slot) => {
            *slot = v;
            Ok(())
        }
        None => Err(KernelError::Failed("loft blend: the control net is too small".into())),
    }
}

/// One exact degree elevation of a Bézier control net of rows: the same curve, one control row
/// more.
fn elevate(rows: &[Vec<Vec3>]) -> Vec<Vec<Vec3>> {
    let degree = rows.len().saturating_sub(1);
    let up = degree as f64 + 1.0;
    let mut out: Vec<Vec<Vec3>> = Vec::with_capacity(rows.len() + 1);
    if let Some(first) = rows.first() {
        out.push(first.clone());
    }
    for i in 1..=degree {
        if let (Some(before), Some(here)) = (rows.get(i - 1), rows.get(i)) {
            let a = i as f64 / up;
            out.push(before.iter().zip(here).map(|(x, y)| *x * a + *y * (1.0 - a)).collect());
        }
    }
    if let Some(last) = rows.last() {
        out.push(last.clone());
    }
    out
}

/// Whether a third difference for the interior section `i` can be taken from the sections around
/// it: a stencil needs a station on one side of it and two on the other, and with three sections
/// there is none.
fn third_difference_available(i: usize, run: usize) -> bool {
    i >= 2 || i + 2 < run
}

/// The degree a family of sections asks for: the largest `c + c′ − 1` over its spans.
fn span_degree(conditions: &[usize]) -> usize {
    let mut degree = 0;
    for w in conditions.windows(2) {
        if let [a, b] = w {
            degree = degree.max(a.saturating_add(*b).saturating_sub(1));
        }
    }
    degree
}

/// The third derivative a curve needs at a boundary so that the arclength derivative of its
/// curvature vector is `target`, from its first two derivatives there. Only the normal-plane part
/// of the third derivative can hold that condition; the rest is a reparameterisation of the
/// cross-section. This is `bridge`'s own construction, kept here so the bridge core stays
/// untouched.
fn third_derivative(d1: Vec3, d2: Vec3, target: Vec3) -> Result<Vec3> {
    let sigma = d1.len();
    if !(sigma > 1e-12) || !sigma.is_finite() {
        return Err(KernelError::Failed("loft blend: no room for the curvature condition".into()));
    }
    let t = d1 * (1.0 / sigma);
    let tp = d2 * (1.0 / sigma) - d1 * (d1.dot(d2) / (sigma * sigma * sigma));
    let nn = d2 - t * d2.dot(t);
    let r = tp * (-d2.dot(t)) - t * d2.dot(tp);
    let x = (target * (sigma.powi(5)) - r * (sigma * sigma) + nn * (2.0 * d1.dot(d2))) * (1.0 / (sigma * sigma));
    if !x.is_finite() {
        return Err(KernelError::Failed("loft blend: the change of curvature is not finite".into()));
    }
    Ok(x - t * x.dot(t))
}

#[cfg(test)]
#[path = "loft_blend_tests.rs"]
mod tests;
