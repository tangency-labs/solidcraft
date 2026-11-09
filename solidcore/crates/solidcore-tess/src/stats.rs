//! What a plan reports about its faces and edges: how each face was meshed, and what was
//! refused, limited or left out.

/// How the faces of a plan were meshed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanStats {
    /// Planar faces meshed whole as a box or four-cornered layout, or by Delaunay.
    pub planar: usize,
    /// Faces meshed on a structured layout: a box or four-cornered grid, patches or a cut grid.
    pub structured: usize,
    /// Curved faces meshed by constrained Delaunay triangulation.
    pub delaunay: usize,
    /// Faces every method refused, filled leniently in (u, v) (see `Lenient`) so the display
    /// has no hole: each still a defect upstream, in its pcurves or seams.
    pub lenient: Vec<RefusedFace>,
    /// Faces even the lenient fill refused, faces whose surface does not evaluate at a point
    /// the planner needs (a node, a middle it checks, a node of a cut line), and faces the
    /// planner could not read (a surface the evaluator does not take, a pcurve off its edge, a
    /// loop too short to bound anything, an edge that cannot be sampled): left out of the plan
    /// whole (no triangles), so the rest of the body is still meshed, each with why it could not
    /// be planned. Nothing is substituted for a point the surface refuses.
    pub unmeshed: Vec<RefusedFace>,
    /// Edges that bound no face and could not be sampled: left out of the plan (an empty
    /// polyline), so the rest of the body is still meshed, each with why.
    pub unsampled_edges: Vec<RefusedEdge>,
    /// Edges whose sampling at a coarser facet angle, for a face crowded with segments, failed:
    /// each keeps its nodes at the rules' own level (valid, only finer), no face is refused, and
    /// the edge is named here with the coarser sampling's error.
    pub kept_strict: Vec<RefusedEdge>,
    /// Poles of higher order (no normal at the point) whose limit normal the kernel refused
    /// for one of their triangles (a pole of order four or more, or another refusal): each is
    /// meshed on the planner's layout, its normal the area-weighted mean of the face's
    /// triangles' normals there, a display normal only, named here with the kernel's reason.
    /// The other poles of higher order get the exact limit normal per triangle
    /// ([`crate::PoleRule`]).
    pub averaged_poles: Vec<crate::AveragedPole>,
}

/// An edge the planner could not sample: its index in the record and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedEdge {
    /// The edge's index in the record.
    pub edge: usize,
    /// Why it was refused.
    pub reason: String,
}

/// A face a method refused: its index in the record and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedFace {
    /// The face's index in the record.
    pub face: usize,
    /// Why it was refused.
    pub reason: String,
}
