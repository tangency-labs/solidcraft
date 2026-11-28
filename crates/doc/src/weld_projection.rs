//! Projection welds: the display bodies of one projection weld, one or several projections
//! through a stack of lapped sheets ([`projection_weld`]), where the projections picked on a
//! sheet's outer face sit ([`projection_site`]), and the projection size ISO 8167 proposes
//! ([`proposed_projection`]).
//!
//! SolidCraft's own, as [`crate::weld_spot`] and [`crate::weld_seam`] are: every measurement and
//! every body is a kernel call; this module adds the weld's rules and sizes. The `weld.projection`
//! command and its storage come with #15131.
//!
//! # The geometry
//!
//! Each projection is drawn in the style of spot look A ([`MARKS`]) on both outer faces: a core,
//! a band and an edge. The band covers what the projection covers, ISO 8167's sizes:
//!
//! - **round** (spherical, SP): a spot of the projection's diameter `d1` ([`spot_weld`]);
//! - **elongated** (EP): an oval `b1` wide and `l1` long overall, along a direction in the face;
//! - **ring** (RP): a ring from the inner diameter `di` out to `d1`, round its centre line.
//!
//! As an option, one outer face shows the pressed projection's dimple instead: the marks dished
//! as in spot look F ([`SpotLook::Dent`]), following the projection's shape. Display only: the
//! parts are never changed, and nothing here is volume or mass.
//!
//! The oval and the ring are profiles across their centre line, extruded along the oval's
//! straight part, revolved a whole turn round each of its ends and cut in half by a box, or revolved
//! a whole turn round the ring's axis, all in a local frame (the projection at the origin, its normal along +Z, an oval
//! along +X), then placed. A dished top is the arc through its rims and its middle
//! ([`curve::arc_through`]), cut where each zone ends ([`curve::closest_to_ray`]).
//!
//! The drawn weld is the projection's own size. shortcut: the real weld can be larger or smaller
//! than the projection, set by the process; ISO 16432 §8.2.1 gives the weld diameter, and its rule
//! replaces this once it is read.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::Vec3;
use solidcore_record::brep::Curve as RecordCurve;
use solidcraft_kernel::curves as curve;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, BoolOp, KernelError, affine, box_solid, transform_matrix};

use crate::weld_seam::{SeamPiece, SeamRefusal, seam_site};
use crate::weld_spot::{
    MARKS, MarkFaces, MarkTop, SpotLook, SpotMarks, SpotPiece, SpotRefusal, SpotSheet, SpotSide, SpotStyle, SpotWeld, SpotZone, check_sheets,
    positive, spot_site, spot_weld,
};

/// Why a projection weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProjectionRefusal {
    /// Input the weld cannot take: a size, point, normal or direction out of range, no
    /// projections, or sizes the model cannot hold.
    #[error("{0}")]
    Invalid(&'static str),
    /// Projection `projection`, as a spot, is refused: its site, or its bodies.
    #[error("projection {projection}: {why}")]
    AtProjection { projection: usize, why: SpotRefusal },
    /// Elongated projection `projection`, as a short seam along its length, is refused.
    #[error("projection {projection}: {why}")]
    AlongProjection { projection: usize, why: SeamRefusal },
    /// Projection `projection` lies on other sheets or faces than the first.
    #[error("projection {projection} lies on other sheets or faces than the first")]
    OnOtherSheets { projection: usize },
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

type Result<T> = std::result::Result<T, ProjectionRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(ProjectionRefusal::Invalid(why))
}

/// A projection's shape and ISO 8167 sizes (§3, Tables 1 to 3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectionShape {
    /// Spherical (SP): the projection diameter `d1`.
    Round { d1: f64 },
    /// Elongated (EP): the width `b1` and the overall length `l1`, along `along` (a direction in
    /// the face; need not be unit).
    Elongated { b1: f64, l1: f64, along: [f64; 3] },
    /// Ring-shaped (RP): the outer diameter `d1` and the inner diameter `di`.
    Ring { d1: f64, di: f64 },
}

