//! Spot welds (#18407): the display bodies of one resistance spot weld through a stack of lapped
//! sheets ([`spot_weld`]), where a spot picked on a sheet's outer face sits ([`spot_site`]), and
//! the named looks a weld tool offers ([`SpotLook`]).
//!
//! SolidCraft's own: solidcore and solidcore-ops hold no weld (jyviko and bherbruck on #18409,
//! #21393). Every measurement and every body here is an ops call; what this module adds is the
//! weld's rules and sizes. The `weld.spot` command and its storage come with #15131.
//!
//! | Rust                       | What it is                                                  |
//! |----------------------------|-------------------------------------------------------------|
//! | [`SpotWeld`]               | One spot: where, which way, the sheets, the diameter        |
//! | [`SpotStyle`]              | What the spot is drawn with, as data: marks, ball, pin      |
//! | [`SpotLook`]               | The six named styles, and the default                       |
//! | [`SpotZone`], [`SpotSide`] | Which part of the style a body is, and where it lies        |
//! | [`spot_weld`]              | The spot's bodies                                           |
//! | [`spot_site`]              | The spot's frame and sheets, from a point picked on a face  |
//! | [`proposed_spot_diameter`] | The diameter a weld tool proposes, `5 √t`                   |
//! | [`SpotRefusal`]            | Why a spot is refused                                       |
//!
//! # The geometry
//!
//! A spot joins a stack of two or more sheets. It sits on the *faying face* where the first two
//! touch; `normal` points from the first sheet into the next, and the sheets' thicknesses are
//! listed in that order. The first sheet's outer face lies its thickness against `normal`, the last
//! sheet's outer face the other thicknesses along it. A [`SpotStyle`] gives every size as a
//! fraction: of the spot diameter `d` (marks, ball, pin), or of a sheet's thickness (a pin's dish),
//! so a style keeps its proportions at any diameter.
//!
//! The *marks* show the spot on an outer face, as a real spot's electrode imprint does: a core, a
//! band and an edge, three bodies lying on the face, flat or with a dished top. Resistance spot
//! welding adds no filler, so a body inside the sheets ([`SpotZone::Inside`]) overlaps metal the
//! parts already have: every body is for display only, never volume, mass or anything a model
//! depends on. The bodies of one spot share the faces where their pieces meet; they must never be
//! united.
//!
//! Each body is a profile in the local XZ plane (the spot at the origin, its normal along +Z)
//! revolved about Z, or a sphere, then placed at the spot. A dish is the arc through its rim and
//! its middle ([`curve::arc_through`]), cut where each zone ends ([`curve::closest_to_ray`]).
//!
//! # Not covered
//!
//! Flat sheets only: the marks are flat or revolved and the pins straight, so the outer faces must
//! be planes square to `normal` at the spot. A ball only for two sheets: in a stack every faying
//! face has its own nugget, which no look draws.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::brep::Curve as RecordCurve;
use solidcraft_kernel::affine;
use solidcraft_kernel::services::measure::FaceHit;
use solidcraft_kernel::services::sweep::{self, Curve};
use solidcraft_kernel::services::{ExactBody as Body, Order, curve, make_sphere};

