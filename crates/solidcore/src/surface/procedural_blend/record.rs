//! A procedural blend as plain data (#2388): everything its definition S needs, and its
//! certified net B, as serde records. Data only: nothing here evaluates, measures or
//! compares geometry, and nothing depends on the kernel's types, so the module moves into
//! `solidcore-record` unchanged (`docs/layers.md`). The kernel's side, the exact rebuild and
//! the net's re-certification, is [`super::convert`] and
//! [`crate::surface::procedural_blend::ProceduralBlend::recertify`].
//!
//! Not upstream. Its numbers are the kernel's stored values, bit for bit: a NURBS keeps
//! its control points as stored (weighted when rational) and its full knot vectors, so a
//! record written, read back and rebuilt evaluates as the blend it was written from.
//!
//! Deviations: none (no upstream counterpart). Recursion: [`ProceduralBlendRecord::validate`]
//! walks nested supports at most [`RECORD_MAX_DEPTH`] deep and refuses deeper records
//! before descending.

use serde::{Deserialize, Serialize};

/// The most definitions a record nests, the kernel's `SM_BLEND_MAX_DEPTH` (a round on a
/// round on a round).
pub const RECORD_MAX_DEPTH: u32 = 3;

/// A NURBS curve of a definition (the leader, a seed, a radius law), as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NurbsCurveRecord {
    /// The curve's image dimension (`m_lDim`): 2 or 3.
    pub dim: u32,
    /// Whether the control points carry a z (the NURB's own dimension).
    pub points_3d: bool,
    /// The degree p.
    pub degree: u32,
    /// The full knot vector, `points.len() + degree + 1` knots.
    pub knots: Vec<f64>,
    /// The control points as stored: `(w x, w y, w z)` when rational, else `(x, y, z)`;
    /// z is 0 when `points_3d` is false.
    pub points: Vec<[f64; 3]>,
    /// The weights, one per control point, when the curve is rational.
    pub weights: Option<Vec<f64>>,
}

/// A NURBS surface (a support, or the net B), as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NurbsSurfaceRecord {
    /// The degree in u.
    pub u_degree: u32,
    /// The degree in v.
    pub v_degree: u32,
    /// The number of control points along u.
    pub u_count: u32,
    /// The number of control points along v.
    pub v_count: u32,
    /// The full u knot vector, `u_count + u_degree + 1` knots.
    pub u_knots: Vec<f64>,
    /// The full v knot vector, `v_count + v_degree + 1` knots.
    pub v_knots: Vec<f64>,
    /// The control points u-major (`points[i * v_count + j]`), as stored: weighted when
    /// rational.
    pub points: Vec<[f64; 3]>,
    /// The weights, one per control point, when the surface is rational.
    pub weights: Option<Vec<f64>>,
}

/// The analytic kind of a support, a label beside its NURBS form, which is the support's
/// one geometric truth: the kernel derives the analytic surface from the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportKindRecord {
    /// No analytic kind: a B-spline surface (or a kind the record keeps as its form only).
    Free,
    /// A plane.
    Plane,
    /// A cylinder.
    Cylinder,
    /// A cone.
    Cone,
    /// A sphere.
    Sphere,
    /// A torus.
    Torus,
}

/// A surface the ball rolls on: a NURBS form (what the stations are solved on) with its
/// analytic kind, or itself a blend by its definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SupportRecord {
    /// A surface by its NURBS form, and its analytic kind.
    Nurbs {
        /// The analytic kind ([`SupportKindRecord::Free`]: none).
        kind: SupportKindRecord,
        /// The NURBS form.
        form: NurbsSurfaceRecord,
    },
    /// A procedural blend (nested, at most [`RECORD_MAX_DEPTH`] deep in all).
    Blend(Box<ProceduralBlendRecord>),
}

