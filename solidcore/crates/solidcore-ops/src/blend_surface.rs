//! Surfaces between two sheets: a blend sheet between an edge of one sheet and an edge of
//! another, each side held at its own continuity (G0 to G3), with the seam it makes on each side
//! measured ([`blend_edges`]); and a fillet where two sheets meet, round, chordal or
//! curvature-continuous, the sheets cut back to it on request ([`fillet_sheets`]).
//!
//! The blend between edges:
//!
//! The recipe, every number the kernel's:
//! - Each side is a cross-section read off its edge by the kernel's
//!   [`solidcore::sweep::section_blend::section_frames`]: at `stations` stations of even arc
//!   length, the edge's point and its sheet's shape read along the direction the blend leaves in
//!   (the part of it in the sheet's tangent plane). The blend leaves each sheet towards the other
//!   edge: along the chord from the middle of its own edge to the middle of the other's
//!   ([`Body::edge_middle`]), which the kernel takes into the sheet's tangent plane at each
//!   station; at the second side that is the direction back into the blend, as the kernel's
//!   frames of a last section are.
//! - The second side's stations run the way that pairs each end of the first edge with the
//!   nearer end of the second (the kernel's distances between the end stations), so the blend
//!   does not cross over itself.
//! - The sheet is the kernel's section blend through the two sections
//!   ([`solidcore::sweep::section_blend::section_blend_surface`]), holding each side's
//!   continuity at its stations, its face keeping the definition, as
//!   [`crate::sweep::railed::loft_blend`] makes it.
//! - Each seam is measured as an edge between two faces: copies of the two sheets and the blend
//!   are put in one body ([`solidcore::topology::brep::p3::merge_brep`]) and sewn at the
//!   caller's tolerance ([`crate::stitch::stitch`]); the seam on each side is the edge of the
//!   sewn body through the middle of that side's edge, measured by
//!   [`Body::continuity_report`] against the caller's tolerances. The blend returned is the
//!   blend alone; the sheets are unchanged.
//!
//! Ops chooses no tolerance, station count or crown: they are the caller's ([`BlendEdges`]).
//! What holds: the blend passes through every station of both edges and holds each side's
//! continuity there to rounding; between stations its boundary is the kernel's interpolation of
//! the stations, so the seam reports say what the seams are, measured, never assumed.

use solidcore::blend::fillet_executive::api::create_face_from_surface;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::query::api_queries::face_internal_point;
use solidcore::sweep::section_blend::{
    section_blend_surface, section_frames as kernel_frames, EndOrder, FrameOptions, SectionFrame,
};
use solidcore::topology::brep::p3::{
    brep_copy, merge_brep, new_brep, notify_serial, topology_ref, with_kernel_callbacks,
};
use solidcore::Vec3;

use crate::body::{finish, Body, Expect};
use crate::continuity::{ContinuityTolerances, SeamReport, MAX_STATIONS, MIN_STATIONS};
use crate::error::status;
use crate::order::{self, Order};
use crate::session::{run_on, with_body};
use crate::sweep::railed::Continuity;
use crate::{Error, Result};

/// One side of a blend: a sheet, its record order, the record edge the blend meets, and the
/// continuity it meets it with.
#[derive(Clone, Copy, Debug)]
pub struct BlendSide<'a> {
    /// The sheet.
    pub body: &'a Body,
    /// The sheet's record order (as [`Body::from_record`] gives it).
    pub order: &'a Order,
    /// The edge, by its index in the record.
    pub edge: usize,
    /// How the blend meets the sheet across the edge.
    pub continuity: Continuity,
}

/// How a blend between two edges is built and its seams measured: the caller's, ops has no
/// defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlendEdges {
    /// The stations along each edge the blend is built through, and its seams measured at
    /// ([`MIN_STATIONS`]..=[`MAX_STATIONS`]).
    pub stations: usize,
    /// The least share of the leave direction a sheet's tangent plane keeps for the blend to
    /// leave through it (`0..=1`; the kernel's [`FrameOptions::min_in_plane`]).
    pub min_in_plane: f64,
    /// How fast the blend leaves each side, first then second (`-0.9..=3`, 0 the plain blend).
    pub crown: [f64; 2],
    /// The tolerance the blend is sewn to the sheets at, to measure the seams (finite,
    /// positive).
    pub stitch_tolerance: f64,
    /// What counts as continuous across a seam.
    pub seam: ContinuityTolerances,
}

