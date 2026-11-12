//! The workholding and machine model of a job (a child of `cmd/cam.rs`). A job that names a
//! workholding (`workholding_path` or `workholding`) and a machine model has every move of its
//! program checked: the cutter, shank, holder and the machine's parts against the vise, clamps,
//! parallels, table or rotary unit, the spindle head against the part, and the spindle nose
//! against the top of its travel. Each one found is a failure with its operation, G-code line,
//! place and a fix.

use std::path::Path;
use std::sync::Arc;

use solidcore_record::Vec3;
use solidcraft_cam::keys::ignored_warning;
use solidcraft_cam::kinematics::Machine;
use solidcraft_cam::machine_model::{MAX_TRIANGLES, MachineModel, MeshPart, Mount, ObstacleKind, mesh_parts, nose_above_tip};
use solidcraft_cam::obstacle::{MAX_CHECKED_TRIANGLES, ObstacleShape};
use solidcraft_cam::playback::{HeadBody, HoldHit, clearance_check, head_check};
use solidcraft_cam::sim::{SimParams, StockShape, plan_sim};
use solidcraft_cam::workholding::{HoldScene, Holding, LoadedMesh, MeshFrame, Workholding, build_with};
use solidcraft_cam::{Tool, findings::Severity};
use solidcraft_geom::Mesh;

use super::{MAX_JSON_BYTES, beside, cam_err, num};
use crate::params::bad;
use crate::schema::cam::{CamJob, HoldingHitReport, HoldingReport};
use crate::{EngineError, Result};

/// Collisions listed in a reply.
const LISTED: usize = 20;
/// Most bytes of a mesh file a machine model or a workholding names.
const MAX_MESH_BYTES: usize = 512 << 20;
/// How closely a STEP file's solids are meshed for the view and the checks (mm).
const STEP_CHORD: f64 = 0.05;
/// Largest coordinate a mesh may have once placed (mm).
const MAX_MESH_COORD: f64 = 1.0e5;

/// A job's machine model and workholding, read and placed with its stock.
#[derive(Default)]
pub(super) struct Holding3 {
    pub model: Option<Arc<MachineModel>>,
    /// The workholding placed with the stock, in work coordinates.
    pub scene: Option<Arc<HoldScene>>,
    /// How the workholding reads in a sentence: its own name, else its kind.
    pub name: Option<String>,
    /// The machine model's name.
    pub model_name: Option<String>,
    /// The machine model's meshes on the table that are checked against (with the workholding's).
    pub checked: Vec<LoadedMesh>,
}

/// `p` moved by the rows of a 3 x 4 matrix.
fn moved(t: &[[f64; 4]; 3], p: Vec3) -> Vec3 {
    let row = |r: &[f64; 4]| r[0] * p.x + r[1] * p.y + r[2] * p.z + r[3];
    Vec3::new(row(&t[0]), row(&t[1]), row(&t[2]))
}

