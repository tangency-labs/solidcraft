//! Derived guide routing on the native quilt, preserving original rim provenance.
use super::*;
use kernel::history::{BodyHistory, EdgeOrigin, History};
use solidcore_record::brep;
use solidcraft_geom::{
    brep::{eval::CurveEval, pcurve},
    tess::uv,
};

fn unsupported(reason: &str) -> DocError {
    invalid(format!("not supported yet: co-sampled Surface quilt: {reason}"))
}

struct Atlas<'a> {
    patch: &'a kernel::Body,
    source_edges: Vec<Option<usize>>,
    surfaces: Vec<kernel::FaceGeometry>,
    domains: Vec<uv::Domain>,
    starts: Vec<uv::Starts>,
}
#[derive(Clone, Copy)]
struct Node {
    face: usize,
    uv: [f64; 2],
    point: Vec3,
    anchor: Option<kernel::DisplayGuidePoint>,
}

pub(in crate::forms) fn display(
    patch: &kernel::Body,
    open: &kernel::Body,
    source: &kernel::Body,
    edges: &[[usize; 2]],
    rim: &QuiltRim,
    samples: &[Vec<Vec3>],
    tolerance: f64,
) -> Result<kernel::GuidedMesh> {
    let (pieces, groups) = route(patch, open, source, edges, rim, samples)?;
    let mut sampled = patch.mesh_with_guides(&kernel::MeshTolerance::chord(tolerance.min(0.05)), &pieces)?;
    let mut guides = Vec::with_capacity(groups.len());
    for group in groups {
        let mut curve = Vec::new();
        for id in group {
            let piece = sampled.guides.get(id).ok_or_else(|| unsupported("lost a routed guide piece"))?;
            let first = *piece.first().ok_or_else(|| unsupported("empty routed guide piece"))?;
            if let Some(&last) = curve.last() {
                if last != first {
                    return Err(unsupported("adjacent charts did not retain one canonical station"));
                }
                curve.extend(piece.iter().skip(1).copied());
            } else {
                curve.extend(piece.iter().copied());
            }
        }
        guides.push(curve);
    }
    sampled.guides = guides;
    Ok(sampled)
}

fn route(
    patch: &kernel::Body,
    open: &kernel::Body,
    source: &kernel::Body,
    edges: &[[usize; 2]],
    rim: &QuiltRim,
    samples: &[Vec<Vec3>],
) -> Result<(Vec<kernel::DisplayGuide>, Vec<Vec<usize>>)> {
    if edges.len() != samples.len() || patch.face_count() > 64 || edges.len() > 544 {
        return Err(unsupported("guide/chart budget exhausted"));
    }
    let surfaces = (0..patch.face_count()).map(|face| kernel::face_geometry(patch, face)).collect::<kernel::Result<Vec<_>>>()?;
    let domains: Vec<_> = surfaces.iter().map(|face| uv::domain(&face.surface)).collect();
    if domains.iter().any(|domain| domain.range.iter().any(Option::is_none) || domain.period.iter().any(Option::is_some)) {
        return Err(unsupported("requires bounded nonperiodic native charts"));
    }
    let starts = surfaces.iter().map(|_| uv::Starts::default()).collect();
    let patch_history = patch.history().ok_or_else(|| unsupported("missing patch rim history over the open source"))?;
    let open_history = open.history().ok_or_else(|| unsupported("missing open rim history over the original source"))?;
    let source_edges = rim_sources(patch, open, source, patch_history, open_history)?;
    let atlas = Atlas { patch, source_edges, surfaces, domains, starts };
    let mut pieces = Vec::new();
    let mut groups = Vec::with_capacity(edges.len());
    let mut trace_budget = 4096usize;
    for (&edge, samples) in edges.iter().zip(samples) {
        let mut group = Vec::new();
        if let Some((start, span)) = rim.arc(edge)? {
            atlas.perimeter(source, rim, edge, start, span, &mut pieces, &mut group)?;
        } else {
            let mut nodes = Vec::with_capacity(samples.len());
            for (index, &point) in samples.iter().enumerate() {
                let control = if index == 0 {
                    Some(edge[0])
                } else if index + 1 == samples.len() {
                    Some(edge[1])
                } else {
                    None
                };
                let anchor = control.map(|control| atlas.fixed_station(source, rim, control)).transpose()?.flatten();
                nodes.push(match anchor {
                    Some(anchor) => atlas.anchored(point, anchor)?,
                    None => atlas.identify(point)?,
                });
            }
            for pair in nodes.windows(2) {
                let (a, b) = (
                    *pair.first().ok_or_else(|| unsupported("missing guide station"))?,
                    *pair.get(1).ok_or_else(|| unsupported("missing next guide station"))?,
                );
                atlas.trace(a, b, 0, &mut trace_budget, &mut pieces, &mut group)?;
            }
        }
        groups.push(group);
    }
    Ok((pieces, groups))
}

