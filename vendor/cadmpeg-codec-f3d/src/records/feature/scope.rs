// SPDX-License-Identifier: Apache-2.0
//! The parameter scope every feature record carries: its feature kind, its payload, and the frames and bindings the payload states.

use cadmpeg_core::decode::u64_from_index;

use super::align::DesignAlignEnvelope;
use super::assembly::DesignAssemblyAlignment;
#[cfg(test)]
use crate::records::identity::serialize_absent_u64_offset;

use super::assembly_features::{
    DesignComponentInsertConstruction, DesignCopyPasteComponentOperation,
    DesignDerivedInstanceConstruction,
};
use super::base_feature::DesignBaseFeatureConstruction;
use super::body_ops::{DesignCopyPasteBodiesOperation, DesignScaleOperation};
use super::coil::{
    DesignCoilExtent, DesignCoilPlacement, DesignCoilScope, DesignCoilSection,
    DesignCoilSectionPlacement, DesignCoilTransform,
};
use super::combine::DesignCombineOperation;
use super::direct_face::{
    DesignDirectFaceOperation, DesignDraftOperation, DesignMoveOperation,
    DesignOffsetFacesOperation, DesignShellOperation, DesignThickenOperation,
};
use super::emboss::DesignEmbossConstruction;
use super::extrude::{DesignExtrudeOperation, DesignExtrudePrologue};
use super::fixed_parameters::{
    DesignFixedChamferParameters, DesignFixedExtrudeParameters, DesignFixedFilletParameters,
};
use super::hole::DesignHoleConstruction;
use super::mirror::DesignMirrorConstruction;
use super::path_features::{
    DesignLoftConstruction, DesignPathFeatureConstruction, DesignPipeConstruction,
    DesignRevolveConstruction, DesignSweepConstruction,
};
use super::patterns::{DesignCircularPatternConstruction, DesignRectangularPatternConstruction};
use super::primitives::{
    DesignBoxPrimitive, DesignCylinderPrimitive, DesignSolidPrimitive, DesignSpherePrimitive,
    DesignTorusPrimitive,
};
use super::sheet_metal::{
    DesignBaseFlangeOperation, DesignEdgeFlangeOperation, DesignHemOperation,
};
use super::surface_ops::{
    DesignRuledSurfaceOperation, DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation,
    DesignSurfacePatchBoundary, DesignSurfacePatchNormalState, DesignSurfacePatchScope,
    DesignSurfaceStitchOperation,
};
use super::thread::DesignThreadConstruction;
use super::work_geometry::{
    DesignWorkAxisConstruction, DesignWorkPlaneConstruction, DesignWorkPointConstruction,
};
use crate::records::identity::{deserialize_absent_u64_offset, DesignEntityId, ReferenceRun};
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_hole_extent,
    crate::records::feature::hole::DesignHoleExtent,
    "hole_extent"
);
cadmpeg_core::named_optional_field!(
    deserialize_occurrence_transfer,
    super::occurrence_transfer::DesignOccurrenceTransfer,
    "occurrence_transfer"
);

cadmpeg_core::named_optional_field!(deserialize_source_kind, String, "source_kind");
cadmpeg_core::named_optional_field!(
    deserialize_assembly_alignment,
    DesignAssemblyAlignment,
    "assembly_alignment"
);
cadmpeg_core::named_optional_field!(
    deserialize_base_feature_construction,
    DesignBaseFeatureConstruction,
    "base_feature_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_base_flange_operation,
    DesignBaseFlangeOperation,
    "base_flange_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_base_flange_profile,
    DesignSketchProfileOperand,
    "base_flange_profile"
);
cadmpeg_core::named_optional_field!(
    deserialize_circular_pattern_construction,
    DesignCircularPatternConstruction,
    "circular_pattern_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_combine_operation,
    DesignCombineOperation,
    "combine_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_component_insert_construction,
    DesignComponentInsertConstruction,
    "component_insert_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_copy_paste_bodies_operation,
    DesignCopyPasteBodiesOperation,
    "copy_paste_bodies_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_copy_paste_component_operation,
    DesignCopyPasteComponentOperation,
    "copy_paste_component_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_derived_instance_construction,
    DesignDerivedInstanceConstruction,
    "derived_instance_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_direct_face_operation,
    DesignDirectFaceOperation,
    "direct_face_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_draft_operation,
    DesignDraftOperation,
    "draft_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_flange_operation,
    DesignEdgeFlangeOperation,
    "edge_flange_operation"
);
cadmpeg_core::named_optional_field!(deserialize_entity_id, String, "entity_id");
cadmpeg_core::named_optional_field!(
    deserialize_entity_reference_offset,
    u64,
    "entity_reference_offset"
);
cadmpeg_core::named_optional_field!(deserialize_entity_suffix, u64, "entity_suffix");
cadmpeg_core::named_optional_field!(
    deserialize_extrude_profile,
    DesignSketchProfileOperand,
    "extrude_profile"
);
cadmpeg_core::named_optional_field!(
    deserialize_extrude_native_profiles,
    Vec<DesignSketchProfileOperand>,
    "extrude_native_profiles"
);
cadmpeg_core::named_optional_field!(
    deserialize_extrude_prologue,
    DesignExtrudePrologue,
    "extrude_prologue"
);
cadmpeg_core::named_optional_field!(
    deserialize_fixed_chamfer_parameters,
    DesignFixedChamferParameters,
    "fixed_chamfer_parameters"
);
cadmpeg_core::named_optional_field!(
    deserialize_fixed_extrude_parameters,
    DesignFixedExtrudeParameters,
    "fixed_extrude_parameters"
);
cadmpeg_core::named_optional_field!(
    deserialize_fixed_fillet_parameters,
    DesignFixedFilletParameters,
    "fixed_fillet_parameters"
);
cadmpeg_core::named_optional_field!(
    deserialize_hem_operation,
    DesignHemOperation,
    "hem_operation"
);
cadmpeg_core::named_optional_field!(deserialize_history_state_id, i64, "history_state_id");
cadmpeg_core::named_optional_field!(
    deserialize_hole_construction,
    DesignHoleConstruction,
    "hole_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_joint_origin_reference,
    u32,
    "joint_origin_reference"
);
cadmpeg_core::named_optional_field!(
    deserialize_joint_origin_reference_offset,
    u64,
    "joint_origin_reference_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_joint_origin_transform,
    SketchPlacementMatrix,
    "joint_origin_transform"
);
cadmpeg_core::named_optional_field!(
    deserialize_joint_origin_transform_offset,
    u64,
    "joint_origin_transform_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_mirror_construction,
    DesignMirrorConstruction,
    "mirror_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_align_envelope,
    DesignAlignEnvelope,
    "align_envelope"
);
cadmpeg_core::named_optional_field!(
    deserialize_move_operation,
    DesignMoveOperation,
    "move_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_path_feature_construction,
    DesignPathFeatureConstruction,
    "path_feature_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_previous_history_state_id,
    i64,
    "previous_history_state_id"
);
cadmpeg_core::named_optional_field!(
    deserialize_rectangular_pattern_construction,
    DesignRectangularPatternConstruction,
    "rectangular_pattern_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_ruled_surface_operation,
    DesignRuledSurfaceOperation,
    "ruled_surface_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_scale_operation,
    DesignScaleOperation,
    "scale_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_solid_primitive,
    DesignSolidPrimitive,
    "solid_primitive"
);
cadmpeg_core::named_optional_field!(
    deserialize_surface_extend_operation,
    DesignSurfaceExtendOperation,
    "surface_extend_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_surface_offset_operation,
    DesignSurfaceOffsetOperation,
    "surface_offset_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_surface_stitch_operation,
    DesignSurfaceStitchOperation,
    "surface_stitch_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_sweep_profile,
    DesignSketchProfileOperand,
    "sweep_profile"
);
cadmpeg_core::named_optional_field!(
    deserialize_emboss_construction,
    DesignEmbossConstruction,
    "emboss_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_thread_construction,
    DesignThreadConstruction,
    "thread_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_work_axis_construction,
    DesignWorkAxisConstruction,
    "work_axis_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_work_plane_construction,
    DesignWorkPlaneConstruction,
    "work_plane_construction"
);
cadmpeg_core::named_optional_field!(
    deserialize_work_plane_reference,
    u32,
    "work_plane_reference"
);
cadmpeg_core::named_optional_field!(
    deserialize_work_plane_reference_offset,
    u64,
    "work_plane_reference_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_work_plane_transform,
    SketchPlacementMatrix,
    "work_plane_transform"
);
cadmpeg_core::named_optional_field!(
    deserialize_work_plane_transform_offset,
    u64,
    "work_plane_transform_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_work_point_construction,
    DesignWorkPointConstruction,
    "work_point_construction"
);
/// Construction-recipe families admitted by a face selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ConstructionRecipeKind", into = "ConstructionRecipeKind")]
pub(crate) enum DesignFaceRecipeKind {
    Face,
    BoundedFace,
}

impl TryFrom<ConstructionRecipeKind> for DesignFaceRecipeKind {
    type Error = String;

    fn try_from(kind: ConstructionRecipeKind) -> Result<Self, Self::Error> {
        match kind {
            ConstructionRecipeKind::Face => Ok(Self::Face),
            ConstructionRecipeKind::BoundedFace => Ok(Self::BoundedFace),
            ConstructionRecipeKind::Body
            | ConstructionRecipeKind::Edge
            | ConstructionRecipeKind::Vertex => {
                Err("recipe_kind must be face or bounded_face".into())
            }
        }
    }
}

impl From<DesignFaceRecipeKind> for ConstructionRecipeKind {
    fn from(kind: DesignFaceRecipeKind) -> Self {
        match kind {
            DesignFaceRecipeKind::Face => Self::Face,
            DesignFaceRecipeKind::BoundedFace => Self::BoundedFace,
        }
    }
}

/// Nonempty source spelling outside the specialized feature-family names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DesignNativeFeatureName(std::sync::Arc<str>);

/// Optional payloads can be uninterpreted; a counted collection is decoded even when empty.
trait ScopePayloadCompleteness {
    fn is_uninterpreted(&self) -> bool;
}
impl<T> ScopePayloadCompleteness for Option<T> {
    fn is_uninterpreted(&self) -> bool {
        self.is_none()
    }
}
impl<T> ScopePayloadCompleteness for Vec<T> {
    fn is_uninterpreted(&self) -> bool {
        false
    }
}

impl ScopePayloadCompleteness for crate::records::feature::hole::DesignHoleScope {
    fn is_uninterpreted(&self) -> bool {
        self.point.is_none() && self.extent.is_none()
    }
}

