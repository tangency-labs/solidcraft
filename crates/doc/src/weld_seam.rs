//! Resistance seam welds: the display bodies of one seam weld along a path on a stack of lapped
//! sheets ([`seam_weld`]), and where a seam drawn on a sheet's outer face sits ([`seam_site`]).
//!
//! SolidCraft's own, as [`crate::weld_spot`] is: every measurement and every body is a kernel call;
//! this module adds the seam's rules and sizes. The `weld.seam` command and its storage come with
//! #15131.
//!
//! | Rust              | What it is                                                         |
//! |-------------------|--------------------------------------------------------------------|
//! | [`SeamWeld`]      | One seam: its path, which way, the sheets, the width, the look     |
//! | [`SeamPiece`]     | One piece of the path: a line, or an arc turning left or right     |
//! | [`SeamLook`]      | A continuous band, or spots at a pitch along the path              |
//! | [`seam_weld`]     | The seam's bodies, and a notice for every corner of its path        |
//! | [`seam_site`]     | The seam's frame and sheets, from a path drawn on a face           |
//! | [`SeamRefusal`]   | Why a seam is refused                                              |
//!
//! # The geometry
//!
//! The path lies on the *faying face* between the first two sheets, as a spot's point does, and
//! `normal` points from the first sheet into the next. The seam is drawn in the style of spot look
//! A ([`MARKS`]) on both outer faces: a core, a band and an edge, each the region within its
//! radius of the path (core `0.55 w / 2`, band `w / 2`, edge `1.05 w / 2`), flat, `0.006 w` thick.
//! So the band's ends are round, as a seam wheel's run-on and run-off leave them, and a sharp
//! corner of the path is rounded on its outer side.
//!
//! Each zone is built as the union of one outline per piece (lines and arcs offset from the
//! piece, round at both its ends), extruded off the face; the band and the edge then have the zone
//! inside them taken away. [`SeamLook::Spots`] puts discs of the same zones at a pitch along the
//! path instead, overlapping where the pitch is under the width. Every body is for display only,
//! as a spot's are; the bodies of one seam must never be united.
//!
//! # Not covered
//!
//! Flat sheets only, as for the spot. An arc of a whole circle is refused (give it as two arcs).

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, BoolOp, KernelError, affine};

use crate::weld_spot::{MARKS, MarkTop, SpotPiece, SpotRefusal, SpotSheet, SpotSide, SpotZone, check_sheets, positive, spot_site};

/// Why a seam is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SeamRefusal {
    /// Input the seam cannot take: a width, pitch, point, normal or path out of range, or sizes
    /// the model cannot hold.
    #[error("{0}")]
    Invalid(&'static str),
    /// The spot that joint `joint` of the path (0 its start, `i` the end of piece `i - 1`) would
    /// be is refused.
    #[error("at joint {joint} of the seam: {why}")]
    AtJoint { joint: usize, why: SpotRefusal },
    /// Joint `joint` of the path meets other sheets or faces than its start.
    #[error("joint {joint} of the seam lies on other sheets or faces than its start")]
    JointOnOtherFaces { joint: usize },
    /// Arc `piece` turns tighter than the seam's edge zone: its radius is no more than half the
    /// edge zone's width. `fits` is the largest seam width it takes.
    #[error("arc {piece} of the seam turns tighter than the seam is wide; at most {fits} fits")]
    ArcTighterThanSeam { piece: usize, fits: f64 },
    /// Along piece `piece`, a seam of `width` passes the boundary of record face `face` of sheet
    /// `sheet`: an edge lies closer to the path than half the width. `fits` is the largest width
    /// that fits there.
    #[error("along piece {piece}, a seam of width {width} passes the edge of face {face} of sheet {sheet}; at most {fits} fits")]
    PastFaceEdge { piece: usize, sheet: usize, face: usize, width: f64, fits: f64 },
    /// How far the edges of record face `face` of sheet `sheet` lie from piece `piece` was not
    /// found, so the seam's room cannot be checked.
    #[error("how far the edges of face {face} of sheet {sheet} lie from piece {piece} of the seam was not found")]
    EdgeNotMeasured { piece: usize, sheet: usize, face: usize },
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, SeamRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(SeamRefusal::Invalid(why))
}

