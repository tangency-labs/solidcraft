//! Extend: a sheet continued past the side of its surface's box that one of its edges runs
//! along, by a distance measured across that side, as the kernel's exact extension
//! ([`Surface::extended_by_distance`]) makes it: a sheet of one face, or of several faces that
//! are the pieces of one surface (the kernel's [`one_surface`], extended once and cut again
//! where the faces were, [`sheet_of`]). Ops chooses the side the edge is, checks each face is
//! its surface's whole box, sequences the kernel's calls and carries the faces' and sides'
//! attributes onto the new sheet; it computes nothing.
//!
//! A face on a surface that goes on past its trims (a plane: [`Surface::reaches_past_trims`])
//! is extended by growing its trim within it instead, as the kernel's
//! [`extend_trim`] makes it: the edge moved outward by the distance, its neighbours carried
//! along their tangents to meet it, its holes kept; every continuation is the plane itself.
//!
//! The edge is a record index ([`Order`]); which edge a pick means is the application's.

use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::extend::{Continuity, ExtendRefusal, Extended};
use solidcore::surface::side::BoxSide;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::sheet::{one_surface, sheet_of, sheet_on, Pieces};
use solidcore::topology::brep::trim_extend::extend_trim;
use solidcore::topology::face::{p1 as face_p1, p2 as face_p2, p4 as face_p4};
use solidcore::topology::{Brep, EdgeId, FaceId};
use solidcore::{EntityRef, SolidError};
use solidcore_convert::brep::Order;

use crate::body::{copy_attributes, finish, upward, Body, Expect};
use crate::error::status;
use crate::session::read_body;
use crate::{Error, Result};

/// How a surface continues past its side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Continuation {
    /// The surface itself, continued.
    Natural,
    /// Tangent continuous.
    Tangent,
    /// Curvature continuous.
    Curvature,
}

/// Points sampled along a side to find whether an edge runs along all of it.
const SAMPLES: u32 = 9;

/// The parameters `0, 1 / SAMPLES, ..., 1` along a side.
#[allow(
    clippy::float_arithmetic,
    reason = "the sample parameters ops chooses, not a geometric quantity (docs/layers.md)"
)]
fn along() -> impl Iterator<Item = f64> {
    (0..=SAMPLES).map(|i| f64::from(i) / f64::from(SAMPLES))
}

/// How far `p` is from edge `e`; infinite where the kernel gives no distance.
fn edge_distance(k: &Kernel, b: &Brep, e: EdgeId, p: solidcore::geom::vector3d::Point3) -> f64 {
    match edge_closest_point(k, b, e, p) {
        Ok(c) if c.closest.distance.is_finite() => c.closest.distance,
        _ => f64::INFINITY,
    }
}

/// Whether side `s` of `dom` collapses to a point within `on`: by the kernel's certified side
/// diameter; undecided (the enclosure straddles `on`) is [`Error::Unsupported`].
fn collapsed(
    k: &Kernel,
    surf: &Surface,
    dom: &Extent2d,
    s: BoxSide,
    on: f64,
) -> solidcore::Result<Result<bool>> {
    let d = surf.side_diameter(k, dom, s)?;
    Ok(if d.upper <= on {
        Ok(true)
    } else if d.lower > on {
        Ok(false)
    } else {
        Err(Error::Unsupported(
            "a side's collapse to a point is undecided",
        ))
    })
}

/// Whether every sample of side `s` lies on one of `edges` within `on`.
fn on_edges(
    k: &Kernel,
    b: &Brep,
    surf: &Surface,
    dom: &Extent2d,
    s: BoxSide,
    edges: &[EdgeId],
    on: f64,
) -> solidcore::Result<bool> {
    for t in along() {
        let p = surf.evaluate_point(k, s.point(dom, t))?;
        if !edges.iter().any(|&e| edge_distance(k, b, e, p) <= on) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The edge of `edges` nearest side `s`'s middle, within `on`.
fn side_edge(
    k: &Kernel,
    b: &Brep,
    surf: &Surface,
    dom: &Extent2d,
    s: BoxSide,
    edges: &[EdgeId],
    on: f64,
) -> solidcore::Result<Option<EdgeId>> {
    let m = surf.evaluate_point(k, s.point(dom, 0.5))?;
    let mut best: Option<(f64, EdgeId)> = None;
    for &e in edges {
        let d = edge_distance(k, b, e, m);
        if d <= on && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, e));
        }
    }
    Ok(best.map(|(_, e)| e))
}

