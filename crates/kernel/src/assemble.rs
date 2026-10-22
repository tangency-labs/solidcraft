//! Bodies built face by face on the schema 4 record: an operation that rebuilds part of a body
//! from its own topology (an emboss on a cylinder, a raised band, a deleted face healed by its
//! neighbours) starts from the body's record, takes faces away, adds faces bounded by new or
//! existing edges, and finishes into a record in canonical order (docs/native-brep.md): pcurves
//! from the record's one routine ([`solidcraft_geom::brep::pcurve`]) for every new use, the
//! record's own for every use kept, edge uses, regions and shells. The body is that record.
//!
//! Faces keep the record's convention as the operation gives them: each face's loops are seen
//! from the side the material is not on (the outward side), the outer loop counter-clockwise
//! and holes clockwise there; [`Assembly::finish`] stores them relative to the surface's natural
//! normal, as the record does.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::pcurve::{EdgeUse, pcurves_within};
use solidcraft_geom::brep::{Brep, Curve, Curve2, Edge, Face, Loop, Natural, OrientedEdge, OrientedFace, Region, Shell, Surface, VERSION, Vertex};

use crate::body::Body;
use crate::history::{EdgeOrigin, FaceProv};
use crate::{KernelError, Result};

fn invalid(m: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("rebuilding the B-rep: {m}"))
}

/// One edge of the assembly: its curve on `interval`, from `vertices[0]` to `vertices[1]`.
#[derive(Clone, Debug)]
pub(crate) struct AEdge {
    pub curve: Curve,
    pub interval: [f64; 2],
    pub vertices: [u32; 2],
    pub tolerance: f64,
    /// The source record's edge this edge is, kept as it was.
    pub source: Option<usize>,
    /// The source record's edges this new edge is (a piece of, or the moved) edge of.
    pub of: Vec<usize>,
}

/// One use of an edge in a face's loop: the loop runs along it from its start (`forward`) or
/// from its end, with the source record's pcurve where the use is one the record had.
#[derive(Clone, Debug)]
pub(crate) struct AUse {
    pub edge: u32,
    pub forward: bool,
    pub given: Option<Curve2>,
}

/// One face of the assembly: its surface, whether its natural normal points out of the
/// material, its loops (outer first) seen from outside, and the shell it belongs to (a shell of
/// the source record, by index).
#[derive(Clone, Debug)]
pub(crate) struct AFace {
    pub surface: Surface,
    pub natural_out: bool,
    pub loops: Vec<Vec<AUse>>,
    pub shell: usize,
    /// The source record's face whose surface and parameter box this face keeps.
    pub domain: Option<[[f64; 2]; 2]>,
}

/// A body taken apart: vertices, edges and faces that [`Assembly::finish`] builds into a record.
#[derive(Clone, Debug)]
pub(crate) struct Assembly {
    pub tolerance: f64,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<AEdge>,
    /// `None`: a face taken away.
    pub faces: Vec<Option<AFace>>,
    /// The source's regions, by shell index (shells keep their indices; a shell left with no
    /// face is dropped).
    regions: Vec<Region>,
    shells: usize,
}

/// A finished record, with where each face of the assembly went in it (`None`: taken away).
pub(crate) struct Finished {
    pub record: Brep,
    pub faces: Vec<Option<usize>>,
    /// For each edge of the record, the assembly edge it is.
    pub edges: Vec<usize>,
}

