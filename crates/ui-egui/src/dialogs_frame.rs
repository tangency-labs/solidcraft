//! Frame Member (`frame.member`): structural members along the lines of a frame sketch. The lines
//! are picked in the view; the section is a built-in profile with its dimensions, or the profiles
//! picked in another sketch. Anchor, angle and mirror place the section on each line.

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{self, MemberProfile};
use solidcraft_engine::doc::{FeatureKind, ProfileSel};

use crate::SolveApp;
use crate::dialogs::{Kind, check, curves_sketch, form_row, profile_indices, put, shown_expr};
use crate::selection::{PROFILES, SKETCH_LINES, SelInput, SelectionExt as _};
use crate::ui_kit::{choice_row, expr_row, hint_row, text_row};

/// The anchors as `frame.member` names them, and their labels.
const ANCHORS: [(&str, &str); 11] = [
    ("origin", "Origin"),
    ("center", "Center"),
    ("centroid", "Centroid"),
    ("top_left", "Top Left"),
    ("top", "Top Middle"),
    ("top_right", "Top Right"),
    ("left", "Left Middle"),
    ("right", "Right Middle"),
    ("bottom_left", "Bottom Left"),
    ("bottom", "Bottom Middle"),
    ("bottom_right", "Bottom Right"),
];

/// The Frame Member dialog's values (the picks are its selection inputs).
#[derive(Clone, Debug)]
pub struct Member {
    /// Every line of the frame sketch, now and as lines are added, not only the lines picked.
    pub all: bool,
    /// The built-in profile (an index into `frame::profile_kinds`), used when no section is picked.
    pub kind: usize,
    /// Each built-in profile's dimensions, its required ones then its optional ones; kept per
    /// profile so switching back finds them again. An empty optional one takes its default.
    pub dims: Vec<Vec<String>>,
    /// A profile sketch whose whole drawn section is the member's (nested loops alternating), as
    /// a feature made that way keeps it while no section is picked.
    pub whole: Option<u64>,
    pub anchor: usize,
    pub angle: String,
    pub mirror: bool,
    pub name: String,
}

impl Member {
    fn new() -> Member {
        let dims = frame::profile_kinds()
            .iter()
            .map(|k| {
                let example = serde_json::to_value(&k.example).unwrap_or(Value::Null);
                let given = k.required.iter().map(|d| example.get(*d).and_then(Value::as_f64).map(|v| format!("{v} mm")).unwrap_or_default());
                given.chain(k.optional.iter().map(|_| String::new())).collect()
            })
            .collect();
        // A rectangular tube is the most common member.
        let kind = frame::profile_kinds().iter().position(|k| k.kind == "rect_tube").unwrap_or(0);
        Member { all: false, kind, dims, whole: None, anchor: 0, angle: String::new(), mirror: false, name: String::new() }
    }
}

/// The dialog for Frame Member.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.member")
        .then(|| (Kind::Frame(Member::new()), vec![SelInput::new("Lines", SKETCH_LINES, true), SelInput::new("Section (optional)", PROFILES, true)]))
}

fn tr(text: &str) -> &str {
    crate::i18n::tr_label(text)
}

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &SolveApp, ui: &mut egui::Ui, m: &mut Member, inputs: &[SelInput]) -> bool {
    let mut enter = false;
    form_row(ui, "All Lines", |ui| check(ui, &mut m.all));
    let picked = !crate::selection::items(inputs, 1).is_empty();
    if let Some(sketch) = m.whole.filter(|_| !picked) {
        let name = app.session.doc.feature(sketch).map(|f| f.name.clone()).unwrap_or_default();
        let mut on = true;
        form_row(ui, "Section", |ui| crate::ui_kit::checkbox(ui, &name, &mut on));
        if !on {
            m.whole = None;
        }
    } else if !picked {
        let kinds = frame::profile_kinds();
        let labels: Vec<&str> = kinds.iter().map(|k| k.label).collect();
        choice_row(ui, "Profile", "frame_profile", &mut m.kind, &labels);
        if let (Some(k), Some(values)) = (kinds.get(m.kind), m.dims.get_mut(m.kind)) {
            for (i, (name, v)) in k.required.iter().chain(k.optional).zip(values.iter_mut()).enumerate() {
                let label = frame::dimension_label(name);
                enter |= if i < k.required.len() { expr_row(ui, label, v) } else { hint_row(ui, label, v, tr("default")) };
            }
        }
    }
    let anchors: Vec<&str> = ANCHORS.iter().map(|a| a.1).collect();
    choice_row(ui, "Anchor", "frame_anchor", &mut m.anchor, &anchors);
    enter |= hint_row(ui, "Angle", &mut m.angle, "0 deg");
    form_row(ui, "Mirror", |ui| check(ui, &mut m.mirror));
    enter |= text_row(ui, "Name", &mut m.name, tr("from the profile")).0;
    enter
}

