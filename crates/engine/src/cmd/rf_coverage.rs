//! Wi-Fi coverage (`inspect.rf_coverage`, #20161): the signal a router or access point gives
//! across a floor plan, from the walls the model's bodies make at a height, shown as a heatmap.
//!
//! - **The walls are read exactly.** Each body that crosses the slice height is cut there: a
//!   proved prism by its section ([`solidcraft_kernel::certified_prism_section`]), any other solid
//!   by the half-space cut a section view uses ([`solidcraft_doc::ModelBody::section`]) and its cut
//!   faces, a mesh body by its own triangles. Curved outlines are followed to within
//!   [`FLATTEN_MM`].
//! - **Each region gets a loss.** Its body's material's RF loss (`material.create`'s `rf`), a
//!   built-in material's default, or a guess from the wall's thickness where each path crosses
//!   it ([`WallRegion::thickness_at`]): drywall below [`CONCRETE_FROM_MM`], concrete from there.
//!   One body may hold a whole floor's walls, thin and thick.
//! - **The model** is [`crate::rf_coverage`]'s. The walls are kept ([`WallCache`]) while the
//!   model, the slice height and the bodies taken stay the same, so moving an emitter only traces
//!   the paths again.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::{Plane, Region2, Vec3};
use solidcraft_doc::rf::{Band, CONCRETE_FROM_MM, RfLoss, builtin_rf_loss, heuristic_loss};
use solidcraft_doc::{ModelBody, ModelState};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::bad;
use crate::rf_coverage::{
    Cell, Emitter, FLATTEN_MM, Grid, Model, Scratch, WallMap, WallRegion, best_at, coverage, enclosed, rgba, stats, wall_cells,
};
use crate::schema::rf_coverage::{
    BandName, BandStats, DeviceReply, EirpParam, EmitterReply, GridReply, PointReply, RfCoverageParams, RfCoverageReply, WallsReply,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<RfCoverageParams> = TypedContract::new();
const CMD: &str = "inspect.rf_coverage";

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Wi-Fi Coverage", crate::checked!("inspect.rf_coverage", rf_coverage))
    .at("SOLID", "INSPECT")
    .icon("section")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<RfCoverageReply>)
    .params(
        "Predict Wi-Fi signal strength (RSSI) over a floor plan from the walls the bodies make at a height, and show it as a heatmap. \
         emitters: [{name?, at: [x, y] | [x, y, z] (mm, world), height?: mm (default client_height), device?: built-in id (call with no \
         emitters to list them) | eirp_dbm?: dBm | {\"2.4\", \"5\", \"6\": dBm}}]; band?: \"2.4\" | \"5\" | \"6\" | \"all\" (default all); \
         slice_height?: mm where the bodies are cut (default 1000); client_height?: mm (default 1000); grid_step?: mm 10–2000 (default 75); \
         exponent?: path-loss exponent 1.5–6 (default 2.2); region?: {polygon: [[x, y]…]} | {body: name} (default: the floor the walls \
         enclose); points?: [{name, at: [x, y]}] (RSSI at rooms); bodies?: [names] | components?: [names] (only these make walls); \
         rf_materials?: bool (with bodies or components, bodies whose material has an RF loss count too); exclude?: [body or component names] (furniture); include_hidden?: bool (default false: hidden bodies count only when their \
         material has an RF loss); cap_eirp?: bool (default true: 30 dBm at 2.4/5 GHz, 27 at 6 GHz); png?: path; grid?: bool (return \
         the values); show?: bool (default true); clear?: true. A wall's loss comes from its material's rf (material.create), a built-in \
         material's default, or its thickness (under 140 mm drywall, else concrete). RSSI = EIRP − FSPL(1 m) − 10·n·log10(d) − losses \
         of each material run the straight path enters; d is 3D. Returns stats per band (pct_at_least_60, pct_at_least_67, p5_dbm…), \
         points, grid metadata and walls; the design doesn't change",
    )];

/// The most emitters, points and named bodies one call takes.
const MAX_EMITTERS: usize = 64;
const MAX_POINTS: usize = 1000;
const MAX_NAMES: usize = 20_000;
/// The longest a name may be.
const MAX_NAME: usize = 256;
/// The most bodies one slice cuts.
const MAX_BODIES: usize = 50_000;
/// How far an emitter's or point's coordinates may lie from the origin, in mm (100 km).
const MAX_COORD: f64 = 1.0e8;
/// The EIRP indoor limits by band (dBm): 30 dBm at 2.4 and 5 GHz, 27 dBm on 6 GHz low-power indoor.
pub const EIRP_CAP_DBM: [f64; 3] = [30.0, 30.0, 27.0];
/// The EIRP an emitter without a device or `eirp_dbm` has, in every band (dBm).
const DEFAULT_EIRP_DBM: f64 = 20.0;

/// A Wi-Fi device the library knows: its EIRP per band before the cap (dBm), the transmit power
/// plus the antenna gain on its public spec sheet; `None` where it has no radio.
pub struct Device {
    pub id: &'static str,
    pub name: &'static str,
    pub eirp_dbm: [Option<f64>; 3],
}

