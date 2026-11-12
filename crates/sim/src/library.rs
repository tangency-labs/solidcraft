//! Nominal properties to start an analysis from.
//!
//! A design's materials carry a density and a look, which is all modelling needs. An analysis
//! needs a conductivity, a specific heat and an emissivity as well, and asking someone to type
//! three numbers before they can see where their part gets hot is a poor way to begin. So each
//! of the built-in material names has a nominal set here.
//!
//! **These are starting points, not data.** They are the ordinary room-temperature figures for
//! *a* representative grade -- 6061 rather than aluminium in general, 304 rather than stainless
//! in general -- and real materials vary by grade, temper, direction and temperature, often by
//! a lot: a stainless steel conducts a third of what a mild steel does, and an alloy's
//! conductivity falls as it warms. They are therefore marked illustrative where they come out
//! as a material card, which carries that mark through to the result, so a number worked out
//! from them can be reported for what it is. Anything that matters gets a card of its own.
//!
//! The density is not repeated here. It comes from the document's own material library, so the
//! mass an analysis uses and the mass the model reports can never disagree.

use solidcraft_doc::physical_material::{Conditions, Elasticity, Family, MaterialAxes, MaterialCard, Property, PropertyData, Source, SourceKind};

/// Nominal behaviour of an ordinary sample of a material, at about 20 °C.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NominalSolid {
    /// The name the document's material library uses.
    pub name: &'static str,
    /// Conductivity along the material's own axes 1, 2 and 3, W/(m·K). Equal for anything but
    /// a grain.
    pub conductivity_w_m_k: [f64; 3],
    pub specific_heat_j_kg_k: f64,
    /// Total hemispherical emissivity of an ordinary, un-polished surface. A bright metal
    /// radiates almost nothing; paint, plastic, wood and glass radiate almost everything, and
    /// a painted heatsink rejects far more heat than a bare one for that reason.
    pub emissivity: f64,
    /// Linear expansion, per kelvin, along the same axes.
    pub expansion_per_k: [f64; 3],
    /// Young's modulus, Pa, and Poisson's ratio. `None` for a material whose stiffness is not
    /// isotropic -- wood -- since a stress study here has no way to use it and should say so
    /// rather than treat the material as something it is not.
    pub young_modulus_pa: Option<f64>,
    pub poisson_ratio: f64,
    /// For a material whose stiffness differs along each of its own axes: the three moduli, the
    /// three shear moduli and the three ratios, in the card's order. The card keeps it, which is
    /// what lets a load case refuse the material *by name* rather than merely find nothing.
    pub orthotropic: Option<([f64; 3], [f64; 3], [f64; 3])>,
    /// Where it gives way, Pa. `None` for a brittle material, which has no yield point: it
    /// breaks instead, and the number to compare against is the tensile strength.
    pub yield_strength_pa: Option<f64>,
    pub tensile_strength_pa: f64,
    /// True for a material that breaks rather than yields. Von Mises stress is a yield
    /// criterion, so for these it is the wrong measure and a study says so.
    pub brittle: bool,
    /// Which axes the triples are given in.
    pub axes: MaterialAxes,
    pub family: Family,
}