/// Why a spot is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SpotRefusal {
    /// Input the spot cannot take: a size, point, normal or style out of range, a sheet or face
    /// not in the list, or sizes the model cannot hold.
    #[error("{0}")]
    Invalid(&'static str),
    /// The spot's point is not on the picked face: the line from it along the face's inward
    /// normal does not meet that face within the zone tolerance of the point.
    #[error("the spot's point is not on the picked face")]
    PointOffFace,
    /// The picked face is not an outer face of the stack: listed sheet `sheet` lies beyond it.
    #[error("the spot's picked face is not an outer face: sheet {sheet} lies beyond it")]
    FaceNotOuter { sheet: usize },
    /// The spot's line misses listed sheet `sheet`.
    #[error("the spot's line misses sheet {sheet}")]
    SheetMissed { sheet: usize },
    /// The spot's line crosses sheet `sheet` more than once (a hem or a folded sheet).
    #[error("the spot's line crosses sheet {sheet} more than once")]
    SheetCrossedAgain { sheet: usize },
    /// Along the spot's line, sheet `sheet` starts before the sheet before it ends.
    #[error("sheet {sheet} overlaps the sheet before it along the spot's line")]
    SheetsOverlap { sheet: usize },
    /// Along the spot's line, sheet `sheet` starts `gap` past the end of the sheet before it,
    /// more than the zone tolerance.
    #[error("sheet {sheet} lies {gap} from the sheet before it along the spot's line")]
    SheetsApart { sheet: usize, gap: f64 },
    /// Record face `face` of sheet `sheet`, under the spot, is not flat.
    #[error("face {face} of sheet {sheet} under the spot is not flat")]
    FaceNotFlat { sheet: usize, face: usize },
    /// Record face `face` of sheet `sheet` is flat but not square to the spot's line.
    #[error("face {face} of sheet {sheet} is not square to the spot's line")]
    FaceNotSquare { sheet: usize, face: usize },
    /// A spot of `diameter` passes the boundary of record face `face` of sheet `sheet`: an edge
    /// lies closer to the spot's line than half the diameter. `fits` is the largest diameter that
    /// fits on every face the spot crosses.
    #[error("a spot of diameter {diameter} passes the edge of face {face} of sheet {sheet}; at most {fits} fits")]
    PastFaceEdge { sheet: usize, face: usize, diameter: f64, fits: f64 },
    /// How far the edges of record face `face` of sheet `sheet` lie from the spot was not found,
    /// so the footprint cannot be checked.
    #[error("how far the edges of face {face} of sheet {sheet} lie from the spot was not found")]
    EdgeNotMeasured { sheet: usize, face: usize },
    /// An ops call failed.
    #[error(transparent)]
    Ops(#[from] solidcraft_kernel::services::Error),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, SpotRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(SpotRefusal::Invalid(why))
}

/// The top of a spot's marks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkTop {
    /// Flat, `thickness` (a fraction of `d`) off the face.
    Flat { thickness: f64 },
    /// Dished: a sphere's cap, `rim` (a fraction of `d`) off the face at the edge's outer radius
    /// and `middle` on the axis, `0 < middle < rim`, at most a half sphere (`rim - middle <= edge /
    /// 2`). The face itself is not dented.
    Dish { rim: f64, middle: f64 },
}

/// On which outer faces a spot's marks are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkFaces {
    /// On both outer faces.
    Both,
    /// Only on an outer face the spot's ball does not reach (its radius is no more than that
    /// sheet's thickness), so the spot shows on both faces whatever the sheets; on both faces when
    /// the style has no ball.
    WhereBallHidden,
}

/// The marks of a [`SpotStyle`]: three zones on an outer face, a core out to diameter `core`, a
/// band out to `band` and an edge out to `edge` (fractions of `d`, `0 < core < band < edge`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotMarks {
    pub core: f64,
    pub band: f64,
    pub edge: f64,
    pub top: MarkTop,
    pub faces: MarkFaces,
}

/// The ball of a [`SpotStyle`]: a sphere of diameter `diameter` (a fraction of `d`) centred on the
/// faying face. It shows on an outer face only where its radius is more than that sheet's
/// thickness. Two sheets only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotBall {
    pub diameter: f64,
}

/// The pin of a [`SpotStyle`]: a cylinder of diameter `diameter` (a fraction of `d`) through every
/// sheet, flush with the outer faces, each end dished in when `dish` is set: the depth as a
/// fraction of the outer sheet that end lies in, held to the pin's radius (a half sphere, whose
/// rim meets the pin's wall tangentially). The two ends must leave at least the zone tolerance
/// between them on the axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotPin {
    pub diameter: f64,
    pub dish: Option<f64>,
}

/// What a spot is drawn with: any of marks, a ball and a pin, each sized as fractions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotStyle {
    pub marks: Option<SpotMarks>,
    pub ball: Option<SpotBall>,
    pub pin: Option<SpotPin>,
}

/// Which part of a style a body is, so the app can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotZone {
    Core,
    Band,
    Edge,
    /// A body through or between the sheets: the ball or the pin.
    Inside,
}

