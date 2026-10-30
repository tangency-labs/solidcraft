//! The `solidcore` backend: the same calls as the C API's, on the pure-Rust kernel. A [`Handle`] owns
//! a `solidcore` B-rep; one global kernel serialises every call, as SMLib's lock does. Calls not
//! ported here fail with [`NOT_BUILT`], like a build without a backend.
use std::ffi::CStr;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{PropertyAttributes, brep_volume};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cone, create_cylinder, create_sphere, create_torus};
use solidcore::topology::Brep;
use solidcore::topology::brep::mirror::mirror as brep_mirror;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::{p3, p7, p10};
use solidcore::topology::database_io::FileType;
use solidcore::topology::proto_topology::p1::api_transform::{TransformObject, transform_independent_object};
use solidcore::{EntityRef, SolidError};

use super::{ABI_VERSION, ERR_ARG, ERR_EXCEPTION, NOT_BUILT, OK};

// A handle is shared between threads behind the kernel lock; the B-rep must allow it.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Brep>();
};

/// `SM_ERR`, SMLib's generic failure.
const SM_ERR: i32 = 1001;
/// `SM_ERR_INVALID_INPUT`.
pub(super) const SM_ERR_INVALID_INPUT: i32 = 1007;

/// An owned B-rep. Calls take the kernel lock first, then the body's.
pub struct Handle {
    pub(super) brep: Mutex<Brep>,
    /// The key domain a copy names its geometry owners in (see [`keys`]).
    pub(super) domain: u64,
}

impl Handle {
    pub(super) fn new(b: Brep) -> Handle {
        Handle { brep: Mutex::new(b), domain: 0 }
    }
}

pub(super) struct State {
    pub(super) kernel: Kernel,
    /// Copies made so far: each names its objects in its own key domain.
    pub(super) copies: u64,
}

