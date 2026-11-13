//! Frame Member (`frame.member`): structural members along the lines of a frame sketch and between
//! pairs of points. Lines and points are picked in the view into the path, where consecutive points
//! pair up in pick order; the section is a built-in profile with its dimensions, or the profiles
//! picked in another sketch. Anchor, angle and mirror place the section on each member.

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{self, MemberProfile};
use solidcraft_engine::doc::{FeatureKind, ProfileSel};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, check, curves_sketch, form_row, profile_indices, put, shown_expr};
use crate::selection::{CONSTRUCTION_POINTS, ORIGIN, POINTS, PROFILES, SKETCH_LINES, SNAP_POINTS, SelInput, SelectionExt as _, VERTICES, is_point};
use crate::ui_kit::{choice_row, expr_row, hint_row, text_row};

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
    /// The path's items as last synced ([`Member::sync`]); `refs` and `pair_ids` are parallel to it.
    pub path: Vec<Sel>,
    /// For each path item, the lasting point reference (GeoRef JSON) sent back verbatim: the one an
    /// edit loaded it from, or the one a pick was turned into when it was taken, so a point is
    /// never found again from a pick (a snap point is forgotten as others wake); `None` for lines
    /// and for points that came in otherwise (`commands` turns those then). Change the path's
    /// items, then [`Member::sync`] realigns this to them.
    pub refs: Vec<Option<Value>>,
    /// For each path item, the id of the pair an edit loaded it in: a pair whose two points are
    /// still those two keeps its id.
    pub pair_ids: Vec<Option<String>>,
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
            path: Vec::new(),
            refs: Vec::new(),
            pair_ids: Vec::new(),
        }
    }

    /// Realign `refs` and `pair_ids` to the path's `items` after they changed: each point keeps
    /// what it was loaded with, matched in order (a point taken out leaves the others theirs).
    pub fn sync(&mut self, items: &[Sel]) {
        if self.path != items {
            (self.refs, self.pair_ids) = self.aligned(items);
            self.path = items.to_vec();
        }
    }

    /// `refs` and `pair_ids` for `items`, matched against `path` in order.
    fn aligned(&self, items: &[Sel]) -> (Vec<Option<Value>>, Vec<Option<String>>) {
        let mut from = 0;
        items
            .iter()
            .map(|item| {
                let found = if is_point(item) { self.path.iter().skip(from).position(|p| p == item) } else { None };
                let Some(j) = found.map(|k| from + k) else { return (None, None) };
                from = j + 1;
                (self.refs.get(j).cloned().flatten(), self.pair_ids.get(j).cloned().flatten())
            })
            .unzip()
    }
}

/// What the path takes: sketch lines, and the points pairs join.
const PATH: crate::selection::Accept = SKETCH_LINES | VERTICES | POINTS | CONSTRUCTION_POINTS | SNAP_POINTS | ORIGIN;
const NEEDS_PARTNER: &str = "A point needs another point after it";
const PICK_ANOTHER: &str = "Pick another point";

/// The dialog for Frame Member.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.member").then(|| {
        (Kind::Frame(Member::new()), vec![SelInput::new("Path", PATH, true).ordered_points(), SelInput::new("Section (optional)", PROFILES, true)])
    })
}

/// The pairing rule: consecutive points pair in pick order, and a line between them ends a run.
/// Returns the lines, the pairs and a last point still waiting for its partner; a point followed
/// by a line has none, and is refused.
pub fn pairs_of(items: &[Sel]) -> Result<(Vec<Sel>, Vec<(Sel, Sel)>, Option<Sel>), String> {
    let at = |i: usize| items.get(i).cloned();
    let (lines, pairs, waiting) = pair_indices(items)?;
    Ok((lines.into_iter().filter_map(at).collect(), pairs.into_iter().filter_map(|(a, b)| at(a).zip(at(b))).collect(), waiting.and_then(at)))
}

/// [`pairs_of`] as indices into `items`.
fn pair_indices(items: &[Sel]) -> Result<(Vec<usize>, Vec<(usize, usize)>, Option<usize>), String> {
    let (mut lines, mut pairs, mut waiting) = (Vec::new(), Vec::new(), None);
    for (i, item) in items.iter().enumerate() {
        if is_point(item) {
            match waiting.take() {
                Some(a) => pairs.push((a, i)),
                None => waiting = Some(i),
            }
        } else if matches!(item, Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. }) {
            if waiting.is_some() {
                return Err(NEEDS_PARTNER.into());
            }
            lines.push(i);
        }
    }
    Ok((lines, pairs, waiting))
}

