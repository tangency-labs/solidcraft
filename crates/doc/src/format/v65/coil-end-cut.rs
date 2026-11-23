//! A coil may save `"end_cut": "square"` (#20737): its wire then ends square to the wire, the
//! section lying across the helix's tangent, instead of in a plane through the coil's axis.
//! Without it (every older design, and `"axial"`, which is never written) the coil builds as
//! before, so nothing is rewritten. The last release warns about the newer format number, builds
//! such a coil with axial ends and keeps the field it does not know when saving.