/// `SOLIDCORE_FIXES=all` (or a comma list of field names) turns on opt-in upstream fixes (`UpstreamFixes`), for measuring them.
fn new_kernel() -> Kernel {
    let mut k = Kernel::default();
    // Always on: a blend whose ball does not fit its faces (WR-462) is refused as too large, so a taper can take over, where upstream fails with status 1001; and a trace step that lands far short of its aim is not the next point (WR-503).
    k.fixes.fillet_refuse_ball_not_fitting = true;
    k.fixes.fillet_trace_rejects_short_step = true;
    // Not upstream, and wanted always: four equal rounds meeting at a point get the sphere through their arcs.
    k.fixes.fillet_corner_sphere_from_four_arcs = true;
    // Always on (WR-472, WR-494; built into develop's SMLib fork): without them the mass-property integral asks for ~1e-14 relative accuracy at 1e-8 and its steps shrink to nothing.
    k.fixes.precise_properties_body_length_scale = true;
    k.fixes.precise_properties_split_at_surface_knots = true;
    let spec = std::env::var("SOLIDCORE_FIXES").unwrap_or_default();
    for name in spec.split(',') {
        let all = name == "all";
        if all || name == "fillet_retry_radius_as_first" {
            k.fixes.fillet_retry_radius_as_first = true;
        }
        if all || name == "fillet_rail_rim_on_face" {
            k.fixes.fillet_rail_rim_on_face = true;
        }
        if all || name == "fillet_trace_rejects_short_step" {
            k.fixes.fillet_trace_rejects_short_step = true;
        }
        if all || name == "fillet_refuse_ball_not_fitting" {
            k.fixes.fillet_refuse_ball_not_fitting = true;
        }
        if all || name == "fillet_refuse_rails_leaving_faces" {
            k.fixes.fillet_refuse_rails_leaving_faces = true;
        }
        if all || name == "fillet_refuse_face_consuming_blends" {
            k.fixes.fillet_refuse_face_consuming_blends = true;
        }
        if all || name == "fillet_refuse_non_manifold_result" {
            k.fixes.fillet_refuse_non_manifold_result = true;
        }
        if all || name == "fillet_refuse_crossing_rails" {
            k.fixes.fillet_refuse_crossing_rails = true;
        }
        if all || name == "fillet_corner_ball_patch" {
            k.fixes.fillet_corner_ball_patch = true;
        }
        if all || name == "fillet_chamfer_cone_on_arc" {
            k.fixes.fillet_chamfer_cone_on_arc = true;
        }
        if all || name == "fillet_torus_center_on_axis" {
            k.fixes.fillet_torus_center_on_axis = true;
        }
        if all || name == "offset_excess_faces_after_merge" {
            k.fixes.offset_excess_faces_after_merge = true;
        }
        if all || name == "offset_clip_right_side_out" {
            k.fixes.offset_clip_right_side_out = true;
        }
        if all || name == "revolution_offset_axis_both_sides" {
            k.fixes.revolution_offset_axis_both_sides = true;
        }
        if all || name == "offset_extended_edges_mated" {
            k.fixes.offset_extended_edges_mated = true;
        }
        if all || name == "remove_slit_loops_where_solids_touch" {
            k.fixes.remove_slit_loops_where_solids_touch = true;
        }
        if all || name == "keep_operand_uv_trims" {
            k.fixes.keep_operand_uv_trims = true;
        }
        if all || name == "geometric_g2_continuity" {
            k.fixes.geometric_g2_continuity = true;
        }
        if all || name == "proto_vertex_miss_is_false" {
            k.fixes.proto_vertex_miss_is_false = true;
        }
        if all || name == "check_loop_vertex_tolerance" {
            k.fixes.check_loop_vertex_tolerance = true;
        }
        if all || name == "check_loop_projected_seam_ends" {
            k.fixes.check_loop_projected_seam_ends = true;
        }
        if all || name == "bspline_check_min_radius" {
            k.fixes.bspline_check_min_radius = true;
        }
        if all || name == "oscillating_skips_failed_guesses" {
            k.fixes.oscillating_skips_failed_guesses = true;
        }
        if all || name == "refine_cc_range_ends" {
            k.fixes.refine_cc_range_ends = true;
        }
        if all || name == "flat_corners_test_bits" {
            k.fixes.flat_corners_test_bits = true;
        }
        if all || name == "cylinder_rebuild_mean_radius" {
            k.fixes.cylinder_rebuild_mean_radius = true;
        }
        if all || name == "planar_path_uncapped_open_profiles" {
            k.fixes.planar_path_uncapped_open_profiles = true;
        }
        if all || name == "revolve_analytic_profile_faces" {
            k.fixes.revolve_analytic_profile_faces = true;
        }
        if all || name == "revolve_arc_centre_near_axis_is_sphere" {
            k.fixes.revolve_arc_centre_near_axis_is_sphere = true;
        }
        if all || name == "taper_pivot_nearest_corner" {
            k.fixes.taper_pivot_nearest_corner = true;
        }
        if all || name == "revolve_nested_shells_as_cavities" {
            k.fixes.revolve_nested_shells_as_cavities = true;
        }
        if all || name == "curve_sweep_caps_at_sweep_ends" {
            k.fixes.curve_sweep_caps_at_sweep_ends = true;
        }
        if all || name == "join_touching_loops" {
            k.fixes.join_touching_loops = true;
        }
        if all || name == "uv_trim_reversal_on_surface" {
            k.fixes.uv_trim_reversal_on_surface = true;
        }
        if all || name == "offset_resolve_crossing_loops" {
            k.fixes.offset_resolve_crossing_loops = true;
        }
        if all || name == "offset_collapsed_rounds_mated" {
            k.fixes.offset_collapsed_rounds_mated = true;
        }
        if all || name == "offset_missing_surface_fails" {
            k.fixes.offset_missing_surface_fails = true;
        }
        if all || name == "fillet_nx1_end_rib_global_merge" {
            k.fixes.fillet_nx1_end_rib_global_merge = true;
        }
        if all || name == "uv_trim_corner_end_points_only" {
            k.fixes.uv_trim_corner_end_points_only = true;
        }
        if all || name == "bspline_trim_preserves_uv_spans" {
            k.fixes.bspline_trim_preserves_uv_spans = true;
        }
        if all || name == "cone_parallel_line_midpoint" {
            k.fixes.cone_parallel_line_midpoint = true;
        }
        if all || name == "proto_edge_self_loop_on_vertex_degenerate" {
            k.fixes.proto_edge_self_loop_on_vertex_degenerate = true;
        }
        if all || name == "slice_coincident_faces_sheet_side" {
            k.fixes.slice_coincident_faces_sheet_side = true;
        }
        if all || name == "boolean_outer_shell_first" {
            k.fixes.boolean_outer_shell_first = true;
        }
        if all || name == "edge_reversal_keeps_failed_edge" {
            k.fixes.edge_reversal_keeps_failed_edge = true;
        }
        if all || name == "topology_failed_evaluations_propagate" {
            k.fixes.topology_failed_evaluations_propagate = true;
        }
        if all || name == "heal_stage3_failures_propagate" {
            k.fixes.heal_stage3_failures_propagate = true;
        }
        if all || name == "sweep_frame_on_planar_path_normal" {
            k.fixes.sweep_frame_on_planar_path_normal = true;
        }
        if all || name == "cover_refuses_folded_continuation" {
            k.fixes.cover_refuses_folded_continuation = true;
        }
        if all || name == "boolean_vertex_by_best_crossing" {
            k.fixes.boolean_vertex_by_best_crossing = true;
        }
        if all || name == "boolean_cleanup_removes_slits" {
            k.fixes.boolean_cleanup_removes_slits = true;
        }
        if all || name == "classify_edge_interval" {
            k.fixes.classify_edge_interval = true;
        }
        if all || name == "ellipse_trim_seam_lines_span_cone_height" {
            k.fixes.ellipse_trim_seam_lines_span_cone_height = true;
        }
        if all || name == "degenerate_proto_edge_in_one_vertex" {
            k.fixes.degenerate_proto_edge_in_one_vertex = true;
        }
        if all || name == "new_edge_vertex_tolerance_covers_gap" {
            k.fixes.new_edge_vertex_tolerance_covers_gap = true;
        }
        if all || name == "torus_signed_gen_curve_interval" {
            k.fixes.torus_signed_gen_curve_interval = true;
        }
        if all || name == "sheet_sweep_joins_profile_wires" {
            k.fixes.sheet_sweep_joins_profile_wires = true;
        }
        if all || name == "point_solve_keeps_triangle_guess" {
            k.fixes.point_solve_keeps_triangle_guess = true;
        }
        if all || name == "fillet_setback_from_start_vertex" {
            k.fixes.fillet_setback_from_start_vertex = true;
        }
        if all || name == "imprint_trims_edge_to_proto_vertex" {
            k.fixes.imprint_trims_edge_to_proto_vertex = true;
        }
        if all || name == "imprint_extends_edge_to_proto_vertex" {
            k.fixes.imprint_extends_edge_to_proto_vertex = true;
        }
        if all || name == "fillet_point_step_per_point" {
            k.fixes.fillet_point_step_per_point = true;
        }
        if all || name == "analytic_copy_keeps_parameterization" {
            k.fixes.analytic_copy_keeps_parameterization = true;
        }
        if all || name == "fillet_point_bigger_step_retry" {
            k.fixes.fillet_point_bigger_step_retry = true;
        }
        if all || name == "fillet_through_extended_rail_end" {
            k.fixes.fillet_through_extended_rail_end = true;
        }
        if all || name == "precise_properties_body_length_scale" {
            k.fixes.precise_properties_body_length_scale = true;
        }
        if all || name == "offset_extension_from_base" {
            k.fixes.offset_extension_from_base = true;
        }
        if all || name == "corner_blend_stays_inside_neighbours" {
            k.fixes.corner_blend_stays_inside_neighbours = true;
        }
        if all || name == "fillet_cleanup_keeps_target_faces" {
            k.fixes.fillet_cleanup_keeps_target_faces = true;
        }
    }
    k
}

