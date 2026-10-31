//! Ribbon-anchored command search. Ranking comes from the registry; document results use
//! existing feature-edit and parameter commands. JSON mode never starts an interactive tool.

use crate::ui_kit::{Container, Surface};
use egui::{Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::{CommandSpec, command_specs};

use crate::theme::Tokens;
use crate::{SolveApp, ui_kit};

/// How many recent commands are kept.
const RECENT: usize = 12;
pub(crate) const MAX_QUERY: usize = 4096;
const MAX_RESULTS: usize = 80;

#[derive(Default)]
pub struct ShortcutBox {
    /// Fallback anchor when there is no ribbon (headless tests).
    at: Option<Pos2>,
    /// The highlighted row.
    index: usize,
    /// What is typed.
    pub text: String,
    /// Ask the field for focus on the next frame. Set when the box opens, then cleared.
    /// Requesting focus on every frame restarts the text edit's held key, so a Wayland
    /// repeat never sees the release and the key keeps repeating after the app loses focus (#724).
    focus: bool,
    /// Most recent first.
    pub recent: Vec<String>,
    pub pinned: Vec<String>,
    design: Option<(u64, Vec<DesignRow>)>,
    /// Retained paint content while closing; the public query still clears immediately.
    closing_text: String,
    /// The pointer moved while the box was open: only then does a hovered result reveal its
    /// place, not when the results appear under a resting pointer.
    moved: bool,
    /// The command whose place the ribbon shows (#7287).
    pub reveal: Option<Reveal>,
}

/// A command the ribbon points out: its button pulses, or its section's drop-down opens on its
/// row with its tooltip, or (once started) its section label pulses.
#[derive(Clone, Debug)]
pub struct Reveal {
    pub id: &'static str,
    /// When it started (egui input time, seconds).
    pub since: f64,
    /// Shown while a search result is hovered; then the drop-down and tooltip stay open, and
    /// the ribbon goes back to `back` when the pointer leaves the results.
    pub preview: bool,
    /// The tab to return to after a preview.
    pub back: String,
}

/// How long a reveal pulses, in seconds: two blinks.
pub const REVEAL_PULSE: f64 = 1.2;

impl Reveal {
    /// The pulse's strength at `now`: 0 to 1 and back twice over `REVEAL_PULSE`, then 1 for a
    /// preview (it stays marked while hovered) and 0 for a started command. Without animations
    /// a preview is marked steadily and a start isn't pulsed.
    pub fn strength(&self, now: f64, animate: bool) -> f32 {
        let t = now - self.since;
        if !animate || !(0.0..REVEAL_PULSE).contains(&t) {
            return if self.preview { 1.0 } else { 0.0 };
        }
        (0.5 - 0.5 * (t / REVEAL_PULSE * 4.0 * std::f64::consts::PI).cos()) as f32
    }
}

/// Shows the hovered result's place in the ribbon (#7287): its tab, then its button or its
/// section drop-down row. `None` ends a preview and goes back to the tab the user was on.
fn preview(app: &mut SolveApp, now: f64, id: Option<&'static str>) {
    let Some(id) = id else {
        end_preview(app);
        return;
    };
    if app.sbox.reveal.as_ref().is_some_and(|r| r.preview && r.id == id) {
        return;
    }
    let back = match app.sbox.reveal.take() {
        Some(r) if r.preview => r.back,
        _ => app.ui.tab.clone(),
    };
    app.ui.tab = back.clone();
    if let Some(tab) = crate::workspace::command_tab(app, id) {
        app.ui.tab = tab.into();
    }
    app.sbox.reveal = Some(Reveal { id, since: now, preview: true, back });
}

fn end_preview(app: &mut SolveApp) {
    if let Some(r) = app.sbox.reveal.take_if(|r| r.preview) {
        app.ui.tab = r.back;
    }
}

impl ShortcutBox {
    pub fn prefs(&self) -> Value {
        json!({ "recent": self.recent, "pinned": self.pinned })
    }
    pub fn load(&mut self, v: &Value) {
        let list = |k: &str| -> Vec<String> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().take(64).filter_map(|x| x.as_str().filter(|s| s.len() <= 256).map(str::to_string)).collect())
                .unwrap_or_default()
        };
        self.recent = list("recent");
        self.pinned = list("pinned");
    }
    /// A command was run from the box: it goes to the front of the recent list.
    pub fn used(&mut self, id: &str) {
        if solidcraft_engine::find_command(id).is_none() {
            return;
        }
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_string());
        self.recent.truncate(RECENT);
    }
    pub fn toggle_pin(&mut self, id: &str) {
        if self.pinned.iter().any(|p| p == id) {
            self.pinned.retain(|p| p != id);
        } else {
            self.pinned.push(id.to_string());
        }
    }
}

/// How well `q` matches `s` (0 = not at all): a prefix beats a word start beats a substring
/// beats the letters in order with gaps.
pub fn score(q: &str, s: &str) -> u32 {
    if q.is_empty() {
        return 1;
    }
    let s = s.to_lowercase();
    if s.starts_with(q) {
        return 1000 - s.len().min(500) as u32;
    }
    if s.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(q)) {
        return 800;
    }
    if s.contains(q) {
        return 600;
    }
    // Letters in order: fewer gaps score higher.
    let mut gaps = 0u32;
    let mut it = s.chars();
    let mut last_hit = true;
    for qc in q.chars() {
        let mut found = false;
        for c in it.by_ref() {
            if c == qc {
                found = true;
                break;
            }
            if last_hit {
                gaps += 1;
            }
            last_hit = false;
        }
        if !found {
            return 0;
        }
        last_hit = true;
    }
    300u32.saturating_sub(gaps * 20).max(10)
}

/// Words people look for a command by that its name doesn't hold (file formats, what it is for).
fn keywords(id: &str) -> &'static str {
    match id {
        "file.save_mesh" => "export stl 3mf mesh 3d print printing slicer",
        "file.export" => "export stl 3mf obj step urdf mesh 3d print",
        _ => "",
    }
}

/// How well `q` matches a command's `keywords`: every word of the query starts a keyword.
fn keyword_score(q: &str, keywords: &str) -> u32 {
    if keywords.is_empty() || q.is_empty() {
        return 0;
    }
    let words = || keywords.split_whitespace();
    if q.split_whitespace().all(|w| words().any(|k| k.starts_with(w))) { 700 } else { 0 }
}

