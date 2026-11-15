//! The record's B-rep and the kernel's, both ways: the record's lists laid out as the kernel's
//! B-rep data ([`BrepData`]) and built ([`to_kernel`]), and a kernel body read back as the record
//! ([`to_record`]). What is done here is index mapping and field copies: regions and shells and
//! their sides, the uses of faces and edges and their radial order, loops, tolerances. Every
//! geometric step is a kernel call: a face's surface ([`face_surface`]), an edge's curve
//! ([`curve::from_parameters`](solidcore::curve::from_parameters)), a pcurve as its UV trim, the
//! zone tolerance of a tolerance, the order of a region's shells.
//!
//! The record and the kernel's data differ in four places, each taken care of here:
//! - A closed shell is one record listed by the two regions it separates; the kernel keeps one
//!   shell per region, the face used on opposite sides. A sheet's shell holds both sides of its
//!   faces.
//! - The infinite region is never a void in the record; the kernel marks it as one.
//! - A pcurve runs along its edge over the edge's interval in the record; the kernel's UV trim
//!   runs along its edge over the edge's kernel interval. Trims are carried where the face's
//!   parameters are the record's (a spline, a NURBS form, a ball patch's net); elsewhere the
//!   kernel builds them from the edges.
//! - Angles are radians in the record, degrees in the kernel: the kernel's constructors convert.

// The kernel's data records are filled field by field, as its reader documents them.
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use std::collections::BTreeMap;

use solidcore::curve::curve::Curve as KernelCurve;
use solidcore::curve::from_parameters::{
    self as curves, approx_tolerance, zone_tolerance, MAX_TOLERANCE,
};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::Kernel;
use solidcore::surface::face_surface::face_surface;
use solidcore::surface::procedural_blend::convert::adopt_definitions;
use solidcore::topology::brep::enclosing::put_enclosing_shells_first;
use solidcore::topology::brep::p10::{
    heal_made, make_topology_from_data, MakeTopologyOptions, TopologyEnv, HEALER_VERSION,
};
use solidcore::topology::brep_data::{
    BrepData, DataSourceType, EdgeData, EuData, FaceData, FaceuseData, Keyed, LoopData, RegionData,
    ShellData, TopologyData, VertexData, NO_OBJECT, UNSURE,
};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EdgeId, FaceId, SolidError, Vec3, VertexId};
use solidcore_record::brep as r;

/// Why a record cannot go to the kernel, or the kernel's body to the record.
#[derive(Clone, Debug, PartialEq)]
pub enum Unsupported {
    /// The record failed its structural validation, or holds what the kernel's limits refuse.
    Invalid(&'static str),
    /// A part of the record this conversion does not carry yet.
    NotYet(&'static str),
    /// The kernel refused a step of the conversion (a surface or curve not proved on its
    /// numbers, the build).
    Kernel(SolidError),
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unsupported::Invalid(w) => write!(f, "invalid B-rep record: {w}"),
            Unsupported::NotYet(w) => write!(f, "not supported yet: {w}"),
            Unsupported::Kernel(e) => write!(f, "solidcore: {e}"),
        }
    }
}

impl std::error::Error for Unsupported {}

impl From<solidcore_record::brep::Invalid> for Unsupported {
    fn from(e: solidcore_record::brep::Invalid) -> Self {
        Unsupported::Invalid(e.0)
    }
}

impl From<SolidError> for Unsupported {
    fn from(e: SolidError) -> Self {
        Unsupported::Kernel(e)
    }
}

type R<T> = Result<T, Unsupported>;

/// The kernel entity each record index names: `faces[i]` is record face `i`, and so for edges and
/// vertices. In [`to_record`]'s order several record edges may name one kernel edge where solids
/// touch along it; [`to_kernel`] builds one kernel edge per record edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Order {
    /// Record face `i`'s kernel face.
    pub faces: Vec<FaceId>,
    /// Record edge `i`'s kernel edge.
    pub edges: Vec<EdgeId>,
    /// Record vertex `i`'s kernel vertex.
    pub vertices: Vec<VertexId>,
}

