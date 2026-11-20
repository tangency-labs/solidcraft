//! A new exact-rim cap candidate, prepared without changing the editable Form.
use super::{Definition, FittedSource, Placement, convert_source, edge_curve};
use crate::kernel::history::{EdgeOrigin, FaceOrigin};
use crate::{DocError, Result, kernel};
use solidcore_record::brep::Loop;
use solidcraft_geom::bspline::NurbsSurface;
use solidcraft_geom::fill_iso::{QuadCapPiece, quad_cap};
use solidcraft_geom::subdivision::hierarchy::QuadCapRim;
use std::collections::{BTreeMap, BTreeSet};

const CORNERS: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Qualified geometry and ownership for a candidate Form snapshot.
pub struct PreparedQuadCap {
    pub face: usize,
    pub rim: QuadCapRim,
    pub surface: NurbsSurface,
    pub body: kernel::Body,
    pub face_owners: Vec<usize>,
}

/// Construct a new definition with a replayable cap and validate the exact
/// sewn CAD body before returning it to the authoring caller.
pub fn author_quad_cap(definition: &Definition, tolerance: f64, edges: &[[usize; 2]], vertices: &[usize]) -> Result<Definition> {
    let (candidate, _) = cap_definition(definition, tolerance, edges, vertices)?;
    super::convert(&candidate, tolerance)?;
    Ok(candidate)
}

/// Preview the same cap definition and native qualification used by authoring.
/// The body uses the Form's placement while its cap tensor stays local.
pub fn prepare_quad_cap(definition: &Definition, tolerance: f64, edges: &[[usize; 2]], vertices: &[usize]) -> Result<PreparedQuadCap> {
    let (candidate, face) = cap_definition(definition, tolerance, edges, vertices)?;
    let (body, report, source) = convert_source(&candidate, tolerance)?;
    let cap = source.quad_caps.into_iter().find(|cap| cap.face == face).ok_or_else(|| invalid("missing fitted quad cap"))?;
    Ok(PreparedQuadCap { face: cap.face, rim: cap.rim, surface: cap.surface, body, face_owners: report.patch_roots })
}

fn cap_definition(definition: &Definition, tolerance: f64, edges: &[[usize; 2]], vertices: &[usize]) -> Result<(Definition, usize)> {
    if !(1e-6..=1.0).contains(&tolerance) {
        return Err(invalid("quad cap fit tolerance must be 0.000001..1 mm"));
    }
    if definition.matching.is_some() || definition.profile.is_some() || definition.symmetry.is_some() {
        return Err(invalid("quad cap preparation requires an independent Form without symmetry"));
    }
    let mut out = definition.clone();
    let hierarchy = out.hierarchy.as_mut().ok_or_else(|| invalid("quad cap preparation requires a refined Form"))?;
    let face = hierarchy.reserve_quad_cap(edges, vertices).map_err(invalid)?;
    Ok((out, face))
}

pub(super) struct FittedQuadCap {
    pub(super) face: usize,
    rim: QuadCapRim,
    pub(super) surface: NurbsSurface,
    operand: usize,
    edges: CapEdges,
}

enum CapEdges {
    Single([usize; 4]),
    Atlas { edge_sides: BTreeMap<usize, usize>, faces: usize },
}

