//! A construction plane can be another plane moved along its normal until it goes through a
//! point (`through_point`: `base` and the `point` reference), so it follows the point. Older
//! designs need no transformation: they have no such planes. The last release asks for an update
//! instead of meeting a plane type it can't read.
