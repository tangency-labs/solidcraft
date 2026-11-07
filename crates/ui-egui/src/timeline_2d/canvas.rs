//! 2D Grid Canvas, Two-Tier Header, Pinned Component Tracks, and Marker Scrubber.

use super::deps::DependencyGraph;
use super::state::Timeline2DState;
use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit::{HistoryTransport, Menu};
use egui::{FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use solidcraft_engine::doc::{Document, Feature, FeatureKind, Model};
use solidcraft_engine::geom::Vec3Geom;
use std::collections::{BTreeMap, BTreeSet};

pub const HEADER_BANNER_HEIGHT: f32 = 26.0;
pub const HEADER_T1_HEIGHT: f32 = 22.0;
pub const HEADER_T2_HEIGHT: f32 = 20.0;
pub const HEADER_TOTAL_HEIGHT: f32 = HEADER_BANNER_HEIGHT + HEADER_T1_HEIGHT + HEADER_T2_HEIGHT;
pub const ROW_HEIGHT: f32 = 38.0;
pub const COL_WIDTH: f32 = 52.0;
pub const PINNED_LEFT_WIDTH: f32 = 160.0;
pub const CHIP_WIDTH: f32 = 38.0;
pub const CHIP_HEIGHT: f32 = 32.0;
pub const SCROLLBAR_RESERVE_HEIGHT: f32 = 10.0;
pub const MIN_DOCK_HEIGHT: f32 = HEADER_TOTAL_HEIGHT + ROW_HEIGHT + SCROLLBAR_RESERVE_HEIGHT;
pub const DOCK_TOTAL_HEIGHT: f32 = HEADER_TOTAL_HEIGHT + 4.0 * ROW_HEIGHT + SCROLLBAR_RESERVE_HEIGHT;
pub const VIRTUAL_GROUP_HEIGHT: f32 = DOCK_TOTAL_HEIGHT;

/// Discovers component IDs and names for the 2D timeline tracks.
pub fn discover_component_tracks(app: &SolveApp) -> (Vec<u64>, BTreeMap<u64, String>) {
    let mut component_ids: Vec<u64> = Vec::new();
    let mut component_names: BTreeMap<u64, String> = BTreeMap::new();

    // Root component 0 always first
    component_ids.push(0);
    let root_name = if app.session.doc.name.is_empty() { "Root Component".to_string() } else { app.session.doc.name.clone() };
    component_names.insert(0, root_name);

    for comp in &app.session.doc.components {
        if comp.id != 0 && !component_ids.contains(&comp.id) {
            component_ids.push(comp.id);
            component_names.insert(comp.id, comp.name.clone());
        }
    }

    let model = Some(&app.session.model);
    for f in &app.session.doc.features {
        let affected = feature_affected_components(&app.session.doc, model, f);
        for &cid in &affected {
            if !component_ids.contains(&cid) {
                let comp_opt = app.session.doc.components.iter().find(|c| c.id == cid);
                let name = comp_opt.map(|c| c.name.clone()).unwrap_or_else(|| format!("Component {cid}"));
                component_ids.push(cid);
                component_names.insert(cid, name);
            }
        }
    }

    (component_ids, component_names)
}

/// Computes the default dock height that fits all components in the active document
/// with clearance spacing for the horizontal scrollbar so no component is covered.
pub fn default_dock_height(app: &SolveApp) -> f32 {
    let (component_ids, _) = discover_component_tracks(app);
    let num_tracks = component_ids.len().max(1);
    HEADER_TOTAL_HEIGHT + (num_tracks as f32) * ROW_HEIGHT + SCROLLBAR_RESERVE_HEIGHT
}

/// Represents a visual column on the timeline grid.
#[derive(Clone, Debug)]
pub enum VisualColumn {
    /// A single feature column (or multiple features in multi-component step).
    Features { step_number: usize, features: Vec<u64>, open_group: Option<u64> },
    /// A folded group column collapsing member features into a single virtual chip.
    FoldedGroup { group_id: u64, features: Vec<u64> },
}

impl VisualColumn {
    /// Returns true if this visual column represents a folded group.
    pub fn is_folded_group(&self) -> bool {
        matches!(self, VisualColumn::FoldedGroup { .. })
    }

    /// Returns the slice of feature IDs contained in this visual column.
    pub fn feature_ids(&self) -> &[u64] {
        match self {
            VisualColumn::Features { features, .. } => features,
            VisualColumn::FoldedGroup { features, .. } => features,
        }
    }
}

/// Renders the 2D Timeline dock contents.
pub fn paint_timeline_2d(app: &mut SolveApp, ui: &mut Ui, floating: bool) {
    let t = Tokens::get();
    let accent = crate::theme::TabAccent::for_tab(&t, &app.ui.tab);
    let total_rect = ui.max_rect();
    let painter = ui.painter().clone();

    // Load or initialize state
    let mut state = Timeline2DState::load_scoped(ui.ctx(), Some(app.session.uid));

    // The session selection is the source of truth, as in the 1D timeline: Delete, the 3D view
    // and agents act on it. A change made below goes back to it at the end of the frame.
    state.selected_features = crate::timeline::selected_feature_ids(app).into_iter().collect();
    // Joints the same way: the browser's joint pick, which Delete and the joint panels read.
    state.selected_joints = app.tree.picked_joints.iter().copied().collect();
    let selection_before = state.selected_features.clone();
    let mut keep_other_selection = false;
    let mut clicked_chip: Option<(u64, egui::Modifiers)> = None;
    if app.deferred.is_none() {
        state.separately_suppressed_features.retain(|id| app.session.doc.feature(*id).is_some_and(|f| f.suppressed));
    }
    if app.session.doc.features.is_empty() {
        state.selected_features.clear();
        state.folded_groups.clear();
        state.separately_suppressed_features.clear();
    }

    // 1. Build dependency graph
    let dep_graph = DependencyGraph::from_doc(&app.session.doc);
    let total_features = app.session.doc.features.len();
    let marker_slot = app.session.doc.marker.unwrap_or(total_features);

    // 2. Discover component tracks
    let (component_ids, component_names) = discover_component_tracks(app);

    let mut feature_components_map: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    {
        let model = Some(&app.session.model);
        for f in &app.session.doc.features {
            let affected = feature_affected_components(&app.session.doc, model, f);
            feature_components_map.insert(f.id, affected);
        }
    }

    // 3. Build visual columns (respecting folded groups)
    let visual_columns = build_visual_columns(&app.session.doc, &state.folded_groups);
    let num_columns = visual_columns.len();

    // Map marker feature slot to its visual column boundary
    let marker_visual_col = if let Some(marker_idx) = app.session.doc.marker {
        if marker_idx < total_features {
            let f = &app.session.doc.features[marker_idx];
            feature_to_visual_col(&visual_columns, f.id).unwrap_or(num_columns)
        } else {
            num_columns
        }
    } else {
        num_columns
    };

    // 3b. Discover assembly joints for post-modeling track view
    let visual_joints = super::assembly::build_visual_joints(&app.session.doc, &component_ids, &component_names);
    let has_assembly_section = true;
    let num_joints = visual_joints.len();

    // 4. Compute layout dimensions and split sections
    let grid_origin_x = total_rect.left() + PINNED_LEFT_WIDTH;
    let available_track_space = (total_rect.width() - PINNED_LEFT_WIDTH).max(0.0);

    let (modeling_rect, splitter_rect, assembly_rect, strip_modeling_rect, strip_assembly_rect, full_track_space) = if has_assembly_section {
        let full_track_space = (available_track_space - super::assembly::DOCK_STRIP_WIDTH - super::assembly::SPLITTER_WIDTH).max(400.0);
        let split_content_w = (available_track_space - super::assembly::SPLITTER_WIDTH).max(100.0);

        if state.is_modeling_collapsed {
            let s_m = Rect::from_min_size(pos2(grid_origin_x, total_rect.top()), vec2(super::assembly::DOCK_STRIP_WIDTH, total_rect.height()));
            let sp = Rect::from_min_size(
                pos2(grid_origin_x + super::assembly::DOCK_STRIP_WIDTH, total_rect.top()),
                vec2(super::assembly::SPLITTER_WIDTH, total_rect.height()),
            );
            let asm = Rect::from_min_max(
                pos2(grid_origin_x + super::assembly::DOCK_STRIP_WIDTH + super::assembly::SPLITTER_WIDTH, total_rect.top()),
                total_rect.right_bottom(),
            );
            (None, Some(sp), Some(asm), Some(s_m), None, full_track_space)
        } else if state.is_assembly_collapsed {
            let s_a = Rect::from_min_size(
                pos2(total_rect.right() - super::assembly::DOCK_STRIP_WIDTH, total_rect.top()),
                vec2(super::assembly::DOCK_STRIP_WIDTH, total_rect.height()),
            );
            let sp = Rect::from_min_size(
                pos2(total_rect.right() - super::assembly::DOCK_STRIP_WIDTH - super::assembly::SPLITTER_WIDTH, total_rect.top()),
                vec2(super::assembly::SPLITTER_WIDTH, total_rect.height()),
            );
            let m = Rect::from_min_max(
                pos2(grid_origin_x, total_rect.top()),
                pos2(total_rect.right() - super::assembly::DOCK_STRIP_WIDTH - super::assembly::SPLITTER_WIDTH, total_rect.bottom()),
            );
            (Some(m), Some(sp), None, None, Some(s_a), full_track_space)
        } else {
            let target_m_w = (split_content_w * state.modeling_split_ratio)
                .clamp(super::assembly::SNAP_THRESHOLD, split_content_w - super::assembly::SNAP_THRESHOLD);
            let m = Rect::from_min_size(pos2(grid_origin_x, total_rect.top()), vec2(target_m_w, total_rect.height()));
            let sp =
                Rect::from_min_size(pos2(grid_origin_x + target_m_w, total_rect.top()), vec2(super::assembly::SPLITTER_WIDTH, total_rect.height()));
            let asm =
                Rect::from_min_max(pos2(grid_origin_x + target_m_w + super::assembly::SPLITTER_WIDTH, total_rect.top()), total_rect.right_bottom());
            (Some(m), Some(sp), Some(asm), None, None, full_track_space)
        }
    } else {
        let m = Rect::from_min_max(pos2(grid_origin_x, total_rect.top()), total_rect.right_bottom());
        (Some(m), None, None, None, None, available_track_space)
    };

    let whole_cols = (full_track_space / COL_WIDTH).floor() as usize;
    let fractional_width = full_track_space - (whole_cols as f32 * COL_WIDTH);

    let (modeling_grid_cols, modeling_content_width) =
        if num_columns + 3 > whole_cols { (num_columns + 3, (num_columns + 3) as f32 * COL_WIDTH) } else { (whole_cols, full_track_space) };

    let (assembly_grid_cols, assembly_content_width) =
        if num_joints + 3 > whole_cols { (num_joints + 3, (num_joints + 3) as f32 * COL_WIDTH) } else { (whole_cols, full_track_space) };

    let m_max_scroll_x = modeling_rect.map(|r| (modeling_content_width - r.width()).max(0.0)).unwrap_or(0.0);
    state.scroll_x = state.scroll_x.clamp(0.0, m_max_scroll_x);

    let a_max_scroll_x = assembly_rect.map(|r| (assembly_content_width - r.width()).max(0.0)).unwrap_or(0.0);
    state.assembly_scroll_x = state.assembly_scroll_x.clamp(0.0, a_max_scroll_x);

    let num_tracks = component_ids.len();
    let content_height = HEADER_TOTAL_HEIGHT + num_tracks as f32 * ROW_HEIGHT + SCROLLBAR_RESERVE_HEIGHT;
    let _grid_viewport_height = (total_rect.height() - HEADER_TOTAL_HEIGHT).max(0.0);
    let max_scroll_y = (content_height - total_rect.height()).max(0.0);
    state.scroll_y = state.scroll_y.clamp(0.0, max_scroll_y);

    // 5. Handle mouse wheel scrolling and rubberband overscroll
    let scroll_delta = ui.input(|i| {
        i.events
            .iter()
            .find_map(|e| match e {
                egui::Event::MouseWheel { delta, .. } => Some(*delta),
                _ => None,
            })
            .unwrap_or_default()
    });
    let pointer_in_asm = ui.input(|i| i.pointer.hover_pos().is_some_and(|pos| assembly_rect.is_some_and(|r| r.contains(pos))));

    if ui.rect_contains_pointer(total_rect) {
        if scroll_delta.x.abs() > 0.0 {
            if pointer_in_asm {
                state.assembly_scroll_x = (state.assembly_scroll_x - scroll_delta.x).clamp(0.0, a_max_scroll_x);
            } else {
                state.scroll_x = (state.scroll_x - scroll_delta.x).clamp(0.0, m_max_scroll_x);
            }
        }
        if scroll_delta.y.abs() > 0.0 {
            if ui.input(|i| i.modifiers.shift) {
                if pointer_in_asm {
                    state.assembly_scroll_x = (state.assembly_scroll_x - scroll_delta.y).clamp(0.0, a_max_scroll_x);
                } else {
                    state.scroll_x = (state.scroll_x - scroll_delta.y).clamp(0.0, m_max_scroll_x);
                }
            } else {
                let target_y = state.scroll_y - scroll_delta.y;
                if target_y < 0.0 {
                    state.overscroll_y = target_y * 0.35;
                    state.scroll_y = 0.0;
                } else if target_y > max_scroll_y {
                    state.overscroll_y = (target_y - max_scroll_y) * 0.35;
                    state.scroll_y = max_scroll_y;
                } else {
                    state.scroll_y = target_y;
                    state.overscroll_y *= 0.85;
                }
            }
        } else {
            state.overscroll_y *= 0.85;
        }
    }

    // 5b. Middle-button grab-and-drag pan gesture across entire timeline
    let is_middle_drag = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
    if is_middle_drag && ui.rect_contains_pointer(total_rect) {
        let delta = ui.input(|i| i.pointer.delta());
        if pointer_in_asm {
            state.assembly_scroll_x = (state.assembly_scroll_x - delta.x).clamp(0.0, a_max_scroll_x);
        } else {
            state.scroll_x = (state.scroll_x - delta.x).clamp(0.0, m_max_scroll_x);
        }
        state.scroll_y = (state.scroll_y - delta.y).clamp(0.0, max_scroll_y);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        ui.ctx().request_repaint();
    }

    // 6. Draw Orthogonal CAD Background Grid for Geometric Modeling
    let active_comp_id = app.session.active_component;
    let active_track_idx = component_ids.iter().position(|&cid| cid == active_comp_id);
    let grid_right = modeling_rect.map(|r| r.right()).unwrap_or(grid_origin_x);
    let m_is_open = modeling_rect.is_some();

    let grid_rect = if let Some(m_r) = modeling_rect {
        Rect::from_min_max(pos2(grid_origin_x, m_r.top() + HEADER_TOTAL_HEIGHT), m_r.right_bottom())
    } else {
        Rect::NOTHING
    };
    let grid_painter = ui.painter().with_clip_rect(grid_rect);
    let grid_line_color = t.border.gamma_multiply(0.25);

    if m_is_open {
        // Background grab-and-drag pan gesture on empty modeling grid
        let bg_resp = ui.interact(grid_rect, ui.scope_id().with("modeling_bg_pan"), Sense::drag());
        let marker_head_id = ui.scope_id().with("marker_head_2d");
        let marker_line_id = ui.scope_id().with("marker_line_2d");
        let is_marker_dragged = ui.ctx().is_being_dragged(marker_head_id) || ui.ctx().is_being_dragged(marker_line_id);
        if bg_resp.dragged() && state.drag.is_none() && !is_marker_dragged {
            let delta = bg_resp.drag_delta();
            state.scroll_x = (state.scroll_x - delta.x).clamp(0.0, m_max_scroll_x);
            state.scroll_y = (state.scroll_y - delta.y).clamp(0.0, max_scroll_y);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            ui.ctx().request_repaint();
        }

        // Subtle background wash on the active component track row
        if let Some(idx) = active_track_idx {
            let ry = total_rect.top() + HEADER_TOTAL_HEIGHT + idx as f32 * ROW_HEIGHT - state.scroll_y + state.overscroll_y;
            let active_track_rect = Rect::from_min_size(pos2(grid_origin_x, ry), vec2((grid_right - grid_origin_x).max(0.0), ROW_HEIGHT));
            let visible_active_rect = active_track_rect.intersect(grid_rect);
            if visible_active_rect.is_positive() {
                grid_painter.rect_filled(visible_active_rect, 0.0, accent.color.gamma_multiply(0.045));
            }
        }

        for r in 0..=num_tracks {
            let y = total_rect.top() + HEADER_TOTAL_HEIGHT + r as f32 * ROW_HEIGHT - state.scroll_y + state.overscroll_y;
            if y >= grid_rect.top() && y <= grid_rect.bottom() {
                let line_stroke = if active_track_idx.is_some_and(|idx| r == idx || r == idx + 1) {
                    Stroke::new(1.0, accent.color.gamma_multiply(0.20))
                } else {
                    Stroke::new(1.0, grid_line_color)
                };
                grid_painter.line_segment([pos2(grid_origin_x, y), pos2(grid_right, y)], line_stroke);
            }
        }
        for c in 0..=modeling_grid_cols {
            let x = grid_origin_x + c as f32 * COL_WIDTH - state.scroll_x;
            if x >= grid_origin_x && x <= grid_right {
                grid_painter.line_segment([pos2(x, grid_rect.top()), pos2(x, grid_rect.bottom())], Stroke::new(1.0, grid_line_color));
            }
        }

        // 6b. Draw subtle cell alignment dots (·) in unoccupied whole cells across the grid (NO DOT on half-column)
        if app.preferences.timeline_2d_cell_dots {
            let opacity = app.preferences.timeline_2d_dot_opacity.clamp(0.05, 1.0);
            let active_dot_color = accent.color.gamma_multiply(opacity * 0.9);
            let regular_dot_color = t.text_dim.gamma_multiply(opacity * 0.7);
            let inactive_dot_color = t.border.gamma_multiply(opacity * 0.5);

            for c in 0..whole_cols {
                let col_x = grid_origin_x + c as f32 * COL_WIDTH - state.scroll_x;
                if col_x + COL_WIDTH < grid_origin_x || col_x > grid_right {
                    continue;
                }
                let cx = col_x + COL_WIDTH / 2.0;
                let is_active = c < marker_visual_col;

                for t_idx in 0..num_tracks {
                    if cell_is_occupied(c, t_idx, &visual_columns, &feature_components_map, &component_ids, &app.session.doc) {
                        continue;
                    }
                    let cy =
                        total_rect.top() + HEADER_TOTAL_HEIGHT + t_idx as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0 - state.scroll_y + state.overscroll_y;
                    if cy >= grid_rect.top() - 2.0 && cy <= grid_rect.bottom() + 2.0 {
                        let dot_color = if Some(t_idx) == active_track_idx {
                            active_dot_color
                        } else if is_active {
                            regular_dot_color
                        } else {
                            inactive_dot_color
                        };
                        grid_painter.circle_filled(pos2(cx, cy), 1.75, dot_color);
                    }
                }
            }
        }
    }

    // 7. Calculate Active Selection and Movable Corridor in Visual Columns
    let selected_id = state.focused_feature();
    let visual_corridor = compute_visual_corridor(&visual_columns, &dep_graph, &state.selected_features);

    // Draw movable corridor background tint if selected (brackets painted in Step 11b on top of tracks and marker)
    if let Some((min_col, max_col)) = visual_corridor {
        let tracks_height = num_tracks.max(1) as f32 * ROW_HEIGHT;
        let content_tracks_top = grid_rect.top() - state.scroll_y + state.overscroll_y;
        let content_tracks_bottom = content_tracks_top + tracks_height;

        let corridor_top = content_tracks_top.max(grid_rect.top());
        let corridor_bottom = content_tracks_bottom.min(grid_rect.bottom());

        paint_movable_corridor_background(
            &grid_painter,
            grid_origin_x,
            state.scroll_x,
            corridor_top,
            corridor_bottom,
            min_col,
            max_col,
            marker_visual_col,
            &t,
        );
    }

    // 8. Paint Multi-Component Dotted Connectors & Action Chips
    let mut action_to_edit: Option<u64> = None;
    let pointer_pos = ui.input(|i| i.pointer.hover_pos());

    for (col_idx, col) in visual_columns.iter().enumerate() {
        if !m_is_open {
            break;
        }
        let col_x = grid_origin_x + col_idx as f32 * COL_WIDTH - state.scroll_x;
        if col_x + COL_WIDTH < grid_origin_x || col_x > grid_right {
            continue;
        }

        match col {
            VisualColumn::Features { features, open_group, .. } => {
                // If open group, draw subtle corridor tint in Tier 2
                if let Some(_grp_id) = open_group {
                    let banner_rect = Rect::from_min_size(
                        pos2(col_x, total_rect.top() + HEADER_BANNER_HEIGHT + HEADER_T1_HEIGHT),
                        vec2(COL_WIDTH, HEADER_T2_HEIGHT),
                    );
                    let tint = t.accent.gamma_multiply(0.06);
                    painter.rect_filled(banner_rect, 0.0, tint);
                }

                let is_active = col_idx < marker_visual_col;

                // Collect all chips to paint for this column: (fid, track_idx)
                let mut column_chips: Vec<(u64, usize)> = Vec::new();
                let mut all_track_indices: Vec<usize> = Vec::new();
                let mut any_selected = false;

                for &fid in features {
                    if let Some(f) = app.session.doc.feature(fid) {
                        let affected = feature_components_map.get(&fid).cloned().unwrap_or_else(|| vec![f.component]);
                        let mut tracks: Vec<usize> = affected.iter().filter_map(|cid| component_ids.iter().position(|id| id == cid)).collect();
                        if tracks.is_empty() {
                            let default_track = component_ids.iter().position(|&cid| cid == f.component).unwrap_or(0);
                            tracks.push(default_track);
                        }
                        tracks.sort_unstable();
                        tracks.dedup();

                        if state.is_selected(fid) {
                            any_selected = true;
                        }

                        for &t_idx in &tracks {
                            column_chips.push((fid, t_idx));
                            all_track_indices.push(t_idx);
                        }
                    }
                }

                all_track_indices.sort_unstable();
                all_track_indices.dedup();

                // If multi-component (or multiple actions across tracks), draw dead-centered vertical dotted connector
                if all_track_indices.len() > 1
                    && let (Some(&min_track), Some(&max_track)) = (all_track_indices.first(), all_track_indices.last())
                {
                    let y1 = total_rect.top() + HEADER_TOTAL_HEIGHT + min_track as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0 - state.scroll_y
                        + state.overscroll_y;
                    let y2 = total_rect.top() + HEADER_TOTAL_HEIGHT + max_track as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0 - state.scroll_y
                        + state.overscroll_y;

                    if y1 < y2 && y1 < grid_rect.bottom() && y2 > grid_rect.top() {
                        let cx = col_x + COL_WIDTH / 2.0;
                        let line_color = if any_selected {
                            if is_active { accent.color } else { t.text_dim }
                        } else {
                            if is_active { t.text_dim } else { t.border.gamma_multiply(0.7) }
                        };
                        let dot_radius = if any_selected { 1.5 } else { 1.25 };
                        paint_dotted_vertical_line(&grid_painter, cx, y1, y2, line_color, dot_radius);
                    }
                }

                let prereq_col = crate::theme::model_color(app.preferences.timeline_2d_prereq_color);
                let dep_col = crate::theme::model_color(app.preferences.timeline_2d_dependent_color);

                // Paint individual chips
                for (fid, track_idx) in column_chips {
                    if let Some(f) = app.session.doc.feature(fid) {
                        let chip_top = total_rect.top() + HEADER_TOTAL_HEIGHT + track_idx as f32 * ROW_HEIGHT + (ROW_HEIGHT - CHIP_HEIGHT) / 2.0
                            - state.scroll_y
                            + state.overscroll_y;
                        let chip_rect = Rect::from_min_size(pos2(col_x + (COL_WIDTH - CHIP_WIDTH) / 2.0, chip_top), vec2(CHIP_WIDTH, CHIP_HEIGHT));

                        if chip_rect.bottom() < grid_rect.top() || chip_rect.top() > grid_rect.bottom() {
                            continue;
                        }

                        let is_active = col_idx < marker_visual_col;
                        let is_selected = state.is_selected(fid);
                        let is_prereq = selected_id.is_some_and(|sel| dep_graph.is_prerequisite(sel, fid));
                        let is_dependent = selected_id.is_some_and(|sel| dep_graph.is_dependent(sel, fid));

                        let resp = ui.interact(chip_rect, ui.scope_id().with(("chip_2d", fid, track_idx)), Sense::click_and_drag());
                        crate::scenario::publish_handle(&format!("timeline_2d:{}", f.name), chip_rect.center());
                        if resp.hovered() {
                            state.hovered_feature = Some(fid);
                        }

                        if resp.drag_started() {
                            state.drag = Some(super::state::DragTarget::Feature {
                                id: fid,
                                start_col: col_idx,
                                track_idx,
                                current_pos: pointer_pos.unwrap_or(chip_rect.center()),
                            });
                            if !state.is_selected(fid) {
                                state.select_single(fid);
                            }
                        }

                        let is_suppressed = dep_graph.is_suppressed(&app.session.doc, fid);

                        // Attach rich hover tooltip showing action name, kind, status
                        let resp = if state.drag.is_none() {
                            let res = app.session.model.result(fid);
                            let err = res.and_then(|r| r.error.as_deref());
                            let warn = res.and_then(|r| r.warning.as_deref());
                            let mut tip = crate::timeline::status_text(&f.name, f.kind.type_name(), err, warn);
                            if is_suppressed {
                                tip.push_str("\nSuppressed (excluded from 3D solid geometry rebuild)");
                            }
                            if !is_active {
                                let steps_after = col_idx.saturating_sub(marker_visual_col) + 1;
                                tip.push_str(&format!("\nInactive (rolled back, {} step(s) after marker)", steps_after));
                            }
                            resp.on_hover_text(tip)
                        } else {
                            resp
                        };

                        // Selection clicks are handled after the chips are drawn: an open dialog may
                        // take the feature first, as in the 1D timeline.
                        if resp.clicked() {
                            clicked_chip = Some((fid, ui.input(|i| i.modifiers)));
                        }

                        if resp.double_clicked() {
                            action_to_edit = Some(fid);
                        }

                        let is_being_dragged = matches!(state.drag, Some(super::state::DragTarget::Feature { id, .. }) if id == fid);

                        if is_being_dragged {
                            grid_painter.rect_filled(chip_rect, egui::CornerRadius::same(t.radius_sm), t.timeline.gamma_multiply(0.35));
                            grid_painter.rect_stroke(
                                chip_rect,
                                egui::CornerRadius::same(t.radius_sm),
                                Stroke::new(1.0, t.border.gamma_multiply(0.4)),
                                StrokeKind::Inside,
                            );
                        } else {
                            // Paint action chip
                            paint_action_chip(
                                &grid_painter,
                                chip_rect,
                                f,
                                is_active,
                                is_selected,
                                is_prereq,
                                is_dependent,
                                is_suppressed,
                                prereq_col,
                                dep_col,
                                accent,
                                &t,
                            );
                        }

                        // A right-click outside the selection selects that chip first, in the same
                        // frame, so the menu below is built for it alone.
                        if resp.secondary_clicked() {
                            state.select_for_context_menu(fid);
                        }

                        // Attach context menu
                        if state.drag.is_none() {
                            Menu::context(&resp, |ui| {
                                // Built only while the menu is open, not for every chip each frame.
                                let targets = if state.selected_features.contains(&fid) {
                                    state.selected_features.iter().copied().collect::<Vec<_>>()
                                } else {
                                    vec![fid]
                                };
                                let is_adjacent = state.is_selection_adjacent(&dep_graph.features);
                                let sel_count = state.selected_features.len();
                                if crate::context_menu::button(ui, crate::i18n::tr_label("Edit Feature"), "edit").clicked() {
                                    action_to_edit = Some(fid);
                                    ui.close();
                                }
                                if is_adjacent && sel_count >= 2 {
                                    let label = crate::i18n::format("ui.timeline_2d_group_selection", &[("count", &sel_count.to_string())]);
                                    if crate::context_menu::button(ui, &label, "folder").clicked() {
                                        let ordered_selected: Vec<u64> =
                                            dep_graph.features.iter().copied().filter(|id| state.selected_features.contains(id)).collect();
                                        let group_name = format!("Group {}", app.session.doc.timeline_groups.len() + 1);
                                        if let Ok(val) = app.run_heavy(
                                            "timeline.group",
                                            serde_json::json!({
                                                "name": group_name,
                                                "features": ordered_selected,
                                            }),
                                        ) && let Some(gid) = val.get("group").and_then(|v| v.as_u64())
                                        {
                                            state.folded_groups.insert(gid);
                                            app.tree.collapsed.insert(format!("timeline_group:{gid}"));
                                        }
                                        ui.close();
                                    }
                                }
                                // 2D Timeline only: Rename is hidden when the right-clicked chip is part of a
                                // multi-selection, because this menu then acts on the whole selection. The 1D
                                // timeline (crates/ui-egui/src/timeline.rs) keeps its own Rename rule.
                                if sel_count <= 1 && crate::context_menu::button(ui, crate::i18n::tr_label("Rename"), "rename").clicked() {
                                    let f_name = app.session.doc.feature(fid).map(|f| f.name.clone()).unwrap_or_default();
                                    app.dialog = Some(crate::dialogs::Dialog::rename(fid, &f_name));
                                    ui.close();
                                }
                                let suppress_key = if is_suppressed { "timeline.unsuppress_features" } else { "timeline.suppress_features" };
                                let suppress_icon = if is_suppressed { "eye" } else { "suppress" };
                                if crate::context_menu::button(ui, crate::i18n::tr(suppress_key), suppress_icon).clicked() {
                                    let base_targets = if state.selected_features.contains(&fid) {
                                        state.selected_features.iter().copied().collect::<Vec<_>>()
                                    } else {
                                        vec![fid]
                                    };
                                    state.separately_suppressed_features.retain(|id| app.session.doc.feature(*id).is_some_and(|f| f.suppressed));

                                    let target_features = if is_suppressed {
                                        dep_graph.collect_unsuppression_with_mode(
                                            &app.session.doc,
                                            &base_targets,
                                            app.preferences.timeline_2d_unsuppress_mode,
                                            &state.separately_suppressed_features,
                                        )
                                    } else {
                                        for &id in &base_targets {
                                            state.separately_suppressed_features.insert(id);
                                        }
                                        dep_graph.collect_suppression_cascade_for_set(&base_targets)
                                    };
                                    let _ = app.run_heavy(
                                        "timeline.suppress",
                                        serde_json::json!({
                                            "features": target_features,
                                            "suppressed": !is_suppressed,
                                        }),
                                    );
                                    ui.close();
                                }
                                if crate::context_menu::button(ui, crate::i18n::tr_label("Roll History Marker Here"), "rollback").clicked() {
                                    let _ = app.run_heavy("timeline.roll_to", serde_json::json!({ "feature": fid }));
                                    ui.close();
                                }
                                ui.separator();
                                let delete_label = match targets.len() {
                                    1 => crate::i18n::tr_label("Delete").to_string(),
                                    n => crate::i18n::format("timeline.delete_many", &[("count", &n.to_string())]),
                                };
                                if crate::context_menu::button(ui, &delete_label, "delete").clicked() {
                                    crate::timeline::delete_features(app, targets);
                                    ui.close();
                                }
                            });
                        }
                    }
                }
            }
            VisualColumn::FoldedGroup { .. } => {
                // Handled in Step 9b after header background so the integrated header cap is visible on Tier 2
            }
        }
    }

    // 9. Paint Two-Tier Header Track
    if m_is_open {
        paint_two_tier_header(
            &painter,
            ui,
            app,
            total_rect,
            grid_origin_x,
            grid_right,
            state.scroll_x,
            &visual_columns,
            marker_visual_col,
            marker_slot,
            total_features,
            accent,
            &t,
            &mut state,
        );
    }

    // 9b. Paint Folded Group Virtual Chips (starts on Tier 2 of Header, spans component tracks)
    for (col_idx, col) in visual_columns.iter().enumerate() {
        if !m_is_open {
            break;
        }
        let col_x = grid_origin_x + col_idx as f32 * COL_WIDTH - state.scroll_x;
        if col_x + COL_WIDTH < grid_origin_x || col_x > grid_right {
            continue;
        }

        if let VisualColumn::FoldedGroup { group_id, features } = col {
            let card_top = total_rect.top() + HEADER_BANNER_HEIGHT + HEADER_T1_HEIGHT + 2.0;
            let card_bottom = ((total_rect.top() + HEADER_TOTAL_HEIGHT + component_ids.len().max(1) as f32 * ROW_HEIGHT - 6.0 - state.scroll_y
                + state.overscroll_y)
                .min(total_rect.bottom() - 6.0))
            .max(card_top + HEADER_T2_HEIGHT + 24.0);
            let card_height = card_bottom - card_top;
            let card_rect = Rect::from_min_size(pos2(col_x + (COL_WIDTH - CHIP_WIDTH) / 2.0, card_top), vec2(CHIP_WIDTH, card_height));

            let is_active = col_idx < marker_visual_col;
            let is_group_selected = features.iter().any(|id| state.is_selected(*id));

            let grp_name = app
                .session
                .doc
                .timeline_groups
                .iter()
                .find(|g| g.id == *group_id)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| format!("Group {group_id}"));

            let header_cap_rect = Rect::from_min_size(card_rect.left_top(), vec2(card_rect.width(), HEADER_T2_HEIGHT - 2.0));
            crate::scenario::publish_handle(&format!("group_card:{}", grp_name), header_cap_rect.center());

            let resp = ui.interact(card_rect, ui.scope_id().with(("grp_card", *group_id)), Sense::click_and_drag());
            if resp.hovered() {
                state.hovered_group = Some(*group_id);
                if let Some(pos) = pointer_pos
                    && header_cap_rect.contains(pos)
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }

            if resp.clicked() {
                if let Some(pos) = pointer_pos
                    && header_cap_rect.contains(pos)
                {
                    state.set_group_folded(*group_id, false);
                } else {
                    // Like a plain chip click, the group's features replace any picked joints too.
                    app.tree.picked_joints.clear();
                    state.selected_joints.clear();
                    state.selected_features.clear();
                    for &fid in features {
                        state.selected_features.insert(fid);
                    }
                }
            }
            if resp.double_clicked() {
                state.toggle_group_fold(*group_id);
            }

            // Rich hover tooltip
            let mut tooltip_feats = Vec::new();
            for &fid in features {
                if let Some(f) = app.session.doc.feature(fid) {
                    let step = dep_graph.id_to_index.get(&fid).copied().unwrap_or(0) + 1;
                    let comp_name = component_names.get(&f.component).map(|s| s.as_str()).unwrap_or("Component");
                    tooltip_feats.push((step, f.name.as_str(), comp_name));
                }
            }
            let tip = Timeline2DState::format_group_tooltip(&grp_name, &tooltip_feats, is_active);
            let resp = resp.on_hover_text(tip);

            // Group Context Menu
            let gid = *group_id;
            let g_name = grp_name.clone();
            Menu::context(&resp, |ui| {
                if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_open_group"), "folder").clicked() {
                    state.set_group_folded(gid, false);
                    ui.close();
                }
                if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_rename_group"), "rename").clicked() {
                    app.dialog = Some(crate::dialogs::Dialog::rename_group(gid, &g_name));
                    ui.close();
                }
                if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_delete_group_expand"), "folder").clicked() {
                    let _ = app.run_heavy("timeline.ungroup", serde_json::json!({ "group": gid }));
                    ui.close();
                }
                if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_delete_group_all"), "delete").clicked() {
                    let contents = crate::timeline_left::folder_features(app, &[gid]);
                    crate::timeline::delete_folders(app, vec![gid], contents);
                    ui.close();
                }
            });

            paint_virtual_group_chip(&painter, card_rect, &grp_name, features.len(), is_active, is_group_selected, &t);
        }
    }

    // 10. Paint Pinned Left Headers & Transport Controls
    paint_pinned_left(&painter, ui, app, total_rect, &component_ids, &component_names, state.scroll_y, state.overscroll_y, floating, &t);

    // 11. Paint Extended Rollback Marker
    let (line_x, pin_x) = if marker_visual_col == 0 {
        (grid_origin_x + 4.0 - state.scroll_x, grid_origin_x + 4.0 + 7.0 - state.scroll_x)
    } else {
        let normal_x = grid_origin_x + marker_visual_col.min(num_columns) as f32 * COL_WIDTH - state.scroll_x;
        (normal_x, normal_x)
    };

    if m_is_open && pin_x >= grid_origin_x - 20.0 && pin_x <= grid_right + 20.0 {
        let t1_top = total_rect.top() + HEADER_BANNER_HEIGHT;
        let pin_center = pos2(pin_x, t1_top + HEADER_T1_HEIGHT / 2.0);
        let line_top = t1_top + HEADER_T1_HEIGHT;
        let tracks_top = total_rect.top() + HEADER_TOTAL_HEIGHT;
        let scrollbar_pad = if m_max_scroll_x > 0.0 { 8.0 } else { 2.0 };
        let line_bottom = (total_rect.bottom() - scrollbar_pad).max(tracks_top + 4.0);
        let m_section_rect = modeling_rect.unwrap_or(grid_rect);

        let head_hit_rect = Rect::from_min_max(pos2(pin_x - 8.0, t1_top), pos2(pin_x + 8.0, tracks_top)).intersect(m_section_rect);
        let line_half_left = 6.0;
        let line_half_right = if marker_visual_col == 0 { 2.5 } else { 6.0 };
        let line_hit_rect =
            Rect::from_min_max(pos2(line_x - line_half_left, tracks_top), pos2(line_x + line_half_right, line_bottom)).intersect(grid_rect);

        let head_resp = ui.interact(head_hit_rect, ui.scope_id().with("marker_head_2d"), Sense::click_and_drag());
        let line_resp = ui.interact(line_hit_rect, ui.scope_id().with("marker_line_2d"), Sense::click_and_drag());

        crate::scenario::publish_handle("timeline_2d:marker_head", pin_center);
        if line_hit_rect.is_positive() {
            crate::scenario::publish_handle("timeline_2d:marker_line", line_hit_rect.center());
        }

        let marker_hovered = head_resp.hovered() || line_resp.hovered();
        let marker_dragged = head_resp.dragged() || line_resp.dragged();

        if marker_dragged {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if marker_hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        if marker_dragged && let Some(pos) = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos())) {
            let target_visual_col = ((pos.x - grid_origin_x + state.scroll_x) / COL_WIDTH).round().max(0.0) as usize;
            let target_visual_col = target_visual_col.min(num_columns);
            if target_visual_col != marker_visual_col {
                let target_slot = if target_visual_col < num_columns {
                    visual_columns[target_visual_col]
                        .feature_ids()
                        .first()
                        .and_then(|&fid| dep_graph.id_to_index.get(&fid).copied())
                        .unwrap_or(total_features)
                } else {
                    total_features
                };
                if target_slot != marker_slot {
                    let params = if target_slot < total_features { serde_json::json!({ "position": target_slot }) } else { serde_json::json!({}) };
                    let _ = app.run_heavy("timeline.roll_to", params);
                }
            }
            ui.ctx().request_repaint();
        }

        let p_m = painter.with_clip_rect(m_section_rect);
        let line_width = if marker_hovered || marker_dragged { 3.0 } else { 2.5 };

        // Vertical marker line through Tier 2 and component tracks (clipped to modeling section)
        p_m.line_segment([pos2(line_x, line_top), pos2(line_x, line_bottom)], Stroke::new(line_width, accent.color));

        // Bottom handle cap on vertical line
        let bottom_cap_rect = Rect::from_min_size(pos2(line_x - 5.0, line_bottom - 4.0), vec2(10.0, 4.0));
        p_m.rect_filled(bottom_cap_rect, egui::CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, accent.color);

        // Square pin handle in Tier 1 contacting vertical line
        let pin_size = if marker_hovered || marker_dragged { vec2(15.0, 15.0) } else { vec2(14.0, 14.0) };
        let square_rect = Rect::from_center_size(pin_center, pin_size);
        let (pin_bg, inner_col) = if marker_hovered || marker_dragged { (accent.color, t.timeline) } else { (t.timeline, accent.color) };
        p_m.rect_filled(square_rect, 2.5, pin_bg);
        p_m.rect_stroke(square_rect, 2.5, Stroke::new(2.0, accent.color), StrokeKind::Inside);
        let inner_rect = Rect::from_center_size(pin_center, vec2(4.0, 4.0));
        p_m.rect_filled(inner_rect, 1.0, inner_col);
    }

    // 11b. Paint Movable Corridor Brackets on top of grid tracks and the marker line
    if let Some((min_col, max_col)) = visual_corridor {
        let tracks_height = num_tracks.max(1) as f32 * ROW_HEIGHT;
        let content_tracks_top = grid_rect.top() - state.scroll_y + state.overscroll_y;
        let content_tracks_bottom = content_tracks_top + tracks_height;

        let bracket_top = (content_tracks_top + 4.0).max(grid_rect.top() + 4.0);
        let bracket_bottom = (content_tracks_bottom - 4.0).min(grid_rect.bottom() - 4.0);

        paint_movable_corridor_brackets(
            &grid_painter,
            grid_origin_x,
            state.scroll_x,
            bracket_top,
            bracket_bottom,
            min_col,
            max_col,
            marker_visual_col,
            &t,
        );
    }

    // 11c. Paint Splitter and Post-Modeling Assembly Track Section
    if let Some(strip_m) = strip_modeling_rect {
        super::assembly::paint_collapsed_strip(&painter, ui, strip_m, true, num_columns, &mut state, &t);
    }

    if let Some(sp) = splitter_rect {
        super::assembly::paint_splitter(
            &painter,
            ui,
            sp,
            available_track_space,
            grid_origin_x,
            state.is_modeling_collapsed,
            state.is_assembly_collapsed,
            &mut state,
            &t,
        );
    }

    if let Some(a_rect) = assembly_rect {
        super::assembly::paint_assembly_section(
            &painter,
            ui,
            app,
            a_rect,
            assembly_content_width,
            assembly_grid_cols,
            whole_cols,
            fractional_width,
            &component_ids,
            &visual_joints,
            active_comp_id,
            &mut state,
            &t,
        );
    }

    if let Some(strip_a) = strip_assembly_rect {
        super::assembly::paint_collapsed_strip(&painter, ui, strip_a, false, num_joints, &mut state, &t);
    }

    // 12. Active Drag-and-Drop Processing for Feature Action Chips (Horizontal Reordering)
    if let Some(super::state::DragTarget::Feature { id: drag_fid, start_col, track_idx, current_pos: last_pos }) = state.drag {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.drag = None;
        } else {
            let current_pos = pointer_pos.unwrap_or(last_pos);
            state.drag = Some(super::state::DragTarget::Feature { id: drag_fid, start_col, track_idx, current_pos });

            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            ui.ctx().request_repaint();

            let content_tracks_top = grid_rect.top() - state.scroll_y + state.overscroll_y;
            let col_f = (current_pos.x - grid_origin_x + state.scroll_x) / COL_WIDTH;
            let target_visual_col = (col_f.floor() as isize).clamp(0, (num_columns.saturating_sub(1)) as isize) as usize;

            // Constrained to the original component track row horizontally
            let mut single_sel = BTreeSet::new();
            single_sel.insert(drag_fid);
            let drag_corridor = compute_visual_corridor(&visual_columns, &dep_graph, &single_sel);
            let is_valid_drop = drag_corridor.is_some_and(|(min_c, max_c)| target_visual_col >= min_c && target_visual_col <= max_c);

            // A. Target cell highlight in grid (constrained to origin track row)
            let target_cell_x = grid_origin_x + target_visual_col as f32 * COL_WIDTH - state.scroll_x;
            let target_cell_y = content_tracks_top + track_idx as f32 * ROW_HEIGHT;
            let target_cell_rect = Rect::from_min_size(pos2(target_cell_x, target_cell_y), vec2(COL_WIDTH, ROW_HEIGHT));

            if target_cell_rect.intersects(grid_rect) {
                let cell_tint = if is_valid_drop { t.form_success.gamma_multiply(0.12) } else { t.error.gamma_multiply(0.12) };
                let cell_stroke = if is_valid_drop { Stroke::new(1.5, t.form_success) } else { Stroke::new(1.5, t.error) };
                grid_painter.rect_filled(target_cell_rect, 0.0, cell_tint);
                grid_painter.rect_stroke(target_cell_rect, 0.0, cell_stroke, StrokeKind::Inside);
            }

            // B. Vertical insertion drop bar
            if target_visual_col != start_col && is_valid_drop {
                let bar_x = if target_visual_col > start_col { target_cell_x + COL_WIDTH } else { target_cell_x };
                let bar_rect = Rect::from_min_max(pos2(bar_x - 1.5, target_cell_y + 3.0), pos2(bar_x + 1.5, target_cell_y + ROW_HEIGHT - 3.0));
                grid_painter.rect_filled(bar_rect, 1.0, t.sketch_accent);
            }

            // C. Floating dragged chip locked to horizontal rail of its component track
            if let Some(f) = app.session.doc.feature(drag_fid) {
                let track_center_y = target_cell_y + ROW_HEIGHT / 2.0;
                let float_center = pos2(current_pos.x, track_center_y);
                let float_rect = Rect::from_center_size(float_center, vec2(CHIP_WIDTH * 1.08, CHIP_WIDTH * 1.08));
                let stroke_col = if is_valid_drop { t.form_success } else { t.error };
                painter.rect_filled(float_rect, egui::CornerRadius::same(t.radius_sm), t.panel);
                painter.rect_stroke(float_rect, egui::CornerRadius::same(t.radius_sm), Stroke::new(2.0, stroke_col), StrokeKind::Inside);

                let icon = crate::timeline::icon_of(&f.kind);
                let icon_rect = Rect::from_center_size(float_rect.center(), vec2(20.0, 20.0));
                crate::icons::paint_ink(&painter, icon_rect, icon, stroke_col);
                if dep_graph.is_suppressed(&app.session.doc, drag_fid) {
                    crate::icons::paint_icon(&painter, icon_rect, "suppress", 0.5);
                }
            }

            // D. Finalize drop on mouse button release (horizontal reorder only)
            if ui.input(|i| i.pointer.any_released()) {
                state.drag = None;
                let mut changed = false;

                if target_visual_col != start_col && is_valid_drop {
                    let target_fid = if target_visual_col > start_col {
                        visual_columns.get(target_visual_col).and_then(|vc| vc.feature_ids().last().copied())
                    } else {
                        visual_columns.get(target_visual_col).and_then(|vc| vc.feature_ids().first().copied())
                    };
                    if let Some(tfid) = target_fid
                        && let Some(&to_slot) = dep_graph.id_to_index.get(&tfid)
                    {
                        let is_active = target_visual_col < marker_visual_col;
                        let res = app.run_heavy(
                            "timeline.reorder",
                            serde_json::json!({
                                "feature": drag_fid,
                                "position": to_slot,
                                "active": is_active,
                            }),
                        );
                        if res.is_ok() {
                            changed = true;
                        }
                    }
                }

                if changed {
                    ui.ctx().request_repaint();
                }
            }
        }
    }

    if let Some((fid, modifiers)) = clicked_chip
        && !crate::timeline::pick_feature_into_dialog(app, fid, modifiers)
    {
        if modifiers.shift {
            keep_other_selection = modifiers.command || modifiers.ctrl;
            state.select_range(&dep_graph.features, fid, keep_other_selection);
        } else if modifiers.command || modifiers.ctrl {
            keep_other_selection = true;
            state.toggle_selection(fid);
        } else if !modifiers.any() && crate::browser::pick_sketch_into_dialog(app, fid) {
            // Extrude, Revolve or Emboss took every profile of the sketch (#6106).
        } else {
            // A plain click selects the feature alone, as in the 1D timeline.
            app.tree.picked_joints.clear();
            state.selected_joints.clear();
            state.select_single(fid);
        }
    }

    if let Some(edit_id) = action_to_edit {
        app.edit_feature(edit_id);
    }

    // 13. Horizontal Scrollbar for Modeling Track when content overflows
    if let Some(m_r) = modeling_rect
        && m_max_scroll_x > 0.0
    {
        let bar_h = 6.0;
        let bar_y = m_r.bottom() - bar_h - 1.0;
        let slot_rect = Rect::from_min_size(pos2(grid_origin_x, bar_y), vec2(m_r.width(), bar_h));
        let p_sb = painter.with_clip_rect(m_r);
        p_sb.rect_filled(slot_rect, 0.0, t.timeline.gamma_multiply(0.4));

        let ratio = (m_r.width() / modeling_content_width).clamp(0.05, 1.0);
        let pill_w = (m_r.width() * ratio).max(24.0);
        let scroll_frac = (state.scroll_x / m_max_scroll_x).clamp(0.0, 1.0);
        let pill_x = grid_origin_x + scroll_frac * (m_r.width() - pill_w);
        let pill_rect = Rect::from_min_size(pos2(pill_x, bar_y), vec2(pill_w, bar_h));

        let pill_resp = ui.interact(pill_rect, ui.scope_id().with("modeling_h_scrollbar"), Sense::drag());
        if pill_resp.dragged() {
            let dx = pill_resp.drag_delta().x;
            let available_travel = (m_r.width() - pill_w).max(1.0);
            state.scroll_x = (state.scroll_x + dx * (m_max_scroll_x / available_travel)).clamp(0.0, m_max_scroll_x);
            ui.ctx().request_repaint();
        }

        let pill_color = if pill_resp.hovered() || pill_resp.dragged() { accent.color } else { t.border.gamma_multiply(0.6) };
        p_sb.rect_filled(pill_rect, 3.0, pill_color);
    }

    // 14. Unified Vertical Scrollbar across component tracks when height overflows
    if max_scroll_y > 0.0 {
        let bar_w = 6.0;
        let right_x = total_rect.right() - bar_w - 2.0;
        let bar_top = total_rect.top() + HEADER_TOTAL_HEIGHT;
        let bar_height = (total_rect.height() - HEADER_TOTAL_HEIGHT).max(1.0);
        let slot_rect = Rect::from_min_size(pos2(right_x, bar_top), vec2(bar_w, bar_height));
        let p_vsb = painter.with_clip_rect(total_rect);
        p_vsb.rect_filled(slot_rect, 0.0, t.timeline.gamma_multiply(0.4));

        let ratio = (bar_height / content_height).clamp(0.05, 1.0);
        let pill_h = (bar_height * ratio).max(24.0);
        let scroll_frac = (state.scroll_y / max_scroll_y).clamp(0.0, 1.0);
        let pill_y = bar_top + scroll_frac * (bar_height - pill_h);
        let pill_rect = Rect::from_min_size(pos2(right_x, pill_y), vec2(bar_w, pill_h));

        let pill_resp = ui.interact(pill_rect, ui.scope_id().with("timeline_v_scrollbar"), Sense::drag());
        if pill_resp.dragged() {
            let dy = pill_resp.drag_delta().y;
            let available_travel = (bar_height - pill_h).max(1.0);
            state.scroll_y = (state.scroll_y + dy * (max_scroll_y / available_travel)).clamp(0.0, max_scroll_y);
            ui.ctx().request_repaint();
        }

        let pill_color = if pill_resp.hovered() || pill_resp.dragged() { t.text } else { t.border.gamma_multiply(0.6) };
        p_vsb.rect_filled(pill_rect, 3.0, pill_color);
    }

    if state.selected_features != selection_before {
        push_feature_selection(app, &state.selected_features, keep_other_selection);
    }

    // Persist state in egui memory
    state.store_scoped(ui.ctx(), Some(app.session.uid));
}

