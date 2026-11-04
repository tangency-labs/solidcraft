//! A regular chart fitted to a singular tensor chart's exact boundary. The
//! reported distance is sampled in both directions, independently of the guide
//! points, just as subdivision-to-NURBS fitting reports a sampled distance.
use solidcore_record::Vec3;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::bspline::NurbsSurface;
use solidcraft_geom::fill_general::{Across, MAX_FILL_POINTS};
use solidcraft_geom::fill_iso::{IsoEdge, IsoFill, IsoSpec, iso_fill};
use solidcraft_geom::prelude::*;
use solidcraft_geom::tess::{EvalSurface, uv::Starts};

use crate::{KernelError, Result};

mod pole;

fn singular_corner(source: &NurbsSurface) -> Option<usize> {
    let eval = EvalSurface::from(source);
    let [[u0, u1], [v0, v1]] = source.domain();
    [[u0, v0, 1.0, 1.0], [u1, v0, -1.0, 1.0], [u1, v1, -1.0, -1.0], [u0, v1, 1.0, -1.0]].into_iter().enumerate().find_map(
        |(corner, [u, v, su, sv])| {
            let (_, a, b) = eval.derivs(u, v)?;
            let (a, b) = (a.normalized()? * su, b.normalized()? * sv);
            (a.cross(b).len() < 0.5_f64.to_radians().sin()).then_some(corner)
        },
    )
}

