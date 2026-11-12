//! A machine model for the simulation view: the table with its holes, how high the spindle nose
//! sits above the tool and how high it can go, and the machine's parts as boxes and cylinders by
//! what they move with (the format `solidcraft.machine_model`, version 1).
//! Collisions use these primitives, exactly; meshes in the file are pictures, never checked.
//!
//! Frames: the **table frame** has its origin at the table's front-left top corner, X to the
//! right, Y to the back, Z up. A **spindle** part's frame is on the spindle axis, its Z up from
//! the spindle nose (`spindle.nose_gap` above the top of the tool's holder); a **carriage** part
//! follows the tool in X and Y, its Z up from the table top; a **frame** part follows the tool
//! in Y only, its X from the table's centre (a machine whose table moves in Y); a **table** part
//! is fixed in the table frame.
//!
//! [`zones`] places the tool's cutting length, its shank, every holder section and every part
//! for a vertical tool at a tip position (relative to the table, so on a 4th axis the position
//! of the linear axes); [`clearance`] holds them against the workholding's boxes along a move,
//! and the part list against the stock. Ported from the author's own MIT-licensed G-code simulator:
//! the mounts, the part shapes, the zones and the clearance check.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::findings::Severity;
use crate::kinematics::Machine;
use crate::machine_time::MachineRates;
use crate::obstacle::{ObstacleShape, Turn};
use crate::sim::Footprint;
use crate::tool::Tool;
use crate::{Result, invalid};

/// The format id of a machine model file.
pub const FORMAT: &str = "solidcraft.machine_model";
/// Most parts, holes and meshes a model may have.
pub const MAX_PARTS: usize = 1000;
pub const MAX_HOLES: usize = 10_000;
pub const MAX_MESHES: usize = 64;
/// Most triangles of all the display meshes together.
pub const MAX_TRIANGLES: usize = 10_000_000;
/// Largest size or coordinate accepted (mm).
const MAX_LEN: f64 = 1.0e5;

/// What a part moves with, relative to the work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mount {
    /// X, Y and Z: on the spindle axis, Z from the spindle nose.
    Spindle,
    /// X and Y: on the spindle axis, Z from the table top.
    Carriage,
    /// Y only: X from the table's centre, Y from the spindle axis, Z from the table top.
    Frame,
    /// Nothing: in the table frame.
    Table,
}

/// A part's shape in its frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PartShape {
    /// An upright cylinder from `z[0]` to `z[1]`, on the frame's axis or about `center`.
    Cylinder {
        diameter: f64,
        z: [f64; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center: Option<[f64; 2]>,
    },
    /// A box square to the frame.
    Box { min: [f64; 3], max: [f64; 3] },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachinePart {
    /// Reads in a sentence: "the Z plate".
    pub name: String,
    /// The finding codes' stem (kebab-case): `z-plate-hits-vise`, `z-plate-hits-stock`.
    pub code: String,
    pub mount: Mount,
    pub shape: PartShape,
}

/// A hole in the table: its centre (table frame), diameter, depth from the top.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hole {
    pub x: f64,
    pub y: f64,
    pub d: f64,
    pub depth: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

/// The table: a plate `size` (X, Y, thickness) under the table frame's origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub size: [f64; 3],
    #[serde(default)]
    pub corner_radius: f64,
    /// How deep the cutter may go into the table top before it is a collision (through-cuts).
    #[serde(default)]
    pub cut_allowance: f64,
    #[serde(default)]
    pub holes: Vec<Hole>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spindle {
    /// From the top of the tool's holder to the spindle nose (mm).
    pub nose_gap: f64,
    /// The highest the nose gets above the table top (mm): the Z-travel check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nose_max_above_table: Option<f64>,
    /// The spindle's lowest and highest speed (rpm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<[f64; 2]>,
}

/// Soft limits in machine coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoftLimits {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// A display mesh named by a model: a mesh-parts JSON file ([`mesh_parts`]), an STL file or a
/// STEP file (read through the import).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayMesh {
    pub path: String,
    /// For an STL or STEP file: what it moves with (default `spindle`), and its colour (0 to 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<Mount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f64; 3]>,
    /// For an STL or STEP file: its name in the findings.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// For an STL or STEP file: rows of a 3 x 4 matrix (rotation and shift, mm) from the file's
    /// coordinates to its mount's frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[[f64; 4]; 3]>,
    /// For an STL or STEP file on `mount: table`: the tool and the machine are checked against it
    /// too (a part of the machine on the table). Default false: drawn only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub check: bool,
}

