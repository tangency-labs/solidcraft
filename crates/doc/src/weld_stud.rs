//! Stud welds: the collar ring a stud weld leaves round a stud's foot ([`stud_weld`]), and where
//! it sits: from the stud's foot edge, from the stud's side face and the face it stands on, or
//! from a point and a diameter with no stud modelled ([`stud_site_from_edge`],
//! [`stud_site_from_side`], [`stud_site_at_point`]; Q174). The stud is the user's own part; the
//! weld is the collar only (Q53).
//!
//! SolidCraft's own, as [`crate::weld_spot`] is: every measurement and every body is a kernel call;
//! this module adds the weld's rules and sizes. The `weld.stud` command and its storage come with
//! #15131.
//!
//! # The geometry
//!
//! A ring about the stud's axis from the stud's side (radius `d / 2`) out to `outer / 2` on the
//! face, `height` up the stud, its outside a convex circular arc (Q175, as a ceramic ferrule forms
//! it) meeting the face square at the toe. Display only: the parts are never changed.
//!
//! # Sizes
//!
//! [`proposed_collar`]: drawn arc stud welding with a ceramic ferrule or shielding gas (ISO 4063
//! 783) takes ISO 13918:2017 Table 7 (unthreaded studs, UD: weld collar d3 and h4, guidance values,
//! welding position PA) at the stud's diameter, between its rows by straight lines and past its
//! ends in proportion (Q203). ISO 13918 gives no collar for short-cycle (784) or tip-ignition
//! (786) studs, which are flanged: those take proportions of the diameter, not from a standard.

use std::f64::consts::TAU;

use solidcore_record::Vec3;
use solidcore_record::brep::{Curve as RecordCurve, Surface};
use solidcraft_kernel::curves as curve;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, KernelError, affine};

use crate::weld_seam::{add, scale};
use crate::weld_spot::positive;

/// Why a stud weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StudRefusal {
    /// Input the weld cannot take: a size, point or axis out of range, or sizes the model cannot
    /// hold.
    #[error("{0}")]
    Invalid(&'static str),
    /// The picked edge is not a circle, or the picked side face not a cylinder.
    #[error("the picked {0} is not the round foot of a stud: pick its foot edge or its round side")]
    NotRound(&'static str),
    /// The face the stud stands on is not flat.
    #[error("the face the stud stands on is not flat: stud welds on curved faces are not built yet")]
    FaceNotFlat,
    /// The stud's foot stands `gap` off the face.
    #[error("the stud's foot stands {gap} off the face: move it down onto the face")]
    OffFace { gap: f64 },
    /// The stud's axis does not meet the face square, and its foot was given by its side face.
    #[error("the stud is not square to the face: pick its foot edge instead")]
    NotSquare,
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A curve call the kernel refused, in its words.
    #[error(transparent)]
    Curve(#[from] curve::CurveRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, StudRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(StudRefusal::Invalid(why))
}

/// The stud welding process (ISO 4063), which sets the collar a new weld opens with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudProcess {
    /// 783: drawn arc stud welding with a ceramic ferrule or shielding gas.
    DrawnArc,
    /// 784: short-cycle drawn arc stud welding.
    ShortCycle,
    /// 786: capacitor discharge stud welding with tip ignition.
    TipIgnition,
}

/// A stud weld's collar: its outer diameter on the face and its height up the stud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StudCollar {
    pub outer: f64,
    pub height: f64,
}

/// ISO 13918:2017 Table 7 (UD), `(d1, d3, h4)`: the stud's diameter, the weld collar's diameter and
/// height, guidance values.
const UD: [(f64, f64, f64); 6] = [(6.0, 8.5, 4.0), (8.0, 11.0, 4.0), (10.0, 13.0, 4.0), (12.0, 16.0, 5.0), (14.6, 18.5, 6.0), (16.0, 21.0, 7.0)];

/// The collar a new weld on a stud of diameter `d` opens with, by `process` (Q176, Q203).
pub fn proposed_collar(d: f64, process: StudProcess) -> StudCollar {
    match process {
        StudProcess::DrawnArc => {
            let (first, last) = (UD[0], UD[UD.len() - 1]);
            let (outer, height) = if d <= first.0 {
                (first.1 * d / first.0, first.2 * d / first.0)
            } else if d >= last.0 {
                (last.1 * d / last.0, last.2 * d / last.0)
            } else {
                let i = UD.iter().position(|r| r.0 >= d).unwrap_or(UD.len() - 1).max(1);
                let (a, b) = (UD[i - 1], UD[i]);
                let f = (d - a.0) / (b.0 - a.0);
                (a.1 + f * (b.1 - a.1), a.2 + f * (b.2 - a.2))
            };
            StudCollar { outer, height }
        }
        // shortcut: no standard gives these collars (ISO 13918's short-cycle and tip-ignition studs
        // are flanged); proportions proposed 27 Nov, the user changes them. Replace if a source turns up.
        StudProcess::ShortCycle => StudCollar { outer: 1.15 * d, height: 0.15 * d },
        StudProcess::TipIgnition => StudCollar { outer: 1.05 * d, height: 0.05 * d },
    }
}

