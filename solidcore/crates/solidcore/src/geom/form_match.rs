//! Numerical constraints for one damped boundary Match iteration.
use crate::eval::{plan::SurfaceJet, vec::VecExt};
use crate::geom::regularized_fit;
use crate::{Result, SolidError, Vec3};

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input("Form Match", reason)
}

/// The source differential directions and whether this station locks position.
#[derive(Clone, Copy)]
pub struct Station {
    /// Across-boundary direction in chart coordinates.
    pub across: [f64; 2],
    /// Whether the target has a tangent-plane condition here.
    pub differential: bool,
    /// Direction sign for the oriented target tangent.
    pub direction: f64,
    /// Whether this station fixes all three positional components.
    pub vertex: bool,
}

/// Exact target coordinates and support differential at one station.
#[derive(Clone, Copy)]
pub struct Target {
    /// Genuine corner without a unique tangent or normal.
    pub corner: bool,
    /// Boundary position.
    pub point: Vec3,
    /// Boundary tangent before orientation.
    pub tangent: Vec3,
    /// Oriented support normal.
    pub normal: Vec3,
    /// Analytic surface derivatives in the target frame.
    pub support: SurfaceJet,
}

/// Reflection partners in the solve's ordered control columns.
pub struct MirrorColumns {
    partners: Vec<usize>,
    axis: usize,
}

impl MirrorColumns {
    /// Create a checked involution with one sign-flipped axis.
    pub fn new(partners: Vec<usize>, axis: usize) -> Result<Self> {
        if axis >= 3
            || partners.len() > 256
            || partners
                .iter()
                .enumerate()
                .any(|(index, &partner)| partners.get(partner) != Some(&index))
        {
            return Err(bad("invalid Match mirror columns"));
        }
        Ok(Self { partners, axis })
    }

    fn project(&self, row: &mut [f64]) -> Result<()> {
        for (i, &partner) in self.partners.iter().enumerate().filter(|&(i, p)| i <= *p) {
            for axis in 0..3 {
                let (a, b) = (3 * i + axis, 3 * partner + axis);
                let sign = if axis == self.axis { -1.0 } else { 1.0 };
                let va = *row
                    .get(a)
                    .ok_or_else(|| bad("invalid Match mirror column"))?;
                let vb = *row
                    .get(b)
                    .ok_or_else(|| bad("invalid Match mirror column"))?;
                let value = (va + sign * vb) * 0.5;
                *row.get_mut(a)
                    .ok_or_else(|| bad("invalid Match mirror column"))? = value;
                *row.get_mut(b)
                    .ok_or_else(|| bad("invalid Match mirror column"))? = sign * value;
            }
        }
        Ok(())
    }
}

fn perpendicular(v: Vec3) -> Option<Vec3> {
    let a = if v.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    v.cross(a).normalized().unwrap_or(Vec3::Z).normalized()
}