/// One projection: its centre and shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// The centre, on the faying face between the first two sheets ([`projection_weld`]) or on
    /// the picked face ([`projection_site`]).
    pub at: [f64; 3],
    pub shape: ProjectionShape,
}

/// One projection weld.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionWeld {
    pub projections: Vec<Projection>,
    /// The faying face's normal, from the first sheet into the next (need not be unit).
    pub normal: [f64; 3],
    /// Every sheet's thickness, in stack order along `normal`: at least two.
    pub thicknesses: Vec<f64>,
    /// The outer face that shows the projections' dimple, if any; look A marks on the other.
    pub dimple: Option<SpotSide>,
}

/// The bodies of `weld`, projection by projection.
///
/// # Errors
/// [`ProjectionRefusal::Invalid`] for no projections, a centre, direction or normal that is not
/// finite, a zero normal or direction, fewer than two sheets, a thickness or size that is not
/// finite and positive, a ring whose inner diameter is not under its outer, an oval not longer
/// than wide, a dimple on no outer face, and sizes the model cannot hold;
/// [`ProjectionRefusal::AtProjection`] for a round projection the spot refuses. The errors of the
/// kernel calls otherwise.
pub fn projection_weld(weld: &ProjectionWeld) -> Result<Vec<Vec<SpotPiece>>> {
    if weld.projections.is_empty() {
        return bad("a projection weld with no projections");
    }
    let (first, _) = check_sheets(&weld.thicknesses)
        .map_err(|_| ProjectionRefusal::Invalid("fewer than two sheets, or a thickness that is not finite and positive"))?;
    if weld.normal == [0.0; 3] || !weld.normal.iter().all(|v| v.is_finite()) {
        return bad("a projection weld normal that is zero or not finite");
    }
    if weld.dimple == Some(SpotSide::Through) {
        return bad("a dimple on no outer face");
    }
    let t_u: f64 = weld.thicknesses.iter().skip(1).sum();
    let tol = profiles::zone_tolerance()?;
    let mut out = Vec::new();
    for (i, p) in weld.projections.iter().enumerate() {
        if !p.at.iter().all(|v| v.is_finite()) {
            return bad("a projection centre that is not finite");
        }
        out.push(match (p.shape, laid(p.shape, p.at, t_u, first, weld.dimple, tol)?) {
            (ProjectionShape::Round { d1 }, _) => round(weld, p.at, d1).map_err(|why| ProjectionRefusal::AtProjection { projection: i, why })?,
            (ProjectionShape::Elongated { along, .. }, Some((laid, c))) => {
                laid.oval(c, &placement(p.at, weld.normal, in_face(weld.normal, along)?)?)?
            }
            (ProjectionShape::Ring { .. }, Some((laid, rc))) => laid.ring(rc, &placement(p.at, weld.normal, [1.0, 0.0, 0.0])?)?,
            (_, None) => return bad("an oval or a ring projection without its sizes"),
        });
    }
    Ok(out)
}

/// A round projection: the spot's own bodies at `d1`, look A, the dimple's face dished as look F.
fn round(weld: &ProjectionWeld, at: [f64; 3], d1: f64) -> std::result::Result<Vec<SpotPiece>, SpotRefusal> {
    let spot = |marks: SpotMarks| SpotWeld {
        at,
        normal: weld.normal,
        thicknesses: weld.thicknesses.clone(),
        diameter: d1,
        style: SpotStyle { marks: Some(marks), ball: None, pin: None },
    };
    let dent = SpotLook::Dent.style().marks.unwrap_or(MARKS);
    let (upper, lower) = match weld.dimple {
        Some(SpotSide::Upper) => (dent, MARKS),
        Some(SpotSide::Lower) => (MARKS, dent),
        Some(SpotSide::Through) | None => return spot_weld(&spot(MARKS)),
    };
    let mut pieces = spot_weld(&spot(SpotMarks { faces: MarkFaces::Upper, ..upper }))?;
    pieces.extend(spot_weld(&spot(SpotMarks { faces: MarkFaces::Lower, ..lower }))?);
    Ok(pieces)
}

