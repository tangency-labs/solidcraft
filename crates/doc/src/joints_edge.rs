//! Joint origins on an edge of a planar face (`Snap::Edge`): the edge's middle, or a corner where
//! it meets another edge of the face. The edge, the face and the corner's other edge are found
//! again by name on every solve, so the origin follows the face as it grows, shrinks or moves:
//! hardware set 40 mm in from a panel's corner stays 40 mm in from that corner at any size.
//!
//! The frame never depends on which way an edge's polyline runs: z is the face's outward normal;
//! at the middle y points into the face, square to the edge, and x = y × z runs along it; at a
//! corner x and y run along the two edges, away from the corner into the face, in the order that
//! makes the frame right-handed.

use solidcraft_geom::Vec3;

use super::{EdgeAxis, Snap, component_bodies, frame};
use crate::{DocError, Document, IDENTITY, Mat, ModelBody, ModelState, Result, apply_point, mat_inverse, mat_mul};

/// A body's placement in its component and its inverse: a body moved in from another component
/// (an imported part's) keeps its geometry where it was made, and its offset places it. Edges and
/// faces are looked up in the body's own coordinates and the frame is placed back by the offset.
fn placement(doc: &Document, b: &ModelBody) -> (Mat, Mat) {
    let m = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
    (m, mat_inverse(&m).unwrap_or(IDENTITY))
}

/// Points that close are one corner (as a share of the body's size, at least 1 µm).
fn tol(b: &ModelBody) -> f64 {
    (b.body.size() * 2e-3).max(1e-3)
}

/// The edge of `b` nearest `p`, and how far it is.
fn nearest_edge(b: &ModelBody, p: Vec3) -> Option<(usize, f64)> {
    b.mesh()
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min)))
        .filter(|(_, d)| d.is_finite())
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Edge `i` of `b`: by its name (`None` when the body has no such edge), else the edge at `pick`.
fn edge_index(b: &ModelBody, name: Option<&str>, pick: Vec3) -> Option<usize> {
    match name {
        Some(n) => crate::name_lookup::locate_edge(b, n, pick).ok().map(|h| h.edge),
        None => nearest_edge(b, pick).filter(|(_, d)| *d <= tol(b)).map(|(i, _)| i),
    }
}

/// The planar face beside edge `i` the origin sits on: the named one, else the larger planar face
/// the edge bounds. Its index, outward normal and centroid.
fn face_of(b: &ModelBody, i: usize, name: Option<&str>, pick: Vec3) -> Result<(usize, Vec3, Vec3)> {
    let m = b.mesh();
    let beside: Vec<usize> = m.edge_faces.get(i).map(|fs| fs.iter().map(|f| *f as usize).collect()).unwrap_or_default();
    let infos = b.joint_faces()?;
    let planar = |f: usize| infos.iter().find(|x| x.index == f).and_then(|x| x.plane_normal.map(|n| (x.index, n, x.centroid, x.area)));
    let chosen = match name {
        Some(n) => {
            let (f, _, _) = crate::naming::face_on_body(b, n, pick)
                .ok_or_else(|| DocError::Invalid(format!("joint origin: the face `{n}` no longer exists (edit the joint to pick again)")))?;
            if !beside.contains(&f) {
                return Err(DocError::Invalid(format!("joint origin: the face `{n}` no longer meets its edge (edit the joint to pick again)")));
            }
            planar(f)
        }
        None => beside.iter().filter_map(|f| planar(*f)).max_by(|a, b| a.3.total_cmp(&b.3)),
    };
    chosen.map(|(f, n, c, _)| (f, n, c)).ok_or_else(|| DocError::Invalid("joint origin: no planar face beside that edge".into()))
}

/// Edge `i`'s middle (along its polyline) and the direction it runs there.
fn middle(e: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let total: f64 = e.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in e.windows(2) {
        let l = w[0].dist(w[1]);
        if l > 0.0 && acc + l >= total / 2.0 {
            let t = (total / 2.0 - acc) / l;
            return Some((w[0] + (w[1] - w[0]) * t, (w[1] - w[0]) / l));
        }
        acc += l;
    }
    None
}

