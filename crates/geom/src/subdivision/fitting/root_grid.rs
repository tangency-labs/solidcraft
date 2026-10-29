//! Common symmetric parameters keep reversed neighboring boundary curves identical.
use super::*;

impl Cage {
    /// One fitted tensor surface per quad source face. Validation uses the next
    /// subdivision grid, including samples between interpolation parameters.
    pub fn fit_root_patches(&self, tolerance: f64) -> Result<Fit> {
        self.fit_root_patches_at(tolerance, 2)
    }
    /// Keep at least the requested sampling level for derivative-sensitive fitting.
    pub fn fit_root_patches_at(&self, tolerance: f64, minimum_level: usize) -> Result<Fit> {
        if !tolerance.is_finite() || !(1e-6..=1.0).contains(&tolerance) {
            return Err(bad("CAD fit tolerance must be 0.000001..1 mm"));
        }
        if self.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("root surface fitting requires quads"));
        }
        if !(2..=6).contains(&minimum_level) {
            return Err(bad("root fitting requires a sampling level in 2..6"));
        }
        let mut last = f64::INFINITY;
        let exact = super::regular::patches(self)?;
        for level in minimum_level..=6 {
            let grids = self.patch_grids(level)?;
            let fine = self.patch_grids(level + 1)?;
            if let Some(fit) = fit(&grids, &fine, level, tolerance, &mut last, Some(&exact))? {
                return Ok(fit);
            }
        }
        Err(bad(&format!("root surface sampled deviation {last:.6} mm exceeds {tolerance:.6} mm")))
    }
}

pub(in crate::subdivision) fn fit(
    grids: &[Vec<Vec3>],
    fine: &[Vec<Vec3>],
    level: usize,
    tolerance: f64,
    last: &mut f64,
    exact: Option<&[Option<NurbsSurface>]>,
) -> Result<Option<Fit>> {
    let span = 1_usize << level;
    let width = span + 1;
    let check = 2 * span + 1;
    let mut indices: BTreeSet<usize> = (0..=span.min(8)).map(|i| i * span / span.min(8)).collect();
    for i in [1, 2, 4] {
        if i < span / 2 {
            indices.extend([i, span - i]);
        }
    }
    loop {
        let parameters: Vec<_> = indices.iter().map(|&i| i as f64 / span as f64).collect();
        let mut patches = Vec::with_capacity(grids.len());
        let mut worst = 0.0;
        let mut at = [0, 0];
        for (root, (grid, reference)) in grids.iter().zip(fine).enumerate() {
            let mut samples = Vec::with_capacity(indices.len() * indices.len());
            for &i in &indices {
                for &j in &indices {
                    samples.push(*grid.get(i * width + j).ok_or_else(|| bad("invalid root sample grid"))?);
                }
            }
            let patch =
                if let Some(Some(patch)) = exact.and_then(|p| p.get(root)) { patch.clone() } else { fit_grid_parameters(&samples, &parameters)? };
            for i in 0..check {
                for j in 0..check {
                    let p = patch.eval(i as f64 / (check - 1) as f64, j as f64 / (check - 1) as f64).map_err(|e| bad(&e.to_string()))?;
                    let deviation = p.dist(*reference.get(i * check + j).ok_or_else(|| bad("invalid root validation grid"))?);
                    if deviation > worst {
                        worst = deviation;
                        at = [i, j];
                    }
                }
            }
            patches.push(patch);
        }
        *last = worst;
        if worst <= tolerance {
            return Ok(Some(Fit {
                root_basis: Some(RootBasis { level, indices: indices.iter().copied().collect() }),
                patch_roots: (0..patches.len()).collect(),
                patches,
                sampled_deviation: worst,
                subdivision_level: level,
            }));
        }
        let previous = indices.len();
        for i in at {
            // A worst sample between source nodes may need both adjacent knots.
            for k in [i / 2, i.div_ceil(2)] {
                indices.extend([k, span - k]);
            }
        }
        if indices.len() == previous
            && let Some(next) =
                (1..span).filter(|i| !indices.contains(i)).min_by_key(|&i| at.iter().map(|&x| (2 * i).abs_diff(x)).min().unwrap_or(usize::MAX))
        {
            // The error may lie between two existing samples. Add nearby
            // constraints that can still change that first/last cubic span.
            indices.extend([next, span - next]);
        }
        if indices.len() == previous || indices.len() > 33 {
            return Ok(None);
        }
    }
}