/// A collinear tensor corner is singular even when its geometric boundary is
/// smooth. Express it as an explicit collapsed pole side, preserving the exact
/// rational surface instead of introducing an approximate corner fit.
pub(super) fn pole_atlas(source: &NurbsSurface, domain: [[f64; 2]; 2]) -> Result<Option<crate::Body>> {
    if singular_corner(source).is_none() {
        return Ok(None);
    }
    let axes: [Vec<f64>; 2] = std::array::from_fn(|d| {
        let mut knots = std::iter::once(domain[d][0])
            .chain(source.knots()[d].iter().copied().filter(|k| *k > domain[d][0] && *k < domain[d][1]))
            .chain(std::iter::once(domain[d][1]))
            .collect::<Vec<_>>();
        knots.dedup();
        knots
    });
    if axes[0].len().saturating_sub(1).saturating_mul(axes[1].len().saturating_sub(1)) > 256 {
        return Err(KernelError::TooLarge("subdivision pole atlas cell budget".into()));
    }
    let mut sheets = Vec::new();
    for u in axes[0].windows(2) {
        for v in axes[1].windows(2) {
            let crop = restricted(source, [[u[0], u[1]], [v[0], v[1]]])?;
            let wire = vec![[u[0], v[0]], [u[1], v[0]], [u[1], v[1]], [u[0], v[1]]];
            let exact = crate::nurbs_region_sheet(&crop, &[wire])?;
            if super::checked_rechart(&exact).is_ok() {
                sheets.push(exact);
                continue;
            }
            let corner = singular_corner(&crop).ok_or_else(|| failed("a singular Bezier cell has no corner pole"))?;
            for corners in [[corner, (corner + 1) % 4, (corner + 3) % 4], [(corner + 2) % 4, (corner + 3) % 4, (corner + 1) % 4]] {
                let sheet = pole_triangle(&crop, corners)?;
                super::checked_rechart(&sheet)?;
                sheets.push(sheet);
            }
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
    let unit = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let domain = source.domain();
    let parameters = corners.map(|corner| std::array::from_fn(|d| domain[d][0] + (domain[d][1] - domain[d][0]) * unit[corner][d]));
    let mut parts = Parts { tolerance: 1e-5, ..Default::default() };
    for uv in parameters {
        let point = source.eval(uv[0], uv[1]).map_err(failed)?;
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

const CHECK_CELLS: usize = 24;
const MAX_CHECK_STATIONS: usize = 129;
const ROUNDS: usize = 3;

/// Exact restriction by Boehm knot insertion in each direction. It changes the
/// support's domain to the part actually used; all original parameters survive.
pub(super) fn restricted(source: &NurbsSurface, domain: [[f64; 2]; 2]) -> Result<NurbsSurface> {
    let mut surface = source.clone();
    for (d, range) in domain.into_iter().enumerate() {
        let [nu, nv] = surface.count();
        let (lines, width) = if d == 0 { (nv, nu) } else { (nu, nv) };
        let mut rows = Vec::with_capacity(lines);
        let original_knots = surface.knots().clone();
        let mut knots = original_knots.clone();
        for line in 0..lines {
            let row = (0..width).map(|i| if d == 0 { surface.point(i, line) } else { surface.point(line, i) }).collect::<Vec<_>>();
            let (k, row) = solidcraft_geom::bspline::segment(surface.degree()[d], &original_knots[d], &row, range[0], range[1]).map_err(failed)?;
            knots[d] = k;
            rows.push(row);
        }
        let count = rows.first().map_or(0, Vec::len);
        let mut controls = Vec::with_capacity(count * lines);
        if d == 0 {
            for row in rows {
                controls.extend(row);
            }
        } else {
            for j in 0..count {
                for row in &rows {
                    controls.push(*row.get(j).ok_or_else(|| failed("incomplete restricted tensor row"))?);
                }
            }
        }
        surface = NurbsSurface::from_homogeneous(surface.degree(), knots, controls).map_err(failed)?;
    }
    Ok(surface)
}

fn failed(detail: impl std::fmt::Display) -> KernelError {
    KernelError::Invalid(format!("cannot rechart the subdivision surface: {detail}"))
}

/// A parameter point is inside the original trim or within parameter roundoff
/// of its rim. The polygon is solely a trim-domain test, never fitted geometry.
fn inside(wire: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut result = false;
    for (a, b) in wire.iter().zip(wire.iter().cycle().skip(1)).take(wire.len()) {
        let delta = [b[0] - a[0], b[1] - a[1]];
        let cross = delta[0] * (p[1] - a[1]) - delta[1] * (p[0] - a[0]);
        let scale = 1.0 + a[0].abs() + a[1].abs() + b[0].abs() + b[1].abs();
        if cross.abs() <= 1e-10 * scale && (0..2).all(|d| p[d] >= a[d].min(b[d]) - 1e-10 * scale && p[d] <= a[d].max(b[d]) + 1e-10 * scale) {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + delta[0] * (p[1] - a[1]) / delta[1] {
            result = !result;
        }
    }
    result
}

fn stations(surface: &NurbsSurface, domain: [[f64; 2]; 2]) -> Result<[Vec<f64>; 2]> {
    let mut result: [Vec<f64>; 2] = std::array::from_fn(|d| {
        let [a, b] = domain[d];
        std::iter::once(a).chain((0..CHECK_CELLS).map(|i| a + (b - a) * (i as f64 + 0.5) / CHECK_CELLS as f64)).chain(std::iter::once(b)).collect()
    });
    for (d, axis) in result.iter_mut().enumerate() {
        for span in surface.knots()[d].windows(2) {
            let [a, b] = [span.first().copied().unwrap_or(0.0), span.get(1).copied().unwrap_or(0.0)];
            let middle = a + (b - a) * 0.5;
            if b > a && middle > domain[d][0] && middle < domain[d][1] {
                axis.push(middle);
            }
        }
        axis.sort_by(f64::total_cmp);
        axis.dedup();
        if axis.len() > MAX_CHECK_STATIONS {
            return Err(KernelError::TooLarge("subdivision rechart validation budget".into()));
        }
    }
    Ok(result)
}

struct Evaluator {
    surface: SurfaceEval,
    starts: Starts,
}
impl Evaluator {
    fn new(surface: &NurbsSurface) -> Result<Self> {
        Ok(Self { surface: SurfaceEval::new(&crate::surfaces::spline_surface(surface)).map_err(failed)?, starts: Starts::default() })
    }
    fn nearest(&self, point: Vec3) -> Option<([f64; 2], Vec3)> {
        let uv = self.surface.project_with(&self.starts, point, None, f64::INFINITY)?;
        Some((uv, self.surface.point(uv)?))
    }
}

fn measure(
    source: &NurbsSurface,
    source_eval: &Evaluator,
    wire: &[[f64; 2]],
    domain: [[f64; 2]; 2],
    fill: &IsoFill,
) -> Result<(f64, Vec<(f64, Vec3)>)> {
    let fitted = fill.patches.iter().map(|patch| Evaluator::new(&patch.surface)).collect::<Result<Vec<_>>>()?;
    let mut misses = Vec::new();
    let mut worst = 0.0_f64;
    let mut remember = |distance: f64, target: Vec3| -> Result<()> {
        if !distance.is_finite() {
            return Err(failed("a validation point has no finite projection"));
        }
        worst = worst.max(distance);
        misses.push((distance, target));
        Ok(())
    };
    let [us, vs] = stations(source, domain)?;
    for &u in &us {
        for &v in &vs {
            if !inside(wire, [u, v]) {
                continue;
            }
            let point = source.eval(u, v).map_err(failed)?;
            let distance = fitted.iter().filter_map(|s| s.nearest(point).map(|(_, q)| point.dist(q))).fold(f64::INFINITY, f64::min);
            remember(distance, point)?;
        }
    }
    for patch in &fill.patches {
        let [us, vs] = stations(&patch.surface, patch.surface.domain())?;
        for &u in &us {
            for &v in &vs {
                let point = patch.surface.eval(u, v).map_err(failed)?;
                let (uv, target) = source_eval.nearest(point).ok_or_else(|| failed("the fitted chart has no source projection"))?;
                if !inside(wire, uv) {
                    return Err(failed("the fitted chart leaves its source trim"));
                }
                remember(point.dist(target), target)?;
            }
        }
    }
    misses.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok((worst, misses))
}

pub(super) fn fit(
    source: &NurbsSurface,
    edges: &[IsoEdge],
    wire: &[[f64; 2]],
    boundary_uv: &dyn Fn(usize, f64) -> Option<[f64; 2]>,
    tolerance: f64,
) -> Result<(IsoFill, f64)> {
    if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > 1.0 || !(3..=16).contains(&wire.len()) {
        return Err(failed("a finite positive error budget and a simple bounded trim are required"));
    }
    let domain: [[f64; 2]; 2] = std::array::from_fn(|d| {
        [wire.iter().map(|p| p[d]).fold(f64::INFINITY, f64::min), wire.iter().map(|p| p[d]).fold(f64::NEG_INFINITY, f64::max)]
    });
    let source_eval = Evaluator::new(source)?;
    let source_derivs = EvalSurface::from(source);
    let mut guides = Vec::new();
    for i in 1..8 {
        for j in 1..8 {
            let [u, v] =
                [domain[0][0] + (domain[0][1] - domain[0][0]) * i as f64 / 8.0, domain[1][0] + (domain[1][1] - domain[1][0]) * j as f64 / 8.0];
            if inside(wire, [u, v]) {
                guides.push(source.eval(u, v).map_err(failed)?);
            }
        }
    }
    let mut worst = f64::INFINITY;
    for _ in 0..ROUNDS {
        let mut across = |edge: usize, ts: &[f64]| -> std::result::Result<Vec<Option<Across>>, String> {
            Ok(ts
                .iter()
                .map(|&t| {
                    let [u, v] = boundary_uv(edge, t)?;
                    let (_, du, dv) = source_derivs.derivs(u, v)?;
                    let normal = du.normalized()?.cross(dv.normalized()?).normalized()?;
                    let tangent = edges.get(edge)?.tangent(t)?;
                    Some(Across { leave: normal.cross(tangent), normal, bend: None, jet: None })
                })
                .collect())
        };
        let spec = IsoSpec {
            tol: tolerance * 0.2,
            angle_tol: super::PATCH_ANGLE_TOL_DEG.to_radians(),
            crown: 0.0,
            corner_turn: 1.0_f64.to_radians(),
            points: &guides,
        };
        let fill = iso_fill(edges, &mut across, &spec).map_err(failed)?;
        let (deviation, misses) = measure(source, &source_eval, wire, domain, &fill)?;
        worst = deviation;
        if deviation <= tolerance {
            return Ok((fill, deviation));
        }
        let previous = guides.len();
        for (_, point) in misses {
            if guides.len() >= MAX_FILL_POINTS {
                break;
            }
            if guides.iter().all(|p| p.dist(point) > tolerance * 1e-4) {
                guides.push(point);
            }
        }
        if guides.len() == previous {
            break;
        }
    }
    Err(failed(format!("sampled atlas distance {worst:.6} mm exceeds its {tolerance:.6} mm error budget")))
}