/// The least share of an elongated projection's direction that must lie in the face: the sine of
/// its angle off the normal. Less than that, the direction is the normal but for rounding, and the
/// oval would run where the kernel chooses, not where the user said.
const IN_FACE: f64 = 1e-6;

/// `along` as a unit direction in the face square to `normal`. Refused when it is zero or not
/// finite, or runs along `normal`: its part in the face less than [`IN_FACE`] of its length. Made
/// unit first, so a short direction in the face is kept, never replaced.
fn in_face(normal: [f64; 3], along: [f64; 3]) -> Result<[f64; 3]> {
    if !normal.iter().chain(&along).all(|v| v.is_finite()) || along == [0.0; 3] {
        return bad("a projection direction that is zero or not finite");
    }
    let Some(a) = affine::unit_axis_direction(along, f64::MIN_POSITIVE) else {
        return bad("a projection direction that is zero or not finite");
    };
    let [_, u] = affine::orthonormal_axes(normal, a)?;
    if affine::distance_along([0.0; 3], u, a).abs() < IN_FACE {
        return bad("an elongated projection whose direction runs along the normal, not in the face");
    }
    Ok(u)
}

/// The sizes of projection `shape` at `at`, checked as both [`projection_weld`] and
/// [`projection_site`] check them: an oval's width `b1` and the half length `c` of its centre line,
/// or a ring's width `(d1 - di) / 2` and centre radius `(d1 + di) / 4`, with room inside for its
/// marks; each within the model's size limit ([`Laid::check`]). `None` for a round projection,
/// which the spot checks.
fn laid(shape: ProjectionShape, at: [f64; 3], t_u: f64, t_l: f64, dimple: Option<SpotSide>, tol: f64) -> Result<Option<(Laid, f64)>> {
    let (d, extent) = match shape {
        ProjectionShape::Round { .. } => return Ok(None),
        ProjectionShape::Elongated { b1, l1, .. } => {
            if !(positive(b1) && positive(l1)) || l1 - b1 <= tol {
                return bad("an elongated projection whose width and length are not finite and positive, or not longer than wide");
            }
            (b1, (l1 - b1) / 2.0)
        }
        ProjectionShape::Ring { d1, di } => {
            if !(positive(d1) && positive(di)) || di >= d1 {
                return bad("a ring projection whose diameters are not finite and positive, or whose inner is not under its outer");
            }
            let (d, rc) = ((d1 - di) / 2.0, (d1 + di) / 4.0);
            if rc - MARKS.edge * d / 2.0 <= tol {
                return bad("a ring projection too narrow inside for its marks");
            }
            (d, rc)
        }
    };
    let laid = Laid { d, at, t_u, t_l, dimple, tol };
    laid.check(extent)?;
    Ok(Some((laid, extent)))
}

/// The map from the local frame (the projection at the origin, its normal along +Z, `along` made
/// square to it along +X) to the projection.
fn placement(at: [f64; 3], normal: [f64; 3], along: [f64; 3]) -> Result<affine::Matrix> {
    if !normal.iter().chain(&along).all(|v| v.is_finite()) || along == [0.0; 3] {
        return bad("a projection direction that is zero or not finite");
    }
    let [n, u] = affine::orthonormal_axes(normal, along)?;
    let w = affine::apply_point(&affine::rotation([0.0; 3], n, FRAC_PI_2)?, u);
    Ok([[u[0], u[1], u[2], 0.0], [w[0], w[1], w[2], 0.0], [n[0], n[1], n[2], 0.0], [at[0], at[1], at[2], 1.0]])
}

/// The marks' top in model units on one face: flat, this high; or dished, `rim` off the face at
/// the edge zone's radius and `middle` on the centre line.
#[derive(Debug, Clone, Copy)]
enum Top {
    Flat(f64),
    Dish { rim: f64, middle: f64 },
}

