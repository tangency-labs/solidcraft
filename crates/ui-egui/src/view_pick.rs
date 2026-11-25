//! `view.pick` (control: `ui.pick`): screenshot pixels or world rays → the exact geometry
//! under each one (#6304).
//!
//! An agent that has just looked at a screenshot sends back the pixels it saw and gets the
//! body, face index, world point, surface normal and depth under each — the inputs
//! `solid.fillet`, `inspect.measure` and `construct.point` take. The command is read-only:
//! it never changes the selection, the camera or the document, and it answers from the same
//! per-body pick index the mouse uses, so a pick here matches what a click on the same
//! pixel would pick.
//!
//! Frames: `window` pixels are physical pixels of a window screenshot (divided by
//! `pixels_per_point`, offset by the viewport rect); `model` pixels are pixels of a
//! `ui.render` of the same size, so an agent can render, look, and pick without a window.

use solidcraft_engine::geom::prelude::*;
use std::collections::HashMap;

use egui::{Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};

use solidcore_record::Vec3;
use solidcraft_engine::SectionPlane;
use solidcraft_engine::doc::{apply_point, is_identity};
use solidcraft_engine::geom::{Mesh, RaycastStart};
use solidcraft_kernel::services::affine::{clipped_line_box_interval, line_halfspace_interval, line_parameter_from_start, line_point_at};

use crate::SolveApp;

/// At most this many pixels + rays per call: a batch of picks is one request, but it still
/// runs on the window thread.
const MAX_PICKS: usize = 1024;

/// The default model-render size, as the MCP `screenshot` tool renders by default.
const DEFAULT_MODEL_W: f64 = 960.0;
const DEFAULT_MODEL_H: f64 = 600.0;

/// How far past the section plane a face point may lie and still count as kept (the renderer
/// clips at `n·p <= n·o`; this is the same hairline `clip_segment` allows).
pub(crate) const SECTION_EPS: f64 = 1e-6;

/// Crossings one body may contribute before the section-cap walk gives up on it.
const MAX_CROSSINGS: usize = 64;

/// One input: the raw request (echoed back), its screen pixel when it came from one, and
/// the world ray it maps to.
struct Input<'a> {
    raw: &'a Value,
    pixel: Option<Pos2>,
    o: Vec3,
    d: Vec3,
}

/// The nearest triangle of one body along one ray, refined onto its face.
struct Cand {
    body: String,
    face: usize,
    point: Vec3,
    normal: Vec3,
    t: f64,
    ghosted: bool,
}

/// A body's outermost placement: (occurrence path, occurrence name).
struct Placed {
    path: Vec<u64>,
    name: String,
}

