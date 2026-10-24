//! SMLib's own pcurves for a face's uses that have no closed form (an approximated intersection
//! curve on a cone, a file's spline edge on a spline surface): the face goes to SMLib alone, an
//! open sheet, with the trims it has and without the others, and SMLib makes those as it makes
//! every trim it needs (`SmEdgeuse::GetOrCreateUVTrimCurve`, when the body is exported). They are
//! SMLib's, relayed as it holds them on the record's parameters; nothing here fits a curve.

use std::collections::HashSet;

use solidcraft_geom::brep::pcurve::{TrimFace, Trims};
use solidcraft_geom::brep::{self as b, Curve2, EdgeUseRef, Loop, OrientedEdge, OrientedFace, Region, Shell, Vertex};

/// [`Trims`] by SMLib.
#[derive(Clone, Copy, Debug, Default)]
pub struct SmlibTrims;

impl Trims for SmlibTrims {
    fn trims(&self, face: &TrimFace<'_>) -> Result<Vec<Vec<Curve2>>, String> {
        trims(face).map_err(|e| format!("SMLib pcurves: {e}"))
    }
}

fn trims(face: &TrimFace<'_>) -> Result<Vec<Vec<Curve2>>, String> {
    let (record, bare) = sheet(face)?;
    let arrays = crate::import::arrays_with_bare_uses(&record, &bare).map_err(|e| e.to_string())?;
    let body = crate::flat::Flat::from_arrays(&arrays).and_then(|f| f.import()).map_err(|e| e.to_string())?;
    let (out, order) = body.to_native_ordered().map_err(|e| e.to_string())?;
    let [made] = out.faces.as_slice() else { return Err(format!("SMLib made {} faces of one", out.faces.len())) };
    let made_uses: Vec<&OrientedEdge> = made
        .loops
        .iter()
        .flat_map(|l| match l {
            Loop::Edges { edges } => edges.as_slice(),
            Loop::Vertex { .. } => &[],
        })
        .collect();
    // SMLib keeps the edges in the order they came; the record renumbers them.
    let edge_of = |u: u32| order.edges.get(u as usize).copied().ok_or("an edge SMLib lost");
    // A sheet's loops come back the way SMLib runs them, which may be against ours: a use is the
    // same pcurve either way (on its edge's interval in its curve's direction), and the loop's
    // way round is read from an edge the face uses once.
    let ours = face.loops.iter().flatten();
    let once = ours.clone().find(|u| ours.clone().filter(|v| v.edge == u.edge).count() == 1).ok_or("a face of seam edges only")?;
    let at = edge_of(once.edge)?;
    let reversed = made_uses.iter().find(|m| m.edge as usize == at).ok_or("a use SMLib did not keep")?.forward != once.forward;
    let mut result = Vec::with_capacity(face.loops.len());
    for l in &face.loops {
        let mut pcs = Vec::with_capacity(l.len());
        for u in l {
            let e = edge_of(u.edge)?;
            let edge = out.edges.get(e).ok_or("an edge SMLib lost")?;
            let ours = face.edges.get(u.edge as usize).ok_or("edge reference")?;
            let slack = 1e-9 * ours.interval.iter().fold(1.0f64, |m, t| m.max(t.abs()));
            if (0..2).any(|k| (edge.interval[k] - ours.interval[k]).abs() > slack) {
                return Err(format!("SMLib moved an edge's interval from {:?} to {:?}", ours.interval, edge.interval));
            }
            let found = made_uses.iter().find(|m| m.edge as usize == e && m.forward == (u.forward != reversed));
            pcs.push(found.ok_or("a use SMLib did not keep")?.pcurve.clone());
        }
        result.push(pcs);
    }
    Ok(result)
}

/// The face as a one-face open sheet, and its uses that go without trims.
fn sheet(face: &TrimFace<'_>) -> Result<(b::Brep, HashSet<(u32, u32, u32)>), String> {
    let mut edges: Vec<b::Edge> = face
        .edges
        .iter()
        .map(|e| b::Edge {
            curve: e.curve.clone(),
            nurbs: e.nurbs.cloned(),
            interval: e.interval,
            vertices: e.vertices,
            tolerance: e.tolerance,
            uses: Vec::new(),
        })
        .collect();
    let mut bare = HashSet::new();
    let mut loops = Vec::with_capacity(face.loops.len());
    for (li, l) in face.loops.iter().enumerate() {
        let li = u32::try_from(li).map_err(|_| "too many loops")?;
        let mut uses = Vec::with_capacity(l.len());
        for (p, u) in l.iter().enumerate() {
            let p = u32::try_from(p).map_err(|_| "too many uses")?;
            let edge = edges.get_mut(u.edge as usize).ok_or("edge reference")?;
            edge.uses.push(EdgeUseRef { face: 0, r#loop: li, position: p, upper: !u.forward });
            let pcurve = match &u.pcurve {
                Some(pc) => pc.clone(),
                None => {
                    bare.insert((0, li, p));
                    // Not read: the use goes without a trim (`arrays_with_bare_uses`).
                    Curve2::Spline { knots: vec![edge.interval[0], edge.interval[0], edge.interval[1], edge.interval[1]], points: vec![[0.0; 2]; 2] }
                }
            };
            uses.push(OrientedEdge { edge: u.edge, forward: u.forward, pcurve });
        }
        loops.push(Loop::Edges { edges: uses });
    }
    let vertices = face.vertices.iter().map(|&point| Vertex { point, tolerance: face.tolerance }).collect();
    let record = b::Brep {
        version: b::VERSION,
        tolerance: face.tolerance,
        regions: vec![Region { shells: vec![0], void: false }],
        shells: vec![Shell::Faces { faces: vec![OrientedFace { face: 0, forward: true }], wires: Vec::new() }],
        faces: vec![b::Face { surface: face.surface.clone(), nurbs: face.nurbs.cloned(), domain: face.domain, loops }],
        edges,
        vertices,
    };
    Ok((record, bare))
}
