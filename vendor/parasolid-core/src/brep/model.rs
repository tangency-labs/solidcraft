//! Parasolid-native B-Rep topology and geometry values.

use std::ops::Range;

/// Document-local identifier assigned by the B-Rep mapper.
pub type BrepId = u32;

/// Raw-node provenance retained separately from a document-local B-Rep identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNodeRef {
    /// Transmit index in the parsed raw document.
    pub node_index: u32,
    /// Numeric schema type.
    pub node_type: u16,
    /// Effective schema name.
    pub type_name: String,
    /// Persistent Parasolid node-id when that entity carries one.
    pub node_id: Option<i32>,
    /// Complete source record range.
    pub byte_range: Range<usize>,
}

/// Original transmit encoding used to produce a mapped source model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrepSourceFormat {
    /// Neutral binary transmit.
    Binary,
    /// Text transmit.
    Text,
}

impl BrepSourceFormat {
    /// Stable public string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Text => "text",
        }
    }
}

/// Positive or negative Parasolid orientation flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sense {
    /// `+` in the transmit stream.
    Positive,
    /// `-` in the transmit stream.
    Negative,
    /// `?` in the transmit stream, used where orientation is not significant.
    Unknown,
}

impl Sense {
    /// Stable public string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
            Self::Unknown => "unknown",
        }
    }

    /// Numeric multiplier for oriented vector calculations, if orientation is significant.
    #[must_use]
    pub const fn multiplier(self) -> Option<f64> {
        match self {
            Self::Positive => Some(1.0),
            Self::Negative => Some(-1.0),
            Self::Unknown => None,
        }
    }
}

/// Parasolid body classification from `body_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// Manifold three-dimensional volume (`1`).
    Solid,
    /// One-dimensional wire/acorn body (`2`).
    Wire,
    /// Two-dimensional sheet body (`3`).
    Sheet,
    /// General body without manifold restrictions (`6`).
    General,
}

impl BodyKind {
    /// Stable public string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Wire => "wire",
            Self::Sheet => "sheet",
            Self::General => "general",
        }
    }
}

/// Parasolid region classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// Material region (`S`).
    Solid,
    /// Void region (`V`).
    Void,
}

impl RegionKind {
    /// Stable public string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Void => "void",
        }
    }
}

/// Cartesian three-vector retained in source units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Vector3 {
    /// Construct from a fixed array.
    #[must_use]
    pub const fn from_array(value: [f64; 3]) -> Self {
        Self { x: value[0], y: value[1], z: value[2] }
    }

    /// Convert to a fixed array.
    #[must_use]
    pub const fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

/// Axis-aligned bounds derived from mapped vertex points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Minimum coordinates.
    pub minimum: Vector3,
    /// Maximum coordinates.
    pub maximum: Vector3,
}