impl ScopePayloadCompleteness for DesignSurfacePatchScope {
    fn is_uninterpreted(&self) -> bool {
        matches!(
            self.normal,
            Some(DesignSurfacePatchNormalState::Unsupported)
        )
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_surface_patch_normal,
    DesignSurfacePatchNormalState,
    "surface_patch_normal"
);

macro_rules! design_feature_kinds {
    (data { $($variant:ident => $lit:literal : $payload:ty),+ $(,)? }
     fixed { $($fixed:ident => $fixed_lit:literal : $fixed_payload:ty),+ $(,)? }
     required { $($required:ident => $required_lit:literal : $required_payload:ty),+ $(,)? }
     names { $($unit:ident => $unit_lit:literal),+ $(,)? }) => {
        /// Feature family selected by a type identity or stored name.
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
        #[serde(try_from = "String")]
        pub(crate) enum DesignFeatureKind {
            $($variant,)+
            $($fixed,)+
            $($required,)+
            $($unit,)+
            /// Source name without a specialized construction grammar.
            Native(DesignNativeFeatureName),
        }

        impl DesignFeatureKind {
            /// Family token or native source spelling.
            pub(crate) fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $lit,)+
                    $(Self::$fixed => $fixed_lit,)+
                    $(Self::$required => $required_lit,)+
                    $(Self::$unit => $unit_lit,)+
                    Self::Native(name) => &name.0,
                }
            }

        }

        impl TryFrom<String> for DesignFeatureKind {
            type Error = &'static str;

            fn try_from(name: String) -> Result<Self, Self::Error> {
                match name.as_str() {
                    "" => Err("Design feature kind must not be empty"),
                    $($lit => Ok(Self::$variant),)+
                    $($fixed_lit => Ok(Self::$fixed),)+
                    $($required_lit => Ok(Self::$required),)+
                    $($unit_lit => Ok(Self::$unit),)+
                    _ => Ok(Self::Native(DesignNativeFeatureName(name.into()))),
                }
            }
        }

        impl From<DesignFeatureKind> for String {
            fn from(kind: DesignFeatureKind) -> Self { kind.as_str().to_owned() }
        }

        impl std::fmt::Display for DesignFeatureKind {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        /// Feature family and its construction carrier. An independently decoded
        /// scope envelope can lack specialized construction data.
        #[derive(Debug, Clone, PartialEq)]
        pub(crate) enum DesignScopePayload {
            $($variant($payload),)+
            $($fixed($fixed_payload),)+
            $($required($required_payload),)+
            $($unit,)+
            /// Source name without a specialized construction grammar.
            Native(DesignNativeFeatureName),
        }

        impl cadmpeg_ir::schema::rewrite::typed::RewriteIdentities for DesignScopePayload {
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed references")?;
                match self {
                    $(Self::$variant(value) => value.visit_identity_references(ctx, visitor),)+
                    $(Self::$fixed(value) => value.visit_identity_references(ctx, visitor),)+
                    $(Self::$required(value) => value.visit_identity_references(ctx, visitor),)+
                    $(Self::$unit => Ok(()),)+
                    Self::Native(name) => name.visit_identity_references(ctx, visitor),
                }
            }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
                self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
            ) -> Result<Self, cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("rewrite native scope payload")?;
                match self {
                    $(Self::$variant(value) => Ok(Self::$variant(value.rewrite_identities(ctx, map)?)),)+
                    $(Self::$fixed(value) => Ok(Self::$fixed(value.rewrite_identities(ctx, map)?)),)+
                    $(Self::$required(value) => Ok(Self::$required(value.rewrite_identities(ctx, map)?)),)+
                    $(Self::$unit => Ok(Self::$unit),)+
                    Self::Native(name) => Ok(Self::Native(name)),
                }
            }
        }

        /// Mutable construction fields with a fixed feature family.
        pub(crate) enum DesignScopePayloadMut<'a> {
            $($variant(&'a mut $payload),)+
            $($fixed(&'a mut $fixed_payload),)+
            Other,
        }

        impl DesignScopePayload {
            /// The source envelope is decoded, but its optional family-specific data is not.
            pub(crate) fn is_uninterpreted(&self) -> bool {
                if matches!(self, Self::Emboss(_)) {
                    return true;
                }
                match self {
                    $(Self::$variant(value) => ScopePayloadCompleteness::is_uninterpreted(value),)+
                    $(Self::$fixed(value) => ScopePayloadCompleteness::is_uninterpreted(value),)+
                    $(Self::$required(_) => false,)+
                    $(Self::$unit => false,)+
                    Self::Native(_) => false,
                }
            }

            fn fields_mut(&mut self) -> DesignScopePayloadMut<'_> {
                match self {
                    $(Self::$variant(value) => DesignScopePayloadMut::$variant(value),)+
                    $(Self::$fixed(value) => DesignScopePayloadMut::$fixed(value),)+
                    _ => DesignScopePayloadMut::Other,
                }
            }
        }

        impl TryFrom<DesignFeatureKind> for DesignScopePayload {
            type Error = &'static str;
            fn try_from(kind: DesignFeatureKind) -> Result<Self, Self::Error> {
                match kind {
                    $(DesignFeatureKind::$variant => Ok(Self::$variant(Default::default())),)+
                    $(DesignFeatureKind::$fixed => Ok(Self::$fixed(Default::default())),)+
                    $(DesignFeatureKind::$required => Err(concat!($required_lit, " requires its operation")),)+
                    $(DesignFeatureKind::$unit => Ok(Self::$unit),)+
                    DesignFeatureKind::Native(name) => Ok(Self::Native(name)),
                }
            }
        }

        impl DesignScopePayload {
            pub(crate) fn kind(&self) -> DesignFeatureKind {
                match self {
                    $(Self::$variant(_) => DesignFeatureKind::$variant,)+
                    $(Self::$fixed(_) => DesignFeatureKind::$fixed,)+
                    $(Self::$required(_) => DesignFeatureKind::$required,)+
                    $(Self::$unit => DesignFeatureKind::$unit,)+
                    Self::Native(name) => DesignFeatureKind::Native(name.clone()),
                }
            }

            fn kind_name(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $lit,)+
                    $(Self::$fixed(_) => $fixed_lit,)+
                    $(Self::$required(_) => $required_lit,)+
                    $(Self::$unit => $unit_lit,)+
                    Self::Native(name) => &name.0,
                }
            }
        }
    };
}
impl Serialize for DesignFeatureKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

design_feature_kinds! {
    data {
        Sketch => "Sketch": Option<DesignSketchEntityBinding>,
        Esquisse => "Esquisse": Option<DesignSketchEntityBinding>,
        Skizze => "Skizze": Option<DesignSketchEntityBinding>,
        Esboco => "Esboço": Option<DesignSketchEntityBinding>,
        Boceto => "Boceto": Option<DesignSketchEntityBinding>,
        SketchJa => "スケッチ": Option<DesignSketchEntityBinding>,
        Assemble => "Assemble": Option<DesignAssemblyAlignment>,
        AsBuilt => "As-built": Option<DesignAssemblyAlignment>,
        Extrude => "Extrude": Option<DesignExtrudeScope>,
        Extrusion => "Extrusion": Option<DesignExtrudeScope>,
        Extrusao => "Extrusão": Option<DesignExtrudeScope>,
        Extruir => "Extruir": Option<DesignExtrudeScope>,
        ExtrudeJa => "押し出し": Option<DesignExtrudeScope>,
        Fillet => "Fillet": Option<DesignFixedFilletParameters>,
        Conge => "Congé": Option<DesignFixedFilletParameters>,
        Abrundung => "Abrundung": Option<DesignFixedFilletParameters>,
        Arredondamento => "Arredondamento": Option<DesignFixedFilletParameters>,
        Empalme => "Empalme": Option<DesignFixedFilletParameters>,
        FilletJa => "フィレット": Option<DesignFixedFilletParameters>,
        Chamfer => "Chamfer": Option<DesignFixedChamferParameters>,
        Chanfrein => "Chanfrein": Option<DesignFixedChamferParameters>,
        Chaflan => "Chaflán": Option<DesignFixedChamferParameters>,
        ChamferJa => "面取り": Option<DesignFixedChamferParameters>,
        Combine => "Combine": Option<DesignCombineOperation>,
        Draft => "Draft": Option<DesignDraftOperation>,
        CPattern => "C-Pattern": Option<DesignCircularPatternConstruction>,
        CircularPattern => "Circular Pattern": Option<DesignCircularPatternConstruction>,
        ReseauC => "Réseau C": Option<DesignCircularPatternConstruction>,
        RPattern => "R-Pattern": Option<DesignRectangularPatternConstruction>,
        RectangularPattern => "Rectangular Pattern": Option<DesignRectangularPatternConstruction>,
        Mirror => "Mirror": Option<DesignMirrorConstruction>,
        SymetrieMiroir => "Symétrie miroir": Option<DesignMirrorConstruction>,
        Align => "Align": Option<DesignAlignEnvelope>,
        Move => "Move": Option<DesignMoveOperation>,
        MoveFace => "MoveFace": Option<DesignMoveOperation>,
        DeplacerFace => "DéplacerFace": Option<DesignMoveOperation>,
        OffsetFaces => "OffsetFaces": Option<DesignOffsetFacesOperation>,
        DecalerLesFaces => "DécalerLesFaces": Option<DesignOffsetFacesOperation>,
        Shell => "Shell": Option<DesignShellOperation>,
        Schale => "Schale": Option<DesignShellOperation>,
        Vaciado => "Vaciado": Option<DesignShellOperation>,
        Thicken => "Thicken": Option<DesignThickenOperation>,
        SpirePrimitive => "SpirePrimitive": Option<DesignCoilScope>,
        CoilPrimitive => "CoilPrimitive": Option<DesignCoilScope>,
        Sweep => "Sweep": Option<DesignSweepScope>,
        SurfacePatch => "SurfacePatch": DesignSurfacePatchScope,
        SurfaceExtend => "SurfaceExtend": Option<DesignSurfaceExtendOperation>,
        SurfaceOffset => "SurfaceOffset": Option<DesignSurfaceOffsetOperation>,
        Hole => "Hole": crate::records::feature::hole::DesignHoleScope,
        Scale => "Scale": Option<DesignScaleOperation>,
        Massstab => "Maßstab": Option<DesignScaleOperation>,
        Escala => "Escala": Option<DesignScaleOperation>,
        Thread => "Thread": Option<DesignThreadConstruction>,
        Emboss => "Emboss": Option<DesignEmbossConstruction>,
        EdgeFlange => "EdgeFlange": Option<DesignEdgeFlangeOperation>,
        Hem => "Hem": Option<DesignHemOperation>,
        BaseFlange => "BaseFlange": Option<DesignBaseFlangeScope>,
        ComponentInsert => "Component Insert": Option<DesignComponentInsertConstruction>,
        CopyPaste => "CopyPaste": Option<DesignCopyPasteComponentOperation>,
        JointOrigin => "JointOrigin": Option<DesignJointOriginTransform>,
        WorkPlane => "WorkPlane": Option<DesignWorkPlaneTransform>,
        PlanoDeObra => "PlanoDeObra": Option<DesignWorkPlaneTransform>,
        WorkAxis => "WorkAxis": Option<DesignWorkAxisConstruction>,
        WorkPoint => "WorkPoint": Option<DesignWorkPointConstruction>,
        DerivedInstance => "DerivedInstance": Option<DesignDerivedInstanceConstruction>,
        BaseFeature => "Base Feature": Option<DesignBaseFeatureConstruction>,
        CopyPasteBodies => "CopyPasteBodies": Option<DesignCopyPasteBodiesOperation>,
    }
    fixed {
        Revolve => "Revolve": Option<DesignRevolveConstruction>,
        Loft => "Loft": Option<DesignLoftConstruction>,
        Solevacion => "Solevación": Option<DesignLoftConstruction>,
        Pipe => "Pipe": Option<DesignPipeConstruction>,
        SpherePrimitive => "SpherePrimitive": Option<DesignSpherePrimitive>,
        EsferaPrimitiva => "EsferaPrimitiva": Option<DesignSpherePrimitive>,
        TorusPrimitive => "TorusPrimitive": Option<DesignTorusPrimitive>,
        BoxPrimitive => "BoxPrimitive": Option<DesignBoxPrimitive>,
        CylinderPrimitive => "CylinderPrimitive": Option<DesignCylinderPrimitive>,
    }
    required {
        SurfaceStitch => "SurfaceStitch": DesignSurfaceStitchOperation,
        SurfaceRuled => "SurfaceRuled": DesignRuledSurfaceOperation,
    }
    names {
        ReplaceFace => "ReplaceFace",
        SurfaceTrim => "SurfaceTrim",
        BoundaryFill => "BoundaryFill",
        Split => "Split",
        Canvas => "Canvas",
        Lienzo => "Lienzo",
        Decal => "Decal",
        BaseMeshFeature => "Base Mesh Feature",
        CustomFeature => "CustomFeature",
        Form => "Form",
        SplitFace => "SplitFace",
        DeleteFace => "DeleteFace",
        SurfaceDeleteFace => "SurfaceDeleteFace",
        RemoveBody => "RemoveBody",
        Face => "Face",
    }
}

/// Rejected CADIR payload that names more than one family or disagrees with `kind`.
#[derive(Debug)]
pub(crate) struct DesignParameterScopePayloadError(String);

impl std::fmt::Display for DesignParameterScopePayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DesignParameterScopePayloadError {}

/// Distance in bytes from the state word to the length-prefixed kind name of a
/// parameter scope.
const HISTORY_STATE_ID_BACK_OFFSET: u64 = 8;

#[cfg(test)]
std::thread_local! {
    static SCOPE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[derive(Debug, PartialEq)]
struct ScopeCloneProbe;

#[cfg(test)]
impl Clone for ScopeCloneProbe {
    fn clone(&self) -> Self {
        SCOPE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self
    }
}

/// Indexed sketch or construction-operation record that scopes parameters.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignParameterScopeSerde")]
pub(crate) struct DesignParameterScope {
    #[cfg(test)]
    clone_probe: ScopeCloneProbe,
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the primary indexed record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub(crate) record_index: u32,
    /// Byte length from the primary header to the paired header.
    frame_length: u64,
    /// Stored family spelling when it differs from the selected family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_kind: Option<String>,
    /// Byte offset of the kind's UTF-16LE code units.
    kind_offset: u64,
    /// Byte offset of the state word before the length-prefixed kind name.
    history_state_id_offset: u64,
    /// One-based ordinal among scopes of the same feature family.
    pub(crate) feature_ordinal: std::num::NonZeroU32,
    /// Byte offset of `feature_ordinal`.
    feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history_state_id: Option<i64>,
    /// ASM delta-state identity immediately preceding this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    reference_members: ReferenceRun<u32>,
    /// Component ownership and placement recorded by paste operations.
    pub(crate) occurrence_transfer: Option<super::occurrence_transfer::DesignOccurrenceTransfer>,
    /// Family-specific construction records.
    payload: DesignScopePayload,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) unclosed_construction_operand_groups: Vec<u32>,
    /// Per-file dynamic class tag of the paired header.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Byte offset of the paired indexed record header.
    paired_byte_offset: u64,
}

