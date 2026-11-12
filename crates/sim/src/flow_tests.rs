//! The flow solver, checked against answers worked out on paper.
//!
//! Three of these are the ones that matter. The velocity set's moment conditions are what the
//! whole method rests on and hold exactly. A shear wave's decay measures the viscosity the
//! relaxation time claims to give, which is the one physical constant the scheme has. A
//! channel's parabola measures where the walls actually are, which is what decides whether a
//! flow near a surface means anything.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::SimError;
use crate::flow::{
    C, CS2, Collision, Flow, FlowStudy, GHOST3, GHOST3_NORM, MAX_LATTICE_SPEED, MIN_TAU, Q, Scaling, Side, W, equilibrium, opposite, regularised,
    stress,
};
use crate::grid::Grid3;
use crate::library::AIR;

fn channel(n: [usize; 3]) -> Grid3 {
    Grid3::new(n, 1.0, Vec3::ZERO).expect("a lattice")
}

#[test]
fn the_velocity_set_has_the_moments_the_method_needs() {
    // These three identities are not a detail: the Chapman–Enskog expansion that turns this
    // scheme into the Navier–Stokes equations uses each of them, and the weights were chosen to
    // make them hold. If any is wrong the solver is solving something else.
    let total: f64 = W.iter().sum();
    assert!((total - 1.0).abs() < 1e-15, "the weights must add to one: {total}");

    for a in 0..3 {
        let first: f64 = W.iter().zip(&C).map(|(w, c)| w * c[a] as f64).sum();
        assert!(first.abs() < 1e-15, "the first moment must vanish: axis {a} gives {first}");
    }
    for a in 0..3 {
        for b in 0..3 {
            let second: f64 = W.iter().zip(&C).map(|(w, c)| w * c[a] as f64 * c[b] as f64).sum();
            let want = if a == b { CS2 } else { 0.0 };
            assert!((second - want).abs() < 1e-15, "the second moment at ({a},{b}) is {second}, and must be {want}");
        }
    }
    // Third moments vanish, which is what makes the scheme free of odd-order error.
    for a in 0..3 {
        for b in 0..3 {
            for d in 0..3 {
                let third: f64 = W.iter().zip(&C).map(|(w, c)| w * c[a] as f64 * c[b] as f64 * c[d] as f64).sum();
                assert!(third.abs() < 1e-15, "the third moment at ({a},{b},{d}) is {third}");
            }
        }
    }
}

#[test]
fn every_direction_has_its_own_reverse_and_finding_it_is_an_increment() {
    for i in 0..Q {
        let o = opposite(i);
        let (Some(a), Some(b)) = (C.get(i), C.get(o)) else { panic!("a direction with no velocity") };
        assert_eq!([-a[0], -a[1], -a[2]], *b, "direction {i} and {o} are not opposite");
        assert_eq!(opposite(o), i, "reversing twice has to come back");
        assert!((W.get(i).copied().unwrap_or(0.0) - W.get(o).copied().unwrap_or(1.0)).abs() < 1e-18);
    }
    assert_eq!(opposite(0), 0, "standing still has no reverse but itself");
    // The nineteen are distinct: one at rest, six on the faces, twelve on the edges.
    let mut seen: Vec<[i32; 3]> = C.to_vec();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), Q);
    assert_eq!(C.iter().filter(|c| c.iter().map(|v| v.abs()).sum::<i32>() == 1).count(), 6);
    assert_eq!(C.iter().filter(|c| c.iter().map(|v| v.abs()).sum::<i32>() == 2).count(), 12);
}

#[test]
fn the_equilibrium_set_carries_exactly_the_density_momentum_and_stress_it_should() {
    // Σf = ρ, Σf·c = ρu and Σf·c·c = ρ(c_s²δ + uu). The third one is exact for the truncated
    // equilibrium on this set, not merely second order, and it is the one that becomes the
    // pressure and the momentum flux.
    for u in [Vec3::ZERO, Vec3::new(0.05, 0.0, 0.0), Vec3::new(0.02, -0.03, 0.04)] {
        for rho in [1.0, 1.1, 0.9] {
            let f = equilibrium(rho, u);
            let mass: f64 = f.iter().sum();
            assert!((mass - rho).abs() < 1e-14, "mass: {mass} vs {rho}");
            for a in 0..3 {
                let m: f64 = f.iter().zip(&C).map(|(v, c)| v * c[a] as f64).sum();
                let want = rho * [u.x, u.y, u.z][a];
                assert!((m - want).abs() < 1e-14, "momentum along {a}: {m} vs {want}");
            }
            for a in 0..3 {
                for b in 0..3 {
                    let p: f64 = f.iter().zip(&C).map(|(v, c)| v * c[a] as f64 * c[b] as f64).sum();
                    let uu = [u.x, u.y, u.z][a] * [u.x, u.y, u.z][b];
                    let want = rho * (if a == b { CS2 } else { 0.0 } + uu);
                    assert!((p - want).abs() < 1e-14, "stress at ({a},{b}): {p} vs {want}");
                }
            }
        }
    }
}

#[test]
fn a_fluid_at_rest_stays_at_rest_and_keeps_its_mass() {
    let study = FlowStudy { steps: 200, settle: 0.0, check_every: 50, ..FlowStudy::new(0.8) };
    let mut flow = Flow::through(channel([6, 6, 6]), &study).expect("a flow");
    let report = flow.run(&study).expect("run");
    assert!(report.fastest < 1e-9, "nothing drives it, so nothing moves: {}", report.fastest);
    // Single precision, so this is compared in proportion; a scheme that leaked mass would be
    // out by far more than a part in a million.
    assert!((report.mass - 216.0).abs() / 216.0 < 1e-6, "mass must not appear or vanish: {}", report.mass);
    for c in 0..216 {
        assert!((flow.density(c) - 1.0).abs() < 1e-6, "{}", flow.density(c));
        assert!(flow.pressure(c).abs() < 1e-6);
    }
}

#[test]
fn a_uniform_flow_between_free_slip_walls_carries_straight_on() {
    // Free slip drags nothing, so a plug of fluid keeps its speed and its shape. This is what
    // checks the mirrored directions: get the reflection table wrong and the plug shears.
    let mut study = FlowStudy::new(0.8);
    study.sides = [Side::Periodic, Side::Periodic, Side::FreeSlip, Side::FreeSlip, Side::FreeSlip, Side::FreeSlip];
    study.steps = 300;
    study.settle = 0.0;
    let grid = channel([8, 8, 8]);
    let mut flow = Flow::through(grid, &study).expect("a flow");
    let speed = 0.05;
    flow.set_everywhere(|_| (1.0, Vec3::new(speed, 0.0, 0.0)));
    let report = flow.run(&study).expect("run");
    assert!((report.fastest - speed).abs() < 1e-4, "{} vs {speed}", report.fastest);
    for c in 0..512 {
        let u = flow.velocity(c, study.accel);
        assert!((u.x - speed).abs() < 1e-4, "cell {c}: {u:?}");
        assert!(u.y.abs() < 1e-4 && u.z.abs() < 1e-4, "cell {c} picked up a cross-flow: {u:?}");
    }
}

/// How fast a sinusoidal shear wave of wavelength `n` decays, measured.
fn shear_decay(across: usize, tau: f64, steps: u32) -> (f64, f64) {
    let study = FlowStudy { steps, settle: 0.0, check_every: steps, ..FlowStudy::new(tau) };
    let grid = channel([4, across, 4]);
    let mut flow = Flow::through(grid, &study).expect("a flow");
    let k = std::f64::consts::TAU / across as f64;
    let amplitude = 0.02;
    let at = |cell: usize, g: &Grid3| g.ijk(cell).map(|c| c[1] as f64).unwrap_or(0.0);
    let g = flow.grid().clone();
    flow.set_everywhere(|c| (1.0, Vec3::new(amplitude * (k * at(c, &g)).sin(), 0.0, 0.0)));

    let project = |flow: &Flow| -> f64 {
        let g = flow.grid().clone();
        let n = g.cells();
        let sum: f64 = (0..n).map(|c| flow.velocity(c, [0.0; 3]).x * (k * at(c, &g)).sin()).sum();
        // Each y layer holds 4·4 cells, and ⟨sin²⟩ over a whole period is a half.
        sum / (n as f64 / 2.0)
    };
    let before = project(&flow);
    flow.run(&study).expect("run");
    let after = project(&flow);
    let measured = -(after / before).ln() / (k * k * steps as f64);
    (measured, study.viscosity())
}

#[test]
fn a_shear_wave_decays_at_the_viscosity_the_relaxation_time_claims() {
    // The exact Stokes answer is A(t) = A₀·exp(−ν·k²·t), so measuring the decay measures ν.
    // This is the sharpest check there is on the scheme's one physical constant: get the
    // relation ν = (τ − 1/2)/3 wrong and the number comes out plainly different.
    let (measured, want) = shear_decay(32, 0.8, 200);
    assert!((measured - want).abs() / want < 0.02, "measured {measured}, and the relaxation time says {want}");

    // And it is second order in the wavenumber: doubling the wavelength quarters the error.
    let (coarse, _) = shear_decay(16, 0.8, 100);
    let (fine, _) = shear_decay(32, 0.8, 200);
    let (finer, _) = shear_decay(64, 0.8, 400);
    let errors = [(coarse - want).abs(), (fine - want).abs(), (finer - want).abs()];
    assert!(errors[1] < errors[0], "{errors:?}");
    assert!(errors[2] < errors[1], "{errors:?}");
    assert!(errors[2] / want < 2e-3, "at sixty-four cells a wavelength it should be very close: {:?}", errors[2] / want);
}

#[test]
fn the_decay_follows_the_relaxation_time_over_a_range_of_viscosities() {
    for tau in [0.6, 0.8, 1.2, 2.0] {
        let (measured, want) = shear_decay(32, tau, 100);
        assert!((measured - want).abs() / want < 0.03, "at τ = {tau} the viscosity should be {want} and came out {measured}");
    }
}

/// The velocity profile across a channel driven by a body force.
fn poiseuille(across: usize, tau: f64, accel: f64, collision: Collision) -> (Vec<f64>, f64) {
    let mut study = FlowStudy::new(tau);
    study.collision = collision;
    study.accel = [accel, 0.0, 0.0];
    // Endless along the flow and across it, walls above and below.
    study.sides = [Side::Periodic, Side::Periodic, Side::NoSlip, Side::NoSlip, Side::Periodic, Side::Periodic];
    study.steps = 20_000;
    study.settle = 1e-12;
    study.check_every = 200;
    let grid = channel([4, across, 4]);
    let mut flow = Flow::through(grid, &study).expect("a flow");
    flow.run(&study).expect("run");
    let g = flow.grid().clone();
    let profile = (0..across)
        .map(|j| {
            let c = g.at(1, j, 1).expect("a cell");
            flow.velocity(c, study.accel).x
        })
        .collect();
    (profile, study.viscosity())
}

