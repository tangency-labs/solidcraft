//! Drawn 3D sketch arcs: a wire drawn as an exact circular arc now also saves its analytic
//! form (`arc`: centre, unit normal, radius, start and end) beside its three defining fit
//! points and its samples. Older designs contain no such arcs and open unchanged; a design
//! saved with this change carries the newer format number, which the last release refuses.
