//! The equations of motion: body states, joint constraint equations with their Jacobians,
//! applied forces, the constrained accelerations and the projections back onto the joints.
//!
//! Bodies use maximal coordinates: centre-of-mass position `x`, unit quaternion `q`, velocity
//! `v` and angular velocity `ω` (both world). With `u = (v, ω)` per body, mass matrix
//! `M = diag(m·I₃, I_w)` (`I_w = R I_body Rᵀ`), joint equations `Φ(x, q) = 0`, their Jacobian
//! `J` (`Φ̇ = J u`) and `γ = −J̇ u`, the accelerations solve
//!
//! ```text
//! M u̇ + Jᵀ λ = F − ω × I_w ω,      J u̇ = γ
//! ```
//!
//! through the Schur complement `(J M⁻¹ Jᵀ) λ = J M⁻¹ F − γ` (`linalg::solve_psd`, which takes
//! repeated joint equations), and `λ` gives the joints' reactions.

use solidcraft_geom::Vec3;

use crate::linalg::{Sym, solve_psd};
use crate::math::{Mat3, Quat, cross2};
use crate::model::{Force, JointKind, MAX_MAGNITUDE, MAX_PROJECTION_ITERATIONS, Model, marker_axes};
use crate::{BodyState, DynamicsError, Energy, JointResult, Result};

/// Numbers per body in the flat state: x (3), q (4), v (3), ω (3).
pub(crate) const STRIDE: usize = 13;

/// Pivots under this fraction of the largest are taken as repeated equations.
const RANK_TOL: f64 = 1e-10;

fn v3(y: &[f64], at: usize) -> Vec3 {
    let g = |i: usize| y.get(at + i).copied().unwrap_or(0.0);
    Vec3::new(g(0), g(1), g(2))
}
fn put3(y: &mut [f64], at: usize, v: Vec3) {
    for (i, c) in [v.x, v.y, v.z].into_iter().enumerate() {
        if let Some(s) = y.get_mut(at + i) {
            *s = c;
        }
    }
}
fn quat(y: &[f64], at: usize) -> Quat {
    let g = |i: usize| y.get(at + i).copied().unwrap_or(0.0);
    Quat { w: g(0), x: g(1), y: g(2), z: g(3) }
}
fn put_quat(y: &mut [f64], at: usize, q: Quat) {
    for (i, c) in [q.w, q.x, q.y, q.z].into_iter().enumerate() {
        if let Some(s) = y.get_mut(at + i) {
            *s = c;
        }
    }
}

/// One body's precomputed constants.
struct BodyConst {
    mass: f64,
    inertia: Mat3,
    inertia_inv: Mat3,
}

/// A joint frame: its body, origin and axes in the body's coordinates.
struct MarkerConst {
    body: Option<usize>,
    origin: Vec3,
    axes: Mat3,
}

struct JointConst {
    kind: JointKind,
    a: MarkerConst,
    b: MarkerConst,
}

/// A body's kinematics at one state.
#[derive(Clone, Copy)]
struct Kin {
    x: Vec3,
    rot: Mat3,
    v: Vec3,
    w: Vec3,
}

const GROUND: Kin = Kin { x: Vec3::ZERO, rot: Mat3::IDENTITY, v: Vec3::ZERO, w: Vec3::ZERO };

/// A joint frame at one state (world).
struct MarkerWorld {
    body: Option<usize>,
    /// Origin minus the body's centre of mass.
    r: Vec3,
    /// Origin.
    p: Vec3,
    /// Velocity of the origin.
    pdot: Vec3,
    axes: [Vec3; 3],
    w: Vec3,
}

/// A body's part of one Jacobian row: (linear, angular).
type Block = (usize, Vec3, Vec3);

/// One constraint equation at a state.
pub(crate) struct Row {
    a: Option<Block>,
    b: Option<Block>,
    pub(crate) phi: f64,
    /// `J̇ u`: the equation's second derivative at zero accelerations.
    jdot_u: f64,
}

impl Row {
    fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.a.iter().chain(self.b.iter())
    }
    /// `J u`.
    fn rate(&self, kin: &[Kin]) -> f64 {
        self.blocks().map(|(i, l, a)| kin.get(*i).map_or(0.0, |k| l.dot(k.v) + a.dot(k.w))).sum()
    }
}

