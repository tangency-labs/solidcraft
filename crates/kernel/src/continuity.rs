//! Continuity diagnostics across the seams between a body's faces: do the two faces that meet
//! at an edge share a curve (G0), a tangent plane (G1) and a curvature (G2)?
//!
//! The stations are taken on the edge itself — the curve both faces are supposed to share — so
//! the numbers describe the B-rep and not a tessellation: each station is projected onto each
//! face's own surface, and the two projections are compared. The gap is the distance between
//! them (G0); the normal angle is the angle between the faces' outward normals there (G1); the
//! curvature jump is the difference of their normal curvatures in the normal section square to
//! the edge, from the second fundamental form of each surface (G2). A rolling-ball fillet is
//! tangent to the faces it joins but is not curvature continuous: where it meets a plane its
//! seam reads 0° and 1 / radius, which no chord tolerance would reveal.
//!
//! [`crate::smooth_edges`] decides whether a join needs a *display* line; this module measures
//! the join, and backs the engine's `inspect.continuity`.

use serde::Serialize;
use solidcraft_geom::Vec3;
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3};
use crate::{KernelError, Result};

/// Bodies with more edges than this are refused rather than sampled for minutes.
const MAX_EDGES: usize = 4096;
/// Most faces a body may have and still be analysed.
const MAX_FACES: usize = 50_000;
/// Stations one call may take over all its edges; edges past the budget are left out and
/// counted in [`ContinuityReport::skipped`].
const MAX_STATIONS: usize = 200_000;
/// Most edges one pick list may name (the full list always fits the station budget).
const MAX_PICKS: usize = 512;
/// Samples used to find the edge nearest a picked point (as in `surfaces::edge_near`).
const PICK_SAMPLES: usize = 32;

/// Least and most stations along one edge (`inspect.continuity`'s `samples`): three are the two
/// ends and the middle.
pub const MIN_SAMPLES: usize = 3;
pub const MAX_SAMPLES: usize = 257;

/// How an edge is picked: a point on (or near) it, or its index in the display mesh's edge
/// order ([`Body::edges`]), which is what a viewport pick carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgePick {
    Point(Vec3),
    Index(usize),
}

/// What counts as continuous across a seam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuityTolerances {
    /// Largest position gap that still counts as coincident (G0), mm.
    pub gap_mm: f64,
    /// Largest angle between the faces' normals that still counts as tangent (G1), degrees.
    pub angle_deg: f64,
    /// Largest curvature jump that still counts as curvature continuous (G2), 1/mm.
    pub curvature_per_mm: f64,
}

/// The worst value of one metric along an edge, with where it was found.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SeamWorst {
    pub value: f64,
    /// The station that gave the worst value, in the body's coordinates.
    pub at: Vec3,
    /// How far along the edge it is: 0 at the edge's start, 1 at its end.
    pub edge_fraction: f64,
}

/// Which test a seam fails, from the best continuity to the worst: [`SeamVerdict::Smooth`] is
/// within every tolerance (G2 or better), then a tangent seam whose curvature jumps (G1), then
/// a visible crease (G0 only), and finally faces that do not meet at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamVerdict {
    Smooth,
    CurvatureJump,
    Sharp,
    Gap,
}

/// One edge's diagnostics: the worst of each metric along it, and where.
#[derive(Clone, Debug, Serialize)]
pub struct SeamReport {
    /// Index in the display mesh's edge order ([`Body::edges`]).
    pub edge: usize,
    /// Length of the edge, mm.
    pub length: f64,
    /// Stations the values were taken at.
    pub samples: usize,
    pub gap_mm: SeamWorst,
    pub normal_angle_deg: SeamWorst,
    pub curvature_jump_per_mm: SeamWorst,
    /// What the tolerances make of the seam.
    pub verdict: SeamVerdict,
    /// The stations in order: what a viewport shades.
    pub points: Vec<Vec3>,
}

/// The seam diagnostics of one body.
#[derive(Clone, Debug, Serialize)]
pub struct ContinuityReport {
    pub seams: Vec<SeamReport>,
    /// Edges left out: not a seam between two faces (an open boundary, or a periodic face's own
    /// seam, which is one face meeting itself), or past the station budget.
    pub skipped: usize,
    /// Edges that could not be sampled at all: no station of theirs projected onto both faces.
    pub unmeasured: usize,
}

