//! Presentation of Dimension → Named → Key. All document edits use existing commands.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const USED: usize = 7;
pub(super) const MEASURES: usize = 8;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Named,
    Dimensions,
}

#[derive(Default)]
pub(super) struct State {
    pub view: View,
    restored: bool,
    pub narrow: bool,
    pub header_width: f32,
    pub ranges: bool,
    /// Name, star after naming, editor has started. No mutation until confirmation.
    pub naming: Option<(String, bool, bool)>,
    pub demote: Option<String>,
    pub selected: BTreeSet<String>,
    anchor: Option<String>,
    pub selected_group: Option<String>,
    ordered_group: Option<String>,
    pub reveal_group: bool,
    nudge_dismissed: bool,
    prefix: String,
    /// Original key expressions while the compact editor is open.
    pub compact: Option<BTreeMap<String, String>>,
    toast: Option<(u64, String)>,
}

fn tr(id: &'static str) -> &'static str {
    solidcraft_ui_text::tr(id)
}
fn view_id() -> egui::Id {
    egui::Id::unique("sc_parameters_view")
}

pub(super) fn remembered_view(ctx: &egui::Context) -> View {
    if ctx.data_mut(|m| m.get_persisted::<bool>(view_id())).unwrap_or(false) { View::Dimensions } else { View::Named }
}

pub(super) fn named(r: &Row) -> bool {
    r.source == "user" || !auto_name(&r.name)
}

fn auto_name(name: &str) -> bool {
    name.strip_prefix('d').is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}

impl State {
    pub fn start_name(&mut self, row: &Row, star: bool) {
        self.naming = Some((row.name.clone(), star, false));
    }
}

/// Graph edges are [reader, read]. Deduplicate in one pass, including feature readers.
pub(super) fn readers(graph: &Value) -> BTreeMap<String, Vec<String>> {
    let mut reverse: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in graph.get("edges").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(reader), Some(read)) = (edge.get(0).and_then(Value::as_str), edge.get(1).and_then(Value::as_str)) {
            reverse.entry(read.to_owned()).or_default().insert(reader.to_owned());
        }
    }
    reverse.into_iter().map(|(name, readers)| (name, readers.into_iter().collect())).collect()
}

/// Share the revision's evaluated rows with rendering and completed asynchronous edits.
pub(super) fn listed(app: &mut SolveApp, d: &mut ParamsDialog) -> Arc<Vec<Row>> {
    if d.listed.as_ref().is_none_or(|(rev, _)| *rev != app.session.revision) {
        let mut rows = rows(app);
        let graph = app.session.execute("parameters.graph", &json!({})).unwrap_or_default();
        let mut readers = readers(&graph);
        let owners: BTreeMap<_, _> = rows.iter().filter(|r| !r.group.is_empty()).map(|r| (r.name.clone(), r.group.clone())).collect();
        for row in &mut rows {
            row.readers.extend(readers.remove(&row.name).unwrap_or_default());
            row.readers.sort();
            row.readers.dedup();
            row.reader_features = row
                .readers
                .iter()
                .filter_map(|reader| reader.strip_prefix("Feature:").or_else(|| owners.get(reader).map(String::as_str)))
                .collect::<BTreeSet<_>>()
                .len();
        }
        d.listed = Some((app.session.revision, Arc::new(rows)));
        d.lists += 1;
    }
    let all = d.listed.as_ref().map(|(_, rows)| rows.clone()).unwrap_or_default();
    if app.computing_since().is_none() && app.deferred_queue.is_empty() {
        for name in std::mem::take(&mut d.applying).into_keys() {
            if let Some(entry) = d.applied.get_mut(&name)
                && let Some(now) = all.iter().find(|r| r.name == name)
            {
                entry.1 = now.expression.clone();
            }
        }
    }
    all
}

pub(super) fn measure(sketch: &solidcraft_engine::sketch::Sketch, kind: &solidcraft_engine::sketch::ConstraintKind) -> String {
    use solidcraft_engine::sketch::ConstraintKind as C;
    let curve = |index: usize| sketch.curves.get(index).map(|c| c.id.clone()).unwrap_or_default();
    let point = |index: usize| sketch.points.get(index).map(|p| p.id.clone()).unwrap_or_default();
    let (label, entities) = match kind {
        C::Length { l, .. } => ("parameters.redesign.length", curve(*l)),
        C::Distance { p, q, .. } => ("parameters.redesign.distance", format!("{}, {}", point(*p), point(*q))),
        C::DistanceX { p, q, .. } => ("parameters.redesign.distance_x", format!("{}, {}", point(*p), point(*q))),
        C::DistanceY { p, q, .. } => ("parameters.redesign.distance_y", format!("{}, {}", point(*p), point(*q))),
        C::Radius { c, .. } => ("parameters.redesign.radius", curve(*c)),
        C::Diameter { c, .. } => ("parameters.redesign.diameter", curve(*c)),
        C::LineAngle { l: c, .. } | C::ArcSweep { c, .. } => ("parameters.redesign.angle", curve(*c)),
        C::Angle { a, b, .. } => ("parameters.redesign.angle", format!("{}, {}", curve(*a), curve(*b))),
        C::PointLineDistance { p, l: c, .. } | C::PointCircleDistance { p, c, .. } => {
            ("parameters.redesign.distance", format!("{}, {}", point(*p), curve(*c)))
        }
        C::LineCircleDistance { l: a, c: b, .. } | C::CircleCircleDistance { a, b, .. } | C::SlotOverallLength { a, b, .. } => {
            ("parameters.redesign.distance", format!("{}, {}", curve(*a), curve(*b)))
        }
        _ => ("parameters.redesign.dimension", String::new()),
    };
    if entities.is_empty() {
        tr(label).to_owned()
    } else {
        solidcraft_ui_text::format("parameters.redesign.measure", &[("kind", tr(label)), ("entities", &entities)])
    }
}

