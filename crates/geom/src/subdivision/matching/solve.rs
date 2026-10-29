//! Minimum-change, damped constraint solve over the bounded nearby control set.
use super::*;

type Response = Vec<Vec<[f64; 6]>>;
fn component(v: Vec3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}
pub(super) fn responses(cage: &Cage, order: usize, basis: &RootBasis, stations: &[Station], controls: &[usize]) -> Result<Response> {
    let mut out = vec![vec![[0.0; 6]; controls.len()]; stations.len()];
    for (batch, chunk) in controls.chunks(3).enumerate() {
        let mut scalar = cage.clone();
        scalar.points.fill(Vec3::ZERO);
        for (i, &id) in chunk.iter().enumerate() {
            *scalar.points.get_mut(id).ok_or_else(|| bad("invalid Match response control"))? = match i {
                0 => Vec3::X,
                1 => Vec3::Y,
                _ => Vec3::Z,
            };
        }
        let values = jets(&scalar, order, basis, stations)?;
        for (row, jet) in out.iter_mut().zip(values) {
            for i in 0..chunk.len() {
                *row.get_mut(3 * batch + i).ok_or_else(|| bad("invalid Match response column"))? =
                    [jet.point, jet.du, jet.dv, jet.duu, jet.duv, jet.dvv].map(|v| component(v, i));
            }
        }
    }
    Ok(out)
}
struct Rows {
    coefficients: Vec<Vec<f64>>,
    rhs: Vec<f64>,
    columns: usize,
}
impl Rows {
    fn add(&mut self, mut row: Vec<f64>, mut rhs: f64, weight: f64) -> Result<()> {
        if row.len() != self.columns || row.iter().any(|v| !v.is_finite()) || !rhs.is_finite() || self.rhs.len() >= 2304 {
            return Err(bad("Match constraint budget or numeric range exceeded"));
        }
        let norm = row.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm < 1e-14 {
            if rhs.abs() > 1e-9 {
                return Err(bad("the nearby controls cannot supply the requested Match condition"));
            }
            return Ok(());
        }
        for v in &mut row {
            *v *= weight / norm;
        }
        rhs *= weight / norm;
        self.coefficients.push(row);
        self.rhs.push(rhs);
        Ok(())
    }
    fn xyz(&mut self, coefficients: &[f64], rhs: Vec3, weight: f64) -> Result<()> {
        for axis in 0..3 {
            let mut row = vec![0.0; self.columns];
            for (i, &v) in coefficients.iter().enumerate() {
                *row.get_mut(3 * i + axis).ok_or_else(|| bad("invalid Match constraint column"))? = v;
            }
            self.add(row, component(rhs, axis), weight)?;
        }
        Ok(())
    }
    fn solve(self) -> Result<Vec<Vec3>> {
        if self.rhs.is_empty() || !self.columns.is_multiple_of(3) || self.columns > 768 {
            return Err(bad("Match has no usable constraints"));
        }
        // A Aᵀ + λI is positive definite even when position/tangent/curvature
        // constraints overlap. Solving in row space gives the minimum control change.
        let column_space = self.columns < self.rhs.len();
        let gram = if column_space {
            (0..self.columns).map(|i| self.coefficients.iter().map(|r| r.get(i).copied().unwrap_or(0.0)).collect()).collect::<Vec<Vec<f64>>>()
        } else {
            self.coefficients.clone()
        };
        let n = gram.len();
        let mut lower = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..=i {
                let mut value = gram[i].iter().zip(&gram[j]).map(|(a, b)| a * b).sum::<f64>();
                if i == j {
                    value += 1e-10;
                }
                value -= lower[i].iter().zip(&lower[j]).take(j).map(|(a, b)| a * b).sum::<f64>();
                if i == j {
                    if !value.is_finite() || value <= 1e-18 {
                        return Err(bad("Match constraints are numerically ill-conditioned"));
                    }
                    lower[i][j] = value.sqrt();
                } else {
                    lower[i][j] = value / lower[j][j];
                }
            }
        }
        let mut solution =
            if column_space { gram.iter().map(|column| column.iter().zip(&self.rhs).map(|(a, b)| a * b).sum()).collect() } else { self.rhs };
        for i in 0..n {
            for j in 0..i {
                solution[i] -= lower[i][j] * solution[j];
            }
            solution[i] /= lower[i][i];
        }
        for i in (0..n).rev() {
            for j in i + 1..n {
                solution[i] -= lower[j][i] * solution[j];
            }
            solution[i] /= lower[i][i];
        }
        let delta = if column_space {
            solution
        } else {
            let mut delta = vec![0.0; self.columns];
            for (row, value) in self.coefficients.iter().zip(solution) {
                for (out, a) in delta.iter_mut().zip(row) {
                    *out += a * value;
                }
            }
            delta
        };
        if delta.iter().any(|v| !v.is_finite()) {
            return Err(bad("Match produced non-finite control changes"));
        }
        Ok(delta.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect())
    }
}
pub(super) fn step(
    stations: &[Station],
    targets: &[Target],
    original: &[SurfaceJet],
    current: &[SurfaceJet],
    response: &Response,
    config: &Config,
    diagonal: f64,
) -> Result<Vec<Vec3>> {
    let order = config.order;
    let columns = response.first().ok_or_else(|| bad("missing Match response"))?.len() * 3;
    let mut rows = Rows { coefficients: Vec::new(), rhs: Vec::new(), columns };
    for ((((station, target), original), jet), response) in stations.iter().zip(targets).zip(original).zip(current).zip(response) {
        let position = response.iter().map(|c| c[0]).collect::<Vec<_>>();
        let difference = target.point - jet.point;
        if station.vertex || target.corner {
            rows.xyz(&position, difference, 10.0)?;
        } else {
            // A geometric curve match has two positional conditions. Locking its
            // tangent coordinate would oppose the bound vertices when a support
            // changes size and force slow, artificial parameter relaxation.
            let tangent = target.tangent.normalized().ok_or_else(|| bad("Match support has no boundary tangent"))?;
            let u = tangent.any_perp().normalized().ok_or_else(|| bad("Match support has no normal plane"))?;
            let v = tangent.cross(u);
            for normal in [u, v] {
                let row = position.iter().flat_map(|&c| [normal.x * c, normal.y * c, normal.z * c]).collect();
                rows.add(row, difference.dot(normal), 10.0)?;
            }
        }
        if order == 0 || !station.differential || target.corner {
            continue;
        }
        let normal = target.normal.normalized().ok_or_else(|| bad("Match support has no normal"))?;
        let tangent = (target.tangent * station.direction).normalized().ok_or_else(|| bad("Match support has no boundary tangent"))?;
        let leave = normal.cross(tangent).normalized().ok_or_else(|| bad("Match support has no transverse tangent"))?;
        let [u, v] = station.across;
        let before = original.du * u + original.dv * v;
        let scale = before.len();
        if scale < 1e-8 {
            return Err(bad("the source opening has no transverse extent"));
        }
        let projected = before - normal * before.dot(normal);
        let desired = if projected.dot(leave) < scale * 0.25 { leave * scale } else { projected };
        let across = jet.du * u + jet.dv * v;
        let coefficients = response.iter().map(|c| c[1] * u + c[2] * v).collect::<Vec<_>>();
        let mut normal_row = Vec::with_capacity(columns);
        for &c in &coefficients {
            normal_row.extend([normal.x * c, normal.y * c, normal.z * c]);
        }
        rows.add(normal_row, -normal.dot(across), 1.0)?;
        // The normal condition is geometric. The in-plane direction/magnitude is
        // only a soft preference that keeps a usable transverse extent; enforcing
        // it exactly would overconstrain the boundary's independent curve fit.
        rows.xyz(&coefficients, desired - across, 0.01)?;
        if order < 2 {
            continue;
        }
        let sign = if target.support.normal().ok_or_else(|| bad("singular Match support"))?.dot(normal) < 0.0 { -1.0 } else { 1.0 };
        let transverse = normal.cross(tangent).normalized().ok_or_else(|| bad("invalid support tangent plane"))?;
        let curvature_scale = [(tangent, tangent), (tangent, transverse), (transverse, transverse)]
            .into_iter()
            .map(|(a, b)| target.support.curvature(a, b).map(f64::abs).unwrap_or(0.0))
            .fold(1.0 / diagonal.max(1e-6), f64::max);
        for (field, a, b, actual) in [(3, jet.du, jet.du, jet.duu), (4, jet.du, jet.dv, jet.duv), (5, jet.dv, jet.dv, jet.dvv)] {
            let desired = target.support.curvature(a, b).ok_or_else(|| bad("support curvature is unavailable"))? * sign;
            let residual = desired - normal.dot(actual);
            // An already qualified curvature component must not amplify tiny
            // fitting noise in a natural boundary's nearly-null second derivative.
            if residual.abs() <= config.tolerances.curvature_relative * 0.25 * curvature_scale * (a.len() * b.len()).max(1e-12) {
                continue;
            }
            let mut row = Vec::with_capacity(columns);
            for c in response {
                row.extend([normal.x * c[field], normal.y * c[field], normal.z * c[field]]);
            }
            rows.add(row, residual, 1.0)?;
        }
    }
    rows.solve()
}
