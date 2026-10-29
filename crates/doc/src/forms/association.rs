//! Persistent Match references and recomputation; preview geometry is never stored here.
use super::{
    Definition,
    support::{BoundaryCoordinate, BoundarySupport},
};
use crate::{BodyName, DocError, ModelBody, ModelState, Result};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{
    Vec3,
    subdivision::{
        Cage,
        fitting::RootBasis,
        matching::{Config, Report, Tolerances},
    },
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
        let mesh = body.mesh();
        let edges = indices
            .iter()
            .map(|&i| {
                let name = names.get(i).cloned().ok_or_else(|| invalid("support edge index is outside the body"))?;
                let curve = mesh.edges.get(i).ok_or_else(|| invalid("support edge has no geometry"))?;
                let pick = curve.get(curve.len() / 2).copied().ok_or_else(|| invalid("support edge has no anchor point"))?;
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
                let (index, _, _) =
                    crate::naming::face_on_body(body, name, *pick).ok_or_else(|| invalid(format!("Match support face `{name}` is missing")))?;
                if adjacent_faces { BoundarySupport::from_adjacent_faces(&body.body, index) } else { BoundarySupport::from_face(&body.body, index) }
            }
            Self::Wire { edges, .. } => {
                let names = crate::naming::edge_names(body);
                let mesh = body.mesh();
                let indices = edges
                    .iter()
                    .map(|edge| {
                        if let Some(index) = names.iter().position(|name| name == &edge.name) {
                            return Ok(index);
                        }
                        let (point, _) = crate::naming::find_edge(body, &edge.name, edge.pick).map_err(invalid)?;
                        mesh.edges
                            .iter()
                            .enumerate()
                            .map(|(i, curve)| {
                                let distance = curve.windows(2).map(|p| point.dist_to_segment(p[0], p[1])).fold(f64::INFINITY, f64::min);
                                (i, distance)
                            })
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .map(|p| p.0)
                            .ok_or_else(|| invalid("named support edge has no geometry"))
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
    pub config: Config,
    pub coordinates: Vec<BoundaryCoordinate>,
    pub basis: RootBasis,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertices: Vec<VertexBackup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeBackup>,
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
impl Matching {
    pub fn validate(&self, cage: &Cage) -> Result<()> {
        self.source.validate()?;
        self.config.validate_topology(cage).map_err(|e| invalid(e.to_string()))?;
        self.basis.validate().map_err(|e| invalid(e.to_string()))?;
        if self.coordinates.len() != self.config.coordinates.len()
            || self.coordinates.iter().any(|c| c.curve >= 1024 || !c.fraction.is_finite() || !(0.0..=1.0).contains(&c.fraction))
            || self.vertices.len() > 256
            || self.edges.len() > 256
        {
            return Err(invalid("Match metadata budget or coordinates are invalid"));
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
    fn restore(&self, cage: &mut Cage) -> Result<()> {
        self.validate(cage)?;
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
        cage.validate().map_err(|e| invalid(e.to_string()))
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
        let support = request.source.resolve_with(state, request.adjacent_faces && request.order > 0)?;
        let mut definition = definition.clone();
        if let Some(previous) = definition.matching.take() {
            previous.restore(&mut definition.cage)?;
        }
        let config = support.initialize(&definition.cage, &definition.placement, request.edge, request.order, request.tolerances)?;
        let coordinates = config.coordinates.iter().map(|&f| support.coordinate(f)).collect::<Result<Vec<_>>>()?;
        let solution = support.solve(&definition, &config, tolerance)?;
        let (vertices, edges) = Self::history(&definition.cage, &solution.cage);
        definition.cage = solution.cage;
        definition.matching = Some(Box::new(Self {
            adjacent_faces: request.adjacent_faces,
            source: request.source,
            config,
            coordinates,
            basis: solution.basis,
            vertices,
            edges,
        }));
        Ok((definition, solution.report))
    }
    pub fn recompute(&self, definition: &Definition, state: &ModelState, tolerance: f64) -> Result<(Definition, Report)> {
        self.validate(&definition.cage)?;
        let support = self.source.resolve_with(state, self.adjacent_faces && self.config.order > 0)?;
        let mut base = definition.clone();
        base.matching = None;
        self.restore(&mut base.cage)?;
        let mut config = self.config.clone();
        config.coordinates = self.coordinates.iter().map(|c| support.fraction(c)).collect::<Result<Vec<_>>>()?;
        let solution = support.solve(&base, &config, tolerance)?;
        let (vertices, edges) = Self::history(&base.cage, &solution.cage);
        base.cage = solution.cage;
        base.matching = Some(Box::new(Self {
            adjacent_faces: self.adjacent_faces,
            source: self.source.clone(),
            config,
            coordinates: self.coordinates.clone(),
            basis: solution.basis,
            vertices,
            edges,
        }));
        Ok((base, solution.report))
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
        self.source.walk_geometry(f)
    }
    fn walk_geometry_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::GeoItemMut<'a>)) {
        self.source.walk_geometry_mut(f)
    }
}