/// The edge indices `picks` mean, in display-mesh order and without repeats. A point pick takes
/// the edge nearest it (within the distance the kernel re-finds an edge from); an index pick
/// must be an existing index. An empty list means every two-sided seam of the body, and a pick
/// of an edge that is not a seam between two faces is refused, not guessed at.
pub fn pick_edges(body: &Body, picks: &[EdgePick]) -> Result<Vec<usize>> {
    body.require_brep("continuity")?;
    if picks.len() > MAX_PICKS {
        return Err(KernelError::Invalid(format!("pick at most {MAX_PICKS} edges")));
    }
    crate::guard("continuity picks", || {
        let (edges, adjacent) = topology(body)?;
        if picks.is_empty() {
            return Ok((0..edges.len()).filter(|i| two_sided(&adjacent, *i)).collect());
        }
        let reach = body.size() * 0.05 + 1e-6;
        let mut out: Vec<usize> = Vec::new();
        for pick in picks {
            let index = match *pick {
                EdgePick::Index(index) => index,
                EdgePick::Point(p) => nearest_edge(&edges, p, reach)?,
            };
            if !two_sided(&adjacent, index) {
                return Err(KernelError::Invalid(format!("edge {index} of this body is not a seam between two faces")));
            }
            if !out.contains(&index) {
                out.push(index);
            }
        }
        out.sort_unstable();
        Ok(out)
    })
}

/// Sample the two-sided seams `edges` of `body` (indices in display-mesh order; empty: every
/// one of them) at `samples` stations along each edge's own parameter, the ends included, with
/// the tolerances the seams are judged against.
pub fn seam_diagnostics(body: &Body, edges: &[usize], samples: usize, tolerances: ContinuityTolerances) -> Result<ContinuityReport> {
    body.require_brep("continuity")?;
    if !(MIN_SAMPLES..=MAX_SAMPLES).contains(&samples) {
        return Err(KernelError::Invalid(format!("`samples` must be {MIN_SAMPLES}…{MAX_SAMPLES}")));
    }
    for (name, value) in [("gap", tolerances.gap_mm), ("angle", tolerances.angle_deg), ("curvature", tolerances.curvature_per_mm)] {
        // A range test also throws out NaN (both comparisons are false).
        if !(0.0..1e9).contains(&value) {
            return Err(KernelError::Invalid(format!("the {name} tolerance must be a finite number, 0 or more")));
        }
    }
    crate::guard("continuity", || measure(body, edges, samples, tolerances))
}

/// The body's unique edges and the faces meeting at each of them, either under the caps.
fn topology(body: &Body) -> Result<(Vec<mt::Edge>, Vec<Vec<usize>>)> {
    if body.solid.face_iter().take(MAX_FACES + 1).count() > MAX_FACES {
        return Err(KernelError::TooLarge(format!("the body has more than {MAX_FACES} faces")));
    }
    let (edges, adjacent) = crate::body::edge_order(body.solid.face_iter());
    if edges.len() > MAX_EDGES {
        return Err(KernelError::TooLarge(format!("the body has more than {MAX_EDGES} edges")));
    }
    Ok((edges, adjacent))
}

/// Is the edge at `index` shared by (exactly) two faces of the body?
fn two_sided(adjacent: &[Vec<usize>], index: usize) -> bool {
    adjacent.get(index).is_some_and(|faces| faces.len() == 2)
}

/// The index of the edge whose curve comes nearest `p`, within `reach`.
fn nearest_edge(edges: &[mt::Edge], p: Vec3, reach: f64) -> Result<usize> {
    use mt::ParametricCurve;
    let mut best: Option<(f64, usize)> = None;
    for (index, edge) in edges.iter().enumerate() {
        let curve = edge.curve();
        let Some((t0, t1)) = curve.try_range_tuple() else { continue };
        if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
            continue;
        }
        for k in 0..=PICK_SAMPLES {
            let q = from_p3(curve.subs(t0 + (t1 - t0) * k as f64 / PICK_SAMPLES as f64));
            let d = q.dist(p);
            if d.is_finite() && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, index));
            }
        }
    }
    match best {
        Some((d, index)) if d <= reach => Ok(index),
        _ => Err(KernelError::Invalid(format!("no edge of the body is within {reach:.3} mm of ({}, {}, {})", p.x, p.y, p.z))),
    }
}

