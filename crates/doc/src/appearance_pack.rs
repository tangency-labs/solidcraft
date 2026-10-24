//! Original visual authoring choices; never engineering material grades or a catalog codec.
use serde::Serialize;

use crate::{DocError, Result, appearance::Look, pbr_appearance::PbrParameters};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Wood,
    Metal,
    Plastic,
    Glass,
    Solar,
    Regolith,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    OpaquePbr,
    Refraction,
    CertifiedThinOrClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureAxis {
    V,
}

/// Planned original procedural resources, not a loaded texture or a saved binding.
/// Albedo is sRGB; roughness and positive-Y tangent normals are linear data.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TextureRecipe {
    pub id: &'static str,
    pub repeat_mm: [f64; 2],
    pub grain_axis: Option<TextureAxis>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Provenance {
    pub author: &'static str,
    pub license: &'static str,
    pub source: &'static str,
    pub kind: &'static str,
}
pub const ORIGINAL: Provenance = Provenance {
    author: "SolidCraft contributors",
    license: "MIT OR Apache-2.0",
    source: "assets/appearances/starter_pack.rs",
    kind: "original illustrative optical authoring; not measured material data",
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct OpaqueRecipe {
    pub color: [u8; 3],
    pub metallic: f64,
    pub roughness: f64,
    pub dielectric_ior: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum GlassBoundary {
    Thin { thickness_mm: f64 },
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct GlassRecipe {
    pub dielectric_ior: f64,
    /// Linear transmittance over reference_path_mm, not sRGB tint or alpha.
    pub transmission_linear_rgb: [f64; 3],
    pub reference_path_mm: f64,
    pub boundary: GlassBoundary,
}
impl GlassRecipe {
    pub fn validate(&self) -> Result<()> {
        if !self.dielectric_ior.is_finite()
            || !(1.0..=3.0).contains(&self.dielectric_ior)
            || self.transmission_linear_rgb.iter().any(|v| !v.is_finite() || *v <= 0.0 || *v > 1.0)
            || !self.reference_path_mm.is_finite()
            || self.reference_path_mm <= 0.0
            || self.reference_path_mm > 1e6
            || matches!(self.boundary,GlassBoundary::Thin {thickness_mm} if !thickness_mm.is_finite() || thickness_mm<=0.0 || thickness_mm>10000.0)
        {
            return Err(DocError::Invalid(
                "invalid illustrative glass recipe: finite IOR1..3, linear transmission0<T<=1, bounded positive reference path/thickness required"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recipe {
    Opaque(OpaqueRecipe),
    Glass(GlassRecipe),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct VisualPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub revision: u32,
    pub provenance: Provenance,
    pub requirements: &'static [Requirement],
    pub recipe: Recipe,
    pub preview_note: &'static str,
    pub future_texture_recipe: Option<TextureRecipe>,
}

impl VisualPreset {
    pub const fn opaque(
        id: &'static str,
        name: &'static str,
        category: Category,
        recipe: OpaqueRecipe,
        preview_note: &'static str,
        future_texture_recipe: Option<TextureRecipe>,
    ) -> Self {
        Self {
            id,
            name,
            category,
            revision: 1,
            provenance: ORIGINAL,
            requirements: &[Requirement::OpaquePbr],
            recipe: Recipe::Opaque(recipe),
            preview_note,
            future_texture_recipe,
        }
    }
    pub const fn glass(id: &'static str, name: &'static str, recipe: GlassRecipe) -> Self {
        Self {
            id,
            name,
            category: Category::Glass,
            revision: 1,
            provenance: ORIGINAL,
            requirements: &[Requirement::Refraction, Requirement::CertifiedThinOrClosed],
            recipe: Recipe::Glass(recipe),
            preview_note: "Recipe only: requires actual refraction and a certified thin/closed geometry path; no alpha substitute",
            future_texture_recipe: None,
        }
    }
    pub fn is_opaque(&self) -> bool {
        matches!(self.recipe, Recipe::Opaque(_))
    }
    pub fn preview_color(&self) -> [u8; 3] {
        match self.recipe {
            Recipe::Opaque(r) => r.color,
            // A UI swatch of the transmission values, not rendered glass or absorption.
            Recipe::Glass(r) => r.transmission_linear_rgb.map(|v| {
                let encoded = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
                (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
            }),
        }
    }

    /// All opaque presets share the existing finite PBR constructor and owned Look.
    /// Refraction recipes cannot be smuggled into a scalar/alpha appearance.
    pub fn opaque_look(&self) -> Result<Look> {
        let Recipe::Opaque(recipe) = self.recipe else {
            return Err(DocError::Invalid(
                "glass recipe unavailable: actual refraction and certified thin/closed geometry are required; alpha transparency is not a substitute"
                    .into(),
            ));
        };
        let pbr = PbrParameters::new(recipe.metallic, recipe.roughness, recipe.dielectric_ior).map_err(|e| DocError::Invalid(e.to_string()))?;
        Ok(Look { name: self.name.into(), color: recipe.color, opacity: 1.0, pbr: Some(pbr) })
    }
}

#[path = "../../../assets/appearances/starter_pack.rs"]
mod data;
pub use data::STARTER_PACK;

/// Stable IDs are preferred for commands; display-name lookup is unambiguous.
pub fn find(id_or_name: &str) -> Option<&'static VisualPreset> {
    let key = id_or_name.trim();
    STARTER_PACK.iter().find(|p| p.id.eq_ignore_ascii_case(key) || p.name.eq_ignore_ascii_case(key))
}
