//! A variable round's law corners (opt-in, not upstream): where the radius law has a
//! corner, the round's exact pieces on either side, how the corner ball's two touch
//! circles lie (a dip or a peak), the corner ball's zone at a dip, and the crossing curve
//! between rail points at a peak.
//!
//! Not in the pinned upstream: the SolidCraft fork's law corners (tangency-labs/smlib#79,
//! 7babe17, 58d03ef and 62fcc82), for `fixes.fillet_gn_section_generator` (WR-685's
//! law-corner addendum, #2273). The round is the union of its balls: each side of a law
//! corner is skinned on its own and ends on the corner ball's touch circle of its own
//! side, through the same two rail points. Where the law dips the circle after lies ahead
//! of the one before and the ball's own sphere fills the zone between them; where it
//! peaks the sides run past each other and meet along their crossing curve.
//!
//! | Rust                         | Fork                                                     |
//! |------------------------------|----------------------------------------------------------|
//! | [`LawCornerPieces`]          | `SmLawCornerPieces` (`SmFilletSolver.h`)                 |
//! | [`LawCornerKind`]            | `SmLawCornerPieces::SM_LC_*`                             |
//! | [`LawCorner`]                | one entry of `m_vKinds`, `m_vCentres` and `m_vRadii`     |
//! | [`LawCornerPieces::classify`]| `SmGnSectionFSG::CircleSurface`'s corner loop (`SmFilletSection.cpp`) |
//! | [`find_law_corner_pieces`]   | `SmFilletSolver::FindLawCornerPieces` (58d03ef)          |
//! | [`has_built_law_corners`]    | `SmFilletSolver::HasBuiltLawCorners` (58d03ef)           |
//! | [`sphere_zone`]              | `sm_SphereZone` (`SmFilletExecutive.cpp`)                |
//! | [`curve_between`]            | `sm_CurveBetween` (`SmFilletExecutive.cpp`)              |
//!
//! What is in, under `fixes.fillet_gn_section_generator`: 6eecf04's trace (the law's
//! corners, [`FilletLaw::corner_parameters`](crate::blend::fillet_standard_solver::p1::FilletLaw::corner_parameters),
//! are through points, and the fillet intersector steps onto them and flushes each span
//! with its own side's tangents, so the round's curves turn there), and its piecewise skin
//! (the round skinned piece by piece between the turns and joined C0 there,
//! [`CircularCrossSectionFsg::piecewise_surface`](crate::blend::fillet_standard_solver::p1::CircularCrossSectionFsg::piecewise_surface)),
//! whose pieces [`LawCornerPieces::classify`] keeps on the solver (the fork's
//! `AddLawCornerPieces`, 7babe17).
//!
//! The executive splits the round's faces at the corners after the merge
//! (`NoteRadiusCornerSurfaces`, `SplitFilletFacesAtRadiusCorners`, 6eecf04, on
//! `SmBrep::SubdivideFaceAtDiscontinuities` and `ReplaceCurve`) and, where a corner dips or
//! peaks, builds the round there as the union of its balls (`BuildLawCorners`, 7babe17
//! with 62fcc82 and 58d03ef): each side its piece, the zone at a dip, the crossing at a
//! peak (the live session's `law_corners`).
//!
//! Not the fork's (#430): where each piece passes a variable round's definition gate, the
//! pieces are rounds by their definitions, the law trimmed to each piece
//! ([`CircularCrossSectionFsg::piecewise_surface`](crate::blend::fillet_standard_solver::p1::CircularCrossSectionFsg::piecewise_surface)).
//!
//! Not ported yet: the `EaseSetBackSections` refusal of a law-cornered round (58d03ef).
//!
//! Deviations: out-params are return values; `SmLawCornerPieces::Copy` is `Clone`, and the
//! solver's list of kept pieces is a slice the caller owns, so
//! [`find_law_corner_pieces`] returns an index. The fork's three per-corner arrays
//! (`m_vKinds`, `m_vCentres`, `m_vRadii`) are one `Vec<LawCorner>`, so they cannot differ
//! in length. [`sphere_zone`] takes the corner ball's centre and radius, which the fork's
//! `sm_SphereZone` does not, and returns the zone as a [`BallPatch`] that keeps the
//! sphere, where the fork returns a bare `SmBSplineSurface`; the net is the fork's bit for
//! bit. The pieces are surfaces, each a round by its definition where it is one (not the
//! fork's, whose pieces are skins, `SmBSplineSurface`; #430), so the fork's per-piece `SER`
//! on a failed cast has no counterpart. No loop depends on input beyond the pieces and
//! knots given.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::{BSplineCurve, KnotType};
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use crate::surface::ball_patch::BallPatch;
use crate::surface::surface::Surface;

