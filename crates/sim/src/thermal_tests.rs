//! Conduction checked against answers worked out on paper.
//!
//! Every case here is a shape whose exact solution is known in closed form and whose walls
//! land on cell boundaries, so there is no geometric error to hide behind: what the test
//! measures is whether the physics is right.
//!
//! Three of the answers are *exact*, not approximate, and the tests hold them to it:
//!
//! * a slab held at both ends runs linear, and a cell-centred finite volume scheme reproduces
//!   a linear field exactly;
//! * two layers divide the drop by their resistances, because the face conductivity is the
//!   harmonic mean, which *is* the series resistance;
//! * the total heat crossing a wall is exact whatever the spacing, because the scheme
//!   conserves energy over every cell by construction.
//!
//! The one case with a curved answer -- a slab generating heat, whose solution is a parabola --
//! is out by `q·h²/(8k)`, uniformly, and that is checked as an equality rather than a bound.

use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;

use crate::SimError;
use crate::grid::{Grid3, Occupancy};
use crate::test_shapes::block_at;
use crate::thermal::{Condition, Film, Patch, STEFAN_BOLTZMANN, SolidMaterial, Solution, Study, Time, radiative_coefficient, solve};

/// The slab every one-dimensional case is run on: 100 by 20 by 20 mm.
const LENGTH_MM: f64 = 100.0;
const SIDE_MM: f64 = 20.0;

/// Its metric measurements.
const LENGTH_M: f64 = LENGTH_MM / 1000.0;
const AREA_M2: f64 = (SIDE_MM / 1000.0) * (SIDE_MM / 1000.0);

/// The slab, as `span` cells along its length.
fn slab(span: usize) -> Occupancy {
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(LENGTH_MM, SIDE_MM, SIDE_MM) };
    let grid = Grid3::over(b, span, 1).expect("lattice");
    let mesh = block_at([0., 0., 0.], [LENGTH_MM, SIDE_MM, SIDE_MM]);
    Occupancy::of(grid, &[&mesh], 1).expect("occupancy")
}

fn side(axis: usize, positive: bool) -> Patch {
    Patch::Side { axis, positive }
}

/// How far along the slab a cell's centre is, in metres from the x = 0 wall.
fn x_of(occ: &Occupancy, eq: usize) -> f64 {
    let cell = occ.cell_of(eq).expect("cell");
    let centre = occ.grid().centre(cell).expect("centre");
    centre.x / 1000.0
}

fn watts(s: &Solution, name: &str) -> f64 {
    s.patches.iter().find(|p| p.name == name).map(|p| p.watts).unwrap_or_else(|| panic!("no patch {name}: {:?}", s.patches))
}

fn wall_max(s: &Solution, name: &str) -> f64 {
    s.patches.iter().find(|p| p.name == name).map(|p| p.max_kelvin).unwrap_or(f64::NAN)
}

#[test]
fn a_slab_held_at_both_ends_runs_linear_between_them() {
    let occ = slab(20);
    let (cold, hot) = (300.0, 400.0);
    let study = Study::new(vec![SolidMaterial::isotropic(50.0)])
        .with(Condition::new("cold", side(0, false), vec![Film::Fixed { kelvin: cold }]))
        .with(Condition::new("hot", side(0, true), vec![Film::Fixed { kelvin: hot }]));
    let s = solve(&occ, &study).expect("solve");

    // A cell-centred finite volume scheme carries a linear field exactly, and the half-cell
    // treatment of a held wall is exact for one too, so this is an equality, not a fit.
    for (eq, got) in s.kelvin.iter().enumerate() {
        let want = cold + (hot - cold) * x_of(&occ, eq) / LENGTH_M;
        assert!((got - want).abs() < 1e-9, "at x = {:.4} m: {got} K, and the answer is {want} K", x_of(&occ, eq));
    }
    assert!((s.min_kelvin - 302.5).abs() < 1e-9, "{}", s.min_kelvin);
    assert!((s.max_kelvin - 397.5).abs() < 1e-9, "{}", s.max_kelvin);
    assert!(s.outer == 1, "nothing radiates, so there is nothing to iterate: {}", s.outer);
}

#[test]
fn the_heat_crossing_a_held_slab_is_what_fourier_says() {
    let occ = slab(20);
    let (cold, hot, k) = (300.0, 400.0, 50.0);
    let study = Study::new(vec![SolidMaterial::isotropic(k)])
        .with(Condition::new("cold", side(0, false), vec![Film::Fixed { kelvin: cold }]))
        .with(Condition::new("hot", side(0, true), vec![Film::Fixed { kelvin: hot }]));
    let s = solve(&occ, &study).expect("solve");

    // Q = k·A·ΔT/L: 50 W/(m·K) over 4 cm², a hundred kelvin across ten centimetres.
    let truth = k * AREA_M2 * (hot - cold) / LENGTH_M;
    assert!((truth - 20.0).abs() < 1e-12, "the case itself: {truth}");
    assert!((watts(&s, "hot") - truth).abs() < 1e-9, "{} vs {truth}", watts(&s, "hot"));
    assert!((watts(&s, "cold") + truth).abs() < 1e-9, "{} vs {}", watts(&s, "cold"), -truth);
    // Nothing is generated, so what comes in has to go out.
    assert!(s.imbalance < 1e-9, "the energy does not balance: {}", s.imbalance);
    assert!((s.generated_w).abs() < 1e-15);
    // The walls report their own temperature, which with no film on them is the held one.
    assert!((wall_max(&s, "hot") - hot).abs() < 1e-9, "{}", wall_max(&s, "hot"));
}