fn measure(body: &Body, edges: &[usize], samples: usize, tolerances: ContinuityTolerances) -> Result<ContinuityReport> {
    let (list, adjacent) = topology(body)?;
    let faces: Vec<&mt::Face> = body.solid.face_iter().collect();
    let mut selected: Vec<usize> = Vec::new();
    let mut skipped = 0usize;
    if edges.is_empty() {
        for index in 0..list.len() {
            if two_sided(&adjacent, index) {
                selected.push(index);
            } else {
                skipped += 1;
            }
        }
    } else {
        selected.extend(edges.iter().copied());
        selected.sort_unstable();
        selected.dedup();
        for &index in &selected {
            if index >= list.len() {
                return Err(KernelError::Invalid(format!("the body has no edge {index}")));
            }
            if !two_sided(&adjacent, index) {
                return Err(KernelError::Invalid(format!("edge {index} of this body is not a seam between two faces")));
            }
        }
    }
    let mut seams = Vec::new();
    let mut unmeasured = 0usize;
    let mut budget = MAX_STATIONS;
    for index in selected {
        let Some(face_pair) = adjacent.get(index).filter(|faces| faces.len() == 2) else { continue };
        let (Some(&first), Some(&second)) = (face_pair.first(), face_pair.get(1)) else { continue };
        let (Some(a), Some(b)) = (faces.get(first).copied(), faces.get(second).copied()) else { continue };
        let Some(edge) = list.get(index) else { continue };
        // Every edge costs `samples` stations; a run out of budget stops cleanly.
        if budget < samples {
            skipped += 1;
            continue;
        }
        budget -= samples;
        match seam(index, edge, a, b, samples, tolerances) {
            Some(report) => seams.push(report),
            None => unmeasured += 1,
        }
    }
    Ok(ContinuityReport { seams, skipped, unmeasured })
}

/// One edge's worst gap, normal angle and curvature jump.
fn seam(index: usize, edge: &mt::Edge, face_a: &mt::Face, face_b: &mt::Face, samples: usize, tolerances: ContinuityTolerances) -> Option<SeamReport> {
    use mt::ParametricCurve;
    let curve = edge.curve();
    let (t0, t1) = curve.try_range_tuple()?;
    // The station fractions: 0 at the edge's start, 1 at its end.
    let steps = samples.checked_sub(1)?;
    let fraction = |k: usize| k as f64 / steps as f64;
    let mut on_a = FaceSurface::new(face_a);
    let mut on_b = FaceSurface::new(face_b);
    let mut gap = None;
    let mut angle = None;
    let mut jump = None;
    let mut points = Vec::with_capacity(samples);
    for k in 0..samples {
        let f = fraction(k);
        let at = from_p3(curve.subs(t0 + (t1 - t0) * f));
        if !at.is_finite() {
            continue;
        }
        // Both projections and both curvatures, or the station says nothing.
        let (Some((qa, na, uv_a)), Some((qb, nb, uv_b))) = (on_a.project(at), on_b.project(at)) else { continue };
        let tangent = curve.der(t0 + (t1 - t0) * f);
        let Some(tangent) = Vec3::new(tangent.x, tangent.y, tangent.z).normalized() else { continue };
        // The normal sections square to the seam: one direction per face, in its tangent plane.
        let (Some(ka), Some(kb)) = (on_a.normal_curvature(uv_a, na, across(na, tangent)), on_b.normal_curvature(uv_b, nb, across(nb, tangent)))
        else {
            continue;
        };
        // The faces' outward normals agree on a smooth seam; where they are opposed (a knife
        // edge) the curvatures are compared in one frame, since bending away from one face is
        // bending towards the other.
        let signed = if na.dot(nb) < 0.0 { ka + kb } else { ka - kb };
        points.push(at);
        keep(&mut gap, qa.dist(qb), at, f);
        keep(&mut angle, na.dot(nb).clamp(-1.0, 1.0).acos().to_degrees(), at, f);
        keep(&mut jump, signed.abs(), at, f);
    }
    let (gap, angle, jump) = (gap?, angle?, jump?);
    let verdict = if gap.value > tolerances.gap_mm {
        SeamVerdict::Gap
    } else if angle.value > tolerances.angle_deg {
        SeamVerdict::Sharp
    } else if jump.value > tolerances.curvature_per_mm {
        SeamVerdict::CurvatureJump
    } else {
        SeamVerdict::Smooth
    };
    let length = points.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.dist(*b))).sum();
    Some(SeamReport {
        edge: index,
        length,
        samples: points.len(),
        gap_mm: gap,
        normal_angle_deg: angle,
        curvature_jump_per_mm: jump,
        verdict,
        points,
    })
}

/// The direction in which a face's curvature is read at a seam: across the edge, square to it,
/// inside the face's tangent plane (a zero vector where the edge runs along the normal, which
/// leaves that station unmeasured).
fn across(normal: Vec3, tangent: Vec3) -> Vec3 {
    normal.cross(tangent)
}

