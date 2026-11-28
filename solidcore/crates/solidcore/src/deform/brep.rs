//! Topology-preserving B-rep deformation driven by a [`SpaceMap`].
//!
//! For each affected face, edge, and vertex:
//! 1. Exact rigid/identity maps (including `scale == 0.0`, identical base/control surfaces,
//!    or isometric planar-to-planar maps without boundary clamping) preserve exact analytic
//!    geometry via `brep_copy` / `transform_matrix` without refitting.
//! 2. Otherwise, affected vertices are mapped through `map.map_point`, affected face surfaces
//!    are adaptively refitted on their active parameter domains and certified cell-by-cell
//!    with rounded interval Mean-Value Form enclosures, and affected edge curves are refitted
//!    and certified span-by-span.
//! 3. Partial face selections certify via rounded interval enclosures that every shared
//!    boundary edge stays fixed within tolerance across its entire curve.
//! 4. Edits are applied inside `apply_deformed_geometry` with `adjust_tolerances = false`,
//!    every rebuilt UV trim curve gap and vertex gap is enclosed and verified against the
//!    certified construction bound, tolerances grow only to proved bounds, and every closed
//!    manifold solid is verified to keep positive volume.

use std::collections::{BTreeMap, BTreeSet};

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::curve::curve::p2::{Curve, CurveKind};
use crate::deform::surface_map::{surface_jet_ivl, PreparedSurface};
use crate::deform::{Box3, SpaceMap};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::mass::enclosure::VolumeSign;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::crv_fit::p1::interp_knot_vector;
use crate::nurbs::crv_fit::p2::{calc_matrix, cpts_with_matrix, MatrixEnds};
use crate::nurbs::knots::KnotVector;
use crate::nurbs::srf_fit::interp_given_knots;
use crate::surface::surface::p2::{Surface, SurfaceKind};
use crate::topology::brep::mirror::{apply_deformed_geometry, transform_matrix};
use crate::topology::brep::p1::{
    edges as brep_edges, faces as brep_faces, vertices as brep_vertices,
};
use crate::topology::brep::p2::is_manifold_solid;
use crate::topology::brep::p3::{brep_copy, brep_copy_mapped};
use crate::topology::brep::validation;
use crate::topology::edge as topo_edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p1::calculate_uv_domain_from_uv_trim_curves;
use crate::topology::vertex as topo_vertex;
use crate::topology::{Brep, EdgeId, FaceId, VertexId};

const OP: &str = "deform::brep::deform_brep";

/// Maximum sample count per direction when adaptively refitting deformed surfaces/curves.
const MAX_SAMPLES: usize = 65;

/// Maximum sub-cells inspected while certifying one surface fit.
const MAX_CERT_SURFACE_CELLS: usize = 65_536;

/// Maximum sub-intervals inspected while certifying one curve fit, boundary edge, or trim gap.
const MAX_CERT_CURVE_SPANS: usize = 4_096;

/// Maximum subdivision depth during interval certification.
const MAX_CERT_DEPTH: usize = 10;

const IZ: Ivl = Ivl::new(0.0, 0.0);

/// Options for [`deform_brep`].
#[derive(Debug, Clone)]
pub struct DeformOptions {
    /// Refitting tolerance in millimetres (default `1e-2`).
    pub tolerance: f64,
    /// Optional subset of face ids to deform. `None` deforms the entire body.
    pub faces: Option<Vec<FaceId>>,
}

impl Default for DeformOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-2,
            faces: None,
        }
    }
}

/// Deform `brep` through a surface-to-surface map defined by `(base_brep, base_face)` and
/// `(control_brep, control_face)`.
///
/// # Errors
///
/// Returns [`SolidError`] if either reference face is missing or has no surface/domain, or if
/// [`crate::deform::surface_map::SurfaceToSurfaceMap::new`] or [`deform_brep`] fails.
#[allow(clippy::too_many_arguments)]
pub fn deform_brep_by_surface(
    k: &mut Kernel,
    brep: &Brep,
    base_brep: &mut Brep,
    base_face: FaceId,
    control_brep: &mut Brep,
    control_face: FaceId,
    mut surface_options: crate::deform::surface_map::SurfaceToSurfaceOptions,
    deform_options: &DeformOptions,
) -> Result<Brep> {
    if !deform_options.tolerance.is_finite() || deform_options.tolerance <= 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "deformation tolerance must be positive and finite",
        ));
    }

    let base_dom = face_uv_domain(k, base_brep, base_face)?;
    let base_srf = base_brep
        .face(base_face)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "base face surface"))?
        .copy(k)?;
    let ctrl_dom = face_uv_domain(k, control_brep, control_face)?;
    let ctrl_srf = control_brep
        .face(control_face)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "control face surface"))?
        .copy(k)?;

    surface_options.base_forward = face_forward(base_brep, base_face)?;
    surface_options.control_forward = face_forward(control_brep, control_face)?;

    let map = crate::deform::surface_map::SurfaceToSurfaceMap::new(
        &base_srf,
        base_dom,
        &ctrl_srf,
        ctrl_dom,
        surface_options,
    )?;

    // Validate face list and selection on `brep` before any fast path.
    let src_faces = brep_faces(brep, None)?;
    if src_faces.is_empty() {
        return Err(SolidError::invalid_input(OP, "target body has no faces"));
    }
    let src_face_set: BTreeSet<FaceId> = src_faces.iter().copied().collect();
    let is_partial = match &deform_options.faces {
        None => false,
        Some(list) if list.is_empty() => false,
        Some(list) => {
            let mut chosen = BTreeSet::new();
            for &f in list {
                if !src_face_set.contains(&f) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "selected face does not belong to target body",
                    ));
                }
                chosen.insert(f);
            }
            chosen.len() < src_face_set.len()
        }
    };

    // Zero scale is exact identity: return a topology-preserving copy without refitting.
    if map.scale() == 0.0 {
        let mut out = brep_copy(k, brep)?;
        out.refresh_cached_topology();
        return Ok(out);
    }

    // Exact rigid/identity preservation for whole-body (or identity partial) maps so
    // analytic surfaces and curves (cylinders, circles, spheres, planes) stay exact.
    if let Some(body_box) = body_enclosing_box(k, brep)? {
        if let Some(mat) = map.exact_rigid_matrix(k, body_box)? {
            let is_identity = is_identity_matrix(&mat);
            if !is_partial || is_identity {
                let mut out = brep_copy(k, brep)?;
                if !is_identity {
                    transform_matrix(k, &mut out, &mat, true)?;
                }
                out.refresh_cached_topology();
                return Ok(out);
            }
        }
    }

    deform_brep(k, brep, &map, deform_options)
}

fn face_forward(b: &Brep, f: FaceId) -> Result<bool> {
    let face = b.face(f)?;
    if let Some(fu) = face.faceuse {
        let fu_rec = b.faceuse(fu)?;
        return Ok(fu_rec.orientation == OrientType::Same);
    }
    Ok(true)
}