/// A machine model file (`solidcraft.machine_model`, version 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineModel {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub name: String,
    /// The machine's axes (SolidCraft's machine JSON), inline or a file beside this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinematics: Option<Machine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinematics_path: Option<String>,
    /// The machine's rates, inline or a file beside this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rates: Option<MachineRates>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rates_path: Option<String>,
    pub spindle: Spindle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_limits: Option<SoftLimits>,
    pub table: Table,
    #[serde(default)]
    pub parts: Vec<MachinePart>,
    #[serde(default)]
    pub display_meshes: Vec<DisplayMesh>,
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= MAX_LEN)
}

/// Whether `s` is a kebab-case code: lower-case letters, digits and single dashes, at most 64.
pub fn is_code(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// Whether `s` is a name to show: at most 200 characters, no control characters.
pub fn is_name(s: &str) -> bool {
    s.chars().count() <= 200 && !s.chars().any(char::is_control)
}

impl MachineModel {
    /// A model read from JSON text and checked; unknown keys are refused.
    pub fn from_json(text: &str) -> Result<MachineModel> {
        let m: MachineModel = serde_json::from_str(text).map_err(|e| crate::CamError::Invalid(format!("the machine model: {e}")))?;
        m.validate()?;
        Ok(m)
    }

    /// A machine model file read from JSON text and checked, its unknown keys (a newer build's)
    /// left out and named.
    pub fn from_json_lenient(text: &str) -> Result<(MachineModel, Vec<String>)> {
        let (m, ignored): (MachineModel, Vec<String>) =
            crate::keys::from_json_lenient(text).map_err(|e| crate::CamError::Invalid(format!("the machine model: {e}")))?;
        m.validate()?;
        Ok((m, ignored))
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT || self.version != 1 {
            return invalid(format!("a machine model file has `format: \"{FORMAT}\"` and `version: 1`"));
        }
        if !is_name(&self.name) {
            return invalid("the machine model's `name` must be at most 200 characters, without control characters");
        }
        if self.kinematics.is_some() && self.kinematics_path.is_some() {
            return invalid("give the machine model `kinematics` or `kinematics_path`, not both");
        }
        if self.rates.is_some() && self.rates_path.is_some() {
            return invalid("give the machine model `rates` or `rates_path`, not both");
        }
        if let Some(k) = &self.kinematics {
            k.validate()?;
        }
        if let Some(r) = &self.rates {
            r.validate()?;
        }
        let s = &self.spindle;
        if !(finite(&[s.nose_gap]) && s.nose_gap >= 0.0) {
            return invalid("the spindle's `nose_gap` must be 0 or more, at most 100 m");
        }
        if s.nose_max_above_table.is_some_and(|n| !(finite(&[n]) && n > 0.0)) {
            return invalid("the spindle's `nose_max_above_table` must be above 0, at most 100 m");
        }
        if let Some([lo, hi]) = s.rpm
            && !(lo.is_finite() && hi.is_finite() && lo >= 0.0 && lo <= hi && hi <= 1.0e6)
        {
            return invalid("the spindle's `rpm` must be [lowest, highest], the lowest first, at most 1000000");
        }
        if let Some(l) = &self.soft_limits
            && !(finite(&l.min) && finite(&l.max) && l.min.iter().zip(&l.max).all(|(a, b)| a <= b))
        {
            return invalid("the soft limits must be finite, each `min` at most its `max`");
        }
        let t = &self.table;
        if !(finite(&t.size) && t.size.iter().all(|v| *v > 0.0)) {
            return invalid("the table's `size` must be three lengths above 0, at most 100 m");
        }
        if !(finite(&[t.corner_radius, t.cut_allowance]) && t.corner_radius >= 0.0 && t.cut_allowance >= 0.0) {
            return invalid("the table's `corner_radius` and `cut_allowance` must be 0 or more");
        }
        if t.holes.len() > MAX_HOLES {
            return invalid(format!("a table has at most {MAX_HOLES} holes"));
        }
        for h in &t.holes {
            if !(finite(&[h.x, h.y, h.d, h.depth]) && h.d > 0.0 && h.depth > 0.0) || !is_name(&h.label) {
                return invalid("a table hole needs a finite centre, a diameter and a depth above 0, and a short label");
            }
        }
        if self.parts.len() > MAX_PARTS {
            return invalid(format!("a machine model has at most {MAX_PARTS} parts"));
        }
        for p in &self.parts {
            if !is_name(&p.name) || p.name.trim().is_empty() {
                return invalid("every machine part needs a `name` of at most 200 characters, without control characters");
            }
            if !is_code(&p.code) {
                return invalid(format!("machine part `{}`: its `code` must be kebab-case (a-z, 0-9 and dashes), at most 64", p.name));
            }
            let ok = match &p.shape {
                PartShape::Cylinder { diameter, z, center } => {
                    finite(&[*diameter, z[0], z[1]]) && *diameter > 0.0 && z[0] < z[1] && center.is_none_or(|c| finite(&c))
                }
                PartShape::Box { min, max } => finite(min) && finite(max) && min.iter().zip(max).all(|(a, b)| a < b),
            };
            if !ok {
                return invalid(format!(
                    "machine part `{}`: a cylinder needs a diameter above 0 and z [low, high]; a box min below max on every axis, all finite",
                    p.name
                ));
            }
        }
        if self.display_meshes.len() > MAX_MESHES {
            return invalid(format!("a machine model has at most {MAX_MESHES} display meshes"));
        }
        for d in &self.display_meshes {
            if d.path.trim().is_empty() || d.path.len() > 4096 || d.color.is_some_and(|c| !c.iter().all(|v| (0.0..=1.0).contains(v))) {
                return invalid("a display mesh needs a `path`, and a `color` of three numbers from 0 to 1");
            }
            if !is_name(&d.name) || d.transform.is_some_and(|t| !t.iter().all(|row| finite(row))) {
                return invalid(format!("display mesh {}: a short `name`, and a `transform` of three rows of four finite numbers", d.path));
            }
            if d.check && d.mount != Some(Mount::Table) {
                return invalid(format!(
                    "display mesh {}: only a mesh on `mount: table` is checked against; a moving part is checked as the model's `parts`",
                    d.path
                ));
            }
        }
        Ok(())
    }
}

/// A display mesh part: what it moves with, its colour (0 to 1) and its triangles in its frame.
#[derive(Clone, Debug)]
pub struct MeshPart {
    pub name: String,
    pub mount: Mount,
    pub color: [f64; 3],
    pub mesh: Mesh,
}

#[derive(Deserialize)]
struct MeshPartsFile {
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<String>,
    parts: Vec<MeshPartJson>,
}

#[derive(Deserialize)]
struct MeshPartJson {
    #[serde(default)]
    name: String,
    mount: Mount,
    #[serde(default)]
    color: Option<[f64; 3]>,
    tris: Vec<f64>,
}

/// The parts of a mesh-parts file (`{parts: [{name, mount, color, tris: [x, y, z, ...]}]}`, nine
/// numbers a triangle), at most `max_triangles` triangles in all. For display only.
pub fn mesh_parts(text: &str, max_triangles: usize) -> Result<Vec<MeshPart>> {
    let f: MeshPartsFile = serde_json::from_str(text).map_err(|e| crate::CamError::Invalid(format!("the mesh parts: {e}")))?;
    if f.parts.len() > MAX_PARTS {
        return invalid(format!("a mesh-parts file has at most {MAX_PARTS} parts"));
    }
    let mut total = 0usize;
    let mut out = Vec::with_capacity(f.parts.len());
    for p in f.parts {
        if p.tris.len() % 9 != 0 || !finite(&p.tris) {
            return invalid("a mesh part's `tris` must be finite numbers, nine a triangle");
        }
        total = total.saturating_add(p.tris.len() / 9);
        if total > max_triangles.min(MAX_TRIANGLES) {
            return Err(crate::CamError::TooLarge(format!("the mesh parts have more than {} triangles", max_triangles.min(MAX_TRIANGLES))));
        }
        let color = p.color.unwrap_or([0.7, 0.7, 0.7]);
        if !color.iter().all(|v| (0.0..=1.0).contains(v)) || !is_name(&p.name) {
            return invalid("a mesh part's `color` must be three numbers from 0 to 1, its name short");
        }
        let mut mesh = Mesh::default();
        for (k, t) in p.tris.as_chunks::<9>().0.iter().enumerate() {
            for [x, y, z] in t.as_chunks::<3>().0 {
                mesh.positions.push(Vec3::new(*x, *y, *z));
            }
            let base = u32::try_from(k * 3).map_err(|_| crate::CamError::TooLarge("a mesh part is too large".into()))?;
            mesh.triangles.push([base, base + 1, base + 2]);
        }
        out.push(MeshPart { name: p.name, mount: p.mount, color, mesh });
    }
    Ok(out)
}

/// What kind of thing a workholding box is (the finding codes' last word).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObstacleKind {
    Table,
    Vise,
    Clamp,
    Parallels,
    /// What the vise grips instead of the stock (a fixture, a sub-plate).
    Grip,
    /// A rotary unit's headstock, chuck, tailstock, plate, parts, rings or jaws.
    Rotary,
    /// A fixture given as a mesh in a workholding that is neither a vise, clamps nor a rotary unit.
    Fixture,
    /// A part of the machine fixed to the table (a machine model's checked mesh).
    Machine,
}

