//! The design's joints, grounding and masses as CADIR physics records, for an OpenUSD export
//! that asks for them ([`crate::CadmpegOptions::physics`]); cadmpeg's writer writes them as
//! `UsdPhysics` joints, rigid bodies and mass.
//!
//! Joints: each joint is written between the two placements it joins, at the first placement of
//! each (the one the design's own joint solver moves). Its two frames are those the solver
//! mates: A's frame with its lateral offset, and B's frame with the mate (turn, offset and flip)
//! taken off, so at the joint's zero the two frames coincide and its values and limits mean what
//! they mean in SolidCraft. A side on the design itself is the world. Rigid, revolute, slider,
//! cylindrical, planar and ball joints have an OpenUSD form; the writer keeps the limits of
//! revolute and slider joints only. A joint of another kind, a suppressed joint and a motion link
//! (gear, rack and pinion, screw, cam) are left out, each with a warning. A grounded placement is
//! fixed to the world, in every placement written.
//!
//! Mass: each component placed in the design gets its bodies' mass, centre of mass and principal
//! inertia, from each body's density (its material's) and its closed mesh at standard quality, as
//! URDF export's links, and each placement of such a component is a rigid body, joined or not.
//! The design's own bodies get none: they never move.

use std::collections::HashMap;

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{MassPropertiesId, OccurrenceId, PhysicsSceneId, ProductDefinitionId, RigidBodyId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::physics::{MassProperties, PhysicsScene, PhysicsTarget, PrincipalInertia, RigidBody};
use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointId, JointLimits, JointOperand, PairedJointKind, PlaneSense};
use cadmpeg_ir::scalar::{NonNegativeReal, PositiveReal};
use cadmpeg_ir::transform::Transform;
use solidcraft_doc::joints::{self, Joint, JointKind};
use solidcraft_doc::{Document, IDENTITY, Mat, ModelState, mat_inverse, mat_mul};

use crate::urdf::BodyMass;

/// The physics records of a design, and what they leave out.
#[derive(Default)]
pub(super) struct Physics {
    pub joints: Vec<AssemblyJoint>,
    pub masses: Vec<MassProperties>,
    pub rigid_bodies: Vec<RigidBody>,
    pub scenes: Vec<PhysicsScene>,
    pub warnings: Vec<String>,
}

/// An id of this design's model, as [`super::cadir_out`] makes them.
fn id<T: TryFrom<String>>(kind: &str, key: usize) -> Result<T, String> {
    let s = format!("solidcraft:design:{kind}#{key:09}");
    T::try_from(s.clone()).map_err(|_| format!("{s} is not a CADIR identity"))
}

/// `m` (column-major, mm) as a CADIR transform, when finite.
fn transform(m: &Mat) -> Option<Transform> {
    let row = |r: usize| [m[0][r], m[1][r], m[2][r], m[3][r]];
    Transform::affine([row(0), row(1), row(2)])
}