/// A row of the box: the command id, its label, its icon, and where it lives on the toolbar
/// (tab, panel; `None` off the toolbar), in the current language.
pub type Row = (&'static str, &'static str, &'static str, Option<(&'static str, &'static str)>);

/// Whether a command can be run from the search box (#9734): it has a place on the ribbon,
/// opens a dialog or a tool, or runs with no parameters. Commands that take their input as
/// parameters (the script and agent commands) are left out: picking one would run it with no
/// parameters and end in a "`name` is required" error. They stay reachable by typing the id
/// with a JSON object, as the hint under the field describes.
fn runnable_from_search(app: &SolveApp, c: &CommandSpec) -> bool {
    crate::workspace::destination(c).is_some()
        // Needs no parameters: its contract accepts `{}`, or it documents none of its own.
        || c.contract.map_or(c.params.is_empty(), |k| k.check(&json!({})).is_empty())
        || app.interactive(c.id)
}

/// Commands matching `q`, best first. With an empty query, recent commands come first, then
/// everything by name.
///
/// Only commands that can be run from the box are listed (see [`runnable_from_search`], #9734).
///
/// The query matches the label, tab and panel in the current language and in English, and the
/// id, so a name typed in either language, or an id, finds the command in every language.
pub fn search(app: &SolveApp, q: &str) -> Vec<Row> {
    let q = q.trim().to_lowercase();
    let mut v: Vec<(u32, Row)> = command_specs()
        .into_iter()
        .filter(|c| !crate::workspace::AGENT_ONLY.contains(&c.id) && runnable_from_search(app, c))
        .filter_map(|c| {
            let (home_tab, home_panel) = crate::workspace::destination(c).unwrap_or(("", ""));
            let info = crate::render_workspace::interactive_command_info(app, c);
            let (label, tab, panel) =
                (crate::i18n::tr_label(info.label), crate::i18n::tr_label(crate::workspace::tab_label(home_tab)), crate::i18n::tr_label(home_panel));
            let place = (!home_tab.is_empty()).then_some((tab, panel));
            let s = score(&q, label)
                .max(score(&q, c.label))
                .max(score(&q, info.label))
                .max(score(&q, c.id) * 9 / 10)
                .max(keyword_score(&q, keywords(c.id)))
                .max(score(&q, tab).max(score(&q, home_tab)) / 3)
                .max(score(&q, panel).max(score(&q, home_panel)) / 3);
            let recent = app.sbox.recent.iter().position(|r| r == c.id).map_or(0, |i| 2000u32.saturating_sub(i.min(2000) as u32));
            // Commands that can run now come before those that can't.
            let usable = if info.enabled { 150 } else { 0 };
            (s > 0).then_some((s + usable + if q.is_empty() { recent } else { recent / 20 }, (c.id, label, info.icon, place)))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.1.cmp(b.1.1)));
    v.into_iter().map(|(_, row)| row).collect()
}

#[derive(Clone, Debug)]
enum DesignTarget {
    Feature(u64),
    Parameter(String),
}

#[derive(Clone, Debug)]
struct DesignRow {
    label: String,
    detail: String,
    icon: &'static str,
    target: DesignTarget,
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_QUERY).collect()
}

fn design_rows(app: &mut SolveApp, query: &str) -> Vec<DesignRow> {
    if app.sbox.design.as_ref().is_none_or(|(revision, _)| *revision != app.session.revision) {
        let doc = &app.session.doc;
        let params = doc.param_rows();
        let values: std::collections::BTreeMap<_, _> = params.iter().take(20_000).map(|p| (p.name.as_str(), p)).collect();
        let mut rows = Vec::new();
        for feature in doc.features.iter().take(10_000) {
            let value = feature
                .param_names
                .iter()
                .take(64)
                .find_map(|name| values.get(name.as_str()))
                .map(|p| p.value.filter(|v| v.is_finite()).map_or_else(|| bounded(&p.expression), |v| format!("{v} {}", p.unit)))
                .unwrap_or_default();
            rows.push(DesignRow {
                label: bounded(&feature.name),
                detail: format!("{} · {value}", crate::i18n::tr("search.feature")),
                icon: crate::timeline::icon_of(&feature.kind),
                target: DesignTarget::Feature(feature.id),
            });
        }
        let icon = solidcraft_engine::find_command("parameters.change").map_or("", |c| c.icon);
        for p in params.iter().filter(|p| p.source == "user").take(10_000) {
            let value = p.value.filter(|v| v.is_finite()).map_or_else(|| bounded(&p.expression), |v| format!("{v} {}", p.unit));
            rows.push(DesignRow {
                label: bounded(&p.name),
                detail: format!("{} · {value}", crate::i18n::tr("search.parameter")),
                icon,
                target: DesignTarget::Parameter(p.name.clone()),
            });
        }
        app.sbox.design = Some((app.session.revision, rows));
    }
    let q = query.trim().to_lowercase();
    let mut rows: Vec<_> = app
        .sbox
        .design
        .as_ref()
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .filter_map(|row| {
            let rank = score(&q, &row.label);
            (rank > 0).then_some((rank, row.clone()))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.label.cmp(&b.1.label)));
    rows.into_iter().take(24).map(|(_, row)| row).collect()
}

/// Same split, serde parser and error messages as the original command line.
fn typed(line: &str) -> Option<(&str, Result<Value, String>)> {
    let line = line.trim();
    if !line.contains('{') {
        return None;
    }
    let split = line.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(line.len());
    let id = line.get(..split).unwrap_or_default();
    let rest = line.get(split..).unwrap_or_default().trim();
    let result = if solidcraft_engine::find_command(id).is_none() {
        Err(crate::i18n::format("shortcut.no_command", &[("id", id)]))
    } else {
        serde_json::from_str::<Value>(rest).map_err(|e| crate::i18n::format("shortcut.json_error", &[("error", &e.to_string())]))
    };
    Some((id, result))
}

/// Bounded Levenshtein distance also handles transposed/missing letters in a no-match query.
fn closest(query: &str) -> Option<&'static str> {
    let q: Vec<_> = query.trim().to_lowercase().chars().take(128).collect();
    command_specs()
        .into_iter()
        .map(|c| {
            let label = crate::i18n::tr_label(c.label);
            let mut prev: Vec<_> = (0..=q.len()).collect();
            for (j, ch) in label.to_lowercase().chars().take(128).enumerate() {
                let mut next = vec![j + 1];
                for (i, qc) in q.iter().enumerate() {
                    next.push(
                        (prev.get(i).copied().unwrap_or(128) + usize::from(ch != *qc))
                            .min(prev.get(i + 1).copied().unwrap_or(128) + 1)
                            .min(next.last().copied().unwrap_or(128) + 1),
                    );
                }
                prev = next;
            }
            (prev.last().copied().unwrap_or(128), label)
        })
        .min()
        .map(|(_, label)| label)
}