/// Identity comes only from the two operations' checked edge origins. A native
/// record round trip may round a homogeneous coefficient; it must still retain
/// the source's own parameter basis and bounded interval through both steps.
pub(in crate::forms) fn rim_sources(
    patch: &kernel::Body,
    open: &kernel::Body,
    source: &kernel::Body,
    patch_history: &History,
    open_history: &History,
) -> Result<Vec<Option<usize>>> {
    patch_history.check(patch, &[open])?;
    open_history.check(open, &[source])?;
    let origin = |history: &History, id: usize| -> Result<usize> {
        let entry = history.edges.get(id).ok_or_else(|| unsupported("missing native rim edge origin"))?;
        let [EdgeOrigin::Edge { operand: 0, edge }] = entry.origins.as_slice() else {
            return Err(unsupported("native rim requires one original operand edge"));
        };
        Ok(*edge)
    };
    patch
        .record()
        .edges
        .iter()
        .enumerate()
        .map(|(id, edge)| {
            if edge.uses.len() != 1 {
                return Ok(None);
            }
            let open_id = origin(patch_history, id)?;
            let source_id = origin(open_history, open_id)?;
            let middle = open.record().edges.get(open_id).ok_or_else(|| unsupported("missing open rim edge"))?;
            let original = source.record().edges.get(source_id).ok_or_else(|| unsupported("missing original rim edge"))?;
            if !retained_parameters(original, middle) || !retained_parameters(middle, edge) {
                return Err(unsupported("native rim history changed its original intrinsic parameterization"));
            }
            Ok(Some(source_id))
        })
        .collect()
}

/// This certifies a parameter correspondence AFTER history chose the edge; it
/// never associates nearby curves. Identical positive weights and knots retain
/// the same oriented rational basis. Only xyz coefficient roundoff from native
/// homogeneous -> Euclidean -> homogeneous record conversion is accepted.
/// A division and multiplication convert each coefficient; the fixed relative
/// eight-epsilon bound covers small native record conversions, independently
/// of fitting tolerance. It never chooses the edge's identity.
pub(in crate::forms) fn retained_parameters(original: &brep::Edge, edge: &brep::Edge) -> bool {
    if !original.interval.iter().chain(&edge.interval).all(|value| value.is_finite())
        || original.interval[0] >= original.interval[1]
        || edge.interval[0] >= edge.interval[1]
        || edge.interval[0] < original.interval[0]
        || edge.interval[1] > original.interval[1]
    {
        return false;
    }
    if original.parameters() == edge.parameters() {
        return true;
    }
    let (brep::Curve::RationalSpline { knots: a_knots, homogeneous: a }, brep::Curve::RationalSpline { knots: b_knots, homogeneous: b }) =
        (original.parameters(), edge.parameters())
    else {
        return false;
    };
    a_knots == b_knots
        && a_knots.iter().all(|value| value.is_finite())
        && a.len() >= 2
        && a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a[3].is_finite()
                && a[3] > 0.0
                && a[3] == b[3]
                && a[..3]
                    .iter()
                    .zip(&b[..3])
                    .all(|(&a, &b)| a.is_finite() && b.is_finite() && (a - b).abs() <= 8.0 * f64::EPSILON * a.abs().max(b.abs()))
        })
}

fn push(pieces: &mut Vec<kernel::DisplayGuide>, group: &mut Vec<usize>, face: usize, points: Vec<kernel::DisplayGuidePoint>) -> Result<()> {
    if points.windows(2).all(|pair| pair[0] == pair[1]) {
        return Ok(());
    }
    if let Some(&id) = group.last()
        && let Some(piece) = pieces.get_mut(id)
        && piece.face as usize == face
        && piece.points.last() == points.first()
    {
        piece.points.extend(points.into_iter().skip(1));
        return Ok(());
    }
    if pieces.len() >= 20_000 {
        return Err(unsupported("routed piece budget exhausted"));
    }
    group.push(pieces.len());
    pieces.push(kernel::DisplayGuide { face: u32::try_from(face).map_err(|_| unsupported("face index overflow"))?, points });
    Ok(())
}

