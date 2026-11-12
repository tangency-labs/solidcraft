//! How the work is held on the table (the format `solidcraft.workholding`, version 1): a vise
//! with its jaws and parallels, strap clamps on the stock, the table itself, or a rotary unit for
//! a 4th axis. [`build`] places the job's stock in it, finds where work zero is on the table,
//! turns everything into boxes in work coordinates for the collision checks
//! ([`crate::machine_model::clearance`]), and reports what is wrong with the setup itself (the
//! stock too wide for the vise, gripped too shallow, a clamp off the stock, ...).
//!
//! Ported from the author's own MIT-licensed G-code simulator: the vise's placement (quarter
//! turns, the fixed jaw's face, the parallels along the jaws), clamps on the stock's top,
//! parallels under its long edges, the grip box less the stock, and the setup findings.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::Mesh;

use crate::findings::{FindingEvent, Severity};
use crate::machine_model::{Hole, Obstacle, ObstacleKind, Table, TablePlace, is_name};
use crate::obstacle::{MAX_CHECKED_TRIANGLES, MAX_ENTRIES, MeshGrid, MeshShape, ObstacleShape, Section};
use crate::sim::StockShape;
use crate::{Result, invalid};

/// The format id of a workholding file.
pub const FORMAT: &str = "solidcraft.workholding";
/// Most clamps.
pub const MAX_CLAMPS: usize = 100;
/// Most parts and rings of a rotary unit, and meshes of a workholding.
pub const MAX_ROTARY_PARTS: usize = 1000;
pub const MAX_RINGS: usize = 100;
pub const MAX_MESHES: usize = 64;
const MAX_LEN: f64 = 1.0e5;

/// How the work is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Holding {
    Vise,
    Clamps,
    /// The program machines the table itself.
    Table,
    /// A rotary unit (4th axis).
    Rotary,
}

/// A box (min and max corners).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Box3 {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// A precision vise: a base, a fixed jaw at one end and a moving jaw. Its own frame: `u` along
/// the jaws, `v` along the clamping axis towards the fixed jaw, `z` up from its bottom.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViseModel {
    #[serde(default)]
    pub name: String,
    /// Body length along the clamping axis.
    pub length: f64,
    /// Body and jaw width.
    pub width: f64,
    /// The jaws' top above the vise's bottom.
    pub height: f64,
    /// What the stock (or the parallels) sits on, above the vise's bottom.
    pub floor: f64,
    pub fixed_jaw: f64,
    pub moving_jaw: f64,
    pub max_opening: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vise {
    pub model: ViseModel,
    /// The centre of its footprint, table frame.
    pub center: [f64; 2],
    /// Quarter turns counter-clockwise seen from above; 0: the jaws along X, the fixed jaw at
    /// the back (+Y).
    #[serde(default)]
    pub quarter_turns: u32,
    /// The stock's centre along the jaws from the vise's centre.
    #[serde(default)]
    pub offset: f64,
    /// A box (work coordinates) the jaws close on instead of the stock: a fixture or sub-plate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grip: Option<Box3>,
}

/// A pair of parallels under the stock: in the vise one along each jaw, under clamps one under
/// each long (X) edge, centred on the stock unless moved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parallels {
    /// 0: none, the stock sits on the vise's floor or the table.
    pub height: f64,
    pub thickness: f64,
    pub length: f64,
    /// How far each bar sits in from its jaw (or the stock's edge).
    #[serde(default)]
    pub inset: f64,
    /// Along the bars, from centred on the stock.
    #[serde(default)]
    pub offset: f64,
}

/// A strap clamp, a box resting on the stock's top, table frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clamp {
    pub center: [f64; 2],
    pub size: [f64; 2],
    /// Above the stock's top, bolt included.
    pub height: f64,
}

/// The tapped holes clamps bolt into (a fixture plate's grid), table frame: hole `(i, j)` at
/// `origin + (i * pitch[0], j * pitch[1])` for `i < count[0]`, `j < count[1]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClampGrid {
    pub origin: [f64; 2],
    pub pitch: [f64; 2],
    pub count: [u32; 2],
}

/// Most holes along a clamp grid's side.
pub const MAX_GRID: u32 = 1000;
/// Least a clamp's nose rests on the stock (mm).
pub const MIN_CLAMP_GRIP: f64 = 1.0;
/// Least room between a clamp's bolt and the stock's edge (mm).
const BOLT_ROOM: f64 = 4.0;

/// Clamps placed on the stock's edges for `hold` (its `clamp_grid`, the size and height of its
/// first clamp, else 40 x 20, 20 high): each bolted in a hole of the grid outside the stock, its
/// strap across one edge with its nose on the stock's margin outside the part (`part`, work XY
/// corners; the stock `stock`), at most `MIN_CLAMP_GRIP`..half the strap onto it, and clear of
/// every rectangle in `keep_out` (work XY: where the tool reaches at the clamps' height). At
/// most two per edge (its farthest apart), on two opposite edges at the least; `None` when no
/// such placement is clear. Table frame, as the workholding's `clamps` are.
pub fn place_clamps(hold: &Workholding, stock: [[f64; 2]; 2], part: [[f64; 2]; 2], keep_out: &[[[f64; 2]; 2]]) -> Option<Vec<Clamp>> {
    let grid = hold.clamp_grid.as_ref()?;
    let at = hold.stock_at?;
    let (long, wide, height) = hold.clamps.first().map_or((40.0, 20.0, 20.0), |c| (c.size[0].max(c.size[1]), c.size[0].min(c.size[1]), c.height));
    // Work XY to table XY.
    let shift = [at[0] - stock[0][0], at[1] - stock[0][1]];
    let holes = |axis: usize| (0..grid.count[axis].min(MAX_GRID)).map(move |i| grid.origin[axis] + f64::from(i) * grid.pitch[axis] - shift[axis]);
    let clear = |lo: [f64; 2], hi: [f64; 2]| keep_out.iter().all(|k| hi[0] <= k[0][0] || lo[0] >= k[1][0] || hi[1] <= k[0][1] || lo[1] >= k[1][1]);
    // Each edge: (across axis, its outward sign) for left, right, front, back.
    let mut edges: Vec<Vec<Clamp>> = Vec::new();
    for (axis, out) in [(0usize, -1.0f64), (0, 1.0), (1, -1.0), (1, 1.0)] {
        let along = 1 - axis;
        let side = if out < 0.0 { 0 } else { 1 };
        let edge = stock[side][axis];
        let margin = (edge - part[side][axis]) * out;
        let grip = margin.min(long * 0.5);
        let mut found = Vec::new();
        if grip >= MIN_CLAMP_GRIP {
            let nose = edge - out * grip;
            // The nearest row of holes outside the edge the strap reaches over.
            let bolt = holes(axis)
                .map(|h| (h, (h - edge) * out))
                .filter(|(_, d)| *d >= BOLT_ROOM && *d + grip <= long - BOLT_ROOM)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h);
            if bolt.is_some() {
                let (a0, a1) = (nose.min(nose + out * long), nose.max(nose + out * long));
                for c in holes(along) {
                    let (w0, w1) = (c - wide * 0.5, c + wide * 0.5);
                    if w0 < stock[0][along] || w1 > stock[1][along] {
                        continue;
                    }
                    let (lo, hi) = if axis == 0 { ([a0, w0], [a1, w1]) } else { ([w0, a0], [w1, a1]) };
                    if clear(lo, hi) {
                        found.push(Clamp {
                            center: [(lo[0] + hi[0]) * 0.5 + shift[0], (lo[1] + hi[1]) * 0.5 + shift[1]],
                            size: [hi[0] - lo[0], hi[1] - lo[1]],
                            height,
                        });
                    }
                }
            }
        }
        // The two farthest apart.
        if found.len() > 2 {
            let last = found.len() - 1;
            found = vec![found[0].clone(), found[last].clone()];
        }
        edges.push(found);
    }
    let pair = |a: usize, b: usize| edges.get(a).is_some_and(|e| !e.is_empty()) && edges.get(b).is_some_and(|e| !e.is_empty());
    if !(pair(0, 1) || pair(2, 3)) {
        return None;
    }
    Some(edges.into_iter().flatten().collect())
}