/// A one-face blend sheet from `from`'s edge to `to`'s edge, meeting each with its continuity,
/// and the seam it makes with each sheet, measured: `[from's, to's]` (each [`SeamReport::edge`]
/// is the seam's index in the sewn body the seams are measured on). The sheets may be the same
/// body (two edges of one sheet). Neither sheet changes.
///
/// # Errors
/// - [`Error::Argument`]: a station count outside [`MIN_STATIONS`]..=[`MAX_STATIONS`], an
///   in-plane share outside `0..=1`, a crown outside `-0.9..=3`, a stitch tolerance not finite
///   and positive, a seam tolerance negative or not finite, or an edge past its record's list;
/// - [`Error::Unsupported`]: the blend does not sew onto an edge of a sheet within the stitch
///   tolerance, so that seam cannot be measured;
/// - the kernel's refusals by their status: the two edges' middles at one point (no direction
///   to leave in), a sheet whose tangent plane keeps less than `min_in_plane` of it, a station
///   where the two edges meet (no room for a blend), a surface singular at a station;
/// - [`Error::Empty`]; those of the stitch and the continuity report.
pub fn blend_edges(
    from: BlendSide<'_>,
    to: BlendSide<'_>,
    how: &BlendEdges,
) -> Result<(Body, [SeamReport; 2])> {
    let ok_tolerance = |t: f64| t.is_finite() && t >= 0.0;
    let seam = how.seam;
    let valid = (MIN_STATIONS..=MAX_STATIONS).contains(&how.stations)
        && how.min_in_plane.is_finite()
        && (0.0..=1.0).contains(&how.min_in_plane)
        && how
            .crown
            .iter()
            .all(|c| c.is_finite() && (-0.9..=3.0).contains(c))
        && how.stitch_tolerance.is_finite()
        && how.stitch_tolerance > 0.0
        && [
            seam.gap_mm,
            seam.angle_rad,
            seam.curvature_per_mm,
            seam.curvature_change_per_mm2,
            seam.curvature_change_rate_per_mm3,
        ]
        .into_iter()
        .all(ok_tolerance);
    if !valid {
        return Err(Error::Argument);
    }
    let from_edge = order::edge(from.order, from.edge)?;
    let to_edge = order::edge(to.order, to.edge)?;
    let from_middle = from.body.edge_middle(from.order, from.edge)?;
    let to_middle = to.body.edge_middle(to.order, to.edge)?;
    // the chord between the middles, the kernel's vector difference
    let point = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let towards = |a: [f64; 3], b: [f64; 3]| point(b) - point(a);
    let opts = FrameOptions {
        stations: how.stations,
        min_in_plane: how.min_in_plane,
    };
    let orders = [end_order(from.continuity), end_order(to.continuity)];
    let crown = how.crown;
    let blend = Body::new(run_on(
        "blend_edges",
        &[("from", from.body), ("to", to.body)],
        |k| {
            let first = {
                let g = from.body.lock();
                kernel_frames(k, &g, &[from_edge], towards(from_middle, to_middle), &opts)
            };
            let second = {
                let g = to.body.lock();
                kernel_frames(k, &g, &[to_edge], towards(to_middle, from_middle), &opts)
            };
            let (first, mut second) = match (first, second) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => return Ok(Err(status(e))),
            };
            match crossed(&first, &second) {
                Ok(true) => second.reverse(),
                Ok(false) => {}
                Err(e) => return Ok(Err(e)),
            }
            let surface = match section_blend_surface(vec![first, second], orders, crown) {
                Ok(s) => s,
                Err(e) => return Ok(Err(status(e))),
            };
            let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
            if let Err(e) = create_face_from_surface(k, &mut b, &surface) {
                return Ok(Err(status(e)));
            }
            b.refresh_cached_topology();
            finish(k, b, Expect::FACES)
        },
    )??);
    let seams = seams(&blend, [from, to], [from_middle, to_middle], how)?;
    Ok((blend, seams))
}

/// The kernel's end order for a side's continuity.
fn end_order(c: Continuity) -> EndOrder {
    match c {
        Continuity::G0 => EndOrder::G0,
        Continuity::G1 => EndOrder::G1,
        Continuity::G2 => EndOrder::G2,
        Continuity::G3 => EndOrder::G3,
    }
}