pub(super) fn column_order(d: &ParamsDialog) -> &'static [usize] {
    if d.redesign.view == View::Dimensions {
        &[STAR, NAME, MEASURES, EXPRESSION, VALUE, ACTIONS]
    } else if d.redesign.narrow {
        &[STAR, NAME, EXPRESSION, VALUE, ACTIONS]
    } else {
        &[STAR, NAME, EXPRESSION, VALUE, UNIT, COMMENT, USED, ACTIONS]
    }
}

pub(super) fn columns(name_floor: f32, unit_width: f32, order: &[usize]) -> Vec<ui_kit::GridColumn> {
    let mut all = parameter_columns(name_floor, unit_width);
    if order.contains(&MEASURES)
        && let Some(actions) = all.get_mut(ACTIONS)
    {
        *actions = ui_kit::GridColumn::new("actions", "Actions", 0.0, ui_kit::CellKind::Custom)
            .icon_only()
            .min_content_width(solidcraft_theme::Tokens::get().form_label_width * 1.5);
    }
    all.push(ui_kit::GridColumn::new("used_by", tr("parameters.redesign.used_by"), 0.5, ui_kit::CellKind::Custom));
    all.push(ui_kit::GridColumn::new("measures", tr("parameters.redesign.measures"), 1.5, ui_kit::CellKind::ReadOnly).flex());
    order.iter().filter_map(|index| all.get(*index).cloned()).collect()
}

/// Keep edit processing in its original semantic columns when the view hides/reorders them.
pub(super) fn project_output(mut out: ui_kit::GridOutput, order: &[usize]) -> ui_kit::GridOutput {
    let col = |c: usize| order.get(c).copied().unwrap_or(usize::MAX);
    for edit in &mut out.edits {
        match edit {
            ui_kit::Edit::Set { col: c, .. } | ui_kit::Edit::Clear { col: c, .. } => *c = col(*c),
        }
    }
    if let Some((_, c, _)) = &mut out.editing {
        *c = col(*c);
    }
    if let Some((_, c)) = &mut out.cancelled {
        *c = col(*c);
    }
    out
}

pub(super) fn toolbar(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, all: &[Row]) {
    // Prune against this revision before a header action can rename the selection.
    d.redesign.selected.retain(|name| all.iter().any(|r| r.name == *name));
    if !d.redesign.restored {
        d.redesign.view = remembered_view(ui.ctx());
        d.redesign.restored = true;
    }
    let filter = d.filter.to_ascii_lowercase();
    let counts = all.iter().filter(|r| r.matches(&filter)).fold([0usize; 2], |mut counts, r| {
        counts[usize::from(!named(r))] += 1;
        counts
    });
    let mut selected = usize::from(d.redesign.view == View::Dimensions);
    let changed = header(app, ui, d, all, &counts, &mut selected);
    if changed && finish_view_edit(app, ui.ctx(), d, all) {
        d.redesign.view = if selected == 0 { View::Named } else { View::Dimensions };
        d.reset_scroll = true;
        d.redesign.naming = None;
        d.renaming = None;
        d.redesign.selected.clear();
        d.forget_grids(ui.ctx());
    }
    ui.ctx().data_mut(|m| m.insert_persisted(view_id(), d.redesign.view == View::Dimensions));
    crate::scenario::publish_count("params:named:count", counts[0] as f64);
    crate::scenario::publish_count("params:dimensions:count", counts[1] as f64);
    if d.redesign.view == View::Dimensions {
        ui_kit::text(ui, tr("parameters.redesign.dimensions_hint"), ui_kit::Text::Hint("dimensions_hint"));
    } else if !d.redesign.nudge_dismissed {
        let candidates: Vec<_> = all.iter().filter(|r| !named(r) && r.reader_features > 1).map(|r| r.name.as_str()).take(2).collect();
        if !candidates.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let text = solidcraft_ui_text::format("parameters.redesign.nudge", &[("names", &candidates.join(", "))]);
                ui_kit::text(ui, &text, ui_kit::Text::Hint("naming_nudge"));
                if ui_kit::button(ui, tr("parameters.redesign.review"), ui_kit::Button::Normal).clicked() {
                    d.redesign.view = View::Dimensions;
                }
                if ui_kit::button(ui, tr("parameters.redesign.dismiss"), ui_kit::Button::Normal).clicked() {
                    d.redesign.nudge_dismissed = true;
                }
            });
        }
    }
}