pub(super) fn state() -> MutexGuard<'static, State> {
    static KERNEL: OnceLock<Mutex<State>> = OnceLock::new();
    KERNEL.get_or_init(|| Mutex::new(State { kernel: new_kernel(), copies: 0 })).lock().unwrap_or_else(PoisonError::into_inner)
}

/// This thread's own kernel, for calls that only read one (measures, bounds, checks), so the
/// app's worker threads don't queue on [`state`]. Through `&Kernel` nothing moves (the rng, the
/// caches), so a reader built the same way gives the shared kernel's results bit for bit.
pub(super) fn with_reader<R>(f: impl FnOnce(&Kernel) -> R) -> R {
    thread_local! {
        static READER: Kernel = new_kernel();
    }
    READER.with(f)
}

pub(super) fn body(h: &Handle) -> MutexGuard<'_, Brep> {
    h.brep.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The status SMLib's C API would return for `e`.
pub(super) fn status(e: &SolidError) -> i32 {
    match e {
        SolidError::Unported { .. } => NOT_BUILT,
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        SolidError::Upstream { code, .. } => *code,
        _ => SM_ERR,
    }
}

/// The identity a copy's objects have in key domain `domain` (0 for the source's).
pub(super) fn named_ref(entity: EntityRef, domain: u64) -> ObjectRef {
    let r = p3::topology_ref(0, entity);
    ObjectRef { type_code: r.type_code, key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)) }
}