impl ObstacleKind {
    pub fn code(self) -> &'static str {
        match self {
            ObstacleKind::Table => "table",
            ObstacleKind::Vise => "vise",
            ObstacleKind::Clamp => "clamp",
            ObstacleKind::Parallels => "parallels",
            ObstacleKind::Grip => "grip",
            ObstacleKind::Rotary => "rotary",
            ObstacleKind::Fixture => "fixture",
            ObstacleKind::Machine => "machine",
        }
    }

    /// For messages, which merge every hit of a kind: "a clamp", not "clamp 2".
    pub fn noun(self) -> &'static str {
        match self {
            ObstacleKind::Table => "the table",
            ObstacleKind::Vise => "the vise",
            ObstacleKind::Clamp => "a clamp",
            ObstacleKind::Parallels => "the parallels",
            ObstacleKind::Grip => "the fixture the vise grips",
            ObstacleKind::Rotary => "the rotary unit",
            ObstacleKind::Fixture => "a fixture",
            ObstacleKind::Machine => "the machine",
        }
    }
}

/// Something the tool and the machine must keep out of, in work coordinates (with every table
/// rotary at zero): its box, and its shape when it is not the box itself (a prism along the A
/// axis, a mesh: [`ObstacleShape`]). The box holds the shape wherever A turns it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Obstacle {
    /// Reads in a sentence: "the vise's fixed jaw", "clamp 2".
    pub name: String,
    pub kind: ObstacleKind,
    pub min: Vec3,
    pub max: Vec3,
    #[serde(skip)]
    pub shape: ObstacleShape,
}

