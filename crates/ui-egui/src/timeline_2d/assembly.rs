//! Assembly & Post-Modeling Joint Columns, Visual Mates, and Track Projections.

use super::canvas::{CHIP_HEIGHT, CHIP_WIDTH, COL_WIDTH, HEADER_BANNER_HEIGHT, HEADER_T1_HEIGHT, HEADER_T2_HEIGHT, HEADER_TOTAL_HEIGHT, ROW_HEIGHT};
use super::state::Timeline2DState;
use crate::SolveApp;
use crate::dialogs::Dialog;
use crate::theme::Tokens;
use egui::{Align2, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::Value;
use solidcraft_engine::doc::Document;
use solidcraft_engine::doc::joints::JointKind;
use std::collections::{BTreeMap, HashSet};

pub const SPLITTER_WIDTH: f32 = 18.0;
pub const DOCK_STRIP_WIDTH: f32 = 34.0;
pub const SNAP_THRESHOLD: f32 = 46.0;
pub const EXPAND_THRESHOLD: f32 = 52.0;

/// Represents a visual joint column in the post-modeling Assembly timeline track.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualJoint {
    pub id: u64,
    pub name: String,
    pub kind: JointKind,
    pub primary_track_idx: usize,
    pub secondary_track_indices: Vec<usize>,
    pub suppressed: bool,
    pub step_num: usize,
    pub step_label: String,
    pub a_comp_name: String,
    pub b_comp_name: String,
    pub limits: Option<String>,
}

/// Resolves an occurrence ID to its containing component ID in the document.
pub fn occurrence_to_component_id(doc: &Document, occ_id: u64) -> u64 {
    if occ_id == 0 { 0 } else { doc.occurrences.iter().find(|o| o.id == occ_id).map(|o| o.component).unwrap_or(0) }
}

/// Maps a component ID to its visual track index.
pub fn component_id_to_track_idx(component_ids: &[u64], comp_id: u64) -> usize {
    component_ids.iter().position(|&id| id == comp_id).unwrap_or(0)
}

/// Collects component IDs and names from the document in layout order (root first).
pub fn collect_component_ids_and_names(doc: &Document) -> (Vec<u64>, BTreeMap<u64, String>) {
    let mut component_ids: Vec<u64> = vec![0];
    let mut component_names: BTreeMap<u64, String> = BTreeMap::new();
    let root_name = if doc.name.is_empty() { "Root Component".to_string() } else { doc.name.clone() };
    component_names.insert(0, root_name);
    for comp in &doc.components {
        if !component_ids.contains(&comp.id) {
            component_ids.push(comp.id);
            component_names.insert(comp.id, comp.name.clone());
        }
    }
    (component_ids, component_names)
}

/// Discovers and builds visual joint representations for the assembly timeline track.
pub fn build_visual_joints(doc: &Document, component_ids: &[u64], component_names: &BTreeMap<u64, String>) -> Vec<VisualJoint> {
    doc.assembly
        .joints
        .iter()
        .enumerate()
        .map(|(idx, joint)| {
            let step_num = idx + 1;
            let step_label = format!("J{:02}", step_num);

            let comp_a_id = occurrence_to_component_id(doc, joint.a.occurrence);
            let comp_b_id = occurrence_to_component_id(doc, joint.b.occurrence);

            let track_a = component_id_to_track_idx(component_ids, comp_a_id);
            let track_b = component_id_to_track_idx(component_ids, comp_b_id);

            let default_name = "Component".to_string();
            let a_comp_name = component_names.get(&comp_a_id).unwrap_or(&default_name).clone();
            let b_comp_name = component_names.get(&comp_b_id).unwrap_or(&default_name).clone();

            let mut secondary = Vec::new();
            if track_b != track_a && !secondary.contains(&track_b) {
                secondary.push(track_b);
            }

            let fmt = |i: usize, v: f64| {
                if joint.kind.is_angle(i) { format!("{:.1} deg", v.to_degrees()) } else { crate::length_input::model(doc, v) }
            };
            let parts: Vec<String> =
                joint.limits.iter().enumerate().filter_map(|(i, lim)| lim.map(|(min, max)| format!("[{}, {}]", fmt(i, min), fmt(i, max)))).collect();
            let limits = if parts.is_empty() { None } else { Some(parts.join(", ")) };

            VisualJoint {
                id: joint.id,
                name: joint.name.clone(),
                kind: joint.kind,
                primary_track_idx: track_a,
                secondary_track_indices: secondary,
                suppressed: joint.suppressed,
                step_num,
                step_label,
                a_comp_name,
                b_comp_name,
                limits,
            }
        })
        .collect()
}

/// Formats a rich hover tooltip for an assembly joint chip.
pub fn format_joint_tooltip(joint: &VisualJoint, is_active: bool) -> String {
    let kind_str = match joint.kind {
        JointKind::Rigid => "Rigid",
        JointKind::Revolute => "Revolute",
        JointKind::Slider => "Slider",
        JointKind::Cylindrical => "Cylindrical",
        JointKind::PinSlot => "Pin Slot",
        JointKind::Planar => "Planar",
        JointKind::Ball => "Ball",
        JointKind::Parallel => "Parallel",
        JointKind::Perpendicular => "Perpendicular",
        JointKind::Angle => "Angle",
        JointKind::PointOnPlane => "Point On Plane",
        JointKind::PointOnLine => "Point On Line",
        JointKind::LineOnPlane => "Line On Plane",
    };

    let status = if joint.suppressed {
        "Suppressed"
    } else if is_active {
        "Active"
    } else {
        "Past assembly sequence marker"
    };

    let mut tip =
        format!("{}: {} ({})\nConnects: {} ↔ {}\nStatus: {}", joint.step_label, joint.name, kind_str, joint.a_comp_name, joint.b_comp_name, status);

    if let Some(ref lim) = joint.limits {
        tip.push_str(&format!("\nLimits: {lim}"));
    }

    tip.push_str("\n\nClick to select\nRight-click for joint options");
    tip
}