#[test]
fn the_answer_does_not_move_as_the_lattice_is_refined() {
    // The exact answer is linear, so every spacing must give it. A scheme that was only
    // second order here would drift.
    for span in [5, 10, 20, 40] {
        let occ = slab(span);
        let study = Study::new(vec![SolidMaterial::isotropic(50.0)])
            .with(Condition::new("cold", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]))
            .with(Condition::new("hot", side(0, true), vec![Film::Fixed { kelvin: 400.0 }]));
        let s = solve(&occ, &study).expect("solve");
        let worst = s.kelvin.iter().enumerate().map(|(eq, t)| (t - (300.0 + 100.0 * x_of(&occ, eq) / LENGTH_M)).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-9, "{span} cells across was out by {worst} K");
        assert!((watts(&s, "hot") - 20.0).abs() < 1e-9, "{span} cells across: {} W", watts(&s, "hot"));
    }
}

#[test]
fn two_layers_divide_the_drop_by_their_resistances() {
    // Fifty millimetres of k = 50 against fifty of k = 10. This is the test the arithmetic
    // mean fails: averaging the conductivities at the interface would put a sixth too much
    // heat through.
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(LENGTH_MM, SIDE_MM, SIDE_MM) };
    let grid = Grid3::over(b, 20, 1).expect("lattice");
    let left = block_at([0., 0., 0.], [50., SIDE_MM, SIDE_MM]);
    let right = block_at([50., 0., 0.], [50., SIDE_MM, SIDE_MM]);
    let occ = Occupancy::of(grid, &[&left, &right], 1).expect("occupancy");

    let (k1, k2, cold, hot) = (50.0, 10.0, 300.0, 400.0);
    let study = Study::new(vec![SolidMaterial::isotropic(k1), SolidMaterial::isotropic(k2)])
        .with(Condition::new("cold", side(0, false), vec![Film::Fixed { kelvin: cold }]))
        .with(Condition::new("hot", side(0, true), vec![Film::Fixed { kelvin: hot }]));
    let s = solve(&occ, &study).expect("solve");

    let (r1, r2) = (0.05 / (k1 * AREA_M2), 0.05 / (k2 * AREA_M2));
    let truth = (hot - cold) / (r1 + r2);
    assert!((watts(&s, "hot") - truth).abs() < 1e-9, "{} vs {truth}", watts(&s, "hot"));
    // And the temperature where they meet: the cold end plus the drop over the first layer.
    let interface = cold + truth * r1;
    // The two cells either side of x = 50 mm straddle it by half a cell each, and the drop
    // across those two half-cells is known, so the interface temperature can be read off.
    let (near, far) = (
        s.kelvin.iter().enumerate().filter(|(e, _)| x_of(&occ, *e) < 0.05).map(|(_, t)| *t).fold(f64::MIN, f64::max),
        s.kelvin.iter().enumerate().filter(|(e, _)| x_of(&occ, *e) > 0.05).map(|(_, t)| *t).fold(f64::MAX, f64::min),
    );
    let from_left = near + truth * (0.0025 / (k1 * AREA_M2));
    let from_right = far - truth * (0.0025 / (k2 * AREA_M2));
    assert!((from_left - interface).abs() < 1e-9, "from the left: {from_left} vs {interface}");
    assert!((from_right - interface).abs() < 1e-9, "from the right: {from_right} vs {interface}");
}

#[test]
fn a_film_adds_its_own_resistance_in_series() {
    let occ = slab(20);
    let (k, h, held, fluid) = (50.0, 25.0, 300.0, 500.0);
    let study = Study::new(vec![SolidMaterial::isotropic(k)])
        .with(Condition::new("held", side(0, false), vec![Film::Fixed { kelvin: held }]))
        .with(Condition::new("blown on", side(0, true), vec![Film::Convection { w_m2_k: h, kelvin: fluid }]));
    let s = solve(&occ, &study).expect("solve");

    let (r_solid, r_film) = (LENGTH_M / (k * AREA_M2), 1.0 / (h * AREA_M2));
    let truth = (fluid - held) / (r_solid + r_film);
    assert!((watts(&s, "blown on") - truth).abs() < 1e-9, "{} vs {truth}", watts(&s, "blown on"));
    assert!((watts(&s, "held") + truth).abs() < 1e-9);
    // The wall under the film is cooler than the fluid by the drop across the film, and that
    // is the number someone touching it would read -- not the cell centre behind it.
    let wall = fluid - truth * r_film;
    assert!((wall_max(&s, "blown on") - wall).abs() < 1e-9, "{} vs {wall}", wall_max(&s, "blown on"));
    assert!(s.imbalance < 1e-9, "{}", s.imbalance);
}

#[test]
fn blowing_infinitely_hard_is_holding_a_temperature() {
    // A film coefficient has no resistance left as it grows, so convection has to become a
    // held temperature. If the two were assembled differently this would not close.
    let occ = slab(20);
    let fixed = Study::new(vec![SolidMaterial::isotropic(50.0)])
        .with(Condition::new("a", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]))
        .with(Condition::new("b", side(0, true), vec![Film::Fixed { kelvin: 400.0 }]));
    let blown = Study::new(vec![SolidMaterial::isotropic(50.0)])
        .with(Condition::new("a", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]))
        .with(Condition::new("b", side(0, true), vec![Film::Convection { w_m2_k: 1e12, kelvin: 400.0 }]));
    let a = solve(&occ, &fixed).expect("solve");
    let b = solve(&occ, &blown).expect("solve");
    for (x, y) in a.kelvin.iter().zip(&b.kelvin) {
        assert!((x - y).abs() < 1e-6, "{x} vs {y}");
    }
    assert!((watts(&a, "b") - watts(&b, "b")).abs() < 1e-5, "{} vs {}", watts(&a, "b"), watts(&b, "b"));
}