/// The ball's radius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RadiusRecord {
    /// A ball of constant offsets from the two supports.
    Constant {
        /// The offsets from S1 and S2.
        offsets: [f64; 2],
    },
    /// A ball whose radius is the x of a curve over the leader's parameter.
    Law(NurbsCurveRecord),
}

/// How a chamfer's size was asked for (#2486).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ChamferSizeRecord {
    /// The setbacks from the edge along supports 1 and 2.
    Distances([f64; 2]),
    /// The setback along support 1 and the chamfer's angle (radians) to it.
    DistanceAngle {
        /// The setback along support 1.
        distance: f64,
        /// The chamfer's angle to support 1.
        angle: f64,
    },
    /// The chord between the rails.
    Chord(f64),
}

/// A chamfer's sizes and the wedge angle they were converted to offsets at (#2486).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChamferRecord {
    /// The sizes as asked.
    pub size: ChamferSizeRecord,
    /// The angle between the supports the sizes were converted at.
    pub wedge: f64,
}

/// The affine map from the net's (u, v) to the definition's (s, t):
/// `(s, t) = (a[0] + b[0] p, a[1] + b[1] q)`, `(p, q)` = `(u, v)`, or `(v, u)` when swapped.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ParameterMapRecord {
    /// The offsets a.
    pub a: [f64; 2],
    /// The scales b.
    pub b: [f64; 2],
    /// Whether `(p, q)` is `(v, u)`.
    pub swapped: bool,
}

/// The definition S of a rolling-ball blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlendDefinitionRecord {
    /// The supports S1, S2, by value.
    pub supports: [SupportRecord; 2],
    /// The ball's side of each support, +1 or -1 along its normal.
    pub sides: [f64; 2],
    /// The radius: constant offsets, or a law.
    pub radius: RadiusRecord,
    /// The section's order n: 0 the chord (a chamfer), 1 the ball's circle, 2 to 4 a G^n
    /// section.
    pub order: u32,
    /// The thumbweight w for n >= 2 (the equal area rule's marker, or a value below 1);
    /// 0 for n = 1.
    pub thumbweight: f64,
    /// The leader (centre line) the stations are solved in; it fixes s.
    pub leader: NurbsCurveRecord,
    /// The seeds: each foot's (u, v) on its support over s, the stations' Newton starts.
    pub seeds: [NurbsCurveRecord; 2],
    /// The certified bound ε: the net is within ε of S.
    pub bound: f64,
    /// The map from the net's parameters to the definition's.
    pub map: ParameterMapRecord,
    /// A chamfer's sizes (order 0): what its offsets were asked as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer: Option<ChamferRecord>,
}

/// A procedural blend: its definition S and its certified net B.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProceduralBlendRecord {
    /// The definition S.
    pub definition: BlendDefinitionRecord,
    /// The net B, within the definition's bound of S.
    pub net: NurbsSurfaceRecord,
}

/// What a record's structural check refused: the field and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordError {
    /// The field, as a path (`definition.leader.knots`).
    pub field: &'static str,
    /// What is wrong with it.
    pub reason: &'static str,
}

impl core::fmt::Display for RecordError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

impl std::error::Error for RecordError {}

const fn refuse(field: &'static str, reason: &'static str) -> RecordError {
    RecordError { field, reason }
}

fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn points_finite(points: &[[f64; 3]]) -> bool {
    points.iter().all(|p| all_finite(p))
}

/// `count + degree + 1`, or `None` past `usize`.
fn knot_count(count: usize, degree: u32) -> Option<usize> {
    let degree = usize::try_from(degree).ok()?;
    count.checked_add(degree)?.checked_add(1)
}