/// Paints the Assembly & Post-Modeling timeline section.
pub fn paint_assembly_section(
    painter: &egui::Painter,
    ui: &mut Ui,
    app: &mut SolveApp,
    section_rect: Rect,
    content_width: f32,
    grid_cols: usize,
    whole_cols: usize,
    _fractional_width: f32,
    component_ids: &[u64],
    visual_joints: &[VisualJoint],
    active_comp_id: u64,
    state: &mut Timeline2DState,
    t: &Tokens,
) {
    let p = painter.with_clip_rect(section_rect);
    p.rect_filled(section_rect, 0.0, t.timeline);

    let scroll_x = state.assembly_scroll_x;
    let scroll_y = state.scroll_y;
    let overscroll_y = state.overscroll_y;
    let origin_x = section_rect.left();
    let num_joints = visual_joints.len();
    if let Some(step) = state.assembly_marker_step {
        state.set_assembly_marker(step, num_joints);
    }
    let marker_step = state.assembly_marker_step.unwrap_or(num_joints);

    let purple_accent = t.icons.agent;
    let grid_line_color = t.border.gamma_multiply(0.2);
    let prereq_color = crate::theme::model_color(app.preferences.timeline_2d_prereq_color);
    let dep_color = crate::theme::model_color(app.preferences.timeline_2d_dependent_color);

    let mut open: Option<Dialog> = None;
    let mut run: Option<(&'static str, Value)> = None;
    // The joints the menu's Suppress or Delete acts on: every selected joint when the right-clicked
    // chip is part of the selection, as for features.
    let mut run_targets: Vec<u64> = Vec::new();

    let content_tracks_top = section_rect.top() + HEADER_TOTAL_HEIGHT - scroll_y + overscroll_y;

    // Grab-and-drag panning gesture on empty assembly track background
    let bg_rect = Rect::from_min_max(pos2(section_rect.left(), section_rect.top() + HEADER_TOTAL_HEIGHT), section_rect.right_bottom());
    let bg_resp = ui.interact(bg_rect, ui.scope_id().with("assembly_bg_pan"), Sense::drag());
    let asm_head_id = ui.scope_id().with("assembly_marker_head_2d");
    let asm_line_id = ui.scope_id().with("assembly_marker_line_2d");
    let is_asm_marker_dragged = ui.ctx().is_being_dragged(asm_head_id) || ui.ctx().is_being_dragged(asm_line_id);
    if bg_resp.dragged() && !is_asm_marker_dragged {
        let delta = bg_resp.drag_delta();
        let a_max_scroll_x = (content_width - section_rect.width()).max(0.0);
        state.assembly_scroll_x = (state.assembly_scroll_x - delta.x).clamp(0.0, a_max_scroll_x);
        let max_scroll_y = ((HEADER_TOTAL_HEIGHT + component_ids.len() as f32 * ROW_HEIGHT) - section_rect.height()).max(0.0);
        state.scroll_y = (state.scroll_y - delta.y).clamp(0.0, max_scroll_y);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        ui.ctx().request_repaint();
    }

    // 1. Draw horizontal track row dividing lines across full content width
    for r in 0..=component_ids.len() {
        let y = content_tracks_top + r as f32 * ROW_HEIGHT;
        if y >= section_rect.top() + HEADER_TOTAL_HEIGHT - 1.0 && y <= section_rect.bottom() + 1.0 {
            p.line_segment([pos2(origin_x - scroll_x, y), pos2(origin_x - scroll_x + content_width, y)], Stroke::new(1.0, grid_line_color));
        }
    }

    // Active component track background highlight
    let active_track_idx = component_ids.iter().position(|&id| id == active_comp_id);
    if let Some(act_idx) = active_track_idx {
        let act_y = content_tracks_top + act_idx as f32 * ROW_HEIGHT;
        let act_rect = Rect::from_min_size(pos2(origin_x - scroll_x, act_y), vec2(content_width, ROW_HEIGHT));
        if act_rect.intersects(section_rect) {
            p.rect_filled(act_rect, 0.0, t.accent.gamma_multiply(0.06));
        }
    }

    // 2. Draw vertical column grid lines
    for c in 0..=grid_cols {
        let x = origin_x + c as f32 * COL_WIDTH - scroll_x;
        if x >= origin_x && x <= section_rect.right() {
            p.line_segment([pos2(x, section_rect.top()), pos2(x, section_rect.bottom())], Stroke::new(1.0, grid_line_color));
        }
    }

    // 3. Draw alignment dots (·) in unoccupied whole cells across the grid (NO DOT on half-column)
    if app.preferences.timeline_2d_cell_dots {
        let opacity = app.preferences.timeline_2d_dot_opacity.clamp(0.05, 1.0);
        let active_dot_color = purple_accent.gamma_multiply(opacity * 0.9);
        let regular_dot_color = t.text_dim.gamma_multiply(opacity * 0.7);

        for c in 0..whole_cols {
            let cx = origin_x + c as f32 * COL_WIDTH + COL_WIDTH / 2.0 - scroll_x;
            if cx + COL_WIDTH / 2.0 < origin_x || cx - COL_WIDTH / 2.0 > section_rect.right() {
                continue;
            }

            let joint_opt = visual_joints.get(c);
            let is_active = c < marker_step;

            for (r_idx, _) in component_ids.iter().enumerate() {
                // If cell is occupied by primary chip or secondary target, skip dot
                if let Some(j) = joint_opt
                    && (j.primary_track_idx == r_idx || j.secondary_track_indices.contains(&r_idx))
                {
                    continue;
                }

                let cy = content_tracks_top + r_idx as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0;
                if cy >= section_rect.top() + HEADER_TOTAL_HEIGHT - 2.0 && cy <= section_rect.bottom() + 2.0 {
                    let dot_color = if Some(r_idx) == active_track_idx { active_dot_color } else { regular_dot_color };
                    let dot_color = if is_active { dot_color } else { dot_color.gamma_multiply(0.5) };
                    p.circle_filled(pos2(cx, cy), 1.75, dot_color);
                }
            }
        }
    }

    // Prerequisite and downstream dependencies and movable corridor for selected joint
    let mut selected_prereqs: HashSet<u64> = HashSet::new();
    let mut selected_downstream: HashSet<u64> = HashSet::new();
    let mut selected_joint_corridor = None;

    if let Some(sel_id) = state.focused_joint()
        && let Some(col) = visual_joints.iter().position(|j| j.id == sel_id)
    {
        let sel_j = &visual_joints[col];
        let tracks = |j: &VisualJoint| {
            let mut t = j.secondary_track_indices.clone();
            t.push(j.primary_track_idx);
            t
        };
        let sel_tracks = tracks(sel_j);
        let shares = |j: &VisualJoint| {
            let other_tracks = tracks(j);
            sel_tracks.iter().any(|t| other_tracks.contains(t))
        };

        let mut min_c = 0;
        for (idx, prev_j) in visual_joints[..col].iter().enumerate() {
            if shares(prev_j) {
                selected_prereqs.insert(prev_j.id);
                min_c = min_c.max(idx + 1);
            }
        }

        let mut max_c = num_joints.saturating_sub(1);
        let mut found_downstream_corridor = false;
        for (idx, next_j) in visual_joints[(col + 1)..].iter().enumerate() {
            let actual_idx = col + 1 + idx;
            if shares(next_j) {
                selected_downstream.insert(next_j.id);
                if !found_downstream_corridor {
                    max_c = max_c.min(actual_idx.saturating_sub(1));
                    found_downstream_corridor = true;
                }
            }
        }

        selected_joint_corridor = if min_c <= max_c { Some((min_c, max_c)) } else { Some((col, col)) };
    }

    // Draw movable corridor background for selected joint (green for active segment)
    if let Some((min_col, max_col)) = selected_joint_corridor {
        let x_start = origin_x + min_col as f32 * COL_WIDTH - scroll_x;
        let x_end = origin_x + (max_col + 1) as f32 * COL_WIDTH - scroll_x;
        let c_top = content_tracks_top;
        let c_bottom = content_tracks_top + component_ids.len() as f32 * ROW_HEIGHT;
        let marker_x = origin_x + marker_step as f32 * COL_WIDTH - scroll_x;

        if max_col < marker_step {
            let rect = Rect::from_min_max(pos2(x_start, c_top), pos2(x_end, c_bottom));
            p.rect_filled(rect, 0.0, t.form_success.gamma_multiply(0.08));
        } else if min_col >= marker_step {
            let rect = Rect::from_min_max(pos2(x_start, c_top), pos2(x_end, c_bottom));
            p.rect_filled(rect, 0.0, t.text_dim.gamma_multiply(0.08));
        } else {
            let active_rect = Rect::from_min_max(pos2(x_start, c_top), pos2(marker_x, c_bottom));
            p.rect_filled(active_rect, 0.0, t.form_success.gamma_multiply(0.08));

            let inactive_rect = Rect::from_min_max(pos2(marker_x, c_top), pos2(x_end, c_bottom));
            p.rect_filled(inactive_rect, 0.0, t.text_dim.gamma_multiply(0.08));
        }
    }

    // 4. Draw multi-track vertical connectors for joints spanning multiple components
    for (col_idx, joint) in visual_joints.iter().enumerate() {
        if joint.secondary_track_indices.is_empty() {
            continue;
        }

        let cx = origin_x + col_idx as f32 * COL_WIDTH + COL_WIDTH / 2.0 - scroll_x;
        if cx + COL_WIDTH < origin_x || cx > section_rect.right() {
            continue;
        }

        let is_active = col_idx < marker_step;
        let is_selected = state.selected_joints.contains(&joint.id);
        let is_prereq = selected_prereqs.contains(&joint.id);
        let is_dependent = selected_downstream.contains(&joint.id);

        let mut all_tracks = joint.secondary_track_indices.clone();
        all_tracks.push(joint.primary_track_idx);
        all_tracks.sort_unstable();

        if let (Some(&min_t), Some(&max_t)) = (all_tracks.first(), all_tracks.last()) {
            let y1 = content_tracks_top + min_t as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0;
            let y2 = content_tracks_top + max_t as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0;
            let line_col = if is_selected {
                if is_active { purple_accent } else { t.text_dim }
            } else if is_prereq {
                if is_active { prereq_color } else { t.text_dim }
            } else if is_dependent {
                if is_active { dep_color } else { t.border }
            } else if is_active {
                t.text_dim
            } else {
                t.border.gamma_multiply(0.7)
            };
            let dot_radius = if is_selected || is_prereq || is_dependent { 1.5 } else { 1.25 };

            let mut y = y1;
            while y <= y2 {
                p.circle_filled(pos2(cx, y), dot_radius, line_col);
                y += 6.0;
            }
        }
    }

    // 5. Draw joint chips across all connected component tracks
    for (col_idx, joint) in visual_joints.iter().enumerate() {
        let col_x = origin_x + col_idx as f32 * COL_WIDTH - scroll_x;
        if col_x + COL_WIDTH < origin_x || col_x > section_rect.right() {
            continue;
        }

        let cx = col_x + COL_WIDTH / 2.0;
        let is_active = col_idx < marker_step;
        let is_selected = state.selected_joints.contains(&joint.id);
        let is_prereq = selected_prereqs.contains(&joint.id);
        let is_dependent = selected_downstream.contains(&joint.id);
        let is_suppressed = joint.suppressed;

        let mut all_tracks = joint.secondary_track_indices.clone();
        all_tracks.push(joint.primary_track_idx);
        all_tracks.sort_unstable();
        all_tracks.dedup();

        for &t_idx in &all_tracks {
            let cy = content_tracks_top + t_idx as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0;
            if cy >= section_rect.top() + HEADER_TOTAL_HEIGHT - ROW_HEIGHT / 2.0 && cy <= section_rect.bottom() + ROW_HEIGHT / 2.0 {
                let chip_rect = Rect::from_center_size(pos2(cx, cy), vec2(CHIP_WIDTH, CHIP_HEIGHT));
                let resp = ui.interact(chip_rect, ui.scope_id().with(("joint_chip", joint.id, t_idx)), Sense::click_and_drag());
                crate::scenario::publish_handle(&format!("assembly_2d:{}", joint.name), chip_rect.center());

                if resp.clicked() {
                    state.joint_anchor = Some(joint.id);
                    let add = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                    // The browser's joint pick is what Delete and the joint panels act on. A
                    // plain click on the joint already picked alone keeps it picked (pick_joint
                    // would take it out).
                    if add || app.tree.picked_joints != [joint.id] {
                        crate::dialogs_assembly::pick_joint(app, joint.id, add);
                    }
                    if add {
                        if state.selected_joints.contains(&joint.id) {
                            state.selected_joints.remove(&joint.id);
                        } else {
                            state.selected_joints.insert(joint.id);
                        }
                    } else {
                        state.selected_joints.clear();
                        state.selected_joints.insert(joint.id);
                        state.selected_features.clear();
                    }
                }

                if resp.double_clicked() {
                    open = crate::dialogs_assembly::edit_joint(app, joint.id);
                }

                if resp.secondary_clicked() && !state.selected_joints.contains(&joint.id) {
                    crate::dialogs_assembly::pick_joint(app, joint.id, false);
                    state.joint_anchor = Some(joint.id);
                    state.selected_joints.clear();
                    state.selected_joints.insert(joint.id);
                    state.selected_features.clear();
                }

                crate::ui_kit::Menu::context(&resp, |ui| {
                    crate::dialogs_assembly::joint_menu(app, ui, (joint.id, joint.kind, joint.suppressed), &mut open, &mut run);
                    if run.is_some() && run_targets.is_empty() {
                        run_targets = if state.selected_joints.contains(&joint.id) {
                            app.session.doc.assembly.joints.iter().map(|j| j.id).filter(|id| state.selected_joints.contains(id)).collect()
                        } else {
                            vec![joint.id]
                        };
                    }
                });

                let stroke_color = if is_selected {
                    if is_active { t.text } else { t.text_dim }
                } else if is_prereq {
                    if is_active { prereq_color } else { t.text_dim }
                } else if is_dependent {
                    if is_active { dep_color } else { t.border }
                } else if is_suppressed || is_active {
                    t.border
                } else {
                    t.border.gamma_multiply(0.5)
                };

                let bg_color = if is_selected {
                    t.panel
                } else if is_suppressed {
                    t.panel.gamma_multiply(0.7)
                } else if is_active {
                    t.timeline_item
                } else {
                    t.panel
                };

                let stroke_width = if is_selected || is_prereq || is_dependent { 1.5 } else { 1.0 };

                p.rect_filled(chip_rect, 4.0, bg_color);
                if is_selected && is_active {
                    p.rect_filled(chip_rect, 4.0, purple_accent.gamma_multiply(0.35));
                }
                if is_suppressed {
                    let corners =
                        [chip_rect.left_top(), chip_rect.right_top(), chip_rect.right_bottom(), chip_rect.left_bottom(), chip_rect.left_top()];
                    p.extend(egui::Shape::dashed_line(&corners, Stroke::new(stroke_width, stroke_color), 4.0, 3.0));
                } else {
                    p.rect_stroke(chip_rect, 4.0, Stroke::new(stroke_width, stroke_color), StrokeKind::Inside);
                }

                let icon_rect = Rect::from_center_size(chip_rect.center(), vec2(20.0, 20.0));
                if is_suppressed {
                    if is_active {
                        crate::icons::paint_icon(&p, icon_rect, "joint", 0.55);
                    } else {
                        crate::icons::paint_greyscale(&p, icon_rect, "joint", None, 0.55);
                    }
                    crate::icons::paint_icon(&p, icon_rect, "suppress", 0.5);
                } else if is_active {
                    crate::icons::paint_icon(&p, icon_rect, "joint", 1.0);
                } else {
                    crate::icons::paint_greyscale(&p, icon_rect, "joint", None, 1.0);
                }

                resp.on_hover_text(format_joint_tooltip(joint, is_active));
            }
        }
    }

    // 6. Draw Header for Assembly (Banner + Tier 1: Step Numbers + Tier 2: Labels)
    let header_rect = Rect::from_min_max(pos2(origin_x, section_rect.top()), pos2(section_rect.right(), section_rect.top() + HEADER_TOTAL_HEIGHT));
    p.rect_filled(header_rect, 0.0, t.timeline);

    // Top Banner
    let banner_rect = Rect::from_min_max(pos2(origin_x, section_rect.top()), pos2(section_rect.right(), section_rect.top() + HEADER_BANNER_HEIGHT));
    p.rect_filled(banner_rect, 0.0, t.panel);
    p.line_segment([banner_rect.left_bottom(), banner_rect.right_bottom()], Stroke::new(1.0, t.border.gamma_multiply(0.4)));

    // Circle icon + Title + Count Badge
    p.text(pos2(banner_rect.left() + 10.0, banner_rect.center().y), Align2::LEFT_CENTER, "●", FontId::proportional(t.font_small), purple_accent);
    p.text(
        pos2(banner_rect.left() + 24.0, banner_rect.center().y),
        Align2::LEFT_CENTER,
        "ASSEMBLY & JOINTS",
        FontId::proportional(t.font_small),
        t.text,
    );
    let count_text = format!("{} Joints", num_joints);
    let badge_rect = Rect::from_center_size(pos2(banner_rect.left() + 175.0, banner_rect.center().y), vec2(56.0, 16.0));
    p.rect_filled(badge_rect, 3.0, purple_accent.gamma_multiply(0.2));
    p.rect_stroke(badge_rect, 3.0, Stroke::new(1.0, purple_accent.gamma_multiply(0.4)), StrokeKind::Inside);
    p.text(badge_rect.center(), Align2::CENTER_CENTER, count_text, FontId::monospace(t.font_badge), purple_accent);

    // Right side: Active summary and quick transport buttons
    if banner_rect.width() > 320.0 {
        let active_joints = app.session.doc.assembly.joints.iter().take(marker_step.min(num_joints)).filter(|j| !j.suppressed).count();
        let active_summary = format!("Active: {} / {}", active_joints, num_joints);
        p.text(
            pos2(banner_rect.right() - 85.0, banner_rect.center().y),
            Align2::RIGHT_CENTER,
            active_summary,
            FontId::monospace(t.font_small),
            purple_accent,
        );

        let btn_b = Rect::from_center_size(pos2(banner_rect.right() - 65.0, banner_rect.center().y), vec2(16.0, 16.0));
        let btn_f = Rect::from_center_size(pos2(banner_rect.right() - 44.0, banner_rect.center().y), vec2(16.0, 16.0));
        let btn_e = Rect::from_center_size(pos2(banner_rect.right() - 22.0, banner_rect.center().y), vec2(18.0, 16.0));

        let resp_b = ui.interact(btn_b, ui.scope_id().with("j_step_back"), Sense::click());
        let resp_f = ui.interact(btn_f, ui.scope_id().with("j_step_forward"), Sense::click());
        let resp_e = ui.interact(btn_e, ui.scope_id().with("j_step_end"), Sense::click());

        let ink_b = if resp_b.hovered() { t.text } else { t.text_dim };
        let ink_f = if resp_f.hovered() { t.text } else { t.text_dim };
        let ink_e = if resp_e.hovered() { t.text } else { t.text_dim };
        crate::icons::glyphs::transport_glyph(&p, btn_b.center(), "back", ink_b);
        crate::icons::glyphs::transport_glyph(&p, btn_f.center(), "forward", ink_f);
        crate::icons::glyphs::transport_glyph(&p, btn_e.center(), "last", ink_e);

        if resp_b.clicked() && marker_step > 0 {
            state.set_assembly_marker(marker_step - 1, num_joints);
            ui.ctx().request_repaint();
        }
        if resp_f.clicked() && marker_step < num_joints {
            state.set_assembly_marker(marker_step + 1, num_joints);
            ui.ctx().request_repaint();
        }
        if resp_e.clicked() {
            state.set_assembly_marker(num_joints, num_joints);
            ui.ctx().request_repaint();
        }
    }

    // Separator line between Tier 1 and Tier 2
    let t1_top = section_rect.top() + HEADER_BANNER_HEIGHT;
    let t2_top = t1_top + HEADER_T1_HEIGHT;
    p.line_segment([pos2(origin_x, t2_top), pos2(section_rect.right(), t2_top)], Stroke::new(1.0, t.border.gamma_multiply(0.4)));
    p.line_segment(
        [pos2(origin_x, section_rect.top() + HEADER_TOTAL_HEIGHT), pos2(section_rect.right(), section_rect.top() + HEADER_TOTAL_HEIGHT)],
        Stroke::new(1.0, t.border),
    );

    let mono_font = FontId::monospace(t.font_small);
    let label_font = FontId::proportional(t.font_micro);

    for c in 0..grid_cols {
        let col_x = origin_x + c as f32 * COL_WIDTH - scroll_x;
        if col_x + COL_WIDTH < origin_x || col_x > section_rect.right() {
            continue;
        }

        let is_step_active = c < marker_step;

        // Tier 1: Step Number cell (J01, J02, ...)
        let t1_rect = Rect::from_min_size(pos2(col_x, t1_top), vec2(COL_WIDTH, HEADER_T1_HEIGHT));
        let tick_color = if is_step_active { purple_accent } else { t.border.gamma_multiply(0.4) };
        p.line_segment([pos2(col_x + COL_WIDTH / 2.0, t1_top + 2.0), pos2(col_x + COL_WIDTH / 2.0, t1_top + 6.0)], Stroke::new(1.0, tick_color));

        if c < num_joints {
            let label = format!("J{:02}", c + 1);
            let text_col = if is_step_active { purple_accent } else { t.text_dim.gamma_multiply(0.6) };
            p.text(pos2(col_x + COL_WIDTH / 2.0, t1_top + HEADER_T1_HEIGHT / 2.0 + 2.0), Align2::CENTER_CENTER, label, mono_font.clone(), text_col);

            let resp = ui.interact(t1_rect, ui.scope_id().with(("assembly_step_click", c)), Sense::click());
            if resp.clicked() {
                state.set_assembly_marker(c + 1, num_joints);
                ui.ctx().request_repaint();
            }
            resp.on_hover_text(format!("Roll assembly history to Step J{:02}", c + 1));
        }

        // Tier 2: Joint type name text
        if let Some(joint) = visual_joints.get(c) {
            let kind_name = format!("{:?}", joint.kind);
            let trunc_name = if kind_name.len() > 6 { format!("{}…", &kind_name[..5]) } else { kind_name };
            p.text(
                pos2(col_x + COL_WIDTH / 2.0, t2_top + HEADER_T2_HEIGHT / 2.0),
                Align2::CENTER_CENTER,
                trunc_name,
                label_font.clone(),
                if is_step_active { t.text_dim } else { t.text_dim.gamma_multiply(0.5) },
            );
        }
    }

    // 7. Draw Assembly Rollback Marker
    let (line_x, pin_x) = if marker_step == 0 {
        (origin_x + 4.0 - scroll_x, origin_x + 4.0 + 7.0 - scroll_x)
    } else {
        let normal_x = origin_x + marker_step.min(num_joints) as f32 * COL_WIDTH - scroll_x;
        (normal_x, normal_x)
    };

    if pin_x >= origin_x - 20.0 && pin_x <= section_rect.right() + 20.0 {
        let pin_center = pos2(pin_x, t1_top + HEADER_T1_HEIGHT / 2.0);
        let line_top = t2_top;
        let tracks_top = section_rect.top() + HEADER_TOTAL_HEIGHT;
        let a_max_scroll_x = (content_width - section_rect.width()).max(0.0);
        let scrollbar_pad = if a_max_scroll_x > 0.0 { 8.0 } else { 2.0 };
        let line_bottom = (section_rect.bottom() - scrollbar_pad).max(tracks_top + 4.0);

        let head_hit_rect = Rect::from_min_max(pos2(pin_x - 8.0, t1_top), pos2(pin_x + 8.0, tracks_top)).intersect(section_rect);
        let line_half_left = 6.0;
        let line_half_right = if marker_step == 0 { 2.5 } else { 6.0 };
        let line_hit_rect =
            Rect::from_min_max(pos2(line_x - line_half_left, tracks_top), pos2(line_x + line_half_right, line_bottom)).intersect(bg_rect);

        let head_resp = ui.interact(head_hit_rect, ui.scope_id().with("assembly_marker_head_2d"), Sense::click_and_drag());
        let line_resp = ui.interact(line_hit_rect, ui.scope_id().with("assembly_marker_line_2d"), Sense::click_and_drag());

        crate::scenario::publish_handle("assembly_2d:marker_head", pin_center);
        if line_hit_rect.is_positive() {
            crate::scenario::publish_handle("assembly_2d:marker_line", line_hit_rect.center());
        }

        let marker_hovered = head_resp.hovered() || line_resp.hovered();
        let marker_dragged = head_resp.dragged() || line_resp.dragged();

        if marker_dragged {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if marker_hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        if marker_dragged && let Some(pos) = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos())) {
            let target_col = ((pos.x - origin_x + scroll_x) / COL_WIDTH).round().max(0.0) as usize;
            let target_col = target_col.min(num_joints);
            state.set_assembly_marker(target_col, num_joints);
            ui.ctx().request_repaint();
        }

        let line_width = if marker_hovered || marker_dragged { 3.0 } else { 2.5 };

        // Vertical marker line through tracks - drawn using clipped painter p so it slides under the splitter bar when scrolled
        p.line_segment([pos2(line_x, line_top), pos2(line_x, line_bottom)], Stroke::new(line_width, purple_accent));

        // Bottom handle cap on vertical line
        let bottom_cap_rect = Rect::from_min_size(pos2(line_x - 5.0, line_bottom - 4.0), vec2(10.0, 4.0));
        p.rect_filled(bottom_cap_rect, egui::CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, purple_accent);

        // Square pin handle in Tier 1 contacting vertical line
        let pin_size = if marker_hovered || marker_dragged { vec2(15.0, 15.0) } else { vec2(14.0, 14.0) };
        let square_rect = Rect::from_center_size(pin_center, pin_size);
        let (pin_bg, inner_col) = if marker_hovered || marker_dragged { (purple_accent, t.timeline) } else { (t.timeline, purple_accent) };
        p.rect_filled(square_rect, 2.5, pin_bg);
        p.rect_stroke(square_rect, 2.5, Stroke::new(2.0, purple_accent), StrokeKind::Inside);
        let inner_rect = Rect::from_center_size(pin_center, vec2(4.0, 4.0));
        p.rect_filled(inner_rect, 1.0, inner_col);
    }

    // 7b. Draw movable corridor brackets for selected joint on top of tracks and marker line
    if let Some((min_col, max_col)) = selected_joint_corridor {
        let x_start = origin_x + min_col as f32 * COL_WIDTH - scroll_x;
        let x_end = origin_x + (max_col + 1) as f32 * COL_WIDTH - scroll_x;
        let marker_x = origin_x + marker_step as f32 * COL_WIDTH - scroll_x;

        let b_top = (content_tracks_top + 4.0).max(section_rect.top() + HEADER_TOTAL_HEIGHT + 4.0);
        let b_bottom = (content_tracks_top + component_ids.len() as f32 * ROW_HEIGHT - 4.0).min(section_rect.bottom() - 4.0);

        if b_bottom > b_top {
            let open_color = if min_col < marker_step { t.form_success } else { t.text_dim.gamma_multiply(0.6) };
            let close_color = if max_col < marker_step { t.form_success } else { t.text_dim.gamma_multiply(0.6) };

            super::canvas::paint_bracket_open(&p, x_start, origin_x, b_top, b_bottom, open_color, Some(marker_x));
            super::canvas::paint_bracket_close(&p, x_end, b_top, b_bottom, close_color, Some(marker_x));
        }
    }

    // 8. Horizontal scrollbar for Assembly track when content overflows
    let a_max_scroll_x = (content_width - section_rect.width()).max(0.0);
    if a_max_scroll_x > 0.0 {
        let bar_h = 6.0;
        let bar_y = section_rect.bottom() - bar_h - 1.0;
        let slot_rect = Rect::from_min_size(pos2(section_rect.left(), bar_y), vec2(section_rect.width(), bar_h));
        p.rect_filled(slot_rect, 0.0, t.timeline.gamma_multiply(0.4));

        let ratio = (section_rect.width() / content_width).clamp(0.05, 1.0);
        let pill_w = (section_rect.width() * ratio).max(24.0);
        let scroll_frac = (scroll_x / a_max_scroll_x).clamp(0.0, 1.0);
        let pill_x = section_rect.left() + scroll_frac * (section_rect.width() - pill_w);
        let pill_rect = Rect::from_min_size(pos2(pill_x, bar_y), vec2(pill_w, bar_h));

        let pill_resp = ui.interact(pill_rect, ui.scope_id().with("assembly_h_scrollbar"), Sense::drag());
        if pill_resp.dragged() {
            let dx = pill_resp.drag_delta().x;
            let available_travel = (section_rect.width() - pill_w).max(1.0);
            state.assembly_scroll_x = (state.assembly_scroll_x + dx * (a_max_scroll_x / available_travel)).clamp(0.0, a_max_scroll_x);
            ui.ctx().request_repaint();
        }

        let pill_color = if pill_resp.hovered() || pill_resp.dragged() { purple_accent } else { t.border.gamma_multiply(0.6) };
        p.rect_filled(pill_rect, 3.0, pill_color);
    }

    // Dispatch context menu actions
    if let Some((command, params)) = run {
        let suppressed = params.get("suppressed").and_then(Value::as_bool);
        match (command, suppressed) {
            ("selection.delete", _) if run_targets.len() > 1 => crate::delete::request(app, serde_json::json!({ "joints": run_targets })),
            ("selection.delete", _) => crate::delete::request(app, params),
            // No command suppresses a list of joints, so each selected joint takes its own edit.
            ("joint.edit", Some(want)) if run_targets.len() > 1 => {
                for id in run_targets {
                    let _ = app.run("joint.edit", serde_json::json!({ "joint": id, "suppressed": want }));
                }
            }
            _ => {
                let _ = app.run(command, params);
            }
        }
    }
    if let Some(dialog) = open {
        app.tool = None;
        app.dialog = Some(dialog);
    }
}