/// The triangles of the mesh file at `at`: an STL file, or a STEP file whose solids are meshed
/// through the import (`STEP_CHORD`); moved by `transform`, at most `max` triangles, every
/// coordinate finite and within 100 m.
pub(super) fn mesh_file(at: &str, transform: Option<&[[f64; 4]; 3]>, max: usize) -> std::result::Result<Mesh, String> {
    let lower = at.to_ascii_lowercase();
    let mut mesh = Mesh::default();
    if lower.ends_with(".stl") {
        let bytes = solidcraft_io::vfs::read(at, MAX_MESH_BYTES).map_err(|e| e.to_string())?;
        let tris = solidcraft_io::read_stl(&bytes).map_err(|e| e.to_string())?;
        if tris.len() > max {
            return Err(format!("more than {max} triangles"));
        }
        for (k, t) in tris.iter().enumerate() {
            for q in t {
                mesh.positions.push(Vec3::new(f64::from(q[0]), f64::from(q[1]), f64::from(q[2])));
            }
            let b = u32::try_from(k * 3).map_err(|_| "too many triangles".to_string())?;
            mesh.triangles.push([b, b + 1, b + 2]);
        }
    } else if lower.ends_with(".step") || lower.ends_with(".stp") {
        let bytes = solidcraft_io::limits::read(at, solidcraft_io::limits::Kind::Step).map_err(|e| e.to_string())?;
        let sf = solidcraft_io::step_import_feature_named(&bytes, at, solidcraft_io::StepNames::Auto).map_err(|e| e.to_string())?;
        for b in &sf.import.bodies {
            let m = b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(STEP_CHORD)).map_err(|e| e.to_string())?;
            let start = u32::try_from(mesh.positions.len()).map_err(|_| "too many triangles".to_string())?;
            mesh.positions.extend(m.positions.iter().copied());
            for t in &m.triangles {
                let [Some(a), Some(c), Some(d)] = t.map(|i| i.checked_add(start)) else { return Err("too many triangles".into()) };
                mesh.triangles.push([a, c, d]);
            }
            if mesh.triangles.len() > max {
                return Err(format!("more than {max} triangles"));
            }
        }
    } else {
        return Err("a mesh is an .stl or a .step file".into());
    }
    if let Some(t) = transform {
        for p in &mut mesh.positions {
            *p = moved(t, *p);
        }
    }
    if !mesh
        .positions
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite() && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= MAX_MESH_COORD)
    {
        return Err("its coordinates must be finite, within 100 m".into());
    }
    if mesh.triangles.is_empty() {
        return Err("it has no triangles".into());
    }
    Ok(mesh)
}

/// The meshes a workholding names (beside `dir`), read: a mesh that is checked and cannot be read
/// is an error (the check would miss it), one drawn only a warning.
pub(super) fn hold_meshes(cmd: &str, hold: &Workholding, dir: Option<&Path>, warnings: &mut Vec<String>) -> Result<Vec<LoadedMesh>> {
    let kind = match hold.holding {
        Holding::Vise => ObstacleKind::Vise,
        Holding::Clamps => ObstacleKind::Clamp,
        Holding::Rotary => ObstacleKind::Rotary,
        Holding::Table => ObstacleKind::Fixture,
    };
    let mut out = Vec::new();
    let (mut drawn, mut checked) = (0usize, 0usize);
    for m in &hold.meshes {
        let at = beside(dir, &m.path);
        let check = m.checked();
        let room = if check { MAX_CHECKED_TRIANGLES.saturating_sub(checked) } else { MAX_TRIANGLES.saturating_sub(drawn) };
        match mesh_file(&at, m.transform.as_ref(), room) {
            Ok(mesh) => {
                if check {
                    checked += mesh.triangles.len();
                } else {
                    drawn += mesh.triangles.len();
                }
                let name = if m.name.trim().is_empty() {
                    Path::new(&m.path).file_name().map_or_else(|| m.path.clone(), |f| f.to_string_lossy().to_string())
                } else {
                    m.name.trim().to_string()
                };
                out.push(LoadedMesh { name, frame: m.frame, check, color: m.color, kind, mesh });
            }
            Err(e) if check => return Err(bad(cmd, format!("workholding mesh {at}: {e}: the tool is checked against it, so it must be read"))),
            Err(e) => warnings.push(format!("workholding mesh {at} was not read ({e}): it is not drawn")),
        }
    }
    Ok(out)
}

fn read_text(cmd: &str, path: &str, what: &str, max: usize) -> Result<String> {
    let bytes = solidcraft_io::vfs::read(path, max).map_err(|e| bad(cmd, format!("{what} {path}: {e}")))?;
    String::from_utf8(bytes).map_err(|_| bad(cmd, format!("{what} {path}: not UTF-8 text")))
}