/// Which way an arc of the path turns: about `normal` by the right-hand rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    /// Counter-clockwise about `normal`.
    Left,
    /// Clockwise about `normal`.
    Right,
}

/// One piece of a seam's path, from where the piece before it ends (the first from the path's
/// start).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeamPiece {
    /// A straight line to `to`.
    Line { to: [f64; 3] },
    /// An arc about `center` to `to`, turning as `turn` says; less than a whole circle.
    Arc { center: [f64; 3], to: [f64; 3], turn: Turn },
}

/// How a seam is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeamLook {
    /// One continuous band along the path (the default).
    Band,
    /// Spots of the seam's width along the path, the first at its start, one every `pitch`
    /// along it: overlapping where `pitch` is under the width, separate otherwise.
    Spots { pitch: f64 },
}

/// One seam weld.
#[derive(Debug, Clone, PartialEq)]
pub struct SeamWeld {
    /// Where the path starts, on the faying face between the first two sheets.
    pub start: [f64; 3],
    /// The path, piece by piece, on that face.
    pub path: Vec<SeamPiece>,
    /// That face's normal, from the first sheet into the next (need not be unit).
    pub normal: [f64; 3],
    /// Every sheet's thickness, in stack order along `normal`: at least two.
    pub thicknesses: Vec<f64>,
    /// The seam's width: the band's, or each spot's diameter.
    pub width: f64,
    pub look: SeamLook,
}

/// Something about a seam the user should know, though it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeamNotice {
    /// The path turns a sharp corner at joint `joint` (the end of piece `joint - 1`), by `angle`
    /// radians about `normal`: a seam wheel cannot follow it; the band is rounded there.
    Corner { joint: usize, angle: f64 },
}

/// A seam's bodies, and what the user should know about it.
#[derive(Debug)]
pub struct Seam {
    pub pieces: Vec<SpotPiece>,
    pub notices: Vec<SeamNotice>,
}

/// The bodies of `seam`.
///
/// # Errors
/// [`SeamRefusal::Invalid`] for a start, path point or normal that is not finite, a zero normal,
/// fewer than two sheets, a thickness, width or pitch that is not finite and positive, an empty
/// path, a line of no length, an arc whose ends are not as far from its centre, or of a whole
/// circle, and sizes the model cannot hold; [`SeamRefusal::ArcTighterThanSeam`] for an arc tighter
/// than the band. The errors of the kernel calls otherwise.
pub fn seam_weld(seam: &SeamWeld) -> Result<Seam> {
    let (first, _) =
        check_sheets(&seam.thicknesses).map_err(|_| SeamRefusal::Invalid("fewer than two sheets, or a thickness that is not finite and positive"))?;
    if !positive(seam.width) {
        return bad("a seam width that is not finite and positive");
    }
    if seam.normal == [0.0; 3] || !seam.normal.iter().all(|v| v.is_finite()) {
        return bad("a seam normal that is zero or not finite");
    }
    let MarkTop::Flat { thickness } = MARKS.top else {
        return bad("seam marks that are not flat");
    };
    let tol = profiles::zone_tolerance()?;
    let [n, _] = affine::orthonormal_axes(seam.normal, [1.0, 0.0, 0.0])?;
    let laid = lay(seam.start, &seam.path, n, tol)?;
    let w = seam.width;
    let h = thickness * w;
    let radii = [MARKS.core, MARKS.band, MARKS.edge].map(|f| f * w / 2.0);
    let t_u: f64 = seam.thicknesses.iter().skip(1).sum();
    let reach = laid.iter().map(Laid::reach).fold(0.0_f64, f64::max) + radii[2] + t_u + first + h;
    if !reach.is_finite() || reach > profiles::MAX_COORD {
        return bad("a seam so large that it passes the model's size limit");
    }
    if h < tol {
        return bad("a seam so narrow that its marks are thinner than the zone tolerance");
    }
    let notices = corners(&laid, n, tol / w)?;
    let quarter = affine::rotation([0.0; 3], n, FRAC_PI_2)?;
    let loops = |r: f64, lift: [f64; 3]| -> Result<Vec<Vec<Curve>>> {
        match seam.look {
            SeamLook::Band => laid.iter().enumerate().map(|(i, piece)| piece.outline(i, r, radii[2], lift, n, &quarter)).collect(),
            SeamLook::Spots { pitch } => {
                let [_, x] = affine::orthonormal_axes(n, [1.0, 0.0, 0.0])?;
                Ok(stations(&laid, pitch, tol)?.into_iter().map(|q| vec![disc(add(q, lift), n, x, r)]).collect())
            }
        }
    };
    let mut pieces = Vec::new();
    for (side, level, sense) in [(SpotSide::Upper, t_u, 1.0), (SpotSide::Lower, -first, -1.0)] {
        let lift = scale(n, level);
        let up = scale(n, sense);
        let mut solids = Vec::new();
        for r in radii {
            let bodies =
                loops(r, lift)?.iter().map(|outline| profiles::extrude_curves(outline, up, h, true)).collect::<std::result::Result<Vec<_>, _>>()?;
            solids.push(solidcraft_kernel::union_all(&bodies.iter().collect::<Vec<_>>())?);
        }
        let [core, band, edge] = <[Body; 3]>::try_from(solids).map_err(|_| SeamRefusal::Invalid("a seam zone not built"))?;
        let band_only = less(&band, &core)?;
        let edge_only = less(&edge, &band)?;
        pieces.extend([(core, SpotZone::Core), (band_only, SpotZone::Band), (edge_only, SpotZone::Edge)].map(|(body, zone)| SpotPiece {
            body,
            zone,
            side,
        }));
    }
    Ok(Seam { pieces, notices })
}

