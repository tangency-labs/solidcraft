//! A profile pick of twin regions (regions the curves around them can't tell apart, such as the
//! halves of a lens a line cuts) saves each region's curves with the side of each it lies on
//! (`{"type": "sided_curves", "loops": [[{"curve": "c1", "side": "inside"}, ...]]}`) instead of the
//! regions' numbers, so the pick follows the sketch when a parameter changes its regions (#22548).
//! Older designs keep the picks they saved. The last release warns about the newer format number,
//! opens the design and keeps a feature with such a pick as a feature it can't read.