impl Obstacle {
    /// A box.
    pub fn boxed(name: impl Into<String>, kind: ObstacleKind, min: Vec3, max: Vec3) -> Obstacle {
        Obstacle { name: name.into(), kind, min, max, shape: ObstacleShape::Box }
    }

    /// Whether it turns with A (fixed to the work, not the bed).
    pub fn turns(&self) -> bool {
        matches!(self.shape, ObstacleShape::AlongX { turns: true, .. })
    }
}

/// What a zone is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneOf {
    /// The cutting length.
    Cutter,
    /// The plain shank from the flutes to the holder.
    Shank,
    /// A holder section.
    Holder(usize),
    /// A machine part, by index.
    Part(usize),
}

/// Something above the tip, as an outline seen from above and the heights it spans.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub of: ZoneOf,
    pub fp: Footprint,
    pub z0: f64,
    pub z1: f64,
}

/// Where the tool and machine are placed: the table's top-left-front corner and centre X in
/// the same coordinates as the tip (work coordinates with the table rotaries at zero).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TablePlace {
    pub corner: Vec3,
    pub size: [f64; 3],
}

/// How high the spindle nose is above the tip: the tool's stickout, its holder, the nose gap.
pub fn nose_above_tip(tool: &Tool, model: Option<&MachineModel>) -> f64 {
    let stickout = tool.stickout.unwrap_or(tool.flute_length).max(tool.flute_length);
    stickout + tool.holder.iter().map(|h| h.length).sum::<f64>() + model.map_or(0.0, |m| m.spindle.nose_gap)
}

