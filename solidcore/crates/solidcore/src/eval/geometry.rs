//! The curves and surfaces the evaluator takes, as plain data: each kind with its own
//! parameters (an angle, a height, a knot parameter). These carry no state and do no
//! maths; [`super::SurfaceEval`] and [`super::CurveEval`] evaluate them.
//!
//! The record's kinds (`solidcore-record`) become these, and these the record's, in
//! `solidcore-convert`.

use super::spline::{basis_check, nl_curve2};
use crate::nurbs::knots::Side;

/// Right-handed orthonormal frame; z = `x_axis × y_axis` (STEP `AXIS2_PLACEMENT_3D`).
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The origin.
    pub origin: [f64; 3],
    /// The unit x axis.
    pub x_axis: [f64; 3],
    /// The unit y axis, square to x.
    pub y_axis: [f64; 3],
}

/// Unbounded 3D curves, each with its parameter `t`.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// The line through both points: `start + t * (end - start)`, t = 0 at `start` and 1 at
    /// `end` (STEP `LINE`).
    Line {
        /// The point at t = 0.
        start: [f64; 3],
        /// The point at t = 1.
        end: [f64; 3],
    },
    /// `origin + radius * (cos t * x + sin t * y)`, t the angle from x (STEP `CIRCLE`).
    Circle {
        /// The placement: origin, and the x and y axes.
        frame: Frame,
        /// The radius.
        radius: f64,
    },
    /// `origin + radii[0] cos t * x + radii[1] sin t * y` (STEP `ELLIPSE`).
    Ellipse {
        /// The placement: origin, and the x and y axes.
        frame: Frame,
        /// The radii along x and y.
        radii: [f64; 2],
    },
    /// Straight segments; segment i spans t in [i, i + 1] (STEP `POLYLINE`).
    Polyline {
        /// The control points.
        points: Vec<[f64; 3]>,
    },
    /// A B-spline on its knots (STEP `B_SPLINE_CURVE_WITH_KNOTS`).
    Spline {
        /// The knots, the control points' count plus the degree plus one.
        knots: Vec<f64>,
        /// The control points.
        points: Vec<[f64; 3]>,
    },
    /// A NURBS curve: homogeneous `[x*w, y*w, z*w, w]` coefficients (STEP `RATIONAL_B_SPLINE_CURVE`).
    RationalSpline {
        /// The knots, the control points' count plus the degree plus one.
        knots: Vec<f64>,
        /// The homogeneous control points.
        homogeneous: Vec<[f64; 4]>,
    },
    /// The intersection of two surfaces, traced from `leader` (a polyline or spline near it)
    /// to within `tolerance`: the procedural curve itself, exactly as its producer holds it
    /// (STEP `INTERSECTION_CURVE`). t is the leader's parameter.
    SurfaceIntersection {
        /// The two surfaces.
        surfaces: [Box<Surface>; 2],
        /// The polyline or spline the curve is traced from.
        leader: Box<Curve>,
        /// How far the traced curve may lie from both surfaces.
        tolerance: f64,
    },
}

