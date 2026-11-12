//! Inspection: measure bodies, describe the document, sketches and model topology.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use serde_json::{Value, json};
use solidcraft_sketch::CurveKind;

use super::CommandSpec;
use crate::params::{bad, bool_, str_, string_list};
use crate::schema::inspect::{BodiesDetail, DocumentBodies, TimelineDetail, TimelineRow};
use crate::schema::inspect::{
    BodyFailure, BodyFeaturesParams, BodyFigures, BodyListParams, BodyMeasurement, BodyName, BodyNames, BodyOutcome, BoundingBox, CommandEntry,
    CommandsParams, CurveShape, DocumentBody, DocumentBodyDetail, DocumentInspectParams, DocumentInspectResult, DocumentParam, DocumentSketch,
    FaceCylinder, KernelCounts, ListedMaterial, MaterialAssignParams, MaterialAssignResult, MaterialListParams, MaterialListResult, MeasureParams,
    MeasureResult, MeasureTotal, Measured, MeasuredBodies, MeasuredBody, ModelBodyFeatures, ModelEdge, ModelEdges, ModelFace, ModelFaces,
    ModelThread, ModelThreads, ModelThreadsParams, SketchConstraint, SketchCurve, SketchInspectParams, SketchInspected, SketchPoint, SketchProfile,
    SketchWire, SplineTangentHandle, TimelineEntry, TimelineSummary, WireArcInfo,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const BODY_LIST_CONTRACT: TypedContract<BodyListParams> = TypedContract::new();
const THREADS_CONTRACT: TypedContract<ModelThreadsParams> = TypedContract::new();
const BODY_FEATURES_CONTRACT: TypedContract<BodyFeaturesParams> = TypedContract::new();
const MEASURE_CONTRACT: TypedContract<MeasureParams> = TypedContract::new();
const DOCUMENT_INSPECT_CONTRACT: TypedContract<DocumentInspectParams> = TypedContract::new();
const SKETCH_INSPECT_CONTRACT: TypedContract<SketchInspectParams> = TypedContract::new();
const MATERIAL_ASSIGN_CONTRACT: TypedContract<MaterialAssignParams> = TypedContract::new();
const MATERIAL_LIST_CONTRACT: TypedContract<MaterialListParams> = TypedContract::new();
const COMMANDS_CONTRACT: TypedContract<CommandsParams> = TypedContract::new();

#[path = "appearance_chain.rs"]
mod appearance_chain;

#[path = "appearance_gestures.rs"]
pub(super) mod appearance_gestures;

#[path = "body_features.rs"]
mod body_features;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("inspect.colored_components", "Component Colors", colored_components)
        .at("SOLID", "INSPECT").icon("colored_components").noundo()
        .params("enabled?: bool (omitted: toggle); temporary component colours, without changing appearances"),
    CommandSpec::new("inspect.measure", "Measure", crate::checked!("inspect.measure", measure))
        .at("SOLID", "INSPECT")
        .icon("measure")
        .key("I")
        .noundo().read_only()
        .contract(&MEASURE_CONTRACT)
        .output(output_schema_for::<MeasureResult>)
        .params("bodies?: [names] (default all); names_only?: bool (each world body's name and its definition's, nothing measured) → closed per body; volume_mm3, center_of_mass, density_g_cm3 and mass_g only for closed, consistently oriented bodies; area, bbox and counts for all; total.open_bodies_skipped counts bodies excluded from volume/mass totals; or items: [1-2 selections] (face/edge: type, body, and point on it or index from model.faces/model.edges, or both) → length/area/volume; a face's perimeter_mm, with outer_perimeter_mm and inner_perimeter_mm on a flat face with holes; diameter, radius, circumference or arc_length/angle_deg of circles, arcs and cylinders; slot sizes; min (distance_mm, from, to), max, center_distance (centre to centre, or from an axis) and angle"),
    CommandSpec::new("document.inspect", "Inspect Design", crate::checked!("document.inspect", inspect))
        .icon("inspect")
        .noundo().read_only()
        .contract(&DOCUMENT_INSPECT_CONTRACT)
        .output(output_schema_for::<DocumentInspectResult>)
        .params(
            "measure?: bool (include body measurements); sketch_counts?: bool (each sketch also gives its plane and its number of points and constraints); \
             timeline?: true (default) | \"summary\" (each feature as id, name, type, with error, warning, suppressed and rolled_back only when set) | false (left out); \
             bodies?: true (default) | \"names\" (only the body names, nothing measured) | false (left out)",
        ),
    CommandSpec::new("document.revision", "Design Revision", revision).icon("inspect").noundo().params(
        "→ document_id (this open design's id: kept while it stays open, another for each design started or opened), \
         revision (moves whenever the design, what is hidden or a component's uncaptured position changes, \
         never on a selection or a view; from one counter for the whole run, so it never repeats). Read-only",
    ),
    CommandSpec::new("sketch.inspect", "Inspect Sketch", crate::checked!("sketch.inspect", sketch_inspect))
        .at("SKETCH", "INSPECT")
        .icon("inspect")
        .noundo().read_only()
        .contract(&SKETCH_INSPECT_CONTRACT)
        .output(output_schema_for::<SketchInspected>)
        .params("sketch?: id|name (default active)"),
    CommandSpec::new("model.edges", "List Edges", crate::checked!("model.edges", model_edges))
        .icon("inspect")
        .noundo().read_only()
        .contract(&BODY_LIST_CONTRACT)
        .output(output_schema_for::<ModelEdges>)
        .params("body: name"),
    CommandSpec::new("model.faces", "List Faces", crate::checked!("model.faces", model_faces))
        .icon("inspect")
        .noundo().read_only()
        .contract(&BODY_LIST_CONTRACT)
        .output(output_schema_for::<ModelFaces>)
        .params(
        "body: name → per face: index, name (what face.delete, sheet.convert `face`, solid.shell … take in place of a point), exact, area, centroid (not always on the face), \
         plane_normal, point (a point on the face that picks it), cylinder? ({origin, axis, radius} of a cylindrical face), \
         sheet_face? (a folded sheet's face: `sheet:<body>:<panel>:<side>`, panel base or a flange's tag F<feature>.<k>, side top | bottom | rim.<edge> | bend.inner | bend.outer; \
         taken wherever a face name is, and it holds while sizes change and flanges come and go)",
    ),
    CommandSpec::new("model.threads", "List Threads", crate::checked!("model.threads", model_threads))
        .icon("inspect")
        .noundo()
        .contract(&THREADS_CONTRACT)
        .output(output_schema_for::<ModelThreads>),
    CommandSpec::new("model.body_features", "List Body Features", crate::checked!("model.body_features", body_features::model_body_features))
        .icon("inspect")
        .noundo()
        .contract(&BODY_FEATURES_CONTRACT)
        .output(output_schema_for::<ModelBodyFeatures>)
        .params(
            "body?: name (a placed copy's name means the body it shows; default every body) → bodies: [{body, features: [{id, name}]}], \
             in timeline order the feature that created each body, every later one that changed it, and those of bodies consumed into it. Read-only",
        ),
    CommandSpec::new("material.assign", "Physical Material", crate::checked!("material.assign", physical_material))
        .at("SOLID", "MODIFY")
        .icon("material")
        .contract(&MATERIAL_ASSIGN_CONTRACT)
        .output(output_schema_for::<MaterialAssignResult>)
        .params("bodies: [names]; material: name (Steel, Aluminum, ABS Plastic, …, or one made with material.create; `material.list`)"),
    CommandSpec::new("appearance.assign", "Appearance", crate::checked!("appearance.assign", appearance))
        .at("SOLID", "MODIFY")
        .icon("appearance")
        .key("A")
        .contract(&ASSIGN_CONTRACT)
        .output(crate::schema::output_schema_for::<AppearanceAssignResult>)
        .params(
        "bodies?: [names]; faces?: [[x,y,z] on faces] (body?: which body, default the nearest); components?: [names or ids, or occurrence names]; \
             appearance?: library name or stable starter.* ID (appearance.library) | color: \"#rrggbb\" or [r, g, b] (0–255), opacity? (0–1, default 1); \
             name?: inert copied look label (requires color, mutually exclusive with appearance); pbr?: {metallic:0..1, roughness:0.045..1, dielectric_ior:1..3} (requires color/appearance and opacity 1); a starter ID or registered starter name resolves its recipe before color overrides; omitted pbr retains that response and pbr: null explicitly clears it. Unavailable glass recipes refuse through either selector. Other custom labels with an explicit color retain legacy scalar behavior. Neither appearance nor color (or color: null): clear, so the face follows its body, the body its component, the component its material",
    ),
    CommandSpec::new("appearance.copy", "Copy Look", appearance_gestures::copy).icon("copy").noundo().key("Ctrl+Alt+C")
        .params("target: appearance.chain target. Copies its resolved look to a transient session clipboard. No document change or dialogs."),
    CommandSpec::new("appearance.paste", "Paste Look", appearance_gestures::paste).icon("paste").key("Ctrl+Alt+V")
        .params("bodies/faces/components: appearance.assign targets. Applies the copied look atomically in one undo step; no dialogs."),
    CommandSpec::new("appearance.favorite", "Favorite Look", appearance_gestures::favorite).icon("star").noundo().doc_only()
        .params("look: appearance.assign look; favorite: bool → validated look and preference request; UI library preference only, no document change or dialogs."),
    CommandSpec::new("appearance.chain", "Appearance Chain", appearance_chain::chain).icon("inspect").noundo().params(
        "target: {body: name} | {body: name, face: zero-based model.faces index} | {component: name or id}; a placed copy's name (`KB (K:2)`, `K:2`) means the body or component it shows. Returns steps [{step: face|body|component|material, look: {name,color,opacity,pbr?} or null}] in precedence order and active (step or null). Component material is the common default look of its directly owned bodies, null if mixed/empty. Imported paint is a display fallback outside the four steps. Read-only; no dialogs",
    ),
    CommandSpec::new("appearance.replace", "Replace Appearance", appearance_chain::replace).icon("appearance").params(
        "from: library name | saved look {name,color,opacity,pbr?} (copy appearance.list in_design); to: library name | assignment look {appearance? | color, name?, opacity?, pbr?} (appearance.assign rules; no clear). Matches the full look including name and PBR. scope?: {bodies?: [names], components?: [names or ids]} (a placed copy's name means the body or component it shows), at most 10000 targets; body scope includes face overrides, component scope selects only the component override. Omitted scope means all overrides, including hidden/suppressed ones; physical defaults are unchanged. Duplicate targets count once. Returns count of rewritten overrides; zero adds no history. One undo step, atomic; no dialogs. Designs above 100000 overrides refuse",
    ),
    CommandSpec::new("appearance.library", "Appearance Library", crate::checked!("appearance.library", appearance_library))
        .icon("library")
        .noundo()
        .contract(&LIBRARY_CONTRACT)
        .output(crate::schema::output_schema_for::<AppearanceLibraryResult>)
        .params("→ 20 legacy scalar appearances, six opaque PBR finishes and 40 starter recipes with IDs, categories, provenance, requirements and availability; six starter glass recipes are unavailable"),
    CommandSpec::new("appearance.list", "List Appearances", crate::checked!("appearance.list", appearance_list))
        .icon("list")
        .noundo()
        .contract(&LIST_CONTRACT)
        .output(crate::schema::output_schema_for::<AppearanceListResult>)
        .params("body?: name (or a placed copy's, `KB (K:2)`: the body it shows) → assigned appearances; with body, its resolved look and its faces' (face index, look)"),
    CommandSpec::new("material.list", "List Materials", crate::checked!("material.list", material_list))
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&MATERIAL_LIST_CONTRACT)
        .output(output_schema_for::<MaterialListResult>)
        .params("→ built-in and design-owned materials (user: true, look, optional physical_card/physical_validation/density_comparison). Read-only; candidate engineering records do not certify sources or supply a solver"),
    CommandSpec::new("engine.commands", "List Commands", crate::checked!("engine.commands", commands))
        .icon("list")
        .noundo()
        .contract(&COMMANDS_CONTRACT)
        .output(output_schema_for::<Vec<CommandEntry>>),
    CommandSpec::new("engine.describe", "Describe Command", crate::checked!("engine.describe", describe))
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&DESCRIBE_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::inspect::DescribeResult>)
        .params("command: a command id. Returns {id, label, params, input, output} (and param_warnings in the warn parameter-check mode): the command's parameter documentation and, once the command has a typed contract, the JSON Schemas (2020-12) of its parameters and of a successful result (null otherwise). Read-only."),
];

