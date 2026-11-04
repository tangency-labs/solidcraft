//! Transactional Forms editing sessions. Saved source lives in doc::forms::Definition.
use crate::{EngineError, Result};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_geom::subdivision::{Cage, SurfacePreview, primitives::Parameters};

pub(crate) mod draft;
mod extrusion_preview;
mod objects;
mod placement;
mod symmetry;
mod topology;

pub use extrusion_preview::{ExtrusionPreview, ExtrusionResponse};
pub use objects::Part;

#[derive(Clone)]
struct Snapshot {
    imported: Option<Box<crate::doc::forms::Imported>>,
    pipe_segments: Vec<crate::geom::subdivision::pipe_network::SegmentGeometry>,
    pipe: Option<Box<crate::doc::forms::Pipe>>,
    profile: Option<Box<crate::doc::forms::Profile>>,
    report: Option<solidcraft_geom::subdivision::matching::Report>,
    matching: Option<Box<crate::doc::forms::Matching>>,
    parts: std::collections::BTreeMap<u64, Part>,
    consumed: std::collections::BTreeMap<u64, std::sync::Arc<crate::doc::Feature>>,
    next_part: u64,
    placement: crate::doc::forms::Placement,
    cage: Cage,
    levels: usize,
    parameters: Parameters,
    symmetry: Option<solidcraft_geom::subdivision::symmetry::Symmetry>,
    hierarchy: Option<solidcraft_geom::subdivision::hierarchy::Hierarchy>,
}

#[derive(Clone)]
enum ExtrusionSource {
    Base(Cage),
    Hierarchical(Box<solidcraft_geom::subdivision::hierarchy::Hierarchy>, Option<solidcraft_geom::subdivision::symmetry::Symmetry>),
}

#[derive(Clone)]
struct ExtrusionGesture {
    source: ExtrusionSource,
    match_source: Option<Box<crate::doc::forms::Definition>>,
    faces: Vec<usize>,
    edges: Vec<[usize; 2]>,
    front: Vec<[usize; 2]>,
    revision: u64,
}

#[derive(Clone)]
pub struct Form {
    pub imported: Option<Box<crate::doc::forms::Imported>>,
    pub pipe_segments: Vec<crate::geom::subdivision::pipe_network::SegmentGeometry>,
    pub pipe: Option<Box<crate::doc::forms::Pipe>>,
    pub profile: Option<Box<crate::doc::forms::Profile>>,
    pub match_report: Option<solidcraft_geom::subdivision::matching::Report>,
    pub matching: Option<Box<crate::doc::forms::Matching>>,
    pub parts: std::collections::BTreeMap<u64, Part>,
    pub(crate) consumed: std::collections::BTreeMap<u64, std::sync::Arc<crate::doc::Feature>>,
    next_part: u64,
    pub(crate) authoring_component: u64,
    pub(crate) authoring_path: Option<Vec<u64>>,
    pub placement: crate::doc::forms::Placement,
    local_vertices: Vec<Vec3>,
    pub(crate) tool_original: Option<std::sync::Arc<Form>>,
    pub cage: Cage,
    pub logical: Cage,
    pub hierarchy: Option<solidcraft_geom::subdivision::hierarchy::Hierarchy>,
    pub parameters: Parameters,
    pub symmetry: Option<solidcraft_geom::subdivision::symmetry::Symmetry>,
    pub editing_feature: Option<u64>,
    pub surface: SurfacePreview,
    pub levels: usize,
    pub revision: u64,
    draft_identity: u64,
    pub finish_options: crate::schema::forms_prototype::FinishOptions,
    finish_revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    response: Option<(Vec<usize>, usize, Vec<Vec<f64>>)>,
    extrusion: Option<ExtrusionGesture>,
}

/// Actual constrained source and its exact CAD extents in local authoring axes.
pub(crate) struct DimensionShape {
    pub definition: crate::doc::forms::Definition,
    pub bounds: crate::geom::Aabb3,
    pub match_report: Option<solidcraft_geom::subdivision::matching::Report>,
}