/// The built-in devices. Transmit power plus antenna gain from the makers' public spec sheets
/// (techspecs.ui.com, read November 2026).
pub const DEVICES: &[Device] = &[
    Device { id: "ucg-industrial", name: "UniFi Cloud Gateway Industrial", eirp_dbm: [Some(26.0), Some(31.0), None] },
    Device { id: "u7-lite", name: "UniFi U7 Lite", eirp_dbm: [Some(27.0), Some(29.0), None] },
    Device { id: "u7-pro", name: "UniFi U7 Pro", eirp_dbm: [Some(27.0), Some(32.0), Some(28.8)] },
    Device { id: "u7-pro-wall", name: "UniFi U7 Pro Wall", eirp_dbm: [Some(26.0), Some(31.0), Some(29.0)] },
    Device { id: "u7-pro-xgs", name: "UniFi U7 Pro XGS (4×4 on 5 GHz)", eirp_dbm: [Some(27.0), Some(35.0), Some(30.0)] },
    Device { id: "udr-5g-max", name: "UniFi Dream Router 5G Max", eirp_dbm: [Some(27.0), Some(31.0), Some(30.0)] },
    Device { id: "udr7", name: "UniFi Dream Router 7", eirp_dbm: [Some(28.0), Some(33.0), Some(30.0)] },
    Device { id: "ux7", name: "UniFi Express 7", eirp_dbm: [Some(27.0), Some(33.0), Some(30.0)] },
];

/// The device `id` names.
pub fn device(id: &str) -> Option<&'static Device> {
    DEVICES.iter().find(|d| d.id.eq_ignore_ascii_case(id.trim()))
}

/// The walls one slice made, kept for the next call while the model and the slice settings stay
/// the same.
pub(crate) struct WallCache {
    world: Arc<ModelState>,
    key: u64,
    walls: Arc<WallMap>,
    report: Arc<WallReport>,
}

/// What a slice found, for the reply.
#[derive(Clone, Debug, Default)]
struct WallReport {
    bodies: usize,
    skipped: Vec<String>,
}

/// The heatmap the view shows.
#[derive(Clone, Debug)]
pub struct RfOverlay {
    pub grid: Grid,
    /// Straight RGBA8, one pixel a cell, the first row the grid's lowest.
    pub rgba: Arc<Vec<u8>>,
    /// The height the heatmap lies at (the slice height), mm.
    pub height: f64,
    pub band: Band,
    pub emitters: Vec<Emitter>,
    /// The parameters it was made with, so a view can move an emitter and ask again.
    pub params: Value,
    /// Changes whenever the picture does.
    pub version: u64,
    /// The legend: (lowest dBm of the step, colour).
    pub legend: Vec<(f64, [u8; 3])>,
    /// The shown band's figures: % of the floor at −60 dBm or better, at −67 dBm or better, and
    /// the 5th percentile (dBm).
    pub summary: Option<[f64; 3]>,
}

