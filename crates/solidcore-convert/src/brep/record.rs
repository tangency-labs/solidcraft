//! The kernel's body read back as the record ([`to_record`]): SolidCraft's flat-array export
//! (the body as flat arrays, the arrays as the record) fused into one pass over the kernel's
//! B-rep data, with no arrays between. What is done here is index mapping and field copies:
//!
//! - **Geometry:** each edge's curve and each face's surface as the kernel writes its record
//!   ([`record_of`], [`record_face`]): a line by its two end points, a conic by its own frame and
//!   its NURBS form, proved on each other; an analytic face by its label and its NURBS form,
//!   proved (`lies_on`); a plane by its placement, its parameters its kernel ones times its
//!   scales. Nothing is sampled, and nothing is approximated: a curve or surface with no exact
//!   record is refused.
//! - **Pcurves:** every use's pcurve is the kernel's own UV trim (made first where a use has
//!   none), its knots on the edge record's interval ([`record_trim`]). A face's parameters in
//!   the record are its NURBS's (or a plane's, scaled), none periodic, so no loop is moved by
//!   periods.
//! - **Sides:** the kernel keeps a face's loops relative to its NURBS's natural normal, which is
//!   the record's except on a plane whose scales have opposite signs: those loops are turned
//!   over (the kernel says so; no area is measured).
//! - **Shells and regions:** a closed boundary's two runtime shells become one record, its faces
//!   as the side in the region that is not material uses them; a sheet's one shell keeps the
//!   `same` use of each face; the infinite region is never a void.
//! - **Touching:** an edge or vertex solids touching each other share is copied once per solid;
//!   an edge one solid touches itself along (four or more uses in a shell) is copied once per
//!   pair of faces the kernel's radial order puts across one sector round it.
//! - **Order:** faces, edges and vertices renumbered in the record's canonical walk, each edge's
//!   uses from that walk (`upper = !forward`); [`Order`] names each record entity's kernel one.
//! - **Tolerances:** each edge's and vertex's own (the largest of its own, the body's and its
//!   faces'), the body's the least of them.

use std::collections::HashMap;

use solidcore::curve::from_parameters::{intersection_tolerance, record_of, record_trim};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::Kernel;
use solidcore::surface::face_surface::record_face;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep_data::{BrepData, NO_OBJECT, UNDEF_ULONG};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edgeuse::{p1 as eu1, p3 as eu3};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, EdgeId, FaceId};
use solidcore_record::brep as r;

use super::{Order, Unsupported, R};

fn bad(what: &'static str) -> Unsupported {
    Unsupported::Invalid(what)
}

fn index(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| bad("index"))
}

fn at<T>(v: &[T], i: usize) -> R<&T> {
    v.get(i).ok_or(bad("index past its list"))
}

fn data_index(x: u64) -> R<usize> {
    usize::try_from(x).map_err(|_| bad("data index"))
}

/// One face before renumbering: its record geometry, loops relative to the record's normal as
/// (edge, forward), and each use's pcurve and data edgeuse.
#[derive(Clone)]
struct Face {
    surface: r::Surface,
    nurbs: Option<r::Surface>,
    domain: [[f64; 2]; 2],
    agrees: bool,
    loops: Vec<Loop>,
    /// The data edgeuse of each use, loop by loop as `loops` (none for a vertex loop).
    records: Vec<Vec<usize>>,
    /// Each use's pcurve, loop by loop as `loops`.
    pcurves: Vec<Vec<r::Curve2>>,
}

#[derive(Clone)]
enum Loop {
    Edges(Vec<(u32, bool)>),
    Vertex(u32),
}

/// One edge before renumbering.
#[derive(Clone)]
struct Edge {
    curve: r::Curve,
    nurbs: Option<r::Curve>,
    interval: [f64; 2],
    vertices: [u32; 2],
}

