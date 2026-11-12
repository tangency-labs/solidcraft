use serde_json::json;
use solidcraft_doc::physical_material::{MaterialCard, admit_card};

use crate::SimError;
use crate::material::{Alignment, read};

/// A card built from a list of properties, all from one source.
fn card(kind: &str, properties: serde_json::Value) -> MaterialCard {
    let value = json!({
        "schema_version": 1,
        "designation": "test material",
        "family": "metal",
        "sources": [{"id": "s", "title": "a test", "kind": kind, "license": null, "reference": null}],
        "properties": properties,
    });
    admit_card(&value).expect("a card")
}

fn property(data: serde_json::Value) -> serde_json::Value {
    json!({"data": data, "source_id": "s"})
}

#[test]
fn a_card_with_everything_reads_straight_through() {
    let c = card(
        "public_reference",
        json!([
            property(json!({"kind": "thermal_conductivity", "w_m_k": [45.0, 45.0, 45.0], "axes": "material123"})),
            property(json!({"kind": "density", "kg_m3": 7850.0})),
            property(json!({"kind": "specific_heat", "j_kg_k": 460.0})),
            property(json!({"kind": "thermal_expansion", "per_k": [1.2e-5, 1.2e-5, 1.2e-5], "axes": "material123"})),
            property(json!({"kind": "elasticity", "model": {"symmetry": "isotropic", "young_modulus_pa": 2.0e11, "poisson_ratio": 0.3}})),
            property(json!({"kind": "yield_strength", "pa": 2.5e8})),
            property(json!({"kind": "tensile_strength", "pa": 4.0e8})),
        ]),
    );
    let p = read(&c, Alignment::WorldAxes).expect("read");
    assert_eq!(p.designation, "test material");
    assert_eq!(p.thermal.conductivity_w_m_k, [45.0; 3]);
    assert_eq!(p.thermal.density_kg_m3, 7850.0);
    assert_eq!(p.thermal.specific_heat_j_kg_k, 460.0);
    assert_eq!(p.expansion_per_k, Some([1.2e-5; 3]));
    assert_eq!(p.young_modulus_pa, Some(2.0e11));
    assert_eq!(p.poisson_ratio, Some(0.3));
    assert_eq!(p.yield_strength_pa, Some(2.5e8));
    assert_eq!(p.tensile_strength_pa, Some(4.0e8));
    assert!(p.absent.is_empty(), "{:?}", p.absent);
    assert!(!p.illustrative_only);
    // Volumetric capacity is the product, which is what a transient needs.
    assert!((p.thermal.volumetric_capacity() - 7850.0 * 460.0).abs() < 1e-6);
    p.for_transient_conduction().expect("a transient can run on this");
}