/// The boxes a toolpath cuts through, one per run of cutting moves between rapids (from where
/// the run starts), as `[[x0, y0, z0], [x1, y1, z1]]` of the tip: an operation over the stock's
/// corners keeps clamps clear of each cut ([`place_clamps`]), not of one box spanning them. A run
/// overlapping the box before it in XY joins it.
pub fn cut_runs(moves: &[crate::Move5]) -> Vec<[[f64; 3]; 2]> {
    let mut runs: Vec<[[f64; 3]; 2]> = Vec::new();
    let mut open: Option<[[f64; 3]; 2]> = None;
    let mut last: Option<Vec3> = None;
    let close = |r: [[f64; 3]; 2], runs: &mut Vec<[[f64; 3]; 2]>| match runs.last_mut() {
        Some(b) if r[0][0] <= b[1][0] && r[1][0] >= b[0][0] && r[0][1] <= b[1][1] && r[1][1] >= b[0][1] => {
            for k in 0..3 {
                b[0][k] = b[0][k].min(r[0][k]);
                b[1][k] = b[1][k].max(r[1][k]);
            }
        }
        _ => runs.push(r),
    };
    let add = |r: &mut [[f64; 3]; 2], p: Vec3| {
        for (k, v) in [p.x, p.y, p.z].into_iter().enumerate() {
            r[0][k] = r[0][k].min(v);
            r[1][k] = r[1][k].max(v);
        }
    };
    for m in moves {
        match m {
            crate::Move5::Feed { to, .. } => {
                let r = open.get_or_insert_with(|| {
                    let from = last.unwrap_or(to.tip);
                    [[from.x, from.y, from.z]; 2]
                });
                add(r, to.tip);
                last = Some(to.tip);
            }
            crate::Move5::Rapid { to } => {
                if let Some(r) = open.take() {
                    close(r, &mut runs);
                }
                last = Some(to.tip);
            }
            crate::Move5::Dwell { .. } => {}
        }
    }
    if let Some(r) = open.take() {
        close(r, &mut runs);
    }
    runs.retain(|r| r.iter().flatten().all(|v| v.is_finite()));
    runs
}

/// Which end of the bar the chuck holds, and so which way the chuck's body reaches from its
/// face along X.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChuckSide {
    /// The unit at the table's +X end: the chuck holds the bar's +X end and reaches back from its
    /// face towards +X; the bar stands out towards -X (+X points into the axis).
    #[default]
    PlusX,
    /// The unit at the table's -X end: the chuck holds the bar's -X end and reaches back towards -X.
    MinusX,
}

impl ChuckSide {
    /// +1 when the chuck's body is on the +X side of its face, -1 on the -X side.
    pub fn sign(self) -> f64 {
        match self {
            ChuckSide::PlusX => 1.0,
            ChuckSide::MinusX => -1.0,
        }
    }
}

/// Where a clearing carried out past a face's open sides must stop along X so the cutter and
/// holder stay clear of a chuck's jaws: `rect` (a stock's rectangle seen down `frame`, in its
/// own coordinates) cut back to the side of work X `limit` away from the chuck (`side`). A
/// frame with no axis along work X (one looking along it) is left as it is. None: nothing of the
/// rectangle is left on the free side of `limit`.
pub fn short_of_jaws(rect: [solidcore_record::Vec2; 2], frame: &crate::Frame, side: ChuckSide, limit: f64) -> Option<[solidcore_record::Vec2; 2]> {
    if !limit.is_finite() {
        return None;
    }
    let [mut lo, mut hi] = rect;
    let along = |v: Vec3| (v.x.abs() > 1.0 - 1e-9).then_some(v.x.signum());
    // The local axis along work X, and its direction (+1 or -1) there.
    let (on_x, dir) = match (along(frame.x), along(frame.y)) {
        (Some(d), _) => (true, d),
        (None, Some(d)) => (false, d),
        (None, None) => return Some(rect),
    };
    // Local coordinate u maps to work X as origin.x + u * dir.
    let u = (limit - frame.origin.x) * dir;
    let upper = dir * side.sign() > 0.0;
    let (l, h) = if on_x { (&mut lo.x, &mut hi.x) } else { (&mut lo.y, &mut hi.y) };
    if upper {
        *h = h.min(u);
    } else {
        *l = l.max(u);
    }
    (*l < *h).then_some([lo, hi])
}

