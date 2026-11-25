//! A ruled wall derived from an active trim, separate from the subdivision evaluator.
use super::*;
use crate::bspline::NurbsSurface;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrimWall {
    attachment: TrimAttachment,
    pub(super) delta: Vec3,
}

impl TrimWall {
    /// Saved directed source trim, revalidated by Hierarchy::validated_trim_wall.
    pub fn attachment(&self) -> &TrimAttachment {
        &self.attachment
    }

    pub fn root(&self) -> usize {
        self.attachment.root
    }

    pub fn ruled_patch(&self, skin: &NurbsSurface) -> Result<NurbsSurface> {
        self.attachment.ruled_patch(skin, self.delta)
    }

    fn validate_delta(&self) -> Result<()> {
        solidcraft_kernel_services::form_subdivision::validate_trim_wall_delta(self.delta).map_err(|error| bad(&error.to_string()))
    }

    pub(super) fn append_preview(&self, surface: &mut SurfacePreview, logical: &mut Cage) -> Result<()> {
        let [a, b] = self.attachment.edge;
        let (edge, path) =
            surface.boundaries.iter().find(|(edge, _)| key(edge[0], edge[1]) == key(a, b)).ok_or_else(|| bad("missing trim preview path"))?;
        let mut base = path.clone();
        if edge[0] != a {
            base.reverse();
        }
        let start = *base.first().ok_or_else(|| bad("empty trim preview path"))?;
        let end = *base.last().ok_or_else(|| bad("empty trim preview path"))?;
        if base.len() < 2 || surface.vertices.len() != logical.points.len() {
            return Err(bad("trim preview control slots disagree with the active skin"));
        }
        let geometry = solidcraft_kernel_services::form_subdivision::trim_wall(&base, [logical.point(a)?, logical.point(b)?], self.delta)
            .map_err(|error| bad(&error.to_string()))?;
        let solidcraft_kernel_services::form_subdivision::TrimWallGeometry { top, normals, controls } = geometry;
        let top_start = *top.first().ok_or_else(|| bad("empty trim preview path"))?;
        let top_end = *top.last().ok_or_else(|| bad("empty trim preview path"))?;

        let first = logical.points.len();
        let owner = logical.faces.len() as u32;
        let mesh_start = u32::try_from(surface.mesh.positions.len()).map_err(|_| bad("trim preview mesh exceeds its index budget"))?;
        for (i, ((&p, &q), normal)) in base.iter().zip(&top).rev().zip(normals).enumerate() {
            surface.mesh.positions.extend([p, q]);
            surface.mesh.normals.extend([normal; 2]);
            if i > 0 {
                let edge = mesh_start + 2 * (i as u32 - 1);
                surface.mesh.push_triangle([edge, edge + 2, edge + 3], owner);
                surface.mesh.push_triangle([edge, edge + 3, edge + 1], owner);
            }
        }
        let cap = [first, first + 1];
        let [c, d] = cap;
        logical.points.extend(controls);
        logical.faces.push(vec![b, a, c, d]);
        surface.vertices.extend([top_start, top_end]);
        surface.boundaries.extend([(cap, top), ([a, c], vec![start, top_start]), ([b, d], vec![end, top_end])]);
        Ok(())
    }
}

impl Hierarchy {
    pub fn has_trim_wall(&self) -> bool {
        self.trim_wall.is_some()
    }

    /// Re-resolve saved geometry before using the attachment in preview or CAD.
    pub fn validated_trim_wall(&self) -> Result<Option<&TrimWall>> {
        let Some(wall) = &self.trim_wall else { return Ok(None) };
        wall.validate_delta()?;
        let current = self.trim_attachments(&[wall.attachment.edge])?;
        if current.first() != Some(&wall.attachment) {
            return Err(bad("saved trim wall does not match its active chart boundary"));
        }
        Ok(Some(wall))
    }

    pub(super) fn check_trim_wall_topology(&self) -> Result<()> {
        if self.has_trim_wall() {
            return Err(bad("remove the derived trim wall before changing its source topology"));
        }
        Ok(())
    }

    /// Extend ordinary evaluator edges or a single trimmed edge.
    /// Both routes return the translated logical rim for the gesture selection.
    pub fn extend_edges(&self, edges: &[[usize; 2]], delta: Vec3) -> Result<(Self, Vec<[usize; 2]>)> {
        if self.has_quad_cap() {
            return Err(bad("edge extrusion on a capped hierarchy is not supported yet"));
        }
        self.check_trim_wall_topology()?;
        let ordinary = match self.extrude_edges(edges, delta) {
            Ok(extruded) => return Ok(extruded),
            Err(error) => error,
        };
        if edges.len() != 1 {
            return Err(ordinary);
        }
        let Ok(attachments) = self.trim_attachments(edges) else { return Err(ordinary) };
        let attachment = attachments.into_iter().next().ok_or_else(|| bad("missing trim attachment"))?;
        let wall = TrimWall { attachment, delta };
        wall.validate_delta()?;
        let first = self.evaluate()?.points.len();
        let mut out = self.clone();
        out.trim_wall = Some(wall);
        Ok((out, vec![[first, first + 1]]))
    }

    /// Translate the derived rim while its attachment and evaluator stay fixed.
    pub fn move_trim_rim(&self, delta: Vec3) -> Result<Self> {
        let mut out = self.clone();
        let wall = out.trim_wall.as_mut().ok_or_else(|| bad("missing derived trim wall"))?;
        wall.delta = solidcraft_kernel_services::form_subdivision::move_trim_wall(wall.delta, delta).map_err(|error| bad(&error.to_string()))?;
        Ok(out)
    }
}
