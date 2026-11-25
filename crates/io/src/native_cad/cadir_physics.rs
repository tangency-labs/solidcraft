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
//! revolute and slider joints only. A joint of another kind and a suppressed joint are left out,
//! each with a warning. A grounded placement is fixed to the world, in every placement written.
//!
//! Motion links: a gear link between two revolute joints and a rack-and-pinion link between a
//! revolute joint and a slider become PhysX gear and rack-and-pinion joints that couple the two
//! written joints ([`coupling`]). Another link (a screw, a cam, a link with no coupling) is left
//! out with a warning.
//!
//! Mass: each component placed in the design gets its bodies' mass, centre of mass and principal
//! inertia, from each body's density (its material's) and its closed mesh at standard quality, as
//! URDF export's links, and each placement of such a component is a rigid body, joined or not.
//! The design's own bodies get none: they never move.

use std::collections::HashMap;

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{ArticulationId, MassPropertiesId, OccurrenceId, PhysicsSceneId, ProductDefinitionId, RigidBodyId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::physics::{Articulation, MassProperties, PhysicsScene, PhysicsTarget, PrincipalInertia, RigidBody};
use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointId, JointLimits, JointOperand, PairedJointKind, PlaneSense};
use cadmpeg_ir::scalar::{NonNegativeReal, PositiveReal};
use cadmpeg_ir::transform::Transform;
use solidcraft_doc::joints::{self, Coupling, Joint, JointKind, MotionLink};
use solidcraft_doc::{Document, IDENTITY, Mat, ModelState, mat_inverse, mat_mul};

use crate::urdf::BodyMass;

