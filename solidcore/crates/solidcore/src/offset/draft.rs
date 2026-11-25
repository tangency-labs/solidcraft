//! Not upstream (SolidCraft's draft, tangency-labs/smlib#27, #53): the surface a face of a
//! solid takes when it is drafted, turned by an angle about its line or circle on a neutral
//! plane so that it leans in going along a pull direction, kept with its definition.
//!
//! | Rust | SolidCraft |
//! |------|-----------|
//! | [`Draft`] | `smlib_draft`'s `unit` checks of the neutral normal and the pull |
//! | [`draft_surface`] | `draft_faces`: a plane tilted, a cone or cylinder about the pull drafted |
//! | [`Cone::drafted_about_plane`] | `drafted_cone` |
//! | [`DraftDefinition`] | none: the drafted surface's definition (#2485's contract) |
//! | [`DraftRefusal`] | the capi's `SMLIB_ERR_UNSUPPORTED` / `SMLIB_ERR_ARGUMENT` refusals in them |
//! | [`largest_clear_angle`], [`refuse_closing_wall`] | none: a draft into a cavity's wall refused naming the largest angle (#20767) |
//!
//! **A plane** parallel to the pull turns about its hinge, the line where it meets the
//! neutral plane (with n the face's outward normal and N the neutral normal, the line along
//! n × N through the point of both planes), in the sense that leans it in along the pull p,
//! until it makes the draft angle θ with the pull: the draft angle is the face's angle to
//! the pull, whatever the neutral plane. With d the hinge's unit direction, chosen so that
//! sin α = (d × n) · p > 0 (α the angle between the hinge and the pull, in the face), the
//! normal goes to n cos φ + (d × n) sin φ (Rodrigues' formula) with sin φ = sin θ / sin α,
//! so that its component along the pull is sin θ; a draft steeper than the hinge allows
//! (|sin θ| > sin α) is refused. For p along N, α is square and φ = θ: SolidCraft's turn
//! about n × p. SolidCraft hinged the plane on the face's internal point moved along the
//! pull onto the neutral plane, about n × p: for a pull oblique to N that line is not on the
//! neutral plane's trace, and the plane depends on which interior point the face gives. A
//! face parallel to the neutral plane, or so near it that its hinge is not proved, is
//! refused.
//!
//! **A cone or cylinder** about the pull keeps its NURB's parameters and each control point
//! moves radially: with R0 the radius on the neutral plane and t the slope of the radius
//! along the pull, the drafted radius at height h along the pull is R0 + h tan(atan t − s θ),
//! s = 1 where the face looks away from the axis (a pin), −1 where it looks towards it (a
//! hole). Its hinge is the circle where its axis crosses the neutral plane: for a neutral
//! plane oblique to the axis that circle is not the plane's trace on the cone (an ellipse),
//! so a plane and a cone drafted about the same oblique neutral plane are hinged by
//! different rules. A rational circle scaled about its axis stays exact, and the rows of a cone's net
//! run straight along its generators, so the edit is exact (Piegl and Tiller, *The NURBS
//! Book*, 2nd ed., §8.4–8.5). The sense s is a proved sign: the surface's normal against
//! the direction away from the axis at a corner of its net, a polynomial in the stored
//! numbers enclosed in rounded interval arithmetic, refused where the enclosure holds 0.
//!
//! **What is proved.** The input thresholds are SolidCraft's acceptance tests on the input
//! (a plane within 1e-6 of parallel to the pull, a pull or axis at least 1e-12 out of the
//! neutral plane, an axis within 1e-9 of the pull), compared as computed. The geometric
//! decisions (the hinge's existence and conditioning, the side a plane leans to, a cone's
//! sense) are proved.
//!
//! **The contract** (#2485, #2488): a drafted surface is a plane or a cone, so it keeps its
//! analytic kind and is its definition (bound 0). The definition rides on the surface
//! ([`crate::surface::surface::Surface::procedural_blend`]): the face's surface before the
//! draft, the sense of its outer faceuse and the [`Draft`]. It is re-certified by the label
//! and the derivation: the net's analytic surface is the drafted one's kind, and its NURBS
//! form lies on both that analytic surface and the one derived from the definition
//! ([`lies_on`], to the rounding of checking it). A radial edit retains the source
//! frame on its positive neutral circle: fitted end circles supply parameter conventions,
//! while [`AnalyticSupport::build`] certifies the intended support on the edited form.

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::as_b_spline;
use crate::error::{offset::OffsetFailure, Failure};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::certified::{above, distance, unit, unit_error};
use crate::geom::tol::are_same;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::ControlPointForm;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::surface::cone::Cone;
use crate::surface::implicit::{lies_on, AnalyticSupport, Implicit};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face::p2::outer_faceuse;
use crate::topology::{Brep, FaceId};
use crate::{Result, SolidError};

const OP: &str = "offset::draft";
const OP_RECERTIFY: &str = "offset::draft::DraftDefinition::recertify";

/// The largest departure of |axis · pull| from 1 at which a cone's axis counts as along the
/// pull (the capi's).
const AXIS_ALONG_PULL: f64 = 1e-9;

/// The largest |n · pull| at which a plane counts as parallel to the pull (the capi's input
/// threshold, compared as computed).
const PLANE_ACROSS_PULL: f64 = 1e-6;

/// The smallest |pull · neutral normal| (or |axis · neutral normal|) at which the neutral
/// plane crosses the pull (the capi's input threshold, compared as computed).
const PULL_ACROSS_PLANE: f64 = 1e-12;

/// How well `1 - (n . N)^2`, the divisor that places a plane's hinge, must be known: its
/// enclosure's width at most this fraction of its lower end. Below it the hinge's point and
/// direction are not known to the rounding the re-certification checks, and the face is
/// refused ([`DraftRefusal::PlaneParallelToNeutral`]): an angle below about 3e-5 radians
/// between the face and the neutral plane.
const HINGE_CONDITION: f64 = 1.0 / 1_048_576.0;

/// A vector of rounded intervals ([`Ivl`]): each component holds the exact value of the
/// expression it was computed by, on the given doubles it started from.
type Ivl3 = [Ivl; 3];

/// A given vector as point intervals.
fn ivl3(v: Vec3) -> Ivl3 {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}

fn ivl3_sub(a: Ivl3, b: Ivl3) -> Ivl3 {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    [a0 - b0, a1 - b1, a2 - b2]
}

fn ivl3_scaled(a: Ivl3, s: Ivl) -> Ivl3 {
    let [a0, a1, a2] = a;
    [a0 * s, a1 * s, a2 * s]
}

fn ivl3_dot(a: Ivl3, b: Ivl3) -> Ivl {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    a0 * b0 + a1 * b1 + a2 * b2
}

fn ivl3_cross(a: Ivl3, b: Ivl3) -> Ivl3 {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    [a1 * b2 - a2 * b1, a2 * b0 - a0 * b2, a0 * b1 - a1 * b0]
}

/// The sign of the exact value an interval holds: `Some(1.0)` or `Some(-1.0)` when the
/// interval proves it, `None` when it holds 0 (or is not a number).
fn proved_sign(x: Ivl) -> Option<f64> {
    if above(x.lo, 0.0) {
        Some(1.0)
    } else if above(0.0, x.hi) {
        Some(-1.0)
    } else {
        None
    }
}

/// A draft: the neutral plane through `neutral_origin` with the unit normal `neutral_normal`,
/// the unit `pull`, and the draft angle in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Draft {
    neutral_origin: Point3,
    neutral_normal: Vec3,
    pull: Vec3,
    angle: f64,
}

/// Why a face cannot be drafted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DraftRefusal {
    /// The face's surface is neither a plane nor a cone or cylinder.
    NotPlaneOrCone,
    /// The face is a plane not parallel to the pull.
    PlaneAcrossPull,
    /// The face is a plane and the pull lies in the neutral plane.
    PullInNeutralPlane,
    /// The face is a plane parallel to the neutral plane, or so near it that its hinge is
    /// not proved to [`HINGE_CONDITION`]: it has no hinge to turn about.
    PlaneParallelToNeutral,
    /// The face is a plane whose hinge makes too small an angle α with the pull for the
    /// draft: no turn about it leaves the face at the draft angle θ to the pull
    /// (|sin θ| > sin α).
    AngleBeyondHinge,
    /// The face is a plane whose hinge runs along the pull: no sense leans it in.
    HingeAlongPull,
    /// The face is a cone or cylinder whose axis does not run along the pull.
    AxisNotAlongPull,
    /// The face is a cone or cylinder whose axis lies in the neutral plane.
    AxisInNeutralPlane,
    /// The face is a cone or cylinder whose sense (towards or away from its axis) is not
    /// proved: its normal is square to the direction away from the axis within rounding, or
    /// its net is not clamped in both directions (the sense is read at the net's corners;
    /// a periodic or unclamped net is refused, not guessed).
    SenseNotProved,
    /// The face is a cone whose radius where its axis crosses the neutral plane is not
    /// positive: the neutral plane is at or past its apex, so there is no hinge circle.
    NeutralPlanePastApex,
    /// The drafted cone would close within the face's span (its radius at a control point
    /// not positive, its slope infinite), or its net could not be edited.
    ConeCloses,
}

impl Draft {
    /// A draft about the plane through `neutral_origin` with normal `neutral_normal`,
    /// pulled along `pull`, by `angle` radians; the directions are made unit.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a direction not finite or not longer than 1e-12, or
    /// an origin or angle not finite.
    pub fn new(
        neutral_origin: Point3,
        neutral_normal: Vec3,
        pull: Vec3,
        angle: f64,
    ) -> Result<Self> {
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        let unit = |v: Vec3| {
            unit(v).ok_or_else(|| {
                SolidError::invalid_input(OP, "a direction not finite or not longer than 1e-12")
            })
        };
        let neutral_normal = unit(neutral_normal)?;
        let pull = unit(pull)?;
        if !finite(neutral_origin) || !angle.is_finite() {
            return Err(SolidError::invalid_input(
                OP,
                "neutral origin or angle not finite",
            ));
        }
        Ok(Self {
            neutral_origin,
            neutral_normal,
            pull,
            angle,
        })
    }

