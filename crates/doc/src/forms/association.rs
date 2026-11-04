//! Persistent Match references and recomputation; preview geometry is never stored here.
use super::{
    Definition,
    support::{BoundaryBinding, BoundaryCoordinate, BoundarySupport},
};
use crate::{BodyName, DocError, ModelBody, ModelState, Result};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::subdivision::{
    Cage, EdgeCrease, VertexCrease,
    fitting::RootBasis,
    hierarchy::{Hierarchy, Layout, LevelSharpness},
    matching::{self, Config, Domain, Report, Tolerances},
};
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}
fn bounded(point: Vec3) -> bool {
    point.is_finite() && point.x.abs().max(point.y.abs()).max(point.z.abs()) <= 100_000.0
}
fn name_valid(name: &str) -> bool {
    !name.is_empty() && name.len() <= 4096 && !name.contains('\0')
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedEdge {
    pub name: String,
    pub pick: Vec3,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SupportRef {
    Face { body: BodyName, name: String, pick: Vec3 },
    Wire { body: BodyName, edges: Vec<NamedEdge> },
}
impl SupportRef {
    pub fn face(body: &ModelBody, index: usize) -> Result<Self> {
        let name = crate::naming::face_names(body).get(index).cloned().ok_or_else(|| invalid("support face index is outside the body"))?;
        let pick = crate::naming::face_spot(body, index).ok_or_else(|| invalid("support face has no anchor point"))?.point;
        let source = Self::Face { body: body.name.clone().into(), name, pick };
        source.validate()?;
        Ok(source)
    }
    pub fn wire(body: &ModelBody, indices: &[usize]) -> Result<Self> {
        let names = crate::naming::edge_names(body);
        let edges = indices
            .iter()
            .map(|&i| {
                let name = names.get(i).cloned().ok_or_else(|| invalid("support edge index is outside the body"))?;
                let pick = crate::kernel::edge_geometry(&body.body, &[i])?
                    .first()
                    .and_then(|curve| curve.at(0.5))
                    .map(|p| p.0)
                    .ok_or_else(|| invalid("support edge has no exact anchor point"))?;
                Ok(NamedEdge { name, pick })
            })
            .collect::<Result<Vec<_>>>()?;
        let source = Self::Wire { body: body.name.clone().into(), edges };
        source.validate()?;
        Ok(source)
    }
    pub fn body(&self) -> &BodyName {
        match self {
            Self::Face { body, .. } | Self::Wire { body, .. } => body,
        }
    }
    pub fn transform_picks(&mut self, matrix: &crate::Mat) {
        match self {
            Self::Face { pick, .. } => *pick = crate::apply_point(matrix, *pick),
            Self::Wire { edges, .. } => {
                for edge in edges {
                    edge.pick = crate::apply_point(matrix, edge.pick);
                }
            }
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !name_valid(self.body().as_ref()) {
            return Err(invalid("Match support requires a bounded body name"));
        }
        match self {
            Self::Face { name, pick, .. } if name_valid(name) && bounded(*pick) => Ok(()),
            Self::Wire { edges, .. }
                if !edges.is_empty()
                    && edges.len() <= 256
                    && edges.iter().all(|e| name_valid(&e.name) && bounded(e.pick))
                    && edges.iter().map(|e| &e.name).collect::<BTreeSet<_>>().len() == edges.len() =>
            {
                Ok(())
            }
            _ => Err(invalid("Match support requires valid persistent names and finite anchor points")),
        }
    }
    pub fn resolve(&self, state: &ModelState) -> Result<BoundarySupport> {
        self.resolve_with(state, false)
    }
    pub fn resolve_with(&self, state: &ModelState, adjacent_faces: bool) -> Result<BoundarySupport> {
        self.validate()?;
        let body = state.body(self.body()).ok_or_else(|| invalid(format!("Match support body `{}` is missing", self.body())))?;
        match self {
            Self::Face { name, pick, .. } => {
                let (index, _) =
                    crate::naming::face_on_body(body, name, *pick).ok_or_else(|| invalid(format!("Match support face `{name}` is missing")))?;
                if adjacent_faces { BoundarySupport::from_adjacent_faces(&body.body, index) } else { BoundarySupport::from_face(&body.body, index) }
            }
            Self::Wire { edges, .. } => {
                let names = crate::naming::edge_names(body);
                let indices = edges
                    .iter()
                    .map(|edge| {
                        if let Some(index) = names.iter().position(|name| name == &edge.name) {
                            return Ok(index);
                        }
                        Ok(crate::naming::locate_edge(body, &edge.name, edge.pick).map_err(invalid)?.edge)
                    })
                    .collect::<Result<Vec<_>>>()?;
                BoundarySupport::from_edges(&body.body, &indices)
            }
        }
    }
    /// Body references are already remapped by the standard typed-reference walk.
    pub(crate) fn remap_names(&mut self, features: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>) {
        match self {
            Self::Face { name, .. } => *name = crate::remap_topology(name, features, bodies),
            Self::Wire { edges, .. } => {
                for edge in edges {
                    edge.name = crate::remap_topology(&edge.name, features, bodies);
                }
            }
        }
    }
}
impl crate::walk::Walk for SupportRef {
    fn walk_refs<'a>(&'a self, f: &mut dyn FnMut(crate::walk::RefItem<'a>)) {
        f(crate::walk::Ref::Body(self.body()));
    }
    fn walk_refs_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::RefItemMut<'a>)) {
        match self {
            Self::Face { body, .. } | Self::Wire { body, .. } => f(crate::walk::Ref::Body(body)),
        }
    }
    fn walk_geometry<'a>(&'a self, f: &mut dyn FnMut(crate::walk::GeoItem<'a>)) {
        match self {
            Self::Face { pick, .. } => f(crate::walk::Geo::Pick(pick)),
            Self::Wire { edges, .. } => {
                for edge in edges {
                    f(crate::walk::Geo::Pos(&edge.pick));
                }
            }
        }
    }
    fn walk_geometry_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::GeoItemMut<'a>)) {
        match self {
            Self::Face { pick, .. } => f(crate::walk::Geo::Pick(pick)),
            Self::Wire { edges, .. } => {
                for edge in edges {
                    f(crate::walk::Geo::Pos(&mut edge.pick));
                }
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VertexBackup {
    pub vertex: usize,
    pub previous: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeBackup {
    pub edge: [usize; 2],
    pub previous: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matching {
    #[serde(default, skip_serializing_if = "is_false")]
    pub adjacent_faces: bool,
    pub source: SupportRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<BoundaryBinding>,
    pub config: Config,
    pub coordinates: Vec<BoundaryCoordinate>,
    pub basis: RootBasis,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertices: Vec<VertexBackup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeBackup>,
    /// Creases inherited by refinement from an earlier Match evaluator.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crease_ancestors: Vec<LevelSharpness>,
}
pub struct MatchRequest {
    pub adjacent_faces: bool,
    pub source: SupportRef,
    pub edge: [usize; 2],
    pub order: usize,
    pub tolerances: Tolerances,
}
fn is_false(value: &bool) -> bool {
    !*value
}
pub(crate) fn domain_layout(hierarchy: Option<&Hierarchy>) -> Result<Option<Layout>> {
    hierarchy.map(Hierarchy::layout).transpose().map_err(|e| invalid(e.to_string()))
}
pub fn match_openings(cage: &Cage, hierarchy: Option<&Hierarchy>) -> Result<Vec<Vec<usize>>> {
    let layout = domain_layout(hierarchy)?;
    Domain::new(cage, layout.as_ref().map(|l| l.owners.as_slice())).and_then(Domain::loops).map_err(|e| invalid(e.to_string()))
}

fn install_evaluator(definition: &mut Definition, cage: Cage) -> Result<()> {
    if let Some(hierarchy) = &mut definition.hierarchy {
        *hierarchy = hierarchy.with_evaluator(&cage).map_err(|e| invalid(e.to_string()))?;
        definition.cage = hierarchy.evaluate().map_err(|e| invalid(e.to_string()))?;
    } else {
        definition.cage = cage;
    }
    Ok(())
}
impl Matching {
    pub fn validate(&self, cage: &Cage, hierarchy: Option<&Hierarchy>) -> Result<()> {
        self.source.validate()?;
        if let Some(binding) = &self.binding {
            binding.validate()?;
        }
        let layout = domain_layout(hierarchy)?;
        Domain::new(cage, layout.as_ref().map(|l| l.owners.as_slice()))
            .and_then(|domain| self.config.validate_domain(domain))
            .map_err(|e| invalid(e.to_string()))?;
        self.basis.validate().map_err(|e| invalid(e.to_string()))?;
        if self.coordinates.len() != self.config.coordinates.len()
            || self.coordinates.iter().any(|c| c.curve >= 1024 || !c.fraction.is_finite() || !(0.0..=1.0).contains(&c.fraction))
            || self.vertices.len() > 256
            || self.edges.len() > 256
        {
            return Err(invalid("Match metadata budget or coordinates are invalid"));
        }
        if self.crease_ancestors.len() > solidcraft_geom::subdivision::hierarchy::MAX_DEPTH
            || self.crease_ancestors.windows(2).any(|s| s[0].level >= s[1].level)
            || self.crease_ancestors.iter().any(|s| {
                hierarchy.is_none_or(|h| s.level >= h.depth)
                    || s.edges.len() > 256
                    || s.vertices.len() > 256
                    || s.edges.iter().any(|e| !e.sharpness.is_finite() || !(0.0..=10.0).contains(&e.sharpness))
                    || s.vertices.iter().any(|v| !v.sharpness.is_finite() || !(0.0..=10.0).contains(&v.sharpness))
            })
        {
            return Err(invalid("invalid Match crease ancestry"));
        }
        let weight = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v));
        if self.vertices.iter().any(|v| v.vertex >= cage.points.len() || !weight(v.previous))
            || self
                .edges
                .iter()
                .any(|e| e.edge[0] >= cage.points.len() || e.edge[1] >= cage.points.len() || e.edge[0] == e.edge[1] || !weight(e.previous))
            || self.vertices.iter().map(|v| v.vertex).collect::<BTreeSet<_>>().len() != self.vertices.len()
            || self.edges.iter().map(|e| (e.edge[0].min(e.edge[1]), e.edge[0].max(e.edge[1]))).collect::<BTreeSet<_>>().len() != self.edges.len()
        {
            return Err(invalid("invalid generated Match sharpness history"));
        }
        Ok(())
    }
    pub fn evaluation_order(&self, cage: &Cage, hierarchy: Option<&Hierarchy>) -> Result<usize> {
        let layout = domain_layout(hierarchy)?;
        Domain::new(cage, layout.as_ref().map(|l| l.owners.as_slice()))
            .and_then(|domain| domain.evaluation_order(&self.config))
            .map_err(|e| invalid(e.to_string()))
    }
    fn restore(&self, definition: &mut Definition, detach: bool) -> Result<()> {
        self.validate(&definition.cage, definition.hierarchy.as_ref())?;
        if detach && let Some(hierarchy) = &mut definition.hierarchy {
            for ancestor in &self.crease_ancestors {
                hierarchy.restore_level_sharpness(ancestor, 10.0).map_err(|e| invalid(e.to_string()))?;
            }
            definition.cage = hierarchy.evaluate().map_err(|e| invalid(e.to_string()))?;
        }
        let mut cage = definition.cage.clone();
        for old in &self.vertices {
            if cage.vertex_creases.iter().find(|c| c.vertex == old.vertex).is_some_and(|c| c.sharpness == 10.0) {
                cage.vertex_creases.retain(|c| c.vertex != old.vertex);
                if let Some(sharpness) = old.previous {
                    cage.vertex_creases.push(solidcraft_geom::subdivision::VertexCrease { vertex: old.vertex, sharpness });
                }
            }
        }
        for old in &self.edges {
            let key = |e: [usize; 2]| (e[0].min(e[1]), e[0].max(e[1]));
            if cage.edge_creases.iter().find(|c| key(c.edge) == key(old.edge)).is_some_and(|c| c.sharpness == 10.0) {
                cage.edge_creases.retain(|c| key(c.edge) != key(old.edge));
                if let Some(sharpness) = old.previous {
                    cage.edge_creases.push(solidcraft_geom::subdivision::EdgeCrease { edge: old.edge, sharpness });
                }
            }
        }
        cage.vertex_creases.sort_by_key(|v| v.vertex);
        cage.edge_creases.sort_by_key(|e| (e.edge[0].min(e.edge[1]), e.edge[0].max(e.edge[1])));
        cage.validate().map_err(|e| invalid(e.to_string()))?;
        install_evaluator(definition, cage)
    }
    fn history(before: &Cage, after: &Cage) -> (Vec<VertexBackup>, Vec<EdgeBackup>) {
        let vertices = after
            .vertex_creases
            .iter()
            .filter_map(|v| {
                let previous = before.vertex_creases.iter().find(|old| old.vertex == v.vertex).map(|v| v.sharpness);
                (previous != Some(v.sharpness)).then_some(VertexBackup { vertex: v.vertex, previous })
            })
            .collect();
        let key = |e: [usize; 2]| (e[0].min(e[1]), e[0].max(e[1]));
        let edges = after
            .edge_creases
            .iter()
            .filter_map(|e| {
                let previous = before.edge_creases.iter().find(|old| key(old.edge) == key(e.edge)).map(|e| e.sharpness);
                (previous != Some(e.sharpness)).then_some(EdgeBackup { edge: e.edge, previous })
            })
            .collect();
        (vertices, edges)
    }
    pub fn attach(definition: &Definition, state: &ModelState, request: MatchRequest, tolerance: f64) -> Result<(Definition, Report)> {
        if definition.profile.is_some() {
            return Err(invalid("Form Surface continuity is set on its original CAD rim"));
        }
        let (support, binding) = request.source.resolve_bound(state, request.adjacent_faces && request.order > 0, None)?;
        let mut definition = definition.clone();
        if let Some(previous) = definition.matching.take() {
            previous.restore(&mut definition, true)?;
        }
        let layout = domain_layout(definition.hierarchy.as_ref())?;
        let domain = Domain::new(&definition.cage, layout.as_ref().map(|l| l.owners.as_slice())).map_err(|e| invalid(e.to_string()))?;
        let edge = if domain.opening(request.edge).is_ok() {
            request.edge
        } else {
            let [a, b] = request.edge;
            let key = [a.min(b), a.max(b)];
            layout
                .as_ref()
                .and_then(|layout| layout.paths.iter().find(|(edge, _)| *edge == key))
                .and_then(|(_, path)| Some([*path.first()?, *path.get(1)?]))
                .ok_or_else(|| invalid("Match requires a complete free opening"))?
        };
        let coarse = definition.hierarchy.as_ref().map(|h| &h.base);
        let config = support.initialize_domain(domain, &definition.placement, edge, request.order, request.tolerances, coarse)?;
        let coordinates = config.coordinates.iter().map(|&f| support.coordinate(f)).collect::<Result<Vec<_>>>()?;
        let solution = support.solve(&definition, &config, tolerance)?;
        let (vertices, edges) = Self::history(&definition.cage, &solution.cage);
        install_evaluator(&mut definition, solution.cage)?;
        definition.matching = Some(Box::new(Self {
            adjacent_faces: request.adjacent_faces,
            source: request.source,
            binding: Some(binding),
            config,
            coordinates,
            basis: solution.basis,
            vertices,
            edges,
            crease_ancestors: Vec::new(),
        }));
        Ok((definition, solution.report))
    }
    pub fn recompute(&self, definition: &Definition, state: &ModelState, tolerance: f64) -> Result<(Definition, Report)> {
        let (support, binding) = self.bound_support(definition, state)?;
        let mut base = definition.clone();
        base.matching = None;
        self.restore(&mut base, false)?;
        let mut config = self.config.clone();
        config.coordinates = self.coordinates.iter().map(|c| support.fraction(c)).collect::<Result<Vec<_>>>()?;
        let solution = support.solve(&base, &config, tolerance)?;
        let (vertices, edges) = Self::history(&base.cage, &solution.cage);
        install_evaluator(&mut base, solution.cage)?;
        base.matching = Some(Box::new(Self {
            adjacent_faces: self.adjacent_faces,
            source: self.source.clone(),
            binding: Some(binding),
            config,
            coordinates: self.coordinates.clone(),
            basis: solution.basis,
            vertices,
            edges,
            crease_ancestors: self.crease_ancestors.clone(),
        }));
        Ok((base, solution.report))
    }
    fn bound_support(&self, definition: &Definition, state: &ModelState) -> Result<(BoundarySupport, BoundaryBinding)> {
        let (support, binding) = self.source.resolve_bound(state, self.adjacent_faces && self.config.order > 0, self.binding.as_ref())?;
        if self.binding.is_some() {
            return Ok((support, binding));
        }
        // Older records retained positional curve ordinals only. Recover their seam from the
        // authored limit constraints, never by picking the closest current curve after a move.
        let layout = domain_layout(definition.hierarchy.as_ref())?;
        let domain = Domain::new(&definition.cage, layout.as_ref().map(|l| l.owners.as_slice())).map_err(|e| invalid(e.to_string()))?;
        let ring = domain.opening(self.config.edge).map_err(|e| invalid(e.to_string()))?;
        let order = domain.evaluation_order(&self.config).map_err(|e| invalid(e.to_string()))?;
        let vertices = matching::evaluated_vertices(&definition.cage, order, 0).map_err(|e| invalid(e.to_string()))?;
        let points = ring
            .iter()
            .map(|&i| vertices.get(i).copied().map(|p| definition.placement.point(p)).ok_or_else(|| invalid("missing legacy Match limit constraint")))
            .collect::<Result<Vec<_>>>()?;
        let support = support.recover_legacy(&self.coordinates, &points, self.config.tolerances.position_mm)?;
        let binding = self.source.bind_support(state, &support)?;
        Ok((support, binding))
    }
    pub fn transform_picks(&mut self, matrix: &crate::Mat) {
        self.source.transform_picks(matrix);
        if let Some(binding) = &mut self.binding {
            binding.transform_picks(matrix);
        }
    }
    pub(crate) fn remap_names(&mut self, features: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>) {
        self.source.remap_names(features, bodies);
        if let Some(binding) = &mut self.binding {
            binding.remap_names(features, bodies);
        }
    }
    /// Follow a subdivision of the bound loop using the old vertex and curve
    /// identities. New controls get coordinates only within their parent span.
    pub fn rebind_refinement(
        &self,
        original: &Definition,
        mut refined: Definition,
        state: &ModelState,
        tolerance: f64,
    ) -> Result<(Definition, Report)> {
        self.validate(&original.cage, original.hierarchy.as_ref())?;
        let old_depth = original.hierarchy.as_ref().map_or(0, |h| h.depth);
        let depth = refined.hierarchy.as_ref().ok_or_else(|| invalid("expected a refined Match hierarchy"))?.depth;
        if depth < old_depth {
            return Err(invalid("the matched evaluator lost a refinement level"));
        }
        if depth > old_depth && self.evaluation_order(&original.cage, original.hierarchy.as_ref())? == 2 {
            let levels = depth - old_depth;
            let ordinary = original.cage.refined(levels).map_err(|e| invalid(e.to_string()))?;
            let continued = matching::refine_g2(&original.cage, levels).map_err(|e| invalid(e.to_string()))?;
            if continued.faces != refined.cage.faces || continued.points.len() != refined.cage.points.len() {
                return Err(invalid("refined G2 topology differs from the bound evaluator"));
            }
            let mut cage = refined.cage.clone();
            // Keep details authored by the refinement tool. Only transfer the
            // difference between the full continuation and open-cage stencils.
            for ((p, continued), ordinary) in cage.points.iter_mut().zip(continued.points).zip(ordinary.points) {
                *p += continued - ordinary;
            }
            install_evaluator(&mut refined, cage)?;
        }
        let layout = domain_layout(original.hierarchy.as_ref())?;
        let old_ring = Domain::new(&original.cage, layout.as_ref().map(|l| l.owners.as_slice()))
            .and_then(|domain| domain.opening(self.config.edge))
            .map_err(|e| invalid(e.to_string()))?;
        let new_ring = match_openings(&refined.cage, refined.hierarchy.as_ref())?
            .into_iter()
            .find(|ring| ring.iter().copied().filter(|&v| v < original.cage.points.len()).eq(old_ring.iter().copied()))
            .ok_or_else(|| invalid("local refinement changed the matched opening"))?;
        if new_ring.len() == old_ring.len() && depth == old_depth {
            return self.recompute(&refined, state, tolerance);
        }
        if new_ring.len() > 64 {
            return Err(invalid("refining this Match opening exceeds the 64 boundary control budget"));
        }
        let (support, binding) = self.bound_support(original, state)?;
        let mut config = self.config.clone();
        config.edge =
            [*new_ring.first().ok_or_else(|| invalid("empty refined opening"))?, *new_ring.get(1).ok_or_else(|| invalid("short refined opening"))?];
        let coordinates = support.refined_coordinates(&self.coordinates, &old_ring, &new_ring, &refined, &config)?;
        config.coordinates = coordinates.iter().map(|c| support.fraction(c)).collect::<Result<Vec<_>>>()?;
        let mut crease_ancestors = self.crease_ancestors.clone();
        if depth > old_depth && (!self.edges.is_empty() || !self.vertices.is_empty()) {
            crease_ancestors.push(LevelSharpness {
                level: old_depth,
                edges: self.edges.iter().map(|e| EdgeCrease { edge: e.edge, sharpness: e.previous.unwrap_or(0.0) }).collect(),
                vertices: self.vertices.iter().map(|v| VertexCrease { vertex: v.vertex, sharpness: v.previous.unwrap_or(0.0) }).collect(),
            });
        }
        let solution = support.solve(&refined, &config, tolerance)?;
        let (vertices, edges) = Self::history(&refined.cage, &solution.cage);
        install_evaluator(&mut refined, solution.cage)?;
        refined.matching = Some(Box::new(Self {
            adjacent_faces: self.adjacent_faces,
            source: self.source.clone(),
            binding: Some(binding),
            config,
            coordinates,
            basis: solution.basis,
            vertices,
            edges,
            crease_ancestors,
        }));
        Ok((refined, solution.report))
    }
}
impl crate::walk::Walk for Matching {
    fn walk_refs<'a>(&'a self, f: &mut dyn FnMut(crate::walk::RefItem<'a>)) {
        self.source.walk_refs(f)
    }
    fn walk_refs_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::RefItemMut<'a>)) {
        self.source.walk_refs_mut(f)
    }
    fn walk_geometry<'a>(&'a self, f: &mut dyn FnMut(crate::walk::GeoItem<'a>)) {
        self.source.walk_geometry(f);
        if let Some(binding) = &self.binding {
            binding.walk_geometry(f);
        }
    }
    fn walk_geometry_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::GeoItemMut<'a>)) {
        self.source.walk_geometry_mut(f);
        if let Some(binding) = &mut self.binding {
            binding.walk_geometry_mut(f);
        }
    }
}
