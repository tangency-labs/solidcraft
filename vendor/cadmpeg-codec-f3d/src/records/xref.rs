// SPDX-License-Identifier: Apache-2.0
//! External-reference designs, their references and the placement transform they state.

use super::identity::DesignAffineTransform;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_transform, XrefPlacementTransform, "transform");
cadmpeg_core::named_optional_field!(deserialize_current_pose, XrefCurrentPose, "current_pose");
cadmpeg_core::named_optional_field!(
    deserialize_operation_pose,
    XrefOperationPose,
    "operation_pose"
);
cadmpeg_core::named_optional_field!(deserialize_type_guid, String, "type_guid");
cadmpeg_core::named_optional_field!(deserialize_owner, XrefLocalOwner, "owner");
cadmpeg_core::named_optional_field!(
    deserialize_placement_source,
    XrefPlacementSource,
    "placement_source"
);
cadmpeg_core::named_optional_field!(
    deserialize_occurrence_guid,
    super::mesh::DesignRelaxedGuidText,
    "occurrence_guid"
);
const EPS_XREF_PLACEMENT_RIGID_E8: f64 = 1.0e-8;

/// The determinant of a finite affine matrix whose linear block is
/// orthonormal, or `None` when the matrix is not one.
pub(super) fn orthonormal_determinant(transform: &[[f64; 4]; 4]) -> Option<f64> {
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return None;
    }
    let columns = [
        [transform[0][0], transform[1][0], transform[2][0]],
        [transform[0][1], transform[1][1], transform[2][1]],
        [transform[0][2], transform[1][2], transform[2][2]],
    ];
    for (ordinal, column) in columns.iter().enumerate() {
        let norm = column.iter().map(|value| value * value).sum::<f64>();
        if (norm - 1.0).abs() > EPS_XREF_PLACEMENT_RIGID_E8 {
            return None;
        }
        for other in &columns[..ordinal] {
            let dot = column
                .iter()
                .zip(other)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > EPS_XREF_PLACEMENT_RIGID_E8 {
                return None;
            }
        }
    }
    Some(
        transform[0][0] * (transform[1][1] * transform[2][2] - transform[1][2] * transform[2][1])
            - transform[0][1]
                * (transform[1][0] * transform[2][2] - transform[1][2] * transform[2][0])
            + transform[0][2]
                * (transform[1][0] * transform[2][1] - transform[1][1] * transform[2][0]),
    )
}

fn valid_xref_placement_transform(transform: &[[f64; 4]; 4]) -> bool {
    orthonormal_determinant(transform)
        .is_some_and(|determinant| (determinant - 1.0).abs() <= EPS_XREF_PLACEMENT_RIGID_E8)
}

/// A finite proper rigid transform from an F3D XREF placement record.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct XrefPlacementTransform(DesignAffineTransform);

impl XrefPlacementTransform {
    /// Four row-major rows.
    pub(crate) fn rows(self) -> [[f64; 4]; 4] {
        self.0.rows()
    }
}

impl TryFrom<[[f64; 4]; 4]> for XrefPlacementTransform {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if valid_xref_placement_transform(&rows) {
            Ok(Self(DesignAffineTransform::try_from(rows)?))
        } else {
            Err("transform must be a finite proper rigid affine matrix".into())
        }
    }
}

impl From<XrefPlacementTransform> for [[f64; 4]; 4] {
    fn from(value: XrefPlacementTransform) -> Self {
        value.rows()
    }
}

/// Required XREF text. Whitespace is preserved; only an empty string is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct RequiredXrefText(String);

impl RequiredXrefText {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RequiredXrefText {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err("required XREF text must be non-empty");
        }
        Ok(Self(value))
    }
}

impl std::ops::Deref for RequiredXrefText {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for RequiredXrefText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One design entry of the top-level `RedirectionsStream.dat` table
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
/// The first source entry describes the document itself; each further entry
/// describes one referenced document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefDesign {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Position of this entry in the source `designs` array; entry 0 is the
    /// document itself.
    pub(crate) ordinal: u32,
    /// Source `file-version` integer.
    pub(crate) file_version: i64,
    /// The document's `.f3d` file name.
    pub(crate) target_file_name: RequiredXrefText,
    /// The document's display name.
    pub(crate) display_name: String,
    /// `urn:adsk.wipprod:dm.lineage:<key>` lineage identity.
    pub(crate) lineage_urn: String,
    /// `urn:adsk.wipprod:fs.file:vf.<key>?version=N` version identity.
    pub(crate) version_urn: String,
}

/// A DERIVED dependency in the container reference table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DerivedReference {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) from: RequiredXrefText,
    pub(crate) relative_path: RequiredXrefText,
    pub(crate) neutron_role: RequiredXrefText,
    pub(crate) neutron_data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct XrefLocalOwner {
    pub(crate) target: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_type_guid"
    )]
    pub(crate) type_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct XrefPlacementSource {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frame_bytes: cadmpeg_ir::native::bytes::NativeBytes,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_guid"
    )]
    pub(crate) occurrence_guid: Option<super::mesh::DesignRelaxedGuidText>,
    pub(crate) stream: String,
    pub(crate) carrier_record_index: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner"
    )]
    pub(crate) owner: Option<XrefLocalOwner>,
}

