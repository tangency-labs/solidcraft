//! The mesh path: a body meshed as a mesh in its own right, for mesh bodies, mesh export (STL,
//! OBJ, 3MF, PLY) and analysis meshers, never for display. Every face goes through constrained
//! Delaunay with an equilateral lattice of seeds, refinement for sag and side length, and
//! Ruppert shaping; it is slow by design.

use solidcore_record::{Vec3, brep};

use crate::hash::BitsMap;
use crate::mesher::{Purpose, Shape, Sides, plan_at};
use crate::{MeshTolerance, PlanStats, RefusedFace, Result, TessError, TessPlan};

/// The smallest angle the mesh path aims for unless asked otherwise (degrees).
pub const MESH_MIN_ANGLE: f64 = 25.0;

/// The largest smallest angle the mesh path takes (degrees): the bound of the Ruppert refinement
/// its 3D shaping runs (Delaunay refinement on the surface, a point at the 3D circumcentre of
/// each triangle sharper than the angle). Asked for more than about 30°, that refinement may not
/// end: each point it adds can make new triangles sharper than the angle. spade only makes the
/// first triangulation in (u, v), which the shaping then refines; this bound is the shaping's.
pub const MAX_MESH_MIN_ANGLE: f64 = 30.0;

/// How the mesh path ([`mesh()`]) meshes a body. Plain data: a size that varies from point to
/// point is passed to [`mesh()`] beside it, as a function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshSpec {
    /// The farthest a facet or an edge segment may stand off the surface or curve it stands for
    /// (mm), as [`MeshTolerance::chord`]: never finer than a millionth of the body.
    pub chord: f64,
    /// The longest triangle side asked (mm). `None`: a fifth of the body's extent (twice the
    /// largest distance of a vertex from the vertices' centroid), the longest side the display
    /// allows. A curved face takes less where its curvature asks for less.
    pub size: Option<f64>,
    /// The smallest angle Ruppert refinement aims for (degrees, more than 0 and at most
    /// [`MAX_MESH_MIN_ANGLE`]). A corner of the boundary sharper than it keeps its angle.
    pub min_angle: f64,
}

impl MeshSpec {
    /// `chord`, no size asked and [`MESH_MIN_ANGLE`].
    #[must_use]
    pub const fn chord(chord: f64) -> Self {
        MeshSpec {
            chord,
            size: None,
            min_angle: MESH_MIN_ANGLE,
        }
    }
}

/// The mesh of `b`, a body of size `size` (its box diagonal, as [`crate::plan`] takes it), to
/// `spec`, as a plan the CPU evaluates ([`TessPlan::evaluate`] for a mesh with each face's own
/// normals, [`TessPlan::welded`] for one whose faces share their vertices).
///
/// **Not for display**: for mesh bodies, mesh export (STL, OBJ, 3MF, PLY) and analysis meshers.
/// Every face, planar ones included, goes through constrained Delaunay in its own (u, v): seeds
/// of an equilateral lattice a side apart in 3D, refined where a triangle sags off the surface
/// by more than `spec.chord` or has a side longer than 1.5 times the side asked there, then
/// shaped by its triangles in 3D (Delaunay refinement on the surface: flips by the 3D angles,
/// and a point at the 3D circumcentre of each triangle sharper than `spec.min_angle`, longer
/// than asked or off the surface). It is slow by design; [`crate::plan`] is the display's.
///
/// A face's side is `spec.size` where its curvature asks for no less, and less where
/// `size_at` asks for less at a point (a value that is not finite and positive asks for nothing
/// there). Every edge is sampled once, its segments no longer than the side of either face it
/// bounds or than `size_at` asks at their middles, and both faces take its nodes, so the mesh
/// is closed wherever the B-rep is. Where shaping would add a point that encroaches on a
/// boundary side (Ruppert's rule), the side's edge takes a node at its middle instead, and every
/// face of the edge is planned again with it, until no face asks for one.
///
/// What the plan's stats name: [`PlanStats::unshaped`], each face left short of its shape (a
/// triangle sharper than `spec.min_angle`, or a bound that stopped the shaping: its points, the
/// rounds of splitting sides, a side on no edge kept whole), with its smallest angle in
/// 3D; [`PlanStats::over_tolerance`], each face whose sag refinement stopped at its bound, with
/// whether its triangles are still within `spec.chord` (`within`), as on the display path; and
/// [`PlanStats::edges_capped`], each edge a bound of its sampling stopped. Faces are planned in
/// parallel where there are several, each on its own, and collected in the record's order: the
/// plan is the same for any number of threads.
///
/// # Errors
/// [`TessError::Invalid`] for a `spec` out of range (a chord or size that is not a positive
/// length, an angle outside (0, [`MAX_MESH_MIN_ANGLE`]]). [`TessError::Failed`] naming the first
/// face that could not be meshed and why: nothing is filled leniently and no face is left out.
pub fn mesh(
    b: &brep::Brep,
    spec: &MeshSpec,
    size: f64,
    size_at: Option<&(dyn Fn(Vec3) -> f64 + Sync)>,
) -> Result<(TessPlan, PlanStats)> {
    let positive = |x: f64| x.is_finite() && x > 0.0;
    if !positive(spec.chord) {
        return Err(TessError::Invalid(format!(
            "mesh: a chord of {} mm",
            spec.chord
        )));
    }
    if let Some(side) = spec.size
        && !positive(side)
    {
        return Err(TessError::Invalid(format!("mesh: a size of {side} mm")));
    }
    let angle_in_range = spec.min_angle > 0.0 && spec.min_angle <= MAX_MESH_MIN_ANGLE;
    if !angle_in_range {
        return Err(TessError::Invalid(format!(
            "mesh: a smallest angle of {} degrees, outside (0, {MAX_MESH_MIN_ANGLE}]",
            spec.min_angle
        )));
    }
    let (tol, _) = MeshTolerance::chord(spec.chord).rules(size);
    let field = size_at.map(|f| move |p: solidcore::Vec3| f(solidcore_convert::record_vec3(p)));
    let shape = Shape {
        min_angle: spec.min_angle,
        side: spec.size,
        sides: Sides::Split,
        size_at: field
            .as_ref()
            .map(|f| f as &(dyn Fn(solidcore::Vec3) -> f64 + Sync)),
    };
    let (plan, stats) = plan_at(b, tol, None, Purpose::Mesh(shape))?;
    if let Some(left_out) = stats.unmeshed.first() {
        return Err(TessError::Failed(format!(
            "mesh: face {}: {}",
            left_out.face, left_out.reason
        )));
    }
    Ok((plan, stats))
}