/// The built-in material names, in the document library's order. `Default` is absent on
/// purpose: it is a stand-in for "no material chosen", and an analysis should say so rather
/// than invent properties for it.
pub const NOMINAL_SOLIDS: &[NominalSolid] = &[
    NominalSolid {
        // A plain carbon steel.
        name: "Steel",
        conductivity_w_m_k: [50.0; 3],
        specific_heat_j_kg_k: 470.0,
        emissivity: 0.8,
        expansion_per_k: [1.2e-5; 3],
        young_modulus_pa: Some(200.0e9),
        poisson_ratio: 0.29,
        orthotropic: None,
        yield_strength_pa: Some(250.0e6),
        tensile_strength_pa: 400.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Metal,
    },
    NominalSolid {
        // Austenitic, of the 304 kind: conducts a third of what a mild steel does.
        name: "Stainless Steel",
        conductivity_w_m_k: [16.0; 3],
        specific_heat_j_kg_k: 500.0,
        emissivity: 0.35,
        expansion_per_k: [1.73e-5; 3],
        young_modulus_pa: Some(193.0e9),
        poisson_ratio: 0.29,
        orthotropic: None,
        yield_strength_pa: Some(215.0e6),
        tensile_strength_pa: 505.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Metal,
    },
    NominalSolid {
        // 6061 in the T6 temper.
        name: "Aluminum",
        conductivity_w_m_k: [167.0; 3],
        specific_heat_j_kg_k: 896.0,
        emissivity: 0.1,
        expansion_per_k: [2.34e-5; 3],
        young_modulus_pa: Some(68.9e9),
        poisson_ratio: 0.33,
        orthotropic: None,
        yield_strength_pa: Some(276.0e6),
        tensile_strength_pa: 310.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Metal,
    },
    NominalSolid {
        name: "Brass",
        conductivity_w_m_k: [120.0; 3],
        specific_heat_j_kg_k: 380.0,
        emissivity: 0.1,
        expansion_per_k: [1.9e-5; 3],
        young_modulus_pa: Some(110.0e9),
        poisson_ratio: 0.34,
        orthotropic: None,
        yield_strength_pa: Some(200.0e6),
        tensile_strength_pa: 400.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Metal,
    },
    NominalSolid {
        // Annealed: soft, and the figure a yield is quoted at for it varies more than most.
        name: "Copper",
        conductivity_w_m_k: [398.0; 3],
        specific_heat_j_kg_k: 385.0,
        emissivity: 0.07,
        expansion_per_k: [1.68e-5; 3],
        young_modulus_pa: Some(117.0e9),
        poisson_ratio: 0.34,
        orthotropic: None,
        yield_strength_pa: Some(70.0e6),
        tensile_strength_pa: 220.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Metal,
    },
    NominalSolid {
        // The library's 4.43 g/cm³ is the Ti-6Al-4V alloy, which conducts far less than pure
        // titanium -- about a third of a stainless steel -- and is far stronger.
        name: "Titanium",
        conductivity_w_m_k: [6.7; 3],
        specific_heat_j_kg_k: 526.0,
        emissivity: 0.3,
        expansion_per_k: [8.6e-6; 3],
        young_modulus_pa: Some(113.8e9),
        poisson_ratio: 0.342,
        orthotropic: None,
        yield_strength_pa: Some(880.0e6),
        tensile_strength_pa: 950.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Metal,
    },
    NominalSolid {
        // Grey iron: it breaks rather than yields, and is far weaker in tension than in
        // compression, so a von Mises safety factor is the wrong measure for it.
        name: "Cast Iron",
        conductivity_w_m_k: [52.0; 3],
        specific_heat_j_kg_k: 460.0,
        emissivity: 0.7,
        expansion_per_k: [1.05e-5; 3],
        young_modulus_pa: Some(100.0e9),
        poisson_ratio: 0.26,
        orthotropic: None,
        yield_strength_pa: None,
        tensile_strength_pa: 200.0e6,
        brittle: true,
        axes: MaterialAxes::Material123,
        family: Family::Metal,
    },
    NominalSolid {
        name: "ABS Plastic",
        conductivity_w_m_k: [0.17; 3],
        specific_heat_j_kg_k: 1400.0,
        emissivity: 0.9,
        expansion_per_k: [9.0e-5; 3],
        young_modulus_pa: Some(2.3e9),
        poisson_ratio: 0.35,
        orthotropic: None,
        yield_strength_pa: Some(40.0e6),
        tensile_strength_pa: 40.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Polymer,
    },
    NominalSolid {
        // Printed parts are weaker than this and weaker still across the layers; the figure is
        // for the bulk material.
        name: "PLA",
        conductivity_w_m_k: [0.13; 3],
        specific_heat_j_kg_k: 1800.0,
        emissivity: 0.9,
        expansion_per_k: [7.0e-5; 3],
        young_modulus_pa: Some(3.5e9),
        poisson_ratio: 0.36,
        orthotropic: None,
        yield_strength_pa: None,
        tensile_strength_pa: 50.0e6,
        brittle: true,
        axes: MaterialAxes::Material123,
        family: Family::Polymer,
    },
    NominalSolid {
        // Dry. Nylon takes up water and softens a long way when it does.
        name: "Nylon",
        conductivity_w_m_k: [0.25; 3],
        specific_heat_j_kg_k: 1700.0,
        emissivity: 0.9,
        expansion_per_k: [8.0e-5; 3],
        young_modulus_pa: Some(2.9e9),
        poisson_ratio: 0.39,
        orthotropic: None,
        yield_strength_pa: Some(70.0e6),
        tensile_strength_pa: 80.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Polymer,
    },
    NominalSolid {
        name: "Polycarbonate",
        conductivity_w_m_k: [0.2; 3],
        specific_heat_j_kg_k: 1200.0,
        emissivity: 0.9,
        expansion_per_k: [6.5e-5; 3],
        young_modulus_pa: Some(2.3e9),
        poisson_ratio: 0.37,
        orthotropic: None,
        yield_strength_pa: Some(62.0e6),
        tensile_strength_pa: 65.0e6,
        brittle: false,
        axes: MaterialAxes::Material123,
        family: Family::Polymer,
    },
    NominalSolid {
        // Wood is the one built-in that is properly orthotropic: along the grain it conducts
        // about twice what it does across, it moves many times more across than along, and it
        // is some twenty times stiffer along than across. That last one is why it has no
        // isotropic modulus here -- there is no single number that would not be a lie -- and a
        // stress study on it is refused rather than answered wrongly.
        name: "Oak",
        conductivity_w_m_k: [0.17, 0.11, 0.10],
        specific_heat_j_kg_k: 2000.0,
        emissivity: 0.9,
        expansion_per_k: [4.9e-6, 3.0e-5, 5.4e-5],
        young_modulus_pa: None,
        poisson_ratio: 0.37,
        // Along the grain, then across the rings, then round them. Twenty times stiffer along
        // than across, which is the whole reason there is no single modulus for it.
        orthotropic: Some(([11.0e9, 1.1e9, 0.57e9], [0.93e9, 0.27e9, 0.8e9], [0.35, 0.67, 0.043])),
        yield_strength_pa: None,
        tensile_strength_pa: 100.0e6,
        brittle: true,
        axes: MaterialAxes::WoodLrt,
        family: Family::Wood,
    },
    NominalSolid {
        // Soda-lime. Its tensile strength depends almost entirely on the flaws in its surface,
        // so the figure is nominal in a stronger sense than the rest of this table.
        name: "Glass",
        conductivity_w_m_k: [1.0; 3],
        specific_heat_j_kg_k: 840.0,
        emissivity: 0.9,
        expansion_per_k: [9.0e-6; 3],
        young_modulus_pa: Some(70.0e9),
        poisson_ratio: 0.22,
        orthotropic: None,
        yield_strength_pa: None,
        tensile_strength_pa: 50.0e6,
        brittle: true,
        axes: MaterialAxes::Material123,
        family: Family::Glass,
    },
];