/// A chuck's jaws: from the bar's surface out by `radial`, standing `axial` out of the chuck's
/// face towards the work. Without a `width` they are the ring they sweep as A turns; with one,
/// `count` blocks that width across, spaced evenly round the bar and turning with A, so the gaps
/// between them are free.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jaws {
    /// How many; default 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// From the bar's surface to the jaws' outside.
    pub radial: f64,
    /// How far they stand out of the chuck's face: the length of bar they grip.
    pub axial: f64,
    /// Each jaw's width across (mm): the jaws are separate blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Where the first jaw points at A = 0: degrees about +X from straight up (+Z); default 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f64>,
}

/// A cylinder along X, table frame: about the A axis unless `center` (Y, Z) is given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XCylinder {
    pub diameter: f64,
    /// Its two ends along X.
    pub x: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 2]>,
}

/// A part of a rotary unit, named in the findings: a box or a cylinder along X, table frame.
/// Fixed to the bed unless `turns` (fixed to the work: it turns with A about the A axis).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotaryPart {
    /// Reads in a sentence: "the cover's top".
    pub name: String,
    #[serde(default, rename = "box", skip_serializing_if = "Option::is_none")]
    pub box3: Option<Box3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cylinder: Option<XCylinder>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub turns: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A ring that turns with A about the A axis (a home flag, a back plate): a disk `diameter`
/// across from `x[0]` to `x[1]`, table frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ring {
    #[serde(default)]
    pub name: String,
    pub diameter: f64,
    pub x: [f64; 2],
}

/// What a mesh is placed in: the table frame, or turning with A (fixed to the work).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshFrame {
    #[default]
    Table,
    Rotary,
}

/// A mesh file (STL, or STEP through the import) of a workholding: drawn, and checked unless
/// `check` is false. In the table frame (`frame: table`) or turning with A (`frame: rotary`,
/// drawn only: give what turns as `rotary.rings`, `parts` and `chuck.jaws` for the check).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldMeshSpec {
    /// The file, beside the workholding file when relative.
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub frame: MeshFrame,
    /// Whether the tool and the machine are checked against it; default: true in the table
    /// frame, false turning with A (where true is refused).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<bool>,
    /// Its colour, three numbers from 0 to 1; default the theme's fixture colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f64; 3]>,
    /// Rows of a 3 x 4 matrix (rotation and shift, mm) from the file's coordinates to the frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[[f64; 4]; 3]>,
}

impl HoldMeshSpec {
    /// Whether it is checked.
    pub fn checked(&self) -> bool {
        self.check.unwrap_or(self.frame == MeshFrame::Table)
    }
}

/// A mesh read for a workholding (or a machine model's checked table-mounted mesh), in the table
/// frame (or, with `frame: rotary`, in the table frame at A = 0).
#[derive(Clone, Debug)]
pub struct LoadedMesh {
    pub name: String,
    pub frame: MeshFrame,
    pub check: bool,
    pub color: Option<[f64; 3]>,
    /// What a hit is called (`holder-hits-rotary`).
    pub kind: ObstacleKind,
    pub mesh: Mesh,
}

/// A mesh drawn with the workholding, in work coordinates with the rotaries at zero: turning
/// with A (fixed to the work) or fixed to the bed.
#[derive(Clone, Debug)]
pub struct DrawnMesh {
    pub name: String,
    pub turns: bool,
    pub color: Option<[f64; 3]>,
    pub mesh: Arc<Mesh>,
}

impl PartialEq for DrawnMesh {
    fn eq(&self, other: &DrawnMesh) -> bool {
        self.name == other.name && self.turns == other.turns && self.color == other.color && Arc::ptr_eq(&self.mesh, &other.mesh)
    }
}

/// A rotary unit's chuck: a cylinder about the A axis ending at its face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chuck {
    pub diameter: f64,
    pub length: f64,
    /// Where its face is along X, table frame; the chuck reaches back from it, away from the work
    /// (towards `side`).
    pub face_x: f64,
    /// Which end of the bar it holds. Default: `plus_x`.
    #[serde(default)]
    pub side: ChuckSide,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jaws: Option<Jaws>,
}

/// A rotary unit (4th axis about X).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotaryUnit {
    /// The A axis line: its Y and Z in the table frame.
    pub axis: [f64; 2],
    /// The headstock as one box (or give the unit's `parts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headstock: Option<Box3>,
    pub chuck: Chuck,
    /// The unit's parts, each named in the findings (a cover with a cut corner is two boxes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<RotaryPart>,
    /// Rings that turn with A beyond the chuck (a home flag).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rings: Vec<Ring>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailstock: Option<Box3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plate: Option<Box3>,
    /// Work zero along X, table frame; left out, the bar's held end (its +X end with the chuck on
    /// the +X side) sits at the chuck's face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zero_x: Option<f64>,
}

/// A workholding file (`solidcraft.workholding`, version 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workholding {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub name: String,
    pub holding: Holding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vise: Option<Vise>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallels: Option<Parallels>,
    #[serde(default)]
    pub clamps: Vec<Clamp>,
    /// With clamps: the tapped holes clamps bolt into (a fixture plate's grid), for placing
    /// them on the stock's edges ([`place_clamps`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clamp_grid: Option<ClampGrid>,
    /// With clamps: the stock's front-left corner on the table (X, Y).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_at: Option<[f64; 2]>,
    /// Machining the table: work zero in the table frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_zero: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotary: Option<RotaryUnit>,
    /// Machine coordinates of work zero, for the machine model's soft limits: every move of the
    /// program is checked against them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_zero_machine: Option<[f64; 3]>,
    /// Mesh files (STL, STEP) drawn with the workholding and checked against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<HoldMeshSpec>,
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= MAX_LEN)
}

fn box_ok(b: &Box3) -> bool {
    finite(&b.min) && finite(&b.max) && b.min.iter().zip(&b.max).all(|(a, c)| a < c)
}

fn cylinder_ok(c: &XCylinder) -> bool {
    finite(&[c.diameter, c.x[0], c.x[1]]) && c.diameter > 0.0 && c.x[0] < c.x[1] && c.center.is_none_or(|p| finite(&p))
}

impl Workholding {
    /// Workholding read from JSON text and checked; unknown keys are refused (a workholding
    /// written into a job).
    pub fn from_json(text: &str) -> Result<Workholding> {
        let w: Workholding = serde_json::from_str(text).map_err(|e| crate::CamError::Invalid(format!("the workholding: {e}")))?;
        w.validate()?;
        Ok(w)
    }

