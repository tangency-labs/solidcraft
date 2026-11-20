//! What a VR controller's ray points at (#2172, docs/vr.md): the part under it, its name, the
//! bodies to light while it is pointed at, and selecting it as the Browser would. The selection
//! is the design's own, so the desktop window shows it too.

use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;

/// What a ray stops at.
#[derive(Clone, Debug, PartialEq)]
pub struct RayHit {
    /// The body the ray stops at.
    pub body: String,
    /// How far along the ray (model units).
    pub depth: f64,
    /// Where it stops (model units).
    pub point: [f64; 3],
}

/// A part as VR names, lights and selects it: an occurrence as the Browser shows it, or a body of
/// the root component.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// What selecting it selects.
    pub sel: Sel,
    /// Its name as the Browser shows it.
    pub name: String,
    /// The bodies lit while it is pointed at.
    pub bodies: Vec<String>,
}

/// One plane of a section: a point on it and the side it cuts away (model units).
pub type Cut = ([f64; 3], [f64; 3]);

/// The body the ray from `origin` along `direction` (model units, any length but zero) stops at,
/// as a click in the design view would pick it; `None` when it misses or is not a ray. What the
/// `section`'s planes cut away is passed through, as the design view's section does; with none,
/// nothing is cut away. The design view's own section is [`view_section`], worked out once for
/// all the rays of a frame: with the Section dialog open it is worked out on a copy of the
/// design. A plane that is not one (a coordinate that is no number, no side) is no ray's.
pub fn hit(app: &SolveApp, origin: [f64; 3], direction: [f64; 3], section: &[Cut]) -> Option<RayHit> {
    let o = Vec3::new(origin[0], origin[1], origin[2]);
    let d = Vec3::new(direction[0], direction[1], direction[2]).normalized()?;
    if ![o.x, o.y, o.z].iter().all(|v| v.is_finite()) {
        return None;
    }
    let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let most = solidcraft_viewport_gpu::view::MAX_SECTION_PLANES;
    let mut planes = Vec::with_capacity(section.len().min(most));
    for (i, (at, toward)) in section.iter().take(most).enumerate() {
        if !at.iter().all(|c| c.is_finite()) {
            return None;
        }
        planes.push(solidcraft_engine::SectionPlane { id: i as u64 + 1, origin: v(*at), normal: v(*toward).normalized()? });
    }
    let (body, depth, point) = crate::view_pick::nearest_body(app, o, d, &planes)?;
    Some(RayHit { body, depth, point: [point.x, point.y, point.z] })
}

/// The design view's own section, as [`hit`] takes it: the open Section dialog's planes, a
/// Slice's, or the design's section; none without one.
pub fn view_section(app: &SolveApp) -> Vec<Cut> {
    crate::viewport::section_planes(app).iter().map(|p| ([p.origin.x, p.origin.y, p.origin.z], [p.normal.x, p.normal.y, p.normal.z])).collect()
}

/// The part `body` belongs to: its outermost placement, as the Browser selects it, or the body
/// itself in the root component.
pub fn part_of(app: &SolveApp, body: &str) -> Part {
    let sel = crate::viewport::component_selection(app, body);
    match &sel {
        Sel::Occurrence { path } => {
            let parent = &path[..path.len().saturating_sub(1)];
            let name = app
                .session
                .occurrence_children(parent)
                .ok()
                .and_then(|children| children.into_iter().find(|c| &c.path == path))
                .map_or_else(|| body.to_string(), |c| c.name);
            let bodies = app.session.occurrence_bodies(path, true).unwrap_or_else(|_| vec![body.to_string()]);
            Part { sel, name, bodies }
        }
        _ => Part { sel, name: body.to_string(), bodies: vec![body.to_string()] },
    }
}

/// Whether `part` is selected now.
pub fn is_selected(app: &SolveApp, part: &Part) -> bool {
    app.session.selection.iter().any(|s| crate::selection::same_item(s, &part.sel))
}

/// The trigger pulled on `part`: it joins the selection, or leaves it when it is in already. On
/// nothing (`None`), the selection is cleared, as a click on empty space clears it.
pub fn toggle(app: &mut SolveApp, part: Option<&Part>) {
    crate::viewport::select(app, part.map(|p| p.sel.clone()), part.is_some(), true);
}