const OP_CLASSIFY: &str = "blend::law_corners::LawCornerPieces::classify";
const OP_ZONE: &str = "blend::law_corners::sphere_zone";

/// How a law corner's two touch circles lie (`SmLawCornerPieces::SM_LC_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LawCornerKind {
    /// `SM_LC_MATCHING`: the two sides end on the same circle (within the tolerance).
    Matching,
    /// `SM_LC_DIP`: the circle after lies ahead of the one before along the rail; the
    /// corner ball's sphere shows between them.
    Dip,
    /// `SM_LC_PEAK`: the circle after lies behind the one before; the sides run past
    /// each other and cross.
    Peak,
}

/// One corner of a radius law: how its touch circles lie, and its ball (the fork keeps the
/// three in parallel arrays of `SmLawCornerPieces`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LawCorner {
    /// `m_vKinds[i]`: how its touch circles lie.
    pub kind: LawCornerKind,
    /// `m_vCentres[i]`: the corner ball's centre.
    pub centre: Point3,
    /// `m_vRadii[i]`: the corner ball's radius.
    pub radius: f64,
}

/// `SmLawCornerPieces`: a round whose radius law has corners, as the exact pieces between
/// them, kept beside the surface joined from them.
#[derive(Debug, Clone, Default)]
pub struct LawCornerPieces {
    /// `m_bTaken`: matched to a fillet geom's surface already ([`find_law_corner_pieces`]).
    pub taken: bool,
    /// `m_vPieces`: the pieces in order along the round: each a round by its definition
    /// where it is one (not the fork's, #430), else its skin, a B-spline surface.
    pub pieces: Vec<Surface>,
    /// `m_vJoinedEnds`: each piece's end in the joined surface's u.
    pub joined_ends: Vec<f64>,
    /// `m_vKinds`, `m_vCentres` and `m_vRadii`: the corners in order along the round, one
    /// between each pair of pieces.
    pub corners: Vec<LawCorner>,
    /// `m_dTol3d`: how near the pieces' sections are to the balls' circles.
    pub tol_3d: f64,
}

/// The arc settings of the circular section the pieces were skinned with (the fork's
/// `SmArcSection`'s `bApproximateArcs` and `dTolPercentRadius`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcApproximation {
    /// `bApproximateArcs`: the arcs are approximated, not exact.
    pub approximate_arcs: bool,
    /// `dTolPercentRadius`: how far an approximated arc may be off, in percent of the
    /// radius.
    pub tol_percent_radius: f64,
}

impl LawCornerPieces {
    /// Whether a corner dips or peaks (`m_vKinds.IsIn(SM_LC_DIP) ||
    /// m_vKinds.IsIn(SM_LC_PEAK)`): the corners the executive builds.
    #[must_use]
    pub fn has_dip_or_peak(&self) -> bool {
        self.corners
            .iter()
            .any(|corner| matches!(corner.kind, LawCornerKind::Dip | LawCornerKind::Peak))
    }

