//! A controlled sweep along a path with corners: built piece by piece, exactly, the pieces
//! meeting at the corners' mitres.
//!
//! Upstream: none (not in SMLib). SolidCraft builds the same body (`cornered_controlled`,
//! `crates/kernel/src/smlib_sweeps.rs` at 52afe21) by sweeping each run, carrying it on past
//! its corners as a prism, cutting it back to the plane halving the corner and uniting the
//! runs. Here no boolean is needed: each piece of the path is swept on its own, as
//! [`super::segments`] sweeps a path of lines and arcs, and a piece arriving at a corner and
//! the piece leaving it end on one section, the corner's mitre.
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`cornered_sweep`] | (none; SolidCraft's `cornered_controlled`) |
//!
//! **The sections.** Each joint of the path (and its start and end) has one section, placed
//! once from its station ([`super::stations`]) and handed to both pieces that meet there, so
//! the pieces share their boundaries by construction, not by matching two computations. At a
//! joint where the section is a rigid image of the profile (no corner, scale 1) it is the
//! profile's own curves moved, so each keeps its class (a line stays a line, an arc an arc);
//! elsewhere it is the affine image of their B-splines. Round a closed path the end's section
//! is the start's.
//!
//! **The mitre.** A station asked as [`Stop::Corner`] is the corner's mitre: the rotation by
//! half the corner's angle `theta`, stretched by `1 / cos(theta / 2)` along `t_1 - t_0`. It
//! takes each point of the section arriving at the corner (square to `t_0`) along `t_0` onto
//! the plane through the corner halving it, whose normal is `t_0 + t_1`; the section leaving
//! the corner (square to `t_1`) goes to the same points along `t_1`.
//!
//! **The pieces, each exact.**
//! - A **line** moves every point of its start section along a straight line to the same
//!   point of its end section, at a scale linear in arc length (the end scale's law), also to
//!   or from a mitre: the skin of degree 1 between its two sections is the swept body itself,
//!   its walls ruled (planes, cylinders, a frustum's cones and their oblique cuts).
//! - An **arc** with no scale is the rotational sweep of its start section about the arc's
//!   axis ([`create_rotational_sweep`]), its walls the analytic surfaces of revolution.
//!
//! The pieces' walls are copied into one body, capped at the ends of an open path, and sewn
//! along the sections they share ([`stitch_and_orient`]), as [`super::segments`] sews its.
//!
//! **Refused, by name.** A twist or guide rails (the section turns or deforms along a piece:
//! no piece above is exact for it); a helix (no exact class: the procedural sweep's ledger
//! `path-spline`); an arc with an end scale (ledger `path-scaled`); an arc ending at a corner
//! (its sweep would have to end on the plane halving the corner); a closed path whose section
//! changes round it (an end scale other than 1: tangency-labs/solidcore#3192), or that closes
//! at a corner (its closing mitre is not one of the path's
//! [`super::stations::Path::corner_fractions`]).
//!
//! Deviations: no upstream counterpart; at most [`super::stations::MAX_PIECES`] pieces. No
//! recursion.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::kernel::Kernel;
use crate::math::{Transform, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::procedural_sweep::path::G1_TANGENTS;
use crate::sweep::api_brep::create_rotational_sweep;
use crate::sweep::api_surfaces::create_skin_primitive;
use crate::sweep::segments::build_faces;
use crate::sweep::stations::{stations, Laws, Path, Piece, Station, Stop};
use crate::topology::brep::p1 as brep_p1;
use crate::topology::brep::p2::{is_manifold_solid, ArenaNotifyState};
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use crate::topology::brep::p7::copy_faces;
use crate::topology::brep::p8::create_planar_faces_with_3d_curves;
use crate::topology::{Brep, FaceId};

const OP: &str = "sweep::cornered";

fn invalid(why: &'static str) -> SolidError {
    SolidError::invalid_input(OP, why)
}

fn unsupported(why: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason: why,
    }
}

/// The profile curve as a B-spline (lines and conics by their stored NURBS forms).
fn b_spline(c: &Curve) -> Result<BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Ok(b.clone()),
        CurveKind::Line(l) => Ok(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Ok(e.base().clone()),
        _ => Err(unsupported("a profile curve with no B-spline form")),
    }
}

