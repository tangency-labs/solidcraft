// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod construction;
mod construction_candidates;
mod face_sources;
mod fixed_fillet_retention;
mod hole_locations;
mod recipe_structures;
mod recipes;
mod selection;
mod typed_profile;
mod work_point;

mod body_recipes;
mod body_target_groups;

mod construction_paths;

mod multiple_profiles;
