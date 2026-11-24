//! Topology-preserving B-rep deformation driven by a [`SpaceMap`].
//!
//! For each affected face, edge, and vertex:
//! 1. Vertices are mapped directly through `map.map_point`.
//! 2. Affected face surfaces are sampled over their active UV domain, mapped through `map`,
//!    and adaptively refitted as B-spline surfaces on the *same* parameter domain so existing
//!    trim loops remain valid.
//! 3. Affected edge curves are sampled over their active parameter interval, snapped at
//!    mapped endpoints, and adaptively refitted as B-spline curves on the same interval.
//! 4. Edits are applied inside `apply_deformed_geometry`, UV trim curves are rebuilt,
//!    infinite regions are refreshed, and closed solids are verified to keep positive volume.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::curve::p2::{Curve, CurveKind};
use crate::deform::SpaceMap;
use crate::error::{Result, SolidError};
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
use crate::topology::brep::mirror::apply_deformed_geometry;
use crate::topology::brep::p1::{
    edges as brep_edges, faces as brep_faces, regions as brep_regions, vertices as brep_vertices,
};
use crate::topology::brep::p3::brep_copy;
use crate::topology::brep::validation;
use crate::topology::edge as topo_edge;
use crate::topology::face::p1::calculate_uv_domain_from_uv_trim_curves;
use crate::topology::{Brep, EdgeId, FaceId, VertexId};

const OP: &str = "deform::brep::deform_brep";