/// Keep the largest value seen, with where it was found.
fn keep(worst: &mut Option<SeamWorst>, value: f64, at: Vec3, edge_fraction: f64) {
    if value.is_finite() && worst.is_none_or(|w| value > w.value) {
        *worst = Some(SeamWorst { value, at, edge_fraction });
    }
}

/// A face's surface, kept over the stations of one edge so each search can start at the last
/// answer (stations run along the edge, so the parameters move smoothly).
struct FaceSurface {
    surface: mt::Surface,
    hint: Option<(f64, f64)>,
}

impl FaceSurface {
    /// The face's own outward-facing surface (`oriented_surface`: a reversed face's surface is
    /// inverted, so its normal is the material's outward normal).
    fn new(face: &mt::Face) -> Self {
        Self { surface: face.oriented_surface(), hint: None }
    }

    /// The nearest point of the surface to `p`, the outward unit normal there and the
    /// parameters. `None` where the search or the normal is unusable.
    fn project(&mut self, p: Vec3) -> Option<(Vec3, Vec3, (f64, f64))> {
        use mt::{ParametricSurface, ParametricSurface3D, SearchNearestParameter};
        let uv = self.surface.search_nearest_parameter(p3(p), self.hint, 100).or_else(|| self.surface.search_nearest_parameter(p3(p), None, 100))?;
        if !(uv.0.is_finite() && uv.1.is_finite()) {
            return None;
        }
        let q = from_p3(self.surface.subs(uv.0, uv.1));
        let n = self.surface.normal(uv.0, uv.1);
        if !q.is_finite() {
            return None;
        }
        let normal = Vec3::new(n.x, n.y, n.z).normalized()?;
        self.hint = Some(uv);
        Some((q, normal, uv))
    }

    /// The normal curvature of the face at `uv` along `direction` (a unit vector of its tangent
    /// plane), from the second fundamental form. A surface point `r(u, v)` with unit normal `n`
    /// crossed that way has curvature `κ = n · r''` of the curve whose tangent there is
    /// `direction`: a plane reads 0 and a cylinder of radius `R`, across its axis, −1 / R
    /// (bending towards its axis). `None` at a degenerate parameter point (a cone's apex), where
    /// the surface has no curvature.
    fn normal_curvature(&self, uv: (f64, f64), normal: Vec3, direction: Vec3) -> Option<f64> {
        use mt::ParametricSurface;
        let ru = vec3(self.surface.uder(uv.0, uv.1));
        let rv = vec3(self.surface.vder(uv.0, uv.1));
        let (ru_len, rv_len) = (ru.len(), rv.len());
        if !(ru_len > 0.0 && rv_len > 0.0) {
            return None;
        }
        // Solve the direction in the unit basis (r_u / |r_u|, r_v / |r_v|): the curve then runs
        // its parameters at du/ds = α / |r_u| and dv/ds = β / |r_v|, and
        // `r'' = r_uu (du/ds)² + 2 r_uv (du/ds)(dv/ds) + r_vv (dv/ds)²`.
        let cos = (ru.dot(rv) / (ru_len * rv_len)).clamp(-1.0, 1.0);
        let sin2 = 1.0 - cos * cos;
        if !(sin2 > 1e-12) {
            return None;
        }
        let (cu, cv) = (direction.dot(ru) / ru_len, direction.dot(rv) / rv_len);
        let (du, dv) = ((cu - cv * cos) / sin2 / ru_len, (cv - cu * cos) / sin2 / rv_len);
        if !(du.is_finite() && dv.is_finite()) {
            return None;
        }
        // `direction` is a unit vector: divide by the speed the parameters imply (they only miss
        // it by rounding), so κ does not depend on how the surface is parameterized.
        let speed = (ru * du + rv * dv).len();
        if !(speed > 1e-6) {
            return None;
        }
        let second = normal.dot(
            vec3(self.surface.uuder(uv.0, uv.1)) * (du * du)
                + vec3(self.surface.uvder(uv.0, uv.1)) * (2.0 * du * dv)
                + vec3(self.surface.vvder(uv.0, uv.1)) * (dv * dv),
        );
        (second / (speed * speed)).is_finite().then_some(second / (speed * speed))
    }
}

