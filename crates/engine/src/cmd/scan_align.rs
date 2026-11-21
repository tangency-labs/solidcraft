//! 3-2-1 scan datum alignment (`scan.align`, #14353): align an imported point cloud or mesh
//! occurrence(s) by defining a primary plane, an in-plane secondary direction, and a projected
//! origin, mapping that right-handed frame rigidly (`det = +1`) onto the model origin or a
//! chosen target frame.
//!
//! All fitting, residual, projection, orthonormal frame, and rigid matrix math lives in
//! `solidcore::geom::scan_frame` and is invoked through `solidcore_ops::scan_align`.

use serde_json::Value;
use solidcore_ops::scan_align::{DEFAULT_TOL, DirectionInput, Frame3D, PlaneInput, compose_column_major, solve_alignment};
use solidcore_record::Vec3;
use solidcraft_doc::joints::AxisRef;
use solidcraft_doc::{IDENTITY, Mat, mat_inverse, mat_mul};
use solidcraft_geom::PlaneGeom;

use super::CommandSpec;
use crate::params::{bad, vec3};
use crate::schema::scan_align::{ScanAlignParams, ScanAlignResult, ScanFrameInfo, ScanOriginInfo, ScanPlaneFitInfo};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ALIGN_CONTRACT: TypedContract<ScanAlignParams> = TypedContract::new();

/// Maximum number of plane sample points accepted in one command call.
const MAX_PLANE_POINTS: usize = 100_000;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(
    "scan.align",
    "Align Scan",
    crate::checked!("scan.align", align),
)
.at("SOLID", "MODIFY")
.icon("align")
.contract(&ALIGN_CONTRACT)
.output(output_schema_for::<ScanAlignResult>)
.params(
    "cloud?: point cloud id or name | occurrence?/occurrences?/path?/paths?: mesh occurrence(s) to align (give cloud or occurrences, not both); \
     plane: {points: [[x,y,z], ...]} (>= 3 noncollinear samples) | XY|XZ|YZ | construction plane name | {origin, normal} | {face: [x,y,z]}; \
     direction: {from: [x,y,z], to: [x,y,z]} | {vector: [x,y,z]} | X|Y|Z|-X|-Y|-Z | construction axis name (projected into primary plane); \
     origin: [x,y,z] | \"origin\" (projected onto primary plane); \
     target?: \"world\" (default) | XY|XZ|YZ | construction plane name | {origin, x_axis, z_axis}; \
     flip_normal?: bool; flip_direction?: bool; tolerance?: mm (default 1e-6); \
     preview?: bool (compute fit residuals and transform without modifying the document); \
     capture?: bool (occurrences, default true); move_grounded?: bool (occurrences, default false) \
     → preview, target_kind, moved_names, plane (rms_residual_mm, max_abs_residual_mm, is_planar), origin (raw_origin, projected_origin, off_plane_distance_mm), source_frame, target_frame, world_transform, world_matrix, determinant, orthogonality_error",
)];

fn opt_bool(p: &Value, key: &str, default: bool, cmd: &str) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(bad(cmd, format!("`{key}` must be a boolean"))),
    }
}

fn to_arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn parse_point(v: &Value, label: &str, cmd: &str) -> Result<[f64; 3]> {
    if v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("origin")) {
        return Ok([0.0, 0.0, 0.0]);
    }
    vec3(v)
        .filter(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
        .map(to_arr)
        .ok_or_else(|| bad(cmd, format!("`{label}` must be [x, y, z] or \"origin\"")))
}

enum ResolvedPlane {
    Points(Vec<[f64; 3]>),
    Explicit { origin: [f64; 3], normal: [f64; 3] },
}

