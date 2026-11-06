//! The approved prototype fixture and its edge cases, in exact decimals.

use rust_decimal::Decimal;
use solidbom_contract::{CalculationInputs, ResolvedPartInput, ResultState};
use uuid::Uuid;

use super::calculate;

fn d(text: &str) -> Decimal {
    text.parse().expect("a decimal in the fixture")
}

fn fixture() -> CalculationInputs {
    CalculationInputs {
        currency: "CNY".into(),
        material_overhead_rate: d("0.05"),
        parts: vec![ResolvedPartInput {
            part_version_id: Uuid::nil(),
            quantity: 3,
            volume_mm3: Some(d("2400")),
            density_g_cm3: Some(d("2.7")),
            material_price_per_kg: Some(d("20")),
            allowance_rate: Some(d("0.02")),
            cycle_seconds: Some(d("30")),
            cavities: Some(1),
            production_rate_per_hour: Some(d("120")),
            setup_hours: Some(d("0.5")),
            setup_rate_per_hour: Some(d("120")),
        }],
    }
}

fn codes(input: &CalculationInputs) -> Vec<String> {
    calculate(input).issues.into_iter().map(|issue| issue.code).collect()
}

fn has(input: &CalculationInputs, code: &str) -> bool {
    codes(input).iter().any(|found| found == code)
}

#[test]
fn fixture_cost() {
    let outcome = calculate(&fixture());
    assert_eq!(outcome.state, ResultState::Complete);
    assert!(outcome.issues.is_empty(), "{:?}", outcome.issues);
    let part = &outcome.parts[0];
    assert_eq!(part.net_mass_kg, Some(d("0.00648")));
    assert_eq!(part.allowed_material_cost, Some(d("0.132192")));
    assert_eq!(part.material_overhead, Some(d("0.0066096")));
    assert_eq!(part.production_cost, Some(d("1")));
    assert_eq!(part.per_part_subtotal, Some(d("1.1388016")));
    assert_eq!(part.line_total, Some(d("3.4164048")));
    assert_eq!(outcome.assembly_subtotal, Some(d("3.4164048")));
    assert_eq!(outcome.setup_total, Some(d("60")));
    assert!(part.explanations.len() >= 6);
}

#[test]
fn repeat_quantity() {
    let outcome = calculate(&fixture());
    let part = &outcome.parts[0];
    assert_eq!(part.quantity, 3);
    assert_eq!(part.line_total, part.per_part_subtotal.map(|subtotal| subtotal * Decimal::from(3)));
    assert_eq!(outcome.assembly_subtotal, Some(d("3.4164048")));
}

#[test]
fn sixty_second_cycle() {
    let mut input = fixture();
    input.parts[0].cycle_seconds = Some(d("60"));
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Complete);
    assert_eq!(outcome.parts[0].production_cost, Some(d("2")));
    assert_eq!(outcome.parts[0].per_part_subtotal, Some(d("2.1388016")));
    assert_eq!(outcome.assembly_subtotal, Some(d("6.4164048")));
}

#[test]
fn allowance_once() {
    let mut without = fixture();
    without.parts[0].allowance_rate = Some(Decimal::ZERO);
    let with = calculate(&fixture());
    let without = calculate(&without);
    let with_part = &with.parts[0];
    let without_part = &without.parts[0];
    assert_eq!(with_part.production_cost, without_part.production_cost);
    assert_eq!(with_part.allowed_material_cost, without_part.allowed_material_cost.map(|cost| cost * d("1.02")));
    assert_eq!(with_part.material_overhead, without_part.material_overhead.map(|cost| cost * d("1.02")));
}

#[test]
fn setup_once() {
    let mut one = fixture();
    one.parts[0].quantity = 1;
    let one = calculate(&one);
    let three = calculate(&fixture());
    assert_eq!(one.setup_total, Some(d("60")));
    assert_eq!(three.setup_total, Some(d("60")));
    assert_eq!(one.parts[0].per_part_subtotal, three.parts[0].per_part_subtotal);
    assert_eq!(one.parts[0].per_part_subtotal, Some(d("1.1388016")));
}