/// One stud weld.
#[derive(Debug, Clone, PartialEq)]
pub struct StudWeld {
    /// The middle of the stud's foot, on the face it stands on.
    pub at: [f64; 3],
    /// The stud's axis, out of the face (need not be unit).
    pub axis: [f64; 3],
    /// The stud's diameter at its foot.
    pub diameter: f64,
    pub collar: StudCollar,
}

/// Something about a stud weld the user should know, though it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StudNotice {
    /// The stud's axis leans `by` (radians) off the face's normal; the collar is built round the
    /// stud's axis.
    Tilted { by: f64 },
    /// The stud's foot lies `by` into the face.
    Sunk { by: f64 },
}

/// A stud weld's collar, and what the user should know about it.
#[derive(Debug)]
pub struct Stud {
    pub body: Body,
    pub notices: Vec<StudNotice>,
}

/// The collar of `weld`.
///
/// # Errors
/// [`StudRefusal::Invalid`] for a point, axis or size that is not usable (a collar no wider than the
/// stud), and sizes the model cannot hold. The errors of the kernel calls otherwise.
pub fn stud_weld(weld: &StudWeld) -> Result<Body> {
    let (r, ro, h) = (weld.diameter / 2.0, weld.collar.outer / 2.0, weld.collar.height);
    if !(positive(r) && positive(h) && ro.is_finite() && ro > r) {
        return bad("a stud diameter or collar that is not finite and positive, or a collar no wider than the stud");
    }
    if !weld.at.iter().all(|v| v.is_finite()) || weld.axis == [0.0; 3] || !weld.axis.iter().all(|v| v.is_finite()) {
        return bad("a stud point or axis that is not finite, or a zero axis");
    }
    let reach = weld.at.iter().fold(0.0_f64, |m, c| m.max(c.abs())) + ro + h;
    if !reach.is_finite() || reach > profiles::MAX_COORD {
        return bad("a stud so large that it passes the model's size limit");
    }
    let [a, u] = affine::orthonormal_axes(weld.axis, [1.0, 0.0, 0.0]).or_else(|_| affine::orthonormal_axes(weld.axis, [0.0, 1.0, 0.0]))?;
    let p = |rho: f64, z: f64| add(add(weld.at, scale(u, rho)), scale(a, z));
    // the arc's middle: the circle through the toe and the top, centred on the face (meeting it
    // square) where the collar is narrower than high, else on the stud's side line
    let w = ro - r;
    let mid = if w < h {
        let c = (ro * ro - r * r - h * h) / (2.0 * w);
        let radius = ro - c;
        (c + (radius * radius - h * h / 4.0).sqrt(), h / 2.0)
    } else {
        let k = (h * h - w * w) / (2.0 * h);
        let radius = h - k;
        (r + w / 2.0, k + (radius * radius - w * w / 4.0).sqrt())
    };
    let arc = curve::arc_through([p(ro, 0.0), p(mid.0, mid.1), p(r, h)])?;
    let mut profile = vec![line(p(r, 0.0), p(ro, 0.0))];
    profile.extend(profiles::curve_pieces(&arc, [0.0, 1.0])?);
    profile.push(line(p(r, h), p(r, 0.0)));
    Ok(profiles::revolve_curves(&profile, weld.at, a, TAU, true, None)?)
}

/// The stud weld on the stud whose foot is circular edge `edge` of `stud`, standing on record face
/// `face` of `base`, with `collar` (`None` opens [`proposed_collar`] for `process`). `tolerance` is
/// the weld tolerance (Q135). Both bodies are read, never changed.
///
/// # Errors
/// [`StudRefusal::NotRound`] for an edge that is not a circle, [`StudRefusal::FaceNotFlat`],
/// [`StudRefusal::OffFace`] for a foot standing off the face by more than the tolerance; those of
/// [`stud_weld`] otherwise.
pub fn stud_site_from_edge(
    stud: &Body,
    edge: usize,
    base: &Body,
    face: usize,
    process: StudProcess,
    collar: Option<StudCollar>,
    tolerance: f64,
) -> Result<Stud> {
    let Some(RecordCurve::Circle { frame, radius }) = stud.record().edges.get(edge).map(|e| &e.curve) else {
        return Err(StudRefusal::NotRound("edge"));
    };
    let axis = cross(frame.x_axis, frame.y_axis);
    on_face(base, face, frame.origin, axis, 2.0 * radius, process, collar, tolerance)
}