fn header(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, all: &[Row], counts: &[usize], selected: &mut usize) -> bool {
    let labels = [tr("parameters.redesign.named"), tr("parameters.redesign.dimensions")];
    ui.horizontal(|ui| {
        let t = solidcraft_theme::Tokens::get();
        ui.spacing_mut().item_spacing.x = t.space_6;
        let narrow = ui.available_width() < t.control_height * 20.0;
        let pages = ui_kit::PageSwitch::new("parameter_pages", &labels).counts(counts).targets(&["params:view:named", "params:view:dimensions"]);
        let actions = [
            ("parameters.redesign.export_json", "export_tray", "json"),
            ("parameters.redesign.export_csv", "export_tray", "csv"),
            ("parameters.redesign.import", "import_tray", ""),
        ];
        let search_width = if narrow { t.space_24 * 5.0 + t.space_10 } else { t.space_24 * 8.0 + t.space_8 };
        let labelled_width: f32 = actions.iter().map(|(label, icon, _)| ui_kit::icon_label_width(ui, tr(label), Some(icon), false)).sum();
        let gaps = t.space_6 * 6.0 + t.form_close_size;
        let natural_page_width = pages.width(ui);
        let header_width = natural_page_width + labelled_width + t.space_24 * 8.0 + t.space_8 + gaps;
        if d.redesign.header_width != header_width {
            d.redesign.header_width = header_width;
            ui.ctx().request_repaint();
        }
        let icons_only = narrow || natural_page_width + labelled_width + search_width + gaps > ui.available_width();
        let actions_width = if icons_only { t.control_height * actions.len() as f32 } else { labelled_width };
        let page_width = natural_page_width.min((ui.available_width() - search_width - actions_width - gaps).max(t.space_24 * 3.0));
        let changed = ui.allocate_ui(egui::vec2(page_width, t.control_height + t.space_2), |ui| pages.show(ui, selected)).inner;
        let more = ui_kit::compact_button(ui, "…", tr("parameters.redesign.actions"), true, false);
        crate::scenario::publish_target("params:header:actions", more.rect);
        ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Popup).menu(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
            |ui| {
                let ranges = ui_kit::checkbox(ui, tr("parameters.redesign.cell_ranges"), &mut d.redesign.ranges);
                crate::scenario::publish_target("params:cell_ranges", ranges.rect);
                if ranges.changed() {
                    d.forget_grids(ui.ctx());
                }
                group_checkbox(app, ui);
                if d.redesign.selected.len() >= 2 {
                    ui_kit::divider(ui, "bulk_naming");
                    let selected: Vec<_> = all.iter().filter(|r| d.redesign.selected.contains(&r.name)).collect();
                    name_prefix(app, ui, d, &selected);
                }
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = t.space_6;
            ui.allocate_ui(egui::vec2(search_width, t.control_height), |ui| {
                let id = ui.make_persistent_id("parameter_filter");
                let response = ui_kit::search_field(ui, id, &mut d.filter, solidcraft_ui_text::tr_label("Filter"));
                crate::scenario::publish_target("dialog:Filter", response.rect);
            });
            for (label, icon, ext) in actions {
                let label = tr(label);
                let response = if icons_only {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(t.control_height, t.control_height), egui::Sense::hover());
                    ui_kit::icon_button_at(ui, rect, icon, label)
                } else {
                    ui_kit::icon_label_button(ui, label, icon, true, false)
                };
                let key = if ext.is_empty() { "params:import".to_owned() } else { format!("params:export_{ext}") };
                crate::scenario::publish_target(&key, response.rect);
                if response.backing_on_hover_text(label).clicked() {
                    if ext.is_empty() {
                        app.pick_open_command("parameters.import", json!({}), IMPORT_TYPES);
                    } else {
                        app.pick_save_command(&format!("parameters.{ext}"), &[ext], "parameters.export", json!({"format":ext}));
                    }
                }
            }
        });
        changed
    })
    .inner
}

/// Switching tables blurs the previous grid. Commit live fields first; held fields stay held.
fn finish_view_edit(app: &mut SolveApp, ctx: &egui::Context, d: &mut ParamsDialog, all: &[Row]) -> bool {
    let Some((_, name, col, _)) = d.typing.clone() else { return true };
    let Some(row) = all.iter().find(|r| r.name == name) else { return true };
    let draft = match col {
        EXPRESSION => d.editing.get(&name),
        COMMENT => d.comments.get(&name),
        _ => None,
    }
    .cloned();
    let Some(text) = draft else { return true };
    let unchanged = text == if col == COMMENT { &row.comment } else { &row.expression }.as_str();
    if unchanged || (col == EXPRESSION && !app.preferences.parameters_apply_live) {
        d.typing = None;
        return true;
    }
    if col == EXPRESSION
        && app.preferences.parameters_apply_live
        && let Err(error) = check(app, &text, &row.unit)
    {
        d.error = Some(error);
        return false;
    }
    edit_cell(app, ctx, d, row, "view_switch", col, text);
    let valid = d.error.is_none();
    if valid {
        d.typing = None;
    }
    valid
}

pub(super) fn follow_selection(app: &SolveApp, d: &mut ParamsDialog, sections: &mut Sections<'_>) {
    use solidcraft_engine::Sel;
    let selected_id = crate::timeline::selected_feature_ids(app).last().copied().or_else(|| {
        app.session.selection.iter().rev().find_map(|selection| match selection {
            Sel::Profile { sketch, .. } | Sel::ExternalSketchCurve { sketch, .. } | Sel::ExternalSketchPoint { sketch, .. } => Some(*sketch),
            Sel::SketchCurve { sketch, .. } => sketch.or(app.session.active_sketch),
            Sel::SketchPoint { .. } | Sel::SketchConstraint { .. } | Sel::SketchMidpoint { .. } => app.session.active_sketch,
            _ => None,
        })
    });
    let selected = selected_id.and_then(|id| app.session.doc.feature(id)).map(|f| f.name.clone());
    if let Some(group) = &selected
        && d.redesign.selected_group.as_ref() != Some(group)
    {
        d.bands.insert(format!("model:{group}"), true);
        // The selected model band must remain reachable inside component folders.
        let mut component = selected_id.and_then(|id| app.session.doc.feature(id)).map_or(0, |f| f.component);
        for _ in 0..app.session.doc.components.len() {
            let Some(parent) = app.session.doc.components.iter().find(|c| c.id == component && c.id != 0) else { break };
            d.bands.insert(format!("component:{}", parent.id), true);
            component = parent.parent;
        }
        d.redesign.ordered_group = Some(group.clone());
        d.redesign.reveal_group = true;
    }
    d.redesign.selected_group = selected;
    if let Some(group) = &d.redesign.ordered_group
        && let Some(at) = sections.groups.iter().position(|(g, _)| *g == group)
    {
        let entry = sections.groups.remove(at);
        sections.groups.insert(0, entry);
    }
}

