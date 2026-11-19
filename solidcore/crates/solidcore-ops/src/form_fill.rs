//! Portable controls for compatible homogeneous Coons blending.
use solidcore::geom::form_fill as kernel;
use solidcore_convert::record_vec3;
use solidcore_record::Vec3;

pub mod quad_cap;
pub mod regularity;

/// Cartesian controls and their separately retained homogeneous weights.
pub struct CoonsNet {
    /// U-varying-fastest controls.
    pub points: Vec<Vec3>,
    /// Positive weights.
    pub weights: Vec<f64>,
    /// Whether any weight differs from one.
    pub rational: bool,
}

/// Blend compatible directed rows without fitting or changing their parameters.
pub fn coons(
    degree: [usize; 2],
    knots: [&[f64]; 2],
    sides: [&[[f64; 4]]; 4],
) -> solidcore::Result<CoonsNet> {
    let net = kernel::coons(degree, knots, sides)?;
    Ok(CoonsNet {
        points: net.points.into_iter().map(record_vec3).collect(),
        weights: net.weights,
        rational: net.rational,
    })
}

pub use kernel::rechart::Tensor;

/// Exact Duffy composition, preserving the directed rational boundary.
pub fn duffy_triangle(
    degree: [usize; 2],
    knots: [&[f64]; 2],
    controls: &[[f64; 4]],
    corners: [usize; 3],
) -> solidcore::Result<Tensor> {
    kernel::rechart::triangle(degree, knots, controls, corners)
}

/// Source chart parameters of an oriented triangle's corner identities.
pub fn triangle_parameters(
    domain: [[f64; 2]; 2],
    corners: [usize; 3],
) -> solidcore::Result<[[f64; 2]; 3]> {
    kernel::rechart::triangle_parameters(domain, corners)
}

/// Immutable validated rational tensor basis; solve coordinates remain separate.
#[derive(Clone, Debug)]
pub struct TensorBasis(kernel::basis::TensorBasis);
impl TensorBasis {
    /// Validate and retain knots and fixed weights once per guided patch.
    pub fn new(
        degree: [usize; 2],
        knots: [Vec<f64>; 2],
        weights: Vec<f64>,
    ) -> solidcore::Result<Self> {
        kernel::basis::TensorBasis::new(degree, knots, weights).map(Self)
    }
    /// Fixed rational weight at a stable local coefficient.
    pub fn weight(&self, i: usize, j: usize) -> Option<f64> {
        self.0.weight(i, j)
    }
    /// One-sided derivative row through guided fifth order.
    pub fn row(
        &self,
        uv: [f64; 2],
        orders: [usize; 2],
        left: [bool; 2],
    ) -> solidcore::Result<Vec<(usize, usize, f64)>> {
        self.0.row(uv, orders, left)
    }
    /// Apply a local row without copying the solve net.
    pub fn apply(&self, points: &[Vec3], row: &[(usize, usize, f64)]) -> solidcore::Result<Vec3> {
        self.0
            .apply(points, row, |point| solidcore_convert::vec3(*point))
            .map(record_vec3)
    }
    /// Directed normal with the guided fitter's existing differential cutoff.
    pub fn normal(&self, points: &[Vec3], uv: [f64; 2]) -> Option<Vec3> {
        self.0
            .normal(points, uv, |point| solidcore_convert::vec3(*point))
            .map(record_vec3)
    }
}
