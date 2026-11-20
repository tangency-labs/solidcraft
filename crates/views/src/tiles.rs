//! How a window lays its views out side by side: a tree of splits whose leaves are view ids.
//! Pure data and small functions, so the layout menu, drag-to-split, the commands and the saved
//! form all read one [`Tiles`], and a front end only turns its rectangles into widgets.

use serde_json::{Value, json};

/// Most views one window shows at once.
pub const MAX_TILES: usize = 4;
/// Deepest nesting a saved layout may have (four tiles need three).
const MAX_DEPTH: usize = 4;
/// Ids beyond this are not exact in JSON numbers.
const MAX_ID: u64 = 1 << 53;
/// The smallest share of a split either side keeps.
const MIN_RATIO: f32 = 0.15;

/// A rectangle in the front end's own units (pixels), without any UI toolkit's type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// How a split places its two sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Side by side: the first on the left.
    Row,
    /// Stacked: the first on top.
    Column,
}

/// The edge of a view a new view is dropped on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// A place to put a view in the current window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Fill an empty cell in reading order.
    Empty(usize),
    /// Take an occupied cell, swapping if the moved view is already shown.
    Replace(u64),
    /// Take half of an occupied cell.
    Split { target: u64, edge: Edge },
}

/// A layout the Layout menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    Single,
    SideBySide,
    Stacked,
    LargeAndTwo,
    Grid,
}

impl Preset {
    /// Every preset, in the order the Layout menu lists them.
    pub const ALL: [Preset; 5] = [Preset::Single, Preset::SideBySide, Preset::Stacked, Preset::LargeAndTwo, Preset::Grid];

    /// The preset's stable key: its name in commands and in the saved form.
    pub fn key(self) -> &'static str {
        match self {
            Preset::Single => "single",
            Preset::SideBySide => "side_by_side",
            Preset::Stacked => "stacked",
            Preset::LargeAndTwo => "large_and_two",
            Preset::Grid => "grid",
        }
    }

    /// The preset with `key` ([`Self::key`]).
    pub fn from_key(key: &str) -> Option<Preset> {
        Self::ALL.into_iter().find(|preset| preset.key() == key)
    }

    /// How many views the preset shows.
    pub fn tiles(self) -> usize {
        match self {
            Preset::Single => 1,
            Preset::SideBySide | Preset::Stacked => 2,
            Preset::LargeAndTwo => 3,
            Preset::Grid => 4,
        }
    }
}

/// The boundary between the two sides of a split, for a front end to draw and drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splitter {
    /// Which split it is, for [`Tiles::set_ratio`]: stable while the layout's shape is.
    pub id: usize,
    pub axis: Axis,
    /// The gutter between the two sides.
    pub rect: Rect,
    /// The whole area the split divides, for turning a pointer position into a ratio.
    pub span: Rect,
}

/// A layout: one view, or two layouts split by an axis.
#[derive(Clone, Debug, PartialEq)]
pub enum Tiles {
    Leaf(u64),
    Split { axis: Axis, ratio: f32, first: Box<Tiles>, second: Box<Tiles> },
}

fn clamp_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() { ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO) } else { 0.5 }
}

fn split(axis: Axis, ratio: f32, first: Tiles, second: Tiles) -> Tiles {
    Tiles::Split { axis, ratio: clamp_ratio(ratio), first: Box::new(first), second: Box::new(second) }
}

/// The id a cell with no view in it carries: the window offers to choose what to show there.
pub const EMPTY: u64 = 0;

