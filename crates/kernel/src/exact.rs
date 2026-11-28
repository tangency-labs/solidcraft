//! Bodies to and from exact bodies ([`solidcraft_kernel_api::ExactBody`]) in memory: how a body
//! crosses to a kernel that can't write the record itself, with no STEP on the way
//! (`docs/design/kernel-layer.md` §3).
//!
//! Out, a body's record is given as it is: its vertices, edges and faces in the record's order,
//! each on its labelled curve or surface, with its pcurves where they are in that surface's own
//! (u, v). A round face on its NURBS form is given as the analytic surface it is, without
//! pcurves (they are on the form), and an edge on a circle's or ellipse's form without its
//! interval (its vertices say where it runs), as the STEP writer writes them. A face whose record
//! (u, v) are a net's (a ball patch, a blend by its definition, a torus whose tube reaches its
//! axis) is given as that net with its pcurves, and its label is lost.
//!
//! In, an exact body is assembled by solidcore's assembly stage (`solidcore_step::assembly`),
//! the code every body read from a file goes through: seams, NURBS forms, tolerances measured
//! on the geometry, the pcurves it lacks, and the canonical order. The producer's pcurves are
//! taken as given on faces without a NURBS form of their own, so no pcurve it holds is fitted
//! again. Its history is renumbered into the record's through the faces and edges the assembly
//! reports each record item came from.

use solidcore_record::brep::history::{Change, FaceProv, History};
use solidcore_record::brep::{Brep, Loop, Shell};
use solidcore_step::assembly::{Assembly, Bound, Kind};
use solidcraft_kernel_api::{ExactAssembled, ExactBody, ExactBound, ExactEdge, ExactExported, ExactFace, ExactHistory, ExactKind, ExactUse};

use crate::body::Body;
use crate::history::faces_of;
use crate::{KernelError, Result};

fn refused(what: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("exact body: {what}"))
}

/// `body`'s record as an exact body, each face and edge's id its record index.
pub fn exact_body(body: &Body) -> Result<ExactBody> {
    if body.is_mesh() {
        return Err(refused("a mesh body has no B-rep record"));
    }
    exact_of(body.record())
}

/// A record as an exact body, each face and edge's id its record index.
pub fn exact_of(record: &Brep) -> Result<ExactBody> {
    let edges = record
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| ExactEdge {
            id: i as u64,
            curve: e.curve.clone(),
            // An interval on a conic's NURBS form is not one on the conic: the vertices say.
            interval: if e.nurbs.is_some() { None } else { Some(e.interval) },
            vertices: e.vertices,
        })
        .collect();
    // Each face's side in the shell that uses it.
    let mut side: Vec<Option<bool>> = vec![None; record.faces.len()];
    let mut shells = Vec::with_capacity(record.shells.len());
    for s in &record.shells {
        let Shell::Faces { faces, wires } = s else { return Err(refused("a shell of wires or a vertex")) };
        if !wires.is_empty() {
            return Err(refused("wire edges in a shell"));
        }
        let mut list = Vec::with_capacity(faces.len());
        for f in faces {
            let slot = side.get_mut(f.face as usize).ok_or_else(|| refused("a face reference"))?;
            match slot {
                Some(was) if *was != f.forward => return Err(refused("a face used on both its sides")),
                _ => *slot = Some(f.forward),
            }
            list.push(f.face);
        }
        shells.push(list);
    }
    let mut faces = Vec::with_capacity(record.faces.len());
    for (i, f) in record.faces.iter().enumerate() {
        let on_net = solidcore_step::write::topology::as_form(f, record.tolerance);
        let surface = match (&f.surface, &f.nurbs) {
            (_, Some(net)) if on_net => net.clone(),
            // The rest of the faces on a form of their own are given as what they are; a
            // screw-swept wall (docs/design/screw-surface-record.md) joins the nets above.
            (s, _) => s.clone(),
        };
        // Pcurves in the (u, v) of the surface given: those of a face on no form, or on its net.
        let pcurves = f.nurbs.is_none() || on_net;
        let bounds = f
            .loops
            .iter()
            .map(|l| match l {
                Loop::Edges { edges } => ExactBound::Edges {
                    uses: edges
                        .iter()
                        .map(|u| ExactUse {
                            edge: u.edge,
                            forward: u.forward,
                            // A pcurve over an edge's NURBS form is not one over its curve.
                            pcurve: (pcurves && record.edges.get(u.edge as usize).is_some_and(|e| e.nurbs.is_none())).then(|| u.pcurve.clone()),
                        })
                        .collect(),
                },
                Loop::Vertex { vertex } => ExactBound::Vertex { vertex: *vertex },
            })
            .collect();
        let forward = side.get(i).copied().flatten().ok_or_else(|| refused("a face no shell uses"))?;
        faces.push(ExactFace { id: i as u64, surface, forward, bounds, outer: None });
    }
    Ok(ExactBody {
        tolerance: record.tolerance,
        vertices: record.vertices.iter().map(|v| v.point).collect(),
        edges,
        faces,
        shells,
        kind: kind_of(record),
    })
}

/// How a record's shells make regions.
fn kind_of(record: &Brep) -> ExactKind {
    let bounded: Vec<_> = record.regions.iter().skip(1).collect();
    match bounded.as_slice() {
        [solid] if !solid.void && record.shells.len() == 1 => ExactKind::Solid,
        [solid, voids @ ..] if !solid.void && voids.iter().all(|r| r.void) && solid.shells.len() == record.shells.len() && solid.shells.len() > 1 => {
            ExactKind::Voids
        }
        _ => ExactKind::Shells,
    }
}

