//! Elasticity checked against answers worked out on paper.
//!
//! Five of these are exact, because a trilinear brick reproduces any *uniform* strain field
//! exactly: a bar pulled along its length, a part squeezed from every side, a part free to grow
//! when it is heated, a part held while it is heated, and a part held in one direction only.
//! Those are asserted to equality. The one case with a field that bends -- a cantilever -- is
//! not exact and is not pretended to be: the element is too stiff in bending, and the test
//! measures by how much and that it gets better as the elements get smaller.

use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;

use super::*;
use crate::SimError;
use crate::grid::{Grid3, Occupancy};
use crate::test_shapes::block_at;

/// Steel, more or less.
fn steel() -> ElasticMaterial {
    ElasticMaterial {
        young_modulus_pa: 200.0e9,
        poisson_ratio: 0.3,
        expansion_per_k: [1.2e-5; 3],
        density_kg_m3: 7850.0,
        yield_strength_pa: Some(250.0e6),
    }
}

const LENGTH_MM: f64 = 100.0;
const SIDE_MM: f64 = 20.0;
const LENGTH_M: f64 = 0.1;
const AREA_M2: f64 = 4.0e-4;

/// A bar a hundred millimetres long and twenty square, as `span` cells along its length. Its
/// walls land on cell boundaries, so the lattice holds it exactly.
fn bar(span: usize) -> Occupancy {
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(LENGTH_MM, SIDE_MM, SIDE_MM) };
    let grid = Grid3::over(b, span, 1).expect("lattice");
    let mesh = block_at([0., 0., 0.], [LENGTH_MM, SIDE_MM, SIDE_MM]);
    Occupancy::of(grid, &[&mesh], 1).expect("occupancy")
}

fn side(axis: usize, positive: bool) -> Patch {
    Patch::Side { axis, positive }
}

/// Rollers on the three near faces: the part cannot move through them and is free to slide
/// along them, which holds it still without stopping it changing shape. The state of strain
/// stays uniform, which is what makes these cases exact.
fn rollers() -> Vec<Load> {
    vec![
        Load::new("x roller", side(0, false), Support::Roller),
        Load::new("y roller", side(1, false), Support::Roller),
        Load::new("z roller", side(2, false), Support::Roller),
    ]
}

/// A uniform field over the eight corners of the reference cube of side `h`, as the
/// twenty-four freedoms of one element.
fn field(h: f64, f: impl Fn(Vec3) -> Vec3) -> [f64; DOFS] {
    let mut out = [0.0f64; DOFS];
    for (i, corner) in CORNERS.iter().enumerate() {
        // The reference cube spans −1 to 1; a cube of side h spans 0 to h.
        let at = Vec3::new((corner[0] + 1.0) * h / 2.0, (corner[1] + 1.0) * h / 2.0, (corner[2] + 1.0) * h / 2.0);
        let u = f(at);
        for (axis, v) in [u.x, u.y, u.z].into_iter().enumerate() {
            if let Some(slot) = out.get_mut(i * 3 + axis) {
                *slot = v;
            }
        }
    }
    out
}

fn apply(k: &[[f64; DOFS]; DOFS], u: &[f64; DOFS]) -> [f64; DOFS] {
    let mut out = [0.0f64; DOFS];
    for (i, row) in k.iter().enumerate() {
        let mut v = 0.0;
        for (c, uj) in row.iter().zip(u) {
            v += c * uj;
        }
        if let Some(slot) = out.get_mut(i) {
            *slot = v;
        }
    }
    out
}

fn energy(k: &[[f64; DOFS]; DOFS], u: &[f64; DOFS]) -> f64 {
    let f = apply(k, u);
    0.5 * f.iter().zip(u).map(|(a, b)| a * b).sum::<f64>()
}

#[test]
fn an_elements_stiffness_is_symmetric() {
    let k = element_stiffness(&steel(), 0.005);
    let scale = k.iter().flatten().fold(0.0f64, |a, b| a.max(b.abs()));
    assert!(scale > 0.0);
    for (i, row) in k.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            let other = k.get(j).and_then(|r| r.get(i)).copied().unwrap_or(f64::NAN);
            assert!((v - other).abs() <= scale * 1e-12, "K[{i}][{j}] = {v}, K[{j}][{i}] = {other}");
        }
    }
}

#[test]
fn an_element_resists_nothing_about_a_rigid_motion() {
    // Six motions cost no energy: three slides and three turns. An element that resisted any of
    // them would invent a force out of nothing, and in a model with no support the solver would
    // then happily find an answer where there is none.
    let h = 0.005;
    let k = element_stiffness(&steel(), h);
    let scale = k.iter().flatten().fold(0.0f64, |a, b| a.max(b.abs()));
    let motions: [(&str, Vec3); 3] = [("x", Vec3::X), ("y", Vec3::Y), ("z", Vec3::Z)];
    for (name, dir) in motions {
        let u = field(h, |_| dir);
        let f = apply(&k, &u);
        let worst = f.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        assert!(worst <= scale * 1e-12, "sliding along {name} cost a force of {worst}");
    }
    // A turn is a linear field too, and a trilinear element carries a linear field exactly, so
    // this holds to the same precision rather than approximately.
    let turns: [(&str, fn(Vec3) -> Vec3); 3] =
        [("z", |p| Vec3::new(-p.y, p.x, 0.0)), ("x", |p| Vec3::new(0.0, -p.z, p.y)), ("y", |p| Vec3::new(p.z, 0.0, -p.x))];
    for (name, turn) in turns {
        let u = field(h, turn);
        let f = apply(&k, &u);
        let worst = f.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        assert!(worst <= scale * 1e-10, "turning about {name} cost a force of {worst}");
    }
}

#[test]
fn an_elements_strain_energy_is_what_the_integral_says() {
    // Stretch an element uniformly along x by s. The energy should be ½·(λ+2μ)·s²·h³ exactly,
    // since two Gauss points integrate the cubic BᵀDB exactly and the strain is uniform.
    let (h, s) = (0.005, 1e-4);
    let m = steel();
    let k = element_stiffness(&m, h);
    let (lambda, mu) = m.lame();
    let u = field(h, |p| Vec3::new(s * p.x, 0.0, 0.0));
    let want = 0.5 * (lambda + 2.0 * mu) * s * s * h.powi(3);
    let got = energy(&k, &u);
    assert!((got - want).abs() / want < 1e-12, "{got} vs {want}");

    // And a pure shear: ½·μ·γ²·h³ for an engineering shear strain γ.
    let g = 1e-4;
    let u = field(h, |p| Vec3::new(g * p.y, 0.0, 0.0));
    let want = 0.5 * mu * g * g * h.powi(3);
    let got = energy(&k, &u);
    assert!((got - want).abs() / want < 1e-12, "{got} vs {want}");
}

#[test]
fn an_element_never_gives_energy_back() {
    // Positive definiteness, which conjugate gradients needs. Anything but a rigid motion must
    // cost energy.
    let (h, m) = (0.005, steel());
    let k = element_stiffness(&m, h);
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 11) as f64 / (1u64 << 53) as f64) - 0.5
    };
    for _ in 0..40 {
        let mut u = [0.0f64; DOFS];
        for slot in u.iter_mut() {
            *slot = next() * 1e-5;
        }
        assert!(energy(&k, &u) >= 0.0, "an element gave energy back");
    }
}

