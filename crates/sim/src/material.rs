//! Reading a material card into the numbers a solver needs.
//!
//! A card ([`solidcraft_doc::physical_material::MaterialCard`]) is candidate engineering data
//! with its provenance attached: where each number came from, and what it was measured at. It
//! is deliberately not a solver input -- it may be incomplete, it may be marked illustrative,
//! and its anisotropic properties are given in the material's own axes rather than the model's.
//! This module is the one place that turns one into coefficients, and it refuses rather than
//! guesses: a study that needs a conductivity the card does not carry is told which property is
//! missing, by name.
//!
//! What a card says about where it came from is carried through to the result, so a number
//! computed from an illustrative value can be reported as one.

use solidcraft_doc::physical_material::{Elasticity, MaterialCard, PropertyData, SourceKind};

use crate::elastic::ElasticMaterial;
use crate::thermal::SolidMaterial;
use crate::{Result, SimError};

/// How a material's own axes lie in the model.
///
/// A card's conductivity and expansion are given along the material's axes 1, 2, 3 (or a wood's
/// longitudinal, radial and tangential), which say nothing about how the board was cut or how
/// the part is turned. Someone has to say, and it is not this module's place to assume.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alignment {
    /// The material's axes are the model's x, y and z.
    WorldAxes,
    /// The material's axes 1, 2 and 3 lie along these three directions in the model. They are
    /// expected to be a right-handed orthonormal set; anything else is refused.
    Along([[f64; 3]; 3]),
}

/// Relative size an off-axis conduction term has to stay under to be ignored.
///
/// Rotating an isotropic material gives exactly zero off-axis terms, and so does any rotation
/// by a quarter turn, so the ordinary cases pass this cleanly. A genuinely oblique orthotropic
/// material does not, and is refused rather than quietly having its cross terms dropped.
const CROSS_AXIS_LIMIT: f64 = 1e-9;

/// What a card could be read as, and what it could not.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidProperties {
    /// What a conduction study needs. Zero conductivity where the card does not say.
    pub thermal: SolidMaterial,
    /// The card's designation, to report a result under.
    pub designation: String,
    /// Linear expansion along the model's axes, per kelvin, where the card says.
    pub expansion_per_k: Option<[f64; 3]>,
    /// Young's modulus and Poisson's ratio, for an isotropic card.
    pub young_modulus_pa: Option<f64>,
    pub poisson_ratio: Option<f64>,
    /// Where the material gives way, in pascals.
    pub yield_strength_pa: Option<f64>,
    pub tensile_strength_pa: Option<f64>,
    /// The temperature each property the card dates was measured at, if it says. A study run
    /// far from these is extrapolating, and a caller can say so.
    pub measured_at_kelvin: Vec<(String, f64)>,
    /// Property kinds the card does not carry at all.
    pub absent: Vec<&'static str>,
    /// True when every source the card draws on is marked illustrative, so no number in it is
    /// anybody's measurement.
    pub illustrative_only: bool,
}

impl SolidProperties {
    /// What a steady conduction study needs: a conductivity, and nothing else.
    pub fn for_steady_conduction(&self) -> Result<SolidMaterial> {
        if !self.thermal.conductivity_w_m_k.iter().any(|k| *k > 0.0) {
            return Err(missing(&self.designation, "thermal_conductivity", "conduct heat"));
        }
        Ok(self.thermal)
    }

    /// What a transient conduction study needs: a conductivity, and something to store heat in.
    pub fn for_transient_conduction(&self) -> Result<SolidMaterial> {
        let m = self.for_steady_conduction()?;
        if !(m.density_kg_m3 > 0.0) {
            return Err(missing(&self.designation, "density", "hold heat over time"));
        }
        if !(m.specific_heat_j_kg_k > 0.0) {
            return Err(missing(&self.designation, "specific_heat", "hold heat over time"));
        }
        Ok(m)
    }