fn rf_coverage(s: &mut Session, p: &Value) -> Result<Value> {
    let started = web_time::Instant::now();
    let params: RfCoverageParams =
        serde_json::from_value(if p.is_null() { Value::Object(Default::default()) } else { p.clone() }).map_err(|e| bad(CMD, e.to_string()))?;
    if params.clear == Some(true) {
        let given = p.as_object().map_or(0, |o| o.len());
        if given > 1 {
            return Err(bad(CMD, "`clear` is given alone"));
        }
        s.rf_coverage = None;
        s.refresh_display();
        return reply(empty(false, started));
    }
    let Some(emitter_params) = params.emitters.as_ref().filter(|e| !e.is_empty()) else {
        // Without emitters: the device library, so a caller can pick one.
        let mut out = empty(s.rf_coverage.is_some(), started);
        out.devices = DEVICES.iter().map(device_reply).collect();
        return reply(out);
    };
    if emitter_params.len() > MAX_EMITTERS {
        return Err(bad(CMD, format!("at most {MAX_EMITTERS} emitters")));
    }
    let length = |key: &str, v: Option<f64>, default: f64, lo: f64, hi: f64| -> Result<f64> {
        let v = v.unwrap_or(default);
        if v.is_finite() && (lo..=hi).contains(&v) { Ok(v) } else { Err(bad(CMD, format!("`{key}` must be a number from {lo} to {hi}"))) }
    };
    let slice = length("slice_height", params.slice_height, 1000.0, -MAX_COORD, MAX_COORD)?;
    let client = length("client_height", params.client_height, 1000.0, -MAX_COORD, MAX_COORD)?;
    let step = length("grid_step", params.grid_step, 75.0, 10.0, 2000.0)?;
    let exponent = length("exponent", params.exponent, 2.2, 1.5, 6.0)?;
    let gap = length("close_gaps", params.close_gaps, 1000.0, 0.0, 5000.0)?;
    let cap = params.cap_eirp.unwrap_or(true);
    let emitters = emitter_params.iter().enumerate().map(|(i, e)| emitter(i, e, client, cap)).collect::<Result<Vec<_>>>()?;
    let bands: Vec<Band> = match params.band.unwrap_or(BandName::All) {
        BandName::G2_4 => vec![Band::G2_4],
        BandName::G5 => vec![Band::G5],
        BandName::G6 => vec![Band::G6],
        BandName::All => Band::ALL.into_iter().filter(|b| emitters.iter().any(|e| e.eirp(*b).is_some())).collect(),
    };
    if bands.is_empty() || bands.iter().any(|b| emitters.iter().all(|e| e.eirp(*b).is_none())) {
        return Err(bad(CMD, "no emitter has a radio in that band"));
    }
    let shown = if bands.contains(&Band::G5) && bands.len() > 1 { Band::G5 } else { bands.first().copied().unwrap_or(Band::G5) };
    let points = params.points.clone().unwrap_or_default();
    if points.len() > MAX_POINTS {
        return Err(bad(CMD, format!("at most {MAX_POINTS} points")));
    }
    if points.iter().any(|pt| pt.at.iter().any(|c| !c.is_finite() || c.abs() > MAX_COORD) || pt.name.chars().count() > MAX_NAME) {
        return Err(bad(CMD, "a point needs a name of at most 256 characters and finite world [x, y] in mm"));
    }
    let names = |list: &Option<Vec<String>>, key: &str| -> Result<Vec<String>> {
        let list = list.clone().unwrap_or_default();
        if list.len() > MAX_NAMES || list.iter().any(|n| n.chars().count() > MAX_NAME) {
            return Err(bad(CMD, format!("`{key}`: at most {MAX_NAMES} names of at most {MAX_NAME} characters")));
        }
        Ok(list)
    };
    let scope = Scope {
        bodies: names(&params.bodies, "bodies")?,
        components: names(&params.components, "components")?,
        exclude: names(&params.exclude, "exclude")?,
        include_hidden: params.include_hidden.unwrap_or(false),
        rf_materials: params.rf_materials.unwrap_or(false),
    };

    if s.model.is_stale() {
        return Err(EngineError::Other("the model is being changed; finish the gesture before working out Wi-Fi coverage".into()));
    }
    let world = s.world_state()?;
    let (walls, report, reused) = walls_for(s, &world, slice, &scope)?;

    // The floor: a given region, else what the walls enclose, else every cell not in a wall.
    let given = match &params.region {
        Some(r) => Some(region(&world, r)?),
        None => None,
    };
    let bounds = match &given {
        Some((r, _)) => r.iter().map(|r| r.bounds).reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
        // Room round the walls for the outside to flow past them.
        None => walls.bounds().map(|b| {
            let m = 0.5 * gap + 2.0 * step;
            [b[0] - m, b[1] - m, b[2] + m, b[3] + m]
        }),
    }
    .ok_or_else(|| bad(CMD, format!("no body crosses the slice height {slice} mm: nothing makes walls there")))?;
    let grid = Grid::covering(bounds, step).ok_or_else(|| bad(CMD, "the grid would have too many cells: give a larger `grid_step` or a `region`"))?;
    let solid = wall_cells(&walls, &grid);
    let (inside, kind): (Vec<bool>, String) = match &given {
        Some((regions, kind)) => {
            let inside = (0..grid.len()).map(|i| regions.iter().any(|r| r.contains(grid.centre(i % grid.columns, i / grid.columns)))).collect();
            (inside, kind.clone())
        }
        None => {
            let e = enclosed(&walls, &grid, &solid, gap);
            if e.iter().any(|x| *x) { (e, "enclosed".into()) } else { (vec![true; grid.len()], "open".into()) }
        }
    };
    let cells: Vec<Cell> = solid
        .iter()
        .zip(&inside)
        .map(|(w, i)| {
            if *w {
                Cell::Wall
            } else if *i {
                Cell::Floor
            } else {
                Cell::Outside
            }
        })
        .collect();
    let model = Model { exponent, client_height: client };
    let mut values: BTreeMap<Band, Vec<f32>> = BTreeMap::new();
    for &band in &bands {
        values.insert(band, coverage(&walls, &emitters, &model, band, &grid, &cells));
    }
    let floor_cells = cells.iter().filter(|c| **c == Cell::Floor).count();
    let stats_out: Vec<BandStats> = bands
        .iter()
        .filter_map(|b| {
            let st = stats(values.get(b)?, &cells)?;
            let round = |v: f64| (v * 10.0).round() / 10.0;
            Some(BandStats {
                band: b.name().into(),
                pct_at_least_60: round(st.good * 100.0),
                pct_at_least_67: round(st.fair * 100.0),
                p5_dbm: round(st.p5),
                median_dbm: round(st.median),
                min_dbm: round(st.min),
                max_dbm: round(st.max),
            })
        })
        .collect();
    let mut scratch = Scratch::default();
    let points_out = points
        .iter()
        .map(|pt| {
            let mut rssi = BTreeMap::new();
            let mut from = BTreeMap::new();
            let mut walls_db = BTreeMap::new();
            for &b in &bands {
                if let Some((v, i)) = best_at(&walls, &emitters, &model, b, pt.at, &mut scratch) {
                    rssi.insert(b.name().to_string(), (v * 10.0).round() / 10.0);
                    if let Some(e) = emitters.get(i) {
                        from.insert(b.name().to_string(), e.name.clone());
                        walls_db.insert(b.name().to_string(), (walls.path_loss(e.at, pt.at, b, &mut scratch) * 10.0).round() / 10.0);
                    }
                }
            }
            PointReply { name: pt.name.clone(), at: pt.at, rssi_dbm: rssi, from, walls_db }
        })
        .collect();
    let shown_values = values.get(&shown).cloned().unwrap_or_default();
    let picture = rgba(&shown_values, &cells, &grid);
    let png = match &params.png {
        Some(path) => Some(write_png(path, &picture, &grid, &emitters, &walls)?),
        None => None,
    };
    let show = params.show.unwrap_or(true);
    if show {
        let version = s.rf_coverage.as_ref().map_or(1, |o| o.version.wrapping_add(1));
        s.rf_coverage = Some(Arc::new(RfOverlay {
            grid: grid.clone(),
            rgba: Arc::new(picture),
            height: slice,
            band: shown,
            emitters: emitters.clone(),
            params: p.clone(),
            version,
            legend: crate::rf_coverage::SCALE.to_vec(),
            summary: stats_out.iter().find(|st| st.band == shown.name()).map(|st| [st.pct_at_least_60, st.pct_at_least_67, st.p5_dbm]),
        }));
        s.refresh_display();
    }
    let mut materials: BTreeMap<String, usize> = BTreeMap::new();
    for r in &walls.regions {
        let name = if r.thick.is_some() { Some("guessed by thickness".to_string()) } else { walls.materials.get(r.material).map(|(n, _)| n.clone()) };
        if let Some(name) = name {
            *materials.entry(name).or_default() += 1;
        }
    }
    let grid_values = params.grid.unwrap_or(false).then(|| {
        values.iter().map(|(b, v)| (b.name().to_string(), v.iter().map(|x| x.is_finite().then_some((x * 10.0).round() / 10.0)).collect())).collect()
    });
    let out = RfCoverageReply {
        shown: show,
        grid: Some(GridReply {
            origin: grid.origin,
            step: grid.step,
            columns: grid.columns,
            rows: grid.rows,
            floor_cells,
            floor_m2: (floor_cells as f64 * step * step / 1e6 * 10.0).round() / 10.0,
            region: kind,
            values: grid_values,
        }),
        walls: Some(WallsReply { bodies: report.bodies, regions: walls.regions.len(), materials, skipped: report.skipped.clone() }),
        emitters: emitters.iter().zip(emitter_params).map(|(e, given)| emitter_reply(e, given.device.as_deref())).collect(),
        stats: stats_out,
        points: points_out,
        showing: Some(shown.name().into()),
        png,
        devices: Vec::new(),
        walls_reused: reused,
        ms: elapsed(started),
        param_warnings: Vec::new(),
    };
    reply(out)
}