/// What an action on `pointed` acts on: the whole selection when `pointed` is in it (as the
/// desktop's menu on a selected item acts on the selection), else `pointed` alone. Selected items
/// that are not parts (faces, features, sketches) are left out.
pub fn targets(app: &SolveApp, pointed: &Part) -> Vec<Part> {
    if !is_selected(app, pointed) {
        return vec![pointed.clone()];
    }
    let mut parts = Vec::new();
    for sel in &app.session.selection {
        let part = match sel {
            Sel::Occurrence { path } => {
                let bodies = app.session.occurrence_bodies(path, true).unwrap_or_default();
                let Some(first) = bodies.first() else { continue };
                Part { name: part_of(app, first).name, sel: sel.clone(), bodies }
            }
            Sel::Body { name } => Part { sel: sel.clone(), name: name.clone(), bodies: vec![name.clone()] },
            _ => continue,
        };
        parts.push(part);
    }
    if parts.is_empty() { vec![pointed.clone()] } else { parts }
}

/// The parts selected now (placements and bodies; faces, features and sketches left out).
pub fn selected_parts(app: &SolveApp) -> Vec<Part> {
    let mut parts = Vec::new();
    for sel in &app.session.selection {
        match sel {
            Sel::Occurrence { path } => {
                let bodies = app.session.occurrence_bodies(path, true).unwrap_or_default();
                let Some(first) = bodies.first() else { continue };
                parts.push(Part { name: part_of(app, first).name, sel: sel.clone(), bodies });
            }
            Sel::Body { name } => parts.push(Part { sel: sel.clone(), name: name.clone(), bodies: vec![name.clone()] }),
            _ => {}
        }
    }
    parts
}

/// What an action on the parts acts on: the part pointed at (with the selection, when it is in
/// it), else the selection. Empty: nothing to act on.
pub fn acted_on(app: &SolveApp, pointed: Option<&Part>) -> Vec<Part> {
    match pointed {
        Some(part) => targets(app, part),
        None => selected_parts(app),
    }
}

/// Whether every one of `parts` is hidden now (a placement as a placement, or all its bodies).
pub fn all_hidden(app: &SolveApp, parts: &[Part]) -> bool {
    !parts.is_empty()
        && parts.iter().all(|p| match &p.sel {
            Sel::Occurrence { path } => app.session.occurrence_hidden(path) || p.bodies.iter().all(|b| !app.session.body_visible(b)),
            _ => p.bodies.iter().all(|b| !app.session.body_visible(b)),
        })
}

/// Show `parts` again: placements hidden as placements as placements, and bodies hidden one by
/// one (a placement's too, as [`hide`] hides them when bodies and placements are mixed) one by
/// one. Each of the two is one step that undo takes back.
pub fn show(app: &mut SolveApp, parts: &[Part]) -> Result<(), String> {
    let paths: Vec<&Vec<u64>> = parts
        .iter()
        .filter_map(|p| if let Sel::Occurrence { path } = &p.sel { Some(path) } else { None })
        .filter(|path| app.session.occurrence_hidden(path))
        .collect();
    let mut result = Ok(());
    if !paths.is_empty() {
        result = app.run("browser.visibility", serde_json::json!({ "paths": paths, "visible": true })).map(|_| ());
    }
    // A placement's bodies only where they are hidden themselves: an assembly's part can hold
    // thousands, and showing the rest would be a step that changes nothing.
    let hidden: std::collections::HashSet<&str> = app.session.visibility.hidden_bodies.iter().map(String::as_str).collect();
    let hidden = &hidden;
    let bodies: Vec<String> = parts
        .iter()
        .flat_map(|p| {
            let placed = matches!(p.sel, Sel::Occurrence { .. });
            p.bodies.iter().filter(move |b| !placed || hidden.contains(b.as_str())).cloned()
        })
        .collect();
    if !bodies.is_empty() {
        result = result.and(app.run("browser.visibility", serde_json::json!({ "items": bodies, "visible": true })).map(|_| ()));
    }
    result
}

