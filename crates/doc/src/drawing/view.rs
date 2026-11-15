//! A drawing's views (W17a, #4816): each a view of one of the drawing's designs, on one of its
//! sheets. A **base view** draws a source from a direction ([`ViewDirection`]). A **projected
//! view** sits on one side of its parent and looks from that side, or from the side away from it
//! in first angle ([`Drawing::projection`], [`projected_basis`]); lined up with its parent, it saves
//! only its place along the free axis, as the other comes from the parent ([`Drawing::view_centre`]).
//! A **section view** (#13046) is cut along a [`CuttingLine`] drawn in its parent's plane, and looks
//! the line's arrows' way ([`CuttingLine::basis`]); it is always placed at a point, and named by its
//! letter.
//!
//! What a view sets itself is in [`ViewOverrides`]: each setting it leaves out follows its parent,
//! then the default, and [`Drawing::resolve_view`] is that one rule for every reader. A projected
//! view's source and basis come from its chain of parents, so neither is saved. Which occurrences it
//! draws is such a setting too, and the paths aren't checked against the design: deleting an
//! occurrence must never refuse the file, so what is left to draw is the reader's to say.
//!
//! The rules, beside the record's: a view is on one of the drawing's sheets; a base view draws one
//! of its sources, has a scale and looks from a direction that makes a view; every parent is a view
//! of the drawing, no chain of parents loops, and a chain is at most [`MAX_VIEW_DEPTH`] views; a
//! view lined up with its parent is on the parent's sheet; a section view is placed at a point, and its
//! cutting line keeps [`CuttingLine::new`]'s rules; a view's place is on paper
//! ([`is_paper_coordinate`]); a view's occurrence paths keep [`check_occurrence_paths`]; at most
//! [`MAX_VIEWS`] views, listed by id ([`Drawing::add_views`]).

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Vec2};
use solidcraft_geom::cell::check_label;
use solidcraft_geom::drawing_sheet::view_letter;
use solidcraft_geom::drawing_view::{ProjectedSide, Scale, is_paper_coordinate, projected_basis};
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_render::VisualStyle;

use super::cutting_line::CuttingLine;
use super::{Drawing, add_by_id, in_id_order, require};
use crate::drawing_view::{
    Exploded, MAX_JOINT_VALUES, MAX_POSE_JOINTS, MAX_POSE_VALUE, MAX_REFERENCE_SHOWN_PERCENT, OccurrencePaths, Pose, TangentEdges, ViewConfiguration,
    ViewDirection, ViewOverrides, ViewSettings,
};
use crate::{DocError, MAX_PLACEMENT_DEPTH, Result, ids};

/// The most views a drawing may have.
pub const MAX_VIEWS: usize = 4096;
/// The most views in a chain, from a base view down through the views projected from it.
pub const MAX_VIEW_DEPTH: usize = 32;
/// Occurrence paths a view's `occurrences` or `hide` may hold, each.
pub const MAX_PATHS: usize = 1_000;
/// Ids a view's `occurrences` and `hide` may hold in all.
pub const MAX_PATH_IDS: usize = 4_096;

/// One view of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// The sheet it is on.
    #[serde(with = "crate::ids")]
    pub sheet: u64,
    pub kind: ViewKind,
    /// Where its centre sits on its sheet.
    pub place: ViewPlace,
    /// What it sets itself; each setting left out follows its parent, then the default.
    #[serde(default, skip_serializing_if = "ViewOverrides::is_empty")]
    pub settings: ViewOverrides,
    /// How far its boundary grows from the model's box towards the box that also holds its reference
    /// parts, each side on its own: 0 to [`MAX_REFERENCE_SHOWN_PERCENT`], each view its own.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reference_shown_percent: f64,
    /// Its label (#13044), from when it is first placed apart from its projection place
    /// ([`Drawing::give_letter`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<ViewLabel>,
}

/// The most letters a view's letter has: `ZZZ` is the last one given.
pub const MAX_LETTERS: usize = 3;
/// What a new label prints before its letter: English capitals, as a new parts list's headings are,
/// typed over after.
pub const VIEW_WORDS: &str = "VIEW";
/// What a new label prints before its scale.
pub const SCALE_WORDS: &str = "SCALE";

