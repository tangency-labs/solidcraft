//! A `SurfaceBlend` (Blend Surface, #764) may save `seam`: a point on each of its two edge chains
//! where their stations start. Older designs need no transformation: a blend without `seam` pairs
//! its chains by itself; the last release asks for an update instead of meeting a seam it can't
//! read.