/// Deform `brep` through `map`, returning a new deformed [`Brep`].
///
/// # Errors
///
/// Returns [`SolidError::InvalidInput`] if `tolerance` is invalid, a requested face id is not
/// in `brep`, a partial face selection would tear shared boundary edges away from unselected
/// adjacent faces, a fit cannot be certified within `tolerance` inside the sample budget, a
/// rebuilt trim curve gap exceeds the certified bound, or a closed solid collapses/inverts to
/// non-positive volume.
pub fn deform_brep(
    k: &mut Kernel,
    brep: &Brep,
    map: &dyn SpaceMap,
    options: &DeformOptions,
) -> Result<Brep> {
    if !options.tolerance.is_finite() || options.tolerance <= 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "deformation tolerance must be positive and finite",
        ));
    }

    let (mut out, copy_map) = brep_copy_mapped(k, brep)?;
    out.refresh_cached_topology();

    let all_faces = brep_faces(&out, None)?;
    if all_faces.is_empty() {
        return Err(SolidError::invalid_input(OP, "target body has no faces"));
    }

    let all_face_set: BTreeSet<FaceId> = all_faces.iter().copied().collect();
    let selected_faces: BTreeSet<FaceId> = match &options.faces {
        None => all_face_set.clone(),
        Some(list) if list.is_empty() => all_face_set.clone(),
        Some(list) => {
            let mut set = BTreeSet::new();
            for &src_f in list {
                let Some(EntityRef::Face(mapped_f)) =
                    copy_map.get_at(Some(EntityRef::Face(src_f)))?
                else {
                    return Err(SolidError::invalid_input(
                        OP,
                        "selected face does not belong to target body",
                    ));
                };
                if !all_face_set.contains(&mapped_f) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "selected face does not belong to target body",
                    ));
                }
                set.insert(mapped_f);
            }
            set
        }
    };
    let is_partial = selected_faces.len() < all_face_set.len();

    // Collect affected edges and vertices.
    let mut selected_edges: BTreeSet<EdgeId> = BTreeSet::new();
    let mut unselected_edges: BTreeSet<EdgeId> = BTreeSet::new();
    for &f in &all_faces {
        let edges = face_edges(&out, f)?;
        if selected_faces.contains(&f) {
            selected_edges.extend(edges);
        } else {
            unselected_edges.extend(edges);
        }
    }

    // If deforming only a subset of faces, prove via interval enclosure that every shared
    // boundary edge stays fixed within tolerance across its entire curve.
    if is_partial {
        for &e in selected_edges.intersection(&unselected_edges) {
            check_boundary_edge_fixed(k, &out, e, map, options.tolerance)?;
        }
    }

    let affected_edges: BTreeSet<EdgeId> = if is_partial {
        selected_edges
            .difference(&unselected_edges)
            .copied()
            .collect()
    } else {
        brep_edges(&out, None)?.into_iter().collect()
    };

    let affected_vertices: BTreeSet<VertexId> = if is_partial {
        let mut pinned = BTreeSet::new();
        for &e in &unselected_edges {
            if let Some(v) = topo_edge::start_vertex(&out, e)? {
                pinned.insert(v);
            }
            if let Some(v) = topo_edge::end_vertex(&out, e)? {
                pinned.insert(v);
            }
        }
        let mut verts = BTreeSet::new();
        for &e in &affected_edges {
            if let Some(v) = topo_edge::start_vertex(&out, e)? {
                if !pinned.contains(&v) {
                    verts.insert(v);
                }
            }
            if let Some(v) = topo_edge::end_vertex(&out, e)? {
                if !pinned.contains(&v) {
                    verts.insert(v);
                }
            }
        }
        verts
    } else {
        brep_vertices(&out, None)?.into_iter().collect()
    };

    // Fit each surface and edge to a quarter of `options.tolerance` so their combined
    // certified error `eps_f + eps_e` stays strictly below `options.tolerance`.
    let fit_tol = 0.25 * options.tolerance;

    // 1. Precompute mapped vertices.
    let mut mapped_vertices: BTreeMap<VertexId, Point3> = BTreeMap::new();
    for &v in &affected_vertices {
        let pt = out.vertex(v)?.point;
        mapped_vertices.insert(v, map.map_point(k, pt)?);
    }

    // 2. Precompute and certify refitted face surfaces.
    let mut mapped_surfaces: BTreeMap<FaceId, Surface> = BTreeMap::new();
    let mut surface_bounds: BTreeMap<FaceId, f64> = BTreeMap::new();
    for &f in &selected_faces {
        let domain = face_uv_domain(k, &mut out, f)?;
        let surface = out
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::invalid_input(OP, "face surface"))?;
        let (refitted, bound) = refit_surface(k, surface, &domain, map, fit_tol)?;
        mapped_surfaces.insert(f, refitted);
        surface_bounds.insert(f, bound);
    }

    // 3. Precompute and certify refitted edge curves.
    let mut mapped_curves: BTreeMap<EdgeId, Curve> = BTreeMap::new();
    let mut edge_bounds: BTreeMap<EdgeId, f64> = BTreeMap::new();
    for &e in &affected_edges {
        let Some(curve) = topo_edge::curve(&out, e)? else {
            continue;
        };
        let interval = topo_edge::interval(&out, e)?;
        let start_pt = topo_edge::start_vertex(&out, e)?.and_then(|v| {
            mapped_vertices
                .get(&v)
                .copied()
                .or_else(|| out.vertex(v).ok().map(|vx| vx.point))
        });
        let end_pt = topo_edge::end_vertex(&out, e)?.and_then(|v| {
            mapped_vertices
                .get(&v)
                .copied()
                .or_else(|| out.vertex(v).ok().map(|vx| vx.point))
        });
        let (refitted, bound) = refit_curve(k, curve, interval, start_pt, end_pt, map, fit_tol)?;
        mapped_curves.insert(e, refitted);
        edge_bounds.insert(e, bound);
    }

    // Classify closed-solid input by topology (`is_manifold_solid`), never by whether the
    // input volume sign was decided `Positive`.
    let was_closed_solid = is_closed_solid(&out)?;

    // 4. Apply all geometry replacements inside `plain_edit` with `adjust_tolerances = false`.
    apply_deformed_geometry(
        k,
        &mut out,
        &mapped_vertices,
        mapped_curves,
        mapped_surfaces,
        &selected_faces,
    )?;

    // 5. Certify every rebuilt trim curve and vertex gap against the construction bound and
    //    raise local tolerances strictly to the proved, enclosed gaps.
    certify_and_update_tolerances(
        k,
        &mut out,
        &selected_faces,
        &surface_bounds,
        &edge_bounds,
        options.tolerance,
    )?;

    out.refresh_cached_topology();

    if was_closed_solid {
        match validation::volume_sign(k, &mut out)? {
            VolumeSign::Positive(_) => {}
            VolumeSign::Negative(_) | VolumeSign::Undecided(_) => {
                return Err(SolidError::invalid_input(
                    OP,
                    "deformed solid collapsed or inverted",
                ));
            }
        }
    }

    Ok(out)
}

fn is_closed_solid(b: &Brep) -> Result<bool> {
    is_manifold_solid(b)
}

fn is_identity_matrix(m: &[[f64; 4]; 4]) -> bool {
    let id = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    m.iter()
        .flatten()
        .zip(id.iter().flatten())
        .all(|(a, b)| (a - b).abs() <= 1e-14)
}

/// Enclose all faces and vertices of `brep` in an outward-rounded 3D box.
fn body_enclosing_box(k: &Kernel, brep: &Brep) -> Result<Option<Box3>> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for v in brep_vertices(brep, None)? {
        let p = brep.vertex(v)?.point;
        if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
            return Ok(None);
        }
        lo[0] = lo[0].min(p.x);
        lo[1] = lo[1].min(p.y);
        lo[2] = lo[2].min(p.z);
        hi[0] = hi[0].max(p.x);
        hi[1] = hi[1].max(p.y);
        hi[2] = hi[2].max(p.z);
        any = true;
    }
    for f in brep_faces(brep, None)? {
        let face = brep.face(f)?;
        let Some(surf) = face.surface.as_ref() else {
            continue;
        };
        let dom = face.uv_domain;
        let Some(jet) = surface_jet_ivl(
            k,
            surf,
            Ivl::new(dom.u_min(), dom.u_max()),
            Ivl::new(dom.v_min(), dom.v_max()),
            0,
        )?
        else {
            return Ok(None);
        };
        lo[0] = lo[0].min(jet.p.x.lo);
        lo[1] = lo[1].min(jet.p.y.lo);
        lo[2] = lo[2].min(jet.p.z.lo);
        hi[0] = hi[0].max(jet.p.x.hi);
        hi[1] = hi[1].max(jet.p.y.hi);
        hi[2] = hi[2].max(jet.p.z.hi);
        any = true;
    }
    if !any || !lo.iter().chain(&hi).all(|x| x.is_finite()) {
        return Ok(None);
    }
    Ok(Some([(lo[0], hi[0]), (lo[1], hi[1]), (lo[2], hi[2])]))
}