pub(super) fn validate_name(app: &SolveApp, old: &str, new: &str, promoting: bool) -> Result<(), String> {
    let new = new.trim();
    if solidcraft_engine::doc::Document::param_name_problem(new).is_some() || (promoting && auto_name(new)) {
        return Err(tr("parameters.redesign.invalid_name").to_owned());
    }
    let taken = app.session.doc.param(new).is_some()
        || app.session.doc.text_param(new).is_some()
        || app.session.doc.features.iter().any(|f| f.param_names.iter().any(|n| n == new));
    if new != old && taken {
        return Err(solidcraft_ui_text::format("parameters.redesign.taken_name", &[("name", new)]));
    }
    Ok(())
}

pub(super) fn suggestions(r: &Row) -> Vec<String> {
    let identifier = |text: &str| {
        let result: String = text.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        result.split('_').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("_")
    };
    let measure = identifier(&r.measures);
    let group = identifier(&r.group);
    [measure.clone(), format!("{group}_{measure}")].into_iter().filter(|s| !s.is_empty()).map(|s| s.chars().take(64).collect()).collect()
}

pub(super) fn name_editor(app: &SolveApp, ui: &mut egui::Ui, r: &Row, id: egui::Id, text: &mut String) -> ui_kit::CellEditor {
    let problem = validate_name(app, &r.name, text, !named(r)).err();
    let response = ui_kit::cell_editor(ui, id, text, "Name", problem.is_some());
    if !named(r) && response.has_focus() {
        let suggestions = suggestions(r);
        let kind = tr("parameters.redesign.suggestions");
        let color = solidcraft_theme::Tokens::get().form_ink;
        let options: Vec<_> = suggestions.iter().map(|text| ui_kit::PopupOption { text, detail: "", kind, color }).collect();
        if let Some(index) = ui_kit::popup_list(
            ui.ctx(),
            id.with("suggestions"),
            response.rect,
            None,
            &options,
            problem.as_deref().unwrap_or(tr("parameters.redesign.name_hint")),
        ) && let Some(suggestion) = suggestions.get(index)
        {
            *text = suggestion.clone();
            response.request_focus();
        }
    }
    ui_kit::CellEditor { response, taken: false }
}

pub(super) fn renamed(app: &SolveApp, d: &mut ParamsDialog, old: &str, new: &str) {
    for drafts in [&mut d.editing, &mut d.comments, &mut d.applying] {
        if let Some(value) = drafts.remove(old) {
            drafts.insert(new.to_owned(), value);
        }
    }
    if let Some(value) = d.applied.remove(old) {
        d.applied.insert(new.to_owned(), value);
    }
    if let Some(originals) = &mut d.redesign.compact
        && let Some(value) = originals.remove(old)
    {
        originals.insert(new.to_owned(), value);
    }
    for (name, expression) in d.editing.iter_mut().chain(d.applying.iter_mut()).chain(d.redesign.compact.iter_mut().flat_map(|m| m.iter_mut())) {
        if app.session.doc.text_param(name).is_none() {
            *expression = solidcraft_engine::doc::expr::rename_reference(expression, old, new);
        }
    }
    for (name, (before, after)) in &mut d.applied {
        if app.session.doc.text_param(name).is_some() {
            continue;
        }
        *before = solidcraft_engine::doc::expr::rename_reference(before, old, new);
        *after = solidcraft_engine::doc::expr::rename_reference(after, old, new);
    }
    d.redesign.selected.remove(old);
    d.redesign.selected.insert(new.to_owned());
    d.redesign.naming = None;
    d.redesign.view = View::Named;
    d.redesign.toast = Some((app.session.revision, solidcraft_ui_text::format("parameters.redesign.promoted", &[("old", old), ("name", new)])));
}

/// The feature a reader stands for: the feature itself (`Feature:<name>`), or the feature or sketch
/// that owns a dimension or feature-input parameter. A user parameter has none.
fn reader_feature(app: &SolveApp, reader: &str) -> Option<u64> {
    let doc = &app.session.doc;
    if let Some(name) = reader.strip_prefix("Feature:") {
        return doc.features.iter().find(|f| f.name == name).map(|f| f.id);
    }
    doc.features
        .iter()
        .find(|f| {
            f.param_names.iter().any(|n| n == reader)
                || match &f.kind {
                    solidcraft_engine::doc::FeatureKind::Sketch { sketch, .. } => {
                        sketch.constraints.iter().any(|c| c.param.as_deref() == Some(reader))
                    }
                    _ => false,
                }
        })
        .map(|f| f.id)
}

fn show_feature(app: &mut SolveApp, name: &str) {
    if let Some(id) = app.session.doc.features.iter().find(|f| f.name == name).map(|f| f.id) {
        select_features(app, &[id]);
    }
}

fn select_features(app: &mut SolveApp, ids: &[u64]) {
    let items: Vec<Value> = ids.iter().map(|id| json!({"type":"feature","id":id})).collect();
    let _ = app.run("select.set", json!({ "items": items }));
}

