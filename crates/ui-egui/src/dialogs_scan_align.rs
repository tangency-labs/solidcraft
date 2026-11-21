//! Live-preview 3-2-1 scan datum alignment dialog (`scan.align`, #14353).
//!
//! Lets the user select a point cloud or mesh occurrence(s), pick a primary plane (3+ measured
//! points or an existing plane/planar face), a secondary direction (2 points or an axis/edge),
//! and an explicit origin (projected onto the primary plane), then preview the rigid (`det = +1`)
//! alignment with 3D source/target axis triads and RMS/max absolute fit residuals before applying
//! in one undo step.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use egui::{Pos2, RichText, Stroke};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::doc::construct::ConstructGeom;
use solidcraft_engine::doc::{Mat, apply_point, mat_mul};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::schema::scan_align::ScanAlignResult;
use solidcraft_engine::{Sel, Session};
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as ui_kit;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::selection::{
    AXES, BODIES, CLOUD_POINTS, CONSTRUCTION_POINTS, EDGES, ORIGIN, PLANAR_FACES, PLANES, POINTS, SNAP_POINTS, SelInput, VERTICES,
};
use crate::viewport::Proj;

const PLANE_PICKS: crate::selection::Accept = CLOUD_POINTS | VERTICES | PLANES | PLANAR_FACES | CONSTRUCTION_POINTS | POINTS | SNAP_POINTS;
const DIR_PICKS: crate::selection::Accept = CLOUD_POINTS | VERTICES | AXES | EDGES | CONSTRUCTION_POINTS | POINTS | SNAP_POINTS;
const ORIGIN_PICKS: crate::selection::Accept = CLOUD_POINTS | VERTICES | ORIGIN | CONSTRUCTION_POINTS | POINTS | SNAP_POINTS;

/// Dialog state for `scan.align`.
#[derive(Clone, Debug)]
pub struct ScanAlignForm {
    /// Selected point cloud id, or `None` when aligning mesh occurrences from `inputs[0]`.
    pub target_cloud: Option<u64>,
    /// Target frame: `"world"`, `"XZ"`, `"YZ"`, or a construction plane name.
    pub target_frame: String,
    /// Reverse the primary plane normal (`Z`) while keeping a right-handed frame (`det = +1`).
    pub flip_normal: bool,
    /// Reverse the projected in-plane direction (`X`) while keeping a right-handed frame.
    pub flip_direction: bool,
    /// Cached non-mutating preview result keyed by `(doc_revision, input_hash)`.
    pub cached: Option<(u64, Result<ScanAlignResult, String>)>,
}

fn plane_idx(form: &ScanAlignForm) -> usize {
    usize::from(form.target_cloud.is_none())
}

fn sync_inputs(d: &mut Dialog, has_cloud: bool) {
    if has_cloud && d.inputs.len() == 4 {
        d.inputs.remove(0);
        d.active = d.active.saturating_sub(1);
    } else if !has_cloud && d.inputs.len() == 3 {
        d.inputs.insert(0, SelInput::new("Mesh occurrences", BODIES, true));
        d.active = 0;
    }
}

/// Start a new `scan.align` dialog, pre-selecting a cloud if one is active/only cloud and no
/// occurrence is preselected.
pub(crate) fn start(s: &Session) -> Dialog {
    let pre_occ = s.selection.iter().any(|x| matches!(x, Sel::Occurrence { .. }));
    let pre_cloud = s.selection.iter().find_map(|x| match x {
        Sel::PointCloudPoint { cloud, .. } => Some(*cloud),
        _ => None,
    });
    let target_cloud = if pre_occ {
        None
    } else if let Some(cid) = pre_cloud {
        Some(cid)
    } else if s.doc.point_clouds.len() == 1 {
        s.doc.point_clouds.first().map(|c| c.id)
    } else {
        None
    };

    let form = ScanAlignForm { target_cloud, target_frame: "world".into(), flip_normal: false, flip_direction: false, cached: None };
    let mut inputs = Vec::with_capacity(4);
    if target_cloud.is_none() {
        inputs.push(SelInput::new("Mesh occurrences", BODIES, true));
    }
    inputs.push(SelInput::new("Primary plane", PLANE_PICKS, true));
    inputs.push(SelInput::new("Direction", DIR_PICKS, true));
    inputs.push(SelInput::new("Origin", ORIGIN_PICKS, false));
    Dialog::new(Kind::ScanAlign(Box::new(form)), inputs)
}