/// The joints, grounding and masses of `doc` (`model`, the model before placing, holds each
/// component's bodies in its frame). `placed` gives the CADIR occurrence of each placement path,
/// `definitions` the CADIR definition of each component, `masses` each component's bodies'
/// masses in its frame.
pub(super) fn build(
    doc: &Document,
    model: &ModelState,
    placed: &HashMap<Vec<u64>, OccurrenceId>,
    definitions: &HashMap<u64, ProductDefinitionId>,
    masses: &HashMap<u64, Vec<BodyMass>>,
) -> Result<Physics, String> {
    let mut out = Physics::default();
    let operand = |occurrence: u64| -> Option<JointOperand> {
        if occurrence == 0 {
            return Some(JointOperand::root("design", Vec::new()));
        }
        let path = crate::urdf::canonical(doc, occurrence);
        placed.get(&path).map(|id| JointOperand::occurrence(id.clone(), "placement", Vec::new()))
    };
    let all = doc.joints_with_rigid_groups();
    for j in all.iter() {
        if j.suppressed {
            out.warnings.push(format!("joint `{}`: suppressed: left out of the physics", j.name));
            continue;
        }
        let Some(kind) = paired_kind(j, &mut out.warnings) else { continue };
        let connectors = match (connectors(doc, model, j), operand(j.a.occurrence), operand(j.b.occurrence)) {
            (Some([fa, fb]), Some(a), Some(b)) if j.a.occurrence != j.b.occurrence => {
                [JointConnector { operand: a, frame: fa, detached: false }, JointConnector { operand: b, frame: fb, detached: false }]
            }
            _ => {
                out.warnings.push(format!(
                    "joint `{}`: its origins don't resolve, its placements are not written, or it joins a placement to itself: left out of the physics",
                    j.name
                ));
                continue;
            }
        };
        let mut joint = AssemblyJoint::paired(id::<JointId>("joint", out.joints.len())?, kind, connectors, None);
        joint.native_ref = Some(format!("solidcraft:design:joint/{}", j.name.replace('/', "_")));
        out.joints.push(joint);
    }
    if !doc.assembly.links.is_empty() {
        out.warnings
            .push(format!("{} motion links (gears, racks, screws, cams) have no OpenUSD physics form here: left out", doc.assembly.links.len()));
    }
    // Every written placement of a grounded occurrence is fixed: a subassembly placed twice
    // grounds its grounded part in both, as the design does.
    let grounded: HashMap<u64, &str> = doc.occurrences.iter().filter(|o| o.grounded).map(|o| (o.id, o.name.as_str())).collect();
    let mut paths: Vec<(&Vec<u64>, &OccurrenceId)> = placed.iter().collect();
    paths.sort();
    for (path, occurrence) in &paths {
        let Some(name) = path.last().and_then(|o| grounded.get(o)) else { continue };
        let operand = JointOperand::occurrence((*occurrence).clone(), "placement", Vec::new());
        let connector = JointConnector { operand, frame: Transform::identity(), detached: false };
        let mut joint = AssemblyJoint::grounded(id::<JointId>("joint", out.joints.len())?, connector, None);
        joint.native_ref = Some(format!("solidcraft:design:joint/{} grounded", name.replace('/', "_")));
        out.joints.push(joint);
    }
    // Every written placement of a component with mass moves as a rigid body, joined or not
    // (the writer makes rigid bodies of joined placements alone); the design itself stays put.
    let component_of: HashMap<u64, u64> = doc.occurrences.iter().map(|o| (o.id, o.component)).collect();
    for (path, occurrence) in &paths {
        let Some(component) = path.last().and_then(|o| component_of.get(o)) else { continue };
        if masses.get(component).is_none_or(Vec::is_empty) {
            continue;
        }
        out.rigid_bodies.push(RigidBody {
            id: id::<RigidBodyId>("rigid-body", out.rigid_bodies.len())?,
            target: PhysicsTarget::Occurrence { occurrence: (*occurrence).clone() },
            kinematic: false,
            enabled: true,
            linear_velocity: None,
            angular_velocity: None,
            starts_asleep: false,
            native_ref: None,
        });
    }
    let mut components: Vec<(&u64, &Vec<BodyMass>)> = masses.iter().filter(|(c, _)| **c != 0).collect();
    components.sort_by_key(|(c, _)| **c);
    for (component, bodies) in components {
        let Some(definition) = definitions.get(component) else { continue };
        let Some(inertial) = crate::urdf::inertial(bodies, &IDENTITY) else { continue };
        // kg m² back to kg mm².
        let tensor = inertial.inertia.map(|row| row.map(|x| x * 1e6));
        let inertia = principal(tensor).and_then(|(moments, axes)| match moments.map(|m| NonNegativeReal::new(m.max(0.0))) {
            [Some(a), Some(b), Some(c)] => Some(PrincipalInertia { moments: [a, b, c], axes }),
            _ => None,
        });
        out.masses.push(MassProperties {
            id: id::<MassPropertiesId>("mass", out.masses.len())?,
            target: PhysicsTarget::Definition { definition: definition.clone() },
            mass: PositiveReal::new(inertial.mass),
            density: None,
            center_of_mass: FinitePoint3::new(Point3::new(inertial.com.x, inertial.com.y, inertial.com.z)),
            inertia,
            material: None,
            native_ref: None,
        });
    }
    out.scenes.push(PhysicsScene {
        id: id::<PhysicsSceneId>("scene", 0)?,
        name: Some("PhysicsScene".into()),
        gravity_direction: None,
        gravity_magnitude: None,
        native_ref: None,
    });
    Ok(out)
}

/// The two connector frames of `j`, each in its placement's component frame: A's frame with its
/// lateral offset, and B's frame with the mate taken off, so the two coincide at the joint's zero.
fn connectors(doc: &Document, model: &ModelState, j: &Joint) -> Option<[Transform; 2]> {
    let a = j.offset_frame_a(joints::resolve_origin(doc, model, &j.a).ok()?);
    let b = mat_mul(&joints::resolve_origin(doc, model, &j.b).ok()?, &mat_inverse(&joints::flip_of(j))?);
    Some([transform(&a)?, transform(&b)?])
}

