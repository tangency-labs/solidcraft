//! Analysis: what happens to the model in heat, and in moving air.
//!
//! These commands read the model and report numbers; none of them changes a design, so none
//! takes an undo step. The mathematics is in `solidcraft-sim`, which knows nothing about
//! documents; this module's work is turning someone's picks into a study and the answer back
//! into something to read.
//!
//! Temperatures are given and reported in degrees Celsius, because that is what people think
//! in, and in kelvin beside them, because that is what the physics is in. Either may be given:
//! `fixed_c` or `fixed_k`.

use serde_json::{Map, Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Mesh, mesh_distance::TriangleSet};
use solidcraft_sim::elastic::{self, ElasticMaterial, Load, Support};
use solidcraft_sim::flow::{Collision, Flow, FlowStudy, MAX_LATTICE_SPEED, MIN_LATTICE_SPEED, SMAGORINSKY, Scaling, Side, Wall};
use solidcraft_sim::grid::{DEFAULT_SPAN, Grid3, MAX_SPAN, Occupancy, SurfaceFace};
use solidcraft_sim::library::{self, FLUIDS, NOMINAL_SOLIDS};
use solidcraft_sim::material::{self, Alignment, SolidProperties};
use solidcraft_sim::tag::FaceTags;
use solidcraft_sim::thermal::{self, Condition, Film, Limits, Patch, SolidMaterial, Study, Time};

use solidcraft_doc::analysis::{Study as SavedStudy, StudyKind as SavedKind};

use super::CommandSpec;
use crate::params::{bad, num, str_, vec3};
use crate::{AnalysisFace, AnalysisField, AnalysisLattice, AnalysisResult, AnalysisView, Result, Session};

/// Most bodies one study may take in.
const MAX_BODIES: usize = 64;
/// Most boundary conditions one study may carry.
const MAX_CONDITIONS: usize = 64;
/// Most face picks one condition may name.
const MAX_PICKS: usize = 4096;
/// Fewest cells across the model one study may ask for. Below this the lattice cannot hold a
/// shape well enough for the answer to mean anything.
const MIN_SPAN: usize = 4;
/// Most hot or cold spots one answer reports.
const MAX_SPOTS: usize = 16;
/// How far apart, in cells, two reported spots have to be, so one hot place is not listed as
/// its own eight neighbours.
const SPOT_SEPARATION: f64 = 2.0;
/// Absolute zero in degrees Celsius.
const ZERO_C: f64 = 273.15;

/// The highest safety factor a picture shows.
///
/// A corner carrying no load has an unbounded one, and a scale stretched to it would paint the
/// whole part the same colour. Ten and over is "nowhere near giving way", which is all the
/// number is being asked here; the study reports the real least safety factor, unheld.
const SAFETY_SHOWN_AT_MOST: f64 = 10.0;
/// Cells across the model a flow study uses when it is not told. Coarser than a thermal
/// study's, because a flow takes thousands of steps where a conduction takes one solve, and
/// because the tunnel around it is several times the model again.
const FLOW_SPAN: usize = 16;
/// How much room a flow study leaves around the model, as a multiple of the model's own span:
/// the wind tunnel it sits in.
///
/// One and a half is not generous -- a wind tunnel wants several body lengths downstream -- but
/// it is enough that the tunnel's own walls and its outlet do not crowd the wake. Below about
/// one the wake reaches the outlet while it is still strong, reflects off the held pressure
/// there and feeds back into the body, and the force grows without ever settling.
const FLOW_PADDING: f64 = 1.5;
/// Steps a flow study takes when it is not told.
const FLOW_STEPS: u32 = 4000;
/// Most cell-steps one flow study may ask for. A cell-step is one cell advanced once, which is
/// what the cost of a flow actually follows; this is a few minutes' work.
const MAX_CELL_STEPS: f64 = 6.0e9;

pub static COMMANDS: &[CommandSpec] = &[
    // On the SIMULATION tab, and answering exactly the same from the CLI, the MCP server, the
    // control channel and scripts.
    CommandSpec::new("sim.materials", "Analysis Materials", materials).icon("sim_materials").noundo().doc_only().params(
        "→ the nominal thermal properties of each built-in material (conductivity, specific heat, emissivity, expansion) \
             and the fluids a flow or a film can run in, with their viscosity and Prandtl number. These are ordinary \
             room-temperature figures for a representative grade, not data: give `properties` or a material card for anything \
             that matters",
    ),
    CommandSpec::new("sim.lattice", "Analysis Lattice", lattice).icon("sim_lattice").noundo().params(
        "bodies?: [name] (default: every body); cells?: 4…2048 across the longest side (default 48), moved by a few per cent where that makes the part a whole number of cells across: a lattice that does not divide a part square to the axes overhangs it, and the overhang is solved as material -- worth up to 42% of the answer at any resolution, and not reduced by refining (docs/analysis-verification.md). → the lattice a study \
         would run on: its cell count and spacing, how many cells the model fills, the volume that comes to against the \
         model's own, and the faces of the model a condition could be put on. Run this before a study to see what it will cost",
    ),
    CommandSpec::new("sim.flow", "Flow Study", flow_study).icon("sim_flow").noundo().params(
        "speed_m_s: how fast the air comes at the model; bodies?: [name] (default: every body); direction?: [x,y,z] the flow \
         comes from behind, so the model is pushed along it (default [1,0,0]); fluid?: Air or Water (sim.materials; default \
         Air); properties?: {density_kg_m3?, dynamic_viscosity_pa_s?, conductivity_w_m_k?, specific_heat_j_kg_k?} \
         giving the fluid's own numbers instead of the named one's -- what a flow turns on is the kinematic viscosity, \
         and air at 200 °C is nothing like air at 20 °C; cells?: 4…2048 across the model (default 16) and fitted to the model as sim.lattice describes; padding?: room around the model as a multiple of its span \
         (default 1.5); lattice_speed?: 0…0.1, how fast the flow runs on the lattice -- a smaller number costs \
         more steps and is more accurate only down to about 0.01; below that it is less accurate, because f32 round-off \
         in the force outgrows the compressibility error a slower flow buys \
         (default 0.05); wall?: where a wall is taken to be -- model (default: each link from the air into \
         the model is cut where the surface really crosses it, so a face the lattice does not land on is held where it \
         is rather than half a cell out) or lattice (half a cell out, whatever the model says); collision?: how the \
         distributions relax -- regularized (default: two rates          with the lattice's own even moments thrown away each step, which holds a lower relaxation time than trt and puts          the walls in the same place), trt (two rates), bgk (one rate, and the walls move with the viscosity) or          projected (all nine of the lattice's own moments thrown away: the lowest relaxation time here by far, and it          gives the walls up, so not for anything measured at a surface); turbulence?: the Smagorinsky constant of the subgrid model, \
         0.1 to 0.2, or 0 to run laminar (default 0.16 -- without it only a flow slow enough for a cell to carry almost no \
         inertia can be run at all); steps?: (default 4000); settle?: stop once the flow stops changing by this much \
         (default 1e-6); show?: false leaves the answer undrawn (default: the pressure is shaded on the model and \n         arrows show where the air goes). → the force on the model in newtons with its drag coefficient, the pressure range, the fastest air \
         and where it is, the Reynolds number, and whether this lattice actually resolves this flow",
    ),
    CommandSpec::new("sim.stress", "Stress Study", stress_study).icon("sim_stress").noundo().params(
        "loads: [{name?, and one place: faces?: [{body, face} or [x,y,z]] | side?: \"+x\"…\"-z\" | rest?: true, and one of: \
         fixed?: true (held in every direction) | roller?: true (held across the face, free along it: a plane of symmetry) | \
         pressure_pa?: positive pushes in | traction_pa?: [x,y,z] | free?: true}]; bodies?: [name]; cells?: 4…2048 across the longest side (default 48), moved by a few per cent where that makes the part a whole number of cells across: a lattice that does not divide a part square to the axes overhangs it, and the overhang is solved as material -- worth up to 42% of the answer at any resolution, and not reduced by refining (docs/analysis-verification.md); material?/properties?: as sim.thermal, with properties taking young_modulus_pa, \
         poisson_ratio, expansion_per_k?, density_kg_m3?, yield_strength_pa?; gravity?: true for 9.80665 m/s² down, or \
         [x,y,z]; fit_surface?: true pulls the lattice's boundary nodes onto the model and solves on elements          that follow it, rather than on the staircase a lattice otherwise holds a curved wall as. Worth most where          the over-read is the roughness of a whole surface -- a round bar pulled along its axis goes from 27.8% over          to 17.3% -- and much less round a hole, where the peak is a concentration and it is worth under a per cent          (docs/analysis-verification.md). Refused together with gravity, a          temperature rise, or a load on a face that moves, since those are still worked out as if every cell were a          box; settled?: true solves the same case again on a lattice three quarters as fine and reports how far the          worst stress moved, which is the check every textbook asks for by hand -- a figure that moves a lot between          two lattices is telling you about the lattice as much as about the part, and on a curved wall it does;          least_fill?: 0.02…0.5 (default 0.5) how much of a cell the model must fill for that cell to be solved \
         for -- lowering it keeps the thin cells a wall leaves, which is worth nothing on its own and is worth about \
         four per cent of a stress concentration together with `fit_surface`, at the cost of reading high on a part \
         whose stress is uniform (docs/analysis-verification.md); temperature_rise_k?: a uniform rise above the temperature the part is unstressed at; heat?: a sim.thermal \
         study (conditions, ambient, generation, time) plus reference_c, run on the same lattice so the part is loaded by \
         its own warmth; tolerance?, max_iterations?; show?: false leaves the answer undrawn (default: the model is \n         shaded by it). → the worst von Mises stress and where, the least safety factor \
         against yield and where, how far the part moved, and whether the load and the reactions balance",
    ),
    CommandSpec::new("sim.thermal", "Thermal Study", thermal_study).icon("sim_thermal").noundo().params(
        "bodies?: [name] (default: every body); cells?: 4…2048 across the longest side (default 48), moved by a few per cent where that makes the part a whole number of cells across: a lattice that does not divide a part square to the axes overhangs it, and the overhang is solved as material -- worth up to 42% of the answer at any resolution, and not reduced by refining (docs/analysis-verification.md); \
         material?: name for every body, or {body: name}; properties?: {body: {conductivity_w_m_k: number or [x,y,z], \
         density_kg_m3?, specific_heat_j_kg_k?}} overriding the material; generation?: {body: {watts?} or {w_m3?}}; \
         conditions: [{name?, and one place: faces?: [{body, face} or [x,y,z]] | side?: \"+x\"…\"-z\" | rest?: true, \
         and one or more of: fixed_c?/fixed_k?, flux_w_m2?, watts?, convection?: {w_m2_k, c?/k?}, \
         radiation?: {emissivity, c?/k?}, insulated?: true}]; ambient?: {c?/k?, convection?: w_m2_k, emissivity?} for the \
         surface no condition claims (default: insulated); time?: {seconds, from_c?/from_k?} for one step of a transient \
         (default: the settled answer); tolerance?, max_iterations?; show?: false leaves the answer undrawn (default: \n         the model is shaded by it). → the temperature range and where it is worst, the \
         watts across each patch, the energy balance, and what the answer rests on",
    ),
    // A study the design keeps. The three above answer once and are gone; these hold the
    // question, so changing one number and asking again does not mean typing it all out.
    CommandSpec::new("sim.study.create", "New Study", study_create).icon("sim_study").params(
        "kind: thermal | flow | stress; name?: (default: the kind and the next free number); \
         setup?: the parameters that kind's own command takes (`sim.thermal`, `sim.flow`, \
         `sim.stress`), saved as given and checked when it runs. → the study's name",
    ),
    CommandSpec::new("sim.study.set", "Edit Study", study_set)
        .icon("settings")
        .params("study: name; setup: the parameters its command takes, replacing what it held"),
    CommandSpec::new("sim.study.delete", "Delete Study", study_delete).icon("delete").params("study: name"),
    CommandSpec::new("sim.study.list", "Studies", study_list).icon("list").noundo().params(
        "→ every study the design holds: its kind, the command it runs, its setup, and what it \
         still needs node by node -- each one complete, optional, incomplete (which stops a run) \
         or error -- worked out the way the solver works it out, so the list and the run agree",
    ),
    CommandSpec::new("sim.study.run", "Run Study", study_run)
        .icon("sim_study")
        .noundo()
        .params("study: name. Runs that study's own command on its saved setup. → the study, its kind and the answer"),

    // Looking at an answer that has come back. None of these runs a solver again: a run keeps
    // every field it worked out on the lattice it solved on, so another quantity, a narrower
    // scale, a cut through the part or a threshold is another reading of the same answer.
    CommandSpec::new("sim.result.show", "Result View", result_show).icon("iso_analysis").noundo().params(
        "field?: the name of one of the fields the run worked out, or its number from one (a prefix will do while it \
         picks out one); low?/high?: the two ends of the scale, or range?: false to stretch it back over the whole \
         field; bands?: 2…64 steps, or false for a smooth wash; cut?: {at?: [x,y,z] (default: the middle of the \
         lattice), normal?: [x,y,z] (default [0,1,0])} takes away the side the normal points at and shades the cut \
         itself, or false to put the model back; iso?: a value to show only the part past it towards the bad end, or \
         false for all of it; streamlines?: true follows the flow in lines rather than arrows. → what is now shaded, \
         over what range, and how many squares, arrows and lines reached the picture",
    ),
    CommandSpec::new("sim.result.probe", "Probe Result", result_probe).icon("measure").noundo().params(
        "at: [x,y,z] a point to read the shown field at, which stays marked, or clear?: true to drop the points \
         already marked. → the value in the cell the point falls in, null where the field has none there, and every \
         point now marked",
    ),
    CommandSpec::new("sim.result.stats", "Result Statistics", result_stats).icon("list").noundo().params(
        "field?: name or number (default: the one being shaded); faces?: [{body?, face} or [x,y,z]] to add it up over \
         those faces of the model as well. → every field the run worked out, and for the chosen one the lowest, \
         highest and mean: `volume` over every cell it has a value in (the model for heat or strength, the whole \
         tunnel for a flow, whose field is the air), `surface` over the model's own surface, `picked` over the \
         faces named together and `faces` over each of them, with the count behind every figure",
    ),
];

/// A physical quantity that may be far larger than an ordinary parameter.
///
/// [`crate::params::num`] caps what it accepts at a thousand million, which is right for the
/// millimetres, degrees and counts it was written for and useless here: a Young's modulus is two
/// hundred thousand million pascals and a heat flux can be larger still. The cap is what keeps
/// a hostile number out, so this keeps one -- just a far higher one, past anything physical.
fn big(p: &Value, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() < 1e24)
}

/// Three of them.
fn big_vec3(v: Option<&Value>) -> Option<Vec3> {
    let a = v?.as_array()?;
    let [x, y, z] = a.as_slice() else { return None };
    let one = |v: &Value| v.as_f64().filter(|v| v.is_finite() && v.abs() < 1e24);
    Some(Vec3::new(one(x)?, one(y)?, one(z)?))
}

