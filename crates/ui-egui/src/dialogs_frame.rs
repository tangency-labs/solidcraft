//! Frame Member (`frame.member`): structural members along the lines of a frame sketch. The lines
//! are picked in the view; the section is a built-in profile with its dimensions, or the profiles
//! picked in another sketch, or a profile of the design's profile library. A placement point, angle
//! and mirror place the section on each line.

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{self, MemberProfile};
use solidcraft_engine::doc::frame_library;
use solidcraft_engine::doc::{FeatureKind, ProfileSel};

use crate::SolveApp;
use crate::dialogs::{Kind, check, curves_sketch, form_row, profile_indices, put, shown_expr};
use crate::selection::{PROFILES, SKETCH_LINES, SelInput, SelectionExt as _};
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
    /// A library profile by id, used when no section is picked.
    pub library: Option<String>,
    /// The library folder browsed (its category levels, outermost first); `None` is the built-in
    /// profiles, an empty folder the profiles without a category.
    pub folder: Option<Vec<String>>,
    /// The name Save to Library gives the section; empty names it after the profile.
    pub save_name: String,
    /// The category Save to Library files the section under, levels split by `/`; empty files it
    /// under the profile's kind.
    pub save_category: String,
    /// A placement point by name beyond the anchors (a kind's own points, a library profile's), or
    /// with a library profile any point the member names instead of the profile's default.
    pub point: Option<String>,
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
        Member {
            all: false,
            kind,
            dims,
            whole: None,
            anchor: 0,
            library: None,
            folder: None,
            save_name: String::new(),
            save_category: String::new(),
            point: None,
            angle: String::new(),
            mirror: false,
            name: String::new(),
        }
    }
}

/// The dialog for Frame Member.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.member")
        .then(|| (Kind::Frame(Member::new()), vec![SelInput::new("Lines", SKETCH_LINES, true), SelInput::new("Section (optional)", PROFILES, true)]))
}

fn tr(text: &str) -> &str {
    solidcraft_ui_text::tr_label(text)
}

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, m: &mut Member, inputs: &[SelInput]) -> bool {
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
        library_rows(app, ui, m);
        if m.library.is_none() {
            let kinds = frame::profile_kinds();
            let labels: Vec<&str> = kinds.iter().map(|k| k.label).collect();
            if choice_row(ui, "Profile", "frame_profile", &mut m.kind, &labels).changed() {
                m.point = None;
            }
            if let (Some(k), Some(values)) = (kinds.get(m.kind), m.dims.get_mut(m.kind)) {
                for (i, (name, v)) in k.required.iter().chain(k.optional).zip(values.iter_mut()).enumerate() {
                    let label = frame::dimension_label(name);
                    enter |= if i < k.required.len() { expr_row(ui, label, v) } else { hint_row(ui, label, v, tr("default")) };
                }
            }
        }
    }
    if m.library.is_none() {
        let (name, category) = save_defaults(app, m, inputs);
        enter |= text_row(ui, "ui.frame_save_name", &mut m.save_name, &name).0;
        enter |= text_row(ui, "ui.frame_category", &mut m.save_category, &category).0;
        if crate::ui_kit::button(ui, solidcraft_ui_text::tr("ui.frame_save_to_library"), crate::ui_kit::Button::Normal).clicked() {
            match save_to_library(app, m, inputs) {
                Ok(id) => {
                    m.folder = app.session.doc.library_profile(&id).map(|e| e.category.clone());
                    m.library = Some(id);
                    m.whole = None;
                    m.save_name.clear();
                    m.save_category.clear();
                }
                Err(e) => app.set_status(e, true),
            }
        }
    }
    point_row(app, ui, m, picked || m.whole.is_some());
    enter |= hint_row(ui, "Angle", &mut m.angle, "0 deg");
    form_row(ui, "Mirror", |ui| check(ui, &mut m.mirror));
    enter |= text_row(ui, "Name", &mut m.name, tr("from the profile")).0;
    enter
}