/// The worst pointwise departure from the exact parabola, as a fraction of the exact value.
///
/// The exact answer is `u(y) = g·y·(H − y)/2ν` with the walls at `y = 0` and `y = H`.
/// Bounce-back puts a wall exactly half a cell beyond the last fluid row, so for `N` rows the
/// channel is `N` cells deep and row `j` sits at `y = j + 1/2`.
fn channel_error(across: usize, tau: f64, collision: Collision) -> f64 {
    let accel = 1e-5;
    let (profile, nu) = poiseuille(across, tau, accel, collision);
    let height = across as f64;
    profile
        .iter()
        .enumerate()
        .map(|(j, got)| {
            let y = j as f64 + 0.5;
            let want = accel * y * (height - y) / (2.0 * nu);
            (got - want).abs() / want
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_channel_driven_by_a_body_force_holds_the_parabola_it_should() {
    let (across, tau, accel) = (16usize, 0.8, 1e-5);
    let (profile, nu) = poiseuille(across, tau, accel, Collision::trt());
    let height = across as f64;
    let exact = |j: usize| {
        let y = j as f64 + 0.5;
        accel * y * (height - y) / (2.0 * nu)
    };

    let worst = channel_error(across, tau, Collision::trt());
    assert!(worst < 1e-3, "the profile is out by {worst} of itself at its worst");

    // The fastest row, against the parabola at that row rather than against the continuum peak:
    // with an even number of rows the middle of the channel falls between two of them, so the
    // highest sample is lower than u_max and comparing the two would be comparing the lattice
    // with a point it has no cell at.
    let (row, got) = profile.iter().copied().enumerate().fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
    assert!((got - exact(row)).abs() / exact(row) < 1e-3, "row {row} is {got}, and the parabola says {}", exact(row));
    // Which, for sixteen rows, is 31.875/32 of the continuum peak -- a fact about where the
    // cells are, not about the solver.
    let peak = accel * height * height / (8.0 * nu);
    assert!((got / peak - 31.875 / 32.0).abs() < 2e-3, "{} vs {}", got / peak, 31.875 / 32.0);

    // Symmetric problem, symmetric answer.
    for j in 0..across / 2 {
        let (a, b) = (profile[j], profile[across - 1 - j]);
        assert!((a - b).abs() / peak < 1e-3, "row {j} and its mirror differ: {a} vs {b}");
    }
    // And nothing anywhere runs backwards.
    assert!(profile.iter().all(|u| *u > 0.0), "{profile:?}");
}

#[test]
fn the_magic_parameter_puts_the_walls_where_they_belong_whatever_the_viscosity() {
    // This is the whole reason the two-rate collision is the default. With one rate, where a
    // bounce-back wall effectively sits moves with the viscosity, so the channel comes out with
    // the right shape and the wrong flow rate -- and by a different amount for each fluid. With
    // the magic parameter at 3/16 the wall stays put.
    let (mut trt, mut bgk) = (Vec::new(), Vec::new());
    for tau in [0.6, 0.8, 1.2] {
        let a = channel_error(16, tau, Collision::trt());
        let b = channel_error(16, tau, Collision::Bgk);
        assert!(a < 1e-3, "two rates, τ = {tau}: out by {a}");
        assert!(b > a * 10.0, "at τ = {tau} one rate was not much worse: {b} against {a}");
        trt.push(a);
        bgk.push(b);
    }
    let spread = |v: &[f64]| v.iter().copied().fold(0.0, f64::max) / v.iter().copied().fold(f64::MAX, f64::min);
    // The two-rate error hardly moves with the viscosity; the one-rate error moves several-fold.
    assert!(spread(&trt) < 1.5, "two rates should not care about the viscosity: {trt:?}");
    assert!(spread(&bgk) > 2.0, "one rate does: {bgk:?}");
}

#[test]
fn a_refused_flow_is_refused_before_it_runs_rather_than_after_it_diverges() {
    let grid = channel([4, 4, 4]);
    // A relaxation time at or below a half has no viscosity.
    for tau in [0.5, 0.4, 0.0, -1.0, f64::NAN] {
        let study = FlowStudy::new(tau);
        assert!(matches!(Flow::through(grid.clone(), &study), Err(SimError::Setup(_))), "τ = {tau} was accepted");
    }
    // A fluid thicker than anything.
    assert!(Flow::through(grid.clone(), &FlowStudy::new(1e6)).is_err());
    // An inlet above the low-Mach limit.
    let mut study = FlowStudy::new(0.8);
    study.inlet = [0.5, 0.0, 0.0];
    assert!(matches!(Flow::through(grid.clone(), &study), Err(SimError::Setup(_))));
    study.inlet = [f64::NAN, 0.0, 0.0];
    assert!(Flow::through(grid.clone(), &study).is_err());
    // A flow of no steps, and a magic parameter that is not one.
    let mut study = FlowStudy::new(0.8);
    study.steps = 0;
    assert!(Flow::through(grid.clone(), &study).is_err());
    let mut study = FlowStudy::new(0.8);
    study.collision = Collision::Trt { magic: 0.0 };
    assert!(Flow::through(grid.clone(), &study).is_err());
    study.collision = Collision::Trt { magic: -1.0 };
    assert!(Flow::through(grid, &study).is_err());
}

#[test]
fn a_lattice_the_model_fills_has_nowhere_for_the_air_to_be() {
    use crate::grid::Occupancy;
    use crate::test_shapes::block_at;
    // A block that covers the whole lattice: there is no outside, so there is no flow.
    let grid = Grid3::new([4, 4, 4], 1.0, Vec3::ZERO).expect("a lattice");
    let mesh = block_at([-1., -1., -1.], [6., 6., 6.]);
    let occ = Occupancy::of(grid, &[&mesh], 1).expect("occupancy");
    let study = FlowStudy::new(0.8);
    assert!(matches!(Flow::around(&occ, &study), Err(SimError::Domain(_))));
}

#[test]
fn a_block_in_a_stream_is_pushed_along_it_and_not_sideways() {
    use crate::grid::Occupancy;
    use crate::test_shapes::block_at;
    // Not a drag coefficient -- that wants a far larger domain and a turbulence model. What is
    // checked is that the momentum exchange has the right sign and the right symmetry: a
    // symmetric body in a straight stream is pushed downstream and nowhere else.
    let grid = Grid3::new([40, 20, 20], 1.0, Vec3::ZERO).expect("a lattice");
    // A cube in the middle of the tunnel, six cells on a side.
    let mesh = block_at([16., 7., 7.], [6., 6., 6.]);
    let occ = Occupancy::of(grid, &[&mesh], 1).expect("occupancy");
    let mut study = FlowStudy::new(0.9);
    study.inlet = [0.05, 0.0, 0.0];
    study.sides = [Side::Inlet, Side::Outlet, Side::FreeSlip, Side::FreeSlip, Side::FreeSlip, Side::FreeSlip];
    study.steps = 2000;
    study.settle = 1e-8;
    study.check_every = 100;
    let mut flow = Flow::around(&occ, &study).expect("a flow");
    flow.set_everywhere(|c| if occ.is_solid(c) { (1.0, Vec3::ZERO) } else { (1.0, Vec3::new(0.05, 0.0, 0.0)) });
    let report = flow.run(&study).expect("run");
    assert!(report.fastest < MAX_LATTICE_SPEED * 3.0, "the flow stayed in range: {}", report.fastest);
    let force = flow.force();
    assert!(force.x > 0.0, "the stream pushes the block downstream: {force:?}");
    let sideways = force.y.abs().max(force.z.abs());
    assert!(sideways < force.x * 0.1, "a symmetric block should feel no side load: {force:?}");
    // And the air has to get out of the way: somewhere beside the block runs faster than the
    // stream it came from.
    let beside = flow.grid().at(19, 3, 10).expect("a cell");
    assert!(flow.velocity(beside, study.accel).x > 0.05, "the flow speeds up past the block");
    // While the block's own cells hold nothing.
    let inside = flow.grid().at(19, 10, 10).expect("a cell");
    assert!(flow.is_solid(inside), "the block is where it was put");
}

#[test]
fn the_scaling_turns_a_real_flow_into_lattice_numbers_and_back() {
    // Ten metres a second of air over a one-millimetre lattice, running at a twentieth of a
    // cell a step.
    let m = Scaling::for_flow(&AIR, 1e-3, 10.0, 0.05, Collision::regularized()).expect("a scaling");
    let scale = m.scaling;
    // Δt = u_lattice·Δx/u, so 0.05·1e-3/10 = 5 µs.
    assert!((scale.step_s - 5e-6).abs() < 1e-12, "{}", scale.step_s);
    // ν_lattice = ν·Δt/Δx² = 1.516e-5·5e-6/1e-6 = 7.58e-5, and τ = 3ν + 1/2. That is the
    // relaxation time the fluid asks for; the one actually run is held at the floor for this
    // case, which the next test is about.
    let nu_lattice = AIR.kinematic_viscosity_m2_s() * scale.step_s / 1e-6;
    assert!((m.physical_tau - (nu_lattice / CS2 + 0.5)).abs() < 1e-12, "{}", m.physical_tau);
    assert!(m.tau >= m.physical_tau && m.tau > 0.5, "{} and {}", m.tau, m.physical_tau);

    // A velocity goes out and comes back.
    let lattice = Vec3::new(0.05, -0.01, 0.0);
    let real = scale.velocity_m_s(lattice);
    assert!((real.x - 10.0).abs() < 1e-9, "{real:?}");
    let back = scale.to_lattice(real);
    assert!((back - lattice).len() < 1e-12, "{back:?} vs {lattice:?}");
    // And a duration is the step times the steps.
    assert!((scale.duration_s(1000) - 5e-3).abs() < 1e-15);
}

#[test]
fn a_flow_the_lattice_cannot_hold_as_given_is_run_thicker_and_said_to_be() {
    // The honest limit of the method, and it is a large one. Ten metres a second of air over a
    // one-millimetre lattice asks for a relaxation time of 0.5002, which is right against the
    // stability edge, and puts a Reynolds number of six hundred and sixty into every single
    // cell. So the fluid is run thicker than it is, the Reynolds number that actually ran is
    // far below the one asked for, and both are reported rather than left to be discovered.
    let m = Scaling::for_flow(&AIR, 1e-3, 10.0, 0.05, Collision::regularized()).expect("a mapping");
    assert!((m.physical_tau - 0.5002).abs() < 1e-4, "{}", m.physical_tau);
    assert!((m.tau - MIN_TAU).abs() < 1e-12, "run at the floor: {}", m.tau);
    assert!(m.thickened());
    // Thickened by the ratio of the viscosity run to the fluid's own.
    let want = (MIN_TAU - 0.5) / (m.physical_tau - 0.5);
    assert!((m.thickening() - want).abs() / want < 1e-9, "{} vs {want}", m.thickening());
    assert!(m.thickening() > 20.0, "{}", m.thickening());
    // And the Reynolds number that ran is below the one asked for by exactly that factor.
    let cells = 20.0;
    let (ran, asked) = (m.effective_reynolds(cells), m.requested_reynolds(cells));
    assert!((asked / ran - m.thickening()).abs() / m.thickening() < 1e-9, "{asked} over {ran}");
    assert!(asked > 1000.0 && ran < 500.0, "asked {asked}, ran {ran}");

    assert!((m.cell_reynolds - 659.6).abs() < 1.0, "{}", m.cell_reynolds);
    assert!(!m.resolved(), "{}", m.cell_reynolds);
    assert!((m.resolved_spacing_m - 1.516e-5).abs() < 1e-8, "{}", m.resolved_spacing_m);
    assert!((m.refinement_wanted() - 65.96).abs() < 0.5, "{}", m.refinement_wanted());
    let caveat = m.caveat(cells).expect("a caveat");
    assert!(caveat.contains("thicker than it is"), "{caveat}");
    assert!(caveat.contains("Reynolds number"), "{caveat}");

    // A slow flow on a fine lattice is held as given, and then there is nothing to say.
    let fine = Scaling::for_flow(&AIR, 1e-5, 1.0, 0.05, Collision::regularized()).expect("a mapping");
    assert!(fine.resolved(), "{}", fine.cell_reynolds);
    assert!(!fine.thickened(), "{} vs {}", fine.tau, fine.physical_tau);
    assert!((fine.thickening() - 1.0).abs() < 1e-12);
    assert_eq!(fine.caveat(20.0), None);
    assert!(fine.refinement_wanted() <= 1.0);
    assert!(fine.tau > 0.7 && fine.tau < 0.8, "a comfortable relaxation time: {}", fine.tau);
    // Asked for and ran are the same number when nothing was thickened.
    assert!((fine.effective_reynolds(20.0) - fine.requested_reynolds(20.0)).abs() < 1e-9);
}

#[test]
fn a_mapping_nobody_could_make_is_refused() {
    assert!(Scaling::for_flow(&AIR, 0.0, 10.0, 0.05, Collision::regularized()).is_err());
    assert!(Scaling::for_flow(&AIR, 1e-3, 0.0, 0.05, Collision::regularized()).is_err());
    assert!(Scaling::for_flow(&AIR, 1e-3, -1.0, 0.05, Collision::regularized()).is_err());
    assert!(Scaling::for_flow(&AIR, 1e-3, 10.0, 0.0, Collision::regularized()).is_err());
    assert!(Scaling::for_flow(&AIR, 1e-3, 10.0, 0.5, Collision::regularized()).is_err(), "above the low-Mach limit");
    assert!(Scaling::for_flow(&AIR, f64::NAN, 10.0, 0.05, Collision::regularized()).is_err());
    assert!(Scaling::for_flow(&AIR, 1e-3, f64::INFINITY, 0.05, Collision::regularized()).is_err());
    // A fluid with no viscosity cannot be made to flow at all.
    let ideal = crate::library::Fluid { dynamic_viscosity_pa_s: 0.0, ..AIR };
    assert!(matches!(Scaling::for_flow(&ideal, 1e-3, 10.0, 0.05, Collision::regularized()), Err(SimError::Material(_))));
}

#[test]
fn a_force_and_a_pressure_come_back_in_newtons_and_pascals() {
    let scale = Scaling::for_flow(&AIR, 1e-3, 10.0, 0.05, Collision::regularized()).expect("a scaling").scaling;
    // Force is ρ·Δx⁴/Δt²: 1.204 · 1e-12 / 2.5e-11 = 0.048 N per lattice unit.
    let f = scale.force_n(Vec3::new(1.0, 0.0, 0.0));
    let want = AIR.density_kg_m3 * 1e-12 / (5e-6f64).powi(2);
    assert!((f.x - want).abs() / want < 1e-12, "{} vs {want}", f.x);
    // Pressure is ρ·(Δx/Δt)²: the dynamic pressure of the lattice's own speed scale.
    let p = scale.pressure_pa(1.0);
    assert!((p - AIR.density_kg_m3 * (1e-3 / 5e-6f64).powi(2)).abs() / p < 1e-12, "{p}");
    // Half the dynamic pressure of a ten-metre-a-second stream is about 60 Pa, and the lattice
    // pressure that stands for it is small, which is what low Mach means.
    let dynamic = 0.5 * AIR.density_kg_m3 * 100.0;
    assert!((dynamic - 60.2).abs() < 0.5, "{dynamic}");
    assert!(scale.pressure_pa(0.0).abs() < 1e-18);
}

/// The lid-driven cavity against Ghia, Ghia and Shin (1982), which is the benchmark every
/// incompressible solver is held to.
///
/// A square box of fluid, shut on three sides, with the lid sliding along the top. It has no
/// closed-form answer, so the reference is the published one: a 129x129 stream-function solution
/// tabulated at Re 100 to 10000, open literature, and the table every code since has been
/// checked against. What makes it the right test is that it exercises what a channel's parabola
/// cannot -- a recirculating flow with a shear layer, a primary vortex and corner eddies -- and
/// gets it wrong in an obvious way if the collision, the wall or the velocity boundary is off.
///
/// At Re 100 their vertical-centreline profile reaches its most negative `u/U` of **-0.2109** at
/// `y/L` **0.4531**. That is what this asserts.
#[test]
fn the_lid_driven_cavity_matches_ghia_at_reynolds_one_hundred() {
    let n = 64usize;
    let lid = 0.05;
    // Re = U*L/nu, so nu follows from the Reynolds number we want.
    let re = 100.0;
    let nu = lid * n as f64 / re;
    let tau = 3.0 * nu + 0.5;
    assert!(tau > MIN_TAU, "tau {tau} would be below what the lattice can carry");

    let mut study = FlowStudy::new(tau);
    study.collision = Collision::trt();
    // Shut on -x, +x and -z; endless across the cavity; the lid slides along +z.
    study.sides = [Side::NoSlip, Side::NoSlip, Side::Periodic, Side::Periodic, Side::NoSlip, Side::Inlet];
    study.inlet = [lid, 0.0, 0.0];
    study.steps = 60_000;
    study.settle = 1e-9;
    study.check_every = 500;

    let grid = Grid3::new([n, 4, n], 1.0, Vec3::ZERO).expect("a lattice");
    let mut flow = Flow::through(grid, &study).expect("a flow");
    flow.run(&study).expect("the cavity settles");
    let g = flow.grid().clone();

    // u along the vertical centreline, as a fraction of the lid's speed.
    let profile: Vec<(f64, f64)> = (0..n)
        .filter_map(|k| {
            let c = g.at(n / 2, 1, k)?;
            Some(((k as f64 + 0.5) / n as f64, flow.velocity(c, study.accel).x / lid))
        })
        .collect();
    assert_eq!(profile.len(), n);

    let (at, least) = profile.iter().fold((0.0, 0.0), |a, b| if b.1 < a.1 { *b } else { a });
    // Ghia's own figures for Re 100.
    let (ghia_u, ghia_y) = (-0.2109, 0.4531);
    // Measured: -0.2116 at 0.4609, which is a third of a per cent out in the speed and less
    // than one cell out in where it happens. Held to that, not to a bound loose enough to pass
    // whatever it does.
    assert!((least - ghia_u).abs() / ghia_u.abs() < 0.03, "the back-flow reaches {least} of the lid's speed, and Ghia says {ghia_u}");
    assert!((at - ghia_y).abs() < 0.025, "it reaches it at y/L {at}, and Ghia says {ghia_y}");

    // And the lid drags the fluid forward at the top: the profile must change sign.
    let top = profile.last().map(|(_, u)| *u).unwrap_or(0.0);
    assert!(top > 0.3, "the fluid under the lid should run with it, got {top}");

    // Where it changes sign, by linear interpolation between the two rows either side.
    let mut cross = 0.0;
    for w in profile.windows(2) {
        if let [(y0, u0), (y1, u1)] = w
            && *u0 < 0.0
            && *u1 >= 0.0
        {
            cross = y0 + (y1 - y0) * (0.0 - u0) / (u1 - u0);
        }
    }
    // v along the horizontal centreline, the other half of Ghia's table.
    let across: Vec<(f64, f64)> = (0..n)
        .filter_map(|i| {
            let c = g.at(i, 1, n / 2)?;
            Some(((i as f64 + 0.5) / n as f64, flow.velocity(c, study.accel).z / lid))
        })
        .collect();
    let vmin = across.iter().fold((0.0, 0.0), |a, b| if b.1 < a.1 { *b } else { a });
    let vmax = across.iter().fold((0.0, 0.0), |a, b| if b.1 > a.1 { *b } else { a });
    // Ghia puts the primary vortex centre at y/L 0.7344 for Re 100, which is the height where
    // the centreline flow changes direction. Measured 0.7236, within a cell of it.
    assert!((cross - 0.7344).abs() < 0.03, "the flow turns over at y/L {cross}, and Ghia says 0.7344");

    // The circulation itself: the lid drives the fluid right, so it descends on the right wall
    // and rises on the left, and the descending side is the stronger of the two. Getting the
    // sense of the vortex wrong is the failure a single extremum would not catch.
    assert!(vmin.1 < 0.0 && vmin.0 > 0.7, "the fluid should descend on the right, got {:.4} at {:.4}", vmin.1, vmin.0);
    assert!(vmax.1 > 0.0 && vmax.0 < 0.35, "and rise on the left, got {:.4} at {:.4}", vmax.1, vmax.0);
    assert!(vmin.1.abs() > vmax.1, "the descending side is the stronger: {:.4} against {:.4}", vmin.1, vmax.1);
}

#[test]
fn rebuilding_the_symmetric_half_from_its_stress_keeps_the_stress_and_nothing_else() {
    // The regularisation is a projection and not an approximation, and this is what that means:
    // the density, the momentum and `Π^neq` come through it untouched, and the even ghost
    // moments -- the three the lattice carries and the fluid does not -- do not come through at
    // all. If that were only approximate the viscosity would be wrong, so it is worth pinning
    // to the last bit rather than to a tolerance.
    let eq = equilibrium(1.0, Vec3::new(0.03, -0.02, 0.01));
    let mut f = [0.0f32; Q];
    for i in 0..Q {
        // Lopsided on purpose: a departure from equilibrium with something in every moment it
        // could have, so no ghost moment is zero by accident.
        let wobble = 1.0 + 0.3 * ((i * 7 + 3) % 11) as f64 / 11.0 - 0.15 * ((i * 5 + 1) % 7) as f64 / 7.0;
        f[i] = (eq[i] * wobble) as f32;
    }
    let pi = stress(&f, &eq);
    let rebuilt = regularised(&pi);

    // No mass and no momentum, so putting this in place of the symmetric half moves neither.
    let mass: f64 = rebuilt.iter().sum();
    assert!(mass.abs() < 1e-15, "the rebuilt half carries mass: {mass}");
    for (a, axis) in ["x", "y", "z"].iter().enumerate() {
        let momentum: f64 = (0..Q).map(|i| rebuilt[i] * C[i][a] as f64).sum();
        assert!(momentum.abs() < 1e-15, "the rebuilt half carries momentum along {axis}: {momentum}");
    }

    // And its own second moment is the stress it was built from, which is the projection
    // property: feeding it back through gives the same tensor again.
    for a in 0..3 {
        for b in 0..3 {
            let got: f64 = (0..Q).map(|i| rebuilt[i] * C[i][a] as f64 * C[i][b] as f64).sum();
            let want = pi[a][b];
            assert!((got - want).abs() <= 1e-14 * want.abs().max(1.0), "Π[{a}][{b}]: rebuilt {got}, was {want}");
        }
    }

    // The case has to have had ghost moments for any of that to mean anything: the symmetric
    // half this replaces is not already the reconstruction.
    let worst = (0..Q)
        .map(|i| {
            let o = opposite(i);
            let actual = 0.5 * ((f[i] as f64 - eq[i]) + (f[o] as f64 - eq[o]));
            (actual - rebuilt[i]).abs()
        })
        .fold(0.0, f64::max);
    assert!(worst > 1e-6, "this departure from equilibrium had no even ghost moments to throw away, so nothing was tested");
}

#[test]
fn throwing_the_even_ghost_moments_away_leaves_the_wall_exactly_where_two_rates_put_it() {
    // The antisymmetric half is what the magic parameter acts on and it is not touched, so the
    // wall has to come out where `the_magic_parameter_puts_the_walls_where_they_belong` puts it.
    // Dropping that half as well is a different operator with a different price, below.
    for tau in [0.6, 0.8, 1.2] {
        let trt = channel_error(16, tau, Collision::trt());
        let reg = channel_error(16, tau, Collision::regularized());
        assert!(reg < 1e-3, "regularised, τ = {tau}: the parabola is out by {reg} of itself");
        assert!(reg < trt * 2.0, "τ = {tau}: regularised {reg} against two rates {trt}");
    }
}

#[test]
fn a_rate_for_the_odd_ghosts_buys_a_lower_relaxation_time_and_gives_the_wall_up() {
    // The six odd ghost moments relaxed at their own rate instead of at `ω₋`. That reaches a
    // relaxation time two rates cannot hold, and the wall is what it costs: `ω₋` no longer acts
    // on anything, so the magic parameter no longer holds a bounce-back wall still.
    for tau in [0.6, 0.8, 1.2] {
        let mrt = channel_error(16, tau, Collision::mrt());
        let reg = channel_error(16, tau, Collision::regularized());
        assert!(mrt > reg * 10.0, "τ = {tau}: the wall should be plainly worse: mrt {mrt} against regularised {reg}");
        // But only by a few per cent, and better than one rate manages.
        assert!(mrt < 0.1, "τ = {tau}: and not worse than a tenth of the flow: {mrt}");
        assert!(mrt < channel_error(16, tau, Collision::Bgk), "τ = {tau}: it should still beat one rate at the wall");
    }
    // What it buys is relaxation time, which is Reynolds number: twenty times as much as two
    // rates hold, for the same lattice and the same speed.
    let ratio = (Collision::trt().min_tau() - 0.5) / (Collision::mrt().min_tau() - 0.5);
    assert!((ratio - 20.0).abs() < 0.5, "the viscosity ratio should be twentyfold: {ratio}");
    // And only the two that relax the antisymmetric half at `ω₋` claim to hold a wall.
    assert!(Collision::trt().holds_the_wall() && Collision::regularized().holds_the_wall());
    assert!(!Collision::mrt().holds_the_wall() && !Collision::Bgk.holds_the_wall());
}

#[test]
fn giving_the_odd_ghosts_the_rate_they_already_had_changes_nothing_at_all() {
    // The cheapest check that the extra rate is the only difference: at `ghost = ω₋` the
    // multiple-rate operator *is* the regularised one, and has to agree to the last bit rather
    // than merely closely.
    for tau in [0.6, 0.9] {
        let omega_minus = 1.0 / (0.5 + (3.0 / 16.0) / (tau - 0.5));
        let same = Collision::Mrt { magic: 3.0 / 16.0, ghost: omega_minus };
        let (a, b) = (channel_error(16, tau, same), channel_error(16, tau, Collision::regularized()));
        assert_eq!(a, b, "τ = {tau}: at ω₋ the two operators should be the same arithmetic");
    }
}

#[test]
fn the_odd_ghost_moments_are_orthogonal_to_everything_the_fluid_has() {
    // The whole licence for relaxing these at a rate of their own. If any of them had a
    // component along the density, the momentum or the stress, changing that rate would change
    // the fluid, and the viscosity would stop being the one asked for.
    let physical = |k: usize| -> Vec<f64> {
        match k {
            0 => (0..Q).map(|_| 1.0).collect(),
            1..=3 => (0..Q).map(|i| C[i][k - 1] as f64).collect(),
            _ => {
                let (a, b) = ((k - 4) / 3, (k - 4) % 3);
                (0..Q).map(|i| (C[i][a] * C[i][b]) as f64).collect()
            }
        }
    };
    for (g, ghost) in GHOST3.iter().enumerate() {
        for k in 0..13 {
            let dot: f64 = (0..Q).map(|i| ghost[i] * physical(k)[i]).sum();
            assert!(dot.abs() < 1e-12, "ghost {g} has a component along physical moment {k}: {dot}");
        }
        // Each is odd, so it lives entirely in the antisymmetric half.
        for i in 0..Q {
            assert_eq!(ghost[i], -ghost[opposite(i)], "ghost {g} is not odd at direction {i}");
        }
        // And its recorded norm is its norm.
        let norm: f64 = (0..Q).map(|i| ghost[i] * ghost[i]).sum();
        assert!((norm - GHOST3_NORM[g]).abs() < 1e-12, "ghost {g}: norm {norm}, recorded {}", GHOST3_NORM[g]);
    }
    // The six are independent of each other too, or they would not be six moments.
    for (a, first) in GHOST3.iter().enumerate() {
        for (b, second) in GHOST3.iter().enumerate().skip(a + 1) {
            let dot: f64 = (0..Q).map(|i| first[i] * second[i]).sum();
            assert!(dot.abs() < 1e-12, "ghosts {a} and {b} are not independent: {dot}");
        }
    }
}

#[test]
fn a_body_force_puts_the_forcing_in_the_first_moment_of_the_departure() {
    // Why the odd half is projected onto its six ghosts rather than simply dropped, which is
    // what regularisation in the literature does. `collide` builds the equilibrium at the
    // forced velocity `u = m/ρ + a/2`, so the departure from it carries a first moment of
    // `−ρa/2`. That is the forcing, not a ghost moment. Dropping the half deletes it -- which
    // on a channel driven by a body force is most of why dropping it reads so much worse at the
    // wall -- while projecting onto GHOST3 cannot touch it, since all six are orthogonal to the
    // momentum.
    let accel = [1e-3, 0.0, 0.0];
    let (rho, m) = (1.0, [0.02, -0.01, 0.005]);
    let u = Vec3::new(m[0] / rho + accel[0] / 2.0, m[1] / rho + accel[1] / 2.0, m[2] / rho + accel[2] / 2.0);
    let eq = equilibrium(rho, u);
    let f: Vec<f32> = equilibrium(rho, Vec3::new(m[0], m[1], m[2])).iter().map(|v| *v as f32).collect();
    for a in 0..3 {
        let first: f64 = (0..Q).map(|i| (f[i] as f64 - eq[i]) * C[i][a] as f64).sum();
        let want = -rho * accel[a] / 2.0;
        assert!((first - want).abs() < 1e-7, "axis {a}: the departure's first moment is {first}, and -ρa/2 is {want}");
    }
}

/// A bluff body at a given relaxation time: the drag, or why the run came apart.
fn bluff_body(tau: f64, collision: Collision) -> Result<f64, String> {
    use crate::grid::Occupancy;
    use crate::test_shapes::block_at;
    let grid = Grid3::new([40, 20, 20], 1.0, Vec3::ZERO).expect("a lattice");
    let mesh = block_at([16., 7., 7.], [6., 6., 6.]);
    let occ = Occupancy::of(grid, &[&mesh], 1).expect("occupancy");
    let mut study = FlowStudy::new(tau);
    study.collision = collision;
    study.inlet = [0.05, 0.0, 0.0];
    study.sides = [Side::Inlet, Side::Outlet, Side::FreeSlip, Side::FreeSlip, Side::FreeSlip, Side::FreeSlip];
    study.steps = 4000;
    study.settle = 0.0;
    study.check_every = 500;
    let mut flow = Flow::around(&occ, &study).map_err(|e| e.to_string())?;
    flow.set_everywhere(|c| if occ.is_solid(c) { (1.0, Vec3::ZERO) } else { (1.0, Vec3::new(0.05, 0.0, 0.0)) });
    let report = flow.run(&study).map_err(|e| e.to_string())?;
    if !(report.fastest < MAX_LATTICE_SPEED * 4.0) {
        return Err(format!("ran away to {}", report.fastest));
    }
    Ok(flow.force().x)
}

#[test]
fn each_collision_operator_holds_the_relaxation_time_it_claims_to() {
    // Where `Collision::min_tau` comes from. Each operator is run at its own floor and has to
    // come back with a drag; the figures in that doc comment are this case.
    for collision in [Collision::Bgk, Collision::trt(), Collision::regularized(), Collision::mrt()] {
        let tau = collision.min_tau();
        let drag = bluff_body(tau, collision).unwrap_or_else(|e| panic!("{} at its floor τ = {tau}: {e}", collision.name()));
        assert!(drag > 0.0 && drag.is_finite(), "{} at τ = {tau} gave a drag of {drag}", collision.name());
    }

    // And the floors are not interchangeable, which is the whole reason they are per operator:
    // two rates come apart at the relaxation time the regularised operator holds. This is the
    // one that matters, because 0.51 is what `MIN_TAU` used to be for every operator alike --
    // so the floor the solver advertised was below where its own default could run.
    let tau = Collision::regularized().min_tau();
    assert!(bluff_body(tau, Collision::regularized()).is_ok(), "the regularised operator holds its own floor");
    assert!(bluff_body(tau, Collision::trt()).is_err(), "two rates should not survive τ = {tau}; the floor would be wrong");
}

/// A channel whose walls sit at a fraction of a cell, driven by a body force.
///
/// The gap runs from `lo` to `hi` in lattice units, which are deliberately not half-cell
/// positions, so plain bounce-back puts the walls somewhere the model does not have them.
/// Returns the worst departure from the exact parabola between the *true* walls.
fn offset_channel_at(across: usize, lo: f64, hi: f64, tau: f64, collision: Collision, wall: crate::flow::Wall, accel: f64) -> (f64, f64) {
    use crate::cut::inside_of;
    use crate::grid::Occupancy;
    use crate::test_shapes::block_at;
    let grid = Grid3::new([4, across, 4], 1.0, Vec3::ZERO).expect("a lattice");
    let span = across as f64;
    let below = block_at([-10.0, -10.0, -10.0], [24.0, 10.0 + lo, 24.0]);
    let above = block_at([-10.0, hi, -10.0], [24.0, 10.0 + span - hi, 24.0]);
    let occ = Occupancy::of(grid.clone(), &[&below, &above], 4).expect("occupancy");
    let (in_below, in_above) = (inside_of(&below), inside_of(&above));
    let inside = |p: Vec3| in_below(p) || in_above(p);

    let mut study = FlowStudy::new(tau);
    study.collision = collision;
    study.wall = wall;
    study.accel = [accel, 0.0, 0.0];
    study.sides = [Side::Periodic; 6];
    // A channel reaches steady state on its own diffusive time, H^2/nu, which is about 41000
    // steps for the widest gap used here. Thirty thousand left the finest case short of it, and
    // an unconverged force reads worse the finer the lattice, which looks exactly like a
    // scheme that does not converge. The lattice is a few hundred cells, so this is cheap.
    // Ten times the widest gap's diffusive time, so nothing here is read before it is steady.
    // The threshold is reached long before that and gives the same answers to every figure
    // printed, which is what says the early stop is not what sets them.
    study.steps = 400_000;
    study.settle = 1e-12;
    study.check_every = 2000;
    let mut flow = Flow::around_surface(&occ, &inside, &study).expect("a flow");
    flow.run(&study).expect("run");

    let nu = study.viscosity();
    let height = hi - lo;
    let peak = accel * height * height / (8.0 * nu);
    let mut worst: f64 = 0.0;
    if std::env::var_os("FLOW_PROFILE").is_some() {
        for j in 0..across {
            let Some(c) = grid.at(1, j, 1) else { continue };
            let y = j as f64 + 0.5;
            let want = accel * (y - lo) * (hi - y) / (2.0 * nu);
            let got = if flow.is_solid(c) { f64::NAN } else { flow.velocity(c, study.accel).x };
            println!("    j {j:<3} y {y:<5} solid {:<5} got {got:.6e} want {want:.6e}", flow.is_solid(c));
        }
    }
    for j in 0..across {
        let Some(c) = grid.at(1, j, 1) else { continue };
        if flow.is_solid(c) {
            continue;
        }
        let y = j as f64 + 0.5;
        let want = accel * (y - lo) * (hi - y) / (2.0 * nu);
        if want <= 0.0 {
            continue;
        }
        // Against the peak, not pointwise: near a wall the parabola is small and a pointwise
        // ratio says more about where the sample is than about the answer.
        worst = worst.max((flow.velocity(c, study.accel).x - want).abs() / peak);
    }
    // In steady state the walls take exactly what the body force puts in, so the force on them
    // is known without solving anything: rho * accel * (the fluid's own volume), which is the
    // channel's true gap and not the lattice's idea of it.
    let want_force = accel * 4.0 * height * 4.0;
    (worst, (flow.force().x - want_force).abs() / want_force)
}

/// As [`offset_channel_at`], driven hard enough to read easily at a gap of about 17 cells.
fn offset_channel(across: usize, lo: f64, hi: f64, tau: f64, collision: Collision, wall: crate::flow::Wall) -> f64 {
    offset_channel_at(across, lo, hi, tau, collision, wall, 1e-5).0
}

#[test]
fn a_wall_the_lattice_does_not_land_on_is_put_where_the_model_has_it() {
    use crate::flow::Wall;
    // The gap is 17.4 cells wide, so neither wall falls on a half cell. Plain bounce-back can
    // only put them at 3.0 and 21.0 -- a channel 18 wide rather than 17.4 -- and no amount of
    // refining moves that, because it is where the scheme thinks the wall is and not a
    // discretisation. The flow comes out about 7% too fast everywhere as a result.
    //
    // Cutting each link where the model's surface really crosses it puts them at 3.3 and 20.7,
    // and the error falls by most of a decade. What is left is second order in the spacing,
    // which is what Bouzidi's interpolation is.
    let (lo, hi) = (3.3, 20.7);
    for tau in [0.6, 0.8, 1.2] {
        let half = offset_channel(24, lo, hi, tau, Collision::regularized(), Wall::HalfWay);
        let interp = offset_channel(24, lo, hi, tau, Collision::regularized(), Wall::Interpolated);
        assert!(half > 0.05, "τ = {tau}: a wall 0.3 of a cell out should cost about 7% of the peak, and cost {half}");
        assert!(interp < 0.02, "τ = {tau}: with the wall put where the model has it: {interp}");
        assert!(half > interp * 5.0, "τ = {tau}: halfway {half} against interpolated {interp}");
    }
    // And it is the geometry doing the work, not the collision operator: the operator that
    // gives the wall up gets the same answer back once the wall comes from the model.
    for tau in [0.6, 0.8] {
        let reg = offset_channel(24, lo, hi, tau, Collision::regularized(), Wall::Interpolated);
        let mrt = offset_channel(24, lo, hi, tau, Collision::mrt(), Wall::Interpolated);
        assert!((reg - mrt).abs() < 0.5 * reg, "τ = {tau}: regularised {reg} against mrt {mrt}");
    }
}

#[test]
fn an_interpolated_wall_holds_the_lattices_mass() {
    use crate::cut::inside_of;
    use crate::flow::Wall;
    use crate::grid::Occupancy;
    use crate::test_shapes::block_at;
    // Bouzidi's rule does not hand back exactly what it was given, so a closed lattice bled:
    // a third of its mass over four hundred thousand steps, against two parts in a million for
    // plain bounce-back. It is not a rounding error and it does not stop; it took the solver
    // with it, since a flow whose density is draining away never settles either. Each boundary
    // node now puts back what its links failed to return, into the one direction that stands
    // still, so the mass is bounce-back's and the momentum is untouched.
    let (lo, hi, across) = (3.25, 19.25, 24usize);
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        let grid = Grid3::new([4, across, 4], 1.0, Vec3::ZERO).expect("a lattice");
        let below = block_at([-10.0, -10.0, -10.0], [24.0, 10.0 + lo, 24.0]);
        let above = block_at([-10.0, hi, -10.0], [24.0, 10.0 + across as f64 - hi, 24.0]);
        let occ = Occupancy::of(grid.clone(), &[&below, &above], 4).expect("occupancy");
        let (in_below, in_above) = (inside_of(&below), inside_of(&above));
        let inside = |p: Vec3| in_below(p) || in_above(p);
        let mut study = FlowStudy::new(0.8);
        study.wall = wall;
        study.accel = [1e-5, 0.0, 0.0];
        study.sides = [Side::Periodic; 6];
        // Twenty thousand, not sixty: the leak was percent-scale over this many steps against
        // the bound below, so three decades of discrimination survive at a third of the runtime.
        study.steps = 20_000;
        study.settle = 0.0;
        study.check_every = 20_000;
        let mut flow = Flow::around_surface(&occ, &inside, &study).expect("a flow");
        let started = (0..grid.cells()).filter(|c| !flow.is_solid(*c)).count() as f64;
        let report = flow.run(&study).expect("run");
        let drift = (report.mass - started) / started;
        assert!(drift.abs() < 1e-5, "{wall:?} moved the lattice's mass by {drift:+.3e} of it over {} steps", study.steps);
    }
}

#[test]
fn a_flow_that_settles_early_reports_the_force_of_the_settled_flow() {
    use crate::flow::Wall;
    // The tally used to start over only at the halfway mark, so a run that reached its
    // threshold before then reported a force averaged over its own transient -- which, starting
    // from rest, reads low. `sim.flow` settles by default, so this was the ordinary case and
    // not an awkward one.
    //
    // A channel driven by a body force is where that is checkable without solving anything: in
    // steady state the walls take exactly what the force puts in, rho * accel * the fluid's own
    // volume. Averaged over the transient this came out 3.6% light and grew worse the finer the
    // lattice, which reads exactly like a scheme that will not converge.
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        let out = offset_channel_at(24, 3.25, 19.25, 0.8, Collision::regularized(), wall, 1e-5).1;
        assert!(out < 0.01, "{wall:?}: the wall force is out by {out:.3e} of what the body force puts in");
    }
}

#[test]
fn a_wall_that_does_land_halfway_is_left_exactly_as_it_was() {
    use crate::flow::Wall;
    // The interpolation is the identity at a half, so a model whose face lands on the lattice
    // is bit for bit what plain bounce-back gives. That is what makes it safe to leave on.
    let (lo, hi) = (3.0, 21.0);
    for tau in [0.6, 1.2] {
        let half = offset_channel(24, lo, hi, tau, Collision::regularized(), Wall::HalfWay);
        let interp = offset_channel(24, lo, hi, tau, Collision::regularized(), Wall::Interpolated);
        assert_eq!(half, interp, "τ = {tau}: a wall already halfway should be untouched");
    }
}

/// Drag coefficient of a circular cylinder in cross-flow, two dimensional, half the flow.
///
/// At this Reynolds number the flow is steady and symmetric about the centreline, so only the
/// half above it is solved: the lower side is a free-slip plane, which is what a plane of
/// symmetry is. That halves the lattice, which is what makes a tunnel wide enough for blockage
/// to be small affordable at all. `half_width` is the tunnel's half width in cells, so the body
/// blocks `diameter / (2 * half_width)` of it.
fn cylinder_cd(
    diameter: f64,
    half_width: usize,
    length: usize,
    tau: f64,
    collision: Collision,
    wall: crate::flow::Wall,
    steps: u32,
    shift: f64,
    u: f64,
) -> (f64, f64, f64, bool) {
    use crate::cut::inside_of;
    use crate::grid::Occupancy;
    use crate::test_shapes::cylinder;
    let (radius, span) = (diameter / 2.0, 3usize);
    // The cylinder sits at the origin, on the symmetry plane, a quarter of the way along.
    // `shift` slides the lattice under it along the stream, which changes nothing physical and
    // changes which links the surface cuts: see `zz_cylinder_offset_scatter`.
    let ahead = (length / 4) as f64;
    let grid = Grid3::new([length, half_width, span], 1.0, Vec3::new(-ahead + shift, 0.0, -1.5)).expect("a lattice");
    let mesh = cylinder(radius, -10.0, 10.0, 256);
    let occ = Occupancy::of(grid.clone(), &[&mesh], 4).expect("occupancy");
    let inside = inside_of(&mesh);

    let mut study = FlowStudy::new(tau);
    study.collision = collision;
    study.wall = wall;
    study.inlet = [u, 0.0, 0.0];
    // Free-slip below is the symmetry plane; free-slip above is the tunnel wall, which does not
    // drag. Periodic along the axis makes the cylinder endless, which is what the reference is.
    study.sides = [Side::Inlet, Side::Outlet, Side::FreeSlip, Side::FreeSlip, Side::Periodic, Side::Periodic];
    study.steps = steps;
    study.settle = 1e-7;
    study.check_every = 500;
    let mut flow = Flow::around_surface(&occ, &inside, &study).expect("a flow");
    flow.set_everywhere(|c| if occ.is_solid(c) { (1.0, Vec3::ZERO) } else { (1.0, Vec3::new(u, 0.0, 0.0)) });
    let report = flow.run(&study).expect("run");
    // Half the body is in the lattice, so it feels half the force.
    let cd = 2.0 * flow.force().x / (0.5 * u * u * diameter * span as f64);
    let blockage = diameter / (2.0 * half_width as f64);
    (blockage, cd, report.change, report.settled)
}

/// The same case, reported every `chunk` steps as it develops.
///
/// One flow carried through several `run` calls rather than several flows: the state stays, and
/// each call's force is the mean over the second half of its own chunk, so what is printed is a
/// local average that can be watched stop moving. Whether the drag has settled is a question
/// about the drag, and the lattice's largest single velocity change -- which one awkward cell
/// can hold up for ever -- is not the way to ask it.
fn cylinder_cd_over_time(diameter: f64, half_width: usize, length: usize, tau: f64, chunk: u32, chunks: usize) -> Vec<(u32, f64, f64)> {
    use crate::cut::inside_of;
    use crate::flow::Wall;
    use crate::grid::Occupancy;
    use crate::test_shapes::cylinder;
    let (radius, span) = (diameter / 2.0, 3usize);
    let ahead = (length / 4) as f64;
    let grid = Grid3::new([length, half_width, span], 1.0, Vec3::new(-ahead, 0.0, -1.5)).expect("a lattice");
    let mesh = cylinder(radius, -10.0, 10.0, 256);
    let occ = Occupancy::of(grid.clone(), &[&mesh], 4).expect("occupancy");
    let inside = inside_of(&mesh);
    let u = 0.05;
    let mut study = FlowStudy::new(tau);
    study.wall = Wall::Interpolated;
    study.inlet = [u, 0.0, 0.0];
    study.sides = [Side::Inlet, Side::Outlet, Side::FreeSlip, Side::FreeSlip, Side::Periodic, Side::Periodic];
    study.steps = chunk;
    study.settle = 0.0;
    study.check_every = chunk;
    let mut flow = Flow::around_surface(&occ, &inside, &study).expect("a flow");
    flow.set_everywhere(|c| if occ.is_solid(c) { (1.0, Vec3::ZERO) } else { (1.0, Vec3::new(u, 0.0, 0.0)) });
    let mut out = Vec::new();
    for n in 1..=chunks {
        let report = flow.run(&study).expect("run");
        let cd = 2.0 * flow.force().x / (0.5 * u * u * diameter * span as f64);
        out.push((chunk * n as u32, cd, report.change));
    }
    out
}

#[test]
#[ignore = "diagnostic: how many steps the drag actually needs, and whether that depends on the resolution (three hours)"]
fn zz_cylinder_settling() {
    // The cheap case, carried a long way, to find what settling costs before paying it on the
    // expensive ones. The advective time across this domain is 40*D/u.
    let (d, half_width, length, tau) = (8.0, 80usize, 320usize, 0.56);
    println!("diameter {d} cells, advective time {:.0} steps", 40.0 * d / 0.05);
    let mut last = f64::NAN;
    for (steps, cd, change) in cylinder_cd_over_time(d, half_width, length, tau, 20_000, 16) {
        println!("  {steps:>7} steps  Cd {cd:.5}  moved {:+.2e}  max change {change:.2e}", cd - last);
        last = cd;
    }

    // The same question at the three resolutions the order sweeps are read from, which is the part
    // one diameter cannot answer. Every run in those sweeps stops on its step budget rather than on
    // the tolerance, reporting a largest-single-velocity-change of 1.3e-4 to 2.6e-4 against 1e-7.
    // The doc comment above says why that flag is the wrong way to ask -- one awkward cell holds it
    // up for ever -- and the chapter has already withdrawn a claim built on reading it as a verdict
    // on the drag. What has *not* been checked is whether whatever drift remains at the budget
    // depends on the diameter, and a drift that shrinks with the diameter is indistinguishable from
    // an order of convergence. The interpolated sweep's own residuals are not even monotone in `d`
    // (2.60e-4, 1.29e-4, 1.71e-4), so this is a question about the drag's late-time motion and not
    // about those numbers.
    //
    // Each resolution is carried over exactly the budget its sweep run gets -- 400 transits, in
    // eight chunks of fifty -- at that run's own `tau`, so what comes out is the drift the sweep's
    // own means carry rather than the drift of some longer or shorter run. The figure to read is
    // the last column against the steps the sweeps are reading, 0.0127 and 0.0155 acoustic and
    // whatever the fixed-`tau` sweep returns: a late drift well under those leaves the order
    // measurements alone, and a late drift that shrinks with `d` and approaches them does not.
    println!("drift over the sweep's own budget, by resolution");
    for d in [8.0, 16.0, 32.0] {
        let (u, re) = (0.05, 20.0);
        let tau = 3.0 * u * d / re + 0.5;
        let (half_width, length) = ((d * 10.0) as usize, (d * 40.0) as usize);
        // Fifty transits a chunk, eight chunks: `50 * d / u` steps each, so 400 transits in all.
        let chunk = (50.0 * d / u) as u32;
        let series = cylinder_cd_over_time(d, half_width, length, tau, chunk, 8);
        let mut last = f64::NAN;
        for (steps, cd, change) in &series {
            println!("  diameter {d:>4}  {steps:>7} steps  Cd {cd:.5}  moved {:+.2e}  max change {change:.2e}", cd - last);
            last = *cd;
        }
        // The second half's own motion: what the mean would still be worth moving if the budget
        // were doubled, measured where the transient is long gone rather than across the whole run.
        if let (Some((_, early, _)), Some((_, late, _))) = (series.get(3), series.last()) {
            println!(
                "  diameter {d:>4}: Cd moved {:+.5} over the second half of its budget, {:.2}% of the drag",
                late - early,
                100.0 * (late - early).abs() / late.abs().max(1e-12)
            );
        }
    }
    println!("against the acoustic sweep's steps of 0.0127 and 0.0155 between its placement-averaged means");
}

#[test]
#[ignore = "measurement: how much of the drag's resolution trend is the link pattern"]
fn zz_cylinder_offset_scatter() {
    use crate::flow::Wall;
    // The resolution sweep reads an order of convergence out of differences of a couple of parts
    // in a hundred. Before that is called an order it has to be larger than the noise it sits on,
    // and on a lattice that noise has a specific source: which links the model's surface cuts,
    // and where, changes in jumps as the body moves under the lattice, not smoothly.
    //
    // So: one resolution, one Reynolds number, one everything, with the lattice slid under the
    // cylinder by fractions of a cell along the stream. Nothing physical differs between these
    // runs -- same diameter, same blockage, same viscosity, same domain -- and only the pattern
    // of cut links does. The spread is therefore the discretisation noise that every point of
    // the resolution sweep carries, and if it is the size of the differences between resolutions
    // then the order read from them is being read out of scatter.
    //
    // Only the along-stream pattern varies: the cylinder sits on a free-slip symmetry plane, so
    // sliding it across the stream would move it off that plane and solve a different problem.
    // The spread measured here is thus a lower bound on the whole noise, not all of it.
    // Both of the resolutions the sweep and the published comparison actually rest on. The
    // scatter should fall as the body is refined -- more cut links, so more of the misplacement
    // cancels -- and if it does then the sweep's three points do not even carry the same noise
    // as each other, which is a second reason an order read from them means nothing.
    for (d, tau) in [(8.0, 0.56), (16.0, 0.62)] {
        let (half_width, length) = ((d * 10.0) as usize, (d * 40.0) as usize);
        let steps = (400.0 * d / 0.05) as u32;
        let mut cds: Vec<f64> = Vec::new();
        for quarter in 0..4 {
            let shift = quarter as f64 / 4.0;
            let (_, cd, change, settled) = cylinder_cd(d, half_width, length, tau, Collision::regularized(), Wall::Interpolated, steps, shift, 0.05);
            println!("diameter {d:>4} cells  shift {shift:.2} cells  Cd {cd:.4}  change {change:.2e}  settled {settled}");
            cds.push(cd);
        }
        let (lo, hi) = cds.iter().fold((f64::MAX, f64::MIN), |(l, h), c| (l.min(*c), h.max(*c)));
        let mean = cds.iter().sum::<f64>() / cds.len() as f64;
        println!("  diameter {d}: spread {:.4} over a mean of {mean:.4}, {:.2}% of it", hi - lo, 100.0 * (hi - lo) / mean);
        // The settled sweep stepped by these between its three resolutions. A spread the size of
        // the signal is the signal being noise.
        println!(
            "  against the settled sweep's steps of 0.0264 (8 to 16 cells) and 0.0152 (16 to 32): {:.0}% and {:.0}%",
            100.0 * (hi - lo) / 0.0264,
            100.0 * (hi - lo) / 0.0152
        );
    }
}

#[test]
#[ignore = "diagnostic: is the half-cell alignment the interpolation or the occupancy"]
fn zz_cylinder_alignment_outlier() {
    use crate::flow::Wall;
    // At eight cells across, three of four sub-cell placements agree to a quarter of a per cent
    // and the half-cell one reads 2.5% above them. That is not scatter, it is one alignment
    // behaving differently, and there are two candidate causes. Either the occupancy pattern at
    // that alignment is genuinely a worse staircase -- in which case a halfway wall, which knows
    // nothing about `q`, shows the same jump -- or it is Bouzidi's rule at the small values of
    // `q` that a degenerate alignment produces, where the `q < 1/2` branch leans almost entirely
    // on the *further* of the two neighbours it interpolates between. Only one of those shows up
    // under both wall treatments, so running both separates them.
    let (d, tau) = (8.0, 0.56);
    let (half_width, length) = ((d * 10.0) as usize, (d * 40.0) as usize);
    let steps = (400.0 * d / 0.05) as u32;
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        let mut cds: Vec<f64> = Vec::new();
        for quarter in 0..4 {
            let shift = quarter as f64 / 4.0;
            let (_, cd, _, _) = cylinder_cd(d, half_width, length, tau, Collision::regularized(), wall, steps, shift, 0.05);
            println!("{wall:?}  shift {shift:.2}  Cd {cd:.4}");
            cds.push(cd);
        }
        // The half-cell case against the mean of the other three, which is the jump in question.
        let odd = cds.get(2).copied().unwrap_or(f64::NAN);
        let rest = [cds.first(), cds.get(1), cds.get(3)].iter().filter_map(|c| c.copied()).sum::<f64>() / 3.0;
        println!("  {wall:?}: the half-cell alignment is {:+.2}% off the other three", 100.0 * (odd - rest) / rest);
    }
}