/// The identities `b` names its objects in, as the caller keying its geometry owners: what
/// `validate_pointers` compares curve owners against. A body's owners follow the naming of the
/// call that made it (a primitive: the arena's, a fresh copy: its key domain, an edited body: the
/// kernel's serial), so the scheme is the one the first edge's owner is in; every other owner must
/// then agree with it.
pub(super) fn keys(h: &Handle, b: &Brep) -> Box<dyn Fn(EntityRef) -> ObjectRef> {
    let owner = b.edge_list().first().and_then(|&e| b.edge(e).ok()?.curve.as_ref()?.owner());
    let first = b.edge_list().first().map(|&e| EntityRef::Edge(e));
    let domain = h.domain;
    let schemes: [Box<dyn Fn(EntityRef) -> ObjectRef>; 3] =
        [Box::new(arena_ref), Box::new(move |e| named_ref(e, domain)), Box::new(move |e| p3::topology_ref(0, e))];
    let kernel = b.notify_serial().map(|serial| -> Box<dyn Fn(EntityRef) -> ObjectRef> { Box::new(move |e| p3::topology_ref(serial, e)) });
    kernel.into_iter().chain(schemes).find(|scheme| first.is_some_and(|e| Some(scheme(e)) == owner)).unwrap_or_else(|| Box::new(arena_ref))
}

/// `SmApiBrepCopy`: a copy that owns its geometry (a `Brep::clone` keeps the source's geometry
/// owners, so moving the clone would not move its curves and surfaces). With the key domain it
/// names its objects in.
pub(super) fn copy_in(s: &mut State, b: &Brep) -> Result<(Brep, u64), i32> {
    s.copies += 1;
    let domain = s.copies * 100_000_000;
    let target_key = |entity| named_ref(entity, domain);
    let mut notify = ArenaNotifyState::new(&Brep::new());
    let mut env = notify.env(None);
    env.key_of = &target_key;
    let copy = p3::api_brep_copy(&mut s.kernel, &mut env, Some(b), &|entity| named_ref(entity, 0)).map_err(|e| status(&e))?;
    Ok((copy, domain))
}