/// The machine model at `path`, and its meshes on the table that are checked against (each
/// must be read; the meshes only drawn are pictures, not read here). Keys this build does not
/// know are left out with a warning.
pub(super) fn model_of(cmd: &str, path: &str, warnings: &mut Vec<String>) -> Result<(MachineModel, Vec<LoadedMesh>)> {
    let (model, ignored) = MachineModel::from_json_lenient(&read_text(cmd, path, "machine_model_path", MAX_JSON_BYTES)?)
        .map_err(|e| bad(cmd, format!("machine_model_path {path}: {e}")))?;
    warnings.extend(ignored_warning(&format!("machine_model_path {path}"), &ignored));
    let dir = Path::new(path).parent().map(Path::to_path_buf);
    let mut checked = Vec::new();
    let mut triangles = 0usize;
    for d in model.display_meshes.iter().filter(|d| d.check) {
        let at = beside(dir.as_deref(), &d.path);
        let lower = at.to_ascii_lowercase();
        let single = lower.ends_with(".stl") || lower.ends_with(".step") || lower.ends_with(".stp");
        let read = || -> std::result::Result<Vec<MeshPart>, String> {
            if single {
                let mesh = mesh_file(&at, d.transform.as_ref(), MAX_TRIANGLES.saturating_sub(triangles))?;
                let name = if d.name.trim().is_empty() { d.path.clone() } else { d.name.trim().to_string() };
                Ok(vec![MeshPart { name, mount: d.mount.unwrap_or(Mount::Spindle), color: d.color.unwrap_or([0.7, 0.7, 0.7]), mesh }])
            } else {
                let text = String::from_utf8(solidcraft_io::vfs::read(&at, MAX_MESH_BYTES).map_err(|e| e.to_string())?)
                    .map_err(|_| "not UTF-8".to_string())?;
                mesh_parts(&text, MAX_TRIANGLES.saturating_sub(triangles)).map_err(|e| e.to_string())
            }
        };
        let parts = read().map_err(|e| bad(cmd, format!("machine model mesh {at}: {e}: the tool is checked against it, so it must be read")))?;
        triangles += parts.iter().map(|p| p.mesh.triangles.len()).sum::<usize>();
        checked.extend(parts.into_iter().map(|p| LoadedMesh {
            name: p.name,
            frame: MeshFrame::Table,
            check: true,
            color: Some(p.color),
            kind: ObstacleKind::Machine,
            mesh: p.mesh,
        }));
    }
    Ok((model, checked))
}