/// Which outer face a mark lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotSide {
    /// The last sheet's outer face (along `normal`).
    Upper,
    /// The first sheet's outer face (against `normal`).
    Lower,
    /// Neither: a body through or between the sheets.
    Through,
}

/// One body of a spot weld.
#[derive(Debug)]
pub struct SpotPiece {
    pub body: Body,
    pub zone: SpotZone,
    pub side: SpotSide,
}

/// One spot weld.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotWeld {
    /// The spot's centre on the faying face between the first two sheets.
    pub at: [f64; 3],
    /// That face's normal, from the first sheet into the next (need not be unit).
    pub normal: [f64; 3],
    /// Every sheet's thickness, in stack order along `normal`: at least two.
    pub thicknesses: Vec<f64>,
    /// The spot diameter: a user setting; [`proposed_spot_diameter`] is a tool's proposal.
    pub diameter: f64,
    pub style: SpotStyle,
}

/// The marks every look but [`SpotLook::Ball`] draws on both outer faces: a core of `0.55 d`, a
/// band out to `d` and an edge out to `1.05 d`, flat, `0.006 d` thick.
pub const MARKS: SpotMarks = SpotMarks { core: 0.55, band: 1.0, edge: 1.05, top: MarkTop::Flat { thickness: 0.006 }, faces: MarkFaces::Both };

/// How a spot weld is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotLook {
    /// The marks on both outer faces, nothing else (the default).
    Marks,
    /// A ball of the spot diameter centred on the faying face; the marks only on an outer face the
    /// ball does not reach. Two sheets only.
    Ball,
    /// The marks and a ball of `0.6 d`. Two sheets only.
    MarksSmallBall,
    /// The marks and a pin of the core's diameter through every sheet.
    MarksThinPin,
    /// The marks and a pin of the spot diameter, each end dished in by `0.3` of its outer sheet.
    MarksPinConcaveEnds,
    /// The marks dished into a shallow inverted dome: `0.04 d` high at the rim, `0.004 d` in the
    /// middle, read as the electrode's dent.
    Dent,
}

impl SpotLook {
    /// Every look, in the order a weld tool lists them.
    pub const ALL: [Self; 6] = [Self::Marks, Self::Ball, Self::MarksSmallBall, Self::MarksThinPin, Self::MarksPinConcaveEnds, Self::Dent];

    /// The look a new spot weld starts with.
    pub const DEFAULT: Self = Self::Marks;

    /// The look's style.
    #[must_use]
    pub const fn style(self) -> SpotStyle {
        let none = SpotStyle { marks: None, ball: None, pin: None };
        match self {
            Self::Marks => SpotStyle { marks: Some(MARKS), ..none },
            Self::Ball => {
                SpotStyle { marks: Some(SpotMarks { faces: MarkFaces::WhereBallHidden, ..MARKS }), ball: Some(SpotBall { diameter: 1.0 }), ..none }
            }
            Self::MarksSmallBall => SpotStyle { marks: Some(MARKS), ball: Some(SpotBall { diameter: 0.6 }), ..none },
            Self::MarksThinPin => SpotStyle { marks: Some(MARKS), pin: Some(SpotPin { diameter: 0.55, dish: None }), ..none },
            Self::MarksPinConcaveEnds => SpotStyle { marks: Some(MARKS), pin: Some(SpotPin { diameter: 1.0, dish: Some(0.3) }), ..none },
            Self::Dent => SpotStyle { marks: Some(SpotMarks { top: MarkTop::Dish { rim: 0.04, middle: 0.004 }, ..MARKS }), ..none },
        }
    }
}

/// The spot diameter a weld tool proposes: `5 √t`, `t` the thinnest sheet in mm.
///
/// A sizing rule, not a shape: TWI's FAQ on interface failures in resistance spot welds gives
/// `5 √t` as the typical target diameter; the AWS C1.1 guidelines take the thinner sheet's
/// parameters for sheets of different thickness. The user can always set another diameter.
///
/// # Errors
/// [`SpotRefusal::Invalid`] for fewer than two sheets, or a thickness not finite and positive.
pub fn proposed_spot_diameter(thicknesses: &[f64]) -> Result<f64> {
    check_sheets(thicknesses)?;
    Ok(5.0 * thicknesses.iter().fold(f64::INFINITY, |m, &t| m.min(t)).sqrt())
}

fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// The first and the last sheet's thickness; refuses fewer than two sheets, or a thickness that is
/// not finite and positive.
fn check_sheets(thicknesses: &[f64]) -> Result<(f64, f64)> {
    let [first, .., last] = *thicknesses else {
        return bad("fewer than two sheets");
    };
    if !thicknesses.iter().all(|&t| positive(t)) {
        return bad("a sheet thickness that is not finite and positive");
    }
    Ok((first, last))
}

/// The bodies of `spot`, placed at the spot.
///
/// # Errors
/// [`SpotRefusal::Invalid`] for a non-finite point, a zero or non-finite normal, fewer than two
/// sheets, a thickness or diameter that is not finite and positive, a style with no piece or a
/// fraction out of its range (see [`SpotStyle`]), a ball on more than two sheets, a pin whose dished
/// ends meet, and sizes the model cannot hold: a piece thinner than the zone tolerance, or a spot
/// past [`sweep::MAX_COORD`]. The errors of the ops calls otherwise.
pub fn spot_weld(spot: &SpotWeld) -> Result<Vec<SpotPiece>> {
    let (first, last) = check_sheets(&spot.thicknesses)?;
    if !positive(spot.diameter) {
        return bad("a spot diameter that is not finite and positive");
    }
    if !spot.at.iter().chain(&spot.normal).all(|v| v.is_finite()) {
        return bad("a spot point or normal that is not finite");
    }
    if spot.normal == [0.0; 3] {
        return bad("a zero normal");
    }
    check_style(&spot.style)?;
    if spot.style.ball.is_some() && spot.thicknesses.len() > 2 {
        return bad("a spot ball on more than two sheets");
    }
    let layout = Layout::of(spot, first, last);
    let tol = sweep::zone_tolerance()?;
    check_sizes(spot, &layout, tol)?;
    let placement = placement(spot.at, spot.normal)?;
    let pieces = local_pieces(&layout, tol)?;
    for piece in &pieces {
        piece.body.transform(&placement)?;
    }
    Ok(pieces)
}

/// Refuses a style with no piece, or a fraction out of its range.
fn check_style(style: &SpotStyle) -> Result<()> {
    if style.marks.is_none() && style.ball.is_none() && style.pin.is_none() {
        return bad("a spot style with no marks, ball or pin");
    }
    if let Some(m) = style.marks {
        if !(positive(m.core) && positive(m.band) && positive(m.edge)) || m.core >= m.band || m.band >= m.edge {
            return bad("spot marks whose diameters are not positive and increasing");
        }
        match m.top {
            MarkTop::Flat { thickness } if !positive(thickness) => return bad("flat spot marks whose thickness is not positive"),
            MarkTop::Dish { rim, middle } if !(positive(rim) && positive(middle) && middle < rim) => {
                return bad("a spot dish whose middle is not between 0 and its rim");
            }
            // past a half sphere the cap's profile turns back over itself
            MarkTop::Dish { rim, middle } if rim - middle > m.edge / 2.0 => {
                return bad("a spot dish deeper than its outer radius (past a half sphere)");
            }
            _ => {}
        }
    }
    if style.ball.is_some_and(|b| !positive(b.diameter)) {
        return bad("a spot ball whose diameter is not positive");
    }
    if style.pin.is_some_and(|p| !positive(p.diameter) || p.dish.is_some_and(|c| !positive(c))) {
        return bad("a spot pin whose diameter or dish is not positive");
    }
    Ok(())
}

/// The marks' top in model units: flat, this high off the face; or dished, `rim` and `middle` off
/// the face.
#[derive(Debug, Clone, Copy)]
enum Top {
    Flat(f64),
    Dish { rim: f64, middle: f64 },
}

