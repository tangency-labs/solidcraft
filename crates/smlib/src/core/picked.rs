//! What a pick by points chose, as indices into the body's own lists: for the operations a trace
//! records ([`crate::trace`]), so the corpus names its edges, faces and corners by topology
//! index as well as by the points the operation was given. Each point is picked alone, by the
//! picker the operation runs (solidcore-ops' [`solidcore_ops::pick`], and the nearest edge or
//! face as the extend and untrim picks take them), on a copy of the body as the operation
//! received it: a pick builds query caches on the body it reads, and the body traced stays as an
//! untraced run leaves it.

use solidcore_ops::measure::Adjacency;
use solidcore_ops::pick::{Miss, Picked};

use super::Body;
use super::call::ops_on;
use crate::Result;
use crate::trace::{Pick, PickKind};

const CALL: &str = "trace pick";

/// The entity each of `points` picks within `tolerance` on `body`, as [`Pick`]s: its index in
/// the body's list of that kind (`None` where the point picks none or is ambiguous) and, for an
/// edge, its end points; for a vertex, its point.
pub(crate) fn picks(body: &Body, kind: PickKind, points: &[[f64; 3]], tolerance: f64) -> Result<Vec<Pick>> {
    let b = ops_on(CALL, &[], || body.copy())?;
    let order = b.own_order();
    let adjacency = ops_on(CALL, &[], || b.adjacency(&order))?;
    let mut out = Vec::with_capacity(points.len());
    for p in points {
        let one = std::slice::from_ref(p);
        let pick = match kind {
            PickKind::Edge => picked(ops_on(CALL, &[], || solidcore_ops::pick::edges(&b, one, tolerance))?, |e| ends(&adjacency, e)),
            PickKind::NearestEdge => {
                let mut distances = Vec::with_capacity(order.edges.len());
                for e in 0..order.edges.len() {
                    // a closest point the kernel does not find, however it fails, is no distance
                    distances.push(b.edge_closest_point(&order, e, *p).ok().flatten().map(|c| c.distance));
                }
                match super::trim::nearest_within(distances, tolerance) {
                    Some(e) => Pick { index: Some(e), at: ends(&adjacency, e), miss: None },
                    None => Pick::missed("none"),
                }
            }
            PickKind::NearestFace => {
                let rows = ops_on(CALL, &[], || b.face_distances_within(&order, one, tolerance))?;
                match super::trim::nearest_within(rows.iter().map(|row| row.first().copied().flatten()), tolerance) {
                    Some(f) => Pick { index: Some(f), at: Vec::new(), miss: None },
                    None => Pick::missed("none"),
                }
            }
            PickKind::Face => picked(ops_on(CALL, &[], || solidcore_ops::pick::faces(&b, one, tolerance))?, |_| Vec::new()),
            PickKind::Vertex => picked(ops_on(CALL, &[], || solidcore_ops::pick::vertices(&b, one, tolerance))?, |v| {
                adjacency.vertex_points.get(v).map(|q| vec![*q]).unwrap_or_default()
            }),
        };
        out.push(pick);
    }
    Ok(out)
}

/// A one-point pick as a [`Pick`], `at` giving the picked entity's points.
fn picked(p: Picked, at: impl Fn(usize) -> Vec<[f64; 3]>) -> Pick {
    match p {
        Ok(v) => match v.first() {
            Some(&i) => Pick { index: Some(i), at: at(i), miss: None },
            None => Pick::missed("none"),
        },
        Err(Miss::None(_)) => Pick::missed("none"),
        Err(Miss::Ambiguous(_)) => Pick::missed("ambiguous"),
    }
}

/// Edge `e`'s end points.
fn ends(adjacency: &Adjacency, e: usize) -> Vec<[f64; 3]> {
    let Some(Some(vs)) = adjacency.edge_vertices.get(e) else { return Vec::new() };
    vs.iter().filter_map(|v| adjacency.vertex_points.get(*v).copied()).collect()
}