    /// The corner loop of the fork's `SmGnSectionFSG::CircleSurface` (7babe17): `pieces`
    /// the round's pieces skinned between `ends` (the rail's parameters: its start, the
    /// law's corners, its end), `joined_ends` each piece's end in the joined surface's u.
    /// Each corner's ball is centred on `center_line` and runs through the rails; the
    /// side before ends on its touch circle at the corner, the side after starts on its
    /// own. Along `rail1`, the middle of the circle after lies ahead of the middle of the
    /// one before by more than `approx_tol_3d` where the law dips, behind it where it
    /// peaks. The pieces' accuracy is `approx_tol_3d`, or an approximated arc's share of
    /// the largest corner radius when that is more.
    ///
    /// # Errors
    ///
    /// `SM_ERR` where the rail has no direction at a corner or `ends` does not hold a
    /// corner for each pair of pieces; those of `Evaluate` and `EvaluatePoint`.
    pub fn classify(
        k: &Kernel,
        center_line: &BSplineCurve,
        rail1: &BSplineCurve,
        ends: &[f64],
        pieces: Vec<Surface>,
        joined_ends: Vec<f64>,
        approx_tol_3d: f64,
        arcs: ArcApproximation,
    ) -> Result<Self> {
        let mut kept = Self {
            pieces,
            joined_ends,
            ..Self::default()
        };
        for (i, pair) in kept.pieces.windows(2).enumerate() {
            let (Some(&t), [before, after]) = (ends.get(i + 1), pair) else {
                return Err(SolidError::sm_err(OP_CLASSIFY));
            };
            // the fork's `Evaluate(dT, 1, TRUE, ...)`, `bNonZeroTangents` at its default FALSE
            let cent_pv = center_line.evaluate(t, 1, true, false)?;
            let rail_pv = rail1.evaluate(t, 1, true, false)?;
            let (Some(&centre), Some(&rail_point), Some(&along)) =
                (cent_pv.first(), rail_pv.first(), rail_pv.get(1))
            else {
                return Err(SolidError::sm_err(OP_CLASSIFY));
            };
            if along.length() <= EFF_ZERO {
                return Err(SolidError::sm_err(OP_CLASSIFY));
            }
            let along = along / along.length();
            let mut mid = [Point3::ZERO; 2];
            for (j, (side, at)) in [before, after].into_iter().zip(&mut mid).enumerate() {
                let domain = side.natural_uv_domain_with_nurb(k)?;
                let uv = Point2::new(
                    if j == 0 {
                        domain.max().x
                    } else {
                        domain.min().x
                    },
                    0.5 * (domain.min().y + domain.max().y),
                );
                *at = side.evaluate_point(k, uv)?;
            }
            let [mid_before, mid_after] = mid;
            let ahead = (mid_after - mid_before).dot(along);
            let kind = if ahead > approx_tol_3d {
                LawCornerKind::Dip
            } else if ahead < -approx_tol_3d {
                LawCornerKind::Peak
            } else {
                LawCornerKind::Matching
            };
            kept.corners.push(LawCorner {
                kind,
                centre,
                radius: (rail_point - centre).length(),
            });
        }
        // the pieces' sections are the balls' circles within the skin's tolerance, and
        // within the arcs' own where they are approximated (a share of the radius)
        kept.tol_3d = approx_tol_3d;
        if arcs.approximate_arcs {
            for corner in &kept.corners {
                kept.tol_3d = smos_max(kept.tol_3d, 0.01 * arcs.tol_percent_radius * corner.radius);
            }
        }
        Ok(kept)
    }
}

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `SmFilletSolver::FindLawCornerPieces` (58d03ef): the kept pieces, not matched yet, of
/// a fillet surface that turns (a C0 knot across u) at each of their corners, marked
/// matched; `None` when there are none. They are matched by the corners, not by the
/// surface: a fillet geom's surface is replaced on the way (extended at its ends,
/// `SmFilletGeom`), keeping its corners, and an extension at an end may add a turn of its
/// own where it joins.
///
/// # Errors
///
/// Those of `FindKnotsByMaxContinuity`.
pub fn find_law_corner_pieces(
    k: &Kernel,
    kept: &mut [LawCornerPieces],
    surface: &Surface,
) -> Result<Option<usize>> {
    let (u_knots, _) = surface.find_knots_by_max_continuity(k, ContinuityType::C0, false)?;
    // the fork's virtual `GetNaturalUVDomain()`: for a fillet's B-spline surface, its own
    // domain
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let tol = 1.0e-9 * (1.0 + domain.min().x.abs() + domain.max().x.abs());
    for (i, pieces) in kept.iter_mut().enumerate() {
        if pieces.taken || pieces.joined_ends.len() < 2 {
            continue;
        }
        // each corner (the pieces' inner ends) a turn of the surface
        let corners = pieces.joined_ends.len() - 1;
        let same = pieces
            .joined_ends
            .iter()
            .take(corners)
            .all(|&end| u_knots.iter().any(|&knot| (knot - end).abs() <= tol));
        if same {
            pieces.taken = true;
            return Ok(Some(i));
        }
    }
    Ok(None)
}

/// `SmFilletSolver::HasBuiltLawCorners` (58d03ef): whether a round of the solver has a
/// law corner that dips or peaks.
#[must_use]
pub fn has_built_law_corners(kept: &[LawCornerPieces]) -> bool {
    kept.iter().any(LawCornerPieces::has_dip_or_peak)
}

