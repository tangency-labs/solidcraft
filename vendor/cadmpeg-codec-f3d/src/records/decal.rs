// SPDX-License-Identifier: Apache-2.0
//! Decal assets and images, the canvas records beside them, and the record header they share.

use cadmpeg_core::decode::u64_from_index;

use super::{identity::Located, references::DesignClassTag};
use serde::{Deserialize, Serialize};
const DESIGN_DECAL_FIT_TO_FACES_CODE: u8 = 0x60;

/// A Decal mapping byte other than the fit-to-faces code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnrecognizedDecalMappingMode(u8);

impl TryFrom<u8> for UnrecognizedDecalMappingMode {
    type Error = String;
    fn try_from(code: u8) -> Result<Self, Self::Error> {
        if code == DESIGN_DECAL_FIT_TO_FACES_CODE {
            Err("mapping_mode unknown payload must exclude the fit-to-faces code".into())
        } else {
            Ok(Self(code))
        }
    }
}

/// Decal image mapping-mode byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub(crate) enum DesignDecalMappingMode {
    FitToFaces,
    Unknown(UnrecognizedDecalMappingMode),
}

impl DesignDecalMappingMode {
    #[must_use]
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            DESIGN_DECAL_FIT_TO_FACES_CODE => Self::FitToFaces,
            code => Self::Unknown(UnrecognizedDecalMappingMode(code)),
        }
    }

    #[must_use]
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::FitToFaces => DESIGN_DECAL_FIT_TO_FACES_CODE,
            Self::Unknown(code) => code.0,
        }
    }
}

impl From<u8> for DesignDecalMappingMode {
    fn from(code: u8) -> Self {
        Self::from_code(code)
    }
}

impl From<DesignDecalMappingMode> for u8 {
    fn from(mode: DesignDecalMappingMode) -> Self {
        mode.code()
    }
}

/// Primary Decal asset and its consecutive image-name record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignDecalAsset {
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: u64,
    entity_suffix: u32,
    name: String,
}

