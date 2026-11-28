//! DXF geometry ([`Dxf`]): a path's pieces converted for a DXF file, their bounds, a drawing's
//! scale and translation, and the checks a read or written entity must pass. The kernel keeps
//! the original piece indices, admission rules and precision checks; the values here are plain
//! data in the record's vectors, and refusals carry the kernel's words.

use solidcore_record::{Seg2, Vec2};

use crate::Backend;

/// A refused path conversion, with the original piece's index and the path's piece count where
/// it is about one piece.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct PathRefusal {
    /// Zero-based source piece index and original piece count, when applicable.
    pub piece: Option<(usize, usize)>,
    /// The kernel's words, format-neutral.
    pub reason: String,
}

/// How the writer encodes the numbers it writes, used only to certify that a shape is
/// representable. Each callback is deterministic and returns the decoded value actually written.
#[derive(Clone, Copy, Debug)]
pub struct ScalarEncoding {
    /// Encode and decode one length (a coordinate, axis or radius), or refuse it.
    pub length: fn(f64) -> Option<f64>,
    /// Encode and decode one dimensionless scalar (an angle, parameter or weight), or refuse it.
    pub dimensionless: fn(f64) -> Option<f64>,
    /// Smallest minor-to-major ratio the writer represents.
    pub min_ellipse_ratio: f64,
    /// Parameters this close to a full turn are written as zero.
    pub turn_snap_radians: f64,
}

/// How a path's curves are represented. Coordinates and tolerances are in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathStyle {
    /// Preserve analytic curves and the original polynomial/rational spline controls.
    Exact,
    /// Collect consecutive lines and circular arcs into polylines with signed bulges. Other
    /// curves retain their exact representation and interrupt the polyline run. An arc that
    /// cannot preserve its support as a bulge retains its ARC entity instead.
    Polyline,
    /// Fit free-form pieces with the existing bounded line/arc fitter. Its sampling is not an
    /// analytic, uniform error certificate.
    Arcs {
        /// Requested distance in millimetres.
        tolerance_mm: f64,
    },
}

/// A polyline vertex and the signed bulge of the edge leaving it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// Stored vertex.
    pub at: Vec2,
    /// Signed bulge of the departing edge.
    pub bulge: f64,
}

/// Exact export geometry.
#[derive(Clone, Debug, PartialEq)]
pub enum PathShape {
    /// A line with its stored endpoints.
    Line(Vec2, Vec2),
    /// A circle with a centre and positive radius.
    Circle {
        /// Centre.
        c: Vec2,
        /// Radius.
        r: f64,
    },
    /// A counterclockwise arc in degrees.
    Arc {
        /// Centre.
        c: Vec2,
        /// Radius.
        r: f64,
        /// Start angle.
        a0: f64,
        /// End angle.
        a1: f64,
    },
    /// An ellipse in its axis frame.
    Ellipse {
        /// Centre.
        c: Vec2,
        /// Major axis vector.
        major: Vec2,
        /// Minor-to-major ratio.
        ratio: f64,
        /// Start parameter.
        t0: f64,
        /// End parameter.
        t1: f64,
    },
    /// A spline retaining its original control values.
    Spline {
        /// Polynomial degree.
        degree: u8,
        /// Clamped knots.
        knots: Vec<f64>,
        /// Control points.
        ctrl: Vec<Vec2>,
        /// Rational weights when present.
        weights: Option<Vec<f64>>,
    },
    /// A run of circular arcs and lines.
    BulgedPolyline {
        /// Vertices in input order.
        vertices: Vec<Vertex>,
        /// Whether the last edge ends at the first vertex.
        closed: bool,
    },
}

/// Conservative bounds in record coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Minimum coordinates.
    pub min: Vec2,
    /// Maximum coordinates.
    pub max: Vec2,
}

/// Whether a read ellipse's supporting axes can be taken as they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EllipseSupport {
    /// Finite, nondegenerate supporting axes.
    Valid,
    /// Invalid centre, major axis or ratio.
    InvalidMajor,
    /// The minor axis collapses in represented units.
    CollapsedMinor,
}

/// The DXF geometry of one kernel ([`crate::OpKind::Dxf`]).
pub trait Dxf: Backend {
    /// The most source pieces, result entities or accumulated polyline vertices one path takes.
    const MAX_PATH_ITEMS: usize;

