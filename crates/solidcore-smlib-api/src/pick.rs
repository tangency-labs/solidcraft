//! Choosing faces, edges and vertices by points, shared by the calls that take a
//! selection (blends, offsets, drafts): each point picks the entity nearest to it, which
//! must lie within the caller's tolerance and be nearer than any other by more than the
//! body's tolerance. Also the face queries those calls share.
//!
//! Source: tangency-labs/smlib `capi/src/smlib_pick.h` (`pick`, `pick_faces`, `pick_edges`,
//! `pick_vertices`, `face_distance`, `edge_distance`, `vertex_distance`) and
//! `capi/src/smlib_ops.cpp` (`outer_faceuse`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API (`SM_API` / SMLib call) |
//! |------|-------|
//! | `pick_faces`, `pick_edges`, `pick_vertices` (crate) | `smlib_pick::pick_faces`, `pick_edges`, `pick_vertices` (`GetFaces`, `GetEdges`, `GetVertices`) |
//! | `pick` (private) | `smlib_pick::pick` |
//! | `face_distance` (crate) | `smlib_pick::face_distance` (`SmApiFaceClosestPoint`: `SmApiSurfaceClosestPoint`, `SmApiFaceClassifyUV`, `SmApiEdgeClosestPoint`) |
//! | `edge_distance` (private) | `smlib_pick::edge_distance` (`SmApiEdgeClosestPoint`) |
//! | `outer_faceuse` (crate) | `outer_faceuse` |
//!
//! Deviations: a pick's result carries the index of the point that first picked each
//! entity for every kind (the C API's optional `first`); a failed enumeration of the
//! body's entities is the kernel's error (SMLib's lists cannot fail).

// The families that take a selection (blends, offsets, drafts) use these.
#![cfg_attr(not(test), allow(dead_code))]

use core::cmp::Ordering;

use solidcore::error::SolidError;
use solidcore::intersect::point_class::PointClassType;
use solidcore::math::Point3;
use solidcore::query::api_queries::{edge_closest_point, face_classify_uv, surface_closest_point};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1::{edges, faces, vertices};
use solidcore::topology::face::p2::upward_faceuse;
use solidcore::topology::face::p4::edges as face_edges;
use solidcore::topology::shell::shell_region;
use solidcore::topology::{Brep, EdgeId, FaceId, FaceuseId, VertexId};
use solidcore::Kernel;

use crate::session::Fail;
use crate::status::Status;

/// The entities a pick chose, each once, in the order first picked, and for each the
/// index of the point that first picked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picked<T> {
    pub(crate) items: Vec<T>,
    pub(crate) first: Vec<usize>,
}

/// Why a pick failed: a point matched no entity or several (`SMLIB_ERR_PICK_NONE`,
/// `SMLIB_ERR_PICK_AMBIGUOUS`, with the point's index for the C API's `out_failed`), or the
/// kernel failed listing the body's entities.
#[derive(Debug)]
pub(crate) enum PickFail {
    Point { status: Status, index: usize },
    Kernel(SolidError),
}

impl PickFail {
    /// The index of the point that matched none or several (`out_failed`).
    pub(crate) const fn index(&self) -> Option<usize> {
        match self {
            Self::Point { index, .. } => Some(*index),
            Self::Kernel(_) => None,
        }
    }
}

impl From<PickFail> for Fail {
    fn from(e: PickFail) -> Self {
        match e {
            PickFail::Point { status, .. } => Self::Status(status),
            PickFail::Kernel(e) => Self::Solid(e),
        }
    }
}

impl From<SolidError> for PickFail {
    fn from(e: SolidError) -> Self {
        Self::Kernel(e)
    }
}