impl Tiles {
    /// A layout with the requested placement applied. The original is kept if a target is
    /// stale, the view was dropped on itself, or the move would exceed the window's tile limit.
    pub fn placed(&self, view: u64, placement: Placement) -> Option<Self> {
        if view == EMPTY {
            return None;
        }
        let mut next = self.clone();
        let shown = self.contains(view);
        let changed = match placement {
            Placement::Empty(ordinal) => {
                // Exchange with a temporary unique id so other empty cells keep their positions.
                const MARKER: u64 = u64::MAX;
                if view == MARKER || !next.fill_empty(ordinal, MARKER) {
                    return None;
                }
                next.replace_leaf(view, &mut |_| Tiles::Leaf(EMPTY));
                next.replace_leaf(MARKER, &mut |_| Tiles::Leaf(view))
            }
            Placement::Replace(target) if target != view && self.contains(target) => {
                if shown {
                    next.swap(view, target)
                } else {
                    next.replace_leaf(target, &mut |_| Tiles::Leaf(view))
                }
            }
            Placement::Split { target, edge } if target != view && self.contains(target) => {
                if shown {
                    next.remove(view);
                }
                next.split_at(target, view, edge)
            }
            _ => false,
        };
        changed.then_some(next)
    }

    /// Which menu preset has this split tree's shape. Splitter sizes and view ids may differ.
    pub fn preset_kind(&self) -> Option<Preset> {
        match self {
            Tiles::Leaf(_) => Some(Preset::Single),
            Tiles::Split { axis, first, second, .. } => match (axis, first.preset_kind(), second.preset_kind()) {
                (Axis::Row, Some(Preset::Single), Some(Preset::Single)) => Some(Preset::SideBySide),
                (Axis::Column, Some(Preset::Single), Some(Preset::Single)) => Some(Preset::Stacked),
                (Axis::Row, Some(Preset::Single), Some(Preset::Stacked)) => Some(Preset::LargeAndTwo),
                (Axis::Column, Some(Preset::SideBySide), Some(Preset::SideBySide)) => Some(Preset::Grid),
                _ => None,
            },
        }
    }

    /// A layout of exactly `preset` showing `views` in order, each place beyond them an empty
    /// cell ([`EMPTY`]). None without a view.
    pub fn preset_padded(preset: Preset, views: &[u64]) -> Option<Tiles> {
        if views.is_empty() {
            return None;
        }
        let leaf = |i: usize| Tiles::Leaf(views.get(i).copied().unwrap_or(EMPTY));
        Some(match preset {
            Preset::Single => leaf(0),
            Preset::SideBySide => split(Axis::Row, 0.5, leaf(0), leaf(1)),
            Preset::Stacked => split(Axis::Column, 0.5, leaf(0), leaf(1)),
            Preset::LargeAndTwo => split(Axis::Row, 0.62, leaf(0), split(Axis::Column, 0.5, leaf(1), leaf(2))),
            Preset::Grid => split(Axis::Column, 0.5, split(Axis::Row, 0.5, leaf(0), leaf(1)), split(Axis::Row, 0.5, leaf(2), leaf(3))),
        })
    }

    /// How many empty cells the layout has.
    pub fn empty_cells(&self) -> usize {
        match self {
            Tiles::Leaf(id) => usize::from(*id == EMPTY),
            Tiles::Split { first, second, .. } => first.empty_cells() + second.empty_cells(),
        }
    }

    /// Show `view` in the empty cell number `ordinal` (in reading order). False when there is no
    /// such cell or `view` is shown already.
    pub fn fill_empty(&mut self, ordinal: usize, view: u64) -> bool {
        if view == EMPTY || self.contains(view) {
            return false;
        }
        self.nth_empty(ordinal).map(|leaf| *leaf = Tiles::Leaf(view)).is_some()
    }

    /// Close the empty cell number `ordinal`: its neighbour takes the space. False when there is
    /// no such cell or it is the only one.
    pub fn remove_empty(&mut self, ordinal: usize) -> bool {
        const GONE: u64 = u64::MAX;
        if self.len() < 2 {
            return false;
        }
        if self.nth_empty(ordinal).map(|leaf| *leaf = Tiles::Leaf(GONE)).is_none() {
            return false;
        }
        self.remove_leaf(GONE);
        true
    }