/// The kernel's refusal as the operation's: a weight vanishing, a pole, the apex or a full turn
/// within the distance, or a length there not measurable to the kernel's stop, names the
/// longest distance that works; a distance the kernel cannot meet to its stop, naming none, is
/// not exact.
fn refused(e: SolidError) -> Error {
    match e {
        SolidError::ExtendRefused {
            refusal: ExtendRefusal::NotExact,
            distance: None,
            ..
        }
        | SolidError::NotConverged { .. } => Error::NotExact,
        SolidError::ExtendRefused { distance, .. } => Error::TooFar { largest: distance },
        SolidError::Unsupported { reason, .. } => Error::Unsupported(reason),
        e => status(e),
    }
}

/// How near a point must be to an edge to be on it: ten of the body's tolerance.
#[allow(
    clippy::float_arithmetic,
    reason = "a tolerance ops chooses: ten of the body's"
)]
fn near(b: &Brep) -> f64 {
    10.0 * b.tolerance().max(1e-9)
}

/// The side of `surf`'s face box `dom` that `picked` runs along all of, each side not collapsed
/// a side of the face (on its `edges`).
fn grown_side(
    k: &Kernel,
    b: &Brep,
    surf: &Surface,
    dom: &Extent2d,
    edges: &[EdgeId],
    picked: EdgeId,
    on: f64,
) -> solidcore::Result<Result<BoxSide>> {
    let mut grow = None;
    for s in BoxSide::ALL {
        match collapsed(k, surf, dom, s, on)? {
            Err(e) => return Ok(Err(e)),
            Ok(true) => continue,
            Ok(false) => {}
        }
        if !on_edges(k, b, surf, dom, s, edges, on)? {
            return Ok(Err(Error::Unsupported(
                "a side of the box that is not a side of the face",
            )));
        }
        if on_edges(k, b, surf, dom, s, &[picked], on)? {
            grow = Some(s);
        }
    }
    Ok(grow.ok_or(Error::Unsupported(
        "an edge that is not all of one side of the box",
    )))
}

/// A face and what its sides are found on: its surface, its box, its edges and how near a point
/// must be to an edge to be on it.
struct Sides<'a> {
    b: &'a Brep,
    surf: &'a Surface,
    dom: &'a Extent2d,
    edges: &'a [EdgeId],
    on: f64,
}

/// Each side of `was` (not collapsed) gives its edge's attributes to the edge on the side of
/// `now` (a face of `r`) that `mapped` says it is on.
fn carry_sides(
    k: &Kernel,
    was: &Sides<'_>,
    r: &mut Brep,
    now: (&Surface, &Extent2d, &[EdgeId]),
    mapped: [BoxSide; 4],
) -> solidcore::Result<()> {
    let (surf, dom, made) = now;
    let new_on = near(r);
    for (s, to_s) in BoxSide::ALL.into_iter().zip(mapped) {
        // a collapsed side has no edge of its own
        if !matches!(collapsed(k, was.surf, was.dom, s, was.on)?, Ok(false)) {
            continue;
        }
        let from = side_edge(k, was.b, was.surf, was.dom, s, was.edges, was.on)?;
        // the side the kernel says side s is on in the result's parameters
        let to = side_edge(k, r, surf, dom, to_s, made, new_on)?;
        if let (Some(from), Some(to)) = (from, to) {
            copy_attributes(was.b, EntityRef::Edge(from), r, EntityRef::Edge(to))?;
        }
    }
    Ok(())
}

/// The kernel's extension as the operation's answer: the surface and where each side went, no
/// effect, or the refusal.
fn extension(
    made: solidcore::Result<Option<Extended>>,
) -> solidcore::Result<Result<(Surface, [BoxSide; 4])>> {
    match made {
        Ok(Some(e)) => {
            let mapped = BoxSide::ALL.map(|s| e.side_of(s));
            Ok(Ok((e.surface, mapped)))
        }
        // a distance below the side's resolution is no extension: the sheet would come back
        // as it is, which is not what was asked
        Ok(None) => Ok(Err(Error::NoEffect)),
        Err(e) if e.is_unported() || e.is_cancelled() => Err(e),
        Err(e) => Ok(Err(refused(e))),
    }
}

