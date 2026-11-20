//! Frame Member (`frame.member`): structural members along the lines of a frame sketch and between
//! pairs of points. Lines and points are picked in the view into the path, where consecutive points
//! pair up in pick order; the section is a built-in profile with its dimensions, or the profiles
//! picked in another sketch. Anchor, angle and mirror place the section on each member.

use serde_json::{Map, Value, json};
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{self, CornerType, MemberProfile};
use solidcraft_engine::doc::frame_corners::CORNER_TOL;
use solidcraft_engine::doc::{FeatureKind, ProfileSel};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::CurveKind;
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, check, curves_sketch, form_row, input_rows_range, profile_indices, put, shown_expr};
use crate::selection::{
    CONSTRUCTION_POINTS, EXTERNAL_POINTS, ORIGIN, POINTS, PROFILES, SKETCH_LINES, SNAP_POINTS, SelInput, SelectionExt as _, VERTICES, is_point,
};
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

/// The corner types as `frame.member` names them, and their labels.
pub(crate) const CORNER_TYPES: [(&str, &str); 4] = [("auto", "Auto"), ("none", "None"), ("mitre", "Mitre"), ("butt", "Butt")];
/// [`CORNER_TYPES`]' Auto and None.
const AUTO: usize = 0;
const NONE: usize = 1;

/// Whether the frame's Flip means anything for the corner type `index`: which member runs through.
/// Not under Auto, which has no one corner to act on.
fn flips(index: usize) -> bool {
    matches!(CORNER_TYPES.get(index), Some(("butt", _)))
}

/// Whether a frame of type `frame` cuts tubes where they cross (every type but None, #4003): a
/// crossing is then a corner an override may name, by Auto's rules.
fn splits(frame: usize) -> bool {
    frame != NONE
}

/// Whether an override row's Flip means anything for its type `index`: Auto's flip swaps the two
/// tubes' roles at its corner.
fn override_flips(index: usize) -> bool {
    index == AUTO || flips(index)
}

/// The types an override row offers on a frame of type `frame`: Auto only on an Auto frame (on any
/// other an Auto override builds as the frame's type, spec A6).
pub(crate) fn override_choices(frame: usize) -> &'static [(&'static str, &'static str)] {
    if frame == AUTO { &CORNER_TYPES } else { CORNER_TYPES.get(NONE..).unwrap_or_default() }
}

/// What an override of type `corner` builds as on a frame of type `frame` (spec A6).
fn override_type(corner: usize, frame: usize) -> usize {
    if corner == AUTO && frame != AUTO { frame } else { corner }
}

fn corner_name(index: usize) -> &'static str {
    CORNER_TYPES.get(index).map_or("none", |c| c.0)
}

fn corner_index(corner: &CornerType) -> usize {
    let name = serde_json::to_value(corner).ok();
    CORNER_TYPES.iter().position(|c| name.as_ref().and_then(Value::as_str) == Some(c.0)).unwrap_or(NONE)
}

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
    /// How the corners are finished (an index into [`CORNER_TYPES`]), and for Butt whether the
    /// other member runs through.
    pub corner: usize,
    pub flip: bool,
    /// Fit Ends to Earlier Frames: fit this Frame Member's free ends to earlier Frame Members' tubes
    /// (under every corner type but None).
    pub fit_ends: bool,
    /// Coped (#17854): every butt joint of the frame cut to the other tube's real shape (under every type but None).
    pub coped: bool,
    /// The weld gap at every welded end (#17854; under every type but None): empty or 0 sends none.
    pub gap: String,
    /// Each corner picked in Corner Overrides with its own settings.
    pub overrides: Vec<Override>,
    /// The feature's `next_pair` (0 for a new one): the number its next new pair's id takes, at the least.
    pub next_pair: u32,
    /// Whether the Corner Overrides rows show (the arrow on its line folds them; a new pick
    /// opens them). For this dialog only: not saved.
    pub overrides_open: bool,
    /// The last pick that completed a tube, until the live preview of the path it made is in ([`settle_pick`]).
    pub pending: Option<Box<Pending>>,
}

/// A corner picked into Corner Overrides, with its own settings.
#[derive(Clone, Debug)]
pub struct Override {
    pub pick: Sel,
    /// Its type (an index into [`CORNER_TYPES`]).
    pub corner: usize,
    pub flip: bool,
    /// Its own weld gap; empty: the frame's.
    pub gap: String,
    /// An edit's: the members in their saved order, and the Flip the row showed for it. While the row's Flip is
    /// unchanged they go back in that order, so OK leaves an order today's rules wouldn't give as it was.
    pub saved: Option<(Vec<String>, bool)>,
}

impl Override {
    /// A new pick: the frame's type `corner`, unflipped, the frame's gap.
    fn new(pick: Sel, corner: usize) -> Override {
        Override { pick, corner, flip: false, gap: String::new(), saved: None }
    }
}

/// A pick into the path that completed a tube onto a path that built: taken back if the live
/// preview of the path it made fails ([`settle_pick`]).
#[derive(Clone, Debug)]
pub struct Pending {
    /// The path the pick made, and the commands it sent: a preview of anything else (the path or
    /// a value changed since) is not the pick's.
    after: Vec<Sel>,
    cmds: Vec<(String, Value)>,
    /// What taking it back restores ([`take_back`]).
    back: Back,
}

/// The path's items before a pick, and the member's `path`, `refs` and `pair_ids` with them.
type Back = (Vec<Sel>, Vec<Sel>, Vec<Option<Value>>, Vec<Option<String>>);

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

/// The centrelines of the Frame Members a preview's commands made or changed (the features its
/// replies name).
pub(crate) fn centrelines(p: &solidcraft_engine::Preview) -> Vec<Centreline> {
    p.replies.iter().filter_map(|r| r.get("feature").and_then(Value::as_u64)).flat_map(|id| feature_centrelines(p, id)).collect()
}

