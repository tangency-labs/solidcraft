// SPDX-License-Identifier: Apache-2.0
//! Sketch text, points, curves, surfaces and NURBS poles.

use cadmpeg_ir::native::bytes::NativeBytes;

use super::references::DesignClassTag;
use super::serde_column::SliceColumn;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{
    Angle, FiniteReal, NonNegativeLength, NonNegativeReal, PositiveLength, PositiveReal,
};
use cadmpeg_ir::sketches::TextPlacement;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_anchor, Point2, "anchor");
cadmpeg_core::named_optional_field!(deserialize_planar_z_noise, [f64; 3], "planar_z_noise");
cadmpeg_core::named_optional_field!(deserialize_base_id, u64, "base_id");
cadmpeg_core::named_optional_field!(deserialize_closure, SketchPointClosureSerde, "closure");
cadmpeg_core::named_optional_field!(deserialize_entity_genesis, u64, "entity_genesis");
cadmpeg_core::named_optional_field!(deserialize_first_reference, u32, "first_reference");
cadmpeg_core::named_optional_field!(deserialize_geometry, SketchCurveGeometry, "geometry");
cadmpeg_core::named_optional_field!(
    deserialize_horizontal_alignment,
    u32,
    "horizontal_alignment"
);
cadmpeg_core::named_optional_field!(deserialize_owner_reference, u32, "owner_reference");
cadmpeg_core::named_optional_field!(deserialize_persistent_id, u64, "persistent_id");
cadmpeg_core::named_optional_field!(deserialize_rotation, f64, "rotation");
cadmpeg_core::named_optional_field!(deserialize_second_reference, u32, "second_reference");
cadmpeg_core::named_optional_field!(deserialize_vertical_alignment, u32, "vertical_alignment");
cadmpeg_core::named_optional_field!(deserialize_width_factor, f64, "width_factor");
/// One text entity in a Fusion sketch coordinate system.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "SketchTextSerde")]
pub(crate) struct SketchText {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this text record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Owning sketch record index.
    pub(crate) owner_reference: u32,
    /// Source per-file dynamic ASCII class tag naming this record's type.
    pub(crate) class_tag: DesignClassTag,
    /// Record version of this record's class, from its Design `MetaStream` type
    /// table. It selects the member sequence the record was written under.
    pub(crate) class_version: u32,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Optional `EntityGenesis` origin bitfield.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) entity_genesis: Option<u64>,
    /// Persistent identity of the text entity. A `txt_tag` record below class
    /// version 4 writes no identity key and stores none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) persistent_id: Option<u64>,
    /// Persistent base identity, a property key absent from some records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) base_id: Option<u64>,
    /// Unicode text content.
    pub(crate) text: String,
    /// Font-family name.
    pub(crate) font_family: String,
    /// Numeric font weight stored by the sketch-text class.
    pub(crate) font_weight: i32,
    /// Nominal text height in millimetres.
    pub(crate) height: PositiveLength,
    /// Display colour of the glyphs. Both identity forms store it, so it is
    /// never absent. `SketchGeometry` carries no display attribute on any
    /// variant, so the colour stays on the native record.
    pub(crate) color: Color,
    /// Identity-form layout: `txt_tag` placement or `textex_tag` width, alignment,
    /// and parameter references.
    pub(crate) layout: SketchTextLayout,
    /// Complete source record bytes for native replay and rewrite.
    pub(crate) raw_bytes: NativeBytes<Vec<u8>>,
}

#[cfg(test)]
thread_local! {
    static SKETCH_TEXT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for SketchText {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SKETCH_TEXT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_index: self.record_index,
            owner_reference: self.owner_reference,
            class_tag: self.class_tag.clone(),
            class_version: self.class_version,
            byte_offset: self.byte_offset,
            entity_genesis: self.entity_genesis,
            persistent_id: self.persistent_id,
            base_id: self.base_id,
            text: self.text.clone(),
            font_family: self.font_family.clone(),
            font_weight: self.font_weight,
            height: self.height,
            color: self.color,
            layout: self.layout.clone(),
            raw_bytes: (self.raw_bytes.clone()),
        }
    }
}

impl Serialize for SketchText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            record_index: u32,
            owner_reference: u32,
            class_tag: &'a str,
            class_version: u32,
            byte_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            entity_genesis: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            persistent_id: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            base_id: Option<u64>,
            text: &'a str,
            font_family: &'a str,
            font_weight: i32,
            height: f64,
            #[serde(skip_serializing_if = "Option::is_none")]
            width_factor: Option<f64>,
            color: Color,
            #[serde(skip_serializing_if = "Option::is_none")]
            anchor: Option<Point2>,
            #[serde(skip_serializing_if = "Option::is_none")]
            rotation: Option<f64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            horizontal_alignment: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            vertical_alignment: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            first_reference: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            second_reference: Option<u32>,
            raw_bytes: NativeBytes<&'a [u8]>,
        }
        let placement = self.placement();
        let alignment = self.alignment();
        WireRef {
            id: &self.id,
            record_index: self.record_index,
            owner_reference: self.owner_reference,
            class_tag: self.class_tag.as_str(),
            class_version: self.class_version,
            byte_offset: self.byte_offset,
            entity_genesis: self.entity_genesis,
            persistent_id: self.persistent_id,
            base_id: self.base_id,
            text: &self.text,
            font_family: &self.font_family,
            font_weight: self.font_weight,
            height: self.height.get(),
            width_factor: self.width_factor().map(NonNegativeReal::get),
            color: self.color,
            anchor: placement.map(|value| value.anchor.get()),
            rotation: placement.map(|value| value.rotation.get()),
            horizontal_alignment: alignment.map(|value| value.horizontal),
            vertical_alignment: alignment.map(|value| value.vertical),
            first_reference: match self.layout {
                SketchTextLayout::TxtTag { .. } => None,
                SketchTextLayout::TextexTag {
                    first_reference, ..
                } => first_reference,
            },
            second_reference: match self.layout {
                SketchTextLayout::TxtTag { .. } => None,
                SketchTextLayout::TextexTag {
                    second_reference, ..
                } => second_reference,
            },
            raw_bytes: (&self.raw_bytes).into(),
        }
        .serialize(serializer)
    }
}

/// Horizontal and vertical alignment members of a sketch-text record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SketchTextAlignment {
    pub(crate) horizontal: u32,
    pub(crate) vertical: u32,
}

/// `txt_tag` versus `textex_tag` member layout of one sketch-text record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SketchTextLayout {
    TxtTag {
        placement: TextPlacement<FinitePoint2>,
    },
    TextexTag {
        width_factor: NonNegativeReal,
        alignment: Option<SketchTextAlignment>,
        first_reference: Option<u32>,
        second_reference: Option<u32>,
        placement: Option<TextPlacement<FinitePoint2>>,
    },
}

impl SketchText {
    pub(crate) fn width_factor(&self) -> Option<NonNegativeReal> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag { width_factor, .. } => Some(width_factor),
        }
    }

    pub(crate) fn placement(&self) -> Option<TextPlacement<FinitePoint2>> {
        match self.layout {
            SketchTextLayout::TxtTag { placement } => Some(placement),
            SketchTextLayout::TextexTag { placement, .. } => placement,
        }
    }

    pub(crate) fn alignment(&self) -> Option<SketchTextAlignment> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag { alignment, .. } => alignment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SketchTextSerde {
    id: String,
    record_index: u32,
    owner_reference: u32,
    class_tag: String,
    class_version: u32,
    byte_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    entity_genesis: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_persistent_id"
    )]
    persistent_id: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_id"
    )]
    base_id: Option<u64>,
    text: String,
    font_family: String,
    font_weight: i32,
    height: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width_factor"
    )]
    width_factor: Option<f64>,
    color: Color,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_anchor"
    )]
    anchor: Option<Point2>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rotation"
    )]
    rotation: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_horizontal_alignment"
    )]
    horizontal_alignment: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vertical_alignment"
    )]
    vertical_alignment: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_reference"
    )]
    first_reference: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_reference"
    )]
    second_reference: Option<u32>,
    raw_bytes: NativeBytes<Vec<u8>>,
}

impl TryFrom<SketchTextSerde> for SketchText {
    type Error = String;

    fn try_from(wire: SketchTextSerde) -> Result<Self, Self::Error> {
        let placement = match (wire.anchor, wire.rotation) {
            (None, None) => None,
            (Some(anchor), Some(rotation)) => Some(TextPlacement {
                anchor: FinitePoint2::new(anchor).ok_or("sketch text anchor must be finite")?,
                rotation: Angle::new(rotation).ok_or("sketch text rotation must be finite")?,
            }),
            _ => return Err("sketch text anchor and rotation must occur together".into()),
        };
        let alignment =
            match (wire.horizontal_alignment, wire.vertical_alignment) {
                (None, None) => None,
                (Some(horizontal), Some(vertical)) => Some(SketchTextAlignment {
                    horizontal,
                    vertical,
                }),
                _ => return Err(
                    "sketch text horizontal_alignment and vertical_alignment must occur together"
                        .into(),
                ),
            };
        let layout = match (
            wire.width_factor,
            alignment,
            wire.first_reference,
            wire.second_reference,
            placement,
        ) {
            (None, None, None, None, Some(placement)) => SketchTextLayout::TxtTag { placement },
            (Some(width_factor), alignment, first_reference, second_reference, placement) => {
                SketchTextLayout::TextexTag {
                    width_factor: NonNegativeReal::new(width_factor)
                        .ok_or("sketch text width_factor must be finite and nonnegative")?,
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                }
            }
            _ => {
                return Err(
                    "sketch text layout disagrees with width_factor, alignment, and placement"
                        .into(),
                )
            }
        };
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag.try_into()?,
            class_version: wire.class_version,
            byte_offset: wire.byte_offset,
            entity_genesis: wire.entity_genesis,
            persistent_id: wire.persistent_id,
            base_id: wire.base_id,
            text: wire.text,
            font_family: wire.font_family,
            font_weight: wire.font_weight,
            height: PositiveLength::new(wire.height)
                .ok_or("sketch text height must be positive and finite")?,
            color: wire.color,
            layout,
            raw_bytes: (wire.raw_bytes),
        })
    }
}

