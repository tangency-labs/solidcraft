//! A feature's operation can now be `slice`: the tools divide the targets and both halves are
//! kept, what lies outside the tools under the target's own name and what lies inside as a body
//! of its own. Combine offers it the way Plasticity's Boolean does, beside union, difference and
//! intersect.
//!
//! An older design has no feature saved with it, since nothing could make one, so it reads back
//! unchanged and needs no rewriting.
//!
//! The last release refuses a design that has it, since the format number it carries is newer
//! than the one that release reads; that is what keeps an older release from reading `slice` as
//! an operation it has never heard of.
