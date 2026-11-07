//! Why a design could not be opened or a snapshot taken.

use crate::schema::{BodyName, FaceName, FeatureId, ParameterName};

/// Why a design could not be opened or a snapshot taken.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a design (or snapshot) this crate reads.
    #[error("cannot read: {0}")]
    Read(String),
    #[error("unknown parameter `{0}`")]
    UnknownParameter(ParameterName),
    /// An override gives a parameter a value it cannot take.
    #[error("parameter `{name}`: {reason}")]
    ParameterValue { name: ParameterName, reason: String },
    /// The design refused the overrides together (an expression that does not parse, a cycle).
    #[error("parameter overrides: {0}")]
    Overrides(String),
    /// Timeline features failed.
    #[error("{} timeline feature(s) failed: {}", .0.len(), .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Evaluation(Vec<FeatureFailure>),
    #[error("the design has no feature F{0}")]
    NoFeature(FeatureId),
    #[error("no body `{0}`")]
    NoBody(BodyName),
    #[error("body `{body}` has no face `{face}`")]
    NoFace { body: BodyName, face: FaceName },
    /// An option or edit is out of range.
    #[error("{0}")]
    Invalid(String),
    /// The snapshot would be larger than a limit allows.
    #[error("too large: {0}")]
    Limit(String),
    /// The design uses something this crate cannot export yet, or the snapshot's schema is newer.
    #[error("not supported yet: {0}")]
    NotSupported(String),
}

/// A timeline feature that failed, and why.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{name} (F{id}): {message}")]
pub struct FeatureFailure {
    pub id: FeatureId,
    pub name: String,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, Error>;