/// A model ready to integrate.
pub(crate) struct System {
    bodies: Vec<BodyConst>,
    joints: Vec<JointConst>,
    forces: Vec<Force>,
    gravity: Vec3,
    baumgarte: Option<(f64, f64)>,
}

/// Accelerations and multipliers at one state.
pub(crate) struct Accel {
    /// The flat state's derivative.
    pub(crate) ydot: Vec<f64>,
    /// One multiplier per constraint row, in joint order.
    lambda: Vec<f64>,
    rows: Vec<Row>,
    /// The rank of `J M⁻¹ Jᵀ`.
    pub(crate) rank: usize,
}

impl Accel {
    /// The number of joint equations.
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }
}

fn marker_const(m: &crate::Marker) -> Result<MarkerConst> {
    let axes = marker_axes(m).ok_or_else(|| DynamicsError::InvalidModel("a joint frame's axes are not usable".into()))?;
    Ok(MarkerConst { body: m.body, origin: m.origin, axes })
}

impl System {
    pub(crate) fn new(model: &Model, baumgarte: Option<(f64, f64)>) -> Result<System> {
        model.validate()?;
        let bodies = model
            .bodies
            .iter()
            .map(|b| {
                let inertia_inv = b.inertia.spd_inverse().ok_or_else(|| DynamicsError::InvalidModel("inertia is not positive definite".into()))?;
                Ok(BodyConst { mass: b.mass, inertia: b.inertia, inertia_inv })
            })
            .collect::<Result<Vec<_>>>()?;
        let joints = model
            .joints
            .iter()
            .map(|j| Ok(JointConst { kind: j.kind, a: marker_const(&j.a)?, b: marker_const(&j.b)? }))
            .collect::<Result<Vec<_>>>()?;
        Ok(System { bodies, joints, forces: model.forces.clone(), gravity: model.gravity, baumgarte })
    }

    /// The flat state of the model's starting conditions.
    pub(crate) fn initial_state(model: &Model) -> Result<Vec<f64>> {
        let mut y = vec![0.0; model.bodies.len() * STRIDE];
        for (i, b) in model.bodies.iter().enumerate() {
            let at = i * STRIDE;
            let q = b.orientation.normalized().ok_or_else(|| DynamicsError::InvalidModel(format!("body {i}: orientation is not a rotation")))?;
            put3(&mut y, at, b.position);
            put_quat(&mut y, at + 3, q);
            put3(&mut y, at + 7, b.velocity);
            put3(&mut y, at + 10, b.angular_velocity);
        }
        Ok(y)
    }

    /// Unit quaternions again after a step.
    pub(crate) fn normalize(&self, y: &mut [f64]) -> Result<()> {
        for i in 0..self.bodies.len() {
            let q = quat(y, i * STRIDE + 3).normalized().ok_or(DynamicsError::Diverged { time: f64::NAN })?;
            put_quat(y, i * STRIDE + 3, q);
        }
        Ok(())
    }

    fn kinematics(&self, y: &[f64]) -> Vec<Kin> {
        (0..self.bodies.len())
            .map(|i| {
                let at = i * STRIDE;
                Kin { x: v3(y, at), rot: quat(y, at + 3).to_mat3(), v: v3(y, at + 7), w: v3(y, at + 10) }
            })
            .collect()
    }

    fn kin_of(kin: &[Kin], body: Option<usize>) -> Kin {
        body.and_then(|b| kin.get(b).copied()).unwrap_or(GROUND)
    }

    fn marker_world(kin: &[Kin], m: &MarkerConst) -> MarkerWorld {
        let k = Self::kin_of(kin, m.body);
        let r = k.rot.mul_vec(m.origin);
        let axes = [k.rot.mul_vec(m.axes.col(0)), k.rot.mul_vec(m.axes.col(1)), k.rot.mul_vec(m.axes.col(2))];
        MarkerWorld { body: m.body, r, p: k.x + r, pdot: k.v + k.w.cross(r), axes, w: k.w }
    }

    /// The constraint equations of every joint at `kin`, in joint order.
    fn rows(&self, kin: &[Kin]) -> Vec<Row> {
        let mut rows = Vec::new();
        for j in &self.joints {
            let a = Self::marker_world(kin, &j.a);
            let b = Self::marker_world(kin, &j.b);
            joint_rows(j.kind, &a, &b, &mut rows);
        }
        rows
    }