/// A kernel point as a vector.
fn vec3(p: mt::Vector3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::mm;
    use solidcraft_geom::Plane;

    fn tolerances() -> ContinuityTolerances {
        ContinuityTolerances { gap_mm: 1e-3, angle_deg: 1.0, curvature_per_mm: 0.01 }
    }

    fn box_body() -> Body {
        crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap()
    }

    fn report(body: &Body) -> ContinuityReport {
        seam_diagnostics(body, &[], 33, tolerances()).unwrap()
    }

    #[test]
    fn a_box_edge_is_a_gap_free_right_angle() {
        let body = box_body();
        let r = report(&body);
        assert_eq!(r.seams.len(), 12, "a box has twelve edges");
        assert_eq!(r.skipped, 0);
        for seam in &r.seams {
            assert!(seam.gap_mm.value < 1e-6, "gap {}", seam.gap_mm.value);
            assert!((seam.normal_angle_deg.value - 90.0).abs() < 1e-9, "angle {}", seam.normal_angle_deg.value);
            assert!(seam.curvature_jump_per_mm.value < 1e-9, "jump {}", seam.curvature_jump_per_mm.value);
            assert_eq!(seam.verdict, SeamVerdict::Sharp);
            assert_eq!(seam.samples, 33);
            assert!((seam.length - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_fillet_seam_is_tangent_with_the_fillets_curvature_jump() {
        let body = box_body();
        let rounded = crate::fillet(&body, &[Vec3::new(10.0, 10.0, 5.0)], mm(4.0)).unwrap();
        let r = report(&rounded);
        // The rounded edge is replaced by the blend face: two tangent seams along it, the two
        // arcs where the blend meets the ends of the edge, and the box's other eleven edges.
        assert_eq!(r.seams.len(), 15);
        let tangent: Vec<&SeamReport> = r.seams.iter().filter(|s| s.normal_angle_deg.value < 0.01).collect();
        assert_eq!(tangent.len(), 2, "the fillet meets its two neighbours tangentially");
        for seam in &tangent {
            assert!(seam.gap_mm.value < 1e-6);
            assert!(seam.curvature_jump_per_mm.value.is_finite());
            assert!((seam.curvature_jump_per_mm.value - 0.25).abs() < 1e-6, "1 / radius, got {}", seam.curvature_jump_per_mm.value);
            assert_eq!(seam.verdict, SeamVerdict::CurvatureJump);
        }
        assert_eq!(r.seams.iter().filter(|s| s.verdict == SeamVerdict::Sharp).count(), 13);
    }

    #[test]
    fn a_line_tangent_into_an_arc_reports_one_over_its_radius() {
        use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};
        let v = |x: f64, y: f64| Vec2::new(x, y);
        let outer = Loop2 {
            segs: vec![
                Seg2::Arc { center: v(10.0, 0.0), radius: 5.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
                Seg2::Line { a: v(10.0, 5.0), b: v(-10.0, 5.0) },
                Seg2::Arc { center: v(-10.0, 0.0), radius: 5.0, start: std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
                Seg2::Line { a: v(-10.0, -5.0), b: v(10.0, -5.0) },
            ],
        };
        let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 3.0).unwrap().remove(0);
        let r = report(&body);
        let joins: Vec<&SeamReport> = r.seams.iter().filter(|s| s.normal_angle_deg.value < 0.01).collect();
        assert_eq!(joins.len(), 4, "a slot has four line-to-arc joins");
        for seam in joins {
            assert!(
                seam.curvature_jump_per_mm.value > 0.19 && seam.curvature_jump_per_mm.value < 0.21,
                "1 / 5, got {}",
                seam.curvature_jump_per_mm.value
            );
        }
    }

    #[test]
    fn picks_refuse_what_is_not_there() {
        let body = box_body();
        let all = pick_edges(&body, &[]).unwrap();
        assert_eq!(all.len(), 12);
        // The edge up the corner at (0, 0), beside a point in the middle of a face.
        let picked = pick_edges(&body, &[EdgePick::Point(Vec3::new(0.0, 0.0, 5.0)), EdgePick::Index(all[0])]).unwrap();
        assert_eq!(picked.len(), 2);
        assert!(pick_edges(&body, &[EdgePick::Index(999)]).is_err());
        assert!(pick_edges(&body, &[EdgePick::Point(Vec3::new(0.0, 5.0, 5.0))]).is_err(), "a point on a face is not on an edge");
        assert!(pick_edges(&body, &[EdgePick::Point(Vec3::new(500.0, 0.0, 0.0))]).is_err());
        assert!(seam_diagnostics(&body, &[], 2, tolerances()).is_err());
        assert!(seam_diagnostics(&body, &[], 258, tolerances()).is_err());
        assert!(seam_diagnostics(&body, &[], 33, ContinuityTolerances { gap_mm: f64::NAN, ..tolerances() }).is_err());
        assert!(seam_diagnostics(&body, &[999], 33, tolerances()).is_err());
    }
}