fn elapsed(started: web_time::Instant) -> f64 {
    (started.elapsed().as_secs_f64() * 10_000.0).round() / 10.0
}

fn empty(shown: bool, started: web_time::Instant) -> RfCoverageReply {
    RfCoverageReply {
        shown,
        grid: None,
        walls: None,
        emitters: Vec::new(),
        stats: Vec::new(),
        points: Vec::new(),
        showing: None,
        png: None,
        devices: Vec::new(),
        walls_reused: false,
        ms: elapsed(started),
        param_warnings: Vec::new(),
    }
}

fn reply(out: RfCoverageReply) -> Result<Value> {
    serde_json::to_value(out).map_err(|e| bad(CMD, e.to_string()))
}

fn band_map(values: &[Option<f64>; 3]) -> BTreeMap<String, f64> {
    Band::ALL.iter().filter_map(|b| Some((b.name().to_string(), (*values.get(b.index())?)?))).collect()
}

fn device_reply(d: &Device) -> DeviceReply {
    DeviceReply { id: d.id.into(), name: d.name.into(), eirp_dbm: band_map(&d.eirp_dbm) }
}

fn emitter_reply(e: &Emitter, device: Option<&str>) -> EmitterReply {
    EmitterReply { name: e.name.clone(), at: e.at, height: e.height, device: device.map(str::to_string), eirp_dbm: band_map(&e.eirp_dbm) }
}

