//! `motion.simulate` (RFC #9719): the assembly's moving rigid groups simulated as rigid bodies
//! under gravity, through its joints and motion links, by `solidcraft_dynamics`.
//!
//! Each rigid group `assembly.mass_properties` finds becomes a body (mass, centre of mass and
//! inertia about it in the group's frame axes); fixed groups are the ground. A joint between two
//! groups becomes a solver joint between frames on them: A's frame where its origin is, and B's
//! with the mate (`flip_of`) taken off, so `B's frame = A's frame · motion(values)` as the joint
//! solver has it and the solver's values are the joint's. A joint inside one group, or between
//! two fixed groups, holds nothing that moves and is left out. Motion links between simulated
//! joints become linear links at their ratio, keeping the offset they have now. The run starts
//! where the assembly is, at rest; the design does not move.

use std::collections::BTreeMap;

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::joints::{self, Coupling, JointKind};
use solidcraft_doc::{Document, Mat, mat_inverse, mat_mul};
use solidcraft_dynamics as dyn_;
use solidcraft_dynamics::{Body, Coord, Link, Marker, Mat3, Model, Quat, STANDARD_GRAVITY, Settings};

use super::CommandSpec;
use super::mass_properties::assembly;
use crate::params::bad;
use crate::schema::motion_simulate::{GroupPose, JointValues, SimulateParams, SimulateResult, SimulatedGroup, SimulationEnergy, SimulationSample};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CMD: &str = "motion.simulate";
const CONTRACT: TypedContract<SimulateParams> = TypedContract::new();

/// Longest run, s.
const MAX_DURATION: f64 = 3600.0;
/// Most numbers a run returns (samples × (groups + joint values)).
const MAX_OUTPUT: usize = 2_000_000;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Simulate Motion", crate::checked!("motion.simulate", run))
    .noundo()
    .enabled(enabled)
    .contract(&CONTRACT)
    .output(output_schema_for::<SimulateResult>)
    .params(
        "duration: s (> 0, at most 3600); step?: s (default 0.001); output_interval?: s (default duration/100, at least the step); \
         gravity?: [x,y,z] mm/s² (default [0,0,-9806.65]) → groups: [{group, occurrences, mass_kg}] (the moving rigid groups, as \
         assembly.mass_properties finds them), joints: [ids] and links: [indices] simulated, samples: [{time, groups: [{group, \
         origin, x, y, z}] (each moving group's frame, world mm), joints: [{joint, values}] (deg and mm), energy: {kinetic, \
         potential} (J)}], steps, max_constraint_error_mm, redundant_equations, warnings. Starts where the assembly is, at rest, \
         every body at its material's density; revolute, slider, cylindrical, planar, ball and rigid joints between rigid groups, \
         and gear, rack-and-pinion and screw links; a joint inside one rigid group or between fixed groups is left out. Read-only: \
         the design does not move",
    )];

fn enabled(s: &Session) -> std::result::Result<(), String> {
    if s.doc.occurrences.iter().any(|o| !o.grounded) { Ok(()) } else { Err("needs a component that is not grounded".into()) }
}

/// A frame (column-major `Mat`) as origin and axes.
fn frame_parts(m: &Mat) -> (Vec3, Vec3, Vec3, Vec3) {
    let col = |c: usize| m.get(c).map_or(Vec3::ZERO, |v| Vec3::new(v[0], v[1], v[2]));
    (col(3), col(0), col(1), col(2))
}

/// A moving body's place: its centre of mass (world) and axes (body → world), and its centre in
/// its group frame's coordinates.
#[derive(Clone, Copy)]
struct Placed {
    index: usize,
    center: Vec3,
    axes: Mat3,
    local_center: Vec3,
}

/// What the solver's model maps back to.
pub(crate) struct Mapping {
    /// Group index → its body, `None` for a fixed group.
    bodies: BTreeMap<usize, Placed>,
    /// The joints simulated: (id, kind), in solver order.
    joints: Vec<(u64, JointKind)>,
    links: Vec<usize>,
    groups: Vec<SimulatedGroup>,
    warnings: Vec<String>,
}

