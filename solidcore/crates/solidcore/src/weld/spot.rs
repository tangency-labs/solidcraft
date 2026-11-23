//! Spot welds: the display bodies of one resistance spot weld between two lapped sheets.
//!
//! Not upstream: SolidCraft's own (#15132). The bodies are built at the spot from exact
//! surfaces only: each mark and pin is its profile (lines and circular arcs) revolved about
//! the spot's axis, a ball is a sphere, and nothing is cut by a boolean. They take no part
//! as input, so building them can never change a part.
//!
//! | Rust                       | What it is                                                  |
//! |----------------------------|-------------------------------------------------------------|
//! | [`SpotWeld`]               | One spot: where, which way, the two sheets, the diameter    |
//! | [`SpotStyle`]              | What the spot is drawn with, as data: marks, ball, pin      |
//! | [`SpotZone`], [`SpotSide`] | Which part of the style a body is, and where it lies        |
//! | [`spot_weld`]              | The spot's bodies                                           |
//! | [`proposed_spot_diameter`] | The diameter a weld tool proposes, `5 √t`                   |
//!
//! # The geometry
//!
//! A spot joins a stack of two or more sheets. It sits on the *faying face* where the
//! first two touch; `normal` points from the first sheet into the next, and the sheets'
//! thicknesses are listed in that order. The first sheet's outer face lies its thickness
//! against `normal`, the last sheet's outer face the other thicknesses along it.
//! A [`SpotStyle`] gives every size as a fraction: of the spot diameter `d` (marks, ball,
//! pin), or of a sheet's thickness (a pin's dish), so a style keeps its proportions at any
//! diameter. Named styles (looks) and which one a user picked are not the kernel's: they
//! live with the operations and the application, as data of this type.
//!
//! The *marks* show the spot on an outer face, as a real spot's electrode imprint does: a
//! core, a band and an edge, three bodies lying on the face, flat or with a dished top.
//! Resistance spot welding adds no filler, so a body inside the sheets
//! ([`SpotZone::Inside`]) overlaps metal the parts already have: every body is for
//! display only, never volume, mass or anything a model depends on.
//!
//! The bodies of one spot touch where their pieces meet: a zone's walls are its
//! neighbours' (core and band, band and edge), and a pin's flat end lies on the core's base
//! when their diameters match. They are separate bodies that share those faces; they must
//! never be united, and an application that colours them draws coincident faces.
//!
//! # Not covered
//!
//! Flat sheets only: the marks are flat or revolved and the pins straight, so the outer
//! faces must be planes square to `normal` at the spot. A ball only for two sheets: in a
//! stack every faying face has its own nugget, which no look draws. [`spot_site`] reads the
//! frame and the thicknesses from the parts and checks their faces; [`spot_weld`] itself does
//! not see the parts.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::kernel::Kernel;
use crate::limits::MAX_COORDINATE_MAGNITUDE;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p1::NurbCircleParam;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::primitive::api_primitives::create_sphere;
use crate::sweep::api_brep::create_rotational_sweep;
use crate::topology::brep::p5::transform;
use crate::topology::Brep;

mod site;
pub use site::{spot_site, SpotSheet, SpotSite};

const OP_SPOT: &str = "weld::spot";

/// The top of a spot's marks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkTop {
    /// Flat, `thickness` (a fraction of `d`) off the face.
    Flat {
        /// Height off the face, as a fraction of `d`.
        thickness: f64,
    },
    /// Dished: a sphere's cap, `rim` (a fraction of `d`) off the face at the edge's outer
    /// radius and `middle` on the axis, `0 < middle < rim`, at most a half sphere
    /// (`rim - middle <= edge / 2`). The face itself is not dented.
    Dish {
        /// Height off the face at the edge's outer radius, as a fraction of `d`.
        rim: f64,
        /// Height off the face on the axis, as a fraction of `d`.
        middle: f64,
    },
}

/// On which outer faces a spot's marks are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkFaces {
    /// On both outer faces.
    Both,
    /// Only on an outer face the spot's ball does not reach (its radius is no more than
    /// that sheet's thickness), so the spot shows on both faces whatever the sheets; on
    /// both faces when the style has no ball.
    WhereBallHidden,
}

