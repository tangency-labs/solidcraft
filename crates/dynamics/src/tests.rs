//! Analytic checks: free fall, pendulum periods, spring-mass oscillation, energy and momentum
//! conservation, slider-crank kinematics, joint reactions, Jacobians against finite
//! differences, and hostile input.

use std::f64::consts::{PI, TAU};

use solidcraft_geom::Vec3;

use crate::system::{System, testing};
use crate::*;

const G: f64 = STANDARD_GRAVITY;

fn down() -> Vec3 {
    Vec3::new(0.0, 0.0, -G)
}

fn ok<T>(r: Result<T>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("simulation failed: {e}"),
    }
}

/// A body hanging `len` mm under a hinge at the origin turning about Y, at angle `theta` from
/// straight down (towards +X) and turning at `omega`.
fn pendulum(len: f64, theta: f64, omega: f64, inertia: f64) -> Model {
    let dir = Vec3::new(theta.sin(), 0.0, -theta.cos());
    // Turning about +Y takes −Z towards −X; the hinge's own Y axis is the world's.
    let rot = Quat::from_axis_angle(Vec3::Y, -theta);
    let mut body = Body::at_rest(1.0, Vec3::new(inertia, inertia, inertia), dir * len);
    body.orientation = rot;
    body.angular_velocity = Vec3::Y * -omega;
    body.velocity = (Vec3::Y * -omega).cross(dir * len);
    let joint = Joint {
        kind: JointKind::Revolute,
        a: Marker::ground(Vec3::ZERO, Vec3::Y, Vec3::X),
        b: Marker::new(Some(0), Vec3::new(0.0, 0.0, len), Vec3::Y, Vec3::X),
    };
    Model { bodies: vec![body], joints: vec![joint], forces: vec![], drives: vec![], links: vec![], gravity: down() }
}

/// Times where `f` crosses zero upwards, linearly interpolated between samples.
fn upward_crossings(traj: &Trajectory, f: impl Fn(&Sample) -> f64) -> Vec<f64> {
    traj.samples
        .windows(2)
        .filter_map(|w| {
            let (a, b) = (w.first()?, w.get(1)?);
            let (fa, fb) = (f(a), f(b));
            (fa < 0.0 && fb >= 0.0).then(|| a.time + (b.time - a.time) * fa / (fa - fb))
        })
        .collect()
}

fn mean_period(crossings: &[f64]) -> f64 {
    assert!(crossings.len() >= 3, "too few crossings: {crossings:?}");
    let (first, last) = (crossings.first().copied().unwrap_or(0.0), crossings.last().copied().unwrap_or(0.0));
    (last - first) / (crossings.len() - 1) as f64
}

fn angle_of(s: &Sample) -> f64 {
    s.joints.first().and_then(|j| j.values.first()).copied().unwrap_or(f64::NAN)
}

#[test]
fn free_fall_follows_the_parabola() {
    let mut body = Body::at_rest(2.5, Vec3::new(10.0, 20.0, 30.0), Vec3::new(1.0, 2.0, 3.0));
    body.velocity = Vec3::new(10.0, -5.0, 50.0);
    body.angular_velocity = Vec3::new(0.0, 0.0, 1.0);
    let model = Model { bodies: vec![body.clone()], joints: vec![], forces: vec![], drives: vec![], links: vec![], gravity: down() };
    let traj = ok(simulate(&model, &Settings::rk4(1.0, 0.01, 0.1)));
    assert_eq!(traj.samples.len(), 11);
    for s in &traj.samples {
        let t = s.time;
        let expect = body.position + body.velocity * t + down() * (0.5 * t * t);
        let got = s.bodies.first().map(|b| b.position).unwrap_or(Vec3::ZERO);
        assert!(got.dist(expect) < 1e-7, "t={t}: {got:?} vs {expect:?}");
        // A symmetric body spinning about z keeps spinning.
        let w = s.bodies.first().map(|b| b.angular_velocity).unwrap_or(Vec3::ZERO);
        assert!(w.dist(Vec3::Z) < 1e-12);
        // Energy is conserved exactly enough.
        let e0 = traj.samples.first().map(|s| s.energy.total()).unwrap_or(0.0);
        assert!((s.energy.total() - e0).abs() < 1e-6 * e0.abs().max(1.0));
    }
}

#[test]
fn pendulum_small_angle_period() {
    let (len, inertia) = (100.0, 40.0);
    let i_pivot = inertia + len * len;
    let period = TAU * (i_pivot / (G * len)).sqrt();
    let amplitude = 0.01;
    let model = pendulum(len, 0.0, amplitude * TAU / period, inertia);
    let traj = ok(simulate(&model, &Settings::rk4(3.0, 1e-3, 1e-3)));
    let measured = mean_period(&upward_crossings(&traj, angle_of));
    // The finite amplitude lengthens the period by about θ²/16 (6e-6 here).
    assert!((measured - period).abs() < 1e-4 * period, "period {measured} vs {period}");
    assert!(traj.stats.max_constraint_error < 1e-9, "{:?}", traj.stats);
}

#[test]
fn adaptive_integration_matches_the_pendulum_period() {
    let (len, inertia) = (100.0, 40.0);
    let period = TAU * ((inertia + len * len) / (G * len)).sqrt();
    let model = pendulum(len, 0.0, 0.01 * TAU / period, inertia);
    let traj = ok(simulate(&model, &Settings::adaptive(3.0, 1e-3, 1e-9, 1e-9)));
    let measured = mean_period(&upward_crossings(&traj, angle_of));
    assert!((measured - period).abs() < 1e-4 * period, "period {measured} vs {period}");
    // Larger tolerances take fewer steps than samples (the output still lands on each).
    let coarse = Settings {
        integrator: Integrator::Adaptive { rel_tol: 1e-6, abs_tol: 1e-6, min_step: 1e-9, max_step: 0.05 },
        ..Settings::rk4(3.0, 1.0, 0.05)
    };
    let traj = ok(simulate(&model, &coarse));
    assert_eq!(traj.samples.len(), 61);
    let measured = mean_period(&upward_crossings(&traj, angle_of));
    assert!((measured - period).abs() < 2e-3 * period, "coarse period {measured} vs {period}");
}