/// A pick for the dialog's active input. A pick into Frame Member's path is tried as the live
/// preview builds it: one that would make the feature fail is taken back, and the dialog says why.
pub(crate) fn pick(app: &SolveApp, d: &mut Dialog, sel: Sel) {
    let before = (matches!(d.kind, Kind::Frame(_)) && d.active == 0).then(|| crate::selection::items(&d.inputs, 0).to_vec());
    d.pick(&app.session, sel);
    let Some(before) = before else { return };
    if let Some(why) = refusal(app, d, &before) {
        if let Some(path) = d.inputs.first_mut() {
            path.items = before;
        }
        d.error = Some(solidcraft_ui_text::format("frame.pick_refused", &[("reason", tr(&why))]));
        return;
    }
    prompt(d);
    // Each new point's reference is fixed as it is taken: a snap point is known only while it
    // stays among the woken ones (`snap_wake::snapped`), which a long session of picks outlives.
    let items = crate::selection::items(&d.inputs, 0);
    if let Kind::Frame(m) = &mut d.kind {
        for (i, item) in items.iter().enumerate().skip(before.len()).filter(|(_, item)| is_point(item)) {
            if let Some(r) = m.refs.get_mut(i).filter(|r| r.is_none()) {
                *r = point_ref(app, item).ok();
            }
        }
    }
}

/// Ctrl+click on a point of the path: its last occurrence goes. False when the click is not that.
pub(crate) fn take_out(app: &mut SolveApp, sel: &Sel) -> bool {
    let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, Kind::Frame(_)) && d.active == 0) else { return false };
    if !is_point(sel) || !d.inputs.first_mut().is_some_and(|path| path.ordered_points && path.remove_last(sel)) {
        return false;
    }
    d.focus = true;
    d.error = None;
    prompt(d);
    true
}

/// Is this the dialog's word that it took a pick back? Guidance, not a failure to report.
pub(crate) fn refused_pick(error: &str) -> bool {
    error.starts_with(&solidcraft_ui_text::format("frame.pick_refused", &[("reason", "")]))
}

/// After the path changed: a last point waiting for its partner asks for it.
fn prompt(d: &mut Dialog) {
    let waiting = pairs_of(crate::selection::items(&d.inputs, 0)).is_ok_and(|(_, _, w)| w.is_some());
    if waiting {
        d.error = Some(PICK_ANOTHER.into());
    } else if d.error.as_deref() == Some(PICK_ANOTHER) {
        d.error = None;
    }
    let items = crate::selection::items(&d.inputs, 0);
    if let Kind::Frame(m) = &mut d.kind {
        m.sync(items);
    }
}

/// Why the path's newest picks (those after `before`) would make the feature fail, if they would.
fn refusal(app: &SolveApp, d: &Dialog, before: &[Sel]) -> Option<String> {
    let items = crate::selection::items(&d.inputs, 0);
    if items.len() <= before.len() {
        return None;
    }
    let (lines, pairs, _) = match pairs_of(items) {
        Ok(p) => p,
        Err(e) => return Some(e),
    };
    let had = pairs_of(before).map(|(l, p, _)| (l.len(), p.len())).unwrap_or_default();
    // A point now waiting for its partner builds nothing new.
    if (lines.len(), pairs.len()) == had {
        return None;
    }
    // What fails already (a value, a point gone) is not this pick's doing: its own error shows.
    if had != (0, 0) {
        let mut was = d.clone();
        if let Some(path) = was.inputs.first_mut() {
            path.items = before.to_vec();
        }
        if app.preview.error.is_some() || crate::dialogs::apply_commands(app, &was).is_err() {
            return None;
        }
    }
    match crate::dialogs::apply_commands(app, d) {
        Ok(cmds) => app.session.preview(&cmds).err().map(|e| e.to_string()),
        Err(e) => Some(e),
    }
}