/// Sends the 2D Timeline's feature selection to the session through `select.set`, so Delete, the
/// 3D view and agents see it. `keep_other` keeps selected faces, edges and the like, as a Ctrl
/// click in the 1D timeline does; otherwise the features replace the whole selection.
fn push_feature_selection(app: &mut SolveApp, features: &BTreeSet<u64>, keep_other: bool) {
    use solidcraft_engine::Sel;
    let mut items: Vec<Sel> =
        if keep_other { app.session.selection.iter().filter(|s| !matches!(s, Sel::Feature { .. })).cloned().collect() } else { Vec::new() };
    items.extend(features.iter().map(|&id| Sel::Feature { id }));
    let _ = app.run("select.set", serde_json::json!({ "items": items }));
}

/// Helper: constructs visual columns respecting folded group states.
fn build_visual_columns(doc: &Document, folded_groups: &BTreeSet<u64>) -> Vec<VisualColumn> {
    let mut columns = Vec::new();
    let mut i = 0;
    let n = doc.features.len();

    while i < n {
        let f = &doc.features[i];
        let grp = doc.timeline_groups.iter().find(|g| g.features.contains(&f.id));

        if let Some(g) = grp {
            if folded_groups.contains(&g.id) {
                // Collect contiguous features belonging to this group
                let mut grp_features = Vec::new();
                while i < n && g.features.contains(&doc.features[i].id) {
                    grp_features.push(doc.features[i].id);
                    i += 1;
                }
                columns.push(VisualColumn::FoldedGroup { group_id: g.id, features: grp_features });
                continue;
            } else {
                columns.push(VisualColumn::Features { step_number: i + 1, features: vec![f.id], open_group: Some(g.id) });
                i += 1;
                continue;
            }
        }

        columns.push(VisualColumn::Features { step_number: i + 1, features: vec![f.id], open_group: None });
        i += 1;
    }
    columns
}

