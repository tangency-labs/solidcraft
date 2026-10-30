//! Curvature combs on a body's edges and along a face's isoparametric curves, from the exact
//! geometry: each tooth stands at an even step of the curve's own parameter and carries the
//! curvature vector there, worked out from the record's derivatives (never from the display
//! mesh), so a comb does not depend on the tessellation and has as many teeth as asked for.
//!
//! An edge's tooth is its curve's curvature vector `κ N` ([`CurveEval::jet`]: lines, conics,
//! polylines, B-splines and NURBS from their first two derivatives, a traced intersection from
//! both surfaces' normal curvatures). A face's tooth along an iso line is the surface's normal
//! curvature in the line's direction times the surface normal, `κₙ n` with
//! `κₙ = n · S_tt / |S_t|²` (the second fundamental form over the first, in that direction).

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};

use crate::body::Body;
use crate::iso::IsoLine;
use crate::{KernelError, Result};

/// Fewest and most teeth along one curve.
pub const MIN_COMB_TEETH: usize = 2;
pub const MAX_COMB_TEETH: usize = 2000;

/// Samples along each edge for the first, coarse look for the edge nearest a point.
const PICK_SAMPLES: usize = 32;
/// Edges (nearest by the coarse look) whose distance is then found exactly.
const PICK_REFINED: usize = 8;
/// Golden-section steps of the exact distance: the bracket shrinks to 0.618⁹⁰ ≈ 10⁻¹⁹ of a sample step.
const PICK_STEPS: usize = 90;

/// One tooth of a comb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CombTooth {
    /// Where the tooth stands, on the curve.
    pub at: Vec3,
    /// The curvature vector there, 1/mm: towards the centre of curvature, its length the
    /// curvature (a face's: the normal curvature along the iso line, along the surface normal).
    pub curvature: Vec3,
}

fn check_density(density: usize) -> Result<()> {
    if (MIN_COMB_TEETH..=MAX_COMB_TEETH).contains(&density) {
        Ok(())
    } else {
        Err(KernelError::Invalid(format!("a comb has {MIN_COMB_TEETH}…{MAX_COMB_TEETH} teeth")))
    }
}

/// The station fractions of `density` teeth: both ends of an open curve, every step of a closed
/// one once (its end is its start).
fn fractions(density: usize, closed: bool) -> impl Iterator<Item = f64> {
    let steps = if closed { density } else { density.saturating_sub(1).max(1) };
    (0..density).map(move |k| k as f64 / steps as f64)
}

/// The edge of `body` nearest `at` and its exact distance: the distance to the edge's own curve
/// over its interval, not to a tessellation. A periodic face's own seam (an edge whose uses are
/// all on one face, the face meeting itself) and a degenerate edge (a pole) are not edges a comb
/// is drawn on. `None` when the body has no such edge.
pub fn comb_edge_near(body: &Body, at: Vec3) -> Result<Option<(f64, usize)>> {
    body.require_brep("a curvature comb")?;
    if !at.is_finite() {
        return Err(KernelError::Invalid("the point must be finite".into()));
    }
    crate::guard("curvature comb pick", || {
        let record = body.record();
        let mut coarse: Vec<(f64, usize, usize)> = Vec::new();
        for (index, edge) in record.edges.iter().enumerate() {
            let seam = edge.uses.len() >= 2 && edge.uses.iter().all(|u| edge.uses.first().is_some_and(|f| f.face == u.face));
            let Ok(curve) = CurveEval::new(edge.parameters()) else { continue };
            let [t0, t1] = edge.interval;
            if seam || curve.jet(0.5 * (t0 + t1)).is_none() {
                continue;
            }
            let best = (0..=PICK_SAMPLES)
                .filter_map(|k| curve.point(t0 + (t1 - t0) * k as f64 / PICK_SAMPLES as f64).map(|q| (q.dist(at), k)))
                .filter(|(d, _)| d.is_finite())
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((d, k)) = best {
                coarse.push((d, index, k));
            }
        }
        coarse.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut best: Option<(f64, usize)> = None;
        for &(_, index, k) in coarse.iter().take(PICK_REFINED) {
            let Some(edge) = record.edges.get(index) else { continue };
            let Ok(curve) = CurveEval::new(edge.parameters()) else { continue };
            let Some(d) = nearest_on(&curve, edge.interval, k, at) else { continue };
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, index));
            }
        }
        Ok(best)
    })
}