/// The kernel's body as the record, in canonical order, and the kernel entity each record
/// face, edge and vertex is. Every use on a face without a UV trim first gets the kernel's own
/// (`GetOrCreateUVTrimCurve`), which is why the body is taken `&mut`.
///
/// # Errors
///
/// [`Unsupported::NotYet`] for wire and point shells, wire edges, and touching the record
/// cannot split; [`Unsupported::Kernel`] for a curve or surface with no exact record, or a
/// step the kernel refuses; [`Unsupported::Invalid`] for kernel data that does not hold
/// together or a record that does not validate.
pub fn to_record(k: &mut Kernel, b: &mut Brep) -> R<(r::Brep, Order)> {
    for eu in solidcore::topology::brep::p2::edgeuses(b, None)? {
        if eu1::faceuse(b, eu)?.is_none() || eu1::uv_trim_curve_pointer(b, eu)?.is_some() {
            continue;
        }
        eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
    }
    let b: &Brep = b;
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &notify_ref,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bd = BrepData::from_brep(b, &env, &mut Vec::new())?;
    let face_ids = bl::faces(b, None)?;
    let edge_ids = bl::edges(b, None)?;
    let vertex_ids = bl::vertices(b, None)?;
    if face_ids.len() != bd.faces.len()
        || edge_ids.len() != bd.edges.len()
        || vertex_ids.len() != bd.vertices.len()
    {
        return Err(bad("the kernel's data lists differ from its body's"));
    }
    if bd.edgeuses.iter().any(|u| u.eu_type != 0) {
        return Err(Unsupported::NotYet("wire edges"));
    }
    let tolerances = tolerances(b, &edge_ids, &vertex_ids)?;

    // Edges along their records' parameters.
    let mut edges = Vec::new();
    let mut kernel_intervals = Vec::new();
    for (i, ed) in bd.edges.iter().enumerate() {
        let curve = edge::curve(b, *at(&edge_ids, i)?)?.ok_or(bad("an edge without a curve"))?;
        let made = record_of(k, curve, &ed.interval)?;
        let vertex = |v: u64| -> R<u32> {
            let v = data_index(v)?;
            at(&vertex_ids, v)?;
            index(v)
        };
        edges.push(Edge {
            curve: crate::record_curve(made.curve),
            nurbs: made.nurbs.map(crate::record_curve),
            interval: made.interval,
            vertices: [vertex(ed.start_vertex)?, vertex(ed.end_vertex)?],
        });
        kernel_intervals.push(ed.interval);
    }

    // Faces: loops as the kernel keeps them, turned over where its normal is not the record's.
    let mut faces = Vec::new();
    for (fi, fd) in bd.faces.iter().enumerate() {
        let f = *at(&face_ids, fi)?;
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or(bad("a face without a surface"))?;
        let made = record_face(k, surface, &fd.uv_domain)?;
        let mut face = Face {
            surface: crate::record_surface(made.surface),
            nurbs: made.nurbs.map(crate::record_surface),
            domain: made.domain,
            agrees: made.agrees,
            loops: Vec::new(),
            records: Vec::new(),
            pcurves: Vec::new(),
        };
        for l in 0..fd.num_loops {
            let ld = at(&bd.loops, data_index(fd.start_loop.saturating_add(l))?)?;
            if ld.loop_type == 1 {
                if ld.vertex == NO_OBJECT || ld.vertex == UNDEF_ULONG {
                    return Err(bad("a vertex loop without its vertex"));
                }
                let v = data_index(ld.vertex)?;
                at(&vertex_ids, v)?;
                face.loops.push(Loop::Vertex(index(v)?));
                face.records.push(Vec::new());
                face.pcurves.push(Vec::new());
                continue;
            }
            if ld.loop_type != 0 || ld.num_eu == 0 {
                return Err(bad("a loop that is neither an edge loop nor a vertex loop"));
            }
            let (mut uses, mut recs, mut pcs) = (Vec::new(), Vec::new(), Vec::new());
            for j in 0..ld.num_eu {
                let u = data_index(ld.start_eu.saturating_add(j))?;
                let ud = at(&bd.edgeuses, u)?;
                let e = data_index(ud.edge)?;
                let edge = at(&edges, e)?;
                if ud.uv_curve == NO_OBJECT {
                    return Err(bad("an edgeuse without its UV trim"));
                }
                let trim = &at(&bd.uv_curves, data_index(ud.uv_curve)?)?.object;
                let from: &Extent1d = at(&kernel_intervals, e)?;
                let pcurve = record_trim(k, trim, from, edge.interval, made.scale)?;
                uses.push((index(e)?, ud.orientation == 1));
                recs.push(u);
                pcs.push(crate::record_curve2(pcurve));
            }
            face.loops.push(Loop::Edges(uses));
            face.records.push(recs);
            face.pcurves.push(pcs);
        }
        if !face.agrees {
            for l in &mut face.loops {
                if let Loop::Edges(uses) = l {
                    uses.reverse();
                    uses.iter_mut().for_each(|u| u.1 = !u.1);
                }
            }
            face.records.iter_mut().for_each(|r| r.reverse());
            face.pcurves.iter_mut().for_each(|p| p.reverse());
        }
        faces.push(face);
    }

    // Regions, shells and faceuses as blocks of the data's lists.
    let n_shells = bd.shells.len();
    let mut region_shells = Vec::new();
    let mut region_of = vec![0usize; n_shells];
    for (ri, rd) in bd.regions.iter().enumerate() {
        let start = data_index(rd.start_shell)?;
        let end = start
            .checked_add(data_index(rd.num_shells)?)
            .filter(|e| *e <= n_shells)
            .ok_or(bad("region shells"))?;
        for s in start..end {
            if let Some(slot) = region_of.get_mut(s) {
                *slot = ri;
            }
        }
        region_shells.push(start..end);
    }
    if region_shells.first().map(|s| s.start) != Some(0) {
        return Err(bad("the kernel's data has no infinite region first"));
    }
    let void: Vec<bool> = bd.regions.iter().map(|g| g.is_void != 0).collect();
    let material = |r: usize| r > 0 && void.get(r) == Some(&false);
    let mut shell_faceuses = Vec::new();
    for sd in &bd.shells {
        if sd.num_faceuses == 0 {
            return Err(Unsupported::NotYet("wire and point shells"));
        }
        let start = data_index(sd.faceuse_start)?;
        let end = start
            .checked_add(data_index(sd.num_faceuses)?)
            .filter(|e| *e <= bd.faceuses.len())
            .ok_or(bad("shell faceuses"))?;
        shell_faceuses.push(start..end);
    }
    let faceuse = |fu: usize| -> R<(usize, bool)> {
        let d = at(&bd.faceuses, fu)?;
        let f = data_index(d.face)?;
        at(&faces, f)?;
        Ok((f, d.orientation == 1))
    };
    let mut face_uses: Vec<Vec<(usize, bool)>> = vec![Vec::new(); faces.len()];
    for (s, uses) in shell_faceuses.iter().enumerate() {
        for fu in uses.clone() {
            let (f, same) = faceuse(fu)?;
            face_uses.get_mut(f).ok_or(bad("face"))?.push((s, same));
        }
    }
    // Each record shell is one closed boundary of material: the faces of one runtime shell on
    // the material side, as the empty side uses them. Solids touching along an edge or at a
    // vertex share their empty side's runtime shell but not their material side's, so each
    // gets its own record shell. A sheet's one shell holds both uses of each face: the `same`
    // one is kept.
    let mut groups: Vec<Group> = Vec::new();
    for (s, uses) in shell_faceuses.iter().enumerate() {
        for fu in uses.clone() {
            let (f, same) = faceuse(fu)?;
            let key = match face_uses.get(f).map(Vec::as_slice) {
                Some([(a, _), (c, _)]) if a == c => {
                    if !same {
                        continue;
                    }
                    Side::Sheet(*a)
                }
                Some([(a, _), (c, _)]) => {
                    let other = if *a == s { *c } else { *a };
                    let here = region_of.get(s).copied().unwrap_or(0);
                    let there = region_of.get(other).copied().unwrap_or(0);
                    match (material(here), material(there)) {
                        (false, true) => Side::Solid(other),
                        (true, false) => continue,
                        _ => {
                            return Err(Unsupported::NotYet(
                                "a face between two material regions or two empty ones",
                            ))
                        }
                    }
                }
                _ => return Err(bad("a face without its two uses")),
            };
            let agrees = at(&faces, f)?.agrees;
            let slot = match groups.iter().position(|g| g.side == key) {
                Some(slot) => slot,
                None => {
                    groups.push(Group {
                        side: key,
                        empty: Vec::new(),
                        faces: Vec::new(),
                    });
                    groups.len().checked_sub(1).ok_or(bad("shell"))?
                }
            };
            let group = groups.get_mut(slot).ok_or(bad("shell"))?;
            if !group.empty.contains(&s) {
                group.empty.push(s);
            }
            group.faces.push((index(f)?, same == agrees));
        }
    }
    let mut regions = record_regions(&region_shells, &region_of, &groups, material)?;
    let shells: Vec<Vec<(u32, bool)>> = groups.into_iter().map(|g| g.faces).collect();
    // Shells renumbered in first appearance walking the regions.
    let mut order: Vec<u32> = Vec::new();
    for g in &regions {
        for s in &g.shells {
            if !order.contains(s) {
                order.push(*s);
            }
        }
    }
    let mut renumber: HashMap<u32, u32> = HashMap::new();
    for (new, old) in order.iter().enumerate() {
        renumber.insert(*old, index(new)?);
    }
    for g in &mut regions {
        g.shells
            .iter_mut()
            .for_each(|s| *s = renumber.get(s).copied().unwrap_or(*s));
    }
    let shells: Vec<Vec<(u32, bool)>> = order
        .iter()
        .filter_map(|s| shells.get(*s as usize).cloned())
        .collect();

    // Round each edge: the kernel's radial order of its uses, and the side of each face the
    // material is on, which say which faces bound the same material where a solid touches
    // itself.
    let material_same: Vec<Option<bool>> = face_uses
        .iter()
        .map(|uses| {
            uses.iter()
                .find(|(s, _)| material(region_of.get(*s).copied().unwrap_or(0)))
                .map(|(_, same)| *same)
        })
        .collect();
    let radial = radial(&bd, material_same)?;

    let parts = Parts {
        faces,
        edges,
        shells,
        regions,
        radial,
    };
    let (brep, order) = canonical(parts, bd.vertices.len(), &bd, &tolerances)?;
    let kernel = Order {
        faces: order
            .faces
            .iter()
            .map(|&f| at(&face_ids, f).copied())
            .collect::<R<_>>()?,
        edges: order
            .edges
            .iter()
            .map(|&e| at(&edge_ids, e).copied())
            .collect::<R<_>>()?,
        vertices: order
            .vertices
            .iter()
            .map(|&v| at(&vertex_ids, v).copied())
            .collect::<R<_>>()?,
    };
    Ok((brep, kernel))
}

