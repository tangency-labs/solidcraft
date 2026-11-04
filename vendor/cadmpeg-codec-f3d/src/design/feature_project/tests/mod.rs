// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod body_selection;
mod coil;
mod direct_face;
mod dispatcher;
mod draft_pull;
mod extrude;
mod extrude_fallbacks;
mod extrude_work_planes;
mod fixed_extrude;
mod hole;
mod hole_sketch_profiles;
mod loft;
mod mirror;
mod native_scope_definition;
mod parameter_cycles;
mod parameters;
mod pattern;
mod pipe;
mod primitives;
mod replace_face;
mod revolve;
mod revolve_axis_candidates;
mod revolve_targets;
mod scope_properties;
mod sheet_metal;
mod spatial_profiles;
mod split;
mod surface;
mod sweep;
mod symmetric_extrude;
mod thread_extent;
mod timeline;
mod treatments;
mod work_plane;
mod work_point_binding;

#[test]
fn audit_regression_near_half_turn_retains_negative_axis() {
    let theta = std::f64::consts::PI - 5.0e-9;
    let (sine, cosine) = theta.sin_cos();
    let matrix = [
        [cosine, sine, 0., 0.],
        [-sine, cosine, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let rotation = super::matrix_axis_angle(&matrix).unwrap();
    assert!(rotation.direction.z < 0.0);
    assert!((rotation.angle.get() - theta).abs() <= 4.0 * f64::EPSILON);
}
