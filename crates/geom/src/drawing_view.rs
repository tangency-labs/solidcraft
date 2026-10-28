//! Drawing views as data (W17a, #4816): which way a view looks, its scale, and the map from the
//! model's millimetres to the paper's. Nothing here draws or measures; the view commands (W07)
//! and the hidden-line and section producers feed it.
//!
//! - A view's **basis** is a [`Plane`]: paper x along `x`, paper y along `y`, and its normal
//!   `x × y` toward the viewer. [`NamedView`] gives the principal views and the four upper
//!   isometric corners exactly, never from a camera, which tilts a top view by its pitch clamp;
//!   [`basis_looking`] gives any other view from its direction and up.
//! - A **[`Scale`]** is a ratio of two numbers from 0.000001 to 1000000, written `"1:2"` (paper :
//!   model), from 1:100000 to 1000:1. [`Scale::fit`] picks the largest of ISO 5455's preset
//!   scales at which a view fits a space.
//! - A **[`ViewFrame`]** places a view on paper: the point of the view's plane at `centre` lands at
//!   `position_mm`, everything scaled by the scale. Its [`ViewFrame::paper_map`] moves the view's
//!   curves ([`Seg2::similar`]) and their accuracy ([`CurveAccuracy::scaled`]) onto paper.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::paper::MAX_SIDE_MM;
use crate::section::{CurveAccuracy, MAX_NUMBER, validate_plane};
use crate::{Aabb2, Plane, Seg2, Similarity2, Vec2, Vec3};

/// The smallest scale: one paper millimetre for 100 m of model.
pub const MIN_SCALE: f64 = 1e-5;
/// The largest scale: 1000 paper millimetres for one of model.
pub const MAX_SCALE: f64 = 1000.0;
/// The smallest and largest number either side of a scale may be: `"1:100000"` and `"1000:1"`
/// with room to spare, and never more than about 50 characters written out.
pub const MIN_PART: f64 = 1e-6;
pub const MAX_PART: f64 = 1e6;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("drawing view: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

/// Two directions closer to parallel than this (the sine of their angle) give no view.
const PARALLEL: f64 = 1e-9;

/// A view with a name: the six principal views and the isometric views from the four upper
/// corners. Each looks at the model from where its name says; `iso` reads as the front-right one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedView {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    #[serde(alias = "iso")]
    IsoFrontRight,
    IsoFrontLeft,
    IsoBackRight,
    IsoBackLeft,
}

impl NamedView {
    pub const ALL: [NamedView; 10] = [
        NamedView::Front,
        NamedView::Back,
        NamedView::Top,
        NamedView::Bottom,
        NamedView::Left,
        NamedView::Right,
        NamedView::IsoFrontRight,
        NamedView::IsoFrontLeft,
        NamedView::IsoBackRight,
        NamedView::IsoBackLeft,
    ];

    /// The view's basis, through the world origin. The principal views' axes are exact unit
    /// vectors, with no -0 to write out, each the front view folded about the edge it shares with
    /// it: the top and bottom views keep paper x along +X (the bottom's y is -Y), and the back and
    /// side views keep paper y along +Z. An isometric view looks from its corner, `(±1, ±1, 1)`,
    /// toward the origin, with paper x horizontal (along the view direction × Z) and paper y up.
    pub fn basis(self) -> Plane {
        let (x, y) = match self {
            NamedView::Front => (Vec3::X, Vec3::Z),
            NamedView::Back => (Vec3::new(-1.0, 0.0, 0.0), Vec3::Z),
            NamedView::Top => (Vec3::X, Vec3::Y),
            NamedView::Bottom => (Vec3::X, Vec3::new(0.0, -1.0, 0.0)),
            NamedView::Left => (Vec3::new(0.0, -1.0, 0.0), Vec3::Z),
            NamedView::Right => (Vec3::Y, Vec3::Z),
            NamedView::IsoFrontRight => iso(1.0, -1.0),
            NamedView::IsoFrontLeft => iso(-1.0, -1.0),
            NamedView::IsoBackRight => iso(1.0, 1.0),
            NamedView::IsoBackLeft => iso(-1.0, 1.0),
        };
        Plane { origin: Vec3::ZERO, x, y }
    }
}

