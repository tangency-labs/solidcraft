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
    /// Faces a memory bound of the planner limited: their grid thinned below the lines their
    /// tolerance asked for, or their sag refinement stopped at the points it may add. Each is
    /// still meshed, named here with the sag its triangles reach and the bounds that limited it,
    /// so a mesh that leaves the tolerance is never so unreported.
    pub over_tolerance: Vec<OverTolerance>,
    /// Edges whose sampling at a coarser facet angle, for a face crowded with segments, failed:
    /// each keeps its nodes at the rules' own level (valid, only finer), no face is refused, and
    /// the edge is named here with the coarser sampling's error.
    pub kept_strict: Vec<RefusedEdge>,
    /// Edges whose sampling stopped at a bound ([`EdgeCap`]) with a segment still wanting a
    /// split: their chords may lie off them by more than the tolerance (no refinement follows an
    /// edge's sampling), each named with the bound and how far its chords lie off.
    pub edges_capped: Vec<CappedEdge>,
    /// Poles of higher order (no normal at the point) whose limit normal the kernel refused
    /// for one of their triangles (a pole of order four or more, or another refusal): each is
    /// meshed on the planner's layout, its normal the area-weighted mean of the face's
    /// triangles' normals there, a display normal only, named here with the kernel's reason.
    /// The other poles of higher order get the exact limit normal per triangle
    /// ([`crate::PoleRule`]).
    pub averaged_poles: Vec<crate::AveragedPole>,
    /// The mesh path's ([`crate::mesh()`]) faces left short of its shape: a triangle sharper
    /// than the spec's angle, or a bound that stopped the shaping. Each is meshed, named with
    /// its smallest angle and what limited it. Apart from [`PlanStats::over_tolerance`], which
    /// names the faces whose sag refinement stopped, as on the display path.
    pub unshaped: Vec<Unshaped>,
}

/// A face the mesh path left short of its shape (see [`PlanStats::unshaped`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Unshaped {
    /// The face's index in the record.
    pub face: usize,
    /// Its triangles' smallest angle in 3D (degrees).
    pub smallest_angle: f64,
    /// What limited it, each once, in the order met; empty where nothing did and a triangle is
    /// still sharper than the spec's angle.
    pub limits: Vec<ShapeLimit>,
}

/// What stopped the mesh path's shaping of a face ([`Unshaped::limits`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeLimit {
    /// It added all the points a face may have (`MAX_MESH_REFINE`, 200 000) with a triangle
    /// still too sharp, too long or off the surface.
    Points,
    /// Its lattice had more seeds than a face may have (`MAX_SEEDS`, 200 000): it was refined
    /// from its boundary alone.
    Seeds,
    /// A point it would add encroaches on a boundary side, and the rounds of putting such points
    /// on the edges (`MAX_SPLIT_ROUNDS`, 16) ran out: the side was kept whole.
    Rounds,
    /// A point it would add encroaches on a boundary side on no edge (a pole's collapsed side,
    /// the line a seamless face is cut along), which no node can be put on: kept whole.
    SidesOnNoEdge,
    /// The point it would add lies nearer a node than a quarter of the triangle's longest side
    /// (where the surface's Delaunay rule could not be kept in (u, v)): not added, as it would
    /// make a sliver.
    Crowded,
    /// A triangle smaller than refinement splits (the spec's chord over a thousand, squared) is
    /// still too sharp.
    Floor,
    /// A triangle's sharpest corner is a corner of the boundary sharper than the spec's angle,
    /// between two of its sides: no point inside the face widens it.
    Corner,
}

/// An edge a bound of its sampling stopped (see [`PlanStats::edges_capped`]).
#[derive(Clone, Debug, PartialEq)]
pub struct CappedEdge {
    /// The edge's index in the record.
    pub edge: usize,
    /// The bound: [`EdgeCap::Nodes`] where both were met.
    pub cap: EdgeCap,
    /// The farthest any of the edge's chords lies from its curve (mm), at their quarter points
    /// and middles (a sampled lower bound: exact where a chord's farthest point is its middle,
    /// as on a circle; below it at a corner between them). The segments exceeding their rules
    /// most are halved first, so at the node bound it is spread along the edge. A line's
    /// chords lie on it: 0.
    pub sag: f64,
    /// The most any of its chords exceeds its rules: the largest of its sag over the tolerance,
    /// its length over the side asked, its turn over a quarter turn and over the facet angle
    /// (past 1 for a rule broken; a line's, its sides' length over the side asked).
    pub excess: f64,
    /// The chord tolerance the edge was sampled to (mm).
    pub tol: f64,
    /// Whether `sag` is within `tol`: the chords follow the curve, and only a side's length or
    /// turn is past its rule (as at a corner the facet angle never settles at, or a line).
    pub within: bool,
}

/// A bound of an edge's sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeCap {
    /// The edge has the most nodes an edge may have (`MAX_EDGE_NODES`, 20 000): for a line,
    /// its sides are longer than the side asked.
    Nodes,
    /// A segment was halved as often as one may be (24 times).
    Depth,
}

/// A face a memory bound limited (see [`PlanStats::over_tolerance`]).
#[derive(Clone, Debug, PartialEq)]
pub struct OverTolerance {
    /// The face's index in the record.
    pub face: usize,
    /// The farthest its triangles lie from its surface (mm), measured as the sag refinement
    /// measures them: at each triangle's middle and its sides' middles, along the surface's
    /// normal there, wherever the surface evaluates.
    pub sag: f64,
    /// The measurements `sag` could not make (one per middle per triangle, so a side shared by
    /// two triangles counts twice): the surface has no point there.
    pub unmeasured: usize,
    /// The chord tolerance the face was planned to (mm).
    pub tol: f64,
    /// Whether every middle was measured and `sag` is within `tol` after all: the refinement
    /// made up what a thinned grid left (for [`Cap::Refine`], which stops with a side still
    /// over, normally not, unless a flip after the refinement laid that side down).
    pub within: bool,
    /// The bounds that limited it, one or both.
    pub caps: Vec<Cap>,
}

/// A memory bound of the planner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    /// The face's grid was thinned to fit `MAX_FACE_GRID` points: the
    /// lines along u and v its division gave, and those kept. `asked` is the division's own
    /// count, itself bounded (at most `MAX_LINES` lines, 20 halvings an interval); `kept` is before
    /// any further thinning a cut grid makes for its traces. The sag refinement's measure of the
    /// final triangles ([`OverTolerance::sag`]) covers both.
    GridThinned {
        /// The lines along u and v the division gave.
        asked: [usize; 2],
        /// The lines kept.
        kept: [usize; 2],
    },
    /// The sag refinement added all the points it may (`MAX_REFINE`) with triangles still off
    /// the surface by more than the tolerance.
    Refine,
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
