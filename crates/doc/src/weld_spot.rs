//! Spot welds (#18407): the display bodies of one resistance spot weld through a stack of lapped
//! sheets ([`spot_weld`]), where a spot picked on a sheet's outer face sits ([`spot_site`]), and
//! the named looks a weld tool offers ([`SpotLook`]).
//!
//! SolidCraft's own: solidcore and solidcore-ops hold no weld (jyviko and bherbruck on #18409,
//! #21393). Every measurement and every body here is a kernel call; what this module adds is the
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
//! | [`spot_site_curved`]       | The same on curved sheets: each face single-valued round the line |
//! | [`spot_weld_on`]           | The spot's bodies on the faces of its site, curved or flat  |
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
//! # Curved sheets
//!
//! [`spot_site_curved`] takes a spot on curved sheets (#21144): the line runs along the picked
//! face's inward normal at the point, and every face it crosses must be single-valued over the
//! spot's footprint (each line along the spot's line within half the diameter of it meets the
//! face once there), with its edges at least half the diameter from the line itself. Two checks
//! give that: the kernel proves the footprint's normals all within a quarter turn of the line, or
//! all past it (`Body::face_normal_angle_interval`), so the projection along the line is a local
//! diffeomorphism; and the footprint is one piece whose rim is the footprint's cylinder (no edge
//! of the face inside it), so the projection is a covering of the footprint's disc, which, the
//! disc being simply connected, is one-to-one. [`spot_weld_on`] builds on such a site: where
//! every face is a plane square to the line, the bodies are [`spot_weld`]'s; otherwise each mark
//! zone is the solid between the outer face and its offset by the mark's thickness (the face's
//! footprint thickened), cut by the cylinders of the zone's radii about the line, and a pin is a
//! cylinder about the line cut by the two outer faces. A thickening the kernel refuses names the
//! largest diameter whose marks build ([`SpotRefusal::OffsetFolds`]). A dished top or a dished pin
//! end is the flat look's own sphere, placed where the line crosses that outer face: a dished mark
//! is the solid between the face and the sphere's cap (refused, with the largest diameter that
//! clears, where the face reaches the sphere: [`SpotRefusal::DishUnderFace`]), and a dished pin
//! end is cut by the sphere, so its rim follows the face's sag.
//!
//! # Not covered
//!
//! [`spot_weld`] and [`spot_site`] take flat sheets only: the marks are flat or revolved and the
//! pins straight, so the outer faces must be planes square to `normal` at the spot (seam,
//! projection and fusion welds build on [`spot_site`]). On curved sheets only planes, cylinders,
//! cones, spheres and ring tori are proved single-valued: a spot on a free-form face (B-spline,
//! extrusion, revolution) is refused as not proved until the kernel's angle interval covers those
//! surfaces. A ball only for two sheets: in a stack every faying face has its own nugget, which no
//! look draws.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::brep::Curve as RecordCurve;
use solidcraft_kernel::api::FaceHit;
use solidcraft_kernel::curves as curve;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{
    Body, BoolOp, KernelError, Members, PointClass, affine, boolean, cylinder, offset_surface, point_closest, segment_closest,
    split_body_by_sheet_where_it_cuts, thicken,
};

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
    /// Record face `face` of sheet `sheet` is not proved single-valued over the footprint of a spot
    /// of `diameter` ([`spot_site_curved`]): the kernel does not prove its normals there within a
    /// quarter turn of the spot's line (it folds or turns away), or the footprint cuts it in pieces.
    /// `fits` is the largest diameter at which every face crossed is proved single-valued, if a
    /// search halving the footprint found one.
    #[error("face {face} of sheet {sheet} is not proved single-valued over a spot of diameter {diameter}{}", fit_words(.fits))]
    FaceNotSingleValued { sheet: usize, face: usize, diameter: f64, fits: Option<f64> },
    /// The marks of a spot of `diameter` on record face `face` of sheet `sheet` do not build: the
    /// kernel refuses to offset the face by the marks' thickness (it curves tighter than they are
    /// thick, so the offset would fold). `fits` is the largest diameter whose marks build there:
    /// the kernel's largest wall, where it names one, else the largest a search halving the
    /// diameter built.
    #[error(
        "the marks of a spot of diameter {diameter} on face {face} of sheet {sheet} are thicker than the face curves, so its offset would fold; at most {fits} fits"
    )]
    OffsetFolds { sheet: usize, face: usize, diameter: f64, fits: f64 },
    /// The dished marks of a spot of `diameter` on record face `face` of sheet `sheet` do not fit:
    /// the face curves up into the dish's sphere (or comes within the zone tolerance of it), so the
    /// marks would dip under the face. `fits` is the largest diameter at which the face stays
    /// below the sphere by at least half the dish's middle height, if a search halving the
    /// diameter found one.
    #[error("the dished marks of a spot of diameter {diameter} on face {face} of sheet {sheet} would dip under the face{}", fit_words(.fits))]
    DishUnderFace { sheet: usize, face: usize, diameter: f64, fits: Option<f64> },
    /// How far the edges of record face `face` of sheet `sheet` lie from the spot was not found,
    /// so the footprint cannot be checked.
    #[error("how far the edges of face {face} of sheet {sheet} lie from the spot was not found")]
    EdgeNotMeasured { sheet: usize, face: usize },
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
    /// A curve call refused, in the kernel's words.
    #[error(transparent)]
    Curve(#[from] curve::CurveRefused),
}

