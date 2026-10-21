//! Pcurves for truck faces, which keep none: each edge use's curve in its face's surface
//! parameters, on the parameter of the edge's own curve (as the adapter writes it), running in
//! that curve's direction. Exact where the edge lies on its surface in closed form, otherwise a
//! 2D B-spline fitted within the tolerance.
use super::Result;
use solidcraft_geom::brep as b;
use truck_modeling as mt;

/// For each of `face`'s boundary wires, in order, the pcurve of each of its edges, in order, in
/// the (u, v) of the face's surface as truck evaluates it (`ParametricSurface::subs`), within
/// `tolerance` mm when lifted onto the surface.
pub(super) fn pcurves(face: &mt::Face, tolerance: f64) -> Result<Vec<Vec<b::Curve2>>> {
    let _ = (face, tolerance);
    Err(crate::KernelError::Failed("not supported yet: pcurves of truck faces".into()))
}
