//! A Single hole's (or a cylinder's) reference saves `datum: {"plane": name}` or `datum: {"axis":
//! name}` when it measures from a plane or an axis (an origin one, or a construction one by its
//! name) instead of an edge, its `edge` then being only the point where it was picked; absent, the
//! reference is an edge, as in designs before this change. The last release refuses a design with
//! this change (the newer format number).
