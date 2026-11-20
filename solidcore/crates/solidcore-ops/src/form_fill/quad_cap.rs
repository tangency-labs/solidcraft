//! Portable directed cap pieces and the kernel's exact preparation operation.
pub use kernel::{Piece, MAX_PIECES};
use solidcore::geom::form_fill::quad_cap as kernel;

/// Normalize, exactly embed, blend and certify four ordered rational rim sides.
/// All coordinates and fitted coefficients are computed by the kernel.
pub fn construct(sides: [&[Piece<'_>]; 4]) -> solidcore::Result<super::Tensor> {
    kernel::construct(sides)
}
