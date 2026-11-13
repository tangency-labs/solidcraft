//! Exact points on the fitted CAD representation, with source chart provenance.
use super::*;
use solidcore_record::Vec3;
use solidcraft_geom::{
    bspline::NurbsSurface,
    subdivision::fitting::{EdgeChart, PatchChart},
};
use std::sync::Arc;

#[derive(Clone, Debug)]
struct Piece {
    surface: Arc<NurbsSurface>,
    domain: [f64; 4],
    chart: EdgeChart,
    active: [f64; 2],
}

impl Piece {
    fn active_uv(&self) -> [[f64; 2]; 2] {
        let domain = self.surface.domain();
        self.active.map(|fraction| {
            let t = (fraction - self.chart.range[0]) / (self.chart.range[1] - self.chart.range[0]);
            std::array::from_fn(|axis| {
                let [start, end] = self.chart.uv.map(|uv| uv[axis]);
                let value = if start == end { start } else { start * (1.0 - t) + end * t };
                let unit = ((value - self.domain[axis]) / (self.domain[axis + 2] - self.domain[axis])).clamp(0.0, 1.0);
                domain[axis][0] * (1.0 - unit) + domain[axis][1] * unit
            })
        })
    }
}
/// Derived curve pieces. No display samples, document references or saved state.
#[derive(Clone, Debug)]
pub struct EdgeCurve {
    pub edge: [usize; 2],
    pieces: Vec<Piece>,
    placement: Placement,
    bounds: solidcraft_geom::Aabb3,
}
fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}
fn overlap(chart: &EdgeChart, patch: &PatchChart) -> Option<[f64; 2]> {
    if chart.face != patch.face {
        return None;
    }
    let mut range = [0.0_f64, 1.0_f64];
    for axis in 0..2 {
        let from = chart.uv[0][axis];
        let delta = chart.uv[1][axis] - from;
        let low = patch.domain[axis];
        let high = patch.domain[axis + 2];
        if !from.is_finite() || !delta.is_finite() || !low.is_finite() || !high.is_finite() || low >= high {
            return None;
        }
        if delta.abs() < 1e-15 {
            if from < low - 1e-12 || from > high + 1e-12 {
                return None;
            }
        } else {
            let a = (low - from) / delta;
            let b = (high - from) / delta;
            range[0] = range[0].max(a.min(b));
            range[1] = range[1].min(a.max(b));
        }
    }
    if range[1] - range[0] <= 1e-14 {
        return None;
    }
    let [a, b] = chart.range;
    Some(range.map(|t| a + (b - a) * t))
}
impl EdgeCurve {
    /// Exact local differentials at one logical parameter. A join can have
    /// multiple sided restrictions, which the caller must qualify together.
    pub(super) fn frame_samples(&self, fraction: f64, remaining: &mut usize) -> Result<Vec<super::selection_frame::EdgeSample>> {
        use super::selection_frame::{EdgeSample, normal, spend};
        spend(remaining, self.pieces.len()).ok_or_else(|| invalid("Form edge frame work budget exhausted"))?;
        self.pieces
            .iter()
            .filter(|piece| fraction >= piece.active[0] - 1e-12 && fraction <= piece.active[1] + 1e-12)
            .map(|piece| {
                let range = piece.chart.range[1] - piece.chart.range[0];
                if range <= 0.0 {
                    return Err(invalid("invalid Form edge frame parameter"));
                }
                let t = ((fraction - piece.chart.range[0]) / range).clamp(0.0, 1.0);
                let domain = piece.surface.domain();
                let mut uv = [0.0; 2];
                let mut derivative = [0.0; 2];
                for axis in 0..2 {
                    let lo = piece.domain[axis];
                    let hi = piece.domain[axis + 2];
                    let [a, b] = piece.chart.uv.map(|p| p[axis]);
                    let s = if a == b { a } else { a * (1.0 - t) + b * t };
                    let q = ((s - lo) / (hi - lo)).clamp(0.0, 1.0);
                    let [first, last] = domain[axis];
                    uv[axis] = (first * (1.0 - q) + last * q).clamp(first, last);
                    derivative[axis] = (b - a) / range * ((last - first) / (hi - lo));
                }
                let [a, b] = piece.surface.degree();
                let work = piece.surface.knots().iter().map(Vec::len).sum::<usize>() + 4 * (a + 1) * (b + 1);
                spend(remaining, work).ok_or_else(|| invalid("Form edge frame work budget exhausted"))?;
                let (point, du, dv) = piece.surface.derivs(uv).ok_or_else(|| invalid("Form edge frame differential is unavailable"))?;
                let normal = normal(du, dv).ok_or_else(|| invalid("Form edge frame chart is singular"))?;
                Ok(EdgeSample { chart: piece.chart.face, point, tangent: du * derivative[0] + dv * derivative[1], normal })
            })
            .collect()
    }

