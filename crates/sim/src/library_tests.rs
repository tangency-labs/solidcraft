use solidcraft_doc::MATERIALS;
use solidcraft_doc::physical_material::SourceKind;

use crate::library::{AIR, FLUIDS, NOMINAL_SOLIDS, WATER, fluid, nominal, nominal_card};
use crate::material::{Alignment, read};

#[test]
fn every_built_in_material_but_the_placeholder_has_a_nominal_set() {
    // The document's library and this one are keyed by the same names. If a material is added
    // there and not here, an analysis on it would have to refuse, so this test is what keeps
    // the two in step.
    let missing: Vec<&str> = MATERIALS.iter().map(|(n, _)| *n).filter(|n| *n != "Default" && nominal(n).is_none()).collect();
    assert!(missing.is_empty(), "no nominal thermal properties for: {missing:?}");
    // And nothing here names a material the document does not have.
    let unknown: Vec<&str> = NOMINAL_SOLIDS.iter().map(|m| m.name).filter(|n| !MATERIALS.iter().any(|(d, _)| d == n)).collect();
    assert!(unknown.is_empty(), "nominal properties for materials the document does not have: {unknown:?}");
    // "Default" stands for no choice made, so it deliberately has none.
    assert!(nominal("Default").is_none(), "the placeholder must not pretend to be a material");
}

#[test]
fn a_nominal_set_is_a_set_of_usable_numbers() {
    for m in NOMINAL_SOLIDS {
        for k in m.conductivity_w_m_k {
            assert!(k > 0.0 && k < 5000.0, "{}: {k} W/(m·K)", m.name);
        }
        assert!(m.specific_heat_j_kg_k > 100.0 && m.specific_heat_j_kg_k < 1.0e4, "{}: {}", m.name, m.specific_heat_j_kg_k);
        assert!((0.0..=1.0).contains(&m.emissivity), "{}: {}", m.name, m.emissivity);
        for a in m.expansion_per_k {
            assert!(a > 0.0 && a < 1e-3, "{}: {a} per K", m.name);
        }
    }
}

#[test]
fn the_nominal_sets_keep_the_order_of_magnitude_the_physics_has() {
    let k = |name: &str| nominal(name).map(|m| m.conductivity_w_m_k[0]).unwrap_or(f64::NAN);
    // Copper beats aluminium beats steel beats stainless beats titanium, and every metal beats
    // every plastic by two to three orders of magnitude. This is the ranking a user's intuition
    // will check the tool against first.
    assert!(k("Copper") > k("Aluminum"), "{} vs {}", k("Copper"), k("Aluminum"));
    assert!(k("Aluminum") > k("Brass"));
    assert!(k("Brass") > k("Steel"));
    assert!(k("Steel") > k("Stainless Steel"));
    assert!(k("Stainless Steel") > k("Titanium"));
    assert!(k("Titanium") > k("Glass"));
    assert!(k("Glass") > k("Nylon"));
    assert!(k("Steel") / k("ABS Plastic") > 100.0, "a metal conducts far more than a plastic");
    // A bright metal hardly radiates; paint and plastic nearly fully.
    assert!(nominal("Copper").is_some_and(|m| m.emissivity < 0.2));
    assert!(nominal("ABS Plastic").is_some_and(|m| m.emissivity > 0.8));
}

#[test]
fn oak_is_the_one_that_knows_which_way_the_grain_runs() {
    let oak = nominal("Oak").expect("oak");
    // Along the grain it conducts more and moves less, which is the whole of why wood behaves
    // the way it does.
    assert!(oak.conductivity_w_m_k[0] > oak.conductivity_w_m_k[1]);
    assert!(oak.expansion_per_k[0] < oak.expansion_per_k[2] / 5.0, "{:?}", oak.expansion_per_k);
}