impl FittedQuadCap {
    pub(super) fn append(definition: &Definition, source: &mut FittedSource, face: usize, rim: QuadCapRim) -> Result<()> {
        let edges = rim.sides.each_ref().map(|side| side.edge);
        let exact = edge_curve::fitted_edge_curves(definition, source, &edges)?;
        let mut remaining = 2_000_000;
        let curves = exact
            .iter()
            .zip(&edges)
            .map(|(curve, &[a, b])| -> Result<Vec<QuadCapPiece>> {
                let mut pieces = curve.with_placement(&Placement::default())?.rational_pieces_bounded(&mut remaining)?;
                pieces.sort_by(|(a, _), (b, _)| a[0].total_cmp(&b[0]));
                if a > b {
                    pieces.reverse();
                    for (range, curve) in &mut pieces {
                        *range = [1.0 - range[1], 1.0 - range[0]];
                        curve.knots.reverse();
                        for knot in &mut curve.knots {
                            *knot = 1.0 - *knot;
                        }
                        curve.ctrl.reverse();
                    }
                }
                Ok(pieces)
            })
            .collect::<Result<Vec<_>>>()?;
        let curves: [Vec<QuadCapPiece>; 4] = curves.try_into().map_err(|_| invalid("missing exact quad cap side"))?;
        let surface = quad_cap(curves.each_ref().map(Vec::as_slice)).map_err(invalid)?;
        // Zero allows the exact pole atlas and excludes a sampled chart fit.
        let (sheet, deviation) = kernel::fitted_nurbs_region_sheet(&surface, &[CORNERS.to_vec()], 0.0)?;
        if deviation != 0.0 {
            return Err(invalid("quad cap chart must preserve the exact cap surface"));
        }
        let edges = if sheet.face_count() == 1 {
            CapEdges::Single(cap_edge_sides(&sheet, &surface)?)
        } else {
            CapEdges::Atlas { edge_sides: atlas_edge_sides(&sheet, &surface)?, faces: sheet.face_count() }
        };
        let operand = source.sheets.len();
        if source.fit.patch_roots.contains(&face) {
            return Err(invalid("quad cap face slot collides with the retained skin"));
        }
        source.sheets.push(sheet);
        source.quad_caps.push(Self { face, rim, surface, operand, edges });
        Ok(())
    }
}

pub(super) fn qualify(caps: &[FittedQuadCap], body: &kernel::Body, face_owners: &[usize], source_owners: &[usize], closed: bool) -> Result<()> {
    if face_owners.iter().copied().collect::<BTreeSet<_>>() != source_owners.iter().copied().collect() {
        return Err(invalid("quad cap sewing lost a logical face owner"));
    }
    if body.is_surface() == closed {
        return Err(invalid("quad caps did not retain the expected open or closed topology"));
    }
    for cap in caps {
        if cap.rim.closes_skin != closed {
            return Err(invalid("quad cap opening count disagrees with the logical surface"));
        }
        match &cap.edges {
            CapEdges::Single(edge_sides) => check_cap_seams(body, face_owners, source_owners, cap, edge_sides)?,
            CapEdges::Atlas { edge_sides, faces } => check_atlas_seams(body, face_owners, source_owners, cap, edge_sides, *faces)?,
        }
    }
    let validity = kernel::validity(body);
    if !validity.ok() {
        return Err(invalid(format!("quad caps failed native validity: {validity:?}")));
    }
    Ok(())
}

// The original unit corners are exact anchors on the native rim.
fn cap_corners(surface: &NurbsSurface) -> Result<[[f64; 3]; 4]> {
    let mut corners = [[0.0; 3]; 4];
    for (index, [u, v]) in CORNERS.into_iter().enumerate() {
        let p = surface.eval(u, v).map_err(invalid)?;
        let point = [p.x, p.y, p.z];
        if !p.is_finite() || corners[..index].contains(&point) {
            return Err(invalid("quad cap needs four distinct finite corners"));
        }
        corners[index] = point;
    }
    Ok(corners)
}

// Planar support changes only the support and pcurves.
fn cap_edge_sides(sheet: &kernel::Body, surface: &NurbsSurface) -> Result<[usize; 4]> {
    let corners = cap_corners(surface)?;
    let record = sheet.record();
    let [face] = record.faces.as_slice() else { return Err(invalid("quad cap needs one source face")) };
    let [Loop::Edges { edges }] = face.loops.as_slice() else { return Err(invalid("quad cap needs one source wire")) };
    let mut sides = [None; 4];
    for edge_use in edges {
        let edge = record.edges.get(edge_use.edge as usize).ok_or_else(|| invalid("quad cap lost a source edge"))?;
        let [a, b] = if edge_use.forward { edge.vertices } else { [edge.vertices[1], edge.vertices[0]] };
        let a = record.vertices.get(a as usize).ok_or_else(|| invalid("quad cap lost an edge start"))?.point;
        let b = record.vertices.get(b as usize).ok_or_else(|| invalid("quad cap lost an edge end"))?.point;
        let side = (0..4)
            .find(|&side| a == corners[side] && b == corners[(side + 1) % 4])
            .ok_or_else(|| invalid("quad cap edge is not a directed source chart side"))?;
        let slot = sides.get_mut(edge_use.edge as usize).ok_or_else(|| invalid("quad cap lost its source edge slots"))?;
        if slot.replace(side).is_some() {
            return Err(invalid("quad cap repeated a source edge"));
        }
    }
    let [Some(a), Some(b), Some(c), Some(d)] = sides else { return Err(invalid("quad cap is missing a source chart side")) };
    if BTreeSet::from([a, b, c, d]).len() != 4 {
        return Err(invalid("quad cap lost distinct source chart sides"));
    }
    Ok([a, b, c, d])
}