    /// The neutral plane's origin.
    #[must_use]
    pub const fn neutral_origin(&self) -> Point3 {
        self.neutral_origin
    }

    /// The neutral plane's unit normal.
    #[must_use]
    pub const fn neutral_normal(&self) -> Vec3 {
        self.neutral_normal
    }

    /// The unit pull.
    #[must_use]
    pub const fn pull(&self) -> Vec3 {
        self.pull
    }

    /// The angle, radians.
    #[must_use]
    pub const fn angle(&self) -> f64 {
        self.angle
    }

    /// A draft from its stored numbers, bit for bit (a record's): the directions must be
    /// unit to the rounding of making them so. [`unit`] leaves each component within
    /// [`unit_error`] of the exact unit vector's, so the vector within `2 unit_error`
    /// (`sqrt 3 < 2`) of length 1; its computed length is within [`distance`]'s bound of
    /// that; `len - 1` is exact (Sterbenz) and the check's own two roundings are covered by
    /// `1 + gamma_2`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a number not finite or a direction not unit.
    pub(crate) fn from_parts(
        neutral_origin: Point3,
        neutral_normal: Vec3,
        pull: Vec3,
        angle: f64,
    ) -> Result<Self> {
        let unit = |v: Vec3| {
            let len = distance(v, Point3::ZERO);
            v.x.is_finite()
                && v.y.is_finite()
                && v.z.is_finite()
                && (len.value - 1.0).abs() <= (2.0 * unit_error() + len.bound) * (1.0 + gamma(2))
        };
        let finite = neutral_origin.x.is_finite()
            && neutral_origin.y.is_finite()
            && neutral_origin.z.is_finite();
        if !(unit(neutral_normal) && unit(pull) && finite && angle.is_finite()) {
            return Err(SolidError::invalid_input(
                OP,
                "a draft's directions are unit and its numbers finite",
            ));
        }
        Ok(Self {
            neutral_origin,
            neutral_normal,
            pull,
            angle,
        })
    }

    /// This draft moved by `rotate_n_move` after the uniform scale `factor`, its directions
    /// made unit as [`Draft::new`] makes them.
    fn moved(&self, rotate_n_move: &Axis2Placement, factor: f64) -> Result<Self> {
        let unit = |v: Vec3| {
            unit(v).ok_or_else(|| {
                SolidError::invalid_input(OP, "a moved direction not finite or too short")
            })
        };
        Ok(Self {
            neutral_origin: rotate_n_move.transform_point(self.neutral_origin * factor),
            neutral_normal: unit(rotate_n_move.transform_vector(self.neutral_normal))?,
            pull: unit(rotate_n_move.transform_vector(self.pull))?,
            angle: self.angle,
        })
    }
}

/// A plane's drafted plane: the hinge's point and unit direction, the turn about it that
/// leaves the face at the draft angle to the pull, and the drafted unit normal.
#[derive(Debug, Clone, Copy)]
struct PlaneDraft {
    hinge: Point3,
    axis: Vec3,
    turn: f64,
    normal: Vec3,
}

/// The plane through `origin` with outward unit normal `n` drafted by `draft`.
fn plane_draft(
    origin: Point3,
    n: Vec3,
    draft: &Draft,
) -> std::result::Result<PlaneDraft, DraftRefusal> {
    let (p, big_n, angle) = (draft.pull, draft.neutral_normal, draft.angle);
    if n.dot(p).abs() > PLANE_ACROSS_PULL {
        return Err(DraftRefusal::PlaneAcrossPull);
    }
    if p.dot(big_n).abs() < PULL_ACROSS_PLANE {
        return Err(DraftRefusal::PullInNeutralPlane);
    }
    // the hinge's direction n x N, and m . N, the divisor that places it (m = N - n (n . N),
    // the neutral normal's part in the face's plane): enclosed in rounded intervals on the
    // given n and N, and refused unless m . N is known to HINGE_CONDITION of itself
    let (ni, big_ni) = (ivl3(n), ivl3(big_n));
    let along_i = ivl3_cross(ni, big_ni);
    let m_i = ivl3_sub(big_ni, ivl3_scaled(ni, ivl3_dot(ni, big_ni)));
    let m_n = ivl3_dot(m_i, big_ni);
    if !above(m_n.lo, 0.0) || above(m_n.hi - m_n.lo, HINGE_CONDITION * m_n.lo) {
        return Err(DraftRefusal::PlaneParallelToNeutral);
    }
    let Some(mut axis) = unit(n.cross(big_n)) else {
        return Err(DraftRefusal::PlaneParallelToNeutral);
    };
    // the side the plane leans to: d x n along the pull, d = (n x N) / |n x N|, so the
    // sign of ((n x N) x n) . p, proved on the same intervals
    let Some(lean) = proved_sign(ivl3_dot(ivl3_cross(along_i, ni), ivl3(p))) else {
        return Err(DraftRefusal::HingeAlongPull);
    };
    if lean < 0.0 {
        axis = -axis;
    }
    // the point of both planes: from the face's plane's origin along m = N - n (n . N),
    // the neutral normal's part in the face's plane
    let m = big_n - n * n.dot(big_n);
    let hinge = origin + m * ((draft.neutral_origin - origin).dot(big_n) / m.dot(big_n));
    if !(hinge.x.is_finite() && hinge.y.is_finite() && hinge.z.is_finite()) {
        return Err(DraftRefusal::PlaneParallelToNeutral);
    }
    // the turn that leaves the face at the draft angle to the pull: sin(turn) sin(alpha) =
    // sin(angle), sin(alpha) = (d x n) . p, positive by the lean's sign
    let lean_dir = axis.cross(n);
    let sin_alpha = lean_dir.dot(p);
    let ratio = angle.sin() / sin_alpha;
    if !above(sin_alpha, 0.0) || ratio.is_nan() || above(ratio.abs(), 1.0) {
        return Err(DraftRefusal::AngleBeyondHinge);
    }
    let turn = ratio.asin();
    let normal = n * turn.cos() + lean_dir * turn.sin();
    Ok(PlaneDraft {
        hinge,
        axis,
        turn,
        normal,
    })
}

/// A cone's drafted cone: the hinge circle's centre and its height on the cone, the radius
/// there, the slope along the pull before and after the draft, `sense`, the pull along the
/// cone's axis (1) or against it (−1), and `lean`, the face looking away from the axis (1)
/// or towards it (−1).
#[derive(Debug, Clone, Copy)]
struct ConeDraft {
    /// The cone's own unit axis, the way the pull runs (the pull is along it within the
    /// capi's 1e-9; the drafted cone is about the cone's axis, so it stays a cone).
    along: Vec3,
    centre: Point3,
    v0: f64,
    r0: f64,
    slope: f64,
    turned: f64,
    sense: f64,
    lean: f64,
}

/// The sign of `r . (S_u x S_v)` on the net `net` of a surface about the axis through
/// `on_axis` along `x_axis x y_axis`, `r` the direction away from the axis: 1 where the
/// surface's normal looks away from its axis, -1 where it looks towards it; `None` where no
/// corner of the net proves it, or two corners disagree.
///
/// The sign is proved, not estimated. At a corner of a net clamped in both directions
/// (end knots of multiplicity degree + 1, the first and last spans not empty) the surface
/// is the corner control point `P_c`, and each first derivative is a positive multiple of
/// `±(P_n - P_c)`, `P_n` the corner's neighbour in that direction (+ at the start of the
/// knots, − at the end), the multiple `p / span · w_n / w_c` (Piegl and Tiller, *The NURBS
/// Book*, 2nd ed., eq. 4.9 and §4.4). With the stored homogeneous points `Pw` and positive
/// weights, `w_c w_n (P_n - P_c) = w_c Pw_n - w_n Pw_c` and `w_c (P_c - O) = Pw_c - w_c O`,
/// and with `z = x_axis x y_axis` unnormalised, `|z|^2 r = |z|^2 d - (d . z) z`: so the
/// sign is that of `|z|^2 (d . N) - (d . z)(z . N)`, `d = Pw_c - w_c O` and
/// `N = a_u x a_v` the two scaled derivatives, a polynomial in the stored numbers. It is
/// evaluated in rounded interval arithmetic ([`Ivl`]), which encloses the exact value, and
/// read only where the enclosure excludes 0. The four corners are tried (a cone's corner at
/// its apex has a zero derivative); the sign of a cone's normal against its radial
/// direction is the same everywhere off the apex, so proved corners agree on a valid net.
///
/// # Errors
///
/// Those of reading the net's knots and control points.
pub(crate) fn net_sense(
    net: &BSplineSurface,
    on_axis: Point3,
    x_axis: Vec3,
    y_axis: Vec3,
) -> Result<Option<f64>> {
    // the corner indices and the sign of the first derivative's difference at each end,
    // for a clamped direction with at least two control points
    let ends = |param: SurfParamType| -> Result<Option<[(usize, usize, f64); 2]>> {
        let (knots, mult) = net.knots(param, None)?;
        let clamped = |m: Option<&usize>| m == Some(&(net.degree(param) + 1));
        let spans = match (knots.first(), knots.get(1)) {
            (Some(&a), Some(&b)) => above(b, a),
            _ => false,
        } && match (
            knots.last(),
            knots.len().checked_sub(2).and_then(|i| knots.get(i)),
        ) {
            (Some(&b), Some(&a)) => above(b, a),
            _ => false,
        };
        let last = net.number_control_points(param).checked_sub(1);
        Ok(match last {
            Some(last) if last > 0 && spans && clamped(mult.first()) && clamped(mult.last()) => {
                // `last > 0`: `last - 1` does not wrap
                Some([(0, 1, 1.0), (last, last - 1, -1.0)])
            }
            _ => None,
        })
    };
    let (Some(us), Some(vs)) = (ends(SurfParamType::U)?, ends(SurfParamType::V)?) else {
        return Ok(None);
    };
    let o = ivl3(on_axis);
    let z = ivl3_cross(ivl3(x_axis), ivl3(y_axis));
    let zz = ivl3_dot(z, z);
    let mut proved: Option<f64> = None;
    for (iu, nu, su) in us {
        for (iv, nv, sv) in vs {
            let at = |i: usize, j: usize| -> Result<(Ivl3, Ivl, bool)> {
                let (pw, w) = net.control_point(ControlPointForm::HomogeneousRational, i, j)?;
                Ok((ivl3(pw), Ivl::from(w), above(w, 0.0)))
            };
            let (pc, wc, okc) = at(iu, iv)?;
            let (pu, wu, oku) = at(nu, iv)?;
            let (pv, wv, okv) = at(iu, nv)?;
            if !(okc && oku && okv) {
                return Ok(None);
            }
            let a_u = ivl3_scaled(
                ivl3_sub(ivl3_scaled(pu, wc), ivl3_scaled(pc, wu)),
                Ivl::from(su),
            );
            let a_v = ivl3_scaled(
                ivl3_sub(ivl3_scaled(pv, wc), ivl3_scaled(pc, wv)),
                Ivl::from(sv),
            );
            let n = ivl3_cross(a_u, a_v);
            let d = ivl3_sub(pc, ivl3_scaled(o, wc));
            let value = zz * ivl3_dot(d, n) - ivl3_dot(d, z) * ivl3_dot(z, n);
            match (proved, proved_sign(value)) {
                (Some(was), Some(now)) if was != now => return Ok(None),
                (None, now @ Some(_)) => proved = now,
                _ => {}
            }
        }
    }
    Ok(proved)
}