    /// `M⁻¹` applied to one body's (force, torque).
    fn minv(&self, kin: &[Kin], body: usize, f: Vec3, t: Vec3) -> (Vec3, Vec3) {
        match (self.bodies.get(body), kin.get(body)) {
            (Some(b), Some(k)) => {
                let iw_inv = k.rot.mul(&b.inertia_inv).mul(&k.rot.transpose());
                (f * (1.0 / b.mass), iw_inv.mul_vec(t))
            }
            _ => (Vec3::ZERO, Vec3::ZERO),
        }
    }

    /// `J M⁻¹ Jᵀ`, and each row's `M⁻¹ Jᵀ` blocks.
    fn schur(&self, kin: &[Kin], rows: &[Row]) -> (Sym, Vec<Vec<Block>>) {
        let w: Vec<Vec<Block>> = rows
            .iter()
            .map(|r| {
                r.blocks()
                    .map(|(i, l, a)| {
                        let (ml, ma) = self.minv(kin, *i, *l, *a);
                        (*i, ml, ma)
                    })
                    .collect()
            })
            .collect();
        let n = rows.len();
        let mut s = Sym::zeros(n);
        for (r, row) in rows.iter().enumerate() {
            for (c, wc) in w.iter().enumerate().skip(r) {
                let mut sum = 0.0;
                for (i, l, a) in row.blocks() {
                    for (k, ml, ma) in wc {
                        if i == k {
                            sum += l.dot(*ml) + a.dot(*ma);
                        }
                    }
                }
                s.set(r, c, sum);
                s.set(c, r, sum);
            }
        }
        (s, w)
    }

    /// Applied forces and torques (about each centre of mass, world), gyroscopic terms included.
    fn applied(&self, kin: &[Kin]) -> Vec<(Vec3, Vec3)> {
        let mut out: Vec<(Vec3, Vec3)> = self
            .bodies
            .iter()
            .zip(kin)
            .map(|(b, k)| {
                let iw = k.rot.mul(&b.inertia).mul(&k.rot.transpose());
                (self.gravity * b.mass, -k.w.cross(iw.mul_vec(k.w)))
            })
            .collect();
        let mut add = |body: Option<usize>, r: Vec3, f: Vec3, t: Vec3| {
            if let Some(slot) = body.and_then(|b| out.get_mut(b)) {
                slot.0 += f;
                slot.1 += r.cross(f) + t;
            }
        };
        for f in &self.forces {
            match *f {
                Force::Spring { a, b, stiffness, damping, free_length, preload } => {
                    let (ka, kb) = (Self::kin_of(kin, a.body), Self::kin_of(kin, b.body));
                    let (ra, rb) = (ka.rot.mul_vec(a.point), kb.rot.mul_vec(b.point));
                    let d = (kb.x + rb) - (ka.x + ra);
                    let len = d.len();
                    let Some(n) = d.normalized().filter(|_| len > 1e-9) else { continue };
                    let rate = n.dot((kb.v + kb.w.cross(rb)) - (ka.v + ka.w.cross(ra)));
                    let tension = stiffness * (len - free_length) + damping * rate + preload;
                    add(a.body, ra, n * tension, Vec3::ZERO);
                    add(b.body, rb, n * -tension, Vec3::ZERO);
                }
                Force::Load { body, point, force, torque } => {
                    let r = kin.get(body).map_or(Vec3::ZERO, |k| k.rot.mul_vec(point));
                    add(Some(body), r, force, torque);
                }
            }
        }
        out
    }

    /// The state's derivative and the joints' multipliers.
    pub(crate) fn accelerations(&self, y: &[f64]) -> Result<Accel> {
        let kin = self.kinematics(y);
        let applied = self.applied(&kin);
        let rows = self.rows(&kin);
        let (s, w) = self.schur(&kin, &rows);
        // Unconstrained accelerations M⁻¹F.
        let free: Vec<(Vec3, Vec3)> = applied.iter().enumerate().map(|(i, (f, t))| self.minv(&kin, i, *f, *t)).collect();
        let rhs: Vec<f64> = rows
            .iter()
            .map(|r| {
                let jmf: f64 = r.blocks().map(|(i, l, a)| free.get(*i).map_or(0.0, |(fl, fa)| l.dot(*fl) + a.dot(*fa))).sum();
                let stab = self.baumgarte.map_or(0.0, |(alpha, beta)| 2.0 * alpha * r.rate(&kin) + beta * beta * r.phi);
                jmf + r.jdot_u + stab
            })
            .collect();
        let (lambda, rank) =
            if rows.is_empty() { (Vec::new(), 0) } else { solve_psd(s, &rhs, RANK_TOL).ok_or(DynamicsError::Singular { time: f64::NAN })? };
        let mut acc = free;
        for (wr, l) in w.iter().zip(&lambda) {
            for (i, ml, ma) in wr {
                if let Some(a) = acc.get_mut(*i) {
                    a.0 -= *ml * *l;
                    a.1 -= *ma * *l;
                }
            }
        }
        let mut ydot = vec![0.0; y.len()];
        for (i, (k, a)) in kin.iter().zip(&acc).enumerate() {
            let at = i * STRIDE;
            put3(&mut ydot, at, k.v);
            put_quat(&mut ydot, at + 3, quat(y, at + 3).rate(k.w));
            put3(&mut ydot, at + 7, a.0);
            put3(&mut ydot, at + 10, a.1);
        }
        if !ydot.iter().all(|x| x.is_finite()) {
            return Err(DynamicsError::Diverged { time: f64::NAN });
        }
        Ok(Accel { ydot, lambda, rows, rank })
    }

