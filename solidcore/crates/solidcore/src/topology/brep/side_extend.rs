//! A sheet's side extended across a curved face and the plane faces beside it, so the edges
//! they share stay one edge each ([`extended_side`]): an arch on straight legs, whose legs the
//! skin makes planes (`sweep::skin_flat_walls`) and whose top is a B-spline.
//!
//! Not upstream. The curved face is extended past the side as one face is
//! ([`Surface::extended_by_distance`]); each seam it shares with a plane beside it runs on, as
//! a straight row does under every continuation, to a new corner on the seam's own line; and
//! each plane's trim grows in its plane until its edge on that side passes through that corner
//! ([`crate::topology::brep::trim_extend`], `Move::Through`). The faces are built again in a
//! new body and stitched, so each seam is one edge again.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3};
use crate::query::api_queries::edge_closest_point;
use crate::surface::extend::Continuity;
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1::{edges as brep_edges, faces as brep_faces};
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p4::stitch_faces;
use crate::topology::brep::sheet::face_on;
use crate::topology::brep::trim_extend::{build_into, grown, Built, Move};
use crate::topology::brep::validation::{check_brep_with_keys, CheckLevel};
use crate::topology::face::p2 as fp2;
use crate::topology::face::p4::edges as face_edges;
use crate::topology::loopuse::notify_ref;
use crate::topology::{Brep, EdgeId, FaceId};

const OP: &str = "topology::brep::side_extend::extended_side";

/// The sheet with a side extended by [`extended_side`]: a new body, each old face and its new
/// one, and each old edge and the edge it is now (`None` for an edge with no counterpart).
#[derive(Debug)]
pub struct SideExtension {
    /// The new body.
    pub brep: Brep,
    /// Each face of the sheet and its face in `brep`.
    pub faces: Vec<(FaceId, FaceId)>,
    /// Each edge of the sheet and its edge in `brep` (a moved or lengthened edge's is the new
    /// one; a seam's the one the stitch kept).
    pub edges: Vec<(EdgeId, Option<EdgeId>)>,
}

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// How near a point must be to an edge to be on it: ten of the body's tolerance.
fn near(b: &Brep) -> f64 {
    10.0 * b.tolerance().max(1e-9)
}

/// Whether `p` lies on edge `e` of `b` within `on`.
fn on_edge(k: &Kernel, b: &Brep, e: EdgeId, p: Point3, on: f64) -> bool {
    edge_closest_point(k, b, e, p).is_ok_and(|c| c.closest.distance <= on)
}

