//! Frame Member (`frame.member`): structural members along the lines of a frame sketch and between
//! pairs of points. Lines and points are picked in the view into the path, where consecutive points
//! pair up in pick order; the section is a built-in profile with its dimensions, or the profiles
//! picked in another sketch. Anchor, angle and mirror place the section on each member.

use serde_json::{Map, Value, json};
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{self, MemberProfile};
use solidcraft_engine::doc::{FeatureKind, ProfileSel};
use solidcraft_engine::sketch::CurveKind;
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, check, curves_sketch, form_row, profile_indices, put, shown_expr};
use crate::selection::{CONSTRUCTION_POINTS, ORIGIN, POINTS, PROFILES, SKETCH_LINES, SNAP_POINTS, SelInput, SelectionExt as _, VERTICES, is_point};
use crate::ui_kit::{choice_row, expr_row, hint_row, text_row};
use crate::viewport::Hit;

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
    /// Ghost Frame Members: every frame member and End Cap body is drawn see-through while the
    /// dialog is open, its faces and edges left out of picks, so the points behind and its own
    /// vertices and centreline ends can be picked. Unticked whenever the dialog opens.
    pub ghost: bool,
    /// The tubes clicked in the view (their centrelines' ids, [`Centreline::id`]), for Remove Tube.
    pub picked_tubes: Vec<String>,
}

/// A member of the Frame Member the dialog previews, as its live preview made it: its id (a
/// sketch line's, or a pair's as the preview numbered it), which of the path's pairs it is (none
/// for a line), the frame sketch of a line, and its centreline's ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Centreline {
    pub id: String,
    pub pair: Option<usize>,
    pub sketch: Option<u64>,
    pub a: solidcore_record::Vec3,
    pub b: solidcore_record::Vec3,
}

/// How near the pointer a centreline lights up and takes a click, in points.
const TUBE_PX: f32 = 8.0;

/// The centrelines of Frame Member feature `id` among a preview's features, in member order:
/// its lines', then its pairs', as the document's own segments place them.
pub(crate) fn centrelines(p: &solidcraft_engine::Preview, id: u64) -> Vec<Centreline> {
    let Some(f) = p.doc.feature(id) else { return Vec::new() };
    let FeatureKind::FrameMember { path_sketch, pairs, .. } = &f.kind else { return Vec::new() };
    let Ok(segments) = frame::member_segments(&p.doc, &p.model.state_before(id), f) else { return Vec::new() };
    // The pairs' members come last, in the pairs' order, which is the order of the path's pairs.
    let first_pair = segments.len().saturating_sub(pairs.len());
    segments
        .into_iter()
        .enumerate()
        .map(|(k, (id, a, b))| {
            let pair = k.checked_sub(first_pair);
            Centreline { id, pair, sketch: path_sketch.filter(|_| pair.is_none()).map(|s| s.0), a, b }
        })
        .collect()
}

/// The centrelines shown and picked: the open Frame Member dialog's, while its preview is up to
/// date with the path (a preview still computing may be of another path).
fn tubes(app: &SolveApp) -> &[Centreline] {
    let open = app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Frame(_)));
    if open && !app.preview.busy { &app.preview.centrelines } else { &[] }
}

/// What the view's picks take from the dialog, for its pick cache: whether Ghost Frame Members
/// is ticked and the centrelines shown.
pub(crate) fn hash_picks(app: &SolveApp, h: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    matches!(app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Frame(m)) if m.ghost).hash(h);
    for c in tubes(app) {
        c.id.hash(h);
        [c.a.x, c.a.y, c.a.z, c.b.x, c.b.y, c.b.z].map(f64::to_bits).hash(h);
    }
}

/// The nearest centreline within [`TUBE_PX`] of `pos`.
pub(crate) fn tube_near(app: &SolveApp, proj: &crate::viewport::Proj, pos: egui::Pos2) -> Option<Hit> {
    tubes(app)
        .iter()
        .filter_map(|c| Some((crate::viewport::seg_dist(pos, proj.to_screen(c.a)?, proj.to_screen(c.b)?), c)))
        .filter(|(d, _)| *d < TUBE_PX)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, c)| Hit::Centreline { line: c.id.clone() })
}

