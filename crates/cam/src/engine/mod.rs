//! The engine boundary: the only module that talks to raygeo.
//!
//! raygeo is the motion authority — it plans and generates every cut. This
//! module turns SolidCraft's regions into the engine's inputs, runs one
//! plan per region and depth level, and translates the engine's motion
//! commands back into this crate's checked [`Move`]s. The rest of the crate
//! never sees a raygeo type: operations are validate → prepare → plan →
//! translate → verify frontends, and the crate's own offsetting survives as
//! the oracle the translator clamps against and the stock-to-leave preparer.

pub(crate) mod geometry;
pub(crate) mod plan;
pub(crate) mod translate;
