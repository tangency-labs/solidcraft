//! URDF robot description of a design's components and joints (#719, docs/urdf.md): a link per
//! placement of a component (plus `base_link` for the root and what is grounded), a joint chain
//! per SolidCraft joint, meshes in metres next to the `.urdf`, and each link's mass, centre of
//! mass and inertia from its bodies and their materials.
//!
//! Frames follow the joint kinematics `T_B · F_b = T_A · F_a · M(q) · Flip` (`doc::joints`).
//! Every constant is taken from world transforms at the current state, so `Flip` never appears:
//! with the child on B its link frame is `L = T_A · F_a · M(q)`; with the child on A it is
//! `L = T_A · F_a` and the parent-side joint frame `L · M(q)`, the chain inverted (reversed, axes
//! negated, so each URDF value still equals SolidCraft's).

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use solidcraft_doc::joints::{self, Joint, JointDynamics, JointKind};
use solidcraft_doc::{Document, IDENTITY, MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, Mat, ModelState, Occurrence, apply_point, mat_inverse, mat_mul};
use solidcraft_geom::{Mesh, Vec3};

use crate::{IoError, Result, export_mesh, stl_binary, weld, weld_eps};

/// Mass (kg) and inertia (kg m²) of a link with no massive body: Gazebo drops massless links.
const TOKEN_MASS: f64 = 1e-6;
const TOKEN_INERTIA: f64 = 1e-12;
/// Travel written for a prismatic step without a limit (mm): URDF requires one.
const UNLIMITED_TRAVEL_MM: f64 = 1e6;
const DEFAULT_EFFORT: f64 = 1000.0;
const DEFAULT_VELOCITY: f64 = 10.0;

/// Choices for [`urdf`] (`file.export` parameters).
#[derive(Clone, Debug, Default)]
pub struct UrdfOptions {
    /// Put before each mesh file name (`package://robot/meshes/` for RViz; empty: relative).
    pub mesh_prefix: String,
    /// `<limit effort>` (N or N m) and `velocity` (m/s or rad/s); defaults with a warning.
    pub effort: Option<f64>,
    pub velocity: Option<f64>,
}

/// The URDF, its mesh files (name next to the `.urdf`, bytes) and what was approximated.
#[derive(Clone, Debug)]
pub struct UrdfPackage {
    pub xml: String,
    pub meshes: Vec<(String, Vec<u8>)>,
    pub model: UrdfModel,
    pub warnings: Vec<String>,
}

/// The robot before serialisation (lengths in mm): tests compose it to check the kinematics.
#[derive(Clone, Debug, Default)]
pub struct UrdfModel {
    pub links: Vec<Link>,
    pub joints: Vec<UrdfJoint>,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub name: String,
    /// The placement it is (occurrence ids from the root, as `Document::placements`); empty for
    /// `base_link` and a chain's virtual links.
    pub path: Vec<u64>,
    /// Mesh coordinates (the component's frame) to the link frame.
    pub mesh_origin: Mat,
    pub visual: Option<String>,
    pub collision: Option<String>,
    /// sRGB and opacity, 0..1.
    pub color: Option<[f32; 4]>,
    pub inertial: Option<Inertial>,
}

