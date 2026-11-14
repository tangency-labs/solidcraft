//! A sheet's control net: read, its control points moved, and the surface rebuilt to new degrees
//! and spans with the kernel's certified bound of how far it moved. The sheet is a record of one
//! untrimmed face on a B-spline surface (its face the whole of the surface's natural box, as
//! `surface::boundary` and `surface::ruled` make it), or one the kernel makes such a sheet
//! ([`editable`]: a plane, a cylinder, a cone, a sphere, a torus); the geometry is the kernel's,
//! and so is which control points are one point and move together (a closed surface's seam, a
//! pole). Ops checks the record and converts.

use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Boundary;
use solidcore::nurbs::crv_fit::p1::approx_knots;
use solidcore::nurbs::geom::Rectangle;
use solidcore::nurbs::knots::{clamped_uniform, KnotVector, Side};
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::{
    falloff_weight as kernel_falloff, Direction, FalloffShape, Surface as NurbSurface,
};
use solidcore::nurbs::srf_deviation::certified_distance;
use solidcore::nurbs::srf_match::{match_boundary, match_plane as kernel_match_plane};
pub use solidcore::nurbs::srf_match::{MatchOrder, MatchRows, SeamBounds};
use solidcore::SolidError;
use solidcore::Vec3;
use solidcore_record::brep::{
    knot_domain, Brep, Curve2, Face, Loop, Natural, OrientedEdge, Surface, MAX_SPLINE_DEGREE,
};

use std::borrow::Cow;

use solidcore::topology::brep::sheet::editable_sheet;
use solidcore::topology::face::p1::calculate_uv_domain_from_uv_trim_curves;

use crate::body::Body;
use crate::error::status;
use crate::session::with_body;
use crate::session::{read_body, run};
use crate::surface::{record, sheet_starting};
use crate::sweep::coord_ok;
use crate::{Error, Result};
use solidcore::surface::side::BoxSide;

/// The most sample points a rebuild fits (rows times columns).
pub const REBUILD_MAX_SAMPLES: usize = 1 << 20;

/// A sheet's control net: `rows` points along u by `cols` along v, `points[i * cols + j]` the
/// point of the i-th u index and the j-th v index.
#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    /// Control points along u.
    pub rows: usize,
    /// Control points along v.
    pub cols: usize,
    /// The points, row by row (u index major), Euclidean.
    pub points: Vec<[f64; 3]>,
    /// Each point's weight, in the order of `points`; empty for a non-rational surface. Edits
    /// keep them.
    pub weights: Vec<f64>,
    /// The points that are one point and move together, `(u index, v index)` (the kernel's
    /// `net_ties`): a closed surface's seam twins, a pole's row. An edit of any member moves
    /// all of its group.
    pub ties: Vec<Vec<(usize, usize)>>,
}

/// `sheet` ready for a control-net edit: a one-face sheet on a spline surface as it is,
/// borrowed (so a spline sheet's edits are exactly what they were); any other one-face sheet as
/// the kernel makes it a sheet with a net (`editable_sheet`, which decides by the surface's
/// kind: a plane as the Coons spline of its edges, an analytic face as its exact NURBS form, its
/// whole box the face). A record of other than one face is borrowed, for the edits to refuse.
///
/// # Errors
/// [`Error::Record`] for a record the kernel refuses; the kernel's refusal of the rebuild
/// ([`Error::Unsupported`]: holes, other than three or four edges, a folding outline, a face that
/// is not its surface's whole box, a kind with no exact NURBS form) by its status.
pub(crate) fn editable(sheet: &Brep) -> Result<Cow<'_, Brep>> {
    let [face] = sheet.faces.as_slice() else {
        return Ok(Cow::Borrowed(sheet));
    };
    let spline = matches!(
        face.surface,
        Surface::Spline { .. } | Surface::RationalSpline { .. }
    );
    if spline && face.nurbs.is_none() {
        return Ok(Cow::Borrowed(sheet));
    }
    let (body, _) = Body::from_record(sheet, true)?;
    let rebuilt = read_body("surface_cvs::editable", &body, |k, b| {
        let Some(&f) = b.face_list().first() else {
            return Ok(Err(Error::Argument));
        };
        Ok(editable_sheet(k, b, f).map_err(status))
    })??;
    match rebuilt {
        Some(b) => Ok(Cow::Owned(Body::new(b).to_record()?.0)),
        None => Ok(Cow::Borrowed(sheet)),
    }
}

/// The side of `sheet`'s box its one face's loop starts on (the kernel's `first_side`), so an
/// edit makes its sheet again listing the edges in its order: an edit changes no topology, and a
/// client names an edge by its index in the record. `None` for a record of other than one face.
///
/// # Errors
/// [`Error::Record`] for a record the kernel refuses; the kernel's error.
fn first_side(sheet: &Brep) -> Result<Option<BoxSide>> {
    if sheet.faces.len() != 1 {
        return Ok(None);
    }
    let (body, _) = Body::from_record(sheet, true)?;
    with_body("surface_cvs::first_side", &body, |k, b| {
        let Some(&f) = b.face_list().first() else {
            return Ok(None);
        };
        solidcore::topology::brep::sheet::first_side(k, b, f)
    })
}

/// The control net of `sheet`'s surface (a planar or analytic sheet's after [`editable`]): its
/// Euclidean points, their weights and the points tied together.
///
/// # Errors
/// [`Error::Argument`] for a record that is not one face on a B-spline surface, or whose net
/// is not rectangular; [`Error::Unsupported`] for a trimmed face (see [`untrimmed`]); the
/// kernel's status for a record tolerance it refuses.
pub fn control_net(sheet: &Brep) -> Result<Net> {
    let sheet = editable(sheet)?;
    let ((u, v), net) = untrimmed(&sheet)?;
    net_of(&kernel_surface(u, v, net)?, sheet.tolerance)
}

/// The [`Net`] of the kernel's surface `surface`, its ties within `tol` (the record's).
fn net_of(surface: &NurbSurface, tol: f64) -> Result<Net> {
    let s = BSplineSurface::from_gw_nurb(surface);
    let net = s.control_point_net().map_err(status)?;
    let ties = s.net_ties(tol).map_err(status)?;
    if net.u_count == 0 || net.v_count == 0 {
        return Err(Error::Argument);
    }
    Ok(Net {
        rows: net.u_count,
        cols: net.v_count,
        points: net.points.iter().map(|p| [p.x, p.y, p.z]).collect(),
        weights: net.weights,
        ties,
    })
}

/// The points tied to `p` in `ties` (its group), or `p` alone.
fn tied(ties: &[Vec<(usize, usize)>], p: (usize, usize)) -> Vec<(usize, usize)> {
    ties.iter()
        .find(|g| g.contains(&p))
        .cloned()
        .unwrap_or_else(|| vec![p])
}

/// `sheet` with the control points `moves` names moved: `((i, j), to)` puts the point of the
/// i-th u index and j-th v index at `to`, its weight kept, and every point tied to it
/// ([`Net::ties`]) there too, so a closed surface stays closed and a pole a point (a later move
/// of one point wins). The knots, degrees and weights are kept; the result is a new one-face
/// sheet on the moved surface, its whole box.
///
/// # Errors
/// As [`control_net`]; [`Error::Argument`] for an index past the net or a point not finite or
/// past the coordinate bound; the kernel's status otherwise.
pub fn move_cvs(sheet: &Brep, moves: &[((usize, usize), [f64; 3])]) -> Result<Brep> {
    let sheet = editable(sheet)?;
    let first = first_side(&sheet)?;
    let ((u, v), net) = untrimmed(&sheet)?;
    let mut surface = kernel_surface(u, v, net)?;
    let ties = net_of(&surface, sheet.tolerance)?.ties;
    for &((i, j), to) in moves {
        if !coord_ok(&to) {
            return Err(Error::Argument);
        }
        for (a, b) in tied(&ties, (i, j)) {
            let c = surface.cpt_mut(a, b).map_err(|_| Error::Argument)?;
            *c = CPoint::weighted(Vec3::new(to[0], to[1], to[2]), c.w);
        }
    }
    record(run("surface_cvs::move_cvs", |k| {
        sheet_starting(k, BSplineSurface::from_gw_nurb(&surface), first)
    }))
}

