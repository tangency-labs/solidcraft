//! What a simulation is made of: rigid bodies, joints between frames on them, force elements
//! and the solver's settings. Everything is checked by [`Model::validate`] before a run.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

use crate::math::{Mat3, Quat};
use crate::{DynamicsError, Result};

/// Bodies one simulation takes at most.
pub const MAX_BODIES: usize = 200;
/// Joints one simulation takes at most.
pub const MAX_JOINTS: usize = 400;
/// Constraint equations one simulation takes at most (the dense solve grows with their cube).
pub const MAX_CONSTRAINT_ROWS: usize = 1200;
/// Force elements one simulation takes at most.
pub const MAX_FORCES: usize = 2000;
/// Integration steps one run takes at most (accepted and rejected together).
pub const MAX_STEPS: usize = 2_000_000;
/// Output samples one run keeps at most.
pub const MAX_SAMPLES: usize = 100_000;
/// Projection iterations per step at most.
pub const MAX_PROJECTION_ITERATIONS: usize = 50;
/// Coordinates are kept within this size (mm, mm/s, rad/s); beyond it the run stops with
/// [`DynamicsError::Diverged`].
pub const MAX_MAGNITUDE: f64 = 1e12;

/// A rigid body. Lengths in mm, mass in kg, time in s, so forces are in kg·mm/s² (mN) and
/// torques in mN·mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// Mass (kg), > 0.
    pub mass: f64,
    /// Inertia tensor about the centre of mass in the body's own axes (kg·mm²), symmetric
    /// positive definite.
    pub inertia: Mat3,
    /// Centre of mass at the start (world, mm). The body's own frame has its origin here.
    pub position: Vec3,
    /// The body's axes at the start (body → world).
    #[serde(default)]
    pub orientation: Quat,
    /// Velocity of the centre of mass at the start (world, mm/s).
    #[serde(default)]
    pub velocity: Vec3,
    /// Angular velocity at the start (world, rad/s).
    #[serde(default)]
    pub angular_velocity: Vec3,
}

impl Body {
    /// A body at rest at `position`, axes along the world's, with a diagonal inertia.
    pub fn at_rest(mass: f64, principal_inertia: Vec3, position: Vec3) -> Body {
        Body {
            mass,
            inertia: Mat3::diagonal(principal_inertia),
            position,
            orientation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        }
    }
}

/// A frame on a body (in the body's own coordinates: origin at its centre of mass) or, with no
/// body, fixed in the world. `z` and `x` need not be unit or square: `x` is laid square to `z`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<usize>,
    pub origin: Vec3,
    pub z: Vec3,
    pub x: Vec3,
}

impl Marker {
    pub fn new(body: Option<usize>, origin: Vec3, z: Vec3, x: Vec3) -> Marker {
        Marker { body, origin, z, x }
    }
    /// A frame fixed in the world.
    pub fn ground(origin: Vec3, z: Vec3, x: Vec3) -> Marker {
        Marker { body: None, origin, z, x }
    }
}

/// A point on a body (body coordinates) or, with no body, in the world.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<usize>,
    pub point: Vec3,
}

/// The joints the solver knows; each holds `b`'s frame to `a`'s as the matching assembly joint
/// does (z is the joint's axis), with the frames coinciding at zero joint values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    /// No relative motion (6 equations).
    Rigid,
    /// Turns about z (5 equations).
    Revolute,
    /// Slides along z (5 equations).
    Slider,
    /// Turns about and slides along z (4 equations).
    Cylindrical,
    /// Slides in the xy plane and turns about z (3 equations).
    Planar,
    /// Turns any way about the common origin (3 equations).
    Ball,
}

impl JointKind {
    /// How many constraint equations the joint adds.
    pub fn equations(self) -> usize {
        match self {
            JointKind::Rigid => 6,
            JointKind::Revolute | JointKind::Slider => 5,
            JointKind::Cylindrical => 4,
            JointKind::Planar | JointKind::Ball => 3,
        }
    }
    /// The joint's values, in the order [`crate::JointResult::values`] gives them.
    pub fn values(self) -> &'static [&'static str] {
        match self {
            JointKind::Rigid => &[],
            JointKind::Revolute => &["angle"],
            JointKind::Slider => &["distance"],
            JointKind::Cylindrical => &["angle", "distance"],
            JointKind::Planar => &["x", "y", "angle"],
            JointKind::Ball => &["rx", "ry", "rz"],
        }
    }
}

/// A joint: `b`'s frame held to `a`'s.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub kind: JointKind,
    pub a: Marker,
    pub b: Marker,
}