fn materials(_s: &mut Session, _p: &Value) -> Result<Value> {
    let solids: Vec<Value> = NOMINAL_SOLIDS
        .iter()
        .map(|m| {
            json!({
                "material": m.name,
                "conductivity_w_m_k": m.conductivity_w_m_k,
                "specific_heat_j_kg_k": m.specific_heat_j_kg_k,
                "emissivity": m.emissivity,
                "expansion_per_k": m.expansion_per_k,
                "orthotropic": m.conductivity_w_m_k.iter().any(|k| (k - m.conductivity_w_m_k[0]).abs() > 1e-12),
            })
        })
        .collect();
    let fluids: Vec<Value> = FLUIDS
        .iter()
        .map(|f| {
            json!({
                "fluid": f.name,
                "density_kg_m3": f.density_kg_m3,
                "dynamic_viscosity_pa_s": f.dynamic_viscosity_pa_s,
                "kinematic_viscosity_m2_s": f.kinematic_viscosity_m2_s(),
                "conductivity_w_m_k": f.conductivity_w_m_k,
                "specific_heat_j_kg_k": f.specific_heat_j_kg_k,
                "prandtl": f.prandtl(),
                "at_c": f.at_kelvin - ZERO_C,
            })
        })
        .collect();
    Ok(json!({
        "materials": solids,
        "fluids": fluids,
        "note": "nominal room-temperature figures for a representative grade, for a first look; give `properties` or a \
                 material card for anything that matters",
    }))
}

fn lattice(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.lattice";
    let picked = bodies(s, cmd, p)?;
    let meshes: Vec<&Mesh> = picked.iter().map(|b| b.mesh.as_ref()).collect();
    let (grid, occ) = build(cmd, p, &meshes)?;
    let tags = FaceTags::of(&occ, &meshes);
    let (model_volume, open) = model_volume_of(&picked);
    let patches: Vec<Value> = tags
        .patches()
        .iter()
        .map(|patch| {
            let body = picked.get(patch.part as usize).map(|b| b.name.clone()).unwrap_or_default();
            json!({
                "body": body,
                "face": patch.face,
                "cells": patch.faces.len(),
                "area_mm2": patch.faces.len() as f64 * grid.spacing() * grid.spacing(),
            })
        })
        .collect();
    let mut out = json!({
        "lattice": lattice_json(&grid, &occ, model_volume),
        "faces": patches,
        "unreachable_surface_cells": tags.unclaimed(),
    });
    if !open.is_empty() {
        out["warnings"] = json!(open);
    }
    Ok(out)
}

/// The volume of the bodies a study was given, and a word about each one that has none or whose
/// own is in doubt.
///
/// A surface body has no volume to add: the same triangles come to a different number for every
/// point the sum is taken from, so adding it in would put an arbitrary figure into the denominator
/// of `volume_error` -- which is the one number that says whether the lattice holds the shape, and
/// so whether any of the answers are about the model at all. Left out, that comparison stays about
/// the bodies it can be made about, and the reader is told which were not. A solid whose triangles
/// do not close still counts, since its volume is its own to within the size of the gap, but it
/// says so: a study reporting a two per cent lattice error wants to know that half a per cent of
/// it is the model's own geometry.
fn model_volume_of(picked: &[Picked]) -> (f64, Vec<String>) {
    let (mut volume, mut said) = (0.0, Vec::new());
    for b in picked {
        let m = b.mesh.measure();
        if !b.solid {
            said.push(super::inspect::no_volume(&b.name, m.vector_area.len()));
            continue;
        }
        volume += m.volume;
        if !m.is_closed() {
            said.push(super::inspect::volume_in_doubt(&b.name, m.vector_area.len(), m.vector_area.len() / m.area.max(1.0)));
        }
    }
    (volume, said)
}

fn lattice_json(grid: &Grid3, occ: &Occupancy, model_volume_mm3: f64) -> Value {
    let on_lattice = occ.volume_mm3();
    json!({
        "cells": grid.count(),
        "total_cells": grid.cells(),
        "spacing_mm": grid.spacing(),
        "model_cells": occ.equations(),
        "surface_cells": occ.surface().count(),
        "volume_mm3": on_lattice,
        "model_volume_mm3": model_volume_mm3,
        // How well the lattice holds the shape. Close to zero means the answer is about the
        // model; a long way off means the spacing is too coarse for this shape.
        "volume_error": if model_volume_mm3 > 0.0 { (on_lattice - model_volume_mm3) / model_volume_mm3 } else { 0.0 },
    })
}

/// A body a study takes in, with everything about it the study needs.
struct Picked {
    name: String,
    mesh: std::sync::Arc<Mesh>,
    /// The material the design assigns it, as the library spells it.
    material: Option<String>,
    /// g/cm³, from the design.
    density_g_cm3: f64,
    /// Whether the body is a solid rather than a sheet. Taken from the topology here, because a
    /// mesh cannot tell a surface body from a solid whose triangles merely do not close, and the
    /// two mean different things for a volume: the first has none, the second has one that is
    /// merely uncertain.
    solid: bool,
}

fn bodies(s: &Session, cmd: &str, p: &Value) -> Result<Vec<Picked>> {
    let state = s.world_state()?;
    let wanted = crate::params::string_list_or_default(p, "bodies");
    let mut out = Vec::new();
    for b in &state.bodies {
        if !wanted.is_empty() && !wanted.iter().any(|w| w.eq_ignore_ascii_case(&b.name)) {
            continue;
        }
        let material = s.doc.materials.get(&b.name).and_then(|m| s.doc.material_name(m));
        out.push(Picked {
            name: b.name.clone(),
            mesh: b.mesh(),
            material,
            density_g_cm3: s.doc.density(&b.name),
            solid: b.body.validity().is_empty(),
        });
        if out.len() > MAX_BODIES {
            return Err(bad(cmd, format!("a study may take in {MAX_BODIES} bodies at once")));
        }
    }
    for w in &wanted {
        if !out.iter().any(|b| b.name.eq_ignore_ascii_case(w)) {
            return Err(bad(cmd, format!("no body `{w}`")));
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, "there is no body to analyse"));
    }
    Ok(out)
}

fn build(cmd: &str, p: &Value, meshes: &[&Mesh]) -> Result<(Grid3, Occupancy)> {
    build_with_room(cmd, p, meshes, DEFAULT_SPAN, 1)
}

fn build_with_room(cmd: &str, p: &Value, meshes: &[&Mesh], default_span: usize, pad: usize) -> Result<(Grid3, Occupancy)> {
    let span = match num(p, "cells") {
        Some(n) if n.is_finite() && n >= MIN_SPAN as f64 && n <= MAX_SPAN as f64 => n as usize,
        None => default_span,
        Some(n) => return Err(bad(cmd, format!("`cells` is {n}; it must be {MIN_SPAN} to {MAX_SPAN}"))),
    };
    // Over the model rather than over its box: the spacing is fitted to the part's own flat
    // faces, which is worth more than any other single thing here (`grid::fitted_span`).
    let grid = Grid3::over_parts(meshes, span, pad).map_err(|e| bad(cmd, e.to_string()))?;
    // Four samples along each axis of a cell: enough for the filled fraction to settle, which
    // is what decides the cells on a wall and what the reported volume is read from.
    // Given at all, it has to be a number in range. Asking `num` alone would take a string for
    // an absent value and run the study on a rule nobody chose.
    let least = match p.get("least_fill") {
        None => solidcraft_sim::grid::HALF_FULL,
        Some(v) => match v.as_f64().filter(|v| v.is_finite() && *v >= 0.02 && *v <= 0.5) {
            Some(v) => v,
            None => return Err(bad(cmd, format!("`least_fill` is {v}; it must be a number from 0.02 to 0.5"))),
        },
    };
    let occ = Occupancy::of_keeping(grid.clone(), meshes, 4, least).map_err(|e| bad(cmd, e.to_string()))?;
    Ok((grid, occ))
}

fn thermal_study(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.thermal";
    let picked = bodies(s, cmd, p)?;
    let meshes: Vec<&Mesh> = picked.iter().map(|b| b.mesh.as_ref()).collect();
    let (grid, occ) = build(cmd, p, &meshes)?;
    let tags = FaceTags::of(&occ, &meshes);
    let mut warnings: Vec<String> = Vec::new();

    let properties = read_materials(cmd, &s.doc, p, &picked, Want::Heat, &mut warnings)?;
    let materials = conduction_materials(cmd, &properties, p.get("time").is_some())?;
    let mut study = thermal_from(cmd, p, &picked, &occ, &tags, materials.clone(), &mut warnings)?;
    study.area_scale = tags.area_scale(&occ, &meshes);
    let solution = thermal::solve(&occ, &study).map_err(|e| bad(cmd, e.to_string()))?;

    // What the answer rests on, said plainly rather than left for someone to find out.
    if solution.imbalance > study.limits.tolerance.max(1e-6) * 100.0 {
        warnings.push(format!("the energy does not balance to {:.3e}, which it should; treat the watts with suspicion", solution.imbalance));
    }
    if tags.unclaimed() > 0 {
        warnings.push(format!(
            "{} cells of the surface belong to no face of the model, so a condition set by picking a face does not reach \
             them; they took the ambient condition",
            tags.unclaimed()
        ));
    }
    warnings.extend(misfit_warning(&meshes, &grid, p));
    let staircase = staircase_faces(&tags);
    if staircase > 0 {
        warnings.push(format!(
            "{staircase} of the model's faces are not square to an axis, so the lattice holds them as a staircase. \
             Each one still carries the area the model gives it, so the heat crossing it is right, but where the wall \
             sits is only as accurate as the spacing"
        ));
    }
    let (model_volume, open) = model_volume_of(&picked);
    warnings.extend(open);
    let on_lattice = occ.volume_mm3();
    if model_volume > 0.0 && ((on_lattice - model_volume) / model_volume).abs() > 0.02 {
        warnings.push(format!(
            "the lattice holds {:.1}% of the model's volume; raise `cells` until this is close before trusting the numbers",
            100.0 * on_lattice / model_volume
        ));
    }

    if wants_showing(p) {
        let about =
            format!("{:.0} to {:.0} degrees, {} cells across the model", solution.min_kelvin - ZERO_C, solution.max_kelvin - ZERO_C, grid.count()[0]);
        let worst = spots(&occ, &solution.kelvin, &picked, true).first().and_then(|v| v.get("at").cloned()).and_then(|v| vec3(&v));
        // The wall temperature on the surface, the cell temperature inside. Two measurements,
        // not one: the wall is what the conditions hold, and a cut through the part is what
        // shows how far in that reaches.
        let temperature = field(
            "Temperature",
            "degrees",
            true,
            solution.wall_kelvin.iter().map(|k| (k - ZERO_C) as f32).collect(),
            cell_field(&grid, |cell| occ.eq_of(cell).and_then(|e| solution.kelvin.get(e)).map(|k| k - ZERO_C)),
            worst,
        );
        // Where the heat is actually going, which a temperature by itself does not say: two
        // parts of an assembly can sit at one temperature with all of the heat crossing one.
        let flux = field(
            "Heat flux",
            "W/m\u{b2}",
            true,
            skin_field(&occ, |f| flux_in_cell(&occ, &grid, &solution.kelvin, &materials, f.cell as usize)),
            cell_field(&grid, |cell| flux_in_cell(&occ, &grid, &solution.kelvin, &materials, cell)),
            None,
        );
        show(s, result_of(&occ, &tags, &picked, about, s.revision, vec![temperature, flux]));
    }

    let bodies_json: Vec<Value> = picked
        .iter()
        .zip(&properties)
        .zip(&materials)
        .map(|((b, read), m)| {
            json!({
                "body": b.name,
                "material": b.material.clone().unwrap_or_else(|| "Default".into()),
                "source": read.source(),
                "conductivity_w_m_k": m.conductivity_w_m_k,
                "density_kg_m3": m.density_kg_m3,
                "specific_heat_j_kg_k": m.specific_heat_j_kg_k,
            })
        })
        .collect();

    Ok(json!({
        "lattice": lattice_json(&grid, &occ, model_volume),
        "bodies": bodies_json,
        "temperature": {
            "min_c": solution.min_kelvin - ZERO_C,
            "max_c": solution.max_kelvin - ZERO_C,
            "min_k": solution.min_kelvin,
            "max_k": solution.max_kelvin,
            "rise_k": solution.max_kelvin - solution.min_kelvin,
        },
        "hot_spots": spots(&occ, &solution.kelvin, &picked, true),
        "cold_spots": spots(&occ, &solution.kelvin, &picked, false),
        "patches": solution
            .patches
            .iter()
            .map(|patch| json!({
                "name": patch.name,
                "watts": patch.watts,
                "area_mm2": patch.area_m2 * 1e6,
                "min_c": patch.min_kelvin - ZERO_C,
                "max_c": patch.max_kelvin - ZERO_C,
            }))
            .collect::<Vec<_>>(),
        "balance": {
            "generated_w": solution.generated_w,
            "surface_w": solution.patches.iter().map(|x| x.watts).sum::<f64>(),
            "imbalance": solution.imbalance,
        },
        "solver": {
            "iterations": solution.linear.iterations,
            "residual": solution.linear.residual,
            "tolerance": solution.linear.tolerance,
            "radiation_iterations": solution.outer,
        },
        "warnings": warnings,
    }))
}

/// What a study needs a material to say.
#[derive(Clone, Copy, PartialEq)]
enum Want {
    /// How it carries heat.
    Heat,
    /// How it carries load.
    Strength,
}

/// Where a body's properties came from.
enum Read {
    /// Thermal numbers the call gave outright.
    Heat(SolidMaterial),
    /// Elastic numbers the call gave outright.
    Strength(ElasticMaterial),
    /// A material card, read along the model's axes.
    Card(SolidProperties),
}

impl Read {
    fn source(&self) -> &'static str {
        match self {
            Read::Heat(_) | Read::Strength(_) => "properties",
            Read::Card(_) => "material",
        }
    }
}