/// The command the dialog's OK runs (`extra`: parameters kept from the feature being edited).
pub fn commands(app: &SolveApp, m: &Member, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let section = crate::selection::items(inputs, 1).profiles();
    let section_sketch = section.first().map(|x| x.0).or(m.whole);
    if section.iter().any(|x| Some(x.0) != section_sketch) {
        return Err("the section's profiles must be in one sketch".into());
    }
    // A line picked in the view comes with its sketch (line ids are a sketch's own); a line
    // selected before the dialog opened is looked for in the newest sketch with it, not the
    // section's.
    let picks = crate::selection::items(inputs, 0);
    let mut lines = Vec::new();
    let mut known = None;
    for pick in picks {
        match pick {
            Sel::ExternalSketchCurve { sketch, id } | Sel::SketchCurve { id, sketch: Some(sketch) } => {
                if known.is_some_and(|k| k != *sketch) {
                    return Err("the lines must be in one sketch".into());
                }
                known = Some(*sketch);
                lines.push(id.clone());
            }
            Sel::SketchCurve { id, sketch: None } => lines.push(id.clone()),
            _ => {}
        }
    }
    if lines.is_empty() {
        return Err("select the frame sketch's lines first".into());
    }
    let sketch = known.or_else(|| curves_sketch(app, &lines, section_sketch)).ok_or("the lines must be in one sketch, not the section's")?;
    let mut p = json!({"sketch": sketch});
    if !m.all {
        p["lines"] = json!(lines);
    }
    if let Some(ps) = section_sketch {
        p["profile_sketch"] = json!(ps);
        if !section.is_empty() {
            p["profiles"] = json!(section.iter().map(|x| x.1).collect::<Vec<_>>());
        }
    } else {
        let kinds = frame::profile_kinds();
        let (k, values) = kinds.get(m.kind).zip(m.dims.get(m.kind)).ok_or("choose a profile")?;
        let mut profile = json!({"kind": k.kind});
        for (name, v) in k.required.iter().chain(k.optional).zip(values) {
            if !v.trim().is_empty() {
                profile[*name] = json!(v);
            }
        }
        p["profile"] = profile;
    }
    if m.anchor != 0
        && let Some((anchor, _)) = ANCHORS.get(m.anchor)
    {
        p["anchor"] = json!(anchor);
    }
    put(&mut p, &[("angle", &m.angle), ("body_name", m.name.trim())]);
    if m.mirror {
        p["mirror"] = json!(true);
    }
    if let Value::Object(o) = &mut p {
        for (key, v) in extra {
            o.entry(key.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("frame.member".to_string(), p)])
}

/// The dialog editing a frame member feature, filled from it.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::FrameMember { path_sketch, lines, profile, anchor, angle, mirror, body_name, .. } = kind else { return None };
    let (_, mut inputs) = start(app, "frame.member")?;
    let st = app.session.model.state();
    let mut m = Member::new();
    m.all = lines.is_empty();
    let ids = if m.all { frame::member_line_ids(&st, kind).unwrap_or_default() } else { lines.clone() };
    if let Some(i) = inputs.get_mut(0) {
        i.items = ids.into_iter().map(|id| Sel::ExternalSketchCurve { sketch: path_sketch.0, id }).collect();
    }
    match profile {
        MemberProfile::Library { profile } => {
            let kinds = frame::profile_kinds();
            if let Some((index, k)) = kinds.iter().enumerate().find(|(_, k)| k.kind == profile.kind)
                && let Some(values) = m.dims.get_mut(index)
            {
                m.kind = index;
                for (name, v) in k.required.iter().chain(k.optional).zip(values.iter_mut()) {
                    *v = profile.dims.get(*name).map(|e| shown_expr(e, ValueKind::Length)).unwrap_or_default();
                }
            }
        }
        MemberProfile::Sketch { sketch, profiles: ProfileSel::All } => m.whole = Some(sketch.0),
        MemberProfile::Sketch { sketch, profiles } => {
            let picks = st.sketch(sketch.0).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(i) = inputs.get_mut(1) {
                i.items = picks.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
            }
        }
    }
    let anchor = serde_json::to_value(anchor).ok();
    m.anchor = ANCHORS.iter().position(|(a, _)| anchor.as_ref().and_then(Value::as_str) == Some(*a)).unwrap_or(0);
    m.angle = angle.as_deref().map(|a| shown_expr(a, ValueKind::Angle)).unwrap_or_default();
    m.mirror = *mirror;
    m.name = body_name.clone().unwrap_or_default();
    Some((Kind::Frame(m), inputs, Map::new()))
}

#[cfg(test)]
#[path = "dialogs_frame_tests.rs"]
mod tests;