/// Helper: paints the Two-Tier Header track (Banner + Tier 1: Step numbers + Tier 2: Folders/Features).
#[allow(clippy::too_many_arguments)]
fn paint_two_tier_header(
    p: &egui::Painter,
    ui: &mut Ui,
    app: &mut SolveApp,
    total_rect: Rect,
    grid_origin_x: f32,
    grid_right: f32,
    scroll_x: f32,
    columns: &[VisualColumn],
    marker_visual_col: usize,
    marker_slot: usize,
    total_features: usize,
    accent: crate::theme::TabAccent,
    t: &Tokens,
    state: &mut Timeline2DState,
) {
    let header_rect = Rect::from_min_max(pos2(grid_origin_x, total_rect.top()), pos2(grid_right, total_rect.top() + HEADER_TOTAL_HEIGHT));
    let header_rounding = egui::CornerRadius { nw: 0, ne: t.radius_lg, sw: 0, se: 0 };
    p.rect_filled(header_rect, header_rounding, t.timeline);

    // Top Banner
    let banner_rect = Rect::from_min_max(pos2(grid_origin_x, total_rect.top()), pos2(grid_right, total_rect.top() + HEADER_BANNER_HEIGHT));
    p.rect_filled(banner_rect, 0.0, t.panel);
    p.line_segment([banner_rect.left_bottom(), banner_rect.right_bottom()], Stroke::new(1.0, t.border.gamma_multiply(0.4)));

    // Diamond icon + Title + Count Badge
    p.text(pos2(banner_rect.left() + 10.0, banner_rect.center().y), egui::Align2::LEFT_CENTER, "◆", FontId::proportional(t.font_small), accent.color);
    p.text(
        pos2(banner_rect.left() + 24.0, banner_rect.center().y),
        egui::Align2::LEFT_CENTER,
        "MODELING FEATURES",
        FontId::proportional(t.font_small),
        t.text,
    );
    let count_text = format!("{} Steps", total_features);
    let badge_rect = Rect::from_center_size(pos2(banner_rect.left() + 175.0, banner_rect.center().y), vec2(58.0, 16.0));
    p.rect_filled(badge_rect, 3.0, accent.color.gamma_multiply(0.18));
    p.rect_stroke(badge_rect, 3.0, Stroke::new(1.0, accent.color.gamma_multiply(0.4)), StrokeKind::Inside);
    p.text(badge_rect.center(), egui::Align2::CENTER_CENTER, count_text, FontId::monospace(t.font_badge), accent.color);

    // Right side: Active summary and quick transport buttons
    if banner_rect.width() > 320.0 {
        let active_features = app.session.doc.features.iter().take(marker_slot.min(total_features)).filter(|f| !f.suppressed).count();
        let active_summary = format!("Active: {} / {}", active_features, total_features);
        p.text(
            pos2(banner_rect.right() - 85.0, banner_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            active_summary,
            FontId::monospace(t.font_small),
            accent.color,
        );

        let btn_b = Rect::from_center_size(pos2(banner_rect.right() - 65.0, banner_rect.center().y), vec2(16.0, 16.0));
        let btn_f = Rect::from_center_size(pos2(banner_rect.right() - 44.0, banner_rect.center().y), vec2(16.0, 16.0));
        let btn_e = Rect::from_center_size(pos2(banner_rect.right() - 22.0, banner_rect.center().y), vec2(18.0, 16.0));

        let resp_b = ui.interact(btn_b, ui.scope_id().with("m_step_back"), Sense::click());
        let resp_f = ui.interact(btn_f, ui.scope_id().with("m_step_forward"), Sense::click());
        let resp_e = ui.interact(btn_e, ui.scope_id().with("m_step_end"), Sense::click());

        let ink_b = if resp_b.hovered() { t.text } else { t.text_dim };
        let ink_f = if resp_f.hovered() { t.text } else { t.text_dim };
        let ink_e = if resp_e.hovered() { t.text } else { t.text_dim };
        crate::icons::glyphs::transport_glyph(p, btn_b.center(), "back", ink_b);
        crate::icons::glyphs::transport_glyph(p, btn_f.center(), "forward", ink_f);
        crate::icons::glyphs::transport_glyph(p, btn_e.center(), "last", ink_e);

        if resp_b.clicked() && marker_slot > 0 {
            let pos = marker_slot - 1;
            let _ = app.run_heavy("timeline.roll_to", serde_json::json!({ "position": pos }));
            ui.ctx().request_repaint();
        }
        if resp_f.clicked() && marker_slot < total_features {
            let pos = marker_slot + 1;
            let params = if pos < total_features { serde_json::json!({ "position": pos }) } else { serde_json::json!({}) };
            let _ = app.run_heavy("timeline.roll_to", params);
            ui.ctx().request_repaint();
        }
        if resp_e.clicked() {
            let _ = app.run_heavy("timeline.roll_to", serde_json::json!({}));
            ui.ctx().request_repaint();
        }
    }

    let t1_top = total_rect.top() + HEADER_BANNER_HEIGHT;
    let t2_top = t1_top + HEADER_T1_HEIGHT;

    // Tier 1 / Tier 2 separator line
    p.line_segment([pos2(grid_origin_x, t2_top), pos2(grid_right, t2_top)], Stroke::new(1.0, t.border.gamma_multiply(0.4)));
    // Header bottom border
    p.line_segment(
        [pos2(grid_origin_x, total_rect.top() + HEADER_TOTAL_HEIGHT), pos2(grid_right, total_rect.top() + HEADER_TOTAL_HEIGHT)],
        Stroke::new(1.0, t.border),
    );

    let mono_font = FontId::monospace(t.font_small);

    for (col_idx, col) in columns.iter().enumerate() {
        let col_x = grid_origin_x + col_idx as f32 * COL_WIDTH - scroll_x;
        if col_x + COL_WIDTH < grid_origin_x || col_x > grid_right {
            continue;
        }

        // Notch indicator
        let notch_x = col_x + COL_WIDTH / 2.0;
        let is_step_active = col_idx < marker_visual_col;
        let tick_color = if is_step_active { accent.color } else { t.border.gamma_multiply(0.4) };
        p.line_segment([pos2(notch_x, t1_top + 2.0), pos2(notch_x, t1_top + 6.0)], Stroke::new(1.0, tick_color));

        match col {
            VisualColumn::Features { step_number, .. } => {
                let text_color = if is_step_active { accent.color } else { t.text_dim.gamma_multiply(0.6) };

                // Tier 1: Step number
                p.text(
                    pos2(notch_x, t1_top + HEADER_T1_HEIGHT / 2.0 + 2.0),
                    egui::Align2::CENTER_CENTER,
                    format!("{step_number:02}"),
                    mono_font.clone(),
                    text_color,
                );

                let t1_rect = Rect::from_min_size(pos2(col_x, t1_top), vec2(COL_WIDTH, HEADER_T1_HEIGHT));
                let resp = ui.interact(t1_rect, ui.scope_id().with(("step_number_click", col_idx)), Sense::click());
                if resp.clicked() {
                    let target_slot = col
                        .feature_ids()
                        .first()
                        .and_then(|&fid| app.session.doc.features.iter().position(|feat| feat.id == fid))
                        .unwrap_or(*step_number);
                    let _ = app.run_heavy("timeline.roll_to", serde_json::json!({ "position": target_slot }));
                    ui.ctx().request_repaint();
                }
            }
            VisualColumn::FoldedGroup { .. } => {
                // Tier 1: Dash (-) in group column
                p.text(
                    pos2(notch_x, t1_top + HEADER_T1_HEIGHT / 2.0 + 2.0),
                    egui::Align2::CENTER_CENTER,
                    "-",
                    mono_font.clone(),
                    if is_step_active { accent.color } else { t.text_dim.gamma_multiply(0.6) },
                );
            }
        }
    }

    // Tier 2: Open group banners spanning contiguous member columns
    let mut i = 0;
    while i < columns.len() {
        if let VisualColumn::Features { open_group: Some(grp_id), .. } = columns[i] {
            let start_col = i;
            let is_active = start_col < marker_visual_col;
            while i + 1 < columns.len() {
                if let VisualColumn::Features { open_group: Some(next_gid), .. } = columns[i + 1]
                    && next_gid == grp_id
                {
                    i += 1;
                    continue;
                }
                break;
            }
            let end_col = i;
            let start_x = grid_origin_x + start_col as f32 * COL_WIDTH - scroll_x;
            let end_x = grid_origin_x + (end_col + 1) as f32 * COL_WIDTH - scroll_x;

            if end_x > grid_origin_x && start_x < grid_right {
                let banner_rect = Rect::from_min_max(
                    pos2(start_x.max(grid_origin_x) + 3.0, t2_top + 2.0),
                    pos2(end_x.min(grid_right) - 3.0, total_rect.top() + HEADER_TOTAL_HEIGHT - 2.0),
                );
                let resp = ui.interact(banner_rect, ui.scope_id().with(("grp_close_pill", grp_id)), Sense::click());
                if resp.clicked() {
                    state.set_group_folded(grp_id, true);
                }
                let grp_name =
                    app.session.doc.timeline_groups.iter().find(|g| g.id == grp_id).map(|g| g.name.clone()).unwrap_or_else(|| "Group".to_string());
                crate::scenario::publish_handle(&format!("group_pill:{}", grp_name), banner_rect.center());

                let pill_color = if is_active { t.accent.gamma_multiply(0.2) } else { t.text_dim.gamma_multiply(0.2) };
                let stroke_color = if is_active { t.accent.gamma_multiply(0.45) } else { t.border };
                let text_color = if is_active { t.accent } else { t.text_dim };

                let member_count = end_col - start_col + 1;
                let max_title_width = (banner_rect.width() - 22.0).max(0.0);
                let font = FontId::proportional(t.font_small);
                let (displayed_title, is_truncated) = fit_group_banner_title(p, &grp_name, member_count, max_title_width, font.clone(), text_color);

                let tooltip = if is_truncated { format!("{grp_name} ({member_count})\nClick to fold") } else { format!("Click to fold {grp_name}") };
                let resp = resp.on_hover_text(tooltip);

                let gid = grp_id;
                let g_name = grp_name.clone();
                Menu::context(&resp, |ui| {
                    if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_close_group"), "folder").clicked() {
                        state.set_group_folded(gid, true);
                        ui.close();
                    }
                    if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_rename_group"), "rename").clicked() {
                        app.dialog = Some(crate::dialogs::Dialog::rename_group(gid, &g_name));
                        ui.close();
                    }
                    if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_delete_group_expand"), "folder").clicked() {
                        let _ = app.run_heavy("timeline.ungroup", serde_json::json!({ "group": gid }));
                        ui.close();
                    }
                    if crate::context_menu::button(ui, crate::i18n::tr("ui.timeline_2d_delete_group_all"), "delete").clicked() {
                        let contents = crate::timeline_left::folder_features(app, &[gid]);
                        crate::timeline::delete_folders(app, vec![gid], contents);
                        ui.close();
                    }
                });

                p.rect_filled(banner_rect, 4.0, pill_color);
                p.rect_stroke(banner_rect, 4.0, Stroke::new(1.0, stroke_color), StrokeKind::Inside);

                let p_banner = p.with_clip_rect(banner_rect);

                // Folder icon
                let icon_rect = Rect::from_center_size(pos2(banner_rect.left() + 10.0, banner_rect.center().y), vec2(11.0, 11.0));
                if is_active {
                    crate::icons::paint_icon(&p_banner, icon_rect, "folder", 1.0);
                } else {
                    crate::icons::paint_greyscale(&p_banner, icon_rect, "folder", None, 1.0);
                }

                if !displayed_title.is_empty() {
                    let galley = p_banner.layout_no_wrap(displayed_title, font.clone(), text_color);
                    let title_w = galley.size().x;
                    let title_pos = pos2(banner_rect.left() + 18.0, banner_rect.center().y - galley.size().y / 2.0);
                    p_banner.galley(title_pos, galley, text_color);

                    let available_right = banner_rect.width() - (title_w + 24.0);
                    let right_pos = pos2(banner_rect.right() - 6.0, banner_rect.center().y);
                    if available_right > 36.0 {
                        p_banner.text(right_pos, egui::Align2::RIGHT_CENTER, "Close", FontId::proportional(t.font_micro), text_color);
                    }
                }
            }
        }
        i += 1;
    }
}

