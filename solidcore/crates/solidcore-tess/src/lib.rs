//! # solidcore-tess
//!
//! Geometry made drawable (docs/layers.md §1): the one mesher of solidcore and the applications
//! built on it. Each face of a B-rep record ([`solidcore_record::brep::Brep`]) is triangulated in
//! its own (u, v), every edge sampled once so neighbouring faces share their boundary points
//! exactly, and handed over as a [`TessPlan`]: the parameters and triangles of every face with
//! the surfaces to evaluate them on, in the kernel evaluator's form ([`solidcore::eval`]). The
//! CPU evaluates a plan into a [`Mesh`] ([`TessPlan::evaluate`]); a GPU evaluates the same plan
//! in a compute shader.
//!
//! - [`plan`]: the face planner and its display entry point, at a [`MeshTolerance`];
//! - [`mesh()`]: the mesh path, not for display: every face by constrained Delaunay with lattice
//!   seeds and Ruppert shaping to a [`MeshSpec`], for mesh bodies, mesh export and analysis
//!   meshers, slow by design; [`TessPlan::welded`] joins its faces at their shared vertices;
//! - [`TessPlan`], [`FacePlan`]: the plan, and [`TessPlan::transformed`] for a placed copy;
//! - [`Mesh`]: an indexed triangle mesh with its edge polylines and their display flags;
//! - [`curve_points`]: a free curve's points at a [`MeshTolerance`], by the edge rules;
//! - [`simplify`]: display levels of detail of a large mesh;
//! - [`preview`]: a bounded coarse mesh of a decoded body.
//!
//! A display mesh decides nothing a model depends on: it is drawn, never measured.
//!
//! Library code never panics, unwraps or indexes on input-derived data: a face the planner
//! cannot mesh is named in its [`PlanStats`], and the rest of the body is meshed.

mod curve;
#[cfg(test)]
mod fixtures;
mod hash;
mod iso;
mod mesh;
// SolidCraft's mesher, moved here (RFC 0003, item 1.1) and kept in SolidCraft's formatting so it
// reads the same as its history.
#[rustfmt::skip]
mod mesher;
mod par;
mod plan;
mod pole;
pub mod preview;
mod ring;
pub mod simplify;
mod spec;
mod stats;
mod tolerance;
mod vec;

pub use curve::curve_points;
pub use iso::{MAX_ISOPARMS, isoparms};
pub use mesh::Mesh;
pub use mesher::{MAX_ANGLE_BOUNDARY, plan};
pub use par::guarded;
pub use plan::{FacePlan, TessPlan};
pub use pole::{AveragedPole, PoleNode, PoleRule};
pub use spec::{MAX_MESH_MIN_ANGLE, MESH_MIN_ANGLE, MeshSpec, WeldedMesh, mesh};
pub use stats::{
    Cap, CappedEdge, EdgeCap, OverTolerance, PlanStats, RefusedEdge, RefusedFace, ShapeLimit,
    Unshaped,
};
pub use tolerance::{DISPLAY_ANGLE, MeshTolerance};

/// The plan evaluator's curve and surface forms a [`TessPlan`] holds (its `surfaces`), so a
/// client that builds or reads a plan names them through this crate, not the kernel's.
pub use solidcore::eval::plan::{EvalCurve, EvalSurface};
/// The vector conversions between the record and the evaluator that plan surfaces are built with.
pub use solidcore_convert::{record_vec3, vec3};

/// Why a body could not be planned. The messages are the ones the mesher has always reported.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TessError {
    /// The record is malformed where the mesher reads it.
    #[error("invalid input: {0}")]
    Invalid(String),
    /// The mesher could not plan it (a face, an edge, a kind not supported yet).
    #[error("the operation failed: {0}")]
    Failed(String),
    /// A panic inside the mesher, caught there.
    #[error("kernel internal error: {0}")]
    Internal(String),
    /// A mesh with more vertices than a `u32` index names ([`TessPlan::welded`]): `vertices`
    /// is the count it had reached when the next one could not be numbered, past `u32::MAX`.
    #[error(
        "the mesh has more than {} vertices, more than a 32-bit index can name (reached {vertices})",
        u32::MAX
    )]
    TooManyVertices {
        /// The vertices numbered or asked for so far (more than `u32::MAX`).
        vertices: usize,
    },
}

/// The mesher's result.
pub type Result<T> = std::result::Result<T, TessError>;

/// A planar fill for display: its vertices and its counter-clockwise triangles.
pub type PlanarFill = (Vec<[f64; 2]>, Vec<[u32; 3]>);

/// Triangulate closed planar boundary loops for display, preserving their holes.
/// The first loop is the outer boundary; remaining loops are holes. The returned
/// vertices are in the same coordinates as the input.
/// # Errors
/// A boundary with fewer than three finite points, more points than the limit, or loops the
/// triangulator cannot fill.
pub fn triangulate_planar(loops: &[Vec<[f64; 2]>]) -> Result<PlanarFill> {
    par::guard("planar triangulation", || {
        if loops.is_empty() {
            return Err(TessError::Invalid(
                "a planar region needs an outer boundary".into(),
            ));
        }
        let mut points = Vec::new();
        let mut ranges = Vec::new();
        for boundary in loops {
            let boundary = if boundary.first() == boundary.last() && boundary.len() > 1 {
                boundary
                    .get(..boundary.len() - 1)
                    .ok_or_else(|| TessError::Invalid("invalid planar boundary".into()))?
            } else {
                boundary.as_slice()
            };
            if boundary.len() < 3 || boundary.iter().flatten().any(|value| !value.is_finite()) {
                return Err(TessError::Invalid(
                    "a planar boundary needs at least three finite points".into(),
                ));
            }
            let start = points.len();
            if boundary.len() > 500_000 - start {
                return Err(TessError::Invalid(
                    "planar boundary exceeds the point limit".into(),
                ));
            }
            points.extend_from_slice(boundary);
            ranges.push(start..points.len());
        }
        let fill = mesher::cdt::Fill {
            seeds: &[],
            refine: None,
            shape: false,
            lenient: false,
            flat: None,
        };
        let (extra, triangles) = mesher::cdt::triangulate_clipped(&points, &ranges, fill)
            .ok_or_else(|| {
                TessError::Invalid("planar boundaries could not be triangulated".into())
            })?;
        points.extend(extra);
        Ok((points, triangles))
    })
}