/// The nominal set for a material name, ignoring case.
pub fn nominal(name: &str) -> Option<&'static NominalSolid> {
    NOMINAL_SOLIDS.iter().find(|m| m.name.eq_ignore_ascii_case(name.trim()))
}

/// The nominal set as a material card, with the density the document gives.
///
/// Everything an analysis runs on comes through a card, so the built-in materials take the same
/// road as somebody's own data and get the same reading, the same checks and the same mark on
/// the result.
pub fn nominal_card(name: &str, density_kg_m3: f64) -> Option<MaterialCard> {
    let m = nominal(name)?;
    let source = Source {
        id: "nominal".into(),
        title: "SolidCraft nominal properties: ordinary room-temperature figures for a representative grade".into(),
        kind: SourceKind::Illustrative,
        license: None,
        reference: None,
    };
    // Every number here was taken at about room temperature, and saying so is what lets a
    // study run at 200 °C report that it is extrapolating.
    let at_room = Conditions { temperature_k: Some(293.15), ..Conditions::default() };
    let property = |data: PropertyData| Property { data, source_id: "nominal".into(), conditions: at_room.clone() };
    Some(MaterialCard {
        schema_version: 1,
        designation: m.name.into(),
        family: m.family,
        sources: vec![source],
        properties: {
            let mut all = vec![
                property(PropertyData::Density { kg_m3: density_kg_m3 }),
                property(PropertyData::ThermalConductivity { w_m_k: m.conductivity_w_m_k, axes: m.axes }),
                property(PropertyData::SpecificHeat { j_kg_k: m.specific_heat_j_kg_k }),
                property(PropertyData::ThermalExpansion { per_k: m.expansion_per_k, axes: m.axes }),
                property(PropertyData::TensileStrength { pa: m.tensile_strength_pa }),
            ];
            if let Some(e) = m.young_modulus_pa {
                all.push(property(PropertyData::Elasticity { model: Elasticity::Isotropic { young_modulus_pa: e, poisson_ratio: m.poisson_ratio } }));
            } else if let Some((young_moduli_pa, shear_moduli_pa, poisson_ratios)) = m.orthotropic {
                all.push(property(PropertyData::Elasticity {
                    model: Elasticity::Orthotropic { axes: m.axes, young_moduli_pa, shear_moduli_pa, poisson_ratios },
                }));
            }
            if let Some(y) = m.yield_strength_pa {
                all.push(property(PropertyData::YieldStrength { pa: y }));
            }
            all
        },
    })
}