/// Helper: paints the stationary pinned left header (Component tracks & Transport controls).
fn paint_pinned_left(
    p: &egui::Painter,
    ui: &mut Ui,
    app: &mut SolveApp,
    total_rect: Rect,
    component_ids: &[u64],
    component_names: &BTreeMap<u64, String>,
    scroll_y: f32,
    overscroll_y: f32,
    floating: bool,
    t: &Tokens,
) {
    let pinned_rect = Rect::from_min_max(total_rect.left_top(), pos2(total_rect.left() + PINNED_LEFT_WIDTH, total_rect.bottom()));
    let pinned_rounding = egui::CornerRadius { nw: t.radius_lg, ne: 0, sw: if floating { t.radius_lg } else { 0 }, se: 0 };
    p.rect_filled(pinned_rect, pinned_rounding, t.panel);

    // Vertical seam divider
    p.line_segment([pos2(pinned_rect.right(), total_rect.top()), pos2(pinned_rect.right(), total_rect.bottom())], Stroke::new(1.0, t.border));

    // Top Banner for Pinned Left (COMPONENTS [N])
    let banner_rect = Rect::from_min_size(pinned_rect.left_top(), vec2(PINNED_LEFT_WIDTH, HEADER_BANNER_HEIGHT));
    p.rect_filled(banner_rect, egui::CornerRadius { nw: t.radius_lg, ne: 0, sw: 0, se: 0 }, t.panel);
    p.line_segment([banner_rect.left_bottom(), banner_rect.right_bottom()], Stroke::new(1.0, t.border.gamma_multiply(0.4)));

    // Components Icon + Label + Badge
    let comp_icon_rect = Rect::from_center_size(pos2(banner_rect.left() + 16.0, banner_rect.center().y), vec2(14.0, 14.0));
    crate::icons::paint_icon(p, comp_icon_rect, "component", 1.0);
    p.text(
        pos2(banner_rect.left() + 28.0, banner_rect.center().y),
        egui::Align2::LEFT_CENTER,
        "COMPONENTS",
        FontId::proportional(t.font_small),
        t.text,
    );

    let count_text = component_ids.len().to_string();
    let badge_rect = Rect::from_center_size(pos2(banner_rect.left() + 118.0, banner_rect.center().y), vec2(22.0, 15.0));
    p.rect_filled(badge_rect, 3.0, t.accent.gamma_multiply(0.2));
    p.rect_stroke(badge_rect, 3.0, Stroke::new(1.0, t.accent.gamma_multiply(0.4)), StrokeKind::Inside);
    p.text(badge_rect.center(), egui::Align2::CENTER_CENTER, count_text, FontId::monospace(t.font_badge), t.accent);

    // Leading Transport Header (area between HEADER_BANNER_HEIGHT and HEADER_TOTAL_HEIGHT = 42px)
    let transport_area = Rect::from_min_max(
        pos2(pinned_rect.left(), total_rect.top() + HEADER_BANNER_HEIGHT),
        pos2(pinned_rect.right(), total_rect.top() + HEADER_TOTAL_HEIGHT),
    );
    p.rect_filled(transport_area, 0.0, t.timeline);
    p.line_segment([transport_area.left_bottom(), transport_area.right_bottom()], Stroke::new(1.0, t.border));

    // Transport buttons centered and scaled across the transport_area
    let controls = HistoryTransport::shown(app.preferences.history_play_button);
    let btn_size = 22.0;
    let gap = 4.0;
    let num_controls = controls.len();
    let total_width = num_controls as f32 * btn_size + num_controls.saturating_sub(1) as f32 * gap;
    let mut bx = pinned_rect.left() + (PINNED_LEFT_WIDTH - total_width) / 2.0;
    let by = transport_area.center().y - btn_size / 2.0;

    for &control in controls {
        let b_rect = Rect::from_min_size(pos2(bx, by), vec2(btn_size, btn_size));
        let playing = app.timeline_playback.is_some();
        let resp = control.show(ui, b_rect, playing);
        crate::timeline::transport_response(app, control, &resp);
        bx += btn_size + gap;
    }

    // Component rows
    let rows_clip = Rect::from_min_max(pos2(pinned_rect.left(), total_rect.top() + HEADER_TOTAL_HEIGHT), pinned_rect.right_bottom());
    let row_p = p.with_clip_rect(rows_clip);
    let active_comp_id = app.session.active_component;

    for (r_idx, &cid) in component_ids.iter().enumerate() {
        let ry = total_rect.top() + HEADER_TOTAL_HEIGHT + r_idx as f32 * ROW_HEIGHT - scroll_y + overscroll_y;
        let row_rect = Rect::from_min_size(pos2(pinned_rect.left(), ry), vec2(PINNED_LEFT_WIDTH, ROW_HEIGHT));

        if row_rect.bottom() < rows_clip.top() || row_rect.top() > rows_clip.bottom() {
            continue;
        }

        let is_active = cid == active_comp_id;
        let name = component_names.get(&cid).map(|s| s.as_str()).unwrap_or("Component");

        // Determine component visibility (matches Browser component/occurrence visibility)
        let first_path = app.session.doc.first_path(cid);
        let bodies = crate::browser::bodies_of_component(app, cid);
        let is_visible = if cid != 0 && !first_path.is_empty() {
            !app.session.occurrence_hidden(&first_path)
        } else {
            bodies.is_empty() || bodies.iter().any(|b| !app.session.visibility.hidden_bodies.contains(b))
        };

        let eye_size = vec2(14.0, 14.0);
        let eye_hit_rect = Rect::from_center_size(pos2(row_rect.right() - 15.0, row_rect.center().y), vec2(22.0, ROW_HEIGHT));
        let eye_icon_rect = Rect::from_center_size(eye_hit_rect.center(), eye_size);

        let resp = ui.interact(row_rect, ui.scope_id().with(("comp_pinned_row", cid)), Sense::click());
        let eye_resp = ui.interact(eye_hit_rect, ui.scope_id().with(("comp_pinned_eye", cid)), Sense::click());

        let toggle_visibility = |app: &mut SolveApp| {
            if cid != 0 && !first_path.is_empty() {
                let _ = app.run("browser.visibility", serde_json::json!({ "visible": !is_visible, "paths": [first_path.clone()] }));
            } else if !bodies.is_empty() {
                let _ = app.run("browser.visibility", serde_json::json!({ "items": bodies.clone(), "visible": !is_visible }));
            }
        };

        if eye_resp.clicked() {
            toggle_visibility(app);
        } else if app.preferences.timeline_2d_component_activation && resp.clicked() && !is_active {
            let _ = app.run("component.activate", serde_json::json!({ "component": cid }));
        }

        if eye_resp.hovered() || (resp.hovered() && app.preferences.timeline_2d_component_activation && !is_active) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let eye_tip = crate::i18n::tr(if is_visible { "ui.hide" } else { "ui.show" });
        let _eye_resp = eye_resp.on_hover_text(eye_tip);

        let tip = if is_active {
            format!("{name} (Active Component)")
        } else if app.preferences.timeline_2d_component_activation {
            format!("Click to activate component {name}")
        } else {
            name.to_string()
        };
        let resp = resp.on_hover_text(tip);

        // Right-click context menu for component row (Rename Component, Show/Hide)
        let menu_pos = resp.interact_pointer_pos().unwrap_or(row_rect.left_bottom());
        let can_toggle_vis = (cid != 0 && !first_path.is_empty()) || !bodies.is_empty();
        Menu::context(&resp, |ui| {
            if crate::context_menu::button(ui, crate::i18n::tr_label("Rename Component"), "rename").clicked() {
                crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::Component(cid), menu_pos);
                ui.close();
            }
            let vis_icon = if is_visible { "eye" } else { "suppress" };
            if ui.add_enabled_ui(can_toggle_vis, |ui| crate::context_menu::button(ui, crate::i18n::tr_label("Show/Hide"), vis_icon)).inner.clicked() {
                toggle_visibility(app);
                ui.close();
            }
        });

        // Active / Hover row background
        if is_active {
            row_p.rect_filled(row_rect, 0.0, t.accent.gamma_multiply(0.10));
            // 3px cyan left highlight strip
            row_p.rect_filled(Rect::from_min_size(row_rect.left_top(), vec2(3.0, ROW_HEIGHT)), 0.0, t.accent);
        } else if (resp.hovered() && app.preferences.timeline_2d_component_activation)
            || eye_hit_rect.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(pos2(-100.0, -100.0)))
        {
            row_p.rect_filled(row_rect, 0.0, t.hover);
        }

        // Bottom divider line
        let bottom_line_color = if is_active { t.accent.gamma_multiply(0.25) } else { t.border.gamma_multiply(0.25) };
        row_p.line_segment([row_rect.left_bottom(), row_rect.right_bottom()], Stroke::new(1.0, bottom_line_color));

        // Activation radio indicator (matches SolidCraft browser tree activation radio: empty circle when inactive, centered dot when active)
        let dot_pos = pos2(row_rect.left() + 15.0, row_rect.center().y);
        let radio_stroke_color = if is_active {
            t.accent
        } else if resp.hovered() && app.preferences.timeline_2d_component_activation {
            t.text
        } else {
            t.text_dim
        };
        row_p.circle_stroke(dot_pos, 5.0, Stroke::new(1.2, radio_stroke_color));
        if is_active {
            row_p.circle_filled(dot_pos, 2.8, t.accent);
        }

        // Component name (truncated before the trailing eye icon)
        let text_color = if !is_visible {
            t.text_dim.gamma_multiply(0.55)
        } else if is_active || (resp.hovered() && app.preferences.timeline_2d_component_activation) {
            t.text
        } else {
            t.text_dim
        };
        let text_left = row_rect.left() + 27.0;
        let max_text_width = (eye_hit_rect.left() - 4.0 - text_left).max(10.0);
        let mut job = egui::text::LayoutJob::simple(name.to_string(), FontId::proportional(t.font_small), text_color, max_text_width);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('\u{2026}');
        let galley = row_p.layout_job(job);
        row_p.galley(pos2(text_left, row_rect.center().y - galley.size().y / 2.0), galley, text_color);

        // Trailing eye icon (matches Browser tree row eye)
        let eye_hovered = ui.input(|i| i.pointer.hover_pos()).is_some_and(|pos| eye_hit_rect.contains(pos));
        let eye_opacity = if eye_hovered {
            1.0
        } else if is_visible {
            0.85
        } else {
            0.4
        };
        crate::icons::paint_icon(&row_p, eye_icon_rect, if is_visible { "eye" } else { "suppress" }, eye_opacity);
    }
}

