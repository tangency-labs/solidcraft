//! From SolidCraft's document and kernel to the snapshot's types: the one place both meet.

mod body;
mod features;

pub(crate) use body::body;
pub(crate) use features::features;

use solidcraft_doc::Document;
use solidcraft_doc::expr::{Kind, unit_info};
use solidcraft_doc::physical_material::{self as pm, PropertyData, ValidatedCard};

use crate::schema::{
    Conditions, Elasticity, Material, MaterialAxes, MaterialName, MaterialProperty, Mm, Parameter, ParameterName, ParameterSource, ParameterValue,
    Property, Quantity, Radians,
};

/// What is left of the snapshot's face, triangle and control point limits.
pub(crate) struct Budget {
    pub(crate) faces: usize,
    pub(crate) triangles: usize,
    pub(crate) control_points: usize,
}

/// A value of `kind` in its canonical unit (mm, radians) as a quantity.
pub(crate) fn quantity(kind: Kind, v: f64) -> Quantity {
    match kind {
        Kind::Length => Quantity::Length(Mm(v)),
        Kind::Angle => Quantity::Angle(Radians(v)),
        Kind::Unitless => Quantity::Number(v),
    }
}

pub(crate) fn parameters(doc: &Document) -> Vec<Parameter> {
    doc.param_rows()
        .into_iter()
        .map(|r| {
            let value = if let Some(text) = r.text {
                ParameterValue::Text(text)
            } else if solidcraft_doc::text_params::is_text_unit(&r.unit) {
                ParameterValue::Text(r.expression.clone())
            } else {
                // The table's value is in the unit it shows; the snapshot's in mm or radians.
                match (r.value.filter(|v| v.is_finite()), unit_info(&r.unit)) {
                    (Some(v), Some((kind, scale))) => ParameterValue::Quantity(quantity(kind, v * scale)),
                    _ => ParameterValue::Invalid,
                }
            };
            let source = match r.source {
                "user" => ParameterSource::User,
                "sketch" => ParameterSource::Sketch,
                "feature" => ParameterSource::Feature,
                _ => ParameterSource::Other,
            };
            Parameter { name: ParameterName(r.name), expression: r.expression, value, display_unit: r.unit, source, comment: r.comment }
        })
        .collect()
}

/// A material by name: its density and, for a design material with a card, its engineering data.
pub(crate) fn material(doc: &Document, name: &str) -> Material {
    // g/cm³ to kg/m³.
    let density_kg_m3 = doc.material_density(name).map(|d| d * 1000.0);
    let properties = doc.user_material(name).and_then(|m| m.physical_card.as_ref()).map(card_properties).unwrap_or_default();
    Material { name: MaterialName(name.to_string()), density_kg_m3, properties }
}

fn axes(a: pm::MaterialAxes) -> MaterialAxes {
    match a {
        pm::MaterialAxes::Material123 => MaterialAxes::Material123,
        pm::MaterialAxes::WoodLrt => MaterialAxes::WoodLrt,
    }
}

fn card_properties(card: &ValidatedCard) -> Vec<MaterialProperty> {
    card.card()
        .properties
        .iter()
        .map(|p| {
            let property = match &p.data {
                PropertyData::Density { kg_m3 } => Property::Density { kg_m3: *kg_m3 },
                PropertyData::Elasticity { model: pm::Elasticity::Isotropic { young_modulus_pa, poisson_ratio } } => {
                    Property::Elasticity { elasticity: Elasticity::Isotropic { young_modulus_pa: *young_modulus_pa, poisson_ratio: *poisson_ratio } }
                }
                PropertyData::Elasticity { model: pm::Elasticity::Orthotropic { axes: a, young_moduli_pa, shear_moduli_pa, poisson_ratios } } => {
                    Property::Elasticity {
                        elasticity: Elasticity::Orthotropic {
                            axes: axes(*a),
                            young_moduli_pa: *young_moduli_pa,
                            shear_moduli_pa: *shear_moduli_pa,
                            poisson_ratios: *poisson_ratios,
                        },
                    }
                }
                PropertyData::TensileStrength { pa } => Property::TensileStrength { pa: *pa },
                PropertyData::CompressiveStrength { pa } => Property::CompressiveStrength { pa: *pa },
                PropertyData::YieldStrength { pa } => Property::YieldStrength { pa: *pa },
                PropertyData::ShearStrength { pa } => Property::ShearStrength { pa: *pa },
                PropertyData::ThermalConductivity { w_m_k, axes: a } => Property::ThermalConductivity { w_m_k: *w_m_k, axes: axes(*a) },
                PropertyData::ThermalExpansion { per_k, axes: a } => Property::ThermalExpansion { per_k: *per_k, axes: axes(*a) },
                PropertyData::SpecificHeat { j_kg_k } => Property::SpecificHeat { j_kg_k: *j_kg_k },
            };
            let c = &p.conditions;
            let conditions = Conditions {
                temperature_k: c.temperature_k,
                moisture_dry_basis_kg_kg: c.moisture_dry_basis_kg_kg,
                strain_rate_per_s: c.strain_rate_per_s,
                state: c.state.clone(),
            };
            MaterialProperty { property, conditions, source: p.source_id.clone() }
        })
        .collect()
}