/// The marks of a [`SpotStyle`]: three zones on an outer face, a core out to diameter
/// `core`, a band out to `band` and an edge out to `edge` (fractions of `d`,
/// `0 < core < band < edge`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotMarks {
    /// The core's diameter, as a fraction of `d`.
    pub core: f64,
    /// The band's outer diameter, as a fraction of `d`.
    pub band: f64,
    /// The edge's outer diameter, as a fraction of `d`.
    pub edge: f64,
    /// Flat or dished.
    pub top: MarkTop,
    /// On which outer faces.
    pub faces: MarkFaces,
}

/// The ball of a [`SpotStyle`]: a sphere of diameter `diameter` (a fraction of `d`)
/// centred on the faying face. It shows on an outer face only where its radius is more
/// than that sheet's thickness. Two sheets only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotBall {
    /// The ball's diameter, as a fraction of `d`.
    pub diameter: f64,
}

/// The pin of a [`SpotStyle`]: a cylinder of diameter `diameter` (a fraction of `d`)
/// through every sheet, flush with the outer faces, each end dished in when `dish` is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotPin {
    /// The pin's diameter, as a fraction of `d`.
    pub diameter: f64,
    /// Each end's dish depth as a fraction of the thickness of the outer sheet that end lies in,
    /// held to the pin's radius: a deeper dish would need a sphere wider than the pin, so
    /// it is held at a half sphere, whose rim meets the pin's wall tangentially (a knife
    /// edge on the outer face).
    /// Positive; the two ends must leave at least the zone tolerance between them on the axis
    /// (on two sheets, a dish of 1 makes them meet), or the spot is refused as invalid input.
    pub dish: Option<f64>,
}

/// What a spot is drawn with: any of marks, a ball and a pin, each sized as fractions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotStyle {
    /// The marks on the outer faces.
    pub marks: Option<SpotMarks>,
    /// A ball centred on the faying face.
    pub ball: Option<SpotBall>,
    /// A pin through every sheet.
    pub pin: Option<SpotPin>,
}

/// Which part of a style a body is, so the app can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotZone {
    /// A mark's core.
    Core,
    /// A mark's band.
    Band,
    /// A mark's edge.
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
    /// The body.
    pub body: Brep,
    /// Which part of the style it is.
    pub zone: SpotZone,
    /// Which face it lies on.
    pub side: SpotSide,
}

/// One spot weld.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotWeld {
    /// The spot's centre on the faying face between the first two sheets.
    pub at: Point3,
    /// That face's normal, from the first sheet into the next (need not be unit).
    pub normal: Vec3,
    /// Every sheet's thickness, in stack order along `normal`: at least two.
    pub thicknesses: Vec<f64>,
    /// The spot diameter: a user setting; [`proposed_spot_diameter`] is a tool's proposal.
    pub diameter: f64,
    /// What the spot is drawn with.
    pub style: SpotStyle,
}

/// The spot diameter a weld tool proposes: `5 √t`, `t` the thinnest sheet in mm.
///
/// Sources: TWI's FAQ on interface failures in resistance spot welds gives `5 √t` as the
/// typical target diameter; the AWS C1.1 guidelines take the thinner sheet's parameters
/// for sheets of different thickness. The user can always set another diameter.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for fewer than two sheets, or a thickness that is not
/// finite and positive.
pub fn proposed_spot_diameter(thicknesses: &[f64]) -> Result<f64> {
    check_sheets(thicknesses)?;
    Ok(5.0
        * thicknesses
            .iter()
            .fold(f64::INFINITY, |m, &t| m.min(t))
            .sqrt())
}

/// The first and the last sheet's thickness; refuses fewer than two sheets, or a thickness
/// that is not finite and positive.
fn check_sheets(thicknesses: &[f64]) -> Result<(f64, f64)> {
    let [first, .., last] = *thicknesses else {
        return Err(SolidError::invalid_input(OP_SPOT, "fewer than two sheets"));
    };
    if !thicknesses.iter().all(|&t| positive(t)) {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a sheet thickness that is not finite and positive",
        ));
    }
    Ok((first, last))
}

