//! Qualify one refined free-edge collapse against the original native charts.
use super::{Definition, FittedSource, convert_source, stitched_roots};
use crate::{DocError, Result, kernel};
use solidcore_record::{Vec3, brep::Surface};
use solidcraft_geom::Vec3Geom;
use solidcraft_geom::subdivision::hierarchy::BoundaryCollapse;
use std::collections::{BTreeMap, BTreeSet};

/// Exact native preview and stable target/neighbor identities.
pub struct PreparedBoundaryCollapse {
    pub body: kernel::Body,
    pub site: BoundaryCollapse,
    pub face_owners: Vec<usize>,
    pub point: Vec3,
}

/// Author the triangle only after exact native conversion succeeds.
pub fn author_boundary_collapse(definition: &Definition, vertices: [usize; 2], tolerance: f64) -> Result<Definition> {
    let candidate = candidate(definition, vertices)?;
    convert_source(&candidate, tolerance)?;
    Ok(candidate)
}

/// Replace one fitted free-edge quad by its trimmed triangle on the same
/// exact planar support. All other source sheets enter sewing unchanged.
pub fn prepare_boundary_collapse(definition: &Definition, vertices: [usize; 2], tolerance: f64) -> Result<PreparedBoundaryCollapse> {
    let candidate = candidate(definition, vertices)?;
    let site = candidate
        .hierarchy
        .as_ref()
        .ok_or_else(|| invalid("missing authored Collapse hierarchy"))?
        .validated_boundary_collapse()
        .map_err(invalid)?
        .ok_or_else(|| invalid("missing authored Collapse site"))?;
    let (body, report, source) = convert_source(&candidate, tolerance)?;
    let point = definition.placement.point(apex(&source, site)?);
    Ok(PreparedBoundaryCollapse { body, site, face_owners: report.patch_roots, point })
}

fn candidate(definition: &Definition, vertices: [usize; 2]) -> Result<Definition> {
    if definition.profile.is_some() || definition.matching.is_some() || definition.pipe.is_some() || definition.symmetry.is_some() {
        return Err(invalid("refined Collapse requires an independent Form without symmetry"));
    }
    let mut candidate = definition.clone();
    let hierarchy = candidate.hierarchy.as_mut().ok_or_else(|| invalid("refined Collapse needs a retained hierarchy"))?;
    hierarchy.reserve_boundary_collapse(vertices).map_err(invalid)?;
    Ok(candidate)
}

pub(super) fn stitch(source: &mut FittedSource, site: BoundaryCollapse) -> Result<(kernel::Body, Vec<usize>)> {
    if !source.trimmed_roots {
        return Err(invalid("refined Collapse requires fitted original leaf charts"));
    }
    let owners = &source.fit.patch_roots;
    if source.sheets.len() != owners.len() || owners.iter().copied().collect::<BTreeSet<_>>().len() != owners.len() {
        return Err(invalid("refined Collapse requires one fitted sheet per logical face"));
    }
    let operand = owners.iter().position(|&face| face == site.face).ok_or_else(|| invalid("missing exact Collapse sheet owner"))?;
    let old = source.sheets.get(operand).ok_or_else(|| invalid("missing exact Collapse sheet"))?;
    if old.record().faces.iter().any(|face| !matches!(face.surface, Surface::Plane { .. })) {
        return Err(invalid("refined Collapse currently requires an exact planar leaf"));
    }
    if old.face_count() != 1 {
        return Err(invalid("refined Collapse requires one original native leaf face"));
    }
    let support = source.fit.patches.get(site.root).ok_or_else(|| invalid("missing original Collapse support"))?;
    let triangle = kernel::nurbs_region_sheet_with_planar_support(support, &[site.parameters.to_vec()])?;
    let [new] = triangle.record().faces.as_slice() else { return Err(invalid("refined Collapse did not create one native triangle")) };
    if !matches!(new.surface, Surface::Plane { .. }) {
        return Err(invalid("refined Collapse could not retain exact planar support"));
    }
    let original = kernel::stitch(&source.sheets.iter().collect::<Vec<_>>(), 1e-7)?;
    let original_owners = stitched_roots(&original, &source.sheets, owners)?;
    if !original.is_surface() || original.shell_count() != 1 || !kernel::validity(&original).ok() {
        return Err(invalid("refined Collapse requires one valid open native skin"));
    }
    *source.sheets.get_mut(operand).ok_or_else(|| invalid("missing Collapse sheet slot"))? = triangle;
    let body = kernel::stitch(&source.sheets.iter().collect::<Vec<_>>(), 1e-7)?;
    let face_owners = stitched_roots(&body, &source.sheets, owners)?;
    if !body.is_surface() || body.shell_count() != 1 || !kernel::validity(&body).ok() || face_owners.len() != original_owners.len() {
        return Err(invalid("refined Collapse did not retain one valid open native skin"));
    }
    let expected: BTreeSet<_> = original_owners.iter().copied().collect();
    let actual: BTreeSet<_> = face_owners.iter().copied().collect();
    if actual != expected || actual.len() != face_owners.len() {
        return Err(invalid("refined Collapse changed native face ownership"));
    }
    if connections(&body, &face_owners)? != connections(&original, &original_owners)? {
        return Err(invalid("refined Collapse changed an existing connection between native faces"));
    }
    let face = face_owners.iter().position(|&owner| owner == site.face).ok_or_else(|| invalid("missing trimmed Collapse face"))?;
    let loops = kernel::face_boundaries(&body, face)?;
    let [boundary] = loops.as_slice() else { return Err(invalid("refined Collapse needs one triangular native boundary")) };
    let neighbors = boundary.iter().filter_map(|edge| edge.neighbor.and_then(|id| face_owners.get(id))).collect::<Vec<_>>();
    if boundary.len() != 3 || neighbors != [&site.retained_neighbor] {
        return Err(invalid("refined Collapse did not retain the original opposite seam"));
    }
    let local = apex(source, site)?;
    let apex_uses = boundary
        .iter()
        .filter(|edge| {
            body.record().edges.get(edge.edge).is_some_and(|record| {
                record.vertices.iter().any(|&id| {
                    body.record().vertices.get(id as usize).is_some_and(|vertex| {
                        let [x, y, z] = vertex.point;
                        local.dist(Vec3::new(x, y, z)) <= 1e-7
                    })
                })
            })
        })
        .count();
    if apex_uses != 2 {
        return Err(invalid("refined Collapse lost the original midpoint station"));
    }
    Ok((body, face_owners))
}

fn apex(source: &FittedSource, site: BoundaryCollapse) -> Result<Vec3> {
    let support = source.fit.patches.get(site.root).ok_or_else(|| invalid("missing original Collapse support"))?;
    let [u, v] = site.parameters[0];
    support.eval(u, v).map_err(invalid)
}

fn connections(body: &kernel::Body, owners: &[usize]) -> Result<BTreeMap<[usize; 2], usize>> {
    let mut result = BTreeMap::new();
    for edge in &body.record().edges {
        match edge.uses.as_slice() {
            [_] => {}
            [a, b] => {
                let owner = |id| owners.get(id as usize).copied().ok_or_else(|| invalid("missing Collapse edge face owner"));
                let (a, b) = (owner(a.face)?, owner(b.face)?);
                *result.entry([a.min(b), a.max(b)]).or_default() += 1;
            }
            _ => return Err(invalid("refined Collapse found a nonmanifold native edge")),
        }
    }
    Ok(result)
}

fn invalid(error: impl std::fmt::Display) -> DocError {
    DocError::Invalid(error.to_string())
}