/// `a` less `b`, which lies inside it: never empty.
fn less(a: &Body, b: &Body) -> Result<Body> {
    solidcraft_kernel::boolean(a, b, BoolOp::Cut)?.ok_or(SeamRefusal::Invalid("a seam zone left empty"))
}

/// Where a seam sits: what [`SeamWeld`] needs, and the faces it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct SeamSite {
    /// Where the path starts, on the faying face between the first two sheets.
    pub start: [f64; 3],
    /// The path, moved onto that face.
    pub path: Vec<SeamPiece>,
    /// The seam's unit normal, from the picked face into the stack.
    pub normal: [f64; 3],
    /// The sheets in stack order along `normal`, the picked one first.
    pub sheets: Vec<SpotSheet>,
}

/// The site of a seam of `width` drawn from `start` along `path` on record face `face` of sheet
/// `picked`, the sheets of the stack listed in `sheets` (any order, at least two).
///
/// Every joint of the path (its start, and every piece's end) must be a spot site
/// ([`spot_site`]) on the same sheets and faces; the whole path must keep half the width from
/// every edge of every face it crosses, measured from each piece by the kernel's exact
/// curve-to-edges solve. The sheets are read, never changed: the solves run on each body's kernel
/// form, never on its record.
///
/// # Errors
/// [`SeamRefusal::Invalid`] for an empty path or a path point that is not finite, and the
/// refusals of [`seam_weld`] for the path itself, [`SeamRefusal::ArcTighterThanSeam`] included;
/// [`SeamRefusal::AtJoint`] with the spot's refusal at a joint; [`SeamRefusal::JointOnOtherFaces`],
/// [`SeamRefusal::PastFaceEdge`] and [`SeamRefusal::EdgeNotMeasured`] where the stack does not
/// take the seam. The errors of the kernel calls otherwise.
pub fn seam_site(sheets: &[&Body], picked: usize, face: usize, start: [f64; 3], path: &[SeamPiece], width: f64) -> Result<SeamSite> {
    site(sheets, picked, face, start, path, width, MARKS.edge * width / 2.0)
}