/// Hide `parts` in one step that undo takes back: placements as placements (their Browser rows
/// show them off), bodies of the design's own component one by one.
pub fn hide(app: &mut SolveApp, parts: &[Part]) -> Result<(), String> {
    let paths: Vec<&Vec<u64>> = parts.iter().filter_map(|p| if let Sel::Occurrence { path } = &p.sel { Some(path) } else { None }).collect();
    let params = if !paths.is_empty() && paths.len() == parts.len() {
        serde_json::json!({ "paths": paths, "visible": false })
    } else {
        serde_json::json!({ "items": bodies_of(parts), "visible": false })
    };
    app.run("browser.visibility", params).map(|_| ())
}

/// Show only `parts`, in one step that undo takes back: everything else is hidden, other
/// placements as placements (as the desktop's Isolate does).
pub fn isolate(app: &mut SolveApp, parts: &[Part]) -> Result<(), String> {
    let items = bodies_of(parts);
    if items.is_empty() {
        return Err("nothing to isolate".into());
    }
    app.run("browser.visibility", serde_json::json!({ "items": items, "visible": true, "isolate": true })).map(|_| ())
}

/// Show everything again (folders and placements too), in one step that undo takes back.
pub fn show_all(app: &mut SolveApp) -> Result<(), String> {
    app.run("browser.visibility", serde_json::json!({ "folder": "all", "visible": true })).map(|_| ())
}

/// Explode the assembly apart, or put it back together when it is shown apart: the design's first
/// exploded view, or without one every top-level part moved away from the middle (nothing is
/// added to the design either way). Whether it is apart now.
///
/// Refused, changing nothing, while parts are moved and their positions not captured: showing or
/// leaving an exploded view puts every part where the view or the design has it, and those moves
/// would be lost without an undo step to take them back.
pub fn explode(app: &mut SolveApp) -> Result<bool, String> {
    if app.session.moved_uncaptured() {
        return Err(solidcraft_ui_text::tr("vr.explode_moved").to_string());
    }
    if !app.session.exploded.is_empty() {
        return app.run("explode.show", serde_json::json!({})).map(|_| false);
    }
    let params = match app.session.doc.assembly.exploded.first() {
        Some(view) => serde_json::json!({ "view": view.id }),
        None => serde_json::json!({ "spread": 1.0 }),
    };
    app.run("explode.show", params).map(|_| true)
}

/// Every body of `parts`, each once, in order.
pub fn bodies_of<'a>(parts: impl IntoIterator<Item = &'a Part>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    parts.into_iter().flat_map(|p| &p.bodies).filter(|b| seen.insert(b.as_str())).cloned().collect()
}

/// Light `bodies` in the design as pointed at, in place of the ones `lit` holds (what this
/// caller lit last time); other hovers (the Browser's, the timeline's) are left as they are.
/// `lit` then holds what is lit now.
pub fn light(app: &mut SolveApp, lit: &mut Vec<String>, bodies: Vec<String>) {
    if *lit == bodies {
        return;
    }
    // One entry taken out for each this lit (another hover may light the same body too), in one
    // pass: an assembly's part can hold thousands of bodies.
    let mut left: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for body in lit.drain(..) {
        *left.entry(body).or_default() += 1;
    }
    app.viewport.hover_bodies.retain(|b| match left.get_mut(b) {
        Some(n) if *n > 0 => {
            *n -= 1;
            false
        }
        _ => true,
    });
    app.viewport.hover_bodies.extend(bodies.iter().cloned());
    *lit = bodies;
}

/// How a joint's glyph looks in VR: its type, the window's glyph style, and whether it is picked,
/// suppressed or limited. Kept with the glyph's picture, which is painted again when it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointBadge {
    pub(crate) kind: solidcraft_engine::doc::joints::JointKind,
    pub(crate) style: u8,
    pub(crate) picked: bool,
    pub(crate) suppressed: bool,
    pub(crate) limit: bool,
}

/// A joint's glyph to draw in the room: the joint, where its origin is (model units), and how it
/// looks.
#[derive(Clone, Debug, PartialEq)]
pub struct JointMark {
    pub id: u64,
    pub at: [f64; 3],
    pub badge: JointBadge,
}

/// The joints whose glyphs the window draws now (their eyes and the Joints folder, or picked),
/// as VR draws them: a badge at each joint's origin.
pub fn joint_marks(app: &SolveApp) -> Vec<JointMark> {
    marks_of(app, crate::dialogs_assembly::drawn_joints(app))
}