#[test]
fn a_heat_flux_on_a_face_is_the_watts_it_says() {
    let occ = slab(20);
    let (k, held, flux) = (50.0, 300.0, 12_500.0);
    let study = Study::new(vec![SolidMaterial::isotropic(k)])
        .with(Condition::new("held", side(0, false), vec![Film::Fixed { kelvin: held }]))
        .with(Condition::new("heated", side(0, true), vec![Film::Flux { w_m2: flux }]));
    let s = solve(&occ, &study).expect("solve");

    let truth = flux * AREA_M2;
    assert!((watts(&s, "heated") - truth).abs() < 1e-9, "{} vs {truth}", watts(&s, "heated"));
    assert!((watts(&s, "held") + truth).abs() < 1e-9);
    // A fixed flux through a slab is a linear rise: T(x) = held + q·x/k.
    for (eq, got) in s.kelvin.iter().enumerate() {
        let want = held + flux * x_of(&occ, eq) / k;
        assert!((got - want).abs() < 1e-9, "{got} vs {want}");
    }
}

#[test]
fn a_slab_that_generates_heat_is_a_parabola_out_by_exactly_one_term() {
    // The exact answer is T(x) = T₀ + q·x·(L − x)/2k. A cell-centred scheme reproduces the
    // second difference of a quadratic exactly in the interior, and the half-cell treatment at
    // the wall leaves a single uniform offset of q·h²/8k -- the same number in every cell, at
    // every spacing. Checking the offset as an equality is a far sharper test than a bound:
    // it pins the boundary treatment and the interior stencil at once.
    let (k, q, held) = (20.0, 5.0e5, 350.0);
    for span in [5, 10, 20, 40] {
        let occ = slab(span);
        let h = occ.grid().spacing() / 1000.0;
        let mut study = Study::new(vec![SolidMaterial::isotropic(k)])
            .with(Condition::new("left", side(0, false), vec![Film::Fixed { kelvin: held }]))
            .with(Condition::new("right", side(0, true), vec![Film::Fixed { kelvin: held }]));
        study.generation_w_m3 = vec![q];
        let s = solve(&occ, &study).expect("solve");

        let offset = q * h * h / (8.0 * k);
        for (eq, got) in s.kelvin.iter().enumerate() {
            let x = x_of(&occ, eq);
            let exact = held + q * x * (LENGTH_M - x) / (2.0 * k);
            assert!(
                (got - exact - offset).abs() < 1e-7,
                "{span} cells across, x = {x:.4} m: {got} K, exact {exact} K, and the scheme's offset is {offset} K"
            );
        }
        // Whatever the spacing, every watt generated leaves through the two ends -- the scheme
        // conserves energy over each cell, so the total is exact rather than second order.
        let generated = q * LENGTH_M * AREA_M2;
        assert!((s.generated_w - generated).abs() < 1e-6, "{} vs {generated}", s.generated_w);
        assert!((watts(&s, "left") + generated / 2.0).abs() < 1e-6, "{span}: {} W", watts(&s, "left"));
        assert!((watts(&s, "right") + generated / 2.0).abs() < 1e-6, "{span}: {} W", watts(&s, "right"));
        assert!(s.imbalance < 1e-9, "{span}: {}", s.imbalance);
    }
}

#[test]
fn an_orthotropic_slab_conducts_by_the_axis_the_heat_runs_along() {
    // Heat driven along x must see k_x and nothing else, whatever the other two are.
    let occ = slab(20);
    let material = SolidMaterial { conductivity_w_m_k: [50.0, 5.0, 500.0], density_kg_m3: 0.0, specific_heat_j_kg_k: 0.0 };
    let study = Study::new(vec![material]).with(Condition::new("cold", side(0, false), vec![Film::Fixed { kelvin: 300.0 }])).with(Condition::new(
        "hot",
        side(0, true),
        vec![Film::Fixed { kelvin: 400.0 }],
    ));
    let s = solve(&occ, &study).expect("solve");
    let truth = 50.0 * AREA_M2 * 100.0 / LENGTH_M;
    assert!((watts(&s, "hot") - truth).abs() < 1e-9, "{} vs {truth}", watts(&s, "hot"));
}

#[test]
fn radiation_settles_where_the_fourth_power_law_balances() {
    // One end held hot, the other radiating to cold surroundings and nothing else. The answer
    // is the wall temperature where the heat conduction delivers is the heat radiation takes:
    //     k·A/L·(T_hot − T_wall) = ε·σ·A·(T_wall⁴ − T_∞⁴)
    // which the test solves for itself, by bisection, with no reference to the solver.
    let occ = slab(20);
    let (k, hot, emissivity, ambient) = (50.0, 800.0, 0.9, 300.0);
    let study = Study::new(vec![SolidMaterial::isotropic(k)])
        .with(Condition::new("hot", side(0, false), vec![Film::Fixed { kelvin: hot }]))
        .with(Condition::new("radiating", side(0, true), vec![Film::Radiation { emissivity, kelvin: ambient }]));
    let s = solve(&occ, &study).expect("solve");

    let conduct = k * AREA_M2 / LENGTH_M;
    let radiate = emissivity * STEFAN_BOLTZMANN * AREA_M2;
    let residual = |t: f64| conduct * (hot - t) - radiate * (t.powi(4) - ambient.powi(4));
    let (mut lo, mut hi) = (ambient, hot);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if residual(mid) > 0.0 { lo = mid } else { hi = mid }
    }
    let wall = 0.5 * (lo + hi);
    let truth = radiate * (wall.powi(4) - ambient.powi(4));

    assert!(s.outer > 1, "a radiating surface has to be iterated: {}", s.outer);
    assert!((wall_max(&s, "radiating") - wall).abs() < 1e-4, "{} K vs {wall} K", wall_max(&s, "radiating"));
    assert!((watts(&s, "radiating") + truth).abs() / truth < 1e-6, "{} W vs {} W", watts(&s, "radiating"), -truth);
    assert!((watts(&s, "hot") - truth).abs() / truth < 1e-6);
    assert!(s.imbalance < 1e-6, "{}", s.imbalance);

    // The point of the identity: the law is satisfied at the fourth power, not at a tangent to
    // it. Read the reported wall temperature back through the full law and it has to agree.
    let reported = wall_max(&s, "radiating");
    let from_law = radiate * (reported.powi(4) - ambient.powi(4));
    assert!((from_law + watts(&s, "radiating")).abs() / truth < 1e-5, "{from_law} vs {}", -watts(&s, "radiating"));
}

