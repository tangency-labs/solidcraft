//! Detail views as geometry (W35a, #9718, part of #860): a region of a parent view, drawn again
//! at its own scale somewhere else on the sheet. Nothing here clips, draws or saves; shared
//! clipping is the paper scene's (#4890, A8), and saved views and commands come later.
//!
//! - A **[`Boundary`]** is a circle or a rectangle in the parent view's plane coordinates, the
//!   coordinates of [`ViewFrame::paper_map`] (and of the curves the view's producers give).
//! - A **[`Detail`]** places that region: its [`Detail::frame`] is a [`ViewFrame`] with the
//!   parent's basis and the detail's own [`Scale`], the boundary's centre at a chosen paper
//!   position. A curve of the parent's plane goes onto the detail with [`ViewFrame::seg_to_paper`].
//! - On the parent, the boundary is drawn as its [`Detail::outline`]; on the detail, it is the
//!   [`Detail::window`], the shape the detail's geometry is cut to.
//! - A **[`Label`]** names the detail (`A`, `B`, …, `AA`, …), next to the outline on the parent
//!   ([`Detail::label_anchor`]). [`next_label`] skips `I`, `O` and `Q`, which read as digits.

use std::collections::HashSet;
use std::f64::consts::PI;
use std::fmt;

use crate::drawing_view::{Invalid, Scale, ViewFrame, is_paper_coordinate, require};
use crate::paper::MAX_SIDE_MM;
use crate::section::MAX_NUMBER;
use crate::{Aabb2, Seg2, Similarity2, Vec2};

type Result<T> = std::result::Result<T, Invalid>;

/// The smallest a detail's window may be across on its paper, in millimetres: below that the
/// detail shows nothing a reader could see.
pub const MIN_WINDOW_MM: f64 = 0.01;

/// The most letters a detail label has: `ZZZ` is the last one.
pub const MAX_LABEL_LETTERS: usize = 3;

/// The letters [`next_label`] uses, in order: the alphabet without `I`, `O` and `Q`.
const LETTERS: &[u8] = b"ABCDEFGHJKLMNPRSTUVWXYZ";

/// A circle or a rectangle with its sides along x and y, in whatever coordinates its owner says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Circle { centre: Vec2, radius: f64 },
    Rectangle(Aabb2),
}

impl Shape {
    pub fn centre(&self) -> Vec2 {
        match self {
            Shape::Circle { centre, .. } => *centre,
            Shape::Rectangle(b) => b.center(),
        }
    }

    /// The box around the shape.
    pub fn bounds(&self) -> Aabb2 {
        match *self {
            Shape::Circle { centre, radius } => {
                let r = Vec2::new(radius, radius);
                Aabb2 { min: centre - r, max: centre + r }
            }
            Shape::Rectangle(b) => b,
        }
    }

    /// Whether `p` is inside the shape or on its edge.
    pub fn contains(&self, p: Vec2) -> bool {
        match *self {
            Shape::Circle { centre, radius } => (p - centre).len() <= radius,
            Shape::Rectangle(b) => b.contains(p),
        }
    }

    /// The shape moved by `m`. A map that turns would leave a rectangle's sides off the axes, so
    /// only the maps of views, which never turn, are taken; others give `None`.
    fn mapped(&self, m: &Similarity2) -> Option<Shape> {
        if m.angle() != 0.0 {
            return None;
        }
        Some(match *self {
            Shape::Circle { centre, radius } => Shape::Circle { centre: m.apply(centre), radius: radius * m.scale() },
            Shape::Rectangle(b) => Shape::Rectangle(Aabb2 { min: m.apply(b.min), max: m.apply(b.max) }),
        })
    }

    /// Whether the whole shape is finite, at least `min_across` wide and high, and lies on paper
    /// (every point at most [`MAX_SIDE_MM`] from the sheet's origin either way).
    fn on_paper(&self, min_across: f64) -> bool {
        let b = self.bounds();
        let size = b.size();
        [b.min.x, b.min.y, b.max.x, b.max.y].into_iter().all(is_paper_coordinate) && size.x >= min_across && size.y >= min_across
    }
}