pub fn abi_version() -> i32 {
    ABI_VERSION
}
pub fn init() -> i32 {
    OK
}
pub fn set_log(_: Option<fn(i32, String)>) {}
pub fn set_dump_dir(_: Option<&CStr>) -> i32 {
    OK
}
pub fn current_call(_: &mut [u8]) -> usize {
    0
}

pub fn read(path: &CStr, ascii: bool) -> Result<Handle, i32> {
    let path = path.to_str().map_err(|_| ERR_ARG)?;
    let data = std::fs::read(path).map_err(|_| ERR_ARG)?;
    let file_type = if ascii { FileType::Ascii } else { FileType::Binary };
    let mut s = state();
    let mut b = Brep::new_with_infinite_region().map_err(|e| status(&e))?;
    let (mut records, mut attributes) = (solidcore::topology::topology::NotifyRecords::new(), AttributeStore::default());
    let key_of = |entity| p3::topology_ref(0, entity);
    let mut env = p10::TopologyEnv { all_attributes: &[], attributes: &mut attributes, key_of: &key_of, records: &mut records, next_key: 1 << 50 };
    p7::read_from_file(&mut s.kernel, &mut b, data, file_type, false, &mut env).map_err(|e| status(&e))?;
    Ok(Handle::new(b))
}
pub fn write(_: &Handle, _: &CStr, _: bool) -> i32 {
    NOT_BUILT
}

pub fn copy(b: &Handle) -> Result<Handle, i32> {
    let mut s = state();
    copy_in(&mut s, &body(b)).map(|(c, domain)| Handle { brep: Mutex::new(c), domain })
}

/// `matrix` as `smlib.h` gives it: row `i` < 3 the image of axis `i`, row 3 the translation. That is
/// a scale along the world axes, then a placement (`SmBrep::Transform`'s two arguments), when the
/// rows are orthogonal and positively scaled. A left-handed matrix is a reflection: its z axis comes
/// back negated (so right-handed) with `true`, and the body is mirrored about the world XY plane
/// first (`SmBrep::Mirror`), the scale and placement undoing the flip. A shear is refused
/// ([`super::ops::ERR_UNSUPPORTED`]).
fn decompose(matrix: &[[f64; 4]; 4]) -> Result<(Axis2Placement, Vec3, bool), i32> {
    let row = |i: usize| Vec3::new(matrix[i][0], matrix[i][1], matrix[i][2]);
    let (rx, ry, rz) = (row(0), row(1), row(2));
    let scale = [rx.length(), ry.length(), rz.length()];
    if matrix.iter().flatten().any(|v| !v.is_finite()) || scale.iter().any(|&s| s <= 1e-12) {
        return Err(ERR_ARG);
    }
    let (x, y, z) = (rx / scale[0], ry / scale[1], rz / scale[2]);
    const TOL: f64 = 1e-9;
    let c = x.cross(y);
    let reflected = (c + z).length() <= TOL;
    if x.dot(y).abs() > TOL || ((c - z).length() > TOL && !reflected) {
        return Err(super::ops::ERR_UNSUPPORTED);
    }
    let origin = Point3::new(matrix[3][0], matrix[3][1], matrix[3][2]);
    Ok((Axis2Placement::new(origin, x, y), Vec3::new(scale[0], scale[1], scale[2]), reflected))
}

