// SPDX-License-Identifier: Apache-2.0
//! Byte and predicate helpers shared by the source-less generator and the
//! edit-and-patch engine.

use crate::native::F3dNative;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::Sense;

pub(super) fn f3d_native(ir: &CadIr) -> Result<Option<F3dNative>, CodecError> {
    ir.native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(Into::into)
}

pub(super) fn validate_configuration_projection(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let projected = crate::design::configurations::project_configurations(
        &native.design_configurations,
        &crate::design::configurations::ConfigurationTargets {
            parameters: &native.design_parameters,
            owners: &native.design_parameter_owners,
            scopes: &native.design_parameter_scopes,
            neutral_parameters: &target.model.parameters,
            neutral_features: &target.model.features,
        },
    )?;
    if target.model.configurations != projected {
        return Err(CodecError::Malformed(
            "neutral F3D configurations must equal the projection of native configuration tables"
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_assembly_projection(
    target: &CadIr,
    native: Option<&F3dNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return target
            .model
            .assembly_joints
            .is_empty()
            .then_some(())
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "source-less F3D generation does not support assembly joints".into(),
                )
            });
    };
    let mut inserts = crate::design::components::ComponentInserts::decide(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
        &native.xref_references,
    );
    inserts.bind_external_paths(&native.xref_references, &target.model.occurrences);
    let projected = crate::design::assembly::project_assembly_joints_with_paths(
        &native.design_parameter_scopes,
        &native.design_component_occurrences,
        &inserts,
        &native.design_joint_motions,
        &crate::design::components::paths::LocalOccurrencePaths::new(
            &native.design_component_occurrences,
            &native.xref_references,
            &target.model.occurrences,
        ),
    );
    if target.model.assembly_joints != projected.joints {
        return Err(CodecError::NotImplemented(
            "editing F3D assembly joints is not supported".into(),
        ));
    }
    Ok(())
}

pub(super) fn normalized_face_sense_to_native(desired: Sense, carrier_flipped: bool) -> Sense {
    if carrier_flipped {
        match desired {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        }
    } else {
        desired
    }
}

/// Zero-fill `bytes` to `length`, or truncate it when it is longer.
pub(crate) fn zero_fill_to(bytes: &mut Vec<u8>, length: usize) {
    if let Some(additional) = length.checked_sub(bytes.len()) {
        for _ in 0..additional {
            bytes.push(0);
        }
    } else {
        bytes.truncate(length);
    }
}

pub(super) fn native_bool(value: bool) -> u8 {
    if value {
        0x0a
    } else {
        0x0b
    }
}

pub(super) fn unique_knot_count(knots: &[f64]) -> usize {
    knots
        .iter()
        .enumerate()
        .filter(|(index, value)| *index == 0 || knots[*index - 1] != **value)
        .count()
}
