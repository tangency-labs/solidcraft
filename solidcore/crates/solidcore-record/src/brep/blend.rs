//! A blend by its definition, as plain data: what a face's [`Surface::Blend`](super::Surface)
//! holds. A procedural blend's definition S (docs/definitions-and-certificates.md: its supports,
//! ball, section, leader and seeds, its certified bound ε and the map from the net's parameters to
//! S's), or a round of an exact class's (its supports, ball, section, leader and what its class is
//! read with). The face's `nurbs` is the net B, certified within ε of S (an exact round's net is S,
//! ε = 0), stored once there; a support that is itself a blend is no face, and keeps its whole
//! record, net included.
//!
//! The kernel holds the same records as its values (`surface::procedural_blend::record`), and
//! `solidcore-convert` copies the fields from one to the other; the kernel's rebuild makes the
//! definition exactly and certifies the net again before it is an operand. Nothing here
//! evaluates, measures or compares geometry: the checks are structural ([`BlendDefinition::validate`]).
//!
//! Recursion: [`BlendDefinitionRecord::validate`] walks nested supports at most
//! [`RECORD_MAX_DEPTH`] deep and refuses deeper records before descending.

use serde::{Deserialize, Serialize};

/// The most definitions a record nests (a round on a round on a round).
pub const RECORD_MAX_DEPTH: u32 = 3;

/// A face's blend definition: a procedural blend's, or a round of an exact class's. The face's
/// `nurbs` is its net.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendDefinition {
    /// A procedural blend: its net within the definition's bound of S.
    Procedural(Box<BlendDefinitionRecord>),
    /// A round of an exact class: its net is S.
    ExactRound(Box<ExactRoundDefinition>),
}

/// A NURBS curve of a definition (the leader, a seed, a radius law), as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NurbsCurveRecord {
    /// The curve's image dimension: 2 or 3.
    pub dim: u32,
    /// Whether the control points carry a z.
    pub points_3d: bool,
    /// The degree p.
    pub degree: u32,
    /// The full knot vector, `points.len() + degree + 1` knots.
    pub knots: Vec<f64>,
    /// The control points as stored: `(w x, w y, w z)` when rational, else `(x, y, z)`; z is 0
    /// when `points_3d` is false.
    pub points: Vec<[f64; 3]>,
    /// The weights, one per control point, when the curve is rational.
    pub weights: Option<Vec<f64>>,
}

/// A NURBS surface (a support, or a nested blend's net), as stored.
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
    /// The control points u-major (`points[i * v_count + j]`), as stored: weighted when rational.
    pub points: Vec<[f64; 3]>,
    /// The weights, one per control point, when the surface is rational.
    pub weights: Option<Vec<f64>>,
}

/// The analytic kind of a support, a label beside its NURBS form, which is the support's one
/// geometric truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportKindRecord {
    /// No analytic kind: a B-spline surface.
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

/// A surface the ball rolls on: a NURBS form with its analytic kind, or itself a blend by its
/// definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SupportRecord {
    /// A surface by its NURBS form, and its analytic kind.
    Nurbs {
        /// The analytic kind ([`SupportKindRecord::Free`]: none).
        kind: SupportKindRecord,
        /// The NURBS form.
        form: NurbsSurfaceRecord,
    },
    /// A procedural blend, definition and net (nested, at most [`RECORD_MAX_DEPTH`] deep in all).
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

/// How a chamfer's size was asked for.
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

/// A chamfer's sizes and the wedge angle they were converted to offsets at.
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
    /// The section's order n: 0 the chord (a chamfer), 1 the ball's circle, 2 to 4 a G^n section.
    pub order: u32,
    /// The thumbweight w for n >= 2; 0 for n = 1.
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

/// A procedural blend: its definition S and its certified net B. A face's blend keeps its net as
/// the face's `nurbs`; this whole record is a nested support's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProceduralBlendRecord {
    /// The definition S.
    pub definition: BlendDefinitionRecord,
    /// The net B, within the definition's bound of S.
    pub net: NurbsSurfaceRecord,
}