impl Atlas<'_> {
    /// Subdivide a derived display path when one sample interval crosses more
    /// than one chart sector. Identity at joins always comes from native edges
    /// or a certified shared native vertex, never from a nearby trim guess.
    fn trace(&self, a: Node, b: Node, depth: u8, budget: &mut usize, pieces: &mut Vec<kernel::DisplayGuide>, group: &mut Vec<usize>) -> Result<()> {
        if a.face == b.face {
            return push(pieces, group, a.face, vec![self.station(a)?, self.station(b)?]);
        }
        let record = self.patch.record();
        let adjacent = record
            .edges
            .iter()
            .filter(|edge| edge.uses.iter().any(|use_| use_.face as usize == a.face) && edge.uses.iter().any(|use_| use_.face as usize == b.face))
            .count();
        if adjacent == 1 {
            if let Ok((edge, parameter)) = self.transition(a, b) {
                let seam = kernel::DisplayGuidePoint::Edge { edge, parameter };
                push(pieces, group, a.face, vec![self.station(a)?, seam])?;
                return push(pieces, group, b.face, vec![seam, self.station(b)?]);
            }
        } else if adjacent == 0
            && let Some((left, right)) = self.vertex_transition(a, b)?
        {
            push(pieces, group, a.face, vec![self.station(a)?, left])?;
            return push(pieces, group, b.face, vec![right, self.station(b)?]);
        }
        if depth >= 16 {
            return Err(unsupported("chart tracing exceeded its bounded subdivision depth"));
        }
        *budget = budget.checked_sub(1).ok_or_else(|| unsupported("chart tracing exhausted its station budget"))?;
        let query = a.point * 0.5 + b.point * 0.5;
        let mut candidates = Vec::with_capacity(self.surfaces.len());
        for face in 0..self.surfaces.len() {
            let node = self.project(face, query, None)?;
            candidates.push((node.point.dist(query), node));
        }
        let &(distance, best) =
            candidates.iter().min_by(|a, b| a.0.total_cmp(&b.0)).ok_or_else(|| unsupported("no finite chart tracing projection"))?;
        for &(other_distance, other) in &candidates {
            let error =
                4.0 * self.rounding(best)?.max(self.rounding(other)?) + 64.0 * f64::EPSILON * (query.len() + best.point.len() + other.point.len());
            if (distance - other_distance).abs() <= error && best.point.dist(other.point) > error {
                return Err(unsupported("chart tracing has equally close distinct surface branches"));
            }
        }
        let middle = self.identify(best.point)?;
        if middle.point == a.point || middle.point == b.point {
            return Err(unsupported("chart tracing cannot make finite progress"));
        }
        self.trace(a, middle, depth + 1, budget, pieces, group)?;
        self.trace(middle, b, depth + 1, budget, pieces, group)
    }
    fn vertex_transition(&self, a: Node, b: Node) -> Result<Option<(kernel::DisplayGuidePoint, kernel::DisplayGuidePoint)>> {
        let record = self.patch.record();
        let vertices = |face: usize| -> Result<std::collections::BTreeSet<u32>> {
            let face = record.faces.get(face).ok_or_else(|| unsupported("missing vertex transition face"))?;
            let mut out = std::collections::BTreeSet::new();
            for use_ in face.loops.iter().flat_map(|loop_| match loop_ {
                brep::Loop::Edges { edges } => edges.as_slice(),
                _ => &[],
            }) {
                let edge = record.edges.get(use_.edge as usize).ok_or_else(|| unsupported("missing vertex transition edge"))?;
                out.extend(edge.vertices);
            }
            Ok(out)
        };
        let (left, right) = (vertices(a.face)?, vertices(b.face)?);
        let common: Vec<_> = left.intersection(&right).copied().collect();
        let [vertex] = common.as_slice() else { return Ok(None) };
        let native = record.vertices.get(*vertex as usize).ok_or_else(|| unsupported("missing transition vertex"))?;
        let point = Vec3::new(native.point[0], native.point[1], native.point[2]);
        let (left, right, query) = self.transition_feet(a, b)?;
        let bound = 16.0 * f64::EPSILON.sqrt() * a.point.dist(b.point) + 4.0 * self.rounding(left)?.max(self.rounding(right)?);
        if left.point.dist(point) > bound || right.point.dist(point) > bound || left.point.dist(right.point) > bound {
            return Ok(None);
        }
        // Distant sectors can both clamp to their common centre while an
        // intervening chart is actually closer to this off-centre query. That
        // requires tracing those sectors, not a shortcut through the vertex.
        let vertex_distance = point.dist(query);
        for face in 0..self.surfaces.len() {
            let candidate = self.project(face, query, None)?;
            let distance = candidate.point.dist(query);
            if distance + bound < vertex_distance || distance <= vertex_distance + bound && candidate.point.dist(point) > bound {
                return Ok(None);
            }
        }
        Ok(Some((self.vertex_station(a.face, *vertex)?, self.vertex_station(b.face, *vertex)?)))
    }
    fn vertex_station(&self, face: usize, vertex: u32) -> Result<kernel::DisplayGuidePoint> {
        let record = self.patch.record();
        let source = record.vertices.get(vertex as usize).ok_or_else(|| unsupported("missing canonical transition vertex"))?;
        let point = Vec3::new(source.point[0], source.point[1], source.point[2]);
        let face_record = record.faces.get(face).ok_or_else(|| unsupported("missing transition vertex face"))?;
        let mut candidates = Vec::new();
        for use_ in face_record.loops.iter().flat_map(|loop_| match loop_ {
            brep::Loop::Edges { edges } => edges.as_slice(),
            _ => &[],
        }) {
            let edge = record.edges.get(use_.edge as usize).ok_or_else(|| unsupported("missing incident transition edge"))?;
            for (endpoint, &id) in edge.vertices.iter().enumerate() {
                if id == vertex {
                    let parameter = edge.interval[endpoint];
                    let uv = pcurve::eval(&use_.pcurve, parameter).ok_or_else(|| unsupported("transition vertex pcurve failed"))?;
                    candidates.push((uv, kernel::DisplayGuidePoint::Edge { edge: use_.edge, parameter }));
                }
            }
        }
        let &(uv, station) = candidates.first().ok_or_else(|| unsupported("transition vertex is not incident to its chart"))?;
        if candidates.iter().any(|&(other, _)| other != uv) {
            return Err(unsupported("transition vertex has ambiguous chart coordinates"));
        }
        let surface = self.surfaces.get(face).ok_or_else(|| unsupported("missing transition vertex chart"))?;
        let node = Node { face, uv, point: surface.surface.eval(uv[0], uv[1]).0, anchor: Some(station) };
        if node.point.dist(point) > self.rounding(node)? {
            return Err(unsupported("transition vertex pcurve disagrees with its canonical position"));
        }
        Ok(station)
    }
    fn transition_feet(&self, a: Node, b: Node) -> Result<(Node, Node, Vec3)> {
        let (mut low, mut high) = (0.0, 1.0);
        let (mut left, mut right) = (a, b);
        let mut query = a.point;
        for _ in 0..56 {
            let t = 0.5 * (low + high);
            query = a.point * (1.0 - t) + b.point * t;
            left = self.project(a.face, query, Some(left.uv))?;
            right = self.project(b.face, query, Some(right.uv))?;
            if left.point.dist(query) <= right.point.dist(query) {
                low = t;
            } else {
                high = t;
            }
        }
        Ok((left, right, query))
    }
    /// All incident guides reuse the immutable logical control's ORIGINAL rim
    /// parameter. Projecting its world position back through a chart would lose
    /// this identity by a rounding step and create conflicting fixed UV nodes.
    fn fixed_station(&self, source: &kernel::Body, rim: &QuiltRim, control: usize) -> Result<Option<kernel::DisplayGuidePoint>> {
        let Some(&fraction) = rim.coordinates.get(&control) else { return Ok(None) };
        let coordinate = rim.support.coordinate(fraction)?;
        let (source_id, parameter) = rim.support.original_parameter(&coordinate)?;
        let original = source.record().edges.get(source_id).ok_or_else(|| unsupported("missing fixed control source curve"))?;
        let record = self.patch.record();
        let mut candidates = Vec::new();
        for (id, edge) in record
            .edges
            .iter()
            .enumerate()
            .filter(|(id, edge)| edge.uses.len() == 1 && self.source_edges.get(*id).copied().flatten() == Some(source_id))
        {
            if let Some(parameter) = rim_parameter(original, parameter, edge.interval) {
                candidates.push((id, parameter));
            }
        }
        let &(first, parameter) = candidates.first().ok_or_else(|| unsupported("fixed control has no original native rim interval"))?;
        if candidates.len() > 1 {
            let vertex = |id: usize, t| -> Option<u32> {
                let edge = record.edges.get(id)?;
                if t == edge.interval[0] {
                    Some(edge.vertices[0])
                } else if t == edge.interval[1] {
                    Some(edge.vertices[1])
                } else {
                    None
                }
            };
            let first_vertex = vertex(first, parameter).ok_or_else(|| unsupported("fixed control has ambiguous native rim intervals"))?;
            if candidates.iter().any(|&(id, t)| vertex(id, t) != Some(first_vertex)) {
                return Err(unsupported("fixed control intervals do not share its original native vertex"));
            }
        }
        Ok(Some(kernel::DisplayGuidePoint::Edge { edge: u32::try_from(first).map_err(|_| unsupported("edge index overflow"))?, parameter }))
    }
    fn anchored(&self, point: Vec3, anchor: kernel::DisplayGuidePoint) -> Result<Node> {
        let kernel::DisplayGuidePoint::Edge { edge, parameter } = anchor else {
            return Err(unsupported("fixed control anchor requires a native edge"));
        };
        let native = self.patch.record().edges.get(edge as usize).ok_or_else(|| unsupported("missing anchored edge"))?;
        let [use_] = native.uses.as_slice() else { return Err(unsupported("fixed rim anchor requires one outer face")) };
        let face = use_.face as usize;
        let record = self.patch.record().faces.get(face).ok_or_else(|| unsupported("missing anchored face"))?;
        let mut uses = record
            .loops
            .iter()
            .flat_map(|loop_| match loop_ {
                brep::Loop::Edges { edges } => edges.as_slice(),
                _ => &[],
            })
            .filter(|use_| use_.edge == edge);
        let use_ = uses.next().ok_or_else(|| unsupported("missing anchored pcurve"))?;
        if uses.next().is_some() {
            return Err(unsupported("ambiguous anchored pcurve"));
        }
        let uv = pcurve::eval(&use_.pcurve, parameter).ok_or_else(|| unsupported("anchored pcurve failed"))?;
        let surface = self.surfaces.get(face).ok_or_else(|| unsupported("missing anchored chart"))?;
        let on = surface.surface.eval(uv[0], uv[1]).0;
        let mut node = Node { face, uv, point: on, anchor: Some(anchor) };
        if !point.is_finite() || point.dist(on) > self.rounding(node)? {
            return Err(unsupported("fixed logical control disagrees with its original native rim station"));
        }
        node.point = point;
        Ok(node)
    }
    fn perimeter(
        &self,
        source: &kernel::Body,
        rim: &QuiltRim,
        logical: [usize; 2],
        start: f64,
        span: f64,
        pieces: &mut Vec<kernel::DisplayGuide>,
        group: &mut Vec<usize>,
    ) -> Result<()> {
        let record = self.patch.record();
        let mut native = Vec::new();
        let mut stations: Vec<(f64, Option<(usize, f64)>)> = rim.stations(logical)?.into_iter().map(|t| (t, None)).collect();
        for (id, edge) in record.edges.iter().enumerate().filter(|(_, edge)| edge.uses.len() == 1) {
            let source_id = self.source_edges.get(id).copied().flatten().ok_or_else(|| unsupported("missing outer edge's original source"))?;
            rim.support
                .original_coordinate(source_id, edge.interval[0])
                .map_err(|_| unsupported("outer quilt edge does not belong to its original support rim"))?;
            native.push((id, source_id));
            for &parameter in &edge.interval {
                let coordinate = rim.support.original_coordinate(source_id, parameter)?;
                let fraction = rim.support.fraction(&coordinate)?;
                let distance = if span > 0.0 { (fraction - start).rem_euclid(1.0) } else { -(start - fraction).rem_euclid(1.0) };
                let t = distance / span;
                if t > 0.0 && t < 1.0 {
                    stations.push((t, Some((source_id, parameter))));
                }
            }
        }
        if stations.len() > 4096 {
            return Err(unsupported("fixed rim station budget exhausted"));
        }
        stations.sort_by(|a, b| a.0.total_cmp(&b.0));
        stations.dedup_by(|a, b| {
            if a.0 == b.0 {
                if b.1.is_none() {
                    b.1 = a.1;
                }
                true
            } else {
                false
            }
        });
        let coordinate_at = |t: f64| -> Result<(usize, f64)> {
            let fraction = if t == 0.0 {
                *rim.coordinates.get(&logical[0]).ok_or_else(|| unsupported("missing original rim start control"))?
            } else if t == 1.0 {
                *rim.coordinates.get(&logical[1]).ok_or_else(|| unsupported("missing original rim end control"))?
            } else {
                (start + t * span).rem_euclid(1.0)
            };
            let coordinate = rim.support.coordinate(fraction)?;
            rim.support.original_parameter(&coordinate)
        };
        for pair in stations.windows(2) {
            let &(low, low_exact) = pair.first().ok_or_else(|| unsupported("missing rim station"))?;
            let &(high, high_exact) = pair.get(1).ok_or_else(|| unsupported("missing next rim station"))?;
            let (source_id, parameter) = coordinate_at(0.5 * low + 0.5 * high)?;
            let candidates: Vec<_> = native
                .iter()
                .filter(|&&(id, original)| {
                    original == source_id && record.edges.get(id).is_some_and(|edge| parameter > edge.interval[0] && parameter < edge.interval[1])
                })
                .collect();
            let [candidate] = candidates.as_slice() else { return Err(unsupported("rim segment does not have one intrinsic native interval")) };
            let (id, _) = **candidate;
            let edge = record.edges.get(id).ok_or_else(|| unsupported("missing native rim edge"))?;
            let parameter_on_edge = |t, exact: Option<(usize, f64)>| -> Result<f64> {
                let (original, parameter) = exact.map_or_else(|| coordinate_at(t), Ok)?;
                let value = original_parameter_on(source, original, parameter, source_id)?;
                let source_edge = source.record().edges.get(source_id).ok_or_else(|| unsupported("missing exact source rim edge"))?;
                rim_parameter(source_edge, value, edge.interval).ok_or_else(|| unsupported(&format!(
                    "rim station escaped its known native interval: logical={logical:?}, arc=[{start:.17e},{span:.17e}], segment=[{low:.17e},{high:.17e}], t={t:.17e}, exact={exact:?}, original={original}, parameter={parameter:.17e}, source={source_id}, native={id}, value={value:.17e}, interval={:?}, source_interval={:?}, source_vertices={:?}",
                    edge.interval,source_edge.interval,source_edge.vertices
                )))
            };
            let (a, b) = (parameter_on_edge(low, low_exact)?, parameter_on_edge(high, high_exact)?);
            if a == b {
                continue;
            }
            let use_ = edge.uses.first().ok_or_else(|| unsupported("missing outer edge face"))?;
            let edge_id = u32::try_from(id).map_err(|_| unsupported("edge index overflow"))?;
            push(
                pieces,
                group,
                use_.face as usize,
                vec![
                    kernel::DisplayGuidePoint::Edge { edge: edge_id, parameter: a },
                    kernel::DisplayGuidePoint::Edge { edge: edge_id, parameter: b },
                ],
            )?;
        }
        Ok(())
    }
    fn project(&self, face: usize, point: Vec3, hint: Option<[f64; 2]>) -> Result<Node> {
        let geometry = self.surfaces.get(face).ok_or_else(|| unsupported("missing native chart"))?;
        let domain = self.domains.get(face).ok_or_else(|| unsupported("missing native chart domain"))?;
        let starts = self.starts.get(face).ok_or_else(|| unsupported("missing native projection starts"))?;
        let parameters =
            uv::project_with(&geometry.surface, domain, starts, point, hint, f64::INFINITY).ok_or_else(|| unsupported("chart projection failed"))?;
        let point = geometry.surface.eval(parameters[0], parameters[1]).0;
        if !point.is_finite() {
            return Err(unsupported("chart projection was nonfinite"));
        }
        Ok(Node { face, uv: parameters, point, anchor: None })
    }
    fn rounding(&self, node: Node) -> Result<f64> {
        let geometry = self.surfaces.get(node.face).ok_or_else(|| unsupported("missing chart"))?;
        let domain = self.domains.get(node.face).ok_or_else(|| unsupported("missing domain"))?;
        let (_, du, dv) = geometry.surface.derivs(node.uv[0], node.uv[1]).ok_or_else(|| unsupported("chart derivatives failed"))?;
        let scale = [0, 1].map(|d| domain.range[d].map_or(node.uv[d].abs(), |(a, b)| a.abs().max(b.abs()).max(b - a)));
        let rounding = 1024.0 * f64::EPSILON * (node.point.len() + du.len() * scale[0] + dv.len() * scale[1]);
        if !rounding.is_finite() || !du.is_finite() || !dv.is_finite() {
            return Err(unsupported("nonfinite chart identity bound"));
        }
        Ok(rounding)
    }
    fn identify(&self, point: Vec3) -> Result<Node> {
        let mut candidates = Vec::new();
        for face in 0..self.surfaces.len() {
            let node = self.project(face, point, None)?;
            let distance = node.point.dist(point);
            if distance <= self.rounding(node)? {
                candidates.push(node);
            }
        }
        let first = *candidates.first().ok_or_else(|| unsupported("guide station has no roundoff-qualified chart owner"))?;
        if candidates.len() > 1 {
            let kernel::DisplayGuidePoint::Edge { edge, parameter } = self.station(first)? else {
                return Err(unsupported("coincident charts do not have one topological boundary station"));
            };
            let record = self.patch.record();
            let first_edge = record.edges.get(edge as usize).ok_or_else(|| unsupported("missing shared station edge"))?;
            let vertex = |edge: &brep::Edge, t| {
                if t == edge.interval[0] {
                    Some(edge.vertices[0])
                } else if t == edge.interval[1] {
                    Some(edge.vertices[1])
                } else {
                    None
                }
            };
            for &candidate in candidates.iter().skip(1) {
                let kernel::DisplayGuidePoint::Edge { edge: other, parameter: t } = self.station(candidate)? else {
                    return Err(unsupported("overlapping chart interiors do not establish an owner"));
                };
                let other_edge = record.edges.get(other as usize).ok_or_else(|| unsupported("missing coincident station edge"))?;
                let same_vertex = vertex(first_edge, parameter).is_some() && vertex(first_edge, parameter) == vertex(other_edge, t);
                let scale = parameter.abs().max(t.abs()).max(first_edge.interval[1] - first_edge.interval[0]);
                let same_edge = edge == other && (parameter - t).abs() <= 64.0 * f64::EPSILON * scale;
                if !same_vertex && !same_edge {
                    return Err(unsupported("coincident chart owners do not share the same native edge or vertex station"));
                }
            }
        }
        Ok(first)
    }
    fn station(&self, node: Node) -> Result<kernel::DisplayGuidePoint> {
        if let Some(anchor) = node.anchor {
            return Ok(anchor);
        }
        let face = self.patch.record().faces.get(node.face).ok_or_else(|| unsupported("missing face record"))?;
        let [brep::Loop::Edges { edges }] = face.loops.as_slice() else { return Err(unsupported("requires one chart loop")) };
        let mut boundary = Vec::new();
        for use_ in edges {
            let parameter = edge_parameter(&use_.pcurve, node.uv)?;
            let edge = self.patch.record().edges.get(use_.edge as usize).ok_or_else(|| unsupported("missing edge record"))?;
            if parameter < edge.interval[0] || parameter > edge.interval[1] {
                continue;
            }
            let on = CurveEval::new(edge.parameters())
                .map_err(|error| unsupported(&error.to_string()))?
                .point(parameter)
                .ok_or_else(|| unsupported("edge does not evaluate"))?;
            if on.dist(node.point) <= self.rounding(node)? {
                boundary.push((use_.edge, parameter));
            }
        }
        if let Some(&(edge, parameter)) = boundary.first() {
            // At a true corner several uses share the same native vertex. The
            // fixed prefix uses that vertex's exact point on every incident face.
            for &(other, t) in boundary.iter().skip(1) {
                let record = self.patch.record();
                let a = record.edges.get(edge as usize).ok_or_else(|| unsupported("missing edge"))?;
                let b = record.edges.get(other as usize).ok_or_else(|| unsupported("missing edge"))?;
                let vertex = |edge: &brep::Edge, t| {
                    if t == edge.interval[0] {
                        Some(edge.vertices[0])
                    } else if t == edge.interval[1] {
                        Some(edge.vertices[1])
                    } else {
                        None
                    }
                };
                if vertex(a, parameter).is_none() || vertex(a, parameter) != vertex(b, t) {
                    return Err(unsupported("ambiguous boundary station"));
                }
            }
            return Ok(kernel::DisplayGuidePoint::Edge { edge, parameter });
        }
        Ok(kernel::DisplayGuidePoint::Surface(node.uv))
    }
    fn transition(&self, a: Node, b: Node) -> Result<(u32, f64)> {
        let record = self.patch.record();
        let adjacent: Vec<_> = record
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                edge.uses.iter().any(|use_| use_.face as usize == a.face) && edge.uses.iter().any(|use_| use_.face as usize == b.face)
            })
            .collect();
        let [(edge_id, edge)] = adjacent.as_slice() else { return Err(unsupported("chart transition does not have one exact shared edge")) };
        let use_for = |face: usize| -> Result<&brep::OrientedEdge> {
            let face = record.faces.get(face).ok_or_else(|| unsupported("missing transition face"))?;
            let mut uses = face
                .loops
                .iter()
                .flat_map(|loop_| match loop_ {
                    brep::Loop::Edges { edges } => edges.as_slice(),
                    _ => &[],
                })
                .filter(|use_| use_.edge as usize == *edge_id);
            let use_ = uses.next().ok_or_else(|| unsupported("missing transition pcurve"))?;
            if uses.next().is_some() {
                return Err(unsupported("ambiguous transition pcurve"));
            }
            Ok(use_)
        };
        let (left_use, right_use) = (use_for(a.face)?, use_for(b.face)?);
        let (left, right, _) = self.transition_feet(a, b)?;
        let (lp, rp) = (edge_parameter(&left_use.pcurve, left.uv)?, edge_parameter(&right_use.pcurve, right.uv)?);
        let parameter = (0.5 * lp + 0.5 * rp).clamp(edge.interval[0], edge.interval[1]);
        let point = CurveEval::new(edge.parameters())
            .map_err(|error| unsupported(&error.to_string()))?
            .point(parameter)
            .ok_or_else(|| unsupported("transition curve failed"))?;
        let objective_rounding = 16.0 * f64::EPSILON.sqrt() * a.point.dist(b.point) + 4.0 * self.rounding(left)?.max(self.rounding(right)?);
        if left.point.dist(right.point) > objective_rounding
            || left.point.dist(point) > objective_rounding
            || right.point.dist(point) > objective_rounding
        {
            return Err(unsupported("projection transition does not approach its known shared edge continuously"));
        }
        for (face, use_) in [(a.face, left_use), (b.face, right_use)] {
            let parameters = pcurve::eval(&use_.pcurve, parameter).ok_or_else(|| unsupported("transition pcurve failed"))?;
            let geometry = self.surfaces.get(face).ok_or_else(|| unsupported("missing transition chart"))?;
            let candidate = Node { face, uv: parameters, point: geometry.surface.eval(parameters[0], parameters[1]).0, anchor: None };
            if candidate.point.dist(point) > self.rounding(candidate)? {
                return Err(unsupported("exact transition pcurves disagree on the canonical edge station"));
            }
        }
        Ok((u32::try_from(*edge_id).map_err(|_| unsupported("edge index overflow"))?, parameter))
    }
}

