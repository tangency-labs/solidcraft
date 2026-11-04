// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_core::decode::u64_from_index;

use super::references::DesignClassTag;
use cadmpeg_ir::features::{FeatureImageBounds, FeatureUnitPlaneFrame, FinitePoint3};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::UnitVector3;
use serde::{Deserialize, Serialize};

pub(crate) mod geometry_layout;
use geometry_layout::DesignCanvasGeometryLayout;

const DESIGN_CANVAS_LENGTH_TO_MM: f64 = 10.0;

/// Canvas opacity and source-space frame; the fixed payload is emitted from these values.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignCanvasGeometryPayload {
    opacity: f32,
    origin_centimetres: [FiniteReal; 3],
    frame: FeatureUnitPlaneFrame,
}
impl TryFrom<&[u8]> for DesignCanvasGeometryPayload {
    type Error = String;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != 77 {
            return Err("geometry_payload must contain 77 bytes".into());
        }
        let mut view = cadmpeg_core::decode::View::over_retained(bytes);
        let opacity = view
            .req_f32_le()
            .map_err(|error| format!("geometry_payload: {error:?}"))?;
        let reserved = view
            .req_u8()
            .map_err(|error| format!("geometry_payload: {error:?}"))?;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) || reserved != 0 {
            return Err(
                "geometry_payload must contain normalized finite opacity and a zero reserved byte"
                    .into(),
            );
        }
        let mut vector = || -> Result<[f64; 3], String> {
            Ok([
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
            ])
        };
        let [origin_x, origin_y, origin_z] = vector()?;
        let u_components = vector()?;
        let v_components = vector()?;
        let invalid_frame = || {
            "geometry_payload must contain a finite origin and an admitted orthonormal frame"
                .to_owned()
        };
        let origin_centimetres = [
            FiniteReal::new(origin_x).ok_or_else(invalid_frame)?,
            FiniteReal::new(origin_y).ok_or_else(invalid_frame)?,
            FiniteReal::new(origin_z).ok_or_else(invalid_frame)?,
        ];
        let origin = FinitePoint3::new(Point3::new(
            origin_x * DESIGN_CANVAS_LENGTH_TO_MM,
            origin_y * DESIGN_CANVAS_LENGTH_TO_MM,
            origin_z * DESIGN_CANVAS_LENGTH_TO_MM,
        ))
        .ok_or_else(invalid_frame)?;
        let u_axis = UnitVector3::new(Vector3::from(u_components)).ok_or_else(invalid_frame)?;
        let v_axis = UnitVector3::new(Vector3::from(v_components)).ok_or_else(invalid_frame)?;
        let frame =
            FeatureUnitPlaneFrame::from_parts(origin, u_axis, v_axis).ok_or_else(invalid_frame)?;
        Ok(Self {
            opacity,
            origin_centimetres,
            frame,
        })
    }
}
impl DesignCanvasGeometryPayload {
    pub(crate) fn decoded(&self) -> (f32, FeatureUnitPlaneFrame) {
        (self.opacity, self.frame)
    }
    fn bytes(&self) -> [u8; 77] {
        let mut bytes = [0; 77];
        bytes[..4].copy_from_slice(&self.opacity.to_le_bytes());
        let mut at = 5;
        let u_axis = self.frame.u_axis();
        let v_axis = self.frame.v_axis();
        for value in self
            .origin_centimetres
            .into_iter()
            .map(FiniteReal::get)
            .chain([u_axis.as_raw().x, u_axis.as_raw().y, u_axis.as_raw().z])
            .chain([v_axis.as_raw().x, v_axis.as_raw().y, v_axis.as_raw().z])
        {
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            at += 8;
        }
        bytes
    }
}

const EPS_CANVAS_BOUNDARY_CLOSURE: f64 = 64.0 * f64::EPSILON;
const EPS_CANVAS_BOUNDARY_ORTHOGONAL: f64 = 64.0 * f64::EPSILON;