fn read_materials(
    cmd: &str,
    doc: &solidcraft_doc::Document,
    p: &Value,
    picked: &[Picked],
    want: Want,
    warnings: &mut Vec<String>,
) -> Result<Vec<Read>> {
    let overrides = p.get("properties").and_then(Value::as_object);
    let by_name = p.get("material").and_then(Value::as_object);
    let for_all = str_(p, "material");
    let mut out = Vec::new();
    let mut nominal_used = Vec::new();

    for body in picked {
        if let Some(given) = overrides.and_then(|o| lookup(o, &body.name)) {
            out.push(match want {
                Want::Heat => Read::Heat(explicit(cmd, &body.name, given)?),
                Want::Strength => Read::Strength(explicit_strength(cmd, &body.name, given)?),
            });
            continue;
        }
        // The material named for this body, then one named for every body, then the design's
        // own assignment.
        let name = by_name
            .and_then(|o| lookup(o, &body.name))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| for_all.map(str::to_string))
            .or_else(|| body.material.clone());
        let Some(name) = name else {
            return Err(bad(
                cmd,
                format!(
                    "`{}` has no material, so there is nothing to say how it conducts heat; assign one (material.assign), \
                     name one for the study (`material`), or give `properties` for it",
                    body.name
                ),
            ));
        };
        // The density goes with the material that was named, not with whatever the body was
        // assigned: a study that says `material: "Steel"` on a body nobody assigned anything to
        // means steel's density too, and taking the document's stand-in instead would weigh the
        // part wrongly without saying so.
        let density = doc.material_density(&name).unwrap_or(body.density_g_cm3) * 1000.0;
        let Some(card) = library::nominal_card(&name, density) else {
            return Err(bad(
                cmd,
                match want {
                    Want::Heat => format!(
                        "`{name}` has no thermal properties here (sim.materials lists the ones that do), so give \
                         `properties` for `{}`: a conductivity in W/(m·K), and for a transient a density and a specific heat",
                        body.name
                    ),
                    Want::Strength => format!(
                        "`{name}` has no mechanical properties here (sim.materials lists the ones that do), so give \
                         `properties` for `{}`: a `young_modulus_pa` and a `poisson_ratio` at least",
                        body.name
                    ),
                },
            ));
        };
        let props = material::read(&card, Alignment::WorldAxes).map_err(|e| bad(cmd, e.to_string()))?;
        if props.illustrative_only && !nominal_used.contains(&name) {
            nominal_used.push(name.clone());
        }
        out.push(Read::Card(props));
    }
    if !nominal_used.is_empty() {
        warnings.push(format!(
            "the properties of {} are nominal figures for a representative grade, not measurements of your material; a \
             stainless steel conducts a third of what a mild steel does, and an alloy's conductivity falls as it warms",
            nominal_used.join(", ")
        ));
    }
    Ok(out)
}

/// The conduction numbers a set of read properties gives.
fn conduction_materials(cmd: &str, properties: &[Read], transient: bool) -> Result<Vec<SolidMaterial>> {
    let mut out = Vec::new();
    for read in properties {
        out.push(match (transient, read) {
            (_, Read::Heat(m)) => *m,
            (_, Read::Strength(_)) => {
                return Err(bad(cmd, "a body was given mechanical properties where thermal ones are needed"));
            }
            (false, Read::Card(props)) => props.for_steady_conduction().map_err(|e| bad(cmd, e.to_string()))?,
            (true, Read::Card(props)) => props.for_transient_conduction().map_err(|e| bad(cmd, e.to_string()))?,
        });
    }
    Ok(out)
}

/// A conduction study built from a parameter object, which is either a `sim.thermal` call's own
/// parameters or the `heat` of a `sim.stress` call. The bodies, the lattice and the materials
/// are settled before this and handed in, so one material choice serves both studies.
fn thermal_from(
    cmd: &str,
    p: &Value,
    picked: &[Picked],
    occ: &Occupancy,
    tags: &FaceTags,
    materials: Vec<SolidMaterial>,
    warnings: &mut Vec<String>,
) -> Result<Study> {
    let mut study = Study::new(materials);
    study.generation_w_m3 = generation(cmd, p, picked, occ)?;
    study.conditions = conditions(cmd, p, picked, tags, warnings)?;
    study.rest = ambient(cmd, p)?;
    study.limits = limits(cmd, p)?;
    study.time = time(cmd, p, occ)?;
    Ok(study)
}

/// A value in a map keyed by body name, matched the way a person would type it.
fn lookup<'a>(map: &'a Map<String, Value>, body: &str) -> Option<&'a Value> {
    map.get(body).or_else(|| map.iter().find(|(k, _)| k.eq_ignore_ascii_case(body)).map(|(_, v)| v))
}

fn explicit(cmd: &str, body: &str, given: &Value) -> Result<SolidMaterial> {
    let k = match given.get("conductivity_w_m_k") {
        Some(Value::Array(_)) => {
            let v = big_vec3(given.get("conductivity_w_m_k"))
                .ok_or_else(|| bad(cmd, format!("`{body}`'s conductivity must be a number or three numbers")))?;
            [v.x, v.y, v.z]
        }
        Some(v) => {
            let one = v
                .as_f64()
                .filter(|x| x.is_finite() && x.abs() < 1e24)
                .ok_or_else(|| bad(cmd, format!("`{body}`'s conductivity must be a number or three numbers")))?;
            [one; 3]
        }
        None => return Err(bad(cmd, format!("`properties` for `{body}` must give a `conductivity_w_m_k`"))),
    };
    for (axis, v) in k.iter().enumerate() {
        if !(*v > 0.0) || !v.is_finite() {
            return Err(bad(cmd, format!("`{body}` conducts {v} W/(m·K) along axis {axis}; it must be above zero")));
        }
    }
    let positive = |key: &str| -> Result<f64> {
        match big(given, key) {
            None => Ok(0.0),
            Some(v) if v >= 0.0 && v.is_finite() => Ok(v),
            Some(v) => Err(bad(cmd, format!("`{body}`'s {key} is {v}; it may not be negative"))),
        }
    };
    Ok(SolidMaterial { conductivity_w_m_k: k, density_kg_m3: positive("density_kg_m3")?, specific_heat_j_kg_k: positive("specific_heat_j_kg_k")? })
}

fn explicit_strength(cmd: &str, body: &str, given: &Value) -> Result<ElasticMaterial> {
    let young_modulus_pa = big(given, "young_modulus_pa")
        .filter(|v| *v > 0.0)
        .ok_or_else(|| bad(cmd, format!("`properties` for `{body}` must give a `young_modulus_pa` above zero")))?;
    let poisson_ratio = num(given, "poisson_ratio")
        .filter(|v| *v > -1.0 && *v < 0.5)
        .ok_or_else(|| bad(cmd, format!("`properties` for `{body}` must give a `poisson_ratio` between −1 and a half")))?;
    let expansion_per_k = match given.get("expansion_per_k") {
        None => [0.0; 3],
        Some(Value::Array(_)) => {
            let v =
                big_vec3(given.get("expansion_per_k")).ok_or_else(|| bad(cmd, format!("`{body}`'s expansion must be a number or three numbers")))?;
            [v.x, v.y, v.z]
        }
        Some(v) => {
            let one = v
                .as_f64()
                .filter(|x| x.is_finite() && x.abs() < 1e24)
                .ok_or_else(|| bad(cmd, format!("`{body}`'s expansion must be a number or three numbers")))?;
            [one; 3]
        }
    };
    let density_kg_m3 = match big(given, "density_kg_m3") {
        None => 0.0,
        Some(v) if v >= 0.0 => v,
        Some(v) => return Err(bad(cmd, format!("`{body}`'s density is {v}; it may not be negative"))),
    };
    let yield_strength_pa = match big(given, "yield_strength_pa") {
        None => None,
        Some(v) if v > 0.0 => Some(v),
        Some(v) => return Err(bad(cmd, format!("`{body}` yields at {v} Pa; that is not a strength"))),
    };
    Ok(ElasticMaterial { young_modulus_pa, poisson_ratio, expansion_per_k, density_kg_m3, yield_strength_pa })
}

fn generation(cmd: &str, p: &Value, picked: &[Picked], occ: &Occupancy) -> Result<Vec<f64>> {
    let Some(map) = p.get("generation").and_then(Value::as_object) else { return Ok(Vec::new()) };
    let volume = occ.grid().cell_volume_m3();
    let mut out = vec![0.0; picked.len()];
    for (index, body) in picked.iter().enumerate() {
        let Some(given) = lookup(map, &body.name) else { continue };
        // A chip dissipates watts, not watts per cubic metre, so watts is what a call gives and
        // the lattice's own volume for that body is what turns it into a density. Using the
        // lattice's volume rather than the model's keeps the total exactly right, whatever the
        // spacing loses at the walls.
        let cells = (0..occ.equations()).filter(|e| occ.cell_of(*e).and_then(|c| occ.part_of(c)) == Some(index as u16)).count();
        let per_m3 = match (big(given, "watts"), big(given, "w_m3")) {
            (Some(_), Some(_)) => return Err(bad(cmd, format!("`{}` is given both `watts` and `w_m3`", body.name))),
            (Some(w), None) => {
                if !w.is_finite() {
                    return Err(bad(cmd, format!("`{}`'s `watts` must be a number", body.name)));
                }
                if cells == 0 {
                    return Err(bad(cmd, format!("`{}` fills no cell of the lattice, so it has nowhere to generate heat", body.name)));
                }
                w / (cells as f64 * volume)
            }
            (None, Some(d)) if d.is_finite() => d,
            (None, Some(d)) => return Err(bad(cmd, format!("`{}`'s `w_m3` is {d}", body.name))),
            (None, None) => continue,
        };
        if let Some(slot) = out.get_mut(index) {
            *slot = per_m3;
        }
    }
    Ok(out)
}

fn conditions(cmd: &str, p: &Value, picked: &[Picked], tags: &FaceTags, warnings: &mut Vec<String>) -> Result<Vec<Condition>> {
    let list = match p.get("conditions") {
        None => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(bad(cmd, "`conditions` must be a list")),
    };
    if list.len() > MAX_CONDITIONS {
        return Err(bad(cmd, format!("a study may carry {MAX_CONDITIONS} conditions")));
    }
    let mut out = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let name = str_(item, "name").map(str::to_string).unwrap_or_else(|| format!("condition {}", i + 1));
        let patch = patch_of(cmd, item, picked, tags, &name, warnings)?;
        let films = films_of(cmd, item, &name)?;
        if films.is_empty() {
            return Err(bad(cmd, format!("`{name}` says where but not what; give it a temperature, a flux, a film or `insulated`")));
        }
        out.push(Condition::new(name, patch, films));
    }
    Ok(out)
}

fn patch_of(cmd: &str, item: &Value, picked: &[Picked], tags: &FaceTags, name: &str, warnings: &mut Vec<String>) -> Result<Patch> {
    let places = [item.get("faces").is_some(), item.get("side").is_some(), item.get("rest").is_some()];
    match places.iter().filter(|x| **x).count() {
        0 => return Err(bad(cmd, format!("`{name}` does not say where it acts; give `faces`, `side` or `rest`"))),
        1 => {}
        _ => return Err(bad(cmd, format!("`{name}` says where it acts more than once; give one of `faces`, `side` or `rest`"))),
    }
    if item.get("rest").is_some() {
        return Ok(Patch::Rest);
    }
    if let Some(side) = str_(item, "side") {
        let (axis, positive) = match side.trim() {
            "+x" | "x" => (0, true),
            "-x" => (0, false),
            "+y" | "y" => (1, true),
            "-y" => (1, false),
            "+z" | "z" => (2, true),
            "-z" => (2, false),
            other => return Err(bad(cmd, format!("`{name}`'s side is `{other}`; it is one of +x, -x, +y, -y, +z, -z"))),
        };
        return Ok(Patch::Side { axis, positive });
    }
    let Some(faces) = item.get("faces").and_then(Value::as_array) else {
        return Err(bad(cmd, format!("`{name}`'s `faces` must be a list")));
    };
    if faces.len() > MAX_PICKS {
        return Err(bad(cmd, format!("`{name}` names more than {MAX_PICKS} faces")));
    }
    let mut picks = Vec::new();
    for pick in faces {
        picks.push(resolve_face(cmd, pick, picked, name)?);
    }
    picks.sort();
    picks.dedup();
    let cells = tags.patch_of(&picks);
    if cells.is_empty() {
        warnings.push(format!(
            "`{name}` names {} face(s) the lattice cannot reach at this spacing, so nothing of it is on the surface; raise \
             `cells` or the condition will not act",
            picks.len()
        ));
    }
    Ok(Patch::Faces(cells))
}

/// One face pick: `{body, face}`, or a point on the model.
fn resolve_face(cmd: &str, pick: &Value, picked: &[Picked], name: &str) -> Result<(u16, u32)> {
    if let Some(at) = vec3(pick) {
        return nearest_face(cmd, at, picked, name);
    }
    let body = str_(pick, "body");
    let index = num(pick, "face");
    match (body, index) {
        (Some(body), Some(face)) if face >= 0.0 && face.is_finite() => {
            let part = picked
                .iter()
                .position(|b| b.name.eq_ignore_ascii_case(body))
                .ok_or_else(|| bad(cmd, format!("`{name}` names body `{body}`, which this study does not take in")))?;
            let faces = picked.get(part).map(|b| b.mesh.tri_face.iter().copied().max().unwrap_or(0)).unwrap_or(0);
            if face as u32 > faces {
                return Err(bad(cmd, format!("`{name}` names face {face} of `{body}`, which has faces 0 to {faces}")));
            }
            Ok((part as u16, face as u32))
        }
        _ => Err(bad(cmd, format!("`{name}`'s faces are `{{body, face}}` pairs or `[x, y, z]` points on the model"))),
    }
}

/// The face of the model nearest a point: what a click in the viewport means.
fn nearest_face(cmd: &str, at: Vec3, picked: &[Picked], name: &str) -> Result<(u16, u32)> {
    let mut best: Option<(f64, u16, u32)> = None;
    for (part, body) in picked.iter().enumerate() {
        let set = TriangleSet::new(&body.mesh);
        for t in 0..set.len() as u32 {
            let d = set.distance_to(at, &[t]);
            let face = set.face(t).unwrap_or(0);
            if best.is_none_or(|(bd, _, bf)| d < bd || (d == bd && face < bf)) {
                best = Some((d, part as u16, face));
            }
        }
    }
    match best {
        Some((_, part, face)) => Ok((part, face)),
        None => Err(bad(cmd, format!("`{name}` names a point, and the bodies have no surface to put it on"))),
    }
}

fn films_of(cmd: &str, item: &Value, name: &str) -> Result<Vec<Film>> {
    let mut films = Vec::new();
    if let Some(kelvin) = temperature(cmd, item, "fixed", name)? {
        films.push(Film::Fixed { kelvin });
    }
    if item.get("flux_w_m2").is_some() {
        let flux = big(item, "flux_w_m2").ok_or_else(|| bad(cmd, format!("`{name}`'s `flux_w_m2` must be a number")))?;
        films.push(Film::Flux { w_m2: flux });
    }
    if let Some(convection) = item.get("convection") {
        let h = big(convection, "w_m2_k")
            .filter(|v| *v >= 0.0 && v.is_finite())
            .ok_or_else(|| bad(cmd, format!("`{name}`'s convection needs a `w_m2_k` of zero or more")))?;
        let kelvin = temperature(cmd, convection, "", name)?
            .ok_or_else(|| bad(cmd, format!("`{name}`'s convection needs the fluid's temperature (`c` or `k`)")))?;
        films.push(Film::Convection { w_m2_k: h, kelvin });
    }
    if let Some(radiation) = item.get("radiation") {
        let emissivity = num(radiation, "emissivity")
            .filter(|v| (0.0..=1.0).contains(v))
            .ok_or_else(|| bad(cmd, format!("`{name}`'s radiation needs an `emissivity` from 0 to 1")))?;
        let kelvin = temperature(cmd, radiation, "", name)?
            .ok_or_else(|| bad(cmd, format!("`{name}`'s radiation needs the surroundings' temperature (`c` or `k`)")))?;
        films.push(Film::Radiation { emissivity, kelvin });
    }
    if crate::params::bool_(item, "insulated").unwrap_or(false) {
        films.push(Film::Insulated);
    }
    Ok(films)
}