/// Helper: paints an individual action chip on the timeline grid.
fn paint_action_chip(
    p: &egui::Painter,
    rect: Rect,
    f: &Feature,
    is_active: bool,
    is_selected: bool,
    is_prereq: bool,
    is_dependent: bool,
    is_suppressed: bool,
    prereq_color: egui::Color32,
    dependent_color: egui::Color32,
    accent: crate::theme::TabAccent,
    t: &Tokens,
) {
    let stroke_color = if is_selected {
        if is_active { t.text } else { t.text_dim }
    } else if is_prereq {
        if is_active { prereq_color } else { t.text_dim }
    } else if is_dependent {
        if is_active { dependent_color } else { t.border }
    } else if is_suppressed || is_active {
        t.border
    } else {
        t.border.gamma_multiply(0.5)
    };

    let bg_color = if is_selected {
        if is_active { accent.soft } else { t.panel }
    } else if is_suppressed {
        t.panel.gamma_multiply(0.7)
    } else if is_active {
        t.timeline_item
    } else {
        t.panel
    };

    let stroke_width = if is_selected || is_prereq || is_dependent { 1.5 } else { 1.0 };
    p.rect_filled(rect, 4.0, bg_color);
    if is_suppressed {
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
        p.extend(egui::Shape::dashed_line(&corners, Stroke::new(stroke_width, stroke_color), 4.0, 3.0));
    } else {
        p.rect_stroke(rect, 4.0, Stroke::new(stroke_width, stroke_color), StrokeKind::Inside);
    }

    // Master SVG icon from assets/icons/ui/design/svg
    let icon = crate::timeline::icon_of(&f.kind);
    let icon_rect = Rect::from_center_size(rect.center(), vec2(20.0, 20.0));
    let feat_accent = crate::ui_kit::feature_icon_accent(f.kind.type_name(), icon);
    if is_suppressed {
        // Base feature icon shines through underneath
        if is_active {
            crate::icons::paint_accented(p, icon_rect, icon, feat_accent, 0.55);
        } else {
            crate::icons::paint_greyscale(p, icon_rect, icon, feat_accent, 0.55);
        }
        // Slashed eye overlay symbol at 50% opacity as requested
        crate::icons::paint_icon(p, icon_rect, "suppress", 0.5);
    } else if is_active {
        crate::icons::paint_accented(p, icon_rect, icon, feat_accent, 1.0);
    } else {
        crate::icons::paint_greyscale(p, icon_rect, icon, feat_accent, 1.0);
    }
}