#[test]
fn the_elasticity_matrix_is_the_isotropic_one() {
    let m = ElasticMaterial::new(200.0e9, 0.3);
    let d = elasticity_matrix(&m);
    let (lambda, mu) = m.lame();
    // Lamé from E and ν, and back: the bulk modulus is λ + 2μ/3 = E/3(1−2ν).
    assert!((lambda - 200.0e9 * 0.3 / (1.3 * 0.4)).abs() / lambda < 1e-12);
    assert!((mu - 200.0e9 / 2.6).abs() / mu < 1e-12);
    let bulk = lambda + 2.0 * mu / 3.0;
    assert!((bulk - 200.0e9 / (3.0 * 0.4)).abs() / bulk < 1e-12, "{bulk}");
    for a in 0..3 {
        assert!((d[a][a] - (lambda + 2.0 * mu)).abs() / d[a][a] < 1e-12);
        assert!((d[a + 3][a + 3] - mu).abs() / mu < 1e-12);
        for b in 0..3 {
            if a != b {
                assert!((d[a][b] - lambda).abs() / lambda < 1e-12);
            }
            assert!(d[a][b + 3].abs() < 1e-12, "no coupling between stretch and shear");
        }
    }
}

#[test]
fn a_bar_pulled_along_its_length_carries_exactly_the_stress_it_is_pulled_with() {
    let occ = bar(20);
    let traction = 100.0e6;
    let m = steel();
    let mut study = Study::new(vec![m]);
    study.loads = rollers();
    study.loads.push(Load::new("pull", side(0, true), Support::Traction([traction, 0.0, 0.0])));
    let s = solve(&occ, &study).expect("solve");

    // A uniform state of strain, which a trilinear element carries exactly: σ_xx is the
    // traction and the other five components are nothing.
    for (e, stress) in s.stress_pa.iter().enumerate() {
        assert!((stress[0] - traction).abs() / traction < 1e-6, "cell {e}: σ_xx = {}", stress[0]);
        for (c, v) in stress.iter().enumerate().skip(1) {
            assert!(v.abs() / traction < 1e-6, "cell {e}: component {c} is {v}, and should be nothing");
        }
    }
    // Von Mises of a uniaxial stress is the stress itself.
    let worst = s.worst_stress.expect("a worst place");
    assert!((worst.value - traction).abs() / traction < 1e-6, "{}", worst.value);
    // And the safety factor is the yield strength over it: 250 MPa against 100 MPa.
    let safety = s.least_safety_factor.expect("a safety factor");
    assert!((safety.value - 2.5).abs() < 1e-5, "{}", safety.value);

    // The bar stretched by t·L/E.
    let nodes = Nodes::of(&occ);
    let cells = occ.grid().count();
    let far = nodes.at([cells[0] - 1, 1, 1]).expect("a node at the far end");
    let moved = s.displacement.get(far * 3).copied().unwrap_or(f64::NAN);
    let want = traction * LENGTH_M / m.young_modulus_pa;
    assert!((moved - want).abs() / want < 1e-6, "the far end moved {moved} m, and should move {want} m");

    // What was pulled with, and what the rollers took, cancel.
    let pull = traction * AREA_M2;
    assert!((s.applied_n[0] - pull).abs() / pull < 1e-9, "{:?}", s.applied_n);
    assert!((s.reaction_n[0] + pull).abs() / pull < 1e-6, "{:?}", s.reaction_n);
    assert!(s.imbalance < 1e-6, "the case does not balance: {}", s.imbalance);
}

#[test]
fn a_bar_pulled_along_its_length_gets_thinner_by_poissons_ratio() {
    let occ = bar(20);
    let traction = 100.0e6;
    let m = steel();
    let mut study = Study::new(vec![m]);
    study.loads = rollers();
    study.loads.push(Load::new("pull", side(0, true), Support::Traction([traction, 0.0, 0.0])));
    let s = solve(&occ, &study).expect("solve");

    let nodes = Nodes::of(&occ);
    let cells = occ.grid().count();
    // A node on the far side across the bar: it should have moved inwards by ν·ε·width.
    let across = nodes.at([1, cells[1] - 1, 1]).expect("a node across the bar");
    let moved = s.displacement.get(across * 3 + 1).copied().unwrap_or(f64::NAN);
    let strain = traction / m.young_modulus_pa;
    let want = -m.poisson_ratio * strain * (SIDE_MM / 1000.0);
    assert!((moved - want).abs() / want.abs() < 1e-5, "it moved {moved} m, and should move {want} m");
}

#[test]
fn a_part_free_to_grow_when_it_is_heated_carries_no_stress_at_all() {
    // The sharpest check on the thermal load there is: the expansion and the stress recovery
    // have to agree exactly, or a part that is free to grow comes out stressed, which is the
    // classic way of getting thermal loading wrong.
    let occ = bar(20);
    let m = steel();
    let rise = 80.0;
    let mut study = Study::new(vec![m]);
    study.loads = rollers();
    study.temperature_rise_k = vec![rise; occ.equations()];
    let s = solve(&occ, &study).expect("solve");

    let reference = m.young_modulus_pa * m.expansion_per_k[0] * rise;
    for (e, stress) in s.stress_pa.iter().enumerate() {
        for (c, v) in stress.iter().enumerate() {
            assert!(v.abs() / reference < 1e-6, "cell {e}, component {c}: {v} Pa, and a free part carries none");
        }
    }
    // And it grew by α·ΔT·L.
    let nodes = Nodes::of(&occ);
    let cells = occ.grid().count();
    let far = nodes.at([cells[0] - 1, 1, 1]).expect("a node at the far end");
    let moved = s.displacement.get(far * 3).copied().unwrap_or(f64::NAN);
    let want = m.expansion_per_k[0] * rise * LENGTH_M;
    assert!((moved - want).abs() / want < 1e-6, "it grew {moved} m, and should grow {want} m");
    // Nothing pushed on it and nothing held it back.
    assert!(s.applied_n.iter().all(|f| f.abs() < 1e-6), "{:?}", s.applied_n);
    // The thermal term is self-equilibrating -- a part warming up pushes on nothing outside
    // itself -- so the supports take almost nothing, against a scale of some tens of kilonewtons.
    assert!(s.reaction_n.iter().all(|f| f.abs() < 1.0), "{:?}", s.reaction_n);
}

#[test]
fn a_part_held_in_one_direction_while_it_is_heated_carries_exactly_e_alpha_delta_t() {
    // The textbook answer for a bar held between two walls and warmed: σ = −E·α·ΔT, whatever
    // Poisson's ratio is, because the sides are free to move. It comes out of the algebra only
    // if the thermal term, the elasticity matrix and the stress recovery all agree.
    let occ = bar(20);
    let m = steel();
    let rise = 80.0;
    let mut study = Study::new(vec![m]);
    study.loads = vec![
        Load::new("held at one end", side(0, false), Support::Roller),
        Load::new("held at the other", side(0, true), Support::Roller),
        Load::new("y roller", side(1, false), Support::Roller),
        Load::new("z roller", side(2, false), Support::Roller),
    ];
    study.temperature_rise_k = vec![rise; occ.equations()];
    let s = solve(&occ, &study).expect("solve");

    let want = -m.young_modulus_pa * m.expansion_per_k[0] * rise;
    for (e, stress) in s.stress_pa.iter().enumerate() {
        assert!((stress[0] - want).abs() / want.abs() < 1e-5, "cell {e}: σ_xx = {} Pa, and should be {want} Pa", stress[0]);
        // Across the bar it is free, so there is nothing there.
        assert!(stress[1].abs() / want.abs() < 1e-5, "cell {e}: σ_yy = {}", stress[1]);
        assert!(stress[2].abs() / want.abs() < 1e-5, "cell {e}: σ_zz = {}", stress[2]);
    }
    // A uniaxial stress, so von Mises is its size -- which here is 192 MPa against a yield of
    // 250, a safety factor of 1.3 from nothing but being warmed and not allowed to grow.
    let worst = s.worst_stress.expect("a worst place");
    assert!((worst.value - want.abs()).abs() / want.abs() < 1e-5, "{}", worst.value);
    let safety = s.least_safety_factor.expect("a safety factor");
    assert!((safety.value - 250.0e6 / want.abs()).abs() < 1e-4, "{}", safety.value);
    assert!(s.imbalance < 1e-6, "{}", s.imbalance);
}

