//! Constant rounds between planes built in closed form, in front of the fillet sessions.
//!
//! A round on a straight edge between two planes, that ends square on a plane at each end or at
//! a corner where three rounds of one radius and convexity meet, is put together here from the
//! kernel's closed forms ([`solidcore::blend::exact_round`]): the result's record is the input's
//! with the rounded edges' faces cut back to the rounds' contact lines, the edges running from
//! a square end cut back to where those lines meet them, each round's face and each corner's
//! patch put in, and nothing traced, intersected or stitched. Every face, edge and vertex the
//! rounds leave keeps its place in the input, so the kernel's attributes (the history's tags
//! among them) carry over to it as the sessions' edits carry them.
//!
//! Which rounds these are is read off the topology and the closed forms before anything is
//! built; the rest go to the fillet sessions as before. [`round`] answers `None` for another
//! surface or curve, a vertex where more edges meet, rounds meeting at a corner of another kind,
//! a round wider than its faces (a contact line crossing another edge, a loop leaving its face)
//! and a round reaching another face of the body.

use solidcore::blend::exact_round::{
    self as x, arc, ball_centre, corner_face, curve_points, grown, interval_has_length,
    keep_definitions, line_parameter, overlap, plane_loops_hold, plane_pcurve, round_face,
    same_point, surface_points, End, KeptCorner, KeptRound, PlaneEdge, Round, RoundFace, Side,
};
use solidcore::eval as k;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::{EdgeId, FaceId, VertexId};
use solidcore::{Brep, EntityRef, Kernel};
use solidcore_convert::brep::{to_kernel, to_record, Unsupported};
use solidcore_convert::{record_curve, record_curve2, record_surface};
use solidcore_record::brep::{self as r, Curve2, Loop, OrientedEdge, OrientedFace, Shell};

/// The rounded body, and what each of its entities stands for in the input.
#[derive(Debug)]
pub(crate) struct Rounded {
    /// The result, in canonical order.
    pub record: r::Brep,
    /// Per result face, the input face it is; `None` for a face the rounds made.
    pub faces: Vec<Option<u32>>,
    /// Per result edge, the input edge it is (whole, or cut back); `None` for a new edge.
    pub edges: Vec<Option<u32>>,
    /// Per result vertex, the input vertex it is; `None` for a new vertex.
    pub vertices: Vec<Option<u32>>,
    /// The result faces the rounds made: each round's face, then each corner's patch.
    pub made: Vec<u32>,
    /// What each face of `made` is, its definition's inputs.
    pub made_as: Vec<Made>,
}

/// What a face the rounds made is.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Made {
    /// A round of `radius` on the input faces `supports` (side 0's, side 1's), its axis
    /// through `centre`.
    Round {
        supports: [u32; 2],
        radius: f64,
        centre: x::V3,
    },
    /// A corner's patch on the ball of `radius` about `centre`.
    Corner { centre: x::V3, radius: f64 },
}

/// One use of an edge on a face.
#[derive(Clone, Copy, Debug)]
struct Use {
    face: u32,
    forward: bool,
}

/// A picked edge and its round.
#[derive(Clone, Copy)]
struct Picked {
    edge: u32,
    /// Its vertices at its start and end.
    va: u32,
    vb: u32,
    /// Its uses: side 0 and side 1.
    uses: [Use; 2],
    round: Round,
}

/// How rounds end at a vertex.
enum EndAt {
    /// The one round `round` ends on the plane `face` square to it, cutting back `edges[k]`
    /// (the edge from the vertex along side k's face).
    Open {
        round: usize,
        face: u32,
        edges: [u32; 2],
    },
    /// Three rounds end at the ball centred at `ball`.
    Corner { rounds: [usize; 3], ball: x::V3 },
}

/// A vertex where rounds end, and how.
struct Ending {
    vertex: u32,
    end: EndAt,
}

/// The input record as the rounds read it: each face's surface and each edge's curve in the
/// kernel's terms, each face's outward side, each edge's uses and each vertex's edges.
struct Input<'a> {
    rec: &'a r::Brep,
    surfaces: Vec<Option<k::Surface>>,
    nets: Vec<Option<k::Surface>>,
    curves: Vec<Option<k::Curve>>,
    forms: Vec<Option<k::Curve>>,
    outward: Vec<bool>,
    uses: Vec<Vec<Use>>,
    at_vertex: Vec<Vec<u32>>,
}

impl Input<'_> {
    fn point(&self, v: u32) -> Option<x::V3> {
        self.rec.vertices.get(v as usize).map(|v| v.point)
    }

    /// Face `f`'s plane and whether its outward normal is the plane's natural one; `None` for a
    /// face on another surface.
    fn plane(&self, f: u32) -> Option<(&k::Surface, bool)> {
        let s = self.surfaces.get(f as usize)?.as_ref()?;
        if self.nets.get(f as usize)?.is_some() || !x::is_plane(s) {
            return None;
        }
        Some((s, *self.outward.get(f as usize)?))
    }

    fn faces_of(&self, e: u32) -> Option<[u32; 2]> {
        match self.uses.get(e as usize)?.as_slice() {
            [a, b] => Some([a.face, b.face]),
            _ => None,
        }
    }
}