/// The region a detail shows, in its parent view's plane coordinates. Made only through
/// [`Boundary::circle`] and [`Boundary::rectangle`], so it is always finite and not empty.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boundary {
    shape: Shape,
}

fn plane_point(p: Vec2) -> bool {
    p.is_finite() && p.x.abs() <= MAX_NUMBER && p.y.abs() <= MAX_NUMBER
}

impl Boundary {
    /// A circle about `centre` of `radius` (above 0), both finite.
    pub fn circle(centre: Vec2, radius: f64) -> Result<Boundary> {
        require(plane_point(centre), "a detail's centre is finite")?;
        require(radius.is_finite() && radius > 0.0 && radius <= MAX_NUMBER, "a detail's radius is finite and above 0")?;
        Ok(Boundary { shape: Shape::Circle { centre, radius } })
    }

    /// The rectangle with opposite corners `a` and `b`, in either order; its sides are above 0.
    pub fn rectangle(a: Vec2, b: Vec2) -> Result<Boundary> {
        require(plane_point(a) && plane_point(b), "a detail's corners are finite")?;
        require(a.x != b.x && a.y != b.y, "a detail's rectangle is wider and higher than 0")?;
        let min = Vec2::new(a.x.min(b.x), a.y.min(b.y));
        let max = Vec2::new(a.x.max(b.x), a.y.max(b.y));
        Ok(Boundary { shape: Shape::Rectangle(Aabb2 { min, max }) })
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    pub fn centre(&self) -> Vec2 {
        self.shape.centre()
    }

    /// Whether the point `p` of the parent's plane is inside the region or on its edge.
    pub fn contains(&self, p: Vec2) -> bool {
        self.shape.contains(p)
    }
}

/// A detail view: a [`Boundary`] of a parent view, shown at its own scale with its centre at a
/// paper position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detail {
    boundary: Boundary,
    /// The boundary on the parent's paper.
    outline: Shape,
    /// The boundary on the detail's paper.
    window: Shape,
    frame: ViewFrame,
}

impl Detail {
    /// The detail of `boundary` on `parent`, at `scale`, its centre at `position_mm` on paper.
    /// Refused unless the outline lies on the parent's paper and the window lies on the detail's,
    /// at least [`MIN_WINDOW_MM`] across.
    pub fn new(parent: &ViewFrame, boundary: Boundary, scale: Scale, position_mm: Vec2) -> Result<Detail> {
        let frame = ViewFrame::new(parent.basis(), scale, boundary.centre(), position_mm)?;
        let message = "a view's map keeps paper x and y";
        let outline = boundary.shape.mapped(&parent.paper_map()).ok_or(Invalid(message))?;
        let window = boundary.shape.mapped(&frame.paper_map()).ok_or(Invalid(message))?;
        require(outline.on_paper(0.0), "a detail's outline lies within 5000 mm of the sheet's origin on its parent")?;
        require(window.on_paper(MIN_WINDOW_MM), "a detail is at least 0.01 mm across and lies within 5000 mm of the sheet's origin")?;
        Ok(Detail { boundary, outline, window, frame })
    }

    pub fn boundary(&self) -> Boundary {
        self.boundary
    }

    /// The detail's own view: the parent's basis at the detail's scale.
    pub fn frame(&self) -> ViewFrame {
        self.frame
    }

    pub fn scale(&self) -> Scale {
        self.frame.scale()
    }

    /// The boundary on the parent's paper: the circle or rectangle the parent shows around the
    /// region.
    pub fn outline_shape(&self) -> Shape {
        self.outline
    }

    /// The outline as curves on the parent's paper, one closed counter-clockwise loop: a circle
    /// as two half turns from its rightmost point, a rectangle as four lines from its lowest-left
    /// corner.
    pub fn outline(&self) -> Vec<Seg2> {
        match self.outline {
            Shape::Circle { centre, radius } => {
                vec![Seg2::Arc { center: centre, radius, start: 0.0, sweep: PI }, Seg2::Arc { center: centre, radius, start: PI, sweep: PI }]
            }
            Shape::Rectangle(b) => {
                let corners = [b.min, Vec2::new(b.max.x, b.min.y), b.max, Vec2::new(b.min.x, b.max.y)];
                corners.iter().zip(corners.iter().cycle().skip(1)).map(|(&a, &b)| Seg2::Line { a, b }).collect()
            }
        }
    }