/// Helper: calculates the fitted/truncated group banner title and whether it was truncated.
pub fn fit_group_banner_title(
    p: &egui::Painter,
    grp_name: &str,
    member_count: usize,
    max_title_width: f32,
    font: FontId,
    text_color: egui::Color32,
) -> (String, bool) {
    let full_title = format!("{grp_name} ({member_count})");
    let full_w = p.layout_no_wrap(full_title.clone(), font.clone(), text_color).size().x;

    if full_w <= max_title_width {
        (full_title, false)
    } else {
        let count_suffix = format!(" ({member_count})");
        let suffix_with_ellipsis = format!("…{count_suffix}");
        let suffix_w = p.layout_no_wrap(suffix_with_ellipsis.clone(), font.clone(), text_color).size().x;

        if suffix_w <= max_title_width {
            let mut best = suffix_with_ellipsis;
            let chars: Vec<char> = grp_name.chars().collect();
            for len in (1..chars.len()).rev() {
                let prefix: String = chars[..len].iter().collect();
                let candidate = format!("{prefix}…{count_suffix}");
                if p.layout_no_wrap(candidate.clone(), font.clone(), text_color).size().x <= max_title_width {
                    best = candidate;
                    break;
                }
            }
            (best, true)
        } else {
            let mut best = String::new();
            let chars: Vec<char> = grp_name.chars().collect();
            for len in (1..chars.len()).rev() {
                let prefix: String = chars[..len].iter().collect();
                let candidate = format!("{prefix}…");
                if p.layout_no_wrap(candidate.clone(), font.clone(), text_color).size().x <= max_title_width {
                    best = candidate;
                    break;
                }
            }
            (best, true)
        }
    }
}

