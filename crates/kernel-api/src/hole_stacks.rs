//! Hole stacks ([`HoleStacks`]): the places a rivet, bolt or pin can pass through two or more
//! touching bodies, read from their records' exact faces. The options and the answer are the
//! record's (`solidcore_record::hole_stacks`).

use solidcore_record::brep::Brep;
use solidcore_record::hole_stacks::{Options, Stacks};

use crate::Backend;

/// A hole-stack query the kernel refuses, in its words: more holes than one call answers, or too
/// many nearly coaxial holes to compare.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HoleStacksRefused(pub String);

/// The hole stacks of one kernel ([`crate::OpKind::HoleStacks`]).
pub trait HoleStacks: Backend {
    /// The hole stacks among `records`: every set of coaxial holes through two or more touching
    /// bodies, and the holes in no stack, shaped by `o`.
    fn hole_stacks(&self, records: &[&Brep], o: &Options) -> Result<Stacks, HoleStacksRefused>;
}