/// One value of a joint: `dof` indexes [`JointKind::values`]. Revolute, slider, cylindrical
/// and planar joints have values that springs, motors, drives and links can act on; ball and
/// rigid joints have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coord {
    pub joint: usize,
    #[serde(default)]
    pub dof: usize,
}

impl Coord {
    pub fn new(joint: usize, dof: usize) -> Coord {
        Coord { joint, dof }
    }
}

/// Points a table may hold at most.
pub const MAX_PROFILE_POINTS: usize = 10_000;

/// A value along time (s): a motor's torque or force, or the value a drive imposes (radians,
/// mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Profile {
    Constant {
        value: f64,
    },
    /// `value + rate·t`.
    Linear {
        value: f64,
        rate: f64,
    },
    /// From `from` to `to` between `start` and `start + duration`, along a cycloidal step
    /// (`τ − sin(2πτ)/2π`) whose speed and acceleration both start and end at zero.
    Ramp {
        from: f64,
        to: f64,
        #[serde(default)]
        start: f64,
        duration: f64,
    },
    /// `offset + amplitude·sin(2π·frequency·t + phase)` (frequency in Hz).
    Harmonic {
        #[serde(default)]
        offset: f64,
        amplitude: f64,
        frequency: f64,
        #[serde(default)]
        phase: f64,
    },
    /// (time, value) points, times increasing: linear in between, held before the first and
    /// after the last. As a drive its speed jumps at the points (its acceleration is taken as 0).
    Table {
        points: Vec<(f64, f64)>,
    },
}

impl Profile {
    /// The value and its first and second time derivatives at `t`.
    pub fn eval(&self, t: f64) -> (f64, f64, f64) {
        match self {
            Profile::Constant { value } => (*value, 0.0, 0.0),
            Profile::Linear { value, rate } => (value + rate * t, *rate, 0.0),
            Profile::Ramp { from, to, start, duration } => {
                let tau = (t - start) / duration;
                if !(tau > 0.0) {
                    return (*from, 0.0, 0.0);
                }
                if tau >= 1.0 {
                    return (*to, 0.0, 0.0);
                }
                let span = to - from;
                let (s, c) = (std::f64::consts::TAU * tau).sin_cos();
                (
                    from + span * (tau - s / std::f64::consts::TAU),
                    span / duration * (1.0 - c),
                    span / (duration * duration) * std::f64::consts::TAU * s,
                )
            }
            Profile::Harmonic { offset, amplitude, frequency, phase } => {
                let w = std::f64::consts::TAU * frequency;
                let (s, c) = (w * t + phase).sin_cos();
                (offset + amplitude * s, amplitude * w * c, -amplitude * w * w * s)
            }
            Profile::Table { points } => {
                let (Some(first), Some(last)) = (points.first(), points.last()) else { return (0.0, 0.0, 0.0) };
                if t <= first.0 {
                    return (first.1, 0.0, 0.0);
                }
                if t >= last.0 {
                    return (last.1, 0.0, 0.0);
                }
                points
                    .windows(2)
                    .find_map(|w| {
                        let (a, b) = (w.first()?, w.get(1)?);
                        (t >= a.0 && t < b.0).then(|| {
                            let slope = (b.1 - a.1) / (b.0 - a.0);
                            (a.1 + slope * (t - a.0), slope, 0.0)
                        })
                    })
                    .unwrap_or((last.1, 0.0, 0.0))
            }
        }
    }

    fn validate(&self) -> std::result::Result<(), String> {
        let fin = |x: f64| x.is_finite() && x.abs() < MAX_MAGNITUDE;
        let ok = match self {
            Profile::Constant { value } => fin(*value),
            Profile::Linear { value, rate } => fin(*value) && fin(*rate),
            Profile::Ramp { from, to, start, duration } => fin(*from) && fin(*to) && fin(*start) && fin(*duration) && *duration > 1e-9,
            Profile::Harmonic { offset, amplitude, frequency, phase } => {
                fin(*offset) && fin(*amplitude) && fin(*frequency) && fin(*phase) && frequency.abs() < 1e6
            }
            Profile::Table { points } => {
                if points.is_empty() || points.len() > MAX_PROFILE_POINTS {
                    return Err(format!("a table needs 1 to {MAX_PROFILE_POINTS} points"));
                }
                points.iter().all(|(t, v)| fin(*t) && fin(*v))
                    && points.windows(2).all(|w| matches!((w.first(), w.get(1)), (Some(a), Some(b)) if b.0 > a.0))
            }
        };
        if ok { Ok(()) } else { Err("a profile's numbers must be finite (ramps need a duration, tables increasing times)".into()) }
    }
}