/// Triangulate the union of planar regions for display, preserving their holes. Each region is
/// its boundary loops (the largest is taken as its outline, the rest as its holes); regions that
/// overlap or abut come back as one surface, with no seam where they meet. The returned vertices
/// are in the same coordinates as the input.
/// # Errors
/// A region without an outline, a boundary with fewer than three finite points, more points than
/// the limit, or loops the triangulator cannot fill.
pub fn triangulate_union(regions: &[Vec<Vec<[f64; 2]>>]) -> Result<PlanarFill> {
    par::guard("planar union", || {
        if regions.is_empty() {
            return Err(TessError::Invalid(
                "a planar union needs at least one region".into(),
            ));
        }
        let mut points: Vec<[f64; 2]> = Vec::new();
        let mut loops: Vec<std::ops::Range<usize>> = Vec::new();
        let mut region_of: Vec<usize> = Vec::new();
        for (index, region) in regions.iter().enumerate() {
            let mut named = 0usize;
            for boundary in region {
                let boundary = if boundary.first() == boundary.last() && boundary.len() > 1 {
                    boundary
                        .get(..boundary.len() - 1)
                        .ok_or_else(|| TessError::Invalid("invalid planar boundary".into()))?
                } else {
                    boundary.as_slice()
                };
                if boundary.len() < 3 || boundary.iter().flatten().any(|value| !value.is_finite()) {
                    return Err(TessError::Invalid(
                        "a planar boundary needs at least three finite points".into(),
                    ));
                }
                let start = points.len();
                if boundary.len() > 500_000 - start {
                    return Err(TessError::Invalid(
                        "planar boundary exceeds the point limit".into(),
                    ));
                }
                points.extend_from_slice(boundary);
                loops.push(start..points.len());
                region_of.push(index);
                named += 1;
            }
            if named == 0 {
                return Err(TessError::Invalid(
                    "a planar region needs an outline".into(),
                ));
            }
        }
        let (extra, triangles) = mesher::cdt::triangulate_union(&points, &loops, &region_of)
            .ok_or_else(|| {
                TessError::Invalid("planar boundaries could not be triangulated".into())
            })?;
        points.extend(extra);
        Ok((points, triangles))
    })
}

/// Why the kernel's evaluator refused a curve or surface, in the words the mesher reports.
pub(crate) fn eval_message(e: solidcore::eval::EvalError) -> String {
    match e {
        solidcore::eval::EvalError::Unsupported(what) => format!("not supported yet: {what}"),
        solidcore::eval::EvalError::Invalid(what)
        | solidcore::eval::EvalError::TooLarge(what)
        | solidcore::eval::EvalError::Unevaluable(what)
        | solidcore::eval::EvalError::Degenerate(what)
        | solidcore::eval::EvalError::OffRange(what) => what,
    }
}

/// An empty `Vec` with room for `n` items, or `None` where that much cannot be reserved (where
/// `Vec::with_capacity` would panic).
pub(crate) fn with_room<T>(n: usize) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve(n).ok()?;
    Some(v)
}

#[cfg(test)]
mod reexport_tests {
    use solidcore_record::Vec3;

    #[test]
    fn planar_region_triangulation_keeps_holes() {
        let loops = vec![
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]],
        ];
        let (points, triangles) = crate::triangulate_planar(&loops).unwrap();
        let area: f64 = triangles
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|index| points[index as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) * 0.5
            })
            .sum();
        assert!((area - 84.0).abs() < 1e-9, "{area}");
    }

    #[test]
    fn planar_region_triangulation_clips_crossing_and_overlapping_holes() {
        let loops = vec![
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            vec![[8.0, 2.0], [12.0, 2.0], [12.0, 6.0], [8.0, 6.0]],
            vec![[9.0, 4.0], [11.0, 4.0], [11.0, 8.0], [9.0, 8.0]],
        ];
        let (points, triangles) = crate::triangulate_planar(&loops).unwrap();
        let area: f64 = triangles
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|index| points[index as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) * 0.5
            })
            .sum();
        assert!((area - 90.0).abs() < 1e-9, "{area}");
    }

    /// A plan's surfaces, read through this crate's names alone.
    fn surfaces(plan: &crate::TessPlan) -> &[crate::EvalSurface] {
        &plan.surfaces
    }

    #[test]
    fn a_plans_surfaces_are_built_and_read_through_this_crate() {
        let plane = crate::EvalSurface::Plane {
            origin: crate::vec3(Vec3::new(1.0, 2.0, 3.0)),
            du: crate::vec3(Vec3::new(1.0, 0.0, 0.0)),
            dv: crate::vec3(Vec3::new(0.0, 1.0, 0.0)),
        };
        let crate::EvalSurface::Plane { origin, .. } = &plane else {
            panic!("{plane:?}")
        };
        assert_eq!(crate::record_vec3(*origin), Vec3::new(1.0, 2.0, 3.0));
        let plan = crate::TessPlan {
            surfaces: vec![plane.clone()],
            ..Default::default()
        };
        assert_eq!(surfaces(&plan), [plane]);
    }
}