#[cfg(test)]
impl From<SketchText> for SketchTextSerde {
    fn from(text: SketchText) -> Self {
        let (width_factor, alignment, first_reference, second_reference, placement) =
            match text.layout {
                SketchTextLayout::TxtTag { placement } => (None, None, None, None, Some(placement)),
                SketchTextLayout::TextexTag {
                    width_factor,
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                } => (
                    Some(width_factor.get()),
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                ),
            };
        Self {
            id: text.id,
            record_index: text.record_index,
            owner_reference: text.owner_reference,
            class_tag: text.class_tag.into(),
            class_version: text.class_version,
            byte_offset: text.byte_offset,
            entity_genesis: text.entity_genesis,
            persistent_id: text.persistent_id,
            base_id: text.base_id,
            text: text.text,
            font_family: text.font_family,
            font_weight: text.font_weight,
            height: text.height.get(),
            width_factor,
            color: text.color,
            anchor: placement.map(|value| value.anchor.get()),
            rotation: placement.map(|value| value.rotation.get()),
            horizontal_alignment: alignment.map(|value| value.horizontal),
            vertical_alignment: alignment.map(|value| value.vertical),
            first_reference,
            second_reference,
            raw_bytes: (text.raw_bytes),
        }
    }
}

/// Selector and state following a three-coordinate sketch point payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointClosure {
    Selector0State0,
    Selector0State1,
    Selector1State0,
    Selector2State1,
    Selector4State0,
    Selector4State1,
}

impl SketchPointClosure {
    pub(crate) fn selector(self) -> u64 {
        match self {
            Self::Selector0State0 | Self::Selector0State1 => 0,
            Self::Selector1State0 => 1,
            Self::Selector2State1 => 2,
            Self::Selector4State0 | Self::Selector4State1 => 4,
        }
    }

    pub(crate) fn state(self) -> u8 {
        match self {
            Self::Selector0State0 | Self::Selector1State0 | Self::Selector4State0 => 0,
            Self::Selector0State1 | Self::Selector2State1 | Self::Selector4State1 => 1,
        }
    }

    pub(crate) fn from_pair(selector: u64, state: u8) -> Option<Self> {
        match (selector, state) {
            (0, 0) => Some(Self::Selector0State0),
            (0, 1) => Some(Self::Selector0State1),
            (1, 0) => Some(Self::Selector1State0),
            (2, 1) => Some(Self::Selector2State1),
            (4, 0) => Some(Self::Selector4State0),
            (4, 1) => Some(Self::Selector4State1),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct SketchPointClosureSerde {
    selector: u64,
    state: u8,
}

impl TryFrom<SketchPointClosureSerde> for SketchPointClosure {
    type Error = String;

    fn try_from(wire: SketchPointClosureSerde) -> Result<Self, Self::Error> {
        Self::from_pair(wire.selector, wire.state).ok_or_else(|| {
            format!(
                "sketch point closure selector {} state {} is not an admitted pair",
                wire.selector, wire.state
            )
        })
    }
}

impl From<SketchPointClosure> for SketchPointClosureSerde {
    fn from(closure: SketchPointClosure) -> Self {
        Self {
            selector: closure.selector(),
            state: closure.state(),
        }
    }
}

/// Version-10 same-segment closure: selector `0` and state `0` or `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointClosure10 {
    State0,
    State1,
}

impl SketchPointClosure10 {
    pub(crate) fn from_closure(closure: SketchPointClosure) -> Option<Self> {
        match closure {
            SketchPointClosure::Selector0State0 => Some(Self::State0),
            SketchPointClosure::Selector0State1 => Some(Self::State1),
            _ => None,
        }
    }

    fn to_closure(self) -> SketchPointClosure {
        match self {
            Self::State0 => SketchPointClosure::Selector0State0,
            Self::State1 => SketchPointClosure::Selector0State1,
        }
    }
}

/// Version-10 inline-typed closure: `(0, 0)`, `(0, 1)`, or `(2, 1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointClosure10Inline {
    Selector0State0,
    Selector0State1,
    Selector2State1,
}

impl SketchPointClosure10Inline {
    pub(crate) fn from_closure(closure: SketchPointClosure) -> Option<Self> {
        match closure {
            SketchPointClosure::Selector0State0 => Some(Self::Selector0State0),
            SketchPointClosure::Selector0State1 => Some(Self::Selector0State1),
            SketchPointClosure::Selector2State1 => Some(Self::Selector2State1),
            _ => None,
        }
    }

    fn to_closure(self) -> SketchPointClosure {
        match self {
            Self::Selector0State0 => SketchPointClosure::Selector0State0,
            Self::Selector0State1 => SketchPointClosure::Selector0State1,
            Self::Selector2State1 => SketchPointClosure::Selector2State1,
        }
    }
}

/// Serialized member sequence of one sketch-point record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SketchPointRecordForm<T = f64> {
    /// Class version 0: one flag, two coordinates, and no persistent identity.
    Version0 { flag: bool },
    /// Class version 8: seven flags and an eight-zero closure lane.
    Version8 {
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        /// Third sketch coordinate in millimetres.
        depth: T,
    },
    /// Class version 10 with same-segment references and seven flags.
    Version10 {
        /// Third sketch coordinate in millimetres.
        depth: T,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        closure: SketchPointClosure10,
    },
    /// Class version 10 with inline target-type GUIDs on its references.
    Version10InlineTyped {
        /// Third sketch coordinate in millimetres.
        depth: T,
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        closure: SketchPointClosure10Inline,
    },
    /// Class version 11 with same-segment references and eight flags.
    Version11 {
        /// Third sketch coordinate in millimetres.
        depth: T,
        /// Optional origin bitfield preceding the persistent identity.
        entity_genesis: Option<u64>,
        /// Whether four fixed zero bytes follow the repeated companion reference.
        padded_paired_reference: bool,
        /// Whether the record is stored as a derived sketch-point type whose
        /// primary frame carries the property block.
        derived_type: bool,
        /// Whether the paired companion record carries the fixed present-zero prefix.
        companion_prefix_present_zero: bool,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 8],
        closure: SketchPointClosure,
    },
    /// Class version 11 with inline target-type GUIDs on its references and eight flags.
    Version11InlineTyped {
        /// Third sketch coordinate in millimetres.
        depth: T,
        /// Optional origin bitfield preceding the persistent identity.
        entity_genesis: Option<u64>,
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        /// Whether the paired companion record carries the fixed present-zero prefix.
        companion_prefix_present_zero: bool,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 8],
        closure: SketchPointClosure,
    },
}

impl SketchPointRecordForm<f64> {
    #[cfg(test)]
    pub(crate) fn version11(
        persistent_id: u64,
        closure: SketchPointClosure,
        entity_genesis: Option<u64>,
        depth: f64,
    ) -> Self {
        Self::Version11 {
            depth,
            entity_genesis,
            padded_paired_reference: false,
            derived_type: false,
            companion_prefix_present_zero: false,
            persistent_id: std::num::NonZeroU64::new(persistent_id).unwrap(),
            flags: [false; 8],
            closure,
        }
    }
}

impl<T> SketchPointRecordForm<T> {
    fn try_map_depth<U, E>(
        self,
        mut map: impl FnMut(T) -> Result<U, E>,
    ) -> Result<SketchPointRecordForm<U>, E> {
        Ok(match self {
            Self::Version0 { flag } => SketchPointRecordForm::Version0 { flag },
            Self::Version8 {
                persistent_id,
                flags,
                depth: value,
            } => SketchPointRecordForm::Version8 {
                persistent_id,
                flags,
                depth: map(value)?,
            },
            Self::Version10 {
                persistent_id,
                flags,
                closure,
                depth: value,
            } => SketchPointRecordForm::Version10 {
                persistent_id,
                flags,
                closure,
                depth: map(value)?,
            },
            Self::Version10InlineTyped {
                persistent_id,
                flags,
                closure,
                trailing_reference,
                depth: value,
            } => SketchPointRecordForm::Version10InlineTyped {
                persistent_id,
                flags,
                closure,
                trailing_reference,
                depth: map(value)?,
            },
            Self::Version11 {
                persistent_id,
                flags,
                closure,
                entity_genesis,
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero,
                depth: value,
            } => SketchPointRecordForm::Version11 {
                persistent_id,
                flags,
                closure,
                entity_genesis,
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero,
                depth: map(value)?,
            },
            Self::Version11InlineTyped {
                persistent_id,
                flags,
                closure,
                entity_genesis,
                trailing_reference,
                companion_prefix_present_zero,
                depth: value,
            } => SketchPointRecordForm::Version11InlineTyped {
                persistent_id,
                flags,
                closure,
                entity_genesis,
                trailing_reference,
                companion_prefix_present_zero,
                depth: map(value)?,
            },
        })
    }
}

impl SketchPointRecordForm<f64> {
    fn try_checked(self) -> Result<SketchPointRecordForm<FiniteReal>, String> {
        self.try_map_depth(|value| {
            FiniteReal::new(value).ok_or_else(|| "sketch point depth must be finite".to_owned())
        })
    }
}

impl SketchPointRecordForm<FiniteReal> {
    #[cfg(test)]
    pub(crate) fn into_raw(self) -> SketchPointRecordForm<f64> {
        match self.try_map_depth(|value| Ok::<f64, std::convert::Infallible>(value.get())) {
            Ok(form) => form,
            Err(never) => match never {},
        }
    }
}

impl<T: Copy + Into<f64>> SketchPointRecordForm<T> {
    pub(crate) fn depth(&self) -> f64 {
        match *self {
            Self::Version0 { .. } => 0.0,
            Self::Version8 { depth, .. }
            | Self::Version10 { depth, .. }
            | Self::Version10InlineTyped { depth, .. }
            | Self::Version11 { depth, .. }
            | Self::Version11InlineTyped { depth, .. } => depth.into(),
        }
    }

    pub(crate) fn class_version(&self) -> u32 {
        match self {
            Self::Version0 { .. } => 0,
            Self::Version8 { .. } => 8,
            Self::Version10 { .. } | Self::Version10InlineTyped { .. } => 10,
            Self::Version11 { .. } | Self::Version11InlineTyped { .. } => 11,
        }
    }

    fn companion_prefix_present_zero(&self) -> bool {
        match *self {
            Self::Version11 {
                companion_prefix_present_zero,
                ..
            }
            | Self::Version11InlineTyped {
                companion_prefix_present_zero,
                ..
            } => companion_prefix_present_zero,
            Self::Version0 { .. }
            | Self::Version8 { .. }
            | Self::Version10 { .. }
            | Self::Version10InlineTyped { .. } => false,
        }
    }

