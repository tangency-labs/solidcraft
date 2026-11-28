//! A driven (reference) sketch dimension can carry a read-only parameter name (`driven_param` on
//! planar `Constraint` and 3D `WireConstraint`, #14426) so downstream features, sketches and
//! expressions evaluated after that sketch in the timeline can reference its measured value.
//! Older designs never have `driven_param`, so no step is needed: their driven dimensions keep
//! `driven_param: None` and behave as before. The last release warns about the newer format number
//! and opens the design; it ignores `driven_param`, so expressions referencing that name will
//! report an unknown parameter there.
