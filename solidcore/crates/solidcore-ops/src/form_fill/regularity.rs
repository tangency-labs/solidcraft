//! Conservative regularity certificate for a raw positive rational tensor cap.

/// Prove regular interiors and consistent native seam normals, permitting only
/// exactly qualified isolated zeros at the four original chart corners.
/// False or an error is a refusal; no sample-based fallback is performed.
pub fn certify(
    degree: [usize; 2],
    knots: [&[f64]; 2],
    size: [usize; 2],
    controls: &[[f64; 4]],
) -> solidcore::Result<bool> {
    solidcore::geom::form_fill::regularity::certify(degree, knots, size, controls)
}
