//! Tensor B-spline fitting of original logical subdivision patches.
//! Reported deviation is a sampled check on the next refinement level, not a bound.
use super::{Cage, Result, bad};
use crate::{
    Vec3,
    bspline::{CurveN, NurbsSurface},
};
use std::collections::{BTreeMap, BTreeSet};

mod regular;
pub(super) mod root_grid;

#[derive(Clone, Debug)]
pub struct Fit {
    pub root_basis: Option<RootBasis>,
    pub patches: Vec<NurbsSurface>,
    pub patch_roots: Vec<usize>,
    pub sampled_deviation: f64,
    pub subdivision_level: usize,
}

/// Fixed dyadic interpolation stations for linear control-response calculations.
/// Every neighboring root uses the same symmetric stations, including on reversal.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootBasis {
    pub level: usize,
    pub indices: Vec<usize>,
}
impl RootBasis {
    pub fn validate(&self) -> Result<()> {
        self.parameters().map(|_| ())
    }
    fn parameters(&self) -> Result<Vec<f64>> {
        if !(2..=6).contains(&self.level) || !(4..=33).contains(&self.indices.len()) {
            return Err(bad("root interpolation basis budget exceeded"));
        }
        let span = 1usize << self.level;
        if self.indices.first() != Some(&0)
            || self.indices.last() != Some(&span)
            || self.indices.windows(2).any(|p| p[0] >= p[1])
            || self.indices.iter().any(|i| *i > span || self.indices.binary_search(&(span - i)).is_err())
        {
            return Err(bad("root interpolation requires distinct symmetric dyadic stations"));
        }
        Ok(self.indices.iter().map(|i| *i as f64 / span as f64).collect())
    }
    /// Fit at fixed stations, without a tolerance check. This is linear in the controls
    /// and can also be applied to scalar influence cages packed into X/Y/Z coordinates.
    pub fn fit(&self, cage: &Cage) -> Result<Vec<NurbsSurface>> {
        let parameters = self.parameters()?;
        let width = (1usize << self.level) + 1;
        let exact = regular::patches(cage)?;
        cage.patch_grids(self.level)?
            .iter()
            .enumerate()
            .map(|(root, grid)| {
                if let Some(Some(patch)) = exact.get(root) {
                    return Ok(patch.clone());
                }
                let mut samples = Vec::with_capacity(self.indices.len() * self.indices.len());
                for &i in &self.indices {
                    for &j in &self.indices {
                        samples.push(*grid.get(i * width + j).ok_or_else(|| bad("invalid fixed root sample"))?);
                    }
                }
                fit_grid_parameters(&samples, &parameters)
            })
            .collect()
    }
    /// Independently validate the fitted geometry on the next subdivision grid.
    pub fn fit_checked(&self, cage: &Cage, tolerance: f64) -> Result<Fit> {
        if !tolerance.is_finite() || !(1e-6..=1.0).contains(&tolerance) {
            return Err(bad("CAD fit tolerance must be 0.000001..1 mm"));
        }
        let patches = self.fit(cage)?;
        let fine = cage.patch_grids(self.level + 1)?;
        let width = (1usize << (self.level + 1)) + 1;
        let mut deviation = 0.0_f64;
        for (patch, grid) in patches.iter().zip(fine) {
            for i in 0..width {
                for j in 0..width {
                    let point = patch.eval(i as f64 / (width - 1) as f64, j as f64 / (width - 1) as f64).map_err(|e| bad(&e.to_string()))?;
                    deviation = deviation.max(point.dist(*grid.get(i * width + j).ok_or_else(|| bad("invalid fixed root validation sample"))?));
                }
            }
        }
        if deviation > tolerance {
            return Err(bad(&format!("fixed root sampled deviation {deviation:.6} mm exceeds {tolerance:.6} mm")));
        }
        Ok(Fit {
            root_basis: Some(self.clone()),
            patch_roots: (0..patches.len()).collect(),
            patches,
            sampled_deviation: deviation,
            subdivision_level: self.level,
        })
    }
}

