//! Physical materials and their engineering properties, each in the SI unit its field names.

use serde::{Deserialize, Serialize};

use super::ids::MaterialName;

/// A physical material.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Material {
    pub name: MaterialName,
    /// Mass density, the one SolidCraft computes mass with (none when it has none).
    pub density_kg_m3: Option<f64>,
    /// Engineering data the design holds for the material (a design material's card), as the
    /// user entered it: candidate values, not certified ones.
    pub properties: Vec<MaterialProperty>,
}

/// One engineering property, the conditions it holds at, and where it comes from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct MaterialProperty {
    pub property: Property,
    pub conditions: Conditions,
    /// The card's source id.
    pub source: String,
}

/// An engineering property.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Property {
    Density {
        kg_m3: f64,
    },
    Elasticity {
        elasticity: Elasticity,
    },
    TensileStrength {
        pa: f64,
    },
    CompressiveStrength {
        pa: f64,
    },
    YieldStrength {
        pa: f64,
    },
    ShearStrength {
        pa: f64,
    },
    /// Along the material axes.
    ThermalConductivity {
        w_m_k: [f64; 3],
        axes: MaterialAxes,
    },
    /// Along the material axes.
    ThermalExpansion {
        per_k: [f64; 3],
        axes: MaterialAxes,
    },
    SpecificHeat {
        j_kg_k: f64,
    },
    /// A kind this version does not know.
    #[default]
    #[serde(other)]
    Other,
}

/// How stiffness depends on direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "symmetry", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Elasticity {
    Isotropic {
        young_modulus_pa: f64,
        poisson_ratio: f64,
    },
    Orthotropic {
        axes: MaterialAxes,
        young_moduli_pa: [f64; 3],
        shear_moduli_pa: [f64; 3],
        poisson_ratios: [f64; 3],
    },
    /// A kind this version does not know.
    #[serde(other)]
    Other,
}

/// The axes a direction-dependent property is given along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MaterialAxes {
    /// The material's 1, 2, 3 axes.
    #[default]
    Material123,
    /// Wood's longitudinal, radial and tangential axes.
    WoodLrt,
    /// A kind this version does not know.
    #[serde(other)]
    Other,
}

/// The conditions a property was measured at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Conditions {
    pub temperature_k: Option<f64>,
    pub moisture_dry_basis_kg_kg: Option<f64>,
    pub strain_rate_per_s: Option<f64>,
    /// The material's state (`annealed`, `T6`, …) as the card gives it.
    pub state: Option<String>,
}