/// [`seam_site`] for a weld along the path whose widest part reaches `widest` from it: every arc
/// of the path must turn wider than that, by more than the zone tolerance, so the site never
/// takes an arc the weld's build refuses.
pub(crate) fn site(sheets: &[&Body], picked: usize, face: usize, start: [f64; 3], path: &[SeamPiece], width: f64, widest: f64) -> Result<SeamSite> {
    if path.is_empty() {
        return bad("a seam with no path");
    }
    let joints: Vec<[f64; 3]> = std::iter::once(start).chain(path.iter().map(SeamPiece::to)).collect();
    if !joints.iter().flatten().all(|v| v.is_finite()) {
        return bad("a seam path point that is not finite");
    }
    let at = |joint: usize, p: [f64; 3]| spot_site(sheets, picked, face, p, width).map_err(|why| SeamRefusal::AtJoint { joint, why });
    let first = at(0, start)?;
    for (joint, &p) in joints.iter().enumerate().skip(1) {
        let here = at(joint, p)?;
        let same = here.sheets.len() == first.sheets.len()
            && here.sheets.iter().zip(&first.sheets).all(|(a, b)| (a.sheet, a.near, a.far) == (b.sheet, b.near, b.far));
        if !same {
            return Err(SeamRefusal::JointOnOtherFaces { joint });
        }
    }
    let tol = profiles::zone_tolerance()?;
    let n = first.normal;
    let laid = lay(start, path, n, tol)?;
    // an arc the build refuses: tighter than the weld's widest part
    for (i, piece) in laid.iter().enumerate() {
        if let Laid::Arc { radius, .. } = *piece
            && radius - widest <= tol
        {
            return Err(SeamRefusal::ArcTighterThanSeam { piece: i, fits: radius * width / widest });
        }
    }

    // every face crossed, at its level along the line from the picked face
    let mut level = 0.0;
    let mut faces = Vec::new();
    for s in &first.sheets {
        faces.push((s.sheet, s.near, level));
        level += s.thickness;
        faces.push((s.sheet, s.far, level));
    }
    let mut tightest: Option<(f64, usize, usize, usize)> = None;
    for (i, piece) in laid.iter().enumerate() {
        for &(sheet, f, level) in &faces {
            let Some(b) = sheets.get(sheet) else {
                continue;
            };
            let edges = b.rim_edges(&[f])?;
            if edges.is_empty() {
                continue;
            }
            let (helper, edge) = piece.helper(scale(n, level), n, width, tol)?;
            let Some(c) = b.closest_to_edge_of(&[], &edges, &helper, edge)? else {
                return Err(SeamRefusal::EdgeNotMeasured { piece: i, sheet, face: f });
            };
            if tightest.is_none_or(|(room, ..)| c.distance < room) {
                tightest = Some((c.distance, i, sheet, f));
            }
        }
    }
    if let Some((room, piece, sheet, face)) = tightest.filter(|(room, ..)| 2.0 * room < width) {
        return Err(SeamRefusal::PastFaceEdge { piece, sheet, face, width, fits: 2.0 * room });
    }

    // the path moved from the picked face to the faying face
    let shift = sub(first.at, start);
    Ok(SeamSite { start: first.at, path: path.iter().map(|p| p.moved(shift)).collect(), normal: n, sheets: first.sheets })
}

impl SeamPiece {
    fn to(&self) -> [f64; 3] {
        match *self {
            Self::Line { to } | Self::Arc { to, .. } => to,
        }
    }

    fn moved(&self, by: [f64; 3]) -> Self {
        match *self {
            Self::Line { to } => Self::Line { to: add(to, by) },
            Self::Arc { center, to, turn } => Self::Arc { center: add(center, by), to: add(to, by), turn },
        }
    }
}

/// A piece of the path laid out: its ends `a` and `b`, its unit tangents there, and for an arc
/// its centre, axis (the normal, or the normal reversed for a right turn), unit directions from
/// the centre to its ends, radius and turn (`0 < sweep < 2π` about the axis).
pub(crate) enum Laid {
    Line { a: [f64; 3], b: [f64; 3], t: [f64; 3], length: f64 },
    Arc { a: [f64; 3], b: [f64; 3], c: [f64; 3], axis: [f64; 3], u: [[f64; 3]; 2], t: [[f64; 3]; 2], radius: f64, sweep: f64 },
}