/// `sm_SphereZone` (7babe17): the zone of a sphere between two of its circles that cross
/// at the same two points (a law dip's corner ball between its two sides' touch circles),
/// as one rational biquadratic patch kept with its sphere: the ball of centre `centre` and
/// radius `radius` that the four points lie on. u runs from the circle through `on_first`
/// (u = 0) to the one through `on_second` (u = 1), v from `two` (v = 0) to `one` (v = 1);
/// both v ends are poles.
///
/// Inverted about `one`, the sphere is a plane and the circles are lines through the
/// image `Q` of `two`; the zone is the wedge between the rays from `Q` that hold the arcs.
/// The ray turned by a rational quadratic unit vector `d(u)` and run out by
/// `t = v / (1 - v)`, inverted back, is quadratic in each parameter:
///
/// `X - one = k2 (W (Q - one) (1-v)^2 + D v (1-v)) / (W |Q - one|^2 (1-v)^2 + 2 (Q - one).D v (1-v) + W v^2)`
///
/// (`D = W d`, `W` the weight). The wedge must be under a half turn and the middle weights
/// positive; the zone is refused otherwise.
///
/// # Errors
///
/// `SM_ERR` where `one` and `two` coincide, an arc point inverts onto `Q`, the wedge is a
/// half turn or more, or a weight is not positive; those of `CreateCanonical` and
/// [`BallPatch::create`] (a centre not finite or a radius not positive).
pub fn sphere_zone(
    centre: Point3,
    radius: f64,
    one: Point3,
    two: Point3,
    on_first: Point3,
    on_second: Point3,
) -> Result<BallPatch> {
    let k2 = (two - one).dot(two - one);
    if k2 <= EFF_ZERO {
        return Err(SolidError::sm_err(OP_ZONE));
    }
    let invert = |x: Point3| {
        let d = x - one;
        one + d * (k2 / d.dot(d))
    };
    let q = invert(two);
    let mut first = invert(on_first) - q;
    let mut second = invert(on_second) - q;
    if first.length() <= EFF_ZERO || second.length() <= EFF_ZERO {
        return Err(SolidError::sm_err(OP_ZONE));
    }
    first = first / first.length();
    second = second / second.length();
    let cos = first.dot(second);
    // The middle control point below divides by 1 + cos, which carries an absolute
    // rounding error of a few ulps from the unit vectors, so its relative error is about
    // eps / (1 + cos). At EFF_ZERO that would be about 1e-4, a net visibly off its
    // sphere; the fork's EFF_ZERO_SQRT keeps it near 1e-10, and the middle weight
    // sqrt((1 + cos) / 2) at least 7e-4. It refuses wedges within about 1.4e-3 rad
    // (sqrt(2e-6)) of a half turn.
    if cos <= -1.0 + EFF_ZERO_SQRT {
        return Err(SolidError::sm_err(OP_ZONE));
    }
    // the unit vector's rational quadratic arc: its control points and weights
    let d = [first, (first + second) / (1.0 + cos), second];
    let w = [1.0, (0.5 * (1.0 + cos)).sqrt(), 1.0];
    let qo = q - one;
    let mut points = Vec::with_capacity(9);
    let mut weights = Vec::with_capacity(9);
    for (&di, &wi) in d.iter().zip(&w) {
        let dh = di * wi;
        let den = [wi * qo.dot(qo), qo.dot(dh), wi];
        let num = [qo * wi, dh * 0.5, Vec3::new(0.0, 0.0, 0.0)];
        for (&den_j, &num_j) in den.iter().zip(&num) {
            if den_j <= EFF_ZERO {
                return Err(SolidError::sm_err(OP_ZONE));
            }
            points.push(one + num_j * (k2 / den_j));
            weights.push(den_j);
        }
    }
    let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 2,
        v_degree: 2,
        control_points: &points,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &[3, 3],
        v_multiplicities: &[3, 3],
        u_knots: &[0.0, 1.0],
        v_knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&weights),
        uv_domain: None,
    })?;
    BallPatch::create(&net, centre, radius)
}

/// `sm_CurveBetween` (7babe17): a copy of `curve` running from `from` to `to` (reversed
/// if it runs the other way), `None` when its ends are not those points within `tol`.
///
/// # Errors
///
/// Those of `EvaluatePoint`, `Copy` and `ReverseParameterization`.
pub fn curve_between(
    k: &Kernel,
    curve: &Curve,
    from: Point3,
    to: Point3,
    tol: f64,
) -> Result<Option<Curve>> {
    let ivl = curve.natural_interval();
    let start = curve.evaluate_point(k, ivl.min())?;
    let end = curve.evaluate_point(k, ivl.max())?;
    let forward = start.closer_than(tol, from) && end.closer_than(tol, to);
    let backward = start.closer_than(tol, to) && end.closer_than(tol, from);
    if !forward && !backward {
        return Ok(None);
    }
    let mut copy = curve.copy(k)?;
    if !forward {
        let natural = copy.natural_interval();
        copy.reverse_parameterization(k, &natural)?;
    }
    Ok(Some(copy))
}
