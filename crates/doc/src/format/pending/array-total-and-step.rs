//! A rectangular pattern can save `extent1` and `extent2`, and a circular one `step`. They say
//! how to read the number beside them: with `extent1` the first direction's `spacing1` is the
//! distance from the first copy to the last rather than the step between neighbours, and with
//! `step` a circular pattern's `angle` is the turn from one copy to the next rather than the
//! whole turn they are spread over. Plasticity's array cards offer both readings (Extent or
//! Spacing, Total or Spacing), and a pattern along a path already saved its own `extent`.
//!
//! An older design has neither, which reads as false: `spacing1` and `spacing2` are steps and
//! `angle` is the total, exactly as they always were, so nothing it holds moves.
//!
//! The last release refuses a design that has them, since the format number it carries is newer
//! than the one that release reads; that is what keeps an older release from reading a span as a
//! step and spreading the copies much too far.