/// Face `face` of the body record `body` as a one-face sheet with a control net, as
/// [`editable`] makes a sheet of a one-face record: an analytic or planar face through the
/// kernel's `editable_sheet` (its exact NURBS form over its whole box, a plane's Coons spline),
/// a face on a B-spline surface as the sheet on that surface's whole box (refused, as
/// [`untrimmed`] refuses, where the face is not that box).
///
/// # Errors
/// [`Error::Argument`] for a face past the record's list; [`Error::Record`] for a record the
/// kernel refuses; [`Error::Unsupported`] for a face that is not its surface's whole box or has
/// no exact NURBS form; the kernel's status.
fn face_sheet(body: &Brep, face: usize) -> Result<Brep> {
    let Some(rf) = body.faces.get(face) else {
        return Err(Error::Argument);
    };
    let (kb, order) = Body::from_record(body, true)?;
    let f = crate::order::face(&order, face)?;
    let sheet = read_body("surface_cvs::face_sheet", &kb, |k, b| {
        Ok(editable_sheet(k, b, f).map_err(status))
    })??;
    if let Some(s) = sheet {
        return Ok(Body::new(s).to_record()?.0);
    }
    // a face on a B-spline surface: the sheet on its surface, when the face is its whole box
    let alone = Brep {
        faces: vec![rf.clone()],
        ..body.clone()
    };
    let ((u, v), net) = untrimmed(&alone)?;
    let surface = kernel_surface(u, v, net)?;
    record(run("surface_cvs::face_sheet", |k| {
        sheet_starting(k, BSplineSurface::from_gw_nurb(&surface), None)
    }))
}

/// The control net of face `face` of the body record `body`, as a sheet of it has it
/// ([`face_sheet`]): the net [`move_face_cvs`] moves, its ties included.
///
/// # Errors
/// As [`face_sheet`] and [`control_net`].
pub fn face_net(body: &Brep, face: usize) -> Result<Net> {
    control_net(&face_sheet(body, face)?)
}

/// The side of the parameter box `domain` a pcurve lies on: its control points all on
/// `u = umin` (left), `u = umax` (right), `v = vmin` (bottom) or `v = vmax` (top); `None` for a
/// pcurve off the sides (or a rational one).
fn side_in(domain: [[f64; 2]; 2], pcurve: &Curve2) -> Option<Boundary> {
    let Curve2::Spline { points: uv, .. } = pcurve else {
        return None;
    };
    let [[u0, u1], [v0, v1]] = domain;
    let on = |k: usize, at: f64| uv.iter().all(|p| p.get(k) == Some(&at));
    if on(0, u0) {
        Some(Boundary::Left)
    } else if on(0, u1) {
        Some(Boundary::Right)
    } else if on(1, v0) {
        Some(Boundary::Bottom)
    } else if on(1, v1) {
        Some(Boundary::Top)
    } else {
        None
    }
}

/// The one edge loop of `face` with each use's side of the face's box ([`side_in`]).
///
/// # Errors
/// [`Error::Unsupported`] for a face with holes or a use off its box's sides.
fn sided_loop(face: &Face) -> Result<Vec<(&OrientedEdge, Boundary)>> {
    let [Loop::Edges { edges }] = face.loops.as_slice() else {
        return Err(Error::Unsupported("a face with holes"));
    };
    edges
        .iter()
        .map(|e| {
            side_in(face.domain, &e.pcurve)
                .map(|s| (e, s))
                .ok_or(Error::Unsupported("a trimmed face"))
        })
        .collect()
}

/// Whether control point `(i, j)` of a `rows` by `cols` net lies on the box side `side`'s
/// boundary row (the row its side's edge is the curve of).
fn on_side(side: Boundary, (i, j): (usize, usize), rows: usize, cols: usize) -> bool {
    match side {
        Boundary::Left => i == 0,
        Boundary::Right => i + 1 == rows,
        Boundary::Bottom => j == 0,
        Boundary::Top => j + 1 == cols,
    }
}

/// The body record `body` with face `face`'s control points `moves` moved as [`move_cvs`] moves
/// a sheet's (`((i, j), to)`, the points tied to each moving with it): the face's surface the
/// moved one, every edge on a side whose boundary row moved the moved surface's edge there, and
/// each other face using such an edge given its pcurves again on its own surface. The record's
/// faces, edges and vertices stay where they are and each face's loops keep their uses, so a
/// client's face and edge indices stay valid. The kernel makes the moved surface, its edges and
/// every pcurve; ops only puts them into the record.
///
/// A neighbour of a moved edge is kept on its surface: a planar one takes the edge while it stays
/// in its plane (within the body's tolerance); one on a curved surface is not rebuilt yet.
///
/// # Errors
/// As [`face_sheet`] and [`move_cvs`]; [`Error::Unsupported`] for a moved edge that leaves the
/// plane of a face it bounds, a moved edge on a face that is not planar, or a face with holes;
/// [`Error::Record`] for a result the kernel refuses; the kernel's status.
pub fn move_face_cvs(
    body: &Brep,
    face: usize,
    moves: &[((usize, usize), [f64; 3])],
) -> Result<Brep> {
    let sheet = face_sheet(body, face)?;
    let net = control_net(&sheet)?;
    let moved = move_cvs(&sheet, moves)?;
    let mut picked: Vec<(usize, usize)> = Vec::new();
    for &(p, _) in moves {
        picked.extend(tied(&net.ties, p));
    }
    splice(body, face, &moved, |side| {
        picked.iter().any(|&p| on_side(side, p, net.rows, net.cols))
    })
}

/// The body record `body` with face `face`'s picked control points moved by `delta` (and with a
/// `falloff` the points around them by their weight of it), as [`move_net`] moves a sheet's, and
/// the edges and faces about it rebuilt as [`move_face_cvs`] rebuilds them: an edge moves where a
/// point of its boundary row moves by any weight.
///
/// # Errors
/// As [`move_face_cvs`] and [`move_net`].
pub fn move_face_net(
    body: &Brep,
    face: usize,
    picks: &Picks,
    delta: [f64; 3],
    falloff: Option<Falloff>,
) -> Result<Brep> {
    let sheet = face_sheet(body, face)?;
    let net = control_net(&sheet)?;
    let weights = net_weights(net.rows, net.cols, picks, falloff)?;
    let moved = move_net(&sheet, picks, delta, falloff)?;
    let mut picked: Vec<(usize, usize)> = Vec::new();
    for i in 0..net.rows {
        for j in 0..net.cols {
            let w = i
                .checked_mul(net.cols)
                .and_then(|r| r.checked_add(j))
                .and_then(|n| weights.get(n));
            if w.is_some_and(|&w| w > 0.0) {
                picked.extend(tied(&net.ties, (i, j)));
            }
        }
    }
    splice(body, face, &moved, |side| {
        picked.iter().any(|&p| on_side(side, p, net.rows, net.cols))
    })
}

