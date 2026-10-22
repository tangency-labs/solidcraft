//! Splitting a record's edges at parameters inside them, exactly: each piece keeps the edge's
//! curve on a sub-interval, a new vertex sits on the curve at each split, and every use of the
//! edge becomes uses of its pieces with the use's pcurve cut at the same parameters
//! ([`crate::bspline::segment`]: the same points at the same parameters). What a sheet needs so
//! that another sheet's vertex on one of its edges is a vertex of both (no T-junction).
use super::eval::CurveEval;
use super::part::Part;
use super::{Brep, Curve2, Edge, Invalid, Loop, OrientedEdge, Result, Shell, Vertex};
use crate::bspline::{HPoint, segment};

impl Brep {
    /// The record with edge `e` split at each `t` of `at` (`(e, t)`, `t` strictly inside the edge's
    /// interval), in canonical order. The part's `edges` name, for each edge, the source edge it is
    /// (a piece of); its `vertices` the source vertex, or for a vertex made at a split
    /// `self.vertices.len()` plus its number among the splits (sorted by edge, then parameter).
    pub fn split_edges(&self, at: &[(u32, f64)]) -> Result<Part> {
        let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); self.edges.len()];
        for &(e, t) in at {
            let edge = self.edges.get(e as usize).ok_or(Invalid("split edge reference"))?;
            let [a, b] = edge.interval;
            if !(t > a && t < b) {
                return Err(Invalid("a split parameter is not inside its edge"));
            }
            cuts.get_mut(e as usize).ok_or(Invalid("split edge reference"))?.push(t);
        }
        // Each source edge's pieces: (new edge index, interval), and the new vertices.
        let mut vertices: Vec<Vertex> = self.vertices.clone();
        let mut edges: Vec<Edge> = Vec::with_capacity(self.edges.len() + at.len());
        let mut source: Vec<u32> = Vec::with_capacity(self.edges.len() + at.len());
        let mut pieces: Vec<Vec<(u32, [f64; 2])>> = Vec::with_capacity(self.edges.len());
        for (e, (edge, ts)) in self.edges.iter().zip(cuts.iter_mut()).enumerate() {
            ts.sort_by(f64::total_cmp);
            ts.dedup();
            let curve = if ts.is_empty() { None } else { Some(CurveEval::new(&edge.curve).map_err(|_| Invalid("a split edge's curve"))?) };
            let [a, b] = edge.interval;
            let mut ends = vec![edge.vertices[0]];
            for &t in ts.iter() {
                let p = curve.as_ref().and_then(|c| c.point(t)).ok_or(Invalid("a split edge has no point there"))?;
                ends.push(index(vertices.len())?);
                vertices.push(Vertex { point: [p.x, p.y, p.z], tolerance: edge.tolerance });
            }
            ends.push(edge.vertices[1]);
            let bounds: Vec<f64> = std::iter::once(a).chain(ts.iter().copied()).chain(std::iter::once(b)).collect();
            let mut list = Vec::with_capacity(bounds.len() - 1);
            for (k, w) in bounds.windows(2).enumerate() {
                let interval = [w[0], w[1]];
                let (Some(&from), Some(&to)) = (ends.get(k), ends.get(k + 1)) else { return Err(Invalid("split ends")) };
                list.push((index(edges.len())?, interval));
                edges.push(Edge { interval, vertices: [from, to], uses: Vec::new(), ..edge.clone() });
                source.push(index(e)?);
            }
            pieces.push(list);
        }
        let piece_list = |e: u32| pieces.get(e as usize).ok_or(Invalid("split edge reference"));
        let mut faces = self.faces.clone();
        for face in &mut faces {
            for l in &mut face.loops {
                let Loop::Edges { edges: uses } = l else { continue };
                let mut out = Vec::with_capacity(uses.len());
                for u in uses.iter() {
                    let list = piece_list(u.edge)?;
                    let cut = |&(edge, [t0, t1]): &(u32, [f64; 2])| -> Result<OrientedEdge> {
                        let pcurve = if list.len() == 1 { u.pcurve.clone() } else { cut_pcurve(&u.pcurve, t0, t1)? };
                        Ok(OrientedEdge { edge, forward: u.forward, pcurve })
                    };
                    if u.forward {
                        for p in list {
                            out.push(cut(p)?);
                        }
                    } else {
                        for p in list.iter().rev() {
                            out.push(cut(p)?);
                        }
                    }
                }
                *uses = out;
            }
        }
        let pieces_of = |wires: &[u32]| -> Result<Vec<u32>> {
            Ok(wires.iter().map(|e| piece_list(*e)).collect::<Result<Vec<_>>>()?.into_iter().flatten().map(|p| p.0).collect())
        };
        let shells = self
            .shells
            .iter()
            .map(|s| {
                Ok(match s {
                    Shell::Faces { faces, wires } => Shell::Faces { faces: faces.clone(), wires: pieces_of(wires)? },
                    Shell::Wire { edges } => Shell::Wire { edges: pieces_of(edges)? },
                    Shell::Vertex { vertex } => Shell::Vertex { vertex: *vertex },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let loose = Brep { version: self.version, tolerance: self.tolerance, regions: self.regions.clone(), shells, faces, edges, vertices };
        let mut part = loose.canonical()?;
        for e in &mut part.edges {
            *e = source.get(*e as usize).copied().ok_or(Invalid("split edge reference"))?;
        }
        Ok(part)
    }
}

/// A pcurve's piece on `[t0, t1]`, a sub-interval of its own.
fn cut_pcurve(c: &Curve2, t0: f64, t1: f64) -> Result<Curve2> {
    let (knots, h): (&[f64], Vec<HPoint>) = match c {
        Curve2::Spline { knots, points } => (knots, points.iter().map(|p| HPoint { x: p[0], y: p[1], z: 0.0, w: 1.0 }).collect()),
        Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.iter().map(|p| HPoint { x: p[0], y: p[1], z: 0.0, w: p[2] }).collect()),
    };
    let degree = knots.len().checked_sub(h.len() + 1).ok_or(Invalid("pcurve degree"))?;
    let (knots, h) = segment(degree, knots, &h, t0, t1).map_err(|_| Invalid("a pcurve cannot be cut there"))?;
    Ok(match c {
        Curve2::Spline { .. } => Curve2::Spline { knots, points: h.iter().map(|p| [p.x, p.y]).collect() },
        Curve2::RationalSpline { .. } => Curve2::RationalSpline { knots, homogeneous: h.iter().map(|p| [p.x, p.y, p.w]).collect() },
    })
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| Invalid("split index"))
}
