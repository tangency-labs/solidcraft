//! A Coil may save a `taper` angle: the coil's radius grows by `pitch · tan(taper)` per turn, so
//! it follows a cone. Older designs need no transformation: their coils have no taper, a
//! cylinder. The last release warns about the newer format number and opens the design, but
//! drops the `taper` of such a coil and rebuilds it as a cylinder.
