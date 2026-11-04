//! Editable Forms source data and conversion through the existing kernel boundary.
use crate::{DocError, Result};
use serde::{Deserialize, Serialize};
use solidcraft_geom::prelude::*;
use solidcraft_geom::subdivision::{Cage, primitives::Parameters};

mod association;
mod capping;
mod edge_curve;
mod pipe;
mod placement;
mod profile;
pub mod support;
pub use association::{MatchRequest, Matching, NamedEdge, SupportRef, match_openings};
pub(crate) use capping::cap;
pub use edge_curve::{EdgeCurve, edge_curves};
pub use pipe::{Pipe, PipeSource};
pub use placement::Placement;
pub use profile::{Profile, ProfileFit};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Imported {
    pub file: String,
    pub object: String,
}
impl Imported {
    pub fn validate(&self) -> Result<()> {
        if self.file.is_empty()
            || self.file.len() > 4096
            || self.object.is_empty()
            || self.object.len() > 256
            || self.file.contains('\0')
            || self.object.contains('\0')
        {
            return Err(DocError::Invalid("invalid imported Form provenance".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported: Option<Box<Imported>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipe: Option<Box<Pipe>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Box<Profile>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matching: Option<Box<Matching>>,
    #[serde(default, skip_serializing_if = "Placement::is_identity")]
    pub placement: Placement,
    pub cage: Cage,
    pub parameters: Parameters,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symmetry: Option<solidcraft_geom::subdivision::symmetry::Symmetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hierarchy: Option<solidcraft_geom::subdivision::hierarchy::Hierarchy>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ConversionReport {
    pub sampled_deviation_mm: f64,
    pub subdivision_level: usize,
    pub patches: usize,
    pub patch_roots: Vec<usize>,
    pub root_surfaces: usize,
    pub trimmed_roots: bool,
    pub solid: bool,
}
fn validate_definition(definition: &Definition) -> Result<()> {
    if let Some(imported) = &definition.imported {
        imported.validate()?;
    }
    if let Some(pipe) = &definition.pipe {
        pipe.validate()?;
        if definition.profile.is_some()
            || (pipe.follow_path && (definition.matching.is_some() || definition.hierarchy.is_some() || definition.symmetry.is_some()))
        {
            return Err(DocError::Invalid("sculpt a parametric pipe before changing its topology or symmetry".into()));
        }
    }
    if definition.profile.is_some() {
        return Err(DocError::Invalid("Form Surface conversion requires its source document state".into()));
    }
    definition.parameters.validate().map_err(|e| DocError::Invalid(e.to_string()))?;
    definition.placement.validate()?;
    if let Some(matching) = &definition.matching {
        matching.validate(&definition.cage, definition.hierarchy.as_ref())?;
    }
    if let Some(symmetry) = &definition.symmetry {
        symmetry.validate(&definition.cage).map_err(|e| DocError::Invalid(e.to_string()))?;
    }
    if let Some(hierarchy) = &definition.hierarchy {
        let evaluated = hierarchy.evaluate().map_err(|e| DocError::Invalid(e.to_string()))?;
        if evaluated.faces != definition.cage.faces
            || evaluated.points.len() != definition.cage.points.len()
            || evaluated.points.iter().zip(&definition.cage.points).any(|(a, b)| a.dist(*b) > 1e-8)
            || evaluated.edge_creases != definition.cage.edge_creases
            || evaluated.vertex_creases != definition.cage.vertex_creases
        {
            return Err(DocError::Invalid("saved Form evaluator disagrees with its hierarchy".into()));
        }
    }
    Ok(())
}
pub fn convert(definition: &Definition, tolerance: f64) -> Result<(crate::kernel::Body, ConversionReport)> {
    validate_definition(definition)?;
    #[cfg(test)]
    let timing = std::time::Instant::now();
    let FittedSource { fit, sheets, trimmed_roots } = fitted_source(definition, tolerance)?;
    #[cfg(test)]
    eprintln!("Forms fit ({} patches): {:?}", fit.patches.len(), timing.elapsed());
    let mut body = crate::kernel::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7)?;
    let patch_roots = stitched_roots(&body, &sheets, &fit.patch_roots)?;
    #[cfg(test)]
    eprintln!("Forms stitch: {:?}", timing.elapsed());
    if body.is_mesh() {
        return Err(DocError::Invalid("Forms CAD conversion returned a mesh instead of a B-rep".into()));
    }
    let mut uses = std::collections::BTreeMap::new();
    let logical = definition.hierarchy.as_ref().map(|h| h.layout()).transpose().map_err(|e| DocError::Invalid(e.to_string()))?;
    let faces = logical.as_ref().map_or(&definition.cage.faces, |layout| &layout.faces);
    for f in faces {
        for (&a, &b) in f.iter().zip(f.iter().cycle().skip(1)).take(f.len()) {
            *uses.entry((a.min(b), a.max(b))).or_insert(0_usize) += 1;
        }
    }
    let closed = uses.values().all(|&n| n == 2);
    if closed && body.is_surface() {
        return Err(DocError::Invalid("Forms CAD patches did not sew into a closed solid".into()));
    }
    if closed {
        let issues = body.validity();
        #[cfg(test)]
        eprintln!("Forms validity: {:?}", timing.elapsed());
        if !issues.is_empty() {
            return Err(DocError::Invalid(format!("Forms CAD solid failed validation: {}", issues.join("; "))));
        }
    }
    if !definition.placement.is_identity() {
        body = crate::kernel::place_matrix(&body, definition.placement.matrix())?;
    }
    Ok((
        body,
        ConversionReport {
            sampled_deviation_mm: fit.sampled_deviation,
            subdivision_level: fit.subdivision_level,
            patches: patch_roots.len(),
            root_surfaces: fit.patches.len(),
            trimmed_roots,
            patch_roots,
            solid: closed,
        },
    ))
}
/// Logical owners in the sewn body's canonical face order. A stitch may reorder
/// or split faces, so input position is never a face identity.
fn stitched_roots(body: &crate::kernel::Body, sheets: &[crate::kernel::Body], roots: &[usize]) -> Result<Vec<usize>> {
    use crate::kernel::history::BodyHistory;
    if sheets.len() != roots.len() {
        return Err(DocError::Invalid("Form CAD sheets lost their logical owners".into()));
    }
    let history = body.history().ok_or_else(|| DocError::Invalid("Form CAD stitching did not retain exact face provenance".into()))?;
    history.check(body, &sheets.iter().collect::<Vec<_>>())?;
    roots_from_origins(&history.faces, roots)
}

fn roots_from_origins(faces: &[crate::kernel::history::FaceProv], roots: &[usize]) -> Result<Vec<usize>> {
    use crate::kernel::history::FaceOrigin;
    faces
        .iter()
        .map(|face| {
            let mut owner = None;
            for origin in &face.origins {
                let FaceOrigin::Face { operand, .. } = origin else {
                    return Err(DocError::Invalid("Form CAD face has no exact source sheet".into()));
                };
                let root = *roots.get(*operand).ok_or_else(|| DocError::Invalid("Form CAD face names a missing source sheet".into()))?;
                if owner.is_some_and(|previous| previous != root) {
                    return Err(DocError::Invalid("Form CAD stitching merged distinct logical faces".into()));
                }
                owner = Some(root);
            }
            owner.ok_or_else(|| DocError::Invalid("Form CAD face lost its logical source".into()))
        })
        .collect()
}

fn root_sheets(
    hierarchy: &solidcraft_geom::subdivision::hierarchy::Hierarchy,
    tolerance: f64,
) -> Result<(solidcraft_geom::subdivision::fitting::Fit, Vec<crate::kernel::Body>)> {
    let mut fit = hierarchy.fit_root_patches(tolerance).map_err(|e| DocError::Invalid(e.to_string()))?;
    let regions = hierarchy.cad_regions().map_err(|e| DocError::Invalid(e.to_string()))?;
    let mut sheets = Vec::with_capacity(regions.len());
    let mut owners = Vec::with_capacity(regions.len());
    let remaining = tolerance - fit.sampled_deviation;
    let mut rechart_deviation = 0.0_f64;
    for mut region in regions {
        let area =
            |wire: &Vec<[f64; 2]>| wire.iter().zip(wire.iter().cycle().skip(1)).take(wire.len()).map(|(a, b)| a[0] * b[1] - a[1] * b[0]).sum::<f64>();
        region.wires.sort_by(|a, b| area(b).total_cmp(&area(a)));
        if region.wires.iter().filter(|w| area(w) > 0.0).count() != 1 {
            return Err(DocError::Invalid("a root trim requires one exterior boundary".into()));
        }
        let surface = fit.patches.get(region.root).ok_or_else(|| DocError::Invalid("missing fitted root surface".into()))?;
        let (sheet, deviation) = crate::kernel::fitted_nurbs_region_sheet(surface, &region.wires, remaining)?;
        sheets.push(sheet);
        rechart_deviation = rechart_deviation.max(deviation);
        owners.push(region.owner);
    }
    if let Some(wall) = hierarchy.validated_trim_wall().map_err(|e| DocError::Invalid(e.to_string()))? {
        let skin = fit.patches.get(wall.root()).ok_or_else(|| DocError::Invalid("missing trim wall root surface".into()))?;
        let patch = wall.ruled_patch(skin).map_err(|e| DocError::Invalid(e.to_string()))?;
        let (sheet, deviation) = crate::kernel::fitted_nurbs_sheet(&patch, remaining)?;
        sheets.push(sheet);
        rechart_deviation = rechart_deviation.max(deviation);
        fit.charts.push(solidcraft_geom::subdivision::fitting::PatchChart::whole(fit.patches.len()));
        fit.patches.push(patch);
        owners.push(hierarchy.leaves.len());
    }
    fit.sampled_deviation += rechart_deviation;
    fit.patch_roots = owners;
    Ok((fit, sheets))
}

fn fitted_sheets(fit: &mut solidcraft_geom::subdivision::fitting::Fit, tolerance: f64) -> Result<Vec<crate::kernel::Body>> {
    let remaining = tolerance - fit.sampled_deviation;
    let mut rechart_deviation = 0.0_f64;
    let mut sheets = Vec::with_capacity(fit.patches.len());
    for patch in &fit.patches {
        let (sheet, deviation) = crate::kernel::fitted_nurbs_sheet(patch, remaining)?;
        sheets.push(sheet);
        rechart_deviation = rechart_deviation.max(deviation);
    }
    fit.sampled_deviation += rechart_deviation;
    Ok(sheets)
}
struct FittedSource {
    fit: solidcraft_geom::subdivision::fitting::Fit,
    sheets: Vec<crate::kernel::Body>,
    trimmed_roots: bool,
}
fn fitted_source(definition: &Definition, tolerance: f64) -> Result<FittedSource> {
    if let Some(hierarchy) = &definition.hierarchy
        && hierarchy.has_trim_wall()
    {
        if definition.matching.is_some() || definition.symmetry.is_some() {
            return Err(DocError::Invalid("trim walls require a Form without Match or symmetry".into()));
        }
        let (fit, sheets) = root_sheets(hierarchy, tolerance)?;
        return Ok(FittedSource { fit, sheets, trimmed_roots: true });
    }
    if let Some(matching) = &definition.matching {
        let evaluator = if matching.evaluation_order(&definition.cage, definition.hierarchy.as_ref())? == 2 {
            solidcraft_geom::subdivision::matching::quadratic_extension(&definition.cage).map_err(|e| DocError::Invalid(e.to_string()))?
        } else {
            definition.cage.clone()
        };
        let mut fit = matching.basis.fit_checked(&evaluator, tolerance).map_err(|e| DocError::Invalid(e.to_string()))?;
        fit.patches.truncate(definition.cage.faces.len());
        fit.patch_roots.truncate(definition.cage.faces.len());
        fit.charts.truncate(definition.cage.faces.len());
        if let Some(hierarchy) = &definition.hierarchy {
            filter_evaluator_patches(&mut fit, hierarchy)?;
        }
        let sheets = fitted_sheets(&mut fit, tolerance)?;
        return Ok(FittedSource { fit, sheets, trimmed_roots: false });
    }
    if let Some(hierarchy) = &definition.hierarchy
        && let Ok((fit, sheets)) = root_sheets(hierarchy, tolerance)
    {
        return Ok(FittedSource { fit, sheets, trimmed_roots: true });
    }
    let mut fit = definition.cage.fit_patches(tolerance).map_err(|e| DocError::Invalid(e.to_string()))?;
    if let Some(hierarchy) = &definition.hierarchy {
        filter_evaluator_patches(&mut fit, hierarchy)?;
    }
    let sheets = fitted_sheets(&mut fit, tolerance)?;
    Ok(FittedSource { fit, sheets, trimmed_roots: false })
}
fn filter_evaluator_patches(
    fit: &mut solidcraft_geom::subdivision::fitting::Fit,
    hierarchy: &solidcraft_geom::subdivision::hierarchy::Hierarchy,
) -> Result<()> {
    let owners = hierarchy.layout().map_err(|e| DocError::Invalid(e.to_string()))?.owners;
    for ((patch, root), chart) in
        std::mem::take(&mut fit.patches).into_iter().zip(std::mem::take(&mut fit.patch_roots)).zip(std::mem::take(&mut fit.charts))
    {
        if let Some(owner) = *owners.get(root).ok_or_else(|| DocError::Invalid("missing hierarchical CAD leaf".into()))? {
            fit.patches.push(patch);
            fit.patch_roots.push(owner);
            fit.charts.push(chart);
        }
    }
    Ok(())
}
impl crate::walk::Walk for Definition {
    fn walk_refs<'a>(&'a self, f: &mut dyn FnMut(crate::walk::RefItem<'a>)) {
        if let Some(pipe) = &self.pipe {
            crate::walk::Walk::walk_refs(&pipe.source, f);
        }
        if let Some(profile) = &self.profile {
            crate::walk::Walk::walk_refs(&profile.source, f);
        }
        if let Some(matching) = &self.matching {
            crate::walk::Walk::walk_refs(matching.as_ref(), f);
        }
    }
    fn walk_refs_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::RefItemMut<'a>)) {
        if let Some(pipe) = &mut self.pipe {
            crate::walk::Walk::walk_refs_mut(&mut pipe.source, f);
        }
        if let Some(profile) = &mut self.profile {
            crate::walk::Walk::walk_refs_mut(&mut profile.source, f);
        }
        if let Some(matching) = &mut self.matching {
            crate::walk::Walk::walk_refs_mut(matching.as_mut(), f);
        }
    }
    fn walk_geometry<'a>(&'a self, f: &mut dyn FnMut(crate::walk::GeoItem<'a>)) {
        if let Some(pipe) = &self.pipe {
            crate::walk::Walk::walk_geometry(&pipe.source, f);
        }
        if let Some(profile) = &self.profile {
            crate::walk::Walk::walk_geometry(&profile.source, f);
        }
        f(crate::walk::Geo::Pos(&self.placement.origin));
        for axis in &self.placement.axes {
            f(crate::walk::Geo::Dir(axis));
        }
        if let Some(matching) = &self.matching {
            crate::walk::Walk::walk_geometry(matching.as_ref(), f);
        }
    }
    fn walk_geometry_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::GeoItemMut<'a>)) {
        if let Some(pipe) = &mut self.pipe {
            crate::walk::Walk::walk_geometry_mut(&mut pipe.source, f);
        }
        if let Some(profile) = &mut self.profile {
            crate::walk::Walk::walk_geometry_mut(&mut profile.source, f);
        }
        f(crate::walk::Geo::Pos(&mut self.placement.origin));
        for axis in &mut self.placement.axes {
            f(crate::walk::Geo::Dir(axis));
        }
        if let Some(matching) = &mut self.matching {
            crate::walk::Walk::walk_geometry_mut(matching.as_mut(), f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::history::BodyHistory;

    fn assert_surface_cell_owners(fit: &ProfileFit, cage: &Cage) {
        let roots = fit.patch_roots.as_ref().expect("the fitted surface must retain its CAD cells");
        assert_eq!(roots.len(), fit.patch.face_count());
        assert_eq!(roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), (0..cage.faces.len()).collect());
        for (face, &owner) in roots.iter().enumerate() {
            let geometry = crate::kernel::face_geometry(&fit.patch, face).unwrap();
            let corners: Vec<_> = geometry.loops.iter().flatten().map(|edge| edge.at(0.0).unwrap().0).collect();
            let expected = &cage.faces[owner];
            assert_eq!(corners.len(), expected.len());
            for &vertex in expected {
                assert!(corners.iter().any(|point| point.dist(cage.points[vertex]) < 0.01), "CAD face {face} does not belong to guide cell {owner}");
            }
        }
    }
    #[test]
    fn stitched_cad_cells_follow_their_exact_sources_after_canonical_reordering() {
        use solidcore_record::Vec3;
        use solidcraft_geom::bspline::NurbsSurface;
        let sheets = [0.0, 10.0, 1.0, 11.0].map(|x| {
            let surface = NurbsSurface::new(
                [1, 1],
                [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
                &[Vec3::new(x, 0.0, 0.0), Vec3::new(x + 1.0, 0.0, 0.0), Vec3::new(x, 1.0, 0.0), Vec3::new(x + 1.0, 1.0, 0.0)],
                &[1.0; 4],
            )
            .unwrap();
            crate::kernel::nurbs_sheet(&surface).unwrap()
        });
        let inputs = [17, 42, 19, 44];
        let body = crate::kernel::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7).unwrap();
        assert_eq!(body.shell_count(), 2);
        assert_eq!(body.face_count(), 4, "sewing must keep each distinct CAD cell");
        let owners = stitched_roots(&body, &sheets, &inputs).unwrap();
        assert_ne!(owners, inputs, "interleaved disconnected shells must be canonicalized");
        for (face, owner) in owners.into_iter().enumerate() {
            let geometry = crate::kernel::face_geometry(&body, face).unwrap();
            let [[u0, u1], [v0, v1]] = crate::kernel::face_parameter_span(&body, face).unwrap();
            let x = geometry.surface.eval((u0 + u1) * 0.5, (v0 + v1) * 0.5).0.x;
            let expected = if x < 1.0 {
                17
            } else if x < 5.0 {
                19
            } else if x < 11.0 {
                42
            } else {
                44
            };
            assert_eq!(owner, expected, "face {face} at x={x} inherited the wrong logical cell");
        }
    }

    #[test]
    fn fitted_logical_edge_curves_keep_parameters_reversal_and_placement() {
        use solidcore_record::Vec3;
        let parameters = Parameters { dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
        let mut cage = parameters.build().unwrap();
        cage.points[0] += Vec3::new(2.0, 0.0, 3.0);
        let edge = [cage.faces[0][0], cage.faces[0][1]];
        let source = Definition {
            cage,
            parameters,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            placement: Default::default(),
            symmetry: None,
            hierarchy: None,
        };
        let curves = edge_curves(&source, 0.01, &[edge, [edge[1], edge[0]]]).unwrap();
        for t in [0.0, 0.3, 0.65, 1.0] {
            assert!(curves[0].at(t).unwrap().dist(curves[1].at(t).unwrap()) < 1e-12);
        }
        assert!(curves[0].at(f64::NAN).is_err());
        assert!(curves[0].at(-0.1).is_err());
        let mut placed = source.clone();
        placed.placement.origin = Vec3::new(1000.0, -2000.0, 3000.0);
        placed.placement.axes = [Vec3::Y, -Vec3::X, Vec3::Z];
        let world = edge_curves(&placed, 0.01, &[edge]).unwrap().remove(0);
        assert!(world.at(0.3).unwrap().dist(placed.placement.point(curves[0].at(0.3).unwrap())) < 1e-10);
        let hierarchy = solidcraft_geom::subdivision::hierarchy::Hierarchy::new(source.cage.clone()).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let (edge, _) = hierarchy.layout().unwrap().paths.into_iter().find(|(_, path)| path.len() > 2).unwrap();
        let mut refined = source;
        refined.cage = hierarchy.evaluate().unwrap();
        refined.hierarchy = Some(hierarchy);
        let curve = edge_curves(&refined, 0.01, &[edge]).unwrap().remove(0);
        for t in [0.0, 0.3, 0.5, 0.65, 1.0] {
            assert!(curve.at(t).unwrap().is_finite());
        }
        let converted = convert(&refined, 0.01).unwrap();
        assert!(!converted.0.is_mesh());
    }

    #[test]
    fn rational_tiled_edge_restrictions_use_exact_parameter_positions() {
        use solidcore_record::Vec3;
        use solidcraft_geom::bspline::NurbsSurface;
        use solidcraft_geom::subdivision::fitting::{EdgeChart, PatchChart};
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let surface = std::sync::Arc::new(
            NurbsSurface::new(
                [2, 1],
                [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
                &[
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(1.0, 1.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    Vec3::new(1.0, 0.0, 2.0),
                    Vec3::new(1.0, 1.0, 2.0),
                    Vec3::new(0.0, 1.0, 2.0),
                ],
                &[1.0, w, 1.0, 1.0, w, 1.0],
            )
            .unwrap(),
        );
        let chart = EdgeChart { face: 0, uv: [[0.0, 0.0], [1.0, 0.0]], range: [0.0, 1.0] };
        let mut patches = Vec::new();
        for range in [[0.0, 0.5], [0.5, 1.0]] {
            let (knots, ctrl) = surface.iso_curve_segment(solidcraft_geom::bspline::Dir::S, 0.0, range).unwrap();
            let mut net = ctrl.clone();
            net.extend(ctrl.into_iter().map(|mut p| {
                p.z += 2.0 * p.w;
                p
            }));
            let tile = NurbsSurface::from_homogeneous([2, 1], [knots, vec![0.0, 0.0, 1.0, 1.0]], net).unwrap();
            patches.push((PatchChart { face: 0, domain: [range[0], 0.0, range[1], 1.0] }, std::sync::Arc::new(tile)));
        }
        let curve = super::edge_curve::assemble([0, 1], vec![chart], &patches, &Default::default()).unwrap();
        for t in [0.3, 0.5, 0.65] {
            let p = curve.at(t).unwrap();
            assert!((p.x * p.x + p.y * p.y - 1.0).abs() < 1e-12);
            assert!(p.dist(surface.eval(t, 0.0).unwrap()) < 1e-12);
        }
        let q = curve.at(0.3).unwrap();
        assert!(q.dist(Vec3::new(0.7, 0.3, 0.0)) > 0.1, "the exact rational curve must not become its endpoint chord");
        let bounds = curve.bounds();
        for i in 0..=100 {
            let p = curve.at(i as f64 / 100.0).unwrap();
            assert!(p.x >= bounds.min.x - 1e-12 && p.x <= bounds.max.x + 1e-12 && p.y >= bounds.min.y - 1e-12 && p.y <= bounds.max.y + 1e-12);
        }
    }
    #[test]
    fn nested_t_junction_edge_targets_follow_independent_leaf_root_parameters() {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::hierarchy::{Hierarchy, MAX_DEPTH};
        let parameters = Parameters { dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
        let mut hierarchy = Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[0], [2, 2]).unwrap().subdivide(&[6], [2, 2]).unwrap();
        let corners = hierarchy.leaf_corners().unwrap();
        let detail = corners[13].unwrap()[0];
        hierarchy.add_deltas(&[detail], &[Vec3::new(0.0, 0.2, 0.7)]).unwrap();
        let layout = hierarchy.layout().unwrap();
        assert!(layout.faces[7].len() > 4, "coarse neighbor must expose the nested T-junction");
        let leaf = &hierarchy.leaves[7];
        let corner = corners[7].unwrap();
        let edge = [corner[1], corner[2]];
        assert!(
            layout.paths.iter().any(|(e, path)| [e[0].min(e[1]), e[0].max(e[1])] == [edge[0].min(edge[1]), edge[0].max(edge[1])] && path.len() > 2)
        );
        let grid = (1usize << MAX_DEPTH) as f64;
        let [_, v, x, y] = leaf.domain;
        let uv = if edge[0] < edge[1] {
            [[x as f64 / grid, v as f64 / grid], [x as f64 / grid, y as f64 / grid]]
        } else {
            [[x as f64 / grid, y as f64 / grid], [x as f64 / grid, v as f64 / grid]]
        };
        let root = leaf.root;
        let source = Definition {
            cage: hierarchy.evaluate().unwrap(),
            hierarchy: Some(hierarchy),
            parameters,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            symmetry: None,
            placement: Default::default(),
        };
        let fitted = fitted_source(&source, 0.02).unwrap();
        assert!(fitted.trimmed_roots);
        let curve = edge_curves(&source, 0.02, &[edge, [edge[1], edge[0]]]).unwrap();
        for t in [0.3, 0.65] {
            let u = uv[0][0] * (1.0 - t) + uv[1][0] * t;
            let v = uv[0][1] * (1.0 - t) + uv[1][1] * t;
            let expected = fitted.fit.patches[root].eval(u, v).unwrap();
            assert!(curve[0].at(t).unwrap().dist(expected) < 1e-9);
            assert!(curve[1].at(t).unwrap().dist(expected) < 1e-9);
        }
    }
    #[test]
    fn ngon_source_edge_curves_retain_original_ids_over_first_step_quad_charts() {
        use solidcore_record::Vec3;
        let parameters = Parameters::default();
        let cage = Cage {
            points: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.2),
                Vec3::new(4.0, 2.0, 0.0),
                Vec3::new(1.0, 4.0, 0.5),
                Vec3::new(-1.0, 2.0, -0.3),
            ],
            faces: vec![vec![0, 1, 2, 3, 4]],
            ..Default::default()
        };
        let source = Definition {
            cage,
            parameters: parameters.clone(),
            hierarchy: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            symmetry: None,
            placement: Default::default(),
        };
        let fitted = fitted_source(&source, 0.05).unwrap();
        assert!(fitted.fit.charts.iter().all(|chart| chart.domain == [0.0, 0.0, 1.0, 1.0]));
        let curves = edge_curves(&source, 0.05, &[[0, 1], [1, 0]]).unwrap();
        assert_eq!(curves[0].edge, [0, 1]);
        for t in [0.3, 0.65] {
            let (face, u, v) = if t < 0.5 { (0, t * 2.0, 0.0) } else { (1, 0.0, 2.0 * (1.0 - t)) };
            let index = fitted.fit.charts.iter().position(|chart| chart.face == face).unwrap();
            let expected = fitted.fit.patches[index].eval(u, v).unwrap();
            assert!(curves[0].at(t).unwrap().dist(expected) < 1e-9);
            assert!(curves[1].at(t).unwrap().dist(expected) < 1e-9);
        }
        let cube = Parameters { segments: [1; 3], dimensions: [10.0; 3], ..parameters }.build().unwrap();
        let edge = [cube.faces[0][0], cube.faces[0][1]];
        let dissolved = cube.dissolve_edge(edge).unwrap();
        assert!(dissolved.faces.iter().any(|face| face.len() > 4));
        let face = dissolved.faces.iter().find(|face| face.len() > 4).unwrap();
        let edge = [face[0], face[1]];
        let source = Definition { cage: dissolved, ..source };
        let curve = edge_curves(&source, 0.05, &[edge]).unwrap().remove(0);
        for t in [0.0, 0.3, 0.5, 0.65, 1.0] {
            assert!(curve.at(t).unwrap().is_finite());
        }
        assert!(!convert(&source, 0.05).unwrap().0.is_mesh());
    }

    #[test]
    fn cad_face_provenance_requires_all_source_owners_to_agree() {
        use crate::kernel::history::{Change, FaceOrigin, FaceProv};
        let origin = |operand| FaceOrigin::Face { operand, face: 0 };
        let merged = FaceProv { origins: vec![origin(1), origin(0)], change: Change::Modified };
        assert_eq!(roots_from_origins(std::slice::from_ref(&merged), &[7, 7]).unwrap(), vec![7]);
        assert!(roots_from_origins(&[merged], &[7, 8]).unwrap_err().to_string().contains("distinct logical faces"));
        assert!(roots_from_origins(&[FaceProv { origins: Vec::new(), change: Change::Generated }], &[7]).is_err());
    }

    #[test]
    fn pipe_sources_follow_sketch_and_named_cad_edges_until_the_cage_is_sculpted() {
        use solidcore_record::{Plane, Vec2, Vec3};
        use solidcraft_geom::subdivision::pipe_network::PipeSettings;
        let mut state = crate::ModelState::default();
        let body = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        state.bodies.push(crate::ModelBody::new("Support".into(), body, 1));
        let mesh = state.bodies[0].mesh();
        let edges: Vec<_> = mesh
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.first().is_some_and(|p| p.len() < 1e-8) || e.last().is_some_and(|p| p.len() < 1e-8))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(edges.len(), 3);
        let pipe = Pipe {
            source: PipeSource::from_edges(&state.bodies[0], &edges).unwrap(),
            settings: PipeSettings { diameter: 2.0, ..Default::default() },
            follow_path: true,
        };
        let original = pipe.rebuild(&state).unwrap();
        let keys: Vec<_> = original.segments.iter().map(|s| s.key.clone()).collect();
        state.bodies[0] = crate::ModelBody::new("Support".into(), crate::kernel::box_solid(Vec3::ZERO, Vec3::new(30.0, 20.0, 20.0)).unwrap(), 1);
        let updated = pipe.rebuild(&state).unwrap();
        assert_eq!(updated.segments.iter().map(|s| s.key.clone()).collect::<Vec<_>>(), keys);
        assert!(updated.cage.points.iter().any(|p| p.x > 29.9));
        assert!(!original.cage.points.iter().any(|p| p.x > 21.0));
        let restored: Pipe = serde_json::from_slice(&serde_json::to_vec(&pipe).unwrap()).unwrap();
        assert_eq!(restored, pipe);
        let mut frozen = pipe.clone();
        frozen.follow_path = false;
        assert_eq!(frozen.recompute(&original.cage, &crate::ModelState::default()).unwrap(), original.cage);
        assert!(pipe.rebuild(&crate::ModelState::default()).is_err());
        let mut sketch = solidcraft_sketch::Sketch::new();
        for (id, end) in [("a", Vec2::new(20.0, 0.0)), ("b", Vec2::new(-20.0, 0.0)), ("c", Vec2::new(0.0, 20.0))] {
            sketch.add_line(Vec2::ZERO, end, None, None, Some(id)).unwrap();
        }
        let report = solidcraft_sketch::solve(&mut sketch);
        let solved =
            crate::SolvedSketch { feature: 7, name: "Path".into(), plane: Plane::XY, sketch, report, profiles: Vec::new(), ends: Default::default() };
        let pipe = Pipe {
            source: PipeSource::from_sketch(&solved, &[]).unwrap(),
            settings: PipeSettings { diameter: 2.0, ..Default::default() },
            follow_path: true,
        };
        state.sketches.push(std::sync::Arc::new(solved));
        let first = pipe.rebuild(&state).unwrap();
        assert_eq!(first.segments.len(), 3);
        let solved = std::sync::Arc::make_mut(&mut state.sketches[0]);
        let solidcraft_sketch::CurveKind::Line { b, .. } = solved.sketch.curves[0].kind else { panic!("line") };
        solved.sketch.points[b].pos.x = 35.0;
        let second = pipe.recompute(&first.cage, &state).unwrap();
        assert!(second.points.iter().any(|p| p.x > 34.9));
    }
    #[test]
    fn network_pipe_cages_fit_closed_cad_at_junctions_and_closed_paths() {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::pipe_network::{Network, PathEdge, PipeSettings};
        let mut paths =
            vec![vec![Vec3::X, -Vec3::X, Vec3::Y], vec![Vec3::X, Vec3::new(-0.5, 0.866025403784, 0.0), Vec3::new(-0.5, -0.866025403784, 0.0)]];
        for (id, directions) in paths.drain(..).enumerate() {
            let source =
                directions.iter().enumerate().map(|(i, &d)| PathEdge { key: format!("edge{i}"), points: vec![Vec3::ZERO, d * 20.0] }).collect();
            let network = Network::new(source).unwrap();
            let built = network.build(&PipeSettings { diameter: 2.0, ..Default::default() }).unwrap();
            let definition = Definition {
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
                placement: Default::default(),
                cage: built.cage,
                parameters: Parameters { kind: "pipe".into(), dimensions: [2.0; 3], ..Default::default() },
                symmetry: None,
                hierarchy: None,
            };
            let (body, report) = convert(&definition, 0.05).unwrap_or_else(|e| panic!("junction {id}: {e}"));
            assert!(!body.is_surface() && body.validity().is_empty(), "junction {id}");
            assert!(report.solid && crate::kernel::measure(&body).unwrap().volume > 1.0);
        }
        let points = (0..=24)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / 24.0;
                Vec3::new(a.cos() * 10.0, a.sin() * 10.0, (2.0 * a).sin())
            })
            .collect();
        let network = Network::new(vec![PathEdge { key: "closed".into(), points }]).unwrap();
        let built = network.build(&PipeSettings { diameter: 1.0, path_samples: 16, ..Default::default() }).unwrap();
        let definition = Definition {
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            placement: Default::default(),
            cage: built.cage,
            parameters: Parameters { kind: "pipe".into(), dimensions: [1.0; 3], ..Default::default() },
            symmetry: None,
            hierarchy: None,
        };
        let (body, report) = convert(&definition, 0.05).unwrap();
        assert!(!body.is_surface() && body.validity().is_empty() && report.solid);
    }
    #[test]
    fn surface_cad_cells_follow_skewed_guides_without_refitting_the_skin() {
        use solidcore_record::Vec3;
        let mut state = crate::ModelState::default();
        let body = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 2.0)).unwrap();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        state.bodies.push(crate::ModelBody::new("Support".into(), body, 1));
        let (profile, mut cage) = Profile::seed(&state.bodies[0], top, [2, 2], 0).unwrap();
        cage.points[4] += Vec3::new(0.7, -0.4, 1.0);
        let fit = profile.fit(&state, &cage, &Placement::default(), 0.01).unwrap();
        assert_surface_cell_owners(&fit, &cage);
        assert_eq!(fit.patch.face_count(), 4);
        assert_eq!(fit.solid.face_count(), 9);
        assert!(!fit.solid.is_surface() && fit.solid.validity().is_empty());
        fit.solid.history().unwrap().check(&fit.solid, &[&fit.open, &fit.patch]).unwrap();
        let mut legacy = profile.clone();
        legacy.partition = false;
        let old = legacy.fit(&state, &cage, &Placement::default(), 0.01).unwrap();
        let volume = crate::kernel::measure(&fit.solid).unwrap().volume;
        let previous = crate::kernel::measure(&old.solid).unwrap().volume;
        assert!((volume - previous).abs() < 0.1, "{volume} vs {previous}");
        let (mesh, report) = profile.display(&state, &cage, &Placement::default(), 0.01).unwrap();
        assert_eq!(report.patch_roots, fit.patch_roots.clone().unwrap());
        assert_eq!(mesh.tri_face.iter().copied().collect::<std::collections::BTreeSet<_>>(), (0..4).collect());
        for (edge, curve) in cage.edges().unwrap().iter().zip(&mesh.edges) {
            assert!(curve[0].dist(cage.points[edge[0]]) < 0.01);
            assert!(curve.last().unwrap().dist(cage.points[edge[1]]) < 0.01);
        }
        for (up, segments, continuity) in [(true, [2, 2], 1), (true, [3, 2], 0), (false, [2, 2], 0)] {
            let source = &state.bodies[0];
            let face = (0..source.body.face_count())
                .find(|&i| source.body.planar_face(i).is_some_and(|p| p.normal().z * if up { 1.0 } else { -1.0 } > 0.99))
                .unwrap();
            let (profile, mut cage) = Profile::seed(source, face, segments, continuity).unwrap();
            for (id, point) in cage.points.iter_mut().enumerate() {
                if !profile.boundary().contains(&id) {
                    *point += Vec3::new(0.3, -0.2, if up { 1.0 } else { -1.0 });
                }
            }
            let fit = profile.fit(&state, &cage, &Placement::default(), 0.01).unwrap();
            assert_surface_cell_owners(&fit, &cage);
            assert!(!fit.solid.is_surface() && fit.solid.validity().is_empty());
        }
    }
    #[test]
    fn tall_g1_surface_cells_snap_only_inverse_projection_roundoff_at_stored_knots() {
        use solidcore_record::Vec3;
        for height in [2.0, 3.0] {
            let mut state = crate::ModelState::default();
            let body = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, height)).unwrap();
            let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
            state.bodies.push(crate::ModelBody::new("Support".into(), body, 1));
            let (profile, mut cage) = Profile::seed(&state.bodies[0], top, [2, 2], 1).unwrap();
            let center = cage.points.iter().position(|point| (point.x - 5.0).abs() < 1e-12 && (point.y - 5.0).abs() < 1e-12).unwrap();
            cage.points[center].z += 5.0;
            let fit = profile.fit(&state, &cage, &Placement::default(), 0.01).unwrap();
            assert_surface_cell_owners(&fit, &cage);
            assert_eq!(fit.patch.face_count(), 4, "a roundoff-oblique seam must not silently fall back to one CAD face");
            assert_eq!(fit.solid.face_count(), 9);
            assert!(!fit.solid.is_surface() && fit.solid.validity().is_empty());
            assert!(fit.report.normal_deviation_deg <= 0.051);
            fit.solid.history().unwrap().check(&fit.solid, &[&fit.open, &fit.patch]).unwrap();
            assert!(profile.boundary().iter().all(|&i| cage.points[i] == profile.rest[i]));
        }
    }
    #[test]
    fn face_profile_keeps_the_cad_rim_and_rebases_interior_offsets_on_source_changes() {
        use solidcore_record::Vec3;
        let mut state = crate::ModelState::default();
        let original = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 2.0)).unwrap();
        state.bodies.push(crate::ModelBody::new("Support".into(), original.clone(), 1));
        let top = (0..original.face_count()).find(|&i| original.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let (mut profile, mut cage) = Profile::seed(&state.bodies[0], top, [2, 2], 0).unwrap();
        profile.partition = false;
        let boundary = profile.boundary();
        cage.points[4].z += 1.0;
        let ProfileFit { open, patch, solid, .. } = profile.fit(&state, &cage, &Placement::default(), 0.01).unwrap();
        assert!(open.is_surface() && patch.face_count() == 1 && !solid.is_surface() && solid.validity().is_empty());
        assert!(crate::kernel::face_geometry(&patch, 0).unwrap().parameters(cage.points[4], None, 0.01).is_ok());
        assert!(boundary.iter().all(|&i| cage.points[i] == profile.rest[i]));
        assert!(state.bodies[0].body.same_shape(&original));
        let volume = crate::kernel::measure(&solid).unwrap().volume;
        assert!(volume > 200.1 && volume < 300.0, "{volume}");
        let mut tangent = profile.clone();
        tangent.continuity = 1;
        let mut dome = cage.clone();
        dome.points[4].z = 7.0;
        let fitted = tangent.fit(&state, &dome, &Placement::default(), 0.01).unwrap();
        assert!(fitted.report.normal_deviation_deg <= 0.051);
        assert!(!fitted.solid.is_surface() && fitted.solid.validity().is_empty());
        let definition = Definition {
            imported: None,
            pipe: None,
            profile: Some(Box::new(profile.clone())),
            matching: None,
            placement: Default::default(),
            cage,
            parameters: Parameters { kind: "face".into(), ..Default::default() },
            hierarchy: None,
            symmetry: None,
        };
        let saved = serde_json::to_vec(&definition).unwrap();
        assert!(!serde_json::to_value(&definition).unwrap()["profile"].as_object().unwrap().contains_key("partition"));
        let restored: Definition = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored, definition);
        state.bodies[0] = crate::ModelBody::new("Support".into(), crate::kernel::box_solid(Vec3::ZERO, Vec3::new(12.0, 8.0, 3.0)).unwrap(), 1);
        let (changed, cage) = profile.recompute(&definition, &state).unwrap();
        assert!((cage.points[4].z - changed.rest[4].z - 1.0).abs() < 1e-9);
        assert!((changed.rest[4].z - 3.0).abs() < 1e-9);
        assert!(changed.boundary().iter().all(|&i| cage.points[i] == changed.rest[i]));
        assert!(Profile::seed(&state.bodies[0], top, [0, 2], 0).is_err());
        assert!(Profile::seed(&state.bodies[0], top, [2, 2], 2).is_err());
    }
    #[test]
    fn adjacent_matching_follows_neighboring_wall_jets_and_sews_the_same_conditions() {
        use solidcore_record::Vec3;
        let source = crate::kernel::box_solid(Vec3::new(-1.0, -1.0, -2.0), Vec3::new(1.0, 1.0, 0.0)).unwrap();
        let top = (0..source.face_count()).find(|&i| source.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let support = support::BoundarySupport::from_adjacent_faces(&source, top).unwrap();
        for t in [0.031, 0.27, 0.58, 0.89] {
            let target = support.target(t, &Placement::default(), 2).unwrap();
            assert!(target.normal.dot(Vec3::Z).abs() < 1e-9);
            assert!(!target.corner);
            assert!(target.normal.dot(target.tangent).abs() < 1e-9);
        }
        assert!(support.target(0.0, &Placement::default(), 2).unwrap().corner);
        let mut state = crate::ModelState::default();
        state.bodies.push(crate::ModelBody::new("Support".into(), source, 1));
        let reference = SupportRef::face(&state.bodies[0], top).unwrap();
        for order in [1, 2] {
            let parameters = Parameters { dimensions: [2.0; 3], segments: [3; 3], ..Default::default() };
            let cage = parameters.build().unwrap();
            let bottom: Vec<_> = cage
                .faces
                .iter()
                .enumerate()
                .filter(|(_, face)| face.iter().all(|&i| (cage.points[i].z + 1.0).abs() < 1e-9))
                .map(|(i, _)| i)
                .collect();
            let cage = cage.delete_faces(&bottom).unwrap();
            let ring = cage.boundary_loops().unwrap().remove(0);
            let definition = Definition {
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
                placement: Placement { origin: Vec3::Z, ..Default::default() },
                cage,
                parameters,
                symmetry: None,
                hierarchy: None,
            };
            let request =
                MatchRequest { source: reference.clone(), adjacent_faces: true, edge: [ring[0], ring[1]], order, tolerances: Default::default() };
            let (matched, report) = Matching::attach(&definition, &state, request, 0.01).unwrap_or_else(|e| panic!("adjacent G{order}: {e}"));
            assert!(report.position_mm <= 0.001 && report.tangent_deg <= 0.1);
            assert!(order < 2 || report.curvature_relative <= 0.01);
            assert!(matched.matching.as_ref().unwrap().adjacent_faces);
            let (cad, _) = convert(&matched, 0.01).unwrap();
            let (closed, cap) = cap(&matched, &state, &cad, 0.01).unwrap().unwrap();
            assert!(!closed.is_surface() && closed.validity().is_empty());
            assert_eq!(cap.face_count(), 1);
            assert_eq!(closed.face_count(), cad.face_count() + 1);
            assert!(crate::kernel::measure(&closed).unwrap().volume > 0.1);
            let mesh = cad.mesh(&crate::kernel::MeshTolerance::chord(0.01)).unwrap();
            for (i, faces) in mesh.edge_faces.iter().enumerate().filter(|(_, faces)| faces.len() == 1) {
                let curve = crate::kernel::edge_geometry(&cad, &[i]).unwrap().remove(0);
                let face = crate::kernel::face_geometry(&cad, faces[0] as usize).unwrap();
                for t in [0.25, 0.5, 0.75] {
                    let point = curve.at(t).unwrap().0;
                    let uv = face.parameters(point, None, 1e-7).unwrap();
                    let jet = face.surface.jet(uv[0], uv[1]).unwrap();
                    let normal = if (point.x.abs() - 1.0).abs() < 1e-5 { Vec3::X } else { Vec3::Y };
                    assert!(jet.normal().unwrap().dot(normal).abs().acos().to_degrees() < 0.11);
                    if order == 2 {
                        let along = normal.cross(Vec3::Z);
                        for (a, b) in [(along, along), (along, Vec3::Z), (Vec3::Z, Vec3::Z)] {
                            assert!(jet.curvature(a, b).unwrap().abs() < 0.004);
                        }
                    }
                }
            }
            let decoded: Definition = serde_json::from_value(serde_json::to_value(&matched).unwrap()).unwrap();
            assert!(decoded.matching.as_ref().unwrap().adjacent_faces);
        }
    }
    #[test]
    fn matching_survives_serialization_and_recomputes_native_edge_bindings() {
        use solidcore_record::Vec3;
        let state = |width: f64| {
            let mut state = crate::ModelState::default();
            state.bodies.push(crate::ModelBody::new("Support".into(), crate::kernel::box_solid(Vec3::ZERO, Vec3::new(width, 6.0, 8.0)).unwrap(), 1));
            state
        };
        let old = state(4.0);
        let body = &old.bodies[0];
        let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let source = SupportRef::face(body, top).unwrap();
        for order in 0..=2 {
            let parameters = Parameters { kind: "face".into(), dimensions: [4.0, 6.0, 1.0], segments: [2; 3], ..Default::default() };
            let definition = Definition {
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
                placement: Placement { origin: Vec3::new(2.0, 3.0, 3.0), ..Default::default() },
                cage: parameters.build().unwrap(),
                parameters,
                symmetry: None,
                hierarchy: None,
            };
            let ring = definition.cage.boundary_loops().unwrap().remove(0);
            let request =
                MatchRequest { adjacent_faces: false, source: source.clone(), edge: [ring[0], ring[1]], order, tolerances: Default::default() };
            let (matched, report) = Matching::attach(&definition, &old, request, 0.01).unwrap();
            assert!(report.position_mm <= 0.001);
            let matched: Definition = serde_json::from_value(serde_json::to_value(&matched).unwrap()).unwrap();
            let (resized, report) = matched
                .matching
                .as_ref()
                .unwrap()
                .recompute(&matched, &state(8.0), 0.01)
                .unwrap_or_else(|e| panic!("resize order {order}, metadata {:?}: {e}", matched.matching));
            assert!(report.position_mm <= 0.001);
            let (cad, _) = convert(&resized, 0.01).unwrap();
            assert!(cad.is_surface());
            let mesh = cad.mesh(&crate::kernel::MeshTolerance::chord(0.01)).unwrap();
            let boundary: Vec<_> = mesh.edges.iter().zip(&mesh.edge_faces).filter(|(_, f)| f.len() == 1).flat_map(|(e, _)| e).copied().collect();
            assert!(!boundary.is_empty());
            for point in &boundary {
                assert!((point.z - 8.0).abs() < 0.0011);
            }
            for corner in [Vec3::new(0.0, 0.0, 8.0), Vec3::new(8.0, 0.0, 8.0), Vec3::new(8.0, 6.0, 8.0), Vec3::new(0.0, 6.0, 8.0)] {
                assert!(boundary.iter().any(|p| p.dist(corner) < 0.0011), "missing resized corner {corner:?}");
            }
            assert!(matched.matching.as_ref().unwrap().recompute(&matched, &crate::ModelState::default(), 0.01).is_err());
            assert!(cad.face_count() <= matched.cage.faces.len());
        }
    }
    #[test]
    fn exact_face_and_reordered_wire_supports_match_a_placed_form_and_sew_to_cad() {
        use solidcore_record::Vec3;
        let body = crate::kernel::box_solid(Vec3::new(10.0, 20.0, 0.0), Vec3::new(14.0, 26.0, 8.0)).unwrap();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let face = support::BoundarySupport::from_face(&body, top).unwrap();
        let resized = crate::kernel::box_solid(Vec3::new(10.0, 20.0, 0.0), Vec3::new(18.0, 26.0, 8.0)).unwrap();
        let resized = support::BoundarySupport::from_face(&resized, top).unwrap();
        for curve in 0..4 {
            let coordinate = support::BoundaryCoordinate { curve, fraction: 0.0 };
            let old = face.at(face.fraction(&coordinate).unwrap()).unwrap().0;
            let new = resized.at(resized.fraction(&coordinate).unwrap()).unwrap().0;
            assert!(new.dist(Vec3::new(10.0 + (old.x - 10.0) * 2.0, old.y, old.z)) < 1e-8);
        }
        for fraction in [0.0, 0.13, 0.37, 0.88] {
            let coordinate = face.coordinate(fraction).unwrap();
            assert!((face.fraction(&coordinate).unwrap() - fraction).abs() < 1e-12);
        }
        let mesh = body.mesh(&crate::kernel::MeshTolerance::chord(0.01)).unwrap();
        let edges: Vec<_> = mesh.edge_faces.iter().enumerate().filter(|(_, faces)| faces.contains(&(top as u32))).map(|(i, _)| i).rev().collect();
        assert_eq!(edges.len(), 4);
        let wire = support::BoundarySupport::from_edges(&body, &edges).unwrap();
        assert!(face.supports_curvature() && wire.supports_curvature());
        assert!(support::BoundarySupport::from_edges(&body, &edges[..2]).is_err());
        let placement = Placement { origin: Vec3::new(12.0, 23.0, 3.0), ..Default::default() };
        for support in [&face, &wire] {
            for t in [0.0, 0.21, 0.57, 0.99] {
                let target = support.target(t, &placement, 2).unwrap();
                assert!((target.point.z - 5.0).abs() < 1e-8);
                assert!((target.normal.dot(Vec3::Z).abs() - 1.0).abs() < 1e-8);
                assert!(target.support.curvature(Vec3::X, Vec3::X).unwrap().abs() < 1e-10);
            }
        }
        for order in 0..=2 {
            let parameters = Parameters { kind: "face".into(), dimensions: [4.0, 6.0, 1.0], segments: [2; 3], ..Default::default() };
            let definition = Definition {
                placement: placement.clone(),
                cage: parameters.build().unwrap(),
                parameters,
                symmetry: None,
                hierarchy: None,
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
            };
            let ring = definition.cage.boundary_loops().unwrap().remove(0);
            let config = face.initialize(&definition.cage, &placement, [ring[0], ring[1]], order, Default::default()).unwrap();
            let result = face.solve(&definition, &config, 0.01).unwrap();
            assert!(result.report.position_mm <= 0.001);
            assert!(order == 0 || result.report.tangent_deg <= 0.1);
            assert!(order < 2 || result.report.curvature_relative <= 0.01);
            let final_definition = Definition { cage: result.cage, ..definition };
            let (cad, report) = convert(&final_definition, 0.01).unwrap();
            assert!(!cad.is_mesh() && cad.is_surface() && !report.solid);
            let mesh = cad.mesh(&crate::kernel::MeshTolerance::chord(0.01)).unwrap();
            for (edge, faces) in mesh.edges.iter().zip(&mesh.edge_faces) {
                if faces.len() == 1 {
                    assert!(edge.iter().all(|p| (p.z - 8.0).abs() < 0.001));
                }
            }
            if order > 0 {
                for i in 0..cad.face_count() {
                    let geometry = crate::kernel::face_geometry(&cad, i).unwrap();
                    let jet = geometry.surface.jet(0.37, 0.61).unwrap();
                    assert!((jet.point.z - 8.0).abs() < 1e-7);
                    assert!((jet.normal().unwrap().dot(Vec3::Z).abs() - 1.0).abs() < 1e-7);
                    assert!(jet.curvature(Vec3::X, Vec3::X).unwrap().abs() < 1e-7);
                    assert!(jet.curvature(Vec3::X, Vec3::Y).unwrap().abs() < 1e-7);
                    assert!(jet.curvature(Vec3::Y, Vec3::Y).unwrap().abs() < 1e-7);
                }
            }
        }
    }
    #[test]
    fn rigid_placement_moves_the_cad_sheet_and_loads_older_local_cages() {
        use solidcore_record::Vec3;
        let parameters = Parameters { kind: "face".into(), dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
        let cage = parameters.build().unwrap().with_creases(&[], &[0, 1, 2, 3], 10.0).unwrap();
        let matrix = crate::rigid(Vec3::new(30.0, 40.0, 50.0), Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_2);
        let definition = Definition {
            placement: Placement::from_matrix(&matrix).unwrap(),
            cage,
            parameters,
            symmetry: None,
            hierarchy: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let (body, report) = convert(&definition, 0.01).unwrap();
        assert!(!report.solid && body.is_surface());
        let mesh = body.mesh(&crate::kernel::MeshTolerance::chord(0.01)).unwrap();
        let bounds: solidcraft_geom::Aabb3 = mesh.positions.iter().copied().collect();
        assert!(bounds.center().dist(Vec3::new(30.0, 40.0, 50.0)) < 1e-9);
        assert!(bounds.size().dist(Vec3::new(10.0, 0.0, 10.0)) < 1e-8);
        assert!(mesh.normals.iter().all(|n| n.dot(-Vec3::Y) > 1.0 - 1e-9));
        let mut old = serde_json::to_value(&definition).unwrap();
        old.as_object_mut().unwrap().remove("placement");
        let old: Definition = serde_json::from_value(old).unwrap();
        assert!(old.placement.is_identity());
        assert_eq!(old.cage, definition.cage);
        let mut invalid = crate::IDENTITY;
        invalid[0][0] = -1.0;
        assert!(Placement::from_matrix(&invalid).is_err());
        invalid[0][0] = 2.0;
        assert!(Placement::from_matrix(&invalid).is_err());
    }
    #[test]
    fn sharp_thickened_sheet_is_a_valid_cad_solid_of_the_requested_height() {
        let parameters = Parameters { kind: "face".into(), dimensions: [10.0; 3], ..Default::default() };
        let cage = parameters.build().unwrap().thicken(2.0, true).unwrap();
        let definition = Definition {
            placement: Default::default(),
            cage,
            parameters,
            symmetry: None,
            hierarchy: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let (body, report) = convert(&definition, 0.02).unwrap();
        assert!(report.solid && !body.is_mesh() && !body.is_surface());
        let mesh = body.mesh(&crate::kernel::MeshTolerance::chord(0.02)).unwrap();
        let low = mesh.positions.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
        let high = mesh.positions.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
        assert!(low.abs() < 1e-8 && (high - 2.0).abs() < 1e-8);
        let volume = crate::kernel::measure(&body).unwrap().volume;
        assert!(volume > 100.0 && volume < 201.0, "volume {volume}");
    }
    #[test]
    fn forms_deleted_local_leaf_is_an_open_cad_shell_with_surviving_identities() {
        let parameters = Parameters { dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
        let original = solidcraft_geom::subdivision::hierarchy::Hierarchy::new(parameters.build().unwrap()).unwrap();
        let refined = original.subdivide(&[0], [2, 1]).unwrap();
        let hierarchy = refined.delete_faces(&[6]).unwrap();
        let definition = Definition {
            placement: Default::default(),
            cage: hierarchy.evaluate().unwrap(),
            parameters,
            symmetry: None,
            hierarchy: Some(hierarchy),
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let (body, report) = convert(&definition, 0.05).unwrap();
        assert!(!report.solid && body.is_surface() && !body.is_mesh());
        assert_eq!(report.patch_roots.len(), body.face_count());
        assert_eq!(report.patch_roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), [1, 2, 3, 4, 5, 7].into_iter().collect());
        assert!(!body.mesh(&crate::kernel::MeshTolerance::chord(0.05)).unwrap().triangles.is_empty());
        assert_eq!(serde_json::from_str::<Definition>(&serde_json::to_string(&definition).unwrap()).unwrap(), definition);
    }
    #[test]
    fn a_basic_extruded_face_finishes_as_a_valid_cad_solid() {
        let parameters = Parameters::default();
        let cage = parameters.build().unwrap().extrude_faces(&[0], solidcore_record::Vec3::X * 8.0).unwrap();
        let definition = Definition {
            placement: Default::default(),
            cage,
            parameters,
            symmetry: None,
            hierarchy: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let started = std::time::Instant::now();
        let (body, report) = convert(&definition, 0.01).unwrap();
        eprintln!("basic extrusion CAD conversion: {:?}, {} patches", started.elapsed(), report.patches);
        assert!(report.solid && !body.is_mesh() && !body.is_surface());
        assert!(report.sampled_deviation_mm <= 0.01);
        assert_eq!(report.patch_roots.len(), body.face_count());
        assert_eq!(report.patch_roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), (0..28).collect());
    }
    #[test]
    fn hierarchical_form_sews_and_retains_logical_leaf_ids() {
        let parameters = Parameters { dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
        let hierarchy =
            solidcraft_geom::subdivision::hierarchy::Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[0], [2, 1]).unwrap();
        let definition = Definition {
            placement: Default::default(),
            cage: hierarchy.evaluate().unwrap(),
            parameters,
            symmetry: None,
            hierarchy: Some(hierarchy),
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let (body, report) = convert(&definition, 0.05).unwrap();
        assert!(report.trimmed_roots);
        assert_eq!(report.root_surfaces, 6);
        assert_eq!(body.face_count(), 7);
        assert!(!body.is_mesh() && !body.is_surface());
        assert!(body.validity().is_empty());
        assert_eq!(report.patch_roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), [1, 2, 3, 4, 5, 6, 7].into_iter().collect());
        assert_eq!(serde_json::from_str::<Definition>(&serde_json::to_string(&definition).unwrap()).unwrap(), definition);

        let mut document = crate::Document::new("Refined Form");
        let id = document
            .add_feature(
                crate::FeatureKind::Form {
                    definition: Box::new(definition),
                    tolerance: "0.05 mm".into(),
                    operation: crate::Operation::NewBody,
                    targets: Vec::new(),
                },
                None,
            )
            .unwrap();
        let saved = serde_json::to_vec(&document).unwrap();
        let reopened: crate::Document = serde_json::from_slice(&saved).unwrap();
        assert_eq!(reopened.feature(id), document.feature(id));
    }
    #[test]
    fn nested_sculpted_leaves_are_exact_root_trims_and_deleted_regions_stay_open() {
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;
        let parameters = Parameters { dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
        let mut h = Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[0], [2, 1]).unwrap().subdivide(&[6], [2, 1]).unwrap();
        let vertex = *h.controls.iter().find(|(_, level)| **level == 2).unwrap().0;
        h.add_deltas(&[vertex], &[solidcore_record::Vec3::new(0.15, 0.1, 0.2)]).unwrap();
        for deleted in [false, true] {
            let hierarchy = if deleted { h.delete_faces(&[8]).unwrap() } else { h.clone() };
            let definition = Definition {
                placement: Default::default(),
                cage: hierarchy.evaluate().unwrap(),
                parameters: parameters.clone(),
                symmetry: None,
                hierarchy: Some(hierarchy),
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
            };
            let (body, report) = convert(&definition, 0.05).unwrap();
            assert!(report.trimmed_roots);
            assert_eq!(report.root_surfaces, 6);
            assert_eq!(body.face_count(), if deleted { 7 } else { 8 });
            assert_eq!(report.solid, !deleted);
            assert_eq!(body.is_surface(), deleted);
            assert_eq!(report.patch_roots.len(), body.face_count());
            assert_eq!(report.patches, body.face_count());
            assert_eq!(report.patch_roots.iter().filter(|&&id| id == 8).count(), usize::from(!deleted));
        }
    }
    #[test]
    fn dissolved_polygon_converts_without_losing_its_logical_root() {
        let parameters = Parameters { dimensions: [10.0; 3], ..Default::default() };
        let cage = parameters.build().unwrap();
        let edges = cage.edges().unwrap();
        let edge = edges.into_iter().find(|e| e.iter().all(|v| cage.faces.iter().filter(|f| f.contains(v)).count() == 4)).unwrap();
        let cage = cage.dissolve_edge(edge).unwrap();
        assert_eq!(cage.faces.len(), 23);
        let definition = Definition {
            placement: Default::default(),
            cage,
            parameters,
            symmetry: None,
            hierarchy: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let (body, report) = convert(&definition, 0.05).unwrap();
        assert!(!body.is_mesh());
        assert!(!body.is_surface());
        assert_eq!(report.patch_roots.len(), body.face_count());
        assert_eq!(report.patch_roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), (0..23).collect());
        assert!(report.patch_roots.iter().filter(|&&root| root == 22).count() > 1);
    }
    #[test]
    fn sculpted_open_form_preserves_logical_owners_and_the_total_cad_fit_budget() {
        for picked in [2, 4] {
            let parameters = Parameters { kind: "face".into(), dimensions: [10.0; 3], segments: [2; 3], ..Default::default() };
            let mut cage = parameters.build().unwrap();
            cage.points[picked].z = 2.0;
            let definition = Definition {
                cage,
                parameters,
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
                placement: Default::default(),
                symmetry: None,
                hierarchy: None,
            };
            let curves = edge_curves(&definition, 0.05, &definition.cage.edges().unwrap()).unwrap();
            let original_error = definition.cage.fit_patches(0.05).unwrap().sampled_deviation;
            let (body, report) = convert(&definition, 0.05).unwrap();
            assert_eq!(report.sampled_deviation_mm, original_error);
            assert_eq!(body.face_count(), 20);
            assert!(!body.is_mesh() && body.is_surface());
            assert!(!report.solid && report.sampled_deviation_mm <= 0.05);
            assert_eq!(report.patch_roots.len(), body.face_count());
            assert_eq!(report.patches, body.face_count());
            assert_eq!(report.patch_roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), (0..4).collect());
            assert!(body.mesh(&crate::kernel::MeshTolerance::chord(0.01)).unwrap().positions.iter().any(|p| p.z > 0.1));
            let boundary = crate::kernel::edge_geometry(&body, &(0..body.edge_count()).collect::<Vec<_>>()).unwrap();
            // Source picking and Insert Point keep their original fitted curves:
            // the alternate CAD chart keeps those curves as its exact rims.
            for source in curves {
                for f in [0.13, 0.57, 0.91] {
                    let point = source.at(f).unwrap();
                    assert!(boundary.iter().any(|curve| {
                        let distance = |t: f64| curve.at(t).unwrap().0.dist(point);
                        let best: usize = (0..=32).min_by(|a, b| distance(*a as f64 / 32.0).total_cmp(&distance(*b as f64 / 32.0))).unwrap();
                        let mut lo = best.saturating_sub(1) as f64 / 32.0;
                        let mut hi = (best + 1).min(32) as f64 / 32.0;
                        for _ in 0..72 {
                            let a = lo + (hi - lo) / 3.0;
                            let b = hi - (hi - lo) / 3.0;
                            if distance(a) < distance(b) {
                                hi = b;
                            } else {
                                lo = a;
                            }
                        }
                        distance((lo + hi) * 0.5) < 1e-8
                    }));
                }
            }
        }
    }

    #[test]
    fn subdivision_box_is_a_cad_solid_and_plane_stays_open() {
        for kind in ["box", "plane"] {
            let parameters = Parameters { kind: kind.into(), dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
            let definition = Definition {
                placement: Default::default(),
                cage: parameters.build().unwrap(),
                parameters,
                symmetry: None,
                hierarchy: None,
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
            };
            let (body, report) = convert(&definition, 0.05).unwrap();
            assert!(!body.is_mesh());
            assert_eq!(body.is_surface(), kind == "plane");
            assert_eq!(report.solid, kind == "box");
            let mesh = body.mesh(&crate::kernel::MeshTolerance::chord(0.05)).unwrap();
            assert!(!mesh.triangles.is_empty());
            if report.solid {
                let properties = crate::kernel::measure(&body).unwrap();
                assert!(properties.volume > 0.0);
                assert_eq!(properties.faces, 6);
            }
        }
    }
}
