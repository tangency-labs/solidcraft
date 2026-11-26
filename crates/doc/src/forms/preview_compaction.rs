//! Compact only certified coplanar evaluator cells in the live CAD preview.
use super::{Definition, FittedSource, stitched_roots};
use crate::kernel::{self, Body, history::FaceOrigin};
use solidcore_record::brep::Surface;
use solidcraft_geom::bspline::{HPoint, NurbsSurface};
use std::collections::{BTreeMap, BTreeSet};

const UNIT: [f64; 8] = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
const WIRE: [[f64; 2]; 8] = [[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 0.5], [1.0, 1.0], [0.5, 1.0], [0.0, 1.0], [0.0, 0.5]];

/// The original fitted cell tensors remain the exact guide and frame source.
/// Keep every external evaluator edge on the new sheet wire for sewing.
pub(super) fn stitch(definition: &Definition, source: &FittedSource, tolerance: f64) -> Option<(Body, Vec<usize>, f64)> {
    let hierarchy = definition.hierarchy.as_ref()?;
    if definition.parameters.kind != "face"
        || definition.symmetry.is_some()
        || definition.matching.is_some()
        || definition.profile.is_some()
        || definition.pipe.is_some()
        || definition.imported.is_some()
        || source.trimmed_roots
        || !source.quad_caps.is_empty()
    {
        return None;
    }
    let fit = &source.fit;
    let count = definition.cage.faces.len();
    if fit.patches.len() != count
        || fit.charts.len() != count
        || fit.patch_roots.len() != count
        || source.sheets.len() != count
        || fit.charts.iter().enumerate().any(|(cell, chart)| chart.face != cell || chart.domain != [0.0, 0.0, 1.0, 1.0])
        || source.sheets.iter().any(|sheet| sheet.face_count() != 1)
    {
        return None;
    }
    let mut replacements = BTreeMap::new();
    let mut merged = BTreeSet::new();
    let mut displacement = 0.0_f64;
    for cells in hierarchy.extrusion_preview_sheet_groups().ok()? {
        let first = cells[0];
        let owner = *fit.patch_roots.get(first)?;
        if cells.iter().any(|&cell| fit.patch_roots.get(cell) != Some(&owner) || !source.sheets.get(cell).is_some_and(is_plane)) {
            continue;
        }
        let [a, b, c, d] = cells.map(|cell| fit.patches.get(cell));
        let Some((surface, error)) = tensor([a?, b?, c?, d?]) else { continue };
        let Ok(sheet) = kernel::nurbs_region_sheet_with_planar_support(&surface, &[WIRE.to_vec()]) else { continue };
        if !is_plane(&sheet) {
            continue;
        }
        if cells.into_iter().any(|cell| !merged.insert(cell)) || replacements.insert(first, sheet).is_some() {
            return None;
        }
        displacement = displacement.max(error);
    }
    let deviation = (fit.sampled_deviation + displacement).next_up();
    if replacements.is_empty() || !deviation.is_finite() || deviation > tolerance {
        return None;
    }
    let mut sheets = Vec::with_capacity(count);
    let mut owners = Vec::with_capacity(count);
    for (cell, (sheet, &owner)) in source.sheets.iter().zip(&fit.patch_roots).enumerate() {
        let input = replacements.remove(&cell).or_else(|| (!merged.contains(&cell)).then(|| sheet.clone()));
        if let Some(input) = input {
            sheets.push(input);
            owners.push(owner);
        }
    }
    let body = kernel::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7).ok()?;
    let history = body.history()?;
    let mut operands = BTreeSet::new();
    if !body.is_surface()
        || body.face_count() != sheets.len()
        || history.faces.iter().any(|face| {
            !matches!(face.origins.as_slice(), [FaceOrigin::Face { operand, face: 0 }] if *operand < sheets.len() && operands.insert(*operand))
        })
        || operands.len() != sheets.len()
    {
        return None;
    }
    let owners = stitched_roots(&body, &sheets, &owners).ok()?;
    Some((body, owners, deviation))
}

fn is_plane(body: &Body) -> bool {
    matches!(body.record().faces.as_slice(), [face] if matches!(face.surface, Surface::Plane { .. }))
}

/// Concatenate four cubic Bezier pieces, oriented in root UV. A degree-three
/// knot at the interior line makes each half exactly its original Bezier
/// segment. The only displacement is reconciling roundoff-sized duplicate
/// seam coefficients. The nonnegative unit-weight basis bounds its effect
/// everywhere by the largest displaced coefficient.
fn tensor(patches: [&NurbsSurface; 4]) -> Option<(NurbsSurface, f64)> {
    if patches.iter().any(|patch| {
        patch.degree() != [3, 3]
            || patch.count() != [4, 4]
            || patch.knots().iter().any(|knots| knots.as_slice() != UNIT)
            || patch.homogeneous().iter().any(|point| point.w != 1.0)
    }) {
        return None;
    }
    let scale =
        patches.iter().flat_map(|patch| patch.homogeneous()).map(|point| point.x.abs().max(point.y.abs()).max(point.z.abs())).fold(1.0, f64::max);
    let mut controls = [None; 49];
    let mut gap = 0.0_f64;
    for (cell, (du, dv)) in [(0, 0), (3, 0), (3, 3), (0, 3)].into_iter().enumerate() {
        for v in 0..4 {
            for u in 0..4 {
                let (x, y) = match cell {
                    0 => (u, v),
                    1 => (v, 3 - u),
                    2 => (3 - u, 3 - v),
                    _ => (3 - v, u),
                };
                let point = patches[cell].point(x, y);
                let slot = &mut controls[(v + dv) * 7 + u + du];
                if let Some(previous) = slot {
                    let previous: &HPoint = previous;
                    let distance = [point.x - previous.x, point.y - previous.y, point.z - previous.z]
                        .into_iter()
                        .fold(0.0, |sum, coordinate| (sum + coordinate.abs().next_up()).next_up());
                    if distance > 64.0 * f64::EPSILON * scale {
                        return None;
                    }
                    gap = gap.max(distance);
                } else {
                    *slot = Some(point);
                }
            }
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
    let surface = NurbsSurface::from_homogeneous([3, 3], [knots.clone(), knots], controls.into_iter().collect::<Option<Vec<_>>>()?).ok()?;
    // Both the previous and the merged native planar supports may project
    // roundoff off their control hull, as qualified by planar_tensor_face.
    Some((surface, (gap + 128.0 * f64::EPSILON * scale).next_up()))
}