#[test]
#[ignore = "benchmark: the same Reynolds number realised at three lattice speeds"]
fn zz_cylinder_drag_is_the_reynolds_numbers_alone() {
    use crate::flow::Wall;
    // The published comparison above is one case at one Reynolds number, which is thin. This
    // widens it without needing a second published figure, by checking an exact property
    // instead: for a given geometry the drag coefficient is a function of the Reynolds number
    // and of nothing else. The lattice speed is a free numerical choice -- it sets the time step
    // and nothing physical -- so realising one Reynolds number at several speeds, with the
    // viscosity moved to compensate, must give one drag coefficient.
    //
    // It cannot do so exactly, and what it fails by is the method's central approximation: the
    // compressibility error grows as the square of the speed and the f32 round-off in the force
    // as its reciprocal (`zz_wall_precision_floor`). So this reads out the width of the usable
    // window directly, on the quantity a user actually asks for.
    //
    // Same diameter, same tunnel, same placement on the cells, so the staircase and the
    // placement are common to all three and cancel out of the comparison.
    let (d, re) = (8.0, 20.0);
    let (half_width, length) = ((d * 10.0) as usize, (d * 40.0) as usize);
    let mut cds: Vec<f64> = Vec::new();
    for u in [0.0125, 0.025, 0.05] {
        let tau = 3.0 * u * d / re + 0.5;
        // Ten transits apiece, and a transit takes longer the slower the stream.
        let steps = (400.0 * d / u) as u32;
        let (_, cd, change, settled) = cylinder_cd(d, half_width, length, tau, Collision::regularized(), Wall::Interpolated, steps, 0.0, u);
        println!("u {u:.4}  tau {tau:.3}  Mach {:.3}  steps {steps}  Cd {cd:.4}  change {change:.2e}  settled {settled}", u * 3.0f64.sqrt());
        cds.push(cd);
    }
    let (lo, hi) = cds.iter().fold((f64::MAX, f64::MIN), |(l, h), c| (l.min(*c), h.max(*c)));
    let mean = cds.iter().sum::<f64>() / cds.len() as f64;
    println!("spread {:.4} over a mean of {mean:.4}, {:.2}% of it -- against 0.24% for merely sliding the body", hi - lo, 100.0 * (hi - lo) / mean);
}