struct Rows<'a> {
    coefficients: Vec<Vec<f64>>,
    rhs: Vec<f64>,
    columns: usize,
    mirror: Option<&'a MirrorColumns>,
}
impl Rows<'_> {
    fn add(&mut self, mut row: Vec<f64>, mut rhs: f64, weight: f64) -> Result<()> {
        if row.len() != self.columns
            || row.iter().any(|v| !v.is_finite())
            || !rhs.is_finite()
            || self.rhs.len() >= 2304
        {
            return Err(bad("Match constraint budget or numeric range exceeded"));
        }
        if let Some(mirror) = self.mirror {
            mirror.project(&mut row)?;
        }
        let norm = row.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm < 1e-14 {
            if rhs.abs() > 1e-9 {
                return Err(bad(if self.mirror.is_some() {
                    "the Match support condition conflicts with the Form's symmetry"
                } else {
                    "the nearby controls cannot supply the requested Match condition"
                }));
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
        for (axis, rhs) in rhs.to_array().into_iter().enumerate() {
            let mut row = vec![0.0; self.columns];
            for (i, &v) in coefficients.iter().enumerate() {
                *row.get_mut(3 * i + axis)
                    .ok_or_else(|| bad("invalid Match constraint column"))? = v;
            }
            self.add(row, rhs, weight)?;
        }
        Ok(())
    }
    fn solve(self) -> Result<Vec<Vec3>> {
        if self.rhs.is_empty() {
            return Err(bad("Match has no usable constraints"));
        }
        let mut delta = regularized_fit::solve(&self.coefficients, &self.rhs, self.columns, 1e-10)?;
        if let Some(mirror) = self.mirror {
            mirror.project(&mut delta)?;
        }
        if delta.iter().any(|v| !v.is_finite()) {
            return Err(bad("Match produced non-finite control changes"));
        }
        Ok(delta
            .chunks_exact(3)
            .filter_map(|p| {
                let &[x, y, z] = p else { return None };
                Some(Vec3::new(x, y, z))
            })
            .collect())
    }
}

/// One Match step in the caller's ordered control basis.
///
/// Each response row supplies the point, first, and second derivative
/// influence of one control. Topology and subsequent fit acceptance are
/// handled by the caller.
pub fn step(
    samples: impl ExactSizeIterator<Item = (Station, Target, SurfaceJet, SurfaceJet)>,
    response: &[Vec<[f64; 6]>],
    mirror: Option<&MirrorColumns>,
    order: usize,
    curvature_tolerance: f64,
    diagonal: f64,
) -> Result<Vec<Vec3>> {
    let count = response
        .first()
        .ok_or_else(|| bad("missing Match response"))?
        .len();
    if !(1..=256).contains(&count)
        || samples.len() != response.len()
        || samples.len() > 256
        || response.iter().any(|row| row.len() != count)
        || mirror.is_some_and(|m| m.partners.len() != count)
        || order > 2
        || !curvature_tolerance.is_finite()
        || curvature_tolerance < 0.0
        || !diagonal.is_finite()
        || diagonal < 1e-6
    {
        return Err(bad("invalid Match solve dimensions or tolerances"));
    }
    let columns = count * 3;
    let mut rows = Rows {
        coefficients: Vec::new(),
        rhs: Vec::new(),
        columns,
        mirror,
    };
    for ((station, target, original, jet), response) in samples.zip(response) {
        let position = response
            .iter()
            .map(|&[position, ..]| position)
            .collect::<Vec<_>>();
        let difference = target.point - jet.point;
        if station.vertex || target.corner {
            rows.xyz(&position, difference, 10.0)?;
        } else {
            // A geometric curve match has two positional conditions. Locking its
            // tangent coordinate would oppose the bound vertices when a support
            // changes size and force slow, artificial parameter relaxation.
            let tangent = target
                .tangent
                .normalized()
                .ok_or_else(|| bad("Match support has no boundary tangent"))?;
            let u =
                perpendicular(tangent).ok_or_else(|| bad("Match support has no normal plane"))?;
            let v = tangent.cross(u);
            for normal in [u, v] {
                let row = position
                    .iter()
                    .flat_map(|&c| [normal.x * c, normal.y * c, normal.z * c])
                    .collect();
                rows.add(row, difference.dot(normal), 10.0)?;
            }
        }
        if order == 0 || !station.differential || target.corner {
            continue;
        }
        let normal = target
            .normal
            .normalized()
            .ok_or_else(|| bad("Match support has no normal"))?;
        let tangent = (target.tangent * station.direction)
            .normalized()
            .ok_or_else(|| bad("Match support has no boundary tangent"))?;
        let leave = normal
            .cross(tangent)
            .normalized()
            .ok_or_else(|| bad("Match support has no transverse tangent"))?;
        let [u, v] = station.across;
        let before = original.du * u + original.dv * v;
        let scale = before.len();
        if scale < 1e-8 {
            return Err(bad("the source opening has no transverse extent"));
        }
        let projected = before - normal * before.dot(normal);
        let desired = if projected.dot(leave) < scale * 0.25 {
            leave * scale
        } else {
            projected
        };
        let across = jet.du * u + jet.dv * v;
        let coefficients = response
            .iter()
            .map(|&[_, du, dv, ..]| du * u + dv * v)
            .collect::<Vec<_>>();
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
        let sign = if target
            .support
            .normal()
            .ok_or_else(|| bad("singular Match support"))?
            .dot(normal)
            < 0.0
        {
            -1.0
        } else {
            1.0
        };
        let transverse = normal
            .cross(tangent)
            .normalized()
            .ok_or_else(|| bad("invalid support tangent plane"))?;
        let curvature_scale = [
            (tangent, tangent),
            (tangent, transverse),
            (transverse, transverse),
        ]
        .into_iter()
        .map(|(a, b)| target.support.curvature(a, b).map(f64::abs).unwrap_or(0.0))
        .fold(1.0 / diagonal.max(1e-6), f64::max);
        for (field, a, b, actual) in [
            (3, jet.du, jet.du, jet.duu),
            (4, jet.du, jet.dv, jet.duv),
            (5, jet.dv, jet.dv, jet.dvv),
        ] {
            let desired = target
                .support
                .curvature(a, b)
                .ok_or_else(|| bad("support curvature is unavailable"))?
                * sign;
            let residual = desired - normal.dot(actual);
            // An already qualified curvature component must not amplify tiny
            // fitting noise in a natural boundary's nearly-null second derivative.
            if residual.abs()
                <= curvature_tolerance * 0.25 * curvature_scale * (a.len() * b.len()).max(1e-12)
            {
                continue;
            }
            let mut row = Vec::with_capacity(columns);
            for c in response {
                let c = *c
                    .get(field)
                    .ok_or_else(|| bad("invalid Match derivative column"))?;
                row.extend([normal.x * c, normal.y * c, normal.z * c]);
            }
            rows.add(row, residual, 1.0)?;
        }
    }
    rows.solve()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interior_curve_match_can_slide_along_its_tangent() {
        let jet = SurfaceJet {
            point: Vec3::ZERO,
            du: Vec3::X,
            dv: Vec3::Y,
            duu: Vec3::ZERO,
            duv: Vec3::ZERO,
            dvv: Vec3::ZERO,
        };
        let target = Target {
            corner: false,
            point: Vec3::new(9.0, 3.0, -2.0),
            tangent: Vec3::X,
            normal: Vec3::Z,
            support: jet,
        };
        let station = Station {
            across: [0.0, 1.0],
            differential: false,
            direction: 1.0,
            vertex: false,
        };
        let delta = step(
            std::iter::once((station, target, jet, jet)),
            &[vec![[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]]],
            None,
            0,
            0.01,
            3.0,
        )
        .unwrap();
        assert!(
            matches!(delta.as_slice(), [point] if point.distance(Vec3::new(0.0, 3.0, -2.0)) < 1e-10)
        );

        let station = Station {
            vertex: true,
            ..station
        };
        let mirror = MirrorColumns::new(vec![0], 0).unwrap();
        let incompatible = step(
            std::iter::once((station, target, jet, jet)),
            &[vec![[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]]],
            Some(&mirror),
            0,
            0.01,
            3.0,
        );
        assert!(incompatible.is_err());
    }
}
