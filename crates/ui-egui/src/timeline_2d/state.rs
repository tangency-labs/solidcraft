//! Interactive state, scrolling, multi-selection, and drag models for the 2D Timeline.

use egui::Pos2;
use std::collections::BTreeSet;

/// Dragging state for interactive timeline gestures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragTarget {
    /// Scrubbing the rollback marker head pin across timeline columns.
    Marker,
    /// Scrubbing the assembly rollback marker head pin across joint columns.
    AssemblyMarker,
    /// Dragging the vertical splitter bar between modeling and assembly sections.
    Splitter { start_x: f32, start_ratio: f32 },
    /// Dragging a feature action chip horizontally for ripple push reordering.
    Feature { id: u64, start_col: usize, track_idx: usize, current_pos: Pos2 },
    /// Dragging a virtual group chip.
    Group { id: u64, start_slot: usize },
    /// Dragging an assembly joint action chip.
    Joint { id: u64, start_col: usize, current_pos: Pos2 },
}

/// Persistent interaction and layout state for the 2D Timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline2DState {
    /// Horizontal scroll offset in pixels for modeling track.
    pub scroll_x: f32,
    /// Vertical scroll offset in pixels (shared 1:1 between pinned headers, modeling, and assembly).
    pub scroll_y: f32,
    /// Elastic vertical overscroll displacement for rubberband spring physics.
    pub overscroll_y: f32,
    /// IDs of timeline groups that are currently collapsed/folded.
    pub folded_groups: BTreeSet<u64>,
    /// Set of selected feature IDs.
    pub selected_features: BTreeSet<u64>,
    /// Anchor feature ID for Shift+Click range selections.
    pub selection_anchor: Option<u64>,
    /// Active drag gesture, if any.
    pub drag: Option<DragTarget>,
    /// Last hovered feature ID.
    pub hovered_feature: Option<u64>,
    /// Last hovered group ID.
    pub hovered_group: Option<u64>,
    /// Horizontal scroll offset in pixels for assembly / post-modeling section.
    pub assembly_scroll_x: f32,
    /// Width ratio for the modeling section when split view is active (0.0 to 1.0, default 0.65).
    pub modeling_split_ratio: f32,
    /// Whether the geometric modeling section is collapsed into the dock tab.
    pub is_modeling_collapsed: bool,
    /// Whether the assembly / post-modeling section is collapsed into the dock tab.
    pub is_assembly_collapsed: bool,
    /// Selected joint IDs in the post-modeling section.
    pub selected_joints: BTreeSet<u64>,
    /// The joint clicked last; its outlines and corridor show while it stays selected.
    pub joint_anchor: Option<u64>,
    /// Assembly rollback marker step (number of active joints, None = all active).
    pub assembly_marker_step: Option<usize>,
    /// Feature IDs that were explicitly/separately suppressed by the user.
    pub separately_suppressed_features: BTreeSet<u64>,
}

impl Default for Timeline2DState {
    /// Creates a default `Timeline2DState` with initial 65/35 modeling/assembly split and uncollapsed tracks.
    fn default() -> Self {
        Self {
            scroll_x: 0.0,
            scroll_y: 0.0,
            overscroll_y: 0.0,
            folded_groups: BTreeSet::new(),
            selected_features: BTreeSet::new(),
            selection_anchor: None,
            drag: None,
            hovered_feature: None,
            hovered_group: None,
            assembly_scroll_x: 0.0,
            modeling_split_ratio: 0.65,
            is_modeling_collapsed: false,
            is_assembly_collapsed: false,
            selected_joints: BTreeSet::new(),
            joint_anchor: None,
            assembly_marker_step: None,
            separately_suppressed_features: BTreeSet::new(),
        }
    }
}

pub const STATE_ID: &str = "sc_timeline_2d_state";

impl Timeline2DState {
    /// Computes the storage ID scoped to a specific design UID.
    pub fn state_id(doc_uid: Option<u64>) -> egui::Id {
        match doc_uid {
            Some(uid) => egui::Id::unique(STATE_ID).with(uid),
            None => egui::Id::unique(STATE_ID),
        }
    }

    /// Loads state from the egui Context temp storage for a specific design UID.
    pub fn load_scoped(ctx: &egui::Context, doc_uid: Option<u64>) -> Self {
        ctx.data(|d| d.get_temp(Self::state_id(doc_uid))).unwrap_or_default()
    }

    /// Stores state into the egui Context temp storage for a specific design UID.
    pub fn store_scoped(&self, ctx: &egui::Context, doc_uid: Option<u64>) {
        ctx.data_mut(|d| d.insert_temp(Self::state_id(doc_uid), self.clone()));
    }

    /// Loads state from the egui Context temp storage.
    pub fn load(ctx: &egui::Context) -> Self {
        Self::load_scoped(ctx, None)
    }

    /// Stores state into the egui Context temp storage.
    pub fn store(&self, ctx: &egui::Context) {
        self.store_scoped(ctx, None);
    }

    /// Checks if a group is folded (closed).
    pub fn is_group_folded(&self, group_id: u64) -> bool {
        self.folded_groups.contains(&group_id)
    }

    /// Toggles the folded state of a group.
    pub fn toggle_group_fold(&mut self, group_id: u64) {
        if self.folded_groups.contains(&group_id) {
            self.folded_groups.remove(&group_id);
        } else {
            self.folded_groups.insert(group_id);
        }
    }

