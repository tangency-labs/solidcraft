// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion Design object, sketch, identity, and construction records.
//!
//! These functions read Design `MetaStream.dat` and `BulkStream.dat` entries
//! selected by [`crate::container`]. Returned records retain source offsets and
//! stable identifiers for native regeneration.

pub(crate) mod assembly;
pub(crate) mod body;
pub(crate) mod components;
pub(crate) mod configurations;
pub(crate) mod constraints;
pub(crate) mod decode;
pub(crate) mod dimensions;
pub(crate) mod edge_resolve;
pub(crate) mod face_resolve;
pub(crate) mod feature_project;
pub(crate) mod geometry;
pub(crate) mod owners;
pub(crate) mod presentation;
pub(crate) mod profile_select;
mod relation_kinds;
pub(crate) mod sketch_project;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
mod text;

use crate::records::recipes::ConstructionRecipeKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesignFeatureFamily {
    Sketch,
    Assemble,
    Extrude,
    Fillet,
    Chamfer,
    Combine,
    Draft,
    ReplaceFace,
    CircularPattern,
    RectangularPattern,
    Mirror,
    Move,
    OffsetFaces,
    Revolve,
    Shell,
    Thicken,
    Coil,
    Loft,
    Sweep,
    Pipe,
    SurfacePatch,
    SurfaceExtend,
    SurfaceOffset,
    SurfaceRuled,
    SurfaceTrim,
    BoundaryFill,
    Hole,
    Split,
    Scale,
    Thread,
    SheetMetalEdgeFlange,
    SheetMetalHem,
}

/// Return the canonical operation family while preserving `kind` verbatim on
/// the native scope. Fusion serializes this field through its UI localization.
pub(crate) fn design_feature_family(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> Option<DesignFeatureFamily> {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    match kind {
        Kind::Sketch
        | Kind::Esquisse
        | Kind::Skizze
        | Kind::Esboco
        | Kind::Boceto
        | Kind::SketchJa => Some(DesignFeatureFamily::Sketch),
        Kind::Assemble | Kind::AsBuilt => Some(DesignFeatureFamily::Assemble),
        Kind::Extrude | Kind::Extrusion | Kind::Extrusao | Kind::Extruir | Kind::ExtrudeJa => {
            Some(DesignFeatureFamily::Extrude)
        }
        Kind::Fillet
        | Kind::Conge
        | Kind::Abrundung
        | Kind::Arredondamento
        | Kind::Empalme
        | Kind::FilletJa => Some(DesignFeatureFamily::Fillet),
        Kind::Chamfer | Kind::Chanfrein | Kind::Chaflan | Kind::ChamferJa => {
            Some(DesignFeatureFamily::Chamfer)
        }
        Kind::Combine => Some(DesignFeatureFamily::Combine),
        Kind::Draft => Some(DesignFeatureFamily::Draft),
        Kind::ReplaceFace => Some(DesignFeatureFamily::ReplaceFace),
        Kind::CPattern | Kind::CircularPattern | Kind::ReseauC => {
            Some(DesignFeatureFamily::CircularPattern)
        }
        Kind::RPattern | Kind::RectangularPattern => Some(DesignFeatureFamily::RectangularPattern),
        Kind::Mirror | Kind::SymetrieMiroir => Some(DesignFeatureFamily::Mirror),
        Kind::Move => Some(DesignFeatureFamily::Move),
        Kind::OffsetFaces | Kind::DecalerLesFaces => Some(DesignFeatureFamily::OffsetFaces),
        Kind::Revolve => Some(DesignFeatureFamily::Revolve),
        Kind::Shell | Kind::Schale | Kind::Vaciado => Some(DesignFeatureFamily::Shell),
        Kind::Thicken => Some(DesignFeatureFamily::Thicken),
        Kind::SpirePrimitive | Kind::CoilPrimitive => Some(DesignFeatureFamily::Coil),
        Kind::Loft | Kind::Solevacion => Some(DesignFeatureFamily::Loft),
        Kind::Sweep => Some(DesignFeatureFamily::Sweep),
        Kind::Pipe => Some(DesignFeatureFamily::Pipe),
        Kind::SurfacePatch => Some(DesignFeatureFamily::SurfacePatch),
        Kind::SurfaceExtend => Some(DesignFeatureFamily::SurfaceExtend),
        Kind::SurfaceOffset => Some(DesignFeatureFamily::SurfaceOffset),
        Kind::SurfaceRuled => Some(DesignFeatureFamily::SurfaceRuled),
        Kind::SurfaceTrim => Some(DesignFeatureFamily::SurfaceTrim),
        Kind::BoundaryFill => Some(DesignFeatureFamily::BoundaryFill),
        Kind::Hole => Some(DesignFeatureFamily::Hole),
        Kind::Split => Some(DesignFeatureFamily::Split),
        Kind::Scale | Kind::Massstab | Kind::Escala => Some(DesignFeatureFamily::Scale),
        Kind::Thread => Some(DesignFeatureFamily::Thread),
        Kind::EdgeFlange => Some(DesignFeatureFamily::SheetMetalEdgeFlange),
        Kind::Hem => Some(DesignFeatureFamily::SheetMetalHem),
        Kind::SpherePrimitive
        | Kind::EsferaPrimitiva
        | Kind::TorusPrimitive
        | Kind::BoxPrimitive
        | Kind::CylinderPrimitive
        | Kind::Emboss
        | Kind::Align
        | Kind::Native(_)
        | Kind::Canvas
        | Kind::Lienzo
        | Kind::Decal
        | Kind::BaseMeshFeature
        | Kind::WorkPlane
        | Kind::PlanoDeObra
        | Kind::WorkAxis
        | Kind::WorkPoint
        | Kind::DerivedInstance
        | Kind::CustomFeature
        | Kind::Form
        | Kind::SurfaceStitch
        | Kind::BaseFeature
        | Kind::CopyPasteBodies
        | Kind::ComponentInsert
        | Kind::CopyPaste
        | Kind::JointOrigin
        | Kind::BaseFlange
        | Kind::SplitFace
        | Kind::MoveFace
        | Kind::DeplacerFace
        | Kind::DeleteFace
        | Kind::SurfaceDeleteFace
        | Kind::RemoveBody
        | Kind::Face => None,
    }
}

/// Return whether `kind` is a localized spelling of an edge-treatment family.
///
/// Canonical Fillet and Chamfer scopes require every selection to use a
/// counted construction-operand group. Their localized spellings do not.
fn is_localized_edge_treatment_kind(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> bool {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    matches!(
        design_feature_family(kind),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    ) && !matches!(kind, Kind::Fillet | Kind::Chamfer)
}

const RECIPES: &[(&[u8], ConstructionRecipeKind)] = &[
    (b"body_recipe_data", ConstructionRecipeKind::Body),
    (b"face_recipe_data", ConstructionRecipeKind::Face),
    (
        b"bounded_face_recipe_data",
        ConstructionRecipeKind::BoundedFace,
    ),
    (b"edge_recipe_data", ConstructionRecipeKind::Edge),
    (b"vertex_recipe_data", ConstructionRecipeKind::Vertex),
];

pub(crate) const fn construction_recipe_family_name_len(kind: ConstructionRecipeKind) -> usize {
    match kind {
        ConstructionRecipeKind::Body => b"body_recipe_data".len(),
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        ConstructionRecipeKind::Edge => b"edge_recipe_data".len(),
        ConstructionRecipeKind::Vertex => b"vertex_recipe_data".len(),
    }
}

mod json_value;