/// The emitter `p` describes, the `i`th.
fn emitter(i: usize, p: &crate::schema::rf_coverage::EmitterParam, client: f64, cap: bool) -> Result<Emitter> {
    let finite = |v: f64| v.is_finite() && v.abs() <= MAX_COORD;
    let (at, z) = match p.at.as_slice() {
        [x, y] => ([*x, *y], None),
        [x, y, z] => ([*x, *y], Some(*z)),
        _ => return Err(bad(CMD, "an emitter's `at` is world [x, y] or [x, y, z] in mm")),
    };
    let height = p.height.or(z).unwrap_or(client);
    if !at.iter().all(|v| finite(*v)) || !finite(height) {
        return Err(bad(CMD, "an emitter's position and height must be finite numbers in mm"));
    }
    let mut eirp = match (&p.device, &p.eirp_dbm) {
        (Some(_), Some(_)) => return Err(bad(CMD, "give an emitter `device` or `eirp_dbm`, not both")),
        (Some(id), None) => {
            let ids: Vec<&str> = DEVICES.iter().map(|d| d.id).collect();
            device(id).ok_or_else(|| bad(CMD, format!("no device `{id}`: one of {}", ids.join(", "))))?.eirp_dbm
        }
        (None, Some(EirpParam::Every(v))) => [Some(*v); 3],
        (None, Some(EirpParam::PerBand(m))) => {
            let mut out = [None; 3];
            for (k, v) in m {
                let band = match k {
                    BandName::G2_4 => Band::G2_4,
                    BandName::G5 => Band::G5,
                    BandName::G6 => Band::G6,
                    BandName::All => return Err(bad(CMD, "`eirp_dbm` keys are \"2.4\", \"5\" and \"6\"")),
                };
                if let Some(slot) = out.get_mut(band.index()) {
                    *slot = Some(*v);
                }
            }
            out
        }
        (None, None) => [Some(DEFAULT_EIRP_DBM); 3],
    };
    if eirp.iter().flatten().any(|v| !v.is_finite() || !(-30.0..=60.0).contains(v)) {
        return Err(bad(CMD, "an emitter's EIRP must be -30 to 60 dBm"));
    }
    if cap {
        for (v, limit) in eirp.iter_mut().zip(EIRP_CAP_DBM) {
            *v = v.map(|v| v.min(limit));
        }
    }
    let name = p.name.clone().filter(|n| !n.trim().is_empty() && n.chars().count() <= MAX_NAME).unwrap_or_else(|| format!("AP{}", i + 1));
    Ok(Emitter { name, at, height, eirp_dbm: eirp })
}

/// Which bodies make walls.
#[derive(Clone, Debug, Default, Hash)]
struct Scope {
    bodies: Vec<String>,
    components: Vec<String>,
    exclude: Vec<String>,
    include_hidden: bool,
    rf_materials: bool,
}

/// The names of `component` and every component above it.
fn component_names(s: &Session, mut component: u64) -> Vec<&str> {
    let mut out = Vec::new();
    for _ in 0..256 {
        if component == 0 {
            break;
        }
        let Some(c) = s.doc.components.iter().find(|c| c.id == component) else { break };
        out.push(c.name.as_str());
        component = c.parent;
    }
    out
}

/// The loss a body's material gives it: the design material's `rf`, or a built-in material's
/// default; `None` when it has neither.
fn material_loss(s: &Session, body: &ModelBody) -> Option<(String, RfLoss)> {
    let name = s.doc.body_material(body.definition_name());
    let own = s.doc.user_materials.iter().find(|m| m.name == name).map(|m| m.rf_loss_db.filter(RfLoss::is_valid));
    let loss = match own {
        Some(loss) => loss,
        None => builtin_rf_loss(&name),
    }?;
    Some((name, loss))
}

/// Whether the scope takes `body`.
fn takes(s: &Session, scope: &Scope, body: &ModelBody) -> bool {
    let names = [body.name.as_str(), body.definition_name()];
    let components = component_names(s, body.component);
    let named = |list: &[String]| list.iter().any(|n| names.contains(&n.as_str()) || components.contains(&n.as_str()));
    if named(&scope.exclude) {
        return false;
    }
    if !scope.bodies.is_empty() || !scope.components.is_empty() {
        let by_body = scope.bodies.iter().any(|n| names.contains(&n.as_str()));
        let by_component = scope.components.iter().any(|n| components.contains(&n.as_str()));
        let by_material = scope.rf_materials && material_loss(s, body).is_some();
        if !by_body && !by_component && !by_material {
            return false;
        }
    }
    s.body_visible(&body.name) || scope.include_hidden || material_loss(s, body).is_some()
}

/// What a slice depends on besides the model: the height, the bodies taken and their materials.
fn cache_key(s: &Session, slice: f64, scope: &Scope) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    slice.to_bits().hash(&mut h);
    scope.hash(&mut h);
    for (body, material) in &s.doc.materials {
        (body, material).hash(&mut h);
    }
    for m in &s.doc.user_materials {
        m.name.hash(&mut h);
        m.rf_loss_db.map(|l| l.0.map(f64::to_bits)).hash(&mut h);
    }
    // Hidden bodies leave the walls; the visibility is part of what was sliced.
    serde_json::to_string(&s.visibility).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// The walls at `slice`, from the cache when the model and the settings are the ones it holds.
fn walls_for(s: &Session, world: &Arc<ModelState>, slice: f64, scope: &Scope) -> Result<(Arc<WallMap>, Arc<WallReport>, bool)> {
    let key = cache_key(s, slice, scope);
    if let Ok(cache) = s.rf_walls.lock()
        && let Some(c) = cache.as_ref()
        && Arc::ptr_eq(&c.world, world)
        && c.key == key
    {
        return Ok((c.walls.clone(), c.report.clone(), true));
    }
    let (walls, report) = slice_walls(s, world, slice, scope)?;
    let (walls, report) = (Arc::new(walls), Arc::new(report));
    if let Ok(mut cache) = s.rf_walls.lock() {
        *cache = Some(WallCache { world: world.clone(), key, walls: walls.clone(), report: report.clone() });
    }
    Ok((walls, report, false))
}

