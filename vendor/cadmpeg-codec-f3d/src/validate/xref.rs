// SPDX-License-Identifier: Apache-2.0
use super::{push_constant_finding, Ctx};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};

pub(super) fn validate_xref_placement_sources(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for reference in &ctx.native.xref_references {
        if !crate::xref::placement_source_matches(reference)? {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion XREF placement source does not match its repeated-target frame",
                Some(reference.id.clone()),
            );
        }
        if !crate::xref::owned_poses_match(reference) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion XREF owned pose does not match its component-path carrier",
                Some(reference.id.clone()),
            );
        }
        if !crate::xref::contained_poses_match(reference) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion XREF contained pose does not match its component-path carrier",
                Some(reference.id.clone()),
            );
        }
        if !crate::xref::operation_pose::reference_matches(reference) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion XREF definition pose does not match its operation table",
                Some(reference.id.clone()),
            );
        }
        if !crate::xref::current_pose_matches(reference) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion XREF current pose does not match its component-path carrier",
                Some(reference.id.clone()),
            );
        }
    }
    for occurrence in &ctx.native.design_component_occurrences {
        if !crate::xref::operation_pose::local_matches(occurrence) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design occurrence definition pose does not match its operation table",
                Some(occurrence.id.clone()),
            );
        }
        if !crate::xref::local_current_pose_matches(occurrence) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design component occurrence current pose does not match its component-path carrier",
                Some(occurrence.id.clone()),
            );
        }
    }
    for path in &ctx.native.design_component_paths {
        if !crate::xref::component_path_matches(path) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design component path does not match its carrier record",
                Some(path.id.clone()),
            );
        }
    }
    Ok(())
}