/// A joint value made to follow a profile along time (radians or mm): the joint turns or
/// slides as told, whatever it takes; [`crate::Sample::drives`] reports what it takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drive {
    pub at: Coord,
    pub motion: Profile,
}

/// A motion link: `follower = ratio·driver + offset`, between values of two joints (a gear
/// pair: angle to angle; a rack and pinion: angle to distance, the ratio being the pinion's
/// radius; a screw: angle to distance along the same cylindrical joint, the ratio being
/// pitch / 2π). Without an offset, the one that holds at the start is kept.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub driver: Coord,
    pub follower: Coord,
    pub ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
}

/// A force element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Force {
    /// A spring and damper on a joint value: the joint is pushed back by
    /// `stiffness·(value − rest) + damping·rate` (torsional for an angle: mN·mm per radian and
    /// mN·mm·s; linear for a distance: kg/s² and kg/s).
    JointSpring {
        at: Coord,
        stiffness: f64,
        #[serde(default)]
        damping: f64,
        #[serde(default)]
        rest: f64,
    },
    /// A motor's torque (mN·mm) or force (mN) along a joint value, following a profile: it
    /// pushes B's side forward and A's side back.
    Motor { at: Coord, effort: Profile },
    /// A linear spring and damper between two points: the tension is
    /// `stiffness·(length − free_length) + damping·(rate of length) + preload`.
    Spring {
        a: Anchor,
        b: Anchor,
        /// kg/s² (= N/m = mN/mm), ≥ 0.
        stiffness: f64,
        /// kg/s (= N·s/m), ≥ 0.
        damping: f64,
        /// mm, ≥ 0.
        free_length: f64,
        /// mN, tension at free length.
        #[serde(default)]
        preload: f64,
    },
    /// A constant force (world, mN) at a point of a body, and a constant torque (world, mN·mm).
    Load {
        body: usize,
        #[serde(default)]
        point: Vec3,
        #[serde(default)]
        force: Vec3,
        #[serde(default)]
        torque: Vec3,
    },
}

/// How the equations are stepped in time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Integrator {
    /// Classical fourth-order Runge–Kutta with a fixed step (s).
    Rk4 { step: f64 },
    /// Dormand–Prince 5(4) with error control: steps between `min_step` and `max_step` (s)
    /// keep the local error under `abs_tol + rel_tol·|y|`.
    Adaptive { rel_tol: f64, abs_tol: f64, min_step: f64, max_step: f64 },
}

/// A run's settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Simulated time (s), > 0.
    pub duration: f64,
    /// Time between output samples (s), > 0; the integrator lands on every sample.
    pub output_interval: f64,
    pub integrator: Integrator,
    /// Project positions and velocities back onto the joints after every step (on by default).
    #[serde(default = "yes")]
    pub projection: bool,
    /// The constraint error (mm, or radians times mm for directions) projection reaches.
    #[serde(default = "default_projection_tol")]
    pub projection_tol: f64,
    /// Baumgarte stabilisation `(α, β)` (1/s): the constraint accelerations become
    /// `Φ̈ + 2αΦ̇ + β²Φ = 0`. Off by default (projection keeps the joints).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baumgarte: Option<(f64, f64)>,
}

fn yes() -> bool {
    true
}
fn default_projection_tol() -> f64 {
    1e-10
}

impl Settings {
    /// Fixed-step RK4 settings.
    pub fn rk4(duration: f64, step: f64, output_interval: f64) -> Settings {
        Settings {
            duration,
            output_interval,
            integrator: Integrator::Rk4 { step },
            projection: true,
            projection_tol: default_projection_tol(),
            baumgarte: None,
        }
    }
    /// Adaptive settings with the given relative and absolute tolerances.
    pub fn adaptive(duration: f64, output_interval: f64, rel_tol: f64, abs_tol: f64) -> Settings {
        Settings {
            duration,
            output_interval,
            integrator: Integrator::Adaptive { rel_tol, abs_tol, min_step: 1e-9, max_step: output_interval },
            projection: true,
            projection_tol: default_projection_tol(),
            baumgarte: None,
        }
    }
}

/// A multibody system.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub bodies: Vec<Body>,
    #[serde(default)]
    pub joints: Vec<Joint>,
    #[serde(default)]
    pub forces: Vec<Force>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<Drive>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    /// Gravity (mm/s²), e.g. `(0, 0, −9806.65)`.
    #[serde(default)]
    pub gravity: Vec3,
}

