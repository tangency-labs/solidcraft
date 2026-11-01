//! Topology-first construction: a body built from the file's own topology.
//!
//! The faces, loops, edges and vertices the file shares are made once each and joined
//! as the file joins them, with no geometric stitching, through the kernel's
//! `BrepConstructor` (`SmBrepConstructor`). The calls are those of
//! [`step_import::script::from_body`]: per shell its region and shell, per face its
//! surface (over the face's STEP parameter box) and loops, every oriented edge joined
//! to the edge an earlier face made, and edge limits at the vertex points.
//!
//! A face whose calls fail is left out: the body is built again without it (the edges
//! and vertices it would have made first are then made by the next face that uses
//! them), up to [`skip_limit`] faces, the first failing face in file order each time.
//! Each left-out face is reported with the call that failed.

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep_constructor::{BrepConstructor, VertexNode};
use solidcore::topology::{Brep, EdgeId, FaceId, VertexId};
use solidcore::SolidError;
use step_import::model::{Body, BodyKind, Bound, Face, Shell};

/// A face the topology-first build left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceFailure {
    /// `#id` of the body.
    pub body: u64,
    /// `#id` of the face.
    pub face: u64,
    /// The call that failed and its error.
    pub reason: String,
}

/// Why a build stopped: the failing face (its position among the faces built, in file
/// order) or none for a call outside any face, and the failing call.
struct Failure {
    face: Option<usize>,
    reason: String,
}

/// The most faces of a body of `faces` the build may leave out: 16, or 1% of a larger
/// body.
pub(crate) fn skip_limit(faces: usize) -> usize {
    16usize.max(faces / 100)
}

/// What [`construct`] built.
pub(crate) struct Constructed {
    /// The kernel it was built with.
    pub kernel: Kernel,
    /// The B-rep.
    pub brep: Brep,
    /// Each built face with its file face's `#id`.
    pub faces: Vec<(FaceId, u64)>,
    /// The faces left out.
    pub failures: Vec<FaceFailure>,
}

/// Builds `body` from its topology, leaving out faces whose calls fail.
///
/// # Errors
///
/// The reason when a call outside any face fails, more than [`skip_limit`] faces fail
/// or no face is built.
pub(crate) fn construct(body: &Body) -> Result<Constructed, String> {
    let mut trial = body.clone();
    let mut failures = Vec::new();
    let limit = skip_limit(body.faces.len());
    loop {
        let mut kernel = Kernel::default();
        let mut faces = Vec::new();
        match build(&mut kernel, &trial, &mut faces) {
            Ok(_) if faces.is_empty() => return Err("no face could be built".to_string()),
            Ok(brep) => {
                return Ok(Constructed {
                    kernel,
                    brep,
                    faces,
                    failures,
                })
            }
            Err(Failure {
                face: Some(position),
                reason,
            }) => {
                let face = remove_face(&mut trial, position)
                    .ok_or_else(|| format!("failing face {position} not found: {reason}"))?;
                failures.push(FaceFailure {
                    body: body.step_id,
                    face,
                    reason,
                });
                if failures.len() > limit {
                    return Err(format!("more than {limit} faces failed"));
                }
            }
            Err(Failure { face: None, reason }) => return Err(reason),
        }
    }
}

/// The faces [`build`] builds, in order: shell by shell, in each shell's face order,
/// skipping missing faces.
fn faces_in_order(body: &Body) -> impl Iterator<Item = (usize, usize, &Face)> + '_ {
    body.shells.iter().enumerate().flat_map(move |(s, shell)| {
        shell
            .faces
            .iter()
            .enumerate()
            .filter_map(move |(slot, &fi)| body.faces.get(fi).map(|face| (s, slot, face)))
    })
}

/// Removes the `position`th face [`build`] builds from its shell; returns its `#id`.
fn remove_face(body: &mut Body, position: usize) -> Option<u64> {
    let (s, slot, id) = faces_in_order(body)
        .nth(position)
        .map(|(s, slot, face)| (s, slot, face.step_id))?;
    body.shells.get_mut(s)?.faces.remove(slot);
    Some(id)
}

fn point(p: [f64; 3]) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

fn orient(same: bool) -> OrientType {
    if same {
        OrientType::Same
    } else {
        OrientType::Opposite
    }
}

/// `what` failed with `e`.
fn call(what: &'static str) -> impl Fn(SolidError) -> String {
    move |e| format!("{what}: {e}")
}