/// Cut every body the scope takes at `slice` and tag each region with its loss.
fn slice_walls(s: &Session, world: &ModelState, slice: f64, scope: &Scope) -> Result<(WallMap, WallReport)> {
    let plane = Plane { origin: Vec3::new(0.0, 0.0, slice), ..Plane::XY };
    let mut materials: Vec<(String, RfLoss)> = Vec::new();
    let index = |name: String, loss: RfLoss, materials: &mut Vec<(String, RfLoss)>| -> usize {
        match materials.iter().position(|(n, l)| *n == name && *l == loss) {
            Some(i) => i,
            None => {
                materials.push((name, loss));
                materials.len() - 1
            }
        }
    };
    let mut regions = Vec::new();
    let mut report = WallReport::default();
    let mut edges = 0usize;
    for body in world.bodies.iter().filter(|b| takes(s, scope, b)) {
        if report.bodies >= MAX_BODIES {
            return Err(bad(CMD, format!("more than {MAX_BODIES} bodies cross the slice: name the walls with `bodies` or `components`")));
        }
        let cut = match cut_body(body, &plane, slice) {
            Ok(Some(cut)) => cut,
            Ok(None) => continue,
            Err(why) => {
                if report.skipped.len() < 100 {
                    report.skipped.push(format!("{}: {why}", body.name));
                }
                continue;
            }
        };
        report.bodies += 1;
        let own = material_loss(s, body);
        for rings in cut {
            edges += rings.iter().map(Vec::len).sum::<usize>();
            if edges > crate::rf_coverage::MAX_EDGES {
                return Err(bad(CMD, "the slice has too many wall edges: name the walls with `bodies` or `components`"));
            }
            let region = match &own {
                Some((name, loss)) => WallRegion::new(rings, index(name.clone(), *loss, &mut materials), body.name.clone()),
                None => {
                    let (thin, thin_loss) = heuristic_loss(0.0);
                    let (thick, thick_loss) = heuristic_loss(CONCRETE_FROM_MM);
                    let thin = index(format!("{thin} (guessed)"), thin_loss, &mut materials);
                    let thick = index(format!("{thick} (guessed)"), thick_loss, &mut materials);
                    WallRegion::new(rings, thin, body.name.clone()).map(|r| r.guessed(thin, thick, CONCRETE_FROM_MM))
                }
            };
            if let Some(r) = region {
                regions.push(r);
            }
        }
    }
    Ok((WallMap::new(regions, materials), report))
}

/// `region` followed by straight sides, as rings of world x, y.
fn rings_of(region: &Region2) -> Vec<Vec<[f64; 2]>> {
    region
        .loops()
        .map(|l| {
            let mut points: Vec<[f64; 2]> = l.polyline(FLATTEN_MM).into_vec().into_iter().map(|p| [p.x, p.y]).collect();
            points.dedup();
            if points.len() > 1 && points.first() == points.last() {
                points.pop();
            }
            points
        })
        .collect()
}

/// The regions the horizontal plane at `slice` cuts out of `body`, each as rings (an outline and
/// its holes); `None` when the body doesn't cross the plane.
fn cut_body(body: &ModelBody, plane: &Plane, slice: f64) -> std::result::Result<Option<Vec<Vec<Vec<[f64; 2]>>>>, String> {
    if body.body.is_mesh() {
        return Ok(mesh_cut(body, slice));
    }
    let bounds = body.body.exact_bounds().map_err(|e| e.to_string())?;
    let tol = body.body.planar_edges_tolerance();
    if bounds.min.z >= slice - tol || bounds.max.z <= slice + tol {
        return Ok(None);
    }
    // A proved prism along Z: its section is the cut, no boolean needed.
    if let Ok(Some(prism)) = solidcraft_kernel::certified_prism_section(&body.body, plane)
        && prism.lo < -tol
        && prism.hi > tol
    {
        return Ok(Some(vec![rings_of(&prism.region)]));
    }
    // The kernel's exact check can refuse a cut that lands on a numerical edge case; a cut a
    // fraction of a millimetre away gives the same walls.
    let mut first_error = None;
    for nudge in NUDGES_MM {
        let at = Plane { origin: Vec3::new(0.0, 0.0, slice + nudge), ..Plane::XY };
        if bounds.min.z >= slice + nudge - tol || bounds.max.z <= slice + nudge + tol {
            continue;
        }
        match cut_at(body, &at) {
            Ok(cut) => return Ok(cut),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| "it couldn't be cut".into()))
}

/// How far from the slice height a body is cut again when the cut at the height is refused (mm).
const NUDGES_MM: [f64; 5] = [0.0, 0.37, -0.37, 1.3, -1.3];