/// The job's machine model and workholding, the workholding placed with `stock` on the model's
/// table. A workholding needs a machine model (its table); a rotary unit a machine with a
/// rotary axis.
pub(super) fn holding_of(
    cmd: &str,
    job: &CamJob,
    base: Option<&Path>,
    stock: &StockShape,
    machine: &Machine,
    notes: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<Holding3> {
    let model_path = job.machine_model_path.as_ref().map(|p| beside(base, p));
    if job.workholding.is_some() && job.workholding_path.is_some() {
        return Err(bad(cmd, "give the job `workholding` or `workholding_path`, not both"));
    }
    let hold_path = job.workholding_path.as_ref().map(|p| beside(base, p));
    // The workholding as text, where it came from (a file, or the job's own `workholding`), and
    // the folder its meshes are read from.
    let hold = match (&hold_path, &job.workholding) {
        (Some(path), _) => Some((
            format!("workholding_path {path}"),
            read_text(cmd, path, "workholding_path", MAX_JSON_BYTES)?,
            true,
            Path::new(path).parent().map(Path::to_path_buf),
        )),
        (None, Some(inline)) => Some((
            "the job's `workholding`".to_string(),
            serde_json::to_string(inline).map_err(|e| EngineError::Other(e.to_string()))?,
            false,
            base.map(Path::to_path_buf),
        )),
        (None, None) => None,
    };
    let mut out = Holding3::default();
    if let Some(path) = &model_path {
        let (m, checked) = model_of(cmd, path, warnings)?;
        out.checked = checked;
        if m.kinematics.is_some() || m.kinematics_path.is_some() {
            notes.push("the job's machine is used for the axes; the machine model's kinematics are not".into());
        }
        out.model_name = Some(if m.name.trim().is_empty() { "the machine model".into() } else { m.name.clone() });
        out.model = Some(Arc::new(m));
    }
    if let Some((what, text, file, dir)) = &hold {
        let Some(m) = &out.model else {
            return Err(bad(cmd, "a workholding needs a machine model (its table): give the job `machine_model_path`"));
        };
        // A file a newer build wrote reads with its unknown keys left out; the job's own refuses them.
        let hold = if *file {
            let (h, ignored) = Workholding::from_json_lenient(text).map_err(|e| bad(cmd, format!("{what}: {e}")))?;
            warnings.extend(ignored_warning(what, &ignored));
            h
        } else {
            Workholding::from_json(text).map_err(|e| bad(cmd, format!("{what}: {e}")))?
        };
        if hold.holding == Holding::Rotary && machine.rotaries.is_empty() {
            return Err(bad(cmd, "a rotary unit's workholding needs a machine with a rotary axis"));
        }
        let kind = serde_json::to_value(hold.holding).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        out.name = Some(if hold.name.trim().is_empty() { kind } else { hold.name.trim().to_string() });
        let mut meshes = hold_meshes(cmd, &hold, dir.as_deref(), warnings)?;
        meshes.extend(out.checked.iter().cloned());
        out.scene = Some(Arc::new(build_with(&m.table, &hold, stock, &meshes).map_err(|e| cam_err(cmd, what, e))?));
    } else if !out.checked.is_empty() {
        // Their place on the bed comes from the workholding's table: without one, say so rather
        // than let `check: true` read as checked.
        let names: Vec<String> = out.checked.iter().map(|m| format!("`{}`", m.name)).collect();
        warnings.push(format!(
            "the machine model's mesh {} is not checked: the job has no workholding to place the table it stands on",
            names.join(", ")
        ));
    }
    Ok(out)
}

/// What may set how high the tool goes in a program: each operation's own clearance (its
/// highest rapid, in program Z, by its name) and the post's `clearance`. The post rises to the
/// highest of them where it does not know the tool's height (at the start, after a tool change
/// or its safe lines), so a move may go above its own operation's clearance.
#[derive(Clone, Debug, Default)]
pub(super) struct Rises {
    pub ops: Vec<(String, f64)>,
    pub post: Option<f64>,
}

/// What set the height of a move that went too high, as a fix names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RiseBy<'a> {
    /// The operation's own clearance (its rapids, and on a 4th axis its turns and rewinds of A).
    Own,
    /// Another operation's clearance, the program's highest.
    Other(&'a str),
    /// The post's `clearance`.
    Post,
}

impl Rises {
    /// What set the height `z` of a move of the operation `op`.
    pub(super) fn by(&self, op: Option<&str>, z: f64) -> RiseBy<'_> {
        // Program Z is written to 0.001 mm.
        let near = |a: f64| (a - z).abs() <= 2e-3;
        let own = op.and_then(|n| self.ops.iter().find(|(m, _)| m == n)).map(|(_, c)| *c);
        if own.is_some_and(|c| near(c) || z < c) {
            return RiseBy::Own;
        }
        if self.post.is_some_and(near) {
            return RiseBy::Post;
        }
        match self.ops.iter().find(|(m, c)| near(*c) && Some(m.as_str()) != op) {
            Some((m, _)) => RiseBy::Other(m),
            None => RiseBy::Own,
        }
    }
}

/// The highest work Z tool `tool`'s tip may reach before the spindle's nose passes the top of its
/// travel, to a tenth of a millimetre (down), when the machine model says how high it goes.
fn z_travel_fits(tool: Option<&Tool>, model: Option<&MachineModel>, scene: &HoldScene) -> Option<f64> {
    match (model.and_then(|m| m.spindle.nose_max_above_table), tool) {
        (Some(max), Some(t)) => Some(((scene.table.corner.z + max - nose_above_tip(t, model)) * 10.0).floor() / 10.0),
        _ => None,
    }
}

