//! STEP geometry (ISO 10303-42) as plain data: the curves and surfaces a file defines, exactly
//! as it defines them, in millimetres and radians. Nothing is approximated, bounded or turned
//! into another kind on the way in: a circle stays a circle with its placement, an unbounded
//! cylinder stays unbounded, and a B-spline keeps its own knots and form. Every curve and surface
//! keeps STEP's own parameterisation, so a 2D curve in a surface's parameter space means the same
//! thing here as in the file (SMLib's `SmSTEPSurface` evaluates the same parameterisation, with
//! angles in degrees where we use radians).
//!
//! Entities the file shares (one surface under many edges' pcurves) are shared here too, through
//! `Arc`, so reading is linear in the file however often an entity is referenced.

use std::sync::Arc;

/// A point or vector.
pub type P3 = [f64; 3];

/// AXIS2_PLACEMENT_3D (or _2D, with `axis` = +Z): an origin and two orthogonal unit directions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub origin: P3,
    /// Local Z.
    pub axis: P3,
    /// Local X (`ref_direction`, made orthogonal to `axis`).
    pub ref_direction: P3,
}

impl Placement {
    /// Local Y (`axis` × `ref_direction`).
    pub fn y(&self) -> P3 {
        cross(self.axis, self.ref_direction)
    }
    /// `origin + x·X + y·Y + z·Z`.
    pub fn at(&self, x: f64, y: f64, z: f64) -> P3 {
        let yy = self.y();
        add(self.origin, add(scale(self.ref_direction, x), add(scale(yy, y), scale(self.axis, z))))
    }
}

/// AXIS1_PLACEMENT: an origin and a unit direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis1 {
    pub origin: P3,
    pub direction: P3,
}

/// A STEP LOGICAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logical {
    True,
    False,
    Unknown,
}

/// `b_spline_curve_form`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveForm {
    PolylineForm,
    CircularArc,
    EllipticArc,
    ParabolicArc,
    HyperbolicArc,
    Unspecified,
}

/// `b_spline_surface_form`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceForm {
    PlaneSurf,
    CylindricalSurf,
    ConicalSurf,
    SphericalSurf,
    ToroidalSurf,
    SurfOfRevolution,
    RuledSurf,
    GeneralisedCone,
    QuadricSurf,
    SurfOfLinearExtrusion,
    Unspecified,
}

/// `knot_type`. A curve given without knots (BEZIER_CURVE, UNIFORM_CURVE, QUASI_UNIFORM_CURVE)
/// has them worked out on reading, and keeps the matching type here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnotSpec {
    UniformKnots,
    QuasiUniformKnots,
    PiecewiseBezierKnots,
    Unspecified,
}

/// A B-spline curve: distinct knots with their multiplicities, as STEP writes them.
#[derive(Clone, Debug, PartialEq)]
pub struct BSplineCurve {
    pub degree: usize,
    pub points: Vec<P3>,
    /// Present for a rational curve, one positive weight per point.
    pub weights: Option<Vec<f64>>,
    pub knots: Vec<f64>,
    pub multiplicities: Vec<usize>,
    pub form: CurveForm,
    pub closed: Logical,
    pub self_intersect: Logical,
    pub knot_spec: KnotSpec,
}

/// A B-spline surface; `points[i][j]` runs along u with `i`, along v with `j`.
#[derive(Clone, Debug, PartialEq)]
pub struct BSplineSurface {
    pub u_degree: usize,
    pub v_degree: usize,
    pub points: Vec<Vec<P3>>,
    pub weights: Option<Vec<Vec<f64>>>,
    pub u_knots: Vec<f64>,
    pub u_multiplicities: Vec<usize>,
    pub v_knots: Vec<f64>,
    pub v_multiplicities: Vec<usize>,
    pub form: SurfaceForm,
    pub u_closed: Logical,
    pub v_closed: Logical,
    pub self_intersect: Logical,
    pub knot_spec: KnotSpec,
}

/// One end of a TRIMMED_CURVE: a point on the basis curve, its parameter, or both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trim {
    Point(P3),
    Parameter(f64),
}