/// The centreline under the cursor, unless a point the click would pick is there too: a point
/// wins (a pair's end is the point it was picked on).
pub(crate) fn tube_hover(app: &SolveApp, hits: &[Hit]) -> Option<Hit> {
    let tube = hits.iter().find(|h| matches!(h, Hit::Centreline { .. }))?;
    let point = crate::viewport::candidate(app, hits).is_some_and(|(h, _)| {
        matches!(
            h,
            Hit::SketchPoint { .. }
                | Hit::SketchPoint3d { .. }
                | Hit::SketchMidpoint { .. }
                | Hit::Vertex { .. }
                | Hit::ConstructionPoint { .. }
                | Hit::MemberEnd { .. }
        )
    });
    (!point).then(|| tube.clone())
}

fn frame_member(app: &mut SolveApp) -> Option<&mut Member> {
    match app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(Kind::Frame(m)) => Some(m),
        _ => None,
    }
}

/// A click on a centreline: its tube goes into the picked tubes, or out again.
pub(crate) fn click_tube(app: &mut SolveApp, hit: &Hit) -> bool {
    let Hit::Centreline { line } = hit else { return false };
    let Some(m) = frame_member(app) else { return false };
    match m.picked_tubes.iter().position(|t| t == line) {
        Some(i) => drop(m.picked_tubes.remove(i)),
        None => m.picked_tubes.push(line.clone()),
    }
    true
}

/// A right-click on a centreline: its tube is picked, unless it already is, and the Remove Tube
/// menu opens. False when the click is not on one.
pub(crate) fn open_menu(app: &mut SolveApp, at: egui::Pos2) -> bool {
    let Some(Hit::Centreline { line }) = app.viewport.hover.clone() else { return false };
    let Some(m) = frame_member(app) else { return false };
    if !m.picked_tubes.contains(&line) {
        m.picked_tubes = vec![line];
    }
    crate::context_menu::open_for(app, at, crate::context_menu::Target::FrameTubes);
    true
}

/// The Remove Tube menu: one item for the picked tubes.
pub(crate) fn menu_items(app: &SolveApp) -> Vec<crate::ui_kit::Item> {
    let n = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Frame(m)) => m.picked_tubes.len(),
        _ => return Vec::new(),
    };
    vec![crate::context_menu::act("ui.removeTubes", &remove_tubes_label(n), "delete").key("Del").on(n > 0)]
}

/// "Remove Tube", or "Remove N Tubes" for `n` of them.
pub(crate) fn remove_tubes_label(n: usize) -> String {
    let key = if n == 1 { "frame.remove_tubes_one" } else { "frame.remove_tubes_other" };
    solidcraft_ui_text::format(key, &[("count", &n.to_string())])
}

/// Remove Tube on the picked tubes (the menu's item, the Delete key). False with none picked.
pub(crate) fn remove_picked(app: &mut SolveApp) -> bool {
    let Some(picked) = frame_member(app).map(|m| std::mem::take(&mut m.picked_tubes)).filter(|p| !p.is_empty()) else { return false };
    remove_tubes(app, &picked);
    true
}

