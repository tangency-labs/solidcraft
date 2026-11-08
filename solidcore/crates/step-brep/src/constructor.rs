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
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep_constructor::{BrepConstructor, VertexNode};
use solidcore::topology::{Brep, EdgeId, FaceId, VertexId};
use solidcore::SolidError;
use step_import::model::{Body, BodyKind, Bound, Face, Shell, Surface as StepSurface};

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
            let loops = normalized_face_loops(k, body, face).map_err(outside)?;
            let made = face_calls(
                k,
                &mut c,
                body,
                shell,
                face,
                loops,
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

/// A face loop ready for B-rep construction: oriented edge uses `(edge_index, forward)`
/// or a single-vertex loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NormalizedLoop {
    /// `VERTEX_LOOP`: index into [`Body::vertices`].
    Vertex(usize),
    /// Oriented `EDGE_LOOP` uses `(edge_index, forward)`.
    Edges(Vec<(usize, bool)>),
}

fn surface_kind_name(s: &StepSurface) -> &'static str {
    match s {
        StepSurface::Plane { .. } => "plane",
        StepSurface::Cylinder { .. } => "cylinder",
        StepSurface::Cone { .. } => "cone",
        StepSurface::Sphere { .. } => "sphere",
        StepSurface::Torus { .. } => "torus",
        StepSurface::Revolution { .. } => "surface of revolution",
        StepSurface::Extrusion { .. } => "surface of linear extrusion",
        StepSurface::BSpline(_) => "B-spline surface",
    }
}

/// Orders `face`'s bounds with the outer boundary first and splices vertex-touching
/// loops into valid single-walk boundary cycles (`SmFace::AssertValid` Face Test 39).
///
/// # Outer-loop selection
/// - At most one `FACE_OUTER_BOUND` (`l.outer == true`) may appear on a STEP
///   `ADVANCED_FACE` (ISO 10303-42); two or more return `Err`.
/// - When no `FACE_OUTER_BOUND` is present and `face.loops.len() > 1`:
///   - **Planar faces (`StepSurface::Plane`):** largest 3D tight bounding-box diagonal
///     ([`Curve::calculate_tight_bounding_box`]) is valid **only on planes**, where every
///     interior hole Jordan curve lies strictly inside the outer Jordan curve and has a
///     strictly smaller 3D bounding box. Two or more loops tying for the largest squared
///     diagonal within `EFF_ZERO * best_diag_sq.max(1.0)` return `Err` rather than picking
///     arbitrarily.
///   - **Periodic or closed non-planar surfaces** (`closed_u || closed_v`: cylinders,
///     cones, spheres, tori, surfaces of revolution, and periodic swept or B-spline
///     surfaces): two bounding loops can bound a band without enclosing a topological
///     disk in `(u, v)` or have equal/reversed 3D diagonals, so missing
///     `FACE_OUTER_BOUND` returns `Err` rather than guessing.
///   - **Open non-periodic parameter sheets** (`!closed_u && !closed_v`, e.g. open
///     B-spline patches): 3D bounding boxes are still not used; instead the kernel's
///     [`solidcore::topology::face::p3::compute_loop_orientation_3d`] classifies each
///     loop's UV winding and UV bounds on the surface, accepting an outer loop iff
///     exactly one loop winds CCW (`OrientType::Same`) in `(u, v)` and encloses any
///     resolved interior hole bounds, else returning `Err`.
///
/// # Vertex-touching loop splice invariants and termination
/// - **Orientation invariant:** before splicing, each loop's STEP `l.orientation` is
///   applied so face material lies to the left of every directed edge use around the
///   face normal: `slots[0]` is the single CCW outer loop and `slots[1..]` are CW hole
///   loops. When two non-crossing loops in `slots[i]` and `slots[j]` (`0 <= i < j`)
///   share a vertex `v = start(uses_i[ei]) == start(uses_j[ej])`, splicing
///   `uses_i[..ei] + uses_j[ej..] + uses_j[..ej] + uses_i[ei..]` into `slots[i]`
///   concatenates two directed cycles that both keep material on their left into one
///   Eulerian boundary walk preserving material-on-left everywhere:
///   - Opposite-sense interior tangency (`i == 0` CCW outer loop and `j > 0` CW hole
///     loop touching from inside, or two boundary loops on a curved face sharing a
///     seam/tangency vertex) produces a single CCW outer loop in `slots[0]`.
///   - Same-sense exterior tangency (`0 < i < j`, two CW hole loops touching externally
///     at a vertex, such as bolt-hole circles tangent to a central bore circle)
///     produces a single CW figure-8 hole loop in `slots[i]`. (Concentric nested loops
///     of the same sense cannot share a vertex without crossing or coinciding, and
///     multiple CCW outer loops are rejected above.)
/// - **Two shared vertices between two loops:** the search pairs distinct occupied slots
///   (`i < j`). When two loops share two vertices, the first merge combines `slots[j]`
///   into `slots[i]` at the first shared vertex and clears `slots[j] = None`; the second
///   shared vertex then appears twice inside the single walk in `slots[i]` and is not
///   spliced a second time across slots.
/// - **Termination:** `slots` starts with `m = ordered.len()` occupied `Some(_)`
///   entries. Every iteration that finds a shared vertex takes two `Some` entries
///   (`i < j`), writes one merged `Some` back to `slots[i]`, and leaves `slots[j]` as
///   `None`, strictly decreasing the count of `Some` entries by `1`. The loop therefore
///   terminates in at most `m - 1` merges, including chains where a third or later loop
///   touches the already-merged walk in `slots[i]`.
pub(crate) fn normalized_face_loops(
    k: &Kernel,
    body: &Body,
    face: &Face,
) -> Result<Vec<NormalizedLoop>, String> {
    let outer_count = face.loops.iter().filter(|l| l.outer).count();
    if outer_count > 1 {
        return Err(format!(
            "face #{} has {outer_count} FACE_OUTER_BOUND loops (at most 1 allowed)",
            face.step_id
        ));
    }

    let mut ordered: Vec<&step_import::model::Loop> = Vec::new();
    if outer_count == 1 {
        ordered.extend(face.loops.iter().filter(|l| l.outer));
        ordered.extend(face.loops.iter().filter(|l| !l.outer));
    } else if face.loops.len() > 1 {
        let best_idx = if matches!(face.surface, StepSurface::Plane { .. }) {
            let mut diags = Vec::new();
            let mut idx = 0usize;
            let mut best_diag_sq = -1.0_f64;
            for (i, l) in face.loops.iter().enumerate() {
                let diag_sq = loop_bbox_diag_sq(k, body, l)?;
                if diag_sq > best_diag_sq {
                    best_diag_sq = diag_sq;
                    idx = i;
                }
                diags.push(diag_sq);
            }
            let tie_tol = solidcore::math::EFF_ZERO * best_diag_sq.max(1.0);
            for (i, &diag_sq) in diags.iter().enumerate() {
                if i != idx && (best_diag_sq - diag_sq).abs() <= tie_tol {
                    return Err(format!(
                        "face #{} on plane has {} bounds, no FACE_OUTER_BOUND, and tied largest bounding-box diagonals on loops {idx} and {i}",
                        face.step_id,
                        face.loops.len()
                    ));
                }
            }
            idx
        } else {
            find_outer_loop_on_open_sheet(k, body, face)?
        };
        if let Some(outer) = face.loops.get(best_idx) {
            ordered.push(outer);
        }
        for (i, l) in face.loops.iter().enumerate() {
            if i != best_idx {
                ordered.push(l);
            }
        }
    } else {
        ordered.extend(face.loops.iter());
    }

    let mut slots: Vec<Option<NormalizedLoop>> = ordered
        .into_iter()
        .map(|l| Some(orient_step_loop(l)))
        .collect();

    loop {
        let mut found = None;
        'search: for (i, slot_i) in slots.iter().enumerate() {
            let Some(NormalizedLoop::Edges(uses_i)) = slot_i else {
                continue;
            };
            for (j_off, slot_j) in slots.iter().skip(i + 1).enumerate() {
                let j = i + 1 + j_off;
                let Some(NormalizedLoop::Edges(uses_j)) = slot_j else {
                    continue;
                };
                for (ei, &(edge_i, fwd_i)) in uses_i.iter().enumerate() {
                    let Some(e_i) = body.edges.get(edge_i) else {
                        continue;
                    };
                    let vi = if fwd_i { e_i.start } else { e_i.end };
                    for (ej, &(edge_j, fwd_j)) in uses_j.iter().enumerate() {
                        let Some(e_j) = body.edges.get(edge_j) else {
                            continue;
                        };
                        let vj = if fwd_j { e_j.start } else { e_j.end };
                        if vi == vj {
                            found = Some((i, j, ei, ej));
                            break 'search;
                        }
                    }
                }
            }
        }
        let Some((i, j, ei, ej)) = found else {
            break;
        };
        let taken_j = slots.get_mut(j).and_then(Option::take);
        let taken_i = slots.get_mut(i).and_then(Option::take);
        if let (Some(NormalizedLoop::Edges(uses_i)), Some(NormalizedLoop::Edges(uses_j))) =
            (taken_i, taken_j)
        {
            let mut merged = Vec::new();
            merged.extend(uses_i.iter().copied().take(ei));
            merged.extend(uses_j.iter().copied().skip(ej));
            merged.extend(uses_j.iter().copied().take(ej));
            merged.extend(uses_i.iter().copied().skip(ei));
            if let Some(slot) = slots.get_mut(i) {
                *slot = Some(NormalizedLoop::Edges(merged));
            }
        } else {
            break;
        }
    }

    Ok(slots.into_iter().flatten().collect())
}

