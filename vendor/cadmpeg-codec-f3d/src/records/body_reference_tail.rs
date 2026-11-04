// SPDX-License-Identifier: Apache-2.0
//! Independent counted reference runs in whole-body serialization tails.

use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::bytes::take_reference;
#[cfg(test)]
use crate::bytes::Reference;
use crate::records::identity::Located;

cadmpeg_core::named_optional_field!(deserialize_extra_zero, Located<u32>, "extra_zero");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BodyTailReferenceWire", into = "BodyTailReferenceWire")]
pub(crate) struct BodyTailReference {
    offset: u64,
    encoding: NativeBytes<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyTailReferenceWire {
    offset: u64,
    encoding: NativeBytes<Vec<u8>>,
}

impl TryFrom<BodyTailReferenceWire> for BodyTailReference {
    type Error = String;

    fn try_from(wire: BodyTailReferenceWire) -> Result<Self, Self::Error> {
        Self::new(wire.offset, wire.encoding.into_inner())
    }
}

impl From<BodyTailReference> for BodyTailReferenceWire {
    fn from(value: BodyTailReference) -> Self {
        Self {
            offset: value.offset,
            encoding: value.encoding,
        }
    }
}

impl BodyTailReference {
    pub(crate) fn null() -> Self {
        Self {
            offset: 0,
            encoding: vec![0].into(),
        }
    }

    pub(crate) fn new(offset: u64, encoding: Vec<u8>) -> Result<Self, String> {
        let mut cursor = 0;
        if take_reference(&encoding, &mut cursor).is_none() || cursor != encoding.len() {
            return Err("body-tail reference must contain one complete Design reference".into());
        }
        Ok(Self {
            offset,
            encoding: encoding.into(),
        })
    }

    #[cfg(test)]
    pub(crate) fn target(&self) -> Option<u64> {
        take_reference(self.encoding.as_ref(), &mut 0).and_then(|reference| reference.target())
    }

    #[cfg(test)]
    pub(crate) fn is_null(&self) -> bool {
        matches!(
            take_reference(self.encoding.as_ref(), &mut 0),
            Some(Reference::Null)
        )
    }

    fn end(&self) -> Option<u64> {
        self.offset
            .checked_add(u64::try_from(self.encoding.as_ref().len()).ok()?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignBodyReferenceTailWire",
    into = "DesignBodyReferenceTailWire"
)]
pub(crate) struct DesignBodyReferenceTail {
    pub(crate) id: String,
    pub(crate) record_index: u64,
    pub(crate) byte_offset: u64,
    pub(crate) serialization_form: Located<u32>,
    pub(crate) first_count_offset: u64,
    pub(crate) first_references: Vec<BodyTailReference>,
    pub(crate) second_count_offset: u64,
    pub(crate) second_references: Vec<BodyTailReference>,
    pub(crate) extra_zero: Option<Located<u32>>,
    pub(crate) trailing_form: Located<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignBodyReferenceTailWire {
    pub(crate) id: String,
    pub(crate) record_index: u64,
    pub(crate) byte_offset: u64,
    pub(crate) serialization_form: Located<u32>,
    pub(crate) first_count_offset: u64,
    pub(crate) first_references: Vec<BodyTailReference>,
    pub(crate) second_count_offset: u64,
    pub(crate) second_references: Vec<BodyTailReference>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extra_zero"
    )]
    pub(crate) extra_zero: Option<Located<u32>>,
    pub(crate) trailing_form: Located<u32>,
}

fn run_end(count_offset: u64, references: &[BodyTailReference]) -> Result<u64, String> {
    u32::try_from(references.len()).map_err(|_| "body-tail reference count exceeds u32")?;
    let mut expected = count_offset
        .checked_add(4)
        .ok_or("body-tail reference count offset overflows")?;
    for reference in references {
        if reference.offset != expected {
            return Err("body-tail reference locations are not contiguous".into());
        }
        expected = reference
            .end()
            .ok_or("body-tail reference extent overflows")?;
    }
    Ok(expected)
}

impl TryFrom<DesignBodyReferenceTailWire> for DesignBodyReferenceTail {
    type Error = String;

    fn try_from(wire: DesignBodyReferenceTailWire) -> Result<Self, Self::Error> {
        if wire.record_index == 0
            || wire.serialization_form.offset <= wire.byte_offset
            || wire.serialization_form.offset.checked_add(4) != Some(wire.first_count_offset)
            || run_end(wire.first_count_offset, &wire.first_references)? != wire.second_count_offset
        {
            return Err("body-tail first run disagrees with its frame".into());
        }
        let mut end = run_end(wire.second_count_offset, &wire.second_references)?;
        if let Some(extra) = wire.extra_zero {
            if extra.value != 0 || extra.offset != end || wire.trailing_form.value != 9 {
                return Err("body-tail extra zero disagrees with its frame".into());
            }
            end = end
                .checked_add(4)
                .ok_or("body-tail extra-zero extent overflows")?;
        }
        if wire.trailing_form.offset != end
            || wire.trailing_form.offset.checked_add(4).is_none()
            || !matches!(wire.trailing_form.value, 8 | 9)
        {
            return Err("body-tail trailing form disagrees with its frame".into());
        }
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            serialization_form: wire.serialization_form,
            first_count_offset: wire.first_count_offset,
            first_references: wire.first_references,
            second_count_offset: wire.second_count_offset,
            second_references: wire.second_references,
            extra_zero: wire.extra_zero,
            trailing_form: wire.trailing_form,
        })
    }
}

impl From<DesignBodyReferenceTail> for DesignBodyReferenceTailWire {
    fn from(value: DesignBodyReferenceTail) -> Self {
        Self {
            id: value.id,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            serialization_form: value.serialization_form,
            first_count_offset: value.first_count_offset,
            first_references: value.first_references,
            second_count_offset: value.second_count_offset,
            second_references: value.second_references,
            extra_zero: value.extra_zero,
            trailing_form: value.trailing_form,
        }
    }
}

mod identity_rewrite;

#[cfg(test)]
mod tests;