/// A temperature given either way round: `<base>_c` in degrees Celsius or `<base>_k` in kelvin
/// (plain `c` and `k` inside a film's own object).
fn temperature(cmd: &str, item: &Value, base: &str, name: &str) -> Result<Option<f64>> {
    let (celsius, kelvin) = if base.is_empty() { ("c", "k") } else { (&*format!("{base}_c"), &*format!("{base}_k")) };
    let c = num(item, celsius);
    let k = num(item, kelvin);
    match (c, k) {
        (Some(_), Some(_)) => Err(bad(cmd, format!("`{name}` gives a temperature in both degrees and kelvin"))),
        (Some(c), None) if c.is_finite() && c > -ZERO_C => Ok(Some(c + ZERO_C)),
        (None, Some(k)) if k.is_finite() && k > 0.0 => Ok(Some(k)),
        (Some(c), None) => Err(bad(cmd, format!("`{name}`'s {c} °C is below absolute zero"))),
        (None, Some(k)) => Err(bad(cmd, format!("`{name}`'s {k} K is not a temperature; kelvin is absolute"))),
        (None, None) => Ok(None),
    }
}

fn ambient(cmd: &str, p: &Value) -> Result<Vec<Film>> {
    let Some(a) = p.get("ambient") else { return Ok(vec![Film::Insulated]) };
    let Some(kelvin) = temperature(cmd, a, "", "`ambient`")? else {
        return Err(bad(cmd, "`ambient` needs a temperature (`c` or `k`)"));
    };
    let mut films = Vec::new();
    if let Some(h) = big(a, "convection") {
        if !(h >= 0.0) || !h.is_finite() {
            return Err(bad(cmd, format!("`ambient`'s convection is {h} W/(m²·K); it may not be negative")));
        }
        films.push(Film::Convection { w_m2_k: h, kelvin });
    }
    if let Some(e) = num(a, "emissivity") {
        if !(0.0..=1.0).contains(&e) {
            return Err(bad(cmd, format!("`ambient`'s emissivity is {e}; it is a fraction from 0 to 1")));
        }
        films.push(Film::Radiation { emissivity: e, kelvin });
    }
    if films.is_empty() {
        return Err(bad(cmd, "`ambient` gives a temperature but no way for heat to reach it; give `convection`, `emissivity`, or both"));
    }
    Ok(films)
}

fn limits(cmd: &str, p: &Value) -> Result<Limits> {
    let mut l = Limits::default();
    if let Some(t) = num(p, "tolerance") {
        if !(t > 0.0 && t < 1.0) {
            return Err(bad(cmd, format!("`tolerance` is {t}; it must be between 0 and 1")));
        }
        l.tolerance = t;
    }
    if let Some(n) = num(p, "max_iterations") {
        if !(1.0..=1e7).contains(&n) {
            return Err(bad(cmd, format!("`max_iterations` is {n}; it must be 1 to 10000000")));
        }
        l.max_iterations = n as u32;
    }
    Ok(l)
}

fn time(cmd: &str, p: &Value, occ: &Occupancy) -> Result<Time> {
    let Some(t) = p.get("time") else { return Ok(Time::Steady) };
    let seconds = num(t, "seconds").filter(|v| *v > 0.0 && v.is_finite()).ok_or_else(|| bad(cmd, "`time` needs `seconds` above zero"))?;
    let start =
        temperature(cmd, t, "from", "`time`")?.ok_or_else(|| bad(cmd, "`time` needs the temperature to start from (`from_c` or `from_k`)"))?;
    Ok(Time::Step { seconds, from: vec![start; occ.equations()] })
}

/// Most squares of surface one result shows. Past this they are smaller than a pixel on any
/// ordinary view and all the lattice does is cost memory.
const MAX_SHOWN_PATCHES: usize = 400_000;
/// Whether a call asked for its answer to be drawn on the model. On by default: an analysis
/// nobody can see is a number in a log.
fn wants_showing(p: &Value) -> bool {
    crate::params::bool_(p, "show").unwrap_or(true)
}

/// Hand a result to the viewport.
///
/// The revision moves, as it does for every other analysis that shades the model
/// (`inspect.zebra` and the rest): the viewport keeps the scene it built and rebuilds it when
/// the revision says there is something new in it, so a result that did not move the revision
/// would be held and never drawn. The design itself is untouched and no undo step is taken --
/// the revision is about what is on screen, not about what the part is.
fn show(s: &mut Session, result: Option<AnalysisResult>) {
    let Some(mut result) = result else { return };
    s.revision += 1;
    result.revision = s.revision;
    s.analysis_result = Some(result);
}

/// The biggest lattice a result keeps the inside of.
///
/// Past this, a cut plane, a threshold and a probe are not offered, rather than tens of
/// megabytes being held per field against the chance that someone asks for one. Two million
/// cells is a lattice around a hundred and twenty-five across, well past the resolution these
/// solvers are quick enough to be used at.
const MAX_KEPT_CELLS: usize = 2_000_000;

/// Whether a result keeps the values inside the model, and not only those on its surface.
fn keeps_inside(grid: &Grid3) -> bool {
    grid.cells() <= MAX_KEPT_CELLS
}

/// One number per cell of the lattice, NaN where `value` has none there.
fn cell_field(grid: &Grid3, mut value: impl FnMut(usize) -> Option<f64>) -> Vec<f32> {
    if !keeps_inside(grid) {
        return Vec::new();
    }
    (0..grid.cells()).map(|c| value(c).filter(|v| v.is_finite()).map(|v| v as f32).unwrap_or(f32::NAN)).collect()
}

/// One number per square of the lattice's surface, in the order `Occupancy::surface` walks it,
/// which is the order a result's `skin` is in.
fn skin_field(occ: &Occupancy, mut value: impl FnMut(SurfaceFace) -> Option<f64>) -> Vec<f32> {
    occ.surface().map(|f| value(f).filter(|v| v.is_finite()).map(|v| v as f32).unwrap_or(f32::NAN)).collect()
}

/// One quantity over the surface and the inside.
fn field(quantity: &str, unit: &str, low_is_cool: bool, surface: Vec<f32>, cells: Vec<f32>, worst: Option<Vec3>) -> AnalysisField {
    AnalysisField { quantity: quantity.to_string(), unit: unit.to_string(), low_is_cool, cells, surface, worst }
}

/// Everything a run worked out, laid out for the viewport: the squares of the model's surface,
/// the lattice behind them, and every field over both.
///
/// The first field is the one shown. `None` when there is nothing to draw -- no surface, no
/// fields, or no finite value anywhere in the one that would be shown -- because an empty
/// picture with a scale beside it says the study failed when it did not.
fn result_of(
    occ: &Occupancy,
    tags: &FaceTags,
    picked: &[Picked],
    about: String,
    revision: u64,
    fields: Vec<AnalysisField>,
) -> Option<AnalysisResult> {
    let grid = occ.grid();
    let side = grid.spacing();
    let mut skin = Vec::new();
    for (index, face) in occ.surface().enumerate() {
        if skin.len() >= MAX_SHOWN_PATCHES {
            break;
        }
        let Some(at) = grid.face_centre(face) else { continue };
        skin.push(AnalysisFace { at, normal: face.outward(), side, face: tags.tag_of(index) });
    }
    if skin.is_empty() || fields.is_empty() {
        return None;
    }
    let lattice = keeps_inside(grid).then(|| AnalysisLattice { origin: grid.origin(), spacing: side, count: grid.count() });
    let mut result = AnalysisResult {
        about,
        fields,
        skin,
        lattice,
        bodies: picked.iter().map(|b| b.name.clone()).collect(),
        vectors: Vec::new(),
        vector_quantity: String::new(),
        vector_unit: String::new(),
        view: AnalysisView::default(),
        revision,
        quantity: String::new(),
        unit: String::new(),
        low: 0.0,
        high: 0.0,
        low_is_cool: true,
        patches: Vec::new(),
        arrows: Vec::new(),
        lines: Vec::new(),
        probes: Vec::new(),
        worst: None,
    };
    result.redraw();
    (!result.patches.is_empty()).then_some(result)
}

/// How hard the heat is flowing through one cell, in watts per square metre.
///
/// `|k grad T|`, by central differences on the cells the solver solved for, with the cell's own
/// material's conductivity along each axis. Not a second solve: the same answer read a second
/// way, which is what a flux plot is. One-sided at a wall, where there is no cell on the far
/// side to difference against.
fn flux_in_cell(occ: &Occupancy, grid: &Grid3, kelvin: &[f64], materials: &[SolidMaterial], cell: usize) -> Option<f64> {
    let at = |c: usize| occ.eq_of(c).and_then(|e| kelvin.get(e)).copied();
    let here = at(cell)?;
    let k = occ.part_of(cell).and_then(|part| materials.get(part as usize)).map(|m| m.conductivity_w_m_k)?;
    let pitch = grid.pitch_m();
    if !(pitch > 0.0) {
        return None;
    }
    let mut sum = 0.0;
    for axis in 0..3usize {
        let back = grid.prev(cell, axis).and_then(at);
        let fore = grid.next(cell, axis).and_then(at);
        let slope = match (back, fore) {
            (Some(b), Some(f)) => (f - b) / (2.0 * pitch),
            (Some(b), None) => (here - b) / pitch,
            (None, Some(f)) => (f - here) / pitch,
            // No cell on either side along this axis: the model is one cell thick here, and a
            // difference taken over nothing is not a flux of zero, it is no answer at all.
            // Saying so leaves the cell blank, where a confident nought would read as a wall
            // that nothing crosses. The study already warns to raise `cells`.
            (None, None) => return None,
        };
        let along = k.get(axis).copied().unwrap_or(0.0) * slope;
        sum += along * along;
    }
    Some(sum.sqrt())
}

/// The `cells` this study was asked for.
/// What to say when the lattice could not be landed on the part, or nothing when it could.
///
/// `Grid3::over_parts` moves the cell count a few per cent to make the part a whole number of
/// cells across, and gives up rather than wander far from the count asked for. Where it gives
/// up, the overhang is solved as material and the part is bigger than the one on the screen --
/// up to 42 per cent of the answer on a bar whose answer is exact, and not reduced by refining
/// (`grid::fitted_span`, docs/analysis-verification.md). Saying nothing is the one thing that
/// cannot be right, so the warning names the axis, how much too big it is, and what to ask for.
fn misfit_warning(meshes: &[&Mesh], grid: &Grid3, p: &Value) -> Option<String> {
    let misfit = solidcraft_sim::grid::misfit(meshes, grid, span_of(p))?;
    let mut parts: Vec<String> = Vec::new();
    for (axis, over) in misfit.oversize.iter().enumerate() {
        let (Some(over), Some(name)) = (over, ["x", "y", "z"].get(axis)) else { continue };
        parts.push(format!("{:.1}% along {name}", over * 100.0));
    }
    if parts.is_empty() {
        return None;
    }
    let advice = match misfit.would_fit {
        Some(n) => format!("; `cells` of {n} would land on it exactly"),
        None => String::new(),
    };
    Some(format!(
        "the lattice does not divide the part, so it overhangs it and the overhang is solved as material: the part \
         comes out too big by {}. That is not reduced by raising `cells` -- a finer lattice that still does not \
         divide the part is just as wrong{advice}",
        parts.join(" and "),
    ))
}

fn span_of(p: &Value) -> usize {
    match num(p, "cells") {
        Some(n) if n.is_finite() && n >= MIN_SPAN as f64 && n <= MAX_SPAN as f64 => n as usize,
        _ => DEFAULT_SPAN,
    }
}

/// The same case again on a lattice three quarters as fine, and the worst stress it gives.
///
/// This is the check every textbook tells a person to do by hand and almost nobody does: solve
/// it twice and see whether the answer stayed put. It is worth having the study do it because
/// the answer that matters here often does *not* stay put -- on a part with a curved wall the
/// peak climbs as the lattice is refined and never settles, which no single run can show and
/// which two runs show immediately.
///
/// `None` where the coarser lattice is too coarse to hold the model at all, which is not a
/// failure of the study that was asked for.
fn coarser_worst(
    cmd: &str,
    p: &Value,
    meshes: &[&Mesh],
    picked: &[Picked],
    materials: &[solidcraft_sim::elastic::ElasticMaterial],
) -> Result<Option<(usize, f64)>> {
    let span = span_of(p);
    let coarse = (span * 3 / 4).max(MIN_SPAN);
    if coarse >= span {
        return Ok(None);
    }
    let mut less = p.clone();
    if let Some(o) = less.as_object_mut() {
        o.insert("cells".into(), json!(coarse));
        // The check is about the lattice, so everything else is held exactly as it was.
        o.remove("settled");
    }
    let (_, occ) = match build(cmd, &less, meshes) {
        Ok(v) => v,
        // Too coarse to hold the model: nothing to compare with, and not an error in the study.
        Err(_) => return Ok(None),
    };
    let tags = FaceTags::of(&occ, meshes);
    // A second set of warnings, thrown away: the study that was asked for has already said
    // whatever there was to say, and saying it twice helps nobody.
    let mut quiet: Vec<String> = Vec::new();
    let mut study = elastic::Study::new(materials.to_vec());
    study.loads = supports(cmd, &less, picked, &tags, &mut quiet)?;
    study.area_scale = tags.area_scale(&occ, meshes);
    study.limits = stress_limits(cmd, &less)?;
    let solution = elastic::solve(&occ, &study).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(solution.worst_stress.map(|w| (coarse, w.value)))
}

/// The nodes a load or a support has already been worked out over, which a snap must leave
/// alone.
///
/// A pressure or a traction is spread over the area the lattice's own face has. Move that face
/// and the load and the element stop agreeing about the same piece of surface -- which the
/// solver refuses outright rather than approximate, so a node that would cause it is better not
/// moved in the first place. A face square to the axes has nothing to gain by moving anyway,
/// which is where loads almost always are.
fn loaded_nodes(occ: &Occupancy, nodes: &solidcraft_sim::elastic::Nodes, study: &elastic::Study) -> Vec<bool> {
    let mut pinned = vec![false; nodes.count()];
    let faces: Vec<solidcraft_sim::grid::SurfaceFace> = occ.surface().collect();
    let Ok(owner) = elastic::claim_faces(&faces, &study.loads) else { return pinned };
    for (index, face) in faces.iter().enumerate() {
        let Some(load) = owner.get(index).copied().and_then(|o| study.loads.get(o)) else { continue };
        if !matches!(load.support, elastic::Support::Pressure(_) | elastic::Support::Traction(_)) {
            continue;
        }
        let Some(corners) = nodes.of_face(occ, *face) else { continue };
        for eq in corners {
            if let Some(slot) = pinned.get_mut(eq) {
                *slot = true;
            }
        }
    }
    pinned
}