#[test]
fn radiation_and_convection_on_one_face_add_up() {
    let occ = slab(20);
    let (k, hot, h, emissivity, ambient) = (50.0, 600.0, 15.0, 0.8, 300.0);
    let study = Study::new(vec![SolidMaterial::isotropic(k)]).with(Condition::new("hot", side(0, false), vec![Film::Fixed { kelvin: hot }])).with(
        Condition::new(
            "cooled",
            side(0, true),
            vec![Film::Convection { w_m2_k: h, kelvin: ambient }, Film::Radiation { emissivity, kelvin: ambient }],
        ),
    );
    let s = solve(&occ, &study).expect("solve");

    let conduct = k * AREA_M2 / LENGTH_M;
    let residual =
        |t: f64| conduct * (hot - t) - h * AREA_M2 * (t - ambient) - emissivity * STEFAN_BOLTZMANN * AREA_M2 * (t.powi(4) - ambient.powi(4));
    let (mut lo, mut hi) = (ambient, hot);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if residual(mid) > 0.0 { lo = mid } else { hi = mid }
    }
    let wall = 0.5 * (lo + hi);
    assert!((wall_max(&s, "cooled") - wall).abs() < 1e-4, "{} vs {wall}", wall_max(&s, "cooled"));
    assert!(s.imbalance < 1e-6, "{}", s.imbalance);
}

#[test]
fn the_radiative_film_coefficient_is_the_identity_it_claims_to_be() {
    // εσ(T⁴ − T∞⁴) = h_r·(T − T∞) is an algebraic identity, not an approximation, and this is
    // the statement the whole radiation treatment rests on.
    for (t, a, e) in [(400.0, 300.0, 0.9), (1200.0, 290.0, 0.35), (301.0, 300.0, 1.0), (300.0, 300.0, 0.5)] {
        let h = radiative_coefficient(e, t, a);
        let through_h = h * (t - a);
        let direct = e * STEFAN_BOLTZMANN * (t.powi(4) - a.powi(4));
        let scale = direct.abs().max(1e-12);
        assert!((through_h - direct).abs() / scale < 1e-12, "T = {t}: {through_h} vs {direct}");
    }
    assert_eq!(radiative_coefficient(0.0, 500.0, 300.0), 0.0, "a surface that does not radiate carries nothing");
}

#[test]
fn a_transient_cools_along_the_exponential_a_lumped_body_follows() {
    // A cube of very conductive material has no gradient worth speaking of, so it cools as a
    // single lump: T(t) = T∞ + (T₀ − T∞)·exp(−t/τ) with τ = ρ·c·V/(h·A). Backward Euler is
    // first order in the step, so halving the step has to halve the error -- which is the real
    // claim being tested, that the time term is assembled right.
    let side = 20.0;
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(side, side, side) };
    let grid = Grid3::over(b, 4, 1).expect("lattice");
    let mesh = block_at([0., 0., 0.], [side, side, side]);
    let occ = Occupancy::of(grid, &[&mesh], 1).expect("occupancy");

    // Copper's conductivity against a 100 W/(m²·K) film over a 20 mm cube puts the Biot
    // number at 8e-4, so the cube really has no gradient to speak of and the lumped answer is
    // the answer.
    let material = SolidMaterial { conductivity_w_m_k: [400.0; 3], density_kg_m3: 8000.0, specific_heat_j_kg_k: 500.0 };
    let (h, ambient, start) = (100.0, 300.0, 400.0);
    let volume = (side / 1000.0).powi(3);
    let area = 6.0 * (side / 1000.0).powi(2);
    let tau = material.volumetric_capacity() * volume / (h * area);
    assert!((tau - 133.333_333_333).abs() < 1e-6, "the case itself: {tau}");

    let finish = tau;
    let exact = ambient + (start - ambient) * (-finish / tau).exp();
    let mut errors = Vec::new();
    for steps in [10u32, 20, 40] {
        let dt = finish / steps as f64;
        let mut now = vec![start; occ.equations()];
        for _ in 0..steps {
            let mut study =
                Study::new(vec![material]).with(Condition::new("all over", Patch::Rest, vec![Film::Convection { w_m2_k: h, kelvin: ambient }]));
            study.time = Time::Step { seconds: dt, from: now.clone() };
            let s = solve(&occ, &study).expect("solve");
            now = s.kelvin;
        }
        let mean = now.iter().sum::<f64>() / now.len() as f64;
        errors.push((mean - exact).abs());
    }
    let (coarse, middle, fine) = (errors[0], errors[1], errors[2]);
    // Backward Euler over ten steps of a time constant lands at T∞ + ΔT/(1 + Δt/τ)^10, which
    // for this case is 1.77 K above the exponential -- so the error is known, not merely
    // bounded, and the test holds it to that as well as to its order.
    let predicted = (ambient + (start - ambient) / (1.0 + 1.0 / 10.0f64).powi(10) - exact).abs();
    assert!((coarse - predicted).abs() < 0.08, "{coarse} K, and backward Euler predicts {predicted} K");
    // First order: each halving of the step takes about half the error away.
    assert!((coarse / middle - 2.0).abs() < 0.25, "{coarse} then {middle}");
    assert!((middle / fine - 2.0).abs() < 0.25, "{middle} then {fine}");
    assert!(fine < 0.5, "{fine} K");
}