/// An oval's or a ring's sizes, local frame: the faying face `z = 0`, the upper outer face
/// `z = t_u`, the lower `z = -t_l`; `d` the width the zones are fractions of.
struct Laid {
    d: f64,
    at: [f64; 3],
    t_u: f64,
    t_l: f64,
    dimple: Option<SpotSide>,
    tol: f64,
}

/// Where a profile lies: `base + side · s + Z · z` for the distance `s` across the centre line
/// and the height `z`.
#[derive(Clone, Copy)]
struct Plane {
    base: [f64; 3],
    side: [f64; 3],
}

impl Plane {
    fn at(self, s: f64, z: f64) -> [f64; 3] {
        [self.base[0] + self.side[0] * s, self.base[1] + self.side[1] * s, self.base[2] + self.side[2] * s + z]
    }
}

impl Laid {
    /// The zones' radii: 0, core, band, edge.
    fn radii(&self) -> [f64; 4] {
        [0.0, MARKS.core * self.d / 2.0, MARKS.band * self.d / 2.0, MARKS.edge * self.d / 2.0]
    }

    /// Refuses sizes the model cannot hold: the marks reach `extent` (an oval's half length, a
    /// ring's centre radius) and then the width from the centre in the face.
    fn check(&self, extent: f64) -> Result<()> {
        let MarkTop::Flat { thickness } = MARKS.top else {
            return bad("projection marks that are not flat");
        };
        let reach = self.at.iter().fold(0.0_f64, |m, c| m.max(c.abs())) + extent + self.d + self.t_u + self.t_l;
        if !reach.is_finite() || reach > profiles::MAX_COORD {
            return bad("a projection so large that it passes the model's size limit");
        }
        if thickness * self.d < self.tol {
            return bad("a projection so small that its marks are thinner than the zone tolerance");
        }
        Ok(())
    }

    /// Each face's level, the way its marks stand out, which side it is and its top.
    fn faces(&self) -> Result<[(SpotSide, f64, f64, Top); 2]> {
        let (MarkTop::Flat { thickness }, Some(SpotMarks { top: MarkTop::Dish { rim, middle }, .. })) = (MARKS.top, SpotLook::Dent.style().marks)
        else {
            return bad("projection marks that are not flat, or a dent that is not dished");
        };
        let top =
            |side| if self.dimple == Some(side) { Top::Dish { rim: rim * self.d, middle: middle * self.d } } else { Top::Flat(thickness * self.d) };
        Ok([(SpotSide::Upper, self.t_u, 1.0, top(SpotSide::Upper)), (SpotSide::Lower, -self.t_l, -1.0, top(SpotSide::Lower))])
    }

    /// An oval: the centre line from `x = -c` to `c`. Each zone is its strips along the straight
    /// part and a half disc at each end: the disc turned a whole turn about the end (as the spot's
    /// marks are), less a box over the straight part. The pieces of a zone touch, never overlap, and
    /// stay separate bodies, as a spot's pieces do.
    fn oval(&self, c: f64, frame: &affine::Matrix) -> Result<Vec<SpotPiece>> {
        let r = self.radii();
        let strip = Plane { base: [-c, 0.0, 0.0], side: [0.0, 1.0, 0.0] };
        // the box reaches past the marks on both faces, and across the widest zone
        let (span, low) = (2.0 * r[3], -(self.t_l + self.d + 1.0));
        let height = self.t_u + self.t_l + 2.0 * (self.d + 1.0);
        // each end's profile starts pointing away from the straight part, so the revolve's seam lies
        // in the half that stays, square to the cut
        let ends = [(c, 1.0, [c - span, -span, low]), (-c, -1.0, [-c, -span, low])];
        let mut pieces = Vec::new();
        for (side, face, sense, top) in self.faces()? {
            for (k, zone) in [SpotZone::Core, SpotZone::Band, SpotZone::Edge].into_iter().enumerate() {
                let mut bodies = Vec::new();
                for (a, b) in across(r, k) {
                    bodies.push(profiles::extrude_curves(&self.profile(strip, a, b, face, sense, top)?, [1.0, 0.0, 0.0], 2.0 * c, true)?);
                }
                for (x, out, corner) in ends {
                    let end = Plane { base: [x, 0.0, 0.0], side: [out, 0.0, 0.0] };
                    let disc = profiles::revolve_curves(
                        &self.profile(end, r[k], r[k + 1], face, sense, top)?,
                        [x, 0.0, 0.0],
                        [0.0, 0.0, 1.0],
                        TAU,
                        true,
                        None,
                    )?;
                    let lo = Vec3::from(corner);
                    let straight = box_solid(lo, lo + Vec3::new(span, 2.0 * span, height))?;
                    bodies.push(
                        solidcraft_kernel::boolean(&disc, &straight, BoolOp::Cut)?.ok_or(ProjectionRefusal::Invalid("an oval's end left empty"))?,
                    );
                }
                for body in bodies {
                    let body = transform_matrix(&body, *frame)?;
                    pieces.push(SpotPiece { body, zone, side });
                }
            }
        }
        Ok(pieces)
    }