/// A source corner can have two exact edge-coordinate descriptions. Transfer
/// only by the original shared vertex ID, never by a nearest world position.
fn original_parameter_on(source: &kernel::Body, from: usize, parameter: f64, to: usize) -> Result<f64> {
    if from == to {
        return Ok(parameter);
    }
    let record = source.record();
    let a = record.edges.get(from).ok_or_else(|| unsupported("missing original corner edge"))?;
    let b = record.edges.get(to).ok_or_else(|| unsupported("missing next original corner edge"))?;
    let vertex = if parameter == a.interval[0] {
        a.vertices[0]
    } else if parameter == a.interval[1] {
        a.vertices[1]
    } else {
        return Err(unsupported("source curve changed away from an exact corner"));
    };
    if vertex == b.vertices[0] {
        Ok(b.interval[0])
    } else if vertex == b.vertices[1] {
        Ok(b.interval[1])
    } else {
        Err(unsupported("source rim coordinates do not share a native vertex"))
    }
}

/// The two exact endpoints of one intrinsically closed source edge name the
/// same original vertex. A split native interval can require the other endpoint
/// description. No interior parameter, open edge, or nearby value is rewritten.
pub(in crate::forms) fn rim_parameter(source: &brep::Edge, value: f64, interval: [f64; 2]) -> Option<f64> {
    if !value.is_finite() || !interval.iter().all(|value| value.is_finite()) || interval[0] >= interval[1] {
        return None;
    }
    if value >= interval[0] && value <= interval[1] {
        return Some(value);
    }
    if source.vertices[0] != source.vertices[1] {
        return None;
    }
    let alternative = if value == source.interval[0] {
        source.interval[1]
    } else if value == source.interval[1] {
        source.interval[0]
    } else {
        return None;
    };
    (alternative.is_finite() && (alternative == interval[0] || alternative == interval[1])).then_some(alternative)
}

/// Coordinate on an already identified linear pcurve. This never chooses an edge.
fn edge_parameter(curve: &brep::Curve2, point: [f64; 2]) -> Result<f64> {
    let brep::Curve2::Spline { knots, points } = curve else { return Err(unsupported("requires linear chart pcurves")) };
    let [a, b] = points.as_slice() else { return Err(unsupported("requires two-point chart pcurves")) };
    if knots.len() != 4 {
        return Err(unsupported("requires degree-one pcurves"));
    }
    let [low, high] = curve.interval().ok_or_else(|| unsupported("invalid pcurve interval"))?;
    let [dx, dy] = [b[0] - a[0], b[1] - a[1]];
    let length = dx * dx + dy * dy;
    if !length.is_finite() || length <= 0.0 {
        return Err(unsupported("degenerate pcurve"));
    }
    let fraction = ((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length;
    Ok(if fraction == 0.0 {
        low
    } else if fraction == 1.0 {
        high
    } else {
        low + fraction * (high - low)
    })
}