/// Does `body` enclose a volume? A B-rep body does when every shell is closed and consistently
/// oriented (not a surface body), a mesh body when every triangle side is shared by exactly two
/// triangles. Only that is asked: other findings of a full validity check (faces that don't
/// mesh, tolerance gaps of an imported solid) don't take a closed body's volume away, and the
/// check stays cheap for designs with many bodies.
pub(crate) fn encloses_volume(body: &solidcraft_kernel::Body) -> bool {
    if body.is_mesh() { body.is_closed_mesh() } else { !body.is_surface() }
}

pub fn measure_json(b: &solidcraft_doc::ModelBody) -> Value {
    serde_json::to_value(measure_sheet(b, None)).unwrap_or_default()
}

pub(super) fn measure_body(
    b: &solidcraft_doc::ModelBody,
    definitions: Option<&solidcraft_doc::ModelState>,
) -> Result<solidcraft_kernel::BodyMeasure> {
    let mut measured = b.measure()?;
    if let Some(state) = definitions
        && let Some(sheet) = state.sheets.iter().find(|sheet| sheet.body == b.definition_name() && !sheet.perforations.is_empty())
    {
        let definition = state.body(b.definition_name()).ok_or_else(|| EngineError::Other(format!("missing sheet body `{}`", sheet.body)))?;
        // A body transfer moves the geometry and sheet metadata together. Apply only
        // the placement relative to the current definition, so holes move once.
        let inverse = solidcraft_doc::mat_inverse(&definition.source_transform)
            .ok_or_else(|| EngineError::Other(format!("invalid sheet placement for `{}`", sheet.body)))?;
        let placement = solidcraft_doc::mat_mul(&b.source_transform, &inverse);
        sheet.correct_perforation_properties(&mut measured, &placement)?;
    }
    Ok(measured)
}