/// `smlib_pick::pick`: the entity of `all` nearest each point by `dist` (`None`, or a
/// distance that is not finite, for one that does not answer). It must lie within `tol`,
/// and no other within `tol` may be nearer than it plus `model_tol`.
fn pick<T: Copy + PartialEq>(
    all: &[T],
    points: &[[f64; 3]],
    tol: f64,
    model_tol: f64,
    mut dist: impl FnMut(T, Point3) -> Option<f64>,
) -> Result<Picked<T>, PickFail> {
    let mut picked = Picked {
        items: Vec::new(),
        first: Vec::new(),
    };
    for (index, &[x, y, z]) in points.iter().enumerate() {
        let p = Point3::new(x, y, z);
        let mut best = None;
        let mut d0 = f64::INFINITY;
        let mut d1 = f64::INFINITY;
        for &e in all {
            let Some(d) = dist(e, p).filter(|d| d.is_finite()) else {
                continue;
            };
            if d < d0 {
                d1 = d0;
                d0 = d;
                best = Some(e);
            } else if d < d1 {
                d1 = d;
            }
        }
        // (`best == nullptr || d0 > tol`)
        let Some(best) = best.filter(|_| d0.partial_cmp(&tol) != Some(Ordering::Greater)) else {
            return Err(PickFail::Point {
                status: Status::ERR_PICK_NONE,
                index,
            });
        };
        if d1 <= tol && d1 - d0 <= model_tol {
            return Err(PickFail::Point {
                status: Status::ERR_PICK_AMBIGUOUS,
                index,
            });
        }
        if !picked.items.contains(&best) {
            picked.items.push(best);
            picked.first.push(index);
        }
    }
    Ok(picked)
}

/// `smlib_pick::face_distance` (`SmApiFaceClosestPoint`): the distance from `p` to the
/// face `f` of `b` within its trims, and the nearest point: the surface's closest point
/// where the face classifies it as on the face (inside its trims or on their boundary),
/// else the nearest point of the face's edges, the same minimum for a trimmed face. It
/// stands in for solidcore's topology solve of the face's shape
/// (`face_closest_point_in_shape`) until this crate's call of that solve is fixed (the
/// solve itself agrees with C++). The classification may add the face's UV trim curves
/// to `b`, as SMLib's solve does.
pub(crate) fn face_distance(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    p: Point3,
) -> solidcore::Result<(f64, Point3)> {
    let mut best: Option<(f64, Point3)> = None;
    let mut consider = |d: f64, q: Point3| {
        if d.is_finite() && best.is_none_or(|(nearest, _)| d < nearest) {
            best = Some((d, q));
        }
    };
    // (an evaluation copy: the classification below takes the B-rep mutably)
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .map(Surface::clone_for_evaluation)
        .transpose()?;
    if let Some(surface) = surface {
        if let Ok(c) = surface_closest_point(k, &surface, p) {
            for &uv in &c.uvs {
                let on_face = face_classify_uv(k, b, f, uv).is_ok_and(|class| {
                    matches!(
                        class,
                        PointClassType::Face | PointClassType::Edge | PointClassType::Vertex
                    )
                });
                if on_face {
                    consider(c.closest.distance, c.closest.point);
                    break;
                }
            }
        }
    }
    for e in face_edges(b, f, None, false, None)? {
        if let Ok(c) = edge_closest_point(k, b, e, p) {
            consider(c.closest.distance, c.closest.point);
        }
    }
    best.ok_or_else(|| SolidError::sm_err("smlib_api::pick::face_distance"))
}

/// `smlib_pick::edge_distance` (`SmApiEdgeClosestPoint`).
fn edge_distance(k: &Kernel, b: &Brep, e: EdgeId, p: Point3) -> Option<f64> {
    edge_closest_point(k, b, e, p)
        .ok()
        .map(|c| c.closest.distance)
}

/// `smlib_pick::pick_faces`: the faces of `b` nearest `points` (`SmBrep::GetFaces`
/// order), at the body's tolerance.
pub(crate) fn pick_faces(
    k: &mut Kernel,
    b: &mut Brep,
    points: &[[f64; 3]],
    tol: f64,
) -> Result<Picked<FaceId>, PickFail> {
    let all = faces(b, None)?;
    let model_tol = b.tolerance();
    pick(&all, points, tol, model_tol, |f, p| {
        face_distance(k, b, f, p).ok().map(|(d, _)| d)
    })
}