#[test]
fn a_part_held_on_every_side_while_it_is_heated_is_squeezed_but_does_not_yield() {
    // Held in all three directions the strain is nothing, so the stress is −E·α·ΔT/(1−2ν) in
    // each direction at once -- 480 MPa here, nearly twice the steel's yield. And yet the von
    // Mises stress is zero and the part does not yield, because yielding is a matter of shear
    // and there is none: squeezing something equally from every side does not shear it. A
    // solver that reported a safety factor here would be wrong in a way that matters.
    let occ = bar(10);
    let m = steel();
    let rise = 80.0;
    let mut study = Study::new(vec![m]);
    study.loads = (0..3)
        .flat_map(|axis| {
            [
                Load::new(format!("{axis} near"), side(axis, false), Support::Roller),
                Load::new(format!("{axis} far"), side(axis, true), Support::Roller),
            ]
        })
        .collect();
    study.temperature_rise_k = vec![rise; occ.equations()];
    let s = solve(&occ, &study).expect("solve");

    let want = -m.young_modulus_pa * m.expansion_per_k[0] * rise / (1.0 - 2.0 * m.poisson_ratio);
    assert!((want + 480.0e6).abs() < 1.0e6, "the case itself: {want} Pa");
    for (e, stress) in s.stress_pa.iter().enumerate() {
        for (axis, v) in stress.iter().take(3).enumerate() {
            assert!((v - want).abs() / want.abs() < 1e-5, "cell {e}, axis {axis}: {v} Pa");
        }
        for v in stress.iter().skip(3) {
            assert!(v.abs() / want.abs() < 1e-5, "cell {e}: a shear of {v}");
        }
    }
    let worst = s.worst_stress.expect("a worst place");
    assert!(worst.value / want.abs() < 1e-5, "pressure from every side does not shear anything: {}", worst.value);
    // Nothing is anywhere near yielding, whatever the pressure: the factor, if it is reported
    // at all, is enormous.
    assert!(s.least_safety_factor.is_none_or(|f| f.value > 1000.0), "{:?}", s.least_safety_factor);
}

#[test]
fn a_part_squeezed_from_every_side_shrinks_by_its_bulk_modulus() {
    let occ = bar(10);
    let m = ElasticMaterial { yield_strength_pa: None, ..steel() };
    let pressure = 50.0e6;
    let mut study = Study::new(vec![m]);
    study.loads = rollers();
    for axis in 0..3 {
        study.loads.push(Load::new(format!("squeeze {axis}"), side(axis, true), Support::Pressure(pressure)));
    }
    let s = solve(&occ, &study).expect("solve");

    // Hydrostatic: −p in every direction, and no shear anywhere.
    for (e, stress) in s.stress_pa.iter().enumerate() {
        for (axis, v) in stress.iter().take(3).enumerate() {
            assert!((v + pressure).abs() / pressure < 1e-5, "cell {e}, axis {axis}: {v} Pa");
        }
    }
    assert!(s.worst_stress.is_some_and(|w| w.value / pressure < 1e-5), "no shear, so no von Mises");
    // And it shrank: each axis by p(1−2ν)/E, which is a third of the volumetric strain p/K.
    let nodes = Nodes::of(&occ);
    let cells = occ.grid().count();
    let far = nodes.at([cells[0] - 1, 1, 1]).expect("a node");
    let moved = s.displacement.get(far * 3).copied().unwrap_or(f64::NAN);
    let want = -pressure * (1.0 - 2.0 * m.poisson_ratio) / m.young_modulus_pa * LENGTH_M;
    assert!((moved - want).abs() / want.abs() < 1e-5, "it moved {moved} m, and should move {want} m");
    assert!(s.imbalance < 1e-6, "{}", s.imbalance);
}

#[test]
fn a_bar_hung_from_one_end_is_held_up_by_exactly_its_own_weight() {
    let occ = bar(10);
    let m = steel();
    let mut study = Study::new(vec![m]);
    study.loads = vec![Load::new("hung", side(0, false), Support::Fixed)];
    study.gravity_m_s2 = [-9.806_65, 0.0, 0.0];
    let s = solve(&occ, &study).expect("solve");

    // The lattice holds the bar exactly, so its weight is exactly ρ·V·g.
    let weight = m.density_kg_m3 * (LENGTH_M * AREA_M2) * 9.806_65;
    assert!((s.applied_n[0] + weight).abs() / weight < 1e-9, "{:?} against a weight of {weight} N", s.applied_n);
    assert!((s.reaction_n[0] - weight).abs() / weight < 1e-6, "{:?}", s.reaction_n);
    assert!(s.imbalance < 1e-6, "{}", s.imbalance);
    // It is worst where it is held, which is where the whole weight passes through.
    let worst = s.worst_stress.expect("a worst place");
    assert!(worst.at.x < LENGTH_MM / 4.0, "the worst place is at the top: {:?}", worst.at);
    // And nothing hanging from steel comes anywhere near yielding: ρ·g·L is about 7.7 kPa.
    assert!(worst.value < 1.0e5, "{}", worst.value);
}

#[test]
fn a_cantilever_bends_very_nearly_as_far_as_beam_theory_says() {
    // What the element's own bubble modes are for. A beam clamped at one end with a load across
    // the other deflects PL³/3EI plus a shear term. A plain trilinear brick resists the bending
    // with a shear that is not really there and comes out more than a tenth short at this
    // spacing; with the modes condensed in it is short by under two per cent at ten cells along
    // and under one at forty, still approaching from below as it must.
    let m = ElasticMaterial { yield_strength_pa: None, ..steel() };
    let load = 1.0e6;
    let width = SIDE_MM / 1000.0;
    let inertia = width * width.powi(3) / 12.0;
    // Euler–Bernoulli, plus Timoshenko's shear term, which for a stubby beam is not small.
    let (_, mu) = m.lame();
    let bending = load * AREA_M2 * LENGTH_M.powi(3) / (3.0 * m.young_modulus_pa * inertia);
    let shear = load * AREA_M2 * LENGTH_M / (mu * AREA_M2) * 1.2;
    let want = bending + shear;

    let mut errors = Vec::new();
    for span in [10, 20, 40] {
        let occ = bar(span);
        let mut study = Study::new(vec![m]);
        study.loads =
            vec![Load::new("clamped", side(0, false), Support::Fixed), Load::new("pushed", side(0, true), Support::Traction([0.0, 0.0, -load]))];
        let s = solve(&occ, &study).expect("solve");
        let tip = s.most_movement.expect("a furthest node").value;
        assert!(tip > 0.0 && tip < want * 2.0, "{span} cells: {tip} m against {want} m");
        errors.push((want - tip) / want);
    }
    // Still stiff at every spacing -- the deflection is short of the beam answer, never over
    // it, which is what a displacement-based element must do.
    assert!(errors.iter().all(|e| *e > 0.0), "{errors:?}");
    // And close. Two per cent at ten cells along is a different kind of answer from the
    // fourteen per cent the same beam gives without the modes.
    assert!(errors[0] < 0.03, "ten cells along: {errors:?}");
    assert!(errors[2] < 0.01, "forty cells along: {errors:?}");
    // Improving as the elements shrink, by about half each time the spacing halves.
    assert!(errors[1] < errors[0] * 0.7, "{errors:?}");
    assert!(errors[2] < errors[1] * 0.7, "{errors:?}");
}