    fn nth_empty(&mut self, ordinal: usize) -> Option<&mut Tiles> {
        match self {
            Tiles::Leaf(id) if *id == EMPTY && ordinal == 0 => Some(self),
            Tiles::Leaf(_) => None,
            Tiles::Split { first, second, .. } => {
                let before = first.empty_cells();
                if ordinal < before { first.nth_empty(ordinal) } else { second.nth_empty(ordinal - before) }
            }
        }
    }

    /// A layout of `preset` showing `views` in order. With fewer views than the preset shows, the
    /// largest preset that they fill; no views at all is not a layout (None).
    pub fn preset(preset: Preset, views: &[u64]) -> Option<Tiles> {
        let leaf = |i: usize| views.get(i).copied().map(Tiles::Leaf);
        let preset = if views.len() >= preset.tiles() {
            preset
        } else {
            match views.len() {
                0 => return None,
                1 => Preset::Single,
                2 => match preset {
                    Preset::Stacked => Preset::Stacked,
                    _ => Preset::SideBySide,
                },
                _ => Preset::LargeAndTwo,
            }
        };
        Some(match preset {
            Preset::Single => leaf(0)?,
            Preset::SideBySide => split(Axis::Row, 0.5, leaf(0)?, leaf(1)?),
            Preset::Stacked => split(Axis::Column, 0.5, leaf(0)?, leaf(1)?),
            Preset::LargeAndTwo => split(Axis::Row, 0.62, leaf(0)?, split(Axis::Column, 0.5, leaf(1)?, leaf(2)?)),
            Preset::Grid => split(Axis::Column, 0.5, split(Axis::Row, 0.5, leaf(0)?, leaf(1)?), split(Axis::Row, 0.5, leaf(2)?, leaf(3)?)),
        })
    }

    /// The views shown, in reading order (left to right, top to bottom).
    pub fn views(&self) -> Vec<u64> {
        match self {
            Tiles::Leaf(id) if *id == EMPTY => Vec::new(),
            Tiles::Leaf(id) => vec![*id],
            Tiles::Split { first, second, .. } => first.views().into_iter().chain(second.views()).collect(),
        }
    }

    /// How many cells there are: the views shown and the empty cells.
    pub fn len(&self) -> usize {
        match self {
            Tiles::Leaf(_) => 1,
            Tiles::Split { first, second, .. } => first.len() + second.len(),
        }
    }

    /// Always false: a layout shows at least one view.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether `view` is shown.
    pub fn contains(&self, view: u64) -> bool {
        match self {
            Tiles::Leaf(id) => *id == view && view != EMPTY,
            Tiles::Split { first, second, .. } => first.contains(view) || second.contains(view),
        }
    }

    /// Where each cell goes in `area`, with `gutter` between neighbours: a view's id, or [`EMPTY`].
    pub fn rects(&self, area: Rect, gutter: f32) -> Vec<(u64, Rect)> {
        match self {
            Tiles::Leaf(id) => vec![(*id, area)],
            Tiles::Split { axis, ratio, first, second } => {
                let (a, _, b) = divide(area, *axis, *ratio, gutter);
                first.rects(a, gutter).into_iter().chain(second.rects(b, gutter)).collect()
            }
        }
    }

    /// The boundaries between views in `area`, to draw and drag.
    pub fn splitters(&self, area: Rect, gutter: f32) -> Vec<Splitter> {
        let mut out = Vec::new();
        self.collect_splitters(area, gutter, &mut out);
        out
    }

    fn collect_splitters(&self, area: Rect, gutter: f32, out: &mut Vec<Splitter>) {
        if let Tiles::Split { axis, ratio, first, second } = self {
            let (a, gap, b) = divide(area, *axis, *ratio, gutter);
            out.push(Splitter { id: out.len(), axis: *axis, rect: gap, span: area });
            first.collect_splitters(a, gutter, out);
            second.collect_splitters(b, gutter, out);
        }
    }

