//! Candidate geometry for automatic centre marks. This does not draw marks or decide which
//! features belong together: callers must first establish that the arcs are on the same rim.

use std::f64::consts::TAU;

use solidcraft_geom::{Plane, Seg2, Vec2};

use super::dimension::{Lookup, ModelEdge, Status, check_edge, edge_candidates};
use super::view::MAX_MARK_EXCLUSIONS;
use crate::name_lookup::BodyEdges;
use crate::{DocError, Document, MAX_PLACEMENT_DEPTH, Model, ModelBody, Result};

/// Maximum number of arc pieces considered for one candidate rim.
pub const MAX_RIM_ARCS: usize = 10_000;

/// Whether angular spans cover a complete rim, allowing at most `tolerance` radians in any gap.
/// Each pair is `(start, sweep)`, in radians, with negative sweeps running clockwise. The caller
/// establishes that every span is on the same circle; this function checks angular coverage only.
/// Invalid values, excessive input, an empty set, and sweeps exceeding one turn are refused.
/// The union is measured rather than the sum, so overlapping arcs cannot fabricate coverage.
///
/// `tolerance` must be finite, nonnegative and less than half a turn. Geometry-specific choice
/// of tolerance remains with the caller.
pub fn full_rim(spans: &[(f64, f64)], tolerance: f64) -> bool {
    if spans.is_empty() || spans.len() > MAX_RIM_ARCS || !tolerance.is_finite() || !(0.0..TAU / 2.0).contains(&tolerance) {
        return false;
    }
    let mut intervals = Vec::with_capacity(spans.len().saturating_mul(2));
    for &(start, sweep) in spans {
        if !start.is_finite() || !sweep.is_finite() || sweep == 0.0 || sweep.abs() > TAU + tolerance {
            return false;
        }
        let width = sweep.abs().min(TAU);
        let first = (start.rem_euclid(TAU) + if sweep < 0.0 { -width } else { 0.0 }).rem_euclid(TAU);
        let end = first + width;
        intervals.push((first, end.min(TAU)));
        if end > TAU {
            intervals.push((0.0, end - TAU));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let Some(&(first, mut end)) = intervals.first() else { return false };
    for &(start, next_end) in intervals.iter().skip(1) {
        if start - end > tolerance {
            return false;
        }
        end = end.max(next_end);
    }
    first + (TAU - end) <= tolerance
}

/// An exclusion looked up against one model evaluation. `edge_index` is valid only for that
/// evaluation and is never saved. A split name is ambiguous rather than silently choosing a piece.
#[derive(Clone, Debug, PartialEq)]
pub struct ExclusionResolution {
    pub status: Status,
    pub edge_index: Option<usize>,
    /// Whether the resolved edge's name follows verified history.
    pub exact: bool,
}

/// Resolve exclusions together, sharing each body's naming lookup. Lost, suppressed and ambiguous
/// entries remain in the returned list in the same order as the saved references. A missing edge
/// is not replaced with a nearby one. The caller must separately decide how an excluded edge
/// relates to a group of rims.
pub fn resolve_exclusions(doc: &Document, model: &Model, exclusions: &[ModelEdge]) -> Result<Vec<ExclusionResolution>> {
    if exclusions.len() > MAX_MARK_EXCLUSIONS {
        return Err(DocError::Invalid(format!("a view has at most {MAX_MARK_EXCLUSIONS} centre mark exclusions")));
    }
    exclusions.iter().try_for_each(check_edge)?;
    let mut lookup = Lookup::new(doc, model);
    Ok(exclusions
        .iter()
        .map(|edge| match lookup.edges(edge) {
            Ok(found) if !found.whole => {
                ExclusionResolution { status: Status::Ambiguous(edge_candidates(&edge.body, &found)), edge_index: None, exact: false }
            }
            Ok(found) => match found.pieces.first() {
                Some((index, _)) => ExclusionResolution { status: Status::Ok, edge_index: Some(*index), exact: found.exact },
                None => {
                    ExclusionResolution { status: Status::Lost(format!("edge `{}` no longer exists", edge.name)), edge_index: None, exact: false }
                }
            },
            Err(status) => ExclusionResolution { status, edge_index: None, exact: false },
        })
        .collect())
}

/// The maximum edges read from one body in a single automatic-mark scan.
pub const MAX_SCANNED_EDGES: usize = 100_000;

/// One named, face-on circular edge before complete rims or features have been grouped. Indices
/// and adjacent faces belong to this evaluation only; `edge` is the persistent association.
#[derive(Clone, Debug, PartialEq)]
pub struct CircularEdge {
    pub edge: ModelEdge,
    pub index: usize,
    pub faces: Vec<u32>,
    pub center: Vec2,
    pub radius: f64,
    /// Signed model-space distance from the view plane, distinguishing front and back rims.
    pub depth: f64,
    pub spans: Vec<(f64, f64)>,
    pub exact: bool,
}

/// Read one placed body's circular edges from the exact planar-edge facility, independently of
/// visibility. `body` must be the placed body in the same world frame as `basis`; `path` is its
/// occurrence path and is saved in each name. No index from this result may be persisted. This
/// finds candidate arcs only: it does not infer complete rims, features, or exclusions.
/// Mesh bodies and a body beyond the work limit are reported instead of silently guessed.
pub fn circular_edges(body: &ModelBody, path: &[u64], basis: &Plane) -> Result<Vec<CircularEdge>> {
    if path.len() > MAX_PLACEMENT_DEPTH || path.contains(&0) {
        return Err(DocError::Invalid(format!("an occurrence path is at most {MAX_PLACEMENT_DEPTH} occurrences, none of them 0")));
    }
    if body.body.is_mesh() {
        return Err(DocError::Invalid("automatic centre marks cannot read a mesh body's circles".into()));
    }
    if !basis.origin.is_finite()
        || !basis.x.is_finite()
        || !basis.y.is_finite()
        || (basis.x.len2() - 1.0).abs() > 1e-9
        || (basis.y.len2() - 1.0).abs() > 1e-9
        || basis.x.dot(basis.y).abs() > 1e-9
    {
        return Err(DocError::Invalid("the view basis is not finite and orthonormal".into()));
    }
    let named = BodyEdges::new(body);
    let count = named.mesh.edges.len();
    if count > MAX_SCANNED_EDGES {
        return Err(DocError::Invalid(format!("a body has more than {MAX_SCANNED_EDGES} edges to scan for centre marks")));
    }
    let indices: Vec<usize> = (0..count).collect();
    let projected = body.body.planar_edges(basis, &indices);
    let mut out = Vec::new();
    for (index, segments) in indices.into_iter().zip(projected) {
        let Some(segments) = segments else { continue };
        let Some(Seg2::Arc { center, radius, .. }) = segments.first().copied() else { continue };
        if !center.is_finite() || !radius.is_finite() || radius <= 0.0 || segments.len() > MAX_RIM_ARCS {
            continue;
        }
        let mut spans = Vec::with_capacity(segments.len());
        for segment in &segments {
            let Seg2::Arc { center: c, radius: r, start, sweep } = *segment else {
                spans.clear();
                break;
            };
            if !c.is_finite()
                || !r.is_finite()
                || !start.is_finite()
                || !sweep.is_finite()
                || c.dist(center) > radius * 1e-9
                || (r - radius).abs() > radius * 1e-9
                || sweep == 0.0
            {
                spans.clear();
                break;
            }
            spans.push((start, sweep));
        }
        if spans.is_empty() {
            continue;
        }
        let Some(point) = named.mesh.edges.get(index).and_then(|line| line.first()) else { continue };
        let depth = basis.height(*point);
        if !depth.is_finite() {
            continue;
        }
        let Some(name) = named.name(index) else { continue };
        let edge = ModelEdge { body: body.definition_name().to_string(), origin_feature: body.feature, name: name.to_string(), path: path.to_vec() };
        check_edge(&edge)?;
        let faces = named.mesh.edge_faces.get(index).cloned().unwrap_or_default();
        out.push(CircularEdge { edge, index, faces, center, radius, depth, spans, exact: named.verified(index) });
    }
    Ok(out)
}

/// Maximum circular edges accepted by one grouping operation. Grouping performs pairwise
/// topological comparisons, so its input and total span count are bounded separately from scanning.
pub const MAX_GROUP_EDGES: usize = 2048;
const MAX_GROUP_SPANS: usize = 100_000;
const MAX_EDGE_FACES: usize = 64;
const RIM_GAP_RADIANS: f64 = 1e-6;

/// A candidate automatic mark, derived for this evaluation. The source edges are persistent
/// associations, but this grouping is never written to the drawing file.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkCandidate {
    pub center: Vec2,
    pub radius: f64,
    pub sources: Vec<ModelEdge>,
    pub exact: bool,
}

#[derive(Clone)]
struct Rim {
    center: Vec2,
    radius: f64,
    owner: ModelEdge,
    sources: Vec<ModelEdge>,
    faces: Vec<u32>,
    exact: bool,
}

fn root(parents: &[usize], mut i: usize) -> usize {
    while let Some(&next) = parents.get(i) {
        if next == i {
            break;
        }
        i = next;
    }
    i
}

fn join(parents: &mut [usize], a: usize, b: usize) {
    let (a, b) = (root(parents, a), root(parents, b));
    if a != b
        && let Some(slot) = parents.get_mut(b)
    {
        *slot = a;
    }
}

fn same_owner(a: &ModelEdge, b: &ModelEdge) -> bool {
    a.body == b.body && a.origin_feature == b.origin_feature && a.path == b.path
}

fn close_length(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-7 + a.abs().max(b.abs()) * 1e-9
}

fn same_center(a: Vec2, b: Vec2, radius: f64) -> bool {
    a.dist(b) <= 1e-7 + radius * 1e-9
}

fn shares_face(a: &[u32], b: &[u32]) -> bool {
    let (mut i, mut j) = (0, 0);
    while let (Some(left), Some(right)) = (a.get(i), b.get(j)) {
        match left.cmp(right) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}

fn source_order(a: &ModelEdge, b: &ModelEdge) -> std::cmp::Ordering {
    (&a.body, a.origin_feature, &a.path, &a.name).cmp(&(&b.body, b.origin_feature, &b.path, &b.name))
}

/// Group face-on arcs conservatively. Split edges first form a complete rim only when they share
/// a face and have the same circle and depth. Complete coaxial rims then join only through shared
/// model faces, within the same placed body; projected coincidence alone never joins features.
/// The largest rim determines the paper size. Tolerances are provisional pending issue #9462's
/// interface decision. Invalid or excessively large input is refused rather than guessed.
pub fn group_candidates(edges: &[CircularEdge]) -> Result<Vec<MarkCandidate>> {
    if edges.len() > MAX_GROUP_EDGES {
        return Err(DocError::Invalid(format!("more than {MAX_GROUP_EDGES} automatic-mark edges to group")));
    }
    let mut span_count = 0usize;
    for edge in edges {
        check_edge(&edge.edge)?;
        span_count = span_count.saturating_add(edge.spans.len());
        if !edge.center.is_finite()
            || !edge.radius.is_finite()
            || edge.radius <= 0.0
            || !edge.depth.is_finite()
            || edge.faces.len() > MAX_EDGE_FACES
            || edge.spans.is_empty()
            || edge.spans.len() > MAX_RIM_ARCS
            || edge
                .spans
                .iter()
                .any(|(start, sweep)| !start.is_finite() || !sweep.is_finite() || *sweep == 0.0 || sweep.abs() > TAU + RIM_GAP_RADIANS)
            || span_count > MAX_GROUP_SPANS
        {
            return Err(DocError::Invalid("invalid or excessive automatic-mark candidate geometry".into()));
        }
    }
    let mut ordered = edges.to_vec();
    for edge in &mut ordered {
        edge.faces.sort_unstable();
        edge.faces.dedup();
    }
    ordered.sort_by(|a, b| source_order(&a.edge, &b.edge).then(a.index.cmp(&b.index)));
    let mut parents: Vec<usize> = (0..ordered.len()).collect();
    for (i, a) in ordered.iter().enumerate() {
        for (j, b) in ordered.iter().enumerate().skip(i + 1) {
            if same_owner(&a.edge, &b.edge)
                && same_center(a.center, b.center, a.radius.max(b.radius))
                && close_length(a.radius, b.radius)
                && (a.depth - b.depth).abs() <= 1e-7 + a.radius.max(b.radius) * 1e-9
                && shares_face(&a.faces, &b.faces)
            {
                join(&mut parents, i, j);
            }
        }
    }
    let mut components = std::collections::BTreeMap::<usize, Vec<&CircularEdge>>::new();
    for (i, edge) in ordered.iter().enumerate() {
        components.entry(root(&parents, i)).or_default().push(edge);
    }
    let mut rims = Vec::new();
    for members in components.into_values() {
        let Some(first) = members.first() else { continue };
        let spans: Vec<_> = members.iter().flat_map(|edge| edge.spans.iter().copied()).collect();
        if spans.len() > MAX_RIM_ARCS {
            return Err(DocError::Invalid(format!("a rim has more than {MAX_RIM_ARCS} arcs")));
        }
        if !full_rim(&spans, RIM_GAP_RADIANS) {
            continue;
        }
        let mut sources: Vec<_> = members.iter().map(|edge| edge.edge.clone()).collect();
        sources.sort_by(source_order);
        sources.dedup();
        let mut faces: Vec<_> = members.iter().flat_map(|edge| edge.faces.iter().copied()).collect();
        faces.sort_unstable();
        faces.dedup();
        rims.push(Rim {
            center: first.center,
            radius: first.radius,
            owner: first.edge.clone(),
            sources,
            faces,
            exact: members.iter().all(|edge| edge.exact),
        });
    }
    let mut parents: Vec<usize> = (0..rims.len()).collect();
    for (i, a) in rims.iter().enumerate() {
        for (j, b) in rims.iter().enumerate().skip(i + 1) {
            if same_owner(&a.owner, &b.owner) && same_center(a.center, b.center, a.radius.max(b.radius)) && shares_face(&a.faces, &b.faces) {
                join(&mut parents, i, j);
            }
        }
    }
    let mut components = std::collections::BTreeMap::<usize, Vec<&Rim>>::new();
    for (i, rim) in rims.iter().enumerate() {
        components.entry(root(&parents, i)).or_default().push(rim);
    }
    let mut marks = Vec::new();
    for members in components.into_values() {
        let Some(largest) = members.iter().max_by(|a, b| a.radius.total_cmp(&b.radius).then_with(|| source_order(&a.owner, &b.owner))) else {
            continue;
        };
        let mut sources: Vec<_> = members.iter().flat_map(|rim| rim.sources.iter().cloned()).collect();
        sources.sort_by(source_order);
        sources.dedup();
        marks.push(MarkCandidate { center: largest.center, radius: largest.radius, sources, exact: members.iter().all(|rim| rim.exact) });
    }
    marks.sort_by(|a, b| {
        a.sources
            .first()
            .zip(b.sources.first())
            .map_or(std::cmp::Ordering::Equal, |(a, b)| source_order(a, b))
            .then_with(|| a.center.x.total_cmp(&b.center.x))
            .then_with(|| a.center.y.total_cmp(&b.center.y))
            .then_with(|| a.radius.total_cmp(&b.radius))
    });
    Ok(marks)
}