/// The record with the edges `picked` (record indices) rounded with `radii` (one, or one per
/// edge), or `None` where this construction does not cover the rounds (they are left to the
/// fillet sessions).
pub(crate) fn round(k: &Kernel, rec: &r::Brep, picked: &[u32], radii: &[f64]) -> Option<Rounded> {
    if picked.is_empty() || !(radii.len() == 1 || radii.len() == picked.len()) {
        return None;
    }
    let input = read(rec)?;
    let mut rounds: Vec<Picked> = Vec::with_capacity(picked.len());
    let mut round_of: Vec<Option<usize>> = vec![None; rec.edges.len()];
    for (i, &e) in picked.iter().enumerate() {
        let radius = *radii.get(if radii.len() == 1 { 0 } else { i })?;
        let slot = round_of.get_mut(e as usize)?;
        if slot.is_some() {
            return None;
        }
        *slot = Some(i);
        let edge = rec.edges.get(e as usize)?;
        if !matches!(edge.curve, r::Curve::Line { .. }) || edge.nurbs.is_some() {
            return None;
        }
        let [va, vb] = edge.vertices;
        if va == vb {
            return None;
        }
        let &[u0, u1] = input.uses.get(e as usize)?.as_slice() else {
            return None;
        };
        if u0.face == u1.face {
            return None;
        }
        let side = |u: Use| -> Option<Side<'_>> {
            let (plane, outward) = input.plane(u.face)?;
            Some(Side {
                plane,
                outward,
                forward: u.forward,
            })
        };
        let round = Round::new(
            input.point(va)?,
            input.point(vb)?,
            [side(u0)?, side(u1)?],
            radius,
        )?;
        rounds.push(Picked {
            edge: e,
            va,
            vb,
            uses: [u0, u1],
            round,
        });
    }
    let (endings, ending_at) = endings(&input, &rounds, &round_of)?;
    if !clear(&input, &rounds, &endings, &ending_at) {
        return None;
    }
    build(k, &input, &rounds, &round_of, &endings, &ending_at)
}

/// The record read for the rounds, `None` for anything but one solid of faces.
fn read(rec: &r::Brep) -> Option<Input<'_>> {
    let [outer, inside] = rec.regions.as_slice() else {
        return None;
    };
    if outer.shells.len() != 1 || inside.shells.len() != 1 || inside.void {
        return None;
    }
    let shell = *inside.shells.first()?;
    if outer.shells.first() != Some(&shell) || rec.shells.len() != 1 {
        return None;
    }
    let Shell::Faces {
        faces: shell_faces,
        wires,
    } = rec.shells.get(shell as usize)?
    else {
        return None;
    };
    if !wires.is_empty() {
        return None;
    }
    let mut outward = vec![None::<bool>; rec.faces.len()];
    for f in shell_faces {
        let slot = outward.get_mut(f.face as usize)?;
        if slot.is_some() {
            return None;
        }
        *slot = Some(f.forward);
    }
    let outward: Vec<bool> = outward.into_iter().collect::<Option<_>>()?;
    let mut uses: Vec<Vec<Use>> = vec![Vec::new(); rec.edges.len()];
    for (fi, f) in rec.faces.iter().enumerate() {
        for l in &f.loops {
            if let Loop::Edges { edges } = l {
                for u in edges {
                    uses.get_mut(u.edge as usize)?.push(Use {
                        face: u32::try_from(fi).ok()?,
                        forward: u.forward,
                    });
                }
            }
        }
    }
    let mut at_vertex: Vec<Vec<u32>> = vec![Vec::new(); rec.vertices.len()];
    for (ei, e) in rec.edges.iter().enumerate() {
        let ei = u32::try_from(ei).ok()?;
        let [s, t] = e.vertices;
        at_vertex.get_mut(s as usize)?.push(ei);
        if t != s {
            at_vertex.get_mut(t as usize)?.push(ei);
        }
    }
    let surfaces = rec
        .faces
        .iter()
        .map(|f| solidcore_convert::surface(&f.surface).ok())
        .collect();
    let nets = rec
        .faces
        .iter()
        .map(|f| {
            f.nurbs
                .as_ref()
                .and_then(|n| solidcore_convert::surface(n).ok())
        })
        .collect();
    let curves = rec
        .edges
        .iter()
        .map(|e| solidcore_convert::curve(&e.curve).ok())
        .collect();
    let forms = rec
        .edges
        .iter()
        .map(|e| {
            e.nurbs
                .as_ref()
                .and_then(|n| solidcore_convert::curve(n).ok())
        })
        .collect();
    Some(Input {
        rec,
        surfaces,
        nets,
        curves,
        forms,
        outward,
        uses,
        at_vertex,
    })
}

/// How the rounds end at each of their vertices, and each vertex's ending.
fn endings(
    input: &Input<'_>,
    rounds: &[Picked],
    round_of: &[Option<usize>],
) -> Option<(Vec<Ending>, Vec<Option<usize>>)> {
    let mut endings: Vec<Ending> = Vec::new();
    let mut ending_at: Vec<Option<usize>> = vec![None; input.rec.vertices.len()];
    for (ri, rd) in rounds.iter().enumerate() {
        for v in [rd.va, rd.vb] {
            if ending_at.get(v as usize).copied().flatten().is_some() {
                continue;
            }
            let here = input.at_vertex.get(v as usize)?;
            if here.len() != 3 {
                return None;
            }
            let rounded: Vec<usize> = here
                .iter()
                .filter_map(|e| round_of.get(*e as usize).copied().flatten())
                .collect();
            let end = match rounded.as_slice() {
                [only] if *only == ri => open_end(input, rd, here, v, ri)?,
                [a, b, c] => corner(input, rounds, [*a, *b, *c], v)?,
                _ => return None,
            };
            *ending_at.get_mut(v as usize)? = Some(endings.len());
            endings.push(Ending { vertex: v, end });
        }
    }
    Some((endings, ending_at))
}