/// The bodies of `spot`, placed at the spot.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite point, a zero or non-finite normal, fewer
/// than two sheets, a thickness or diameter that is not finite and positive, a style with
/// no piece or a fraction out of its range (see [`SpotStyle`]), a ball on more than two
/// sheets, and for sizes the model cannot hold: a
/// diameter or sheet so small that a piece is thinner than the zone tolerance, or so large
/// that the spot passes [`MAX_COORDINATE_MAGNITUDE`]. The errors of the sweeps, the sphere
/// and the placement otherwise.
pub fn spot_weld(k: &mut Kernel, spot: &SpotWeld) -> Result<Vec<SpotPiece>> {
    let (first, last) = check_sheets(&spot.thicknesses)?;
    if !positive(spot.diameter) {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a spot diameter that is not finite and positive",
        ));
    }
    let finite = |p: Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    if !finite(spot.at) || !finite(spot.normal) {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a spot point or normal that is not finite",
        ));
    }
    if spot.normal.length_and_unit().is_none() {
        return Err(SolidError::invalid_input(OP_SPOT, "a zero normal"));
    }
    check_style(&spot.style)?;
    if spot.style.ball.is_some() && spot.thicknesses.len() > 2 {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a spot ball on more than two sheets",
        ));
    }
    let layout = Layout::of(spot, first, last);
    check_sizes(k, spot, &layout)?;
    let mut pieces = local_pieces(k, &layout)?;
    let mut placement = Axis2Placement::default();
    placement.set_step_canonical(spot.at, spot.normal, Vec3::X)?;
    for piece in &mut pieces {
        transform(k, &mut piece.body, &placement, None, false)?;
    }
    Ok(pieces)
}

fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// Refuses a style with no piece, or a fraction out of its range.
fn check_style(style: &SpotStyle) -> Result<()> {
    let bad = |reason| Err(SolidError::invalid_input(OP_SPOT, reason));
    if style.marks.is_none() && style.ball.is_none() && style.pin.is_none() {
        return bad("a spot style with no marks, ball or pin");
    }
    if let Some(m) = style.marks {
        if !(positive(m.core) && positive(m.band) && positive(m.edge))
            || m.core >= m.band
            || m.band >= m.edge
        {
            return bad("spot marks whose diameters are not positive and increasing");
        }
        match m.top {
            MarkTop::Flat { thickness } if !positive(thickness) => {
                return bad("flat spot marks whose thickness is not positive");
            }
            MarkTop::Dish { rim, middle }
                if !(positive(rim) && positive(middle) && middle < rim) =>
            {
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
    if let Some(p) = style.pin {
        if !positive(p.diameter) || p.dish.is_some_and(|c| !positive(c)) {
            return bad("a spot pin whose diameter or dish is not positive");
        }
    }
    Ok(())
}

/// The marks' top in model units.
#[derive(Debug, Clone, Copy)]
enum Top {
    /// Flat, this high off the face.
    Flat(f64),
    /// Dished: `rim` and `middle` off the face, the sphere's radius `rs` and the height of
    /// its centre off the face.
    Dish {
        rim: f64,
        middle: f64,
        rs: f64,
        centre: f64,
    },
}

/// Every size of a spot in model units, local frame: the first faying face `z = 0`, the
/// last sheet's outer face `z = t_u`, the first sheet's outer face `z = -t_l`.
struct Layout {
    t_u: f64,
    t_l: f64,
    /// The thinnest sheet.
    thinnest: f64,
    /// Zone radii: core, band, edge.
    zones: Option<[f64; 3]>,
    /// The marks' top.
    mark_top: Option<Top>,
    /// Marks on the upper and on the lower face.
    marks_on: [bool; 2],
    ball: Option<f64>,
    pin: Option<PinLayout>,
}

/// A pin's radius, and each end's dish (upper, lower): its depth and its sphere's radius.
#[derive(Debug, Clone, Copy)]
struct PinLayout {
    r: f64,
    ends: [Option<(f64, f64)>; 2],
}

impl Layout {
    /// `first` and `last` are the outer sheets' thicknesses.
    fn of(spot: &SpotWeld, first: f64, last: f64) -> Self {
        let (d, t) = (spot.diameter, &spot.thicknesses);
        let (t_l, t_u) = (first, t.iter().skip(1).sum::<f64>());
        let style = &spot.style;
        let ball = style.ball.map(|b| b.diameter * d / 2.0);
        let zones = style
            .marks
            .map(|m| [m.core * d / 2.0, m.band * d / 2.0, m.edge * d / 2.0]);
        let mark_top = style.marks.map(|m| match m.top {
            MarkTop::Flat { thickness } => Top::Flat(thickness * d),
            MarkTop::Dish { rim, middle } => {
                let (rim, middle) = (rim * d, middle * d);
                let ro = m.edge * d / 2.0;
                let c = rim - middle;
                let rs = (ro * ro + c * c) / (2.0 * c);
                Top::Dish {
                    rim,
                    middle,
                    rs,
                    centre: middle + rs,
                }
            }
        });
        let shows = |t: f64| ball.is_some_and(|r| r > t);
        let marks_on = match style.marks.map(|m| m.faces) {
            None => [false, false],
            Some(MarkFaces::Both) => [true, true],
            Some(MarkFaces::WhereBallHidden) => [!shows(last), !shows(first)],
        };
        let pin = style.pin.map(|p| {
            let r = p.diameter * d / 2.0;
            let end = |t: f64| {
                p.dish.map(|f| {
                    let c = (f * t).min(r);
                    (c, (r * r + c * c) / (2.0 * c))
                })
            };
            PinLayout {
                r,
                ends: [end(last), end(first)],
            }
        });
        Self {
            t_u,
            t_l,
            thinnest: t.iter().fold(f64::INFINITY, |m, &v| m.min(v)),
            zones,
            mark_top,
            marks_on,
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
        match self.mark_top {
            Some(Top::Flat(h)) => out.push(h),
            Some(Top::Dish {
                rim, middle, rs, ..
            }) => out.extend([rim, middle, rim - middle, rs]),
            None => {}
        }
        out.extend(self.ball);
        if let Some(PinLayout { r, ends }) = self.pin {
            out.push(r);
            for (c, rs) in ends.into_iter().flatten() {
                out.extend([c, rs]);
            }
        }
        out
    }
}

/// Refuses sizes the model cannot hold, before any piece is built (a primitive would
/// refuse them with its own, misleading reason).
fn check_sizes(k: &Kernel, spot: &SpotWeld, layout: &Layout) -> Result<()> {
    let lengths = layout.lengths();
    let largest = lengths.iter().fold(0.0_f64, |m, &l| m.max(l));
    let reach = spot.at.x.abs().max(spot.at.y.abs()).max(spot.at.z.abs()) + largest;
    if lengths.iter().any(|l| !l.is_finite()) || reach > MAX_COORDINATE_MAGNITUDE {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a spot diameter or sheet so large that the spot passes the model's size limit",
        ));
    }
    if let Some(PinLayout {
        ends: [upper, lower],
        ..
    }) = layout.pin
    {
        let depth = |end: Option<(f64, f64)>| end.map_or(0.0, |(c, _)| c);
        let between = (layout.t_u - depth(upper)) - (-layout.t_l + depth(lower));
        if between < k.tolerances.zone_tol_3d {
            return Err(SolidError::invalid_input(
                OP_SPOT,
                "a spot pin whose dished ends meet or cross on its axis",
            ));
        }
    }
    let smallest = lengths.iter().fold(f64::INFINITY, |m, &l| m.min(l));
    if smallest < k.tolerances.zone_tol_3d {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a spot diameter or sheet so small that a piece of the spot is thinner than the zone tolerance",
        ));
    }
    Ok(())
}

/// The pieces in the local frame.
fn local_pieces(k: &mut Kernel, layout: &Layout) -> Result<Vec<SpotPiece>> {
    let (t_u, t_l) = (layout.t_u, layout.t_l);
    let mut pieces = Vec::new();
    let inside = |body| SpotPiece {
        body,
        zone: SpotZone::Inside,
        side: SpotSide::Through,
    };
    if let Some(r) = layout.ball {
        pieces.push(inside(create_sphere(k, Point3::new(0.0, 0.0, 0.0), r)?));
    }
    if let Some(PinLayout {
        r,
        ends: [upper, lower],
    }) = layout.pin
    {
        pieces.push(inside(pin(k, r, t_u, t_l, upper, lower)?));
    }
    if let (Some(zones), Some(top)) = (layout.zones, layout.mark_top) {
        let faces = [(SpotSide::Upper, t_u, 1.0), (SpotSide::Lower, -t_l, -1.0)];
        for ((side, face, sense), on) in faces.into_iter().zip(layout.marks_on) {
            if !on {
                continue;
            }
            let mut inner = 0.0;
            for (outer, zone) in
                zones
                    .into_iter()
                    .zip([SpotZone::Core, SpotZone::Band, SpotZone::Edge])
            {
                let body = zone_body(k, inner, outer, face, sense, top)?;
                pieces.push(SpotPiece { body, zone, side });
                inner = outer;
            }
        }
    }
    Ok(pieces)
}

