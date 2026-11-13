//! Derived exact logical routes, independent of display samples and triangle IDs.
use super::*;
use crate::kernel;
use solidcore_record::Vec3;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::subdivision::fitting::{EdgeChart, PatchChart};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct NativeGuide {
    pub edge: [usize; 2],
    /// Parameter-line paths on these exact native faces, with intrinsic native
    /// edge stations at rims and chart transitions. No sampled world points.
    pub pieces: Vec<kernel::DisplayGuide>,
}
#[derive(Clone, Debug)]
pub struct NativeGuides {
    /// May be a display partition of the edited skin. This is not a context body.
    pub body: kernel::Body,
    pub edges: Vec<NativeGuide>,
}
#[derive(Clone, Debug)]
pub enum SelectionGuides {
    Subdivision(Vec<EdgeCurve>),
    Native(NativeGuides),
    /// An ordinary display fallback is permitted; an exact query must refuse it.
    Unmapped(String),
}
impl SelectionGuides {
    /// Exact logical endpoint positions; disagreement refuses the whole point
    /// contract instead of averaging adjacent charts or choosing nearby nodes.
    pub fn points(&self) -> Result<BTreeMap<usize, Vec3>> {
        let mut points = BTreeMap::new();
        let mut insert = |id, point: Vec3| -> Result<()> {
            if !point.is_finite() || points.get(&id).is_some_and(|previous: &Vec3| previous.dist(point) > 1e-7) {
                return Err(DocError::Invalid("exact logical guide endpoints disagree".into()));
            }
            points.entry(id).or_insert(point);
            Ok(())
        };
        match self {
            Self::Subdivision(curves) => {
                for curve in curves {
                    insert(curve.edge[0], curve.at(0.0)?)?;
                    insert(curve.edge[1], curve.at(1.0)?)?;
                }
            }
            Self::Native(native) => {
                for edge in &native.edges {
                    let first = edge.pieces.first().ok_or_else(|| DocError::Invalid("missing native logical guide start".into()))?;
                    let last = edge.pieces.last().ok_or_else(|| DocError::Invalid("missing native logical guide end".into()))?;
                    insert(edge.edge[0], station(&native.body, first, true)?)?;
                    insert(edge.edge[1], station(&native.body, last, false)?)?;
                }
            }
            Self::Unmapped(reason) => return Err(DocError::Invalid(reason.clone())),
        }
        Ok(points)
    }
    pub fn relocated(&self, old: &Placement, new: &Placement) -> Result<Self> {
        old.validate()?;
        new.validate()?;
        Ok(match self {
            Self::Subdivision(curves) => Self::Subdivision(curves.iter().map(|curve| curve.with_placement(new)).collect::<Result<_>>()?),
            Self::Native(native) => {
                let inverse = crate::mat_inverse(&old.matrix()).ok_or_else(|| DocError::Invalid("invalid native guide placement".into()))?;
                let matrix = crate::mat_mul(&new.matrix(), &inverse);
                if solidcraft_geom::brep::placement::Rigid::of_matrix(&matrix).is_none() {
                    return Err(DocError::Invalid("exact native guide placement requires the topology-preserving rigid path".into()));
                }
                let body = kernel::transform_matrix(&native.body, matrix)?;
                if body.face_count() != native.body.face_count() || body.edge_count() != native.body.edge_count() {
                    return Err(DocError::Invalid("rigid native guide placement lost topology identities".into()));
                }
                Self::Native(NativeGuides { body, edges: native.edges.clone() })
            }
            Self::Unmapped(reason) => Self::Unmapped(reason.clone()),
        })
    }
}
fn station(body: &kernel::Body, piece: &kernel::DisplayGuide, first: bool) -> Result<Vec3> {
    let point = if first { piece.points.first() } else { piece.points.last() }.ok_or_else(|| DocError::Invalid("empty native guide piece".into()))?;
    match *point {
        kernel::DisplayGuidePoint::Surface(uv) => {
            Ok(solidcraft_geom::tess::from_kernel(kernel::face_geometry(body, piece.face as usize)?.surface.eval(uv[0], uv[1]).0))
        }
        kernel::DisplayGuidePoint::Edge { edge, parameter } => {
            let edge = body.record().edges.get(edge as usize).ok_or_else(|| DocError::Invalid("missing intrinsic native guide edge".into()))?;
            if !parameter.is_finite() || parameter < edge.interval[0] || parameter > edge.interval[1] {
                return Err(DocError::Invalid("native guide parameter escaped its edge interval".into()));
            }
            let vertex = if parameter == edge.interval[0] {
                Some(edge.vertices[0])
            } else if parameter == edge.interval[1] {
                Some(edge.vertices[1])
            } else {
                None
            };
            if let Some(vertex) = vertex {
                let vertex =
                    body.record().vertices.get(vertex as usize).ok_or_else(|| DocError::Invalid("missing intrinsic native guide vertex".into()))?;
                return Ok(Vec3::new(vertex.point[0], vertex.point[1], vertex.point[2]));
            }
            CurveEval::new(edge.parameters())
                .map_err(|error| DocError::Invalid(error.to_string()))?
                .point(parameter)
                .ok_or_else(|| DocError::Invalid("intrinsic native guide station does not evaluate".into()))
        }
    }
}
pub(super) fn from_source(definition: &Definition, source: FittedSource) -> SelectionGuides {
    let routes = || -> Result<Vec<EdgeCurve>> {
        let edges = match &definition.hierarchy {
            Some(hierarchy) => hierarchy.layout().map_err(|error| DocError::Invalid(error.to_string()))?.visible_edges().collect(),
            None => definition.cage.edges().map_err(|error| DocError::Invalid(error.to_string()))?,
        };
        let wall = definition
            .hierarchy
            .as_ref()
            .map(|hierarchy| hierarchy.validated_trim_wall())
            .transpose()
            .map_err(|error| DocError::Invalid(error.to_string()))?
            .flatten();
        let derived = if let Some(wall) = wall {
            let hierarchy = definition.hierarchy.as_ref().ok_or_else(|| DocError::Invalid("missing trim wall hierarchy".into()))?;
            let first = definition.cage.points.len();
            let [a, b] = wall.attachment().edge;
            let patch = Arc::new(source.fit.patches.last().ok_or_else(|| DocError::Invalid("missing exact ruled wall patch".into()))?.clone());
            // The ruled patch's base direction is b -> a; virtual c,d are the
            // translated a,b. These identities are saved provenance, not nearness.
            let charts =
                [([a, first], [[1.0, 0.0], [1.0, 1.0]]), ([b, first + 1], [[0.0, 0.0], [0.0, 1.0]]), ([first, first + 1], [[1.0, 1.0], [0.0, 1.0]])];
            let patches = vec![(PatchChart::whole(hierarchy.leaves.len()), patch)];
            charts
                .into_iter()
                .map(|(edge, uv)| {
                    edge_curve::assemble(
                        edge,
                        vec![EdgeChart { face: hierarchy.leaves.len(), uv, range: [0.0, 1.0] }],
                        &patches,
                        &definition.placement,
                    )
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let mut curves = edge_curve::from_source(definition, source, &edges)?;
        curves.extend(derived);
        Ok(curves)
    };
    match routes() {
        Ok(curves) => SelectionGuides::Subdivision(curves),
        Err(error) => SelectionGuides::Unmapped(format!("exact {} logical guides are unavailable: {error}", definition.parameters.kind)),
    }
}