#[test]
fn a_case_with_nothing_holding_the_part_still_is_refused() {
    let occ = bar(10);
    let study = Study::new(vec![steel()]);
    match solve(&occ, &study) {
        Err(SimError::Underdetermined(m)) => {
            assert!(m.contains("drift"), "{m}");
            assert!(m.contains("roller") || m.contains("fixed"), "it says what to do: {m}");
        }
        other => panic!("{other:?}"),
    }
    // A push with nothing to push against is the same problem.
    let study = Study::new(vec![steel()]).with(Load::new("pull", side(0, true), Support::Traction([1.0e6, 0.0, 0.0])));
    assert!(matches!(solve(&occ, &study), Err(SimError::Underdetermined(_))));
}

#[test]
fn a_material_that_cannot_be_strained_is_refused() {
    let occ = bar(10);
    let held = || Load::new("held", side(0, false), Support::Fixed);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let study = Study::new(vec![ElasticMaterial::new(bad, 0.3)]).with(held());
        match solve(&occ, &study) {
            Err(SimError::Material(m)) => assert!(m.contains("Young"), "{m}"),
            other => panic!("{bad} was accepted: {other:?}"),
        }
    }
    // Poisson's ratio has to leave the material stable, and a half exactly is incompressible.
    for bad in [0.5, 0.6, -1.0, -2.0, f64::NAN] {
        let study = Study::new(vec![ElasticMaterial::new(200.0e9, bad)]).with(held());
        match solve(&occ, &study) {
            Err(SimError::Material(m)) => assert!(m.contains("Poisson"), "{m}"),
            other => panic!("{bad} was accepted: {other:?}"),
        }
    }
    // And no material at all.
    assert!(matches!(solve(&occ, &Study::new(vec![])), Err(SimError::Material(_))));
}

#[test]
fn a_hostile_case_is_refused_rather_than_guessed_at() {
    let occ = bar(10);
    let base = || Study::new(vec![steel()]).with(Load::new("held", side(0, false), Support::Fixed));

    // A temperature field of the wrong length, or one that is not numbers.
    let mut study = base();
    study.temperature_rise_k = vec![10.0; occ.equations() - 1];
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));
    let mut study = base();
    study.temperature_rise_k = vec![f64::NAN; occ.equations()];
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));

    // A gravity, a pressure or a traction that is not a number.
    let mut study = base();
    study.gravity_m_s2 = [0.0, 0.0, f64::NAN];
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));
    let study = base().with(Load::new("odd", side(0, true), Support::Pressure(f64::INFINITY)));
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));
    let study = base().with(Load::new("odd", side(0, true), Support::Traction([f64::NAN, 0.0, 0.0])));
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));

    // A tolerance outside the only range one can have.
    let mut study = base();
    study.limits = Limits { tolerance: 0.0, ..Limits::default() };
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));
    let mut study = base();
    study.limits = Limits { tolerance: 2.0, ..Limits::default() };
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));
}

#[test]
fn the_first_load_to_claim_a_face_keeps_it() {
    let occ = bar(10);
    let study = Study::new(vec![steel()])
        .with(Load::new("first", side(0, false), Support::Fixed))
        .with(Load::new("second", side(0, false), Support::Traction([1.0e9, 0.0, 0.0])))
        .with(Load::new("pull", side(0, true), Support::Traction([10.0e6, 0.0, 0.0])));
    let s = solve(&occ, &study).expect("solve");
    // Only the pull was applied: the second claim on the held face was ignored.
    let pull = 10.0e6 * AREA_M2;
    assert!((s.applied_n[0] - pull).abs() / pull < 1e-9, "{:?}", s.applied_n);
}

#[test]
fn two_materials_share_a_load_by_their_stiffness() {
    // Two halves of a bar, one twice as stiff as the other, pulled along it. In series the
    // stress is the same in both and the strain is not, so the soft half stretches twice as far.
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(LENGTH_MM, SIDE_MM, SIDE_MM) };
    let grid = Grid3::over(b, 20, 1).expect("lattice");
    let soft = block_at([0., 0., 0.], [50., SIDE_MM, SIDE_MM]);
    let stiff = block_at([50., 0., 0.], [50., SIDE_MM, SIDE_MM]);
    let occ = Occupancy::of(grid, &[&soft, &stiff], 1).expect("occupancy");

    let traction = 50.0e6;
    let a = ElasticMaterial { young_modulus_pa: 100.0e9, yield_strength_pa: None, ..steel() };
    let b = ElasticMaterial { young_modulus_pa: 200.0e9, yield_strength_pa: None, ..steel() };
    let mut study = Study::new(vec![a, b]);
    study.loads = rollers();
    study.loads.push(Load::new("pull", side(0, true), Support::Traction([traction, 0.0, 0.0])));
    // Two materials in series want to contract laterally by different amounts, which leaves a
    // real disturbance in the stress near where they meet; the field is exactly uniform only in
    // the limit, so the solve is pushed further than the default to keep the comparison about
    // the physics rather than about the iteration.
    study.limits = Limits { tolerance: 1e-12, ..Limits::default() };
    let s = solve(&occ, &study).expect("solve");

    // Averaged over the bar the stress is the traction exactly, because that is what
    // equilibrium says: whatever the stress does locally, every cross-section has to carry the
    // whole pull.
    let mean = s.stress_pa.iter().map(|x| x[0]).sum::<f64>() / s.stress_pa.len() as f64;
    assert!((mean - traction).abs() / traction < 1e-6, "{mean} Pa against {traction} Pa");
    // Pointwise it is nearly the traction but not exactly, and the reason is physical rather
    // than numerical: the two halves want to get thinner by different amounts under the same
    // stress, and having to stay joined leaves a disturbance around the seam. It dies away over
    // about a bar's width, as Saint-Venant says it must, and it does not shrink when the solve
    // is pushed harder -- which is how one tells it from an error.
    for (e, stress) in s.stress_pa.iter().enumerate() {
        assert!((stress[0] - traction).abs() / traction < 0.2, "cell {e}: {} Pa", stress[0]);
    }
    // That it is the seam and not the solver is the claim worth testing: along the middle of
    // the bar the departure has to grow towards the seam and fall away from it.
    let along: Vec<f64> = (0..occ.grid().count()[0])
        .filter_map(|i| occ.grid().at(i, 2, 2))
        .filter_map(|c| occ.eq_of(c))
        .filter_map(|e| s.stress_pa.get(e))
        .map(|x| (x[0] - traction).abs() / traction)
        .collect();
    assert!(along.len() >= 18, "{}", along.len());
    let far = along.first().copied().unwrap_or(f64::NAN);
    let near = along.get(along.len() / 2 - 1).copied().unwrap_or(f64::NAN);
    assert!(near > far * 2.0, "the seam is where the disturbance is: {near} there against {far} at the end");
    // And the total stretch is the sum of the two halves' own, to within the same disturbance:
    // a quarter of a per cent, which is the seam again rather than the solver.
    let nodes = Nodes::of(&occ);
    let cells = occ.grid().count();
    let far = nodes.at([cells[0] - 1, 1, 1]).expect("a node");
    let moved = s.displacement.get(far * 3).copied().unwrap_or(f64::NAN);
    let want = traction * 0.05 / a.young_modulus_pa + traction * 0.05 / b.young_modulus_pa;
    assert!((moved - want).abs() / want < 5e-3, "it stretched {moved} m, and should stretch {want} m");
}