#[test]
fn hanging_body_pin_carries_its_weight() {
    let model = pendulum(100.0, 0.0, 0.0, 40.0);
    let traj = ok(simulate(&model, &Settings::rk4(0.1, 1e-3, 0.05)));
    for s in &traj.samples {
        let j = s.joints.first().map(|j| (j.force, j.torque)).unwrap_or_default();
        assert!(j.0.dist(Vec3::new(0.0, 0.0, G)) < 1e-6 * G, "force {:?}", j.0);
        assert!(j.1.len() < 1e-6 * G, "torque {:?}", j.1);
        assert!(angle_of(s).abs() < 1e-12);
    }
}

#[test]
fn large_swing_conserves_energy() {
    // Released from horizontal: a compound pendulum swinging through ±90°.
    let model = pendulum(100.0, PI / 2.0, 0.0, 400.0);
    let traj = ok(simulate(&model, &Settings::rk4(2.0, 1e-3, 1e-2)));
    let e0 = traj.samples.first().map(|s| s.energy.total()).unwrap_or(0.0);
    let scale = G * 100.0;
    for s in &traj.samples {
        assert!((s.energy.total() - e0).abs() < 1e-6 * scale, "t={} drift {}", s.time, s.energy.total() - e0);
    }
}

/// Two links hinged about Y, released from horizontal.
fn double_pendulum() -> Model {
    let link = |x: f64| Body::at_rest(1.0, Vec3::new(100.0, 900.0, 900.0), Vec3::new(x, 0.0, 0.0));
    let hinge = |a: Option<usize>, pa: Vec3, b: usize, pb: Vec3| Joint {
        kind: JointKind::Revolute,
        a: Marker::new(a, pa, Vec3::Y, Vec3::X),
        b: Marker::new(Some(b), pb, Vec3::Y, Vec3::X),
    };
    Model {
        bodies: vec![link(50.0), link(150.0)],
        joints: vec![
            hinge(None, Vec3::ZERO, 0, Vec3::new(-50.0, 0.0, 0.0)),
            hinge(Some(0), Vec3::new(50.0, 0.0, 0.0), 1, Vec3::new(-50.0, 0.0, 0.0)),
        ],
        forces: vec![],
        gravity: down(),
        ..Model::default()
    }
}

#[test]
fn double_pendulum_conserves_energy() {
    let traj = ok(simulate(&double_pendulum(), &Settings::rk4(5.0, 5e-4, 1e-2)));
    let e0 = traj.samples.first().map(|s| s.energy.total()).unwrap_or(0.0);
    let scale = 2.0 * G * 150.0;
    let worst = traj.samples.iter().map(|s| (s.energy.total() - e0).abs()).fold(0.0, f64::max);
    assert!(worst < 1e-5 * scale, "energy drift {worst} of {scale}");
    // It did swing hard: the lower link went well past vertical.
    let lowest = traj.samples.iter().filter_map(|s| s.bodies.get(1)).map(|b| b.position.z).fold(0.0, f64::min);
    assert!(lowest < -100.0, "{lowest}");
    assert_eq!(traj.stats.redundant_equations, 0);
}

#[test]
fn constraint_drift_stays_bounded_over_long_runs() {
    // Projection holds the joints to its tolerance (1e-10) however long the run.
    let traj = ok(simulate(&double_pendulum(), &Settings::rk4(20.0, 1e-3, 0.1)));
    assert!(traj.stats.max_constraint_error <= 1e-10, "{:?}", traj.stats);
    // Without projection, Baumgarte terms keep the drift bounded too, if less tightly.
    let settings = Settings { projection: false, baumgarte: Some((20.0, 20.0)), ..Settings::rk4(20.0, 1e-3, 0.1) };
    let traj = ok(simulate(&double_pendulum(), &settings));
    assert!(traj.stats.max_constraint_error < 1e-3, "{:?}", traj.stats);
}

#[test]
fn slider_crank_follows_its_closed_form() {
    let (r, l) = (50.0, 150.0);
    let theta0 = PI / 3.0;
    let pin = Vec3::new(r * theta0.cos(), 0.0, -r * theta0.sin());
    let s0 = r * theta0.cos() + (l * l - (r * theta0.sin()).powi(2)).sqrt();
    let slide = Vec3::new(s0, 0.0, 0.0);
    let u = (slide - pin) * (1.0 / l);
    let mut crank = Body::at_rest(1.0, Vec3::new(100.0, 2000.0, 2000.0), pin * 0.5);
    crank.orientation = Quat::from_axis_angle(Vec3::Y, theta0);
    crank.angular_velocity = Vec3::Y * TAU;
    crank.velocity = (Vec3::Y * TAU).cross(pin * 0.5);
    let mut rod = Body::at_rest(0.5, Vec3::new(50.0, 1000.0, 1000.0), (pin + slide) * 0.5);
    rod.orientation = Quat::from_axis_angle(Vec3::Y, (-u.z).atan2(u.x));
    let slider = Body::at_rest(0.3, Vec3::new(50.0, 50.0, 50.0), slide);
    let hinge = |a: Option<usize>, pa: Vec3, b: usize, pb: Vec3| Joint {
        kind: JointKind::Revolute,
        a: Marker::new(a, pa, Vec3::Y, Vec3::X),
        b: Marker::new(Some(b), pb, Vec3::Y, Vec3::X),
    };
    let model = Model {
        bodies: vec![crank, rod, slider],
        joints: vec![
            hinge(None, Vec3::ZERO, 0, Vec3::new(-r / 2.0, 0.0, 0.0)),
            hinge(Some(0), Vec3::new(r / 2.0, 0.0, 0.0), 1, Vec3::new(-l / 2.0, 0.0, 0.0)),
            hinge(Some(1), Vec3::new(l / 2.0, 0.0, 0.0), 2, Vec3::ZERO),
            Joint { kind: JointKind::Slider, a: Marker::ground(Vec3::ZERO, Vec3::X, Vec3::Y), b: Marker::new(Some(2), Vec3::ZERO, Vec3::X, Vec3::Y) },
        ],
        forces: vec![],
        gravity: Vec3::ZERO,
        ..Model::default()
    };
    let traj = ok(simulate(&model, &Settings::rk4(2.0, 1e-3, 1e-2)));
    // A planar linkage built from 3D joints repeats three equations; they are solved, not refused.
    assert_eq!(traj.stats.redundant_equations, 3);
    let mut turned = 0.0_f64;
    let mut last = theta0;
    for s in &traj.samples {
        let theta = angle_of(s);
        let x = s.joints.get(3).and_then(|j| j.values.first()).copied().unwrap_or(f64::NAN);
        let expect = r * theta.cos() + (l * l - (r * theta.sin()).powi(2)).sqrt();
        assert!((x - expect).abs() < 1e-6, "t={}: slider {x} vs {expect}", s.time);
        let step = (theta - last + PI).rem_euclid(TAU) - PI;
        turned += step;
        last = theta;
    }
    assert!(turned.abs() > TAU, "the crank turned {turned} rad");
    assert!(traj.stats.max_constraint_error < 1e-9, "{:?}", traj.stats);
    // No forces: kinetic energy is conserved.
    let e0 = traj.samples.first().map(|s| s.energy.kinetic).unwrap_or(0.0);
    assert!(traj.samples.iter().all(|s| (s.energy.kinetic - e0).abs() < 1e-6 * e0), "kinetic energy drifted");
}

