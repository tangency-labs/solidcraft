//! Contracts and result types for the commands in `cmd/heat_insert.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcore_record::Vec3;

use super::check::whole;
use super::types::{IdOrName, Point3, StringList, present};
use crate::params;

/// The most inserts one call places.
pub const MAX_INSERTS: usize = 1000;

/// Where the inserts go: one point on a flat face, or a list of them (a list's first item is a
/// list), with the handler's messages.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertPositions(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for InsertPositions {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom(whole("`position` must be a point [x, y, z] on a flat face, or a list of them"));
        let one = |v: &Value| params::vec3(v).ok_or_else(fail);
        match Value::deserialize(d)? {
            Value::Array(list) if list.first().is_some_and(Value::is_array) => {
                if list.len() > MAX_INSERTS {
                    return Err(D::Error::custom(whole(format!("`position` takes at most {MAX_INSERTS} points"))));
                }
                list.iter().map(one).collect::<Result<_, _>>().map(InsertPositions)
            }
            v => Ok(InsertPositions(vec![one(&v)?])),
        }
    }
}

impl JsonSchema for InsertPositions {
    fn schema_name() -> Cow<'static, str> {
        "InsertPositions".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A point on a flat face of a solid body, in mm, or a list of them (one insert each, at most 1000).",
            "anyOf": [point.clone(), {"type": "array", "prefixItems": [{"allOf": [{"type": "array"}, point.clone()]}], "items": point, "minItems": 1, "maxItems": MAX_INSERTS}]
        })
    }
}

/// What a Heat-Set Insert does when the wall or the floor around the insert is too thin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum InsertCheck {
    /// Refuse, and leave the design unchanged.
    #[default]
    Stop,
    /// Drill the hole and list the thin wall or floor in `warnings`.
    Warn,
    /// Drill the hole without measuring the wall or the floor.
    Off,
}

/// A lead-in chamfer: a width in mm, or `true` for one out to the insert's outer diameter
/// (`false`: none).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ChamferParam {
    /// Out to the insert's outer diameter (`true`), or none (`false`).
    Outer(bool),
    /// The chamfer's width across, in mm.
    Width(f64),
}

/// Drill blind holes for heat-set threaded inserts at points on flat faces, after checking that
/// the material around each insert is thick enough. Every point is checked before any hole is
/// drilled: one that is too thin stops them all (with `check: stop`). The hole takes the insert's hole
/// diameter; its depth is the insert's length plus `extra_depth`. The wall is the material
/// around the insert's outer diameter, over its length; the floor is the material below the
/// hole, under the insert. Both are checked on the body's exact geometry. Every value of the size can be changed; without `size`, give
/// `outer_diameter`, `length`, `hole_diameter` and `min_wall`. The result is a Hole feature.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct HeatInsertParams {
    /// Where the holes go: a point on a flat face of a solid body, in mm (the centre of the hole),
    /// or a list of them. Each hole goes into its face, against the face's outward normal. Give
    /// this or `sketch`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "InsertPositions")]
    pub position: Option<InsertPositions>,
    /// A sketch on a face, by id or name: a hole at each of its points, which follows them. Give
    /// this or `position`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// With `sketch`: the sketch point ids to place inserts at. Default: every standalone point and
    /// circle centre of the sketch.
    #[serde(default)]
    pub points: Option<StringList>,
    /// The insert size, as `plastic.heat_insert_sizes` lists it: `M3x5.7`, `#4-40x5.7`,
    /// `1/4-20x12.7`, or a thread alone (`M3`) for that thread's standard length. Matching ignores
    /// case and spaces, and takes `×` for `x` and `,` for `.`. Default: none (give every value).
    #[serde(default)]
    pub size: Option<String>,
    /// The body to drill, by name. Default: the one solid body whose face holds each point.
    #[serde(default)]
    pub body: Option<String>,
    /// The insert's outer diameter, in mm; the wall is measured from it. Default: the size's.
    #[serde(default)]
    pub outer_diameter: Option<f64>,
    /// The insert's length, in mm. Default: the size's.
    #[serde(default)]
    pub length: Option<f64>,
    /// The diameter of the hole, in mm; at most `outer_diameter`. Default: the size's.
    #[serde(default)]
    pub hole_diameter: Option<f64>,
    /// The least wall around the insert, in mm. Default: the size's.
    #[serde(default)]
    pub min_wall: Option<f64>,
    /// The least floor below the hole, in mm. Default: `min_wall`.
    #[serde(default)]
    pub min_floor: Option<f64>,
    /// How much deeper than the insert the hole goes, in mm, for the plastic the insert pushes
    /// down. Default: 1. Ignored when `depth` is given.
    #[serde(default)]
    pub extra_depth: Option<f64>,
    /// The hole's depth, in mm; at least `length`. Default: `length` plus `extra_depth`.
    #[serde(default)]
    pub depth: Option<f64>,
    /// A 45° chamfer at the hole's mouth that leads the insert in: its width across, in mm, or
    /// `true` for a chamfer out to the insert's outer diameter. It takes at most half the hole's
    /// depth; a chamfer wider than the insert is part of the wall check. The Hole feature is then
    /// a 90° countersink. Default: none.
    #[serde(default)]
    pub chamfer: Option<ChamferParam>,
    /// What a wall or a floor thinner than its least value does. Default: stop.
    #[serde(default)]
    pub check: Option<InsertCheck>,
    /// The Hole feature's name in the timeline; with several points, the later holes add their
    /// number (`Name 2`, `Name 3`).
    #[serde(default)]
    pub name: Option<String>,
}