/// The fluid a flow runs in: one of the named ones, with any of its numbers given instead.
///
/// The named fluids are room-temperature figures for a representative sample, which is enough to
/// see where the air goes and not enough to measure with. What a flow actually turns on is the
/// kinematic viscosity v = u/p: it sets the Reynolds number, and on a lattice it sets the
/// relaxation time. Air at 200 °C has about two and a half times the kinematic viscosity it has
/// at 20 °C, so a study run at a temperature that matters needs its own numbers, the way a
/// thermal or a stress study already takes `properties`.
///
/// Every number is checked rather than trusted: a density or a viscosity that is zero, negative
/// or not finite would divide the solver by nothing or hand it an infinity, so each is refused
/// here where it can still be explained.
fn fluid_properties(cmd: &str, p: &Value, named: library::Fluid, warnings: &mut Vec<String>) -> Result<library::Fluid> {
    let Some(given) = p.get("properties") else { return Ok(named) };
    let Some(given) = given.as_object() else {
        return Err(bad(cmd, "`properties` must be an object of the fluid's own numbers"));
    };
    let mut fluid = named;
    let mut changed = false;
    for (key, value) in given {
        let v = value
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| bad(cmd, format!("`properties.{key}` must be a number above zero")))?;
        match key.as_str() {
            "density_kg_m3" => fluid.density_kg_m3 = v,
            "dynamic_viscosity_pa_s" => fluid.dynamic_viscosity_pa_s = v,
            "conductivity_w_m_k" => fluid.conductivity_w_m_k = v,
            "specific_heat_j_kg_k" => fluid.specific_heat_j_kg_k = v,
            other => return Err(bad(cmd, format!("`properties.{other}` is not a fluid property a flow takes"))),
        }
        changed = true;
    }
    if changed {
        // Not the named fluid any more, and saying so keeps the answer honest: the result
        // already reports the density, the kinematic viscosity and the Prandtl number it ran on.
        fluid.name = "Custom";
    }
    // The flow itself runs on the kinematic viscosity alone, so a partial set is enough for it.
    // The Prandtl number is not: it is the viscosity against the conductivity and the heat
    // capacity, so giving the first two and leaving the others takes one temperature's viscosity
    // and another's conductivity and divides them. Air given at 200 °C but left with 20 °C
    // conductivity reads Pr 1.03, where air at 200 °C is about 0.68.
    let moved = given.contains_key("dynamic_viscosity_pa_s") || given.contains_key("density_kg_m3");
    let kept = !given.contains_key("conductivity_w_m_k") || !given.contains_key("specific_heat_j_kg_k");
    if moved && kept {
        warnings.push(format!(
            "the flow runs on the viscosity you gave, which is what it turns on, but the Prandtl number reported with it \
             still uses {}'s own conductivity and specific heat; give `conductivity_w_m_k` and `specific_heat_j_kg_k` as \
             well before reading it",
            named.name
        ));
    }
    Ok(fluid)
}

/// How many of the model's faces the lattice can only hold as a staircase: ones whose cell
/// faces do not all point the same way.
fn staircase_faces(tags: &FaceTags) -> usize {
    tags.patches()
        .iter()
        .filter(|patch| {
            let first = patch.faces.first().map(|f| f.dir);
            patch.faces.iter().any(|f| Some(f.dir) != first)
        })
        .count()
}

/// The hottest (or coldest) places, kept apart so one hot region is reported once.
fn spots(occ: &Occupancy, kelvin: &[f64], picked: &[Picked], hottest: bool) -> Vec<Value> {
    let mut order: Vec<(usize, f64)> = kelvin.iter().copied().enumerate().collect();
    order.sort_by(|a, b| if hottest { b.1.total_cmp(&a.1) } else { a.1.total_cmp(&b.1) });
    let apart = occ.grid().spacing() * SPOT_SEPARATION;
    let mut taken: Vec<Vec3> = Vec::new();
    let mut out = Vec::new();
    for (eq, t) in order {
        if out.len() >= MAX_SPOTS {
            break;
        }
        let Some(cell) = occ.cell_of(eq) else { continue };
        let Some(at) = occ.grid().centre(cell) else { continue };
        if taken.iter().any(|p| (*p - at).len() < apart) {
            continue;
        }
        let body = occ.part_of(cell).and_then(|part| picked.get(part as usize)).map(|b| b.name.clone()).unwrap_or_default();
        taken.push(at);
        out.push(json!({"body": body, "at": [at.x, at.y, at.z], "c": t - ZERO_C, "k": t}));
    }
    out
}

fn flow_study(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.flow";
    let picked = bodies(s, cmd, p)?;
    let meshes: Vec<&Mesh> = picked.iter().map(|b| b.mesh.as_ref()).collect();
    let mut warnings: Vec<String> = Vec::new();

    let speed = num(p, "speed_m_s")
        .filter(|v| *v > 0.0)
        .ok_or_else(|| bad(cmd, "`speed_m_s` must be a speed above zero; with nothing moving there is no flow"))?;
    let named = match str_(p, "fluid") {
        None => library::AIR,
        Some(name) => *library::fluid(name).ok_or_else(|| bad(cmd, format!("no fluid `{name}` (sim.materials lists them)")))?,
    };
    let fluid = fluid_properties(cmd, p, named, &mut warnings)?;
    let direction = match p.get("direction") {
        None => Vec3::new(1.0, 0.0, 0.0),
        Some(v) => vec3(v).and_then(|d| d.normalized()).ok_or_else(|| bad(cmd, "`direction` must be three numbers that are not all zero"))?,
    };
    let lattice_speed = match num(p, "lattice_speed") {
        None => 0.05,
        Some(v) if v > 0.0 && v <= MAX_LATTICE_SPEED => v,
        Some(v) => {
            return Err(bad(cmd, format!("`lattice_speed` is {v}; it must be above zero and at most {MAX_LATTICE_SPEED}")));
        }
    };
    let padding = match num(p, "padding") {
        None => FLOW_PADDING,
        Some(v) if (0.1..=8.0).contains(&v) => v,
        Some(v) => return Err(bad(cmd, format!("`padding` is {v}; it must be 0.1 to 8"))),
    };
    let collision = match str_(p, "collision").map(str::trim) {
        None | Some("regularized") => Collision::regularized(),
        Some("trt") => Collision::trt(),
        Some("bgk") => Collision::Bgk,
        Some("mrt") => Collision::mrt(),
        Some(other) => {
            return Err(bad(cmd, format!("`collision` is `{other}`; it is `regularized`, `trt`, `bgk` or `mrt`")));
        }
    };
    let wall = match str_(p, "wall").map(str::trim) {
        None | Some("model") => Wall::Interpolated,
        Some("lattice") => Wall::HalfWay,
        Some(other) => return Err(bad(cmd, format!("`wall` is `{other}`; it is `model` or `lattice`"))),
    };
    let steps = match num(p, "steps") {
        None => FLOW_STEPS,
        Some(v) if (1.0..=1e7).contains(&v) => v as u32,
        Some(v) => return Err(bad(cmd, format!("`steps` is {v}; it must be 1 to 10000000"))),
    };
    let settle = match num(p, "settle") {
        None => 1e-6,
        Some(v) if v >= 0.0 => v,
        Some(v) => return Err(bad(cmd, format!("`settle` is {v}; it may not be negative"))),
    };
    let turbulence = match num(p, "turbulence") {
        None => Some(SMAGORINSKY),
        // Zero turns the model off; anything above it is the constant.
        Some(v) if (0.0..=1.0).contains(&v) => (v > 0.0).then_some(v),
        Some(v) => return Err(bad(cmd, format!("`turbulence` is {v}; it is 0 to run laminar, or a constant up to 1"))),
    };

    let span = num(p, "cells").map(|n| n as usize).unwrap_or(FLOW_SPAN);
    let pad = ((span as f64 * padding).round() as usize).max(2);
    let (grid, occ) = build_with_room(cmd, p, &meshes, FLOW_SPAN, pad)?;

    let work = grid.cells() as f64 * steps as f64;
    if work > MAX_CELL_STEPS {
        return Err(bad(
            cmd,
            format!(
                "this study is {work:.3e} cell-steps and {MAX_CELL_STEPS:.0e} is the most one may ask for: \
                 {} cells for {steps} steps. Use fewer `cells`, less `padding`, or fewer `steps`",
                grid.cells()
            ),
        ));
    }

    // Map the real flow onto the lattice. This is where a flow turns out to be beyond what the
    // lattice can hold, and the mapping says so rather than leaving it to be discovered.
    let spacing_m = grid.spacing() / 1000.0;
    // The relaxation time the lattice can hold is the collision operator's property, not the
    // method's, so the mapping is worked out for the operator this study actually runs.
    let mapping = Scaling::for_flow(&fluid, spacing_m, speed, lattice_speed, collision).map_err(|e| bad(cmd, e.to_string()))?;
    let area_m2 = projected_area_m2(&occ, direction);
    let length_m = area_m2.sqrt();
    let length_cells = if spacing_m > 0.0 { length_m / spacing_m } else { 0.0 };
    if let Some(caveat) = mapping.caveat(length_cells) {
        match turbulence {
            Some(c) => warnings.push(format!(
                "{caveat}. A subgrid turbulence model is standing in for the eddies smaller than a cell, with a Smagorinsky \
                 constant of {c}; it is a model of how much momentum they would have moved, not an account of what they did"
            )),
            None => warnings.push(format!("{caveat}. Running laminar, so expect the solver to come apart")),
        }
    }

    let lattice_velocity = direction * lattice_speed;
    let mut study = FlowStudy::new(mapping.tau);
    study.collision = collision;
    study.wall = wall;
    study.inlet = [lattice_velocity.x, lattice_velocity.y, lattice_velocity.z];
    study.sides = tunnel(direction);
    study.steps = steps;
    study.settle = settle;
    study.check_every = 50;
    study.turbulence = turbulence;

    // Each link from the air into the model is cut where the model's surface really crosses it,
    // so a face the lattice does not land on is held where it is rather than half a cell out.
    // The test for whether the point is inside is the model's own, one closure per body.
    let inside_each: Vec<_> = meshes.iter().map(|m| solidcraft_sim::cut::inside_of(m)).collect();
    let inside = |p: Vec3| inside_each.iter().any(|f| f(p));
    let mut flow = Flow::around_surface(&occ, &inside, &study).map_err(|e| bad(cmd, e.to_string()))?;
    flow.set_everywhere(|c| if occ.is_solid(c) { (1.0, Vec3::ZERO) } else { (1.0, lattice_velocity) });
    let report = flow.run(&study).map_err(|e| bad(cmd, e.to_string()))?;
    if !report.settled {
        warnings.push(format!(
            "the flow was still changing by {:.2e} on the lattice when the steps ran out; a flow that sheds a wake never \
             settles, and one that has not settled for any other reason wants more `steps`",
            report.change
        ));
    }
    if lattice_speed < MIN_LATTICE_SPEED {
        warnings.push(format!(
            "the flow ran at {lattice_speed} on the lattice, below the {MIN_LATTICE_SPEED} where a smaller number stops \
             buying accuracy: the distributions are f32, and the force is a difference that loses more to round-off the \
             slower the flow, about tenfold for every decade. Raise `lattice_speed`"
        ));
    }
    if report.fastest > MAX_LATTICE_SPEED * 2.0 {
        warnings.push(format!(
            "the air reached {:.3} on the lattice, well past the {MAX_LATTICE_SPEED} the low-Mach approximation holds at; \
             lower `lattice_speed`",
            report.fastest
        ));
    }

    // The force, out of lattice units.
    let force = mapping.scaling.force_n(flow.force());
    let drag = force.dot(direction);
    let cross = force - direction * drag;
    let dynamic = 0.5 * fluid.density_kg_m3 * speed * speed;

    // The pressure and the fastest air.
    let (mut lowest, mut highest) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut fastest, mut fastest_at) = (0.0f64, Vec3::ZERO);
    for c in 0..grid.cells() {
        if flow.is_solid(c) {
            continue;
        }
        let pa = mapping.scaling.pressure_pa(flow.pressure(c));
        lowest = lowest.min(pa);
        highest = highest.max(pa);
        let u = flow.velocity(c, study.accel).len();
        if u > fastest {
            fastest = u;
            fastest_at = grid.centre(c).unwrap_or(Vec3::ZERO);
        }
    }
    let fastest_m_s = mapping.scaling.velocity_m_s(Vec3::new(fastest, 0.0, 0.0)).x;

    if area_m2 <= 0.0 {
        warnings.push("the model has no area across the flow on this lattice, so there is no drag coefficient".into());
    }
    if !collision.holds_the_wall() {
        // The force and the drag coefficient are read off the momentum arriving at the model's
        // surface, so an operator that cannot say where that surface is cannot report them.
        warnings.push(format!(
            "`collision` is `{}`, which does not hold a bounce-back wall in one place: where the wall effectively sits \
             moves with the viscosity. On a channel whose answer is known that is a few per cent of the flow, against the \
             four parts in ten thousand `regularized` and `trt` give. The force, the drag coefficient and the pressure on \
             the model are all read at that surface, so treat them as showing where the air goes rather than as \
             measurements; `regularized` is the one to measure with",
            collision.name()
        ));
    }
    if padding < 1.0 {
        warnings.push(format!(
            "the tunnel is only {padding} of the model's span, so its own walls crowd the flow and its outlet is close \
             enough behind the model for the wake to reflect off it; raise `padding` to at least one, and to several for \
             anything with a long wake"
        ));
    }

    if wants_showing(p) {
        // The pressure *on* the model is the pressure of the air against it, so it is read from
        // the fluid cell the face looks at -- not from the solid cell behind it, which holds no
        // fluid at all and would read as a vacuum everywhere.
        let air_of = |face: SurfaceFace| {
            let cell = face.cell as usize;
            let outside = if face.positive() { grid.next(cell, face.axis()) } else { grid.prev(cell, face.axis()) };
            outside.filter(|c| !flow.is_solid(*c)).unwrap_or(cell)
        };
        let pressure_of = |cell: usize| (!flow.is_solid(cell)).then(|| mapping.scaling.pressure_pa(flow.pressure(cell)));
        let speed_of = |cell: usize| (!flow.is_solid(cell)).then(|| mapping.scaling.velocity_m_s(flow.velocity(cell, study.accel)).len());
        let about = format!("{speed} m/s of {}, drag {:.3} N, Reynolds number {:.0}", fluid.name, drag, mapping.effective_reynolds(length_cells));
        let pressure = field("Pressure", "Pa", false, skin_field(&occ, |f| pressure_of(air_of(f))), cell_field(&grid, pressure_of), None);
        let fast = field("Speed", "m/s", true, skin_field(&occ, |f| speed_of(air_of(f))), cell_field(&grid, speed_of), None);
        // Which face of the model each square of the lattice's surface belongs to, so the
        // pressure can be added up over a face someone picks. A flow study does not otherwise
        // need them, so they are worked out here rather than with the study.
        let tags = FaceTags::of(&occ, &meshes);
        let mut result = result_of(&occ, &tags, &picked, about, s.revision, vec![pressure, fast]);
        if let Some(r) = result.as_mut() {
            // The velocity itself, which the arrows and the streamlines follow. Kept as the
            // vector it is rather than as the speed it works out to, because a picture of where
            // the air goes needs the direction and a scale of speeds does not.
            r.vectors = (0..grid.cells())
                .map(|c| {
                    if flow.is_solid(c) {
                        return [0.0f32; 3];
                    }
                    let u = mapping.scaling.velocity_m_s(flow.velocity(c, study.accel));
                    [u.x as f32, u.y as f32, u.z as f32]
                })
                .collect();
            r.vector_quantity = "Velocity".to_string();
            r.vector_unit = "m/s".to_string();
            r.redraw();
        }
        show(s, result);
    }

    Ok(json!({
        "lattice": lattice_json(&grid, &occ, model_volume_of(&picked).0),
        "fluid": {
            "fluid": fluid.name,
            "density_kg_m3": fluid.density_kg_m3,
            "kinematic_viscosity_m2_s": fluid.kinematic_viscosity_m2_s(),
            "prandtl": fluid.prandtl(),
        },
        "mapping": {
            "relaxation_time": mapping.tau,
            "physical_relaxation_time": mapping.physical_tau,
            "thickened": mapping.thickened(),
            "turbulence": turbulence,
            "step_s": mapping.scaling.step_s,
            "simulated_s": mapping.scaling.duration_s(report.steps),
            "cell_reynolds": mapping.cell_reynolds,
            "resolved": mapping.resolved(),
            "resolved_spacing_mm": mapping.resolved_spacing_m * 1000.0,
            "refinement_wanted": mapping.refinement_wanted(),
        },
        "collision": {
            "operator": collision.name(),
            "holds_the_wall": collision.holds_the_wall(),
            "min_tau": collision.min_tau(),
        },
        "reynolds": {
            "value": fluid.reynolds(speed, length_m),
            "ran_at": mapping.effective_reynolds(length_cells),
            "length_mm": length_m * 1000.0,
            "based_on": "the square root of the model's area across the flow",
        },
        "force": {
            "vector_n": [force.x, force.y, force.z],
            "drag_n": drag,
            "cross_n": cross.len(),
            "cross_vector_n": [cross.x, cross.y, cross.z],
            "drag_coefficient": if area_m2 > 0.0 && dynamic > 0.0 { drag / (dynamic * area_m2) } else { 0.0 },
            "reference_area_mm2": area_m2 * 1e6,
            "dynamic_pressure_pa": dynamic,
        },
        "pressure": {
            "min_pa": if lowest.is_finite() { lowest } else { 0.0 },
            "max_pa": if highest.is_finite() { highest } else { 0.0 },
        },
        "speed": {
            "inlet_m_s": speed,
            "fastest_m_s": fastest_m_s,
            "fastest_at": [fastest_at.x, fastest_at.y, fastest_at.z],
            "lattice_fastest": report.fastest,
        },
        "solver": {
            "steps": report.steps,
            "settled": report.settled,
            "change": report.change,
            "mass": report.mass,
        },
        "warnings": warnings,
    }))
}