/// The record's tolerances: the body's, and each data edge's and vertex's own.
struct Tolerances {
    body: f64,
    edges: Vec<f64>,
    vertices: Vec<f64>,
}

/// Each edge's and vertex's tolerance: the intersection tolerance of the largest of its own
/// zone tolerance, the body's and its faces'. The body's is the least of them and of the
/// body's own (or, where that is not below every entity's, the largest entity's): only what
/// needs more carries more, so each crossing between record and kernel keeps each entity's.
fn tolerances(b: &Brep, edge_ids: &[EdgeId], vertex_ids: &[solidcore::VertexId]) -> R<Tolerances> {
    let brep_tol = b.tolerance();
    let largest_of = |own: f64, faces: &[FaceId]| -> R<f64> {
        let mut t = own.max(brep_tol);
        for &f in faces {
            let ft = solidcore::topology::face::p2::tolerance(b, f)?;
            if ft.is_finite() && ft > t {
                t = ft;
            }
        }
        Ok(t)
    };
    let mut zone = brep_tol;
    let mut take = |t: f64| {
        if t.is_finite() && t >= 0.0 && t > zone {
            zone = t;
        }
    };
    for f in bl::faces(b, None)? {
        take(solidcore::topology::face::p2::tolerance(b, f)?);
    }
    let mut edges = Vec::new();
    for &e in edge_ids {
        let own = edge::tolerance(b, e)?;
        take(own);
        edges.push(intersection_tolerance(largest_of(
            own,
            &edge::faces(b, e, None)?,
        )?));
    }
    let mut vertices = Vec::new();
    for &v in vertex_ids {
        let own = vertex::tolerance(b, v)?;
        take(own);
        vertices.push(intersection_tolerance(largest_of(
            own,
            &vertex::faces(b, v)?,
        )?));
    }
    let body_zone = if brep_tol.is_finite() && brep_tol > 0.0 && brep_tol < zone {
        brep_tol
    } else {
        zone
    };
    let body = edges
        .iter()
        .chain(&vertices)
        .copied()
        .fold(intersection_tolerance(body_zone), f64::min);
    if !(body.is_finite() && body > 0.0) {
        return Err(bad("the body's tolerance is not finite and positive"));
    }
    Ok(Tolerances {
        body,
        edges,
        vertices,
    })
}