/// The exact class of a round whose net is its definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExactClassRecord {
    /// A straight spine: the cylinder about it.
    Line,
    /// A circle about a common axis: the section turned about it.
    Circle,
    /// One point: a piece of the ball.
    Point,
    /// A chamfer between planes: the plane through the feet of its line spine.
    Chord,
    /// A conic spine.
    Conic,
    /// One of a pair of conics.
    ConicPair,
    /// A nodal quartic spine.
    NodalQuartic,
}

/// The analytic kind of an exact net, a label beside its NURBS form (the face's `nurbs`).
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

/// How an exact round's net runs along its leader: the leader's parameter is `a + b w`, `w` the
/// net's u, or its v when `along_v`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LeaderMapRecord {
    /// The offset a.
    pub a: f64,
    /// The scale b.
    pub b: f64,
    /// Whether the leader runs along the net's v.
    pub along_v: bool,
}

/// A round of an exact class by its definition (supports, ball, section, leader and what its
/// class is read with) and its net's analytic kind: the kernel's `ExactRoundRecord` without the
/// net's form, which is the face's `nurbs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactRoundDefinition {
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
    /// A chamfer's sizes (the chord).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer: Option<ChamferRecord>,
    /// How the net's parameter runs along the leader.
    pub leader_map: LeaderMapRecord,
    /// The net's analytic kind.
    pub net_kind: ExactNetKindRecord,
}

/// What a definition's structural check refused: the field and why.
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
    /// The structural check: a degree of at least 1, `points + degree + 1` knots, a weight per
    /// point when rational, a dimension of 2 or 3 (a 3D curve has 3D points), and every number
    /// finite.
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
        if let Some(w) = &self.weights
            && (w.len() != self.points.len() || !all_finite(w))
        {
            return Err(refuse(
                field,
                "a rational curve has a finite weight per point",
            ));
        }
        if !all_finite(&self.knots) || !points_finite(&self.points) {
            return Err(refuse(field, "a curve's numbers are finite"));
        }
        Ok(())
    }

    /// How many numbers it holds, for the record's item budget.
    pub fn items(&self) -> usize {
        self.knots
            .len()
            .saturating_add(self.points.len())
            .saturating_add(self.weights.as_ref().map_or(0, Vec::len))
    }
}

impl NurbsSurfaceRecord {
    /// The structural check: degrees of at least 1, `u_count * v_count` points, each direction's
    /// `count + degree + 1` knots, a weight per point when rational, and every number finite.
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
        if let Some(w) = &self.weights
            && (w.len() != self.points.len() || !all_finite(w))
        {
            return Err(refuse(
                field,
                "a rational surface has a finite weight per point",
            ));
        }
        if !all_finite(&self.u_knots) || !all_finite(&self.v_knots) || !points_finite(&self.points)
        {
            return Err(refuse(field, "a surface's numbers are finite"));
        }
        Ok(())
    }

    /// How many numbers it holds, for the record's item budget.
    pub fn items(&self) -> usize {
        self.u_knots
            .len()
            .saturating_add(self.v_knots.len())
            .saturating_add(self.points.len())
            .saturating_add(self.weights.as_ref().map_or(0, Vec::len))
    }
}

impl SupportRecord {
    fn validate_at(&self, depth: u32, field: &'static str) -> Result<(), RecordError> {
        match self {
            SupportRecord::Nurbs { form, .. } => form.validate(field),
            SupportRecord::Blend(b) => b.validate_at(depth + 1),
        }
    }

    /// How many numbers it holds, nested blends included (call on a validated record: they nest
    /// at most [`RECORD_MAX_DEPTH`] deep).
    pub fn items(&self) -> usize {
        match self {
            SupportRecord::Nurbs { form, .. } => form.items(),
            SupportRecord::Blend(b) => b.definition.items().saturating_add(b.net.items()),
        }
    }
}

fn chamfer_finite(c: &ChamferRecord) -> bool {
    let sizes = match c.size {
        ChamferSizeRecord::Distances([a, b]) => [a, b, c.wedge],
        ChamferSizeRecord::DistanceAngle { distance, angle } => [distance, angle, c.wedge],
        ChamferSizeRecord::Chord(l) => [l, l, c.wedge],
    };
    all_finite(&sizes)
}