/// The sheet `sheet` continued `distance` past the side of its surface's box that record edge
/// `edge` runs along, as `how` says: a new sheet, `sheet` unchanged. Its faces and sides carry
/// the attributes of the faces and sides they continue, each face facing the way it did.
///
/// - A sheet of one face: the edge all of one side of the face, its surface's whole box; the
///   face continued on its surface extended. A face on a surface that goes on past its trims (a
///   plane) is extended by growing its trim ([`extend_trim`]: the edge, on its outer loop, moved
///   `distance` outward, its neighbours carried along their tangents), the same for every `how`.
/// - A sheet of several faces that are the pieces of one surface, as the kernel splits a
///   surface into faces ([`one_surface`]: a skin through a section with a double knot is two
///   faces): the edge all of one side of its face, that side on the side of the surface's whole
///   box. The surface is extended once, the distance met at the picked edge's face, and cut
///   again where the faces were ([`sheet_of`]): the whole side grows, every face along it with
///   it, and the faces stay as they were split, their seams one edge each.
///
/// # Errors
/// - [`Error::Argument`]: a distance not finite and positive, an edge past the record's list or
///   not the sheet's (on a plane, not on its face's outer loop);
/// - [`Error::Unsupported`] (with what): a sheet of several faces that are not one surface's
///   pieces, a face with holes or that is not its surface's whole box, the edge not all of one
///   side, or between two faces, a side whose collapse is undecided, a surface with no exact
///   extension; on a plane, the trim refused ([`extend_trim`]: a neighbour that never meets the
///   moved edge, neighbours that cross, a moved edge crossing the others);
/// - [`Error::TooFar`]: a weight would vanish, a pole, the apex or a full turn be reached, or
///   the length not be measurable to the kernel's stop within the distance (with the longest
///   that works, when the kernel names one);
/// - [`Error::NotExact`]: the distance not met to the kernel's stop, no shorter one named; on a
///   plane, a spline edge's offset not within a tenth of the tolerance;
/// - [`Error::NoEffect`]: the distance below the side's resolution (the shortest extension its
///   knot can carry is proved longer): no extension, the sheet would be returned as it is;
/// - the kernel's errors.
pub fn extend_sheet(
    sheet: &Body,
    order: &Order,
    edge: usize,
    distance: f64,
    how: Continuation,
) -> Result<Body> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err(Error::Argument);
    }
    let picked = *order.edges.get(edge).ok_or(Error::Argument)?;
    let how = match how {
        Continuation::Natural => Continuity::Natural,
        Continuation::Tangent => Continuity::Tangent,
        Continuation::Curvature => Continuity::Curvature,
    };
    // the sheet is only read: the result is a new body, and the record's edge is the sheet's own
    read_body("extend_sheet", sheet, |k, b| {
        let faces = brep_p1::faces(b, None)?;
        if let [f] = faces.as_slice() {
            return one_face(k, b, *f, picked, how, distance);
        }
        match one_surface(k, b)? {
            Some(pieces) => pieces_extended(k, b, &pieces, picked, how, distance),
            None => Ok(Err(Error::Unsupported("a sheet of more than one face"))),
        }
    })
    .and_then(|r| r.map(Body::new))
}