/// A fluid a flow runs in, or a film carries heat through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fluid {
    pub name: &'static str,
    pub density_kg_m3: f64,
    /// Dynamic viscosity μ, Pa·s.
    pub dynamic_viscosity_pa_s: f64,
    pub conductivity_w_m_k: f64,
    pub specific_heat_j_kg_k: f64,
    /// The temperature these were taken at.
    pub at_kelvin: f64,
}

impl Fluid {
    /// Kinematic viscosity ν = μ/ρ, m²/s. This is the number a flow solver wants: it is what
    /// sets the Reynolds number and, on a lattice, the relaxation time.
    pub fn kinematic_viscosity_m2_s(&self) -> f64 {
        if self.density_kg_m3 > 0.0 { self.dynamic_viscosity_pa_s / self.density_kg_m3 } else { 0.0 }
    }

    /// Prandtl number Pr = μ·c/k: how momentum diffuses compared with heat. Near one for a
    /// gas, several for water, which is why a liquid's thermal boundary layer is thin.
    pub fn prandtl(&self) -> f64 {
        if self.conductivity_w_m_k > 0.0 { self.dynamic_viscosity_pa_s * self.specific_heat_j_kg_k / self.conductivity_w_m_k } else { 0.0 }
    }

    /// Reynolds number for a speed and a length: inertia against viscosity, and the single
    /// number that says whether a flow will be orderly or turbulent.
    pub fn reynolds(&self, speed_m_s: f64, length_m: f64) -> f64 {
        let nu = self.kinematic_viscosity_m2_s();
        if nu > 0.0 { speed_m_s.abs() * length_m.abs() / nu } else { f64::INFINITY }
    }

    /// Thermal diffusivity α = k/(ρ·c), m²/s.
    pub fn diffusivity_m2_s(&self) -> f64 {
        let capacity = self.density_kg_m3 * self.specific_heat_j_kg_k;
        if capacity > 0.0 { self.conductivity_w_m_k / capacity } else { 0.0 }
    }
}

/// Dry air at 20 °C and one atmosphere.
pub const AIR: Fluid = Fluid {
    name: "Air",
    density_kg_m3: 1.204,
    dynamic_viscosity_pa_s: 1.825e-5,
    conductivity_w_m_k: 0.02514,
    specific_heat_j_kg_k: 1007.0,
    at_kelvin: 293.15,
};

/// Water at 20 °C.
pub const WATER: Fluid = Fluid {
    name: "Water",
    density_kg_m3: 998.2,
    dynamic_viscosity_pa_s: 1.002e-3,
    conductivity_w_m_k: 0.598,
    specific_heat_j_kg_k: 4182.0,
    at_kelvin: 293.15,
};

pub const FLUIDS: &[Fluid] = &[AIR, WATER];

/// A fluid by name, ignoring case.
pub fn fluid(name: &str) -> Option<&'static Fluid> {
    FLUIDS.iter().find(|f| f.name.eq_ignore_ascii_case(name.trim()))
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
