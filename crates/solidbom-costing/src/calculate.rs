//! Pure, deterministic direct-cost arithmetic for the SolidBOM prototype.
//!
//! Only the supported direct-cost lines are calculated: material, material overhead and
//! production, with setup kept as a separate charge. A missing input leaves its affected amounts
//! `None` and makes the result `Incomplete`; it never becomes zero.
//!
//! The calculator is server-side only. The desktop never calls it.

use rust_decimal::Decimal;
use solidbom_contract::{CalculationInputs, CalculationOutcome, CostLine, Issue, PartCost, ResolvedPartInput, ResultState};

/// One million, from mm³ and grams to kilograms: 1 cm³ = 1000 mm³, 1 kg = 1000 g.
const MM3_TIMES_GRAMS_PER_KG: u32 = 1_000_000;
const SECONDS_PER_HOUR: u32 = 3_600;
const PROTOTYPE_VERSION: &str = "solidbom-direct-cost/1";

/// The calculator version recorded in an immutable snapshot.
pub fn calculator_version() -> &'static str {
    PROTOTYPE_VERSION
}

/// Calculate the prototype's direct-cost subtotal for resolved inputs.
///
/// Unsupported or invalid inputs produce named issues and an `Incomplete` result. The function
/// never panics: arithmetic overflow is an issue, not a crash.
pub fn calculate(input: &CalculationInputs) -> CalculationOutcome {
    let mut issues = Vec::new();
    if input.currency.trim().is_empty() {
        issues.push(Issue::new("invalid_currency", "currency", "the project has no currency"));
    }
    if input.material_overhead_rate.is_sign_negative() {
        issues.push(Issue::new("invalid_overhead_rate", "material_overhead_rate", "the material overhead rate cannot be negative"));
    }
    let overhead_ok = !input.material_overhead_rate.is_sign_negative();
    if input.parts.is_empty() {
        issues.push(Issue::new("no_parts", "parts", "there is no reviewed part to calculate"));
    }

    let mut parts = Vec::with_capacity(input.parts.len());
    let mut assembly: Option<Decimal> = Some(Decimal::ZERO);
    let mut setup_total: Option<Decimal> = Some(Decimal::ZERO);
    let mut every_part_complete = true;

    for (index, part) in input.parts.iter().enumerate() {
        let computed = part_cost(index, part, input.material_overhead_rate, overhead_ok);
        if !computed.issues.is_empty() {
            issues.extend(computed.issues);
        }
        every_part_complete &= computed.complete;
        assembly = match (assembly, computed.cost.line_total) {
            (Some(total), Some(line)) => total.checked_add(line),
            _ => None,
        };
        setup_total = match (setup_total, computed.setup) {
            (Some(total), Some(charge)) => total.checked_add(charge),
            _ => None,
        };
        parts.push(computed.cost);
    }

    let complete = issues.is_empty() && every_part_complete && assembly.is_some() && setup_total.is_some();
    CalculationOutcome {
        state: if complete { ResultState::Complete } else { ResultState::Incomplete },
        parts,
        assembly_subtotal: assembly,
        setup_total,
        issues,
    }
}

struct Computed {
    cost: PartCost,
    setup: Option<Decimal>,
    issues: Vec<Issue>,
    complete: bool,
}