/// [`extend_sheet`] on a sheet of one face `f`.
fn one_face(
    k: &mut Kernel,
    b: &Brep,
    f: FaceId,
    picked: EdgeId,
    how: Continuity,
    distance: f64,
) -> solidcore::Result<Result<Brep>> {
    let Some(surf) = face_p2::surface(b, f)?.cloned() else {
        return Ok(Err(Error::Unsupported("a face with no surface")));
    };
    // a surface that goes on past its trims (a plane) grows the trim within itself, the
    // same for every continuation; its face's holes are kept
    if surf.reaches_past_trims() {
        // the trim grows past an edge of its outer loop, not of a hole
        if !face_p4::edges(b, f, None, true, None)?.contains(&picked) {
            return Ok(Err(Error::Argument));
        }
        let made = match extend_trim(k, b, f, picked, distance) {
            Ok(made) => made,
            Err(e) if e.is_unported() || e.is_cancelled() => return Err(e),
            Err(e) => return Ok(Err(refused(e))),
        };
        let mut r = made.brep;
        copy_attributes(b, EntityRef::Face(f), &mut r, EntityRef::Face(made.face))?;
        for (from, to) in made.edges {
            copy_attributes(b, EntityRef::Edge(from), &mut r, EntityRef::Edge(to))?;
        }
        return finish(k, r, Expect::FACES);
    }
    if face_p2::loops(b, f, false, None)?.len() != 1 {
        return Ok(Err(Error::Unsupported("a face with holes")));
    }
    let edges = face_p4::edges(b, f, None, false, None)?;
    if !edges.contains(&picked) {
        return Ok(Err(Error::Argument));
    }
    let on = near(b);
    let dom = face_p2::uv_domain(b, f)?;
    // the face is its surface's whole box, as the kernel's extension decides it
    if !surf.is_whole_box(k, &dom)? {
        return Ok(Err(Error::Unsupported(
            "a face that is not its surface's whole box",
        )));
    }
    let grow = match grown_side(k, b, &surf, &dom, &edges, picked, on)? {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };
    let (grown, mapped) = match extension(surf.extended_by_distance(k, &dom, grow, how, distance))?
    {
        Ok(made) => made,
        Err(e) => return Ok(Err(e)),
    };
    let grown_eval = grown.copy(k)?;
    let (mut r, nf) = sheet_on(k, grown, b.tolerance())?;
    // the face and each side stand for the face and side they continue
    copy_attributes(b, EntityRef::Face(f), &mut r, EntityRef::Face(nf))?;
    let made = face_p4::edges(&r, nf, None, false, None)?;
    let rdom = face_p2::uv_domain(&r, nf)?;
    let was = Sides {
        b,
        surf: &surf,
        dom: &dom,
        edges: &edges,
        on,
    };
    carry_sides(k, &was, &mut r, (&grown_eval, &rdom, &made), mapped)?;
    // the new face faces the way the old one did
    if let (Some(was), Some(now)) = (upward(b, f)?, upward(&r, nf)?) {
        if was != now {
            face_p1::flip_face_orientation(&mut r, nf)?;
        }
    }
    finish(k, r, Expect::FACES)
}

/// [`extend_sheet`] on a sheet whose faces are the pieces of one surface (`pieces`).
fn pieces_extended(
    k: &mut Kernel,
    b: &Brep,
    pieces: &Pieces,
    picked: EdgeId,
    how: Continuity,
    distance: f64,
) -> solidcore::Result<Result<Brep>> {
    // the face the picked edge bounds, and its side the edge is all of
    let mut at = None;
    for &(f, dom) in &pieces.faces {
        let edges = face_p4::edges(b, f, None, false, None)?;
        if edges.contains(&picked) {
            at = Some((f, dom, edges));
            break;
        }
    }
    let Some((f, dom, edges)) = at else {
        return Ok(Err(Error::Argument));
    };
    let Some(surf) = face_p2::surface(b, f)?.cloned() else {
        return Ok(Err(Error::Unsupported("a face with no surface")));
    };
    let on = near(b);
    let grow = match grown_side(k, b, &surf, &dom, &edges, picked, on)? {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };
    // a side inside the whole box is a seam, which two faces share
    if !pieces.on_side(&dom, grow) {
        return Ok(Err(Error::Unsupported("an edge between two faces")));
    }
    // the one surface extended once, the distance met at the picked edge's face
    let made = pieces
        .surface
        .extended_by_distance_at(k, &pieces.whole, &dom, grow, how, distance);
    let (grown, mapped) = match extension(made)? {
        Ok(made) => made,
        Err(e) => return Ok(Err(e)),
    };
    // cut again where the faces were, each new face in the order of the old
    let (mut r, made) = sheet_of(k, grown, &pieces.u_cuts, &pieces.v_cuts, b.tolerance())?;
    if made.len() != pieces.faces.len() {
        return Ok(Err(Error::Unsupported(
            "the surface cut again is not as many faces",
        )));
    }
    for (&(old, old_dom), &(new, new_dom)) in pieces.faces.iter().zip(&made) {
        let (Some(old_surf), Some(new_surf)) = (
            face_p2::surface(b, old)?.cloned(),
            face_p2::surface(&r, new)?.cloned(),
        ) else {
            return Ok(Err(Error::Unsupported("a face with no surface")));
        };
        copy_attributes(b, EntityRef::Face(old), &mut r, EntityRef::Face(new))?;
        let old_edges = face_p4::edges(b, old, None, false, None)?;
        let new_edges = face_p4::edges(&r, new, None, false, None)?;
        let was = Sides {
            b,
            surf: &old_surf,
            dom: &old_dom,
            edges: &old_edges,
            on,
        };
        carry_sides(k, &was, &mut r, (&new_surf, &new_dom, &new_edges), mapped)?;
        // each new face faces the way the old one did
        if let (Some(was), Some(now)) = (upward(b, old)?, upward(&r, new)?) {
            if was != now {
                face_p1::flip_face_orientation(&mut r, new)?;
            }
        }
    }
    finish(k, r, Expect::FACES)
}