/// The open end of round `ri` (`rd`) at its vertex `v`, where `here` are the vertex's three
/// edges: the two others run from it along the round's faces to one plane square to the edge.
fn open_end(input: &Input<'_>, rd: &Picked, here: &[u32], v: u32, ri: usize) -> Option<EndAt> {
    let others: Vec<u32> = here.iter().copied().filter(|e| *e != rd.edge).collect();
    let [g, h] = others.as_slice() else {
        return None;
    };
    let (fg, fh) = (input.faces_of(*g)?, input.faces_of(*h)?);
    // the face across `faces` from side k's face, where that is one of them
    let across = |faces: [u32; 2], side: usize| -> Option<u32> {
        let f = rd.uses.get(side)?.face;
        if faces.contains(&f) {
            faces.iter().copied().find(|x| *x != f)
        } else {
            None
        }
    };
    let (e0, e1, end0, end1) = match (across(fg, 0), across(fh, 1)) {
        (Some(a), Some(b)) => (*g, *h, a, b),
        _ => match (across(fh, 0), across(fg, 1)) {
            (Some(a), Some(b)) => (*h, *g, a, b),
            _ => return None,
        },
    };
    if end0 != end1 || rd.uses.iter().any(|u| u.face == end0) {
        return None;
    }
    let (plane, outward) = input.plane(end0)?;
    rd.round.open_end(input.point(v)?, plane, outward)?;
    Some(EndAt::Open {
        round: ri,
        face: end0,
        edges: [e0, e1],
    })
}

/// The corner at vertex `v` of the three rounds `three`: one radius and one convexity, between
/// three planes each two of the rounds share.
fn corner(input: &Input<'_>, rounds: &[Picked], three: [usize; 3], v: u32) -> Option<EndAt> {
    let rs: Vec<&Picked> = three
        .iter()
        .map(|i| rounds.get(*i))
        .collect::<Option<_>>()?;
    let first = rs.first()?.round;
    if rs
        .iter()
        .any(|q| q.round.radius() != first.radius() || q.round.convex() != first.convex())
    {
        return None;
    }
    let mut faces: Vec<u32> = Vec::new();
    for q in &rs {
        for u in &q.uses {
            if !faces.contains(&u.face) {
                faces.push(u.face);
            }
        }
    }
    let [f0, f1, f2] = faces.as_slice() else {
        return None;
    };
    let ball = ball_centre(
        input.point(v)?,
        [input.plane(*f0)?, input.plane(*f1)?, input.plane(*f2)?],
        first.convex(),
        first.radius(),
    )?;
    Some(EndAt::Corner {
        rounds: three,
        ball,
    })
}

/// Whether nothing of the body but the faces each round rounds and ends on reaches into the
/// box round the material it takes away or adds.
fn clear(
    input: &Input<'_>,
    rounds: &[Picked],
    endings: &[Ending],
    ending_at: &[Option<usize>],
) -> bool {
    let face_box = |f: usize| -> Option<x::Bounds> {
        let face = input.rec.faces.get(f)?;
        // a face on its NURBS form (a blend's net among them) lies in its net's box
        if let Some(net) = input.nets.get(f)?.as_ref() {
            return x::bounds(&surface_points(net, None)?);
        }
        let surface = input.surfaces.get(f)?.as_ref()?;
        if let Some(points) = surface_points(surface, None) {
            return x::bounds(&points);
        }
        if !x::is_plane(surface) {
            return None;
        }
        let mut points = Vec::new();
        for l in &face.loops {
            match l {
                Loop::Edges { edges } => {
                    for u in edges {
                        let e = input.rec.edges.get(u.edge as usize)?;
                        points.extend(curve_points(
                            input.curves.get(u.edge as usize)?.as_ref()?,
                            input.forms.get(u.edge as usize)?.as_ref(),
                            e.interval,
                        )?);
                    }
                }
                Loop::Vertex { vertex } => points.push(input.point(*vertex)?),
            }
        }
        x::bounds(&points)
    };
    let boxes: Vec<Option<x::Bounds>> = (0..input.rec.faces.len()).map(face_box).collect();
    rounds.iter().all(|rd| {
        let mut own: Vec<u32> = rd.uses.iter().map(|u| u.face).collect();
        for v in [rd.va, rd.vb] {
            let Some(ending) = ending_at
                .get(v as usize)
                .copied()
                .flatten()
                .and_then(|i| endings.get(i))
            else {
                return false;
            };
            match &ending.end {
                EndAt::Open { face, .. } => own.push(*face),
                EndAt::Corner { rounds: three, .. } => {
                    for q in three.iter().filter_map(|q| rounds.get(*q)) {
                        own.extend(q.uses.iter().map(|u| u.face));
                    }
                }
            }
        }
        let Some(reach) = input.point(rd.vb).and_then(|b| rd.round.reach(b)) else {
            return false;
        };
        let margin = x::margin(rd.round.radius(), input.rec.tolerance);
        boxes.iter().enumerate().all(|(fi, fb)| {
            u32::try_from(fi).is_ok_and(|f| own.contains(&f))
                || fb.as_ref().is_some_and(|fb| !overlap(&reach, fb, margin))
        })
    })
}