/// An edge's end at `p` (within `tol`) and the direction it leaves the end in.
fn leaving(e: &[Vec3], p: Vec3, tol: f64) -> Option<Vec3> {
    let (first, last) = (*e.first()?, *e.last()?);
    let away = |q: Option<&Vec3>, from: Vec3| q.and_then(|q| (*q - from).normalized());
    if first.dist(p) <= tol {
        away(e.iter().find(|q| q.dist(first) > tol * 0.5), first)
    } else if last.dist(p) <= tol {
        away(e.iter().rev().find(|q| q.dist(last) > tol * 0.5), last)
    } else {
        None
    }
}

/// The corner where edges `e` and `f` meet: the closest pair of their ends, when within `tol`.
fn meeting(e: &[Vec3], f: &[Vec3], tol: f64) -> Option<Vec3> {
    let ends = |x: &[Vec3]| [x.first().copied(), x.last().copied()];
    ends(e)
        .into_iter()
        .flatten()
        .flat_map(|a| ends(f).into_iter().flatten().map(move |b| (a, a.dist(b))))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(a, _)| a)
}

/// The frame of an edge snap in its component's coordinates.
pub(super) fn edge_frame(doc: &Document, st: &ModelState, comp: u64, snap: &Snap) -> Result<Mat> {
    let Snap::Edge { pick, edge, face, corner, axis } = snap else { return Err(DocError::Invalid("not an edge origin".into())) };
    let bad = || DocError::Invalid("joint origin: degenerate frame".into());
    let gone = |what: &str| DocError::Invalid(format!("joint origin: {what} no longer exists (edit the joint to pick again)"));
    let found = component_bodies(doc, st, comp).into_iter().find_map(|b| {
        let (m, inv) = placement(doc, b);
        let local = apply_point(&inv, *pick);
        edge_index(b, edge.as_deref(), local).map(|i| (b, i, local, m))
    });
    let (b, i, pick, placed) = found.ok_or_else(|| match edge {
        Some(n) => gone(&format!("the edge `{n}`")),
        None => DocError::Invalid("joint origin: no edge there".into()),
    })?;
    let m = b.mesh();
    let e = m.edges.get(i).ok_or_else(bad)?;
    let (_, z, centroid) = face_of(b, i, face.as_deref(), pick)?;
    let flat = |v: Vec3| (v - z * v.dot(z)).normalized();
    let local = match corner {
        None => {
            let (mid, along) = middle(e).ok_or_else(bad)?;
            // Into the face: toward its centroid, square to the edge; a face whose centroid lies on
            // the edge's line (only a sliver) falls back to the side z × along gives.
            let to_c = centroid - mid;
            let y = flat(to_c - along * to_c.dot(along)).unwrap_or_else(|| z.cross(along));
            frame(mid, z, Some(y.cross(z))).ok_or_else(bad)
        }
        Some(other) => {
            let j = crate::name_lookup::locate_edge(b, other, pick).map_err(|_| gone(&format!("the edge `{other}`")))?.edge;
            let f = m.edges.get(j).ok_or_else(bad)?;
            let t = tol(b);
            let at = meeting(e, f, t).ok_or_else(|| DocError::Invalid(format!("joint origin: its edges no longer meet at a corner (`{other}`)")))?;
            let (de, df) = (leaving(e, at, t).and_then(flat).ok_or_else(bad)?, leaving(f, at, t).and_then(flat).ok_or_else(bad)?);
            let x = if z.dot(de.cross(df)) >= 0.0 { de } else { df };
            frame(at, z, Some(x)).ok_or_else(bad)
        }
    }?;
    // A z of its own: x from z alone, at the point the edge gives.
    let local = match axis {
        None => local,
        Some(a) => {
            let at = Vec3::new(local[3][0], local[3][1], local[3][2]);
            let (_, inv) = placement(doc, b);
            let want = crate::apply_vector(&inv, a.dir());
            let z = match a {
                EdgeAxis::Fixed(_) => want,
                // The edge's run, the way `want` points.
                EdgeAxis::Along(_) => {
                    let run = match (e.first(), e.last()) {
                        (Some(p), Some(q)) => (*q - *p).normalized().ok_or_else(bad)?,
                        _ => return Err(bad()),
                    };
                    if run.dot(want) < 0.0 { -run } else { run }
                }
            };
            frame(at, z, None).ok_or_else(bad)?
        }
    };
    Ok(mat_mul(&placed, &local))
}