/// The affine map of `s` (the path's first station `first`): `q -> m q + (at - m at_0)`, in
/// [`Transform`]'s rows (the images of the axes, then the translation).
fn transform_of(s: &Station, first: &Station) -> Transform {
    let t = s.translation(first);
    let row = |v: Vec3, w: f64| [v.x, v.y, v.z, w];
    let [x, y, z] = s.m;
    Transform {
        m: [row(x, 0.0), row(y, 0.0), row(z, 0.0), row(t, 1.0)],
    }
}

/// The section `profile` takes at `s`: its own curves moved when `rigid` (the station's map a
/// rotation: no corner, scale 1), so each keeps its class; else the affine images of their
/// B-splines.
fn section(
    k: &Kernel,
    profile: &[Curve],
    s: &Station,
    first: &Station,
    rigid: bool,
) -> Result<Vec<Curve>> {
    if rigid {
        let [x, y, _] = s.m;
        let motion = Axis2Placement::new(s.translation(first), x, y);
        return profile
            .iter()
            .map(|c| {
                let mut moved = c.clone();
                moved.transform(k, &motion, None)?;
                Ok(moved)
            })
            .collect();
    }
    let t = transform_of(s, first);
    profile
        .iter()
        .map(|c| Ok(Curve::from(b_spline(c)?.affine_image(&t)?.0)))
        .collect()
}