    /// A workholding file read from JSON text and checked, its unknown keys (a newer build's)
    /// left out and named.
    pub fn from_json_lenient(text: &str) -> Result<(Workholding, Vec<String>)> {
        let (w, ignored): (Workholding, Vec<String>) =
            crate::keys::from_json_lenient(text).map_err(|e| crate::CamError::Invalid(format!("the workholding: {e}")))?;
        w.validate()?;
        Ok((w, ignored))
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT || self.version != 1 {
            return invalid(format!("a workholding file has `format: \"{FORMAT}\"` and `version: 1`"));
        }
        if !is_name(&self.name) {
            return invalid("the workholding's `name` must be at most 200 characters, without control characters");
        }
        if let Some(v) = &self.vise {
            let m = &v.model;
            let sizes = [m.length, m.width, m.height, m.floor, m.fixed_jaw, m.moving_jaw, m.max_opening];
            if !(finite(&sizes) && sizes.iter().all(|s| *s >= 0.0) && m.length > 0.0 && m.width > 0.0 && m.height > 0.0 && m.floor < m.height)
                || !is_name(&m.name)
            {
                return invalid("the vise's sizes must be finite and positive, its floor below its jaws' top");
            }
            if m.fixed_jaw + m.moving_jaw >= m.length {
                return invalid("the vise's jaws must be thinner together than its length");
            }
            if !finite(&[v.center[0], v.center[1], v.offset]) || v.grip.as_ref().is_some_and(|g| !box_ok(g)) {
                return invalid("the vise's `center`, `offset` and `grip` must be finite (a grip box min below max)");
            }
        }
        if let Some(p) = &self.parallels {
            let s = [p.height, p.thickness, p.length, p.inset, p.offset];
            if !(finite(&s) && p.height >= 0.0 && p.thickness >= 0.0 && p.length >= 0.0 && p.inset >= 0.0) {
                return invalid("the parallels' sizes must be finite, 0 or more");
            }
        }
        if self.clamps.len() > MAX_CLAMPS {
            return invalid(format!("at most {MAX_CLAMPS} clamps"));
        }
        for c in &self.clamps {
            if !(finite(&[c.center[0], c.center[1], c.size[0], c.size[1], c.height]) && c.size[0] > 0.0 && c.size[1] > 0.0 && c.height > 0.0) {
                return invalid("a clamp needs a finite centre and a size and height above 0");
            }
        }
        if self.clamp_grid.as_ref().is_some_and(|g| {
            !(finite(&[g.origin[0], g.origin[1], g.pitch[0], g.pitch[1]])
                && g.pitch[0] > 0.0
                && g.pitch[1] > 0.0
                && g.count.iter().all(|n| (1..=MAX_GRID).contains(n)))
        }) {
            return invalid(format!("the clamp grid needs a finite `origin`, a `pitch` above 0 and a `count` of 1 to {MAX_GRID} holes each way"));
        }
        if self.stock_at.is_some_and(|s| !finite(&s))
            || self.table_zero.is_some_and(|z| !finite(&z))
            || self.work_zero_machine.is_some_and(|z| !finite(&z))
        {
            return invalid("`stock_at`, `table_zero` and `work_zero_machine` must be finite");
        }
        if self.meshes.len() > MAX_MESHES {
            return invalid(format!("a workholding has at most {MAX_MESHES} meshes"));
        }
        for m in &self.meshes {
            if m.path.trim().is_empty() || m.path.len() > 4096 || !is_name(&m.name) {
                return invalid("a workholding mesh needs a `path` (at most 4096 characters) and a short `name`");
            }
            if m.color.is_some_and(|c| !c.iter().all(|v| (0.0..=1.0).contains(v))) {
                return invalid(format!("mesh {}: its `color` is three numbers from 0 to 1", m.path));
            }
            if m.transform.is_some_and(|t| !t.iter().all(|row| finite(row))) {
                return invalid(format!("mesh {}: its `transform` is three rows of four finite numbers", m.path));
            }
            if m.frame == MeshFrame::Rotary && m.check == Some(true) {
                return invalid(format!(
                    "mesh {}: a mesh that turns with A is drawn only; give what turns as the rotary unit's `rings`, `parts` (`turns: true`) and `chuck.jaws` for the check",
                    m.path
                ));
            }
        }
        if let Some(r) = &self.rotary {
            if r.parts.len() > MAX_ROTARY_PARTS || r.rings.len() > MAX_RINGS {
                return invalid(format!("a rotary unit has at most {MAX_ROTARY_PARTS} parts and {MAX_RINGS} rings"));
            }
            for p in &r.parts {
                if !is_name(&p.name) || p.name.trim().is_empty() {
                    return invalid("every part of the rotary unit needs a `name` of at most 200 characters, without control characters");
                }
                let ok = match (&p.box3, &p.cylinder) {
                    (Some(b), None) => box_ok(b),
                    (None, Some(c)) => cylinder_ok(c),
                    _ => false,
                };
                if !ok {
                    return invalid(format!(
                        "rotary part `{}`: give one `box` (min below max) or one `cylinder` (a diameter above 0, x [low, high]), all finite",
                        p.name
                    ));
                }
            }
            for g in &r.rings {
                if !is_name(&g.name) || !cylinder_ok(&XCylinder { diameter: g.diameter, x: g.x, center: None }) {
                    return invalid("a ring needs a `diameter` above 0 and `x` [low, high], finite, and a short name");
                }
            }
            let boxes_ok = r.headstock.as_ref().is_none_or(box_ok) && r.tailstock.as_ref().is_none_or(box_ok) && r.plate.as_ref().is_none_or(box_ok);
            let c = &r.chuck;
            if !(boxes_ok && finite(&[r.axis[0], r.axis[1], c.diameter, c.length, c.face_x]) && c.diameter > 0.0 && c.length > 0.0)
                || r.zero_x.is_some_and(|x| !finite(&[x]))
            {
                return invalid("the rotary unit's boxes must have min below max, its chuck a diameter and a length above 0, all finite");
            }
            if c.jaws
                .as_ref()
                .is_some_and(|j| !(finite(&[j.radial, j.axial]) && j.radial > 0.0 && j.axial > 0.0 && j.count.is_none_or(|n| (1..=12).contains(&n))))
            {
                return invalid("the chuck's jaws need a `radial` and an `axial` size above 0, and 1 to 12 of them");
            }
            if c.jaws.as_ref().is_some_and(|j| {
                j.width.is_some_and(|w| !(finite(&[w]) && w > 0.0)) || j.angle_deg.is_some_and(|a| !(a.is_finite() && a.abs() <= 3600.0))
            }) {
                return invalid("the chuck's jaws' `width` must be above 0 and their `angle_deg` a finite angle, all finite");
            }
        }
        match self.holding {
            Holding::Vise if self.vise.is_none() => invalid("`holding: vise` needs the `vise`"),
            Holding::Clamps if self.stock_at.is_none() => invalid("`holding: clamps` needs `stock_at`, the stock's front-left corner on the table"),
            Holding::Table if self.table_zero.is_none() => invalid("`holding: table` needs `table_zero`, work zero on the table"),
            Holding::Rotary if self.rotary.is_none() => invalid("`holding: rotary` needs the `rotary` unit"),
            _ => Ok(()),
        }
    }
}