/// A plan's mesh with its faces joined ([`TessPlan::welded`]): one vertex for each point the
/// faces share.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeldedMesh {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Triangles as vertex index triples (counter-clockwise seen from outside).
    pub triangles: Vec<[u32; 3]>,
    /// The B-rep face index of each triangle.
    pub tri_face: Vec<u32>,
    /// Each B-rep edge's nodes as vertex indices, in the record's edge order.
    pub edges: Vec<Vec<u32>>,
}

impl TessPlan {
    /// The mesh with its faces joined, for formats and meshers that need the vertices shared:
    /// each face's boundary node (a shared edge node, at its exact position, bit for bit the
    /// same in every face that runs the edge) is one vertex however many faces run it, numbered
    /// where the first face in the record's order meets it, and every other point is its face's
    /// own. No normals: a face's are [`TessPlan::evaluate`]'s. Returns the mesh and the faces
    /// left out of it, each with why (the surface does not evaluate at one of its nodes), as
    /// [`TessPlan::evaluate`] leaves them out.
    ///
    /// # Errors
    /// [`TessError::TooManyVertices`] for a mesh with more vertices than a `u32` index names:
    /// none is numbered past `u32::MAX`, so no two vertices share an index.
    pub fn welded(&self) -> Result<(WeldedMesh, Vec<RefusedFace>)> {
        let mut out = WeldedMesh::default();
        let mut refused = Vec::new();
        // The shared nodes so far, by position bit for bit (-0.0 as 0.0). Only looked up: the
        // numbering follows the faces' order.
        let mut shared: BitsMap<[u64; 3], u32> = BitsMap::default();
        let key = |p: Vec3| [p.x, p.y, p.z].map(|x| (x + 0.0).to_bits());
        let push = |positions: &mut Vec<Vec3>, p: Vec3| -> Result<u32> {
            let i = vertex_index(positions.len())?;
            positions.push(p);
            Ok(i)
        };
        let shared_id = |shared: &mut BitsMap<[u64; 3], u32>,
                         positions: &mut Vec<Vec3>,
                         p: Vec3|
         -> Result<u32> {
            if let Some(id) = shared.get(&key(p)) {
                return Ok(*id);
            }
            let id = push(positions, p)?;
            shared.insert(key(p), id);
            Ok(id)
        };
        for f in &self.faces {
            let nodes = self.nodes(f);
            if let Some(reason) = nodes.failure {
                refused.push(RefusedFace {
                    face: f.face as usize,
                    reason,
                });
                continue;
            }
            let mut ids: Vec<u32> = Vec::with_capacity(nodes.positions.len());
            for (k, p) in nodes.positions.iter().enumerate() {
                let id = if k < f.fixed.len() {
                    shared_id(&mut shared, &mut out.positions, *p)?
                } else {
                    push(&mut out.positions, *p)?
                };
                ids.push(id);
            }
            for t in &f.triangles {
                let corner = |i: u32| ids.get(i as usize).copied();
                if let (Some(a), Some(b), Some(c)) = (corner(t[0]), corner(t[1]), corner(t[2])) {
                    out.triangles.push([a, b, c]);
                    out.tri_face.push(f.face);
                }
            }
        }
        for e in &self.edges {
            let line = e
                .iter()
                .map(|p| shared_id(&mut shared, &mut out.positions, *p))
                .collect::<Result<Vec<u32>>>()?;
            out.edges.push(line);
        }
        Ok((out, refused))
    }
}

/// The index of the vertex after `numbered` others, or [`TessError::TooManyVertices`] where a
/// `u32` cannot name it.
fn vertex_index(numbered: usize) -> Result<u32> {
    u32::try_from(numbered).map_err(|_| TessError::TooManyVertices {
        vertices: numbered.saturating_add(1),
    })
}

#[cfg(test)]
mod welded_tests {
    use super::*;

    /// The vertex past `u32::MAX` is an error naming the count, not a second vertex numbered
    /// `u32::MAX` (as the index was capped before).
    #[test]
    fn a_vertex_past_u32_max_is_an_error_not_a_capped_index() {
        assert_eq!(vertex_index(0), Ok(0));
        assert_eq!(vertex_index(u32::MAX as usize), Ok(u32::MAX));
        let past = u32::MAX as usize + 1;
        assert_eq!(
            vertex_index(past),
            Err(TessError::TooManyVertices { vertices: past + 1 })
        );
        let message = vertex_index(past).unwrap_err().to_string();
        assert!(message.contains(&u32::MAX.to_string()), "{message}");
    }
}