    /// Exact oriented rational B-spline restrictions in world homogeneous
    /// coordinates. Each range is in this logical edge's normalized parameter.
    /// Overlapping incident-chart restrictions are retained, not averaged.
    pub fn rational_pieces(&self) -> Result<Vec<([f64; 2], solidcraft_geom::bspline::CurveN<4>)>> {
        let mut remaining = usize::MAX;
        self.rational_pieces_bounded(&mut remaining)
    }
    /// Share the extractor's work budget across the actual restrictions of
    /// this logical edge. Constant parameter coordinates stay constant.
    pub fn rational_pieces_bounded(&self, remaining: &mut usize) -> Result<Vec<([f64; 2], solidcraft_geom::bspline::CurveN<4>)>> {
        self.pieces
            .iter()
            .map(|piece| {
                let uv = piece.active_uv();
                let (knots, controls) = piece.surface.parameter_line_bounded(uv[0], uv[1], remaining).map_err(|error| invalid(error.to_string()))?;
                let degree = knots.len().checked_sub(controls.len() + 1).ok_or_else(|| invalid("invalid exact guide degree"))?;
                let ctrl = controls
                    .into_iter()
                    .map(|point| {
                        let xyz = self.placement.vector(point.xyz()) + self.placement.origin * point.w;
                        [xyz.x, xyz.y, xyz.z, point.w]
                    })
                    .collect();
                Ok((piece.active, solidcraft_geom::bspline::CurveN { degree, knots, ctrl }))
            })
            .collect()
    }
    /// Prove the complete fitted rims agree under opposite boundary traversal.
    /// The caller supplies each free boundary's authoritative directed logical
    /// edge. The result only qualifies geometry; it never changes either rim.
    pub fn opposite_rim_gap(
        &self,
        directed: [usize; 2],
        other: &Self,
        other_directed: [usize; 2],
        tolerance: f64,
        remaining: &mut usize,
    ) -> Result<Option<f64>> {
        let reversed = |curve: &Self, direction: [usize; 2]| {
            let [a, b] = curve.edge;
            if a == b {
                return Err(invalid("degenerate rational weld rim"));
            }
            if direction == [a, b] {
                Ok(false)
            } else if direction == [b, a] {
                Ok(true)
            } else {
                Err(invalid("directed weld rim does not name its fitted edge"))
            }
        };
        let traversal = [reversed(self, directed)?, !reversed(other, other_directed)?];
        let first = self.rational_pieces_bounded(remaining)?;
        let second = other.rational_pieces_bounded(remaining)?;
        solidcraft_geom::bspline::rational_rim_gap(&first, &second, traversal, tolerance, remaining).map_err(|error| invalid(error.to_string()))
    }
    pub fn with_placement(&self, placement: &Placement) -> Result<Self> {
        placement.validate()?;
        let mut placed = self.clone();
        let mut corners = Vec::with_capacity(8);
        for x in [self.bounds.min.x, self.bounds.max.x] {
            for y in [self.bounds.min.y, self.bounds.max.y] {
                for z in [self.bounds.min.z, self.bounds.max.z] {
                    corners.push(placement.point(self.placement.local_point(Vec3::new(x, y, z))));
                }
            }
        }
        placed.bounds = corners.into_iter().collect();
        placed.placement = placement.clone();
        Ok(placed)
    }