/// Helper: paints the virtual group chip spanning component tracks.
fn paint_virtual_group_chip(p: &egui::Painter, rect: Rect, group_name: &str, count: usize, is_active: bool, is_selected: bool, t: &Tokens) {
    let border_color = if is_selected { if is_active { t.text } else { t.text_dim } } else { if is_active { t.accent } else { t.border } };

    let bg_color = if is_active { t.timeline_item } else { t.panel };

    p.rect_filled(rect, 5.0, bg_color);
    p.rect_stroke(rect, 5.0, Stroke::new(if is_selected { 1.5 } else { 1.0 }, border_color), StrokeKind::Inside);

    // Integrated header cap (folder icon + expansion chevron indicator)
    let header_cap_rect = Rect::from_min_size(rect.left_top(), vec2(rect.width(), HEADER_T2_HEIGHT - 2.0));
    let cap_bg = if is_active { t.accent.gamma_multiply(0.25) } else { t.text_dim.gamma_multiply(0.2) };
    let cap_stroke = if is_active { t.accent.gamma_multiply(0.5) } else { t.border };
    let cap_rounding = egui::CornerRadius { nw: 4, ne: 4, sw: 2, se: 2 };
    p.rect_filled(header_cap_rect, cap_rounding, cap_bg);
    p.rect_stroke(header_cap_rect, cap_rounding, Stroke::new(1.0, cap_stroke), StrokeKind::Inside);

    // Folder icon
    let folder_rect = Rect::from_center_size(pos2(header_cap_rect.left() + 11.0, header_cap_rect.center().y), vec2(13.0, 13.0));
    if is_active {
        crate::icons::paint_icon(p, folder_rect, "folder", 1.0);
    } else {
        crate::icons::paint_greyscale(p, folder_rect, "folder", None, 1.0);
    }

    // Expansion chevron indicator
    let chev_c = pos2(header_cap_rect.right() - 8.0, header_cap_rect.center().y);
    let chev_col = if is_active { t.accent } else { t.text_dim };
    let s_chev = Stroke::new(1.2, chev_col);
    p.line_segment([pos2(chev_c.x - 2.0, chev_c.y - 3.0), pos2(chev_c.x + 1.5, chev_c.y)], s_chev);
    p.line_segment([pos2(chev_c.x + 1.5, chev_c.y), pos2(chev_c.x - 2.0, chev_c.y + 3.0)], s_chev);

    // Count badge
    let badge_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + HEADER_T2_HEIGHT + 14.0), vec2(18.0, 14.0));
    p.rect_filled(badge_rect, 3.0, if is_active { t.accent.gamma_multiply(0.2) } else { t.text_dim.gamma_multiply(0.15) });
    p.text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        count.to_string(),
        FontId::monospace(t.font_badge),
        if is_active { t.accent } else { t.text },
    );

    // Vertical 90-degree rotated title text
    let title_pos = pos2(rect.center().x, rect.top() + HEADER_T2_HEIGHT + 30.0);
    let title_color = if is_active { t.text } else { t.text_dim };
    let max_v_len = (rect.bottom() - (rect.top() + HEADER_T2_HEIGHT + 36.0)).max(0.0);
    let font = FontId::proportional(t.font_small);
    let full_w = p.layout_no_wrap(group_name.to_string(), font.clone(), title_color).size().x;
    let displayed_name = if full_w <= max_v_len {
        group_name.to_string()
    } else {
        let mut best = String::new();
        let chars: Vec<char> = group_name.chars().collect();
        for len in (1..chars.len()).rev() {
            let candidate = format!("{}…", chars[..len].iter().collect::<String>());
            if p.layout_no_wrap(candidate.clone(), font.clone(), title_color).size().x <= max_v_len {
                best = candidate;
                break;
            }
        }
        best
    };
    if !displayed_name.is_empty() {
        let p_chip = p.with_clip_rect(rect);
        let mut shape = egui::epaint::TextShape::new(title_pos, p_chip.layout_no_wrap(displayed_name, font, title_color), title_color);
        shape.angle = std::f32::consts::FRAC_PI_2;
        p_chip.add(shape);
    }
}

/// Helper: maps a feature ID to its visual column index on the 2D timeline grid.
fn feature_to_visual_col(columns: &[VisualColumn], fid: u64) -> Option<usize> {
    for (idx, col) in columns.iter().enumerate() {
        if col.feature_ids().contains(&fid) {
            return Some(idx);
        }
    }
    None
}

/// Helper: computes the visual corridor `(min_col, max_col)` in visual column indices
/// for the currently selected feature(s), taking into account folded groups.
fn compute_visual_corridor(columns: &[VisualColumn], dep_graph: &DependencyGraph, selected_features: &BTreeSet<u64>) -> Option<(usize, usize)> {
    if selected_features.is_empty() {
        return None;
    }

    let selected_vec: Vec<u64> = selected_features.iter().copied().collect();
    let min_curr = selected_vec.iter().filter_map(|&id| feature_to_visual_col(columns, id)).min()?;
    let max_curr = selected_vec.iter().filter_map(|&id| feature_to_visual_col(columns, id)).max()?;

    let max_prereq_col = selected_vec
        .iter()
        .filter_map(|id| dep_graph.all_prereqs.get(id))
        .flatten()
        .filter(|p| !selected_features.contains(p))
        .filter_map(|&p| feature_to_visual_col(columns, p))
        .max();

    let min_dependent_col = selected_vec
        .iter()
        .filter_map(|id| dep_graph.all_dependents.get(id))
        .flatten()
        .filter(|d| !selected_features.contains(d))
        .filter_map(|&d| feature_to_visual_col(columns, d))
        .min();

    let min_col = max_prereq_col.map(|c| c + 1).unwrap_or(0).min(min_curr);
    let max_col = min_dependent_col.map(|c| c.saturating_sub(1)).unwrap_or(columns.len().saturating_sub(1)).max(max_curr);

    Some((min_col, max_col))
}

/// Helper: paints the movable corridor background tint.
fn paint_movable_corridor_background(
    p: &egui::Painter,
    grid_origin_x: f32,
    scroll_x: f32,
    corridor_top: f32,
    corridor_bottom: f32,
    min_col: usize,
    max_col: usize,
    marker_visual_col: usize,
    t: &Tokens,
) {
    if corridor_bottom <= corridor_top {
        return;
    }

    let x_start = grid_origin_x + min_col as f32 * COL_WIDTH - scroll_x;
    let x_end = grid_origin_x + (max_col + 1) as f32 * COL_WIDTH - scroll_x;
    let marker_x = grid_origin_x + marker_visual_col as f32 * COL_WIDTH - scroll_x;

    if max_col < marker_visual_col {
        let rect = Rect::from_min_max(pos2(x_start, corridor_top), pos2(x_end, corridor_bottom));
        p.rect_filled(rect, 0.0, t.form_success.gamma_multiply(0.08));
    } else if min_col >= marker_visual_col {
        let rect = Rect::from_min_max(pos2(x_start, corridor_top), pos2(x_end, corridor_bottom));
        p.rect_filled(rect, 0.0, t.text_dim.gamma_multiply(0.08));
    } else {
        // Active segment (left of marker)
        let active_rect = Rect::from_min_max(pos2(x_start, corridor_top), pos2(marker_x, corridor_bottom));
        p.rect_filled(active_rect, 0.0, t.form_success.gamma_multiply(0.08));

        // Inactive segment (right of marker)
        let inactive_rect = Rect::from_min_max(pos2(marker_x, corridor_top), pos2(x_end, corridor_bottom));
        p.rect_filled(inactive_rect, 0.0, t.text_dim.gamma_multiply(0.08));
    }
}

