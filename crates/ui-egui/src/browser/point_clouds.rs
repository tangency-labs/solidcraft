//! The Point Clouds folder of the Browser (#5151): an eye per cloud and for the folder, a menu,
//! and the Edit Point Cloud card: the file's units, the size of the dots, how the cloud is
//! coloured (its colours, one colour, a ramp over a property, its range on a track, or the
//! palette of a label, with its legend), and its filters (a range on a track, played with ▶ or
//! Space, or the values of a label picked in its legend). Each track shows how much of the cloud
//! lies at each value, over what the other filters keep. The live edits of one opening are one
//! undo step.

use serde_json::{Value, json};
use solidcraft_engine::doc::point_cloud::{COORDINATES, ColorBy, Filter, Keep, MAX_FILTERS, Names, PLAIN, PointCloud, UNITS, byte, label_color};
use solidcraft_engine::view::POINT_CLOUDS_FOLDER;

use super::{Actions, Row, draw_row, is_open, panel_pos, toggle, toggle_folder};
use crate::SolveApp;
use crate::context_menu::{Item, Target};
use crate::ui_kit;

#[path = "point_cloud_tracks.rs"]
pub(crate) mod point_cloud_tracks;
#[path = "range_track.rs"]
mod range_track;
use point_cloud_tracks::{Ask, Paint, Tracks};
use range_track::{Part, Track, range_track};

/// Filters the GPU applies; the others are kept but not drawn.
pub const DRAWN_FILTERS: usize = solidcraft_gpu::view::FILTERS;

/// The Edit Point Cloud card: the cloud, the undo depth when it opened, what it worked out once
/// for the cloud's data (the automatic height range), its tracks' bars, and a filter playing.
#[derive(Clone, Debug)]
pub struct CloudPanel {
    pub cloud: u64,
    pub(crate) depth: usize,
    auto: Option<(usize, [u64; 16], Option<[f64; 2]>)>,
    tracks: Tracks,
    pub play: Option<Play>,
}

/// A range filter moving on its own: the part of it touched last moves forward (its upper end
/// unrolls, its band slides, its lower end wipes), across its track's loupe in [`PLAY_SECONDS`].
/// It is written to the design once, when it stops.
#[derive(Clone, Debug)]
pub struct Play {
    pub filter: usize,
    part: Part,
    /// When it last moved, and where it is exactly.
    at: f64,
    exact: [f64; 2],
    /// The range now (a whole-numbered property's ends whole).
    pub now: [f64; 2],
}

/// How long playing a filter across its track's loupe takes, in seconds: zooming in slows it.
const PLAY_SECONDS: f64 = 20.0;

/// The names of a cloud's tracks.
fn filter_key(cloud: u64, k: usize) -> String {
    format!("point_cloud_{cloud}_filter_{k}")
}
fn colour_key(cloud: u64) -> String {
    format!("point_cloud_{cloud}_colour")
}

impl Play {
    /// Start playing `part` of `range` within `span`, from the start again when it is at the end.
    fn new(filter: usize, part: Part, [lo, hi]: [f64; 2], span: [f64; 2], at: f64) -> Play {
        let exact = match part {
            Part::High if hi >= span[1] => [lo, lo],
            Part::Band if hi >= span[1] => [span[0], span[0] + hi - lo],
            Part::Low if lo >= hi => [span[0], hi],
            _ => [lo, hi],
        };
        Play { filter, part, at, exact, now: exact }
    }
    /// Move on to time `now` at `speed` units a second; whether it reached the end.
    fn step(&mut self, now: f64, speed: f64, span: [f64; 2], integer: bool) -> bool {
        let d = speed * (now - self.at).max(0.0);
        self.at = now;
        let [lo, hi] = self.exact;
        let (exact, end) = match self.part {
            Part::High => ([lo, (hi + d).min(span[1])], hi + d >= span[1]),
            Part::Low => ([(lo + d).min(hi), hi], lo + d >= hi),
            Part::Band => {
                let w = hi - lo;
                let start = (lo + d).min(span[1] - w);
                ([start, start + w], lo + d >= span[1] - w)
            }
        };
        self.exact = exact;
        self.now = if integer { exact.map(f64::floor) } else { exact };
        end
    }
}