/// Unadmitted parameter-scope fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignParameterScopeDraft {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the primary indexed record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub(crate) record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub(crate) frame_length: u64,
    /// Byte offset of the kind's UTF-16LE code units.
    pub(crate) kind_offset: u64,
    /// One-based ordinal among scopes of the same feature family.
    pub(crate) feature_ordinal: std::num::NonZeroU32,
    /// Byte offset of `feature_ordinal`.
    pub(crate) feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    pub(crate) history_state_id: Option<i64>,
    /// ASM delta-state identity immediately preceding this scope, when active.
    pub(crate) previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    pub(crate) previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    pub(crate) reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    pub(crate) reference_members: ReferenceRun<u32>,
    /// Family-specific construction records.
    pub(crate) payload: DesignScopePayload,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    pub(crate) unclosed_construction_operand_groups: Vec<u32>,
    /// Per-file dynamic class tag of the paired header.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Byte offset of the paired indexed record header.
    pub(crate) paired_byte_offset: u64,
}

/// Wire form of [`DesignParameterScope`] with the historical flat field set.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DesignParameterScopeSerde {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_transfer"
    )]
    occurrence_transfer: Option<super::occurrence_transfer::DesignOccurrenceTransfer>,
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of the primary indexed record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Shared logical record identity.
    record_index: u32,
    /// Byte length from the primary header to the paired header.
    frame_length: u64,
    /// Selected feature family.
    kind: DesignFeatureKind,
    /// Stored family spelling when it differs from the selected family.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_kind"
    )]
    source_kind: Option<String>,
    /// Byte offset of the kind's UTF-16LE code units.
    kind_offset: u64,
    /// Extrude prologue, fixed parameters, and profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    extrude: Option<DesignExtrudeScope>,
    /// Coil discriminators, placement, and transform.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    coil: Option<DesignCoilScope>,
    /// One-based ordinal among scopes of the same feature family.
    feature_ordinal: u32,
    /// Byte offset of `feature_ordinal`.
    feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_history_state_id"
    )]
    history_state_id: Option<i64>,
    /// Byte offset of the encoded history-state identity or null sentinel.
    history_state_id_offset: u64,
    /// ASM delta-state identity immediately preceding this scope, when active.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_previous_history_state_id"
    )]
    previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    #[serde(
        default,
        serialize_with = "serialize_absent_u64_offset",
        deserialize_with = "deserialize_absent_u64_offset"
    )]
    previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    reference_members: Vec<u32>,
    /// Byte offsets parallel to `reference_members`.
    reference_member_offsets: Vec<u64>,
    /// Exact solid-primitive construction carried by this scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_solid_primitive"
    )]
    solid_primitive: Option<DesignSolidPrimitive>,
    /// Exact fixed-form construction carried by a direct-face scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direct_face_operation"
    )]
    direct_face_operation: Option<DesignDirectFaceOperation>,
    /// Exact rigid transform carried by a Move scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_move_operation"
    )]
    move_operation: Option<DesignMoveOperation>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_align_envelope"
    )]
    align_envelope: Option<DesignAlignEnvelope>,
    /// Exact uniform body-scale construction carried by a Scale scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scale_operation"
    )]
    scale_operation: Option<DesignScaleOperation>,
    /// Exact tolerance and setting-record references carried by a `SurfaceStitch` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface_stitch_operation"
    )]
    surface_stitch_operation: Option<DesignSurfaceStitchOperation>,
    /// Exact distance, method, and boundary records carried by a `SurfaceExtend` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface_extend_operation"
    )]
    surface_extend_operation: Option<DesignSurfaceExtendOperation>,
    /// Exact distance and boundary records carried by a `SurfaceOffset` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface_offset_operation"
    )]
    surface_offset_operation: Option<DesignSurfaceOffsetOperation>,
    /// Exact mode, parameter, and selection records carried by a `SurfaceRuled` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ruled_surface_operation"
    )]
    ruled_surface_operation: Option<DesignRuledSurfaceOperation>,
    /// `BaseFlange` operation and sketch profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    base_flange: Option<DesignBaseFlangeScope>,
    /// Per-boundary-component settings carried by a `SurfacePatch` scope, in
    /// scope reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    surface_patch_boundaries: Vec<DesignSurfacePatchBoundary>,
    /// Versioned whole-surface normal state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface_patch_normal"
    )]
    surface_patch_normal: Option<DesignSurfacePatchNormalState>,
    /// Exact edge, parameter, and settings records carried by an `EdgeFlange` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_flange_operation"
    )]
    edge_flange_operation: Option<DesignEdgeFlangeOperation>,
    /// Exact edge, parameter, and settings records carried by a `Hem` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hem_operation"
    )]
    hem_operation: Option<DesignHemOperation>,

    /// Exact fixed scalar lanes carried by a Fillet scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fixed_fillet_parameters"
    )]
    fixed_fillet_parameters: Option<DesignFixedFilletParameters>,
    /// Exact fixed scalar lane carried by an equal-distance Chamfer scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fixed_chamfer_parameters"
    )]
    fixed_chamfer_parameters: Option<DesignFixedChamferParameters>,
    /// Path-feature construction and Sweep sketch profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    path_feature: Option<DesignPathFeatureWire>,
    /// Exact Boolean construction carried by a `Combine` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_combine_operation"
    )]
    combine_operation: Option<DesignCombineOperation>,
    /// Exact form and size construction carried by a `Thread` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thread_construction"
    )]
    thread_construction: Option<DesignThreadConstruction>,
    /// Emboss scalar owners, selection groups, and retained control words.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_emboss_construction"
    )]
    emboss_construction: Option<DesignEmbossConstruction>,
    /// Exact signed-angle construction carried by a `Draft` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft_operation"
    )]
    draft_operation: Option<DesignDraftOperation>,
    /// Exact construction carried by a circular-pattern scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_circular_pattern_construction"
    )]
    circular_pattern_construction: Option<DesignCircularPatternConstruction>,
    /// Exact scalar lanes carried by a rectangular-pattern scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rectangular_pattern_construction"
    )]
    rectangular_pattern_construction: Option<DesignRectangularPatternConstruction>,
    /// Exact alignment scalars carried by an `Assemble` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_assembly_alignment"
    )]
    assembly_alignment: Option<DesignAssemblyAlignment>,
    /// Exact external-occurrence construction carried by a `Component Insert` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_insert_construction"
    )]
    component_insert_construction: Option<DesignComponentInsertConstruction>,
    /// Exact local-occurrence construction carried by a `DerivedInstance` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_derived_instance_construction"
    )]
    derived_instance_construction: Option<DesignDerivedInstanceConstruction>,
    /// Exact local-component construction carried by a legacy `CopyPaste` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_copy_paste_component_operation"
    )]
    copy_paste_component_operation: Option<DesignCopyPasteComponentOperation>,
    /// Exact construction carried by a Mirror scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_mirror_construction"
    )]
    mirror_construction: Option<DesignMirrorConstruction>,
    /// Exact source-to-copy body mapping carried by a `CopyPasteBodies` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_copy_paste_bodies_operation"
    )]
    copy_paste_bodies_operation: Option<DesignCopyPasteBodiesOperation>,
    /// Exact result-body references carried by a `Base Feature` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_feature_construction"
    )]
    base_feature_construction: Option<DesignBaseFeatureConstruction>,
    /// Exact row-major local-to-model frame carried by a `WorkPlane` scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_work_plane_frame")]
    work_plane_frame: Option<DesignWorkPlaneTransform>,
    /// Exact two-point construction carried by a `WorkAxis` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_work_axis_construction"
    )]
    work_axis_construction: Option<DesignWorkAxisConstruction>,
    /// Exact row-major local-to-model frame owned by a `JointOrigin` scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_joint_origin_frame")]
    joint_origin_frame: Option<DesignJointOriginTransform>,

    /// Exact solved construction carried by a `WorkPoint` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_work_point_construction"
    )]
    work_point_construction: Option<DesignWorkPointConstruction>,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unclosed_construction_operand_groups: Vec<u32>,
    /// Exact point-and-direction construction carried by a `Hole` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hole_construction"
    )]
    hole_construction: Option<DesignHoleConstruction>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hole_extent"
    )]
    hole_extent: Option<crate::records::feature::hole::DesignHoleExtent>,

    /// Sketch-module entity bound to this sketch scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_sketch_entity")]
    sketch_entity: Option<DesignSketchEntityBinding>,
    /// Per-file dynamic class tag of the paired header.
    paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    paired_byte_offset: u64,
}

// Deserialize the payload itself: flattened Option<T> suppresses T's errors.
fn deserialize_flattened_scope<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
struct WorkPlaneFrameWire {
    #[serde(default, deserialize_with = "deserialize_work_plane_transform")]
    #[serde(rename = "work_plane_transform")]
    transform: Option<SketchPlacementMatrix>,
    #[serde(default, deserialize_with = "deserialize_work_plane_transform_offset")]
    #[serde(rename = "work_plane_transform_offset")]
    transform_offset: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_work_plane_reference")]
    #[serde(rename = "work_plane_reference")]
    reference: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_work_plane_reference_offset")]
    #[serde(rename = "work_plane_reference_offset")]
    reference_offset: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_work_plane_construction")]
    #[serde(rename = "work_plane_construction")]
    construction: Option<DesignWorkPlaneConstruction>,
}

fn deserialize_work_plane_frame<'de, D>(
    deserializer: D,
) -> Result<Option<DesignWorkPlaneTransform>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = WorkPlaneFrameWire::deserialize(deserializer)?;
    let reference = match (wire.reference, wire.reference_offset) {
        (None, None) => None,
        (Some(work_plane_reference), Some(work_plane_reference_offset)) => {
            Some(DesignWorkPlaneReference {
                work_plane_reference,
                work_plane_reference_offset,
            })
        }
        _ => {
            return Err(serde::de::Error::custom(
                "work_plane_reference and work_plane_reference_offset must occur together",
            ))
        }
    };
    match (wire.transform, wire.transform_offset) {
        (None, None) if reference.is_none() && wire.construction.is_none() => Ok(None),
        (Some(work_plane_transform), Some(work_plane_transform_offset)) => Ok(Some(DesignWorkPlaneTransform {
            work_plane_transform,
            work_plane_transform_offset,
            reference,
            work_plane_construction: wire.construction,
        })),
        _ => Err(serde::de::Error::custom("work_plane_transform and work_plane_transform_offset are required for work_plane frame data")),
    }
}

impl<'de> Deserialize<'de> for DesignWorkPlaneTransform {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_work_plane_frame(deserializer)?
            .ok_or_else(|| serde::de::Error::missing_field("work_plane_transform"))
    }
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
struct JointOriginFrameWire {
    #[serde(default, deserialize_with = "deserialize_joint_origin_transform")]
    #[serde(rename = "joint_origin_transform")]
    transform: Option<SketchPlacementMatrix>,
    #[serde(
        default,
        deserialize_with = "deserialize_joint_origin_transform_offset"
    )]
    #[serde(rename = "joint_origin_transform_offset")]
    transform_offset: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_joint_origin_reference")]
    #[serde(rename = "joint_origin_reference")]
    reference: Option<u32>,
    #[serde(
        default,
        deserialize_with = "deserialize_joint_origin_reference_offset"
    )]
    #[serde(rename = "joint_origin_reference_offset")]
    reference_offset: Option<u64>,
}

fn deserialize_joint_origin_frame<'de, D>(
    deserializer: D,
) -> Result<Option<DesignJointOriginTransform>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = JointOriginFrameWire::deserialize(deserializer)?;
    let reference = match (wire.reference, wire.reference_offset) {
        (None, None) => None,
        (Some(joint_origin_reference), Some(joint_origin_reference_offset)) => {
            Some(DesignJointOriginReference {
                joint_origin_reference,
                joint_origin_reference_offset,
            })
        }
        _ => {
            return Err(serde::de::Error::custom(
                "joint_origin_reference and joint_origin_reference_offset must occur together",
            ))
        }
    };
    match (wire.transform, wire.transform_offset) {
        (None, None) if reference.is_none() => Ok(None),
        (Some(joint_origin_transform), Some(joint_origin_transform_offset)) => Ok(Some(DesignJointOriginTransform {
            joint_origin_transform,
            joint_origin_transform_offset,
            reference,
        })),
        _ => Err(serde::de::Error::custom("joint_origin_transform and joint_origin_transform_offset are required for joint_origin frame data")),
    }
}

impl<'de> Deserialize<'de> for DesignJointOriginTransform {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_joint_origin_frame(deserializer)?
            .ok_or_else(|| serde::de::Error::missing_field("joint_origin_transform"))
    }
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
struct SketchEntityWire {
    #[serde(default, deserialize_with = "deserialize_entity_id")]
    #[serde(rename = "entity_id")]
    id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_entity_suffix")]
    #[serde(rename = "entity_suffix")]
    suffix: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_entity_reference_offset")]
    #[serde(rename = "entity_reference_offset")]
    reference_offset: Option<u64>,
}