#[test]
fn a_transient_with_nothing_happening_stays_where_it_is() {
    let occ = slab(10);
    let material = SolidMaterial { conductivity_w_m_k: [50.0; 3], density_kg_m3: 7800.0, specific_heat_j_kg_k: 460.0 };
    let mut study = Study::new(vec![material]);
    study.time = Time::Step { seconds: 10.0, from: vec![350.0; occ.equations()] };
    let s = solve(&occ, &study).expect("solve");
    // Insulated all over and uniform to start with: a step changes nothing.
    for t in &s.kelvin {
        assert!((t - 350.0).abs() < 1e-8, "{t}");
    }
}

#[test]
fn an_insulated_model_has_no_single_answer_and_is_told_so() {
    let occ = slab(10);
    let study = Study::new(vec![SolidMaterial::isotropic(50.0)]);
    match solve(&occ, &study) {
        Err(SimError::Underdetermined(m)) => {
            assert!(m.contains("fixed temperature"), "{m}");
        }
        other => panic!("{other:?}"),
    }
    // Generating heat into an insulated body is just as underdetermined: it never settles.
    let mut study = Study::new(vec![SolidMaterial::isotropic(50.0)]);
    study.generation_w_m3 = vec![1000.0];
    assert!(matches!(solve(&occ, &study), Err(SimError::Underdetermined(_))));
    // An insulated flux condition does not settle it either.
    let study = Study::new(vec![SolidMaterial::isotropic(50.0)]).with(Condition::new("pushed", side(0, false), vec![Film::Flux { w_m2: 100.0 }]));
    assert!(matches!(solve(&occ, &study), Err(SimError::Underdetermined(_))));
}

#[test]
fn the_first_condition_to_claim_a_face_keeps_it() {
    let occ = slab(10);
    let study = Study::new(vec![SolidMaterial::isotropic(50.0)])
        .with(Condition::new("first", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]))
        .with(Condition::new("second", side(0, false), vec![Film::Fixed { kelvin: 900.0 }]))
        .with(Condition::new("other end", side(0, true), vec![Film::Fixed { kelvin: 400.0 }]));
    let s = solve(&occ, &study).expect("solve");
    assert!((s.min_kelvin - 305.0).abs() < 1e-9, "the second claim was ignored: {}", s.min_kelvin);
    assert!(s.patches.iter().all(|p| p.name != "second"), "{:?}", s.patches);
    // And a catch-all put first swallows everything after it.
    let study = Study::new(vec![SolidMaterial::isotropic(50.0)])
        .with(Condition::new("everything", Patch::Rest, vec![Film::Convection { w_m2_k: 10.0, kelvin: 300.0 }]))
        .with(Condition::new("never reached", side(0, true), vec![Film::Fixed { kelvin: 900.0 }]));
    let s = solve(&occ, &study).expect("solve");
    assert!((s.max_kelvin - 300.0).abs() < 1e-6, "{}", s.max_kelvin);
}

#[test]
fn naming_a_handful_of_faces_works_the_same_as_naming_a_side() {
    use crate::thermal::SurfaceFace;
    let occ = slab(10);
    let listed: Vec<SurfaceFace> =
        occ.boundary().filter(|f| f.axis == 0 && !f.positive).map(|f| SurfaceFace::new(f.cell, f.axis, f.positive)).collect();
    assert!(!listed.is_empty());
    let by_side = Study::new(vec![SolidMaterial::isotropic(50.0)])
        .with(Condition::new("a", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]))
        .with(Condition::new("b", side(0, true), vec![Film::Fixed { kelvin: 400.0 }]));
    let by_face = Study::new(vec![SolidMaterial::isotropic(50.0)])
        .with(Condition::new("a", Patch::Faces(listed), vec![Film::Fixed { kelvin: 300.0 }]))
        .with(Condition::new("b", side(0, true), vec![Film::Fixed { kelvin: 400.0 }]));
    let x = solve(&occ, &by_side).expect("solve");
    let y = solve(&occ, &by_face).expect("solve");
    for (a, b) in x.kelvin.iter().zip(&y.kelvin) {
        assert!((a - b).abs() < 1e-12, "{a} vs {b}");
    }
}

#[test]
fn a_contradictory_or_impossible_study_is_refused_rather_than_guessed_at() {
    let occ = slab(10);
    let held = |k: f64| Condition::new("end", side(0, true), vec![Film::Fixed { kelvin: k }]);
    let base = || Study::new(vec![SolidMaterial::isotropic(50.0)]);

    // A surface cannot be held at a temperature and also given a film.
    let study =
        base().with(Condition::new("both", side(0, false), vec![Film::Fixed { kelvin: 300.0 }, Film::Convection { w_m2_k: 5.0, kelvin: 300.0 }]));
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));

    // Nor at two temperatures.
    let study = base().with(Condition::new("twice", side(0, false), vec![Film::Fixed { kelvin: 300.0 }, Film::Fixed { kelvin: 400.0 }]));
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));

    // Temperatures here are absolute, so zero and below are not temperatures.
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        let study = base().with(Condition::new("cold", side(0, false), vec![Film::Fixed { kelvin: bad }])).with(held(400.0));
        assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))), "{bad} was accepted");
    }
    // An emissivity is a fraction.
    for bad in [-0.1, 1.5, f64::NAN] {
        let study = base().with(Condition::new("glow", side(0, false), vec![Film::Radiation { emissivity: bad, kelvin: 300.0 }])).with(held(400.0));
        assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))), "{bad} was accepted");
    }
    // A film coefficient may not be negative, and a flux must be a number.
    let study = base().with(Condition::new("suck", side(0, false), vec![Film::Convection { w_m2_k: -5.0, kelvin: 300.0 }])).with(held(400.0));
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));
    let study = base().with(Condition::new("odd", side(0, false), vec![Film::Flux { w_m2: f64::NAN }])).with(held(400.0));
    assert!(matches!(solve(&occ, &study), Err(SimError::Setup(_))));
}

