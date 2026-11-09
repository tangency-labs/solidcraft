//! Candidate engineering data, separate from assigned density and visual appearance.
//! Validation establishes numerical consistency, not certification or solver support.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::{DocError, Result};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialCard {
    pub schema_version: u32,
    pub designation: String,
    pub family: Family,
    pub sources: Vec<Source>,
    pub properties: Vec<Property>,
}

/// Design-owned candidate data. Private storage prevents bypassing admission;
/// validation does not certify sources, applicability or analysis capability.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ValidatedCard(MaterialCard);

impl ValidatedCard {
    pub fn from_value(value: &Value) -> Result<Self> {
        let card = admit_card(value)?;
        card.validate()?;
        Ok(Self(card))
    }

    pub fn card(&self) -> &MaterialCard {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ValidatedCard {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Wood,
    Polymer,
    Glass,
    Metal,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub kind: SourceKind,
    /// A caller's claim, never an approval to redistribute or proof of authenticity.
    pub license: Option<String>,
    pub reference: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    UserSupplied,
    Measurement,
    PublicReference,
    Illustrative,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Property {
    pub data: PropertyData,
    pub source_id: String,
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    pub temperature_k: Option<f64>,
    /// kg water / kg dry material; unlike wet-basis fraction, may exceed one.
    pub moisture_dry_basis_kg_kg: Option<f64>,
    pub strain_rate_per_s: Option<f64>,
    pub state: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialAxes {
    Material123,
    WoodLrt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropertyData {
    Density { kg_m3: f64 },
    Elasticity { model: Elasticity },
    TensileStrength { pa: f64 },
    CompressiveStrength { pa: f64 },
    YieldStrength { pa: f64 },
    ShearStrength { pa: f64 },
    ThermalConductivity { w_m_k: [f64; 3], axes: MaterialAxes },
    ThermalExpansion { per_k: [f64; 3], axes: MaterialAxes },
    SpecificHeat { j_kg_k: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "symmetry", rename_all = "snake_case", deny_unknown_fields)]
pub enum Elasticity {
    Isotropic {
        young_modulus_pa: f64,
        poisson_ratio: f64,
    },
    /// E1,E2,E3; G12,G23,G31; nu12,nu23,nu31, with nu_ij = -eps_j/eps_i
    /// under uniaxial stress in i. Reciprocal ratios follow nu_ij/Ei = nu_ji/Ej.
    Orthotropic {
        axes: MaterialAxes,
        young_moduli_pa: [f64; 3],
        shear_moduli_pa: [f64; 3],
        poisson_ratios: [f64; 3],
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IsotropicModuli {
    pub shear_pa: f64,
    pub bulk_pa: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Validation {
    pub schema_version: u32,
    pub property_kinds: Vec<String>,
    pub isotropic_moduli: Option<IsotropicModuli>,
    pub unspecified_conditions: Vec<String>,
    /// Property kind -> absent condition fields under the existing family policy.
    /// Empty when reading an older report that did not include this detail.
    #[serde(default)]
    pub missing_condition_fields: BTreeMap<String, Vec<String>>,
    pub source_review_required: Vec<String>,
    pub analysis_solver_available: bool,
    pub rendering_properties_available: bool,
}

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}
fn text(value: &str, limit: usize, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(invalid(format!("{field}: expected nonempty text of at most {limit} UTF-8 bytes without controls")));
    }
    Ok(())
}
fn number(value: f64, low: f64, high: f64, field: &str) -> Result<()> {
    if !value.is_finite() || value < low || value > high {
        return Err(invalid(format!("{field}: expected a finite value in [{low}, {high}]")));
    }
    Ok(())
}
fn positive(value: f64, high: f64, field: &str) -> Result<()> {
    number(value, 0., high, field)?;
    if value == 0. {
        return Err(invalid(format!("{field}: must be positive")));
    }
    Ok(())
}

impl Elasticity {
    pub fn validate(&self) -> Result<Option<IsotropicModuli>> {
        match self {
            Self::Isotropic { young_modulus_pa: e, poisson_ratio: nu } => {
                positive(*e, 1e15, "young_modulus_pa")?;
                if !e.recip().is_finite() {
                    return Err(invalid("young_modulus_pa produces unrepresentable compliance"));
                }
                if !nu.is_finite() || *nu <= -1. || *nu >= 0.5 {
                    return Err(invalid("poisson_ratio: isotropic stability requires -1 < nu < 0.5"));
                }
                let result = IsotropicModuli { shear_pa: e / (2. * (1. + nu)), bulk_pa: e / (3. * (1. - 2. * nu)) };
                if !result.shear_pa.is_finite()
                    || !result.bulk_pa.is_finite()
                    || result.shear_pa <= 0.
                    || result.bulk_pa <= 0.
                    || !result.shear_pa.recip().is_finite()
                    || !result.bulk_pa.recip().is_finite()
                {
                    return Err(invalid("elastic moduli exceed numeric limits"));
                }
                Ok(Some(result))
            }
            Self::Orthotropic { young_moduli_pa: e, shear_moduli_pa: g, poisson_ratios: nu, .. } => {
                for value in e.iter().chain(g) {
                    positive(*value, 1e15, "orthotropic modulus_pa")?;
                    if !value.recip().is_finite() {
                        return Err(invalid("orthotropic modulus produces unrepresentable compliance"));
                    }
                }
                for value in nu {
                    number(*value, -1e6, 1e6, "orthotropic poisson_ratio")?;
                }
                // Diagonal scaling of the reciprocal, symmetric normal compliance
                // gives unit diagonal and these off-diagonal entries. Sylvester's
                // criterion proves positive definiteness without dimensional cutoffs.
                let [e1, e2, e3] = *e;
                let [n12, n23, n31] = *nu;
                let [a, b, c] = [-n12 * (e2.sqrt() / e1.sqrt()), -n23 * (e3.sqrt() / e2.sqrt()), -n31 * (e1.sqrt() / e3.sqrt())];
                let determinant = 1. + 2. * a * b * c - a * a - b * b - c * c;
                if ![a, b, c, determinant].iter().all(|v| v.is_finite()) || 1. - a * a <= 0. || determinant <= 0. {
                    return Err(invalid("orthotropic compliance must be finite and positive definite"));
                }
                Ok(None)
            }
        }
    }
}

impl PropertyData {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Density { .. } => "density",
            Self::Elasticity { .. } => "elasticity",
            Self::TensileStrength { .. } => "tensile_strength",
            Self::CompressiveStrength { .. } => "compressive_strength",
            Self::YieldStrength { .. } => "yield_strength",
            Self::ShearStrength { .. } => "shear_strength",
            Self::ThermalConductivity { .. } => "thermal_conductivity",
            Self::ThermalExpansion { .. } => "thermal_expansion",
            Self::SpecificHeat { .. } => "specific_heat",
        }
    }
    fn validate(&self) -> Result<Option<IsotropicModuli>> {
        match self {
            Self::Density { kg_m3 } => positive(*kg_m3, 1e6, "density.kg_m3")?,
            Self::Elasticity { model } => return model.validate(),
            Self::TensileStrength { pa } | Self::CompressiveStrength { pa } | Self::YieldStrength { pa } | Self::ShearStrength { pa } => {
                positive(*pa, 1e15, self.kind())?
            }
            Self::ThermalConductivity { w_m_k, .. } => {
                for k in w_m_k {
                    positive(*k, 1e9, "thermal_conductivity.w_m_k")?;
                }
            }
            Self::ThermalExpansion { per_k, .. } => {
                for a in per_k {
                    number(*a, -1., 1., "thermal_expansion.per_k")?;
                }
            }
            Self::SpecificHeat { j_kg_k } => positive(*j_kg_k, 1e9, "specific_heat.j_kg_k")?,
        }
        Ok(None)
    }
}

impl MaterialCard {
    pub fn validate(&self) -> Result<Validation> {
        if self.schema_version != 1 {
            return Err(invalid("unsupported material card schema_version (expected 1)"));
        }
        text(&self.designation, 256, "designation")?;
        if self.sources.len() > 32 || self.properties.len() > 16 {
            return Err(invalid("material card exceeds 32 sources or 16 properties"));
        }
        let mut sources = BTreeSet::new();
        for source in &self.sources {
            text(&source.id, 128, "source.id")?;
            text(&source.title, 256, "source.title")?;
            if let Some(value) = &source.license {
                text(value, 512, "source.license")?;
            }
            if let Some(value) = &source.reference {
                text(value, 2048, "source.reference")?;
            }
            if !sources.insert(source.id.clone()) {
                return Err(invalid("duplicate source id"));
            }
        }
        let mut kinds = BTreeSet::new();
        let mut used_sources = BTreeSet::new();
        let mut unspecified = Vec::new();
        let mut missing_condition_fields = BTreeMap::new();
        let mut isotropic_moduli = None;
        for property in &self.properties {
            let kind = property.data.kind();
            if !kinds.insert(kind.to_string()) {
                return Err(invalid(format!("duplicate property kind: {kind}")));
            }
            if !sources.contains(&property.source_id) {
                return Err(invalid(format!("{kind}: unknown source_id")));
            }
            used_sources.insert(property.source_id.clone());
            let conditions = &property.conditions;
            if let Some(t) = conditions.temperature_k {
                positive(t, 20000., "temperature_k")?;
            }
            if let Some(m) = conditions.moisture_dry_basis_kg_kg {
                number(m, 0., 1e6, "moisture_dry_basis_kg_kg")?;
            }
            if let Some(r) = conditions.strain_rate_per_s {
                positive(r, 1e12, "strain_rate_per_s")?;
            }
            if let Some(state) = &conditions.state {
                text(state, 1024, "conditions.state")?;
            }
            let mut missing = Vec::new();
            for (absent, field) in [
                (conditions.temperature_k.is_none(), "temperature_k"),
                (conditions.state.is_none(), "state"),
                (self.family == Family::Wood && conditions.moisture_dry_basis_kg_kg.is_none(), "moisture_dry_basis_kg_kg"),
                (self.family == Family::Polymer && conditions.strain_rate_per_s.is_none(), "strain_rate_per_s"),
            ] {
                if absent {
                    missing.push(field.to_string());
                }
            }
            if !missing.is_empty() {
                unspecified.push(kind.to_string());
                missing_condition_fields.insert(kind.to_string(), missing);
            }
            if let Some(moduli) = property.data.validate()? {
                isotropic_moduli = Some(moduli);
            }
        }
        Ok(Validation {
            schema_version: 1,
            property_kinds: kinds.into_iter().collect(),
            isotropic_moduli,
            unspecified_conditions: unspecified,
            missing_condition_fields,
            source_review_required: used_sources.into_iter().collect(),
            analysis_solver_available: false,
            rendering_properties_available: false,
        })
    }
}

/// Bound JSON traversal before cloning/deserializing it. Metadata and numeric
/// bounds are admission limits, not claims about a real material's valid range.
pub fn validate_card(value: &Value) -> Result<Validation> {
    admit_card(value)?.validate()
}

/// Bound and deserialize a candidate before cloning it. Semantic checks belong
/// to `validate` or the derived query; this alone does not validate the numbers.
pub fn admit_card(value: &Value) -> Result<MaterialCard> {
    let mut pending = vec![(value, 0usize)];
    let (mut nodes, mut bytes) = (0usize, 0usize);
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        bytes += 32;
        if nodes > 2048 || depth > 12 {
            return Err(invalid("material card exceeds JSON node/depth budget"));
        }
        match value {
            Value::String(s) => {
                if s.len() > 256 * 1024 {
                    return Err(invalid("material card string exceeds admission budget"));
                }
                bytes += s.len();
            }
            Value::Array(a) => {
                if a.len() > 32 {
                    return Err(invalid("material card array exceeds 32 entries"));
                }
                pending.extend(a.iter().map(|v| (v, depth + 1)));
            }
            Value::Object(o) => {
                if o.len() > 32 {
                    return Err(invalid("material card object exceeds 32 fields"));
                }
                for (key, v) in o {
                    if key.len() > 256 * 1024 {
                        return Err(invalid("material card key exceeds admission budget"));
                    }
                    bytes += key.len();
                    pending.push((v, depth + 1));
                }
            }
            _ => {}
        }
        if bytes > 256 * 1024 {
            return Err(invalid("material card exceeds 256 KiB admission budget"));
        }
    }
    let card: MaterialCard = serde_json::from_value(value.clone()).map_err(|e| invalid(format!("material card: {e}")))?;
    Ok(card)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DirectionalModulus {
    pub direction: [f64; 3],
    pub axes: Option<MaterialAxes>,
    pub young_modulus_pa: f64,
    pub validation: Validation,
}

impl MaterialCard {
    /// Uniaxial small-strain modulus in the supplied material-local direction.
    /// This is a constitutive query, not a body load case or structural solver.
    pub fn directional_modulus(&self, direction: [f64; 3]) -> Result<DirectionalModulus> {
        let validation = self.validate()?;
        if !direction.iter().all(|x| x.is_finite()) {
            return Err(invalid("direction: expected three finite components"));
        }
        let scale = direction.iter().fold(0.0_f64, |a, x| a.max(x.abs()));
        if scale == 0. {
            return Err(invalid("direction: must be nonzero"));
        }
        let scaled = direction.map(|x| x / scale);
        let length = scaled.iter().map(|x| x * x).sum::<f64>().sqrt();
        let direction = scaled.map(|x| x / length);
        let model = self
            .properties
            .iter()
            .find_map(|p| match &p.data {
                PropertyData::Elasticity { model } => Some(model),
                _ => None,
            })
            .ok_or_else(|| invalid("elasticity is unspecified"))?;
        let (axes, modulus) = match model {
            Elasticity::Isotropic { young_modulus_pa, .. } => (None, *young_modulus_pa),
            Elasticity::Orthotropic { axes, young_moduli_pa, shear_moduli_pa, poisson_ratios } => {
                let [x, y, z] = direction;
                let [e1, e2, e3] = *young_moduli_pa;
                let [g12, g23, g31] = *shear_moduli_pa;
                let [n12, n23, n31] = *poisson_ratios;
                let [a, b, c] = [-n12 * (e2.sqrt() / e1.sqrt()), -n23 * (e3.sqrt() / e2.sqrt()), -n31 * (e1.sqrt() / e3.sqrt())];
                let minor = 1. - a * a;
                let determinant = 1. + 2. * a * b * c - a * a - b * b - c * c;
                // Cholesky of the scaled normal compliance: evaluate the energy
                // as sums of squares instead of cancelling positive/negative terms.
                let l22 = minor.sqrt();
                let l32 = (b - a * c) / l22;
                let l33 = (determinant / minor).sqrt();
                let [u, v, w] = [x * x / e1.sqrt(), y * y / e2.sqrt(), z * z / e3.sqrt()];
                let terms = [u + a * v + c * w, l22 * v + l32 * w, l33 * w];
                let compliance = terms.iter().map(|t| t * t).sum::<f64>() + (x * y).powi(2) / g12 + (y * z).powi(2) / g23 + (z * x).powi(2) / g31;
                if !compliance.is_finite() || compliance <= 0. {
                    return Err(invalid("directional compliance is unrepresentable"));
                }
                (Some(*axes), compliance.recip())
            }
        };
        if !modulus.is_finite() || modulus <= 0. {
            return Err(invalid("directional modulus is unrepresentable"));
        }
        Ok(DirectionalModulus { direction, axes, young_modulus_pa: modulus, validation })
    }
}

#[cfg(test)]
#[path = "physical_material_tests.rs"]
mod tests;
