//! Frame Member (`frame.member`): structural members along the lines of a frame sketch. The lines
//! are picked in the view; the section is a built-in profile with its dimensions, or the profiles
//! picked in another sketch. Anchor, angle and mirror place the section on each line. The lines
//! may come in groups, each with its own anchor, angle, mirror and name: the Lines input and those
//! rows show the group chosen in the group list.

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{self, MemberProfile};
use solidcraft_engine::doc::{FeatureKind, ProfileSel};

use crate::SolveApp;
use crate::dialogs::{Kind, check, curves_sketch, form_row, profile_indices, put, shown_expr};
use crate::selection::{PROFILES, SKETCH_LINES, SelInput, SelectionExt as _};
use crate::ui_kit::{Button, button, choice_row, expr_row, hint_row, text_row};

/// The anchors as `frame.member` names them, and their labels.
const ANCHORS: &[(&str, &str)] = &[
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

/// A group of lines and how its members sit on them, while another group is shown.
#[derive(Clone, Debug, Default)]
pub struct Group {
    pub lines: Vec<Sel>,
    pub anchor: usize,
    pub angle: String,
    pub mirror: bool,
    pub name: String,
}

/// The Frame Member dialog's values (the picks are its selection inputs).
#[derive(Clone, Debug)]
pub struct Member {
    /// Every line of the frame sketch, now and as lines are added, not only the lines picked
    /// (with one group only).
    pub all: bool,
    /// The built-in profile (an index into `frame::profile_kinds`), used when no section is picked.
    pub kind: usize,
    /// Each built-in profile's dimensions, its required ones then its optional ones; kept per
    /// profile so switching back finds them again. An empty optional one takes its default.
    pub dims: Vec<Vec<String>>,
    /// A profile sketch whose whole drawn section is the member's (nested loops alternating), as
    /// a feature made that way keeps it while no section is picked.
    pub whole: Option<u64>,
    /// The shown group's anchor, angle, mirror and name (its lines are the Lines input).
    pub anchor: usize,
    pub angle: String,
    pub mirror: bool,
    pub name: String,
    /// Every group; the shown one's entry is brought up to date when another is shown.
    pub groups: Vec<Group>,
    /// The shown group, an index into `groups`.
    pub current: usize,
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
        Member {
            all: false,
            kind,
            dims,
            whole: None,
            anchor: 0,
            angle: String::new(),
            mirror: false,
            name: String::new(),
            groups: vec![Group::default()],
            current: 0,
        }
    }

    /// The shown group as it stands, its lines from the Lines input.
    fn shown(&self, inputs: &[SelInput]) -> Group {
        let lines = crate::selection::items(inputs, 0).to_vec();
        Group { lines, anchor: self.anchor, angle: self.angle.clone(), mirror: self.mirror, name: self.name.clone() }
    }

    /// Every group as it stands, the shown one included (none is while `current` is past the end).
    pub fn all_groups(&self, inputs: &[SelInput]) -> Vec<Group> {
        let mut groups = self.groups.clone();
        if let Some(g) = groups.get_mut(self.current) {
            *g = self.shown(inputs);
        }
        groups
    }

    /// Show group `k`: the shown one is kept, `k`'s lines go into the Lines input.
    pub fn show(&mut self, k: usize, inputs: &mut [SelInput]) {
        let groups = self.all_groups(inputs);
        let Some(g) = groups.get(k).cloned() else { return };
        self.groups = groups;
        self.current = k;
        self.anchor = g.anchor;
        self.angle = g.angle;
        self.mirror = g.mirror;
        self.name = g.name;
        if let Some(i) = inputs.first_mut() {
            i.items = g.lines;
        }
    }

    /// A new, empty group after the others, shown.
    pub fn add_group(&mut self, inputs: &mut [SelInput]) {
        self.groups = self.all_groups(inputs);
        self.groups.push(Group::default());
        self.all = false;
        self.show(self.groups.len() - 1, inputs);
    }

    /// The shown group taken out (never the last one), its neighbour shown.
    pub fn remove_group(&mut self, inputs: &mut [SelInput]) {
        let mut groups = self.all_groups(inputs);
        if groups.len() < 2 || self.current >= groups.len() {
            return;
        }
        groups.remove(self.current);
        let k = self.current.min(groups.len() - 1);
        self.groups = groups;
        // The removed group must not be written back over its neighbour.
        self.current = usize::MAX;
        self.show(k, inputs);
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

/// The group list: a button per group (the shown one marked), then New Group and Remove Group.
fn group_rows(ui: &mut egui::Ui, m: &mut Member, inputs: &mut [SelInput]) {
    let count = m.groups.len().max(1);
    let mut show = None;
    let (mut add, mut remove) = (false, false);
    form_row(ui, crate::i18n::tr("ui.frame_member.groups"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for k in 0..count {
                let label = crate::i18n::format("ui.frame_member.group", &[("number", &(k + 1).to_string())]);
                let style = if k == m.current { Button::SmallSelected } else { Button::Small };
                if button(ui, label, style).clicked() && k != m.current {
                    show = Some(k);
                }
            }
            add = button(ui, crate::i18n::tr("ui.frame_member.new_group"), Button::Small).clicked();
            if count > 1 {
                remove = button(ui, crate::i18n::tr("ui.frame_member.remove_group"), Button::Small).clicked();
            }
        });
    });
    if let Some(k) = show {
        m.show(k, inputs);
    } else if add {
        m.add_group(inputs);
    } else if remove {
        m.remove_group(inputs);
    }
}

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &SolveApp, ui: &mut egui::Ui, m: &mut Member, inputs: &mut [SelInput]) -> bool {
    let mut enter = false;
    group_rows(ui, m, inputs);
    if m.groups.len() <= 1 {
        form_row(ui, "All Lines", |ui| check(ui, &mut m.all));
    }
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

/// The ids of the lines picked, `sketch` the frame sketch they are in once one names it (line ids
/// are a sketch's own).
fn picked_lines(picks: &[Sel], sketch: &mut Option<u64>) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for pick in picks {
        match pick {
            Sel::ExternalSketchCurve { sketch: s, id } | Sel::SketchCurve { id, sketch: Some(s) } => {
                if sketch.is_some_and(|k| k != *s) {
                    return Err("the lines must be in one sketch".into());
                }
                *sketch = Some(*s);
                lines.push(id.clone());
            }
            Sel::SketchCurve { id, sketch: None } => lines.push(id.clone()),
            _ => {}
        }
    }
    Ok(lines)
}

