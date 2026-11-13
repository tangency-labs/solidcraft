//! A sketch point can be placed by parameters: `exprs` (`x` and/or `y`, expressions over the
//! design's parameters, in the design's units when a number in them has no unit) on a point that
//! is `fixed` where they come to, and moved there again before each solve when the parameters
//! change. Older designs need no transformation: their points have no `exprs`. The last release
//! asks for an update instead of opening a design with this format number.