/// Standard gravity (mm/s²).
pub const STANDARD_GRAVITY: f64 = 9806.65;

fn finite_vec(v: Vec3) -> bool {
    v.is_finite() && v.x.abs() < MAX_MAGNITUDE && v.y.abs() < MAX_MAGNITUDE && v.z.abs() < MAX_MAGNITUDE
}

fn bad(what: String) -> DynamicsError {
    DynamicsError::InvalidModel(what)
}

/// A marker's axes in its body's coordinates (columns x, y, z), from its `z` and `x`.
pub(crate) fn marker_axes(m: &Marker) -> Option<Mat3> {
    let z = m.z.normalized()?;
    let x = (m.x - z * m.x.dot(z)).normalized()?;
    let y = z.cross(x).normalized()?;
    Some(Mat3::from_cols(x, y, z))
}

impl Model {
    /// Checks every number and reference, and the caps. Called by [`crate::simulate`].
    pub fn validate(&self) -> Result<()> {
        if self.bodies.is_empty() {
            return Err(bad("no bodies".into()));
        }
        if self.bodies.len() > MAX_BODIES {
            return Err(DynamicsError::TooLarge(format!("{} bodies (at most {MAX_BODIES})", self.bodies.len())));
        }
        if self.joints.len() > MAX_JOINTS {
            return Err(DynamicsError::TooLarge(format!("{} joints (at most {MAX_JOINTS})", self.joints.len())));
        }
        if self.forces.len() > MAX_FORCES {
            return Err(DynamicsError::TooLarge(format!("{} force elements (at most {MAX_FORCES})", self.forces.len())));
        }
        if self.drives.len() > MAX_JOINTS || self.links.len() > MAX_JOINTS {
            return Err(DynamicsError::TooLarge(format!("drives and links: at most {MAX_JOINTS} each")));
        }
        let rows: usize = self.joints.iter().map(|j| j.kind.equations()).sum::<usize>() + self.drives.len() + self.links.len();
        if rows > MAX_CONSTRAINT_ROWS {
            return Err(DynamicsError::TooLarge(format!("{rows} constraint equations (at most {MAX_CONSTRAINT_ROWS})")));
        }
        if !finite_vec(self.gravity) {
            return Err(bad("gravity is not a finite vector".into()));
        }
        for (i, b) in self.bodies.iter().enumerate() {
            if !(b.mass > 0.0) || !(b.mass < MAX_MAGNITUDE) {
                return Err(bad(format!("body {i}: mass must be positive and finite")));
            }
            if b.inertia.spd_inverse().is_none() {
                return Err(bad(format!("body {i}: inertia must be symmetric positive definite")));
            }
            if !finite_vec(b.position) || !finite_vec(b.velocity) || !finite_vec(b.angular_velocity) {
                return Err(bad(format!("body {i}: position and velocities must be finite")));
            }
            if b.orientation.normalized().is_none() {
                return Err(bad(format!("body {i}: orientation is not a rotation")));
            }
        }
        let body_ok = |b: Option<usize>| b.is_none_or(|b| b < self.bodies.len());
        for (i, j) in self.joints.iter().enumerate() {
            for m in [&j.a, &j.b] {
                if !body_ok(m.body) {
                    return Err(bad(format!("joint {i}: no body {}", m.body.unwrap_or(0))));
                }
                if !finite_vec(m.origin) || marker_axes(m).is_none() {
                    return Err(bad(format!("joint {i}: a frame's origin or axes are not usable")));
                }
            }
            if j.a.body == j.b.body {
                return Err(bad(format!("joint {i}: both frames are on the same body")));
            }
        }
        for (i, f) in self.forces.iter().enumerate() {
            match f {
                Force::Spring { a, b, stiffness, damping, free_length, preload } => {
                    if !body_ok(a.body) || !body_ok(b.body) {
                        return Err(bad(format!("force {i}: no such body")));
                    }
                    if !finite_vec(a.point) || !finite_vec(b.point) {
                        return Err(bad(format!("force {i}: anchor points must be finite")));
                    }
                    let ok = |x: f64| (0.0..MAX_MAGNITUDE).contains(&x);
                    if !ok(*stiffness) || !ok(*damping) || !ok(*free_length) || !preload.is_finite() || preload.abs() >= MAX_MAGNITUDE {
                        return Err(bad(format!("force {i}: stiffness, damping and free length must be ≥ 0 and finite")));
                    }
                }
                Force::Load { body, point, force, torque } => {
                    if *body >= self.bodies.len() {
                        return Err(bad(format!("force {i}: no body {body}")));
                    }
                    if !finite_vec(*point) || !finite_vec(*force) || !finite_vec(*torque) {
                        return Err(bad(format!("force {i}: point, force and torque must be finite")));
                    }
                }
                Force::JointSpring { at, stiffness, damping, rest } => {
                    self.check_coord(*at).map_err(|e| bad(format!("force {i}: {e}")))?;
                    let ok = |x: f64| (0.0..MAX_MAGNITUDE).contains(&x);
                    if !ok(*stiffness) || !ok(*damping) || !rest.is_finite() || rest.abs() >= MAX_MAGNITUDE {
                        return Err(bad(format!("force {i}: stiffness and damping must be ≥ 0 and finite, rest finite")));
                    }
                }
                Force::Motor { at, effort } => {
                    self.check_coord(*at).map_err(|e| bad(format!("force {i}: {e}")))?;
                    effort.validate().map_err(|e| bad(format!("force {i}: {e}")))?;
                }
            }
        }
        for (i, d) in self.drives.iter().enumerate() {
            self.check_coord(d.at).map_err(|e| bad(format!("drive {i}: {e}")))?;
            d.motion.validate().map_err(|e| bad(format!("drive {i}: {e}")))?;
        }
        for (i, l) in self.links.iter().enumerate() {
            self.check_coord(l.driver).map_err(|e| bad(format!("link {i}: {e}")))?;
            self.check_coord(l.follower).map_err(|e| bad(format!("link {i}: {e}")))?;
            if l.driver == l.follower {
                return Err(bad(format!("link {i}: a value can't drive itself")));
            }
            let fin = |x: f64| x.is_finite() && x.abs() < MAX_MAGNITUDE;
            if !fin(l.ratio) || !l.offset.is_none_or(fin) {
                return Err(bad(format!("link {i}: ratio and offset must be finite")));
            }
        }
        Ok(())
    }