/// A kernel curve as the record's, `None` where the record cannot hold it.
fn rc(c: k::Curve) -> Option<r::Curve> {
    record_curve(c).ok()
}

/// A pcurve [`replane`] works out: an arc put into its end face's loop before that face's
/// pcurves are, over the arc's [0, 1].
fn placeholder() -> Curve2 {
    Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[0.0, 0.0], [1.0, 1.0]],
    }
}

/// Each round's ends (at its start and its end), `None` where one does not hold or a round
/// would not run forward between them.
fn round_ends(input: &Input<'_>, rounds: &[Picked], endings: &[Ending]) -> Option<Vec<[End; 2]>> {
    let mut ends: Vec<[Option<End>; 2]> = vec![[None, None]; rounds.len()];
    for ending in endings {
        let p = input.point(ending.vertex)?;
        match &ending.end {
            EndAt::Open { round, face, .. } => {
                let rd = rounds.get(*round)?;
                let (plane, outward) = input.plane(*face)?;
                let which = usize::from(ending.vertex == rd.vb);
                *ends.get_mut(*round)?.get_mut(which)? =
                    Some(rd.round.open_end(p, plane, outward)?);
            }
            EndAt::Corner {
                rounds: three,
                ball,
            } => {
                for &ri in three {
                    let rd = rounds.get(ri)?;
                    let which = usize::from(ending.vertex == rd.vb);
                    *ends.get_mut(ri)?.get_mut(which)? = Some(rd.round.corner_end(*ball)?);
                }
            }
        }
    }
    let ends: Vec<[End; 2]> = ends
        .into_iter()
        .map(|[a, b]| Some([a?, b?]))
        .collect::<Option<_>>()?;
    rounds
        .iter()
        .zip(&ends)
        .all(|(rd, [e0, e1])| rd.round.runs_between([e0, e1]))
        .then_some(ends)
}