#[test]
fn a_material_that_cannot_conduct_is_refused() {
    let occ = slab(10);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let study = Study::new(vec![SolidMaterial::isotropic(bad)]).with(Condition::new("a", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]));
        match solve(&occ, &study) {
            Err(SimError::Material(m)) => assert!(m.contains("conduct"), "{m}"),
            other => panic!("{bad} was accepted: {other:?}"),
        }
    }
    // No material at all.
    let study = Study::new(vec![]).with(Condition::new("a", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]));
    assert!(matches!(solve(&occ, &study), Err(SimError::Material(_))));
    // A transient needs something to store heat in.
    let mut study = Study::new(vec![SolidMaterial::isotropic(50.0)]).with(Condition::new("a", side(0, false), vec![Film::Fixed { kelvin: 300.0 }]));
    study.time = Time::Step { seconds: 1.0, from: vec![300.0; occ.equations()] };
    assert!(matches!(solve(&occ, &study), Err(SimError::Material(_))));
}

#[test]
fn a_hostile_time_step_is_refused() {
    let occ = slab(10);
    let material = SolidMaterial { conductivity_w_m_k: [50.0; 3], density_kg_m3: 7800.0, specific_heat_j_kg_k: 460.0 };
    let make = |seconds: f64, from: Vec<f64>| {
        let mut s = Study::new(vec![material]);
        s.time = Time::Step { seconds, from };
        s
    };
    let n = occ.equations();
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(solve(&occ, &make(bad, vec![300.0; n])), Err(SimError::Setup(_))), "{bad} was accepted");
    }
    assert!(matches!(solve(&occ, &make(1.0, vec![300.0; n - 1])), Err(SimError::Setup(_))));
    assert!(matches!(solve(&occ, &make(1.0, vec![f64::NAN; n])), Err(SimError::Setup(_))));
    assert!(matches!(solve(&occ, &make(1.0, vec![-10.0; n])), Err(SimError::Setup(_))));
}

#[test]
fn a_cube_cooled_all_over_is_hottest_in_the_middle() {
    // The first real shape question: generating heat inside a block cooled on every side, the
    // worst place is the centre, and the answer is symmetric because the problem is. This is
    // the shape of every "where does it get hot" answer the tool will give.
    let side = 40.0;
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(side, side, side) };
    let grid = Grid3::over(b, 8, 1).expect("lattice");
    let mesh = block_at([0., 0., 0.], [side, side, side]);
    let occ = Occupancy::of(grid, &[&mesh], 1).expect("occupancy");
    let study = Study::new(vec![SolidMaterial::isotropic(1.0)]).with(Condition::new(
        "all over",
        Patch::Rest,
        vec![Film::Convection { w_m2_k: 50.0, kelvin: 300.0 }],
    ));
    let mut study = study;
    study.generation_w_m3 = vec![2.0e5];
    let s = solve(&occ, &study).expect("solve");

    // The hottest cell is one of the eight that share the body's centre.
    let hottest =
        s.kelvin.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(e, _)| e).expect("a hottest cell");
    let at = occ.grid().centre(occ.cell_of(hottest).expect("cell")).expect("centre");
    for v in [at.x, at.y, at.z] {
        assert!((v - side / 2.0).abs() <= occ.grid().spacing(), "the hottest place is not central: {at:?}");
    }
    // Symmetric problem, symmetric answer: the eight cells around the centre agree.
    let middle: Vec<f64> = s
        .kelvin
        .iter()
        .enumerate()
        .filter(|(e, _)| {
            let c = occ.grid().centre(occ.cell_of(*e).expect("cell")).expect("centre");
            [c.x, c.y, c.z].iter().all(|v| (v - side / 2.0).abs() < occ.grid().spacing())
        })
        .map(|(_, t)| *t)
        .collect();
    assert_eq!(middle.len(), 8, "eight cells share the centre");
    let spread = middle.iter().fold(f64::MIN, |a, b| a.max(*b)) - middle.iter().fold(f64::MAX, |a, b| a.min(*b));
    assert!(spread < 1e-9, "the answer is not symmetric: {spread} K across the middle");
    // Every watt generated leaves through the surface.
    assert!(s.imbalance < 1e-9, "{}", s.imbalance);
    assert!(s.max_kelvin > 300.0 && s.min_kelvin > 300.0, "{s:?}");
}

