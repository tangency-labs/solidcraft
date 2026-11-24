//! A Split Face by sketch curves may save `loops: true`: the sketch's curves are joined end to end
//! before the split, so a closed loop of lines and arcs cuts the face along the loop only. An older
//! design has none and splits by each curve on its own, an open one extended past its ends, as
//! before. The last release warns about the newer format number and opens the design, splitting
//! that feature curve by curve (its sides extended across the face).