/// One mapped body.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Body classification.
    pub kind: BodyKind,
    /// Size resolution stored on the body.
    pub size_resolution: f64,
    /// Linear modeller resolution stored on the body.
    pub linear_resolution: f64,
    /// Null-terminated region chain in source order.
    pub regions: Vec<BrepId>,
    /// Non-wireframe edge chain owned directly by the body.
    pub edges: Vec<BrepId>,
    /// Vertex chain owned directly by the body.
    pub vertices: Vec<BrepId>,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One material or void region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Material/void classification.
    pub kind: RegionKind,
    /// Owning body.
    pub body: BrepId,
    /// Shell chain in source order.
    pub shells: Vec<BrepId>,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One connected boundary component of a region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Owning region.
    pub region: BrepId,
    /// Back-face chain (`face` / `next`).
    pub back_faces: Vec<BrepId>,
    /// Front-face chain (`front_face` / `next_front`).
    pub front_faces: Vec<BrepId>,
    /// Wireframe edge chain for a wire/sheet shell.
    pub wire_edges: Vec<BrepId>,
    /// Single acorn vertex, when present.
    pub isolated_vertex: Option<BrepId>,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One oriented surface subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Shell for which this is a back face.
    pub back_shell: BrepId,
    /// Shell for which this is a front face.
    pub front_shell: BrepId,
    /// Loop chain in source order.
    pub loops: Vec<BrepId>,
    /// Attached surface, if present.
    pub surface: Option<BrepId>,
    /// Orientation relative to the attached surface.
    pub sense: Sense,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One connected component of a face boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loop {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Owning face.
    pub face: BrepId,
    /// Ordered fin/half-edge ring using `forward` links.
    pub half_edges: Vec<BrepId>,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One oriented use of an edge by a loop, including dummy fins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HalfEdge {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Owning loop; absent for a dummy fin.
    pub loop_id: Option<BrepId>,
    /// Next fin around the loop.
    pub forward: Option<BrepId>,
    /// Previous fin around the loop.
    pub backward: Option<BrepId>,
    /// Forward vertex of this fin; absent for a ring edge.
    pub vertex: Option<BrepId>,
    /// Next fin around the owning edge.
    pub other: Option<BrepId>,
    /// Owning edge; absent for an isolated-loop fin.
    pub edge: Option<BrepId>,
    /// Per-fin curve used by tolerant edges.
    pub curve: Option<BrepId>,
    /// Orientation relative to the owning edge.
    pub sense: Sense,
    /// Whether the fin is internal/dummy rather than part of a loop.
    pub dummy: bool,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One bounded subset of a curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Owner node retained as provenance because it may be a body or shell.
    pub owner: SourceNodeRef,
    /// Ordered fin ring using `other` links.
    pub half_edges: Vec<BrepId>,
    /// Backward/start vertex; absent for a ring edge.
    pub start_vertex: Option<BrepId>,
    /// Forward/end vertex; absent for a ring edge.
    pub end_vertex: Option<BrepId>,
    /// Attached curve; absent for a tolerant edge.
    pub curve: Option<BrepId>,
    /// Explicit edge tolerance, or `None` for an accurate edge.
    pub tolerance: Option<f64>,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One topological point.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    /// Document-local B-Rep identifier.
    pub id: BrepId,
    /// Attached point geometry.
    pub point: BrepId,
    /// Explicit vertex tolerance, or `None` for an accurate vertex.
    pub tolerance: Option<f64>,
    /// Owner node retained as provenance because it may be a body or shell.
    pub owner: SourceNodeRef,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// One point geometry entity.
#[derive(Debug, Clone, PartialEq)]
pub struct PointGeometry {
    /// Document-local point identifier.
    pub id: BrepId,
    /// Position in source units.
    pub position: Vector3,
    /// Topological or part owner when transmitted.
    pub owner: Option<SourceNodeRef>,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// Exact non-uniform rational B-spline curve data.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve {
    /// Polynomial degree.
    pub degree: u16,
    /// Number of control vertices.
    pub control_vertex_count: u32,
    /// Number of doubles per stored control vertex.
    pub vertex_dimension: u16,
    /// Knot classification byte retained from the source.
    pub knot_type: u8,
    /// Whether parameterization is periodic.
    pub periodic: bool,
    /// Whether endpoints coincide.
    pub closed: bool,
    /// Whether control vertices use homogeneous weights.
    pub rational: bool,
    /// Curve-form classification byte retained from the source.
    pub curve_form: u8,
    /// Control vertices grouped by `vertex_dimension`.
    pub control_vertices: Vec<Vec<f64>>,
    /// Distinct knots.
    pub knots: Vec<f64>,
    /// Multiplicity for each distinct knot.
    pub knot_multiplicities: Vec<u16>,
    /// Auxiliary-node provenance required to assemble the definition.
    pub sources: Vec<SourceNodeRef>,
}

/// Exact non-uniform rational B-spline surface data.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct NurbsSurface {
    /// U and V degrees.
    pub u_degree: u16,
    pub v_degree: u16,
    /// Control-grid dimensions.
    pub u_control_vertex_count: u32,
    pub v_control_vertex_count: u32,
    /// Number of doubles per stored control vertex.
    pub vertex_dimension: u16,
    /// Knot classification bytes retained from the source.
    pub u_knot_type: u8,
    pub v_knot_type: u8,
    /// Periodicity and closure flags.
    pub u_periodic: bool,
    pub v_periodic: bool,
    pub u_closed: bool,
    pub v_closed: bool,
    /// Whether control vertices use homogeneous weights.
    pub rational: bool,
    /// Surface-form classification byte retained from the source.
    pub surface_form: u8,
    /// Flattened row-major control vertices grouped by `vertex_dimension`.
    pub control_vertices: Vec<Vec<f64>>,
    /// Distinct U/V knots and corresponding multiplicities.
    pub u_knots: Vec<f64>,
    pub v_knots: Vec<f64>,
    pub u_knot_multiplicities: Vec<u16>,
    pub v_knot_multiplicities: Vec<u16>,
    /// Auxiliary-node provenance required to assemble the definition.
    pub sources: Vec<SourceNodeRef>,
}

/// Curve geometry definition.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveKind {
    /// `R(t) = point + t * direction`.
    Line { point: Vector3, direction: Vector3 },
    /// Analytic circle.
    Circle { center: Vector3, normal: Vector3, x_axis: Vector3, radius: f64 },
    /// Analytic ellipse.
    Ellipse { center: Vector3, normal: Vector3, x_axis: Vector3, major_radius: f64, minor_radius: f64 },
    /// Analytic parabola.
    Parabola { origin: Vector3, normal: Vector3, x_axis: Vector3, focal_length: f64 },
    /// Analytic hyperbola.
    Hyperbola { origin: Vector3, normal: Vector3, x_axis: Vector3, transverse_radius: f64, conjugate_radius: f64 },
    /// Curve restricted to two parameter values and endpoint positions. On a
    /// line whose stored parameters are both unset, they are the parameters of
    /// the stored end points.
    Trimmed { basis_curve: BrepId, start_point: Vector3, end_point: Vector3, start_parameter: f64, end_parameter: f64 },
    /// Exact NURBS definition assembled from auxiliary nodes.
    Nurbs(NurbsCurve),
    /// Curve embedded in a surface parameter space.
    SurfaceParametric { surface: BrepId, parameter_curve: BrepId, original_curve: Option<BrepId>, tolerance_to_original: Option<f64> },
    /// Exact surface/surface intersection identity and limit provenance.
    Intersection {
        surfaces: [BrepId; 2],
        chart: SourceNodeRef,
        /// Ordered source points used to identify the intersection branch, not a fitted curve.
        chart_points: Vec<Vector3>,
        start: SourceNodeRef,
        /// Source limit points: one ordinary point, or singularity then branch point.
        start_points: Vec<Vector3>,
        end: SourceNodeRef,
        /// Source limit points in the same format as `start_points`.
        end_points: Vec<Vector3>,
        intersection_data: Option<SourceNodeRef>,
    },
    /// Geometry type retained without claiming semantic support.
    Unsupported { type_name: String },
}