/// The cone `cone` drafted by `draft`, `outward` whether the face's outer faceuse runs with
/// the surface's normal.
fn cone_draft(
    cone: &Cone,
    outward: bool,
    draft: &Draft,
) -> Result<std::result::Result<ConeDraft, DraftRefusal>> {
    let (origin, normal, pull, angle) = (
        draft.neutral_origin,
        draft.neutral_normal,
        draft.pull,
        draft.angle,
    );
    let frame = cone.canonical().origin;
    let z = frame.x_axis().cross(frame.y_axis());
    let len = z.length();
    if !(len > 0.0 && len.is_finite()) {
        return Ok(Err(DraftRefusal::AxisNotAlongPull));
    }
    let z = z * (1.0 / len);
    let sense = z.dot(pull);
    if (sense.abs() - 1.0).abs() > AXIS_ALONG_PULL {
        return Ok(Err(DraftRefusal::AxisNotAlongPull));
    }
    let sense = sense.signum();
    let along = z * sense;
    let axis = frame.origin();
    let toward = z.dot(normal);
    if toward.abs() < PULL_ACROSS_PLANE {
        return Ok(Err(DraftRefusal::AxisInNeutralPlane));
    }
    // the hinge circle's centre: the axis on the neutral plane, at the cone's height v0
    let v0 = (origin - axis).dot(normal) / toward;
    let centre = axis + z * v0;
    // the sense: the surface's normal against the direction away from the axis, a sign
    // proved on the net ([`net_sense`]), turned with the outer faceuse
    let Some(s) = net_sense(cone.rev.base(), axis, frame.x_axis(), frame.y_axis())? else {
        return Ok(Err(DraftRefusal::SenseNotProved));
    };
    let s = if outward { s } else { -s };
    // a cylinder (or a cone whose semi-angle is below EFF_ZERO, which the edit treats as
    // one: Cone::lean_cylinder) has its canonical radius everywhere; a cone's slope is its
    // stored semi-angle's tangent, not a difference of radii
    let (r0, slope) = if cone.is_cylinder() {
        (cone.canonical().radius, 0.0)
    } else {
        (cone.radius(v0), cone.tan_semi_angle() * sense)
    };
    if !above(r0, 0.0) {
        return Ok(Err(DraftRefusal::NeutralPlanePastApex));
    }
    let turned = (slope.atan() - s * angle).tan();
    if !turned.is_finite() {
        return Ok(Err(DraftRefusal::ConeCloses));
    }
    Ok(Ok(ConeDraft {
        along,
        centre,
        v0,
        r0,
        slope,
        turned,
        sense,
        lean: s,
    }))
}

/// Maps a status failure of a net edit to the refusal [`DraftRefusal::ConeCloses`]; any
/// other error is passed on.
fn cone_edit<T>(r: Result<T>) -> Result<std::result::Result<T, DraftRefusal>> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) if e.sm_status().is_some() => Ok(Err(DraftRefusal::ConeCloses)),
        Err(e) => Err(e),
    }
}