/// Maximum sample count per direction when adaptively refitting deformed surfaces/curves.
const MAX_SAMPLES: usize = 65;

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
/// [`SurfaceToSurfaceMap::new`] or [`deform_brep`] fails.
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
    let base_dom = face_uv_domain(k, base_brep, base_face)?;
    let base_srf = base_brep
        .face(base_face)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::topology_missing(OP, "base face surface"))?;
    let ctrl_dom = face_uv_domain(k, control_brep, control_face)?;
    let ctrl_srf = control_brep
        .face(control_face)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::topology_missing(OP, "control face surface"))?;

    surface_options.base_forward = face_forward(base_brep, base_face)?;
    surface_options.control_forward = face_forward(control_brep, control_face)?;

    let map = crate::deform::surface_map::SurfaceToSurfaceMap::new(
        base_srf,
        base_dom,
        ctrl_srf,
        ctrl_dom,
        surface_options,
    )?;
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
/// adjacent faces, or a closed solid collapses/inverts to non-positive volume.
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

    let mut out = brep_copy(k, brep)?;
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
            for &f in list {
                if !all_face_set.contains(&f) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "selected face does not belong to target body",
                    ));
                }
                set.insert(f);
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

    // If deforming only a subset of faces, verify shared boundary edges stay fixed within tol.
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

    // 1. Precompute mapped vertices.
    let mut mapped_vertices: BTreeMap<VertexId, Point3> = BTreeMap::new();
    for &v in &affected_vertices {
        let pt = out.vertex(v)?.point;
        mapped_vertices.insert(v, map.map_point(k, pt)?);
    }

    // 2. Precompute refitted face surfaces.
    let mut mapped_surfaces: BTreeMap<FaceId, Surface> = BTreeMap::new();
    for &f in &selected_faces {
        let domain = face_uv_domain(k, &mut out, f)?;
        let surface = out
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::topology_missing(OP, "face surface"))?;
        let refitted = refit_surface(k, surface, &domain, map, options.tolerance)?;
        mapped_surfaces.insert(f, refitted);
    }

    // 3. Precompute refitted edge curves.
    let mut mapped_curves: BTreeMap<EdgeId, Curve> = BTreeMap::new();
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
        let refitted = refit_curve(k, curve, interval, start_pt, end_pt, map, options.tolerance)?;
        mapped_curves.insert(e, refitted);
    }

    // 4. Apply all geometry replacements cleanly inside `plain_edit`.
    let was_closed_solid = is_closed_solid(k, &mut out)?;
    apply_deformed_geometry(
        k,
        &mut out,
        &mapped_vertices,
        mapped_curves,
        mapped_surfaces,
        &selected_faces,
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

fn is_closed_solid(k: &Kernel, b: &mut Brep) -> Result<bool> {
    if brep_regions(b, None)?.len() <= 1 {
        return Ok(false);
    }
    Ok(matches!(
        validation::volume_sign(k, b)?,
        VolumeSign::Positive(_)
    ))
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
    const CHECKS: usize = 5;
    for i in 0..=CHECKS {
        let t = iv.min() + (iv.max() - iv.min()) * (i as f64 / CHECKS as f64);
        let p = curve.evaluate_point(k, t)?;
        let q = map.map_point(k, p)?;
        if (q - p).length() > tol {
            return Err(SolidError::invalid_input(
                OP,
                "selected faces cannot deform independently: shared boundary edges move off unselected adjacent faces",
            ));
        }
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
        .ok_or_else(|| SolidError::topology_missing(OP, "face surface"))?;
    surface.natural_uv_domain_with_nurb(k)
}

fn refit_surface(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    map: &dyn SpaceMap,
    tol: f64,
) -> Result<Surface> {
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

        let (err_u, err_v) = surface_midspan_error(k, surface, &candidate, &us, &vs, map)?;
        if err_u <= tol && err_v <= tol {
            return Ok(candidate);
        }
        let next_u = if err_u > tol && nu < MAX_SAMPLES {
            (nu.saturating_sub(1) * 2 + 1).min(MAX_SAMPLES)
        } else {
            nu
        };
        let next_v = if err_v > tol && nv < MAX_SAMPLES {
            (nv.saturating_sub(1) * 2 + 1).min(MAX_SAMPLES)
        } else {
            nv
        };
        if next_u == nu && next_v == nv {
            return Ok(candidate);
        }
        nu = next_u;
        nv = next_v;
    }
}

fn surface_midspan_error(
    k: &Kernel,
    orig: &Surface,
    fitted: &Surface,
    us: &[f64],
    vs: &[f64],
    map: &dyn SpaceMap,
) -> Result<(f64, f64)> {
    let mut err_u = 0.0_f64;
    let mut err_v = 0.0_f64;
    for pair_u in us.windows(2) {
        let Some(&ua) = pair_u.first() else { continue };
        let Some(&ub) = pair_u.get(1) else { continue };
        let um = 0.5 * (ua + ub);
        for &v in vs {
            let uv = Point2::new(um, v);
            let target = map.map_point(k, orig.evaluate_point(k, uv)?)?;
            let got = fitted.evaluate_point(k, uv)?;
            err_u = err_u.max((got - target).length());
        }
    }
    for &u in us {
        for pair_v in vs.windows(2) {
            let Some(&va) = pair_v.first() else { continue };
            let Some(&vb) = pair_v.get(1) else { continue };
            let vm = 0.5 * (va + vb);
            let uv = Point2::new(u, vm);
            let target = map.map_point(k, orig.evaluate_point(k, uv)?)?;
            let got = fitted.evaluate_point(k, uv)?;
            err_v = err_v.max((got - target).length());
        }
    }
    Ok((err_u, err_v))
}

fn refit_curve(
    k: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    start_pt: Option<Point3>,
    end_pt: Option<Point3>,
    map: &dyn SpaceMap,
    tol: f64,
) -> Result<Curve> {
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

        let mut max_err = 0.0_f64;
        for pair in ts.windows(2) {
            let Some(&ta) = pair.first() else { continue };
            let Some(&tb) = pair.get(1) else { continue };
            let tm = 0.5 * (ta + tb);
            let target = map.map_point(k, curve.evaluate_point(k, tm)?)?;
            let got = candidate.evaluate_point(k, tm)?;
            max_err = max_err.max((got - target).length());
        }
        if max_err <= tol || nt >= MAX_SAMPLES {
            return Ok(candidate);
        }
        nt = (nt.saturating_sub(1) * 2 + 1).min(MAX_SAMPLES);
    }
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
        MapMode, ProjectionMode, SurfaceToSurfaceMap, SurfaceToSurfaceOptions,
    };
    use crate::geom::extent2d::Extent2d;
    use crate::kernel::KernelOptions;
    use crate::mass::integrator::properties::brep_volume_default;
    use crate::nurbs::math::CPoint;
    use crate::nurbs::srf_basic::Surface as NurbSurface;
    use crate::primitive::api_primitives::create_box;
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

    fn dome_surface(width: f64, depth: f64, height: f64) -> Surface {
        let mut grid = Vec::new();
        for iu in 0..=2 {
            let u = iu as f64 / 2.0;
            let x = u * width;
            let mut row = Vec::new();
            for iv in 0..=2 {
                let v = iv as f64 / 2.0;
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

        // Sheet body deformation: flat sheet on [0,10]^2 deformed onto the dome.
        let (sheet, _f) = sheet_on(&mut k, control.clone(), 1e-6).unwrap();
        let deformed_sheet = deform_brep(&mut k, &sheet, &map, &DeformOptions::default()).unwrap();
        assert_eq!(brep_faces(&deformed_sheet, None).unwrap().len(), 1);
    }
}