/// A view's label (#13044): its letter, which a section line, a detail's circle or a note names the
/// view by, and what it prints. Made when the view is given its letter, then typed over; drawn while
/// the view needs it ([`View::needs_letter`]) and it is `shown`, above the view unless moved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewLabel {
    /// One to [`MAX_LETTERS`] capitals A to Z.
    pub letter: String,
    /// What it prints before its letter, [`VIEW_WORDS`] when made; empty: the letter alone.
    pub words: String,
    /// What it prints before its scale, [`SCALE_WORDS`] when made.
    pub scale_words: String,
    /// A name, on a line of its own under its letter's; empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub show_letter: bool,
    /// Its view's scale, shown when it isn't its sheet's ([`Drawing::sheet_scale`]).
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub show_scale: bool,
    /// Whether it is on the sheet: a hidden label keeps the view's letter.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub shown: bool,
    /// Where its middle sits from its view's centre, in paper millimetres, so it moves with the view;
    /// none: above the view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f64; 2]>,
}

impl ViewLabel {
    /// A new label of `letter`, printing [`VIEW_WORDS`] and [`SCALE_WORDS`].
    pub fn new(letter: String) -> ViewLabel {
        ViewLabel {
            letter,
            words: VIEW_WORDS.into(),
            scale_words: SCALE_WORDS.into(),
            name: String::new(),
            show_letter: true,
            show_scale: true,
            shown: true,
            offset: None,
        }
    }
}

fn yes() -> bool {
    true
}

fn is_yes(v: &bool) -> bool {
    *v
}

/// How a drawing gives its views their letters ([`Drawing::give_letter`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewLetters {
    /// Whether a letter a deleted view held is given again (the default), or never.
    pub reuse: bool,
    /// How far along the sequence letters have been given: a letter is given past this when letters
    /// aren't reused. Counted either way, so stopping reuse later still skips what was given before.
    pub given: u32,
}

impl Default for ViewLetters {
    fn default() -> ViewLetters {
        ViewLetters { reuse: true, given: 0 }
    }
}

impl ViewLetters {
    pub fn is_default(&self) -> bool {
        *self == ViewLetters::default()
    }

    /// The next [`view_letter`] that none of `used` is: the first, or the first
    /// past those given when letters aren't reused. It counts as given. Only as many places as `used`
    /// holds, and one more, are looked at: at most that many can be taken. Refused past
    /// [`MAX_LETTERS`] letters.
    pub fn next<'a>(&mut self, used: impl IntoIterator<Item = &'a str>) -> Result<String> {
        let used: BTreeSet<&str> = used.into_iter().collect();
        let start = if self.reuse { 0 } else { self.given };
        let place = |k: usize| {
            let i = start.checked_add(u32::try_from(k).ok()?)?;
            Some((i, view_letter(usize::try_from(i).ok()?)?))
        };
        match (0..=used.len()).filter_map(place).find(|(_, letter)| !used.contains(letter.as_str())) {
            Some((i, letter)) if letter.len() <= MAX_LETTERS => {
                self.given = self.given.max(i.saturating_add(1));
                Ok(letter)
            }
            _ => Err(DocError::Invalid("every view letter up to ZZZ has been given".into())),
        }
    }
}

/// What a view draws, and from where.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewKind {
    /// The design of `source`, seen from `view`.
    Base {
        #[serde(with = "crate::ids")]
        source: u64,
        view: ViewDirection,
    },
    /// Projected from `parent`, on `side` of it.
    Projected {
        #[serde(with = "crate::ids")]
        parent: u64,
        side: ProjectedSide,
    },
    /// `parent` cut along `line`, drawn in `parent`'s plane, seen the way its arrows point.
    Section {
        #[serde(with = "crate::ids")]
        parent: u64,
        line: CuttingLine,
    },
}

impl ViewKind {
    /// Its name in replies: `base`, `projected` or `section`.
    pub fn name(&self) -> &'static str {
        match self {
            ViewKind::Base { .. } => "base",
            ViewKind::Projected { .. } => "projected",
            ViewKind::Section { .. } => "section",
        }
    }

    /// The view it comes from: none for a base view. The one place a view's parent is read, so every
    /// rule about parents (the chain, deleting, copying, listing) holds for each kind with one.
    pub fn parent(&self) -> Option<u64> {
        match self {
            ViewKind::Base { .. } => None,
            ViewKind::Projected { parent, .. } | ViewKind::Section { parent, .. } => Some(*parent),
        }
    }
}

