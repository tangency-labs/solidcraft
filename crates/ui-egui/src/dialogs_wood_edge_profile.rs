//! Router sections, edge picks and the standard feature preview/edit flow.

use egui::{Sense, vec2};
use serde_json::{Map, Value, json};
use solidcraft_engine::{
    Sel,
    doc::{FeatureKind, expr::Kind as ValueKind},
};

use crate::{
    SolveApp,
    dialogs::{Kind, shown_expr},
    selection::{EDGES, SelInput},
    ui_kit::{choice_row, expr_row, row},
};

const PROFILES: [&str; 8] = ["chamfer", "bevel", "roundover", "cove", "rabbet", "ogee", "bead", "none"];
const LABELS: [&str; 8] =
    ["ui.wood.chamfer", "ui.wood.bevel", "ui.wood.roundover", "ui.wood.cove", "ui.wood.rabbet", "ui.wood.ogee", "ui.wood.bead", "ui.wood.edge_none"];

#[derive(Clone, Debug)]
pub struct Router {
    pub profile: usize,
    pub radius: String,
    pub width: String,
    pub depth: String,
    pub angle: String,
    pub step: String,
    pub fillet: String,
    pub tangent_chain: bool,
}

impl Default for Router {
    fn default() -> Self {
        Self {
            profile: 2,
            radius: "6 mm".into(),
            width: "12 mm".into(),
            depth: "12 mm".into(),
            angle: "45 deg".into(),
            step: "1 mm".into(),
            fillet: "0 mm".into(),
            tangent_chain: false,
        }
    }
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "wood.edge_profile").then(|| (Kind::WoodEdgeProfile(Router::default()), vec![SelInput::new("ui.wood.edges", EDGES, true)]))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, r: &mut Router) -> bool {
    let before = r.profile;
    let labels = LABELS.map(|key| app.tr(key));
    choice_row(ui, app.tr("ui.wood.profile"), "router_profile", &mut r.profile, &labels);
    if r.profile != before {
        r.width = if r.profile < 2 { "3 mm" } else { "12 mm" }.into();
    }
    // Four sections to a row, each as wide as its picture, so the dialog keeps its width.
    let t = solidcraft_theme::Tokens::get();
    let tile = vec2(t.control_height * 2.0, t.control_height + t.space_16);
    egui::Grid::new("router_profiles").num_columns(4).spacing(vec2(t.space_4, t.space_4)).show(ui, |ui| {
        for (i, label) in labels.iter().enumerate() {
            ui.vertical(|ui| {
                ui.set_width(tile.x);
                let (rect, response) = ui.allocate_exact_size(tile, Sense::click());
                if response.clicked() && r.profile != i {
                    r.profile = i;
                    r.width = if i < 2 { "3 mm" } else { "12 mm" }.into();
                }
                sketch(ui, rect, i, r.profile == i);
                ui.add(egui::Label::new(*label).truncate()).on_hover_text(*label);
            });
            if i % 4 == 3 {
                ui.end_row();
            }
        }
    });
    let mut enter = false;
    if matches!(r.profile, 2 | 3 | 6) {
        enter |= expr_row(ui, app.tr("ui.radius"), &mut r.radius);
    }
    if matches!(r.profile, 0 | 1 | 4 | 5) {
        enter |= expr_row(ui, app.tr("ui.width"), &mut r.width);
    }
    if matches!(r.profile, 4 | 5) {
        enter |= expr_row(ui, app.tr("ui.depth"), &mut r.depth);
    }
    if r.profile == 1 {
        enter |= expr_row(ui, app.tr("ui.angle"), &mut r.angle);
    }
    if r.profile == 2 {
        enter |= expr_row(ui, app.tr("ui.wood.fillet"), &mut r.fillet);
    }
    if r.profile == 6 {
        enter |= expr_row(ui, app.tr("ui.wood.step"), &mut r.step);
    }
    row(ui, app.tr("ui.wood.tangent_chain"), |ui| {
        crate::dialogs::check(ui, &mut r.tangent_chain);
    });
    crate::ui_kit::text(ui, app.tr("ui.wood.supports"), crate::ui_kit::Text::Hint("router_supports"));
    enter
}