/// `view.pick` / `ui.pick`: `pixels` (screenshot pixels) and/or `rays` (world-space
/// `{origin, dir}`) → one entry per input, in order; `null` where nothing is hit.
pub fn pick_command(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    let frame = match p.get("frame").and_then(Value::as_str) {
        Some("window") => true,
        Some("model") => false,
        None => app.viewport.rect.is_some(),
        Some(other) => return Err(format!(r#"view.pick: frame must be "window" or "model", not {other:?}"#)),
    };
    let edges_on = p.get("edges").and_then(Value::as_bool).unwrap_or(true);
    let edge_px = f("edge_px").filter(|x| *x > 0.0).unwrap_or(6.0) as f32;
    let point_px = match p.get("point_px") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|x| x.is_finite() && *x >= 0.0 && *x <= 1024.0)
                .ok_or("view.pick: `point_px` must be a number from 0 to 1024".to_string())? as f32,
        ),
    };
    let clouds_on = solidcraft_engine::point_cloud_pick::clouds_requested(p)?;
    let cloud_radius = match p.get("cloud_radius") {
        None | Some(Value::Null) => solidcraft_engine::point_cloud_pick::DEFAULT_CLOUD_RADIUS_MM,
        Some(v) => {
            v.as_f64().filter(|x| x.is_finite() && (0.0..=solidcraft_engine::point_cloud_pick::MAX_CLOUD_RADIUS_MM).contains(x)).ok_or_else(|| {
                format!("view.pick: `cloud_radius` must be a number from 0 to {}", solidcraft_engine::point_cloud_pick::MAX_CLOUD_RADIUS_MM)
            })?
        }
    };
    let all = p.get("all").and_then(Value::as_bool).unwrap_or(false);
    let include_ghosted = p.get("include_ghosted").and_then(Value::as_bool).unwrap_or(false);

    // The projection the pixels live in: the window's, or a model render's own w×h.
    let (proj, ppp) = if frame {
        let rect = app.viewport.rect.ok_or(r#"view.pick: frame "window" needs a drawn window; headless callers use frame "model""#)?;
        (crate::viewport::projection(app, rect), app.viewport.pixels_per_point())
    } else {
        let w = f("width").unwrap_or(DEFAULT_MODEL_W).clamp(16.0, 8192.0);
        let h = f("height").unwrap_or(DEFAULT_MODEL_H).clamp(16.0, 8192.0);
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
        let cam = app.cam;
        (crate::viewport::Proj { cam, vp: cam.view_proj(w / h, crate::viewport::scene_radius(app)), rect, bounds: None }, 1.0)
    };

    let mut inputs: Vec<Input> = Vec::new();
    if let Some(px) = p.get("pixels") {
        for q in px.as_array().ok_or("view.pick: `pixels` must be an array of [x, y]".to_string())? {
            let mut xy = [0.0f64; 2];
            for (i, v) in xy.iter_mut().enumerate() {
                *v = q.get(i).and_then(Value::as_f64).filter(|x| x.is_finite()).ok_or("view.pick: a pixel must be [x, y] numbers")?;
            }
            let (x, y) = (xy[0], xy[1]);
            // Window screenshots are whole-window, in physical pixels; `Proj` speaks
            // absolute window points, so only the HiDPI scale comes off.
            let pos = if frame { pos2((x / ppp as f64) as f32, (y / ppp as f64) as f32) } else { pos2(x as f32, y as f32) };
            let (o, d) = proj.ray(pos);
            inputs.push(Input { raw: q, pixel: Some(pos), o, d });
        }
    }
    if let Some(rs) = p.get("rays") {
        for r in rs.as_array().ok_or("view.pick: `rays` must be an array of {origin, dir}".to_string())? {
            let triple = |key: &str| -> Result<Vec3, String> {
                let v = r.get(key).ok_or_else(|| format!("view.pick: a ray needs `{key}` [x, y, z]"))?;
                let mut xyz = [0.0f64; 3];
                for (i, x) in xyz.iter_mut().enumerate() {
                    *x = v.get(i).and_then(Value::as_f64).filter(|x| x.is_finite()).ok_or(format!("view.pick: `{key}` must be [x, y, z] numbers"))?;
                }
                Ok(Vec3::new(xyz[0], xyz[1], xyz[2]))
            };
            let d = triple("dir")?.normalized().ok_or("view.pick: a ray needs a non-zero `dir`")?;
            inputs.push(Input { raw: r, pixel: None, o: triple("origin")?, d });
        }
    }
    if inputs.is_empty() {
        return Err("view.pick: give `pixels` and/or `rays`".to_string());
    }
    if inputs.len() > MAX_PICKS {
        return Err(format!("view.pick: at most {MAX_PICKS} pixels+rays per call"));
    }

    // What the viewport shows now, and what a pick may not answer for.
    let st = crate::open_meshing::shown_world(app);
    let bounds = crate::viewport::body_bounds(app, &st);
    let active_bodies = crate::viewport::active_body_names(app);
    let replaced: &[String] = app.preview.effective_replaced();
    let section = crate::viewport::section_planes(app);
    let wireframe = app.ui.visual_style == 2;
    let slack = crate::viewport::scene_pick_slack(app);
    let occ = occurrence_map(app);
    let comps = component_names(app);

    let mut hits: Vec<Value> = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let mut cands = face_candidates(app, &st, &bounds, input, &active_bodies, replaced, include_ghosted);
        cands.sort_by(|a, b| a.t.total_cmp(&b.t));
        // A section hides the material on the side any plane's normal points at: faces there do
        // not stop a pick, and the cut cap answers where the cut crosses material.
        let kept: Vec<&Cand> = cands.iter().filter(|c| section_keeps(&section, c.point)).collect();
        let cap = section_cap(app, &st, &bounds, input, &section, kept.first().map(|c| c.t), include_ghosted, replaced, &active_bodies);
        let face_t = kept.first().map(|c| c.t).or(cap.as_ref().map(|c| c.t));
        let (edge, vertex) = match (edges_on, input.pixel) {
            (true, Some(pos)) => near_edges(
                app,
                &st,
                &bounds,
                &proj,
                input,
                pos,
                face_t,
                edge_px,
                &active_bodies,
                replaced,
                include_ghosted,
                wireframe,
                slack,
                &section,
            ),
            _ => (None, None),
        };
        // Whatever is nearest in front hides the cloud: the first kept face or the section's cap.
        let cover_t = match (kept.first().map(|c| c.t), cap.as_ref().map(|c| c.t)) {
            (Some(f), Some(c)) => Some(f.min(c)),
            (f, c) => f.or(c),
        };
        let cloud_max = if all || wireframe { None } else { cover_t.map(|t| t + slack) };
        let cloud_hits = if clouds_on { cloud_candidates(app, &proj, input, cloud_max, point_px, cloud_radius, &section, all) } else { Vec::new() };
        hits.push(match (all, cap, kept) {
            (true, cap, kept) => {
                let mut rows: Vec<(f64, Value)> = Vec::new();
                if let Some(cap) = &cap {
                    rows.push((cap.t, cap_json(input, cap)));
                }
                rows.extend(kept.iter().map(|c| (c.t, hit_json(input, c, &occ, &comps, None, None))));
                rows.extend(cloud_hits.into_iter().map(|(h, px)| (h.depth, cloud_json(input, &h, px))));
                rows.sort_by(|a, b| a.0.total_cmp(&b.0));
                Value::Array(rows.into_iter().map(|(_, v)| v).collect())
            }
            (false, cap, kept) => {
                let surface = match (cap, kept.first()) {
                    (Some(cap), Some(c)) if c.t < cap.t => Some((c.t, hit_json(input, c, &occ, &comps, edge.as_ref(), vertex.as_ref()))),
                    (Some(cap), _) => Some((cap.t, cap_json(input, &cap))),
                    (None, Some(c)) => Some((c.t, hit_json(input, c, &occ, &comps, edge.as_ref(), vertex.as_ref()))),
                    (None, None) => None,
                };
                match (cloud_hits.into_iter().next(), surface) {
                    (Some((ch, px)), Some((st_t, _))) if wireframe || ch.depth <= st_t + slack => cloud_json(input, &ch, px),
                    (Some((ch, px)), None) => cloud_json(input, &ch, px),
                    (_, Some((_, v))) => v,
                    (None, None) => match (&edge, &vertex) {
                        (None, None) => Value::Null,
                        _ => json!({
                            "pixel": input.raw, "body": null, "face": null,
                            "edge": edge.as_ref().map(edge_json), "vertex": vertex.as_ref().map(vertex_json),
                        }),
                    },
                }
            }
        });
    }
    Ok(json!({
        "frame": if frame { "window" } else { "model" },
        "camera": {
            "projection": if proj.cam.fov > 0.0 { "perspective" } else { "orthographic" },
            "eye": v3(proj.cam.eye()),
            "target": v3(proj.cam.target),
        },
        "hits": hits,
    }))
}