/// [`move_face_cvs`]'s record: face `face` of `body` on the surface of the one-face sheet
/// `moved` (its sheet, its edges on the same sides of the box), the edges on the sides
/// `changed` says moved taken from `moved`, the faces they bound given their pcurves again.
fn splice(
    body: &Brep,
    face: usize,
    moved: &Brep,
    changed: impl Fn(Boundary) -> bool,
) -> Result<Brep> {
    let tol = body.tolerance;
    let [new_face] = moved.faces.as_slice() else {
        return Err(Error::Argument);
    };
    let old_face = body.faces.get(face).ok_or(Error::Argument)?;
    let old_loop = sided_loop(old_face)?;
    let new_loop = sided_loop(new_face)?;
    let mut out = body.clone();
    // the moved edges, by their record index, with whether their direction turns over
    let mut moved_edges: Vec<(usize, bool)> = Vec::new();
    for (old_use, side) in &old_loop {
        if !changed(*side) {
            continue;
        }
        let (new_use, _) = new_loop
            .iter()
            .find(|(_, s)| s == side)
            .ok_or(Error::Unsupported(
                "the moved face's sides differ from the face's",
            ))?;
        let e = usize::try_from(old_use.edge).map_err(|_| Error::Argument)?;
        if moved_edges.iter().any(|&(m, _)| m == e) {
            continue;
        }
        let ne = usize::try_from(new_use.edge).map_err(|_| Error::Argument)?;
        let src = moved.edges.get(ne).ok_or(Error::Argument)?;
        // the loop runs round the face the same way on both surfaces, so the edge turns over
        // where the two uses on one side run opposite ways
        let turned = new_use.forward != old_use.forward;
        let dst = out.edges.get_mut(e).ok_or(Error::Argument)?;
        dst.curve = src.curve.clone();
        dst.nurbs.clone_from(&src.nurbs);
        dst.interval = src.interval;
        dst.tolerance = dst.tolerance.max(src.tolerance);
        if turned {
            dst.vertices.swap(0, 1);
        }
        // the vertices where the moved edge has them
        let [a, b] = dst.vertices;
        let [sa, sb] = src.vertices;
        for (v, sv) in [(a, sa), (b, sb)] {
            let point = moved
                .vertices
                .get(usize::try_from(sv).map_err(|_| Error::Argument)?)
                .ok_or(Error::Argument)?
                .point;
            if let Some(x) = usize::try_from(v)
                .ok()
                .and_then(|v| out.vertices.get_mut(v))
            {
                x.point = point;
            }
        }
        moved_edges.push((e, turned));
    }
    // every use of a turned edge runs the other way along it
    for &(e, turned) in &moved_edges {
        if !turned {
            continue;
        }
        let refs = out.edges.get(e).map(|x| x.uses.clone()).unwrap_or_default();
        for r in refs {
            if let Some(u) = use_mut(&mut out, r.face, r.r#loop, r.position) {
                u.forward = !u.forward;
            }
        }
        if let Some(x) = out.edges.get_mut(e) {
            for r in &mut x.uses {
                r.upper = !r.upper;
            }
        }
    }
    // the edited face on the moved surface: the use of each moved edge the moved sheet's pcurve
    // on its side (exact: the edge is the sheet's, and runs the same way now), every other made
    // again on the surface (a kept edge has its own parameter, not the sheet's)
    let mut exact = Vec::new();
    {
        let f = out.faces.get_mut(face).ok_or(Error::Argument)?;
        f.surface = new_face.surface.clone();
        f.nurbs.clone_from(&new_face.nurbs);
        f.domain = new_face.domain;
        if let Some(Loop::Edges { edges }) = f.loops.first_mut() {
            for (p, (u, (_, side))) in edges.iter_mut().zip(&old_loop).enumerate() {
                let moved_here = moved_edges
                    .iter()
                    .any(|&(e, _)| usize::try_from(u.edge).is_ok_and(|x| x == e));
                let same = new_loop
                    .iter()
                    .find(|(n, s)| moved_here && s == side && n.forward == u.forward);
                if let Some((n, _)) = same {
                    u.pcurve = n.pcurve.clone();
                    exact.push(p);
                }
            }
        }
    }
    let mut refit = vec![face];
    for &(e, _) in &moved_edges {
        for r in out.edges.get(e).map(|x| x.uses.clone()).unwrap_or_default() {
            let n = usize::try_from(r.face).map_err(|_| Error::Argument)?;
            if !refit.contains(&n) {
                let planar = out.faces.get(n).is_some_and(|f| {
                    matches!(
                        f.surface,
                        Surface::Plane { .. } | Surface::PlaneThrough { .. }
                    ) && f.nurbs.is_none()
                });
                if !planar {
                    return Err(Error::Unsupported(
                        "a moved edge bounds a face that is not planar, which is not rebuilt yet",
                    ));
                }
                refit.push(n);
            }
        }
    }
    let moved_set: Vec<usize> = moved_edges.iter().map(|&(e, _)| e).collect();
    for &n in &refit {
        let kept = if n == face { exact.as_slice() } else { &[] };
        let pcurves = face_pcurves(&out, n, &moved_set, tol, n == face, kept)?;
        let f = out.faces.get_mut(n).ok_or(Error::Argument)?;
        for (l, ps) in f.loops.iter_mut().zip(pcurves) {
            if let Loop::Edges { edges } = l {
                for (u, p) in edges.iter_mut().zip(ps) {
                    u.pcurve = p;
                }
            }
        }
    }
    // each rebuilt neighbour's box around its new loops, the kernel's
    let neighbours: Vec<usize> = refit.iter().copied().filter(|&n| n != face).collect();
    if !neighbours.is_empty() {
        let (kb, order) = Body::from_record(&out, true)?;
        for n in neighbours {
            let f = crate::order::face(&order, n)?;
            let d = with_body("surface_cvs::move_face_cvs", &kb, |k, b| {
                calculate_uv_domain_from_uv_trim_curves(k, b, f, false, false, false).map(|o| {
                    let (lo, hi) = (o.domain.min(), o.domain.max());
                    Ok::<_, Error>([[lo.x, hi.x], [lo.y, hi.y]])
                })
            })??;
            if let Some(face) = out.faces.get_mut(n) {
                face.domain = d;
            }
        }
    }
    // the result as the kernel takes it
    Body::from_record(&out, false)?;
    Ok(out)
}

/// The use at `(face, loop, position)` of `b`.
fn use_mut(b: &mut Brep, face: u32, l: u32, position: u32) -> Option<&mut OrientedEdge> {
    let f = b.faces.get_mut(usize::try_from(face).ok()?)?;
    let Loop::Edges { edges } = f.loops.get_mut(usize::try_from(l).ok()?)? else {
        return None;
    };
    edges.get_mut(usize::try_from(position).ok()?)
}

/// The pcurves of face `n`'s loops on its surface, the kernel's ([`crate::pcurve`]): a use of
/// an edge not in `moved` keeps its given pcurve; on the edited face (`edited`), whose surface
/// changed, only the uses of its first loop at the positions `kept` (the moved sheet's own) do,
/// the rest made again. A moved edge that is not on the surface within `tol` is refused as
/// leaving the face's plane.
fn face_pcurves(
    b: &Brep,
    n: usize,
    moved: &[usize],
    tol: f64,
    edited: bool,
    kept: &[usize],
) -> Result<Vec<Vec<Curve2>>> {
    let f = b.faces.get(n).ok_or(Error::Argument)?;
    let mut loops = Vec::new();
    for (li, l) in f.loops.iter().enumerate() {
        let Loop::Edges { edges } = l else {
            loops.push(Vec::new());
            continue;
        };
        let mut uses = Vec::new();
        for (p, u) in edges.iter().enumerate() {
            let i = usize::try_from(u.edge).map_err(|_| Error::Argument)?;
            let e = b.edges.get(i).ok_or(Error::Argument)?;
            uses.push(crate::pcurve::EdgeUse {
                edge: u.edge,
                curve: e.nurbs.as_ref().unwrap_or(&e.curve),
                interval: e.interval,
                forward: u.forward,
                tolerance: e.tolerance,
                given: if edited {
                    (li == 0 && kept.contains(&p)).then_some(&u.pcurve)
                } else {
                    (!moved.contains(&i)).then_some(&u.pcurve)
                },
            });
        }
        loops.push(uses);
    }
    let surface = f.nurbs.as_ref().unwrap_or(&f.surface);
    let domain = edited.then_some(f.domain);
    crate::pcurve::pcurves_within(surface, &loops, tol, domain).map_err(|e| match e {
        crate::pcurve::Error::Failed(_) if !edited => {
            Error::Unsupported("a moved edge leaves the plane of a face it bounds")
        }
        _ => Error::Unsupported("the moved face's edges have no pcurves on it"),
    })
}