/// One build of `body` on a new `BrepConstructor`; `faces` gets each built face with
/// its file face's `#id`.
fn build(k: &mut Kernel, body: &Body, faces: &mut Vec<(FaceId, u64)>) -> Result<Brep, Failure> {
    let outside = |reason: String| Failure { face: None, reason };
    let solid = !matches!(body.kind, BodyKind::ShellModel);
    let mut c = BrepConstructor::new();
    c.start_brep(k, solidcore::geom::tol::USE_DEFAULT)
        .map_err(call("StartBrep"))
        .map_err(outside)?;
    let mut edges: Vec<Option<EdgeId>> = vec![None; body.edges.len()];
    let mut vertices: Vec<Option<VertexId>> = vec![None; body.vertices.len()];
    let mut first_region = None;
    let mut position = 0usize;
    for (si, shell) in body.shells.iter().enumerate() {
        let void = solid && si > 0 && matches!(body.kind, BodyKind::SolidWithVoids);
        if solid {
            let region = c
                .start_region(true, if void { first_region } else { None })
                .map_err(call("StartRegion"))
                .map_err(outside)?;
            first_region = first_region.or(Some(region));
        }
        c.start_shell()
            .map_err(call("StartShell"))
            .map_err(outside)?;
        for &fi in &shell.faces {
            let Some(face) = body.faces.get(fi) else {
                continue;
            };
            let made = face_calls(
                k,
                &mut c,
                body,
                shell,
                face,
                void,
                &mut edges,
                &mut vertices,
            )
            .map_err(|reason| Failure {
                face: Some(position),
                reason,
            })?;
            faces.push((made, face.step_id));
            position += 1;
        }
        c.end_shell(k, false)
            .map_err(call("EndShell"))
            .map_err(outside)?;
        if solid {
            c.end_region().map_err(call("EndRegion")).map_err(outside)?;
        }
    }
    c.end_brep().map_err(call("EndBrep")).map_err(outside)
}

/// The calls of one face; returns the face made.
fn face_calls(
    k: &mut Kernel,
    c: &mut BrepConstructor,
    body: &Body,
    shell: &Shell,
    face: &Face,
    void: bool,
    edges: &mut [Option<EdgeId>],
    vertices: &mut [Option<VertexId>],
) -> Result<FaceId, String> {
    let vertex_point = |v: usize| body.vertices.get(v).map_or([0.0; 3], |x| x.point);
    // the face normal (after the shell's orientation) points away from the material
    let normal_is_surface_normal = face.same_sense == shell.orientation;
    let mut surface = crate::make_surface(k, &face.surface, None).map_err(call("face surface"))?;
    if let Some(d) = step_import::script::step_uv_domain(body, face) {
        surface
            .adjust_step_uv_domain(k, &Extent2d::from_bounds(d[0], d[1], d[2], d[3]))
            .map_err(call("AdjustSTEPUVDomain"))?;
    }
    let made = c
        .start_face(orient(normal_is_surface_normal != void))
        .map_err(call("StartFace"))?;
    let domain = surface
        .natural_uv_domain_with_nurb(k)
        .map_err(call("GetNaturalUVDomain"))?;
    c.set_face_surface(surface, &domain)
        .map_err(call("SetFaceSurface"))?;
    // outer loop first
    let loops = face
        .loops
        .iter()
        .filter(|l| l.outer)
        .chain(face.loops.iter().filter(|l| !l.outer));
    for l in loops {
        let uses = match &l.bound {
            Bound::Vertex(v) => {
                c.start_and_end_single_vertex_loop(point(vertex_point(*v)))
                    .map_err(call("StartAndEndSingleVertexLoop"))?;
                continue;
            }
            Bound::Edges(uses) => uses,
        };
        // the loop runs counter-clockwise about the face's own normal
        c.start_loop(orient(face.same_sense))
            .map_err(call("StartLoop"))?;
        let uses: Vec<(usize, bool)> = if l.orientation {
            uses.iter().map(|u| (u.edge, u.forward)).collect()
        } else {
            uses.iter().rev().map(|u| (u.edge, !u.forward)).collect()
        };
        for (ei, forward) in uses {
            let Some(e) = body.edges.get(ei) else {
                continue;
            };
            let o = orient(forward == e.same_sense);
            if let Some(existing) = edges.get(ei).copied().flatten() {
                c.start_edge(k, o, Some(existing), None, None)
                    .map_err(call("StartEdge"))?;
                c.end_edge().map_err(call("EndEdge"))?;
                continue;
            }
            // the edge's vertices in its curve's direction, made at first use
            let (a, b) = if e.same_sense {
                (e.start, e.end)
            } else {
                (e.end, e.start)
            };
            let mut ends = [None, None];
            for (slot, v) in ends.iter_mut().zip([a, b]) {
                if let Some(id) = vertices.get_mut(v) {
                    if id.is_none() {
                        *id = Some(
                            c.start_vertex_of_loop(point(vertex_point(v)))
                                .map_err(call("StartVertexOfLoop"))?,
                        );
                    }
                    *slot = *id;
                }
            }
            let [start, end] = ends;
            let id = c
                .start_edge(k, o, None, start, end)
                .map_err(call("StartEdge"))?;
            if let Some(slot) = edges.get_mut(ei) {
                *slot = Some(id);
            }
            let (lo, hi) = if e.same_sense {
                (e.t_start, e.t_end)
            } else {
                (e.t_end, e.t_start)
            };
            let curve = crate::make_curve(k, &e.curve, Some((lo.min(hi), lo.max(hi))))
                .map_err(call("edge curve"))?;
            c.set_edge_curve(k, Some(curve), false)
                .map_err(call("SetEdgeCurve"))?;
            c.set_limits(
                k,
                &VertexNode::from_point(point(vertex_point(a))),
                &VertexNode::from_point(point(vertex_point(b))),
            )
            .map_err(call("SetLimits"))?;
            c.end_edge().map_err(call("EndEdge"))?;
        }
        c.end_loop(k).map_err(call("EndLoop"))?;
    }
    c.end_face().map_err(call("EndFace"))?;
    Ok(made)
}