/// Lays `path` out from `start` on the plane square to unit `n`.
pub(crate) fn lay(start: [f64; 3], path: &[SeamPiece], n: [f64; 3], tol: f64) -> Result<Vec<Laid>> {
    if path.is_empty() {
        return bad("a seam with no path");
    }
    if !start.iter().all(|v| v.is_finite()) {
        return bad("a seam start that is not finite");
    }
    let mut from = start;
    let mut out = Vec::new();
    for piece in path {
        let laid = match *piece {
            SeamPiece::Line { to } => {
                let d = sub(to, from);
                let length = len(d);
                if !length.is_finite() || length < tol {
                    return bad("a seam line of no length, or not finite");
                }
                let [_, t] = affine::orthonormal_axes(n, d)?;
                Laid::Line { a: from, b: to, t, length }
            }
            SeamPiece::Arc { center, to, turn } => {
                let axis = match turn {
                    Turn::Left => n,
                    Turn::Right => scale(n, -1.0),
                };
                let (v0, v1) = (sub(from, center), sub(to, center));
                let radius = len(v0);
                if !(radius.is_finite() && len(v1).is_finite()) || radius < tol || (radius - len(v1)).abs() > tol {
                    return bad("a seam arc whose ends are not as far from its centre, or not finite");
                }
                let [_, u0] = affine::orthonormal_axes(axis, v0)?;
                let [_, u1] = affine::orthonormal_axes(axis, v1)?;
                let turned = affine::signed_unit_angle(axis, u0, u1)?;
                // a turn the other way round the axis is the rest of the circle
                let sweep = if turned > 0.0 { turned } else { turned + TAU };
                if len(sub(to, from)) < tol {
                    return bad("a seam arc of a whole circle: give it as two arcs");
                }
                let quarter = affine::rotation([0.0; 3], axis, FRAC_PI_2)?;
                let t = [affine::apply_point(&quarter, u0), affine::apply_point(&quarter, u1)];
                Laid::Arc { a: from, b: to, c: center, axis, u: [u0, u1], t, radius, sweep }
            }
        };
        out.push(laid);
        from = piece.to();
    }
    Ok(out)
}

impl Laid {
    pub(crate) fn ends(&self) -> ([f64; 3], [f64; 3]) {
        match *self {
            Self::Line { a, b, .. } | Self::Arc { a, b, .. } => (a, b),
        }
    }

    pub(crate) fn tangents(&self) -> ([f64; 3], [f64; 3]) {
        match *self {
            Self::Line { t, .. } => (t, t),
            Self::Arc { t, .. } => (t[0], t[1]),
        }
    }

    fn length(&self) -> f64 {
        match *self {
            Self::Line { length, .. } => length,
            Self::Arc { radius, sweep, .. } => radius * sweep,
        }
    }

    /// How far the piece reaches from the origin along any axis, at most: a line's largest end
    /// coordinate; an arc's largest centre coordinate plus its radius, which bounds an arc that
    /// bulges past its ends.
    pub(crate) fn reach(&self) -> f64 {
        let largest = |p: [f64; 3]| p.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
        match *self {
            Self::Line { a, b, .. } => largest(a).max(largest(b)),
            Self::Arc { c, radius, .. } => largest(c) + radius,
        }
    }

    /// The closed outline of the region within `r` of piece `i`, round at both its ends, moved by
    /// `lift`: the piece offset to its left, a half circle round its end, the piece offset to its
    /// right back to its start, a half circle round its start. `edge` is the widest zone's radius,
    /// which an arc must turn wider than.
    fn outline(&self, i: usize, r: f64, edge: f64, lift: [f64; 3], n: [f64; 3], quarter: &affine::Matrix) -> Result<Vec<Curve>> {
        let (a, b) = self.ends();
        let (t0, t1) = self.tangents();
        // the left of the path, about n
        let (s0, s1) = (affine::apply_point(quarter, t0), affine::apply_point(quarter, t1));
        let back = scale(n, -1.0);
        let cap = |at: [f64; 3], from: [f64; 3]| Curve::Arc { center: add(at, lift), axis: back, reference: from, radius: r, start: 0.0, end: PI };
        let sides = match *self {
            Self::Line { .. } => {
                let (left, right) = (scale(s0, r), scale(s0, -r));
                [line(add(add(a, left), lift), add(add(b, left), lift)), line(add(add(b, right), lift), add(add(a, right), lift))]
            }
            Self::Arc { c, axis, u, radius, sweep, .. } => {
                if radius - edge <= 0.0 {
                    return Err(SeamRefusal::ArcTighterThanSeam { piece: i, fits: 2.0 * radius / MARKS.edge });
                }
                // turning left, the left side lies towards the centre
                let (l, rr) = if axis == n { (radius - r, radius + r) } else { (radius + r, radius - r) };
                let arc = |axis: [f64; 3], from: [f64; 3], radius: f64| Curve::Arc {
                    center: add(c, lift),
                    axis,
                    reference: from,
                    radius,
                    start: 0.0,
                    end: sweep,
                };
                [arc(axis, u[0], l), arc(scale(axis, -1.0), u[1], rr)]
            }
        };
        let [left, right] = sides;
        Ok(vec![left, cap(b, s1), right, cap(a, scale(s0, -1.0))])
    }