/// Everything above a vertical tool's tip at `tip`: its cutting length, shank and holder
/// sections, then (with a model) every part placed by its mount; carriage, frame and table parts
/// only when the table's place is known.
pub fn zones(tool: &Tool, model: Option<&MachineModel>, table: Option<&TablePlace>, tip: Vec3) -> Vec<Zone> {
    let r = tool.radius();
    let circle = |rad: f64| Footprint::Circle { x: tip.x, y: tip.y, r: rad };
    let mut z = vec![Zone { of: ZoneOf::Cutter, fp: circle(r), z0: tip.z, z1: tip.z + tool.flute_length }];
    let mut holder = 0usize;
    for (k, e) in tool.envelopes().into_iter().enumerate() {
        let of = if k == 0 && e.bottom <= tool.flute_length + 1e-9 && tool.stickout.unwrap_or(0.0) > tool.flute_length {
            ZoneOf::Shank
        } else {
            holder += 1;
            ZoneOf::Holder(holder - 1)
        };
        z.push(Zone { of, fp: circle(e.radius), z0: tip.z + e.bottom, z1: tip.z + e.top });
    }
    let Some(model) = model else { return z };
    let nose = tip.z + nose_above_tip(tool, Some(model));
    for (k, part) in model.parts.iter().enumerate() {
        let origin = match (part.mount, table) {
            (Mount::Spindle, _) => Vec3::new(tip.x, tip.y, nose),
            (Mount::Carriage, Some(t)) => Vec3::new(tip.x, tip.y, t.corner.z),
            (Mount::Frame, Some(t)) => Vec3::new(t.corner.x + t.size[0] * 0.5, tip.y, t.corner.z),
            (Mount::Table, Some(t)) => t.corner,
            (_, None) => continue,
        };
        let zone = match &part.shape {
            PartShape::Cylinder { diameter, z: [z0, z1], center } => {
                let c = center.unwrap_or([0.0, 0.0]);
                Zone {
                    of: ZoneOf::Part(k),
                    fp: Footprint::Circle { x: origin.x + c[0], y: origin.y + c[1], r: diameter * 0.5 },
                    z0: origin.z + z0,
                    z1: origin.z + z1,
                }
            }
            PartShape::Box { min, max } => Zone {
                of: ZoneOf::Part(k),
                fp: Footprint::Rect { x0: origin.x + min[0], x1: origin.x + max[0], y0: origin.y + min[1], y1: origin.y + max[1] },
                z0: origin.z + min[2],
                z1: origin.z + max[2],
            },
        };
        z.push(zone);
    }
    z
}

/// Whether triangle `t` reaches inside zone `zone` (its outline from above, between its
/// heights): exact.
pub fn zone_meets_triangle(zone: &Zone, t: &[Vec3; 3]) -> bool {
    match zone.fp {
        Footprint::Circle { x, y, r } => crate::collision::upright_cylinder_meets_triangle(x, y, r, zone.z0, zone.z1, t),
        Footprint::Rect { x0, x1, y0, y1 } => crate::collision::box_meets_triangle(Vec3::new(x0, y0, zone.z0), Vec3::new(x1, y1, zone.z1), t),
    }
}

/// The box zone `zone` fills (its outline from above, between its heights).
pub fn zone_box(zone: &Zone) -> (Vec3, Vec3) {
    match zone.fp {
        Footprint::Circle { x, y, r } => (Vec3::new(x - r, y - r, zone.z0), Vec3::new(x + r, y + r, zone.z1)),
        Footprint::Rect { x0, x1, y0, y1 } => (Vec3::new(x0, y0, zone.z0), Vec3::new(x1, y1, zone.z1)),
    }
}

/// Work zero's X and Y in machine coordinates when the workholding does not give them (its
/// `work_zero_machine`), a stated default: the machine's X and Y travel (its soft limits) centred
/// on its table, +X to the right and +Y towards the back of the table, so the spindle is over the
/// table's centre at the middle of each travel. `table` is the table's place in work coordinates.
/// `None` without soft limits.
pub fn work_zero_machine_xy(model: &MachineModel, table: &TablePlace) -> Option<[f64; 2]> {
    let l = model.soft_limits.as_ref()?;
    let mid = |k: usize| (l.min[k] + l.max[k]) * 0.5;
    // Work zero in the table frame is the table's corner seen from work zero, turned round.
    let at = [mid(0) - table.corner.x - table.size[0] * 0.5, mid(1) - table.corner.y - table.size[1] * 0.5];
    at.iter().all(|v| v.is_finite()).then_some(at)
}