/// What [`move_net`] moves: whole rows (u indices), whole columns (v indices) or single control
/// points `(i, j)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picks {
    /// Every point of these u indices.
    Rows(Vec<usize>),
    /// Every point of these v indices.
    Columns(Vec<usize>),
    /// These points, `(u index, v index)`.
    Cvs(Vec<(usize, usize)>),
}

/// The shape of a soft selection's falloff over the index distance `d` from the picks, with
/// `s = d / (radius + 1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `1 - s`.
    Linear,
    /// `1 - (3 s² - 2 s³)`: flat at the picks and at the radius's edge.
    Smooth,
}

/// A soft selection: the points within `radius` index steps of the picks move too, by the
/// weight [`Shape`] gives their distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Falloff {
    /// The most index steps from a pick a point moves at: 0 moves the picks alone.
    pub radius: u32,
    /// How the weight falls from 1 at the picks.
    pub shape: Shape,
}

/// The weight of a point `d` index steps from the nearest pick: 1 at a pick, falling by
/// `falloff`'s shape, 0 past its radius (and for every point off the picks without a falloff).
#[must_use]
pub fn falloff_weight(falloff: Option<Falloff>, d: usize) -> f64 {
    let Some(f) = falloff else {
        return if d == 0 { 1.0 } else { 0.0 };
    };
    let Ok(d) = u32::try_from(d) else {
        return 0.0;
    };
    let shape = match f.shape {
        Shape::Linear => FalloffShape::Linear,
        Shape::Smooth => FalloffShape::Smooth,
    };
    kernel_falloff(shape, f.radius, d)
}

/// The weight table of a `rows` by `cols` net (row by row, as [`Net::points`]): each point's
/// [`falloff_weight`] at its index distance from the nearest pick. A row pick's distance is
/// along u (`|i - row|`), a column pick's along v (`|j - column|`), a point pick's the larger of
/// the two (the square ring of the net around it).
///
/// # Errors
/// [`Error::Argument`] for no picks, a pick past the net, or a net of no points or more than
/// [`REBUILD_MAX_SAMPLES`].
pub fn net_weights(
    rows: usize,
    cols: usize,
    picks: &Picks,
    falloff: Option<Falloff>,
) -> Result<Vec<f64>> {
    let total = rows
        .checked_mul(cols)
        .filter(|&n| n > 0 && n <= REBUILD_MAX_SAMPLES)
        .ok_or(Error::Argument)?;
    let (empty, inside) = match picks {
        Picks::Rows(p) => (p.is_empty(), p.iter().all(|&i| i < rows)),
        Picks::Columns(p) => (p.is_empty(), p.iter().all(|&j| j < cols)),
        Picks::Cvs(p) => (p.is_empty(), p.iter().all(|&(i, j)| i < rows && j < cols)),
    };
    if empty || !inside {
        return Err(Error::Argument);
    }
    let nearest = |i: usize, j: usize| -> usize {
        let d = match picks {
            Picks::Rows(p) => p.iter().map(|&r| i.abs_diff(r)).min(),
            Picks::Columns(p) => p.iter().map(|&c| j.abs_diff(c)).min(),
            Picks::Cvs(p) => p
                .iter()
                .map(|&(a, b)| i.abs_diff(a).max(j.abs_diff(b)))
                .min(),
        };
        d.unwrap_or(usize::MAX)
    };
    let mut table = Vec::new();
    table
        .try_reserve_exact(total)
        .map_err(|_| Error::Argument)?;
    for i in 0..rows {
        for j in 0..cols {
            table.push(falloff_weight(falloff, nearest(i, j)));
        }
    }
    Ok(table)
}

/// `sheet` with the picked control points moved by `delta`, and with a `falloff` the points
/// around them by their weight of it ([`net_weights`]): the kernel moves each point of weight
/// `t` by `t delta` (`Surface::displace_cpts`), the points tied together ([`Net::ties`]) each by
/// the largest weight among them, so a closed surface stays closed and a pole a point. The
/// knots, degrees and point weights are kept; the result is a new one-face sheet on the moved
/// surface, its whole box.
///
/// # Errors
/// As [`control_net`] and [`net_weights`]; [`Error::Argument`] for a `delta` not finite or
/// past the coordinate bound, or a moved point past it; the kernel's status otherwise.
pub fn move_net(
    sheet: &Brep,
    picks: &Picks,
    delta: [f64; 3],
    falloff: Option<Falloff>,
) -> Result<Brep> {
    if !coord_ok(&delta) {
        return Err(Error::Argument);
    }
    let sheet = editable(sheet)?;
    let first = first_side(&sheet)?;
    let ((u, v), record_net) = untrimmed(&sheet)?;
    let mut surface = kernel_surface(u, v, record_net)?;
    let net = net_of(&surface, sheet.tolerance)?;
    let mut weights = net_weights(net.rows, net.cols, picks, falloff)?;
    let place = |(i, j): (usize, usize)| i.checked_mul(net.cols).and_then(|r| r.checked_add(j));
    for group in &net.ties {
        let most = group
            .iter()
            .filter_map(|&p| place(p).and_then(|n| weights.get(n)))
            .copied()
            .fold(0.0, f64::max);
        for &p in group {
            if let Some(t) = place(p).and_then(|n| weights.get_mut(n)) {
                *t = most;
            }
        }
    }
    let moves: Vec<((usize, usize), f64)> = (0..net.rows)
        .flat_map(|i| (0..net.cols).map(move |j| (i, j)))
        .zip(weights)
        .filter(|&(_, t)| t > 0.0)
        .collect();
    let moved = record(run("surface_cvs::move_net", |k| {
        surface.displace_cpts(&moves, Vec3::new(delta[0], delta[1], delta[2]))?;
        sheet_starting(k, BSplineSurface::from_gw_nurb(&surface), first)
    }))?;
    if !control_net(&moved)?
        .points
        .iter()
        .all(|p| coord_ok(p.as_slice()))
    {
        return Err(Error::Argument);
    }
    Ok(moved)
}