/// A kernel edge solids touch along, as [`to_record`] writes it: one record edge per pair of
/// faces it separates (SolidCraft's `history::Piece`). An edge with uses on three or more faces
/// has one [`Piece`] per record edge [`to_record`] copied it into, each with the faces of that
/// copy's own uses (two, for the sector pairs the radial order gives); none is merged or split
/// here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The kernel edge.
    pub kernel: EdgeId,
    /// One of its record edges.
    pub edge: u32,
    /// The record faces that record edge bounds, ascending.
    pub faces: Vec<u32>,
}

impl Order {
    /// Each kernel face's record face (the inverse of [`Order::faces`]).
    #[must_use]
    pub fn record_faces(&self) -> BTreeMap<FaceId, u32> {
        inverse(&self.faces)
    }

    /// Each kernel edge's first record edge (the inverse of [`Order::edges`]; a kernel edge
    /// solids touch along has more, its [`Order::pieces`]).
    #[must_use]
    pub fn record_edges(&self) -> BTreeMap<EdgeId, u32> {
        inverse(&self.edges)
    }

    /// Each kernel vertex's first record vertex (the inverse of [`Order::vertices`]).
    #[must_use]
    pub fn record_vertices(&self) -> BTreeMap<VertexId, u32> {
        inverse(&self.vertices)
    }

    /// Every record edge of each kernel edge that has more than one, in record order, with the
    /// record faces it bounds (`record` is the record this order came with): the faces of that
    /// record edge's uses, sorted and without repeats.
    ///
    /// # Errors
    ///
    /// [`Unsupported::Invalid`] for a record whose edges are not this order's.
    pub fn pieces(&self, record: &r::Brep) -> R<Vec<Piece>> {
        if record.edges.len() != self.edges.len() {
            return Err(Unsupported::Invalid("a record not of this order"));
        }
        let mut count: BTreeMap<EdgeId, usize> = BTreeMap::new();
        for &e in &self.edges {
            *count.entry(e).or_default() += 1;
        }
        let mut out = Vec::new();
        for (i, (&kernel, e)) in self.edges.iter().zip(&record.edges).enumerate() {
            if count.get(&kernel).copied().unwrap_or(0) < 2 {
                continue;
            }
            let mut faces: Vec<u32> = e.uses.iter().map(|u| u.face).collect();
            faces.sort_unstable();
            faces.dedup();
            out.push(Piece {
                kernel,
                edge: u32::try_from(i).map_err(|_| Unsupported::Invalid("index"))?,
                faces,
            });
        }
        Ok(out)
    }
}

/// Each entity's first index in `list`. An index past `u32` is left out: unreachable, since
/// every list is an [`Order`]'s, indexed by a record's `u32` indices.
fn inverse<T: Copy + Ord>(list: &[T]) -> BTreeMap<T, u32> {
    let mut out = BTreeMap::new();
    for (i, &x) in list.iter().enumerate() {
        if let Ok(i) = u32::try_from(i) {
            out.entry(x).or_insert(i);
        }
    }
    out
}

fn int(n: usize) -> R<u64> {
    u64::try_from(n).map_err(|_| Unsupported::Invalid("count"))
}

fn at<T>(v: &[T], i: u32) -> R<&T> {
    v.get(i as usize)
        .ok_or(Unsupported::Invalid("index past its list"))
}

/// The record's topology fields left undefined, as the kernel's data reader expects them.
fn undefined(t: &mut TopologyData) {
    t.flags = 0;
    t.user_index1 = 0;
    t.user_index2 = 0;
}