/// The six sides of a wind tunnel for a flow along `direction`.
///
/// Every axis the flow has a component along gets an inlet where it comes from and an outlet
/// where it goes, so a direction off the axes is handled properly rather than approximated by
/// the nearest one. An axis the flow has no component along gets free-slip walls, which let the
/// air past without dragging on it.
fn tunnel(direction: Vec3) -> [Side; 6] {
    let mut sides = [Side::FreeSlip; 6];
    for (axis, d) in [direction.x, direction.y, direction.z].into_iter().enumerate() {
        let (low, high) = (axis * 2, axis * 2 + 1);
        // A thousandth of the flow along an axis is not a flow along it.
        let (from, to) = if d > 1e-3 {
            (low, high)
        } else if d < -1e-3 {
            (high, low)
        } else {
            continue;
        };
        if let Some(slot) = sides.get_mut(from) {
            *slot = Side::Inlet;
        }
        if let Some(slot) = sides.get_mut(to) {
            *slot = Side::Outlet;
        }
    }
    sides
}

/// The model's area across the flow, as the lattice has it: the shadow it casts along the
/// dominant axis of the flow.
///
/// Taken from the lattice rather than from the mesh on purpose. It is the lattice's own
/// staircase of the body that the force was computed from, so a drag coefficient made with the
/// lattice's area is self-consistent; using the mesh's exact silhouette would divide one by the
/// other.
fn projected_area_m2(occ: &Occupancy, direction: Vec3) -> f64 {
    let grid = occ.grid();
    let n = grid.count();
    let [dx, dy, dz] = [direction.x.abs(), direction.y.abs(), direction.z.abs()];
    let axis = if dx >= dy && dx >= dz {
        0
    } else if dy >= dz {
        1
    } else {
        2
    };
    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
    let (Some(across_a), Some(across_b)) = (n.get(a).copied(), n.get(b).copied()) else { return 0.0 };
    let mut shadow = 0usize;
    for i in 0..across_a {
        for j in 0..across_b {
            let mut hit = false;
            for k in 0..n.get(axis).copied().unwrap_or(0) {
                let mut at = [0usize; 3];
                if let Some(s) = at.get_mut(a) {
                    *s = i;
                }
                if let Some(s) = at.get_mut(b) {
                    *s = j;
                }
                if let Some(s) = at.get_mut(axis) {
                    *s = k;
                }
                if grid.at(at[0], at[1], at[2]).is_some_and(|c| occ.is_solid(c)) {
                    hit = true;
                    break;
                }
            }
            if hit {
                shadow += 1;
            }
        }
    }
    shadow as f64 * grid.face_area_m2()
}

fn stress_study(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.stress";
    let picked = bodies(s, cmd, p)?;
    let meshes: Vec<&Mesh> = picked.iter().map(|b| b.mesh.as_ref()).collect();
    let (grid, occ) = build(cmd, p, &meshes)?;
    let tags = FaceTags::of(&occ, &meshes);
    let mut warnings: Vec<String> = Vec::new();

    let properties = read_materials(cmd, &s.doc, p, &picked, Want::Strength, &mut warnings)?;
    let mut materials = Vec::new();
    for read in &properties {
        materials.push(match read {
            Read::Strength(m) => *m,
            Read::Heat(_) => return Err(bad(cmd, "a body was given thermal properties where mechanical ones are needed")),
            Read::Card(props) => props.for_stress().map_err(|e| bad(cmd, e.to_string()))?,
        });
    }
    for (body, m) in picked.iter().zip(&materials) {
        if m.yield_strength_pa.is_none() {
            warnings.push(format!(
                "`{}` has no yield point to measure against, so there is no safety factor for it. A brittle material -- \
                 cast iron, glass, a printed plastic -- breaks rather than yields, and von Mises stress is a yield \
                 criterion: compare it with the tensile strength yourself, and remember that a brittle material is far \
                 weaker in tension than in compression, which von Mises does not distinguish",
                body.name
            ));
        }
    }

    // The part's own warmth as a load. Either a single rise, or a conduction study run on this
    // very lattice so the field is the one the part actually has.
    let mut rise = Vec::new();
    let mut heat: Value = Value::Null;
    match (p.get("temperature_rise_k"), p.get("heat")) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give either `temperature_rise_k` or `heat`, not both")),
        (Some(_), None) => {
            // Said but not a number: refused rather than quietly taken as nothing, because a
            // load case with the warming dropped out of it is a different answer that looks
            // just as finished.
            let uniform = big(p, "temperature_rise_k")
                .ok_or_else(|| bad(cmd, "`temperature_rise_k` must be a number of kelvin above the unstressed temperature"))?;
            rise = vec![uniform; occ.equations()];
        }
        (None, Some(params)) => {
            let reference = temperature(cmd, params, "reference", "`heat`")?
                .ok_or_else(|| bad(cmd, "`heat` needs `reference_c` or `reference_k`: the temperature the part is unstressed at"))?;
            let warm = read_materials(cmd, &s.doc, p, &picked, Want::Heat, &mut warnings)?;
            let conduction = conduction_materials(cmd, &warm, params.get("time").is_some())?;
            let mut study = thermal_from(cmd, params, &picked, &occ, &tags, conduction, &mut warnings)?;
            study.area_scale = tags.area_scale(&occ, &meshes);
            let answer = thermal::solve(&occ, &study).map_err(|e| bad(cmd, e.to_string()))?;
            rise = answer.kelvin.iter().map(|k| k - reference).collect();
            heat = json!({
                "min_c": answer.min_kelvin - ZERO_C,
                "max_c": answer.max_kelvin - ZERO_C,
                "reference_c": reference - ZERO_C,
                "generated_w": answer.generated_w,
                "imbalance": answer.imbalance,
                "patches": answer
                    .patches
                    .iter()
                    .map(|x| json!({"name": x.name, "watts": x.watts}))
                    .collect::<Vec<_>>(),
            });
        }
        (None, None) => {}
    }

    let mut study = elastic::Study::new(materials.clone());
    study.loads = supports(cmd, p, &picked, &tags, &mut warnings)?;
    study.temperature_rise_k = rise;
    study.gravity_m_s2 = gravity(cmd, p)?;
    study.area_scale = tags.area_scale(&occ, &meshes);
    study.limits = stress_limits(cmd, p)?;

    // Asked to follow the model's surface: pull the boundary nodes onto it, leaving alone the
    // ones a load's area was worked out from, since that area is still the lattice's.
    let mut fitted_nodes = None;
    if crate::params::bool_(p, "fit_surface").unwrap_or(false) {
        let nodes = elastic::Nodes::of(&occ);
        let pinned = loaded_nodes(&occ, &nodes, &study);
        let snapped = solidcraft_sim::snap::snap_keeping(&occ, &nodes, &meshes, &pinned);
        if snapped.is_lattice() {
            warnings.push(
                "nothing to follow: every wall of this model already lands on the lattice, so the answer is the one it \
                 would have given anyway"
                    .into(),
            );
        }
        if snapped.gave_up() > 0 {
            warnings.push(format!(
                "{} nodes were left where the lattice put them because moving them turned an element inside out; the \
                 surface is followed everywhere else",
                snapped.gave_up()
            ));
        }
        fitted_nodes = Some((snapped.moved(), snapped.gave_up()));
        study.fitted = Some(snapped);
    }

    let solution = elastic::solve(&occ, &study).map_err(|e| bad(cmd, e.to_string()))?;

    // Asked to check itself: the same case on a coarser lattice, and how far the answer moved.
    let settling = if crate::params::bool_(p, "settled").unwrap_or(false) {
        match coarser_worst(cmd, p, &meshes, &picked, &materials) {
            Ok(Some(coarse)) => {
                let fine = solution.worst_stress.map(|w| w.value).unwrap_or(0.0);
                let moved = if fine > 0.0 { (fine - coarse.1).abs() / fine } else { 0.0 };
                // A twentieth. A part square to the axes moves by a fraction of a per cent
                // between these two lattices; anything near a tenth is a figure still finding
                // its feet, and the plate with a hole moves 9.5%.
                if moved > 0.05 {
                    warnings.push(format!(
                        "the worst stress moved {:.0}% between a lattice of {} cells and one of {}, so this figure has \
                         not settled: it is telling you about the lattice as much as about the part. Where the part is \
                         worst is still the place to look",
                        moved * 100.0,
                        coarse.0,
                        span_of(p)
                    ));
                }
                Some(json!({
                    "coarser_cells": coarse.0,
                    "coarser_worst_von_mises_mpa": coarse.1 / 1.0e6,
                    "moved_fraction": moved,
                }))
            }
            Ok(None) => None,
            Err(e) => {
                warnings.push(format!("the coarser lattice for the settling check could not be solved: {e}"));
                None
            }
        }
    } else {
        None
    };

    if solution.imbalance > study.limits.tolerance.max(1e-6) * 1000.0 {
        warnings.push(format!(
            "the load and the reactions do not cancel to {:.3e}, which they should; treat the forces with suspicion",
            solution.imbalance
        ));
    }
    warnings.extend(misfit_warning(&meshes, &grid, p));
    let staircase = staircase_faces(&tags);
    if staircase > 0 {
        warnings.push(format!(
            "{staircase} of the model's faces are not square to an axis, so the lattice holds them as a staircase; a \
             stress read at a staircase corner is a stress at a corner the model does not have, and comes out high. \
             Raising `cells` makes it worse rather than better: a step's inside corner has no finite stress, so a \
             finer lattice reads a higher peak, and the peak rises without limit. On a plate with a hole, pulled, the \
             worst stress reads 2.89, 4.27, 5.22 and 6.20 times the pull as the cells go 2.0, 1.0, 0.67, 0.5 mm -- \
             a power law in the cell size with an exponent of -0.55, which is a corner singularity (a 90-degree \
             notch gives -0.46) and not a convergence, straight past the 3.24 a conforming mesh gives rather than \
             approaching it. Neither `fit_surface` nor \
             `least_fill` settles it; they move it by a few per cent. A setting that lands near the truth at one \
             spacing is a curve crossing it, not the tool reaching it. So *where* the part is worst is reliable and \
             the stress there is not a figure to quote: compare two parts at one `cells`, and do not read the number \
             as a stress the part really sees"
        ));
    }
    warnings.push(
        "a lattice of bricks is a little stiffer in bending than the part is, so a deflection comes out short -- about two \
         per cent with ten cells across the bending part of it, less as the lattice is refined. Raise `cells` until the \
         answer stops moving before trusting a deflection"
            .into(),
    );

    if wants_showing(p) {
        // The stress in the cell behind each face of the surface. A staircase corner carries a
        // stress the real surface does not, which the warning above says.
        let nodes = elastic::Nodes::of(&occ);
        let stress_of = |cell: usize| solution.stress_at_cell(&occ, cell).map(|v| v / 1.0e6);
        // How far the cell moved: its eight corners averaged, in millimetres. Averaged because
        // the solver moves corners while the picture shades cells; the furthest a single corner
        // went is in the reply, where it is not averaged with anything.
        let movement_of = |cell: usize| {
            let corners = nodes.of_cell(&occ, cell)?;
            let mut sum = 0.0;
            for node in corners {
                let d = solution.displacement.get(node * 3..node * 3 + 3)?;
                sum += Vec3::new(*d.first()?, *d.get(1)?, *d.get(2)?).len();
            }
            Some(sum / corners.len() as f64 * 1000.0)
        };
        let safety_of = |cell: usize| {
            let stress = solution.stress_at_cell(&occ, cell)?;
            let yield_pa = occ.part_of(cell).and_then(|part| materials.get(part as usize)).and_then(|m| m.yield_strength_pa)?;
            (stress > 0.0).then(|| (yield_pa / stress).min(SAFETY_SHOWN_AT_MOST))
        };
        let about = match solution.least_safety_factor {
            Some(w) => format!("worst {:.0} MPa, safety factor {:.2}", solution.worst_stress.map_or(0.0, |x| x.value / 1.0e6), w.value),
            None => format!("worst {:.0} MPa, no yield point to measure against", solution.worst_stress.map_or(0.0, |x| x.value / 1.0e6)),
        };
        let mut fields = vec![
            field(
                "Von Mises stress",
                "MPa",
                true,
                skin_field(&occ, |f| stress_of(f.cell as usize)),
                cell_field(&grid, stress_of),
                solution.worst_stress.map(|w| w.at),
            ),
            field(
                "Displacement",
                "mm",
                true,
                skin_field(&occ, |f| movement_of(f.cell as usize)),
                cell_field(&grid, movement_of),
                solution.most_movement.map(|w| w.at),
            ),
        ];
        // Only where there is a yield point to measure against. A safety factor against nothing
        // is a column of blanks, and the warning above has already said which body lacks one.
        if materials.iter().any(|m| m.yield_strength_pa.is_some()) {
            fields.push(field(
                "Safety factor",
                "",
                false,
                skin_field(&occ, |f| safety_of(f.cell as usize)),
                cell_field(&grid, safety_of),
                solution.least_safety_factor.map(|w| w.at),
            ));
        }
        show(s, result_of(&occ, &tags, &picked, about, s.revision, fields));
    }

    let where_json = |w: Option<solidcraft_sim::elastic::Worst>| match w {
        None => Value::Null,
        Some(w) => {
            let body = occ
                .cell_of(w.equation)
                .and_then(|c| occ.part_of(c))
                .and_then(|part| picked.get(part as usize))
                .map(|b| b.name.clone())
                .unwrap_or_default();
            json!({"value": w.value, "at": [w.at.x, w.at.y, w.at.z], "body": body})
        }
    };

    let bodies_json: Vec<Value> = picked
        .iter()
        .zip(&properties)
        .zip(&materials)
        .map(|((b, read), m)| {
            json!({
                "body": b.name,
                "material": b.material.clone().unwrap_or_else(|| "Default".into()),
                "source": read.source(),
                "young_modulus_pa": m.young_modulus_pa,
                "poisson_ratio": m.poisson_ratio,
                "yield_strength_pa": m.yield_strength_pa,
                "expansion_per_k": m.expansion_per_k,
                "density_kg_m3": m.density_kg_m3,
            })
        })
        .collect();

    Ok(json!({
        "lattice": lattice_json(&grid, &occ, model_volume_of(&picked).0),
        "bodies": bodies_json,
        "stress": {
            "fitted": fitted_nodes.map(|(moved, gave_up)| json!({"nodes_moved": moved, "nodes_left": gave_up})),
            "settled": settling,
            "worst_von_mises_pa": solution.worst_stress.map(|w| w.value),
            "worst_von_mises_mpa": solution.worst_stress.map(|w| w.value / 1.0e6),
            "worst": where_json(solution.worst_stress),
        },
        "safety": where_json(solution.least_safety_factor),
        "movement": {
            "most_mm": solution.most_movement.map(|w| w.value * 1000.0),
            "most": where_json(solution.most_movement),
        },
        "balance": {
            "applied_n": solution.applied_n,
            "reaction_n": solution.reaction_n,
            "imbalance": solution.imbalance,
        },
        "heat": heat,
        "solver": {
            "iterations": solution.linear.iterations,
            "residual": solution.linear.residual,
            "tolerance": solution.linear.tolerance,
            "freedoms": solution.displacement.len(),
        },
        "warnings": warnings,
    }))
}