fn measure_sheet(b: &solidcraft_doc::ModelBody, definitions: Option<&solidcraft_doc::ModelState>) -> BodyMeasurement {
    let name = b.name.clone();
    match measure_body(b, definitions) {
        Ok(m) => {
            let closed = encloses_volume(&b.body);
            let outcome = BodyOutcome::Measured(BodyFigures {
                closed,
                volume_mm3: closed.then_some(m.volume),
                area_mm2: m.area,
                center_of_mass: closed.then(|| m.centroid.into()),
                bbox: BoundingBox { min: m.bbox.min.into(), max: m.bbox.max.into() },
                faces: count(m.merged.faces),
                edges: count(m.merged.edges),
                vertices: count(m.merged.vertices),
                face_types: m.merged.face_types,
                kernel: KernelCounts { faces: m.faces, edges: m.edges, vertices: m.vertices, shells: m.shells },
                measured_by: m.measured_by.to_string(),
                notes: m.notes,
            });
            BodyMeasurement { name, outcome }
        }
        Err(e) => measure_failed(name, e.to_string()),
    }
}

fn measure_failed(name: String, error: String) -> BodyMeasurement {
    BodyMeasurement { name, outcome: BodyOutcome::Failed(BodyFailure { error, faces: None, edges: None, vertices: None }) }
}

/// A topology count as the replies give it.
fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(0)
}

/// A reply as JSON: the commands build their replies from the types their output schemas
/// describe, so the two can't drift apart.
fn typed_reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

impl BodyMeasurement {
    /// The volume, when the body has one that is a number.
    fn volume(&self) -> Option<f64> {
        match &self.outcome {
            BodyOutcome::Measured(f) => f.volume_mm3.filter(|v| v.is_finite()),
            BodyOutcome::Failed(_) => None,
        }
    }
}

fn measure(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.measure";
    if let Some(items) = p.get("items") {
        return super::measure_sel::measure_items(s, items);
    }
    let want = string_list(cmd, p, "bodies")?;
    let definitions = s.model.state();
    let st = s.world_state()?;
    let selected: Vec<_> = st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).collect();
    if !want.is_empty() && selected.len() != want.len() {
        return Err(bad(cmd, "unknown body name"));
    }
    if bool_(p, "names_only").unwrap_or(false) {
        let bodies: Vec<_> = selected.iter().map(|b| BodyName { name: b.name.clone(), definition: b.definition_name().to_string() }).collect();
        let measured = Measured::Names(BodyNames { body_count: bodies.len(), bodies });
        return typed_reply(cmd, MeasureResult { measured, param_warnings: Vec::new() });
    }
    // Each body is measured on its own: on all cores, in body order.
    let measurements: Vec<BodyMeasurement> = solidcraft_kernel::map_on_threads(&selected, 16, |b| measure_sheet(b, Some(&definitions)))
        .into_iter()
        .zip(&selected)
        .map(|(m, b)| m.unwrap_or_else(|| measure_failed(b.name.clone(), "measuring this body failed".into())))
        .collect();
    // Mass from each body's material.
    let mut bodies: Vec<MeasuredBody> = Vec::with_capacity(measurements.len());
    for (measurement, body) in measurements.into_iter().zip(selected) {
        let source = body.definition_name();
        // Every body's material, measured or not: naming it takes no measuring.
        let mut b = MeasuredBody { measurement, material: s.doc.body_material(source), density_g_cm3: None, mass_g: None, sheet_metal: None };
        if let Some(v) = b.measurement.volume() {
            let rho = s.doc.density(source);
            b.density_g_cm3 = Some(rho);
            b.mass_g = Some(v * rho / 1000.0);
        }
        // Only Measure exposes sheet boundary counts; document.inspect keeps kernel counts.
        if let Some(sheet) = definitions.sheets.iter().find(|sheet| sheet.body == source) {
            let (df, de, dv) = sheet.round_hole_correction();
            match &mut b.measurement.outcome {
                BodyOutcome::Measured(f) => {
                    f.faces = count(f.kernel.faces) + df;
                    f.edges = count(f.kernel.edges) + de;
                    f.vertices = count(f.kernel.vertices) + dv;
                }
                BodyOutcome::Failed(x) => (x.faces, x.edges, x.vertices) = (Some(df), Some(de), Some(dv)),
            }
            b.sheet_metal = Some(true);
        }
        bodies.push(b);
    }
    fn figures(b: &MeasuredBody) -> Option<&BodyFigures> {
        match &b.measurement.outcome {
            BodyOutcome::Measured(f) => Some(f),
            BodyOutcome::Failed(_) => None,
        }
    }
    let counted = |n: Option<i64>| n.and_then(|n| u64::try_from(n).ok());
    let faces = |b: &MeasuredBody| match &b.measurement.outcome {
        BodyOutcome::Measured(f) => (counted(Some(f.faces)), counted(Some(f.edges)), counted(Some(f.vertices))),
        BodyOutcome::Failed(x) => (counted(x.faces), counted(x.edges), counted(x.vertices)),
    };
    let total = MeasureTotal {
        open_bodies_skipped: bodies.iter().filter(|b| figures(b).is_some_and(|f| !f.closed)).count(),
        volume_mm3: bodies.iter().filter_map(|b| b.measurement.volume()).sum(),
        area_mm2: bodies.iter().filter_map(|b| figures(b)).map(|f| f.area_mm2).filter(|a| a.is_finite()).sum(),
        mass_g: bodies.iter().filter_map(|b| b.mass_g).filter(|m| m.is_finite()).sum(),
        faces: bodies.iter().filter_map(|b| faces(b).0).sum(),
        edges: bodies.iter().filter_map(|b| faces(b).1).sum(),
        vertices: bodies.iter().filter_map(|b| faces(b).2).sum(),
    };
    typed_reply(
        cmd,
        MeasureResult { measured: Measured::Bodies(MeasuredBodies { body_count: bodies.len(), bodies, total }), param_warnings: Vec::new() },
    )
}

/// A 2D position as the replies give it.
fn at2(p: solidcore_record::Vec2) -> [f64; 2] {
    [p.x, p.y]
}