impl NurbsCurveRecord {
    /// The structural check: a degree of at least 1, `points + degree + 1` knots, a weight
    /// per point when rational, a dimension of 2 or 3 (a 3D curve has 3D points), and
    /// every number finite.
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met, naming `field`.
    pub fn validate(&self, field: &'static str) -> Result<(), RecordError> {
        if !(self.dim == 2 || self.dim == 3) || (self.dim == 3 && !self.points_3d) {
            return Err(refuse(field, "a curve's dimension is 2 or 3"));
        }
        if self.degree < 1 || self.points.len() <= self.degree as usize {
            return Err(refuse(
                field,
                "a curve has degree 1 or more and degree + 1 points",
            ));
        }
        if knot_count(self.points.len(), self.degree) != Some(self.knots.len()) {
            return Err(refuse(field, "a curve has points + degree + 1 knots"));
        }
        if let Some(w) = &self.weights {
            if w.len() != self.points.len() || !all_finite(w) {
                return Err(refuse(
                    field,
                    "a rational curve has a finite weight per point",
                ));
            }
        }
        if !all_finite(&self.knots) || !points_finite(&self.points) {
            return Err(refuse(field, "a curve's numbers are finite"));
        }
        Ok(())
    }
}

impl NurbsSurfaceRecord {
    /// The structural check: degrees of at least 1, `u_count * v_count` points, each
    /// direction's `count + degree + 1` knots, a weight per point when rational, and every
    /// number finite.
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met, naming `field`.
    pub fn validate(&self, field: &'static str) -> Result<(), RecordError> {
        let (nu, nv) = (self.u_count as usize, self.v_count as usize);
        if self.u_degree < 1
            || self.v_degree < 1
            || nu <= self.u_degree as usize
            || nv <= self.v_degree as usize
        {
            return Err(refuse(
                field,
                "a surface has degree 1 or more and degree + 1 points each way",
            ));
        }
        if nu.checked_mul(nv) != Some(self.points.len()) {
            return Err(refuse(field, "a surface has u_count * v_count points"));
        }
        if knot_count(nu, self.u_degree) != Some(self.u_knots.len())
            || knot_count(nv, self.v_degree) != Some(self.v_knots.len())
        {
            return Err(refuse(
                field,
                "a surface has count + degree + 1 knots each way",
            ));
        }
        if let Some(w) = &self.weights {
            if w.len() != self.points.len() || !all_finite(w) {
                return Err(refuse(
                    field,
                    "a rational surface has a finite weight per point",
                ));
            }
        }
        if !all_finite(&self.u_knots) || !all_finite(&self.v_knots) || !points_finite(&self.points)
        {
            return Err(refuse(field, "a surface's numbers are finite"));
        }
        Ok(())
    }
}

impl ProceduralBlendRecord {
    /// The structural check of the whole record, nested supports included: every curve
    /// and surface as their own checks, the definition's numbers finite, and supports
    /// nested at most [`RECORD_MAX_DEPTH`] deep. The kernel's rebuild checks what the
    /// numbers mean (orders, sides, radii, the net against S).
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met.
    pub fn validate(&self) -> Result<(), RecordError> {
        self.validate_at(1)
    }

    /// [`Self::validate`] of a record `depth` definitions deep (1 at the top).
    fn validate_at(&self, depth: u32) -> Result<(), RecordError> {
        if depth > RECORD_MAX_DEPTH {
            return Err(refuse(
                "definition.supports",
                "blends nest at most 3 definitions deep",
            ));
        }
        let d = &self.definition;
        for support in &d.supports {
            match support {
                SupportRecord::Nurbs { form, .. } => form.validate("definition.supports")?,
                SupportRecord::Blend(b) => b.validate_at(depth + 1)?,
            }
        }
        let numbers = [
            d.sides[0],
            d.sides[1],
            d.thumbweight,
            d.bound,
            d.map.a[0],
            d.map.a[1],
            d.map.b[0],
            d.map.b[1],
        ];
        if !all_finite(&numbers) {
            return Err(refuse("definition", "a definition's numbers are finite"));
        }
        if let Some(c) = &d.chamfer {
            let sizes = match c.size {
                ChamferSizeRecord::Distances([a, b]) => [a, b, c.wedge],
                ChamferSizeRecord::DistanceAngle { distance, angle } => [distance, angle, c.wedge],
                ChamferSizeRecord::Chord(l) => [l, l, c.wedge],
            };
            if !all_finite(&sizes) {
                return Err(refuse(
                    "definition.chamfer",
                    "a definition's numbers are finite",
                ));
            }
        }
        match &d.radius {
            RadiusRecord::Constant { offsets } => {
                if !all_finite(offsets) {
                    return Err(refuse(
                        "definition.radius",
                        "a definition's numbers are finite",
                    ));
                }
            }
            RadiusRecord::Law(law) => law.validate("definition.radius")?,
        }
        d.leader.validate("definition.leader")?;
        for seed in &d.seeds {
            seed.validate("definition.seeds")?;
        }
        self.net.validate("net")
    }

