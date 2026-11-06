//! Linear elastic vibration of the current world geometry. The analysis mesh is separate
//! from the design: boundary face tags carry the selected supports through tetrahedralization.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use fieldcraft_fem::{Formulation, Solid};
use fieldcraft_mesh::tet::QualityMesh;
pub use fieldcraft_model::progress::{Progress, scoped as with_progress};
use fieldcraft_model::{BoundaryCondition, Elastic, ElementFace, Material, Model, Procedure, Section, SectionKind, Set, Step, Target, Tie};
use fieldcraft_solver::{FrequencyOptions, Limits, MassKind, solve_frequency};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Vec3};
use solidcraft_doc::physical_material::{Elasticity, PropertyData};
use solidcraft_geom::{Aabb3, prelude::*};

use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session, cmd::CommandSpec};

#[path = "vibration_tessellation.rs"]
mod tessellation;

pub const COMMAND: &str = "inspect.vibration";
const CONTRACT: TypedContract<SweepParams> = TypedContract::new();
pub(crate) static COMMANDS: &[CommandSpec] = &[CommandSpec::new(COMMAND, "Vibration Sweep", crate::checked!("inspect.vibration", execute))
    .contract(&CONTRACT).output(output_schema_for::<SweepResult>)
    .at("SOLID", "INSPECT").icon("graph").noundo().params(
        "body?: one world body name, omit for all bodies; supports: [{body, face}], start_hz and end_hz. Returns a modal and Z-base excitation study of the current geometry without editing it.",
    )];

fn execute(s: &mut Session, p: &serde_json::Value) -> Result<serde_json::Value> {
    let params = serde_json::from_value(p.clone()).map_err(error)?;
    serde_json::to_value(solve(s, &params)?).map_err(error)
}