/// A pressure on a slanted face pushes exactly as hard, and exactly which way, as it pushes on
/// the model's own face -- and does so without any correction at all.
///
/// This is worth a test of its own because it is the one quantity the staircase gets right for
/// free, and a well-meant area correction would break it. The push of a pressure over a patch
/// of surface is the sum of area times outward direction, and that sum depends only on the rim
/// of the patch; the steps have the model's rim, so their sum is the model's. Their areas are
/// √2 too large and their directions are tilted away by the same √2, and the two cancel.
#[test]
fn a_pressure_on_a_slanted_face_pushes_the_way_the_face_does() {
    let (leg, width) = (20.0, 20.0);
    let mesh = crate::test_shapes::wedge(leg, width);
    let refs = [&mesh];
    let pressure = 1.0e6;
    let slant_m2 = leg * 2.0f64.sqrt() * width * 1e-6;
    // The slant's outward normal is (1, 0, 1)/√2, so a pressure pushes in along −(1, 0, 1)/√2
    // and the push is p·A/√2 along each of x and z.
    let want = -pressure * slant_m2 / 2.0f64.sqrt();

    let mut errors = Vec::new();
    for span in [32usize, 64] {
        let grid = Grid3::over(mesh.bounds(), span, 0).expect("a lattice over the wedge");
        let occ = Occupancy::of(grid, &refs, 4).expect("the wedge on the lattice");
        let tags = crate::tag::FaceTags::of(&occ, &refs);

        let mut study = Study::new(vec![ElasticMaterial::new(200e9, 0.3)]);
        study.loads = vec![
            Load::new("held", Patch::Faces(tags.patch(0, 1)), Support::Fixed),
            Load::new("pushed", Patch::Faces(tags.patch(0, 0)), Support::Pressure(pressure)),
            Load::new("free", Patch::Rest, Support::Free),
        ];
        study.area_scale = tags.area_scale(&occ, &refs);

        let answer = solve(&occ, &study).expect("the wedge stands");
        assert!(answer.imbalance < 1e-5, "span {span}: the forces are out by {}", answer.imbalance);

        // What is left is the rim: the sliver at the sharp end of the wedge is thinner than
        // half a cell, so one row of steps is missing and the push is short by one row in
        // `span` of it. That is the staircase's own error and it halves with the spacing --
        // unlike the area of the steps, which does not.
        for (axis, got) in [(0usize, answer.applied_n[0]), (2, answer.applied_n[2])] {
            let error = (got - want).abs() / want.abs();
            assert!(error < 1.5 / span as f64, "span {span}: the push along axis {axis} came out {got} N against {want}");
        }
        // And nothing along y, the slant having no y to it.
        assert!(answer.applied_n[1].abs() < pressure * slant_m2 * 1e-6, "span {span}: it pushed {} N along y", answer.applied_n[1]);
        errors.push((answer.applied_n[0] - want).abs() / want.abs());
    }
    let (Some(coarse), Some(fine)) = (errors.first(), errors.last()) else {
        panic!("two spans were measured");
    };
    assert!(fine < &(coarse * 0.7), "the push was out by {coarse} coarse and {fine} fine, so it is not converging");
}

/// A traction on a slanted face does need the correction: it is a force per unit area in a
/// direction of its own, so there is no tilt to cancel the steps being too large.
#[test]
fn a_traction_on_a_slanted_face_pulls_over_the_area_the_face_has() {
    let (leg, width) = (20.0, 20.0);
    let mesh = crate::test_shapes::wedge(leg, width);
    let refs = [&mesh];
    let grid = Grid3::over(mesh.bounds(), 32, 0).expect("a lattice over the wedge");
    let occ = Occupancy::of(grid, &refs, 4).expect("the wedge on the lattice");
    let tags = crate::tag::FaceTags::of(&occ, &refs);

    let pull = 1.0e6;
    let slant_m2 = leg * 2.0f64.sqrt() * width * 1e-6;
    let mut study = Study::new(vec![ElasticMaterial::new(200e9, 0.3)]);
    study.loads = vec![
        Load::new("held", Patch::Faces(tags.patch(0, 2)), Support::Fixed),
        Load::new("pulled", Patch::Faces(tags.patch(0, 0)), Support::Traction([0.0, 0.0, pull])),
        Load::new("free", Patch::Rest, Support::Free),
    ];
    study.area_scale = tags.area_scale(&occ, &refs);

    let answer = solve(&occ, &study).expect("the wedge stands");
    let want = pull * slant_m2;
    let error = (answer.applied_n[2] - want).abs() / want;
    assert!(error < 0.01, "the pull came out {} N against {want}", answer.applied_n[2]);

    // Uncorrected it would be √2 times too much, which is what the correction is there for.
    let mut staircase = study.clone();
    staircase.area_scale = Vec::new();
    let worse = solve(&occ, &staircase).expect("the staircase stands too");
    let over = worse.applied_n[2] / want;
    assert!(over > 1.3, "the staircase pulled only {over} times as hard, so this test proves nothing");
}

/// A part warmed all over with nothing holding it back ends up unstressed, whatever shape it
/// is and however the lattice cuts it.
///
/// A box square to the axes gets this exactly right and a test already says so. A cylinder is
/// the harder case: its cells carry a share of material rather than a whole one, and the test
/// is that the share scaling the stiffness and the share scaling the expansion are the same
/// share. If they were not, a round part would come out stressed by simply sitting in the sun.
#[test]
fn a_round_part_warmed_all_over_is_left_unstressed() {
    let mesh = crate::test_shapes::cylinder(10.0, 0.0, 20.0, 192);
    let refs = [&mesh];
    let grid = Grid3::over(mesh.bounds(), 24, 0).expect("a lattice over the cylinder");
    let occ = Occupancy::of(grid, &refs, 4).expect("the cylinder on the lattice");
    let tags = crate::tag::FaceTags::of(&occ, &refs);

    let modulus = 70e9;
    let material =
        ElasticMaterial { young_modulus_pa: modulus, poisson_ratio: 0.33, expansion_per_k: [23e-6; 3], density_kg_m3: 0.0, yield_strength_pa: None };
    let mut study = Study::new(vec![material]);
    // Held only where it must be to stop it drifting: one face on rollers in each direction.
    study.loads = vec![Load::new("stood on", Patch::Faces(tags.patch(0, 1)), Support::Roller), Load::new("free", Patch::Rest, Support::Free)];
    study.temperature_rise_k = vec![80.0; occ.equations()];
    study.area_scale = tags.area_scale(&occ, &refs);

    let answer = solve(&occ, &study).expect("the cylinder grows");
    // Rollers on one cap leave it free to slide and spin, which a lattice of a round part does
    // not pin down, so the case is checked on the stress rather than on where it went.
    let worst = answer.von_mises_pa.iter().copied().fold(0.0f64, f64::max);
    // Against a stress of E·α·ΔT = 129 MPa if the expansion were held back at all.
    let held = modulus * 23e-6 * 80.0;
    assert!(worst < held * 1e-6, "a freely warmed cylinder came out stressed to {worst} Pa of {held}");
}