/// A group's anchor, angle, mirror and name in `p`.
fn put_placement(p: &mut Value, g: &Group) {
    if g.anchor != 0
        && let Some((anchor, _)) = ANCHORS.get(g.anchor)
    {
        p["anchor"] = json!(anchor);
    }
    put(p, &[("angle", &g.angle), ("body_name", g.name.trim())]);
    if g.mirror {
        p["mirror"] = json!(true);
    }
}

/// The command the dialog's OK runs (`extra`: parameters kept from the feature being edited).
pub fn commands(app: &SolveApp, m: &Member, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let section = crate::selection::items(inputs, 1).profiles();
    let section_sketch = section.first().map(|x| x.0).or(m.whole);
    if section.iter().any(|x| Some(x.0) != section_sketch) {
        return Err("the section's profiles must be in one sketch".into());
    }
    // A line picked in the view comes with its sketch; a line selected before the dialog opened
    // is looked for in the newest sketch with it, not the section's.
    let groups = m.all_groups(inputs);
    let mut known = None;
    let lines = groups.iter().map(|g| picked_lines(&g.lines, &mut known)).collect::<Result<Vec<_>, _>>()?;
    if lines.iter().all(Vec::is_empty) {
        return Err("select the frame sketch's lines first".into());
    }
    let every: Vec<String> = lines.concat();
    let sketch = known.or_else(|| curves_sketch(app, &every, section_sketch)).ok_or("the lines must be in one sketch, not the section's")?;
    let mut p = json!({"sketch": sketch});
    if let ([g], [lines]) = (groups.as_slice(), lines.as_slice()) {
        if !m.all {
            p["lines"] = json!(lines);
        }
        put_placement(&mut p, g);
    } else {
        let mut list = Vec::with_capacity(groups.len());
        for (k, (g, lines)) in groups.iter().zip(&lines).enumerate() {
            if lines.is_empty() {
                return Err(format!("group {} has no lines", k + 1));
            }
            let mut group = json!({"lines": lines});
            put_placement(&mut group, g);
            list.push(group);
        }
        p["groups"] = json!(list);
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
    if let Value::Object(o) = &mut p {
        for (key, v) in extra {
            o.entry(key.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("frame.member".to_string(), p)])
}

/// The dialog editing a frame member feature, filled from it.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::FrameMember { path_sketch, lines, profile, .. } = kind else { return None };
    let (_, mut inputs) = start(app, "frame.member")?;
    let st = app.session.model.state();
    let mut m = Member::new();
    m.all = lines.is_empty() && frame::member_groups(kind).len() == 1;
    let picks = |ids: Vec<String>| ids.into_iter().map(|id| Sel::ExternalSketchCurve { sketch: path_sketch.0, id }).collect();
    m.groups = frame::member_groups(kind)
        .into_iter()
        .map(|g| {
            let ids = if g.lines.is_empty() { frame::member_line_ids(&st, kind).unwrap_or_default() } else { g.lines };
            let anchor = serde_json::to_value(g.anchor).ok();
            Group {
                lines: picks(ids),
                anchor: ANCHORS.iter().position(|(a, _)| anchor.as_ref().and_then(Value::as_str) == Some(*a)).unwrap_or(0),
                angle: g.angle.as_deref().map(|a| shown_expr(a, ValueKind::Angle)).unwrap_or_default(),
                mirror: g.mirror,
                name: g.body_name.unwrap_or_default(),
            }
        })
        .collect();
    if m.groups.is_empty() {
        m.groups.push(Group::default());
    }
    m.current = usize::MAX;
    m.show(0, &mut inputs);
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
    Some((Kind::Frame(m), inputs, Map::new()))
}

#[cfg(test)]
#[path = "dialogs_frame_tests.rs"]
mod tests;