/// The workholding placed with the stock: everything in work coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct HoldScene {
    pub holding: Holding,
    /// Work zero in the table frame.
    pub work_zero_table: Vec3,
    /// The table: its top-front-left corner in work coordinates, its size.
    pub table: TablePlace,
    pub corner_radius: f64,
    /// The table's holes, centres in work coordinates (`x`, `y`).
    pub holes: Vec<Hole>,
    pub obstacles: Vec<Obstacle>,
    /// In the vise: how far the stock's top stands above the jaws.
    pub stock_above_jaws: Option<f64>,
    /// In the vise, the stock held in the jaws (no fixture's grip box): how much of its height
    /// the jaws grip (mm), weighed against the job's cuts by [`grip_for_cut`].
    pub stock_grip: Option<f64>,
    /// In a rotary unit: how the bar sits in the chuck, for the operator.
    pub bar_grip: Option<BarGrip>,
    /// What is wrong with the setup itself.
    pub findings: Vec<FindingEvent>,
    /// The workholding's meshes, drawn (the checked ones are in `obstacles` too).
    pub meshes: Vec<DrawnMesh>,
    /// Work zero in machine coordinates, when the workholding says: the machine's soft limits
    /// are checked with it.
    pub work_zero_machine: Option<Vec3>,
}

/// How a bar sits in a rotary unit's chuck, along work X (mm): what the operator sets up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarGrip {
    /// The chuck's face.
    pub face_x: f64,
    /// The bar's end towards the chuck, and its free end.
    pub held_end_x: f64,
    pub free_end_x: f64,
    /// How much of the bar the jaws grip (0 without jaws, or when it does not reach them).
    pub in_jaws: f64,
    /// How far the bar stands out of the jaws (of the face, without jaws) to its free end.
    pub stands_out: f64,
    /// How far work zero (X0) is out from the chuck's face, towards the free end.
    pub zero_out: f64,
}

/// Local vise `(u, v)` to a table offset, after `k` quarter turns counter-clockwise.
fn turn(k: u32, u: f64, v: f64) -> (f64, f64) {
    match k % 4 {
        0 => (u, v),
        1 => (-v, u),
        2 => (-u, -v),
        _ => (v, -u),
    }
}

/// The table-frame box of a rectangle in the vise's frame.
fn vise_rect(c: [f64; 2], k: u32, (u0, u1): (f64, f64), (v0, v1): (f64, f64), (z0, z1): (f64, f64)) -> (Vec3, Vec3) {
    let (ax, ay) = turn(k, u0, v0);
    let (bx, by) = turn(k, u1, v1);
    (Vec3::new(c[0] + ax.min(bx), c[1] + ay.min(by), z0), Vec3::new(c[0] + ax.max(bx), c[1] + ay.max(by), z1))
}

/// `a` with `b` cut out, as up to six boxes.
fn box_minus(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> Vec<(Vec3, Vec3)> {
    let get = |v: Vec3, i: usize| match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    };
    let set = |mut v: Vec3, i: usize, x: f64| {
        match i {
            0 => v.x = x,
            1 => v.y = x,
            _ => v.z = x,
        }
        v
    };
    let (mut lo, mut hi) = a;
    if (0..3).any(|i| get(b.1, i) <= get(lo, i) || get(b.0, i) >= get(hi, i)) {
        return vec![a];
    }
    let mut out = Vec::new();
    for ax in 0..3 {
        if get(b.0, ax) > get(lo, ax) {
            out.push((lo, set(hi, ax, get(b.0, ax))));
            lo = set(lo, ax, get(b.0, ax));
        }
        if get(b.1, ax) < get(hi, ax) {
            out.push((set(lo, ax, get(b.1, ax)), hi));
            hi = set(hi, ax, get(b.1, ax));
        }
    }
    out
}

/// Each bar's inset and thickness across a stock `across` wide: both fit within it.
fn bar_across(par: &Parallels, across: f64) -> (f64, f64) {
    let half = across / 2.0;
    let inset = par.inset.clamp(0.0, half.max(0.0));
    (inset, par.thickness.max(0.0).min((half - inset).max(0.0)))
}

/// The share of a side cut's reach above the jaws the jaws should grip, in aluminium, brass,
/// plastics and wood ([`grip_for_cut`]).
pub const GRIP_SHARE: f64 = 1.0 / 3.0;
/// [`GRIP_SHARE`] in steel, stainless and anything else hard to cut, whose cutting forces are
/// several times aluminium's.
pub const GRIP_SHARE_HARD: f64 = 0.5;

/// The deepest side cut of a job, for [`grip_for_cut`]: the operation's name and how far below
/// the stock's top its tool cuts (mm).
#[derive(Clone, Debug, PartialEq)]
pub struct SideCut {
    pub operation: String,
    pub depth: f64,
}

