//! Drawing views of the session's model (W07, #5866): what a view shows and how far it reaches,
//! and where a saved view sits on its sheet (#4816). The snapshot command and saved views use it;
//! their pictures (W17b) will too.
//!
//! A view shows every body the design places, where its occurrences place it ([`shown`]). The
//! browser's eye counts for nothing: hiding a part in the model never changes a drawing. A body
//! placed through a reference occurrence is a reference part: it doesn't set the view's extents
//! when model parts are there too ([`measure`]).

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::Arc;

use solidcraft_doc::drawing::{Drawing, Sheet};
use solidcraft_doc::{MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, ModelBody, ModelState, ids};
use solidcraft_geom::drawing_view::{Scale, ViewFrame};
use solidcraft_geom::{Aabb2, EPS, Plane, Vec2};
use solidcraft_render::VisualStyle;

use crate::params::bad;
use crate::{Result, Session};

#[path = "drawing_view_lines.rs"]
mod lines;

pub use lines::{
    CHORD_MM, CHORD_RANGE, LineRequest, Lines, MIN_MODEL_CHORD_MM, Piece, REFERENCE_MM, Stats, VISIBLE_MM, ViewLine, lines, paper_group,
};

/// The paper millimetres each way of a view's boundary when there's nothing to measure, round its
/// centre, so it can still be picked, moved and deleted.
pub const PLACEHOLDER_MM: f64 = 20.0;

/// How far a view reaches on its own plane, in model millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extents {
    /// The bodies' box in the plane's own coordinates ([`Plane::to_local`]).
    pub model: Aabb2,
    /// How far a curved face may lie outside its display mesh: the largest display tolerance.
    pub error_mm: f64,
    pub bodies: usize,
}

/// Why a view has nothing to measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmeasured {
    /// The assembly places more than a view walks, so a view of it would leave parts out.
    PastWalkLimit,
    NoBodies,
}

impl Unmeasured {
    /// Its name in replies.
    pub fn name(self) -> &'static str {
        match self {
            Unmeasured::PastWalkLimit => "past_walk_limit",
            Unmeasured::NoBodies => "no_bodies",
        }
    }
}

impl fmt::Display for Unmeasured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unmeasured::PastWalkLimit => write!(
                f,
                "the assembly is deeper than {MAX_PLACEMENT_DEPTH} levels or has more than {MAX_PLACEMENTS} placements: a view of it would leave parts out"
            ),
            Unmeasured::NoBodies => f.write_str("the view's source has no bodies"),
        }
    }
}

/// One body a view shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// Its index among the world state's bodies: the body in place, with its mesh.
    pub index: usize,
    /// The occurrences that place it, from the design's root.
    pub path: Vec<u64>,
    /// Its definition's body name.
    pub body: String,
    /// It is placed through a reference occurrence ([`solidcraft_doc::Document::reference_occurrences`]).
    pub reference: bool,
}

/// What a view shows, worked out once per request: the world state and each body it shows, in the
/// world state's order. The extents and the lines read it; choosing occurrences and hiding parts
/// (V4) will filter it.
pub struct ShownSet {
    pub state: Arc<ModelState>,
    pub bodies: Vec<Shown>,
}

/// Occurrence paths a view's `source` or `hide` may hold, each.
pub const MAX_PATHS: usize = 1_000;
/// Ids a view's `source` and `hide` may hold in all.
pub const MAX_PATH_IDS: usize = 4_096;

impl ShownSet {
    /// The shown body in place.
    pub fn body(&self, shown: &Shown) -> Option<&ModelBody> {
        self.state.bodies.get(shown.index)
    }

    /// The bodies under one of the `source` occurrence paths (all of them when there is none;
    /// `[]` takes the design's own bodies) and under no `hide` path, in the same order.
    pub fn keep(mut self, source: Option<&[Vec<u64>]>, hide: &[Vec<u64>]) -> ShownSet {
        let source: Option<HashSet<&[u64]>> = source.map(|paths| paths.iter().map(Vec::as_slice).collect());
        let hide: HashSet<&[u64]> = hide.iter().map(Vec::as_slice).collect();
        self.bodies.retain(|b| chosen(&b.path, source.as_ref(), &hide));
        self
    }
}

/// Whether a body placed by `path` is drawn: under a `source` path (`[]` only for the design's own
/// bodies) and under no `hide` path. Each of the path's heads is one look-up, so a body costs at
/// most 64 of them however many paths there are.
fn chosen(path: &[u64], source: Option<&HashSet<&[u64]>>, hide: &HashSet<&[u64]>) -> bool {
    let heads = || (1..=path.len()).filter_map(|n| path.get(..n));
    let wanted = source.is_none_or(|set| if path.is_empty() { set.contains(path) } else { heads().any(|h| set.contains(h)) });
    wanted && !heads().any(|h| hide.contains(h))
}