    /// Sets the folded state of a group.
    pub fn set_group_folded(&mut self, group_id: u64, folded: bool) {
        if folded {
            self.folded_groups.insert(group_id);
        } else {
            self.folded_groups.remove(&group_id);
        }
    }

    /// Returns true if `id` is currently selected.
    pub fn is_selected(&self, id: u64) -> bool {
        self.selected_features.contains(&id)
    }

    /// Checks if a feature was explicitly/separately suppressed by the user.
    pub fn is_separately_suppressed(&self, id: u64) -> bool {
        self.separately_suppressed_features.contains(&id)
    }

    /// Marks a feature as separately suppressed.
    pub fn mark_separately_suppressed(&mut self, id: u64) {
        self.separately_suppressed_features.insert(id);
    }

    /// Unmarks a feature as separately suppressed.
    pub fn unmark_separately_suppressed(&mut self, id: u64) {
        self.separately_suppressed_features.remove(&id);
    }

    /// The feature whose prerequisites, dependents and corridor show: the one clicked last while it
    /// stays selected, otherwise the first selected one.
    pub fn focused_feature(&self) -> Option<u64> {
        Self::focus(self.selection_anchor, &self.selected_features)
    }

    /// The joint whose prerequisites, dependents and corridor show, chosen as for features.
    pub fn focused_joint(&self) -> Option<u64> {
        Self::focus(self.joint_anchor, &self.selected_joints)
    }

    /// Moves the assembly marker to just before joint `step`. At or past the last joint it is
    /// stored as `None`, the end, so joints added later stay active.
    pub fn set_assembly_marker(&mut self, step: usize, num_joints: usize) {
        self.assembly_marker_step = (step < num_joints).then_some(step);
    }

    fn focus(anchor: Option<u64>, selected: &BTreeSet<u64>) -> Option<u64> {
        anchor.filter(|id| selected.contains(id)).or_else(|| selected.iter().next().copied())
    }

    /// Right-click on a chip. A chip outside the selection replaces it, so the menu that opens
    /// acts on that chip alone and offers Rename. A chip inside the selection keeps it whole.
    pub fn select_for_context_menu(&mut self, id: u64) {
        if !self.selected_features.contains(&id) {
            self.select_single(id);
        }
    }

    /// Clears selection and selects a single feature.
    pub fn select_single(&mut self, id: u64) {
        self.selected_features.clear();
        self.selected_features.insert(id);
        self.selection_anchor = Some(id);
    }

    /// Toggles selection of a single feature (Cmd/Ctrl+Click).
    pub fn toggle_selection(&mut self, id: u64) {
        if self.selected_features.contains(&id) {
            self.selected_features.remove(&id);
        } else {
            self.selected_features.insert(id);
        }
        self.selection_anchor = Some(id);
    }

    /// Extends selection across a range in timeline order (Shift+Click).
    pub fn select_range(&mut self, timeline_order: &[u64], to_id: u64, add: bool) {
        let Some(to_idx) = timeline_order.iter().position(|&f| f == to_id) else {
            return;
        };
        let from_idx = match self.selection_anchor.and_then(|anchor| timeline_order.iter().position(|&f| f == anchor)) {
            Some(idx) => idx,
            None => {
                self.selection_anchor = Some(to_id);
                to_idx
            }
        };
        let min_idx = from_idx.min(to_idx);
        let max_idx = from_idx.max(to_idx);

        if !add {
            self.selected_features.clear();
        }
        for &id in &timeline_order[min_idx..=max_idx] {
            self.selected_features.insert(id);
        }
    }

    /// Checks if the current selection set is contiguous in timeline order.
    pub fn is_selection_adjacent(&self, timeline_order: &[u64]) -> bool {
        if self.selected_features.len() < 2 {
            return false;
        }
        let mut indices: Vec<usize> = self.selected_features.iter().filter_map(|id| timeline_order.iter().position(|f| f == id)).collect();
        indices.sort_unstable();

        for w in indices.windows(2) {
            if w[1] != w[0] + 1 {
                return false;
            }
        }
        true
    }

    /// The hover tooltip of a group card: its name and size, its steps, whether it is rolled back,
    /// its features and how to open it, in the reader's language.
    pub fn format_group_tooltip(
        group_name: &str,
        features: &[(usize, &str, &str)], // (step_1_based, name, component_name)
        is_active: bool,
    ) -> String {
        use crate::i18n::{format, tr};
        let status = tr(if is_active { "ui.timeline_2d_group_tip_active" } else { "ui.timeline_2d_group_tip_inactive" });
        let count = features.len().to_string();
        let step_range = match features {
            [] => tr("ui.timeline_2d_group_tip_no_steps").to_string(),
            [(step, ..)] => format("ui.timeline_2d_group_tip_step", &[("step", &format!("{step:02}"))]),
            [(first, ..), .., (last, ..)] => {
                format("ui.timeline_2d_group_tip_steps", &[("first", &format!("{first:02}")), ("last", &format!("{last:02}"))])
            }
        };
        let title = format("ui.timeline_2d_group_tip_title", &[("name", group_name), ("count", &count)]);
        let mut tip = format!("{title}\n{step_range} • {status}\n\n{}\n", tr("ui.timeline_2d_group_tip_features"));
        for (step, name, comp) in features {
            tip.push_str(&format!("  • [{step:02}] {name} ({comp})\n"));
        }
        tip.push('\n');
        tip.push_str(tr("ui.timeline_2d_group_tip_hint"));
        tip
    }
}