    /// A ring round the centre circle of radius `rc` about Z: each zone its profiles across the
    /// circle turned a whole turn.
    fn ring(&self, rc: f64, frame: &affine::Matrix) -> Result<Vec<SpotPiece>> {
        let r = self.radii();
        let across_ring = Plane { base: [rc, 0.0, 0.0], side: [1.0, 0.0, 0.0] };
        let mut pieces = Vec::new();
        for (side, face, sense, top) in self.faces()? {
            for (k, zone) in [SpotZone::Core, SpotZone::Band, SpotZone::Edge].into_iter().enumerate() {
                let mut bodies = Vec::new();
                for (a, b) in across(r, k) {
                    bodies.push(profiles::revolve_curves(
                        &self.profile(across_ring, a, b, face, sense, top)?,
                        [0.0; 3],
                        [0.0, 0.0, 1.0],
                        TAU,
                        true,
                        None,
                    )?);
                }
                pieces.push(self.placed(bodies, frame, zone, side)?);
            }
        }
        Ok(pieces)
    }

    /// One zone's bodies united and placed.
    fn placed(&self, bodies: Vec<Body>, frame: &affine::Matrix, zone: SpotZone, side: SpotSide) -> Result<SpotPiece> {
        let body = solidcraft_kernel::union_all(&bodies.iter().collect::<Vec<_>>())?;
        let body = transform_matrix(&body, *frame)?;
        Ok(SpotPiece { body, zone, side })
    }

    /// The closed profile from `s = a` to `b` across the centre line on `plane`, on the face at
    /// `face`, standing out along `sense` to `top`: the face, the far side, the top, the near side.
    fn profile(&self, plane: Plane, a: f64, b: f64, face: f64, sense: f64, top: Top) -> Result<Vec<Curve>> {
        let line = |s0: f64, z0: f64, s1: f64, z1: f64| Curve::Line { start: plane.at(s0, z0), end: plane.at(s1, z1) };
        let mut curves = vec![line(a, face, b, face)];
        match top {
            Top::Flat(h) => {
                let z = face + sense * h;
                curves.extend([line(b, face, b, z), line(b, z, a, z), line(a, z, a, face)]);
            }
            Top::Dish { rim, middle } => {
                let ro = self.radii()[3];
                let arc =
                    curve::arc_through([plane.at(-ro, face + sense * rim), plane.at(0.0, face + sense * middle), plane.at(ro, face + sense * rim)])?;
                let (ta, za) = where_at(&arc, plane, a, face, sense, self.tol)?;
                let (tb, zb) = where_at(&arc, plane, b, face, sense, self.tol)?;
                curves.push(line(b, face, b, zb));
                curves.extend(profiles::curve_pieces(&arc, [ta.min(tb), ta.max(tb)])?);
                curves.push(line(a, za, a, face));
            }
        }
        Ok(curves)
    }
}

/// Zone `k`'s spans across the centre line: the core one span through it, the band and the edge
/// one on each side.
fn across(r: [f64; 4], k: usize) -> Vec<(f64, f64)> {
    if k == 0 { vec![(-r[1], r[1])] } else { vec![(-r[k + 1], -r[k]), (r[k], r[k + 1])] }
}

