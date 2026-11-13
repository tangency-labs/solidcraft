//! How far a mesh strays from its surface, measured rather than assumed.

use crate::surface::{lerp, NurbsSurface};
use crate::{MeshError, TrimmedMesh};

/// What [`deviation`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deviation {
    /// The largest distance from a sample to the surface, in model units.
    pub max: f64,
    /// The mean over triangles of each triangle's largest sample distance.
    pub mean: f64,
    /// Triangles with a sample further than the tolerance from the surface.
    pub over: usize,
    /// Triangles measured.
    pub triangles: usize,
    /// The index of the worst triangle.
    pub worst: usize,
}

/// Barycentric samples: the centroid, the three edge midpoints, and three points between the
/// centroid and the corners.
const SAMPLES: [[f64; 3]; 7] = [
    [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
    [0.5, 0.5, 0.0],
    [0.0, 0.5, 0.5],
    [0.5, 0.0, 0.5],
    [2.0 / 3.0, 1.0 / 6.0, 1.0 / 6.0],
    [1.0 / 6.0, 2.0 / 3.0, 1.0 / 6.0],
    [1.0 / 6.0, 1.0 / 6.0, 2.0 / 3.0],
];

/// Every triangle measured at seven points: each point's (u, v) blended from its corners', a
/// Gauss-Newton search from there to the foot on the surface ([`NurbsSurface::foot_near`]), and
/// the distance. Each distance is to a real surface point, so it bounds the true one from above.
///
/// Edge midpoints on the face's boundary measure the caller's edge polyline, not the grid.
pub fn deviation(s: &NurbsSurface, mesh: &TrimmedMesh, tol: f64) -> Result<Deviation, MeshError> {
    let mut out = Deviation {
        max: 0.0,
        mean: 0.0,
        over: 0,
        triangles: 0,
        worst: 0,
    };
    let mut sum = 0.0;
    for (t, ids) in mesh.triangles.iter().enumerate() {
        let corner = |k: usize| -> Result<([f64; 3], [f64; 2]), MeshError> {
            let id = *ids.get(k).ok_or(MeshError::Internal)? as usize;
            Ok((
                *mesh.positions.get(id).ok_or(MeshError::Internal)?,
                *mesh.uvs.get(id).ok_or(MeshError::Internal)?,
            ))
        };
        let [(pa, ua), (pb, ub), (pc, uc)] = [corner(0)?, corner(1)?, corner(2)?];
        let mut worst = 0.0_f64;
        for [a, b, c] in SAMPLES {
            // a pa + b pb + c pc, as two lerps.
            let q = lerp(lerp(pa, pb, b / (a + b)), pc, c);
            let seed = [
                a * ua[0] + b * ub[0] + c * uc[0],
                a * ua[1] + b * ub[1] + c * uc[1],
            ];
            let (_, d) = s.foot_near(q, seed)?;
            worst = worst.max(d);
        }
        sum += worst;
        out.triangles += 1;
        if worst > tol {
            out.over += 1;
        }
        if worst > out.max {
            out.max = worst;
            out.worst = t;
        }
    }
    if out.triangles > 0 {
        out.mean = sum / out.triangles as f64;
    }
    Ok(out)
}