    /// The largest joint equation error at `y`.
    pub(crate) fn constraint_error(&self, y: &[f64]) -> f64 {
        self.rows(&self.kinematics(y)).iter().map(|r| r.phi.abs()).fold(0.0, f64::max)
    }

    /// Moves the bodies the least (mass-weighted) onto the joints: Gauss–Newton on `Φ = 0`.
    /// Returns the error left.
    pub(crate) fn project_positions(&self, y: &mut [f64], tol: f64) -> Result<f64> {
        let mut err;
        let mut iteration = 0;
        loop {
            let kin = self.kinematics(y);
            let rows = self.rows(&kin);
            err = rows.iter().map(|r| r.phi.abs()).fold(0.0, f64::max);
            if !(err > tol) || iteration >= MAX_PROJECTION_ITERATIONS {
                break;
            }
            iteration += 1;
            let (s, w) = self.schur(&kin, &rows);
            let phi: Vec<f64> = rows.iter().map(|r| r.phi).collect();
            let (mu, _) = solve_psd(s, &phi, RANK_TOL).ok_or(DynamicsError::Singular { time: f64::NAN })?;
            let mut delta = vec![(Vec3::ZERO, Vec3::ZERO); self.bodies.len()];
            for (wr, m) in w.iter().zip(&mu) {
                for (i, ml, ma) in wr {
                    if let Some(d) = delta.get_mut(*i) {
                        d.0 -= *ml * *m;
                        d.1 -= *ma * *m;
                    }
                }
            }
            for (i, (dx, dth)) in delta.into_iter().enumerate() {
                let at = i * STRIDE;
                put3(y, at, v3(y, at) + dx);
                let q = Quat::from_rotation_vector(dth).compose(quat(y, at + 3)).normalized().ok_or(DynamicsError::Diverged { time: f64::NAN })?;
                put_quat(y, at + 3, q);
            }
        }
        if !err.is_finite() || !y.iter().all(|x| x.is_finite() && x.abs() < MAX_MAGNITUDE) {
            return Err(DynamicsError::Diverged { time: f64::NAN });
        }
        Ok(err)
    }

    /// Removes the velocities the joints don't allow (mass-weighted): `J u = 0`.
    pub(crate) fn project_velocities(&self, y: &mut [f64]) -> Result<()> {
        let kin = self.kinematics(y);
        let rows = self.rows(&kin);
        if rows.is_empty() {
            return Ok(());
        }
        let (s, w) = self.schur(&kin, &rows);
        let ju: Vec<f64> = rows.iter().map(|r| r.rate(&kin)).collect();
        let (mu, _) = solve_psd(s, &ju, RANK_TOL).ok_or(DynamicsError::Singular { time: f64::NAN })?;
        for (wr, m) in w.iter().zip(&mu) {
            for (i, ml, ma) in wr {
                let at = i * STRIDE;
                put3(y, at + 7, v3(y, at + 7) - *ml * *m);
                put3(y, at + 10, v3(y, at + 10) - *ma * *m);
            }
        }
        Ok(())
    }

    /// The bodies' states.
    pub(crate) fn body_states(&self, y: &[f64]) -> Vec<BodyState> {
        (0..self.bodies.len())
            .map(|i| {
                let at = i * STRIDE;
                BodyState { position: v3(y, at), orientation: quat(y, at + 3), velocity: v3(y, at + 7), angular_velocity: v3(y, at + 10) }
            })
            .collect()
    }