    /// How deep definitions nest in this record: 1, or one more than its deepest blend
    /// support (bounded by [`RECORD_MAX_DEPTH`] + 1: it stops descending past that).
    #[must_use]
    pub fn depth(&self) -> u32 {
        self.depth_at(1)
    }

    fn depth_at(&self, depth: u32) -> u32 {
        if depth > RECORD_MAX_DEPTH {
            return depth;
        }
        let mut deepest = depth;
        for support in &self.definition.supports {
            if let SupportRecord::Blend(b) = support {
                deepest = deepest.max(b.depth_at(depth + 1));
            }
        }
        deepest
    }
}

/// The exact class of a round whose net is its definition (#2485).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExactClassRecord {
    /// A straight spine: the cylinder about it.
    Line,
    /// A circle about a common axis: the section turned about it.
    Circle,
    /// One point: a piece of the ball.
    Point,
    /// A chamfer between planes (#2486): the plane through the feet of its line spine.
    Chord,
    /// A conic spine.
    Conic,
    /// One of a pair of conics.
    ConicPair,
    /// A nodal quartic spine.
    NodalQuartic,
}

/// The analytic kind of an exact net, a label beside its NURBS form as a support's is
/// ([`SupportKindRecord`]): the kernel derives the analytic surface from the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExactNetKindRecord {
    /// A plane.
    Plane,
    /// A cylinder.
    Cylinder,
    /// A cone.
    Cone,
    /// A sphere.
    Sphere,
    /// A torus.
    Torus,
    /// A surface of revolution: its first section turned about the axis of its circles.
    Revolution,
    /// A ball patch: a rational patch on its sphere.
    BallPatch,
    /// A B-spline surface, the form itself (the rational classes' exact surfaces).
    BSpline,
}

/// An exact net: its analytic kind and its NURBS form, as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactNetRecord {
    /// The analytic kind.
    pub kind: ExactNetKindRecord,
    /// The NURBS form.
    pub form: NurbsSurfaceRecord,
}

/// What an exact round's class is read with beside its supports.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClassInputsRecord {
    /// The `(u, v)` each support's quadric is read at.
    pub uv: [[f64; 2]; 2],
    /// The signed offset of the ball's centre from each support along its normal.
    pub offsets: [f64; 2],
    /// The ball's centre the spine is classified at.
    pub centre: [f64; 3],
    /// The tolerance placements are decided within.
    pub tol: f64,
    /// Whether a collapsed offset crossed by the other is the ball's piece.
    pub ball_on_collapsed: bool,
    /// Whether the radii can change along the edge.
    pub radii_can_change: bool,
}