    /// Return this form with the companion prefix observed on the paired record,
    /// or `None` when the form has no member for a present-zero prefix.
    pub(crate) fn with_companion_prefix_present_zero(self, present_zero: bool) -> Option<Self> {
        match self {
            Self::Version11 {
                depth,
                entity_genesis,
                padded_paired_reference,
                derived_type,
                persistent_id,
                flags,
                closure,
                ..
            } => Some(Self::Version11 {
                depth,
                entity_genesis,
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero: present_zero,
                persistent_id,
                flags,
                closure,
            }),
            Self::Version11InlineTyped {
                depth,
                entity_genesis,
                trailing_reference,
                persistent_id,
                flags,
                closure,
                ..
            } => Some(Self::Version11InlineTyped {
                depth,
                entity_genesis,
                trailing_reference,
                companion_prefix_present_zero: present_zero,
                persistent_id,
                flags,
                closure,
            }),
            other => (!present_zero).then_some(other),
        }
    }

    pub(crate) fn uses_inline_typed_references(&self) -> bool {
        matches!(
            self,
            Self::Version10InlineTyped { .. } | Self::Version11InlineTyped { .. }
        )
    }

    pub(crate) fn persistent_id(&self) -> Option<u64> {
        match *self {
            Self::Version0 { .. } => None,
            Self::Version8 { persistent_id, .. }
            | Self::Version10 { persistent_id, .. }
            | Self::Version10InlineTyped { persistent_id, .. }
            | Self::Version11 { persistent_id, .. }
            | Self::Version11InlineTyped { persistent_id, .. } => Some(persistent_id.get()),
        }
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        let mut flags = [0; 8];
        match self {
            Self::Version0 { flag, .. } => flags[0] = u8::from(*flag),
            Self::Version8 { flags: source, .. }
            | Self::Version10 { flags: source, .. }
            | Self::Version10InlineTyped { flags: source, .. } => {
                flags[..7].copy_from_slice(&source.map(u8::from));
            }
            Self::Version11 { flags: source, .. }
            | Self::Version11InlineTyped { flags: source, .. } => flags = source.map(u8::from),
        }
        flags
    }

    pub(crate) fn closure(&self) -> Option<SketchPointClosure> {
        match *self {
            Self::Version0 { .. } => None,
            Self::Version8 { .. } => Some(SketchPointClosure::Selector0State0),
            Self::Version10 { closure, .. } => Some(closure.to_closure()),
            Self::Version10InlineTyped { closure, .. } => Some(closure.to_closure()),
            Self::Version11 { closure, .. } | Self::Version11InlineTyped { closure, .. } => {
                Some(closure)
            }
        }
    }
}

/// Encoding of every reference owned by a point companion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointCompanionReferenceEncoding {
    /// Target entity ID followed directly by the same-segment flags.
    SameSegment,
    /// Target entity ID followed by the target type GUID and same-segment flags.
    InlineTyped,
}

impl SketchPointCompanionReferenceEncoding {
    /// Return the reference encoding the point record form dictates.
    pub(crate) fn for_form(form: &SketchPointRecordForm) -> Self {
        if form.uses_inline_typed_references() {
            Self::InlineTyped
        } else {
            Self::SameSegment
        }
    }
}

/// Reverse curve-incidence record paired with a version-11 sketch point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SketchPointCompanion {
    /// Incident sketch-curve record indexes in serialized order.
    pub(crate) incident_curves: Vec<u32>,
}

impl SketchPointCompanion {
    fn validate(&self) -> Result<(), String> {
        let unique: std::collections::HashSet<_> = self.incident_curves.iter().collect();
        if unique.len() != self.incident_curves.len() {
            return Err("sketch point companion.incident_curves must be distinct".into());
        }
        Ok(())
    }
}

/// Borrowed companion payload with the prefix derived for older point forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SketchPointCompanionRef<'a> {
    pub(crate) prefix_present_zero: bool,
    pub(crate) incident_curves: &'a [u32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SketchPointCompanionWire {
    #[serde(default)]
    incident_curves: Vec<u32>,
}

// Serde requires `skip_serializing_if` predicates to borrow the field.
fn sketch_point_flags_are_zero<T: AsRef<[u8]>>(flags: &T) -> bool {
    flags.as_ref().iter().all(|flag| *flag == 0)
}

/// One point in a Fusion sketch coordinate system.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "SketchPointSerde")]
pub(crate) struct SketchPoint {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this point record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Resolved owning-sketch reference from a direct backlink, typed relation,
    /// or sketch-container member run.
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this point's record type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub(crate) coordinate_offset: u32,
    /// Serialized point-record member sequence, identity, flags, and closure.
    record_form: SketchPointRecordForm<FiniteReal>,
    companion: SketchPointCompanion,
    /// Record index of the paired reverse curve-incidence companion.
    pub(crate) paired_reference: u32,
    /// First two sketch coordinates in millimetres.
    coordinates: FinitePoint2,
}

#[cfg(test)]
thread_local! {
    static SKETCH_POINT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for SketchPoint {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SKETCH_POINT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_index: self.record_index,
            owner_reference: self.owner_reference,
            class_tag: self.class_tag.clone(),
            byte_offset: self.byte_offset,
            coordinate_offset: self.coordinate_offset,
            record_form: self.record_form.clone(),
            companion: self.companion.clone(),
            paired_reference: self.paired_reference,
            coordinates: self.coordinates,
        }
    }
}

impl Serialize for SketchPoint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct CompanionRef<'a> {
            incident_curves: &'a [u32],
        }
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            record_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            owner_reference: Option<u32>,
            class_tag: &'a str,
            byte_offset: u64,
            coordinate_offset: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            entity_genesis: Option<u64>,
            record_form: SketchPointRecordFormSerde,
            #[serde(skip_serializing_if = "Option::is_none")]
            persistent_id: Option<u64>,
            paired_reference: u32,
            #[serde(skip_serializing_if = "sketch_point_flags_are_zero")]
            flags: NativeBytes<[u8; 8]>,
            coordinates: Point2,
            depth: f64,
            #[serde(skip_serializing_if = "Option::is_none")]
            closure: Option<SketchPointClosureSerde>,
            companion: CompanionRef<'a>,
        }
        let record_form = match self.record_form {
            SketchPointRecordForm::Version0 { .. } => SketchPointRecordFormSerde::Version0,
            SketchPointRecordForm::Version8 { .. } => SketchPointRecordFormSerde::Version8,
            SketchPointRecordForm::Version10 { .. } => SketchPointRecordFormSerde::Version10,
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            } => SketchPointRecordFormSerde::Version10InlineTyped { trailing_reference },
            SketchPointRecordForm::Version11 {
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero,
                ..
            } => SketchPointRecordFormSerde::Version11 {
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero,
            },
            SketchPointRecordForm::Version11InlineTyped {
                trailing_reference,
                companion_prefix_present_zero,
                ..
            } => SketchPointRecordFormSerde::Version11InlineTyped {
                trailing_reference,
                companion_prefix_present_zero,
            },
        };
        WireRef {
            id: &self.id,
            record_index: self.record_index,
            owner_reference: self.owner_reference,
            class_tag: self.class_tag.as_str(),
            byte_offset: self.byte_offset,
            coordinate_offset: self.coordinate_offset,
            entity_genesis: self.entity_genesis(),
            record_form,
            persistent_id: self.persistent_id(),
            paired_reference: self.paired_reference,
            flags: (self.flags()).into(),
            coordinates: self.coordinates.get(),
            depth: self.depth(),
            closure: self.closure().map(SketchPointClosureSerde::from),
            companion: CompanionRef {
                incident_curves: &self.companion.incident_curves,
            },
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SketchPointDraft {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this point record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Resolved owning-sketch reference from a direct backlink, typed relation,
    /// or sketch-container member run.
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this point's record type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub(crate) coordinate_offset: u32,
    /// Serialized point-record member sequence, identity, flags, and closure.
    pub(crate) record_form: SketchPointRecordForm,
    pub(crate) companion: SketchPointCompanion,
    /// Record index of the paired reverse curve-incidence companion.
    pub(crate) paired_reference: u32,
    /// First two sketch coordinates in millimetres.
    pub(crate) coordinates: Point2,
}

impl TryFrom<SketchPointDraft> for SketchPoint {
    type Error = String;
    fn try_from(draft: SketchPointDraft) -> Result<Self, Self::Error> {
        let coordinates = FinitePoint2::new(draft.coordinates)
            .ok_or_else(|| "sketch point coordinates must be finite".to_owned())?;
        let record_form = draft.record_form.clone().try_checked()?;
        draft.companion.validate()?;
        Ok(Self::from_validated(draft, coordinates, record_form))
    }
}

impl SketchPoint {
    fn from_validated(
        draft: SketchPointDraft,
        coordinates: FinitePoint2,
        record_form: SketchPointRecordForm<FiniteReal>,
    ) -> Self {
        Self {
            id: draft.id,
            record_index: draft.record_index,
            owner_reference: draft.owner_reference,
            class_tag: draft.class_tag,
            byte_offset: draft.byte_offset,
            coordinate_offset: draft.coordinate_offset,
            record_form,
            companion: draft.companion,
            paired_reference: draft.paired_reference,
            coordinates,
        }
    }
}

impl SketchPoint {
    pub(crate) fn validate_for_write(&self, curves: &[SketchCurveIdentity]) -> Result<(), String> {
        const CENTER_RELATIVE_TOLERANCE: f64 = 1e-7;
        let flags = self.flags();
        if self.record_form.class_version() == 0 {
            return Ok(());
        }
        if flags[2] != 0 {
            return Err("sketch point reserved flag lane 2 must be zero".into());
        }
        let coordinates = self.coordinates();
        let position = [coordinates.u, coordinates.v, self.depth()];
        if flags[5] != 0 && (flags[3] == 0 || position.iter().any(|value| value.to_bits() != 0)) {
            return Err(
                "sketch point flag lane 5 requires lane 3 and positive-zero coordinates".into(),
            );
        }
        if flags[7] == 0 {
            return Ok(());
        }
        if self.record_form.class_version() != 11
            || self.closure() != Some(SketchPointClosure::Selector0State1)
            || self.companion.incident_curves.len() != 1
        {
            return Err("sketch point flag lane 7 requires version 11, closure (0,1), and one incident curve".into());
        }
        let stream = self.id.rsplit_once(':').map(|(prefix, _)| prefix);
        let mut incident = curves.iter().filter(|curve| {
            curve.record_index == self.companion.incident_curves[0]
                && curve.id.rsplit_once(':').map(|(prefix, _)| prefix) == stream
        });
        let curve = incident.next().ok_or_else(|| {
            "sketch point flag lane 7 has no matching incident circular curve".to_owned()
        })?;
        if incident.next().is_some() {
            return Err("sketch point flag lane 7 has an ambiguous incident curve".into());
        }
        let Some(SketchCurveGeometry::Arc { center, .. }) = &curve.geometry else {
            return Err("sketch point flag lane 7 requires an incident circular curve".into());
        };
        let center = center.as_raw();
        let expected = [center.x, center.y, center.z];
        if position.iter().zip(expected).any(|(actual, expected)| {
            (actual - expected).abs()
                > CENTER_RELATIVE_TOLERANCE * (1.0 + actual.abs().max(expected.abs()))
        }) {
            return Err("sketch point flag lane 7 requires the incident circular center".into());
        }
        Ok(())
    }

    pub(crate) fn coordinates(&self) -> Point2 {
        self.coordinates.get()
    }
    pub(crate) fn record_form(&self) -> &SketchPointRecordForm<FiniteReal> {
        &self.record_form
    }
    pub(crate) fn try_set_coordinates(&mut self, coordinates: Point2) -> Result<(), String> {
        self.coordinates = FinitePoint2::new(coordinates)
            .ok_or_else(|| "sketch point coordinates must be finite".to_owned())?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn try_set_record_form(
        &mut self,
        record_form: SketchPointRecordForm,
    ) -> Result<(), String> {
        self.record_form = record_form.try_checked()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn try_set_companion(
        &mut self,
        companion: SketchPointCompanion,
    ) -> Result<(), String> {
        companion.validate()?;
        self.companion = companion;
        Ok(())
    }
    pub(crate) fn companion(&self) -> SketchPointCompanionRef<'_> {
        SketchPointCompanionRef {
            prefix_present_zero: self.record_form.companion_prefix_present_zero(),
            incident_curves: &self.companion.incident_curves,
        }
    }

    pub(crate) fn depth(&self) -> f64 {
        self.record_form.depth()
    }

    pub(crate) fn entity_genesis(&self) -> Option<u64> {
        match self.record_form {
            SketchPointRecordForm::Version11 { entity_genesis, .. }
            | SketchPointRecordForm::Version11InlineTyped { entity_genesis, .. } => entity_genesis,
            SketchPointRecordForm::Version0 { .. }
            | SketchPointRecordForm::Version8 { .. }
            | SketchPointRecordForm::Version10 { .. }
            | SketchPointRecordForm::Version10InlineTyped { .. } => None,
        }
    }

    pub(crate) fn persistent_id(&self) -> Option<u64> {
        self.record_form.persistent_id()
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        self.record_form.flags()
    }

    pub(crate) fn closure(&self) -> Option<SketchPointClosure> {
        self.record_form.closure()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SketchPointRecordFormSerde {
    Version0,
    Version8,
    Version10,
    Version10InlineTyped {
        trailing_reference: u32,
    },
    Version11 {
        padded_paired_reference: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        derived_type: bool,
        #[serde(default)]
        companion_prefix_present_zero: bool,
    },
    Version11InlineTyped {
        trailing_reference: u32,
        #[serde(default)]
        companion_prefix_present_zero: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct SketchPointSerde {
    id: String,
    record_index: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_reference"
    )]
    owner_reference: Option<u32>,
    class_tag: String,
    byte_offset: u64,
    coordinate_offset: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    entity_genesis: Option<u64>,
    #[serde(default)]
    record_form: SketchPointRecordFormSerde,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_persistent_id"
    )]
    persistent_id: Option<u64>,
    paired_reference: u32,
    #[serde(default, skip_serializing_if = "sketch_point_flags_are_zero")]
    flags: NativeBytes<[u8; 8]>,
    coordinates: Point2,
    #[serde(default)]
    pub(super) depth: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_closure"
    )]
    closure: Option<SketchPointClosureSerde>,
    companion: SketchPointCompanionWire,
}