#[test]
fn cavities_divide_the_cycle() {
    let mut input = fixture();
    input.parts[0].cavities = Some(2);
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Complete);
    assert_eq!(outcome.parts[0].production_cost, Some(d("0.5")));
    assert_eq!(outcome.parts[0].per_part_subtotal, Some(d("0.6388016")));
    assert_eq!(outcome.assembly_subtotal, Some(d("1.9164048")));
}

#[test]
fn missing_density() {
    let mut input = fixture();
    input.parts[0].density_g_cm3 = None;
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Incomplete);
    assert_eq!(outcome.parts[0].net_mass_kg, None);
    assert_eq!(outcome.parts[0].per_part_subtotal, None);
    assert_eq!(outcome.assembly_subtotal, None);
    assert!(has(&input, "missing_density"));
    assert!(!has(&input, "arithmetic_overflow"));
}

#[test]
fn missing_cycle_time_is_incomplete() {
    let mut input = fixture();
    input.parts[0].cycle_seconds = None;
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Incomplete);
    assert_eq!(outcome.parts[0].production_cost, None);
    assert!(has(&input, "missing_cycle_time"));
}

#[test]
fn zero_cycle_is_valid_and_gives_zero_production() {
    let mut input = fixture();
    input.parts[0].cycle_seconds = Some(Decimal::ZERO);
    input.parts[0].cavities = None;
    input.parts[0].production_rate_per_hour = None;
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Complete);
    assert_eq!(outcome.parts[0].production_cost, Some(Decimal::ZERO));
    assert_eq!(outcome.parts[0].per_part_subtotal, Some(d("0.1388016")));
    assert_eq!(outcome.assembly_subtotal, Some(d("0.4164048")));
}

#[test]
fn open_surface_volume_is_missing_never_zero() {
    let mut input = fixture();
    input.parts[0].volume_mm3 = None;
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Incomplete);
    assert_eq!(outcome.parts[0].allowed_material_cost, None);
    assert!(has(&input, "missing_volume"));
}

#[test]
fn omitted_setup_is_missing_not_zero() {
    let mut input = fixture();
    input.parts[0].setup_hours = None;
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Incomplete);
    assert_eq!(outcome.setup_total, None);
    assert_eq!(outcome.parts[0].per_part_subtotal, Some(d("1.1388016")));
    assert!(has(&input, "missing_setup_hours"));
}

#[test]
fn explicit_zero_setup_is_allowed() {
    let mut input = fixture();
    input.parts[0].setup_hours = Some(Decimal::ZERO);
    input.parts[0].setup_rate_per_hour = None;
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Complete);
    assert_eq!(outcome.setup_total, Some(Decimal::ZERO));
}

#[test]
fn invalid_inputs_are_refused() {
    let mut negative_cycle = fixture();
    negative_cycle.parts[0].cycle_seconds = Some(d("-1"));
    assert!(has(&negative_cycle, "invalid_cycle_time"));

    let mut zero_density = fixture();
    zero_density.parts[0].density_g_cm3 = Some(Decimal::ZERO);
    assert!(has(&zero_density, "invalid_density"));

    let mut zero_rate = fixture();
    zero_rate.parts[0].production_rate_per_hour = Some(Decimal::ZERO);
    assert!(has(&zero_rate, "invalid_production_rate"));

    let mut zero_cavities = fixture();
    zero_cavities.parts[0].cavities = Some(0);
    assert!(has(&zero_cavities, "invalid_cavities"));

    let mut negative_allowance = fixture();
    negative_allowance.parts[0].allowance_rate = Some(d("-0.01"));
    assert!(has(&negative_allowance, "invalid_allowance_rate"));

    for input in [negative_cycle, zero_density, zero_rate, zero_cavities, negative_allowance] {
        assert_eq!(calculate(&input).state, ResultState::Incomplete);
    }
}

#[test]
fn an_empty_currency_or_part_list_is_incomplete() {
    let mut input = fixture();
    input.currency = String::new();
    input.parts.clear();
    let outcome = calculate(&input);
    assert_eq!(outcome.state, ResultState::Incomplete);
    assert!(has(&input, "invalid_currency"));
    assert!(has(&input, "no_parts"));
}
