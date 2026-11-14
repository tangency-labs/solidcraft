//! The Pipe Routes window: the UI side of the `route.*` commands (`cmd/route.rs` in the engine).
//! New Route takes a name, the pipe spec (size, schedule, elbow) and the centreline points, one
//! `x, y, z` per line, and runs `route.create`; picking a route shows its pieces and cut list, and
//! moves one of its points (`route.move_point`), changes its spec (`route.edit`) or deletes it.
//! Starting any `route.*` command from the toolbar or the command search opens it.

use egui::RichText;
use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit;
use solidcraft_theme::Tokens;

/// The sizes, schedules and elbows the window offers (`route.fittings` has them all).
const SIZES: &[&str] = &["1/2 in", "3/4 in", "1 in", "1 1/4 in", "1 1/2 in", "2 in", "2 1/2 in", "3 in", "4 in"];
const NPS: &[f64] = &[0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0];
const SCHEDULES: &[&str] = &["Sch 10S", "Sch 40", "Sch 80"];
const SCHEDULE_IDS: &[&str] = &["10S", "40", "80"];
const ELBOWS: &[&str] = &["Long radius", "Short radius"];
const ELBOW_IDS: &[&str] = &["long_radius", "short_radius"];

/// The window's state; the routes come from the engine.
pub struct RoutePanel {
    pub name: String,
    pub size: usize,
    pub schedule: usize,
    pub elbow: usize,
    /// The centreline, one `x, y, z` per line (mm).
    pub points: String,
    /// The picked route's name, its point to move, and the move.
    pub picked: Option<String>,
    pub point: usize,
    pub delta: [String; 3],
    pub carry: bool,
    /// The routes for a design tab and revision, and the picked route's details.
    cache: Option<((u64, u64), Value, Value)>,
    /// What the last action did, or why it was refused.
    pub status: Option<(String, bool)>,
}

impl Default for RoutePanel {
    fn default() -> Self {
        RoutePanel {
            name: "Route1".into(),
            size: 2,
            schedule: 1, // Sch 40, the default before Sch 10S was offered
            elbow: 0,
            points: "0, 0, 0\n1000, 0, 0\n1000, 800, 0".into(),
            picked: None,
            point: 0,
            delta: ["100".into(), "0".into(), "0".into()],
            carry: true,
            cache: None,
            status: None,
        }
    }
}

/// Open the window (any `route.*` command from the toolbar or the command search).
pub fn open(app: &mut SolveApp) {
    if app.routes.is_none() {
        app.routes = Some(RoutePanel::default());
    }
}

/// The centreline points in `text`: one `x, y, z` per line (commas or spaces), blank lines skipped.
pub fn parse_points(text: &str) -> Result<Vec<[f64; 3]>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut v = Vec::new();
        for token in line.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()) {
            match token.parse::<f64>() {
                Ok(x) if x.is_finite() => v.push(x),
                // A token that is not a number is a point the user did not give: refuse it
                // rather than dropping it and reading the rest as millimetres.
                _ => {
                    let line = (i + 1).to_string();
                    return Err(solidcraft_ui_text::format("ui.route_bad_point", &[("line", &line)]));
                }
            }
        }
        match v.as_slice() {
            [x, y, z] => out.push([*x, *y, *z]),
            _ => {
                let line = (i + 1).to_string();
                return Err(solidcraft_ui_text::format("ui.route_bad_point", &[("line", &line)]));
            }
        }
    }
    Ok(out)
}

fn text(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |x| format!("{}", (x * 10.0).round() / 10.0)),
        v => v.to_string(),
    }
}

impl RoutePanel {
    /// The routes and the picked one's details, for the design as it is now.
    fn data(&mut self, app: &mut SolveApp) -> (Value, Value) {
        // Two open designs can share a revision: the tab is part of the key, or the panel shows
        // the last design's routes and acts on a same-named route in this one.
        let revision = (app.docs.active_tab(), app.session.revision);
        if let Some((key, list, got)) = &self.cache
            && *key == revision
        {
            return (list.clone(), got.clone());
        }
        let list = app.session.execute("route.list", &json!({})).unwrap_or_else(|_| json!({"routes": []}));
        let known = |n: &str| list["routes"].as_array().is_some_and(|a| a.iter().any(|r| r["name"] == n));
        if self.picked.as_deref().is_some_and(|n| !known(n)) {
            self.picked = None;
            self.status = None;
        }
        let got = match &self.picked {
            Some(n) => app.session.execute("route.get", &json!({"route": n})).unwrap_or(Value::Null),
            None => Value::Null,
        };
        self.cache = Some((revision, list.clone(), got.clone()));
        (list, got)
    }

