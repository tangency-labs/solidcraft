//! Transactional Forms editing sessions. Saved source lives in doc::forms::Definition.
use crate::{EngineError, Result};
use serde_json::{Value, json};
use solidcraft_geom::{
    Vec3,
    subdivision::{Cage, SurfacePreview, primitives::Parameters},
};

mod objects;
mod placement;
mod symmetry;
mod topology;

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
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    response: Option<(Vec<usize>, usize, Vec<Vec<f64>>)>,
    extrusion: Option<(Cage, Vec<usize>, Vec<[usize; 2]>, u64)>,
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
    pub fn set_parameters(&mut self, parameters: Parameters) -> Result<()> {
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
            let bounds: crate::geom::Aabb3 = self.surface.mesh.positions.iter().map(|&p| self.placement.local_point(p)).collect();
            let center = bounds.center();
            let size = bounds.size();
            let extents = [size.x, size.y, size.z];
            let mut factors = [1.0; 3];
            for i in 0..3 {
                if parameters.dimensions[i] != self.parameters.dimensions[i] && extents[i] > 1e-8 {
                    factors[i] = parameters.dimensions[i] / extents[i];
                }
            }
            for p in &mut candidate.points {
                let q = *p - center;
                *p = center + Vec3::new(q.x * factors[0], q.y * factors[1], q.z * factors[2]);
            }
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
            matching.validate(&cage)?;
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
            if hierarchy.is_some() {
                return Err(error("Match requires base topology"));
            }
            matching.validate(cage)?;
            return Ok((solidcraft_geom::subdivision::matching::preview(cage, matching.config.order, levels).map_err(error)?, cage.clone()));
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
        let (surface, logical) = Self::preview(
            &definition.cage,
            definition.hierarchy.as_ref(),
            definition.matching.as_deref(),
            definition.profile.as_deref(),
            self.levels,
        )?;
        self.checkpoint();
        self.freeze_pipe();
        self.cage = definition.cage;
        self.matching = definition.matching;
        self.match_report = Some(report);
        self.logical = logical;
        self.install_surface(surface);
        self.response = None;
        self.extrusion = None;
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn refine(&mut self, faces: &[usize], counts: [usize; 2]) -> Result<()> {
        self.refine_symmetric(faces, counts)
    }
    pub fn insert_points(&mut self, records: &[([usize; 2], f64)]) -> Result<()> {
        if self.symmetry.is_some() || !(2..=32).contains(&records.len()) {
            return Err(error("Insert Point needs 2..32 edge positions with symmetry off"));
        }
        let h =
            self.hierarchy.clone().map_or_else(|| solidcraft_geom::subdivision::hierarchy::Hierarchy::new(self.cage.clone()), Ok).map_err(error)?;
        let mut plan = Vec::new();
        let mut faces = std::collections::BTreeSet::new();
        let mut targets = Vec::new();
        for &(edge, fraction) in records {
            if !fraction.is_finite() || !(1e-6..=1.0 - 1e-6).contains(&fraction) {
                return Err(error("insert positions must be inside their edges"));
            }
            let curve = self
                .surface
                .boundaries
                .iter()
                .find(|(e, _)| (e[0].min(e[1]), e[0].max(e[1])) == (edge[0].min(edge[1]), edge[0].max(edge[1])))
                .ok_or_else(|| error("select a visible logical edge"))?;
            let fraction = if curve.0[0] <= curve.0[1] { fraction } else { 1.0 - fraction };
            let t = fraction * (curve.1.len().saturating_sub(1)) as f64;
            let i = t.floor() as usize;
            let a = *curve.1.get(i).ok_or_else(|| error("invalid insertion curve"))?;
            let b = curve.1.get(i + 1).copied().unwrap_or(a);
            targets.push(a.lerp(b, t - i as f64));
        }
        for pair in records.windows(2) {
            let target = h.insert_target(pair[0].0, pair[1].0).map_err(error)?;
            if !faces.insert(target.0) {
                return Err(error("an Insert Point chain cannot cross the same logical face twice"));
            }
            plan.push(target);
        }
        let mut refined = h;
        for &(face, axis) in &plan {
            refined = refined.subdivide(&[face], if axis == 0 { [2, 1] } else { [1, 2] }).map_err(error)?;
        }
        let mut ids = std::collections::BTreeMap::new();
        for (i, &(face, axis)) in plan.iter().enumerate() {
            let endpoints = refined.seam_endpoints(face, axis).map_err(error)?;
            let original_edge = records[i].0;
            // The seam's first endpoint is on side 0 (U split) or side 3 (V split).
            let layout = refined.layout().map_err(error)?;
            let adjacent = |vertex: usize| {
                layout.paths.iter().any(|(edge, _)| edge.contains(&vertex) && (edge.contains(&original_edge[0]) || edge.contains(&original_edge[1])))
            };
            let endpoints = if adjacent(endpoints[0]) { endpoints } else { [endpoints[1], endpoints[0]] };
            for (vertex, target) in endpoints.into_iter().zip([targets[i], targets[i + 1]]) {
                if let Some(old) = ids.insert(vertex, target)
                    && old.dist(target) > 1e-8
                {
                    return Err(error("Insert Point chain has incompatible shared endpoints"));
                }
            }
        }
        let cage = refined.evaluate().map_err(error)?;
        let levels = cage.preview_level(self.levels).map_err(error)?;
        let mut candidate = Self::from_definition(
            crate::doc::forms::Definition {
                placement: self.placement.clone(),
                cage,
                parameters: self.parameters.clone(),
                symmetry: None,
                hierarchy: Some(refined),
                imported: self.imported.clone(),
                pipe: None,
                profile: None,
                matching: None,
            },
            levels,
            self.editing_feature,
        )?;
        let indices: Vec<_> = ids.keys().copied().collect();
        let positions: Vec<_> = ids.values().map(|p| [p.x, p.y, p.z]).collect();
        candidate.set_surface(&indices, &positions, false)?;
        self.adopt_hierarchy(candidate.hierarchy.ok_or_else(|| error("missing inserted hierarchy"))?, true)
    }
    pub fn extrude_drag(&mut self, faces: &[usize], edges: &[[usize; 2]], displacement: Vec3, checkpoint: bool) -> Result<()> {
        if self.profile.is_some() {
            return Err(error("Form Surface keeps its fixed source boundary"));
        }
        if self.matching.is_some() {
            return Err(error("extrusion changes a matched opening's topology; match the resulting opening separately"));
        }
        let displacement = self.placement.local_vector(displacement);
        if self.hierarchy.is_some() || self.symmetry.is_some() {
            return Err(error("drag extrusion currently requires base topology with symmetry off"));
        }
        let source = if checkpoint {
            self.cage.clone()
        } else {
            let (c, f, e, rev) = self.extrusion.as_ref().ok_or_else(|| error("start an extrusion gesture first"))?;
            if *rev != self.revision || f != faces || e != edges {
                return Err(error("extrusion gesture is stale"));
            }
            c.clone()
        };
        let cage = if !faces.is_empty() && edges.is_empty() {
            source.extrude_faces(faces, displacement)
        } else if faces.is_empty() && !edges.is_empty() {
            source.extrude_edges(edges, displacement)
        } else {
            return Err(error("select faces or boundary edges to extrude"));
        }
        .map_err(error)?;
        if cage.faces.len() <= source.faces.len() {
            return Err(error("extrusion needs a region with a boundary"));
        }
        let surface = cage.surface(self.levels).map_err(error)?;
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
        self.extrusion = Some((source, faces.to_vec(), edges.to_vec(), self.revision));
        Ok(())
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
        let symmetry =
            self.symmetry.as_ref().map(|s| solidcraft_geom::subdivision::symmetry::Symmetry::new(&cage, s.axis)).transpose().map_err(error)?;
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
    fn set_surface_unconstrained(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool) -> Result<()> {
        if indices.len().saturating_mul(self.surface.mesh.triangles.len()) > 4_000_000 {
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
        if indices.is_empty()
            || indices.len() > 256
            || indices.len() != positions.len()
            || indices.len().saturating_mul(self.surface.mesh.triangles.len()) > 4_000_000
        {
            return Err(error("hierarchical edit exceeds the bounded surface solve budget"));
        }
        let unique: std::collections::BTreeSet<_> = indices.iter().copied().collect();
        if unique.len() != indices.len() {
            return Err(error("duplicate hierarchical selection"));
        }
        let mut h = self.hierarchy.clone().ok_or_else(|| error("missing hierarchy"))?;
        let mut matrix = vec![vec![0.0; indices.len()]; indices.len()];
        for (column, &vertex) in indices.iter().enumerate() {
            let basis = h.basis(vertex).map_err(error)?;
            let evaluated = if surface { basis.refined(self.levels).map_err(error)?.limit_vertices().map_err(error)? } else { basis.points };
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
            "parts":self.parts.iter().map(|(id,p)|json!({"id":id,"name":p.name,"point_count":p.definition.cage.points.len(),"face_count":p.definition.cage.faces.len(),"placement":p.definition.placement})).collect::<Vec<_>>(),
            "consumed_features":self.consumed.keys().copied().collect::<Vec<_>>(),
            "imported":self.imported,"hierarchy":self.hierarchy,"pipe":self.pipe,"pipe_segments":self.pipe_segments,"profile":self.profile,"matching":self.matching,"match_report":self.match_report,
            "edge_creases":self.cage.edge_creases,"vertex_creases":self.cage.vertex_creases,
            "symmetry":self.symmetry.as_ref().map(|s|s.axis),
            "levels":self.levels,"triangles":self.surface.mesh.triangles.len(),"undo":self.undo.len(),"redo":self.redo.len(),"revision":self.revision})
    }
    pub fn set_symmetry(&mut self, axis: Option<usize>) -> Result<()> {
        if self.profile.is_some() && axis.is_some() {
            return Err(error("Form Surface keeps its fixed source boundary"));
        }
        if axis.is_some() && self.matching.is_some() {
            return Err(error("Match currently requires symmetry off"));
        }
        let symmetry = axis.map(|axis| solidcraft_geom::subdivision::symmetry::Symmetry::new(&self.cage, axis)).transpose().map_err(error)?;
        if let Some(symmetry) = &symmetry {
            self.validate_logical_symmetry(symmetry)?;
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
        self.revision == other.revision
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