    /// Give the split `id` ([`Splitter::id`]) the share `ratio` for its first side (kept between
    /// a sixth and five sixths). False when there is no such split.
    pub fn set_ratio(&mut self, id: usize, ratio: f32) -> bool {
        let mut next = 0;
        self.set_ratio_at(id, &mut next, ratio)
    }

    fn set_ratio_at(&mut self, id: usize, next: &mut usize, to: f32) -> bool {
        let Tiles::Split { ratio, first, second, .. } = self else { return false };
        let mine = *next;
        *next += 1;
        if mine == id {
            *ratio = clamp_ratio(to);
            return true;
        }
        first.set_ratio_at(id, next, to) || second.set_ratio_at(id, next, to)
    }

    /// Show `view` beside `target`, on its `edge`. False when `view` is shown already, `target`
    /// is not, or the window already shows [`MAX_TILES`] views.
    pub fn split_at(&mut self, target: u64, view: u64, edge: Edge) -> bool {
        if self.len() >= MAX_TILES || self.contains(view) || !self.contains(target) {
            return false;
        }
        self.replace_leaf(target, &mut |old| {
            let new = Tiles::Leaf(view);
            match edge {
                Edge::Left => split(Axis::Row, 0.5, new, old),
                Edge::Right => split(Axis::Row, 0.5, old, new),
                Edge::Top => split(Axis::Column, 0.5, new, old),
                Edge::Bottom => split(Axis::Column, 0.5, old, new),
            }
        })
    }

    fn replace_leaf(&mut self, target: u64, make: &mut impl FnMut(Tiles) -> Tiles) -> bool {
        match self {
            Tiles::Leaf(id) if *id == target => {
                *self = make(Tiles::Leaf(target));
                true
            }
            Tiles::Leaf(_) => false,
            Tiles::Split { first, second, .. } => first.replace_leaf(target, make) || second.replace_leaf(target, make),
        }
    }

    /// Swap the places of two views. False unless both are shown.
    pub fn swap(&mut self, a: u64, b: u64) -> bool {
        if a == b || !self.contains(a) || !self.contains(b) {
            return false;
        }
        self.swap_leaves(a, b);
        true
    }

    fn swap_leaves(&mut self, a: u64, b: u64) {
        match self {
            Tiles::Leaf(id) if *id == a => *id = b,
            Tiles::Leaf(id) if *id == b => *id = a,
            Tiles::Leaf(_) => {}
            Tiles::Split { first, second, .. } => {
                first.swap_leaves(a, b);
                second.swap_leaves(a, b);
            }
        }
    }

    /// Stop showing `view`: its neighbour takes its space. False when it is not shown or is the
    /// only one.
    pub fn remove(&mut self, view: u64) -> bool {
        if self.len() < 2 || !self.contains(view) {
            return false;
        }
        self.remove_leaf(view);
        true
    }

    fn remove_leaf(&mut self, view: u64) {
        let Tiles::Split { first, second, .. } = self else { return };
        if matches!(**first, Tiles::Leaf(id) if id == view) {
            *self = std::mem::replace(&mut **second, Tiles::Leaf(0));
        } else if matches!(**second, Tiles::Leaf(id) if id == view) {
            *self = std::mem::replace(&mut **first, Tiles::Leaf(0));
        } else {
            first.remove_leaf(view);
            second.remove_leaf(view);
        }
    }

    /// The saved form.
    pub fn to_json(&self) -> Value {
        match self {
            Tiles::Leaf(id) if *id == EMPTY => json!({"empty": true}),
            Tiles::Leaf(id) => json!({"view": id}),
            Tiles::Split { axis, ratio, first, second } => json!({
                "axis": if *axis == Axis::Row { "row" } else { "column" },
                "ratio": ratio,
                "first": first.to_json(),
                "second": second.to_json(),
            }),
        }
    }