impl Assembly {
    /// The record `b` taken apart, every face and edge as it is (face i and edge i of the
    /// assembly are the record's). Only face shells: a record with wire or vertex shells is not
    /// rebuilt here.
    pub(crate) fn from_record(b: &Brep) -> Result<Assembly> {
        let mut faces: Vec<Option<AFace>> = vec![None; b.faces.len()];
        for (s, shell) in b.shells.iter().enumerate() {
            let Shell::Faces { faces: uses, wires } = shell else { return Err(invalid("a wire or vertex shell")) };
            if !wires.is_empty() {
                return Err(invalid("wire edges inside a shell"));
            }
            for of in uses {
                let f = b.faces.get(of.face as usize).ok_or_else(|| invalid("face reference"))?;
                let mut loops = Vec::with_capacity(f.loops.len());
                for l in &f.loops {
                    let Loop::Edges { edges } = l else { return Err(invalid("a vertex loop")) };
                    let mut lp: Vec<AUse> = edges.iter().map(|u| AUse { edge: u.edge, forward: u.forward, given: Some(u.pcurve.clone()) }).collect();
                    // Stored about the natural normal; the assembly sees loops from outside.
                    if !of.forward {
                        reverse(&mut lp);
                    }
                    loops.push(lp);
                }
                let slot = faces.get_mut(of.face as usize).ok_or_else(|| invalid("face reference"))?;
                if slot.is_some() {
                    return Err(invalid("a face used by two shells"));
                }
                *slot = Some(AFace { surface: f.surface.clone(), natural_out: of.forward, loops, shell: s, domain: Some(f.domain) });
            }
        }
        if faces.iter().any(Option::is_none) {
            return Err(invalid("a face no shell uses"));
        }
        let edges = b
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| AEdge {
                curve: e.curve.clone(),
                interval: e.interval,
                vertices: e.vertices,
                tolerance: e.tolerance,
                source: Some(i),
                of: Vec::new(),
            })
            .collect();
        Ok(Assembly { tolerance: b.tolerance, vertices: b.vertices.clone(), edges, faces, regions: b.regions.clone(), shells: b.shells.len() })
    }

    /// A new vertex at `p`.
    pub(crate) fn vertex(&mut self, p: Vec3) -> Result<u32> {
        if !p.is_finite() {
            return Err(invalid("a vertex off the finite range"));
        }
        self.vertices.push(Vertex { point: [p.x, p.y, p.z], tolerance: self.tolerance });
        index(self.vertices.len() - 1)
    }

    /// A vertex's point.
    pub(crate) fn point(&self, v: u32) -> Option<Vec3> {
        self.vertices.get(v as usize).map(|v| Vec3::from(v.point))
    }

    /// A new edge on `curve` over `interval`, from `a` to `b`.
    pub(crate) fn edge(&mut self, curve: Curve, interval: [f64; 2], a: u32, b: u32) -> Result<u32> {
        self.edges.push(AEdge { curve, interval, vertices: [a, b], tolerance: self.tolerance, source: None, of: Vec::new() });
        index(self.edges.len() - 1)
    }

    /// A new straight edge from vertex `a` to vertex `b` (t from 0 to 1).
    pub(crate) fn line(&mut self, a: u32, b: u32) -> Result<u32> {
        let (Some(p), Some(q)) = (self.point(a), self.point(b)) else { return Err(invalid("vertex reference")) };
        if p.dist(q) <= self.tolerance {
            return Err(invalid("a line of no length"));
        }
        self.edge(Curve::Line { start: [p.x, p.y, p.z], end: [q.x, q.y, q.z] }, [0.0, 1.0], a, b)
    }

    /// Record that edge `e` is (a piece of, or the moved) edge `of` of the assembly: the source
    /// edges under `of` are under `e`.
    pub(crate) fn piece(&mut self, e: u32, of: u32) {
        let roots: Vec<usize> = match self.edges.get(of as usize) {
            Some(x) => x.source.map(|s| vec![s]).unwrap_or_else(|| x.of.clone()),
            None => Vec::new(),
        };
        if let Some(x) = self.edges.get_mut(e as usize) {
            for r in roots {
                if !x.of.contains(&r) {
                    x.of.push(r);
                }
            }
        }
    }

    /// A new face.
    pub(crate) fn face(&mut self, face: AFace) {
        self.faces.push(Some(face));
    }

    /// Where an edge use starts and ends (in the loop's direction).
    pub(crate) fn ends(&self, u: &AUse) -> Option<(u32, u32)> {
        let e = self.edges.get(u.edge as usize)?;
        let [a, b] = e.vertices;
        Some(if u.forward { (a, b) } else { (b, a) })
    }

    /// The record of the faces left and added, in canonical order (`Brep::canonical`), checked:
    /// each face's loops stored relative to its surface's natural normal with their pcurves and
    /// parameter box, shells and regions as the source's (a region whose outer shell lost every
    /// face is gone).
    pub(crate) fn finish(&self) -> Result<Finished> {
        // The faces left and added, in assembly order, each in its shell.
        let kept: Vec<usize> = self.faces.iter().enumerate().filter(|(_, f)| f.is_some()).map(|(i, _)| i).collect();
        let mut by_shell: Vec<Vec<u32>> = vec![Vec::new(); self.shells];
        let mut faces: Vec<Face> = Vec::with_capacity(kept.len());
        let mut sides: Vec<bool> = Vec::with_capacity(kept.len());
        for (k, &i) in kept.iter().enumerate() {
            let f = self.faces.get(i).and_then(Option::as_ref).ok_or_else(|| invalid("face reference"))?;
            by_shell.get_mut(f.shell).ok_or_else(|| invalid("shell reference"))?.push(index(k)?);
            faces.push(self.stored(i, f)?);
            sides.push(f.natural_out);
        }
        let shells = by_shell
            .iter()
            .map(|members| {
                let faces =
                    members.iter().map(|&k| Ok(OrientedFace { face: k, forward: *sides.get(k as usize).ok_or_else(|| invalid("face reference"))? }));
                Ok(Shell::Faces { faces: faces.collect::<Result<_>>()?, wires: Vec::new() })
            })
            .collect::<Result<Vec<_>>>()?;
        let used = |s: u32| by_shell.get(s as usize).is_some_and(|f| !f.is_empty());
        let mut regions: Vec<Region> = Vec::new();
        for region in &self.regions {
            // A bounded region whose outer shell went is gone (its faces were all taken away).
            if regions.is_empty() || region.shells.first().is_some_and(|s| used(*s)) {
                regions.push(Region { shells: region.shells.iter().copied().filter(|s| used(*s)).collect(), void: region.void });
            }
        }
        let edges = self.edges.iter().map(|a| Edge {
            nurbs: None,
            curve: a.curve.clone(),
            interval: a.interval,
            vertices: a.vertices,
            tolerance: a.tolerance,
            uses: Vec::new(),
        });
        let loose =
            Brep { version: VERSION, tolerance: self.tolerance, regions, shells, faces, edges: edges.collect(), vertices: self.vertices.clone() };
        let part = loose.canonical().map_err(invalid)?;
        part.brep.check_geometry().map_err(invalid)?;
        let mut face_map: Vec<Option<usize>> = vec![None; self.faces.len()];
        for (c, &k) in part.faces.iter().enumerate() {
            if let Some(slot) = kept.get(k as usize).and_then(|i| face_map.get_mut(*i)) {
                *slot = Some(c);
            }
        }
        Ok(Finished { record: part.brep, faces: face_map, edges: part.edges.iter().map(|e| *e as usize).collect() })
    }

    /// Face `i` as the record stores it: its loops relative to its surface's natural normal, each
    /// use with its pcurve (the record's own where the use kept one), and its parameter box.
    fn stored(&self, i: usize, f: &AFace) -> Result<Face> {
        let loops: Vec<Vec<AUse>> = f
            .loops
            .iter()
            .map(|l| {
                let mut l = l.clone();
                if !f.natural_out {
                    reverse(&mut l);
                }
                l
            })
            .collect();
        let uses: Vec<Vec<EdgeUse<'_>>> = loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|u| {
                        let e = self.edges.get(u.edge as usize).ok_or_else(|| invalid("edge reference"))?;
                        Ok(EdgeUse {
                            edge: u.edge,
                            curve: &e.curve,
                            interval: e.interval,
                            forward: u.forward,
                            tolerance: e.tolerance,
                            given: u.given.as_ref(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<_>>()?;
        let pcurves = pcurves_within(&f.surface, &uses, self.tolerance, f.domain).map_err(|e| invalid(format!("face {i}: {e}")))?;
        let domain = domain_of(&f.surface, &pcurves, f.domain).ok_or_else(|| invalid(format!("face {i}: no parameter box")))?;
        let stored = loops
            .iter()
            .zip(&pcurves)
            .map(|(l, pc)| Loop::Edges {
                edges: l.iter().zip(pc).map(|(u, pcurve)| OrientedEdge { edge: u.edge, forward: u.forward, pcurve: pcurve.clone() }).collect(),
            })
            .collect();
        Ok(Face { nurbs: None, surface: f.surface.clone(), domain, loops: stored })
    }
}

/// Reverse a loop: the same edges, the other way round.
fn reverse(l: &mut [AUse]) {
    l.reverse();
    for u in l {
        u.forward = !u.forward;
    }
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| invalid("too many items"))
}

/// A face's parameter box: the source face's where it kept it, else the box of its pcurves'
/// control points (which holds the curves), inside the surface's natural domain.
fn domain_of(surface: &Surface, pcurves: &[Vec<Curve2>], kept: Option<[[f64; 2]; 2]>) -> Option<[[f64; 2]; 2]> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for c in pcurves.iter().flatten() {
        let points: Vec<[f64; 2]> = match c {
            Curve2::Spline { points, .. } => points.clone(),
            Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
        };
        for p in points {
            for d in 0..2 {
                if let (Some(l), Some(h), Some(x)) = (lo.get_mut(d), hi.get_mut(d), p.get(d)) {
                    *l = l.min(*x);
                    *h = h.max(*x);
                }
            }
        }
    }
    if let Some(k) = kept {
        // The source's box, grown to hold any loop the face has now.
        for d in 0..2 {
            if let (Some(l), Some(h), Some(r)) = (lo.get_mut(d), hi.get_mut(d), k.get(d)) {
                *l = l.min(r[0]);
                *h = h.max(r[1]);
            }
        }
    }
    let natural = surface.natural();
    let mut out = [[0.0; 2]; 2];
    for d in 0..2 {
        let (mut a, mut b) = (*lo.get(d)?, *hi.get(d)?);
        if !(a.is_finite() && b.is_finite()) {
            return None;
        }
        match natural.get(d)? {
            Natural::Periodic => b = b.min(a + std::f64::consts::TAU),
            Natural::Range(x, y) => {
                a = a.max(*x);
                b = b.min(*y);
            }
        }
        if !(a < b) {
            return None;
        }
        *out.get_mut(d)? = [a, b];
    }
    Some(out)
}

