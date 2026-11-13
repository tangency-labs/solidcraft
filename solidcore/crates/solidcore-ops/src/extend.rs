//! Extend: a one-face sheet continued past the side of its surface's box that one of its edges
//! runs along, by a distance measured across that side, as the kernel's exact extension
//! ([`Surface::extended_by_distance`]) makes it. Ops chooses the side the edge is, checks the
//! sheet is its surface's whole box, sequences the kernel's calls and carries the face's and
//! sides' attributes onto the new sheet; it computes nothing.
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
use solidcore::surface::extend::{Continuity, ExtendRefusal};
use solidcore::surface::side::BoxSide;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::sheet::sheet_on;
use solidcore::topology::brep::trim_extend::extend_trim;
use solidcore::topology::face::{p1 as face_p1, p2 as face_p2, p4 as face_p4};
use solidcore::topology::{Brep, EdgeId};
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

/// The one-face sheet `sheet` continued `distance` past the side of its surface's box that
/// record edge `edge` runs along (all of it), as `how` says: a new sheet on the extended surface,
/// its face and sides carrying the attributes of the face and sides they continue, facing the
/// way the old face did. `sheet` is unchanged. A face on a surface that goes on past its trims (a
/// plane) is extended by growing its trim ([`extend_trim`]: the edge, on its outer loop, moved
/// `distance` outward, its neighbours carried along their tangents), the same for every `how`.
///
/// # Errors
/// - [`Error::Argument`]: a distance not finite and positive, an edge past the record's list or
///   not the face's (on a plane, not on its outer loop);
/// - [`Error::Unsupported`] (with what): not one face with one loop that is its surface's whole box, the edge
///   not all of one side, a side whose collapse is undecided, a surface with no exact extension;
///   on a plane, the trim refused ([`extend_trim`]: a neighbour that never meets the moved
///   edge, neighbours that cross, a moved edge crossing the others);
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
        let [f] = faces.as_slice() else {
            return Ok(Err(Error::Unsupported("a sheet of more than one face")));
        };
        let f = *f;
        let Some(surf) = face_p2::surface(b, f)?.cloned() else {
            return Ok(Err(Error::Unsupported("a face with no surface")));
        };
        let edges = face_p4::edges(b, f, None, false, None)?;
        if !edges.contains(&picked) {
            return Ok(Err(Error::Argument));
        }
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
        #[allow(
            clippy::float_arithmetic,
            reason = "a tolerance ops chooses: ten of the body's"
        )]
        let on = 10.0 * b.tolerance().max(1e-9);
        let dom = face_p2::uv_domain(b, f)?;
        // the face is its surface's whole box, as the kernel's extension decides it
        if !surf.is_whole_box(k, &dom)? {
            return Ok(Err(Error::Unsupported(
                "a face that is not its surface's whole box",
            )));
        }
        let mut grow = None;
        for s in BoxSide::ALL {
            match collapsed(k, &surf, &dom, s, on)? {
                Err(e) => return Ok(Err(e)),
                Ok(true) => continue,
                Ok(false) => {}
            }
            if !on_edges(k, b, &surf, &dom, s, &edges, on)? {
                return Ok(Err(Error::Unsupported(
                    "a side of the box that is not a side of the face",
                )));
            }
            if on_edges(k, b, &surf, &dom, s, &[picked], on)? {
                grow = Some(s);
            }
        }
        let Some(grow) = grow else {
            return Ok(Err(Error::Unsupported(
                "an edge that is not all of one side of the box",
            )));
        };
        // a distance below the side's resolution is no extension: the sheet would come back
        // as it is, which is not what was asked
        let (grown, mapped) = match surf.extended_by_distance(k, &dom, grow, how, distance) {
            Ok(Some(e)) => {
                let mapped = BoxSide::ALL.map(|s| e.side_of(s));
                (e.surface, mapped)
            }
            Ok(None) => return Ok(Err(Error::NoEffect)),
            Err(e) if e.is_unported() || e.is_cancelled() => return Err(e),
            Err(e) => return Ok(Err(refused(e))),
        };
        let grown_eval = grown.copy(k)?;
        let (mut r, nf) = sheet_on(k, grown, b.tolerance())?;
        // the face and each side stand for the face and side they continue
        copy_attributes(b, EntityRef::Face(f), &mut r, EntityRef::Face(nf))?;
        let made = face_p4::edges(&r, nf, None, false, None)?;
        #[allow(
            clippy::float_arithmetic,
            reason = "a tolerance ops chooses: ten of the body's"
        )]
        let new_on = 10.0 * r.tolerance().max(1e-9);
        let rdom = face_p2::uv_domain(&r, nf)?;
        for (s, to_s) in BoxSide::ALL.into_iter().zip(mapped) {
            // a collapsed side has no edge of its own
            if !matches!(collapsed(k, &surf, &dom, s, on)?, Ok(false)) {
                continue;
            }
            let from = side_edge(k, b, &surf, &dom, s, &edges, on)?;
            // the side the kernel says side s is on in the result's parameters
            let to = side_edge(k, &r, &grown_eval, &rdom, to_s, &made, new_on)?;
            if let (Some(from), Some(to)) = (from, to) {
                copy_attributes(b, EntityRef::Edge(from), &mut r, EntityRef::Edge(to))?;
            }
        }
        // the new face faces the way the old one did
        if let (Some(was), Some(now)) = (upward(b, f)?, upward(&r, nf)?) {
            if was != now {
                face_p1::flip_face_orientation(&mut r, nf)?;
            }
        }
        finish(k, r, Expect::FACES)
    })
    .and_then(|r| r.map(Body::new))
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