/// Where a view's centre sits on its sheet, in paper millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewPlace {
    /// At this point of the sheet.
    At([f64; 2]),
    /// Lined up with its parent, this far along the free axis: across for a view to the left or
    /// right of it, up for one above or below. A projected view only.
    Aligned(f64),
}

/// Who gave a resolved setting its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetBy {
    /// The view itself.
    This,
    /// The parent of this id, the nearest that sets it.
    Parent(u64),
    /// No view of the chain: the view's sheet.
    Sheet,
    /// No view of the chain nor its sheet: the drawing.
    Drawing,
    /// No view of the chain: the default.
    Default,
}

/// A setting's value and who gave it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setting<T> {
    pub value: T,
    pub set_by: SetBy,
}

/// What a section view cuts with: its cutting line, and the plane it is drawn in, its parent's basis.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub line: CuttingLine,
    pub parent: Plane,
}

/// A view as it draws: the source it draws, which way it looks, and each setting with who gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedView {
    pub source: u64,
    pub basis: Plane,
    /// A section view's cut: none for any other view.
    pub cut: Option<Cut>,
    pub scale: Setting<Scale>,
    pub visual_style: Setting<VisualStyle>,
    pub reference_style: Setting<VisualStyle>,
    pub reference_hides_model: Setting<bool>,
    pub tangent_edges: Setting<TangentEdges>,
    /// The subtrees it draws: every body when None.
    pub occurrences: Setting<Option<OccurrencePaths>>,
    /// The subtrees it leaves out: none when None.
    pub hide: Setting<Option<OccurrencePaths>>,
    /// The line group its lines are drawn in: the chain's, else its sheet's, else the drawing's.
    pub line_group: Setting<LineGroup>,
    pub reference_shown_percent: f64,
    /// The state of its assembly it draws: assembled, or an exploded view of the design.
    pub exploded: Setting<Exploded>,
    /// The position of its assembly it draws: the design's, or its joints at values of its own.
    pub pose: Setting<Pose>,
    /// The configuration of the design it draws: the active one, or a row of its table.
    pub configuration: Setting<ViewConfiguration>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl View {
    /// Whether its label names it: it stands apart from its projection place, a projected view placed
    /// at a point, or it is a section view, which its cutting line names (`A–A`). A base view is the
    /// principal view and a view lined up with its parent is where its projection puts it, so neither
    /// needs one (ISO 128-30). Each later kind of view placed apart (auxiliary, detail) adds its arm
    /// here.
    pub fn needs_letter(&self) -> bool {
        matches!((&self.kind, self.place), (ViewKind::Projected { .. }, ViewPlace::At(_)) | (ViewKind::Section { .. }, _))
    }
}

/// What occurrence paths a view may choose, saved or asked for: at most [`MAX_PATHS`] in each list
/// and [`MAX_PATH_IDS`] ids in all, none deeper than an assembly goes, with `[]` (the design's own
/// bodies) in `occurrences` only.
pub fn check_occurrence_paths(occurrences: &[Vec<u64>], hide: &[Vec<u64>]) -> Result<()> {
    let ids = occurrences.iter().chain(hide).map(Vec::len).fold(0usize, usize::saturating_add);
    require(occurrences.len() <= MAX_PATHS && hide.len() <= MAX_PATHS && ids <= MAX_PATH_IDS, || {
        format!("at most {MAX_PATHS} occurrence paths in source.occurrences and in hide, and {MAX_PATH_IDS} ids in all")
    })?;
    require(occurrences.iter().chain(hide).all(|path| path.len() <= MAX_PLACEMENT_DEPTH), || {
        format!("an occurrence path is 1..{MAX_PLACEMENT_DEPTH} ids")
    })?;
    require(hide.iter().all(|path| !path.is_empty()), || "hide [] would leave out the design's own bodies: give source instead".into())
}