    /// What a load case needs: a stiffness, and whatever else the card happens to carry.
    ///
    /// A card with an orthotropic stiffness and no isotropic one is refused rather than having
    /// one direction's modulus used as though it held in every direction. Wood is twenty times
    /// stiffer along the grain than across it; there is no single number for it that would not
    /// be a lie.
    pub fn for_stress(&self) -> Result<ElasticMaterial> {
        let Some(young_modulus_pa) = self.young_modulus_pa else {
            if self.absent.contains(&"isotropic elasticity") {
                return Err(SimError::Material(format!(
                    "{}'s stiffness is orthotropic -- different along each of its own axes -- and a load case here can only \
                     use an isotropic one, so there is no honest way to run this: it is not supported yet",
                    self.designation
                )));
            }
            return Err(missing(&self.designation, "elasticity", "stretch or bend"));
        };
        let Some(poisson_ratio) = self.poisson_ratio else {
            return Err(missing(&self.designation, "elasticity", "stretch or bend"));
        };
        Ok(ElasticMaterial {
            young_modulus_pa,
            poisson_ratio,
            expansion_per_k: self.expansion_per_k.unwrap_or([0.0; 3]),
            density_kg_m3: self.thermal.density_kg_m3,
            // Yield is what von Mises is measured against. A card that gives only a tensile
            // strength is a brittle material's card, and von Mises is a yield criterion rather
            // than a fracture one, so that strength is deliberately not substituted here --
            // the caller is told there is no yield point instead.
            yield_strength_pa: self.yield_strength_pa,
        })
    }
}

fn missing(designation: &str, property: &str, what: &str) -> SimError {
    SimError::Material(format!(
        "{designation} has no {property} on its card, so there is nothing to say how it would {what}; add the property to the \
         material or pick a material that carries it"
    ))
}

