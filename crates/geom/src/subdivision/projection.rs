//! Least-squares line/plane projection for Form controls or evaluated surface points.
use super::{MAX_POINTS, Result, bad};
use crate::Vec3;

#[derive(Clone, Copy)]
pub enum Projection {
    Line,
    Plane,
}

/// With no direction, use the principal line or least-squares plane. A supplied direction
/// is the line direction or plane normal; an omitted origin uses the selection centroid.
pub fn project(points: &[Vec3], tool: Projection, direction: Option<Vec3>, origin: Option<Vec3>) -> Result<Vec<Vec3>> {
    let minimum = match tool {
        Projection::Line => 2,
        Projection::Plane => 3,
    };
    if points.len() < minimum || points.len() > MAX_POINTS || points.iter().any(|p| !bounded(*p)) || origin.is_some_and(|p| !bounded(p)) {
        return Err(bad("select enough finite points within the Form coordinate budget"));
    }
    let center = points.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / points.len() as f64;
    let axis = match direction {
        Some(d) => {
            if !d.is_finite() {
                return Err(bad("projection direction must be finite and nonzero"));
            }
            d.normalized().ok_or_else(|| bad("projection direction must be finite and nonzero"))?
        }
        None => principal_axis(points, center, tool)?,
    };
    let origin = origin.unwrap_or(center);
    let out: Vec<_> = points
        .iter()
        .map(|p| {
            let component = axis * (*p - origin).dot(axis);
            match tool {
                Projection::Line => origin + component,
                Projection::Plane => *p - component,
            }
        })
        .collect();
    if out.iter().any(|p| !bounded(*p)) {
        return Err(bad("projected points exceed the Form coordinate budget"));
    }
    Ok(out)
}

fn bounded(p: Vec3) -> bool {
    p.is_finite() && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= 100_000.0
}

fn principal_axis(points: &[Vec3], center: Vec3, tool: Projection) -> Result<Vec3> {
    let mut a = [[0.0; 3]; 3];
    let scale = points.iter().map(|p| (*p - center).len()).fold(0.0_f64, f64::max);
    if scale < 1e-10 {
        return Err(bad("coincident points do not define a best-fit direction"));
    }
    for p in points {
        let q = (*p - center) / scale;
        let q = [q.x, q.y, q.z];
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += q[i] * q[j] / points.len() as f64;
            }
        }
    }
    // Bounded Jacobi diagonalization of the symmetric 3x3 covariance. Columns of v are
    // eigenvectors; using all axes avoids the starting-vector bias of power iteration.
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let (p, q) = [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .max_by(|&(p, q), &(r, s)| a[p][q].abs().total_cmp(&a[r][s].abs()))
            .ok_or_else(|| bad("missing covariance axes"))?;
        if a[p][q].abs() <= 1e-14 {
            break;
        }
        let angle = 0.5 * (2.0 * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (sn, cs) = angle.sin_cos();
        for row in &mut a {
            let (x, y) = (row[p], row[q]);
            row[p] = cs * x - sn * y;
            row[q] = sn * x + cs * y;
        }
        let (x, y) = (a[p], a[q]);
        a[p] = std::array::from_fn(|j| cs * x[j] - sn * y[j]);
        a[q] = std::array::from_fn(|j| sn * x[j] + cs * y[j]);
        for row in &mut v {
            let (x, y) = (row[p], row[q]);
            row[p] = cs * x - sn * y;
            row[q] = sn * x + cs * y;
        }
    }
    if a[0][1].abs().max(a[0][2].abs()).max(a[1][2].abs()) > 1e-10 {
        return Err(bad("best-fit direction did not converge"));
    }
    let mut order = [0, 1, 2];
    order.sort_by(|&i, &j| a[i][i].total_cmp(&a[j][j]));
    let (i, neighbor) = match tool {
        Projection::Line => (order[2], order[1]),
        Projection::Plane => (order[0], order[1]),
    };
    if (a[i][i] - a[neighbor][neighbor]).abs() < 1e-10 * a[order[2]][order[2]].abs().max(1e-14) {
        return Err(bad("selection has no unique best-fit direction; specify an axis"));
    }
    Vec3::new(v[0][i], v[1][i], v[2][i]).normalized().ok_or_else(|| bad("invalid best-fit direction"))
}