impl Default for SketchPointRecordFormSerde {
    fn default() -> Self {
        Self::Version11 {
            padded_paired_reference: false,
            derived_type: false,
            companion_prefix_present_zero: false,
        }
    }
}

fn seven_flags(flags: [bool; 8]) -> Result<[bool; 7], String> {
    if flags[7] {
        return Err("sketch point flags beyond the form width must be zero".into());
    }
    let mut dest = [false; 7];
    dest.copy_from_slice(&flags[..7]);
    Ok(dest)
}

impl TryFrom<SketchPointSerde> for SketchPoint {
    type Error = String;

    fn try_from(wire: SketchPointSerde) -> Result<Self, Self::Error> {
        if wire.flags.iter().any(|flag| *flag > 1) {
            return Err("sketch point flags must be zero or one".into());
        }
        if wire.entity_genesis.is_some()
            && !matches!(
                wire.record_form,
                SketchPointRecordFormSerde::Version11 { .. }
                    | SketchPointRecordFormSerde::Version11InlineTyped { .. }
            )
        {
            return Err("sketch point entity_genesis requires version 11".into());
        }
        if matches!(wire.record_form, SketchPointRecordFormSerde::Version0) && wire.depth != 0.0 {
            return Err("sketch point depth must be zero for version 0".into());
        }
        let flags = wire.flags.map(|flag| flag == 1);
        let closure = wire.closure.map(SketchPointClosure::try_from).transpose()?;
        let persistent_id = wire
            .persistent_id
            .map(|value| std::num::NonZeroU64::new(value).ok_or("persistent_id must be nonzero"))
            .transpose()?;
        let record_form = match (wire.record_form, persistent_id, closure) {
            (SketchPointRecordFormSerde::Version0, None, None) => {
                SketchPointRecordForm::Version0 { flag: flags[0] }
            }
            (
                SketchPointRecordFormSerde::Version8,
                Some(persistent_id),
                Some(SketchPointClosure::Selector0State0),
            ) => SketchPointRecordForm::Version8 {
                depth: wire.depth,
                persistent_id,
                flags: seven_flags(flags)?,
            },
            (SketchPointRecordFormSerde::Version10, Some(persistent_id), Some(closure)) => {
                SketchPointRecordForm::Version10 {
                    depth: wire.depth,
                    persistent_id,
                    flags: seven_flags(flags)?,
                    closure: SketchPointClosure10::from_closure(closure).ok_or_else(|| {
                        "sketch point version-10 closure must be selector 0 with state 0 or 1"
                            .to_string()
                    })?,
                }
            }
            (
                SketchPointRecordFormSerde::Version10InlineTyped { trailing_reference },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version10InlineTyped {
                depth: wire.depth,
                trailing_reference,
                persistent_id,
                flags: seven_flags(flags)?,
                closure: SketchPointClosure10Inline::from_closure(closure).ok_or_else(|| {
                    "sketch point version-10 inline closure must be (0,0), (0,1), or (2,1)"
                        .to_string()
                })?,
            },
            (
                SketchPointRecordFormSerde::Version11 {
                    padded_paired_reference,
                    derived_type,
                    companion_prefix_present_zero,
                },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version11 {
                depth: wire.depth,
                entity_genesis: wire.entity_genesis,
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero,
                persistent_id,
                flags,
                closure,
            },
            (
                SketchPointRecordFormSerde::Version11InlineTyped {
                    trailing_reference,
                    companion_prefix_present_zero,
                },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version11InlineTyped {
                depth: wire.depth,
                entity_genesis: wire.entity_genesis,
                trailing_reference,
                companion_prefix_present_zero,
                persistent_id,
                flags,
                closure,
            },
            _ => {
                return Err(
                    "sketch point record_form disagrees with persistent_id or closure".into(),
                );
            }
        };
        if matches!(record_form, SketchPointRecordForm::Version0 { .. })
            && wire.flags[1..].iter().any(|flag| *flag != 0)
        {
            return Err("sketch point flags beyond the form width must be zero".into());
        }
        let companion = SketchPointCompanion {
            incident_curves: wire.companion.incident_curves,
        };
        Self::try_from(SketchPointDraft {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag.try_into()?,
            byte_offset: wire.byte_offset,
            coordinate_offset: wire.coordinate_offset,
            record_form,
            companion,
            paired_reference: wire.paired_reference,
            coordinates: wire.coordinates,
        })
    }
}

#[cfg(test)]
impl From<SketchPoint> for SketchPointSerde {
    fn from(point: SketchPoint) -> Self {
        let depth = point.depth();
        let entity_genesis = point.entity_genesis();
        let persistent_id = point.persistent_id();
        let flags = point.flags();
        let closure = point.closure().map(SketchPointClosureSerde::from);
        let record_form = match point.record_form {
            SketchPointRecordForm::Version0 { .. } => SketchPointRecordFormSerde::Version0,
            SketchPointRecordForm::Version8 { .. } => SketchPointRecordFormSerde::Version8,
            SketchPointRecordForm::Version10 { .. } => SketchPointRecordFormSerde::Version10,
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            } => SketchPointRecordFormSerde::Version10InlineTyped { trailing_reference },
            SketchPointRecordForm::Version11 {
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero,
                ..
            } => SketchPointRecordFormSerde::Version11 {
                padded_paired_reference,
                derived_type,
                companion_prefix_present_zero,
            },
            SketchPointRecordForm::Version11InlineTyped {
                trailing_reference,
                companion_prefix_present_zero,
                ..
            } => SketchPointRecordFormSerde::Version11InlineTyped {
                trailing_reference,
                companion_prefix_present_zero,
            },
        };
        let companion = SketchPointCompanionWire {
            incident_curves: point.companion.incident_curves,
        };
        Self {
            id: point.id,
            record_index: point.record_index,
            owner_reference: point.owner_reference,
            class_tag: point.class_tag.into(),
            byte_offset: point.byte_offset,
            coordinate_offset: point.coordinate_offset,
            entity_genesis,
            record_form,
            persistent_id,
            paired_reference: point.paired_reference,
            flags: (flags).into(),
            coordinates: point.coordinates.get(),
            depth,
            closure,
            companion,
        }
    }
}

/// Persistent identity pair attached to one source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SketchCurveIdentity {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this identity record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Direct owning-sketch backlink when the curve record form carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_reference"
    )]
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the fixed analytic geometry payload relative to the record start.
    pub(crate) geometry_offset: u32,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the curve identities.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    pub(crate) entity_genesis: Option<u64>,
    /// Primary persistent identifier of the source sketch curve.
    pub(crate) primary_id: std::num::NonZeroU64,
    /// Secondary persistent identifier of the source sketch curve (e.g. its
    /// complementary endpoint or paired-curve identity).
    pub(crate) secondary_id: u64,
    /// Exact analytic geometry carried by this sketch-curve record, when the
    /// decoder recovered one; `None` when the geometry subtype was not decoded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_geometry"
    )]
    pub(crate) geometry: Option<SketchCurveGeometry>,
    /// Raw z components of a compact planar line's start, displacement, and
    /// direction when at least one is nonzero floating-point noise that the
    /// projection reads as zero.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_planar_z_noise"
    )]
    pub(crate) planar_z_noise: Option<[f64; 3]>,
}