fn tr(text: &str) -> &str {
    solidcraft_ui_text::tr_label(text)
}

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &SolveApp, ui: &mut egui::Ui, m: &mut Member, inputs: &[SelInput]) -> bool {
    let mut enter = false;
    m.sync(crate::selection::items(inputs, 0));
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
    let (line_picks, pair_picks, _) = pair_indices(picks)?;
    let mut lines = Vec::new();
    let mut known = None;
    for pick in line_picks.iter().filter_map(|i| picks.get(*i)) {
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
    // A point goes back as the reference it was loaded or picked as, and a pair whose two points
    // an edit loaded together keeps its id.
    let (refs, ids) = m.aligned(picks);
    let end = |i: usize| match refs.get(i).cloned().flatten() {
        Some(r) => Ok(r),
        None => point_ref(app, picks.get(i).ok_or("the path changed")?),
    };
    let mut pairs = Vec::with_capacity(pair_picks.len());
    for (a, b) in pair_picks {
        let mut pair = json!({"a": end(a)?, "b": end(b)?});
        if let (Some(Some(x)), Some(Some(y))) = (ids.get(a), ids.get(b))
            && x == y
        {
            pair["id"] = json!(x);
        }
        pairs.push(pair);
    }
    if lines.is_empty() && pairs.is_empty() {
        return Err("select lines or points for the path first".into());
    }
    let mut p = json!({});
    if !lines.is_empty() {
        p["sketch"] =
            json!(known.or_else(|| curves_sketch(app, &lines, section_sketch)).ok_or("the lines must be in one sketch, not the section's")?);
        if !m.all {
            p["lines"] = json!(lines);
        }
    }
    if !pairs.is_empty() {
        p["pairs"] = json!(pairs);
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

/// A point picked into the path as `frame.member` takes it; the origin stays the origin.
fn point_ref(app: &SolveApp, sel: &Sel) -> Result<Value, String> {
    match sel {
        Sel::Vertex { body, point }
            if body.is_empty() && [point.x, point.y, point.z].iter().all(|v| v.abs() < 1e-9) && crate::snap_wake::snapped(app, *point).is_none() =>
        {
            Ok(json!({"type": "origin"}))
        }
        _ => crate::dialogs::construct_ref(app, sel),
    }
}

/// The dialog editing a frame member feature, filled from it: the path holds its lines, then each
/// pair's two points, each point keeping the reference it was saved as.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::FrameMember { path_sketch, lines, pairs, profile, anchor, locate, angle, mirror, body_name, .. } = kind else { return None };
    let (_, mut inputs) = start(app, "frame.member")?;
    let st = app.session.model.state();
    let mut m = Member::new();
    m.all = lines.is_empty() && path_sketch.is_some();
    // The sketch's lines (without the pairs' ids, which come after them).
    let ids = if m.all { frame::member_line_ids(&st, kind).unwrap_or_default() } else { lines.clone() };
    let ids = ids.into_iter().filter(|id| !pairs.iter().any(|p| p.id == *id));
    let mut items: Vec<Sel> = path_sketch.map(|ps| ids.map(|id| Sel::ExternalSketchCurve { sketch: ps.0, id }).collect()).unwrap_or_default();
    m.refs = vec![None; items.len()];
    m.pair_ids = vec![None; items.len()];
    for pair in pairs {
        for end in [&pair.a, &pair.b] {
            items.push(crate::dialogs::point_pick(&app.session, end));
            m.refs.push(serde_json::to_value(end).ok());
            m.pair_ids.push(Some(pair.id.clone()));
        }
    }
    m.path.clone_from(&items);
    if let Some(i) = inputs.get_mut(0) {
        i.items = items;
    }
    match profile {
        MemberProfile::Library { profile } => {
            let kinds = frame::profile_kinds();
            if let Some((index, k)) = kinds.iter().enumerate().find(|(_, k)| k.kind == profile.kind)
                && let Some(values) = m.dims.get_mut(index)
            {
                m.kind = index;
                for (name, v) in k.required.iter().chain(k.optional).zip(values.iter_mut()) {
                    *v = profile.dims.get(*name).map(|e| shown_expr(e, frame::dimension_kind(name))).unwrap_or_default();
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
    // The dialog has no field for a located point: an edit keeps it, so the member stays where it
    // is attached.
    let mut extra = Map::new();
    if let Some(at) = locate {
        extra.insert("locate".into(), json!(at));
    }
    Some((Kind::Frame(m), inputs, extra))
}

#[cfg(test)]
#[path = "dialogs_frame_tests.rs"]
mod tests;