fn orient_step_loop(l: &step_import::model::Loop) -> NormalizedLoop {
    match &l.bound {
        Bound::Vertex(v) => NormalizedLoop::Vertex(*v),
        Bound::Edges(uses) => {
            let oriented_uses: Vec<(usize, bool)> = if l.orientation {
                uses.iter().map(|u| (u.edge, u.forward)).collect()
            } else {
                uses.iter().rev().map(|u| (u.edge, !u.forward)).collect()
            };
            NormalizedLoop::Edges(oriented_uses)
        }
    }
}

/// Identifies the unique outer loop on a multi-bound non-planar face lacking
/// `FACE_OUTER_BOUND` when `face.surface` is an open non-periodic parameter sheet
/// (`!closed_u && !closed_v`), using the kernel's
/// [`solidcore::topology::face::p3::compute_loop_orientation_3d`]. Periodic or closed
/// surfaces (cylinders, cones, spheres, tori, surfaces of revolution, periodic swept or
/// B-spline surfaces) and ambiguous UV windings return `Err` instead of guessing.
fn find_outer_loop_on_open_sheet(k: &Kernel, body: &Body, face: &Face) -> Result<usize, String> {
    use solidcore::nurbs::b_spline_surface::p1::SurfParamType;

    let mut surface = crate::make_surface(k, &face.surface, None).map_err(call("face surface"))?;
    if let Some(d) = step_import::script::step_uv_domain(body, face) {
        surface
            .adjust_step_uv_domain(k, &Extent2d::from_bounds(d[0], d[1], d[2], d[3]))
            .map_err(call("AdjustSTEPUVDomain"))?;
    }
    let domain = surface
        .natural_uv_domain_with_nurb(k)
        .map_err(call("GetNaturalUVDomain"))?;
    let closed_u = surface
        .is_closed(k, &domain, SurfParamType::U, None, false)
        .map_err(call("IsClosed"))?
        .0;
    let closed_v = surface
        .is_closed(k, &domain, SurfParamType::V, None, false)
        .map_err(call("IsClosed"))?
        .0;
    if closed_u || closed_v {
        return Err(format!(
            "face #{} on {} has {} bounds and no FACE_OUTER_BOUND",
            face.step_id,
            surface_kind_name(&face.surface),
            face.loops.len()
        ));
    }

    // Copy the caller's tolerances, RNG state, active upstream fixes, and cancellation
    // token onto a temporary probe kernel (`normalized_face_loops` borrows `&Kernel`),
    // and pass `USE_DEFAULT` to `start_brep` (matching `constructor::build`) solely to
    // wire temporary topological loop entities on `probe_brep` for
    // `compute_loop_orientation_3d`.
    let mut probe_k = Kernel {
        tolerances: k.tolerances,
        rng: k.rng.clone(),
        fixes: k.fixes,
        cancel: k.cancel.clone(),
        ..Kernel::default()
    };
    let mut c = BrepConstructor::new();
    c.start_brep(&mut probe_k, solidcore::geom::tol::USE_DEFAULT)
        .map_err(call("StartBrep"))?;
    c.start_shell().map_err(call("StartShell"))?;
    let probe_shell = Shell {
        step_id: 0,
        closed: false,
        orientation: true,
        faces: Vec::new(),
    };
    let raw_loops: Vec<NormalizedLoop> = face.loops.iter().map(orient_step_loop).collect();
    let mut probe_edges = vec![None; body.edges.len()];
    let mut probe_vertices = vec![None; body.vertices.len()];
    let probe_face = face_calls(
        &mut probe_k,
        &mut c,
        body,
        &probe_shell,
        face,
        raw_loops,
        false,
        &mut probe_edges,
        &mut probe_vertices,
    )?;
    c.end_shell(&mut probe_k, false).map_err(call("EndShell"))?;
    let probe_brep = c.end_brep().map_err(call("EndBrep"))?;

    let loop_ids = solidcore::topology::face::p2::loops(&probe_brep, probe_face, false, None)
        .map_err(call("GetLoops"))?;
    let mut outer_candidates = Vec::new();
    let mut all_uv_boxes = Vec::new();
    for (idx, &lid) in loop_ids.iter().enumerate() {
        let mut uv_box = Extent2d::new();
        let orient = solidcore::topology::face::p3::compute_loop_orientation_3d(
            &probe_k,
            &probe_brep,
            probe_face,
            lid,
            Some(&mut uv_box),
        )
        .map_err(call("ComputeLoopOrientation"))?;
        if orient == OrientType::Same {
            outer_candidates.push((idx, uv_box));
        }
        all_uv_boxes.push((idx, uv_box));
    }
    let [(outer_idx, outer_uv)] = outer_candidates.as_slice() else {
        return Err(format!(
            "face #{} on {} has {} bounds, no FACE_OUTER_BOUND, and {} CCW UV outer-loop candidates",
            face.step_id,
            surface_kind_name(&face.surface),
            face.loops.len(),
            outer_candidates.len()
        ));
    };
    for (idx, other_uv) in &all_uv_boxes {
        if *idx != *outer_idx
            && !other_uv.has_negative_area()
            && !other_uv.is_contained_by(outer_uv, solidcore::math::EFF_ZERO_PARAM)
        {
            return Err(format!(
                "face #{} on {} has {} bounds, no FACE_OUTER_BOUND, and loop {idx} outside candidate outer UV bounds",
                face.step_id,
                surface_kind_name(&face.surface),
                face.loops.len()
            ));
        }
    }
    Ok(*outer_idx)
}

/// Squared 3D tight bounding-box diagonal of `l`, computed through the kernel's
/// [`Curve::calculate_tight_bounding_box`] and [`Extent3d::max_distance_squared`] so
/// single-edge closed conics and splines report their exact 3D extent without sample
/// counts in `step-brep`.
fn loop_bbox_diag_sq(k: &Kernel, body: &Body, l: &step_import::model::Loop) -> Result<f64, String> {
    let mut extent = Extent3d::new();
    match &l.bound {
        Bound::Vertex(v) => {
            if let Some(vtx) = body.vertices.get(*v) {
                extent.add_point(point(vtx.point));
            }
        }
        Bound::Edges(uses) => {
            for u in uses {
                let e = body.edges.get(u.edge).ok_or_else(|| {
                    format!("body #{} references missing edge {}", body.step_id, u.edge)
                })?;
                let (lo, hi) = if e.same_sense {
                    (e.t_start, e.t_end)
                } else {
                    (e.t_end, e.t_start)
                };
                let curve = crate::make_curve(k, &e.curve, Some((lo.min(hi), lo.max(hi))))
                    .map_err(call("edge curve"))?;
                let edge_box = curve
                    .calculate_tight_bounding_box(k, &curve.natural_interval())
                    .map_err(call("CalculateTightBoundingBox"))?;
                if !edge_box.has_negative_volume() {
                    extent = extent.union(&edge_box);
                }
            }
        }
    }
    if extent.has_negative_volume() {
        Ok(0.0)
    } else {
        Ok(extent.max_distance_squared(&extent))
    }
}