/// Builds the result record from the rounds and their endings.
#[allow(clippy::too_many_lines)]
fn build(
    k: &Kernel,
    input: &Input<'_>,
    rounds: &[Picked],
    round_of: &[Option<usize>],
    endings: &[Ending],
    ending_at: &[Option<usize>],
) -> Option<Rounded> {
    let rec = input.rec;
    let tolerance = rec.tolerance;
    let made_tolerance = x::made_tolerance(tolerance);
    let ends = round_ends(input, rounds, endings)?;
    // each round's face, between its ends: open ones square on a plane, the rest at corners
    let open = |v: u32| {
        ending_at
            .get(v as usize)
            .copied()
            .flatten()
            .and_then(|i| endings.get(i))
            .is_some_and(|e| matches!(e.end, EndAt::Open { .. }))
    };
    let round_faces: Vec<RoundFace> = rounds
        .iter()
        .zip(&ends)
        .map(|(rd, [e0, e1])| round_face(&rd.round, [e0, e1], [open(rd.va), open(rd.vb)]))
        .collect::<Option<_>>()?;

    // Vertices: the input's but those where rounds end; then the new ones, each round's end
    // contacts, shared at a corner by the two rounds along each face.
    let mut vertices: Vec<r::Vertex> = Vec::new();
    let mut vertex_source: Vec<Option<u32>> = Vec::new();
    let mut new_vertex_of: Vec<Option<u32>> = vec![None; rec.vertices.len()];
    for (vi, v) in rec.vertices.iter().enumerate() {
        if ending_at.get(vi).copied().flatten().is_some() {
            continue;
        }
        *new_vertex_of.get_mut(vi)? = Some(u32::try_from(vertices.len()).ok()?);
        vertices.push(v.clone());
        vertex_source.push(Some(u32::try_from(vi).ok()?));
    }
    // end_vertex[round][end][side]; cut[edge]: (its end, the new vertex, the new point)
    let mut end_vertex: Vec<[[u32; 2]; 2]> = vec![[[0; 2]; 2]; rounds.len()];
    let mut cut: Vec<Vec<(usize, u32, x::V3)>> = vec![Vec::new(); rec.edges.len()];
    // each corner's contact vertex on each of its faces: (face, vertex, point)
    let mut corner_vertex: Vec<Vec<(u32, u32, x::V3)>> = vec![Vec::new(); endings.len()];
    for (ei, ending) in endings.iter().enumerate() {
        match &ending.end {
            EndAt::Open { round, edges, .. } => {
                let rd = rounds.get(*round)?;
                let which = usize::from(ending.vertex == rd.vb);
                let end = ends.get(*round)?.get(which)?;
                for (side, (&edge, &q)) in edges.iter().zip(&end.contacts).enumerate() {
                    let e = rec.edges.get(edge as usize)?;
                    let at = usize::from(e.vertices.get(1) == Some(&ending.vertex));
                    if e.vertices.get(at) != Some(&ending.vertex) {
                        return None;
                    }
                    let nv = u32::try_from(vertices.len()).ok()?;
                    vertices.push(r::Vertex {
                        point: q,
                        tolerance: made_tolerance,
                    });
                    vertex_source.push(None);
                    cut.get_mut(edge as usize)?.push((at, nv, q));
                    *end_vertex.get_mut(*round)?.get_mut(which)?.get_mut(side)? = nv;
                }
            }
            EndAt::Corner { rounds: three, .. } => {
                for &ri in three {
                    let rd = rounds.get(ri)?;
                    let which = usize::from(ending.vertex == rd.vb);
                    let end = ends.get(ri)?.get(which)?;
                    for (side, &q) in end.contacts.iter().enumerate() {
                        let face = rd.uses.get(side)?.face;
                        let made = corner_vertex.get(ei)?.iter().find(|(f, _, _)| *f == face);
                        let nv = if let Some(&(_, nv, at)) = made {
                            if !same_point(at, q, rd.round.radius()) {
                                return None;
                            }
                            nv
                        } else {
                            let nv = u32::try_from(vertices.len()).ok()?;
                            vertices.push(r::Vertex {
                                point: q,
                                tolerance: made_tolerance,
                            });
                            vertex_source.push(None);
                            corner_vertex.get_mut(ei)?.push((face, nv, q));
                            nv
                        };
                        *end_vertex.get_mut(ri)?.get_mut(which)?.get_mut(side)? = nv;
                    }
                }
            }
        }
    }

    // Edges: the input's, less the picked ones, the cut-back ones moved; then each round's two
    // contact lines and its two end arcs.
    let mut edges: Vec<r::Edge> = Vec::new();
    let mut edge_source: Vec<Option<u32>> = Vec::new();
    let mut new_edge_of: Vec<Option<u32>> = vec![None; rec.edges.len()];
    for (ei, e) in rec.edges.iter().enumerate() {
        if round_of.get(ei).copied().flatten().is_some() {
            continue;
        }
        let mut e = e.clone();
        e.uses = Vec::new();
        let cuts = cut.get(ei)?;
        for &(at, nv, q) in cuts {
            if e.nurbs.is_some() {
                return None;
            }
            let t = line_parameter(input.curves.get(ei)?.as_ref()?, q)?;
            *e.interval.get_mut(at)? = t;
            *e.vertices.get_mut(at)? = nv;
        }
        if !cuts.is_empty() && !interval_has_length(e.interval) {
            return None;
        }
        for at in 0..2 {
            if cuts.iter().any(|(c, _, _)| *c == at) {
                continue;
            }
            let v = e.vertices.get_mut(at)?;
            *v = new_vertex_of.get(*v as usize).copied().flatten()?;
        }
        *new_edge_of.get_mut(ei)? = Some(u32::try_from(edges.len()).ok()?);
        edges.push(e);
        edge_source.push(Some(u32::try_from(ei).ok()?));
    }
    // contact_edge[round][side], arc_edge[round][end]
    let mut contact_edge: Vec<[u32; 2]> = vec![[0; 2]; rounds.len()];
    let mut arc_edge: Vec<[u32; 2]> = vec![[0; 2]; rounds.len()];
    for (ri, rd) in rounds.iter().enumerate() {
        let tol = rec
            .edges
            .get(rd.edge as usize)?
            .tolerance
            .max(made_tolerance);
        let [e0, e1] = ends.get(ri)?;
        let [v0, v1] = *end_vertex.get(ri)?;
        let rf = round_faces.get(ri)?;
        for (side, curve) in rf.lines.iter().enumerate() {
            *contact_edge.get_mut(ri)?.get_mut(side)? = u32::try_from(edges.len()).ok()?;
            edges.push(r::Edge {
                curve: rc(curve.clone())?,
                nurbs: None,
                interval: rf.interval,
                vertices: [*v0.get(side)?, *v1.get(side)?],
                tolerance: tol,
                uses: Vec::new(),
            });
            edge_source.push(None);
        }
        // each end arc runs the way the face's loop does: the start's the way u does
        for (which, (end, vs)) in [(e0, v0), (e1, v1)].into_iter().enumerate() {
            let [mut p0, mut p1] = end.contacts;
            let [mut s0, mut s1] = vs;
            if (rf.u0_side == 1) != (which == 1) {
                (p0, p1, s0, s1) = (p1, p0, s1, s0);
            }
            let (circle, form) = arc(end.centre, rd.round.radius(), p0, p1)?;
            *arc_edge.get_mut(ri)?.get_mut(which)? = u32::try_from(edges.len()).ok()?;
            edges.push(r::Edge {
                curve: rc(circle)?,
                nurbs: Some(rc(form)?),
                interval: [0.0, 1.0],
                vertices: [s0, s1],
                tolerance: tol,
                uses: Vec::new(),
            });
            edge_source.push(None);
        }
    }
    // the edges the rounds made or cut back
    let changed: Vec<bool> = edge_source
        .iter()
        .zip(&edges)
        .map(|(src, e)| match src {
            None => true,
            Some(old) => rec
                .edges
                .get(*old as usize)
                .is_none_or(|o| o.interval != e.interval),
        })
        .collect();

    // Faces: the input's, their loops with the edges replaced and the end arcs put in; then each
    // round's face and each corner's patch.
    let mut faces: Vec<r::Face> = Vec::new();
    let mut face_source: Vec<Option<u32>> = Vec::new();
    let mut shell: Vec<OrientedFace> = Vec::new();
    let Shell::Faces {
        faces: input_shell, ..
    } = rec.shells.first()?
    else {
        return None;
    };
    // the arc that goes in at an open end on its end face: (end face, arc edge)
    let open_arc = |vertex: u32| -> Option<(u32, u32)> {
        let ei = ending_at.get(vertex as usize).copied().flatten()?;
        let EndAt::Open { round, face, .. } = endings.get(ei)?.end else {
            return None;
        };
        let rd = rounds.get(round)?;
        let which = usize::from(vertex == rd.vb);
        Some((face, *arc_edge.get(round)?.get(which)?))
    };
    for of in input_shell {
        let fi = of.face;
        let f = rec.faces.get(fi as usize)?;
        let mut touched = false;
        let mut loops = Vec::with_capacity(f.loops.len());
        for l in &f.loops {
            let old = match l {
                Loop::Edges { edges } => edges,
                Loop::Vertex { vertex } => {
                    loops.push(Loop::Vertex {
                        vertex: new_vertex_of.get(*vertex as usize).copied().flatten()?,
                    });
                    continue;
                }
            };
            let mut out: Vec<OrientedEdge> = Vec::with_capacity(old.len());
            for u in old {
                let edge = if let Some(ri) = round_of.get(u.edge as usize).copied().flatten() {
                    let rd = rounds.get(ri)?;
                    let side = rd.uses.iter().position(|x| x.face == fi)?;
                    touched = true;
                    *contact_edge.get(ri)?.get(side)?
                } else {
                    touched |= !cut.get(u.edge as usize)?.is_empty();
                    new_edge_of.get(u.edge as usize).copied().flatten()?
                };
                out.push(OrientedEdge {
                    edge,
                    forward: u.forward,
                    pcurve: u.pcurve.clone(),
                });
            }
            // an open end's arc between the two cut-back edges on its end face
            let n = out.len();
            let mut with_arcs: Vec<OrientedEdge> = Vec::with_capacity(n + 1);
            for i in 0..n {
                let cur = out.get(i)?.clone();
                let stop = end_of(&edges, &cur)?;
                let start = start_of(&edges, out.get((i + 1) % n)?)?;
                with_arcs.push(cur);
                if stop != start {
                    // the input's vertex between them was an open end on this face
                    let old_u = old.get(i)?;
                    let [s, t] = rec.edges.get(old_u.edge as usize)?.vertices;
                    let (face, a) = open_arc(if old_u.forward { t } else { s })?;
                    let [v0, v1] = edges.get(a as usize)?.vertices;
                    if face != fi {
                        return None;
                    }
                    let forward = if (v0, v1) == (stop, start) {
                        true
                    } else if (v1, v0) == (stop, start) {
                        false
                    } else {
                        return None;
                    };
                    touched = true;
                    with_arcs.push(OrientedEdge {
                        edge: a,
                        forward,
                        pcurve: placeholder(),
                    });
                }
            }
            loops.push(Loop::Edges { edges: with_arcs });
        }
        let mut face = r::Face {
            surface: f.surface.clone(),
            nurbs: f.nurbs.clone(),
            domain: f.domain,
            loops,
        };
        if touched {
            // a plane face the rounds change: its pcurves anew, its domain round its loops,
            // which must still be simple and nested as they were
            let (plane, _) = input.plane(fi)?;
            replane(&mut face, plane, &edges, &vertices, &changed, tolerance)?;
        }
        face_source.push(Some(fi));
        shell.push(OrientedFace {
            face: u32::try_from(faces.len()).ok()?,
            forward: of.forward,
        });
        faces.push(face);
    }
    let mut made = Vec::new();
    let mut made_as = Vec::new();
    for (ri, (rd, rf)) in rounds.iter().zip(round_faces).enumerate() {
        let [k0, k1] = *contact_edge.get(ri)?;
        let [a0, a1] = *arc_edge.get(ri)?;
        let [arc0, arc1] = rf.arcs.clone().map(record_curve2);
        let [line0, line1] = rf.contacts.clone().map(record_curve2);
        // counter-clockwise in (u, v): the start's arc from u = 0 to 1, up the contact at u = 1,
        // the end's arc back, down the contact at u = 0
        let ((k_at0, l_at0), (k_at1, l_at1)) = if rf.u0_side == 0 {
            ((k0, line0), (k1, line1))
        } else {
            ((k1, line1), (k0, line0))
        };
        let uses = vec![
            OrientedEdge {
                edge: a0,
                forward: true,
                pcurve: arc0,
            },
            OrientedEdge {
                edge: k_at1,
                forward: true,
                pcurve: l_at1,
            },
            OrientedEdge {
                edge: a1,
                forward: true,
                pcurve: arc1,
            },
            OrientedEdge {
                edge: k_at0,
                forward: false,
                pcurve: l_at0,
            },
        ];
        let fi = u32::try_from(faces.len()).ok()?;
        faces.push(r::Face {
            surface: record_surface(rf.label).ok()?,
            nurbs: Some(record_surface(rf.net).ok()?),
            domain: rf.domain,
            loops: vec![Loop::Edges { edges: uses }],
        });
        face_source.push(None);
        // the net's normal points away from the axis: out of the material at a convex edge
        shell.push(OrientedFace {
            face: fi,
            forward: rd.round.convex(),
        });
        made.push(fi);
        made_as.push(Made::Round {
            supports: [rd.uses[0].face, rd.uses[1].face],
            radius: rd.round.radius(),
            centre: ends.get(ri)?[0].centre,
        });
    }
    // each corner's patch: the ball's triangle between its three contacts, bounded by the three
    // rounds' end arcs there
    for (ei, ending) in endings.iter().enumerate() {
        let EndAt::Corner {
            rounds: three,
            ball,
        } = ending.end
        else {
            continue;
        };
        let first = rounds.get(*three.first()?)?.round;
        let contacts: Vec<x::V3> = corner_vertex.get(ei)?.iter().map(|c| c.2).collect();
        let contacts = <[x::V3; 3]>::try_from(contacts).ok()?;
        let patch = corner_face(k, ball, first.radius(), contacts, first.convex()).ok()??;
        // the arcs, in a loop
        let mut arcs: Vec<(u32, u32, u32)> = Vec::with_capacity(3);
        for &ri in &three {
            let rd = rounds.get(ri)?;
            let which = usize::from(ending.vertex == rd.vb);
            let a = *arc_edge.get(ri)?.get(which)?;
            let [v0, v1] = edges.get(a as usize)?.vertices;
            arcs.push((a, v0, v1));
        }
        let &(a0, s0, t0) = arcs.first()?;
        let mut walk = vec![(a0, true, s0, t0)];
        let mut at = t0;
        for _ in 0..2 {
            let &(a, x0, x1) = arcs
                .iter()
                .find(|(e, x0, x1)| !walk.iter().any(|w| w.0 == *e) && (*x0 == at || *x1 == at))?;
            let forward = x0 == at;
            at = if forward { x1 } else { x0 };
            walk.push((a, forward, x0, x1));
        }
        if at != s0 {
            return None;
        }
        let corners: Vec<x::V3> = walk
            .iter()
            .map(|&(_, forward, x0, x1)| {
                vertices
                    .get((if forward { x0 } else { x1 }) as usize)
                    .map(|v| v.point)
            })
            .collect::<Option<_>>()?;
        if patch.turns(<[x::V3; 3]>::try_from(corners).ok()?)? {
            walk.reverse();
            for w in &mut walk {
                w.1 = !w.1;
            }
        }
        let mut uses = Vec::with_capacity(3);
        for &(edge, forward, _, _) in &walk {
            let form = solidcore_convert::curve(edges.get(edge as usize)?.nurbs.as_ref()?).ok()?;
            uses.push(OrientedEdge {
                edge,
                forward,
                pcurve: record_curve2(patch.pcurve(&form)?),
            });
        }
        let fi = u32::try_from(faces.len()).ok()?;
        faces.push(r::Face {
            surface: record_surface(patch.label.clone()).ok()?,
            nurbs: Some(record_surface(patch.net.clone()).ok()?),
            domain: patch.domain,
            loops: vec![Loop::Edges { edges: uses }],
        });
        face_source.push(None);
        shell.push(OrientedFace {
            face: fi,
            forward: true,
        });
        made.push(fi);
        made_as.push(Made::Corner {
            centre: ball,
            radius: first.radius(),
        });
    }

    let built = r::Brep {
        version: rec.version,
        tolerance: rec.tolerance,
        regions: rec.regions.clone(),
        shells: vec![Shell::Faces {
            faces: shell,
            wires: Vec::new(),
        }],
        faces,
        edges,
        vertices,
    };
    let part = built.canonical().ok()?;
    let source = |list: &[u32], from: &[Option<u32>]| -> Vec<Option<u32>> {
        list.iter()
            .map(|&i| from.get(i as usize).copied().flatten())
            .collect()
    };
    let made = made
        .iter()
        .map(|f| {
            part.faces
                .iter()
                .position(|x| x == f)
                .and_then(|p| u32::try_from(p).ok())
        })
        .collect::<Option<_>>()?;
    Some(Rounded {
        faces: source(&part.faces, &face_source),
        edges: source(&part.edges, &edge_source),
        vertices: source(&part.vertices, &vertex_source),
        made,
        made_as,
        record: part.brep,
    })
}

