//! Thin gusset form; exact support and fabrication validation live in the command.
use crate::{
    SolveApp,
    dialogs::{Kind, shown_expr},
    selection::{PLANAR_FACES, SelInput},
    ui_kit::{choice_row, expr_row},
};
use serde_json::{Map, Value, json};
use solidcraft_engine::{
    Sel,
    doc::{FeatureKind, expr::Kind as ValueKind, gusset::Placement},
};
#[derive(Clone, Debug)]
pub struct Gusset {
    pub leg1: String,
    pub leg2: String,
    pub thickness: String,
    pub relief: String,
    pub clip: String,
    pub offset: String,
    pub placement: usize,
    pub profile: usize,
}
impl Default for Gusset {
    fn default() -> Self {
        Self {
            leg1: "75 mm".into(),
            leg2: "75 mm".into(),
            thickness: "6 mm".into(),
            relief: "0 mm".into(),
            clip: "10 mm".into(),
            offset: "0 mm".into(),
            placement: 0,
            profile: 0,
        }
    }
}
pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.gusset").then(|| {
        (
            Kind::Gusset(Gusset::default()),
            vec![SelInput::new("ui.gusset.face1", PLANAR_FACES, false), SelInput::new("ui.gusset.face2", PLANAR_FACES, false)],
        )
    })
}
pub fn rows(app: &SolveApp, ui: &mut egui::Ui, g: &mut Gusset) -> bool {
    choice_row(ui, app.tr("ui.gusset.profile"), "gusset_profile", &mut g.profile, &[app.tr("ui.gusset.triangle"), app.tr("ui.gusset.clipped")]);
    let mut enter = expr_row(ui, app.tr("ui.gusset.leg1"), &mut g.leg1);
    enter |= expr_row(ui, app.tr("ui.gusset.leg2"), &mut g.leg2);
    enter |= expr_row(ui, app.tr("ui.thickness"), &mut g.thickness);
    if g.profile == 1 {
        enter |= expr_row(ui, app.tr("ui.gusset.clip"), &mut g.clip);
    }
    enter |= expr_row(ui, app.tr("ui.gusset.relief"), &mut g.relief);
    enter |= expr_row(ui, app.tr("ui.gusset.offset"), &mut g.offset);
    choice_row(
        ui,
        app.tr("ui.gusset.placement"),
        "gusset_placement",
        &mut g.placement,
        &[app.tr("ui.gusset.center"), app.tr("ui.gusset.positive"), app.tr("ui.gusset.negative")],
    );
    enter
}
pub fn commands(app: &SolveApp, g: &Gusset, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let mut supports = Vec::new();
    for i in 0..2 {
        let [Sel::Face { body, index, point }] = crate::selection::items(inputs, i) else { return Err(app.tr("error.gusset_support").into()) };
        supports.push(json!({"face":point,"body":body,"index":index}));
    }
    let placement = match g.placement {
        0 => "center",
        1 => "positive",
        2 => "negative",
        _ => return Err(app.tr("error.gusset_support").into()),
    };
    let clip = if g.profile == 0 { "0 mm" } else { &g.clip };
    let mut args = json!({"supports":supports,"leg1":g.leg1,"leg2":g.leg2,"thickness":g.thickness,"relief":g.relief,"clip":clip,"offset":g.offset,"placement":placement});
    if let Value::Object(args) = &mut args {
        for (k, v) in extra {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("frame.gusset".into(), args)])
}
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::FrameGusset { supports, leg1, leg2, thickness, relief, clip, offset, placement } = kind else { return None };
    let (_, mut inputs) = start(app, "frame.gusset")?;
    let length = |e: &solidcraft_engine::doc::Expr| shown_expr(e, ValueKind::Length);
    let g = Gusset {
        leg1: length(leg1),
        leg2: length(leg2),
        thickness: length(thickness),
        relief: length(relief),
        clip: length(clip),
        offset: length(offset),
        placement: match placement {
            Placement::Center => 0,
            Placement::Positive => 1,
            Placement::Negative => 2,
        },
        profile: usize::from(clip.as_str() != "0 mm"),
    };
    let state = app.session.model.state();
    let world = app.session.world_state();
    for (support, input) in supports.iter().zip(&mut inputs) {
        if let Some(anchor) = solidcraft_engine::doc::face_anchor::plane_face_anchor(&support.plane)
            && let Some(body) = state.body(&anchor.target.body)
        {
            let name = app.session.doc.world_body_name(&state, &body.name, anchor.component, &anchor.path);
            let indices = app.session.doc.configuration_face_indices(body, &anchor.target).ok();
            if let Some(body) = world.body(&name) {
                let mesh = body.mesh();
                if let Some((tri, index)) =
                    mesh.triangles.iter().zip(&mesh.tri_face).find(|(_, i)| indices.as_ref().is_some_and(|is| is.contains(&(**i as usize))))
                    && let Some([a, b, c]) = mesh.tri(tri)
                {
                    input.items = vec![Sel::Face { body: name, index: *index as usize, point: (a + b + c) / 3. }];
                }
            }
        }
    }
    Some((Kind::Gusset(g), inputs, Map::new()))
}