    fn done(&mut self, r: Result<Value, String>, what: &str) {
        self.cache = None;
        self.status = Some(match r {
            Ok(v) => {
                let pieces = v["pieces"].as_array().map_or(0, Vec::len).to_string();
                let pipes = v["cut_list"].as_array().map_or(0, Vec::len).to_string();
                let welds = text(&v["welds"]);
                let free = v["free_dof"].as_array().map_or(0, Vec::len);
                let key = if free > 0 { "ui.route_done_free" } else { "ui.route_done" };
                (solidcraft_ui_text::format(key, &[("what", what), ("pieces", &pieces), ("pipes", &pipes), ("welds", &welds)]), false)
            }
            Err(e) => (e, true),
        });
    }
}

/// Whether a feature editor or a dialog holds the design. A route change made now is not in the
/// editor's snapshot, so cancelling the editor would drop the route.
fn editor_open(app: &SolveApp) -> bool {
    app.pending_feature_edit.is_some() || app.editing_state.is_some() || app.dialog.is_some()
}

fn build(app: &mut SolveApp, d: &mut RoutePanel) {
    if editor_open(app) {
        d.status = Some((solidcraft_ui_text::tr("ui.route_editor_open").to_string(), true));
        return;
    }
    let points = match parse_points(&d.points) {
        Ok(p) => p,
        Err(e) => {
            d.status = Some((e, true));
            return;
        }
    };
    let nps = NPS.get(d.size).copied().unwrap_or(1.0);
    let p = json!({"name": d.name.trim(), "nps": nps, "schedule": SCHEDULE_IDS.get(d.schedule).copied().unwrap_or("40"),
                   "elbow": ELBOW_IDS.get(d.elbow).copied().unwrap_or("long_radius"), "points": points});
    let r = app.run("route.create", p);
    if r.is_ok() {
        d.picked = Some(d.name.trim().to_string());
        d.point = 0;
        app.fit_view();
    }
    d.done(r, solidcraft_ui_text::tr("ui.route_built"));
}