/// The vertex a use ends at.
fn end_of(edges: &[r::Edge], u: &OrientedEdge) -> Option<u32> {
    let [s, t] = edges.get(u.edge as usize)?.vertices;
    Some(if u.forward { t } else { s })
}

/// The vertex a use starts at.
fn start_of(edges: &[r::Edge], u: &OrientedEdge) -> Option<u32> {
    let [s, t] = edges.get(u.edge as usize)?.vertices;
    Some(if u.forward { s } else { t })
}

/// A plane face the rounds change: the pcurves of the edges the rounds made or cut back worked
/// out on its plane, its domain grown round its edges, and its loops checked to be simple and
/// nested still (`None` where they are not).
fn replane(
    face: &mut r::Face,
    plane: &k::Surface,
    edges: &[r::Edge],
    vertices: &[r::Vertex],
    changed: &[bool],
    tolerance: f64,
) -> Option<()> {
    // per use: its curve and form, interval, vertices and their points, changed, loop
    type Use = (
        k::Curve,
        Option<k::Curve>,
        [f64; 2],
        [u32; 2],
        [x::V3; 2],
        bool,
        usize,
    );
    let mut domain = face.domain;
    let mut all: Vec<Use> = Vec::new();
    for (li, l) in face.loops.iter_mut().enumerate() {
        let Loop::Edges { edges: uses } = l else {
            continue;
        };
        for u in uses {
            let e = edges.get(u.edge as usize)?;
            let curve = solidcore_convert::curve(&e.curve).ok()?;
            let form = match &e.nurbs {
                Some(n) => Some(solidcore_convert::curve(n).ok()?),
                None => None,
            };
            let is_changed = changed.get(u.edge as usize).copied().unwrap_or(true);
            match plane_pcurve(plane, &curve, form.as_ref(), e.interval) {
                Some((pcurve, points)) => {
                    if is_changed {
                        u.pcurve = record_curve2(pcurve);
                    }
                    domain = grown(domain, &points);
                }
                None if is_changed => return None,
                None => {}
            }
            let [s, t] = e.vertices;
            let points = [
                vertices.get(s as usize)?.point,
                vertices.get(t as usize)?.point,
            ];
            all.push((curve, form, e.interval, e.vertices, points, is_changed, li));
        }
    }
    let pieces: Vec<PlaneEdge<'_>> = all
        .iter()
        .map(
            |(curve, form, interval, vs, points, is_changed, li)| PlaneEdge {
                curve,
                nurbs: form.as_ref(),
                interval: *interval,
                vertices: *vs,
                points: *points,
                changed: *is_changed,
                r#loop: *li,
            },
        )
        .collect();
    if !plane_loops_hold(plane, domain, &pieces, tolerance)? {
        return None;
    }
    face.domain = domain;
    Some(())
}