/// The view through `origin` looking along `direction`, with `up` toward the top of the paper:
/// paper x = direction × up, paper y = x × direction, so the normal points back at the viewer as
/// the named views' do. `up` need not be square to the direction, only not along it.
pub fn basis_looking(direction: Vec3, up: Vec3, origin: Vec3) -> Result<Plane> {
    let d = direction.normalized().ok_or(Invalid("the view direction is zero or not finite"))?;
    let up = up.normalized().ok_or(Invalid("`up` is zero or not finite"))?;
    let across = d.cross(up);
    require(across.len() >= PARALLEL, "`up` lies along the view direction: give another up")?;
    require(
        origin.is_finite() && [origin.x, origin.y, origin.z].iter().all(|v| v.abs() <= MAX_NUMBER),
        "a view's origin is finite and within 1e12 mm of the world's",
    )?;
    // Square to the direction again: near the threshold the cross product's rounding is a large
    // share of its length, which would tilt the normal by up to 1e-8 rad.
    let across = across - d * across.dot(d);
    let x = across * (1.0 / across.len());
    let basis = Plane { origin, x, y: x.cross(d) };
    validate_plane(&basis).map_err(|_| Invalid("a view's basis is finite and orthonormal"))?;
    Ok(basis)
}

/// Paper x and y for an isometric view from the upper corner `(cx, cy, 1)`: x is horizontal, the
/// view direction × Z, and y = x × direction, so the normal points back at the corner.
fn iso(cx: f64, cy: f64) -> (Vec3, Vec3) {
    let toward = Vec3::new(-cx, -cy, -1.0) * (1.0 / 3f64.sqrt());
    let x = Vec3::new(-cy, cx, 0.0) * (1.0 / 2f64.sqrt());
    (x, x.cross(toward))
}

/// A drawing scale, paper : model, as two positive numbers so `"1:3"` stays exactly what was
/// written. Read and written as that text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    paper: f64,
    model: f64,
}

/// ISO 5455's recommended scales, largest first: enlargements, full size, then reductions.
pub const PRESETS: [Scale; 18] = [
    Scale { paper: 50.0, model: 1.0 },
    Scale { paper: 20.0, model: 1.0 },
    Scale { paper: 10.0, model: 1.0 },
    Scale { paper: 5.0, model: 1.0 },
    Scale { paper: 2.0, model: 1.0 },
    Scale { paper: 1.0, model: 1.0 },
    Scale { paper: 1.0, model: 2.0 },
    Scale { paper: 1.0, model: 5.0 },
    Scale { paper: 1.0, model: 10.0 },
    Scale { paper: 1.0, model: 20.0 },
    Scale { paper: 1.0, model: 50.0 },
    Scale { paper: 1.0, model: 100.0 },
    Scale { paper: 1.0, model: 200.0 },
    Scale { paper: 1.0, model: 500.0 },
    Scale { paper: 1.0, model: 1000.0 },
    Scale { paper: 1.0, model: 2000.0 },
    Scale { paper: 1.0, model: 5000.0 },
    Scale { paper: 1.0, model: 10000.0 },
];

impl Scale {
    /// `paper : model`, each from [`MIN_PART`] to [`MAX_PART`] (so the text stays short), their
    /// ratio from [`MIN_SCALE`] to [`MAX_SCALE`].
    pub fn new(paper: f64, model: f64) -> Result<Scale> {
        let parts = [paper, model].into_iter().all(|v| (MIN_PART..=MAX_PART).contains(&v));
        let message = "a scale is two numbers from 0.000001 to 1000000, from 1:100000 to 1000:1";
        require(parts && (MIN_SCALE..=MAX_SCALE).contains(&(paper / model)), message)?;
        Ok(Scale { paper, model })
    }

    /// Paper millimetres per model millimetre.
    pub fn factor(self) -> f64 {
        self.paper / self.model
    }

    /// The largest preset at which a view `size_mm` wide and high (in model millimetres) fits
    /// within `room_mm` on paper, if any does. Compared as `size * paper <= room * model`, so an
    /// exact fit at 1:5 isn't lost to 0.2's rounding. A size of 0 sets no limit that way (a view of
    /// a line along the other), so a view of a point fits at the largest preset; a size or room
    /// that is negative or not finite fits none.
    pub fn fit(size_mm: Vec2, room_mm: Vec2) -> Option<Scale> {
        if ![size_mm, room_mm].into_iter().all(|v| v.is_finite() && v.x >= 0.0 && v.y >= 0.0) {
            return None;
        }
        PRESETS.into_iter().find(|s| size_mm.x * s.paper <= room_mm.x * s.model && size_mm.y * s.paper <= room_mm.y * s.model)
    }
}

