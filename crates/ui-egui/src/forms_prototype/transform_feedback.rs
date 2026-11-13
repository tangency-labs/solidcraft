//! Submitted gesture transforms and acceptance status, never measured shape motion.
use super::*;
use solidcraft_engine::forms_prototype::validation::Stamp;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Value {
    Move(Vec3),
    Rotate { axis: usize, angle: f64 },
    Scale([f64; 3]),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Phase {
    Requested,
    Fitting,
    Accepted,
}
pub(super) struct Feedback {
    source: Stamp,
    value: Value,
    basis: [Vec3; 3],
    soft: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Snapshot {
    pub(super) value: Value,
    pub(super) basis: [Vec3; 3],
    pub(super) soft: bool,
    pub(super) phase: Phase,
}
/// Submit the same scale factors to the command and readout. Constraints and
/// soft weights can change individual point motion; these are request values.
pub(super) fn gizmo_value(change: crate::gizmo::Change, drag: &Drag, handle: crate::gizmo::Handle, camera: &Camera) -> Value {
    match change {
        crate::gizmo::Change::Translate(delta) if drag.scale => {
            let factors = if handle == crate::gizmo::Handle::Ball {
                let (r, u, _) = camera.basis();
                solidcore_ops::form_transform::uniform_factors(delta, drag.length, r, u)
            } else {
                solidcore_ops::form_transform::scale_factors(delta, drag.length, drag.basis)
            };
            Value::Scale(factors)
        }
        crate::gizmo::Change::Translate(delta) => Value::Move(delta),
        crate::gizmo::Change::Rotate { axis, angle } => Value::Rotate { axis: axis.min(2), angle },
    }
}
pub(super) fn clear(app: &mut SolveApp) {
    app.forms_prototype.feedback = None;
}
pub(super) fn record(app: &mut SolveApp, value: Value, basis: [Vec3; 3], soft: bool) {
    if !app.preferences.forms_drag_feedback {
        clear(app);
        return;
    }
    // A successful synchronous edit changes the source revision: qualify AFTER
    // the command. Pending extrusion qualifies against its own accepted source.
    app.forms_prototype.feedback = Stamp::current(&app.session).map(|source| Feedback { source, value, basis, soft });
}
pub(super) fn current(app: &SolveApp) -> Option<Snapshot> {
    if !app.preferences.forms_drag_feedback {
        return None;
    }
    let feedback = app.forms_prototype.feedback.as_ref()?;
    let phase = if extrusion::active(app) {
        let Value::Move(target) = feedback.value else { return None };
        extrusion::readout_phase(app, target)?
    } else if transform_job::active(app) {
        transform_job::readout_phase(app)?
    } else {
        let drag = app.forms_prototype.drag.as_ref().or(app.forms_prototype.gizmo_drag.as_ref())?;
        if !drag.moved || feedback.source != Stamp::current(&app.session)? {
            return None;
        }
        if live_cad::mesh(app).is_some() { Phase::Accepted } else { Phase::Fitting }
    };
    let finite = feedback.basis.iter().all(|axis| axis.is_finite() && (axis.len() - 1.0).abs() < 1e-6)
        && match feedback.value {
            Value::Move(delta) => delta.is_finite() && delta.len().is_finite(),
            Value::Rotate { axis, angle } => axis < 3 && angle.is_finite() && angle.to_degrees().is_finite(),
            Value::Scale(factors) => factors.iter().all(|f| f.is_finite() && *f > 0.0),
        };
    finite.then_some(Snapshot { value: feedback.value, basis: feedback.basis, soft: feedback.soft, phase })
}
pub(super) fn text(app: &SolveApp, snapshot: Snapshot) -> String {
    let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
    let length = |v: f64| crate::prefs::format_length(if v.abs() < 1e-12 { 0.0 } else { v }, &app.session.doc.units, app.preferences.length_decimals);
    let value = match snapshot.value {
        Value::Move(delta) => {
            let (magnitude, components) = solidcore_ops::form_transform::translation_measure(delta, snapshot.basis);
            let components = components.map(length);
            locale.format(
                "forms.feedback.move",
                &[("distance", &length(magnitude)), ("x", &components[0]), ("y", &components[1]), ("z", &components[2])],
            )
        }
        Value::Rotate { axis, angle } => {
            let angle = format!("{:.*}°", usize::from(app.preferences.angle_decimals), angle.to_degrees());
            locale.format("forms.feedback.rotate", &[("axis", ["X", "Y", "Z"][axis.min(2)]), ("angle", &angle)])
        }
        Value::Scale(factors) => {
            let factors = factors.map(|factor| format!("{factor:.4}"));
            locale.format("forms.feedback.scale", &[("x", &factors[0]), ("y", &factors[1]), ("z", &factors[2])])
        }
    };
    let value = if snapshot.soft { locale.format("forms.feedback.soft", &[("value", &value)]) } else { value };
    locale.format(
        match snapshot.phase {
            Phase::Requested => "forms.feedback.requested",
            Phase::Fitting => "forms.feedback.fitting",
            Phase::Accepted => "forms.feedback.accepted",
        },
        &[("value", &value)],
    )
}
/// Read the same qualified request as the viewport, without advancing work or
/// claiming that constraints applied that request as measured point motion.
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    let Some(snapshot) = current(app) else { return serde_json::Value::Null };
    let request = match snapshot.value {
        Value::Move(delta) => json!({"kind":"move","delta_mm":[delta.x,delta.y,delta.z]}),
        Value::Rotate { axis, angle } => json!({"kind":"rotate","axis":axis,"angle_rad":angle}),
        Value::Scale(factors) => json!({"kind":"scale","factors":factors}),
    };
    json!({
        "request":request,
        "semantics":"submitted-request",
        "basis":snapshot.basis.map(|axis|[axis.x,axis.y,axis.z]),
        "soft":snapshot.soft,
        "phase":match snapshot.phase { Phase::Requested=>"requested",Phase::Fitting=>"fitting",Phase::Accepted=>"accepted" },
        "text":text(app,snapshot),
    })
}
pub(super) fn paint(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj) {
    let Some(snapshot) = current(app) else {
        clear(app);
        return;
    };
    let t = Tokens::get();
    let mut obstacles = crate::shell::covered_rects(app);
    obstacles.push(crate::viewport::axis_rect(app, proj.rect));
    if let Some(layout) = &app.viewport.layout {
        obstacles.extend([layout.navigation, layout.confirm]);
    }
    let available = crate::viewport_layout::free_rect(crate::shell::content_rect(app, proj.rect), &obstacles).intersect(proj.rect).shrink(t.space_8);
    if !available.is_positive() || available.width() < t.space_8 * 4.0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(proj.rect);
    let galley =
        painter.layout(text(app, snapshot), crate::ui_kit::type_face::medium(t.font_label), t.text, (available.width() - t.space_8 * 2.0).max(1.0));
    let size = galley.size() + egui::vec2(t.space_8 * 2.0, t.space_4 * 2.0);
    let rect = egui::Rect::from_min_size(egui::pos2(available.center().x - size.x * 0.5, available.top()), size);
    t.container_rect(&painter, rect, t.control_radius, t.panel, t.border);
    painter.galley(rect.min + egui::vec2(t.space_8, t.space_4), galley, t.text);
    crate::scenario::publish_target("forms_transform_feedback", rect.intersect(proj.rect));
}
