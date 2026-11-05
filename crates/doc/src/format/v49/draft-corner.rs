//! An extrude's taper saves how it closes the profile's sharp corners when the user chose:
//! `extent.corner`, `preserve_curvature` (each wall continues as itself until it meets its
//! neighbour), `tangent` (each offset curve continued straight along its end tangent) or `round`
//! (an arc about the corner). Without it each profile preserves curvature where every curve is a
//! line or an arc and takes the tangent rule where another curve has a corner, the feature's
//! warning saying so; an older design has none and gets that choice. The last release refuses a
//! design with this change (the newer format number).
