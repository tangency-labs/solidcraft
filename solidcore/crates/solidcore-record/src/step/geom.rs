//! STEP curves (ISO 10303-42) as plain data: the curves a file defines, exactly as it defines
//! them, with lengths in millimetres and angles in radians. Nothing is approximated, bounded,
//! normalised or turned into another kind on the way in: a circle stays a circle with its
//! placement, a line stays unbounded, a B-spline keeps its own knots and form, and a direction
//! keeps the ratios the file writes. Every curve keeps STEP's own parameterisation. Surfaces,
//! surface curves and pcurves follow in the next slice (#2578, slice 2c).
//!
//! Entities the file shares (one basis curve under many trimmed curves) are shared here too,
//! through `Arc`, so reading is linear in the file however often an entity is referenced.
//!
//! The record computes no geometric quantity (docs/layers.md §1, §4): a direction is not made a
//! unit vector, a placement's reference direction is not made orthogonal to its axis (nor chosen
//! when the file omits it), its y axis is not formed, a vector's length is not taken, and nothing
//! is evaluated. Those are the kernel's, on its own types, when it builds from this data (#2578,
//! slice 4). The formulas on the variants below say what the data means, in ISO 10303-42's
//! terms; they are not computed here.
//!
//! Origin: SolidCraft's `crates/step/src/geom.rs` at `a5b71ca2` (MIT OR Apache-2.0).
//!
//! Deviations from SolidCraft: [`Placement`] keeps `axis` and `ref_direction` as the file gives
//! them (each optional, neither normalised; was: both unit and orthogonal, a missing reference
//! direction chosen perpendicular to the axis). A `VECTOR` is a [`Vector`] of the written
//! orientation, magnitude and unit factors (was: a unit direction and the vector's length), in
//! [`Curve::Line`]. Removed, to come back as kernel primitives in slice 4: `Placement::y`,
//! `Placement::at` and the vector helpers `add`, `sub`, `scale`, `dot`, `cross`, `norm`. Doc
//! comments on every public item and field.

use std::sync::Arc;

/// A point or vector: `[x, y, z]` (z = 0 in a 2D parameter space).
pub type P3 = [f64; 3];

/// `AXIS2_PLACEMENT_3D`, or `AXIS2_PLACEMENT_2D` (no `axis`): an origin and the directions of
/// its local Z and X axes, as the file writes them.
///
/// ISO 10303-42 makes the local axes from these: Z is `axis` (+Z when absent), X is
/// `ref_direction` made orthogonal to Z (a chosen perpendicular when absent), Y is Z × X. That
/// construction, with its checks (a zero or parallel reference direction), is the kernel's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The location, in millimetres (or the parameter space's units).
    pub origin: P3,
    /// Local Z as the file gives it (not normalised); `None` when the file omits it and for an
    /// `AXIS2_PLACEMENT_2D`, which ISO 10303-42 defines as +Z.
    pub axis: Option<P3>,
    /// Local X as the file gives it (not normalised, not made orthogonal to `axis`); `None` when
    /// the file omits it.
    pub ref_direction: Option<P3>,
}

/// `VECTOR`: an orientation and a magnitude, as the file writes them, and the unit factors of
/// the space they are in.
///
/// In millimetres and radians, component `i` of the vector is
/// `scale[i] · magnitude · orientation[i] / |orientation|`. The record does not form it; the
/// kernel does (slice 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    /// The direction ratios as written (not normalised; never all zero).
    pub orientation: P3,
    /// The magnitude as written, in file units (positive).
    pub magnitude: f64,
    /// The unit factor of each component: the length unit in model space, the parameter's unit
    /// (length, angle, or 1) in a surface's parameter space.
    pub scale: P3,
}

/// A STEP LOGICAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logical {
    /// `.T.`
    True,
    /// `.F.`
    False,
    /// `.U.`
    Unknown,
}

/// `b_spline_curve_form`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveForm {
    /// `.POLYLINE_FORM.`
    PolylineForm,
    /// `.CIRCULAR_ARC.`
    CircularArc,
    /// `.ELLIPTIC_ARC.`
    EllipticArc,
    /// `.PARABOLIC_ARC.`
    ParabolicArc,
    /// `.HYPERBOLIC_ARC.`
    HyperbolicArc,
    /// `.UNSPECIFIED.`, or anything else.
    Unspecified,
}