// Display-only normalised section diagrams; no model decisions use these samples.
fn sketch(ui: &egui::Ui, rect: egui::Rect, kind: usize, selected: bool) {
    let t = solidcraft_theme::Tokens::get();
    let stroke = if selected { t.primary_border(t.accent) } else { t.secondary_border(t.accent) };
    let rect = rect.shrink(t.space_4);
    let p = |x: f32, y: f32| rect.lerp_inside(vec2(x, y));
    let mut points = vec![p(1.0, 0.0)];
    match kind {
        0 => points.push(p(0.0, 1.0)),
        1 => points.push(p(0.0, 0.65)),
        2 | 3 | 6 => {
            let (offset, scale) = if kind == 6 { (0.16, 0.68) } else { (0.0, 1.0) };
            if kind == 6 {
                points.extend([p(1.0, offset), p(1.0 - offset, offset)]);
            }
            for i in 0..=16 {
                let a = i as f32 * std::f32::consts::FRAC_PI_2 / 16.0;
                let (x, y) = if kind == 3 { (a.cos(), a.sin()) } else { (1.0 - a.sin(), 1.0 - a.cos()) };
                points.push(p(offset + scale * x, offset + scale * y));
            }
            if kind == 6 {
                points.extend([p(offset, 1.0), p(0.0, 1.0)]);
            }
        }
        4 => points.extend([p(1.0, 1.0), p(0.0, 1.0)]),
        5 => {
            for i in 0..=16 {
                let a = i as f32 * std::f32::consts::FRAC_PI_2 / 16.0;
                points.push(p(0.5 + 0.5 * a.cos(), 0.5 * a.sin()));
            }
            for i in 0..=16 {
                let a = i as f32 * std::f32::consts::FRAC_PI_2 / 16.0;
                points.push(p(0.5 - 0.5 * a.sin(), 1.0 - 0.5 * a.cos()));
            }
        }
        _ => points.extend([p(0.0, 0.0), p(0.0, 1.0)]),
    }
    ui.painter().add(egui::Shape::line(points, stroke));
    t.secondary_line(ui.painter(), [p(0.0, 0.0), p(1.0, 0.0)], t.accent);
    t.secondary_line(ui.painter(), [p(0.0, 0.0), p(0.0, 1.0)], t.accent);
}

pub fn commands(app: &SolveApp, r: &Router, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let edges: Vec<_> = crate::selection::items(inputs, 0)
        .iter()
        .filter_map(|s| match s {
            Sel::Edge { body, point, .. } => Some(json!({"body": body, "point": point})),
            _ => None,
        })
        .collect();
    if edges.is_empty() {
        return Err(app.tr("error.wood.edges").into());
    }
    let profile = PROFILES.get(r.profile).ok_or_else(|| app.tr("error.wood.profile").to_string())?;
    let mut p = json!({"edges": edges, "profile": profile, "radius": r.radius, "width": r.width, "depth": r.depth, "angle": r.angle, "step": r.step, "fillet": r.fillet, "tangent_chain": r.tangent_chain});
    if let Some(p) = p.as_object_mut() {
        p.extend(extra.clone());
    }
    Ok(vec![("wood.edge_profile".into(), p)])
}

pub fn for_feature(app: &SolveApp, feature: &solidcraft_engine::doc::Feature) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::WoodEdgeProfile { edges, body, profile, radius, width, depth, angle, step, fillet, tangent_chain } = &feature.kind else {
        return None;
    };
    let (_, mut inputs) = start(app, "wood.edge_profile")?;
    let length = |e: &solidcraft_engine::doc::Expr| shown_expr(e, ValueKind::Length);
    if let Some(input) = inputs.first_mut() {
        let st = app.session.model.state();
        input.items = edges
            .iter()
            .filter_map(|p| {
                let (owner, hit) = solidcraft_engine::doc::wood_edge_profile::pick(&st, body.as_deref(), p.0).ok()?;
                Some(Sel::Edge { body: st.bodies.get(owner)?.name.clone(), index: hit.edge, point: hit.point })
            })
            .collect();
    }
    let r = Router {
        profile: PROFILES.iter().position(|p| *p == profile)?,
        radius: length(radius),
        width: length(width),
        depth: length(depth),
        angle: shown_expr(angle, ValueKind::Angle),
        step: length(step),
        fillet: length(fillet),
        tangent_chain: *tangent_chain,
    };
    Some((Kind::WoodEdgeProfile(r), inputs, Map::new()))
}