/// Paints the vertical splitter bar between Modeling and Assembly sections.
pub fn paint_splitter(
    painter: &egui::Painter,
    ui: &mut Ui,
    splitter_rect: Rect,
    available_track_space: f32,
    grid_origin_x: f32,
    is_modeling_collapsed: bool,
    is_assembly_collapsed: bool,
    state: &mut Timeline2DState,
    t: &Tokens,
) {
    let p = painter.with_clip_rect(splitter_rect);
    let bg_color = t.panel;
    p.rect_filled(splitter_rect, 0.0, bg_color);
    p.line_segment([splitter_rect.left_top(), splitter_rect.left_bottom()], Stroke::new(1.0, t.border));
    p.line_segment([splitter_rect.right_top(), splitter_rect.right_bottom()], Stroke::new(1.0, t.border));

    // Center drag grip and buttons with dynamic sizing and comfortable spacing
    let grip_center_y = splitter_rect.center().y;
    let one_row_h = HEADER_TOTAL_HEIGHT + ROW_HEIGHT;
    let max_h = HEADER_TOTAL_HEIGHT + 5.0 * ROW_HEIGHT;
    let t_factor = ((splitter_rect.height() - one_row_h) / (max_h - one_row_h)).clamp(0.0, 1.0);
    let base_grip_len = 24.0;
    let grip_h = base_grip_len * (1.0 + 0.50 * t_factor);

    let btn_size = vec2(SPLITTER_WIDTH - 2.0, 20.0);
    let gap = 7.0;
    let btn_upper_center_y = grip_center_y - grip_h / 2.0 - gap - btn_size.y / 2.0;
    let btn_lower_center_y = grip_center_y + grip_h / 2.0 + gap + btn_size.y / 2.0;
    let btn_upper_rect = Rect::from_center_size(pos2(splitter_rect.center().x, btn_upper_center_y), btn_size);
    let btn_lower_rect = Rect::from_center_size(pos2(splitter_rect.center().x, btn_lower_center_y), btn_size);
    let grip_rect = Rect::from_center_size(pos2(splitter_rect.center().x, grip_center_y), vec2(4.0, grip_h));
    crate::scenario::publish_handle("timeline_2d:splitter_upper", btn_upper_rect.center());
    crate::scenario::publish_handle("timeline_2d:splitter_lower", btn_lower_rect.center());
    crate::scenario::publish_handle("timeline_2d:splitter_grip", grip_rect.center());

    // Upper button: Collapse / Reopen Modeling (Left)
    let resp_upper = ui.interact(btn_upper_rect, ui.scope_id().with("splitter_btn_upper"), Sense::click());
    let accent_color = t.accent;
    let bg_upper = if resp_upper.is_pointer_button_down_on() {
        accent_color.gamma_multiply(0.6)
    } else if resp_upper.hovered() {
        accent_color.gamma_multiply(0.25)
    } else {
        t.timeline
    };
    p.rect_filled(btn_upper_rect, 3.0, bg_upper);
    p.rect_stroke(btn_upper_rect, 3.0, Stroke::new(1.0, accent_color), StrokeKind::Inside);
    let icon_color_upper = if resp_upper.is_pointer_button_down_on() || resp_upper.hovered() { t.text } else { t.text_dim };
    let s_upper = Stroke::new(1.4, icon_color_upper);
    let c_up = btn_upper_rect.center();
    if is_modeling_collapsed {
        // Dual-pane reopen icon: a rounded rectangle with a vertical center split line
        let icon_rect = Rect::from_center_size(c_up, vec2(8.0, 9.0));
        p.rect_stroke(icon_rect, 1.0, Stroke::new(1.2, icon_color_upper), StrokeKind::Inside);
        p.line_segment([pos2(c_up.x, icon_rect.top() + 0.5), pos2(c_up.x, icon_rect.bottom() - 0.5)], Stroke::new(1.0, icon_color_upper));
    } else {
        // Contraction icon: |< (vertical bar on left + left chevron)
        p.line_segment([pos2(c_up.x - 3.5, c_up.y - 4.0), pos2(c_up.x - 3.5, c_up.y + 4.0)], s_upper);
        p.line_segment([pos2(c_up.x + 2.5, c_up.y - 4.0), pos2(c_up.x - 1.0, c_up.y)], s_upper);
        p.line_segment([pos2(c_up.x - 1.0, c_up.y), pos2(c_up.x + 2.5, c_up.y + 4.0)], s_upper);
    }
    let tip_upper =
        if is_modeling_collapsed { "Reopen Modeling section (Expand to Split View)" } else { "Contract Modeling section into Left Dock Tab" };
    let resp_upper = resp_upper.on_hover_text(tip_upper);

    if resp_upper.clicked() {
        if is_modeling_collapsed {
            state.is_modeling_collapsed = false;
            state.is_assembly_collapsed = false;
            state.modeling_split_ratio = 0.65;
        } else {
            state.is_modeling_collapsed = true;
            state.is_assembly_collapsed = false;
        }
        ui.ctx().request_repaint();
    }

    // Lower button: Collapse / Reopen Assembly (Right)
    let resp_lower = ui.interact(btn_lower_rect, ui.scope_id().with("splitter_btn_lower"), Sense::click());
    let purple_accent = t.icons.agent;
    let bg_lower = if resp_lower.is_pointer_button_down_on() {
        purple_accent.gamma_multiply(0.6)
    } else if resp_lower.hovered() {
        purple_accent.gamma_multiply(0.25)
    } else {
        t.timeline
    };
    p.rect_filled(btn_lower_rect, 3.0, bg_lower);
    p.rect_stroke(btn_lower_rect, 3.0, Stroke::new(1.0, purple_accent), StrokeKind::Inside);
    let icon_color_lower = if resp_lower.is_pointer_button_down_on() || resp_lower.hovered() { t.text } else { t.text_dim };
    let s_lower = Stroke::new(1.4, icon_color_lower);
    let c_dn = btn_lower_rect.center();
    if is_assembly_collapsed {
        // Dual-pane reopen icon: a rounded rectangle with a vertical center split line
        let icon_rect = Rect::from_center_size(c_dn, vec2(8.0, 9.0));
        p.rect_stroke(icon_rect, 1.0, Stroke::new(1.2, icon_color_lower), StrokeKind::Inside);
        p.line_segment([pos2(c_dn.x, icon_rect.top() + 0.5), pos2(c_dn.x, icon_rect.bottom() - 0.5)], Stroke::new(1.0, icon_color_lower));
    } else {
        // Contraction icon: >| (right chevron + vertical bar on right)
        p.line_segment([pos2(c_dn.x - 2.5, c_dn.y - 4.0), pos2(c_dn.x + 1.0, c_dn.y)], s_lower);
        p.line_segment([pos2(c_dn.x + 1.0, c_dn.y), pos2(c_dn.x - 2.5, c_dn.y + 4.0)], s_lower);
        p.line_segment([pos2(c_dn.x + 3.5, c_dn.y - 4.0), pos2(c_dn.x + 3.5, c_dn.y + 4.0)], s_lower);
    }
    let tip_lower =
        if is_assembly_collapsed { "Reopen Assembly section (Expand to Split View)" } else { "Contract Assembly section into Right Dock Tab" };
    let resp_lower = resp_lower.on_hover_text(tip_lower);

    if resp_lower.clicked() {
        if is_assembly_collapsed {
            state.is_modeling_collapsed = false;
            state.is_assembly_collapsed = false;
            state.modeling_split_ratio = 0.65;
        } else {
            state.is_assembly_collapsed = true;
            state.is_modeling_collapsed = false;
        }
        ui.ctx().request_repaint();
    }

    // Set pointing hand cursor on hover for buttons
    if resp_upper.hovered() || resp_lower.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Non-overlapping drag zones for the splitter bar
    let top_drag_rect = Rect::from_min_max(splitter_rect.left_top(), pos2(splitter_rect.right(), btn_upper_rect.top()));
    let mid_drag_rect = Rect::from_min_max(pos2(splitter_rect.left(), btn_upper_rect.bottom()), pos2(splitter_rect.right(), btn_lower_rect.top()));
    let bot_drag_rect = Rect::from_min_max(pos2(splitter_rect.left(), btn_lower_rect.bottom()), splitter_rect.right_bottom());

    let resp_drag_top = ui.interact(top_drag_rect, ui.scope_id().with("splitter_drag_top"), Sense::drag());
    let resp_drag_mid = ui.interact(mid_drag_rect, ui.scope_id().with("splitter_drag_mid"), Sense::drag());
    let resp_drag_bot = ui.interact(bot_drag_rect, ui.scope_id().with("splitter_drag_bot"), Sense::drag());

    let is_drag_hovered = resp_drag_top.hovered() || resp_drag_mid.hovered() || resp_drag_bot.hovered();
    let is_dragged = resp_drag_top.dragged() || resp_drag_mid.dragged() || resp_drag_bot.dragged();

    // Draw center drag grip pill with highlight when hovered or dragged
    let grip_color = if resp_drag_mid.hovered() || is_dragged { t.accent } else { t.border.gamma_multiply(1.8) };
    p.rect_filled(grip_rect, 2.0, grip_color);

    if (is_drag_hovered || is_dragged) && !resp_upper.hovered() && !resp_lower.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    if is_dragged && let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
        let split_content_w = (available_track_space - SPLITTER_WIDTH).max(100.0);
        let mouse_offset = pos.x - grid_origin_x;

        if is_modeling_collapsed {
            if mouse_offset >= EXPAND_THRESHOLD {
                state.is_modeling_collapsed = false;
                state.modeling_split_ratio = 0.5;
                ui.ctx().request_repaint();
            }
        } else if is_assembly_collapsed {
            let dist_from_right = available_track_space - mouse_offset;
            if dist_from_right >= EXPAND_THRESHOLD {
                state.is_assembly_collapsed = false;
                state.modeling_split_ratio = 0.5;
                ui.ctx().request_repaint();
            }
        } else {
            // Normal split view dragging
            if mouse_offset < SNAP_THRESHOLD {
                state.is_modeling_collapsed = true;
                ui.ctx().request_repaint();
            } else if available_track_space - mouse_offset < SNAP_THRESHOLD {
                state.is_assembly_collapsed = true;
                ui.ctx().request_repaint();
            } else {
                state.modeling_split_ratio = (mouse_offset / split_content_w).clamp(0.05, 0.95);
                ui.ctx().request_repaint();
            }
        }
    }
}