fn supports(cmd: &str, p: &Value, picked: &[Picked], tags: &FaceTags, warnings: &mut Vec<String>) -> Result<Vec<Load>> {
    let list = match p.get("loads") {
        None => return Err(bad(cmd, "`loads` must be a list; a part with nothing holding it would simply drift")),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(bad(cmd, "`loads` must be a list")),
    };
    if list.len() > MAX_CONDITIONS {
        return Err(bad(cmd, format!("a study may carry {MAX_CONDITIONS} loads")));
    }
    let mut out = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let name = str_(item, "name").map(str::to_string).unwrap_or_else(|| format!("load {}", i + 1));
        let patch = patch_of(cmd, item, picked, tags, &name, warnings)?;
        let support = support_of(cmd, item, &name)?;
        out.push(Load::new(name, patch, support));
    }
    Ok(out)
}

fn support_of(cmd: &str, item: &Value, name: &str) -> Result<Support> {
    let said = [
        item.get("fixed").is_some(),
        item.get("roller").is_some(),
        item.get("pressure_pa").is_some(),
        item.get("traction_pa").is_some(),
        item.get("free").is_some(),
    ];
    match said.iter().filter(|x| **x).count() {
        0 => {
            return Err(bad(cmd, format!("`{name}` says where but not what; give it `fixed`, `roller`, `pressure_pa`, `traction_pa` or `free`")));
        }
        1 => {}
        _ => return Err(bad(cmd, format!("`{name}` says what it does more than once"))),
    }
    if crate::params::bool_(item, "fixed").unwrap_or(false) {
        return Ok(Support::Fixed);
    }
    if crate::params::bool_(item, "roller").unwrap_or(false) {
        return Ok(Support::Roller);
    }
    if crate::params::bool_(item, "free").unwrap_or(false) {
        return Ok(Support::Free);
    }
    if item.get("pressure_pa").is_some() {
        let pa = big(item, "pressure_pa").ok_or_else(|| bad(cmd, format!("`{name}`'s `pressure_pa` must be a number")))?;
        return Ok(Support::Pressure(pa));
    }
    let t = big_vec3(item.get("traction_pa")).ok_or_else(|| bad(cmd, format!("`{name}`'s `traction_pa` must be three numbers")))?;
    Ok(Support::Traction([t.x, t.y, t.z]))
}

fn gravity(cmd: &str, p: &Value) -> Result<[f64; 3]> {
    match p.get("gravity") {
        None => Ok([0.0; 3]),
        Some(Value::Bool(false)) => Ok([0.0; 3]),
        // The direction a part stands up in: the document's Z is up.
        Some(Value::Bool(true)) => Ok([0.0, 0.0, -9.806_65]),
        Some(v) => {
            let g = vec3(v).ok_or_else(|| bad(cmd, "`gravity` is `true`, or three numbers in m/s²"))?;
            Ok([g.x, g.y, g.z])
        }
    }
}

fn stress_limits(cmd: &str, p: &Value) -> Result<elastic::Limits> {
    let mut l = elastic::Limits::default();
    if let Some(t) = num(p, "tolerance") {
        if !(t > 0.0 && t < 1.0) {
            return Err(bad(cmd, format!("`tolerance` is {t}; it must be between 0 and 1")));
        }
        l.tolerance = t;
    }
    if let Some(n) = num(p, "max_iterations") {
        if !(1.0..=1e7).contains(&n) {
            return Err(bad(cmd, format!("`max_iterations` is {n}; it must be 1 to 10000000")));
        }
        l.max_iterations = n as u32;
    }
    Ok(l)
}

// ---- saved studies -------------------------------------------------------------------------
//
// A study the design keeps, so a question put to a solver can be asked again after the part
// changes. The setup is the running command's own parameters, held as they are: see
// `solidcraft_doc::analysis` for why there is one parameter model here and not two.

/// The study `name` stands for, or a refusal that names the studies there are, since a caller
/// who got the name wrong is better served by the list than by being told no.
fn unknown_study(cmd: &str, s: &Session, name: &str) -> crate::EngineError {
    let have: Vec<&str> = s.doc.analysis.studies.keys().map(String::as_str).collect();
    if have.is_empty() {
        return bad(cmd, format!("there is no study called `{name}`; this design has none yet (`sim.study.create`)"));
    }
    bad(cmd, format!("there is no study called `{name}`; this design has {}", have.join(", ")))
}

fn study_name(cmd: &str, p: &Value) -> Result<String> {
    let name = str_(p, "study").map(str::trim).filter(|n| !n.is_empty());
    name.map(str::to_string).ok_or_else(|| bad(cmd, "`study` is required: the name of the study"))
}

/// A setup is the parameters its command takes. Anything but an object is a caller who has
/// confused the study with the study's name.
fn study_setup(cmd: &str, p: &Value) -> Result<Map<String, Value>> {
    match p.get("setup") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(o)) => Ok(o.clone()),
        Some(_) => Err(bad(cmd, "`setup` is an object: the parameters the study's own command takes")),
    }
}

/// A name for a new study of this kind that is not taken: "Thermal Study 2" after
/// "Thermal Study 1", the way a design names anything else it makes for you.
fn next_study_name(s: &Session, kind: SavedKind) -> String {
    let label = match kind {
        SavedKind::Thermal => "Thermal Study",
        SavedKind::Flow => "Flow Study",
        SavedKind::Stress => "Stress Study",
    };
    (1..=solidcraft_doc::analysis::MAX_STUDIES + 1)
        .map(|i| format!("{label} {i}"))
        .find(|n| !s.doc.analysis.studies.contains_key(n))
        .unwrap_or_else(|| label.to_string())
}

/// What a study still needs, node by node, the way a simulation tree shows it.
///
/// `incomplete` is the only one that stops a run, and it is worked out the same way the solver
/// works it out, so the tree and the run never disagree: a status that says ready about a study
/// that then refuses to run is worse than no status at all.
fn study_nodes(s: &Session, study: &SavedStudy) -> Value {
    let setup = Value::Object(study.setup.clone());
    let cmd = study.kind.command();
    // The bodies the study covers. If they cannot even be worked out, that is the first thing
    // wrong with it and the only thing worth saying.
    let picked = match bodies(s, cmd, &setup) {
        Ok(picked) => picked,
        Err(e) => {
            return json!([{ "node": "bodies", "status": "error", "says": format!("{e}") }]);
        }
    };
    let overrides = study.setup.get("properties").and_then(Value::as_object);
    let by_name = study.setup.get("material").and_then(Value::as_object);
    let for_all = str_(&setup, "material");
    let without: Vec<&str> = picked
        .iter()
        .filter(|b| {
            let named = overrides.and_then(|o| lookup(o, &b.name)).is_some()
                || by_name.and_then(|o| lookup(o, &b.name)).is_some()
                || for_all.is_some()
                || b.material.is_some();
            !named
        })
        .map(|b| b.name.as_str())
        .collect();
    let materials = if without.is_empty() {
        json!({ "node": "materials", "status": "complete" })
    } else {
        json!({ "node": "materials", "status": "incomplete", "says": format!("no material for {}", without.join(", ")) })
    };

    // What the study is driven by: conditions for a conduction, loads for a stress, a speed for
    // a flow. Each solver refuses a study that has none, so each is mandatory here.
    let driver = match study.kind {
        SavedKind::Thermal => {
            let n = study.setup.get("conditions").and_then(Value::as_array).map_or(0, Vec::len);
            if n == 0 {
                json!({ "node": "conditions", "status": "incomplete", "says": "nothing holds a temperature, so nothing settles" })
            } else {
                json!({ "node": "conditions", "status": "complete", "says": format!("{n}") })
            }
        }
        SavedKind::Flow => match num(&setup, "speed_m_s").filter(|v| *v > 0.0) {
            Some(v) => json!({ "node": "flow", "status": "complete", "says": format!("{v} m/s") }),
            None => json!({ "node": "flow", "status": "incomplete", "says": "no speed, and with nothing moving there is no flow" }),
        },
        SavedKind::Stress => {
            let n = study.setup.get("loads").and_then(Value::as_array).map_or(0, Vec::len);
            if n == 0 {
                json!({ "node": "loads", "status": "incomplete", "says": "nothing is held and nothing pushes" })
            } else {
                json!({ "node": "loads", "status": "complete", "says": format!("{n}") })
            }
        }
    };

    // The lattice and the solver's own limits have defaults that work, so leaving them alone is
    // a choice rather than an omission.
    let cells = num(&setup, "cells").map_or_else(|| "default".to_string(), |v| format!("{v}"));
    json!([
        { "node": "bodies", "status": "complete", "says": format!("{}", picked.len()) },
        materials,
        driver,
        { "node": "lattice", "status": "optional", "says": cells },
    ])
}

/// Whether anything about a study stops it running.
fn study_ready(nodes: &Value) -> bool {
    nodes.as_array().is_none_or(|nodes| !nodes.iter().any(|n| matches!(n.get("status").and_then(Value::as_str), Some("incomplete" | "error"))))
}

fn study_create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.study.create";
    let Some(given) = str_(p, "kind") else {
        return Err(bad(cmd, "`kind` is required: thermal, flow or stress"));
    };
    let Some(kind) = SavedKind::from_id(given) else {
        return Err(bad(cmd, format!("`{given}` is not a kind of study: thermal, flow or stress")));
    };
    let setup = study_setup(cmd, p)?;
    let name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => next_study_name(s, kind),
    };
    if s.doc.analysis.studies.contains_key(&name) {
        return Err(bad(cmd, format!("a study called `{name}` is already here; `sim.study.set` changes one")));
    }
    // Built aside and checked before the design is touched, so a refused study leaves the
    // design exactly as it was.
    let mut studies = s.doc.analysis.clone();
    studies.studies.insert(name.clone(), SavedStudy { kind, setup });
    studies.validate().map_err(|e| bad(cmd, format!("{e}")))?;
    s.doc_mut().analysis = studies;
    Ok(json!({ "study": name, "kind": kind.id() }))
}

fn study_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.study.set";
    let name = study_name(cmd, p)?;
    if !s.doc.analysis.studies.contains_key(&name) {
        return Err(unknown_study(cmd, s, &name));
    }
    let setup = study_setup(cmd, p)?;
    let mut studies = s.doc.analysis.clone();
    if let Some(study) = studies.studies.get_mut(&name) {
        study.setup = setup;
    }
    studies.validate().map_err(|e| bad(cmd, format!("{e}")))?;
    s.doc_mut().analysis = studies;
    Ok(json!({ "study": name }))
}

fn study_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.study.delete";
    let name = study_name(cmd, p)?;
    if s.doc_mut().analysis.studies.remove(&name).is_none() {
        return Err(unknown_study(cmd, s, &name));
    }
    Ok(json!({ "study": name }))
}

/// What every saved study holds, and what each still needs.
///
/// Public, and not only a command, because the browser shows this: going through
/// `Session::execute` to draw a panel would run a command every frame, and this way the tree
/// and the run are reading the same answer rather than two that have to be kept in step.
pub fn studies_status(s: &Session) -> Value {
    let studies: Vec<Value> = s
        .doc
        .analysis
        .studies
        .iter()
        .map(|(name, study)| {
            let nodes = study_nodes(s, study);
            json!({
                "study": name,
                "kind": study.kind.id(),
                "command": study.kind.command(),
                "ready": study_ready(&nodes),
                "nodes": nodes,
                "setup": Value::Object(study.setup.clone()),
            })
        })
        .collect();
    json!({ "studies": studies })
}

fn study_list(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(studies_status(s))
}

fn study_run(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.study.run";
    let name = study_name(cmd, p)?;
    let Some(study) = s.doc.analysis.studies.get(&name) else {
        return Err(unknown_study(cmd, s, &name));
    };
    let (kind, setup) = (study.kind, Value::Object(study.setup.clone()));
    let answer = match kind {
        SavedKind::Thermal => thermal_study(s, &setup),
        SavedKind::Flow => flow_study(s, &setup),
        SavedKind::Stress => stress_study(s, &setup),
    }?;
    Ok(json!({ "study": name, "kind": kind.id(), "result": answer }))
}

/// The result the viewport is drawing, or why there is none to look at.
fn drawn_result<'a>(cmd: &str, s: &'a mut Session) -> Result<&'a mut AnalysisResult> {
    let revision = s.revision;
    if s.analysis_result.as_ref().is_some_and(|r| r.revision != revision) {
        return Err(bad(cmd, "the model has changed since this result was worked out, so nothing is being drawn; run the study again"));
    }
    s.analysis_result
        .as_mut()
        .ok_or_else(|| bad(cmd, "no analysis result to look at: run `sim.thermal`, `sim.flow` or `sim.stress` first, or `sim.study.run`"))
}