fn move_point(app: &mut SolveApp, d: &mut RoutePanel, route: &str, at: &Value) {
    if editor_open(app) {
        d.status = Some((solidcraft_ui_text::tr("ui.route_editor_open").to_string(), true));
        return;
    }
    let delta: Option<Vec<f64>> = d.delta.iter().map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect();
    let Some(delta) = delta else {
        d.status = Some((solidcraft_ui_text::tr("ui.route_bad_move").to_string(), true));
        return;
    };
    let r = app.run("route.move_point", json!({"route": route, "at": at, "delta": delta, "carry": d.carry}));
    d.done(r, solidcraft_ui_text::tr("ui.route_moved"));
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.routes.take() else { return };
    let t = Tokens::get();
    let (list, got) = d.data(app);
    let routes: Vec<Value> = list["routes"].as_array().cloned().unwrap_or_default();
    let (mut do_build, mut do_move, mut do_edit, mut do_delete) = (false, false, false, false);
    let closed =
        ui_kit::Window::new(egui::Id::unique("sc_routes"), "pipe", solidcraft_ui_text::tr("cmd.route.list")).cards(2.0).show_body(ctx, |ui| {
            ui.label(RichText::new(solidcraft_ui_text::tr("ui.route_new")).strong());
            ui_kit::row(ui, "Name", |ui| ui_kit::text_field(ui, &mut d.name, "Route1"));
            ui_kit::row(ui, "Size", |ui| ui_kit::dropdown(ui, "sc_route_size", &mut d.size, SIZES));
            ui_kit::row(ui, solidcraft_ui_text::tr("ui.route_schedule"), |ui| ui_kit::dropdown(ui, "sc_route_schedule", &mut d.schedule, SCHEDULES));
            ui_kit::row(ui, solidcraft_ui_text::tr("ui.route_elbows"), |ui| ui_kit::dropdown(ui, "sc_route_elbow", &mut d.elbow, ELBOWS));
            ui.label(RichText::new(solidcraft_ui_text::tr("ui.route_points")).color(t.text_dim));
            ui_kit::text_area(ui, "Points", egui::Id::unique("sc_route_points"), &mut d.points, 6, 20_000);
            ui.horizontal(|ui| {
                do_build = ui_kit::button(ui, solidcraft_ui_text::tr("ui.route_build"), ui_kit::Button::Primary).clicked();
                if let Some(name) = &d.picked {
                    do_edit = ui_kit::button(ui, solidcraft_ui_text::tr("ui.route_apply_spec"), ui_kit::Button::Normal).clicked();
                    ui.label(RichText::new(name.as_str()).color(t.text_dim));
                }
            });
            ui.separator();
            ui.label(RichText::new(solidcraft_ui_text::tr("ui.route_routes")).strong());
            if routes.is_empty() {
                ui.label(RichText::new(solidcraft_ui_text::tr("ui.route_none")).color(t.text_dim));
            }
            for r in &routes {
                let name = text(&r["name"]);
                let picked = d.picked.as_deref() == Some(name.as_str());
                let elbow = text(&r["elbow"]).replace('_', " ");
                let line = solidcraft_ui_text::format(
                    "ui.route_line",
                    &[
                        ("name", &name),
                        ("nps", &text(&r["nps"])),
                        ("schedule", &text(&r["schedule"])),
                        ("elbow", &elbow),
                        ("pieces", &text(&r["pieces"])),
                        ("pipes", &text(&r["pipes"])),
                        ("length", &text(&r["pipe_length"])),
                    ],
                );
                if ui_kit::list_picker(ui, &format!("sc_route_{name}"), &line, picked).clicked() {
                    d.picked = if picked { None } else { Some(name.clone()) };
                    d.point = 0;
                    d.cache = None;
                }
            }
            if let Some(name) = d.picked.clone()
                && !got.is_null()
            {
                ui.separator();
                ui.label(RichText::new(solidcraft_ui_text::format("ui.route_cut_list", &[("route", &name)])).strong());
                egui::ScrollArea::vertical().id_salt("sc_route_cut").max_height(200.0).show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    egui::Grid::new("sc_route_cut_grid").num_columns(3).striped(true).show(ui, |ui| {
                        for h in [solidcraft_ui_text::tr("ui.pipe"), "Size", solidcraft_ui_text::tr("ui.route_cut_length")] {
                            ui.label(RichText::new(h).strong().color(t.text_dim));
                        }
                        ui.end_row();
                        for c in got["cut_list"].as_array().into_iter().flatten() {
                            ui.label(text(&c["piece"]));
                            ui.label(solidcraft_ui_text::format(
                                "ui.route_size_schedule",
                                &[("nps", &text(&c["nps"])), ("schedule", &text(&c["schedule"]))],
                            ));
                            ui.label(format!("{:.1}", c["length"].as_f64().unwrap_or(0.0)));
                            ui.end_row();
                        }
                    });
                });
                let points: Vec<String> = got["points"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(i, p)| {
                        solidcraft_ui_text::format(
                            "ui.route_point_line",
                            &[("index", &(i + 1).to_string()), ("x", &text(&p[0])), ("y", &text(&p[1])), ("z", &text(&p[2]))],
                        )
                    })
                    .collect();
                let names: Vec<&str> = points.iter().map(String::as_str).collect();
                d.point = d.point.min(names.len().saturating_sub(1));
                ui_kit::row(ui, "Point", |ui| ui_kit::dropdown(ui, "sc_route_point", &mut d.point, &names));
                ui_kit::row(ui, solidcraft_ui_text::tr("ui.route_move_by"), |ui| {
                    ui.horizontal(|ui| {
                        for (axis, v) in ["X", "Y", "Z"].iter().zip(d.delta.iter_mut()) {
                            ui.label(*axis);
                            ui.add_sized(egui::vec2(64.0, ui_kit::row_height()), |ui: &mut egui::Ui| {
                                ui_kit::cell_editor(ui, ui.make_persistent_id(axis), v, "0", false)
                            });
                        }
                    });
                });
                ui_kit::checkbox(ui, solidcraft_ui_text::tr("ui.route_carry"), &mut d.carry);
                ui.horizontal(|ui| {
                    do_move = ui_kit::button(ui, solidcraft_ui_text::tr("ui.route_move_point"), ui_kit::Button::Primary).clicked();
                    do_delete = ui_kit::button(ui, solidcraft_ui_text::tr("ui.route_delete"), ui_kit::Button::Normal).clicked();
                });
            }
            if let Some((msg, error)) = &d.status {
                ui_kit::status_line(ui, msg, if *error { ui_kit::StatusTone::Error } else { ui_kit::StatusTone::Dim });
            }
        });
    if (do_build || do_move || do_edit || do_delete) && editor_open(app) {
        d.status = Some((solidcraft_ui_text::tr("ui.route_editor_open").to_string(), true));
    } else {
        if do_build {
            build(app, &mut d);
        }
        if let Some(name) = d.picked.clone() {
            if do_move {
                let at = got["points"].get(d.point).cloned().unwrap_or(Value::Null);
                move_point(app, &mut d, &name, &at);
            }
            if do_edit {
                let p = json!({"route": name, "nps": NPS.get(d.size).copied().unwrap_or(1.0), "schedule": SCHEDULE_IDS.get(d.schedule).copied().unwrap_or("40"),
                               "elbow": ELBOW_IDS.get(d.elbow).copied().unwrap_or("long_radius")});
                let r = app.run("route.edit", p);
                d.done(r, solidcraft_ui_text::tr("ui.route_rebuilt"));
            }
            if do_delete {
                let r = app.run("route.delete", json!({"route": name}));
                d.cache = None;
                d.status = Some(match r {
                    Ok(_) => (solidcraft_ui_text::format("ui.route_deleted", &[("route", &name)]), false),
                    Err(e) => (e, true),
                });
                d.picked = None;
            }
        }
    }
    if !closed {
        app.routes = Some(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    #[test]
    fn points_are_read_one_per_line_and_a_bad_line_is_named() {
        crate::i18n::install();
        assert_eq!(parse_points("0, 0, 0\n\n 1000 0 0\n1000,800,0").unwrap(), vec![[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 800.0, 0.0]]);
        assert_eq!(parse_points("0, 0, 0\n1, 2").unwrap_err(), "Line 2: give x, y, z in mm.");
        // A token that is not a number is refused, not dropped.
        assert_eq!(parse_points("100 cm, 0, 0").unwrap_err(), "Line 1: give x, y, z in mm.");
        assert_eq!(parse_points("0, bad, 100, 0").unwrap_err(), "Line 1: give x, y, z in mm.");
    }

    #[test]
    fn a_route_change_is_refused_while_a_feature_editor_is_open() {
        let mut h = Harness::new();
        h.app.start("solid.box");
        h.frames(4);
        assert!(h.app.dialog.is_some(), "the box's editor is open");
        let mut d = RoutePanel::default();
        build(&mut h.app, &mut d);
        assert!(d.status.as_ref().is_some_and(|(_, error)| *error), "{:?}", d.status);
        assert_eq!(h.app.run("route.list", json!({})).unwrap()["routes"], json!([]), "nothing was built");
    }

    #[test]
    fn build_route_makes_the_route_and_lists_its_cut_lengths() {
        let mut h = Harness::new();
        open(&mut h.app);
        h.frames(2);
        assert!(h.widget("Build Route").is_some() && h.widget("Points").is_some());
        let b = h.widget("Build Route").expect("Build Route");
        h.call("ui.click", json!({"x": b[0], "y": b[1]}));
        h.frames(3);
        let list = h.app.run("route.list", json!({})).unwrap();
        assert_eq!(list["routes"][0]["name"], "Route1");
        assert_eq!(h.app.routes.as_ref().and_then(|d| d.picked.clone()).as_deref(), Some("Route1"));
        assert!(h.widget("Move Point").is_some(), "the picked route's point move");
        let m = h.widget("Move Point").expect("Move Point");
        h.call("ui.click", json!({"x": m[0], "y": m[1]}));
        h.frames(3);
        let got = h.app.run("route.get", json!({"route": "Route1"})).unwrap();
        assert_eq!(got["points"][0], json!([100.0, 0.0, 0.0]), "the first point moved 100 mm along X");
        h.app.run("file.new", json!({})).unwrap();
        h.frames(3);
        let d = h.app.routes.as_ref().expect("the window stays open");
        assert!(d.picked.is_none() && d.status.is_none(), "a new design shows nothing of the last one");
    }
}
