//! An extrude's taper saves how it closes the profile's sharp corners: `extent.corner`,
//! `tangent` or `round` (each offset curve continued straight along its end tangent, or an arc
//! about the corner); absent for `preserve_curvature`, the default, where each wall continues as
//! itself until it meets its neighbour. An older design has none and takes the default, the rule
//! its tapers were built with before SMLib (the walls meeting where the curves' offsets cross). The
//! last release refuses a design with this change (the newer format number).