/// The centrelines of Frame Member feature `id` among a preview's features, in member order:
/// its lines', then its pairs', as the document's own segments place them.
fn feature_centrelines(p: &solidcraft_engine::Preview, id: u64) -> Vec<Centreline> {
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

/// The centreline under the cursor, unless the click would pick something else there (a point,
/// a pair's end, the sketch line a line member lies on): that wins, as it did before tubes could
/// be picked.
pub(crate) fn tube_hover(app: &SolveApp, hits: &[Hit]) -> Option<Hit> {
    let tube = hits.iter().find(|h| matches!(h, Hit::Centreline { .. }))?;
    // Only where a click would pick nothing else: a line or point under it is picked as before.
    crate::viewport::candidate(app, hits).is_none().then(|| tube.clone())
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
    // With Right-click confirms on, a right-click confirms the input there as before.
    if app.preferences.right_click_confirms {
        return false;
    }
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
    // While OK computes, the path holds.
    if app.applying.is_some() {
        return;
    }
    // The last finished preview's, busy or not: tubes are picked again whenever the path
    // changes, so a preview still computing is of this same path.
    let all = app.preview.centrelines.clone();
    // A corner override naming a tube taken out (its line or pair id) goes with it, as
    // `frame.remove_member` drops it: a kept one, and a picked corner whose members, as OK would
    // send them now, name it.
    let removed: std::collections::BTreeSet<&str> = all.iter().filter(|t| ids.contains(&t.id)).map(|t| t.id.as_str()).collect();
    let names_removed = |o: &Value| read_override(o).is_some_and(|o| o.lines().any(|m| removed.contains(m)));
    let mut gone_corners = Vec::new();
    if let Some(d) = app.dialog.as_ref()
        && let Kind::Frame(m) = &d.kind
        && let Ok(cmds) = commands(app, m, &d.inputs, &d.extra)
        && let Some(sent) = cmds.first().and_then(|c| c.1.get("corners")).and_then(Value::as_array)
    {
        // The picked corners come first, in order, each a frame sketch point.
        let points = crate::selection::items(&d.inputs, 2).iter().filter(|p| matches!(p, Sel::ExternalSketchPoint { .. }));
        gone_corners = points.zip(sent).filter(|(_, o)| names_removed(o)).map(|(p, _)| p.clone()).collect();
    }
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
    if let Some(corners) = d.inputs.get_mut(2) {
        corners.items.retain(|p| !gone_corners.contains(p));
    }
    if let Some(Value::Array(kept)) = d.extra.get_mut("corners") {
        kept.retain(|o| !names_removed(o));
        if kept.is_empty() {
            d.extra.remove("corners");
        }
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
            corner: NONE,
            flip: false,
            fit_ends: true,
            coped: false,
            gap: String::new(),
            overrides: Vec::new(),
            next_pair: 0,
            overrides_open: true,
            pending: None,
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
        self.remove(items, &std::collections::BTreeSet::from([at]));
    }

    /// Take the Path list's row `row` ([`path_rows`]) out of the path `items`: a pair's row takes
    /// both its points.
    pub fn remove_row(&mut self, items: &mut Vec<Sel>, row: usize) {
        let gone = path_rows(items).into_iter().nth(row).unwrap_or_default().into_iter().collect();
        self.remove(items, &gone);
    }

    /// Take the entries at `gone` out of the path `items`, with what is kept for each.
    fn remove(&mut self, items: &mut Vec<Sel>, gone: &std::collections::BTreeSet<usize>) {
        self.sync(items);
        drop_at(items, gone);
        drop_at(&mut self.path, gone);
        drop_at(&mut self.refs, gone);
        drop_at(&mut self.pair_ids, gone);
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
const PICK_SECOND: &str = "Pick the second point";

/// The dialog for Frame Member.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.member").then(|| {
        let inputs = vec![
            SelInput::new("Path", PATH, true).ordered_points(),
            SelInput::new("Section (optional)", PROFILES, true),
            // The frame sketch isn't being edited: the view offers its points only to an input
            // that takes other sketches' points alone (`SKETCH_POINTS` would offer the active
            // sketch's instead).
            SelInput::new("Corner Overrides", EXTERNAL_POINTS, true),
        ];
        (Kind::Frame(Member::new()), inputs)
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

/// The Path list's rows (Goal 6), in path order, each the path entries it stands for: a line one
/// row, a pair of points one row, and a point waiting for its partner none. A path the pairing
/// refuses lists each entry alone.
pub(crate) fn path_rows(items: &[Sel]) -> Vec<Vec<usize>> {
    let Ok((lines, pairs, _)) = pair_indices(items) else { return (0..items.len()).map(|i| vec![i]).collect() };
    let mut rows: Vec<Vec<usize>> = lines.into_iter().map(|i| vec![i]).chain(pairs.into_iter().map(|(a, b)| vec![a, b])).collect();
    rows.sort_by_key(|r| r.first().copied());
    rows
}

/// [`path_rows`] when input `i` of `d` is Frame Member's Path: its list's rows, which its chip counts.
pub(crate) fn listed_rows(d: &Dialog, i: usize) -> Option<Vec<Vec<usize>>> {
    (matches!(d.kind, Kind::Frame(_)) && i == 0).then(|| path_rows(crate::selection::items(&d.inputs, 0)))
}

/// What each of [`listed_rows`] reads: a line as it is picked, a pair as its tube ("Tube n: a → b").
pub(crate) fn row_labels(s: &Session, names: crate::prefs::OriginPlaneNames, d: &Dialog, i: usize) -> Option<Vec<String>> {
    let rows = listed_rows(d, i)?;
    let inp = d.inputs.get(i)?;
    let label = |k: &usize| inp.items.get(*k).map(|x| crate::dialogs::selection_label(s, names, d, inp, x)).unwrap_or_default();
    let mut n = 0;
    let labels = rows.iter().map(|r| match r.as_slice() {
        [a, b] => {
            n += 1;
            solidcraft_ui_text::format("frame.path_pair_row", &[("n", &n.to_string()), ("a", &label(a)), ("b", &label(b))])
        }
        r => r.first().map(label).unwrap_or_default(),
    });
    Some(labels.collect())
}

/// A pick of `sel` as [`pick_hit`] takes it, from no particular hit.
#[cfg(test)]
pub(crate) fn pick(app: &SolveApp, d: &mut Dialog, sel: Sel) {
    pick_as(app, d, sel, None);
}

/// [`pick_hit`], a new point taking `given` as its lasting reference when there is one.
fn pick_as(app: &SolveApp, d: &mut Dialog, sel: Sel, given: Option<Value>) {
    let mut before = (matches!(d.kind, Kind::Frame(_)) && d.active == 0).then(|| crate::selection::items(&d.inputs, 0).to_vec());
    // What is kept for each entry goes back with the entries when the pick is refused.
    let back: Option<Back> = match (&before, &d.kind) {
        (Some(items), Kind::Frame(m)) => Some((items.clone(), m.path.clone(), m.refs.clone(), m.pair_ids.clone())),
        _ => None,
    };
    // A point waiting for its partner: clicked again it is taken out, and a line clicked instead
    // takes its place.
    if let Some(at) = before.as_deref().and_then(|b| pair_indices(b).ok()).and_then(|(_, _, waiting)| waiting) {
        let again = is_point(&sel) && before.as_ref().and_then(|b| b.get(at)) == Some(&sel);
        if again || line_id(Some(&sel)).is_some() {
            if let (Some(path), Kind::Frame(m)) = (d.inputs.first_mut(), &mut d.kind) {
                m.remove_at(&mut path.items, at);
            }
            d.error = None;
            prompt(d);
            if again {
                return;
            }
            before = Some(crate::selection::items(&d.inputs, 0).to_vec());
        }
    }
    let picks = match before.as_deref().and_then(|b| other_sketch_ends(app, b, &sel)) {
        Some(ends) => ends.map(|(end, r)| (end, Some(r))).to_vec(),
        None => vec![(sel, given)],
    };
    let mut given = Vec::new();
    for (sel, r) in picks {
        d.pick(&app.session, sel);
        given.push(r);
    }
    let mut given = given.into_iter().flatten();
    let (Some(before), Some(back)) = (before, back) else { return };
    let check = match refusal(app, d, &before) {
        Ok(check) => check,
        Err(why) => return take_back(d, back, &why),
    };
    prompt(d);
    // Each new point's reference is fixed as it is taken: a snap point is known only while it
    // stays among the woken ones (`snap_wake::snapped`), which a long session of picks outlives.
    let items = crate::selection::items(&d.inputs, 0);
    if let Kind::Frame(m) = &mut d.kind {
        for (i, item) in items.iter().enumerate().skip(before.len()).filter(|(_, item)| is_point(item)) {
            if let Some(r) = m.refs.get_mut(i).filter(|r| r.is_none()) {
                *r = given.next().or_else(|| point_ref(app, item).ok());
            }
        }
        if let Some(cmds) = check {
            m.pending = Some(Box::new(Pending { after: items.to_vec(), cmds, back }));
        }
    }
}

/// Take a pick back: the path and what is kept for its entries as they were before it, and the
/// dialog says why.
fn take_back(d: &mut Dialog, (items, path, refs, pair_ids): Back, why: &str) {
    if let Some(input) = d.inputs.first_mut() {
        input.items = items;
    }
    if let Kind::Frame(m) = &mut d.kind {
        (m.path, m.refs, m.pair_ids) = (path, refs, pair_ids);
    }
    d.error = Some(solidcraft_ui_text::format("frame.pick_refused", &[("reason", tr(why))]));
}

/// After the live preview is updated: the pick that completed a tube ([`refusal`]) is taken back
/// when the preview of the path it made is in and failed, as the dialog refuses a pick. A preview
/// of any other path, an older one still computing included, takes nothing back.
pub(crate) fn settle_pick(app: &mut SolveApp) {
    // While computing, the preview shows an older path; once done, the dialog's path now.
    if app.preview.busy {
        return;
    }
    let error = app.preview.error.clone();
    if !matches!(app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Frame(m)) if m.pending.is_some()) {
        return;
    }
    let now = app.dialog.as_ref().and_then(|d| crate::dialogs::apply_commands(app, d).ok());
    let Some(d) = app.dialog.as_mut() else { return };
    let Kind::Frame(m) = &mut d.kind else { return };
    let Some(pending) = m.pending.take() else { return };
    if let Some(why) = error
        && crate::selection::items(&d.inputs, 0) == pending.after.as_slice()
        && now.as_ref() == Some(&pending.cmds)
    {
        take_back(d, pending.back, &why);
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
        let before = s.model.state_before(f.id);
        let Ok(segments) = frame::member_segments(&s.doc, &before, f) else { continue };
        let FeatureKind::FrameMember { names, .. } = &f.kind else { continue };
        // A copy names its bodies by position among its members (under Auto, each line's pieces), as
        // `end_cap::member` finds them. When they don't pair up (a piece left with no body), they
        // aren't looked up: every end is offered.
        let made = s.model.result(f.id).map_or(0, |r| r.state.bodies.iter().filter(|b| b.feature == f.id).count());
        let members = frame::member_line_ids(&before, &f.kind).ok().filter(|ids| ids.len() == made);
        for (line, a, b) in segments {
            // A hidden member (its body, component or occurrence) offers no end to pick: the
            // markers are drawn over everything and would take a click meant for what is shown.
            // Its body: by its member id, or a copy's by position.
            let visible = |body: Option<&String>| body.is_some_and(|b| app.body_visible(b));
            let shown = if f.body_names.is_empty() {
                visible(names.get(&line))
            } else {
                members.as_ref().is_none_or(|ids| visible(ids.iter().position(|id| *id == line).and_then(|k| f.body_names.get(k))))
            };
            if !shown {
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
    let [a, b] = line_ends(s, path_sketch.as_ref()?.0, line, false)?;
    Some(if *end == 0 { a } else { b })
}

/// Line `line` of sketch `sketch`'s two end points, as the path picks them and the references
/// they keep: a sketch line's points, a 3D line's end nodes. With `straight`, only a straight
/// line's (a 3D spline or arc between its ends is not the member its two ends would make).
fn line_ends(s: &Session, sketch: u64, line: &str, straight: bool) -> Option<[(Sel, Value); 2]> {
    let sk = s.doc.sketch(sketch).ok()?;
    let [a, b] = match sk.wire_index(line).and_then(|i| sk.wires.get(i)) {
        Some(w) if straight && (w.fit.len() != 2 || w.arc.is_some()) => return None,
        Some(w) => [format!("{}:0", w.id), format!("{}:{}", w.id, w.fit.len().checked_sub(1)?)],
        None => {
            let CurveKind::Line { a, b } = sk.curves.get(sk.curve_index(line)?)?.kind else { return None };
            [sk.points.get(a)?.id.clone(), sk.points.get(b)?.id.clone()]
        }
    };
    let end = |point: String| (Sel::SketchPoint { id: format!("{sketch}:{point}") }, json!({"sketch": sketch, "point": point}));
    Some([end(a), end(b)])
}

/// A line clicked in another sketch than the path's lines: one Frame Member's lines are one
/// sketch's, so it goes in as its two end points, a pair, unless a point is waiting for its
/// partner (which a line can't be).
fn other_sketch_ends(app: &SolveApp, before: &[Sel], sel: &Sel) -> Option<[(Sel, Value); 2]> {
    let (Sel::ExternalSketchCurve { sketch, id } | Sel::SketchCurve { id, sketch: Some(sketch) }) = sel else { return None };
    let path_sketch = before.iter().find_map(|x| match x {
        Sel::ExternalSketchCurve { sketch, .. } | Sel::SketchCurve { sketch: Some(sketch), .. } => Some(*sketch),
        _ => None,
    })?;
    if path_sketch == *sketch || !pairs_of(before).is_ok_and(|(_, _, waiting)| waiting.is_none()) {
        return None;
    }
    line_ends(&app.session, *sketch, id, true)
}

/// Ctrl+click on a point of the path: its last occurrence goes. False when the click is not that.
pub(crate) fn take_out(app: &mut SolveApp, sel: &Sel) -> bool {
    // While OK computes, the path holds.
    if app.applying.is_some() {
        return false;
    }
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
        d.error = Some(PICK_SECOND.into());
    } else if d.error.as_deref() == Some(PICK_SECOND) {
        d.error = None;
    }
    let items = crate::selection::items(&d.inputs, 0);
    if let Kind::Frame(m) = &mut d.kind {
        m.sync(items);
    }
}

/// Why the path's newest picks (those after `before`) make the feature fail, where that shows
/// without building it; `Ok(Some(commands))` when they complete a tube onto a path that built, for
/// the live preview of those commands to check ([`settle_pick`]).
fn refusal(app: &SolveApp, d: &Dialog, before: &[Sel]) -> Result<Option<Vec<(String, Value)>>, String> {
    let items = crate::selection::items(&d.inputs, 0);
    if items.len() <= before.len() {
        return Ok(None);
    }
    let (lines, pairs, _) = pairs_of(items)?;
    let had = pairs_of(before).map(|(l, p, _)| (l.len(), p.len())).unwrap_or_default();
    // A point now waiting for its partner builds nothing new.
    if (lines.len(), pairs.len()) == had {
        return Ok(None);
    }
    // What fails already (a value, a point gone) is not this pick's doing: its own error shows.
    // Nor is a failure whose path before isn't known to build yet (its preview still computing).
    if had != (0, 0) {
        let mut was = d.clone();
        if let Some(path) = was.inputs.first_mut() {
            path.items = before.to_vec();
        }
        if app.preview.error.is_some() || crate::dialogs::apply_commands(app, &was).is_err() {
            return Ok(None);
        }
    }
    let cmds = crate::dialogs::apply_commands(app, d)?;
    Ok((had == (0, 0) || !app.preview.busy).then_some(cmds))
}

fn tr(text: &str) -> &str {
    solidcraft_ui_text::tr_label(text)
}

/// The dialog's own rows (after the Lines and Section inputs), then under a separator the corner
/// settings with Corner Overrides and a row for each overridden corner; true when Enter was
/// pressed in a value.
pub fn rows(app: &SolveApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let Kind::Frame(m) = &mut d.kind else { return false };
    let mut enter = member_rows(app, ui, m, &d.inputs);
    ui.separator();
    let labels = CORNER_TYPES.map(|c| c.1);
    choice_row(ui, "Corner Type", "frame_corner", &mut m.corner, &labels);
    if flips(m.corner) {
        form_row(ui, "Flip", |ui| check(ui, &mut m.flip));
    }
    // Fit Ends to Earlier Frames, and Coped to the right of its box; the weld Gap under them (a row of its own: beside
    // Coped the card has no room for it). None fits, copes and welds nothing: no Fit Ends, and Coped and Gap greyed.
    let none = m.corner == NONE;
    form_row(ui, if none { "" } else { "Fit Ends to Earlier Frames" }, |ui| {
        if !none {
            check(ui, &mut m.fit_ends);
        }
        ui.add_enabled_ui(!none, |ui| {
            let mut on = m.coped && !none;
            crate::ui_kit::checkbox(ui, tr("Coped"), &mut on);
            if !none {
                m.coped = on;
            }
        });
    });
    enter |= ui.add_enabled_ui(!none, |ui| hint_row(ui, "Gap", &mut m.gap, "0 mm")).inner;
    // The overridden corner under the pointer in the view lights its row, or the Corner Overrides
    // line while the rows are folded.
    let hovered = match &app.viewport.hover {
        Some(crate::viewport::Hit::SketchPoint { sketch, id, .. }) => Some(Sel::ExternalSketchPoint { sketch: *sketch, id: id.clone() }),
        _ => None,
    };
    let lit = hovered.and_then(|h| crate::selection::items(&d.inputs, 2).iter().position(|p| *p == h));
    let folded = matches!(&d.kind, Kind::Frame(m) if !m.overrides_open);
    lit_rows(ui, "Corner Overrides", lit.is_some() && folded, |ui| {
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 2..3);
    });
    let mut crossings: Vec<bool> = crate::selection::items(&d.inputs, 2).iter().map(|p| crossing(&app.session, &d.kind, &d.inputs, p)).collect();
    crossings.extend(kept_crossings(&app.session, d));
    enter | corner_rows(ui, d, lit, &crossings)
}

/// `add`'s rows, lit as a hovered row is when `lit` (rect `dialog:lit:{name}`).
fn lit_rows(ui: &mut egui::Ui, name: &str, lit: bool, add: impl FnOnce(&mut egui::Ui)) {
    let behind = ui.painter().add(egui::Shape::Noop);
    let top = ui.cursor().top();
    add(ui);
    if lit {
        let t = solidcraft_theme::Tokens::get();
        let rect = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top());
        ui.painter().set(behind, egui::Shape::rect_filled(rect, t.radius_sm, t.hover));
        crate::scenario::publish_rect(&format!("dialog:lit:{name}"), rect);
    }
}

/// Whether input `input` of the dialog folds rows of its own (Frame Member's Corner Overrides)
/// rather than listing its picks: `Some(open)`.
pub(crate) fn folds_own_rows(kind: &Kind, input: usize) -> Option<bool> {
    match kind {
        Kind::Frame(m) if input == 2 => Some(m.overrides_open),
        _ => None,
    }
}

/// Fold or open input `input`'s own rows; false when it has none.
pub(crate) fn toggle_own_rows(kind: &mut Kind, input: usize) -> bool {
    match kind {
        Kind::Frame(m) if input == 2 => {
            m.overrides_open = !m.overrides_open;
            true
        }
        _ => false,
    }
}

/// "Corner n", the label of the n-th override row (from 1).
/// What a kept override's row says: the type of one where a point-to-point member ends (Corner
/// Overrides can't pick that corner, but it is no less there), Crossing for one of lines crossing
/// where no sketch point is (`crossing`, [`frame::members_cross`]: it still applies), else Missing
/// corner.
fn kept_corner_text(o: &Value, crossing: bool) -> &str {
    if crossing {
        return tr("Crossing");
    }
    let o = read_override(o);
    let names_pair = o.as_ref().is_some_and(|o| o.lines().any(|id| frame::parse_pair_id(id).is_some()));
    let corner = o.and_then(|o| CORNER_TYPES.get(corner_index(&o.kind)));
    tr(corner.filter(|_| names_pair).map_or("Missing corner", |c| c.1))
}

/// A corner override as the dialog keeps or sends it, read in either shape (`frame.member`'s).
fn read_override(o: &Value) -> Option<frame::CornerOverride> {
    serde_json::from_value(o.clone()).ok()
}

fn corner_label(n: usize) -> String {
    solidcraft_ui_text::format("ui.frame_corner_number", &[("n", &n.to_string())])
}

/// Under Corner Overrides, one row for each picked corner, "Corner n" in pick order: its type (as it
/// builds on this frame, [`override_type`]) or for a crossing "Crossing" (its Flip keeps the other
/// tube whole), its Flip (greyed for None and Mitre, so the rows line up) and a bin, with a Gap row under it (empty:
/// the frame's, shown as the hint); then one for each saved override whose lines no longer meet, kept until it is
/// removed (as it was saved: no Gap). Folded, none show. `lit`: the picked corner under the pointer in the view, whose row is lit.
/// `crossings`: which picks are crossings ([`crossing`]), then which kept overrides are ([`kept_crossings`]). True
/// when Enter was pressed in a Gap.
fn corner_rows(ui: &mut egui::Ui, d: &mut Dialog, lit: Option<usize>, crossings: &[bool]) -> bool {
    let Kind::Frame(m) = &mut d.kind else { return false };
    let picks = crate::selection::items(&d.inputs, 2).to_vec();
    // A corner taken out forgets its own type and flip: picked again, it starts afresh.
    m.overrides.retain(|o| picks.contains(&o.pick));
    for pick in &picks {
        if !m.overrides.iter().any(|o| o.pick == *pick) {
            // A new pick starts as the feature's type, unflipped, and shows its row.
            m.overrides.push(Override::new(pick.clone(), m.corner));
            m.overrides_open = true;
        }
    }
    let kept: Vec<Value> = d.extra.get("corners").and_then(Value::as_array).cloned().unwrap_or_default();
    if !m.overrides_open && !picks.is_empty() {
        return false;
    }
    let frame = m.corner;
    let frame_gap = if m.gap.trim().is_empty() { "0 mm".to_string() } else { m.gap.clone() };
    let mut enter = false;
    let choices = override_choices(frame);
    let skip = CORNER_TYPES.len() - choices.len();
    let labels: Vec<&str> = choices.iter().map(|c| c.1).collect();
    let mut drop = None;
    for (n, pick) in picks.iter().enumerate() {
        let Some(Override { corner, flip, gap, .. }) = m.overrides.iter_mut().find(|o| o.pick == *pick) else { continue };
        let label = corner_label(n + 1);
        let crossed = crossings.get(n).copied().unwrap_or(false);
        // A scope of its own keeps each row's ids apart.
        ui.scope_builder(egui::UiBuilder::new().id_salt(("frame_corner", n)), |ui| {
            lit_rows(ui, &label, lit == Some(n), |ui| {
                form_row(ui, &label, |ui| {
                    // What is left beside Flip (box, gap, label) and the bin.
                    let t = solidcraft_theme::Tokens::get();
                    let beside = t.space_14 + t.space_6 + crate::ui_kit::ContentWidth::label(ui, tr("Flip")) + t.control_height;
                    let room = ui.available_width() - beside - 2.0 * ui.spacing().item_spacing.x;
                    // The type takes the width its longest label needs (never what Flip and the bin need).
                    let translated: Vec<&str> = labels.iter().map(|l| solidcraft_ui_text::tr_label(l)).collect();
                    let width = crate::ui_kit::ContentWidth::dropdown(ui, &translated).min(room);
                    if crossed {
                        // A crossing has no type to choose, only which tube stays whole.
                        crate::ui_kit::settings_control_width(ui, width, |ui| {
                            ui.label(tr("Crossing"));
                            ui.add_space(ui.available_width());
                        });
                    } else {
                        // An Auto override on a frame of another type shows as that type, and
                        // stays Auto until a type is chosen for it.
                        let mut shown = override_type(*corner, frame).saturating_sub(skip);
                        let before = shown;
                        crate::ui_kit::settings_control_width(ui, width, |ui| {
                            crate::ui_kit::dropdown(ui, &format!("frame_corner_{n}"), &mut shown, &labels)
                        });
                        if shown != before {
                            *corner = shown + skip;
                        }
                    }
                    let enabled = crossed || override_flips(override_type(*corner, frame));
                    ui.add_enabled_ui(enabled, |ui| {
                        let mut on = *flip && enabled;
                        crate::ui_kit::checkbox(ui, tr("Flip"), &mut on);
                        if enabled {
                            *flip = on;
                        }
                    });
                    // The bin sits at the right edge of the value column.
                    ui.add_space((ui.available_width() - t.control_height).max(0.0));
                    if crate::ui_kit::bin_button(ui, tr("Remove this corner")).clicked() {
                        drop = Some(pick.clone());
                    }
                });
                enter |= hint_row(ui, "Gap", gap, &frame_gap);
            });
        });
    }
    if let Some(pick) = drop
        && let Some(input) = d.inputs.get_mut(2)
    {
        input.items.retain(|x| *x != pick);
    }
    let mut gone = None;
    for (k, o) in kept.iter().enumerate() {
        let label = corner_label(picks.len() + k + 1);
        ui.scope_builder(egui::UiBuilder::new().id_salt(("frame_missing_corner", k)), |ui| {
            form_row(ui, &label, |ui| {
                ui.label(kept_corner_text(o, crossings.get(picks.len() + k).copied().unwrap_or(false)));
                let t = solidcraft_theme::Tokens::get();
                ui.add_space((ui.available_width() - t.control_height).max(0.0));
                if crate::ui_kit::bin_button(ui, tr("Remove this corner")).clicked() {
                    gone = Some(k);
                }
            });
        });
    }
    if let Some(k) = gone
        && let Some(Value::Array(list)) = d.extra.get_mut("corners")
    {
        let mut i = 0;
        list.retain(|_| {
            i += 1;
            i != k + 1
        });
        if list.is_empty() {
            d.extra.remove("corners");
        }
    }
    enter
}

/// Why a point picked into Corner Overrides is not taken: no two members end there alone. `None`
/// takes it (or, already picked, takes it out).
pub(crate) fn refused_corner(s: &Session, m: &Member, inputs: &[SelInput], pick: &Sel) -> Option<String> {
    if crate::selection::items(inputs, 2).contains(pick) {
        return None;
    }
    let Sel::ExternalSketchPoint { sketch, id } = pick else { return None };
    let (st, lines) = (s.model.state(), path_lines(m, inputs, *sketch));
    let crossed = splits(m.corner) && frame::is_crossing(&st, *sketch, &lines, id);
    frame::corner_at_point(&st, *sketch, &lines, id, m.corner == AUTO || crossed).err().map(|e| e.to_string())
}

/// The path's lines of sketch `sketch`, among which a corner is looked for: none (every line) under All Lines.
fn path_lines(m: &Member, inputs: &[SelInput], sketch: u64) -> Vec<String> {
    if m.all {
        return Vec::new();
    }
    crate::selection::items(inputs, 0)
        .iter()
        .filter_map(|x| match x {
            Sel::ExternalSketchCurve { sketch: k, id } | Sel::SketchCurve { id, sketch: Some(k) } => (*k == sketch).then(|| id.clone()),
            Sel::SketchCurve { id, sketch: None } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Whether the corner picked as `pick` is a crossing of the frame's lines on a frame that cuts
/// crossings ([`splits`]): its row reads Crossing.
fn crossing(s: &Session, kind: &Kind, inputs: &[SelInput], pick: &Sel) -> bool {
    let (Kind::Frame(m), Sel::ExternalSketchPoint { sketch, id }) = (kind, pick) else { return false };
    splits(m.corner) && frame::is_crossing(&s.model.state(), *sketch, &path_lines(m, inputs, *sketch), id)
}

/// For each override an edit keeps ([`kept_corner_text`]), whether it is the override of a frame
/// that cuts crossings ([`splits`]) of lines of its path crossing where no sketch point is.
fn kept_crossings(s: &Session, d: &Dialog) -> Vec<bool> {
    let Some(Value::Array(kept)) = d.extra.get("corners") else { return Vec::new() };
    let auto = matches!(&d.kind, Kind::Frame(m) if splits(m.corner));
    let path = d.editing.and_then(|(id, _)| s.doc.feature(id)).and_then(|f| match &f.kind {
        FeatureKind::FrameMember { path_sketch, .. } => *path_sketch,
        _ => None,
    });
    let st = s.model.state();
    kept.iter()
        .map(|o| {
            let members: Vec<String> = read_override(o).map(|o| o.lines().map(String::from).collect()).unwrap_or_default();
            auto && path.is_some_and(|ps| frame::members_cross(&st, ps.0, &members))
        })
        .collect()
}

/// The Frame Member rows proper: the section, its placement and the name.
fn member_rows(app: &SolveApp, ui: &mut egui::Ui, m: &mut Member, inputs: &[SelInput]) -> bool {
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
    let mut pair_ends = Vec::with_capacity(pair_picks.len());
    for (a, b) in pair_picks {
        let mut pair = json!({"a": end(a)?, "b": end(b)?});
        let mut kept = None;
        if let (Some(Some(x)), Some(Some(y))) = (ids.get(a), ids.get(b))
            && x == y
        {
            pair["id"] = json!(x);
            kept = Some(x.clone());
        }
        pairs.push(pair);
        pair_ends.push((kept, [picks.get(a), picks.get(b)]));
    }
    // Each pair's id as `frame.member` gives it: the one it keeps, else, in order, the next number past the feature's
    // `next_pair` and every id kept (as the command and Edit Feature number new pairs).
    let past = pair_ends.iter().filter_map(|(id, _)| frame::parse_pair_id(id.as_deref()?)).max().map_or(1, |n| n.saturating_add(1));
    let mut next = m.next_pair.max(past);
    let pair_ends: Vec<(String, [Option<&Sel>; 2])> = pair_ends
        .into_iter()
        .map(|(id, ends)| {
            let id = id.unwrap_or_else(|| {
                let id = frame::pair_id(next);
                next = next.saturating_add(1);
                id
            });
            (id, ends)
        })
        .collect();
    if lines.is_empty() && pairs.is_empty() {
        return Err("select lines or points for the path first".into());
    }
    let mut p = json!({});
    let sketch = match lines.is_empty() {
        true => None,
        false => Some(known.or_else(|| curves_sketch(app, &lines, section_sketch)).ok_or("the lines must be in one sketch, not the section's")?),
    };
    if let Some(sketch) = sketch {
        p["sketch"] = json!(sketch);
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
    if m.corner != NONE {
        p["corner"] = json!(corner_name(m.corner));
        // Fit Ends to Earlier Frames applies under every type but None (ruling A10).
        if m.fit_ends {
            p["fit_ends"] = json!(true);
        }
        if m.coped {
            p["coped"] = json!(true);
        }
        if !no_gap(&app.session, &m.gap) {
            p["gap"] = json!(m.gap.trim());
        }
    }
    if m.flip && flips(m.corner) {
        p["flip"] = json!(true);
    }
    // Each picked corner, saved by the lines that end there (two, or under Auto two or more).
    let mut corners = Vec::new();
    let picks = crate::selection::items(inputs, 2);
    if !picks.is_empty() {
        let st = app.session.model.state();
        let members: &[String] = if m.all { &[] } else { &lines };
        for pick in picks {
            let Sel::ExternalSketchPoint { sketch: at, id } = pick else { continue };
            let sketch = sketch.filter(|s| s == at).ok_or("the corner overrides must be corners of the frame sketch")?;
            // A crossing is named by the lines crossing there alone (a pair's tube is never cut).
            let crossed = splits(m.corner) && frame::is_crossing(&st, sketch, members, id);
            let mut ends = frame::corner_at_point(&st, sketch, members, id, m.corner == AUTO || crossed).map_err(|e| e.to_string())?;
            // Under Auto the build counts every member ending at the corner, the pairs' too (ruling A16).
            if m.corner == AUTO
                && !crossed
                && let Some(at) = path_point(&app.session, pick)
            {
                let here = |p: &Option<&Sel>| p.and_then(|p| path_point(&app.session, p)).is_some_and(|q| q.dist(at) <= CORNER_TOL);
                ends.extend(pair_ends.iter().filter(|(_, e)| e.iter().any(here)).map(|(id, _)| id.clone()));
            }
            let o = m.overrides.iter().find(|o| o.pick == *pick);
            let (corner, flip) = o.map_or((m.corner, false), |o| (o.corner, o.flip));
            let corner = override_type(corner, m.corner);
            // A crossing's Flip keeps the other tube whole, whatever the type.
            let flips = crossed || override_flips(corner);
            let mut entry = json!({"kind": corner_name(corner)});
            // A row whose Flip was not touched goes back in its saved order, as long as it names the same members;
            // any other row as `corner_at_point` lists them with its Flip (unset where greyed), which the command
            // applies to today's order: so a Mitre or None row is saved in today's order too, and reopens unflipped.
            let same = |order: &Vec<String>| order.len() == ends.len() && ends.iter().all(|e| order.contains(e));
            match o.and_then(|o| o.saved.as_ref()).filter(|(order, was)| *was == flip && same(order)) {
                Some((order, _)) => entry["members"] = json!(order),
                None => {
                    entry["flip"] = json!(flip && flips);
                    entry["members"] = json!(ends);
                }
            }
            if let Some(gap) = o.map(|o| o.gap.trim()).filter(|g| !g.is_empty()) {
                entry["gap"] = json!(gap);
            }
            corners.push(entry);
        }
    }
    // Overrides whose corner has gone stay until removed (the feature then says so).
    if let Some(Value::Array(kept)) = extra.get("corners") {
        corners.extend(kept.iter().cloned());
    }
    if !corners.is_empty() {
        p["corners"] = json!(corners);
    }
    if let Value::Object(o) = &mut p {
        for (key, v) in extra {
            o.entry(key.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("frame.member".to_string(), p)])
}

/// Whether a weld gap typed as `g` is none: empty, or a plain 0 (`0`, `0 mm`; a parameter that is 0 now is kept).
fn no_gap(s: &Session, g: &str) -> bool {
    g.trim().is_empty() || (solidcraft_engine::doc::expr::is_literal(g) && s.doc.eval(g, ValueKind::Length).is_ok_and(|v| v == 0.0))
}

/// Where a point picked into the path is in the world, as the view shows it: a sketch point picked across sketches
/// (`<sketch>:<point>`) where its sketch puts it, a construction point where it is built, any other pick as
/// [`crate::primitive_placement::world_point`] finds it.
fn path_point(s: &Session, sel: &Sel) -> Option<solidcore_record::Vec3> {
    match sel {
        Sel::SketchPoint { id } if id.contains(':') => {
            let (sketch, point) = id.split_once(':')?;
            crate::primitive_placement::world_point(s, &Sel::ExternalSketchPoint { sketch: sketch.parse().ok()?, id: point.to_string() })
        }
        Sel::ConstructionPoint { name } => match s.world_state().ok()?.construct.iter().find(|c| c.name == *name)?.geom {
            solidcraft_engine::doc::construct::ConstructGeom::Point(p) => Some(p),
            _ => None,
        },
        _ => crate::primitive_placement::world_point(s, sel),
    }
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
pub fn for_feature(app: &SolveApp, feature: &solidcraft_engine::doc::Feature) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let kind = &feature.kind;
    let FeatureKind::FrameMember {
        path_sketch,
        lines,
        pairs,
        profile,
        anchor,
        locate,
        angle,
        mirror,
        body_name,
        corner,
        flip,
        corners,
        fit_ends,
        coped,
        gap,
        next_pair,
        ..
    } = kind
    else {
        return None;
    };
    let (_, mut inputs) = start(app, "frame.member")?;
    let st = app.session.model.state();
    let mut m = Member::new();
    m.all = lines.is_empty() && path_sketch.is_some();
    // The sketch's lines (without the pairs' ids, which come after them, or an Auto frame's later pieces).
    let ids = if m.all {
        let (ids, later) = frame::member_parts(&st, kind).unwrap_or_default();
        ids.into_iter().zip(later).filter(|(_, piece)| piece.is_none()).map(|(id, _)| id).collect()
    } else {
        lines.clone()
    };
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
    m.corner = corner_index(corner);
    m.flip = *flip;
    m.fit_ends = *fit_ends;
    m.coped = *coped;
    m.gap = gap.as_deref().map(|g| shown_expr(g, ValueKind::Length)).unwrap_or_default();
    m.next_pair = *next_pair;
    // Each override shows as the point of its corner; one whose lines no longer end there alone
    // (one has gone, or another now ends there too), or one of a corner where a point-to-point
    // member ends (not a sketch point the input can show), is kept as it is, listed with a bin so
    // that it can be removed ([`kept_corner_text`]).
    let path_lines: Vec<String> = if m.all { Vec::new() } else { lines.iter().filter(|id| !pairs.iter().any(|p| p.id == **id)).cloned().collect() };
    let auto = m.corner == AUTO;
    let matched = |ps: u64, members: &[String]| {
        let id = frame::corner_point(&st, ps, members)?;
        let crossed = splits(m.corner) && frame::is_crossing(&st, ps, &path_lines, &id);
        let ends = frame::corner_at_point(&st, ps, &path_lines, &id, auto || crossed).ok()?;
        (ends.len() == members.len() && ends.iter().all(|e| members.contains(e))).then_some((ps, id))
    };
    // Flip shows an override whose members run in another order than today's rules give them (saved as an order),
    // worked out on the model before this feature (as the command applies a Flip).
    let before = app.session.model.state_before(feature.id);
    let flipped = |lines: &[String]| frame::default_order(&app.session.doc, &before, feature, lines).is_ok_and(|d| d.get(..2) != lines.get(..2));
    let mut missing = Vec::new();
    for o in corners {
        let lines: Vec<String> = o.lines().map(str::to_string).collect();
        match path_sketch.and_then(|ps| matched(ps.0, &lines)) {
            Some((sketch, id)) => {
                let pick = Sel::ExternalSketchPoint { sketch, id };
                if let Some(i) = inputs.get_mut(2) {
                    i.items.push(pick.clone());
                }
                let flip = flipped(&lines);
                let gap = o.gap.as_deref().map(|g| shown_expr(g, ValueKind::Length)).unwrap_or_default();
                m.overrides.push(Override { flip, gap, saved: Some((lines, flip)), ..Override::new(pick, corner_index(&o.kind)) });
            }
            None => missing.push(o),
        }
    }
    if !missing.is_empty() {
        extra.insert("corners".into(), json!(missing));
    }
    Some((Kind::Frame(m), inputs, extra))
}

#[cfg(test)]
#[path = "dialogs_frame_tests.rs"]
mod tests;