/// The physics records of a design, and what they leave out.
#[derive(Default)]
pub(super) struct Physics {
    pub joints: Vec<AssemblyJoint>,
    pub masses: Vec<MassProperties>,
    pub rigid_bodies: Vec<RigidBody>,
    pub articulations: Vec<Articulation>,
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
    // A side's written placement: `Some(None)` for the design itself (the world).
    let end = |occurrence: u64| -> Option<Option<OccurrenceId>> {
        if occurrence == 0 {
            return Some(None);
        }
        placed.get(&crate::urdf::canonical(doc, occurrence)).map(|id| Some(id.clone()))
    };
    let operand = |occurrence: u64| -> Option<JointOperand> {
        end(occurrence).map(|id| match id {
            None => JointOperand::root("design", Vec::new()),
            Some(id) => JointOperand::occurrence(id, "placement", Vec::new()),
        })
    };
    // The written joints' ends, for the articulations.
    let mut edges: Vec<(Option<OccurrenceId>, Option<OccurrenceId>, String)> = Vec::new();
    // Each written joint's id and kind, by the design's joint, for the motion links.
    let mut written: HashMap<u64, (JointId, JointKind)> = HashMap::new();
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
        written.insert(j.id, (joint.id.clone(), j.kind));
        out.joints.push(joint);
        if let (Some(a), Some(b)) = (end(j.a.occurrence), end(j.b.occurrence)) {
            edges.push((a, b, j.name.clone()));
        }
    }
    // Each coupling joins the parts its two joints turn or slide: each joint's moving side (the
    // side not grounded, as the assembly solver moves it). It runs outside the articulations, so
    // it adds no edge.
    for (n, link) in doc.assembly.links.iter().enumerate() {
        let name = |id: u64| all.iter().find(|j| j.id == id).map_or_else(|| format!("#{id}"), |j| j.name.replace('/', "_"));
        let what = format!("motion link {} between `{}` and `{}`", n + 1, name(link.a), name(link.b));
        let (kind, coupled) = match coupling(link, &written) {
            Ok(found) => found,
            Err(reason) => {
                out.warnings.push(format!("{what}: {reason}: left out of the physics"));
                continue;
            }
        };
        // The part each joint moves, as the assembly solver takes it: not the grounded side.
        let moving = |id: u64| all.iter().find(|j| j.id == id).and_then(|j| operand(joints::cam::moving_occurrence(doc, j)));
        let (Some(a), Some(b)) = (moving(coupled[0]), moving(coupled[1])) else {
            out.warnings.push(format!("{what}: its parts are not written: left out of the physics"));
            continue;
        };
        let connector = |operand| JointConnector { operand, frame: Transform::identity(), detached: false };
        let mut joint = AssemblyJoint::paired(id::<JointId>("joint", out.joints.len())?, kind, [connector(a), connector(b)], None);
        joint.native_ref = Some(format!("solidcraft:design:link/{}_{}", name(link.a), name(link.b)));
        out.joints.push(joint);
    }
    // Every written placement of a grounded occurrence is fixed: a subassembly placed twice
    // grounds its grounded part in both, as the design does.
    let grounded: HashMap<u64, &str> = doc.occurrences.iter().filter(|o| o.grounded).map(|o| (o.id, o.name.as_str())).collect();
    let mut paths: Vec<(&Vec<u64>, &OccurrenceId)> = placed.iter().collect();
    paths.sort();
    let mut anchored: Vec<OccurrenceId> = Vec::new();
    for (path, occurrence) in &paths {
        let Some(name) = path.last().and_then(|o| grounded.get(o)) else { continue };
        anchored.push((*occurrence).clone());
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
    out.articulations = articulations(&edges, &anchored, &mut out.warnings)?;
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

/// The articulations of the joints `edges` (each joint's two ends: a placement, or `None` for the
/// world) with the placements `anchored` fixed to the world: one for each group of placements the
/// joints connect that forms a tree. Its root is the group's anchor (a placement fixed to the world
/// or jointed to it), else the first joint's A side, as URDF export anchors a floating group. A
/// group with a closed loop, through two anchors and the world included, has no articulation, with
/// a warning: PhysX would solve its joints apart.
pub(super) fn articulations(
    edges: &[(Option<OccurrenceId>, Option<OccurrenceId>, String)],
    anchored: &[OccurrenceId],
    warnings: &mut Vec<String>,
) -> Result<Vec<Articulation>, String> {
    // Union-find over the placements the joints touch, in the order the joints first touch them,
    // halving each path it walks, so a long chain of unions is walked once.
    let mut order: Vec<OccurrenceId> = Vec::new();
    let mut parent: HashMap<OccurrenceId, OccurrenceId> = HashMap::new();
    fn find(parent: &mut HashMap<OccurrenceId, OccurrenceId>, mut x: OccurrenceId) -> OccurrenceId {
        while let Some(p) = parent.get(&x).filter(|p| **p != x).cloned() {
            let up = parent.get(&p).cloned().unwrap_or_else(|| p.clone());
            parent.insert(x, up.clone());
            x = up;
        }
        x
    }
    for (a, b, _) in edges {
        for id in [a, b].into_iter().flatten() {
            if !parent.contains_key(id) {
                parent.insert(id.clone(), id.clone());
                order.push(id.clone());
            }
        }
        if let (Some(a), Some(b)) = (a, b) {
            let (ra, rb) = (find(&mut parent, a.clone()), find(&mut parent, b.clone()));
            if ra != rb {
                parent.insert(rb, ra);
            }
        }
    }
    // Each group's members, joints and world joints, gathered in one pass over the placements
    // and one over the joints, the groups in the order of their first placements.
    struct Group<'e> {
        first: OccurrenceId,
        members: Vec<OccurrenceId>,
        joints: Vec<&'e str>,
        internal: usize,
        to_world: Vec<OccurrenceId>,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut group_of: HashMap<OccurrenceId, usize> = HashMap::new();
    for o in &order {
        let root = find(&mut parent, o.clone());
        let k = *group_of.entry(root).or_insert_with(|| {
            groups.push(Group { first: o.clone(), members: Vec::new(), joints: Vec::new(), internal: 0, to_world: Vec::new() });
            groups.len() - 1
        });
        if let Some(g) = groups.get_mut(k) {
            g.members.push(o.clone());
        }
    }
    for (a, b, name) in edges {
        let Some(side) = a.as_ref().or(b.as_ref()) else { continue };
        let root = find(&mut parent, side.clone());
        let Some(g) = group_of.get(&root).and_then(|k| groups.get_mut(*k)) else { continue };
        g.joints.push(name.as_str());
        match (a, b) {
            (Some(_), Some(_)) => g.internal += 1,
            (Some(o), None) | (None, Some(o)) => g.to_world.push(o.clone()),
            (None, None) => {}
        }
    }
    let anchored: std::collections::HashSet<&OccurrenceId> = anchored.iter().collect();
    let mut out = Vec::new();
    for g in groups {
        let fixed: Vec<&OccurrenceId> = g.members.iter().filter(|m| anchored.contains(m)).collect();
        if g.internal + 1 != g.members.len() || g.to_world.len() + fixed.len() > 1 {
            warnings.push(format!(
                "joints `{}` close a loop (two fixed or world-jointed parts close one through the world): written without an articulation, so PhysX solves them as separate joints",
                g.joints.join("`, `")
            ));
            continue;
        }
        let root = fixed.first().map(|m| (*m).clone()).or_else(|| g.to_world.first().cloned()).unwrap_or(g.first);
        out.push(Articulation {
            id: id::<ArticulationId>("articulation", out.len())?,
            root: PhysicsTarget::Occurrence { occurrence: root },
            native_ref: None,
        });
    }
    Ok(out)
}

/// The PhysX coupling of motion link `link` between the written joints `written` (each design
/// joint's written id and kind), with the design joints it couples in CADIR's order (the gears' a
/// and b; the pinion's, then the rack's), or why it has none. PhysX's gear joint turns its second
/// hinge `ratio` times its first's angle the other way round, and its rack-and-pinion joint moves
/// the rack along its slider's axis as the pinion turns positively about its hinge's. CADIR states
/// each ratio as a magnitude, so gears that turn the same way (an internal gear, or a reversed
/// link) and a rack that moves the other way have no form.
pub(super) fn coupling(link: &MotionLink, written: &HashMap<u64, (JointId, JointKind)>) -> Result<(PairedJointKind, [u64; 2]), String> {
    let joint = |id: u64| written.get(&id).ok_or("its joints are not written");
    let ((ja, ka), (jb, kb)) = (joint(link.a)?, joint(link.b)?);
    let ratio = |r: f64| PositiveReal::new(r).ok_or_else(|| format!("its ratio {} has no PhysX form", link.ratio));
    match &link.coupling {
        Some(Coupling::Gear { .. }) => {
            if (*ka, *kb) != (JointKind::Revolute, JointKind::Revolute) {
                return Err("PhysX gears couple two revolute joints".into());
            }
            if link.ratio > 0.0 {
                return Err("its gears turn the same way, and PhysX's gear joint turns them opposite ways".into());
            }
            let kind = PairedJointKind::Gears {
                distance: None,
                distance2: None,
                ratio: Some(ratio(-link.ratio)?),
                coupled_joints: vec![ja.clone(), jb.clone()],
            };
            Ok((kind, [link.a, link.b]))
        }
        Some(Coupling::RackPinion { .. }) => {
            // The rack's travel (mm) per radian of the pinion, and the two joints in that order.
            let (travel, pinion, rack) = match (ka, kb) {
                (JointKind::Revolute, JointKind::Slider) if link.ia == 0 && link.ib == 0 => (link.ratio, (link.a, ja), (link.b, jb)),
                (JointKind::Slider, JointKind::Revolute) if link.ia == 0 && link.ib == 0 => (1.0 / link.ratio, (link.b, jb), (link.a, ja)),
                _ => return Err("a PhysX rack and pinion couples a revolute joint and a slider".into()),
            };
            if travel < 0.0 {
                return Err("its rack moves against the pinion's turn, and PhysX's rack and pinion moves it with the turn".into());
            }
            let kind = PairedJointKind::RackPinion {
                distance: None,
                distance2: None,
                ratio: Some(ratio(1.0 / travel)?),
                coupled_joints: vec![pinion.1.clone(), rack.1.clone()],
            };
            Ok((kind, [pinion.0, rack.0]))
        }
        Some(Coupling::Screw { .. }) => Err("a screw has no OpenUSD physics form".into()),
        Some(Coupling::Cam(_)) => Err("a cam has no OpenUSD physics form".into()),
        None => Err("a link with no gear or rack has no OpenUSD physics form".into()),
    }
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
    use super::articulations;
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::physics::PhysicsTarget;

    fn occ(name: &str) -> OccurrenceId {
        OccurrenceId::try_from(format!("test:physics:occurrence#{name}")).unwrap()
    }

    fn edge(a: Option<&str>, b: Option<&str>, name: &str) -> (Option<OccurrenceId>, Option<OccurrenceId>, String) {
        (a.map(occ), b.map(occ), name.to_string())
    }

    fn roots(edges: &[(Option<OccurrenceId>, Option<OccurrenceId>, String)], anchored: &[&str]) -> (Vec<OccurrenceId>, Vec<String>) {
        let anchored: Vec<OccurrenceId> = anchored.iter().map(|a| occ(a)).collect();
        let mut warnings = Vec::new();
        let found = articulations(edges, &anchored, &mut warnings).unwrap();
        let roots = found
            .into_iter()
            .map(|a| match a.root {
                PhysicsTarget::Occurrence { occurrence } => occurrence,
                other => panic!("{other:?}"),
            })
            .collect();
        (roots, warnings)
    }

    /// A chain fixed at one end is one articulation rooted there; a floating chain is rooted at
    /// its first joint's A side; a part jointed to the world anchors its group.
    #[test]
    fn a_tree_is_one_articulation_rooted_at_its_anchor() {
        let chain = [edge(Some("b"), Some("a"), "j1"), edge(Some("b"), Some("c"), "j2")];
        assert_eq!(roots(&chain, &["a"]), (vec![occ("a")], vec![]));
        assert_eq!(roots(&chain, &[]), (vec![occ("b")], vec![]));
        let to_world = [edge(Some("a"), Some("b"), "j1"), edge(None, Some("b"), "j0")];
        assert_eq!(roots(&to_world, &[]), (vec![occ("b")], vec![]));
    }

    /// Two groups are two articulations.
    #[test]
    fn separate_groups_are_separate_articulations() {
        let edges = [edge(Some("a"), Some("b"), "j1"), edge(Some("c"), Some("d"), "j2")];
        assert_eq!(roots(&edges, &["d"]), (vec![occ("a"), occ("d")], vec![]));
    }

    /// Many groups, and a long chain, take one pass each: 50,000 separate pairs are 50,000
    /// articulations in their order, and a chain of 50,000 joints one, rooted at its anchor.
    #[test]
    fn many_groups_and_a_long_chain_are_grouped_in_one_pass() {
        let pairs: Vec<_> = (0..50_000).map(|k| edge(Some(&format!("a{k}")), Some(&format!("b{k}")), &format!("j{k}"))).collect();
        let (found, warnings) = roots(&pairs, &[]);
        assert_eq!(found.len(), 50_000);
        assert!(warnings.is_empty() && found[0] == occ("a0") && found[49_999] == occ("a49999"));
        let chain: Vec<_> = (0..50_000).map(|k| edge(Some(&format!("c{}", k + 1)), Some(&format!("c{k}")), &format!("j{k}"))).collect();
        assert_eq!(roots(&chain, &["c0"]), (vec![occ("c0")], vec![]));
    }

    /// A closed loop has no articulation, with a warning naming its joints; two anchors close one
    /// through the world.
    #[test]
    fn a_loop_has_no_articulation() {
        let triangle = [edge(Some("a"), Some("b"), "j1"), edge(Some("b"), Some("c"), "j2"), edge(Some("c"), Some("a"), "j3")];
        let (found, warnings) = roots(&triangle, &[]);
        assert!(found.is_empty());
        assert!(warnings[0].contains("`j1`, `j2`, `j3`"), "{warnings:?}");
        let (found, warnings) = roots(&[edge(Some("a"), Some("b"), "j1")], &["a", "b"]);
        assert!(found.is_empty() && warnings.len() == 1, "{warnings:?}");
        let (found, _) = roots(&[edge(Some("a"), Some("b"), "j1"), edge(None, Some("b"), "j0")], &["a"]);
        assert!(found.is_empty());
    }

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

    mod couplings {
        use super::super::coupling;
        use cadmpeg_ir::products::{JointId, PairedJointKind};
        use solidcraft_doc::joints::{Coupling, JointKind, MotionLink, Size};
        use std::collections::HashMap;

        fn joint(n: u64) -> JointId {
            JointId::try_from(format!("test:physics:joint#{n}")).unwrap()
        }

        /// Joints 1 and 2 are revolute, joint 3 a slider; joint 4 is not written.
        fn written() -> HashMap<u64, (JointId, JointKind)> {
            [(1, JointKind::Revolute), (2, JointKind::Revolute), (3, JointKind::Slider)].into_iter().map(|(n, k)| (n, (joint(n), k))).collect()
        }

        fn link(a: u64, b: u64, ratio: f64, coupling: Coupling) -> MotionLink {
            MotionLink { a, ia: 0, b, ib: 0, ratio, offset: 0.0, coupling: Some(coupling), lift: Vec::new(), carried_cam: false }
        }

        fn gear() -> Coupling {
            Coupling::Gear { ra: Size::Length(20.0), rb: Size::Length(10.0), internal: false, reverse: false }
        }

        fn rack() -> Coupling {
            Coupling::RackPinion { radius: Size::Length(5.0), reverse: false }
        }

        fn ratio_of(kind: &PairedJointKind) -> (f64, Vec<JointId>) {
            match kind {
                PairedJointKind::Gears { ratio, coupled_joints, .. } | PairedJointKind::RackPinion { ratio, coupled_joints, .. } => {
                    (ratio.unwrap().get(), coupled_joints.clone())
                }
                other => panic!("{other:?} is no coupling"),
            }
        }

        #[test]
        fn opposite_gears_are_a_gear_joint_of_the_ratio() {
            // b turns twice a's angle the other way: gearRatio 2, hinge0 a's joint.
            let (kind, coupled) = coupling(&link(1, 2, -2.0, gear()), &written()).unwrap();
            assert!(matches!(kind, PairedJointKind::Gears { .. }));
            assert_eq!(ratio_of(&kind), (2.0, vec![joint(1), joint(2)]));
            assert_eq!(coupled, [1, 2]);
        }

        #[test]
        fn gears_turning_the_same_way_have_no_form() {
            let err = coupling(&link(1, 2, 0.5, gear()), &written()).unwrap_err();
            assert!(err.contains("same way"), "{err}");
        }

        #[test]
        fn a_rack_is_coupled_pinion_first_in_radians_per_millimetre() {
            // 5 mm of rack per radian is 0.2 rad/mm, whichever joint drives.
            for (l, want) in [(link(1, 3, 5.0, rack()), 0.2), (link(3, 1, 0.2, rack()), 0.2)] {
                let (kind, coupled) = coupling(&l, &written()).unwrap();
                assert!(matches!(kind, PairedJointKind::RackPinion { .. }));
                let (ratio, joints) = ratio_of(&kind);
                assert!((ratio - want).abs() < 1e-12, "{ratio}");
                assert_eq!(joints, vec![joint(1), joint(3)]);
                assert_eq!(coupled, [1, 3]);
            }
        }

        #[test]
        fn a_rack_moving_against_the_turn_has_no_form() {
            assert!(coupling(&link(1, 3, -5.0, rack()), &written()).is_err());
            assert!(coupling(&link(3, 1, -0.2, rack()), &written()).is_err());
        }

        #[test]
        fn other_links_have_no_form() {
            let screw = Coupling::Screw { pitch: Size::Length(2.0), reverse: false };
            assert!(coupling(&link(1, 3, 1.0, screw), &written()).is_err());
            assert!(coupling(&link(1, 3, -2.0, gear()), &written()).is_err(), "a gear on a slider");
            assert!(coupling(&link(1, 4, -2.0, gear()), &written()).is_err(), "an unwritten joint");
            let plain = MotionLink { coupling: None, ..link(1, 2, -1.0, gear()) };
            assert!(coupling(&plain, &written()).is_err());
        }
    }
}