// ---------------------------------------------------------------------------------------------
// A hexahedron that is not a cube.
//
// The lattice holds a curved wall as a staircase, and a staircase's inside corners are where the
// stress it reports goes wrong -- they are re-entrant corners, with no finite stress, so the peak
// climbs without limit as the lattice is refined. The way out is to stop having them: pull the
// cells on the wall onto the model's own surface, which makes them hexahedra rather than cubes.
// These three check the element that has to hold them, before anything is pulled anywhere.

/// The eight corners of a cube of side `h` with its middle at the origin, in [`CORNERS`] order.
fn cube_corners(h: f64) -> [Vec3; 8] {
    let mut out = [Vec3::new(0.0, 0.0, 0.0); 8];
    for (i, c) in CORNERS.iter().enumerate() {
        out[i] = Vec3::new(c[0], c[1], c[2]) * (h / 2.0);
    }
    out
}

/// On a cube the general element is the cube element, to the last bit that floating point allows.
///
/// This is the one that keeps the rest of the solver safe: every answer the suite already holds
/// to equality was worked out by [`element_stiffness`], and a general element that disagreed with
/// it on a cube would quietly move all of them.
#[test]
fn a_hexahedron_that_happens_to_be_a_cube_is_the_cube_element() {
    let m = steel();
    for h in [0.001, 0.005, 0.02] {
        let cube = element_stiffness(&m, h);
        let hex = element_stiffness_hex(&m, &cube_corners(h)).expect("a cube is a fine hexahedron");
        let scale = cube.iter().flatten().fold(0.0f64, |a, v| a.max(v.abs()));
        for (r, (a, b)) in cube.iter().zip(&hex).enumerate() {
            for (c, (x, y)) in a.iter().zip(b).enumerate() {
                assert!((x - y).abs() <= scale * 1e-12, "h {h}, entry ({r}, {c}): cube {x}, hexahedron {y}");
            }
        }
    }
}

/// A distorted element still reads a uniform strain as exactly that strain.
///
/// This is the patch test at the level of the one thing that can get it wrong. Displace every
/// corner by `u = E·x` for a constant strain `E` and the element must read `E` back, at every
/// Gauss point, however the corners are pushed about -- that is what the Jacobian is for, and
/// getting it wrong is how a distorted element invents strain out of a field that has none.
#[test]
fn a_distorted_element_reads_a_uniform_strain_exactly() {
    // A cube with its corners pushed about, by a good fraction of its own size.
    let mut corners = cube_corners(0.01);
    for (i, push) in [
        (0usize, Vec3::new(0.0012, -0.0009, 0.0006)),
        (2, Vec3::new(-0.0015, 0.0011, -0.0004)),
        (5, Vec3::new(0.0008, 0.0013, 0.0010)),
        (7, Vec3::new(-0.0006, -0.0007, 0.0014)),
    ] {
        corners[i] += push;
    }
    let centre = jacobian([0.0; 3], &corners).expect("the element is not inside out");

    // A strain with something in every component, and the displacement field that has it.
    let e = [[3.0e-4, 1.1e-4, -0.7e-4], [1.1e-4, -2.0e-4, 0.5e-4], [-0.7e-4, 0.5e-4, 1.4e-4]];
    let want = [e[0][0], e[1][1], e[2][2], 2.0 * e[0][1], 2.0 * e[1][2], 2.0 * e[2][0]];
    let mut u = [0.0f64; DOFS];
    for (i, x) in corners.iter().enumerate() {
        let p = [x.x, x.y, x.z];
        for a in 0..3 {
            u[i * 3 + a] = e[a][0] * p[0] + e[a][1] * p[1] + e[a][2] * p[2];
        }
    }

    for x in GAUSS {
        for y in GAUSS {
            for z in GAUSS {
                let (b, det) = strain_matrix_hex([x, y, z], &corners, &centre).expect("a point inside it");
                assert!(det > 0.0, "the element is turned inside out at ({x}, {y}, {z})");
                for (r, want) in want.iter().enumerate() {
                    let got: f64 = (0..DOFS).map(|c| b[r][c] * u[c]).sum();
                    assert!((got - want).abs() < 1e-14, "at ({x}, {y}, {z}) strain {r}: read {got}, which is not the {want} it was given");
                }
            }
        }
    }
}

/// A distorted element carries no force when it is only moved or turned.
///
/// Six rigid motions, three of them translations and three small rotations, and the stiffness
/// must answer each with nothing. A translation is exact arithmetic and is held to it; a
/// rotation is a linearisation of one, so it is held to the size of the element's own stiffness
/// rather than to zero.
#[test]
fn a_distorted_element_resists_no_rigid_motion() {
    let mut corners = cube_corners(0.01);
    corners[1] += Vec3::new(0.0018, 0.0007, -0.0011);
    corners[6] += Vec3::new(-0.0013, 0.0016, 0.0009);
    let k = element_stiffness_hex(&steel(), &corners).expect("the element is not inside out");
    let scale = k.iter().flatten().fold(0.0f64, |a, v| a.max(v.abs()));

    let modes: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for mode in modes {
        let mut u = [0.0f64; DOFS];
        for i in 0..8 {
            for a in 0..3 {
                u[i * 3 + a] = mode[a];
            }
        }
        for (row, krow) in k.iter().enumerate() {
            let f: f64 = krow.iter().zip(&u).map(|(a, b)| a * b).sum();
            assert!(f.abs() <= scale * 1e-12, "a translation along {mode:?} puts {f} N on freedom {row}");
        }
    }

    // Turning it about each axis in turn, by a small angle, as a cross product.
    for axis in 0..3 {
        let mut w = [0.0f64; 3];
        w[axis] = 1.0e-6;
        let mut u = [0.0f64; DOFS];
        for (i, x) in corners.iter().enumerate() {
            let p = [x.x, x.y, x.z];
            u[i * 3] = w[1] * p[2] - w[2] * p[1];
            u[i * 3 + 1] = w[2] * p[0] - w[0] * p[2];
            u[i * 3 + 2] = w[0] * p[1] - w[1] * p[0];
        }
        for (row, krow) in k.iter().enumerate() {
            let f: f64 = krow.iter().zip(&u).map(|(a, b)| a * b).sum();
            assert!(f.abs() <= scale * 1e-12, "turning about axis {axis} puts {f} N on freedom {row}");
        }
    }
}

/// An element turned inside out is refused rather than solved.
#[test]
fn a_hexahedron_folded_through_itself_is_refused() {
    let mut corners = cube_corners(0.01);
    // Swap a pair across the element, which folds it.
    corners.swap(0, 6);
    assert!(element_stiffness_hex(&steel(), &corners).is_none(), "a folded hexahedron has no stiffness worth having");
    // And one squashed flat has none either.
    let flat: [Vec3; 8] = std::array::from_fn(|i| Vec3::new(cube_corners(0.01)[i].x, cube_corners(0.01)[i].y, 0.0));
    assert!(element_stiffness_hex(&steel(), &flat).is_none(), "a flattened hexahedron has no volume to integrate over");
}