/// How low the spindle nose goes, above the table's top: the top of its travel less the Z
/// travel the soft limits give, when the model gives both.
pub fn nose_min_above_table(model: &MachineModel) -> Option<f64> {
    let top = model.spindle.nose_max_above_table?;
    let l = model.soft_limits.as_ref()?;
    Some(top - (l.max[2] - l.min[2]))
}

/// Whether `fp` overlaps the box's XY rectangle by more than `tol`.
pub fn overlaps(fp: Footprint, min: Vec3, max: Vec3, tol: f64) -> bool {
    match fp {
        Footprint::Circle { x, y, r } => {
            let dx = (min.x - x).max(0.0).max(x - max.x);
            let dy = (min.y - y).max(0.0).max(y - max.y);
            dx.hypot(dy) < r - tol
        }
        Footprint::Rect { x0, x1, y0, y1 } => x0 < max.x - tol && x1 > min.x + tol && y0 < max.y - tol && y1 > min.y + tol,
    }
}

/// A collision found along a move.
#[derive(Clone, Debug, PartialEq)]
pub struct ClearanceHit {
    pub code: String,
    pub severity: Severity,
    pub at: Vec3,
    pub message: String,
}

/// The finding stem and sentence subject of zone `of`.
fn subject(of: ZoneOf, tool: &Tool, model: Option<&MachineModel>) -> (String, String) {
    let mm = |v: f64| crate::gcode::number(v, 3);
    match of {
        ZoneOf::Cutter => ("cutter".into(), "the cutter".into()),
        ZoneOf::Shank => ("shank".into(), format!("the tool's shank (Ø{} mm)", mm(tool.shank_diameter.unwrap_or(tool.diameter)))),
        ZoneOf::Holder(k) => ("holder".into(), format!("the tool's holder (Ø{} mm)", mm(tool.holder.get(k).map_or(0.0, |h| h.diameter)))),
        ZoneOf::Part(k) => match model.and_then(|m| m.parts.get(k)) {
            Some(p) => (p.code.clone(), p.name.clone()),
            None => ("machine".into(), "a machine part".into()),
        },
    }
}