/// The faces edge `e` bounds, each once.
fn faces_of(b: &Brep, e: EdgeId) -> Result<Vec<FaceId>> {
    let mut out = Vec::new();
    for eu in crate::topology::edge::edgeuses(b, e, None, None)? {
        if let Some(f) = crate::topology::edgeuse::p1::face(b, eu)? {
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    Ok(out)
}

/// Whether face `f` is on a surface that reaches past its trims (a plane).
fn is_plane(b: &Brep, f: FaceId) -> Result<bool> {
    Ok(fp2::surface(b, f)?.is_some_and(Surface::reaches_past_trims))
}

/// The edge of `edges` whose middle side `t` of `dom` on `surf` passes through.
fn side_edge(
    k: &Kernel,
    b: &Brep,
    surf: &Surface,
    dom: &Extent2d,
    t: BoxSide,
    edges: &[EdgeId],
    on: f64,
) -> Result<Option<EdgeId>> {
    let m = surf.evaluate_point(k, t.point(dom, 0.5))?;
    Ok(edges.iter().copied().find(|&e| on_edge(k, b, e, m, on)))
}

/// The corner of `dom` where side `s` meets side `t` (one of the sides across it).
fn corner(dom: &Extent2d, s: BoxSide, t: BoxSide) -> Point2 {
    if s.is_u() {
        Point2::new(s.at(dom), t.at(dom))
    } else {
        Point2::new(t.at(dom), s.at(dom))
    }
}

/// The two sides at the ends of side `s`.
fn across(s: BoxSide) -> [BoxSide; 2] {
    if s.is_u() {
        [BoxSide::VMin, BoxSide::VMax]
    } else {
        [BoxSide::UMin, BoxSide::UMax]
    }
}

/// The ends of edge `e`.
fn ends(b: &Brep, e: EdgeId) -> Result<[Point3; 2]> {
    let [a, z] = b.edge(e)?.vertices;
    Ok([b.vertex(a)?.point, b.vertex(z)?.point])
}

/// The point at the middle of edge `e`'s interval.
fn middle(k: &Kernel, b: &Brep, e: EdgeId) -> Result<Point3> {
    let r = b.edge(e)?;
    let c = r
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge without a curve"))?;
    c.evaluate_point(k, r.interval.mid())
}

/// The curved face and its edge on the side to grow, for edge `e` of face `f` picked: `f`
/// itself where it is curved; for a plane, the curved face across the seam that shares a
/// vertex with `e`, and its edge at that vertex.
fn curved_of(b: &Brep, f: FaceId, e: EdgeId, on: f64) -> Result<(FaceId, EdgeId)> {
    if !is_plane(b, f)? {
        return Ok((f, e));
    }
    let picked = ends(b, e)?;
    let shares = |q: [Point3; 2]| {
        q.iter()
            .any(|&x| picked.iter().any(|&y| (x - y).length() <= on))
    };
    let mut found = None;
    for seam in face_edges(b, f, None, false, None)? {
        if seam == e {
            continue;
        }
        for g in faces_of(b, seam)? {
            if g == f || is_plane(b, g)? || !shares(ends(b, seam)?) {
                continue;
            }
            if found.is_some() {
                return Err(unsupported(
                    "a plane face's side that runs on into two curved faces",
                ));
            }
            // the shared vertex: the end of the seam on the picked edge
            let at = ends(b, seam)?
                .into_iter()
                .find(|&x| picked.iter().any(|&y| (x - y).length() <= on))
                .ok_or_else(|| unsupported("a seam without the picked edge's vertex"))?;
            found = Some((g, seam, at));
        }
    }
    let (g, seam, at) = found.ok_or_else(|| {
        unsupported("a side of a plane face that does not run on into a curved face")
    })?;
    // the curved face's edge at that vertex, other than the seam
    let mut edge = None;
    for c in face_edges(b, g, None, false, None)? {
        if c != seam && ends(b, c)?.iter().any(|&x| (x - at).length() <= on) {
            if edge.is_some() {
                return Err(unsupported("a corner of the curved face with three edges"));
            }
            edge = Some(c);
        }
    }
    let edge = edge.ok_or_else(|| unsupported("a curved face without its side at the seam"))?;
    Ok((g, edge))
}

/// The sheet `b` with the side its edge `e` (of face `f`) runs along extended `distance` as
/// `how` says, across one curved face and the plane faces beside it, the seams they share kept
/// one edge each (module docs): `None` where the distance is below the side's resolution (no
/// extension), as [`Surface::extended_by_distance`]. `b` is unchanged.
///
/// The edge may be the curved face's (all of one side of its whole box) or a plane's beside it
/// (the edge that meets the seam). Either way the distance is met on the curved face, at the
/// middle of its side, as a one-face extension meets it, and the planes follow: each grows
/// until its edge on that side passes through the corner the curved face's seam has run on to.
/// Where the walls stand square to the section (an arch's walls ruled straight across) every
/// corner moves by exactly the distance. On a tapered wall a leg moves by its seam's own
/// extension, which may differ a little from the distance met at the curved face's middle.
///
/// # Errors
///
/// - `InvalidInput`: a distance not finite and positive; `e` not an edge of `f`;
/// - `Unsupported`: a plane's edge that does not run on into a curved face; a curved face that
///   is not a B-spline on its whole box with one loop; an edge that is not all of one side; a
///   seam beside the curved face shared with another curved face (more than one curved face,
///   not supported yet), or with a plane already met (a ring); faces beyond the curved face and
///   the planes beside it (not supported yet); a plane's edge on the side that is not straight
///   or that its carried neighbour does not take through the corner
///   (`trim_extend::extend_trim_through`'s refusals); a result that is not valid;
/// - those of the extension, the trim, the face constructors and the stitch.
pub fn extended_side(
    k: &mut Kernel,
    b: &Brep,
    f: FaceId,
    e: EdgeId,
    how: Continuity,
    distance: f64,
) -> Result<Option<SideExtension>> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a distance not finite and positive",
        ));
    }
    if !face_edges(b, f, None, false, None)?.contains(&e) {
        return Err(SolidError::invalid_input(OP, "an edge not of the face"));
    }
    let on = near(b);
    let (c, ce) = curved_of(b, f, e, on)?;

    // the curved face: a B-spline on its whole box, one loop, and the side its edge runs along
    let surf = fp2::surface(b, c)?
        .cloned()
        .ok_or_else(|| SolidError::invalid_input(OP, "a face without a surface"))?;
    if !matches!(surf.kind(), SurfaceKind::BSpline(_)) {
        return Err(unsupported(
            "a curved face that is not a B-spline (not supported yet)",
        ));
    }
    let dom = fp2::uv_domain(b, c)?;
    if !surf.is_whole_box(k, &dom)? || fp2::loops(b, c, false, None)?.len() != 1 {
        return Err(unsupported(
            "a curved face that is not its surface's whole box with one loop",
        ));
    }
    let c_edges = face_edges(b, c, None, false, None)?;
    let mut grow = None;
    for s in BoxSide::ALL {
        let mut all = true;
        for t in [0.0, 0.5, 1.0] {
            let p = surf.evaluate_point(k, s.point(&dom, t))?;
            all = all && on_edge(k, b, ce, p, on);
        }
        if all {
            grow = Some(s);
            break;
        }
    }
    let s = grow.ok_or_else(|| unsupported("an edge that is not all of one side"))?;

    // the planes beside it, across the side's ends: each with its seam and its edge on the side
    let mut legs: Vec<(BoxSide, EdgeId, FaceId, EdgeId)> = Vec::new();
    for t in across(s) {
        let Some(seam) = side_edge(k, b, &surf, &dom, t, &c_edges, on)? else {
            continue;
        };
        let others: Vec<FaceId> = faces_of(b, seam)?.into_iter().filter(|&g| g != c).collect();
        let leg = match others.as_slice() {
            [] => continue,
            [g] => *g,
            _ => return Err(unsupported("an edge of more than two faces")),
        };
        if !is_plane(b, leg)? {
            return Err(unsupported(
                "a seam shared with another curved face (not supported yet)",
            ));
        }
        if legs.iter().any(|&(_, _, l, _)| l == leg) {
            return Err(unsupported("a ring of faces round the side"));
        }
        // its edge on the grown side: the one, besides the seam, at the old corner
        let was = surf.evaluate_point(k, corner(&dom, s, t))?;
        let mut on_side = None;
        for le in face_edges(b, leg, None, false, None)? {
            if le != seam && ends(b, le)?.iter().any(|&x| (x - was).length() <= on) {
                if on_side.is_some() {
                    return Err(unsupported("a plane's corner at the seam with three edges"));
                }
                on_side = Some(le);
            }
        }
        let le = on_side.ok_or_else(|| unsupported("a plane without its edge at the corner"))?;
        legs.push((t, seam, leg, le));
    }
    let all_faces = brep_faces(b, None)?;
    if all_faces.len() != 1 + legs.len()
        || !all_faces
            .iter()
            .all(|&g| g == c || legs.iter().any(|&(_, _, l, _)| l == g))
    {
        return Err(unsupported(
            "faces beyond the curved face and the planes beside it (not supported yet)",
        ));
    }

    // the curved face extended, and each plane's trim grown through its seam's new corner
    let Some(ext) = surf.extended_by_distance(k, &dom, s, how, distance)? else {
        return Ok(None);
    };
    if ext.sides != BoxSide::ALL {
        return Err(unsupported(
            "an extension that turned the face's parameters",
        ));
    }
    let grown_surf = ext.surface;
    let new_dom = grown_surf.natural_uv_domain_with_nurb(k)?;
    let mut grown_legs = Vec::new();
    for &(t, _, leg, le) in &legs {
        let q = grown_surf.evaluate_point(k, corner(&new_dom, s, t))?;
        grown_legs.push((leg, grown(k, b, leg, le, Move::Through(q))?));
    }

    // the new body: the curved face, then the planes
    let mut nb = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    nb.set_tolerance(b.tolerance());
    let grown_eval = grown_surf.copy(k)?;
    let nc = face_on(k, &mut nb, grown_surf, &new_dom)?;
    nb.refresh_cached_topology();
    let nb_on = near(&nb);
    let nc_edges = face_edges(&nb, nc, None, false, None)?;
    // each old edge and its new edge before the stitch: the curved face's side by side
    let mut pre: Vec<(EdgeId, EdgeId)> = Vec::new();
    for t in BoxSide::ALL {
        if let (Some(old), Some(new)) = (
            side_edge(k, b, &surf, &dom, t, &c_edges, on)?,
            side_edge(k, &nb, &grown_eval, &new_dom, t, &nc_edges, nb_on)?,
        ) {
            pre.push((old, new));
        }
    }
    let mut face_map = vec![(c, nc)];
    for (leg, g) in grown_legs {
        let Built {
            face: nf, edges, ..
        } = build_into(k, &mut nb, g)?;
        face_map.push((leg, nf));
        pre.extend(edges);
    }
    // each new edge's middle, before the stitch keeps one edge of each seam
    let mut middles: Vec<(EdgeId, EdgeId, Point3)> = Vec::new();
    for (old, new) in pre {
        if middles.iter().any(|&(o, _, _)| o == old) {
            continue;
        }
        middles.push((old, new, middle(k, &nb, new)?));
    }
    let _stitched = stitch_faces(k, &mut nb, b.tolerance(), None, false)?;
    nb.refresh_cached_topology();
    let check = check_brep_with_keys(k, &mut nb, CheckLevel::Level2, &notify_ref)?;
    if !check.is_valid() {
        return Err(unsupported("the extended sheet is not valid"));
    }

    // each old edge: its new edge where it survived the stitch, else the edge through its middle
    let live = brep_edges(&nb, None)?;
    let mut edge_map = Vec::new();
    for old in brep_edges(b, None)? {
        let now = match middles.iter().find(|&&(o, _, _)| o == old) {
            Some(&(_, new, _)) if live.contains(&new) => Some(new),
            Some(&(_, _, m)) => live.iter().copied().find(|&x| on_edge(k, &nb, x, m, nb_on)),
            None => None,
        };
        edge_map.push((old, now));
    }
    Ok(Some(SideExtension {
        brep: nb,
        faces: face_map,
        edges: edge_map,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::primitive::api_curves::{create_arc, create_line_segment};
    use crate::sweep::api_surfaces::create_skin_primitive;

    const W: f64 = std::f64::consts::FRAC_1_SQRT_2;

    /// The arch on legs: at z = 0 and z = 10 the leg from (5, -5) up to (5, 0), the half circle
    /// of radius 5 about the origin over to (-5, 0), the leg down to (-5, -5); skinned, its legs
    /// planes and its top a B-spline.
    fn legs_arch(k: &mut Kernel) -> Brep {
        let section = |z: f64| -> Vec<Curve> {
            let p = |x: f64, y: f64| Point3::new(x, y, z);
            vec![
                Curve::from(create_line_segment(p(5.0, -5.0), p(5.0, 0.0)).unwrap()),
                Curve::from(create_arc(p(0.0, 0.0), 5.0, 0.0, 180.0).unwrap()),
                Curve::from(create_line_segment(p(-5.0, 0.0), p(-5.0, -5.0)).unwrap()),
            ]
        };
        let curves: Vec<Curve> = section(0.0).into_iter().chain(section(10.0)).collect();
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        create_skin_primitive(k, &[3, 3], &refs, 1, false)
            .unwrap()
            .brep
    }

    /// The edge of `b` through `p`.
    fn edge_through(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
        let p = Point3::new(p[0], p[1], p[2]);
        brep_edges(b, None)
            .unwrap()
            .into_iter()
            .find(|&e| on_edge(k, b, e, p, 1e-9))
            .unwrap_or_else(|| panic!("no edge through {p:?}"))
    }

    fn counts(b: &Brep) -> [usize; 3] {
        [
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len(),
        ]
    }

    fn planes(b: &Brep) -> usize {
        brep_faces(b, None)
            .unwrap()
            .into_iter()
            .filter(|&f| is_plane(b, f).unwrap())
            .count()
    }

    #[test]
    fn the_legs_arch_is_two_planes_and_a_curved_top() {
        let mut k = Kernel::default();
        let b = legs_arch(&mut k);
        // three faces, two seams: 3 bottom, 3 top, 2 outer sides, 2 seams; 8 vertices
        assert_eq!(counts(&b), [3, 10, 8]);
        assert_eq!(planes(&b), 2);
        edge_through(&k, &b, [5.0, 0.0, 5.0]);
        edge_through(&k, &b, [-5.0, 0.0, 5.0]);
    }

    /// Extended 5 at its top from the arc's top edge or from a leg's, the whole top rises to
    /// z = 15: the legs stay planes, each seam one edge, and the bottom stays.
    #[test]
    fn the_whole_top_rises_from_the_arc_or_a_leg() {
        for through in [[5.0 * W, 5.0 * W, 10.0], [5.0, -2.5, 10.0]] {
            let mut k = Kernel::default();
            let b = legs_arch(&mut k);
            let e = edge_through(&k, &b, through);
            let f = faces_of(&b, e).unwrap()[0];
            let out = extended_side(&mut k, &b, f, e, Continuity::Natural, 5.0)
                .unwrap_or_else(|err| panic!("{through:?}: {err:?}"))
                .unwrap_or_else(|| panic!("{through:?}: no extension"));
            let nb = &out.brep;
            assert_eq!(counts(nb), [3, 10, 8], "{through:?}");
            assert_eq!(planes(nb), 2, "{through:?}");
            // the vertices at the bottom or at the new top, none left at the old
            for &v in nb.vertex_list() {
                let z = nb.vertex(v).unwrap().point.z;
                assert!(
                    z.abs() < 1e-6 || (z - 15.0).abs() < 1e-6,
                    "{through:?}: z {z}"
                );
            }
            for p in [
                [5.0, 0.0, 12.5],
                [-5.0, 0.0, 12.5],
                [5.0, -2.5, 15.0],
                [5.0 * W, 5.0 * W, 15.0],
                [-5.0, -2.5, 15.0],
                [5.0, -2.5, 0.0],
            ] {
                edge_through(&k, nb, p);
            }
            // every face mapped; the old top of the arc is the new one
            assert_eq!(out.faces.len(), 3);
            let arc_top = edge_through(&k, &b, [5.0 * W, 5.0 * W, 10.0]);
            let new_top = edge_through(&k, nb, [5.0 * W, 5.0 * W, 15.0]);
            assert_eq!(
                out.edges
                    .iter()
                    .find(|(o, _)| *o == arc_top)
                    .and_then(|(_, n)| *n),
                Some(new_top)
            );
        }
    }

    /// A leg's outer edge does not run on into the curved face: refused.
    #[test]
    fn a_legs_outer_edge_is_refused() {
        let mut k = Kernel::default();
        let b = legs_arch(&mut k);
        let e = edge_through(&k, &b, [5.0, -5.0, 5.0]);
        let f = faces_of(&b, e).unwrap()[0];
        assert!(matches!(
            extended_side(&mut k, &b, f, e, Continuity::Natural, 5.0),
            Err(SolidError::Unsupported { .. })
        ));
    }
}