impl Cone {
    /// Not upstream (SolidCraft's `drafted_cone`, tangency-labs/smlib#27, #53): the cone a
    /// face on this cone or cylinder about the pull becomes drafted by `draft`, about its
    /// circle on the neutral plane, so that it leans in going along the pull; `outward`,
    /// whether the face's outer faceuse runs with the surface's normal, says whether the face
    /// looks away from the axis (a pin, its radius shrinking along the pull) or towards it (a
    /// hole, growing). The NURB keeps its parameters, each control point moved radially by
    /// the new radius over the old at its height, so the face's UV trims stay valid: for a
    /// cylinder [`Cone::lean_cylinder`], for a cone the same edit on [`Cone::base_mut`], then
    /// the analytic data rebuilt. The result carries no definition; [`draft_surface`] gives
    /// it one.
    ///
    /// # Errors
    ///
    /// Non-status errors of the evaluations, the edits and the rebuild. A pull not along the
    /// axis, an axis in the neutral plane, a sense not proved, or a cone that would close
    /// within the face's span is an `Ok(Err(refusal))`, as is a status failure of the edits.
    pub fn drafted_about_plane(
        &self,
        k: &Kernel,
        outward: bool,
        draft: &Draft,
    ) -> Result<std::result::Result<Cone, DraftRefusal>> {
        let d = match cone_draft(self, outward, draft)? {
            Ok(d) => d,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let pull = d.along;
        if self.is_cylinder() {
            return match cone_edit(self.lean_cylinder(k, d.turned * d.sense, d.v0))? {
                Ok(cone) => Ok(Ok(self.drafted_on_source_axis(
                    k,
                    cone.rev.base().copy(),
                    d,
                )?)),
                Err(refusal) => Ok(Err(refusal)),
            };
        }
        let mut cone = self.clone();
        let net = cone.base_mut();
        let (nu, nv) = (
            net.number_control_points(SurfParamType::U),
            net.number_control_points(SurfParamType::V),
        );
        for i in 0..nu {
            for j in 0..nv {
                let (p, w) = match cone_edit(net.control_point(
                    ControlPointForm::EuclideanRational,
                    i,
                    j,
                ))? {
                    Ok(pw) => pw,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                let h = (p - d.centre).dot(pull);
                let (was, now) = (self.radius(d.v0 + h * d.sense), d.r0 + h * d.turned);
                if !(was > 0.0 && now > 0.0) {
                    return Ok(Err(DraftRefusal::ConeCloses));
                }
                let foot = d.centre + pull * h;
                let moved = foot + (p - foot) * (now / was);
                if let Err(refusal) = cone_edit(net.set_control_point(
                    ControlPointForm::EuclideanRational,
                    i,
                    j,
                    moved,
                    w,
                ))? {
                    return Ok(Err(refusal));
                }
            }
        }
        // `CopyAndAddAnalytics`: the cone's analytic data from its edited NURB.
        if let Err(refusal) = cone_edit(cone.rebuild_step_from_nurb_parameters(k))? {
            return Ok(Err(refusal));
        }
        Ok(Ok(self.drafted_on_source_axis(
            k,
            cone.rev.base().copy(),
            d,
        )?))
    }

    /// A radial edit keeps its source axis. End-circle recognition supplies the
    /// parameter conventions, but its fitted centres must not redefine that axis.
    /// Use the positive neutral circle as origin and certify both the implicit
    /// support and its UV parametrisation on the edited form.
    fn drafted_on_source_axis(
        &self,
        k: &Kernel,
        form: BSplineSurface,
        d: ConeDraft,
    ) -> Result<Cone> {
        let mut placement = self.canonical().origin;
        placement.set_origin(d.centre);
        let support = AnalyticSupport::Cone {
            placement,
            base_radius: d.r0,
            semi_angle_deg: (d.sense * d.turned).atan().to_degrees(),
        };
        let surface = support.build(k, form)?;
        match surface.kind() {
            SurfaceKind::Cone(cone) => Ok((**cone).clone()),
            _ => Err(SolidError::invalid_input(
                OP,
                "a drafted cone keeps its cone kind",
            )),
        }
    }
}

/// A drafted surface's definition: the face's surface before the draft, whether the face's
/// outer faceuse runs with its normal, and the draft. Its net is the drafted plane or cone,
/// which is the definition's surface (bound 0).
#[derive(Debug, Clone)]
pub struct DraftDefinition {
    /// The face's surface before the draft (a plane, cone or cylinder), without a
    /// definition of its own.
    pub(crate) source: Box<Surface>,
    /// Whether the face's outer faceuse runs with the surface's normal.
    pub(crate) outward: bool,
    /// The draft.
    pub(crate) draft: Draft,
}

impl DraftDefinition {
    /// A definition on a copy of `source`.
    ///
    /// # Errors
    ///
    /// Those of copying the surface.
    pub fn create(k: &Kernel, source: &Surface, outward: bool, draft: Draft) -> Result<Self> {
        let mut s = source.copy(k)?;
        s.clear_definition();
        Ok(Self {
            source: Box::new(s),
            outward,
            draft,
        })
    }

    /// The face's surface before the draft.
    #[must_use]
    pub fn source(&self) -> &Surface {
        &self.source
    }

    /// Whether the face's outer faceuse runs with the surface's normal.
    #[must_use]
    pub const fn outward(&self) -> bool {
        self.outward
    }

    /// The draft.
    #[must_use]
    pub const fn draft(&self) -> &Draft {
        &self.draft
    }

    /// The net's certified distance from the definition: 0, the net is its surface.
    #[must_use]
    pub const fn bound(&self) -> f64 {
        0.0
    }

    /// The analytic surface the definition derives, as its implicit equation.
    fn derived(&self, k: &Kernel) -> Result<Implicit> {
        let refuse = |_: DraftRefusal| {
            SolidError::invalid_input(OP_RECERTIFY, "the definition no longer drafts its surface")
        };
        match self.source.kind_with_nurb(k)? {
            SurfaceKind::Plane(plane) => {
                let mut n = plane.position().z_axis();
                if !self.outward {
                    n = -n;
                }
                let d = plane_draft(plane.position().origin(), n, &self.draft).map_err(refuse)?;
                Ok(Implicit::Plane {
                    origin: d.hinge,
                    normal: d.normal,
                })
            }
            SurfaceKind::Cone(c) => {
                let d = cone_draft(c, self.outward, &self.draft)?.map_err(refuse)?;
                Ok(Implicit::Cone {
                    origin: d.centre,
                    axis: d.along,
                    radius: d.r0,
                    tan_angle: d.turned,
                })
            }
            _ => Err(SolidError::invalid_input(
                OP_RECERTIFY,
                "a draft's source is a plane or a cone",
            )),
        }
    }

    /// Re-certifies `net` against this definition: the net's analytic kind is the drafted
    /// one's (a plane for a plane, a cone for a cone or cylinder), and its NURBS form lies on
    /// both its own analytic surface and the one the definition derives ([`lies_on`]).
    /// Returns the distance proved, 0.
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming what fails; those of the derivation and of making the forms.
    pub fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        let refuse = |why| SolidError::invalid_input(OP_RECERTIFY, why);
        let derived = self.derived(k)?;
        let label = match (AnalyticSupport::of(net), &derived) {
            (Some(a @ AnalyticSupport::Plane { .. }), Implicit::Plane { .. })
            | (Some(a @ AnalyticSupport::Cone { .. }), Implicit::Cone { .. }) => a.implicit(),
            _ => return Err(refuse("a drafted surface keeps its plane or cone kind")),
        };
        let form =
            as_b_spline(k, net)?.ok_or_else(|| refuse("a drafted surface has a NURBS form"))?;
        if !lies_on(&form, &label)? {
            return Err(refuse(
                "a drafted surface's form is off its own analytic surface",
            ));
        }
        if !lies_on(&form, &derived)? {
            return Err(refuse("a drafted surface is not its definition's"));
        }
        Ok(0.0)
    }

    /// Moves the definition with its net (`Transform`): the source surface, the neutral
    /// plane and the pull by the same motion, the angle kept.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a scale that is not uniform and positive (a draft is not kept
    /// under it); those of the source's transform.
    pub fn transform(
        &mut self,
        k: &Kernel,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
        depth: usize,
    ) -> Result<()> {
        let mut factor = 1.0;
        if let Some(sc) = scale {
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
            if !are_same(sc.x, sc.y) || !are_same(sc.x, sc.z) || !(sc.x > EFF_ZERO) {
                return Err(SolidError::invalid_input(
                    "offset::draft::DraftDefinition::transform",
                    "a draft moves rigidly or by a uniform scale",
                ));
            }
            factor = sc.x;
        }
        let draft = self.draft.moved(rotate_n_move, factor)?;
        self.source
            .transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
        self.draft = draft;
        Ok(())
    }
}

/// The refusal of face `f` of the caller's body, which has no surface or no face use
/// (`missing_surface`), with the capi's status for it, `SM_ERR_INVALID_INPUT`.
fn invalid(f: FaceId, missing_surface: bool) -> SolidError {
    let failure = if missing_surface {
        OffsetFailure::FaceHasNoSurface { face: f }
    } else {
        OffsetFailure::FaceHasNoOuterFaceuse { face: f }
    };
    Failure::new(OP, failure)
        .with_status(crate::error::SM_ERR_INVALID_INPUT)
        .into_error()
}

/// The surface face `f` of a solid takes drafted by `draft`, with its definition
/// ([`DraftDefinition`], re-certified before it is returned): a plane parallel to the pull
/// turned about its hinge on the neutral plane, a cone or cylinder about the pull the cone
/// [`Cone::drafted_about_plane`] gives. The face's outward sense is read off its outer
/// faceuse ([`outer_faceuse`]).
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` for a face without a surface or without an outer faceuse; those
/// of the surface's kind, copy, transform and evaluations, and of the re-certification. A
/// face that cannot be drafted is an `Ok(Err(refusal))`.
pub fn draft_surface(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    draft: &Draft,
) -> Result<std::result::Result<Surface, DraftRefusal>> {
    let fu = outer_faceuse(b, f)?;
    let surf = b.face(f)?.surface.as_ref();
    let (Some(surf), Some(fu)) = (surf, fu) else {
        return Err(invalid(f, surf.is_none()));
    };
    let outward = b.faceuse(fu)?.orientation == OrientType::Same;
    let mut drafted = match surf.kind_with_nurb(k)? {
        SurfaceKind::Cone(c) => match c.drafted_about_plane(k, outward, draft)? {
            Ok(cone) => Surface::from(cone),
            Err(refusal) => return Ok(Err(refusal)),
        },
        SurfaceKind::Plane(plane) => {
            let mut n = plane.position().z_axis();
            if !outward {
                n = -n;
            }
            let d = match plane_draft(plane.position().origin(), n, draft) {
                Ok(d) => d,
                Err(refusal) => return Ok(Err(refusal)),
            };
            let mut tilted = surf.copy(k)?;
            tilted.clear_definition();
            let mut turn = Axis2Placement::IDENTITY;
            turn.rotate_about_axis_at_point(d.turn, d.hinge, d.axis);
            tilted.transform(k, &turn, None)?;
            tilted
        }
        _ => return Ok(Err(DraftRefusal::NotPlaneOrCone)),
    };
    let definition = DraftDefinition::create(k, surf, outward, *draft)?;
    definition.recertify(k, &drafted)?;
    drafted.set_draft_definition(definition);
    Ok(Ok(drafted))
}

/// Not upstream (#20767): the largest draft angle at which face `f` of `b`, a cone or
/// cylinder about the pull, stays clear of another shell of the body (a cavity's wall, or
/// another lump), with the face of that shell it meets first; `None` where no shell is
/// reached by a draft the way `draft` leans, or none is of the kind the bound is exact for.
///
/// Drafted by θ, the face's cone is the radius r0 + h tan(atan t − s θ) at height h along
/// the pull above its hinge circle (radius r0, slope t, sense s: the module's header). It
/// passes through a point at height h and distance ρ from the axis where s θ is
/// ψ = atan t − atan σ, σ = (ρ − r0) / h: as θ grows from 0 the cone sweeps, at each height,
/// the points whose ψ lies between 0 and s θ, so it reaches a shell first at the least |ψ|
/// of the shell's points on that side. For a shell all of whose faces are cones or cylinders
/// about the same axis or planes across it, lying wholly on one side of the neutral plane
/// and within the face's span of heights, that least |ψ| is at a vertex: on a plane across
/// the axis σ is monotone in ρ; along a generator of a cone σ is a ratio of linear functions of
/// h without a pole, so monotone; on a circle about the axis it is constant; and every
/// edge of such a shell is a circle about the axis or a generator, each with its vertices.
/// A shell of another kind is not bounded here (the local operation's check still
/// judges the body built).
///
/// "About the same axis" is held to the body's tolerance over the shell's own extent, not to
/// a direction's rounding: a cone's axis may leave the drafted axis by no more than the
/// tolerance beside each of its vertices (the departure is linear along it, so largest at
/// the ends of its span), and the tilt of a cone's axis or a plane's normal, times the
/// farthest of its points from the axis, may move them along the pull by no more than the
/// tolerance. A shell coaxial only to a direction's rounding (a radius-9 circle 900 up an
/// axis 4e-5 off sits 0.036 off the axis) gets no bound: on a circle off the axis ψ is not
/// constant, and its least could lie between the circle's vertices.
///
/// A shell holding a face of `drafted` (the faces the same call drafts) gets no bound: its
/// faces move too, so its undrafted vertices say nothing of where the drafted walls meet.
/// Such a draft is left to the local operation's check.
///
/// The angle is computed as the doubles give it, not enclosed: a draft within rounding of
/// it may be decided either way, as at that angle the wall is a line wide. A shell off the
/// axis by up to the tolerance moves the angle by about tolerance / h.
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` for a face without a surface or an outer faceuse; those of the
/// surfaces' kinds and of the topology queries.
pub fn largest_clear_angle(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    drafted: &[FaceId],
    draft: &Draft,
) -> Result<Option<(f64, FaceId)>> {
    use crate::topology::face::p4::vertices;
    use std::collections::{BTreeMap, BTreeSet};
    let fu = outer_faceuse(b, f)?;
    let surf = b.face(f)?.surface.as_ref();
    let (Some(surf), Some(fu)) = (surf, fu) else {
        return Err(invalid(f, surf.is_none()));
    };
    let outward = b.faceuse(fu)?.orientation == OrientType::Same;
    let d = match surf.kind_with_nurb(k)? {
        SurfaceKind::Cone(cone) => match cone_draft(cone, outward, draft)? {
            Ok(d) => d,
            Err(_) => return Ok(None),
        },
        _ => return Ok(None),
    };
    let lean = d.lean * draft.angle;
    let height = |p: Point3| (p - d.centre).dot(d.along);
    let off_axis = |p: Point3| {
        let r = p - d.centre;
        (r - d.along * r.dot(d.along)).length()
    };
    // the sine of the angle between a direction and the axis, `None` for no direction
    let tilt = |v: Vec3| {
        let len = v.length();
        (len > 0.0 && len.is_finite()).then(|| (v.cross(d.along).length() / len).min(1.0))
    };
    // the face's span of heights, from its vertices (its edges are circles about the axis or
    // generators)
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in vertices(b, f, None, false, None)? {
        let h = height(b.vertex(v)?.point);
        lo = lo.min(h);
        hi = hi.max(h);
    }
    // the shells a face's two faceuses lie in
    let shells_of = |g: FaceId| -> Result<BTreeSet<crate::topology::ShellId>> {
        let mut out = BTreeSet::new();
        if let Some(use_) = b.face(g)?.faceuse {
            let use_ = b.faceuse(use_)?;
            out.extend(use_.shell);
            if let Some(mate) = use_.mate {
                out.extend(b.faceuse(mate)?.shell);
            }
        }
        Ok(out)
    };
    let own = shells_of(f)?;
    let mut others: BTreeMap<crate::topology::ShellId, Vec<FaceId>> = BTreeMap::new();
    for g in crate::topology::brep::p1::faces(b, None)? {
        for s in shells_of(g)? {
            if !own.contains(&s) {
                others.entry(s).or_default().push(g);
            }
        }
    }
    let tolerance = b.tolerance();
    let mut least: Option<(f64, FaceId)> = None;
    'shells: for faces in others.values() {
        if faces.iter().any(|g| drafted.contains(g)) {
            continue;
        }
        let mut shell_least: Option<(f64, FaceId)> = None;
        let mut side = 0.0;
        for &g in faces {
            let Some(surface) = b.face(g)?.surface.as_ref() else {
                continue 'shells;
            };
            let mut points = Vec::new();
            for v in vertices(b, g, None, false, None)? {
                points.push(b.vertex(v)?.point);
            }
            let reach = points.iter().fold(0.0_f64, |m, &p| m.max(off_axis(p)));
            let about_axis = match surface.kind_with_nurb(k)? {
                // a plane across the axis: its tilt moves no point of it (within `reach` of
                // the axis, its origin's distance more) along the pull past the tolerance
                SurfaceKind::Plane(plane) => {
                    let frame = plane.position();
                    tilt(frame.z_axis())
                        .is_some_and(|t| t * (reach + off_axis(frame.origin())) <= tolerance)
                }
                // a cone about the axis: its axis beside each vertex within the tolerance of
                // the drafted axis, and its tilt moving no point along the pull past it
                SurfaceKind::Cone(cone) => {
                    let frame = cone.canonical().origin;
                    let z = frame.x_axis().cross(frame.y_axis());
                    let o = frame.origin();
                    tilt(z).is_some_and(|t| {
                        let z = z * (1.0 / z.length());
                        t * reach <= tolerance
                            && points
                                .iter()
                                .all(|&p| off_axis(o + z * (p - o).dot(z)) <= tolerance)
                    })
                }
                _ => false,
            };
            if !about_axis {
                continue 'shells;
            }
            for p in points {
                let h = height(p);
                // on one side of the neutral plane, off it by more than the tolerance, inside
                // the face's span
                if !(h > lo && h < hi && h.abs() > tolerance) || h.signum() * side < 0.0 {
                    continue 'shells;
                }
                side = h.signum();
                let psi = d.slope.atan() - ((off_axis(p) - d.r0) / h).atan();
                if psi * lean > 0.0 && shell_least.is_none_or(|(at, _)| psi.abs() < at) {
                    shell_least = Some((psi.abs(), g));
                }
            }
        }
        if let Some((at, g)) = shell_least {
            if least.is_none_or(|(was, _)| at < was) {
                least = Some((at, g));
            }
        }
    }
    Ok(least)
}