/// Every zone of a vertical tool moving through `tips` (in order, sampled every `step` mm
/// between them) against `obstacles`, and whether the spindle nose rises above its travel; the
/// cutter may go `cut_allowance` into the table. `turns` says how the work stands against the
/// bed at each tip (as many as `tips`, else none known): an obstacle that turns with A is held
/// at its angle there. Each code once, at the first place it happens.
#[allow(clippy::too_many_arguments)]
pub fn clearance(
    tool: &Tool,
    model: Option<&MachineModel>,
    table: Option<&TablePlace>,
    obstacles: &[Obstacle],
    tips: &[Vec3],
    turns: &[Option<Turn>],
    step: f64,
    tol: f64,
) -> Vec<ClearanceHit> {
    let mut hits: Vec<ClearanceHit> = Vec::new();
    let step = if step.is_finite() && step > 1e-3 { step } else { 0.5 };
    let allowance = model.map_or(0.0, |m| m.table.cut_allowance);
    // Quick reject: every zone starts at or above the tip, so a move whose lowest point clears
    // every obstacle hits none, unless a part not on the spindle hangs below that.
    let lowest = tips.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    let top = obstacles.iter().map(|o| o.max.z).fold(f64::NEG_INFINITY, f64::max);
    let hangs = model.is_some_and(|m| {
        m.parts.iter().any(|p| {
            p.mount != Mount::Spindle
                && match &p.shape {
                    PartShape::Cylinder { z, .. } => table.is_some_and(|t| t.corner.z + z[0] < top),
                    PartShape::Box { min, .. } => table.is_some_and(|t| t.corner.z + min[2] < top),
                }
        })
    });
    let travel = match (model.and_then(|m| m.spindle.nose_max_above_table), table) {
        (Some(n), Some(t)) => Some(t.corner.z + n),
        _ => None,
    };
    let reach = nose_above_tip(tool, model);
    if let Some(max) = travel
        && let Some(high) = tips.iter().copied().reduce(|a, b| if b.z > a.z { b } else { a })
        && high.z + reach > max + tol
    {
        // The program cannot run as written: an error, in the simulation as in the job's check.
        hits.push(ClearanceHit {
            code: "z-travel".into(),
            severity: Severity::Error,
            at: high,
            message: format!(
                "the spindle cannot go this high: the nose would be {} mm above the table, and it tops out at {} mm",
                crate::gcode::number(high.z + reach - max + model.and_then(|m| m.spindle.nose_max_above_table).unwrap_or(0.0), 1),
                crate::gcode::number(model.and_then(|m| m.spindle.nose_max_above_table).unwrap_or(0.0), 1)
            ),
        });
    }
    // The bottom of the travel: the tool must reach down to every move.
    if let (Some(low), Some(t)) = (model.and_then(nose_min_above_table), table)
        && let Some(deep) = tips.iter().copied().reduce(|a, b| if b.z < a.z { b } else { a })
        && deep.z + reach < t.corner.z + low - tol
    {
        hits.push(ClearanceHit {
            code: "z-travel-low".into(),
            severity: Severity::Error,
            at: deep,
            message: format!(
                "the spindle cannot go this low: the nose would be {} mm above the table, and the bottom of its travel is {} mm above it",
                crate::gcode::number(deep.z + reach - t.corner.z, 1),
                crate::gcode::number(low, 1)
            ),
        });
    }
    if obstacles.is_empty() || (lowest >= top - tol && !hangs) {
        return hits;
    }
    let mut seen: Vec<(ZoneOf, usize)> = Vec::new();
    let mut check = |p: Vec3, turn: Option<Turn>, hits: &mut Vec<ClearanceHit>| {
        for zone in zones(tool, model, table, p) {
            for (k, o) in obstacles.iter().enumerate() {
                let top = if zone.of == ZoneOf::Cutter && o.kind == ObstacleKind::Table { o.max.z - allowance } else { o.max.z };
                if zone.z0 < top - tol
                    && zone.z1 > o.min.z + tol
                    && overlaps(zone.fp, o.min, o.max, tol)
                    && !seen.contains(&(zone.of, k))
                    && o.shape.meets(zone.fp, zone.z0, zone.z1, turn.as_ref(), tol)
                {
                    seen.push((zone.of, k));
                    let (stem, what) = subject(zone.of, tool, model);
                    let code = if zone.of == ZoneOf::Cutter && o.kind == ObstacleKind::Table {
                        "cutter-into-table".to_string()
                    } else {
                        format!("{stem}-hits-{}", o.kind.code())
                    };
                    if hits.iter().any(|h| h.code == code) {
                        continue;
                    }
                    let message = if code == "cutter-into-table" {
                        format!("the cutter goes more than {} mm into the table", crate::gcode::number(allowance, 3))
                    } else if zone.of == ZoneOf::Cutter && o.kind == ObstacleKind::Parallels {
                        "the tool cuts through the stock into the parallels under it".to_string()
                    } else {
                        format!("{what} runs into {} ({})", o.kind.noun(), o.name)
                    };
                    hits.push(ClearanceHit { code, severity: Severity::Error, at: Vec3::new(p.x, p.y, zone.z0.max(o.min.z)), message });
                }
            }
        }
    };
    let turn_at = |k: usize| if turns.len() == tips.len() { turns.get(k).copied().flatten() } else { None };
    let mut prev: Option<(Vec3, Option<Turn>)> = None;
    for (k, &q) in tips.iter().enumerate() {
        let tq = turn_at(k);
        match prev {
            None => check(q, tq, &mut hits),
            Some((a, ta)) => {
                let n = ((a.dist(q) / step).ceil() as usize).clamp(1, 100_000);
                for s in 1..=n {
                    let f = s as f64 / n as f64;
                    let t = match (ta, tq) {
                        (Some(x), Some(y)) if x.center == y.center => Some(Turn { angle: x.angle + (y.angle - x.angle) * f, center: x.center }),
                        _ => None,
                    };
                    check(a.lerp(q, f), t, &mut hits);
                }
            }
        }
        prev = Some((q, tq));
    }
    hits
}