/// Open the box at a point (the cursor), or at the middle of the window.
pub fn open(app: &mut SolveApp, at: Option<Pos2>) {
    app.ui.palette_open = true;
    app.sbox.at = at;
    app.sbox.index = 0;
    app.sbox.text.clear();
    app.sbox.focus = true;
    app.sbox.design = None;
    app.sbox.moved = false;
}

/// A line typed as `command.id {"param": value}`: the command runs at once with those parameters
/// (the same entry the bottom command line used to take).
fn run_typed(app: &mut SolveApp, line: &str) {
    if let Some((id, result)) = typed(line) {
        match result {
            Ok(params) => {
                app.sbox.used(id);
                let _ = app.run(id, params);
            }
            Err(error) => app.set_status(error, true),
        }
    }
    close(app);
}

fn close(app: &mut SolveApp) {
    end_preview(app);
    app.ui.palette_open = false;
    app.sbox.at = None;
    app.sbox.text.clear();
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let t = Tokens::get();
    let screen = ctx.content_rect();
    let open = app.ui.palette_open;
    // Respect egui's instant/reduced animation policy, with a 120 ms upper bound.
    let progress = ctx.animate_bool_with_time_and_easing(
        egui::Id::new("sc_search_expansion"),
        open,
        ctx.global_style().animation_time.clamp(0.0, 0.12),
        egui::emath::easing::cubic_out,
    );
    let ribbon = ctx.data(|d| d.get_temp::<Rect>(egui::Id::new("sc_ribbon_search")));
    if !open {
        app.sbox.focus = false;
        ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("sc_sbox_text")));
        if progress == 0.0 {
            app.sbox.at = None;
        }
        if ribbon.is_none() && progress == 0.0 {
            return;
        }
    }
    let fallback = app.sbox.at.filter(|p| p.x.is_finite() && p.y.is_finite()).unwrap_or(screen.center());
    let width = ui_kit::CommandSearch::width().min((screen.width() - t.space_8).max(t.space_24));
    let closed = ribbon.unwrap_or_else(|| {
        Rect::from_min_size(
            pos2(
                fallback.x.clamp(screen.left() + t.space_4, (screen.right() - width - t.space_4).max(screen.left() + t.space_4)),
                fallback.y.max(screen.top() + t.space_4),
            ),
            vec2(width, t.control_height),
        )
    });
    let field_size = egui::lerp(closed.size()..=vec2(width, t.control_height + t.space_12), progress);
    let pos = pos2(closed.right() - field_size.x, closed.top());
    let height = (screen.bottom() - pos.y - field_size.y - t.space_8).max(t.space_24 * 4.0);
    let mut query = if open {
        bounded(&app.sbox.text)
    } else if progress > 0.0 {
        app.sbox.closing_text.clone()
    } else {
        String::new()
    };
    let (up, down, enter, esc, tab, keep) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Tab),
            i.modifiers.shift,
        )
    });
    let mut picked = None;
    let mut menu = None;
    // The command result under the pointer, when it has a place on the ribbon to show.
    let mut hovered = None;
    let now = ctx.input(|i| i.time);
    if open && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
        app.sbox.moved = true;
    }
    let mut rows = Vec::new();
    let mut design = Vec::new();
    let mut mode = None;
    // Closed, the field is part of the ribbon and belongs under the system windows the rest of
    // the ribbon goes under (Preferences, say); open, its sheet floats over everything.
    let floating = open || progress > 0.0;
    let response = Container::new(ctx, Surface::Popup)
        .area(egui::Id::new("sc_sbox"))
        .fixed_pos(pos)
        .order(if floating { egui::Order::Foreground } else { egui::Order::Background })
        .fade_in(false)
        .constrain(false)
        .show(ctx, |ui| {
            // Area rounds its origin; place the child at the exact interpolated bounds
            // so the field's right edge does not drift during expansion.
            ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos, vec2(field_size.x, height + field_size.y))), |ui| {
                ui.set_width(field_size.x);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui_kit::scope(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let invalid = typed(&query).is_some_and(|(_, result)| result.is_err());
                    let field = ui_kit::CommandSearch::field(
                        ui,
                        &mut query,
                        invalid,
                        std::mem::take(&mut app.sbox.focus),
                        open,
                        ui_kit::SearchFieldGeometry {
                            height: field_size.y,
                            progress,
                            radius: ui_kit::CommandSearch::field_radius(progress, ui_kit::Container::new(ctx, ui_kit::Surface::Chrome).floating()),
                        },
                        MAX_QUERY,
                    );
                    if !open && progress == 0.0 {
                        if field.clicked()
                            || ui.interact(Rect::from_min_size(pos, field_size), ui.id().with("search_open"), egui::Sense::click()).clicked()
                        {
                            app.start("ui.palette");
                            ctx.request_repaint();
                        }
                        return;
                    }
                    if field.changed() {
                        app.sbox.index = 0;
                    }
                    mode = typed(&query).map(|(id, result)| (id.to_owned(), result));
                    if mode.is_none() {
                        rows = search(app, &query)
                            .into_iter()
                            .filter(|r| {
                                !query.trim().is_empty()
                                    || (app.sbox.recent.is_empty() && app.sbox.pinned.is_empty())
                                    || app.sbox.recent.iter().chain(&app.sbox.pinned).any(|id| id == r.0)
                            })
                            .take(if query.trim().is_empty() {
                                if app.sbox.recent.is_empty() && app.sbox.pinned.is_empty() { 8 } else { MAX_RESULTS }
                            } else {
                                56
                            })
                            .collect();
                        design = if query.trim().is_empty() { Vec::new() } else { design_rows(app, &query) };
                    }
                    let enabled: Vec<_> = rows
                        .iter()
                        .map(|r| {
                            solidcraft_engine::find_command(r.0).is_some_and(|c| crate::render_workspace::interactive_command_info(app, c).enabled)
                        })
                        .chain(design.iter().map(|_| true))
                        .take(MAX_RESULTS)
                        .collect();
                    if down || up {
                        let step = if down { 1 } else { enabled.len().saturating_sub(1) };
                        for _ in 0..enabled.len() {
                            app.sbox.index = (app.sbox.index + step) % enabled.len().max(1);
                            if enabled.get(app.sbox.index).copied().unwrap_or(false) {
                                break;
                            }
                        }
                    }
                    app.sbox.index = app.sbox.index.min(enabled.len().saturating_sub(1));
                    if !enabled.get(app.sbox.index).copied().unwrap_or(false) {
                        app.sbox.index = enabled.iter().position(|v| *v).unwrap_or(0);
                    }
                    ui.add_space(t.space_6 * progress);
                    ui.add_enabled_ui(open, |ui| {
                        ui_kit::CommandSearch::sheet(ui, field_size.x, progress, mode.is_some(), |ui| {
                            egui::ScrollArea::vertical().max_height((height - t.control_height * 4.0).max(t.control_height)).show(ui, |ui| {
                                if let Some((id, result)) = &mode {
                                    let mut row = ui_kit::MenuItem::new(
                                        "search:run",
                                        crate::i18n::tr("search.run"),
                                        solidcraft_engine::find_command(id).map_or("", |c| c.icon),
                                    );
                                    row.enabled = result.is_ok();
                                    row.highlighted = true;
                                    let badge = crate::i18n::tr(if result.is_ok() { "search.valid" } else { "search.invalid" });
                                    row.shortcut = badge;
                                    if row
                                        .search_row(
                                            ui,
                                            "",
                                            id,
                                            Some(if result.is_ok() { ui_kit::SearchBadge::Valid } else { ui_kit::SearchBadge::Invalid }),
                                        )
                                        .clicked()
                                    {
                                        picked = Some(0);
                                    }
                                    match result {
                                        Ok(params) => ui_kit::CommandSearch::detail(
                                            ui,
                                            &format!("{id}\n{}", serde_json::to_string_pretty(params).unwrap_or_default()),
                                        ),
                                        Err(error) => {
                                            ui_kit::text(ui, error, ui_kit::Text::Error("command_parameters"));
                                        }
                                    }
                                } else {
                                    let empty = query.trim().is_empty();
                                    let mut last_recent = None;
                                    for (index, (id, label, icon, place)) in rows.iter().enumerate() {
                                        let recent = empty && app.sbox.recent.iter().any(|r| r == id);
                                        if last_recent != Some(recent) {
                                            let count =
                                                rows.iter().filter(|r| (empty && app.sbox.recent.iter().any(|id| id == r.0)) == recent).count();
                                            ui_kit::CommandSearch::heading(ui, if recent { "search.recent" } else { "search.commands" }, count);
                                            last_recent = Some(recent);
                                        }
                                        let mut item = ui_kit::MenuItem::new(id, label, icon);
                                        item.enabled = enabled.get(index).copied().unwrap_or(false);
                                        item.highlighted = index == app.sbox.index;
                                        let shortcut = crate::keymap::display_for(app, id).unwrap_or_default();
                                        item.shortcut = &shortcut;
                                        let path = place.map_or_else(|| id.to_string(), |(tab, panel)| format!("{tab} › {panel}"));
                                        let response = item.search_row(ui, &query, &path, None);
                                        crate::scenario::publish_target(&format!("search:command:{id}"), response.rect);
                                        if item.highlighted && (up || down) {
                                            ui.scroll_to_rect(response.rect, None);
                                        }
                                        if response.clicked() {
                                            picked = Some(index);
                                        }
                                        if response.hovered() && place.is_some() {
                                            hovered = Some(*id);
                                        }
                                        if response.secondary_clicked() {
                                            menu = Some((response.rect.left_bottom(), (*id).to_owned()));
                                        }
                                    }
                                    if !design.is_empty() {
                                        ui_kit::CommandSearch::heading(ui, "search.design", design.len());
                                    }
                                    for (index, row) in design.iter().enumerate() {
                                        let handle = format!("search:design:{index}");
                                        let mut item = ui_kit::MenuItem::new(&handle, &row.label, row.icon);
                                        item.highlighted = app.sbox.index == rows.len() + index;
                                        let badge = crate::i18n::tr(match row.target {
                                            DesignTarget::Feature(_) => "search.feature",
                                            DesignTarget::Parameter(_) => "search.parameter",
                                        });
                                        item.shortcut = badge;
                                        let response = item.search_row(ui, &query, &row.detail, Some(ui_kit::SearchBadge::Kind));
                                        if item.highlighted && (up || down) {
                                            ui.scroll_to_rect(response.rect, None);
                                        }
                                        if response.clicked() {
                                            picked = Some(rows.len() + index);
                                        }
                                    }
                                    if rows.is_empty() && design.is_empty() {
                                        ui.add_space(t.space_16);
                                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                                            ui_kit::text(
                                                ui,
                                                &crate::i18n::format("search.no_match", &[("query", &query)]),
                                                ui_kit::Text::Heading("search_no_match"),
                                            );
                                            if let Some(label) = closest(&query) {
                                                ui_kit::text(
                                                    ui,
                                                    &crate::i18n::format("search.suggest", &[("label", label)]),
                                                    ui_kit::Text::Report("search_suggest", false),
                                                );
                                            }
                                        });
                                        ui.add_space(t.space_8);
                                    }
                                    if empty {
                                        ui_kit::text(
                                            ui,
                                            &crate::i18n::format("search.try", &[("example", r#"solid.extrude {"distance": 20}"#)]),
                                            ui_kit::Text::Note("search_try"),
                                        );
                                    }
                                }
                            });
                        });
                    });
                });
            });
        });
    if !open {
        return;
    }
    preview(app, now, hovered.filter(|_| app.preferences.ribbon_reveal && app.sbox.moved && menu.is_none()));
    if app.sbox.reveal.is_some() {
        ctx.request_repaint();
    }
    app.sbox.text = query;
    app.sbox.closing_text = app.sbox.text.clone();
    if let Some((at, id)) = menu {
        crate::context_menu::open_for(app, at, crate::context_menu::Target::ToolbarCommand { id });
        return;
    }
    if tab && mode.is_none() {
        if let Some(row) = rows.get(app.sbox.index) {
            app.sbox.text = row.0.to_owned();
        } else if let Some(row) = design.get(app.sbox.index.saturating_sub(rows.len())) {
            app.sbox.text = row.label.clone();
        }
        app.sbox.focus = true;
    }
    if enter || picked.is_some() {
        if mode.is_some() {
            let line = app.sbox.text.clone();
            // Invalid submissions retain the old error, while the field remains editable.
            if let Some((_, Err(error))) = &mode {
                app.set_status(error.clone(), true);
            } else {
                run_typed(app, &line);
                if keep {
                    app.ui.palette_open = true;
                    app.sbox.at = Some(pos);
                    app.sbox.text = line;
                    app.sbox.focus = true;
                }
            }
        } else {
            let index = picked.unwrap_or(app.sbox.index);
            if let Some((id, ..)) = rows
                .get(index)
                .filter(|r| solidcraft_engine::find_command(r.0).is_some_and(|c| crate::render_workspace::interactive_command_info(app, c).enabled))
            {
                app.sbox.used(id);
                close(app);
                app.start(id);
                // The ribbon shows where it was started from: its button or section label pulses.
                if app.preferences.ribbon_reveal {
                    app.sbox.reveal = Some(Reveal { id, since: now, preview: false, back: String::new() });
                }
            } else if let Some(row) = design.get(index.saturating_sub(rows.len())).filter(|_| index >= rows.len()) {
                let target = row.target.clone();
                close(app);
                match target {
                    DesignTarget::Feature(id) => app.edit_feature(id),
                    DesignTarget::Parameter(name) => {
                        app.start("parameters.change");
                        if let Some(dialog) = &mut app.params {
                            dialog.filter = name;
                        }
                    }
                }
            }
        }
    }
    let outside = ctx.input(|i| i.pointer.any_pressed()) && !response.response.contains_pointer();
    if esc || outside {
        close(app);
    }
    if app.ui.palette_open != open {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_scores_prefixes_over_words_over_letters() {
        assert!(score("ext", "Extrude") > score("ext", "Offset Text"));
        assert!(score("pres", "Press Pull") > score("ppl", "Press Pull"));
        assert!(score("ppl", "Press Pull") > 0);
        assert_eq!(score("xyz", "Extrude"), 0);
    }

    fn frame(app: &mut SolveApp, ctx: &egui::Context) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| show(app, ui.ctx()));
        out.textures_delta.clear();
    }

    /// The field takes focus when the box opens and is not asked again on later frames: asking on
    /// every frame restarts the text edit's held key, which kept a Wayland key repeat running (#724).
    #[test]
    fn the_search_field_asks_for_focus_once_per_opening() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let ctx = crate::fonts::test_context();
        let field = egui::Id::new("sc_sbox_text");
        open(&mut app, None);
        frame(&mut app, &ctx);
        frame(&mut app, &ctx);
        assert!(ctx.memory(|m| m.has_focus(field)), "the field has focus once the box opens");
        ctx.memory_mut(|m| m.surrender_focus(field));
        frame(&mut app, &ctx);
        frame(&mut app, &ctx);
        assert!(!ctx.memory(|m| m.has_focus(field)), "the field asked for focus again while open");
        app.ui.palette_open = false;
        frame(&mut app, &ctx);
        open(&mut app, None);
        frame(&mut app, &ctx);
        frame(&mut app, &ctx);
        assert!(ctx.memory(|m| m.has_focus(field)), "reopening the box focuses the field again");
    }

    #[test]
    fn a_reveal_pulses_twice_then_stays_for_a_preview_and_ends_for_a_start() {
        let preview = Reveal { id: "surface.stitch", since: 10.0, preview: true, back: "MODEL".into() };
        let start = Reveal { preview: false, ..preview.clone() };
        for r in [&preview, &start] {
            assert_eq!(r.strength(10.0, true), 0.0);
            assert!((r.strength(10.0 + REVEAL_PULSE / 4.0, true) - 1.0).abs() < 1e-6, "the first blink");
            assert!(r.strength(10.0 + REVEAL_PULSE / 2.0, true) < 1e-6, "dark between the blinks");
        }
        assert_eq!(preview.strength(12.0, true), 1.0, "a hovered result stays marked");
        assert_eq!(start.strength(12.0, true), 0.0, "a started command's pulse ends");
        // Without animations: marked steadily while hovered, no pulse after a start.
        assert_eq!(preview.strength(10.2, false), 1.0);
        assert_eq!(start.strength(10.2, false), 0.0);
    }

    /// Hovering a result shows where its command lives (#7287): the ribbon switches to its tab
    /// and marks its button with its tooltip, or opens the section drop-down that holds it on
    /// its row; leaving the results goes back to the tab the user was on. Starting a command
    /// that lives only in a drop-down marks that section's label while it runs.
    #[test]
    fn hovering_a_result_reveals_its_place_in_the_ribbon() {
        use crate::scenario::rect_of;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.ribbon_reveal = true;
        h.frames(3);
        assert_eq!(h.app.ui.tab, "MODEL");
        let away = rect_of("toolbar").unwrap().center_bottom() + vec2(0.0, 300.0);
        h.call("ui.move", json!({"x": away.x, "y": away.y}));
        h.call("ui.palette", json!({}));
        h.frames(10);
        let hover = |h: &mut crate::scenario::Harness, id: &str| {
            let row = rect_of(&format!("search:command:{id}")).unwrap_or_else(|| panic!("no result row for {id}"));
            h.call("ui.move", json!({"x": row.center().x, "y": row.center().y + 1.0}));
            h.call("ui.move", json!({"x": row.center().x, "y": row.center().y}));
            h.frames(3);
        };

        // Results appearing under a resting pointer reveal nothing: the box opens again with
        // the pointer already where the row will be.
        h.app.sbox.text = "stitch".into();
        h.frames(2);
        let row = rect_of("search:command:surface.stitch").unwrap();
        h.call("ui.key", json!({"key": "Escape"}));
        h.frames(10);
        h.call("ui.move", json!({"x": row.center().x, "y": row.center().y}));
        h.frames(2);
        h.call("ui.palette", json!({}));
        h.app.sbox.text = "stitch".into();
        h.frames(10);
        assert!(h.app.sbox.reveal.is_none() && h.app.ui.tab == "MODEL", "{:?}", h.app.sbox.reveal);

        // A visible button: its tab, the button marked, its tooltip open.
        hover(&mut h, "surface.stitch");
        assert_eq!(h.app.ui.tab, "SURFACE");
        assert_eq!(rect_of("ribbon:reveal"), rect_of("button:surface.stitch"));
        assert!(rect_of("ribbon:reveal_tooltip").is_some());
        h.call("ui.move", json!({"x": away.x, "y": away.y}));
        h.frames(2);
        assert_eq!(h.app.ui.tab, "MODEL", "leaving the results goes back");
        assert!(rect_of("ribbon:reveal").is_none());

        // Only in its section's drop-down: the drop-down opens on its row.
        h.app.sbox.text = "circular".into();
        h.frames(2);
        hover(&mut h, "solid.pattern.circular");
        assert_eq!(h.app.ui.tab, "MODEL");
        assert!(rect_of("button:solid.pattern.circular").is_none(), "the command has no button on the default ribbon");
        let row = rect_of("ribbon:reveal").expect("the revealed row");
        assert!(rect_of("toolbar_dropdown").unwrap().contains_rect(row), "{row:?}");
        assert!(rect_of("ribbon:reveal_tooltip").is_some_and(|tip| tip.left() >= row.right()), "the row's tooltip beside it");
        assert_eq!(h.ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::new("sc_panel_open"))), None, "the preview doesn't open it for good");

        // Starting it from the box: the section label pulses, then stays marked while it runs.
        h.call("ui.key", json!({"key": "Enter"}));
        h.frames(2);
        assert!(!h.app.ui.palette_open);
        assert!(rect_of("toolbar_dropdown").is_none(), "the preview's drop-down closes");
        let label = rect_of("ribbon:section:MODEL:ARRANGE").unwrap();
        assert_eq!(rect_of("ribbon:reveal"), Some(label));
        assert_eq!(rect_of("ribbon:running_section:MODEL:ARRANGE"), Some(label));
        h.frames((REVEAL_PULSE * 60.0) as usize + 2);
        assert!(rect_of("ribbon:reveal").is_none(), "the pulse ends");
        assert_eq!(rect_of("ribbon:running_section:MODEL:ARRANGE"), Some(label), "marked while it runs");
    }

    /// Without the preference (the default) the ribbon stays as it was: hovering a result
    /// neither switches the tab nor opens a drop-down, and a command started from a drop-down
    /// doesn't mark its section label.
    #[test]
    fn without_the_preference_hovering_a_result_leaves_the_ribbon_alone() {
        use crate::scenario::rect_of;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.frames(3);
        assert!(!h.app.preferences.ribbon_reveal, "off by default");
        let away = rect_of("toolbar").unwrap().center_bottom() + vec2(0.0, 300.0);
        h.call("ui.move", json!({"x": away.x, "y": away.y}));
        h.call("ui.palette", json!({}));
        h.frames(10);
        for (query, id) in [("stitch", "surface.stitch"), ("circular", "solid.pattern.circular")] {
            h.app.sbox.text = query.into();
            h.frames(2);
            let row = rect_of(&format!("search:command:{id}")).unwrap_or_else(|| panic!("no result row for {id}"));
            h.call("ui.move", json!({"x": row.center().x, "y": row.center().y + 1.0}));
            h.call("ui.move", json!({"x": row.center().x, "y": row.center().y}));
            h.frames(3);
            assert_eq!(h.app.ui.tab, "MODEL", "{id}");
            assert!(h.app.sbox.reveal.is_none(), "{id}");
            assert!(rect_of("ribbon:reveal").is_none() && rect_of("toolbar_dropdown").is_none(), "{id}");
        }
        h.call("ui.key", json!({"key": "Enter"}));
        h.frames(2);
        assert!(!h.app.ui.palette_open);
        assert!(h.app.sbox.reveal.is_none());
        assert!(rect_of("ribbon:reveal").is_none());
        assert!(rect_of("ribbon:running_section:MODEL:ARRANGE").is_none());
    }

    #[test]
    fn search_finds_by_name_id_and_panel_and_puts_recent_first() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert_eq!(search(&app, "extr").first().map(|x| x.0), Some("solid.extrude"));
        assert_eq!(search(&app, "solid.fil").first().map(|x| x.0), Some("solid.fillet"));
        assert!(search(&app, "constrain").iter().any(|x| x.0 == "sketch.constraint.parallel"), "by panel");
        app.sbox.used("solid.shell");
        app.sbox.used("solid.revolve");
        let all = search(&app, "");
        assert_eq!((all[0].0, all[1].0), ("solid.revolve", "solid.shell"));
        app.sbox.toggle_pin("solid.extrude");
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.sbox.load(&app.sbox.prefs());
        assert_eq!((other.sbox.pinned.clone(), other.sbox.recent.len()), (vec!["solid.extrude".to_string()], 2));
    }

    #[test]
    fn solve_sketch_is_not_offered_but_the_command_remains() {
        let app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        for q in ["solve", "sketch.solve", ""] {
            assert!(!search(&app, q).iter().any(|x| x.0 == "sketch.solve"), "{q:?}");
        }
        assert!(solidcraft_engine::find_command("sketch.solve").is_some(), "agents and MCP keep it");
    }

    /// Commands that take their input as parameters, have no place on the ribbon and open no
    /// dialog or tool are not listed: Enter would run them with no parameters and fail with
    /// "`name` is required" (#9734). Ribbon commands, dialog and tool openers, and commands
    /// that run with no parameters stay listed.
    #[test]
    fn search_lists_only_commands_that_can_run_from_it() {
        let app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let listed = |id: &str| search(&app, "").iter().any(|r| r.0 == id);
        for id in [
            "parameters.add",
            "parameters.rename",
            "parameters.delete",
            "config.row",
            "timeline.rename",
            "component.delete",
            "joint.edit",
            "material.create",
            "browser.group",
        ] {
            assert!(!listed(id), "{id} takes parameters and has no place, dialog or tool");
        }
        for id in ["solid.extrude", "parameters.change", "timeline.play", "edit.repeat", "view.list"] {
            assert!(listed(id), "{id} can be run from the search box");
        }
    }

    /// A command the list leaves out still runs when its id is typed with a JSON object,
    /// as the hint under the field describes (#9734).
    #[test]
    fn a_command_left_out_of_the_list_still_runs_when_typed_with_values() {
        let (id, result) = typed(r#"parameters.rename {"name": "L", "new_name": "W"}"#).unwrap();
        assert_eq!(id, "parameters.rename");
        assert!(result.is_ok());
    }

    /// The box shows each command's label and place in the current language, and finds a name
    /// typed in that language, in English, or an id (#2544).
    #[test]
    fn search_shows_and_matches_the_current_language() {
        let app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let _ja = crate::i18n::scope("ja");
        let ja = crate::i18n::Locale::for_language("ja");
        let name = ja.text("cmd.sketch_rectangle_center");
        assert_ne!(name, "Center Rectangle");
        let place = Some((ja.label("SKETCH"), ja.label("CREATE")));
        for query in [name, "長方形", "center rect", "sketch.rectangle.center"] {
            let rows = search(&app, query);
            let row = rows.iter().find(|r| r.0 == "sketch.rectangle.center").unwrap_or_else(|| panic!("{query:?} doesn't find it"));
            assert_eq!((row.1, row.3), (name, place), "{query:?}");
        }
        assert_eq!(search(&app, name).first().map(|r| r.0), Some("sketch.rectangle.center"), "the exact name comes first");
        // Search shows and matches the redesigned destination, including translated panels.
        for (id, tab, panel) in
            [("solid.extrude", "MODEL", "CREATE"), ("inspect.draft", "INSPECT", "CHECK"), ("sketch.constraint.parallel", "SKETCH", "CONSTRAIN")]
        {
            let c = solidcraft_engine::find_command(id).unwrap();
            let label = ja.label(c.label);
            let place = Some((ja.label(tab), ja.label(panel)));
            for query in [label, c.label, id, ja.label(tab), tab, ja.label(panel), panel] {
                let rows = search(&app, query);
                let row = rows.iter().find(|r| r.0 == id).unwrap_or_else(|| panic!("{query:?} doesn't find {id}"));
                assert_eq!((row.1, row.3), (label, place), "{query:?}: {id}");
            }
        }
        // English is unchanged.
        drop(_ja);
        let row = search(&app, "center rect").into_iter().find(|r| r.0 == "sketch.rectangle.center").unwrap();
        assert_eq!((row.1, row.3), ("Center Rectangle", Some(("SKETCH", "CREATE"))));
    }

    #[test]
    fn a_line_typed_with_json_parameters_runs_the_command() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let before = app.session.doc.features.len();
        app.ui.palette_open = true;
        app.sbox.text = r#"solid.box {"length": 10, "width": 10, "height": 10}"#.into();
        let line = app.sbox.text.clone();
        run_typed(&mut app, &line);
        assert_eq!(app.session.doc.features.len(), before + 1, "{:?}", app.status);
        assert!(!app.ui.palette_open && app.sbox.text.is_empty(), "the box closes");
        assert_eq!(app.sbox.recent.first().map(String::as_str), Some("solid.box"));
        // Bad JSON and an unknown id report instead of running anything.
        run_typed(&mut app, r#"solid.box {"length": "#);
        assert!(app.status.as_ref().is_some_and(|(m, _, err)| *err && m.contains("JSON")), "{:?}", app.status);
        run_typed(&mut app, r#"not.a.command {}"#);
        assert!(app.status.as_ref().is_some_and(|(m, _, err)| *err && m.contains("no command")), "{:?}", app.status);
        assert_eq!(app.session.doc.features.len(), before + 1);
    }

    #[test]
    fn typing_json_in_the_open_box_and_pressing_enter_runs_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.frames(2);
        let before = h.app.session.doc.features.len();
        h.step(&json!({"key": "S"})).unwrap();
        h.step(&json!({"text": r#"solid.box {"length": 10, "width": 10, "height": 10}"#})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.features.len(), before + 1, "{:?}", h.app.status);
        assert!(!h.app.ui.palette_open);
    }
    #[test]
    fn command_mode_preserves_parser_and_keeps_invalid_input_editable() {
        assert!(typed("solid.box").is_none());
        assert_eq!(typed(r#"solid.box {"length":10}"#).unwrap().1.unwrap(), json!({"length":10}));
        assert!(typed("solid.box {}").unwrap().1.is_ok());
        assert!(typed("not.a.command {}").unwrap().1.unwrap_err().contains("no command"));
        let expected = serde_json::from_str::<Value>(r#"{"length": }"#).unwrap_err();
        assert_eq!(
            typed(r#"solid.box {"length": }"#).unwrap().1.unwrap_err(),
            crate::i18n::format("shortcut.json_error", &[("error", &expected.to_string())])
        );
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.palette", json!({"text": r#"solid.box {"length": }"#}));
        h.frames(3);
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.ui.palette_open);
        assert!(h.app.sbox.text.contains("length"));
        assert!(h.app.status.as_ref().is_some_and(|(s, _, error)| *error && s.contains("JSON")));
        assert!(h.app.session.doc.features.is_empty());
    }

    #[test]
    fn shift_enter_runs_json_and_retains_the_query_and_focus() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let query = r#"solid.box {"length":10,"width":10,"height":10}"#;
        h.call("ui.palette", json!({"text":query}));
        h.frames(3);
        h.step(&json!({"key":"Enter", "shift":true})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.features.len(), 1, "{:?}", h.app.status);
        assert!(h.app.ui.palette_open);
        assert_eq!(h.app.sbox.text, query);
        assert!(h.ctx.memory(|m| m.has_focus(egui::Id::new("sc_sbox_text"))));
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.features.len(), 2);
        assert!(!h.app.ui.palette_open);
    }

    #[test]
    fn document_results_open_existing_feature_and_parameter_editors() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":6,"width":6,"height":6})).unwrap();
        h.app.run("parameters.add", json!({"name":"box_width", "expression":"6 mm"})).unwrap();
        let rows = design_rows(&mut h.app, "box");
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.iter().any(|r| matches!(r.target, DesignTarget::Feature(_)) && r.detail.contains("6 mm")));
        assert!(rows.iter().any(|r| matches!(r.target, DesignTarget::Parameter(_)) && r.detail.contains("6 mm")));
        h.call("ui.palette", json!({"text":"box_width"}));
        h.frames(3);
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!h.app.ui.palette_open);
        assert_eq!(h.app.params.as_ref().unwrap().filter, "box_width");
        h.app.params = None;
        h.call("ui.palette", json!({"text":"box"}));
        h.frames(3);
        h.step(&json!({"click":{"handle":"menu:search:design:0"}})).unwrap();
        assert!(!h.app.ui.palette_open);
        assert!(h.app.dialog.is_some(), "a feature opens its existing edit dialog");
    }

    #[test]
    fn tab_fills_selected_registry_id_arrows_choose_and_escape_closes() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.start", json!({"command":"ui.palette"}));
        h.frames(3);
        h.step(&json!({"text":"solid."})).unwrap();
        let first = h.app.sbox.index;
        h.step(&json!({"key":"ArrowDown"})).unwrap();
        assert_ne!(h.app.sbox.index, first);
        h.step(&json!({"key":"ArrowUp"})).unwrap();
        assert_eq!(h.app.sbox.index, first);
        h.step(&json!({"key":"Tab"})).unwrap();
        assert_eq!(h.app.sbox.text, "solid.box");
        assert_eq!(crate::scenario::rect_of("palette:outer"), crate::scenario::rect_of("ribbon:search_popup"));
        assert_eq!(crate::scenario::rect_of("palette:search"), crate::scenario::rect_of("shortcut:search"));
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.app.ui.palette_open && h.app.sbox.text.is_empty());
    }

    #[test]
    fn hostile_queries_and_non_finite_anchors_are_bounded() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        open(&mut h.app, Some(pos2(f32::NAN, f32::INFINITY)));
        h.app.sbox.text = "界".repeat(MAX_QUERY * 2);
        h.frames(3);
        assert_eq!(h.app.sbox.text.chars().count(), MAX_QUERY);
        assert!(crate::scenario::rect_of("ribbon:search_popup").unwrap().is_finite());
        assert!(crate::scenario::handles("menu:").len() <= MAX_RESULTS);
        assert_eq!(closest("fliet"), Some("Fillet"));
        h.call("ui.palette", json!({"open":false}));
        assert!(!h.app.ui.palette_open);
    }
    #[test]
    fn the_closed_field_draws_under_system_windows_and_the_open_sheet_over_them() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.frames(3);
        let order = |h: &crate::scenario::Harness| {
            h.ctx.memory(|m| m.areas().visible_layer_ids().into_iter().find(|l| l.id == egui::Id::new("sc_sbox")).map(|l| l.order))
        };
        // Closed, the field is ribbon chrome: Preferences and the other system windows cover it.
        assert_eq!(order(&h), Some(egui::Order::Background), "the closed field belongs to the ribbon");
        h.call("ui.prefs", json!({"open": "General"}));
        h.frames(3);
        let window = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_prefs"))).unwrap();
        let field = crate::scenario::rect_of("ribbon:search").unwrap();
        assert!(order(&h) < Some(egui::Order::Middle), "a system window draws over the closed field");
        assert!(
            !window.contains(field.center()) || h.ctx.memory(|m| m.layer_id_at(field.center())).is_some_and(|l| l.id == egui::Id::new("sc_prefs"))
        );
        h.call("ui.prefs", json!({"button": "cancel"}));
        // Open, the sheet is a popup again and floats over every window.
        h.call("ui.palette", json!({}));
        h.frames(3);
        assert_eq!(order(&h), Some(egui::Order::Foreground), "the open sheet floats over windows");
    }

    #[test]
    fn ribbon_field_is_one_editor_and_interpolates_with_anchored_right_edge() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.frames(3);
        let closed = crate::scenario::rect_of("ribbon:search").unwrap();
        // Parent bba2183a: 30 pt tab row, 4 pt inset on every side.
        let toolbar = crate::scenario::rect_of("toolbar").unwrap();
        assert_eq!(crate::toolbar::TAB_ROW_HEIGHT, 30.0);
        assert_eq!(crate::toolbar::RIBBON_HEIGHT, 117.0);
        assert_eq!(closed.size(), vec2(ui_kit::CommandSearch::closed_width(&h.ctx), 22.0));
        assert_eq!(closed.right(), toolbar.right() - 4.0);
        assert_eq!(closed.top(), toolbar.top() + 4.0);
        assert_eq!(closed.center().y, toolbar.top() + 15.0);
        h.call("ui.palette", json!({}));
        h.frame();
        let middle = crate::scenario::rect_of("ribbon:search").unwrap();
        assert!(middle.width() > closed.width() && middle.width() < 520.0, "{closed:?} -> {middle:?}");
        assert!(middle.height() > closed.height() && middle.height() < 40.0, "{middle:?}");
        let width_fraction = (middle.width() - closed.width()) / (520.0 - closed.width());
        let height_fraction = (middle.height() - closed.height()) / (40.0 - closed.height());
        assert!((width_fraction - height_fraction).abs() < 0.001);
        assert_eq!(middle.right(), closed.right());
        let unfolding = crate::scenario::rect_of("ribbon:search_popup").unwrap();
        assert!((unfolding.top() - middle.bottom() - egui::emath::GuiRounding::round_ui(Tokens::get().space_6 * width_fraction)).abs() < 0.001);
        h.frames(10);
        let field = crate::scenario::rect_of("ribbon:search").unwrap();
        assert_eq!(field.size(), vec2(520.0, 40.0));
        assert_eq!(field.right(), closed.right());
        assert_eq!(Some(field), crate::scenario::rect_of("shortcut:search"));
        assert_eq!(Some(field), crate::scenario::rect_of("palette:search"));
        let sheet = crate::scenario::rect_of("ribbon:search_popup").unwrap();
        assert_eq!(sheet.top(), field.bottom() + Tokens::get().space_6);
        assert_eq!(sheet.x_range(), field.x_range());
        assert!(sheet.height() > unfolding.height());
        let footer = crate::scenario::rect_of("ribbon:search_footer").unwrap();
        assert_eq!(footer, Rect::from_min_max(pos2(sheet.left(), footer.top()), sheet.right_bottom()));
        // Inspect native accessibility widgets, not just aliases or painted labels.
        let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
        let search_edit = h.ctx.read_response(egui::Id::new("sc_sbox_text")).unwrap().rect;
        let fields: Vec<_> = nodes
            .iter()
            .filter(|(_, n)| n.role() == egui::accesskit::Role::TextInput)
            .filter(|(_, n)| n.bounds().is_some_and(|b| b.y0 >= f64::from(field.top()) && b.y1 <= f64::from(sheet.bottom())))
            .collect();
        assert_eq!(fields.len(), 1, "one editor in the field and sheet: {fields:?}");
        assert!(field.contains_rect(search_edit));
        let editor_id = fields[0].0;
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frame();
        let shrinking = crate::scenario::rect_of("ribbon:search").unwrap();
        assert!(shrinking.width() > closed.width() && shrinking.width() < field.width());
        assert!(shrinking.height() > closed.height() && shrinking.height() < 40.0);
        assert_eq!(shrinking.right(), closed.right());
        h.frames(10);
        assert_eq!(crate::scenario::rect_of("ribbon:search").unwrap(), closed);
        h.step(&json!({"click":{"handle":"ribbon:search"}})).unwrap();
        h.frames(10);
        assert!(h.app.ui.palette_open);
        let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
        assert!(nodes.iter().any(|(id, n)| *id == editor_id && n.role() == egui::accesskit::Role::TextInput));
    }

    #[test]
    fn hovering_the_closed_and_open_search_field_uses_the_text_cursor() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.frames(3);
        for open in [false, true] {
            if open {
                h.call("ui.palette", json!({}));
                h.frames(10);
            }
            let field = crate::scenario::rect_of("ribbon:search").unwrap();
            // Cover the icon, editor and keycap, including the closed click overlay.
            for x in [field.left() + 12.0, field.center().x, field.right() - 12.0] {
                h.step(&json!({"move": [x, field.center().y]})).unwrap();
                h.frames(2);
                assert_eq!(h.output.cursor_icon, egui::CursorIcon::Text, "open={open}, x={x}");
            }
        }
    }

    #[test]
    fn search_respects_instant_animation_and_click_away_reverses_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.ctx.global_style_mut(|style| style.animation_time = 0.0);
        h.frames(3);
        let closed = crate::scenario::rect_of("ribbon:search").unwrap();
        h.step(&json!({"click":{"handle":"ribbon:search"}})).unwrap();
        h.frames(2);
        assert!(h.app.ui.palette_open);
        let field = crate::scenario::rect_of("ribbon:search").unwrap();
        assert_eq!(field.size(), vec2(520.0, 40.0));
        assert_eq!(field.right(), closed.right());
        h.step(&json!({"click":[10,600]})).unwrap();
        h.frames(2);
        assert!(!h.app.ui.palette_open);
        assert_eq!(crate::scenario::rect_of("ribbon:search").unwrap(), closed);
    }

    #[test]
    fn saved_pins_remain_visible_without_a_query_or_recent_commands() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.sbox.load(&json!({"pinned":["solid.extrude"]}));
        h.call("ui.palette", json!({}));
        h.frames(3);
        assert!(h.app.sbox.recent.is_empty());
        assert!(crate::scenario::rect_of("menu:solid.extrude").is_some());
        assert_eq!(crate::scenario::handles("menu:").len(), 1);
    }
}
