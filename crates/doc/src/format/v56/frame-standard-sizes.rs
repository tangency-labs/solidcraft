//! A frame member's built-in profile can save `size`, the standard steel size it was made from
//! (`"size": "RSC 203x89"`), beside its dimensions, which it saves as before. A channel profile
//! can save `flange_slope` (an angle; its flanges' inner faces taper) and a rectangular tube
//! profile `inner_radius`. An older design has none of them: its channels have parallel flanges,
//! its tubes an inner radius of the outer one less the wall, and its members no size name. The
//! last release warns about the newer format number; it can't read a frame member feature with
//! one of these keys, which keeps its place on the timeline unevaluated and is saved back as it
//! came.