/// Extract a 3D world point from any point-like selection item.
fn sel_point(app: &SolveApp, sel: &Sel) -> Option<[f64; 3]> {
    match sel {
        Sel::PointCloudPoint { point, .. } | Sel::Vertex { point, .. } | Sel::Face { point, .. } | Sel::Edge { point, .. } => {
            Some([point.x, point.y, point.z])
        }
        Sel::ConstructionPoint { name } => {
            let st = crate::open_meshing::shown_world(app);
            st.construct.iter().find_map(|c| match &c.geom {
                ConstructGeom::Point(p) if c.name == *name => Some([p.x, p.y, p.z]),
                _ => None,
            })
        }
        Sel::SketchPoint { id } => {
            let (sk, pt) = id.split_once(':')?;
            let sid: u64 = sk.parse().ok()?;
            let st = crate::open_meshing::shown_world(app);
            solidcraft_engine::doc::construct::sketch_point_position(&st, sid, pt).map(|p| [p.x, p.y, p.z])
        }
        _ => None,
    }
}

/// Extract occurrence paths from `inputs[0]`.
fn occurrence_paths(s: &Session, items: &[Sel]) -> Vec<Vec<u64>> {
    let mut out = Vec::new();
    for sel in items {
        let path = match sel {
            Sel::Occurrence { path } if !path.is_empty() => Some(path.clone()),
            Sel::Body { name } => crate::dialogs::occurrence_path_of_body(s, name),
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => crate::dialogs::occurrence_path_of_body(s, body),
            _ => None,
        };
        if let Some(p) = path
            && !out.contains(&p)
        {
            out.push(p);
        }
    }
    out
}