/// A round of an exact class (#2485): its definition (supports, ball, section, leader and
/// what its class is read with) and its net, the exact surface, at a bound of 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactRoundRecord {
    /// The class.
    pub class: ExactClassRecord,
    /// The supports S1, S2, by value.
    pub supports: [SupportRecord; 2],
    /// The ball's radius (0 for the chord).
    pub radius: f64,
    /// The section's order: 1, the ball's circle; 0, the chord.
    pub order: u32,
    /// The leader (the centre line), where the class's construction makes one.
    pub leader: Option<NurbsCurveRecord>,
    /// What the class is read with beside the supports.
    pub inputs: ClassInputsRecord,
    /// A chamfer's sizes (the chord, #2486).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer: Option<ChamferRecord>,
    /// How the net's parameter runs along the leader: `a + b w`, `w` its u (or v).
    pub leader_map: LeaderMapRecord,
    /// The net, the exact surface.
    pub net: ExactNetRecord,
}

/// How an exact round's net runs along its leader: the leader's parameter is `a + b w`,
/// `w` the net's u, or its v when `along_v`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LeaderMapRecord {
    /// The offset a.
    pub a: f64,
    /// The scale b.
    pub b: f64,
    /// Whether the leader runs along the net's v.
    pub along_v: bool,
}

impl ExactRoundRecord {
    /// The structural check: every curve and surface as their own checks, the supports
    /// NURBS forms (an exact round's faces are never blends), its numbers finite, an
    /// order of 1 and a positive radius.
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met.
    pub fn validate(&self) -> Result<(), RecordError> {
        for support in &self.supports {
            match support {
                SupportRecord::Nurbs { form, .. } => form.validate("supports")?,
                SupportRecord::Blend(_) => {
                    return Err(refuse("supports", "an exact round's supports are faces"))
                }
            }
        }
        let i = &self.inputs;
        let numbers = [
            self.radius,
            i.uv[0][0],
            i.uv[0][1],
            i.uv[1][0],
            i.uv[1][1],
            i.offsets[0],
            i.offsets[1],
            i.centre[0],
            i.centre[1],
            i.centre[2],
            i.tol,
            self.leader_map.a,
            self.leader_map.b,
        ];
        if !all_finite(&numbers) {
            return Err(refuse("inputs", "an exact round's numbers are finite"));
        }
        if self.leader_map.b == 0.0 {
            return Err(refuse("leader_map", "the leader's map is singular"));
        }
        // the chord (#2486): order 0, no ball, its sizes; a round: the ball's circle
        if self.class == ExactClassRecord::Chord {
            let Some(c) = &self.chamfer else {
                return Err(refuse("chamfer", "a chord keeps its chamfer's sizes"));
            };
            if self.order != 0 || self.radius != 0.0 {
                return Err(refuse(
                    "order",
                    "a chord's section is of order 0, with no ball",
                ));
            }
            let sizes = match c.size {
                ChamferSizeRecord::Distances([a, b]) => [a, b, c.wedge],
                ChamferSizeRecord::DistanceAngle { distance, angle } => [distance, angle, c.wedge],
                ChamferSizeRecord::Chord(l) => [l, l, c.wedge],
            };
            if !all_finite(&sizes) {
                return Err(refuse("chamfer", "an exact round's numbers are finite"));
            }
        } else {
            if self.radius <= 0.0 {
                return Err(refuse("radius", "the ball's radius is positive"));
            }
            if self.order != 1 || self.chamfer.is_some() {
                return Err(refuse(
                    "order",
                    "an exact round's section is the ball's circle",
                ));
            }
        }
        if let Some(leader) = &self.leader {
            leader.validate("leader")?;
        }
        self.net.form.validate("net.form")
    }
}

/// A face of a blend corner's (#2488): its corner kind's definition and its net.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CornerRecord {
    /// A Swept turn's torus.
    Swept(SweptCornerRecord),
    /// An End's patch on its support face.
    End(EndCornerRecord),
    /// A Setback's fill patch.
    Setback(SetbackCornerRecord),
    /// A Mixed corner's face.
    Mixed(Box<MixedCornerRecord>),
}