/// The faces of `piece` copied into `b`.
fn copy_in(k: &mut Kernel, piece: &mut Brep, b: &mut Brep) -> Result<()> {
    let faces = brep_p1::faces(piece, None)?;
    let mut state = ArenaNotifyState::new(piece);
    state.seed(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let (_, copied) = copy_faces(k, piece, &faces, b, false, &this_face, &mut env)?;
    if !copied {
        return Err(SolidError::sweep_failed(
            "CopyFaces",
            "the corner pieces' faces could not be copied",
        ));
    }
    Ok(())
}

/// A line's or an arc's unit tangent at its start, or at its end with `at_end`; `None` for a
/// helix (refused before this is asked).
fn tangent(p: &Piece, at_end: bool) -> Option<Vec3> {
    let unit = |v: Vec3| {
        let l = v.length();
        (l > 0.0).then(|| v * (1.0 / l))
    };
    match *p {
        Piece::Line { a, b } => unit(b - a),
        Piece::Arc {
            centre,
            axis,
            start,
            angle,
        } => {
            let r = start - centre;
            let r = if at_end {
                // the radius turned by the arc's angle about its axis (square to it)
                r * angle.cos() + axis.cross(r) * angle.sin()
            } else {
                r
            };
            unit(axis.cross(r))
        }
        Piece::Helix { .. } => None,
    }
}

/// Whether the closed path's last and first pieces meet G1 (unit tangents within
/// [`G1_TANGENTS`], the rule [`Path::corner_fractions`] decides joints by).
fn closes_smoothly(path: &Path) -> Result<bool> {
    let (Some(first), Some(last)) = (path.pieces.first(), path.pieces.last()) else {
        return Err(invalid("a path without pieces"));
    };
    let (Some(t_end), Some(t_start)) = (tangent(last, true), tangent(first, false)) else {
        return Ok(false);
    };
    let gap = (t_end - t_start).length();
    Ok(matches!(
        gap.partial_cmp(&G1_TANGENTS),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ))
}

/// The solid swept by the closed planar `profile` (its plane's normal `profile_normal`) along
/// `path` by `laws`, piece by piece, its pieces meeting at the path's corners in mitres
/// (module docs).
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a twist or rails, a helix, an arc with an end scale or
/// ending at a corner, a closed path with an end scale (tangency-labs/solidcore#3192) or
/// closing at a corner, a profile curve with no B-spline form; [`SolidError::InvalidInput`] for
/// an empty profile; those of [`stations`], of the sections, of the sweeps and skins, of the
/// face copies and caps and of the stitch; `SM_ERR` when the sewn body is no manifold solid.
pub fn cornered_sweep(
    k: &mut Kernel,
    profile: &[Curve],
    profile_normal: Vec3,
    path: &Path,
    laws: &Laws<'_>,
) -> Result<Brep> {
    if profile.is_empty() {
        return Err(invalid("an empty profile"));
    }
    if laws.twist != 0.0 || !laws.rails.is_empty() {
        return Err(unsupported(
            "a twist or guide rails: the section turns or deforms along a piece",
        ));
    }
    let scaled = laws.end_scale != 1.0;
    // each joint's stop: a corner's mitre, or the fraction where the pieces meet G1
    let joints = path.joints()?;
    let corners = path.corner_fractions()?;
    let mut stops = vec![Stop::At(0.0)];
    let mut mitre = vec![false];
    let mut corner = 0;
    for &u in &joints {
        if corners.get(corner) == Some(&u) {
            stops.push(Stop::Corner(corner));
            mitre.push(true);
            corner += 1;
        } else {
            stops.push(Stop::At(u));
            mitre.push(false);
        }
    }
    stops.push(Stop::At(1.0));
    mitre.push(false);
    for (j, piece) in path.pieces.iter().enumerate() {
        let at_corner =
            mitre.get(j).copied().unwrap_or(false) || mitre.get(j + 1).copied().unwrap_or(false);
        match piece {
            Piece::Arc { .. } if scaled => {
                return Err(unsupported(
                    "an arc with an end scale: no exact class (the procedural sweep's ledger path-scaled)",
                ));
            }
            Piece::Arc { .. } if at_corner => {
                return Err(unsupported(
                    "an arc ending at a corner: its sweep would have to end on the plane halving the corner",
                ));
            }
            Piece::Line { .. } | Piece::Arc { .. } => {}
            Piece::Helix { .. } => {
                return Err(unsupported(
                    "a helix: no exact class (the procedural sweep's ledger path-spline)",
                ));
            }
        }
    }
    let closed = path.is_closed()?;
    if closed && scaled {
        return Err(unsupported(
            "a closed path whose section changes round it (tangency-labs/solidcore#3192)",
        ));
    }
    if closed && !closes_smoothly(path)? {
        return Err(unsupported(
            "a closed path that closes at a corner: its closing mitre is not one of the path's corners",
        ));
    }
    let placed = stations(k, path, profile_normal, laws, &stops)?;
    let first = placed
        .first()
        .copied()
        .ok_or_else(|| invalid("no stations"))?;
    // one section per joint, handed to both pieces that meet there
    let mut sections: Vec<Vec<Curve>> = Vec::new();
    for (j, s) in placed.iter().enumerate() {
        if closed && j + 1 == placed.len() {
            let start = sections
                .first()
                .cloned()
                .ok_or_else(|| invalid("no sections"))?;
            sections.push(start);
            continue;
        }
        let rigid = !scaled && !mitre.get(j).copied().unwrap_or(false);
        sections.push(section(k, profile, s, &first, rigid)?);
    }
    let mut b = new_brep(
        k,
        crate::geom::tol::USE_DEFAULT,
        crate::geom::tol::USE_DEFAULT,
    )?;
    for (j, piece) in path.pieces.iter().enumerate() {
        let (Some(from), Some(to)) = (sections.get(j), sections.get(j + 1)) else {
            return Err(invalid("a piece without its sections"));
        };
        let mut swept = match *piece {
            Piece::Line { .. } => {
                let curves: Vec<Option<&Curve>> = from.iter().chain(to).map(Some).collect();
                create_skin_primitive(k, &[from.len(), to.len()], &curves, 1, false)?.brep
            }
            Piece::Arc {
                centre,
                axis,
                angle,
                ..
            } => {
                let curves: Vec<Option<&Curve>> = from.iter().map(Some).collect();
                create_rotational_sweep(
                    k,
                    &curves,
                    centre,
                    axis,
                    angle * 180.0 / std::f64::consts::PI,
                    false,
                )?
            }
            Piece::Helix { .. } => return Err(unsupported("a helix")),
        };
        copy_in(k, &mut swept, &mut b)?;
    }
    if !closed {
        for cap in [sections.first(), sections.last()] {
            let cap = cap.ok_or_else(|| invalid("no sections"))?;
            let (region, tol) = (b.infinite_region(), b.tolerance());
            build_faces(&mut b, |b, fenv| {
                create_planar_faces_with_3d_curves(k, b, region, cap, tol, fenv)
            })?;
        }
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    if !is_manifold_solid(&b)? {
        return Err(SolidError::sweep_failed(
            OP,
            "the cornered sweep is not a manifold solid",
        ));
    }
    Ok(b)
}