/// The parameter and height (off the faying face, along Z) where the dish arc `arc` crosses the
/// line at `s` across `plane`, found from the face at `face` out along `sense`.
fn where_at(arc: &RecordCurve, plane: Plane, s: f64, face: f64, sense: f64, tol: f64) -> Result<(f64, f64)> {
    let found = curve::closest_to_ray(arc, plane.at(s, face), [0.0, 0.0, sense], tol)?;
    let Some(p) = found.filter(|p| p.distance <= tol) else {
        return bad("a dish whose height at a zone's edge was not found");
    };
    Ok((p.parameter, p.point[2]))
}

/// Where a projection weld sits: what [`ProjectionWeld`] needs, and the faces it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionSite {
    /// The projections, moved onto the faying face between the first two sheets.
    pub projections: Vec<Projection>,
    /// The weld's unit normal, from the picked face into the stack.
    pub normal: [f64; 3],
    /// The sheets in stack order along `normal`, the picked one first.
    pub sheets: Vec<SpotSheet>,
}

/// The site of `projections` picked on record face `face` of sheet `picked` (each centre on that
/// face), the sheets of the stack listed in `sheets` with their record orders (any order, at least
/// two).
///
/// A round or ring projection is a spot site of its outer diameter ([`spot_site`]); an elongated
/// one a seam site of its width along its straight part ([`seam_site`]). Every projection must lie
/// on the same sheets and faces, and its sizes are checked as [`projection_weld`] checks them. The
/// sheets are read, never changed.
///
/// # Errors
/// [`ProjectionRefusal::Invalid`] for no projections or a size or direction out of range, as
/// [`projection_weld`] refuses them;
/// [`ProjectionRefusal::AtProjection`] and [`ProjectionRefusal::AlongProjection`] with the spot's
/// or the seam's refusal; [`ProjectionRefusal::OnOtherSheets`]. The errors of the kernel calls
/// otherwise.
pub fn projection_site(sheets: &[&Body], picked: usize, face: usize, projections: &[Projection]) -> Result<ProjectionSite> {
    let tol = profiles::zone_tolerance()?;
    let mut first: Option<(Vec<SpotSheet>, [f64; 3])> = None;
    let mut moved = Vec::new();
    for (i, p) in projections.iter().enumerate() {
        let size = match p.shape {
            ProjectionShape::Round { d1 } | ProjectionShape::Ring { d1, .. } => d1,
            ProjectionShape::Elongated { b1, .. } => b1,
        };
        let site = spot_site(sheets, picked, face, p.at, size).map_err(|why| ProjectionRefusal::AtProjection { projection: i, why })?;
        let t_l = site.sheets.first().map_or(0.0, |s| s.thickness);
        let t_u: f64 = site.sheets.iter().skip(1).map(|s| s.thickness).sum();
        if let (ProjectionShape::Elongated { b1, along, .. }, Some((_, c))) = (p.shape, laid(p.shape, site.at, t_u, t_l, None, tol)?) {
            let u = in_face(site.normal, along)?;
            let end = |k: f64| [p.at[0] + u[0] * k, p.at[1] + u[1] * k, p.at[2] + u[2] * k];
            seam_site(sheets, picked, face, end(-c), &[SeamPiece::Line { to: end(c) }], b1)
                .map_err(|why| ProjectionRefusal::AlongProjection { projection: i, why })?;
        }
        match &first {
            None => first = Some((site.sheets.clone(), site.normal)),
            Some((sheets, _)) => {
                let same = sheets.len() == site.sheets.len()
                    && sheets.iter().zip(&site.sheets).all(|(a, b)| (a.sheet, a.near, a.far) == (b.sheet, b.near, b.far));
                if !same {
                    return Err(ProjectionRefusal::OnOtherSheets { projection: i });
                }
            }
        }
        moved.push(Projection { at: site.at, ..*p });
    }
    let Some((sheets, normal)) = first else {
        return bad("a projection weld with no projections");
    };
    Ok(ProjectionSite { projections: moved, normal, sheets })
}