/// The library's rows: Type (Built-in or a top-level category), a Group row for each level of
/// subcategories, and Size (the profiles in the chosen folder and below). Choosing a folder picks
/// its first profile, so a library profile is always chosen unless Type is Built-in.
fn library_rows(app: &SolveApp, ui: &mut egui::Ui, m: &mut Member) {
    let library = &app.session.doc.frame_profiles;
    if library.is_empty() {
        (m.library, m.folder) = (None, None);
        return;
    }
    // A member edited or a profile just saved names its profile: open the profile's folder.
    if m.folder.is_none()
        && let Some(e) = m.library.as_ref().and_then(|id| library.iter().find(|e| &e.id == id))
    {
        m.folder = Some(e.category.clone());
    }
    let tops = subfolders(library, &[]);
    let uncategorised = library.iter().any(|e| e.category.is_empty());
    let mut labels: Vec<&str> = vec![tr("Built-in")];
    labels.extend(tops.iter().map(String::as_str));
    if uncategorised {
        labels.push(tr("ui.frame_uncategorised"));
    }
    let mut index = match &m.folder {
        None => 0,
        Some(f) => match f.first() {
            Some(top) => tops.iter().position(|t| t == top).map_or(0, |i| i + 1),
            None if uncategorised => tops.len() + 1,
            None => 0,
        },
    };
    let before = index;
    choice_row(ui, "Type", "frame_library", &mut index, &labels);
    if index != before {
        m.folder = index.checked_sub(1).map(|i| tops.get(i).map(|t| vec![t.clone()]).unwrap_or_default());
    }
    let Some(mut folder) = m.folder.clone() else {
        if m.library.take().is_some() {
            (m.point, m.anchor) = (None, 0);
        }
        return;
    };
    for level in 1..frame_library::MAX_CATEGORY_LEVELS {
        if folder.len() < level {
            break;
        }
        let subs = subfolders(library, &folder[..level]);
        if subs.is_empty() {
            break;
        }
        let mut labels: Vec<&str> = vec![tr("All")];
        labels.extend(subs.iter().map(String::as_str));
        let mut index = folder.get(level).and_then(|g| subs.iter().position(|s| s == g)).map_or(0, |i| i + 1);
        let before = index;
        choice_row(ui, "ui.frame_group", &format!("frame_library_group_{level}"), &mut index, &labels);
        if index != before {
            folder.truncate(level);
            if let Some(g) = index.checked_sub(1).and_then(|i| subs.get(i)) {
                folder.push(g.clone());
            }
        }
    }
    let entries = profiles_in(library, &folder);
    let Some(first) = entries.first() else {
        // The folder is gone (its profiles were removed or moved): back to the built-in profiles.
        (m.library, m.folder) = (None, None);
        return;
    };
    let labels: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut index = m.library.as_ref().and_then(|id| entries.iter().position(|e| &e.id == id)).unwrap_or(0);
    choice_row(ui, "Size", "frame_library_size", &mut index, &labels);
    let chosen = entries.get(index).unwrap_or(first).id.clone();
    if m.library.as_ref() != Some(&chosen) {
        // Another profile has other points: start from its default again.
        m.point = None;
        m.anchor = 0;
        m.library = Some(chosen);
    }
    m.folder = Some(folder);
}

/// The folders one level below `folder`, in natural order (`50x25` before `100x50`).
fn subfolders(library: &[frame_library::LibraryProfile], folder: &[String]) -> Vec<String> {
    let mut subs: Vec<String> =
        library.iter().filter(|e| e.category.starts_with(folder)).filter_map(|e| e.category.get(folder.len()).cloned()).collect();
    subs.sort_by(|a, b| natural(a, b));
    subs.dedup();
    subs
}

/// The profiles in `folder` and its subfolders, in natural order by name; an empty folder holds the
/// profiles without a category.
fn profiles_in<'a>(library: &'a [frame_library::LibraryProfile], folder: &[String]) -> Vec<&'a frame_library::LibraryProfile> {
    let mut entries: Vec<_> =
        library.iter().filter(|e| if folder.is_empty() { e.category.is_empty() } else { e.category.starts_with(folder) }).collect();
    entries.sort_by(|a, b| natural(&a.name, &b.name));
    entries
}