/// Ordered opposite image edges in the supporting plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignCanvasBounds {
    segments: [[Point2; 2]; 2],
}
impl TryFrom<[[Point2; 2]; 2]> for DesignCanvasBounds {
    type Error = String;
    fn try_from(segments: [[Point2; 2]; 2]) -> Result<Self, Self::Error> {
        if segments.iter().flatten().any(|point| !point.is_finite()) {
            return Err("boundary_segments must contain finite coordinates".into());
        }
        let [[origin_corner, width_corner], [height_corner, opposite_corner]] = segments;
        let width_delta = Point2::new(
            width_corner.u - origin_corner.u,
            width_corner.v - origin_corner.v,
        );
        let height_delta = Point2::new(
            height_corner.u - origin_corner.u,
            height_corner.v - origin_corner.v,
        );
        let width = width_delta.u.hypot(width_delta.v);
        let height = height_delta.u.hypot(height_delta.v);
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err("boundary_segments must have finite nonzero image axes".into());
        }
        let dot = (width_delta.u / width) * (height_delta.u / height)
            + (width_delta.v / width) * (height_delta.v / height);
        let close = |actual: f64, expected: f64| {
            expected.is_finite()
                && (actual - expected).abs()
                    <= EPS_CANVAS_BOUNDARY_CLOSURE * actual.abs().max(expected.abs()).max(1.0)
        };
        if dot.abs() > EPS_CANVAS_BOUNDARY_ORTHOGONAL
            || !close(opposite_corner.u, width_corner.u + height_delta.u)
            || !close(opposite_corner.v, width_corner.v + height_delta.v)
        {
            return Err("boundary_segments must form an ordered rectangle".into());
        }
        Ok(Self { segments })
    }
}
impl DesignCanvasBounds {
    fn segments(self) -> [[Point2; 2]; 2] {
        self.segments
    }

    pub(crate) fn image_placement(
        self,
        plane: FeatureUnitPlaneFrame,
    ) -> Result<(FeatureUnitPlaneFrame, FeatureImageBounds), String> {
        let [[origin_corner, width_corner], [height_corner, _]] = self.segments;
        let width_delta = Point2::new(
            width_corner.u - origin_corner.u,
            width_corner.v - origin_corner.v,
        );
        let height_delta = Point2::new(
            height_corner.u - origin_corner.u,
            height_corner.v - origin_corner.v,
        );
        let width = width_delta.u.hypot(width_delta.v);
        let height = height_delta.u.hypot(height_delta.v);
        let plane_u = *plane.u_axis().as_raw();
        let plane_v = *plane.v_axis().as_raw();
        let origin = plane
            .origin()
            .get()
            .translated(plane_u, origin_corner.u * DESIGN_CANVAS_LENGTH_TO_MM)
            .translated(plane_v, origin_corner.v * DESIGN_CANVAS_LENGTH_TO_MM);
        let frame = FeatureUnitPlaneFrame::new(
            origin,
            plane_u.scale(width_delta.u / width) + plane_v.scale(width_delta.v / width),
            plane_u.scale(height_delta.u / height) + plane_v.scale(height_delta.v / height),
        )
        .ok_or("Canvas image frame must be finite and orthonormal")?;
        let bounds = FeatureImageBounds::new([
            Point2::new(0.0, 0.0),
            Point2::new(
                width * DESIGN_CANVAS_LENGTH_TO_MM,
                height * DESIGN_CANVAS_LENGTH_TO_MM,
            ),
        ])
        .ok_or("Canvas image dimensions must be finite and positive")?;
        Ok((frame, bounds))
    }
}