/// The whole point of the two pieces before it: a round part pulled along its axis, solved on
/// the lattice and then on elements that follow its surface.
///
/// The answer is a uniform stress -- the traction, everywhere -- and the lattice cannot give it.
/// Its barrel is a staircase, and the stress it reports at the worst cell climbs as the lattice
/// is refined rather than settling, because a step's inside corner is re-entrant. Pulling the
/// boundary nodes onto the barrel takes the steps away, and with them the thing that was being
/// read.
///
/// What is asserted is the comparison, not a number: a figure pinned here would be pinning this
/// lattice rather than the physics.
#[test]
fn a_round_part_pulled_along_its_axis_is_nearer_the_truth_on_fitted_elements() {
    let radius = 10.0;
    let (z0, z1) = (0.0, 40.0);
    let mesh = crate::test_shapes::cylinder(radius, z0, z1, 256);
    let traction = 10.0e6;

    let mut off = f64::NAN;
    let mut on = f64::NAN;
    for fit in [false, true] {
        let grid = Grid3::over(mesh.bounds(), 40, 0).expect("a lattice over the rod");
        let occ = Occupancy::of(grid, &[&mesh], 4).expect("the rod on the lattice");
        let nodes = Nodes::of(&occ);
        let mut study = Study::new(vec![ElasticMaterial { yield_strength_pa: None, ..steel() }]);
        study.loads =
            vec![Load::new("held", side(2, false), Support::Fixed), Load::new("pulled", side(2, true), Support::Traction([0.0, 0.0, traction]))];
        if fit {
            // The flat ends carry the hold and the pull, and a load is still spread over the
            // area the lattice's face had -- so those nodes stay where that area was worked out
            // for. They are on the cap plane already and have nothing to gain by moving.
            let across = occ.grid().count();
            let mut pinned = vec![false; nodes.count()];
            for (eq, slot) in pinned.iter_mut().enumerate() {
                *slot = matches!(nodes.position(eq), Some([_, _, k]) if k == 0 || k == across[2]);
            }
            let snapped = crate::snap::snap_keeping(&occ, &nodes, &[&mesh], &pinned);
            assert!(!snapped.is_lattice(), "the barrel should have nodes to pull");
            study.fitted = Some(snapped);
        }
        let s = solve(&occ, &study).expect("solve");
        let worst = s.worst_stress.expect("a worst place").value;
        if fit { on = worst } else { off = worst }
    }

    let (over_off, over_on) = ((off - traction) / traction, (on - traction) / traction);
    println!(
        "  lattice {:.3} MPa (+{:.1}%), fitted {:.3} MPa (+{:.1}%), exact {:.1} MPa",
        off / 1e6,
        over_off * 100.0,
        on / 1e6,
        over_on * 100.0,
        traction / 1e6
    );
    assert!(over_off > 0.1, "the lattice should over-read a uniform stress: {over_off:.3} over");
    assert!(
        over_on < over_off * 0.75,
        "fitting the surface should take a good part of the over-read off: {:.1}% on the lattice, {:.1}% fitted",
        over_off * 100.0,
        over_on * 100.0
    );
    assert!(on > traction * 0.9, "and it must not start under-reading: {on} Pa against {traction} Pa");
}

/// A study that follows the surface is refused rather than mixed with one that cannot.
///
/// The weight of a cell and the push of its warmth are still worked out as if every cell were a
/// box of side `h`. For a fitted cell they are not, and an element whose stiffness and whose
/// load come from two different shapes invents stress out of nothing -- which is exactly how the
/// cut-cell quadrature went wrong. Refusing is what keeps that from happening again quietly.
#[test]
fn surface_fitting_is_refused_where_the_rest_of_the_study_would_not_match_it() {
    let mesh = crate::test_shapes::cylinder(8.0, 0.0, 20.0, 128);
    let grid = Grid3::over(mesh.bounds(), 20, 0).expect("a lattice");
    let occ = Occupancy::of(grid, &[&mesh], 4).expect("the rod on the lattice");
    let nodes = Nodes::of(&occ);
    let snapped = crate::snap::snap(&occ, &nodes, &[&mesh]);

    let base = || {
        let mut study = Study::new(vec![ElasticMaterial { yield_strength_pa: None, ..steel() }]);
        study.loads = vec![Load::new("held", side(2, false), Support::Fixed)];
        study.fitted = Some(snapped.clone());
        study
    };

    let mut heavy = base();
    heavy.gravity_m_s2 = [0.0, 0.0, -9.80665];
    let why = solve(&occ, &heavy).expect_err("gravity and fitting together");
    assert!(format!("{why}").contains("not supported yet"), "{why}");

    let mut warm = base();
    warm.temperature_rise_k = vec![50.0; occ.equations()];
    let why = solve(&occ, &warm).expect_err("warmth and fitting together");
    assert!(format!("{why}").contains("not supported yet"), "{why}");

    // A traction on the curved wall itself, whose area the lattice still owns.
    let mut pushed = base();
    pushed.loads.push(Load::new("squeezed", Patch::Rest, Support::Pressure(1.0e6)));
    let why = solve(&occ, &pushed).expect_err("a pressure on a fitted face");
    assert!(format!("{why}").contains("not supported yet"), "{why}");

    // And with none of those, it runs.
    solve(&occ, &base()).expect("fitting on its own is fine");
}

// ---------------------------------------------------------------------------------------------
// The tetrahedron, which is the element a cut cell will be filled with.
//
// A brick can be pulled onto the surface but it cannot *end* at it: a cell the wall crosses keeps
// eight corners and stands a sliver past the model however its nodes are moved. A tetrahedron
// can have a face on the surface. These check the element before anything is cut with it.

/// A regular-ish tetrahedron with a positive volume.
fn tet(scale: f64) -> [Vec3; 4] {
    [Vec3::new(0.0, 0.0, 0.0) * scale, Vec3::new(1.0, 0.0, 0.0) * scale, Vec3::new(0.0, 1.0, 0.0) * scale, Vec3::new(0.0, 0.0, 1.0) * scale]
}

/// A tetrahedron reads a uniform strain as exactly that strain, whatever shape it is.
///
/// Its displacement is linear, so its strain is constant and there is no quadrature to get
/// wrong -- but the gradients still come from inverting its own map, and that is what this
/// checks. Three shapes: a tidy one, a stretched one and a thin sliver like the ones a cut cell
/// will actually produce.
#[test]
fn a_tetrahedron_reads_a_uniform_strain_exactly() {
    let shapes: [[Vec3; 4]; 3] = [
        tet(0.01),
        [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.03, 0.002, 0.0), Vec3::new(0.001, 0.012, 0.0), Vec3::new(0.004, 0.003, 0.02)],
        // A sliver: wide in two directions and almost flat in the third.
        [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.02, 0.0, 0.0), Vec3::new(0.0, 0.02, 0.0), Vec3::new(0.006, 0.005, 0.0004)],
    ];
    let e = [[3.0e-4, 1.1e-4, -0.7e-4], [1.1e-4, -2.0e-4, 0.5e-4], [-0.7e-4, 0.5e-4, 1.4e-4]];
    let want = [e[0][0], e[1][1], e[2][2], 2.0 * e[0][1], 2.0 * e[1][2], 2.0 * e[2][0]];

    for (which, corners) in shapes.iter().enumerate() {
        let (b, volume) = tet_strain_matrix_of(corners).expect("a tetrahedron with volume");
        assert!(volume > 0.0, "shape {which} has no volume");
        let mut u = [0.0f64; TET_DOFS];
        for (i, x) in corners.iter().enumerate() {
            let p = [x.x, x.y, x.z];
            for a in 0..3 {
                u[i * 3 + a] = e[a][0] * p[0] + e[a][1] * p[1] + e[a][2] * p[2];
            }
        }
        for (r, want) in want.iter().enumerate() {
            let got: f64 = (0..TET_DOFS).map(|c| b[r][c] * u[c]).sum();
            assert!((got - want).abs() < 1e-14, "shape {which}, strain {r}: read {got}, given {want}");
        }
    }
}