/// Every size of a spot in model units, local frame: the first faying face `z = 0`, the last
/// sheet's outer face `z = t_u`, the first sheet's outer face `z = -t_l`.
struct Layout {
    t_u: f64,
    t_l: f64,
    thinnest: f64,
    /// Zone radii: core, band, edge.
    zones: Option<[f64; 3]>,
    top: Option<Top>,
    /// Marks on the upper and on the lower face.
    marks_on: [bool; 2],
    ball: Option<f64>,
    /// A pin's radius, and each end's dish depth (upper, lower).
    pin: Option<(f64, [Option<f64>; 2])>,
}

impl Layout {
    /// `first` and `last` are the outer sheets' thicknesses.
    fn of(spot: &SpotWeld, first: f64, last: f64) -> Self {
        let (d, style) = (spot.diameter, &spot.style);
        let ball = style.ball.map(|b| b.diameter * d / 2.0);
        let shows = |t: f64| ball.is_some_and(|r| r > t);
        let pin = style.pin.map(|p| {
            let r = p.diameter * d / 2.0;
            let end = |t: f64| p.dish.map(|f| (f * t).min(r));
            (r, [end(last), end(first)])
        });
        Self {
            t_u: spot.thicknesses.iter().skip(1).sum(),
            t_l: first,
            thinnest: spot.thicknesses.iter().fold(f64::INFINITY, |m, &v| m.min(v)),
            zones: style.marks.map(|m| [m.core * d / 2.0, m.band * d / 2.0, m.edge * d / 2.0]),
            top: style.marks.map(|m| match m.top {
                MarkTop::Flat { thickness } => Top::Flat(thickness * d),
                MarkTop::Dish { rim, middle } => Top::Dish { rim: rim * d, middle: middle * d },
            }),
            marks_on: match style.marks.map(|m| m.faces) {
                None => [false, false],
                Some(MarkFaces::Both) => [true, true],
                Some(MarkFaces::WhereBallHidden) => [!shows(last), !shows(first)],
            },
            ball,
            pin,
        }
    }

    /// Every length the spot builds with: its pieces' radii, heights, widths and depths.
    fn lengths(&self) -> Vec<f64> {
        let mut out = vec![self.t_u, self.t_l, self.thinnest];
        if let Some([core, band, edge]) = self.zones {
            out.extend([core, band - core, edge - band, edge]);
        }
        match self.top {
            Some(Top::Flat(h)) => out.push(h),
            Some(Top::Dish { rim, middle }) => out.extend([rim, middle, rim - middle]),
            None => {}
        }
        out.extend(self.ball);
        if let Some((r, ends)) = self.pin {
            out.push(r);
            out.extend(ends.into_iter().flatten());
        }
        out
    }
}

/// Refuses sizes the model cannot hold, before any piece is built (an ops call would refuse them
/// with its own, misleading reason).
fn check_sizes(spot: &SpotWeld, layout: &Layout, tol: f64) -> Result<()> {
    let lengths = layout.lengths();
    let largest = lengths.iter().fold(0.0_f64, |m, &l| m.max(l));
    let reach = spot.at.iter().fold(0.0_f64, |m, &c| m.max(c.abs())) + largest;
    if lengths.iter().any(|l| !l.is_finite()) || reach > sweep::MAX_COORD {
        return bad("a spot diameter or sheet so large that the spot passes the model's size limit");
    }
    if let Some((_, [upper, lower])) = layout.pin {
        let between = (layout.t_u - upper.unwrap_or(0.0)) - (-layout.t_l + lower.unwrap_or(0.0));
        if between < tol {
            return bad("a spot pin whose dished ends meet or cross on its axis");
        }
    }
    if lengths.iter().any(|&l| l < tol) {
        return bad("a spot diameter or sheet so small that a piece of the spot is thinner than the zone tolerance");
    }
    Ok(())
}