/// One insert's hole.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InsertHole {
    /// The centre of the hole on its face, in mm.
    pub position: [f64; 3],
    /// The body drilled.
    pub body: String,
    /// The Hole feature's id (the same for every point of a sketch).
    pub feature: u64,
    /// The Hole feature's name.
    pub name: String,
    /// The wall measured around the insert, in mm, when it is thinner than `min_wall` (to a
    /// thousandth of `min_wall`). Absent when it is at least `min_wall`, or when `check` was off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall: Option<f64>,
    /// The floor measured below the hole, in mm, when it is thinner than `min_floor`. Absent when
    /// it is at least `min_floor`, or when `check` was off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<f64>,
}

/// What `plastic.heat_insert` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HeatInsertResult {
    /// The first feature made (Edit Feature rebuilds one): a Heat-Set Insert holds the holes of one
    /// face, or of one sketch.
    pub feature: u64,
    /// The holes, in the order of the points.
    pub holes: Vec<InsertHole>,
    /// The insert size used, as `plastic.heat_insert_sizes` lists it; absent for an insert given
    /// by its values alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// The insert's outer diameter, in mm.
    pub outer_diameter: f64,
    /// The insert's length, in mm.
    pub length: f64,
    /// The hole's diameter, in mm.
    pub hole_diameter: f64,
    /// The hole's depth, in mm.
    pub depth: f64,
    /// The lead-in chamfer's width across, in mm; absent without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer: Option<f64>,
    /// The least wall the check asked for, in mm.
    pub min_wall: f64,
    /// The least floor the check asked for, in mm.
    pub min_floor: f64,
    /// Whether the walls and the floors were checked (false with `check: off`).
    pub checked: bool,
    /// With `check: warn`: each wall or floor that is thinner than asked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// List the built-in heat-set insert sizes `plastic.heat_insert` takes. The values are starting
/// points from manufacturers' published dimensions: change any of them in the call for your
/// inserts and your printer.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct HeatInsertSizesParams {
    /// Only the sizes for this thread (`M3`, `#4-40`, `1/4-20`). Matching ignores case and spaces.
    /// Default: every size.
    #[serde(default)]
    pub thread: Option<String>,
}

/// One built-in heat-set insert size.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InsertSizeInfo {
    /// The size's id, which `plastic.heat_insert`'s `size` takes.
    pub id: String,
    /// The thread the insert carries.
    pub thread: String,
    /// The insert's outer diameter, in mm.
    pub outer_diameter: f64,
    /// The insert's length, in mm.
    pub length: f64,
    /// The recommended hole diameter, in mm.
    pub hole_diameter: f64,
    /// The least wall around the insert, in mm.
    pub min_wall: f64,
    /// Whether this is the size `size` means when it names the thread alone.
    pub standard: bool,
}

/// What `plastic.heat_insert_sizes` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HeatInsertSizesResult {
    /// The sizes, metric first, each thread from short to long.
    pub sizes: Vec<InsertSizeInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{HeatInsertParams, HeatInsertSizesParams};
    use crate::schema::input_schema_for;
    use crate::schema::tests::assert_keys;

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(
            input_schema_for::<HeatInsertParams>(),
            &[
                "body",
                "chamfer",
                "check",
                "depth",
                "extra_depth",
                "hole_diameter",
                "length",
                "min_floor",
                "min_wall",
                "name",
                "outer_diameter",
                "points",
                "position",
                "size",
                "sketch",
            ],
        );
        assert_keys(input_schema_for::<HeatInsertSizesParams>(), &["thread"]);
    }
}