/// The marks of the joints `drawn` (each with whether it is picked), in that order: each joint
/// and its frame found by its id in one pass over the design's joints, not a search of them all
/// for each.
fn marks_of(app: &SolveApp, drawn: Vec<(u64, bool)>) -> Vec<JointMark> {
    if drawn.is_empty() {
        return Vec::new();
    }
    let frames: std::collections::HashMap<u64, solidcraft_engine::doc::Mat> =
        crate::dialogs_assembly::joint_frames_world(&app.session).into_iter().collect();
    let joints: std::collections::HashMap<u64, &solidcraft_engine::doc::joints::Joint> =
        app.session.doc.assembly.joints.iter().map(|j| (j.id, j)).collect();
    let style = app.preferences.joint_symbols;
    drawn
        .into_iter()
        .filter_map(|(id, picked)| {
            let j = joints.get(&id)?;
            let m = frames.get(&id)?;
            let at = solidcraft_engine::doc::apply_point(m, Vec3::new(0.0, 0.0, 0.0));
            let badge = JointBadge { kind: j.kind, style, picked, suppressed: j.suppressed, limit: crate::joint_symbols::has_limit(j) };
            Some(JointMark { id, at: [at.x, at.y, at.z], badge })
        })
        .collect()
}

/// Everything the joints' marks are worked out from: the design (`Session::uid`) and its state
/// (`revision`: the joints, their types, suppression and limits), the window's glyph style, which
/// joints are drawn and picked (their eyes, the Joints folder, the Browser's picks), and the
/// moves not captured (they move the joints' origins without a new revision).
#[derive(Clone, Debug, PartialEq)]
struct MarksKey {
    design: u64,
    revision: u64,
    style: u8,
    drawn: Vec<(u64, bool)>,
    moves: std::collections::BTreeMap<u64, solidcraft_engine::doc::Mat>,
}

/// The joints' marks, kept between frames while nothing they are worked out from changes
/// ([`MarksKey`]): working them out walks every joint of the design.
#[derive(Default)]
pub struct JointMarks {
    key: Option<MarksKey>,
    marks: Vec<JointMark>,
}

impl JointMarks {
    /// The marks of `app`'s joints now ([`joint_marks`]), worked out again only when what they
    /// are worked out from changed.
    pub fn get(&mut self, app: &SolveApp) -> &[JointMark] {
        let drawn = crate::dialogs_assembly::drawn_joints(app);
        let s = &app.session;
        let same = self.key.as_ref().is_some_and(|k| {
            k.design == s.uid
                && k.revision == s.revision
                && k.style == app.preferences.joint_symbols
                && k.drawn == drawn
                && k.moves == s.pending_moves
        });
        if !same {
            let key = MarksKey {
                design: s.uid,
                revision: s.revision,
                style: app.preferences.joint_symbols,
                drawn: drawn.clone(),
                moves: s.pending_moves.clone(),
            };
            self.marks = marks_of(app, drawn);
            self.key = Some(key);
        }
        &self.marks
    }
}

/// `badge` painted to fill `rect`, as the window draws it (the ring-and-axis style, which has no
/// badge, as its plain badge).
pub fn paint_joint(painter: &egui::Painter, rect: egui::Rect, badge: JointBadge) {
    use crate::joint_symbols::Mood;
    let t = solidcraft_theme::Tokens::get();
    let style = if crate::joint_symbols::badge_reach(badge.style).is_some() { badge.style } else { 1 };
    let mood = if badge.picked {
        Mood::Picked
    } else if badge.suppressed {
        Mood::Suppressed
    } else {
        Mood::Bright
    };
    let look = crate::joint_symbols::badge(style, badge.kind, mood, t.sel_edge, &t);
    let mut shapes = crate::joint_symbols::badge_shapes(egui::Pos2::ZERO, egui::vec2(1.0, 0.0), badge.kind, &look, badge.limit, false, &t);
    // The badge with its rings and halo inside the picture.
    let reach = look.radius + 6.0;
    let scale = rect.width().min(rect.height()) * 0.5 / reach;
    let place = egui::emath::TSTransform::new(rect.center().to_vec2(), scale);
    for shape in &mut shapes {
        shape.transform(place);
    }
    painter.extend(shapes);
}