/// Build the `scan.align` command JSON from dialog state.
pub(crate) fn build_params(app: &SolveApp, d: &Dialog, form: &ScanAlignForm, preview: bool) -> Result<Value, String> {
    let mut p = json!({
        "flip_normal": form.flip_normal,
        "flip_direction": form.flip_direction,
        "preview": preview,
    });
    if !form.target_frame.is_empty() && form.target_frame != "world" {
        p["target"] = json!(form.target_frame);
    }

    // Target: point cloud OR mesh occurrence(s)
    if let Some(cid) = form.target_cloud {
        if !app.session.doc.point_clouds.iter().any(|c| c.id == cid) {
            return Err(solidcraft_ui_text::tr("scan_align.err_pick_target").to_string());
        }
        p["cloud"] = json!(cid);
    } else {
        let items = d.inputs.first().map_or(&[][..], |i| i.items.as_slice());
        let paths = occurrence_paths(&app.session, items);
        if paths.is_empty() {
            return Err(solidcraft_ui_text::tr("scan_align.err_pick_target").to_string());
        }
        p["paths"] = json!(paths);
    }

    // Primary plane (existing plane / planar face OR >= 3 measured points)
    let base = plane_idx(form);
    let plane_items = d.inputs.get(base).map_or(&[][..], |i| i.items.as_slice());
    if plane_items.is_empty() {
        return Err(solidcraft_ui_text::tr("scan_align.err_need_plane").to_string());
    }
    if plane_items.len() == 1 {
        match &plane_items[0] {
            Sel::Plane { name } => p["plane"] = json!(name),
            Sel::Face { point, .. } => p["plane"] = json!({"face": [point.x, point.y, point.z]}),
            _ => return Err(solidcraft_ui_text::tr("scan_align.err_need_three_points").to_string()),
        }
    } else {
        let mut pts = Vec::with_capacity(plane_items.len());
        for sel in plane_items {
            if let Some(pt) = sel_point(app, sel) {
                pts.push(pt);
            }
        }
        if pts.len() < 3 {
            return Err(solidcraft_ui_text::tr("scan_align.err_need_three_points").to_string());
        }
        p["plane"] = json!({"points": pts});
    }

    // Secondary direction (existing axis OR 2 distinct points OR straight edge)
    let dir_items = d.inputs.get(base + 1).map_or(&[][..], |i| i.items.as_slice());
    if dir_items.is_empty() {
        return Err(solidcraft_ui_text::tr("scan_align.err_need_direction").to_string());
    }
    if dir_items.len() == 1 {
        match &dir_items[0] {
            Sel::Axis { name } => p["direction"] = json!(name),
            Sel::Edge { body, index, .. } => {
                let st = crate::open_meshing::shown_world(app);
                let edge_dir = st.body(body).and_then(|b| {
                    let (m, place) = b.mesh_in_place();
                    let e = m.edges.get(*index)?;
                    let a = apply_point(&place, *e.first()?);
                    let c = apply_point(&place, *e.last()?);
                    let v = c - a;
                    (v.len() > 1e-9).then_some([v.x, v.y, v.z])
                });
                match edge_dir {
                    Some(v) => p["direction"] = json!({"vector": v}),
                    None => {
                        return Err(solidcraft_ui_text::tr("scan_align.err_need_two_dir_points").to_string());
                    }
                }
            }
            _ => {
                return Err(solidcraft_ui_text::tr("scan_align.err_need_two_dir_points").to_string());
            }
        }
    } else {
        let a = sel_point(app, &dir_items[0]).ok_or_else(|| solidcraft_ui_text::tr("scan_align.err_need_two_dir_points").to_string())?;
        let b = sel_point(app, &dir_items[1]).ok_or_else(|| solidcraft_ui_text::tr("scan_align.err_need_two_dir_points").to_string())?;
        p["direction"] = json!({"from": a, "to": b});
    }

    // Explicit origin point
    let origin_items = d.inputs.get(base + 2).map_or(&[][..], |i| i.items.as_slice());
    let Some(first_origin) = origin_items.first() else {
        return Err(solidcraft_ui_text::tr("scan_align.err_need_origin").to_string());
    };
    let origin_pt = sel_point(app, first_origin).ok_or_else(|| solidcraft_ui_text::tr("scan_align.err_need_origin").to_string())?;
    p["origin"] = json!(origin_pt);

    Ok(p)
}

/// Command tuple for OK / occurrence live preview.
pub(crate) fn command(app: &SolveApp, d: &Dialog, form: &ScanAlignForm) -> Result<(&'static str, Value), String> {
    let p = build_params(app, d, form, false)?;
    Ok(("scan.align", p))
}

fn cache_key(app: &SolveApp, d: &Dialog, form: &ScanAlignForm) -> u64 {
    let mut h = DefaultHasher::new();
    app.session.revision.hash(&mut h);
    form.target_cloud.hash(&mut h);
    form.target_frame.hash(&mut h);
    form.flip_normal.hash(&mut h);
    form.flip_direction.hash(&mut h);
    for inp in &d.inputs {
        inp.items.len().hash(&mut h);
        for sel in &inp.items {
            format!("{sel:?}").hash(&mut h);
        }
    }
    h.finish()
}

/// Compute (or return cached) non-mutating alignment preview solution.
fn evaluate_preview(app: &SolveApp, d: &Dialog, form: &ScanAlignForm) -> Result<ScanAlignResult, String> {
    let params = build_params(app, d, form, true)?;
    solidcraft_engine::preview_scan_align(&app.session, &params).map_err(|e| e.to_string())
}