impl BlendDefinitionRecord {
    /// The structural check of the definition, nested supports included: every curve and
    /// surface as their own checks, the definition's numbers finite, and supports nested at most
    /// [`RECORD_MAX_DEPTH`] deep. The kernel's rebuild checks what the numbers mean.
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met.
    pub fn validate(&self) -> Result<(), RecordError> {
        self.validate_at(1)
    }

    fn validate_at(&self, depth: u32) -> Result<(), RecordError> {
        if depth > RECORD_MAX_DEPTH {
            return Err(refuse(
                "definition.supports",
                "blends nest at most 3 definitions deep",
            ));
        }
        for support in &self.supports {
            support.validate_at(depth, "definition.supports")?;
        }
        let numbers = [
            self.sides[0],
            self.sides[1],
            self.thumbweight,
            self.bound,
            self.map.a[0],
            self.map.a[1],
            self.map.b[0],
            self.map.b[1],
        ];
        if !all_finite(&numbers) {
            return Err(refuse("definition", "a definition's numbers are finite"));
        }
        if self.chamfer.as_ref().is_some_and(|c| !chamfer_finite(c)) {
            return Err(refuse(
                "definition.chamfer",
                "a definition's numbers are finite",
            ));
        }
        match &self.radius {
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
        self.leader.validate("definition.leader")?;
        for seed in &self.seeds {
            seed.validate("definition.seeds")?;
        }
        Ok(())
    }

    /// How many numbers it holds, nested supports included (call on a validated record).
    pub fn items(&self) -> usize {
        let law = match &self.radius {
            RadiusRecord::Constant { .. } => 0,
            RadiusRecord::Law(law) => law.items(),
        };
        self.supports
            .iter()
            .map(SupportRecord::items)
            .chain([law, self.leader.items()])
            .chain(self.seeds.iter().map(NurbsCurveRecord::items))
            .fold(0usize, usize::saturating_add)
    }
}

impl ProceduralBlendRecord {
    /// The structural check of a nested blend: its definition's and its net's.
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met.
    pub fn validate(&self) -> Result<(), RecordError> {
        self.validate_at(1)
    }

    fn validate_at(&self, depth: u32) -> Result<(), RecordError> {
        self.definition.validate_at(depth)?;
        self.net.validate("net")
    }
}

impl ExactRoundDefinition {
    /// The structural check: every curve and surface as their own checks, the supports NURBS
    /// forms (an exact round's supports are faces, never blends), its numbers finite, and for the
    /// chord its sizes and order 0, for any other class order 1 and a positive radius.
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met.
    pub fn validate(&self) -> Result<(), RecordError> {
        for support in &self.supports {
            match support {
                SupportRecord::Nurbs { form, .. } => form.validate("supports")?,
                SupportRecord::Blend(_) => {
                    return Err(refuse("supports", "an exact round's supports are faces"));
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
            if !chamfer_finite(c) {
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
        Ok(())
    }

    /// How many numbers it holds.
    pub fn items(&self) -> usize {
        self.supports
            .iter()
            .map(SupportRecord::items)
            .chain([self.leader.as_ref().map_or(0, NurbsCurveRecord::items)])
            .fold(0usize, usize::saturating_add)
    }
}

impl BlendDefinition {
    /// The structural check of the definition ([`BlendDefinitionRecord::validate`],
    /// [`ExactRoundDefinition::validate`]).
    ///
    /// # Errors
    ///
    /// The first [`RecordError`] met.
    pub fn validate(&self) -> Result<(), RecordError> {
        match self {
            BlendDefinition::Procedural(d) => d.validate(),
            BlendDefinition::ExactRound(r) => r.validate(),
        }
    }

    /// How many numbers it holds, for the record's item budget (call on a validated record).
    pub fn items(&self) -> usize {
        match self {
            BlendDefinition::Procedural(d) => d.items(),
            BlendDefinition::ExactRound(r) => r.items(),
        }
    }
}