const MAX_BODIES: usize = 12;
const MAX_NODES: usize = 12_000;
const MAX_MODES: u32 = 40;
const DAMPING: f64 = 0.02;
const BASE_ACCELERATION: f64 = 9.80665;
const MM_TO_M: f64 = 0.001;
const BOND_GAP_MM: f64 = 0.005;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Support {
    pub body: String,
    pub face: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SweepParams {
    /// World body name for a part. Omit to analyse all world bodies, including hidden bodies.
    pub body: Option<String>,
    pub supports: Vec<Support>,
    pub start_hz: f64,
    pub end_hz: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct SurfaceTriangle {
    pub vertices: [usize; 3],
    pub body: usize,
    pub support: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct SweepPoint {
    pub hz: f64,
    pub max_mm: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Shape {
    pub hz: f64,
    /// Complex surface displacement in mm (response), or normalized to one (mode).
    pub real: Vec<[f64; 3]>,
    pub imaginary: Vec<[f64; 3]>,
    /// Peak magnitude during one cycle, in the same units as the displacement.
    pub amplitude: Vec<f64>,
    pub maximum: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct SweepResult {
    pub bodies: Vec<String>,
    pub vertices_mm: Vec<[f64; 3]>,
    pub vertex_body: Vec<usize>,
    pub triangles: Vec<SurfaceTriangle>,
    pub curve: Vec<SweepPoint>,
    pub peaks: Vec<Shape>,
    pub has_resonance: bool,
    pub modes: Vec<Shape>,
    pub damping_ratio: f64,
    pub base_acceleration_m_s2: f64,
    pub highest_mode_hz: f64,
    pub element_count: usize,
    pub warnings: Vec<String>,
}

fn error(e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(e.to_string())
}

fn point(v: [f64; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

fn coords(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn material(s: &Session, body: &solidcraft_doc::ModelBody) -> Result<Material> {
    let name = s.doc.body_material(body.definition_name());
    let missing =
        || error(format!("{} ({name}): assign a material with one isotropic elastic property (Young's modulus and Poisson's ratio)", body.name));
    let data = s.doc.user_material(&name).and_then(|m| m.physical_card.as_ref()).ok_or_else(missing)?;
    let mut elasticity = data.card().properties.iter().filter_map(|p| match &p.data {
        PropertyData::Elasticity { model } => Some(model),
        _ => None,
    });
    let Some(Elasticity::Isotropic { young_modulus_pa, poisson_ratio }) = elasticity.next() else { return Err(missing()) };
    if elasticity.next().is_some() {
        return Err(missing());
    }
    Ok(Material {
        elastic: Some(Elastic::Isotropic { young: *young_modulus_pa, poisson: *poisson_ratio }),
        density: s.doc.material_density(&name).map(|rho| rho / MM_TO_M),
        ..Material::default()
    })
}

fn opposing_planes(a: &Plane, b: &Plane) -> bool {
    let normal = a.normal();
    normal.dot(b.normal()) <= -0.99999 && (b.origin - a.origin).dot(normal).abs() <= BOND_GAP_MM
}

struct BodyMesh {
    name: String,
    mesh: QualityMesh,
    fixed: BTreeSet<u32>,
    bounds: Aabb3,
    planes: BTreeMap<u32, Plane>,
    material: Material,
}

fn mesh_body(s: &Session, body: &solidcraft_doc::ModelBody, supports: &[Support], assembly: bool) -> Result<BodyMesh> {
    fieldcraft_model::progress::check().map_err(error)?;
    let material = material(s, body)?;
    if !body.is_closed() {
        return Err(error(format!("{}: vibration requires a closed solid", body.name)));
    }
    let bounds = body.body.exact_bounds().map_err(|e| error(format!("{}: {e}", body.name)))?;
    let face_count = body.body.face_count();
    let fixed: BTreeSet<_> = supports.iter().filter(|r| r.body == body.name).map(|r| r.face).collect();
    if fixed.iter().any(|&f| f as usize >= face_count) {
        return Err(error(format!("{}: a fixed face no longer exists", body.name)));
    }
    let mut planes = BTreeMap::new();
    if assembly {
        for i in 0..face_count {
            if let Some(plane) = body.body.planar_face(i) {
                if planes.len() >= 256 {
                    return Err(error(format!("{}: too many planar faces for automatic bonding in this preview", body.name)));
                }
                planes.insert(u32::try_from(i).map_err(error)?, plane);
            }
        }
    }
    let extent = bounds.size();
    // Mortar interfaces currently accept linear faces. Use a finer linear mesh for
    // assemblies, and quadratic elements to represent bending in a single body.
    let subdivisions = if assembly { 4.0 } else { 2.5 };
    let size = (extent.len() / 18.0).min(extent.x.min(extent.y).min(extent.z) / subdivisions);
    let mesh = tessellation::build(body, size, assembly, MAX_NODES).map_err(|e| error(format!("{}: {e}", body.name)))?;
    Ok(BodyMesh { name: body.name.clone(), mesh, fixed, bounds, planes, material })
}

#[derive(Default)]
struct Surface {
    vertices: Vec<[f64; 3]>,
    owners: Vec<usize>,
    labels: Vec<u32>,
    facets: Vec<SurfaceTriangle>,
}

fn label(offset: usize, index: usize) -> Result<u32> {
    u32::try_from(offset + index + 1).map_err(error)
}

type FaceSets = BTreeMap<u32, Vec<ElementFace>>;

fn append_body(
    model: &mut Model,
    body: &BodyMesh,
    body_index: usize,
    surface: &mut Surface,
    nodal_mass: &mut BTreeMap<u32, f64>,
    fixed: &mut BTreeSet<u32>,
) -> Result<FaceSets> {
    let mesh = &body.mesh;
    let node_offset = model.nodes().len();
    let element_offset = model.elements().len();
    if node_offset + mesh.points.len() > MAX_NODES {
        return Err(error("the combined vibration mesh is too large for this preview"));
    }
    for (i, &xyz) in mesh.points.iter().enumerate() {
        model.set_node(label(node_offset, i)?, xyz.map(|x| x * MM_TO_M)).map_err(error)?;
    }
    let mut elements = Vec::with_capacity(mesh.elements.len());
    let density = body.material.density.ok_or_else(|| error(format!("{}: assign a material with a density", body.name)))?;
    for (i, element) in mesh.elements.iter().enumerate() {
        let element_label = label(element_offset, i)?;
        let nodes: Vec<_> = element.iter().map(|&v| label(node_offset, v as usize)).collect::<Result<_>>()?;
        let points =
            nodes.iter().map(|&n| model.node(n).map(|p| p.xyz).ok_or_else(|| error("invalid vibration mesh node"))).collect::<Result<Vec<_>>>()?;
        let solid = Solid::new(mesh.kind, &points, Formulation::Full).map_err(error)?;
        // M r for uniform base translation in Z, including mass coupling to the supported
        // nodes. The consistent body-force integral works for curved Tet10 elements too.
        for (&n, force) in nodes.iter().zip(solid.body_force([0.0, 0.0, density]).map_err(error)?) {
            *nodal_mass.entry(n).or_default() += force[2];
        }
        model.set_element(element_label, mesh.kind, nodes).map_err(error)?;
        elements.push(element_label);
    }
    let name = format!("Part{body_index}");
    model.add_material(&name, body.material.clone()).map_err(error)?;
    model.add_set(&name, Set { elements, ..Set::default() }).map_err(error)?;
    model
        .add_section(&name, Section { set: name.clone(), material: name.clone(), kind: SectionKind::Solid {}, formulation: Default::default() })
        .map_err(error)?;
    let mut surface_indices = BTreeMap::new();
    let mut faces = FaceSets::new();
    for face in &mesh.boundary {
        let element_face = face.element_face().ok_or_else(|| error("invalid vibration boundary face"))?;
        let element = mesh.elements.get(face.tet as usize).ok_or_else(|| error("invalid vibration boundary element"))?;
        let indices = mesh.kind.faces().get(usize::from(element_face - 1)).ok_or_else(|| error("invalid face connectivity"))?;
        let support = body.fixed.contains(&face.face);
        let mut vertices = Vec::with_capacity(3);
        for &slot in *indices {
            let node = *element.get(slot).ok_or_else(|| error("invalid face node"))? as usize;
            let xyz = *mesh.points.get(node).ok_or_else(|| error("invalid face point"))?;
            let id = label(node_offset, node)?;
            if support {
                fixed.insert(id);
            }
            let index = *surface_indices.entry(node).or_insert_with(|| {
                let next = surface.vertices.len();
                surface.vertices.push(xyz);
                surface.labels.push(id);
                surface.owners.push(body_index);
                next
            });
            vertices.push(index);
        }
        let triangles = match *vertices.as_slice() {
            [a, b, c] => vec![[a, b, c]],
            [a, b, c, ab, bc, ca] => vec![[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]],
            _ => return Err(error("expected a triangular vibration boundary")),
        };
        surface.facets.extend(triangles.into_iter().map(|vertices| SurfaceTriangle { vertices, body: body_index, support }));
        faces.entry(face.face).or_default().push(ElementFace { element: label(element_offset, face.tet as usize)?, face: element_face });
    }
    if body.fixed.iter().any(|tag| !faces.contains_key(tag)) {
        return Err(error(format!("{}: a fixed face is absent from the volume mesh", body.name)));
    }
    Ok(faces)
}

fn bond(model: &mut Model, bodies: &[BodyMesh], faces: &[FaceSets]) -> Result<()> {
    let mut order: Vec<_> = bodies.iter().zip(faces).enumerate().collect();
    order.sort_by_key(|(_, (body, _))| body.fixed.is_empty());
    let mut connected = vec![false; bodies.len()];
    // Visit the supported bodies first and check their mutual interfaces too. Every tie runs
    // from an earlier body to a newly reached body, so the dependency graph stays acyclic.
    while connected.iter().any(|&v| !v) {
        let mut next = None;
        for &(i, (body, map)) in order.iter().filter(|(i, _)| !connected[*i]) {
            let mut contacts = Vec::new();
            for (j, (main, main_map)) in bodies.iter().zip(faces).enumerate().filter(|(j, _)| connected[*j]) {
                if !body.bounds.inflated(BOND_GAP_MM).meets(&main.bounds) {
                    continue;
                }
                for (tag, face) in &body.planes {
                    for (main_tag, main_face) in &main.planes {
                        if opposing_planes(face, main_face)
                            && let (Some(secondary), Some(primary)) = (map.get(tag), main_map.get(main_tag))
                        {
                            contacts.push((format!("Bond{i}_{j}_{tag}_{main_tag}"), primary.clone(), secondary.clone()));
                        }
                    }
                }
            }
            if !body.fixed.is_empty() || !contacts.is_empty() {
                next = Some((i, contacts));
                break;
            }
        }
        let Some((index, contacts)) = next else {
            let names =
                bodies.iter().zip(&connected).filter(|(_, connected)| !**connected).map(|(b, _)| b.name.as_str()).collect::<Vec<_>>().join(", ");
            return Err(error(format!("No bonded planar interface to a supported body: {names}")));
        };
        for (name, primary, secondary) in contacts {
            let (a, b) = (format!("{name}A"), format!("{name}B"));
            model.add_set(&a, Set { faces: primary, ..Set::default() }).map_err(error)?;
            model.add_set(&b, Set { faces: secondary, ..Set::default() }).map_err(error)?;
            let mut tie = Tie::new(&a, &b);
            tie.position_tolerance = Some(BOND_GAP_MM * MM_TO_M);
            model.add_tie(&name, tie).map_err(error)?;
        }
        if let Some(v) = connected.get_mut(index) {
            *v = true;
        }
    }
    Ok(())
}

struct Mode {
    hz: f64,
    participation: f64,
    shape: Vec<[f64; 3]>,
}

fn response_gain(mode: &Mode, hz: f64) -> (f64, f64) {
    let (w, wn) = (TAU * hz, TAU * mode.hz);
    let (real, imaginary) = (wn * wn - w * w, 2.0 * DAMPING * w * wn);
    let factor = -BASE_ACCELERATION * mode.participation / (real * real + imaginary * imaginary) / MM_TO_M;
    (factor * real, -factor * imaginary)
}

fn cycle_peak(r: Vec3, i: Vec3) -> f64 {
    let (rr, ii, ri) = (r.dot(r), i.dot(i), r.dot(i));
    (0.5 * (rr + ii + (rr - ii).hypot(2.0 * ri))).sqrt()
}

fn displacement(modes: &[Mode], count: usize, hz: f64) -> (Vec<Vec3>, Vec<Vec3>) {
    let mut real = vec![Vec3::ZERO; count];
    let mut imaginary = vec![Vec3::ZERO; count];
    for mode in modes {
        let (a, b) = response_gain(mode, hz);
        for ((r, i), &v) in real.iter_mut().zip(&mut imaginary).zip(&mode.shape) {
            let v = point(v);
            *r += v * a;
            *i += v * b;
        }
    }
    (real, imaginary)
}

fn sample(modes: &[Mode], count: usize, hz: f64) -> SweepPoint {
    let (real, imaginary) = displacement(modes, count, hz);
    let max_mm = real.into_iter().zip(imaginary).map(|(a, b)| cycle_peak(a, b)).fold(0.0, f64::max);
    SweepPoint { hz, max_mm }
}

fn response(modes: &[Mode], count: usize, hz: f64) -> Shape {
    let (real, imaginary) = displacement(modes, count, hz);
    let amplitude: Vec<_> = real.iter().zip(&imaginary).map(|(&r, &i)| cycle_peak(r, i)).collect();
    let maximum = max_index(&amplitude);
    Shape { hz, real: real.into_iter().map(coords).collect(), imaginary: imaginary.into_iter().map(coords).collect(), amplitude, maximum }
}

fn refine_peak(modes: &[Mode], count: usize, mut left: f64, mut right: f64) -> SweepPoint {
    // Refine the local maximum bracketed by three consecutive sweep samples.
    for _ in 0..11 {
        let a = left + (right - left) / 3.0;
        let b = right - (right - left) / 3.0;
        if sample(modes, count, a).max_mm < sample(modes, count, b).max_mm {
            left = a;
        } else {
            right = b;
        }
    }
    sample(modes, count, 0.5 * (left + right))
}

fn max_index(values: &[f64]) -> usize {
    values.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |(i, _)| i)
}

fn normalized(mode: &Mode) -> Shape {
    let mut amplitude: Vec<_> = mode.shape.iter().map(|&v| point(v).len()).collect();
    let maximum = max_index(&amplitude);
    let max = amplitude.get(maximum).copied().filter(|v| *v > 0.0).unwrap_or(1.0);
    amplitude.iter_mut().for_each(|v| *v /= max);
    let real = mode.shape.iter().map(|v| v.map(|x| x / max)).collect();
    Shape { hz: mode.hz, real, imaginary: vec![[0.0; 3]; amplitude.len()], amplitude, maximum }
}

pub fn solve(s: &Session, p: &SweepParams) -> Result<SweepResult> {
    if !(p.start_hz >= 0.01 && p.start_hz < p.end_hz && p.end_hz <= 1.0e6) {
        return Err(error("Enter a frequency range with 0.01 Hz ≤ start < end ≤ 1 MHz"));
    }
    if p.supports.is_empty() || p.supports.len() > 64 {
        return Err(error("Select between one and 64 fixed faces"));
    }
    let state = s.world_state()?;
    let selected: Vec<_> = state.bodies.iter().filter(|b| p.body.as_ref().is_none_or(|name| name == &b.name)).collect();
    if selected.is_empty() || selected.len() > MAX_BODIES {
        return Err(error("The vibration preview needs one to twelve solid bodies"));
    }
    if p.supports.iter().any(|r| !selected.iter().any(|b| b.name == r.body)) {
        return Err(error("A fixed face belongs to a body outside the analysis"));
    }
    let names = selected.iter().map(|b| b.name.clone()).collect();
    let assembly = selected.len() > 1;
    let bodies = selected.into_iter().map(|b| mesh_body(s, b, &p.supports, assembly)).collect::<Result<Vec<_>>>()?;
    let mut model = Model::default();
    let mut surface = Surface::default();
    let mut nodal_mass = BTreeMap::new();
    let mut fixed = BTreeSet::new();
    let mut faces = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        faces.push(append_body(&mut model, body, i, &mut surface, &mut nodal_mass, &mut fixed)?);
    }
    bond(&mut model, &bodies, &faces)?;
    model.add_set("Fixed", Set { nodes: fixed.into_iter().collect(), ..Set::default() }).map_err(error)?;
    model.add_boundary_condition("Fixed", BoundaryCondition::Fixed { target: Target::Set("Fixed".into()) }).map_err(error)?;
    let mut step = Step::new("Vibration", Procedure::Frequency { modes: MAX_MODES, min_hz: None, max_hz: None });
    step.boundary_conditions.insert("Fixed".into());
    model.add_step(step, None).map_err(error)?;
    let settings = FrequencyOptions { mass: MassKind::Consistent, shift: Some(0.0) };
    let limits = Limits { max_dofs: 3 * MAX_NODES, max_factor_entries: 12_000_000, max_lanczos_vectors: 260, ..Limits::default() };
    let result = solve_frequency(&model, Some("Vibration"), &settings, &limits).map_err(error)?;
    let highest_mode_hz = result.modes.last().map_or(0.0, |m| m.frequency);
    if result.modes.len() == MAX_MODES as usize && highest_mode_hz <= p.end_hz {
        return Err(error(format!("The first {MAX_MODES} modes reach only {highest_mode_hz:.2} Hz. Lower the end frequency.")));
    }
    if result.modes.is_empty() || result.modes.iter().any(|m| !m.frequency.is_finite() || m.frequency <= 0.0) {
        return Err(error("The analysis has no positive supported modes; check the fixed faces and bonded interfaces"));
    }
    let indices = surface
        .labels
        .iter()
        .map(|label| result.nodes.binary_search(label).map_err(|_| error("missing vibration surface result")))
        .collect::<Result<Vec<_>>>()?;
    let modes: Vec<_> = result
        .modes
        .iter()
        .map(|m| {
            let participation = result.nodes.iter().zip(&m.shape).map(|(n, v)| nodal_mass.get(n).copied().unwrap_or(0.0) * v[2]).sum();
            let shape = indices.iter().map(|&i| m.shape.get(i).copied().unwrap_or_default()).collect();
            Mode { hz: m.frequency, participation, shape }
        })
        .collect();
    let within = |f: f64| f >= p.start_hz && f <= p.end_hz;
    let mut frequencies: Vec<_> = (0..=240).map(|i| p.start_hz + (p.end_hz - p.start_hz) * f64::from(i) / 240.0).collect();
    frequencies.extend(modes.iter().flat_map(|m| (-12..=12).map(move |i| m.hz * (1.0 + f64::from(i) * DAMPING / 3.0))).filter(|&f| within(f)));
    frequencies.sort_by(f64::total_cmp);
    frequencies.dedup();
    let count = surface.vertices.len();
    let mut curve = Vec::with_capacity(frequencies.len());
    for hz in frequencies {
        fieldcraft_model::progress::check().map_err(error)?;
        curve.push(sample(&modes, count, hz));
    }
    let mut peaks: Vec<_> = curve
        .windows(3)
        .filter_map(|w| match w {
            [a, b, c] if b.max_mm > a.max_mm && b.max_mm >= c.max_mm => {
                let refined = refine_peak(&modes, count, a.hz, c.hz);
                Some(if refined.max_mm > b.max_mm { refined } else { b.clone() })
            }
            _ => None,
        })
        .collect();
    let has_resonance = !peaks.is_empty();
    if has_resonance {
        curve.extend(peaks.iter().cloned());
        curve.sort_by(|a, b| a.hz.total_cmp(&b.hz));
        curve.dedup_by(|a, b| a.hz == b.hz);
    } else if let Some(p) = curve.iter().max_by(|a, b| a.max_mm.total_cmp(&b.max_mm)) {
        peaks.push(p.clone());
    }
    let peaks = peaks.into_iter().map(|p| response(&modes, count, p.hz)).collect();
    let modes = modes.iter().filter(|m| within(m.hz)).map(normalized).collect();
    let mut warnings = result.warnings;
    if highest_mode_hz < 2.0 * p.end_hz && result.modes.len() == MAX_MODES as usize {
        warnings.push(
            "The extracted modes do not extend to twice the end frequency. The upper part of the sweep can be sensitive to mode truncation.".into(),
        );
    }
    Ok(SweepResult {
        bodies: names,
        vertices_mm: surface.vertices,
        vertex_body: surface.owners,
        triangles: surface.facets,
        curve,
        peaks,
        has_resonance,
        modes,
        damping_ratio: DAMPING,
        base_acceleration_m_s2: BASE_ACCELERATION,
        highest_mode_hz,
        element_count: model.elements().len(),
        warnings,
    })
}