/// A Swept turn (#2488): the end section of its two rounds revolved about the side edge,
/// its net the exact torus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweptCornerRecord {
    /// The face both rounds meet, a plane.
    pub common: SupportRecord,
    /// A point of the side edge's line.
    pub axis_point: [f64; 3],
    /// The side edge's unit direction.
    pub axis: [f64; 3],
    /// The two rounds' balls' centres at the side vertex.
    pub centres: [[f64; 3]; 2],
    /// The balls' radius.
    pub radius: f64,
    /// The tolerance the construction decided the turn within.
    pub tol: f64,
    /// The net, the torus: its label and NURBS form.
    pub net: SupportRecord,
}

/// An End's patch (#2488): its support face's surface and the patch's boundary, its net
/// the support's surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndCornerRecord {
    /// The support face's surface.
    pub support: SupportRecord,
    /// The patch's loop's curves.
    pub boundary: Vec<NurbsCurveRecord>,
    /// The net: its label and NURBS form.
    pub net: SupportRecord,
}

/// A face at a Setback: its plane, its normal out of the material; or, beside a curved face
/// (#2343), the natural quadric it is (`quadric`), `point` then its axis point (a cone's
/// apex) or centre and `normal` its unit normal out of the material at the corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaneRecord {
    /// A point of the plane; a quadric's axis point or centre.
    pub point: [f64; 3],
    /// Its unit normal (a quadric's at the corner).
    pub normal: [f64; 3],
    /// The quadric, where the face is no plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quadric: Option<QuadricRecord>,
}

/// What a curved face at a Setback is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuadricKindRecord {
    /// A circular cylinder.
    Cylinder,
    /// A circular cone.
    Cone,
    /// A sphere.
    Sphere,
}

/// A curved face at a Setback as its fill reads it (#2343): a cylinder or cone about the axis
/// through its record's point, of `radius` there and `radius + slope h` at `h` along the
/// axis, or a sphere about the point of `radius`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QuadricRecord {
    /// What it is.
    pub kind: QuadricKindRecord,
    /// A cylinder's or cone's unit axis.
    pub axis: [f64; 3],
    /// Its radius at the point (a cone's at its apex, 0).
    pub radius: f64,
    /// A cone's change of radius along its axis (a cylinder's and sphere's 0).
    pub slope: f64,
    /// `+1` where the material is inside it, `-1` outside.
    pub side: f64,
}

/// A round at a Setback as its fill reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetbackRoundRecord {
    /// Its surface: a NURBS form with its label, or a procedural blend.
    pub surface: SupportRecord,
    /// Whether its rails run along u (else along v).
    pub along_u: bool,
    /// The rail parameter of its end section.
    pub end: f64,
    /// Whether the corner lies at rail parameters above `end`.
    pub corner_above: bool,
}

/// A Setback's fill patch (#2488): the fill's definition (the faces' planes, the rounds at
/// their end sections, the rule's order and tolerance, the corner's convexity and its
/// certified containment), the patch's index in the fill, and its net.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetbackCornerRecord {
    /// The faces' planes.
    pub faces: Vec<PlaneRecord>,
    /// The rounds, one per face.
    pub rounds: Vec<SetbackRoundRecord>,
    /// The fill's order: 1 (G1) or 2 (G2).
    pub order: u32,
    /// The length the faces and the rounds' ends are matched to.
    pub tol: f64,
    /// Whether the corner's edges are convex.
    pub convex: bool,
    /// The most the fill stands past a face, certified (at most `tol`).
    pub containment: f64,
    /// The patch's index: setback patches first, then the interior parts.
    pub patch: u32,
    /// The net, the patch.
    pub net: NurbsSurfaceRecord,
}

/// A piecewise Bezier curve in R^4 (homogeneous), segment s on `[brk[s], brk[s + 1]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BezierCurveRecord {
    /// The breaks.
    pub brk: Vec<f64>,
    /// The segments' control points, all of one degree.
    pub seg: Vec<Vec<[f64; 4]>>,
}