/// Mass (kg), centre of mass (mm, link frame) and inertia about it (kg m², link axes).
#[derive(Clone, Copy, Debug)]
pub struct Inertial {
    pub mass: f64,
    pub com: Vec3,
    pub inertia: [[f64; 3]; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Fixed,
    Revolute,
    Prismatic,
}

#[derive(Clone, Debug)]
pub struct UrdfJoint {
    pub name: String,
    pub parent: usize,
    pub child: usize,
    /// Parent link frame to this joint's frame (mm).
    pub origin: Mat,
    pub step: Step,
    pub axis: Vec3,
    /// (lower, upper): radians or mm; `None` for a revolute without limit (`continuous`).
    pub limit: Option<(f64, f64)>,
    /// The SolidCraft joint and the index of its value this step follows.
    pub source: Option<(u64, usize)>,
    /// What the design saves for that value, without what a hand-edited file could hold.
    pub dynamics: JointDynamics,
}

impl UrdfModel {
    /// Every link's world frame (mm) for the given value of each step.
    pub fn link_frames(&self, value: impl Fn(&UrdfJoint) -> f64) -> Vec<Mat> {
        let mut frames = vec![IDENTITY; self.links.len()];
        let mut done = vec![false; self.links.len()];
        if let Some(d) = done.get_mut(0) {
            *d = true;
        }
        // Joints are emitted parents first.
        for j in &self.joints {
            let (Some(parent), true) = (frames.get(j.parent).copied(), done.get(j.parent).copied().unwrap_or(false)) else { continue };
            let q = value(j);
            let m = match j.step {
                Step::Fixed => IDENTITY,
                Step::Revolute => rotation(j.axis, q),
                Step::Prismatic => translation(j.axis * q),
            };
            if let (Some(f), Some(d)) = (frames.get_mut(j.child), done.get_mut(j.child)) {
                *f = mat_mul(&mat_mul(&parent, &j.origin), &m);
                *d = true;
            }
        }
        frames
    }
}

fn translation(v: Vec3) -> Mat {
    let mut m = IDENTITY;
    m[3] = [v.x, v.y, v.z, 1.0];
    m
}

/// Rotation by `a` about the unit axis `u` (Rodrigues), column-major like `Mat`.
fn rotation(u: Vec3, a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    let t = 1.0 - c;
    let r = [
        [t * u.x * u.x + c, t * u.x * u.y - s * u.z, t * u.x * u.z + s * u.y],
        [t * u.x * u.y + s * u.z, t * u.y * u.y + c, t * u.y * u.z - s * u.x],
        [t * u.x * u.z - s * u.y, t * u.y * u.z + s * u.x, t * u.z * u.z + c],
    ];
    // r[row][col] -> m[col][row]
    let mut m = IDENTITY;
    for (row, cols) in r.iter().enumerate() {
        for (col, v) in cols.iter().enumerate() {
            m[col][row] = *v;
        }
    }
    m
}

/// The joint's motion as URDF steps, in order from A's frame: (kind, axis, index of the value).
/// `None` for a relation whose turns follow a fixed step (a tilt, or the offset off a plane or
/// line): URDF has no fixed step inside a joint's chain.
fn steps(kind: JointKind) -> Option<Vec<(Step, Vec3, usize)>> {
    use Step::{Prismatic as P, Revolute as R};
    Some(match kind {
        JointKind::Rigid => vec![],
        JointKind::Revolute => vec![(R, Vec3::Z, 0)],
        JointKind::Slider => vec![(P, Vec3::Z, 0)],
        JointKind::Cylindrical => vec![(R, Vec3::Z, 0), (P, Vec3::Z, 1)],
        JointKind::PinSlot => vec![(P, Vec3::X, 1), (R, Vec3::Z, 0)],
        JointKind::Planar => vec![(P, Vec3::X, 0), (P, Vec3::Y, 1), (R, Vec3::Z, 2)],
        JointKind::Ball => vec![(R, Vec3::Z, 0), (R, Vec3::Y, 1), (R, Vec3::Z, 2)],
        JointKind::Parallel => vec![(P, Vec3::X, 0), (P, Vec3::Y, 1), (P, Vec3::Z, 2), (R, Vec3::Z, 3)],
        JointKind::Perpendicular | JointKind::Angle | JointKind::PointOnPlane | JointKind::PointOnLine | JointKind::LineOnPlane => return None,
    })
}

/// `raw` with every character but `[A-Za-z0-9_]` (and `-` in file names) replaced by `_`, so it
/// is a valid name in URDF, SDFormat and XML.
fn sanitise(raw: &str, file: bool) -> String {
    let s: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || (file && c == '-') { c } else { '_' }).collect();
    if s.is_empty() { "part".into() } else { s }
}

/// Unique names over links, joints and files (SDFormat refuses a joint named like a link).
#[derive(Default)]
struct Names(HashSet<String>);

impl Names {
    fn take(&mut self, raw: &str) -> String {
        self.unique(sanitise(raw, false))
    }
    /// A mesh file name, unique without regard to case: Windows and macOS file systems ignore
    /// it, so `t-base` and `t-Base` would be one file.
    fn file(&mut self, raw: &str) -> String {
        let base = sanitise(raw, true);
        let mut name = base.clone();
        let mut k = 2;
        while !self.0.insert(name.to_ascii_lowercase()) {
            name = format!("{base}_{k}");
            k += 1;
        }
        format!("{name}.stl")
    }
    fn unique(&mut self, base: String) -> String {
        let mut name = base.clone();
        let mut k = 2;
        while !self.0.insert(name.clone()) {
            name = format!("{base}_{k}");
            k += 1;
        }
        name
    }
}

/// One body's mass properties in its component's frame: (mass kg, centre mm, inertia kg mm²).
type BodyMass = (f64, Vec3, [[f64; 3]; 3]);

/// A component's meshes and masses.
struct Part {
    visual: Option<String>,
    collision: Option<String>,
    color: Option<[f32; 4]>,
    masses: Vec<BodyMass>,
}

/// Mesh files and masses per component (0: the root's bodies, named `base`).
fn parts(doc: &Document, st: &ModelState, stem: &str, names: &mut Names, out: &mut UrdfPackage) -> Result<BTreeMap<u64, Part>> {
    let mut used: BTreeSet<u64> = doc.occurrences.iter().map(|o| o.component).collect();
    used.insert(0);
    let mut parts = BTreeMap::new();
    for c in used {
        let label = if c == 0 { "base".to_string() } else { doc.components.iter().find(|x| x.id == c).map(|x| x.name.clone()).unwrap_or_default() };
        let (mut visual, mut collision) = (Mesh::default(), Mesh::default());
        let (mut masses, mut color, mut open) = (Vec::new(), None, false);
        for b in st.bodies.iter().filter(|b| doc.model_body_component(b) == c) {
            let mut m = export_mesh(&b.body)?;
            if let Some(o) = doc.model_body_offset(b) {
                for p in &mut m.positions {
                    *p = apply_point(o, *p);
                }
            }
            let (positions, triangles) = weld(&m.positions, &m.triangles, weld_eps(&b.body));
            let welded = Mesh { positions, triangles, ..Mesh::default() };
            let opacity = b.body.paint().map(|p| p.opacity).filter(|o| o.is_finite()).map_or(1.0, |o| o.clamp(0.0, 1.0));
            color = color.or(b.body.color().map(|[r, g, b]| [r, g, b, opacity]));
            append(&mut visual, &welded);
            match welded.mass_properties() {
                Some(mp) => {
                    // g/cm³ to kg/mm³.
                    let rho = doc.density(&b.name) * 1e-6;
                    masses.push((rho * mp.volume, mp.centroid, mp.inertia.map(|r| r.map(|x| x * rho))));
                    append(&mut collision, &welded);
                }
                None => {
                    open = true;
                    out.warnings.push(format!("body `{}` is not a closed solid: it is drawn but adds no mass or collision", b.name));
                }
            }
        }
        let mut part = Part { visual: None, collision: None, color, masses };
        if !visual.triangles.is_empty() {
            let file = names.file(&format!("{stem}-{label}"));
            out.meshes.push((file.clone(), stl_binary(&[metres(&visual)], &label)));
            part.visual = Some(file.clone());
            if !open {
                part.collision = Some(file);
            } else if !collision.triangles.is_empty() {
                let file = names.file(&format!("{stem}-{label}-collision"));
                out.meshes.push((file.clone(), stl_binary(&[metres(&collision)], &label)));
                part.collision = Some(file);
            }
        }
        parts.insert(c, part);
    }
    Ok(parts)
}