/// The record an exact body assembles into, and where each of its faces and edges came from.
pub fn assemble(exact: &ExactBody) -> Result<(Brep, ExactAssembled)> {
    let trims = solidcraft_smlib::pcurves::SmlibImportTrims;
    let mut asm = Assembly::new(exact.tolerance, Some(&trims));
    for p in &exact.vertices {
        asm.add_vertex(*p).map_err(refused)?;
    }
    // Each exact edge's assembly edge, and whether it runs as given.
    let mut edges = Vec::with_capacity(exact.edges.len());
    for (i, e) in exact.edges.iter().enumerate() {
        let made = match e.interval {
            Some(interval) => asm.add_edge(i as u64, e.curve.clone(), interval, e.vertices),
            None => asm.add_edge_on(i as u64, e.curve.clone(), e.vertices),
        };
        edges.push(made.map_err(refused)?);
    }
    let mut faces = Vec::with_capacity(exact.faces.len());
    for (i, f) in exact.faces.iter().enumerate() {
        let mut bounds = Vec::with_capacity(f.bounds.len());
        let mut pcurves = Vec::new();
        for b in &f.bounds {
            match b {
                ExactBound::Edges { uses } => {
                    let mut chain = Vec::with_capacity(uses.len());
                    let mut given = Vec::with_capacity(uses.len());
                    for u in uses {
                        let &(e, along) = edges.get(u.edge as usize).ok_or_else(|| refused(format!("face {i}: an edge reference")))?;
                        chain.push((e, u.forward == along));
                        given.push(u.pcurve.clone());
                    }
                    bounds.push(Bound::Edges(chain));
                    pcurves.push(given);
                }
                ExactBound::Vertex { vertex } => bounds.push(Bound::Vertex(*vertex)),
            }
        }
        let added = if pcurves.iter().flatten().any(Option::is_some) {
            asm.add_face_with_pcurves(i as u64, f.surface.clone(), bounds, f.outer, f.forward, pcurves)
        } else {
            asm.add_face(i as u64, f.surface.clone(), bounds, f.outer, f.forward)
        };
        faces.push((added.map_err(refused)?, f.forward));
    }
    let mut shells = Vec::with_capacity(exact.shells.len());
    for s in &exact.shells {
        shells
            .push(s.iter().map(|&f| faces.get(f as usize).copied().ok_or_else(|| refused("a shell's face reference"))).collect::<Result<Vec<_>>>()?);
    }
    let kind = match exact.kind {
        ExactKind::Solid => Kind::Solid,
        ExactKind::Voids => Kind::Voids,
        ExactKind::Shells => Kind::Model,
    };
    let solid = asm.finish(0, String::new(), shells, kind).map_err(refused)?;
    let from = ExactAssembled {
        faces: solid.face_origin.iter().map(|&o| o as usize).collect(),
        edges: if solid.edge_origin.len() == solid.brep.edges.len() {
            solid.edge_origin.iter().map(|o| o.map(|o| o as usize)).collect()
        } else {
            vec![None; solid.brep.edges.len()]
        },
    };
    Ok((solid.brep, from))
}

/// The history of an exact body in the record's indices: `None` where an item of the record
/// can't be followed back to the exact body's (the record's edges were rebuilt, or an edge the
/// record made separates two faces), so no origin is guessed.
pub fn record_history(history: &ExactHistory, from: &ExactAssembled, record: &Brep) -> Option<History> {
    let mut count = vec![0usize; history.faces.len()];
    for &f in &from.faces {
        *count.get_mut(f)? += 1;
    }
    let faces = from
        .faces
        .iter()
        .map(|&f| {
            let prov = history.faces.get(f)?;
            let split = count.get(f).is_some_and(|n| *n > 1) && prov.change != Change::Generated;
            Some(FaceProv { origins: prov.origins.clone(), change: if split { Change::Split } else { prov.change } })
        })
        .collect::<Option<Vec<_>>>()?;
    if from.edges.len() != record.edges.len() {
        return None;
    }
    let edges = record
        .edges
        .iter()
        .zip(&from.edges)
        .map(|(e, from)| {
            let faces = faces_of(e);
            let origins = match from {
                Some(x) => history.edges.get(*x)?.clone(),
                // Made by the record within one face (a seam): from nothing.
                None if faces.len() == 1 => Vec::new(),
                None => return None,
            };
            Some(solidcore_record::brep::history::EdgeProv { faces, origins })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(History { operands: history.operands, faces, edges })
}

/// A body from an exact body a kernel made from `operands`, its history renumbered into the
/// record's and checked against them; without history where the kernel gave none or it can't
/// be followed.
pub fn body_from_exact(made: ExactExported, operands: &[&Body]) -> Result<Body> {
    let (record, from) = assemble(&made.body)?;
    let history = made.history.as_ref().and_then(|h| record_history(h, &from, &record));
    let body = Body::from_record(record)?;
    match history {
        Some(h) => body.with_history(h, operands),
        None => Ok(body),
    }
}

/// `body` as an exact body with the history that made it, in the exact body's indices (its
/// record's, as it is given in that order).
pub fn exact_exported(body: &Body) -> Result<ExactExported> {
    let exact = exact_body(body)?;
    let history = body.history().map(|h| ExactHistory {
        operands: h.operands,
        faces: h.faces.clone(),
        edges: h.edges.iter().map(|e| e.origins.clone()).collect(),
    });
    Ok(ExactExported { body: exact, history })
}

#[cfg(test)]
#[path = "exact_tests.rs"]
mod tests;