/// The rounds on the kernel's body `b`: the edges `picked` rounded with `radii` (one, or one per
/// edge) as [`round`] builds them, every entity the result keeps from `b` carrying `b`'s
/// attributes, and the faces the rounds made. `None` where [`round`] does not cover them, `b`
/// untouched.
///
/// # Errors
/// A cancellation, and the kernel's failures copying `b` or putting attributes on the result.
pub(super) fn round_on(
    k: &mut Kernel,
    b: &Brep,
    picked: &[EdgeId],
    radii: &[f64],
) -> solidcore::Result<Option<(Brep, Vec<FaceId>)>> {
    // the record is read off a copy: making it fills in missing trims, which the sessions
    // would otherwise not see
    let mut copy = brep_copy(k, b)?;
    if copy.edge_list().len() != b.edge_list().len() {
        return Ok(None);
    }
    let (rec, order) = match to_record(k, &mut copy) {
        Ok(x) => x,
        Err(Unsupported::Kernel(e)) if e.is_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    // the copy lists its edges as `b` does, each at the place of the one it copies
    let Some(at): Option<Vec<u32>> = picked
        .iter()
        .map(|e| {
            let place = b.edge_list().iter().position(|x| x == e)?;
            let copied = copy.edge_list().get(place)?;
            order
                .edges
                .iter()
                .position(|x| x == copied)
                .and_then(|p| u32::try_from(p).ok())
        })
        .collect()
    else {
        return Ok(None);
    };
    let Some(rounded) = round(k, &rec, &at, radii) else {
        return Ok(None);
    };
    let (mut out, made_order) = match to_kernel(k, &rounded.record, true) {
        Ok(x) => x,
        Err(Unsupported::Kernel(e)) if e.is_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    let pairs = |sources: &[Option<u32>], from: &[EntityRef], to: &[EntityRef]| {
        sources
            .iter()
            .zip(to)
            .filter_map(|(s, t)| s.and_then(|s| from.get(s as usize)).map(|f| (*f, *t)))
            .collect::<Vec<_>>()
    };
    let faces = |l: &[FaceId]| l.iter().map(|f| EntityRef::Face(*f)).collect::<Vec<_>>();
    let edges = |l: &[EdgeId]| l.iter().map(|e| EntityRef::Edge(*e)).collect::<Vec<_>>();
    let vertices = |l: &[VertexId]| l.iter().map(|v| EntityRef::Vertex(*v)).collect::<Vec<_>>();
    let mut moves = pairs(
        &rounded.faces,
        &faces(&order.faces),
        &faces(&made_order.faces),
    );
    moves.extend(pairs(
        &rounded.edges,
        &edges(&order.edges),
        &edges(&made_order.edges),
    ));
    moves.extend(pairs(
        &rounded.vertices,
        &vertices(&order.vertices),
        &vertices(&made_order.vertices),
    ));
    for (from, to) in moves {
        for (_, a) in copy.entity_attributes(from) {
            out.add_attribute(to, a.make_copy())?;
        }
    }
    let made: Vec<FaceId> = rounded
        .made
        .iter()
        .filter_map(|f| made_order.faces.get(*f as usize).copied())
        .collect();
    // each round and corner keeps its definition, as the sessions' do
    let result_face = |input: u32| {
        rounded
            .faces
            .iter()
            .position(|f| *f == Some(input))
            .and_then(|i| made_order.faces.get(i).copied())
    };
    let (mut kept_rounds, mut kept_corners) = (Vec::new(), Vec::new());
    for (&face, made_as) in made.iter().zip(&rounded.made_as) {
        match *made_as {
            Made::Round {
                supports: [s0, s1],
                radius,
                centre,
            } => {
                let (Some(a), Some(b)) = (result_face(s0), result_face(s1)) else {
                    return Ok(None);
                };
                kept_rounds.push(KeptRound {
                    face,
                    supports: [a, b],
                    radius,
                    centre,
                });
            }
            Made::Corner { centre, radius } => kept_corners.push(KeptCorner {
                face,
                centre,
                radius,
            }),
        }
    }
    match keep_definitions(k, &mut out, &kept_rounds, &kept_corners) {
        Ok(()) => {}
        Err(e) if e.is_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    }
    Ok(Some((out, made)))
}

#[cfg(test)]
mod tests;