fn face_edges(b: &Brep, f: FaceId) -> Result<Vec<EdgeId>> {
    let mut out = Vec::new();
    let loops = crate::topology::face::p2::loops(b, f, false, None)?;
    for l in loops {
        let Some(lu) = b.loop_(l)?.loopuse else {
            continue;
        };
        let eus = crate::topology::loopuse::edgeuses(b, lu, None, None)?;
        for eu in eus {
            if let Some(e) = b.edgeuse(eu)?.edge {
                out.push(e);
            }
        }
    }
    Ok(out)
}

/// Prove via rounded interval enclosures that `|Φ(C(t)) - C(t)| <= tol` for every `t` on
/// shared boundary edge `e`. Refuses if the map cannot enclose the edge or if the bound
/// exceeds `tol`.
fn check_boundary_edge_fixed(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    map: &dyn SpaceMap,
    tol: f64,
) -> Result<()> {
    let Some(curve) = topo_edge::curve(b, e)? else {
        return Ok(());
    };
    let iv = topo_edge::interval(b, e)?;
    let (t0, t1) = (iv.min(), iv.max());
    if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
        return Err(SolidError::invalid_input(
            OP,
            "shared boundary edge has a degenerate interval",
        ));
    }

    let mut work: Vec<(f64, f64, usize)> = vec![(t0, t1, 0)];
    let mut inspected: usize = 0;
    while let Some((ta, tb, depth)) = work.pop() {
        inspected += 1;
        if inspected > MAX_CERT_CURVE_SPANS {
            return Err(SolidError::invalid_input(
                OP,
                "selected faces cannot deform independently: could not certify shared boundary edge stays fixed within tolerance",
            ));
        }

        let tm = 0.5 * (ta + tb);
        let Some((pm, _)) = curve_jet_ivl(curve, Ivl::from(tm), 0)? else {
            return Err(SolidError::invalid_input(
                OP,
                "selected faces cannot deform independently: shared boundary edge cannot be enclosed",
            ));
        };
        let pm_box = [(pm.x.lo, pm.x.hi), (pm.y.lo, pm.y.hi), (pm.z.lo, pm.z.hi)];
        let Some(q_mid_box) = map.enclose_box(k, pm_box)? else {
            return Err(SolidError::invalid_input(
                OP,
                "selected faces cannot deform independently: deformation map cannot enclose shared boundary edge",
            ));
        };
        let qm = box_to_v3(q_mid_box);
        let disp_mid = v3_sub(qm, pm);
        // Fast lower bound: if even the midpoint displacement is proved above `tol`, reject immediately.
        let mid_lo = v3_norm_lower(disp_mid);
        if mid_lo > tol {
            return Err(SolidError::invalid_input(
                OP,
                "selected faces cannot deform independently: shared boundary edges move off unselected adjacent faces",
            ));
        }

        let Some((p_span, dp_span)) = curve_jet_ivl(curve, Ivl::new(ta, tb), 1)? else {
            return Err(SolidError::invalid_input(
                OP,
                "selected faces cannot deform independently: shared boundary edge cannot be enclosed",
            ));
        };
        let p_box = [
            (p_span.x.lo, p_span.x.hi),
            (p_span.y.lo, p_span.y.hi),
            (p_span.z.lo, p_span.z.hi),
        ];
        let bound = if let Some((jac, slack)) = map.enclose_jet(k, p_box)? {
            // G(t) = Φ(C(t)) - C(t) ∈ G(tm) + (J - I) · C'(I) · (I - tm) + slack
            let dt = Ivl::new(ta, tb) - Ivl::from(tm);
            let j_minus_i = jac_sub_identity(jac);
            let slope = mat_mul_v3(j_minus_i, dp_span);
            let total = v3_add(v3_add(disp_mid, v3_scale(slope, dt)), box_to_v3(slack));
            v3_norm_upper(total)
        } else {
            f64::INFINITY
        };

        if bound <= tol {
            continue;
        }
        if depth >= MAX_CERT_DEPTH {
            return Err(SolidError::invalid_input(
                OP,
                "selected faces cannot deform independently: shared boundary edges move off unselected adjacent faces",
            ));
        }
        work.push((ta, tm, depth + 1));
        work.push((tm, tb, depth + 1));
    }
    Ok(())
}

fn face_uv_domain(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<Extent2d> {
    let domain = calculate_uv_domain_from_uv_trim_curves(k, b, f, true, false, false)?.domain;
    let du = domain.u_max() - domain.u_min();
    let dv = domain.v_max() - domain.v_min();
    if du.is_finite() && dv.is_finite() && du > 1e-12 && dv > 1e-12 {
        return Ok(domain);
    }
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "face surface"))?;
    surface.natural_uv_domain_with_nurb(k)
}

fn refit_surface(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    map: &dyn SpaceMap,
    tol: f64,
) -> Result<(Surface, f64)> {
    let u0 = domain.u_min();
    let u1 = domain.u_max();
    let v0 = domain.v_min();
    let v1 = domain.v_max();

    let mut nu: usize = 5;
    let mut nv: usize = 5;

    loop {
        let us = uniform_params(u0, u1, nu);
        let vs = uniform_params(v0, v1, nv);
        let mut grid: Vec<Vec<Vec3>> = Vec::new();
        for &u in &us {
            let mut row = Vec::new();
            for &v in &vs {
                let p = surface.evaluate_point(k, Point2::new(u, v))?;
                row.push(map.map_point(k, p)?);
            }
            grid.push(row);
        }

        let deg_u = 3usize.min(nu.saturating_sub(1));
        let deg_v = 3usize.min(nv.saturating_sub(1));
        let knu_vec = interp_knot_vector(&us, deg_u)?;
        let knv_vec = interp_knot_vector(&vs, deg_v)?;
        let knu = KnotVector::from_slice(&knu_vec)?;
        let knv = KnotVector::from_slice(&knv_vec)?;
        let nurb = interp_given_knots(&grid, &us, &vs, &knu, &knv, deg_u, deg_v)?;
        let bsurf = BSplineSurface::from_nurb(nurb, false);
        let candidate = Surface::new(SurfaceKind::BSpline(bsurf));

        match certify_surface_fit(k, surface, &candidate, &us, &vs, map, tol)? {
            CertOutcome::Certified(bound) => return Ok((candidate, bound)),
            CertOutcome::NeedsRefine { refine_u, refine_v } => {
                let next_u = if refine_u && nu < MAX_SAMPLES {
                    (nu.saturating_sub(1) * 2 + 1).min(MAX_SAMPLES)
                } else {
                    nu
                };
                let next_v = if refine_v && nv < MAX_SAMPLES {
                    (nv.saturating_sub(1) * 2 + 1).min(MAX_SAMPLES)
                } else {
                    nv
                };
                if next_u == nu && next_v == nv {
                    return Err(SolidError::invalid_input(
                        OP,
                        "surface deformation fit exceeded sample budget before reaching certified tolerance",
                    ));
                }
                nu = next_u;
                nv = next_v;
            }
            CertOutcome::Uncertifiable => {
                return Err(SolidError::invalid_input(
                    OP,
                    "surface deformation fit could not be certified over its UV cells",
                ));
            }
        }
    }
}

enum CertOutcome {
    Certified(f64),
    NeedsRefine { refine_u: bool, refine_v: bool },
    Uncertifiable,
}