/// Whether the vise grips enough of the stock for the job's deepest side cut (a rule of thumb):
/// a side cut pushes the stock over about the jaws' top with a force that grows with how deep
/// it cuts, so the cut's reach above the jaws (its depth, at most the stock's height above
/// them, `above`) asks the jaws to grip at least [`GRIP_SHARE`] of it ([`GRIP_SHARE_HARD`] in a
/// `hard` material). A facing that steps down is a side cut at its stock's edge, and counts
/// so. `grip` is how much of the stock the jaws grip ([`HoldScene::stock_grip`]); a warning,
/// with what to change, when that is short; `None` when it is enough or nothing is known.
pub fn grip_for_cut(grip: f64, above: f64, cut: &SideCut, hard: bool) -> Option<FindingEvent> {
    if !(grip > 0.0 && above > 0.0 && cut.depth > 0.0) || !(grip.is_finite() && above.is_finite() && cut.depth.is_finite()) {
        return None;
    }
    let reach = cut.depth.min(above);
    let share = if hard { GRIP_SHARE_HARD } else { GRIP_SHARE };
    let need = reach * share;
    if grip + 1e-6 >= need {
        return None;
    }
    let rule = if hard { "half of it in steel and other hard materials" } else { "a third of it (half in steel)" };
    Some(setup(
        "grip-short-for-cut",
        Severity::Warning,
        format!(
            "the jaws grip only {} mm of the stock, which stands {} mm above them, and operation `{}` cuts {} mm down its side \
             ({} mm of it above the jaws): grip at least {} mm, {rule}, or the cut can pull the stock out of the jaws. Grip deeper \
             (step jaws, or the reversed jaws with the stock on parallels), set the stock lower in the vise (lower parallels, a \
             shorter stock), or cut the side lighter (less depth per pass, a smaller stepover)",
            num(grip),
            num(above),
            cut.operation,
            num(cut.depth),
            num(reach),
            num(need)
        ),
    ))
}

fn setup(code: &str, severity: Severity, message: String) -> FindingEvent {
    FindingEvent { code: code.into(), severity, at_move: None, line: 0, op: None, at: None, depth: 0.0, message }
}

/// `shape` (table frame) moved by `-zero`.
fn shifted(shape: ObstacleShape, zero: Vec3) -> ObstacleShape {
    let yz = |p: [f64; 2]| [p[0] - zero.y, p[1] - zero.z];
    match shape {
        ObstacleShape::AlongX { x, section, turns, axis } => ObstacleShape::AlongX {
            x: [x[0] - zero.x, x[1] - zero.x],
            section: match section {
                Section::Disk { c, r } => Section::Disk { c: yz(c), r },
                Section::Ring { c, r0, r1 } => Section::Ring { c: yz(c), r0, r1 },
                Section::Polygon(p) => Section::Polygon(p.into_iter().map(yz).collect()),
            },
            turns,
            axis: yz(axis),
        },
        other => other,
    }
}

/// The box (table frame) of a prism along X from `x` of `section`, wherever it is turned about
/// `axis` when it `turns`.
fn prism_box(x: (f64, f64), section: &Section, turns: bool, axis: [f64; 2]) -> (Vec3, Vec3) {
    let (lo, hi) = if turns { section.swept(axis).bounds() } else { section.bounds() };
    (Vec3::new(x.0, lo[0], lo[1]), Vec3::new(x.1, hi[0], hi[1]))
}

/// Places `stock` (work coordinates) in `hold` on `table`; see the module documentation.
pub fn build(table: &Table, hold: &Workholding, stock: &StockShape) -> Result<HoldScene> {
    build_with(table, hold, stock, &[])
}