    /// A layout from [`Self::to_json`]. Anything else, a view shown twice, too many views, or
    /// nesting deeper than a window needs gives None.
    pub fn from_json(value: &Value) -> Option<Tiles> {
        let tiles = Self::read(value, 0)?;
        let views = tiles.views();
        let mut sorted = views.clone();
        sorted.sort_unstable();
        sorted.dedup();
        (tiles.len() <= MAX_TILES && !views.is_empty() && sorted.len() == views.len()).then_some(tiles)
    }

    fn read(value: &Value, depth: usize) -> Option<Tiles> {
        if depth > MAX_DEPTH {
            return None;
        }
        if value.get("empty").and_then(Value::as_bool) == Some(true) {
            return Some(Tiles::Leaf(EMPTY));
        }
        if let Some(id) = value.get("view") {
            return id.as_u64().filter(|id| (1..=MAX_ID).contains(id)).map(Tiles::Leaf);
        }
        let axis = match value.get("axis")?.as_str()? {
            "row" => Axis::Row,
            "column" => Axis::Column,
            _ => return None,
        };
        let ratio = value.get("ratio")?.as_f64()? as f32;
        Some(split(axis, ratio, Self::read(value.get("first")?, depth + 1)?, Self::read(value.get("second")?, depth + 1)?))
    }
}