/// `sheet`'s surface refitted to degrees `(deg_u, deg_v)` with `(spans_u, spans_v)` uniform
/// spans on `[0, 1]²`, and the kernel's certified bound of how far the refit is from the
/// surface at every parameter ([`certified_distance`]), kept when within `tol`. The surface,
/// its box mapped onto `[0, 1]²`, is sampled at even steps of each parameter, four per
/// control point each way (at most [`REBUILD_MAX_SAMPLES`] in all), and fitted by least
/// squares through its boundary rows: each column of samples along u at those parameters,
/// then each row of the result along v ([`approx_knots`], The NURBS Book §9.4.3).
///
/// A rational surface (an analytic face's exact form) is not rebuilt: the kernel's certified
/// bound takes polynomial surfaces only, so a refit could not say how far it moved.
///
/// # Errors
/// As [`control_net`]; [`Error::Unsupported`] for a rational surface; [`Error::Argument`] for
/// a surface on knots not clamped (a periodic one), a degree of 0 or past the record's most, no
/// spans, fewer than three or more than a record's control points either way, more samples
/// than [`REBUILD_MAX_SAMPLES`], or a `tol` not finite or negative; [`Error::PastTolerance`]
/// with the bound when it is past `tol`; the kernel's status otherwise.
pub fn rebuild(
    sheet: &Brep,
    deg_u: usize,
    deg_v: usize,
    spans_u: usize,
    spans_v: usize,
    tol: f64,
) -> Result<(Brep, f64)> {
    let sheet = editable(sheet)?;
    let first = first_side(&sheet)?;
    let ((u, v), points) = untrimmed(&sheet)?;
    if points.is_rational() {
        return Err(Error::Unsupported(
            "a rational surface: no certified bound for its refit",
        ));
    }
    let degrees_ok = |d: usize| (1..=MAX_SPLINE_DEGREE).contains(&d);
    if !(degrees_ok(deg_u) && degrees_ok(deg_v) && tol.is_finite() && tol >= 0.0) {
        return Err(Error::Argument);
    }
    let count = |spans: usize, degree: usize| {
        spans
            .checked_add(degree)
            .filter(|c| (3..=4096).contains(c))
            .ok_or(Error::Argument)
    };
    let (nu, nv) = (count(spans_u, deg_u)?, count(spans_v, deg_v)?);
    let samples = |c: usize| c.checked_mul(4).and_then(|s| s.checked_add(1));
    let (su, sv) = (
        samples(nu).ok_or(Error::Argument)?,
        samples(nv).ok_or(Error::Argument)?,
    );
    if su.checked_mul(sv).is_none_or(|n| n > REBUILD_MAX_SAMPLES) {
        return Err(Error::Argument);
    }
    // even steps on [0, 1]: the distinct knots of a polyline of that many points
    let steps = |n: usize| -> Result<Vec<f64>> {
        let knots = clamped_uniform(n, 1).map_err(|_| Error::Argument)?;
        Ok(knots.get(1..=n).ok_or(Error::Argument)?.to_vec())
    };
    let (pu, pv) = (steps(su)?, steps(sv)?);
    let uk = clamped_uniform(nu, deg_u).map_err(|_| Error::Argument)?;
    let vk = clamped_uniform(nv, deg_v).map_err(|_| Error::Argument)?;
    let original = kernel_surface(u, v, points)?;
    // A surface on unclamped knots is not sampled: the kernel's surface evaluation keeps
    // upstream's clamped reading of a domain's ends (N_BasisEval), and the certified bound takes
    // clamped knots only.
    if !(clamped(u, original.p()) && clamped(v, original.q())) {
        return Err(Error::Argument);
    }
    let fitted = run("surface_cvs::rebuild", |_| {
        let mut unit = original.clone();
        let square = Rectangle {
            ul: 0.0,
            ur: 1.0,
            vb: 0.0,
            vt: 1.0,
        };
        unit.reparam_to_interval(square, Direction::U)?;
        unit.reparam_to_interval(square, Direction::V)?;
        // the samples, row a at u = pu[a], column b at v = pv[b]
        let mut grid: Vec<Vec<Vec3>> = Vec::new();
        for &a in &pu {
            grid.push(
                pv.iter()
                    .map(|&b| unit.eval(a, b, Side::Left, Side::Left))
                    .collect::<solidcore::Result<_>>()?,
            );
        }
        // along u, column by column: rows[i][b] the i-th u point of column b
        let knots_u = KnotVector::from_slice(&uk)?;
        let mut along_u: Vec<Vec<Vec3>> = vec![Vec::new(); nu];
        for b in 0..sv {
            let column: Vec<Vec3> = grid.iter().filter_map(|row| row.get(b).copied()).collect();
            let fit = approx_knots(&column, &pu, &knots_u, nu - 1, deg_u)?;
            for (row, p) in along_u.iter_mut().zip(fit.cpts()) {
                row.push(Vec3::new(p.x, p.y, p.z));
            }
        }
        // then along v, row by row
        let knots_v = KnotVector::from_slice(&vk)?;
        let mut net: Vec<Vec<CPoint>> = Vec::new();
        for row in &along_u {
            let fit = approx_knots(row, &pv, &knots_v, nv - 1, deg_v)?;
            net.push(fit.cpts().to_vec());
        }
        let fit = NurbSurface::new(net, deg_u, deg_v, uk.clone(), vk.clone())?;
        let bound = certified_distance(&unit, &fit)?;
        Ok(Ok((fit, bound)))
    })??;
    let (fit, bound) = fitted;
    if bound > tol {
        return Err(Error::PastTolerance { bound });
    }
    let made = record(run("surface_cvs::rebuild", |k| {
        sheet_starting(k, BSplineSurface::from_gw_nurb(&fit), first)
    }))?;
    Ok((made, bound))
}

/// `sheet` with the control rows next to its record edge `edge` moved so the edge meets record
/// edge `target` of `other` at `order` (the kernel's
/// [`solidcore::nurbs::srf_match::match_boundary`]: at most `rows.rows` rows from the edge move,
/// the rows the order does not fix following by `rows.blend`, G2's row along the normal only, the
/// sheet refined first where its knots cannot carry the target), and the seam after, measured at
/// the edge's stations. Both sheets are one untrimmed face (as [`control_net`] reads them), each
/// edge a side of its face's box. Refused when the seam would be worse than before in a measure
/// the order holds: past both what it was and `held`, the caller's floor for that measure (the
/// gap, the angle, the curvature jump), so rounding at a seam already held is not "worse".
///
/// # Errors
/// As [`control_net`] for either sheet; [`Error::Argument`] for an edge past its record's list
/// or not a side of its face's box, a floor in `held` negative or not finite, and the kernel's
/// refusals of the input (rows outside 1..=3 or below the order plus one, a blend outside 0..=1,
/// knots not clamped, a target degenerate at a station); [`Error::Unsupported`] for a match that
/// would leave the seam worse; the kernel's status otherwise.
pub fn match_edge(
    sheet: &Brep,
    edge: usize,
    other: &Brep,
    target: usize,
    order: MatchOrder,
    rows: MatchRows,
    held: SeamBounds,
) -> Result<(Brep, SeamBounds)> {
    if ![held.gap, held.angle, held.curvature]
        .iter()
        .all(|t| t.is_finite() && *t >= 0.0)
    {
        return Err(Error::Argument);
    }
    let side = side_of(sheet, edge)?;
    let target_side = side_of(other, target)?;
    let first = first_side(sheet)?;
    let ((u, v), points) = untrimmed(sheet)?;
    let ((tu, tv), tpoints) = untrimmed(other)?;
    // spline sheets on polynomial surfaces only, as before rational nets were read
    if points.is_rational() || tpoints.is_rational() {
        return Err(Error::Argument);
    }
    let surface = kernel_surface(u, v, points)?;
    let target_surface = kernel_surface(tu, tv, tpoints)?;
    let matched = run("surface_cvs::match_edge", |_| {
        Ok(
            match match_boundary(&surface, side, &target_surface, target_side, order, rows) {
                Ok(m) => Ok(m),
                Err(SolidError::InvalidInput { .. }) => Err(Error::Argument),
                Err(e) => return Err(e),
            },
        )
    })??;
    let (before, after) = (matched.before, matched.after);
    let worse = |now: f64, was: f64, floor: f64| now > was.max(floor);
    let k = order.order();
    if worse(after.gap, before.gap, held.gap)
        || (k >= 1 && worse(after.angle, before.angle, held.angle))
        || (k >= 2 && worse(after.curvature, before.curvature, held.curvature))
    {
        return Err(Error::Unsupported(
            "the match would leave the seam worse than it was",
        ));
    }
    let made = record(run("surface_cvs::match_edge", |k| {
        sheet_starting(k, BSplineSurface::from_gw_nurb(&matched.surface), first)
    }))?;
    Ok((made, after))
}