impl DesignDecalAsset {
    pub(crate) fn new(
        class_tags: [String; 2],
        record_indices: [u32; 2],
        byte_offset: u64,
        entity_suffix: u32,
        name: String,
    ) -> Result<Self, String> {
        if record_indices[0].checked_add(1) != Some(record_indices[1]) {
            return Err("name_record_index must immediately follow asset_record_index".into());
        }
        for (field, tag) in ["asset_class_tag", "name_class_tag"]
            .into_iter()
            .zip(&class_tags)
        {
            if tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(format!("{field} must contain printable ASCII characters"));
            }
        }
        if name.is_empty() {
            return Err("asset_name must be nonempty".into());
        }
        let units = u32::try_from(name.encode_utf16().count())
            .map_err(|_| "asset_name UTF-16 count must fit u32")?;
        let length = u64_from_index(crate::layout::design_decal_image_asset_record::LEN)
            + u64_from_index(crate::layout::design_decal_image_name_prefix::LEN)
            + 2 * u64::from(units);
        byte_offset
            .checked_add(length)
            .ok_or("asset_byte_offset and asset_name must leave a complete name record")?;
        Ok(Self {
            class_tags,
            record_index: record_indices[0],
            byte_offset,
            entity_suffix,
            name,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn entity_suffix(&self) -> u32 {
        self.entity_suffix
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) const fn primary_frame_length() -> u64 {
        u64_from_index(crate::layout::design_decal_image_asset_record::LEN)
    }
    pub(crate) fn name_frame_length(&self) -> u64 {
        u64_from_index(crate::layout::design_decal_image_name_prefix::LEN)
            + 2 * u64_from_index(self.name.encode_utf16().count())
    }
    fn name_record_index(&self) -> u32 {
        self.record_index + 1
    }
    fn name_byte_offset(&self) -> u64 {
        self.byte_offset + Self::primary_frame_length()
    }
    fn entity_reference_offset(&self) -> u64 {
        self.byte_offset
            + u64_from_index(
                crate::layout::design_decal_image_asset_record::DESIGN_ENTITY_SUFFIX_REFERENCE,
            )
            + 1
    }
    fn name_offset(&self) -> u64 {
        self.name_byte_offset() + u64_from_index(crate::layout::design_decal_image_name_prefix::LEN)
    }
}

/// Exact image and target binding owned by one Design `Decal` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignDecalImageWire")]
pub(crate) struct DesignDecalImage {
    /// Globally unique native binding identity.
    pub(crate) id: String,
    scope: Located<u32>,
    /// Source mapping-mode byte.
    pub(crate) mapping_mode: DesignDecalMappingMode,
    /// Target construction-group record index.
    pub(crate) target_group_record_index: u32,
    /// Consecutive asset and image-name records.
    pub(crate) asset: DesignDecalAsset,
}

#[cfg(test)]
thread_local! {
    pub(super) static DECAL_IMAGE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignDecalImage {
    fn clone(&self) -> Self {
        DECAL_IMAGE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope: self.scope,
            mapping_mode: self.mapping_mode,
            target_group_record_index: self.target_group_record_index,
            asset: self.asset.clone(),
        }
    }
}

impl DesignDecalImage {
    pub(crate) fn new(
        id: String,
        scope: Located<u32>,
        mapping_mode: DesignDecalMappingMode,
        target_group_record_index: u32,
        asset: DesignDecalAsset,
    ) -> Result<Self, String> {
        scope
            .offset
            .checked_add(u64_from_index(
                crate::layout::design_decal_scope_prefix::LEN,
            ))
            .ok_or("asset_reference_offset must belong to a complete Decal scope prefix")?;
        Ok(Self {
            id,
            scope,
            mapping_mode,
            target_group_record_index,
            asset,
        })
    }
    pub(crate) fn scope_record_index(&self) -> u32 {
        self.scope.value
    }
    pub(crate) fn scope_byte_offset(&self) -> u64 {
        self.scope.offset
    }
    fn asset_reference_offset(&self) -> u64 {
        self.scope.offset
            + u64_from_index(crate::layout::design_decal_scope_prefix::ASSET_REFERENCE)
            + 1
    }
    fn mapping_mode_offset(&self) -> u64 {
        self.scope.offset + u64_from_index(crate::layout::design_decal_scope_prefix::MAPPING_MODE)
    }
    fn target_group_reference_offset(&self) -> u64 {
        self.scope.offset
            + u64_from_index(crate::layout::design_decal_scope_prefix::TARGET_GROUP_REFERENCE)
            + 1
    }
}

/// Exact image and target binding owned by one Design `Decal` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignDecalImageWire {
    /// Globally unique deterministic identifier for this native binding.
    id: String,
    /// Decal scope record index.
    scope_record_index: u32,
    /// Byte offset of the scope's marked image-asset reference.
    asset_reference_offset: u64,
    /// Source mapping-mode byte.
    mapping_mode: DesignDecalMappingMode,
    /// Byte offset of `mapping_mode`.
    mapping_mode_offset: u64,
    /// Target construction-group record index.
    target_group_record_index: u32,
    /// Byte offset of the scope's marked target-group reference.
    target_group_reference_offset: u64,
    /// Dynamic class tag of the primary image-asset record.
    asset_class_tag: String,
    /// Primary image-asset record index.
    asset_record_index: u32,
    /// Byte offset of the primary image-asset record.
    asset_byte_offset: u64,
    /// Byte length from the primary image header to the name-record header.
    asset_frame_length: u64,
    /// Design entity suffix carried by the primary image record.
    asset_entity_suffix: u32,
    /// Byte offset of the marked Design entity-suffix reference.
    asset_entity_reference_offset: u64,
    /// Dynamic class tag of the image-name record.
    name_class_tag: String,
    /// Image-name record index.
    name_record_index: u32,
    /// Byte offset of the image-name record.
    name_byte_offset: u64,
    /// Byte length of the complete image-name record.
    name_frame_length: u64,
    /// Archive entry basename stored by the image-name record.
    asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    asset_name_offset: u64,
}

#[derive(Serialize)]
struct DesignDecalImageWireRef<'a> {
    id: &'a str,
    scope_record_index: u32,
    asset_reference_offset: u64,
    mapping_mode: DesignDecalMappingMode,
    mapping_mode_offset: u64,
    target_group_record_index: u32,
    target_group_reference_offset: u64,
    asset_class_tag: &'a str,
    asset_record_index: u32,
    asset_byte_offset: u64,
    asset_frame_length: u64,
    asset_entity_suffix: u32,
    asset_entity_reference_offset: u64,
    name_class_tag: &'a str,
    name_record_index: u32,
    name_byte_offset: u64,
    name_frame_length: u64,
    asset_name: &'a str,
    asset_name_offset: u64,
}