fn check_cap_seams(body: &kernel::Body, owners: &[usize], source_owners: &[usize], cap: &FittedQuadCap, edge_sides: &[usize; 4]) -> Result<()> {
    let mut caps = owners.iter().enumerate().filter_map(|(index, &owner)| (owner == cap.face).then_some(index));
    let face = caps.next().filter(|_| caps.next().is_none()).ok_or_else(|| invalid("quad cap sewing did not retain one distinct face"))?;
    let history = body.history().ok_or_else(|| invalid("quad cap sewing has no exact edge history"))?;
    let loops = kernel::face_boundaries(body, face)?;
    let [boundary] = loops.as_slice() else { return Err(invalid("quad cap sewing did not retain one rim")) };
    let mut seen = BTreeSet::new();
    for edge in boundary {
        let provenance = history.edges.get(edge.edge).ok_or_else(|| invalid("missing sewn quad cap edge history"))?;
        let mut sides = provenance.origins.iter().filter_map(|origin| match *origin {
            EdgeOrigin::Edge { operand: source, edge } if source == cap.operand => edge_sides.get(edge).copied(),
            _ => None,
        });
        let side = sides.next().filter(|_| sides.next().is_none()).ok_or_else(|| invalid("sewn cap edge lost its source side"))?;
        let owner = edge.neighbor.and_then(|neighbor| owners.get(neighbor));
        let expected = cap.rim.sides[side].owner;
        let mut retained = provenance.origins.iter().filter_map(|origin| match *origin {
            EdgeOrigin::Edge { operand: source, .. } if source != cap.operand => source_owners.get(source).copied(),
            _ => None,
        });
        if owner != Some(&expected) || retained.next() != Some(expected) || retained.any(|owner| owner != expected) {
            return Err(invalid("quad cap sewn edge has the wrong retained owner"));
        }
        seen.insert(side);
    }
    if seen.len() != 4 {
        return Err(invalid("quad cap sewing lost a rim side"));
    }
    Ok(())
}

// Exact atlas cells partition the unit chart. Walk the exterior ring between
// the four original corners to assign each restricted edge to its source side.
fn atlas_edge_sides(sheet: &kernel::Body, surface: &NurbsSurface) -> Result<BTreeMap<usize, usize>> {
    let corners = cap_corners(surface)?;
    let record = sheet.record();
    let mut incident = BTreeMap::<u32, Vec<(usize, u32)>>::new();
    let mut count = 0;
    for (index, edge) in record.edges.iter().enumerate().filter(|(_, edge)| edge.uses.len() == 1) {
        let [a, b] = edge.vertices;
        if a == b {
            return Err(invalid("quad cap atlas perimeter has a closed edge"));
        }
        incident.entry(a).or_default().push((index, b));
        incident.entry(b).or_default().push((index, a));
        count += 1;
    }
    let mut anchors = [None; 4];
    for (&vertex, edges) in &incident {
        if edges.len() != 2 {
            return Err(invalid("quad cap atlas perimeter branches or is open"));
        }
        let point = record.vertices.get(vertex as usize).ok_or_else(|| invalid("missing quad cap atlas perimeter vertex"))?.point;
        if let Some(index) = corners.iter().position(|&corner| corner == point)
            && anchors[index].replace(vertex).is_some()
        {
            return Err(invalid("quad cap atlas has an ambiguous corner"));
        }
    }
    let [Some(a), Some(b), Some(c), Some(d)] = anchors else { return Err(invalid("quad cap atlas lost an exact corner")) };
    let anchors = [a, b, c, d];
    let mut at = a;
    let mut previous_edge = None;
    let mut previous_corner = 0;
    let mut run = Vec::new();
    let mut completed = BTreeSet::new();
    let mut sides = BTreeMap::new();
    for step in 0..count {
        let (edge, next) = incident
            .get(&at)
            .and_then(|edges| edges.iter().copied().find(|&(edge, _)| Some(edge) != previous_edge))
            .ok_or_else(|| invalid("quad cap atlas perimeter has no successor"))?;
        previous_edge = Some(edge);
        run.push(edge);
        at = next;
        if at == a && step + 1 != count {
            return Err(invalid("quad cap atlas has multiple perimeter rings"));
        }
        let Some(corner) = anchors.iter().position(|&vertex| vertex == at) else { continue };
        let side = if (previous_corner + 1) % 4 == corner {
            previous_corner
        } else if (corner + 1) % 4 == previous_corner {
            corner
        } else {
            return Err(invalid("quad cap atlas perimeter changed corner order"));
        };
        if !completed.insert(side) {
            return Err(invalid("quad cap atlas repeated a perimeter side"));
        }
        for edge in run.drain(..) {
            if sides.insert(edge, side).is_some() {
                return Err(invalid("quad cap atlas repeated a perimeter edge"));
            }
        }
        previous_corner = corner;
    }
    if at != a || completed.len() != 4 || sides.len() != count {
        return Err(invalid("quad cap atlas lost a perimeter side"));
    }
    Ok(sides)
}