/// A tetrahedron carries no force when it is only moved or turned, and its stiffness is
/// symmetric.
#[test]
fn a_tetrahedron_resists_no_rigid_motion_and_is_symmetric() {
    let corners = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.012, 0.001, 0.0), Vec3::new(0.002, 0.014, 0.0), Vec3::new(0.003, 0.004, 0.011)];
    let k = tet_stiffness(&steel(), &corners).expect("a tetrahedron with volume");
    let scale = k.iter().flatten().fold(0.0f64, |a, v| a.max(v.abs()));

    for (i, row) in k.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            assert!((v - k[j][i]).abs() <= scale * 1e-12, "stiffness is not symmetric at ({i}, {j})");
        }
    }

    for mode in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        let mut u = [0.0f64; TET_DOFS];
        for i in 0..4 {
            for a in 0..3 {
                u[i * 3 + a] = mode[a];
            }
        }
        for (row, krow) in k.iter().enumerate() {
            let f: f64 = krow.iter().zip(&u).map(|(a, b)| a * b).sum();
            assert!(f.abs() <= scale * 1e-12, "a translation along {mode:?} puts {f} N on freedom {row}");
        }
    }
    for axis in 0..3 {
        let mut w = [0.0f64; 3];
        w[axis] = 1.0e-6;
        let mut u = [0.0f64; TET_DOFS];
        for (i, x) in corners.iter().enumerate() {
            let p = [x.x, x.y, x.z];
            u[i * 3] = w[1] * p[2] - w[2] * p[1];
            u[i * 3 + 1] = w[2] * p[0] - w[0] * p[2];
            u[i * 3 + 2] = w[0] * p[1] - w[1] * p[0];
        }
        for (row, krow) in k.iter().enumerate() {
            let f: f64 = krow.iter().zip(&u).map(|(a, b)| a * b).sum();
            assert!(f.abs() <= scale * 1e-12, "turning about axis {axis} puts {f} N on freedom {row}");
        }
    }
}

/// Six tetrahedra filling a cube hold the same strain energy as the brick, and as the closed
/// form.
///
/// This is the one that ties the new element to the verified one, and the quantity has to be
/// chosen with care. The *force on each node* is not the same for the two and should not be: a
/// brick's faces are bilinear and hand a uniform load a quarter to each corner, where two
/// triangles hand it a third to the corners on their shared diagonal and a sixth to the others.
/// Asserting that was this test's first mistake, and it came out as a clean 4/3.
///
/// Strain energy does not care. Under a uniform strain it is `½ εᵀDε · V` whatever shape
/// functions carry it, so the brick, the six tetrahedra and the closed form must all agree. If
/// the tetrahedron's volume, its gradients or its winding were wrong, this is where it shows.
#[test]
fn six_tetrahedra_filling_a_cube_hold_the_energy_the_brick_does() {
    let h = 0.01;
    let m = ElasticMaterial { yield_strength_pa: None, ..steel() };
    let corners = cube_corners(h);

    // A uniform strain, and the displacement of every corner under it.
    let e = [[2.0e-4, 0.6e-4, -0.3e-4], [0.6e-4, -1.1e-4, 0.4e-4], [-0.3e-4, 0.4e-4, 0.9e-4]];
    let u_at = |x: Vec3| {
        let p = [x.x, x.y, x.z];
        std::array::from_fn::<f64, 3, _>(|a| e[a][0] * p[0] + e[a][1] * p[1] + e[a][2] * p[2])
    };
    let mut u = [0.0f64; DOFS];
    for (i, x) in corners.iter().enumerate() {
        let d = u_at(*x);
        for a in 0..3 {
            u[i * 3 + a] = d[a];
        }
    }

    // The brick's answer: energy = half u'Ku.
    let hex = element_stiffness_hex(&m, &corners).expect("a cube is a fine hexahedron");
    let brick: f64 = 0.5 * u.iter().enumerate().map(|(i, ui)| ui * hex[i].iter().zip(&u).map(|(a, b)| a * b).sum::<f64>()).sum::<f64>();

    // Kuhn's six, all sharing the 0-6 diagonal. CORNERS orders the -z face then the +z face, so
    // these are the six wedges round that diagonal.
    const KUHN: [[usize; 4]; 6] = [[0, 1, 2, 6], [0, 2, 3, 6], [0, 3, 7, 6], [0, 7, 4, 6], [0, 4, 5, 6], [0, 5, 1, 6]];
    let mut tets = 0.0f64;
    let mut total = 0.0;
    for mut which in KUHN {
        let mut at: [Vec3; 4] = std::array::from_fn(|i| corners[which[i]]);
        // Either winding describes the same tetrahedron; the element wants the one with a
        // positive volume, so swap a pair where it is not.
        if tet_strain_matrix_of(&at).is_none() {
            which.swap(1, 2);
            at = std::array::from_fn(|i| corners[which[i]]);
        }
        let (_, volume) = tet_strain_matrix_of(&at).expect("one winding or the other has volume");
        total += volume;
        let k = tet_stiffness(&m, &at).expect("a tetrahedron with volume");
        let mut local = [0.0f64; TET_DOFS];
        for (i, node) in which.iter().enumerate() {
            for a in 0..3 {
                local[i * 3 + a] = u[node * 3 + a];
            }
        }
        tets += 0.5 * local.iter().enumerate().map(|(i, ui)| ui * k[i].iter().zip(&local).map(|(a, b)| a * b).sum::<f64>()).sum::<f64>();
    }

    // The six fill the cube and nothing else.
    assert!((total - h * h * h).abs() < 1e-18, "the six hold {total} m3 of a {} m3 cube", h * h * h);

    // And the closed form: half the strain against the stress it causes, over the volume.
    let voigt = [e[0][0], e[1][1], e[2][2], 2.0 * e[0][1], 2.0 * e[1][2], 2.0 * e[2][0]];
    let d = elasticity_matrix(&m);
    let exact = 0.5 * voigt.iter().enumerate().map(|(i, ei)| ei * d[i].iter().zip(&voigt).map(|(a, b)| a * b).sum::<f64>()).sum::<f64>() * h * h * h;

    assert!(exact > 0.0, "the case stores no energy, so this proves nothing");
    assert!((brick - exact).abs() / exact < 1e-12, "the brick holds {brick} J against {exact} J");
    assert!((tets - exact).abs() / exact < 1e-12, "the six tetrahedra hold {tets} J against {exact} J");
}

/// A tetrahedron with no volume, or wound the other way, is refused.
#[test]
fn a_flat_or_inside_out_tetrahedron_is_refused() {
    let good = tet(0.01);
    assert!(tet_stiffness(&steel(), &good).is_some());
    // Wound the other way: the same four points, two swapped.
    let mut back = good;
    back.swap(1, 2);
    assert!(tet_stiffness(&steel(), &back).is_none(), "a tetrahedron wound the other way has a negative volume");
    // Flat: all four in a plane.
    let flat = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.01, 0.0, 0.0), Vec3::new(0.0, 0.01, 0.0), Vec3::new(0.01, 0.01, 0.0)];
    assert!(tet_stiffness(&steel(), &flat).is_none(), "a flat tetrahedron has no volume to integrate over");
}
