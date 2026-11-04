// SPDX-License-Identifier: Apache-2.0
//! Component-path carriers of any length
//! ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).

use super::identity::DesignAffineTransform;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_path_transform,
    ComponentPathTransform,
    "transform"
);

const EPS_PATH_DETERMINANT: f64 = 1.0e-8;

/// A finite affine matrix with an orthonormal linear block. The determinant
/// is `1` or `-1`; `-1` is a mirrored placement.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct ComponentPathTransform(DesignAffineTransform);

impl ComponentPathTransform {
    /// Four row-major rows.
    pub(crate) fn rows(self) -> [[f64; 4]; 4] {
        self.0.rows()
    }
}

impl TryFrom<[[f64; 4]; 4]> for ComponentPathTransform {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        match super::xref::orthonormal_determinant(&rows) {
            Some(determinant) if (determinant.abs() - 1.0).abs() <= EPS_PATH_DETERMINANT => {
                Ok(Self(DesignAffineTransform::try_from(rows)?))
            }
            _ => Err("transform must be a finite orthonormal affine matrix".into()),
        }
    }
}

impl From<ComponentPathTransform> for [[f64; 4]; 4] {
    fn from(value: ComponentPathTransform) -> Self {
        value.rows()
    }
}

/// What a path element names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignComponentPathElementKind {
    /// A component-definition record that a local occurrence carrier of the
    /// same Design stream names as its definition.
    Component,
    /// A local occurrence carrier of the same Design stream.
    Occurrence,
    /// A record that no local occurrence carrier names or is.
    Other,
}

impl DesignComponentPathElementKind {
    pub(crate) fn admits_inline_type(self, guid: Option<&str>) -> bool {
        guid.is_none_or(|guid| match self {
            Self::Component => guid.eq_ignore_ascii_case("E03784ED-5E19-4E14-B9F2-3B07017018CD"),
            Self::Occurrence => guid.eq_ignore_ascii_case("CE2913AA-CFE0-4F04-9102-24424ED3BCFA"),
            Self::Other => false,
        })
    }
}

/// One element of a component path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignComponentPathElement {
    /// Indexed record that the element references.
    pub(crate) record_index: u64,
    pub(crate) kind: DesignComponentPathElementKind,
    /// The stored u32 instance discriminator.
    pub(crate) discriminator: u32,
}

/// A component-path carrier whose elements are all local references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignComponentPath {
    /// Stable native record identity.
    pub(crate) id: String,
    /// The carrier's own indexed record.
    pub(crate) carrier_record_index: u64,
    /// Path elements in stored order.
    pub(crate) elements: Vec<DesignComponentPathElement>,
    /// World pose in centimetres. `None` is the stored identity form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path_transform"
    )]
    pub(crate) transform: Option<ComponentPathTransform>,
    /// The complete carrier record, header included.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frame_bytes: cadmpeg_ir::native::bytes::NativeBytes,
}