/// Live preview composed placement matrix for a point cloud while the `ScanAlign` dialog is open,
/// without mutating `app.session.doc`.
pub(crate) fn preview_cloud_transform(app: &SolveApp, cloud_id: u64) -> Option<Mat> {
    let d = app.dialog.as_ref()?;
    let Kind::ScanAlign(form) = &d.kind else { return None };
    if form.target_cloud != Some(cloud_id) {
        return None;
    }
    let key = cache_key(app, d, form);
    let owned_res;
    let sol = if let Some((cached_key, cached_res)) = form.cached.as_ref()
        && *cached_key == key
    {
        cached_res.as_ref().ok()?
    } else {
        owned_res = evaluate_preview(app, d, form);
        owned_res.as_ref().ok()?
    };
    let cloud = app.session.doc.point_clouds.iter().find(|c| c.id == cloud_id)?;
    Some(mat_mul(&sol.world_transform, &cloud.transform))
}

/// Draw the `scan.align` dialog fields inside the standard dialog card.
pub(crate) fn fields(ui: &mut egui::Ui, app: &SolveApp, d: &mut Dialog) {
    let Kind::ScanAlign(form_box) = &mut d.kind else { return };

    // Keep target_cloud valid if the cloud was deleted externally.
    let mut switched_cloud: Option<bool> = None;
    if let Some(cid) = form_box.target_cloud
        && !app.session.doc.point_clouds.iter().any(|c| c.id == cid)
    {
        form_box.target_cloud = None;
        switched_cloud = Some(false);
    }

    // 1. Target selector: any point cloud in the design, or mesh occurrence picks.
    let mut choices: Vec<(Option<u64>, String)> = Vec::new();
    for c in &app.session.doc.point_clouds {
        choices.push((Some(c.id), c.name.clone()));
    }
    choices.push((None, solidcraft_ui_text::tr("scan_align.target_mesh_occurrences").to_string()));
    let selected_idx = choices.iter().position(|(id, _)| *id == form_box.target_cloud).unwrap_or(choices.len().saturating_sub(1));
    let labels: Vec<&str> = choices.iter().map(|(_, s)| s.as_str()).collect();
    let mut picked = selected_idx;
    ui_kit::choice_row(ui, solidcraft_ui_text::tr("scan_align.target_label"), "scan_align_target", &mut picked, &labels);
    if let Some((next_cloud, _)) = choices.get(picked)
        && *next_cloud != form_box.target_cloud
    {
        form_box.target_cloud = *next_cloud;
        switched_cloud = Some(form_box.target_cloud.is_some());
    }

    // 2. Target frame selector: World Origin (XY), XZ, YZ, or any construction plane.
    let mut frames: Vec<(String, String)> =
        vec![("world".into(), solidcraft_ui_text::tr("scan_align.frame_world").to_string()), ("XZ".into(), "XZ".into()), ("YZ".into(), "YZ".into())];
    let st = crate::open_meshing::shown_world(app);
    for c in &st.construct {
        if matches!(c.geom, ConstructGeom::Plane(_)) && !frames.iter().any(|(k, _)| *k == c.name) {
            frames.push((c.name.clone(), c.name.clone()));
        }
    }
    let frame_idx = frames.iter().position(|(k, _)| *k == form_box.target_frame).unwrap_or(0);
    let frame_labels: Vec<&str> = frames.iter().map(|(_, l)| l.as_str()).collect();
    let mut picked_frame = frame_idx;
    ui_kit::choice_row(ui, solidcraft_ui_text::tr("scan_align.target_frame_label"), "scan_align_frame", &mut picked_frame, &frame_labels);
    if let Some((k, _)) = frames.get(picked_frame) {
        form_box.target_frame.clone_from(k);
    }

    // 3. Axis reversal checkboxes (keep right-handed det = +1).
    let flip_n_lbl = solidcraft_ui_text::tr("scan_align.flip_normal");
    let flip_d_lbl = solidcraft_ui_text::tr("scan_align.flip_direction");
    ui_kit::options(ui, "scan_align_options", &mut [(flip_n_lbl, &mut form_box.flip_normal), (flip_d_lbl, &mut form_box.flip_direction)]);

    // 4. Sync input rows if target kind changed, then evaluate cached non-mutating preview.
    let (form_snapshot, prev_key) = (form_box.clone(), form_box.cached.as_ref().map(|(k, _)| *k));
    if let Some(has_cloud) = switched_cloud {
        sync_inputs(d, has_cloud);
    }
    let key = cache_key(app, d, &form_snapshot);
    if prev_key != Some(key) {
        let res = evaluate_preview(app, d, &form_snapshot);
        if let Kind::ScanAlign(f) = &mut d.kind {
            f.cached = Some((key, res));
        }
    }

    let t = Tokens::get();
    let Kind::ScanAlign(form_box) = &d.kind else { return };
    if form_box.target_cloud.is_none() && crate::dialogs::align_moves_shared(&app.session, d.inputs.first().map_or(&[], |i| i.items.as_slice())) {
        ui.add(egui::Label::new(RichText::new(solidcraft_ui_text::tr("scan_align.shared_warning")).color(t.warning).size(t.font_small)).wrap());
    }

    match form_box.cached.as_ref().map(|(_, r)| r) {
        Some(Ok(sol)) => {
            let moved = sol.moved_names.join(", ");
            ui_kit::status_line(ui, &solidcraft_ui_text::format("scan_align.will_move", &[("names", &moved)]), ui_kit::StatusTone::Dim);
            if sol.plane.sample_count >= 3 {
                let rms = crate::length_input::model_drag(&app.session.doc, sol.plane.rms_residual_mm, 0.001, 3);
                let max = crate::length_input::model_drag(&app.session.doc, sol.plane.max_abs_residual_mm, 0.001, 3);
                let tone = if sol.plane.is_planar { ui_kit::StatusTone::Dim } else { ui_kit::StatusTone::Warning };
                ui_kit::status_line(
                    ui,
                    &solidcraft_ui_text::format(
                        "scan_align.plane_residuals",
                        &[("count", &sol.plane.sample_count.to_string()), ("rms", &rms), ("max", &max)],
                    ),
                    tone,
                );
            }
            if sol.origin.off_plane_distance_mm > 1e-4 {
                let off = crate::length_input::model_drag(&app.session.doc, sol.origin.off_plane_distance_mm, 0.001, 3);
                ui_kit::status_line(ui, &solidcraft_ui_text::format("scan_align.origin_projected", &[("offset", &off)]), ui_kit::StatusTone::Dim);
            }
        }
        Some(Err(msg)) => {
            ui_kit::status_line(ui, msg, ui_kit::StatusTone::Warning);
        }
        None => {}
    }
}