/// A plane a sheet's edge is matched to as its plane of symmetry ([`match_plane`]): a point on
/// it and its normal (any length but none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorPlane {
    /// A point on the plane.
    pub origin: [f64; 3],
    /// Its normal.
    pub normal: [f64; 3],
}

/// `sheet` with the control rows next to its record edge `edge` moved so the sheet meets its own
/// mirror image in `plane` at `order` (the kernel's
/// [`solidcore::nurbs::srf_match::match_plane`]: the edge row put on the plane, at G1 and G2 the
/// next row square to it, which makes the sheet curvature continuous with its mirror too; at
/// most `rows.rows` rows from the edge move, the rows the order does not fix following by
/// `rows.blend`), and the seam with the mirror after, measured at the edge's stations. The sheet
/// is one untrimmed face (as [`control_net`] reads it), the edge a side of its face's box.
/// Refused when the seam would be worse than before in a measure the order holds, past both what
/// it was and `held`, as [`match_edge`] refuses.
///
/// # Errors
/// As [`control_net`]; [`Error::Argument`] for an edge past the record's list or not a side of
/// its face's box, a plane not finite or past the coordinate bound, a floor in `held` negative or
/// not finite, and the kernel's refusals of the input (a normal of no length, an edge that does
/// not leave the plane to one side, rows outside 1..=3 or below what the order fixes, a blend
/// outside 0..=1, knots not clamped); [`Error::Unsupported`] for a match that would leave the seam
/// worse; the kernel's status otherwise.
pub fn match_plane(
    sheet: &Brep,
    edge: usize,
    plane: MirrorPlane,
    order: MatchOrder,
    rows: MatchRows,
    held: SeamBounds,
) -> Result<(Brep, SeamBounds)> {
    if ![held.gap, held.angle, held.curvature]
        .iter()
        .all(|t| t.is_finite() && *t >= 0.0)
        || !coord_ok(&plane.origin)
        || !coord_ok(&plane.normal)
    {
        return Err(Error::Argument);
    }
    let side = side_of(sheet, edge)?;
    let first = first_side(sheet)?;
    let ((u, v), points) = untrimmed(sheet)?;
    if points.is_rational() {
        return Err(Error::Argument);
    }
    let surface = kernel_surface(u, v, points)?;
    let v3 = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    let matched = run("surface_cvs::match_plane", |_| {
        Ok(
            match kernel_match_plane(
                &surface,
                side,
                v3(plane.origin),
                v3(plane.normal),
                order,
                rows,
            ) {
                Ok(m) => Ok(m),
                Err(SolidError::InvalidInput { .. }) => Err(Error::Argument),
                Err(e) => return Err(e),
            },
        )
    })??;
    let (before, after) = (matched.before, matched.after);
    let worse = |now: f64, was: f64, floor: f64| now > was.max(floor);
    let k = order.order();
    if worse(after.gap, before.gap, held.gap)
        || (k >= 1 && worse(after.angle, before.angle, held.angle))
        || (k >= 2 && worse(after.curvature, before.curvature, held.curvature))
    {
        return Err(Error::Unsupported(
            "the match would leave the seam worse than it was",
        ));
    }
    let made = record(run("surface_cvs::match_plane", |k| {
        sheet_starting(k, BSplineSurface::from_gw_nurb(&matched.surface), first)
    }))?;
    Ok((made, after))
}

/// The side of its face's box record edge `edge` of the one-face untrimmed `sheet` runs along:
/// its use's pcurve all on `u = umin` (left), `u = umax` (right), `v = vmin` (bottom) or
/// `v = vmax` (top).
///
/// # Errors
/// As [`untrimmed`]; [`Error::Argument`] for an edge the face does not use, or one not on a side.
fn side_of(sheet: &Brep, edge: usize) -> Result<Boundary> {
    untrimmed(sheet)?;
    let [face] = sheet.faces.as_slice() else {
        return Err(Error::Argument);
    };
    let [[u0, u1], [v0, v1]] = face.domain;
    let [Loop::Edges { edges }] = face.loops.as_slice() else {
        return Err(Error::Argument);
    };
    let Some(used) = edges
        .iter()
        .find(|e| usize::try_from(e.edge).is_ok_and(|i| i == edge))
    else {
        return Err(Error::Argument);
    };
    let Curve2::Spline { points: uv, .. } = &used.pcurve else {
        return Err(Error::Argument);
    };
    let on = |k: usize, at: f64| uv.iter().all(|p| p.get(k) == Some(&at));
    if on(0, u0) {
        Ok(Boundary::Left)
    } else if on(0, u1) {
        Ok(Boundary::Right)
    } else if on(1, v0) {
        Ok(Boundary::Bottom)
    } else if on(1, v1) {
        Ok(Boundary::Top)
    } else {
        Err(Error::Argument)
    }
}

/// Whether the first and last `degree + 1` of `knots` are each one value.
fn clamped(knots: &[f64], degree: usize) -> bool {
    let flat =
        |ends: Option<&[f64]>| ends.is_some_and(|e| e.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(degree + 1);
    flat(knots.get(..=degree)) && flat(last.and_then(|s| knots.get(s..)))
}

/// A record's control net as the record holds it: Euclidean points of a polynomial surface, or
/// the homogeneous `[x w, y w, z w, w]` of a rational one (the kernel's own form).
#[derive(Clone, Copy, Debug)]
pub(crate) enum RecordNet<'a> {
    /// `Surface::Spline`'s points.
    Plain(&'a [Vec<[f64; 3]>]),
    /// `Surface::RationalSpline`'s homogeneous points.
    Rational(&'a [Vec<[f64; 4]>]),
}

impl RecordNet<'_> {
    fn rows(self) -> usize {
        match self {
            Self::Plain(n) => n.len(),
            Self::Rational(n) => n.len(),
        }
    }

    fn cols(self) -> usize {
        match self {
            Self::Plain(n) => n.first().map_or(0, Vec::len),
            Self::Rational(n) => n.first().map_or(0, Vec::len),
        }
    }

    fn is_rational(self) -> bool {
        matches!(self, Self::Rational(_))
    }
}

/// The kernel's surface of a record net and its knots (degrees from the knot counts).
///
/// # Errors
/// [`Error::Argument`] for knots that do not fit the net.
fn kernel_surface(u: &[f64], v: &[f64], net: RecordNet<'_>) -> Result<NurbSurface> {
    let p = u.len().checked_sub(net.rows() + 1).ok_or(Error::Argument)?;
    let q = v.len().checked_sub(net.cols() + 1).ok_or(Error::Argument)?;
    let pw = match net {
        RecordNet::Plain(n) => n
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
                    .collect()
            })
            .collect(),
        RecordNet::Rational(n) => n
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                    .collect()
            })
            .collect(),
    };
    NurbSurface::new(pw, p, q, u.to_vec(), v.to_vec()).map_err(|_| Error::Argument)
}