/// Whether the second section's stations, as they run, pair the first's ends with the farther
/// ends: the first section's start is nearer the second's end than its start, and the first's
/// end nearer the second's start than its end ([`crate::measure::point_distance`], the
/// kernel's). Where the two ends disagree the stations are kept as they run.
///
/// # Errors
/// Those of the distances (a station not finite).
fn crossed(first: &[SectionFrame], second: &[SectionFrame]) -> Result<bool> {
    let (Some(a0), Some(a1), Some(b0), Some(b1)) =
        (first.first(), first.last(), second.first(), second.last())
    else {
        return Ok(false);
    };
    let at = |f: &SectionFrame| [f.p.x, f.p.y, f.p.z];
    let apart = |f: &SectionFrame, g: &SectionFrame| {
        crate::measure::point_distance(at(f), at(g)).map(|d| d.value)
    };
    Ok(apart(a0, b1)? < apart(a0, b0)? && apart(a1, b0)? < apart(a1, b1)?)
}

/// The seam `blend` makes with each side's sheet: the blend and copies of the sheets in one body,
/// sewn at the caller's tolerance, and the edge through each side's edge middle measured.
fn seams(
    blend: &Body,
    sides: [BlendSide<'_>; 2],
    middles: [[f64; 3]; 2],
    how: &BlendEdges,
) -> Result<[SeamReport; 2]> {
    let [from, to] = sides;
    let all = joined("blend_edges::seams", &[blend, from.body, to.body])?;
    let (sewn, _) = crate::stitch::stitch(all, how.stitch_tolerance)?;
    let order = native_order(&sewn);
    let mut out = Vec::with_capacity(2);
    for middle in middles {
        let Some((edge, _)) = nearest_edge(&sewn, &order, middle)? else {
            return Err(Error::Unsupported(
                "the blend does not sew onto the sheet's edge",
            ));
        };
        let report = match sewn.continuity_report(&order, &[edge], how.stations, how.seam) {
            Ok(r) => r,
            // the edge is no seam between two faces: the blend was not sewn onto it
            Err(Error::Argument) => {
                return Err(Error::Unsupported(
                    "the blend does not sew onto the sheet's edge within the stitch tolerance",
                ))
            }
            Err(e) => return Err(e),
        };
        let Some(seam) = report.seams.into_iter().next() else {
            return Err(Error::Unsupported(
                "no station of the seam between the blend and the sheet measured",
            ));
        };
        out.push(seam);
    }
    <[SeamReport; 2]>::try_from(out).map_err(|_| Error::Exception)
}

/// Copies of `bodies` in one body, each body once (a body named twice is copied once), unsewn:
/// the first copied, the others merged into it ([`merge_brep`]). The bodies are unchanged.
///
/// # Errors
/// [`Error::Argument`] for no body; [`Error::Empty`]; the kernel's.
fn joined(call: &'static str, bodies: &[&Body]) -> Result<Body> {
    let inputs: Vec<(&'static str, &Body)> = bodies.iter().map(|b| ("brep", *b)).collect();
    let made = run_on(call, &inputs, |k| {
        let Some((first, rest)) = bodies.split_first() else {
            return Ok(Err(Error::Argument));
        };
        let mut joined = {
            let g = first.lock();
            brep_copy(k, &g)?
        };
        let mut taken: Vec<&Body> = vec![*first];
        for body in rest {
            if taken.iter().any(|t| std::ptr::eq(*t, *body)) {
                continue;
            }
            taken.push(*body);
            let mut copy = {
                let g = body.lock();
                brep_copy(k, &g)?
            };
            let serial = notify_serial(&mut k.context, &mut copy)?;
            let source_key = move |e| topology_ref(serial, e);
            with_kernel_callbacks(k, &mut joined, |k, j, env| {
                merge_brep(k, env, j, &copy, &source_key, None, None, true)
            })?;
        }
        joined.refresh_cached_topology();
        finish(k, joined, Expect::FACES)
    })??;
    Ok(Body::new(made))
}

/// The record order of `body` that is its own lists (faces, edges and vertices as the kernel
/// holds them).
fn native_order(body: &Body) -> Order {
    let g = body.lock();
    Order {
        faces: g.face_list().to_vec(),
        edges: g.edge_list().to_vec(),
        vertices: g.vertex_list().to_vec(),
    }
}

/// The edge of `body` (by its index in `order`) nearest `p`, with how far it is (the kernel's
/// closest points); `None` for a body with no edge that answers.
///
/// # Errors
/// Those of the closest points.
fn nearest_edge(body: &Body, order: &Order, p: [f64; 3]) -> Result<Option<(usize, f64)>> {
    let mut nearest: Option<(usize, f64)> = None;
    for i in 0..order.edges.len() {
        if let Some(at) = body.edge_closest_point(order, i, p)? {
            if nearest.is_none_or(|(_, d)| at.distance < d) {
                nearest = Some((i, at.distance));
            }
        }
    }
    Ok(nearest)
}

// ---- fillet between two sheets -------------------------------------------------------------

/// The section of a fillet between two sheets and its size: the kernel's rounds
/// ([`crate::Blend`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Section {
    /// A rolling ball's round of this radius.
    Radius(f64),
    /// A circular round whose chord is this wide.
    Chord(f64),
    /// The curvature-continuous (G2) round that takes off the area of the ball of this radius.
    Curvature(f64),
}

/// How a fillet between two sheets is built and its seams measured: the caller's, ops has no
/// defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilletSheets {
    /// The section and its size.
    pub section: Section,
    /// Whether copies of the sheets cut back along the fillet's contact curves are made too.
    pub trim: bool,
    /// The tolerance the sheets are sewn at where they meet, and the reach within which a face
    /// is said to be on a sheet (finite, positive).
    pub tolerance: f64,
    /// The stations each seam is measured at ([`MIN_STATIONS`]..=[`MAX_STATIONS`]).
    pub stations: usize,
    /// What counts as continuous across a seam.
    pub seam: ContinuityTolerances,
}