/// A round part sheds heat over the area it has, not over the area of the steps standing in
/// for it.
///
/// A cylinder generating heat and cooled all over settles where the heat it makes equals the
/// heat its surface gives up: `Q = h·A·ΔT`. Nothing about the shape lands on the lattice, so
/// this is the case that shows whether the areas are the model's. The staircase's curved side
/// is `4/π` too large, which on this part would read the temperature rise 18 per cent low --
/// the sort of error that says a part is fine when it is not.
#[test]
fn a_cylinder_sheds_heat_over_the_area_a_cylinder_has() {
    let (radius, length) = (10.0, 20.0);
    let mesh = crate::test_shapes::cylinder(radius, 0.0, length, 256);
    let refs = [&mesh];
    let grid = Grid3::over(mesh.bounds(), 40, 0).expect("a lattice over the cylinder");
    let occ = Occupancy::of(grid, &refs, 4).expect("the cylinder on the lattice");
    let tags = crate::tag::FaceTags::of(&occ, &refs);

    // Copper, so that the part is near enough all one temperature and the answer is the
    // surface's and nothing else: centre to wall is only q·r²/(4k) = 0.6 K of the 667 below.
    let copper = SolidMaterial { conductivity_w_m_k: [400.0; 3], density_kg_m3: 8960.0, specific_heat_j_kg_k: 385.0 };
    let generation = 1.0e7;
    let film = 50.0;
    let ambient = 300.0;

    let mut study = Study::new(vec![copper]);
    study.generation_w_m3 = vec![generation];
    study.conditions = vec![Condition::new("all over", Patch::Rest, vec![Film::Convection { w_m2_k: film, kelvin: ambient }])];
    study.area_scale = tags.area_scale(&occ, &refs);

    let answer = solve(&occ, &study).expect("the cylinder settles");
    assert!(answer.imbalance < 1e-6, "the balance is out by {}", answer.imbalance);

    let volume = std::f64::consts::PI * radius * radius * length * 1e-9;
    let area = (std::f64::consts::TAU * radius * length + std::f64::consts::TAU * radius * radius) * 1e-6;
    let watts = generation * volume;
    let rise = watts / (film * area);

    // The heat generated is the volume's, which the cells carry by their share.
    let generated_error = (answer.generated_w - watts).abs() / watts;
    assert!(generated_error < 0.005, "it generated {} W against {watts}", answer.generated_w);

    // The area it sheds over is the cylinder's.
    let reported: f64 = answer.patches.iter().map(|p| p.area_m2).sum();
    let area_error = (reported - area).abs() / area;
    assert!(area_error < 0.005, "it shed over {reported} m2 against {area}");

    // And so the temperature it settles at is the one `Q = h·A·ΔT` gives.
    let mean = answer.wall_kelvin.iter().sum::<f64>() / answer.wall_kelvin.len() as f64;
    let got = mean - ambient;
    let error = (got - rise).abs() / rise;
    assert!(error < 0.02, "the wall settled {got} K above the air against {rise} K");

    // What it would have said without the correction, so that this test is measuring something
    // and goes on measuring it: the staircase sheds over a larger surface than the cylinder
    // has, and reads the part a good deal cooler than it runs.
    let mut staircase = study.clone();
    staircase.area_scale = Vec::new();
    let worse = solve(&occ, &staircase).expect("the staircase settles too");
    let mean = worse.wall_kelvin.iter().sum::<f64>() / worse.wall_kelvin.len() as f64;
    let under = (rise - (mean - ambient)) / rise;
    assert!(under > 0.1, "the staircase read only {under} of the rise low, so this test proves nothing");
}

/// The heat crossing a slanted wall is the heat crossing the wall the model has.
///
/// A wedge held hot inside and cooled on its 45° face loses `h·A·ΔT` through it, and the
/// staircase standing in for that face has √2 times its area. Here the drop across the part
/// matters as little as it can be made to, so what is left is the area.
#[test]
fn a_slanted_wall_loses_heat_over_its_own_area() {
    let (leg, width) = (20.0, 20.0);
    let mesh = crate::test_shapes::wedge(leg, width);
    let refs = [&mesh];
    let grid = Grid3::over(mesh.bounds(), 32, 0).expect("a lattice over the wedge");
    let occ = Occupancy::of(grid, &refs, 4).expect("the wedge on the lattice");
    let tags = crate::tag::FaceTags::of(&occ, &refs);

    let copper = SolidMaterial { conductivity_w_m_k: [400.0; 3], density_kg_m3: 8960.0, specific_heat_j_kg_k: 385.0 };
    let hot = 400.0;
    let ambient = 300.0;
    let film = 20.0;

    let mut study = Study::new(vec![copper]);
    study.conditions = vec![
        // Face 2, the bottom, is held hot; face 0, the slant, is cooled; the rest is shut.
        Condition::new("heated", Patch::Faces(tags.patch(0, 2)), vec![Film::Fixed { kelvin: hot }]),
        Condition::new("cooled", Patch::Faces(tags.patch(0, 0)), vec![Film::Convection { w_m2_k: film, kelvin: ambient }]),
        Condition::new("shut", Patch::Rest, vec![Film::Insulated]),
    ];
    study.area_scale = tags.area_scale(&occ, &refs);

    let answer = solve(&occ, &study).expect("the wedge settles");
    assert!(answer.imbalance < 1e-6, "the balance is out by {}", answer.imbalance);

    let slant = leg * 2.0f64.sqrt() * width * 1e-6;
    let cooled = answer.patches.iter().find(|p| p.name == "cooled").expect("the cooled patch");
    let area_error = (cooled.area_m2 - slant).abs() / slant;
    assert!(area_error < 0.01, "the slant came out {} m2 against {slant}", cooled.area_m2);

    // Copper over 20 mm at this film drops under a kelvin, so the loss is h·A·ΔT to within
    // that: a staircase would have claimed √2 times as much.
    let want = -film * slant * (hot - ambient);
    let error = (cooled.watts - want).abs() / want.abs();
    assert!(error < 0.03, "the slant lost {} W against {want}", cooled.watts);
}