/// The record built as the kernel's body, and where each record face, edge and vertex went.
/// `trusted`: the record comes from a modeller that keeps the kernel's rules, so the kernel's
/// healer is skipped; otherwise the body is healed as it is built.
///
/// # Errors
///
/// [`Unsupported::Invalid`] for a record that fails its validation or a tolerance past the
/// kernel's limit; [`Unsupported::NotYet`] for embedded wire edges and traced
/// intersection curves, and for an untrusted record whose topology the healer changed (its
/// record order is then not known: [`to_kernel_body`] gives the body alone);
/// [`Unsupported::Kernel`] for a step the kernel refuses.
pub fn to_kernel(k: &mut Kernel, record: &r::Brep, trusted: bool) -> R<(Brep, Order)> {
    to_kernel_with_bare_uses(k, record, trusted, &[])
}

/// The record built as the kernel's body, as [`to_kernel`], without its record order: an
/// untrusted record whose topology the healer changed is its healed body.
///
/// # Errors
///
/// As [`to_kernel`], except for the healer's changes.
pub fn to_kernel_body(k: &mut Kernel, record: &r::Brep, trusted: bool) -> R<Brep> {
    build(k, record, trusted, &[]).map(|(b, _)| b)
}

/// [`to_kernel`], the uses `bare` names (each `(face, loop, position)`, sorted) going without
/// their record pcurves, which they need not have: the kernel makes their trims as it makes every
/// trim it needs ([`to_record`] asks for them). For a face whose uses have no pcurve of their own
/// (an approximated edge on a cone, a file's spline edge on a spline surface): the face goes to
/// the kernel alone, its bare uses' pcurves placeholders, and comes back with the kernel's.
///
/// # Errors
///
/// As [`to_kernel`]; [`Unsupported::Invalid`] for `bare` not sorted.
pub fn to_kernel_with_bare_uses(
    k: &mut Kernel,
    record: &r::Brep,
    trusted: bool,
    bare: &[(u32, u32, u32)],
) -> R<(Brep, Order)> {
    match build(k, record, trusted, bare)? {
        (b, Some(order)) => Ok((b, order)),
        (_, None) => Err(Unsupported::NotYet(
            "the healer changed the record's topology, so its record order is not known",
        )),
    }
}

/// The body, and its record order unless the healer changed its topology. The kernel makes
/// its faces, edges and vertices in the data's order, which is the record's, and lists them so;
/// for an untrusted record it then heals the body ([`heal_made`]). Entities the healer keeps
/// keep their identities (the kernel never reuses one), so the order stands exactly when every
/// list is the same after healing as before; otherwise the healer removed, added or replaced
/// one, which entity a record index names is not known, and there is no order.
fn build(
    k: &mut Kernel,
    record: &r::Brep,
    trusted: bool,
    bare: &[(u32, u32, u32)],
) -> R<(Brep, Option<Order>)> {
    record
        .validate_built()
        .map_err(|e| Unsupported::Invalid(e.0))?;
    if !bare.is_sorted() {
        return Err(Unsupported::Invalid("bare uses not sorted"));
    }
    let mut data = data(k, record, bare)?;
    data.set_healer_version(if trusted {
        HEALER_VERSION
    } else {
        HEALER_VERSION - 2
    });
    let mut b = Brep::new_with_infinite_region()?;
    let mut store = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let next_key = int(data.uv_curves.len() + data.curves_3d.len() + data.surfaces.len())? + 1;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &notify_ref,
        records: &mut records,
        next_key,
    };
    // the healer runs after the order is taken (`heal_made`, as the kernel would run it)
    let options = MakeTopologyOptions {
        make_uv_trim_curves: false,
        add_analytics: false,
        force_same_tol: true,
        check_uv_trim_curves: false,
        always_heal: false,
        healer_enabled: false,
    };
    make_topology_from_data(k, &mut b, &mut data, &mut env, options)?;
    let order = Order {
        faces: b.face_list().to_vec(),
        edges: b.edge_list().to_vec(),
        vertices: b.vertex_list().to_vec(),
    };
    adopt(k, &mut b, record, &order.faces)?;
    if (order.faces.len(), order.edges.len(), order.vertices.len())
        != (
            record.faces.len(),
            record.edges.len(),
            record.vertices.len(),
        )
    {
        return Err(Unsupported::Invalid(
            "the kernel built another number of faces, edges or vertices than the record has",
        ));
    }
    let kept = if trusted {
        true
    } else {
        heal_made(k, &mut b, &notify_ref, false)?;
        b.face_list() == order.faces.as_slice()
            && b.edge_list() == order.edges.as_slice()
            && b.vertex_list() == order.vertices.as_slice()
    };
    put_enclosing_shells_first(k, &mut b)?;
    Ok((b, kept.then_some(order)))
}