/// The nearest face of every pickable body along the ray, nearest first. Bodies the
/// viewport hides, ghosts (unless asked for) or stands in for with a preview are skipped,
/// exactly as the mouse picker skips them.
fn face_candidates(
    app: &SolveApp,
    st: &solidcraft_engine::doc::ModelState,
    bounds: &[(String, solidcraft_engine::geom::Aabb3)],
    input: &Input,
    active_bodies: &Option<crate::viewport::ActiveBodyNames>,
    replaced: &[String],
    include_ghosted: bool,
) -> Vec<Cand> {
    let mut out = Vec::new();
    for (i, b) in st.bodies.iter().enumerate() {
        if !app.body_visible(&b.name) || replaced.contains(&b.name) {
            continue;
        }
        let ghosted = crate::viewport::body_faded(app, b, active_bodies);
        // Ghost Frame Members lets clicks through a member's faces, as the mouse picker does.
        if (ghosted && !include_ghosted) || crate::viewport::frame_ghosted(app, b) {
            continue;
        }
        if let Some((_, bb)) = bounds.get(i) {
            let pad = bb.diagonal() * 1e-6 + 1e-9;
            if crate::viewport::ray_box(input.o, input.d, bb, pad).is_none() {
                continue;
            }
        }
        // A placed part is picked in its own frame, as the mouse picker does: the ray moves
        // there (a rigid move keeps distances along it), so no moved copy of the mesh is made.
        let (m, place) = b.mesh_in_place();
        let Some((lo, ld)) = crate::viewport::local_ray(&place, input.o, input.d) else { continue };
        let Some((t, ti)) = crate::pick_index::raycast(app.viewport.pick_index.of(&m).as_deref(), &m, lo, ld) else { continue };
        let Some((point, normal)) = refine(&m, ti, t, lo, ld, &place) else { continue };
        out.push(Cand { body: b.name.clone(), face: m.tri_face.get(ti).copied().unwrap_or(0) as usize, point, normal, t, ghosted });
    }
    out
}

/// What the orbit ray can meet. Independent of camera movement and display refinement.
pub(crate) fn orbit_scene_key(app: &SolveApp) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut h = std::collections::hash_map::DefaultHasher::new();
    let s = &app.session;
    s.revision.hash(&mut h);
    std::sync::Arc::as_ptr(&crate::open_meshing::shown_world(app)).hash(&mut h);
    s.visibility.hidden_bodies.hash(&mut h);
    s.visibility.hidden_occurrences.hash(&mut h);
    s.visibility.hidden_folders.hash(&mut h);
    s.active_sketch.hash(&mut h);
    app.dialog.as_ref().map(|d| &d.edit_source_bodies).hash(&mut h);
    app.preview.effective_replaced().hash(&mut h);
    crate::viewport::frame_ghosting(app).hash(&mut h);
    for plane in crate::viewport::section_planes(app) {
        [plane.origin.x, plane.origin.y, plane.origin.z, plane.normal.x, plane.normal.y, plane.normal.z].map(f64::to_bits).hash(&mut h);
    }
    h.finish()
}