/// An edge snap for an edge picked at `pick` on component `comp` (its own coordinates): on the
/// planar face holding `on` (default: the larger planar face beside the edge), at the edge's
/// middle or, with `corner`, at the end nearest it. The edge, the face and the corner's other
/// edge are kept by name.
pub fn edge_snap(doc: &Document, st: &ModelState, comp: u64, pick: Vec3, on: Option<Vec3>, corner: Option<Vec3>) -> Result<Snap> {
    let (b, i, inv) = component_bodies(doc, st, comp)
        .into_iter()
        .filter_map(|b| {
            let (_, inv) = placement(doc, b);
            nearest_edge(b, apply_point(&inv, pick)).filter(|(_, d)| *d <= tol(b)).map(|(i, _)| (b, i, inv))
        })
        .next()
        .ok_or_else(|| DocError::Invalid("joint origin: no edge there".into()))?;
    // The picks in the body's own coordinates; the snap keeps the pick as given.
    let (local, on, corner) = (apply_point(&inv, pick), on.map(|p| apply_point(&inv, p)), corner.map(|c| apply_point(&inv, c)));
    let names = crate::naming::edge_names(b);
    let face_names = crate::naming::face_names(b);
    let m = b.mesh();
    let beside: Vec<usize> = m.edge_faces.get(i).map(|fs| fs.iter().map(|f| *f as usize).collect()).unwrap_or_default();
    let face_index = match on {
        Some(p) => {
            let f = crate::naming::face_name_at_body(b, p)
                .and_then(|n| crate::naming::face_on_body(b, &n, p))
                .map(|(f, _, _)| f)
                .ok_or_else(|| DocError::Invalid("joint origin: `on` is not on a face of that body".into()))?;
            if !beside.contains(&f) {
                return Err(DocError::Invalid("joint origin: `on` picks a face the edge doesn't bound".into()));
            }
            f
        }
        None => face_of(b, i, None, local)?.0,
    };
    let face = face_names.get(face_index).cloned().filter(|n| !n.is_empty());
    let corner = match corner {
        None => None,
        Some(c) => {
            let e = m.edges.get(i).cloned().unwrap_or_default();
            let t = tol(b);
            let end = [e.first().copied(), e.last().copied()]
                .into_iter()
                .flatten()
                .min_by(|a, b| a.dist(c).total_cmp(&b.dist(c)))
                .ok_or_else(|| DocError::Invalid("joint origin: that edge has no ends".into()))?;
            // The other edge of the face that ends there.
            let other = m
                .edges
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i && m.edge_faces.get(*j).is_some_and(|fs| fs.contains(&(face_index as u32))))
                .find(|(_, f)| meeting(&[end], f, t).is_some())
                .map(|(j, _)| j)
                .ok_or_else(|| DocError::Invalid("joint origin: no other edge of the face meets that end (a round edge has no corner)".into()))?;
            Some(
                names
                    .get(other)
                    .cloned()
                    .filter(|n| !n.is_empty())
                    .ok_or_else(|| DocError::Invalid("joint origin: the corner's other edge has no name".into()))?,
            )
        }
    };
    Ok(Snap::Edge { pick, edge: names.get(i).cloned().filter(|n| !n.is_empty()), face, corner, axis: None })
}

/// Edge snap `snap` of component `comp` given z `z` (component coordinates) of its own: along the
/// edge when the edge runs that way (so a hinge on it keeps turning about it), else fixed.
pub fn edge_axis(doc: &Document, st: &ModelState, comp: u64, snap: Snap, z: Vec3) -> Snap {
    let Snap::Edge { pick, edge, face, corner, .. } = snap else { return snap };
    let with = |axis| Snap::Edge { pick, edge: edge.clone(), face: face.clone(), corner: corner.clone(), axis: Some(axis) };
    let along = with(EdgeAxis::Along(z));
    let runs =
        edge_frame(doc, st, comp, &along).is_ok_and(|m| z.normalized().is_some_and(|z| Vec3::new(m[2][0], m[2][1], m[2][2]).dot(z) > 1.0 - 1e-9));
    if runs { along } else { with(EdgeAxis::Fixed(z)) }
}