fn deserialize_sketch_entity<'de, D>(
    deserializer: D,
) -> Result<Option<DesignSketchEntityBinding>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = SketchEntityWire::deserialize(deserializer)?;
    match (wire.id, wire.suffix, wire.reference_offset) {
        (None, None, None) => Ok(None),
        (Some(entity_id), Some(entity_suffix), Some(entity_reference_offset)) => {
            DesignSketchEntityBinding::try_from(DesignSketchEntityBindingWire {
                id: entity_id,
                suffix: entity_suffix,
                reference_offset: entity_reference_offset,
            })
            .map(Some)
            .map_err(serde::de::Error::custom)
        }
        _ => Err(serde::de::Error::custom(
            "entity_id, entity_suffix, and entity_reference_offset must occur together",
        )),
    }
}

fn base_flange_scope_is_absent(base_flange: Option<&DesignBaseFlangeScope>) -> bool {
    match base_flange {
        None => true,
        Some(base_flange) => {
            base_flange.base_flange_operation.is_none() && base_flange.base_flange_profile.is_none()
        }
    }
}

fn coil_scope_is_absent(coil: Option<&DesignCoilScope>) -> bool {
    match coil {
        None => true,
        Some(coil) => {
            coil.operation.is_none()
                && coil.extent.is_none()
                && coil.section.is_none()
                && coil.section_placement.is_none()
                && coil.clockwise.is_none()
                && coil.placement.is_none()
                && coil.transform.is_none()
        }
    }
}

fn extrude_scope_is_absent(extrude: Option<&DesignExtrudeScope>) -> bool {
    match extrude {
        None => true,
        Some(extrude) => {
            extrude.extrude_prologue.is_none()
                && extrude.fixed_extrude_parameters.is_none()
                && extrude.extrude_profile.is_none()
                && extrude.extrude_native_profiles.is_none()
        }
    }
}

fn path_feature_scope_is_absent(path_feature: Option<&DesignPathFeatureWire>) -> bool {
    match path_feature {
        None => true,
        Some(path_feature) => {
            path_feature.path_feature_construction.is_none() && path_feature.sweep_profile.is_none()
        }
    }
}

/// BaseFlange-specific records carried by a `BaseFlange` parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(crate) struct DesignBaseFlangeScope {
    /// Exact profile and thickness records carried by a `BaseFlange` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_flange_operation"
    )]
    pub(crate) base_flange_operation: Option<DesignBaseFlangeOperation>,
    /// Sketch-profile operand carried by a `BaseFlange` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_flange_profile"
    )]
    pub(crate) base_flange_profile: Option<DesignSketchProfileOperand>,
}

/// Extrude-specific records carried by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(crate) struct DesignExtrudeScope {
    /// Extrude fixed prologue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extrude_prologue"
    )]
    pub(crate) extrude_prologue: Option<DesignExtrudePrologue>,
    /// Exact fixed scalar lanes carried by an Extrude scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fixed_extrude_parameters"
    )]
    pub(crate) fixed_extrude_parameters: Option<DesignFixedExtrudeParameters>,
    /// Profile operand carried by an Extrude scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extrude_profile"
    )]
    pub(crate) extrude_profile: Option<DesignSketchProfileOperand>,
    /// Multiple Sketch-profile operands retained with their independent selections.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extrude_native_profiles"
    )]
    pub(crate) extrude_native_profiles: Option<Vec<DesignSketchProfileOperand>>,
}

/// Sweep construction and its independently decoded profile operand.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(crate) struct DesignSweepScope {
    pub(crate) construction: Option<DesignSweepConstruction>,
    pub(crate) sweep_profile: Option<DesignSketchProfileOperand>,
}

impl From<DesignPathFeatureConstruction> for DesignScopePayload {
    fn from(value: DesignPathFeatureConstruction) -> Self {
        match value {
            DesignPathFeatureConstruction::Revolve(value) => Self::Revolve(Some(value)),
            DesignPathFeatureConstruction::Loft(value) => Self::Loft(Some(value)),
            DesignPathFeatureConstruction::Pipe(value) => Self::Pipe(Some(value)),
            DesignPathFeatureConstruction::Sweep(value) => Self::Sweep(Some(DesignSweepScope {
                construction: Some(value),
                sweep_profile: None,
            })),
        }
    }
}

/// Flat wire fields for path construction and the Sweep profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct DesignPathFeatureWire {
    /// Exact fixed construction carried by a Loft, Sweep, Revolve, or Pipe scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path_feature_construction"
    )]
    path_feature_construction: Option<DesignPathFeatureConstruction>,
    /// Sketch-profile operand carried by a `Sweep` scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sweep_profile"
    )]
    sweep_profile: Option<DesignSketchProfileOperand>,
}

/// Sketch-module entity named by a sketch parameter scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignSketchEntityBindingWire")]
pub(crate) struct DesignSketchEntityBinding {
    /// Full Design entity id of a sketch scope.
    pub(crate) entity_id: DesignEntityId,
    /// Byte offset of the sketch entity suffix.
    pub(crate) entity_reference_offset: u64,
}