/// Not upstream (#20767): refuses the draft of `faces` of `b` by `draft` where the drafted
/// cone of one of them would reach another shell of the body, closing the wall between them
/// ([`largest_clear_angle`], each face against the shells none of `faces` lies on):
/// [`OffsetFailure::DraftClosesWall`] with the largest angle that keeps the wall, for the
/// first of `faces` that reaches one. A face of another kind, or a shell the bound does not
/// cover, passes.
///
/// # Errors
///
/// [`OffsetFailure::DraftClosesWall`]; those of [`largest_clear_angle`].
pub fn refuse_closing_wall(k: &Kernel, b: &Brep, faces: &[FaceId], draft: &Draft) -> Result<()> {
    for &f in faces {
        if let Some((largest, wall)) = largest_clear_angle(k, b, f, faces, draft)? {
            if draft.angle.abs() >= largest {
                return Err(Failure::new(
                    OP,
                    OffsetFailure::DraftClosesWall {
                        face: f,
                        wall,
                        largest,
                    },
                )
                .into_error());
            }
        }
    }
    Ok(())
}

/// Points tried on each edge of a face by [`drafted_through`], its ends included.
const THROUGH_POINTS: usize = 17;

/// A face a draft would have to cut through ([`drafted_through`]): the drafted face, the face
/// it runs into, a point of that face inside the band the draft takes off or adds, and how far
/// that point lies past the drafted plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraftedThrough {
    /// The drafted face.
    pub drafted: FaceId,
    /// The face it runs into.
    pub through: FaceId,
    /// A point of `through` in the band between the face's plane and its drafted plane.
    pub at: Point3,
    /// The point's distance past the drafted plane: the face reaches at least this far in.
    pub depth: f64,
}