/// The record as the kernel's B-rep data: edgeuses face by face, loop by loop, in loop order;
/// edges and vertices in the record's order. Each edge and vertex keeps its own tolerance; the
/// body's and each face's are the least of theirs ([`own_tolerances`]). The record has no face
/// tolerance: a face's own crosses as its edges' (`to_record` puts it into them).
fn data(k: &Kernel, rec: &r::Brep, bare: &[(u32, u32, u32)]) -> R<BrepData> {
    let zone = zone_tolerance(rec.tolerance);
    let mut d = BrepData::new(UNSURE, DataSourceType::Usd);
    d.zone_tol_3d = zone;
    d.approx_tol_3d = approx_tolerance(rec.tolerance);
    d.start_region = 0;
    d.num_regions = int(rec.regions.len())?;
    d.color = Vec3::new(0.0, 0.0, 0.0);
    d.is_geom_borrowed = false;
    let mut next_key = 0u64;
    let mut key = || {
        next_key += 1;
        ObjectKey(next_key)
    };

    // Regions and shells: each region gets its own copy of each shell it lists.
    let enclosed_by = |s: u32| {
        rec.regions
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, g)| g.shells.first() == Some(&s))
            .map(|(i, _)| i)
    };
    let listed = |s: u32| rec.regions.iter().filter(|g| g.shells.contains(&s)).count();
    let mut start_shell = 0u64;
    let mut wire_shells = Vec::new();
    for (ri, region) in rec.regions.iter().enumerate() {
        let mut rd = RegionData::default();
        rd.start_shell = start_shell;
        rd.num_shells = int(region.shells.len())?;
        // the kernel marks the infinite region as a void
        rd.is_void = i32::from(ri == 0 || region.void);
        undefined(&mut rd.topology);
        start_shell += rd.num_shells;
        d.regions.push(rd);
        for (pos, &s) in region.shells.iter().enumerate() {
            let shell = at(&rec.shells, s)?;
            let r::Shell::Faces { faces, wires } = shell else {
                let mut sd = ShellData::default();
                undefined(&mut sd.topology);
                match shell {
                    r::Shell::Wire { edges } => {
                        sd.shell_type = 1;
                        sd.edge = u64::from(
                            *edges
                                .first()
                                .ok_or(Unsupported::Invalid("empty wire shell"))?,
                        );
                        wire_shells.push((int(d.shells.len())?, edges));
                    }
                    r::Shell::Vertex { vertex } => {
                        sd.shell_type = 2;
                        sd.vertex = u64::from(*vertex);
                    }
                    r::Shell::Faces { .. } => return Err(Unsupported::Invalid("shell kind")),
                }
                d.shells.push(sd);
                continue;
            };
            if !wires.is_empty() {
                return Err(Unsupported::NotYet("wire edges embedded in a shell"));
            }
            let uses: Vec<(u32, bool)> = if listed(s) == 2 {
                // A closed shell: its normals (out of the material) point into the region that
                // is not material. As the outer shell of region ri the shell encloses ri; as an
                // inner shell ri lies outside it.
                let inside = enclosed_by(s)
                    .ok_or(Unsupported::Invalid("a closed shell encloses no region"))?;
                let inside_is_void = rec.regions.get(inside).is_some_and(|g| g.void);
                let normals_into_this = if pos == 0 && ri > 0 {
                    inside_is_void
                } else {
                    !inside_is_void
                };
                faces
                    .iter()
                    .map(|f| (f.face, f.forward == normals_into_this))
                    .collect()
            } else {
                // An open shell lies in one region and holds both sides of each face.
                faces
                    .iter()
                    .flat_map(|f| [(f.face, true), (f.face, false)])
                    .collect()
            };
            let mut sd = ShellData::default();
            sd.shell_type = 0;
            sd.faceuse_start = int(d.faceuses.len())?;
            sd.num_faceuses = int(uses.len())?;
            undefined(&mut sd.topology);
            d.shells.push(sd);
            for (face, same) in uses {
                let mut fu = FaceuseData::default();
                fu.face = u64::from(face);
                fu.orientation = i32::from(same);
                undefined(&mut fu.topology);
                d.faceuses.push(fu);
            }
        }
    }

    // Edges first (their kernel intervals are the trims' parameter), in the record's order.
    let mut intervals = Vec::new();
    for e in &rec.edges {
        let tolerance = e.tolerance;
        if !(tolerance <= MAX_TOLERANCE) {
            return Err(Unsupported::NotYet(
                "an edge tolerance over the kernel's limit",
            ));
        }
        if matches!(e.curve, r::Curve::SurfaceIntersection { .. }) && e.nurbs.is_none() {
            return Err(Unsupported::NotYet("traced intersection curves"));
        }
        let (curve, interval): (KernelCurve, Extent1d) = match (&e.curve, &e.nurbs) {
            (r::Curve::Circle { .. } | r::Curve::Ellipse { .. }, Some(n)) => {
                let [lo, hi] = e.interval;
                (
                    curves::conic_on_form(k, &crate::curve(&e.curve)?, &crate::curve(n)?)?,
                    Extent1d::from_raw(lo, hi),
                )
            }
            (_, Some(n)) => {
                let c = curves::from_parameters(k, &crate::curve(n)?, e.interval)?;
                (c.curve, c.interval)
            }
            (c, None) => {
                let c = curves::from_parameters(k, &crate::curve(c)?, e.interval)?;
                (c.curve, c.interval)
            }
        };
        let mut ed = EdgeData::default();
        ed.zone_tol_3d = zone_tolerance(tolerance);
        ed.interval = interval;
        ed.curve = int(d.curves_3d.len())?;
        ed.start_vertex = u64::from(e.vertices[0]);
        ed.end_vertex = u64::from(e.vertices[1]);
        ed.prim_eu = NO_OBJECT as i64;
        undefined(&mut ed.topology);
        d.curves_3d.push(Keyed {
            key: key(),
            object: curve,
        });
        d.edges.push(ed);
        intervals.push(interval);
    }

    // Edgeuses are numbered face, loop, position; each use's place in its edge's radial cycle.
    let mut numbered: Vec<((u32, u32, u32), u64)> = Vec::new();
    for (fi, f) in rec.faces.iter().enumerate() {
        for (li, l) in f.loops.iter().enumerate() {
            if let r::Loop::Edges { edges } = l {
                for p in 0..edges.len() {
                    let n = int(numbered.len())?;
                    numbered.push(((idx(fi)?, idx(li)?, idx(p)?), n));
                }
            }
        }
    }
    numbered.sort_unstable_by_key(|(place, _)| *place);
    let number = |u: &r::EdgeUseRef| -> R<u64> {
        let place = (u.face, u.r#loop, u.position);
        let i = numbered
            .binary_search_by_key(&place, |(p, _)| *p)
            .map_err(|_| Unsupported::Invalid("an edge use names no use"))?;
        numbered
            .get(i)
            .map(|(_, n)| *n)
            .ok_or(Unsupported::Invalid("edge use"))
    };
    let mut radial = vec![(u64::MAX, false); numbered.len()];
    for e in &rec.edges {
        for (j, u) in e.uses.iter().enumerate() {
            let after = e
                .uses
                .get(j + 1)
                .or_else(|| e.uses.first())
                .ok_or(Unsupported::Invalid("edge use"))?;
            let here = usize::try_from(number(u)?).map_err(|_| Unsupported::Invalid("edge use"))?;
            *radial
                .get_mut(here)
                .ok_or(Unsupported::Invalid("edge use"))? = (number(after)?, u.upper);
        }
    }

    // Faces, their loops and edgeuses.
    let mut eu = 0u64;
    for (fi, f) in rec.faces.iter().enumerate() {
        // A blend face is built on its net, its NURBS form; its definition is given back once
        // the body is built ([`adopt`]).
        let made = match (&f.surface, &f.nurbs) {
            (r::Surface::Blend { .. }, Some(net)) => {
                face_surface(k, &crate::surface(net)?, None, f.domain, &[])?
            }
            _ => face_surface(
                k,
                &crate::surface(&f.surface)?,
                f.nurbs.as_ref().map(crate::surface).transpose()?.as_ref(),
                f.domain,
                &boundary(rec, f)?,
            )?,
        };
        let mut fd = FaceData::default();
        fd.zone_tol_3d = zone;
        fd.surface = int(d.surfaces.len())?;
        fd.start_loop = int(d.loops.len())?;
        fd.num_loops = int(f.loops.len())?;
        fd.uv_domain = made.uv_domain;
        fd.rectangular_trim = UNSURE;
        undefined(&mut fd.topology);
        d.surfaces.push(Keyed {
            key: key(),
            object: made.surface,
        });
        d.faces.push(fd);
        for (li, l) in f.loops.iter().enumerate() {
            let mut ld = LoopData::default();
            undefined(&mut ld.topology);
            match l {
                r::Loop::Vertex { vertex } => {
                    ld.loop_type = 1;
                    ld.num_eu = 0;
                    ld.vertex = u64::from(*vertex);
                }
                r::Loop::Edges { edges } => {
                    ld.loop_type = 0;
                    ld.num_eu = int(edges.len())?;
                    ld.start_eu = eu;
                    for (p, u) in edges.iter().enumerate() {
                        let here = i64::try_from(eu).map_err(|_| Unsupported::Invalid("count"))?;
                        let slot =
                            usize::try_from(eu).map_err(|_| Unsupported::Invalid("count"))?;
                        let &(next, this_top) =
                            radial.get(slot).ok_or(Unsupported::Invalid("edge use"))?;
                        let next_index = i64::try_from(next).map_err(|_| {
                            Unsupported::Invalid("an edge use outside its edge's uses")
                        })?;
                        let next_slot =
                            usize::try_from(next).map_err(|_| Unsupported::Invalid("edge use"))?;
                        let next_top = radial
                            .get(next_slot)
                            .ok_or(Unsupported::Invalid("edge use"))?
                            .1;
                        let next_flag = if next_top {
                            next_index
                        } else {
                            -next_index - 1
                        };
                        let mut ud = EuData::default();
                        ud.eu_type = 0;
                        ud.edge = u64::from(u.edge);
                        ud.loop_ = int(d.loops.len())?;
                        ud.orientation = i32::from(u.forward);
                        ud.downward_lu = false;
                        undefined(&mut ud.topology);
                        // topEntry: top -> bottom -> next; bottomEntry: bottom -> top -> next
                        if this_top {
                            ud.next_eu = -here - 1;
                            ud.mate_next_eu = next_flag;
                        } else {
                            ud.next_eu = next_flag;
                            ud.mate_next_eu = here;
                        }
                        ud.uv_curve = NO_OBJECT;
                        let place = (idx(fi)?, idx(li)?, idx(p)?);
                        if made.trims && bare.binary_search(&place).is_err() {
                            let interval = intervals
                                .get(u.edge as usize)
                                .ok_or(Unsupported::Invalid("edge"))?;
                            let source_interval = rec
                                .edges
                                .get(u.edge as usize)
                                .ok_or(Unsupported::Invalid("edge"))?
                                .interval;
                            ud.uv_curve = int(d.uv_curves.len())?;
                            let trim = curves::uv_trim_on_edge(
                                &crate::curve2(&u.pcurve),
                                source_interval,
                                interval,
                                made.scale,
                            )?;
                            d.uv_curves.push(Keyed {
                                key: key(),
                                object: trim,
                            });
                        }
                        // the primary edgeuse is the first of its edge's in this order
                        let ed = d
                            .edges
                            .get_mut(u.edge as usize)
                            .ok_or(Unsupported::Invalid("edge"))?;
                        if ed.prim_eu == NO_OBJECT as i64 {
                            ed.prim_eu = if this_top { here } else { -here - 1 };
                        }
                        d.edgeuses.push(ud);
                        eu += 1;
                    }
                }
            }
            d.loops.push(ld);
        }
    }

    // A wire edge owns a radial cycle of its two opposite uses, attached to its shell.
    for (shell, edges) in wire_shells {
        for &edge in edges {
            let here = i64::try_from(d.edgeuses.len())
                .map_err(|_| Unsupported::Invalid("edge use count"))?;
            let mut ud = EuData::default();
            ud.eu_type = 2;
            ud.edge = u64::from(edge);
            ud.shell = shell;
            ud.orientation = 1;
            ud.uv_curve = NO_OBJECT;
            ud.next_eu = -here - 1;
            ud.mate_next_eu = here;
            undefined(&mut ud.topology);
            d.edges
                .get_mut(edge as usize)
                .ok_or(Unsupported::Invalid("wire edge"))?
                .prim_eu = here;
            d.edgeuses.push(ud);
        }
    }

    // Vertices, each with its own tolerance.
    for v in &rec.vertices {
        let tolerance = v.tolerance;
        if !(tolerance <= MAX_TOLERANCE) {
            return Err(Unsupported::NotYet(
                "a vertex tolerance over the kernel's limit",
            ));
        }
        let mut vd = VertexData::default();
        vd.zone_tol_3d = zone_tolerance(tolerance);
        undefined(&mut vd.topology);
        vd.point = Vec3::new(v.point[0], v.point[1], v.point[2]);
        d.vertices.push(vd);
    }
    own_tolerances(&mut d, rec, zone)?;
    Ok(d)
}