impl Cage {
    /// Limit positions, using closed masks for smooth/fully sharp corners and bounded
    /// local refinement for semi-sharp corners and darts.
    pub fn limit_vertices(&self) -> Result<Vec<Vec3>> {
        if self.faces.iter().any(|f| f.len() != 4) {
            return Ok(self.step()?.limit_vertices()?.into_iter().take(self.points.len()).collect());
        }
        let topology = self.topology()?;
        let sharp_edges: BTreeSet<_> = self.edge_creases.iter().filter(|c| c.sharpness >= 10.0).map(|c| super::key(c.edge[0], c.edge[1])).collect();
        let sharp_vertices: BTreeSet<_> = self.vertex_creases.iter().filter(|c| c.sharpness >= 10.0).map(|c| c.vertex).collect();
        let edge_values: BTreeMap<_, _> = self.edge_creases.iter().map(|c| (super::key(c.edge[0], c.edge[1]), c.sharpness)).collect();
        let vertex_values: BTreeMap<_, _> = self.vertex_creases.iter().map(|c| (c.vertex, c.sharpness)).collect();
        let mut out = Vec::with_capacity(self.points.len());
        for (v, &p) in self.points.iter().enumerate() {
            let edges = topology.incident.get(v).ok_or_else(|| bad("invalid vertex"))?;
            let mut boundary = Vec::new();
            let mut neighbors = Vec3::ZERO;
            let mut faces = BTreeSet::new();
            let mut sharp_neighbors = Vec::new();
            let mut has_finite = vertex_values.get(&v).is_some_and(|s| *s > 0.0 && *s < 10.0);
            let mut positive_edges = 0;
            for &ei in edges {
                let edge = topology.edges.get(ei).ok_or_else(|| bad("invalid edge"))?;
                let q = self.point(if edge.a == v { edge.b } else { edge.a })?;
                let s = if edge.faces.len() == 1 { 10.0 } else { edge_values.get(&super::key(edge.a, edge.b)).copied().unwrap_or(0.0) };
                if s > 0.0 {
                    positive_edges += 1;
                    if s < 10.0 {
                        has_finite = true;
                    }
                }
                neighbors += q;
                faces.extend(edge.faces.iter().copied());
                if edge.faces.len() == 1 {
                    boundary.push(q);
                }
                if edge.faces.len() == 1 || sharp_edges.contains(&super::key(edge.a, edge.b)) {
                    sharp_neighbors.push(q);
                }
            }
            if has_finite || positive_edges == 1 {
                out.push(self.corner_limit(v)?);
                continue;
            }
            if sharp_neighbors.len() > 2 || sharp_vertices.contains(&v) {
                out.push(p);
                continue;
            }
            if sharp_neighbors.len() == 2 {
                out.push((p * 4.0 + sharp_neighbors[0] + sharp_neighbors[1]) / 6.0);
                continue;
            }
            if boundary.len() == 2 {
                out.push((p * 4.0 + boundary[0] + boundary[1]) / 6.0);
                continue;
            }
            let mut diagonals = Vec3::ZERO;
            for f in faces {
                let face = self.faces.get(f).ok_or_else(|| bad("invalid face"))?;
                let corner = face.iter().position(|&i| i == v).ok_or_else(|| bad("vertex is not on its face"))?;
                diagonals += self.point(face[(corner + 2) % 4])?;
            }
            let n = edges.len() as f64;
            out.push((p * (n * n) + neighbors * 4.0 + diagonals) / (n * (n + 5.0)));
        }
        Ok(out)
    }