/// The line from `a` to `b`, each `(ρ, z)` in the profile plane (local XZ).
fn line(a: (f64, f64), b: (f64, f64)) -> Result<Curve> {
    let p = |(r, z): (f64, f64)| Point3::new(r, 0.0, z);
    Ok(Curve::from(BSplineCurve::create_line_segment(
        3,
        p(a),
        p(b),
    )?))
}

/// The arc of radius `rs` about the axis point `z = centre` in the profile plane, from
/// angle `from` to `to` (degrees from the radial direction towards +Z). The angles must
/// ascend: an arc given from the larger angle makes a revolved ring whose loops fail the
/// B-rep check.
fn arc(centre: f64, rs: f64, from: f64, to: f64) -> Result<Curve> {
    let mut frame = Axis2Placement::default();
    frame.set_canonical(Point3::new(0.0, 0.0, centre), Vec3::X, Vec3::Z)?;
    Ok(Curve::from(BSplineCurve::create_circle_segment(
        3,
        &frame,
        rs,
        from.min(to),
        from.max(to),
        NurbCircleParam::Quadratic,
    )?))
}

/// The body swept by revolving the closed profile `curves` once about the local Z axis.
fn revolve(k: &mut Kernel, curves: &[Curve]) -> Result<Brep> {
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_rotational_sweep(k, &refs, Point3::new(0.0, 0.0, 0.0), Vec3::Z, 360.0, true)
}

/// A mark zone between radii `inner` and `outer` on the outer face at `face`, standing
/// out along `sense`: flat, or dished by a sphere about the axis.
fn zone_body(
    k: &mut Kernel,
    inner: f64,
    outer: f64,
    face: f64,
    sense: f64,
    top: Top,
) -> Result<Brep> {
    let mut curves = vec![line((inner, face), (outer, face))?];
    match top {
        Top::Flat(h) => {
            let z = face + sense * h;
            curves.push(line((outer, face), (outer, z))?);
            curves.push(line((outer, z), (inner, z))?);
            curves.push(line((inner, z), (inner, face))?);
        }
        Top::Dish { rs, centre, .. } => {
            // the dish is the sphere's side nearest the face
            let height = |rho: f64| face + sense * (centre - (rs * rs - rho * rho).sqrt());
            let angle = |rho: f64| -sense * (rho / rs).acos().to_degrees();
            let centre = face + sense * centre;
            curves.push(line((outer, face), (outer, height(outer)))?);
            curves.push(arc(centre, rs, angle(inner), angle(outer))?);
            curves.push(line((inner, height(inner)), (inner, face))?);
        }
    }
    revolve(k, &curves)
}

/// The pin of radius `r` from `z = -t_l` to `z = t_u`, each end flat or dished by the
/// `(depth, sphere radius)` given.
fn pin(
    k: &mut Kernel,
    r: f64,
    t_u: f64,
    t_l: f64,
    upper: Option<(f64, f64)>,
    lower: Option<(f64, f64)>,
) -> Result<Brep> {
    let mut curves = vec![line((r, -t_l), (r, t_u))?];
    match upper {
        None => curves.push(line((r, t_u), (0.0, t_u))?),
        Some((c, rs)) => {
            let angle = -(r / rs).acos().to_degrees();
            curves.push(arc(t_u - c + rs, rs, -90.0, angle)?);
        }
    }
    let (top, bottom) = (
        upper.map_or(t_u, |(c, _)| t_u - c),
        lower.map_or(-t_l, |(c, _)| -t_l + c),
    );
    curves.push(line((0.0, top), (0.0, bottom))?);
    match lower {
        None => curves.push(line((0.0, -t_l), (r, -t_l))?),
        Some((c, rs)) => {
            let angle = (r / rs).acos().to_degrees();
            curves.push(arc(-t_l + c - rs, rs, angle, 90.0)?);
        }
    }
    revolve(k, &curves)
}