/// The component-path carrier that stores an occurrence's current world pose
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefCurrentPose {
    pub(crate) carrier_record_index: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frame_bytes: cadmpeg_ir::native::bytes::NativeBytes,
    /// Current pose in centimetres. `None` is the stored identity form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<XrefPlacementTransform>,
}

/// A direct operation table's pose in the source component definition frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefOperationPose {
    pub(crate) carrier_record_index: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frame_bytes: cadmpeg_ir::native::bytes::NativeBytes,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<XrefPlacementTransform>,
}

/// The world pose that a component-path carrier stores for one occurrence
/// inside an external occurrence
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefContainedPose {
    pub(crate) carrier_record_index: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frame_bytes: cadmpeg_ir::native::bytes::NativeBytes,
    /// The contained occurrence's placement record in the referenced design.
    pub(crate) contained_record_index: u64,
    /// The local occurrence carriers that hold the external occurrence,
    /// innermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owner_record_indices: Vec<u64>,
    /// World pose in centimetres. `None` is the stored identity form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<XrefPlacementTransform>,
}

/// The world pose that a component-path carrier stores for an external
/// occurrence below local owners
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefOwnedPose {
    pub(crate) carrier_record_index: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frame_bytes: cadmpeg_ir::native::bytes::NativeBytes,
    /// The local occurrence carriers that hold the external occurrence,
    /// innermost first.
    pub(crate) owner_record_indices: Vec<u64>,
    /// World pose in centimetres. `None` is the stored identity form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<XrefPlacementTransform>,
}

/// One outgoing XREF placement of the top-level `RedirectionsStream.dat` table
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefReference {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Position of this reference in the source `references` array.
    pub(crate) ordinal: u32,
    /// Zero-based occurrence position among Design records carrying this
    /// container reference's occurrence role.
    #[serde(default)]
    pub(crate) occurrence_ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement_source"
    )]
    pub(crate) placement_source: Option<XrefPlacementSource>,
    /// The referencing document's own file name.
    pub(crate) from: RequiredXrefText,
    /// The target design entry's `target_file_name`.
    pub(crate) relative_path: RequiredXrefText,
    /// Occurrence role joining this reference to the Design-segment
    /// `DcXRefPCIFeature` record and the ACT GUID pool.
    /// A role can be a GUID, a GUID with a URN suffix, or a version URN.
    pub(crate) neutron_role: RequiredXrefText,
    /// The independent `neutronData` property value. It is retained exactly
    /// and is never inferred from or aliased to `neutron_role`.
    pub(crate) neutron_data: String,
    /// Source Design occurrence transform in centimetres. An admitted
    /// `placement_source` with `None` stores the identity-placement form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<XrefPlacementTransform>,
    /// Current world pose from the occurrence's component-path carrier.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_current_pose"
    )]
    pub(crate) current_pose: Option<XrefCurrentPose>,
    /// The definition-local pose retained from a direct operation table.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operation_pose"
    )]
    pub(crate) operation_pose: Option<XrefOperationPose>,
    /// World poses that component-path carriers store for occurrences inside
    /// this external occurrence, each through this placement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) contained_poses: Vec<XrefContainedPose>,
    /// World poses that component-path carriers store for this external
    /// occurrence below its local owners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owned_poses: Vec<XrefOwnedPose>,
}

#[cfg(test)]
mod tests {
    use super::{XrefDesign, XrefReference};
    #[test]
    fn native_xref_required_text_refuses_empty_fields() {
        let design = serde_json::json!({"id":"f3d:xref:design#0","ordinal":0,"file_version":1,
            "target_file_name":"part.f3d","display_name":"","lineage_urn":"","version_urn":""});
        let reference = serde_json::json!({"id":"f3d:xref:reference#0","ordinal":0,
            "from":"root.f3d","relative_path":"part.f3d","neutron_role":"role","neutron_data":""});
        let mut empty = design.clone();
        empty["target_file_name"] = serde_json::json!("");
        assert!(serde_json::from_value::<XrefDesign>(empty).is_err());
        for field in ["from", "relative_path", "neutron_role"] {
            let mut empty = reference.clone();
            empty[field] = serde_json::json!("");
            assert!(
                serde_json::from_value::<XrefReference>(empty).is_err(),
                "{field}"
            );
            let mut whitespace = reference.clone();
            whitespace[field] = serde_json::json!(" ");
            let record = serde_json::from_value::<XrefReference>(whitespace.clone()).unwrap();
            whitespace["occurrence_ordinal"] = serde_json::json!(0);
            assert_eq!(serde_json::to_value(record).unwrap(), whitespace);
        }
        let mut whitespace = design;
        whitespace["target_file_name"] = serde_json::json!(" ");
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<XrefDesign>(whitespace.clone()).unwrap())
                .unwrap(),
            whitespace
        );
    }

    #[test]
    fn derived_reference_native_shape_round_trip() {
        let wire = serde_json::json!({
            "id": "f3d:derived:reference#0", "ordinal": 0,
            "from": "root.f3d", "relative_path": "source.f3d",
            "neutron_role": "role", "neutron_data": "data"
        });
        let restored: super::DerivedReference = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), wire);
        for field in ["from", "relative_path", "neutron_role"] {
            let mut invalid = wire.clone();
            invalid[field] = serde_json::json!("");
            assert!(serde_json::from_value::<super::DerivedReference>(invalid).is_err());
        }
    }
}