    // Refining only the incident fan preserves exactly the next incident fan: its
    // vertices are the old corner, radial edge points and incident face centres.
    // Outer vertices are discarded before their artificial boundary rules can reach it.
    fn corner_fan(&self, vertex: usize) -> Result<(Cage, usize)> {
        self.point(vertex)?;
        let faces: Vec<_> = self.faces.iter().filter(|f| f.contains(&vertex)).cloned().collect();
        let used: BTreeSet<_> = faces.iter().flatten().copied().collect();
        if used.len() > 256 || faces.is_empty() {
            return Err(bad("corner fan budget exceeded"));
        }
        let ids: BTreeMap<_, _> = used.iter().enumerate().map(|(i, &v)| (v, i)).collect();
        let points = used.into_iter().map(|v| self.point(v)).collect::<Result<Vec<_>>>()?;
        let faces = faces.into_iter().map(|f| f.into_iter().map(|v| ids[&v]).collect()).collect();
        let edge_creases = self
            .edge_creases
            .iter()
            .filter(|e| e.edge.contains(&vertex))
            .map(|e| super::EdgeCrease { edge: e.edge.map(|v| ids[&v]), sharpness: e.sharpness })
            .collect();
        let vertex_creases = self
            .vertex_creases
            .iter()
            .filter(|v| v.vertex == vertex)
            .map(|v| super::VertexCrease { vertex: ids[&vertex], sharpness: v.sharpness })
            .collect();
        Ok((Cage { points, faces, edge_creases, vertex_creases }, ids[&vertex]))
    }
    fn corner_limit(&self, vertex: usize) -> Result<Vec3> {
        let (mut fan, mut index) = self.corner_fan(vertex)?;
        let mut previous = fan.point(index)?;
        for step in 0..96 {
            let next = fan.step()?;
            let point = next.point(index)?;
            if step > 12 && point.dist(previous) < 1e-12 * point.len().max(1.0) {
                return Ok(point);
            }
            (fan, index) = next.corner_fan(index)?;
            previous = point;
        }
        Err(bad("crease corner limit did not converge within its evaluation budget"))
    }

    fn patch_grids(&self, level: usize) -> Result<Vec<Vec<Vec3>>> {
        if self.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("CAD fitting currently requires quad root patches"));
        }
        self.refined_budget_at(level, 7, super::MAX_EVAL_FACES)?;
        let n = 1_usize << level;
        let width = n + 1;
        if self.faces.len().saturating_mul(width * width) > 500_000 {
            return Err(bad("patch sample budget exceeded"));
        }
        let tags: Vec<_> = (0..self.faces.len()).map(|f| (f, [[0, 0], [n, 0], [n, n], [0, n]])).collect();
        sample_grids(self.clone(), tags, level, n, self.faces.len())
    }

    pub fn fit_patches(&self, tolerance: f64) -> Result<Fit> {
        if !tolerance.is_finite() || !(1e-6..=1.0).contains(&tolerance) {
            return Err(bad("CAD fit tolerance must be 0.000001..1 mm"));
        }
        if self.faces.iter().any(|f| f.len() != 4) {
            self.validate()?;
            let mut fit = self.step()?.fit_patches(tolerance)?;
            fit.root_basis = None;
            let roots: Vec<_> = self.faces.iter().enumerate().flat_map(|(i, f)| std::iter::repeat_n(i, f.len())).collect();
            for root in &mut fit.patch_roots {
                *root = *roots.get(*root).ok_or_else(|| bad("invalid non-quad patch root"))?;
            }
            fit.subdivision_level += 1;
            return Ok(fit);
        }
        // Prefer a compact common root basis. Tiled fitting remains a bounded
        // fallback for cages whose interior detail needs more tensor controls.
        if let Ok(fit) = self.fit_root_patches(tolerance) {
            return Ok(fit);
        }
        let mut last = f64::INFINITY;
        for level in 2..=6 {
            let grids = self.patch_grids(level)?;
            let fine = match self.patch_grids(level + 1) {
                Ok(g) => g,
                Err(e) => {
                    return Err(bad(&format!("CAD fit validation budget: {e}; best sampled deviation {last:.6} mm, requested {tolerance:.6} mm")));
                }
            };
            let n = (1_usize << level) + 1;
            let check = 2 * n - 1;
            // Keep CAD surface evaluators small at the highest sampling density.
            // Tiles share the same limit samples along every boundary; their physical
            // faces still map to the original editable face through patch_roots.
            let span = (n - 1).min(16);
            let tiles = (n - 1) / span;
            let mut patches = Vec::with_capacity(grids.len() * tiles * tiles);
            let mut patch_roots = Vec::with_capacity(patches.capacity());
            let mut worst = 0.0_f64;
            for (root, (grid, reference)) in grids.iter().zip(fine).enumerate() {
                for u in 0..tiles {
                    for v in 0..tiles {
                        let mut samples = Vec::with_capacity((span + 1) * (span + 1));
                        for i in 0..=span {
                            for j in 0..=span {
                                samples.push(*grid.get((u * span + i) * n + v * span + j).ok_or_else(|| bad("invalid tile sample"))?);
                            }
                        }
                        let patch = fit_grid(&samples, span + 1)?;
                        for i in 0..=2 * span {
                            for j in 0..=2 * span {
                                let p = patch.eval(i as f64 / (2 * span) as f64, j as f64 / (2 * span) as f64).map_err(|e| bad(&e.to_string()))?;
                                let k = (2 * u * span + i) * check + 2 * v * span + j;
                                worst = worst.max(p.dist(*reference.get(k).ok_or_else(|| bad("invalid validation grid"))?));
                            }
                        }
                        patches.push(patch);
                        patch_roots.push(root);
                    }
                }
            }
            if worst <= tolerance {
                return Ok(Fit { root_basis: None, patches, patch_roots, sampled_deviation: worst, subdivision_level: level });
            }
            last = worst;
        }
        Err(bad(&format!("sampled CAD fit deviation {last:.6} mm exceeds {tolerance:.6} mm at the supported refinement budget")))
    }
}