/// Certify `|fitted(u, v) - Φ(orig(u, v))| <= tol` across every UV cell of `us × vs`
/// using rounded-interval Mean-Value Form enclosures.
fn certify_surface_fit(
    k: &Kernel,
    orig: &Surface,
    fitted: &Surface,
    us: &[f64],
    vs: &[f64],
    map: &dyn SpaceMap,
    tol: f64,
) -> Result<CertOutcome> {
    let Some(orig_prep) = PreparedSurface::of(k, orig)? else {
        return Ok(CertOutcome::Uncertifiable);
    };
    let Some(fit_prep) = PreparedSurface::of(k, fitted)? else {
        return Ok(CertOutcome::Uncertifiable);
    };
    let mut work: Vec<(f64, f64, f64, f64, usize)> = Vec::new();
    for pu in us.windows(2) {
        let &[ua, ub] = pu else { continue };
        for pv in vs.windows(2) {
            let &[va, vb] = pv else { continue };
            work.push((ua, ub, va, vb, 0));
        }
    }

    let mut worst = 0.0_f64;
    let mut inspected: usize = 0;
    let mut any_uncertified = false;

    while let Some((ua, ub, va, vb, depth)) = work.pop() {
        inspected += 1;
        if inspected > MAX_CERT_SURFACE_CELLS {
            any_uncertified = true;
            break;
        }

        let um = 0.5 * (ua + ub);
        let vm = 0.5 * (va + vb);

        let Some(orig_mid) = orig_prep.jet_ivl(Ivl::from(um), Ivl::from(vm), 0)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let Some(fit_mid) = fit_prep.jet_ivl(Ivl::from(um), Ivl::from(vm), 0)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let p_mid_box = [
            (orig_mid.p.x.lo, orig_mid.p.x.hi),
            (orig_mid.p.y.lo, orig_mid.p.y.hi),
            (orig_mid.p.z.lo, orig_mid.p.z.hi),
        ];
        let Some(phi_mid_box) = map.enclose_box(k, p_mid_box)? else {
            if depth < MAX_CERT_DEPTH {
                push_quarters(&mut work, ua, ub, va, vb, um, vm, depth);
                continue;
            }
            return Ok(CertOutcome::Uncertifiable);
        };
        let err_mid = v3_sub(fit_mid.p, box_to_v3(phi_mid_box));
        // If even the midpoint error is already proved above `tol`, stop certifying and refine.
        if v3_norm_lower(err_mid) > tol {
            return Ok(CertOutcome::NeedsRefine {
                refine_u: true,
                refine_v: true,
            });
        }

        let u_ivl = Ivl::new(ua, ub);
        let v_ivl = Ivl::new(va, vb);
        let Some(orig_cell) = orig_prep.jet_ivl(u_ivl, v_ivl, 1)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let Some(fit_cell) = fit_prep.jet_ivl(u_ivl, v_ivl, 1)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let cell_box = [
            (orig_cell.p.x.lo, orig_cell.p.x.hi),
            (orig_cell.p.y.lo, orig_cell.p.y.hi),
            (orig_cell.p.z.lo, orig_cell.p.z.hi),
        ];

        let bound = if let Some((jac, slack)) = map.enclose_jet(k, cell_box)? {
            let du = u_ivl - Ivl::from(um);
            let dv = v_ivl - Ivl::from(vm);
            let dphi_du = mat_mul_v3(jac, orig_cell.su);
            let dphi_dv = mat_mul_v3(jac, orig_cell.sv);
            let rem_u = v3_scale(v3_sub(fit_cell.su, dphi_du), du);
            let rem_v = v3_scale(v3_sub(fit_cell.sv, dphi_dv), dv);
            let total = v3_add(v3_add(err_mid, v3_add(rem_u, rem_v)), box_to_v3(slack));
            v3_norm_upper(total)
        } else {
            f64::INFINITY
        };

        if bound <= tol {
            worst = worst.max(bound);
            continue;
        }
        if depth >= MAX_CERT_DEPTH {
            return Ok(CertOutcome::NeedsRefine {
                refine_u: true,
                refine_v: true,
            });
        }
        push_quarters(&mut work, ua, ub, va, vb, um, vm, depth);
    }

    if any_uncertified {
        return Ok(CertOutcome::NeedsRefine {
            refine_u: true,
            refine_v: true,
        });
    }
    Ok(CertOutcome::Certified(worst))
}

#[allow(clippy::too_many_arguments)]
fn push_quarters(
    work: &mut Vec<(f64, f64, f64, f64, usize)>,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    um: f64,
    vm: f64,
    depth: usize,
) {
    let nd = depth + 1;
    work.push((ua, um, va, vm, nd));
    work.push((ua, um, vm, vb, nd));
    work.push((um, ub, va, vm, nd));
    work.push((um, ub, vm, vb, nd));
}

fn refit_curve(
    k: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    start_pt: Option<Point3>,
    end_pt: Option<Point3>,
    map: &dyn SpaceMap,
    tol: f64,
) -> Result<(Curve, f64)> {
    let t0 = interval.min();
    let t1 = interval.max();
    if !t0.is_finite() || !t1.is_finite() || (t1 - t0) <= 1e-12 {
        return Err(SolidError::invalid_input(
            OP,
            "edge has a degenerate parameter interval",
        ));
    }

    let mut nt: usize = 9;
    loop {
        let ts = uniform_params(t0, t1, nt);
        let mut pts: Vec<Vec3> = Vec::new();
        for (idx, &t) in ts.iter().enumerate() {
            if idx == 0 {
                if let Some(p) = start_pt {
                    pts.push(p);
                    continue;
                }
            }
            if idx + 1 == ts.len() {
                if let Some(p) = end_pt {
                    pts.push(p);
                    continue;
                }
            }
            let p = curve.evaluate_point(k, t)?;
            pts.push(map.map_point(k, p)?);
        }

        let deg = 3usize.min(nt.saturating_sub(1));
        let uk = interp_knot_vector(&ts, deg)?;
        let knt = KnotVector::from_slice(&uk)?;
        let cm = calc_matrix(&knt, deg, &ts, MatrixEnds::None)?;
        let cpts = cpts_with_matrix(&pts, deg, &cm)?;
        let nurb = NurbCurve::new(cpts, deg, uk)?;
        let bcrv = BSplineCurve::from_nurb(nurb, 3, false);
        let candidate = Curve::new(CurveKind::BSpline(bcrv));

        match certify_curve_fit(k, curve, &candidate, &ts, map, tol)? {
            CertOutcome::Certified(bound) => return Ok((candidate, bound)),
            CertOutcome::NeedsRefine { .. } => {
                if nt >= MAX_SAMPLES {
                    return Err(SolidError::invalid_input(
                        OP,
                        "edge curve deformation fit exceeded sample budget before reaching certified tolerance",
                    ));
                }
                nt = (nt.saturating_sub(1) * 2 + 1).min(MAX_SAMPLES);
            }
            CertOutcome::Uncertifiable => {
                return Err(SolidError::invalid_input(
                    OP,
                    "edge curve deformation fit could not be certified",
                ));
            }
        }
    }
}

