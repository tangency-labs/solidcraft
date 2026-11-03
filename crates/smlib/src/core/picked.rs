//! What a pick by points chose, as indices into the body's own lists: for the operations a trace
//! records ([`crate::trace`]), so the corpus names its edges, faces and corners by topology
//! index as well as by the points the operation was given. Each point is picked alone, by the
//! same picker the operation runs, on the body as the operation received it.

use std::sync::PoisonError;

use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::{edge, vertex};

use super::Body;
use super::blend::{Miss, pick_edges, pick_vertices};
use super::offset::pick_faces;
use crate::Result;
use crate::trace::{Pick, PickKind};

/// The entity each of `points` picks within `tolerance` on `body`, as [`Pick`]s: its index in
/// the body's list of that kind (`None` where the point picks none or is ambiguous) and, for an
/// edge, its end points; for a vertex, its point.
pub(crate) fn picks(body: &Body, kind: PickKind, points: &[[f64; 3]], tolerance: f64) -> Result<Vec<Pick>> {
    super::run_on("trace pick", &[], |k| {
        let mut b = body.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out = Vec::with_capacity(points.len());
        for p in points {
            let one = std::slice::from_ref(p);
            let pick = match kind {
                PickKind::Edge => {
                    let all = brep_p1::edges(&b, None)?;
                    match pick_edges(k, &b, one, tolerance)? {
                        Ok((es, _)) => match es.first() {
                            Some(&e) => {
                                let mut ends = Vec::with_capacity(2);
                                for v in [edge::start_vertex(&b, e)?, edge::end_vertex(&b, e)?].into_iter().flatten() {
                                    let q = vertex::point(&b, v)?;
                                    ends.push([q.x, q.y, q.z]);
                                }
                                Pick { index: all.iter().position(|&x| x == e), at: ends, miss: None }
                            }
                            None => Pick::missed("none"),
                        },
                        Err(m) => Pick::missed(miss(m)),
                    }
                }
                PickKind::Face => {
                    let all = brep_p1::faces(&b, None)?;
                    match pick_faces(k, &mut b, one, tolerance)? {
                        Ok(fs) => match fs.first() {
                            Some(&f) => Pick { index: all.iter().position(|&x| x == f), at: Vec::new(), miss: None },
                            None => Pick::missed("none"),
                        },
                        Err(m) => Pick::missed(miss(m)),
                    }
                }
                PickKind::Vertex => {
                    let all = brep_p1::vertices(&b, None)?;
                    match pick_vertices(&b, one, tolerance)? {
                        Ok((vs, _)) => match vs.first() {
                            Some(&v) => {
                                let q = vertex::point(&b, v)?;
                                Pick { index: all.iter().position(|&x| x == v), at: vec![[q.x, q.y, q.z]], miss: None }
                            }
                            None => Pick::missed("none"),
                        },
                        Err(m) => Pick::missed(miss(m)),
                    }
                }
            };
            out.push(pick);
        }
        b.refresh_cached_topology();
        Ok(out)
    })
}

fn miss(m: Miss) -> &'static str {
    match m {
        Miss::None(_) => "none",
        Miss::Ambiguous(_) => "ambiguous",
    }
}