/// The body's and each face's tolerance from the edges' and vertices' own, as SolidCraft's
/// flat import took them (`own_tolerances`, after the capi's): the kernel's build raises every
/// entity to the body's, so the body's is the least of the record's and its edges' and
/// vertices', and a face's the least of its loops' edges' and vertices', at most the record's.
/// Each entity keeps its own across the crossing, none raised to the body's.
///
/// This keeps invariant 5 of `docs/validity-and-tolerances.md` §10 (every local tolerance at
/// least the body's) by the other route from raising each entity to the body's (8252ae3e93,
/// "never below the body's"): the body comes down to the least of them instead, so no entity
/// is below it and none is changed. A face's is at least the body's (both are minima, the
/// body's over more) and at most its loops' edges' and vertices'.
///
/// # Errors
///
/// [`Unsupported::Invalid`] when the least zone is not positive and finite: a tolerance so
/// small that its zone (half of it) underflows to zero passes the record's validation.
fn own_tolerances(d: &mut BrepData, rec: &r::Brep, zone: f64) -> R<()> {
    let edge_zone = |e: u32| -> R<f64> { Ok(at(&d.edges, e)?.zone_tol_3d) };
    let vertex_zone = |v: u32| -> R<f64> { Ok(at(&d.vertices, v)?.zone_tol_3d) };
    let mut faces = Vec::with_capacity(rec.faces.len());
    for f in &rec.faces {
        let mut t = f64::INFINITY;
        for l in &f.loops {
            match l {
                r::Loop::Edges { edges } => {
                    for oe in edges {
                        t = t.min(edge_zone(oe.edge)?);
                    }
                }
                r::Loop::Vertex { vertex } => t = t.min(vertex_zone(*vertex)?),
            }
        }
        faces.push(if t.is_finite() { t.min(zone) } else { zone });
    }
    let least = d
        .edges
        .iter()
        .map(|e| e.zone_tol_3d)
        .chain(d.vertices.iter().map(|v| v.zone_tol_3d))
        .fold(zone, f64::min);
    if !(least.is_finite() && least > 0.0) {
        return Err(Unsupported::Invalid(
            "a tolerance whose zone (half of it) is zero or not finite",
        ));
    }
    d.zone_tol_3d = least;
    for (fd, t) in d.faces.iter_mut().zip(faces) {
        fd.zone_tol_3d = t;
    }
    Ok(())
}