/// What to change for a collision of kind `code` (as `machine_model::clearance` names them);
/// `stock_top` is the stock's highest point (work Z), which no clearance may go below; `rises`
/// and `op` (the operation the move is in) say what set the height of a move that went too high.
#[allow(clippy::too_many_arguments)]
fn fix_for(
    code: &str,
    hit: &HoldHit,
    tool: Option<&Tool>,
    model: Option<&MachineModel>,
    scene: &HoldScene,
    stock_top: f64,
    rises: &Rises,
    op: Option<&str>,
) -> String {
    let part = code.split("-hits-").next().unwrap_or(code);
    let with = code.rsplit("-hits-").next().unwrap_or(code);
    if code == "z-travel" {
        let fits = z_travel_fits(tool, model, scene);
        // A clearance must stay above the stock (and above work zero on its top).
        let lowest = stock_top.max(0.0);
        let by = hit.first.at.map_or(RiseBy::Own, |a| rises.by(op, a.z));
        let height = hit.first.at.map_or_else(|| "its height".to_string(), |a| format!("Z{}", num((a.z * 1000.0).round() / 1000.0)));
        return match fits {
            Some(z) if z > lowest => {
                // `fits` is already down to a tenth of a millimetre.
                let to = num(z);
                let what = match by {
                    RiseBy::Own => format!(
                        "this move is at the operation's own clearance ({height}; its rapids, and on a 4th axis its turns and rewinds \
                         of A, go no higher): lower that clearance to Z{to} or below"
                    ),
                    RiseBy::Other(name) => format!(
                        "this move rises to {height}, the clearance of operation `{name}`, the program's highest, which the post rises \
                         to where it does not know the tool's height (at the start, after a tool change or its safe lines): lower \
                         `{name}`'s clearance to Z{to} or below"
                    ),
                    RiseBy::Post => format!("the post's `clearance` ({height}) sets this height: lower it to Z{to} or below"),
                };
                format!(
                    "{what} for tool {}, set the tool shorter in its holder (less stickout), or lower the workholding (lower parallels)",
                    hit.tool
                )
            }
            _ => {
                // By how much the tool is too long: its tip must stay above the stock's top, and
                // at this move it is higher still.
                let short = fits.map(|z| ((lowest - z) * 10.0).ceil() / 10.0).filter(|d| *d > 0.0);
                let here = match (fits, hit.first.at) {
                    (Some(z), Some(a)) if a.z > z => format!(" ({} mm at this move, {height})", num(((a.z - z) * 10.0).ceil() / 10.0)),
                    _ => String::new(),
                };
                let by = short.map_or_else(String::new, |d| format!(" by {} mm or more{here}", num(d)));
                let lower = if scene.obstacles.iter().any(|o| o.kind == ObstacleKind::Parallels) {
                    "lower parallels or none, or clamps straight on the table"
                } else {
                    "clamps straight on the table, or the stock lower in its holding"
                };
                format!(
                    "no clearance above the stock's top (Z{}) fits tool {} with its holder: the spindle would go past the top of its travel{}. \
                     Shorten it{by}: set the tool shorter in its holder (less stickout), use a shorter tool, or lower the workholding \
                     ({lower})",
                    num(lowest),
                    hit.tool,
                    short.map_or_else(String::new, |d| format!(" even at the stock's top, by {} mm", num(d))),
                )
            }
        };
    }
    if code == "z-travel-low" {
        return format!(
            "the spindle cannot reach this low with tool {}: stick the tool out further or use a longer one, or raise the work (taller \
             parallels, a riser under the vise)",
            hit.tool
        );
    }
    if with == "part" || with == "fixture-body" {
        return format!(
            "the spindle head reaches {}: stick tool {} out further (or use a longer tool) so the head stays above it, or keep the \
             operation away from the tall walls near it",
            if with == "part" { "the part" } else { "a fixture of the design" },
            hit.tool
        );
    }
    if code == "cutter-into-table" {
        return "raise the operation's bottom, or put a spoil board or parallels under the stock (and give them in the workholding)".into();
    }
    // A part of its own (a mesh of the workholding, a part or ring of a rotary unit, a part of the
    // machine on the table) is not the vise's jaws or the chuck: the fix names it.
    let own = scene.obstacles.iter().filter(|o| o.kind.code() == with).find(|o| hit.first.message.ends_with(&format!("({})", o.name))).filter(|o| {
        matches!(o.shape, ObstacleShape::Mesh(_))
            || o.kind == ObstacleKind::Machine
            || (o.kind == ObstacleKind::Rotary && o.name != "the rotary unit's chuck" && o.name != "the rotary unit's jaws")
    });
    if let (Some(o), "cutter" | "shank" | "holder") = (own, part) {
        let what = match part {
            "cutter" => "the cut".to_string(),
            _ => format!("tool {}'s {part}", hit.tool),
        };
        return format!(
            "keep {what} clear of {}: give the operation a boundary (or limit its range along X on a rotary unit) that stays off it, \
             stick the tool out further if its holder reaches it, or place the work further from it",
            o.name
        );
    }
    match (part, with) {
        ("cutter", "parallels") => "move the parallels out from under the cut (closer together, or narrower), raise the operation's bottom to \
             the stock's bottom, or put the stock on a spoil board instead"
            .into(),
        ("cutter", "vise" | "clamp" | "grip") => format!(
            "keep the cut off {}: raise the stock in the vise (taller parallels) so the cut stays above the jaws, give the operation a \
             boundary that keeps the tool clear, or hold the part another way (other jaws, clamps, a second setup)",
            if with == "clamp" { "the clamps" } else { "the jaws" }
        ),
        ("cutter", "rotary") => "keep the cut clear of the chuck: limit the operation's range along X to stay off the jaws, or grip the bar \
             further from the part (longer stock)"
            .into(),
        ("shank" | "holder", "rotary") => format!(
            "tool {} reaches the chuck with its holder: stick the tool out further, limit the operation's range along X so it stays clear of \
             the jaws, or grip the bar further from the part",
            hit.tool
        ),
        ("shank" | "holder", _) => format!(
            "tool {} reaches {} with its holder: stick the tool out further (stickout), raise the stock (taller parallels), or keep the \
             operation clear of it",
            hit.tool,
            scene.obstacles.iter().find(|o| o.kind.code() == with).map_or("the workholding", |o| o.kind.noun())
        ),
        _ => "keep the machine clear of the workholding: move the stock or the workholding, or change the operation".into(),
    }
}