/// `smlib_pick::pick_edges`: the edges of `b` nearest `points` (`SmBrep::GetEdges` order),
/// at the body's tolerance.
pub(crate) fn pick_edges(
    k: &Kernel,
    b: &Brep,
    points: &[[f64; 3]],
    tol: f64,
) -> Result<Picked<EdgeId>, PickFail> {
    let all = edges(b, None)?;
    pick(&all, points, tol, b.tolerance(), |e, p| {
        edge_distance(k, b, e, p)
    })
}

/// `smlib_pick::pick_vertices`: the vertices of `b` nearest `points`
/// (`SmBrep::GetVertices` order), at the body's tolerance.
pub(crate) fn pick_vertices(
    b: &Brep,
    points: &[[f64; 3]],
    tol: f64,
) -> Result<Picked<VertexId>, PickFail> {
    let all = vertices(b, None)?;
    pick(&all, points, tol, b.tolerance(), |v, p| {
        b.vertex(v).ok().map(|v| (v.point - p).length())
    })
}

/// `outer_faceuse`: the face use of `f` on the outside of its body, the one bounding the
/// infinite region (for a sheet, both do: then the upward one, along the surface
/// normal). `None` where SMLib's is null.
pub(crate) fn outer_faceuse(b: &Brep, f: FaceId) -> solidcore::Result<Option<FaceuseId>> {
    let Some(fu) = upward_faceuse(b, f)? else {
        return Ok(None);
    };
    let face_use = b.faceuse(fu)?;
    let outside = match face_use.shell {
        Some(shell) => shell_region(b, shell)? == b.infinite_region(),
        None => false,
    };
    Ok(if outside { Some(fu) } else { face_use.mate })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box's faces picked at their centres, a point between two faces, and a point far
    /// from every face.
    #[test]
    fn picks_box_faces() {
        let mut k = crate::session::fork_kernel();
        let mut b = solidcore::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        let points = [[5.0, 10.0, 30.0], [0.0, 10.0, 15.0], [5.0, 10.0, 0.0]];
        // each centre is on a face: distance 0
        for &[x, y, z] in &points {
            let p = Point3::new(x, y, z);
            let near = |d: f64| d.abs() < 1e-9;
            let nearest = faces(&b, None)
                .unwrap()
                .into_iter()
                .map(|f| face_distance(&mut k, &mut b, f, p).unwrap().0)
                .fold(f64::INFINITY, f64::min);
            assert!(near(nearest), "{p:?}: {nearest}");
        }
        let picked = pick_faces(&mut k, &mut b, &points, 1e-3).unwrap();
        assert_eq!(picked.items.len(), 3);
        assert_eq!(picked.first, [0, 1, 2]);
        // on the edge between the top and the x = 0 face: both at distance 0
        let edge = [[0.0, 10.0, 30.0]];
        let e = pick_faces(&mut k, &mut b, &edge, 1e-3).unwrap_err();
        assert!(
            matches!(e, PickFail::Point { status, index: 0 } if status == Status::ERR_PICK_AMBIGUOUS)
        );
        let far = [[5.0, 10.0, 15.0], [50.0, 50.0, 50.0]];
        let e = pick_faces(&mut k, &mut b, &far, 1e-3).unwrap_err();
        assert_eq!(e.index(), Some(0));
        // every edge of the box from its midpoint, and a vertex
        let mid = [[5.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 15.0]];
        assert_eq!(pick_edges(&k, &b, &mid, 1e-3).unwrap().items.len(), 3);
        assert_eq!(
            pick_vertices(&b, &[[10.0, 20.0, 30.0]], 1e-3)
                .unwrap()
                .items
                .len(),
            1
        );
        for f in faces(&b, None).unwrap() {
            assert!(outer_faceuse(&b, f).unwrap().is_some());
        }
    }
}