    pub fn bounds(&self) -> solidcraft_geom::Aabb3 {
        self.bounds
    }
    /// Normalized source parameter; zero is the lower logical vertex ID.
    /// The fitted CAD representation is exact here; its subdivision fit residual
    /// is a separate sampled approximation report.
    pub fn at(&self, fraction: f64) -> Result<Vec3> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(invalid("an edge fraction must be finite and in 0..1"));
        }
        let mut point = None;
        for piece in self.pieces.iter().filter(|p| fraction >= p.active[0] - 1e-12 && fraction <= p.active[1] + 1e-12) {
            let [a, b] = piece.chart.range;
            if b <= a {
                return Err(invalid("source edge has an invalid parameter interval"));
            }
            let t = ((fraction - a) / (b - a)).clamp(0.0, 1.0);
            let uv = std::array::from_fn::<_, 2, _>(|axis| piece.chart.uv[0][axis] * (1.0 - t) + piece.chart.uv[1][axis] * t);
            let domain = piece.surface.domain();
            let local = std::array::from_fn::<_, 2, _>(|axis| {
                let unit = ((uv[axis] - piece.domain[axis]) / (piece.domain[axis + 2] - piece.domain[axis])).clamp(0.0, 1.0);
                domain[axis][0] * (1.0 - unit) + domain[axis][1] * unit
            });
            let next = piece.surface.eval(local[0], local[1]).map_err(|e| invalid(e.to_string()))?;
            if !next.is_finite() {
                return Err(invalid("source curve returned a nonfinite insertion target"));
            }
            if point.is_some_and(|previous: Vec3| previous.dist(next) > 1e-7) {
                return Err(invalid("fitted source edge restrictions disagree at the requested parameter"));
            }
            if point.is_none() {
                point = Some(next);
            }
        }
        let point = point.ok_or_else(|| invalid("source edge has no fitted curve at the requested parameter"))?;
        let world = self.placement.point(point);
        if !world.is_finite() {
            return Err(invalid("placed insertion target must remain finite"));
        }
        Ok(world)
    }
}
/// Build from the same fitted source as CAD conversion, including Match's saved
/// basis and original/tiled/evaluator chart choices. Resolve Match first when
/// current document state may have changed.
pub(super) fn assemble(
    edge: [usize; 2],
    charts: Vec<EdgeChart>,
    patches: &[(PatchChart, Arc<NurbsSurface>)],
    placement: &Placement,
) -> Result<EdgeCurve> {
    let mut pieces = Vec::new();
    for chart in charts {
        for (patch, surface) in patches {
            if let Some(active) = overlap(&chart, patch) {
                pieces.push(Piece { surface: surface.clone(), domain: patch.domain, chart: chart.clone(), active });
            }
        }
    }
    if pieces.is_empty() {
        return Err(invalid("logical edge has no fitted source curve"));
    }
    let mut hull = Vec::new();
    let mut remaining = 8_000_000;
    for piece in &pieces {
        let uv = piece.active_uv();
        let restriction = if let Some(fixed) = (0..2).find(|&axis| piece.chart.uv[0][axis] == piece.chart.uv[1][axis]) {
            let varying = 1 - fixed;
            let direction = if varying == 0 { solidcraft_geom::bspline::Dir::S } else { solidcraft_geom::bspline::Dir::T };
            let [a, b] = uv.map(|point| point[varying]);
            piece.surface.iso_curve_segment(direction, uv[0][fixed], [a.min(b), a.max(b)])
        } else {
            // Retained trims may follow a diagonal on the source chart. Its
            // rational control hull bounds the full route through each span.
            piece.surface.parameter_line_bounded(uv[0], uv[1], &mut remaining)
        };
        let (_, controls) = restriction.map_err(|e| invalid(e.to_string()))?;
        for control in controls {
            if control.w <= 0.0 {
                return Err(invalid("source edge hull requires positive rational weights"));
            }
            let point = placement.point(control.project().ok_or_else(|| invalid("source edge hull is nonfinite"))?);
            if !point.is_finite() {
                return Err(invalid("placed source edge hull is nonfinite"));
            }
            hull.push(point);
        }
        if hull.len() > 262_144 {
            return Err(invalid("source edge hull exceeds its control budget"));
        }
    }
    let bounds = hull.into_iter().collect();
    let curve = EdgeCurve { edge: [edge[0].min(edge[1]), edge[0].max(edge[1])], pieces, placement: placement.clone(), bounds };
    let mut spans: Vec<_> = curve.pieces.iter().map(|p| p.active).collect();
    spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut end = 0.0_f64;
    for [a, b] in spans {
        if a > end + 1e-12 {
            return Err(invalid("fitted source edge has a missing parameter interval"));
        }
        end = end.max(b);
    }
    if end < 1.0 - 1e-12 {
        return Err(invalid("fitted source edge is incomplete"));
    }
    Ok(curve)
}
pub fn edge_curves(definition: &Definition, tolerance: f64, edges: &[[usize; 2]]) -> Result<Vec<EdgeCurve>> {
    if edges.is_empty() || edges.len() > 65_536 {
        return Err(invalid("request 1..65536 bounded logical edge curves"));
    }
    validate_definition(definition)?;
    if definition.hierarchy.as_ref().is_some_and(|h| h.has_trim_wall()) {
        return Err(invalid("remove the derived trim wall before changing its source topology"));
    }
    let source = fitted_source(definition, tolerance)?;
    from_source(definition, source, edges)
}
pub(super) fn from_source(definition: &Definition, source: FittedSource, edges: &[[usize; 2]]) -> Result<Vec<EdgeCurve>> {
    if source.fit.charts.len() != source.fit.patches.len() {
        return Err(invalid("fitted source lost its chart provenance"));
    }
    let charts = source_charts(definition, edges, source.trimmed_roots)?;
    let patches = source.fit.charts.into_iter().zip(source.fit.patches.into_iter().map(Arc::new));
    curves_in_patches(definition, edges, charts, patches)
}