#[test]
fn a_nominal_card_reads_back_as_the_numbers_it_was_made_from() {
    // Nothing takes a short cut: the built-in materials go through a card and the card reader,
    // the same as somebody's own data.
    let card = nominal_card("Aluminum", 2700.0).expect("a card");
    card.validate().expect("the card the library makes is a valid card");
    let p = read(&card, Alignment::WorldAxes).expect("read");
    assert_eq!(p.designation, "Aluminum");
    assert_eq!(p.thermal.conductivity_w_m_k, [167.0; 3]);
    assert_eq!(p.thermal.density_kg_m3, 2700.0);
    assert_eq!(p.thermal.specific_heat_j_kg_k, 896.0);
    assert_eq!(p.expansion_per_k, Some([2.34e-5; 3]));
    p.for_transient_conduction().expect("a transient can run on a nominal card");
    // And it is marked for what it is, so the mark reaches the result.
    assert!(p.illustrative_only, "nominal numbers are not measurements");
    assert_eq!(card.sources.first().map(|s| s.kind), Some(SourceKind::Illustrative));
    // With the temperature it was taken at attached.
    assert!(p.measured_at_kelvin.iter().all(|(_, t)| (*t - 293.15).abs() < 1e-9), "{:?}", p.measured_at_kelvin);
}