/// The users `parameters.users` reports that are not readers in the Used by list, and that the
/// list cannot select: a component's quantity, a joint, a configuration column, a rule, a canvas
/// or a moved occurrence (#20879), by their names in the design.
pub(super) fn other_uses(users: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for user in users.get("users").and_then(Value::as_array).into_iter().flatten() {
        let name = match user.get("type").and_then(Value::as_str) {
            Some("parameter" | "feature" | "dimension") | None => continue,
            Some("configuration") => user.get("column"),
            Some(_) => user.get("name"),
        };
        if let Some(name) = name.and_then(Value::as_str)
            && !out.iter().any(|known| known == name)
        {
            out.push(name.to_owned());
        }
    }
    out
}

/// The Used by cell and its list. Choosing a reader selects its feature or sketch, so it lights in
/// the viewport and the Browser; a reader that is a user parameter filters the window to it
/// instead, to follow a chain of parameters; Select All selects every reader's feature at once;
/// uses the list cannot select are named under Other uses (#20879).
pub(super) fn used_by(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, r: &Row) -> egui::Response {
    let response = ui_kit::text(ui, &r.readers.len().to_string(), ui_kit::Text::Cell).interact(egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("parameters.redesign.used_by")));
    let key = response.id.with(("readers", app.session.revision));
    if response.clicked()
        && let Ok(users) = app.session.execute("parameters.users", &json!({"name":r.name}))
    {
        ui.ctx().data_mut(|data| data.insert_temp(key, users));
    }
    crate::scenario::publish_target(&format!("param:users:{}", r.name), response.rect);
    if !response.clicked() && !ui_kit::Menu::response_is_open(&response) {
        return response;
    }
    let users = ui.ctx().data(|data| data.get_temp::<Value>(key)).unwrap_or_default();
    let title = solidcraft_ui_text::format("parameters.redesign.used_title", &[("name", &r.name), ("count", &r.readers.len().to_string())]);
    let mut items = vec![ui_kit::Item::heading(&title)];
    for (index, reader) in r.readers.iter().enumerate() {
        let label = reader.strip_prefix("Feature:").unwrap_or(reader);
        let detail =
            users.get("users").and_then(Value::as_array).into_iter().flatten().find(|user| user.get("name").and_then(Value::as_str) == Some(label));
        let detail = detail.and_then(|user| user.get("input").or_else(|| user.get("constraint"))).and_then(Value::as_str).unwrap_or_default();
        let shown = if detail.is_empty() {
            label.to_owned()
        } else {
            solidcraft_ui_text::format("parameters.redesign.reader_detail", &[("name", label), ("detail", detail)])
        };
        let mut item = ui_kit::Item::action(&format!("reader:{index}"), &shown, "");
        item.aliases.push(format!("param:reader:{}:{reader}", r.name));
        items.push(item);
    }
    let mut features = Vec::new();
    for id in r.readers.iter().filter_map(|reader| reader_feature(app, reader)) {
        if !features.contains(&id) {
            features.push(id);
        }
    }
    if features.len() > 1 {
        let mut item = ui_kit::Item::action("all", solidcraft_ui_text::tr_label("Select All"), "select");
        item.aliases.push(format!("param:readers:{}:all", r.name));
        items.push(item);
    }
    let others = other_uses(&users);
    if !others.is_empty() {
        items.push(ui_kit::Item::heading(tr("parameters.redesign.other_uses")));
        items.extend(others.iter().map(|other| ui_kit::Item::heading(other)));
    }
    if r.readers.is_empty() && others.is_empty() {
        items.push(ui_kit::Item::heading(tr("parameters.redesign.no_readers")));
    }
    let picked = {
        // User names and already resolved captions must remain literal.
        let _names = solidcraft_ui_text::resolved_labels();
        ui_kit::Menu::button_items(&response, &items)
    };
    if let Some(picked) = picked {
        if picked.id == "all" {
            select_features(app, &features);
        } else if let Some(reader) =
            picked.id.strip_prefix("reader:").and_then(|index| index.parse::<usize>().ok()).and_then(|index| r.readers.get(index))
        {
            if let Some(id) = reader_feature(app, reader) {
                select_features(app, &[id]);
            } else if app.session.doc.param(reader).is_some() {
                d.filter = reader.clone();
                d.reset_scroll = true;
            }
        }
    }
    response
}