/// Every body the design places, or why there's nothing to show: the assembly places more than a
/// view walks, or it has no bodies.
pub fn shown(s: &Session) -> std::result::Result<ShownSet, Unmeasured> {
    if !s.doc.placements_complete() {
        return Err(Unmeasured::PastWalkLimit);
    }
    let state = s.world_state();
    // The world state names each placed body as this walk does, so its name finds its path.
    let mut placed: BTreeMap<String, (Vec<u64>, String)> =
        s.doc.body_frames_with_source(&s.model.state()).into_iter().map(|(name, body, _, path, _)| (name, (path, body))).collect();
    let references = s.doc.reference_occurrences();
    let bodies: Vec<Shown> = state
        .bodies
        .iter()
        .enumerate()
        .map(|(index, b)| {
            let (path, body) = placed.remove(&b.name).unwrap_or_else(|| (Vec::new(), b.definition_name().to_string()));
            let reference = path.iter().any(|occurrence| references.contains(occurrence));
            Shown { index, path, body, reference }
        })
        .collect();
    if bodies.is_empty() { Err(Unmeasured::NoBodies) } else { Ok(ShownSet { state, bodies }) }
}

/// A saved view on its sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnSheet {
    /// Its box on the sheet, in paper millimetres.
    pub boundary: Aabb2,
    /// Its design's extents on its plane; with nothing to measure, the placeholder's box there,
    /// [`PLACEHOLDER_MM`] each way on paper round the plane's origin.
    pub extents: Extents,
    /// Why there's nothing to measure, when there isn't.
    pub unmeasured: Option<Unmeasured>,
}

/// Why a view drawing in `display` and `reference` styles can't be drawn yet, if it can't: shaded
/// views need pictures, and a paper scene (`scene`) has no dashed lines for hidden ones, of the
/// model or of reference parts. The snapshot and the sheet scene refuse alike.
pub fn not_drawable(display: VisualStyle, reference: VisualStyle, scene: bool) -> Option<&'static str> {
    if display.is_shaded() || reference.is_shaded() {
        Some("not supported yet: shaded views need the drawing renderer (#5624)")
    } else if scene && (display == VisualStyle::VisibleHidden || reference == VisualStyle::VisibleHidden) {
        Some("not supported yet: hidden lines in the paper scene need dashed lines (#4890 A5)")
    } else {
        None
    }
}

/// The extents of the bodies `shown` holds, seen on `basis`, or that they have nothing to measure:
/// of its model parts when it has any, as a reference part only shows around them, else of its
/// reference parts.
pub fn measure(shown: &ShownSet, basis: &Plane) -> std::result::Result<Extents, Unmeasured> {
    let has_model = shown.bodies.iter().any(|b| !b.reference);
    let bodies = shown.bodies.iter().filter(|b| !(has_model && b.reference)).filter_map(|b| shown.body(b));
    let extents = bodies.fold(Extents { model: Aabb2::EMPTY, error_mm: 0.0, bodies: 0 }, |e, body| Extents {
        model: e.model.union(&body.mesh_bounds_in(basis)),
        error_mm: e.error_mm.max(body.display_tol()),
        bodies: e.bodies + 1,
    });
    if extents.model.is_empty() { Err(Unmeasured::NoBodies) } else { Ok(extents) }
}

/// The view through `basis` at `scale` on paper, with the middle of `shown` (model mm on its
/// plane) at `centre`: the one rule the snapshot and saved views are placed by.
pub fn frame(basis: Plane, scale: Scale, shown: &Aabb2, centre: Vec2, cmd: &str) -> Result<ViewFrame> {
    ViewFrame::new(basis, scale, shown.center(), centre).map_err(|e| bad(cmd, e.to_string()))
}

/// The boundary on the sheet of [`frame`]'s view: the box round `shown` there.
pub fn boundary(basis: Plane, scale: Scale, shown: &Aabb2, centre: Vec2, cmd: &str) -> Result<Aabb2> {
    frame(basis, scale, shown, centre, cmd)?.boundary(shown).map_err(|e| bad(cmd, e.to_string()))
}

/// Saved view `id` of `drawing`, one of `s`'s design's drawings, on its sheet: the design's extents
/// on its plane (a view's source is the design the drawing is in), grown by its reference margin,
/// at its scale, with their middle on its centre ([`Drawing::view_centre`]); with nothing to
/// measure, a placeholder [`PLACEHOLDER_MM`] each way round its centre. It reads the model as it
/// is evaluated: no model is evaluated.
pub fn on_sheet(s: &Session, drawing: &Drawing, id: u64, cmd: &str) -> Result<OnSheet> {
    let view = drawing.resolve_view(id).map_err(|e| bad(cmd, e.to_string()))?;
    let centre = drawing.view_centre(id).map_err(|e| bad(cmd, e.to_string()))?;
    let scale = view.scale.value;
    let (extents, shown, unmeasured) = match shown(s).and_then(|set| measure(&set, &view.basis)) {
        Ok(extents) => (extents, extents.model.inflated(view.reference_margin_mm), None),
        Err(why) => {
            let half = PLACEHOLDER_MM / 2.0 / scale.factor();
            let model = Aabb2 { min: Vec2::new(-half, -half), max: Vec2::new(half, half) };
            (Extents { model, error_mm: 0.0, bodies: 0 }, model, Some(why))
        }
    };
    Ok(OnSheet { boundary: boundary(view.basis, scale, &shown, centre, cmd)?, extents, unmeasured })
}