/// Certify `|fitted(t) - Φ(orig(t))| <= tol` across every span of `ts` using
/// rounded-interval Mean-Value Form enclosures.
fn certify_curve_fit(
    k: &Kernel,
    orig: &Curve,
    fitted: &Curve,
    ts: &[f64],
    map: &dyn SpaceMap,
    tol: f64,
) -> Result<CertOutcome> {
    let mut work: Vec<(f64, f64, usize)> = Vec::new();
    for pair in ts.windows(2) {
        let &[ta, tb] = pair else { continue };
        work.push((ta, tb, 0));
    }

    let mut worst = 0.0_f64;
    let mut inspected: usize = 0;

    while let Some((ta, tb, depth)) = work.pop() {
        inspected += 1;
        if inspected > MAX_CERT_CURVE_SPANS {
            return Ok(CertOutcome::NeedsRefine {
                refine_u: true,
                refine_v: false,
            });
        }

        let tm = 0.5 * (ta + tb);
        let Some((orig_mid, _)) = curve_jet_ivl(orig, Ivl::from(tm), 0)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let Some((fit_mid, _)) = curve_jet_ivl(fitted, Ivl::from(tm), 0)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let p_mid_box = [
            (orig_mid.x.lo, orig_mid.x.hi),
            (orig_mid.y.lo, orig_mid.y.hi),
            (orig_mid.z.lo, orig_mid.z.hi),
        ];
        let Some(phi_mid_box) = map.enclose_box(k, p_mid_box)? else {
            if depth < MAX_CERT_DEPTH {
                work.push((ta, tm, depth + 1));
                work.push((tm, tb, depth + 1));
                continue;
            }
            return Ok(CertOutcome::Uncertifiable);
        };
        let err_mid = v3_sub(fit_mid, box_to_v3(phi_mid_box));
        if v3_norm_lower(err_mid) > tol {
            return Ok(CertOutcome::NeedsRefine {
                refine_u: true,
                refine_v: false,
            });
        }

        let t_ivl = Ivl::new(ta, tb);
        let Some((orig_p, orig_dp)) = curve_jet_ivl(orig, t_ivl, 1)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let Some((_, fit_dp)) = curve_jet_ivl(fitted, t_ivl, 1)? else {
            return Ok(CertOutcome::Uncertifiable);
        };
        let p_box = [
            (orig_p.x.lo, orig_p.x.hi),
            (orig_p.y.lo, orig_p.y.hi),
            (orig_p.z.lo, orig_p.z.hi),
        ];
        let bound = if let Some((jac, slack)) = map.enclose_jet(k, p_box)? {
            let dt = t_ivl - Ivl::from(tm);
            let dphi_dt = mat_mul_v3(jac, orig_dp);
            let rem = v3_scale(v3_sub(fit_dp, dphi_dt), dt);
            let total = v3_add(v3_add(err_mid, rem), box_to_v3(slack));
            v3_norm_upper(total)
        } else {
            f64::INFINITY
        };

        if bound <= tol {
            worst = worst.max(bound);
            continue;
        }
        if depth >= MAX_CERT_DEPTH {
            return Ok(CertOutcome::NeedsRefine {
                refine_u: true,
                refine_v: false,
            });
        }
        work.push((ta, tm, depth + 1));
        work.push((tm, tb, depth + 1));
    }

    Ok(CertOutcome::Certified(worst))
}

/// Certify every rebuilt face's UV trim curve gap `|C'(t) - S'(p_eu(t))|` and vertex gap
/// over the full parameter interval in rounded interval arithmetic, refusing any uncertified
/// or out-of-bound gap and raising edge/vertex tolerances strictly to the proved bounds.
fn certify_and_update_tolerances(
    k: &Kernel,
    b: &mut Brep,
    rebuild_faces: &BTreeSet<FaceId>,
    surface_bounds: &BTreeMap<FaceId, f64>,
    edge_bounds: &BTreeMap<EdgeId, f64>,
    tol: f64,
) -> Result<()> {
    let mut edge_certified_gap: BTreeMap<EdgeId, f64> = BTreeMap::new();
    let mut vertex_certified_gap: BTreeMap<VertexId, f64> = BTreeMap::new();
    // `validate_pointers` rejects edge/vertex tolerances exceeding `1000 * brep.tolerance`.
    // Tighten the interval enclosure target so even after safety factors (`1.05x` / `2.02x`)
    // every certified edge/vertex tolerance stays strictly below that cap.
    let max_entity_tol = 450.0 * b.tolerance().max(k.tolerances.zone_tol_3d);

    for &f in rebuild_faces {
        let eps_f = surface_bounds.get(&f).copied().unwrap_or(0.0);
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::invalid_input(OP, "rebuilt face has no surface"))?
            .clone();
        let loops = crate::topology::face::p2::loops(b, f, false, None)?;
        for l in loops {
            let Some(lu) = b.loop_(l)?.loopuse else {
                continue;
            };
            let eus = crate::topology::loopuse::edgeuses(b, lu, None, None)?;
            for eu in eus {
                let Some(e) = b.edgeuse(eu)?.edge else {
                    continue;
                };
                let Some(edge_curve) = topo_edge::curve(b, e)? else {
                    continue;
                };
                let interval = topo_edge::interval(b, e)?;
                let (Some(uv_curve), _) = eu1::uv_trim_curve(b, eu)? else {
                    continue;
                };
                let eps_e = edge_bounds.get(&e).copied().unwrap_or(0.0);
                let allowed =
                    round_up(tol.max(eps_e + eps_f + b.edge(e)?.tolerance + b.face(f)?.tolerance))
                        .min(max_entity_tol);

                let gap =
                    enclose_edge_trim_gap(k, edge_curve, uv_curve, &surface, interval, allowed)?;
                if !(gap.is_finite() && gap <= allowed) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "rebuilt UV trim curve gap exceeds certified deformation bound",
                    ));
                }
                let entry = edge_certified_gap.entry(e).or_insert(0.0);
                *entry = entry.max(gap);

                // Certify vertex-to-curve endpoint gaps in rounded intervals.
                for (v_opt, t_end) in [
                    (topo_edge::start_vertex(b, e)?, interval.min()),
                    (topo_edge::end_vertex(b, e)?, interval.max()),
                ] {
                    if let Some(v) = v_opt {
                        let vp = b.vertex(v)?.point;
                        let Some((cp, _)) = curve_jet_ivl(edge_curve, Ivl::from(t_end), 0)? else {
                            return Err(SolidError::invalid_input(
                                OP,
                                "could not enclose edge endpoint at vertex",
                            ));
                        };
                        let diff = v3_sub(
                            cp,
                            V3 {
                                x: Ivl::from(vp.x),
                                y: Ivl::from(vp.y),
                                z: Ivl::from(vp.z),
                            },
                        );
                        let v_gap = v3_norm_upper(diff);
                        let v_allowed = round_up(allowed + b.vertex(v)?.tolerance);
                        if !(v_gap.is_finite() && v_gap <= v_allowed) {
                            return Err(SolidError::invalid_input(
                                OP,
                                "rebuilt vertex gap exceeds certified deformation bound",
                            ));
                        }
                        let ventry = vertex_certified_gap.entry(v).or_insert(0.0);
                        *ventry = ventry.max(v_gap);
                    }
                }
            }
        }
    }

    for (e, gap) in edge_certified_gap {
        let certified_tol = round_up(gap * 1.05);
        if certified_tol > b.edge(e)?.tolerance {
            topo_edge::set_tolerance(b, e, certified_tol, true, true)?;
        }
    }
    for (v, gap) in vertex_certified_gap {
        let certified_tol = round_up(gap * 2.02);
        if certified_tol > b.vertex(v)?.tolerance {
            topo_vertex::set_tolerance(b, v, certified_tol, true, false)?;
        }
    }
    Ok(())
}