/// The cut faces of `body` on `plane`, by the half-space cut a section view uses.
fn cut_at(body: &ModelBody, plane: &Plane) -> std::result::Result<Option<Vec<Vec<Vec<[f64; 2]>>>>, String> {
    let Some(kept) = body.section(plane).map_err(|e| e.to_string())? else { return Ok(None) };
    let Some(telling) = crate::drawing_view::hatch::cut_faces(&body.body, &kept.body, plane) else { return Ok(None) };
    let faces = crate::drawing_view::hatch::faces_on(&kept.body, telling, plane, plane);
    if faces.is_empty() {
        return Err("its cut faces couldn't be read".into());
    }
    Ok(Some(faces.iter().map(rings_of).collect()))
}

/// A mesh body's cut at `slice`, by its own triangles: each triangle that crosses the plane gives
/// a segment between two of its edges, and segments sharing an edge join into rings. Open chains
/// (a mesh with holes) are left out. One region of every ring, inside by the even-odd rule.
fn mesh_cut(body: &ModelBody, slice: f64) -> Option<Vec<Vec<Vec<[f64; 2]>>>> {
    let (points, triangles) = body.body.triangle_mesh()?;
    let above = |i: u32| points.get(i as usize).map(|p| p.z > slice);
    let crossing = |a: u32, b: u32| -> Option<[f64; 2]> {
        let (pa, pb) = (points.get(a.min(b) as usize)?, points.get(a.max(b) as usize)?);
        let t = (slice - pa.z) / (pb.z - pa.z);
        Some([pa.x + (pb.x - pa.x) * t, pa.y + (pb.y - pa.y) * t])
    };
    // Each segment joins two edges, keyed by their vertices (lower first).
    let mut links: HashMap<(u32, u32), Vec<(u32, u32)>> = HashMap::new();
    let mut spots: HashMap<(u32, u32), [f64; 2]> = HashMap::new();
    for t in triangles {
        let mut ends = Vec::with_capacity(2);
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if above(a)? != above(b)? {
                let key = (a.min(b), a.max(b));
                spots.entry(key).or_insert(crossing(a, b)?);
                ends.push(key);
            }
        }
        if let [a, b] = ends.as_slice() {
            links.entry(*a).or_default().push(*b);
            links.entry(*b).or_default().push(*a);
        }
    }
    let mut used: HashMap<(u32, u32), bool> = HashMap::new();
    let mut rings = Vec::new();
    let mut starts: Vec<(u32, u32)> = links.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        if used.get(&start).copied().unwrap_or(false) {
            continue;
        }
        let mut ring = Vec::new();
        let (mut at, mut previous) = (start, None);
        let closed = loop {
            used.insert(at, true);
            ring.push(*spots.get(&at)?);
            let next = links.get(&at)?.iter().copied().find(|n| Some(*n) != previous && !used.get(n).copied().unwrap_or(false));
            match next {
                Some(n) => {
                    previous = Some(at);
                    at = n;
                }
                None => break links.get(&at)?.contains(&start) && ring.len() >= 3,
            }
        };
        if closed {
            rings.push(ring);
        }
    }
    (!rings.is_empty()).then_some(vec![rings])
}

/// The floor `r` gives: its regions and which kind it is.
fn region(world: &ModelState, r: &crate::schema::rf_coverage::RegionParam) -> Result<(Vec<WallRegion>, String)> {
    match (&r.polygon, &r.body) {
        (Some(polygon), None) => {
            if polygon.len() < 3 || polygon.len() > 100_000 || polygon.iter().flatten().any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
                return Err(bad(CMD, "`region.polygon` needs 3 to 100000 finite [x, y] points in mm"));
            }
            let r = WallRegion::new(vec![polygon.clone()], 0, "region".into()).ok_or_else(|| bad(CMD, "`region.polygon` is empty"))?;
            Ok((vec![r], "polygon".into()))
        }
        (None, Some(name)) => {
            let body = world
                .bodies
                .iter()
                .find(|b| b.name == *name || b.definition_name() == name)
                .ok_or_else(|| bad(CMD, format!("no body `{name}` for `region.body`")))?;
            let bounds = if body.body.is_mesh() {
                let (points, _) = body.body.triangle_mesh().ok_or_else(|| bad(CMD, "the region's mesh body has no triangles"))?;
                let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.z), hi.max(p.z)));
                (lo, hi)
            } else {
                let b = body.body.exact_bounds().map_err(|e| bad(CMD, e.to_string()))?;
                (b.min.z, b.max.z)
            };
            let mid = 0.5 * (bounds.0 + bounds.1);
            let plane = Plane { origin: Vec3::new(0.0, 0.0, mid), ..Plane::XY };
            let cut = cut_body(body, &plane, mid).map_err(|why| bad(CMD, format!("`{name}` couldn't be cut: {why}")))?.unwrap_or_default();
            let regions: Vec<WallRegion> = cut.into_iter().filter_map(|rings| WallRegion::new(rings, 0, name.clone())).collect();
            if regions.is_empty() {
                return Err(bad(CMD, format!("`{name}` has no outline from above")));
            }
            Ok((regions, "body".into()))
        }
        _ => Err(bad(CMD, "`region` takes `polygon` or `body`")),
    }
}