/// Canvas geometry flags and their physical layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignCanvasPrologue {
    layout: DesignCanvasGeometryLayout,
    first_flag: bool,
    visible: bool,
}
impl TryFrom<[u8; 15]> for DesignCanvasPrologue {
    type Error = String;
    fn try_from(bytes: [u8; 15]) -> Result<Self, Self::Error> {
        Self::from_layout(DesignCanvasGeometryLayout::Modern, &bytes)
    }
}
impl DesignCanvasPrologue {
    pub(crate) fn from_layout(
        layout: DesignCanvasGeometryLayout,
        bytes: &[u8],
    ) -> Result<Self, String> {
        let length = layout.offsets().first_boundary - 11;
        if bytes.len() != length || !matches!(bytes.last(), Some(0 | 1)) {
            return Err("geometry_prologue must match its layout and visibility Boolean".into());
        }
        let valid = match layout {
            DesignCanvasGeometryLayout::Legacy | DesignCanvasGeometryLayout::TypedLegacy => {
                bytes[..12] == [0; 12]
            }
            DesignCanvasGeometryLayout::Transitional
            | DesignCanvasGeometryLayout::TypedTransitional => {
                bytes[..10] == [0; 10] && bytes[10..13] == [1, 0, 0]
            }
            _ => bytes[..10] == [0; 10] && matches!(bytes[10], 0 | 1) && bytes[11..14] == [0; 3],
        };
        if !valid {
            return Err("geometry_prologue must contain its fixed bytes and Boolean flags".into());
        }
        Ok(Self {
            layout,
            first_flag: bytes[10] != 0,
            visible: bytes[length - 1] != 0,
        })
    }
    pub(crate) fn visible(self) -> bool {
        self.visible
    }
    fn bytes(self) -> Vec<u8> {
        let mut bytes = match self.layout {
            DesignCanvasGeometryLayout::Legacy | DesignCanvasGeometryLayout::TypedLegacy => {
                vec![0; 13]
            }
            DesignCanvasGeometryLayout::Transitional
            | DesignCanvasGeometryLayout::TypedTransitional => {
                let mut bytes = vec![0; 14];
                bytes[10] = 1;
                bytes
            }
            _ => {
                let mut bytes = vec![0; 15];
                bytes[10] = u8::from(self.first_flag);
                bytes
            }
        };
        let last = bytes.len() - 1;
        bytes[last] = u8::from(self.visible);
        bytes
    }
}

const CANVAS_IMAGE_ASSET_PREFIX_BYTES: u64 = 25;