    /// Kinetic, gravity and spring energy.
    pub(crate) fn energy(&self, y: &[f64]) -> Energy {
        let kin = self.kinematics(y);
        let mut e = Energy::default();
        for (b, k) in self.bodies.iter().zip(&kin) {
            let iw = k.rot.mul(&b.inertia).mul(&k.rot.transpose());
            e.kinetic += 0.5 * b.mass * k.v.len2() + 0.5 * k.w.dot(iw.mul_vec(k.w));
            e.gravity -= b.mass * self.gravity.dot(k.x);
        }
        for f in &self.forces {
            if let Force::Spring { a, b, stiffness, free_length, preload, .. } = *f {
                let (ka, kb) = (Self::kin_of(&kin, a.body), Self::kin_of(&kin, b.body));
                let stretch = ((kb.x + kb.rot.mul_vec(b.point)) - (ka.x + ka.rot.mul_vec(a.point))).len() - free_length;
                e.spring += 0.5 * stiffness * stretch * stretch + preload * stretch;
            }
        }
        e
    }

    /// Each joint's values, rates and reaction (on `b`'s side, at its frame's origin, world),
    /// from the multipliers `acc` found at `y`.
    pub(crate) fn joint_results(&self, y: &[f64], acc: &Accel) -> Vec<JointResult> {
        let kin = self.kinematics(y);
        let mut at = 0;
        let mut out = Vec::with_capacity(self.joints.len());
        for j in &self.joints {
            let a = Self::marker_world(&kin, &j.a);
            let b = Self::marker_world(&kin, &j.b);
            let (values, rates) = joint_values(j.kind, &a, &b);
            let n = j.kind.equations();
            // The wrench the joint's equations put on each body: −Jᵀλ.
            let (mut fa, mut ta, mut fb, mut tb) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
            for k in at..at + n {
                let (Some(row), Some(l)) = (acc.rows.get(k), acc.lambda.get(k)) else { continue };
                if let Some((_, lin, ang)) = row.a {
                    fa -= lin * *l;
                    ta -= ang * *l;
                }
                if let Some((_, lin, ang)) = row.b {
                    fb -= lin * *l;
                    tb -= ang * *l;
                }
            }
            at += n;
            let (force, torque) = if b.body.is_some() {
                (fb, tb - b.r.cross(fb))
            } else {
                // B is the ground: it takes what A takes, turned round, about B's origin.
                let xa = Self::kin_of(&kin, a.body).x;
                (-fa, -(ta + (xa - b.p).cross(fa)))
            };
            out.push(JointResult { values, rates, force, torque });
        }
        out
    }
}

/// `(Φ, J, J̇u)` for one body point's world component along `e`: `e·(p_B − p_A)`.
fn point_row(a: &MarkerWorld, b: &MarkerWorld, e: Vec3) -> Row {
    let acc = |m: &MarkerWorld| cross2(m.w, m.r);
    Row { a: a.body.map(|i| (i, -e, -a.r.cross(e))), b: b.body.map(|i| (i, e, b.r.cross(e))), phi: e.dot(b.p - a.p), jdot_u: e.dot(acc(b) - acc(a)) }
}

/// `u·v = 0` for a direction `u` of A and `v` of B.
fn dot1_row(a: &MarkerWorld, u: Vec3, b: &MarkerWorld, v: Vec3) -> Row {
    let (ud, vd) = (a.w.cross(u), b.w.cross(v));
    let (udd, vdd) = (cross2(a.w, u), cross2(b.w, v));
    Row {
        a: a.body.map(|i| (i, Vec3::ZERO, u.cross(v))),
        b: b.body.map(|i| (i, Vec3::ZERO, v.cross(u))),
        phi: u.dot(v),
        jdot_u: udd.dot(v) + 2.0 * ud.dot(vd) + u.dot(vdd),
    }
}

/// `d·u = 0` for `d = p_B − p_A` and a direction `u` of A.
fn dot2_row(a: &MarkerWorld, u: Vec3, b: &MarkerWorld) -> Row {
    let d = b.p - a.p;
    let dd = b.pdot - a.pdot;
    let ddd = cross2(b.w, b.r) - cross2(a.w, a.r);
    let (ud, udd) = (a.w.cross(u), cross2(a.w, u));
    Row {
        a: a.body.map(|i| (i, -u, u.cross(d) - a.r.cross(u))),
        b: b.body.map(|i| (i, u, b.r.cross(u))),
        phi: d.dot(u),
        jdot_u: ddd.dot(u) + 2.0 * dd.dot(ud) + d.dot(udd),
    }
}