/// The map from the local frame (the spot at the origin, its normal along +Z) to the spot.
fn placement(at: [f64; 3], normal: [f64; 3]) -> Result<affine::Matrix> {
    let [n, u] = affine::orthonormal_axes(normal, [1.0, 0.0, 0.0]).map_err(|_| SpotRefusal::Invalid("a zero normal"))?;
    // the third axis, n × u: u turned a quarter about n
    let w = affine::apply_point(&affine::rotation([0.0; 3], n, FRAC_PI_2)?, u);
    Ok([[u[0], u[1], u[2], 0.0], [w[0], w[1], w[2], 0.0], [n[0], n[1], n[2], 0.0], [at[0], at[1], at[2], 1.0]])
}

/// The pieces in the local frame.
fn local_pieces(layout: &Layout, tol: f64) -> Result<Vec<SpotPiece>> {
    let (t_u, t_l) = (layout.t_u, layout.t_l);
    let mut pieces = Vec::new();
    let inside = |body| SpotPiece { body, zone: SpotZone::Inside, side: SpotSide::Through };
    if let Some(r) = layout.ball {
        pieces.push(inside(make_sphere([0.0; 3], r)?));
    }
    if let Some((r, [upper, lower])) = layout.pin {
        pieces.push(inside(pin(r, t_u, t_l, upper, lower, tol)?));
    }
    if let (Some(zones), Some(top)) = (layout.zones, layout.top) {
        for ((side, face, sense), on) in [(SpotSide::Upper, t_u, 1.0), (SpotSide::Lower, -t_l, -1.0)].into_iter().zip(layout.marks_on) {
            if !on {
                continue;
            }
            let radii = [0.0, zones[0], zones[1], zones[2]];
            let (h, dish) = match top {
                Top::Flat(h) => (h, None),
                Top::Dish { rim, middle } => (rim, Some(Dish::of(face, sense, zones[2], rim, middle, &radii, tol)?)),
            };
            for (i, zone) in [SpotZone::Core, SpotZone::Band, SpotZone::Edge].into_iter().enumerate() {
                let (inner, outer) = (radii[i], radii[i + 1]);
                let mut curves = vec![line((inner, face), (outer, face))];
                match &dish {
                    Some(dish) => {
                        curves.push(line((outer, face), (outer, dish.heights[i + 1])));
                        curves.extend(Curve::from_record(&dish.arc, [dish.at[i], dish.at[i + 1]])?);
                        curves.push(line((inner, dish.heights[i]), (inner, face)));
                    }
                    None => {
                        let z = face + sense * h;
                        curves.extend([line((outer, face), (outer, z)), line((outer, z), (inner, z)), line((inner, z), (inner, face))]);
                    }
                }
                pieces.push(SpotPiece { body: revolve(&curves)?, zone, side });
            }
        }
    }
    Ok(pieces)
}

/// A dished top on one face: the arc of its sphere through the rim and the middle, in the profile
/// plane, and the arc's parameter and height at each zone radius.
struct Dish {
    arc: RecordCurve,
    at: [f64; 4],
    heights: [f64; 4],
}

impl Dish {
    /// On the face at `face`, standing out along `sense`: `rim` off it at radius `ro`, `middle` on
    /// the axis; at each of `radii`, ascending from 0 to `ro`.
    fn of(face: f64, sense: f64, ro: f64, rim: f64, middle: f64, radii: &[f64; 4], tol: f64) -> Result<Self> {
        let (zr, zm) = (face + sense * rim, face + sense * middle);
        let arc = curve::arc_through([[-ro, 0.0, zr], [0.0, 0.0, zm], [ro, 0.0, zr]])?;
        let (mut at, mut heights) = ([0.0; 4], [0.0; 4]);
        for (i, &rho) in radii.iter().enumerate() {
            let (t, z) = where_at(&arc, rho, face, sense, tol)?;
            at[i] = t;
            heights[i] = z;
        }
        Ok(Self { arc, at, heights })
    }
}

/// The parameter and height where the profile arc `arc` crosses the line at radius `rho`, found from
/// the face at `face` out along `sense`.
fn where_at(arc: &RecordCurve, rho: f64, face: f64, sense: f64, tol: f64) -> Result<(f64, f64)> {
    let found = curve::closest_to_ray(arc, [rho, 0.0, face], [0.0, 0.0, sense], tol)?;
    let Some(p) = found.filter(|p| p.distance <= tol) else {
        return bad("a dish whose height at a zone's radius was not found");
    };
    Ok((p.parameter, p.point[2]))
}