#[test]
#[ignore = "benchmark: cylinder drag extrapolated to an unblocked tunnel"]
fn zz_cylinder_drag_benchmark() {
    use crate::flow::Wall;
    // Dennis and Chang (1970), steady 2D flow past a circular cylinder in an unbounded stream:
    // Cd = 2.045 at Re 20. A tunnel of any width reads high, and the excess falls off with the
    // blockage, so the comparable number is what these extrapolate to at no blockage at all.
    let d = 16.0;
    let mut points: Vec<(f64, f64)> = Vec::new();
    for half_width in [80usize, 160, 320] {
        // Ten transits of the domain. The drag settles on the time the flow takes to cross it,
        // 640/u here, and takes several of them: measured on the cheap case, which held Cd to two
        // parts in ten thousand from six transits onward (`zz_cylinder_settling`). The lattice's
        // largest single velocity change never reaches the threshold, so `settled` stays false
        // long after the drag has stopped moving -- it is a max over every cell, and one
        // jittering cell holds it up for ever.
        let (blockage, cd, change, settled) = cylinder_cd(d, half_width, 640, 0.62, Collision::regularized(), Wall::Interpolated, 128_000, 0.0, 0.05);
        println!("blockage {:>5.2}%  Cd {cd:.4}  change {change:.2e}  settled {settled}", 100.0 * blockage);
        points.push((blockage, cd));
    }
    // How far from a straight line the points are decides how much the extrapolation is worth,
    // so the slope between each neighbouring pair is printed before any fit is attempted. A
    // slope that moves is a fit whose intercept depends on which points go into it.
    for pair in points.windows(2) {
        if let [(x0, y0), (x1, y1)] = pair {
            println!("  slope between {:.1}% and {:.1}%: {:.3}", 100.0 * x0, 100.0 * x1, (y0 - y1) / (x0 - x1));
        }
    }
    // Least squares through Cd = a + b*blockage, read at blockage zero.
    let at_zero = |ps: &[(f64, f64)]| -> f64 {
        let n = ps.len() as f64;
        let (sx, sy): (f64, f64) = (ps.iter().map(|p| p.0).sum(), ps.iter().map(|p| p.1).sum());
        let sxx: f64 = ps.iter().map(|p| p.0 * p.0).sum();
        let sxy: f64 = ps.iter().map(|p| p.0 * p.1).sum();
        let b = (n * sxy - sx * sy) / (n * sxx - sx * sx);
        (sy - b * sx) / n
    };
    let all = at_zero(&points);
    let near = at_zero(points.get(1..).unwrap_or(&points));
    let (lo, hi) = (all.min(near), all.max(near));
    println!("extrapolated to no blockage: {all:.4} through all of them, {near:.4} through the two narrowest");
    println!(
        "so Cd is {lo:.2} to {hi:.2} against a reference of 2.045: out by {:+.1}% to {:+.1}%, and the spread between \
         the two fits is the extrapolation's own and not the solver's",
        100.0 * (lo - 2.045) / 2.045,
        100.0 * (hi - 2.045) / 2.045
    );
}