/// The calls of one face; returns the face made.
#[allow(clippy::too_many_arguments)]
fn face_calls(
    k: &mut Kernel,
    c: &mut BrepConstructor,
    body: &Body,
    shell: &Shell,
    face: &Face,
    loops: Vec<NormalizedLoop>,
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

    for l in loops {
        let uses = match l {
            NormalizedLoop::Vertex(v) => {
                c.start_and_end_single_vertex_loop(point(vertex_point(v)))
                    .map_err(call("StartAndEndSingleVertexLoop"))?;
                continue;
            }
            NormalizedLoop::Edges(uses) => uses,
        };
        // the loop runs counter-clockwise about the face's own normal
        c.start_loop(orient(face.same_sense))
            .map_err(call("StartLoop"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_body, BuildMode, BuildOptions};
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use step_import::model::{Curve, Edge, Frame, Loop, OrientedEdge, Surface, Vertex};

    fn xy_frame(origin: [f64; 3], flip_z: bool) -> Frame {
        if flip_z {
            Frame {
                origin,
                x: [1.0, 0.0, 0.0],
                y: [0.0, -1.0, 0.0],
                z: [0.0, 0.0, -1.0],
            }
        } else {
            Frame {
                origin,
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                z: [0.0, 0.0, 1.0],
            }
        }
    }

    fn line_edge(step_id: u64, vertices: &[Vertex], start: usize, end: usize) -> Edge {
        let p0 = vertices[start].point;
        let p1 = vertices[end].point;
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let dz = p1[2] - p0[2];
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        Edge {
            step_id,
            start,
            end,
            curve: Curve::Line {
                origin: p0,
                dir: [dx / len, dy / len, dz / len],
                magnitude: 1.0,
            },
            same_sense: true,
            t_start: 0.0,
            t_end: len,
        }
    }

    fn measure_volume(brep: &Brep) -> f64 {
        let k = Kernel::default();
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let mut copy = brep.clone_standing_for_self().unwrap();
        brep_volume(&k, &mut copy, 1e-7, &attrs).unwrap()
    }

    fn assert_level2_valid(brep: &Brep) {
        let mut k = Kernel::default();
        let mut copy = brep.clone_standing_for_self().unwrap();
        let res = check_brep(&mut k, &mut copy, CheckLevel::Level2).unwrap();
        assert!(
            res.pointer_reports.is_empty(),
            "pointer reports: {:?}",
            res.pointer_reports
        );
        assert_eq!(
            res.assert_valid,
            Some(true),
            "assert_valid reports: {:?}",
            res.reports.reports()
        );
        assert!(
            res.reports.reports().is_empty(),
            "reports: {:?}",
            res.reports.reports()
        );
    }

    #[test]
    fn missing_face_outer_bound_orders_closed_circle_outer_loop_first() {
        // Annular cylinder z in [0, 10], R_out = 20, R_in = 8.
        // Both planar caps omit FACE_OUTER_BOUND (`outer: false` on every loop) and list
        // the small inner circle before the closed single-edge outer circle (`start == end`).
        let r_out = 20.0;
        let r_in = 8.0;
        let h = 10.0;
        let vertices = vec![
            Vertex {
                step_id: 1,
                point: [r_in, 0.0, 0.0],
            },
            Vertex {
                step_id: 2,
                point: [r_out, 0.0, 0.0],
            },
            Vertex {
                step_id: 3,
                point: [r_in, 0.0, h],
            },
            Vertex {
                step_id: 4,
                point: [r_out, 0.0, h],
            },
        ];
        let edges = vec![
            Edge {
                step_id: 10,
                start: 0,
                end: 0,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: r_in,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            Edge {
                step_id: 11,
                start: 1,
                end: 1,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: r_out,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            Edge {
                step_id: 12,
                start: 2,
                end: 2,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, h], false),
                    radius: r_in,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            Edge {
                step_id: 13,
                start: 3,
                end: 3,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, h], false),
                    radius: r_out,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            line_edge(14, &vertices, 0, 2),
            line_edge(15, &vertices, 1, 3),
        ];
        let faces = vec![
            // Bottom cap at z = 0 (outward normal -Z): inner loop listed first, outer: false.
            Face {
                step_id: 100,
                surface: Surface::Plane {
                    frame: xy_frame([0.0, 0.0, 0.0], true),
                },
                same_sense: true,
                loops: vec![
                    Loop {
                        outer: false,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 0,
                            forward: true,
                        }]),
                    },
                    Loop {
                        outer: false,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 1,
                            forward: false,
                        }]),
                    },
                ],
            },
            // Top cap at z = h (outward normal +Z): inner loop listed first, outer: false.
            Face {
                step_id: 101,
                surface: Surface::Plane {
                    frame: xy_frame([0.0, 0.0, h], false),
                },
                same_sense: true,
                loops: vec![
                    Loop {
                        outer: false,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 2,
                            forward: false,
                        }]),
                    },
                    Loop {
                        outer: false,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 3,
                            forward: true,
                        }]),
                    },
                ],
            },
            // Outer cylinder wall (outward normal +R) with seam edge 5.
            Face {
                step_id: 102,
                surface: Surface::Cylinder {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: r_out,
                },
                same_sense: true,
                loops: vec![Loop {
                    outer: false,
                    orientation: true,
                    bound: Bound::Edges(vec![
                        OrientedEdge {
                            edge: 1,
                            forward: true,
                        },
                        OrientedEdge {
                            edge: 5,
                            forward: true,
                        },
                        OrientedEdge {
                            edge: 3,
                            forward: false,
                        },
                        OrientedEdge {
                            edge: 5,
                            forward: false,
                        },
                    ]),
                }],
            },
            // Inner bore wall (outward normal -R, same_sense: false) with seam edge 4.
            Face {
                step_id: 103,
                surface: Surface::Cylinder {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: r_in,
                },
                same_sense: false,
                loops: vec![Loop {
                    outer: false,
                    orientation: true,
                    bound: Bound::Edges(vec![
                        OrientedEdge {
                            edge: 0,
                            forward: false,
                        },
                        OrientedEdge {
                            edge: 4,
                            forward: true,
                        },
                        OrientedEdge {
                            edge: 2,
                            forward: true,
                        },
                        OrientedEdge {
                            edge: 4,
                            forward: false,
                        },
                    ]),
                }],
            },
        ];
        let body = Body {
            step_id: 1,
            name: None,
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: vec![Shell {
                step_id: 200,
                closed: true,
                orientation: true,
                faces: vec![0, 1, 2, 3],
            }],
            faces,
            edges,
            vertices,
        };

        let k = Kernel::default();
        let norm = normalized_face_loops(&k, &body, &body.faces[1]).unwrap();
        assert_eq!(
            norm,
            vec![
                NormalizedLoop::Edges(vec![(3, true)]),
                NormalizedLoop::Edges(vec![(2, false)]),
            ]
        );

        let expected_vol = core::f64::consts::PI * (r_out * r_out - r_in * r_in) * h;
        for mode in [BuildMode::Default, BuildMode::StitchOnly] {
            let built = build_body(
                &body,
                BuildOptions {
                    mode,
                    ..BuildOptions::default()
                },
            )
            .unwrap();
            assert!(built.face_failures.is_empty());
            assert_level2_valid(&built.brep);
            let vol = measure_volume(&built.brep);
            assert!(
                (vol - expected_vol).abs() < 1e-2,
                "mode {mode:?}: got {vol}, expected {expected_vol}"
            );
        }
    }

    #[test]
    fn touching_loops_sharing_vertex_splice_without_face_test_39() {
        // Stepped 2-manifold solid:
        // - Base [0, 20] x [0, 20] x [0, 5] with a triangular through-hole
        //   (10, 10) - (6, 16) - (14, 16) for z in [0, 5].
        // - Boss [0, 20] x [0, 10] x [5, 10] on y in [0, 10].
        // On the z = 5 step face [0, 20] x [10, 20], the hole's apex (10, 10, 5) lies on
        // the outer boundary y = 10, so the STEP outer loop and inner hole loop share a
        // vertex while every 3D edge of the solid remains strictly 2-manifold.
        let pts = [
            // 0..4: z = 0 outer square
            [0.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 20.0, 0.0],
            [0.0, 20.0, 0.0],
            // 4..7: z = 0 triangular hole
            [10.0, 10.0, 0.0],
            [6.0, 16.0, 0.0],
            [14.0, 16.0, 0.0],
            // 7..14: z = 5 vertices (note vertex 12 = (10, 10, 5) shared on step face!)
            [0.0, 0.0, 5.0],   // 7
            [20.0, 0.0, 5.0],  // 8
            [20.0, 10.0, 5.0], // 9
            [20.0, 20.0, 5.0], // 10
            [0.0, 20.0, 5.0],  // 11
            [0.0, 10.0, 5.0],  // 12
            [10.0, 10.0, 5.0], // 13 (shared vertex!)
            [6.0, 16.0, 5.0],  // 14
            [14.0, 16.0, 5.0], // 15
            // 16..20: z = 10 boss top
            [0.0, 0.0, 10.0],   // 16
            [20.0, 0.0, 10.0],  // 17
            [20.0, 10.0, 10.0], // 18
            [0.0, 10.0, 10.0],  // 19
        ];
        let vertices: Vec<Vertex> = pts
            .iter()
            .enumerate()
            .map(|(i, &point)| Vertex {
                step_id: (i + 1) as u64,
                point,
            })
            .collect();

        let edge_pairs = [
            // 0..4: z = 0 outer CCW (+Z)
            (0, 1), // 0
            (1, 2), // 1
            (2, 3), // 2
            (3, 0), // 3
            // 4..7: z = 0 hole CCW (+Z): 4->6->5->4
            (4, 6), // 4
            (6, 5), // 5
            (5, 4), // 6
            // 7..13: z = 5 perimeter segments
            (7, 8),   // 7: y=0
            (8, 9),   // 8: x=20, y in [0,10]
            (9, 10),  // 9: x=20, y in [10,20]
            (10, 11), // 10: y=20
            (11, 12), // 11: x=0, y in [10,20]
            (12, 7),  // 12: x=0, y in [0,10]
            // 13..15: z = 5 step split line y=10 via shared vertex 13=(10,10,5)
            (12, 13), // 13: (0,10,5)->(10,10,5)
            (13, 9),  // 14: (10,10,5)->(20,10,5)
            // 15..18: z = 5 hole CW (+Z): 13->14->15->13
            (13, 14), // 15: (10,10,5)->(6,16,5)
            (14, 15), // 16: (6,16,5)->(14,16,5)
            (15, 13), // 17: (14,16,5)->(10,10,5)
            // 18..22: base verticals
            (0, 7),  // 18
            (1, 8),  // 19
            (2, 10), // 20
            (3, 11), // 21
            // 22..25: hole verticals
            (4, 13), // 22
            (5, 14), // 23
            (6, 15), // 24
            // 25..29: boss verticals
            (7, 16),  // 25
            (8, 17),  // 26
            (9, 18),  // 27
            (12, 19), // 28
            // 29..33: z = 10 boss top CCW (+Z)
            (16, 17), // 29
            (17, 18), // 30
            (18, 19), // 31
            (19, 16), // 32
        ];
        let edges: Vec<Edge> = edge_pairs
            .iter()
            .enumerate()
            .map(|(i, &(s, e))| line_edge((100 + i) as u64, &vertices, s, e))
            .collect();

        let oe = |edge: usize, forward: bool| OrientedEdge { edge, forward };
        let lp = |outer: bool, uses: Vec<OrientedEdge>| Loop {
            outer,
            orientation: true,
            bound: Bound::Edges(uses),
        };
        let plane = |origin: [f64; 3], z: [f64; 3], x: [f64; 3]| {
            let y = [
                z[1] * x[2] - z[2] * x[1],
                z[2] * x[0] - z[0] * x[2],
                z[0] * x[1] - z[1] * x[0],
            ];
            Surface::Plane {
                frame: Frame { origin, x, y, z },
            }
        };

        let faces = vec![
            // 0: Bottom z = 0 (normal -Z): outer CW from +Z = 0<-3<-2<-1, hole CCW from +Z = 4->6->5
            Face {
                step_id: 200,
                surface: plane([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
                same_sense: true,
                loops: vec![
                    lp(
                        true,
                        vec![oe(3, false), oe(2, false), oe(1, false), oe(0, false)],
                    ),
                    lp(false, vec![oe(4, true), oe(5, true), oe(6, true)]),
                ],
            },
            // 1: Step face z = 5 (normal +Z): TWO loops sharing vertex 13!
            Face {
                step_id: 201,
                surface: plane([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                same_sense: true,
                loops: vec![
                    lp(
                        true,
                        vec![
                            oe(13, true),
                            oe(14, true),
                            oe(9, true),
                            oe(10, true),
                            oe(11, true),
                        ],
                    ),
                    lp(false, vec![oe(15, true), oe(16, true), oe(17, true)]),
                ],
            },
            // 2: Boss top z = 10 (normal +Z)
            Face {
                step_id: 202,
                surface: plane([0.0, 0.0, 10.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![oe(29, true), oe(30, true), oe(31, true), oe(32, true)],
                )],
            },
            // 3: Front y = 0, z in [0, 10] (normal -Y, x = +X => y_axis = -Z; CCW about -Y: (0,0,0)->(20,0,0)->(20,0,5)->(20,0,10)->(0,0,10)->(0,0,5)->(0,0,0) is +X then +Z => normal (+X)x(+Z) = -Y!)
            Face {
                step_id: 203,
                surface: plane([0.0, 0.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![
                        oe(0, true),
                        oe(19, true),
                        oe(26, true),
                        oe(29, false),
                        oe(25, false),
                        oe(18, false),
                    ],
                )],
            },
            // 4: Back y = 20, z in [0, 5] (normal +Y: (-X)x(+Z) = +Y)
            Face {
                step_id: 204,
                surface: plane([0.0, 20.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![oe(2, true), oe(21, true), oe(10, false), oe(20, false)],
                )],
            },
            // 5: Boss back wall y = 10, z in [5, 10] (normal +Y: (-X)x(+Z) = +Y)
            Face {
                step_id: 205,
                surface: plane([0.0, 10.0, 5.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![
                        oe(14, false),
                        oe(13, false),
                        oe(28, true),
                        oe(31, false),
                        oe(27, false),
                    ],
                )],
            },
            // 6: Right x = 20 (normal +X: (+Y)x(+Z) = +X)
            Face {
                step_id: 206,
                surface: plane([20.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![
                        oe(1, true),
                        oe(20, true),
                        oe(9, false),
                        oe(27, true),
                        oe(30, false),
                        oe(26, false),
                        oe(19, false),
                    ],
                )],
            },
            // 7: Left x = 0 (normal -X: (-Y)x(+Z) = -X)
            Face {
                step_id: 207,
                surface: plane([0.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![
                        oe(3, true),
                        oe(18, true),
                        oe(25, true),
                        oe(32, false),
                        oe(28, false),
                        oe(11, false),
                        oe(21, false),
                    ],
                )],
            },
            // 8: Hole wall 4->13->15->6 (bottom edge 4=(10,10)->6=(14,16), d=(4,6,0); inward to hole is (-6,4,0): (+d)x(+Z) = (6,-4,0) points into solid? Wait: hole interior is around (10,14), edge (10,10)->(14,16) has left side (-6,4) toward (10,14), so outward from solid into hole is (-6,4,0) = (+Z)x(d)! So loop (-d) then (+Z) has (-d)x(+Z) = (-6,4,0)!)
            Face {
                step_id: 208,
                surface: plane(
                    [10.0, 10.0, 0.0],
                    [-6.0 / 52.0_f64.sqrt(), 4.0 / 52.0_f64.sqrt(), 0.0],
                    [-4.0 / 52.0_f64.sqrt(), -6.0 / 52.0_f64.sqrt(), 0.0],
                ),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![oe(4, false), oe(22, true), oe(17, false), oe(24, false)],
                )],
            },
            // 9: Hole wall 6=(14,16)->5=(6,16), d=(-8,0,0), outward from solid into hole is (0,-1,0) = (-d)x(+Z) where -d=(+8,0,0), (+8,0,0)x(0,0,1) = (0,-8,0)!
            Face {
                step_id: 209,
                surface: plane([14.0, 16.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![oe(5, false), oe(24, true), oe(16, false), oe(23, false)],
                )],
            },
            // 10: Hole wall 5=(6,16)->4=(10,10), d=(4,-6,0), hole interior is right of d? Left of (6,16)->(10,10) is (6,4) toward (10,14)! So (-d)x(+Z) = (-4,6,0)x(0,0,1) = (6,4,0) points into hole!
            Face {
                step_id: 210,
                surface: plane(
                    [6.0, 16.0, 0.0],
                    [6.0 / 52.0_f64.sqrt(), 4.0 / 52.0_f64.sqrt(), 0.0],
                    [-4.0 / 52.0_f64.sqrt(), 6.0 / 52.0_f64.sqrt(), 0.0],
                ),
                same_sense: true,
                loops: vec![lp(
                    true,
                    vec![oe(6, false), oe(23, true), oe(15, false), oe(22, false)],
                )],
            },
        ];

        let body = Body {
            step_id: 2,
            name: None,
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: vec![Shell {
                step_id: 300,
                closed: true,
                orientation: true,
                faces: (0..faces.len()).collect(),
            }],
            faces,
            edges,
            vertices,
        };

        // Step face (index 1) had 2 loops sharing vertex 13:
        // outer = [(13, true), (14, true), (9, true), (10, true), (11, true)]
        // hole  = [(15, true), (16, true), (17, true)]
        // Edge 14 starts at vertex 13 (`ei = 1`) and Edge 15 starts at vertex 13 (`ej = 0`),
        // so splicing inserts the CW hole right at vertex 13 between Edge 13 and Edge 14.
        let k = Kernel::default();
        let spliced = normalized_face_loops(&k, &body, &body.faces[1]).unwrap();
        assert_eq!(
            spliced,
            vec![NormalizedLoop::Edges(vec![
                (13, true),
                (15, true),
                (16, true),
                (17, true),
                (14, true),
                (9, true),
                (10, true),
                (11, true),
            ])]
        );

        let expected_vol = 20.0 * 20.0 * 5.0 + 20.0 * 10.0 * 5.0 - 0.5 * 8.0 * 6.0 * 5.0; // 2880.0
        let built = build_body(&body, BuildOptions::default()).unwrap();
        assert!(built.face_failures.is_empty());
        assert_level2_valid(&built.brep);
        let vol = measure_volume(&built.brep);
        assert!(
            (vol - expected_vol).abs() < 1e-6,
            "got {vol}, expected {expected_vol}"
        );
    }

    #[test]
    fn non_planar_multi_bound_face_without_outer_bound_returns_build_error() {
        let vertices = vec![
            Vertex {
                step_id: 1,
                point: [10.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 2,
                point: [15.0, 0.0, 5.0],
            },
            Vertex {
                step_id: 3,
                point: [10.0, 0.0, 5.0],
            },
        ];
        let edges = vec![
            Edge {
                step_id: 10,
                start: 0,
                end: 0,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: 10.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            Edge {
                step_id: 11,
                start: 1,
                end: 1,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, 5.0], false),
                    radius: 15.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            Edge {
                step_id: 12,
                start: 2,
                end: 2,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, 5.0], false),
                    radius: 10.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
        ];
        let k = Kernel::default();

        for (step_id, kind_label, surface, hi_edge) in [
            (
                404,
                "cone",
                Surface::Cone {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: 10.0,
                    semi_angle_deg: 45.0,
                },
                1usize,
            ),
            (
                405,
                "cylinder",
                Surface::Cylinder {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: 10.0,
                },
                2usize,
            ),
        ] {
            let body = Body {
                step_id: 3,
                name: None,
                kind: BodyKind::ManifoldSolid,
                unit_mm: 1.0,
                uncertainty_mm: Some(1e-6),
                shells: vec![Shell {
                    step_id: 400,
                    closed: false,
                    orientation: true,
                    faces: vec![0],
                }],
                faces: vec![Face {
                    step_id,
                    surface,
                    same_sense: true,
                    loops: vec![
                        Loop {
                            outer: false,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 0,
                                forward: true,
                            }]),
                        },
                        Loop {
                            outer: false,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: hi_edge,
                                forward: false,
                            }]),
                        },
                    ],
                }],
                edges: edges.clone(),
                vertices: vertices.clone(),
            };

            let err = normalized_face_loops(&k, &body, &body.faces[0]).unwrap_err();
            assert!(
                err.contains(&format!("face #{step_id}"))
                    && err.contains(kind_label)
                    && err.contains("no FACE_OUTER_BOUND"),
                "unexpected error for {kind_label}: {err}"
            );

            for mode in [BuildMode::Default, BuildMode::Strict, BuildMode::StitchOnly] {
                let Err(build_err) = build_body(
                    &body,
                    BuildOptions {
                        mode,
                        ..BuildOptions::default()
                    },
                ) else {
                    panic!("{kind_label} mode {mode:?} unexpectedly succeeded");
                };
                assert!(
                    build_err.message.contains(&format!("face #{step_id}"))
                        && build_err.message.contains(kind_label)
                        && build_err.message.contains("no FACE_OUTER_BOUND"),
                    "{kind_label} mode {mode:?}: unexpected error: {build_err}"
                );
            }

            if step_id == 404 {
                // Two FACE_OUTER_BOUND loops on one face are also rejected rather than spliced.
                let mut two_outer = body.faces[0].clone();
                two_outer.loops[0].outer = true;
                two_outer.loops[1].outer = true;
                let err_two = normalized_face_loops(&k, &body, &two_outer).unwrap_err();
                assert!(
                    err_two.contains("2 FACE_OUTER_BOUND"),
                    "unexpected error: {err_two}"
                );
            }
        }

        // Planar multi-bound face with equal largest bounding-box diagonals and no FACE_OUTER_BOUND
        // returns an explicit tie error instead of picking the first loop arbitrarily.
        let tied_plane_body = Body {
            step_id: 30,
            name: None,
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: vec![Shell {
                step_id: 410,
                closed: false,
                orientation: true,
                faces: vec![0],
            }],
            faces: vec![Face {
                step_id: 406,
                surface: Surface::Plane {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                },
                same_sense: true,
                loops: vec![
                    Loop {
                        outer: false,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 0,
                            forward: true,
                        }]),
                    },
                    Loop {
                        outer: false,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 1,
                            forward: true,
                        }]),
                    },
                ],
            }],
            edges: vec![
                Edge {
                    step_id: 30,
                    start: 0,
                    end: 0,
                    curve: Curve::Circle {
                        frame: xy_frame([-10.0, 0.0, 0.0], false),
                        radius: 5.0,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 360.0,
                },
                Edge {
                    step_id: 31,
                    start: 1,
                    end: 1,
                    curve: Curve::Circle {
                        frame: xy_frame([10.0, 0.0, 0.0], false),
                        radius: 5.0,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 360.0,
                },
            ],
            vertices: vec![
                Vertex {
                    step_id: 1,
                    point: [-5.0, 0.0, 0.0],
                },
                Vertex {
                    step_id: 2,
                    point: [15.0, 0.0, 0.0],
                },
            ],
        };
        let tie_err =
            normalized_face_loops(&k, &tied_plane_body, &tied_plane_body.faces[0]).unwrap_err();
        assert!(
            tie_err.contains("face #406")
                && tie_err.contains("tied largest bounding-box diagonals"),
            "unexpected tie error: {tie_err}"
        );
        for mode in [BuildMode::Default, BuildMode::Strict, BuildMode::StitchOnly] {
            let Err(build_err) = build_body(
                &tied_plane_body,
                BuildOptions {
                    mode,
                    ..BuildOptions::default()
                },
            ) else {
                panic!("planar tie mode {mode:?} unexpectedly succeeded");
            };
            assert!(
                build_err
                    .message
                    .contains("tied largest bounding-box diagonals"),
                "planar tie mode {mode:?}: unexpected error: {build_err}"
            );
        }
    }

    fn frame_from_normal(origin: [f64; 3], raw_normal: [f64; 3]) -> Frame {
        let len = (raw_normal[0] * raw_normal[0]
            + raw_normal[1] * raw_normal[1]
            + raw_normal[2] * raw_normal[2])
            .sqrt();
        let z = [
            raw_normal[0] / len,
            raw_normal[1] / len,
            raw_normal[2] / len,
        ];
        let seed = if z[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let dot = seed[0] * z[0] + seed[1] * z[1] + seed[2] * z[2];
        let rx = [
            seed[0] - dot * z[0],
            seed[1] - dot * z[1],
            seed[2] - dot * z[2],
        ];
        let rx_len = (rx[0] * rx[0] + rx[1] * rx[1] + rx[2] * rx[2]).sqrt();
        let x = [rx[0] / rx_len, rx[1] / rx_len, rx[2] / rx_len];
        let y = [
            z[1] * x[2] - z[2] * x[1],
            z[2] * x[0] - z[0] * x[2],
            z[0] * x[1] - z[1] * x[0],
        ];
        Frame { origin, x, y, z }
    }

    fn oriented_cycle_loop(
        vertices: &[Vertex],
        edges: &[Edge],
        frame: &Frame,
        outer: bool,
        want_ccw: bool,
        raw_uses: &[(usize, bool)],
    ) -> Loop {
        let mut pts_2d = Vec::new();
        for &(ei, fwd) in raw_uses {
            let vi = if fwd { edges[ei].start } else { edges[ei].end };
            let p = vertices[vi].point;
            let d = [
                p[0] - frame.origin[0],
                p[1] - frame.origin[1],
                p[2] - frame.origin[2],
            ];
            let u = d[0] * frame.x[0] + d[1] * frame.x[1] + d[2] * frame.x[2];
            let v = d[0] * frame.y[0] + d[1] * frame.y[1] + d[2] * frame.y[2];
            pts_2d.push((u, v));
        }
        let mut signed_area2 = 0.0;
        for idx in 0..pts_2d.len() {
            let (u0, v0) = pts_2d[idx];
            let (u1, v1) = pts_2d[(idx + 1) % pts_2d.len()];
            signed_area2 += u0 * v1 - u1 * v0;
        }
        let is_ccw = signed_area2 > 0.0;
        let uses: Vec<OrientedEdge> = if is_ccw == want_ccw {
            raw_uses
                .iter()
                .map(|&(edge, forward)| OrientedEdge { edge, forward })
                .collect()
        } else {
            raw_uses
                .iter()
                .rev()
                .map(|&(edge, forward)| OrientedEdge {
                    edge,
                    forward: !forward,
                })
                .collect()
        };
        Loop {
            outer,
            orientation: true,
            bound: Bound::Edges(uses),
        }
    }

    #[test]
    fn open_bspline_sheet_without_outer_bound_orders_ccw_outer_loop_first() {
        use step_import::model::{BSplineCurve, BSplineSurface};

        // Vaulted block `[-10, 10] x [-10, 10]` from `z = 0` up to open quadratic B-spline
        // sheet `S(u, v) = (20u - 10, 20v - 10, 6 - 0.01 * (20u - 10)^2)` (`!closed_u && !closed_v`),
        // pierced by two rectangular vertical thru-holes `[-6, -2] x [-3, 3]` and `[2, 6] x [-3, 3]`.
        let z_arch = |x: f64| 6.0 - 0.01 * x * x;
        let quad_arch_edge =
            |step_id: u64, vertices: &[Vertex], start: usize, end: usize| -> Edge {
                let p0 = vertices[start].point;
                let p1 = vertices[end].point;
                let xm = 0.5 * (p0[0] + p1[0]);
                let zm = 6.0 - 0.01 * p0[0] * p1[0];
                Edge {
                    step_id,
                    start,
                    end,
                    curve: Curve::BSpline(BSplineCurve {
                        degree: 2,
                        points: vec![p0, [xm, p0[1], zm], p1],
                        weights: None,
                        knots: vec![0.0, 1.0],
                        mults: vec![3, 3],
                    }),
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 1.0,
                }
            };

        let coords_xy = [
            // 0..4: outer rectangle
            (-10.0, -10.0),
            (10.0, -10.0),
            (10.0, 10.0),
            (-10.0, 10.0),
            // 4..8: hole 1 `[-6, -2] x [-3, 3]`
            (-6.0, -3.0),
            (-2.0, -3.0),
            (-2.0, 3.0),
            (-6.0, 3.0),
            // 8..12: hole 2 `[2, 6] x [-3, 3]`
            (2.0, -3.0),
            (6.0, -3.0),
            (6.0, 3.0),
            (2.0, 3.0),
        ];
        let mut vertices = Vec::new();
        // 0..12: bottom `z = 0`; 12..24: top `z = z_arch(x)`
        for (i, &(x, y)) in coords_xy.iter().enumerate() {
            vertices.push(Vertex {
                step_id: (i + 1) as u64,
                point: [x, y, 0.0],
            });
        }
        for (i, &(x, y)) in coords_xy.iter().enumerate() {
            vertices.push(Vertex {
                step_id: (i + 13) as u64,
                point: [x, y, z_arch(x)],
            });
        }

        let mut edges = Vec::new();
        // 0..4: outer bottom; 4..8: hole 1 bottom; 8..12: hole 2 bottom
        for base in [0usize, 4, 8] {
            for k_idx in 0..4 {
                let a = base + k_idx;
                let b = base + (k_idx + 1) % 4;
                edges.push(line_edge(100 + edges.len() as u64, &vertices, a, b));
            }
        }
        // 12..16: outer top (12 & 14 along x -> quadratic B-spline; 13 & 15 along y -> straight line)
        // 16..20: hole 1 top; 20..24: hole 2 top
        for base in [12usize, 16, 20] {
            edges.push(quad_arch_edge(
                100 + edges.len() as u64,
                &vertices,
                base,
                base + 1,
            ));
            edges.push(line_edge(
                100 + edges.len() as u64,
                &vertices,
                base + 1,
                base + 2,
            ));
            edges.push(quad_arch_edge(
                100 + edges.len() as u64,
                &vertices,
                base + 2,
                base + 3,
            ));
            edges.push(line_edge(
                100 + edges.len() as u64,
                &vertices,
                base + 3,
                base,
            ));
        }
        // 24..36: vertical edges `v` -> `v + 12`
        for v_idx in 0..12 {
            edges.push(line_edge(
                100 + edges.len() as u64,
                &vertices,
                v_idx,
                v_idx + 12,
            ));
        }

        let roof_surface = Surface::BSpline(BSplineSurface {
            u_degree: 2,
            v_degree: 1,
            points: vec![
                vec![[-10.0, -10.0, 5.0], [-10.0, 10.0, 5.0]],
                vec![[0.0, -10.0, 7.0], [0.0, 10.0, 7.0]],
                vec![[10.0, -10.0, 5.0], [10.0, 10.0, 5.0]],
            ],
            weights: None,
            u_knots: vec![0.0, 1.0],
            u_mults: vec![3, 3],
            v_knots: vec![0.0, 1.0],
            v_mults: vec![2, 2],
        });
        let top_proj_frame = xy_frame([0.0, 0.0, 6.0], false);
        // List both CW interior holes FIRST and the CCW outer boundary LAST, all `outer: false`!
        let hole1_top = oriented_cycle_loop(
            &vertices,
            &edges,
            &top_proj_frame,
            false,
            false,
            &[(16, true), (17, true), (18, true), (19, true)],
        );
        let hole2_top = oriented_cycle_loop(
            &vertices,
            &edges,
            &top_proj_frame,
            false,
            false,
            &[(20, true), (21, true), (22, true), (23, true)],
        );
        let outer_top = oriented_cycle_loop(
            &vertices,
            &edges,
            &top_proj_frame,
            false,
            true,
            &[(12, true), (13, true), (14, true), (15, true)],
        );

        let bot_frame = xy_frame([0.0, 0.0, 0.0], true);
        let mut faces = vec![
            Face {
                step_id: 700,
                surface: roof_surface,
                same_sense: true,
                loops: vec![hole1_top, hole2_top, outer_top],
            },
            Face {
                step_id: 701,
                surface: Surface::Plane { frame: bot_frame },
                same_sense: true,
                loops: vec![
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &bot_frame,
                        true,
                        true,
                        &[(0, true), (1, true), (2, true), (3, true)],
                    ),
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &bot_frame,
                        false,
                        false,
                        &[(4, true), (5, true), (6, true), (7, true)],
                    ),
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &bot_frame,
                        false,
                        false,
                        &[(8, true), (9, true), (10, true), (11, true)],
                    ),
                ],
            },
        ];

        // 4 outer vertical walls (outward normals) + 8 hole vertical walls (normals into hole void).
        let wall_specs: [([f64; 3], usize, usize); 12] = [
            // Outer walls
            ([0.0, -1.0, 0.0], 0, 0),
            ([1.0, 0.0, 0.0], 0, 1),
            ([0.0, 1.0, 0.0], 0, 2),
            ([-1.0, 0.0, 0.0], 0, 3),
            // Hole 1 walls (outward from solid = inward toward hole center `(-4, 0)`)
            ([0.0, 1.0, 0.0], 4, 0),
            ([-1.0, 0.0, 0.0], 4, 1),
            ([0.0, -1.0, 0.0], 4, 2),
            ([1.0, 0.0, 0.0], 4, 3),
            // Hole 2 walls (inward toward hole center `(4, 0)`)
            ([0.0, 1.0, 0.0], 8, 0),
            ([-1.0, 0.0, 0.0], 8, 1),
            ([0.0, -1.0, 0.0], 8, 2),
            ([1.0, 0.0, 0.0], 8, 3),
        ];
        for (wi, &(normal, base, side)) in wall_specs.iter().enumerate() {
            let a = base + side;
            let b = base + (side + 1) % 4;
            let bot_e = base + side;
            let top_e = 12 + base + side;
            let va_e = 24 + a;
            let vb_e = 24 + b;
            let w_frame = frame_from_normal(vertices[a].point, normal);
            faces.push(Face {
                step_id: 710 + wi as u64,
                surface: Surface::Plane { frame: w_frame },
                same_sense: true,
                loops: vec![oriented_cycle_loop(
                    &vertices,
                    &edges,
                    &w_frame,
                    true,
                    true,
                    &[(bot_e, true), (vb_e, true), (top_e, false), (va_e, false)],
                )],
            });
        }

        let body = Body {
            step_id: 70,
            name: None,
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: vec![Shell {
                step_id: 750,
                closed: true,
                orientation: true,
                faces: (0..faces.len()).collect(),
            }],
            faces,
            edges,
            vertices,
        };

        let mut custom_k = Kernel::default();
        custom_k.tolerances.zone_tol_3d = 2e-5;
        let norm_loops = normalized_face_loops(&custom_k, &body, &body.faces[0]).unwrap();
        assert_eq!(norm_loops.len(), 3);
        assert_eq!(
            norm_loops[0],
            NormalizedLoop::Edges(vec![(12, true), (13, true), (14, true), (15, true)]),
            "CCW outer loop listed last on open B-spline sheet must be placed at index 0"
        );

        // Ambiguous open sheet with two CCW loops and no FACE_OUTER_BOUND is refused.
        let mut two_ccw_face = body.faces[0].clone();
        two_ccw_face.loops[0].orientation = !two_ccw_face.loops[0].orientation;
        let err_ambig = normalized_face_loops(&custom_k, &body, &two_ccw_face).unwrap_err();
        assert!(
            err_ambig.contains("2 CCW UV outer-loop candidates"),
            "unexpected error: {err_ambig}"
        );

        // Exact volume: int_{-10}^{10} 20 * (6 - 0.01 x^2) dx - 2 * int_2^6 6 * (6 - 0.01 x^2) dx
        //             = 6800 / 3 - 279.68 = 1986.9866666666667.
        let expected_vol = 6800.0 / 3.0 - 279.68;
        for mode in [BuildMode::Default, BuildMode::StitchOnly] {
            let built = build_body(
                &body,
                BuildOptions {
                    mode,
                    ..BuildOptions::default()
                },
            )
            .unwrap();
            assert!(built.face_failures.is_empty());
            assert_level2_valid(&built.brep);
            let vol = measure_volume(&built.brep);
            assert!(
                (vol - expected_vol).abs() < 1e-4,
                "mode {mode:?}: got {vol}, expected {expected_vol}"
            );
        }
    }

    #[test]
    fn same_sense_cw_holes_sharing_vertex_splice_into_figure_eight_level2_solid() {
        // Block `[-10, 10] x [-10, 10] x [0, 5]` pierced by two inverted diamond frustum
        // holes `|x + 3| + |y| <= 2 + 0.2 z` and `|x - 3| + |y| <= 2 + 0.2 z`.
        // At `z = 0` the two holes are disjoint (`x = -1` vs `x = +1`); at `z = 5` both
        // CW interior hole loops touch externally at the single shared vertex `V_pinch = (0, 0, 5)`.
        let vertices = vec![
            // 0..4: outer bottom z = 0
            Vertex {
                step_id: 1,
                point: [-10.0, -10.0, 0.0],
            },
            Vertex {
                step_id: 2,
                point: [10.0, -10.0, 0.0],
            },
            Vertex {
                step_id: 3,
                point: [10.0, 10.0, 0.0],
            },
            Vertex {
                step_id: 4,
                point: [-10.0, 10.0, 0.0],
            },
            // 4..8: outer top z = 5
            Vertex {
                step_id: 5,
                point: [-10.0, -10.0, 5.0],
            },
            Vertex {
                step_id: 6,
                point: [10.0, -10.0, 5.0],
            },
            Vertex {
                step_id: 7,
                point: [10.0, 10.0, 5.0],
            },
            Vertex {
                step_id: 8,
                point: [-10.0, 10.0, 5.0],
            },
            // 8..12: hole A bottom z = 0 (`|x + 3| + |y| = 2`)
            Vertex {
                step_id: 9,
                point: [-1.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 10,
                point: [-3.0, 2.0, 0.0],
            },
            Vertex {
                step_id: 11,
                point: [-5.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 12,
                point: [-3.0, -2.0, 0.0],
            },
            // 12..16: hole B bottom z = 0 (`|x - 3| + |y| = 2`)
            Vertex {
                step_id: 13,
                point: [1.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 14,
                point: [3.0, 2.0, 0.0],
            },
            Vertex {
                step_id: 15,
                point: [5.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 16,
                point: [3.0, -2.0, 0.0],
            },
            // 16: shared pinch vertex `(0, 0, 5)` on top face `z = 5`!
            Vertex {
                step_id: 17,
                point: [0.0, 0.0, 5.0],
            },
            // 17..20: remaining 3 top vertices of hole A (`|x + 3| + |y| = 3`)
            Vertex {
                step_id: 18,
                point: [-3.0, 3.0, 5.0],
            },
            Vertex {
                step_id: 19,
                point: [-6.0, 0.0, 5.0],
            },
            Vertex {
                step_id: 20,
                point: [-3.0, -3.0, 5.0],
            },
            // 20..23: remaining 3 top vertices of hole B (`|x - 3| + |y| = 3`)
            Vertex {
                step_id: 21,
                point: [3.0, 3.0, 5.0],
            },
            Vertex {
                step_id: 22,
                point: [6.0, 0.0, 5.0],
            },
            Vertex {
                step_id: 23,
                point: [3.0, -3.0, 5.0],
            },
        ];

        let hole_a_top_verts = [16usize, 17, 18, 19];
        let hole_b_top_verts = [16usize, 20, 21, 22];

        let mut edges = Vec::new();
        // 0..4: outer bottom; 4..8: outer top; 8..12: outer verticals
        for i in 0..4 {
            edges.push(line_edge(
                200 + edges.len() as u64,
                &vertices,
                i,
                (i + 1) % 4,
            ));
        }
        for i in 0..4 {
            edges.push(line_edge(
                200 + edges.len() as u64,
                &vertices,
                4 + i,
                4 + (i + 1) % 4,
            ));
        }
        for i in 0..4 {
            edges.push(line_edge(200 + edges.len() as u64, &vertices, i, 4 + i));
        }
        // 12..16: hole A bottom; 16..20: hole A top (`16 -> 17 -> 18 -> 19 -> 16`, CCW from +Z);
        // 20..24: hole A slanted ribs
        for i in 0..4 {
            edges.push(line_edge(
                200 + edges.len() as u64,
                &vertices,
                8 + i,
                8 + (i + 1) % 4,
            ));
        }
        for (i, &tv) in hole_a_top_verts.iter().enumerate() {
            edges.push(line_edge(
                200 + edges.len() as u64,
                &vertices,
                tv,
                hole_a_top_verts[(i + 1) % 4],
            ));
        }
        for (i, &tv) in hole_a_top_verts.iter().enumerate() {
            edges.push(line_edge(200 + edges.len() as u64, &vertices, 8 + i, tv));
        }
        // 24..28: hole B bottom; 28..32: hole B top (`16 -> 20 -> 21 -> 22 -> 16`, CW from +Z);
        // 32..36: hole B slanted ribs
        for i in 0..4 {
            edges.push(line_edge(
                200 + edges.len() as u64,
                &vertices,
                12 + i,
                12 + (i + 1) % 4,
            ));
        }
        for (i, &tv) in hole_b_top_verts.iter().enumerate() {
            edges.push(line_edge(
                200 + edges.len() as u64,
                &vertices,
                tv,
                hole_b_top_verts[(i + 1) % 4],
            ));
        }
        for (i, &tv) in hole_b_top_verts.iter().enumerate() {
            edges.push(line_edge(200 + edges.len() as u64, &vertices, 12 + i, tv));
        }

        let top_frame = xy_frame([0.0, 0.0, 5.0], false);
        let bot_frame = xy_frame([0.0, 0.0, 0.0], true);
        let mut faces = vec![
            // Face #800: top z = 5 plane with CCW outer loop + TWO same-sense CW hole loops
            // (`slots[1]` and `slots[2]`) touching externally at shared vertex 16 = `(0, 0, 5)`.
            Face {
                step_id: 800,
                surface: Surface::Plane { frame: top_frame },
                same_sense: true,
                loops: vec![
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &top_frame,
                        true,
                        true,
                        &[(4, true), (5, true), (6, true), (7, true)],
                    ),
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &top_frame,
                        false,
                        false,
                        &[(16, true), (17, true), (18, true), (19, true)],
                    ),
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &top_frame,
                        false,
                        false,
                        &[(28, true), (29, true), (30, true), (31, true)],
                    ),
                ],
            },
            Face {
                step_id: 801,
                surface: Surface::Plane { frame: bot_frame },
                same_sense: true,
                loops: vec![
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &bot_frame,
                        true,
                        true,
                        &[(0, true), (1, true), (2, true), (3, true)],
                    ),
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &bot_frame,
                        false,
                        false,
                        &[(12, true), (13, true), (14, true), (15, true)],
                    ),
                    oriented_cycle_loop(
                        &vertices,
                        &edges,
                        &bot_frame,
                        false,
                        false,
                        &[(24, true), (25, true), (26, true), (27, true)],
                    ),
                ],
            },
        ];

        // 4 outer vertical walls
        for (wi, (normal, side)) in [
            ([0.0, -1.0, 0.0], 0usize),
            ([1.0, 0.0, 0.0], 1),
            ([0.0, 1.0, 0.0], 2),
            ([-1.0, 0.0, 0.0], 3),
        ]
        .into_iter()
        .enumerate()
        {
            let next = (side + 1) % 4;
            let w_frame = frame_from_normal(vertices[side].point, normal);
            faces.push(Face {
                step_id: 810 + wi as u64,
                surface: Surface::Plane { frame: w_frame },
                same_sense: true,
                loops: vec![oriented_cycle_loop(
                    &vertices,
                    &edges,
                    &w_frame,
                    true,
                    true,
                    &[
                        (side, true),
                        (8 + next, true),
                        (4 + side, false),
                        (8 + side, false),
                    ],
                )],
            });
        }

        // 4 slanted planar walls of Hole A (`|x + 3| + |y| <= 2 + 0.2 z`; outward from solid
        // points into the hole toward `(-3, 0)` and `+z`: `(-sx, -sy, +0.2)`!)
        let hole_a_normals = [
            [-1.0, -1.0, 0.2],
            [1.0, -1.0, 0.2],
            [1.0, 1.0, 0.2],
            [-1.0, 1.0, 0.2],
        ];
        for (side, normal) in hole_a_normals.into_iter().enumerate() {
            let next = (side + 1) % 4;
            let w_frame = frame_from_normal(vertices[8 + side].point, normal);
            faces.push(Face {
                step_id: 820 + side as u64,
                surface: Surface::Plane { frame: w_frame },
                same_sense: true,
                loops: vec![oriented_cycle_loop(
                    &vertices,
                    &edges,
                    &w_frame,
                    true,
                    true,
                    &[
                        (12 + side, true),
                        (20 + next, true),
                        (16 + side, false),
                        (20 + side, false),
                    ],
                )],
            });
        }

        // 4 slanted planar walls of Hole B (`|x - 3| + |y| <= 2 + 0.2 z`; inward toward `(3, 0)` + `+z`)
        let hole_b_normals = [
            [1.0, -1.0, 0.2],
            [-1.0, -1.0, 0.2],
            [-1.0, 1.0, 0.2],
            [1.0, 1.0, 0.2],
        ];
        for (side, normal) in hole_b_normals.into_iter().enumerate() {
            let next = (side + 1) % 4;
            let w_frame = frame_from_normal(vertices[12 + side].point, normal);
            faces.push(Face {
                step_id: 830 + side as u64,
                surface: Surface::Plane { frame: w_frame },
                same_sense: true,
                loops: vec![oriented_cycle_loop(
                    &vertices,
                    &edges,
                    &w_frame,
                    true,
                    true,
                    &[
                        (24 + side, true),
                        (32 + next, true),
                        (28 + side, false),
                        (32 + side, false),
                    ],
                )],
            });
        }

        let body = Body {
            step_id: 80,
            name: None,
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: vec![Shell {
                step_id: 850,
                closed: true,
                orientation: true,
                faces: (0..faces.len()).collect(),
            }],
            faces,
            edges,
            vertices,
        };

        let k = Kernel::default();
        let top_spliced = normalized_face_loops(&k, &body, &body.faces[0]).unwrap();
        assert_eq!(
            top_spliced.len(),
            2,
            "outer loop stays separate while the two CW holes splice into one CW figure-8 walk"
        );
        let NormalizedLoop::Edges(ref fig8) = top_spliced[1] else {
            panic!("expected figure-8 edge loop");
        };
        assert_eq!(fig8.len(), 8);

        // Exact volume: 20 * 20 * 5 - 2 * (5 / 3) * (8 + sqrt(8 * 18) + 18) = 2000 - 380 / 3 = 5620 / 3.
        let expected_vol = 5620.0 / 3.0;
        for mode in [BuildMode::Default, BuildMode::StitchOnly] {
            let built = build_body(
                &body,
                BuildOptions {
                    mode,
                    ..BuildOptions::default()
                },
            )
            .unwrap();
            assert!(built.face_failures.is_empty());
            assert_level2_valid(&built.brep);
            let vol = measure_volume(&built.brep);
            assert!(
                (vol - expected_vol).abs() < 1e-5,
                "mode {mode:?}: got {vol}, expected {expected_vol}"
            );
        }

        // Documented two-shared-vertices behavior: two loops touching at BOTH vertex 0 and
        // vertex 1 merge once into `slots[0]`, leaving the second shared vertex visited twice
        // inside that single walk rather than splicing across slots a second time.
        let two_touch_verts = vec![
            Vertex {
                step_id: 1,
                point: [0.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 2,
                point: [4.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 3,
                point: [2.0, 3.0, 0.0],
            },
            Vertex {
                step_id: 4,
                point: [2.0, 1.0, 0.0],
            },
        ];
        let two_touch_edges = vec![
            line_edge(1, &two_touch_verts, 0, 1),
            line_edge(2, &two_touch_verts, 1, 2),
            line_edge(3, &two_touch_verts, 2, 0),
            line_edge(4, &two_touch_verts, 0, 3),
            line_edge(5, &two_touch_verts, 3, 1),
            line_edge(6, &two_touch_verts, 1, 0),
        ];
        let two_touch_body = Body {
            step_id: 81,
            name: None,
            kind: BodyKind::ShellModel,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: Vec::new(),
            faces: vec![Face {
                step_id: 860,
                surface: Surface::Plane {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                },
                same_sense: true,
                loops: vec![
                    Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(vec![
                            OrientedEdge {
                                edge: 0,
                                forward: true,
                            },
                            OrientedEdge {
                                edge: 1,
                                forward: true,
                            },
                            OrientedEdge {
                                edge: 2,
                                forward: true,
                            },
                        ]),
                    },
                    Loop {
                        outer: false,
                        orientation: false,
                        bound: Bound::Edges(vec![
                            OrientedEdge {
                                edge: 3,
                                forward: true,
                            },
                            OrientedEdge {
                                edge: 4,
                                forward: true,
                            },
                            OrientedEdge {
                                edge: 5,
                                forward: true,
                            },
                        ]),
                    },
                ],
            }],
            edges: two_touch_edges,
            vertices: two_touch_verts,
        };
        assert_eq!(
            normalized_face_loops(&k, &two_touch_body, &two_touch_body.faces[0]).unwrap(),
            vec![NormalizedLoop::Edges(vec![
                (5, false),
                (4, false),
                (3, false),
                (0, true),
                (1, true),
                (2, true),
            ])]
        );
    }

    #[test]
    fn curved_and_chamfer_faces_with_touching_circular_loops_splice_exact_edge_order() {
        let k = Kernel::default();

        // 1. Standalone 45-degree chamfer cone face carrying closed circular/conic loops
        //    that share seam vertex 0 = (15, 0, 5), plus a third loop chained onto the second
        //    at vertex 1 = (-9, 0, -1), verifying exact edge-use splice order and multi-loop
        //    termination (`m = 3` -> `2` merges -> `1` loop).
        let cone_vertices = vec![
            Vertex {
                step_id: 1,
                point: [15.0, 0.0, 5.0],
            },
            Vertex {
                step_id: 2,
                point: [10.0, 0.0, 0.0],
            },
        ];
        let cone_edges = vec![
            // 0: Outer circle at z = 5, R = 15, closed at seam vertex 0 = (15, 0, 5).
            Edge {
                step_id: 10,
                start: 0,
                end: 0,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, 5.0], false),
                    radius: 15.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            // 1 + 2: Two-arc inner loop touching seam vertex 0 and vertex 1 (orientation: false
            //        flips `[(1, true), (2, true)]` -> `[(2, false), (1, false)]`, whose edge 2
            //        reversed starts at vertex 0 and edge 1 reversed starts at vertex 1).
            line_edge(11, &cone_vertices, 0, 1),
            line_edge(12, &cone_vertices, 1, 0),
            // 3: Third closed loop at vertex 1, chained onto the already-spliced walk.
            Edge {
                step_id: 13,
                start: 1,
                end: 1,
                curve: Curve::Circle {
                    frame: xy_frame([0.0, 0.0, 0.0], false),
                    radius: 10.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
        ];
        let cone_body = Body {
            step_id: 4,
            name: None,
            kind: BodyKind::ShellModel,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: Vec::new(),
            faces: vec![
                // Two-loop chamfer cone: outer circle `[(0, true)]` + inner loop (`orientation: false`
                // -> `[(2, false), (1, false)]`) sharing seam vertex 0.
                Face {
                    step_id: 501,
                    surface: Surface::Cone {
                        frame: xy_frame([0.0, 0.0, -10.0], false),
                        radius: 0.0,
                        semi_angle_deg: 45.0,
                    },
                    same_sense: true,
                    loops: vec![
                        Loop {
                            outer: true,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 0,
                                forward: true,
                            }]),
                        },
                        Loop {
                            outer: false,
                            orientation: false,
                            bound: Bound::Edges(vec![
                                OrientedEdge {
                                    edge: 1,
                                    forward: true,
                                },
                                OrientedEdge {
                                    edge: 2,
                                    forward: true,
                                },
                            ]),
                        },
                    ],
                },
                // Three-loop chain on the same cone: loop 2 (`[(3, false)]` at vertex 1) touches
                // loop 1 after loop 1 has already been spliced into loop 0 at seam vertex 0.
                Face {
                    step_id: 502,
                    surface: Surface::Cone {
                        frame: xy_frame([0.0, 0.0, -10.0], false),
                        radius: 0.0,
                        semi_angle_deg: 45.0,
                    },
                    same_sense: true,
                    loops: vec![
                        Loop {
                            outer: true,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 0,
                                forward: true,
                            }]),
                        },
                        Loop {
                            outer: false,
                            orientation: false,
                            bound: Bound::Edges(vec![
                                OrientedEdge {
                                    edge: 1,
                                    forward: true,
                                },
                                OrientedEdge {
                                    edge: 2,
                                    forward: true,
                                },
                            ]),
                        },
                        Loop {
                            outer: false,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 3,
                                forward: false,
                            }]),
                        },
                    ],
                },
            ],
            edges: cone_edges,
            vertices: cone_vertices,
        };

        assert_eq!(
            normalized_face_loops(&k, &cone_body, &cone_body.faces[0]).unwrap(),
            vec![NormalizedLoop::Edges(vec![
                (2, false),
                (1, false),
                (0, true),
            ])]
        );
        assert_eq!(
            normalized_face_loops(&k, &cone_body, &cone_body.faces[1]).unwrap(),
            vec![NormalizedLoop::Edges(vec![
                (2, false),
                (3, false),
                (1, false),
                (0, true),
            ])]
        );

        // 2. Closed 3-face 2-manifold curved solid where two exact `Curve::Circle` loops on
        //    `Surface::Sphere` (R = 10) share a single seam/tangency vertex V0 = (10, 0, 0):
        //    equator circle z = 0 (`R = 10`, FACE_OUTER_BOUND) and tilted cap circle
        //    3x + 4z = 30 (`center = (3.6, 0, 4.8)`, `r = 8`, FACE_BOUND), closed by two
        //    planar disks. Exact volume = (2/3) pi 10^3 - (pi 4^2 / 3)(30 - 4) = 528 pi.
        let tilted_frame = Frame {
            origin: [3.6, 0.0, 4.8],
            x: [0.8, 0.0, -0.6],
            y: [0.0, 1.0, 0.0],
            z: [0.6, 0.0, 0.8],
        };
        let sphere_body = Body {
            step_id: 5,
            name: None,
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: vec![Shell {
                step_id: 600,
                closed: true,
                orientation: true,
                faces: vec![0, 1, 2],
            }],
            faces: vec![
                // Sphere face (outward +R): equator circle CCW `(0, true)` + tilted circle CW `(1, false)`.
                Face {
                    step_id: 601,
                    surface: Surface::Sphere {
                        frame: xy_frame([0.0, 0.0, 0.0], false),
                        radius: 10.0,
                    },
                    same_sense: true,
                    loops: vec![
                        Loop {
                            outer: true,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 0,
                                forward: true,
                            }]),
                        },
                        Loop {
                            outer: false,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 1,
                                forward: false,
                            }]),
                        },
                    ],
                },
                // Bottom disk z = 0 (outward -Z): `(0, false)`.
                Face {
                    step_id: 602,
                    surface: Surface::Plane {
                        frame: xy_frame([0.0, 0.0, 0.0], true),
                    },
                    same_sense: true,
                    loops: vec![Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 0,
                            forward: false,
                        }]),
                    }],
                },
                // Tilted disk 3x + 4z = 30 (outward +n = (0.6, 0, 0.8)): `(1, true)`.
                Face {
                    step_id: 603,
                    surface: Surface::Plane {
                        frame: tilted_frame,
                    },
                    same_sense: true,
                    loops: vec![Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 1,
                            forward: true,
                        }]),
                    }],
                },
            ],
            edges: vec![
                Edge {
                    step_id: 20,
                    start: 0,
                    end: 0,
                    curve: Curve::Circle {
                        frame: xy_frame([0.0, 0.0, 0.0], false),
                        radius: 10.0,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 360.0,
                },
                Edge {
                    step_id: 21,
                    start: 0,
                    end: 0,
                    curve: Curve::Circle {
                        frame: tilted_frame,
                        radius: 8.0,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 360.0,
                },
            ],
            vertices: vec![Vertex {
                step_id: 1,
                point: [10.0, 0.0, 0.0],
            }],
        };

        assert_eq!(
            normalized_face_loops(&k, &sphere_body, &sphere_body.faces[0]).unwrap(),
            vec![NormalizedLoop::Edges(vec![(1, false), (0, true)])]
        );

        let expected_sphere_vol = 528.0 * core::f64::consts::PI;
        for mode in [BuildMode::Default, BuildMode::StitchOnly] {
            let built = build_body(
                &sphere_body,
                BuildOptions {
                    mode,
                    ..BuildOptions::default()
                },
            )
            .unwrap();
            assert!(built.face_failures.is_empty());
            assert_level2_valid(&built.brep);
            let vol = measure_volume(&built.brep);
            assert!(
                (vol - expected_sphere_vol).abs() < 1e-2,
                "mode {mode:?}: got {vol}, expected {expected_sphere_vol}"
            );
        }

        // 3. Closed 3-face 2-manifold 45-degree chamfer-cone solid (`x^2 + y^2 <= z^2`,
        //    `0.5 x + 5 <= z <= 10`) where `Surface::Cone` carries two closed conic loops
        //    that share seam/tangency vertex `V0 = (10, 0, 10)`: horizontal circle `z = 10`
        //    (`R = 10`, `FACE_OUTER_BOUND`) and tilted ellipse `-x + 2z = 10`
        //    (`semi1 = 10 sqrt(5) / 3`, `semi2 = 10 / sqrt(3)`, `FACE_BOUND`), closed by two
        //    planar caps. Exact volume = (1000 pi / 9) * (3 - 1 / sqrt(3)).
        let s5 = 5.0_f64.sqrt();
        let s3 = 3.0_f64.sqrt();
        let cone_tilt_frame = Frame {
            origin: [10.0 / 3.0, 0.0, 20.0 / 3.0],
            x: [2.0 / s5, 0.0, 1.0 / s5],
            y: [0.0, -1.0, 0.0],
            z: [1.0 / s5, 0.0, -2.0 / s5],
        };
        let closed_cone_body = Body {
            step_id: 6,
            name: None,
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: Some(1e-6),
            shells: vec![Shell {
                step_id: 610,
                closed: true,
                orientation: true,
                faces: vec![0, 1, 2],
            }],
            faces: vec![
                Face {
                    step_id: 611,
                    surface: Surface::Cone {
                        frame: xy_frame([0.0, 0.0, 2.0], false),
                        radius: 2.0,
                        semi_angle_deg: 45.0,
                    },
                    same_sense: true,
                    loops: vec![
                        Loop {
                            outer: true,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 0,
                                forward: false,
                            }]),
                        },
                        Loop {
                            outer: false,
                            orientation: true,
                            bound: Bound::Edges(vec![OrientedEdge {
                                edge: 1,
                                forward: false,
                            }]),
                        },
                    ],
                },
                // Top disk z = 10 (outward +Z): `(0, true)`.
                Face {
                    step_id: 612,
                    surface: Surface::Plane {
                        frame: xy_frame([0.0, 0.0, 10.0], false),
                    },
                    same_sense: true,
                    loops: vec![Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 0,
                            forward: true,
                        }]),
                    }],
                },
                // Bottom tilted ellipse cap (outward `(1, 0, -2) / sqrt(5)`): `(1, true)`.
                Face {
                    step_id: 613,
                    surface: Surface::Plane {
                        frame: cone_tilt_frame,
                    },
                    same_sense: true,
                    loops: vec![Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 1,
                            forward: true,
                        }]),
                    }],
                },
            ],
            edges: vec![
                Edge {
                    step_id: 40,
                    start: 0,
                    end: 0,
                    curve: Curve::Circle {
                        frame: xy_frame([0.0, 0.0, 10.0], false),
                        radius: 10.0,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 360.0,
                },
                Edge {
                    step_id: 41,
                    start: 0,
                    end: 0,
                    curve: Curve::Ellipse {
                        frame: cone_tilt_frame,
                        semi1: 10.0 * s5 / 3.0,
                        semi2: 10.0 / s3,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 360.0,
                },
            ],
            vertices: vec![Vertex {
                step_id: 1,
                point: [10.0, 0.0, 10.0],
            }],
        };

        assert_eq!(
            normalized_face_loops(&k, &closed_cone_body, &closed_cone_body.faces[0]).unwrap(),
            vec![NormalizedLoop::Edges(vec![(1, false), (0, false)])]
        );

        let expected_cone_vol = (1000.0 * core::f64::consts::PI / 9.0) * (3.0 - 1.0 / s3);
        for mode in [BuildMode::Default, BuildMode::StitchOnly] {
            let built = build_body(
                &closed_cone_body,
                BuildOptions {
                    mode,
                    ..BuildOptions::default()
                },
            )
            .unwrap();
            assert!(built.face_failures.is_empty());
            assert_level2_valid(&built.brep);
            let vol = measure_volume(&built.brep);
            assert!(
                (vol - expected_cone_vol).abs() < 1e-2,
                "chamfer cone mode {mode:?}: got {vol}, expected {expected_cone_vol}"
            );
        }
    }
}