/// The line from `a` to `b`, each `(ρ, z)` in the profile plane (local XZ).
fn line((ra, za): (f64, f64), (rb, zb): (f64, f64)) -> Curve {
    Curve::Line { start: [ra, 0.0, za], end: [rb, 0.0, zb] }
}

/// The body swept by revolving the closed profile `curves` once about the local Z axis.
fn revolve(curves: &[Curve]) -> Result<Body> {
    Ok(sweep::revolve(curves, [0.0; 3], [0.0, 0.0, 1.0], TAU, true)?)
}

/// The pin of radius `r` from `z = -t_l` to `z = t_u`, each end flat or dished by the depth given.
fn pin(r: f64, t_u: f64, t_l: f64, upper: Option<f64>, lower: Option<f64>, tol: f64) -> Result<Body> {
    // an end dished `c` into the face at `face`, from the axis out to the pin's wall
    let dished = |face: f64, sense: f64, c: f64| -> Result<Vec<Curve>> {
        let arc = curve::arc_through([[-r, 0.0, face], [0.0, 0.0, face - sense * c], [r, 0.0, face]])?;
        let (t, _) = where_at(&arc, 0.0, face, -sense, tol)?;
        Ok(Curve::from_record(&arc, [t, 1.0])?)
    };
    let mut curves = vec![line((r, -t_l), (r, t_u))];
    match upper {
        None => curves.push(line((r, t_u), (0.0, t_u))),
        Some(c) => curves.extend(dished(t_u, 1.0, c)?),
    }
    curves.push(line((0.0, t_u - upper.unwrap_or(0.0)), (0.0, -t_l + lower.unwrap_or(0.0))));
    match lower {
        None => curves.push(line((0.0, -t_l), (r, -t_l))),
        Some(c) => curves.extend(dished(-t_l, -1.0, c)?),
    }
    revolve(&curves)
}

/// One sheet a spot joins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotSheet {
    /// The sheet, by its place in the list given to [`spot_site`].
    pub sheet: usize,
    /// The record face the spot's line enters it by.
    pub near: usize,
    /// The record face it leaves it by.
    pub far: usize,
    /// The distance between the two along the line.
    pub thickness: f64,
}

/// Where a spot weld sits: what [`SpotWeld`] needs, and the faces it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotSite {
    /// The spot's centre on the faying face between the first two sheets.
    pub at: [f64; 3],
    /// The spot's unit line, from the picked face into the stack.
    pub normal: [f64; 3],
    /// The sheets in stack order along `normal`, the picked one first.
    pub sheets: Vec<SpotSheet>,
}