/// A side of a Mixed face and its ribbon, as the Coons rule reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixedSideRecord {
    /// The side.
    pub c: BezierCurveRecord,
    /// Its transversal direction field.
    pub dir: BezierCurveRecord,
    /// Whether the field is the one shared along X-V.
    pub shared: bool,
    /// alpha, beta, gamma at its ends.
    pub val: [[f64; 3]; 2],
    /// Their derivatives.
    pub der: [[f64; 3]; 2],
    /// The field at its ends.
    pub dir_val: [[f64; 4]; 2],
    /// The field's derivative at its ends.
    pub dir_der: [[f64; 4]; 2],
    /// The derivative's part per unit of the inflection's rate at its ends.
    pub dir_k: [[f64; 4]; 2],
    /// The cross field in the side's tangent's place.
    pub t: BezierCurveRecord,
    /// Whether it has one.
    pub has_t: bool,
}

/// A Mixed corner's face (#2488): its rule (the Coons patch of its four sides, bilinear for
/// chamfers, turned or not), its supports (the two trimmed blends and T), its trimline's
/// blend, side and uv, its conic's side, the construction's tolerance and its net.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixedCornerRecord {
    /// The two blends B1, B2.
    pub blends: [SupportRecord; 2],
    /// T.
    pub common: SupportRecord,
    /// The four sides c0, c1, d0, d1.
    pub sides: Vec<MixedSideRecord>,
    /// Whether the rule is the bilinear Coons patch.
    pub bilinear: bool,
    /// Whether the patch is turned.
    pub transposed: bool,
    /// The trimline's blend (0 or 1).
    pub blend: u32,
    /// The trimline's side.
    pub trim_side: u32,
    /// The trimline's uv on its blend.
    pub trim_uv: BezierCurveRecord,
    /// The conic's side.
    pub conic_side: u32,
    /// The construction's tolerance.
    pub tol: f64,
    /// The net.
    pub net: NurbsSurfaceRecord,
}

impl BezierCurveRecord {
    fn validate(&self, field: &'static str) -> Result<(), RecordError> {
        // (an empty curve is a field the rule does not read)
        if !(self.brk.is_empty() && self.seg.is_empty()) && self.brk.len() != self.seg.len() + 1 {
            return Err(refuse(field, "a curve has one break more than segments"));
        }
        let degree = self.seg.first().map_or(0, Vec::len);
        if self.seg.iter().any(|s| s.len() != degree)
            || !all_finite(&self.brk)
            || self.seg.iter().flatten().any(|q| !all_finite(q))
        {
            return Err(refuse(
                field,
                "a curve's segments are of one degree and finite",
            ));
        }
        Ok(())
    }
}

/// A support or net of a corner's: a NURBS form (corners' faces are not blends here).
fn corner_surface(s: &SupportRecord, field: &'static str) -> Result<(), RecordError> {
    match s {
        SupportRecord::Nurbs { form, .. } => form.validate(field),
        SupportRecord::Blend(_) => Err(refuse(field, "a corner's face is a NURBS form")),
    }
}