fn joint_rows(kind: JointKind, a: &MarkerWorld, b: &MarkerWorld, rows: &mut Vec<Row>) {
    let [ax, ay, az] = a.axes;
    let [bx, by, bz] = b.axes;
    let point = |rows: &mut Vec<Row>| rows.extend([Vec3::X, Vec3::Y, Vec3::Z].map(|e| point_row(a, b, e)));
    match kind {
        JointKind::Rigid => {
            point(rows);
            rows.extend([dot1_row(a, ay, b, bz), dot1_row(a, az, b, bx), dot1_row(a, ax, b, by)]);
        }
        JointKind::Revolute => {
            point(rows);
            rows.extend([dot1_row(a, ax, b, bz), dot1_row(a, ay, b, bz)]);
        }
        JointKind::Ball => point(rows),
        JointKind::Cylindrical => {
            rows.extend([dot2_row(a, ax, b), dot2_row(a, ay, b), dot1_row(a, ax, b, bz), dot1_row(a, ay, b, bz)]);
        }
        JointKind::Slider => {
            rows.extend([dot2_row(a, ax, b), dot2_row(a, ay, b), dot1_row(a, ax, b, bz), dot1_row(a, ay, b, bz), dot1_row(a, ax, b, by)]);
        }
        JointKind::Planar => {
            rows.extend([dot2_row(a, az, b), dot1_row(a, az, b, bx), dot1_row(a, az, b, by)]);
        }
    }
}

/// A joint's values and their rates (see [`JointKind::values`]).
fn joint_values(kind: JointKind, a: &MarkerWorld, b: &MarkerWorld) -> (Vec<f64>, Vec<f64>) {
    let [ax, ay, az] = a.axes;
    let bx = b.axes[0];
    let d = b.p - a.p;
    let dd = b.pdot - a.pdot;
    let angle = || bx.dot(ay).atan2(bx.dot(ax));
    let spin = || (b.w - a.w).dot(az);
    let along = |u: Vec3| (d.dot(u), dd.dot(u) + d.dot(a.w.cross(u)));
    match kind {
        JointKind::Rigid => (vec![], vec![]),
        JointKind::Revolute => (vec![angle()], vec![spin()]),
        JointKind::Slider => {
            let (s, r) = along(az);
            (vec![s], vec![r])
        }
        JointKind::Cylindrical => {
            let (s, r) = along(az);
            (vec![angle(), s], vec![spin(), r])
        }
        JointKind::Planar => {
            let ((x, xr), (y, yr)) = (along(ax), along(ay));
            (vec![x, y, angle()], vec![xr, yr, spin()])
        }
        JointKind::Ball => {
            // B's turn from A as a rotation vector in A's frame, and the relative angular
            // velocity in A's frame.
            let am = Mat3::from_cols(ax, ay, az);
            let rel = am.transpose().mul(&Mat3::from_cols(b.axes[0], b.axes[1], b.axes[2]));
            let rv = Quat::from_mat3(&rel).map_or(Vec3::ZERO, Quat::rotation_vector);
            let w = am.transpose().mul_vec(b.w - a.w);
            (vec![rv.x, rv.y, rv.z], vec![w.x, w.y, w.z])
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Finite-difference checks of the Jacobians and `J̇u` (tests only).
    use super::*;

    /// `(Φ, J u, J̇ u)` of every row at `y`.
    pub(crate) fn rows_at(sys: &System, y: &[f64]) -> Vec<(f64, f64, f64)> {
        let kin = sys.kinematics(y);
        sys.rows(&kin).iter().map(|r| (r.phi, r.rate(&kin), r.jdot_u)).collect()
    }

    /// The state moved along its velocities for `h` (positions and orientations only).
    pub(crate) fn drift(sys: &System, y: &[f64], h: f64) -> Vec<f64> {
        let mut out = y.to_vec();
        let kin = sys.kinematics(y);
        for (i, k) in kin.iter().enumerate() {
            let at = i * STRIDE;
            put3(&mut out, at, k.x + k.v * h);
            let q = Quat::from_rotation_vector(k.w * h).compose(quat(y, at + 3));
            put_quat(&mut out, at + 3, q);
        }
        out
    }
}