#[test]
#[ignore = "measurement: how much of a resolution sweep's step is the relaxation time it forces"]
fn zz_cylinder_drag_against_the_relaxation_time_one_resolution_at_a_time() {
    use crate::flow::Wall;
    // `zz_cylinder_drag_order_placement_averaged` came out at -0.29: the means rise 2.1351,
    // 2.1477, 2.1632 and the steps *grow*, 0.0127 then 0.0155. A sequence whose steps grow is
    // not converging, so before anything is concluded about the staircase, the sweep has to
    // answer for a second thing it moved.
    //
    // It holds the lattice speed at 0.05 and the Reynolds number at 20, so `tau = 3*u*d/Re + 1/2`
    // rises with the diameter: 0.56, 0.62, 0.74. That is acoustic scaling, and each step between
    // resolutions therefore mixes a finer staircase with a different relaxation time. The earlier
    // bound on this -- 0.13% over `tau` 0.515 to 0.560 from
    // `zz_cylinder_drag_is_the_reynolds_numbers_alone` -- was measured over a quarter of the range
    // the sweep spans, and quoting it as "a tenth of the signal at most" extended it further than
    // it reaches.
    //
    // So: one diameter, one placement, the same four `tau` the sweep visits. The staircase and the
    // placement are then common to every run and cancel out of the comparison, which is the only
    // way to see the relaxation time on its own. What this *cannot* separate is the lattice speed
    // itself -- `tau`, `u`, `d` and `Re` are tied by one equation, so moving `tau` at fixed `d`
    // and `Re` means moving `u`, and with it the compressibility error and the f32 force floor.
    // The reading is therefore an upper bound on what `tau` contributes, which is what the
    // question needs: if the bound is small the order measurement survives, and if it is the size
    // of the steps themselves the order measurement is void.
    //
    // Both walls, because the mechanism is checkable rather than guessable.
    // `Collision::Regularized`'s own documentation says `magic` holds a *bounce-back* wall half a
    // cell out whatever the viscosity -- a statement about bounce-back, not about an interpolated
    // wall placed at a measured cut. If the halfway wall comes out flat across `tau` and the
    // interpolated one does not, the interpolation's own viscosity dependence is where to look.
    //
    // THE ANSWER, 4.6 hours, eight runs: the bound is the size of the steps, so the order
    // measurement is void.
    //
    //   wall          tau 0.53  0.56     0.62     0.74    | across the sweep's 0.56-0.74
    //   HalfWay         2.1706  2.1710   2.1738   2.1870  | 0.74%
    //   Interpolated    2.1423  2.1427   2.1454   2.1579  | 0.71%
    //
    // Against the sweep's own total rise of 1.32%, that is over half of it from `tau` alone. Worse
    // for the sweep, and the point that settles it: at the three `tau` the sweep visits this curve
    // steps 0.0027 then 0.0125 -- 21% then 81% of the sweep's steps of 0.0127 and 0.0155 -- and it
    // *grows* by 0.0098 where the sweep's own steps grow by 0.0028. The growth that made the
    // exponent negative is three and a half times over-explained by this one confound, and the
    // mechanism is plain once seen: `tau - 1/2 = 3*u*d/Re` is proportional to the diameter, so
    // acoustic scaling *doubles* it at every refinement, and any smooth error in `tau` therefore
    // contributes steps that double rather than halve. A divergent-looking sequence is what
    // acoustic scaling manufactures, not what the lattice did.
    //
    // Two things this refutes, both of them mine. The interpolation's cut-fraction dependence is
    // *not* the mechanism: the two walls drift by the same amount, and the offset between them is
    // constant to 0.0283, 0.0283, 0.0284, 0.0291 across a fourteenfold range of viscosity -- which
    // is the magic parameter doing exactly what it claims, on both walls. And an earlier note of
    // mine that a viscosity-dependent wall shift "would not obviously make each refinement step
    // larger than the last" is wrong; the proportionality above is how it does.
    //
    // What this still does not do is attribute the drift, and it never could: at fixed `d` and
    // `Re`, `Ma` is proportional to `tau - 1/2`, so the compressibility error and the viscosity
    // error are collinear along the only line this test can walk. Correcting the sweep by the
    // whole of this curve gives an order of about 1.7 on the interpolated wall and about 2.1 on the
    // halfway one, and *both of those are now withdrawn as quantities*. Two measurements say this
    // curve does not transfer across resolutions: subtracting it from the bounce-back leg turns
    // that leg's steps into +0.0027 then -0.0081, a sign change, and the fixed-`tau` sweep's
    // measured first step is +0.0016 where subtracting this curve from the acoustic first step
    // predicted +0.0100. A derivative measured at one diameter is not a correction at three. What
    // survives is the direction: the relaxation time accounts for enough of the acoustic sweep to
    // void its exponent, and how much is not quantified by this test.
    let (d, re) = (16.0, 20.0);
    let (half_width, length) = ((d * 10.0) as usize, (d * 40.0) as usize);
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        let mut cds: Vec<(f64, f64)> = Vec::new();
        for u in [0.0125, 0.025, 0.05, 0.1] {
            let tau = 3.0 * u * d / re + 0.5;
            // Ten transits apiece, and a transit takes longer the slower the stream.
            let steps = (400.0 * d / u) as u32;
            let (_, cd, change, settled) = cylinder_cd(d, half_width, length, tau, Collision::regularized(), wall, steps, 0.0, u);
            println!(
                "{wall:?}  u {u:.4}  tau {tau:.3}  Mach {:.3}  steps {steps}  Cd {cd:.4}  change {change:.2e}  settled {settled}",
                u * 3.0f64.sqrt()
            );
            cds.push((tau, cd));
        }
        let only = |a: f64, b: f64| {
            let got: Vec<f64> = cds.iter().filter(|(t, _)| *t >= a - 1e-9 && *t <= b + 1e-9).map(|(_, c)| *c).collect();
            let (lo, hi) = got.iter().fold((f64::MAX, f64::MIN), |(l, h), c| (l.min(*c), h.max(*c)));
            let mean = if got.is_empty() { 1.0 } else { got.iter().sum::<f64>() / got.len() as f64 };
            (hi - lo, 100.0 * (hi - lo) / mean)
        };
        let (_, all) = only(0.0, 10.0);
        let (_, swept) = only(0.56, 0.74);
        println!("  {wall:?}: {all:.2}% across tau 0.53 to 0.74, {swept:.2}% across the 0.56 to 0.74 the sweep visits");
        println!("  {wall:?}: against steps of 0.59% and 0.72% between the sweep's means");
    }
}