    /// The boundary on the detail's paper: the shape its geometry is cut to.
    pub fn window(&self) -> Shape {
        self.window
    }

    /// The map from the parent's paper to the detail's: a point the parent shows at `p` is where
    /// the detail shows it.
    pub fn parent_to_detail(&self, parent: &ViewFrame) -> Result<Similarity2> {
        let from = parent.paper_map();
        let to = self.frame.paper_map();
        let k = to.scale() / from.scale();
        let centre = self.boundary.centre();
        Similarity2::new(k, 0.0, to.apply(centre) - from.apply(centre) * k).ok_or(Invalid("a detail's map is finite"))
    }

    /// Where the label goes on the parent's paper: `gap_mm` (from 0 to [`MAX_SIDE_MM`]) outside
    /// the outline, from its centre `direction` radians counter-clockwise from paper x.
    pub fn label_anchor(&self, direction: f64, gap_mm: f64) -> Result<Vec2> {
        require(direction.is_finite(), "a label's direction is finite")?;
        require(gap_mm.is_finite() && (0.0..=MAX_SIDE_MM).contains(&gap_mm), "a label's gap is from 0 to 5000 mm")?;
        let d = Vec2::from_angle(direction);
        let to_edge = match self.outline {
            Shape::Circle { radius, .. } => radius,
            Shape::Rectangle(b) => {
                let half = b.size() * 0.5;
                // The ray leaves the box through the side it reaches first; along an axis the other
                // side is never reached.
                let along = |h: f64, c: f64| if c.abs() > 0.0 { h / c.abs() } else { f64::INFINITY };
                along(half.x, d.x).min(along(half.y, d.y))
            }
        };
        let p = self.outline.centre() + d * (to_edge + gap_mm);
        require(is_paper_coordinate(p.x) && is_paper_coordinate(p.y), "a label lies within 5000 mm of the sheet's origin")?;
        Ok(p)
    }
}

/// A detail's name: one to [`MAX_LABEL_LETTERS`] capital letters `A` to `Z`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label(String);

impl Label {
    pub fn new(text: &str) -> Result<Label> {
        let letters = (1..=MAX_LABEL_LETTERS).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_uppercase());
        require(letters, "a detail label is one to three capital letters A to Z")?;
        Ok(Label(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `n`th label of [`next_label`]'s sequence, from 0: `A`…`Z` without `I`, `O` and `Q`, then
/// `AA`, `AB`… with the same letters, then three letters. `None` past `ZZZ`.
fn nth_label(mut n: usize) -> Option<String> {
    let base = LETTERS.len();
    let mut letters = 1;
    let mut count = base;
    while n >= count {
        n -= count;
        letters += 1;
        if letters > MAX_LABEL_LETTERS {
            return None;
        }
        count = count.checked_mul(base)?;
    }
    let mut out = vec![b'A'; letters];
    for slot in out.iter_mut().rev() {
        *slot = *LETTERS.get(n % base)?;
        n /= base;
    }
    String::from_utf8(out).ok()
}

/// The first label of the sequence `A`, `B`, … `Z`, `AA`, … `ZZZ` (skipping `I`, `O` and `Q`)
/// that is not in `used`. Refused when every one is.
pub fn next_label<'a>(used: impl IntoIterator<Item = &'a Label>) -> Result<Label> {
    let used: HashSet<&str> = used.into_iter().map(Label::as_str).collect();
    (0..).map_while(nth_label).find(|l| !used.contains(l.as_str())).map(Label).ok_or(Invalid("every detail label up to ZZZ is in use"))
}

#[cfg(test)]
#[path = "drawing_detail_tests.rs"]
mod tests;