/// Every move of the program `text` (for `machine`, with `tools`) checked against the job's
/// workholding and machine model, `op_at_line` naming the operation (by its index in the job,
/// and its name) each line is under, `rises` what sets how high the tool goes (for the fix of a
/// move too high for the spindle), `bodies` what the spindle head is checked against. Errors
/// (and the spindle's travel, an error here) go into `failures`; `None` when the job has no
/// workholding.
#[allow(clippy::too_many_arguments)]
pub(super) fn check_holding(
    cmd: &str,
    text: &str,
    machine: &Machine,
    tools: &[Tool],
    stock: &StockShape,
    holding: &Holding3,
    tol: f64,
    op_at_line: &dyn Fn(usize) -> Option<(usize, String)>,
    rises: &Rises,
    bodies: &[HeadBody<'_>],
    failures: &mut Vec<String>,
) -> Result<Option<HoldingReport>> {
    let Some(scene) = holding.scene.as_deref() else { return Ok(None) };
    let model = holding.model.as_deref();
    // Coarse cells: only the moves are needed, never the stock's cells.
    let span = solidcraft_profile::span("cam", "hold.plan_sim");
    let plan = plan_sim(text, machine, tools, stock, &SimParams { tolerance: tol, resolution: Some(1.0e3) })
        .map_err(|e| cam_err(cmd, "checking the program against the workholding", e))?;
    drop(span);
    // Each operation that has a collision is named, not only the first.
    let group = |line| op_at_line(line).map_or(usize::MAX, |(index, _)| index);
    let span = solidcraft_profile::span("cam", "hold.clearance");
    let mut hits = clearance_check(&plan, tools, model, Some(scene), &group);
    drop(span);
    // The spindle head's body (the machine model's exact parts that move with the tool) against
    // the part and the design's fixtures, as the holder check takes the tool's holder.
    if let Some(m) = model {
        let _span = solidcraft_profile::span("cam", "hold.head");
        hits.extend(head_check(&plan, tools, m, Some(scene), bodies, &group));
    }
    let stock_top = match *stock {
        StockShape::Box { max, .. } => max.z,
        StockShape::Cylinder { radius, .. } => radius,
    };
    let name = holding.name.clone().unwrap_or_else(|| "the workholding".into());
    let mut listed = Vec::new();
    let mut setup = Vec::new();
    // The setup itself (the stock too low in the vise, a clamp off the table): as the view says.
    for f in scene.findings.iter().filter(|f| f.severity == Severity::Error) {
        setup.push(f.message.clone());
        failures.push(format!("workholding: the setup in {name}: {}: change the stock or the workholding so the stock is held", f.message));
    }
    let mut errors = 0usize;
    for h in &hits {
        let op = op_at_line(h.first.line);
        let opname = op.as_ref().map_or(String::new(), |(_, n)| format!("operation `{n}`: "));
        let at = h.first.at.map(|a| [a.x, a.y, a.z]);
        let place = at.map_or(String::new(), |a| format!(" at X{} Y{} Z{} (work coordinates)", num(a[0]), num(a[1]), num(a[2])));
        let check = match h.first.code.as_str() {
            "z-travel" | "z-travel-low" => "z_travel",
            c if c.ends_with("-hits-part") || c.ends_with("-hits-fixture-body") => "head",
            _ => "workholding",
        };
        let tool = tools.iter().find(|t| t.number == h.tool);
        let fix = fix_for(&h.first.code, h, tool, model, scene, stock_top, rises, op.as_ref().map(|(_, n)| n.as_str()));
        // The spindle's travel is an error here: the program cannot run as written.
        let error = h.first.severity == Severity::Error || h.first.code == "z-travel";
        if error {
            errors += 1;
            failures.push(format!(
                "{check}: {opname}{} (tool {}, {}), along {} moves; first at G-code line {}{place}: {fix}",
                h.first.message, h.tool, h.first.code, h.moves, h.first.line
            ));
        }
        if listed.len() < LISTED {
            listed.push(HoldingHitReport {
                code: h.first.code.clone(),
                error,
                tool: h.tool,
                operation: op_at_line(h.first.line).map(|(_, n)| n),
                line: h.first.line,
                moves: h.moves,
                at,
                message: h.first.message.clone(),
                fix,
            });
        }
    }
    Ok(Some(HoldingReport {
        name,
        holding: serde_json::to_value(scene.holding).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        machine_model: holding.model_name.clone(),
        checked_moves: plan.moves(),
        hits: listed,
        stock_above_jaws: scene.stock_above_jaws,
        bar_grip: scene.bar_grip.map(grip_report),
        pass: errors == 0 && setup.is_empty(),
        setup,
        setup_warnings: Vec::new(),
    }))
}

/// How a bar sits in the chuck, as a reply reports it.
fn grip_report(g: solidcraft_cam::workholding::BarGrip) -> crate::schema::cam::BarGripReport {
    crate::schema::cam::BarGripReport {
        face_x: g.face_x,
        held_end_x: g.held_end_x,
        free_end_x: g.free_end_x,
        in_jaws: g.in_jaws,
        stands_out: g.stands_out,
        zero_out: g.zero_out,
    }
}