/// One persistent tensor-product surface owned by a spatial Fusion sketch.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "SketchSurfaceWire")]
pub(crate) struct SketchSurface {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this surface record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Owning sketch entity derived from relations using this surface.
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the surface identity.
    pub(crate) entity_genesis: Option<u64>,
    /// Persistent Fusion identifier for the sketch surface.
    pub(crate) persistent_id: std::num::NonZeroU64,
    /// Admitted tensor-product geometry.
    pub(crate) geometry: SketchSurfaceGeometry,
}

#[cfg(test)]
thread_local! {
    static SKETCH_SURFACE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for SketchSurface {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SKETCH_SURFACE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_index: self.record_index,
            owner_reference: self.owner_reference,
            class_tag: self.class_tag.clone(),
            byte_offset: self.byte_offset,
            entity_genesis: self.entity_genesis,
            persistent_id: self.persistent_id,
            geometry: self.geometry.clone(),
        }
    }
}

struct SurfaceControlRow<'a>(&'a [FinitePoint3]);

impl Serialize for SurfaceControlRow<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|point| point.get()))
    }
}

struct SurfaceControlGrid<'a>(&'a [Vec<FinitePoint3>]);

impl Serialize for SurfaceControlGrid<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|row| SurfaceControlRow(row)))
    }
}

impl Serialize for SketchSurface {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            record_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            owner_reference: Option<u32>,
            class_tag: &'a DesignClassTag,
            byte_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            entity_genesis: Option<u64>,
            persistent_id: std::num::NonZeroU64,
            u_degree: u32,
            v_degree: u32,
            u_knots: SliceColumn<'a, FiniteReal, f64>,
            v_knots: SliceColumn<'a, FiniteReal, f64>,
            control_points: SurfaceControlGrid<'a>,
        }
        WireRef {
            id: &self.id,
            record_index: self.record_index,
            owner_reference: self.owner_reference,
            class_tag: &self.class_tag,
            byte_offset: self.byte_offset,
            entity_genesis: self.entity_genesis,
            persistent_id: self.persistent_id,
            u_degree: self.geometry.u_degree.get(),
            v_degree: self.geometry.v_degree.get(),
            u_knots: SliceColumn::new(&self.geometry.u_knots, |knot| knot.get()),
            v_knots: SliceColumn::new(&self.geometry.v_knots, |knot| knot.get()),
            control_points: SurfaceControlGrid(&self.geometry.control_points),
        }
        .serialize(serializer)
    }
}

/// Positive degrees, ordered knots, and a finite rectangular control grid.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchSurfaceGeometry {
    pub(crate) u_degree: std::num::NonZeroU32,
    pub(crate) v_degree: std::num::NonZeroU32,
    pub(crate) u_knots: Vec<FiniteReal>,
    pub(crate) v_knots: Vec<FiniteReal>,
    pub(crate) control_points: Vec<Vec<FinitePoint3>>,
}

/// Whether adjacent knots never decrease. A NaN pair fails this predicate.
fn knots_nondecreasing(knots: &[f64]) -> bool {
    knots.windows(2).all(|pair| pair[0] <= pair[1])
}

impl SketchSurfaceGeometry {
    pub(crate) fn from_parts(
        u_degree: u32,
        v_degree: u32,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        control_points: Vec<Vec<Point3>>,
    ) -> Result<Self, String> {
        let control_points = control_points
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|point| {
                        FinitePoint3::new(point).ok_or("surface point is not finite".into())
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .collect::<Result<Vec<_>, String>>()?;
        Self::from_checked_parts(u_degree, v_degree, u_knots, v_knots, control_points)
            .map_err(ToString::to_string)
    }

    pub(crate) fn from_checked_parts(
        u_degree: u32,
        v_degree: u32,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        control_points: Vec<Vec<FinitePoint3>>,
    ) -> Result<Self, &'static str> {
        let u_degree = std::num::NonZeroU32::new(u_degree).ok_or("surface u_degree is zero")?;
        let v_degree = std::num::NonZeroU32::new(v_degree).ok_or("surface v_degree is zero")?;
        let row_count = control_points.len();
        let column_count = control_points.first().map_or(0, Vec::len);
        let point_count = row_count
            .checked_mul(column_count)
            .ok_or("surface control grid exceeds address space")?;
        if row_count == 0 || column_count == 0 || point_count > 100_000 {
            return Err("surface control grid must be nonempty and rectangular");
        }
        for row in &control_points {
            if row.len() != column_count {
                return Err("surface control grid must be nonempty and rectangular");
            }
        }
        let expected_u_knots = row_count
            .checked_add(
                usize::try_from(u_degree.get())
                    .map_err(|_| "surface u_degree exceeds address space")?,
            )
            .and_then(|count| count.checked_add(1))
            .ok_or("surface u knot count overflows")?;
        let expected_v_knots = column_count
            .checked_add(
                usize::try_from(v_degree.get())
                    .map_err(|_| "surface v_degree exceeds address space")?,
            )
            .and_then(|count| count.checked_add(1))
            .ok_or("surface v knot count overflows")?;
        if u_knots.len() != expected_u_knots || v_knots.len() != expected_v_knots {
            return Err("surface knot counts disagree with degrees and grid");
        }
        if !knots_nondecreasing(&u_knots) || !knots_nondecreasing(&v_knots) {
            return Err("surface knots must be nondecreasing");
        }
        let mut finite_u_knots = Vec::new();
        for value in u_knots {
            finite_u_knots.push(FiniteReal::new(value).ok_or("surface u knot is not finite")?);
        }
        let mut finite_v_knots = Vec::new();
        for value in v_knots {
            finite_v_knots.push(FiniteReal::new(value).ok_or("surface v knot is not finite")?);
        }
        Ok(Self {
            u_degree,
            v_degree,
            u_knots: finite_u_knots,
            v_knots: finite_v_knots,
            control_points,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SketchSurfaceWire {
    id: String,
    record_index: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_reference"
    )]
    owner_reference: Option<u32>,
    class_tag: DesignClassTag,
    byte_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    entity_genesis: Option<u64>,
    persistent_id: std::num::NonZeroU64,
    u_degree: u32,
    v_degree: u32,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    control_points: Vec<Vec<Point3>>,
}

impl TryFrom<SketchSurfaceWire> for SketchSurface {
    type Error = String;

    fn try_from(wire: SketchSurfaceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag,
            byte_offset: wire.byte_offset,
            entity_genesis: wire.entity_genesis,
            persistent_id: wire.persistent_id,
            geometry: SketchSurfaceGeometry::from_parts(
                wire.u_degree,
                wire.v_degree,
                wire.u_knots,
                wire.v_knots,
                wire.control_points,
            )?,
        })
    }
}

#[cfg(test)]
impl From<SketchSurface> for SketchSurfaceWire {
    fn from(surface: SketchSurface) -> Self {
        Self {
            id: surface.id,
            record_index: surface.record_index,
            owner_reference: surface.owner_reference,
            class_tag: surface.class_tag,
            byte_offset: surface.byte_offset,
            entity_genesis: surface.entity_genesis,
            persistent_id: surface.persistent_id,
            u_degree: surface.geometry.u_degree.get(),
            v_degree: surface.geometry.v_degree.get(),
            u_knots: surface
                .geometry
                .u_knots
                .into_iter()
                .map(FiniteReal::get)
                .collect(),
            v_knots: surface
                .geometry
                .v_knots
                .into_iter()
                .map(FiniteReal::get)
                .collect(),
            control_points: surface
                .geometry
                .control_points
                .into_iter()
                .map(|row| row.into_iter().map(FinitePoint3::get).collect())
                .collect(),
        }
    }
}

/// Exact analytic geometry carried by a source sketch-curve record.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "SketchCurveGeometryWire")]
pub(crate) enum SketchCurveGeometry {
    /// A straight line segment.
    Line {
        /// Start point in sketch space, millimetres.
        start: FinitePoint3,
        /// End point in sketch space, millimetres.
        end: FinitePoint3,
        /// Direction from `start` to `end`; the decoder stores it at unit length.
        direction: UnitVector3,
        /// Normal of the sketch plane the line lies in; the decoder stores it at
        /// unit length.
        normal: UnitVector3,
    },
    /// A circular arc.
    Arc {
        /// Arc center in sketch space, millimetres.
        center: FinitePoint3,
        /// Normal of the sketch plane the arc lies in; the decoder admits a norm
        /// within `1e-9` of one.
        normal: UnitVector3,
        /// Zero-angle direction for `start_angle`/`end_angle`; the decoder admits a
        /// norm within `1e-9` of one.
        reference_direction: UnitVector3,
        /// Arc radius in millimetres.
        radius: PositiveLength,
        /// Start angle in radians, measured from `reference_direction`.
        start_angle: Angle,
        /// End angle in radians, measured from `reference_direction`.
        end_angle: Angle,
    },
    /// A NURBS (procedural spline) curve.
    Nurbs {
        /// Record index of the underlying carrier geometry, when the NURBS record
        /// references one; `None` when the control data is self-contained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carrier_reference: Option<u64>,
        /// Source per-file dynamic three-digit ASCII class tag naming the NURBS subtype.
        subtype_class_tag: DesignClassTag,
        /// Record index of the NURBS subtype record.
        subtype_record_index: u32,
        /// Admitted degree, fit tolerance, knots and poles; source scalar width is eight.
        geometry: SketchNurbsGeometry,
    },
}

#[cfg(test)]
thread_local! {
    static SKETCH_CURVE_GEOMETRY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for SketchCurveGeometry {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SKETCH_CURVE_GEOMETRY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        match self {
            Self::Line {
                start,
                end,
                direction,
                normal,
            } => Self::Line {
                start: *start,
                end: *end,
                direction: *direction,
                normal: *normal,
            },
            Self::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Self::Arc {
                center: *center,
                normal: *normal,
                reference_direction: *reference_direction,
                radius: *radius,
                start_angle: *start_angle,
                end_angle: *end_angle,
            },
            Self::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                geometry,
            } => Self::Nurbs {
                carrier_reference: *carrier_reference,
                subtype_class_tag: subtype_class_tag.clone(),
                subtype_record_index: *subtype_record_index,
                geometry: geometry.clone(),
            },
        }
    }
}