/// A fillet between two sheets: the fillet sheet, the sheets cut back to it when asked, and the
/// seams between the fillet and the cut sheets, measured.
#[derive(Debug)]
pub struct Fillet {
    /// The fillet's faces, as one sheet.
    pub fillet: Body,
    /// With [`FilletSheets::trim`], copies of the two sheets (first, second) cut back along the
    /// fillet's contact curves.
    pub trimmed: Option<(Body, Body)>,
    /// Each seam between the fillet and a cut sheet (each [`SeamReport::edge`] its index in the
    /// filleted body the seams are measured on).
    pub seams: Vec<SeamReport>,
}

/// The fillet between sheets `a` and `b` where they meet along an edge of each: the two sheets'
/// copies sewn into one body at `how.tolerance`, the edges they then share rounded by the
/// kernel's blend ([`crate::blend`]) with `how.section` (on the side the sheets' normals give,
/// as the kernel's blend of a sheet's edge takes it). Each face of the rounded body is a sheet's
/// when a point inside it (the kernel's) lies within `how.tolerance` of that sheet, and the
/// fillet's otherwise (the blend makes the faces it trims anew, so their ids do not tell); the
/// fillet's faces are taken out as the fillet ([`crate::trim::remove_faces`]), and with
/// `how.trim` each sheet's as that sheet, cut back along the contact curves. The
/// seams between the fillet and the cut sheets are measured on the rounded body
/// ([`Body::continuity_report`]). Neither sheet changes.
///
/// # Errors
/// - [`Error::Argument`]: a size not finite, positive and at most [`crate::blend::MAX_SIZE`], a
///   tolerance not finite and positive, a station count out of range, a seam tolerance negative
///   or not finite, or the same body as both sheets;
/// - [`Error::Unsupported`]: the sheets do not meet along an edge of each (sheets that cross
///   each other or stand apart are not filleted: the kernel's rounds run along an edge two faces
///   share);
/// - [`Error::NoEffect`]: the blend made no face;
/// - the blend's refusals (a radius the faces have no room for: [`Error::NoRoom`] and its
///   kin), those of the stitch, the face removal and the continuity report.
pub fn fillet_sheets(a: &Body, b: &Body, how: &FilletSheets) -> Result<Fillet> {
    let (kind, size) = match how.section {
        Section::Radius(r) => (crate::Blend::Round, r),
        Section::Chord(c) => (crate::Blend::Chord, c),
        Section::Curvature(r) => (crate::Blend::Curvature, r),
    };
    let seam = how.seam;
    let valid = size.is_finite()
        && size > 0.0
        && size <= crate::blend::MAX_SIZE
        && how.tolerance.is_finite()
        && how.tolerance > 0.0
        && (MIN_STATIONS..=MAX_STATIONS).contains(&how.stations)
        && [
            seam.gap_mm,
            seam.angle_rad,
            seam.curvature_per_mm,
            seam.curvature_change_per_mm2,
            seam.curvature_change_rate_per_mm3,
        ]
        .into_iter()
        .all(|t| t.is_finite() && t >= 0.0)
        && !std::ptr::eq(a, b);
    if !valid {
        return Err(Error::Argument);
    }
    let (a_order, b_order) = (native_order(a), native_order(b));
    let (sewn, _) = crate::stitch::stitch(joined("fillet_sheets", &[a, b])?, how.tolerance)?;
    let order = native_order(&sewn);
    // the edges the sheets share once sewn: each runs along an edge of both
    let mut shared = Vec::new();
    for i in 0..order.edges.len() {
        let middle = sewn.edge_middle(&order, i)?;
        let on = |body: &Body, o: &Order| -> Result<bool> {
            Ok(nearest_edge(body, o, middle)?.is_some_and(|(_, d)| d <= how.tolerance))
        };
        if on(a, &a_order)? && on(b, &b_order)? {
            shared.push(i);
        }
    }
    if shared.is_empty() {
        return Err(Error::Unsupported(
            "the sheets do not meet along an edge of each: a fillet between sheets that cross or stand apart is not built",
        ));
    }
    let sizes = vec![size; shared.len()];
    crate::blend::blend(&sewn, kind, &shared, &sizes, &[], crate::Corners::Filled)?;
    let rounded = native_order(&sewn);
    // Each face of the rounded body is a sheet's (cut back) or the fillet's, by where a point
    // inside it lies: the kernel's blend makes the faces it trims anew, so a face's id does not
    // say which it is.
    let inside = inside_points(&sewn)?;
    let on = |sheet: &Body, order: &Order, p: [f64; 3]| -> Result<bool> {
        Ok(sheet
            .face_distances_within(order, &[p], how.tolerance)?
            .iter()
            .flatten()
            .any(|d| d.is_some_and(|d| d <= how.tolerance)))
    };
    let (mut of_a, mut of_b, mut made) = (Vec::new(), Vec::new(), Vec::new());
    for (i, p) in inside.iter().enumerate() {
        if on(a, &a_order, *p)? {
            of_a.push(i);
        } else if on(b, &b_order, *p)? {
            of_b.push(i);
        } else {
            made.push(i);
        }
    }
    if made.is_empty() {
        return Err(Error::NoEffect);
    }
    let without = |keep: &[usize]| -> Vec<usize> {
        (0..rounded.faces.len())
            .filter(|i| !keep.contains(i))
            .collect()
    };
    let fillet = crate::trim::remove_faces(&sewn, &rounded, &without(&made))?;
    let trimmed = if how.trim {
        Some((
            crate::trim::remove_faces(&sewn, &rounded, &without(&of_a))?,
            crate::trim::remove_faces(&sewn, &rounded, &without(&of_b))?,
        ))
    } else {
        None
    };
    // the seams: the fillet's edges that are edges between two faces of the rounded body
    let fillet_order = native_order(&fillet);
    let mut seams = Vec::new();
    for i in 0..fillet_order.edges.len() {
        let middle = fillet.edge_middle(&fillet_order, i)?;
        let Some((edge, d)) = nearest_edge(&sewn, &rounded, middle)? else {
            continue;
        };
        if d > how.tolerance {
            continue;
        }
        match sewn.continuity_report(&rounded, &[edge], how.stations, how.seam) {
            Ok(report) => seams.extend(report.seams),
            // an open end of the fillet: no seam there
            Err(Error::Argument) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Fillet {
        fillet,
        trimmed,
        seams,
    })
}

/// A point inside each face of `body`, in its own face order (the kernel's
/// `face_internal_point`).
///
/// # Errors
/// The kernel's.
fn inside_points(body: &Body) -> Result<Vec<[f64; 3]>> {
    with_body("fillet_sheets::inside_points", body, |k, b| {
        let faces = b.face_list().to_vec();
        let mut out = Vec::with_capacity(faces.len());
        for f in faces {
            match face_internal_point(k, b, f) {
                Ok(p) => out.push([p.x, p.y, p.z]),
                Err(e) => return Ok(Err(status(e))),
            }
        }
        Ok(Ok(out))
    })?
}