impl CloudPanel {
    pub fn new(app: &SolveApp, cloud: u64) -> Self {
        CloudPanel { cloud, depth: app.session.undo.len(), auto: None, tracks: Tracks::default(), play: None }
    }
    /// The cloud's automatic height range, worked out again only when its points or placement
    /// change.
    fn auto_range(&mut self, c: &PointCloud) -> Option<[f64; 2]> {
        let key = (std::sync::Arc::as_ptr(c.points.values()).cast::<f32>() as usize, std::array::from_fn(|i| c.transform[i / 4][i % 4].to_bits()));
        match &self.auto {
            Some((p, t, r)) if (*p, *t) == key => *r,
            _ => {
                let r = c.auto_height_range();
                self.auto = Some((key.0, key.1, r));
                r
            }
        }
    }
    /// The range filter `k` of this card's cloud shows now, while it plays.
    pub fn playing(&self, cloud: u64, k: usize) -> Option<[f64; 2]> {
        self.play.as_ref().filter(|p| self.cloud == cloud && p.filter == k).map(|p| p.now)
    }
}

/// The ways the card colours the points, as the dropdown lists them.
const COLOR_BY: [(ColorBy, &str); 3] = [(ColorBy::File, "File Colors"), (ColorBy::Single, "Single Color"), (ColorBy::Height, "Height")];

/// The names of `property` when it is filtered by a set of values: a label whose values span
/// fewer than the 32 the view's mask holds. Any other property is filtered by a range.
fn label<'a>(c: &'a PointCloud, property: &str) -> Option<&'a Names> {
    let names = c.complex.names(property)?;
    let (lo, hi) = names.values.iter().fold((i32::MAX, i32::MIN), |(lo, hi), (v, _)| (lo.min(*v), hi.max(*v)));
    (i64::from(hi) - i64::from(lo) < 32).then_some(names)
}

/// Bins of a track's bars: about one a point across the card.
fn bins(ui: &egui::Ui) -> usize {
    (ui.available_width().round() as usize).clamp(16, 1024)
}

/// The filters as `pointcloud.edit` takes them.
fn filters_json(filters: &[Filter]) -> Vec<Value> {
    filters.iter().map(|f| serde_json::to_value(f).unwrap_or(Value::Null)).collect()
}

/// The filters with `value` of the label `property` shown or hidden: in its set filter, or in a
/// new one keeping every named value but it.
fn toggled(c: &PointCloud, names: &Names, value: i32) -> Vec<Value> {
    let mut filters = c.filters.clone();
    match filters.iter_mut().find(|f| f.property == names.property && matches!(f.keep, Keep::Values { .. })) {
        Some(Filter { keep: Keep::Values { values }, .. }) => {
            if let Some(k) = values.iter().position(|v| *v == value) {
                values.remove(k);
            } else {
                values.push(value);
            }
        }
        _ => filters.push(Filter {
            property: names.property.clone(),
            keep: Keep::Values { values: names.values.iter().map(|(v, _)| *v).filter(|v| *v != value).collect() },
        }),
    }
    filters_json(&filters)
}

/// A label's legend: a swatch and a name per value, dimmed when a filter hides it. The value
/// clicked, to show or hide.
fn legend(ui: &mut egui::Ui, c: &PointCloud, names: &Names) -> Option<i32> {
    let kept = c.filters.iter().find_map(|f| match &f.keep {
        Keep::Values { values } if f.property == names.property => Some(values.clone()),
        _ => None,
    });
    let mut clicked = None;
    for (v, name) in &names.values {
        let shown = kept.as_ref().is_none_or(|k| k.contains(v));
        let colour = crate::theme::model_color(label_color(f64::from(*v)));
        let swatch = if shown { colour } else { colour.gamma_multiply(0.16) };
        let response = ui
            .horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, swatch);
                let text = egui::RichText::new(name.as_str());
                ui.add(egui::Label::new(if shown { text } else { text.weak().strikethrough() }).sense(egui::Sense::click()))
            })
            .inner;
        if response.clicked() {
            clicked = Some(*v);
        }
    }
    clicked
}