/// A body on a slider along X, pulled toward the origin by a spring of free length 40 mm,
/// starting 10 mm stretched, with gravity across the slide.
fn spring_mass(damping: f64) -> Model {
    Model {
        bodies: vec![Body::at_rest(2.0, Vec3::new(500.0, 500.0, 500.0), Vec3::new(50.0, 0.0, 0.0))],
        joints: vec![Joint {
            kind: JointKind::Slider,
            a: Marker::ground(Vec3::ZERO, Vec3::X, Vec3::Y),
            b: Marker::new(Some(0), Vec3::ZERO, Vec3::X, Vec3::Y),
        }],
        forces: vec![Force::Spring {
            a: Anchor { body: None, point: Vec3::ZERO },
            b: Anchor { body: Some(0), point: Vec3::ZERO },
            stiffness: 800.0,
            damping,
            free_length: 40.0,
            preload: 0.0,
        }],
        gravity: down(),
        ..Model::default()
    }
}

#[test]
fn spring_mass_oscillates_at_its_natural_frequency() {
    // ω = √(k/m) = 20 rad/s.
    let traj = ok(simulate(&spring_mass(0.0), &Settings::rk4(1.0, 1e-4, 1e-3)));
    for s in &traj.samples {
        let x = s.bodies.first().map(|b| b.position.x).unwrap_or(f64::NAN);
        let expect = 40.0 + 10.0 * (20.0 * s.time).cos();
        assert!((x - expect).abs() < 1e-6, "t={}: {x} vs {expect}", s.time);
        // The slide holds the body up against gravity.
        let f = s.joints.first().map(|j| j.force).unwrap_or_default();
        assert!((f.z - 2.0 * G).abs() < 1e-6 * G && f.x.abs() < 1e-6 * G, "reaction {f:?}");
    }
    let e0 = traj.samples.first().map(|s| s.energy.total()).unwrap_or(0.0);
    assert!(traj.samples.iter().all(|s| (s.energy.total() - e0).abs() < 1e-8 * 800.0 * 100.0));
}

#[test]
fn spring_damper_decays_as_the_closed_form() {
    // c = 8 kg/s: ζ = c / (2√(km)) = 0.1.
    let (wn, zeta) = (20.0_f64, 0.1_f64);
    let wd = wn * (1.0 - zeta * zeta).sqrt();
    let traj = ok(simulate(&spring_mass(8.0), &Settings::rk4(1.0, 1e-4, 1e-3)));
    for s in &traj.samples {
        let t = s.time;
        let x = s.bodies.first().map(|b| b.position.x).unwrap_or(f64::NAN);
        let expect = 40.0 + (-zeta * wn * t).exp() * 10.0 * ((wd * t).cos() + zeta * wn / wd * (wd * t).sin());
        assert!((x - expect).abs() < 1e-6, "t={t}: {x} vs {expect}");
    }
}

#[test]
fn torque_free_body_keeps_energy_and_momentum() {
    let mut body = Body::at_rest(1.0, Vec3::new(1000.0, 2000.0, 3000.0), Vec3::ZERO);
    body.orientation = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.7);
    body.angular_velocity = Vec3::new(1.0, 0.3, 5.0);
    let model = Model { bodies: vec![body], joints: vec![], forces: vec![], drives: vec![], links: vec![], gravity: Vec3::ZERO };
    let traj = ok(simulate(&model, &Settings::rk4(2.0, 1e-4, 1e-2)));
    let momentum = |s: &Sample| {
        let b = s.bodies.first().copied();
        b.map_or(Vec3::ZERO, |b| {
            let r = b.orientation.to_mat3();
            r.mul(&Mat3::diagonal(Vec3::new(1000.0, 2000.0, 3000.0))).mul(&r.transpose()).mul_vec(b.angular_velocity)
        })
    };
    let (l0, e0) = traj.samples.first().map(|s| (momentum(s), s.energy.kinetic)).unwrap_or_default();
    let mut tumbled = false;
    for s in &traj.samples {
        assert!(momentum(s).dist(l0) < 1e-7 * l0.len(), "t={} momentum {:?} vs {l0:?}", s.time, momentum(s));
        assert!((s.energy.kinetic - e0).abs() < 1e-7 * e0);
        let w = s.bodies.first().map(|b| b.angular_velocity).unwrap_or_default();
        tumbled |= w.dist(Vec3::new(1.0, 0.3, 5.0)) > 0.1;
    }
    assert!(tumbled, "the angular velocity should precess");
}