/// Say that the drawn result has changed, so the viewport rebuilds its scene.
///
/// The same reckoning as [`show`]: the revision is about what is on screen, so moving it is how
/// the viewport is told there is something new to draw. The design is untouched and no undo step
/// is taken -- none of these commands changes what the part is.
fn redrawn(s: &mut Session) {
    s.revision += 1;
    let revision = s.revision;
    if let Some(r) = s.analysis_result.as_mut() {
        r.revision = revision;
    }
}

/// Which field a selector names: its name, however it is capitalised, or its number from one.
fn pick_field(cmd: &str, given: &Value, names: &[String]) -> Result<usize> {
    let listed = names.join(", ");
    if let Some(n) = given.as_f64() {
        // Checked as a number before it is made into an index. A float-to-integer cast
        // saturates, so `-1e308 as isize` is `isize::MIN`, and subtracting one from that
        // overflows -- which panics in every build that has overflow checks on, and those are
        // the builds the tests and CI run.
        if n.is_finite() && n >= 1.0 && n <= names.len() as f64 {
            return Ok(n as usize - 1);
        }
        return Err(bad(cmd, format!("`field` {n} is not one of the {} this run worked out: {listed}", names.len())));
    }
    let Some(want) = given.as_str() else {
        return Err(bad(cmd, format!("`field` must name one of: {listed}")));
    };
    let want = want.to_lowercase();
    if let Some(at) = names.iter().position(|n| n.to_lowercase() == want) {
        return Ok(at);
    }
    // Part of the name is enough while it picks out one, from either end: `temp` for
    // temperature, `flux` for heat flux, `mises` for von Mises stress. Two matches are refused
    // rather than guessed between, and the refusal names them.
    let hits: Vec<(usize, &String)> = names.iter().enumerate().filter(|(_, n)| n.to_lowercase().contains(&want)).collect();
    match hits.as_slice() {
        [(at, _)] => Ok(*at),
        [] => Err(bad(cmd, format!("no field `{want}`; this run worked out: {listed}"))),
        several => {
            let both = several.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(" and ");
            Err(bad(cmd, format!("`field` `{want}` could be {both}; name one of them")))
        }
    }
}

/// The middle of what a result covers, where a cut plane sits when it is not told where.
fn middle_of(r: &AnalysisResult) -> Option<Vec3> {
    if let Some(l) = r.lattice {
        let [nx, ny, nz] = l.count;
        return Some(l.origin + Vec3::new(nx as f64, ny as f64, nz as f64) * (l.spacing / 2.0));
    }
    if r.skin.is_empty() {
        return None;
    }
    let mut sum = Vec3::new(0.0, 0.0, 0.0);
    for face in &r.skin {
        sum += face.at;
    }
    Some(sum / r.skin.len() as f64)
}

/// How a result is being looked at, as a reply: what is shaded, over what range, and how much of
/// it reached the picture.
fn view_json(r: &AnalysisResult) -> Value {
    json!({
        "field": r.quantity,
        "unit": r.unit,
        "fields": r.fields.iter().map(|f| f.quantity.clone()).collect::<Vec<_>>(),
        "low": r.low,
        "high": r.high,
        "bands": r.view.bands,
        "cut": r.view.cut.map(|(at, n)| json!({"at": [at.x, at.y, at.z], "normal": [n.x, n.y, n.z]})),
        "iso": r.view.iso,
        "streamlines": r.view.streamlines,
        // False when the lattice was too big to keep, and then a cut shows the inside of the
        // surface but no field across it, and a point cannot be probed at all.
        "inside": r.lattice.is_some(),
        "squares": r.patches.len(),
        "arrows": r.arrows.len(),
        "lines": r.lines.len(),
        "probes": r.probes.iter().map(probe_json).collect::<Vec<_>>(),
    })
}

fn probe_json(p: &crate::AnalysisProbe) -> Value {
    json!({"at": [p.at.x, p.at.y, p.at.z], "value": p.value})
}

/// Choose which field is shaded, over what range, and how much of the result is drawn.
///
/// Nothing here runs a solver again. A run keeps every field it worked out on the lattice it
/// solved on, so another quantity, a narrower scale, a cut through the part or a threshold is a
/// different reading of the same answer rather than a different answer.
fn result_show(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.result.show";
    // Everything is read and checked before anything is changed, so a refused parameter leaves
    // the view as it was rather than half moved.
    let (names, span, middle) = {
        let r = drawn_result(cmd, s)?;
        (r.fields.iter().map(|f| f.quantity.clone()).collect::<Vec<_>>(), (r.low, r.high), middle_of(r))
    };
    let mut view = drawn_result(cmd, s)?.view.clone();

    if let Some(given) = p.get("field") {
        let chosen = pick_field(cmd, given, &names)?;
        // A scale pinned by hand belongs to the field it was pinned for. Carried on to another
        // one it is not a scale but a lie: nought to a hundred degrees over a heat flux of a
        // hundred and sixty thousand watts a square metre paints the whole part one colour.
        // Given both at once, the call means what it says and the range stands.
        if chosen != view.field && p.get("low").is_none() && p.get("high").is_none() {
            view.range = None;
        }
        view.field = chosen;
    }

    // The two ends of the scale. Either may be given on its own; `range: false` stretches it
    // back over whatever the field holds.
    if p.get("range").is_some_and(|v| v == &Value::Bool(false)) {
        view.range = None;
    }
    if p.get("low").is_some() || p.get("high").is_some() {
        let (was_low, was_high) = view.range.unwrap_or(span);
        let low = match p.get("low") {
            None => was_low,
            Some(_) => big(p, "low").ok_or_else(|| bad(cmd, "`low` must be a number: the comfortable end of the scale"))?,
        };
        let high = match p.get("high") {
            None => was_high,
            Some(_) => big(p, "high").ok_or_else(|| bad(cmd, "`high` must be a number: the bad end of the scale"))?,
        };
        if low == high {
            return Err(bad(cmd, "`low` and `high` must differ; a scale with no span cannot colour anything"));
        }
        view.range = Some((low, high));
    }

    match p.get("bands") {
        None => {}
        Some(Value::Bool(false)) => view.bands = None,
        Some(given) => {
            let n = given
                .as_f64()
                .filter(|n| n.is_finite() && (2.0..=64.0).contains(n))
                .ok_or_else(|| bad(cmd, "`bands` must be 2 to 64 steps, or false for a smooth scale"))?;
            view.bands = Some(n.round() as u32);
        }
    }

    match p.get("cut") {
        None => {}
        Some(Value::Bool(false)) => view.cut = None,
        Some(given) => {
            if !given.is_object() {
                return Err(bad(cmd, "`cut` must be {at?: [x,y,z], normal?: [x,y,z]}, or false to put the whole model back"));
            }
            let normal = match given.get("normal") {
                None => Vec3::new(0.0, 1.0, 0.0),
                Some(_) => big_vec3(given.get("normal")).ok_or_else(|| bad(cmd, "`cut.normal` must be [x,y,z]"))?,
            };
            let normal = normal.normalized().ok_or_else(|| bad(cmd, "`cut.normal` must point somewhere; [0,0,0] is not a direction"))?;
            let at = match given.get("at") {
                Some(a) => vec3(a).ok_or_else(|| bad(cmd, "`cut.at` must be a point [x,y,z] on the plane"))?,
                None => middle.ok_or_else(|| bad(cmd, "this result covers nothing, so there is nowhere to put a cut"))?,
            };
            view.cut = Some((at, normal));
        }
    }

    match p.get("iso") {
        None => {}
        Some(Value::Bool(false)) => view.iso = None,
        Some(_) => {
            let at = big(p, "iso").ok_or_else(|| bad(cmd, "`iso` must be a value to show the part past, or false to show all of it"))?;
            view.iso = Some(at);
        }
    }

    if let Some(on) = crate::params::bool_(p, "streamlines") {
        view.streamlines = on;
    }

    let reply = {
        let r = drawn_result(cmd, s)?;
        r.view = view;
        r.redraw();
        view_json(r)
    };
    redrawn(s);
    Ok(reply)
}

/// Read the shown field at a point, and leave the point marked.
fn result_probe(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.result.probe";
    if crate::params::bool_(p, "clear").unwrap_or(false) {
        let reply = {
            let r = drawn_result(cmd, s)?;
            r.view.probes.clear();
            r.redraw();
            view_json(r)
        };
        redrawn(s);
        return Ok(reply);
    }
    let at = crate::params::req_vec3(cmd, p, "at")?;
    let reply = {
        let r = drawn_result(cmd, s)?;
        if r.lattice.is_none() {
            return Err(bad(
                cmd,
                "this result kept only the values on the model's surface, because its lattice was too big to hold; run the \
                 study with fewer `cells` to be able to probe inside it",
            ));
        }
        if r.view.probes.len() >= crate::MAX_PROBES {
            return Err(bad(cmd, format!("{} points are already marked; `clear: true` drops them", crate::MAX_PROBES)));
        }
        r.view.probes.push(at);
        r.redraw();
        let found = r.probes.last().and_then(|x| x.value);
        json!({
            "at": [at.x, at.y, at.z],
            "field": r.quantity,
            "unit": r.unit,
            // Null when the point is off the lattice, or in a cell this field has no value in:
            // outside the model for a temperature, inside it for the pressure of the air.
            "value": found,
            "points": r.probes.iter().map(probe_json).collect::<Vec<_>>(),
        })
    };
    redrawn(s);
    Ok(reply)
}

/// The lowest, highest and mean of a field over the places it has a value, and how many of them
/// there are.
///
/// The cells of the lattice are all the same size and the squares of its surface are all the
/// same square, so a plain mean is already the volume-weighted one over the model and the
/// area-weighted one over its surface. That is one of the things a lattice buys.
fn summary(values: &[f32]) -> Value {
    let (mut low, mut high, mut sum, mut count) = (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0u64);
    for v in values {
        let v = f64::from(*v);
        if v.is_finite() {
            low = low.min(v);
            high = high.max(v);
            sum += v;
            count += 1;
        }
    }
    if count == 0 {
        return json!({"count": 0});
    }
    json!({"count": count, "low": low, "high": high, "mean": sum / count as f64})
}

/// Which faces of the model a call picked out: each by body and number, or by a point on it.
///
/// A point is matched to the nearest square of the lattice's surface that belongs to a face,
/// which is the same reckoning a condition set by a point uses -- a pick and a point on what was
/// picked should mean the same thing.
fn picked_faces(cmd: &str, r: &AnalysisResult, given: &Value) -> Result<Vec<(u16, u32)>> {
    let items = given.as_array().ok_or_else(|| bad(cmd, "`faces` must be a list of {body?, face} or [x,y,z]"))?;
    if items.len() > MAX_PICKS {
        return Err(bad(cmd, format!("{} faces, and at most {MAX_PICKS} may be picked", items.len())));
    }
    let mut out: Vec<(u16, u32)> = Vec::new();
    for item in items {
        let found = if let Some(at) = vec3(item) {
            let mut best: Option<(f64, (u16, u32))> = None;
            for square in &r.skin {
                let Some(tag) = square.face else { continue };
                let away = (square.at - at).len();
                if best.is_none_or(|(near, _)| away < near) {
                    best = Some((away, tag));
                }
            }
            best.map(|(_, tag)| tag).ok_or_else(|| bad(cmd, "no face of the model is near that point"))?
        } else if item.is_object() {
            let face = num(item, "face")
                .filter(|n| *n >= 0.0 && n.fract() == 0.0)
                .ok_or_else(|| bad(cmd, "a picked face needs `face`: its number on the body (`sim.lattice` lists them)"))?
                as u32;
            let body = match str_(item, "body") {
                None if r.bodies.len() == 1 => 0u16,
                None => return Err(bad(cmd, format!("a picked face needs `body`, one of: {}", r.bodies.join(", ")))),
                Some(name) => r
                    .bodies
                    .iter()
                    .position(|b| b == name)
                    .ok_or_else(|| bad(cmd, format!("no body `{name}` in this study; it ran on: {}", r.bodies.join(", "))))?
                    as u16,
            };
            (body, face)
        } else {
            return Err(bad(cmd, "a picked face is {body?, face} or a point [x,y,z] on it"));
        };
        if !out.contains(&found) {
            out.push(found);
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, "`faces` picked nothing"));
    }
    Ok(out)
}

/// A field added up over the squares of one face of the model.
fn face_summary(r: &AnalysisResult, values: &[f32], want: (u16, u32)) -> Value {
    let kept: Vec<f32> = r.skin.iter().zip(values).filter(|(square, _)| square.face == Some(want)).map(|(_, value)| *value).collect();
    summary(&kept)
}

/// What a field comes to, as numbers rather than as a picture.
fn result_stats(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.result.stats";
    let r = drawn_result(cmd, s)?;
    let names: Vec<String> = r.fields.iter().map(|f| f.quantity.clone()).collect();
    let which = match p.get("field") {
        Some(given) => pick_field(cmd, given, &names)?,
        None => r.view.field,
    };
    let f = r.fields.get(which).ok_or_else(|| bad(cmd, "this run worked out no fields"))?;
    // Over the faces someone picked, where any were: a mean over one face is the number a
    // decision is usually made on, and a mean over the whole skin is not that number.
    let (picked, faces) = match p.get("faces") {
        None => (Value::Null, Value::Null),
        Some(given) => {
            let wanted = picked_faces(cmd, r, given)?;
            let over: Vec<f32> = r
                .skin
                .iter()
                .zip(&f.surface)
                .filter(|(square, _)| square.face.is_some_and(|tag| wanted.contains(&tag)))
                .map(|(_, value)| *value)
                .collect();
            let each: Vec<Value> = wanted
                .iter()
                .map(|(body, face)| {
                    let mut row = face_summary(r, &f.surface, (*body, *face));
                    if let Some(map) = row.as_object_mut() {
                        map.insert("body".into(), json!(r.bodies.get(*body as usize)));
                        map.insert("face".into(), json!(face));
                    }
                    row
                })
                .collect();
            (summary(&over), Value::Array(each))
        }
    };
    Ok(json!({
        "fields": names,
        "field": f.quantity,
        "unit": f.unit,
        "about": r.about,
        // Over every cell the field has a value in, cell by cell: the model for a conduction or
        // a strength study, and the whole wind tunnel for a flow, whose field is the air. Empty
        // when the lattice was too big to keep. Not called "model", which for a flow would put
        // the undisturbed far field into a figure that reads as being about the part.
        "volume": summary(&f.cells),
        // Over the model's whole surface, square by square. This one is about the part in
        // every study.
        "surface": summary(&f.surface),
        // Over the faces `faces` picked, and then each of them on its own.
        "picked": picked,
        "faces": faces,
        "worst": f.worst.map(|at| json!([at.x, at.y, at.z])),
    }))
}

#[cfg(test)]
#[path = "sim_tests.rs"]
mod tests;