struct NurbsPoints<'a>(&'a SketchNurbsPoles);

impl Serialize for NurbsPoints<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.points())
    }
}

struct NurbsWeights<'a>(&'a SketchNurbsPoles);

impl Serialize for NurbsWeights<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.weights())
    }
}

impl Serialize for SketchCurveGeometry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum WireRef<'a> {
            Line {
                start: Point3,
                end: Point3,
                direction: Vector3,
                normal: Vector3,
            },
            Arc {
                center: Point3,
                normal: Vector3,
                reference_direction: Vector3,
                radius: f64,
                start_angle: f64,
                end_angle: f64,
            },
            Nurbs {
                #[serde(skip_serializing_if = "Option::is_none")]
                carrier_reference: Option<u64>,
                subtype_class_tag: &'a str,
                subtype_record_index: u32,
                degree: u32,
                fit_tolerance: f64,
                scalar_width: u32,
                knots: &'a [f64],
                weights: NurbsWeights<'a>,
                control_points: NurbsPoints<'a>,
            },
        }
        let view = match self {
            Self::Line {
                start,
                end,
                direction,
                normal,
            } => WireRef::Line {
                start: start.get(),
                end: end.get(),
                direction: *direction.as_raw(),
                normal: *normal.as_raw(),
            },
            Self::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => WireRef::Arc {
                center: center.get(),
                normal: *normal.as_raw(),
                reference_direction: *reference_direction.as_raw(),
                radius: radius.get(),
                start_angle: start_angle.get(),
                end_angle: end_angle.get(),
            },
            Self::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                geometry,
            } => WireRef::Nurbs {
                carrier_reference: *carrier_reference,
                subtype_class_tag: subtype_class_tag.as_str(),
                subtype_record_index: *subtype_record_index,
                degree: geometry.degree,
                fit_tolerance: geometry.fit_tolerance.get(),
                scalar_width: 8,
                knots: &geometry.knots,
                weights: NurbsWeights(&geometry.poles),
                control_points: NurbsPoints(&geometry.poles),
            },
        };
        view.serialize(serializer)
    }
}

const EPS_SKETCH_LINE_FRAME: f64 = 1.0e-9;
const EPS_SKETCH_ARC_FRAME: f64 = 1.0e-9;
const EPS_SKETCH_ARC_SWEEP: f64 = 1.0e-12;

impl SketchCurveGeometry {
    pub(crate) fn line_from_parts(
        start: FinitePoint3,
        end: FinitePoint3,
        direction: UnitVector3,
        normal: UnitVector3,
    ) -> Result<Self, String> {
        let start_raw = start.get();
        let end_raw = end.get();
        let displacement = FiniteVector3::new(Vector3::new(
            end_raw.x - start_raw.x,
            end_raw.y - start_raw.y,
            end_raw.z - start_raw.z,
        ))
        .ok_or("line endpoint displacement overflows")?;
        let displacement_raw = displacement.get();
        let source_displacement = FiniteVector3::new(Vector3::new(
            displacement_raw.x / 10.0,
            displacement_raw.y / 10.0,
            displacement_raw.z / 10.0,
        ))
        .ok_or("line source-unit displacement overflows")?;
        let canonical = UnitVector3::normalized(source_displacement.get())
            .ok_or("line endpoint displacement is zero in source units")?;
        let difference = *direction.as_raw() - *canonical.as_raw();
        if difference.norm() > EPS_SKETCH_LINE_FRAME {
            return Err("line direction disagrees with endpoints".into());
        }
        if canonical.as_raw().dot(*normal.as_raw()).abs() > EPS_SKETCH_LINE_FRAME {
            return Err("line normal is not perpendicular to its direction".into());
        }
        Ok(Self::Line {
            start,
            end,
            direction: canonical,
            normal,
        })
    }

    pub(crate) fn line(
        start: Point3,
        end: Point3,
        direction: Vector3,
        normal: Vector3,
    ) -> Result<Self, String> {
        Self::line_from_parts(
            FinitePoint3::new(start).ok_or("line start is not finite")?,
            FinitePoint3::new(end).ok_or("line end is not finite")?,
            UnitVector3::new(direction).ok_or("line direction is not unit")?,
            UnitVector3::new(normal).ok_or("line normal is not unit")?,
        )
    }

    pub(crate) fn arc_from_parts(
        center: FinitePoint3,
        normal: UnitVector3,
        reference_direction: UnitVector3,
        radius: PositiveLength,
        start_angle: Angle,
        end_angle: Angle,
    ) -> Result<Self, String> {
        let normal_raw = normal.as_raw();
        let reference_raw = reference_direction.as_raw();
        let dot = normal_raw.x * reference_raw.x
            + normal_raw.y * reference_raw.y
            + normal_raw.z * reference_raw.z;
        if dot.abs() > EPS_SKETCH_ARC_FRAME {
            return Err("arc frame directions are not perpendicular".into());
        }
        if start_angle.get().abs() > std::f64::consts::TAU + EPS_SKETCH_ARC_FRAME
            || end_angle.get().abs() > std::f64::consts::TAU + EPS_SKETCH_ARC_FRAME
            || (end_angle.get() - start_angle.get()).abs() < EPS_SKETCH_ARC_SWEEP
        {
            return Err("arc sweep is outside the source domain".into());
        }
        Ok(Self::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        })
    }

    pub(crate) fn arc(
        center: Point3,
        normal: Vector3,
        reference_direction: Vector3,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> Result<Self, String> {
        Self::arc_from_parts(
            FinitePoint3::new(center).ok_or("arc center is not finite")?,
            UnitVector3::new(normal).ok_or("arc normal is not unit")?,
            UnitVector3::new(reference_direction).ok_or("arc reference direction is not unit")?,
            PositiveLength::new(radius).ok_or("arc radius must be positive and finite")?,
            Angle::new(start_angle).ok_or("arc start angle is not finite")?,
            Angle::new(end_angle).ok_or("arc end angle is not finite")?,
        )
    }

    pub(crate) fn nurbs_from_parts(
        carrier_reference: Option<u64>,
        subtype_class_tag: DesignClassTag,
        subtype_record_index: u32,
        geometry: SketchNurbsGeometry,
    ) -> Self {
        Self::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index,
            geometry,
        }
    }
}

/// Control lanes and their shared source and native admission for one sketch spline.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchNurbsGeometry {
    degree: u32,
    fit_tolerance: NonNegativeLength,
    knots: Vec<f64>,
    poles: SketchNurbsPoles,
}

impl SketchNurbsGeometry {
    pub(crate) fn from_parts(
        degree: u32,
        fit_tolerance: f64,
        scalar_width: u32,
        knots: Vec<f64>,
        poles: SketchNurbsPoles,
    ) -> Result<Self, String> {
        if scalar_width != 8 {
            return Err("sketch NURBS scalar_width must be 8".into());
        }
        let fit_tolerance = NonNegativeLength::new(fit_tolerance)
            .ok_or("sketch NURBS fit_tolerance must be finite and nonnegative")?;
        Self::from_checked_parts(degree, fit_tolerance, knots, poles).map_err(ToString::to_string)
    }

    pub(crate) fn from_checked_parts(
        degree: u32,
        fit_tolerance: NonNegativeLength,
        knots: Vec<f64>,
        poles: SketchNurbsPoles,
    ) -> Result<Self, &'static str> {
        if knots.len() > 100_000 || poles.point_count() > 100_000 || poles.weights().len() > 100_000
        {
            return Err("sketch NURBS lane count exceeds source limit");
        }
        let expected_knots = poles
            .point_count()
            .checked_add(usize::try_from(degree).map_err(|_| "sketch NURBS degree overflows")?)
            .and_then(|count| count.checked_add(1))
            .ok_or("sketch NURBS knot count overflows")?;
        if knots.len() != expected_knots {
            return Err("sketch NURBS knot count must equal control points + degree + 1");
        }
        if !knots_nondecreasing(&knots) {
            return Err("sketch NURBS knots must be nondecreasing");
        }
        for knot in &knots {
            if !knot.is_finite() {
                return Err("sketch NURBS knot is not finite");
            }
        }
        Ok(Self {
            degree,
            fit_tolerance,
            knots,
            poles,
        })
    }

    pub(crate) fn degree(&self) -> u32 {
        self.degree
    }

    pub(crate) fn fit_tolerance(&self) -> NonNegativeLength {
        self.fit_tolerance
    }

    pub(crate) fn knots(&self) -> &[f64] {
        &self.knots
    }

    pub(crate) fn knots_copy(&self) -> Vec<f64> {
        self.knots.clone()
    }

    pub(crate) fn knot_count(&self) -> usize {
        self.knots.len()
    }

    pub(crate) fn poles(&self) -> &SketchNurbsPoles {
        &self.poles
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SketchCurveGeometryWire {
    /// A straight line segment.
    Line {
        /// Start point in sketch space, millimetres.
        start: Point3,
        /// End point in sketch space, millimetres.
        end: Point3,
        /// Direction from `start` to `end`; the decoder stores it at unit length.
        direction: Vector3,
        /// Normal of the sketch plane the line lies in; the decoder stores it at
        /// unit length.
        normal: Vector3,
    },
    /// A circular arc.
    Arc {
        /// Arc center in sketch space, millimetres.
        center: Point3,
        /// Normal of the sketch plane the arc lies in; the decoder admits a norm
        /// within `1e-9` of one.
        normal: Vector3,
        /// Zero-angle direction for `start_angle`/`end_angle`; the decoder admits a
        /// norm within `1e-9` of one.
        reference_direction: Vector3,
        /// Arc radius in millimetres.
        radius: f64,
        /// Start angle in radians, measured from `reference_direction`.
        start_angle: f64,
        /// End angle in radians, measured from `reference_direction`.
        end_angle: f64,
    },
    /// A NURBS (procedural spline) curve.
    Nurbs {
        /// Record index of the underlying carrier geometry, when the NURBS record
        /// references one; `None` when the control data is self-contained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carrier_reference: Option<u64>,
        /// Source per-file dynamic three-digit ASCII class tag naming the NURBS subtype.
        subtype_class_tag: String,
        /// Record index of the NURBS subtype record.
        subtype_record_index: u32,
        /// Polynomial degree of the curve.
        degree: u32,
        /// Source fit tolerance used when the curve was fitted, in millimetres.
        fit_tolerance: f64,
        /// Width in scalars of each control-point record as stored in the source
        /// (control point components plus weight, before decoding into `control_points`/`weights`).
        scalar_width: u32,
        /// Knot vector, non-decreasing, length `control_points.len() + degree + 1`.
        knots: Vec<f64>,
        /// Per-control-point rational weights, parallel to `control_points`.
        weights: Vec<f64>,
        /// Control points in sketch space, millimetres, parallel to `weights`.
        control_points: Vec<Point3>,
    },
}

impl TryFrom<SketchCurveGeometryWire> for SketchCurveGeometry {
    type Error = String;
    fn try_from(wire: SketchCurveGeometryWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            SketchCurveGeometryWire::Line {
                start,
                end,
                direction,
                normal,
            } => Self::line(start, end, direction, normal)?,
            SketchCurveGeometryWire::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Self::arc(
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            )?,
            SketchCurveGeometryWire::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                fit_tolerance,
                scalar_width,
                knots,
                weights,
                control_points,
            } => Self::nurbs_from_parts(
                carrier_reference,
                DesignClassTag::try_from(subtype_class_tag)
                    .map_err(|error| format!("subtype_class_tag: {error}"))?,
                subtype_record_index,
                SketchNurbsGeometry::from_parts(
                    degree,
                    fit_tolerance,
                    scalar_width,
                    knots,
                    SketchNurbsPoles::from_wire(control_points, weights)?,
                )?,
            ),
        })
    }
}

