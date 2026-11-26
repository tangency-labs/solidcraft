//! Native qualification for a non-destructive split of refined Form material.
use super::{Definition, FittedSource, convert_source, weld_native};
use crate::{DocError, FeatureId, Result, kernel};
use std::collections::{BTreeMap, BTreeSet};

/// Split one complete seam into two qualified open native skins. The saved
/// definitions retain all coefficients and independent active leaf masks.
pub fn prepare_refined_unweld(definition: &Definition, edges: &[[usize; 2]], tolerance: f64) -> Result<[Definition; 2]> {
    if definition.profile.is_some() || definition.matching.is_some() || definition.pipe.is_some() || definition.symmetry.is_some() {
        return Err(invalid("refined Unweld requires an independent Form without symmetry"));
    }
    let hierarchy = definition.hierarchy.as_ref().ok_or_else(|| invalid("refined Unweld requires a retained hierarchy"))?;
    let split = hierarchy.split_logical_seam(edges).map_err(invalid)?;
    let (original, report, fitted) = convert_source(definition, tolerance)?;
    if !report.trimmed_roots || original.is_surface() || original.shell_count() != 1 || !kernel::validity(&original).ok() {
        return Err(invalid("refined Unweld requires one closed native solid with fitted leaf charts"));
    }
    let old_sheets = sheets(&fitted)?;
    if report.patch_roots.iter().copied().collect::<BTreeSet<_>>().len() != original.face_count() || old_sheets.len() != original.face_count() {
        return Err(invalid("refined Unweld requires one original native face per logical owner"));
    }
    let halves = split.halves.map(|hierarchy| {
        let mut part = definition.clone();
        part.hierarchy = Some(hierarchy);
        part
    });
    let qualify_half = |part: &Definition, index: usize| {
        let (body, report, source) = convert_source(part, tolerance)?;
        if !report.trimmed_roots || source.fit.patches != fitted.fit.patches || !body.is_surface() || !kernel::validity(&body).ok() {
            return Err(invalid("refined Unweld changed a retained surface or did not produce a valid open skin"));
        }
        let available = sheets(&source)?;
        if available.len() != body.face_count()
            || report.patch_roots.iter().copied().collect::<BTreeSet<_>>().len() != body.face_count()
            || available.iter().any(|(owner, sheet)| old_sheets.get(owner).is_none_or(|old| old.record() != sheet.record()))
        {
            return Err(invalid("refined Unweld changed a retained fitted face"));
        }
        let edges: Vec<_> = split.rim.iter().map(|pair| pair[index].edge).collect();
        weld_native::fitted_operand(part, body, report.patch_roots, &source, &edges)
    };
    let operands = [qualify_half(&halves[0], 0)?, qualify_half(&halves[1], 1)?];
    // These two temporary namespaces only distinguish proof operands. No
    // feature or name is authored during native validation.
    let checked = weld_native::qualify([FeatureId(1), FeatureId(2)], &split.rim, operands)
        .map_err(|error| invalid(format!("refined Unweld could not qualify its complete cut rim: {error}")))?;
    let recovered: BTreeSet<_> = checked.face_owners.iter().map(|owner| owner.face).collect();
    let expected: BTreeSet<_> = report.patch_roots.into_iter().collect();
    if recovered != expected || checked.body.face_count() != original.face_count() || checked.body.edge_count() != original.edge_count() {
        return Err(invalid("refined Unweld changed native face or seam coverage"));
    }
    Ok(halves)
}

fn sheets(source: &FittedSource) -> Result<BTreeMap<usize, &kernel::Body>> {
    let owners = &source.fit.patch_roots;
    let by_owner: BTreeMap<_, _> = owners.iter().copied().zip(&source.sheets).collect();
    if by_owner.len() != source.sheets.len() || owners.len() != source.sheets.len() {
        return Err(invalid("refined Unweld requires exact native sheet ownership"));
    }
    Ok(by_owner)
}

fn invalid(error: impl std::fmt::Display) -> DocError {
    DocError::Invalid(error.to_string())
}