/// Natural order: numbers compare by value (`63.5x38` after `50x25`, before `100x50`), the rest
/// ignoring case.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn tokens(s: &str) -> Vec<Result<f64, char>> {
        let mut out = Vec::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            if c.is_ascii_digit() {
                let start = i;
                while chars.get(i).is_some_and(|c| c.is_ascii_digit())
                    || (chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) && !chars[start..i].contains(&'.'))
                {
                    i += 1;
                }
                let number: String = chars[start..i].iter().collect();
                out.push(number.parse::<f64>().map_err(|_| c));
            } else {
                out.extend(c.to_lowercase().map(Err));
                i += 1;
            }
        }
        out
    }
    let order = |x: &Result<f64, char>, y: &Result<f64, char>| match (x, y) {
        (Ok(x), Ok(y)) => x.total_cmp(y),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(x), Err(y)) => x.cmp(y),
    };
    let (ta, tb) = (tokens(a), tokens(b));
    ta.iter().zip(&tb).map(|(x, y)| order(x, y)).find(|o| o.is_ne()).unwrap_or_else(|| ta.len().cmp(&tb.len())).then_with(|| a.cmp(b))
}

/// What Save to Library names and files the section as when its name and category are empty: the
/// built-in profile's description (`RHS 100 × 50 × 3`) under its kind, or the section sketch's name
/// under Custom. The name gets ` (2)`… when the library has it already.
fn save_defaults(app: &SolveApp, m: &Member, inputs: &[SelInput]) -> (String, String) {
    let doc = &app.session.doc;
    let section = crate::selection::items(inputs, 1).profiles();
    let (base, category) = match section.first().map(|x| x.0).or(m.whole) {
        Some(sketch) => (doc.feature(sketch).map(|f| f.name.clone()).unwrap_or_else(|| "Profile".into()), "Custom".to_string()),
        None => {
            let category = frame::profile_kinds().get(m.kind).map(|k| k.label.to_string()).unwrap_or_default();
            let name = builtin_profile(m)
                .ok()
                .and_then(|p| serde_json::from_value::<frame::ProfileSpec>(p).ok())
                .and_then(|spec| spec.resolve(|_, e, kind| doc.eval(&doc.with_design_unit(e, kind), kind)).ok())
                .map(|p| p.description())
                .unwrap_or_else(|| category.clone());
            (name, category)
        }
    };
    let taken = |n: &str| doc.frame_profiles.iter().any(|e| e.name.eq_ignore_ascii_case(n));
    let name = std::iter::once(base.clone()).chain((2..1000).map(|i| format!("{base} ({i})"))).find(|n| !taken(n)).unwrap_or(base);
    (name, category)
}

/// Adds the dialog's section to the design's profile library (`frame.library.add`): the built-in
/// profile with its dimensions, or the section sketch (its picked profiles), named and filed as
/// Save As and Category say (`Steel/RHS` is two levels), else as `save_defaults` does. Returns the
/// new profile's id.
fn save_to_library(app: &mut SolveApp, m: &Member, inputs: &[SelInput]) -> Result<String, String> {
    let section = crate::selection::items(inputs, 1).profiles();
    let mut p = match section.first().map(|x| x.0).or(m.whole) {
        Some(sketch) => {
            let mut p = json!({"profile_sketch": sketch});
            if !section.is_empty() {
                p["profiles"] = json!(section.iter().map(|x| x.1).collect::<Vec<_>>());
            }
            p
        }
        None => json!({"profile": builtin_profile(m)?}),
    };
    let (name, category) = save_defaults(app, m, inputs);
    let name = Some(m.save_name.trim()).filter(|n| !n.is_empty()).map_or(name, str::to_string);
    let category = Some(m.save_category.trim()).filter(|c| !c.is_empty()).map_or(category, str::to_string);
    p["name"] = json!(name);
    p["category"] = json!(category.split('/').map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>());
    let out = app.run("frame.library.add", p)?;
    out["profile"]["id"].as_str().map(str::to_string).ok_or_else(|| "the library profile has no id".to_string())
}