type Result<T> = std::result::Result<T, SpotRefusal>;

/// The words a refusal ends with when it names a fit, or none.
fn fit_words(fits: &Option<f64>) -> String {
    fits.map(|f| format!("; at most {f} fits")).unwrap_or_default()
}

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
    /// Only on the last sheet's outer face (along `normal`).
    Upper,
    /// Only on the first sheet's outer face (against `normal`).
    Lower,
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

pub(crate) fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// The first and the last sheet's thickness; refuses fewer than two sheets, or a thickness that is
/// not finite and positive.
pub(crate) fn check_sheets(thicknesses: &[f64]) -> Result<(f64, f64)> {
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
/// past [`profiles::MAX_COORD`]. The errors of the kernel calls otherwise.
pub fn spot_weld(spot: &SpotWeld) -> Result<Vec<SpotPiece>> {
    let (layout, tol) = checked_layout(spot)?;
    let placement = placement(spot.at, spot.normal)?;
    local_pieces(&layout, tol, &placement)
}

/// The spot's sizes in model units, and the zone tolerance, after [`spot_weld`]'s checks.
fn checked_layout(spot: &SpotWeld) -> Result<(Layout, f64)> {
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
    let tol = profiles::zone_tolerance()?;
    check_sizes(spot, &layout, tol)?;
    Ok((layout, tol))
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
                Some(MarkFaces::Upper) => [true, false],
                Some(MarkFaces::Lower) => [false, true],
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

/// Refuses sizes the model cannot hold, before any piece is built (a kernel call would refuse them
/// with its own, misleading reason).
fn check_sizes(spot: &SpotWeld, layout: &Layout, tol: f64) -> Result<()> {
    let lengths = layout.lengths();
    let largest = lengths.iter().fold(0.0_f64, |m, &l| m.max(l));
    let reach = spot.at.iter().fold(0.0_f64, |m, &c| m.max(c.abs())) + largest;
    if lengths.iter().any(|l| !l.is_finite()) || reach > profiles::MAX_COORD {
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

/// The pieces, each made in the local frame and moved by `place` to the spot.
fn local_pieces(layout: &Layout, tol: f64, place: &affine::Matrix) -> Result<Vec<SpotPiece>> {
    let (t_u, t_l) = (layout.t_u, layout.t_l);
    let mut pieces = Vec::new();
    let inside = |body| SpotPiece { body, zone: SpotZone::Inside, side: SpotSide::Through };
    if let Some(r) = layout.ball {
        pieces.push(inside(profiles::ball([0.0; 3], r, Some(place))?));
    }
    if let Some((r, [upper, lower])) = layout.pin {
        pieces.push(inside(pin(r, t_u, t_l, upper, lower, tol, place)?));
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
                        curves.extend(profiles::curve_pieces(&dish.arc, [dish.at[i], dish.at[i + 1]])?);
                        curves.push(line((inner, dish.heights[i]), (inner, face)));
                    }
                    None => {
                        let z = face + sense * h;
                        curves.extend([line((outer, face), (outer, z)), line((outer, z), (inner, z)), line((inner, z), (inner, face))]);
                    }
                }
                pieces.push(SpotPiece { body: revolve(&curves, place)?, zone, side });
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

/// The body swept by revolving the closed profile `curves` once about the local Z axis, moved by
/// `place`.
fn revolve(curves: &[Curve], place: &affine::Matrix) -> Result<Body> {
    Ok(profiles::revolve_curves(curves, [0.0; 3], [0.0, 0.0, 1.0], TAU, true, Some(place))?)
}

/// The pin of radius `r` from `z = -t_l` to `z = t_u`, each end flat or dished by the depth given,
/// moved by `place`.
fn pin(r: f64, t_u: f64, t_l: f64, upper: Option<f64>, lower: Option<f64>, tol: f64, place: &affine::Matrix) -> Result<Body> {
    // an end dished `c` into the face at `face`, from the axis out to the pin's wall
    let dished = |face: f64, sense: f64, c: f64| -> Result<Vec<Curve>> {
        let arc = curve::arc_through([[-r, 0.0, face], [0.0, 0.0, face - sense * c], [r, 0.0, face]])?;
        let (t, _) = where_at(&arc, 0.0, face, -sense, tol)?;
        Ok(profiles::curve_pieces(&arc, [t, 1.0])?)
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
    revolve(&curves, place)
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
/// sheets of the stack listed in `sheets` (any order, at least two).
///
/// The spot's line runs from the point along the picked face's inward normal, through the stack.
/// Each listed sheet must be crossed exactly once, the sheets must follow one another along the
/// line without a gap or an overlap, every face crossed must be flat and square to the line, and
/// the spot's footprint must lie inside every such face. The sheets are read, never changed: the
/// solves run on each body's kernel form, never on its record.
///
/// # Errors
/// [`SpotRefusal::Invalid`] for fewer than two sheets, a picked sheet not in the list, a face the
/// picked sheet does not have, a point that is not finite or a diameter that is not finite and
/// positive. The other [`SpotRefusal`]s where the stack does not take the spot. The errors of the
/// kernel calls otherwise.
pub fn spot_site(sheets: &[&Body], picked: usize, face: usize, point: [f64; 3], diameter: f64) -> Result<SpotSite> {
    let body = picked_body(sheets, picked, face, point, diameter)?;
    let tol = profiles::zone_tolerance()?;
    let out = flat_normal(body, picked, face)?;
    let normal = out.map(|v| -v);
    let crossed = crossed_sheets(sheets, body, face, point, normal, out, tol)?;

    // every face crossed flat and square, and the footprint inside it
    let mut tightest: Option<(f64, usize, usize)> = None;
    for &(sheet, near, far) in &crossed {
        let Some(b) = sheets.get(sheet) else {
            continue;
        };
        for hit in [near, far] {
            flat_normal(b, sheet, hit.face)?;
            if b.face_square_level(hit.face, v3(normal), v3(point))?.is_none() {
                return Err(SpotRefusal::FaceNotSquare { sheet, face: hit.face });
            }
            let edges = b.rim_edges(&[hit.face])?;
            if edges.is_empty() {
                continue;
            }
            let Some(c) = b.closest_to_point(&[], &edges, hit.point)? else {
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
    Ok(site_of(point, normal, crossed))
}

/// The picked sheet, after the input checks [`spot_site`] and [`spot_site_curved`] share.
fn picked_body<'a>(sheets: &[&'a Body], picked: usize, face: usize, point: [f64; 3], diameter: f64) -> Result<&'a Body> {
    if sheets.len() < 2 {
        return bad("fewer than two sheets");
    }
    if !point.iter().all(|v| v.is_finite()) {
        return bad("a spot point that is not finite");
    }
    if !positive(diameter) {
        return bad("a spot diameter that is not finite and positive");
    }
    let Some(&body) = sheets.get(picked) else {
        return bad("a picked sheet not in the list");
    };
    if face >= body.face_count() {
        return bad("a picked face the picked sheet does not have");
    }
    Ok(body)
}

/// Each sheet the line from `point` along `normal` crosses (`out` the other way), with where it
/// enters and leaves it, in stack order: the point on the picked face first, every sheet crossed
/// once, and no gap or overlap beyond the zone tolerance `tol` between one and the next.
fn crossed_sheets(
    sheets: &[&Body],
    body: &Body,
    face: usize,
    point: [f64; 3],
    normal: [f64; 3],
    out: [f64; 3],
    tol: f64,
) -> Result<Vec<(usize, FaceHit, FaceHit)>> {
    let on_face = body.face_hits_by_ray(point, normal)?.first().is_some_and(|h| h.face == face && h.distance <= tol);
    if !on_face {
        return Err(SpotRefusal::PointOffFace);
    }
    let mut crossed: Vec<(usize, FaceHit, FaceHit)> = Vec::new();
    for (sheet, b) in sheets.iter().enumerate() {
        // shortcut: a line through an edge meets both its faces at one distance and is refused as
        // crossed again, not as past the face's edge; merge hits at one distance if the app needs
        // the right name
        match b.face_hits_by_ray(point, normal)?.as_slice() {
            [near, far] => crossed.push((sheet, *near, *far)),
            [] if b.face_hits_by_ray(point, out)?.is_empty() => return Err(SpotRefusal::SheetMissed { sheet }),
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
    Ok(crossed)
}

/// The site of the line from `point` along `normal` through the sheets `crossed`.
fn site_of(point: [f64; 3], normal: [f64; 3], crossed: Vec<(usize, FaceHit, FaceHit)>) -> SpotSite {
    SpotSite {
        // where the line leaves the picked sheet
        at: crossed.first().map_or(point, |(_, _, far)| far.point),
        normal,
        sheets: crossed
            .into_iter()
            .map(|(sheet, near, far)| SpotSheet { sheet, near: near.face, far: far.face, thickness: far.distance - near.distance })
            .collect(),
    }
}

/// The site of a spot of `diameter` picked at `point` on record face `face` of sheet `picked`, on
/// sheets that may be curved: [`spot_site`]'s, with the line along the picked face's inward
/// normal at the point, and in place of "flat and square" every face crossed single-valued over
/// the spot's footprint (the faces within half the diameter of the line, each plane square to the
/// line meeting it once), its edges at least half the diameter from the line itself (not from the
/// point where it crosses the face). On flat, square sheets it is [`spot_site`]'s site.
///
/// # Errors
/// [`spot_site`]'s, with [`SpotRefusal::FaceNotSingleValued`] in place of
/// [`SpotRefusal::FaceNotFlat`] and [`SpotRefusal::FaceNotSquare`], and
/// [`SpotRefusal::PastFaceEdge`] measured to the line.
pub fn spot_site_curved(sheets: &[&Body], picked: usize, face: usize, point: [f64; 3], diameter: f64) -> Result<SpotSite> {
    let body = picked_body(sheets, picked, face, point, diameter)?;
    let tol = profiles::zone_tolerance()?;
    let Some(n) = body.face_normal_at(face, v3(point))? else {
        return Err(SpotRefusal::PointOffFace);
    };
    let out = [n.x, n.y, n.z];
    let normal = out.map(|v| -v);
    let crossed = crossed_sheets(sheets, body, face, point, normal, out, tol)?;

    let r = diameter / 2.0;
    let mut tightest: Option<(f64, usize, usize)> = None;
    // the first face not proved single-valued, and the largest radius proved on every such face
    let mut folded: Option<(usize, usize)> = None;
    let mut proved = r;
    for &(sheet, near, far) in &crossed {
        let Some(&b) = sheets.get(sheet) else {
            continue;
        };
        let reach = reach(b, diameter)?;
        for hit in [near, far] {
            let edges = b.rim_edges(&[hit.face])?;
            if !edges.is_empty() {
                let (from, to) = (along(hit.point, normal, -reach), along(hit.point, normal, reach));
                let Some(c) = segment_closest(v3(from), v3(to), b, Members::Edges(&edges))? else {
                    return Err(SpotRefusal::EdgeNotMeasured { sheet, face: hit.face });
                };
                if tightest.is_none_or(|(room, ..)| c.distance < room) {
                    tightest = Some((c.distance, sheet, hit.face));
                }
            }
            let face_sheet = face_sheet(b, hit.face, tol)?;
            if !single_valued(&face_sheet, hit.point, normal, r, reach)? {
                folded = folded.or(Some((sheet, hit.face)));
                // the largest radius proved on every face so far, if this one holds there too
                if proved > 0.0 && !(proved < r && single_valued(&face_sheet, hit.point, normal, proved, reach)?) {
                    proved = largest_that(proved, |x| single_valued(&face_sheet, hit.point, normal, x, reach))?.unwrap_or(0.0);
                }
            }
        }
    }
    if let Some((room, sheet, face)) = tightest.filter(|(room, ..)| 2.0 * room < diameter) {
        return Err(SpotRefusal::PastFaceEdge { sheet, face, diameter, fits: 2.0 * room });
    }
    if let Some((sheet, face)) = folded {
        let fits = (proved > 0.0).then_some(2.0 * proved.min(tightest.map_or(f64::INFINITY, |(room, ..)| room)));
        return Err(SpotRefusal::FaceNotSingleValued { sheet, face, diameter, fits });
    }
    Ok(site_of(point, normal, crossed))
}

/// How many times [`largest_that`] tries a size: the diameter it names is then within `1 / 4096` of
/// the one it starts from (past the first half, which halving finds).
const FIT_TRIES: usize = 12;

/// The largest size under `size` for which `works` holds, as far as [`FIT_TRIES`] tries find it:
/// halving from `size` until one works, then halving the gap between the largest that works and
/// the smallest that does not. `None` where none tried works.
fn largest_that(size: f64, mut works: impl FnMut(f64) -> Result<bool>) -> Result<Option<f64>> {
    let (mut found, mut over) = (None, size);
    let mut x = size / 2.0;
    for _ in 0..FIT_TRIES {
        if works(x)? {
            found = Some(x);
        } else {
            over = x;
        }
        x = match found {
            Some(f) => f + (over - f) / 2.0,
            None => x / 2.0,
        };
    }
    Ok(found)
}

/// A length past which the line from any point of `b`, both ways, leaves it: its box's three
/// sides together, and `d` more.
fn reach(b: &Body, d: f64) -> Result<f64> {
    let bb = b.exact_bounds()?;
    Ok((bb.max.x - bb.min.x) + (bb.max.y - bb.min.y) + (bb.max.z - bb.min.z) + d)
}

/// The point `s` along `u` from `p` (a frame point, as the spot's placement's).
fn along(p: [f64; 3], u: [f64; 3], s: f64) -> [f64; 3] {
    [p[0] + s * u[0], p[1] + s * u[1], p[2] + s * u[2]]
}

/// Record face `face` of `b` as a sheet of its own, to cut footprints from: its offset by `0`
/// (the face itself, at a fitting tolerance the zone tolerance `tol`, kept in the call's range).
fn face_sheet(b: &Body, face: usize, tol: f64) -> Result<Body> {
    Ok(offset_surface(b, &[face], 0.0, tol.clamp(1e-6, 10.0))?)
}

/// The cylinder of radius `r` about the line through `at` along unit `u`, `reach` each way.
fn column(at: [f64; 3], u: [f64; 3], r: f64, reach: f64) -> Result<Body> {
    Ok(cylinder(v3(along(at, u, -reach)), v3(u), r, 2.0 * reach)?)
}

/// The part of the face sheet `sheet` within `r` of the line through `at` along `u`; `None`
/// where there is none.
fn footprint(sheet: &Body, at: [f64; 3], u: [f64; 3], r: f64, reach: f64) -> Result<Option<Body>> {
    Ok(boolean(sheet, &column(at, u, r, reach)?, BoolOp::Intersect)?)
}

/// Whether the face sheet `sheet` is proved single-valued within `r` of the line through `at`
/// along `u` (module docs): one piece there, every face of it with its normals proved within a
/// quarter turn of the line, all the same way. The caller has checked that the face's edges lie
/// at least `r` from the line.
fn single_valued(sheet: &Body, at: [f64; 3], u: [f64; 3], r: f64, reach: f64) -> Result<bool> {
    let Some(patch) = footprint(sheet, at, u, r, reach)? else {
        return Ok(false);
    };
    if patch.lumps()?.len() != 1 {
        return Ok(false);
    }
    let (mut toward, mut away) = (true, true);
    for f in 0..patch.face_count() {
        let Some([lo, hi]) = patch.face_normal_angle_interval(f, v3(u))? else {
            return Ok(false);
        };
        toward &= hi < FRAC_PI_2;
        away &= lo > FRAC_PI_2;
    }
    Ok(toward || away)
}

/// The bodies of a spot of `diameter` and `style` on `site`, the site of its `sheets`
/// ([`spot_site_curved`] or [`spot_site`]). Where every face the spot crosses is a plane square
/// to its line, they are [`spot_weld`]'s; otherwise each mark zone is cut from the outer face's
/// footprint thickened by the marks' thickness (a dished top: the solid between the face and the
/// flat look's sphere, placed where the line crosses the face), the pin from a cylinder about the
/// line cut by the two outer faces (each dished end cut by its sphere, placed the same way), and
/// the ball is [`spot_weld`]'s, at the spot.
///
/// # Errors
/// [`spot_weld`]'s. On curved sheets also [`SpotRefusal::OffsetFolds`] for marks the kernel does
/// not thicken (with the largest diameter that builds; where none does, the kernel's refusal),
/// [`SpotRefusal::DishUnderFace`] for dished marks the face curves up into, and
/// [`SpotRefusal::Invalid`] for a sheet of the site not in the list.
pub fn spot_weld_on(sheets: &[&Body], site: &SpotSite, diameter: f64, style: SpotStyle) -> Result<Vec<SpotPiece>> {
    let spot = SpotWeld { at: site.at, normal: site.normal, thicknesses: site.sheets.iter().map(|s| s.thickness).collect(), diameter, style };
    if flat_stack(sheets, site)? {
        return spot_weld(&spot);
    }
    let (layout, tol) = checked_layout(&spot)?;
    let (Some(lower), Some(upper)) = (site.sheets.first(), site.sheets.last()) else {
        return bad("fewer than two sheets");
    };
    let (Some(&b_low), Some(&b_up)) = (sheets.get(lower.sheet), sheets.get(upper.sheet)) else {
        return bad("a sheet of the site not in the list");
    };
    let (at, u) = (site.at, site.normal);
    let down = u.map(|v| -v);
    let reach = reach(b_low, diameter)? + reach(b_up, diameter)? + layout.t_u + layout.t_l;
    // where the line crosses the two outer faces (the site's thicknesses are lengths along it)
    let (up_at, low_at) = (along(at, u, layout.t_u), along(at, u, -layout.t_l));
    let up_sheet = face_sheet(b_up, upper.far, tol)?;
    let low_sheet = face_sheet(b_low, lower.near, tol)?;

    let mut pieces = Vec::new();
    let inside = |body| SpotPiece { body, zone: SpotZone::Inside, side: SpotSide::Through };
    if let Some(r) = layout.ball {
        pieces.push(inside(profiles::ball(at, r, None)?));
    }
    if let Some((r, [upper_end, lower_end])) = layout.pin {
        let mut pin = curved_pin(at, u, r, reach, [(&up_sheet, up_at), (&low_sheet, low_at)])?;
        // a dished end: the flat look's sphere through the end's rim and its middle, `c` in
        for (c, p, out) in [(upper_end, up_at, u), (lower_end, low_at, down)] {
            let Some(c) = c else {
                continue;
            };
            let radius = (r * r + c * c) / (2.0 * c);
            let Some(cut) = boolean(&pin, &profiles::ball(along(p, out, radius - c), radius, None)?, BoolOp::Cut)? else {
                return bad("a spot pin that its dished ends took away");
            };
            pin = cut;
        }
        pieces.push(inside(pin));
    }
    if let (Some(zones), Some(top)) = (layout.zones, layout.top) {
        let outers =
            [(SpotSide::Upper, upper.sheet, upper.far, &up_sheet, up_at, u), (SpotSide::Lower, lower.sheet, lower.near, &low_sheet, low_at, down)];
        for ((side, sheet, face, face_sheet, p, out), on) in outers.into_iter().zip(layout.marks_on) {
            if !on {
                continue;
            }
            let slab = match top {
                Top::Flat(h) => match mark_slab(face_sheet, p, out, zones[2] + h, h, reach)? {
                    Ok(slab) => slab,
                    Err(e) => return Err(fold_fit(e, face_sheet, p, out, diameter, zones[2], h, reach, sheet, face)?),
                },
                Top::Dish { rim, middle } => {
                    let dish = DishSphere::of(zones[2], rim, middle);
                    if !dish_clear(face_sheet, p, out, &dish, tol, reach)? {
                        let fits = largest_that(diameter, |d| {
                            let k = d / diameter;
                            dish_clear(face_sheet, p, out, &dish.scaled(k), (0.5 * k * middle).max(tol), reach)
                        })?;
                        return Err(SpotRefusal::DishUnderFace { sheet, face, diameter, fits });
                    }
                    dish_solid(face_sheet, p, out, &dish, reach)?
                }
            };
            let radii = [0.0, zones[0], zones[1], zones[2]];
            for (i, zone) in [SpotZone::Core, SpotZone::Band, SpotZone::Edge].into_iter().enumerate() {
                let (inner, outer) = (radii[i], radii[i + 1]);
                let Some(mut body) = boolean(&slab, &column(p, out, outer, reach)?, BoolOp::Intersect)? else {
                    return bad("a spot mark zone that came out empty");
                };
                if inner > 0.0 {
                    let Some(cut) = boolean(&body, &column(p, out, inner, reach)?, BoolOp::Cut)? else {
                        return bad("a spot mark zone that came out empty");
                    };
                    body = cut;
                }
                pieces.push(SpotPiece { body, zone, side });
            }
        }
    }
    Ok(pieces)
}

/// Whether every face the spot of `site` crosses is a plane square to its line.
fn flat_stack(sheets: &[&Body], site: &SpotSite) -> Result<bool> {
    for s in &site.sheets {
        let Some(&b) = sheets.get(s.sheet) else {
            return bad("a sheet of the site not in the list");
        };
        for face in [s.near, s.far] {
            if b.face_plane_normal(face)?.is_none() || b.face_square_level(face, v3(site.normal), v3(site.at))?.is_none() {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The footprint of the face sheet `face_sheet` within `r` of the line through `p` along `out`,
/// thickened by `h` off the face (the side the face looks out to): `Err` inside for the kernel's
/// refusal of the thickening.
fn mark_slab(face_sheet: &Body, p: [f64; 3], out: [f64; 3], r: f64, h: f64, reach: f64) -> Result<std::result::Result<Body, KernelError>> {
    let Some(patch) = footprint(face_sheet, p, out, r, reach)? else {
        return bad("a spot footprint that came out empty");
    };
    Ok(thicken(&patch, h, false))
}

/// [`SpotRefusal::OffsetFolds`] for marks whose thickening the kernel refused with `refusal`, at
/// a spot of `diameter` (edge radius `r_edge`, marks `h` thick, both in proportion to it): the
/// diameter whose marks are as thick as the kernel's largest wall, where it names one, else the
/// largest diameter a search finds the marks build at. Where none builds, the kernel's refusal.
fn fold_fit(
    refusal: KernelError,
    face_sheet: &Body,
    p: [f64; 3],
    out: [f64; 3],
    diameter: f64,
    r_edge: f64,
    h: f64,
    reach: f64,
    sheet: usize,
    face: usize,
) -> Result<SpotRefusal> {
    let fits = match &refusal {
        KernelError::WallFolds { largest: Some(w), .. } if positive(*w) && *w < h => Some(diameter * *w / h),
        _ => largest_that(diameter, |d| {
            let k = d / diameter;
            Ok(mark_slab(face_sheet, p, out, k * (r_edge + h), k * h, reach)?.is_ok())
        })?,
    };
    Ok(match fits {
        Some(fits) => SpotRefusal::OffsetFolds { sheet, face, diameter, fits },
        None => SpotRefusal::Kernel(refusal),
    })
}

/// The sphere of a dished top in the frame of one outer face, the face's point at height 0 and its
/// outward direction up: its cap stands `middle` off on the axis and passes through the rim at
/// radius `ro`, the flat look's sphere ([`Dish`]).
#[derive(Debug, Clone, Copy)]
struct DishSphere {
    ro: f64,
    middle: f64,
    radius: f64,
}

impl DishSphere {
    /// The sphere of a dish `rim` high at radius `ro` and `middle` high on the axis.
    fn of(ro: f64, rim: f64, middle: f64) -> Self {
        let c = rim - middle;
        Self { ro, middle, radius: (ro * ro + c * c) / (2.0 * c) }
    }

    /// The same dish `k` times the size.
    fn scaled(self, k: f64) -> Self {
        Self { ro: k * self.ro, middle: k * self.middle, radius: k * self.radius }
    }

    /// The height of its centre.
    fn centre(self) -> f64 {
        self.middle + self.radius
    }
}

/// Whether the face sheet `face_sheet`, within the dish's rim radius of the line through `p`
/// along `out`, stays outside the dish's sphere (placed at `p`) by at least `gap`: the kernel's
/// closest point of the footprint to the sphere's centre is at least the radius and `gap` away.
fn dish_clear(face_sheet: &Body, p: [f64; 3], out: [f64; 3], dish: &DishSphere, gap: f64, reach: f64) -> Result<bool> {
    let Some(patch) = footprint(face_sheet, p, out, dish.ro, reach)? else {
        return Ok(false);
    };
    let faces: Vec<usize> = (0..patch.face_count()).collect();
    let Some(c) = point_closest(v3(along(p, out, dish.centre())), &patch, Members::Faces(&faces))? else {
        return Ok(false);
    };
    Ok(c.distance >= dish.radius + gap)
}

/// The solid of dished marks on the face sheet `face_sheet` at `p`, standing out along `out`: the
/// part of a column about the line, a little wider than the dish, between the face and the height
/// of the sphere's centre (where the sphere is wider than the dish), less the sphere. The caller
/// cuts each zone from it by cylinders no wider than the dish, so the column's own wall is never
/// a zone's.
fn dish_solid(face_sheet: &Body, p: [f64; 3], out: [f64; 3], dish: &DishSphere, reach: f64) -> Result<Body> {
    let top = dish.centre();
    let col = cylinder(v3(along(p, out, -reach)), v3(out), 1.05 * dish.ro, reach + top)?;
    let Some(cutter) = footprint(face_sheet, p, out, 1.1 * dish.ro, reach)? else {
        return bad("a spot footprint that came out empty");
    };
    let above = piece_holding(split_by(vec![col], &cutter)?, along(p, out, 0.5 * dish.middle))?
        .ok_or(SpotRefusal::Invalid("a dished spot mark whose piece above the face was not found"))?;
    let Some(solid) = boolean(&above, &profiles::ball(along(p, out, top), dish.radius, None)?, BoolOp::Cut)? else {
        return bad("a dished spot mark that came out empty");
    };
    Ok(solid)
}

/// Every piece of `pieces` split where the sheet `cutter` cuts it, the others as they are.
fn split_by(pieces: Vec<Body>, cutter: &Body) -> Result<Vec<Body>> {
    let mut next = Vec::new();
    for b in pieces {
        match split_body_by_sheet_where_it_cuts(&b, cutter, 0)? {
            Some(parts) => next.extend(parts),
            None => next.push(b),
        }
    }
    Ok(next)
}

/// The first of `pieces` that holds `at` inside it.
fn piece_holding(pieces: Vec<Body>, at: [f64; 3]) -> Result<Option<Body>> {
    for b in pieces {
        if b.classify_point(v3(at))? == PointClass::Inside {
            return Ok(Some(b));
        }
    }
    Ok(None)
}

/// A pin of radius `r` about the line through `at` along `u`: the cylinder about the line cut by
/// each outer face (given as a face sheet and where the line crosses it), the piece that holds
/// `at`.
fn curved_pin(at: [f64; 3], u: [f64; 3], r: f64, reach: f64, outers: [(&Body, [f64; 3]); 2]) -> Result<Body> {
    let mut pieces = vec![column(at, u, r, reach)?];
    for (face_sheet, p) in outers {
        // a footprint a little wider than the pin, so its rim stays clear of the pin's wall
        let Some(cutter) = footprint(face_sheet, p, u, 1.05 * r, reach)? else {
            return bad("a spot footprint that came out empty");
        };
        pieces = split_by(pieces, &cutter)?;
    }
    piece_holding(pieces, at)?.ok_or(SpotRefusal::Invalid("a spot pin whose piece between the outer faces was not found"))
}

/// The outward unit normal of flat record face `face` of sheet `sheet`.
fn flat_normal(b: &Body, sheet: usize, face: usize) -> Result<[f64; 3]> {
    match b.face_plane_normal(face)? {
        Some(n) => Ok(n.value),
        None => Err(SpotRefusal::FaceNotFlat { sheet, face }),
    }
}

fn v3(p: [f64; 3]) -> solidcore_record::Vec3 {
    p.into()
}

#[cfg(test)]
#[path = "weld_spot_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