/// What a view's position may hold, saved or asked for: 1 to [`MAX_POSE_JOINTS`] joints, each once,
/// each with 1 to [`MAX_JOINT_VALUES`] values, finite and at most [`MAX_POSE_VALUE`] either way; or
/// any study step, which its types bound. It isn't checked against the design: a joint or study the
/// design no longer has is skipped where it draws.
pub fn check_pose(pose: &Pose) -> Result<()> {
    let Pose::Joints(joints) = pose else { return Ok(()) };
    require(!joints.is_empty(), || "a position sets at least one joint: give \"designed\" instead".into())?;
    require(joints.len() <= MAX_POSE_JOINTS, || format!("a position sets at most {MAX_POSE_JOINTS} joints"))?;
    require(joints.iter().all(|j| (1..=MAX_JOINT_VALUES).contains(&j.values.len())), || {
        format!("a joint in a position has 1..{MAX_JOINT_VALUES} values")
    })?;
    require(joints.iter().flat_map(|j| &j.values).all(|v| v.is_finite() && v.abs() <= MAX_POSE_VALUE), || {
        format!("a position's values are at most {MAX_POSE_VALUE} either way")
    })?;
    let listed: BTreeSet<u64> = joints.iter().map(|j| j.joint).collect();
    require(listed.len() == joints.len(), || "a position lists each joint once".into())
}

/// The views from `id` up to its base view, nearest first, each found by `find`: refused when a
/// parent isn't one, the chain loops, or it is longer than [`MAX_VIEW_DEPTH`].
fn chain<'a>(id: u64, find: impl Fn(u64) -> Option<&'a View>) -> Result<Vec<&'a View>> {
    let mut chain: Vec<&View> = Vec::new();
    let mut at = id;
    loop {
        let view = find(at).ok_or_else(|| match chain.as_slice() {
            [] => DocError::Invalid(format!("the drawing has no view {}", ids::encode(at))),
            [_] => DocError::Invalid(format!("its parent {} isn't one of the drawing's views", ids::encode(at))),
            // Further up, the view whose parent is missing is named: it is that view's parent.
            [.., child] => DocError::Invalid(format!(
                "its chain of parents reaches view {}, whose parent {} isn't one of the drawing's views",
                ids::encode(child.id),
                ids::encode(at)
            )),
        })?;
        require(chain.iter().all(|v| v.id != at), || "its chain of parents loops".into())?;
        chain.push(view);
        match view.kind.parent() {
            None => return Ok(chain),
            Some(parent) => at = parent,
        }
        require(chain.len() < MAX_VIEW_DEPTH, || format!("a chain of views is at most {MAX_VIEW_DEPTH} views"))?;
    }
}

/// The nearest view of `chain` (the view first) that sets a setting `field` reads, with its value.
fn inherited<T>(chain: &[&View], field: impl Fn(&ViewOverrides) -> Option<T>) -> Option<Setting<T>> {
    chain.iter().enumerate().find_map(|(i, view)| {
        let set_by = if i == 0 { SetBy::This } else { SetBy::Parent(view.id) };
        field(&view.settings).map(|value| Setting { value, set_by })
    })
}

/// [`inherited`], or the default `value` when no view of the chain sets it.
fn or_default<T>(chain: &[&View], field: impl Fn(&ViewOverrides) -> Option<T>, value: T) -> Setting<T> {
    inherited(chain, field).unwrap_or(Setting { value, set_by: SetBy::Default })
}

impl Drawing {
    /// Adds `views`, each where its id puts it: a drawing lists its views by id, never in the order
    /// they were added in, so the same views always save as the same bytes. Every edit keeps the
    /// order ([`Drawing::validate`]), and reading puts a file's views in it.
    pub fn add_views(&mut self, views: impl IntoIterator<Item = Arc<View>>) {
        add_by_id(&mut self.views, views, |v| v.id);
    }

    /// View `id`, found by halves in the views' id order.
    pub fn view(&self, id: u64) -> Option<&View> {
        self.view_index(id).and_then(|i| self.views.get(i)).map(|v| &**v)
    }