/// Take tubes `ids` out of the path: a pair's two entries (only this pair's, so a point another
/// pair shares stays), a line's pick. Each other entry keeps its reference and pair id. With All
/// Lines, the lines kept are listed from then on, so the one taken out stays out.
pub(crate) fn remove_tubes(app: &mut SolveApp, ids: &[String]) {
    // The last finished preview's, busy or not: tubes are picked again whenever the path
    // changes, so a preview still computing is of this same path.
    let all = app.preview.centrelines.clone();
    let Some(d) = app.dialog.as_mut() else { return };
    let (Some(path), Kind::Frame(m)) = (d.inputs.first_mut(), &mut d.kind) else { return };
    m.sync(&path.items);
    let Ok((lines, pairs, _)) = pair_indices(&path.items) else { return };
    let mut gone = std::collections::BTreeSet::new();
    for t in all.iter().filter(|t| ids.contains(&t.id)) {
        match t.pair {
            Some(k) => gone.extend(pairs.get(k).into_iter().flat_map(|(a, b)| [*a, *b])),
            None => gone.extend(lines.iter().filter(|i| line_id(path.items.get(**i)) == Some(t.id.as_str()))),
        }
    }
    // The entries and what is kept for each go together, so each keeps its own.
    drop_at(&mut path.items, &gone);
    drop_at(&mut m.path, &gone);
    drop_at(&mut m.refs, &gone);
    drop_at(&mut m.pair_ids, &gone);
    if m.all && all.iter().any(|t| t.pair.is_none() && ids.contains(&t.id)) {
        m.all = false;
        let missing: Vec<Sel> = all
            .iter()
            .filter(|t| t.pair.is_none() && !ids.contains(&t.id))
            .filter(|t| !path.items.iter().any(|s| line_id(Some(s)) == Some(t.id.as_str())))
            .filter_map(|t| Some(Sel::ExternalSketchCurve { sketch: t.sketch?, id: t.id.clone() }))
            .collect();
        path.items.splice(0..0, missing);
    }
    m.picked_tubes.clear();
    prompt(d);
}

/// A sketch line's id, when `item` is one.
fn line_id(item: Option<&Sel>) -> Option<&str> {
    match item? {
        Sel::SketchCurve { id, .. } | Sel::ExternalSketchCurve { id, .. } => Some(id),
        _ => None,
    }
}

/// `v` without the entries at `gone`.
fn drop_at<T>(v: &mut Vec<T>, gone: &std::collections::BTreeSet<usize>) {
    let mut i = 0;
    v.retain(|_| {
        i += 1;
        !gone.contains(&(i - 1))
    });
}

/// The previewed members' centrelines, lit when hovered and when picked for Remove Tube.
pub(crate) fn draw_tubes(app: &SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    let t = solidcraft_theme::Tokens::get();
    let picked = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Frame(m)) => &m.picked_tubes[..],
        _ => &[],
    };
    for c in tubes(app) {
        let (Some(a), Some(b)) = (proj.to_screen(c.a), proj.to_screen(c.b)) else { continue };
        let hovered = matches!(&app.viewport.hover, Some(Hit::Centreline { line }) if *line == c.id);
        let stroke = if hovered {
            egui::Stroke::new(3.0, t.hover_edge)
        } else if picked.contains(&c.id) {
            egui::Stroke::new(2.5, t.sel_edge)
        } else {
            egui::Stroke::new(1.5, t.construction_geom)
        };
        painter.line_segment([a, b], stroke);
    }
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
            ghost: false,
            picked_tubes: Vec::new(),
        }
    }

    /// Realign `refs` and `pair_ids` to the path's `items` after they changed: each point keeps
    /// what it was loaded with, matched in order (a point taken out leaves the others theirs).
    pub fn sync(&mut self, items: &[Sel]) {
        if self.path != items {
            (self.refs, self.pair_ids) = self.aligned(items);
            self.path = items.to_vec();
            // Tubes picked for another path may now be other members.
            self.picked_tubes.clear();
        }
    }

    /// Take entry `at` out of the path `items` with what is kept for it, so each other entry
    /// keeps its own reference and pair id (matching by value would give a shared point's
    /// remaining occurrence the wrong pair's).
    pub fn remove_at(&mut self, items: &mut Vec<Sel>, at: usize) {
        self.sync(items);
        let gone = std::collections::BTreeSet::from([at]);
        drop_at(items, &gone);
        drop_at(&mut self.path, &gone);
        drop_at(&mut self.refs, &gone);
        drop_at(&mut self.pair_ids, &gone);
        self.picked_tubes.clear();
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

/// A pick of `sel` as [`pick_hit`] takes it, from no particular hit.
#[cfg(test)]
pub(crate) fn pick(app: &SolveApp, d: &mut Dialog, sel: Sel) {
    pick_as(app, d, sel, None);
}

/// [`pick_hit`], a new point taking `given` as its lasting reference when there is one.
fn pick_as(app: &SolveApp, d: &mut Dialog, sel: Sel, mut given: Option<Value>) {
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
                *r = given.take().or_else(|| point_ref(app, item).ok());
            }
        }
    }
}

