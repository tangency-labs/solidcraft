//! Socket Reach (`inspect.socket_reach`, #21884): whether a socket can reach a fastener. The
//! socket's space is a cylinder of its outside diameter about a circular edge's axis, from the
//! edge's plane along the side the edge has no material on (or the other, flipped). Each solid
//! body whose box meets it is intersected with it on the exact faces; a body taking more than a
//! contact's volume blocks the socket. Nothing is added to the design.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::{bad, bool_, num, string_list};
use crate::schema::socket_reach::{Blocker, MAX_SIZE, SocketReach, SocketReachParams, Unchecked};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CONTRACT: TypedContract<SocketReachParams> = TypedContract::new();

/// A smaller shared volume is contact, not a block (mm³), as Clearance's default.
const CONTACT_VOLUME: f64 = 0.01;

/// Most bodies one call checks.
const MAX_BODIES: usize = 20_000;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.socket_reach", "Socket Reach", crate::checked!("inspect.socket_reach", socket_reach))
    .at("SOLID", "INSPECT")
    .icon("interference")
    .noundo()
    .read_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<SocketReach>)
    .params(
        "edge: [x,y,z] (a point on a circular edge: a hole's rim, a bolt head's or a nut's top edge); diameter: mm (the socket's outside diameter); \
         length: mm (how far along the axis to check, from the edge's plane); flip?: bool (approach from the other side; default: the side the edge has no material on); \
         ignore?: [bodies] (bodies the socket may overlap, such as the fastener) \
         → origin (the edge's centre), direction (unit, the way the socket's space runs), diameter_mm, length_mm, clear: bool, \
         blocked: [{body, volume_mm3, from_mm (along the axis from the edge's plane), bbox}] (largest first), unchecked: [{body, reason}] (mesh bodies). \
         Overlaps are measured on the exact faces; nothing is added to the design",
    )];

/// The socket's space: where it starts, the way it runs, and the cylinder.
pub(crate) struct Space {
    pub origin: Vec3,
    pub direction: Vec3,
    pub body: solidcraft_kernel::Body,
}

/// The socket's space about the circular edge through `pick`: `diameter` wide and `length` long,
/// from the edge's plane along the side the edge has no material on, or the other with `flip`.
pub(crate) fn space(s: &Session, pick: Vec3, diameter: f64, length: f64, flip: bool, cmd: &str) -> Result<Space> {
    let Some((_, origin, axis)) = super::construct::model_axis_param(s, Some(&json!({ "edge": [pick.x, pick.y, pick.z] })), cmd)? else {
        return Err(bad(cmd, "`edge` must be a point on a circular edge"));
    };
    let axis = axis.normalized().ok_or_else(|| bad(cmd, "the edge gives no axis"))?;
    let st = s.world_state()?;
    // A straight edge has an axis too, along itself, but no centre for a socket to turn about.
    let radial = (pick - origin) - axis * (pick - origin).dot(axis);
    if radial.len() < 1e-6 {
        return Err(bad(cmd, "`edge` must be a point on a circular edge: a straight edge gives no centre"));
    }
    // The side the edge has material on: points just off the edge, outside and inside the
    // circle (a hole's rim has it outside, a bolt head's top edge inside), a little along each
    // way. The socket comes from the other side.
    let step = (0.05 * radial.len()).clamp(0.01, 0.5);
    let outward = radial.normalized().unwrap_or(Vec3::ZERO);
    let inside = |q: Vec3| {
        st.bodies
            .iter()
            .filter(|b| !b.body.is_mesh())
            .any(|b| b.body.classify_point(q).is_ok_and(|c| matches!(c, solidcraft_kernel::PointClass::Inside)))
    };
    let material = |side: f64| [1.0, -1.0].iter().any(|across| inside(pick + outward * (across * step) + axis * (side * step)));
    let along = if material(1.0) && !material(-1.0) { -axis } else { axis };
    let direction = if flip { -along } else { along };
    let body = solidcraft_kernel::cylinder(origin, direction, diameter / 2.0, length).map_err(|e| bad(cmd, format!("the socket's space: {e}")))?;
    Ok(Space { origin, direction, body })
}

fn socket_reach(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.socket_reach";
    let size = |k: &str| -> Result<f64> {
        num(p, k)
            .filter(|x| *x > 0.0 && *x <= MAX_SIZE)
            .ok_or_else(|| bad(cmd, format!("`{k}` must be a length in mm above 0 and at most {MAX_SIZE}")))
    };
    let (diameter, length) = (size("diameter")?, size("length")?);
    let flip = match p.get("flip") {
        None | Some(Value::Null) => false,
        Some(_) => bool_(p, "flip").ok_or_else(|| bad(cmd, "`flip` must be true or false"))?,
    };
    let ignore = string_list(cmd, p, "ignore")?;
    let pick = p.get("edge").and_then(crate::params::vec3).ok_or_else(|| bad(cmd, "`edge` must be a point [x, y, z] on a circular edge"))?;
    let st = s.world_state()?;
    if st.bodies.len() > MAX_BODIES {
        return Err(bad(cmd, format!("at most {MAX_BODIES} bodies")));
    }
    if let Some(n) = ignore.iter().find(|n| st.body(n).is_none()) {
        return Err(bad(cmd, format!("no body `{n}`")));
    }
    let Space { origin, direction, body: space } = space(s, pick, diameter, length, flip, cmd)?;
    let space_box = solidcraft_kernel::measure(&space).map_err(|e| bad(cmd, e.to_string()))?.bbox;
    let mut blocked = Vec::new();
    let mut unchecked = Vec::new();
    for b in &st.bodies {
        if ignore.contains(&b.name) {
            continue;
        }
        if b.body.is_mesh() {
            unchecked.push(Unchecked { body: b.name.clone(), reason: "mesh body: not checked".into() });
            continue;
        }
        // A body whose exact box stays clear of the space's cannot reach into it. A box the
        // kernel cannot give rules nothing out: the body is intersected.
        if b.body.exact_bounds().is_ok_and(|bounds| solidcraft_geom::mesh_distance::box_gap(&bounds, &space_box) > 0.0) {
            continue;
        }
        let shared = match solidcraft_kernel::boolean(&b.body, &space, solidcraft_kernel::BoolOp::Intersect) {
            Ok(Some(shared)) => shared,
            Ok(None) => continue,
            Err(e) => {
                unchecked.push(Unchecked { body: b.name.clone(), reason: format!("the overlap could not be worked out: {e}") });
                continue;
            }
        };
        let m = match solidcraft_kernel::measure(&shared) {
            Ok(m) => m,
            Err(e) => {
                unchecked.push(Unchecked { body: b.name.clone(), reason: format!("the overlap could not be measured: {e}") });
                continue;
            }
        };
        if m.volume <= CONTACT_VOLUME {
            continue;
        }
        let (lo, hi) = (m.bbox.min, m.bbox.max);
        let from = (0..8)
            .map(|k: usize| {
                let corner =
                    Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
                (corner - origin).dot(direction)
            })
            .fold(f64::INFINITY, f64::min)
            .clamp(0.0, length);
        blocked.push(Blocker { body: b.name.clone(), volume_mm3: m.volume, from_mm: from, bbox: [[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]] });
    }
    blocked.sort_by(|a, b| b.volume_mm3.total_cmp(&a.volume_mm3));
    let reply = SocketReach {
        origin: [origin.x, origin.y, origin.z],
        direction: [direction.x, direction.y, direction.z],
        diameter_mm: diameter,
        length_mm: length,
        clear: blocked.is_empty(),
        blocked,
        unchecked,
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}
