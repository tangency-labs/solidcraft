//! A face anchor of a design saved in format 48 or before saves `old_kernel_frame`. 0.30.1's
//! kernel read a reversed plane face with its axes swapped, where today's turns the second one
//! over, so on such a face of an imported body the anchor's origin and x direction are in that
//! frame; without the mark the sketch opens turned a quarter turn (#15891). Anchors captured
//! since have no mark. The last release warns about the newer format number, opens what it can
//! read and keeps the rest.