pub(super) fn row_menu(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, r: &Row, members: &[&Row]) -> egui::Response {
    if !named(r) {
        return ui
            .horizontal(|ui| {
                let response = ui_kit::button(ui, tr("parameters.redesign.name_it"), ui_kit::Button::Normal);
                crate::scenario::publish_handle(&format!("param:name_it:{}", r.name), response.rect.center());
                if response.clicked() {
                    d.redesign.start_name(r, false);
                }
                if ui_kit::icon_label_button(ui, "", "select", true, false).backing_on_hover_text(tr("parameters.redesign.show_model")).clicked() {
                    show_feature(app, &r.group);
                }
                response
            })
            .inner;
    }
    let response = ui_kit::compact_button(ui, "…", tr("parameters.redesign.actions"), true, false);
    crate::scenario::publish_handle(&format!("param:menu:{}", r.name), response.rect.center());
    let mut items = Vec::new();
    if !r.group.is_empty() {
        items.push(ui_kit::Item::action("show_model", tr("parameters.redesign.show_model"), "select"));
    }
    if r.source != "user" {
        items.push(ui_kit::Item::action("demote", tr("parameters.redesign.demote"), "rename"));
    } else {
        items.push(ui_kit::Item::action("delete", solidcraft_ui_text::tr_label("Delete"), "delete"));
        let users = members.iter().copied().filter(|row| row.source == "user").collect::<Vec<_>>();
        if let Some(index) = users.iter().position(|row| row.name == r.name) {
            for (label, side, neighbour) in
                [("Move up", "before", index.checked_sub(1).and_then(|i| users.get(i))), ("Move down", "after", users.get(index + 1))]
            {
                let mut item = ui_kit::Item::action(side, solidcraft_ui_text::tr_label(label), "reorder")
                    .on_if(d.sort == ui_kit::Sort::Listed && neighbour.is_some());
                item.aliases.push(format!("param:{side}:{}", r.name));
                if let Some(neighbour) = neighbour {
                    item.params = Some(json!({"name":r.name,side:neighbour.name}));
                }
                items.push(item);
            }
        }
    }
    let mut picked = None;
    if !d.redesign.narrow {
        picked = ui_kit::Menu::button_items(&response, &items);
    } else {
        // This host also contains real unit/comment editors; its action rows use the shared menu.
        let menu =
            ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Popup).menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        menu.show(|ui| {
            let output = ui_kit::Menu::items(ui, response.id.with("actions"), &items, response.clicked());
            picked = output.picked;
            if output.close {
                ui.close();
            }
            if d.redesign.narrow {
                let _ = used_by(app, ui, d, r);
                if r.source == "user" {
                    // A combo popup replaces the parent popup in egui's memory. Use the kit's
                    // nested menu so the row remains available while choosing a unit.
                    let response = ui_kit::text_button(ui, unit_label(&r.unit), true);
                    let units = UNITS
                        .iter()
                        .map(|unit| {
                            let mut item = ui_kit::Item::action(unit, unit_label(unit), "");
                            item.checked = Some(*unit == r.unit);
                            item.radio = true;
                            item
                        })
                        .collect::<Vec<_>>();
                    if let Some(picked) = ui_kit::Menu::button_items(&response, &units) {
                        edit_cell(app, ui.ctx(), d, r, "user", UNIT, picked.id);
                    }
                }
                let mut comment = d.comments.get(&r.name).cloned().unwrap_or_else(|| r.comment.clone());
                let id = ui.make_persistent_id("menu_comment");
                let response = ui_kit::cell_editor(ui, id, &mut comment, solidcraft_ui_text::tr_label("Comment"), false);
                if response.changed() {
                    d.comments.insert(r.name.clone(), comment.clone());
                }
                if response.lost_focus() {
                    d.error = app.run("parameters.comment", json!({"name":r.name,"comment":comment})).err();
                }
            }
        });
    }
    if let Some(picked) = picked {
        match picked.id.as_str() {
            "show_model" => show_feature(app, &r.group),
            "demote" => d.redesign.demote = Some(r.name.clone()),
            "delete" => d.error = app.run("parameters.delete", json!({"name":r.name})).err(),
            "before" | "after" => {
                if let Some(params) = picked.params {
                    d.error = app.run("parameters.move", params).err();
                }
            }
            _ => {}
        }
    }
    response
}

fn free_dimension(all: &[Row]) -> Option<String> {
    let taken: BTreeSet<_> = all.iter().map(|r| r.name.as_str()).collect();
    (1..=all.len().saturating_add(1)).map(|n| format!("d{n}")).find(|n| !taken.contains(n.as_str()))
}

pub(super) fn demote(app: &mut SolveApp, d: &mut ParamsDialog, r: &Row, all: &[Row]) -> Result<(), String> {
    if r.source == "user" {
        return Err(tr("parameters.redesign.user_demote").to_owned());
    }
    let new = free_dimension(all).ok_or_else(|| tr("parameters.redesign.invalid_name").to_owned())?;
    run_as_one(
        app,
        &[("parameters.favorite", json!({"name":r.name,"favorite":false})), ("parameters.rename", json!({"name":r.name,"new_name":new}))],
    )?;
    renamed(app, d, &r.name, &new);
    d.redesign.toast = None;
    d.redesign.view = View::Dimensions;
    d.bands.insert(format!("model:{}", r.group), true);
    Ok(())
}

pub(super) fn confirm_demote(app: &mut SolveApp, ctx: &egui::Context, d: &mut ParamsDialog, all: &[Row]) {
    let Some(name) = d.redesign.demote.clone() else { return };
    let Some(r) = all.iter().find(|r| r.name == name) else {
        d.redesign.demote = None;
        return;
    };
    let title = tr("parameters.redesign.demote");
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_parameter_demote"), "rename", title)
        .actions(title, tr("parameters.redesign.keep_name"), None)
        .show(ctx, true, |ui| {
            let text = solidcraft_ui_text::format("parameters.redesign.demote_detail", &[("name", &name), ("readers", &r.readers.join(", "))]);
            ui_kit::text(ui, &text, ui_kit::Text::Hint("demote_readers"));
        });
    if action == ui_kit::Action::Ok {
        d.error = demote(app, d, r, all).err();
    }
    if closed || action != ui_kit::Action::None {
        d.redesign.demote = None;
    }
}

pub(super) fn select_row(ui: &egui::Ui, d: &mut ParamsDialog, rows: &[&Row], row: usize, response: &egui::Response) {
    let Some(r) = rows.get(row) else { return };
    if response.clicked() {
        let modifiers = ui.input(|i| i.modifiers);
        if modifiers.shift {
            let names = selection_order(d);
            let anchor = d.redesign.anchor.as_ref().and_then(|name| names.iter().position(|n| n == name));
            let end = names.iter().position(|name| *name == r.name);
            if let (Some(anchor), Some(end)) = (anchor, end) {
                d.redesign.selected.extend(names.into_iter().take(anchor.max(end) + 1).skip(anchor.min(end)));
            } else {
                d.redesign.selected.insert(r.name.clone());
                d.redesign.anchor = Some(r.name.clone());
            }
        } else {
            if !modifiers.command {
                d.redesign.selected.clear();
            }
            if !d.redesign.selected.remove(&r.name) {
                d.redesign.selected.insert(r.name.clone());
            }
            d.redesign.anchor = Some(r.name.clone());
        }
    }
}