pub(super) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let clouds: Vec<(u64, String, bool)> = app.session.doc.point_clouds.iter().map(|c| (c.id, c.name.clone(), c.visible)).collect();
    if clouds.is_empty() {
        return;
    }
    let key = "point_clouds";
    let open = is_open(app, key, true);
    // The folder's eye is its own: each cloud keeps its eye while the folder hides them all.
    let shown = !app.session.folder_hidden(POINT_CLOUDS_FOLDER);
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row { depth, fold: Some(open), eye: Some(shown), icon: "folder", label: "Point Clouds", dim: !shown, ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, true);
    }
    if r.eye {
        toggle_folder(app, POINT_CLOUDS_FOLDER);
    }
    if !open {
        return;
    }
    for (id, name, visible) in clouds {
        let r = draw_row(
            ui,
            ui.id().with(("point_cloud", id)),
            &Row { depth: depth + 1, eye: Some(visible), icon: "point", label: &name, name: true, dim: !visible || !shown, ..Default::default() },
        );
        if r.eye {
            let _ = app.run("pointcloud.edit", json!({ "cloud": id, "visible": !visible }));
        }
        if r.double {
            app.tree.cloud_panel = Some(CloudPanel::new(app, id));
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::PointCloud { id }));
        }
    }
}

/// The menu of a point cloud.
pub fn items(app: &SolveApp, id: u64) -> Vec<Item> {
    let visible = app.session.doc.point_clouds.iter().find(|c| c.id == id).is_some_and(|c| c.visible);
    vec![
        Item::action("ui.editPointCloud", "Edit Point Cloud", "edit").with(json!({ "cloud": id })),
        Item::sep(),
        Item::action("pointcloud.delete", "Delete", "delete").with(json!({ "cloud": id })),
        Item::sep(),
        Item::action("pointcloud.edit", "Show/Hide", "eye").key("V").with(json!({ "cloud": id, "visible": !visible })),
    ]
}