#[test]
fn spherical_pendulum_keeps_energy_and_vertical_momentum() {
    // A body on a ball joint, tilted and pushed sideways: it swings round the vertical.
    let com = Vec3::new(60.0, 0.0, -80.0);
    let mut body = Body::at_rest(1.5, Vec3::new(300.0, 500.0, 700.0), com);
    body.velocity = Vec3::new(0.0, 400.0, 0.0);
    let model = Model {
        bodies: vec![body],
        joints: vec![Joint {
            kind: JointKind::Ball,
            a: Marker::ground(Vec3::ZERO, Vec3::Z, Vec3::X),
            b: Marker::new(Some(0), -com, Vec3::Z, Vec3::X),
        }],
        forces: vec![],
        gravity: down(),
        ..Model::default()
    };
    let traj = ok(simulate(&model, &Settings::rk4(3.0, 5e-4, 1e-2)));
    let lz = |s: &Sample| {
        s.bodies.first().map_or(0.0, |b| {
            let r = b.orientation.to_mat3();
            let iw = r.mul(&Mat3::diagonal(Vec3::new(300.0, 500.0, 700.0))).mul(&r.transpose());
            (b.position.cross(b.velocity * 1.5) + iw.mul_vec(b.angular_velocity)).z
        })
    };
    let (l0, e0) = traj.samples.first().map(|s| (lz(s), s.energy.total())).unwrap_or_default();
    assert!(l0.abs() > 1e3);
    for s in &traj.samples {
        assert!((lz(s) - l0).abs() < 1e-6 * l0.abs(), "t={}: Lz {} vs {l0}", s.time, lz(s));
        assert!((s.energy.total() - e0).abs() < 1e-6 * G * 100.0, "t={}: energy drift {}", s.time, s.energy.total() - e0);
        // The joint holds the body's pivot at the origin.
        let pivot = s.bodies.first().map(|b| b.position + b.orientation.rotate(-com)).unwrap_or_default();
        assert!(pivot.len() < 1e-9, "{pivot:?}");
    }
}

/// Every joint kind between two moving bodies and between the ground and a body: the
/// Jacobian and `J̇u` agree with finite differences of `Φ` along the motion.
#[test]
fn jacobians_match_finite_differences() {
    let kinds = [JointKind::Rigid, JointKind::Revolute, JointKind::Slider, JointKind::Cylindrical, JointKind::Planar, JointKind::Ball];
    let mut a = Body::at_rest(1.0, Vec3::new(1.0, 2.0, 3.0), Vec3::new(10.0, -20.0, 5.0));
    a.orientation = Quat::from_axis_angle(Vec3::new(0.3, -1.0, 0.4), 0.9);
    a.velocity = Vec3::new(3.0, -1.0, 2.0);
    a.angular_velocity = Vec3::new(0.7, -0.4, 1.1);
    let mut b = Body::at_rest(2.0, Vec3::new(3.0, 2.0, 1.0), Vec3::new(-5.0, 15.0, 30.0));
    b.orientation = Quat::from_axis_angle(Vec3::new(1.0, 0.2, -0.5), -1.3);
    b.velocity = Vec3::new(-2.0, 4.0, 1.0);
    b.angular_velocity = Vec3::new(-0.5, 1.3, 0.2);
    for kind in kinds {
        for a_body in [Some(0), None] {
            let joint = Joint {
                kind,
                a: Marker::new(a_body, Vec3::new(4.0, -3.0, 7.0), Vec3::new(0.2, 0.5, 1.0), Vec3::new(1.0, 0.0, 0.3)),
                b: Marker::new(Some(1), Vec3::new(-6.0, 2.0, 1.0), Vec3::new(-0.4, 1.0, 0.1), Vec3::new(0.0, 0.3, 1.0)),
            };
            let model =
                Model { bodies: vec![a.clone(), b.clone()], joints: vec![joint], forces: vec![], drives: vec![], links: vec![], gravity: Vec3::ZERO };
            let sys = ok(System::new(&model, None));
            let y = ok(sys.initial_state(&model));
            let h = 1e-4;
            let (now, plus, minus) = (
                testing::rows_at(&sys, &y),
                testing::rows_at(&sys, &testing::drift(&sys, &y, h)),
                testing::rows_at(&sys, &testing::drift(&sys, &y, -h)),
            );
            assert_eq!(now.len(), kind.equations());
            for ((phi, ju, jdu), ((p, _, _), (m, _, _))) in now.iter().zip(plus.iter().zip(&minus)) {
                let rate = (p - m) / (2.0 * h);
                let accel = (p - 2.0 * phi + m) / (h * h);
                assert!((rate - ju).abs() < 1e-5 * (1.0 + ju.abs()), "{kind:?} {a_body:?}: J u {ju} vs {rate}");
                assert!((accel - jdu).abs() < 1e-3 * (1.0 + jdu.abs()), "{kind:?} {a_body:?}: J̇ u {jdu} vs {accel}");
            }
        }
    }
}

#[test]
fn joint_values_read_the_relative_motion() {
    // A body on each joint kind, displaced along its free directions, reads them back.
    let place = |kind: JointKind, offset: Vec3, turn: f64| {
        let mut body = Body::at_rest(1.0, Vec3::new(1.0, 1.0, 1.0), offset);
        body.orientation = Quat::from_axis_angle(Vec3::Z, turn);
        let model = Model {
            bodies: vec![body],
            joints: vec![Joint { kind, a: Marker::ground(Vec3::ZERO, Vec3::Z, Vec3::X), b: Marker::new(Some(0), Vec3::ZERO, Vec3::Z, Vec3::X) }],
            forces: vec![],
            gravity: Vec3::ZERO,
            ..Model::default()
        };
        let traj = ok(simulate(&model, &Settings::rk4(0.01, 0.01, 0.01)));
        traj.samples.first().and_then(|s| s.joints.first()).map(|j| j.values.clone()).unwrap_or_default()
    };
    let close = |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9);
    assert!(close(&place(JointKind::Revolute, Vec3::ZERO, 0.4), &[0.4]));
    assert!(close(&place(JointKind::Slider, Vec3::new(0.0, 0.0, 12.0), 0.0), &[12.0]));
    assert!(close(&place(JointKind::Cylindrical, Vec3::new(0.0, 0.0, -3.0), -1.0), &[-1.0, -3.0]));
    assert!(close(&place(JointKind::Planar, Vec3::new(5.0, -7.0, 0.0), 2.0), &[5.0, -7.0, 2.0]));
    assert!(close(&place(JointKind::Ball, Vec3::ZERO, 0.5), &[0.0, 0.0, 0.5]));
    assert!(close(&place(JointKind::Rigid, Vec3::ZERO, 0.0), &[]));
}