/// Each of `views` of `drawing` (one of `s`'s design's) whose boundary leaves its sheet's drawing
/// space ([`Sheet::drawing_space`]), sides included, with where it sits:
/// what a change of paper or a move to another sheet leaves outside. Nothing is moved.
pub fn outside(s: &Session, drawing: &Drawing, views: &[u64], cmd: &str) -> Result<Vec<(u64, OnSheet)>> {
    let mut out = Vec::new();
    for &id in views {
        let view = drawing.view(id).ok_or_else(|| bad(cmd, format!("no view {}", ids::encode(id))))?;
        let placed = on_sheet(s, drawing, id, cmd)?;
        if leaves(sheet(drawing, view.sheet, cmd)?, &placed, cmd)? {
            out.push((id, placed));
        }
    }
    Ok(out)
}

/// Sheet `id` of `drawing`.
pub fn sheet<'a>(drawing: &'a Drawing, id: u64, cmd: &str) -> Result<&'a Sheet> {
    drawing.sheets.iter().find(|s| s.id == id).map(|s| &**s).ok_or_else(|| bad(cmd, format!("no sheet {}", ids::encode(id))))
}

/// Whether a view `placed` on `sheet` leaves its drawing space
/// ([`Sheet::drawing_space`]), sides included: [`outside`]'s rule.
pub fn leaves(sheet: &Sheet, placed: &OnSheet, cmd: &str) -> Result<bool> {
    // A view drawn up to the border, its sides on it, is inside: the margin takes rounding.
    let space = sheet.drawing_space().map_err(|e| bad(cmd, e.to_string()))?.inflated(EPS);
    Ok(!(space.contains(placed.boundary.min) && space.contains(placed.boundary.max)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `source` path takes everything under it, `[]` only the design's own bodies, and a `hide`
    /// path wins over a `source` path above it.
    #[test]
    fn a_body_is_drawn_under_a_source_path_and_no_hide_path() {
        let set = |paths: &[&'static [u64]]| paths.iter().copied().collect::<HashSet<&[u64]>>();
        let paths: [&[u64]; 5] = [&[], &[1], &[1, 2], &[1, 2, 3], &[4]];
        let cases: [(Option<HashSet<&[u64]>>, HashSet<&[u64]>, [bool; 5]); 4] = [
            (None, set(&[&[1, 2]]), [true, true, false, false, true]),
            (Some(set(&[&[1, 2]])), set(&[]), [false, false, true, true, false]),
            (Some(set(&[&[]])), set(&[]), [true, false, false, false, false]),
            (Some(set(&[&[1], &[4]])), set(&[&[1, 2]]), [false, true, false, false, true]),
        ];
        for (source, hide, want) in cases {
            let got = paths.map(|p| chosen(p, source.as_ref(), &hide));
            assert_eq!(got, want, "{source:?} {hide:?}");
        }
    }

    /// Seeded paths and choices: `chosen` always agrees with the rule worked out the long way.
    #[test]
    fn seeded_choices_agree_with_the_rule() {
        let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
        let path = |seed: u64, salt: u64| -> Vec<u64> { (0..pick(seed, salt, 4)).map(|i| 1 + pick(seed, salt + 10 + i, 3)).collect() };
        for seed in 1..=500u64 {
            let sources: Vec<Vec<u64>> = (0..pick(seed, 1, 3)).map(|i| path(seed, 100 + i * 20)).collect();
            let hides: Vec<Vec<u64>> = (0..pick(seed, 2, 3)).map(|i| path(seed, 200 + i * 20)).filter(|p| !p.is_empty()).collect();
            let body = path(seed, 3);
            let source = (pick(seed, 4, 2) == 0).then(|| sources.iter().map(Vec::as_slice).collect::<HashSet<&[u64]>>());
            let hide: HashSet<&[u64]> = hides.iter().map(Vec::as_slice).collect();
            let under = |p: &Vec<u64>| if p.is_empty() { body.is_empty() } else { body.starts_with(p) };
            let long_way = (source.is_none() || sources.iter().any(under)) && !hides.iter().any(under);
            assert_eq!(chosen(&body, source.as_ref(), &hide), long_way, "seed {seed}: {body:?} {sources:?} {hides:?}");
        }
    }
}