pub fn transform(b: &Handle, matrix: &[[f64; 4]; 4]) -> Result<Handle, i32> {
    let (placement, scale, reflected) = decompose(matrix)?;
    let mut s = state();
    let (mut moved, domain) = copy_in(&mut s, &body(b))?;
    if reflected {
        let xy = Axis2Placement::new(Point3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        brep_mirror(&mut s.kernel, &mut moved, &xy).map_err(|e| status(&e))?;
    }
    move_and_scale(&mut s.kernel, &mut moved, &placement, scale)?;
    Ok(Handle { brep: Mutex::new(moved), domain })
}

fn move_and_scale(k: &mut Kernel, b: &mut Brep, placement: &Axis2Placement, scale: Vec3) -> Result<(), i32> {
    transform_independent_object(k, Some(TransformObject::Brep(b)), placement, Some(scale)).map_err(|e| status(&e))
}

pub fn make_box(origin: &[f64; 3], size: &[f64; 3]) -> Result<Handle, i32> {
    if origin.iter().chain(size).any(|v| !v.is_finite()) || size.iter().any(|&v| v <= 0.0) {
        return Err(ERR_ARG);
    }
    let mut s = state();
    create_box(&mut s.kernel, Point3::new(origin[0], origin[1], origin[2]), size[0], size[1], size[2]).map(Handle::new).map_err(|e| status(&e))
}

pub fn volume(b: &Handle, relative_accuracy: f64) -> Result<f64, i32> {
    if !relative_accuracy.is_finite() {
        return Err(ERR_ARG);
    }
    let s = state();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes { store: &store, key_of: &|_| ObjectKey(0) };
    brep_volume(&s.kernel, &mut body(b), relative_accuracy, &attrs).map_err(|e| status(&e))
}

pub fn counts(b: &Handle) -> Result<[i64; 3], i32> {
    let _guard = state();
    let b = body(b);
    let n = |len: usize| i64::try_from(len).map_err(|_| ERR_EXCEPTION);
    Ok([n(b.face_list().len())?, n(b.edge_list().len())?, n(b.vertex_list().len())?])
}

fn point(p: &[f64; 3]) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

/// Every argument finite, and the first `positive` of `sizes` above zero.
fn valid(p: &[f64; 3], sizes: &[f64], positive: usize) -> Result<(), i32> {
    let finite = p.iter().chain(sizes).all(|v| v.is_finite());
    if finite && sizes.iter().take(positive).all(|&v| v > 0.0) { Ok(()) } else { Err(ERR_ARG) }
}

fn primitive(make: impl FnOnce(&mut Kernel) -> solidcore::Result<Brep>) -> Result<Handle, i32> {
    let mut s = state();
    make(&mut s.kernel).map(Handle::new).map_err(|e| status(&e))
}

pub fn make_cylinder(origin: &[f64; 3], radius: f64, height: f64) -> Result<Handle, i32> {
    valid(origin, &[radius, height], 2)?;
    primitive(|k| create_cylinder(k, point(origin), radius, height))
}
pub fn make_sphere(center: &[f64; 3], radius: f64) -> Result<Handle, i32> {
    valid(center, &[radius], 1)?;
    primitive(|k| create_sphere(k, point(center), radius))
}
pub fn make_cone(origin: &[f64; 3], base_radius: f64, top_radius: f64, height: f64) -> Result<Handle, i32> {
    valid(origin, &[base_radius, top_radius, height], 1)?;
    if top_radius < 0.0 || height <= 0.0 || top_radius == base_radius {
        return Err(ERR_ARG);
    }
    primitive(|k| create_cone(k, point(origin), base_radius, top_radius, height))
}
pub fn make_torus(center: &[f64; 3], major_radius: f64, minor_radius: f64) -> Result<Handle, i32> {
    valid(center, &[major_radius, minor_radius], 2)?;
    if major_radius <= minor_radius {
        return Err(ERR_ARG);
    }
    primitive(|k| create_torus(k, point(center), major_radius, minor_radius))
}

/// The arena's own identity for an object: `kind << 32 | (index + 1)` (the topology key without the
/// kernel's serial), which primitives name their geometry owners by.
fn arena_ref(entity: EntityRef) -> ObjectRef {
    let r = p3::topology_ref(0, entity);
    ObjectRef { type_code: r.type_code, key: ObjectKey((r.key.0 & ((1 << 36) - 1)) + 1) }
}