/// Sketch `id`, placed where the placed model `world` has it (its component's placement).
fn sketch_json(s: &Session, world: &solidcraft_doc::ModelState, id: u64) -> Option<SketchInspected> {
    let state = s.model.state();
    let ss = state.sketch(id)?;
    let plane = world.sketch(id).map(|w| w.plane).unwrap_or(ss.plane);
    let sk = &ss.sketch;
    let curves: Vec<SketchCurve> = sk
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let det = ss.report.curve_determined.get(i).copied().unwrap_or(false);
            let pid = |k: usize| sk.points.get(k).map(|p| p.id.clone()).unwrap_or_default();
            let at = |k: usize| sk.point(k).map(at2);
            let shape = match c.kind {
                CurveKind::Line { a, b } => CurveShape::Line { start: pid(a), end: pid(b), start_at: at(a), end_at: at(b) },
                CurveKind::Circle { c: cc, r } => CurveShape::Circle { center: pid(cc), center_at: at(cc), radius: r },
                CurveKind::Arc { c: cc, a, b } => CurveShape::Arc {
                    center: pid(cc),
                    start: pid(a),
                    end: pid(b),
                    center_at: at(cc),
                    start_at: at(a),
                    end_at: at(b),
                    radius: sk.radius(i),
                },
                CurveKind::Ellipse { c: cc, m, r } => {
                    CurveShape::Ellipse { center: pid(cc), major: pid(m), center_at: at(cc), major_at: at(m), minor_radius: r }
                }
                CurveKind::Spline { ref pts, control, degree, periodic, ref nurbs, ref handles } => CurveShape::Spline {
                    points: pts.iter().map(|q| pid(*q)).collect(),
                    points_at: pts.iter().map(|q| at(*q)).collect(),
                    control,
                    degree,
                    periodic,
                    tangent_handles: handles
                        .iter()
                        .map(|&point| SplineTangentHandle {
                            point: pid(point),
                            at: at(point),
                            curve: sk
                                .curves
                                .iter()
                                .find(|candidate| {
                                    candidate.spline_handle_of.as_deref() == Some(c.id.as_str())
                                        && matches!(candidate.kind, CurveKind::Line { a, b } if a == point || b == point)
                                })
                                .map(|candidate| candidate.id.clone()),
                        })
                        .collect(),
                    // A control spline with its own knots (a projected free-form edge) says them.
                    knots: nurbs.as_ref().map(|n| n.knots.clone()),
                    weights: nurbs.as_ref().filter(|n| !n.weights.is_empty()).map(|n| n.weights.clone()),
                },
                CurveKind::Conic { a, b, apex, rho } => {
                    CurveShape::Conic { start: pid(a), end: pid(b), apex: pid(apex), start_at: at(a), end_at: at(b), apex_at: at(apex), rho }
                }
            };
            SketchCurve {
                id: c.id.clone(),
                shape,
                construction: c.construction,
                fixed: sk.curve_locked(i),
                fully_constrained: det,
                link: c.link.clone(),
            }
        })
        .collect();
    let locked = sk.locked_points();
    let points: Vec<SketchPoint> = sk
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| SketchPoint {
            id: p.id.clone(),
            at: at2(p.pos),
            world: plane.to_world(p.pos).into(),
            fixed: locked.get(i).copied().unwrap_or(false),
            link: p.link.clone(),
            fully_constrained: ss.report.point_determined.get(i).copied().unwrap_or(false),
        })
        .collect();
    let constraints: Vec<SketchConstraint> = sk
        .constraints
        .iter()
        .map(|c| {
            let mut kind = match serde_json::to_value(&c.kind) {
                Ok(Value::Object(kind)) => kind,
                _ => Default::default(),
            };
            // The constraint's own keys win over any of the kind's.
            for key in ["id", "name", "param", "expression"] {
                kind.remove(key);
            }
            SketchConstraint {
                id: c.id.clone(),
                name: c.kind.name().to_string(),
                param: c.param.clone(),
                expression: c.param.as_ref().map(|p| s.doc.param(p).map(|x| x.expr.clone())),
                kind,
            }
        })
        .collect();
    let profiles: Vec<SketchProfile> = ss
        .profiles
        .iter()
        .enumerate()
        // A loop records one entry per piece it walks, and a conic or spline is walked as many
        // short pieces: each loop names a curve once, in loop order.
        .map(|(i, p)| SketchProfile {
            index: i,
            area_mm2: p.area,
            centroid_sketch: at2(p.centroid),
            curves: solidcraft_sketch::distinct_curves(&p.outer_curves),
            holes: p.hole_curves.iter().map(|h| solidcraft_sketch::distinct_curves(h)).collect(),
        })
        .collect();
    // Planar loops the 3D sketch curves close in planes of their own (#741): the plane is given
    // as an origin and a unit normal, and the centroid is in the same frame as the sketch plane.
    let profiles_3d: Vec<Value> = ss
        .profiles_3d
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "index": i,
                "area_mm2": p.area,
                "plane": { "origin": p.plane.origin, "normal": p.plane.normal() },
                "centroid": p.centroid,
                "curves": p.outer_curves,
                "holes": p.hole_curves,
            })
        })
        .collect();
    let wires = sk
        .wires
        .iter()
        .map(|w| SketchWire {
            id: w.id.clone(),
            points: w.pts.len(),
            start: w.pts.first().map(|&p| p.into()),
            end: w.pts.last().map(|&p| p.into()),
            link: w.link.clone(),
            arc: w.arc.map(|a| WireArcInfo {
                center: a.center.into(),
                radius: a.radius,
                start: a.start.into(),
                end: a.end.into(),
                sweep_deg: solidcraft_sketch::arc_sweep(&a).to_degrees(),
            }),
        })
        .collect();
    Some(SketchInspected {
        id,
        name: ss.name.clone(),
        plane: serde_json::to_value(ss.plane).unwrap_or_default(),
        status: ss.report.status.into(),
        dof: ss.report.dof,
        fully_constrained: ss.report.fully_constrained(),
        failing: ss.report.failing.clone(),
        curves,
        points,
        constraints,
        profiles,
        profiles_3d,
        links: serde_json::to_value(&sk.links).unwrap_or_default(),
        wires,
        visible: crate::view::sketch_shown(s, id),
        param_warnings: Vec::new(),
    })
}

fn sketch_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::sketch_store::target(s, p, "sketch.inspect")?;
    let inspected = sketch_json(s, &*s.world_state()?, id)
        .ok_or_else(|| EngineError::Other("that sketch is not evaluated (rolled back or not a sketch)".into()))?;
    typed_reply("sketch.inspect", inspected)
}

/// Revisions of every design of this run (#6646).
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

/// The design as `document.revision` last saw it, which its revision names.
pub(crate) struct Followed {
    /// Held weakly: a design something holds weakly is never changed in place (`Arc::make_mut`
    /// moves it), and its address can't go to another design meanwhile.
    doc: Weak<solidcraft_doc::Document>,
    visibility: crate::Visibility,
    pending_moves: BTreeMap<u64, solidcraft_doc::Mat>,
    revision: u64,
}