impl CornerRecord {
    /// The structural check: every curve and surface as their own checks, its numbers
    /// finite, a positive radius and tolerance, a torus label on a Swept turn's net, a
    /// bounded End.
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met.
    pub fn validate(&self) -> Result<(), RecordError> {
        match self {
            Self::Swept(r) => {
                corner_surface(&r.common, "common")?;
                corner_surface(&r.net, "net")?;
                if !matches!(
                    r.net,
                    SupportRecord::Nurbs {
                        kind: SupportKindRecord::Torus,
                        ..
                    }
                ) {
                    return Err(refuse("net", "a swept turn's net is a torus"));
                }
                let [c0, c1] = r.centres;
                if !points_finite(&[r.axis_point, r.axis, c0, c1])
                    || !all_finite(&[r.radius, r.tol])
                {
                    return Err(refuse("swept", "a swept turn's numbers are finite"));
                }
                if !(r.radius > 0.0 && r.tol > 0.0) {
                    return Err(refuse(
                        "radius",
                        "a swept turn's radius and tolerance are positive",
                    ));
                }
                Ok(())
            }
            Self::End(r) => {
                corner_surface(&r.support, "support")?;
                corner_surface(&r.net, "net")?;
                if r.boundary.is_empty() {
                    return Err(refuse("boundary", "an end's patch is bounded"));
                }
                for c in &r.boundary {
                    c.validate("boundary")?;
                }
                Ok(())
            }
            Self::Setback(r) => {
                let n = r.rounds.len();
                if n < 3 || r.faces.len() != n {
                    return Err(refuse(
                        "rounds",
                        "a setback's fill has a round per face, three or more",
                    ));
                }
                for round in &r.rounds {
                    match &round.surface {
                        SupportRecord::Nurbs { form, .. } => form.validate("rounds")?,
                        SupportRecord::Blend(b) => b.validate()?,
                    }
                    if !round.end.is_finite() {
                        return Err(refuse("rounds", "a round's end is finite"));
                    }
                }
                for f in &r.faces {
                    if !points_finite(&[f.point, f.normal]) {
                        return Err(refuse("faces", "a face's plane is finite"));
                    }
                    if let Some(q) = &f.quadric {
                        let radius_ok = match q.kind {
                            QuadricKindRecord::Cone => q.radius >= 0.0 && q.slope > 0.0,
                            QuadricKindRecord::Cylinder | QuadricKindRecord::Sphere => {
                                q.radius > 0.0 && q.slope == 0.0
                            }
                        };
                        let valid = points_finite(&[q.axis])
                            && q.radius.is_finite()
                            && q.slope.is_finite()
                            && radius_ok
                            && (q.side == 1.0 || q.side == -1.0);
                        if !valid {
                            return Err(refuse(
                                "faces",
                                "a curved face is a cylinder or sphere of positive radius or a cone opening along its axis, with a side of +1 or -1",
                            ));
                        }
                    }
                }
                if !(r.order == 1 || r.order == 2) {
                    return Err(refuse("order", "a setback's fill is G1 or G2"));
                }
                if !(r.tol.is_finite() && r.tol > 0.0 && r.containment.is_finite()) {
                    return Err(refuse(
                        "tol",
                        "a setback's tolerance is finite and positive",
                    ));
                }
                if (r.patch as usize) >= 2 * n {
                    return Err(refuse(
                        "patch",
                        "a setback's fill has two patches per round",
                    ));
                }
                r.net.validate("net")
            }
            Self::Mixed(r) => {
                for b in &r.blends {
                    match b {
                        SupportRecord::Nurbs { form, .. } => form.validate("blends")?,
                        SupportRecord::Blend(b) => b.validate()?,
                    }
                }
                corner_surface(&r.common, "common")?;
                if r.sides.len() != 4 || r.blend > 1 || r.trim_side > 3 || r.conic_side > 3 {
                    return Err(refuse("sides", "a mixed face has four sides on two blends"));
                }
                for side in &r.sides {
                    side.c.validate("sides")?;
                    side.dir.validate("sides")?;
                    side.t.validate("sides")?;
                    let numbers = side.val.iter().chain(&side.der).flatten();
                    let fields = side
                        .dir_val
                        .iter()
                        .chain(&side.dir_der)
                        .chain(&side.dir_k)
                        .flatten();
                    if !numbers.chain(fields).all(|x| x.is_finite()) {
                        return Err(refuse("sides", "a side's numbers are finite"));
                    }
                }
                r.trim_uv.validate("trim_uv")?;
                if !(r.tol.is_finite() && r.tol > 0.0) {
                    return Err(refuse(
                        "tol",
                        "a mixed face's tolerance is finite and positive",
                    ));
                }
                r.net.validate("net")
            }
        }
    }
}