/// Row ranges cross section bands; only a Shift gesture needs this ordered snapshot.
fn selection_order(d: &ParamsDialog) -> Vec<String> {
    let Some((_, all)) = &d.listed else { return Vec::new() };
    let filter = d.filter.to_ascii_lowercase();
    let visible: Vec<_> = all.iter().filter(|r| r.matches(&filter)).collect();
    let mut sections = sections(&visible, d.sort);
    if d.redesign.view == View::Named {
        return [("fav", sections.favorites), ("user", sections.users)]
            .into_iter()
            .filter(|(key, _)| d.bands.get(*key).copied().unwrap_or(true))
            .flat_map(|(_, rows)| rows.into_iter().map(|r| r.name.clone()))
            .collect();
    }
    if let Some(group) = &d.redesign.ordered_group
        && let Some(index) = sections.groups.iter().position(|(name, _)| *name == group)
    {
        let group = sections.groups.remove(index);
        sections.groups.insert(0, group);
    }
    sections
        .groups
        .into_iter()
        .filter(|(group, _)| d.bands.get(&format!("model:{group}")).copied().unwrap_or(!filter.is_empty()))
        .flat_map(|(_, rows)| rows.into_iter().map(|r| r.name.clone()))
        .collect()
}

pub(super) fn bulk(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, all: &[Row]) {
    if d.redesign.selected.len() < 2 {
        return;
    }
    let selected: Vec<_> = all.iter().filter(|r| d.redesign.selected.contains(&r.name)).collect();
    if selected.len() < 2 {
        return;
    }
    let label = solidcraft_ui_text::format("parameters.redesign.selected_count", &[("count", &selected.len().to_string())]);
    let named = selected.iter().all(|r| named(r));
    let bar = ui_kit::SelectionBar { id: "params:bulk", label: &label, clear_label: tr("parameters.redesign.clear_selection") }.show(
        ui,
        &[
            ui_kit::SelectionAction { label: tr("parameters.redesign.pin"), icon: "star", enabled: named },
            ui_kit::SelectionAction { label: tr("parameters.redesign.unpin"), icon: "star", enabled: named },
            ui_kit::SelectionAction { label: tr("parameters.redesign.copy"), icon: "copy", enabled: true },
            ui_kit::SelectionAction {
                label: solidcraft_ui_text::tr_label("Delete"),
                icon: "delete",
                enabled: selected.iter().all(|r| r.source == "user"),
            },
        ],
    );
    for (index, favorite) in [(0, true), (1, false)] {
        if bar.actions.get(index).is_some_and(|r| r.clicked()) {
            let steps: Vec<_> = selected.iter().map(|r| ("parameters.favorite", json!({"name":r.name,"favorite":favorite}))).collect();
            d.error = run_as_one(app, &steps).err();
        }
    }
    if bar.actions.get(2).is_some_and(|r| r.clicked()) {
        let rows: Vec<_> = selected.iter().map(|r| vec![r.name.clone(), r.expression.clone(), r.unit.clone(), r.comment.clone()]).collect();
        ui.ctx().copy_text(ui_kit::write_tsv(&rows));
    }
    if bar.actions.get(3).is_some_and(|r| r.clicked()) {
        let steps: Vec<_> = selected.iter().rev().map(|r| ("parameters.delete", json!({"name":r.name}))).collect();
        d.error = run_as_one(app, &steps).err();
    }
    if bar.clear.clicked() {
        d.redesign.selected.clear();
    }
}

fn name_prefix(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, selected: &[&Row]) {
    let id = ui.make_persistent_id("name_prefix");
    let field = ui_kit::cell_editor(ui, id, &mut d.redesign.prefix, tr("parameters.redesign.prefix"), false);
    crate::scenario::publish_target("params:name_prefix", field.rect);
    let apply = ui_kit::button(ui, tr("parameters.redesign.prefix"), ui_kit::Button::Enabled(!d.redesign.prefix.is_empty()));
    crate::scenario::publish_target("params:apply_prefix", apply.rect);
    if apply.clicked() {
        if d.redesign.prefix.len() > 64 {
            d.error = Some(tr("parameters.redesign.invalid_name").to_owned());
            return;
        }
        let targets: Vec<_> = selected.iter().map(|r| (r.name.clone(), format!("{}{}", d.redesign.prefix, r.name))).collect();
        let validation = targets.iter().try_for_each(|(old, new)| validate_name(app, old, new, false));
        d.error = validation.err();
        if d.error.is_none() {
            let steps: Vec<_> = targets.iter().map(|(old, new)| ("parameters.rename", json!({"name":old,"new_name":new}))).collect();
            d.error = run_as_one(app, &steps).err();
            if d.error.is_none() {
                for (old, new) in targets {
                    renamed(app, d, &old, &new);
                }
                ui.close();
            }
        }
    }
}