/// The body of `finished`, built from the one operand `from`, with the history whose faces are
/// `faces` (in the assembly's face order, taken-away faces skipped): each kept edge the operand
/// edge it is, each re-made one the operand edges it is a piece of, each new one where its two
/// faces meet ([`crate::record::of_faces`]). The body is the finished record, so its faces and
/// edges are numbered as the record's.
pub(crate) fn body_with_history(finished: Finished, assembly: &Assembly, from: &Body, faces: Vec<FaceProv>) -> Result<Body> {
    // The provenance of each record face, from the assembly's face order.
    let mut provs: Vec<Option<FaceProv>> = vec![None; finished.record.faces.len()];
    let mut given = faces.into_iter();
    for k in finished.faces.iter().flatten() {
        let p = given.next().ok_or_else(|| invalid("history: fewer faces than the body"))?;
        *provs.get_mut(*k).ok_or_else(|| invalid("history: face"))? = Some(p);
    }
    let faces: Option<Vec<FaceProv>> = provs.into_iter().collect();
    let edges: Vec<Vec<EdgeOrigin>> = finished
        .edges
        .iter()
        .map(|a| match assembly.edges.get(*a) {
            Some(AEdge { source: Some(edge), .. }) => crate::record::pieces_of(0, [*edge]),
            Some(AEdge { of, .. }) => crate::record::pieces_of(0, of.iter().copied()),
            None => Vec::new(),
        })
        .collect();
    let body = Body::from_record(finished.record)?;
    Ok(crate::record::attach(body, &crate::record::counts(&[from]), faces, &edges))
}

/// Whether the face `face` of `b` has its natural normal out of the material.
pub(crate) fn natural_out(b: &Brep, face: usize) -> Option<bool> {
    b.shells.iter().find_map(|s| match s {
        Shell::Faces { faces, .. } => faces.iter().find(|f| f.face as usize == face).map(|f| f.forward),
        _ => None,
    })
}