/// Certified upper bound of `sup_{s ∈ [0, 1]} |edge_curve(t(s)) - surface(uv_curve(u(s)))|`.
fn enclose_edge_trim_gap(
    k: &Kernel,
    edge_curve: &Curve,
    uv_curve: &Curve,
    surface: &Surface,
    interval: Extent1d,
    allowed: f64,
) -> Result<f64> {
    let (t0, t1) = (interval.min(), interval.max());
    let uv_iv = uv_curve.natural_interval();
    let (u0, u1) = (uv_iv.min(), uv_iv.max());
    if !(t0.is_finite() && t1.is_finite() && t1 > t0 && u0.is_finite() && u1.is_finite() && u1 > u0)
    {
        return Ok(f64::INFINITY);
    }
    let dt_ds = Ivl::from(t1) - Ivl::from(t0);
    let du_ds = Ivl::from(u1) - Ivl::from(u0);
    let map_t = |s: Ivl| Ivl::from(t0) + s * dt_ds;
    let map_u = |s: Ivl| Ivl::from(u0) + s * du_ds;

    let Some(srf_prep) = PreparedSurface::of(k, surface)? else {
        return Ok(f64::INFINITY);
    };
    let srf_dom = surface.natural_uv_domain_with_nurb(k)?;
    let (su0, su1) = (srf_dom.u_min(), srf_dom.u_max());
    let (sv0, sv1) = (srf_dom.v_min(), srf_dom.v_max());
    let pad_u = 1e-10 * ((su1 - su0).abs() + 1.0);
    let pad_v = 1e-10 * ((sv1 - sv0).abs() + 1.0);
    let clamp_dom = |iv: Ivl, lo: f64, hi: f64, pad: f64| -> Option<(Ivl, f64)> {
        if iv.lo < lo - pad || iv.hi > hi + pad {
            return None;
        }
        let overhang = round_up((lo - iv.lo).max(0.0).max((iv.hi - hi).max(0.0)));
        let c = Ivl::new(iv.lo.max(lo).min(hi), iv.hi.min(hi).max(lo));
        Some((c, overhang))
    };

    let mut work: Vec<(f64, f64, usize)> = vec![(0.0, 1.0, 0)];
    let mut worst = 0.0_f64;
    let mut inspected: usize = 0;

    while let Some((sa, sb, depth)) = work.pop() {
        inspected += 1;
        if inspected > MAX_CERT_CURVE_SPANS {
            return Ok(f64::INFINITY);
        }

        let sm = 0.5 * (sa + sb);
        let tm = map_t(Ivl::from(sm));
        let um = map_u(Ivl::from(sm));
        let Some((c_mid, _)) = curve_jet_ivl(edge_curve, tm, 0)? else {
            return Ok(f64::INFINITY);
        };
        let Some((uv_mid, _)) = curve_jet_ivl(uv_curve, um, 0)? else {
            return Ok(f64::INFINITY);
        };
        let Some((um_c, ov_mu)) = clamp_dom(uv_mid.x, su0, su1, pad_u) else {
            return Ok(f64::INFINITY);
        };
        let Some((vm_c, ov_mv)) = clamp_dom(uv_mid.y, sv0, sv1, pad_v) else {
            return Ok(f64::INFINITY);
        };
        let Some(s_mid) = srf_prep.jet_ivl(um_c, vm_c, 0)? else {
            return Ok(f64::INFINITY);
        };
        let err_mid = v3_sub(c_mid, s_mid.p);
        if v3_norm_lower(err_mid) > allowed {
            return Ok(v3_norm_upper(err_mid));
        }

        let s_ivl = Ivl::new(sa, sb);
        let t_ivl = Ivl::new(map_t(s_ivl).lo.max(t0), map_t(s_ivl).hi.min(t1));
        let u_ivl = Ivl::new(map_u(s_ivl).lo.max(u0), map_u(s_ivl).hi.min(u1));
        let Some((_, dc_dt)) = curve_jet_ivl(edge_curve, t_ivl, 1)? else {
            return Ok(f64::INFINITY);
        };
        let Some((uv_span, duv_du)) = curve_jet_ivl(uv_curve, u_ivl, 1)? else {
            return Ok(f64::INFINITY);
        };
        let bound = if let (Some((us_c, ov_u)), Some((vs_c, ov_v))) = (
            clamp_dom(uv_span.x, su0, su1, pad_u),
            clamp_dom(uv_span.y, sv0, sv1, pad_v),
        ) {
            if let Some(s_span) = srf_prep.jet_ivl(us_c, vs_c, 1)? {
                let ds = s_ivl - Ivl::from(sm);
                let dc_ds = v3_scale(dc_dt, dt_ds);
                let ds_ds = v3_scale(
                    v3_add(v3_scale(s_span.su, duv_du.x), v3_scale(s_span.sv, duv_du.y)),
                    du_ds,
                );
                let rem = v3_scale(v3_sub(dc_ds, ds_ds), ds);
                let clamp_slack = round_up(
                    ov_u.max(ov_mu) * v3_norm_upper(s_span.su)
                        + ov_v.max(ov_mv) * v3_norm_upper(s_span.sv),
                );
                round_up(v3_norm_upper(v3_add(err_mid, rem)) + clamp_slack)
            } else {
                f64::INFINITY
            }
        } else {
            f64::INFINITY
        };

        if bound <= allowed {
            worst = worst.max(bound);
            continue;
        }
        if depth >= MAX_CERT_DEPTH {
            return Ok(bound);
        }
        work.push((sa, sm, depth + 1));
        work.push((sm, sb, depth + 1));
    }
    Ok(worst)
}