/// What kind of projection a size is proposed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionKind {
    Spherical,
    Elongated,
    Ring,
}

/// A projection size ISO 8167 proposes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProposedProjection {
    /// The diameter `d1` (spherical, ring) or the width `b1` (elongated).
    pub size: f64,
    /// A ring's inner diameter `di` (Table 2).
    pub inner: Option<f64>,
    /// An elongated projection's length `l1`, least and most (Table 3).
    pub length: Option<[f64; 2]>,
}

/// ISO 8167:2021 Annex A, Table A.1, group B (standard applications): the thinner sheet's upper
/// bound and the projection diameter or width.
const GROUP_B: [(f64, f64); 7] = [(0.4, 1.6), (0.5, 2.0), (0.63, 2.5), (1.0, 3.2), (1.6, 4.0), (2.5, 5.0), (3.0, 6.3)];

/// ISO 8167:2021 Table 3: an elongated projection's width and its length, least and most.
const ELONGATED: [(f64, [f64; 2]); 9] = [
    (1.4, [2.8, 4.5]),
    (1.6, [3.2, 5.1]),
    (2.0, [4.0, 6.4]),
    (2.5, [5.0, 8.0]),
    (3.2, [6.4, 10.2]),
    (4.0, [8.0, 12.8]),
    (5.0, [10.0, 16.0]),
    (6.3, [12.6, 20.2]),
    (8.0, [16.0, 25.6]),
];

/// The projection ISO 8167:2021 proposes for the thinnest of `thicknesses` (the projection is made
/// in the thicker sheet with the thinner sheet's sizes, §5):
///
/// - spherical and elongated: Annex A Table A.1, group B; past 3 mm its largest value, 6.3; an
///   elongated projection's length range from Table 3;
/// - ring: Annex A Table A.2 (sheets over 0.5 up to 1.0 mm, as printed) with Table 2's inner
///   diameter; `None` outside it (the tool keeps the last size used, else the user types it).
///
/// A thickness within the zone tolerance of a table's boundary counts as on it: a measured
/// thickness carries rounding (a 1 mm sheet can measure `0.9999999999999998`).
///
/// # Errors
/// [`ProjectionRefusal::Invalid`] for fewer than two sheets, or a thickness that is not finite and
/// positive; the kernel's refusal of its zone tolerance.
pub fn proposed_projection(thicknesses: &[f64], kind: ProjectionKind) -> Result<Option<ProposedProjection>> {
    check_sheets(thicknesses).map_err(|_| ProjectionRefusal::Invalid("fewer than two sheets, or a thickness that is not finite and positive"))?;
    let t = thicknesses.iter().fold(f64::INFINITY, |m, &v| m.min(v));
    let tol = profiles::zone_tolerance()?;
    let group_b = GROUP_B.iter().find(|&&(upto, _)| t <= upto + tol).map_or(6.3, |&(_, size)| size);
    Ok(match kind {
        ProjectionKind::Spherical => Some(ProposedProjection { size: group_b, inner: None, length: None }),
        ProjectionKind::Elongated => {
            let length = ELONGATED.iter().find(|&&(b1, _)| b1 == group_b).map(|&(_, l)| l);
            Some(ProposedProjection { size: group_b, inner: None, length })
        }
        // Table A.2 as printed: 0.5 < t <= 0.8, 0.8 < t < 1.0, t = 1.0; Table 2's inner diameters
        ProjectionKind::Ring => match t {
            t if t > 0.5 + tol && t <= 0.8 + tol => Some(ProposedProjection { size: 3.0, inner: Some(1.6), length: None }),
            t if t > 0.8 + tol && t < 1.0 - tol => Some(ProposedProjection { size: 4.0, inner: Some(2.0), length: None }),
            t if (t - 1.0).abs() <= tol => Some(ProposedProjection { size: 4.5, inner: Some(2.0), length: None }),
            _ => None,
        },
    })
}

#[cfg(test)]
#[path = "weld_projection_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