    /// The joint and value a [`Coord`] names, if they exist and can be acted on.
    fn check_coord(&self, c: Coord) -> std::result::Result<(), String> {
        let j = self.joints.get(c.joint).ok_or_else(|| format!("no joint {}", c.joint))?;
        if matches!(j.kind, JointKind::Ball | JointKind::Rigid) {
            return Err(format!("joint {}: not supported yet: springs, motors, drives and links on {:?} joints", c.joint, j.kind));
        }
        if c.dof >= j.kind.values().len() {
            return Err(format!("joint {} has no value {}", c.joint, c.dof));
        }
        Ok(())
    }
}

impl Settings {
    /// Checks the times, tolerances and that the run fits the step and sample caps.
    pub fn validate(&self) -> Result<()> {
        let pos = |x: f64| x > 0.0 && x < MAX_MAGNITUDE;
        if !pos(self.duration) || !pos(self.output_interval) {
            return Err(DynamicsError::InvalidSettings("duration and output interval must be positive and finite".into()));
        }
        let samples = (self.duration / self.output_interval).ceil();
        if !(samples < MAX_SAMPLES as f64) {
            return Err(DynamicsError::TooLarge(format!("{samples} output samples (at most {MAX_SAMPLES})")));
        }
        if !pos(self.projection_tol) {
            return Err(DynamicsError::InvalidSettings("projection tolerance must be positive".into()));
        }
        if let Some((a, b)) = self.baumgarte
            && (!(0.0..MAX_MAGNITUDE).contains(&a) || !(0.0..MAX_MAGNITUDE).contains(&b))
        {
            return Err(DynamicsError::InvalidSettings("Baumgarte terms must be ≥ 0 and finite".into()));
        }
        match self.integrator {
            Integrator::Rk4 { step } => {
                if !pos(step) {
                    return Err(DynamicsError::InvalidSettings("step must be positive".into()));
                }
                let steps = (self.duration / step.min(self.output_interval)).ceil();
                if !(steps < MAX_STEPS as f64) {
                    return Err(DynamicsError::TooLarge(format!("{steps} steps (at most {MAX_STEPS})")));
                }
            }
            Integrator::Adaptive { rel_tol, abs_tol, min_step, max_step } => {
                if !pos(rel_tol) || !pos(abs_tol) || !pos(min_step) || !pos(max_step) || min_step > max_step {
                    return Err(DynamicsError::InvalidSettings("tolerances and step bounds must be positive, min ≤ max".into()));
                }
            }
        }
        Ok(())
    }
}