/// The placement `occurrence_world` resolves for an occurrence: through the first occurrence of
/// each parent component.
fn canonical(doc: &Document, occ: u64) -> Vec<u64> {
    let mut path = Vec::new();
    let mut cur = doc.occurrences.iter().find(|o| o.id == occ);
    for _ in 0..1000 {
        let Some(o) = cur else { break };
        path.push(o.id);
        if o.parent == 0 {
            break;
        }
        cur = doc.occurrence_of(o.parent);
    }
    path.reverse();
    path
}

/// The tree as it grows.
struct Tree<'a> {
    doc: &'a Document,
    parts: BTreeMap<u64, Part>,
    names: Names,
    model: UrdfModel,
    warnings: Vec<String>,
    /// Occurrence to its link and the link's world frame (0: `base_link`).
    placed: BTreeMap<u64, (usize, Mat)>,
    /// Each usable joint's name, taken before any link's so it keeps the name the user gave.
    joint_names: BTreeMap<u64, String>,
}

impl<'a> Tree<'a> {
    fn world(&self, occ: u64) -> Mat {
        if occ == 0 { IDENTITY } else { joints::occurrence_world(self.doc, occ) }
    }

    fn occurrence(&self, occ: u64) -> Option<&'a Occurrence> {
        let doc = self.doc;
        doc.occurrences.iter().find(|o| o.id == occ)
    }

    /// A link for placement `path` of `o`: its frame is `frame`, its component's meshes sit at
    /// `at` (both world). Named after the component when it is placed once, else the occurrence.
    fn link(&mut self, o: &Occurrence, path: Vec<u64>, frame: &Mat, at: &Mat) -> Option<usize> {
        let mesh_origin = mat_mul(&mat_inverse(frame)?, at);
        let doc = self.doc;
        let single = doc.occurrences.iter().filter(|x| x.component == o.component).count() == 1;
        let raw = match doc.components.iter().find(|c| c.id == o.component) {
            Some(c) if single => c.name.as_str(),
            _ => o.name.as_str(),
        };
        let part = self.parts.get(&o.component);
        let link = Link {
            name: self.names.take(raw),
            path,
            mesh_origin,
            visual: part.and_then(|p| p.visual.clone()),
            collision: part.and_then(|p| p.collision.clone()),
            color: part.and_then(|p| p.color),
            inertial: part.and_then(|p| inertial(&p.masses, &mesh_origin)).or(Some(token())),
        };
        self.model.links.push(link);
        Some(self.model.links.len() - 1)
    }

    /// `child` hangs on `parent` where it is (`origin`: parent link frame to child link frame).
    fn fixed(&mut self, parent: usize, child: usize, origin: Mat) {
        let n = self.model.links.get(child).map(|l| format!("{}_fixed", l.name)).unwrap_or_default();
        let name = self.names.take(&n);
        self.model.joints.push(UrdfJoint {
            name,
            parent,
            child,
            origin,
            step: Step::Fixed,
            axis: Vec3::Z,
            limit: None,
            source: None,
            dynamics: JointDynamics::default(),
        });
    }

    /// The link an occurrence fixed in place hangs on, and its frame: `base_link` for the top
    /// level (or `to_base`), else its parent occurrence's link once that is in the tree.
    fn parent_link(&self, o: &Occurrence, to_base: bool) -> Option<(usize, Mat)> {
        if to_base || o.parent == 0 {
            return Some((0, IDENTITY));
        }
        let p = self.doc.occurrence_of(o.parent)?;
        self.placed.get(&p.id).copied()
    }

    /// Fix an occurrence where it is on its parent link.
    fn fix(&mut self, occ: u64, to_base: bool) -> Option<usize> {
        let o = self.occurrence(occ)?;
        let (parent, parent_frame) = self.parent_link(o, to_base)?;
        let w = self.world(occ);
        let origin = mat_mul(&mat_inverse(&parent_frame)?, &w);
        let i = self.link(o, canonical(self.doc, occ), &w, &w)?;
        self.fixed(parent, i, origin);
        self.placed.insert(occ, (i, w));
        Some(i)
    }

    /// Breadth-first over the usable joints from the occurrences in `queue`.
    fn traverse(&mut self, queue: &mut VecDeque<u64>, usable: &[(&Joint, Mat)], taken: &mut HashSet<u64>) {
        let values = joints::joint_values(&self.doc.assembly);
        while let Some(p) = queue.pop_front() {
            for (j, fa) in usable {
                if taken.contains(&j.id) || (j.a.occurrence != p && j.b.occurrence != p) {
                    continue;
                }
                taken.insert(j.id);
                let child = if j.a.occurrence == p { j.b.occurrence } else { j.a.occurrence };
                if self.placed.contains_key(&child) {
                    self.warnings.push(format!("joint `{}` closes a loop: skipped (URDF is a tree)", j.name));
                    continue;
                }
                if self.attach(j, fa, p, child, values.get(&j.id).map(Vec::as_slice).unwrap_or(&[])).is_some() {
                    queue.push_back(child);
                }
            }
        }
    }

    /// `child` on joint `j` from `p` (already in the tree): the joint's chain and the child's link.
    fn attach(&mut self, j: &Joint, fa: &Mat, p: u64, child: u64, q: &[f64]) -> Option<()> {
        // A relation URDF can't write is fixed where it is: the child link's frame is the joint's.
        let (chain, m) = match steps(j.kind) {
            Some(chain) => (chain, joints::motion(j.kind, q)),
            None => {
                self.warnings.push(format!("joint `{}` is a {:?} relation, which URDF has no joint for: written fixed as it is", j.name, j.kind));
                (Vec::new(), IDENTITY)
            }
        };
        let child_on_b = j.a.occurrence == p;
        // (parent-side joint frame, child link frame) in the world.
        let (jf, lf) = if child_on_b {
            let jf = mat_mul(&self.world(p), fa);
            (jf, mat_mul(&jf, &m))
        } else {
            let lf = mat_mul(&self.world(child), fa);
            (mat_mul(&lf, &m), lf)
        };
        let (parent_index, parent_frame) = self.placed.get(&p).copied()?;
        let origin = mat_mul(&mat_inverse(&parent_frame)?, &jf);
        let mut chain = chain;
        if !child_on_b {
            chain.reverse();
            for s in &mut chain {
                s.1 = -s.1;
            }
        }
        let o = self.occurrence(child)?;
        let child_index = self.link(o, canonical(self.doc, child), &lf, &self.world(child))?;
        self.placed.insert(child, (child_index, lf));
        let joint_name = self.joint_names.get(&j.id).cloned().unwrap_or_else(|| j.name.clone());
        if chain.is_empty() {
            let name = joint_name;
            self.model.joints.push(UrdfJoint {
                name,
                parent: parent_index,
                child: child_index,
                origin,
                step: Step::Fixed,
                axis: Vec3::Z,
                limit: None,
                source: None,
                dynamics: JointDynamics::default(),
            });
            return Some(());
        }
        let mut from = parent_index;
        let last = chain.len() - 1;
        for (k, (step, axis, dof)) in chain.into_iter().enumerate() {
            let to = if k == last {
                child_index
            } else {
                let name = self.names.take(&format!("{}_link{}", j.name, k + 1));
                self.model.links.push(Link {
                    name,
                    path: Vec::new(),
                    mesh_origin: IDENTITY,
                    visual: None,
                    collision: None,
                    color: None,
                    inertial: Some(token()),
                });
                self.model.links.len() - 1
            };
            // A limit that is not finite (a hand-edited file) is no limit.
            let limit =
                j.limits.get(dof).copied().flatten().filter(|(lo, hi)| lo.is_finite() && hi.is_finite()).map(|(lo, hi)| (lo.min(hi), lo.max(hi)));
            let limit = match (step, limit) {
                (Step::Prismatic, None) => {
                    self.warnings.push(format!("joint `{}` slides without a limit: written as ±1000 m (URDF needs one)", j.name));
                    Some((-UNLIMITED_TRAVEL_MM, UNLIMITED_TRAVEL_MM))
                }
                (_, l) => l,
            };
            let name = if k == last { joint_name.clone() } else { self.names.take(&format!("{}_dof{}", j.name, k + 1)) };
            let saved = j.dynamics.get(dof).copied().flatten().unwrap_or_default();
            let dynamics = saved.sane();
            if dynamics != saved {
                self.warnings.push(format!("joint `{}`: dynamics that are not usable numbers are ignored", j.name));
            }
            self.model.joints.push(UrdfJoint {
                name,
                parent: from,
                child: to,
                origin: if k == 0 { origin } else { IDENTITY },
                step,
                axis,
                limit,
                source: Some((j.id, dof)),
                dynamics,
            });
            from = to;
        }
        Some(())
    }

    /// Every other placement of a nested occurrence (its parent component is placed several
    /// times): a copy fixed to its parent placement's link.
    fn copies(&mut self, jointed: &HashSet<u64>) {
        let mut copies: BTreeMap<Vec<u64>, (usize, Mat)> = BTreeMap::new();
        let placements = self.doc.placements();
        if placements.len() > MAX_PLACEMENTS || placements.iter().any(|p| p.1.len() > MAX_PLACEMENT_DEPTH) {
            self.warnings.push(format!(
                "the assembly is deeper than {MAX_PLACEMENT_DEPTH} levels or has more than {MAX_PLACEMENTS} placements: copies past that are left out"
            ));
        }
        for (_, path, w) in placements {
            let Some((&occ, parent_path)) = path.split_last() else { continue };
            if path == canonical(self.doc, occ) {
                continue;
            }
            let parent = match parent_path.last() {
                None => Some((0, IDENTITY)),
                Some(&p) if parent_path == canonical(self.doc, p).as_slice() => self.placed.get(&p).copied(),
                Some(_) => copies.get(parent_path).copied(),
            };
            let (Some((pi, pf)), Some(o)) = (parent, self.occurrence(occ)) else { continue };
            let Some(origin) = mat_inverse(&pf).map(|inv| mat_mul(&inv, &w)) else { continue };
            let Some(i) = self.link(o, path.clone(), &w, &w) else { continue };
            self.fixed(pi, i, origin);
            copies.insert(path, (i, w));
            if jointed.contains(&occ)
                && let Some(l) = self.model.links.get(i)
            {
                self.warnings.push(format!(
                    "`{}` is a copy of a jointed occurrence: fixed where it is (the URDF moves only the first placement; copies would need <mimic>)",
                    l.name
                ));
            }
        }
    }
}