/// A new design's id: one for this run, then a number, so no two designs of one run, or of two
/// runs, have the same.
pub(crate) fn new_document_id() -> String {
    static RUN: OnceLock<u64> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let run = RUN.get_or_init(|| {
        let started = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        // Hashed with this process's random keys (where the system has them), so two runs differ
        // even with the clock set back.
        RandomState::new().hash_one(started)
    });
    format!("{run:016x}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

impl Session {
    /// The revision of what the design holds now (`document.revision`), a new one when it changed
    /// since last asked. Changes between two asks that end where they began (an edit undone) need
    /// no new one: the design is the one that revision named.
    pub fn design_revision(&mut self) -> u64 {
        let seen = |f: &Followed| {
            std::ptr::eq(f.doc.as_ptr(), Arc::as_ptr(&self.doc)) && f.visibility == self.visibility && f.pending_moves == self.pending_moves
        };
        match self.followed.as_ref().filter(|f| seen(f)) {
            Some(f) => f.revision,
            None => {
                let revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
                let (doc, visibility, pending_moves) = (Arc::downgrade(&self.doc), self.visibility.clone(), self.pending_moves.clone());
                self.followed = Some(Followed { doc, visibility, pending_moves, revision });
                revision
            }
        }
    }
}

/// `document.revision`: the design's id and [`Session::design_revision`].
fn revision(s: &mut Session, _p: &Value) -> Result<Value> {
    let revision = s.design_revision();
    Ok(json!({"document_id": s.document_id, "revision": revision}))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let timeline_detail = TimelineDetail::read(p.get("timeline")).map_err(|e| bad("document.inspect", e))?;
    let bodies_detail = BodiesDetail::read(p.get("bodies")).map_err(|e| bad("document.inspect", e))?;
    let st = s.model.state();
    let (vals, perr) = s.doc.param_values();
    let params: Vec<DocumentParam> = s
        .doc
        .params
        .iter()
        .map(|p| DocumentParam {
            name: p.name.clone(),
            expression: p.expr.clone(),
            unit: p.unit.clone(),
            value: vals.get(&p.name).map(|v| v.v),
            model: p.model,
            comment: p.comment.clone(),
            error: perr.get(&p.name).cloned(),
        })
        .collect();
    let marker = s.doc.marker.unwrap_or(s.doc.features.len());
    let timeline: Option<Vec<TimelineRow>> = (timeline_detail != TimelineDetail::Left).then(|| {
        s.doc
            .features
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let r = s.model.result(f.id);
                let (error, warning) = (r.and_then(|r| r.error.clone()), r.and_then(|r| r.warning.clone()));
                if timeline_detail == TimelineDetail::Summary {
                    return TimelineRow::Summary(TimelineSummary {
                        id: f.id,
                        name: f.name.clone(),
                        kind: f.kind.type_name().to_string(),
                        error,
                        warning,
                        suppressed: f.suppressed.then_some(true),
                        rolled_back: (i >= marker).then_some(true),
                    });
                }
                TimelineRow::Full(TimelineEntry {
                    id: f.id,
                    name: f.name.clone(),
                    kind: f.kind.type_name().to_string(),
                    suppressed: f.suppressed,
                    rolled_back: i >= marker,
                    error,
                    warning,
                    ms: r.map(|r| r.ms),
                })
            })
            .collect()
    });
    let definitions = s.model.state();
    // Only bodies given in full are measured.
    let with_measure = bool_(p, "measure").unwrap_or(false) && bodies_detail == BodiesDetail::Full;
    let with_counts = bool_(p, "sketch_counts").unwrap_or(false);
    // Each body is measured on its own: on all cores, in body order (as `inspect.measure`).
    let mut measured =
        if with_measure { solidcraft_kernel::map_on_threads(&st.bodies, 16, |b| measure_sheet(b, Some(&definitions))) } else { Vec::new() }
            .into_iter();
    let bodies: Vec<DocumentBody> = st
        .bodies
        .iter()
        .map(|b| {
            let detail = match measured.next() {
                Some(m) => {
                    DocumentBodyDetail::Measured(Box::new(m.unwrap_or_else(|| measure_failed(b.name.clone(), "measuring this body failed".into()))))
                }
                None => DocumentBodyDetail::Listed { name: b.name.clone(), feature: b.feature },
            };
            // Editing a sketch rolls the timeline back; a body deleted further on is `removed_later`.
            DocumentBody { detail, visible: s.body_visible(&b.name), removed_later: s.body_removed_later(&b.name).then_some(true) }
        })
        .collect();
    let bodies = match bodies_detail {
        BodiesDetail::Full => Some(DocumentBodies::Full(bodies)),
        BodiesDetail::Names => Some(DocumentBodies::Names(st.bodies.iter().map(|b| b.name.clone()).collect())),
        BodiesDetail::Left => None,
    };
    let sketches: Vec<DocumentSketch> = st
        .sketches
        .iter()
        .map(|ss| {
            let mut v = DocumentSketch {
                id: ss.feature,
                name: ss.name.clone(),
                dof: ss.report.dof,
                status: ss.report.status.into(),
                curves: ss.sketch.curves.len(),
                profiles: ss.profiles.len(),
                visible: crate::view::sketch_shown(s, ss.feature),
                plane: None,
                points: None,
                constraints: None,
            };
            // A summary that stands in for the sketch's contents (the compact MCP inspect_design).
            if with_counts {
                v.plane = Some(serde_json::to_value(ss.plane).unwrap_or_default());
                v.points = Some(ss.sketch.points.len());
                v.constraints = Some(ss.sketch.constraints.len());
            }
            v
        })
        .collect();
    typed_reply(
        "document.inspect",
        DocumentInspectResult {
            name: s.doc.name.clone(),
            units: s.doc.units.clone(),
            metadata: s.doc.metadata.0.clone(),
            path: s.path.clone(),
            dirty: s.is_dirty(),
            revision: s.revision,
            active_sketch: s.active_sketch,
            marker,
            params,
            timeline,
            bodies,
            sketches,
            selection: serde_json::to_value(&s.selection).unwrap_or_default(),
            undo: s.undo.len(),
            redo: s.redo.len(),
            hidden_origin: s.visibility.hidden_origin.clone(),
            param_warnings: Vec::new(),
        },
    )
}

fn body_of<'a>(st: &'a solidcraft_doc::ModelState, p: &Value, cmd: &str) -> Result<&'a solidcraft_doc::ModelBody> {
    let n = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?;
    st.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))
}

fn model_edges(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.world_state()?;
    let b = body_of(&st, p, "model.edges")?;
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let names = solidcraft_doc::naming::edge_names(b);
    let edges = b
        .body
        .edges(tol)?
        .iter()
        .map(|e| ModelEdge {
            index: e.index,
            name: names.get(e.index).cloned(),
            mid: e.mid.into(),
            length: e.length,
            start: e.points.first().map(|&p| p.into()),
            end: e.points.last().map(|&p| p.into()),
        })
        .collect();
    reply("model.edges", ModelEdges { body: b.name.clone(), edges, param_warnings: Vec::new() })
}

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn model_faces(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.world_state()?;
    let b = body_of(&st, p, "model.faces")?;
    let names = solidcraft_doc::naming::face_names(b);
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let spots = solidcraft_doc::naming::face_spots(b);
    // A folded sheet's faces also have the readable names its sheet gives them, worked out in
    // the body's own coordinates.
    let model = s.model.state();
    let sheet_names = model.body(&b.name).map(|own| solidcraft_doc::sheet::body_sheet_face_names(&model, own)).unwrap_or_default();
    let faces = b
        .body
        .faces(tol)?
        .iter()
        .map(|f| {
            let spot = spots.get(f.index).copied().flatten();
            let cylinder = match spot.and_then(|x| x.surface) {
                Some(solidcraft_kernel::FaceSurface::Cylinder { origin, axis, radius }) => {
                    Some(FaceCylinder { origin: origin.into(), axis: solidcore_record::Vec3::from(axis).into(), radius })
                }
                _ => None,
            };
            ModelFace {
                index: f.index,
                name: names.get(f.index).cloned(),
                exact: solidcraft_doc::naming::face_name_exact(b, f.index),
                area: f.area,
                centroid: f.centroid.into(),
                plane_normal: f.plane_normal.map(Into::into),
                point: spot.map(|x| x.point.into()),
                cylinder,
                sheet_face: sheet_names.get(f.index).and_then(|n| n.as_ref().map(|n| n.to_string())),
            }
        })
        .collect();
    reply("model.faces", ModelFaces { body: b.name.clone(), faces, param_warnings: Vec::new() })
}