    /// `views` and every view projected or cut from them, directly or through others. A parent can sort
    /// after its child, so each pass over the views finds at least one more level of a chain, which
    /// is at most [`MAX_VIEW_DEPTH`] views.
    pub fn with_projected(&self, views: &BTreeSet<u64>) -> BTreeSet<u64> {
        let mut all = views.clone();
        for _ in 0..MAX_VIEW_DEPTH {
            let before = all.len();
            for view in &self.views {
                if view.kind.parent().is_some_and(|parent| all.contains(&parent)) {
                    all.insert(view.id);
                }
            }
            if all.len() == before {
                break;
            }
        }
        all
    }

    /// Where view `id` is in [`Drawing::views`].
    pub fn view_index(&self, id: u64) -> Option<usize> {
        self.views.binary_search_by_key(&id, |v| v.id).ok()
    }

    /// The views' rules (the module's notes).
    pub(super) fn check_views(&self) -> Result<()> {
        require(self.views.len() <= MAX_VIEWS, || format!("a drawing has at most {MAX_VIEWS} views"))?;
        // In id order, so each view after this is found by halves.
        require(in_id_order(&self.views, |v| v.id), || "a drawing lists its views by id".into())?;
        for view in &self.views {
            self.check_view(view).map_err(|e| DocError::Invalid(format!("view {}: {e}", ids::encode(view.id))))?;
        }
        Ok(())
    }

    fn check_view(&self, view: &View) -> Result<()> {
        let on = |sheet: u64| format!("it is on sheet {}, which the drawing doesn't have", ids::encode(sheet));
        require(self.sheets.iter().any(|s| s.id == view.sheet), || on(view.sheet))?;
        let percent = view.reference_shown_percent;
        require(percent.is_finite() && (0.0..=MAX_REFERENCE_SHOWN_PERCENT).contains(&percent), || {
            format!("its reference_shown_percent is 0 to {MAX_REFERENCE_SHOWN_PERCENT}")
        })?;
        let place = match view.place {
            ViewPlace::At([x, y]) => is_paper_coordinate(x) && is_paper_coordinate(y),
            ViewPlace::Aligned(along) => is_paper_coordinate(along),
        };
        require(place, || format!("its place is at most {MAX_SIDE_MM} mm from the sheet's origin"))?;
        check_occurrence_paths(view.settings.occurrences.as_deref().unwrap_or_default(), view.settings.hide.as_deref().unwrap_or_default())?;
        view.settings.pose.as_ref().map_or(Ok(()), check_pose)?;
        if let Some(label) = &view.label {
            let letter = (1..=MAX_LETTERS).contains(&label.letter.len()) && label.letter.bytes().all(|b| b.is_ascii_uppercase());
            require(letter, || "its letter is one to three capital letters A to Z".into())?;
            for text in [&label.words, &label.scale_words, &label.name] {
                check_label(text).map_err(|e| DocError::Invalid(e.to_string()))?;
            }
            let offset = label.offset.is_none_or(|[x, y]| is_paper_coordinate(x) && is_paper_coordinate(y));
            require(offset, || format!("its label's offset is at most {MAX_SIDE_MM} mm from the view's centre"))?;
        }
        match &view.kind {
            ViewKind::Base { source, view: direction } => {
                let drawn = || format!("it draws source {}, which the drawing doesn't have", ids::encode(*source));
                require(self.sources.iter().any(|s| s.id == *source), drawn)?;
                require(view.settings.scale.is_some(), || "a base view has a scale".into())?;
                require(matches!(view.place, ViewPlace::At(_)), || "a base view is placed at a point: it has no parent to line up with".into())?;
                direction.basis().map(|_| ()).map_err(|e| DocError::Invalid(e.to_string()))
            }
            ViewKind::Projected { parent, .. } => {
                self.chain_of(view.id)?;
                let parent_sheet = self.view(*parent).map(|p| p.sheet);
                let lined_up = matches!(view.place, ViewPlace::Aligned(_));
                require(!lined_up || parent_sheet == Some(view.sheet), || "a view lined up with its parent is on its parent's sheet".into())
            }
            ViewKind::Section { .. } => {
                self.chain_of(view.id)?;
                require(matches!(view.place, ViewPlace::At(_)), || "a section view is placed at a point: it has no side to line up on".into())
            }
        }
    }