pub(crate) fn panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut m) = app.tree.cloud_panel.clone() else { return };
    let Some(c) = app.session.doc.point_clouds.iter().find(|c| c.id == m.cloud).cloned() else {
        app.tree.cloud_panel = None;
        return;
    };
    let mut edit: Option<Value> = None;
    let title = crate::i18n::format("browser.edit_point_cloud_title", &[("name", &c.name)]);
    let auto = if c.color_by == ColorBy::Height { m.auto_range(&c) } else { None };
    let now = ctx.input(|i| i.time);
    // A playing filter moves on; at the end of its span it stops, and is written once.
    let mut stop_play = false;
    if let Some(p) = m.play.as_mut()
        && let Some(span) = c.filters.get(p.filter).and_then(|f| c.span(&f.property))
    {
        let integer = c.filters.get(p.filter).is_some_and(|f| c.complex.integer(&f.property));
        let [a, b] = range_track::loupe(ctx, &filter_key(c.id, p.filter), span);
        stop_play = p.step(now, (b - a) / PLAY_SECONDS, span, integer);
        ctx.request_repaint();
    }
    // The cloud as the view draws it now, a playing filter where it is: the filters the tracks
    // apply, how their bars are coloured, as the cloud is, and the values they snap to, where a
    // label changes.
    let live = crate::point_clouds::as_drawn(&c, |k| m.playing(c.id, k)).into_owned();
    let drawn: Vec<Filter> = live.filters.iter().take(DRAWN_FILTERS).cloned().collect();
    let paint = match (c.colored_property(), crate::point_clouds::color_range(ctx, &live)) {
        (Some(n), _) if c.complex.integer(n) => Paint::Palette { property: n.to_string() },
        (Some(n), Some(range)) => Paint::Ramp { property: n.to_string(), range },
        _ => Paint::One,
    };
    let changes_of = c.colored_property().filter(|n| c.complex.integer(n)).map(str::to_string);
    let at = panel_pos(app);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_point_cloud_panel"), "point", &title)
        .placed(egui::Align2::LEFT_TOP, at)
        .fields()
        .actions("", crate::i18n::tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui_kit::row(ui, crate::i18n::tr_label("Points"), |ui| ui.label(c.len().to_string()));
            let mut size = c.point_size;
            if ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_size"), |ui| ui_kit::slider(ui, &mut size, 0.5..=16.0, Default::default())).changed() {
                edit = Some(json!({ "cloud": c.id, "point_size": size }));
            }
            let units: Vec<&str> = UNITS.iter().map(|(u, _)| *u).collect();
            let was = units.iter().position(|u| *u == c.units).unwrap_or(0);
            let mut unit = was;
            // A dropdown's response doesn't report the pick: compare the index.
            ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_units"), |ui| ui_kit::dropdown(ui, "point_cloud_units", &mut unit, &units));
            if unit != was
                && let Some(u) = units.get(unit)
            {
                edit = Some(json!({ "cloud": c.id, "units": u }));
            }
            // The ways of colouring, then every property of the file.
            let properties: Vec<&str> = c.property_names().filter(|n| !COORDINATES.contains(n)).collect();
            let labels: Vec<&str> = COLOR_BY.iter().map(|(_, l)| *l).chain(properties.iter().copied()).collect();
            let was = match c.color_by {
                ColorBy::Property => {
                    c.color_property.as_deref().and_then(|n| properties.iter().position(|p| *p == n)).map_or(0, |k| COLOR_BY.len() + k)
                }
                by => COLOR_BY.iter().position(|(b, _)| *b == by).unwrap_or(0),
            };
            let mut by = was;
            ui_kit::row(ui, crate::i18n::tr("ui.point_cloud_color_by"), |ui| ui_kit::dropdown(ui, "point_cloud_color_by", &mut by, &labels));
            if by != was {
                let name = COLOR_BY.get(by).map(|(b, _)| b.name()).or_else(|| properties.get(by - COLOR_BY.len()).copied());
                if let Some(name) = name {
                    edit = Some(json!({ "cloud": c.id, "color_by": name }));
                }
            }
            let colored = c.colored_property();
            let legend_shown = colored.and_then(|n| label(&c, n));
            match c.color_by {
                ColorBy::File if c.colors.is_empty() => {
                    ui_kit::row(ui, "", |ui| ui.weak(crate::i18n::tr("ui.point_cloud_no_file_colors")));
                }
                ColorBy::File => {}
                ColorBy::Single => {
                    let mut rgb = c.color.unwrap_or(PLAIN).map(byte);
                    if ui_kit::row(ui, crate::i18n::tr_label("Colour"), |ui| ui_kit::color_field(ui, &mut rgb)).changed() {
                        edit = Some(json!({ "cloud": c.id, "color": rgb.map(|v| f64::from(v) / 255.0) }));
                    }
                }
                ColorBy::Height | ColorBy::Property => {
                    let name = colored.unwrap_or_default();
                    if let Some(names) = legend_shown {
                        // A label: its legend, which also shows and hides its values.
                        if let Some(v) = legend(ui, &c, names) {
                            edit = Some(json!({ "cloud": c.id, "filters": toggled(&c, names, v) }));
                        }
                    } else if !c.complex.integer(name)
                        && let Some(span) = if c.color_by == ColorBy::Height { c.span("z").or(auto) } else { c.span(name) }
                    {
                        // A ramp over what is shown, unless its range is set.
                        let automatic = c.height_range.is_none();
                        if ui_kit::checkbox_row(ui, "point_cloud_auto_range", crate::i18n::tr("ui.point_cloud_auto_range"), automatic) {
                            edit = Some(json!({ "cloud": c.id, "color_range": if automatic { json!(crate::point_clouds::color_range(ui.ctx(), &live)) } else { Value::Null } }));
                        }
                        // The ramp's range is a range on a track of the property, over what the
                        // filters keep; a double click beside it gives it back to the filters.
                        let shown = crate::point_clouds::color_range(ui.ctx(), &live).unwrap_or(span);
                        let key = colour_key(c.id);
                        let ask = Ask {
                            property: name.to_string(),
                            span,
                            loupe: range_track::loupe(ui.ctx(), &key, span),
                            bins: bins(ui),
                            paint: Paint::Ramp { property: name.to_string(), range: shown },
                            filters: drawn.clone(),
                            changes_of: None,
                        };
                        let answer = m.tracks.answer(ui.ctx(), &c, &key, ask);
                        let out = range_track(
                            ui,
                            &Track {
                                key: &key,
                                range: shown,
                                span,
                                integer: false,
                                overview: answer.as_ref().map(|a| &a.overview),
                                loupe: answer.as_ref().map(|a| &a.loupe),
                                changes: &[],
                                // An automatic range moves with a playing filter: its moving end
                                // stays in the loupe.
                                follow: m.play.as_ref().filter(|_| automatic).map(|p| if p.part == Part::Low { shown[0] } else { shown[1] }),
                            },
                        );
                        if let Some(range) = out.range {
                            edit = Some(json!({ "cloud": c.id, "color_range": range }));
                        } else if out.whole {
                            edit = Some(json!({ "cloud": c.id, "color_range": Value::Null }));
                        }
                    }
                }
            }
            // Filters: only what every one of them keeps is shown.
            let mut filters = c.filters.clone();
            let mut changed = false;
            let mut removed = None;
            for (k, f) in c.filters.iter().enumerate() {
                let span = c.span(&f.property);
                let integer = c.complex.integer(&f.property);
                // Play or stop, by ▶ or Space on the track.
                let mut toggle = None;
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr_dyn(&format!("Filter: {}", f.property)));
                    let playing = m.play.as_ref().is_some_and(|p| p.filter == k);
                    if let (Some(range), Some(span)) = (f.range(), span)
                        && ui_kit::button(
                            ui,
                            if playing {
                                format!("⏸ {}", crate::i18n::tr_label("Pause"))
                            } else {
                                format!("▶ {}", crate::i18n::tr_label("Play"))
                            },
                            ui_kit::Button::Small,
                        )
                        .clicked()
                    {
                        toggle = Some((m.playing(c.id, k).unwrap_or(range), span));
                    }
                    if ui_kit::button(ui, "×", ui_kit::Button::Small).clicked() {
                        removed = Some(k);
                    }
                });
                if k >= DRAWN_FILTERS {
                    ui.weak(crate::i18n::tr_dyn(&format!("Not drawn: the view applies {DRAWN_FILTERS} filters at most")));
                }
                match (&f.keep, span) {
                    (Keep::Values { .. }, _) => match label(&c, &f.property) {
                        Some(names) if legend_shown.is_some_and(|l| l.property == names.property) => {
                            ui.weak(crate::i18n::tr_label("Shown and hidden in the legend above"));
                        }
                        Some(names) => {
                            if let Some(v) = legend(ui, &c, names) {
                                edit = Some(json!({ "cloud": c.id, "filters": toggled(&c, names, v) }));
                            }
                        }
                        None => {}
                    },
                    (Keep::Range { range }, Some(span)) => {
                        let shown = m.playing(c.id, k).unwrap_or(*range);
                        let key = filter_key(c.id, k);
                        // The track shows what the other filters keep.
                        let others = drawn.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, f)| f.clone()).collect();
                        let ask = Ask {
                            property: f.property.clone(),
                            span,
                            loupe: range_track::loupe(ui.ctx(), &key, span),
                            bins: bins(ui),
                            paint: paint.clone(),
                            filters: others,
                            changes_of: changes_of.clone(),
                        };
                        let answer = m.tracks.answer(ui.ctx(), &c, &key, ask);
                        let moving = m.play.as_ref().filter(|p| p.filter == k).map(|p| if p.part == Part::Low { shown[0] } else { shown[1] });
                        let out = range_track(
                            ui,
                            &Track {
                                key: &key,
                                range: shown,
                                span,
                                integer,
                                overview: answer.as_ref().map(|a| &a.overview),
                                loupe: answer.as_ref().map(|a| &a.loupe),
                                changes: answer.as_ref().map_or(&[][..], |a| &a.changes[..]),
                                follow: moving,
                            },
                        );
                        let moved = out.range.or(out.whole.then_some(span));
                        if let Some(range) = moved
                            && let Some(slot) = filters.get_mut(k)
                        {
                            slot.keep = Keep::Range { range };
                            changed = true;
                            // Moving it by hand stops it playing.
                            if m.play.as_ref().is_some_and(|p| p.filter == k) {
                                m.play = None;
                            }
                        }
                        if out.play {
                            toggle = Some((moved.unwrap_or(shown), span));
                        }
                    }
                    (Keep::Range { .. }, None) => {}
                }
                if let Some((range, span)) = toggle {
                    if m.play.as_ref().is_some_and(|p| p.filter == k) {
                        stop_play = true;
                    } else {
                        // Another filter playing stops where it is.
                        if let Some(p) = m.play.take()
                            && let Some(slot) = filters.get_mut(p.filter)
                        {
                            slot.keep = Keep::Range { range: p.now };
                            changed = true;
                        }
                        let part = range_track::last_part(ui.ctx(), &filter_key(c.id, k));
                        m.play = Some(Play::new(k, part, range, span, now));
                    }
                }
            }
            if let Some(k) = removed {
                filters.remove(k);
                changed = true;
                m.play = None;
            }
            let all: Vec<&str> = c.property_names().collect();
            if c.filters.len() < MAX_FILTERS {
                let mut add_labels = vec![crate::i18n::tr_label("Add filter…")];
                add_labels.extend(all.iter().copied());
                let mut pick = 0;
                ui_kit::row(ui, crate::i18n::tr_label("Filter by"), |ui| ui_kit::dropdown(ui, "point_cloud_add_filter", &mut pick, &add_labels));
                if let Some(name) = all.get(pick.wrapping_sub(1)) {
                    // A label keeps all its values at first, anything else its whole span.
                    let keep = match (label(&c, name), c.span(name)) {
                        (Some(names), _) => Some(Keep::Values { values: names.values.iter().map(|(v, _)| *v).collect() }),
                        (None, Some(span)) => Some(Keep::Range { range: span }),
                        (None, None) => None,
                    };
                    if let Some(keep) = keep {
                        filters.push(Filter { property: name.to_string(), keep });
                        changed = true;
                    }
                }
            }
            if changed {
                edit = Some(json!({ "cloud": c.id, "filters": filters_json(&filters) }));
            }
        });
    // A filter that stops playing is written where it stopped.
    if stop_play && let Some(p) = m.play.take() {
        let mut filters = c.filters.clone();
        if let Some(f) = filters.get_mut(p.filter) {
            f.keep = Keep::Range { range: p.now };
            edit = Some(json!({ "cloud": c.id, "filters": filters_json(&filters) }));
        }
    }
    if let Some(p) = edit
        && app.run("pointcloud.edit", p).is_ok()
    {
        // One undo step for the whole edit.
        if app.session.undo.len() > m.depth + 1 {
            let keep = app.session.undo.get(m.depth).cloned();
            app.session.undo.truncate(m.depth);
            app.session.undo.extend(keep);
        }
    }
    app.tree.cloud_panel = if closed || action == ui_kit::Action::Cancel { None } else { Some(m) };
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_cloud_has_a_menu_and_a_card_that_closes() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let dir = std::env::temp_dir().join(format!("solidcraft-browser-cloud-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scan.xyz");
        std::fs::write(
            &path,
            "0 0 0 255 0 0
1 0 0 0 255 0
",
        )
        .unwrap();
        let id = h.app.run("pointcloud.insert", json!({"path": path.to_string_lossy()})).unwrap()["cloud"].as_u64().unwrap();
        let items = super::items(&h.app, id);
        assert!(items.iter().any(|i| i.id == "pointcloud.delete"));
        h.app.tree.cloud_panel = Some(super::CloudPanel::new(&h.app, id));
        h.frames(4);
        assert!(h.widget("Close").is_some());
        // Each way of colouring shows its own rows.
        for by in ["single", "height", "file"] {
            h.app.run("pointcloud.edit", json!({"cloud": id, "color_by": by})).unwrap();
            h.frames(2);
        }
        h.app.run("pointcloud.edit", json!({"cloud": id, "color_by": "height"})).unwrap();
        h.frames(2);
        assert!(h.widget(crate::i18n::tr("ui.point_cloud_auto_range")).is_some());
        h.step(&json!({"widget": "Close"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.cloud_panel.is_none());
        assert_eq!(h.app.session.doc.point_clouds.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A PLY file of points along x, `t` running 0 to 10 by halves, a layer every 2: the layer
    /// changes at t = 2, 4, 6 and 8.
    fn layered_points(dir: &std::path::Path) -> std::path::PathBuf {
        let mut ply = String::from(
            "ply\nformat ascii 1.0\nelement vertex 21\nproperty float x\nproperty float y\nproperty float z\nproperty float t\nproperty int layer\nend_header\n",
        );
        for i in 0..=20 {
            let t = f64::from(i) / 2.0;
            ply += &format!("{t} 0 0 {t} {}\n", (i / 4).min(4));
        }
        let path = dir.join("layers.ply");
        std::fs::write(&path, ply).unwrap();
        path
    }

    #[test]
    fn a_filter_track_draws_snaps_steps_frames_and_plays() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let dir = std::env::temp_dir().join(format!("solidcraft-browser-track-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = layered_points(&dir);
        let id = h.app.run("pointcloud.insert", json!({"path": path.to_string_lossy()})).unwrap()["cloud"].as_u64().unwrap();
        h.app.run("pointcloud.edit", json!({"cloud": id, "color_by": "layer", "filters": [{"property": "t", "range": [2.0, 5.0]}]})).unwrap();
        h.app.tree.cloud_panel = Some(super::CloudPanel::new(&h.app, id));
        let key = super::filter_key(id, 0);
        let range = |h: &crate::scenario::Harness| h.app.session.doc.point_clouds[0].filters[0].range().unwrap();
        // The bars come from the worker: wait for them, and for the layer changes to snap to.
        let mut bars = None;
        for _ in 0..200 {
            h.frames(1);
            bars = crate::scenario::rect_of(&format!("range_track:{key}:bars"));
            if bars.is_some() && h.app.tree.cloud_panel.as_ref().is_some_and(|m| m.tracks.answered(&key)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        h.frames(2);
        let r = bars.expect("the track is drawn");
        let x = |t: f64| f64::from(r.left()) + t / 10.0 * f64::from(r.width());
        let y = f64::from(r.center().y);
        let drag = |h: &mut crate::scenario::Harness, a: f64, b: f64| {
            h.call("ui.drag", json!({"x0": x(a), "y0": y, "x1": x(b), "y1": y, "steps": 8}));
            h.frames(2);
        };
        // Drawn beside the range: a new range, its start snapped to the layer change at 6.
        drag(&mut h, 6.04, 8.9);
        let [lo, hi] = range(&h);
        assert!(lo == 6.0 && (hi - 8.9).abs() < 0.05, "{lo} {hi}");
        // Its end dragged near 8 snaps to it; its start, far from any change, does not.
        drag(&mut h, 8.9, 7.97);
        drag(&mut h, 6.0, 6.5);
        let [lo, hi] = range(&h);
        assert!((lo - 6.5).abs() < 0.05 && hi == 8.0, "{lo} {hi}");
        // ← steps the start, touched last, to the change before it, then the one before that.
        for _ in 0..2 {
            h.call("ui.key", json!({"key": "ArrowLeft"}));
            h.frames(2);
        }
        assert_eq!(range(&h), [4.0, 8.0]);
        // A double click on the range frames the loupe on it; beside it, the whole span.
        h.call("ui.click", json!({"x": x(6.0), "y": y, "double": true}));
        h.frames(2);
        let [a, b] = super::range_track::loupe(&h.ctx, &key, [0.0, 10.0]);
        assert!(a > 3.0 && a < 4.0 && b > 8.0 && b < 9.0, "{a} {b}");
        // A double click on the strip above shows the whole span again; beside the range, the
        // range becomes the whole span.
        // (Past the double click's time, so that it is not a third click.)
        h.frames(30);
        let strip = crate::scenario::rect_of(&format!("range_track:{key}:strip")).unwrap();
        h.call("ui.click", json!({"x": f64::from(strip.center().x), "y": f64::from(strip.center().y), "double": true}));
        h.frames(2);
        assert_eq!(super::range_track::loupe(&h.ctx, &key, [0.0, 10.0]), [0.0, 10.0]);
        h.frames(30);
        h.call("ui.click", json!({"x": x(9.5), "y": y, "double": true}));
        h.frames(2);
        assert_eq!(range(&h), [0.0, 10.0]);
        // Space plays the part touched last (here the end, unrolling), written once when it stops.
        drag(&mut h, 10.0, 7.0);
        assert!((range(&h)[1] - 7.0).abs() < 0.05);
        h.call("ui.key", json!({"key": "Space"}));
        h.frames(2);
        assert!(h.app.tree.cloud_panel.as_ref().unwrap().play.is_some());
        h.frames(20);
        h.call("ui.key", json!({"key": "Space"}));
        h.frames(2);
        assert!(h.app.tree.cloud_panel.as_ref().unwrap().play.is_none());
        let [lo, hi] = range(&h);
        assert!(lo == 0.0 && hi > 7.0 && hi < 10.0, "played on from 7: {hi}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