/// The knots `(u, v)` and net of the one face of `sheet`, when it is untrimmed: one face on a
/// B-spline surface, polynomial or rational (no separate NURBS form), its domain the surface's
/// natural box, one loop, and every use's pcurve on a side of that box (a spline pcurve whose
/// control points all share the side's u or v: by their convex hull, the whole pcurve lies on
/// it).
///
/// # Errors
/// [`Error::Argument`] for a record of more or fewer faces or another surface;
/// [`Error::Unsupported`] for a face that is trimmed (or not proved untrimmed: a rational
/// pcurve, a pcurve off the box's sides, holes).
///
/// A planar sheet is taken through [`editable`] first by the net edits (`control_net`,
/// `move_cvs`, `move_net`, `rebuild`); `match_edge`, which names its edges by record index,
/// takes spline sheets only (a planar one's rebuilt edges would need the edge map).
#[allow(clippy::type_complexity)]
pub(crate) fn untrimmed(sheet: &Brep) -> Result<((&[f64], &[f64]), RecordNet<'_>)> {
    let [face] = sheet.faces.as_slice() else {
        return Err(Error::Argument);
    };
    let (u_knots, v_knots, net) = match (&face.surface, &face.nurbs) {
        (
            Surface::Spline {
                u_knots,
                v_knots,
                points,
            },
            None,
        ) => (u_knots, v_knots, RecordNet::Plain(points)),
        (
            Surface::RationalSpline {
                u_knots,
                v_knots,
                homogeneous,
            },
            None,
        ) => (u_knots, v_knots, RecordNet::Rational(homogeneous)),
        _ => return Err(Error::Argument),
    };
    let (Natural::Range(u0, u1), Natural::Range(v0, v1)) = (
        knot_domain(u_knots, net.rows()),
        knot_domain(v_knots, net.cols()),
    ) else {
        return Err(Error::Argument);
    };
    let trimmed = Err(Error::Unsupported("a trimmed face"));
    if face.domain != [[u0, u1], [v0, v1]] {
        return trimmed;
    }
    let [Loop::Edges { edges }] = face.loops.as_slice() else {
        return trimmed;
    };
    for used in edges {
        let Curve2::Spline { points: uv, .. } = &used.pcurve else {
            return trimmed;
        };
        let side = |k: usize, at: f64| uv.iter().all(|p| p.get(k) == Some(&at));
        if !(side(0, u0) || side(0, u1) || side(1, v0) || side(1, v1)) {
            return trimmed;
        }
    }
    Ok(((u_knots, v_knots), net))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::ruled;
    use solidcore_record::brep::Curve;

    /// The ruled sheet between two cubic splines, a one-face untrimmed sheet.
    fn sheet() -> Brep {
        let a = Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: vec![
                [0.0, 0.0, 0.0],
                [3.0, 0.0, 2.0],
                [6.0, 0.0, -1.0],
                [9.0, 0.0, 0.0],
            ],
        };
        let b = Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: vec![
                [0.0, 8.0, 1.0],
                [3.0, 8.0, 0.0],
                [6.0, 8.0, 2.0],
                [9.0, 8.0, 1.0],
            ],
        };
        ruled(&a, &b, false).unwrap()
    }

    /// A spline sheet is not rebuilt: `editable` hands the record back borrowed (not converted),
    /// so every net edit of a spline sheet reads and makes exactly what it did before planar
    /// sheets were taken.
    #[test]
    fn a_spline_sheet_is_edited_as_it_is() {
        let s = sheet();
        match editable(&s).unwrap() {
            Cow::Borrowed(b) => assert_eq!(b, &s),
            Cow::Owned(_) => panic!("a spline sheet was rebuilt"),
        }
    }

    #[test]
    fn a_ruled_sheets_net_is_its_two_curves() {
        let net = control_net(&sheet()).unwrap();
        assert_eq!(net.rows * net.cols, net.points.len());
        // one direction runs along the curves (4 points), the other across (2)
        assert!(
            (net.rows, net.cols) == (4, 2) || (net.rows, net.cols) == (2, 4),
            "{net:?}"
        );
        for corner in [
            [0.0, 0.0, 0.0],
            [9.0, 0.0, 0.0],
            [0.0, 8.0, 1.0],
            [9.0, 8.0, 1.0],
        ] {
            assert!(net.points.contains(&corner), "{corner:?} in {net:?}");
        }
    }

    /// The point of a sheet record's surface at `(u, v)`, by the kernel's evaluator.
    fn at(sheet: &Brep, u: f64, v: f64) -> [f64; 3] {
        let ((uk, vk), net) = untrimmed(sheet).unwrap();
        let p = kernel_surface(uk, vk, net)
            .unwrap()
            .eval(u, v, Side::Left, Side::Left)
            .unwrap();
        [p.x, p.y, p.z]
    }

    /// A ruled sheet between two cubics of six control points (interior knots 1/3 and 2/3).
    fn long_sheet() -> Brep {
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0];
        let row = |y: f64| Curve::Spline {
            knots: knots.clone(),
            points: (0..6)
                .map(|i| [f64::from(i) * 2.0, y, f64::from(i % 2)])
                .collect(),
        };
        ruled(&row(0.0), &row(5.0), false).unwrap()
    }

    #[test]
    fn moving_one_cv_changes_only_its_local_span() {
        let s = long_sheet();
        let net = control_net(&s).unwrap();
        // the direction along the curves has six points; the first of them reaches only the
        // first third of it
        let (along_u, first) = (net.rows == 6, (0, 0));
        let moved = move_cvs(&s, &[(first, [0.0, 0.0, 3.0])]).unwrap();
        let after = control_net(&moved).unwrap();
        assert_eq!((after.rows, after.cols), (net.rows, net.cols));
        assert_eq!(after.points[0], [0.0, 0.0, 3.0]);
        assert_eq!(&after.points[1..], &net.points[1..]);
        for i in 0..=20 {
            for j in 0..=20 {
                let (a, b) = (f64::from(i) / 20.0, f64::from(j) / 20.0);
                let along = if along_u { a } else { b };
                let (p, q) = (at(&s, a, b), at(&moved, a, b));
                if along >= 1.0 / 3.0 {
                    assert!(
                        p.iter().zip(q).all(|(x, y)| (x - y).abs() < 1e-12),
                        "moved outside its span at ({a}, {b}): {p:?} and {q:?}"
                    );
                }
            }
        }
        // the moved point's span did move
        let (a, b) = if along_u { (0.05, 0.0) } else { (0.0, 0.05) };
        assert_ne!(at(&s, a, b), at(&moved, a, b));
        // bad moves
        assert_eq!(
            move_cvs(&s, &[((net.rows, 0), [0.0; 3])]),
            Err(Error::Argument)
        );
        assert_eq!(
            move_cvs(&s, &[((0, 0), [f64::NAN, 0.0, 0.0])]),
            Err(Error::Argument)
        );
    }

    #[test]
    fn a_row_move_moves_exactly_that_row() {
        let s = long_sheet();
        let net = control_net(&s).unwrap();
        let delta = [0.0, 0.0, 1.0];
        let last = net.rows - 1;
        for falloff in [
            None,
            Some(Falloff {
                radius: 0,
                shape: Shape::Smooth,
            }),
        ] {
            let moved = move_net(&s, &Picks::Rows(vec![last]), delta, falloff).unwrap();
            let after = control_net(&moved).unwrap();
            assert_eq!((after.rows, after.cols), (net.rows, net.cols));
            for (n, (p, q)) in net.points.iter().zip(&after.points).enumerate() {
                if n / net.cols == last {
                    assert_eq!(*q, [p[0], p[1], p[2] + 1.0], "point {n}");
                } else {
                    assert_eq!(p, q, "point {n}");
                }
            }
        }
        // a column the same way
        let moved = move_net(&s, &Picks::Columns(vec![0]), delta, None).unwrap();
        let after = control_net(&moved).unwrap();
        for (n, (p, q)) in net.points.iter().zip(&after.points).enumerate() {
            if n % net.cols == 0 {
                assert_eq!(*q, [p[0], p[1], p[2] + 1.0], "point {n}");
            } else {
                assert_eq!(p, q, "point {n}");
            }
        }
    }

    #[test]
    fn falloff_weights_are_one_at_the_pick_zero_past_the_radius_and_monotone() {
        assert_eq!(falloff_weight(None, 0), 1.0);
        assert_eq!(falloff_weight(None, 1), 0.0);
        for shape in [Shape::Linear, Shape::Smooth] {
            let f = Some(Falloff { radius: 3, shape });
            let w: Vec<f64> = (0..8).map(|d| falloff_weight(f, d)).collect();
            assert_eq!(w[0], 1.0, "{shape:?}");
            assert!(
                w[1..=3].iter().all(|&x| x > 0.0 && x < 1.0),
                "{shape:?}: {w:?}"
            );
            assert!(w[4..].iter().all(|&x| x == 0.0), "{shape:?}: {w:?}");
            assert!(
                w.windows(2).all(|p| p[1] < p[0] || p[1] == 0.0),
                "{shape:?}: {w:?}"
            );
            // the table: a point pick's distance is its square ring on the net
            let table = net_weights(9, 7, &Picks::Cvs(vec![(4, 3)]), f).unwrap();
            assert_eq!(table.len(), 63);
            for i in 0..9_usize {
                for j in 0..7_usize {
                    let d = i.abs_diff(4).max(j.abs_diff(3));
                    assert_eq!(table[i * 7 + j], w[d], "{shape:?} at ({i}, {j})");
                }
            }
        }
        // the smooth shape is flat at the pick: it falls less over the first step
        let (linear, smooth) = (
            falloff_weight(
                Some(Falloff {
                    radius: 3,
                    shape: Shape::Linear,
                }),
                1,
            ),
            falloff_weight(
                Some(Falloff {
                    radius: 3,
                    shape: Shape::Smooth,
                }),
                1,
            ),
        );
        assert!(smooth > linear, "{smooth} {linear}");
        // rows and columns by their one index
        let rows = net_weights(
            5,
            3,
            &Picks::Rows(vec![0]),
            Some(Falloff {
                radius: 1,
                shape: Shape::Linear,
            }),
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        // bad picks
        assert_eq!(
            net_weights(5, 3, &Picks::Rows(vec![]), None),
            Err(Error::Argument)
        );
        assert_eq!(
            net_weights(5, 3, &Picks::Rows(vec![5]), None),
            Err(Error::Argument)
        );
        assert_eq!(
            net_weights(5, 3, &Picks::Columns(vec![3]), None),
            Err(Error::Argument)
        );
        assert_eq!(
            net_weights(5, 3, &Picks::Cvs(vec![(0, 3)]), None),
            Err(Error::Argument)
        );
        assert_eq!(
            net_weights(0, 3, &Picks::Rows(vec![0]), None),
            Err(Error::Argument)
        );
    }

    #[test]
    fn a_soft_move_moves_each_point_by_its_weight() {
        let s = bicubic();
        let net = control_net(&s).unwrap();
        let f = Some(Falloff {
            radius: 1,
            shape: Shape::Linear,
        });
        let picks = Picks::Cvs(vec![(1, 1)]);
        let moved = move_net(&s, &picks, [0.0, 0.0, 2.0], f).unwrap();
        let after = control_net(&moved).unwrap();
        let weights = net_weights(net.rows, net.cols, &picks, f).unwrap();
        for (n, ((p, q), w)) in net
            .points
            .iter()
            .zip(&after.points)
            .zip(&weights)
            .enumerate()
        {
            assert_eq!((q[0], q[1]), (p[0], p[1]), "point {n}");
            assert!(
                (q[2] - (p[2] + 2.0 * w)).abs() <= 1e-15 * (1.0 + p[2].abs()),
                "point {n}: {p:?} to {q:?}, weight {w}"
            );
        }
        // the pick by the whole delta, its ring by half, the rest not at all
        assert_eq!(weights[5], 1.0);
        assert_eq!(weights[0], 0.5);
        assert_eq!(weights[3 * 4 + 3], 0.0);
        assert_eq!(after.points[15], net.points[15]);
        // bad moves
        assert_eq!(
            move_net(&s, &picks, [f64::NAN, 0.0, 0.0], f),
            Err(Error::Argument)
        );
        assert_eq!(
            move_net(&s, &Picks::Cvs(vec![(4, 0)]), [0.0; 3], f),
            Err(Error::Argument)
        );
    }

    /// The Coons sheet of four bent cubic Bézier sides: a bicubic Bézier patch.
    fn bicubic() -> Brep {
        let side = |points: [[f64; 3]; 4]| Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: points.to_vec(),
        };
        crate::surface::boundary(&[
            side([
                [0.0, 0.0, 0.0],
                [3.0, 0.0, 2.0],
                [6.0, 0.0, -1.0],
                [9.0, 0.0, 0.0],
            ]),
            side([
                [9.0, 0.0, 0.0],
                [9.0, 3.0, 1.0],
                [9.0, 6.0, -2.0],
                [9.0, 9.0, 0.0],
            ]),
            side([
                [9.0, 9.0, 0.0],
                [6.0, 9.0, 2.0],
                [3.0, 9.0, 1.0],
                [0.0, 9.0, 0.0],
            ]),
            side([
                [0.0, 9.0, 0.0],
                [0.0, 6.0, -1.0],
                [0.0, 3.0, 2.0],
                [0.0, 0.0, 0.0],
            ]),
        ])
        .unwrap()
    }

    #[test]
    fn a_bicubic_rebuilt_into_its_own_space_is_itself() {
        let s = bicubic();
        let net = control_net(&s).unwrap();
        assert_eq!((net.rows, net.cols), (4, 4), "{net:?}");
        let (r, bound) = rebuild(&s, 3, 3, 1, 1, 1e-9).unwrap();
        assert!(bound < 1e-9, "{bound}");
        for i in 0..=10 {
            for j in 0..=10 {
                let (a, b) = (f64::from(i) / 10.0, f64::from(j) / 10.0);
                let (p, q) = (at(&s, a, b), at(&r, a, b));
                assert!(p.iter().zip(q).all(|(x, y)| (x - y).abs() <= bound));
            }
        }
    }

    #[test]
    fn a_rebuild_past_the_tolerance_is_refused_with_its_bound() {
        let s = bicubic();
        // the bent patch is no bilinear one
        assert!(matches!(
            rebuild(&s, 1, 1, 2, 2, 1e-6),
            Err(Error::PastTolerance { bound }) if bound > 1e-6
        ));
        // and a looser tolerance takes the same fit
        let (_, bound) = rebuild(&s, 2, 2, 2, 2, 100.0).unwrap();
        assert!(bound > 0.0 && bound <= 100.0);
        // bad arguments
        assert_eq!(rebuild(&s, 0, 3, 1, 1, 1.0), Err(Error::Argument));
        assert_eq!(rebuild(&s, 3, 3, 0, 1, 1.0), Err(Error::Argument));
        assert_eq!(rebuild(&s, 1, 1, 1, 1, 1.0), Err(Error::Argument));
        assert_eq!(rebuild(&s, 3, 3, 1, 1, f64::NAN), Err(Error::Argument));
    }

    #[test]
    fn a_surface_on_unclamped_knots_is_not_sampled() {
        // the bicubic's u knots made unclamped on the same domain [0, 1]: its face, box and
        // sides unchanged, so it reads as untrimmed, but its ends are no clamped ends
        let mut s = bicubic();
        let Surface::Spline { u_knots, .. } = &mut s.faces[0].surface else {
            panic!("a spline sheet")
        };
        *u_knots = vec![-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0];
        assert!(control_net(&s).is_ok());
        assert_eq!(rebuild(&s, 3, 3, 1, 1, 1.0), Err(Error::Argument));
    }

    #[test]
    fn a_record_of_other_faces_or_a_trimmed_face_is_refused() {
        let mut two = sheet();
        let face = two.faces[0].clone();
        two.faces.push(face);
        assert_eq!(control_net(&two), Err(Error::Argument));
        let mut smaller = sheet();
        smaller.faces[0].domain[0][1] *= 0.5;
        assert_eq!(
            control_net(&smaller),
            Err(Error::Unsupported("a trimmed face"))
        );
    }
}