    /// View `id` as it draws: its base view's source, its basis turned down its chain of parents
    /// ([`projected_basis`], in the drawing's projection; [`CuttingLine::basis`] at a section view),
    /// and each setting from the nearest view of the chain that sets it, then the default. A section
    /// view also gives its cut. One rule for every reader of a view.
    pub fn resolve_view(&self, id: u64) -> Result<ResolvedView> {
        let chain = self.chain_of(id)?;
        let Some(ViewKind::Base { source, view: direction }) = chain.last().map(|base| &base.kind) else {
            return Err(DocError::Invalid(format!("view {} has no base view", ids::encode(id))));
        };
        let source = *source;
        let mut basis = direction.basis().map_err(|e| DocError::Invalid(e.to_string()))?;
        let mut cut = None;
        for view in chain.iter().rev() {
            match &view.kind {
                ViewKind::Base { .. } => {}
                ViewKind::Projected { side, .. } => {
                    basis = projected_basis(&basis, *side, self.projection).map_err(|e| DocError::Invalid(e.to_string()))?;
                }
                ViewKind::Section { line, .. } => {
                    let parent = basis;
                    basis = line.basis(&parent)?;
                    // Only the view itself is cut: a view projected from a section view isn't.
                    cut = (view.id == id).then(|| Cut { line: line.clone(), parent });
                }
            }
        }
        let scale = inherited(&chain, |s| s.scale).ok_or_else(|| DocError::Invalid(format!("view {} has no scale", ids::encode(id))))?;
        let defaults = ViewSettings::default();
        let sheet = chain.first().and_then(|view| self.sheets.iter().find(|sheet| sheet.id == view.sheet));
        let line_group = inherited(&chain, |s| s.line_group).unwrap_or(match sheet.and_then(|sheet| sheet.line_group) {
            Some(value) => Setting { value, set_by: SetBy::Sheet },
            None => Setting { value: self.line_group, set_by: SetBy::Drawing },
        });
        Ok(ResolvedView {
            source,
            basis,
            cut,
            scale,
            visual_style: or_default(&chain, |s| s.visual_style, defaults.visual_style),
            reference_style: or_default(&chain, |s| s.reference_style, defaults.reference_style),
            reference_hides_model: or_default(&chain, |s| s.reference_hides_model, defaults.reference_hides_model),
            tangent_edges: or_default(&chain, |s| s.tangent_edges, defaults.tangent_edges),
            occurrences: or_default(&chain, |s| s.occurrences.clone().map(Some), None),
            hide: or_default(&chain, |s| s.hide.clone().map(Some), None),
            line_group,
            reference_shown_percent: chain.first().map_or(0.0, |view| view.reference_shown_percent),
            exploded: or_default(&chain, |s| s.exploded, Exploded::Assembled),
            pose: or_default(&chain, |s| s.pose.clone(), Pose::Designed),
            configuration: or_default(&chain, |s| s.configuration, ViewConfiguration::Active),
        })
    }

    /// Where view `id`'s centre sits on its sheet, in paper millimetres. A view placed at a point
    /// is there. One lined up with its parent takes its own place along the free axis and its
    /// parent's centre across it ([`ProjectedSide::on_free_axis`]): both draw the same model, so
    /// their extents share that axis's middle, and they line up whatever their scales.
    pub fn view_centre(&self, id: u64) -> Result<Vec2> {
        let mut centre = Vec2::ZERO;
        // From the base view down to this one, each lined up with the one before or placed apart.
        for view in self.chain_of(id)?.iter().rev() {
            centre = match (view.place, &view.kind) {
                (ViewPlace::At([x, y]), _) => Vec2::new(x, y),
                (ViewPlace::Aligned(along), ViewKind::Projected { side, .. }) => side.on_free_axis(centre, along),
                (ViewPlace::Aligned(_), ViewKind::Base { .. }) => {
                    return Err(DocError::Invalid(format!("base view {} isn't placed at a point", ids::encode(view.id))));
                }
                (ViewPlace::Aligned(_), ViewKind::Section { .. }) => {
                    return Err(DocError::Invalid(format!("section view {} isn't placed at a point", ids::encode(view.id))));
                }
            };
        }
        Ok(centre)
    }

