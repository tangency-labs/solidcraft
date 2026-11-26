//! Exact rational Duffy composition through the shared kernel construction.
use crate::{KernelError, Result};
use solidcraft_geom::bspline::{HPoint, NurbsSurface};

pub(super) fn triangle(source: &NurbsSurface, corners: [usize; 3]) -> Result<NurbsSurface> {
    let controls = source.homogeneous().iter().map(|p| [p.x, p.y, p.z, p.w]).collect::<Vec<_>>();
    let tensor = solidcore_ops::form_fill::duffy_triangle(source.degree(), [&source.knots()[0], &source.knots()[1]], &controls, corners)
        .map_err(|e| KernelError::Invalid(format!("subdivision pole chart: {e}")))?;
    let controls = tensor.controls.into_iter().map(|[x, y, z, w]| HPoint { x, y, z, w }).collect();
    NurbsSurface::from_homogeneous(tensor.degree, tensor.knots, controls).map_err(|e| KernelError::Invalid(format!("subdivision pole chart: {e}")))
}
