// SPDX-License-Identifier: Apache-2.0
//! Typed Align scope and constructor records.

use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_align_construction,
    DesignAlignConstruction,
    "construction"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignAlignForm {
    Compact,
    Extended,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAlignEnvelope {
    pub(crate) form: DesignAlignForm,
    pub(crate) first_reference: Located<u32>,
    pub(crate) source_code: Located<u32>,
    pub(crate) second_reference: Located<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_align_construction"
    )]
    pub(crate) construction: Option<DesignAlignConstruction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAlignConstruction {
    pub(crate) record_index: u32,
    pub(crate) byte_offset: u64,
    pub(crate) paired_byte_offset: u64,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) paired_class_tag: DesignClassTag,
    pub(crate) marked_words: [Located<u32>; 8],
    pub(crate) codes: [Located<u32>; 5],
    pub(crate) matrix_scalars: Located<[FiniteReal; 16]>,
    pub(crate) scalar: Located<FiniteReal>,
}

rewrite_native_record!(DesignAlignConstruction, []; {record_index, byte_offset, paired_byte_offset, class_tag, paired_class_tag, marked_words, codes, matrix_scalars, scalar});
rewrite_native_scalar!(DesignAlignForm);
rewrite_native_record!(DesignAlignEnvelope, []; {form, first_reference, source_code, second_reference, construction});