impl fmt::Display for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.paper, self.model)
    }
}

/// Plain decimal numbers only (`2`, `2.5`): digits, then at most one point with digits after it;
/// no sign, exponent or space.
fn decimal(text: &str) -> Option<f64> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let plain = match text.split_once('.') {
        Some((whole, fraction)) => digits(whole) && digits(fraction),
        None => digits(text),
    };
    plain.then(|| text.parse().ok()).flatten()
}

impl FromStr for Scale {
    type Err = Invalid;

    fn from_str(text: &str) -> Result<Scale> {
        let parts = text.split_once(':').and_then(|(p, m)| Some((decimal(p)?, decimal(m)?)));
        let (paper, model) = parts.ok_or(Invalid("a scale is written paper:model, such as 1:2"))?;
        Scale::new(paper, model)
    }
}

impl Serialize for Scale {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Scale {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Scale, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

/// A view placed on paper: its basis, its scale, and where it sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewFrame {
    basis: Plane,
    scale: Scale,
    map: Similarity2,
}

impl ViewFrame {
    /// The view through `basis` at `scale`, with the point `centre` of the view's plane (in its
    /// own coordinates, [`Plane::to_local`]) at `position_mm` on paper.
    pub fn new(basis: Plane, scale: Scale, centre: Vec2, position_mm: Vec2) -> Result<ViewFrame> {
        validate_plane(&basis).map_err(|_| Invalid("a view's basis is finite and orthonormal"))?;
        require(centre.is_finite() && centre.x.abs() <= MAX_NUMBER && centre.y.abs() <= MAX_NUMBER, "a view's centre is finite")?;
        let on_paper = position_mm.is_finite() && position_mm.x.abs() <= MAX_SIDE_MM && position_mm.y.abs() <= MAX_SIDE_MM;
        require(on_paper, "a view's position is finite and at most 5000 mm from the sheet's origin")?;
        let factor = scale.factor();
        let map = Similarity2::new(factor, 0.0, position_mm - centre * factor).ok_or(Invalid("a view's position is finite"))?;
        Ok(ViewFrame { basis, scale, map })
    }

    pub fn basis(&self) -> Plane {
        self.basis
    }

    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// The map from the view's plane coordinates to paper millimetres, for its curves. Plane
    /// coordinates are the plane's own ([`Plane::to_local`]: from the basis's origin along its x
    /// and y), as section curves are. Points measured along x and y from the world origin (as the
    /// hidden-line output is planned to be) first lose the origin's own `(origin · x, origin · y)`;
    /// a named view's origin is the world origin, so for it the two are the same.
    pub fn paper_map(&self) -> Similarity2 {
        self.map
    }

    /// Where a model point appears on paper.
    pub fn to_paper(&self, p: Vec3) -> Vec2 {
        self.map.apply(self.basis.to_local(p))
    }

    /// A box of the view's plane (in plane coordinates, as [`Self::paper_map`]'s) on paper: the box
    /// around its four corners, mapped. A box that is inside out, or not finite on paper, has none.
    pub fn boundary(&self, model: &Aabb2) -> Result<Aabb2> {
        let corners = [model.min, Vec2::new(model.max.x, model.min.y), model.max, Vec2::new(model.min.x, model.max.y)].map(|c| self.map.apply(c));
        require(!model.is_empty() && corners.iter().all(|c| c.is_finite()), "a view's box is not inside out and is finite on paper")?;
        Ok(corners.into_iter().collect())
    }

    /// A curve of the view's plane (in plane coordinates, as [`Self::paper_map`]'s), on paper.
    pub fn seg_to_paper(&self, seg: &Seg2) -> Seg2 {
        seg.similar(&self.map)
    }

    /// A curve's accuracy on paper: its deviation scaled as the curve is.
    pub fn accuracy_on_paper(&self, accuracy: CurveAccuracy) -> CurveAccuracy {
        accuracy.scaled(self.map.scale())
    }
}

#[cfg(test)]
#[path = "drawing_view_tests.rs"]
mod tests;
