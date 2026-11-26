//! Contract and reply types for `inspect.socket_reach` in `cmd/socket_reach.rs` (#21884).

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::types::{Point3, StringList};
use crate::params;

/// Largest socket diameter and length checked (mm).
pub const MAX_SIZE: f64 = 10_000.0;

/// A length in mm above 0 and at most [`MAX_SIZE`], as the handler reads it.
macro_rules! positive_mm {
    ($read:ident, $schema:ident, $key:literal) => {
        fn $read<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
            Value::deserialize(d)?
                .as_f64()
                .and_then(params::finite)
                .filter(|x| *x > 0.0 && *x <= MAX_SIZE)
                .ok_or_else(|| D::Error::custom(format!("`{}` must be a length in mm above 0 and at most {}", $key, MAX_SIZE)))
        }

        fn $schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({"type": "number", "exclusiveMinimum": 0, "maximum": MAX_SIZE})
        }
    };
}

positive_mm!(diameter, diameter_schema, "diameter");
positive_mm!(length, length_schema, "length");

/// `inspect.socket_reach`: whether a socket can reach a fastener along its axis.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SocketReachParams {
    /// A point on a circular edge (a hole's rim, a bolt head's or a nut's top edge): the socket
    /// turns about its axis and starts in its plane.
    pub edge: Point3,
    /// The socket's outside diameter, in mm.
    #[serde(deserialize_with = "diameter")]
    #[schemars(schema_with = "diameter_schema")]
    pub diameter: f64,
    /// How far along the axis to check, from the edge's plane, in mm.
    #[serde(deserialize_with = "length")]
    #[schemars(schema_with = "length_schema")]
    pub length: f64,
    /// Approach from the other side. Default: from the side the edge has no material on.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Bodies the socket may overlap, such as the fastener itself.
    #[serde(default)]
    pub ignore: Option<StringList>,
}

/// `inspect.socket_reach`'s reply: the space the socket needs and what is in it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SocketReach {
    /// The centre of the circular edge: where the socket's space starts.
    pub origin: [f64; 3],
    /// The unit direction the socket's space runs from the edge's plane.
    pub direction: [f64; 3],
    /// The socket's outside diameter, in mm.
    pub diameter_mm: f64,
    /// The length checked, in mm.
    pub length_mm: f64,
    /// Whether nothing blocks the socket.
    pub clear: bool,
    /// The bodies in the socket's space, the largest overlap first.
    pub blocked: Vec<Blocker>,
    /// Bodies not checked, with why (a mesh body, an overlap the kernel could not work out).
    pub unchecked: Vec<Unchecked>,
}

/// A body in the socket's space.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Blocker {
    /// The body's name.
    pub body: String,
    /// The volume it takes from the socket's space, in mm³.
    pub volume_mm3: f64,
    /// How far along the axis from the edge's plane the body first enters the space, in mm.
    pub from_mm: f64,
    /// The box around the overlap: `[min, max]`.
    pub bbox: [[f64; 3]; 2],
}

/// A body that could not be checked.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Unchecked {
    /// The body's name.
    pub body: String,
    /// Why.
    pub reason: String,
}