fn arr(v: [f64; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

/// The design (pending moves applied) as a dynamics model with the given gravity (mm/s²).
pub(crate) fn model_of(s: &Session, gravity: Vec3) -> Result<(Model, Mapping)> {
    let mut doc: Document = (*s.doc).clone();
    for occurrence in &mut doc.occurrences {
        if let Some(transform) = s.pending_moves.get(&occurrence.id) {
            occurrence.transform = *transform;
        }
    }
    let state = s.model.state();
    let mass = assembly(&doc, &state)?;
    let group_of: BTreeMap<u64, usize> = mass.occurrences.iter().map(|o| (o.occurrence, o.group)).collect();
    let mut model = Model { gravity, ..Model::default() };
    let mut map = Mapping { bodies: BTreeMap::new(), joints: Vec::new(), links: Vec::new(), groups: Vec::new(), warnings: mass.warnings.clone() };
    for g in &mass.groups {
        if g.fixed {
            continue;
        }
        let names = || g.occurrences.iter().map(|o| o.to_string()).collect::<Vec<_>>().join(", ");
        let Some(m) = g.mass else {
            return Err(bad(CMD, format!("the rigid group of occurrences {} has no mass: give its bodies a closed solid", names())));
        };
        let Some(inertia) = m.inertia_local_kg_mm2 else {
            return Err(bad(CMD, format!("the rigid group of occurrences {} has no inertia (an open mesh or a scaled frame)", names())));
        };
        let axes = Mat3::from_cols(arr(g.frame.x), arr(g.frame.y), arr(g.frame.z));
        let orientation =
            Quat::from_mat3(&axes).ok_or_else(|| bad(CMD, format!("the rigid group of occurrences {} is not placed by a rotation", names())))?;
        let center = arr(m.center_of_mass_mm);
        let index = model.bodies.len();
        model.bodies.push(Body {
            mass: m.mass_kg,
            inertia: Mat3(inertia),
            position: center,
            orientation,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        });
        // The axes the body really has (the frame's, made exactly orthonormal).
        map.bodies.insert(g.group, Placed { index, center, axes: orientation.to_mat3(), local_center: arr(m.center_of_mass_local_mm) });
        map.groups.push(SimulatedGroup { group: g.group, occurrences: g.occurrences.clone(), mass_kg: m.mass_kg });
    }
    if model.bodies.is_empty() {
        return Err(bad(CMD, "nothing can move: every rigid group is grounded or tied to the ground"));
    }
    // A world frame as a marker on a group's body (or the ground).
    let marker = |group: usize, w: &Mat| -> Marker {
        let (o, x, _, z) = frame_parts(w);
        match map.bodies.get(&group) {
            Some(p) => {
                let back = p.axes.transpose();
                Marker::new(Some(p.index), back.mul_vec(o - p.center), back.mul_vec(z), back.mul_vec(x))
            }
            None => Marker::ground(o, z, x),
        }
    };
    let mut simulated: BTreeMap<u64, usize> = BTreeMap::new();
    for j in doc.assembly.joints.iter().filter(|j| !j.suppressed) {
        let (Some(&ga), Some(&gb)) = (group_of.get(&j.a.occurrence), group_of.get(&j.b.occurrence)) else {
            return Err(bad(CMD, format!("joint `{}` names an occurrence that is not in the assembly", j.name)));
        };
        if ga == gb || (!map.bodies.contains_key(&ga) && !map.bodies.contains_key(&gb)) {
            continue;
        }
        let kind = match j.kind {
            JointKind::Rigid => dyn_::JointKind::Rigid,
            JointKind::Revolute => dyn_::JointKind::Revolute,
            JointKind::Slider => dyn_::JointKind::Slider,
            JointKind::Cylindrical => dyn_::JointKind::Cylindrical,
            JointKind::Planar => dyn_::JointKind::Planar,
            JointKind::Ball => dyn_::JointKind::Ball,
            other => {
                return Err(bad(
                    CMD,
                    format!(
                        "not supported yet: joint `{}` is a {other:?} joint; simulation takes revolute, slider, cylindrical, planar, ball and rigid joints",
                        j.name
                    ),
                ));
            }
        };
        let resolve = |o: &joints::JointOrigin| {
            joints::resolve_origin(&doc, &state, o).map_err(|e| bad(CMD, format!("joint `{}`: its origin is lost ({e})", j.name)))
        };
        let (fa, fb) = (resolve(&j.a)?, resolve(&j.b)?);
        let wa = mat_mul(&joints::occurrence_world(&doc, j.a.occurrence), &fa);
        let unmate = mat_inverse(&joints::flip_of(j)).ok_or_else(|| bad(CMD, format!("joint `{}`: its offset can't be undone", j.name)))?;
        let wb = mat_mul(&mat_mul(&joints::occurrence_world(&doc, j.b.occurrence), &fb), &unmate);
        simulated.insert(j.id, model.joints.len());
        model.joints.push(dyn_::Joint { kind, a: marker(ga, &wa), b: marker(gb, &wb) });
        map.joints.push((j.id, j.kind));
    }
    for (i, l) in doc.assembly.links.iter().enumerate() {
        if matches!(l.coupling, Some(Coupling::Cam(_))) {
            return Err(bad(CMD, format!("not supported yet: motion link {i} is a cam")));
        }
        let (Some(&a), Some(&b)) = (simulated.get(&l.a), simulated.get(&l.b)) else {
            // A link between joints that hold nothing moving moves nothing either.
            if simulated.contains_key(&l.a) || simulated.contains_key(&l.b) {
                return Err(bad(CMD, format!("not supported yet: motion link {i} ties a simulated joint to one inside a rigid group")));
            }
            continue;
        };
        model.links.push(Link { driver: Coord::new(a, l.ia), follower: Coord::new(b, l.ib), ratio: l.ratio, offset: None });
        map.links.push(i);
    }
    Ok((model, map))
}

fn finite_positive(cmd_field: &str, v: f64) -> Result<f64> {
    if v.is_finite() && v > 0.0 { Ok(v) } else { Err(bad(CMD, format!("`{cmd_field}` must be a positive number of seconds"))) }
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let params: SimulateParams = serde_json::from_value(p.clone()).map_err(|e| bad(CMD, e.to_string()))?;
    let duration = finite_positive("duration", params.duration)?;
    if duration > MAX_DURATION {
        return Err(bad(CMD, format!("`duration` is at most {MAX_DURATION} s")));
    }
    let step = finite_positive("step", params.step.unwrap_or(1e-3))?;
    let output_interval = finite_positive("output_interval", params.output_interval.unwrap_or(duration / 100.0))?.max(step);
    let gravity = match params.gravity {
        Some(g) if g.iter().all(|c| c.is_finite() && c.abs() < 1e9) => arr(g),
        Some(_) => return Err(bad(CMD, "`gravity` must be three finite numbers (mm/s²)")),
        None => Vec3::new(0.0, 0.0, -STANDARD_GRAVITY),
    };
    let settings = Settings::rk4(duration, step, output_interval);
    settings.validate().map_err(|e| bad(CMD, e.to_string()))?;
    let (model, map) = model_of(s, gravity)?;
    let samples = (duration / output_interval).ceil() + 1.0;
    let per_sample = map.bodies.len() * 12 + model.joints.len() * 3 + 2;
    if !(samples * per_sample as f64 <= MAX_OUTPUT as f64) {
        return Err(bad(CMD, format!("the run would return more than {MAX_OUTPUT} numbers: take a longer output_interval")));
    }
    let traj = dyn_::simulate(&model, &settings).map_err(|e| bad(CMD, e.to_string()))?;
    let out = SimulateResult {
        groups: map.groups.clone(),
        joints: map.joints.iter().map(|(id, _)| *id).collect(),
        links: map.links.clone(),
        samples: traj.samples.iter().map(|smp| sample(&map, smp)).collect(),
        steps: traj.stats.steps,
        max_constraint_error_mm: traj.stats.max_constraint_error,
        redundant_equations: traj.stats.redundant_equations,
        warnings: map.warnings.clone(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(out).map_err(|e| EngineError::Other(e.to_string()))
}

fn sample(map: &Mapping, s: &dyn_::Sample) -> SimulationSample {
    let v = |v: Vec3| [v.x, v.y, v.z];
    let groups = map
        .bodies
        .iter()
        .filter_map(|(group, p)| {
            let b = s.bodies.get(p.index)?;
            let r = b.orientation.to_mat3();
            // The group frame's origin sits at −(its centre in frame coordinates) from the centre.
            let origin = b.position - r.mul_vec(p.local_center);
            Some(GroupPose { group: *group, origin: v(origin), x: v(r.col(0)), y: v(r.col(1)), z: v(r.col(2)) })
        })
        .collect();
    let joints = map
        .joints
        .iter()
        .zip(&s.joints)
        .map(|((id, kind), j)| JointValues {
            joint: *id,
            values: j.values.iter().enumerate().map(|(i, x)| if kind.is_angle(i) { x.to_degrees() } else { *x }).collect(),
        })
        .collect();
    SimulationSample {
        time: s.time,
        groups,
        joints,
        energy: SimulationEnergy { kinetic: s.energy.kinetic * 1e-6, potential: s.energy.gravity * 1e-6 },
    }
}

#[cfg(test)]
#[path = "motion_simulate_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
