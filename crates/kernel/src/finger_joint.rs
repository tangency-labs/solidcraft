//! Finger-joint tool generation through the `solidcore-ops` modelling recipe.

use crate::{Body, KernelError, Result};

/// Build each plate's cutters from two placed bodies. Placement matrices use the row-vector
/// convention of [`Body::transform`]. The tools returned are in each body's local coordinates.
///
/// `None` means the inputs have no positive-volume overlap or their plate axes are parallel. A
/// body that is not one closed manifold solid, or has no unique dominant plate axis, is an error.
/// The caller retains selection, expression evaluation, lineage and feature commits.
///
/// # Errors
/// The solidcore operation's refusal or error, and record conversion failures.
pub fn pair_tools(
    first: &Body,
    second: &Body,
    first_world: [[f64; 4]; 4],
    second_world: [[f64; 4]; 4],
    feature_world: [[f64; 4]; 4],
    count: usize,
    clearance: f64,
    edge_offset: f64,
    inverted: bool,
) -> Result<Option<(Option<Body>, Option<Body>)>> {
    if first.is_mesh() || first.shell_count() != 1 || first.lumps()?.len() != 1 {
        return Err(KernelError::Failed("finger joint: the first body is not one closed solid".into()));
    }
    if second.is_mesh() || second.shell_count() != 1 || second.lumps()?.len() != 1 {
        return Err(KernelError::Failed("finger joint: the second body is not one closed solid".into()));
    }
    let (first_ops, _) = solidcore_ops::Body::from_record(first.record(), true)
        .map_err(|error| KernelError::Failed(format!("finger joint: importing the first body into SolidCore: {error}")))?;
    let (second_ops, _) = solidcore_ops::Body::from_record(second.record(), true)
        .map_err(|error| KernelError::Failed(format!("finger joint: importing the second body into SolidCore: {error}")))?;
    if !first_ops.is_manifold_solid().map_err(|error| KernelError::Failed(format!("finger joint: {error}")))? {
        return Err(KernelError::Failed("finger joint: the first body is not a manifold solid".into()));
    }
    if !second_ops.is_manifold_solid().map_err(|error| KernelError::Failed(format!("finger joint: {error}")))? {
        return Err(KernelError::Failed("finger joint: the second body is not a manifold solid".into()));
    }
    let Some((first_tools, second_tools)) = solidcore_ops::finger_joint::pair_tools(
        &first_ops,
        &second_ops,
        first_world,
        second_world,
        feature_world,
        count,
        clearance,
        edge_offset,
        inverted,
    )
    .map_err(|error| KernelError::Failed(format!("finger joint: {error}")))?
    else {
        return Ok(None);
    };
    let from_ops = |tools: Option<solidcore_ops::Body>| -> Result<Option<Body>> {
        let Some(tools) = tools else {
            return Ok(None);
        };
        let (record, _) =
            tools.to_record().map_err(|error| KernelError::Failed(format!("finger joint: converting generated cutters to a record: {error}")))?;
        Body::from_record(record)
            .map(Some)
            .map_err(|error| KernelError::Failed(format!("finger joint: importing generated cutters into the document kernel: {error}")))
    };
    Ok(Some((from_ops(first_tools)?, from_ops(second_tools)?)))
}

/// Whether the placed pair still intersects after the caller applies the generated cuts.
///
/// # Errors
/// The solidcore operation's refusal or error, and record conversion failures.
pub fn pair_intersects(
    first: &Body,
    second: &Body,
    first_world: [[f64; 4]; 4],
    second_world: [[f64; 4]; 4],
    feature_world: [[f64; 4]; 4],
) -> Result<bool> {
    let (first_ops, _) = solidcore_ops::Body::from_record(first.record(), true)
        .map_err(|error| KernelError::Failed(format!("finger joint post-check: importing the first body into SolidCore: {error}")))?;
    let (second_ops, _) = solidcore_ops::Body::from_record(second.record(), true)
        .map_err(|error| KernelError::Failed(format!("finger joint post-check: importing the second body into SolidCore: {error}")))?;
    solidcore_ops::finger_joint::pair_intersects(&first_ops, &second_ops, first_world, second_world, feature_world)
        .map_err(|error| KernelError::Failed(format!("finger joint: {error}")))
}
