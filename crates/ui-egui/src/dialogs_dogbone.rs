//! Dogbone: the inside corner edges (or the faces whose inside corners) to relieve, the router
//! bit's diameter and where each relief sits. The command finds the inside corners and says
//! which edges it skipped.
use crate::{
    SolveApp,
    dialogs::{Kind, shown_expr},
    selection::{EDGES, PLANAR_FACES, SelInput},
    ui_kit::{choice_row, expr_row},
};
use serde_json::{Map, Value, json};
use solidcraft_engine::{
    Sel,
    doc::{
        Feature, FeatureKind,
        dogbone::{DogboneSide, DogboneStyle},
        expr::Kind as ValueKind,
    },
};

/// The styles in the dialog's order.
const STYLES: [DogboneStyle; 3] = [DogboneStyle::Normal, DogboneStyle::Minimal, DogboneStyle::TBone];

#[derive(Clone, Debug)]
pub struct Dogbone {
    /// The bit's diameter.
    pub diameter: String,
    /// An index into [`STYLES`].
    pub style: usize,
    /// Minimal: how far short of the corner the relief stops; empty: the command's default.
    pub gap: String,
    /// T-bone: 0 the longer wall, 1 the shorter.
    pub side: usize,
    /// The largest corner angle relieved.
    pub max_angle: String,
}

impl Default for Dogbone {
    fn default() -> Self {
        // A 1/8" bit, the smallest common router bit for plywood and MDF.
        Self { diameter: "3.175 mm".into(), style: 0, gap: String::new(), side: 0, max_angle: "90 deg".into() }
    }
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "solid.dogbone").then(|| (Kind::Dogbone(Dogbone::default()), vec![SelInput::new("ui.dogbone.corners", EDGES | PLANAR_FACES, true)]))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, c: &mut Dogbone) -> bool {
    let mut enter = expr_row(ui, app.tr("ui.dogbone.diameter"), &mut c.diameter);
    choice_row(
        ui,
        app.tr("ui.dogbone.style"),
        "dogbone_style",
        &mut c.style,
        &[app.tr("ui.dogbone.normal"), app.tr("ui.dogbone.minimal"), app.tr("ui.dogbone.t_bone")],
    );
    match STYLES.get(c.style) {
        Some(DogboneStyle::Minimal) => enter |= expr_row(ui, app.tr("ui.dogbone.gap"), &mut c.gap),
        Some(DogboneStyle::TBone) => {
            choice_row(ui, app.tr("ui.dogbone.side"), "dogbone_side", &mut c.side, &[app.tr("ui.dogbone.longer"), app.tr("ui.dogbone.shorter")]);
        }
        _ => {}
    }
    enter |= expr_row(ui, app.tr("ui.dogbone.max_angle"), &mut c.max_angle);
    enter
}

pub fn commands(app: &SolveApp, c: &Dogbone, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let picks = crate::selection::items(inputs, 0);
    let mut edges = Vec::new();
    let mut faces = Vec::new();
    let mut bodies: Vec<&str> = Vec::new();
    for s in picks {
        match s {
            Sel::Edge { body, point, .. } => {
                edges.push(json!([point.x, point.y, point.z]));
                bodies.push(body);
            }
            Sel::Face { body, point, .. } => {
                faces.push(json!([point.x, point.y, point.z]));
                bodies.push(body);
            }
            _ => {}
        }
    }
    if edges.is_empty() && faces.is_empty() {
        return Err(app.tr("error.dogbone_corners").into());
    }
    let style = STYLES.get(c.style).copied().unwrap_or_default();
    let mut args = json!({"diameter": c.diameter, "max_angle": c.max_angle});
    if let Value::Object(args) = &mut args {
        if !edges.is_empty() {
            args.insert("edges".into(), Value::Array(edges));
        }
        if !faces.is_empty() {
            args.insert("faces".into(), Value::Array(faces));
        }
        // Picks on one body name it, so a flush face of another body is never taken (#4555).
        if let Some(first) = bodies.first()
            && bodies.iter().all(|b| b == first)
        {
            args.insert("body".into(), json!(first));
        }
        if style != DogboneStyle::Normal {
            args.insert("style".into(), json!(style.id()));
        }
        if style == DogboneStyle::Minimal && !c.gap.trim().is_empty() {
            args.insert("gap".into(), json!(c.gap));
        }
        if style == DogboneStyle::TBone {
            args.insert("side".into(), json!(if c.side == 1 { DogboneSide::Shorter } else { DogboneSide::Longer }.id()));
        }
        for (k, v) in extra {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("solid.dogbone".into(), args)])
}

pub fn for_feature(app: &SolveApp, f: &Feature) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::Dogbone { edges, faces, diameter, style, gap, side, max_angle, along, body } = &f.kind else { return None };
    let (_, mut inputs) = start(app, "solid.dogbone")?;
    let c = Dogbone {
        diameter: shown_expr(diameter, ValueKind::Length),
        style: STYLES.iter().position(|s| s == style).unwrap_or(0),
        gap: gap.as_ref().map(|e| shown_expr(e, ValueKind::Length)).unwrap_or_default(),
        side: usize::from(*side == DogboneSide::Shorter),
        max_angle: max_angle.as_ref().map_or_else(|| "90 deg".into(), |e| shown_expr(e, ValueKind::Angle)),
    };
    // Each pick comes back by its stored name first, as Fillet's and Shell's do, so a pocket
    // that moved keeps its corners; a pick with no name comes back where it was stored, on the
    // body the feature names.
    let s = &app.session;
    let only = body.as_deref();
    let mut items = Vec::new();
    for (i, e) in edges.iter().enumerate() {
        items.extend(
            f.edge_names
                .get(i)
                .and_then(|name| crate::dialogs::named_refill(s, only, name, e.0, true))
                .or_else(|| crate::dialogs::edge_sel_on(s, only, e.0)),
        );
    }
    for (i, p) in faces.iter().enumerate() {
        items.extend(
            f.face_names
                .get(i)
                .and_then(|name| crate::dialogs::named_refill(s, only, name, p.0, false))
                .or_else(|| crate::dialogs::face_sel_on(s, only, p.0)),
        );
    }
    if let Some(input) = inputs.first_mut() {
        input.items = items;
    }
    // Kept as it is: the dialog has no field for the bit's axis.
    let mut extra = Map::new();
    if let Some(a) = along {
        extra.insert("along".into(), json!([a.0.x, a.0.y, a.0.z]));
    }
    Some((Kind::Dogbone(c), inputs, extra))
}
