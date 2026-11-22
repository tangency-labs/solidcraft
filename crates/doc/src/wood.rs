//! Manufacturing grain is independent of a body's visual material or texture mapping.
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::{DocError, Document, Mat, ModelBody, Result};
use solidcraft_kernel as kernel;

/// An explicit manufacturing direction in the body's construction coordinates. `None`
/// deliberately disables grain, including a frame member's default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grain {
    pub direction: Option<[f64; 3]>,
}

/// Evaluated placement of the construction coordinates; rebuilt from the timeline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrainFrame {
    pub matrix: Mat,
    /// Copy sources, nearest first. Each uses the same construction coordinates.
    pub sources: Vec<String>,
    pub member_axis: Option<Vec3>,
}

impl Default for GrainFrame {
    fn default() -> Self {
        Self { matrix: crate::IDENTITY, sources: Vec::new(), member_axis: None }
    }
}

impl GrainFrame {
    pub(crate) fn moved(body: &ModelBody, matrix: &Mat, copy: bool) -> Self {
        let mut frame = body.grain_frame.as_deref().cloned().unwrap_or_default();
        frame.matrix = crate::mat_mul(matrix, &frame.matrix);
        if copy {
            frame.sources.insert(0, body.definition_name().to_string());
        }
        frame
    }
}

impl Document {
    /// Unit grain direction in the coordinates of `body`, or no grain. A malformed saved
    /// direction is an error rather than an instruction to infer grain from the stock size.
    pub fn grain_direction(&self, body: &ModelBody) -> Result<Option<Vec3>> {
        let frame = body.grain_frame.as_deref().cloned().unwrap_or_default();
        let saved = self.wood_grain.get(body.definition_name()).or_else(|| frame.sources.iter().find_map(|name| self.wood_grain.get(name)));
        let direction = match saved {
            Some(grain) => grain.direction.map(Vec3::from),
            None => frame.member_axis,
        };
        direction
            .map(|v| {
                let v = crate::apply_vector(&frame.matrix, v);
                if ![v.x, v.y, v.z].iter().all(|x| x.is_finite()) {
                    return Err(DocError::Invalid("grain direction must be finite".into()));
                }
                v.normalized().ok_or_else(|| DocError::Invalid("grain direction must have nonzero length".into()))
            })
            .transpose()
    }
}

/// Exact bounds with `direction` as the X axis. No tessellation participates in the measure.
pub fn along_grain(body: &kernel::Body, direction: Vec3) -> Result<f64> {
    let x = unit(direction)?;
    let y = x.any_perp();
    let z = x.cross(y);
    let matrix = [[x.x, y.x, z.x, 0.0], [x.y, y.y, z.y, 0.0], [x.z, y.z, z.z, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let bounds = kernel::transform_matrix(body, matrix)?.exact_bounds()?;
    Ok(bounds.max.x - bounds.min.x)
}

/// A straight B-rep edge's direction; a curved edge has no single grain direction.
pub fn edge_direction(body: &kernel::Body, index: usize, face: Option<usize>) -> Result<Vec3> {
    let edge = body.record().edges.get(index).ok_or_else(|| DocError::Invalid("the edge is not on this body".into()))?;
    if let Some(face) = face {
        let plane = body.planar_face(face).ok_or_else(|| DocError::Invalid("choose a planar face".into()))?;
        if !edge.uses.iter().any(|u| u.face as usize == face) {
            return Err(DocError::Invalid("the edge is not on the selected face".into()));
        }
        let direction = edge_direction(body, index, None)?;
        if plane.normal().dot(direction).abs() > 1e-8 {
            return Err(DocError::Invalid("the edge must run in the face's plane".into()));
        }
        return Ok(direction);
    }
    let solidcore_record::brep::Curve::Line { start, end } = edge.curve else {
        return Err(DocError::Invalid("choose a straight edge".into()));
    };
    unit(Vec3::from(end) - Vec3::from(start))
}

pub fn unit(direction: Vec3) -> Result<Vec3> {
    if ![direction.x, direction.y, direction.z].iter().all(|v| v.is_finite()) {
        return Err(DocError::Invalid("grain direction must be finite".into()));
    }
    direction.normalized().ok_or_else(|| DocError::Invalid("grain direction must have nonzero length".into()))
}