    // Writing.
    /// Check source geometry, style and budgets before any conversion.
    fn admit_path(&self, segs: &[Seg2], style: PathStyle) -> Result<(), PathRefusal>;
    /// Admit one piece, keeping its original index in any refusal.
    fn admit_piece(&self, seg: &Seg2, index: usize, total: usize) -> Result<(), PathRefusal>;
    /// A whole circle or ellipse recognised in a closed path, without converting separate
    /// pieces or fitting; `None` for an open path or one that is neither.
    fn recognized_path(&self, segs: &[Seg2], closed: bool, style: PathStyle, encoding: ScalarEncoding) -> Result<Option<PathShape>, PathRefusal>;
    /// Admitted exact or polyline geometry converted; the caller fits free-form arcs.
    fn path_shapes(&self, segs: &[Seg2], closed: bool, style: PathStyle, encoding: ScalarEncoding) -> Result<Vec<PathShape>, PathRefusal>;
    /// One source or fitted piece converted, with the original source index.
    fn exact_piece(&self, seg: &Seg2, index: usize, total: usize, encoding: ScalarEncoding) -> Result<PathShape, PathRefusal>;
    /// The arc of a bulged edge from `from` to `to`, for checking a written vertex without
    /// changing its stored values; `None` where it has none.
    fn bulge_arc(&self, from: Vertex, to: Vec2) -> Option<Seg2>;
    /// A bulged edge's endpoints and directed cardinal extrema, at most six points.
    fn bulged_edge_points(&self, from: Vertex, to: Vec2) -> Result<Vec<Vec2>, PathRefusal>;
    /// Whether degree-valued arc fields can be sampled finitely.
    fn arc_fields_finite(&self, a0: f64, a1: f64) -> bool;
    /// Whether rounded degree endpoints remain distinct modulo one turn.
    fn distinct_degrees(&self, start: f64, end: f64) -> bool;
    /// Whether a degree value has a finite radian representation.
    fn finite_degrees(&self, angle: f64) -> bool;
    /// Whether an interval's difference is finite.
    fn finite_span(&self, start: f64, end: f64) -> bool;
    /// A dimension scaled, refusing a result that is not positive and finite.
    fn scaled_positive(&self, value: f64, scale: f64) -> Result<f64, PathRefusal>;

    // Bounds and transforms.
    /// Two validated bounds united.
    fn union_bounds(&self, amin: Vec2, amax: Vec2, bmin: Vec2, bmax: Vec2) -> Bounds;
    /// A whole circle's bounds, rounded outward.
    fn supporting_circle(&self, c: Vec2, r: f64) -> Result<Bounds, PathRefusal>;
    /// A whole ellipse's bounds, outward along its axes.
    fn ellipse_bounds(&self, c: Vec2, major: Vec2, ratio: f64, t0: f64, t1: f64) -> Result<Bounds, PathRefusal>;
    /// Stored points' bounds.
    fn point_bounds(&self, points: &[Vec2]) -> Result<Bounds, PathRefusal>;
    /// `p` under a positive uniform scale and a translation.
    fn transformed_point(&self, p: Vec2, scale: f64, translation: Vec2) -> Result<Vec2, PathRefusal>;
    /// An axis scaled, not translated.
    fn scaled_axis(&self, axis: Vec2, scale: f64) -> Result<Vec2, PathRefusal>;
    /// Whether an axis has a finite nonzero length.
    fn axis_nonzero(&self, axis: Vec2) -> bool;
    /// A bulged path's bounds: `count` vertices, at most `max_vertices` and `max_work`, checked
    /// before any vertex is read.
    fn bulged_bounds(
        &self,
        vertices: &mut dyn Iterator<Item = Vertex>,
        count: usize,
        closed: bool,
        max_vertices: usize,
        max_work: usize,
    ) -> Result<Bounds, PathRefusal>;
    /// A retained source anchor under a scale and a translation.
    fn transform_anchor(&self, point: Vec2, scale: f64, translation: Vec2) -> Vec2;

    // Reading.
    /// Coordinates scaled by a source's unit.
    fn scale_point(&self, point: Vec2, scale: f64) -> Vec2;
    /// A stored scalar scaled by a source's unit.
    fn scale_value(&self, value: f64, scale: f64) -> f64;
    /// Whether a unit's scale collapsed a nonzero coordinate of `source` to zero in `scaled`.
    fn collapsed_coordinate(&self, source: Vec2, scaled: Vec2) -> bool;
    /// Whether a read ellipse's supporting axes can be taken as they are, without changing either.
    fn ellipse_support(&self, center: Vec2, major: Vec2, ratio: f64) -> EllipseSupport;
}