#[cfg(test)]
impl From<SketchCurveGeometry> for SketchCurveGeometryWire {
    fn from(geometry: SketchCurveGeometry) -> Self {
        match geometry {
            SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            } => Self::Line {
                start: start.get(),
                end: end.get(),
                direction: *direction.as_raw(),
                normal: *normal.as_raw(),
            },
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Self::Arc {
                center: center.get(),
                normal: *normal.as_raw(),
                reference_direction: *reference_direction.as_raw(),
                radius: radius.get(),
                start_angle: start_angle.get(),
                end_angle: end_angle.get(),
            },
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                geometry,
            } => {
                let (control_points, weights) = match geometry.poles {
                    SketchNurbsPoles::Polynomial(points) => (
                        points.into_iter().map(FinitePoint3::get).collect(),
                        Vec::new(),
                    ),
                    SketchNurbsPoles::Rational(poles) => poles
                        .into_iter()
                        .map(|pole| (pole.point.get(), pole.weight.get()))
                        .unzip(),
                };
                Self::Nurbs {
                    carrier_reference,
                    subtype_class_tag: subtype_class_tag.into(),
                    subtype_record_index,
                    degree: geometry.degree,
                    fit_tolerance: geometry.fit_tolerance.get(),
                    scalar_width: 8,
                    knots: geometry.knots,
                    weights,
                    control_points,
                }
            }
        }
    }
}