/// The orbit's window ray and its first visible material interval (distances from the origin).
/// The orthographic ray starts in front of the conservative bounds, including any curved
/// surface that reaches past the display mesh.
pub(crate) fn orbit_ray(app: &SolveApp, proj: &crate::viewport::Proj, at: egui::Pos2) -> (Vec3, Vec3, Option<(f64, f64)>) {
    let state = crate::open_meshing::shown_world(app);
    let (boxes, tree) = crate::viewport::orbit_body_bounds(app, &state);
    let scene = tree.union;
    let bounds = if !scene.is_empty() && scene.min.is_finite() && scene.max.is_finite() {
        Some(proj.bounds.map_or(scene, |b| b.union(&scene)))
    } else {
        proj.bounds
    };
    let (origin, dir) = crate::viewport::Proj { bounds, ..*proj }.ray(at);
    (origin, dir, visible_body_interval(app, &state, &boxes, &tree, origin, dir))
}

/// The hierarchy only rules out candidates. Each solid answers from its trimmed B-rep,
/// and a mesh body from its own triangles.
fn visible_body_interval(
    app: &SolveApp,
    state: &solidcraft_engine::doc::ModelState,
    boxes: &[(String, solidcraft_engine::geom::Aabb3)],
    tree: &crate::viewport::BodyTree,
    origin: Vec3,
    dir: Vec3,
) -> Option<(f64, f64)> {
    use crate::viewport::{frame_ghosted, section_planes};

    let ((near, far), _) = section_ray_interval(&section_planes(app), origin, dir)?;
    let clip = (near.max(0.0), far);
    if clip.0 >= clip.1 {
        return None;
    }
    let entry = |bounds: &solidcraft_engine::geom::Aabb3| {
        if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
            return Some(clip.0);
        }
        let (near, _) = clipped_line_box_interval([bounds.min.into(), bounds.max.into()], origin.into(), dir.into(), clip, (1e-6, 1e-9), 1e-15)?;
        near.is_finite().then_some(near)
    };
    let replaced = app.preview.effective_replaced();
    let mut candidates: Vec<_> = tree
        .candidates(boxes, |bounds| entry(bounds).is_some())
        .into_iter()
        .filter_map(|i| {
            let body = state.bodies.get(i)?;
            if !app.body_visible(&body.name) || replaced.contains(&body.name) || frame_ghosted(app, body) {
                return None;
            }
            Some((entry(&boxes.get(i)?.1)?, body))
        })
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut closest = None;
    for (lower_bound, body) in candidates {
        if closest.is_some_and(|(near, _)| lower_bound > near) {
            break;
        }
        let interval = if body.body.is_mesh() {
            let mesh_hit = mesh_hit_in_clip(app, body, origin, dir, clip);
            mesh_hit.map(|t| (t, t))
        } else {
            let Ok(interval) = solidcraft_engine::kernel::guard("orbit depth", || body.body.first_ray_interval(origin, dir, clip, body.is_surface()))
            else {
                continue;
            };
            interval
        };
        if let Some(interval) = interval
            && closest.is_none_or(|(near, _)| interval.0 < near)
        {
            closest = Some(interval);
        }
    }
    closest
}

/// The nearest displayed triangle in the kept part of the ray, using the mesh's own index.
fn mesh_hit_in_clip(app: &SolveApp, body: &solidcraft_engine::doc::ModelBody, origin: Vec3, dir: Vec3, clip: (f64, f64)) -> Option<f64> {
    let (mesh, place) = body.mesh_in_place();
    let clipped_origin = Vec3::from(line_point_at(origin.into(), dir.into(), clip.0));
    let (local_origin, local_dir) = crate::viewport::local_ray(&place, clipped_origin, dir)?;
    let (t, _) = crate::pick_index::raycast_with_start(
        app.viewport.pick_index.of(&mesh).as_deref(),
        &mesh,
        local_origin,
        local_dir,
        RaycastStart::IncludeOrigin,
    )?;
    line_parameter_from_start(clip, t)
}