    /// Where view `id` goes dragged to `to` on its sheet, one rule for a drag and its preview. A view
    /// lined up with its parent takes only its free axis, and with `break_alignment` is placed
    /// apart there; a view placed at a point goes there. The views lined up with it follow by
    /// [`Self::view_centre`]'s rule, with no change to their records.
    pub fn place_moved_to(&self, id: u64, to: Vec2, break_alignment: bool) -> Result<ViewPlace> {
        let view = self.view(id).ok_or_else(|| DocError::Unknown(format!("view {}", ids::encode(id))))?;
        Ok(match (view.place, &view.kind) {
            (ViewPlace::Aligned(_), ViewKind::Projected { side, .. }) if !break_alignment => ViewPlace::Aligned(side.along(to)),
            _ => ViewPlace::At([to.x, to.y]),
        })
    }

    /// The views from `id` up to its base view, nearest first ([`chain`]).
    fn chain_of(&self, id: u64) -> Result<Vec<&View>> {
        chain(id, |id| self.view(id))
    }

    /// Gives view `id` its letter when it needs one ([`View::needs_letter`]) and has none, or holds one
    /// another view holds (a copy): the next of [`ViewLetters::next`], the one rule every command that
    /// places a view apart calls. Its label's words, name and place are kept. Nothing for a view the
    /// drawing hasn't.
    pub fn give_letter(&mut self, id: u64) -> Result<()> {
        let Some(view) = self.view(id) else { return Ok(()) };
        let held = |letter: &str| self.views.iter().any(|other| other.id != id && other.label.as_ref().is_some_and(|label| label.letter == letter));
        if !view.needs_letter() || view.label.as_ref().is_some_and(|label| !held(&label.letter)) {
            return Ok(());
        }
        let mut given = self.view_letters;
        let letter = given.next(self.views.iter().filter_map(|v| v.label.as_ref()).map(|label| label.letter.as_str()))?;
        let Some(view) = self.view_index(id).and_then(|at| self.views.get_mut(at)).map(Arc::make_mut) else { return Ok(()) };
        match &mut view.label {
            Some(label) => label.letter = letter,
            None => view.label = Some(ViewLabel::new(letter)),
        }
        self.view_letters = given;
        Ok(())
    }

    /// What view `id`'s label reads, a line for each part shown: its words and letter (`VIEW B`), its
    /// name, then its scale when that isn't its sheet's ([`Drawing::sheet_scale`]), written as the
    /// sheet's scale field writes it (`SCALE 2:1`). None when it draws no label: it has none, doesn't
    /// need one, is hidden, or shows no part.
    pub fn label_text(&self, id: u64) -> Option<String> {
        let view = self.view(id).filter(|view| view.needs_letter())?;
        let label = view.label.as_ref().filter(|label| label.shown)?;
        let after = |words: &str, what: String| if words.is_empty() { what } else { format!("{words} {what}") };
        let scale = self.resolve_view(id).ok().map(|resolved| resolved.scale.value).filter(|scale| self.sheet_scale(view.sheet) != Some(*scale));
        let lines: Vec<String> = [
            label.show_letter.then(|| after(&label.words, label.letter.clone())),
            Some(label.name.clone()).filter(|name| !name.is_empty()),
            scale.filter(|_| label.show_scale).map(|scale| after(&label.scale_words, scale.to_string())),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// The scale sheet `sheet`'s views draw at, as its scale field reads it: the one its base views
    /// share (a view at a scale of its own carries that in its label), else, with no base view, the
    /// one all its views draw at. None when they differ, or it has no view.
    pub fn sheet_scale(&self, sheet: u64) -> Option<Scale> {
        let on_sheet = || self.views.iter().filter(move |v| v.sheet == sheet);
        let mut base = on_sheet().filter(|v| matches!(v.kind, ViewKind::Base { .. })).peekable();
        if base.peek().is_some() {
            shared(base.map(|v| v.settings.scale))
        } else {
            shared(on_sheet().map(|v| self.resolve_view(v.id).ok().map(|r| r.scale.value)))
        }
    }
}

/// The one scale all of `scales` are, so neither the order the views were placed in nor their ids
/// decide it: none when there are none, or when one is unknown or differs.
fn shared(mut scales: impl Iterator<Item = Option<Scale>>) -> Option<Scale> {
    let first = scales.next()??;
    scales.all(|scale| scale == Some(first)).then_some(first)
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