/// `area` cut by an axis into the first side, the gutter and the second side.
fn divide(area: Rect, axis: Axis, ratio: f32, gutter: f32) -> (Rect, Rect, Rect) {
    match axis {
        Axis::Row => {
            let room = (area.w - gutter).max(0.0);
            let first = (room * ratio).floor();
            (Rect { w: first, ..area }, Rect { x: area.x + first, w: gutter, ..area }, Rect { x: area.x + first + gutter, w: room - first, ..area })
        }
        Axis::Column => {
            let room = (area.h - gutter).max(0.0);
            let first = (room * ratio).floor();
            (Rect { h: first, ..area }, Rect { y: area.y + first, h: gutter, ..area }, Rect { y: area.y + first + gutter, h: room - first, ..area })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0.0, y: 0.0, w: 1006.0, h: 506.0 };

    fn grid() -> Tiles {
        Tiles::preset(Preset::Grid, &[1, 2, 3, 4]).unwrap()
    }

    #[test]
    fn placement_splits_swaps_replaces_and_preserves_empty_cells() {
        let initial = Tiles::Leaf(1);
        let split = initial.placed(2, Placement::Split { target: 1, edge: Edge::Left }).unwrap();
        assert_eq!(split.views(), [2, 1]);
        assert_eq!(split.preset_kind(), Some(Preset::SideBySide));
        assert_eq!(split.placed(2, Placement::Replace(1)).unwrap().views(), [1, 2]);
        assert_eq!(split.placed(3, Placement::Replace(2)).unwrap().views(), [3, 1]);

        let padded = Tiles::preset_padded(Preset::Grid, &[1, 2]).unwrap();
        let swap = padded.placed(1, Placement::Empty(1)).unwrap();
        assert_eq!(swap.rects(AREA, 6.0).into_iter().map(|(id, _)| id).collect::<Vec<_>>(), [EMPTY, 2, EMPTY, 1]);
        assert_eq!(Tiles::from_json(&swap.to_json()), Some(swap));
        assert!(padded.placed(3, Placement::Split { target: 1, edge: Edge::Bottom }).is_none());
        assert!(grid().placed(1, Placement::Split { target: 4, edge: Edge::Bottom }).is_some());
        assert!(padded.placed(1, Placement::Replace(1)).is_none());
        assert!(padded.placed(1, Placement::Empty(2)).is_none());
    }

    /// Every preset shows its views in reading order, fills the area without overlap, and a preset
    /// asked for more than the views there are becomes the largest one they fill.
    #[test]
    fn presets_fill_the_area_and_shrink_to_the_views_there_are() {
        for preset in Preset::ALL {
            let views: Vec<u64> = (1..=preset.tiles() as u64).collect();
            let tiles = Tiles::preset(preset, &views).unwrap();
            assert_eq!(tiles.views(), views, "{preset:?}");
            let rects = tiles.rects(AREA, 6.0);
            for (i, (_, a)) in rects.iter().enumerate() {
                assert!(a.x >= 0.0 && a.y >= 0.0 && a.x + a.w <= AREA.w && a.y + a.h <= AREA.h, "{preset:?} stays inside");
                for (_, b) in &rects[i + 1..] {
                    let apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                    assert!(apart, "{preset:?}: {a:?} overlaps {b:?}");
                }
            }
            assert_eq!(Preset::from_key(preset.key()), Some(preset));
        }
        assert_eq!(Tiles::preset(Preset::Grid, &[7]), Some(Tiles::Leaf(7)));
        assert_eq!(Tiles::preset(Preset::Grid, &[7, 8]).unwrap().len(), 2);
        assert_eq!(Tiles::preset(Preset::Grid, &[7, 8, 9]).unwrap().len(), 3);
        assert_eq!(Tiles::preset(Preset::Stacked, &[7, 8]).unwrap().rects(AREA, 6.0)[0].1.w, AREA.w);
        assert_eq!(Tiles::preset(Preset::Single, &[]), None);
    }

    /// A split takes a gutter out of the area and the splitter is that gutter.
    #[test]
    fn splitters_are_the_gutters_between_views() {
        let tiles = Tiles::preset(Preset::SideBySide, &[1, 2]).unwrap();
        let rects = tiles.rects(AREA, 6.0);
        let splitters = tiles.splitters(AREA, 6.0);
        assert_eq!(splitters.len(), 1);
        let splitter = splitters[0];
        assert_eq!(splitter.rect.x, rects[0].1.x + rects[0].1.w);
        assert_eq!(splitter.rect.x + splitter.rect.w, rects[1].1.x);
        assert_eq!((splitter.axis, splitter.span), (Axis::Row, AREA));
        assert_eq!(grid().splitters(AREA, 6.0).len(), 3);
    }

    /// Dragging a splitter moves only that split, and never past the smallest share.
    #[test]
    fn a_splitter_moves_its_own_split_within_limits() {
        let mut tiles = grid();
        let before: Vec<f32> = tiles.rects(AREA, 6.0).iter().map(|(_, r)| r.w).collect();
        assert!(tiles.set_ratio(1, 0.7));
        let after: Vec<f32> = tiles.rects(AREA, 6.0).iter().map(|(_, r)| r.w).collect();
        assert_ne!(before[0], after[0]);
        assert_eq!((before[2], before[3]), (after[2], after[3]), "the other row did not move");
        assert!(tiles.set_ratio(0, 0.0));
        assert!(tiles.set_ratio(0, f32::NAN));
        assert!(!tiles.set_ratio(9, 0.5));
        assert!(matches!(&tiles, Tiles::Split { ratio, .. } if *ratio == 0.5), "a ratio that is not a number is half");
    }

    /// A view dropped on an edge takes that side of the view it lands on, up to four, once each.
    #[test]
    fn dropping_a_view_on_an_edge_splits_the_view_under_it() {
        let mut tiles = Tiles::Leaf(1);
        assert!(tiles.split_at(1, 2, Edge::Right));
        assert_eq!(tiles.views(), [1, 2]);
        assert!(tiles.split_at(1, 3, Edge::Left));
        assert_eq!(tiles.views(), [3, 1, 2]);
        assert!(tiles.split_at(2, 4, Edge::Bottom));
        assert_eq!(tiles.views(), [3, 1, 2, 4]);
        assert!(!tiles.split_at(2, 5, Edge::Top), "four is the most");
        let mut two = Tiles::preset(Preset::Stacked, &[1, 2]).unwrap();
        assert!(!two.split_at(1, 2, Edge::Left), "a view is shown once");
        assert!(!two.split_at(9, 3, Edge::Left), "the target must be shown");
        assert!(two.split_at(2, 3, Edge::Top));
        assert_eq!(two.views(), [1, 3, 2]);
    }

    /// Swapping exchanges two places; removing gives a view's space to its neighbour, never
    /// leaving the window empty.
    #[test]
    fn views_swap_and_leave() {
        let mut tiles = grid();
        assert!(tiles.swap(1, 4));
        assert_eq!(tiles.views(), [4, 2, 3, 1]);
        assert!(!tiles.swap(1, 1) && !tiles.swap(1, 9));
        assert!(tiles.remove(2));
        assert_eq!(tiles.views(), [4, 3, 1]);
        assert!(tiles.remove(3) && tiles.remove(4));
        assert_eq!(tiles, Tiles::Leaf(1));
        assert!(!tiles.remove(1), "the last view stays");
        assert!(!grid().remove(9));
    }

    /// A saved layout reads back as it was; one that breaks the rules is refused.
    #[test]
    fn the_saved_form_round_trips_and_refuses_what_breaks_the_rules() {
        let mut tiles = grid();
        tiles.set_ratio(0, 0.4);
        assert_eq!(Tiles::from_json(&tiles.to_json()), Some(tiles));
        assert_eq!(Tiles::from_json(&json!({"view": 0})), None);
        assert_eq!(Tiles::from_json(&json!({"view": 1.5})), None);
        assert_eq!(Tiles::from_json(&json!({"axis": "row", "ratio": 0.5, "first": {"view": 1}, "second": {"view": 1}})), None, "a view twice");
        assert_eq!(Tiles::from_json(&json!({"axis": "diagonal", "ratio": 0.5, "first": {"view": 1}, "second": {"view": 2}})), None);
        assert_eq!(Tiles::from_json(&json!("grid")), None);
        let five = json!({"axis": "row", "ratio": 0.5, "first": {"view": 1}, "second": {"axis": "row", "ratio": 0.5, "first": {"view": 2},
            "second": {"axis": "row", "ratio": 0.5, "first": {"view": 3}, "second": {"axis": "row", "ratio": 0.5, "first": {"view": 4}, "second": {"view": 5}}}}});
        assert_eq!(Tiles::from_json(&five), None, "five views");
        let mut deep = json!({"view": 1});
        for _ in 0..40 {
            deep = json!({"axis": "row", "ratio": 0.5, "first": deep, "second": {"view": 2}});
        }
        assert_eq!(Tiles::from_json(&deep), None, "nesting is bounded");
        let wild = Tiles::from_json(&json!({"axis": "row", "ratio": 1.0e30, "first": {"view": 1}, "second": {"view": 2}})).unwrap();
        assert!(matches!(wild, Tiles::Split { ratio, .. } if ratio == 1.0 - MIN_RATIO), "a ratio out of range is held to the limit");
    }

    #[test]
    fn an_empty_cell_is_filled_or_closed_and_is_saved() {
        let mut tiles = Tiles::preset_padded(Preset::Grid, &[1, 2]).unwrap();
        assert_eq!((tiles.len(), tiles.views(), tiles.empty_cells()), (4, vec![1, 2], 2));
        assert!(!tiles.contains(EMPTY));
        assert_eq!(Tiles::from_json(&tiles.to_json()), Some(tiles.clone()));
        assert!(!tiles.fill_empty(0, 2), "a view shown already");
        assert!(tiles.fill_empty(1, 7), "the second empty cell, in reading order");
        assert_eq!(tiles.views(), vec![1, 2, 7]);
        assert!(!tiles.fill_empty(1, 8), "there is one empty cell left, number 0");
        assert!(tiles.remove_empty(0));
        assert_eq!((tiles.len(), tiles.empty_cells()), (3, 0));
        assert!(!tiles.remove_empty(0));
        assert_eq!(Tiles::preset_padded(Preset::Grid, &[]), None);
    }
}
