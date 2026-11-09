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

/// Return the canonical operation family for a selected feature kind.
pub(crate) fn design_feature_family(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> Option<DesignFeatureFamily> {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    match kind {
        Kind::Sketch => Some(DesignFeatureFamily::Sketch),
        Kind::Assemble | Kind::AsBuilt => Some(DesignFeatureFamily::Assemble),
        Kind::Extrude => Some(DesignFeatureFamily::Extrude),
        Kind::Fillet => Some(DesignFeatureFamily::Fillet),
        Kind::Chamfer => Some(DesignFeatureFamily::Chamfer),
        Kind::Combine => Some(DesignFeatureFamily::Combine),
        Kind::Draft => Some(DesignFeatureFamily::Draft),
        Kind::ReplaceFace => Some(DesignFeatureFamily::ReplaceFace),
        Kind::CPattern | Kind::CircularPattern => Some(DesignFeatureFamily::CircularPattern),
        Kind::RPattern | Kind::RectangularPattern => Some(DesignFeatureFamily::RectangularPattern),
        Kind::Mirror => Some(DesignFeatureFamily::Mirror),
        Kind::Move => Some(DesignFeatureFamily::Move),
        Kind::OffsetFaces => Some(DesignFeatureFamily::OffsetFaces),
        Kind::Revolve => Some(DesignFeatureFamily::Revolve),
        Kind::Shell => Some(DesignFeatureFamily::Shell),
        Kind::Thicken => Some(DesignFeatureFamily::Thicken),
        Kind::SpirePrimitive | Kind::CoilPrimitive => Some(DesignFeatureFamily::Coil),
        Kind::Loft => Some(DesignFeatureFamily::Loft),
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
        Kind::Scale => Some(DesignFeatureFamily::Scale),
        Kind::Thread => Some(DesignFeatureFamily::Thread),
        Kind::EdgeFlange => Some(DesignFeatureFamily::SheetMetalEdgeFlange),
        Kind::Hem => Some(DesignFeatureFamily::SheetMetalHem),
        Kind::SpherePrimitive
        | Kind::TorusPrimitive
        | Kind::BoxPrimitive
        | Kind::CylinderPrimitive
        | Kind::Emboss
        | Kind::Align
        | Kind::Native(_)
        | Kind::Canvas
        | Kind::Decal
        | Kind::BaseMeshFeature
        | Kind::WorkPlane
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
        | Kind::PathPattern
        | Kind::MoveFace
        | Kind::DeleteFace
        | Kind::SurfaceDeleteFace
        | Kind::RemoveBody
        | Kind::ReverseNormal
        | Kind::Face => None,
    }
}

/// Return whether `group` is the face seed group of a rectangular, circular,
/// or Mirror pattern scope: role `0x0000001000000000` at scope-reference
/// ordinal one.
pub(crate) fn is_pattern_face_seed_group(
    kind: &crate::records::feature::scope::DesignFeatureKind,
    group: &crate::records::topology::construction::DesignConstructionOperandGroup,
) -> bool {
    matches!(
        design_feature_family(kind),
        Some(
            DesignFeatureFamily::RectangularPattern
                | DesignFeatureFamily::CircularPattern
                | DesignFeatureFamily::Mirror
        )
    ) && group.role() == crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X10
        && group.scope_reference_ordinal == 1
}

/// Return whether a Fillet or Chamfer scope stores a localized family name.
///
/// Canonical Fillet and Chamfer scopes require every selection to use a
/// counted construction-operand group. Localized ones do not.
fn is_localized_edge_treatment(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> bool {
    matches!(
        design_feature_family(&scope.kind()),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    ) && scope.source_kind_name() != scope.kind_name()
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