#[test]
fn inconsistent_start_is_projected_onto_the_joints() {
    // The pendulum's body starts 2 mm off its hinge and moving across it.
    let mut model = pendulum(100.0, 0.3, 0.0, 40.0);
    if let Some(b) = model.bodies.first_mut() {
        b.position += Vec3::new(1.0, 2.0, -1.0);
        b.velocity = Vec3::new(0.0, 50.0, 0.0);
    }
    let traj = ok(simulate(&model, &Settings::rk4(0.5, 1e-3, 0.05)));
    assert!(traj.stats.max_constraint_error < 1e-9, "{:?}", traj.stats);
    let y = traj.samples.first().and_then(|s| s.bodies.first()).map(|b| b.velocity.y).unwrap_or(f64::NAN);
    assert!(y.abs() < 1e-9, "velocity along the hinge is removed: {y}");
}

#[test]
fn hostile_input_is_an_error_not_a_panic() {
    let good = pendulum(100.0, 0.2, 0.0, 40.0);
    let settings = Settings::rk4(0.1, 1e-3, 1e-2);
    let mut cases: Vec<(Model, Settings)> = Vec::new();
    let mut with = |f: &dyn Fn(&mut Model)| {
        let mut m = good.clone();
        f(&mut m);
        cases.push((m, settings));
    };
    with(&|m| m.bodies.clear());
    with(&|m| m.bodies.iter_mut().for_each(|b| b.mass = f64::NAN));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.mass = 0.0));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.mass = -1.0));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.inertia = Mat3::diagonal(Vec3::new(1.0, -1.0, 1.0))));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.inertia = Mat3([[1.0, 5.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.inertia = Mat3::ZERO));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.position.x = f64::INFINITY));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.orientation = Quat { w: 0.0, x: 0.0, y: 0.0, z: 0.0 }));
    with(&|m| m.bodies.iter_mut().for_each(|b| b.velocity.z = 1e300));
    with(&|m| m.gravity = Vec3::new(f64::NAN, 0.0, 0.0));
    with(&|m| m.joints.iter_mut().for_each(|j| j.b.body = Some(7)));
    with(&|m| m.joints.iter_mut().for_each(|j| j.a.body = j.b.body));
    with(&|m| m.joints.iter_mut().for_each(|j| j.a.z = Vec3::ZERO));
    with(&|m| m.joints.iter_mut().for_each(|j| j.b.x = j.b.z));
    with(&|m| m.forces.push(Force::Load { body: 3, point: Vec3::ZERO, force: Vec3::X, torque: Vec3::ZERO }));
    with(&|m| {
        m.forces.push(Force::Spring {
            a: Anchor { body: None, point: Vec3::ZERO },
            b: Anchor { body: Some(0), point: Vec3::ZERO },
            stiffness: -1.0,
            damping: 0.0,
            free_length: 0.0,
            preload: 0.0,
        })
    });
    with(&|m| {
        m.forces.push(Force::Spring {
            a: Anchor { body: Some(9), point: Vec3::ZERO },
            b: Anchor { body: Some(0), point: Vec3::ZERO },
            stiffness: 1.0,
            damping: 0.0,
            free_length: 0.0,
            preload: 0.0,
        })
    });
    with(&|m| m.bodies = vec![Body::at_rest(1.0, Vec3::new(1.0, 1.0, 1.0), Vec3::ZERO); MAX_BODIES + 1]);
    with(&|m| {
        m.joints = vec![
            m.joints.first().copied().unwrap_or(Joint {
                kind: JointKind::Ball,
                a: Marker::ground(Vec3::ZERO, Vec3::Z, Vec3::X),
                b: Marker::new(Some(0), Vec3::ZERO, Vec3::Z, Vec3::X)
            });
            MAX_JOINTS + 1
        ]
    });
    let bad_settings = [
        Settings::rk4(0.0, 1e-3, 1e-2),
        Settings::rk4(-1.0, 1e-3, 1e-2),
        Settings::rk4(f64::NAN, 1e-3, 1e-2),
        Settings::rk4(1.0, 0.0, 1e-2),
        Settings::rk4(1.0, f64::INFINITY, 1e-2),
        Settings::rk4(1.0, 1e-3, 0.0),
        Settings::rk4(1e6, 1e-6, 1e-2),
        Settings::rk4(1e3, 1e-3, 1e-6),
        Settings { projection_tol: -1.0, ..settings },
        Settings { baumgarte: Some((f64::NAN, 1.0)), ..settings },
        Settings { integrator: Integrator::Adaptive { rel_tol: 1e-6, abs_tol: 1e-6, min_step: 1.0, max_step: 0.1 }, ..settings },
        Settings { integrator: Integrator::Adaptive { rel_tol: 0.0, abs_tol: 1e-6, min_step: 1e-9, max_step: 0.1 }, ..settings },
    ];
    cases.extend(bad_settings.into_iter().map(|s| (good.clone(), s)));
    for (i, (m, s)) in cases.iter().enumerate() {
        assert!(simulate(m, s).is_err(), "case {i} should be refused");
    }
    // Inputs that are valid but extreme stop with an error or finish; they never panic.
    let stiff = Model {
        forces: vec![Force::Spring {
            a: Anchor { body: None, point: Vec3::ZERO },
            b: Anchor { body: Some(0), point: Vec3::ZERO },
            stiffness: 1e11,
            damping: 0.0,
            free_length: 0.0,
            preload: 0.0,
        }],
        ..good.clone()
    };
    let _ = simulate(&stiff, &Settings::rk4(0.1, 1e-2, 1e-2));
    let tight = Settings { integrator: Integrator::Adaptive { rel_tol: 1e-14, abs_tol: 1e-14, min_step: 1e-3, max_step: 1e-3 }, ..settings };
    let _ = simulate(&stiff, &tight);
}