#[test]
fn a_card_that_says_nothing_about_heat_is_refused_by_name() {
    let c = card("measurement", json!([property(json!({"kind": "density", "kg_m3": 1200.0}))]));
    let p = read(&c, Alignment::WorldAxes).expect("read");
    assert!(p.absent.contains(&"thermal_conductivity"), "{:?}", p.absent);
    match p.for_steady_conduction() {
        Err(SimError::Material(m)) => {
            assert!(m.contains("thermal_conductivity"), "{m}");
            assert!(m.contains("test material"), "the refusal names the material: {m}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_card_with_no_heat_capacity_can_still_settle_but_not_cool_down() {
    let c = card("measurement", json!([property(json!({"kind": "thermal_conductivity", "w_m_k": [12.0, 12.0, 12.0], "axes": "material123"}))]));
    let p = read(&c, Alignment::WorldAxes).expect("read");
    p.for_steady_conduction().expect("steady is fine without a capacity");
    match p.for_transient_conduction() {
        Err(SimError::Material(m)) => assert!(m.contains("density"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_orthotropic_card_keeps_its_conductivity_along_the_models_axes() {
    let c = card("public_reference", json!([property(json!({"kind": "thermal_conductivity", "w_m_k": [0.4, 0.2, 0.1], "axes": "wood_lrt"}))]));
    let p = read(&c, Alignment::WorldAxes).expect("read");
    assert_eq!(p.thermal.conductivity_w_m_k, [0.4, 0.2, 0.1]);
}

#[test]
fn turning_a_material_by_a_quarter_permutes_its_conductivity() {
    // Axis 1 along the model's y, axis 2 along −x, axis 3 along z: a quarter turn about z.
    let axes = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let c = card("public_reference", json!([property(json!({"kind": "thermal_conductivity", "w_m_k": [0.4, 0.2, 0.1], "axes": "wood_lrt"}))]));
    let p = read(&c, Alignment::Along(axes)).expect("read");
    // The grain now runs along y, so y conducts 0.4 and x conducts 0.2.
    let k = p.thermal.conductivity_w_m_k;
    assert!((k[0] - 0.2).abs() < 1e-12, "{k:?}");
    assert!((k[1] - 0.4).abs() < 1e-12, "{k:?}");
    assert!((k[2] - 0.1).abs() < 1e-12, "{k:?}");
}

#[test]
fn an_isotropic_material_may_be_turned_any_way_at_all() {
    // A rotation about an arbitrary axis: for an isotropic tensor every rotation gives it back,
    // so there are no cross terms and nothing to refuse.
    let (c0, s0) = (0.6, 0.8);
    let axes = [[c0, s0, 0.0], [-s0 * c0, c0 * c0, s0], [s0 * s0, -c0 * s0, c0]];
    let c = card("public_reference", json!([property(json!({"kind": "thermal_conductivity", "w_m_k": [30.0, 30.0, 30.0], "axes": "material123"}))]));
    let p = read(&c, Alignment::Along(axes)).expect("read");
    for k in p.thermal.conductivity_w_m_k {
        assert!((k - 30.0).abs() < 1e-9, "{k}");
    }
}

#[test]
fn an_obliquely_turned_orthotropic_material_is_refused_rather_than_truncated() {
    // Half a quarter turn about z puts real conduction across x and y, which the lattice
    // cannot carry. Dropping the cross terms would quietly understate the heat path.
    let a = std::f64::consts::FRAC_1_SQRT_2;
    let axes = [[a, a, 0.0], [-a, a, 0.0], [0.0, 0.0, 1.0]];
    let c = card("public_reference", json!([property(json!({"kind": "thermal_conductivity", "w_m_k": [0.4, 0.2, 0.1], "axes": "wood_lrt"}))]));
    match read(&c, Alignment::Along(axes)) {
        Err(SimError::Material(m)) => {
            assert!(m.contains("across the model's axes"), "{m}");
            assert!(m.contains("not supported yet"), "an unfinished feature says so: {m}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn axes_that_are_not_a_frame_are_refused() {
    let c = card("public_reference", json!([property(json!({"kind": "thermal_conductivity", "w_m_k": [1.0, 1.0, 1.0], "axes": "material123"}))]));
    // Not unit length.
    let bad = [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    assert!(matches!(read(&c, Alignment::Along(bad)), Err(SimError::Material(_))));
    // Not square to each other.
    let a = std::f64::consts::FRAC_1_SQRT_2;
    let bad = [[1.0, 0.0, 0.0], [a, a, 0.0], [0.0, 0.0, 1.0]];
    assert!(matches!(read(&c, Alignment::Along(bad)), Err(SimError::Material(_))));
    // Left-handed.
    let bad = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
    match read(&c, Alignment::Along(bad)) {
        Err(SimError::Material(m)) => assert!(m.contains("left-handed"), "{m}"),
        other => panic!("{other:?}"),
    }
    // Not numbers.
    let bad = [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    assert!(matches!(read(&c, Alignment::Along(bad)), Err(SimError::Material(_))));
}

#[test]
fn a_card_made_of_illustrative_numbers_says_so() {
    let c = card("illustrative", json!([property(json!({"kind": "thermal_conductivity", "w_m_k": [1.0, 1.0, 1.0], "axes": "material123"}))]));
    let p = read(&c, Alignment::WorldAxes).expect("read");
    assert!(p.illustrative_only, "a result from this is not a measurement of anything");
    // It still solves: the point is that the provenance travels with the answer.
    p.for_steady_conduction().expect("read");
}

#[test]
fn the_temperature_a_property_was_measured_at_travels_with_it() {
    let value = json!({
        "schema_version": 1,
        "designation": "warm steel",
        "family": "metal",
        "sources": [{"id": "s", "title": "a test", "kind": "measurement", "license": null, "reference": null}],
        "properties": [
            {"data": {"kind": "thermal_conductivity", "w_m_k": [40.0, 40.0, 40.0], "axes": "material123"},
             "source_id": "s", "conditions": {"temperature_k": 773.15, "moisture_dry_basis_kg_kg": null,
                                              "strain_rate_per_s": null, "state": null}},
            {"data": {"kind": "density", "kg_m3": 7700.0}, "source_id": "s",
             "conditions": {"temperature_k": 293.15, "moisture_dry_basis_kg_kg": null,
                            "strain_rate_per_s": null, "state": null}}
        ],
    });
    let c = admit_card(&value).expect("a card");
    let p = read(&c, Alignment::WorldAxes).expect("read");
    assert_eq!(
        p.measured_at_kelvin,
        vec![("density".to_string(), 293.15), ("thermal_conductivity".to_string(), 773.15)],
        "a study run at room temperature on a conductivity measured at 500 °C is extrapolating, and a caller can see it"
    );
}

#[test]
fn an_orthotropic_stiffness_is_noted_as_something_no_solver_here_can_use_yet() {
    let c = card(
        "public_reference",
        json!([
            property(json!({"kind": "thermal_conductivity", "w_m_k": [1.0, 1.0, 1.0], "axes": "material123"})),
            property(json!({"kind": "elasticity", "model": {"symmetry": "orthotropic", "axes": "wood_lrt",
                "young_moduli_pa": [1.1e10, 7.0e8, 4.0e8], "shear_moduli_pa": [7.0e8, 2.0e8, 6.0e8],
                "poisson_ratios": [0.37, 0.43, 0.04]}})),
        ]),
    );
    let p = read(&c, Alignment::WorldAxes).expect("read");
    assert_eq!(p.young_modulus_pa, None);
    assert!(p.absent.contains(&"isotropic elasticity"), "{:?}", p.absent);
    // Heat still works: the two are separate questions.
    p.for_steady_conduction().expect("read");
}