/// Not upstream: a face of `b` that drafting the plane faces `faces` onto the planes `surfaces`
/// (in the same order) would cut through. A draft replaces each face by its drafted plane and
/// extends or trims the faces around it ([`crate::topology::brep::local_operation`]); a face that
/// meets no drafted face, not even at a vertex, but has a point between a drafted face's plane
/// and its drafted plane, over the face itself, would be cut through, a change of topology the
/// draft does not make (a hole whose wall passes within the draft of the side, #19686).
///
/// The point is proved, not estimated: it is one of the face's vertices or a point evaluated on
/// one of its edges' curves, and it lies past each plane by more than the edge's or vertex's
/// tolerance and the rounding of the comparison. It lies over the drafted face when its foot on
/// the face's plane is classified inside the face's trimmed area, off its edges and vertices, by
/// the face's own point classification against its loops
/// ([`crate::query::face::p1::point_3d_classify_at`]), so a point beside a face that is not a
/// rectangle (an L, a face with a hole) is not taken for one over it. The classification runs on
/// `b` in place, its lazy caches as upstream's const query keeps them. A face with no such point
/// among those tried is not reported; a pair that is not two planes is skipped.
///
/// # Errors
///
/// Those of reading the faces, edges, vertices and surfaces, of evaluating the curves and of
/// the point classification.
pub fn drafted_through(
    k: &Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    surfaces: &[Surface],
) -> Result<Option<DraftedThrough>> {
    use crate::intersect::point_class::{PointClassType, PointClassification};
    use crate::query::face::p1::{point_3d_classify_at, Point3dClassifyOptions};
    use crate::topology::face::p4::{edges, vertices};
    // each drafted plane face: its plane's origin and outward normal, and its drafted plane's
    let mut bands = Vec::new();
    for (&f, drafted) in faces.iter().zip(surfaces) {
        let (Some(surf), Some(fu)) = (b.face(f)?.surface.as_ref(), outer_faceuse(b, f)?) else {
            continue;
        };
        let (SurfaceKind::Plane(plane), SurfaceKind::Plane(tilted)) =
            (surf.kind_with_nurb(k)?, drafted.kind_with_nurb(k)?)
        else {
            continue;
        };
        let sense = if b.faceuse(fu)?.orientation == OrientType::Same {
            1.0
        } else {
            -1.0
        };
        let (at, tilted_at) = (plane.position(), tilted.position());
        bands.push((
            f,
            at.origin(),
            at.z_axis() * sense,
            tilted_at.origin(),
            tilted_at.z_axis() * sense,
        ));
    }
    if bands.is_empty() {
        return Ok(None);
    }
    // what touches a drafted face: its edges' and vertices' other faces are trimmed or extended
    let mut near_edges = Vec::new();
    let mut near_vertices = Vec::new();
    for &f in faces {
        near_edges.extend(edges(b, f, None, false, None)?);
        near_vertices.extend(vertices(b, f, None, false, None)?);
    }
    for g in crate::topology::brep::p1::faces(b, None)? {
        if faces.contains(&g) {
            continue;
        }
        let g_edges = edges(b, g, None, false, None)?;
        let g_vertices = vertices(b, g, None, false, None)?;
        if g_edges.iter().any(|e| near_edges.contains(e))
            || g_vertices.iter().any(|v| near_vertices.contains(v))
        {
            continue;
        }
        let mut points = Vec::new();
        for &vx in &g_vertices {
            let vertex = b.vertex(vx)?;
            points.push((vertex.point, vertex.tolerance));
        }
        for &e in &g_edges {
            let edge = b.edge(e)?;
            let Some(curve) = edge.curve.as_ref() else {
                continue;
            };
            let (t0, t1) = (edge.interval.min(), edge.interval.max());
            if !(t0.is_finite() && t1.is_finite() && t0 <= t1) {
                continue;
            }
            for i in 0..THROUGH_POINTS {
                let s = i as f64 / (THROUGH_POINTS - 1) as f64;
                points.push((curve.evaluate_point(k, t0 + (t1 - t0) * s)?, edge.tolerance));
            }
        }
        let mut deepest: Option<DraftedThrough> = None;
        for &(f, origin, n, hinge, normal) in &bands {
            for &(q, tol) in &points {
                let (to_face, to_drafted) = (q - origin, q - hinge);
                // the comparisons' rounding: a few units in the last place of the largest term
                let scale = [
                    q.x, q.y, q.z, origin.x, origin.y, origin.z, hinge.x, hinge.y, hinge.z,
                ]
                .iter()
                .fold(1.0_f64, |m, c| m.max(c.abs()));
                let margin = tol.max(0.0) + 64.0 * f64::EPSILON * scale;
                let (a, d) = (n.dot(to_face), normal.dot(to_drafted));
                let between = (a < -margin && d > margin) || (a > margin && d < -margin);
                if !between || deepest.is_some_and(|w| d.abs() <= w.depth) {
                    continue;
                }
                // its foot on the face's plane, inside the face's trimmed area
                let mut pc = PointClassification::new(margin);
                point_3d_classify_at(
                    k,
                    b,
                    f,
                    q - n * a,
                    margin,
                    true,
                    &mut pc,
                    Point3dClassifyOptions::default(),
                )?;
                if pc.point_class() == PointClassType::Face {
                    deepest = Some(DraftedThrough {
                        drafted: f,
                        through: g,
                        at: q,
                        depth: d.abs(),
                    });
                }
            }
        }
        if deepest.is_some() {
            return Ok(deepest);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
    use crate::query::api_queries::face_bounding_box;
    use crate::surface::procedural_blend::convert::{draft_rebuild, draft_to_record};
    use crate::topology::brep::p1 as brep_p1;

    const THETA: f64 = 5.0 * core::f64::consts::PI / 180.0;
    /// A draft steep enough to reach past a pocket's floor into the empty pocket behind it.
    const THIRTY: f64 = 30.0 * core::f64::consts::PI / 180.0;

    /// The face of `b` whose tight box is flat at `x = at` (`axis` 0), `y = at` (1) or
    /// `z = at` (2).
    fn flat_face(k: &Kernel, b: &Brep, axis: usize, at: f64) -> FaceId {
        brep_p1::faces(b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let bx = face_bounding_box(k, b, f, true).unwrap();
                let c = |p: Point3| [p.x, p.y, p.z][axis];
                (c(bx.min()) - at).abs() < 1e-9 && (c(bx.max()) - at).abs() < 1e-9
            })
            .unwrap()
    }

    /// The face of `b` that is no plane.
    fn curved_face(k: &Kernel, b: &Brep) -> FaceId {
        brep_p1::faces(b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                !matches!(
                    b.face(f)
                        .unwrap()
                        .surface
                        .as_ref()
                        .unwrap()
                        .kind_with_nurb(k)
                        .unwrap(),
                    SurfaceKind::Plane(_)
                )
            })
            .unwrap()
    }

    fn on(k: &Kernel, s: &Surface, a: &Implicit) -> bool {
        lies_on(&as_b_spline(k, s).unwrap().unwrap(), a).unwrap()
    }

    fn up() -> Draft {
        Draft::new(Point3::ZERO, Vec3::Z, Vec3::Z, THETA).unwrap()
    }

    #[test]
    fn hostile_directions_are_refused() {
        for (n, p) in [
            (Vec3::ZERO, Vec3::Z),
            (Vec3::Z, Vec3::new(1e-13, 0.0, 0.0)),
            (Vec3::new(f64::NAN, 0.0, 1.0), Vec3::Z),
        ] {
            assert!(matches!(
                Draft::new(Point3::ZERO, n, p, THETA),
                Err(SolidError::InvalidInput { .. })
            ));
        }
        assert!(Draft::new(Point3::ZERO, Vec3::Z, Vec3::Z, f64::INFINITY).is_err());
        // made unit
        let d = Draft::new(
            Point3::ZERO,
            Vec3::new(0.0, 0.0, 3.0),
            Vec3::new(0.0, 0.0, 2.0),
            THETA,
        )
        .unwrap();
        assert_eq!((d.neutral_normal(), d.pull()), (Vec3::Z, Vec3::Z));
    }

    /// A box side drafted 5 degrees about the bottom, pulled up: the plane through its line
    /// on the bottom, x = 10 - z tan 5 degrees, a plane kept as its definition at bound 0.
    #[test]
    fn a_side_turns_about_its_line_on_the_neutral_plane() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = flat_face(&k, &b, 0, 10.0);
        let s = draft_surface(&k, &b, f, &up()).unwrap().unwrap();
        assert!(matches!(s.kind(), SurfaceKind::Plane(_)));
        let expected = Implicit::Plane {
            origin: Point3::new(10.0, 0.0, 0.0),
            normal: Vec3::new(THETA.cos(), 0.0, THETA.sin()),
        };
        assert!(on(&k, &s, &expected));
        // (a plane off by a micron is not it)
        assert!(!on(
            &k,
            &s,
            &Implicit::Plane {
                origin: Point3::new(10.001, 0.0, 0.0),
                normal: Vec3::new(THETA.cos(), 0.0, THETA.sin())
            }
        ));
        let blend = s.procedural_blend().unwrap();
        assert!(blend.draft().is_some());
        assert_eq!(blend.bound(), 0.0);
        assert_eq!(blend.recertify(&k).unwrap(), 0.0);
    }

    /// The neutral plane oblique to the pull: the hinge is the line where the face's plane
    /// meets it, x = 10 and 0.6 y + 0.8 z = 0, and the normal turns towards N's part in the
    /// face's plane, (0, 0.6, 0.8): the plane through (10, 0, 0) with normal
    /// (cos t, 0.6 sin t, 0.8 sin t). SolidCraft's hinge (the face's interior point moved along
    /// the pull onto the neutral plane, about n x p) gives the plane
    /// cos t (x - 10) + sin t (z + 0.75 y) = 0 through an interior point at height y: a
    /// different plane for every y.
    #[test]
    fn an_oblique_neutral_plane_hinges_on_its_trace() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = flat_face(&k, &b, 0, 10.0);
        let draft = Draft::new(Point3::ZERO, Vec3::new(0.0, 0.6, 0.8), Vec3::Z, THETA).unwrap();
        let s = draft_surface(&k, &b, f, &draft).unwrap().unwrap();
        // the hinge x = 10, 0.6 y + 0.8 z = 0 makes alpha with the pull, sin alpha = 0.8; the
        // face turns by phi, sin phi = sin t / 0.8, about it, to the normal
        // (cos phi, 0.6 sin phi, 0.8 sin phi): its component along the pull is sin t, so the
        // face is at exactly t to the pull
        let sp = THETA.sin() / 0.8;
        let cp = (1.0 - sp * sp).sqrt();
        let drafted = Vec3::new(cp, 0.6 * sp, 0.8 * sp);
        assert!((drafted.dot(Vec3::Z) - THETA.sin()).abs() < 1e-15);
        assert!(on(
            &k,
            &s,
            &Implicit::Plane {
                origin: Point3::new(10.0, 0.0, 0.0),
                normal: drafted
            }
        ));
        // turned by t about the hinge instead, the face would be at asin(0.8 sin t), 4 degrees
        let (c, sn) = (THETA.cos(), THETA.sin());
        assert!(!on(
            &k,
            &s,
            &Implicit::Plane {
                origin: Point3::new(10.0, 0.0, 0.0),
                normal: Vec3::new(c, 0.6 * sn, 0.8 * sn)
            }
        ));
        // SolidCraft's: the plane's offset at the origin depends on the interior point
        let old = |y: f64| {
            let inside = Point3::new(10.0, y, 5.0);
            let big_n = draft.neutral_normal();
            let hinge =
                inside + Vec3::Z * ((Point3::ZERO - inside).dot(big_n) / Vec3::Z.dot(big_n));
            let normal = Vec3::new(c, 0.0, sn);
            normal.dot(Point3::ZERO - hinge)
        };
        assert!(
            (old(2.0) - old(8.0)).abs() > 0.1,
            "{} {}",
            old(2.0),
            old(8.0)
        );
    }

    #[test]
    fn a_face_that_is_no_side_is_refused() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let top = flat_face(&k, &b, 2, 10.0);
        assert_eq!(
            draft_surface(&k, &b, top, &up()).unwrap().unwrap_err(),
            DraftRefusal::PlaneAcrossPull
        );
        let side = flat_face(&k, &b, 0, 10.0);
        let flat = Draft::new(Point3::ZERO, Vec3::X, Vec3::Z, THETA).unwrap();
        assert_eq!(
            draft_surface(&k, &b, side, &flat).unwrap().unwrap_err(),
            DraftRefusal::PullInNeutralPlane
        );
        // a side parallel to the neutral plane, the pull a hair off it: no hinge
        let parallel = Draft::new(Point3::ZERO, Vec3::X, Vec3::new(1e-7, 0.0, 1.0), THETA).unwrap();
        assert_eq!(
            draft_surface(&k, &b, side, &parallel).unwrap().unwrap_err(),
            DraftRefusal::PlaneParallelToNeutral
        );
        let ball = create_sphere(&mut k, Point3::ZERO, 5.0).unwrap();
        let f = brep_p1::faces(&ball, None).unwrap()[0];
        assert_eq!(
            draft_surface(&k, &ball, f, &up()).unwrap().unwrap_err(),
            DraftRefusal::NotPlaneOrCone
        );
    }

    /// A pin (the cylinder r 5 about z) drafted 5 degrees about its bottom: the cone of
    /// radius 5 - z tan 5 degrees; a hole (the same surface looked at from its axis) grows,
    /// 5 + z tan 5 degrees.
    #[test]
    fn a_pin_narrows_and_a_hole_widens() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
        let f = curved_face(&k, &b);
        let s = draft_surface(&k, &b, f, &up()).unwrap().unwrap();
        assert!(matches!(s.kind(), SurfaceKind::Cone(_)));
        let cone = |t: f64| Implicit::Cone {
            origin: Point3::ZERO,
            axis: Vec3::Z,
            radius: 5.0,
            tan_angle: t,
        };
        assert!(on(&k, &s, &cone(-THETA.tan())));
        assert!(!on(&k, &s, &cone(THETA.tan())));
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
        let SurfaceKind::Cone(c) = b
            .face(f)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .kind_with_nurb(&k)
            .unwrap()
        else {
            panic!()
        };
        let outward = b
            .faceuse(outer_faceuse(&b, f).unwrap().unwrap())
            .unwrap()
            .orientation
            == OrientType::Same;
        let hole = Surface::from(c.drafted_about_plane(&k, !outward, &up()).unwrap().unwrap());
        assert!(on(&k, &hole, &cone(THETA.tan())));
    }

    /// Radial drafting preserves the source axis and the angular frame. Re-fitting
    /// the two end circles can perturb that frame and lose exact coaxiality.
    #[test]
    fn a_drafted_cylinder_keeps_its_source_axis() {
        use crate::geom::expansion as exact;
        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let b = create_cylinder(&mut k, Point3::ZERO, 10.0, 20.0).unwrap();
        let f = curved_face(&k, &b);
        let SurfaceKind::Cone(source) = b.face(f).unwrap().surface.as_ref().unwrap().kind() else {
            panic!("cylinder")
        };
        let frame = source.canonical().origin;
        for pull in [Vec3::Z, -Vec3::Z] {
            for degrees in [3.0_f64, 5.0] {
                let draft = Draft::new(
                    Point3::new(0.0, 0.0, 10.0),
                    Vec3::Z,
                    pull,
                    degrees.to_radians(),
                )
                .unwrap();
                let s = draft_surface(&k, &b, f, &draft).unwrap().unwrap();
                let SurfaceKind::Cone(cone) = s.kind() else {
                    panic!("drafted cone")
                };
                let placement = cone.canonical().origin;
                assert_eq!(placement.x_axis(), frame.x_axis());
                assert_eq!(placement.y_axis(), frame.y_axis());
                let axis = frame.x_axis().cross(frame.y_axis());
                let was = exact::cross(frame.origin(), axis).unwrap();
                let now = exact::cross(placement.origin(), axis).unwrap();
                for (a, c) in was.iter().zip(&now) {
                    assert!(
                        exact::is_zero(&exact::sub(a, c)),
                        "pull={pull:?}, angle={degrees}: {placement:?} vs {frame:?}"
                    );
                }
                assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
            }
        }
    }

    /// A frustum r 10 at its base to 8 at its top, 20 tall, drafted 5 degrees about its base:
    /// its generators turn 5 degrees more, radius 10 + z tan(atan(-0.1) - 5 degrees).
    #[test]
    fn a_cone_about_the_pull_turns_its_generators() {
        let mut k = Kernel::default();
        let b =
            crate::primitive::api_primitives::create_cone(&mut k, Point3::ZERO, 10.0, 8.0, 20.0)
                .unwrap();
        let f = curved_face(&k, &b);
        let s = draft_surface(&k, &b, f, &up()).unwrap().unwrap();
        let t = ((-0.1_f64).atan() - THETA).tan();
        assert!(on(
            &k,
            &s,
            &Implicit::Cone {
                origin: Point3::ZERO,
                axis: Vec3::Z,
                radius: 10.0,
                tan_angle: t
            }
        ));
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
        // Cancelling the source slope gives a zero-angle cone on the same axis;
        // its analytic label must agree with the cylinder-shaped edited net.
        let SurfaceKind::Cone(source) = b.face(f).unwrap().surface.as_ref().unwrap().kind() else {
            panic!("cone")
        };
        let cancel = Draft::new(
            Point3::ZERO,
            Vec3::Z,
            Vec3::Z,
            source.tan_semi_angle().atan(),
        )
        .unwrap();
        let straight = draft_surface(&k, &b, f, &cancel).unwrap().unwrap();
        assert!(on(
            &k,
            &straight,
            &Implicit::Cone {
                origin: Point3::ZERO,
                axis: Vec3::Z,
                radius: 10.0,
                tan_angle: 0.0,
            }
        ));
        assert_eq!(
            straight.procedural_blend().unwrap().recertify(&k).unwrap(),
            0.0
        );
    }

    /// A cone of radius 10 at z = 0 and 5 at z = 10 has its apex at z = 20: about the plane
    /// z = 25 its radius is -2.5, about z = 20 it is 0, and neither has a hinge circle. The
    /// refusal says so, apart from a cone that closes within the face's span.
    #[test]
    fn a_cone_about_a_plane_at_or_past_its_apex_is_refused_as_such() {
        let mut k = Kernel::default();
        let b =
            crate::primitive::api_primitives::create_cone(&mut k, Point3::ZERO, 10.0, 5.0, 10.0)
                .unwrap();
        let f = curved_face(&k, &b);
        for height in [25.0, 20.0] {
            let past = Draft::new(Point3::new(0.0, 0.0, height), Vec3::Z, Vec3::Z, THETA).unwrap();
            assert_eq!(
                draft_surface(&k, &b, f, &past).unwrap().unwrap_err(),
                DraftRefusal::NeutralPlanePastApex
            );
        }
        let below = Draft::new(Point3::new(0.0, 0.0, 15.0), Vec3::Z, Vec3::Z, THETA).unwrap();
        assert!(draft_surface(&k, &b, f, &below).unwrap().is_ok());
    }

    #[test]
    fn a_cylinder_off_the_pull_is_refused() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
        let f = curved_face(&k, &b);
        let sideways = Draft::new(Point3::ZERO, Vec3::X, Vec3::X, THETA).unwrap();
        assert_eq!(
            draft_surface(&k, &b, f, &sideways).unwrap().unwrap_err(),
            DraftRefusal::AxisNotAlongPull
        );
        let lying = Draft::new(Point3::ZERO, Vec3::X, Vec3::Z, THETA).unwrap();
        assert_eq!(
            draft_surface(&k, &b, f, &lying).unwrap().unwrap_err(),
            DraftRefusal::AxisInNeutralPlane
        );
    }

    /// A definition that is not the net's (another angle) does not certify it; the net and
    /// its definition moved together still do.
    #[test]
    fn a_moved_input_is_refused_and_a_moved_draft_certifies() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = flat_face(&k, &b, 0, 10.0);
        let mut s = draft_surface(&k, &b, f, &up()).unwrap().unwrap();
        let src = b.face(f).unwrap().surface.as_ref().unwrap();
        let other = DraftDefinition::create(
            &k,
            src,
            true,
            Draft::new(Point3::ZERO, Vec3::Z, Vec3::Z, 2.0 * THETA).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            other.recertify(&k, &s),
            Err(SolidError::InvalidInput { .. })
        ));
        let moved = DraftDefinition::create(
            &k,
            src,
            true,
            Draft::new(Point3::new(0.0, 0.0, 1.0), Vec3::Z, Vec3::Z, THETA).unwrap(),
        )
        .unwrap();
        assert!(moved.recertify(&k, &s).is_err());
        let mut turn = Axis2Placement::IDENTITY;
        turn.rotate_about_axis_at_point(0.7, Point3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.6, 0.8));
        s.transform(&k, &turn, None).unwrap();
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }

    /// The sense proved on the net's corners is the kernel's own normal's: the sign of the
    /// direction away from the axis against the evaluated normal, in the middle of the
    /// domain, for cylinders, frustums and a cone to its apex (a corner with no derivative),
    /// each with its parameters swapped and its linear direction reversed, about a tilted
    /// frame off the origin.
    #[test]
    fn the_proved_sense_is_the_evaluated_normals() {
        use crate::nurbs::knots::Side;
        let k = Kernel::default();
        let tilted = Axis2Placement::new(
            Point3::new(3.0, -2.0, 7.0),
            Vec3::new(0.6, 0.8, 0.0),
            Vec3::new(-0.48, 0.36, 0.8),
        );
        let mut checked = 0;
        for frame in [Axis2Placement::IDENTITY, tilted] {
            for (bot, top) in [(5.0, 5.0), (10.0, 8.0), (4.0, 9.0), (6.0, 0.0)] {
                for swap in [false, true] {
                    for inside in [false, true] {
                        let cone = Cone::create_canonical(&k, &frame, bot, top, 10.0, swap, inside)
                            .unwrap();
                        let axis = cone.canonical().origin;
                        let z = axis.x_axis().cross(axis.y_axis());
                        let surf = Surface::from(cone.clone());
                        let uv = surf
                            .natural_uv_domain_with_nurb(&k)
                            .unwrap()
                            .evaluate(0.5, 0.5);
                        let x = surf.evaluate_point(&k, uv).unwrap();
                        let n = surf
                            .evaluate_normal(&k, uv, Side::Left, Side::Left)
                            .unwrap();
                        let d = x - axis.origin();
                        let radial = d - z * (d.dot(z) / z.dot(z));
                        let proved =
                            net_sense(cone.rev.base(), axis.origin(), axis.x_axis(), axis.y_axis())
                                .unwrap();
                        assert_eq!(
                            proved,
                            Some(radial.dot(n).signum()),
                            "{bot} {top} swap {swap} inside {inside}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 32);
    }

    /// An interval holding 0 proves no sign; one clear of it does.
    #[test]
    fn a_sign_within_its_rounding_is_not_proved() {
        let tiny = Ivl::from(1.0) - Ivl::from(1.0);
        assert_eq!(proved_sign(tiny), None);
        assert_eq!(proved_sign(Ivl::from(1.0) + Ivl::from(1e-300)), Some(1.0));
        assert_eq!(proved_sign(Ivl::from(-2.0) * Ivl::from(3.0)), Some(-1.0));
        assert_eq!(proved_sign(Ivl::new(f64::NAN, f64::NAN)), None);
        // a plane at 1e-170 radians to the neutral plane: |n x N|^2 underflows, its interval
        // holds 0, so no hinge is proved
        let d = Draft::new(
            Point3::ZERO,
            Vec3::new(1.0, 1e-170, 0.0),
            Vec3::new(1e-7, 0.0, 1.0),
            THETA,
        )
        .unwrap();
        assert_eq!(
            plane_draft(Point3::ZERO, Vec3::X, &d).unwrap_err(),
            DraftRefusal::PlaneParallelToNeutral
        );
    }

    /// A draft steeper than its hinge allows: the hinge x = 10, 0.99 y + 0.141 z = 0 is at
    /// sin alpha = 0.141 to the pull, so 10 degrees (sin 0.174) cannot be reached about it;
    /// 5 degrees (sin 0.087) can.
    #[test]
    fn a_draft_steeper_than_its_hinge_is_refused() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = flat_face(&k, &b, 0, 10.0);
        let n = Vec3::new(0.0, 0.99, 0.141);
        let steep = Draft::new(Point3::ZERO, n, Vec3::Z, 2.0 * THETA).unwrap();
        assert_eq!(
            draft_surface(&k, &b, f, &steep).unwrap().unwrap_err(),
            DraftRefusal::AngleBeyondHinge
        );
        let fine = Draft::new(Point3::ZERO, n, Vec3::Z, THETA).unwrap();
        let s = draft_surface(&k, &b, f, &fine).unwrap().unwrap();
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }

    /// A face 1e-7 radians off parallel to a neutral plane that holds the pull to 1e-7: its
    /// hinge is proved to exist but not placed to the re-certification's rounding, a typed
    /// refusal (never an `InvalidInput` from the re-certification, nor a NaN hinge).
    #[test]
    fn an_ill_conditioned_hinge_is_refused() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = flat_face(&k, &b, 0, 10.0);
        let d = Draft::new(
            Point3::new(3.0, 0.0, 0.0),
            Vec3::new(1.0, 1e-7, 0.0),
            Vec3::new(1e-7, 0.0, 1.0),
            THETA,
        )
        .unwrap();
        assert_eq!(
            draft_surface(&k, &b, f, &d).unwrap().unwrap_err(),
            DraftRefusal::PlaneParallelToNeutral
        );
    }

    /// The pull against a cylinder's axis: a pin pulled down narrows going down, so drafted
    /// about its bottom it widens going up, radius 5 + z tan t.
    #[test]
    fn a_pin_pulled_against_its_axis() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
        let f = curved_face(&k, &b);
        let down = Draft::new(Point3::ZERO, Vec3::Z, -Vec3::Z, THETA).unwrap();
        let s = draft_surface(&k, &b, f, &down).unwrap().unwrap();
        assert!(on(
            &k,
            &s,
            &Implicit::Cone {
                origin: Point3::ZERO,
                axis: Vec3::Z,
                radius: 5.0,
                tan_angle: THETA.tan()
            }
        ));
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }

    /// A drafted face's definition moves rigidly or by a uniform scale with its net; a scale
    /// that is not uniform is refused (the kept-definition rule, as for exact rounds), the
    /// surface unchanged.
    #[test]
    fn a_drafted_face_refuses_a_scale_that_is_not_uniform() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
        let f = curved_face(&k, &b);
        let mut s = draft_surface(&k, &b, f, &up()).unwrap().unwrap();
        let before = draft_to_record(&k, &s).unwrap();
        assert!(matches!(
            s.transform(
                &k,
                &Axis2Placement::IDENTITY,
                Some(Vec3::new(1.0, 2.0, 1.0))
            ),
            Err(SolidError::InvalidInput { .. })
        ));
        assert_eq!(draft_to_record(&k, &s).unwrap(), before);
        s.transform(
            &k,
            &Axis2Placement::IDENTITY,
            Some(Vec3::new(2.0, 2.0, 2.0)),
        )
        .unwrap();
        let mut turn = Axis2Placement::IDENTITY;
        turn.rotate_about_axis_at_point(0.7, Point3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.6, 0.8));
        s.transform(&k, &turn, None).unwrap();
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }

    /// A drafted plane and a drafted cone through their record and back, the same numbers,
    /// re-certified.
    #[test]
    fn a_draft_goes_through_its_record_and_certifies_again() {
        let mut k = Kernel::default();
        let bx = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let cyl = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
        let frustum =
            crate::primitive::api_primitives::create_cone(&mut k, Point3::ZERO, 10.0, 8.0, 20.0)
                .unwrap();
        let fs = [
            (&bx, flat_face(&k, &bx, 0, 10.0)),
            (&cyl, curved_face(&k, &cyl)),
            (&frustum, curved_face(&k, &frustum)),
        ];
        for (b, f) in fs {
            let s = draft_surface(&k, b, f, &up()).unwrap().unwrap();
            let rec = draft_to_record(&k, &s).unwrap();
            let json = serde_json::to_string(&rec).unwrap();
            let back: crate::surface::procedural_blend::record::DraftRecord =
                serde_json::from_str(&json).unwrap();
            assert_eq!(back, rec);
            let rebuilt = draft_rebuild(&k, &back).unwrap();
            assert_eq!(draft_to_record(&k, &rebuilt).unwrap(), rec);
            // a net moved off its definition in the record is refused
            let mut off = rec.clone();
            off.angle = 2.0 * THETA;
            assert!(draft_rebuild(&k, &off).is_err());
        }
    }

    /// A 40 x 40 x 20 block with an r = 1 hole through it at (37, 20), 2 from the x = 40 side.
    /// Drafted 30 degrees inwards about its bottom, that side reaches x = 40 - z tan 30 degrees,
    /// so the hole's wall, which meets the side nowhere, lies inside the band the draft takes
    /// off and over the side itself: the draft would cut through it (#19686).
    #[test]
    fn a_draft_through_a_hole_near_the_side_is_reported() {
        use crate::boolean::api_brep::boolean_difference;
        let mut k = Kernel::default();
        let block = create_box(&mut k, Point3::ZERO, 40.0, 40.0, 20.0).unwrap();
        let bore = create_cylinder(&mut k, Point3::new(37.0, 20.0, -1.0), 1.0, 22.0).unwrap();
        let mut b = boolean_difference(&mut k, block, bore).unwrap();
        let wall = curved_face(&k, &b);
        let f = flat_face(&k, &b, 0, 40.0);
        let steep = Draft::new(Point3::ZERO, Vec3::Z, Vec3::Z, THIRTY).unwrap();
        let s = draft_surface(&k, &b, f, &steep).unwrap().unwrap();
        let through = drafted_through(&k, &mut b, &[f], &[s]).unwrap().unwrap();
        assert_eq!((through.drafted, through.through), (f, wall));
        // the point is on the wall, behind the side's plane and past the drafted one:
        // 40 - z tan 30 degrees < x < 40
        let r = ((through.at.x - 37.0).powi(2) + (through.at.y - 20.0).powi(2)).sqrt();
        assert!((r - 1.0).abs() < 1e-9, "not on the wall: {:?}", through.at);
        let reached = 40.0 - through.at.z * THIRTY.tan();
        assert!(
            reached < through.at.x && through.at.x < 40.0,
            "{:?} {}",
            through.at,
            reached
        );
        assert!(through.depth > 0.0);
    }

    /// The same block with a pocket cut through the x = 40 side, opening y 15..25, z 5..15 and
    /// 5 deep, so that side is a rectangle with a rectangular hole in it and the pocket's floor
    /// is the face at x = 35. The drafted plane does reach past that floor (40 - 15 tan 30
    /// degrees is 31.3), but every point of the floor meets the side's plane inside the
    /// pocket's opening, over no part of the side: the drafted face passes through the empty
    /// pocket and cuts nothing. The box of the side's vertices, which knows only the side's
    /// outer edges, would name the floor.
    #[test]
    fn a_draft_into_the_empty_space_of_a_pocket_is_not_reported() {
        use crate::boolean::api_brep::boolean_difference;
        let mut k = Kernel::default();
        let block = create_box(&mut k, Point3::ZERO, 40.0, 40.0, 20.0).unwrap();
        let pocket = create_box(&mut k, Point3::new(35.0, 15.0, 5.0), 6.0, 10.0, 10.0).unwrap();
        let mut b = boolean_difference(&mut k, block, pocket).unwrap();
        let f = flat_face(&k, &b, 0, 40.0);
        // the pocket is there, and its floor is a face of its own, meeting the side nowhere
        let floor = flat_face(&k, &b, 0, 35.0);
        assert_ne!(floor, f);
        let steep = Draft::new(Point3::ZERO, Vec3::Z, Vec3::Z, THIRTY).unwrap();
        let s = draft_surface(&k, &b, f, &steep).unwrap().unwrap();
        // the drafted plane is the one the case is read against, and it does reach past the floor
        assert!(on(
            &k,
            &s,
            &Implicit::Plane {
                origin: Point3::new(40.0, 0.0, 0.0),
                normal: Vec3::new(THIRTY.cos(), 0.0, THIRTY.sin())
            }
        ));
        let reaches = 40.0 - 15.0 * THIRTY.tan();
        assert!(
            reaches < 35.0,
            "the drafted plane does not reach the floor: {reaches}"
        );
        assert_eq!(drafted_through(&k, &mut b, &[f], &[s]).unwrap(), None);
    }
}