/// A bar that tapers conducts what its own cross-section lets through.
///
/// Held at both ends, a bar whose height rises linearly has a resistance of
/// `L·ln(h₁/h₀) / (k·w·(h₁−h₀))` -- the integral of `dx/(k·A(x))`. By the complementary
/// principle that figure is the most resistance the bar can have, so the heat crossing it is at
/// least what this test asks for and no more than a little over.
///
/// This is the case that decided against weighing each cell face by how much of it is material:
/// see `docs/analysis-verification.md`. Rounding a cell to solid or not costs 1.1 per cent here,
/// where the face fractions cost 3.2, because a cell the model fills less than half of is left
/// out and the material it held leaves the cross-section with it -- while the rounding lets the
/// cells just over half make up for the cells just under.
#[test]
fn a_tapering_bar_conducts_what_its_cross_section_allows() {
    let (length, width, h0, h1) = (80.0, 20.0, 10.0, 20.0);
    let mesh = crate::test_shapes::taper(length, width, h0, h1);
    let refs = [&mesh];
    let k = 50.0;
    let (hot, cold) = (400.0, 300.0);

    // L·ln(h₁/h₀) / (k·w·(h₁−h₀)), in metres.
    let resistance = (length * 1e-3) * (h1 / h0).ln() / (k * (width * 1e-3) * ((h1 - h0) * 1e-3));
    let want = (hot - cold) / resistance;

    for span in [32usize, 48] {
        let grid = Grid3::over(mesh.bounds(), span, 0).expect("a lattice over the bar");
        let occ = Occupancy::of(grid, &refs, 4).expect("the bar on the lattice");
        let tags = crate::tag::FaceTags::of(&occ, &refs);

        let mut study = Study::new(vec![SolidMaterial::isotropic(k)]);
        study.conditions = vec![
            Condition::new("hot", Patch::Faces(tags.patch(0, 0)), vec![Film::Fixed { kelvin: hot }]),
            Condition::new("cold", Patch::Faces(tags.patch(0, 1)), vec![Film::Fixed { kelvin: cold }]),
            Condition::new("shut", Patch::Rest, vec![Film::Insulated]),
        ];
        study.area_scale = tags.area_scale(&occ, &refs);

        let answer = solve(&occ, &study).expect("the bar settles");
        assert!(answer.imbalance < 1e-6, "span {span}: the balance is out by {}", answer.imbalance);
        let crossing = answer.patches.iter().find(|p| p.name == "hot").expect("the hot end").watts;
        // The slope of the top is one in eight, so the heat does not run quite straight and the
        // one-dimensional answer is itself a few per cent out; this is as close as the exact
        // number deserves to be held.
        let error = (crossing - want).abs() / want;
        assert!(error < 0.05, "span {span}: {crossing} W crossed against {want}");
    }
}

/// Two-dimensional conduction, against the Fourier series that solves it.
///
/// Every conduction case above runs in one direction. A solver can get all of them right and
/// still be wrong in two, where the heat turns a corner: the face conductivities, the corner
/// cells and the boundary half-cells all couple in a way a slab never exercises.
///
/// A square plate held cold on three sides and hot on the fourth is the standard case, and its
/// answer is known exactly. The series is
/// `T = (4 dT/pi) sum_{n odd} (1/n) sin(n pi x/L) sinh(n pi z/L) / sinh(n pi)`, and at the
/// centre it collapses: four copies of this problem, one per side, add to `dT` everywhere by
/// superposition, so the centre of each is **exactly a quarter of the rise**, with no series to
/// sum and no tolerance to argue about.
#[test]
fn a_square_plate_hot_on_one_side_is_a_quarter_warm_at_its_centre() {
    let l = 40.0;
    // Thin across y and insulated there, so the problem is the two-dimensional one.
    let mesh = crate::test_shapes::block_at([0.0, 0.0, 0.0], [l, 5.0, l]);
    let refs = [&mesh];
    // An odd span puts a cell at the centre rather than between four of them.
    let grid = Grid3::over(mesh.bounds(), 33, 0).expect("a lattice over the plate");
    let occ = Occupancy::of(grid, &refs, 2).expect("the plate on the lattice");
    let tags = crate::tag::FaceTags::of(&occ, &refs);

    let (cold, hot) = (300.0, 400.0);
    let rise = hot - cold;
    let mut study = Study::new(vec![SolidMaterial::isotropic(50.0)]);
    // block_at numbers its sides -x, +x, -y, +y, -z, +z.
    let mut cold_faces = tags.patch(0, 0);
    cold_faces.extend(tags.patch(0, 1));
    cold_faces.extend(tags.patch(0, 4));
    study.conditions = vec![
        Condition::new("hot", Patch::Faces(tags.patch(0, 5)), vec![Film::Fixed { kelvin: hot }]),
        Condition::new("cold", Patch::Faces(cold_faces), vec![Film::Fixed { kelvin: cold }]),
        Condition::new("sides", Patch::Rest, vec![Film::Insulated]),
    ];

    let answer = solve(&occ, &study).expect("the plate settles");

    // The centre cell, and what the exact answer says it holds.
    let g = occ.grid();
    let [nx, _, nz] = g.count();
    let centre = g.at(nx / 2, 1, nz / 2).and_then(|c| occ.eq_of(c)).expect("a cell at the centre");
    let got = answer.kelvin.get(centre).copied().expect("its temperature");
    let want = cold + rise / 4.0;
    let error = (got - want).abs() / rise;
    // Measured: 325.0000 K against 325.0000 K. The scheme carries the superposition the exact
    // answer is built from, so the quarter is not approached, it is reproduced.
    assert!(error < 1e-6, "the centre is at {got} K and the exact answer is {want} K");

    // And a point off the centre, against the series itself.
    let series = |x: f64, z: f64| -> f64 {
        let mut sum = 0.0;
        let mut n = 1.0f64;
        while n < 200.0 {
            let a = n * std::f64::consts::PI;
            sum += (a * x / l).sin() * (a * z / l).sinh() / (n * a.sinh());
            n += 2.0;
        }
        cold + rise * 4.0 / std::f64::consts::PI * sum
    };
    let (i, k) = (nx / 4, (3 * nz) / 4);
    let at = g.centre(g.at(i, 1, k).expect("a cell")).expect("its middle");
    let off = g.at(i, 1, k).and_then(|c| occ.eq_of(c)).expect("an equation");
    let got = answer.kelvin.get(off).copied().expect("its temperature");
    let want = series(at.x, at.z);
    let error = (got - want).abs() / rise;
    // Measured: 342.7841 against the series' 342.7902, six parts in a hundred thousand.
    assert!(error < 5e-4, "at ({:.1}, {:.1}) it is {got} K and the series says {want} K", at.x, at.z);
}