/// Outward-rounded enclosure of a curve's position (and first derivative when `order >= 1`)
/// over parameter interval `t`.
fn curve_jet_ivl(curve: &Curve, t: Ivl, order: usize) -> Result<Option<(V3<Ivl>, V3<Ivl>)>> {
    if !t.is_finite() {
        return Ok(None);
    }
    if let CurveKind::Line(l) = curve.kind() {
        let p0 = l.line_point();
        let dir = l.line_vector();
        let scale = Ivl::from(l.line_scale());
        let (step_t, jac) = if l.is_bounded() {
            let nurb_ivl = l.natural_interval();
            let anal_ivl = l.step_interval();
            let nlen = Ivl::from(nurb_ivl.max()) - Ivl::from(nurb_ivl.min());
            let alen = Ivl::from(anal_ivl.max()) - Ivl::from(anal_ivl.min());
            if !(nlen.is_finite() && nlen.lo > 0.0 && alen.is_finite()) {
                return Ok(None);
            }
            let ratio = alen / nlen;
            if l.inside_out() {
                let s = Ivl::from(anal_ivl.min()) + (Ivl::from(nurb_ivl.max()) - t) * ratio;
                (s, IZ - ratio)
            } else {
                let s = Ivl::from(anal_ivl.min()) + (t - Ivl::from(nurb_ivl.min())) * ratio;
                (s, ratio)
            }
        } else if l.inside_out() {
            (IZ - t, Ivl::from(-1.0))
        } else {
            (t, Ivl::from(1.0))
        };
        let v = V3 {
            x: Ivl::from(dir.x) * scale,
            y: Ivl::from(dir.y) * scale,
            z: Ivl::from(dir.z) * scale,
        };
        let pos = V3 {
            x: Ivl::from(p0.x) + v.x * step_t,
            y: Ivl::from(p0.y) + v.y * step_t,
            z: Ivl::from(p0.z) + v.z * step_t,
        };
        let dp = v3_scale(v, jac);
        return Ok((v3_finite(pos) && v3_finite(dp)).then_some((pos, dp)));
    }

    let Some(bs) = curve.b_spline_base() else {
        return Ok(None);
    };
    let Ok(nc) = NurbsCurve::of(bs) else {
        return Ok(None);
    };
    let derivs = match curve_derivs_over(&nc, t, order, None) {
        Ok(d) => d,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    let Some(&pos) = derivs.first() else {
        return Ok(None);
    };
    let dp = if order >= 1 {
        let Some(&d1) = derivs.get(1) else {
            return Ok(None);
        };
        d1
    } else {
        V3 {
            x: IZ,
            y: IZ,
            z: IZ,
        }
    };
    Ok((v3_finite(pos) && v3_finite(dp)).then_some((pos, dp)))
}

fn box_to_v3(b: Box3) -> V3<Ivl> {
    V3 {
        x: Ivl::new(b[0].0, b[0].1),
        y: Ivl::new(b[1].0, b[1].1),
        z: Ivl::new(b[2].0, b[2].1),
    }
}

fn jac_sub_identity(mut j: [[(f64, f64); 3]; 3]) -> [[(f64, f64); 3]; 3] {
    let d0 = Ivl::new(j[0][0].0, j[0][0].1) - Ivl::from(1.0);
    let d1 = Ivl::new(j[1][1].0, j[1][1].1) - Ivl::from(1.0);
    let d2 = Ivl::new(j[2][2].0, j[2][2].1) - Ivl::from(1.0);
    j[0][0] = (d0.lo, d0.hi);
    j[1][1] = (d1.lo, d1.hi);
    j[2][2] = (d2.lo, d2.hi);
    j
}

fn mat_mul_v3(m: [[(f64, f64); 3]; 3], v: V3<Ivl>) -> V3<Ivl> {
    let dot_row = |row: [(f64, f64); 3]| {
        Ivl::new(row[0].0, row[0].1) * v.x
            + Ivl::new(row[1].0, row[1].1) * v.y
            + Ivl::new(row[2].0, row[2].1) * v.z
    };
    V3 {
        x: dot_row(m[0]),
        y: dot_row(m[1]),
        z: dot_row(m[2]),
    }
}

fn v3_finite(v: V3<Ivl>) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn v3_add(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn v3_sub(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn v3_scale(a: V3<Ivl>, s: Ivl) -> V3<Ivl> {
    V3 {
        x: a.x * s,
        y: a.y * s,
        z: a.z * s,
    }
}

fn v3_norm_upper(v: V3<Ivl>) -> f64 {
    if !v3_finite(v) {
        return f64::INFINITY;
    }
    let mx = Ivl::from(v.x.mag());
    let my = Ivl::from(v.y.mag());
    let mz = Ivl::from(v.z.mag());
    (mx * mx + my * my + mz * mz).sqrt().hi
}

fn v3_norm_lower(v: V3<Ivl>) -> f64 {
    if !v3_finite(v) {
        return 0.0;
    }
    let mx = Ivl::from(v.x.mig());
    let my = Ivl::from(v.y.mig());
    let mz = Ivl::from(v.z.mig());
    (mx * mx + my * my + mz * mz).sqrt().lo
}

fn uniform_params(a: f64, b: f64, count: usize) -> Vec<f64> {
    let steps = count.saturating_sub(1).max(1);
    let mut out = Vec::new();
    for i in 0..=steps {
        if i == 0 {
            out.push(a);
        } else if i == steps {
            out.push(b);
        } else {
            out.push(a + (b - a) * (i as f64 / steps as f64));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deform::surface_map::{
        is_valid_direction, MapMode, ProjectionMode, SurfaceToSurfaceMap, SurfaceToSurfaceOptions,
    };
    use crate::geom::extent2d::Extent2d;
    use crate::kernel::KernelOptions;
    use crate::mass::integrator::properties::brep_volume_default;
    use crate::nurbs::math::CPoint;
    use crate::nurbs::srf_basic::Surface as NurbSurface;
    use crate::primitive::api_primitives::{create_box, create_cylinder};
    use crate::surface::plane::Plane;
    use crate::surface::surface::p2::{Surface, SurfaceKind};
    use crate::topology::brep::sheet::sheet_on;

    fn xy_plane() -> Surface {
        let dom = Extent2d::from_bounds(-100.0, -100.0, 100.0, 100.0);
        Surface::new(SurfaceKind::Plane(Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            crate::math::Vec2::new(1.0, 1.0),
            &dom,
        )))
    }

    fn flat_sheet_surface(width: f64, depth: f64) -> Surface {
        let mut grid = Vec::new();
        for iu in 0..=1_i32 {
            let x = f64::from(iu) * width;
            let mut row = Vec::new();
            for iv in 0..=1_i32 {
                let y = f64::from(iv) * depth;
                row.push(CPoint::from_point(Vec3::new(x, y, 0.0)));
            }
            grid.push(row);
        }
        let nurb = NurbSurface::new(
            grid,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::new(SurfaceKind::BSpline(BSplineSurface::from_nurb(nurb, false)))
    }

    fn dome_surface(width: f64, depth: f64, height: f64) -> Surface {
        let mut grid = Vec::new();
        for iu in 0..=2_i32 {
            let u = f64::from(iu) / 2.0;
            let x = u * width;
            let mut row = Vec::new();
            for iv in 0..=2_i32 {
                let v = f64::from(iv) / 2.0;
                let y = v * depth;
                // Quadratic Bezier basis at (0.5, 0.5) has weight 0.25 on the center pole,
                // so placing the center pole at 4*height gives surface peak = height.
                let z = if iu == 1 && iv == 1 {
                    4.0 * height
                } else {
                    0.0
                };
                row.push(CPoint::from_point(Vec3::new(x, y, z)));
            }
            grid.push(row);
        }
        let nurb = NurbSurface::new(
            grid,
            2,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::new(SurfaceKind::BSpline(BSplineSurface::from_nurb(nurb, false)))
    }

    #[test]
    fn identity_surface_map_preserves_box_volume() {
        let mut k = Kernel::new(&KernelOptions::default());
        let mut bx = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 4.0).unwrap();
        let orig_vol = brep_volume_default(&k, &mut bx, 1e-6).unwrap();

        let base = xy_plane();
        let control = xy_plane();
        let dom = Extent2d::from_bounds(0.0, 0.0, 10.0, 10.0);
        let map = SurfaceToSurfaceMap::new(
            &base,
            dom,
            &control,
            dom,
            SurfaceToSurfaceOptions::default(),
        )
        .unwrap();

        let mut deformed = deform_brep(&mut k, &bx, &map, &DeformOptions::default()).unwrap();
        let new_vol = brep_volume_default(&k, &mut deformed, 1e-6).unwrap();
        assert!(
            (new_vol - orig_vol).abs() < 1e-2,
            "expected {orig_vol}, got {new_vol}"
        );
    }

    #[test]
    fn dome_control_surface_shapes_box_and_sheet() {
        let mut k = Kernel::new(&KernelOptions::default());
        let bx = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 2.0).unwrap();

        let base = xy_plane();
        let base_dom = Extent2d::from_bounds(0.0, 0.0, 10.0, 10.0);
        let control = dome_surface(10.0, 10.0, 3.0);
        let ctrl_dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);

        let map = SurfaceToSurfaceMap::new(
            &base,
            base_dom,
            &control,
            ctrl_dom,
            SurfaceToSurfaceOptions {
                projection: ProjectionMode::Normal,
                map_mode: MapMode::NormalToNormal,
                scale: 1.0,
                ..SurfaceToSurfaceOptions::default()
            },
        )
        .unwrap();

        // Center of bottom face (5, 5, 0) should rise to z = 3.0.
        let p_mid = map.map_point(&k, Point3::new(5.0, 5.0, 0.0)).unwrap();
        assert!(
            (p_mid.z - 3.0).abs() < 1e-4,
            "expected z=3.0, got {}",
            p_mid.z
        );

        let mut deformed = deform_brep(&mut k, &bx, &map, &DeformOptions::default()).unwrap();
        let chk =
            validation::check_brep(&mut k, &mut deformed, validation::CheckLevel::Level0).unwrap();
        assert!(
            chk.assert_valid == Some(true),
            "deformed box failed check_brep: pointers={:?} assert_valid={:?} pointer_reports={:?} reports={:?}",
            chk.pointers,
            chk.assert_valid,
            chk.pointer_reports.reports(),
            chk.reports.reports()
        );
        let failure =
            crate::topology::brep::valid_and_outward_failure(&mut k, &mut deformed).unwrap();
        assert!(
            failure.is_none(),
            "deformed box failed valid_and_outward_failure: {failure:?}"
        );
        let vol = brep_volume_default(&k, &mut deformed, 1e-5).unwrap();
        // Convex dome offset outward by h=2 along normals expands volume by Steiner's formula (> 200).
        assert!(
            vol > 220.0 && vol < 360.0,
            "unexpected NormalToNormal deformed volume {vol}"
        );

        // Under Translate mode, vertical shearing preserves exact volume (Cavalieri's principle: 200.0).
        let translate_map = SurfaceToSurfaceMap::new(
            &base,
            base_dom,
            &control,
            ctrl_dom,
            SurfaceToSurfaceOptions {
                projection: ProjectionMode::Normal,
                map_mode: MapMode::Translate,
                scale: 1.0,
                ..SurfaceToSurfaceOptions::default()
            },
        )
        .unwrap();
        let mut sheared =
            deform_brep(&mut k, &bx, &translate_map, &DeformOptions::default()).unwrap();
        let sheared_vol = brep_volume_default(&k, &mut sheared, 1e-5).unwrap();
        assert!(
            (sheared_vol - 200.0).abs() < 1.0,
            "expected Cavalieri volume 200.0, got {sheared_vol}"
        );

        // Sheet body deformation: bounded flat sheet over [0,10]^2 deformed onto the dome.
        let flat = flat_sheet_surface(10.0, 10.0);
        let (sheet, _f) = sheet_on(&mut k, flat, 1e-6).unwrap();
        let deformed_sheet = deform_brep(&mut k, &sheet, &map, &DeformOptions::default()).unwrap();
        let faces = brep_faces(&deformed_sheet, None).unwrap();
        assert_eq!(faces.len(), 1);
        let face_srf = deformed_sheet
            .face(faces[0])
            .unwrap()
            .surface
            .as_ref()
            .unwrap();
        let center_pt = face_srf.evaluate_point(&k, Point2::new(0.5, 0.5)).unwrap();
        assert!(
            (center_pt.z - 3.0).abs() < 1e-2,
            "expected deformed sheet center z ≈ 3.0, got {}",
            center_pt.z
        );
    }

    #[test]
    fn cell_interior_bump_vanishing_on_initial_grid_lines_is_captured() {
        let mut k = Kernel::new(&KernelOptions::default());
        let flat = flat_sheet_surface(10.0, 10.0);
        let (sheet, _f) = sheet_on(&mut k, flat, 1e-6).unwrap();

        // Build a smooth cubic B-spline control surface on [0, 1]^2 with a localized bump
        // strictly inside the first cell [0, 0.25] x [0, 0.25], vanishing identically on
        // every initial 5x5 grid line u, v ∈ {0.0, 0.25, 0.5, 0.75, 1.0}.
        // With clamped cubic knot vector [0,0,0,0, 0.25, 0.25, 0.25, 1,1,1,1] (Wait: C^0 at
        // 0.25 gives support [0, 0.25]; even smoother: C^2 clamped cubic with knots
        // [0,0,0,0, 0.0625, 0.125, 0.1875, 0.25, 0.5, 0.75, 1,1,1,1] -> single B-spline
        // basis function B_{1,3} is C^2 smooth and strictly supported on [0, 0.25]!).
        let knots = vec![
            0.0, 0.0, 0.0, 0.0, 0.0625, 0.125, 0.1875, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0,
        ];
        // Greville abscissae for exact linear x(u), y(v) parameterization.
        let n_cp = knots.len() - 4; // 10 control points
        let greville: Vec<f64> = (0..n_cp)
            .map(|i| (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0)
            .collect();
        // Basis function index 3 has support [knots[3], knots[7]] = [0.0, 0.25]!
        // It is C^2 smooth everywhere, vanishes identically at u = 0 and u >= 0.25,
        // and peaks inside (0, 0.25) near u = 0.125!
        let mut grid = Vec::new();
        for iu in 0..n_cp {
            let mut row = Vec::new();
            for iv in 0..n_cp {
                let z = if iu == 3 && iv == 3 { 4.0 } else { 0.0 };
                row.push(CPoint::from_point(Vec3::new(
                    greville[iu] * 10.0,
                    greville[iv] * 10.0,
                    z,
                )));
            }
            grid.push(row);
        }
        let nurb = NurbSurface::new(grid, 3, 3, knots.clone(), knots).unwrap();
        let control = Surface::new(SurfaceKind::BSpline(BSplineSurface::from_nurb(nurb, false)));
        let base = xy_plane();
        let base_dom = Extent2d::from_bounds(0.0, 0.0, 10.0, 10.0);
        let ctrl_dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);

        let map = SurfaceToSurfaceMap::new(
            &base,
            base_dom,
            &control,
            ctrl_dom,
            SurfaceToSurfaceOptions {
                projection: ProjectionMode::Normal,
                map_mode: MapMode::Translate,
                scale: 1.0,
                ..SurfaceToSurfaceOptions::default()
            },
        )
        .unwrap();

        // Confirm the bump is strictly nonzero inside the first cell at (1.25, 1.25)
        // while vanishing on the initial 5x5 grid lines.
        let expected_peak = map.map_point(&k, Point3::new(1.25, 1.25, 0.0)).unwrap();
        assert!(
            expected_peak.z > 0.5,
            "bump should be substantial: {}",
            expected_peak.z
        );
        assert!(
            map.map_point(&k, Point3::new(2.5, 2.5, 0.0))
                .unwrap()
                .z
                .abs()
                < 1e-12
        );

        let tol = 1e-2;
        let deformed = deform_brep(
            &mut k,
            &sheet,
            &map,
            &DeformOptions {
                tolerance: tol,
                faces: None,
            },
        )
        .unwrap();
        let faces = brep_faces(&deformed, None).unwrap();
        let face_srf = deformed.face(faces[0]).unwrap().surface.as_ref().unwrap();
        let got_peak = face_srf
            .evaluate_point(&k, Point2::new(0.125, 0.125))
            .unwrap();
        assert!(
            (got_peak.z - expected_peak.z).abs() <= tol,
            "interior bump missed: got z={}, expected z={}",
            got_peak.z,
            expected_peak.z
        );
    }

    #[test]
    fn zero_scale_and_identity_preserve_exact_cylinder_analytics() {
        let mut k = Kernel::new(&KernelOptions::default());
        let cyl = create_cylinder(&mut k, Point3::new(5.0, 5.0, 0.0), 3.0, 4.0).unwrap();
        let (mut base_sheet, base_f) =
            sheet_on(&mut k, flat_sheet_surface(10.0, 10.0), 1e-6).unwrap();
        let (mut dome_sheet, dome_f) =
            sheet_on(&mut k, dome_surface(10.0, 10.0, 3.0), 1e-6).unwrap();

        // Zero scale on a non-trivial dome still preserves exact cylinder/circle analytics.
        let zero_out = deform_brep_by_surface(
            &mut k,
            &cyl,
            &mut base_sheet,
            base_f,
            &mut dome_sheet,
            dome_f,
            SurfaceToSurfaceOptions {
                scale: 0.0,
                ..SurfaceToSurfaceOptions::default()
            },
            &DeformOptions::default(),
        )
        .unwrap();
        let has_exact_cone = brep_faces(&zero_out, None).unwrap().into_iter().any(|f| {
            matches!(
                zero_out.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Cone(_)
            )
        });
        assert!(
            has_exact_cone,
            "zero scale must preserve exact analytic cylinder face"
        );

        // Identity map (base == control) at scale = 1.0 also preserves exact cylinder analytics.
        let (mut base_sheet_2, base_f_2) =
            sheet_on(&mut k, flat_sheet_surface(10.0, 10.0), 1e-6).unwrap();
        let id_out = deform_brep_by_surface(
            &mut k,
            &cyl,
            &mut base_sheet,
            base_f,
            &mut base_sheet_2,
            base_f_2,
            SurfaceToSurfaceOptions::default(),
            &DeformOptions::default(),
        )
        .unwrap();
        let has_exact_cone_id = brep_faces(&id_out, None).unwrap().into_iter().any(|f| {
            matches!(
                id_out.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Cone(_)
            )
        });
        assert!(
            has_exact_cone_id,
            "identity map must preserve exact analytic cylinder face"
        );
    }

    #[test]
    fn directional_projection_rejects_misses_and_accepts_tiny_scale_directions() {
        let k = Kernel::new(&KernelOptions::default());
        assert!(is_valid_direction(Vec3::new(0.0, 0.0, 1e-13)));
        assert!(is_valid_direction(Vec3::new(0.0, 0.0, 1e200)));
        assert!(!is_valid_direction(Vec3::new(0.0, 0.0, 0.0)));
        assert!(!is_valid_direction(Vec3::new(f64::NAN, 0.0, 1.0)));

        let base = xy_plane();
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let map = SurfaceToSurfaceMap::new(
            &base,
            dom,
            &base,
            dom,
            SurfaceToSurfaceOptions {
                projection: ProjectionMode::Direction(Vec3::new(0.0, 0.0, 1e-13)),
                map_mode: MapMode::Translate,
                scale: 1.0,
                tolerance: 1e-4,
                ..SurfaceToSurfaceOptions::default()
            },
        )
        .unwrap();
        // Interior hit succeeds.
        let hit = map.map_point(&k, Point3::new(0.5, 0.5, 1.0)).unwrap();
        assert!((hit - Point3::new(0.5, 0.5, 1.0)).length() < 1e-9);
        // Ray through (2, 0.5, 1) along +Z misses [0, 1]^2 and must be rejected.
        assert!(map.map_point(&k, Point3::new(2.0, 0.5, 1.0)).is_err());
    }
}