/// The built-in profile the dialog's values describe, as `frame.member` takes it.
fn builtin_profile(m: &Member) -> Result<Value, String> {
    let kinds = frame::profile_kinds();
    let (k, values) = kinds.get(m.kind).zip(m.dims.get(m.kind)).ok_or("choose a profile")?;
    let mut profile = json!({"kind": k.kind});
    for (name, v) in k.required.iter().chain(k.optional).zip(values) {
        if !v.trim().is_empty() {
            profile[*name] = json!(v);
        }
    }
    Ok(profile)
}

/// The placement points the dialog offers, by name: the anchors, then the points the profile's kind
/// generates, then the points a library profile names. A sketch section has the anchors only.
fn point_names(app: &SolveApp, m: &Member, sketch_section: bool) -> Vec<String> {
    let mut names: Vec<String> = ANCHORS.iter().map(|a| a.0.to_string()).collect();
    if sketch_section {
        return names;
    }
    let kinds = frame::profile_kinds();
    let (kind, named) = match m.library.as_ref().and_then(|id| app.session.doc.library_profile(id)) {
        Some(e) => {
            let kind = match &e.section {
                frame_library::LibrarySection::BuiltIn { profile } => Some(profile.kind.as_str()),
                frame_library::LibrarySection::Drawn { .. } => None,
            };
            (kind, e.points.iter().map(|p| p.name.clone()).collect())
        }
        None => (kinds.get(m.kind).map(|k| k.kind), Vec::new()),
    };
    if let Some(example) = kind.and_then(|kind| kinds.iter().find(|k| k.kind == kind)).map(|k| &k.example) {
        names.extend(frame_library::generated_points(example).into_iter().map(|(n, _)| n.to_string()));
    }
    names.extend(named);
    names
}

/// A placement point's label: an anchor's own, else its name with spaces (`heel_inside` → `Heel
/// inside`).
fn point_label(name: &str) -> String {
    if let Some((_, label)) = ANCHORS.iter().find(|a| a.0 == name) {
        return tr(label).to_string();
    }
    let spaced = name.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The Point row: which placement point sits on each line. With a library profile the first
/// choice is its default point; otherwise it is the origin.
fn point_row(app: &SolveApp, ui: &mut egui::Ui, m: &mut Member, sketch_section: bool) {
    let names = point_names(app, m, sketch_section);
    let library = m.library.as_ref().and_then(|id| app.session.doc.library_profile(id));
    let mut labels: Vec<String> = Vec::new();
    if let Some(e) = library {
        let default = e.default_point.as_deref().unwrap_or("origin");
        labels.push(format!("{} ({})", tr("Default"), point_label(default)));
    }
    let offset = labels.len();
    labels.extend(names.iter().map(|n| point_label(n)));
    let mut index = match &m.point {
        Some(n) => names.iter().position(|x| x == n).map_or(0, |i| i + offset),
        None if offset > 0 => 0,
        None => m.anchor,
    };
    let shown: Vec<&str> = labels.iter().map(String::as_str).collect();
    choice_row(ui, "Point", "frame_point", &mut index, &shown);
    let picked = index.checked_sub(offset).and_then(|i| names.get(i));
    match picked {
        // With a library profile, any point it names wins over its default.
        Some(name) if library.is_some() => (m.anchor, m.point) = (0, Some(name.clone())),
        Some(name) => match ANCHORS.iter().position(|a| a.0 == name) {
            Some(anchor) => (m.anchor, m.point) = (anchor, None),
            None => (m.anchor, m.point) = (0, Some(name.clone())),
        },
        None => (m.anchor, m.point) = (0, None),
    }
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
    } else if let Some(id) = &m.library {
        p["library"] = json!(id);
    } else {
        p["profile"] = builtin_profile(m)?;
    }
    if m.anchor != 0
        && let Some((anchor, _)) = ANCHORS.get(m.anchor)
    {
        p["anchor"] = json!(anchor);
    }
    if let Some(point) = &m.point {
        p["point"] = json!(point);
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
    let FeatureKind::FrameMember { path_sketch, lines, profile, anchor, locate, point, angle, mirror, body_name, .. } = kind else { return None };
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
                    *v = profile.dims.get(*name).map(|e| shown_expr(e, frame::dimension_kind(name))).unwrap_or_default();
                }
            }
        }
        MemberProfile::LibraryProfile { id } => m.library = Some(id.clone()),
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
    m.point = point.clone();
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