#[cfg(test)]
// The tests build their sheets and closed forms (placements, pi); ops itself computes none.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::surface::cone::Cone;
    use solidcore::surface::surface::SurfaceKind;

    /// The quarter cylinder sheet r 10, 20 high (the rig's `cov-extend-quarter-cylinder-along-
    /// its-axis`), and its order (the kernel's own lists).
    fn quarter() -> (Body, Order) {
        let mut k = Kernel::default();
        let pl = Axis2Placement::new(Vec3::ZERO, Vec3::X, Vec3::Y);
        let mut c = Cone::create_canonical(&k, &pl, 10.0, 10.0, 20.0, false, false).unwrap();
        c.adjust_step_uv_domain(&k, &Extent2d::from_bounds(0.0, 0.0, 90.0, 20.0))
            .unwrap();
        let (b, _) = sheet_on(&mut k, Surface::from(c), 1e-6).unwrap();
        let order = Order {
            faces: b.face_list().to_vec(),
            edges: b.edge_list().to_vec(),
            vertices: b.vertex_list().to_vec(),
        };
        (Body::new(b), order)
    }

    /// The record index of the edge through `p`.
    fn edge_at(body: &Body, order: &Order, p: [f64; 3]) -> usize {
        let k = Kernel::default();
        let b = body.lock();
        let p = solidcore::geom::vector3d::Point3::new(p[0], p[1], p[2]);
        order
            .edges
            .iter()
            .position(|&e| edge_distance(&k, &b, e, p) < 1e-9)
            .unwrap()
    }

    fn step_box(body: &Body) -> (SurfaceKind, Extent2d) {
        let k = Kernel::default();
        let b = body.lock();
        let f = b.face_list()[0];
        let s = face_p2::surface(&b, f).unwrap().unwrap();
        (s.kind().clone(), s.step_uv_domain_with_nurb(&k).unwrap())
    }

    #[test]
    fn the_quarter_cylinder_continues_along_its_axis_as_itself() {
        let (sheet, order) = quarter();
        // the top arc, through (10 cos 10, 10 sin 10, 20)
        let top = edge_at(&sheet, &order, [9.84807753012208, 1.7364817766693033, 20.0]);
        let out = extend_sheet(&sheet, &order, top, 5.0, Continuation::Natural).unwrap();
        let (kind, d) = step_box(&out);
        assert!(matches!(kind, SurfaceKind::Cone(_)));
        assert!(
            (d.min().y - 0.0).abs() < 1e-12 && (d.max().y - 25.0).abs() < 1e-12,
            "{d:?}"
        );
        assert!((d.max().x - d.min().x - 90.0).abs() < 1e-12);
        // the input is unchanged
        assert!((step_box(&sheet).1.max().y - 20.0).abs() < 1e-12);
    }

    #[test]
    fn round_the_axis_tangent_is_the_mirrored_spline() {
        let (sheet, order) = quarter();
        // the straight side at angle 90, through (0, 10, 10)
        let side = edge_at(&sheet, &order, [0.0, 10.0, 10.0]);
        let out = extend_sheet(&sheet, &order, side, 3.0, Continuation::Tangent).unwrap();
        assert!(matches!(step_box(&out).0, SurfaceKind::BSpline(_)));
    }

    /// A body whose edge list is not in id order (as an importer may leave it): the record's
    /// edge is the sheet's own, not one of a renumbered copy (REVIEW2 finding 1).
    #[test]
    fn an_edge_list_out_of_id_order_extends_the_side_asked() {
        let (sheet, _) = quarter();
        {
            let mut b = sheet.lock();
            let mut edges = b.edge_list().to_vec();
            edges.reverse();
            b.set_edge_list(edges).unwrap();
        }
        let order = {
            let b = sheet.lock();
            Order {
                faces: b.face_list().to_vec(),
                edges: b.edge_list().to_vec(),
                vertices: b.vertex_list().to_vec(),
            }
        };
        let top = edge_at(&sheet, &order, [9.84807753012208, 1.7364817766693033, 20.0]);
        let out = extend_sheet(&sheet, &order, top, 5.0, Continuation::Natural).unwrap();
        let (_, d) = step_box(&out);
        assert!(
            (d.max().y - 25.0).abs() < 1e-12 && d.min().y.abs() < 1e-12,
            "{d:?}"
        );
        assert!((d.max().x - d.min().x - 90.0).abs() < 1e-12, "{d:?}");
    }

    /// The corpus's `cov-extend-quarter-cylinder-along-its-axis`: the record's quarter cylinder
    /// sheet, its top arc (record edge 3) extended naturally by 5: the cylinder continued to 25
    /// high, (pi / 2) 10 25 = 392.70, one face with four edges, as the rig measured.
    #[test]
    fn the_corpus_quarter_cylinder_continues_as_the_rig_measured() {
        use crate::test_corpus::{area, entry, operand, raw_counts, rel, rig, rig_fixes, valid};
        let case = "surfaces/cov-extend-quarter-cylinder-along-its-axis";
        rig_fixes();
        let e = entry(case);
        let call = e["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["op"] == "extend_sheet")
            .unwrap();
        let p = &call["params"];
        let edge = usize::try_from(p["edge"]["picked"][0]["index"].as_u64().unwrap()).unwrap();
        assert_eq!(p["how"], "natural");
        let (sheet, order) = operand(case, "b2");
        let out = extend_sheet(
            &sheet,
            &order,
            edge,
            p["distance"].as_f64().unwrap(),
            Continuation::Natural,
        )
        .unwrap();
        valid(case, &out);
        let (a, m) = (area(&out), rig(&e));
        println!(
            "{case}: area {a} (rig {}), counts {:?}",
            m.area,
            out.counts().unwrap()
        );
        assert!(rel(a, m.area) < 1e-9, "area {a} vs rig {}", m.area);
        assert!(rel(a, 125.0 * std::f64::consts::PI) < 1e-9, "area {a}");
        assert_eq!(out.counts().unwrap(), raw_counts(&e));
        assert!(matches!(step_box(&out).0, SurfaceKind::Cone(_)));
    }

    /// A distance below the side's resolution grows nothing: refused as no effect, not
    /// returned as if extended.
    #[test]
    fn a_rounding_sized_distance_is_no_effect() {
        let (sheet, order) = quarter();
        let top = edge_at(&sheet, &order, [9.84807753012208, 1.7364817766693033, 20.0]);
        for d in [3.5691962659704166e-16, f64::MIN_POSITIVE] {
            assert_eq!(
                extend_sheet(&sheet, &order, top, d, Continuation::Natural).err(),
                Some(Error::NoEffect),
                "{d:e}"
            );
        }
    }

    #[test]
    fn bad_asks_are_refused() {
        let (sheet, order) = quarter();
        for d in [0.0, -1.0, f64::NAN] {
            assert_eq!(
                extend_sheet(&sheet, &order, 0, d, Continuation::Natural).err(),
                Some(Error::Argument)
            );
        }
        assert_eq!(
            extend_sheet(&sheet, &order, 99, 1.0, Continuation::Natural).err(),
            Some(Error::Argument)
        );
    }
}
