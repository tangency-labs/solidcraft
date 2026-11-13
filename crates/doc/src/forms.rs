//! Editable Forms source data and conversion through the existing kernel boundary.
use crate::{DocError, Result};
use serde::{Deserialize, Serialize};
use solidcraft_geom::prelude::*;
use solidcraft_geom::subdivision::{Cage, primitives::Parameters};

mod association;
mod capping;
mod collapse;
mod edge_curve;
mod pipe;
mod placement;
mod planar_wall;
mod preview_compaction;
mod profile;
mod quad_cap;
mod selection_frame;
mod selection_guides;
pub mod support;
mod unweld;
mod weld_native;
mod weld_seam;
pub use association::{MatchRequest, Matching, NamedEdge, SupportRef, match_openings};
pub(crate) use capping::cap;
pub use collapse::{PreparedBoundaryCollapse, author_boundary_collapse, prepare_boundary_collapse};
pub use edge_curve::{EdgeCurve, edge_curves};
pub use pipe::{Pipe, PipeSource};
pub use placement::Placement;
pub(crate) use planar_wall::build as build_planar_wall;
pub use planar_wall::can_build_planar_wall;
pub use profile::{Profile, ProfileDisplay, ProfileFit};
pub use quad_cap::{PreparedQuadCap, author_quad_cap, prepare_quad_cap};
pub use selection_frame::SelectionFrames;
pub use selection_guides::{NativeGuide, NativeGuides, SelectionGuides};
pub use unweld::prepare_refined_unweld;
pub use weld_native::{PreparedRefinedWeld, RefinedWeldFace, prepare_refined_weld};
pub use weld_seam::RefinedWeldSeam;

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
        if hierarchy.has_boundary_collapse() && (definition.matching.is_some() || definition.symmetry.is_some() || definition.pipe.is_some()) {
            return Err(DocError::Invalid("a retained Collapse requires an independent Form without symmetry".into()));
        }
        if hierarchy.has_quad_cap() && (definition.matching.is_some() || definition.symmetry.is_some()) {
            return Err(DocError::Invalid("a derived quad cap requires an independent Form without symmetry".into()));
        }
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
    let (body, report, _) = convert_source(definition, tolerance)?;
    Ok((body, report))
}
/// Exact skin and logical guide routes from the very same source fit.
pub fn convert_with_guides(definition: &Definition, tolerance: f64) -> Result<(crate::kernel::Body, ConversionReport, SelectionGuides)> {
    let (body, report, source) = convert_source(definition, tolerance)?;
    let guides = selection_guides::from_source(definition, source);
    Ok((body, report, guides))
}
/// Exact logical editing frames derived from the accepted fitted source. The
/// axes are Form-local, including when the sewn native body uses new charts.
pub fn convert_with_selection(
    definition: &Definition,
    tolerance: f64,
) -> Result<(crate::kernel::Body, ConversionReport, SelectionGuides, SelectionFrames)> {
    let (body, report, source) = convert_source_with_preview(definition, tolerance, true)?;
    let mut frames = SelectionFrames::from_faces(definition, &source);
    let guides = selection_guides::from_source(definition, source);
    frames.add_guides(definition, &guides);
    Ok((body, report, guides, frames))
}
fn convert_source(definition: &Definition, tolerance: f64) -> Result<(crate::kernel::Body, ConversionReport, FittedSource)> {
    convert_source_with_preview(definition, tolerance, false)
}
fn convert_source_with_preview(
    definition: &Definition,
    tolerance: f64,
    compact_preview: bool,
) -> Result<(crate::kernel::Body, ConversionReport, FittedSource)> {
    validate_definition(definition)?;
    #[cfg(test)]
    let timing = std::time::Instant::now();
    let caps = definition
        .hierarchy
        .as_ref()
        .map(|h| h.validated_quad_caps())
        .transpose()
        .map_err(|error| DocError::Invalid(error.to_string()))?
        .unwrap_or_default();
    let mut source = fitted_source(definition, tolerance)?;
    for (face, rim) in caps {
        quad_cap::FittedQuadCap::append(definition, &mut source, face, rim)?;
    }
    let site =
        definition.hierarchy.as_ref().map(|h| h.validated_boundary_collapse()).transpose().map_err(|e| DocError::Invalid(e.to_string()))?.flatten();
    let collapsed = site.map(|site| collapse::stitch(&mut source, site)).transpose()?;
    let FittedSource { fit, sheets, trimmed_roots, quad_caps } = &source;
    #[cfg(test)]
    eprintln!("Forms fit ({} patches): {:?}", fit.patches.len(), timing.elapsed());
    // Caps add sewing operands. The fitted skin's patches, charts and owners stay aligned.
    let sheet_roots = if quad_caps.is_empty() {
        std::borrow::Cow::Borrowed(fit.patch_roots.as_slice())
    } else {
        std::borrow::Cow::Owned(fit.patch_roots.iter().copied().chain(quad_caps.iter().map(|cap| cap.face)).collect())
    };
    let prepared = if let Some((body, owners)) = collapsed {
        Some((body, owners, fit.sampled_deviation))
    } else if compact_preview {
        preview_compaction::stitch(definition, &source, tolerance)
    } else {
        None
    };
    let (mut body, patch_roots, sampled_deviation) = match prepared {
        Some(preview) => preview,
        None => {
            let body = crate::kernel::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7)?;
            let owners = stitched_roots(&body, sheets, &sheet_roots)?;
            (body, owners, fit.sampled_deviation)
        }
    };
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
    if !quad_caps.is_empty() {
        quad_cap::qualify(quad_caps, &body, &patch_roots, &sheet_roots, closed)?;
    }
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
            sampled_deviation_mm: sampled_deviation,
            subdivision_level: fit.subdivision_level,
            patches: patch_roots.len(),
            root_surfaces: fit.patches.len() + quad_caps.len(),
            trimmed_roots: *trimmed_roots,
            patch_roots,
            solid: closed,
        },
        source,
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
    quad_caps: Vec<quad_cap::FittedQuadCap>,
}
fn fitted_source(definition: &Definition, tolerance: f64) -> Result<FittedSource> {
    if let Some(hierarchy) = &definition.hierarchy
        && hierarchy.has_trim_wall()
    {
        if definition.matching.is_some() || definition.symmetry.is_some() {
            return Err(DocError::Invalid("trim walls require a Form without Match or symmetry".into()));
        }
        let (fit, sheets) = root_sheets(hierarchy, tolerance)?;
        return Ok(FittedSource { fit, sheets, trimmed_roots: true, quad_caps: Vec::new() });
    }
    if let Some(matching) = &definition.matching {
        let mut fit = matching.cad_fit(definition, tolerance)?;
        if let Some(hierarchy) = &definition.hierarchy {
            filter_evaluator_patches(&mut fit, hierarchy)?;
        }
        let sheets = fitted_sheets(&mut fit, tolerance)?;
        return Ok(FittedSource { fit, sheets, trimmed_roots: false, quad_caps: Vec::new() });
    }
    if let Some(hierarchy) = &definition.hierarchy
        && let Ok((fit, sheets)) = root_sheets(hierarchy, tolerance)
    {
        return Ok(FittedSource { fit, sheets, trimmed_roots: true, quad_caps: Vec::new() });
    }
    let mut fit = definition.cage.fit_patches(tolerance).map_err(|e| DocError::Invalid(e.to_string()))?;
    if let Some(hierarchy) = &definition.hierarchy {
        filter_evaluator_patches(&mut fit, hierarchy)?;
    }
    let sheets = fitted_sheets(&mut fit, tolerance)?;
    Ok(FittedSource { fit, sheets, trimmed_roots: false, quad_caps: Vec::new() })
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
            crate::walk::Walk::walk_refs(profile.as_ref(), f);
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
            crate::walk::Walk::walk_refs_mut(profile.as_mut(), f);
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
            crate::walk::Walk::walk_geometry(profile.as_ref(), f);
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
            crate::walk::Walk::walk_geometry_mut(profile.as_mut(), f);
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
        let solved = crate::SolvedSketch {
            feature: 7,
            name: "Path".into(),
            plane: Plane::XY,
            sketch,
            report,
            profiles: Vec::new(),
            profiles_3d: Vec::new(),
            ends: Default::default(),
        };
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
    fn surface_rim_parameters_keep_exact_closed_seam_identity_and_refuse_unrelated_rewrites() {
        use solidcore_record::Vec3;
        let body = crate::kernel::cylinder(Vec3::ZERO, Vec3::Z, 3.0, 2.0).unwrap();
        let source = body.record().edges.iter().find(|edge| edge.vertices[0] == edge.vertices[1]).unwrap();
        let [low, high] = source.interval;
        let middle = 0.5 * low + 0.5 * high;
        let parameter = super::profile::quilt::rim_parameter;
        assert_eq!(parameter(source, low, [middle, high]), Some(high));
        assert_eq!(parameter(source, high, [low, middle]), Some(low));
        assert_eq!(parameter(source, middle, [low, middle]), Some(middle));
        assert_eq!(parameter(source, middle, [middle + 0.25 * (high - low), high]), None, "interior parameters never wrap");
        assert_eq!(parameter(source, f64::from_bits(low.to_bits() + 1), [middle, high]), None, "near an endpoint is not that endpoint");
        let mut open = source.clone();
        open.vertices[1] = open.vertices[0] + 1;
        assert_eq!(parameter(&open, low, [middle, high]), None, "an open edge does not establish a seam alias");
        assert_eq!(parameter(source, low, [middle, high - 0.01 * (high - low)]), None, "the other exact endpoint must belong to the known interval");
        assert_eq!(parameter(source, f64::NAN, [low, high]), None);
    }
    #[test]
    fn circular_surface_guides_fit_across_the_cad_quilt_and_display_on_its_exact_charts() {
        use solidcore_record::Vec3;
        let mut state = crate::ModelState::default();
        let body = crate::kernel::cylinder(Vec3::ZERO, Vec3::Z, 3.0, 2.0).unwrap();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        state.bodies.push(crate::ModelBody::new("Support".into(), body, 1));
        let (mut profile, cage) = Profile::seed(&state.bodies[0], top, [3, 2], 0).unwrap();
        profile.partition = false;
        let source = serde_json::to_value((&profile, &cage)).unwrap();
        let interior: Vec<_> = (0..cage.points.len()).filter(|i| !profile.boundary().contains(i)).collect();
        assert_eq!(interior.len(), 2);
        assert!(interior.iter().all(|&i| cage.points[i].x.hypot(cage.points[i].y) < 3.0));
        let ring = cage.boundary_loops().unwrap().pop().unwrap();
        let perimeter: std::collections::BTreeSet<_> =
            ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()).map(|(&a, &b)| [a.min(b), a.max(b)]).collect();
        let mut fixed_rim = None;
        for sculpt in [false, true] {
            let mut edited = cage.clone();
            if sculpt {
                edited.points[interior[0]] += Vec3::new(0.12, -0.08, 0.8);
                edited.points[interior[1]] += Vec3::new(-0.09, 0.06, 0.35);
            }
            let before = serde_json::to_value((&profile, &edited)).unwrap();
            let fit = profile.fit(&state, &edited, &Placement::default(), 0.01).unwrap();
            assert!(fit.patch.face_count() > 1, "the actual circular fill must exercise multiple native charts");
            assert!(!fit.solid.is_surface() && fit.solid.shell_count() == 1 && fit.solid.validity().is_empty());
            assert!(fit.report.gap <= 0.01 && fit.report.point_gap <= 0.01);
            let faces: Vec<_> = (0..fit.patch.face_count()).map(|face| crate::kernel::face_geometry(&fit.patch, face).unwrap()).collect();
            for &i in &interior {
                assert!(
                    faces.iter().any(|face| face.parameters(edited.points[i], None, 0.01).is_ok()),
                    "authored guide {i} must lie on an actual exact patch face"
                );
            }
            if !sculpt {
                assert!(
                    interior.iter().any(|&i| faces[0].parameters(edited.points[i], None, 0.01).is_err()),
                    "a face-zero-only check must refuse this actual fixture"
                );
            }
            let (mesh, report) = profile.display(&state, &edited, &Placement::default(), 0.01).unwrap();
            let rim = super::profile::QuiltRim::new(&profile, &edited, &Placement::default(), &state.bodies[0].body, top).unwrap();
            let logical_edges = edited.edges().unwrap();
            let originals: Vec<_> = logical_edges
                .iter()
                .map(|&edge| rim.stations(edge).unwrap().into_iter().map(|t| rim.sample(edge, t).unwrap()).collect::<Vec<_>>())
                .collect();
            let native_before = serde_json::to_value(fit.patch.record()).unwrap();
            let tolerance = crate::kernel::MeshTolerance::chord(0.01);
            let cached = fit.patch.keeping_meshes();
            let ordinary_before = cached.mesh_plan(&tolerance).unwrap();
            let ordinary_mesh_before = serde_json::to_value(cached.mesh(&tolerance).unwrap()).unwrap();
            let raw_guides = super::profile::quilt_guide_edges(&fit.patch, &edited, &Placement::default(), &logical_edges, &rim).unwrap();
            let sampled = super::profile::quilt::display(&cached, &fit.open, &state.bodies[0].body, &logical_edges, &rim, &raw_guides, 0.01).unwrap();
            assert_eq!(
                cached.mesh_plan(&tolerance).unwrap(),
                ordinary_before,
                "a guide-dependent temporary plan must not enter the ordinary tolerance-only memo"
            );
            assert_eq!(
                serde_json::to_value(cached.mesh(&tolerance).unwrap()).unwrap(),
                ordinary_mesh_before,
                "guided display must not replace the ordinary tolerance-only mesh memo"
            );
            assert_eq!(serde_json::to_value(fit.patch.record()).unwrap(), native_before, "display constraints must not author CAD edges or faces");
            let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
            let side = |a: Vec3, b: Vec3| {
                let (a, b) = (key(a), key(b));
                if a < b { [a, b] } else { [b, a] }
            };
            let triangle_sides: std::collections::BTreeSet<_> = sampled
                .mesh
                .triangles
                .iter()
                .flat_map(|&[a, b, c]| {
                    let [a, b, c] = [a, b, c].map(|id| sampled.mesh.positions[id as usize]);
                    [side(a, b), side(b, c), side(c, a)]
                })
                .collect();
            for curve in &sampled.guides {
                for pair in curve.windows(2) {
                    assert!(triangle_sides.contains(&side(pair[0], pair[1])), "each quilt guide chord must be a bit-identical skin triangle side");
                }
            }
            for (edge, curve) in sampled.mesh.edges.iter().enumerate() {
                let owners = &sampled.mesh.edge_faces[edge];
                if owners.len() != 2 {
                    continue;
                }
                for &point in curve {
                    for &owner in owners {
                        assert!(
                            sampled
                                .mesh
                                .triangles
                                .iter()
                                .zip(&sampled.mesh.tri_face)
                                .any(|(triangle, &face)| face == owner && triangle.iter().any(|&id| sampled.mesh.positions[id as usize] == point)),
                            "both incident charts must use the identical canonical native seam node"
                        );
                    }
                }
            }
            assert!(!mesh.triangles.is_empty());
            assert_eq!(mesh.edges.len(), edited.edges().unwrap().len());
            assert_eq!(report.patch_roots, vec![0; fit.patch.face_count()]);
            assert!(report.sampled_deviation_mm <= 0.01);
            let curves: Vec<_> = edited
                .edges()
                .unwrap()
                .iter()
                .zip(&mesh.edges)
                .filter(|(edge, _)| perimeter.contains(*edge))
                .map(|(_, curve)| curve.clone())
                .collect();
            assert_eq!(curves.len(), ring.len());
            for curve in &curves {
                for point in curve {
                    assert!(
                        (point.x.hypot(point.y) - 3.0).abs() < 1e-7 && (point.z - 2.0).abs() < 1e-7,
                        "every perimeter sample must stay on the original exact circle"
                    );
                }
            }
            let logical_rim: Vec<_> = logical_edges
                .iter()
                .zip(&originals)
                .zip(&mesh.edges)
                .filter(|((edge, _), _)| perimeter.contains(*edge))
                .map(|((_, original), curve)| {
                    let original: Vec<_> = original.iter().copied().flatten().collect();
                    assert!(
                        original.iter().all(|point| curve.iter().any(|sample| sample.dist(*point) < 1e-12)),
                        "co-discretization must retain every immutable original rim station"
                    );
                    original
                })
                .collect();
            if let Some(before) = &fixed_rim {
                assert_eq!(&logical_rim, before, "asymmetric sculpt must not re-associate fixed rim arcs");
            } else {
                fixed_rim = Some(logical_rim);
            }
            for &guide in &interior {
                assert!(edited.edges().unwrap().iter().zip(&mesh.edges).any(|(edge, curve)| {
                    edge[0] == guide && curve[0].dist(edited.points[guide]) <= 0.01
                        || edge[1] == guide && curve.last().unwrap().dist(edited.points[guide]) <= 0.01
                }));
            }
            for curve in &mesh.edges {
                assert!(
                    curve.len() >= 25,
                    "native seam stations and exact skin intersections may add display nodes, never omit the original guide stations"
                );
                for &point in curve {
                    assert!(point.is_finite());
                    assert!(
                        faces.iter().any(|face| face.parameters(point, None, 1e-7).is_ok()),
                        "display guides must remain on the actual exact quilt"
                    );
                }
            }
            assert_eq!(serde_json::to_value((&profile, &edited)).unwrap(), before);
        }
        assert_eq!(serde_json::to_value((&profile, &cage)).unwrap(), source);
        // With one V row, an interior edge can have two fixed endpoints. It is
        // still a guide chord and must not become an extra perimeter arc.
        let (profile, thin) = Profile::seed(&state.bodies[0], top, [3, 1], 0).unwrap();
        let rim = super::profile::QuiltRim::new(&profile, &thin, &Placement::default(), &state.bodies[0].body, top).unwrap();
        let loop_ = thin.boundary_loops().unwrap().pop().unwrap();
        let perimeter: std::collections::BTreeSet<_> =
            loop_.iter().zip(loop_.iter().cycle().skip(1)).take(loop_.len()).map(|(&a, &b)| [a.min(b), a.max(b)]).collect();
        let edge = thin.edges().unwrap().into_iter().find(|e| !perimeter.contains(e)).unwrap();
        assert!(edge.iter().all(|i| profile.boundary().contains(i)));
        assert!(rim.sample(edge, 0.0).unwrap().is_some() && rim.sample(edge, 1.0).unwrap().is_some());
        assert!(rim.sample(edge, 0.5).unwrap().is_none());
    }
    #[test]
    fn circular_surface_default_and_denser_guides_share_the_native_skin_before_and_after_sculpt() {
        use solidcore_record::Vec3;
        let mut state = crate::ModelState::default();
        let body = crate::kernel::cylinder(Vec3::ZERO, Vec3::Z, 3.0, 2.0).unwrap();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        state.bodies.push(crate::ModelBody::new("Support".into(), body, 1));
        for segments in [[2, 2], [4, 4]] {
            let (mut profile, cage) = Profile::seed(&state.bodies[0], top, segments, 0).unwrap();
            profile.partition = false;
            let immutable = serde_json::to_value((&profile, &cage, state.bodies[0].body.record())).unwrap();
            let boundary = profile.boundary();
            let centre = (0..cage.points.len())
                .filter(|id| !boundary.contains(id))
                .min_by(|&a, &b| cage.points[a].x.hypot(cage.points[a].y).total_cmp(&cage.points[b].x.hypot(cage.points[b].y)))
                .unwrap();
            let ring = cage.boundary_loops().unwrap().pop().unwrap();
            let perimeter: std::collections::BTreeSet<_> =
                ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()).map(|(&a, &b)| [a.min(b), a.max(b)]).collect();
            for sculpt in [false, true] {
                let mut edited = cage.clone();
                if sculpt {
                    edited.points[centre] += Vec3::new(0.13, -0.07, 0.45);
                }
                let authored = serde_json::to_value((&profile, &edited)).unwrap();
                let fit = profile
                    .fit(&state, &edited, &Placement::default(), 0.01)
                    .unwrap_or_else(|error| panic!("{segments:?} sculpt={sculpt} fit: {error}"));
                assert!(fit.patch.face_count() > 1, "the actual density must exercise a quilt");
                let native = serde_json::to_value(fit.patch.record()).unwrap();
                let rim = super::profile::QuiltRim::new(&profile, &edited, &Placement::default(), &state.bodies[0].body, top).unwrap();
                let edges = edited.edges().unwrap();
                let original = super::profile::quilt_guide_edges(&fit.patch, &edited, &Placement::default(), &edges, &rim).unwrap();
                let sampled = super::profile::quilt::display(&fit.patch, &fit.open, &state.bodies[0].body, &edges, &rim, &original, 0.01)
                    .unwrap_or_else(|error| panic!("{segments:?} sculpt={sculpt} direct route: {error}"));
                assert_eq!(sampled.guides.len(), edges.len());
                let key = |point: Vec3| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
                let side = |a: Vec3, b: Vec3| {
                    let (a, b) = (key(a), key(b));
                    if a < b { [a, b] } else { [b, a] }
                };
                let sides: std::collections::BTreeSet<_> = sampled
                    .mesh
                    .triangles
                    .iter()
                    .flat_map(|&[a, b, c]| {
                        let [a, b, c] = [a, b, c].map(|id| sampled.mesh.positions[id as usize]);
                        [side(a, b), side(b, c), side(c, a)]
                    })
                    .collect();
                for (&edge, curve) in edges.iter().zip(&sampled.guides) {
                    assert!(curve.len() >= 2);
                    for pair in curve.windows(2) {
                        assert!(sides.contains(&side(pair[0], pair[1])), "{segments:?} sculpt={sculpt}: every chord must be an actual skin side");
                    }
                    if perimeter.contains(&edge) {
                        for point in curve {
                            assert!(
                                (point.x.hypot(point.y) - 3.0).abs() < 1e-7 && (point.z - 2.0).abs() < 1e-7,
                                "all perimeter samples remain on the fixed original circle"
                            );
                        }
                        for t in rim.stations(edge).unwrap() {
                            let point = rim.sample(edge, t).unwrap().unwrap();
                            assert!(curve.iter().any(|sample| sample.dist(point) < 1e-12), "immutable fixed rim stations must all be retained");
                        }
                    }
                }
                assert_eq!(serde_json::to_value(fit.patch.record()).unwrap(), native);
                assert_eq!(serde_json::to_value((&profile, &edited)).unwrap(), authored);
            }
            assert_eq!(serde_json::to_value((&profile, &cage, state.bodies[0].body.record())).unwrap(), immutable);
        }
    }
    #[test]
    fn surface_fixed_rim_keeps_more_source_corners_than_logical_boundary_controls() {
        use solidcore_record::{Loop2, Plane, Region2, Vec2};
        let polygon: Vec<_> = (0..9)
            .map(|i| {
                let angle = std::f64::consts::TAU * i as f64 / 9.0;
                Vec2::new(3.0 * angle.cos(), 3.0 * angle.sin())
            })
            .collect();
        let region = Region2 { outer: Loop2::polygon(&polygon), holes: vec![] };
        let body = crate::kernel::extrude(&Plane::XY, &[region], 0.0, 2.0).unwrap().pop().unwrap();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let source = crate::ModelBody::new("Support".into(), body, 1);
        let (profile, cage) = Profile::seed(&source, top, [2, 2], 0).unwrap();
        let before = serde_json::to_value((&profile, &cage)).unwrap();
        let ring = cage.boundary_loops().unwrap().pop().unwrap();
        assert_eq!(ring.len(), 8);
        let geometry = crate::kernel::face_geometry(&source.body, top).unwrap();
        let corners: Vec<_> = geometry.loops[0].iter().map(|edge| edge.at(0.0).unwrap().0).collect();
        assert_eq!(corners.len(), 9);
        let rim = super::profile::QuiltRim::new(&profile, &cage, &Placement::default(), &source.body, top).unwrap();
        let curves: Vec<_> = ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
            .map(|(&a, &b)| {
                let edge = [a, b];
                let stations = rim.stations(edge).unwrap();
                assert!(stations.windows(2).all(|pair| pair[0] < pair[1]));
                stations.into_iter().map(|t| rim.sample(edge, t).unwrap().unwrap()).collect::<Vec<_>>()
            })
            .collect();
        assert!(curves.iter().any(|curve| curve.len() > 25), "source corners add display samples rather than new authored controls");
        for &corner in &corners {
            assert!(curves.iter().flatten().any(|point| point.dist(corner) < 1e-8), "no perimeter polyline may cut a source corner");
        }
        for curve in &curves {
            for point in curve {
                assert!(
                    geometry.loops[0].iter().any(|edge| {
                        let a = edge.at(0.0).unwrap().0;
                        let b = edge.at(1.0).unwrap().0;
                        let direction = b - a;
                        let t = ((*point - a).dot(direction) / direction.len2()).clamp(0.0, 1.0);
                        point.dist(a + direction * t) < 1e-8
                    }),
                    "every perimeter sample must stay on an actual native source edge"
                );
            }
        }
        let mut state = crate::ModelState::default();
        state.bodies.push(source.clone());
        let mut display_profile = profile.clone();
        display_profile.partition = false;
        let fit = display_profile.fit(&state, &cage, &Placement::default(), 0.01).unwrap();
        eprintln!("nine-corner Surface fixture: {} native patch faces", fit.patch.face_count());
        if fit.patch.face_count() > 1 {
            let native = serde_json::to_value(fit.patch.record()).unwrap();
            let edges = cage.edges().unwrap();
            let original = super::profile::quilt_guide_edges(&fit.patch, &cage, &Placement::default(), &edges, &rim).unwrap();
            let sampled = super::profile::quilt::display(&fit.patch, &fit.open, &source.body, &edges, &rim, &original, 0.01).unwrap();
            let (centre_id, incident_faces) = (0..fit.patch.record().vertices.len())
                .map(|id| {
                    let faces: std::collections::BTreeSet<_> = fit
                        .patch
                        .record()
                        .edges
                        .iter()
                        .filter(|edge| edge.vertices.contains(&(id as u32)))
                        .flat_map(|edge| edge.uses.iter().map(|use_| use_.face))
                        .collect();
                    (id, faces.len())
                })
                .max_by_key(|&(_, count)| count)
                .unwrap();
            assert!(incident_faces >= 5, "this fixture must exercise its many-sector common centre");
            let point = fit.patch.record().vertices[centre_id].point;
            let centre = solidcore_record::Vec3::new(point[0], point[1], point[2]);
            let mut near_misses = 0;
            for (reference, curve) in original.iter().zip(&sampled.guides) {
                let distance = reference.windows(2).map(|pair| centre.dist_to_segment(pair[0], pair[1])).fold(f64::INFINITY, f64::min);
                if distance > 1e-4 {
                    assert!(
                        !curve.contains(&centre),
                        "an off-centre guide must traverse intermediate sectors rather than detour through their common vertex"
                    );
                    if reference.windows(2).any(|pair| {
                        let direction = pair[1] - pair[0];
                        let t = (centre - pair[0]).dot(direction) / direction.len2();
                        t > 0.05 && t < 0.95 && centre.dist_to_segment(pair[0], pair[1]) < 0.1
                    }) {
                        near_misses += 1;
                    }
                }
                assert!(
                    reference.iter().all(|point| curve.iter().any(|sample| sample.dist(*point) < 1e-10)),
                    "sector tracing must retain every original actual-endpoint guide station"
                );
            }
            assert!(near_misses > 0, "the fixture must contain a guide that passes near but misses the valence-nine centre");
            eprintln!("nine-corner common centre {centre:?}: {incident_faces} incident faces, {near_misses} off-centre guides");
            let key = |point: solidcore_record::Vec3| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
            let side = |a, b| {
                let (a, b) = (key(a), key(b));
                if a < b { [a, b] } else { [b, a] }
            };
            let sides: std::collections::BTreeSet<_> = sampled
                .mesh
                .triangles
                .iter()
                .flat_map(|&[a, b, c]| {
                    let [a, b, c] = [a, b, c].map(|id| sampled.mesh.positions[id as usize]);
                    [side(a, b), side(b, c), side(c, a)]
                })
                .collect();
            for curve in &sampled.guides {
                for pair in curve.windows(2) {
                    assert!(sides.contains(&side(pair[0], pair[1])), "nine-corner quilt guide chords must be actual skin edges");
                }
            }
            for corner in corners {
                assert!(
                    sampled.guides.iter().flatten().any(|point| point.dist(corner) < 1e-12),
                    "co-discretization must retain every original polygon corner"
                );
            }
            assert_eq!(serde_json::to_value(fit.patch.record()).unwrap(), native);
        }
        assert_eq!(serde_json::to_value((&profile, &cage)).unwrap(), before);
    }
    #[test]
    fn native_surface_selection_frames_follow_directed_stations_and_owned_material() {
        use crate::kernel::{DisplayGuide, DisplayGuidePoint};
        use solidcore_record::Vec3;
        use solidcraft_geom::bspline::NurbsSurface;

        let support = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 6.0, 2.0)).unwrap();
        let face = (0..support.face_count()).find(|&i| support.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let support = crate::ModelBody::new("Support".into(), support, 1);
        let (profile, mut cage) = Profile::seed(&support, face, [1, 1], 0).unwrap();
        let point = |u: f64, v: f64| Vec3::new(10.0 * u, 6.0 * v, 2.0 * u * v);
        let uv = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        cage.points = uv.map(|[u, v]| point(u, v)).to_vec();
        let surface = NurbsSurface::new([1, 1], std::array::from_fn(|_| vec![0.0, 0.0, 1.0, 1.0]), &cage.points, &[1.0; 4]).unwrap();
        let body = crate::kernel::nurbs_region_sheet(&surface, &[vec![uv[0], uv[1], uv[3], uv[2]]]).unwrap();
        let mut native = NativeGuides { body: body.clone(), edges: Vec::new() };
        for edge @ [a, b] in cage.edges().unwrap() {
            let (id, parameters) = body
                .record()
                .edges
                .iter()
                .enumerate()
                .find_map(|(id, native)| {
                    let [first, last] = native.vertices.map(|vertex| {
                        let [x, y, z] = body.record().vertices[vertex as usize].point;
                        Vec3::new(x, y, z)
                    });
                    if first.dist(cage.points[a]) < 1e-10 && last.dist(cage.points[b]) < 1e-10 {
                        Some((id, native.interval))
                    } else if last.dist(cage.points[a]) < 1e-10 && first.dist(cage.points[b]) < 1e-10 {
                        Some((id, [native.interval[1], native.interval[0]]))
                    } else {
                        None
                    }
                })
                .unwrap();
            native.edges.push(NativeGuide {
                edge,
                pieces: vec![DisplayGuide {
                    face: 0,
                    points: parameters.map(|parameter| DisplayGuidePoint::Edge { edge: id as u32, parameter }).to_vec(),
                }],
            });
        }
        // Two ordered chart restrictions that meet at the centre. This
        // station is derived from native UV, independent of any display mesh.
        native.edges.push(NativeGuide {
            edge: [4, 5],
            pieces: vec![DisplayGuide { face: 0, points: [[0.2, 0.4], [0.5, 0.4], [0.8, 0.4]].map(DisplayGuidePoint::Surface).to_vec() }],
        });
        let definition = Definition {
            imported: None,
            pipe: None,
            profile: Some(Box::new(profile)),
            matching: None,
            placement: Placement::default(),
            cage,
            parameters: Parameters::default(),
            symmetry: None,
            hierarchy: None,
        };
        let guides = SelectionGuides::Native(native.clone());
        let frames = SelectionFrames::from_profile(&definition, &body, Some(&[0]), &guides);
        let normal = |u: f64, v: f64| Vec3::new(10.0, 0.0, 2.0 * v).cross(Vec3::new(0.0, 6.0, 2.0 * u)).normalized().unwrap();
        let on_cell = frames.faces[&0];
        let on_edge = frames.edges[&[0, 1]];
        let at_point = frames.points[&0];
        let routed = frames.edges[&[4, 5]];
        assert!(on_cell[0].dist(Vec3::new(10.0, 0.0, 1.0).normalized().unwrap()) < 1e-10);
        assert!(on_cell[2].dist(normal(0.5, 0.5)) < 1e-10);
        assert!(on_edge[0].dist(Vec3::X) < 1e-10);
        assert!(on_edge[2].dist(normal(0.5, 0.0)) < 1e-10);
        assert!(at_point[0].dist(Vec3::X) < 1e-10 && at_point[2].dist(Vec3::Z) < 1e-10);
        assert!(routed[0].dist(Vec3::new(10.0, 0.0, 0.8).normalized().unwrap()) < 1e-10);
        assert!(routed[2].dist(normal(0.5, 0.4)) < 1e-10);
        for frame in [on_cell, on_edge, at_point, routed] {
            assert!(frame[0].cross(frame[1]).dist(frame[2]) < 1e-10);
        }

        let unowned = SelectionFrames::from_profile(&definition, &body, None, &guides);
        assert!(unowned.faces.is_empty());
        assert_eq!(unowned.edges[&[4, 5]], routed);
        let mut kinked = native.clone();
        kinked.edges.last_mut().unwrap().pieces[0].points[2] = DisplayGuidePoint::Surface([0.5, 0.8]);
        let refused = SelectionFrames::from_profile(&definition, &body, Some(&[0]), &SelectionGuides::Native(kinked));
        assert!(!refused.edges.contains_key(&[4, 5]), "two incompatible sided tangents cannot define the central station");
        assert_eq!(refused.faces[&0], on_cell);

        let matrix = crate::rigid(Vec3::new(2.0, 4.0, -3.0), Vec3::ZERO, Vec3::Y, 0.7);
        let moved = crate::kernel::transform_matrix(&body, matrix).unwrap();
        let mut placed = definition;
        placed.placement = Placement::from_matrix(&matrix).unwrap();
        native.body = moved.clone();
        let frames = SelectionFrames::from_profile(&placed, &moved, Some(&[0]), &SelectionGuides::Native(native));
        for (actual, original) in [(frames.faces[&0], on_cell), (frames.edges[&[0, 1]], on_edge), (frames.points[&0], at_point)] {
            for (actual, original) in actual.into_iter().zip(original) {
                assert!(actual.dist(original) < 1e-10, "native world derivatives are stored in Form-local axes once");
            }
        }
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
        assert_surface_guides_are_triangle_edges(&mesh);
        assert_eq!(report.patch_roots, fit.patch_roots.clone().unwrap());
        assert_eq!(mesh.tri_face.iter().copied().collect::<std::collections::BTreeSet<_>>(), (0..4).collect());
        for (edge, curve) in cage.edges().unwrap().iter().zip(&mesh.edges) {
            assert!(curve[0].dist(cage.points[edge[0]]) < 0.01);
            assert!(curve.last().unwrap().dist(cage.points[edge[1]]) < 0.01);
        }
        let authored = serde_json::to_value((&legacy, &cage)).unwrap();
        let original_patch = serde_json::to_value(old.patch.record()).unwrap();
        let original_solid = serde_json::to_value(old.solid.record()).unwrap();
        let (legacy_mesh, legacy_report) = legacy.display(&state, &cage, &Placement::default(), 0.01).unwrap();
        assert_surface_guides_are_triangle_edges(&legacy_mesh);
        assert_eq!(legacy_mesh.edges.len(), cage.edges().unwrap().len());
        assert!(legacy_mesh.tri_face.iter().all(|&face| face == u32::MAX));
        assert!(legacy_mesh.edge_faces.iter().all(|faces| faces == &[u32::MAX]));
        assert_eq!(legacy_report.patch_roots, vec![0; old.patch.face_count()]);
        assert_eq!(serde_json::to_value((&legacy, &cage)).unwrap(), authored);
        let after_display = legacy.fit(&state, &cage, &Placement::default(), 0.01).unwrap();
        assert_eq!(serde_json::to_value(after_display.patch.record()).unwrap(), original_patch);
        assert_eq!(serde_json::to_value(after_display.solid.record()).unwrap(), original_solid);
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

    fn assert_surface_guides_are_triangle_edges(mesh: &solidcraft_geom::Mesh) {
        let point_key =
            |point: solidcore_record::Vec3| [point.x, point.y, point.z].map(|coordinate| if coordinate == 0.0 { 0 } else { coordinate.to_bits() });
        let edge_key = |a, b| {
            let mut key = [point_key(a), point_key(b)];
            key.sort();
            key
        };
        let sides: std::collections::BTreeSet<_> = mesh
            .triangles
            .iter()
            .flat_map(|triangle| {
                let [a, b, c] = mesh.tri(triangle).unwrap();
                [edge_key(a, b), edge_key(b, c), edge_key(c, a)]
            })
            .collect();
        for curve in &mesh.edges {
            assert!(curve.len() >= 2);
            for segment in curve.windows(2) {
                assert!(sides.contains(&edge_key(segment[0], segment[1])), "a Surface guide must share its skin's exact triangle edge nodes");
            }
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
    fn legacy_surface_chart_preserves_rectangular_asymmetric_sculpt_and_follows_source_motion() {
        use solidcore_record::Vec3;
        let source = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(12.0, 8.0, 2.0)).unwrap();
        let top = (0..source.face_count()).find(|&i| source.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let mut state = crate::ModelState::default();
        state.bodies.push(crate::ModelBody::new("Support".into(), source, 1));
        let (mut profile, mut cage) = Profile::seed(&state.bodies[0], top, [3, 2], 0).unwrap();
        // Historical chart: logical U runs toward -X and V toward -Y.
        // Unequal resolution catches implementations that only reverse or
        // permute a square grid instead of resampling swapped native axes.
        profile.chart = None;
        profile.rest = (0..=2).flat_map(|j| (0..=3).map(move |i| Vec3::new(12.0 - 4.0 * i as f64, 8.0 - 4.0 * j as f64, 2.0))).collect();
        cage.points = profile.rest.clone();
        let first = Vec3::new(0.2, -0.3, 1.25);
        let second = Vec3::new(-0.15, 0.1, 0.4);
        cage.points[5] += first;
        cage.points[6] += second;
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
        let before = definition.clone();
        let (upgraded, recovered) = profile.recompute(&definition, &state).unwrap();
        assert_eq!(recovered, definition.cage, "an unchanged legacy source must preserve every sculpted coordinate");
        assert_eq!(upgraded.rest, profile.rest);
        assert!(upgraded.chart.is_some());
        assert_eq!(definition, before, "chart recovery is not a document mutation");
        let mut bound = definition.clone();
        bound.profile = Some(Box::new(upgraded.clone()));
        let saved = serde_json::to_vec(&bound).unwrap();
        let restored: Definition = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored, bound);
        let (unchanged_profile, unchanged_cage) = upgraded.recompute(&restored, &state).unwrap();
        assert_eq!(unchanged_profile, upgraded);
        assert_eq!(unchanged_cage, definition.cage);
        let resized = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(18.0, 6.0, 3.0)).unwrap();
        state.bodies[0] = crate::ModelBody::new("Support".into(), resized, 1);
        let (changed, changed_cage) = upgraded.recompute(&restored, &state).unwrap();
        for j in 0..=2 {
            for i in 0..=3 {
                let id = j * 4 + i;
                let expected = Vec3::new(18.0 - 6.0 * i as f64, 6.0 - 3.0 * j as f64, 3.0);
                assert!(changed.rest[id].dist(expected) < 1e-9, "guide {id}: {:?} vs {expected:?}", changed.rest[id]);
                let delta = match id {
                    5 => first,
                    6 => second,
                    _ => Vec3::ZERO,
                };
                assert!(changed_cage.points[id].dist(expected + delta) < 1e-9);
            }
        }
        let mut rebased = restored;
        rebased.profile = Some(Box::new(changed.clone()));
        rebased.cage = changed_cage.clone();
        // A 90-degree source rotation about X plus translation has a known
        // physical answer for every point and every asymmetric sculpt vector.
        let motion = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [7.0, -4.0, 9.0, 1.0]];
        let old_body = state.bodies[0].clone();
        state.bodies[0] = state.body_in_frame(&old_body, &motion).unwrap();
        assert_eq!(state.bodies[0].source_transform, motion);
        let (rotated, rotated_cage) = changed.recompute(&rebased, &state).unwrap();
        for id in 0..changed_cage.points.len() {
            assert!(rotated.rest[id].dist(crate::apply_point(&motion, changed.rest[id])) < 1e-9);
            assert!(
                rotated_cage.points[id].dist(crate::apply_point(&motion, changed_cage.points[id])) < 1e-9,
                "sculpt {id} did not follow source rotation"
            );
        }
        assert_eq!(rebased.cage, changed_cage);
        // Without an established binding an already changed physical source
        // cannot be upgraded by nearest-position guesses.
        assert!(profile.recompute(&definition, &state).is_err());
        assert_eq!(definition, before);
    }
    #[test]
    fn moved_circular_surface_quilt_retains_the_resized_source_rim_at_saved_tolerance() {
        use solidcore_record::Vec3;
        let mut doc = crate::Document::new("Moved circular Surface display");
        let source = doc
            .add_feature(
                crate::FeatureKind::Cylinder {
                    base: crate::point_expr(Vec3::ZERO),
                    axis: Vec3::Z.into(),
                    radius: "3".into(),
                    height: "2".into(),
                    operation: crate::Operation::NewBody,
                    references: Vec::new(),
                },
                None,
            )
            .unwrap();
        doc.feature_mut(source).unwrap().body_names = vec!["Support".into()];
        let mut model = crate::Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|result| result.error.is_none()));
        let state = model.state();
        let body = &state.bodies[0].body;
        let top = (0..body.face_count()).find(|&face| body.planar_face(face).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
        let (profile, mut cage) = Profile::seed(&state.bodies[0], top, [3, 2], 0).unwrap();
        cage.points[5] += Vec3::new(0.12, -0.08, 0.8);
        cage.points[6] += Vec3::new(-0.09, 0.06, 0.35);
        let mut definition = Definition {
            imported: None,
            pipe: None,
            profile: Some(Box::new(profile)),
            matching: None,
            placement: Default::default(),
            cage,
            parameters: Parameters { kind: "face".into(), segments: [3, 2, 1], ..Default::default() },
            hierarchy: None,
            symmetry: None,
        };
        let crate::FeatureKind::Cylinder { radius, height, .. } = &mut doc.feature_mut(source).unwrap().kind else { panic!("cylinder") };
        *radius = "4".into();
        *height = "3".into();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|result| result.error.is_none()));
        let (profile, cage) = definition.profile.as_ref().unwrap().recompute(&definition, &model.state()).unwrap();
        definition.profile = Some(Box::new(profile));
        definition.cage = cage;
        doc.add_feature(
            crate::FeatureKind::Move {
                bodies: vec!["Support".into()],
                translate: ["7".into(), "-4".into(), "9".into()],
                rotate_axis: Some(Vec3::X.into()),
                origin: None,
                angle: Some("90 deg".into()),
                copy: false,
            },
            None,
        )
        .unwrap();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|result| result.error.is_none()));
        let state = model.state();
        let (profile, cage) = definition.profile.as_ref().unwrap().recompute(&definition, &state).unwrap();
        definition.profile = Some(Box::new(profile));
        definition.cage = cage;
        let before = serde_json::to_value(&definition).unwrap();
        let profile = definition.profile.as_ref().unwrap();
        let tolerance = 0.05;
        let fit = profile.fit(&state, &definition.cage, &definition.placement, tolerance).unwrap();
        assert!(fit.patch_roots.is_none() && fit.patch.face_count() > 1);
        fit.open.history().unwrap().check(&fit.open, &[&state.bodies[0].body]).unwrap();
        fit.patch.history().unwrap().check(&fit.patch, &[&fit.open]).unwrap();
        let map = |patch_history: &crate::kernel::history::History| {
            super::profile::quilt::rim_sources(&fit.patch, &fit.open, &state.bodies[0].body, patch_history, fit.open.history().unwrap())
        };
        let origins = map(fit.patch.history().unwrap()).unwrap();
        let outer = fit.patch.record().edges.iter().position(|edge| edge.uses.len() == 1).unwrap();
        let source_edge = &state.bodies[0].body.record().edges[origins[outer].unwrap()];
        let patch_edge = &fit.patch.record().edges[outer];
        assert_ne!(source_edge.parameters(), patch_edge.parameters(), "the fixture must exercise actual native coefficient roundoff");
        let retained = super::profile::quilt::retained_parameters;
        assert!(retained(source_edge, patch_edge));
        let mut ambiguous = fit.patch.history().unwrap().clone();
        let duplicate = ambiguous.edges[outer].origins[0].clone();
        ambiguous.edges[outer].origins.push(duplicate);
        assert!(map(&ambiguous).is_err(), "multiple origins cannot be disambiguated by curve nearness");
        ambiguous.edges[outer].origins.clear();
        assert!(map(&ambiguous).is_err(), "an unrecorded origin is not guessed from the same curve");
        ambiguous.edges[outer].origins.push(crate::kernel::history::EdgeOrigin::Edge { operand: 1, edge: 0 });
        assert!(map(&ambiguous).is_err(), "the known operation has exactly one operand");
        let mut rewritten = patch_edge.clone();
        let solidcore_record::brep::Curve::RationalSpline { knots, .. } = rewritten.nurbs.as_mut().unwrap() else { panic!("rational rim") };
        for knot in knots {
            *knot *= 2.0;
        }
        for parameter in &mut rewritten.interval {
            *parameter *= 2.0;
        }
        let original_curve = solidcraft_geom::brep::eval::CurveEval::new(patch_edge.parameters()).unwrap();
        let rewritten_curve = solidcraft_geom::brep::eval::CurveEval::new(rewritten.parameters()).unwrap();
        for t in [0.17, 0.43, 0.81] {
            let parameter = patch_edge.interval[0] * (1.0 - t) + patch_edge.interval[1] * t;
            assert!(original_curve.point(parameter).unwrap().dist(rewritten_curve.point(2.0 * parameter).unwrap()) < 1e-12);
        }
        assert!(!retained(source_edge, &rewritten), "the identical physical rim with rewritten parameters requires an explicit map");
        let mut changed = patch_edge.clone();
        let solidcore_record::brep::Curve::RationalSpline { homogeneous, .. } = changed.nurbs.as_mut().unwrap() else { panic!("rational rim") };
        homogeneous.reverse();
        assert!(!retained(source_edge, &changed), "reversed intrinsic orientation is refused");
        let mut changed = patch_edge.clone();
        let solidcore_record::brep::Curve::RationalSpline { homogeneous, .. } = changed.nurbs.as_mut().unwrap() else { panic!("rational rim") };
        homogeneous[3][2] += 1e-6;
        assert!(!retained(source_edge, &changed), "a change far below fit tolerance is still not coefficient roundoff");
        let mut changed = patch_edge.clone();
        let solidcore_record::brep::Curve::RationalSpline { homogeneous, .. } = changed.nurbs.as_mut().unwrap() else { panic!("rational rim") };
        homogeneous[3][2] = f64::NAN;
        assert!(!retained(source_edge, &changed));
        let mut changed = patch_edge.clone();
        let solidcore_record::brep::Curve::RationalSpline { homogeneous, .. } = changed.nurbs.as_mut().unwrap() else { panic!("rational rim") };
        homogeneous[3][3] = f64::from_bits(homogeneous[3][3].to_bits() + 1);
        assert!(!retained(source_edge, &changed), "weights and rational basis remain exact");
        changed = patch_edge.clone();
        changed.interval[0] = source_edge.interval[0] - 1e-6;
        assert!(!retained(source_edge, &changed), "a piece cannot escape its known source interval");
        let (_, top) = profile.resolve(&state).unwrap();
        let rim = super::profile::QuiltRim::new(profile, &definition.cage, &definition.placement, &state.bodies[0].body, top).unwrap();
        let edges = definition.cage.edges().unwrap();
        let guides = super::profile::quilt_guide_edges(&fit.patch, &definition.cage, &definition.placement, &edges, &rim).unwrap();
        let native_before = serde_json::to_value((fit.patch.record(), fit.open.record(), state.bodies[0].body.record())).unwrap();
        let cached = fit.patch.keeping_meshes();
        let mesh_tolerance = crate::kernel::MeshTolerance::chord(tolerance);
        let memo_before = serde_json::to_value(cached.mesh(&mesh_tolerance).unwrap()).unwrap();
        let plan_before = cached.mesh_plan(&mesh_tolerance).unwrap();
        let direct = super::profile::quilt::display(&cached, &fit.open, &state.bodies[0].body, &edges, &rim, &guides, tolerance);
        assert!(direct.is_ok(), "the direct co-sampled path must retain a moved source rim: {:?}", direct.err());
        assert!(map(&crate::kernel::history::History::default()).is_err(), "missing operation history is refused");
        let (mesh, _) = profile.display(&state, &definition.cage, &definition.placement, tolerance).unwrap();
        assert_eq!(edges.len(), 3 * 3 + 2 * 4, "the analytic 3x2 guide grid has seventeen edges");
        assert_eq!(mesh.edges.len(), edges.len());
        assert!(mesh.edges.iter().all(|curve| curve.len() >= 2));
        assert!(!mesh.triangles.is_empty());
        let key = |point: Vec3| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
        let side = |a, b| {
            let (a, b) = (key(a), key(b));
            if a < b { [a, b] } else { [b, a] }
        };
        let sides: std::collections::BTreeSet<_> = mesh
            .triangles
            .iter()
            .flat_map(|&[a, b, c]| {
                let [a, b, c] = [a, b, c].map(|id| mesh.positions[id as usize]);
                [side(a, b), side(b, c), side(c, a)]
            })
            .collect();
        for pair in mesh.edges.iter().flat_map(|curve| curve.windows(2)) {
            assert!(sides.contains(&side(pair[0], pair[1])), "every public guide chord must be a bit-identical skin triangle edge");
        }
        let ring = definition.cage.boundary_loops().unwrap().pop().unwrap();
        let perimeter: std::collections::BTreeSet<_> =
            ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()).map(|(&a, &b)| [a.min(b), a.max(b)]).collect();
        for (edge, curve) in edges.iter().zip(&mesh.edges).filter(|(edge, _)| perimeter.contains(*edge)) {
            for point in curve {
                assert!(((point.x - 7.0).hypot(point.z - 9.0) - 4.0).abs() < 1e-12 && (point.y + 7.0).abs() < 1e-12);
            }
            for t in rim.stations(*edge).unwrap() {
                let fixed = rim.sample(*edge, t).unwrap().unwrap();
                assert!(curve.iter().any(|point| point.dist(fixed) < 1e-12), "every original fixed rim station must remain in its guide");
            }
        }
        assert_eq!(serde_json::to_value(&definition).unwrap(), before);
        assert_eq!(serde_json::to_value((fit.patch.record(), fit.open.record(), state.bodies[0].body.record())).unwrap(), native_before);
        assert_eq!(serde_json::to_value(cached.mesh(&mesh_tolerance).unwrap()).unwrap(), memo_before);
        assert_eq!(cached.mesh_plan(&mesh_tolerance).unwrap(), plan_before);
    }
    #[test]
    fn surface_chart_uses_the_circular_trim_when_uv_bbox_corners_are_off_the_rim() {
        use solidcore_record::Vec3;
        let mut doc = crate::Document::new("Circular Surface source motion");
        let source = doc
            .add_feature(
                crate::FeatureKind::Cylinder {
                    base: crate::point_expr(Vec3::ZERO),
                    axis: Vec3::Z.into(),
                    radius: "3".into(),
                    height: "2".into(),
                    operation: crate::Operation::NewBody,
                    references: Vec::new(),
                },
                None,
            )
            .unwrap();
        doc.feature_mut(source).unwrap().body_names = vec!["Support".into()];
        let mut model = crate::Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
        let state = model.state();
        let body = &state.bodies[0].body;
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let (profile, cage) = Profile::seed(&state.bodies[0], top, [3, 2], 0).unwrap();
        assert!(cage.points.iter().any(|p| p.x.hypot(p.y) > 3.5), "the rectangular UV bbox must have points outside the circular rim");
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
        let (unchanged, cage) = profile.recompute(&definition, &state).unwrap();
        assert_eq!(unchanged, profile);
        assert_eq!(cage, definition.cage);
        let support = profile.source.resolve(&state).unwrap();
        for anchor in &profile.chart.as_ref().unwrap().anchors {
            let point = support.at_coordinate(&anchor.coordinate).unwrap().0;
            assert!((point.x.hypot(point.y) - 3.0).abs() < 1e-9);
        }
        let motion = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [7.0, -4.0, 9.0, 1.0]];
        doc.add_feature(
            crate::FeatureKind::Move {
                bodies: vec!["Support".into()],
                translate: ["7".into(), "-4".into(), "9".into()],
                rotate_axis: Some(Vec3::X.into()),
                origin: None,
                angle: Some("90 deg".into()),
                copy: false,
            },
            None,
        )
        .unwrap();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
        let state = model.state();
        assert_eq!(state.bodies[0].source_transform, crate::rigid(Vec3::new(7.0, -4.0, 9.0), Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_2));
        let (changed, cage) = profile.recompute(&definition, &state).unwrap();
        for id in 0..profile.rest.len() {
            assert!(changed.rest[id].dist(crate::apply_point(&motion, profile.rest[id])) < 1e-9);
            assert!(cage.points[id].dist(crate::apply_point(&motion, definition.cage.points[id])) < 1e-9);
        }

        let bottom_center = crate::apply_point(&motion, Vec3::ZERO);
        doc.add_feature(
            crate::FeatureKind::DeleteFace {
                faces: vec![crate::FacePick(bottom_center)],
                body: Some(crate::BodyName("Support".into())),
                heal: false,
            },
            None,
        )
        .unwrap();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
        let modified = model.state();
        assert!(modified.bodies[0].is_surface());
        assert_eq!(modified.bodies[0].source_transform, state.bodies[0].source_transform);
        let (support, face) = profile.resolve(&modified).unwrap();
        let support_center = crate::apply_point(&motion, Vec3::new(0.0, 0.0, 2.0));
        assert_eq!(crate::appearance::face_index_at(support, support_center), Some(face));
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
                    let jet = face.surface.jet(uv[0], uv[1]).map(solidcraft_geom::tess::SurfaceJet::from_kernel).unwrap();
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
    fn named_match_boundary_keeps_its_seam_through_native_reordering_and_source_changes() {
        use solidcore_record::{Vec3, brep::Loop};
        let old_body = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(4.0, 6.0, 8.0)).unwrap();
        let top = (0..old_body.face_count()).find(|&i| old_body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let mut state = crate::ModelState::default();
        state.bodies.push(crate::ModelBody::new("Support".into(), old_body.clone(), 1));
        let source = SupportRef::face(&state.bodies[0], top).unwrap();
        let (old, binding) = source.resolve_bound(&state, false, None).unwrap();
        let binding: support::BoundaryBinding = serde_json::from_value(serde_json::to_value(&binding).unwrap()).unwrap();
        let bytes = crate::kernel::encode_native_body(&old_body).unwrap();
        let mut saved = solidcore_record::native::decode(&bytes).unwrap();
        for wire in &mut saved.solid.faces[top].loops {
            if let Loop::Edges { edges } = wire {
                edges.rotate_left(1);
            }
        }
        // Rebuild canonical edge/vertex numbering after the changed first walk.
        // Native decoding retains all of its ordinary structural/geometric checks.
        saved.solid = saved.solid.canonical().unwrap().brep;
        let shifted = crate::kernel::decode_native_body(&solidcore_record::native::encode(&saved).unwrap()).unwrap();
        let mut reordered = crate::ModelState::default();
        reordered.bodies.push(crate::ModelBody::new("Support".into(), shifted, 1));
        let raw = source.resolve(&reordered).unwrap();
        let (bound, _) = source.resolve_bound(&reordered, false, Some(&binding)).unwrap();
        let first = support::BoundaryCoordinate { curve: 0, fraction: 0.25 };
        assert!(old.at_coordinate(&first).unwrap().0.dist(raw.at_coordinate(&first).unwrap().0) > 1.0);
        for curve in 0..4 {
            for fraction in [0.0, 0.25, 0.7, 1.0] {
                let coordinate = support::BoundaryCoordinate { curve, fraction };
                let (a, ta) = old.at_coordinate(&coordinate).unwrap();
                let (b, tb) = bound.at_coordinate(&coordinate).unwrap();
                assert!(a.dist(b) < 1e-12);
                assert!(ta.dist(tb) < 1e-12, "the derivative must follow the same orientation");
            }
        }
        let mut resized = crate::ModelState::default();
        resized.bodies.push(crate::ModelBody::new("Support".into(), crate::kernel::box_solid(Vec3::ZERO, Vec3::new(8.0, 6.0, 10.0)).unwrap(), 1));
        let (large, _) = source.resolve_bound(&resized, false, Some(&binding)).unwrap();
        for curve in 0..4 {
            for fraction in [0.0, 0.3, 1.0] {
                let coordinate = support::BoundaryCoordinate { curve, fraction };
                let p = old.at_coordinate(&coordinate).unwrap().0;
                assert!(large.at_coordinate(&coordinate).unwrap().0.dist(Vec3::new(p.x * 2.0, p.y, 10.0)) < 1e-10);
            }
        }
        let matrix = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [30.0, 40.0, 50.0, 1.0]];
        let mut moved = crate::ModelState::default();
        moved.bodies.push(state.body_in_frame(&state.bodies[0], &matrix).unwrap());
        let (rigid, _) = source.resolve_bound(&moved, false, Some(&binding)).unwrap();
        for curve in 0..4 {
            for fraction in [0.0, 0.3, 1.0] {
                let coordinate = support::BoundaryCoordinate { curve, fraction };
                let (p, t) = old.at_coordinate(&coordinate).unwrap();
                let (q, u) = rigid.at_coordinate(&coordinate).unwrap();
                assert!(q.dist(crate::apply_point(&matrix, p)) < 1e-10);
                assert!(u.dist(crate::apply_vector(&matrix, t)) < 1e-10);
            }
        }
    }
    #[test]
    fn legacy_match_recovers_only_a_unique_unchanged_physical_boundary() {
        use solidcore_record::{Vec3, brep::Loop};
        let body = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(4.0, 6.0, 8.0)).unwrap();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let mut old = crate::ModelState::default();
        old.bodies.push(crate::ModelBody::new("Support".into(), body.clone(), 1));
        let source = SupportRef::face(&old.bodies[0], top).unwrap();
        let mut saved = solidcore_record::native::decode(&crate::kernel::encode_native_body(&body).unwrap()).unwrap();
        for wire in &mut saved.solid.faces[top].loops {
            if let Loop::Edges { edges } = wire {
                edges.rotate_left(1);
            }
        }
        // Rebuild canonical edge/vertex numbering after the changed first walk.
        // Native decoding retains all of its ordinary structural/geometric checks.
        saved.solid = saved.solid.canonical().unwrap().brep;
        let shifted = crate::kernel::decode_native_body(&solidcore_record::native::encode(&saved).unwrap()).unwrap();
        let mut now = crate::ModelState::default();
        now.bodies.push(crate::ModelBody::new("Support".into(), shifted, 1));
        for order in 0..=2 {
            let parameters = Parameters { kind: "face".into(), dimensions: [4.0, 6.0, 1.0], segments: [2; 3], ..Default::default() };
            let definition = Definition {
                cage: parameters.build().unwrap(),
                parameters,
                placement: Placement { origin: Vec3::new(2.0, 3.0, 8.0), ..Default::default() },
                matching: None,
                hierarchy: None,
                symmetry: None,
                profile: None,
                pipe: None,
                imported: None,
            };
            let ring = definition.cage.boundary_loops().unwrap().remove(0);
            let request =
                MatchRequest { source: source.clone(), adjacent_faces: false, edge: [ring[0], ring[1]], order, tolerances: Default::default() };
            let (mut legacy, _) = Matching::attach(&definition, &old, request, 0.01).unwrap();
            legacy.matching.as_mut().unwrap().binding = None;
            let before = serde_json::to_value(&legacy).unwrap();
            let (restored, report) = legacy.matching.as_ref().unwrap().recompute(&legacy, &now, 0.01).unwrap();
            assert_eq!(restored.cage, legacy.cage, "G{order} legacy authoring controls must not rotate or drift");
            assert!(restored.matching.as_ref().unwrap().binding.is_some());
            let mut existing_fields = serde_json::to_value(&restored).unwrap();
            existing_fields["matching"].as_object_mut().unwrap().remove("binding");
            assert_eq!(existing_fields, before, "legacy upgrade may add only its certified binding");
            assert!(report.position_mm <= 0.001);
            let mut changed = crate::ModelState::default();
            changed.bodies.push(crate::ModelBody::new("Support".into(), crate::kernel::box_solid(Vec3::ZERO, Vec3::new(12.0, 6.0, 8.0)).unwrap(), 1));
            assert!(legacy.matching.as_ref().unwrap().recompute(&legacy, &changed, 0.01).is_err());
            assert_eq!(serde_json::to_value(&legacy).unwrap(), before, "refusal must retain the authored source");
        }
        let tiny = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(0.2, 0.2, 0.2)).unwrap();
        let support = support::BoundarySupport::from_face(&tiny, top).unwrap();
        let coordinates = [0.0, 0.25, 0.5, 0.75].map(|f| support.coordinate(f).unwrap());
        let points = coordinates.iter().map(|c| support.at_coordinate(c).unwrap().0).collect::<Vec<_>>();
        assert!(support.recover_legacy(&coordinates, &points, 1.0).err().unwrap().to_string().contains("ambiguous"));
    }
    #[test]
    fn two_open_edges_with_the_same_incidence_follow_distinct_named_vertices() {
        use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
        let sheet = |radius: f64, sweep: f64| {
            let a = Vec2::new(radius, 0.0);
            let b = Vec2::new(radius * sweep.cos(), radius * sweep.sin());
            let outer = Loop2 { segs: vec![Seg2::Arc { center: Vec2::ZERO, radius, start: 0.0, sweep }, Seg2::Line { a: b, b: a }] };
            crate::kernel::patch_region(&Plane::XY, &Region2 { outer, holes: Vec::new() }).unwrap()
        };
        let state = |body| {
            let mut state = crate::ModelState::default();
            state.bodies.push(crate::ModelBody::new("Support".into(), body, 1));
            state
        };
        let original = state(sheet(4.0, std::f64::consts::PI));
        assert_eq!(original.bodies[0].body.edge_count(), 2);
        let reference = SupportRef::face(&original.bodies[0], 0).unwrap();
        let (before, binding) = reference.resolve_bound(&original, false, None).unwrap();
        let record = serde_json::to_value(&binding).unwrap();
        for curve in record["curves"].as_array().unwrap() {
            assert_eq!(curve["start"], curve["end"]);
            assert!(curve["intrinsic"].is_null());
            let vertices = curve["endpoint_vertices"].as_array().unwrap();
            assert_ne!(vertices[0], vertices[1]);
        }
        let binding = serde_json::from_value(record).unwrap();
        let after = state(sheet(4.0, std::f64::consts::FRAC_PI_2 * 1.9));
        let (after, _) = reference.resolve_bound(&after, false, Some(&binding)).unwrap();
        let bound = |wire: &support::BoundarySupport, curve| {
            let a = wire.at_coordinate(&support::BoundaryCoordinate { curve, fraction: 0.0 }).unwrap().0;
            let b = wire.at_coordinate(&support::BoundaryCoordinate { curve, fraction: 1.0 }).unwrap().0;
            (a, b)
        };
        for i in 0..2 {
            let (a, b) = bound(&before, i);
            let (x, y) = bound(&after, i);
            assert_eq!(a.x.is_sign_positive(), x.x.is_sign_positive());
            assert_eq!(b.x.is_sign_positive(), y.x.is_sign_positive());
            assert!((x.len() - 4.0).abs() < 1e-9 && (y.len() - 4.0).abs() < 1e-9);
        }
        assert!(after.supports_curvature());
        assert!(after.target(0.2, &Placement::default(), 2).unwrap().normal.dot(Vec3::Z).abs() > 0.99);
    }

    #[test]
    fn periodic_support_binding_follows_radius_and_real_source_frame_but_refuses_intrinsic_rewrites() {
        use solidcore_record::Vec3;
        let cylinder = |radius, height| crate::kernel::cylinder(Vec3::ZERO, Vec3::Z, radius, height).unwrap();
        let body = cylinder(2.0, 3.0);
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let mut state = crate::ModelState::default();
        state.bodies.push(crate::ModelBody::new("Support".into(), body.clone(), 1));
        let source = SupportRef::face(&state.bodies[0], top).unwrap();
        let (old, binding) = source.resolve_bound(&state, false, None).unwrap();
        assert_eq!(old.curve_count(), 1);
        let binding: support::BoundaryBinding = serde_json::from_value(serde_json::to_value(&binding).unwrap()).unwrap();
        let mut resized = crate::ModelState::default();
        resized.bodies.push(crate::ModelBody::new("Support".into(), cylinder(4.0, 5.0), 1));
        let (large, _) = source.resolve_bound(&resized, false, Some(&binding)).unwrap();
        for fraction in [0.0, 0.13, 0.37, 0.79, 1.0] {
            let coordinate = support::BoundaryCoordinate { curve: 0, fraction };
            let p = old.at_coordinate(&coordinate).unwrap().0;
            assert!(large.at_coordinate(&coordinate).unwrap().0.dist(Vec3::new(p.x * 2.0, p.y * 2.0, 5.0)) < 1e-9);
        }
        let matrix = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [30.0, 40.0, 50.0, 1.0]];
        let mut moved = crate::ModelState::default();
        moved.bodies.push(state.body_in_frame(&state.bodies[0], &matrix).unwrap());
        let (rigid, _) = source.resolve_bound(&moved, false, Some(&binding)).unwrap();
        for fraction in [0.0, 0.13, 0.37, 0.79, 1.0] {
            let coordinate = support::BoundaryCoordinate { curve: 0, fraction };
            let p = old.at_coordinate(&coordinate).unwrap().0;
            assert!(rigid.at_coordinate(&coordinate).unwrap().0.dist(crate::apply_point(&matrix, p)) < 1e-9);
        }
        // The same geometric circle redefined with a different intrinsic seam has no source
        // placement to follow. Its old fractional anchors must not silently rotate with it.
        let rotation = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let mut rewritten = crate::ModelState::default();
        rewritten.bodies.push(crate::ModelBody::new("Support".into(), crate::kernel::transform_matrix(&body, rotation).unwrap(), 1));
        assert!(source.resolve_bound(&rewritten, false, Some(&binding)).is_err());
        let certificate = serde_json::to_value(&binding).unwrap();
        // Interval-only changes must fail even if the complete rational shape
        // coefficients are identical (shape() ignores trimmed spline intervals).
        let mut shifted_interval = certificate.clone();
        shifted_interval["curves"][0]["intrinsic"]["interval"][0] = serde_json::json!(0.125);
        let shifted_interval: support::BoundaryBinding = serde_json::from_value(shifted_interval).unwrap();
        shifted_interval.validate().unwrap();
        assert!(source.resolve_bound(&state, false, Some(&shifted_interval)).is_err());
        let mut wrong_kind = serde_json::to_value(&binding).unwrap();
        wrong_kind["curves"][0]["intrinsic"]["kind"] = serde_json::json!("different-exact-curve-kind");
        let wrong_kind: support::BoundaryBinding = serde_json::from_value(wrong_kind).unwrap();
        assert!(source.resolve_bound(&state, false, Some(&wrong_kind)).is_err());
        let mut oversized = serde_json::to_value(&binding).unwrap();
        oversized["curves"][0]["intrinsic"]["points"] = serde_json::json!(vec![[0.0, 0.0, 0.0]; 257]);
        let oversized: support::BoundaryBinding = serde_json::from_value(oversized).unwrap();
        assert!(oversized.validate().is_err());
        // A real 3 -> 4 resize changes normalized 3/4 knots by one rounding
        // step, while all indexed controls keep their physical phase/direction.
        let mut seed = crate::ModelState::default();
        let body = cylinder(3.0, 2.0);
        seed.bodies.push(crate::ModelBody::new("Support".into(), body, 1));
        let reference = SupportRef::face(&seed.bodies[0], top).unwrap();
        let (original, bound) = reference.resolve_bound(&seed, false, None).unwrap();
        let mut scaled = crate::ModelState::default();
        scaled.bodies.push(crate::ModelBody::new("Support".into(), cylinder(4.0, 3.0), 1));
        let (large, _) = reference.resolve_bound(&scaled, false, Some(&bound)).unwrap();
        let matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [7.0, -4.0, 9.0, 1.0]];
        let mut moved = crate::ModelState::default();
        moved.bodies.push(scaled.body_in_frame(&scaled.bodies[0], &matrix).unwrap());
        let (transformed, _) = reference.resolve_bound(&moved, false, Some(&bound)).unwrap();
        for fraction in [0.0, 0.13, 0.25, 0.37, 0.5, 0.75, 0.79, 1.0] {
            let coordinate = support::BoundaryCoordinate { curve: 0, fraction };
            let (point, tangent) = original.at_coordinate(&coordinate).unwrap();
            let expected = Vec3::new(point.x * 4.0 / 3.0, point.y * 4.0 / 3.0, 3.0);
            let (actual, direction) = large.at_coordinate(&coordinate).unwrap();
            assert!(actual.dist(expected) < 1e-9);
            assert!(direction.normalized().unwrap().dot(tangent.normalized().unwrap()) > 1.0 - 1e-12);
            let (actual, direction) = transformed.at_coordinate(&coordinate).unwrap();
            assert!(actual.dist(crate::apply_point(&matrix, expected)) < 1e-9);
            assert!(direction.normalized().unwrap().dot(crate::apply_vector(&matrix, tangent).normalized().unwrap()) > 1.0 - 1e-12);
        }
        let certificate = serde_json::to_value(&bound).unwrap();
        for (label, changed) in [
            ("real parameter change", vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.750001, 0.750001, 1.0, 1.0, 1.0]),
            ("split knot multiplicity", vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, f64::from_bits(0.75_f64.to_bits() + 1), 1.0, 1.0, 1.0]),
        ] {
            let mut altered = certificate.clone();
            altered["curves"][0]["intrinsic"]["knots"] = serde_json::json!(changed);
            let altered: support::BoundaryBinding = serde_json::from_value(altered).unwrap();
            altered.validate().unwrap();
            assert!(reference.resolve_bound(&seed, false, Some(&altered)).is_err(), "{label} must retain explicit reattachment refusal");
        }
    }
    #[test]
    fn a_nonplanar_closed_wire_refuses_tangent_and_curvature_support() {
        use solidcore_record::Vec3;
        let body = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(3.0, 2.0, 1.0)).unwrap();
        let record = body.record();
        let points = [[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 2.0, 0.0], [3.0, 2.0, 1.0], [0.0, 2.0, 1.0], [0.0, 0.0, 1.0]];
        let vertices: Vec<_> = points.iter().map(|p| record.vertices.iter().position(|v| &v.point == p).unwrap() as u32).collect();
        let edges: Vec<_> = vertices
            .iter()
            .zip(vertices.iter().cycle().skip(1))
            .take(vertices.len())
            .map(|(&a, &b)| record.edges.iter().position(|e| e.vertices == [a, b] || e.vertices == [b, a]).unwrap())
            .collect();
        let wire = support::BoundarySupport::from_edges(&body, &edges).unwrap();
        assert!(!wire.supports_curvature());
        assert!(wire.target(0.2, &Placement::default(), 0).is_ok());
        for order in [1, 2] {
            assert!(wire.target(0.2, &Placement::default(), order).is_err());
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
                    let jet = geometry.surface.jet(0.37, 0.61).map(solidcraft_geom::tess::SurfaceJet::from_kernel).unwrap();
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
    fn collapse_plane_fixture(detail: bool) -> Definition {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;
        let parameters = Parameters { kind: "plane".into(), dimensions: [8., 8., 1.], segments: [1, 1, 1], ..Default::default() };
        let mut h = Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[0], [2, 1]).unwrap();
        if detail {
            let cage = h.evaluate().unwrap();
            let mid = h.controls.iter().find(|&(&id, &birth)| birth == 1 && cage.points[id].x.abs() < 1e-9).map(|(&id, _)| id).unwrap();
            h.add_deltas(&[mid], &[Vec3::X * 0.125]).unwrap();
            assert_ne!(h.evaluate().unwrap(), cage);
        }
        Definition {
            cage: h.evaluate().unwrap(),
            hierarchy: Some(h),
            placement: Placement { origin: Vec3::new(2., -3., 4.), ..Default::default() },
            parameters,
            symmetry: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        }
    }
    #[test]
    fn boundary_collapse_keeps_native_neighbor_and_creates_a_true_triangle() {
        use crate::kernel;
        use solidcore_record::Vec3;
        for detail in [false, true] {
            let source = collapse_plane_fixture(detail);
            let before = source.clone();
            let layout = source.hierarchy.as_ref().unwrap().layout().unwrap();
            let left = &layout.faces[1];
            let picks = [left[3], left[0]];
            let prepared = prepare_boundary_collapse(&source, picks, 0.05).unwrap();
            let opposite = prepare_boundary_collapse(&source, [picks[1], picks[0]], 0.05).unwrap();
            assert_eq!(prepared.site, opposite.site);
            assert_eq!(prepared.site.face, 1);
            assert_eq!(prepared.site.retained_neighbor, 2);
            assert!(!left.contains(&prepared.site.point));
            assert_eq!(prepared.site.boundary[0], prepared.site.point);
            assert_eq!(prepared.site.parameters, [[0., 0.5], [0.5, 0.], [0.5, 1.]]);
            assert_eq!(prepared.body.face_count(), 2);
            assert_eq!(prepared.body.edge_count(), 6);
            assert_eq!(prepared.body.shell_count(), 1);
            assert!(prepared.body.is_surface() && kernel::validity(&prepared.body).ok());
            let old = convert(&source, 0.05).unwrap().0;
            let old_area = kernel::measure(&old).unwrap().area;
            let new_area = kernel::measure(&prepared.body).unwrap().area;
            assert!(new_area < old_area - 1.);
            println!("detail {detail}, apex {:?}, original area {old_area}, collapsed area {new_area}", prepared.point);
            if !detail {
                assert!(prepared.point.dist(Vec3::new(2. - 11. / 3., -3., 4.)) < 1e-8);
            }
            assert_eq!(prepared.face_owners.iter().copied().collect::<std::collections::BTreeSet<_>>(), [1, 2].into_iter().collect());
            assert_eq!(source, before);
        }
    }
    #[test]
    fn mixed_depth_collapse_retains_both_fine_sculpted_neighbors() {
        use crate::kernel;
        use solidcore_record::Vec3;
        let mut source = collapse_plane_fixture(false);
        let mut h = source.hierarchy.as_ref().unwrap().subdivide(&[2], [2, 1]).unwrap();
        assert_eq!(h.depth, 2);
        let cage = h.evaluate().unwrap();
        let fine = h.controls.iter().find(|&(&_, &birth)| birth == 2).map(|(&id, _)| id).unwrap();
        h.add_deltas(&[fine], &[Vec3::X * 0.125]).unwrap();
        assert_ne!(h.evaluate().unwrap(), cage);
        source.cage = h.evaluate().unwrap();
        source.hierarchy = Some(h);
        let source_before = source.clone();
        let layout = source.hierarchy.as_ref().unwrap().layout().unwrap();
        let left = &layout.faces[1];
        let prepared = prepare_boundary_collapse(&source, [left[3], left[0]], 0.05).unwrap();
        assert_eq!((prepared.site.face, prepared.site.retained_neighbor), (1, 3));
        assert_eq!(prepared.face_owners.iter().copied().collect::<std::collections::BTreeSet<_>>(), [1, 3, 4].into_iter().collect());
        assert_eq!((prepared.body.face_count(), prepared.body.edge_count()), (3, 9));
        assert!(kernel::validity(&prepared.body).ok());
        assert_eq!(source, source_before);
        println!("preserved mixed-depth owners 1,3,4 and fine source sculpt");
    }

    #[test]
    fn authored_boundary_collapse_replays_exact_diagonals_and_retires_original_endpoints() {
        use crate::{Document, FeatureKind, Operation};
        let source = collapse_plane_fixture(true);
        let before = source.hierarchy.as_ref().unwrap().layout().unwrap();
        let face = &before.faces[1];
        let endpoints = [face[3], face[0]];
        let saved = author_boundary_collapse(&source, endpoints, 0.05).unwrap();
        let hierarchy = saved.hierarchy.as_ref().unwrap();
        let layout = hierarchy.layout().unwrap();
        let site = hierarchy.validated_boundary_collapse().unwrap().unwrap();
        assert_eq!(hierarchy.collapsed_face(), Some(1));
        assert_eq!(layout.faces[1], site.boundary);
        assert_eq!(layout.faces[2], before.faces[2]);
        assert_eq!(saved.cage, source.cage);
        assert_eq!(hierarchy.details, source.hierarchy.as_ref().unwrap().details);
        assert!(endpoints.iter().all(|&id| hierarchy.is_retired_collapse_control(id)));
        assert!(layout.visible_edges().all(|edge| !edge.iter().any(|id| endpoints.contains(id))));
        let (native, report, guides, _) = convert_with_selection(&saved, 0.05).unwrap();
        assert_eq!(report.patch_roots.len(), 2);
        let points = guides.points().unwrap();
        assert!(endpoints.iter().all(|id| !points.contains_key(id)));
        let expected = prepare_boundary_collapse(&source, endpoints, 0.05).unwrap();
        assert!(points[&site.point].dist(expected.point) < 1e-8);
        let SelectionGuides::Subdivision(curves) = guides else { panic!("expected exact source routes") };
        for edge in site.diagonal_edges() {
            let curve = curves.iter().find(|curve| curve.edge == edge).unwrap();
            let pieces = curve.rational_pieces().unwrap();
            assert!(!pieces.is_empty());
            assert!(curve.at(0.0).unwrap().dist(points[&edge[0]]) < 1e-8);
            assert!(curve.at(1.0).unwrap().dist(points[&edge[1]]) < 1e-8);
        }
        let mut document = Document::new("Refined Collapse");
        let id = document
            .add_feature(
                FeatureKind::Form {
                    definition: Box::new(saved.clone()),
                    tolerance: "0.05 mm".into(),
                    operation: Operation::NewBody,
                    targets: Vec::new(),
                },
                None,
            )
            .unwrap();
        let reopened = Document::from_json(&document.to_json()).unwrap();
        let FeatureKind::Form { definition, .. } = &reopened.feature(id).unwrap().kind else { panic!("expected saved Form") };
        assert_eq!(definition.as_ref(), &saved);
        let (body, reopened_report) = convert(definition, 0.05).unwrap();
        assert_eq!(native.record(), body.record());
        assert_eq!(report.patch_roots, reopened_report.patch_roots);
        let mut forged = serde_json::to_value(&saved).unwrap();
        forged["hierarchy"]["boundary_collapse"]["site"]["retained_neighbor"] = serde_json::json!(1);
        let forged: Definition = serde_json::from_value(forged).unwrap();
        assert!(convert(&forged, 0.05).is_err());
    }

    fn refined_weld_source(lower: bool) -> crate::FeatureKind {
        use crate::{FeatureKind, Operation};
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;

        let parameters = Parameters { kind: "box".into(), dimensions: [8.0, 8.0, 5.0], ..Default::default() };
        let mut points = Vec::new();
        for z in [-5.0, 0.0] {
            for (x, y) in [(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)] {
                points.push(Vec3::new(x, y, z));
            }
        }
        let mut faces = (0..4)
            .map(|a| {
                let b = (a + 1) % 4;
                vec![a, b, b + 4, a + 4]
            })
            .collect::<Vec<_>>();
        faces.push(if lower { vec![3, 2, 1, 0] } else { vec![4, 5, 6, 7] });
        let count = if lower { 2 } else { 4 };
        let mut hierarchy = Hierarchy::new(Cage { points, faces, ..Default::default() }).unwrap().subdivide(&[4], [count, count]).unwrap();
        let cage = hierarchy.evaluate().unwrap();
        let center = hierarchy
            .controls
            .iter()
            .find(|&(&id, &level)| {
                let p = cage.points[id];
                level == 1 && p.x == 0.0 && p.y == 0.0
            })
            .map(|(&id, _)| id)
            .unwrap();
        hierarchy.add_deltas(&[center], &[Vec3::Z * 0.25]).unwrap();
        assert_ne!(hierarchy.evaluate().unwrap(), cage);
        let placement = Placement { origin: if lower { Vec3::ZERO } else { Vec3::Z * 5.0 }, ..Default::default() };
        FeatureKind::Form {
            definition: Box::new(Definition {
                cage: hierarchy.evaluate().unwrap(),
                hierarchy: Some(hierarchy),
                placement,
                parameters,
                symmetry: None,
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
            }),
            tolerance: "0.05 mm".into(),
            operation: Operation::NewBody,
            targets: Vec::new(),
        }
    }

    #[test]
    fn refined_weld_seam_replays_source_namespaces_and_refuses_retired_rim_owners() {
        use crate::walk::{Ref, Walk};
        use crate::{Document, FeatureId, FeatureKind};

        let mut doc = Document::new("Refined weld sources");
        let first = FeatureId(doc.add_feature(refined_weld_source(true), Some("Lower")).unwrap());
        let second = FeatureId(doc.add_feature(refined_weld_source(false), Some("Upper")).unwrap());
        let sources = [first, second];
        let seam = RefinedWeldSeam::capture(&doc, sources, [[4, 5], [0, 1]]).unwrap();
        assert_eq!(seam.sources(), sources);
        assert!(RefinedWeldSeam::capture(&doc, [first, first], [[4, 5], [4, 5]]).is_err());
        let expected = seam.replay(&doc).unwrap();
        assert_eq!(expected.len(), 4);
        assert_eq!(expected[0].each_ref().map(|side| side.owner), [0, 0], "the same local ID has two source namespaces");
        assert_eq!(expected[0].each_ref().map(|side| side.edge), [[5, 4], [0, 1]]);
        let mut refs = Vec::new();
        seam.walk_refs(&mut |reference| {
            if let Ref::Feature(id) = reference {
                refs.push(*id);
            }
        });
        assert_eq!(refs, sources);
        let json = serde_json::to_value(&seam).unwrap();
        let saved: RefinedWeldSeam = serde_json::from_value(json.clone()).unwrap();
        let mut reopened = Document::from_json(&doc.to_json()).unwrap();
        assert_eq!(saved.replay(&reopened).unwrap(), expected);
        assert_eq!(reopened.feature(first.0), doc.feature(first.0));
        assert_eq!(reopened.feature(second.0), doc.feature(second.0));

        let edit = |doc: &mut Document, leaf, split| {
            let FeatureKind::Form { definition, .. } = &mut doc.feature_mut(first.0).unwrap().kind else { panic!("source Form") };
            let old = definition.hierarchy.as_ref().unwrap();
            let next = old.subdivide(&[leaf], split).unwrap();
            assert_eq!(old.details, next.details[..old.details.len()]);
            definition.cage = next.evaluate().unwrap();
            definition.hierarchy = Some(next);
        };
        edit(&mut reopened, 5, [2, 2]);
        let refined = saved.replay(&reopened).unwrap();
        assert_eq!(refined[0][0].owner, expected[0][0].owner);
        assert_eq!(refined[0][0].edge, expected[0][0].edge);
        assert_eq!(refined[0][0].path.len(), 5);
        assert_eq!(refined[0][1], expected[0][1]);
        edit(&mut reopened, 0, [2, 1]);
        let failed = reopened.to_json();
        assert!(saved.replay(&reopened).is_err());
        assert_eq!(reopened.to_json(), failed, "a refused replay never changes either source");

        let mut wrong_owner = json.clone();
        wrong_owner["sides"][0][0]["owner"] = serde_json::json!(2);
        assert!(serde_json::from_value::<RefinedWeldSeam>(wrong_owner).unwrap().replay(&doc).is_err());
        let mut over_budget = json.clone();
        let side = json["sides"][0].clone();
        over_budget["sides"] = serde_json::json!(vec![side; 257]);
        assert!(serde_json::from_value::<RefinedWeldSeam>(over_budget).is_err());
    }

    #[test]
    fn refined_weld_qualifies_real_native_coedges_and_preserves_both_source_face_namespaces() {
        use crate::{Document, FeatureId, FeatureKind};
        use std::collections::BTreeSet;
        let mut doc = Document::new("Refined weld CAD");
        let a = FeatureId(doc.add_feature(refined_weld_source(true), Some("Lower")).unwrap());
        let b = FeatureId(doc.add_feature(refined_weld_source(false), Some("Upper")).unwrap());
        let seam = RefinedWeldSeam::capture(&doc, [a, b], [[4, 5], [0, 1]]).unwrap();
        let before = doc.to_json();
        let made = prepare_refined_weld(&doc, &seam, [0.05; 2]).unwrap();
        assert_eq!((made.body.face_count(), made.body.edge_count(), made.body.shell_count()), (28, 64, 1));
        assert!(!made.body.is_surface());
        assert!(crate::kernel::validity(&made.body).ok());
        for (source, count) in [(a, 8), (b, 20)] {
            let FeatureKind::Form { definition, .. } = &doc.feature(source.0).unwrap().kind else { panic!("expected Form") };
            let expected = definition
                .hierarchy
                .as_ref()
                .unwrap()
                .layout()
                .unwrap()
                .faces
                .iter()
                .enumerate()
                .filter_map(|(id, face)| (!face.is_empty()).then_some(id))
                .collect::<BTreeSet<_>>();
            let actual = made.face_owners.iter().filter(|face| face.source == source).map(|face| face.face).collect::<BTreeSet<_>>();
            assert_eq!(actual.len(), count);
            assert_eq!(actual, expected);
        }
        let restored = Document::from_json(&before).unwrap();
        let replayed = prepare_refined_weld(&restored, &seam, [0.05; 2]).unwrap();
        assert!(made.matches_operands(replayed.operands()));
        let [first, second] = replayed.operands();
        assert!(!made.matches_operands([second, first]));
        assert_eq!(made.face_owners, replayed.face_owners);
        assert_eq!(before, doc.to_json());

        let wrong = RefinedWeldSeam::capture(&doc, [a, b], [[4, 5], [1, 2]]).unwrap();
        let error = prepare_refined_weld(&doc, &wrong, [0.05; 2]).err().unwrap().to_string();
        assert!(error.contains("do not coincide"), "{error}");
        let mut offset = restored.clone();
        let FeatureKind::Form { definition, .. } = &mut offset.feature_mut(b.0).unwrap().kind else { panic!("expected Form") };
        definition.placement.origin.z += 0.01;
        let frozen = offset.to_json();
        let error = prepare_refined_weld(&offset, &seam, [0.05; 2]).err().unwrap().to_string();
        assert!(error.contains("do not coincide"), "{error}");
        assert_eq!(frozen, offset.to_json());

        let mut opened = restored;
        let FeatureKind::Form { definition, .. } = &mut opened.feature_mut(b.0).unwrap().kind else { panic!("expected Form") };
        let hierarchy = definition.hierarchy.as_ref().unwrap();
        let cap = hierarchy.leaves.iter().enumerate().filter_map(|(id, leaf)| (leaf.active && leaf.root == 4).then_some(id)).collect::<Vec<_>>();
        let hierarchy = hierarchy.delete_faces(&cap).unwrap();
        definition.cage = hierarchy.evaluate().unwrap();
        definition.hierarchy = Some(hierarchy);
        assert_eq!(seam.replay(&opened).unwrap().len(), 4, "the selected rim still exists");
        let frozen = opened.to_json();
        let error = prepare_refined_weld(&opened, &seam, [0.05; 2]).err().unwrap().to_string();
        assert!(error.contains("no other free edges"), "{error}");
        assert_eq!(frozen, opened.to_json());
    }

    fn quad_cap_opening_fixture() -> (Definition, [usize; 2], [usize; 2]) {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;

        let parameters = Parameters { kind: "face".into(), dimensions: [8.0, 8.0, 1.0], segments: [4, 4, 1], ..Default::default() };
        let original = Hierarchy::new(parameters.build().unwrap()).unwrap();
        let mut refined = original.subdivide(&[0], [2, 2]).unwrap();
        let layout = refined.layout().unwrap();
        let children = &layout.faces[original.leaves.len()..];
        let center = *children[0].iter().find(|vertex| children.iter().all(|face| face.contains(vertex))).unwrap();
        refined.add_deltas(&[center], &[Vec3::Z * 0.01]).unwrap();
        let mut ring = layout.faces[10].clone();
        assert_eq!(ring.len(), 4);
        ring.rotate_left(1);
        let seed = [ring[0], ring[1]];
        let hierarchy = refined.delete_faces(&[10]).unwrap();
        let definition = Definition {
            placement: Placement { origin: Vec3::new(2.0, -3.0, 4.0), ..Default::default() },
            cage: hierarchy.evaluate().unwrap(),
            parameters,
            symmetry: None,
            hierarchy: Some(hierarchy),
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let interior = layout.faces[5][..2].try_into().unwrap();
        (definition, seed, interior)
    }

    #[test]
    fn quad_cap_preparation_sews_new_owned_face_without_reactivating_sculpted_source() {
        use solidcore_record::Vec3;
        let (definition, seed, interior) = quad_cap_opening_fixture();
        let before = definition.clone();
        let (_, original_report) = convert(&definition, 0.05).unwrap();
        let cap = prepare_quad_cap(&definition, 0.05, &[seed], &[]).unwrap();
        assert_eq!(cap.face, definition.hierarchy.as_ref().unwrap().leaves.len());
        assert!(!cap.rim.closes_skin && cap.body.is_surface() && !cap.body.is_mesh());
        assert_eq!(cap.face_owners.iter().filter(|&&owner| owner == cap.face).count(), 1);
        assert!(!cap.face_owners.contains(&10), "the original material remains deleted");
        let mut expected: std::collections::BTreeSet<_> = original_report.patch_roots.into_iter().collect();
        expected.insert(cap.face);
        assert_eq!(cap.face_owners.iter().copied().collect::<std::collections::BTreeSet<_>>(), expected);
        assert!(crate::kernel::validity(&cap.body).ok());
        let face = cap.face_owners.iter().position(|&owner| owner == cap.face).unwrap();
        let edges = crate::kernel::face_boundaries(&cap.body, face).unwrap();
        let record = cap.body.record();
        for uv in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            let expected = definition.placement.point(cap.surface.eval(uv[0], uv[1]).unwrap());
            assert!(edges.iter().flatten().flat_map(|edge| record.edges[edge.edge].vertices).any(|vertex| {
                let [x, y, z] = record.vertices[vertex as usize].point;
                expected.dist(Vec3::new(x, y, z)) < 1e-7
            }));
        }
        assert!(prepare_quad_cap(&definition, 0.05, &[seed, interior], &[]).is_err());
        assert_eq!(definition, before);
    }
    #[test]
    fn nonplanar_cap_on_original_rounded_rim_keeps_one_logical_owner() {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;

        let parameters = Parameters { kind: "face".into(), dimensions: [64.0, 64.0, 1.0], segments: [8, 8, 1], ..Default::default() };
        let cage = parameters.build().unwrap();
        let seed = <[usize; 2]>::try_from(&cage.faces[18][..2]).unwrap();
        let mut hierarchy = Hierarchy::new(cage.delete_faces(&[18]).unwrap()).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let rim = hierarchy.quad_cap_rim(&[seed], &[]).unwrap();
        let vertex = rim.sides[0].edge[0];
        hierarchy.add_deltas(&[vertex], &[Vec3::Z * 0.05]).unwrap();
        let face = hierarchy.leaves.len();
        let definition = Definition {
            cage: hierarchy.evaluate().unwrap(),
            parameters,
            hierarchy: Some(hierarchy),
            placement: Placement::default(),
            symmetry: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let prepared = prepare_quad_cap(&definition, 0.05, &[seed], &[]).unwrap();
        assert_eq!(prepared.face, face);
        assert_eq!(prepared.rim, rim);
        assert!(prepared.body.is_surface() && crate::kernel::validity(&prepared.body).ok());
        assert!(prepared.face_owners.iter().filter(|&&id| id == face).count() > 1);

        let authored = author_quad_cap(&definition, 0.05, &[seed], &[]).unwrap();
        let hierarchy = authored.hierarchy.as_ref().unwrap();
        assert_eq!(hierarchy.validated_quad_cap().unwrap(), Some((face, rim)));
        assert_eq!(authored.cage, definition.cage);
        let saved: Definition = serde_json::from_value(serde_json::to_value(&authored).unwrap()).unwrap();
        let (_, report) = convert(&saved, 0.05).unwrap();
        assert_eq!(report.patch_roots, prepared.face_owners);

        // The preview for a second cap still groups every atlas chart of
        // the existing cap under the earlier reserved face ID.
        let next_seed = <[usize; 2]>::try_from(&hierarchy.layout().unwrap().faces[44][..2]).unwrap();
        let mut opened = authored.clone();
        let next = hierarchy.delete_faces(&[44]).unwrap();
        let next_face = next.leaves.len();
        opened.hierarchy = Some(next);
        let preview = prepare_quad_cap(&opened, 0.05, &[next_seed], &[]).unwrap();
        assert_eq!(preview.face, next_face);
        let atlas_faces = preview.face_owners.iter().filter(|&&id| id == face).count();
        assert!(atlas_faces > 1);
        assert_eq!(preview.face_owners.iter().filter(|&&id| id == next_face).count(), 1);
        let both = author_quad_cap(&opened, 0.05, &[next_seed], &[]).unwrap();

        let mut document = crate::Document::new("Nonplanar cap and planar cap");
        let feature = document
            .add_feature(
                crate::FeatureKind::Form {
                    definition: Box::new(both.clone()),
                    tolerance: "0.05 mm".into(),
                    operation: crate::Operation::NewBody,
                    targets: Vec::new(),
                },
                None,
            )
            .unwrap();
        let reopened = crate::Document::from_json(&document.to_json()).unwrap();
        let mut model = crate::Model::new();
        model.evaluate(&reopened);
        assert!(model.results.iter().all(|result| result.error.is_none()), "{:?}", model.results);
        let state = model.state();
        let names = crate::naming::face_names(&state.bodies[0]);
        let atlas_name = format!("F{feature}:FormPatch{}", face + 1);
        let planar_name = format!("F{feature}:FormPatch{}", next_face + 1);
        assert_eq!(names.iter().filter(|name| crate::naming::strip_piece(name) == atlas_name).count(), atlas_faces);
        assert_eq!(names.iter().filter(|name| *name == &planar_name).count(), 1);

        let mut removed = both.clone();
        removed.hierarchy = Some(both.hierarchy.as_ref().unwrap().delete_faces(&[face]).unwrap());
        let (_, report) = convert(&removed, 0.05).unwrap();
        assert!(!report.patch_roots.contains(&face));
        assert!(report.patch_roots.contains(&next_face));
        let recapped = author_quad_cap(&removed, 0.05, &[seed], &[]).unwrap();
        let recapped = recapped.hierarchy.as_ref().unwrap();
        assert_eq!(recapped.quad_cap_faces().collect::<Vec<_>>(), [next_face, next_face + 1]);
        assert!(recapped.layout().unwrap().faces[face].is_empty());
    }

    #[test]
    fn authored_quad_cap_keeps_its_face_after_leaf_allocation_and_design_reopen() {
        let (source, seed, _) = quad_cap_opening_fixture();
        let before = source.clone();
        let mut definition = author_quad_cap(&source, 0.05, &[seed], &[]).unwrap();
        let hierarchy = definition.hierarchy.as_ref().unwrap();
        let original = source.hierarchy.as_ref().unwrap();
        let (face, rim) = hierarchy.validated_quad_cap().unwrap().unwrap();
        let vertices = rim.sides.each_ref().map(|side| side.edge[0]);
        assert_eq!(face, original.leaves.len());
        assert_eq!(hierarchy.layout().unwrap().faces[face], vertices);
        assert_eq!(hierarchy.base, original.base);
        assert_eq!(hierarchy.details, original.details);
        assert_eq!(hierarchy.controls, original.controls);
        assert_eq!(hierarchy.sharpness, original.sharpness);
        assert_eq!(definition.cage, source.cage);
        assert_eq!(definition.placement, source.placement);
        assert!(author_quad_cap(&definition, 0.05, &[seed], &[]).is_err());
        let support = rim.sides[0].owner;
        assert!(hierarchy.subdivide(&[support], [2, 2]).is_err(), "a cap side cannot silently gain a different owner");
        assert!(hierarchy.delete_faces(&[support]).is_err(), "the bound rim must survive source edits");
        assert!(hierarchy.delete_faces(&[face, support]).is_err(), "delete the derived face on its own");
        assert!(hierarchy.restore_deleted_holes(&[seed], &[], false).is_err(), "the new cap and original material cannot overlap");
        assert_eq!(hierarchy.validated_quad_cap().unwrap(), Some((face, rim.clone())));
        assert_eq!(source, before);

        let refined = hierarchy.subdivide(&[3], [4, 4]).unwrap();
        assert!(refined.depth > hierarchy.depth && refined.leaves.len() > face + 1);
        assert_eq!(refined.validated_quad_cap().unwrap(), Some((face, rim.clone())));
        assert_eq!(refined.layout().unwrap().faces[face], vertices);
        assert_eq!(&refined.details[..hierarchy.details.len()], &hierarchy.details);
        definition.cage = refined.evaluate().unwrap();
        definition.hierarchy = Some(refined);

        let mut document = crate::Document::new("Quad cap");
        let id = document
            .add_feature(
                crate::FeatureKind::Form {
                    definition: Box::new(definition.clone()),
                    tolerance: "0.05 mm".into(),
                    operation: crate::Operation::NewBody,
                    targets: Vec::new(),
                },
                None,
            )
            .unwrap();
        let reopened = crate::Document::from_json(&document.to_json()).unwrap();
        assert_eq!(reopened.feature(id), document.feature(id));
        let crate::FeatureKind::Form { definition: saved, .. } = &reopened.feature(id).unwrap().kind else { panic!("saved Form") };
        assert_eq!(saved.hierarchy.as_ref().unwrap().validated_quad_cap().unwrap(), Some((face, rim.clone())));
        let mut model = crate::Model::new();
        model.evaluate(&reopened);
        assert!(model.results.iter().all(|result| result.error.is_none()), "{:?}", model.results);
        let state = model.state();
        assert_eq!(state.bodies.len(), 1);
        assert!(state.bodies[0].body.is_surface());
        let (body, report) = convert(saved, 0.05).unwrap();
        assert_eq!(report.patch_roots.iter().filter(|&&owner| owner == face).count(), 1);
        assert!(crate::kernel::validity(&body).ok());

        let mut hostile = serde_json::to_value(saved).unwrap();
        hostile["hierarchy"]["quad_cap"]["sides"][0]["owner"] = serde_json::json!(face);
        let moved: Definition = serde_json::from_value(hostile).unwrap();
        assert!(convert(&moved, 0.05).is_err(), "a changed saved owner cannot retarget the cap");
        let old = serde_json::to_value(source).unwrap();
        assert!(old["hierarchy"].get("quad_cap").is_none());
        assert_eq!(serde_json::from_value::<Definition>(old).unwrap(), before);
    }
    #[test]
    fn disjoint_quad_caps_replay_and_reshape_independently_with_stable_ids() {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;

        let parameters = Parameters { kind: "face".into(), dimensions: [64.0, 64.0, 1.0], segments: [8, 8, 1], ..Default::default() };
        let refined = Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let layout = refined.layout().unwrap();
        let [first_edge, second_edge] = [18, 45].map(|id| <[usize; 2]>::try_from(&layout.faces[id][..2]).unwrap());
        let hierarchy = refined.delete_faces(&[18, 45]).unwrap();
        let first = hierarchy.leaves.len();
        let second = first + 1;
        let source = Definition {
            cage: hierarchy.evaluate().unwrap(),
            hierarchy: Some(hierarchy),
            parameters,
            placement: Placement::default(),
            imported: None,
            pipe: None,
            profile: None,
            symmetry: None,
            matching: None,
        };
        let single = author_quad_cap(&source, 0.05, &[first_edge], &[]).unwrap();
        let legacy = serde_json::to_value(&single).unwrap();
        assert!(legacy["hierarchy"].get("additional_quad_caps").is_none());
        assert_eq!(serde_json::from_value::<Definition>(legacy).unwrap(), single);
        let prepared = prepare_quad_cap(&single, 0.05, &[second_edge], &[]).unwrap();
        assert_eq!(prepared.face, second);
        assert!(prepared.body.is_surface());
        for owner in [first, second] {
            assert_eq!(prepared.face_owners.iter().filter(|&&id| id == owner).count(), 1);
        }
        let both = author_quad_cap(&single, 0.05, &[second_edge], &[]).unwrap();
        let hierarchy = both.hierarchy.as_ref().unwrap();
        assert_eq!(hierarchy.quad_cap_faces().collect::<Vec<_>>(), [first, second]);
        let rims = hierarchy.validated_quad_caps().unwrap();
        assert_eq!(rims[0].1.sides.each_ref().map(|side| side.owner), [10, 19, 26, 17]);
        assert_eq!(rims[1].1.sides.each_ref().map(|side| side.owner), [37, 46, 53, 44]);
        assert!(author_quad_cap(&both, 0.05, &[first_edge], &[]).is_err());
        assert!(author_quad_cap(&both, 0.05, &[second_edge], &[]).is_err());
        assert!(hierarchy.delete_faces(&[first, second]).is_err());

        let mut document = crate::Document::new("Disjoint caps");
        let feature = document
            .add_feature(
                crate::FeatureKind::Form {
                    definition: Box::new(both.clone()),
                    tolerance: "0.05 mm".into(),
                    operation: crate::Operation::NewBody,
                    targets: Vec::new(),
                },
                None,
            )
            .unwrap();
        let reopened = crate::Document::from_json(&document.to_json()).unwrap();
        assert_eq!(reopened.feature(feature), document.feature(feature));

        let (body, report, fitted) = convert_source(&both, 0.05).unwrap();
        assert!(crate::kernel::validity(&body).ok());
        assert!(!report.patch_roots.iter().any(|id| [18, 45].contains(id)));
        let center = |cap: &quad_cap::FittedQuadCap| cap.surface.eval(0.5, 0.5).unwrap();
        let mut sculpted = both.clone();
        let sculpted_h = sculpted.hierarchy.as_mut().unwrap();
        sculpted_h.add_deltas(&[rims[0].1.sides[0].edge[0]], &[Vec3::X * 0.05]).unwrap();
        sculpted.cage = sculpted_h.evaluate().unwrap();
        let (body, report, sculpted_fit) = convert_source(&sculpted, 0.05).unwrap();
        assert!(crate::kernel::validity(&body).ok());
        assert_eq!(sculpted_fit.quad_caps.iter().map(|cap| cap.face).collect::<Vec<_>>(), [first, second]);
        let movement = center(&sculpted_fit.quad_caps[0]) - center(&fitted.quad_caps[0]);
        assert!((0.01..0.05).contains(&movement.x), "{movement:?}");
        assert!(center(&sculpted_fit.quad_caps[1]).dist(center(&fitted.quad_caps[1])) < 1e-12);
        assert_eq!(report.patch_roots.iter().filter(|&&id| id == second).count(), 1);

        for (removed, retained, seed) in [(first, second, first_edge), (second, first, second_edge)] {
            let mut open = both.clone();
            let next = hierarchy.delete_faces(&[removed]).unwrap();
            let cap = next.leaves.len();
            assert_eq!(next.quad_cap_faces().collect::<Vec<_>>(), [retained]);
            assert!(next.layout().unwrap().faces[removed].is_empty());
            open.hierarchy = Some(next);
            let (_, report) = convert(&open, 0.05).unwrap();
            assert!(report.patch_roots.contains(&retained));
            assert!(!report.patch_roots.contains(&removed));
            let recapped = author_quad_cap(&open, 0.05, &[seed], &[]).unwrap();
            let recapped_h = recapped.hierarchy.as_ref().unwrap();
            assert_eq!(recapped_h.quad_cap_faces().collect::<Vec<_>>(), [retained, cap]);
            assert!(recapped_h.layout().unwrap().faces[removed].is_empty());
            // The old survivor may precede a previously retired slot.
            let retired = recapped_h.delete_faces(&[retained]).unwrap();
            assert_eq!(retired.quad_cap_faces().collect::<Vec<_>>(), [cap]);
            let layout = retired.layout().unwrap();
            assert!(layout.faces[retained].is_empty());
            assert!(layout.faces[removed].is_empty());
        }

        let mut invalid = serde_json::to_value(&both).unwrap();
        let saved_first = invalid["hierarchy"]["quad_cap"].clone();
        invalid["hierarchy"]["additional_quad_caps"][0]["sides"] = saved_first["sides"].clone();
        let repeated_rim: Definition = serde_json::from_value(invalid).unwrap();
        assert!(convert(&repeated_rim, 0.05).is_err());
    }

    #[test]
    fn disjoint_quad_caps_close_a_box_and_either_cap_can_reopen_it() {
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;

        let parameters = Parameters { kind: "box".into(), dimensions: [20.0; 3], segments: [1; 3], ..Default::default() };
        let base = parameters.build().unwrap();
        let opposite = base.faces.iter().position(|face| face.iter().all(|v| !base.faces[0].contains(v))).unwrap();
        let [a, b] = [0, opposite].map(|id| [base.faces[id][0], base.faces[id][1]]);
        let hierarchy = Hierarchy::new(base).unwrap().delete_faces(&[0, opposite]).unwrap();
        let first = hierarchy.leaves.len();
        let second = first + 1;
        let source = Definition {
            cage: hierarchy.evaluate().unwrap(),
            hierarchy: Some(hierarchy),
            parameters,
            placement: Placement::default(),
            symmetry: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let single = author_quad_cap(&source, 0.05, &[a], &[]).unwrap();
        let both = author_quad_cap(&single, 0.05, &[b], &[]).unwrap();
        let (body, report) = convert(&both, 0.05).unwrap();
        assert!(report.solid && !body.is_surface() && crate::kernel::validity(&body).ok());
        assert_eq!(report.patch_roots.len(), 6);
        assert!(report.patch_roots.contains(&first) && report.patch_roots.contains(&second));
        let hierarchy = both.hierarchy.as_ref().unwrap();
        for (removed, retained) in [(first, second), (second, first)] {
            let mut reopened = both.clone();
            reopened.hierarchy = Some(hierarchy.delete_faces(&[removed]).unwrap());
            let (body, report) = convert(&reopened, 0.05).unwrap();
            assert!(!report.solid && body.is_surface() && crate::kernel::validity(&body).ok());
            assert!(report.patch_roots.contains(&retained) && !report.patch_roots.contains(&removed));
        }
    }

    #[test]
    fn refined_erase_and_fill_replaces_a_real_fine_sculpt_with_a_native_cap() {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::hierarchy::Hierarchy;
        let parameters = Parameters { kind: "face".into(), dimensions: [8.0, 8.0, 1.0], segments: [4, 4, 1], ..Default::default() };
        let mut hierarchy = Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[10], [4, 4]).unwrap();
        let selected: Vec<_> = hierarchy.leaves.iter().enumerate().filter_map(|(id, leaf)| (leaf.active && leaf.root == 10).then_some(id)).collect();
        let center = hierarchy.leaf_corners().unwrap()[selected[5]].unwrap()[2];
        hierarchy.add_deltas(&[center], &[Vec3::Z * 0.25]).unwrap();
        let before = Definition {
            placement: Default::default(),
            cage: hierarchy.evaluate().unwrap(),
            parameters,
            symmetry: None,
            hierarchy: Some(hierarchy.clone()),
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let previous = fitted_source(&before, 0.05).unwrap();
        assert!(previous.trimmed_roots, "the fixture must retain the old root chart");
        let original = previous.fit.patches[10].eval(0.5, 0.5).unwrap();
        assert!(original.z > 0.06, "the original fitted region must contain the fine sculpt");
        let next = hierarchy.erase_and_cap(&selected).unwrap();
        let face = next.quad_cap_face().unwrap();
        let after = Definition { hierarchy: Some(next), ..before.clone() };
        let (body, report, current) = convert_source(&after, 0.05).unwrap();
        assert!(body.is_surface() && crate::kernel::validity(&body).ok());
        assert!(current.trimmed_roots);
        for (root, (old, new)) in previous.fit.patches.iter().zip(&current.fit.patches).enumerate() {
            if root != 10 {
                for [u, v] in [[0.0, 0.0], [0.25, 0.75], [0.5, 0.5], [1.0, 1.0]] {
                    assert!(old.eval(u, v).unwrap().dist(new.eval(u, v).unwrap()) < 1e-9);
                }
            }
        }
        let native = report.patch_roots.iter().position(|&owner| owner == face).unwrap();
        let [[u0, u1], [v0, v1]] = body.record().faces[native].domain;
        let geometry = crate::kernel::face_geometry(&body, native).unwrap();
        let (point, _, _) = geometry.surface.derivs((u0 + u1) / 2.0, (v0 + v1) / 2.0).map(solidcraft_geom::tess::from_kernel3).unwrap();
        assert!(point.dist(Vec3::new(original.x, original.y, point.z)) < 1e-9);
        // The fine control also raises the exact rim. The cap follows that rim,
        // with a lower center than the sculpted patch at the same XY station.
        assert!(point.z > 0.0 && original.z - point.z > 0.05, "the new cap replaces the raised interior");
        assert!(selected.iter().all(|id| !report.patch_roots.contains(id)));
        let expected: std::collections::BTreeSet<_> =
            previous.fit.patch_roots.into_iter().filter(|id| !selected.contains(id)).chain([face]).collect();
        assert_eq!(report.patch_roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), expected);
        assert_eq!(after.cage, before.cage);
        assert_eq!(after.hierarchy.as_ref().unwrap().details, hierarchy.details);
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
    fn refined_plane_extrusion_compacts_only_preview_sheets_and_keeps_logical_routes() {
        use std::collections::{BTreeMap, BTreeSet};

        let parameters = Parameters { kind: "face".into(), dimensions: [64.0, 64.0, 1.0], segments: [8, 8, 1], ..Default::default() };
        let hierarchy = solidcraft_geom::subdivision::hierarchy::Hierarchy::new(parameters.build().unwrap())
            .unwrap()
            .subdivide(&[27], [2, 2])
            .unwrap()
            .extrude_faces(&[67], solidcore_record::Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        let definition = Definition {
            cage: hierarchy.evaluate().unwrap(),
            hierarchy: Some(hierarchy),
            parameters,
            placement: Default::default(),
            symmetry: None,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let (saved, old, guides) = convert_with_guides(&definition, 0.05).unwrap();
        let (preview, new, preview_guides, _) = convert_with_selection(&definition, 0.05).unwrap();
        assert_eq!(saved.face_count(), 260);
        assert!(preview.face_count() < saved.face_count());
        assert!(preview.is_surface() && crate::kernel::validity(&preview).ok());
        assert!(old.sampled_deviation_mm <= new.sampled_deviation_mm && new.sampled_deviation_mm <= 0.05);
        assert_eq!(old.patch_roots.iter().copied().collect::<BTreeSet<_>>(), new.patch_roots.iter().copied().collect());

        let owner_edges = |body: &crate::kernel::Body, report: &ConversionReport| {
            let mut edges = BTreeMap::new();
            for edge in &body.record().edges {
                let mut owners: BTreeSet<_> = edge.uses.iter().map(|use_| report.patch_roots[use_.face as usize]).collect();
                let first = owners.pop_first().unwrap();
                let second = owners.pop_first();
                if second.is_some() || edge.uses.len() == 1 {
                    *edges.entry((first, second)).or_insert(0usize) += 1;
                }
            }
            edges
        };
        assert_eq!(owner_edges(&saved, &old), owner_edges(&preview, &new));
        let (SelectionGuides::Subdivision(old), SelectionGuides::Subdivision(new)) = (guides, preview_guides) else {
            panic!("both bodies must retain exact subdivision guide routes");
        };
        assert_eq!(old.len(), new.len());
        for (a, b) in old.iter().zip(&new) {
            assert_eq!(a.edge, b.edge);
            assert_eq!(a.rational_pieces().unwrap(), b.rational_pieces().unwrap());
        }
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
    fn sphere_converts_at_tight_tolerance_without_losing_source_faces() {
        let parameters = Parameters { kind: "sphere".into(), dimensions: [10.0; 3], segments: [4, 4, 2], ..Default::default() };
        let definition = Definition {
            cage: parameters.build().unwrap(),
            parameters,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            placement: Default::default(),
            symmetry: None,
            hierarchy: None,
        };
        let (body, report) = convert(&definition, 0.01).unwrap();
        assert!(report.solid && !body.is_surface() && !body.is_mesh());
        assert_eq!(body.shell_count(), 1);
        assert_eq!(report.patches, body.face_count());
        assert_eq!(report.patches, definition.cage.faces.len());
        assert_eq!(report.patch_roots.iter().copied().collect::<std::collections::BTreeSet<_>>(), (0..definition.cage.faces.len()).collect());
        assert!(report.sampled_deviation_mm <= 0.01);
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
