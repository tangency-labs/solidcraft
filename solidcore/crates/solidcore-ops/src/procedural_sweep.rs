//! Procedural sweeps in records: the kernel moves and bounds them; this converts.
//!
//! Callers holding a portable record (an application placing a body, or sizing its box)
//! call these instead of computing on a sweep's fields themselves.

use solidcore_record::brep::sweep::ProceduralSweepRecord;

/// `sweep` moved by the rigid motion `p ↦ rotation · p + translation` (`rotation`'s rows
/// orthonormal, determinant +1). Its stations, parameter map and certified bound stay.
#[must_use]
pub fn moved(
    sweep: &ProceduralSweepRecord,
    rotation: &[[f64; 3]; 3],
    translation: [f64; 3],
) -> ProceduralSweepRecord {
    solidcore_convert::record_procedural_sweep(
        solidcore_convert::procedural_sweep(sweep).moved(rotation, translation),
    )
}

/// A box `(min, max)` enclosing the exact sweep: its net's control hull widened by its
/// certified bound. `None` when the record has no usable net or a bad bound.
#[must_use]
pub fn outline(sweep: &ProceduralSweepRecord) -> Option<([f64; 3], [f64; 3])> {
    solidcore_convert::procedural_sweep(sweep).outline()
}