/// Read the requested fitted edge restrictions while retaining the source
/// sheets and their exact ownership for conversion.
pub(super) fn fitted_edge_curves(definition: &Definition, source: &FittedSource, edges: &[[usize; 2]]) -> Result<Vec<EdgeCurve>> {
    if source.fit.charts.len() != source.fit.patches.len() {
        return Err(invalid("fitted source lost its chart provenance"));
    }
    let charts = source_charts(definition, edges, source.trimmed_roots)?;
    let wanted: std::collections::BTreeSet<_> = charts.iter().flatten().map(|chart| chart.face).collect();
    let patches = source
        .fit
        .charts
        .iter()
        .zip(&source.fit.patches)
        .filter(|(chart, _)| wanted.contains(&chart.face))
        .map(|(chart, patch)| (chart.clone(), Arc::new(patch.clone())));
    curves_in_patches(definition, edges, charts, patches)
}

fn source_charts(definition: &Definition, edges: &[[usize; 2]], trimmed_roots: bool) -> Result<Vec<Vec<EdgeChart>>> {
    match &definition.hierarchy {
        Some(h) => h.skin_edge_charts_many(edges, trimmed_roots),
        None => definition.cage.edge_charts_many(edges),
    }
    .map_err(|e| invalid(e.to_string()))
}

fn curves_in_patches(
    definition: &Definition,
    edges: &[[usize; 2]],
    charts: Vec<Vec<EdgeChart>>,
    patches: impl Iterator<Item = (PatchChart, Arc<NurbsSurface>)>,
) -> Result<Vec<EdgeCurve>> {
    let mut index = std::collections::BTreeMap::<_, Vec<_>>::new();
    for (chart, surface) in patches {
        index.entry(chart.face).or_default().push((chart, surface));
    }
    edges
        .iter()
        .zip(charts)
        .map(|(&edge, charts)| {
            if edge[0] == edge[1] {
                return Err(invalid("select a nondegenerate logical edge"));
            }
            let faces: std::collections::BTreeSet<_> = charts.iter().map(|chart| chart.face).collect();
            let patches = faces.into_iter().filter_map(|face| index.get(&face)).flatten().cloned().collect::<Vec<_>>();
            assemble(edge, charts, &patches, &definition.placement)
        })
        .collect()
}