#[test]
fn oaks_card_carries_its_grain_through_the_reader() {
    let card = nominal_card("Oak", 750.0).expect("a card");
    card.validate().expect("valid");
    // Grain along the model's x.
    let p = read(&card, Alignment::WorldAxes).expect("read");
    assert_eq!(p.thermal.conductivity_w_m_k, [0.17, 0.11, 0.10]);
    // Grain turned to run along the model's z: axis 1 → z, axis 2 → x, axis 3 → y.
    let axes = [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let p = read(&card, Alignment::Along(axes)).expect("read");
    let k = p.thermal.conductivity_w_m_k;
    assert!((k[2] - 0.17).abs() < 1e-12, "the grain now conducts along z: {k:?}");
    assert!((k[0] - 0.11).abs() < 1e-12, "{k:?}");
    assert!((k[1] - 0.10).abs() < 1e-12, "{k:?}");
}

#[test]
fn an_unknown_material_has_no_nominal_set_rather_than_a_guessed_one() {
    assert!(nominal("Unobtainium").is_none());
    assert!(nominal_card("Unobtainium", 1000.0).is_none());
    assert!(nominal("").is_none());
}

#[test]
fn a_material_name_is_matched_the_way_a_person_would_type_it() {
    assert!(nominal("  stainless steel  ").is_some());
    assert!(nominal("ALUMINUM").is_some());
    assert!(fluid(" air ").is_some());
    assert!(fluid("WATER").is_some());
}

#[test]
fn the_fluids_prandtl_numbers_are_the_ones_the_tables_give() {
    // Pr = μ·c/k, and the accepted values at 20 °C are 0.731 for air and 7.0 for water. These
    // follow from the four numbers stored, so getting them right means all four are consistent.
    assert!((AIR.prandtl() - 0.731).abs() < 0.002, "air: {}", AIR.prandtl());
    assert!((WATER.prandtl() - 7.0).abs() < 0.02, "water: {}", WATER.prandtl());
    // ν = μ/ρ: 1.516e-5 m²/s for air, 1.004e-6 for water -- water is a hundred times less
    // viscous by this measure, despite being a thousand times denser.
    assert!((AIR.kinematic_viscosity_m2_s() - 1.516e-5).abs() < 1e-7, "{}", AIR.kinematic_viscosity_m2_s());
    assert!((WATER.kinematic_viscosity_m2_s() - 1.004e-6).abs() < 1e-8, "{}", WATER.kinematic_viscosity_m2_s());
    // α = k/(ρc), and for a gas it is close to ν, which is what Pr ≈ 1 means.
    assert!((AIR.diffusivity_m2_s() / AIR.kinematic_viscosity_m2_s() - 1.0 / AIR.prandtl()).abs() < 1e-9);
}

#[test]
fn a_reynolds_number_comes_out_where_it_should() {
    // Ten metres a second over a tenth of a metre of air: about 66 thousand, which is well
    // into the turbulent range for an external flow and is the sort of case a fan study is.
    let re = AIR.reynolds(10.0, 0.1);
    assert!((re - 65_963.0).abs() < 50.0, "{re}");
    // The same in water, a hundred times more.
    assert!(WATER.reynolds(10.0, 0.1) / re > 10.0);
    // Nothing moving, nothing happening.
    assert_eq!(AIR.reynolds(0.0, 0.1), 0.0);
    // A fluid with no viscosity has no Reynolds number to speak of, and says so rather than
    // dividing by nothing.
    let ideal = crate::library::Fluid { dynamic_viscosity_pa_s: 0.0, ..AIR };
    assert!(ideal.reynolds(1.0, 1.0).is_infinite());
}

#[test]
fn the_fluids_are_a_usable_set() {
    for f in FLUIDS {
        assert!(f.density_kg_m3 > 0.0 && f.dynamic_viscosity_pa_s > 0.0, "{}", f.name);
        assert!(f.conductivity_w_m_k > 0.0 && f.specific_heat_j_kg_k > 0.0, "{}", f.name);
        assert!(f.prandtl() > 0.1 && f.prandtl() < 100.0, "{}: {}", f.name, f.prandtl());
    }
}

#[test]
fn every_built_in_material_but_the_wood_has_a_stiffness_a_load_case_can_use() {
    for m in NOMINAL_SOLIDS {
        match (m.young_modulus_pa, m.orthotropic) {
            (Some(e), None) => {
                assert!(e > 1.0e9 && e < 1.0e12, "{}: {e} Pa", m.name);
                assert!(m.poisson_ratio > -1.0 && m.poisson_ratio < 0.5, "{}: {}", m.name, m.poisson_ratio);
            }
            // Wood is the one whose stiffness has no single number, and it says so by carrying
            // the orthotropic set instead.
            (None, Some((e, g, nu))) => {
                assert_eq!(m.name, "Oak");
                assert!(e[0] > e[1] && e[1] > e[2], "stiffest along the grain: {e:?}");
                assert!(g.iter().all(|v| *v > 0.0), "{g:?}");
                assert!(nu.iter().all(|v| v.abs() < 1.0), "{nu:?}");
            }
            other => panic!("{} has {other:?}: a material needs one stiffness or the other", m.name),
        }
        assert!(m.tensile_strength_pa > 1.0e6 && m.tensile_strength_pa < 1.0e10, "{}: {}", m.name, m.tensile_strength_pa);
        if let Some(y) = m.yield_strength_pa {
            assert!(y > 1.0e6, "{}: {y} Pa", m.name);
            assert!(y <= m.tensile_strength_pa, "{} yields above where it breaks: {y} against {}", m.name, m.tensile_strength_pa);
            assert!(!m.brittle, "{} both yields and is brittle", m.name);
        } else {
            assert!(m.brittle, "{} has no yield point and is not marked brittle", m.name);
        }
    }
}

#[test]
fn the_stiffnesses_keep_the_order_a_users_intuition_has() {
    let e = |name: &str| nominal(name).and_then(|m| m.young_modulus_pa).unwrap_or(f64::NAN);
    let y = |name: &str| nominal(name).and_then(|m| m.yield_strength_pa).unwrap_or(f64::NAN);
    // Steel is the stiffest of the common metals, aluminium about a third of it, and every
    // metal is tens of times stiffer than any plastic.
    assert!(e("Steel") > e("Copper"), "{} vs {}", e("Steel"), e("Copper"));
    assert!(e("Copper") > e("Titanium"));
    assert!(e("Titanium") > e("Aluminum"));
    assert!(e("Aluminum") / e("ABS Plastic") > 20.0, "a metal is far stiffer than a plastic");
    // Titanium is where the strength is, and annealed copper is where it is not.
    assert!(y("Titanium") > y("Steel") * 3.0, "{} vs {}", y("Titanium"), y("Steel"));
    assert!(y("Copper") < y("Steel"));
    // Glass is stiff and weak at once, which is what being brittle means.
    assert!(nominal("Glass").is_some_and(|m| m.brittle && m.young_modulus_pa.is_some_and(|e| e > 50.0e9)));
    assert!(nominal("Glass").is_some_and(|m| m.tensile_strength_pa < 100.0e6));
}

#[test]
fn a_nominal_card_reads_back_as_a_material_a_load_case_can_run_on() {
    let card = nominal_card("Aluminum", 2700.0).expect("a card");
    card.validate().expect("valid");
    let p = read(&card, Alignment::WorldAxes).expect("read");
    let m = p.for_stress().expect("aluminium can carry a load");
    assert!((m.young_modulus_pa - 68.9e9).abs() < 1.0);
    assert!((m.poisson_ratio - 0.33).abs() < 1e-12);
    assert_eq!(m.yield_strength_pa, Some(276.0e6));
    assert_eq!(m.expansion_per_k, [2.34e-5; 3]);
    assert!((m.density_kg_m3 - 2700.0).abs() < 1e-9, "the density it was made with");
    // Lamé's constants follow, and the shear modulus of aluminium is about 26 GPa.
    let (_, mu) = m.lame();
    assert!((mu / 1.0e9 - 25.9).abs() < 0.5, "{} GPa", mu / 1.0e9);
}

#[test]
fn a_brittle_materials_card_gives_no_yield_point_to_measure_against() {
    let card = nominal_card("Cast Iron", 7200.0).expect("a card");
    card.validate().expect("valid");
    let p = read(&card, Alignment::WorldAxes).expect("read");
    let m = p.for_stress().expect("cast iron can still carry a load");
    assert_eq!(m.yield_strength_pa, None, "it breaks rather than yields");
    // The tensile strength is on the card for a caller to compare against itself.
    assert_eq!(p.tensile_strength_pa, Some(200.0e6));
}

#[test]
fn oaks_card_is_refused_by_a_load_case_rather_than_averaged_into_a_lie() {
    let card = nominal_card("Oak", 750.0).expect("a card");
    card.validate().expect("an orthotropic stiffness is a valid card");
    let p = read(&card, Alignment::WorldAxes).expect("read");
    assert_eq!(p.young_modulus_pa, None);
    assert!(p.absent.contains(&"isotropic elasticity"), "{:?}", p.absent);
    match p.for_stress() {
        Err(crate::SimError::Material(m)) => {
            assert!(m.contains("orthotropic"), "{m}");
            assert!(m.contains("not supported yet"), "an unfinished feature says so: {m}");
            assert!(m.contains("Oak"), "it names the material: {m}");
        }
        other => panic!("{other:?}"),
    }
    // And conduction on the same oak is fine: that much the lattice can carry.
    p.for_steady_conduction().expect("oak conducts");
}

#[test]
fn a_card_with_no_stiffness_at_all_says_which_property_is_missing() {
    use serde_json::json;
    use solidcraft_doc::physical_material::admit_card;
    let value = json!({
        "schema_version": 1,
        "designation": "mystery",
        "family": "other",
        "sources": [{"id": "s", "title": "a test", "kind": "user_supplied", "license": null, "reference": null}],
        "properties": [{"data": {"kind": "thermal_conductivity", "w_m_k": [1.0, 1.0, 1.0], "axes": "material123"},
                        "source_id": "s"}],
    });
    let card = admit_card(&value).expect("a card");
    let p = read(&card, Alignment::WorldAxes).expect("read");
    match p.for_stress() {
        Err(crate::SimError::Material(m)) => {
            assert!(m.contains("elasticity"), "{m}");
            assert!(m.contains("mystery"), "{m}");
        }
        other => panic!("{other:?}"),
    }
}