/// The hit point and outward surface normal in world space. A flat face (constant mesh
/// normals) is solved exactly against its plane, so a box picks to the last digit; a curved
/// face keeps the mesh hit (within the display tolerance) and takes its normal interpolated
/// from the mesh's per-vertex surface normals.
fn refine(m: &Mesh, ti: usize, t: f64, lo: Vec3, ld: Vec3, place: &solidcraft_engine::doc::Mat) -> Option<(Vec3, Vec3)> {
    let [a, b, c] = *m.triangles.get(ti)?;
    let (p0, p1, p2) = (m.positions.get(a as usize)?, m.positions.get(b as usize)?, m.positions.get(c as usize)?);
    let lp = lo + ld * t;
    let mesh_normal = || (*p1 - *p0).cross(*p2 - *p0).normalized().or_else(|| (ld * -1.0).normalized());
    let (point_local, n_local) = match (m.normals.get(a as usize), m.normals.get(b as usize), m.normals.get(c as usize)) {
        (Some(na), Some(nb), Some(nc)) if nb.dist(*na) < 1e-6 && nc.dist(*na) < 1e-6 && nc.dist(*nb) < 1e-6 => {
            // Flat: the exact ray-plane intersection, on the face's own plane.
            let n = na.normalized()?;
            let den = n.dot(ld);
            let tp = if den.abs() < 1e-12 { t } else { (n.dot(*p0 - lo) / den).max(0.0) };
            (lo + ld * tp, n)
        }
        (Some(na), Some(nb), Some(nc)) => {
            let (w0, w1, w2) = barycentric(lp, *p0, *p1, *p2);
            let n = (Vec3::new(na.x * w0 + nb.x * w1 + nc.x * w2, na.y * w0 + nb.y * w1 + nc.y * w2, na.z * w0 + nb.z * w1 + nc.z * w2))
                .normalized()
                .or_else(mesh_normal)?;
            (lp, n)
        }
        _ => (lp, mesh_normal()?),
    };
    let point = apply_point(place, point_local);
    let normal = apply_dir(place, point_local, n_local);
    Some((point, normal))
}

/// A placement is rigid, so a direction rides along it as a point difference.
fn apply_dir(place: &solidcraft_engine::doc::Mat, at: Vec3, n: Vec3) -> Vec3 {
    if is_identity(place) {
        return n;
    }
    (apply_point(place, at + n) - apply_point(place, at)).normalized().unwrap_or(n)
}