/// `trimming_preference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimPreference {
    Cartesian,
    Parameter,
    Unspecified,
}

/// `preferred_surface_curve_representation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferredRep {
    Curve3d,
    PcurveS1,
    PcurveS2,
}

/// Which SURFACE_CURVE subtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceCurveKind {
    Surface,
    Seam,
    Intersection,
    BoundedSurface,
}

/// PCURVE: a 2D curve in the parameter space of `surface`. Its coordinates are the surface's
/// own parameters (lengths in mm, angles in radians).
#[derive(Clone, Debug, PartialEq)]
pub struct Pcurve {
    pub surface: Arc<Surface>,
    /// A 2D curve: every point and placement has z = 0 and every placement axis is +Z.
    pub curve: Arc<Curve>,
}

/// An item of a SURFACE_CURVE's `associated_geometry`.
#[derive(Clone, Debug, PartialEq)]
pub enum Associated {
    Pcurve(Pcurve),
    Surface(Arc<Surface>),
}

/// A STEP curve.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// `origin + t·magnitude·direction`.
    Line {
        origin: P3,
        direction: P3,
        magnitude: f64,
    },
    /// Parameter: the angle from `ref_direction`.
    Circle {
        position: Placement,
        radius: f64,
    },
    Ellipse {
        position: Placement,
        semi_axis_1: f64,
        semi_axis_2: f64,
    },
    /// `origin + a·cosh t·X + b·sinh t·Y`.
    Hyperbola {
        position: Placement,
        semi_axis: f64,
        semi_imag_axis: f64,
    },
    /// `origin + f·t²·X + 2f·t·Y`.
    Parabola {
        position: Placement,
        focal_dist: f64,
    },
    BSpline(Box<BSplineCurve>),
    /// Segment `i` (from 0) is the parameters `[i, i + 1]`.
    Polyline(Vec<P3>),
    Trimmed {
        basis: Arc<Curve>,
        trim_1: Vec<Trim>,
        trim_2: Vec<Trim>,
        sense: bool,
        preference: TrimPreference,
    },
    SurfaceCurve {
        kind: SurfaceCurveKind,
        curve_3d: Arc<Curve>,
        associated: Vec<Associated>,
        master: PreferredRep,
    },
    OffsetCurve3d {
        basis: Arc<Curve>,
        distance: f64,
        self_intersect: Logical,
        ref_direction: P3,
    },
}

/// A STEP surface.
#[derive(Clone, Debug, PartialEq)]
pub enum Surface {
    /// `origin + u·X + v·Y`.
    Plane {
        position: Placement,
    },
    /// `origin + r·(cos u·X + sin u·Y) + v·Z`.
    Cylinder {
        position: Placement,
        radius: f64,
    },
    /// `origin + (r + v·tan a)·(cos u·X + sin u·Y) + v·Z`.
    Cone {
        position: Placement,
        radius: f64,
        semi_angle: f64,
    },
    /// `origin + r·cos v·(cos u·X + sin u·Y) + r·sin v·Z`.
    Sphere {
        position: Placement,
        radius: f64,
    },
    /// `origin + (R + r·cos v)·(cos u·X + sin u·Y) + r·sin v·Z`.
    Torus {
        position: Placement,
        major_radius: f64,
        minor_radius: f64,
    },
    /// A torus whose minor radius reaches the axis; `select_outer` keeps the outer part.
    DegenerateTorus {
        position: Placement,
        major_radius: f64,
        minor_radius: f64,
        select_outer: bool,
    },
    BSpline(Box<BSplineSurface>),
    /// The profile at `v` turned by the angle `u` about the axis.
    Revolution {
        profile: Arc<Curve>,
        axis: Axis1,
    },
    /// `profile(u) + v·magnitude·direction`.
    Extrusion {
        profile: Arc<Curve>,
        direction: P3,
        magnitude: f64,
    },
    Offset {
        basis: Arc<Surface>,
        distance: f64,
        self_intersect: Logical,
    },
    RectangularTrimmed {
        basis: Arc<Surface>,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        u_sense: bool,
        v_sense: bool,
    },
}

pub(crate) fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(crate) fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(crate) fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}