const DESCRIBE_CONTRACT: crate::schema::TypedContract<crate::schema::inspect::DescribeParams> = crate::schema::TypedContract::new();

/// A command's prose documentation and, for converted commands, its schemas.
fn describe(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("command").and_then(Value::as_str).ok_or_else(|| crate::params::bad("engine.describe", "`command` must be a command id"))?;
    if let Some(entry) = crate::plugin::describe_entry(s, id)? {
        return Ok(entry);
    }
    let spec = super::find_command(id).ok_or_else(|| crate::EngineError::UnknownCommand(id.to_string()))?;
    let reply = crate::schema::inspect::DescribeResult {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        params: spec.params.to_string(),
        input: spec.contract.map(|c| c.input_schema().to_value()),
        output: spec.output.map(|f| f().to_value()),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn commands(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut list: Vec<CommandEntry> = crate::command_specs().iter().map(|c| c.info(s).into()).collect();
    for (package_id, path) in &s.plugin_state.packages {
        if let Ok(package) = crate::plugin::read_package(path)
            && let Some(runtime) = package.manifest.runtime
        {
            for entry in runtime.entries {
                list.push(CommandEntry {
                    id: format!("{package_id}.{}", entry.name),
                    label: entry.name,
                    tab: String::new(),
                    panel: String::new(),
                    icon: "play".into(),
                    shortcut: None,
                    params: "approval, params: declared entry inputs (engine.describe)".into(),
                    enabled: !s.disabled_plugins.contains(package_id),
                    disabled_reason: s.disabled_plugins.contains(package_id).then(|| "plugin is disabled".into()),
                    planned: None,
                });
            }
        }
    }
    typed_reply("engine.commands", list)
}

fn model_threads(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.world_state()?;
    reply("model.threads", ModelThreads { threads: st.threads.iter().map(ModelThread::from).collect(), param_warnings: Vec::new() })
}

fn physical_material(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "material.assign";
    let bodies = string_list(cmd, p, "bodies")?;
    let material = str_(p, "material").ok_or_else(|| bad(cmd, "`material` must be a material name"))?;
    let canonical = s.doc.material_name(material).ok_or_else(|| bad(cmd, format!("unknown material `{material}` (see material.list)")))?;
    let st = s.model.state();
    if bodies.is_empty() || bodies.iter().any(|b| st.body(b).is_none()) {
        return Err(bad(cmd, "`bodies` must list existing bodies"));
    }
    for b in &bodies {
        if canonical == solidcraft_doc::DEFAULT_MATERIAL {
            s.doc_mut().materials.remove(b);
        } else {
            s.doc_mut().materials.insert(b.clone(), canonical.clone());
        }
    }
    typed_reply(cmd, MaterialAssignResult { bodies, material: canonical, param_warnings: Vec::new() })
}

/// A colour parameter: `"#rrggbb"` or `[r, g, b]` (0–255).
pub(crate) fn color_param(v: &Value) -> Option<[u8; 3]> {
    if let Some(t) = v.as_str() {
        let h = t.trim().trim_start_matches('#');
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let byte = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
        return Some([byte(0)?, byte(2)?, byte(4)?]);
    }
    let a = v.as_array().filter(|a| a.len() == 3)?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite() && (0.0..=255.0).contains(x)).map(|x| x.round() as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

use crate::schema::appearance::{
    AppearanceAssignParams, AppearanceAssignResult, AppearanceLibraryParams, AppearanceLibraryResult, AppearanceListParams, AppearanceListResult,
    BodyLooks, ComponentLookReply, FaceIndexLook, FaceLookReply, FaceSpot, LibraryAppearance, LookInDesign, LookReply, PbrReply, PlainAppearance,
    StarterAppearance,
};

const ASSIGN_CONTRACT: TypedContract<AppearanceAssignParams> = TypedContract::new();
const LIBRARY_CONTRACT: TypedContract<AppearanceLibraryParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<AppearanceListParams> = TypedContract::new();

/// An appearance command's reply, built from its schema's type so the two can't drift apart.
fn appearance_reply(cmd: &str, r: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(r).map_err(|e| bad(cmd, e.to_string()))
}

pub(super) fn appearance(s: &mut Session, p: &Value) -> Result<Value> {
    use solidcraft_doc::appearance::FaceLook;
    let cmd = "appearance.assign";
    let look = appearance_look(cmd, p)?;
    let bodies = string_list(cmd, p, "bodies")?;
    let st = s.model.state();
    if bodies.iter().any(|b| st.body(b).is_none()) {
        return Err(bad(cmd, "`bodies` must list existing bodies"));
    }
    // Faces: a point on each, on the named body or the nearest one.
    let mut faces: Vec<(String, solidcore_record::Vec3)> = Vec::new();
    if let Some(list) = p.get("faces") {
        let list = list.as_array().filter(|a| a.len() <= 10_000).ok_or_else(|| bad(cmd, "`faces` must list points [x, y, z]"))?;
        for v in list {
            let pt = crate::params::vec3(v)
                .or_else(|| v.get("point").and_then(crate::params::vec3))
                .ok_or_else(|| bad(cmd, "`faces` must list points [x, y, z]"))?;
            let on = |b: &solidcraft_doc::ModelBody| solidcraft_doc::appearance::face_index_at(b, pt).is_some();
            let body = match v.get("body").and_then(Value::as_str).or_else(|| str_(p, "body")) {
                Some(n) => st.body(n).filter(|b| on(b)).map(|b| b.name.clone()),
                None => st.bodies.iter().filter(|b| on(b)).min_by(|a, b| dist_to(a, pt).total_cmp(&dist_to(b, pt))).map(|b| b.name.clone()),
            };
            faces.push((body.ok_or_else(|| bad(cmd, format!("no face at {pt:?}")))?, pt));
        }
    }
    let mut comps: Vec<u64> = Vec::new();
    for c in string_list(cmd, p, "components")? {
        let id = s.component_or_placement(&c).map_err(|error| bad(cmd, error.to_string()))?.ok_or_else(|| bad(cmd, format!("no component `{c}`")))?;
        comps.push(id);
    }
    if bodies.is_empty() && faces.is_empty() && comps.is_empty() {
        return Err(bad(cmd, "give `bodies`, `faces` or `components`"));
    }
    // Persistent names of the picked faces.
    let st_names: Vec<(String, solidcore_record::Vec3, Option<String>)> = faces
        .iter()
        .map(|(b, pt)| {
            let n = st.body(b).and_then(|mb| {
                solidcraft_doc::appearance::face_index_at(mb, *pt).and_then(|i| solidcraft_doc::naming::face_names(mb).get(i).cloned())
            });
            (b.clone(), *pt, n)
        })
        .collect();
    let a = &mut s.doc_mut().appearances;
    for b in &bodies {
        match &look {
            Some(l) => a.bodies.insert(b.clone(), l.clone()),
            None => a.bodies.remove(b),
        };
    }
    for (body, pt) in &faces {
        // One entry per face: a new one replaces any at the same spot.
        a.faces.retain(|f| !(f.body == *body && f.point.dist(*pt) < 1e-9));
        if let Some(l) = &look {
            if a.faces.len() >= 100_000 {
                return Err(bad(cmd, "too many face appearances"));
            }
            let name = st_names.iter().find(|(b, p, _)| b == body && p.dist(*pt) < 1e-12).and_then(|(_, _, n)| n.clone());
            a.faces.push(FaceLook { body: body.clone(), point: *pt, look: l.clone(), name, strict_name: false, certificate: None });
        }
    }
    for c in &comps {
        match &look {
            Some(l) => a.components.insert(*c, l.clone()),
            None => a.components.remove(c),
        };
    }
    // Clearing a face: drop every entry on that face, not just at the same point.
    if look.is_none() && !faces.is_empty() {
        let st = s.model.state();
        let doc = s.doc_mut();
        let picked: Vec<(String, Option<usize>, Option<String>)> = faces
            .iter()
            .map(|(b, pt)| {
                (
                    b.clone(),
                    st.body(b).and_then(|mb| solidcraft_doc::appearance::face_index_at(mb, *pt)),
                    st_names.iter().find(|(name, p, _)| name == b && p.dist(*pt) < 1e-12).and_then(|(_, _, name)| name.clone()),
                )
            })
            .collect();
        doc.appearances.faces.retain(|f| {
            let idx = st.body(&f.body).and_then(|mb| solidcraft_doc::appearance::face_index_at(mb, f.point));
            !picked.iter().any(|(b, i, name)| {
                *b == f.body
                    && (f
                        .name
                        .as_ref()
                        .zip(name.as_ref())
                        .is_some_and(|(a, b)| solidcraft_doc::naming::strip_piece(a) == solidcraft_doc::naming::strip_piece(b))
                        || (!f.strict_name && i.is_some() && *i == idx))
            })
        });
    }
    let faces = faces.iter().map(|(body, p)| FaceSpot { body: body.clone(), point: [p.x, p.y, p.z] }).collect();
    let appearance = look.as_ref().map(LookReply::from);
    appearance_reply(cmd, AppearanceAssignResult { bodies, faces, components: comps, appearance, param_warnings: Vec::new() })
}

/// Shared look parser for assignment and atomic replacement.
pub(super) fn appearance_look(cmd: &str, p: &Value) -> Result<Option<solidcraft_doc::appearance::Look>> {
    use solidcraft_doc::appearance::{Look, library};
    // A saved label is data, not a request to resolve a recipe. Keep the explicit
    // appearance selector separate so an owned look may have any recipe's ID as its name.
    let copied_name = match p.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) if !name.is_empty() && name.chars().count() <= 128 && !name.chars().any(char::is_control) => Some(name),
        _ => return Err(bad(cmd, "`name` must be a nonempty label of at most 128 characters without controls")),
    };
    if copied_name.is_some() && (p.get("color").is_none_or(Value::is_null) || p.get("appearance").is_some_and(|value| !value.is_null())) {
        return Err(bad(cmd, "`name` requires an explicit color and cannot be combined with `appearance`"));
    }
    // Both the stable ID and a registered display name explicitly select a recipe.
    // Copied labels use `name`, so color overrides cannot bypass unsupported optics.
    let preset = str_(p, "appearance").and_then(solidcraft_doc::appearance_pack::find);
    let mut look: Option<Look> = if let Some(preset) = preset {
        // Refuse unsupported optics before any color/PBR/opacity override.
        let mut look = preset.opaque_look().map_err(|e| bad(cmd, e.to_string()))?;
        if let Some(color) = p.get("color").filter(|v| !v.is_null()) {
            look.color = color_param(color).ok_or_else(|| bad(cmd, "invalid preset color override"))?;
        }
        if let Some(opacity) = p.get("opacity").and_then(Value::as_f64).filter(|x| x.is_finite()) {
            look.opacity = opacity.clamp(0.0, 1.0);
        }
        Some(look)
    } else {
        match (str_(p, "appearance"), p.get("color")) {
            // A colour with a name: that colour, called that (a name not in the library is fine).
            (Some(n), Some(v)) if !v.is_null() => {
                let c = color_param(v).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or [r, g, b] with values 0–255"))?;
                let o = p.get("opacity").and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(1.0);
                let mut l = Look::custom(c, o);
                l.name = n.chars().take(128).collect();
                Some(l)
            }
            (Some(n), _) => {
                let mut l = if let Some(preset) = solidcraft_doc::appearance_pack::find(n) {
                    preset.opaque_look().map_err(|e| bad(cmd, e.to_string()))?
                } else {
                    library(n).ok_or_else(|| bad(cmd, format!("no appearance `{n}` in the library (appearance.library)")))?
                };
                if let Some(o) = p.get("opacity").and_then(Value::as_f64).filter(|x| x.is_finite()) {
                    l.opacity = o.clamp(0.0, 1.0);
                }
                Some(l)
            }
            (None, None | Some(Value::Null)) => None,
            (None, Some(v)) => {
                let c = color_param(v).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or [r, g, b] with values 0–255"))?;
                let o = match p.get("opacity") {
                    Some(o) => o.as_f64().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)).ok_or_else(|| bad(cmd, "`opacity` must be 0…1"))?,
                    None => 1.0,
                };
                Some(Look::custom(c, o))
            }
        }
    };
    if let Some(l) = &mut look {
        if let Some(name) = copied_name {
            l.name = name.clone();
        }
        attach_pbr(cmd, p, l)?;
    } else if p.get("pbr").is_some_and(|v| !v.is_null()) {
        return Err(bad(cmd, "`pbr` requires a color or appearance"));
    }
    Ok(look)
}