    /// A one-face planar sheet with the piece, moved by `lift`, as one of its edges, and that
    /// edge's record index: a line closed by a triangle standing off it along `n`, an arc by the
    /// two lines to its centre.
    pub(crate) fn helper(&self, lift: [f64; 3], n: [f64; 3], size: f64, tol: f64) -> Result<(Body, usize)> {
        let (a, b) = self.ends();
        let (a, b) = (add(a, lift), add(b, lift));
        let curves = match *self {
            Self::Line { .. } => {
                let apex = add(a, scale(n, size));
                vec![line(a, b), line(b, apex), line(apex, a)]
            }
            Self::Arc { c, axis, u, radius, sweep, .. } => {
                let c = add(c, lift);
                vec![Curve::Arc { center: c, axis, reference: u[0], radius, start: 0.0, end: sweep }, line(b, c), line(c, a)]
            }
        };
        let sheet = profiles::planar_faces(&curves)?;
        // the piece is the one edge that reaches both its ends
        for e in 0..sheet.edge_count() {
            let on = |p: [f64; 3]| -> Result<bool> { Ok(sheet.closest_to_point(&[], &[e], p)?.is_some_and(|c| c.distance <= tol)) };
            if on(a)? && on(b)? {
                return Ok((sheet, e));
            }
        }
        bad("a seam piece's helper sheet without the piece as an edge")
    }
}

/// A notice for every sharp corner between two pieces of `laid`: a turn of more than `limit`
/// radians about unit `n`.
pub(crate) fn corners(laid: &[Laid], n: [f64; 3], limit: f64) -> Result<Vec<SeamNotice>> {
    let mut out = Vec::new();
    for (i, pair) in laid.windows(2).enumerate() {
        let [before, after] = pair else {
            continue;
        };
        let angle = affine::signed_unit_angle(n, before.tangents().1, after.tangents().0)?;
        if angle.abs() > limit {
            out.push(SeamNotice::Corner { joint: i + 1, angle });
        }
    }
    Ok(out)
}

/// The spots' centres along `laid`: the path's start, then one every `pitch` along it.
fn stations(laid: &[Laid], pitch: f64, tol: f64) -> Result<Vec<[f64; 3]>> {
    if !positive(pitch) {
        return bad("a seam spot pitch that is not finite and positive");
    }
    let total: f64 = laid.iter().map(Laid::length).sum();
    if total / pitch > 1e5 {
        return bad("a seam spot pitch so small that the seam has too many spots");
    }
    let mut out = Vec::new();
    let mut begun = 0.0;
    let mut next = 0.0;
    for piece in laid {
        let length = piece.length();
        while next <= begun + length + tol {
            let along = (next - begun).min(length);
            out.push(match *piece {
                Laid::Line { a, t, .. } => add(a, scale(t, along)),
                Laid::Arc { a, c, axis, radius, .. } => affine::apply_point(&affine::rotation(c, axis, along / radius)?, a),
            });
            next += pitch;
        }
        begun += length;
    }
    // a station on a joint is counted by the piece it ends, not again by the next
    out.dedup_by(|p, q| len(sub(*p, *q)) <= tol);
    Ok(out)
}

/// A whole circle of radius `r` about `at`, square to unit `n`, from unit `x` square to it.
fn disc(at: [f64; 3], n: [f64; 3], x: [f64; 3], r: f64) -> Curve {
    Curve::Arc { center: at, axis: n, reference: x, radius: r, start: 0.0, end: TAU }
}

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|v| v * k)
}

/// The length of `v`. shortcut: plain arithmetic in the app (as the spot's sums are), not a
/// kernel call; move to solidcore if a reviewer asks.
fn len(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[cfg(test)]
#[path = "weld_seam_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