/// What a three-point sequence of means supports being read as.
///
/// Separated from the benchmark that produces one because the benchmarks are hour-scale and this is
/// arithmetic: `a_three_point_sequence_is_read_as_what_it_is` exercises every arm below in
/// milliseconds. Each arm the chapter has got wrong was got wrong in print, by a formula that
/// returned a number for data it did not apply to, so the deciding happens here and the printing
/// happens elsewhere.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reading {
    /// One of the two steps is exactly zero, so there is no ratio to take an order from.
    NoStep,
    /// The steps change sign: the sequence overshoots and comes back. Not divergence and not
    /// convergence, and the usual cause is two error terms of opposite sign crossing over.
    Overshoot { first: f64, second: f64 },
    /// The steps grow. The formal exponent is zero or negative and Richardson's denominator
    /// `2^p - 1` is zero or negative with it, so the formula extrapolates *away* from the data.
    Diverging { order: f64 },
    /// The steps shrink, but Richardson's correction is larger than the whole distance the three
    /// means travel, so the limit it would give is set by the exponent rather than by the
    /// measurement. For three points this is exactly `p < 1/2`.
    OrderOnly { order: f64, correction: f64, travelled: f64 },
    /// An order, and a limit the measurement is entitled to.
    Extrapolated { order: f64, limit: f64 },
}

/// Read three means as what they are, deciding the case before applying any formula to them.
///
/// The quiet failure this exists to stop: `log2(s0/s1)` is an order only when the two steps share a
/// sign and shrink. A sign change makes the ratio negative and the logarithm `NaN`, and `NaN` fails
/// *every* comparison, so a guard phrased as `order > 0.0` sends a sign change into whichever arm
/// is written last -- which in this file announced "the steps are growing", a thing a sign change
/// is not.
fn read_three_means(c0: f64, c1: f64, c2: f64) -> Reading {
    let (s0, s1) = (c1 - c0, c2 - c1);
    if s0 == 0.0 || s1 == 0.0 {
        return Reading::NoStep;
    }
    if s0.signum() != s1.signum() {
        return Reading::Overshoot { first: s0, second: s1 };
    }
    // Same sign from here, so the ratio is positive and the logarithm is real.
    let order = (s0 / s1).log2();
    if s1.abs() >= s0.abs() {
        return Reading::Diverging { order };
    }
    // Shrinking, so `s0/s1 > 1` and the order is positive by construction rather than by a test.
    // What still has to be checked is the lever: Richardson adds `s1 / (2^p - 1)` to the finest
    // mean, and that runs away as the order falls -- 1.00 at first order, 1.57 at 0.71, 13.4 at
    // 0.10. A positive exponent is not on its own a licence to extrapolate. The test is that the
    // correction be no larger than the distance the three means have travelled, which for three
    // points works out to exactly `p >= 1/2`: with `x = 2^p`, `travelled = s1*(x + 1)` and
    // `correction = s1/(x - 1)`, so the test is `x^2 >= 2`. It is a fixed threshold wearing the
    // clothes of a data-dependent one, which is worth knowing when reading it.
    // `zz_cylinder_drag_order_bounce_back_two_rates` is why it is here: it came out at `p = 0.104`
    // and an earlier guard, which asked only for a positive exponent, printed Cd 2.2415 -- a tenth
    // of an order buying a correction thirteen times the step it was derived from, and landing
    // nowhere near the 2.05 of Dennis and Chang.
    let correction = s1 / (2.0f64.powf(order) - 1.0);
    let travelled = (c2 - c0).abs();
    if correction.abs() <= travelled {
        Reading::Extrapolated { order, limit: c2 + correction }
    } else {
        Reading::OrderOnly { order, correction, travelled }
    }
}

#[test]
fn a_three_point_sequence_is_read_as_what_it_is() {
    // Every arm, including the two an `order > 0.0` guard could not reach. The numbers are the
    // chapter's own wherever it has any.
    //
    // The placement-averaged acoustic sweep: steps that grow, formal exponent -0.29.
    let Reading::Diverging { order } = read_three_means(2.1351, 2.1477, 2.1632) else {
        panic!("the acoustic sweep's means are a diverging sequence");
    };
    // The run itself printed -0.290 from means it had to four more digits of; these four-decimal
    // ones give -0.299, because the first step is 0.01266 and rounds up in the log and down here.
    // The arm is the thing under test, so the figure is checked loosely and the discrepancy is
    // written down rather than tuned away.
    assert!(order < 0.0 && (order - -0.29).abs() < 0.02, "formal exponent {order}");
    // The bounce-back leg: shrinking, but a tenth of an order, so an order and no limit. The lever
    // is 13.4 and the correction it wants is thirteen times the step and ten times the travel.
    let Reading::OrderOnly { order, correction, travelled } = read_three_means(2.1620, 2.1675, 2.1726) else {
        panic!("a tenth of an order must not be extrapolated from");
    };
    assert!((order - 0.104).abs() < 0.005, "formal exponent {order}");
    assert!(correction.abs() > travelled, "{correction} against {travelled}");
    // Exactly at the threshold the test works out to, and just inside it: `p = 1/2` is the first
    // order a three-point sequence may be extrapolated from, and it is admitted rather than
    // rejected. Steps of `s0 = sqrt(2)*s1` put it there.
    let s1 = 0.01;
    let at_half = read_three_means(0.0, 2.0f64.sqrt() * s1, 2.0f64.sqrt() * s1 + s1);
    let Reading::Extrapolated { order, .. } = at_half else { panic!("p = 1/2 is admissible, got {at_half:?}") };
    assert!((order - 0.5).abs() < 1e-12, "{order}");
    // First order, where the lever is 1.00 and the correction is exactly one more step.
    let Reading::Extrapolated { order, limit } = read_three_means(0.0, 1.0, 1.5) else {
        panic!("first order is extrapolable");
    };
    assert!((order - 1.0).abs() < 1e-12 && (limit - 2.0).abs() < 1e-12, "{order}, {limit}");
    // A sign change. This is the arm that mattered: the ratio is negative, so the logarithm is
    // `NaN`, and the old guard called it growing steps.
    assert_eq!(read_three_means(0.0, 0.01, 0.005), Reading::Overshoot { first: 0.01, second: -0.005 });
    assert!((0.01f64 / -0.005).log2().is_nan(), "the hazard this arm exists for");
    // And a step of exactly zero, which has no ratio either way.
    assert_eq!(read_three_means(1.0, 1.0, 2.0), Reading::NoStep);
    assert_eq!(read_three_means(1.0, 2.0, 2.0), Reading::NoStep);
}

/// How a sweep keeps the Reynolds number while the diameter changes.
///
/// `Re = u*d/nu` and `nu = (tau - 1/2)/3`, so `u`, `tau` and `d` are tied by one equation and a
/// sweep that refines at a fixed Reynolds number *must* move either the lattice speed or the
/// relaxation time. There is no third option, and which one a sweep moves is its own choice rather
/// than a property of the method -- a distinction that cost this chapter an exponent.
#[derive(Clone, Copy, Debug)]
enum SweepScaling {
    /// Hold the lattice speed, and with it the Mach number; `tau` rises with the diameter.
    ///
    /// The convention the refinement literature uses. It is also what produced the -0.29: across
    /// 8, 16 and 32 cells it walks `tau` 0.56, 0.62, 0.74, and
    /// `zz_cylinder_drag_against_the_relaxation_time_one_resolution_at_a_time` measured that walk
    /// as worth 0.71% of the drag on its own -- more than half the 1.32% the whole sweep moved.
    Acoustic { u: f64 },
    /// Hold the relaxation time, and with it the viscosity; the lattice speed falls as `1/d`.
    ///
    /// The scaling that takes the relaxation time out of the sweep -- and puts the Mach number in,
    /// which is not an improvement so much as a different confound. `Ma` is proportional to `u`, so
    /// `Ma^2` falls as `1/d^2` and *quarters* at every refinement, giving steps in the ratio 4:1
    /// and an apparent order of exactly 2.00 from compressibility alone. Where `Acoustic`
    /// manufactures a divergence, this manufactures a clean second order. Neither is clean; they
    /// fail in opposite directions, which is what makes running both worth more than running
    /// either. Its other price is the floor
    /// under the lattice speed: the distributions are f32, so the force loses about tenfold more
    /// to round-off per decade of speed, and `MIN_LATTICE_SPEED` is where that stops being
    /// usable. The way to buy room is to anchor at the *coarse* end, since the speed only falls
    /// from there -- at `Re = 20`, `tau = 0.62` puts 8 cells at exactly `MAX_LATTICE_SPEED` and
    /// leaves three doublings before the floor.
    Diffusive { tau: f64 },
}

impl SweepScaling {
    /// The relaxation time and lattice speed this scaling asks for at one diameter.
    fn at(self, d: f64, re: f64) -> (f64, f64) {
        match self {
            SweepScaling::Acoustic { u } => (3.0 * u * d / re + 0.5, u),
            SweepScaling::Diffusive { tau } => (tau, (tau - 0.5) * re / (3.0 * d)),
        }
    }
}

#[test]
fn a_sweep_holds_the_reynolds_number_whichever_thing_it_moves() {
    // The two scalings are one line of arithmetic each and a twenty-one hour benchmark turns on
    // them, so they are checked here in milliseconds instead. Both must put the same Reynolds
    // number on every resolution -- that is the whole point of a scaling -- and the diffusive one
    // must land inside the usable speed window at the three diameters it is used at, because a
    // speed under `MIN_LATTICE_SPEED` would have the benchmark measuring f32 round-off.
    let re = 20.0;
    for d in [8.0, 16.0, 32.0] {
        for scaling in [SweepScaling::Acoustic { u: 0.05 }, SweepScaling::Diffusive { tau: 0.62 }] {
            let (tau, u) = scaling.at(d, re);
            let got = u * d / ((tau - 0.5) / 3.0);
            assert!((got - re).abs() < 1e-9, "{scaling:?} at {d} cells gives Re {got}, not {re}");
        }
    }
    // Acoustic: the speed is held and `tau` walks the range that cost this chapter an exponent.
    let acoustic: Vec<f64> = [8.0, 16.0, 32.0].iter().map(|d| SweepScaling::Acoustic { u: 0.05 }.at(*d, re).0).collect();
    assert_eq!(acoustic.iter().map(|t| (t * 100.0).round() as i64).collect::<Vec<_>>(), vec![56, 62, 74]);
    // Diffusive: `tau` is held and the speed halves, anchored on `MAX_LATTICE_SPEED` at the coarse
    // end so that all three stay above the floor. This is the property the anchoring was chosen
    // for, and the one that would silently break if `tau` were nudged.
    let speeds: Vec<f64> = [8.0, 16.0, 32.0].iter().map(|d| SweepScaling::Diffusive { tau: 0.62 }.at(*d, re).1).collect();
    assert!((speeds.first().copied().unwrap_or_default() - crate::flow::MAX_LATTICE_SPEED).abs() < 1e-12);
    for u in &speeds {
        assert!(
            (crate::flow::MIN_LATTICE_SPEED..=crate::flow::MAX_LATTICE_SPEED).contains(u),
            "diffusive sweep wants a lattice speed of {u}, outside the usable window"
        );
    }
    assert_eq!(speeds.iter().map(|u| (u * 1e4).round() as i64).collect::<Vec<_>>(), vec![1000, 500, 250]);
}

/// The placement-averaged resolution sweep, for one wall treatment, operator and scaling.
///
/// A resolution sweep cannot give an order from single runs, because each point is one arbitrary
/// placement of the body on the cells and the exponent moves from -0.03 to 1.02 depending on which
/// placements go in (`zz_cylinder_offset_scatter`). The fix is not subtle, only expensive: average
/// over placements at *every* resolution, so the quantity being refined is the same quantity each
/// time. Four quarter-cell placements is a uniform sample of the unit cell, and it is taken at all
/// three resolutions rather than at the cheap ones only, since an averaging rule that changes with
/// the resolution is what produced the spurious exponents in the first place.
///
/// The Reynolds number is held at 20 by [`SweepScaling`], and that choice is not a detail: under
/// `SweepScaling::Acoustic` the relaxation time refines along with the lattice, and
/// `zz_cylinder_drag_against_the_relaxation_time_one_resolution_at_a_time` measured what that is
/// worth -- 0.71% of the drag across the `tau` the sweep walks, against the 1.32% the sweep moved
/// in total, and with its own steps *growing* three times faster than the sweep's. An acoustic
/// sweep therefore cannot be read as an order for the staircase, whatever exponent it returns.
/// `SweepScaling::Diffusive` is the one that can.
/// The four placements every leg of the sweep averages over: the lattice slid under the body by
/// each quarter of a cell along the stream, which changes nothing physical and changes which links
/// the surface cuts.
const QUARTERS: [f64; 4] = [0.0, 0.25, 0.5, 0.75];

/// Eight placements: the quarters, and the odd eighths between them.
///
/// A four-point mean cancels the first three harmonics of a one-cell-periodic error exactly; an
/// eight-point mean cancels seven. The difference between the two is what the four-point average
/// was getting wrong -- see `placement_average_residual`.
const EIGHTHS: [f64; 8] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];

