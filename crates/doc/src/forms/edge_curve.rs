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
    for piece in &pieces {
        let uv = piece.active.map(|fraction| {
            let t = (fraction - piece.chart.range[0]) / (piece.chart.range[1] - piece.chart.range[0]);
            std::array::from_fn::<_, 2, _>(|axis| piece.chart.uv[0][axis] * (1.0 - t) + piece.chart.uv[1][axis] * t)
        });
        let varying = usize::from((uv[0][0] - uv[1][0]).abs() < 1e-14);
        let fixed = 1 - varying;
        if (uv[0][fixed] - uv[1][fixed]).abs() > 1e-12 {
            return Err(invalid("logical source edge is not an isoparametric restriction"));
        }
        let domain = piece.surface.domain();
        let parameter = |point: [f64; 2], axis: usize| {
            let unit = ((point[axis] - piece.domain[axis]) / (piece.domain[axis + 2] - piece.domain[axis])).clamp(0.0, 1.0);
            domain[axis][0] * (1.0 - unit) + domain[axis][1] * unit
        };
        let ends = uv.map(|point| parameter(point, varying));
        let direction = if varying == 0 { solidcraft_geom::bspline::Dir::S } else { solidcraft_geom::bspline::Dir::T };
        let (_, controls) = piece
            .surface
            .iso_curve_segment(direction, parameter(uv[0], fixed), [ends[0].min(ends[1]), ends[0].max(ends[1])])
            .map_err(|e| invalid(e.to_string()))?;
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
    if source.fit.charts.len() != source.fit.patches.len() {
        return Err(invalid("fitted source lost its chart provenance"));
    }
    let patches: Vec<_> = source.fit.charts.into_iter().zip(source.fit.patches.into_iter().map(Arc::new)).collect();
    let charts = match &definition.hierarchy {
        Some(h) => h.edge_charts_many(edges, source.trimmed_roots),
        None => definition.cage.edge_charts_many(edges),
    }
    .map_err(|e| invalid(e.to_string()))?;
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