pub(super) fn sample_grids(
    mut cage: Cage,
    mut tags: Vec<(usize, [[usize; 2]; 4])>,
    steps: usize,
    span: usize,
    roots: usize,
) -> Result<Vec<Vec<Vec3>>> {
    let width = span + 1;
    if roots.saturating_mul(width.saturating_mul(width)) > 500_000 {
        return Err(bad("patch sample budget exceeded"));
    }
    for _ in 0..steps {
        let mut next = Vec::with_capacity(tags.len() * 4);
        for (root, uv) in tags {
            let center = [(uv[0][0] + uv[1][0] + uv[2][0] + uv[3][0]) / 4, (uv[0][1] + uv[1][1] + uv[2][1] + uv[3][1]) / 4];
            for i in 0..4 {
                let a = uv[i];
                let b = uv[(i + 1) % 4];
                let c = uv[(i + 3) % 4];
                next.push((root, [a, [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], center, [(a[0] + c[0]) / 2, (a[1] + c[1]) / 2]]));
            }
        }
        cage = cage.step()?;
        tags = next;
    }
    let limit = cage.limit_vertices()?;
    let mut grids = vec![vec![None; width * width]; roots];
    for (face, (root, uv)) in cage.faces.iter().zip(tags) {
        let grid = grids.get_mut(root).ok_or_else(|| bad("invalid root patch"))?;
        for (&vertex, uv) in face.iter().zip(uv) {
            let slot = grid.get_mut(uv[0] * width + uv[1]).ok_or_else(|| bad("patch parameter outside its grid"))?;
            *slot = Some(*limit.get(vertex).ok_or_else(|| bad("invalid limit vertex"))?);
        }
    }
    grids.into_iter().map(|g| g.into_iter().map(|p| p.ok_or_else(|| bad("incomplete patch sample grid"))).collect()).collect()
}

