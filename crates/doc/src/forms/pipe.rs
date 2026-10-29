//! Persistent path references for a parametric pipe; the sculpted cage remains editable.
use super::{NamedEdge, SupportRef};
use crate::{BodyName, DocError, ModelBody, ModelState, Result, SketchId, SolvedSketch, kernel};
use serde::{Deserialize, Serialize};
use solidcraft_geom::subdivision::{
    Cage,
    pipe_network::{BuiltPipe, Network, PathEdge, PipeSettings},
};
use std::collections::{BTreeMap, BTreeSet};

mod sampling;

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipeSource {
    Sketch { sketch: SketchId, curves: Vec<String> },
    Edges { body: BodyName, edges: Vec<NamedEdge> },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipe {
    pub source: PipeSource,
    pub settings: PipeSettings,
    /// Regenerate from the source until the user sculpts this cage. The path
    /// association and overrides remain available for an explicit regeneration.
    pub follow_path: bool,
}
impl Pipe {
    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        self.settings.validate().map_err(|e| invalid(e.to_string()))
    }
    pub fn rebuild(&self, state: &ModelState) -> Result<BuiltPipe> {
        self.validate()?;
        self.source.network(state)?.build(&self.settings).map_err(|e| invalid(e.to_string()))
    }
    pub fn recompute(&self, cage: &Cage, state: &ModelState) -> Result<Cage> {
        self.validate()?;
        if self.follow_path {
            Ok(self.rebuild(state)?.cage)
        } else {
            cage.validate().map_err(|e| invalid(e.to_string()))?;
            Ok(cage.clone())
        }
    }
}
impl PipeSource {
    pub fn transform_picks(&mut self, matrix: &crate::Mat) {
        if let Self::Edges { edges, .. } = self {
            for edge in edges {
                edge.pick = crate::apply_point(matrix, edge.pick);
            }
        }
    }
    pub fn from_sketch(sketch: &SolvedSketch, curves: &[String]) -> Result<Self> {
        let curves = if curves.is_empty() {
            sketch
                .sketch
                .curves
                .iter()
                .filter(|c| !c.construction)
                .map(|c| c.id.clone())
                .chain(sketch.sketch.wires.iter().filter(|w| w.pts.len() > 1).map(|w| w.id.clone()))
                .collect()
        } else {
            curves.to_vec()
        };
        let source = Self::Sketch { sketch: sketch.feature.into(), curves };
        source.validate()?;
        Ok(source)
    }
    pub fn from_edges(body: &ModelBody, indices: &[usize]) -> Result<Self> {
        let SupportRef::Wire { body, edges } = SupportRef::wire(body, indices)? else {
            return Err(invalid("expected path edges"));
        };
        let source = Self::Edges { body, edges };
        source.validate()?;
        Ok(source)
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Sketch { sketch, curves } => {
                if sketch.0 == 0
                    || curves.is_empty()
                    || curves.len() > 128
                    || curves.iter().any(|id| id.is_empty() || id.len() > 1024 || id.contains('\0'))
                    || curves.iter().collect::<BTreeSet<_>>().len() != curves.len()
                {
                    return Err(invalid("a pipe sketch source needs 1..128 distinct bounded curve identities"));
                }
                Ok(())
            }
            Self::Edges { body, edges } => {
                if edges.len() > 128 {
                    return Err(invalid("select at most 128 pipe source edges"));
                }
                SupportRef::Wire { body: body.clone(), edges: edges.clone() }.validate()
            }
        }
    }
    /// `state` is already in the Form author's component frame. WireSpace and
    /// sketch plane placement are resolved by the standard state-frame adapter.
    pub fn network(&self, state: &ModelState) -> Result<Network> {
        self.validate()?;
        let mut source = Vec::new();
        let mut points_used = 0;
        match self {
            Self::Sketch { sketch, curves } => {
                let solved = state.sketches.iter().find(|s| s.feature == sketch.0).ok_or_else(|| invalid("the pipe source sketch is missing"))?;
                for id in curves {
                    let points = if let Some(ci) = solved.sketch.curve_index(id) {
                        solved.sketch.polyline(ci).into_iter().map(|p| solved.plane.to_world(p)).collect::<Vec<_>>()
                    } else if let Some(wi) = solved.sketch.wire_index(id) {
                        let wire = solved.sketch.wires.get(wi).ok_or_else(|| invalid("the pipe source wire is missing"))?;
                        if wire.pts.len() > 32_768 - points_used {
                            return Err(invalid("pipe path sampling budget exceeded"));
                        }
                        wire.pts.clone()
                    } else {
                        return Err(invalid(format!("pipe source curve `{id}` is missing")));
                    };
                    points_used += points.len();
                    if points_used > 32_768 {
                        return Err(invalid("pipe path sampling budget exceeded"));
                    }
                    source.push(PathEdge { key: format!("s{}:{id}", sketch.0), points });
                }
            }
            Self::Edges { body, edges } => {
                let body = state.body(body).ok_or_else(|| invalid("the pipe source body is missing"))?;
                let mut used = BTreeSet::new();
                for named in edges {
                    let hit = crate::naming::locate_edge(body, &named.name, named.pick).map_err(invalid)?;
                    if hit.split {
                        return Err(invalid("a pipe source edge split; select the desired path edges again"));
                    }
                    if !used.insert(hit.edge) {
                        return Err(invalid("pipe path references resolve to the same edge"));
                    }
                    let curve = kernel::edge_geometry(&body.body, &[hit.edge])?
                        .into_iter()
                        .next()
                        .ok_or_else(|| invalid("the pipe edge has no exact curve"))?;
                    let points = sampling::curve(&curve, 32_768 - points_used)?;
                    points_used += points.len();
                    source.push(PathEdge { key: named.name.clone(), points });
                }
            }
        }
        Network::new(source).map_err(|e| invalid(e.to_string()))
    }
    pub(crate) fn remap_names(&mut self, features: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>) {
        if let Self::Edges { edges, .. } = self {
            for edge in edges {
                edge.name = crate::remap_topology(&edge.name, features, bodies);
            }
        }
    }
}
impl crate::walk::Walk for PipeSource {
    fn walk_refs<'a>(&'a self, f: &mut dyn FnMut(crate::walk::RefItem<'a>)) {
        match self {
            Self::Sketch { sketch, .. } => f(crate::walk::Ref::Sketch(sketch)),
            Self::Edges { body, .. } => f(crate::walk::Ref::Body(body)),
        }
    }
    fn walk_refs_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::RefItemMut<'a>)) {
        match self {
            Self::Sketch { sketch, .. } => f(crate::walk::Ref::Sketch(sketch)),
            Self::Edges { body, .. } => f(crate::walk::Ref::Body(body)),
        }
    }
    fn walk_geometry<'a>(&'a self, f: &mut dyn FnMut(crate::walk::GeoItem<'a>)) {
        if let Self::Edges { edges, .. } = self {
            for edge in edges {
                f(crate::walk::Geo::Pos(&edge.pick));
            }
        }
    }
    fn walk_geometry_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::GeoItemMut<'a>)) {
        if let Self::Edges { edges, .. } = self {
            for edge in edges {
                f(crate::walk::Geo::Pos(&mut edge.pick));
            }
        }
    }
}