fn dist_to(b: &solidcraft_doc::ModelBody, p: solidcore_record::Vec3) -> f64 {
    let bb = b.mesh().bounds();
    let c = solidcore_record::Vec3::new(p.x.clamp(bb.min.x, bb.max.x), p.y.clamp(bb.min.y, bb.max.y), p.z.clamp(bb.min.z, bb.max.z));
    c.dist(p)
}

pub(super) fn look_json(l: &solidcraft_doc::appearance::Look) -> Value {
    let mut value = json!({"name": l.name, "color": l.hex(), "opacity": l.opacity});
    if let Some(pbr) = l.pbr {
        value["pbr"] = json!(pbr);
    }
    value
}

#[cfg(test)]
#[path = "pbr_appearance_tests.rs"]
mod pbr_tests;

pub(super) fn attach_pbr(cmd: &str, params: &Value, look: &mut solidcraft_doc::appearance::Look) -> Result<()> {
    if let Some(value) = params.get("pbr") {
        look.pbr = if value.is_null() {
            None
        } else {
            let object = value
                .as_object()
                .filter(|o| o.len() == 3 && o.keys().all(|k| matches!(k.as_str(), "metallic" | "roughness" | "dielectric_ior")))
                .ok_or_else(|| bad(cmd, "`pbr` requires exactly metallic, roughness and dielectric_ior"))?;
            let number = |key: &str| object.get(key).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("`pbr.{key}` must be a finite number")));
            Some(
                solidcraft_doc::pbr_appearance::PbrParameters::new(number("metallic")?, number("roughness")?, number("dielectric_ior")?)
                    .map_err(|e| bad(cmd, e.to_string()))?,
            )
        };
    }
    // Validate the final look, including a named preset's inherited response.
    // Both appearance assignments and physical-material defaults use this path.
    if look.pbr.is_some() {
        if let Some(opacity) = params.get("opacity")
            && opacity.as_f64().filter(|v| v.is_finite() && *v == 1.0).is_none()
        {
            return Err(bad(cmd, "opaque PBR requires numeric opacity 1"));
        }
        if look.opacity != 1.0 {
            return Err(bad(cmd, "opaque PBR requires opacity 1; refraction/transmission is not supported"));
        }
    }
    Ok(())
}