/// [`build`], with the workholding's meshes (and the machine model's checked table-mounted ones)
/// read: drawn, and checked against where `check` says so.
pub fn build_with(table: &Table, hold: &Workholding, stock: &StockShape, meshes: &[LoadedMesh]) -> Result<HoldScene> {
    hold.validate()?;
    let (smin, smax) = match *stock {
        StockShape::Box { min, max } => (min, max),
        StockShape::Cylinder { x_min, x_max, radius } => (Vec3::new(x_min, -radius, -radius), Vec3::new(x_max, radius, radius)),
    };
    let size = smax - smin;
    let mut f: Vec<FindingEvent> = Vec::new();
    let mut obs: Vec<Obstacle> = Vec::new();
    let mut push = |name: &str, kind: ObstacleKind, (min, max): (Vec3, Vec3)| obs.push(Obstacle::boxed(name, kind, min, max));
    // Prisms along X (table frame), pushed after the match.
    let mut prisms: Vec<Obstacle> = Vec::new();
    let table_box = (Vec3::new(0.0, 0.0, -table.size[2]), Vec3::new(table.size[0], table.size[1], 0.0));
    let par = hold.parallels.clone().unwrap_or(Parallels { height: 0.0, thickness: 0.0, length: 0.0, inset: 0.0, offset: 0.0 });
    let mut stock_above_jaws = None;
    let mut stock_grip = None;
    let mut bar_grip = None;
    let on_table = |lo: Vec3, hi: Vec3| lo.x >= -1e-6 && lo.y >= -1e-6 && hi.x <= table.size[0] + 1e-6 && hi.y <= table.size[1] + 1e-6;
    let zero = match hold.holding {
        Holding::Table => {
            let z = hold.table_zero.unwrap_or([0.0; 3]);
            Vec3::new(z[0], z[1], z[2])
        }
        Holding::Vise => {
            let Some(v) = &hold.vise else { return invalid("`holding: vise` needs the `vise`") };
            let m = &v.model;
            let k = v.quarter_turns % 4;
            // The jaws close on the grip box when there is one, else on the stock.
            let held = v.grip.map_or((smin, smax), |g| (Vec3::new(g.min[0], g.min[1], g.min[2]), Vec3::new(g.max[0], g.max[1], g.max[2])));
            let hsize = held.1 - held.0;
            let what = if v.grip.is_some() { "fixture" } else { "stock" };
            let (su, sv) = if k % 2 == 0 { (hsize.x, hsize.y) } else { (hsize.y, hsize.x) };
            let (hl, hw) = (m.length / 2.0, m.width / 2.0);
            let face = hl - m.fixed_jaw;
            let bottom = m.floor + par.height.max(0.0);
            let top = bottom + hsize.z;
            stock_above_jaws = Some(top - m.height);
            let (u0, u1) = (v.offset - su / 2.0, v.offset + su / 2.0);
            let (sv0, sv1) = (face - sv, face);
            push("the table", ObstacleKind::Table, table_box);
            push("the vise's body", ObstacleKind::Vise, vise_rect(v.center, k, (-hw, hw), (-hl, hl), (0.0, m.floor)));
            push("the vise's fixed jaw", ObstacleKind::Vise, vise_rect(v.center, k, (-hw, hw), (face, hl), (0.0, m.height)));
            push("the vise's moving jaw", ObstacleKind::Vise, vise_rect(v.center, k, (-hw, hw), (sv0 - m.moving_jaw, sv0), (0.0, m.height)));
            if par.height > 1e-6 {
                let c = v.offset + par.offset;
                let (p0, p1) = (c - par.length / 2.0, c + par.length / 2.0);
                let (inset, t) = bar_across(&par, sv);
                if t > 1e-6 {
                    for (a, b) in [(sv1 - inset - t, sv1 - inset), (sv0 + inset, sv0 + inset + t)] {
                        push("the parallels", ObstacleKind::Parallels, vise_rect(v.center, k, (p0, p1), (a, b), (m.floor, bottom)));
                    }
                }
            }
            if sv > m.max_opening + 1e-6 {
                f.push(setup(
                    "vise-too-narrow",
                    Severity::Error,
                    format!("the {what} is {} mm across the jaws; this vise opens to {} mm", num(sv), num(m.max_opening)),
                ));
            }
            let grip = m.height - bottom;
            if v.grip.is_none() && grip > 0.0 {
                stock_grip = Some(grip.min(hsize.z));
            }
            if grip <= 0.0 {
                f.push(setup("stock-not-gripped", Severity::Error, format!("the {what} sits entirely above the jaws: nothing holds it")));
            } else if grip < 3.0 {
                f.push(setup(
                    "shallow-grip",
                    Severity::Warning,
                    format!("the jaws grip only {} mm of the {what}: aim for 3 mm or more (lower parallels)", num(grip)),
                ));
            }
            let (lo, hi) = vise_rect(v.center, k, (-hw, hw), (-hl, hl), (0.0, 0.0));
            if !on_table(lo, hi) {
                f.push(setup("vise-overhangs-table", Severity::Info, "the vise overhangs the table".into()));
            }
            let (held_min, _) = vise_rect(v.center, k, (u0, u1), (sv0, sv1), (bottom, top));
            held_min - held.0
        }
        Holding::Clamps => {
            let at = hold.stock_at.unwrap_or([0.0, 0.0]);
            let h = par.height.max(0.0);
            let lo = Vec3::new(at[0], at[1], h);
            let hi = lo + size;
            push("the table", ObstacleKind::Table, table_box);
            if h > 1e-6 {
                let cx = (lo.x + hi.x) / 2.0 + par.offset;
                let (x0, x1) = (cx - par.length / 2.0, cx + par.length / 2.0);
                let (inset, t) = bar_across(&par, size.y);
                if t > 1e-6 {
                    for (a, b) in [(lo.y + inset, lo.y + inset + t), (hi.y - inset - t, hi.y - inset)] {
                        push("the parallels", ObstacleKind::Parallels, (Vec3::new(x0, a, 0.0), Vec3::new(x1, b, h)));
                    }
                }
            }
            for (n, c) in hold.clamps.iter().enumerate() {
                let cl = Vec3::new(c.center[0] - c.size[0] / 2.0, c.center[1] - c.size[1] / 2.0, hi.z);
                let ch = Vec3::new(c.center[0] + c.size[0] / 2.0, c.center[1] + c.size[1] / 2.0, hi.z + c.height);
                push(&format!("clamp {}", n + 1), ObstacleKind::Clamp, (cl, ch));
                if ch.x <= lo.x || cl.x >= hi.x || ch.y <= lo.y || cl.y >= hi.y {
                    f.push(setup("clamp-off-stock", Severity::Info, format!("clamp {} does not reach the stock", n + 1)));
                }
            }
            if hold.clamps.is_empty() {
                f.push(setup("no-clamps", Severity::Info, "no clamps are given: the stock is taken as held some other way".into()));
            }
            if !on_table(lo, hi) {
                f.push(setup("stock-off-table", Severity::Warning, "the stock hangs over the edge of the table".into()));
            }
            lo - smin
        }
        Holding::Rotary => {
            let Some(r) = &hold.rotary else { return invalid("`holding: rotary` needs the `rotary` unit") };
            if !(smin.y < 0.0 && smax.y > 0.0 && smin.z < 0.0 && smax.z > 0.0) {
                return invalid("a rotary unit holds a bar the A axis runs through (a cylinder about X, or a box round it)");
            }
            let to = |b: &Box3| (Vec3::new(b.min[0], b.min[1], b.min[2]), Vec3::new(b.max[0], b.max[1], b.max[2]));
            let axis = r.axis;
            let mut prism = |name: &str, x: (f64, f64), section: Section, turns: bool| {
                let (min, max) = prism_box(x, &section, turns, axis);
                prisms.push(Obstacle {
                    name: name.into(),
                    kind: ObstacleKind::Rotary,
                    min,
                    max,
                    shape: ObstacleShape::AlongX { x: [x.0, x.1], section, turns, axis },
                });
            };
            push("the table", ObstacleKind::Table, table_box);
            if let Some(h) = &r.headstock {
                push("the rotary unit's headstock", ObstacleKind::Rotary, to(h));
            }
            let c = &r.chuck;
            let sign = c.side.sign();
            let span = |a: f64, b: f64| (a.min(b), a.max(b));
            let ring =
                |(x0, x1): (f64, f64), rad: f64| (Vec3::new(x0, r.axis[0] - rad, r.axis[1] - rad), Vec3::new(x1, r.axis[0] + rad, r.axis[1] + rad));
            prism("the rotary unit's chuck", span(c.face_x, c.face_x + sign * c.length), Section::Disk { c: axis, r: c.diameter * 0.5 }, true);
            if let Some(t) = &r.tailstock {
                push("the rotary unit's tailstock", ObstacleKind::Rotary, to(t));
            }
            if let Some(p) = &r.plate {
                push("the rotary unit's plate", ObstacleKind::Rotary, to(p));
            }
            for part in &r.parts {
                let name = part.name.trim();
                match (&part.box3, &part.cylinder) {
                    (Some(b), _) if !part.turns => push(name, ObstacleKind::Rotary, to(b)),
                    (Some(b), _) => {
                        let corners = vec![[b.min[1], b.min[2]], [b.max[1], b.min[2]], [b.max[1], b.max[2]], [b.min[1], b.max[2]]];
                        prism(name, (b.min[0], b.max[0]), Section::Polygon(corners), true);
                    }
                    (None, Some(cy)) => {
                        prism(name, (cy.x[0], cy.x[1]), Section::Disk { c: cy.center.unwrap_or(axis), r: cy.diameter * 0.5 }, part.turns);
                    }
                    (None, None) => {}
                }
            }
            for g in &r.rings {
                let name = if g.name.trim().is_empty() { "a ring of the rotary unit" } else { g.name.trim() };
                prism(name, (g.x[0], g.x[1]), Section::Disk { c: axis, r: g.diameter * 0.5 }, true);
            }
            // The bar's held end at the chuck's face unless work zero is given.
            let held_end = if c.side == ChuckSide::PlusX { smax.x } else { smin.x };
            let zero_x = r.zero_x.unwrap_or(c.face_x - held_end);
            let zero = Vec3::new(zero_x, r.axis[0], r.axis[1]);
            // Along work X: the face, the bar's ends, and the stretch the jaws reach over.
            let face = c.face_x - zero_x;
            let free_end = if c.side == ChuckSide::PlusX { smin.x } else { smax.x };
            let reach = c.jaws.as_ref().map_or(0.0, |j| j.axial);
            let jaws_end = face - sign * reach;
            let in_jaws = (smax.x.min(face.max(jaws_end)) - smin.x.max(face.min(jaws_end))).max(0.0);
            // Out of the jaws: from where they (or the bar's end, short of them) stop.
            let out_from = if sign > 0.0 { jaws_end.min(held_end) } else { jaws_end.max(held_end) };
            bar_grip = Some(BarGrip {
                face_x: face,
                held_end_x: held_end,
                free_end_x: free_end,
                in_jaws,
                stands_out: (sign * (out_from - free_end)).max(0.0),
                zero_out: sign * face,
            });
            if let Some(j) = &c.jaws {
                let zone = span(c.face_x, c.face_x - sign * j.axial);
                let round = matches!(stock, StockShape::Cylinder { .. });
                match j.width {
                    Some(w) => {
                        // Separate blocks, turning with A: from the bar's surface out, `w` across.
                        let n = j.count.unwrap_or(3).max(1);
                        let first = j.angle_deg.unwrap_or(0.0).to_radians();
                        for k in 0..n {
                            let phi = first + f64::from(k) * std::f64::consts::TAU / f64::from(n);
                            let (d, e) = ([-phi.sin(), phi.cos()], [phi.cos(), phi.sin()]);
                            // Where the bar's surface is that way from the axis.
                            let inner = if round {
                                smax.y
                            } else {
                                [(smin.y, smin.z), (smin.y, smax.z), (smax.y, smin.z), (smax.y, smax.z)]
                                    .iter()
                                    .map(|(y, z)| y * d[0] + z * d[1])
                                    .fold(0.0, f64::max)
                            };
                            let outer = inner + j.radial;
                            let at = |r: f64, s: f64| [axis[0] + d[0] * r + e[0] * s, axis[1] + d[1] * r + e[1] * s];
                            let block = vec![at(inner, -w / 2.0), at(inner, w / 2.0), at(outer, w / 2.0), at(outer, -w / 2.0)];
                            prism("the rotary unit's jaws", zone, Section::Polygon(block), true);
                        }
                    }
                    None if round => {
                        // The ring the jaws sweep, from the bar's surface out.
                        prism("the rotary unit's jaws", zone, Section::Ring { c: axis, r0: smax.y, r1: smax.y + j.radial }, true);
                    }
                    None => {
                        // The ring the jaws sweep, less the bar: where a tool must not go.
                        let rad = smin.y.abs().max(smax.y).hypot(smin.z.abs().max(smax.z));
                        for b in box_minus(ring(zone, rad + j.radial), (smin + zero, smax + zero)) {
                            push("the rotary unit's jaws", ObstacleKind::Rotary, b);
                        }
                    }
                }
                let grip = (smax.x + zero_x).min(zone.1) - (smin.x + zero_x).max(zone.0);
                if grip <= 1e-9 {
                    f.push(setup("stock-not-gripped", Severity::Error, "the bar does not reach the chuck's jaws: nothing holds it".into()));
                } else if grip < 3.0 {
                    f.push(setup("shallow-grip", Severity::Warning, format!("the jaws grip only {} mm of the bar: aim for 3 mm or more", num(grip))));
                }
            }
            zero
        }
    };
    // The grip box is an obstacle where the stock is not.
    if let (Holding::Vise, Some(g)) = (hold.holding, hold.vise.as_ref().and_then(|v| v.grip)) {
        let gb = (Vec3::new(g.min[0], g.min[1], g.min[2]) + zero, Vec3::new(g.max[0], g.max[1], g.max[2]) + zero);
        for b in box_minus(gb, (smin + zero, smax + zero)) {
            push("the fixture the vise grips", ObstacleKind::Grip, b);
        }
    }
    obs.extend(prisms);
    for o in &mut obs {
        o.min -= zero;
        o.max -= zero;
        o.shape = shifted(std::mem::take(&mut o.shape), zero);
    }
    // The meshes, already in work coordinates once moved by `-zero`.
    let mut drawn = Vec::new();
    // One budget for all of them: each within the limits on its own, many could still ask for
    // more memory than the machine has.
    let (mut triangles_left, mut entries_left) = (MAX_CHECKED_TRIANGLES, MAX_ENTRIES);
    for m in meshes {
        if m.check && m.frame == MeshFrame::Table {
            let grid = MeshGrid::within(&m.mesh, Vec3::ZERO - zero, triangles_left, entries_left)?;
            triangles_left = triangles_left.saturating_sub(grid.triangles());
            entries_left = entries_left.saturating_sub(grid.entries());
            let name = if m.name.trim().is_empty() { "a mesh of the workholding".to_string() } else { m.name.trim().to_string() };
            obs.push(Obstacle { name, kind: m.kind, min: grid.min, max: grid.max, shape: ObstacleShape::Mesh(MeshShape(Arc::new(grid))) });
        }
        if m.kind != ObstacleKind::Machine {
            let mut mesh = m.mesh.clone();
            for p in &mut mesh.positions {
                *p -= zero;
            }
            drawn.push(DrawnMesh { name: m.name.clone(), turns: m.frame == MeshFrame::Rotary, color: m.color, mesh: Arc::new(mesh) });
        }
    }
    let holes = table.holes.iter().map(|h| Hole { x: h.x - zero.x, y: h.y - zero.y, ..h.clone() }).collect();
    Ok(HoldScene {
        holding: hold.holding,
        work_zero_table: zero,
        table: TablePlace { corner: Vec3::new(0.0, 0.0, 0.0) - zero, size: table.size },
        corner_radius: table.corner_radius,
        holes,
        obstacles: obs,
        stock_above_jaws,
        stock_grip,
        bar_grip,
        findings: f,
        meshes: drawn,
        work_zero_machine: hold.work_zero_machine.map(|[x, y, z]| Vec3::new(x, y, z)),
    })
}

fn num(v: f64) -> String {
    crate::gcode::number(v, 1)
}