#[test]
fn models_round_trip_through_json() {
    let mut model = spring_mass(1.5);
    model.forces.push(Force::Load { body: 0, point: Vec3::new(1.0, 0.0, 0.0), force: Vec3::new(0.0, 5.0, 0.0), torque: Vec3::ZERO });
    model.forces.push(Force::JointSpring { at: Coord::new(0, 0), stiffness: 3.0, damping: 0.5, rest: 1.0 });
    model.forces.push(Force::Motor { at: Coord::new(0, 0), effort: Profile::Table { points: vec![(0.0, 1.0), (2.0, 3.0)] } });
    model.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Ramp { from: 0.0, to: 1.0, start: 0.5, duration: 2.0 } });
    model.links.push(Link { driver: Coord::new(0, 0), follower: Coord::new(1, 0), ratio: 2.0, offset: Some(0.5) });
    let text = match serde_json::to_string(&model) {
        Ok(t) => t,
        Err(e) => panic!("{e}"),
    };
    let back: Model = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(back, model);
    let settings: Settings = match serde_json::from_str(r#"{"duration":1.0,"output_interval":0.1,"integrator":{"type":"rk4","step":0.001}}"#) {
        Ok(s) => s,
        Err(e) => panic!("{e}"),
    };
    assert!(settings.projection && settings.validate().is_ok());
}

// Joint springs, motors, drives and motion links.

/// `a − b` turned into (−π, π].
fn wrap(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

/// A wheel turning about Z on a revolute to the ground at `at`, its centre of mass on the axis.
/// Wheels turning about Z on revolutes to the ground, centres of mass on their axes (inertia,
/// centre); joint `i` holds body `i`.
fn rotors(parts: &[(f64, Vec3)]) -> Model {
    let mut model = Model::default();
    for (i, (inertia, at)) in parts.iter().enumerate() {
        model.bodies.push(Body::at_rest(1.0, Vec3::new(*inertia, *inertia, *inertia), *at));
        model.joints.push(Joint {
            kind: JointKind::Revolute,
            a: Marker::ground(*at, Vec3::Z, Vec3::X),
            b: Marker::new(Some(i), Vec3::ZERO, Vec3::Z, Vec3::X),
        });
    }
    model
}

fn value(s: &Sample, joint: usize, dof: usize) -> f64 {
    s.joints.get(joint).and_then(|j| j.values.get(dof)).copied().unwrap_or(f64::NAN)
}

#[test]
fn profiles_give_consistent_derivatives() {
    let profiles = [
        Profile::Constant { value: 2.0 },
        Profile::Linear { value: 1.0, rate: -3.0 },
        Profile::Ramp { from: 0.5, to: 2.0, start: 0.2, duration: 0.6 },
        Profile::Harmonic { offset: 1.0, amplitude: 0.3, frequency: 2.0, phase: 0.4 },
    ];
    let h = 1e-5;
    for p in &profiles {
        for k in 0..100 {
            let t = k as f64 * 0.0123;
            let (f, fd, fdd) = p.eval(t);
            let (fp, fdp, _) = p.eval(t + h);
            let (fm, fdm, _) = p.eval(t - h);
            assert!((fd - (fp - fm) / (2.0 * h)).abs() < 1e-5 * (1.0 + fd.abs()), "{p:?} at {t}: ḟ");
            assert!((fdd - (fdp - fdm) / (2.0 * h)).abs() < 1e-3 * (1.0 + fdd.abs()), "{p:?} at {t}: f̈");
            assert!(f.is_finite());
        }
    }
    let ramp = Profile::Ramp { from: 0.5, to: 2.0, start: 0.2, duration: 0.6 };
    assert_eq!(ramp.eval(0.0).0, 0.5);
    assert_eq!(ramp.eval(0.8).0, 2.0);
    assert!((ramp.eval(0.5).0 - 1.25).abs() < 1e-12);
    let table = Profile::Table { points: vec![(0.0, 1.0), (1.0, 3.0), (2.0, 0.0)] };
    assert_eq!(table.eval(-1.0), (1.0, 0.0, 0.0));
    assert_eq!(table.eval(0.5), (2.0, 2.0, 0.0));
    assert_eq!(table.eval(1.5), (1.5, -3.0, 0.0));
    assert_eq!(table.eval(9.0), (0.0, 0.0, 0.0));
}

#[test]
fn driven_pendulum_reaches_its_commanded_angle_and_holds_it() {
    // A drive lifts the pendulum from hanging to horizontal along a cycloidal ramp in 1 s,
    // then holds it: the angle follows the ramp, and the holding torque is m·g·L.
    let (len, mass) = (100.0, 1.0);
    let mut model = pendulum(len, 0.0, 0.0, 40.0);
    let ramp = Profile::Ramp { from: 0.0, to: PI / 2.0, start: 0.0, duration: 1.0 };
    model.drives.push(Drive { at: Coord::new(0, 0), motion: ramp.clone() });
    let traj = ok(simulate(&model, &Settings::rk4(1.5, 1e-3, 1e-2)));
    let b0 = traj.samples.first().map(|s| s.energy.balance()).unwrap_or(f64::NAN);
    for s in &traj.samples {
        let angle = value(s, 0, 0);
        assert!((angle - ramp.eval(s.time).0).abs() < 1e-9, "t={}: angle {angle} vs {}", s.time, ramp.eval(s.time).0);
        // Work put in = kinetic + gravity energy gained.
        assert!((s.energy.balance() - b0).abs() < 1e-6 * G * len, "t={}: balance {}", s.time, s.energy.balance() - b0);
    }
    let held: Vec<&Sample> = traj.samples.iter().filter(|s| s.time >= 1.0).collect();
    assert!(held.len() > 40);
    for s in held {
        let effort = s.drives.first().copied().unwrap_or(f64::NAN);
        assert!((effort.abs() - mass * G * len).abs() < 1e-6 * G * len, "t={}: holding torque {effort}", s.time);
        assert!((s.energy.input - mass * G * len).abs() < 1e-6 * G * len, "work put in {}", s.energy.input);
        assert!(s.energy.kinetic < 1e-9 * G * len);
    }
}

#[test]
fn driven_speed_counts_whole_turns() {
    // 10 rad/s for 2 s: more than three turns; the driven angle never jumps a turn.
    let mut model = rotors(&[(500.0, Vec3::ZERO)]);
    model.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Linear { value: 0.0, rate: 10.0 } });
    let traj = ok(simulate(&model, &Settings::rk4(2.0, 1e-3, 1e-2)));
    for s in &traj.samples {
        assert!(wrap(value(s, 0, 0) - 10.0 * s.time).abs() < 1e-9, "t={}", s.time);
        let w = s.bodies.first().map(|b| b.angular_velocity.z).unwrap_or(f64::NAN);
        assert!((w - 10.0).abs() < 1e-9);
        assert!(s.drives.first().is_some_and(|e| e.abs() < 1e-6), "{:?}", s.drives);
    }
    assert!(traj.stats.max_constraint_error < 1e-9);
}