/// The sweep, over whichever diameters and placements it is handed.
///
/// Both are parameters because a single process holding the whole of a twenty-one hour sweep is a
/// bad bet, and one that has now been lost: the diffusive sweep was killed ten hours in, part way
/// through the first placement of its 32-cell leg, and took that leg's work with it. The two
/// coarser legs survived only because their placement lines had already been printed. A leg named
/// on its own is restartable, and -- since nothing in `Flow` is threaded -- the four placements of
/// one leg are four independent single-threaded runs that the test harness will run concurrently
/// given `--test-threads 4`, turning nineteen and a half hours of wall clock into five.
///
/// Run a leg from a *copy* of the test binary, not the one under `target`. Windows will not relink
/// a running `.exe`, so for as long as a leg runs, every `cargo test` or `cargo build` in the same
/// checkout fails with `LNK1104` on this crate's test binary -- and the usual next move, in a
/// checkout more than one person or agent is working in, is to kill whatever holds the file. That
/// is how the first ten hours were lost. The binary is self-contained, so copying it somewhere
/// else and running the copy removes the conflict rather than negotiating it:
/// `cp target/release/deps/solidcraft_sim-*.exe /tmp/sim-bench.exe`, then run `/tmp/sim-bench.exe
/// --ignored --test-threads 4 --nocapture one_relaxation_time_tail`.
fn cylinder_drag_order_over_placements(wall: crate::flow::Wall, collision: Collision, scaling: SweepScaling, diameters: &[f64], shifts: &[f64]) {
    println!("{wall:?} wall, {collision:?}, {scaling:?}, diameters {diameters:?}, placements {shifts:?}");
    let re = 20.0;
    let mut means: Vec<(f64, f64)> = Vec::new();
    for &d in diameters {
        let (tau, u) = scaling.at(d, re);
        // A speed outside the usable window would be measuring f32 round-off or compressibility
        // rather than the lattice, so the point is dropped and said to be dropped. Reporting an
        // order over two points is the lesser error against reporting one over a bad third.
        if !(crate::flow::MIN_LATTICE_SPEED..=crate::flow::MAX_LATTICE_SPEED).contains(&u) {
            println!("diameter {d:>4}: skipped, lattice speed {u:.5} is outside the usable window");
            continue;
        }
        let (half_width, length) = ((d * 10.0) as usize, (d * 40.0) as usize);
        let steps = (400.0 * d / u) as u32;
        let mut cds: Vec<f64> = Vec::new();
        for &shift in shifts {
            let (_, cd, change, settled) = cylinder_cd(d, half_width, length, tau, collision, wall, steps, shift, u);
            println!("diameter {d:>4}  shift {shift:.2}  tau {tau:.3}  Cd {cd:.4}  change {change:.2e}  settled {settled}");
            cds.push(cd);
        }
        // An empty placement list would divide by zero and report a NaN as a drag coefficient.
        if cds.is_empty() {
            println!("diameter {d:>4}: no placements asked for");
            continue;
        }
        let mean = cds.iter().sum::<f64>() / cds.len() as f64;
        let (lo, hi) = cds.iter().fold((f64::MAX, f64::MIN), |(l, h), c| (l.min(*c), h.max(*c)));
        let noun = if cds.len() == 1 { "placement" } else { "placements" };
        println!("  diameter {d}: mean Cd {mean:.4} over {} {noun}, spanning {:.4} ({:.2}%)", cds.len(), hi - lo, 100.0 * (hi - lo) / mean);
        means.push((d, mean));
    }
    // Three means, two steps, one exponent -- the first one this chapter is entitled to quote.
    if let (Some((_, c0)), Some((_, c1)), Some((_, c2))) = (means.first(), means.get(1), means.get(2)) {
        let (s0, s1) = (c1 - c0, c2 - c1);
        println!("steps between the means: {s0:.4} and {s1:.4}");
        match read_three_means(*c0, *c1, *c2) {
            Reading::NoStep => println!("one of the steps is zero, so there is no ratio to take an order from"),
            Reading::Overshoot { first, second } => println!(
                "the steps change sign ({first:+.4} then {second:+.4}), so this sequence overshoots and comes back \
                 rather than converging: there is no order in it, and two error terms of opposite sign are the \
                 usual reason"
            ),
            Reading::Diverging { order } => println!(
                "observed order of convergence {order:.3}: the steps are growing rather than shrinking, so \
                 this sequence is not converging and Richardson has nothing it may be applied to"
            ),
            Reading::OrderOnly { order, correction, travelled } => println!(
                "observed order of convergence {order:.3}: Richardson would add {correction:+.4} to the finest \
                 mean, more than the {travelled:.4} the three means move in all, so the limit is set by the \
                 exponent rather than by the measurement and is not quoted"
            ),
            Reading::Extrapolated { order, limit } => {
                println!("observed order of convergence {order:.3}, extrapolating to Cd {limit:.4} at 5% blockage")
            }
        }
    } else {
        println!(
            "{} of the three means measured here, so no order: read it with `read_three_means` against the \
             means this leg is part of",
            means.len()
        );
    }
}

#[test]
#[ignore = "benchmark: the drag's order of convergence, averaged over placements (ten hours)"]
fn zz_cylinder_drag_order_placement_averaged() {
    // What `sim.flow` does out of the box: the interpolated wall and the regularized operator.
    // This is the sweep that came out at -0.290 -- means 2.1351, 2.1477 and 2.1632, steps of 0.0127
    // then 0.0155, which *grow* rather than shrink. A sequence whose steps grow has neither an
    // order nor a limit, so the figures of 0.71, 0.80 and 1.02 this chapter quoted at various times
    // are all withdrawn, and so is the Cd the test used to print beside the negative exponent.
    //
    // The -0.29 is withdrawn too, as a statement about the method.
    // `zz_cylinder_drag_against_the_relaxation_time_one_resolution_at_a_time` then measured the
    // relaxation time this sweep drags along with the lattice and found it worth 0.71% of the drag
    // against the sweep's total of 1.32%, with steps that grow three and a half times faster than
    // the sweep's own. The exponent this run returns is a property of acoustic scaling and the
    // staircase together, and no arithmetic on these twelve numbers can separate them. It is kept
    // because it is the measurement that found the confound, and because it is the acoustic half
    // of the pair -- `zz_cylinder_drag_order_at_one_relaxation_time` is the diffusive half, and the
    // difference between the two is the confound measured a second, independent way.
    cylinder_drag_order_over_placements(
        crate::flow::Wall::Interpolated,
        Collision::regularized(),
        SweepScaling::Acoustic { u: 0.05 },
        &[8.0, 16.0, 32.0],
        &QUARTERS,
    );
}

#[test]
#[ignore = "benchmark: the drag's order of convergence at one relaxation time (twenty-one hours)"]
fn zz_cylinder_drag_order_at_one_relaxation_time() {
    // The measurement the chapter has been missing: a resolution sweep in which the resolution is
    // the only thing that changes.
    //
    // This runs the whole of it in one process, which is how it was first run and how it was lost:
    // killed ten hours in, part way through the first placement of its 32-cell leg, with the two
    // coarser legs surviving only because their placement lines had already printed. The 8- and
    // 16-cell legs are measured at 2.1461 and 2.1477, so in practice the leg left to run is the
    // 32-cell one, and `zz_one_relaxation_time_tail_shift_0` to `_3` are that leg as four
    // concurrent placements: five hours of wall clock rather than nineteen and a half, and a kill
    // costing one placement rather than a leg. This test is kept because it is the sweep as a
    // whole, and the thing the chapter quotes.
    //
    // `zz_cylinder_drag_order_placement_averaged` averaged the placements properly and still could
    // not give an order, because holding `Re` while refining forces the relaxation time up with the
    // diameter and
    // `zz_cylinder_drag_against_the_relaxation_time_one_resolution_at_a_time` measured that forcing
    // as worth more than half the signal, with steps that grow faster than the sweep's. Correcting
    // for the whole of it gives about 1.7 and correcting for none of it gives -0.290; the two cannot be
    // separated at fixed diameter because `Ma` moves with `tau` there. They separate here, by
    // holding `tau` and letting `Ma` fall instead.
    //
    // `tau = 0.62` is chosen by the floor, not by taste. `u = (tau - 1/2)*Re/(3*d)`, so the
    // coarsest point sets the highest speed in the sweep and every later one halves it: at `Re =
    // 20` and 8 cells, `tau = 0.62` lands exactly on `MAX_LATTICE_SPEED`, giving 0.1, 0.05, 0.025
    // across the three resolutions -- all of them two and a half times or more above
    // `MIN_LATTICE_SPEED`, where the f32 force floor starts to bite. An earlier note rejected
    // diffusive scaling as unaffordable here; that was true of the anchoring it considered
    // (`tau = 0.56` pinned at the *fine* end, which needs 0.0125 at 32 cells and falls under the
    // floor at 64) and is not true in general. Anchoring at the coarse end is what buys the room.
    //
    // 0.62 is also one of the three the acoustic sweep visited, at 16 cells, so that sweep's
    // middle point and this one agree on both `tau` and `u` and the two sweeps cross there. The
    // difference between the two sequences is then the confound measured a second time, by
    // subtraction rather than by a separate experiment.
    //
    // Everything else is held to the acoustic sweep exactly -- the same four quarter-cell
    // placements at each resolution, the same four hundred transits, the same wall and operator --
    // so the two are differenceable. Four placements at 32 cells is most of the twenty-one hours
    // and is not negotiable: the placement spread is 2.65% at 8 cells against the 0.5% of signal
    // being measured, and a single placement is how this chapter got three wrong exponents.
    //
    // What a number here means, stated more carefully than the first version of this comment did.
    // It claimed this sweep leaves the staircase as the only thing refining. It does not: holding
    // `tau` makes `u` fall as `1/d`, so `Ma^2` quarters at every refinement and a compressibility
    // error alone would return steps in the ratio 4:1 -- an apparent order of exactly 2.00. An
    // order near 2 here is therefore the one reading that cannot be told apart from the confound,
    // which is unfortunate, because the first version of this comment said an order near 1 to 2
    // "prices the refinement investment and unblocks it". It would not.
    //
    // What the pair of sweeps does buy is that the two confounds push opposite ways. `Acoustic`
    // holds `Ma` fixed, so compressibility contributes a constant and nothing to its steps, while
    // `tau - 1/2` doubles and manufactures growing steps. `Diffusive` holds `tau`, so the
    // relaxation time contributes nothing to its steps, while `Ma^2` quarters and manufactures
    // second order. A staircase term is common to both. So an order well *below* 2 here is
    // informative -- compressibility cannot produce it and the relaxation time is not in it -- and
    // an order near 2 is not. The first step is already measurable against this: the acoustic
    // sweep steps +0.0127 from 8 to 16 cells and this one steps +0.0016 over the same pair, both
    // placement-averaged and sharing the 16-cell point exactly.
    //
    // And one outcome must not be read as either: the drag could converge to a limit that is
    // simply wrong, since nothing here checks it against Dennis and Chang. An order is a statement
    // about the sequence, not about the answer.
    cylinder_drag_order_over_placements(
        crate::flow::Wall::Interpolated,
        Collision::regularized(),
        SweepScaling::Diffusive { tau: 0.62 },
        &[8.0, 16.0, 32.0],
        &QUARTERS,
    );
    println!(
        "against the acoustic sweep's means of 2.1351, 2.1477 and 2.1632 at the same three diameters, \
         whose steps of 0.0127 and 0.0155 grew rather than shrank"
    );
}

/// One placement of the 32-cell leg of `zz_cylinder_drag_order_at_one_relaxation_time`.
///
/// That sweep's own run measured its 8- and 16-cell legs -- 2.1461 over four placements spanning
/// 2.69%, and 2.1477 spanning 0.20%, the latter reproducing the acoustic sweep's 16-cell placements
/// bit for bit because at 16 cells the two sweeps are the same run -- and was then killed part way
/// through the first placement of its 32-cell leg, ten hours in. That leg is nineteen and a half of
/// the twenty-one hours, so it is the whole remaining cost, and re-running the two legs that are
/// already measured would add an hour and a half of recomputation to it.
///
/// Split into four because `Flow` is not threaded: each placement is one single-threaded run of
/// 512,000 steps over 1,228,800 cells, so four of them on a machine with cores to spare cost five
/// hours of wall clock rather than nineteen and a half, and a kill costs at most one placement
/// instead of a leg. Run them with
/// `--ignored --test-threads 4 --nocapture one_relaxation_time_tail`.
///
/// The order is not printed here and must not be: each of these measures one placement of one
/// diameter, and an order is a statement about three means. Average the four and read the three
/// means through `read_three_means`.
fn one_relaxation_time_tail(shift: f64) {
    cylinder_drag_order_over_placements(
        crate::flow::Wall::Interpolated,
        Collision::regularized(),
        SweepScaling::Diffusive { tau: 0.62 },
        &[32.0],
        &[shift],
    );
}

#[test]
#[ignore = "benchmark: one placement of the fixed-relaxation-time sweep's 32-cell leg (five hours)"]
fn zz_one_relaxation_time_tail_shift_0() {
    one_relaxation_time_tail(0.0);
}

#[test]
#[ignore = "benchmark: one placement of the fixed-relaxation-time sweep's 32-cell leg (five hours)"]
fn zz_one_relaxation_time_tail_shift_1() {
    one_relaxation_time_tail(0.25);
}

#[test]
#[ignore = "benchmark: one placement of the fixed-relaxation-time sweep's 32-cell leg (five hours)"]
fn zz_one_relaxation_time_tail_shift_2() {
    one_relaxation_time_tail(0.5);
}

#[test]
#[ignore = "benchmark: one placement of the fixed-relaxation-time sweep's 32-cell leg (five hours)"]
fn zz_one_relaxation_time_tail_shift_3() {
    one_relaxation_time_tail(0.75);
}

/// How much of a placement-averaged mean is the averaging rather than the lattice.
///
/// Every order in this chapter is read off means of four quarter-cell placements, and at fixed
/// `tau` those means step by +0.0016 while one placement differs from another by 0.0578 at 8 cells
/// -- thirty-six times larger than the step being read from them. That is only safe if the
/// four-point mean really is the phase-average of the placement dependence. A four-point mean
/// cancels the first three harmonics of a one-cell-periodic error exactly, so a single smooth hump
/// would be averaged away. The 8-cell placements are not that: they come out 2.1297, 2.1353, 2.1875
/// and 2.1317, the third 0.0553 above the mean of the other three, and a spike carries
/// fourth-harmonic content which aliases straight into a four-point mean. Both sweeps show it, at
/// the same placement and almost the same size -- the acoustic leg is 2.1190, 2.1242, 2.1755,
/// 2.1216 at a different `tau` and `u` -- so it is the unlucky alignment this chapter already
/// measured at 2.5%, not noise, and it falls at shift 0.50 because that is where 8 cells across
/// the body lands symmetrically under it.
///
/// What makes it load-bearing rather than a footnote is that it does not cancel in the step. A
/// quarter of the spike's excess goes into the mean, which is +0.0138 at 8 cells; at 16 cells the
/// same placement is 0.0010 *below* its neighbours' mean and contributes -0.0002. So the 8-cell
/// mean carries an aliasing term 8.6 times the +0.0016 step being read from it, and the 16-cell
/// mean carries essentially none of it.
///
/// So the residual is measured rather than assumed away. The same leg at eight placements cancels
/// seven harmonics, and the gap between the eight-point mean and the four-point one is what the
/// four-point average was getting wrong -- to be read against the +0.0016 step, and against
/// whatever the 32-cell leg returns, before any order is quoted from either.
///
/// Done where it is both large and cheap: eight placements cost nine minutes at 8 cells, where the
/// spread is 2.69%, and two hours and a half at 16, where it is 0.20%. At 32 cells four extra
/// placements would cost nineteen and a half hours to bound the smallest of the three residuals,
/// so they are not run; if the 16-cell residual comes out large, that judgement needs revisiting
/// rather than extending.
fn placement_average_residual(d: f64) {
    cylinder_drag_order_over_placements(
        crate::flow::Wall::Interpolated,
        Collision::regularized(),
        SweepScaling::Diffusive { tau: 0.62 },
        &[d],
        &EIGHTHS,
    );
}

#[test]
#[ignore = "benchmark: how much of the 8-cell placement-averaged mean is the averaging (ten minutes)"]
fn zz_placement_average_residual_at_8_cells() {
    placement_average_residual(8.0);
    println!("against the four-placement mean of 2.1461 over 2.1297, 2.1353, 2.1875 and 2.1317");
}

#[test]
#[ignore = "benchmark: how much of the 16-cell placement-averaged mean is the averaging (two hours and a half)"]
fn zz_placement_average_residual_at_16_cells() {
    placement_average_residual(16.0);
    println!("against the four-placement mean of 2.1477 over 2.1454, 2.1487, 2.1470 and 2.1498");
}