fn fit_grid(samples: &[Vec3], n: usize) -> Result<NurbsSurface> {
    let parameters: Vec<_> = (0..n).map(|i| i as f64 / (n - 1) as f64).collect();
    fit_grid_parameters(samples, &parameters)
}
fn fit_grid_parameters(samples: &[Vec3], parameters: &[f64]) -> Result<NurbsSurface> {
    let n = parameters.len();
    if n < 4 || samples.len() != n.saturating_mul(n) {
        return Err(bad("a cubic tensor fit requires a complete grid"));
    }
    let mut first = vec![Vec3::ZERO; n * n];
    let mut knots = Vec::new();
    for v in 0..n {
        let row: Vec<_> = (0..n)
            .map(|u| {
                let p = *samples.get(u * n + v).unwrap_or(&Vec3::ZERO);
                [p.x, p.y, p.z]
            })
            .collect();
        let curve = CurveN::<3>::interpolate(parameters, &row, 3).map_err(|e| bad(&e.to_string()))?;
        knots = curve.knots;
        for (u, p) in curve.ctrl.into_iter().enumerate() {
            *first.get_mut(u * n + v).ok_or_else(|| bad("invalid fitted row"))? = Vec3::new(p[0], p[1], p[2]);
        }
    }
    let mut controls = vec![Vec3::ZERO; n * n];
    for u in 0..n {
        let row: Vec<_> = (0..n)
            .map(|v| {
                let p = *first.get(u * n + v).unwrap_or(&Vec3::ZERO);
                [p.x, p.y, p.z]
            })
            .collect();
        let curve = CurveN::<3>::interpolate(parameters, &row, 3).map_err(|e| bad(&e.to_string()))?;
        for (v, p) in curve.ctrl.into_iter().enumerate() {
            *controls.get_mut(u * n + v).ok_or_else(|| bad("invalid fitted column"))? = Vec3::new(p[0], p[1], p[2]);
        }
    }
    // NurbsSurface stores u fastest, while the interpolation scratch grid uses v fastest.
    let mut points = Vec::with_capacity(n * n);
    for v in 0..n {
        for u in 0..n {
            points.push(*controls.get(u * n + v).ok_or_else(|| bad("invalid control grid"))?);
        }
    }
    NurbsSurface::new([3, 3], [knots.clone(), knots], &points, &vec![1.0; n * n]).map_err(|e| bad(&e.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn regular_tensor_patches_match_dense_subdivision_even_beside_an_open_boundary() {
        let mut plane = super::super::primitives::Parameters { kind: "face".into(), dimensions: [2.0; 3], segments: [4; 3], ..Default::default() }
            .build()
            .unwrap();
        for (i, p) in plane.points.iter_mut().enumerate() {
            p.z = 0.2 * p.x * p.y + 0.1 * p.x * p.x + (i % 3) as f64 * 0.07;
        }
        let torus =
            super::super::primitives::Parameters { kind: "torus".into(), radii: [3.0, 0.8], segments: [2; 3], ..Default::default() }.build().unwrap();
        for cage in [plane, torus] {
            let patches = super::regular::patches(&cage).unwrap();
            assert!(patches.iter().any(Option::is_some));
            let grids = cage.patch_grids(5).unwrap();
            let mut count = 0;
            for (patch, grid) in patches.iter().zip(grids) {
                if let Some(patch) = patch {
                    assert_eq!(patch.count(), [4, 4]);
                    for i in 0..33 {
                        for j in 0..33 {
                            let actual = patch.eval(i as f64 / 32.0, j as f64 / 32.0).unwrap();
                            assert!(actual.dist(grid[i * 33 + j]) < 1e-10, "exact regular patch drift: {:?}", actual - grid[i * 33 + j]);
                        }
                    }
                    count += 1;
                }
            }
            assert!(count > 0);
        }
    }
    #[test]
    fn fixed_root_basis_is_linear_and_rechecks_the_geometry_independently() {
        use crate::Vec3;
        let cage = super::super::primitive("box", 2.0, 1).unwrap();
        let fit = cage.fit_root_patches_at(0.02, 3).unwrap();
        let basis = fit.root_basis.as_ref().unwrap();
        assert!(basis.level >= 3);
        let checked = basis.fit_checked(&cage, 0.02).unwrap();
        assert_eq!(checked.patches, fit.patches);
        let transform = |p: Vec3| Vec3::new(1.4 * p.x + 3.0, 0.8 * p.y - 2.0, 1.1 * p.z + 4.0);
        let mut moved = cage.clone();
        for p in &mut moved.points {
            *p = transform(*p);
        }
        for (a, b) in basis.fit(&moved).unwrap().iter().zip(&fit.patches) {
            for (p, q) in a.homogeneous().iter().zip(b.homogeneous()) {
                assert!(p.project().unwrap().dist(transform(q.project().unwrap())) < 1e-10);
            }
        }
        let mut influence = cage.clone();
        influence.points.fill(Vec3::ZERO);
        influence.points[0].x = 1.0;
        let response = basis.fit(&influence).unwrap();
        let mut pulled = cage.clone();
        pulled.points[0] += Vec3::new(0.3, 0.2, -0.1);
        for ((a, b), response) in basis.fit(&pulled).unwrap().iter().zip(&fit.patches).zip(response) {
            for ((p, q), r) in a.homogeneous().iter().zip(b.homogeneous()).zip(response.homogeneous()) {
                let expected = q.project().unwrap() + Vec3::new(0.3, 0.2, -0.1) * r.project().unwrap().x;
                assert!(p.project().unwrap().dist(expected) < 1e-10);
            }
        }
        let encoded = serde_json::to_string(basis).unwrap();
        assert_eq!(serde_json::from_str::<super::RootBasis>(&encoded).unwrap(), *basis);
        for invalid in [
            super::RootBasis { level: usize::MAX, indices: vec![] },
            super::RootBasis { level: 3, indices: vec![0, 1, 4, 9] },
            super::RootBasis { level: 3, indices: vec![0, 2, 2, 8] },
        ] {
            assert!(invalid.fit(&cage).is_err());
        }
        assert!(cage.fit_root_patches_at(0.02, usize::MAX).is_err());
    }
    #[test]
    fn root_fits_keep_one_surface_per_face_and_validate_between_samples() {
        let c = super::super::primitive("box", 40.0, 2).unwrap();
        for cage in [c.clone(), c.extrude_faces(&[0], crate::Vec3::Z * 8.0).unwrap(), c.extrude_faces(&[0], crate::Vec3::X * 8.0).unwrap()] {
            let fit = cage.fit_root_patches(0.01).unwrap();
            assert_eq!(fit.patches.len(), cage.faces.len());
            assert!(fit.sampled_deviation <= 0.01);
            assert!(fit.patches.iter().all(|p| p.count().iter().all(|&n| n <= 33)));
            eprintln!(
                "root fit: {} faces, level{}, controls{:?}, deviation{}",
                fit.patches.len(),
                fit.subdivision_level,
                fit.patches.iter().map(|p| p.count()).max(),
                fit.sampled_deviation
            );
        }
    }
    #[test]
    fn smooth_cube_limit_and_shared_fitted_edges() {
        let c = super::super::primitive("box", 2.0, 1).unwrap();
        for p in c.limit_vertices().unwrap() {
            assert!((p.x.abs() - 0.5).abs() < 1e-12);
            assert!((p.y.abs() - 0.5).abs() < 1e-12);
            assert!((p.z.abs() - 0.5).abs() < 1e-12);
        }
        let fit = c.fit_patches(0.02).unwrap();
        assert_eq!(fit.patches.len(), 6);
        assert!(fit.sampled_deviation <= 0.02);
        for (fi, f) in c.faces.iter().enumerate() {
            for i in 0..4 {
                let (a, b) = (f[i], f[(i + 1) % 4]);
                let other = c
                    .faces
                    .iter()
                    .enumerate()
                    .find_map(|(fj, g)| if fj == fi { None } else { (0..4).find(|&j| g[j] == b && g[(j + 1) % 4] == a).map(|j| (fj, j)) })
                    .unwrap();
                let uv = |edge, t| match edge {
                    0 => (t, 0.),
                    1 => (1., t),
                    2 => (1. - t, 1.),
                    _ => (0., 1. - t),
                };
                for k in 0..=20 {
                    let t = k as f64 / 20.;
                    let (u, v) = uv(i, t);
                    let (s, w) = uv(other.1, 1. - t);
                    assert!(fit.patches[fi].eval(u, v).unwrap().dist(fit.patches[other.0].eval(s, w).unwrap()) < 1e-10);
                }
            }
        }
    }
}