/// Give the record's blend faces their definitions back on the body built from it: each face's
/// surface, its net, becomes the blend its definition makes, the net certified against it again
/// ([`adopt_definitions`]); a net its definition does not certify refuses the record. `faces` are
/// the body's faces in the record's order.
fn adopt(k: &Kernel, b: &mut Brep, record: &r::Brep, faces: &[FaceId]) -> R<()> {
    let mut definitions = Vec::new();
    for (i, f) in record.faces.iter().enumerate() {
        if let r::Surface::Blend { definition } = &f.surface {
            let face = *faces
                .get(i)
                .ok_or(Unsupported::Invalid("a face the body does not have"))?;
            definitions.push((face, crate::blend::blend(definition)));
        }
    }
    if !definitions.is_empty() {
        adopt_definitions(k, b, &notify_ref, &definitions)?;
    }
    Ok(())
}

/// The curves of a ball patch face's loops, for [`face_surface`] to certify on its sphere:
/// each edge's NURBS form, or its curve; empty for any other face, whose label is not
/// checked against its loops.
fn boundary(rec: &r::Brep, f: &r::Face) -> R<Vec<solidcore::eval::Curve>> {
    if !matches!(f.surface, r::Surface::BallPatch { .. }) {
        return Ok(Vec::new());
    }
    let mut curves = Vec::new();
    for l in &f.loops {
        if let r::Loop::Edges { edges } = l {
            for u in edges {
                let e = at(&rec.edges, u.edge)?;
                curves.push(crate::curve(e.nurbs.as_ref().unwrap_or(&e.curve))?);
            }
        }
    }
    Ok(curves)
}

fn idx(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| Unsupported::Invalid("index"))
}

/// Whether the kernel holds `label` over `domain` as one of its analytic surfaces, proved: the
/// face's surface as the kernel makes it from the record ([`face_surface`]), read back as the
/// record's analytic kind when it is one.
///
/// # Errors
///
/// Those of [`face_surface`].
pub fn recognised(
    k: &mut Kernel,
    label: &r::Surface,
    domain: [[f64; 2]; 2],
) -> R<Option<&'static str>> {
    use solidcore::surface::surface::SurfaceKind;
    let made = face_surface(k, &crate::surface(label)?, None, domain, &[])?;
    Ok(match made.surface.kind() {
        SurfaceKind::Plane(_) => Some("plane"),
        SurfaceKind::Cone(c) if c.is_cylinder_class() => Some("cylinder"),
        SurfaceKind::Cone(_) => Some("cone"),
        SurfaceKind::Sphere(_) => Some("sphere"),
        SurfaceKind::Torus(_) => Some("torus"),
        _ => None,
    })
}

mod record;
pub use record::to_record;