fn resolve_plane(s: &Session, v: &Value, cmd: &str) -> Result<ResolvedPlane> {
    if let Some(obj) = v.as_object() {
        if let Some(pts_val) = obj.get("points") {
            let arr = pts_val.as_array().ok_or_else(|| bad(cmd, "`plane.points` must be an array of at least 3 [x, y, z] points"))?;
            if arr.len() < 3 {
                return Err(bad(cmd, "`plane.points` requires at least 3 noncollinear points"));
            }
            if arr.len() > MAX_PLANE_POINTS {
                return Err(bad(cmd, format!("`plane.points` accepts at most {MAX_PLANE_POINTS} points")));
            }
            let mut pts = Vec::with_capacity(arr.len());
            for (idx, item) in arr.iter().enumerate() {
                let p = vec3(item)
                    .filter(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
                    .ok_or_else(|| bad(cmd, format!("`plane.points[{idx}]` must be finite [x, y, z]")))?;
                pts.push(to_arr(p));
            }
            return Ok(ResolvedPlane::Points(pts));
        }
        if let Some(face_val) = obj.get("face") {
            let fp = vec3(face_val)
                .filter(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
                .ok_or_else(|| bad(cmd, "`plane.face` must be [x, y, z]"))?;
            let n = super::face::face_normal_world(s, fp).ok_or_else(|| bad(cmd, "`plane.face` must lie on a planar face"))?;
            return Ok(ResolvedPlane::Explicit { origin: to_arr(fp), normal: to_arr(n) });
        }
        if obj.contains_key("origin") || obj.contains_key("normal") {
            let origin = obj
                .get("origin")
                .and_then(vec3)
                .filter(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
                .ok_or_else(|| bad(cmd, "`plane.origin` must be finite [x, y, z]"))?;
            let normal = obj
                .get("normal")
                .and_then(vec3)
                .filter(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
                .ok_or_else(|| bad(cmd, "`plane.normal` must be finite [x, y, z]"))?;
            return Ok(ResolvedPlane::Explicit { origin: to_arr(origin), normal: to_arr(normal) });
        }
    }
    // Plane name ("XY", "XZ", "YZ", or construction plane name)
    let pl_ref = super::features::plane_param(s, Some(v), cmd)?;
    let (vals, _) = s.doc.param_values();
    let world = s.world_state()?;
    let pl = s.placed_document().resolve_plane_in(&vals, &pl_ref, 0, 0, Some(&*world)).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(ResolvedPlane::Explicit { origin: to_arr(pl.origin), normal: to_arr(pl.normal()) })
}

fn resolve_direction(s: &Session, v: &Value, cmd: &str) -> Result<DirectionInput> {
    if let Some(name) = v.as_str() {
        let trimmed = name.trim();
        let axis = match trimmed.to_ascii_uppercase().as_str() {
            "X" | "+X" => Some([1.0, 0.0, 0.0]),
            "-X" => Some([-1.0, 0.0, 0.0]),
            "Y" | "+Y" => Some([0.0, 1.0, 0.0]),
            "-Y" => Some([0.0, -1.0, 0.0]),
            "Z" | "+Z" => Some([0.0, 0.0, 1.0]),
            "-Z" => Some([0.0, 0.0, -1.0]),
            _ => None,
        };
        if let Some(vec) = axis {
            return Ok(DirectionInput::Vector(vec));
        }
        let world = s.world_state()?;
        let (_, dir) = solidcraft_doc::joints::resolve_axis_ref(&s.placed_document(), &world, 0, &AxisRef::construction(trimmed))
            .map_err(|e| bad(cmd, format!("unknown direction axis `{trimmed}`: {e}")))?;
        return Ok(DirectionInput::Vector(to_arr(dir)));
    }
    if let Some(obj) = v.as_object() {
        if obj.contains_key("from") || obj.contains_key("to") {
            let from = obj.get("from").ok_or_else(|| bad(cmd, "`direction` needs both `from` and `to`"))?;
            let to = obj.get("to").ok_or_else(|| bad(cmd, "`direction` needs both `from` and `to`"))?;
            return Ok(DirectionInput::Points { from: parse_point(from, "direction.from", cmd)?, to: parse_point(to, "direction.to", cmd)? });
        }
        if let Some(vec_val) = obj.get("vector") {
            let d = vec3(vec_val)
                .filter(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
                .ok_or_else(|| bad(cmd, "`direction.vector` must be finite [x, y, z]"))?;
            return Ok(DirectionInput::Vector(to_arr(d)));
        }
    }
    Err(bad(cmd, "`direction` must be {from, to}, {vector}, or an axis name (X, Y, Z, -X, -Y, -Z, or construction axis)"))
}

fn resolve_target_frame(s: &Session, v: Option<&Value>, tol: f64, cmd: &str) -> Result<Frame3D> {
    let Some(v) = v.filter(|x| !x.is_null()) else {
        return Ok(Frame3D::world());
    };
    if let Some(name) = v.as_str() {
        let trimmed = name.trim();
        if trimmed.eq_ignore_ascii_case("world") || trimmed.eq_ignore_ascii_case("origin") {
            return Ok(Frame3D::world());
        }
        let pl_ref = super::features::plane_param(s, Some(v), cmd)?;
        let (vals, _) = s.doc.param_values();
        let world = s.world_state()?;
        let pl = s.placed_document().resolve_plane_in(&vals, &pl_ref, 0, 0, Some(&*world)).map_err(|e| bad(cmd, e.to_string()))?;
        let (frame, _) =
            solidcore_ops::scan_align::build_frame(to_arr(pl.origin), to_arr(pl.normal()), to_arr(pl.x), to_arr(pl.origin), false, false, tol)
                .map_err(|e| bad(cmd, format!("invalid target frame: {e}")))?;
        return Ok(frame);
    }
    if let Some(obj) = v.as_object() {
        let origin = obj.get("origin").map(|x| parse_point(x, "target.origin", cmd)).transpose()?.unwrap_or([0.0, 0.0, 0.0]);
        let x_axis = obj
            .get("x_axis")
            .and_then(vec3)
            .filter(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
            .ok_or_else(|| bad(cmd, "`target.x_axis` must be finite [x, y, z]"))?;
        let z_axis = obj
            .get("z_axis")
            .and_then(vec3)
            .filter(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
            .ok_or_else(|| bad(cmd, "`target.z_axis` must be finite [x, y, z]"))?;
        let (frame, _) = solidcore_ops::scan_align::build_frame(origin, to_arr(z_axis), to_arr(x_axis), origin, false, false, tol)
            .map_err(|e| bad(cmd, format!("invalid target frame: {e}")))?;
        return Ok(frame);
    }
    Err(bad(cmd, "`target` must be \"world\", a plane name (XY, XZ, YZ, or construction plane), or {origin, x_axis, z_axis}"))
}

fn to_frame_info(f: Frame3D) -> ScanFrameInfo {
    ScanFrameInfo { origin: f.origin, x_axis: f.x_axis, y_axis: f.y_axis, z_axis: f.z_axis }
}

enum AlignPlan {
    Cloud { idx: usize, composed: Mat },
    Occurrences { updates: Vec<(u64, Mat)>, capture: bool, world_move: Mat },
}

/// Evaluate `scan.align` without mutating `Session` (used by live UI preview and `preview: true`).
pub fn preview_align(s: &Session, p: &Value) -> Result<ScanAlignResult> {
    plan_align(s, p, Some(true)).map(|(_, res)| res)
}

fn plan_align(s: &Session, p: &Value, preview_override: Option<bool>) -> Result<(AlignPlan, ScanAlignResult)> {
    let cmd = "scan.align";
    let has_cloud = p.get("cloud").is_some_and(|v| !v.is_null());
    let has_occ = ["occurrence", "occurrences", "path", "paths"].into_iter().any(|k| p.get(k).is_some_and(|v| !v.is_null()));
    if has_cloud && has_occ {
        return Err(bad(cmd, "give `cloud` or occurrence target keys, not both"));
    }
    if !has_cloud && !has_occ {
        return Err(bad(cmd, "give `cloud` or `occurrence`/`occurrences`/`path`/`paths`"));
    }

    let flip_normal = opt_bool(p, "flip_normal", false, cmd)?;
    let flip_direction = opt_bool(p, "flip_direction", false, cmd)?;
    let preview = preview_override.unwrap_or(opt_bool(p, "preview", false, cmd)?);
    let capture = opt_bool(p, "capture", true, cmd)?;
    let move_grounded = opt_bool(p, "move_grounded", false, cmd)?;
    let tol = match p.get("tolerance") {
        None | Some(Value::Null) => DEFAULT_TOL,
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && *x > 0.0 && *x <= 1e3)
            .ok_or_else(|| bad(cmd, "`tolerance` must be a finite number in (0, 1000] mm"))?,
    };

    let plane_val = p.get("plane").ok_or_else(|| bad(cmd, "`plane` is required"))?;
    let dir_val = p.get("direction").ok_or_else(|| bad(cmd, "`direction` is required"))?;
    let origin_val = p.get("origin").ok_or_else(|| bad(cmd, "`origin` is required"))?;

    let resolved_plane = resolve_plane(s, plane_val, cmd)?;
    let direction = resolve_direction(s, dir_val, cmd)?;
    let raw_origin = parse_point(origin_val, "origin", cmd)?;
    let target_frame = resolve_target_frame(s, p.get("target"), tol, cmd)?;

    let plane_input = match &resolved_plane {
        ResolvedPlane::Points(pts) => PlaneInput::Points(pts),
        ResolvedPlane::Explicit { origin, normal } => PlaneInput::Explicit { origin: *origin, normal: *normal },
    };
    let sol =
        solve_alignment(plane_input, direction, raw_origin, flip_normal, flip_direction, target_frame, tol).map_err(|e| bad(cmd, e.to_string()))?;

    let world_move: Mat = sol.column_major;
    let world_matrix = super::point_cloud::matrix_rows(&world_move);

    if has_cloud {
        let idx = super::point_cloud::find(s, p, cmd)?;
        let cloud = s.doc.point_clouds.get(idx).ok_or_else(|| bad(cmd, "no point cloud"))?;
        let cloud_id = cloud.id;
        let cloud_name = cloud.name.clone();
        let composed = compose_column_major(&world_move, &cloud.transform);
        if !super::component::valid_position(&composed) {
            return Err(bad(cmd, "alignment transform exceeds supported numeric range"));
        }
        let composed_rows = super::point_cloud::matrix_rows(&composed);
        let result = ScanAlignResult {
            preview,
            target_kind: "cloud".into(),
            cloud: Some(cloud_id),
            occurrences: Vec::new(),
            paths: Vec::new(),
            moved_names: vec![cloud_name],
            shared: Vec::new(),
            pending: None,
            plane: ScanPlaneFitInfo {
                origin: sol.plane.origin,
                normal: sol.plane.normal,
                sample_count: sol.plane.sample_count,
                rms_residual_mm: sol.plane.rms_residual,
                max_abs_residual_mm: sol.plane.max_abs_residual,
                is_planar: sol.plane.is_planar,
            },
            origin: ScanOriginInfo {
                raw_origin: sol.origin.raw_origin,
                projected_origin: sol.origin.projected_origin,
                off_plane_distance_mm: sol.origin.off_plane_distance,
            },
            source_frame: to_frame_info(sol.source),
            target_frame: to_frame_info(sol.target),
            world_transform: world_move,
            world_matrix,
            composed_matrix: Some(composed_rows),
            determinant: sol.determinant,
            orthogonality_error: sol.orthogonality_error,
            param_warnings: Vec::new(),
        };
        return Ok((AlignPlan::Cloud { idx, composed }, result));
    }

    // Mesh / component occurrence targets: enforce grounding, rigid-group, parent-frame, and
    // shared-placement rules identically to `occurrence.align`.
    let paths = super::component::align::resolve_paths(s, p, cmd)?;
    let mut moved_names = Vec::with_capacity(paths.len());
    for path in &paths {
        let Some(&id) = path.last() else {
            return Err(bad(cmd, "occurrence path is empty"));
        };
        let o = s.doc.occurrences.iter().find(|o| o.id == id).ok_or_else(|| bad(cmd, "no occurrence"))?;
        if o.grounded && !move_grounded {
            return Err(bad(
                cmd,
                format!("{} is grounded: unground it first (occurrence.ground; a design's first part is grounded when it is made)", o.name),
            ));
        }
        if let Some(mut group) = s.held_by_rigid_group(id) {
            for _ in 0..64 {
                match s.doc.occurrence_of(group).and_then(|occ| s.held_by_rigid_group(occ.id)) {
                    Some(up) => group = up,
                    None => break,
                }
            }
            let name = s.doc.components.iter().find(|c| c.id == group).map(|c| c.name.clone()).unwrap_or_default();
            return Err(bad(cmd, format!("{} is held in {name} by a rigid group: move {name}, or Unlock Contents (component.unlock) first", o.name)));
        }
        moved_names.push(o.name.clone());
    }

    // Compute every new local placement before mutating anything.
    let mut updates = Vec::with_capacity(paths.len());
    for path in &paths {
        let placement = s.occurrence_placement(path).map_err(|e| bad(cmd, e.to_string()))?;
        let inverse = mat_inverse(&placement.parent_world).ok_or_else(|| bad(cmd, "parent frame is singular"))?;
        let local_delta = mat_mul(&mat_mul(&inverse, &world_move), &placement.parent_world);
        let next = mat_mul(&local_delta, &placement.local);
        if !super::component::valid_position(&next) {
            return Err(bad(cmd, "alignment transform exceeds supported numeric range"));
        }
        updates.push((placement.occurrence, next));
    }

    let moved: Vec<u64> = updates.iter().map(|(id, _)| *id).collect();
    let shared: Vec<Vec<u64>> = paths.iter().filter(|path| s.occurrence_shared(path)).cloned().collect();

    let result = ScanAlignResult {
        preview,
        target_kind: "occurrences".into(),
        cloud: None,
        occurrences: moved,
        paths,
        moved_names,
        shared,
        pending: Some(!capture),
        plane: ScanPlaneFitInfo {
            origin: sol.plane.origin,
            normal: sol.plane.normal,
            sample_count: sol.plane.sample_count,
            rms_residual_mm: sol.plane.rms_residual,
            max_abs_residual_mm: sol.plane.max_abs_residual,
            is_planar: sol.plane.is_planar,
        },
        origin: ScanOriginInfo {
            raw_origin: sol.origin.raw_origin,
            projected_origin: sol.origin.projected_origin,
            off_plane_distance_mm: sol.origin.off_plane_distance,
        },
        source_frame: to_frame_info(sol.source),
        target_frame: to_frame_info(sol.target),
        world_transform: world_move,
        world_matrix,
        composed_matrix: None,
        determinant: sol.determinant,
        orthogonality_error: sol.orthogonality_error,
        param_warnings: Vec::new(),
    };
    Ok((AlignPlan::Occurrences { updates, capture, world_move }, result))
}

fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let (plan, result) = plan_align(s, p, None)?;
    if !result.preview {
        match plan {
            AlignPlan::Cloud { idx, composed } => {
                if let Some(cloud) = s.doc.point_clouds.get(idx) {
                    let mut next_cloud = cloud.clone();
                    next_cloud.transform = composed;
                    next_cloud.check()?;
                    if let Some(slot) = s.doc_mut().point_clouds.get_mut(idx) {
                        *slot = next_cloud;
                    }
                }
            }
            AlignPlan::Occurrences { updates, capture, world_move } => {
                if capture {
                    if world_move != IDENTITY || updates.iter().any(|(id, _)| s.pending_moves.contains_key(id)) {
                        for (id, next) in updates {
                            s.pending_moves.remove(&id);
                            if let Some(x) = s.doc_mut().occurrences.iter_mut().find(|x| x.id == id) {
                                x.transform = next;
                            }
                        }
                    }
                } else {
                    for (id, next) in updates {
                        s.pending_moves.insert(id, next);
                    }
                    s.revision += 1;
                }
            }
        }
    }
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcore_record::Vec3;
    use solidcraft_doc::point_cloud::{ColorBy, Colors, Complex, PointCloud, Points};
    use solidcraft_geom::prelude::*;

    use crate::Session;

    fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
        s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
    }

    fn tilt(p: Vec3) -> Vec3 {
        // Rotate 25° about X, then 35° about Y, then translate by (80, -40, 25).
        let (ax, ay) = (25.0_f64.to_radians(), 35.0_f64.to_radians());
        let (sx, cx) = ax.sin_cos();
        let (sy, cy) = ay.sin_cos();
        let y1 = p.y * cx - p.z * sx;
        let z1 = p.y * sx + p.z * cx;
        let x2 = p.x * cy + z1 * sy;
        let z2 = -p.x * sy + z1 * cy;
        Vec3::new(x2 + 80.0, y1 - 40.0, z2 + 25.0)
    }

    fn add_synthetic_cloud(s: &mut Session, pts: &[Vec3]) -> u64 {
        let mut flat = Vec::with_capacity(pts.len() * 3);
        for p in pts {
            flat.push(p.x as f32);
            flat.push(p.y as f32);
            flat.push(p.z as f32);
        }
        let id = s.doc_mut().allocate_id().unwrap();
        s.doc_mut().point_clouds.push(PointCloud {
            id,
            name: "Scan".into(),
            file: "scan.ply".into(),
            origin: Vec3::ZERO,
            points: Points::new(flat.into()),
            colors: Colors::default(),
            source_points: pts.len() as u64,
            units: "mm".into(),
            transform: solidcraft_doc::IDENTITY,
            point_size: 2.0,
            color_by: ColorBy::File,
            color: None,
            height_range: None,
            visible: true,
            complex: Complex::default(),
            color_property: None,
            filters: Vec::new(),
            source: String::new(),
            max_points: None,
        });
        id
    }

    #[test]
    fn cloud_321_align_preview_apply_undo_compose_and_flip_preserve_rigidity() {
        let mut s = Session::default();
        // Smaller functional flange at z=0 (30x20), plus larger unrelated face at z=10 (100x100)
        // to prove choosing the smaller functional face never switches to the largest face.
        let local_pts = [
            Vec3::new(0.0, 0.0, 0.0),   // origin
            Vec3::new(30.0, 0.0, 0.0),  // +X direction
            Vec3::new(30.0, 20.0, 0.0), // plane sample 3
            Vec3::new(0.0, 20.0, 0.0),  // plane sample 4
            Vec3::new(0.0, 0.0, 10.0),  // larger back face
            Vec3::new(100.0, 0.0, 10.0),
            Vec3::new(100.0, 100.0, 10.0),
            Vec3::new(0.0, 100.0, 10.0),
        ];
        let world_pts: Vec<Vec3> = local_pts.iter().copied().map(tilt).collect();
        let cloud_id = add_synthetic_cloud(&mut s, &world_pts);
        let p = |i: usize| [world_pts[i].x, world_pts[i].y, world_pts[i].z];

        // 1. Preview does not mutate the document or push an undo step.
        let doc_before = s.doc.clone();
        let prev = run(
            &mut s,
            "scan.align",
            json!({
                "cloud": cloud_id,
                "plane": {"points": [p(0), p(1), p(2), p(3)]},
                "direction": {"from": p(0), "to": p(1)},
                "origin": p(4), // off-plane by 10 mm!
                "preview": true
            }),
        );
        assert_eq!(s.doc, doc_before, "preview must not mutate the document");
        assert_eq!(prev["preview"], true);
        assert!((prev["origin"]["off_plane_distance_mm"].as_f64().unwrap() - 10.0).abs() < 1e-4, "{prev}");
        assert!((prev["determinant"].as_f64().unwrap() - 1.0).abs() < 1e-9, "{prev}");

        // Check whether PCA normal pointed toward -Z of local frame; choose flip_normal so local +Z aligns to world +Z.
        let src_z = prev["source_frame"]["z_axis"].as_array().unwrap();
        let sz = Vec3::new(src_z[0].as_f64().unwrap(), src_z[1].as_f64().unwrap(), src_z[2].as_f64().unwrap());
        let expected_z = (world_pts[4] - world_pts[0]).normalized().unwrap();
        let flip_n = sz.dot(expected_z) < 0.0;

        // 2. Apply alignment in one undo step.
        let res = run(
            &mut s,
            "scan.align",
            json!({
                "cloud": cloud_id,
                "plane": {"points": [p(0), p(1), p(2), p(3)]},
                "direction": {"from": p(0), "to": p(1)},
                "origin": p(4),
                "flip_normal": flip_n,
            }),
        );
        assert_eq!(res["preview"], false);
        assert_eq!(res["target_kind"], "cloud");
        assert!((res["determinant"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert!(res["orthogonality_error"].as_f64().unwrap() < 1e-9);

        let aligned = &s.doc.point_clouds[0];
        let a0 = aligned.world_point(0).unwrap();
        let a1 = aligned.world_point(1).unwrap();
        let a4 = aligned.world_point(4).unwrap();
        assert!(a0.dist(Vec3::ZERO) < 1e-3, "origin landed at {a0:?}");
        assert!(a1.dist(Vec3::new(30.0, 0.0, 0.0)) < 1e-3, "+X landed at {a1:?}");
        assert!(a4.dist(Vec3::new(0.0, 0.0, 10.0)) < 1e-3, "+Z landed at {a4:?}");

        // Pairwise distances preserved strictly.
        for i in 0..local_pts.len() {
            for j in (i + 1)..local_pts.len() {
                let d0 = local_pts[i].dist(local_pts[j]);
                let d1 = aligned.world_point(i).unwrap().dist(aligned.world_point(j).unwrap());
                assert!((d0 - d1).abs() < 1e-3, "distance ({i},{j}) drifted: {d0} vs {d1}");
            }
        }

        // 3. Second alignment composes on top of existing non-identity cloud transform (with flip_direction).
        let res2 = run(
            &mut s,
            "scan.align",
            json!({
                "cloud": cloud_id,
                "plane": "XY",
                "direction": "X",
                "origin": [30.0, 0.0, 0.0],
                "flip_direction": true,
                "target": {"origin": [5.0, 10.0, 2.0], "x_axis": [0.0, 1.0, 0.0], "z_axis": [0.0, 0.0, 1.0]}
            }),
        );
        assert!((res2["determinant"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        let moved1 = s.doc.point_clouds[0].world_point(1).unwrap();
        let moved0 = s.doc.point_clouds[0].world_point(0).unwrap();
        assert!(moved1.dist(Vec3::new(5.0, 10.0, 2.0)) < 1e-3, "got {moved1:?}");
        // Old point 0 was at -30 along +X from point 1, which is +30 along flipped X -> +30 along target +Y.
        assert!(moved0.dist(Vec3::new(5.0, 40.0, 2.0)) < 1e-3, "got {moved0:?}");

        // 4. Undo restores previous placement in one step; redo reapplies it.
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.point_clouds[0].world_point(0).unwrap().dist(Vec3::ZERO) < 1e-3);
        run(&mut s, "edit.redo", json!({}));
        assert!(s.doc.point_clouds[0].world_point(1).unwrap().dist(Vec3::new(5.0, 10.0, 2.0)) < 1e-3);
    }

    #[test]
    fn noisy_plane_reports_rms_and_max_abs_and_degenerate_inputs_reject_without_mutation() {
        let mut s = Session::default();
        let id = add_synthetic_cloud(&mut s, &[Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0)]);
        let doc_before = s.doc.clone();

        // Noisy plane with asymmetric negative outlier (-0.4 mm) larger than positive (+0.1 mm).
        let r = run(
            &mut s,
            "scan.align",
            json!({
                "cloud": id,
                "plane": {"points": [
                    [-10.0, -10.0, 0.1], [10.0, -10.0, -0.05], [10.0, 10.0, 0.1],
                    [-10.0, 10.0, -0.05], [0.0, 0.0, -0.4], [0.0, 0.0, 0.1]
                ]},
                "direction": {"from": [0.0, 0.0, 0.0], "to": [10.0, 0.0, 0.0]},
                "origin": [0.0, 0.0, 0.0],
                "tolerance": 1e-3,
                "preview": true
            }),
        );
        assert_eq!(r["plane"]["is_planar"], false, "{r}");
        assert!(r["plane"]["max_abs_residual_mm"].as_f64().unwrap() > 0.3, "{r}");
        assert!(r["plane"]["rms_residual_mm"].as_f64().unwrap() > 0.1, "{r}");

        // Degenerate inputs must fail cleanly without mutating the document.
        for bad_call in [
            // Collinear plane points
            json!({"cloud": id, "plane": {"points": [[0,0,0],[5,0,0],[10,0,0]]}, "direction": "Y", "origin": [0,0,0]}),
            // Coincident direction points
            json!({"cloud": id, "plane": "XY", "direction": {"from": [1,2,3], "to": [1,2,3]}, "origin": [0,0,0]}),
            // Direction normal to plane
            json!({"cloud": id, "plane": "XY", "direction": {"vector": [0,0,5]}, "origin": [0,0,0]}),
            // Both cloud and occurrence
            json!({"cloud": id, "occurrence": 1, "plane": "XY", "direction": "X", "origin": [0,0,0]}),
        ] {
            assert!(s.execute("scan.align", &bad_call).is_err(), "{bad_call}");
        }
        assert_eq!(s.doc, doc_before);
    }

    #[test]
    fn nested_mesh_occurrence_respects_parent_transform_grounding_and_undo() {
        let mut s = Session::default();
        // Write a tiny valid ASCII STL box (0..10 in X, 0..10 in Y, 0..10 in Z) tilted by moving its occurrence.
        let dir = std::env::temp_dir().join(format!("solidcraft-scan-align-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("scan_box.stl");
        let tri = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            format!(
                "facet normal 0 0 0\nouter loop\nvertex {} {} {}\nvertex {} {} {}\nvertex {} {} {}\nendloop\nendfacet\n",
                a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]
            )
        };
        let p = [[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.], [0., 0., 10.], [10., 0., 10.], [10., 10., 10.], [0., 10., 10.]];
        let faces =
            [(0, 2, 1), (0, 3, 2), (4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4), (2, 3, 7), (2, 7, 6), (1, 2, 6), (1, 6, 5), (3, 0, 4), (3, 4, 7)];
        let mut body = String::from("solid box\n");
        for (a, b, c) in faces {
            body.push_str(&tri(p[a], p[b], p[c]));
        }
        body.push_str("endsolid box\n");
        std::fs::write(&stl, body).unwrap();

        let ins = run(&mut s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
        let occ = ins["occurrence"].as_u64().unwrap();

        // Grounded occurrence is refused by default without mutation.
        run(&mut s, "occurrence.ground", json!({"occurrence": occ, "grounded": true}));
        let doc_grounded = s.doc.clone();
        let err = s
            .execute(
                "scan.align",
                &json!({
                    "occurrence": occ,
                    "plane": {"points": [[0,0,10],[10,0,10],[10,10,10]]},
                    "direction": {"from": [0,0,10], "to": [0,10,10]},
                    "origin": [0,0,10]
                }),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("grounded"), "{err}");
        assert_eq!(s.doc, doc_grounded);

        // Unground and align top face (z=10) onto XY with +Y becoming +X and (0,0,10) at origin.
        run(&mut s, "occurrence.ground", json!({"occurrence": occ, "grounded": false}));
        // Translate the mesh occurrence first so we test non-identity existing placement.
        run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [20, 30, 5]}));
        let r = run(
            &mut s,
            "scan.align",
            json!({
                "occurrence": occ,
                "plane": {"points": [[20,30,15],[30,30,15],[30,40,15]]},
                "direction": {"from": [20,30,15], "to": [20,40,15]},
                "origin": [20,30,15],
                "flip_normal": false
            }),
        );
        assert_eq!(r["target_kind"], "occurrences");
        assert_eq!(r["occurrences"], json!([occ]));
        assert!((r["determinant"].as_f64().unwrap() - 1.0).abs() < 1e-9);

        // Check world placement applied cleanly: (20,30,15) moved to (0,0,0).
        let world_t = s.occurrence_placement(&[occ]).unwrap().world;
        let corner = solidcraft_doc::apply_point(&world_t, Vec3::new(0.0, 0.0, 10.0));
        assert!(corner.dist(Vec3::ZERO) < 1e-6, "aligned corner landed at {corner:?}");

        // Undo returns to translated position in one step.
        run(&mut s, "edit.undo", json!({}));
        let back_t = s.occurrence_placement(&[occ]).unwrap().world;
        let back_corner = solidcraft_doc::apply_point(&back_t, Vec3::new(0.0, 0.0, 10.0));
        assert!(back_corner.dist(Vec3::new(20.0, 30.0, 15.0)) < 1e-6, "got {back_corner:?}");

        // Nested occurrence under a rotated+translated parent subassembly respects parent_world.
        let sub = run(&mut s, "component.create", json!({"name": "Fixture"}));
        let sub_occ = sub["occurrence"].as_u64().unwrap();
        let sub_comp = sub["component"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component": sub_comp}));
        let child = run(&mut s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
        let child_occ = child["occurrence"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "occurrence.ground", json!({"occurrence": sub_occ, "grounded": false}));
        run(&mut s, "occurrence.move", json!({"occurrence": sub_occ, "translate": [100, -40, 25], "rotate": {"axis": [0, 0, 1], "angle": 90}}));
        let child_before = s.occurrence_placement(&[sub_occ, child_occ]).unwrap().world;
        let wp = |v: Vec3| {
            let q = solidcraft_doc::apply_point(&child_before, v);
            json!([q.x, q.y, q.z])
        };
        run(
            &mut s,
            "scan.align",
            json!({
                "path": [sub_occ, child_occ],
                "plane": {"points": [wp(Vec3::new(0.0, 0.0, 10.0)), wp(Vec3::new(10.0, 0.0, 10.0)), wp(Vec3::new(10.0, 10.0, 10.0))]},
                "direction": {"from": wp(Vec3::new(0.0, 0.0, 10.0)), "to": wp(Vec3::new(10.0, 0.0, 10.0))},
                "origin": wp(Vec3::new(0.0, 0.0, 10.0)),
            }),
        );
        let child_after = s.occurrence_placement(&[sub_occ, child_occ]).unwrap().world;
        let c0 = solidcraft_doc::apply_point(&child_after, Vec3::new(0.0, 0.0, 10.0));
        let cx = solidcraft_doc::apply_point(&child_after, Vec3::new(10.0, 0.0, 10.0));
        assert!(c0.dist(Vec3::ZERO) < 1e-5, "nested origin landed at {c0:?}");
        assert!(cx.dist(Vec3::new(10.0, 0.0, 0.0)) < 1e-5, "nested +X landed at {cx:?}");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_reopen_and_cloud_reload_preserve_aligned_placement() {
        let dir = std::env::temp_dir().join(format!("solidcraft-scan-align-persist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ply_path = dir.join("fixture_scan.ply");
        let write_ply = |extra: bool| {
            let mut pts = vec![[10.0_f64, 20.0, 5.0], [40.0, 20.0, 20.0], [40.0, 50.0, 20.0], [10.0, 50.0, 5.0]];
            if extra {
                pts.push([25.0, 35.0, 12.5]);
            }
            let mut s =
                format!("ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nend_header\n", pts.len());
            for p in pts {
                s.push_str(&format!("{} {} {}\n", p[0], p[1], p[2]));
            }
            std::fs::write(&ply_path, s).unwrap();
        };
        write_ply(false);

        let mut s = Session::default();
        let ins = run(&mut s, "pointcloud.insert", json!({"path": ply_path.to_string_lossy()}));
        let id = ins["cloud"].as_u64().unwrap();
        run(
            &mut s,
            "scan.align",
            json!({
                "cloud": id,
                "plane": {"points": [[10, 20, 5], [40, 20, 20], [40, 50, 20], [10, 50, 5]]},
                "direction": {"from": [10, 20, 5], "to": [40, 20, 20]},
                "origin": [10, 20, 5],
            }),
        );
        let expected_transform = s.doc.point_clouds[0].transform;
        let expected_p0 = s.doc.point_clouds[0].world_point(0).unwrap();
        let expected_p1 = s.doc.point_clouds[0].world_point(1).unwrap();
        assert!(expected_p0.dist(Vec3::ZERO) < 1e-4, "got {expected_p0:?}");
        assert!(expected_p1.y.abs() < 1e-4 && expected_p1.z.abs() < 1e-4 && expected_p1.x > 30.0);

        // Save and reopen container design: aligned placement is preserved verbatim.
        let container = solidcraft_io::write_design_container(&s.doc).unwrap();
        let reopened = solidcraft_io::read_design(&container).unwrap();
        assert_eq!(reopened.point_clouds[0].transform, expected_transform);
        assert!(reopened.point_clouds[0].world_point(0).unwrap().dist(expected_p0) < 1e-6);

        // Cloud reload reads updated points while preserving the aligned transform.
        write_ply(true);
        let rel = run(&mut s, "pointcloud.reload", json!({"cloud": id}));
        assert_eq!(rel["points"].as_u64(), Some(5));
        assert_eq!(s.doc.point_clouds[0].transform, expected_transform);
        assert!(s.doc.point_clouds[0].world_point(0).unwrap().dist(expected_p0) < 1e-5);
        // The newly added midpoint (25, 35, 12.5) lies on the fitted plane -> z == 0 after reload.
        let mid = s.doc.point_clouds[0].world_point(4).unwrap();
        assert!(mid.z.abs() < 1e-4, "reloaded midpoint should sit on Z=0, got {mid:?}");

        let _ = std::fs::remove_dir_all(dir);
    }
}