impl CurveKind {
    /// Stable public variant name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Line { .. } => "line",
            Self::Circle { .. } => "circle",
            Self::Ellipse { .. } => "ellipse",
            Self::Parabola { .. } => "parabola",
            Self::Hyperbola { .. } => "hyperbola",
            Self::Trimmed { .. } => "trimmed",
            Self::Nurbs(_) => "nurbs",
            Self::SurfaceParametric { .. } => "surface_parametric",
            Self::Intersection { .. } => "intersection",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

/// One curve geometry entity.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveGeometry {
    /// Document-local curve identifier.
    pub id: BrepId,
    /// Orientation of the curve's natural parameterization.
    pub sense: Sense,
    /// Topological or part owner when transmitted.
    pub owner: Option<SourceNodeRef>,
    /// Decoded geometry definition.
    pub kind: CurveKind,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// Exact rolling-ball blend classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendType {
    /// `R`: rolling-ball blend.
    RollingBall,
    /// `E`: cliff-edge blend.
    CliffEdge,
}

impl BlendType {
    /// Stable public string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RollingBall => "rolling_ball",
            Self::CliffEdge => "cliff_edge",
        }
    }
}

/// Surface geometry definition.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceKind {
    /// Analytic plane.
    Plane { point: Vector3, normal: Vector3, x_axis: Vector3 },
    /// Analytic cylinder.
    Cylinder { point: Vector3, axis: Vector3, radius: f64, x_axis: Vector3 },
    /// Analytic cone.
    Cone { point: Vector3, axis: Vector3, radius: f64, sin_half_angle: f64, cos_half_angle: f64, x_axis: Vector3 },
    /// Analytic sphere.
    Sphere { center: Vector3, radius: f64, axis: Vector3, x_axis: Vector3 },
    /// Analytic torus.
    Torus { center: Vector3, axis: Vector3, major_radius: f64, minor_radius: f64, x_axis: Vector3 },
    /// Exact rolling-ball or cliff-edge blend definition.
    BlendedEdge {
        blend_type: BlendType,
        supporting_surfaces: [BrepId; 2],
        spine_curve: BrepId,
        ranges: [f64; 2],
        thumb_weights: [f64; 2],
        boundary_surfaces: [Option<BrepId>; 2],
        start: Option<SourceNodeRef>,
        end: Option<SourceNodeRef>,
    },
    /// Construction surface identifying one boundary of a blend surface.
    BlendBoundary { boundary_index: u8, blend_surface: BrepId },
    /// Offset of another mapped surface.
    Offset { basis_surface: BrepId, offset: f64 },
    /// Exact NURBS definition assembled from auxiliary nodes.
    Nurbs(NurbsSurface),
    /// Geometry type retained without claiming semantic support.
    Unsupported { type_name: String },
}