/// Unbounded surfaces, each with its (u, v). Natural
/// normals are ∂S/∂u × ∂S/∂v.
#[derive(Clone, Debug, PartialEq)]
pub enum Surface {
    /// `origin + u * u_axis + v * v_axis` (STEP `PLANE`).
    Plane {
        /// The origin.
        origin: [f64; 3],
        /// `S_u`.
        u_axis: [f64; 3],
        /// `S_v`.
        v_axis: [f64; 3],
    },
    /// `origin + u * (u_point - origin) + v * (v_point - origin)`: the plane through three points,
    /// as its producer holds them (axes worked out from them would round).
    PlaneThrough {
        /// The origin.
        origin: [f64; 3],
        /// The point at (1, 0).
        u_point: [f64; 3],
        /// The point at (0, 1).
        v_point: [f64; 3],
    },
    /// `origin + v z + radius (cos u x + sin u y)`: u the angle, v the height (STEP
    /// `CYLINDRICAL_SURFACE`).
    Cylinder {
        /// The placement: origin, and the x and y axes.
        frame: Frame,
        /// The radius.
        radius: f64,
    },
    /// `origin + v z + r(v) (cos u x + sin u y)`, `r(v) = radius_at_origin + v tan(half_angle)`,
    /// on the side of the apex where r(v) ≥ 0: above it for a positive half angle, below it for
    /// a negative one (a cone narrowing along z, as SMLib makes them) (STEP `CONICAL_SURFACE`).
    Cone {
        /// The placement: origin, and the x and y axes.
        frame: Frame,
        /// The radius at v = 0.
        radius_at_origin: f64,
        /// The angle between the axis and the cone's lines (radians).
        half_angle: f64,
    },
    /// `origin + radius (cos v (cos u x + sin u y) + sin v z)`: u the longitude, v the latitude
    /// in [-π/2, π/2] (STEP `SPHERICAL_SURFACE`).
    Sphere {
        /// The placement: origin, and the x and y axes.
        frame: Frame,
        /// The radius.
        radius: f64,
    },
    /// `origin + (R + r cos w)(cos u x + sin u y) + r sin w z`, `w = v + tube_start`: u the
    /// longitude, v the meridian angle measured from `tube_start` (radians; 0, the outer equator,
    /// in STEP `TOROIDAL_SURFACE` and older records). The tube start is the tube's own reference,
    /// as the frame's x axis is the longitude's: a face's form starts its tube on a quarter line
    /// from there (`form`).
    Torus {
        /// The placement: origin, and the x and y axes.
        frame: Frame,
        /// The ring's radius.
        major_radius: f64,
        /// The tube's radius.
        minor_radius: f64,
        /// The tube angle at v = 0 (radians).
        tube_start: f64,
    },
    /// A rational patch on the sphere of `center` and `radius`: the corner of three equal rounds as
    /// SMLib holds it (`SmBallPatch`). It has no (u, v) of its own: the face's `nurbs` is the patch,
    /// and this says it lies on that sphere, whose offsets are exact and collapse at its centre.
    BallPatch {
        /// The sphere's centre.
        center: [f64; 3],
        /// The radius.
        radius: f64,
    },
    /// A tensor-product B-spline: `points[i][j]`, i along u (STEP `B_SPLINE_SURFACE_WITH_KNOTS`).
    Spline {
        /// The knots along u.
        u_knots: Vec<f64>,
        /// The knots along v.
        v_knots: Vec<f64>,
        /// The control points.
        points: Vec<Vec<[f64; 3]>>,
    },
    /// A NURBS surface: homogeneous `[x*w, y*w, z*w, w]` coefficients.
    RationalSpline {
        /// The knots along u.
        u_knots: Vec<f64>,
        /// The knots along v.
        v_knots: Vec<f64>,
        /// The homogeneous control points.
        homogeneous: Vec<Vec<[f64; 4]>>,
    },
    /// `profile` revolved about `origin` and unit `axis` (right-hand rule), then mapped by the
    /// column-major affine `matrix`: the revolution exactly as its producer holds it, nothing
    /// baked into the profile. When `forward`,
    /// u is the profile's parameter and v the angle in [0, 2π); otherwise u and v swap (u the
    /// angle, v the profile's), which reverses the natural normal.
    PlacedRevolution {
        /// The profile curve.
        profile: Box<Curve>,
        /// The origin.
        origin: [f64; 3],
        /// The unit axis.
        axis: [f64; 3],
        /// The affine map, column-major (`matrix[column][row]`).
        matrix: [[f64; 4]; 4],
        /// Whether u is the profile's parameter and v the angle.
        forward: bool,
    },
}

/// A pcurve: a 2D B-spline in a surface's (u, v).
#[derive(Clone, Debug, PartialEq)]
pub enum Curve2 {
    /// A B-spline on its knots.
    Spline {
        /// The knots, `points.len() + degree + 1` of them.
        knots: Vec<f64>,
        /// The control points.
        points: Vec<[f64; 2]>,
    },
    /// Homogeneous `[u*w, v*w, w]` coefficients.
    RationalSpline {
        /// The knots, `homogeneous.len() + degree + 1` of them.
        knots: Vec<f64>,
        /// The homogeneous control points.
        homogeneous: Vec<[f64; 3]>,
    },
}

impl Curve2 {
    /// The active parameter interval `[knots[degree], knots[count]]`.
    #[must_use]
    pub fn interval(&self) -> Option<[f64; 2]> {
        let (k, count) = match self {
            Curve2::Spline { knots, points } => (knots, points.len()),
            Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
        };
        let degree = k.len().checked_sub(count.checked_add(1)?)?;
        Some([*k.get(degree)?, *k.get(count)?])
    }

    /// The point at `t` (`None` outside the knot range or for malformed data).
    #[must_use]
    pub fn eval(&self, t: f64) -> Option<[f64; 2]> {
        let c = nl_curve2(self)?;
        if !t.is_finite()
            || !basis_check(
                c.degree(),
                c.knots().len().checked_sub(c.degree() + 1)?,
                c.knots(),
            )
        {
            return None;
        }
        let p = c.eval(t, Side::Left).ok()?;
        (p.x.is_finite() && p.y.is_finite()).then_some([p.x, p.y])
    }
}