/// Affine weights of `p` in triangle `a b c`.
fn barycentric(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> (f64, f64, f64) {
    let (e0, e1, e2) = (b - a, c - a, p - a);
    let (d00, d01, d11) = (e0.dot(e0), e0.dot(e1), e1.dot(e1));
    let (d20, d21) = (e2.dot(e0), e2.dot(e1));
    let den = d00 * d11 - d01 * d01;
    if !(den.is_finite() && den.abs() > 1e-24) {
        return (1.0, 0.0, 0.0);
    }
    let v = (d11 * d20 - d01 * d21) / den;
    let w = (d00 * d21 - d01 * d20) / den;
    (1.0 - v - w, v, w)
}

/// Whether the section keeps `p`: every plane keeps it (the section hides what lies on the
/// positive side of any of them), within [`SECTION_EPS`].
pub(crate) fn section_keeps(planes: &[SectionPlane], p: Vec3) -> bool {
    planes.iter().all(|c| c.keeps(p, SECTION_EPS))
}

/// Where the ray `o + d·t` comes into what the section keeps (inside every plane) ahead of `o`:
/// `t` and the index of the plane it comes in through, where its cut face is.
pub(crate) fn section_entry(planes: &[SectionPlane], o: Vec3, d: Vec3) -> Option<(f64, usize)> {
    let ((lo, hi), entry_plane) = section_ray_interval(planes, o, d)?;
    (lo > 0.0 && lo < hi).then_some((lo, entry_plane?))
}

/// Where the section keeps the ray, and which plane it enters through if there is one.
fn section_ray_interval(planes: &[SectionPlane], o: Vec3, d: Vec3) -> Option<((f64, f64), Option<usize>)> {
    let (mut lo, mut hi, mut through) = (f64::NEG_INFINITY, f64::INFINITY, None);
    for (i, c) in planes.iter().enumerate() {
        let (near, far) = line_halfspace_interval(o.into(), d.into(), c.origin.into(), c.normal.into(), 1e-12)?;
        hi = hi.min(far);
        if near > lo {
            (lo, through) = (near, Some(i));
        }
    }
    (lo <= hi).then_some(((lo, hi), through))
}

/// A cut cap under a pick: where, in which body, and the plane it is a cut face of.
pub(crate) struct CapHit {
    pub t: f64,
    pub body: String,
    pub plane: SectionPlane,
}

/// Whether the section's cut crosses material at the ray (where a cut cap answers): where the
/// ray comes into what the section keeps lies inside a body, told by an odd number of mesh crossings
/// before it. The walk re-raycasts each body past every crossing it finds, bounded per body.
#[allow(clippy::too_many_arguments)]
fn section_cap(
    app: &SolveApp,
    st: &solidcraft_engine::doc::ModelState,
    bounds: &[(String, solidcraft_engine::geom::Aabb3)],
    input: &Input,
    planes: &[SectionPlane],
    first_kept: Option<f64>,
    include_ghosted: bool,
    replaced: &[String],
    active_bodies: &Option<crate::viewport::ActiveBodyNames>,
) -> Option<CapHit> {
    let (t, i) = section_entry(planes, input.o, input.d)?;
    if t >= first_kept.unwrap_or(f64::INFINITY) {
        return None;
    }
    let body = body_cut_at(app, st, bounds, input.o, input.d, t, include_ghosted, replaced, active_bodies)?;
    Some(CapHit { t, body, plane: *planes.get(i)? })
}

/// The section plane whose cut face lies under the viewport point `pos` (window points), nearer
/// than any face the section keeps.
pub(crate) fn section_plane_at(app: &SolveApp, pos: Pos2) -> Option<SectionPlane> {
    let section = crate::viewport::section_planes(app);
    if section.is_empty() {
        return None;
    }
    let proj = crate::viewport::projection(app, app.viewport.rect?);
    let (o, d) = proj.ray(pos);
    let raw = Value::Null;
    let input = Input { raw: &raw, pixel: Some(pos), o, d };
    let st = crate::open_meshing::shown_world(app);
    let bounds = crate::viewport::body_bounds(app, &st);
    let active_bodies = crate::viewport::active_body_names(app);
    let replaced = app.preview.effective_replaced();
    let mut cands = face_candidates(app, &st, &bounds, &input, &active_bodies, replaced, false);
    cands.sort_by(|a, b| a.t.total_cmp(&b.t));
    let first_kept = cands.iter().find(|c| section_keeps(&section, c.point)).map(|c| c.t);
    section_cap(app, &st, &bounds, &input, &section, first_kept, false, replaced, &active_bodies).map(|c| c.plane)
}

/// The body the ray `o + d·t` is inside of at `t_cap` (where it crosses the section plane): an
/// odd number of mesh crossings before it. The walk re-raycasts each body past every crossing
/// it finds, bounded per body. The viewport's picker asks the same, so a cut cap hides what lies
/// behind it there too.
#[allow(clippy::too_many_arguments)]
pub(crate) fn body_cut_at(
    app: &SolveApp,
    st: &solidcraft_engine::doc::ModelState,
    bounds: &[(String, solidcraft_engine::geom::Aabb3)],
    o: Vec3,
    d: Vec3,
    t_cap: f64,
    include_ghosted: bool,
    replaced: &[String],
    active_bodies: &Option<crate::viewport::ActiveBodyNames>,
) -> Option<String> {
    let walk_eps = 1e-6 * (1.0 + crate::viewport::scene_radius(app));
    for (i, b) in st.bodies.iter().enumerate() {
        if !app.body_visible(&b.name) || replaced.contains(&b.name) {
            continue;
        }
        if !include_ghosted && crate::viewport::body_faded(app, b, active_bodies) {
            continue;
        }
        // Ghost Frame Members lets picks through a member, its cut too, as `face_candidates` does.
        if crate::viewport::frame_ghosted(app, b) {
            continue;
        }
        // A sheet or an open shell has no inside to cut: a ray crossing it once is not within
        // material, so it must not raise a cap that hides what lies behind (#17177).
        if !b.is_closed() {
            continue;
        }
        if let Some((_, bb)) = bounds.get(i) {
            let pad = bb.diagonal() * 1e-6 + 1e-9;
            match crate::viewport::ray_box(o, d, bb, pad) {
                None => continue,
                Some(t0) if t0 > t_cap => continue,
                _ => {}
            }
        }
        let (m, place) = b.mesh_in_place();
        let index = app.viewport.pick_index.of(&m);
        let (mut t, mut crossings) = (0.0f64, 0usize);
        while let Some((lo, ld)) = crate::viewport::local_ray(&place, o + d * t, d) {
            let Some((dt, _)) = crate::pick_index::raycast(index.as_deref(), &m, lo, ld) else { break };
            let tt = t + dt;
            if tt >= t_cap - SECTION_EPS {
                break;
            }
            crossings += 1;
            t = tt + walk_eps;
            if crossings > MAX_CROSSINGS {
                break;
            }
        }
        if crossings % 2 == 1 {
            return Some(b.name.clone());
        }
    }
    None
}

/// The edge and vertex nearest the pixel, within `edge_px`, hidden geometry excluded the way
/// the mouse picker excludes it. Rays without a pixel get neither (they have no screen reach).
#[allow(clippy::too_many_arguments)]
fn near_edges(
    app: &SolveApp,
    st: &solidcraft_engine::doc::ModelState,
    bounds: &[(String, solidcraft_engine::geom::Aabb3)],
    proj: &crate::viewport::Proj,
    input: &Input,
    pos: Pos2,
    face_t: Option<f64>,
    edge_px: f32,
    active_bodies: &Option<crate::viewport::ActiveBodyNames>,
    replaced: &[String],
    include_ghosted: bool,
    wireframe: bool,
    slack: f64,
    section: &[SectionPlane],
) -> (Option<EdgeHit>, Option<VertexHit>) {
    let reach = crate::pick_index::reach(proj, pos, edge_px.max(6.0));
    // Nothing the section hides is drawn, so the mouse picker skips edges and vertices there; a
    // pick must not answer with what the user cannot see (#10704, the same `kept` test
    // `viewport::pick` applies).
    let kept = |p: Vec3| section_keeps(section, p);
    // `slack` is the body's own: what counts as lying on the face in front depends on how
    // coarsely that body is meshed, never on where the camera stands.
    let visible = |p: Vec3, slack: f64| kept(p) && (wireframe || face_t.is_none_or(|t| (p - input.o).dot(input.d) <= t + slack));
    let mut best_e: Option<EdgeHit> = None;
    let mut best_v: Option<VertexHit> = None;
    for (i, b) in st.bodies.iter().enumerate() {
        if !app.body_visible(&b.name) || replaced.contains(&b.name) {
            continue;
        }
        if !include_ghosted && crate::viewport::body_faded(app, b, active_bodies) {
            continue;
        }
        // A frame member Ghost Frame Members draws see-through: its vertices, not its edges.
        let frame_ghosted = crate::viewport::frame_ghosted(app, b);
        let mut body_slack = slack;
        if let Some((_, bb)) = bounds.get(i) {
            if crate::viewport::screen_box(proj, bb, edge_px).is_some_and(|r| !r.contains(pos)) {
                continue;
            }
            body_slack = crate::viewport::pick_slack(bb.diagonal());
        }
        let (m, place) = b.mesh_in_place();
        let Some((lo, ld)) = crate::viewport::local_ray(&place, input.o, input.d) else { continue };
        let moved = !is_identity(&place);
        let at = |p: Vec3| if moved { apply_point(&place, p) } else { p };
        let near = crate::pick_index::segments_near(app.viewport.pick_index.of(&m).as_deref(), &m, lo, ld, reach);
        let mut last_edge = None;
        for (ei, si) in near {
            let Some(e) = m.edges.get(ei) else { continue };
            if !m.edge_pickable(ei) {
                continue;
            }
            if last_edge != Some(ei) {
                last_edge = Some(ei);
                for v in [e.first(), e.last()].into_iter().flatten().map(|v| at(*v)) {
                    if let Some(sp) = proj.to_screen(v) {
                        let dd = sp.distance(pos);
                        if dd <= edge_px + 1.0 && visible(v, body_slack) && best_v.as_ref().is_none_or(|x| dd < x.dist_px) {
                            best_v = Some(VertexHit { dist_px: dd, point: v });
                        }
                    }
                }
            }
            if frame_ghosted {
                continue;
            }
            let (Some(p0), Some(p1)) = (e.get(si).map(|p| at(*p)), e.get(si + 1).map(|p| at(*p))) else { continue };
            let (Some(a), Some(c)) = (proj.to_screen(p0), proj.to_screen(p1)) else { continue };
            let dd = crate::viewport::seg_dist(pos, a, c);
            if dd <= edge_px
                && visible(crate::viewport::ray_segment_closest(input.o, input.d, p0, p1), body_slack)
                && best_e.as_ref().is_none_or(|x| dd < x.dist_px)
            {
                let k = seg_param(pos, a, c);
                best_e = Some(EdgeHit { index: ei, dist_px: dd, point: p0 + (p1 - p0) * f64::from(k) });
            }
        }
    }
    (best_e, best_v)
}

struct EdgeHit {
    index: usize,
    dist_px: f32,
    point: Vec3,
}

struct VertexHit {
    dist_px: f32,
    point: Vec3,
}

/// Where on the screen segment `a`–`b` the pixel `p` lies closest, as a 0..1 parameter.
fn seg_param(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    if l2 < 1e-9 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) }
}