impl Serialize for DesignDecalImage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let [asset_class_tag, name_class_tag] = &self.asset.class_tags;
        DesignDecalImageWireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index(),
            asset_reference_offset: self.asset_reference_offset(),
            mapping_mode: self.mapping_mode,
            mapping_mode_offset: self.mapping_mode_offset(),
            target_group_record_index: self.target_group_record_index,
            target_group_reference_offset: self.target_group_reference_offset(),
            asset_class_tag,
            asset_record_index: self.asset.record_index,
            asset_byte_offset: self.asset.byte_offset,
            asset_frame_length: DesignDecalAsset::primary_frame_length(),
            asset_entity_suffix: self.asset.entity_suffix,
            asset_entity_reference_offset: self.asset.entity_reference_offset(),
            name_class_tag,
            name_record_index: self.asset.name_record_index(),
            name_byte_offset: self.asset.name_byte_offset(),
            name_frame_length: self.asset.name_frame_length(),
            asset_name: &self.asset.name,
            asset_name_offset: self.asset.name_offset(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignDecalImageWire> for DesignDecalImage {
    type Error = String;
    fn try_from(wire: DesignDecalImageWire) -> Result<Self, Self::Error> {
        let scope_offset = wire
            .asset_reference_offset
            .checked_sub(
                u64_from_index(crate::layout::design_decal_scope_prefix::ASSET_REFERENCE) + 1,
            )
            .ok_or("asset_reference_offset precedes the Decal scope prefix")?;
        let asset = DesignDecalAsset::new(
            [wire.asset_class_tag, wire.name_class_tag],
            [wire.asset_record_index, wire.name_record_index],
            wire.asset_byte_offset,
            wire.asset_entity_suffix,
            wire.asset_name,
        )?;
        let image = Self::new(
            wire.id,
            Located {
                value: wire.scope_record_index,
                offset: scope_offset,
            },
            wire.mapping_mode,
            wire.target_group_record_index,
            asset,
        )?;
        for (name, declared, derived) in [
            (
                "mapping_mode_offset",
                wire.mapping_mode_offset,
                image.mapping_mode_offset(),
            ),
            (
                "target_group_reference_offset",
                wire.target_group_reference_offset,
                image.target_group_reference_offset(),
            ),
            (
                "asset_frame_length",
                wire.asset_frame_length,
                DesignDecalAsset::primary_frame_length(),
            ),
            (
                "asset_entity_reference_offset",
                wire.asset_entity_reference_offset,
                image.asset.entity_reference_offset(),
            ),
            (
                "name_byte_offset",
                wire.name_byte_offset,
                image.asset.name_byte_offset(),
            ),
            (
                "name_frame_length",
                wire.name_frame_length,
                image.asset.name_frame_length(),
            ),
            (
                "asset_name_offset",
                wire.asset_name_offset,
                image.asset.name_offset(),
            ),
        ] {
            if declared != derived {
                return Err(format!("{name} must match the Decal record layout"));
            }
        }
        Ok(image)
    }
}

#[cfg(test)]
impl From<DesignDecalImage> for DesignDecalImageWire {
    fn from(value: DesignDecalImage) -> Self {
        let scope_record_index = value.scope_record_index();
        let asset_reference_offset = value.asset_reference_offset();
        let mapping_mode_offset = value.mapping_mode_offset();
        let target_group_reference_offset = value.target_group_reference_offset();
        let asset_record_index = value.asset.record_index;
        let asset_byte_offset = value.asset.byte_offset;
        let asset_frame_length = DesignDecalAsset::primary_frame_length();
        let asset_entity_suffix = value.asset.entity_suffix;
        let asset_entity_reference_offset = value.asset.entity_reference_offset();
        let name_record_index = value.asset.name_record_index();
        let name_byte_offset = value.asset.name_byte_offset();
        let name_frame_length = value.asset.name_frame_length();
        let asset_name_offset = value.asset.name_offset();
        let [asset_class_tag, name_class_tag] = value.asset.class_tags;
        Self {
            id: value.id,
            scope_record_index,
            asset_reference_offset,
            mapping_mode: value.mapping_mode,
            mapping_mode_offset,
            target_group_record_index: value.target_group_record_index,
            target_group_reference_offset,
            asset_class_tag,
            asset_record_index,
            asset_byte_offset,
            asset_frame_length,
            asset_entity_suffix,
            asset_entity_reference_offset,
            name_class_tag,
            name_record_index,
            name_byte_offset,
            name_frame_length,
            asset_name: value.asset.name,
            asset_name_offset,
        }
    }
}

/// One indexed record header in the recursive Design `BulkStream` tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignRecordHeader {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this record within the recursive `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this header within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
}

#[cfg(test)]
mod tests;
