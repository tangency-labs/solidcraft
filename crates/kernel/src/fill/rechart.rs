//! A regular chart fitted to a singular tensor chart's exact boundary. The
//! reported distance is sampled in both directions, independently of the guide
//! points, just as subdivision-to-NURBS fitting reports a sampled distance.
use solidcore_ops::form_fill::rechart as geometry;
use solidcraft_geom::bspline::{HPoint, NurbsSurface};
use solidcraft_geom::fill_general::{Across, MAX_FILL_POINTS};
use solidcraft_geom::fill_iso::{IsoEdge, IsoFill, IsoSpec, iso_fill};

use crate::{KernelError, Result};

mod pole;

fn tensor(source: &NurbsSurface) -> geometry::Tensor {
    geometry::Tensor {
        degree: source.degree(),
        knots: source.knots().clone(),
        controls: source.homogeneous().iter().map(|h| [h.x, h.y, h.z, h.w]).collect(),
    }
}
fn prepared(source: &NurbsSurface) -> Result<geometry::Source> {
    geometry::Source::new(tensor(source)).map_err(failed)
}
/// A collinear tensor corner is singular even when its geometric boundary is
/// smooth. Express it as an explicit collapsed pole side, preserving the exact
/// rational surface instead of introducing an approximate corner fit.
pub(super) fn pole_atlas(source: &NurbsSurface, domain: [[f64; 2]; 2]) -> Result<Option<crate::Body>> {
    let source_tensor = tensor(source);
    if geometry::singular_corner(&source_tensor).map_err(failed)?.is_none() {
        return Ok(None);
    }
    let domains = geometry::atlas_domains(source.knots(), domain).map_err(failed)?;
    let mut sheets = Vec::new();
    for domain in domains {
        let crop = restricted(source, domain)?;
        let wire = geometry::rectangle(domain).to_vec();
        let exact = crate::nurbs_region_sheet(&crop, &[wire])?;
        if super::checked_rechart(&exact).is_ok() {
            sheets.push(exact);
            continue;
        }
        let corner = geometry::singular_corner(&tensor(&crop)).map_err(failed)?.ok_or_else(|| failed("a singular Bezier cell has no corner pole"))?;
        for corners in [[corner, (corner + 1) % 4, (corner + 3) % 4], [(corner + 2) % 4, (corner + 3) % 4, (corner + 1) % 4]] {
            let sheet = pole_triangle(&crop, corners)?;
            super::checked_rechart(&sheet)?;
            sheets.push(sheet);
        }
    }
    let sheet = crate::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7)?;
    super::checked_rechart(&sheet)?;
    Ok(Some(sheet))
}

fn pole_triangle(source: &NurbsSurface, corners: [usize; 3]) -> Result<crate::Body> {
    use crate::surfaces::{Parts, spline_curve, spline_surface, surface_body, uv_line};
    use solidcore_record::brep as b;
    let surface = pole::triangle(source, corners)?;
    let domain = source.domain();
    let parameters = solidcore_ops::form_fill::triangle_parameters(domain, corners).map_err(failed)?;
    let mut parts = Parts { tolerance: 1e-5, ..Default::default() };
    for uv in parameters {
        let point = geometry::point(source.degree(), source.knots(), source.count(), source.homogeneous(), uv, |h| [h.x, h.y, h.z, h.w])
            .ok_or_else(|| failed("source corner cannot be evaluated"))?;
        parts.vertices.push(b::Vertex { point: [point.x, point.y, point.z], tolerance: parts.tolerance });
    }
    let mut uses = Vec::with_capacity(3);
    for (i, (a, b, uv)) in
        [(0, 1, [[0.0, 0.0], [1.0, 0.0]]), (1, 2, [[1.0, 0.0], [1.0, 1.0]]), (2, 0, [[1.0, 1.0], [0.0, 1.0]])].into_iter().enumerate()
    {
        let (knots, controls) = source.parameter_line(parameters[a], parameters[b]).map_err(failed)?;
        let (curve, interval) = spline_curve(&controls, knots)?;
        parts.edges.push(b::Edge { nurbs: None, curve, interval, vertices: [a as u32, b as u32], tolerance: parts.tolerance, uses: Vec::new() });
        uses.push(b::OrientedEdge { edge: i as u32, forward: true, pcurve: uv_line(uv[0], uv[1], interval) });
    }
    parts.faces.push((
        b::Face { nurbs: None, surface: spline_surface(&surface), domain: surface.domain(), loops: vec![b::Loop::Edges { edges: uses }] },
        true,
    ));
    surface_body(&parts.canonical()?.brep)
}

/// Exact restriction, retaining original parameters and coefficient ordering.
pub(super) fn restricted(source: &NurbsSurface, domain: [[f64; 2]; 2]) -> Result<NurbsSurface> {
    let output = geometry::restricted(&tensor(source), domain).map_err(failed)?;
    let controls = output.controls.into_iter().map(|[x, y, z, w]| HPoint { x, y, z, w }).collect();
    NurbsSurface::from_homogeneous(output.degree, output.knots, controls).map_err(failed)
}

fn failed(detail: impl std::fmt::Display) -> KernelError {
    KernelError::Invalid(format!("cannot rechart the subdivision surface: {detail}"))
}

pub(super) fn fit(
    source: &NurbsSurface,
    edges: &[IsoEdge],
    wire: &[[f64; 2]],
    boundary_uv: &dyn Fn(usize, f64) -> Option<[f64; 2]>,
    tolerance: f64,
) -> Result<(IsoFill, f64)> {
    let source_eval = prepared(source)?;
    source_eval
        .fit(wire, tolerance, MAX_FILL_POINTS, |guides| {
            let mut across = |edge: usize, ts: &[f64]| -> std::result::Result<Vec<Option<Across>>, String> {
                Ok(ts
                    .iter()
                    .map(|&t| {
                        let [u, v] = boundary_uv(edge, t)?;
                        let tangent = edges.get(edge)?.tangent(t)?;
                        let [leave, normal] = source_eval.frame([u, v], tangent)?;
                        Some(Across { leave, normal, bend: None, jet: None })
                    })
                    .collect())
            };
            let spec = IsoSpec {
                tol: tolerance * 0.2,
                angle_tol: super::PATCH_ANGLE_TOL_DEG.to_radians(),
                crown: 0.0,
                corner_turn: 1.0_f64.to_radians(),
                points: guides,
            };
            let fill = iso_fill(edges, &mut across, &spec).map_err(failed)?;
            let fitted = fill.patches.iter().map(|patch| prepared(&patch.surface)).collect::<Result<Vec<_>>>()?;
            Ok::<_, KernelError>((fill, fitted))
        })
        .map_err(|error| match error {
            geometry::FitError::Build(error) => error,
            geometry::FitError::Kernel(error) => failed(error),
            geometry::FitError::InvalidInput => failed("a finite positive error budget and a simple bounded trim are required"),
            geometry::FitError::Distance(deviation) => {
                failed(format!("sampled atlas distance {deviation:.6} mm exceeds its {tolerance:.6} mm error budget"))
            }
        })
}
