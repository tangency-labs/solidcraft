//! #18166: a G2 round cut by the body it was carried on. SolidCraft's
//! `concave_chord_and_g2_rounds_run_on_past_where_their_face_turns_into_a_round` (G2 r5) cuts the
//! rounded body (`data/issue_18166_g2_round_rounded.smb`) by the carried body
//! (`data/issue_18166_g2_round_carried.smb`), both written as SolidCraft's core hands them to
//! the boolean, under SolidCraft's fixes. The carried body's cylinder is in G2 contact with the
//! round's blend, so their intersection curve runs inside the contact band and cuts a sliver of
//! the blend off along its tangent line (3.8e-5 wide at mid-height). Classifying the cylinder at
//! that curve, the step off the sector's sliver face leaves it after 1.2e-4, while the cylinder
//! and the blend are still within 1e-13 of each other: read there, the cylinder coincided with
//! the blend, took the blend's outer region, and no radial edge use could take its next edge
//! (`make_faceuse: no radial edge use classifies the face's edge use`). The faces part by
//! 2.8e-5 at 0.1 from the curve, past the tolerance band 2e-5: the classifier now holds a
//! coincidence of faces on surfaces of different kinds until they part or a face ends, and
//! follows the sector's other face past the sliver.
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::Brep;
use solidcore::Kernel;

/// The fixes SolidCraft's core turns on at this case (`crates/smlib/src/core.rs` `fixes()`),
/// less those retired since (their behaviour is now the default).
const SOLIDCRAFT_FIXES: &[&str] = &[
    "fillet_chamfer_cone_on_arc",
    "proto_vertex_miss_is_false",
    "revolve_analytic_profile_faces",
    "offset_analytic_mate_edges",
    "uv_trim_corner_end_points_only",
    "bspline_trim_preserves_uv_spans",
    "analytic_copy_keeps_parameterization",
    "offset_extension_from_base",
    "fillet_cleanup_keeps_target_faces",
    "fillet_patch_keeps_fillet_edges",
    "fillet_knife_edge_rounds",
    "linear_sweep_arc_is_cylinder",
    "fillet_trace_reaches_rails",
    "curve_loop_normal_from_area",
    "offset_extension_as_itself",
    "fillet_variable_span_follows_fillet",
    "simple_coincidence_crossing_is_point",
    "ssi_proto_topology_statuses_pass_on",
    "fillet_exact_round_classes",
    "edgeuse_convexity_propagates_step_off_failure",
    "keep_exact_pcurves",
    "closed_smooth_pole_tangent_limit",
    "brep_ray_classifies_current_trims",
];

/// SolidCraft's fixes; without `simple_coincidence_crossing_is_point` when
/// `nearest_point` is false (WR-647 part B: the flag is being retired).
fn kernel(nearest_point: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in SOLIDCRAFT_FIXES {
        *f.flag_mut(name).unwrap() = true;
    }
    f.simple_coincidence_crossing_is_point = nearest_point;
    k.fixes = f;
    k
}

fn load(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let fresh = Brep::new();
    let mut store = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let key_of = |e| fresh.owner_identity(e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

#[test]
fn g2_round_minus_its_carried_body_cuts() {
    cut_takes_the_carried_body(true);
}

/// WR-647 part B: without `simple_coincidence_crossing_is_point` the cut is the same.
/// There the blend/cylinder pair returns the round's tangent edge twice: once as a curve
/// that is the edge from vertex to vertex, once as a curve that runs along it from 0.35
/// to its end. The second run is a copy of the edge (the topology intersector's
/// `duplicate_edge_run`); before it was dropped, its inner end split the edge the first
/// curve keeps whole and the merge failed with `RelateConflict`.
#[test]
fn g2_round_minus_its_carried_body_cuts_without_the_nearest_point_flag() {
    cut_takes_the_carried_body(false);
}

fn cut_takes_the_carried_body(nearest_point: bool) {
    let mut k = kernel(nearest_point);
    let mut rounded = load(
        &mut k,
        include_bytes!("data/issue_18166_g2_round_rounded.smb"),
    );
    let mut carried = load(
        &mut k,
        include_bytes!("data/issue_18166_g2_round_carried.smb"),
    );
    let (v_rounded, v_carried) = (volume(&k, &mut rounded), volume(&k, &mut carried));
    let mut cut = boolean_difference(&mut k, rounded, carried).unwrap();
    assert!(is_manifold_solid(&cut).unwrap(), "not manifold");
    let lines = check_brep(&mut k, &mut cut, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let v = volume(&k, &mut cut);
    // The carried body lies inside the rounded one (MEASURED: the cut takes exactly its
    // volume, 150.881825732 of 150.881825725 at the integrator's 1e-8).
    let want = v_rounded - v_carried;
    assert!(
        (v - want).abs() <= 1e-8 * v_rounded,
        "cut volume {v} vs {want} (rounded {v_rounded}, carried {v_carried})"
    );
}