fn appearance_library(_s: &mut Session, _p: &Value) -> Result<Value> {
    let category = |name: &str| name.split(" - ").next().unwrap_or(name).to_string();
    let mut list: Vec<LibraryAppearance> = solidcraft_doc::appearance::LIBRARY
        .iter()
        .map(|(n, c, o)| {
            let look = LookReply { name: n.to_string(), color: format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]), opacity: *o, pbr: None };
            LibraryAppearance::Plain(PlainAppearance { look, category: category(n) })
        })
        .collect();
    list.extend(solidcraft_doc::appearance::OPAQUE_FINISHES.iter().filter_map(|(name, ..)| {
        let look = solidcraft_doc::appearance::library(name)?;
        Some(LibraryAppearance::Plain(PlainAppearance { look: LookReply::from(&look), category: category(name) }))
    }));
    for preset in solidcraft_doc::appearance_pack::STARTER_PACK {
        let c = preset.preview_color();
        list.push(LibraryAppearance::Starter(StarterAppearance {
            id: preset.id.to_string(),
            name: preset.name.to_string(),
            category: json!(preset.category),
            revision: preset.revision,
            color: format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
            opacity: 1.0,
            pbr: preset.opaque_look().ok().and_then(|look| look.pbr).map(PbrReply::from),
            available: preset.is_opaque(),
            requirements: json!(preset.requirements),
            provenance: json!(preset.provenance),
            recipe: json!(preset.recipe),
            preview_note: preset.preview_note.to_string(),
            future_texture_recipe: json!(preset.future_texture_recipe),
        }));
    }
    appearance_reply("appearance.library", AppearanceLibraryResult { appearances: list, param_warnings: Vec::new() })
}

fn appearance_list(s: &mut Session, p: &Value) -> Result<Value> {
    let a = &s.doc.appearances;
    // "In this design": each look used, with where.
    type Used = Vec<(solidcraft_doc::appearance::Look, Vec<String>, Vec<FaceSpot>, Vec<u64>)>;
    let mut used: Used = Vec::new();
    fn slot(used: &mut Used, l: &solidcraft_doc::appearance::Look) -> usize {
        match used.iter().position(|(u, ..)| u == l) {
            Some(i) => i,
            None => {
                used.push((l.clone(), Vec::new(), Vec::new(), Vec::new()));
                used.len() - 1
            }
        }
    }
    for (b, l) in &a.bodies {
        let i = slot(&mut used, l);
        if let Some(u) = used.get_mut(i) {
            u.1.push(b.clone());
        }
    }
    for f in &a.faces {
        let i = slot(&mut used, &f.look);
        if let Some(u) = used.get_mut(i) {
            u.2.push(FaceSpot { body: f.body.clone(), point: [f.point.x, f.point.y, f.point.z] });
        }
    }
    for (c, l) in &a.components {
        let i = slot(&mut used, l);
        if let Some(u) = used.get_mut(i) {
            u.3.push(*c);
        }
    }
    let in_design =
        used.into_iter().map(|(l, bodies, faces, components)| LookInDesign { look: LookReply::from(&l), bodies, faces, components }).collect();
    let mut out = AppearanceListResult {
        in_design,
        bodies: a.bodies.iter().map(|(b, l)| (b.clone(), LookReply::from(l))).collect(),
        faces: a
            .faces
            .iter()
            .map(|f| FaceLookReply { body: f.body.clone(), point: [f.point.x, f.point.y, f.point.z], appearance: LookReply::from(&f.look) })
            .collect(),
        components: a.components.iter().map(|(c, l)| ComponentLookReply { component: *c, appearance: LookReply::from(l) }).collect(),
        body: None,
        param_warnings: Vec::new(),
    };
    if let Some(name) = str_(p, "body") {
        let st = s.model.state();
        // A placed copy's name (`KB (K:2)`) means the body it shows, as appearance.assign takes
        // it (#9740): the reply names that body.
        let own = s.own_body_name(name);
        let b = own.as_deref().and_then(|n| st.body(n)).ok_or_else(|| bad("appearance.list", format!("no body `{name}`")))?;
        out.body = Some(BodyLooks {
            name: b.name.clone(),
            appearance: s.doc.model_body_look(b).as_ref().map(LookReply::from),
            color: s.doc.body_color(b),
            faces: s.doc.face_colors(b).iter().map(|(i, l)| FaceIndexLook { face: *i, appearance: LookReply::from(l) }).collect(),
        });
    }
    appearance_reply("appearance.list", out)
}

fn material_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let built_in = solidcraft_doc::MATERIALS.iter().map(|(n, d)| ListedMaterial::BuiltIn { name: n.to_string(), density_g_cm3: *d });
    let user = s
        .doc
        .user_materials
        .iter()
        .map(|m| {
            let mut out = super::materials::listed_material(m)?;
            out.appearance = s.doc.material_look(&m.name).as_ref().map(LookReply::from);
            Ok(ListedMaterial::Design(Box::new(out)))
        })
        .collect::<Result<Vec<_>>>()?;
    typed_reply("material.list", MaterialListResult { materials: built_in.chain(user).collect(), param_warnings: Vec::new() })
}

fn colored_components(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("enabled").is_some_and(|v| !v.is_boolean()) {
        return Err(bad("inspect.colored_components", "enabled must be a boolean"));
    }
    let enabled = bool_(p, "enabled").unwrap_or(s.colored_components.is_none());
    if enabled && s.colored_components.is_none() {
        s.colored_components = Some(());
    } else if !enabled {
        s.colored_components = None;
    }
    Ok(json!({"enabled": enabled}))
}