/// Body world name → outermost placement, walked breadth-first so a shallower placement
/// wins; bounded like the engine's own occurrence expansion.
fn occurrence_map(app: &SolveApp) -> HashMap<String, Placed> {
    let mut out: HashMap<String, Placed> = HashMap::new();
    let mut queue: std::collections::VecDeque<Vec<u64>> = std::collections::VecDeque::from([vec![]]);
    let mut budget = 10_000usize;
    while let Some(path) = queue.pop_front() {
        if budget == 0 {
            break;
        }
        let Ok(children) = app.session.occurrence_children(&path) else { continue };
        for placement in children {
            budget -= 1;
            for b in &placement.bodies {
                out.entry(b.world.clone()).or_insert_with(|| Placed { path: placement.path.clone(), name: placement.name.clone() });
            }
            queue.push_back(placement.path.clone());
        }
    }
    out
}

/// Body name → the component that owns it, by name (the root component names nothing).
fn component_names(app: &SolveApp) -> HashMap<String, String> {
    let doc = &app.session.doc;
    let names: HashMap<u64, String> = doc.components.iter().map(|c| (c.id, c.name.clone())).collect();
    doc.body_components.iter().filter_map(|(body, id)| names.get(id).map(|n| (body.clone(), n.clone()))).collect()
}