#[cfg(test)]
thread_local! {
    static SKETCH_ENTITY_BINDING_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignSketchEntityBinding {
    fn clone(&self) -> Self {
        SKETCH_ENTITY_BINDING_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            entity_id: self.entity_id.clone(),
            entity_reference_offset: self.entity_reference_offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Field names are the native record serialized keys.
struct DesignSketchEntityBindingWire {
    /// Full Design entity id of a sketch scope.
    #[serde(rename = "entity_id")]
    id: String,
    /// Numeric suffix of `entity_id`.
    #[serde(rename = "entity_suffix")]
    suffix: u64,
    /// Byte offset of the sketch entity suffix.
    #[serde(rename = "entity_reference_offset")]
    reference_offset: u64,
}

impl Serialize for DesignSketchEntityBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            #[serde(rename = "entity_id")]
            id: &'a str,
            #[serde(rename = "entity_suffix")]
            suffix: u64,
            #[serde(rename = "entity_reference_offset")]
            reference_offset: u64,
        }
        WireRef {
            id: self.entity_id.as_str(),
            suffix: self.entity_id.suffix(),
            reference_offset: self.entity_reference_offset,
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignSketchEntityBindingWire> for DesignSketchEntityBinding {
    type Error = String;

    fn try_from(wire: DesignSketchEntityBindingWire) -> Result<Self, Self::Error> {
        let entity_id = DesignEntityId::try_from(wire.id)?;
        if entity_id.suffix() != wire.suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }
        Ok(Self {
            entity_id,
            entity_reference_offset: wire.reference_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignSketchEntityBinding> for DesignSketchEntityBindingWire {
    fn from(value: DesignSketchEntityBinding) -> Self {
        let entity_suffix = value.entity_id.suffix();
        Self {
            id: value.entity_id.into_string(),
            suffix: entity_suffix,
            reference_offset: value.entity_reference_offset,
        }
    }
}

/// Solved frame carried by a `WorkPlane` scope.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct DesignWorkPlaneTransform {
    /// Exact row-major local-to-model frame.
    pub(crate) work_plane_transform: SketchPlacementMatrix,
    /// Byte offset of the matrix or its implicit-identity marker.
    pub(crate) work_plane_transform_offset: u64,
    /// Construction record referenced by the frame, when present.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reference: Option<DesignWorkPlaneReference>,
    /// Exact construction rule carried by this `WorkPlane` frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) work_plane_construction: Option<DesignWorkPlaneConstruction>,
}

/// Construction record named by a `WorkPlane` frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkPlaneReference {
    /// Construction record referenced by the `WorkPlane` frame.
    pub(crate) work_plane_reference: u32,
    /// Byte offset of the `WorkPlane` construction reference.
    pub(crate) work_plane_reference_offset: u64,
}

/// Explicit 16-f64 frame carried by a `JointOrigin` scope.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct DesignJointOriginTransform {
    /// Exact row-major local-to-model frame.
    pub(crate) joint_origin_transform: SketchPlacementMatrix,
    /// Byte offset of the explicit 16-f64 matrix.
    pub(crate) joint_origin_transform_offset: u64,
    /// Construction record referenced by the frame, when present.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reference: Option<DesignJointOriginReference>,
}

/// Construction record named by a `JointOrigin` frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignJointOriginReference {
    /// Construction record referenced by the `JointOrigin` frame.
    pub(crate) joint_origin_reference: u32,
    /// Byte offset of the `JointOrigin` construction reference.
    pub(crate) joint_origin_reference_offset: u64,
}

impl TryFrom<DesignParameterScopeSerde> for DesignParameterScope {
    type Error = DesignParameterScopePayloadError;

    fn try_from(mut wire: DesignParameterScopeSerde) -> Result<Self, Self::Error> {
        if let Some(source) = &wire.source_kind {
            let units = u64::try_from(source.encode_utf16().count()).ok();
            if !units.is_some_and(|count| (1..=256).contains(&count))
                || source.chars().any(char::is_control)
                || units.and_then(|count| count.checked_mul(2))
                    != wire.feature_ordinal_offset.checked_sub(wire.kind_offset)
            {
                return Err(DesignParameterScopePayloadError(
                    "invalid source_kind".into(),
                ));
            }
        }
        if extrude_scope_is_absent(wire.extrude.as_ref()) {
            wire.extrude = None;
        }
        if coil_scope_is_absent(wire.coil.as_ref()) {
            wire.coil = None;
        }
        if base_flange_scope_is_absent(wire.base_flange.as_ref()) {
            wire.base_flange = None;
        }
        if path_feature_scope_is_absent(wire.path_feature.as_ref()) {
            wire.path_feature = None;
        }
        let mut present = Vec::new();
        if wire.extrude.is_some() {
            present.push("extrude");
        }
        if wire.coil.is_some() {
            present.push("coil");
        }
        if wire.base_flange.is_some() {
            present.push("base_flange");
        }
        if wire.path_feature.is_some() {
            present.push("path_feature");
        }
        if wire.work_plane_frame.is_some() {
            present.push("work_plane_frame");
        }
        if wire.joint_origin_frame.is_some() {
            present.push("joint_origin_frame");
        }
        if wire.sketch_entity.is_some() {
            present.push("sketch_entity");
        }
        if wire.solid_primitive.is_some() {
            present.push("solid_primitive");
        }
        if wire.direct_face_operation.is_some() {
            present.push("direct_face_operation");
        }
        if wire.align_envelope.is_some() {
            present.push("align_envelope");
        }
        if wire.move_operation.is_some() {
            present.push("move_operation");
        }
        if wire.scale_operation.is_some() {
            present.push("scale_operation");
        }
        if wire.surface_stitch_operation.is_some() {
            present.push("surface_stitch_operation");
        }
        if wire.surface_extend_operation.is_some() {
            present.push("surface_extend_operation");
        }
        if wire.surface_offset_operation.is_some() {
            present.push("surface_offset_operation");
        }
        if wire.ruled_surface_operation.is_some() {
            present.push("ruled_surface_operation");
        }
        if !wire.surface_patch_boundaries.is_empty() || wire.surface_patch_normal.is_some() {
            present.push("surface_patch");
        }
        if wire.edge_flange_operation.is_some() {
            present.push("edge_flange_operation");
        }
        if wire.hem_operation.is_some() {
            present.push("hem_operation");
        }
        if wire.fixed_fillet_parameters.is_some() {
            present.push("fixed_fillet_parameters");
        }
        if wire.fixed_chamfer_parameters.is_some() {
            present.push("fixed_chamfer_parameters");
        }
        if wire.combine_operation.is_some() {
            present.push("combine_operation");
        }
        if wire.emboss_construction.is_some() {
            present.push("emboss_construction");
        }
        if wire.thread_construction.is_some() {
            present.push("thread_construction");
        }
        if wire.draft_operation.is_some() {
            present.push("draft_operation");
        }
        if wire.circular_pattern_construction.is_some() {
            present.push("circular_pattern_construction");
        }
        if wire.rectangular_pattern_construction.is_some() {
            present.push("rectangular_pattern_construction");
        }
        if wire.assembly_alignment.is_some() {
            present.push("assembly_alignment");
        }
        if wire.component_insert_construction.is_some() {
            present.push("component_insert_construction");
        }
        if wire.derived_instance_construction.is_some() {
            present.push("derived_instance_construction");
        }
        if wire.copy_paste_component_operation.is_some() {
            present.push("copy_paste_component_operation");
        }
        if wire.copy_paste_bodies_operation.is_some() {
            present.push("copy_paste_bodies_operation");
        }
        if wire.mirror_construction.is_some() {
            present.push("mirror_construction");
        }
        if wire.base_feature_construction.is_some() {
            present.push("base_feature_construction");
        }
        if wire.work_axis_construction.is_some() {
            present.push("work_axis_construction");
        }
        if wire.work_point_construction.is_some() {
            present.push("work_point_construction");
        }
        if wire.hole_construction.is_some() || wire.hole_extent.is_some() {
            present.push("hole_construction");
        }
        if present.len() > 1 {
            return Err(DesignParameterScopePayloadError(format!(
                "design parameter scope carries more than one payload family: {}",
                present.join(", ")
            )));
        }
        let payload = match &wire.kind {
            DesignFeatureKind::Sketch => DesignScopePayload::Sketch(wire.sketch_entity.take()),
            DesignFeatureKind::Esquisse => DesignScopePayload::Esquisse(wire.sketch_entity.take()),
            DesignFeatureKind::Skizze => DesignScopePayload::Skizze(wire.sketch_entity.take()),
            DesignFeatureKind::Esboco => DesignScopePayload::Esboco(wire.sketch_entity.take()),
            DesignFeatureKind::Boceto => DesignScopePayload::Boceto(wire.sketch_entity.take()),
            DesignFeatureKind::SketchJa => DesignScopePayload::SketchJa(wire.sketch_entity.take()),
            DesignFeatureKind::Assemble => {
                DesignScopePayload::Assemble(wire.assembly_alignment.take())
            }
            DesignFeatureKind::AsBuilt => {
                DesignScopePayload::AsBuilt(wire.assembly_alignment.take())
            }
            DesignFeatureKind::Extrude => DesignScopePayload::Extrude(wire.extrude.take()),
            DesignFeatureKind::Extrusion => DesignScopePayload::Extrusion(wire.extrude.take()),
            DesignFeatureKind::Extrusao => DesignScopePayload::Extrusao(wire.extrude.take()),
            DesignFeatureKind::Extruir => DesignScopePayload::Extruir(wire.extrude.take()),
            DesignFeatureKind::ExtrudeJa => DesignScopePayload::ExtrudeJa(wire.extrude.take()),
            DesignFeatureKind::Fillet => {
                DesignScopePayload::Fillet(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Conge => {
                DesignScopePayload::Conge(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Abrundung => {
                DesignScopePayload::Abrundung(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Arredondamento => {
                DesignScopePayload::Arredondamento(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Empalme => {
                DesignScopePayload::Empalme(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::FilletJa => {
                DesignScopePayload::FilletJa(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Chamfer => {
                DesignScopePayload::Chamfer(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::Chanfrein => {
                DesignScopePayload::Chanfrein(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::Chaflan => {
                DesignScopePayload::Chaflan(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::ChamferJa => {
                DesignScopePayload::ChamferJa(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::Combine => {
                DesignScopePayload::Combine(wire.combine_operation.take())
            }
            DesignFeatureKind::Draft => DesignScopePayload::Draft(wire.draft_operation.take()),
            DesignFeatureKind::ReplaceFace => DesignScopePayload::ReplaceFace,
            DesignFeatureKind::CPattern => {
                DesignScopePayload::CPattern(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::CircularPattern => {
                DesignScopePayload::CircularPattern(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::ReseauC => {
                DesignScopePayload::ReseauC(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::RPattern => {
                DesignScopePayload::RPattern(wire.rectangular_pattern_construction.take())
            }
            DesignFeatureKind::RectangularPattern => {
                DesignScopePayload::RectangularPattern(wire.rectangular_pattern_construction.take())
            }
            DesignFeatureKind::Mirror => {
                DesignScopePayload::Mirror(wire.mirror_construction.take())
            }
            DesignFeatureKind::SymetrieMiroir => {
                DesignScopePayload::SymetrieMiroir(wire.mirror_construction.take())
            }
            DesignFeatureKind::Align => DesignScopePayload::Align(wire.align_envelope.take()),
            DesignFeatureKind::Move => DesignScopePayload::Move(wire.move_operation.take()),
            DesignFeatureKind::MoveFace => DesignScopePayload::MoveFace(wire.move_operation.take()),
            DesignFeatureKind::DeplacerFace => {
                DesignScopePayload::DeplacerFace(wire.move_operation.take())
            }
            DesignFeatureKind::OffsetFaces => {
                DesignScopePayload::OffsetFaces(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::OffsetFaces(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match OffsetFaces".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::DecalerLesFaces => {
                DesignScopePayload::DecalerLesFaces(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::OffsetFaces(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match DecalerLesFaces".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Revolve => {
                DesignScopePayload::Revolve(match wire.path_feature.take() {
                    None => None,
                    Some(DesignPathFeatureWire {
                        path_feature_construction:
                            Some(DesignPathFeatureConstruction::Revolve(value)),
                        sweep_profile: None,
                    }) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "path_feature_construction or sweep_profile does not match Revolve"
                                .into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Shell => {
                DesignScopePayload::Shell(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Shell(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match Shell".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Schale => {
                DesignScopePayload::Schale(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Shell(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match Schale".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Vaciado => {
                DesignScopePayload::Vaciado(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Shell(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match Vaciado".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Thicken => {
                DesignScopePayload::Thicken(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Thicken(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match Thicken".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::SpirePrimitive => {
                DesignScopePayload::SpirePrimitive(wire.coil.take())
            }
            DesignFeatureKind::CoilPrimitive => DesignScopePayload::CoilPrimitive(wire.coil.take()),
            DesignFeatureKind::Loft => DesignScopePayload::Loft(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Loft(value)),
                    sweep_profile: None,
                }) => Some(value),
                Some(_) => {
                    return Err(DesignParameterScopePayloadError(
                        "path_feature_construction or sweep_profile does not match Loft".into(),
                    ))
                }
            }),
            DesignFeatureKind::Solevacion => {
                DesignScopePayload::Solevacion(match wire.path_feature.take() {
                    None => None,
                    Some(DesignPathFeatureWire {
                        path_feature_construction: Some(DesignPathFeatureConstruction::Loft(value)),
                        sweep_profile: None,
                    }) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "path_feature_construction or sweep_profile does not match Solevación"
                                .into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Sweep => DesignScopePayload::Sweep(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire {
                    path_feature_construction,
                    sweep_profile,
                }) => {
                    let construction = match path_feature_construction {
                        None => None,
                        Some(DesignPathFeatureConstruction::Sweep(value)) => Some(value),
                        Some(_) => {
                            return Err(DesignParameterScopePayloadError(
                                "path_feature_construction.kind does not match Sweep".into(),
                            ))
                        }
                    };
                    Some(DesignSweepScope {
                        construction,
                        sweep_profile,
                    })
                }
            }),
            DesignFeatureKind::Pipe => DesignScopePayload::Pipe(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Pipe(value)),
                    sweep_profile: None,
                }) => Some(value),
                Some(_) => {
                    return Err(DesignParameterScopePayloadError(
                        "path_feature_construction or sweep_profile does not match Pipe".into(),
                    ))
                }
            }),
            DesignFeatureKind::SurfacePatch => {
                DesignScopePayload::SurfacePatch(DesignSurfacePatchScope {
                    boundaries: std::mem::take(&mut wire.surface_patch_boundaries),
                    normal: wire.surface_patch_normal.take(),
                })
            }
            DesignFeatureKind::SurfaceExtend => {
                DesignScopePayload::SurfaceExtend(wire.surface_extend_operation.take())
            }
            DesignFeatureKind::SurfaceOffset => {
                DesignScopePayload::SurfaceOffset(wire.surface_offset_operation.take())
            }
            DesignFeatureKind::SurfaceRuled => DesignScopePayload::SurfaceRuled(
                wire.ruled_surface_operation.take().ok_or_else(|| {
                    DesignParameterScopePayloadError("ruled_surface_operation is required".into())
                })?,
            ),
            DesignFeatureKind::SurfaceTrim => DesignScopePayload::SurfaceTrim,
            DesignFeatureKind::BoundaryFill => DesignScopePayload::BoundaryFill,
            DesignFeatureKind::Hole => {
                DesignScopePayload::Hole(crate::records::feature::hole::DesignHoleScope {
                    point: wire.hole_construction.take(),
                    extent: wire.hole_extent.take(),
                })
            }
            DesignFeatureKind::Split => DesignScopePayload::Split,
            DesignFeatureKind::Scale => DesignScopePayload::Scale(wire.scale_operation.take()),
            DesignFeatureKind::Massstab => {
                DesignScopePayload::Massstab(wire.scale_operation.take())
            }
            DesignFeatureKind::Escala => DesignScopePayload::Escala(wire.scale_operation.take()),
            DesignFeatureKind::Emboss => {
                DesignScopePayload::Emboss(wire.emboss_construction.take())
            }
            DesignFeatureKind::Thread => {
                DesignScopePayload::Thread(wire.thread_construction.take())
            }
            DesignFeatureKind::EdgeFlange => {
                DesignScopePayload::EdgeFlange(wire.edge_flange_operation.take())
            }
            DesignFeatureKind::Hem => DesignScopePayload::Hem(wire.hem_operation.take()),
            DesignFeatureKind::BaseFlange => {
                DesignScopePayload::BaseFlange(wire.base_flange.take())
            }
            DesignFeatureKind::ComponentInsert => {
                DesignScopePayload::ComponentInsert(wire.component_insert_construction.take())
            }
            DesignFeatureKind::CopyPaste => {
                DesignScopePayload::CopyPaste(wire.copy_paste_component_operation.take())
            }
            DesignFeatureKind::JointOrigin => {
                DesignScopePayload::JointOrigin(wire.joint_origin_frame.take())
            }
            DesignFeatureKind::Canvas => DesignScopePayload::Canvas,
            DesignFeatureKind::Lienzo => DesignScopePayload::Lienzo,
            DesignFeatureKind::Decal => DesignScopePayload::Decal,
            DesignFeatureKind::BaseMeshFeature => DesignScopePayload::BaseMeshFeature,
            DesignFeatureKind::WorkPlane => {
                DesignScopePayload::WorkPlane(wire.work_plane_frame.take())
            }
            DesignFeatureKind::PlanoDeObra => {
                DesignScopePayload::PlanoDeObra(wire.work_plane_frame.take())
            }
            DesignFeatureKind::WorkAxis => {
                DesignScopePayload::WorkAxis(wire.work_axis_construction.take())
            }
            DesignFeatureKind::WorkPoint => {
                DesignScopePayload::WorkPoint(wire.work_point_construction.take())
            }
            DesignFeatureKind::DerivedInstance => {
                DesignScopePayload::DerivedInstance(wire.derived_instance_construction.take())
            }
            DesignFeatureKind::CustomFeature => DesignScopePayload::CustomFeature,
            DesignFeatureKind::Form => DesignScopePayload::Form,
            DesignFeatureKind::SurfaceStitch => DesignScopePayload::SurfaceStitch(
                wire.surface_stitch_operation.take().ok_or_else(|| {
                    DesignParameterScopePayloadError("surface_stitch_operation is required".into())
                })?,
            ),
            DesignFeatureKind::BaseFeature => {
                DesignScopePayload::BaseFeature(wire.base_feature_construction.take())
            }
            DesignFeatureKind::CopyPasteBodies => {
                DesignScopePayload::CopyPasteBodies(wire.copy_paste_bodies_operation.take())
            }
            DesignFeatureKind::SplitFace => DesignScopePayload::SplitFace,
            DesignFeatureKind::DeleteFace => DesignScopePayload::DeleteFace,
            DesignFeatureKind::SurfaceDeleteFace => DesignScopePayload::SurfaceDeleteFace,
            DesignFeatureKind::RemoveBody => DesignScopePayload::RemoveBody,
            DesignFeatureKind::Face => DesignScopePayload::Face,
            DesignFeatureKind::SpherePrimitive => {
                DesignScopePayload::SpherePrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Sphere(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match SpherePrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::EsferaPrimitiva => {
                DesignScopePayload::EsferaPrimitiva(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Sphere(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match EsferaPrimitiva".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::TorusPrimitive => {
                DesignScopePayload::TorusPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Torus(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match TorusPrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::BoxPrimitive => {
                DesignScopePayload::BoxPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Box(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match BoxPrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::CylinderPrimitive => {
                DesignScopePayload::CylinderPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Cylinder(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match CylinderPrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Native(name) => DesignScopePayload::Native(name.clone()),
        };
        if wire.extrude.is_some()
            || wire.coil.is_some()
            || wire.base_flange.is_some()
            || wire.path_feature.is_some()
            || wire.work_plane_frame.is_some()
            || wire.joint_origin_frame.is_some()
            || wire.sketch_entity.is_some()
            || wire.solid_primitive.is_some()
            || wire.direct_face_operation.is_some()
            || wire.align_envelope.is_some()
            || wire.move_operation.is_some()
            || wire.scale_operation.is_some()
            || wire.surface_stitch_operation.is_some()
            || wire.surface_extend_operation.is_some()
            || wire.surface_offset_operation.is_some()
            || wire.ruled_surface_operation.is_some()
            || !wire.surface_patch_boundaries.is_empty()
            || wire.surface_patch_normal.is_some()
            || wire.edge_flange_operation.is_some()
            || wire.hem_operation.is_some()
            || wire.fixed_fillet_parameters.is_some()
            || wire.fixed_chamfer_parameters.is_some()
            || wire.combine_operation.is_some()
            || wire.emboss_construction.is_some()
            || wire.thread_construction.is_some()
            || wire.draft_operation.is_some()
            || wire.circular_pattern_construction.is_some()
            || wire.rectangular_pattern_construction.is_some()
            || wire.assembly_alignment.is_some()
            || wire.component_insert_construction.is_some()
            || wire.derived_instance_construction.is_some()
            || wire.copy_paste_component_operation.is_some()
            || wire.copy_paste_bodies_operation.is_some()
            || wire.mirror_construction.is_some()
            || wire.base_feature_construction.is_some()
            || wire.work_axis_construction.is_some()
            || wire.work_point_construction.is_some()
            || wire.hole_construction.is_some()
            || wire.hole_extent.is_some()
        {
            return Err(DesignParameterScopePayloadError(format!(
                "design parameter scope payload disagrees with kind {}",
                wire.kind
            )));
        }
        if wire.kind_offset.checked_sub(HISTORY_STATE_ID_BACK_OFFSET)
            != Some(wire.history_state_id_offset)
        {
            return Err(DesignParameterScopePayloadError(
                "history_state_id_offset disagrees with kind_offset".into(),
            ));
        }
        let occurrence_transfer = wire.occurrence_transfer.take();
        let mut admitted = Self::try_new(DesignParameterScopeDraft {
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire
                .class_tag
                .try_into()
                .map_err(DesignParameterScopePayloadError)?,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            kind_offset: wire.kind_offset,
            feature_ordinal: std::num::NonZeroU32::new(wire.feature_ordinal).ok_or_else(|| {
                DesignParameterScopePayloadError("feature_ordinal must be nonzero".into())
            })?,
            feature_ordinal_offset: wire.feature_ordinal_offset,
            history_state_id: wire.history_state_id,
            previous_history_state_id: wire.previous_history_state_id,
            previous_history_state_id_offset: wire.previous_history_state_id_offset,
            reference_count_offset: wire.reference_count_offset,
            reference_members: ReferenceRun::from_columns(
                wire.reference_members,
                wire.reference_member_offsets,
                "reference_members/reference_member_offsets",
            )
            .map_err(DesignParameterScopePayloadError)?,
            payload,
            unclosed_construction_operand_groups: wire.unclosed_construction_operand_groups,
            paired_class_tag: wire
                .paired_class_tag
                .try_into()
                .map_err(DesignParameterScopePayloadError)?,
            paired_byte_offset: wire.paired_byte_offset,
        })?;
        if occurrence_transfer
            .as_ref()
            .is_some_and(|value| !value.valid_for_scope(&admitted))
        {
            return Err(DesignParameterScopePayloadError(
                "occurrence_transfer disagrees with scope".into(),
            ));
        }
        admitted.source_kind = wire.source_kind;
        admitted.occurrence_transfer = occurrence_transfer;
        Ok(admitted)
    }
}

#[cfg(test)]
impl From<DesignParameterScope> for DesignParameterScopeSerde {
    fn from(scope: DesignParameterScope) -> Self {
        let kind = scope.kind();
        let history_state_id_offset = scope.history_state_id_offset();
        let (reference_members, reference_member_offsets) = scope.reference_members.into_wire();
        let mut wire = DesignParameterScopeSerde {
            occurrence_transfer: scope.occurrence_transfer,
            id: scope.id,
            byte_offset: scope.byte_offset,
            class_tag: scope.class_tag.into(),
            record_index: scope.record_index,
            frame_length: scope.frame_length,
            kind,
            source_kind: scope.source_kind,
            kind_offset: scope.kind_offset,
            extrude: None,
            coil: None,
            feature_ordinal: scope.feature_ordinal.get(),
            feature_ordinal_offset: scope.feature_ordinal_offset,
            history_state_id: scope.history_state_id,
            history_state_id_offset,
            previous_history_state_id: scope.previous_history_state_id,
            previous_history_state_id_offset: scope.previous_history_state_id_offset,
            reference_count_offset: scope.reference_count_offset,
            reference_members,
            reference_member_offsets,
            solid_primitive: None,
            direct_face_operation: None,
            align_envelope: None,
            move_operation: None,
            scale_operation: None,
            surface_stitch_operation: None,
            surface_extend_operation: None,
            surface_offset_operation: None,
            ruled_surface_operation: None,
            base_flange: None,
            surface_patch_boundaries: Vec::new(),
            surface_patch_normal: None,
            edge_flange_operation: None,
            hem_operation: None,
            fixed_fillet_parameters: None,
            fixed_chamfer_parameters: None,
            path_feature: None,
            combine_operation: None,
            thread_construction: None,
            emboss_construction: None,
            draft_operation: None,
            circular_pattern_construction: None,
            rectangular_pattern_construction: None,
            assembly_alignment: None,
            component_insert_construction: None,
            derived_instance_construction: None,
            copy_paste_component_operation: None,
            copy_paste_bodies_operation: None,
            mirror_construction: None,
            base_feature_construction: None,
            work_plane_frame: None,
            work_axis_construction: None,
            joint_origin_frame: None,
            work_point_construction: None,
            unclosed_construction_operand_groups: scope.unclosed_construction_operand_groups,
            hole_construction: None,
            hole_extent: None,
            sketch_entity: None,
            paired_class_tag: scope.paired_class_tag.into(),
            paired_byte_offset: scope.paired_byte_offset,
        };
        match scope.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value)
            | DesignScopePayload::Extruir(value)
            | DesignScopePayload::ExtrudeJa(value) => wire.extrude = value,
            DesignScopePayload::SpirePrimitive(value)
            | DesignScopePayload::CoilPrimitive(value) => wire.coil = value,
            DesignScopePayload::BaseFlange(value) => wire.base_flange = value,
            DesignScopePayload::Revolve(value) => {
                wire.path_feature = value.map(|value| DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Revolve(value)),
                    sweep_profile: None,
                });
            }
            DesignScopePayload::Loft(value) | DesignScopePayload::Solevacion(value) => {
                wire.path_feature = value.map(|value| DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Loft(value)),
                    sweep_profile: None,
                });
            }
            DesignScopePayload::Sweep(value) => {
                wire.path_feature = value.map(|sweep| DesignPathFeatureWire {
                    path_feature_construction: sweep
                        .construction
                        .map(DesignPathFeatureConstruction::Sweep),
                    sweep_profile: sweep.sweep_profile,
                });
            }
            DesignScopePayload::Pipe(value) => {
                wire.path_feature = value.map(|value| DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Pipe(value)),
                    sweep_profile: None,
                });
            }
            DesignScopePayload::WorkPlane(value) | DesignScopePayload::PlanoDeObra(value) => {
                wire.work_plane_frame = value;
            }
            DesignScopePayload::JointOrigin(value) => wire.joint_origin_frame = value,
            DesignScopePayload::Sketch(value)
            | DesignScopePayload::Esquisse(value)
            | DesignScopePayload::Skizze(value)
            | DesignScopePayload::Esboco(value)
            | DesignScopePayload::Boceto(value)
            | DesignScopePayload::SketchJa(value) => wire.sketch_entity = value,
            DesignScopePayload::SpherePrimitive(value)
            | DesignScopePayload::EsferaPrimitiva(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Sphere);
            }
            DesignScopePayload::TorusPrimitive(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Torus);
            }
            DesignScopePayload::BoxPrimitive(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Box);
            }
            DesignScopePayload::CylinderPrimitive(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Cylinder);
            }
            DesignScopePayload::ReplaceFace => {}
            DesignScopePayload::OffsetFaces(value) | DesignScopePayload::DecalerLesFaces(value) => {
                wire.direct_face_operation = value.map(DesignDirectFaceOperation::OffsetFaces);
            }
            DesignScopePayload::Shell(value)
            | DesignScopePayload::Schale(value)
            | DesignScopePayload::Vaciado(value) => {
                wire.direct_face_operation = value.map(DesignDirectFaceOperation::Shell);
            }
            DesignScopePayload::Thicken(value) => {
                wire.direct_face_operation = value.map(DesignDirectFaceOperation::Thicken);
            }
            DesignScopePayload::Align(value) => wire.align_envelope = value,
            DesignScopePayload::Move(value)
            | DesignScopePayload::MoveFace(value)
            | DesignScopePayload::DeplacerFace(value) => {
                wire.move_operation = value;
            }
            DesignScopePayload::Scale(value)
            | DesignScopePayload::Massstab(value)
            | DesignScopePayload::Escala(value) => {
                wire.scale_operation = value;
            }
            DesignScopePayload::SurfaceStitch(value) => wire.surface_stitch_operation = Some(value),
            DesignScopePayload::SurfaceExtend(value) => wire.surface_extend_operation = value,
            DesignScopePayload::SurfaceOffset(value) => wire.surface_offset_operation = value,
            DesignScopePayload::SurfaceRuled(value) => wire.ruled_surface_operation = Some(value),
            DesignScopePayload::SurfacePatch(value) => {
                wire.surface_patch_boundaries = value.boundaries;
                wire.surface_patch_normal = value.normal;
            }
            DesignScopePayload::EdgeFlange(value) => wire.edge_flange_operation = value,
            DesignScopePayload::Hem(value) => wire.hem_operation = value,
            DesignScopePayload::Fillet(value)
            | DesignScopePayload::Conge(value)
            | DesignScopePayload::Abrundung(value)
            | DesignScopePayload::Arredondamento(value)
            | DesignScopePayload::Empalme(value)
            | DesignScopePayload::FilletJa(value) => wire.fixed_fillet_parameters = value,
            DesignScopePayload::Chamfer(value)
            | DesignScopePayload::Chanfrein(value)
            | DesignScopePayload::Chaflan(value)
            | DesignScopePayload::ChamferJa(value) => {
                wire.fixed_chamfer_parameters = value;
            }
            DesignScopePayload::Combine(value) => wire.combine_operation = value,
            DesignScopePayload::Thread(value) => wire.thread_construction = value,
            DesignScopePayload::Emboss(value) => wire.emboss_construction = value,
            DesignScopePayload::Draft(value) => wire.draft_operation = value,
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => wire.circular_pattern_construction = value,
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                wire.rectangular_pattern_construction = value;
            }
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                wire.assembly_alignment = value;
            }
            DesignScopePayload::ComponentInsert(value) => {
                wire.component_insert_construction = value;
            }
            DesignScopePayload::DerivedInstance(value) => {
                wire.derived_instance_construction = value;
            }
            DesignScopePayload::CopyPaste(value) => wire.copy_paste_component_operation = value,
            DesignScopePayload::CopyPasteBodies(value) => wire.copy_paste_bodies_operation = value,
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                wire.mirror_construction = value;
            }
            DesignScopePayload::BaseFeature(value) => wire.base_feature_construction = value,
            DesignScopePayload::WorkAxis(value) => wire.work_axis_construction = value,
            DesignScopePayload::WorkPoint(value) => wire.work_point_construction = value,
            DesignScopePayload::Hole(value) => {
                wire.hole_construction = value.point;
                wire.hole_extent = value.extent;
            }
            DesignScopePayload::SurfaceTrim
            | DesignScopePayload::BoundaryFill
            | DesignScopePayload::Split
            | DesignScopePayload::Canvas
            | DesignScopePayload::Lienzo
            | DesignScopePayload::Decal
            | DesignScopePayload::BaseMeshFeature
            | DesignScopePayload::CustomFeature
            | DesignScopePayload::Form
            | DesignScopePayload::SplitFace
            | DesignScopePayload::DeleteFace
            | DesignScopePayload::SurfaceDeleteFace
            | DesignScopePayload::RemoveBody
            | DesignScopePayload::Face
            | DesignScopePayload::Native(_) => {}
        }
        if base_flange_scope_is_absent(wire.base_flange.as_ref()) {
            wire.base_flange = None;
        }
        if coil_scope_is_absent(wire.coil.as_ref()) {
            wire.coil = None;
        }
        if extrude_scope_is_absent(wire.extrude.as_ref()) {
            wire.extrude = None;
        }
        if path_feature_scope_is_absent(wire.path_feature.as_ref()) {
            wire.path_feature = None;
        }
        wire
    }
}

/// Reference-table entry width of a scope draft: 11 for plain references or
/// 51 for typed references. The last entry's target is followed by the rest
/// of its reference, the u32 history-state identity, and the kind length.
fn reference_table_stride(draft: &DesignParameterScopeDraft) -> Option<usize> {
    let last = draft.reference_members.offsets().next_back()?;
    let count = u64_from_index(draft.reference_members.len());
    [11, crate::bytes::TYPED_LOCAL_REFERENCE_LEN]
        .into_iter()
        .find(|stride| {
            let stride = u64_from_index(*stride);
            last.checked_add(stride + 7) == Some(draft.kind_offset)
                && draft
                    .reference_count_offset
                    .checked_add(5)
                    .zip(count.checked_sub(1).and_then(|n| n.checked_mul(stride)))
                    .and_then(|(first, span)| first.checked_add(span))
                    == Some(*last)
        })
}

impl DesignParameterScope {
    pub(crate) fn try_new(
        draft: DesignParameterScopeDraft,
    ) -> Result<Self, DesignParameterScopePayloadError> {
        let fail = |field: &str| {
            DesignParameterScopePayloadError(format!("invalid parameter scope {field}"))
        };
        let kind = draft.payload.kind_name();
        if let DesignScopePayload::SurfacePatch(DesignSurfacePatchScope {
            normal: Some(DesignSurfacePatchNormalState::Explicit { reversed }),
            ..
        }) = &draft.payload
        {
            let relative =
                u64_from_index(crate::layout::surface_patch_v7_normal_flip_prefix::NORMAL_FLIP);
            if draft.byte_offset.checked_add(relative) != Some(reversed.offset)
                || reversed.offset >= draft.paired_byte_offset
            {
                return Err(fail("surface_patch_normal"));
            }
        }
        if draft.frame_length <= 89
            || draft.byte_offset.checked_add(draft.frame_length) != Some(draft.paired_byte_offset)
        {
            return Err(fail("frame_length/paired_byte_offset"));
        }
        if !(draft.byte_offset < draft.kind_offset
            && draft.kind_offset < draft.feature_ordinal_offset)
        {
            return Err(fail("kind_offset/feature_ordinal_offset"));
        }
        let history_state_id_offset = draft
            .kind_offset
            .checked_sub(HISTORY_STATE_ID_BACK_OFFSET)
            .ok_or_else(|| fail("kind_offset"))?;
        let tail = draft
            .paired_byte_offset
            .checked_sub(draft.feature_ordinal_offset)
            .and_then(|length| usize::try_from(length).ok())
            .ok_or_else(|| fail("feature_ordinal_offset"))?;
        // The reference-table stride shows the reference spelling: 11-byte
        // plain references or 51-byte typed references.
        // A table that fits neither width falls through to the plain checks.
        let stride = reference_table_stride(&draft).unwrap_or(11);
        let typed = stride == crate::bytes::TYPED_LOCAL_REFERENCE_LEN
            || (draft.reference_members.is_empty()
                && kind == "As-built"
                && tail == crate::layout::typed_reference_parameter_scope_tail::LEN);
        let typed_canvas =
            typed && kind == "Canvas" && tail == crate::layout::canvas_scope_typed_tail::LEN;
        let tail_valid = if typed_canvas {
            true
        } else if typed {
            tail == crate::layout::typed_reference_parameter_scope_tail::LEN
        } else {
            crate::design::decode::scopes::parameter_scope::parameter_scope_tail_length_is_valid(
                kind, tail,
            )
        };
        if !tail_valid {
            return Err(fail("feature_ordinal_offset/kind"));
        }
        match draft.previous_history_state_id_offset {
            None if draft.previous_history_state_id.is_none() => {}
            Some(offset) => {
                let relatives = if typed_canvas {
                    Vec::new()
                } else if typed {
                    vec![crate::layout::typed_reference_parameter_scope_tail::PREVIOUS_HISTORY_STATE_ID]
                } else {
                    [false, true]
                        .into_iter()
                        .filter_map(|second_tail_reference| {
                            crate::design::decode::scopes::parameter_scope::parameter_scope_previous_history_offset(
                                kind,
                                tail,
                                draft.reference_members.is_empty(),
                                second_tail_reference,
                            )
                        })
                        .collect()
                };
                if relatives.is_empty() {
                    return Err(fail("previous_history_state_id_offset"));
                }
                // A scope that commits no state of its own can still record
                // the state it follows.
                if !relatives.iter().any(|relative| {
                    draft
                        .feature_ordinal_offset
                        .checked_add(u64_from_index(*relative))
                        == Some(offset)
                }) || (draft.history_state_id.is_some()
                    && draft.previous_history_state_id.is_none())
                {
                    return Err(fail("previous_history_state_id_offset/history_state_id/previous_history_state_id"));
                }
            }
            None => {
                return Err(fail(
                    "previous_history_state_id_offset/previous_history_state_id",
                ))
            }
        }
        let empty_table_at = (kind == "As-built")
            .then(|| {
                crate::design::assembly::RigidGroupLayout::new(
                    draft.frame_length,
                    u64_from_index(tail),
                )
            })
            .flatten()
            .and_then(|layout| {
                draft
                    .byte_offset
                    .checked_add(u64_from_index(layout.empty_table))
            });
        if !(draft.byte_offset < draft.reference_count_offset
            && draft.reference_count_offset < draft.kind_offset)
            || (draft.reference_members.is_empty()
                && empty_table_at != Some(draft.reference_count_offset))
        {
            return Err(fail("reference_count_offset/reference_members"));
        }
        let mut expected = draft
            .reference_count_offset
            .checked_add(5)
            .ok_or_else(|| fail("reference_count_offset"))?;
        for member in draft.reference_members.offsets() {
            if *member != expected
                || *member <= draft.reference_count_offset
                || *member >= draft.kind_offset
            {
                return Err(fail("reference_member_offsets"));
            }
            expected = expected
                .checked_add(u64_from_index(stride))
                .ok_or_else(|| fail("reference_member_offsets"))?;
        }
        // An empty table is its count, then the history state and the kind's
        // length prefix.
        let table_end = match draft.reference_members.offsets().next_back() {
            Some(offset) => offset.checked_add(u64_from_index(stride) + 7),
            None => draft.reference_count_offset.checked_add(12),
        };
        if draft.reference_members.offsets().count() != draft.reference_members.len()
            || table_end != Some(draft.kind_offset)
        {
            return Err(fail("reference_member_offsets/kind_offset"));
        }
        if let DesignScopePayload::Pipe(Some(pipe)) = &draft.payload {
            if !pipe.scope_offsets_are_valid(draft.byte_offset, draft.reference_count_offset) {
                return Err(fail("Pipe prefix offsets"));
            }
        }
        let scope = Self {
            #[cfg(test)]
            clone_probe: ScopeCloneProbe,
            id: draft.id,
            byte_offset: draft.byte_offset,
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            frame_length: draft.frame_length,
            kind_offset: draft.kind_offset,
            source_kind: None,
            history_state_id_offset,
            feature_ordinal: draft.feature_ordinal,
            feature_ordinal_offset: draft.feature_ordinal_offset,
            history_state_id: draft.history_state_id,
            previous_history_state_id: draft.previous_history_state_id,
            previous_history_state_id_offset: draft.previous_history_state_id_offset,
            reference_count_offset: draft.reference_count_offset,
            reference_members: draft.reference_members,
            occurrence_transfer: None,
            payload: draft.payload,
            unclosed_construction_operand_groups: draft.unclosed_construction_operand_groups,
            paired_class_tag: draft.paired_class_tag,
            paired_byte_offset: draft.paired_byte_offset,
        };
        if scope
            .emboss_construction()
            .is_some_and(|construction| !construction.links_match(&scope))
        {
            return Err(fail("emboss_construction"));
        }
        Ok(scope)
    }

    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignParameterScopeDraft {
        DesignParameterScopeDraft {
            id: self.id,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag,
            record_index: self.record_index,
            frame_length: self.frame_length,
            kind_offset: self.kind_offset,
            feature_ordinal: self.feature_ordinal,
            feature_ordinal_offset: self.feature_ordinal_offset,
            history_state_id: self.history_state_id,
            previous_history_state_id: self.previous_history_state_id,
            previous_history_state_id_offset: self.previous_history_state_id_offset,
            reference_count_offset: self.reference_count_offset,
            reference_members: self.reference_members,
            payload: self.payload,
            unclosed_construction_operand_groups: self.unclosed_construction_operand_groups,
            paired_class_tag: self.paired_class_tag,
            paired_byte_offset: self.paired_byte_offset,
        }
    }

    #[cfg(test)]
    pub(crate) fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut DesignParameterScopeDraft),
    ) -> Result<(), DesignParameterScopePayloadError> {
        let mut draft = self.clone().into_draft();
        edit(&mut draft);
        *self = Self::try_new(draft)?;
        Ok(())
    }

    pub(crate) fn payload_mut(&mut self) -> DesignScopePayloadMut<'_> {
        self.payload.fields_mut()
    }

    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn frame_length(&self) -> u64 {
        self.frame_length
    }
    pub(crate) fn kind_offset(&self) -> u64 {
        self.kind_offset
    }
    pub(crate) fn feature_ordinal_offset(&self) -> u64 {
        self.feature_ordinal_offset
    }
    pub(crate) fn history_state_id(&self) -> Option<i64> {
        self.history_state_id
    }
    pub(crate) fn previous_history_state_id(&self) -> Option<i64> {
        self.previous_history_state_id
    }
    pub(crate) fn previous_history_state_id_offset(&self) -> Option<u64> {
        self.previous_history_state_id_offset
    }
    pub(crate) fn reference_count_offset(&self) -> u64 {
        self.reference_count_offset
    }
    pub(crate) fn reference_members(&self) -> &ReferenceRun<u32> {
        &self.reference_members
    }
    /// Whether the ordered reference table names `owner`, so that the
    /// parameter owner belongs to this scope.
    pub(crate) fn names_owner(
        &self,
        owner: &crate::records::parameters::DesignParameterOwner,
    ) -> bool {
        self.record_index == owner.scope_record_index()
            && self
                .reference_members
                .values()
                .any(|record_index| *record_index == owner.record_index())
    }
    pub(crate) fn payload(&self) -> &DesignScopePayload {
        &self.payload
    }
    pub(crate) fn paired_byte_offset(&self) -> u64 {
        self.paired_byte_offset
    }
}

impl DesignParameterScope {
    /// Byte offset of the state word before the length-prefixed kind name.
    fn history_state_id_offset(&self) -> u64 {
        self.history_state_id_offset
    }

    /// Source feature-family name, derived from its construction variant.
    pub(crate) fn kind(&self) -> DesignFeatureKind {
        self.payload.kind()
    }

    /// Stored display spelling of the family name.
    pub(crate) fn source_kind_name(&self) -> &str {
        self.source_kind
            .as_deref()
            .unwrap_or_else(|| self.kind_name())
    }

    /// Selected family name without allocating a kind tag.
    pub(crate) fn kind_name(&self) -> &str {
        self.payload.kind_name()
    }

    fn extrude(&self) -> Option<&DesignExtrudeScope> {
        match &self.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value)
            | DesignScopePayload::Extruir(value)
            | DesignScopePayload::ExtrudeJa(value) => value.as_ref(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn extrude_mut(&mut self) -> Option<&mut DesignExtrudeScope> {
        match &mut self.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value)
            | DesignScopePayload::Extruir(value)
            | DesignScopePayload::ExtrudeJa(value) => value.as_mut(),
            _ => None,
        }
    }

    fn coil(&self) -> Option<&DesignCoilScope> {
        match &self.payload {
            DesignScopePayload::SpirePrimitive(value)
            | DesignScopePayload::CoilPrimitive(value) => value.as_ref(),
            _ => None,
        }
    }

    fn base_flange(&self) -> Option<&DesignBaseFlangeScope> {
        match &self.payload {
            DesignScopePayload::BaseFlange(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_plane_frame(&self) -> Option<&DesignWorkPlaneTransform> {
        match &self.payload {
            DesignScopePayload::WorkPlane(value) | DesignScopePayload::PlanoDeObra(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn work_plane_frame_mut(&mut self) -> Option<&mut DesignWorkPlaneTransform> {
        match &mut self.payload {
            DesignScopePayload::WorkPlane(value) | DesignScopePayload::PlanoDeObra(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn joint_origin_frame(&self) -> Option<&DesignJointOriginTransform> {
        match &self.payload {
            DesignScopePayload::JointOrigin(value) => value.as_ref(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn joint_origin_frame_mut(&mut self) -> Option<&mut DesignJointOriginTransform> {
        match &mut self.payload {
            DesignScopePayload::JointOrigin(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn sketch_entity(&self) -> Option<&DesignSketchEntityBinding> {
        match &self.payload {
            DesignScopePayload::Sketch(value)
            | DesignScopePayload::Esquisse(value)
            | DesignScopePayload::Skizze(value)
            | DesignScopePayload::Esboco(value)
            | DesignScopePayload::Boceto(value)
            | DesignScopePayload::SketchJa(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn align_envelope(&self) -> Option<&DesignAlignEnvelope> {
        match &self.payload {
            DesignScopePayload::Align(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn move_operation(&self) -> Option<&DesignMoveOperation> {
        match &self.payload {
            DesignScopePayload::Move(value)
            | DesignScopePayload::MoveFace(value)
            | DesignScopePayload::DeplacerFace(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn scale_operation(&self) -> Option<&DesignScaleOperation> {
        match &self.payload {
            DesignScopePayload::Scale(value)
            | DesignScopePayload::Massstab(value)
            | DesignScopePayload::Escala(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn surface_stitch_operation(&self) -> Option<&DesignSurfaceStitchOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceStitch(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn surface_extend_operation(&self) -> Option<&DesignSurfaceExtendOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceExtend(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn surface_offset_operation(&self) -> Option<&DesignSurfaceOffsetOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceOffset(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn ruled_surface_operation(&self) -> Option<&DesignRuledSurfaceOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceRuled(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn edge_flange_operation(&self) -> Option<&DesignEdgeFlangeOperation> {
        match &self.payload {
            DesignScopePayload::EdgeFlange(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn hem_operation(&self) -> Option<&DesignHemOperation> {
        match &self.payload {
            DesignScopePayload::Hem(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn fixed_fillet_parameters(&self) -> Option<&DesignFixedFilletParameters> {
        match &self.payload {
            DesignScopePayload::Fillet(value)
            | DesignScopePayload::Conge(value)
            | DesignScopePayload::Abrundung(value)
            | DesignScopePayload::Arredondamento(value)
            | DesignScopePayload::Empalme(value)
            | DesignScopePayload::FilletJa(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn fixed_chamfer_parameters(&self) -> Option<&DesignFixedChamferParameters> {
        match &self.payload {
            DesignScopePayload::Chamfer(value)
            | DesignScopePayload::Chanfrein(value)
            | DesignScopePayload::Chaflan(value)
            | DesignScopePayload::ChamferJa(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn combine_operation(&self) -> Option<&DesignCombineOperation> {
        match &self.payload {
            DesignScopePayload::Combine(value) => value.as_ref(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn combine_operation_mut(&mut self) -> Option<&mut DesignCombineOperation> {
        match &mut self.payload {
            DesignScopePayload::Combine(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn emboss_construction(&self) -> Option<&DesignEmbossConstruction> {
        match &self.payload {
            DesignScopePayload::Emboss(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn emboss_operand_construction(&self) -> Option<&DesignEmbossConstruction> {
        self.emboss_construction()
            .filter(|construction| construction.has_fixed_operands(self))
    }

    pub(crate) fn thread_construction(&self) -> Option<&DesignThreadConstruction> {
        match &self.payload {
            DesignScopePayload::Thread(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn draft_operation(&self) -> Option<&DesignDraftOperation> {
        match &self.payload {
            DesignScopePayload::Draft(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn circular_pattern_construction(
        &self,
    ) -> Option<&DesignCircularPatternConstruction> {
        match &self.payload {
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn circular_pattern_construction_mut(
        &mut self,
    ) -> Option<&mut DesignCircularPatternConstruction> {
        match &mut self.payload {
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn rectangular_pattern_construction(
        &self,
    ) -> Option<&DesignRectangularPatternConstruction> {
        match &self.payload {
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn rectangular_pattern_construction_mut(
        &mut self,
    ) -> Option<&mut DesignRectangularPatternConstruction> {
        match &mut self.payload {
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn assembly_alignment(&self) -> Option<&DesignAssemblyAlignment> {
        match &self.payload {
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn assembly_alignment_mut(&mut self) -> Option<&mut DesignAssemblyAlignment> {
        match &mut self.payload {
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn component_insert_construction(
        &self,
    ) -> Option<&DesignComponentInsertConstruction> {
        match &self.payload {
            DesignScopePayload::ComponentInsert(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn derived_instance_construction(
        &self,
    ) -> Option<&DesignDerivedInstanceConstruction> {
        match &self.payload {
            DesignScopePayload::DerivedInstance(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_component_operation(
        &self,
    ) -> Option<&DesignCopyPasteComponentOperation> {
        match &self.payload {
            DesignScopePayload::CopyPaste(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_bodies_operation(&self) -> Option<&DesignCopyPasteBodiesOperation> {
        match &self.payload {
            DesignScopePayload::CopyPasteBodies(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn mirror_construction(&self) -> Option<&DesignMirrorConstruction> {
        match &self.payload {
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn mirror_construction_mut(&mut self) -> Option<&mut DesignMirrorConstruction> {
        match &mut self.payload {
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn base_feature_construction(&self) -> Option<&DesignBaseFeatureConstruction> {
        match &self.payload {
            DesignScopePayload::BaseFeature(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_axis_construction(&self) -> Option<&DesignWorkAxisConstruction> {
        match &self.payload {
            DesignScopePayload::WorkAxis(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_point_construction(&self) -> Option<&DesignWorkPointConstruction> {
        match &self.payload {
            DesignScopePayload::WorkPoint(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_point_construction_mut(
        &mut self,
    ) -> Option<&mut DesignWorkPointConstruction> {
        match &mut self.payload {
            DesignScopePayload::WorkPoint(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn hole_extent(&self) -> Option<&crate::records::feature::hole::DesignHoleExtent> {
        match &self.payload {
            DesignScopePayload::Hole(value) => value.extent.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn hole_construction(&self) -> Option<&DesignHoleConstruction> {
        match &self.payload {
            DesignScopePayload::Hole(value) => value.point.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn hole_construction_mut(&mut self) -> Option<&mut DesignHoleConstruction> {
        match &mut self.payload {
            DesignScopePayload::Hole(value) => value.point.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn surface_patch_normal(&self) -> Option<DesignSurfacePatchNormalState> {
        match &self.payload {
            DesignScopePayload::SurfacePatch(value) => value.normal,
            _ => None,
        }
    }

    pub(crate) fn surface_patch_boundaries(&self) -> &[DesignSurfacePatchBoundary] {
        match &self.payload {
            DesignScopePayload::SurfacePatch(value) => &value.boundaries,
            _ => &[],
        }
    }

    pub(crate) fn extrude_prologue(&self) -> Option<DesignExtrudePrologue> {
        self.extrude().and_then(|extrude| extrude.extrude_prologue)
    }

    #[cfg(test)]
    pub(crate) fn extrude_prologue_mut(&mut self) -> Option<&mut DesignExtrudePrologue> {
        self.extrude_mut()
            .and_then(|extrude| extrude.extrude_prologue.as_mut())
    }

    pub(crate) fn extrude_profile(&self) -> Option<&DesignSketchProfileOperand> {
        self.extrude()
            .and_then(|extrude| extrude.extrude_profile.as_ref())
    }

    pub(crate) fn extrude_native_profiles(&self) -> Option<&[DesignSketchProfileOperand]> {
        self.extrude()
            .and_then(|extrude| extrude.extrude_native_profiles.as_deref())
    }

    #[cfg(test)]
    pub(crate) fn extrude_profile_mut(&mut self) -> Option<&mut DesignSketchProfileOperand> {
        self.extrude_mut()
            .and_then(|extrude| extrude.extrude_profile.as_mut())
    }

    pub(crate) fn fixed_extrude_parameters(&self) -> Option<&DesignFixedExtrudeParameters> {
        self.extrude()
            .and_then(|extrude| extrude.fixed_extrude_parameters.as_ref())
    }

    pub(crate) fn base_flange_operation(&self) -> Option<&DesignBaseFlangeOperation> {
        self.base_flange()
            .and_then(|base_flange| base_flange.base_flange_operation.as_ref())
    }

    pub(crate) fn base_flange_profile(&self) -> Option<&DesignSketchProfileOperand> {
        self.base_flange()
            .and_then(|base_flange| base_flange.base_flange_profile.as_ref())
    }

    pub(crate) fn coil_operation(&self) -> Option<DesignExtrudeOperation> {
        self.coil()
            .and_then(|coil| coil.operation.map(|field| field.value))
    }

    pub(crate) fn coil_operation_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.operation.map(|field| field.offset))
    }

    pub(crate) fn coil_extent(&self) -> Option<DesignCoilExtent> {
        self.coil()
            .and_then(|coil| coil.extent.map(|field| field.value()))
    }

    pub(crate) fn coil_section(&self) -> Option<DesignCoilSection> {
        self.coil()
            .and_then(|coil| coil.section.map(|field| field.value()))
    }

    pub(crate) fn coil_section_placement(&self) -> Option<DesignCoilSectionPlacement> {
        self.coil()
            .and_then(|coil| coil.section_placement.map(|field| field.value()))
    }

    pub(crate) fn coil_clockwise(&self) -> Option<bool> {
        self.coil()
            .and_then(|coil| coil.clockwise.map(|field| field.value()))
    }

    #[cfg(test)]
    pub(crate) fn coil_extent_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.extent.and_then(|field| field.offset()))
    }

    #[cfg(test)]
    pub(crate) fn coil_section_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.section.and_then(|field| field.offset()))
    }

    #[cfg(test)]
    pub(crate) fn coil_section_placement_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.section_placement.and_then(|field| field.offset()))
    }

    #[cfg(test)]
    pub(crate) fn coil_clockwise_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.clockwise.and_then(|field| field.offset()))
    }

    pub(crate) fn coil_placement(&self) -> Option<&DesignCoilPlacement> {
        self.coil().and_then(|coil| coil.placement.as_ref())
    }

    pub(crate) fn coil_transform(&self) -> Option<&DesignCoilTransform> {
        self.coil().and_then(|coil| coil.transform.as_ref())
    }

    pub(crate) fn has_path_construction(&self) -> bool {
        match &self.payload {
            DesignScopePayload::Revolve(value) => value.is_some(),
            DesignScopePayload::Loft(value) | DesignScopePayload::Solevacion(value) => {
                value.is_some()
            }
            DesignScopePayload::Pipe(value) => value.is_some(),
            DesignScopePayload::Sweep(value) => value
                .as_ref()
                .is_some_and(|sweep| sweep.construction.is_some()),
            _ => false,
        }
    }

    pub(crate) fn sweep_profile(&self) -> Option<&DesignSketchProfileOperand> {
        match &self.payload {
            DesignScopePayload::Sweep(value) => value
                .as_ref()
                .and_then(|sweep| sweep.sweep_profile.as_ref()),
            _ => None,
        }
    }

    pub(crate) fn work_plane_transform(&self) -> Option<SketchPlacementMatrix> {
        self.work_plane_frame()
            .map(|frame| frame.work_plane_transform)
    }

    pub(crate) fn work_plane_reference(&self) -> Option<u32> {
        self.work_plane_frame()
            .and_then(|frame| frame.reference.as_ref().map(|r| r.work_plane_reference))
    }

    pub(crate) fn work_plane_construction(&self) -> Option<&DesignWorkPlaneConstruction> {
        self.work_plane_frame()
            .and_then(|frame| frame.work_plane_construction.as_ref())
    }

    pub(crate) fn work_plane_construction_mut(
        &mut self,
    ) -> Option<&mut DesignWorkPlaneConstruction> {
        self.work_plane_frame_mut()
            .and_then(|frame| frame.work_plane_construction.as_mut())
    }

    pub(crate) fn joint_origin_transform(&self) -> Option<SketchPlacementMatrix> {
        self.joint_origin_frame()
            .map(|frame| frame.joint_origin_transform)
    }

    pub(crate) fn joint_origin_transform_offset(&self) -> Option<u64> {
        self.joint_origin_frame()
            .map(|frame| frame.joint_origin_transform_offset)
    }

    #[cfg(test)]
    pub(crate) fn joint_origin_reference(&self) -> Option<u32> {
        self.joint_origin_frame().and_then(|frame| {
            frame
                .reference
                .as_ref()
                .map(|reference| reference.joint_origin_reference)
        })
    }

    #[cfg(test)]
    pub(crate) fn joint_origin_reference_offset(&self) -> Option<u64> {
        self.joint_origin_frame().and_then(|frame| {
            frame
                .reference
                .as_ref()
                .map(|reference| reference.joint_origin_reference_offset)
        })
    }
}

#[cfg(test)]
impl DesignParameterScope {
    /// Build a scope carrying only its identity, kind, and record index.
    pub(crate) fn with_work_plane_transform(&mut self, transform: SketchPlacementMatrix) {
        self.payload = DesignScopePayload::WorkPlane(Some(DesignWorkPlaneTransform {
            work_plane_transform: transform,
            work_plane_transform_offset: 0,
            reference: None,
            work_plane_construction: None,
        }));
    }

    pub(crate) fn with_work_plane_reference(&mut self, record_index: u32) {
        if let Some(frame) = self.work_plane_frame_mut() {
            frame.reference = Some(DesignWorkPlaneReference {
                work_plane_reference: record_index,
                work_plane_reference_offset: 0,
            });
        }
    }

    pub(crate) fn with_joint_origin_transform(&mut self, transform: SketchPlacementMatrix) {
        self.payload = DesignScopePayload::JointOrigin(Some(DesignJointOriginTransform {
            joint_origin_transform: transform,
            joint_origin_transform_offset: 0,
            reference: None,
        }));
    }

    pub(crate) fn empty<P>(id: &str, payload: P, record_index: u32) -> Self
    where
        P: TryInto<DesignScopePayload>,
        P::Error: std::fmt::Debug,
    {
        Self::try_new(DesignParameterScopeDraft {
            id: id.to_string(),
            byte_offset: 0,
            class_tag: DesignClassTag::try_from("256".to_owned()).unwrap(),
            record_index,
            frame_length: 128,
            kind_offset: 32,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 48,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 9,
            reference_members: ReferenceRun::located(vec![crate::records::identity::Located {
                value: record_index,
                offset: 14,
            }]),
            payload: payload.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: DesignClassTag::try_from("257".to_owned()).unwrap(),
            paired_byte_offset: 128,
        })
        .unwrap()
    }
}

#[cfg(test)]
mod tests;

mod serialize;

mod identity_rewrite;