pub(super) fn states(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, all: &[Row], filtered: &[&Row]) {
    let visible = filtered.iter().any(|r| named(r) == (d.redesign.view == View::Named));
    if !visible {
        let key = match (d.redesign.view, d.filter.is_empty()) {
            (View::Named, true) => "parameters.redesign.empty_named",
            (View::Dimensions, true) => "parameters.redesign.empty_dimensions",
            _ => "parameters.redesign.no_matches",
        };
        ui_kit::text(ui, tr(key), ui_kit::Text::Hint("empty_parameters"));
        if d.redesign.view == View::Named && ui_kit::button(ui, tr("parameters.redesign.browse"), ui_kit::Button::Normal).clicked() {
            d.redesign.view = View::Dimensions;
        }
    }
    for r in all.iter().filter(|r| r.error.is_some()) {
        let message =
            solidcraft_ui_text::format("parameters.redesign.row_error", &[("name", &r.name), ("error", r.error.as_deref().unwrap_or_default())]);
        ui_kit::text(ui, &message, ui_kit::Text::Error(&r.name));
    }
    // Stored errors disable OK. A held batch must still be able to repair a cycle together.
    if all.iter().any(|r| r.error.is_some())
        && !app.preferences.parameters_apply_live
        && !held(d, all).is_empty()
        && ui_kit::button(ui, solidcraft_ui_text::tr_label("Apply"), ui_kit::Button::Normal).clicked()
    {
        apply_held(app, d, all);
    }
    if let Some((revision, message)) = d.redesign.toast.clone() {
        if revision == app.session.revision {
            ui.horizontal(|ui| {
                ui_kit::text(ui, &message, ui_kit::Text::Hint("named_toast"));
                if ui_kit::button(ui, solidcraft_ui_text::tr_label("Undo"), ui_kit::Button::Normal).clicked() {
                    d.error = app.run("edit.undo", json!({})).err();
                    d.redesign.toast = None;
                }
            });
        } else {
            d.redesign.toast = None;
        }
    }
    if d.redesign.view == View::Named {
        let toggle = ui_kit::button(ui, tr("parameters.redesign.compact"), ui_kit::Button::Normal);
        crate::scenario::publish_target("params:key:toggle", toggle.rect);
        if toggle.clicked() {
            d.redesign.compact = if d.redesign.compact.is_some() {
                None
            } else {
                ui.scroll_to_cursor(Some(egui::Align::TOP));
                Some(all.iter().filter(|r| named(r) && r.favorite).map(|r| (r.name.clone(), r.expression.clone())).collect())
            };
        }
        compact(app, ui, d, all);
    }
}

/// The explicitly permitted compact-window fallback. Uses the same held/live edit path.
fn compact(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, all: &[Row]) {
    let Some(originals) = d.redesign.compact.take() else { return };
    for r in all.iter().filter(|r| named(r) && r.favorite) {
        ui.push_id(("key_field", &r.name), |ui| {
            let changed = originals.get(&r.name).is_some_and(|original| d.editing.get(&r.name).unwrap_or(&r.expression) != original);
            let label = if changed { solidcraft_ui_text::format("parameters.redesign.changed_key", &[("name", &r.name)]) } else { r.name.clone() };
            ui_kit::text(ui, &label, ui_kit::Text::MonoCell);
            ui_kit::text(ui, &r.comment, ui_kit::Text::Hint("key_comment"));
            let id = ui.make_persistent_id("expression");
            let mut expression = d.editing.get(&r.name).cloned().unwrap_or_else(|| r.expression.clone());
            let response = ui_kit::cell_editor(ui, id, &mut expression, "Expression", false);
            crate::scenario::publish_target(&format!("params:key:{}", r.name), response.rect);
            let completed = complete(ui, &response, &mut expression);
            let mut commit = !completed && (enter_for(ui, response.has_focus() || response.lost_focus()) || response.lost_focus());
            let numeric_text = expression.trim().strip_suffix(&r.unit).unwrap_or(expression.trim()).trim();
            let mut numeric = numeric_text.parse::<f64>().ok().filter(|v| v.is_finite());
            if let Some(value) = numeric.as_mut() {
                let extent = value.abs().max(1.0) * 2.0;
                if extent.is_finite() {
                    let response = ui_kit::slider(
                        ui,
                        value,
                        -extent..=extent,
                        ui_kit::Slider { format: |v| format!("{v:.3}"), show_value: false, ..Default::default() },
                    );
                    crate::scenario::publish_target(&format!("params:key:slider:{}", r.name), response.rect);
                    if response.changed() {
                        expression = value.to_string();
                    }
                    commit |= response.drag_stopped() || response.clicked() || (response.changed() && !ui.input(|i| i.pointer.any_down()));
                }
            }
            if expression != r.expression {
                d.editing.insert(r.name.clone(), expression.clone());
            }
            if commit {
                edit_cell(app, ui.ctx(), d, r, "fav", EXPRESSION, expression);
            }
        });
    }
    let reset = ui_kit::button(ui, solidcraft_ui_text::tr_label("Reset"), ui_kit::Button::Normal);
    crate::scenario::publish_target("params:key:reset", reset.rect);
    if reset.clicked() {
        let edits: Vec<_> = all
            .iter()
            .filter_map(|r| {
                let original = originals.get(&r.name)?;
                (d.editing.get(&r.name).unwrap_or(&r.expression) != original).then(|| ui_kit::Edit::Set {
                    row: ui_kit::RowRef::Existing(r.name.clone()),
                    col: EXPRESSION,
                    text: original.clone(),
                })
            })
            .collect();
        let rows: Vec<_> = all.iter().collect();
        apply_grid(app, ui.ctx(), d, &rows, "fav", ui_kit::GridOutput { edits, ..Default::default() });
    }
    d.redesign.compact = Some(originals);
}

fn edit_cell(app: &mut SolveApp, ctx: &egui::Context, d: &mut ParamsDialog, r: &Row, salt: &str, col: usize, text: String) {
    let edit = ui_kit::Edit::Set { row: ui_kit::RowRef::Existing(r.name.clone()), col, text };
    apply_grid(app, ctx, d, &[r], salt, ui_kit::GridOutput { edits: vec![edit], ..Default::default() });
}