// Check that every source atlas face and exterior edge survives sewing.
// The same edge may contribute to several sewn edges after a trim split.
fn check_atlas_seams(
    body: &kernel::Body,
    owners: &[usize],
    source_owners: &[usize],
    cap: &FittedQuadCap,
    edge_sides: &BTreeMap<usize, usize>,
    face_count: usize,
) -> Result<()> {
    let history = body.history().ok_or_else(|| invalid("quad cap atlas sewing has no exact history"))?;
    let mut atlas_faces = BTreeSet::new();
    let mut rim_edges = BTreeSet::new();
    for (face, &owner) in owners.iter().enumerate().filter(|(_, owner)| **owner == cap.face) {
        let origins = history.faces.get(face).ok_or_else(|| invalid("missing sewn quad cap atlas face history"))?;
        let [FaceOrigin::Face { operand, face: source }] = origins.origins.as_slice() else {
            return Err(invalid("sewn quad cap atlas face has no unique source"));
        };
        if *operand != cap.operand || *source >= face_count || !atlas_faces.insert(*source) {
            return Err(invalid("quad cap atlas lost a distinct source face"));
        }
        let loops = kernel::face_boundaries(body, face)?;
        let [boundary] = loops.as_slice() else { return Err(invalid("quad cap atlas chart has no unique boundary")) };
        for edge in boundary {
            let provenance = history.edges.get(edge.edge).ok_or_else(|| invalid("missing sewn quad cap atlas edge history"))?;
            let mut source = None;
            let mut retained = None;
            for origin in &provenance.origins {
                let EdgeOrigin::Edge { operand, edge } = *origin else {
                    return Err(invalid("quad cap atlas sewing generated an unsupported edge"));
                };
                if operand == cap.operand {
                    if source.replace(edge).is_some() {
                        return Err(invalid("quad cap atlas edge has multiple source edges"));
                    }
                } else {
                    let owner = *source_owners.get(operand).ok_or_else(|| invalid("missing quad cap atlas seam owner"))?;
                    if retained.replace(owner).is_some_and(|old| old != owner) {
                        return Err(invalid("quad cap atlas seam has conflicting retained owners"));
                    }
                }
            }
            let source = source.ok_or_else(|| invalid("quad cap atlas edge has no source"))?;
            let neighbor = edge.neighbor.and_then(|neighbor| owners.get(neighbor)).copied();
            if let Some(&side) = edge_sides.get(&source) {
                let expected = Some(cap.rim.sides[side].owner);
                if retained != expected || neighbor != expected {
                    return Err(invalid("quad cap atlas rim did not sew to its retained side owner"));
                }
                rim_edges.insert(source);
            } else if neighbor != Some(owner) || retained.is_some() {
                return Err(invalid("quad cap atlas has an unsewn internal edge"));
            }
        }
    }
    if atlas_faces.len() != face_count || rim_edges.iter().ne(edge_sides.keys()) {
        return Err(invalid("quad cap atlas did not retain every face and rim side"));
    }
    Ok(())
}

fn invalid(error: impl std::fmt::Display) -> DocError {
    DocError::Invalid(error.to_string())
}
