//! Drawing views of the session's model (W07, #5866): what a view shows and how far it reaches,
//! and where a saved view sits on its sheet (#4816). The snapshot command and saved views use it;
//! their pictures (W17b) will too.
//!
//! A view shows every body the design places, where its occurrences place it ([`shown`]). The
//! browser's eye counts for nothing: hiding a part in the model never changes a drawing.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::{MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, ModelBody, ModelState, ids};
use solidcraft_geom::drawing_view::{Scale, ViewFrame};
use solidcraft_geom::{Aabb2, EPS, Plane, Vec2};

use crate::params::bad;
use crate::{Result, Session};

#[path = "drawing_view_lines.rs"]
mod lines;

pub use lines::{CHORD_MM, CHORD_RANGE, Lines, Piece, VISIBLE_MM, ViewLine, lines, paper_group};

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
}

/// What a view shows, worked out once per request: the world state and each body it shows, in the
/// world state's order. The extents and the lines read it; choosing occurrences and hiding parts
/// (V4) and the reference split (V5) will filter it.
pub struct ShownSet {
    pub state: Arc<ModelState>,
    pub bodies: Vec<Shown>,
}

impl ShownSet {
    /// The shown body in place.
    pub fn body(&self, shown: &Shown) -> Option<&ModelBody> {
        self.state.bodies.get(shown.index)
    }
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
    let bodies: Vec<Shown> = state
        .bodies
        .iter()
        .enumerate()
        .map(|(index, b)| {
            let (path, body) = placed.remove(&b.name).unwrap_or_else(|| (Vec::new(), b.definition_name().to_string()));
            Shown { index, path, body }
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

/// The extents of the bodies `shown` holds, seen on `basis`, or that they have nothing to measure.
pub fn measure(shown: &ShownSet, basis: &Plane) -> std::result::Result<Extents, Unmeasured> {
    let bodies = shown.bodies.iter().filter_map(|b| shown.body(b));
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
/// space ([`solidcraft_doc::drawing::Sheet::drawing_space`]), sides included, with where it sits:
/// what a change of paper or a move to another sheet leaves outside. Nothing is moved.
pub fn outside(s: &Session, drawing: &Drawing, views: &[u64], cmd: &str) -> Result<Vec<(u64, OnSheet)>> {
    let mut out = Vec::new();
    for &id in views {
        let view = drawing.view(id).ok_or_else(|| bad(cmd, format!("no view {}", ids::encode(id))))?;
        let sheet = drawing.sheets.iter().find(|s| s.id == view.sheet).ok_or_else(|| bad(cmd, format!("no sheet {}", ids::encode(view.sheet))))?;
        // A view drawn up to the border, its sides on it, is inside: the margin takes rounding.
        let space = sheet.drawing_space().map_err(|e| bad(cmd, e.to_string()))?.inflated(EPS);
        let placed = on_sheet(s, drawing, id, cmd)?;
        if !(space.contains(placed.boundary.min) && space.contains(placed.boundary.max)) {
            out.push((id, placed));
        }
    }
    Ok(out)
}