/// Helper: paints the movable corridor boundary brackets [ ... ] on top of tracks and marker line.
fn paint_movable_corridor_brackets(
    p: &egui::Painter,
    grid_origin_x: f32,
    scroll_x: f32,
    bracket_top: f32,
    bracket_bottom: f32,
    min_col: usize,
    max_col: usize,
    marker_visual_col: usize,
    t: &Tokens,
) {
    if bracket_bottom <= bracket_top {
        return;
    }

    let x_start = grid_origin_x + min_col as f32 * COL_WIDTH - scroll_x;
    let x_end = grid_origin_x + (max_col + 1) as f32 * COL_WIDTH - scroll_x;
    let marker_x = grid_origin_x + marker_visual_col as f32 * COL_WIDTH - scroll_x;

    let (open_color, close_color) = if max_col < marker_visual_col {
        (t.form_success, t.form_success)
    } else if min_col >= marker_visual_col {
        (t.text_dim, t.text_dim)
    } else {
        (t.form_success, t.text_dim)
    };

    if x_start >= grid_origin_x - 1.0 {
        paint_bracket_open(p, x_start, grid_origin_x, bracket_top, bracket_bottom, open_color, Some(marker_x));
    }
    if x_end >= grid_origin_x {
        paint_bracket_close(p, x_end, bracket_top, bracket_bottom, close_color, Some(marker_x));
    }
}

/// Helper: paints both corridor background and brackets.
#[allow(dead_code)]
fn paint_movable_corridor(
    p: &egui::Painter,
    grid_origin_x: f32,
    scroll_x: f32,
    corridor_top: f32,
    corridor_bottom: f32,
    bracket_top: f32,
    bracket_bottom: f32,
    min_col: usize,
    max_col: usize,
    marker_visual_col: usize,
    t: &Tokens,
) {
    paint_movable_corridor_background(p, grid_origin_x, scroll_x, corridor_top, corridor_bottom, min_col, max_col, marker_visual_col, t);
    paint_movable_corridor_brackets(p, grid_origin_x, scroll_x, bracket_top, bracket_bottom, min_col, max_col, marker_visual_col, t);
}

/// Renders the opening bracket `[` of a movable corridor with smart inset when coincident with the rollback marker.
pub(crate) fn paint_bracket_open(p: &egui::Painter, x: f32, grid_origin_x: f32, top: f32, bottom: f32, color: egui::Color32, marker_x: Option<f32>) {
    let stroke = Stroke::new(2.0, color);

    // Inset from the pinned component layer's border seam if at the boundary
    let (mut bx, tick_len) = if (x - grid_origin_x).abs() <= 1.0 { (grid_origin_x + 1.5, 6.0) } else { (x, 7.0) };

    // Smart Inset: when coincident with the rollback marker, inset bracket 3px into the corridor
    // so both the bracket clasp and the marker line remain 100% visible side-by-side.
    if marker_x.is_some_and(|mx| (bx - mx).abs() <= 2.0) {
        bx += 3.0;
    }

    p.line_segment([pos2(bx + tick_len, top), pos2(bx, top)], stroke);
    p.line_segment([pos2(bx, top - stroke.width / 2.0), pos2(bx, bottom + stroke.width / 2.0)], stroke);
    p.line_segment([pos2(bx, bottom), pos2(bx + tick_len, bottom)], stroke);
}

/// Renders the closing bracket `]` of a movable corridor with smart inset when coincident with the rollback marker.
pub(crate) fn paint_bracket_close(p: &egui::Painter, x: f32, top: f32, bottom: f32, color: egui::Color32, marker_x: Option<f32>) {
    let stroke = Stroke::new(2.0, color);
    let tick_len = 7.0;

    // Smart Inset: when coincident with the rollback marker, inset bracket 3px into the corridor
    // so both the bracket clasp and the marker line remain 100% visible side-by-side.
    let bx = if marker_x.is_some_and(|mx| (x - mx).abs() <= 2.0) { x - 3.0 } else { x };

    p.line_segment([pos2(bx - tick_len, top), pos2(bx, top)], stroke);
    p.line_segment([pos2(bx, top - stroke.width / 2.0), pos2(bx, bottom + stroke.width / 2.0)], stroke);
    p.line_segment([pos2(bx, bottom), pos2(bx - tick_len, bottom)], stroke);
}

/// Paints a vertical dotted connector line between components affected by a multi-component feature.
fn paint_dotted_vertical_line(p: &egui::Painter, x: f32, y_start: f32, y_end: f32, color: egui::Color32, radius: f32) {
    let dot_spacing = 6.0;
    let mut y = y_start;
    while y <= y_end {
        p.circle_filled(pos2(x, y), radius, color);
        y += dot_spacing;
    }
}

/// Returns the sorted, deduplicated list of component IDs affected by a feature.
pub fn feature_affected_components(doc: &Document, model: Option<&Model>, f: &Feature) -> Vec<u64> {
    let mut comps = BTreeSet::new();
    comps.insert(f.component);

    // 1. If evaluation results exist in model, compare state before and after this feature
    if let Some(m) = model
        && let Some(res) = m.result(f.id)
    {
        let before = m.state_before(f.id);
        let after = &res.state;

        for b_before in &before.bodies {
            if let Some(b_after) = after.body(&b_before.name) {
                if b_before.geometry_key() != b_after.geometry_key() || b_before.component != b_after.component {
                    comps.insert(doc.model_body_component(b_before));
                    comps.insert(doc.model_body_component(b_after));
                }
            } else {
                comps.insert(doc.model_body_component(b_before));
            }
        }

        for b_after in &after.bodies {
            if b_after.feature == f.id || before.body(&b_after.name).is_none() {
                comps.insert(doc.model_body_component(b_after));
            }
        }
    }

    // 2. Edge name references (e.g. F1:top|F4:side in fillets and chamfers)
    for name in &f.edge_names {
        for part in name.split(|c: char| !c.is_ascii_alphanumeric()) {
            if let Some(id_str) = part.strip_prefix('F')
                && let Ok(fid) = id_str.parse::<u64>()
                && let Some(feat) = doc.feature(fid)
            {
                comps.insert(feat.component);
            }
        }
    }

    let model_state = model.map(|m| m.state());
    let model_bodies = model_state.as_deref().map(|s| s.bodies.as_slice());

    let resolve_body = |b: &str| -> Option<u64> {
        let trimmed = b.trim();
        if trimmed.is_empty() || trimmed == "*" {
            return None;
        }
        if let Some(bodies) = model_bodies
            && let Some(mb) = bodies.iter().find(|body| body.name.trim() == trimmed)
        {
            return Some(doc.model_body_component(mb));
        }
        if let Some(&cid) = doc.body_components.get(trimmed) {
            return Some(cid);
        }
        for prev in &doc.features {
            if prev.body_names.iter().any(|bn| bn.trim() == trimmed) || prev.name.trim() == trimmed {
                return Some(prev.component);
            }
        }
        Some(doc.body_component(trimmed, f.id))
    };

    for p in &f.participants {
        if let Some(cid) = resolve_body(p) {
            comps.insert(cid);
        }
    }

    match &f.kind {
        FeatureKind::Combine { target, tools, .. } => {
            if let Some(cid) = resolve_body(target.as_ref()) {
                comps.insert(cid);
            }
            for tool in tools {
                if let Some(cid) = resolve_body(tool.as_ref()) {
                    comps.insert(cid);
                }
            }
        }
        FeatureKind::BodyTransfer { from, to, bodies, .. } => {
            comps.insert(*from);
            comps.insert(*to);
            for b in bodies {
                if let Some(cid) = resolve_body(b.as_ref()) {
                    comps.insert(cid);
                }
            }
        }
        FeatureKind::Extrude { targets, .. }
        | FeatureKind::Revolve { targets, .. }
        | FeatureKind::Loft { targets, .. }
        | FeatureKind::Sweep { targets, .. }
        | FeatureKind::Emboss { targets, .. } => {
            for t in targets {
                if let Some(cid) = resolve_body(t.as_ref()) {
                    comps.insert(cid);
                }
            }
        }
        FeatureKind::Fillet { body, edges, .. } | FeatureKind::Chamfer { body, edges, .. } => {
            if let Some(b) = body
                && let Some(cid) = resolve_body(b.as_ref())
            {
                comps.insert(cid);
            }
            if let Some(bodies) = model_bodies {
                for p in edges {
                    for b in bodies {
                        let mesh = b.mesh();
                        let is_near = mesh.edges.iter().any(|e| e.windows(2).any(|w| p.0.dist_to_segment(w[0], w[1]) < 0.05));
                        if is_near {
                            comps.insert(doc.model_body_component(b));
                        }
                    }
                }
            }
        }
        FeatureKind::Split { body: b, .. } => {
            if let Some(cid) = resolve_body(b.as_ref()) {
                comps.insert(cid);
            }
        }
        FeatureKind::Move { bodies, .. }
        | FeatureKind::Scale { bodies, .. }
        | FeatureKind::Align { bodies, .. }
        | FeatureKind::Remove { bodies, .. } => {
            for b in bodies {
                if let Some(cid) = resolve_body(b.as_ref()) {
                    comps.insert(cid);
                }
            }
        }
        FeatureKind::BoundaryFill { tools, .. } => {
            for t in tools {
                if let Some(cid) = resolve_body(t.as_ref()) {
                    comps.insert(cid);
                }
            }
        }
        FeatureKind::Pattern { bodies, features, .. } => {
            for b in bodies {
                if let Some(cid) = resolve_body(b.as_ref()) {
                    comps.insert(cid);
                }
            }
            for &fid in features {
                if let Some(feat) = doc.feature(fid.0) {
                    comps.insert(feat.component);
                }
            }
        }
        FeatureKind::Mirror { bodies, features, placements, .. } => {
            for b in bodies {
                if let Some(cid) = resolve_body(b.as_ref()) {
                    comps.insert(cid);
                }
            }
            for &fid in features {
                if let Some(feat) = doc.feature(fid.0) {
                    comps.insert(feat.component);
                }
            }
            for path in placements {
                if let Some(&cid) = path.last() {
                    comps.insert(cid);
                }
            }
        }
        _ => {}
    }

    comps.into_iter().collect()
}

/// Helper to determine whether a grid cell (col_idx, track_idx) is occupied by an action chip,
/// a multi-component vertical connector, or a folded group card.
fn cell_is_occupied(
    col_idx: usize,
    track_idx: usize,
    visual_columns: &[VisualColumn],
    feature_components_map: &BTreeMap<u64, Vec<u64>>,
    component_ids: &[u64],
    doc: &solidcraft_engine::doc::Document,
) -> bool {
    let Some(col) = visual_columns.get(col_idx) else {
        return false;
    };
    match col {
        VisualColumn::Features { features, .. } => {
            let mut tracks: Vec<usize> = Vec::new();
            for &fid in features {
                if let Some(f) = doc.feature(fid) {
                    let affected = feature_components_map.get(&fid).cloned().unwrap_or_else(|| vec![f.component]);
                    for &cid in &affected {
                        if let Some(pos) = component_ids.iter().position(|&id| id == cid) {
                            tracks.push(pos);
                        }
                    }
                }
            }
            tracks.sort_unstable();
            tracks.dedup();
            if tracks.contains(&track_idx) {
                return true;
            }
            if tracks.len() > 1 {
                let min_t = tracks.first().copied().unwrap_or(0);
                let max_t = tracks.last().copied().unwrap_or(0);
                if track_idx >= min_t && track_idx <= max_t {
                    return true;
                }
            }
            false
        }
        VisualColumn::FoldedGroup { features, .. } => {
            let mut tracks: Vec<usize> = Vec::new();
            for &fid in features {
                if let Some(f) = doc.feature(fid) {
                    let affected = feature_components_map.get(&fid).cloned().unwrap_or_else(|| vec![f.component]);
                    for &cid in &affected {
                        if let Some(pos) = component_ids.iter().position(|&id| id == cid) {
                            tracks.push(pos);
                        }
                    }
                }
            }
            tracks.sort_unstable();
            tracks.dedup();
            let min_t = tracks.first().copied().unwrap_or(0);
            let max_t = tracks.last().copied().unwrap_or(0);
            track_idx >= min_t && track_idx <= max_t
        }
    }
}
