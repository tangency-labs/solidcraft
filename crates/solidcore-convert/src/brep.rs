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

use solidcore::curve::curve::Curve as KernelCurve;
use solidcore::curve::from_parameters::{
    self as curves, approx_tolerance, zone_tolerance, MAX_TOLERANCE,
};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::Kernel;
use solidcore::surface::face_surface::face_surface;
use solidcore::topology::brep::enclosing::put_enclosing_shells_first;
use solidcore::topology::brep::p10::{
    make_topology_from_data, MakeTopologyOptions, TopologyEnv, HEALER_VERSION,
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

impl From<SolidError> for Unsupported {
    fn from(e: SolidError) -> Self {
        Unsupported::Kernel(e)
    }
}

type R<T> = Result<T, Unsupported>;

/// The kernel entity each record index names: `faces[i]` is record face `i`, and so for edges and
/// vertices. Several record edges may name one kernel edge where solids touch along it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Order {
    /// Record face `i`'s kernel face.
    pub faces: Vec<FaceId>,
    /// Record edge `i`'s kernel edge.
    pub edges: Vec<EdgeId>,
    /// Record vertex `i`'s kernel vertex.
    pub vertices: Vec<VertexId>,
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
/// kernel's limit; [`Unsupported::NotYet`] for wire and vertex shells, wire edges and traced
/// intersection curves; [`Unsupported::Kernel`] for a step the kernel refuses.
pub fn to_kernel(k: &mut Kernel, record: &r::Brep, trusted: bool) -> R<(Brep, Order)> {
    record
        .validate_built()
        .map_err(|e| Unsupported::Invalid(e.0))?;
    let mut data = data(k, record)?;
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
    let options = MakeTopologyOptions {
        make_uv_trim_curves: false,
        add_analytics: false,
        force_same_tol: true,
        check_uv_trim_curves: false,
        always_heal: false,
        healer_enabled: true,
    };
    make_topology_from_data(k, &mut b, &mut data, &mut env, options)?;
    put_enclosing_shells_first(k, &mut b)?;
    // The kernel lists its faces, edges and vertices in the data's order, which is the record's.
    let order = Order {
        faces: b.face_list().to_vec(),
        edges: b.edge_list().to_vec(),
        vertices: b.vertex_list().to_vec(),
    };
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
    Ok((b, order))
}

/// The record as the kernel's B-rep data: edgeuses face by face, loop by loop, in loop order;
/// edges and vertices in the record's order.
fn data(k: &Kernel, rec: &r::Brep) -> R<BrepData> {
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
            let r::Shell::Faces { faces, wires } = at(&rec.shells, s)? else {
                return Err(Unsupported::NotYet("wire and vertex shells"));
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
        let tolerance = e.tolerance.max(rec.tolerance);
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
                    curves::conic_on_form(k, &crate::curve(&e.curve), &crate::curve(n))?,
                    Extent1d::from_raw(lo, hi),
                )
            }
            (_, Some(n)) => {
                let c = curves::from_parameters(k, &crate::curve(n), e.interval)?;
                (c.curve, c.interval)
            }
            (c, None) => {
                let c = curves::from_parameters(k, &crate::curve(c), e.interval)?;
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
    let mut eu_face: Vec<usize> = Vec::new();
    let mut loop_vertices: Vec<(usize, u64)> = Vec::new();
    for (fi, f) in rec.faces.iter().enumerate() {
        let made = face_surface(
            k,
            &crate::surface(&f.surface),
            f.nurbs.as_ref().map(crate::surface).as_ref(),
            f.domain,
        )?;
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
        for l in &f.loops {
            let mut ld = LoopData::default();
            undefined(&mut ld.topology);
            match l {
                r::Loop::Vertex { vertex } => {
                    ld.loop_type = 1;
                    ld.num_eu = 0;
                    ld.vertex = u64::from(*vertex);
                    loop_vertices.push((fi, ld.vertex));
                }
                r::Loop::Edges { edges } => {
                    ld.loop_type = 0;
                    ld.num_eu = int(edges.len())?;
                    ld.start_eu = eu;
                    for u in edges {
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
                        if made.trims {
                            let interval = intervals
                                .get(u.edge as usize)
                                .ok_or(Unsupported::Invalid("edge"))?;
                            ud.uv_curve = int(d.uv_curves.len())?;
                            let trim = curves::uv_trim(&crate::curve2(&u.pcurve), interval)?;
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
                        eu_face.push(fi);
                        eu += 1;
                    }
                }
            }
            d.loops.push(ld);
        }
    }

    // Vertices, each with its own tolerance, never below the body's.
    for v in &rec.vertices {
        let tolerance = v.tolerance.max(rec.tolerance);
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
    own_tolerances(&mut d, &eu_face, &loop_vertices)?;
    Ok(d)
}

fn idx(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| Unsupported::Invalid("index"))
}

/// The body's and each face's tolerance from the edges' and vertices' own. The kernel's build
/// raises every entity to the body's tolerance: taken as the largest, it would become every
/// entity's, and each crossing between record and kernel spread the largest over the whole body.
/// Each edge or vertex tolerance is at least the body's, so the body's is the least of them and a
/// face's the least of its edges' and loop vertices'.
fn own_tolerances(d: &mut BrepData, eu_face: &[usize], loop_vertices: &[(usize, u64)]) -> R<()> {
    let zone = d.zone_tol_3d;
    let mut faces = vec![f64::INFINITY; d.faces.len()];
    let mut lower = |face: usize, t: f64| -> R<()> {
        let slot = faces
            .get_mut(face)
            .ok_or(Unsupported::Invalid("edgeuse face"))?;
        *slot = slot.min(t);
        Ok(())
    };
    for (u, &face) in d.edgeuses.iter().zip(eu_face) {
        let edge = usize::try_from(u.edge).map_err(|_| Unsupported::Invalid("edge"))?;
        lower(
            face,
            d.edges
                .get(edge)
                .ok_or(Unsupported::Invalid("edgeuse edge"))?
                .zone_tol_3d,
        )?;
    }
    for &(face, vertex) in loop_vertices {
        let v = usize::try_from(vertex).map_err(|_| Unsupported::Invalid("vertex"))?;
        lower(
            face,
            d.vertices
                .get(v)
                .ok_or(Unsupported::Invalid("loop vertex"))?
                .zone_tol_3d,
        )?;
    }
    let least = d
        .edges
        .iter()
        .map(|e| e.zone_tol_3d)
        .chain(d.vertices.iter().map(|v| v.zone_tol_3d))
        .fold(zone, f64::min);
    if !(least.is_finite() && least > 0.0) {
        return Ok(());
    }
    d.zone_tol_3d = least;
    for (fd, t) in d.faces.iter_mut().zip(faces) {
        fd.zone_tol_3d = if t.is_finite() { t.min(zone) } else { zone };
    }
    Ok(())
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
    let made = face_surface(k, &crate::surface(label), None, domain)?;
    Ok(match made.surface.kind() {
        SurfaceKind::Plane(_) => Some("plane"),
        SurfaceKind::Cone(c) if c.is_cylinder_class() => Some("cylinder"),
        SurfaceKind::Cone(_) => Some("cone"),
        SurfaceKind::Sphere(_) => Some("sphere"),
        SurfaceKind::Torus(_) => Some("torus"),
        _ => None,
    })
}

/// The kernel's body as the record, in canonical order, and where each record face, edge and
/// vertex is in the kernel.
///
/// # Errors
///
/// [`Unsupported::NotYet`]: the kernel-to-record half of the direct conversion is the next
/// commit of this change.
pub fn to_record(_k: &mut Kernel, _b: &mut Brep) -> R<(r::Brep, Order)> {
    Err(Unsupported::NotYet("the kernel body as a record"))
}