fn part_cost(index: usize, part: &ResolvedPartInput, overhead_rate: Decimal, overhead_ok: bool) -> Computed {
    let path = |field: &str| format!("parts[{index}].{field}");
    let mut issues = Vec::new();
    let issue = |code: &str, field: &str, message: &str| Issue::new(code, path(field), message).for_definition(part.part_version_id);

    if part.quantity == 0 {
        issues.push(issue("invalid_quantity", "quantity", "a part must occur at least once"));
    }

    let volume = match part.volume_mm3 {
        Some(value) if value.is_sign_negative() => {
            issues.push(issue("invalid_volume", "volume_mm3", "a reviewed volume cannot be negative"));
            None
        }
        Some(value) => Some(value),
        None => {
            issues.push(issue("missing_volume", "volume_mm3", "the part has no reviewed volume; an open surface needs a manual value"));
            None
        }
    };

    let density = match part.density_g_cm3 {
        Some(value) if value <= Decimal::ZERO => {
            issues.push(issue("invalid_density", "density_g_cm3", "the material density must be positive"));
            None
        }
        Some(value) => Some(value),
        None => {
            issues.push(issue("missing_density", "density_g_cm3", "no reviewed material density was selected"));
            None
        }
    };

    let price = match part.material_price_per_kg {
        Some(value) if value.is_sign_negative() => {
            issues.push(issue("invalid_material_price", "material_price_per_kg", "a material price cannot be negative"));
            None
        }
        Some(value) => Some(value),
        None => {
            issues.push(issue("missing_material_price", "material_price_per_kg", "the selected material version has no price"));
            None
        }
    };

    let allowance = match part.allowance_rate {
        Some(value) if value.is_sign_negative() => {
            issues.push(issue("invalid_allowance_rate", "allowance_rate", "a material allowance cannot be negative"));
            None
        }
        Some(value) => Some(value),
        None => {
            issues.push(issue("missing_allowance_rate", "allowance_rate", "no material allowance was entered"));
            None
        }
    };

    let cycle = match part.cycle_seconds {
        Some(value) if value.is_sign_negative() => {
            issues.push(issue("invalid_cycle_time", "cycle_seconds", "a cycle time cannot be negative"));
            None
        }
        Some(value) => Some(value),
        None => {
            issues.push(issue("missing_cycle_time", "cycle_seconds", "no production cycle time was entered"));
            None
        }
    };

    let cavities = match part.cavities {
        Some(0) => {
            issues.push(issue("invalid_cavities", "cavities", "at least one cavity is needed"));
            None
        }
        Some(value) => Some(value),
        None if cycle == Some(Decimal::ZERO) => Some(1),
        None => {
            issues.push(issue("missing_cavities", "cavities", "no cavity count was entered"));
            None
        }
    };

    let production_rate = match part.production_rate_per_hour {
        Some(value) if value <= Decimal::ZERO => {
            issues.push(issue("invalid_production_rate", "production_rate_per_hour", "the production rate must be positive"));
            None
        }
        Some(value) => Some(value),
        None if cycle == Some(Decimal::ZERO) => None,
        None => {
            issues.push(issue("missing_production_rate", "production_rate_per_hour", "no production rate version was selected"));
            None
        }
    };

    let setup = match part.setup_hours {
        Some(hours) if hours.is_sign_negative() => {
            issues.push(issue("invalid_setup_hours", "setup_hours", "setup time cannot be negative"));
            None
        }
        Some(hours) if hours > Decimal::ZERO => match part.setup_rate_per_hour {
            Some(rate) if rate <= Decimal::ZERO => {
                issues.push(issue("invalid_setup_rate", "setup_rate_per_hour", "the setup rate must be positive"));
                None
            }
            Some(rate) => checked_mul(hours, rate),
            None => {
                issues.push(issue("missing_setup_rate", "setup_rate_per_hour", "no setup rate version was selected"));
                None
            }
        },
        Some(_) => Some(Decimal::ZERO),
        None => {
            issues.push(issue("missing_setup_hours", "setup_hours", "no setup duration was entered"));
            None
        }
    };

    let net_mass_kg = match (volume, density) {
        (Some(volume), Some(density)) => checked_mul(volume, density)
            .and_then(|grams| checked_div(grams, Decimal::from(MM3_TIMES_GRAMS_PER_KG)))
            .or_else(|| overflow(&mut issues, &issue, "net mass")),
        _ => None,
    };
    let allowed_material_cost = match (net_mass_kg, price, allowance) {
        (Some(mass), Some(price), Some(allowance)) => checked_mul(mass, price)
            .and_then(|net| checked_add(Decimal::ONE, allowance).and_then(|factor| checked_mul(net, factor)))
            .or_else(|| overflow(&mut issues, &issue, "material cost")),
        _ => None,
    };
    let material_overhead = match (allowed_material_cost, overhead_ok) {
        (Some(material), true) => checked_mul(material, overhead_rate).or_else(|| overflow(&mut issues, &issue, "material overhead")),
        _ => None,
    };
    let production_cost = match (cycle, cavities, production_rate) {
        // A cycle time is for the tool, so a multi-cavity tool makes a piece faster.
        (Some(cycle), Some(cavities), Some(rate)) => checked_div(cycle, Decimal::from(cavities))
            .and_then(|per_piece| checked_mul(per_piece, rate))
            .and_then(|scaled| checked_div(scaled, Decimal::from(SECONDS_PER_HOUR)))
            .or_else(|| overflow(&mut issues, &issue, "production cost")),
        (Some(cycle), Some(_), None) if cycle == Decimal::ZERO => Some(Decimal::ZERO),
        _ => None,
    };
    let per_part_subtotal = match (allowed_material_cost, material_overhead, production_cost) {
        (Some(material), Some(overhead), Some(production)) => checked_add(material, overhead)
            .and_then(|partial| checked_add(partial, production))
            .or_else(|| overflow(&mut issues, &issue, "the direct-cost subtotal")),
        _ => None,
    };
    let line_total = match per_part_subtotal {
        Some(subtotal) => checked_mul(subtotal, Decimal::from(part.quantity)).or_else(|| overflow(&mut issues, &issue, "the quantity total")),
        None => None,
    };

    let explanations = vec![
        line("material.mass_kg", "Net mass", net_mass_kg, "volume × density"),
        line("material.allowed_cost", "Allowed material cost", allowed_material_cost, "mass × price × (1 + allowance)"),
        line("material.overhead", "Material overhead", material_overhead, "material cost × overhead rate"),
        line("production.cost", "Production cost", production_cost, "cycle seconds ÷ cavities × hourly rate ÷ 3600"),
        line("part.subtotal", "Direct-cost subtotal per part", per_part_subtotal, "material + overhead + production"),
        line("part.line_total", "Direct-cost subtotal for the quantity", line_total, "per-part subtotal × quantity"),
        line("setup.charge", "Setup charge (separate)", setup, "setup hours × setup rate"),
    ];

    let complete = issues.is_empty()
        && part.quantity > 0
        && volume.is_some()
        && density.is_some()
        && price.is_some()
        && allowance.is_some()
        && cycle.is_some()
        && cavities.is_some()
        && (production_rate.is_some() || cycle == Some(Decimal::ZERO))
        && setup.is_some()
        && per_part_subtotal.is_some()
        && line_total.is_some();

    let cost = PartCost {
        part_version_id: part.part_version_id,
        quantity: part.quantity,
        net_mass_kg,
        allowed_material_cost,
        material_overhead,
        production_cost,
        per_part_subtotal,
        line_total,
        explanations,
    };
    Computed { cost, setup, issues, complete }
}

fn overflow(issues: &mut Vec<Issue>, issue: &impl Fn(&str, &str, &str) -> Issue, line: &str) -> Option<Decimal> {
    issues.push(issue("arithmetic_overflow", "arithmetic", &format!("{line} exceeds the supported numeric range")));
    None
}

fn line(code: &str, label: &str, amount: Option<Decimal>, basis: &str) -> CostLine {
    CostLine { code: code.to_string(), label: label.to_string(), amount, basis: basis.to_string() }
}

fn checked_mul(a: Decimal, b: Decimal) -> Option<Decimal> {
    a.checked_mul(b)
}

fn checked_add(a: Decimal, b: Decimal) -> Option<Decimal> {
    a.checked_add(b)
}

fn checked_div(a: Decimal, b: Decimal) -> Option<Decimal> {
    if b == Decimal::ZERO { None } else { a.checked_div(b) }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