/// The CADIR form of `j`'s kind, or `None` with a warning when it has none the writer writes.
fn paired_kind(j: &Joint, warnings: &mut Vec<String>) -> Option<PairedJointKind> {
    let limit = |i: usize| j.limits.get(i).copied().flatten().and_then(|(min, max)| JointLimits::new(Some(min), Some(max)));
    let limited = j.limits.iter().any(Option::is_some);
    let mut unlimited = |kind: &str| {
        if limited {
            warnings.push(format!("joint `{}`: OpenUSD writes a {kind} joint without limits: its limits are left out", j.name));
        }
    };
    Some(match j.kind {
        JointKind::Rigid => PairedJointKind::Fixed { angle: None, translation_offset: None, angular_limits: None, linear_limits: None },
        JointKind::Revolute => PairedJointKind::Revolute { angle: None, angular_limits: limit(0) },
        JointKind::Slider => PairedJointKind::Slider { distance: None, translation_offset: None, linear_limits: limit(0) },
        JointKind::Cylindrical => {
            unlimited("cylindrical");
            PairedJointKind::Cylindrical { angle: None, distance: None, angular_limits: limit(0), linear_limits: limit(1) }
        }
        JointKind::Planar => {
            unlimited("planar");
            PairedJointKind::PlaneCoincidence { sense: PlaneSense::Aligned, offset: None }
        }
        JointKind::Ball => {
            unlimited("ball");
            PairedJointKind::Ball {}
        }
        other => {
            warnings.push(format!("joint `{}`: a {other:?} joint has no OpenUSD physics form: left out", j.name));
            return None;
        }
    })
}

/// How close two principal moments are (a share of the largest) to count as equal: round-off
/// only, so the axes written are the tensor's own.
const EQUAL_MOMENTS: f64 = 1e-9;

/// The principal moments of the symmetric tensor `t` and the rotation whose columns are its
/// principal axes (`t = R · diag(moments) · Rᵀ`), `None` when they are the tensor's own axes;
/// [`solidcraft_geom::inertia::principal`] finds them. `None` for a tensor with non-finite entries.
pub(super) fn principal(t: [[f64; 3]; 3]) -> Option<([f64; 3], Option<Transform>)> {
    let (moments, [a, b, c]) = solidcraft_geom::inertia::principal(&t, EQUAL_MOMENTS)?;
    let own = [a, b, c] == [solidcore_record::Vec3::X, solidcore_record::Vec3::Y, solidcore_record::Vec3::Z];
    let axes = if own { None } else { Transform::affine([[a.x, b.x, c.x, 0.0], [a.y, b.y, c.y, 0.0], [a.z, b.z, c.z, 0.0]]) };
    Some((moments, axes))
}

#[cfg(test)]
mod tests {
    use super::principal;

    /// `R · diag(moments) · Rᵀ` from the rotation's rows.
    fn rebuilt(moments: [f64; 3], rows: [[f64; 4]; 3]) -> [[f64; 3]; 3] {
        let mut t = [[0.0; 3]; 3];
        for (i, row) in t.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| rows[i][k] * moments[k] * rows[j][k]).sum();
            }
        }
        t
    }

    #[test]
    fn a_diagonal_tensor_keeps_its_axes() {
        let (moments, axes) = principal([[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]]).unwrap();
        assert_eq!(moments, [2.0, 3.0, 4.0]);
        assert!(axes.is_none());
    }

    /// A box's tensor turned 30° about z: `R diag(Ixx, Iyy, Izz) Rᵀ` with R the turn. The moments
    /// come back, and the axes rebuild the tensor.
    #[test]
    fn a_turned_tensor_gives_its_moments_and_axes() {
        let (ixx, iyy, izz) = (5.0, 13.0, 10.0);
        let (s, c) = 30f64.to_radians().sin_cos();
        let t = [[c * c * ixx + s * s * iyy, c * s * (ixx - iyy), 0.0], [c * s * (ixx - iyy), s * s * ixx + c * c * iyy, 0.0], [0.0, 0.0, izz]];
        let (moments, axes) = principal(t).unwrap();
        let mut sorted = moments;
        sorted.sort_by(f64::total_cmp);
        for (got, want) in sorted.iter().zip([ixx, izz, iyy]) {
            assert!((got - want).abs() < 1e-12, "{moments:?}");
        }
        let rows = axes.expect("turned axes").affine_rows();
        let back = rebuilt(moments, rows);
        for (g, w) in back.iter().flatten().zip(t.iter().flatten()) {
            assert!((g - w).abs() < 1e-12, "{back:?} is not {t:?}");
        }
    }
}
