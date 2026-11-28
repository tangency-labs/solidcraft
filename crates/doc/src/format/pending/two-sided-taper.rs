//! An extrude's taper drafts both sides of a two-sided or symmetric extrude (#22419), and a
//! two-sided extrude may save `taper2` (an angle expression: the second side's taper, with
//! `distance2` only; absent, the second side takes `taper`). The halves' side faces are named by
//! their sketch curves, as one side's are, and their caps as the straight extrude names them.
//! Older designs never hold `taper2`, and none built a two-sided or symmetric extrude with a
//! taper other than 0 (it was refused), so nothing is rewritten and every older design builds as
//! it did. The last release warns about the newer format number, opens the design and keeps
//! `taper2` for the next save without reading it: an extrude with a taper other than 0 on two
//! sides or symmetric is refused there with its old message, and one with `taper2` alone (no
//! `taper`, or a taper of 0) builds there as a straight two-sided extrude.
