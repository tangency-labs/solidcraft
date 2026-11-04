use super::{dimension_null_locus_wire, dimensions::DesignDimensionLocusPair};
use serde::{Deserialize, Serialize};
use std::ops::Deref;

/// An arena containing only pairs of nonnull geometry loci with an opaque index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<DesignDimensionLocusPair>",
    into = "Vec<DesignDimensionLocusPair>"
)]
pub(crate) struct DesignDimensionLocusPairs(Vec<DesignDimensionLocusPair>);

impl TryFrom<Vec<DesignDimensionLocusPair>> for DesignDimensionLocusPairs {
    type Error = String;

    fn try_from(pairs: Vec<DesignDimensionLocusPair>) -> Result<Self, Self::Error> {
        if pairs.iter().any(|pair| pair.opaque_index().is_none()) {
            return Err(
                "design_dimension_locus_pairs requires two nonnull geometry loci and opaque_index"
                    .into(),
            );
        }
        Ok(Self(pairs))
    }
}

impl From<DesignDimensionLocusPairs> for Vec<DesignDimensionLocusPair> {
    fn from(pairs: DesignDimensionLocusPairs) -> Self {
        pairs.0
    }
}

impl Deref for DesignDimensionLocusPairs {
    type Target = [DesignDimensionLocusPair];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a DesignDimensionLocusPairs {
    type Item = &'a DesignDimensionLocusPair;
    type IntoIter = std::slice::Iter<'a, DesignDimensionLocusPair>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// An arena containing only null-first geometry pairs without an opaque index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<dimension_null_locus_wire::Entry>",
    into = "Vec<dimension_null_locus_wire::Wire>"
)]
pub(crate) struct DesignDimensionNullLocusPairs(Vec<DesignDimensionLocusPair>);

impl TryFrom<Vec<DesignDimensionLocusPair>> for DesignDimensionNullLocusPairs {
    type Error = String;

    fn try_from(pairs: Vec<DesignDimensionLocusPair>) -> Result<Self, Self::Error> {
        if pairs.iter().any(|pair| pair.opaque_index().is_some()) {
            return Err("design_dimension_null_locus_pairs requires a null first locus, a nonnull second locus, and no opaque_index".into());
        }
        Ok(Self(pairs))
    }
}

impl TryFrom<Vec<dimension_null_locus_wire::Entry>> for DesignDimensionNullLocusPairs {
    type Error = String;

    fn try_from(entries: Vec<dimension_null_locus_wire::Entry>) -> Result<Self, Self::Error> {
        entries
            .into_iter()
            .map(|entry| entry.0)
            .collect::<Vec<_>>()
            .try_into()
    }
}

impl DesignDimensionNullLocusPairs {
    pub(crate) fn from_entries(
        entries: Vec<dimension_null_locus_wire::Entry>,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let pairs = entries.into_iter().map(|entry| entry.0).collect::<Vec<_>>();
        Self::try_from(pairs).map_err(cadmpeg_ir::NativeConvertError::InvalidCollection)
    }
}

impl From<DesignDimensionNullLocusPairs> for Vec<dimension_null_locus_wire::Wire> {
    fn from(pairs: DesignDimensionNullLocusPairs) -> Self {
        pairs
            .0
            .iter()
            .map(dimension_null_locus_wire::Wire::from)
            .collect()
    }
}

impl Deref for DesignDimensionNullLocusPairs {
    type Target = [DesignDimensionLocusPair];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a DesignDimensionNullLocusPairs {
    type Item = &'a DesignDimensionLocusPair;
    type IntoIter = std::slice::Iter<'a, DesignDimensionLocusPair>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests;