/// A wall outline's colour in the PNG, by its preset: glazing blue, metal red, wood brown,
/// drywall grey, concrete and anything else near black.
fn outline_colour(loss: Option<&RfLoss>, guessed: bool) -> [u8; 4] {
    if guessed {
        return [60, 66, 74, 255];
    }
    match loss.and_then(solidcraft_doc::rf::rf_preset_of).map(|p| p.id) {
        Some("low-e-glass") => [40, 120, 210, 255],
        Some("interior-glass") => [120, 180, 230, 255],
        Some("metal") => [200, 60, 60, 255],
        Some("wood-door") => [160, 110, 60, 255],
        Some("drywall") => [130, 136, 144, 255],
        _ => [30, 34, 40, 255],
    }
}

/// Write the heatmap seen from above as a PNG at `path`, north up, with the emitters marked;
/// the path written.
fn write_png(path: &str, picture: &[u8], grid: &Grid, emitters: &[Emitter], walls: &WallMap) -> Result<String> {
    if !path.to_ascii_lowercase().ends_with(".png") || path.chars().count() > 4096 {
        return Err(bad(CMD, "`png` must be a path ending in .png"));
    }
    let scale = (1200 / grid.columns.max(grid.rows).max(1)).clamp(1, 8);
    let (w, h) = (grid.columns * scale, grid.rows * scale);
    let background = [255u8, 255, 255, 255];
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        // The image's first row is the grid's highest (north up).
        let row = grid.rows - 1 - y / scale;
        for x in 0..w {
            let cell = row * grid.columns + x / scale;
            let src = picture.get(cell * 4..cell * 4 + 4).unwrap_or(&[0, 0, 0, 0]);
            let a = f64::from(*src.get(3).unwrap_or(&0)) / 255.0;
            let blend =
                |i: usize| (f64::from(*src.get(i).unwrap_or(&0)) * a + f64::from(*background.get(i).unwrap_or(&255)) * (1.0 - a)).round() as u8;
            if let Some(px) = out.get_mut((y * w + x) * 4..(y * w + x) * 4 + 4) {
                px.copy_from_slice(&[blend(0), blend(1), blend(2), 255]);
            }
        }
    }
    // Each wall's outline, in its material's colour, so a pane thinner than a cell shows.
    let to_px = |p: [f64; 2]| [(p[0] - grid.origin[0]) / grid.step * scale as f64, h as f64 - (p[1] - grid.origin[1]) / grid.step * scale as f64];
    for r in &walls.regions {
        let c = outline_colour(walls.materials.get(r.material).map(|(_, l)| l), r.thick.is_some());
        for ring in &r.rings {
            for (i, a) in ring.iter().enumerate() {
                let Some(b) = ring.get((i + 1) % ring.len()) else { continue };
                let (pa, pb) = (to_px(*a), to_px(*b));
                let n = ((pb[0] - pa[0]).abs().max((pb[1] - pa[1]).abs()) * 2.0).ceil().clamp(1.0, 1.0e5) as usize;
                for k in 0..=n {
                    let t = k as f64 / n as f64;
                    let (x, y) = (pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t);
                    for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                        let (px, py) = ((x - 0.5 + dx).floor(), (y - 0.5 + dy).floor());
                        if px < 0.0 || py < 0.0 || px >= w as f64 || py >= h as f64 {
                            continue;
                        }
                        let at = (py as usize * w + px as usize) * 4;
                        if let Some(dst) = out.get_mut(at..at + 4) {
                            dst.copy_from_slice(&c);
                        }
                    }
                }
            }
        }
    }
    // Each emitter: a white disc ringed in blue.
    let radius = (scale * 3).max(6) as f64;
    for e in emitters {
        let cx = (e.at[0] - grid.origin[0]) / grid.step * scale as f64;
        let cy = h as f64 - (e.at[1] - grid.origin[1]) / grid.step * scale as f64;
        let (x0, x1) = ((cx - radius).floor().max(0.0) as usize, ((cx + radius).ceil().max(0.0) as usize).min(w));
        let (y0, y1) = ((cy - radius).floor().max(0.0) as usize, ((cy + radius).ceil().max(0.0) as usize).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                let c = if d <= radius * 0.6 {
                    [255, 255, 255, 255]
                } else if d <= radius {
                    [11, 107, 203, 255]
                } else {
                    continue;
                };
                if let Some(px) = out.get_mut((y * w + x) * 4..(y * w + x) * 4 + 4) {
                    px.copy_from_slice(&c);
                }
            }
        }
    }
    let (w32, h32) =
        (u32::try_from(w).map_err(|_| bad(CMD, "the picture is too large"))?, u32::try_from(h).map_err(|_| bad(CMD, "the picture is too large"))?);
    let mut bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut bytes, w32, h32);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut writer = e.write_header().map_err(|e| bad(CMD, e.to_string()))?;
        writer.write_image_data(&out).map_err(|e| bad(CMD, e.to_string()))?;
    }
    solidcraft_io::vfs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(path.to_string())
}

#[cfg(test)]
#[path = "rf_coverage_tests.rs"]
mod tests;