/// Read a card into coefficients along the model's axes.
pub fn read(card: &MaterialCard, alignment: Alignment) -> Result<SolidProperties> {
    let rotation = match alignment {
        Alignment::WorldAxes => None,
        Alignment::Along(axes) => Some(orthonormal(axes)?),
    };
    let illustrative = !card.sources.is_empty() && card.sources.iter().all(|s| s.kind == SourceKind::Illustrative);
    let mut out = SolidProperties {
        thermal: SolidMaterial { conductivity_w_m_k: [0.0; 3], density_kg_m3: 0.0, specific_heat_j_kg_k: 0.0 },
        designation: card.designation.clone(),
        expansion_per_k: None,
        young_modulus_pa: None,
        poisson_ratio: None,
        yield_strength_pa: None,
        tensile_strength_pa: None,
        measured_at_kelvin: Vec::new(),
        absent: Vec::new(),
        illustrative_only: illustrative,
    };

    for property in &card.properties {
        if let Some(t) = property.conditions.temperature_k {
            out.measured_at_kelvin.push((property.data.kind().to_string(), t));
        }
        match &property.data {
            PropertyData::Density { kg_m3 } => out.thermal.density_kg_m3 = *kg_m3,
            PropertyData::SpecificHeat { j_kg_k } => out.thermal.specific_heat_j_kg_k = *j_kg_k,
            PropertyData::ThermalConductivity { w_m_k, .. } => {
                out.thermal.conductivity_w_m_k = project(*w_m_k, rotation.as_ref(), "thermal conductivity", &card.designation)?;
            }
            PropertyData::ThermalExpansion { per_k, .. } => {
                out.expansion_per_k = Some(project(*per_k, rotation.as_ref(), "thermal expansion", &card.designation)?);
            }
            PropertyData::Elasticity { model } => match model {
                Elasticity::Isotropic { young_modulus_pa, poisson_ratio } => {
                    out.young_modulus_pa = Some(*young_modulus_pa);
                    out.poisson_ratio = Some(*poisson_ratio);
                }
                // An orthotropic stiffness is a six-by-six matrix in the material's axes, which
                // nothing here can use yet; the card keeps it, and a stress study will say so
                // rather than pretend the material is isotropic.
                Elasticity::Orthotropic { .. } => out.absent.push("isotropic elasticity"),
            },
            PropertyData::YieldStrength { pa } => out.yield_strength_pa = Some(*pa),
            PropertyData::TensileStrength { pa } => out.tensile_strength_pa = Some(*pa),
            PropertyData::CompressiveStrength { .. } | PropertyData::ShearStrength { .. } => {}
        }
    }

    for (absent, name) in [
        (!out.thermal.conductivity_w_m_k.iter().any(|k| *k > 0.0), "thermal_conductivity"),
        (!(out.thermal.density_kg_m3 > 0.0), "density"),
        (!(out.thermal.specific_heat_j_kg_k > 0.0), "specific_heat"),
        (out.expansion_per_k.is_none(), "thermal_expansion"),
        (out.young_modulus_pa.is_none(), "elasticity"),
        (out.yield_strength_pa.is_none(), "yield_strength"),
    ] {
        if absent {
            out.absent.push(name);
        }
    }
    out.measured_at_kelvin.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Turn three directions into a rotation whose columns they are, refusing anything that is not
/// a right-handed orthonormal set -- a tensor rotated by anything else is not the same tensor.
fn orthonormal(axes: [[f64; 3]; 3]) -> Result<[[f64; 3]; 3]> {
    let axis = |i: usize| axes.get(i).copied().unwrap_or([0.0; 3]);
    for i in 0..3 {
        let a = axis(i);
        if !a.iter().all(|v| v.is_finite()) {
            return Err(SimError::Material("a material axis that is not a direction".into()));
        }
        let length = dot(a, a).sqrt();
        if (length - 1.0).abs() > 1e-6 {
            return Err(SimError::Material(format!("material axis {} is {length} long, and an axis must be a unit vector", i + 1)));
        }
        for j in (i + 1)..3 {
            let d = dot(a, axis(j));
            if d.abs() > 1e-6 {
                return Err(SimError::Material(format!(
                    "material axes {} and {} are not square to each other (their dot product is {d})",
                    i + 1,
                    j + 1
                )));
            }
        }
    }
    let [a, b, c] = axes;
    let handed = dot(cross(a, b), c);
    if handed < 0.0 {
        return Err(SimError::Material("the material axes are left-handed; axis 3 must be axis 1 crossed into axis 2".into()));
    }
    Ok(axes)
}

/// The three numbers a property has along the model's axes.
///
/// With no rotation they are the card's own. With one, the property is a tensor
/// `K = R·diag(k)·Rᵀ`, and only its diagonal has a meaning for a lattice square to the model's
/// axes; if the off-diagonal terms are not negligible, conduction runs across axes and this
/// refuses rather than dropping them.
fn project(values: [f64; 3], rotation: Option<&[[f64; 3]; 3]>, what: &str, designation: &str) -> Result<[f64; 3]> {
    let Some(r) = rotation else { return Ok(values) };
    let mut tensor = [[0.0f64; 3]; 3];
    for (axis, k) in values.iter().enumerate() {
        let column = r.get(axis).copied().unwrap_or([0.0; 3]);
        for i in 0..3 {
            for j in 0..3 {
                let (Some(ci), Some(cj)) = (column.get(i), column.get(j)) else { continue };
                if let Some(slot) = tensor.get_mut(i).and_then(|row| row.get_mut(j)) {
                    *slot += k * ci * cj;
                }
            }
        }
    }
    let diagonal = [tensor[0][0], tensor[1][1], tensor[2][2]];
    let scale = diagonal.iter().fold(0.0f64, |a, b| a.max(b.abs()));
    let cross_terms = [tensor[0][1], tensor[1][2], tensor[2][0]].iter().fold(0.0f64, |a, b| a.max(b.abs()));
    if scale > 0.0 && cross_terms > scale * CROSS_AXIS_LIMIT {
        return Err(SimError::Material(format!(
            "{designation}'s {what} runs across the model's axes once the material is turned this way, which is not supported \
             yet: line the material's axes up with the model's, or give it an isotropic {what}"
        )));
    }
    Ok(diagonal)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| x * y).sum()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[cfg(test)]
#[path = "material_tests.rs"]
mod tests;