/// Paints the collapsed dock strip for contracted Modeling or Assembly tracks.
pub fn paint_collapsed_strip(
    painter: &egui::Painter,
    ui: &mut Ui,
    strip_rect: Rect,
    is_modeling: bool,
    item_count: usize,
    state: &mut Timeline2DState,
    t: &Tokens,
) {
    let p = painter.with_clip_rect(strip_rect);
    let accent_col = if is_modeling { t.accent } else { t.icons.agent };

    let resp = ui.interact(strip_rect, ui.scope_id().with(("collapsed_strip", is_modeling)), Sense::click());
    let bg_color = if resp.hovered() { t.hover } else { t.panel };
    p.rect_filled(strip_rect, 0.0, bg_color);

    let border_x = if is_modeling { strip_rect.right() } else { strip_rect.left() };
    p.line_segment([pos2(border_x, strip_rect.top()), pos2(border_x, strip_rect.bottom())], Stroke::new(1.0, t.border));

    // Accent edge indicator line
    let edge_x = if is_modeling { strip_rect.left() + 1.0 } else { strip_rect.right() - 1.0 };
    p.line_segment([pos2(edge_x, strip_rect.top() + 4.0), pos2(edge_x, strip_rect.bottom() - 4.0)], Stroke::new(2.5, accent_col));

    // Item count badge at top
    let badge_rect = Rect::from_center_size(pos2(strip_rect.center().x, strip_rect.top() + 20.0), vec2(22.0, 16.0));
    p.rect_filled(badge_rect, 3.0, accent_col.gamma_multiply(0.25));
    p.rect_stroke(badge_rect, 3.0, Stroke::new(1.0, accent_col.gamma_multiply(0.5)), StrokeKind::Inside);
    p.text(badge_rect.center(), Align2::CENTER_CENTER, item_count.to_string(), FontId::monospace(t.font_badge), accent_col);

    // Expressive rotated vertical text in the center
    let title_text = if is_modeling { "Modeling Features" } else { "Assembly & Joints" };
    let font = FontId::proportional(t.font_small);
    let text_col = if resp.hovered() { t.text } else { t.text_dim };
    let galley = p.layout_no_wrap(title_text.to_string(), font, text_col);
    let title_w = galley.size().x;

    let center_y = strip_rect.center().y;
    let text_start_y = center_y - title_w / 2.0;

    let mut shape = egui::epaint::TextShape::new(pos2(strip_rect.center().x + 4.0, text_start_y), galley, text_col);
    shape.angle = std::f32::consts::FRAC_PI_2;
    p.add(shape);

    // Dot symbol at bottom of rotated text
    let dot_y = text_start_y + title_w + 14.0;
    if dot_y < strip_rect.bottom() - 10.0 {
        p.circle_filled(pos2(strip_rect.center().x, dot_y), 3.0, accent_col);
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        state.is_modeling_collapsed = false;
        state.is_assembly_collapsed = false;
        state.modeling_split_ratio = if is_modeling { 0.65 } else { 0.5 };
        ui.ctx().request_repaint();
    }
    resp.on_hover_text(if is_modeling {
        "Modeling Track contracted. Click to restore split view."
    } else {
        "Assembly Track contracted. Click to restore split view."
    });
}