#[test]
fn torsion_spring_period() {
    // ω = √(k/I) = √(5e5 / 500) rad/s; released from 0.3 rad.
    let (inertia, k) = (500.0_f64, 5e5_f64);
    let w = (k / inertia).sqrt();
    let mut model = rotors(&[(inertia, Vec3::ZERO)]);
    if let Some(b) = model.bodies.first_mut() {
        b.orientation = Quat::from_axis_angle(Vec3::Z, 0.3);
    }
    model.forces.push(Force::JointSpring { at: Coord::new(0, 0), stiffness: k, damping: 0.0, rest: 0.0 });
    let traj = ok(simulate(&model, &Settings::rk4(1.0, 1e-4, 1e-3)));
    for s in &traj.samples {
        let expect = 0.3 * (w * s.time).cos();
        assert!((value(s, 0, 0) - expect).abs() < 1e-7, "t={}: {} vs {expect}", s.time, value(s, 0, 0));
    }
    let measured = mean_period(&upward_crossings(&traj, |s| value(s, 0, 0)));
    assert!((measured - TAU / w).abs() < 1e-5 * TAU / w);
    let e0 = traj.samples.first().map(|s| s.energy.total()).unwrap_or(0.0);
    assert!((e0 - 0.5 * k * 0.09).abs() < 1e-9 * e0);
    // With damping the energy left plus the energy dissipated stays what it was.
    if let Some(Force::JointSpring { damping, .. }) = model.forces.first_mut() {
        *damping = 2.0 * 0.05 * (k * inertia).sqrt();
    }
    let traj = ok(simulate(&model, &Settings::rk4(1.0, 1e-4, 1e-2)));
    let last = traj.samples.last().map(|s| s.energy).unwrap_or_default();
    assert!(last.total() < 0.2 * e0 && last.dissipated > 0.8 * e0);
    assert!(traj.samples.iter().all(|s| (s.energy.balance() - e0).abs() < 1e-7 * e0));
}

#[test]
fn gear_pair_keeps_its_ratio_and_motor_work_is_kinetic_energy() {
    // A motor's constant torque τ on gear 1; gear 2 turns at −r₁/r₂ = −0.5 of it. The pair
    // accelerates as one inertia I₁ + 0.25·I₂: θ₁ = ½(τ/I_eff)t², and the motor's work τ·θ₁ is
    // all kinetic energy. Over 3 s gear 1 makes more than half a turn past ±π.
    let (i1, i2, ratio, tau) = (1000.0, 2000.0, -0.5, 1500.0);
    let alpha = tau / (i1 + ratio * ratio * i2);
    let mut model = rotors(&[(i1, Vec3::ZERO), (i2, Vec3::new(30.0, 0.0, 0.0))]);
    model.forces.push(Force::Motor { at: Coord::new(0, 0), effort: Profile::Constant { value: tau } });
    model.links.push(Link { driver: Coord::new(0, 0), follower: Coord::new(1, 0), ratio, offset: None });
    let traj = ok(simulate(&model, &Settings::rk4(3.0, 1e-3, 1e-2)));
    for s in &traj.samples {
        let theta = 0.5 * alpha * s.time * s.time;
        assert!(wrap(value(s, 0, 0) - theta).abs() < 1e-8, "t={}: gear 1 {} vs {theta}", s.time, value(s, 0, 0));
        assert!(wrap(value(s, 1, 0) - ratio * theta).abs() < 1e-8, "t={}: gear 2", s.time);
        assert!((s.energy.input - tau * theta).abs() < 1e-6 * tau * 4.5, "t={}: work {}", s.time, s.energy.input);
        assert!((s.energy.kinetic - s.energy.input).abs() < 1e-6 * tau * 4.5, "t={}: kinetic {}", s.time, s.energy.kinetic);
    }
    assert!(0.5 * alpha * 9.0 > PI);
}

#[test]
fn rack_and_pinion_follow_the_pinion_radius() {
    // A pinion of radius 20 mm drives a rack on a slider along X; a harmonic motor torque. The
    // rack's travel is 20·θ, and the motor's work is the kinetic energy of both.
    let mut model = rotors(&[(800.0, Vec3::ZERO)]);
    model.bodies.push(Body::at_rest(0.5, Vec3::new(100.0, 100.0, 100.0), Vec3::new(0.0, 20.0, 0.0)));
    model.joints.push(Joint {
        kind: JointKind::Slider,
        a: Marker::ground(Vec3::new(0.0, 20.0, 0.0), Vec3::X, Vec3::Y),
        b: Marker::new(Some(1), Vec3::ZERO, Vec3::X, Vec3::Y),
    });
    model
        .forces
        .push(Force::Motor { at: Coord::new(0, 0), effort: Profile::Harmonic { offset: 0.0, amplitude: 2000.0, frequency: 1.0, phase: 0.0 } });
    model.links.push(Link { driver: Coord::new(0, 0), follower: Coord::new(1, 0), ratio: 20.0, offset: None });
    let traj = ok(simulate(&model, &Settings::rk4(2.0, 1e-3, 1e-2)));
    // I_eff = 800 + 0.5·20²; θ̈ = τ(t)/I_eff with τ = A sin(2πt): θ = A/I_eff·(t/2π − sin(2πt)/4π²).
    let i_eff = 800.0 + 0.5 * 400.0;
    for s in &traj.samples {
        let t = s.time;
        let theta = 2000.0 / i_eff * (t / TAU - (TAU * t).sin() / (TAU * TAU));
        assert!((value(s, 0, 0) - theta).abs() < 1e-8, "t={t}: pinion {} vs {theta}", value(s, 0, 0));
        assert!((value(s, 1, 0) - 20.0 * theta).abs() < 1e-7, "t={t}: rack {}", value(s, 1, 0));
        assert!((s.energy.kinetic - s.energy.input).abs() < 1e-8 * 2000.0, "t={t}: {:?}", s.energy);
    }
}