fn arr_vec(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn draw_triad(painter: &egui::Painter, proj: &Proj, origin: Vec3, x: Vec3, y: Vec3, z: Vec3, axis_len: f64, alpha: f32, width: f32, t: &Tokens) {
    let Some(o2) = proj.to_screen(origin) else { return };
    for (dir, color) in [(x, t.axis_x), (y, t.axis_y), (z, t.axis_z)] {
        let Some(end2) = proj.to_screen(origin + dir * axis_len) else { continue };
        painter.line_segment([o2, end2], Stroke::new(width, color.gamma_multiply(alpha)));
    }
    painter.circle_filled(o2, 3.5, t.text.gamma_multiply(alpha));
    painter.circle_stroke(o2, 4.5, Stroke::new(1.5, t.accent.gamma_multiply(alpha)));
}

/// Draw 3D datum triads (source and target frames) and off-plane origin projection on the canvas.
pub(crate) fn overlay(_app: &SolveApp, painter: &egui::Painter, proj: &Proj, d: &Dialog) {
    let Kind::ScanAlign(form) = &d.kind else { return };
    let t = Tokens::get();
    let Some((_, Ok(sol))) = form.cached.as_ref() else { return };

    // Choose a readable triad axis length proportional to camera distance.
    let src_o = arr_vec(sol.source_frame.origin);
    let tgt_o = arr_vec(sol.target_frame.origin);
    let cam_dist = proj.cam.eye().dist(src_o).max(1.0);
    let axis_len = (cam_dist * 0.14).clamp(5.0, 200.0);

    // Draw target frame triad (subtle, dashed-like lower alpha) and source frame triad (bold).
    draw_triad(
        painter,
        proj,
        tgt_o,
        arr_vec(sol.target_frame.x_axis),
        arr_vec(sol.target_frame.y_axis),
        arr_vec(sol.target_frame.z_axis),
        axis_len,
        0.55,
        1.75,
        &t,
    );
    draw_triad(
        painter,
        proj,
        src_o,
        arr_vec(sol.source_frame.x_axis),
        arr_vec(sol.source_frame.y_axis),
        arr_vec(sol.source_frame.z_axis),
        axis_len,
        1.0,
        2.5,
        &t,
    );

    // If the picked origin lies off-plane, show both positions connected by a projection line.
    if sol.origin.off_plane_distance_mm > 1e-4 {
        let raw_o = arr_vec(sol.origin.raw_origin);
        if let (Some(a), Some(b)) = (proj.to_screen(raw_o), proj.to_screen(src_o)) {
            draw_dashed(painter, a, b, Stroke::new(1.5, t.warning));
            painter.circle_filled(a, 3.5, t.warning);
            painter.circle_stroke(a, 5.0, Stroke::new(1.25, t.text));
        }
    }
}

fn draw_dashed(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    let d = b - a;
    let len = d.length();
    if len < 1.0 {
        return;
    }
    let step = 8.0_f32;
    let dir = d / len;
    let mut s = 0.0_f32;
    while s < len {
        let e = (s + 4.5).min(len);
        painter.line_segment([a + dir * s, a + dir * e], stroke);
        s += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::apply_commands;
    use crate::scenario::Harness;

    fn insert_tilted_cloud(h: &mut Harness) -> (u64, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("solidcraft-ui-scan-align-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ply = dir.join("tilted_scan.ply");
        // 4 points on a 30°-tilted plane + 1 off-plane origin point (+10 mm along tilted +Z above pt 0)
        let text = "\
ply
format ascii 1.0
element vertex 5
property float x
property float y
property float z
end_header
10.0 20.0 5.0
35.980762 20.0 20.0
35.980762 50.0 20.0
10.0 50.0 5.0
5.0 20.0 13.660254
";
        std::fs::write(&ply, text).unwrap();
        let ins = h.app.run("pointcloud.insert", json!({"path": ply.to_string_lossy()})).unwrap();
        (ins["cloud"].as_u64().unwrap(), dir)
    }

    #[test]
    fn scan_align_dialog_previews_without_mutating_cancels_cleanly_and_applies_in_one_undo_step() {
        let mut h = Harness::new();
        let (cloud_id, tmp_dir) = insert_tilted_cloud(&mut h);
        let initial_transform = h.app.session.doc.point_clouds[0].transform;
        let undo_before = h.app.session.undo.len();

        h.app.start("scan.align");
        h.frames(2);
        {
            let d = h.app.dialog.as_ref().expect("scan.align dialog opens");
            let Kind::ScanAlign(form) = &d.kind else { panic!("expected ScanAlign dialog") };
            assert_eq!(form.target_cloud, Some(cloud_id));
            assert_eq!(d.inputs.len(), 3, "cloud mode omits mesh-occurrence picker");
            assert_eq!(d.inputs[0].label, "Primary plane");
            assert_eq!(d.active, 0, "starts directly on Primary plane when single cloud is auto-selected");
        }

        let picks: Vec<Sel> = (0..5)
            .map(|idx| {
                let p = h.app.session.doc.point_clouds[0].world_point(idx).unwrap();
                Sel::PointCloudPoint { cloud: cloud_id, index: idx, point: p }
            })
            .collect();
        {
            let d = h.app.dialog.as_mut().unwrap();
            d.inputs[0].items = vec![picks[0].clone(), picks[1].clone(), picks[2].clone(), picks[3].clone()];
            d.inputs[1].items = vec![picks[0].clone(), picks[1].clone()];
            d.inputs[2].items = vec![picks[4].clone()]; // off-plane origin: 10 mm along tilted +Z
        }
        h.frames(2);

        // Live preview populates cached residuals and preview_cloud_transform without mutating doc.
        assert_eq!(h.app.session.doc.point_clouds[0].transform, initial_transform);
        assert_eq!(h.app.session.undo.len(), undo_before);
        let preview_mat = preview_cloud_transform(&h.app, cloud_id).expect("live preview transform ready");
        assert_ne!(preview_mat, initial_transform);
        {
            let d = h.app.dialog.as_ref().unwrap();
            let Kind::ScanAlign(form) = &d.kind else { panic!("expected ScanAlign") };
            let sol = form.cached.as_ref().expect("preview cached").1.as_ref().expect("valid solution");
            assert!(sol.plane.rms_residual_mm < 1e-4);
            assert!((sol.origin.off_plane_distance_mm - 10.0).abs() < 1e-3);
            assert!((sol.determinant - 1.0).abs() < 1e-9);
        }

        // Flip normal updates preview deterministically while preserving det = +1.
        {
            let d = h.app.dialog.as_mut().unwrap();
            if let Kind::ScanAlign(form) = &mut d.kind {
                form.flip_normal = true;
            }
        }
        h.frames(2);
        let flipped_mat = preview_cloud_transform(&h.app, cloud_id).expect("flipped preview ready");
        assert_ne!(flipped_mat, preview_mat);
        {
            let d = h.app.dialog.as_ref().unwrap();
            let Kind::ScanAlign(form) = &d.kind else { panic!("expected ScanAlign") };
            let sol = form.cached.as_ref().unwrap().1.as_ref().unwrap();
            assert!((sol.determinant - 1.0).abs() < 1e-9);
            assert_eq!(h.app.session.doc.point_clouds[0].transform, initial_transform);
        }

        // Cancel leaves document completely untouched.
        h.app.dialog = None;
        h.frames(1);
        assert_eq!(h.app.session.doc.point_clouds[0].transform, initial_transform);
        assert_eq!(h.app.session.undo.len(), undo_before);

        // Reopen, pick again, and apply OK in one undo step.
        h.app.start("scan.align");
        h.frames(1);
        {
            let d = h.app.dialog.as_mut().unwrap();
            d.inputs[0].items = vec![picks[0].clone(), picks[1].clone(), picks[2].clone(), picks[3].clone()];
            d.inputs[1].items = vec![picks[0].clone(), picks[1].clone()];
            d.inputs[2].items = vec![picks[4].clone()];
        }
        let cmds = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].0, "scan.align");
        h.app.run(&cmds[0].0, cmds[0].1.clone()).unwrap();
        h.app.dialog = None;

        assert_eq!(h.app.session.undo.len(), undo_before + 1);
        let aligned0 = h.app.session.doc.point_clouds[0].world_point(0).unwrap();
        let aligned1 = h.app.session.doc.point_clouds[0].world_point(1).unwrap();
        assert!(aligned0.dist(Vec3::ZERO) < 1e-3, "aligned origin landed at {aligned0:?}");
        assert!(aligned1.dist(Vec3::new(30.0, 0.0, 0.0)) < 1e-3, "aligned +X landed at {aligned1:?}");

        // Undo restores initial transform in one step.
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.point_clouds[0].transform, initial_transform);

        let _ = std::fs::remove_dir_all(tmp_dir);
    }
}