/// Which runtime shell a record shell is made from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The material side of a closed boundary.
    Solid(usize),
    /// A sheet's one shell, both uses of each face in it.
    Sheet(usize),
}

/// One record shell: its runtime shell, the runtime shells on its empty side, and its faces as
/// the empty side uses them.
struct Group {
    side: Side,
    empty: Vec<usize>,
    faces: Vec<(u32, bool)>,
}

/// The record's regions, each listing its record shells (`groups` by index), outer first.
///
/// A material region is the kernel's own. An empty one is too, except where solids touch
/// along a closed chain of edges round a pocket of empty space: the kernel keeps the pocket as
/// a void of its own, bounded by faces of more than one solid, or of one solid on both sides
/// of where it touches itself. The record splits what the solids share, so no shell bounds the
/// pocket on its own: each solid's one closed boundary faces both the pocket and the empty
/// space round the solids, and the pocket is part of that space. Empty regions a solid's
/// boundary faces together are one record region: the infinite region where it is among them,
/// otherwise the void whose outer shell bounds it alone.
fn record_regions(
    region_shells: &[std::ops::Range<usize>],
    region_of: &[usize],
    groups: &[Group],
    material: impl Fn(usize) -> bool,
) -> R<Vec<r::Region>> {
    let n = region_shells.len();
    let region = |s: usize| region_of.get(s).copied().ok_or(bad("shell region"));
    // The empty regions each solid's boundary faces, joined (each set's root its lowest
    // region).
    let mut root: Vec<usize> = (0..n).collect();
    fn find(root: &[usize], mut r: usize) -> usize {
        while let Some(&up) = root.get(r).filter(|up| **up != r) {
            r = up;
        }
        r
    }
    for g in groups {
        let mut it = g.empty.iter();
        let Some(&first) = it.next() else { continue };
        let mut a = find(&root, region(first)?);
        for &s in it {
            let c = find(&root, region(s)?);
            let (lo, hi) = (a.min(c), a.max(c));
            if let Some(slot) = root.get_mut(hi) {
                *slot = lo;
            }
            a = lo;
        }
    }
    let set: Vec<usize> = (0..n).map(|r| find(&root, r)).collect();
    // A region's outer runtime shell bounds it alone when every solid it faces faces no
    // region but this one.
    let alone = |r: usize| -> R<bool> {
        let Some(outer) = region_shells
            .get(r)
            .and_then(|s| (s.start < s.end).then_some(s.start))
        else {
            return Ok(false);
        };
        for g in groups.iter().filter(|g| g.empty.contains(&outer)) {
            for &s in &g.empty {
                if region(s)? != r {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    };
    // The record region of each set of empty regions.
    let mut kept = vec![None; n];
    for r in 0..n {
        let s = set.get(r).copied().unwrap_or(r);
        if material(r) || s != r {
            continue;
        }
        let members: Vec<usize> = (0..n).filter(|q| set.get(*q) == Some(&r)).collect();
        let keep = if members.len() == 1 || members.contains(&0) {
            r
        } else {
            let mut candidates = Vec::new();
            for &q in &members {
                if alone(q)? {
                    candidates.push(q);
                }
            }
            match candidates.as_slice() {
                [q] => *q,
                _ => {
                    return Err(Unsupported::NotYet(
                        "solids touching round pockets of empty space that no one shell bounds",
                    ))
                }
            }
        };
        if let Some(slot) = kept.get_mut(r) {
            *slot = Some(keep);
        }
    }
    let mut regions = Vec::new();
    for (r, block) in region_shells.iter().enumerate() {
        let mut listed: Vec<u32> = Vec::new();
        let list = |s: usize, listed: &mut Vec<u32>| -> R<()> {
            for (g, group) in groups.iter().enumerate() {
                let here = match group.side {
                    Side::Solid(m) => m == s || group.empty.contains(&s),
                    Side::Sheet(a) => a == s,
                };
                let id = index(g)?;
                if here && !listed.contains(&id) {
                    listed.push(id);
                }
            }
            Ok(())
        };
        if material(r) {
            for s in block.clone() {
                list(s, &mut listed)?;
            }
        } else {
            let s = set.get(r).copied().unwrap_or(r);
            if kept.get(s).copied().flatten() != Some(r) {
                continue;
            }
            // This region's shells first, its outer shell the record region's, then the
            // pockets'.
            let members =
                std::iter::once(r).chain((0..n).filter(|q| *q != r && set.get(*q) == Some(&s)));
            for q in members {
                for s in region_shells.get(q).cloned().unwrap_or_default() {
                    list(s, &mut listed)?;
                }
            }
        }
        regions.push(r::Region {
            shells: listed,
            void: r > 0 && !material(r),
        });
    }
    Ok(regions)
}

/// The mapped records before renumbering.
struct Parts {
    faces: Vec<Face>,
    edges: Vec<Edge>,
    shells: Vec<Vec<(u32, bool)>>,
    regions: Vec<r::Region>,
    radial: Radial,
}

/// The kernel's order of the faces round each edge. Each data edgeuse is a face's two sides
/// along the edge; a right-hand traversal round the edge meets one side of each, then its
/// other side, then the next. Between two lies one sector of space round the edge, empty or
/// material.
struct Radial {
    /// The next data edgeuse round the edge.
    next: Vec<usize>,
    /// The traversal enters the edgeuse from its face's `same` side (the kernel's top
    /// edgeuse) and leaves from the other.
    top_entry: Vec<bool>,
    /// Per data face: whether the material is on its `same` side; none for a sheet's face.
    material_same: Vec<Option<bool>>,
}

/// The radial cycles of the data's edgeuses (each pair's `next_eu`, `mate_next_eu`: a
/// non-negative entry names the next pair's top edgeuse, `-i - 1` its bottom one), checked: in
/// range and each edge's edgeuses one cycle or more.
fn radial(bd: &BrepData, material_same: Vec<Option<bool>>) -> R<Radial> {
    let n = bd.edgeuses.len();
    let decode = |x: i64| -> R<usize> {
        let i = if x >= 0 {
            x
        } else {
            x.checked_neg()
                .and_then(|y| y.checked_sub(1))
                .ok_or(bad("radial index"))?
        };
        usize::try_from(i)
            .ok()
            .filter(|i| *i < n)
            .ok_or(bad("radial index"))
    };
    let (mut next, mut top_entry) = (Vec::new(), Vec::new());
    for (j, eu) in bd.edgeuses.iter().enumerate() {
        let top_next = decode(eu.next_eu)?;
        let bottom_next = decode(eu.mate_next_eu)?;
        let top = top_next == j;
        next.push(if top { bottom_next } else { top_next });
        top_entry.push(top);
    }
    let mut reached = vec![false; n];
    for (u, &m) in next.iter().enumerate() {
        let slot = reached.get_mut(m).ok_or(bad("next radial edgeuse"))?;
        let edge_of = |i: usize| bd.edgeuses.get(i).map(|e| e.edge);
        if *slot || edge_of(u) != edge_of(m) {
            return Err(bad("a radial list that is not a cycle round its edge"));
        }
        *slot = true;
    }
    Ok(Radial {
        next,
        top_entry,
        material_same,
    })
}

/// Where each record entity came from: the data face, edge and vertex of each, in canonical
/// order.
struct Sources {
    faces: Vec<usize>,
    edges: Vec<usize>,
    vertices: Vec<usize>,
}

/// The record in canonical order: faces in first appearance through the shells, edges
/// through the faces (a face whose first oriented face is reversed walked backwards),
/// vertices through the edges and then the vertex loops; validated. With it, the data face,
/// edge and vertex of each record one.
fn canonical(
    parts: Parts,
    n_vertices: usize,
    bd: &BrepData,
    tolerances: &Tolerances,
) -> R<(r::Brep, Sources)> {
    let Parts {
        mut faces,
        mut edges,
        shells,
        regions,
        radial,
    } = parts;
    let (mut edge_source, vertex_source) =
        split_shared(&mut faces, &mut edges, &shells, n_vertices)?;
    split_touching(&mut faces, &mut edges, &mut edge_source, &radial)?;
    let n_vertices = vertex_source.len();
    let mut face_new: Vec<Option<u32>> = vec![None; faces.len()];
    let (mut face_order, mut direction) = (Vec::new(), Vec::new());
    for shell in &shells {
        for &(f, forward) in shell {
            let slot = face_new.get_mut(f as usize).ok_or(bad("face"))?;
            if slot.is_none() {
                *slot = Some(index(face_order.len())?);
                face_order.push(f);
                direction.push(forward);
            }
        }
    }
    if face_order.len() != faces.len() {
        return Err(bad("a face no shell uses"));
    }
    let mut edge_new: Vec<Option<u32>> = vec![None; edges.len()];
    let mut edge_order = Vec::new();
    let mut edge_uses: Vec<Vec<r::EdgeUseRef>> = Vec::new();
    for (fi, (&f, &forward)) in face_order.iter().zip(&direction).enumerate() {
        let face = faces.get(f as usize).ok_or(bad("face"))?;
        for (li, l) in face.loops.iter().enumerate() {
            let Loop::Edges(uses) = l else { continue };
            let n = uses.len();
            for k in 0..n {
                let p = if forward {
                    k
                } else {
                    n.checked_sub(1 + k).ok_or(bad("loop"))?
                };
                let &(e, fwd) = uses.get(p).ok_or(bad("loop"))?;
                let slot = edge_new.get_mut(e as usize).ok_or(bad("edge"))?;
                let id = match slot {
                    Some(id) => *id,
                    None => {
                        let id = index(edge_order.len())?;
                        *slot = Some(id);
                        edge_order.push(e);
                        edge_uses.push(Vec::new());
                        id
                    }
                };
                let use_ref = r::EdgeUseRef {
                    face: index(fi)?,
                    r#loop: index(li)?,
                    position: index(p)?,
                    upper: !fwd,
                };
                edge_uses
                    .get_mut(id as usize)
                    .ok_or(bad("edge"))?
                    .push(use_ref);
            }
        }
    }
    if edge_order.len() != edges.len() {
        return Err(Unsupported::NotYet("wire edges"));
    }
    if edge_uses.iter().any(|u| u.len() > 2) {
        return Err(Unsupported::NotYet("a solid touching itself along an edge"));
    }
    let mut vertex_new: Vec<Option<u32>> = vec![None; n_vertices];
    let mut vertex_order = Vec::new();
    let mut visit = |v: u32| -> R<u32> {
        let slot = vertex_new.get_mut(v as usize).ok_or(bad("vertex"))?;
        Ok(match slot {
            Some(id) => *id,
            None => {
                let id = index(vertex_order.len())?;
                *slot = Some(id);
                vertex_order.push(v);
                id
            }
        })
    };
    let mut edge_vertices = Vec::new();
    for &e in &edge_order {
        let [a, z] = edges.get(e as usize).ok_or(bad("edge"))?.vertices;
        edge_vertices.push([visit(a)?, visit(z)?]);
    }
    let mut loop_vertices: HashMap<(usize, usize), u32> = HashMap::new();
    for (fi, &f) in face_order.iter().enumerate() {
        let face = faces.get(f as usize).ok_or(bad("face"))?;
        for (li, l) in face.loops.iter().enumerate() {
            if let Loop::Vertex(v) = l {
                loop_vertices.insert((fi, li), visit(*v)?);
            }
        }
    }
    if vertex_order.len() != n_vertices {
        return Err(Unsupported::NotYet("vertices no edge or vertex loop uses"));
    }
    let edge_id = |e: u32| {
        edge_new
            .get(e as usize)
            .copied()
            .flatten()
            .ok_or(bad("edge"))
    };

    let mut edge_records = Vec::new();
    for ((&e, vertices), uses) in edge_order.iter().zip(edge_vertices).zip(edge_uses) {
        let edge = edges.get(e as usize).ok_or(bad("edge"))?;
        let source = *at(&edge_source, e as usize)?;
        edge_records.push(r::Edge {
            nurbs: edge.nurbs.clone(),
            curve: edge.curve.clone(),
            interval: edge.interval,
            vertices,
            tolerance: *at(&tolerances.edges, source)?,
            uses,
        });
    }
    let mut face_records = Vec::new();
    for (fi, &f) in face_order.iter().enumerate() {
        let face = faces.get(f as usize).ok_or(bad("face"))?;
        let mut loops = Vec::new();
        for (li, (l, pcs)) in face.loops.iter().zip(&face.pcurves).enumerate() {
            loops.push(match l {
                Loop::Vertex(_) => r::Loop::Vertex {
                    vertex: *loop_vertices.get(&(fi, li)).ok_or(bad("vertex loop"))?,
                },
                Loop::Edges(uses) => {
                    if pcs.len() != uses.len() {
                        return Err(bad("pcurves"));
                    }
                    let mut out = Vec::new();
                    for (&(e, forward), pcurve) in uses.iter().zip(pcs) {
                        out.push(r::OrientedEdge {
                            edge: edge_id(e)?,
                            forward,
                            pcurve: pcurve.clone(),
                        });
                    }
                    r::Loop::Edges { edges: out }
                }
            });
        }
        face_records.push(r::Face {
            nurbs: face.nurbs.clone(),
            surface: face.surface.clone(),
            domain: face.domain,
            loops,
        });
    }
    let mut vertices = Vec::new();
    for &v in &vertex_order {
        let source = *at(&vertex_source, v as usize)?;
        let p = at(&bd.vertices, source)?.point;
        vertices.push(r::Vertex {
            point: [p.x, p.y, p.z],
            tolerance: *at(&tolerances.vertices, source)?,
        });
    }
    let mut shell_records = Vec::new();
    for s in shells {
        let mut faces = Vec::new();
        for (f, forward) in s {
            faces.push(r::OrientedFace {
                face: face_new
                    .get(f as usize)
                    .copied()
                    .flatten()
                    .ok_or(bad("face"))?,
                forward,
            });
        }
        shell_records.push(r::Shell::Faces {
            faces,
            wires: Vec::new(),
        });
    }
    let brep = r::Brep {
        version: r::VERSION,
        tolerance: tolerances.body,
        regions,
        shells: shell_records,
        faces: face_records,
        edges: edge_records,
        vertices,
    };
    brep.validate_built()
        .map_err(|e| Unsupported::Invalid(e.0))?;
    let sources = Sources {
        faces: face_order.iter().map(|&f| f as usize).collect(),
        edges: edge_order
            .iter()
            .map(|&e| at(&edge_source, e as usize).copied())
            .collect::<R<_>>()?,
        vertices: vertex_order
            .iter()
            .map(|&v| at(&vertex_source, v as usize).copied())
            .collect::<R<_>>()?,
    };
    Ok((brep, sources))
}

/// Edges and vertices that solids touching each other share (faces of more than one shell
/// use them), split into one copy per shell, so each shell is a closed manifold boundary of
/// its own. The geometry is unchanged. Returns, for every edge and vertex after the split, the
/// data edge and vertex it is (a copy's original).
fn split_shared(
    faces: &mut [Face],
    edges: &mut Vec<Edge>,
    shells: &[Vec<(u32, bool)>],
    n_vertices: usize,
) -> R<(Vec<usize>, Vec<usize>)> {
    let mut shell_of = vec![None; faces.len()];
    for (s, shell) in shells.iter().enumerate() {
        for &(f, _) in shell {
            *shell_of.get_mut(f as usize).ok_or(bad("face"))? = Some(s);
        }
    }
    let mut edge_source: Vec<usize> = (0..edges.len()).collect();
    // Edges: the first shell to use one keeps it, every other gets a copy.
    let mut edge_shell: Vec<Option<usize>> = vec![None; edges.len()];
    let mut copies: HashMap<(u32, usize), u32> = HashMap::new();
    for (f, face) in faces.iter_mut().enumerate() {
        let Some(s) = shell_of.get(f).copied().flatten() else {
            continue;
        };
        for l in &mut face.loops {
            let Loop::Edges(uses) = l else { continue };
            for (e, _) in uses.iter_mut() {
                let slot = edge_shell.get_mut(*e as usize).ok_or(bad("edge"))?;
                match slot {
                    None => *slot = Some(s),
                    Some(owner) if *owner == s => {}
                    Some(_) => {
                        let copy = match copies.get(&(*e, s)) {
                            Some(c) => *c,
                            None => {
                                let c = index(edges.len())?;
                                let original = edges.get(*e as usize).ok_or(bad("edge"))?.clone();
                                edges.push(original);
                                edge_source.push(*e as usize);
                                edge_shell.push(Some(s));
                                copies.insert((*e, s), c);
                                c
                            }
                        };
                        *e = copy;
                    }
                }
            }
        }
    }
    // Vertices: likewise, through the edges (each now in one shell) and the vertex loops.
    let mut vertex_source: Vec<usize> = (0..n_vertices).collect();
    let mut vertex_shell: Vec<Option<usize>> = vec![None; n_vertices];
    let mut vertex_copies: HashMap<(u32, usize), u32> = HashMap::new();
    let mut place = |v: &mut u32, s: usize| -> R<()> {
        let slot = vertex_shell.get_mut(*v as usize).ok_or(bad("vertex"))?;
        match slot {
            None => *slot = Some(s),
            Some(owner) if *owner == s => {}
            Some(_) => {
                let copy = match vertex_copies.get(&(*v, s)) {
                    Some(c) => *c,
                    None => {
                        let c = index(vertex_source.len())?;
                        vertex_source.push(*v as usize);
                        vertex_shell.push(Some(s));
                        vertex_copies.insert((*v, s), c);
                        c
                    }
                };
                *v = copy;
            }
        }
        Ok(())
    };
    for (e, edge) in edges.iter_mut().enumerate() {
        let Some(s) = edge_shell.get(e).copied().flatten() else {
            continue;
        };
        for v in &mut edge.vertices {
            place(v, s)?;
        }
    }
    for (f, face) in faces.iter_mut().enumerate() {
        let Some(s) = shell_of.get(f).copied().flatten() else {
            continue;
        };
        for l in &mut face.loops {
            if let Loop::Vertex(v) = l {
                place(v, s)?;
            }
        }
    }
    Ok((edge_source, vertex_source))
}

/// Edges one shell uses more than twice, where a solid touches itself along a line, split so
/// that every edge bounds two faces: the kernel's radial order round the edge pairs each use
/// with the next one across a sector, and each pair gets its own copy of the edge. The pairs
/// are those across a material sector, unless that would give one face two copies (a face on
/// both sides of the line), which the kernel refuses (its check's tests 33 and 34): then the
/// pairs across the empty sectors, so that face meets itself across the line as the kernel's
/// crack edges do. The vertices at the ends stay shared (test 32). The geometry is unchanged;
/// each copy is added to `edge_source` with its original's source.
fn split_touching(
    faces: &mut [Face],
    edges: &mut Vec<Edge>,
    edge_source: &mut Vec<usize>,
    radial: &Radial,
) -> R<()> {
    // The data edgeuses using each edge, and where each is: (face, loop, position).
    let mut records_of: Vec<Vec<usize>> = vec![Vec::new(); edges.len()];
    let mut place: HashMap<usize, (usize, usize, usize)> = HashMap::new();
    for (f, face) in faces.iter().enumerate() {
        for (li, (l, records)) in face.loops.iter().zip(&face.records).enumerate() {
            let Loop::Edges(uses) = l else { continue };
            if records.len() != uses.len() {
                return Err(bad("loop records"));
            }
            for (p, (&(e, _), &rec)) in uses.iter().zip(records).enumerate() {
                records_of.get_mut(e as usize).ok_or(bad("edge"))?.push(rec);
                if place.insert(rec, (f, li, p)).is_some() {
                    return Err(bad("an edgeuse in two loops"));
                }
            }
        }
    }
    let crowded: Vec<usize> = (0..edges.len())
        .filter(|e| records_of.get(*e).is_some_and(|r| r.len() > 2))
        .collect();
    let face_of = |rec: usize| place.get(&rec).map(|p| p.0).ok_or(bad("edgeuse"));
    let material_same = |rec: usize| -> R<bool> {
        radial
            .material_same
            .get(face_of(rec)?)
            .copied()
            .flatten()
            .ok_or(Unsupported::NotYet("a sheet touching itself along an edge"))
    };
    let top_entry = |rec: usize| {
        radial
            .top_entry
            .get(rec)
            .copied()
            .ok_or(bad("radial entry"))
    };
    // Whether the sector the traversal leaves a use into, and the one it enters a use from,
    // is material: it leaves from the side it did not enter.
    let after = |rec: usize| -> R<bool> { Ok(material_same(rec)? != top_entry(rec)?) };
    let before = |rec: usize| -> R<bool> { Ok(material_same(rec)? == top_entry(rec)?) };

    for e in crowded {
        let mine = records_of.get(e).cloned().unwrap_or_default();
        let start = *mine.first().ok_or(bad("edge"))?;
        // The edge's radial list from one of its uses, keeping this edge's.
        let mut cycle = Vec::new();
        let mut rec = start;
        for _ in 0..radial.next.len() {
            if mine.contains(&rec) {
                cycle.push(rec);
            }
            rec = *at(&radial.next, rec)?;
            if rec == start {
                break;
            }
        }
        if rec != start || cycle.len() != mine.len() {
            return Err(bad(
                "a radial list that does not go once round its edge's uses",
            ));
        }
        let k = cycle.len();
        let around = |i: usize| -> R<usize> {
            let j = i.checked_rem(k).ok_or(bad("radial list"))?;
            at(&cycle, j).copied()
        };
        // Sectors alternate material and space, each seen alike from the uses on its sides.
        let mut alternate = k & 1 == 0;
        for i in 0..k {
            let (a, c) = (around(i)?, around(i + 1)?);
            alternate &= after(a)? == before(c)? && after(a)? != after(c)?;
        }
        if !alternate {
            return Err(Unsupported::NotYet(
                "a solid touching itself along an edge whose faces do not alternate between material and space round it",
            ));
        }
        // The pairs across material sectors, unless one face would get two copies; then
        // those across empty sectors, unless that does too.
        let material = usize::from(!after(around(0)?)?);
        let one_copy_per_face = |first: usize| -> R<bool> {
            let mut pair_of: HashMap<usize, usize> = HashMap::new();
            for i in 0..k {
                let f = face_of(around(first + i)?)?;
                if *pair_of.entry(f).or_insert(i >> 1) != i >> 1 {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let first = if one_copy_per_face(material)? || !one_copy_per_face(1 - material)? {
            material
        } else {
            1 - material
        };
        for pair in 0..k >> 1 {
            let (a, c) = (around(first + 2 * pair)?, around(first + 2 * pair + 1)?);
            let copy = if pair == 0 {
                e
            } else {
                let made = edges.len();
                let original = edges.get(e).ok_or(bad("edge"))?.clone();
                edges.push(original);
                edge_source.push(*at(edge_source, e)?);
                made
            };
            for rec in [a, c] {
                let (f, li, p) = place.get(&rec).copied().ok_or(bad("edgeuse"))?;
                let slot = faces
                    .get_mut(f)
                    .and_then(|face| face.loops.get_mut(li))
                    .and_then(|l| {
                        if let Loop::Edges(u) = l {
                            u.get_mut(p)
                        } else {
                            None
                        }
                    })
                    .ok_or(bad("edgeuse"))?;
                slot.0 = index(copy)?;
            }
        }
    }
    Ok(())
}