impl SurfaceKind {
    /// Stable public variant name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Plane { .. } => "plane",
            Self::Cylinder { .. } => "cylinder",
            Self::Cone { .. } => "cone",
            Self::Sphere { .. } => "sphere",
            Self::Torus { .. } => "torus",
            Self::BlendedEdge { .. } => "blended_edge",
            Self::BlendBoundary { .. } => "blend_boundary",
            Self::Offset { .. } => "offset",
            Self::Nurbs(_) => "nurbs",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

/// One surface geometry entity.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceGeometry {
    /// Document-local surface identifier.
    pub id: BrepId,
    /// Orientation of the surface's natural normal.
    pub sense: Sense,
    /// Topological or part owner when transmitted.
    pub owner: Option<SourceNodeRef>,
    /// Decoded geometry definition.
    pub kind: SurfaceKind,
    /// Raw-node provenance.
    pub source: SourceNodeRef,
}

/// Explicit non-fatal loss or unsupported geometry finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrepDiagnostic {
    /// Stable dotted diagnostic code.
    pub code: &'static str,
    /// Human-readable explanation.
    pub message: String,
    /// Geometry/topology role which could not be represented completely.
    pub role: &'static str,
    /// Source node responsible for the finding.
    pub source: SourceNodeRef,
}

/// Counts and invariant results for one mapped topology graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyValidation {
    /// True only when all checked structural invariants passed.
    pub valid: bool,
    /// Number of loops whose `forward` ring closed at its starting fin.
    pub closed_loop_count: usize,
    /// Number of edges whose `other` ring closed at its starting fin.
    pub closed_edge_ring_count: usize,
    /// Model-wide Euler characteristic `V - E + F`.
    pub euler_characteristic: i64,
}

/// Metrics derived without a geometry-kernel dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct BrepMetrics {
    /// Bounds of all mapped topological vertex points.
    pub bounding_box: Option<BoundingBox>,
    /// Sum of oriented planar loop areas, when every face is planar.
    pub surface_area: Option<f64>,
    /// Absolute signed-polyhedron volume, when every face is planar and the solid is closed.
    pub volume: Option<f64>,
}

/// Complete Parasolid-native source model for one raw document.
#[derive(Debug, Clone, PartialEq)]
pub struct BrepModel {
    /// Raw document encoding.
    pub source_format: BrepSourceFormat,
    /// Exact internal schema key.
    pub schema_key: String,
    /// Whether topology is valid and all encountered boundary geometry is supported.
    pub complete: bool,
    /// Topology entities.
    pub bodies: Vec<Body>,
    pub regions: Vec<Region>,
    pub shells: Vec<Shell>,
    pub faces: Vec<Face>,
    pub loops: Vec<Loop>,
    pub half_edges: Vec<HalfEdge>,
    pub edges: Vec<Edge>,
    pub vertices: Vec<Vertex>,
    /// Geometry entities.
    pub points: Vec<PointGeometry>,
    pub curves: Vec<CurveGeometry>,
    pub surfaces: Vec<SurfaceGeometry>,
    /// L4 structural report.
    pub topology: TopologyValidation,
    /// Kernel-free derived metrics.
    pub metrics: BrepMetrics,
    /// Explicit non-fatal mapping losses.
    pub diagnostics: Vec<BrepDiagnostic>,
}