fn v3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn hit_json(
    input: &Input,
    c: &Cand,
    occ: &HashMap<String, Placed>,
    comps: &HashMap<String, String>,
    edge: Option<&EdgeHit>,
    vertex: Option<&VertexHit>,
) -> Value {
    let mut j = json!({
        "pixel": input.raw,
        "body": c.body,
        "face": c.face,
        "point": v3(c.point),
        "normal": v3(c.normal),
        "depth": c.t,
    });
    if let Some(p) = occ.get(&c.body) {
        j["occurrence"] = json!(p.path);
        j["occurrence_name"] = json!(p.name);
    }
    if let Some(name) = comps.get(&c.body) {
        j["component"] = json!(name);
    }
    if c.ghosted {
        j["ghosted"] = json!(true);
    }
    if let Some(e) = edge {
        j["edge"] = edge_json(e);
    }
    if let Some(v) = vertex {
        j["vertex"] = vertex_json(v);
    }
    j
}

fn cap_json(input: &Input, cap: &CapHit) -> Value {
    let point = input.o + input.d * cap.t;
    json!({
        "pixel": input.raw, "body": cap.body, "face": null, "section_cap": true, "section_plane": cap.plane.id,
        "point": v3(point), "normal": v3(cap.plane.normal), "depth": cap.t,
    })
}

fn edge_json(e: &EdgeHit) -> Value {
    json!({ "index": e.index, "point": v3(e.point), "distance_px": e.dist_px })
}

fn vertex_json(v: &VertexHit) -> Value {
    json!({ "point": v3(v.point), "distance_px": v.dist_px })
}

fn cloud_json(input: &Input, h: &solidcraft_engine::point_cloud_pick::CloudPickHit, dist_px: Option<f32>) -> Value {
    let mut j = json!({
        "pixel": input.raw,
        "body": null,
        "face": null,
        "cloud": h.cloud,
        "cloud_name": h.cloud_name,
        "point_index": h.index,
        "point": v3(h.point),
        "depth": h.depth,
    });
    if let Some(px) = dist_px {
        j["distance_px"] = json!(px);
    } else {
        j["distance_mm"] = json!(h.ray_dist);
    }
    j
}

/// Pick visible point-cloud points along `input`, nearest first. For a pixel, uses screen-space
/// distance within `point_px` (or each cloud's dot radius + 2 px, minimum 6 px); for a world ray,
/// uses perpendicular distance within `cloud_radius` mm.
#[allow(clippy::too_many_arguments)]
fn cloud_candidates(
    app: &SolveApp,
    proj: &crate::viewport::Proj,
    input: &Input,
    max_depth: Option<f64>,
    point_px: Option<f32>,
    cloud_radius: f64,
    section: &[SectionPlane],
    all: bool,
) -> Vec<(solidcraft_engine::point_cloud_pick::CloudPickHit, Option<f32>)> {
    if input.pixel.is_some() && point_px.is_some_and(|p| p <= 0.0) {
        return Vec::new();
    }
    if input.pixel.is_none() && cloud_radius <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Every cloud is searched to the same depth: the nearest across clouds is chosen after.
    for raw_cloud in solidcraft_engine::view::shown_point_clouds(&app.session) {
        if raw_cloud.is_empty() {
            continue;
        }
        let playing = |k: usize| app.tree.cloud_panel.as_ref().and_then(|p| p.playing(raw_cloud.id, k));
        let cloud = crate::point_clouds::as_drawn(raw_cloud, playing);
        let index = app.viewport.cloud_pick_index.of(raw_cloud.points.values());
        let hit = match input.pixel {
            Some(pos) => {
                let px = point_px.unwrap_or_else(|| ((cloud.point_size as f32) * 0.5 + 2.0).max(6.0));
                let (a, b) = crate::pick_index::reach(proj, pos, px);
                solidcraft_engine::point_cloud_pick::pick_cloud_screen(&cloud, index.as_deref(), input.o, input.d, a, b, max_depth, section, |w| {
                    proj.to_screen(w).map(|sp| sp.distance(pos)).filter(|d| *d <= px)
                })
                .map(|(h, dpx)| (h, Some(dpx)))
            }
            None => {
                solidcraft_engine::point_cloud_pick::pick_cloud_ray(&cloud, index.as_deref(), input.o, input.d, cloud_radius, 0.0, max_depth, section)
                    .map(|h| (h, None))
            }
        };
        if let Some(h) = hit {
            out.push(h);
        }
    }
    out.sort_by(|a, b| a.0.depth.total_cmp(&b.0.depth));
    if !all {
        out.truncate(1);
    }
    out
}

#[cfg(test)]
#[path = "view_pick_tests.rs"]
mod tests;