#[test]
#[ignore = "benchmark: the same sweep on plain bounce-back, against a published order (thirteen hours)"]
fn zz_cylinder_drag_order_bounce_back_two_rates() {
    // The same twelve runs with the wall changed, which is the only thing this experiment turns on.
    //
    // `Collision::Regularized`'s own documentation says the magic parameter holds a *bounce-back*
    // wall half a cell out whatever the viscosity. That is a statement about bounce-back, not about
    // an interpolated wall placed at a measured cut, and the sweep above moves the relaxation time
    // from 0.56 to 0.74 as it refines. So the candidate mechanism for the missing order is the
    // interpolation's own viscosity dependence at cut fractions that are mostly not halves -- and
    // `Wall::HalfWay` is precisely the wall the magic parameter is known to protect.
    //
    // What this is not: a test of whether holding the magic parameter fixes it. `Λ = 3/16` is
    // already held on every level of the sweep that failed, by `regularized()` as much as by
    // `trt()`. The operator is swapped here only because doing so costs nothing extra and makes
    // the configuration the published one exactly -- two relaxation times at the magic parameter,
    // plain bounce-back, a staircase curved wall, acoustic scaling, the Reynolds number held --
    // which is the configuration waLBerla reports a clean first order for. `trt()` differs from
    // `regularized()` in dropping the stress rebuild and in nothing that touches `Λ`.
    //
    // THE PREMISE ABOVE IS REFUTED, and this test is no longer next in line.
    // `zz_cylinder_drag_against_the_relaxation_time_one_resolution_at_a_time` ran both walls over
    // the same four relaxation times and they drift by the same amount -- 0.74% on bounce-back,
    // 0.71% interpolated -- with the offset between them constant to four parts in a thousand
    // across a fourteenfold range of viscosity. So the interpolation's cut-fraction dependence is
    // not the mechanism, and swapping to the wall the magic parameter protects would change
    // nothing about the exponent: the drift is common to both walls and lives in the bulk, where
    // `tau` and the Mach number move together under acoustic scaling.
    //
    // What survives is the weaker reason: this is waLBerla's published configuration exactly, and
    // thirteen hours would say whether *their* configuration shows an order here. That is a
    // cross-check against someone else's number rather than a measurement of ours, so it waits
    // behind `zz_cylinder_drag_order_at_one_relaxation_time`, which answers the actual question.
    cylinder_drag_order_over_placements(
        crate::flow::Wall::HalfWay,
        Collision::trt(),
        SweepScaling::Acoustic { u: 0.05 },
        &[8.0, 16.0, 32.0],
        &QUARTERS,
    );
    println!("against the interpolated wall's means of 2.1351, 2.1477 and 2.1632 at the same three diameters");
}

#[test]
#[ignore = "benchmark: cylinder drag at three resolutions, blockage held fixed"]
fn zz_cylinder_drag_resolution() {
    use crate::flow::Wall;
    // The blockage study holds the cylinder at sixteen cells, so whatever error that resolution
    // carries is in its answer. This holds the blockage at 5% instead and moves the resolution,
    // which separates the two. The Reynolds number is held at 20 by moving the viscosity with
    // the diameter: Re = U·D/ν, so τ = 3·U·D/Re + 1/2.
    let (u, re) = (0.05, 20.0);
    for d in [8.0, 16.0, 32.0] {
        let tau = 3.0 * u * d / re + 0.5;
        let half_width = (d * 10.0) as usize;
        let length = (d * 40.0) as usize;
        // Ten transits apiece, the same as the blockage study: the domain is 40*d long and the
        // stream crosses it at u, so that is 400*d/u steps. One fixed count across the sweep
        // would leave the finest case the least settled, and an under-settled fine case reads as
        // a scheme that has stopped converging -- which is what the first attempt concluded.
        let steps = (400.0 * d / u) as u32;
        let (blockage, cd, change, settled) = cylinder_cd(d, half_width, length, tau, Collision::regularized(), Wall::Interpolated, steps, 0.0, 0.05);
        println!(
            "diameter {d:>4} cells  tau {tau:.3}  blockage {:.2}%  steps {steps}  Cd {cd:.4}  change {change:.2e}  settled {settled}",
            100.0 * blockage
        );
    }
}

#[test]
#[ignore = "measurement: the order the velocity field converges at, against the drag's"]
fn zz_wall_order_of_convergence() {
    use crate::flow::Wall;
    // The drag on a cylinder converges at 0.71, which is well short of the second order
    // Bouzidi's interpolation is supposed to give. That is either the boundary condition or the
    // force evaluation, and the channel can tell them apart: it has an exact answer for the
    // *field*, with no force in it at all. The same geometry is refined by doubling, so the
    // walls stay at the same fraction of the gap.
    // The refinement has to hold the walls at the same fraction of a cell, or each resolution
    // is a different problem: doubling a gap of 17.4 moves q from 0.2 to 0.9 to 0.3, which is
    // not a refinement but three unrelated cases. A gap of 16·s cells starting at 3.25 keeps
    // q at a quarter below and three quarters above at every scale.
    // Two forcing amplitudes, because the first one chosen here was the wrong one and the
    // comparison is the point. Both hold the peak velocity fixed across the sweep -- a constant
    // Mach number, which is what a refinement study needs -- but at 1e-5 the finest case sits on
    // the f32 round-off branch, four times above the best this arithmetic can do on it, and
    // reads as a scheme that has stopped converging. At 1e-4 every case is off that branch.
    // A steady channel's convective term is identically zero, so there is almost no
    // compressibility error to pay for the larger amplitude; `zz_wall_precision_floor` traces
    // both branches and shows where the floor is.
    for (wall, base) in [(Wall::HalfWay, 1e-5), (Wall::Interpolated, 1e-5), (Wall::HalfWay, 1e-4), (Wall::Interpolated, 1e-4)] {
        let mut errors: Vec<(f64, f64)> = Vec::new();
        for scale in [1usize, 2, 4] {
            let gap = 16.0 * scale as f64;
            let (lo, hi) = (3.25, 3.25 + gap);
            let across = gap as usize + 8;

            // The peak of a forced channel goes as H^2, so a fixed forcing would run the finest
            // case sixteen times faster than the coarsest -- up against the lattice's own speed
            // of sound, where the compressibility error goes as u^2 and swamps what is being
            // measured. Scaling the forcing by 1/s^2 holds the peak where it was.
            let accel = base / (scale * scale) as f64;
            errors.push(offset_channel_at(across, lo, hi, 0.8, Collision::regularized(), wall, accel));
        }
        let order = |a: f64, b: f64| (a / b).log2();
        println!(
            "{:?} accel {:.0e} field: {:.3e} {:.3e} {:.3e}  orders {:.2} {:.2}",
            wall,
            base,
            errors[0].0,
            errors[1].0,
            errors[2].0,
            order(errors[0].0, errors[1].0),
            order(errors[1].0, errors[2].0)
        );
        println!(
            "{:?} accel {:.0e} force: {:.3e} {:.3e} {:.3e}  orders {:.2} {:.2}",
            wall,
            base,
            errors[0].1,
            errors[1].1,
            errors[2].1,
            order(errors[0].1, errors[1].1),
            order(errors[1].1, errors[2].1)
        );
    }
}

#[test]
fn the_wall_force_accounts_for_every_bit_the_body_force_puts_in() {
    use crate::cut::inside_of;
    use crate::flow::Wall;
    use crate::grid::Occupancy;
    use crate::test_shapes::block_at;
    // The one thing a body-force channel settles exactly. Momentum enters at `a` per unit of
    // mass per step and can leave only through the walls, so in steady state the force on them
    // is `a` times the fluid's mass, whatever the viscosity, the operator, or where the walls
    // are taken to be. That is a statement about bookkeeping rather than about accuracy, and it
    // is the statement worth asserting: the streaming and the tally ask one function what comes
    // back off a wall, so a tally that disagreed with the flow it reported would show up here
    // and nowhere else.
    //
    // It is also the reason this geometry cannot be used to measure how *well* the tally places
    // a wall: conservation pins the answer, so an interpolated wall and a halfway one give the
    // same total to four figures even where their velocity fields differ sixteenfold.
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        for q in [0.5, 0.7, 0.98] {
            let (lo, hi, across) = (3.5 - q, 18.5 + q, 24usize);
            let grid = Grid3::new([4, across, 4], 1.0, Vec3::ZERO).expect("a lattice");
            let below = block_at([-10.0, -10.0, -10.0], [24.0, 10.0 + lo, 24.0]);
            let above = block_at([-10.0, hi, -10.0], [24.0, 10.0 + across as f64 - hi, 24.0]);
            let occ = Occupancy::of(grid.clone(), &[&below, &above], 4).expect("occupancy");
            let (in_below, in_above) = (inside_of(&below), inside_of(&above));
            let inside = |p: Vec3| in_below(p) || in_above(p);
            let accel = 1e-4;
            let mut study = FlowStudy::new(0.8);
            study.wall = wall;
            study.accel = [accel, 0.0, 0.0];
            study.sides = [Side::Periodic; 6];
            study.steps = 120_000;
            study.settle = 0.0;
            study.check_every = 120_000;
            let mut flow = Flow::around_surface(&occ, &inside, &study).expect("a flow");
            let report = flow.run(&study).expect("run");
            let put_in = accel * report.mass;
            let out = (flow.force().x - put_in).abs() / put_in;
            assert!(
                out < 2e-3,
                "{wall:?} at q {q}: the walls took {:.6e} where the body force put in {put_in:.6e}, out by {out:.2e}",
                flow.force().x
            );
        }
    }
}

#[test]
#[ignore = "measurement: does the momentum tally drift with the cut fraction"]
fn zz_tally_against_the_cut_fraction() {
    use crate::flow::Wall;
    // Momentum exchange is exact for a wall that returns what it was given: the credit
    // `-(f*_i + back)*c_i` is then `-(f*_i + f*_ī)*c_i`, which is what arrived plus what left.
    // Bouzidi returns a combination of two neighbours instead, and on the `q >= 1/2` branch the
    // algebra leaves a residue that grows with `q` -- nothing at `q = 1/2`, and
    // `-(1/2)(f*_i - f*_ī)*c_i` at `q = 1`, proportional to the momentum imbalance along the
    // link. If that is real, the tally gets worse as the wall approaches the far node while the
    // *field* stays good, and a halfway wall -- which knows nothing about `q` -- does not show it.
    //
    // The channel is where this is checkable against an exact answer: in steady state the walls
    // take exactly what the body force puts in, whatever the viscosity or the wall treatment, and
    // `q` is whatever the walls are placed at. Cell centres sit at `j + 1/2`, so a lower wall at
    // `3.5 - q` and an upper at `18.5 + q` put both cut fractions at `q` by construction.
    //
    // Run at the larger forcing, since the smaller one is below the arithmetic floor
    // (`zz_wall_precision_floor`) and would bury the effect being looked for.
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        println!("{wall:?}:");
        for q in [0.5, 0.7, 0.9, 0.98] {
            let (lo, hi) = (3.5 - q, 18.5 + q);
            let (field, force) = offset_channel_at(24, lo, hi, 0.8, Collision::regularized(), wall, 1e-4);
            // `force` here is measured against the *model's* volume, so it is the occupancy
            // quantisation -- 16 whole cells standing for a gap of 15 + 2q -- and not the
            // tally's error. `(15.0 + 2q - 16).abs() / (15 + 2q)` reproduces it to three
            // figures, which is why it comes out the same for both wall treatments.
            let quantised = (15.0 + 2.0 * q - 16.0).abs() / (15.0 + 2.0 * q);
            println!(
                "  q {q:.2}  walls at {lo:.2} and {hi:.2}  field {field:.3e}  force {force:.3e}                   which the cell count alone predicts as {quantised:.3e}"
            );
        }
    }
}

#[test]
#[ignore = "measurement: where f32 distributions stop resolving the answer"]
fn zz_wall_precision_floor() {
    use crate::flow::Wall;
    // A forced channel is a Stokes problem: linear in the forcing. So its *relative* error may
    // not depend on the forcing's amplitude at all -- except through two effects that are not
    // the scheme's. Compressibility grows as u^2, which bounds the amplitude from above; and
    // round-off grows as 1/u, because the distributions are f32 and the force tally differences
    // two numbers near w_i*rho to get one near rho*a. That bounds it from below.
    //
    // So sweeping the amplitude on one fixed lattice must trace a U, and the bottom of the U is
    // the best this arithmetic can do on this problem. Anything measured on the rising left-hand
    // branch is the floating point, not the method -- which is where the refinement study above
    // put its finest case, by scaling the forcing down as 1/s^2 to hold the peak velocity.
    let scale = 4usize;
    let gap = 16.0 * scale as f64;
    let (lo, hi) = (3.25, 3.25 + gap);
    let across = gap as usize + 8;
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        println!("{wall:?}:");
        // Four decades is the whole useful range. Below this the answer is round-off; above it
        // the lattice goes supersonic, where the scheme means nothing and, two decades up, the
        // solver stops returning a field at all rather than returning a wrong one.
        for decade in 0..4 {
            let accel = 6.25e-8 * 10.0f64.powi(decade);
            let (field, force) = offset_channel_at(across, lo, hi, 0.8, Collision::regularized(), wall, accel);
            // The peak the exact answer has, so the Mach number of the case is on the line too.
            let peak = accel * (hi - lo) * (hi - lo) / (8.0 * (0.8 - 0.5) / 3.0);
            println!("  accel {accel:.2e}  peak u {peak:.3e}  Mach {:.4}  field {field:.3e}  force {force:.3e}", peak * 3.0f64.sqrt());
        }
    }
}

#[test]
#[ignore = "diagnostic: does the interpolated wall hold the lattice's mass"]
fn zz_wall_mass_drift() {
    use crate::cut::inside_of;
    use crate::flow::Wall;
    use crate::grid::Occupancy;
    use crate::test_shapes::block_at;
    let (lo, hi, across) = (3.25, 19.25, 24usize);
    for wall in [Wall::HalfWay, Wall::Interpolated] {
        for steps in [1000u32, 30_000, 400_000] {
            let grid = Grid3::new([4, across, 4], 1.0, Vec3::ZERO).expect("a lattice");
            let below = block_at([-10.0, -10.0, -10.0], [24.0, 10.0 + lo, 24.0]);
            let above = block_at([-10.0, hi, -10.0], [24.0, 10.0 + across as f64 - hi, 24.0]);
            let occ = Occupancy::of(grid.clone(), &[&below, &above], 4).expect("occupancy");
            let (in_below, in_above) = (inside_of(&below), inside_of(&above));
            let inside = |p: Vec3| in_below(p) || in_above(p);
            let mut study = FlowStudy::new(0.8);
            study.wall = wall;
            study.accel = [1e-5, 0.0, 0.0];
            study.sides = [Side::Periodic; 6];
            study.steps = steps;
            study.settle = 1e-12;
            study.check_every = 2000;
            let mut flow = Flow::around_surface(&occ, &inside, &study).expect("a flow");
            let started: f64 = (0..grid.cells()).filter(|c| !flow.is_solid(*c)).count() as f64;
            let report = flow.run(&study).expect("run");
            println!("{wall:?} {steps:>7} steps: mass {:.6} from {started:.0}, drift {:+.3e} of it", report.mass, (report.mass - started) / started);
        }
    }
}
