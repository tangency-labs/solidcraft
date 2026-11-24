//! The exact volume of a faceted record: every face flat, every edge straight (not upstream;
//! tangency-labs/solidcraft#18458).
//!
//! Such a body is a polygonal B-rep, and solidcore measures one exactly
//! ([`solidcore::query::poly_properties::poly_brep_compute_volume`], upstream's
//! `SmApiPolyBrepComputeVolume`): the per-face pyramid sums of its loops, with no integration.
//! The mass-properties integration ([`crate::Body::mass_properties`]) reaches the same volume
//! to its requested accuracy, at a cost that grows with the faces' count: seconds for a mesh of
//! a few thousand facets, which Convert Mesh rebuilds as such a body.

use solidcore::math::Point3;
use solidcore::query::poly::{PolyBrep, PolyEdgeId};
use solidcore::query::poly_properties::poly_brep_compute_volume;
use solidcore_record::brep::{Brep, Curve, Loop, Shell, Surface};

use crate::error::Result;
use crate::session;

/// The volume of the record `b` when it is a faceted solid: every face a plane, every edge a
/// line between its two vertices, used by exactly two faces once each way as the shells'
/// normals run (closed, consistently oriented shells), and every face in exactly one shell. It
/// is signed as the shells' normals give it: positive for material inside normals that point
/// out of it, a void counting against it. `None` for any other record (a curved face or edge, a
/// wire or vertex shell, an open or inconsistent shell): measure it by integration instead.
///
/// Each face goes into a polygonal B-rep as its loops' polygons, run as its normal in its shell
/// runs, and the B-rep's volume is measured by solidcore.
///
/// # Errors
/// [`crate::Error::Reentered`] from inside a kernel call; the kernel's error for a polygonal
/// B-rep it refuses.
pub fn faceted_volume(b: &Brep) -> Result<Option<f64>> {
    let Some(faces) = faceted_faces(b) else {
        return Ok(None);
    };
    session::run("faceted_volume", |k| {
        let tolerance = b.tolerance;
        let mut poly = PolyBrep::new(k, tolerance);
        let region = poly.add_region()?;
        let mut vertices = Vec::with_capacity(b.vertices.len());
        for v in &b.vertices {
            let [x, y, z] = v.point;
            vertices.push(poly.add_vertex(k, Point3::new(x, y, z), v.tolerance)?);
        }
        let point = |i: u32| {
            b.vertices.get(i as usize).map(|v| {
                let [x, y, z] = v.point;
                Point3::new(x, y, z)
            })
        };
        // The polygonal edges each record edge became, for their radial pairs.
        let mut uses: Vec<Vec<PolyEdgeId>> = vec![Vec::new(); b.edges.len()];
        for shell_faces in &faces {
            let shell = poly.add_shell(region)?;
            for &(fi, forward) in shell_faces {
                let Some(face) = b.faces.get(fi) else {
                    return Ok(None);
                };
                let pf = poly.add_face(k, shell, tolerance, None)?;
                for lp in &face.loops {
                    let Loop::Edges { edges } = lp else {
                        return Ok(None);
                    };
                    // Each side from the corner the loop enters it at to the one it leaves by,
                    // with its record edge; run backwards for a face on its other side.
                    let mut sides = Vec::with_capacity(edges.len());
                    for oe in edges {
                        let Some(edge) = b.edges.get(oe.edge as usize) else {
                            return Ok(None);
                        };
                        let [start, end] = edge.vertices;
                        let (from, to) = if oe.forward {
                            (start, end)
                        } else {
                            (end, start)
                        };
                        sides.push(if forward {
                            (from, to, oe.edge)
                        } else {
                            (to, from, oe.edge)
                        });
                    }
                    if !forward {
                        sides.reverse();
                    }
                    let ploop = poly.start_poly_edge_loop(pf)?;
                    for &(from, to, edge) in &sides {
                        let (Some(p), Some(q), Some(&start_vertex)) =
                            (point(from), point(to), vertices.get(from as usize))
                        else {
                            return Ok(None);
                        };
                        let e = poly.add_poly_edge(k, ploop, tolerance, p, q, start_vertex)?;
                        if let Some(list) = uses.get_mut(edge as usize) {
                            list.push(e);
                        }
                    }
                    poly.finish_poly_edge_loop(ploop)?;
                }
            }
        }
        for pair in &uses {
            poly.link_radials(pair)?;
        }
        let volume = poly_brep_compute_volume(k, &mut poly)?;
        Ok(volume.is_finite().then_some(volume))
    })
}

/// The faces of each shell of `b`, with the side each is used on, when `b` is a faceted solid
/// as [`faceted_volume`] takes one; `None` otherwise. Topology only: nothing is computed.
fn faceted_faces(b: &Brep) -> Option<Vec<Vec<(usize, bool)>>> {
    let mut seen = vec![false; b.faces.len()];
    let mut shells = Vec::with_capacity(b.shells.len());
    for shell in &b.shells {
        let Shell::Faces { faces, .. } = shell else {
            return None;
        };
        let mut list = Vec::with_capacity(faces.len());
        for of in faces {
            let slot = seen.get_mut(of.face as usize)?;
            if *slot {
                return None;
            }
            *slot = true;
            list.push((of.face as usize, of.forward));
        }
        shells.push(list);
    }
    if seen.iter().any(|s| !s) {
        return None;
    }
    if b.edges.iter().any(|e| {
        !matches!(e.curve, Curve::Line { .. })
            || e.vertices.first() == e.vertices.last()
            || e.uses.len() != 2
    }) {
        return None;
    }
    // Each edge run once each way, as the shells' normals run its two uses.
    let mut runs: Vec<[u8; 2]> = vec![[0, 0]; b.edges.len()];
    for list in &shells {
        for &(fi, forward) in list {
            let face = b.faces.get(fi)?;
            if !matches!(
                face.surface,
                Surface::Plane { .. } | Surface::PlaneThrough { .. }
            ) {
                return None;
            }
            for lp in &face.loops {
                let Loop::Edges { edges } = lp else {
                    return None;
                };
                for oe in edges {
                    let [ahead, back] = runs.get_mut(oe.edge as usize)?;
                    let run = if oe.forward == forward { ahead } else { back };
                    *run = run.saturating_add(1);
                }
            }
        }
    }
    runs.iter().all(|r| *r == [1, 1]).then_some(shells)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box's volume is exact, and agrees with the integration to its accuracy; a record with an
    /// edge that is no line has none.
    #[test]
    fn a_faceted_records_volume_is_exact() {
        let body = crate::make_box([1.0, -2.0, 3.0], [2.0, 3.0, 4.0]).unwrap();
        let (record, _) = body.to_record().unwrap();
        let v = faceted_volume(&record).unwrap().unwrap();
        assert!((v - 24.0).abs() < 1e-12, "{v}");
        let integrated = body.mass_properties(1e-6).unwrap().volume;
        assert!(
            (integrated - v).abs() < 1e-4 * v,
            "{integrated} against {v}"
        );
        let mut curved = record;
        if let Some(e) = curved.edges.first_mut() {
            e.curve = Curve::Polyline {
                points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            };
        }
        assert_eq!(faceted_volume(&curved).unwrap(), None);
    }
}