#[test]
fn screw_turns_as_it_falls() {
    // A nut on a vertical cylindrical joint, its travel tied to its turn by a 5 mm pitch, falls
    // under gravity: z̈ = −m·g·k² / (I + m·k²) with k = pitch / 2π.
    let (mass, inertia, k) = (0.2, 50.0, 5.0 / TAU);
    let model = Model {
        bodies: vec![Body::at_rest(mass, Vec3::new(inertia, inertia, inertia), Vec3::ZERO)],
        joints: vec![Joint {
            kind: JointKind::Cylindrical,
            a: Marker::ground(Vec3::ZERO, Vec3::Z, Vec3::X),
            b: Marker::new(Some(0), Vec3::ZERO, Vec3::Z, Vec3::X),
        }],
        links: vec![Link { driver: Coord::new(0, 0), follower: Coord::new(0, 1), ratio: k, offset: None }],
        gravity: down(),
        ..Model::default()
    };
    let traj = ok(simulate(&model, &Settings::rk4(1.0, 1e-3, 1e-2)));
    let a = -mass * G * k * k / (inertia + mass * k * k);
    for s in &traj.samples {
        let z = s.bodies.first().map(|b| b.position.z).unwrap_or(f64::NAN);
        assert!((z - 0.5 * a * s.time * s.time).abs() < 1e-7, "t={}: z {z}", s.time);
        assert!(wrap(value(s, 0, 0) - z / k).abs() < 1e-8, "t={}: turn", s.time);
        assert!(s.energy.balance().abs() < 1e-6 * mass * G, "t={}: {:?}", s.time, s.energy);
    }
    // It went down more than one turn.
    assert!(0.5 * a < -5.0);
}

#[test]
fn hostile_springs_motors_drives_and_links_are_errors() {
    let good = rotors(&[(500.0, Vec3::ZERO), (500.0, Vec3::new(30.0, 0.0, 0.0))]);
    let mut bad: Vec<Model> = Vec::new();
    let mut with = |f: &dyn Fn(&mut Model)| {
        let mut m = good.clone();
        f(&mut m);
        bad.push(m);
    };
    with(&|m| m.drives.push(Drive { at: Coord::new(5, 0), motion: Profile::Constant { value: 0.0 } }));
    with(&|m| m.drives.push(Drive { at: Coord::new(0, 1), motion: Profile::Constant { value: 0.0 } }));
    with(&|m| m.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Constant { value: f64::NAN } }));
    with(&|m| m.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Ramp { from: 0.0, to: 1.0, start: 0.0, duration: 0.0 } }));
    with(&|m| m.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Table { points: vec![] } }));
    with(&|m| m.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Table { points: vec![(1.0, 0.0), (0.5, 1.0)] } }));
    with(&|m| m.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Table { points: vec![(0.0, 0.0); MAX_PROFILE_POINTS + 1] } }));
    with(&|m| m.forces.push(Force::JointSpring { at: Coord::new(0, 0), stiffness: -1.0, damping: 0.0, rest: 0.0 }));
    with(&|m| m.forces.push(Force::JointSpring { at: Coord::new(0, 0), stiffness: 1.0, damping: f64::INFINITY, rest: 0.0 }));
    with(&|m| m.forces.push(Force::Motor { at: Coord::new(9, 0), effort: Profile::Constant { value: 1.0 } }));
    with(&|m| {
        m.forces
            .push(Force::Motor { at: Coord::new(0, 0), effort: Profile::Harmonic { offset: 0.0, amplitude: 1.0, frequency: f64::NAN, phase: 0.0 } })
    });
    with(&|m| m.links.push(Link { driver: Coord::new(0, 0), follower: Coord::new(0, 0), ratio: 1.0, offset: None }));
    with(&|m| m.links.push(Link { driver: Coord::new(0, 0), follower: Coord::new(1, 0), ratio: f64::NAN, offset: None }));
    with(&|m| m.links.push(Link { driver: Coord::new(0, 0), follower: Coord::new(1, 0), ratio: 1.0, offset: Some(f64::INFINITY) }));
    with(&|m| m.links = vec![Link { driver: Coord::new(0, 0), follower: Coord::new(1, 0), ratio: 1.0, offset: None }; MAX_JOINTS + 1]);
    with(&|m| {
        // Ball and rigid joints have no value to act on yet.
        if let Some(j) = m.joints.get_mut(0) {
            j.kind = JointKind::Ball;
        }
        m.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Constant { value: 0.0 } });
    });
    for (i, m) in bad.iter().enumerate() {
        assert!(simulate(m, &Settings::rk4(0.1, 1e-3, 1e-2)).is_err(), "case {i} should be refused");
    }
    // Links that fight each other, and drives that can't be met, stop or finish but never panic.
    let mut locked = good.clone();
    locked.links.push(Link { driver: Coord::new(0, 0), follower: Coord::new(1, 0), ratio: 1.0, offset: None });
    locked.links.push(Link { driver: Coord::new(1, 0), follower: Coord::new(0, 0), ratio: 2.0, offset: None });
    locked.drives.push(Drive { at: Coord::new(0, 0), motion: Profile::Harmonic { offset: 0.0, amplitude: 1e6, frequency: 1e3, phase: 0.0 } });
    let _ = simulate(&locked, &Settings::rk4(0.1, 1e-3, 1e-2));
    let _ = simulate(&locked, &Settings::adaptive(0.1, 1e-2, 1e-9, 1e-9));
}