/// Build the URDF of a design: `doc` with its occurrences placed (pending moves applied), `st`
/// with the bodies in their components' own frames, painted. `stem` names the robot and
/// prefixes the mesh files.
pub fn urdf(doc: &Document, st: &ModelState, stem: &str, options: &UrdfOptions) -> Result<UrdfPackage> {
    // XML 1.0 has no way to write these, escaped or not.
    let unwritable = |c: char| (c.is_control() && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{FFFE}' | '\u{FFFF}');
    if options.mesh_prefix.len() > 4096 {
        return Err(IoError::Invalid("`mesh_prefix` is longer than 4096 bytes".into()));
    }
    if options.mesh_prefix.chars().any(unwritable) {
        return Err(IoError::Invalid("`mesh_prefix` has a character XML cannot hold".into()));
    }
    let mut names = Names::default();
    let mut out = UrdfPackage { xml: String::new(), meshes: Vec::new(), model: UrdfModel::default(), warnings: Vec::new() };
    let base = names.take("base_link");
    let robot = sanitise(stem, false);
    let parts = parts(doc, st, stem, &mut names, &mut out)?;
    let mut tree = Tree {
        doc,
        parts,
        names,
        model: UrdfModel::default(),
        warnings: std::mem::take(&mut out.warnings),
        placed: BTreeMap::from([(0, (0, IDENTITY))]),
        joint_names: BTreeMap::new(),
    };
    let root = tree.parts.get(&0);
    let base_link = Link {
        name: base,
        path: Vec::new(),
        mesh_origin: IDENTITY,
        visual: root.and_then(|p| p.visual.clone()),
        collision: root.and_then(|p| p.collision.clone()),
        color: root.and_then(|p| p.color),
        inertial: root.and_then(|p| inertial(&p.masses, &IDENTITY)),
    };
    tree.model.links.push(base_link);

    // Usable joints: unsuppressed (as many as the solver reads), both ends placed and distinct,
    // both origins resolving.
    let exists = |occ: u64| occ == 0 || doc.occurrences.iter().any(|o| o.id == occ);
    let mut usable: Vec<(&Joint, Mat)> = Vec::new();
    for j in doc.assembly.joints.iter().filter(|j| !j.suppressed).take(joints::MAX_JOINTS) {
        let fa = joints::resolve_origin(doc, st, &j.a).ok().filter(|_| joints::resolve_origin(doc, st, &j.b).is_ok());
        match fa {
            Some(fa) if exists(j.a.occurrence) && exists(j.b.occurrence) && j.a.occurrence != j.b.occurrence => usable.push((j, fa)),
            _ => tree.warnings.push(format!("joint `{}` is skipped: its origins don't resolve or it joins an occurrence to itself", j.name)),
        }
    }
    let jointed: HashSet<u64> = usable.iter().flat_map(|(j, _)| [j.a.occurrence, j.b.occurrence]).collect();
    for (j, _) in &usable {
        let name = tree.names.take(&j.name);
        tree.joint_names.insert(j.id, name);
    }

    // Grounded occurrences stay where they are in the world, nested or not.
    let mut queue = VecDeque::from([0]);
    // In id order, as the solver's queue, so both keep the same joint where two could reach a part.
    let mut grounded: Vec<u64> = doc.occurrences.iter().filter(|o| o.grounded).map(|o| o.id).collect();
    grounded.sort_unstable();
    for id in grounded {
        if tree.fix(id, true).is_some() {
            queue.push_back(id);
        }
    }
    let mut taken = HashSet::new();
    let mut left_out: HashSet<u64> = HashSet::new();
    loop {
        tree.traverse(&mut queue, &usable, &mut taken);
        let done = |t: &Tree, occ: u64| t.placed.contains_key(&occ) || left_out.contains(&occ);
        let ready = |t: &Tree, occ: u64| t.occurrence(occ).is_some_and(|o| t.parent_link(o, false).is_some());
        // A floating group: anchored at its first joint's A side, as the solver anchors one
        // when nothing is grounded; then what no joint reaches, parents first.
        let next = usable
            .iter()
            .find(|(j, _)| !done(&tree, j.a.occurrence) && !done(&tree, j.b.occurrence) && ready(&tree, j.a.occurrence))
            .map(|(j, _)| (j.a.occurrence, false))
            .or_else(|| doc.occurrences.iter().find(|o| !done(&tree, o.id) && ready(&tree, o.id)).map(|o| (o.id, false)))
            .or_else(|| doc.occurrences.iter().find(|o| !done(&tree, o.id)).map(|o| (o.id, true)));
        let Some((occ, to_base)) = next else { break };
        let Some(i) = tree.fix(occ, to_base) else {
            tree.warnings.push(format!("occurrence {occ} has a degenerate placement: left out"));
            left_out.insert(occ);
            continue;
        };
        queue.push_back(occ);
        let nested = tree.occurrence(occ).is_some_and(|o| o.parent != 0);
        let name = tree.model.links.get(i).map(|l| l.name.clone()).unwrap_or_default();
        if jointed.contains(&occ) {
            tree.warnings.push(format!("`{name}` is not grounded: fixed where it is"));
        } else if !nested || to_base {
            tree.warnings.push(format!("`{name}` has no joint: fixed where it is"));
        }
    }
    tree.copies(&jointed);

    let on_fallback: Vec<&str> = tree
        .model
        .joints
        .iter()
        .filter(|j| {
            j.step != Step::Fixed
                && ((j.dynamics.effort.is_none() && options.effort.is_none()) || (j.dynamics.velocity.is_none() && options.velocity.is_none()))
        })
        .map(|j| j.name.as_str())
        .collect();
    if !on_fallback.is_empty() {
        tree.warnings.push(format!(
            "joint effort and velocity limits are not given for {}: written as {DEFAULT_EFFORT} and {DEFAULT_VELOCITY} (use joint.dynamics, or pass effort and velocity)",
            on_fallback.join(", ")
        ));
    }
    out.xml = write_xml(&tree.model, &robot, options);
    out.model = tree.model;
    out.warnings = tree.warnings;
    Ok(out)
}

fn token() -> Inertial {
    Inertial { mass: TOKEN_MASS, com: Vec3::ZERO, inertia: [[TOKEN_INERTIA, 0.0, 0.0], [0.0, TOKEN_INERTIA, 0.0], [0.0, 0.0, TOKEN_INERTIA]] }
}

/// Bodies' masses (component frame) combined in the link frame (`to_link`: component to link).
fn inertial(masses: &[BodyMass], to_link: &Mat) -> Option<Inertial> {
    let mass: f64 = masses.iter().map(|m| m.0).sum();
    if !(mass > 0.0) {
        return None;
    }
    let com = masses.iter().fold(Vec3::ZERO, |acc, m| acc + apply_point(to_link, m.1) * m.0) * (1.0 / mass);
    let r = |i: usize, j: usize| to_link.get(j).and_then(|c| c.get(i)).copied().unwrap_or(0.0);
    let mut total = [[0.0f64; 3]; 3];
    for (m, c, inertia) in masses {
        // R I Rᵀ, then the parallel-axis term about the link's centre of mass.
        let d = apply_point(to_link, *c) - com;
        let dv = [d.x, d.y, d.z];
        let d2 = d.dot(d);
        for (i, (row_out, di)) in total.iter_mut().zip(dv).enumerate() {
            for (j, (cell, dj)) in row_out.iter_mut().zip(dv).enumerate() {
                let mut v = 0.0;
                for (a, row) in inertia.iter().enumerate() {
                    for (b, x) in row.iter().enumerate() {
                        v += r(i, a) * x * r(j, b);
                    }
                }
                *cell += v + m * (if i == j { d2 } else { 0.0 } - di * dj);
            }
        }
    }
    // kg mm² to kg m².
    let inertia = total.map(|row| row.map(|x| x * 1e-6));
    let finite = mass.is_finite() && [com.x, com.y, com.z].iter().all(|x| x.is_finite()) && inertia.iter().flatten().all(|x| x.is_finite());
    finite.then_some(Inertial { mass, com, inertia })
}

fn append(into: &mut Mesh, m: &Mesh) {
    let base = u32::try_from(into.positions.len()).unwrap_or(u32::MAX);
    into.positions.extend_from_slice(&m.positions);
    into.triangles.extend(m.triangles.iter().map(|t| t.map(|i| i.saturating_add(base))));
}

fn metres(m: &Mesh) -> Mesh {
    Mesh { positions: m.positions.iter().map(|p| *p * 1e-3).collect(), triangles: m.triangles.clone(), ..Mesh::default() }
}

/// `xyz` (m) and `rpy` (rad, fixed axes: R = Rz(yaw) Ry(pitch) Rx(roll)) of a frame.
fn xyz_rpy(m: &Mat) -> (Vec3, Vec3) {
    let r = |row: usize, col: usize| m.get(col).and_then(|c| c.get(row)).copied().unwrap_or(0.0);
    // atan2 over hypot keeps pitch precise next to +-90 degrees, where asin would not.
    let cos_pitch = r(0, 0).hypot(r(1, 0));
    let pitch = (-r(2, 0)).atan2(cos_pitch);
    let (roll, yaw) = if cos_pitch > 1e-12 {
        (r(2, 1).atan2(r(2, 2)), r(1, 0).atan2(r(0, 0)))
    } else {
        // Gimbal lock: yaw carries the whole turn about z.
        (0.0, (-r(0, 1)).atan2(r(1, 1)))
    };
    let t = apply_point(m, Vec3::ZERO) * 1e-3;
    (t, Vec3::new(roll, pitch, yaw))
}

fn attr(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

fn num(x: f64) -> String {
    // No "-0" and no non-finite values in the XML. Shortest round-trip digits either way; very
    // small or large values with an exponent rather than a run of zeros.
    if !x.is_finite() || x == 0.0 {
        "0".into()
    } else if x.abs() < 1e-3 || x.abs() >= 1e7 {
        format!("{x:e}")
    } else {
        format!("{x}")
    }
}

fn triple(v: Vec3) -> String {
    format!("{} {} {}", num(v.x), num(v.y), num(v.z))
}

fn origin_xml(m: &Mat) -> String {
    let (xyz, rpy) = xyz_rpy(m);
    format!("<origin xyz=\"{}\" rpy=\"{}\"/>", triple(xyz), triple(rpy))
}

fn write_xml(model: &UrdfModel, name: &str, options: &UrdfOptions) -> String {
    let fallback_effort = options.effort.filter(|x| x.is_finite() && *x > 0.0).unwrap_or(DEFAULT_EFFORT);
    let fallback_velocity = options.velocity.filter(|x| x.is_finite() && *x > 0.0).unwrap_or(DEFAULT_VELOCITY);
    let prefix = attr(&options.mesh_prefix);
    let mut s = String::from("<?xml version=\"1.0\"?>\n<!-- Exported by SolidCraft; units: m, kg, rad -->\n");
    s += &format!("<robot name=\"{}\">\n", attr(name));
    for l in &model.links {
        s += &format!("  <link name=\"{}\">\n", attr(&l.name));
        if let Some(i) = &l.inertial {
            let c = i.com * 1e-3;
            let t = &i.inertia;
            s += &format!(
                "    <inertial>\n      <origin xyz=\"{}\" rpy=\"0 0 0\"/>\n      <mass value=\"{}\"/>\n      <inertia ixx=\"{}\" ixy=\"{}\" ixz=\"{}\" iyy=\"{}\" iyz=\"{}\" izz=\"{}\"/>\n    </inertial>\n",
                triple(c),
                num(i.mass),
                num(t[0][0]),
                num(t[0][1]),
                num(t[0][2]),
                num(t[1][1]),
                num(t[1][2]),
                num(t[2][2])
            );
        }
        if let Some(file) = &l.visual {
            s += &format!(
                "    <visual>\n      {}\n      <geometry><mesh filename=\"{prefix}{}\"/></geometry>\n",
                origin_xml(&l.mesh_origin),
                attr(file)
            );
            if let Some([r, g, b, a]) = l.color {
                s += &format!(
                    "      <material name=\"{}_color\"><color rgba=\"{} {} {} {}\"/></material>\n",
                    attr(&l.name),
                    num(r as f64),
                    num(g as f64),
                    num(b as f64),
                    num(a as f64)
                );
            }
            s += "    </visual>\n";
        }
        if let Some(file) = &l.collision {
            s += &format!(
                "    <collision>\n      {}\n      <geometry><mesh filename=\"{prefix}{}\"/></geometry>\n    </collision>\n",
                origin_xml(&l.mesh_origin),
                attr(file)
            );
        }
        s += "  </link>\n";
    }
    for j in &model.joints {
        let (Some(parent), Some(child)) = (model.links.get(j.parent), model.links.get(j.child)) else { continue };
        let kind = match (j.step, j.limit) {
            (Step::Fixed, _) => "fixed",
            (Step::Revolute, None) => "continuous",
            (Step::Revolute, Some(_)) => "revolute",
            (Step::Prismatic, _) => "prismatic",
        };
        s += &format!("  <joint name=\"{}\" type=\"{kind}\">\n", attr(&j.name));
        s += &format!("    <parent link=\"{}\"/>\n    <child link=\"{}\"/>\n    {}\n", attr(&parent.name), attr(&child.name), origin_xml(&j.origin));
        if j.step != Step::Fixed {
            s += &format!("    <axis xyz=\"{}\"/>\n", triple(j.axis));
            let scale = if j.step == Step::Prismatic { 1e-3 } else { 1.0 };
            let effort = j.dynamics.effort.unwrap_or(fallback_effort);
            let velocity = j.dynamics.velocity.unwrap_or(fallback_velocity);
            match j.limit {
                Some((lo, hi)) => {
                    s += &format!(
                        "    <limit lower=\"{}\" upper=\"{}\" effort=\"{}\" velocity=\"{}\"/>\n",
                        num(lo * scale),
                        num(hi * scale),
                        num(effort),
                        num(velocity)
                    );
                }
                None => s += &format!("    <limit effort=\"{}\" velocity=\"{}\"/>\n", num(effort), num(velocity)),
            }
            if j.dynamics.damping.is_some() || j.dynamics.friction.is_some() {
                s += "    <dynamics";
                if let Some(x) = j.dynamics.damping {
                    s += &format!(" damping=\"{}\"", num(x));
                }
                if let Some(x) = j.dynamics.friction {
                    s += &format!(" friction=\"{}\"", num(x));
                }
                s += "/>\n";
            }
        }
        s += "  </joint>\n";
    }
    s += "</robot>\n";
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::f64::consts::FRAC_PI_2;

    use quick_xml::events::Event;

    /// Rotations that stress the decomposition: general ones, gimbal lock (pitch ±90°, which a
    /// hinge whose axis lies along x gives) and next to it.
    fn hard_rotations() -> Vec<Mat> {
        let r = |axis: Vec3, a: f64| rotation(axis.normalized().unwrap_or(Vec3::Z), a);
        let mut out = vec![r(Vec3::X, 0.3), r(Vec3::Y, -1.1), r(Vec3::Z, 2.5), r(Vec3::new(1.0, 2.0, 3.0), 0.7)];
        for pitch in [FRAC_PI_2, -FRAC_PI_2, FRAC_PI_2 - 1e-7, -FRAC_PI_2 + 1e-9] {
            for (yaw, roll) in [(0.0, 0.0), (0.7, 0.3), (-2.0, 1.4)] {
                out.push(mat_mul(&mat_mul(&r(Vec3::Z, yaw), &r(Vec3::Y, pitch)), &r(Vec3::X, roll)));
            }
        }
        out
    }

    /// The frame an `<origin xyz rpy>` stands for (xyz in m, the frame in mm).
    fn compose(xyz: Vec3, rpy: Vec3) -> Mat {
        mat_mul(&mat_mul(&mat_mul(&translation(xyz * 1e3), &rotation(Vec3::Z, rpy.z)), &rotation(Vec3::Y, rpy.y)), &rotation(Vec3::X, rpy.x))
    }

    fn close(a: &Mat, b: &Mat) -> bool {
        a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn rpy_round_trips_a_rotation() {
        for (k, mut m) in hard_rotations().into_iter().enumerate() {
            m[3] = [10.0, -20.0, 30.0, 1.0];
            let (xyz, rpy) = xyz_rpy(&m);
            assert!(close(&compose(xyz, rpy), &m), "{k}: {rpy:?}");
        }
    }

    /// Every `<origin>` in the file parses back to the model's frame, so the forward kinematics
    /// the engine tests check on [`UrdfModel`] are the ones a URDF reader gets.
    #[test]
    fn written_origins_parse_back_to_the_model() {
        let link = |name: String, mesh_origin: Mat| Link {
            name,
            path: Vec::new(),
            mesh_origin,
            visual: Some("m.stl".into()),
            collision: Some("m.stl".into()),
            color: None,
            inertial: None,
        };
        let mut model = UrdfModel { links: vec![link("base_link".into(), IDENTITY)], joints: Vec::new() };
        for (k, mut m) in hard_rotations().into_iter().enumerate() {
            m[3] = [k as f64, 2.0, -3.5, 1.0];
            model.links.push(link(format!("l{k}"), m));
            let step = if k % 2 == 0 { Step::Revolute } else { Step::Prismatic };
            model.joints.push(UrdfJoint {
                name: format!("j{k}"),
                parent: 0,
                child: k + 1,
                origin: m,
                step,
                axis: -Vec3::Y,
                limit: Some((-1.0, 2.0)),
                source: None,
                dynamics: JointDynamics::default(),
            });
        }
        let xml = write_xml(&model, "r", &UrdfOptions::default());
        let mut reader = quick_xml::Reader::from_str(&xml);
        let mut origins = Vec::new();
        loop {
            match reader.read_event() {
                Ok(Event::Empty(e)) if e.local_name().as_ref() == b"origin" => {
                    let triple = |key: &[u8]| -> Vec3 {
                        let v: Vec<f64> = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.local_name().as_ref() == key)
                            .map(|a| String::from_utf8_lossy(&a.value).split_whitespace().filter_map(|x| x.parse().ok()).collect())
                            .unwrap_or_default();
                        Vec3::new(
                            v.first().copied().unwrap_or(f64::NAN),
                            v.get(1).copied().unwrap_or(f64::NAN),
                            v.get(2).copied().unwrap_or(f64::NAN),
                        )
                    };
                    origins.push(compose(triple(b"xyz"), triple(b"rpy")));
                }
                Ok(Event::Eof) => break,
                Err(e) => panic!("{e}"),
                _ => {}
            }
        }
        // Each link writes its visual's and collision's origin, then each joint its own.
        let want: Vec<Mat> = model.links.iter().flat_map(|l| [l.mesh_origin, l.mesh_origin]).chain(model.joints.iter().map(|j| j.origin)).collect();
        assert_eq!(origins.len(), want.len());
        for (k, (got, want)) in origins.iter().zip(&want).enumerate() {
            assert!(close(got, want), "origin {k}: {got:?} vs {want:?}");
        }
    }

    /// XML cannot hold a control character, escaped or not: the prefix is refused here, for
    /// every caller.
    #[test]
    fn a_mesh_prefix_xml_cannot_hold_is_refused() {
        let doc = Document::new("t");
        let st = ModelState::default();
        for (prefix, why) in [("a\u{1}b".to_string(), "character"), ("a".repeat(5000), "longer than")] {
            let options = UrdfOptions { mesh_prefix: prefix, ..UrdfOptions::default() };
            let e = urdf(&doc, &st, "t", &options).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(e.contains("mesh_prefix") && e.contains(why), "{e}");
        }
        assert!(urdf(&doc, &st, "t", &UrdfOptions { mesh_prefix: "package://r/m/".into(), ..UrdfOptions::default() }).is_ok());
    }

    #[test]
    fn numbers_are_short_and_exact() {
        assert_eq!(num(0.01), "0.01");
        assert_eq!(num(-1000.0), "-1000");
        assert_eq!(num(2.1500000000000004e-8), "2.1500000000000004e-8");
        assert_eq!(num(-8.881784197001253e-19), "-8.881784197001253e-19");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(f64::NAN), "0");
        for x in [0.01, 1e-3, 9.99e-4, 123456.789, 3.0e12, -2.5e-7] {
            assert_eq!(num(x).parse::<f64>().ok(), Some(x), "{x}");
        }
    }

    #[test]
    fn names_are_unique_and_sanitised() {
        let mut n = Names::default();
        assert_eq!(n.take("base_link"), "base_link");
        assert_eq!(n.take("Leaf A:1"), "Leaf_A_1");
        assert_eq!(n.take("Leaf_A_1"), "Leaf_A_1_2");
        assert_eq!(n.take(""), "part");
        // File systems that ignore case would make these one file.
        assert_eq!(n.file("t-base"), "t-base.stl");
        assert_eq!(n.file("t-Base"), "t-Base_2.stl");
    }
}