/// `knot_type`. A B-spline given without knots (`BEZIER_CURVE`, `UNIFORM_CURVE`,
/// `QUASI_UNIFORM_CURVE`) has them laid out on reading, and keeps the matching type here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnotSpec {
    /// `.UNIFORM_KNOTS.`
    UniformKnots,
    /// `.QUASI_UNIFORM_KNOTS.`
    QuasiUniformKnots,
    /// `.PIECEWISE_BEZIER_KNOTS.`
    PiecewiseBezierKnots,
    /// `.UNSPECIFIED.`, or anything else.
    Unspecified,
}

/// A B-spline curve: distinct knots with their multiplicities, as STEP writes them.
#[derive(Clone, Debug, PartialEq)]
pub struct BSplineCurve {
    /// The degree (1 to `read::MAX_DEGREE`).
    pub degree: usize,
    /// The control points.
    pub points: Vec<P3>,
    /// Present for a rational curve: one positive weight per point.
    pub weights: Option<Vec<f64>>,
    /// The distinct knots, increasing.
    pub knots: Vec<f64>,
    /// Each knot's multiplicity (1 to degree + 1); they add up to points + degree + 1.
    pub multiplicities: Vec<usize>,
    /// The curve form.
    pub form: CurveForm,
    /// `closed_curve`.
    pub closed: Logical,
    /// `self_intersect`.
    pub self_intersect: Logical,
    /// The knot type.
    pub knot_spec: KnotSpec,
}

/// One end of a `TRIMMED_CURVE`: a point on the basis curve or its parameter (a trim lists one
/// or both).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trim {
    /// A `CARTESIAN_POINT`.
    Point(P3),
    /// A `PARAMETER_VALUE` (radians on a circle or an ellipse).
    Parameter(f64),
}

/// `trimming_preference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimPreference {
    /// `.CARTESIAN.`
    Cartesian,
    /// `.PARAMETER.`
    Parameter,
    /// `.UNSPECIFIED.`, or anything else.
    Unspecified,
}

/// A STEP curve.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// `LINE`: `origin + t·v`, with `v` the [`Vector`].
    Line {
        /// `pnt`.
        origin: P3,
        /// `dir`.
        vector: Vector,
    },
    /// `CIRCLE`. Parameter: the angle from the placement's X axis.
    Circle {
        /// The placement.
        position: Placement,
        /// The radius (positive).
        radius: f64,
    },
    /// `ELLIPSE`: `origin + a·cos t·X + b·sin t·Y`.
    Ellipse {
        /// The placement.
        position: Placement,
        /// `a`, along X (positive).
        semi_axis_1: f64,
        /// `b`, along Y (positive).
        semi_axis_2: f64,
    },
    /// `HYPERBOLA`: `origin + a·cosh t·X + b·sinh t·Y`.
    Hyperbola {
        /// The placement.
        position: Placement,
        /// `a` (positive).
        semi_axis: f64,
        /// `b` (positive).
        semi_imag_axis: f64,
    },
    /// `PARABOLA`: `origin + f·t²·X + 2f·t·Y`.
    Parabola {
        /// The placement.
        position: Placement,
        /// `f` (not zero).
        focal_dist: f64,
    },
    /// `B_SPLINE_CURVE` and its subtypes.
    BSpline(Box<BSplineCurve>),
    /// `POLYLINE`: segment `i` (from 0) is the parameters `[i, i + 1]`.
    Polyline(Vec<P3>),
    /// `TRIMMED_CURVE`.
    Trimmed {
        /// `basis_curve`.
        basis: Arc<Curve>,
        /// `trim_1` (one or two items).
        trim_1: Vec<Trim>,
        /// `trim_2` (one or two items).
        trim_2: Vec<Trim>,
        /// `sense_agreement`.
        sense: bool,
        /// `master_representation`.
        preference: TrimPreference,
    },
    /// `OFFSET_CURVE_3D`.
    OffsetCurve3d {
        /// `basis_curve`.
        basis: Arc<Curve>,
        /// `distance`, in millimetres.
        distance: f64,
        /// `self_intersect`.
        self_intersect: Logical,
        /// `ref_direction` as the file gives it (not normalised).
        ref_direction: P3,
    },
}