/// The stud weld on the stud whose round side is record face `side` of `stud`, standing on record
/// face `face` of `base`, square to it; as [`stud_site_from_edge`] otherwise.
///
/// # Errors
/// [`StudRefusal::NotRound`] for a side face that is not a cylinder, [`StudRefusal::NotSquare`] for
/// a stud not square to the face; those of [`stud_site_from_edge`] otherwise.
pub fn stud_site_from_side(
    stud: &Body,
    side: usize,
    base: &Body,
    face: usize,
    process: StudProcess,
    collar: Option<StudCollar>,
    tolerance: f64,
) -> Result<Stud> {
    let Some(Surface::Cylinder { frame, radius }) = stud.record().faces.get(side).map(|f| &f.surface) else {
        return Err(StudRefusal::NotRound("side face"));
    };
    let axis = cross(frame.x_axis, frame.y_axis);
    // where the axis meets the face's plane: the face's level along the axis from the frame
    let Some(level) = base.face_square_level(face, vec3(axis), vec3(frame.origin))? else {
        return Err(StudRefusal::NotSquare);
    };
    let foot = add(frame.origin, scale(axis, level.level));
    on_face(base, face, foot, axis, 2.0 * radius, process, collar, tolerance)
}

/// The stud weld of a stud of `diameter` at `point` on record face `face` of `base`, with no stud
/// modelled; as [`stud_site_from_edge`] otherwise.
///
/// # Errors
/// Those of [`stud_site_from_edge`].
pub fn stud_site_at_point(
    base: &Body,
    face: usize,
    point: [f64; 3],
    diameter: f64,
    process: StudProcess,
    collar: Option<StudCollar>,
    tolerance: f64,
) -> Result<Stud> {
    let Some(n) = base.face_plane_normal(face)? else {
        return Err(StudRefusal::FaceNotFlat);
    };
    on_face(base, face, point, n.value, diameter, process, collar, tolerance)
}

/// The stud weld of a stud of `diameter` whose foot's middle is `at` and axis `axis`, on record face
/// `face` of `base`.
fn on_face(
    base: &Body,
    face: usize,
    at: [f64; 3],
    axis: [f64; 3],
    diameter: f64,
    process: StudProcess,
    collar: Option<StudCollar>,
    tolerance: f64,
) -> Result<Stud> {
    if !positive(tolerance) || !positive(diameter) {
        return bad("a weld tolerance or stud diameter that is not finite and positive");
    }
    let Some(n) = base.face_plane_normal(face)? else {
        return Err(StudRefusal::FaceNotFlat);
    };
    let n = n.value;
    let Some(level) = base.face_square_level(face, vec3(n), vec3(at))? else {
        return Err(StudRefusal::FaceNotFlat);
    };
    // the face lies `level` from the foot along n: below it (negative) the foot stands off
    let mut notices = Vec::new();
    if -level.level > tolerance {
        return Err(StudRefusal::OffFace { gap: -level.level });
    }
    if level.level > tolerance {
        notices.push(StudNotice::Sunk { by: level.level });
    }
    let [a, _] = affine::orthonormal_axes(axis, [1.0, 0.0, 0.0]).or_else(|_| affine::orthonormal_axes(axis, [0.0, 1.0, 0.0]))?;
    let [n, _] = affine::orthonormal_axes(n, [1.0, 0.0, 0.0]).or_else(|_| affine::orthonormal_axes(n, [0.0, 1.0, 0.0]))?;
    // the stud's axis out of the face, and how far it leans
    let along = affine::distance_along([0.0; 3], n, a);
    let a = if along < 0.0 { scale(a, -1.0) } else { a };
    let lean = along.abs().clamp(-1.0, 1.0).acos();
    if lean * diameter > tolerance {
        notices.push(StudNotice::Tilted { by: lean });
    }
    let collar = collar.unwrap_or_else(|| proposed_collar(diameter, process));
    let body = stud_weld(&StudWeld { at: add(at, scale(n, level.level)), axis: a, diameter, collar })?;
    Ok(Stud { body, notices })
}

/// `a × b`. shortcut: plain arithmetic in the app, as the seam's `len` is; the record frame's
/// normal, not a kernel call. Move to the kernel if a reviewer asks.
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

#[cfg(test)]
#[path = "weld_stud_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