/// Control data for a polynomial or rational sketch spline.
#[derive(Debug, Clone)]
pub(crate) enum SketchNurbsPoles {
    Polynomial(Vec<FinitePoint3>),
    Rational(Vec<SketchNurbsPole>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SketchNurbsPole {
    point: FinitePoint3,
    weight: PositiveReal,
}

impl PartialEq for SketchNurbsPoles {
    fn eq(&self, other: &Self) -> bool {
        self.points().eq(other.points()) && self.weights().eq(other.weights())
    }
}

impl SketchNurbsPoles {
    pub(crate) fn from_wire(points: Vec<Point3>, weights: Vec<f64>) -> Result<Self, String> {
        if !weights.is_empty() && points.len() != weights.len() {
            return Err("weights must be absent or match every control_points entry".into());
        }
        let points = points
            .into_iter()
            .map(|point| {
                FinitePoint3::new(point).ok_or("sketch NURBS control point is not finite".into())
            })
            .collect::<Result<Vec<_>, String>>()?;
        Self::from_checked_points(points, weights)
    }

    pub(crate) fn from_checked_points(
        points: Vec<FinitePoint3>,
        weights: Vec<f64>,
    ) -> Result<Self, String> {
        if weights.is_empty() {
            return Ok(Self::Polynomial(points));
        }
        if points.len() != weights.len() {
            return Err("weights must be absent or match every control_points entry".into());
        }
        Ok(Self::Rational(
            points
                .into_iter()
                .zip(weights)
                .map(|(point, weight)| {
                    Ok(SketchNurbsPole {
                        point,
                        weight: PositiveReal::new(weight)
                            .ok_or("sketch NURBS weight is not positive and finite")?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        ))
    }

    pub(crate) fn point_count(&self) -> usize {
        match self {
            Self::Polynomial(points) => points.len(),
            Self::Rational(poles) => poles.len(),
        }
    }

    pub(crate) fn points(&self) -> impl DoubleEndedIterator<Item = &Point3> {
        let (points, poles): (&[FinitePoint3], &[SketchNurbsPole]) = match self {
            Self::Polynomial(points) => (points, &[]),
            Self::Rational(poles) => (&[], poles),
        };
        points
            .iter()
            .map(FinitePoint3::as_raw)
            .chain(poles.iter().map(|pole| pole.point.as_raw()))
    }

    pub(crate) fn weights(&self) -> impl ExactSizeIterator<Item = f64> + '_ {
        let poles: &[SketchNurbsPole] = match self {
            Self::Polynomial(_) => &[],
            Self::Rational(poles) => poles,
        };
        poles.iter().map(|pole| pole.weight.get())
    }
}

#[cfg(test)]
mod tests {
    use super::{SketchCurveGeometry, SketchPoint, SketchSurface, SketchText};
    use serde_json::json;

    #[test]
    fn sketch_geometry_constructors_preserve_standard_values() {
        use super::{SketchNurbsGeometry, SketchNurbsPoles, SketchSurfaceGeometry};
        use cadmpeg_ir::features::FinitePoint3;
        use cadmpeg_ir::math::Point3;
        use cadmpeg_ir::scalar::NonNegativeLength;

        let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
        let surface = SketchSurfaceGeometry::from_checked_parts(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![vec![point; 2]; 2],
        )
        .unwrap();
        assert_eq!(
            surface,
            SketchSurfaceGeometry::from_parts(
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                vec![vec![point.get(); 2]; 2]
            )
            .unwrap()
        );
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let poles = SketchNurbsPoles::Polynomial(vec![point; 2]);
        let curve = SketchNurbsGeometry::from_checked_parts(
            1,
            NonNegativeLength::new(0.0).unwrap(),
            knots.clone(),
            poles.clone(),
        )
        .unwrap();
        assert_eq!(
            curve,
            SketchNurbsGeometry::from_parts(1, 0.0, 8, knots, poles).unwrap()
        );
    }

    fn native_surface_wire() -> serde_json::Value {
        json!({
            "id": "surface", "record_index": 1, "class_tag": "306",
            "byte_offset": 0, "persistent_id": 29,
            "u_degree": 1, "v_degree": 1,
            "u_knots": [0.0, 0.0, 1.0, 1.0],
            "v_knots": [0.0, 0.0, 1.0, 1.0],
            "control_points": [
                [{"x":0.0,"y":0.0,"z":0.0},{"x":0.0,"y":1.0,"z":0.0}],
                [{"x":1.0,"y":0.0,"z":0.0},{"x":1.0,"y":1.0,"z":0.0}]
            ]
        })
    }

    fn native_text_wire(extended: bool) -> serde_json::Value {
        let mut wire = json!({
            "id": "text", "record_index": 1, "owner_reference": 2,
            "class_tag": "000", "class_version": 4, "byte_offset": 0,
            "text": "text", "font_family": "Arial", "font_weight": 400,
            "height": 10.0, "color": {"r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0},
            "raw_bytes": "010203"
        });
        if extended {
            wire["width_factor"] = json!(1.0);
            wire["horizontal_alignment"] = json!(3);
            wire["vertical_alignment"] = json!(7);
            wire["first_reference"] = json!(12);
            wire["second_reference"] = json!(13);
        }
        wire["anchor"] = json!({"u": 2.0, "v": 3.0});
        wire["rotation"] = json!(0.5);
        wire
    }

    fn native_point_wire(form: &serde_json::Value) -> serde_json::Value {
        let mut wire = json!({
            "id": "point", "record_index": 1, "class_tag": "000",
            "byte_offset": 0, "coordinate_offset": 1, "record_form": form,
            "paired_reference": 2, "coordinates": {"u": 2.0, "v": 3.0},
            "depth": 0.0, "companion": {"incident_curves": [7, 2]}
        });
        if wire["record_form"]["kind"] != "version0" {
            wire["persistent_id"] = json!(4);
            wire["closure"] = json!({"selector": 0, "state": 0});
        }
        wire
    }

    #[test]
    fn sketch_point_write_checks_zero_marker_without_deriving_unknown_flags() {
        let form = json!({"kind": "version11", "padded_paired_reference": false});
        let mut wire = native_point_wire(&form);
        wire["coordinates"] = json!({"u": 0.0, "v": 0.0});
        wire["flags"] = json!("0000000101010000");
        let point: SketchPoint = serde_json::from_value(wire.clone()).unwrap();
        assert!(point.validate_for_write(&[]).is_ok());
        for coordinates in [json!({"u": 1.0, "v": 0.0}), json!({"u": -0.0, "v": 0.0})] {
            let mut invalid = wire.clone();
            invalid["coordinates"] = coordinates;
            let point: SketchPoint = serde_json::from_value(invalid).unwrap();
            assert!(point.validate_for_write(&[]).is_err());
        }
        for flags in ["0000000001010000", "0000010101010000"] {
            let mut invalid = wire.clone();
            invalid["flags"] = json!(flags);
            let point: SketchPoint = serde_json::from_value(invalid).unwrap();
            assert!(point.validate_for_write(&[]).is_err());
        }
        wire["flags"] = json!("0101000101000100");
        wire["coordinates"] = json!({"u": 2.0, "v": 3.0});
        wire["closure"] = json!({"selector": 2, "state": 1});
        wire["companion"] = json!({"incident_curves": [7]});
        let point: SketchPoint = serde_json::from_value(wire.clone()).unwrap();
        assert!(point.validate_for_write(&[]).is_ok());
        wire["companion"] = json!({"incident_curves": []});
        let point: SketchPoint = serde_json::from_value(wire).unwrap();
        assert!(point.validate_for_write(&[]).is_ok());
    }

    #[test]
    fn sketch_point_write_checks_its_own_incident_circular_center() {
        let mut wire =
            native_point_wire(&json!({"kind": "version11", "padded_paired_reference": false}));
        wire["id"] = json!("f3d:stream:sketch-point#1");
        wire["flags"] = json!("0000000000000001");
        wire["closure"] = json!({"selector": 0, "state": 1});
        wire["companion"] = json!({"incident_curves": [7]});
        let point: SketchPoint = serde_json::from_value(wire.clone()).unwrap();
        let curve: super::SketchCurveIdentity = serde_json::from_value(json!({
            "id": "f3d:stream:sketch-curve#7", "record_index": 7,
            "class_tag": "000", "byte_offset": 0, "geometry_offset": 0,
            "owner_reference": 17, "primary_id": 1, "secondary_id": 0,
            "geometry": {"kind": "arc", "center": {"x": 2.0, "y": 3.0, "z": 0.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "reference_direction": {"x": 1.0, "y": 0.0, "z": 0.0},
                "radius": 1.0, "start_angle": 0.0, "end_angle": 1.0}
        }))
        .unwrap();
        assert!(point
            .validate_for_write(std::slice::from_ref(&curve))
            .is_ok());
        assert!(point.validate_for_write(&[]).is_err());
        let mut wrong_stream = curve.clone();
        wrong_stream.id = "f3d:other:sketch-curve#7".into();
        assert!(point.validate_for_write(&[wrong_stream]).is_err());
        let mut moved = point.clone();
        moved
            .try_set_coordinates(cadmpeg_ir::math::Point2 { u: 2.5, v: 3.0 })
            .unwrap();
        assert!(moved
            .validate_for_write(std::slice::from_ref(&curve))
            .is_err());
        wire["closure"] = json!({"selector": 4, "state": 0});
        let wrong_closure: SketchPoint = serde_json::from_value(wire).unwrap();
        assert!(wrong_closure.validate_for_write(&[curve]).is_err());
    }

    #[test]
    fn sketch_point_borrowed_wire_matches_owned_wire_bytes() {
        for form in [
            json!({"kind": "version0"}),
            json!({"kind": "version8"}),
            json!({"kind": "version10"}),
            json!({"kind": "version10_inline_typed", "trailing_reference": 3}),
            json!({"kind": "version11", "padded_paired_reference": false, "companion_prefix_present_zero": false}),
            json!({"kind": "version11_inline_typed", "trailing_reference": 3, "companion_prefix_present_zero": false}),
        ] {
            let point: SketchPoint = serde_json::from_value(native_point_wire(&form)).unwrap();
            let owned = super::SketchPointSerde::from(point.clone());
            assert_eq!(
                serde_json::to_vec(&point).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn sketch_point_native_retained_limit_refuses_before_clone() {
        #[derive(serde::Serialize)]
        struct NestedRecord<'a> {
            id: &'static str,
            value: &'a SketchPoint,
        }
        let point: SketchPoint = serde_json::from_value(native_point_wire(&json!({
            "kind": "version11", "padded_paired_reference": false,
            "companion_prefix_present_zero": false
        })))
        .unwrap();
        let record = NestedRecord {
            id: "f3d:native:sketch-point#0",
            value: &point,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "sketch_points",
            || super::SKETCH_POINT_CLONE_COUNT.with(|count| count.set(0)),
            || super::SKETCH_POINT_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn sketch_text_borrowed_wire_matches_owned_wire_bytes() {
        for extended in [false, true] {
            let text: SketchText = serde_json::from_value(native_text_wire(extended)).unwrap();
            let owned = super::SketchTextSerde::from(text.clone());
            assert_eq!(
                serde_json::to_vec(&text).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn sketch_text_native_retained_limit_refuses_before_clone() {
        #[derive(serde::Serialize)]
        struct NestedRecord<'a> {
            id: &'static str,
            value: &'a SketchText,
        }
        let text: SketchText = serde_json::from_value(native_text_wire(true)).unwrap();
        let record = NestedRecord {
            id: "f3d:native:sketch-text#0",
            value: &text,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "sketch_texts",
            || super::SKETCH_TEXT_CLONE_COUNT.with(|count| count.set(0)),
            || super::SKETCH_TEXT_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    fn native_arc_wire() -> serde_json::Value {
        json!({
            "kind": "arc",
            "center": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "reference_direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "radius": 1.0,
            "start_angle": 0.0,
            "end_angle": 1.0
        })
    }

    fn native_nurbs_wire(weights: &[f64]) -> serde_json::Value {
        json!({
            "kind": "nurbs", "subtype_class_tag": "306", "subtype_record_index": 4,
            "degree": 1, "fit_tolerance": 0.0, "scalar_width": 8,
            "knots": [0.0, 0.0, 1.0, 1.0], "weights": weights,
            "control_points": [
                {"x": 0.0, "y": 0.0, "z": 0.0},
                {"x": 1.0, "y": 0.0, "z": 0.0}
            ]
        })
    }

    #[test]
    fn sketch_curve_geometry_borrowed_wire_matches_owned_wire_bytes() {
        let line = json!({
            "kind": "line", "start": {"x": 0.0, "y": 0.0, "z": 0.0},
            "end": {"x": 10.0, "y": 0.0, "z": 0.0},
            "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}
        });
        for wire in [
            line,
            native_arc_wire(),
            native_nurbs_wire(&[]),
            native_nurbs_wire(&[1.0, 2.0]),
        ] {
            let geometry: SketchCurveGeometry = serde_json::from_value(wire).unwrap();
            let owned = super::SketchCurveGeometryWire::from(geometry.clone());
            assert_eq!(
                serde_json::to_vec(&geometry).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn sketch_curve_geometry_native_retained_limit_refuses_before_clone() {
        #[derive(serde::Serialize)]
        struct NestedRecord<'a> {
            id: &'static str,
            value: &'a SketchCurveGeometry,
        }
        let geometry: SketchCurveGeometry =
            serde_json::from_value(native_nurbs_wire(&[1.0, 2.0])).unwrap();
        let record = NestedRecord {
            id: "f3d:native:sketch-curve#0",
            value: &geometry,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "sketch_curves",
            || super::SKETCH_CURVE_GEOMETRY_CLONE_COUNT.with(|count| count.set(0)),
            || super::SKETCH_CURVE_GEOMETRY_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn native_arc_refuses_zero_sweep() {
        let mut wire = native_arc_wire();
        wire["end_angle"] = json!(0.0);
        assert!(serde_json::from_value::<SketchCurveGeometry>(wire).is_err());
    }

    #[test]
    fn native_arc_refuses_angle_past_full_turn() {
        let mut wire = native_arc_wire();
        wire["end_angle"] = json!(7.0);
        assert!(serde_json::from_value::<SketchCurveGeometry>(wire).is_err());
    }

    #[test]
    fn native_arc_refuses_zero_radius() {
        let mut wire = native_arc_wire();
        wire["radius"] = json!(0.0);
        assert!(serde_json::from_value::<SketchCurveGeometry>(wire).is_err());
    }

    #[test]
    fn native_arc_refuses_nonperpendicular_frame() {
        let mut wire = native_arc_wire();
        wire["reference_direction"] = json!({"x": 0.0, "y": 0.0, "z": 1.0});
        assert!(serde_json::from_value::<SketchCurveGeometry>(wire).is_err());
    }

    #[test]
    fn native_surface_wire_round_trip_preserves_flat_shape() {
        let wire = native_surface_wire();
        let surface: SketchSurface = serde_json::from_value(wire.clone()).expect("valid surface");
        assert_eq!(
            serde_json::to_value(surface).expect("serialize surface"),
            wire
        );
    }

    #[test]
    fn sketch_surface_borrowed_wire_matches_owned_wire_bytes() {
        let surface: SketchSurface = serde_json::from_value(native_surface_wire()).unwrap();
        let owned = super::SketchSurfaceWire::from(surface.clone());
        assert_eq!(
            serde_json::to_vec(&surface).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }

    #[test]
    fn sketch_surface_native_retained_limit_refuses_before_clone() {
        #[derive(serde::Serialize)]
        struct NestedRecord<'a> {
            id: &'static str,
            value: &'a SketchSurface,
        }
        let surface: SketchSurface = serde_json::from_value(native_surface_wire()).unwrap();
        let record = NestedRecord {
            id: "f3d:native:sketch-surface#0",
            value: &surface,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "sketch_surfaces",
            || super::SKETCH_SURFACE_CLONE_COUNT.with(|count| count.set(0)),
            || super::SKETCH_SURFACE_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn native_surface_refuses_zero_degree() {
        let mut wire = native_surface_wire();
        wire["u_degree"] = json!(0);
        assert!(serde_json::from_value::<SketchSurface>(wire).is_err());
    }

    #[test]
    fn native_surface_refuses_decreasing_knots() {
        let mut wire = native_surface_wire();
        wire["u_knots"] = json!([0.0, 1.0, 0.0, 1.0]);
        assert!(serde_json::from_value::<SketchSurface>(wire).is_err());
    }

    #[test]
    fn native_surface_refuses_ragged_control_grid() {
        let mut wire = native_surface_wire();
        wire["control_points"][1].as_array_mut().expect("row").pop();
        assert!(serde_json::from_value::<SketchSurface>(wire).is_err());
    }

    #[test]
    fn native_line_refuses_zero_endpoint_displacement() {
        let geometry = json!({
            "kind": "line",
            "start": {"x": 0.0, "y": 0.0, "z": 0.0},
            "end": {"x": 0.0, "y": 0.0, "z": 0.0},
            "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}
        });
        assert!(serde_json::from_value::<SketchCurveGeometry>(geometry).is_err());
    }

    #[test]
    fn native_line_refuses_overflowing_source_displacement() {
        let geometry = json!({
            "kind": "line",
            "start": {"x": -f64::MAX, "y": 0.0, "z": 0.0},
            "end": {"x": f64::MAX, "y": 0.0, "z": 0.0},
            "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}
        });
        assert!(serde_json::from_value::<SketchCurveGeometry>(geometry).is_err());
    }

    #[test]
    fn native_line_refuses_direction_that_disagrees_with_endpoints() {
        let geometry = json!({
            "kind": "line",
            "start": {"x": 0.0, "y": 0.0, "z": 0.0},
            "end": {"x": 0.0, "y": 1.0, "z": 0.0},
            "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}
        });
        assert!(serde_json::from_value::<SketchCurveGeometry>(geometry).is_err());
    }
}