/// The site of a spot of `diameter` picked at `point` on record face `face` of sheet `picked`, the
/// sheets of the stack listed in `sheets` with their record orders (any order, at least two).
///
/// The spot's line runs from the point along the picked face's inward normal, through the stack.
/// Each listed sheet must be crossed exactly once, the sheets must follow one another along the
/// line without a gap or an overlap, every face crossed must be flat and square to the line, and
/// the spot's footprint must lie inside every such face. The sheets are read, never changed: every
/// solve runs on a copy.
///
/// # Errors
/// [`SpotRefusal::Invalid`] for fewer than two sheets, a picked sheet not in the list, a face the
/// picked sheet does not have, a point that is not finite or a diameter that is not finite and
/// positive. The other [`SpotRefusal`]s where the stack does not take the spot. The errors of the
/// ops calls otherwise.
pub fn spot_site(sheets: &[(&Body, &Order)], picked: usize, face: usize, point: [f64; 3], diameter: f64) -> Result<SpotSite> {
    if sheets.len() < 2 {
        return bad("fewer than two sheets");
    }
    if picked >= sheets.len() {
        return bad("a picked sheet not in the list");
    }
    if !point.iter().all(|v| v.is_finite()) {
        return bad("a spot point that is not finite");
    }
    if !positive(diameter) {
        return bad("a spot diameter that is not finite and positive");
    }
    let copies = sheets.iter().map(|(b, o)| b.copy_with_order(o)).collect::<solidcraft_kernel::services::Result<Vec<_>>>()?;
    let Some((body, order)) = copies.get(picked) else {
        return bad("a picked sheet not in the list");
    };
    if face >= order.faces.len() {
        return bad("a picked face the picked sheet does not have");
    }
    let tol = sweep::zone_tolerance()?;
    let out = flat_normal(body, order, picked, face)?;
    let normal = out.map(|v| -v);

    // the point on the picked face first, then every sheet crossed once
    let on_face = body.face_hits_by_ray(order, point, normal)?.first().is_some_and(|h| h.face == face && h.distance <= tol);
    if !on_face {
        return Err(SpotRefusal::PointOffFace);
    }
    let mut crossed: Vec<(usize, FaceHit, FaceHit)> = Vec::new();
    for (sheet, (b, o)) in copies.iter().enumerate() {
        // shortcut: a line through an edge meets both its faces at one distance and is refused as
        // crossed again, not as past the face's edge; merge hits at one distance if the app needs
        // the right name
        match b.face_hits_by_ray(o, point, normal)?.as_slice() {
            [near, far] => crossed.push((sheet, *near, *far)),
            [] if b.face_hits_by_ray(o, point, out)?.is_empty() => return Err(SpotRefusal::SheetMissed { sheet }),
            [] => return Err(SpotRefusal::FaceNotOuter { sheet }),
            // one hit: the point lies inside that sheet
            [_] => return Err(SpotRefusal::SheetsOverlap { sheet }),
            _ => return Err(SpotRefusal::SheetCrossedAgain { sheet }),
        }
    }
    crossed.sort_by(|a, b| a.1.distance.total_cmp(&b.1.distance));
    for pair in crossed.windows(2) {
        let [(_, _, before), (sheet, next, _)] = pair else {
            continue;
        };
        let gap = next.distance - before.distance;
        if gap < -tol {
            return Err(SpotRefusal::SheetsOverlap { sheet: *sheet });
        }
        if gap > tol {
            return Err(SpotRefusal::SheetsApart { sheet: *sheet, gap });
        }
    }

    // every face crossed flat and square, and the footprint inside it
    let mut tightest: Option<(f64, usize, usize)> = None;
    for &(sheet, near, far) in &crossed {
        let Some((b, o)) = copies.get(sheet) else {
            continue;
        };
        for hit in [near, far] {
            flat_normal(b, o, sheet, hit.face)?;
            if b.face_square_level(o, hit.face, normal, point)?.is_none() {
                return Err(SpotRefusal::FaceNotSquare { sheet, face: hit.face });
            }
            let edges = b.rim_edges(o, &[hit.face])?;
            if edges.is_empty() {
                continue;
            }
            let Some(c) = b.closest_to_point(o, &[], &edges, hit.point)? else {
                return Err(SpotRefusal::EdgeNotMeasured { sheet, face: hit.face });
            };
            if tightest.is_none_or(|(room, ..)| c.distance < room) {
                tightest = Some((c.distance, sheet, hit.face));
            }
        }
    }
    if let Some((room, sheet, face)) = tightest.filter(|(room, ..)| 2.0 * room < diameter) {
        return Err(SpotRefusal::PastFaceEdge { sheet, face, diameter, fits: 2.0 * room });
    }

    Ok(SpotSite {
        // where the line leaves the picked sheet
        at: crossed.first().map_or(point, |(_, _, far)| far.point),
        normal,
        sheets: crossed
            .into_iter()
            .map(|(sheet, near, far)| SpotSheet { sheet, near: near.face, far: far.face, thickness: far.distance - near.distance })
            .collect(),
    })
}

/// The outward unit normal of flat record face `face` of sheet `sheet`.
fn flat_normal(b: &Body, o: &Order, sheet: usize, face: usize) -> Result<[f64; 3]> {
    match b.face_plane_normal(o, face)? {
        Some(n) => Ok(n.value),
        None => Err(SpotRefusal::FaceNotFlat { sheet, face }),
    }
}

#[cfg(test)]
#[path = "weld_spot_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