/// A pick of `sel`, clicked on `hit`, for the dialog's active input. A pick into Frame Member's
/// path is tried as the live preview builds it: one that would make the feature fail is taken
/// back, and the dialog says why. A ghosted member's centreline end goes in as the point that end
/// is built on ([`end_pick`]), its reference copied rather than found again.
pub(crate) fn pick_hit(app: &SolveApp, d: &mut Dialog, hit: &Hit, sel: Sel) {
    pick_as(app, d, sel, end_pick(&app.session, hit).map(|(_, r)| r));
}

/// The centreline ends of the members Ghost Frame Members draws see-through, as picks offer them:
/// both ends of each shown member of every evaluated Frame Member but the one being edited (whose
/// members are the preview). None while the dialog isn't open with the box ticked.
pub(crate) fn member_ends(app: &SolveApp) -> Vec<Hit> {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(&d.kind, Kind::Frame(m) if m.ghost)) else { return Vec::new() };
    let s = &app.session;
    let editing = d.editing.map(|(id, _)| id);
    let mut out = Vec::new();
    // ponytail: worked out again each call (each pick and frame while the dialog is open); cache
    // by document and state if a large frame makes that show.
    for f in s.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::FrameMember { .. }) && Some(f.id) != editing) {
        if !s.model.result(f.id).is_some_and(|r| !r.skipped && r.error.is_none()) {
            continue;
        }
        let Ok(segments) = frame::member_segments(&s.doc, &s.model.state_before(f.id), f) else { continue };
        let FeatureKind::FrameMember { names, .. } = &f.kind else { continue };
        for (k, (line, a, b)) in segments.into_iter().enumerate() {
            // A hidden member (its body, component or occurrence) offers no end to pick: the
            // markers are drawn over everything and would take a click meant for what is shown.
            // Its body: by position for a copy, else by its member id, as `end_cap::member` finds it.
            let body = if f.body_names.is_empty() { names.get(&line) } else { f.body_names.get(k) };
            if !body.is_some_and(|b| app.body_visible(b)) {
                continue;
            }
            out.push(Hit::MemberEnd { feature: f.id, line: line.clone(), end: 0, point: a });
            out.push(Hit::MemberEnd { feature: f.id, line, end: 1, point: b });
        }
    }
    out
}

/// What a centreline end is built on, as the path picks it and the reference it keeps: for a
/// sketch line's member, the line's end point (a 3D line's end node); for a pair's, a copy of
/// that pair's first or second point.
pub(crate) fn end_pick(s: &Session, hit: &Hit) -> Option<(Sel, Value)> {
    let Hit::MemberEnd { feature, line, end, .. } = hit else { return None };
    let FeatureKind::FrameMember { path_sketch, pairs, .. } = &s.doc.feature(*feature)?.kind else { return None };
    if let Some(p) = pairs.iter().find(|p| p.id == *line) {
        let g = if *end == 0 { &p.a } else { &p.b };
        return Some((crate::dialogs::point_pick(s, g), serde_json::to_value(g).ok()?));
    }
    let sketch = path_sketch.as_ref()?.0;
    let sk = s.doc.sketch(sketch).ok()?;
    let point = match sk.wire_index(line).and_then(|i| sk.wires.get(i)) {
        Some(w) => format!("{}:{}", w.id, if *end == 0 { 0 } else { w.fit.len().checked_sub(1)? }),
        None => {
            let CurveKind::Line { a, b } = sk.curves.get(sk.curve_index(line)?)?.kind else { return None };
            sk.points.get(if *end == 0 { a } else { b })?.id.clone()
        }
    };
    Some((Sel::SketchPoint { id: format!("{sketch}:{point}") }, json!({"sketch": sketch, "point": point})))
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
pub(crate) fn prompt(d: &mut Dialog) {
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
    // ponytail: one Frame Member evaluation per completing pick, on the UI thread; check the
    // async preview's result and take the pick back then if a large frame makes clicks lag.
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
    form_row(ui, "Ghost Frame Members", |ui| check(ui, &mut m.ghost));
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