/// Canvas geometry and its same-index closing record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignCanvasGeometry {
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: u64,
    label: String,
    /// Flags from the fixed geometry prologue.
    pub(crate) prologue: DesignCanvasPrologue,
    /// Authored image boundaries and their orientation.
    pub(crate) boundary: DesignCanvasBounds,
    /// Opacity and source-space image frame.
    pub(crate) payload: DesignCanvasGeometryPayload,
}
impl DesignCanvasGeometry {
    pub(crate) fn new(
        class_tags: [String; 2],
        record_index: u32,
        byte_offset: u64,
        label: String,
        prologue: DesignCanvasPrologue,
        boundary: DesignCanvasBounds,
        payload: DesignCanvasGeometryPayload,
    ) -> Result<Self, String> {
        for (name, tag) in ["geometry_class_tag", "paired_geometry_class_tag"]
            .into_iter()
            .zip(&class_tags)
        {
            if tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(format!("{name} must contain printable ASCII characters"));
            }
        }
        if label.is_empty() {
            return Err("label must be nonempty".into());
        }
        let units = u32::try_from(label.encode_utf16().count())
            .map_err(|_| "label UTF-16 count must fit u32")?;
        let frame_length = u64_from_index(prologue.layout.offsets().prefix) + 2 * u64::from(units);
        byte_offset
            .checked_add(frame_length)
            .and_then(|end| end.checked_add(prologue.layout.paired_len()))
            .ok_or("geometry_byte_offset and label must leave a complete paired geometry record")?;
        Ok(Self {
            class_tags,
            record_index,
            byte_offset,
            label,
            prologue,
            boundary,
            payload,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    fn frame_length(&self) -> u64 {
        u64_from_index(self.prologue.layout.offsets().prefix)
            + 2 * u64_from_index(self.label.encode_utf16().count())
    }
    fn paired_byte_offset(&self) -> u64 {
        self.byte_offset + self.frame_length()
    }
    fn scope_reference_offset(&self) -> u64 {
        self.byte_offset + u64_from_index(self.prologue.layout.offsets().scope + 1)
    }
    fn visibility_offset(&self) -> u64 {
        self.byte_offset + u64_from_index(self.prologue.layout.offsets().first_boundary - 1)
    }
    fn paired_component_reference_offset(&self) -> u64 {
        self.paired_byte_offset() + 20
    }
    fn boundary_coordinate_offsets(&self) -> [u64; 8] {
        {
            let offsets = self.prologue.layout.offsets();
            [
                offsets.first_boundary,
                offsets.first_boundary + 8,
                offsets.first_boundary + 16,
                offsets.first_boundary + 24,
                offsets.second_boundary,
                offsets.second_boundary + 8,
                offsets.second_boundary + 16,
                offsets.second_boundary + 24,
            ]
            .map(|relative| self.byte_offset + u64_from_index(relative))
        }
    }
    fn second_boundary_present_offset(&self) -> u64 {
        self.byte_offset + u64_from_index(self.prologue.layout.offsets().state)
    }
    fn plane_reference_offset(&self) -> u64 {
        self.byte_offset + u64_from_index(self.prologue.layout.offsets().plane + 1)
    }
    fn component_reference_offset(&self) -> u64 {
        self.byte_offset + u64_from_index(self.prologue.layout.offsets().component + 1)
    }
    fn asset_reference_offset(&self) -> u64 {
        self.byte_offset + u64_from_index(self.prologue.layout.offsets().asset + 1)
    }
    fn label_offset(&self) -> u64 {
        self.byte_offset + u64_from_index(self.prologue.layout.offsets().prefix)
    }
}

/// Canvas image-asset record with a nonempty UTF-16 name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCanvasAsset {
    class_tag: DesignClassTag,
    record_index: u32,
    name: String,
}
impl DesignCanvasAsset {
    pub(crate) fn new(
        class_tag: DesignClassTag,
        record_index: u32,
        name: String,
    ) -> Result<Self, String> {
        if name.is_empty() {
            return Err("asset_name must be nonempty".into());
        }
        u32::try_from(name.encode_utf16().count())
            .map_err(|_| "asset_name UTF-16 count must fit u32")?;
        Ok(Self {
            class_tag,
            record_index,
            name,
        })
    }
    fn frame_length(&self) -> u64 {
        CANVAS_IMAGE_ASSET_PREFIX_BYTES + 2 * u64_from_index(self.name.encode_utf16().count())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignCanvasScopeForm {
    Compact,
    Expanded,
}
impl DesignCanvasScopeForm {
    fn reference_offset(self) -> u64 {
        match self {
            Self::Compact => 22,
            Self::Expanded => 26,
        }
    }
}

/// Exact image-plane binding owned by one Design `Canvas` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignCanvasImageWire")]
pub(crate) struct DesignCanvasImage {
    /// Globally unique native binding identity.
    pub(crate) id: String,
    /// Owning Canvas scope.
    pub(crate) scope_record_index: u32,
    /// Supporting construction-plane entity.
    pub(crate) plane_entity_suffix: u32,
    /// Component entity owning the Canvas.
    pub(crate) component_entity_suffix: u32,
    geometry: DesignCanvasGeometry,
    asset: DesignCanvasAsset,
    scope_form: DesignCanvasScopeForm,
}

#[cfg(test)]
thread_local! {
    pub(super) static CANVAS_IMAGE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignCanvasImage {
    fn clone(&self) -> Self {
        CANVAS_IMAGE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            plane_entity_suffix: self.plane_entity_suffix,
            component_entity_suffix: self.component_entity_suffix,
            geometry: self.geometry.clone(),
            asset: self.asset.clone(),
            scope_form: self.scope_form,
        }
    }
}
impl DesignCanvasImage {
    pub(crate) fn new(
        id: String,
        scope_record_index: u32,
        geometry_reference_offset: u64,
        geometry: DesignCanvasGeometry,
        asset: DesignCanvasAsset,
        plane_entity_suffix: u32,
        component_entity_suffix: u32,
    ) -> Result<Self, String> {
        if geometry.record_index == asset.record_index {
            return Err("asset_record_index must differ from geometry_record_index".into());
        }
        let scope_offset = geometry
            .paired_byte_offset()
            .checked_add(geometry.prologue.layout.paired_len())
            .and_then(|offset| offset.checked_add(asset.frame_length()))
            .ok_or("asset_name must end at a representable scope byte offset")?;
        let scope_form = match geometry_reference_offset.checked_sub(scope_offset) {
            Some(22) => DesignCanvasScopeForm::Compact,
            Some(26) => DesignCanvasScopeForm::Expanded,
            _ => return Err("geometry_reference_offset must locate a compact or expanded Canvas scope reference".into()),
        };
        geometry_reference_offset
            .checked_add(4)
            .ok_or("geometry_reference_offset must leave a complete u32 reference")?;
        Ok(Self {
            id,
            scope_record_index,
            plane_entity_suffix,
            component_entity_suffix,
            geometry,
            asset,
            scope_form,
        })
    }
    pub(crate) fn geometry(&self) -> &DesignCanvasGeometry {
        &self.geometry
    }
    pub(crate) fn asset_name(&self) -> &str {
        &self.asset.name
    }
    pub(crate) fn scope_byte_offset(&self) -> u64 {
        self.asset_byte_offset() + self.asset.frame_length()
    }
    fn asset_byte_offset(&self) -> u64 {
        self.geometry.paired_byte_offset() + self.geometry.prologue.layout.paired_len()
    }
    fn asset_name_offset(&self) -> u64 {
        self.asset_byte_offset() + CANVAS_IMAGE_ASSET_PREFIX_BYTES
    }
    fn geometry_reference_offset(&self) -> u64 {
        self.scope_byte_offset() + self.scope_form.reference_offset()
    }
}

/// Exact image-plane binding owned by one Design `Canvas` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCanvasImageWire {
    /// Globally unique deterministic identifier for this native binding.
    id: String,
    /// Canvas scope record index.
    scope_record_index: u32,
    /// Byte offset of the marked scope reference in the geometry record.
    scope_reference_offset: u64,
    /// Dynamic class tag of the primary geometry record.
    geometry_class_tag: String,
    /// Geometry record index.
    geometry_record_index: u32,
    /// Byte offset of the scope's marked geometry-record reference.
    geometry_reference_offset: u64,
    /// Byte offset of the primary geometry record.
    geometry_byte_offset: u64,
    /// Fixed geometry prologue immediately following the primary record header.
    geometry_prologue: NativeBytes<Vec<u8>>,
    /// Whether the Canvas raster is visible.
    visible: bool,
    /// Byte offset of the visibility byte in the geometry prologue.
    visibility_offset: u64,
    /// Byte length from the primary geometry header to its paired header.
    geometry_frame_length: u64,
    /// Dynamic class tag of the paired geometry record.
    paired_geometry_class_tag: String,
    /// Byte offset of the paired geometry record.
    paired_geometry_byte_offset: u64,
    /// Byte offset of the paired record's marked component reference.
    paired_component_reference_offset: u64,
    /// Two opposite boundary segments in plane-local coordinates.
    boundary_segments: [[Point2; 2]; 2],
    /// Byte offsets of the eight boundary-coordinate f64 values.
    boundary_coordinate_offsets: [u64; 8],
    /// Byte offset of the presence marker preceding the second boundary segment.
    second_boundary_present_offset: u64,
    /// Design entity suffix of the supporting construction plane.
    plane_entity_suffix: u32,
    /// Byte offset of the marked construction-plane reference.
    plane_reference_offset: u64,
    /// Design entity suffix of the component owning the Canvas.
    component_entity_suffix: u32,
    /// Byte offset of the marked component reference.
    component_reference_offset: u64,
    /// Dynamic class tag of the standalone image-asset record.
    asset_class_tag: String,
    /// Image-asset record index.
    asset_record_index: u32,
    /// Byte offset of the marked image-asset reference.
    asset_reference_offset: u64,
    /// Byte offset of the image-asset record.
    asset_byte_offset: u64,
    /// Archive entry basename stored by the image-asset record.
    asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    asset_name_offset: u64,
    /// Persistent Canvas label stored after the boundary segments.
    label: String,
    /// Byte offset of the label's UTF-16LE code units.
    label_offset: u64,
    /// Normalized raster opacity.
    opacity: f32,
    /// Image-plane origin in model-space millimeters.
    origin: Point3,
    /// Unit direction of increasing image u coordinate.
    u_axis: Vector3,
    /// Unit direction of increasing image v coordinate.
    v_axis: Vector3,
    /// Uninterpreted fixed geometry payload between the plane reference and scope link.
    geometry_payload: NativeBytes<Vec<u8>>,
}

#[derive(Serialize)]
struct DesignCanvasImageWireRef<'a> {
    id: &'a str,
    scope_record_index: u32,
    scope_reference_offset: u64,
    geometry_class_tag: &'a str,
    geometry_record_index: u32,
    geometry_reference_offset: u64,
    geometry_byte_offset: u64,
    geometry_prologue: NativeBytes<Vec<u8>>,
    visible: bool,
    visibility_offset: u64,
    geometry_frame_length: u64,
    paired_geometry_class_tag: &'a str,
    paired_geometry_byte_offset: u64,
    paired_component_reference_offset: u64,
    boundary_segments: [[Point2; 2]; 2],
    boundary_coordinate_offsets: [u64; 8],
    second_boundary_present_offset: u64,
    plane_entity_suffix: u32,
    plane_reference_offset: u64,
    component_entity_suffix: u32,
    component_reference_offset: u64,
    asset_class_tag: &'a str,
    asset_record_index: u32,
    asset_reference_offset: u64,
    asset_byte_offset: u64,
    asset_name: &'a str,
    asset_name_offset: u64,
    label: &'a str,
    label_offset: u64,
    opacity: f32,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
    geometry_payload: NativeBytes<&'a [u8]>,
}

impl Serialize for DesignCanvasImage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (opacity, frame) = self.geometry.payload.decoded();
        let geometry_payload = self.geometry.payload.bytes();
        let [geometry_class_tag, paired_geometry_class_tag] = &self.geometry.class_tags;
        DesignCanvasImageWireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_offset: self.geometry.scope_reference_offset(),
            geometry_class_tag,
            geometry_record_index: self.geometry.record_index,
            geometry_reference_offset: self.geometry_reference_offset(),
            geometry_byte_offset: self.geometry.byte_offset,
            geometry_prologue: (self.geometry.prologue.bytes()).into(),
            visible: self.geometry.prologue.visible(),
            visibility_offset: self.geometry.visibility_offset(),
            geometry_frame_length: self.geometry.frame_length(),
            paired_geometry_class_tag,
            paired_geometry_byte_offset: self.geometry.paired_byte_offset(),
            paired_component_reference_offset: self.geometry.paired_component_reference_offset(),
            boundary_segments: self.geometry.boundary.segments(),
            boundary_coordinate_offsets: self.geometry.boundary_coordinate_offsets(),
            second_boundary_present_offset: self.geometry.second_boundary_present_offset(),
            plane_entity_suffix: self.plane_entity_suffix,
            plane_reference_offset: self.geometry.plane_reference_offset(),
            component_entity_suffix: self.component_entity_suffix,
            component_reference_offset: self.geometry.component_reference_offset(),
            asset_class_tag: self.asset.class_tag.as_str(),
            asset_record_index: self.asset.record_index,
            asset_reference_offset: self.geometry.asset_reference_offset(),
            asset_byte_offset: self.asset_byte_offset(),
            asset_name: &self.asset.name,
            asset_name_offset: self.asset_name_offset(),
            label: &self.geometry.label,
            label_offset: self.geometry.label_offset(),
            opacity,
            origin: frame.origin().get(),
            u_axis: *frame.u_axis().as_raw(),
            v_axis: *frame.v_axis().as_raw(),
            geometry_payload: (&geometry_payload).into(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignCanvasImageWire> for DesignCanvasImage {
    type Error = String;
    fn try_from(wire: DesignCanvasImageWire) -> Result<Self, Self::Error> {
        let label_bytes = u64_from_index(wire.label.encode_utf16().count())
            .checked_mul(2)
            .ok_or("label length overflow")?;
        let prefix = wire
            .geometry_frame_length
            .checked_sub(label_bytes)
            .ok_or("geometry_frame_length is shorter than its label")?;
        let layout = DesignCanvasGeometryLayout::from_prefix(prefix)
            .ok_or("unsupported geometry_frame_length")?;
        let geometry_prologue =
            DesignCanvasPrologue::from_layout(layout, wire.geometry_prologue.as_slice())?;
        if geometry_prologue.visible() != wire.visible {
            return Err("visible must match geometry_prologue".into());
        }
        let geometry_payload =
            DesignCanvasGeometryPayload::try_from(wire.geometry_payload.as_slice())?;
        let (opacity, frame) = geometry_payload.decoded();
        let origin = frame.origin().get();
        let u_axis = frame.u_axis();
        let v_axis = frame.v_axis();
        if wire.opacity.to_bits() != opacity.to_bits() {
            return Err("opacity must match geometry_payload".into());
        }
        for (name, declared, decoded) in [
            (
                "origin",
                [wire.origin.x, wire.origin.y, wire.origin.z],
                [origin.x, origin.y, origin.z],
            ),
            (
                "u_axis",
                [wire.u_axis.x, wire.u_axis.y, wire.u_axis.z],
                [u_axis.as_raw().x, u_axis.as_raw().y, u_axis.as_raw().z],
            ),
            (
                "v_axis",
                [wire.v_axis.x, wire.v_axis.y, wire.v_axis.z],
                [v_axis.as_raw().x, v_axis.as_raw().y, v_axis.as_raw().z],
            ),
        ] {
            if declared
                .into_iter()
                .zip(decoded)
                .any(|(left, right)| left.to_bits() != right.to_bits())
            {
                return Err(format!("{name} must match geometry_payload"));
            }
        }
        let geometry = DesignCanvasGeometry::new(
            [wire.geometry_class_tag, wire.paired_geometry_class_tag],
            wire.geometry_record_index,
            wire.geometry_byte_offset,
            wire.label,
            geometry_prologue,
            DesignCanvasBounds::try_from(wire.boundary_segments)?,
            geometry_payload,
        )?;
        let asset = DesignCanvasAsset::new(
            DesignClassTag::try_from(wire.asset_class_tag)
                .map_err(|error| format!("asset_class_tag: {error}"))?,
            wire.asset_record_index,
            wire.asset_name,
        )?;
        let image = Self::new(
            wire.id,
            wire.scope_record_index,
            wire.geometry_reference_offset,
            geometry,
            asset,
            wire.plane_entity_suffix,
            wire.component_entity_suffix,
        )?;
        for (name, declared, derived) in [
            (
                "scope_reference_offset",
                wire.scope_reference_offset,
                image.geometry.scope_reference_offset(),
            ),
            (
                "visibility_offset",
                wire.visibility_offset,
                image.geometry.visibility_offset(),
            ),
            (
                "geometry_frame_length",
                wire.geometry_frame_length,
                image.geometry.frame_length(),
            ),
            (
                "paired_geometry_byte_offset",
                wire.paired_geometry_byte_offset,
                image.geometry.paired_byte_offset(),
            ),
            (
                "paired_component_reference_offset",
                wire.paired_component_reference_offset,
                image.geometry.paired_component_reference_offset(),
            ),
            (
                "second_boundary_present_offset",
                wire.second_boundary_present_offset,
                image.geometry.second_boundary_present_offset(),
            ),
            (
                "plane_reference_offset",
                wire.plane_reference_offset,
                image.geometry.plane_reference_offset(),
            ),
            (
                "component_reference_offset",
                wire.component_reference_offset,
                image.geometry.component_reference_offset(),
            ),
            (
                "asset_reference_offset",
                wire.asset_reference_offset,
                image.geometry.asset_reference_offset(),
            ),
            (
                "asset_byte_offset",
                wire.asset_byte_offset,
                image.asset_byte_offset(),
            ),
            (
                "asset_name_offset",
                wire.asset_name_offset,
                image.asset_name_offset(),
            ),
            (
                "label_offset",
                wire.label_offset,
                image.geometry.label_offset(),
            ),
        ] {
            if declared != derived {
                return Err(format!("{name} must match the Canvas record layout"));
            }
        }
        if wire.boundary_coordinate_offsets != image.geometry.boundary_coordinate_offsets() {
            return Err("boundary_coordinate_offsets must match the Canvas record layout".into());
        }
        Ok(image)
    }
}

#[cfg(test)]
impl From<DesignCanvasImage> for DesignCanvasImageWire {
    fn from(value: DesignCanvasImage) -> Self {
        let (opacity, frame) = value.geometry.payload.decoded();
        let scope_reference_offset = value.geometry.scope_reference_offset();
        let geometry_record_index = value.geometry.record_index;
        let geometry_reference_offset = value.geometry_reference_offset();
        let geometry_byte_offset = value.geometry.byte_offset;
        let geometry_prologue = value.geometry.prologue.bytes();
        let visible = value.geometry.prologue.visible();
        let visibility_offset = value.geometry.visibility_offset();
        let geometry_frame_length = value.geometry.frame_length();
        let paired_geometry_byte_offset = value.geometry.paired_byte_offset();
        let paired_component_reference_offset = value.geometry.paired_component_reference_offset();
        let boundary_segments = value.geometry.boundary.segments();
        let boundary_coordinate_offsets = value.geometry.boundary_coordinate_offsets();
        let second_boundary_present_offset = value.geometry.second_boundary_present_offset();
        let plane_reference_offset = value.geometry.plane_reference_offset();
        let component_reference_offset = value.geometry.component_reference_offset();
        let asset_record_index = value.asset.record_index;
        let asset_reference_offset = value.geometry.asset_reference_offset();
        let asset_byte_offset = value.asset_byte_offset();
        let asset_name_offset = value.asset_name_offset();
        let label_offset = value.geometry.label_offset();
        let geometry_payload = value.geometry.payload.bytes().to_vec();
        let [geometry_class_tag, paired_geometry_class_tag] = value.geometry.class_tags;
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            scope_reference_offset,
            geometry_class_tag,
            geometry_record_index,
            geometry_reference_offset,
            geometry_byte_offset,
            geometry_prologue: (geometry_prologue).into(),
            visible,
            visibility_offset,
            geometry_frame_length,
            paired_geometry_class_tag,
            paired_geometry_byte_offset,
            paired_component_reference_offset,
            boundary_segments,
            boundary_coordinate_offsets,
            second_boundary_present_offset,
            plane_entity_suffix: value.plane_entity_suffix,
            plane_reference_offset,
            component_entity_suffix: value.component_entity_suffix,
            component_reference_offset,
            asset_class_tag: value.asset.class_tag.into(),
            asset_record_index,
            asset_reference_offset,
            asset_byte_offset,
            asset_name: value.asset.name,
            asset_name_offset,
            label: value.geometry.label,
            label_offset,
            opacity,
            origin: frame.origin().get(),
            u_axis: *frame.u_axis().as_raw(),
            v_axis: *frame.v_axis().as_raw(),
            geometry_payload: (geometry_payload).into(),
        }
    }
}

#[cfg(test)]
mod tests;