impl Form {
    pub fn replace_primitive(&mut self, next: Self) {
        self.checkpoint();
        self.imported = next.imported;
        self.pipe = next.pipe;
        self.pipe_segments = next.pipe_segments;
        self.cage = next.cage;
        self.profile = next.profile;
        self.matching = next.matching;
        self.match_report = next.match_report;
        self.placement = next.placement;
        self.local_vertices = next.local_vertices;
        self.logical = next.logical;
        self.hierarchy = next.hierarchy;
        self.parameters = next.parameters;
        self.symmetry = next.symmetry;
        self.surface = next.surface;
        self.levels = next.levels;
        self.response = None;
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
    }
    pub(crate) fn set_parameters(
        &mut self,
        parameters: Parameters,
        tolerance: f64,
        mut evaluate: impl FnMut(&crate::doc::forms::Definition) -> Result<DimensionShape>,
    ) -> Result<()> {
        if self.pipe.is_some() {
            return Err(error("use Pipe settings to regenerate its source path"));
        }
        if self.profile.is_some() {
            return Err(error("Form Surface resolution changes require a new source grid"));
        }
        if self.hierarchy.is_some() {
            return Err(error("primitive parameter changes on hierarchical Forms are not supported yet"));
        }
        let mut candidate = parameters.build().map_err(error)?;
        let mut resolved = None;
        if parameters.segments != self.parameters.segments
            || parameters.radii != self.parameters.radii
            || parameters.kind != self.parameters.kind
            || parameters.path != self.parameters.path
        {
            let original = self.parameters.build().map_err(error)?;
            if self.cage != original {
                return Err(error("changing primitive topology after sculpting requires shape-preserving resampling, which is not implemented yet"));
            }
        } else if self.parameters.build().is_ok_and(|original| original != self.cage) {
            candidate = self.cage.clone();
            let changed: [bool; 3] = std::array::from_fn(|i| parameters.dimensions[i] != self.parameters.dimensions[i]);
            if changed.iter().any(|&axis| axis) {
                let initial = evaluate(&self.definition())?;
                let baseline = initial.definition;
                let bounds = initial.bounds;
                if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
                    return Err(error("Form CAD bounds must be finite and nonempty"));
                }
                let center = bounds.center();
                let size = bounds.size();
                let mut previous_extents = [size.x, size.y, size.z];
                let mut previous_factors = [1.0; 3];
                let mut factors = [1.0; 3];
                for i in 0..3 {
                    if changed[i] {
                        if previous_extents[i] <= 1e-8 {
                            return Err(error("a Form dimension cannot scale an axis with no CAD extent"));
                        }
                        factors[i] = parameters.dimensions[i] / previous_extents[i];
                    }
                }
                // Always scale the same resolved baseline. A fixed support seam can
                // change the response to scaling, so qualify against the returned
                // constrained source and use bounded secant corrections.
                for _ in 0..6 {
                    candidate = baseline.cage.clone();
                    for point in &mut candidate.points {
                        let q = *point - center;
                        *point = center + Vec3::new(q.x * factors[0], q.y * factors[1], q.z * factors[2]);
                    }
                    candidate.validate().map_err(error)?;
                    let mut source = baseline.clone();
                    source.cage = candidate.clone();
                    source.parameters = parameters.clone();
                    let measured = evaluate(&source)?;
                    let bounds = measured.bounds;
                    if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
                        return Err(error("Form CAD bounds must be finite and nonempty"));
                    }
                    let size = bounds.size();
                    let extents = [size.x, size.y, size.z];
                    if (0..3).all(|i| !changed[i] || (extents[i] - parameters.dimensions[i]).abs() <= tolerance) {
                        candidate = measured.definition.cage.clone();
                        resolved = Some(measured);
                        break;
                    }
                    for i in 0..3 {
                        if !changed[i] || (extents[i] - parameters.dimensions[i]).abs() <= tolerance {
                            continue;
                        }
                        let response = extents[i] - previous_extents[i];
                        let step = factors[i] - previous_factors[i];
                        if response.abs() <= 1e-10 || step.abs() <= 1e-12 {
                            return Err(error("the fixed Form support prevents the requested CAD dimension"));
                        }
                        let next = factors[i] + (parameters.dimensions[i] - extents[i]) * step / response;
                        if !next.is_finite() || next <= 1e-8 {
                            return Err(error("the requested Form CAD dimension is incompatible with its support"));
                        }
                        previous_extents[i] = extents[i];
                        previous_factors[i] = factors[i];
                        factors[i] = next;
                    }
                }
                if resolved.is_none() {
                    return Err(error("Form CAD dimensions did not converge within the fitting tolerance"));
                }
            }
        }
        if let Some(measured) = resolved.filter(|shape| shape.definition.matching.is_some()) {
            let report = measured.match_report.ok_or_else(|| error("the qualified Form dimension lost its Match report"))?;
            self.install_matching(measured.definition, report, true)?;
            self.parameters = parameters;
            return Ok(());
        }
        if candidate == self.cage && parameters != self.parameters {
            self.checkpoint();
            self.redo.clear();
            self.revision = self.revision.wrapping_add(1);
        }
        self.replace_cage(candidate)?;
        self.parameters = parameters;
        Ok(())
    }
    pub fn replace_cage(&mut self, cage: Cage) -> Result<()> {
        if self.hierarchy.is_some() {
            return Err(error("this base topology tool cannot rewrite hierarchical leaves"));
        }
        self.adopt_base(cage)
    }
    fn adopt_base(&mut self, cage: Cage) -> Result<()> {
        if let Some(profile) = &self.profile {
            profile.validate(&cage)?;
            if profile.boundary().iter().any(|&i| cage.points.get(i) != self.cage.points.get(i)) {
                return Err(error("the Form Surface CAD rim is fixed"));
            }
        }
        let symmetry =
            self.symmetry.as_ref().map(|s| solidcraft_geom::subdivision::symmetry::Symmetry::new(&cage, s.axis)).transpose().map_err(error)?;
        if let Some(symmetry) = &symmetry {
            symmetry::validate_layout(&cage, None, symmetry)?;
        }
        let levels = cage.preview_level(self.levels).map_err(error)?;
        if self.matching.is_some() && (cage.faces != self.cage.faces || cage.points.len() != self.cage.points.len()) {
            return Err(error("changing a matched opening's topology requires matching the new opening again"));
        }
        let surface = Self::preview(&cage, None, self.matching.as_deref(), self.profile.as_deref(), levels)?.0;
        if cage == self.cage && self.hierarchy.is_none() {
            return Ok(());
        }
        self.checkpoint();
        self.freeze_pipe();
        self.cage = cage;
        self.symmetry = symmetry;
        self.logical = self.cage.clone();
        self.hierarchy = None;
        self.match_report = None;
        self.levels = levels;
        self.install_surface(surface);
        self.response = None;
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn create(kind: &str, size: f64, spans: usize, levels: usize) -> Result<Self> {
        Self::create_parameters(
            Parameters { kind: kind.into(), dimensions: [size; 3], segments: [spans; 3], radii: [size / 2.0, size / 5.0], ..Default::default() },
            levels,
        )
    }
    pub fn create_parameters(parameters: Parameters, levels: usize) -> Result<Self> {
        let cage = parameters.build().map_err(error)?;
        Self::from_definition(
            crate::doc::forms::Definition {
                placement: Default::default(),
                cage,
                parameters,
                symmetry: None,
                hierarchy: None,
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
            },
            levels,
            None,
        )
    }
    pub fn from_definition(definition: crate::doc::forms::Definition, levels: usize, editing_feature: Option<u64>) -> Result<Self> {
        let crate::doc::forms::Definition { imported, pipe, profile, matching, placement, cage, parameters, symmetry, hierarchy } = definition;
        if symmetry.is_some() && hierarchy.as_ref().is_some_and(|h| h.has_trim_wall()) {
            return Err(error("trim walls require symmetry off"));
        }
        if let Some(source) = &imported {
            source.validate()?;
        }
        placement.validate()?;
        if let Some(pipe) = &pipe {
            pipe.validate()?;
            if profile.is_some() || (pipe.follow_path && (matching.is_some() || symmetry.is_some() || hierarchy.is_some())) {
                return Err(error("parametric pipes require their generated base cage"));
            }
        }
        if let Some(matching) = &matching {
            matching.validate(&cage, hierarchy.as_ref())?;
        }
        if let Some(profile) = &profile {
            profile.validate(&cage)?;
            if matching.is_some() || symmetry.is_some() || hierarchy.is_some() {
                return Err(error("Form Surface requires its fixed guide grid"));
            }
        }
        parameters.validate().map_err(error)?;
        if let Some(s) = &symmetry {
            s.validate(&cage).map_err(error)?;
        }
        if let Some(h) = &hierarchy
            && h.evaluate().map_err(error)? != cage
        {
            return Err(error("Form hierarchy does not match its evaluator"));
        }
        let (surface, logical) = Self::preview(&cage, hierarchy.as_ref(), matching.as_deref(), profile.as_deref(), levels)?;
        let local_vertices = surface.vertices.clone();
        let surface = placement::placed_surface(surface, &placement);
        Ok(Self {
            imported,
            pipe_segments: Vec::new(),
            pipe,
            profile,
            match_report: None,
            matching,
            parts: Default::default(),
            consumed: Default::default(),
            next_part: 1,
            authoring_component: 0,
            authoring_path: None,
            placement,
            local_vertices,
            tool_original: None,
            cage,
            logical,
            hierarchy,
            parameters,
            symmetry,
            editing_feature,
            surface,
            levels,
            revision: 1,
            draft_identity: draft::new_identity(),
            finish_options: Default::default(),
            finish_revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            response: None,
            extrusion: None,
        })
    }
    fn preview(
        cage: &Cage,
        hierarchy: Option<&solidcraft_geom::subdivision::hierarchy::Hierarchy>,
        matching: Option<&crate::doc::forms::Matching>,
        profile: Option<&crate::doc::forms::Profile>,
        levels: usize,
    ) -> Result<(SurfacePreview, Cage)> {
        if let Some(profile) = profile {
            return Ok((profile.preview(cage)?, cage.clone()));
        }
        if let Some(matching) = matching {
            if hierarchy.is_some_and(|h| h.has_trim_wall()) {
                return Err(error("Match requires removing the derived trim wall"));
            }
            matching.validate(cage, hierarchy)?;
            let order = matching.evaluation_order(cage, hierarchy)?;
            let surface = solidcraft_geom::subdivision::matching::preview(cage, order, levels).map_err(error)?;
            return match hierarchy {
                Some(h) => h.layout_surface(cage, surface).map_err(error),
                None => Ok((surface, cage.clone())),
            };
        }
        match hierarchy {
            Some(h) => h.surface(cage, levels).map_err(error),
            None => Ok((cage.surface(levels).map_err(error)?, cage.clone())),
        }
    }
    pub fn definition(&self) -> crate::doc::forms::Definition {
        crate::doc::forms::Definition {
            imported: self.imported.clone(),
            pipe: self.pipe.clone(),
            profile: self.profile.clone(),
            matching: self.matching.clone(),
            placement: self.placement.clone(),
            cage: self.cage.clone(),
            parameters: self.parameters.clone(),
            symmetry: self.symmetry.clone(),
            hierarchy: self.hierarchy.clone(),
        }
    }
    pub fn apply_matching(
        &mut self,
        definition: crate::doc::forms::Definition,
        report: solidcraft_geom::subdivision::matching::Report,
    ) -> Result<()> {
        self.install_matching(definition, report, true)
    }
    /// Incorporate a qualified release solve into the existing edit gesture.
    /// The caller verifies that the input snapshot is still current.
    pub fn refresh_match_preview(
        &mut self,
        definition: crate::doc::forms::Definition,
        report: solidcraft_geom::subdivision::matching::Report,
    ) -> Result<()> {
        if self.definition() == definition {
            self.match_report = Some(report);
            return Ok(());
        }
        self.install_matching(definition, report, false)
    }
    fn install_matching(
        &mut self,
        definition: crate::doc::forms::Definition,
        report: solidcraft_geom::subdivision::matching::Report,
        checkpoint: bool,
    ) -> Result<()> {
        let (surface, logical) = Self::preview(
            &definition.cage,
            definition.hierarchy.as_ref(),
            definition.matching.as_deref(),
            definition.profile.as_deref(),
            self.levels,
        )?;
        if checkpoint {
            self.checkpoint();
            self.redo.clear();
        }
        self.freeze_pipe();
        self.cage = definition.cage;
        self.hierarchy = definition.hierarchy;
        self.symmetry = definition.symmetry;
        self.matching = definition.matching;
        self.match_report = Some(report);
        self.logical = logical;
        self.install_surface(surface);
        self.response = None;
        self.extrusion = None;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn refine(&mut self, faces: &[usize], counts: [usize; 2]) -> Result<()> {
        self.refine_symmetric(faces, counts)
    }
    pub(crate) fn refine_with_match(&mut self, state: &crate::doc::ModelState, edit: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        let Some(matching) = &self.matching else { return edit(self) };
        let mut candidate = self.clone();
        candidate.matching = None;
        candidate.match_report = None;
        edit(&mut candidate)?;
        let (definition, report) = matching.rebind_refinement(&self.definition(), candidate.definition(), state, 0.01)?;
        candidate.refresh_match_preview(definition, report)?;
        self.checkpoint();
        candidate.undo = std::mem::take(&mut self.undo);
        candidate.redo.clear();
        *self = candidate;
        Ok(())
    }
    pub(crate) fn insert_points(&mut self, records: &[([usize; 2], f64)], state: Option<&crate::doc::ModelState>, tolerance: f64) -> Result<()> {
        if self.symmetry.is_some() || !(2..=32).contains(&records.len()) {
            return Err(error("Insert Point needs 2..32 edge positions with symmetry off"));
        }
        if self.cage.faces.iter().any(|face| face.len() != 4) {
            return Err(error("Insert Point requires a quad source cage"));
        }
        if records.iter().any(|(_, fraction)| !fraction.is_finite() || !(1e-6..=1.0 - 1e-6).contains(fraction)) {
            return Err(error("insert positions must be inside their edges"));
        }
        let mut source = self.definition();
        let mut candidate = self.clone();
        if let Some(matching) = &source.matching {
            let state = state.ok_or_else(|| error("matched insertion requires its source document"))?;
            let (resolved, report) = matching.recompute(&source, state, tolerance)?;
            candidate.refresh_match_preview(resolved.clone(), report)?;
            source = resolved;
        }
        // Resolve the original matched CAD curves before removing Match for the
        // topology operation. Neither targets nor response masks read a mesh.
        let edges: Vec<_> = records.iter().map(|(edge, _)| *edge).collect();
        let curves = crate::doc::forms::edge_curves(&source, tolerance, &edges)?;
        let targets = curves.iter().zip(records).map(|(curve, (_, fraction))| curve.at(*fraction).map_err(Into::into)).collect::<Result<Vec<_>>>()?;
        candidate.matching = None;
        candidate.match_report = None;
        let (hierarchy, inserted) = candidate.insertion_topology(records)?;
        candidate.adopt_hierarchy(hierarchy, false)?;
        if let Some(matching) = &source.matching {
            let state = state.ok_or_else(|| error("matched insertion requires its source document"))?;
            let (definition, report) = matching.rebind_refinement(&source, candidate.definition(), state, tolerance)?;
            candidate.refresh_match_preview(definition, report)?;
        }
        // Rebind continuation before the inverse endpoint solve. Otherwise a
        // later G2 transfer would move an endpoint that was already positioned.
        let mut positions = std::collections::BTreeMap::new();
        for (&vertex, &target) in inserted.iter().zip(&targets) {
            if positions.insert(vertex, target).is_some_and(|previous: Vec3| previous.dist(target) > 1e-8) {
                return Err(error("Insert Point chain has incompatible shared endpoints"));
            }
        }
        let indices: Vec<_> = positions.keys().copied().collect();
        let points: Vec<_> = positions.values().map(|point| [point.x, point.y, point.z]).collect();
        candidate.set_surface(&indices, &points, false)?;
        if let Some(matching) = candidate.matching.clone() {
            let state = state.ok_or_else(|| error("matched insertion requires its source document"))?;
            let (definition, report) = matching.recompute(&candidate.definition(), state, tolerance)?;
            candidate.refresh_match_preview(definition, report)?;
        }
        for (vertex, target) in inserted.into_iter().zip(&targets) {
            let position = candidate.local_vertices.get(vertex).copied().ok_or_else(|| error("inserted source endpoint is missing"))?;
            if candidate.world_point(position).dist(*target) > tolerance.max(1e-8) {
                return Err(error("Match could not retain the inserted point within the CAD tolerance"));
            }
        }
        self.checkpoint();
        candidate.undo = std::mem::take(&mut self.undo);
        candidate.redo.clear();
        *self = candidate;
        Ok(())
    }

    fn insertion_topology(&self, records: &[([usize; 2], f64)]) -> Result<(solidcraft_geom::subdivision::hierarchy::Hierarchy, Vec<usize>)> {
        let hierarchy =
            self.hierarchy.clone().map_or_else(|| solidcraft_geom::subdivision::hierarchy::Hierarchy::new(self.cage.clone()), Ok).map_err(error)?;
        let mut plan = Vec::new();
        let mut faces = std::collections::BTreeSet::new();
        for pair in records.windows(2) {
            let target = hierarchy.insert_target(pair[0].0, pair[1].0).map_err(error)?;
            if !faces.insert(target.0) {
                return Err(error("an Insert Point chain cannot cross the same logical face twice"));
            }
            plan.push(target);
        }
        let mut refined = hierarchy;
        for &(face, axis) in &plan {
            refined = refined.subdivide(&[face], if axis == 0 { [2, 1] } else { [1, 2] }).map_err(error)?;
        }
        let layout = refined.layout().map_err(error)?;
        let mut record_vertices = vec![None; records.len()];
        for (i, &(face, axis)) in plan.iter().enumerate() {
            let endpoints = refined.seam_endpoints(face, axis).map_err(error)?;
            let original = records[i].0;
            let adjacent = |vertex: usize| {
                layout.paths.iter().any(|(edge, _)| edge.contains(&vertex) && (edge.contains(&original[0]) || edge.contains(&original[1])))
            };
            let endpoints = if adjacent(endpoints[0]) { endpoints } else { [endpoints[1], endpoints[0]] };
            for (offset, vertex) in endpoints.into_iter().enumerate() {
                if record_vertices[i + offset].replace(vertex).is_some_and(|previous| previous != vertex) {
                    return Err(error("Insert Point chain has incompatible endpoint identities"));
                }
            }
        }
        let vertices = record_vertices
            .into_iter()
            .map(|vertex| vertex.ok_or_else(|| error("missing inserted endpoint identity")))
            .collect::<Result<Vec<_>>>()?;
        Ok((refined, vertices))
    }

    /// Rebind a topology edit away from the associated opening before installing it.
    /// A drag always evaluates from its original matched source, so later moves
    /// change its displacement without accumulating walls or Undo checkpoints.
    pub(crate) fn extrude_drag_with_match(
        &mut self,
        state: &crate::doc::ModelState,
        faces: &[usize],
        edges: &[[usize; 2]],
        displacement: Vec3,
        checkpoint: bool,
    ) -> Result<()> {
        if self.matching.is_none() {
            return self.extrude_drag(faces, edges, displacement, checkpoint);
        }
        let (mut candidate, original) = self.matched_extrusion_candidate(faces, edges, displacement, checkpoint)?;
        let matching = original.matching.as_ref().ok_or_else(|| error("extrusion has no associated Match source"))?;
        let mut gesture = candidate.extrusion.take().ok_or_else(|| error("missing extrusion gesture"))?;
        let (definition, report) = matching.rebind_refinement(&original, candidate.definition(), state, 0.01)?;
        candidate.refresh_match_preview(definition, report)?;
        if checkpoint {
            self.checkpoint();
        }
        candidate.undo = std::mem::take(&mut self.undo);
        candidate.redo.clear();
        gesture.match_source = Some(Box::new(original));
        gesture.revision = candidate.revision;
        candidate.extrusion = Some(gesture);
        *self = candidate;
        Ok(())
    }
    fn matched_extrusion_candidate(
        &self,
        faces: &[usize],
        edges: &[[usize; 2]],
        displacement: Vec3,
        checkpoint: bool,
    ) -> Result<(Self, crate::doc::forms::Definition)> {
        let original = if checkpoint {
            self.definition()
        } else {
            let gesture = self.extrusion.as_ref().ok_or_else(|| error("start an extrusion gesture first"))?;
            if gesture.revision != self.revision || gesture.faces != faces || gesture.edges != edges {
                return Err(error("extrusion gesture is stale"));
            }
            gesture.match_source.as_deref().ok_or_else(|| error("extrusion has no associated Match source"))?.clone()
        };
        let matching = original.matching.as_ref().ok_or_else(|| error("extrusion has no associated Match source"))?;
        let ring = crate::doc::forms::match_openings(&original.cage, original.hierarchy.as_ref())?
            .into_iter()
            .find(|ring| matching.config.edge.iter().all(|v| ring.contains(v)))
            .ok_or_else(|| error("the matched opening is no longer available"))?;
        let logical = original.hierarchy.as_ref().map(|h| h.layout()).transpose().map_err(error)?;
        let logical = logical.as_ref().map_or(original.cage.faces.as_slice(), |layout| layout.faces.as_slice());
        let selected = original
            .symmetry
            .as_ref()
            .map_or_else(|| Ok(faces.to_vec()), |symmetry| symmetry.mirror_face_selection(logical, faces).map_err(error))?;
        if selected.iter().any(|&id| logical.get(id).is_none_or(|face| face.iter().any(|v| ring.contains(v))))
            || edges.iter().flatten().any(|v| ring.contains(v))
        {
            return Err(error("extrude faces or free edges away from the matched opening"));
        }
        let mut candidate = self.clone();
        candidate.matching = None;
        candidate.match_report = None;
        if checkpoint && candidate.hierarchy.is_none() {
            candidate.hierarchy = Some(solidcraft_geom::subdivision::hierarchy::Hierarchy::new(candidate.cage.clone()).map_err(error)?);
        }
        candidate.extrude_drag(faces, edges, displacement, checkpoint)?;
        Ok((candidate, original))
    }
    pub(crate) fn extrude_once_with_match(
        &mut self,
        state: &crate::doc::ModelState,
        faces: &[usize],
        edges: &[[usize; 2]],
        displacement: Vec3,
    ) -> Result<()> {
        self.extrude_drag_with_match(state, faces, edges, displacement, true)?;
        self.extrusion = None;
        Ok(())
    }
    pub fn extrude_drag(&mut self, faces: &[usize], edges: &[[usize; 2]], displacement: Vec3, checkpoint: bool) -> Result<()> {
        if self.profile.is_some() {
            return Err(error("Form Surface keeps its fixed source boundary"));
        }
        if self.matching.is_some() {
            return Err(error("extrusion changes a matched opening's topology; match the resulting opening separately"));
        }
        let displacement = self.placement.local_vector(displacement);
        let source = if checkpoint {
            let hierarchy = if self.hierarchy.is_none() && self.symmetry.is_some() {
                Some(solidcraft_geom::subdivision::hierarchy::Hierarchy::new(self.cage.clone()).map_err(error)?)
            } else {
                self.hierarchy.clone()
            };
            hierarchy.map_or_else(
                || ExtrusionSource::Base(self.cage.clone()),
                |hierarchy| ExtrusionSource::Hierarchical(Box::new(hierarchy), self.symmetry.clone()),
            )
        } else {
            let gesture = self.extrusion.as_ref().ok_or_else(|| error("start an extrusion gesture first"))?;
            if gesture.revision != self.revision || gesture.faces != faces || gesture.edges != edges {
                return Err(error("extrusion gesture is stale"));
            }
            gesture.source.clone()
        };
        let front = match &source {
            ExtrusionSource::Hierarchical(hierarchy, symmetry) => {
                let (edited, front) = match (symmetry, faces.is_empty(), edges.is_empty()) {
                    (Some(symmetry), false, true) => (hierarchy.extrude_symmetric_faces(faces, displacement, symmetry).map_err(error)?, Vec::new()),
                    (Some(symmetry), true, false) => hierarchy.extrude_symmetric_edges(edges, displacement, symmetry).map_err(error)?,
                    (None, false, true) => (hierarchy.extrude_faces(faces, displacement).map_err(error)?, Vec::new()),
                    (None, true, false) => hierarchy.extend_edges(edges, displacement).map_err(error)?,
                    _ => return Err(error("select faces or boundary edges to extrude")),
                };
                self.adopt_hierarchy(edited, checkpoint)?;
                front
            }
            ExtrusionSource::Base(original) => {
                let cage = if !faces.is_empty() && edges.is_empty() {
                    original.extrude_faces(faces, displacement)
                } else if faces.is_empty() && !edges.is_empty() {
                    original.extrude_edges(edges, displacement)
                } else {
                    return Err(error("select faces or boundary edges to extrude"));
                }
                .map_err(error)?;
                if cage.faces.len() <= original.faces.len() {
                    return Err(error("extrusion needs a region with a boundary"));
                }
                let surface = cage.surface(self.levels).map_err(error)?;
                let front = if faces.is_empty() {
                    cage.faces
                        .iter()
                        .skip(original.faces.len())
                        .filter_map(|wall| {
                            let (&a, &b) = wall.get(2).zip(wall.get(3))?;
                            Some([a.min(b), a.max(b)])
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                if checkpoint {
                    self.checkpoint();
                }
                self.freeze_pipe();
                self.cage = cage;
                self.logical = self.cage.clone();
                self.install_surface(surface);
                self.response = None;
                self.redo.clear();
                self.revision = self.revision.wrapping_add(1);
                front
            }
        };
        self.extrusion =
            Some(ExtrusionGesture { source, match_source: None, faces: faces.to_vec(), edges: edges.to_vec(), front, revision: self.revision });
        Ok(())
    }
    /// A complete topology edit with no subsequent gesture updates.
    pub(crate) fn extrude_once(&mut self, faces: &[usize], edges: &[[usize; 2]], displacement: Vec3) -> Result<()> {
        self.extrude_drag(faces, edges, displacement, true)?;
        self.extrusion = None;
        Ok(())
    }
    /// Logical edges translated by the current edge extrusion gesture.
    pub fn extruded_edges(&self) -> &[[usize; 2]] {
        self.extrusion.as_ref().filter(|gesture| gesture.revision == self.revision).map_or(&[], |gesture| &gesture.front)
    }
    pub fn delete_logical_faces(&mut self, faces: &[usize]) -> Result<()> {
        let h =
            self.hierarchy.clone().map_or_else(|| solidcraft_geom::subdivision::hierarchy::Hierarchy::new(self.cage.clone()), Ok).map_err(error)?;
        self.adopt_hierarchy(h.delete_faces(faces).map_err(error)?, true)
    }
    pub fn dissolve_logical_edges(&mut self, edges: &[[usize; 2]]) -> Result<()> {
        let h =
            self.hierarchy.clone().map_or_else(|| solidcraft_geom::subdivision::hierarchy::Hierarchy::new(self.cage.clone()), Ok).map_err(error)?;
        self.adopt_hierarchy(h.dissolve_edges(edges).map_err(error)?, true)
    }
    fn adopt_hierarchy(&mut self, h: solidcraft_geom::subdivision::hierarchy::Hierarchy, checkpoint: bool) -> Result<()> {
        if self.profile.is_some() {
            return Err(error("Form Surface keeps its original regular guide grid"));
        }
        let cage = h.evaluate().map_err(error)?;
        let symmetry = self
            .symmetry
            .as_ref()
            .map(|s| solidcraft_geom::subdivision::symmetry::Symmetry::at(&cage, s.axis, s.origin))
            .transpose()
            .map_err(error)?;
        let levels = cage.preview_level(self.levels).map_err(error)?;
        let (surface, logical) = Self::preview(&cage, Some(&h), self.matching.as_deref(), self.profile.as_deref(), levels)?;
        if let Some(symmetry) = &symmetry {
            symmetry::validate_layout(&logical, Some(&h), symmetry)?;
        }
        if checkpoint {
            self.checkpoint();
        }
        self.freeze_pipe();
        self.cage = cage;
        self.symmetry = symmetry;
        self.logical = logical;
        self.hierarchy = Some(h);
        self.match_report = None;
        self.install_surface(surface);
        self.levels = levels;
        self.response = None;
        self.extrusion = None;
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            imported: self.imported.clone(),
            pipe_segments: self.pipe_segments.clone(),
            pipe: self.pipe.clone(),
            profile: self.profile.clone(),
            report: self.match_report.clone(),
            matching: self.matching.clone(),
            parts: self.parts.clone(),
            consumed: self.consumed.clone(),
            next_part: self.next_part,
            placement: self.placement.clone(),
            cage: self.cage.clone(),
            levels: self.levels,
            parameters: self.parameters.clone(),
            symmetry: self.symmetry.clone(),
            hierarchy: self.hierarchy.clone(),
        }
    }
    fn set_local(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool) -> Result<()> {
        if self.hierarchy.is_some() {
            return self.set_hierarchical(indices, positions, checkpoint, false);
        }
        if indices.is_empty() || indices.len() != positions.len() || indices.len() > self.cage.points.len() {
            return Err(EngineError::Other("select matching vertices and positions".into()));
        }
        let mut cage = self.cage.clone();
        let mut targets = std::collections::BTreeMap::new();
        let mut unique = std::collections::BTreeSet::new();
        let locked = self.profile.as_ref().map(|p| p.boundary()).unwrap_or_default();
        for (&i, p) in indices.iter().zip(positions) {
            if !unique.insert(i) {
                return Err(EngineError::Other("duplicate control vertex".into()));
            }
            cage.point(i).map_err(error)?;
            let point = Vec3::new(p[0], p[1], p[2]);
            if !point.is_finite() || point.len() > 100_000.0 {
                return Err(error("invalid Form guide position"));
            }
            if !locked.contains(&i) {
                targets.insert(i, point);
            }
        }
        if let Some(symmetry) = &self.symmetry {
            symmetry.constrain(&cage, &mut targets).map_err(error)?;
        }
        for (i, p) in targets {
            *cage.points.get_mut(i).ok_or_else(|| error("invalid mirrored control"))? = p;
        }
        if cage == self.cage {
            return Ok(());
        }
        let surface = Self::preview(&cage, None, self.matching.as_deref(), self.profile.as_deref(), self.levels)?.0;
        if checkpoint {
            self.checkpoint();
        }
        self.freeze_pipe();
        self.cage = cage;
        self.logical = self.cage.clone();
        self.match_report = None;
        self.install_surface(surface);
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    fn checkpoint(&mut self) {
        if self.undo.len() >= 64 {
            self.undo.remove(0);
        }
        self.undo.push(self.snapshot());
    }
    fn freeze_pipe(&mut self) {
        if let Some(pipe) = &mut self.pipe {
            pipe.follow_path = false;
        }
    }
    pub fn quality(&mut self, levels: usize) -> Result<()> {
        if levels == self.levels {
            return Ok(());
        }
        let (surface, logical) = Self::preview(&self.cage, self.hierarchy.as_ref(), self.matching.as_deref(), self.profile.as_deref(), levels)?;
        self.checkpoint();
        self.redo.clear();
        self.levels = levels;
        self.install_surface(surface);
        self.logical = logical;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// Move evaluated surface vertices to the requested positions while holding other controls
    /// fixed. The inverse subdivision response keeps the picked surface under the dragger.
    fn set_surface_local(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool) -> Result<()> {
        if self.profile.is_some() {
            return self.set_local(indices, positions, checkpoint);
        }
        if self.hierarchy.is_some() {
            return self.set_hierarchical(indices, positions, checkpoint, true);
        }
        if indices.len() != positions.len() {
            return Err(error("select matching vertices and positions"));
        }
        if let Some(symmetry) = &self.symmetry {
            let mut targets = std::collections::BTreeMap::new();
            for (&i, p) in indices.iter().zip(positions) {
                if targets.insert(i, Vec3::new(p[0], p[1], p[2])).is_some() {
                    return Err(error("duplicate surface vertex"));
                }
            }
            symmetry.constrain(&self.cage, &mut targets).map_err(error)?;
            let ids: Vec<_> = targets.keys().copied().collect();
            let positions: Vec<_> = targets.values().map(|p| [p.x, p.y, p.z]).collect();
            return self.set_surface_unconstrained(&ids, &positions, checkpoint);
        }
        self.set_surface_unconstrained(indices, positions, checkpoint)
    }
    // Bound evaluator work from authored topology, independently of cached
    // viewport triangles (which can have different trimming or display quality).
    fn surface_solve_cost(&self, targets: usize) -> usize {
        let faces = &self.cage.faces;
        let samples = if self.levels == 0 {
            faces.iter().fold(0_usize, |sum, face| sum.saturating_add(face.len().saturating_sub(2)))
        } else {
            let growth = u32::try_from(self.levels - 1).ok().and_then(|level| 4_usize.checked_pow(level)).unwrap_or(usize::MAX);
            faces.iter().fold(0_usize, |sum, face| sum.saturating_add(face.len())).saturating_mul(growth).saturating_mul(2)
        };
        targets.saturating_mul(samples)
    }
    fn set_surface_unconstrained(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool) -> Result<()> {
        if self.surface_solve_cost(indices.len()) > 4_000_000 {
            return Err(error("surface solve budget exceeded; lower preview subdivision or select fewer elements"));
        }
        if indices.is_empty() || indices.len() != positions.len() || indices.len() > 256 {
            return Err(error("surface edits require 1..256 matching vertices and positions"));
        }
        let unique: std::collections::BTreeSet<_> = indices.iter().copied().collect();
        if unique.len() != indices.len() || indices.iter().any(|&i| i >= self.cage.points.len()) {
            return Err(error("surface selection contains duplicate or invalid vertices"));
        }
        let cached = self.response.as_ref().is_some_and(|(ids, levels, _)| ids == indices && *levels == self.levels);
        if !cached {
            let mut matrix = vec![vec![0.0; indices.len()]; indices.len()];
            for (column, &control) in indices.iter().enumerate() {
                let mut basis = self.cage.clone();
                basis.points.fill(Vec3::ZERO);
                basis.points.get_mut(control).ok_or_else(|| error("invalid surface control"))?.x = 1.0;
                let evaluated = solidcraft_geom::subdivision::matching::evaluated_vertices(
                    &basis,
                    self.matching.as_ref().map_or(0, |m| m.config.order),
                    self.levels,
                )
                .map_err(error)?;
                for (row, &vertex) in indices.iter().enumerate() {
                    let coefficient = evaluated.get(vertex).ok_or_else(|| error("invalid response vertex"))?.x;
                    *matrix.get_mut(row).and_then(|r| r.get_mut(column)).ok_or_else(|| error("invalid response matrix"))? = coefficient;
                }
            }
            self.response = Some((indices.to_vec(), self.levels, matrix));
        }
        let matrix = &self.response.as_ref().ok_or_else(|| error("missing surface response"))?.2;
        let mut rhs = Vec::with_capacity(indices.len());
        for (&i, p) in indices.iter().zip(positions) {
            let target = Vec3::new(p[0], p[1], p[2]);
            if !target.is_finite() {
                return Err(error("surface positions must be finite"));
            }
            rhs.push(target - *self.local_vertices.get(i).ok_or_else(|| error("invalid evaluated vertex"))?);
        }
        let delta = solve_response(matrix, rhs)?;
        let points = indices
            .iter()
            .zip(delta)
            .map(|(&i, d)| {
                let p = self.cage.point(i).map_err(error)? + d;
                Ok([p.x, p.y, p.z])
            })
            .collect::<Result<Vec<_>>>()?;
        self.set_local(indices, &points, checkpoint)
    }
    fn set_hierarchical(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool, surface: bool) -> Result<()> {
        if indices.len() != positions.len() || indices.is_empty() || indices.len() > 256 {
            return Err(error("select at most256 matching hierarchical targets"));
        }
        if indices.iter().any(|&id| id >= self.cage.points.len()) {
            return self.set_trim_rim(indices, positions, checkpoint, surface);
        }
        let mut targets: std::collections::BTreeMap<_, _> =
            indices.iter().copied().zip(positions.iter().map(|p| Vec3::new(p[0], p[1], p[2]))).collect();
        if targets.len() != indices.len() {
            return Err(error("duplicate hierarchical selection"));
        }
        if let Some(symmetry) = &self.symmetry {
            symmetry.constrain(&self.cage, &mut targets).map_err(error)?;
        }
        let selected: Vec<_> = targets.keys().copied().collect();
        let positions: Vec<_> = targets.values().map(|p| [p.x, p.y, p.z]).collect();
        let indices = selected.as_slice();
        if indices.is_empty() || indices.len() > 256 || indices.len() != positions.len() || self.surface_solve_cost(indices.len()) > 4_000_000 {
            return Err(error("hierarchical edit exceeds the bounded surface solve budget"));
        }
        let unique: std::collections::BTreeSet<_> = indices.iter().copied().collect();
        if unique.len() != indices.len() {
            return Err(error("duplicate hierarchical selection"));
        }
        let mut h = self.hierarchy.clone().ok_or_else(|| error("missing hierarchy"))?;
        let order = self.matching.as_ref().map_or(Ok(0), |m| m.evaluation_order(&self.cage, Some(&h)))?;
        let mut matrix = vec![vec![0.0; indices.len()]; indices.len()];
        for (column, &vertex) in indices.iter().enumerate() {
            let basis = h.basis(vertex).map_err(error)?;
            let evaluated = if surface {
                solidcraft_geom::subdivision::matching::evaluated_vertices(&basis, order, self.levels).map_err(error)?
            } else {
                basis.points
            };
            for (row, &id) in indices.iter().enumerate() {
                matrix[row][column] = evaluated.get(id).ok_or_else(|| error("invalid hierarchical target"))?.x;
            }
        }
        let source = if surface { &self.local_vertices } else { &self.cage.points };
        let mut rhs = Vec::with_capacity(indices.len());
        for (&vertex, p) in indices.iter().zip(&positions) {
            let target = Vec3::new(p[0], p[1], p[2]);
            if !target.is_finite() {
                return Err(error("hierarchical targets must be finite"));
            }
            rhs.push(target - *source.get(vertex).ok_or_else(|| error("invalid hierarchical target"))?);
        }
        if rhs.iter().all(|v| v.len() < 1e-12) {
            return Ok(());
        }
        let deltas = solve_response(&matrix, rhs)?;
        h.add_deltas(indices, &deltas).map_err(error)?;
        self.adopt_hierarchy(h, checkpoint)
    }
    fn set_trim_rim(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool, surface: bool) -> Result<()> {
        let h = self.hierarchy.as_ref().filter(|h| h.has_trim_wall()).ok_or_else(|| error("missing derived trim wall"))?;
        let first = self.cage.points.len();
        if indices.len() > 2 || indices.iter().any(|&i| i < first || i > first + 1) || (indices.len() == 2 && indices[0] == indices[1]) {
            return Err(error("select only distinct trim wall rim controls to translate the wall"));
        }
        let source = if surface { &self.local_vertices } else { &self.logical.points };
        let mut deltas = indices.iter().zip(positions).map(|(&i, p)| {
            let old = source.get(i).ok_or_else(|| error("missing trim rim control"))?;
            let delta = Vec3::new(p[0], p[1], p[2]) - *old;
            delta.is_finite().then_some(delta).ok_or_else(|| error("trim wall rim targets must be finite"))
        });
        let delta = deltas.next().ok_or_else(|| error("select a trim wall rim control"))??;
        if let Some(other) = deltas.next()
            && other?.dist(delta) > 1e-8
        {
            return Err(error("trim wall rim controls translate together"));
        }
        if delta.len() < 1e-12 {
            return Ok(());
        }
        self.adopt_hierarchy(h.move_trim_rim(delta).map_err(error)?, checkpoint)
    }
    pub fn selectable_control(&self, id: usize) -> bool {
        self.hierarchy
            .as_ref()
            .is_none_or(|h| h.controls.contains_key(&id) || (h.has_trim_wall() && id >= self.cage.points.len() && id < self.logical.points.len()))
    }
    pub fn history(&mut self, redo: bool) -> Result<()> {
        let snap = if redo { self.redo.last() } else { self.undo.last() }
            .cloned()
            .ok_or_else(|| EngineError::Other("no Forms history in that direction".into()))?;
        let (surface, logical) = Self::preview(&snap.cage, snap.hierarchy.as_ref(), snap.matching.as_deref(), snap.profile.as_deref(), snap.levels)?;
        let current = self.snapshot();
        if redo {
            self.redo.pop();
            self.undo.push(current);
        } else {
            self.undo.pop();
            self.redo.push(current);
        }
        self.cage = snap.cage;
        self.imported = snap.imported;
        self.pipe = snap.pipe;
        self.pipe_segments = snap.pipe_segments;
        self.profile = snap.profile;
        self.matching = snap.matching;
        self.match_report = snap.report;
        self.parts = snap.parts;
        self.consumed = snap.consumed;
        self.next_part = snap.next_part;
        self.placement = snap.placement;
        self.logical = logical;
        self.hierarchy = snap.hierarchy;
        self.parameters = snap.parameters;
        self.symmetry = snap.symmetry;
        self.response = None;
        self.levels = snap.levels;
        self.install_surface(surface);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn inspect(&self) -> Value {
        json!({"active":true,"placement":self.placement,"tool_preview":self.tool_original.is_some(),"points":self.control_positions().iter().map(|p| [p.x,p.y,p.z]).collect::<Vec<_>>(),"faces":self.logical.faces,"parameters":self.parameters,"editing_feature":self.editing_feature,
            "finish_options":self.finish_options,
            "parts":self.parts.iter().map(|(id,p)|json!({"id":id,"name":p.name,"point_count":p.definition.cage.points.len(),"face_count":p.definition.cage.faces.len(),"placement":p.definition.placement})).collect::<Vec<_>>(),
            "consumed_features":self.consumed.keys().copied().collect::<Vec<_>>(),
            "imported":self.imported,"hierarchy":self.hierarchy,"pipe":self.pipe,"pipe_segments":self.pipe_segments,"profile":self.profile,"matching":self.matching,"match_report":self.match_report,
            "edge_creases":self.cage.edge_creases,"vertex_creases":self.cage.vertex_creases,
            "symmetry":self.symmetry.as_ref().map(|s|s.axis),
            "levels":self.levels,"triangles":self.surface.mesh.triangles.len(),"undo":self.undo.len(),"redo":self.redo.len(),"revision":self.revision})
    }
    pub fn set_symmetry(&mut self, axis: Option<usize>) -> Result<()> {
        if axis.is_some() && self.hierarchy.as_ref().is_some_and(|h| h.has_trim_wall()) {
            return Err(error("remove the derived trim wall before enabling symmetry"));
        }
        if self.profile.is_some() && axis.is_some() {
            return Err(error("Form Surface keeps its fixed source boundary"));
        }
        let symmetry = axis.map(|axis| solidcraft_geom::subdivision::symmetry::Symmetry::new(&self.cage, axis)).transpose().map_err(error)?;
        if let Some(symmetry) = &symmetry {
            self.validate_logical_symmetry(symmetry)?;
            if self.matching.is_some() {
                symmetry.validate_reflection(&self.cage).map_err(error)?;
            }
        }
        if symmetry != self.symmetry {
            self.checkpoint();
            self.freeze_pipe();
            self.symmetry = symmetry;
            self.redo.clear();
            self.response = None;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }
    pub(crate) fn same_authoring_source(&self, other: &Self) -> bool {
        self.draft_identity == other.draft_identity
            && self.revision == other.revision
            && self.finish_options == other.finish_options
            && self.levels == other.levels
            && self.editing_feature == other.editing_feature
            && self.tool_original.is_some() == other.tool_original.is_some()
            && self.cage == other.cage
            && self.placement == other.placement
            && self.parts == other.parts
            && self.consumed == other.consumed
            && self.authoring_component == other.authoring_component
            && self.authoring_path == other.authoring_path
            && self.parameters == other.parameters
            && self.symmetry == other.symmetry
            && self.hierarchy == other.hierarchy
            && self.matching == other.matching
            && self.profile == other.profile
            && self.imported == other.imported
            && self.pipe == other.pipe
    }
    pub fn has_tool_preview(&self) -> bool {
        self.tool_original.is_some()
    }

    pub(crate) fn update_finish_options(&mut self, update: crate::schema::forms_prototype::FinishOptions) {
        let next = crate::schema::forms_prototype::FinishOptions {
            tolerance: update.tolerance.or_else(|| self.finish_options.tolerance.clone()),
            operation: update.operation.or_else(|| self.finish_options.operation.clone()),
        };
        if next != self.finish_options {
            self.finish_options = next;
            self.finish_revision = self.finish_revision.wrapping_add(1);
        }
    }
}

fn validate_tolerance_text(text: &str) -> Result<&str> {
    if text.len() > 1024 || text.contains('\0') || text.trim().is_empty() {
        return Err(error("invalid Form CAD tolerance expression"));
    }
    Ok(text.trim())
}

pub(crate) fn finish_tolerance(session: &crate::Session, text: &str) -> Result<f64> {
    let text = validate_tolerance_text(text)?;
    text.parse::<f64>()
        .ok()
        .or_else(|| session.doc.eval(text, crate::doc::expr::Kind::Length).ok())
        .filter(|value| (1e-6..=1.0).contains(value))
        .ok_or_else(|| error("Form CAD tolerance must be between 0.000001 and 1 mm"))
}

/// Check the durable source without evaluating expressions against parameters
/// that may have changed since the Form was last edited.
pub(crate) fn checked_finish_options(
    mut options: crate::schema::forms_prototype::FinishOptions,
) -> Result<crate::schema::forms_prototype::FinishOptions> {
    if let Some(tolerance) = &options.tolerance {
        validate_tolerance_text(tolerance)?;
    }
    if let Some(operation) = &options.operation {
        use crate::doc::Operation;
        let operation = match (operation.len() <= 32).then(|| Operation::parse(operation)).flatten() {
            Some(Operation::NewBody) => "new",
            Some(Operation::Join) => "join",
            Some(Operation::Cut) => "cut",
            Some(Operation::Intersect) => "intersect",
            None => return Err(error("unknown Form operation")),
        };
        options.operation = Some(operation.into());
    }
    Ok(options)
}

/// Derived exact curves for an insertion marker, prepared on a worker. The
/// original Match is recomputed before extracting its saved-basis curves.
pub fn insertion_curves(session: &crate::Session, tolerance: f64, edges: &[[usize; 2]]) -> Result<Vec<crate::doc::forms::EdgeCurve>> {
    let form = session.forms_prototype.as_ref().ok_or_else(|| error("no Form is being edited"))?;
    let mut source = form.definition();
    if let Some(matching) = &source.matching {
        let before = form.editing_feature.map_or_else(|| session.model.state(), |id| session.model.state_before(id));
        let mut document = std::borrow::Cow::Borrowed(session.doc.as_ref());
        if !session.pending_moves.is_empty() {
            for occurrence in &mut document.to_mut().occurrences {
                if let Some(matrix) = session.pending_moves.get(&occurrence.id) {
                    occurrence.transform = *matrix;
                }
            }
        }
        let state = crate::doc::world_state(&document, &before)?;
        source = matching.recompute(&source, &state, tolerance)?.0;
    }
    let all;
    let edges = if edges.is_empty() {
        let layout = source.hierarchy.as_ref().map(|h| h.layout()).transpose().map_err(error)?;
        let faces = layout.as_ref().map_or(source.cage.faces.as_slice(), |layout| layout.faces.as_slice());
        let mut unique = std::collections::BTreeSet::new();
        for face in faces {
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                unique.insert([a.min(b), a.max(b)]);
            }
        }
        all = unique.into_iter().collect::<Vec<_>>();
        all.as_slice()
    } else {
        edges
    };
    crate::doc::forms::edge_curves(&source, tolerance, edges).map_err(Into::into)
}

fn solve_response(matrix: &[Vec<f64>], rhs: Vec<Vec3>) -> Result<Vec<Vec3>> {
    let n = rhs.len();
    if n == 0 || matrix.len() != n || matrix.iter().any(|r| r.len() != n) {
        return Err(error("invalid surface response dimensions"));
    }
    let mut a = matrix.to_vec();
    let mut b = rhs.clone();
    let mut pivots = Vec::new();
    for column in 0..n {
        let row = pivots.len();
        if row == n {
            break;
        }
        let pivot = (row..n).max_by(|&i, &j| a[i][column].abs().total_cmp(&a[j][column].abs())).ok_or_else(|| error("missing surface pivot"))?;
        let diagonal = a[pivot][column];
        if !diagonal.is_finite() {
            return Err(error("nonfinite surface response"));
        }
        if diagonal.abs() < 1e-12 {
            continue;
        }
        a.swap(row, pivot);
        b.swap(row, pivot);
        for value in &mut a[row] {
            *value /= diagonal;
        }
        b[row] = b[row] / diagonal;
        let source = a[row].clone();
        let value = b[row];
        for i in 0..n {
            if i != row {
                let factor = a[i][column];
                for (v, &s) in a[i].iter_mut().zip(&source) {
                    *v -= factor * s;
                }
                b[i] -= value * factor;
            }
        }
        pivots.push(column);
    }
    let scale = rhs.iter().map(|v| v.len()).fold(1.0_f64, f64::max);
    if b.iter().skip(pivots.len()).any(|v| v.len() > 1e-9 * scale) {
        return Err(error("requested surface positions conflict with the subdivision constraints"));
    }
    let mut out = vec![Vec3::ZERO; n];
    for (row, &column) in pivots.iter().enumerate() {
        out[column] = b[row];
    }
    // Some complete limit stencils have genuine null modes (for example the cube's
    // alternating corner mode). Keep the minimum-norm compatible control displacement.
    let free: Vec<_> = (0..n).filter(|c| !pivots.contains(c)).collect();
    if !free.is_empty() {
        let nulls: Vec<Vec<f64>> = free
            .into_iter()
            .map(|column| {
                let mut z = vec![0.0; n];
                z[column] = 1.0;
                for (row, &pivot) in pivots.iter().enumerate() {
                    z[pivot] = -a[row][column];
                }
                z
            })
            .collect();
        let gram: Vec<Vec<f64>> = nulls.iter().map(|x| nulls.iter().map(|y| x.iter().zip(y).map(|(a, b)| a * b).sum()).collect()).collect();
        let projections: Vec<_> = nulls.iter().map(|z| out.iter().zip(z).fold(Vec3::ZERO, |v, (&p, &w)| v + p * w)).collect();
        let weights = solve_full(&gram, projections)?;
        for (z, weight) in nulls.iter().zip(weights) {
            for (p, &w) in out.iter_mut().zip(z) {
                *p -= weight * w;
            }
        }
    }
    for (row, wanted) in matrix.iter().zip(rhs) {
        let actual = row.iter().zip(&out).fold(Vec3::ZERO, |v, (&w, &p)| v + p * w);
        if !actual.is_finite() || actual.dist(wanted) > 1e-8 * scale {
            return Err(error("surface response could not meet its positional tolerance"));
        }
    }
    Ok(out)
}
fn solve_full(matrix: &[Vec<f64>], mut rhs: Vec<Vec3>) -> Result<Vec<Vec3>> {
    let mut a = matrix.to_vec();
    let n = rhs.len();
    for k in 0..n {
        let pivot = (k..n)
            .max_by(|&x, &y| {
                let v = |i: usize| a.get(i).and_then(|r| r.get(k)).copied().unwrap_or(0.0).abs();
                v(x).total_cmp(&v(y))
            })
            .ok_or_else(|| error("empty surface response"))?;
        a.swap(k, pivot);
        rhs.swap(k, pivot);
        let row = a.get(k).cloned().ok_or_else(|| error("invalid surface response"))?;
        let diagonal = *row.get(k).ok_or_else(|| error("invalid surface response"))?;
        if !diagonal.is_finite() || diagonal.abs() < 1e-12 {
            return Err(error("surface selection has no independent control response"));
        }
        let source = *rhs.get(k).ok_or_else(|| error("invalid surface response"))?;
        for i in k + 1..n {
            let r = a.get_mut(i).ok_or_else(|| error("invalid surface response"))?;
            let factor = *r.get(k).ok_or_else(|| error("invalid surface response"))? / diagonal;
            for j in k..n {
                *r.get_mut(j).ok_or_else(|| error("invalid surface response"))? -=
                    factor * row.get(j).copied().ok_or_else(|| error("invalid surface response"))?;
            }
            let value = rhs.get_mut(i).ok_or_else(|| error("invalid surface response"))?;
            *value -= source * factor;
        }
    }
    let mut out = vec![Vec3::ZERO; n];
    for i in (0..n).rev() {
        let row = a.get(i).ok_or_else(|| error("invalid surface response"))?;
        let mut b = *rhs.get(i).ok_or_else(|| error("invalid surface response"))?;
        for j in i + 1..n {
            b -= *out.get(j).ok_or_else(|| error("invalid surface response"))?
                * row.get(j).copied().ok_or_else(|| error("invalid surface response"))?;
        }
        let value = b / row.get(i).copied().ok_or_else(|| error("invalid surface response"))?;
        if !value.is_finite() {
            return Err(error("surface control response exceeded its bounds"));
        }
        *out.get_mut(i).ok_or_else(|| error("invalid surface response"))? = value;
    }
    Ok(out)
}
fn error(e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(e.to_string())
}