/// The distance from `at` to `curve` on `[t0, t1]`, by golden-section search on the sample steps
/// either side of sample `k` (the nearest of [`PICK_SAMPLES`] + 1), ends included.
fn nearest_on(curve: &CurveEval, [t0, t1]: [f64; 2], k: usize, at: Vec3) -> Option<f64> {
    let step = (t1 - t0) / PICK_SAMPLES as f64;
    let mid = t0 + step * k as f64;
    let (mut a, mut b) = ((mid - step).max(t0.min(t1)), (mid + step).min(t0.max(t1)));
    let dist = |t: f64| curve.point(t).map(|q| q.dist(at)).filter(|d| d.is_finite());
    let ratio = 0.5 * (5f64.sqrt() - 1.0);
    let mut best = dist(mid)?;
    let (mut x, mut y) = (b - (b - a) * ratio, a + (b - a) * ratio);
    let (mut fx, mut fy) = (dist(x)?, dist(y)?);
    for _ in 0..PICK_STEPS {
        if fx < fy {
            (b, y, fy) = (y, x, fx);
            x = b - (b - a) * ratio;
            fx = dist(x)?;
        } else {
            (a, x, fx) = (x, y, fy);
            y = a + (b - a) * ratio;
            fy = dist(y)?;
        }
    }
    for d in [fx, fy, dist(a)?, dist(b)?] {
        best = best.min(d);
    }
    Some(best)
}

/// The comb of edge `edge` of `body` (its index in the body's edge order): `density` teeth at
/// even steps of the edge curve's parameter over the edge, ends included (a closed edge's end is
/// its start, once), each with the curve's exact curvature vector. Stations where the curve has
/// no tangent are left out.
pub fn edge_comb(body: &Body, edge: usize, density: usize) -> Result<Vec<CombTooth>> {
    body.require_brep("a curvature comb")?;
    check_density(density)?;
    crate::guard("curvature comb", || {
        let e = body.record().edges.get(edge).ok_or_else(|| KernelError::Invalid(format!("the body has no edge {edge}")))?;
        let curve = CurveEval::new(e.parameters()).map_err(|err| KernelError::Failed(format!("curvature comb: {err}")))?;
        let [t0, t1] = e.interval;
        let closed = e.vertices[0] == e.vertices[1];
        Ok(fractions(density, closed)
            .filter_map(|f| curve.jet(t0 + (t1 - t0) * f))
            .map(|j| CombTooth { at: j.point, curvature: j.curvature })
            .collect())
    })
}

/// The comb along `line`, an isoparametric curve of face `face` of `body` ([`crate::iso_grid`]):
/// `density` teeth at even steps of the surface parameter that runs along it, ends included, each
/// the surface's normal curvature in the line's direction along its normal, from the surface's
/// exact partial derivatives. Stations at a pole (no tangent along the line) are left out.
pub fn iso_comb(body: &Body, face: usize, line: &IsoLine, density: usize) -> Result<Vec<CombTooth>> {
    body.require_brep("a curvature comb")?;
    check_density(density)?;
    crate::guard("curvature comb", || {
        let f = body.record().faces.get(face).ok_or_else(|| KernelError::Invalid(format!("the body has no face {face}")))?;
        let surface = SurfaceEval::new(f.parameters()).map_err(|err| KernelError::Failed(format!("curvature comb: {err}")))?;
        let [s0, s1] = line.span;
        let mut out = Vec::with_capacity(density);
        for frac in fractions(density, false) {
            let s = s0 + (s1 - s0) * frac;
            let [u, v] = if line.along_u { [s, line.fixed] } else { [line.fixed, s] };
            let Some(d) = surface.partials(u, v) else { continue };
            let (along, bend) = if line.along_u { (d[1][0], d[2][0]) } else { (d[0][1], d[0][2]) };
            let speed2 = along.len2();
            let Some(n) = d[1][0].cross(d[0][1]).normalized() else { continue };
            if !(speed2 > 0.0) {
                continue;
            }
            let curvature = n * (n.dot(bend) / speed2);
            if d[0][0].is_finite() && curvature.is_finite() {
                out.push(CombTooth { at: d[0][0], curvature });
            }
        }
        Ok(out)
    })
}
