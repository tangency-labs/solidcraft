//! Editable Forms sessions, surface edits and persistent CAD feature creation.
use super::CommandSpec;
#[cfg(test)]
use crate::forms_prototype::validation::validity_checks;
use crate::schema::{TypedContract, forms_prototype as params, output_schema_for};
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};
#[cfg(test)]
use solidcraft_geom::prelude::*;

mod blend;
mod pipe;
mod refined_weld;
mod weld_references;

const CREATE: TypedContract<params::Create> = TypedContract::new();
const PARAMETERS: TypedContract<params::Parameters> = TypedContract::new();
const PROJECT: TypedContract<params::Project> = TypedContract::new();
const SET: TypedContract<params::SetVertices> = TypedContract::new();
const QUALITY: TypedContract<params::Quality> = TypedContract::new();
const EMPTY: TypedContract<params::Empty> = TypedContract::new();
const COMMIT: TypedContract<params::Commit> = TypedContract::new();
const FINISH_OPTIONS: TypedContract<params::FinishOptions> = TypedContract::new();
const VALIDATE: TypedContract<params::Validate> = TypedContract::new();
const EDIT: TypedContract<params::Edit> = TypedContract::new();
const TOPOLOGY: TypedContract<params::Topology> = TypedContract::new();
const SYMMETRY: TypedContract<params::Symmetry> = TypedContract::new();
const REFINE: TypedContract<params::Refine> = TypedContract::new();
const INSERT_EDGE: TypedContract<params::InsertEdge> = TypedContract::new();
const INSERT_POINTS: TypedContract<params::InsertPoints> = TypedContract::new();
const INSERT_LOOP: TypedContract<params::InsertLoop> = TypedContract::new();
const TOOL_PREVIEW: TypedContract<params::ToolPreview> = TypedContract::new();
const EXTRUDE_DRAG: TypedContract<params::ExtrudeDrag> = TypedContract::new();
const TRANSFORM: TypedContract<params::Transform> = TypedContract::new();
const UNWELD: TypedContract<params::Unweld> = TypedContract::new();
const WELD: TypedContract<params::Weld> = TypedContract::new();
const REFINED_WELD: TypedContract<params::RefinedWeld> = TypedContract::new();
const REMOVE_REFINED_WELD: TypedContract<params::RemoveRefinedWeld> = TypedContract::new();
const MATCH: TypedContract<params::Match> = TypedContract::new();
const SURFACE: TypedContract<params::Surface> = TypedContract::new();
const PIPE: TypedContract<params::Pipe> = TypedContract::new();
const BLEND_IMPORT: TypedContract<params::BlendImport> = TypedContract::new();
const BLEND_EXPORT: TypedContract<params::BlendExport> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("form.blend.import","Import Blender Forms",crate::checked!("form.blend.import",blend::import)).icon("edit").at("SURFACE","FORMS")
        .noundo().doc_only().contract(&BLEND_IMPORT).output(output_schema_for::<params::BlendReply>)
        .params("path: .blend file; objects?: exact names (default all supported mesh objects, maximum 16). Open editable cages with units, transforms and creases; Finish saves staged Forms, Cancel discards the import. Unsupported objects are reported."),
    CommandSpec::new("form.blend.export","Export Blender Forms",crate::checked!("form.blend.export",blend::export)).icon("edit").at("SURFACE","FORMS")
        .noundo().doc_only().contract(&BLEND_EXPORT).output(output_schema_for::<params::BlendReply>)
        .params("path: .blend destination; surface?: false (editable quad cages with subdivision and creases) or true (evaluated CAD triangle meshes, no subdivision modifier); features?: saved feature ids (cages require Forms); name?: current editor object name; tolerance?, operation?, targets?: active Form fitting/Finish options. Default: current editor and staged Forms, otherwise saved Forms for cages or current bodies for surfaces. Export never finishes the live editor."),
    CommandSpec::new("form.pipe","Form Pipe",crate::checked!("form.pipe",pipe::create)).icon("edit").at("SURFACE","FORMS")
        .noundo().doc_only().contract(&PIPE).output(output_schema_for::<params::Reply>)
        .params("sketch and optional curves, or body and optional edges: source path; diameter?, section_density?:1..12, path_samples?:1..20. Overrides replace the segment list; zero diameter/samples inherit globals. regenerate?: false, required to replace a sculpted pipe cage."),
    CommandSpec::new("form.surface","Form Surface",crate::checked!("form.surface",surface)).icon("edit").at("SURFACE","FORMS")
        .noundo().doc_only().contract(&SURFACE).output(output_schema_for::<params::Reply>)
        .params("body, face: existing CAD source; segments?: [U,V] (2,2); continuity?:0|1. Edit interior guide points while retaining the source rim; Finish replaces that face in its solid. A retained-repair tool preview requires reattach:{swap,reverse_u,reverse_v} and unchanged guide counts."),
    CommandSpec::new("form.prototype.match","Match Form Boundary",crate::checked!("form.prototype.match",matching)).icon("edit")
        .noundo().doc_only().contract(&MATCH).output(output_schema_for::<params::Reply>)
        .params("body and exactly one of face or closed edges; edge: a free Form boundary edge; order?:0,1,2. Match exact CAD support with persistent references. Tolerances: position_mm?.001, tangent_deg?.1, curvature_relative?.01; tolerance?: fitted CAD mm (.01)."),
    CommandSpec::new("form.prototype.unweld","Unweld Form",crate::checked!("form.prototype.unweld",unweld)).icon("edit")
        .noundo().doc_only().contract(&UNWELD).output(output_schema_for::<params::Reply>)
        .params("edges: one separating interior seam; separate?: true; name?. Refined Unweld retains both source hierarchies and requires separate Forms. Base cages may stay in one disjoint Form. Finish publishes both pieces; Cancel restores the source."),
    CommandSpec::new("form.prototype.weld","Weld Forms",crate::checked!("form.prototype.weld",weld)).icon("edit")
        .noundo().doc_only().contract(&WELD).output(output_schema_for::<params::Reply>)
        .params("other: {kind:draft,part} or {kind:feature,feature}; first_edge,second_edge. Identify equal free openings, opposite winding, midpoint controls, maximum sharpness. The second object is consumed only on Finish; Cancel and undo restore it."),
    CommandSpec::new("form.refined_weld", "Weld Refined Forms", crate::checked!("form.refined_weld",refined_weld::author)).icon("edit")
        .doc_only().contract(&REFINED_WELD).output(output_schema_for::<crate::schema::features::FeatureResult>)
        .params("features: two saved independent Form feature IDs; edges?: one directed free logical edge per Form, omit to align the only two openings; name?. Adds one qualified seam retaining both editable sources."),
    CommandSpec::new("form.refined_weld.remove", "Remove Refined Form Weld", crate::checked!("form.refined_weld.remove",refined_weld::remove)).icon("edit")
        .doc_only().contract(&REMOVE_REFINED_WELD).output(output_schema_for::<params::RemovedRefinedWeld>)
        .params("feature: refined Weld feature ID. Removes the seam as one undoable edit and restores both source bodies when the result has no dependent features."),
    CommandSpec::new("form.prototype.transform","Place Form",crate::checked!("form.prototype.transform",transform)).icon("move")
        .noundo().doc_only().contract(&TRANSFORM).output(output_schema_for::<params::Reply>)
        .params("matrix: proper rigid column-major 4x4 relative world transform; checkpoint?: true. Changes placement while retaining local controls, hierarchy and symmetry."),
    CommandSpec::new("form.prototype.tool.begin","Preview Form Tool",crate::checked!("form.prototype.tool.begin",tool_begin)).icon("edit")
        .noundo().doc_only().contract(&EMPTY).output(output_schema_for::<params::Reply>)
        .params("Start an isolated tool preview. Apply keeps one action; Cancel restores source, undo and redo exactly."),
    CommandSpec::new("form.prototype.tool.preview","Update Form Tool Preview",crate::checked!("form.prototype.tool.preview",tool_preview)).icon("edit")
        .noundo().doc_only().contract(&TOOL_PREVIEW).output(output_schema_for::<params::Reply>)
        .params("Exactly one of topology or project: typed parameters of that Form command. Every update starts from the original tool source."),
    CommandSpec::new("form.prototype.tool.apply","Apply Form Tool",crate::checked!("form.prototype.tool.apply",tool_apply)).icon("finish")
        .noundo().doc_only().contract(&EMPTY).output(output_schema_for::<params::Reply>),
    CommandSpec::new("form.prototype.tool.cancel","Cancel Form Tool",crate::checked!("form.prototype.tool.cancel",tool_cancel)).icon("close")
        .noundo().doc_only().contract(&EMPTY).output(output_schema_for::<params::Reply>),
    CommandSpec::new("form.prototype.insert_loop","Insert Form Edge Loop",crate::checked!("form.prototype.insert_loop",insert_loop)).icon("edit")
        .noundo().doc_only().contract(&INSERT_LOOP).output(output_schema_for::<params::Reply>)
        .params("Use edge: [a,b] and fraction?: interior 0..1 (0.5), or face: active hierarchical leaf and axis: U=0/V=1 for its midpoint. tolerance?: hierarchical CAD fit tolerance in mm. Insert an edge ring or open strip. Compatible mirrored strips retain source details. Mixed-depth cuts follow exact aligned stations and existing fine seams; arbitrary mixed-depth fractions and conflicting mirror cuts remain unsupported."),
    CommandSpec::new("form.prototype.refine", "Refine Form Faces", crate::checked!("form.prototype.refine", refine)).icon("edit")
        .noundo().doc_only().contract(&REFINE).output(output_schema_for::<params::Reply>)
        .params("faces: active logical IDs; counts?: [U,V], powers of two 1..16. Shape-preserving local leaves over nested uniform evaluation and sparse hierarchical detail."),
    CommandSpec::new("form.prototype.insert_edge", "Insert Form Edge", crate::checked!("form.prototype.insert_edge", insert_edge)).icon("edit")
        .noundo().doc_only().contract(&INSERT_EDGE).output(output_schema_for::<params::Reply>)
        .params("face: active logical ID; axis?: 0 (split U) or 1 (split V). Insert a local dyadic seam without changing the limit surface."),
    CommandSpec::new("form.prototype.insert_points", "Insert Form Points", crate::checked!("form.prototype.insert_points", insert_points)).icon("edit")
        .noundo().doc_only().contract(&INSERT_POINTS).output(output_schema_for::<params::Reply>)
        .params("points: [{edge:[a,b],fraction:0..1}], ordered chain on opposite logical face sides. Fractions start at the lower vertex ID on exact fitted CAD source curves, not arc length. tolerance?: active/saved Finish tolerance (0.01 mm). One undoable topology edit."),
    CommandSpec::new("form.prototype.extrude_drag", "Extrude Form by Dragging", crate::checked!("form.prototype.extrude_drag", extrude_drag)).icon("edit")
        .noundo().doc_only().contract(&EXTRUDE_DRAG).output(output_schema_for::<params::Reply>)
        .params("faces? or boundary edges?; displacement:[x,y,z] mm; checkpoint?:true; surface?:false. Surface mode targets the original evaluated cap points plus displacement; the default moves raw cap controls. Later gesture frames use false and displacement from the original source. One undo step."),
    CommandSpec::new("form.prototype.project", "Straighten or Flatten Form", crate::checked!("form.prototype.project", project)).icon("edit")
        .noundo().doc_only().contract(&PROJECT).output(output_schema_for::<params::Reply>)
        .params("tool: straighten|flatten; vertices: distinct indices; points?: surface|control; direction?: line direction or plane normal; origin?: mm. Omitted direction uses least-squares fitting; omitted origin uses the centroid. One undoable edit, no reference association."),
    CommandSpec::new("form.prototype.symmetry", "Form Symmetry", crate::checked!("form.prototype.symmetry", symmetry)).icon("mirror")
        .noundo().doc_only().contract(&SYMMETRY).output(output_schema_for::<params::Reply>)
        .params("axis?: 0 (YZ), 1 (XZ), 2 (XY), or null to disable. Stable mirrored point partners are saved with the Form."),
    CommandSpec::new("form.prototype.parameters", "Form Parameters", crate::checked!("form.prototype.parameters", parameters)).icon("params")
        .noundo().doc_only().contract(&PARAMETERS).output(output_schema_for::<params::Reply>)
        .params("dimensions?: [x,y,z] mm; segments?: [x,y,z]; radii?: [major,minor] or [outer,inner]; path?: [[x,y,z]] mm. Dimension changes preserve sculpted controls and target exact local CAD extents; tolerance?: fitting/qualification mm (Finish tolerance by default). Topology changes currently require an unedited primitive."),
    CommandSpec::new("form.prototype.topology", "Edit Form Topology", crate::checked!("form.prototype.topology", topology)).icon("edit")
        .noundo().doc_only().contract(&TOPOLOGY).output(output_schema_for::<params::Reply>)
        .params("tool: extrude_faces|extrude_edges|delete_faces|dissolve|erase_and_fill|fill_hole|bridge|weld|unweld|thicken|subdivide|crease|uncrease; faces?, edges?, vertices?, displacement?, distance?. Crease uses distance as sharpness 0..10. Refined Fill accepts fill_mode: automatic|single|new_cap. Delete a derived cap by its sole face ID. Refined Fill, Erase and Fill and cap deletion accept tolerance? in mm. Atomic validated topology edit."),
    CommandSpec::new("form.prototype.commit","Finish Form",crate::checked!("form.prototype.commit",commit)).icon("finish").contract(&COMMIT).output(output_schema_for::<Value>)
        .params("tolerance?: mm; operation?: new|join|cut|intersect; targets?: body names; name?. Uses staged Finish options when omitted. Fit and sew CAD patches and save the editable Form feature."),
    CommandSpec::new("form.prototype.validate", "Check Form CAD", crate::checked!("form.prototype.validate", validate)).icon("inspect")
        .noundo().contract(&VALIDATE).output(output_schema_for::<params::ValidationReply>)
        .params("tolerance?: active/saved Finish tolerance, 0.01 mm by default. Read-only checks of the active fitted Form before Finish booleans; staged Forms are counted but not checked. Reports sampled-fit metrics, conversion stage, shape kind, and separate record/SMLib passed, failed, not-supported or not-run coverage. Does not change Finish acceptance policy."),
    CommandSpec::new("form.prototype.finish_options","Form Finish Options",crate::checked!("form.prototype.finish_options",finish_options)).icon("edit")
        .noundo().doc_only().contract(&FINISH_OPTIONS).output(output_schema_for::<params::FinishOptions>)
        .params("tolerance?: CAD fitting expression in mm; operation?: new|join|cut|intersect. Stages output settings in the active Form without committing it."),
    CommandSpec::new("form.repair","Repair Form Support",crate::checked!("form.repair",repair)).icon("edit").noundo().doc_only().contract(&EDIT).output(output_schema_for::<params::RepairReply>)
        .params("feature: saved associated Form id. Explicitly open structurally valid retained source after support resolution fails; ordinary edit stays strict. Reattach through a qualified tool preview and Apply, or Cancel; unresolved source cannot Finish."),
    CommandSpec::new("form.edit","Edit Form",crate::checked!("form.edit",edit)).icon("edit").noundo().doc_only().contract(&EDIT).output(output_schema_for::<params::Reply>)
        .params("feature: Form feature id. Enter an editable session without changing its saved CAD result until Finish Form."),
    CommandSpec::new("form.prototype.set_surface_vertices", "Pull Form Surface", crate::checked!("form.prototype.set_surface_vertices", set_surface)).icon("move")
        .noundo()
        .doc_only()
        .contract(&SET)
        .output(output_schema_for::<params::Reply>)
        .params(
            "vertices: logical indices; positions: evaluated surface positions in mm; checkpoint?: true. Solves the inverse subdivision response.",
        ),
    CommandSpec::new("form.prototype.create", "Create Form", crate::checked!("form.prototype.create", create))
        .noundo()
        .at("SURFACE","FORMS").icon("surface_patch")
        .doc_only()
        .contract(&CREATE)
        .output(output_schema_for::<params::Reply>)
        .params(
            "primitive?: box|cylinder|quadball|sphere|pipe|face|torus|tube; dimensions?: [x,y,z] mm; segments?: [x,y,z]; radii?: [major,minor]; path?: [[x,y,z]]; size?: 40 mm; spans?: 2; levels?: 0..4 (3). Begin an editable Form session. Finish Form saves fitted CAD and editable source.",
        ),
    CommandSpec::new("form.prototype.set_vertices", "Pull Form Vertices", crate::checked!("form.prototype.set_vertices", set)).icon("move")
        .noundo()
        .doc_only()
        .contract(&SET)
        .output(output_schema_for::<params::Reply>)
        .params("vertices: [indices]; positions: [[x,y,z]] in mm; checkpoint?: true. First frame of a drag checkpoints, later frames use false."),
    CommandSpec::new("form.prototype.quality", "Form Preview Quality", crate::checked!("form.prototype.quality", quality)).icon("compute")
        .noundo()
        .doc_only()
        .contract(&QUALITY)
        .output(output_schema_for::<params::Reply>)
        .params("levels: 0..4; changes preview refinement without changing the cage."),
    CommandSpec::new("form.prototype.undo", "Undo Form Edit", crate::checked!("form.prototype.undo", undo)).icon("undo")
        .noundo()
        .doc_only()
        .contract(&EMPTY)
        .output(output_schema_for::<params::Reply>),
    CommandSpec::new("form.prototype.redo", "Redo Form Edit", crate::checked!("form.prototype.redo", redo)).icon("redo")
        .noundo()
        .doc_only()
        .contract(&EMPTY)
        .output(output_schema_for::<params::Reply>),
    CommandSpec::new("form.prototype.inspect", "Inspect Form", crate::checked!("form.prototype.inspect", inspect)).icon("inspect")
        .noundo()
        .doc_only()
        .contract(&EMPTY)
        .output(output_schema_for::<params::Reply>),
    CommandSpec::new("form.prototype.close", "Cancel Form", crate::checked!("form.prototype.close", close)).icon("close")
        .noundo()
        .doc_only()
        .contract(&EMPTY)
        .output(output_schema_for::<params::Reply>),
];
fn parse<T: serde::de::DeserializeOwned>(p: &Value) -> Result<T> {
    serde_json::from_value(p.clone()).map_err(|e| EngineError::Other(e.to_string()))
}
fn form(s: &mut Session) -> Result<&mut crate::forms_prototype::Form> {
    let f = raw_form(s)?;
    f.require_editable()?;
    Ok(f)
}
fn raw_form(s: &mut Session) -> Result<&mut crate::forms_prototype::Form> {
    s.forms_prototype.as_mut().ok_or_else(|| EngineError::Other("create a Form first".into()))
}
fn tool_begin(s: &mut Session, _: &Value) -> Result<Value> {
    let f = raw_form(s)?;
    if f.tool_original.is_some() {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    f.tool_original = Some(std::sync::Arc::new(f.clone()));
    Ok(f.inspect())
}
fn tool_preview(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::ToolPreview = parse(p)?;
    let f = raw_form(s)?;
    let source = f.tool_original.clone().ok_or_else(|| EngineError::Other("start a Form tool preview first".into()))?;
    let revision = f.revision;
    if source.repair.is_some() && !((source.matching.is_some() && args.matching.is_some()) || (source.profile.is_some() && args.surface.is_some())) {
        return Err(EngineError::Other("retained Form repair requires its Match or Surface reattachment preview".into()));
    }
    let repairing_match = source.repair.is_some() && source.matching.is_some();
    let repairing_surface = source.repair.is_some() && source.profile.is_some();
    let mut scratch = s.scratch();
    scratch.forms_prototype = Some(source.as_ref().clone());
    type Preview = (&'static str, bool, fn(&mut Session, &Value) -> Result<Value>);
    let operations: [Preview; 7] = [
        ("pipe", args.pipe.is_some(), pipe::create),
        ("surface", args.surface.is_some(), if repairing_surface { surface_repair } else { surface }),
        ("matching", args.matching.is_some(), if repairing_match { matching_repair } else { matching }),
        ("topology", args.topology.is_some(), topology),
        ("project", args.project.is_some(), project),
        ("unweld", args.unweld.is_some(), unweld),
        ("weld", args.weld.is_some(), weld),
    ];
    let mut chosen = operations.into_iter().filter(|(_, present, _)| *present);
    let Some((key, _, handler)) = chosen.next() else {
        return Err(EngineError::Other("preview exactly one Form operation".into()));
    };
    if chosen.next().is_some() {
        return Err(EngineError::Other("preview exactly one Form operation".into()));
    }
    handler(&mut scratch, p.get(key).ok_or_else(|| EngineError::Other("missing Form tool parameters".into()))?)?;
    if source.repair.is_some() {
        qualify_repair(&mut scratch)?;
        let context = crate::forms_prototype::RepairContext { document: scratch.doc.clone(), moves: scratch.pending_moves.clone() };
        raw_form(&mut scratch)?.repair_context = Some(context);
    }
    let mut next = scratch.forms_prototype.take().ok_or_else(|| EngineError::Other("tool preview did not produce a Form".into()))?;
    next.tool_original = Some(source);
    next.revision = revision.wrapping_add(1);
    let reply = next.inspect();
    s.forms_prototype = Some(next);
    Ok(reply)
}
fn qualify_repair(s: &mut Session) -> Result<()> {
    let form = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no retained Form is open".into()))?;
    let definition = form.definition();
    let tolerance = active_tolerance(s, None)?;
    let state = match_state(s)?;
    if let Some(profile) = &definition.profile {
        let (resolved, cage) = profile.recompute(&definition, &state)?;
        if resolved != **profile || cage != definition.cage {
            return Err(EngineError::Other("the Surface support changed; refresh its reattachment preview before Apply".into()));
        }
        profile.fit(&state, &cage, &definition.placement, tolerance)?;
    } else if let Some(matching) = &definition.matching {
        let (resolved, report) = matching.recompute(&definition, &state, tolerance)?;
        crate::doc::forms::convert(&resolved, tolerance)?;
        raw_form(s)?.refresh_match_preview(resolved, report)?;
    } else {
        return Err(EngineError::Other("retained support repair lost its association".into()));
    }
    Ok(())
}
fn tool_apply(s: &mut Session, _: &Value) -> Result<Value> {
    if let Some(form) = &s.forms_prototype
        && form.repair.is_some()
        && !form.repair_context.as_ref().is_some_and(|context| std::sync::Arc::ptr_eq(&context.document, &s.doc) && context.moves == s.pending_moves)
    {
        return Err(EngineError::Other("refresh and qualify the replacement support preview before Apply".into()));
    }
    let f = raw_form(s)?;
    if f.tool_original.take().is_none() {
        return Err(EngineError::Other("no Form tool preview is open".into()));
    }
    f.repair = None;
    f.repair_context = None;
    Ok(f.inspect())
}
fn tool_cancel(s: &mut Session, _: &Value) -> Result<Value> {
    let f = raw_form(s)?;
    let source = f.tool_original.take().ok_or_else(|| EngineError::Other("no Form tool preview is open".into()))?;
    let revision = f.revision.wrapping_add(1);
    *f = source.as_ref().clone();
    f.revision = revision;
    Ok(f.inspect())
}
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    if s.forms_prototype.as_ref().is_some_and(|f| f.repair.is_some()) {
        return Err(EngineError::Other("reattach the retained Form support and Apply, or Cancel".into()));
    }
    if s.forms_prototype.as_ref().is_some_and(|f| f.tool_original.is_some()) {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    if s.forms_prototype.as_ref().and_then(|form| form.editing_feature).is_some_and(|id| refined_weld::attached(&s.doc, id)) {
        return Err(EngineError::Other("remove the refined Weld before replacing its Form source".into()));
    }
    let p: params::Create = parse(p)?;
    if s.active_sketch.is_some() {
        return Err(EngineError::Other("finish the active sketch before starting a Form".into()));
    }
    let parameters = crate::geom::subdivision::primitives::Parameters {
        kind: p.primitive,
        dimensions: p.dimensions.unwrap_or([p.size; 3]),
        segments: p.segments.unwrap_or([p.spans; 3]),
        radii: p.radii.unwrap_or([p.size / 2.0, p.size / 5.0]),
        path: p.path.unwrap_or_default().into_iter().map(|p| solidcore_record::Vec3::new(p[0], p[1], p[2])).collect(),
    };
    let mut f = crate::forms_prototype::Form::create_parameters(parameters, p.levels)?;
    f.authoring_component = s.active_component;
    f.authoring_path = s.active_occurrence_path.clone();
    if let Some(current) = &mut s.forms_prototype {
        current.replace_primitive(f);
    } else {
        s.forms_prototype = Some(f);
    }
    Ok(s.forms_prototype.as_ref().map_or_else(|| json!({"active":false}), |f| f.inspect()))
}
fn match_state(s: &Session) -> Result<crate::doc::ModelState> {
    let before = s.forms_prototype.as_ref().and_then(|f| f.editing_feature).map_or_else(|| s.model.state(), |id| s.model.state_before(id));
    let mut document = std::borrow::Cow::Borrowed(s.doc.as_ref());
    if !s.pending_moves.is_empty() {
        for occurrence in &mut document.to_mut().occurrences {
            if let Some(matrix) = s.pending_moves.get(&occurrence.id) {
                occurrence.transform = *matrix;
            }
        }
    }
    Ok(crate::doc::world_state(&document, &before)?)
}
fn surface(s: &mut Session, p: &Value) -> Result<Value> {
    surface_with_mode(s, p, false)
}
fn surface_repair(s: &mut Session, p: &Value) -> Result<Value> {
    surface_with_mode(s, p, true)
}
fn surface_with_mode(s: &mut Session, p: &Value, retained: bool) -> Result<Value> {
    let args: params::Surface = parse(p)?;
    if s.active_sketch.is_some() {
        return Err(EngineError::Other("finish the sketch before creating a Form Surface".into()));
    }
    if s.forms_prototype.as_ref().is_some_and(|f| f.tool_original.is_some()) {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    let state = match_state(s)?;
    let body = state.body(&args.body).ok_or_else(|| EngineError::Other("the Form Surface source body is missing".into()))?;
    let repairing = s.forms_prototype.as_ref().is_some_and(|form| form.repair.is_some());
    if repairing || args.reattach.is_some() {
        if !repairing || !retained {
            return Err(EngineError::Other("start an explicit retained Surface reattachment preview first".into()));
        }
        let mapping = args.reattach.ok_or_else(|| EngineError::Other("choose the Surface U/V swap and reverse mapping explicitly".into()))?;
        let current = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no retained Surface source".into()))?;
        let old = current.profile.as_ref().ok_or_else(|| EngineError::Other("this retained Form requires Match reattachment".into()))?;
        if args.segments != old.segments {
            return Err(EngineError::Other("Surface repair keeps the existing guide counts".into()));
        }
        let mut definition = current.definition();
        let source = crate::doc::forms::SupportRef::face(body, args.face)?;
        let (mut profile, cage) = old.reattach(&definition, &state, source, [mapping.swap, mapping.reverse_u, mapping.reverse_v])?;
        profile.continuity = args.continuity;
        profile.partition = args.partition.unwrap_or(old.partition);
        definition.profile = Some(Box::new(profile));
        definition.cage = cage;
        raw_form(s)?.adopt_profile(definition)?;
        return Ok(raw_form(s)?.inspect());
    }
    let (mut profile, cage) = crate::doc::forms::Profile::seed(body, args.face, args.segments, args.continuity)?;
    profile.partition = args.partition.unwrap_or_else(|| s.forms_prototype.as_ref().and_then(|f| f.profile.as_ref()).is_none_or(|p| p.partition));
    let mut definition = crate::doc::forms::Definition {
        imported: None,
        pipe: None,
        profile: Some(Box::new(profile)),
        matching: None,
        placement: Default::default(),
        cage,
        parameters: crate::geom::subdivision::primitives::Parameters {
            kind: "face".into(),
            segments: [args.segments[0], args.segments[1], 1],
            ..Default::default()
        },
        hierarchy: None,
        symmetry: None,
    };
    if let Some(current) = &s.forms_prototype
        && let (Some(old), Some(next)) = (&current.profile, &definition.profile)
        && matches!((&old.source,&next.source),(crate::doc::forms::SupportRef::Face {body:a,name:an,..},crate::doc::forms::SupportRef::Face{body:b,name:bn,..}) if a==b && an==bn)
        && old.segments == next.segments
    {
        let continuity = next.continuity;
        let partition = next.partition;
        definition = current.definition();
        if let Some(profile) = &mut definition.profile {
            profile.continuity = continuity;
            profile.partition = partition;
        }
    }
    if let Some(current) = &mut s.forms_prototype {
        current.adopt_profile(definition)?;
    } else {
        let mut form = crate::forms_prototype::Form::from_definition(definition, 2, None)?;
        form.authoring_component = s.active_component;
        form.authoring_path = s.active_occurrence_path.clone();
        s.forms_prototype = Some(form);
    }
    Ok(s.forms_prototype.as_ref().map_or_else(|| json!({"active":false}), |f| f.inspect()))
}
fn matching(s: &mut Session, p: &Value) -> Result<Value> {
    matching_with_mode(s, p, false)
}
fn matching_repair(s: &mut Session, p: &Value) -> Result<Value> {
    matching_with_mode(s, p, true)
}
fn matching_with_mode(s: &mut Session, p: &Value, retained: bool) -> Result<Value> {
    let args: params::Match = parse(p)?;
    let definition = if retained {
        let form = raw_form(s)?;
        if form.repair.is_none() || form.matching.is_none() || form.tool_original.is_some() {
            return Err(EngineError::Other("start a retained Match reattachment preview first".into()));
        }
        form.definition()
    } else {
        form(s)?.definition()
    };
    if definition.hierarchy.as_ref().is_some_and(|h| h.has_quad_cap()) {
        return Err(EngineError::Other("Match on a Form with a derived cap is not supported yet".into()));
    }
    if definition.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse()) {
        return Err(EngineError::Other("Match requires an untrimmed Form before Collapse".into()));
    }
    let state = match_state(s)?;
    let body = state.body(&args.body).ok_or_else(|| EngineError::Other("Match support body is missing or follows the edited Form".into()))?;
    let source = match (args.face, args.edges) {
        (Some(face), None) if !args.wire_from_face => crate::doc::forms::SupportRef::face(body, face)?,
        (Some(face), None) => {
            crate::kernel::face_geometry(&body.body, face)?;
            let edges = crate::kernel::face_boundaries(&body.body, face)?
                .into_iter()
                .flatten()
                .map(|boundary| boundary.edge)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            crate::doc::forms::SupportRef::wire(body, &edges)?
        }
        (None, Some(edges)) => crate::doc::forms::SupportRef::wire(body, &edges)?,
        _ => return Err(EngineError::Other("select exactly one support face or closed edge wire".into())),
    };
    let mut tolerances = crate::geom::subdivision::matching::Tolerances::default();
    if let Some(v) = args.position_mm {
        tolerances.position_mm = v;
    }
    if let Some(v) = args.tangent_deg {
        tolerances.tangent_deg = v;
    }
    if let Some(v) = args.curvature_relative {
        tolerances.curvature_relative = v;
    }
    let request = crate::doc::forms::MatchRequest { adjacent_faces: args.adjacent_faces, source, edge: args.edge, order: args.order, tolerances };
    let (definition, report) = crate::doc::forms::Matching::attach(&definition, &state, request, args.tolerance.unwrap_or(0.01))?;
    let form = raw_form(s)?;
    form.apply_matching(definition, report)?;
    Ok(form.inspect())
}
fn active_tolerance(s: &Session, requested: Option<f64>) -> Result<f64> {
    let current = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no Form is being edited".into()))?;
    let saved_tolerance = current.editing_feature.and_then(|id| s.doc.feature(id)).and_then(|feature| match &feature.kind {
        crate::doc::FeatureKind::Form { tolerance, .. } => Some(tolerance.as_str()),
        _ => None,
    });
    requested.map_or_else(
        || {
            current
                .finish_options
                .tolerance
                .as_deref()
                .or(saved_tolerance)
                .map(|text| crate::forms_prototype::finish_tolerance(s, text))
                .unwrap_or(Ok(0.01))
        },
        |value| {
            if value.is_finite() && (1e-6..=1.0).contains(&value) {
                Ok(value)
            } else {
                Err(EngineError::Other("Form CAD tolerance must be between 0.000001 and 1 mm".into()))
            }
        },
    )
}
fn parameters(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Parameters = parse(p)?;
    let tolerance = active_tolerance(s, args.tolerance)?;
    let current = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no Form is being edited".into()))?;
    let source_state = current.matching.as_ref().map(|_| match_state(s)).transpose()?;
    let mut parameters = current.parameters.clone();
    if let Some(value) = args.dimensions {
        parameters.dimensions = value;
    }
    if let Some(value) = args.segments {
        parameters.segments = value;
    }
    if let Some(value) = args.radii {
        parameters.radii = value;
    }
    if let Some(value) = args.path {
        parameters.path = value.into_iter().map(|p| solidcore_record::Vec3::new(p[0], p[1], p[2])).collect();
    }
    let current = form(s)?;
    current.set_parameters(parameters, tolerance, |definition| {
        let (resolved, match_report) = if let (Some(matching), Some(state)) = (&definition.matching, &source_state) {
            let (resolved, report) = matching.recompute(definition, state, tolerance)?;
            (resolved, Some(report))
        } else {
            (definition.clone(), None)
        };
        let mut local = resolved.clone();
        // Measuring the world-space AABB and rotating it back would lose the
        // local-axis extents. Fit the actual source in its local authoring frame.
        local.placement = Default::default();
        let (body, _) = crate::doc::forms::convert(&local, tolerance)?;
        Ok(crate::forms_prototype::DimensionShape { definition: resolved, bounds: body.exact_bounds()?, match_report })
    })?;
    Ok(current.inspect())
}
fn refine(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Refine = parse(p)?;
    refine_form(s, |f| f.refine(&args.faces, args.counts))
}
fn refine_form(s: &mut Session, edit: impl FnOnce(&mut crate::forms_prototype::Form) -> Result<()>) -> Result<Value> {
    if form(s)?.matching.is_some() {
        let state = match_state(s)?;
        form(s)?.refine_with_match(&state, edit)?;
    } else {
        let tolerance = crate::forms_prototype::collapse_tolerance(s)?;
        edit_with_collapse_fit(form(s)?, tolerance, edit)?;
    }
    Ok(form(s)?.inspect())
}
fn insert_edge(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::InsertEdge = parse(p)?;
    if args.axis > 1 {
        return Err(EngineError::Other("insert axis must be U or V".into()));
    }
    refine_form(s, |f| f.refine(&[args.face], if args.axis == 0 { [2, 1] } else { [1, 2] }))
}
fn insert_points(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::InsertPoints = parse(p)?;
    let points: Vec<_> = args.points.into_iter().map(|p| (p.edge, p.fraction)).collect();
    let tolerance = active_tolerance(s, args.tolerance)?;
    let state = if s.forms_prototype.as_ref().is_some_and(|f| f.matching.is_some()) { Some(match_state(s)?) } else { None };
    let current = form(s)?;
    edit_with_collapse_fit(current, Some(tolerance), |f| f.insert_points(&points, state.as_ref(), tolerance))?;
    Ok(current.inspect())
}
fn insert_loop(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::InsertLoop = parse(p)?;
    if s.forms_prototype.as_ref().is_some_and(|f| f.hierarchy.is_some()) {
        let requested = match &args {
            params::InsertLoop::Edge(args) => args.tolerance,
            params::InsertLoop::Face(args) => args.tolerance,
        };
        let tolerance = active_tolerance(s, requested)?;
        let state = if s.forms_prototype.as_ref().is_some_and(|f| f.matching.is_some()) { Some(match_state(s)?) } else { None };
        let f = form(s)?;
        edit_with_collapse_fit(f, Some(tolerance), |f| match args {
            params::InsertLoop::Edge(args) => f.insert_hierarchical_loop(args.edge, args.fraction, state.as_ref(), tolerance),
            params::InsertLoop::Face(args) => f.insert_hierarchical_loop_from_leaf(args.face, args.axis, state.as_ref(), tolerance),
        })?;
        return Ok(f.inspect());
    }
    let params::InsertLoop::Edge(args) = args else { return Err(EngineError::Other("a face/axis loop requires a Form hierarchy".into())) };
    if args.tolerance.is_some() {
        active_tolerance(s, args.tolerance)?;
    }
    let f = form(s)?;
    let seams = f.insert_loop(args.edge, args.fraction)?;
    let mut reply = f.inspect();
    reply["loop_seams"] = json!(seams.into_iter().map(|[a, b]| [a.min(b), a.max(b)]).collect::<Vec<_>>());
    Ok(reply)
}
fn extrude_drag(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::ExtrudeDrag = parse(p)?;
    if args.surface {
        let state = if form(s)?.matching.is_some() { Some(match_state(s)?) } else { None };
        let f = form(s)?;
        f.extrude_surface_drag(
            state.as_ref(),
            &args.faces,
            &args.edges,
            solidcore_record::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]),
            args.checkpoint,
        )?;
        return Ok(f.inspect());
    }
    if form(s)?.matching.is_some() {
        let state = match_state(s)?;
        let f = form(s)?;
        f.extrude_drag_with_match(
            &state,
            &args.faces,
            &args.edges,
            solidcore_record::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]),
            args.checkpoint,
        )?;
        return Ok(f.inspect());
    }
    let f = form(s)?;
    f.extrude_drag(
        &args.faces,
        &args.edges,
        solidcore_record::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]),
        args.checkpoint,
    )?;
    Ok(f.inspect())
}
fn project(s: &mut Session, p: &Value) -> Result<Value> {
    use crate::geom::subdivision::projection::{Projection, project};
    let args: params::Project = parse(p)?;
    let tolerance = crate::forms_prototype::collapse_tolerance(s)?;
    let form = form(s)?;
    let controls;
    let source = match args.points {
        params::ProjectionPoints::Control => {
            controls = form.control_positions();
            &controls
        }
        params::ProjectionPoints::Surface => &form.surface.vertices,
    };
    if args.vertices.len() > source.len() {
        return Err(EngineError::Other("too many selected Form vertices".into()));
    }
    let unique: std::collections::BTreeSet<_> = args.vertices.iter().copied().collect();
    if unique.len() != args.vertices.len() {
        return Err(EngineError::Other("select distinct Form vertices".into()));
    }
    let points: Vec<_> = args
        .vertices
        .iter()
        .map(|&i| source.get(i).copied().ok_or_else(|| EngineError::Other("invalid Form vertex".into())))
        .collect::<Result<_>>()?;
    let tool = match args.tool {
        params::ProjectionTool::Flatten => Projection::Plane,
        params::ProjectionTool::Straighten => Projection::Line,
    };
    let vector = |p: [f64; 3]| solidcore_record::Vec3::new(p[0], p[1], p[2]);
    let positions: Vec<_> = project(&points, tool, args.direction.map(vector), args.origin.map(vector))
        .map_err(|e| EngineError::Other(e.to_string()))?
        .into_iter()
        .map(|p| [p.x, p.y, p.z])
        .collect();
    edit_with_collapse_fit(form, tolerance, |form| match args.points {
        params::ProjectionPoints::Control => form.set(&args.vertices, &positions, true),
        params::ProjectionPoints::Surface => form.set_surface(&args.vertices, &positions, true),
    })?;
    Ok(form.inspect())
}
fn validate(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Validate = parse(p)?;
    Ok(json!(crate::forms_prototype::validation::Input::capture(s)?.check(&args)?))
}

fn finish_options(s: &mut Session, p: &Value) -> Result<Value> {
    let update = crate::forms_prototype::checked_finish_options(parse(p)?)?;
    if let Some(tolerance) = &update.tolerance {
        crate::forms_prototype::finish_tolerance(s, tolerance)?;
    }
    let active = form(s)?;
    active.update_finish_options(update);
    Ok(json!(active.finish_options))
}
// Named support identity is authoritative. Its original point can legitimately
// be stale after a source edit or a component move; recapturing that hint on every
// Finish would replace a healthy persistent reference with a nearest-face guess.
fn same_support(a: &crate::doc::forms::SupportRef, b: &crate::doc::forms::SupportRef) -> bool {
    use crate::doc::forms::SupportRef;
    match (a, b) {
        (SupportRef::Face { body: a, name: an, .. }, SupportRef::Face { body: b, name: bn, .. }) => a == b && an == bn,
        (SupportRef::Wire { body: a, edges: ae }, SupportRef::Wire { body: b, edges: be }) => {
            a == b && ae.len() == be.len() && ae.iter().zip(be).all(|(a, b)| a.name == b.name)
        }
        _ => false,
    }
}
fn form_support(definition: &crate::doc::forms::Definition) -> Option<&crate::doc::forms::SupportRef> {
    definition.profile.as_ref().map(|p| &p.source).or_else(|| definition.matching.as_ref().map(|m| &m.source))
}
fn form_attachment_changed(old: &crate::doc::FeatureKind, new: &crate::doc::FeatureKind) -> bool {
    use crate::doc::FeatureKind;
    let (FeatureKind::Form { definition: old, .. }, FeatureKind::Form { definition: new, .. }) = (old, new) else { return false };
    let bindings_changed = |a: Option<&crate::doc::forms::support::BoundaryBinding>, b: Option<&crate::doc::forms::support::BoundaryBinding>| {
        // Capturing a certificate when opening a legacy source keeps its old named
        // support. It is not explicit reattachment and needs no hint recapture.
        matches!((a, b), (Some(a), Some(b)) if !a.same_attachment(b))
    };
    match (&old.profile, &new.profile, &old.matching, &new.matching) {
        (Some(a), Some(b), _, _) => {
            !same_support(&a.source, &b.source) || bindings_changed(a.chart.as_ref().map(|c| &c.binding), b.chart.as_ref().map(|c| &c.binding))
        }
        (None, None, Some(a), Some(b)) => !same_support(&a.source, &b.source) || bindings_changed(a.binding.as_ref(), b.binding.as_ref()),
        (None, None, None, None) => false,
        _ => true,
    }
}
fn reattached_face_names(s: &Session, feature: &crate::doc::Feature, kind: &crate::doc::FeatureKind) -> Result<Vec<String>> {
    let state = s.model.state_before(feature.id);
    let mut state = crate::doc::face_point_state(&s.doc, &state, kind, feature.component)?.into_owned();
    if let crate::doc::FeatureKind::Form { definition, .. } = kind
        && let Some(source) = form_support(definition)
    {
        // Coincident independent bodies can have the same pick. The user chose
        // this exact support, so the standard capture helper must see that body.
        state.bodies.retain(|body| body.name == source.body().as_ref());
        if state.bodies.is_empty() {
            return Err(EngineError::Other("the reattached Form support body is missing".into()));
        }
    }
    Ok(crate::doc::face_point_names(&s.doc, &state, kind, feature.component)?)
}
fn commit(s: &mut Session, p: &Value) -> Result<Value> {
    let f = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no Form is being edited".into()))?;
    let component = f.authoring_component;
    let path = f.authoring_path.clone();
    let before = (s.active_component, std::mem::replace(&mut s.active_occurrence_path, path));
    s.active_component = component;
    let result = commit_in_author(s, p);
    s.active_component = before.0;
    s.active_occurrence_path = before.1;
    result
}
fn commit_in_author(s: &mut Session, p: &Value) -> Result<Value> {
    if s.forms_prototype.as_ref().is_some_and(|f| f.tool_original.is_some()) {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    let args: params::Commit = parse(p)?;
    let form = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no Form is being edited".into()))?;
    if form.repair.is_some() {
        return Err(EngineError::Other("reattach the retained Form support and Apply before Finish".into()));
    }
    let tolerance = if args.tolerance.is_some() {
        args.tolerance
    } else {
        form.finish_options.tolerance.as_deref().map(|text| crate::forms_prototype::finish_tolerance(s, text)).transpose()?
    };
    let mut definition = form.definition();
    pipe::recompute(s, &mut definition, form.authoring_component, form.editing_feature)?;
    if let Some(matching) = &definition.matching {
        definition = matching.recompute(&definition, &match_state(s)?, tolerance.unwrap_or(0.01))?.0;
    }
    if let Some(profile) = &definition.profile {
        let (profile, cage) = profile.recompute(&definition, &match_state(s)?)?;
        definition.profile = Some(Box::new(profile));
        definition.cage = cage;
    }
    if let Some(feature) = form.editing_feature.and_then(|id| s.doc.feature(id)) {
        let frame = if feature.component == s.active_component { s.active_component_frame() } else { s.doc.component_transform(feature.component) };
        let inverse = crate::doc::mat_inverse(&frame).ok_or_else(|| EngineError::Other("the Form component frame is not invertible".into()))?;
        definition.placement = crate::doc::forms::Placement::from_matrix(&crate::doc::mat_mul(&inverse, &definition.placement.matrix()))?;
        if let Some(matching) = &mut definition.matching {
            matching.transform_picks(&inverse);
        }
        if let Some(profile) = &mut definition.profile {
            profile.transform_picks(&inverse);
        }
        if let Some(pipe) = &mut definition.pipe {
            pipe.source.transform_picks(&inverse);
        }
    }
    let previous = form.editing_feature.and_then(|id| s.doc.feature(id)).and_then(|f| match &f.kind {
        crate::doc::FeatureKind::Form { tolerance, operation, targets, .. } => Some((tolerance.clone(), *operation, targets.clone())),
        _ => None,
    });
    let mut operation = args
        .operation
        .as_deref()
        .or(form.finish_options.operation.as_deref())
        .map(crate::doc::Operation::parse)
        .unwrap_or(Some(previous.as_ref().map_or(crate::doc::Operation::NewBody, |p| p.1)))
        .ok_or_else(|| EngineError::Other("unknown Form operation".into()))?;
    if definition.profile.is_some() {
        if args.operation.is_some() && operation != crate::doc::Operation::NewBody {
            return Err(EngineError::Other("Form Surface replaces its source face; choose New Body instead of a boolean operation".into()));
        }
        operation = crate::doc::Operation::NewBody;
    }
    if (!form.parts.is_empty() || !form.consumed.is_empty()) && operation != crate::doc::Operation::NewBody {
        return Err(EngineError::Other("object-level Unweld and Weld require standalone New Body Forms".into()));
    }
    let editing_feature = form.editing_feature;
    let welded_source = editing_feature.filter(|&id| refined_weld::attached(&s.doc, id));
    if let Some(id) = welded_source {
        refined_weld::check_source_edit(&s.doc, id, &definition, operation)?;
        if !form.parts.is_empty() || !form.consumed.is_empty() {
            return Err(EngineError::Other("remove the refined Weld before staging other objects with its Form source".into()));
        }
    }
    let parts = form.parts.clone();
    let imported_name = form.imported.as_ref().map(|i| i.object.clone());
    let consumed = form.consumed.clone();
    for (&id, saved) in &consumed {
        if s.doc.feature(id) != Some(saved.as_ref()) {
            return Err(EngineError::Other("a Form selected for Weld changed; undo the weld and select it again".into()));
        }
    }
    let references = weld_references::prepare(s, &consumed)?;
    let refined_split = !parts.is_empty() && definition.hierarchy.as_ref().is_some_and(|h| h.has_detached_material());
    let refined_collapse = editing_feature.is_some() && definition.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse());
    let fit_tolerance = args
        .tolerance
        .map(|v| format!("{v} mm").into())
        .or_else(|| form.finish_options.tolerance.as_ref().map(|text| text.clone().into()))
        .unwrap_or_else(|| previous.as_ref().map_or_else(|| "0.01 mm".into(), |p| p.0.clone()));
    let kind = crate::doc::FeatureKind::Form {
        definition: Box::new(definition),
        tolerance: fit_tolerance.clone(),
        operation,
        targets: if p.get("targets").is_none() {
            previous.map(|p| p.2).unwrap_or_default()
        } else {
            args.targets.into_iter().map(Into::into).collect()
        },
    };
    let recaptured = editing_feature
        .and_then(|id| s.doc.feature(id))
        .filter(|feature| form_attachment_changed(&feature.kind, &kind))
        .map(|feature| reattached_face_names(s, feature, &kind))
        .transpose()?;
    let before_replay =
        (recaptured.is_some() || welded_source.is_some() || refined_split || refined_collapse).then(|| (s.doc.clone(), s.model.clone()));
    for &id in consumed.keys() {
        s.doc_mut().delete_feature(id)?;
    }
    let mut result = if let Some(id) = editing_feature {
        let feature =
            s.doc_mut().features.iter_mut().find(|f| f.id == id).ok_or_else(|| EngineError::Other("the edited Form feature was removed".into()))?;
        feature.kind = kind;
        if let Some(names) = recaptured {
            feature.face_names = names;
        }
        s.refresh();
        if let Some(error) = s.model.result(id).and_then(|r| r.error.as_ref()) {
            return Err(EngineError::Other(error.clone()));
        }
        json!({"feature":id})
    } else {
        let mut named = p.clone();
        if named.get("name").is_none()
            && let Some(imported_name) = imported_name
        {
            named["name"] = json!(imported_name);
        }
        super::features::add_feature(s, &named, kind)?
    };
    let mut made = Vec::new();
    for part in parts.values() {
        let kind = crate::doc::FeatureKind::Form {
            definition: Box::new(part.definition.as_ref().clone()),
            tolerance: if part.definition.hierarchy.as_ref().is_some_and(|h| h.has_detached_material()) {
                fit_tolerance.clone()
            } else {
                args.tolerance.map_or_else(|| "0.01 mm".into(), |v| format!("{v} mm").into())
            },
            operation: crate::doc::Operation::NewBody,
            targets: Vec::new(),
        };
        let child = super::features::add_feature(s, &json!({"name":part.name}), kind)?;
        made.push(child.get("feature").cloned().ok_or_else(|| EngineError::Other("new Form did not return its identity".into()))?);
    }
    if !made.is_empty() {
        result["parts"] = Value::Array(made);
    }
    let joined = result.get("feature").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("new Form did not return its identity".into()))?;
    references.apply(s, joined)?;
    if let Some((document, model)) = before_replay {
        let broken = super::edit::newly_broken_in_history(&document, &model, &s.doc, &s.model);
        if !broken.is_empty() {
            let operation = if refined_split {
                "finishing refined Unweld"
            } else if refined_collapse {
                "finishing refined Collapse"
            } else if welded_source.is_some() {
                "finishing the welded Form"
            } else {
                "reattaching the Form"
            };
            return Err(EngineError::Other(format!(
                "{operation} would break: {}",
                broken.into_iter().map(|(name, error)| format!("{name}: {error}")).collect::<Vec<_>>().join("; ")
            )));
        }
    }
    s.forms_prototype = None;
    Ok(result)
}
fn topology(s: &mut Session, p: &Value) -> Result<Value> {
    let mut args: params::Topology = parse(p)?;
    let new_cap = matches!(&args.fill_mode, params::FillMode::NewCap);
    if new_cap && args.tool != "fill_hole" {
        return Err(EngineError::Other("New cap is a Fill Hole mode".into()));
    }
    if matches!(args.tool.as_str(), "extrude_faces" | "extrude_edges") && form(s)?.matching.is_some() {
        let state = match_state(s)?;
        let form = form(s)?;
        let displacement = solidcore_record::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]);
        if args.tool == "extrude_faces" {
            form.extrude_once_with_match(&state, &args.faces, &[], displacement)?;
        } else {
            form.extrude_once_with_match(&state, &[], &args.edges, displacement)?;
        }
        return Ok(form.inspect());
    }
    let hierarchy = s.forms_prototype.as_ref().and_then(|form| form.hierarchy.as_ref());
    let deleting_cap = hierarchy.filter(|_| args.tool == "delete_faces").and_then(|h| args.faces.iter().copied().find(|&id| h.is_quad_cap_face(id)));
    let refined_erase = args.tool == "erase_and_fill" && hierarchy.is_some();
    let refined_collapse = args.tool == "weld" && hierarchy.is_some();
    if hierarchy.is_some_and(|h| h.has_boundary_collapse())
        && matches!(args.tool.as_str(), "bridge" | "thicken" | "unweld" | "erase_and_fill" | "fill_hole" | "extrude_faces" | "extrude_edges")
    {
        return Err(EngineError::Other("this topology tool requires an untrimmed Form before Collapse".into()));
    }
    let qualified_tolerance = (deleting_cap.is_some() || refined_erase || refined_collapse || (args.tool == "fill_hole" && hierarchy.is_some()))
        .then(|| active_tolerance(s, args.tolerance))
        .transpose()?;
    let collapse_tolerance = crate::forms_prototype::collapse_tolerance(s)?;
    let form = form(s)?;
    if form.profile.is_some() {
        return Err(EngineError::Other("Form Surface keeps its fixed source boundary and guide grid".into()));
    }
    if refined_collapse {
        let [first, second] = args.vertices.as_slice() else {
            return Err(EngineError::Other("select exactly two endpoints of one free side".into()));
        };
        if !args.edges.is_empty() || !args.faces.is_empty() {
            return Err(EngineError::Other("select only the two Collapse endpoints".into()));
        }
        let tolerance = qualified_tolerance.ok_or_else(|| EngineError::Other("missing refined Collapse tolerance".into()))?;
        form.collapse_boundary([*first, *second], tolerance)?;
        return Ok(form.inspect());
    }
    if let Some(cap) = deleting_cap {
        if args.faces != [cap] || !args.edges.is_empty() || !args.vertices.is_empty() {
            return Err(EngineError::Other("select only the quad cap face to delete it".into()));
        }
        let tolerance = qualified_tolerance.ok_or_else(|| EngineError::Other("missing quad cap removal tolerance".into()))?;
        form.delete_logical_faces_at_tolerance(&[cap], tolerance)?;
        return Ok(form.inspect());
    }
    if refined_erase {
        if !args.edges.is_empty() || !args.vertices.is_empty() {
            return Err(EngineError::Other("select only faces of one four-sided interior region to erase and fill".into()));
        }
        let tolerance = qualified_tolerance.ok_or_else(|| EngineError::Other("missing refined Erase and Fill tolerance".into()))?;
        form.erase_and_fill_logical(&args.faces, tolerance)?;
        return Ok(form.inspect());
    }
    if new_cap {
        let tolerance = qualified_tolerance.ok_or_else(|| EngineError::Other("New cap requires a refined Form".into()))?;
        if form.symmetry.is_some() || form.matching.is_some() {
            return Err(EngineError::Other("New cap requires a Form without Match or symmetry".into()));
        }
        if !args.faces.is_empty()
            || args.edges.len() > 256
            || args.vertices.len() > 256
            || args.edges.iter().any(|edge| edge[0] == edge[1])
            || args.edges.iter().map(|&[a, b]| [a.min(b), a.max(b)]).collect::<std::collections::BTreeSet<_>>().len() != args.edges.len()
            || args.vertices.iter().collect::<std::collections::BTreeSet<_>>().len() != args.vertices.len()
        {
            return Err(EngineError::Other("pick distinct edges or points on one four-sided opening for New cap".into()));
        }
        form.author_logical_cap(&args.edges, &args.vertices, tolerance)?;
        return Ok(form.inspect());
    }
    let preserve_topology = form.hierarchy.is_some() || form.symmetry.is_some();
    if preserve_topology && matches!(args.tool.as_str(), "extrude_faces" | "extrude_edges") {
        let displacement = solidcore_record::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]);
        if args.tool == "extrude_faces" {
            form.extrude_once(&args.faces, &[], displacement)?;
        } else {
            form.extrude_once(&[], &args.edges, displacement)?;
        }
        return Ok(form.inspect());
    }
    if form.symmetry.is_some()
        && !matches!(
            args.tool.as_str(),
            "crease" | "uncrease" | "delete_faces" | "dissolve" | "subdivide" | "fill_hole" | "erase_and_fill" | "thicken"
        )
    {
        return Err(EngineError::Other("disable symmetry before changing topology; mirrored topology edits are not implemented yet".into()));
    }
    if let Some(symmetry) = &form.symmetry {
        let mut edges = args.edges.clone();
        let mut vertices = args.vertices.clone();
        for e in &args.edges {
            let a = *symmetry.partners.get(e[0]).ok_or_else(|| EngineError::Other("invalid mirrored edge".into()))?;
            let b = *symmetry.partners.get(e[1]).ok_or_else(|| EngineError::Other("invalid mirrored edge".into()))?;
            edges.push([a, b]);
        }
        for &v in &args.vertices {
            vertices.push(*symmetry.partners.get(v).ok_or_else(|| EngineError::Other("invalid mirrored vertex".into()))?);
        }
        edges.iter_mut().for_each(|e| e.sort_unstable());
        edges.sort_unstable();
        edges.dedup();
        vertices.sort_unstable();
        vertices.dedup();
        args.edges = edges;
        args.vertices = vertices;
    }
    args.faces = form.mirror_face_selection(&args.faces)?;
    if args.tool == "subdivide" {
        edit_with_collapse_fit(form, collapse_tolerance, |form| form.refine(&args.faces, [2, 2]))?;
        return Ok(form.inspect());
    }
    if form.hierarchy.is_some() && matches!(args.tool.as_str(), "crease" | "uncrease") {
        edit_with_collapse_fit(form, collapse_tolerance, |form| {
            form.hierarchical_creases(&args.edges, &args.vertices, if args.tool == "crease" { args.distance } else { 0.0 })
        })?;
        return Ok(form.inspect());
    }
    if args.tool == "delete_faces" && form.hierarchy.is_some() {
        edit_with_collapse_fit(form, collapse_tolerance, |form| form.delete_logical_faces(&args.faces))?;
        return Ok(form.inspect());
    }
    if args.tool == "dissolve" {
        edit_with_collapse_fit(form, collapse_tolerance, |form| form.dissolve_logical_edges(&args.edges))?;
        return Ok(form.inspect());
    }
    if let Some(tolerance) = qualified_tolerance {
        let single = match args.fill_mode {
            params::FillMode::Automatic => false,
            params::FillMode::Single => true,
            params::FillMode::Minimal | params::FillMode::ReducedStar | params::FillMode::NewCap => {
                return Err(EngineError::Other("refined Fill Hole supports Automatic, Single and New cap".into()));
            }
        };
        form.restore_logical_holes(&args.edges, &args.vertices, single, tolerance)?;
        return Ok(form.inspect());
    }
    if args.tool == "bridge" {
        form.bridge_boundaries(&args.edges, &args.vertices)?;
        return Ok(form.inspect());
    }
    let c = &form.cage;
    let d = form.local_vector(solidcore_record::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]));
    let cage = match args.tool.as_str() {
        "extrude_faces" => c.extrude_faces(&args.faces, d),
        "extrude_edges" => c.extrude_edges(&args.edges, d),
        "delete_faces" => c.delete_faces(&args.faces),
        "fill_hole" => {
            let mut edges = args.edges.clone();
            if edges.is_empty() {
                for ring in c.boundary_loops().map_err(|e| EngineError::Other(e.to_string()))? {
                    if args.vertices.iter().any(|v| ring.contains(v))
                        && let (Some(&a), Some(&b)) = (ring.first(), ring.get(1))
                    {
                        edges.push([a, b]);
                    }
                }
            }
            let mode = match args.fill_mode {
                params::FillMode::Automatic => crate::geom::subdivision::topology_tools::FillMode::Automatic,
                params::FillMode::Single => crate::geom::subdivision::topology_tools::FillMode::Single,
                params::FillMode::Minimal => crate::geom::subdivision::topology_tools::FillMode::Minimal,
                params::FillMode::ReducedStar => crate::geom::subdivision::topology_tools::FillMode::Star,
                params::FillMode::NewCap => return Err(EngineError::Other("New cap requires a refined Form".into())),
            };
            c.fill_boundaries(&edges, mode)
        }
        "erase_and_fill" => c.erase_and_fill(&args.faces),
        "weld" => c.weld(&args.vertices),
        "unweld" => c.unweld(&args.faces),
        "thicken" => c.thicken(args.distance, args.sharp),
        "subdivide" => c.refined(1),
        "crease" => c.with_creases(&args.edges, &args.vertices, args.distance),
        "uncrease" => c.with_creases(&args.edges, &args.vertices, 0.0),
        _ => return Err(EngineError::Other("unknown Form topology tool".into())),
    }
    .map_err(|e| EngineError::Other(e.to_string()))?;
    form.replace_cage(cage)?;
    Ok(form.inspect())
}
fn symmetry(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Symmetry = parse(p)?;
    let form = form(s)?;
    if args.axis.is_some() && form.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse()) {
        return Err(EngineError::Other("symmetry requires an untrimmed Form before Collapse".into()));
    }
    form.set_symmetry(args.axis)?;
    Ok(form.inspect())
}
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    edit_with_mode(s, p, false)
}
fn repair(s: &mut Session, p: &Value) -> Result<Value> {
    edit_with_mode(s, p, true)
}
fn edit_with_mode(s: &mut Session, p: &Value, retained: bool) -> Result<Value> {
    let args: params::Edit = parse(p)?;
    if s.active_sketch.is_some() {
        return Err(EngineError::Other("finish the active sketch before editing a Form".into()));
    }
    if let Some(form) = &s.forms_prototype {
        if form.editing_feature == Some(args.feature) {
            if form.repair.is_some() && !retained {
                return Err(EngineError::Other("this retained Form still requires explicit support repair".into()));
            }
            if retained {
                let error = form.repair.as_ref().ok_or_else(|| EngineError::Other("the Form is already open for normal editing".into()))?;
                return Ok(json!({"feature":args.feature,"error":error,"form":form.inspect()}));
            }
            return Ok(form.inspect());
        }
        return Err(EngineError::Other("finish or cancel the active Form before editing another feature".into()));
    }
    let feature = s.doc.features.iter().find(|f| f.id == args.feature).ok_or_else(|| EngineError::Other("no such feature".into()))?;
    let crate::doc::FeatureKind::Form { definition, .. } = &feature.kind else { return Err(EngineError::Other("select a Form feature".into())) };
    if retained && (definition.pipe.is_some() || (definition.profile.is_none() && definition.matching.is_none())) {
        return Err(EngineError::Other("retained support repair requires a saved Match or Surface".into()));
    }
    let levels = definition.cage.preview_level(3).map_err(|e| EngineError::Other(e.to_string()))?;
    let mut definition = definition.as_ref().clone();
    let mut pipe_segments = Vec::new();
    if let Some(pipe) = &definition.pipe {
        let (state, _) = pipe::state(s, feature.component, Some(feature.id))?;
        if pipe.follow_path {
            let built = pipe.rebuild(&state)?;
            definition.cage = built.cage;
            pipe_segments = built.segments;
        } else {
            // Source annotations are optional for a retained sculpted cage.
            pipe_segments = pipe.rebuild(&state).map(|built| built.segments).unwrap_or_default();
        }
    }
    let saved = definition.clone();
    let association = (|| -> Result<()> {
        if let Some(matching) = &definition.matching {
            let before = s.model.state_before(feature.id);
            let state = crate::doc::state_in_frame(&s.doc, &before, feature.component)?;
            definition = matching.recompute(&definition, &state, 0.01)?.0;
        }
        if let Some(profile) = &definition.profile {
            let before = s.model.state_before(feature.id);
            let state = crate::doc::state_in_frame(&s.doc, &before, feature.component)?;
            let (profile, cage) = profile.recompute(&definition, &state)?;
            definition.profile = Some(Box::new(profile));
            definition.cage = cage;
        }
        Ok(())
    })();
    let repair_error = match association {
        Ok(()) if retained => return Err(EngineError::Other("the Form support currently resolves; use normal Edit Form".into())),
        Ok(()) => None,
        Err(error) if retained => {
            definition = saved;
            Some(error.to_string().chars().take(4096).collect::<String>())
        }
        Err(error) => return Err(error),
    };
    let authoring_component = feature.component;
    let authoring_path = if s.active_component == authoring_component { s.active_occurrence_path.clone() } else { None };
    let frame = if s.active_component == authoring_component { s.active_component_frame() } else { s.doc.component_transform(authoring_component) };
    definition.placement = crate::doc::forms::Placement::from_matrix(&crate::doc::mat_mul(&frame, &definition.placement.matrix()))?;
    if let Some(matching) = &mut definition.matching {
        matching.transform_picks(&frame);
    }
    if let Some(profile) = &mut definition.profile {
        profile.transform_picks(&frame);
    }
    if let Some(pipe) = &mut definition.pipe {
        pipe.source.transform_picks(&frame);
    }
    let mut form = crate::forms_prototype::Form::from_definition(definition, levels, Some(feature.id))?;
    form.repair = repair_error.clone();
    form.pipe_segments = pipe_segments;
    form.authoring_component = authoring_component;
    form.authoring_path = authoring_path;
    let reply = if let Some(error) = repair_error { json!({"feature":args.feature,"error":error,"form":form.inspect()}) } else { form.inspect() };
    s.forms_prototype = Some(form);
    Ok(reply)
}
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    set_targets(s, p, crate::forms_prototype::FormTargetSpace::Control)
}

fn set_targets(s: &mut Session, p: &Value, space: crate::forms_prototype::FormTargetSpace) -> Result<Value> {
    let p: params::SetVertices = parse(p)?;
    let tolerance = crate::forms_prototype::collapse_tolerance(s)?;
    let f = form(s)?;
    edit_with_collapse_fit(f, tolerance, |form| form.apply_transform_targets(&p, space))?;
    Ok(f.inspect())
}
fn edit_with_collapse_fit(
    form: &mut crate::forms_prototype::Form,
    tolerance: Option<f64>,
    edit: impl FnOnce(&mut crate::forms_prototype::Form) -> Result<()>,
) -> Result<()> {
    if let Some(tolerance) = tolerance.filter(|_| form.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse())) {
        let mut candidate = form.clone();
        edit(&mut candidate)?;
        if candidate.source_revision() != form.source_revision() {
            crate::forms_prototype::qualify_collapse_edit(&candidate, tolerance)?;
            *form = candidate;
        }
        Ok(())
    } else {
        edit(form)
    }
}
fn transform(s: &mut Session, p: &Value) -> Result<Value> {
    let p: params::Transform = parse(p)?;
    let f = form(s)?;
    f.transform(&p.matrix, p.checkpoint)?;
    Ok(f.inspect())
}
fn set_surface(s: &mut Session, p: &Value) -> Result<Value> {
    set_targets(s, p, crate::forms_prototype::FormTargetSpace::Surface)
}
fn standalone(s: &Session) -> Result<()> {
    let f = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no Form is being edited".into()))?;
    if let Some(id) = f.editing_feature
        && let Some(feature) = s.doc.feature(id)
        && !matches!(feature.kind, crate::doc::FeatureKind::Form { operation: crate::doc::Operation::NewBody, .. })
    {
        return Err(EngineError::Other("Weld and Unweld require standalone Forms".into()));
    }
    Ok(())
}
fn unweld(s: &mut Session, p: &Value) -> Result<Value> {
    standalone(s)?;
    let args: params::Unweld = parse(p)?;
    let current = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no Form is being edited".into()))?;
    if current.editing_feature.is_some_and(|id| refined_weld::attached(&s.doc, id)) {
        return Err(EngineError::Other("remove the refined Weld before splitting its source".into()));
    }
    let tolerance = if current.hierarchy.is_some() { active_tolerance(s, None)? } else { 0.01 };
    let f = form(s)?;
    f.unweld_parts(&args.edges, args.separate, args.name.as_deref().unwrap_or("Unwelded Form"), tolerance)?;
    Ok(f.inspect())
}
fn weld(s: &mut Session, p: &Value) -> Result<Value> {
    standalone(s)?;
    let args: params::Weld = parse(p)?;
    let current = s.forms_prototype.as_ref().ok_or_else(|| EngineError::Other("no Form is being edited".into()))?;
    let (other, saved) = match args.other {
        params::FormObject::Draft { part } => {
            let p = current.parts.get(&part).ok_or_else(|| EngineError::Other("no such staged Form".into()))?;
            (p.definition.as_ref().clone(), None)
        }
        params::FormObject::Feature { feature } => {
            if current.editing_feature == Some(feature) || current.consumes(feature) || current.consumed.len() >= 16 {
                return Err(EngineError::Other("select a different available Form to weld".into()));
            }
            let saved = s.doc.feature(feature).cloned().ok_or_else(|| EngineError::Other("no such Form feature".into()))?;
            let crate::doc::FeatureKind::Form { definition, operation: crate::doc::Operation::NewBody, .. } = &saved.kind else {
                return Err(EngineError::Other("select another standalone Form feature".into()));
            };
            let mut definition = definition.as_ref().clone();
            definition.placement = crate::doc::forms::Placement::from_matrix(&crate::doc::mat_mul(
                &s.doc.component_transform(saved.component),
                &definition.placement.matrix(),
            ))?;
            (definition, Some(std::sync::Arc::new(saved)))
        }
    };
    let f = form(s)?;
    f.weld_definition(&other, args.first_edge, args.second_edge)?;
    match args.other {
        params::FormObject::Draft { part } => {
            f.parts.remove(&part);
        }
        params::FormObject::Feature { feature } => {
            let saved = saved.ok_or_else(|| EngineError::Other("missing original welded Form".into()))?;
            f.consumed.insert(feature, saved);
        }
    }
    Ok(f.inspect())
}
fn quality(s: &mut Session, p: &Value) -> Result<Value> {
    let p: params::Quality = parse(p)?;
    let f = form(s)?;
    f.quality(p.levels)?;
    Ok(f.inspect())
}
fn undo(s: &mut Session, _: &Value) -> Result<Value> {
    let f = raw_form(s)?;
    if f.tool_original.is_some() {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    f.history(false)?;
    Ok(f.inspect())
}
fn redo(s: &mut Session, _: &Value) -> Result<Value> {
    let f = raw_form(s)?;
    if f.tool_original.is_some() {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    f.history(true)?;
    Ok(f.inspect())
}
fn inspect(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(s.forms_prototype.as_ref().map_or_else(|| json!({"active":false}), |f| f.inspect()))
}
fn close(s: &mut Session, _: &Value) -> Result<Value> {
    s.forms_prototype = None;
    Ok(json!({"active":false}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_form_command_restores_source_and_edit_history() {
        use crate::doc::progress::{Progress, watch};
        use std::sync::Arc;
        let mut session = Session::default();
        session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let initial = session.forms_prototype.as_ref().unwrap().definition();
        session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[0,0,1]]})).unwrap();
        let before = session.forms_prototype.as_ref().unwrap().inspect();
        let progress = Arc::new(Progress::default());
        progress.cancel();
        let reply = watch(progress, || session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[0,0,2]]})));
        assert!(matches!(reply, Err(EngineError::Cancelled(_))));
        assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
        session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), initial);
        session.execute("form.prototype.redo", &json!({})).unwrap();
        let recovered = session.forms_prototype.as_ref().unwrap().inspect();
        assert_eq!(recovered["points"], before["points"]);
        assert_eq!(recovered["undo"], before["undo"]);
    }

    #[test]
    fn history_availability_tracks_real_edit_undo_redo_and_cancelled_tool_preview() {
        let mut s = Session::default();
        let available = |s: &Session| s.forms_prototype.as_ref().unwrap().history_available();
        s.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        assert_eq!(available(&s), (false, false));
        assert!(s.execute("form.prototype.undo", &json!({})).is_err());
        assert_eq!(available(&s), (false, false));

        let point = original.cage.points[0];
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y,point.z+0.25]]})).unwrap();
        let edited = s.forms_prototype.as_ref().unwrap().definition();
        assert_ne!(edited, original);
        assert_eq!(available(&s), (true, false));
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(available(&s), (false, true));
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), edited);
        assert_eq!(available(&s), (true, false));
        s.execute("form.prototype.undo", &json!({})).unwrap();

        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        assert_eq!(available(&s), (false, true));
        s.execute("form.prototype.tool.preview", &json!({"topology":{"tool":"subdivide","faces":[0]}})).unwrap();
        assert_ne!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(available(&s), (true, false));
        // Stack availability preserves the existing UI meaning. A live tool
        // still owns its transaction and rejects Undo/Redo until Apply/Cancel.
        assert!(s.execute("form.prototype.undo", &json!({})).is_err());
        assert!(s.execute("form.prototype.redo", &json!({})).is_err());
        assert_eq!(available(&s), (true, false));
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(available(&s), (false, true));
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), edited);
        assert_eq!(available(&s), (true, false));

        s.execute("form.prototype.undo", &json!({})).unwrap();
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y,point.z+0.5]]})).unwrap();
        assert_eq!(available(&s), (true, false));
        assert!(s.execute("form.prototype.redo", &json!({})).is_err(), "a new edit discards the old redo branch");
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(available(&s), (false, true));
    }

    #[test]
    fn form_checking_reports_fit_and_independent_coverage_without_authoring_or_checkpoint_capture() {
        for primitive in ["face", "box", "sphere", "tube", "pipe"] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"primitive":primitive,"segments":[1,1,1],"levels":0})).unwrap();
            s.execute("edit.checkpoint", &json!({})).unwrap();
            assert!(s.checkpoints.last().unwrap().form.is_none());
            let before = s.forms_prototype.as_ref().unwrap().inspect();
            let document = s.doc.clone();
            let recovery = crate::recovery::capture_bytes(&s).unwrap();
            let undo = s.undo.len();
            let input = crate::forms_prototype::validation::Input::capture(&s).unwrap();
            let shared = input.clone();
            assert!(std::sync::Arc::ptr_eq(&input.source, &shared.source));
            assert!(std::sync::Arc::ptr_eq(&input.context, &shared.context));
            assert!(std::sync::Arc::ptr_eq(&input.context.document, &s.doc));
            assert!(std::sync::Arc::ptr_eq(&input.context.before, &s.model.state()));
            let queried = input.check(&params::Validate { tolerance: Some(0.01) }).unwrap();
            let reply: params::ValidationReply =
                serde_json::from_value(s.execute("form.prototype.validate", &json!({"tolerance":0.01})).unwrap()).unwrap();
            assert_eq!(json!(reply), json!(queried), "command and worker query must share the same validation policy");
            assert_eq!(reply.tolerance_mm, 0.01);
            assert_eq!(reply.staged_forms_not_checked, 0);
            if reply.stage == params::CheckStage::Complete {
                assert_eq!(reply.shape, Some(if primitive == "face" { params::CheckedShape::Surface } else { params::CheckedShape::Solid }));
                assert_eq!(reply.conversion.status, params::CheckStatus::Passed);
                assert!(reply.fit.as_ref().unwrap().sampled_deviation_mm.is_finite());
            } else {
                // Unsupported fitted pole/atlas cases must report conversion
                // failure, never silently become passed record/SMLib checks.
                assert_eq!(reply.conversion.status, params::CheckStatus::Failed);
                assert_eq!(reply.record.status, params::CheckStatus::NotRun);
                assert_eq!(reply.smlib.status, params::CheckStatus::NotRun);
                assert!(reply.shape.is_none());
            }
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
            assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
            assert_eq!(s.undo.len(), undo);
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
            assert!(s.checkpoints.last().unwrap().form.is_none(), "a read-only Form check must not fill lazy checkpoint source snapshots");
        }
    }

    #[test]
    fn checking_never_conflates_unsupported_unrun_failed_and_passed_smlib_coverage() {
        use crate::doc::kernel::{SmlibCheck, Validity};
        use params::CheckStatus::{Failed, NotRun, NotSupported, Passed};
        for (smlib, expected) in [
            (None, NotRun),
            (Some(SmlibCheck::Valid), Passed),
            (Some(SmlibCheck::Invalid("invalid topology".into())), Failed),
            (Some(SmlibCheck::NotSupported("unsupported conversion".into())), NotSupported),
        ] {
            let (record, other) = validity_checks(Validity { record: Ok(()), smlib });
            assert_eq!(record.status, Passed);
            assert_eq!(other.status, expected);
        }
        let (record, other) = validity_checks(Validity { record: Err("bad pcurve".into()), smlib: Some(SmlibCheck::Valid) });
        assert_eq!(record.status, Failed);
        assert_eq!(record.message.as_deref(), Some("bad pcurve"));
        assert_eq!(other.status, Passed, "the independent verdict must not hide the record failure");
    }

    #[test]
    fn browser_form_check_uses_the_canonical_report_and_keeps_authored_state() {
        use crate::forms_prototype::validation::BrowserCheck;
        for primitive in ["face", "box", "sphere", "torus"] {
            let mut session = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
            session.execute("form.prototype.create", &json!({"primitive":primitive,"segments":[2,2,2],"levels":0})).unwrap();
            let form = session.forms_prototype.as_mut().unwrap();
            form.placement.origin = Vec3::new(4.0, -1.5, 2.25);
            form.parts.insert(
                42,
                crate::forms_prototype::Part { name: "Other Form".into(), definition: std::sync::Arc::new(form.definition()), levels: None },
            );
            let before = crate::recovery::capture_bytes(&session).unwrap();
            let report = session.execute("form.prototype.validate", &json!({"tolerance":0.05})).unwrap();
            let request = BrowserCheck::capture(&session, 0.05).unwrap().encode().unwrap();
            let response = BrowserCheck::run(&request).unwrap();
            let checked = BrowserCheck::decode_reply(&response).unwrap();
            assert_eq!(json!(checked), report);
            assert_eq!(checked.staged_forms_not_checked, 1);
            assert_eq!(crate::recovery::capture_bytes(&session).unwrap(), before);
        }
    }

    #[test]
    fn browser_form_check_replays_refinement_and_extrusion_without_authoring() {
        use crate::forms_prototype::validation::BrowserCheck;
        let mut session = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
        session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[10,10,1],"segments":[2,2,1],"levels":1})).unwrap();
        session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let check = |session: &mut Session, faces| {
            let form = session.forms_prototype.as_ref().unwrap();
            assert_eq!(form.cage.faces.len(), faces);
            assert_eq!(form.hierarchy.as_ref().unwrap().replay_face_bound(), Some(faces));
            let before = crate::recovery::capture_bytes(session).unwrap();
            let request = BrowserCheck::capture(session, 0.05).unwrap().encode().unwrap();
            let checked = BrowserCheck::decode_reply(&BrowserCheck::run(&request).unwrap()).unwrap();
            let canonical = session.execute("form.prototype.validate", &json!({"tolerance":0.05})).unwrap();
            assert_eq!(json!(checked), canonical);
            assert_eq!(canonical["conversion"]["status"], "passed");
            assert_eq!(crate::recovery::capture_bytes(session).unwrap(), before);
        };
        check(&mut session, 16);
        session.execute("form.prototype.topology", &json!({"tool":"extrude_faces","faces":[4],"displacement":[0,0,3]})).unwrap();
        check(&mut session, 20);
    }

    #[test]
    fn browser_form_check_refuses_unbounded_or_contextual_inputs_before_fitting() {
        use crate::forms_prototype::validation::BrowserCheck;
        let mut session = Session::default();
        session.execute("form.prototype.create", &json!({"primitive":"box","segments":[2,2,2],"levels":0})).unwrap();
        let request = BrowserCheck::capture(&session, 0.05).unwrap().encode().unwrap();
        let mut malformed: Value = serde_json::from_slice(&request).unwrap();
        malformed["tolerance"] = json!(-1);
        assert!(BrowserCheck::decode_reply(&BrowserCheck::run(&serde_json::to_vec(&malformed).unwrap()).unwrap()).is_err());
        assert!(BrowserCheck::decode_reply(&BrowserCheck::run(&vec![0; BrowserCheck::MAX_PAYLOAD + 1]).unwrap()).is_err());
        assert!(BrowserCheck::decode_reply(&vec![0; BrowserCheck::MAX_PAYLOAD + 1]).is_err());
        session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let refined = BrowserCheck::capture(&session, 0.05).unwrap().encode().unwrap();
        let mut forged: Value = serde_json::from_slice(&refined).unwrap();
        // A small terminal cage must not let a much larger hierarchy allocate
        // its evaluator before we discover that the two descriptions disagree.
        forged["definition"]["hierarchy"]["depth"] = json!(4);
        forged["definition"]["hierarchy"]["details"] = json!([[], [], [], [], []]);
        let refused = BrowserCheck::decode_reply(&BrowserCheck::run(&serde_json::to_vec(&forged).unwrap()).unwrap()).unwrap_err();
        assert!(refused.contains("needs the desktop CAD check"));
        let mut associated = Session::default();
        associated.execute("solid.box", &json!({"length":8,"width":8,"height":1,"body_name":"Support"})).unwrap();
        let support = &associated.model.state().body("Support").unwrap().body.clone();
        let top = (0..support.face_count()).find(|&i| support.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        associated.execute("form.surface", &json!({"body":"Support","face":top,"segments":[2,2],"partition":false})).unwrap();
        assert!(!BrowserCheck::available(&associated));
        assert!(BrowserCheck::capture(&associated, 0.05).is_err());
        let mut too_many = Session::default();
        too_many.execute("form.prototype.create", &json!({"primitive":"face","segments":[16,16,1],"levels":0})).unwrap();
        let form = too_many.forms_prototype.as_mut().unwrap();
        form.cage.faces.push(vec![0, 1, 2, 3]);
        assert!(!BrowserCheck::available(&too_many));
    }

    #[test]
    fn form_checking_resolves_matched_support_and_rebuilt_surface_without_installing_them() {
        let (mut matched, _, _) = matched_extrusion_case(0, false);
        let before = crate::recovery::capture_bytes(&matched).unwrap();
        let queried = crate::forms_prototype::validation::Input::capture(&matched).unwrap().check(&params::Validate { tolerance: None }).unwrap();
        let reply: params::ValidationReply = serde_json::from_value(matched.execute("form.prototype.validate", &json!({})).unwrap()).unwrap();
        assert_eq!(json!(reply), json!(queried));
        assert_eq!(reply.stage, params::CheckStage::Complete);
        assert_eq!(reply.shape, Some(params::CheckedShape::Surface));
        assert_eq!(crate::recovery::capture_bytes(&matched).unwrap(), before);

        let mut surface = Session::default();
        surface.execute("solid.box", &json!({"length":8,"width":8,"height":1,"corner":[-4,-4,-1],"body_name":"Support"})).unwrap();
        let body = surface.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        surface.execute("form.surface", &json!({"body":"Support","face":top,"segments":[2,2],"partition":false})).unwrap();
        let before = crate::recovery::capture_bytes(&surface).unwrap();
        let queried = crate::forms_prototype::validation::Input::capture(&surface).unwrap().check(&params::Validate { tolerance: None }).unwrap();
        let reply: params::ValidationReply = serde_json::from_value(surface.execute("form.prototype.validate", &json!({})).unwrap()).unwrap();
        assert_eq!(json!(reply), json!(queried));
        assert_eq!(reply.stage, params::CheckStage::Complete);
        assert_eq!(
            reply.shape,
            Some(params::CheckedShape::Solid),
            "Surface Finish checks the rebuilt source solid rather than just its displayed patch"
        );
        assert_eq!(reply.fit.as_ref().unwrap().basis, params::FitBasis::BoundaryAndGuideSamples);
        assert_eq!(crate::recovery::capture_bytes(&surface).unwrap(), before);

        let mut pipe = Session::default();
        pipe.execute("solid.box", &json!({"corner":[78,17,0],"length":20,"width":20,"height":20,"body_name":"Support","operation":"new"})).unwrap();
        let author = pipe.execute("component.create", &json!({"name":"Pipe author","grounded":false})).unwrap();
        pipe.execute("occurrence.move", &json!({"occurrence":author["occurrence"],"translate":[80,20,5],"angle":90,"axis":[0,0,1],"capture":true}))
            .unwrap();
        pipe.execute("form.pipe", &json!({"body":"Support","edges":[0],"diameter":2})).unwrap();
        let component = pipe.forms_prototype.as_ref().unwrap().authoring_component;
        let another = pipe.doc_mut().add_occurrence(component, 0, crate::doc::IDENTITY).unwrap();
        let path = vec![another];
        pipe.forms_prototype.as_mut().unwrap().authoring_path = Some(path.clone());
        pipe.pending_moves.insert(another, crate::doc::rigid(Vec3::new(80.0, 20.0, 5.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2));
        pipe.active_component = component;
        pipe.active_occurrence_path = Some(path);
        let expected_frame = pipe.active_component_frame();
        pipe.active_component = 0;
        pipe.active_occurrence_path = None;
        let input = crate::forms_prototype::validation::Input::capture(&pipe).unwrap();
        assert_eq!(input.context.frame(), expected_frame, "stored author path and pending move must survive leaving that component");
        let before = crate::recovery::capture_bytes(&pipe).unwrap();
        let queried = input.check(&params::Validate { tolerance: Some(0.05) }).unwrap();
        let reply: params::ValidationReply =
            serde_json::from_value(pipe.execute("form.prototype.validate", &json!({"tolerance":0.05})).unwrap()).unwrap();
        assert_eq!(json!(reply), json!(queried));
        assert_ne!(reply.stage, params::CheckStage::Source, "the pipe support must resolve in its author's translated/rotated frame");
        assert_eq!(crate::recovery::capture_bytes(&pipe).unwrap(), before);
    }

    #[test]
    fn immutable_form_check_keeps_staged_scope_hierarchy_tolerance_and_pre_feature_state() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let staged = s.forms_prototype.as_ref().unwrap().definition();
        s.forms_prototype =
            Some(crate::forms_prototype::Form::from_import(vec![("Active".into(), staged.clone(), 0), ("Staged".into(), staged, 0)]).unwrap());
        s.execute("form.prototype.finish_options", &json!({"tolerance":"0.02 mm"})).unwrap();
        let input = crate::forms_prototype::validation::Input::capture(&s).unwrap();
        let before = crate::recovery::capture_bytes(&s).unwrap();
        let implicit = input.check(&params::Validate { tolerance: None }).unwrap();
        let explicit = input.check(&params::Validate { tolerance: Some(0.02) }).unwrap();
        assert_eq!(json!(implicit), json!(explicit));
        assert_eq!(explicit.staged_forms_not_checked, 1);
        assert_eq!(explicit.shape, Some(params::CheckedShape::Surface));
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), before);

        let mut edited = Session::default();
        edited.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":0})).unwrap();
        let feature = edited.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        edited.execute("form.edit", &json!({"feature":feature})).unwrap();
        let input = crate::forms_prototype::validation::Input::capture(&edited).unwrap();
        assert!(std::sync::Arc::ptr_eq(&input.context.before, &edited.model.state_before(feature)));
        let before = crate::recovery::capture_bytes(&edited).unwrap();
        let queried = input.check(&params::Validate { tolerance: None }).unwrap();
        assert_eq!(queried.tolerance_mm, 0.01);
        assert_eq!(crate::recovery::capture_bytes(&edited).unwrap(), before);
    }

    #[test]
    fn form_checking_refuses_hostile_tolerances_without_changing_the_live_source() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1]})).unwrap();
        let before = crate::recovery::capture_bytes(&s).unwrap();
        for parameters in
            [json!({"tolerance":0}), json!({"tolerance":-1}), json!({"tolerance":2}), json!({"tolerance":"0.01"}), json!({"unexpected":true})]
        {
            assert!(s.execute("form.prototype.validate", &parameters).is_err());
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), before);
        }
    }

    use solidcore_record::Vec3;

    fn sculpted_dimension_fixture(levels: usize) -> Session {
        let mut session = Session::default();
        session.execute("form.prototype.create", &json!({"dimensions":[10,10,10],"segments":[1,1,1],"levels":levels})).unwrap();
        let point = session.forms_prototype.as_ref().unwrap().cage.points[0];
        session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x-1.0,point.y+0.4,point.z+0.2]]})).unwrap();
        session
    }
    fn local_cad_dimension_bounds(form: &crate::forms_prototype::Form) -> crate::geom::Aabb3 {
        let mut definition = form.definition();
        definition.placement = Default::default();
        crate::doc::forms::convert(&definition, 0.02).unwrap().0.exact_bounds().unwrap()
    }

    #[test]
    fn sculpted_dimensions_use_exact_cad_extents_independently_of_preview_quality() {
        let mut results = Vec::new();
        for levels in [0, 3] {
            let mut session = sculpted_dimension_fixture(levels);
            let before = session.forms_prototype.as_ref().unwrap().definition();
            let bounds = local_cad_dimension_bounds(session.forms_prototype.as_ref().unwrap());
            let undo = session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
            session.execute("form.prototype.parameters", &json!({"dimensions":[14,10,10],"tolerance":0.02})).unwrap();
            let form = session.forms_prototype.as_ref().unwrap();
            let resized = local_cad_dimension_bounds(form);
            assert!((resized.size().x - 14.0).abs() <= 0.02, "{:?}", resized.size());
            assert!((resized.size().y - bounds.size().y).abs() <= 0.04);
            assert!((resized.size().z - bounds.size().z).abs() <= 0.04);
            assert!(resized.center().dist(bounds.center()) <= 0.04);
            assert_eq!(form.inspect()["undo"], undo + 1);
            results.push(form.definition());
            session.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), before);
            session.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(&session.forms_prototype.as_ref().unwrap().definition(), results.last().unwrap());
            assert!(session.doc.features.is_empty() && session.undo.is_empty());
        }
        assert_eq!(results[0], results[1], "preview tessellation must never change the authored dimension edit");
    }

    #[test]
    fn sculpted_dimensions_follow_the_placed_forms_local_axes() {
        use solidcore_record::Vec3;
        let mut session = sculpted_dimension_fixture(1);
        let matrix = crate::doc::rigid(Vec3::new(30.0, -20.0, 8.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
        session.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
        let before = session.forms_prototype.as_ref().unwrap().definition();
        session.execute("form.prototype.parameters", &json!({"dimensions":[14,10,10],"tolerance":0.02})).unwrap();
        let form = session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.placement, before.placement);
        assert!((local_cad_dimension_bounds(form).size().x - 14.0).abs() <= 0.02);
        let world = crate::doc::forms::convert(&form.definition(), 0.02).unwrap().0.exact_bounds().unwrap();
        assert!((world.size().y - 14.0).abs() <= 0.02, "local X must be world Y after this placement");
        assert!((world.size().x - 14.0).abs() > 1.0);
        session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn refused_dimension_bounds_leave_the_form_and_its_history_unchanged() {
        let mut session = sculpted_dimension_fixture(1);
        let before = session.forms_prototype.as_ref().unwrap().inspect();
        for parameters in [json!({"dimensions":[1e20,10,10]}), json!({"dimensions":[14,10,10],"tolerance":0.0})] {
            assert!(session.execute("form.prototype.parameters", &parameters).is_err());
            assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
        }
        let form = session.forms_prototype.as_mut().unwrap();
        let mut parameters = form.parameters.clone();
        parameters.dimensions[0] = 14.0;
        assert!(form.set_parameters(parameters.clone(), 0.02, |_| Err(EngineError::Other("exact bounds unavailable".into()))).is_err());
        assert_eq!(form.inspect(), before);
        assert!(
            form.set_parameters(parameters, 0.02, |definition| Ok(crate::forms_prototype::DimensionShape {
                definition: definition.clone(),
                bounds: crate::geom::Aabb3::EMPTY,
                match_report: None
            }))
            .is_err()
        );
        assert_eq!(form.inspect(), before);
    }

    #[test]
    fn blender_surface_export_evaluates_refinement_matching_profiles_and_staged_forms_without_finishing() {
        use solidcore_record::Vec3;
        let path = std::env::temp_dir().join(format!(
            "solidcraft-form-surfaces-{}-{}.blend",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fn export(s: &mut Session, path: &std::path::Path, expected: usize) {
            let before = s.forms_prototype.as_ref().unwrap().inspect();
            let doc = s.doc.clone();
            let undo = s.undo.len();
            let reply = s.execute("form.blend.export", &json!({"path":path,"surface":true,"tolerance":0.05})).unwrap();
            assert_eq!(reply["surface"], true);
            assert_eq!(reply["objects"].as_array().unwrap().len(), expected);
            assert!(reply["objects"].as_array().unwrap().iter().all(|o| o["faces"].as_u64().unwrap() > 0 && o["levels"] == 0));
            assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
            assert_eq!(s.undo.len(), undo);
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
            let file = solidcraft_io::blend::read(&std::fs::read(path).unwrap()).unwrap();
            assert!(file.objects.is_empty());
            assert_eq!(file.rejected.len(), expected);
        }
        let mut refined = Session::default();
        refined.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1]})).unwrap();
        refined.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        assert!(refined.forms_prototype.as_ref().unwrap().hierarchy.is_some());
        assert!(refined.execute("form.blend.export", &json!({"path":path})).is_err());
        export(&mut refined, &path, 1);
        for profile in [false, true] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length":4,"width":6,"height":8,"corner":[-2,-3,0],"operation":"new","body_name":"Support"})).unwrap();
            let body = s.model.state().bodies[0].clone();
            let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
            if profile {
                s.execute("form.surface", &json!({"body":"Support","face":top})).unwrap();
                let p = s.forms_prototype.as_ref().unwrap().control_positions()[4];
                s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[4],"positions":[[p.x,p.y,p.z+1.]]})).unwrap();
            } else {
                s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":2})).unwrap();
                let edge = opening(&s.forms_prototype.as_ref().unwrap().cage);
                s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":1})).unwrap();
            }
            export(&mut s, &path, 1);
        }
        let mut staged = Session::default();
        staged.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1]})).unwrap();
        let form = staged.forms_prototype.as_ref().unwrap();
        let first = form.definition();
        let mut second = first.clone();
        second.placement.origin = Vec3::new(20., 0., 0.);
        staged.forms_prototype =
            Some(crate::forms_prototype::Form::from_import(vec![("First".into(), first, 2), ("Second".into(), second, 2)]).unwrap());
        export(&mut staged, &path, 2);
        staged.execute("form.prototype.close", &json!({})).unwrap();
        staged.execute("solid.box", &json!({"corner":[30,40,50],"length":10,"width":20,"height":30,"operation":"new"})).unwrap();
        let reply = staged.execute("form.blend.export", &json!({"path":path,"surface":true})).unwrap();
        assert_eq!(reply["objects"].as_array().unwrap().len(), 1);
        assert!(staged.forms_prototype.is_none());
        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_file(solidcraft_io::backup_path(&path));
    }
    #[test]
    fn blender_import_stages_independent_forms_and_export_retains_saved_source_and_world_frame() {
        use crate::geom::subdivision::primitives::Parameters;
        use solidcore_record::Vec3;
        let folder = std::env::temp_dir().join(format!(
            "solidcraft-blend-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        let source = folder.join("source.blend");
        let destination = folder.join("export.blend");
        let cage = Parameters { segments: [1; 3], dimensions: [4.; 3], ..Default::default() }.build().unwrap();
        let mut second = cage.clone();
        for p in &mut second.points {
            *p += Vec3::new(12., 0., 0.);
        }
        let objects = vec![
            solidcraft_io::blend::Object { name: "Imported first".into(), cage: cage.clone(), levels: 2 },
            solidcraft_io::blend::Object { name: "Imported second".into(), cage: second, levels: 1 },
        ];
        std::fs::write(&source, solidcraft_io::blend::write(&objects).unwrap()).unwrap();
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1]})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().cage.clone();
        let import = s.execute("form.blend.import", &json!({"path":source})).unwrap();
        assert_eq!(import["objects"].as_array().unwrap().len(), 2);
        assert!(s.doc.features.is_empty());
        assert_eq!(s.forms_prototype.as_ref().unwrap().parts.len(), 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage, original);
        assert!(s.forms_prototype.as_ref().unwrap().parts.is_empty());
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().parts.len(), 1);
        assert!(s.execute("form.blend.import", &json!({"path":source})).is_err());
        let export = s.execute("form.blend.export", &json!({"path":destination})).unwrap();
        assert!(export["bytes"].as_u64().unwrap() > 1000);
        let round = solidcraft_io::blend::read(&std::fs::read(&destination).unwrap()).unwrap();
        assert_eq!(round.objects.len(), 2);
        assert_eq!(round.objects.iter().map(|o| o.levels).collect::<Vec<_>>(), [2, 1]);
        let mut recovered = crate::recovery::restore_bytes(&crate::recovery::capture_bytes(&s).unwrap(), None).unwrap();
        let export = recovered.execute("form.blend.export", &json!({"path":destination})).unwrap();
        assert_eq!(export["objects"][0]["levels"], 2);
        assert_eq!(export["objects"][1]["levels"], 1);
        assert_eq!(round.objects[0].cage.faces, cage.faces);
        let finish = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        assert_eq!(finish["parts"].as_array().unwrap().len(), 1);
        assert_eq!(s.doc.features.len(), 2);
        assert_eq!(s.doc.features[0].name, "Imported first");
        assert_eq!(s.doc.features[1].name, "Imported second");
        for f in &s.doc.features {
            assert!(s.model.result(f.id).unwrap().error.is_none());
        }
        let encoded = serde_json::to_vec(s.doc.as_ref()).unwrap();
        let saved: crate::doc::Document = serde_json::from_slice(&encoded).unwrap();
        let id = s.doc.features[0].id;
        let mut reopened = Session { doc: std::sync::Arc::new(saved), ..Default::default() };
        reopened.refresh();
        reopened.execute("form.edit", &json!({"feature":id})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().imported.as_ref().unwrap().object, "Imported first");
        reopened.execute("form.prototype.close", &json!({})).unwrap();
        reopened.execute("form.blend.export", &json!({"path":destination,"features":[id]})).unwrap();
        assert_eq!(solidcraft_io::blend::read(&std::fs::read(&destination).unwrap()).unwrap().objects.len(), 1);
        // A selected import is independent of unselected supported cages.
        reopened.execute("form.blend.import", &json!({"path":source,"objects":["Imported second"]})).unwrap();
        assert!(reopened.forms_prototype.as_ref().unwrap().parts.is_empty());
        reopened.execute("form.prototype.close", &json!({})).unwrap();
        assert_eq!(reopened.doc.features.len(), 2);
        std::fs::remove_file(&source).unwrap();
        std::fs::remove_file(&destination).unwrap();
        let _ = std::fs::remove_file(folder.join("export.blend.bak"));
        std::fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn pipe_sources_and_rigid_offsets_use_the_author_component_frame_once() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        let origin = Vec3::new(78.0, 17.0, 0.0);
        let base = s
            .execute("solid.box", &json!({"corner":[78,17,0],"length":20,"width":20,"height":20,"body_name":"Support","operation":"new"}))
            .unwrap()["feature"]
            .as_u64()
            .unwrap();
        let mesh = s.model.state().body("Support").unwrap().mesh();
        let edges: Vec<_> = mesh
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.first().is_some_and(|p| p.dist(origin) < 1e-8) || e.last().is_some_and(|p| p.dist(origin) < 1e-8))
            .map(|(i, _)| i)
            .collect();
        let author = s.execute("component.create", &json!({"name":"Pipe author","grounded":false})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence":author["occurrence"],"translate":[80,20,5],"angle":90,"axis":[0,0,1],"capture":true}))
            .unwrap();
        s.execute("form.pipe", &json!({"body":"Support","edges":edges,"diameter":2})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().control_positions();
        let feature = s.execute("form.prototype.commit", &json!({"name":"Pipe","tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        assert_eq!(s.doc.feature(feature).unwrap().component, author["component"].as_u64().unwrap());
        assert_eq!(s.model.state().body("Support").unwrap().component, 0);
        let saved = s.model.saved(&s.doc).unwrap();
        s.model = crate::doc::Model::adopt(&s.doc, &saved).unwrap();
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let reopened = s.forms_prototype.as_ref().unwrap().control_positions();
        assert_eq!(reopened.len(), original.len());
        assert!(reopened.iter().zip(&original).all(|(a, b)| a.dist(*b) < 1e-6));
        let translation = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [3, 0, 0, 1]];
        s.execute("form.prototype.transform", &json!({"matrix":translation})).unwrap();
        s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        if let crate::doc::FeatureKind::Box { width, .. } = &mut s.doc_mut().feature_mut(base).unwrap().kind {
            *width = "25 mm".into();
        }
        s.refresh();
        assert!(s.model.result(feature).unwrap().error.is_none(), "{:?}", s.model.result(feature).unwrap().error);
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let positions = form.control_positions();
        let max_x = positions.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let max_y = positions.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        assert!((max_x - 101.0).abs() < 1e-5 && (max_y - 42.0).abs() < 1e-5, "{max_x}, {max_y}");
        assert!(form.pipe.as_ref().unwrap().follow_path);
    }
    #[test]
    fn pipe_path_preview_saved_recompute_sculpt_freeze_and_regeneration_are_undoable() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        let base = s.execute("solid.box", &json!({"length":20,"width":20,"height":20,"body_name":"Support","operation":"new"})).unwrap()["feature"]
            .as_u64()
            .unwrap();
        let mesh = s.model.state().body("Support").unwrap().mesh();
        let edges: Vec<_> = mesh
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.first().is_some_and(|p| p.len() < 1e-8) || e.last().is_some_and(|p| p.len() < 1e-8))
            .map(|(i, _)| i)
            .collect();
        let request = json!({"body":"Support","edges":edges,"diameter":2});
        s.execute("form.prototype.create", &json!({"size":2,"segments":[1,1,1],"levels":2})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().definition();
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"pipe":request})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().pipe.is_some());
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.pipe", &request).unwrap();
        let translation = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 3, 1]];
        s.execute("form.prototype.transform", &json!({"matrix":translation})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().pipe.as_ref().unwrap().follow_path);
        let feature = s.execute("form.prototype.commit", &json!({"name":"Pipe","tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        assert_eq!(s.model.state().bodies.len(), 2);
        if let crate::doc::FeatureKind::Box { length, .. } = &mut s.doc_mut().feature_mut(base).unwrap().kind {
            *length = "30 mm".into();
        } else {
            panic!("box")
        }
        s.refresh();
        assert!(s.model.result(feature).unwrap().error.is_none());
        let saved = s.model.saved(&s.doc).unwrap();
        s.model = crate::doc::Model::adopt(&s.doc, &saved).unwrap();
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!(f.control_positions().iter().any(|p| p.x > 29.9));
        let p = f.control_positions()[0] + Vec3::Z * 0.1;
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z]]})).unwrap();
        assert!(!s.forms_prototype.as_ref().unwrap().pipe.as_ref().unwrap().follow_path);
        let sculpt = s.forms_prototype.as_ref().unwrap().definition();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().pipe.as_ref().unwrap().follow_path);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), sculpt);
        assert!(s.execute("form.pipe", &json!({"diameter":3})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), sculpt);
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"pipe":{"diameter":3,"regenerate":true}})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().pipe.as_ref().unwrap().follow_path);
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), sculpt);
        s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        if let crate::doc::FeatureKind::Box { length, .. } = &mut s.doc_mut().feature_mut(base).unwrap().kind {
            *length = "40 mm".into();
        }
        s.refresh();
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), sculpt);
    }
    #[test]
    fn form_surface_in_a_placed_component_replaces_the_root_source_in_its_own_frame() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        let base = s
            .execute("solid.box", &json!({"corner":[78,17,0],"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"}))
            .unwrap()["feature"]
            .as_u64()
            .unwrap();
        let body = s.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let component = s.execute("component.create", &json!({"name":"Surface author","grounded":false})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence":component["occurrence"],"translate":[80,20,5],"angle":90,"axis":[0,0,1],"capture":true}))
            .unwrap();
        s.execute("form.surface", &json!({"body":"Support","face":top})).unwrap();
        let p = s.forms_prototype.as_ref().unwrap().surface.vertices[4] + Vec3::Z;
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[4],"positions":[[p.x,p.y,p.z]]})).unwrap();
        let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert_eq!(s.model.state().body("Support").unwrap().component, 0);
        assert_eq!(s.doc.feature(feature).unwrap().component, component["component"].as_u64().unwrap());
        let bounds = s.model.state().body("Support").unwrap().body.exact_bounds().unwrap();
        assert!((bounds.min.x - 78.0).abs() < 1e-6 && (bounds.min.y - 17.0).abs() < 1e-6 && bounds.min.z.abs() < 1e-6, "{bounds:?}");
        assert!((bounds.max.x - 88.0).abs() < 1e-6 && (bounds.max.y - 27.0).abs() < 1e-6 && bounds.max.z > 2.0, "{bounds:?}");
        if let crate::doc::FeatureKind::Box { height, .. } = &mut s.doc_mut().feature_mut(base).unwrap().kind {
            *height = "3 mm".into();
        } else {
            panic!("expected Box");
        }
        s.refresh();
        assert!(s.model.result(feature).unwrap().error.is_none(), "{:?}", s.model.result(feature).unwrap().error);
        let saved = s.model.saved(&s.doc).unwrap();
        s.model = crate::doc::Model::adopt(&s.doc, &saved).unwrap();
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert!(form.control_positions()[4].dist(Vec3::new(83.0, 22.0, 4.0)) < 1e-6);
        assert!(form.profile.as_ref().unwrap().boundary().iter().all(|&i| (form.control_positions()[i].z - 3.0).abs() < 1e-6));
    }
    fn retained_support_fixture(surface: bool) -> (Session, u64, usize) {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"corner":[-2,-3,-1],"length":4,"width":6,"height":1,"body_name":"Support","operation":"new"})).unwrap();
        let body = s.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
        if surface {
            s.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
            let points = &s.forms_prototype.as_ref().unwrap().cage.points;
            let a = points[5] + solidcore_record::Vec3::new(0.0, 0.0, 0.4);
            let b = points[6] + solidcore_record::Vec3::new(0.0, 0.0, 0.15);
            s.execute("form.prototype.set_vertices", &json!({"vertices":[5,6],"positions":[[a.x,a.y,a.z],[b.x,b.y,b.z]]})).unwrap();
        } else {
            s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,1],"levels":1})).unwrap();
            let center = s.forms_prototype.as_ref().unwrap().cage.points.iter().position(|p| p.x.abs() < 1e-8 && p.y.abs() < 1e-8).unwrap();
            s.execute("form.prototype.set_vertices", &json!({"vertices":[center],"positions":[[0,0,2]]})).unwrap();
            let edge = opening(&s.forms_prototype.as_ref().unwrap().cage);
            s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":0,"tolerance":0.01})).unwrap();
        }
        let feature = s.execute("form.prototype.commit", &json!({"name":"Retained","tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.execute("form.repair", &json!({"feature":feature})).is_err(), "healthy support uses ordinary Edit");
        assert!(s.forms_prototype.is_none());
        let crate::doc::FeatureKind::Form { definition, .. } = &mut s.doc_mut().feature_mut(feature).unwrap().kind else { panic!("Form fixture") };
        let definition = definition.as_mut();
        let source = if surface { &mut definition.profile.as_mut().unwrap().source } else { &mut definition.matching.as_mut().unwrap().source };
        let crate::doc::forms::SupportRef::Face { body, .. } = source else { panic!("face support") };
        *body = "Missing retained support".into();
        s.refresh();
        assert!(s.model.result(feature).unwrap().error.is_some());
        (s, feature, top)
    }
    fn saved_form_source(s: &Session, id: u64) -> crate::doc::forms::Definition {
        let crate::doc::FeatureKind::Form { definition, .. } = &s.doc.feature(id).unwrap().kind else { panic!("Form fixture") };
        definition.as_ref().clone()
    }
    #[test]
    fn genuine_suppressed_support_reattachment_finishes_with_new_face_names_and_kept_consumers() {
        for surface in [false, true] {
            let mut s = Session::default();
            let request = |name| json!({"corner":[-2,-3,-1],"length":4,"width":6,"height":1,"body_name":name,"operation":"new"});
            let lost = s.execute("solid.box", &request("LostSupport")).unwrap()["feature"].as_u64().unwrap();
            let replacement = s.execute("solid.box", &request("RepairSupport")).unwrap()["feature"].as_u64().unwrap();
            let body = s.model.state().body("LostSupport").unwrap().body.clone();
            let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
            if surface {
                s.execute("form.surface", &json!({"body":"LostSupport","face":top,"segments":[3,2],"partition":false})).unwrap();
                let points = &s.forms_prototype.as_ref().unwrap().cage.points;
                let a = points[5] + Vec3::new(0.03, -0.04, 0.4);
                let b = points[6] + Vec3::new(-0.02, 0.01, 0.15);
                s.execute("form.prototype.set_vertices", &json!({"vertices":[5,6],"positions":[[a.x,a.y,a.z],[b.x,b.y,b.z]]})).unwrap();
            } else {
                s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,1],"levels":1})).unwrap();
                let center = s.forms_prototype.as_ref().unwrap().cage.points.iter().position(|p| p.x.abs() + p.y.abs() < 1e-8).unwrap();
                s.execute("form.prototype.set_vertices", &json!({"vertices":[center],"positions":[[0,0,2]]})).unwrap();
                let edge = opening(&s.forms_prototype.as_ref().unwrap().cage);
                s.execute("form.prototype.match", &json!({"body":"LostSupport","face":top,"edge":edge,"order":0})).unwrap();
            }
            let id = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
            let source = saved_form_source(&s, id);
            let names_before = s.doc.feature(id).unwrap().face_names.clone();
            assert!(names_before.iter().any(|n| n.starts_with(&format!("F{lost}:"))));
            // Retain an actual healthy incoming whole-body consumer, without
            // undoing it before Finish. Its identity and source body must survive.
            let child = s.execute("solid.move", &json!({"bodies":["RepairSupport"],"translate":[0,0,0.5]})).unwrap()["feature"].as_u64().unwrap();
            let child_before = s.doc.feature(child).unwrap().clone();
            assert!(s.model.result(child).unwrap().error.is_none());
            let logical_face = {
                let state = s.model.state_before(child);
                state
                    .bodies
                    .iter()
                    .find_map(|body| {
                        let names = crate::doc::naming::face_names(body);
                        let index = names.iter().position(|name| name.starts_with(&format!("F{id}:")))?;
                        let at = crate::doc::naming::face_spot(body, index)?.point;
                        Some(crate::doc::construct::GeoRef::Face { at, name: Some(names[index].clone()) })
                    })
                    .unwrap()
            };
            let logical_pick = logical_face.picked_point(&s.model.state_before(child)).unwrap();
            s.execute("timeline.suppress", &json!({"feature":lost,"suppressed":true})).unwrap();
            assert!(s.model.result(id).unwrap().error.is_some());
            assert!(s.model.result(child).unwrap().error.is_none());
            assert_eq!(saved_form_source(&s, id), source);
            let document = serde_json::to_value(s.doc.as_ref()).unwrap();
            assert!(s.execute("form.edit", &json!({"feature":id})).is_err());
            s.execute("form.repair", &json!({"feature":id})).unwrap();
            s.execute("form.prototype.tool.begin", &json!({})).unwrap();
            let preview = if surface {
                json!({"surface":{"body":"RepairSupport","face":top,"segments":[3,2],"partition":false,"reattach":{"swap":false,"reverse_u":false,"reverse_v":false}}})
            } else {
                json!({"matching":{"body":"RepairSupport","face":top,"edge":opening(&source.cage),"order":0}})
            };
            s.execute("form.prototype.tool.preview", &preview).unwrap();
            s.execute("form.prototype.tool.apply", &json!({})).unwrap();
            let repaired = s.forms_prototype.as_ref().unwrap().definition();
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
            assert!(s.forms_prototype.as_ref().unwrap().repair.is_some());
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
            s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap();
            assert!(s.model.result(id).unwrap().error.is_none());
            assert!(s.model.result(child).unwrap().error.is_none());
            assert_eq!(s.doc.feature(child), Some(&child_before));
            let feature = s.doc.feature(id).unwrap();
            assert_eq!(feature.face_names.len(), names_before.len());
            assert!(feature.face_names.iter().all(|name| name.starts_with(&format!("F{replacement}:"))), "{:?}", feature.face_names);
            assert!(logical_face.picked_point(&s.model.state_before(child)).unwrap().dist(logical_pick) < 1e-7);
            assert_eq!(form_support(&saved_form_source(&s, id)).unwrap().body().as_ref(), "RepairSupport");
            if surface {
                // Surface replaces its selected source and retains that body's
                // ownership; it does not retarget the suppressed body's identity.
                assert!(s.model.state().body("LostSupport").is_none());
                assert_eq!(s.model.state().body("RepairSupport").unwrap().feature, replacement);
            }
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
            assert_eq!(s.doc.feature(id).unwrap().face_names, names_before);
            s.execute("edit.redo", &json!({})).unwrap();
            let encoded = serde_json::to_vec(s.doc.as_ref()).unwrap();
            let document: crate::doc::Document = serde_json::from_slice(&encoded).unwrap();
            let mut reopened = Session { doc: std::sync::Arc::new(document), ..Default::default() };
            reopened.refresh();
            assert!(reopened.model.result(id).unwrap().error.is_none());
            assert!(reopened.model.result(child).unwrap().error.is_none());
            assert_eq!(reopened.doc.feature(child), Some(&child_before));
            reopened.execute("form.edit", &json!({"feature":id})).unwrap();
            assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), repaired);
        }
    }
    #[test]
    fn reattachment_refuses_a_newly_broken_selected_source_face_consumer_atomically() {
        let mut s = Session::default();
        let request = |name| json!({"corner":[-2,-3,-1],"length":4,"width":6,"height":1,"body_name":name,"operation":"new"});
        let lost = s.execute("solid.box", &request("LostSupport")).unwrap()["feature"].as_u64().unwrap();
        let replacement = s.execute("solid.box", &request("RepairSupport")).unwrap()["feature"].as_u64().unwrap();
        let body = s.model.state().body("LostSupport").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        s.execute("form.surface", &json!({"body":"LostSupport","face":top,"segments":[3,2],"partition":false})).unwrap();
        let points = &s.forms_prototype.as_ref().unwrap().cage.points;
        let a = points[5] + Vec3::new(0.03, -0.04, 0.4);
        let b = points[6] + Vec3::new(-0.02, 0.01, 0.15);
        s.execute("form.prototype.set_vertices", &json!({"vertices":[5,6],"positions":[[a.x,a.y,a.z],[b.x,b.y,b.z]]})).unwrap();
        let id = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let state = s.model.state();
        let selected = crate::doc::forms::SupportRef::face(state.body("RepairSupport").unwrap(), top).unwrap();
        let crate::doc::forms::SupportRef::Face { name, pick, .. } = selected else { panic!("face") };
        assert!(name.starts_with(&format!("F{replacement}:")));
        // This is a genuine, initially supported analytic-face consumer. A
        // curved NURBS cap cannot silently become its analytic plane/cylinder.
        let consumer = s
            .execute(
                "construct.axis.normal_to_face",
                &json!({
                    "face":{"type":"face","at":[pick.x,pick.y,pick.z],"name":name},
                    "point":{"point":[pick.x,pick.y,pick.z]},"name":"Replacement plane normal"
                }),
            )
            .unwrap()["feature"]
            .as_u64()
            .unwrap();
        assert!(s.model.result(consumer).unwrap().error.is_none());
        s.execute("timeline.suppress", &json!({"feature":lost,"suppressed":true})).unwrap();
        assert!(s.model.result(id).unwrap().error.is_some());
        assert!(s.model.result(consumer).unwrap().error.is_none());
        s.execute("form.repair", &json!({"feature":id})).unwrap();
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"surface":{"body":"RepairSupport","face":top,"segments":[3,2],"partition":false,"reattach":{"swap":false,"reverse_u":false,"reverse_v":false}}})).unwrap();
        s.execute("form.prototype.tool.apply", &json!({})).unwrap();
        let document = serde_json::to_value(s.doc.as_ref()).unwrap();
        let draft = s.forms_prototype.as_ref().unwrap().definition();
        let error = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap_err().to_string();
        assert!(error.contains("reattaching the Form would break"), "{error}");
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), draft);
        assert!(s.model.result(consumer).unwrap().error.is_none());
        s.execute("form.prototype.close", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
    }
    #[test]
    fn a_moved_legacy_support_certificate_keeps_existing_named_faces_on_noop_finish() {
        let mut s = Session::default();
        let base = s.execute("solid.box", &json!({"length":12,"width":8,"height":2,"body_name":"Support","operation":"new"})).unwrap()["feature"]
            .as_u64()
            .unwrap();
        let body = s.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let movement = s.execute("solid.move", &json!({"bodies":["Support"],"translate":[0,0,0]})).unwrap()["feature"].as_u64().unwrap();
        s.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2]})).unwrap();
        let points = &s.forms_prototype.as_ref().unwrap().cage.points;
        let a = points[5] + Vec3::new(0.03, -0.04, 0.4);
        let b = points[6] + Vec3::new(-0.02, 0.01, 0.15);
        s.execute("form.prototype.set_vertices", &json!({"vertices":[5,6],"positions":[[a.x,a.y,a.z],[b.x,b.y,b.z]]})).unwrap();
        let id = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        // A source-only old-format record has no chart certificate yet. Its
        // existing face references remain authoritative as the source moves.
        let crate::doc::FeatureKind::Form { definition, .. } = &mut s.doc_mut().feature_mut(id).unwrap().kind else { panic!("Form") };
        definition.profile.as_mut().unwrap().chart = None;
        s.refresh();
        let legacy = s.doc.feature(id).unwrap().kind.clone();
        s.execute("form.edit", &json!({"feature":id})).unwrap();
        let mut upgraded = legacy.clone();
        let crate::doc::FeatureKind::Form { definition, .. } = &mut upgraded else { panic!("Form") };
        **definition = s.forms_prototype.as_ref().unwrap().definition();
        assert!(definition.profile.as_ref().unwrap().chart.is_some());
        assert!(!form_attachment_changed(&legacy, &upgraded));
        for hint in upgraded.face_points_mut() {
            *hint += Vec3::new(7.0, -4.0, 9.0);
        }
        assert!(!form_attachment_changed(&legacy, &upgraded), "lookup hints do not change named support identity");
        s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap();
        let names = s.doc.feature(id).unwrap().face_names.clone();
        s.execute(
            "timeline.redefine",
            &json!({"feature":base,"command":"solid.box","params":{"length":12,"width":8,"height":3,"body_name":"Support","operation":"new"}}),
        )
        .unwrap();
        s.execute(
            "timeline.redefine",
            &json!({"feature":movement,"command":"solid.move","params":{"bodies":["Support"],"translate":[7,-4,9],"axis":[1,0,0],"angle":90}}),
        )
        .unwrap();
        assert!(s.model.result(id).unwrap().error.is_none());
        let old = s.doc.feature(id).unwrap().kind.clone();
        s.execute("form.edit", &json!({"feature":id})).unwrap();
        let edited = s.forms_prototype.as_ref().unwrap().definition();
        let crate::doc::FeatureKind::Form { definition, .. } = &old else { panic!("Form") };
        let mut changed = old.clone();
        let crate::doc::FeatureKind::Form { definition: changed_definition, .. } = &mut changed else { panic!("Form") };
        **changed_definition = edited;
        assert!(!form_attachment_changed(&old, &changed));
        assert_eq!(form_support(definition).unwrap().body().as_ref(), "Support");
        s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap();
        assert!(s.model.result(id).unwrap().error.is_none());
        assert_eq!(s.doc.feature(id).unwrap().face_names, names);
        // Coincident picks do not make different named bodies or faces the same
        // attachment; test the semantic trigger independently of numeric picks.
        let mut changed = s.doc.feature(id).unwrap().kind.clone();
        let crate::doc::FeatureKind::Form { definition, .. } = &mut changed else { panic!("Form") };
        let crate::doc::forms::SupportRef::Face { name, .. } = &mut definition.profile.as_mut().unwrap().source else { panic!("face") };
        name.push_str("-different-face");
        assert!(form_attachment_changed(&s.doc.feature(id).unwrap().kind, &changed));
    }
    #[test]
    fn retained_support_repair_is_explicit_cancelable_and_recoverable_without_document_mutation() {
        for surface in [false, true] {
            let (mut s, feature, _) = retained_support_fixture(surface);
            let document = serde_json::to_value(s.doc.as_ref()).unwrap();
            let source = saved_form_source(&s, feature);
            assert!(s.execute("form.edit", &json!({"feature":feature})).is_err());
            assert!(s.forms_prototype.is_none());
            let opened = s.execute("form.repair", &json!({"feature":feature})).unwrap();
            assert!(!opened["error"].as_str().unwrap().is_empty());
            let form = s.forms_prototype.as_ref().unwrap();
            assert_eq!(form.definition(), source);
            let marker = form.repair.clone();
            assert!(s.execute("form.edit", &json!({"feature":feature})).is_err());
            assert!(s.execute("form.prototype.commit", &json!({})).is_err());
            assert!(s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[0,0,9]]})).is_err());
            assert!(s.execute("form.prototype.create", &json!({})).is_err());
            s.execute("form.prototype.tool.begin", &json!({})).unwrap();
            assert!(s.execute("form.prototype.tool.apply", &json!({})).is_err());
            assert!(s.execute("form.prototype.tool.preview", &json!({"project":{"tool":"flatten","vertices":[0,1,2]}})).is_err());
            let encoded = crate::forms_prototype::draft::Draft::capture(s.forms_prototype.as_ref().unwrap(), false).encode().unwrap();
            let (recovered, dirty) = crate::forms_prototype::draft::Draft::restore(&s, &encoded).unwrap();
            assert!(!dirty);
            assert_eq!(recovered.definition(), source);
            assert_eq!(recovered.repair, marker);
            assert!(!recovered.has_tool_preview());
            let mut recovered_session = s.scratch();
            recovered_session.forms_prototype = Some(recovered);
            assert!(recovered_session.execute("form.prototype.commit", &json!({})).is_err());
            s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
            assert_eq!(s.forms_prototype.as_ref().unwrap().repair, marker);
            s.execute("form.prototype.close", &json!({})).unwrap();
            assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
        }
    }
    #[test]
    fn retained_match_reattaches_transactionally_and_persists_after_apply_undo_redo_and_finish() {
        let (mut s, feature, top) = retained_support_fixture(false);
        let saved = saved_form_source(&s, feature);
        let document = serde_json::to_value(s.doc.as_ref()).unwrap();
        s.execute("form.repair", &json!({"feature":feature})).unwrap();
        let edge = opening(&saved.cage);
        let preview = json!({"matching":{"body":"Support","face":top,"edge":edge,"order":0,"tolerance":0.01}});
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &preview).unwrap();
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
        // Autosave retains the accepted unresolved source, never a staged replacement.
        let draft = crate::forms_prototype::draft::Draft::capture(s.forms_prototype.as_ref().unwrap(), false).encode().unwrap();
        let (recovered, _) = crate::forms_prototype::draft::Draft::restore(&s, &draft).unwrap();
        assert_eq!(recovered.definition(), saved);
        assert!(recovered.repair.is_some());
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), saved);
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &preview).unwrap();
        s.execute("form.prototype.tool.apply", &json!({})).unwrap();
        let repaired = s.forms_prototype.as_ref().unwrap().definition();
        assert!(s.forms_prototype.as_ref().unwrap().repair.is_none());
        assert_ne!(repaired.matching, saved.matching);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), saved);
        assert!(s.forms_prototype.as_ref().unwrap().repair.is_some());
        assert!(s.execute("form.prototype.commit", &json!({})).is_err());
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().repair.is_none());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), repaired);
        s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        let encoded = serde_json::to_vec(s.doc.as_ref()).unwrap();
        let document: crate::doc::Document = serde_json::from_slice(&encoded).unwrap();
        let mut reopened = Session { doc: std::sync::Arc::new(document), ..Default::default() };
        reopened.refresh();
        reopened.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), repaired);
    }
    #[test]
    fn retained_reattachment_preview_rejects_changed_upstream_source_until_requalified() {
        let (mut s, feature, top) = retained_support_fixture(true);
        s.execute("form.repair", &json!({"feature":feature})).unwrap();
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        let preview = json!({"surface":{"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false,"reattach":{"swap":false,"reverse_u":false,"reverse_v":false}}});
        s.execute("form.prototype.tool.preview", &preview).unwrap();
        let candidate = s.forms_prototype.as_ref().unwrap().definition();
        let crate::doc::FeatureKind::Box { height, .. } = &mut s.doc_mut().features[0].kind else { panic!("Box") };
        *height = "2 mm".into();
        s.refresh();
        assert!(s.execute("form.prototype.tool.apply", &json!({})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), candidate);
        assert!(s.forms_prototype.as_ref().unwrap().repair.is_some());
        s.execute("form.prototype.tool.preview", &preview).unwrap();
        s.execute("form.prototype.tool.apply", &json!({})).unwrap();
        let repaired = s.forms_prototype.as_ref().unwrap();
        assert!(repaired.repair.is_none());
        assert!(repaired.profile.as_ref().unwrap().rest.iter().all(|p| (p.z - 1.0).abs() < 1e-9));
        for (index, height) in [(5, 0.4), (6, 0.15)] {
            assert!((repaired.cage.points[index].z - 1.0 - height).abs() < 1e-9);
        }
    }
    #[test]
    fn retained_surface_all_eight_explicit_mappings_preserve_sculpt_frames_and_cad_cell_identity() {
        use solidcore_record::Vec3;
        let (mut template, feature, top) = retained_support_fixture(true);
        // Two independent, asymmetric three-dimensional sculpt vectors on an
        // unequal grid. These are retained source data, not a geometric oracle.
        let crate::doc::FeatureKind::Form { definition, .. } = &mut template.doc_mut().feature_mut(feature).unwrap().kind else { panic!("Form") };
        let source = definition.as_mut();
        source.profile.as_mut().unwrap().partition = true;
        source.cage.points[5] += Vec3::new(0.03, -0.04, 0.0);
        source.cage.points[6] += Vec3::new(-0.02, 0.01, 0.0);
        template.refresh();
        let retained = saved_form_source(&template, feature);
        let original_document = serde_json::to_value(template.doc.as_ref()).unwrap();
        let old = retained.profile.as_ref().unwrap();
        let old_u = (old.rest[1] - old.rest[0]).normalized().unwrap();
        let old_v = (old.rest[4] - old.rest[0]).normalized().unwrap();
        let old_n = old_u.cross(old_v).normalized().unwrap();
        for bits in 0..8 {
            let swap = bits & 4 != 0;
            let reverse_u = bits & 1 != 0;
            let reverse_v = bits & 2 != 0;
            let mut s = template.scratch();
            s.execute("form.repair", &json!({"feature":feature})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), retained);
            s.execute("form.prototype.tool.begin", &json!({})).unwrap();
            s.execute("form.prototype.tool.preview",&json!({"surface":{"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":true,"reattach":{"swap":swap,"reverse_u":reverse_u,"reverse_v":reverse_v}}})).unwrap_or_else(|e|panic!("mapping {bits}: {e}"));
            let repaired = s.forms_prototype.as_ref().unwrap().definition();
            let profile = repaired.profile.as_ref().unwrap();
            assert_eq!(repaired.cage.faces, retained.cage.faces, "logical face IDs/winding stay retained");
            assert_eq!(profile.segments, [3, 2]);
            let state = match_state(&s).unwrap();
            let support = state.body("Support").unwrap();
            let face = crate::kernel::face_geometry(&support.body, top).unwrap();
            let span = crate::kernel::face_parameter_span(&support.body, top).unwrap();
            let eval = |u: f64, v: f64| {
                solidcraft_geom::tess::from_kernel(
                    face.surface.eval(span[0][0] + u * (span[0][1] - span[0][0]), span[1][0] + v * (span[1][1] - span[1][0])).0,
                )
            };
            // Native planar derivatives establish the expected physical frame
            // independently of Profile's finite-difference grid_frame helper.
            let native_u = (eval(1.0, 0.0) - eval(0.0, 0.0)).normalized().unwrap() * if reverse_u { -1.0 } else { 1.0 };
            let native_v = (eval(0.0, 1.0) - eval(0.0, 0.0)).normalized().unwrap() * if reverse_v { -1.0 } else { 1.0 };
            let (new_u, new_v) = if swap { (native_v, native_u) } else { (native_u, native_v) };
            let new_n = new_u.cross(new_v).normalized().unwrap();
            let parity = if swap ^ reverse_u ^ reverse_v { -1.0 } else { 1.0 };
            assert!((new_n.dot(old_n) - parity).abs() < 1e-9, "mapping {bits} has a known handedness");
            let boundary = old.boundary();
            for j in 0..=2 {
                for i in 0..=3 {
                    let logical = [i as f64 / 3.0, j as f64 / 2.0];
                    let mut uv = if swap { [logical[1], logical[0]] } else { logical };
                    if reverse_u {
                        uv[0] = 1.0 - uv[0];
                    }
                    if reverse_v {
                        uv[1] = 1.0 - uv[1];
                    }
                    let index = j * 4 + i;
                    let rest = eval(uv[0], uv[1]);
                    assert!(profile.rest[index].dist(rest) < 1e-9, "mapping {bits}, guide {index} native correspondence");
                    let delta = retained.cage.points[index] - old.rest[index];
                    let expected = if boundary.contains(&index) {
                        rest
                    } else {
                        rest + new_u * delta.dot(old_u) + new_v * delta.dot(old_v) + new_n * delta.dot(old_n)
                    };
                    assert!(repaired.cage.points[index].dist(expected) < 1e-9, "mapping {bits}, guide {index} physical sculpt vector");
                    assert!(((repaired.cage.points[index] - rest).len() - delta.len()).abs() < 1e-9);
                }
            }
            let fit = profile.fit(&state, &repaired.cage, &repaired.placement, 0.01).unwrap_or_else(|e| panic!("mapping {bits} fit: {e}"));
            assert!(fit.report.gap <= 0.01 && fit.report.point_gap <= 0.01);
            assert!(
                !fit.solid.is_surface() && fit.solid.shell_count() == 1 && fit.solid.validity().is_empty(),
                "mapping {bits} must sew as one valid solid"
            );
            let roots = fit.patch_roots.as_ref().unwrap_or_else(|| panic!("mapping {bits} lost a qualified rectangular CAD grid"));
            let mut sorted = roots.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..6).collect::<Vec<_>>(), "mapping {bits} retains all six logical CAD cells");
            for (native, &logical) in roots.iter().enumerate() {
                let geometry = crate::kernel::face_geometry(&fit.patch, native).unwrap();
                let corners: Vec<_> = geometry.loops.iter().flatten().map(|edge| edge.at(0.0).unwrap().0).collect();
                for &vertex in &retained.cage.faces[logical] {
                    let expected = repaired.placement.point(repaired.cage.points[vertex]);
                    assert!(
                        corners.iter().any(|point| point.dist(expected) <= 0.01),
                        "mapping {bits}, native cell {native} incorrectly names logical cell {logical}"
                    );
                }
            }
            assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), original_document, "preview must not rewrite accepted source");
            s.execute("form.prototype.tool.apply", &json!({})).unwrap();
            s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap_or_else(|e| panic!("mapping {bits} Finish: {e}"));
            assert!(s.model.result(feature).unwrap().error.is_none());
            let document: crate::doc::Document = serde_json::from_value(serde_json::to_value(s.doc.as_ref()).unwrap()).unwrap();
            let mut reopened = Session { doc: std::sync::Arc::new(document), ..Default::default() };
            reopened.refresh();
            reopened.execute("form.edit", &json!({"feature":feature})).unwrap_or_else(|e| panic!("mapping {bits} source-only reopen: {e}"));
            assert_eq!(
                reopened.forms_prototype.as_ref().unwrap().definition(),
                repaired,
                "mapping {bits} must reopen without rotating, resetting or inverting saved source"
            );
            assert_eq!(saved_form_source(&template, feature), retained);
            assert_eq!(serde_json::to_value(template.doc.as_ref()).unwrap(), original_document);
        }
    }
    #[test]
    fn retained_surface_uses_explicit_fixed_density_mapping_and_preserves_both_sculpt_offsets() {
        use solidcore_record::Vec3;
        let (mut s, feature, top) = retained_support_fixture(true);
        let saved = saved_form_source(&s, feature);
        let old_profile = saved.profile.as_ref().unwrap();
        s.execute("form.repair", &json!({"feature":feature})).unwrap();
        assert!(
            s.execute(
                "form.surface",
                &json!({"body":"Support","face":top,"segments":[3,2],"reattach":{"swap":false,"reverse_u":false,"reverse_v":false}})
            )
            .is_err()
        );
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        assert!(s.execute("form.prototype.tool.preview", &json!({"surface":{"body":"Support","face":top,"segments":[3,2]}})).is_err());
        assert!(
            s.execute(
                "form.prototype.tool.preview",
                &json!({"surface":{"body":"Support","face":top,"segments":[2,2],"reattach":{"swap":false,"reverse_u":false,"reverse_v":false}}})
            )
            .is_err()
        );
        let preview = json!({"surface":{"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false,"reattach":{"swap":true,"reverse_u":true,"reverse_v":false}}});
        s.execute("form.prototype.tool.preview", &preview).unwrap();
        let current = s.forms_prototype.as_ref().unwrap();
        let profile = current.profile.as_ref().unwrap();
        assert_eq!(current.cage.faces, saved.cage.faces);
        assert_eq!(profile.segments, [3, 2]);
        // Independently evaluate the explicitly requested swapped/reversed native chart.
        let source_state = match_state(&s).unwrap();
        let source = source_state.body("Support").unwrap();
        let geometry = crate::kernel::face_geometry(&source.body, top).unwrap();
        let span = crate::kernel::face_parameter_span(&source.body, top).unwrap();
        for j in 0..=2 {
            for i in 0..=3 {
                let uv = [1.0 - j as f64 / 2.0, i as f64 / 3.0];
                let expected = solidcraft_geom::tess::from_kernel(
                    geometry.surface.eval(span[0][0] + uv[0] * (span[0][1] - span[0][0]), span[1][0] + uv[1] * (span[1][1] - span[1][0])).0,
                );
                assert!(profile.rest[j * 4 + i].dist(expected) < 1e-9);
            }
        }
        for (index, height) in [(5, 0.4), (6, 0.15)] {
            let before = saved.cage.points[index] - old_profile.rest[index];
            let after = current.cage.points[index] - profile.rest[index];
            assert!((before.len() - height).abs() < 1e-9);
            assert!((after.len() - height).abs() < 1e-9, "sculpt length must survive explicit reattachment");
            assert!(after.dot(Vec3::Z) > 0.0, "this orientation keeps the top face normal");
        }
        let repaired = current.definition();
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), saved);
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &preview).unwrap();
        s.execute("form.prototype.tool.apply", &json!({})).unwrap();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), saved);
        assert!(s.forms_prototype.as_ref().unwrap().repair.is_some());
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), repaired);
        s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        let document: crate::doc::Document = serde_json::from_value(serde_json::to_value(s.doc.as_ref()).unwrap()).unwrap();
        let mut reopened = Session { doc: std::sync::Arc::new(document), ..Default::default() };
        reopened.refresh();
        reopened.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), repaired);
    }
    #[test]
    fn form_surface_guides_keep_the_rim_preview_cancel_and_saved_editing() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let base = s.model.state().bodies[0].body.clone();
        let top = (0..base.face_count()).find(|&i| base.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        s.execute("form.prototype.create", &json!({"size":2,"segments":[1,1,1]})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().definition();
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"surface":{"body":"Support","face":top}})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().profile.is_some());
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.surface", &json!({"body":"Support","face":top})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let original = f.control_positions();
        let ids: Vec<_> = (0..original.len()).collect();
        let positions: Vec<_> = original.iter().map(|p| [p.x, p.y, p.z + 1.0]).collect();
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":ids,"positions":positions})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!((f.cage.points[4].z - original[4].z - 1.0).abs() < 1e-9);
        assert!(f.profile.as_ref().unwrap().boundary().iter().all(|&i| f.cage.points[i] == original[i]));
        let source = f.definition();
        assert!(s.execute("form.prototype.refine", &json!({"faces":[0]})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
        let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!(!s.model.state().bodies[0].body.is_surface());
        let saved = s.model.saved(&s.doc).unwrap();
        s.model = crate::doc::Model::adopt(&s.doc, &saved).unwrap();
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage, source.cage);
        assert!(s.forms_prototype.as_ref().unwrap().profile.is_some());
        s.execute("form.prototype.close", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.model.state().bodies[0].body.same_shape(&base));
    }
    #[test]
    fn matched_open_tools_cap_for_join_and_cut_and_reopen_without_changing_the_source_cage() {
        use solidcore_record::Vec3;
        for operation in ["join", "cut"] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"corner":[-1,-1,-2],"length":2,"width":2,"height":2,"body_name":"Support","operation":"new"})).unwrap();
            let body = s.model.state().bodies[0].clone();
            let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
            s.execute("form.prototype.create", &json!({"dimensions":[2,2,2],"segments":[3,3,3],"levels":2})).unwrap();
            let cage = &s.forms_prototype.as_ref().unwrap().cage;
            let z = if operation == "join" { -1.0 } else { 1.0 };
            let faces: Vec<_> =
                cage.faces.iter().enumerate().filter(|(_, f)| f.iter().all(|&i| (cage.points[i].z - z).abs() < 1e-9)).map(|(i, _)| i).collect();
            s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":faces})).unwrap();
            if operation == "cut" {
                // The pocket leaves the source's side walls immediately. A
                // straight, coincident wall needs trimming and is tested below.
                let cage = &s.forms_prototype.as_ref().unwrap().cage;
                let vertices: Vec<_> = cage.points.iter().enumerate().filter(|(_, p)| p.z < 1.0 - 1e-9).map(|(i, _)| i).collect();
                let positions: Vec<_> = vertices
                    .iter()
                    .map(|&i| {
                        let p = cage.points[i];
                        [p.x * 0.7, p.y * 0.7, 1.0 + (p.z - 1.0) * 0.75]
                    })
                    .collect();
                s.execute("form.prototype.set_vertices", &json!({"vertices":vertices,"positions":positions})).unwrap();
            }
            let matrix = crate::doc::rigid(Vec3::Z * if operation == "join" { 1.0 } else { -1.0 }, Vec3::ZERO, Vec3::Z, 0.0);
            s.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
            let edge = opening(&s.forms_prototype.as_ref().unwrap().cage);
            s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":1,"adjacent_faces":true})).unwrap();
            let source = s.forms_prototype.as_ref().unwrap().definition();
            let mut commit = json!({"name":"Matched tool","operation":operation,"tolerance":0.01});
            if operation == "join" {
                commit["targets"] = json!(["Support"]);
            }
            let reply = s.execute("form.prototype.commit", &commit).unwrap();
            let feature = reply["feature"].as_u64().unwrap();
            assert!(s.model.result(feature).unwrap().warning.is_none(), "{operation}: {:?}", s.model.result(feature).unwrap().warning);
            let state = s.model.state();
            assert_eq!(state.bodies.len(), 1);
            let result = &state.bodies[0].body;
            assert!(!result.is_surface() && result.validity().is_empty());
            let volume = crate::kernel::measure(result).unwrap().volume;
            assert!(if operation == "join" { volume > 8.1 } else { volume > 0.0 && volume < 7.9 }, "{operation}: {volume}");
            let saved = s.model.saved(&s.doc).unwrap();
            s.model = crate::doc::Model::adopt(&s.doc, &saved).unwrap();
            let pattern =
                s.execute("solid.pattern.rectangular", &json!({"features":["Matched tool"],"dir1":[1,0,0],"count1":2,"spacing1":5})).unwrap();
            let pattern = pattern["feature"].as_u64().unwrap();
            assert!(s.model.result(pattern).unwrap().error.is_none());
            let patterned = s.model.state();
            assert_eq!(patterned.bodies.len(), 1, "Form pattern must retain the source operation");
            let patterned_volume = crate::kernel::measure(&patterned.bodies[0].body).unwrap().volume;
            let expected = if operation == "join" { 2.0 * volume - 8.0 } else { volume };
            assert!((patterned_volume - expected).abs() < 0.02, "{operation} pattern: {patterned_volume}, expected {expected}");
            s.execute("edit.undo", &json!({})).unwrap();
            s.execute("form.edit", &json!({"feature":feature})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().cage, source.cage);
            assert!(s.forms_prototype.as_ref().unwrap().matching.as_ref().unwrap().adjacent_faces);
            s.execute("form.prototype.close", &json!({})).unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc.features.len(), 1);
            assert!((crate::kernel::measure(&s.model.state().bodies[0].body).unwrap().volume - 8.0).abs() < 1e-8);
        }
    }
    #[test]
    fn a_closed_form_cut_that_misses_its_target_keeps_the_form_separately() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"corner":[40,40,40],"length":10,"width":10,"height":10,"body_name":"Stock"})).unwrap();
        session.execute("form.prototype.create", &json!({"primitive":"box","size":8,"segments":[1,1,1]})).unwrap();
        let finish = session.execute("form.prototype.commit", &json!({"operation":"cut","targets":["Stock"],"tolerance":0.05})).unwrap();
        let id = finish["feature"].as_u64().unwrap();
        let result = session.model.result(id).unwrap();
        assert!(result.warning.as_ref().is_some_and(|warning| warning.contains("Form kept separately")));
        let bodies = &result.state.bodies;
        assert_eq!(bodies.len(), 2);
        assert!(bodies.iter().any(|b| b.name == "Stock"));
        assert!(bodies.iter().any(|b| b.feature == id && b.is_closed()));
    }

    #[test]
    fn a_closed_form_cut_can_remove_its_complete_target() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"corner":[-0.5,-0.5,-0.5],"length":1,"width":1,"height":1,"body_name":"Stock"})).unwrap();
        session.execute("form.prototype.create", &json!({"primitive":"box","size":20,"segments":[1,1,1]})).unwrap();
        let finish = session.execute("form.prototype.commit", &json!({"operation":"cut","targets":["Stock"],"tolerance":0.05})).unwrap();
        let id = finish["feature"].as_u64().unwrap();
        let result = session.model.result(id).unwrap();
        assert!(result.error.is_none() && result.warning.is_none(), "{:?}", result.warning);
        assert!(result.state.bodies.is_empty());
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(session.model.state().body("Stock").is_some());
        session.execute("edit.redo", &json!({})).unwrap();
        assert!(session.model.state().bodies.is_empty());
    }

    #[test]
    fn a_closed_form_can_cut_multiple_solid_targets() {
        let mut session = Session::default();
        for (name, x) in [("Left", -3.0), ("Right", 2.0)] {
            session.execute("solid.box", &json!({"corner":[x,0.6,-12],"length":1,"width":1,"height":10,"body_name":name})).unwrap();
        }
        session.execute("form.prototype.create", &json!({"primitive":"box","size":12,"segments":[2,2,2],"levels":2})).unwrap();
        let reply = session.execute("form.prototype.commit", &json!({"operation":"cut","targets":["Left","Right"],"tolerance":0.05})).unwrap();
        let result = session.model.result(reply["feature"].as_u64().unwrap()).unwrap();
        assert!(result.error.is_none() && result.warning.is_none(), "{:?}", result.warning);
        assert_eq!(result.state.bodies.len(), 2);
        for name in ["Left", "Right"] {
            let body = result.state.body(name).unwrap();
            let volume = crate::kernel::measure(&body.body).unwrap().volume;
            assert!(body.is_closed() && (0.0..10.0).contains(&volume), "{name}: {volume}");
        }
    }

    #[test]
    fn an_import_refuses_only_invalid_names_it_would_store_for_staged_parts() {
        let mut session = Session::default();
        session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let definition = session.forms_prototype.as_ref().unwrap().definition();
        for bad in [String::new(), "x".repeat(257), "bad\0name".into()] {
            assert!(crate::forms_prototype::Form::from_import(vec![(bad.clone(), definition.clone(), 0)]).is_ok());
            let error = crate::forms_prototype::Form::from_import(vec![("Active".into(), definition.clone(), 0), (bad, definition.clone(), 0)])
                .err()
                .unwrap();
            assert!(error.to_string().contains("invalid imported Form object name"));
        }
    }

    #[test]
    fn a_matched_cut_trims_coincident_form_walls_and_conserves_volume() {
        use crate::kernel::history::BodyHistory;
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("solid.box", &json!({"corner":[-1,-1,-2],"length":2,"width":2,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let base = s.model.state().bodies[0].clone();
        let top = (0..base.body.face_count()).find(|&i| base.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        s.execute("form.prototype.create", &json!({"dimensions":[2,2,2],"segments":[3,3,3],"levels":2})).unwrap();
        let cage = &s.forms_prototype.as_ref().unwrap().cage;
        let faces: Vec<_> =
            cage.faces.iter().enumerate().filter(|(_, f)| f.iter().all(|&i| (cage.points[i].z - 1.0).abs() < 1e-9)).map(|(i, _)| i).collect();
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":faces})).unwrap();
        s.execute("form.prototype.transform", &json!({"matrix":crate::doc::rigid(-Vec3::Z,Vec3::ZERO,Vec3::Z,0.0)})).unwrap();
        let edge = opening(&s.forms_prototype.as_ref().unwrap().cage);
        s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":1,"adjacent_faces":true})).unwrap();
        let definition = s.forms_prototype.as_ref().unwrap().definition();
        let (skin, _) = crate::doc::forms::convert(&definition, 0.01).unwrap();
        let mesh = skin.mesh(&crate::doc::kernel::MeshTolerance::chord(0.01)).unwrap();
        let free: Vec<_> = mesh.edge_faces.iter().enumerate().filter_map(|(i, faces)| (faces.len() == 1).then_some(i)).collect();
        let picks: Vec<_> = crate::kernel::edge_geometry(&skin, &free).unwrap().iter().map(|curve| curve.at(0.5).unwrap().0).collect();
        let cap = crate::kernel::patch_edges_on_face(&skin, &picks, &base.body, top, 0.001).unwrap();
        let cutter = crate::kernel::stitch(&[&skin, &cap], 0.001).unwrap();
        let cutter_volume = crate::kernel::measure(&cutter).unwrap().volume;
        let trimmed =
            crate::kernel::replace_matched_face(&base.body, top, &skin, true, 0.001).unwrap().expect("coincident walls trimmed in their planes");
        assert!(!trimmed.is_surface() && trimmed.shell_count() == 1 && trimmed.validity().is_empty());
        trimmed.history().unwrap().check(&trimmed, &[&base.body, &skin]).unwrap();
        let kept = crate::kernel::measure(&trimmed).unwrap().volume;
        assert!((kept + cutter_volume - 8.0).abs() < 0.003, "exact wall trimming conserves volume: {kept} + {cutter_volume}");
        assert!((crate::kernel::measure(&base.body).unwrap().volume - 8.0).abs() < 1e-10, "the support remains untouched");
        let feature = s.execute("form.prototype.commit", &json!({"operation":"cut","tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let evaluated = s.model.result(feature).unwrap();
        assert!(evaluated.warning.is_none() && evaluated.error.is_none(), "{evaluated:?}");
        let state = s.model.state();
        assert_eq!(state.bodies.len(), 1);
        let remaining = state.body("Support").unwrap();
        assert!(!remaining.body.is_surface() && remaining.body.validity().is_empty());
        let volume = remaining.measure().unwrap().volume;
        assert!(volume > 0.0 && (volume + cutter_volume - 8.0).abs() < 0.003, "{volume} + {cutter_volume} mmÂ³");
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().matching.as_ref().unwrap().adjacent_faces);
    }
    #[test]
    fn open_or_disjoint_combinations_keep_the_base_and_editable_form_with_a_warning() {
        for primitive in ["face", "box"] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"corner":[20,20,0],"length":2,"width":2,"height":2,"body_name":"Support","operation":"new"})).unwrap();
            let before = s.model.state().bodies[0].body.clone();
            s.execute("form.prototype.create", &json!({"primitive":primitive,"size":2,"segments":[1,1,1],"levels":2})).unwrap();
            let feature = s.execute("form.prototype.commit", &json!({"operation":"join","targets":["Support"],"tolerance":0.01})).unwrap()["feature"]
                .as_u64()
                .unwrap();
            assert!(s.model.result(feature).unwrap().warning.is_some());
            let state = s.model.state();
            assert_eq!(state.bodies.len(), 2);
            assert!(state.body("Support").unwrap().body.same_shape(&before));
            s.execute("form.edit", &json!({"feature":feature})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().parameters.kind, primitive);
        }
    }
    #[test]
    fn matched_dimensions_install_the_qualified_source_and_preserve_the_fixed_seam() {
        fn fixed_seam(body: &crate::kernel::Body) {
            let edges: std::collections::BTreeSet<_> = (0..body.face_count())
                .flat_map(|face| crate::kernel::face_boundaries(body, face).unwrap().into_iter().flatten())
                .filter_map(|edge| edge.neighbor.is_none().then_some(edge.edge))
                .collect();
            assert!(!edges.is_empty(), "the matched sheet must retain its free rim");
            for edge in crate::kernel::edge_geometry(body, &edges.into_iter().collect::<Vec<_>>()).unwrap() {
                for i in 0..=8 {
                    let point = edge.at(i as f64 / 8.0).unwrap().0;
                    assert!(point.z.abs() <= 0.0011, "the CAD seam moved off the fixed support: {point:?}");
                    let side = (point.x.abs() - 2.0).abs().min((point.y.abs() - 3.0).abs());
                    assert!(
                        side <= 0.0011 && point.x.abs() <= 2.0011 && point.y.abs() <= 3.0011,
                        "the CAD seam moved off the support's rectangular rim: {point:?}"
                    );
                }
            }
        }
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":4,"width":6,"height":1,"corner":[-2,-3,-1],"operation":"new","body_name":"Support"})).unwrap();
        let support = session.model.state().bodies[0].clone();
        let top = (0..support.body.face_count()).find(|&face| support.body.planar_face(face).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
        session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":1})).unwrap();
        let centre = session.forms_prototype.as_ref().unwrap().cage.points.iter().position(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9).unwrap();
        session.execute("form.prototype.set_vertices", &json!({"vertices":[centre],"positions":[[0,0,2]]})).unwrap();
        let edge = opening(&session.forms_prototype.as_ref().unwrap().cage);
        session.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":0,"tolerance":0.01})).unwrap();
        let before = session.forms_prototype.as_ref().unwrap().definition();
        let initial_body = crate::doc::forms::convert(&before, 0.01).unwrap().0;
        fixed_seam(&initial_body);
        let height = initial_body.exact_bounds().unwrap().size().z;
        assert!(height > 0.1);
        let target = height + 1.0;
        assert!((target - before.parameters.dimensions[2]).abs() > 0.01);
        let undo = session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        session.execute("form.prototype.parameters", &json!({"dimensions":[4,6,target],"tolerance":0.01})).unwrap();
        let form = session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.inspect()["undo"], undo + 1);
        assert!(form.match_report.as_ref().unwrap().position_mm <= 0.001);
        let installed = form.definition();
        let installed_body = crate::doc::forms::convert(&installed, 0.01).unwrap().0;
        fixed_seam(&installed_body);
        assert!((installed_body.exact_bounds().unwrap().size().z - target).abs() <= 0.01);
        let state = match_state(&session).unwrap();
        let (solved, report) = installed.matching.as_ref().unwrap().recompute(&installed, &state, 0.01).unwrap();
        assert!(report.position_mm <= 0.001);
        assert!(
            solved.cage.points.iter().zip(&installed.cage.points).all(|(a, b)| a.dist(*b) <= 1e-6),
            "the installed source must already satisfy the support solve"
        );
        let solved_body = crate::doc::forms::convert(&solved, 0.01).unwrap().0;
        fixed_seam(&solved_body);
        assert!((solved_body.exact_bounds().unwrap().size().z - target).abs() <= 0.01);

        // The fixed four-millimetre rim makes a two-millimetre X extent impossible.
        let inspect = session.forms_prototype.as_ref().unwrap().inspect();
        assert!(session.execute("form.prototype.parameters", &json!({"dimensions":[2,6,target],"tolerance":0.01})).is_err());
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), installed);
        assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), inspect);
        session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), before);
        session.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), installed);
        let feature = session.execute("form.prototype.commit", &json!({"operation":"new","tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let computed = session.execute("timeline.compute_all", &json!({})).unwrap();
        assert_eq!(computed["errors"], 0);
        let state = session.model.state();
        let body = &state.bodies.iter().find(|body| body.feature == feature).unwrap().body;
        assert!((body.exact_bounds().unwrap().size().z - target).abs() <= 0.01, "saved source recomputation must retain the qualified CAD extent");
        fixed_seam(body);
    }

    #[test]
    fn associative_match_preview_cancel_history_and_upstream_recompute() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        let support = s
            .execute("solid.box", &json!({"length":4,"width":6,"height":8,"corner":[-2,-3,0],"operation":"new","body_name":"Support"}))
            .unwrap()["feature"]
            .as_u64()
            .unwrap();
        let body = s.model.state().bodies[0].clone();
        let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":2})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let edge = opening(&original.cage);
        let params = json!({"body":body.name,"face":top,"edge":edge,"order":1});
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        let reply = s.execute("form.prototype.tool.preview", &json!({"matching":params})).unwrap();
        assert!(reply["match_report"]["position_mm"].as_f64().unwrap() <= 0.001);
        assert!(s.forms_prototype.as_ref().unwrap().matching.is_some());
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
        // Face-to-wire selection retains the exact B-rep boundary identities,
        // independent of whichever display plans have already been prepared.
        let expected_edges = crate::kernel::face_boundaries(&body.body, top)
            .unwrap()
            .into_iter()
            .flatten()
            .map(|boundary| boundary.edge)
            .collect::<std::collections::BTreeSet<_>>();
        let names = crate::doc::naming::edge_names(&body);
        let expected_names = expected_edges.iter().map(|&i| names[i].clone()).collect::<std::collections::BTreeSet<_>>();
        for quality in [crate::doc::display_quality::DisplayQuality::Coarse, crate::doc::display_quality::DisplayQuality::Ultra] {
            assert!(body.plan_mesh_at(quality).is_some());
            let mut wire_params = params.clone();
            wire_params["wire_from_face"] = json!(true);
            s.execute("form.prototype.tool.begin", &json!({})).unwrap();
            s.execute("form.prototype.tool.preview", &json!({"matching":wire_params})).unwrap();
            let source = &s.forms_prototype.as_ref().unwrap().matching.as_ref().unwrap().source;
            let crate::doc::forms::SupportRef::Wire { edges, .. } = source else { panic!("face boundary did not become an exact support wire") };
            assert_eq!(edges.iter().map(|edge| edge.name.clone()).collect::<std::collections::BTreeSet<_>>(), expected_names);
            s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        }
        s.execute("form.prototype.match", &params).unwrap();
        let matched = s.forms_prototype.as_ref().unwrap().definition();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), matched);
        let feature = s.execute("form.prototype.commit", &json!({"operation":"new","tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let saved = serde_json::to_value(s.doc.as_ref()).unwrap();
        let restored: crate::doc::Document = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.feature(feature).unwrap(), s.doc.feature(feature).unwrap());
        if let crate::doc::FeatureKind::Box { length, height, .. } = &mut s.doc_mut().features.iter_mut().find(|f| f.id == support).unwrap().kind {
            *length = "8 mm".into();
            *height = "10 mm".into();
        } else {
            panic!("support is not a Box");
        }
        s.refresh();
        assert!(s.model.result(feature).unwrap().error.is_none(), "{:?}", s.model.result(feature).unwrap().error);
        let state = s.model.state();
        assert_eq!(state.bodies.len(), 2);
        let form = state.bodies.iter().find(|b| b.feature == feature).unwrap();
        let bounds = form.body.outline_bounds();
        assert!((bounds.min.x + 2.0).abs() < 0.002 && (bounds.max.x - 6.0).abs() < 0.002);
        assert!((bounds.min.z - 10.0).abs() < 0.002 && (bounds.max.z - 10.0).abs() < 0.002);
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().surface.mesh.positions.iter().all(|p| (p.z - 10.0).abs() < 0.002));
    }
    #[test]
    fn matching_refined_symmetric_form_preserves_leaf_identity_and_reopens() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":4,"width":6,"height":1,"corner":[-2,-3,-0.875],"operation":"new","body_name":"Support"})).unwrap();
        let body = s.model.state().bodies[0].clone();
        let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":2})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let h = original.hierarchy.as_ref().unwrap();
        let edge = opening(&original.cage);
        let matched = s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":1})).unwrap();
        assert!(matched["match_report"]["position_mm"].as_f64().unwrap() <= 0.001);
        assert!(matched["match_report"]["tangent_deg"].as_f64().unwrap() <= 0.1);
        let definition = s.forms_prototype.as_ref().unwrap().definition();
        let updated = definition.hierarchy.as_ref().unwrap();
        assert_eq!(updated.base, h.base);
        assert_eq!(updated.leaves, h.leaves);
        assert_eq!(updated.controls, h.controls);
        assert_eq!(updated.evaluate().unwrap(), definition.cage);
        assert_eq!(definition.symmetry, original.symmetry);
        definition.symmetry.as_ref().unwrap().validate_reflection(&definition.cage).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().logical.faces, h.layout().unwrap().faces);
        let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none(), "{:?}", s.model.result(feature).unwrap().error);
        let saved = serde_json::to_value(s.doc.as_ref()).unwrap();
        let restored: crate::doc::Document = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.feature(feature).unwrap(), s.doc.feature(feature).unwrap());
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.hierarchy.as_ref().unwrap().leaves, h.leaves);
        assert_eq!(form.hierarchy.as_ref().unwrap().controls, h.controls);
        form.symmetry.as_ref().unwrap().validate_reflection(&form.cage).unwrap();
        let ring = crate::geom::subdivision::matching::opening(&form.cage, edge).unwrap();
        assert!(ring.iter().all(|&v| (form.surface.vertices[v].z - 0.125).abs() < 0.002));
    }
    #[test]
    fn refining_a_matched_opening_rebinds_its_original_curve_coordinates() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":4,"width":6,"height":1,"corner":[-2,-3,-1],"body_name":"Support"})).unwrap();
        let body = s.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":2})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let edge = opening(&s.forms_prototype.as_ref().unwrap().cage);
        s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":1})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().definition();
        let matched = before.matching.as_ref().unwrap();
        let old_ring = crate::geom::subdivision::matching::opening(&before.cage, matched.config.edge).unwrap();
        let count = s.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        let reply = s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        assert_eq!(reply["undo"].as_u64(), Some(count + 1));
        assert!(reply["match_report"]["position_mm"].as_f64().unwrap() <= 0.001);
        let refined = s.forms_prototype.as_ref().unwrap().definition();
        let rebound = refined.matching.as_ref().unwrap();
        let new_ring = crate::geom::subdivision::matching::opening(&refined.cage, rebound.config.edge).unwrap();
        assert_eq!(new_ring.len(), old_ring.len() * 2);
        assert_eq!(rebound.crease_ancestors.len(), 1);
        for (&id, coordinate) in old_ring.iter().zip(&matched.coordinates) {
            let index = new_ring.iter().position(|&v| v == id).unwrap();
            assert_eq!(&rebound.coordinates[index], coordinate);
        }
        assert_eq!(rebound.source, matched.source);
        assert_eq!(refined.hierarchy.as_ref().unwrap().evaluate().unwrap(), refined.cage);
        refined.symmetry.as_ref().unwrap().validate_reflection(&refined.cage).unwrap();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), refined);
        let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().matching.as_ref().unwrap().coordinates, rebound.coordinates);
    }
    #[test]
    fn base_edge_extrusion_reports_the_translated_rim_for_repeated_polygon_edits() {
        for sides in [4, 5] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":1})).unwrap();
            let mut definition = s.forms_prototype.as_ref().unwrap().definition();
            definition.cage = crate::geom::subdivision::Cage {
                points: (0..sides)
                    .map(|i| {
                        let angle = std::f64::consts::TAU * i as f64 / sides as f64;
                        solidcore_record::Vec3::new(4.0 * angle.cos(), 4.0 * angle.sin(), 0.0)
                    })
                    .collect(),
                faces: vec![(0..sides).collect()],
                ..Default::default()
            };
            s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 1, None).unwrap());
            let mut edge = [0, 1];
            for step in 1..=2 {
                let before = s.forms_prototype.as_ref().unwrap().definition();
                s.execute("form.prototype.extrude_drag", &json!({"edges":[edge],"displacement":[0,0,1],"checkpoint":true})).unwrap();
                let form = s.forms_prototype.as_ref().unwrap();
                assert_eq!(form.cage.faces.len(), before.cage.faces.len() + 1);
                assert_eq!(form.extruded_edges().len(), 1);
                let rim = form.extruded_edges()[0];
                assert!(!rim.iter().any(|v| edge.contains(v)));
                assert!(rim.iter().all(|&v| (form.cage.points[v].z - step as f64).abs() < 1e-9));
                assert_eq!(form.inspect()["undo"], step);
                edge = rim;
            }
        }
    }
    fn matched_extrusion_case(order: usize, refined: bool) -> (Session, usize, usize) {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":8,"width":8,"height":1,"corner":[-4,-4,-1],"body_name":"Support"})).unwrap();
        let body = s.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":2})).unwrap();
        if refined {
            s.execute("form.prototype.refine", &json!({"faces":[5],"counts":[2,2]})).unwrap();
        }
        let form = s.forms_prototype.as_ref().unwrap();
        let ring = crate::doc::forms::match_openings(&form.cage, form.hierarchy.as_ref()).unwrap().remove(0);
        s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":[ring[0],ring[1]],"order":order})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let interior = form.logical.faces.iter().position(|f| !f.is_empty() && f.iter().all(|v| !ring.contains(v))).unwrap();
        let rim = form.logical.faces.iter().position(|f| f.iter().any(|v| ring.contains(v))).unwrap();
        (s, interior, rim)
    }
    #[test]
    fn derived_matched_extrusion_feedback_keeps_the_opening_and_never_authors_state() {
        for order in [0, 1, 2] {
            for refined in [false, true] {
                let (s, face, _) = matched_extrusion_case(order, refined);
                let form = s.forms_prototype.as_ref().unwrap();
                let before = form.inspect();
                let document = s.doc.clone();
                let recovery = crate::recovery::capture_bytes(&s).unwrap();
                let response = form.prepare_extrusion_preview(&[face], &[], solidcore_record::Vec3::Z * 0.2, true).unwrap();
                let a = response.preview(solidcore_record::Vec3::Z * 0.2).unwrap();
                let b = response.preview(solidcore_record::Vec3::Z * 0.8).unwrap();
                let middle = response.preview(solidcore_record::Vec3::Z * 0.5).unwrap();
                assert_ne!(a.mesh.positions, b.mesh.positions);
                assert_eq!(a.mesh.triangles, b.mesh.triangles);
                for ((a, b), middle) in a.mesh.positions.iter().zip(&b.mesh.positions).zip(&middle.mesh.positions) {
                    assert!(((*a + *b) * 0.5).dist(*middle) < 1e-7);
                }
                assert!(b.mesh.normals.iter().all(|normal| normal.is_finite()));
                let target = solidcore_record::Vec3::new(0.12, -0.09, 0.8);
                let extrapolated = response.preview(target).unwrap();
                let direct = form.prepare_extrusion_preview(&[face], &[], target, true).unwrap().preview(target).unwrap();
                for (actual, expected) in extrapolated.mesh.positions.iter().zip(&direct.mesh.positions) {
                    assert!(actual.dist(*expected) < 1e-7, "feedback must agree with an independent application of the shared topology operation");
                }
                let ring = crate::doc::forms::match_openings(&form.cage, form.hierarchy.as_ref()).unwrap().remove(0);
                let mut seams = 0;
                for (edge, path) in form.surface.boundaries.iter().filter(|(edge, _)| ring.contains(&edge[0]) && ring.contains(&edge[1])) {
                    let current = b.boundaries.iter().find(|(other, _)| other == edge).unwrap();
                    assert_eq!(&current.1, path, "a pending extrusion cannot move its accepted support opening");
                    seams += 1;
                }
                assert!(seams > 0);
                assert!(response.preview(solidcore_record::Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
                assert_eq!(form.inspect(), before);
                assert_eq!(s.doc, document);
                assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
            }
        }
    }
    #[test]
    fn extruding_matched_interior_faces_preserves_the_seam_and_one_drag_checkpoint() {
        for order in [0, 1, 2] {
            let (mut s, interior, rim) = matched_extrusion_case(order, false);
            let before = s.forms_prototype.as_ref().unwrap().definition();
            let depth = s.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
            let mut numeric = s.scratch();
            for (checkpoint, z) in [(true, 0.2), (false, 1.0)] {
                let reply =
                    s.execute("form.prototype.extrude_drag", &json!({"faces":[interior],"displacement":[0,0,z],"checkpoint":checkpoint})).unwrap();
                assert_eq!(reply["undo"], depth + 1);
                assert!(reply["match_report"]["position_mm"].as_f64().unwrap() <= 0.001);
                if order > 0 {
                    assert!(reply["match_report"]["tangent_deg"].as_f64().unwrap() <= 0.1);
                }
                if order == 2 {
                    assert!(reply["match_report"]["curvature_relative"].as_f64().unwrap() <= 0.01);
                }
            }
            let after = s.forms_prototype.as_ref().unwrap().definition();
            assert_eq!(after.cage.faces.len(), before.cage.faces.len() + 4);
            assert!(after.cage.points.iter().any(|p| p.z > 0.5), "the cap must retain the extrusion");
            assert_eq!(after.matching.as_ref().unwrap().coordinates, before.matching.as_ref().unwrap().coordinates);
            assert_eq!(after.matching.as_ref().unwrap().source, before.matching.as_ref().unwrap().source);
            assert_eq!(after.hierarchy.as_ref().unwrap().evaluate().unwrap(), after.cage);
            let accepted = s.forms_prototype.as_ref().unwrap().inspect();
            assert!(s.execute("form.prototype.extrude_drag", &json!({"faces":[rim],"displacement":[0,0,2],"checkpoint":false})).is_err());
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), accepted);
            numeric.execute("form.prototype.topology", &json!({"tool":"extrude_faces","faces":[interior],"displacement":[0,0,1]})).unwrap();
            assert_eq!(numeric.forms_prototype.as_ref().unwrap().definition(), after);
            assert!(numeric.execute("form.prototype.extrude_drag", &json!({"faces":[interior],"displacement":[0,0,2],"checkpoint":false})).is_err());

            let recovered = crate::recovery::restore_bytes(&crate::recovery::capture_bytes(&s).unwrap(), None).unwrap();
            assert_eq!(recovered.forms_prototype.as_ref().unwrap().definition(), after);
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
            let state = match_state(&s).unwrap();
            let (definition, report) = after.matching.as_ref().unwrap().recompute(&after, &state, 0.01).unwrap();
            s.forms_prototype.as_mut().unwrap().refresh_match_preview(definition, report).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], depth + 1, "release qualification belongs to the drag");
        }
    }
    #[test]
    fn matched_extrusion_rejects_opening_changes_atomically_and_keeps_its_source_after_reopen() {
        use solidcore_record::Vec3;
        let (mut s, interior, rim) = matched_extrusion_case(1, true);
        let before = s.forms_prototype.as_ref().unwrap().definition();
        let old = s.forms_prototype.as_ref().unwrap().inspect();
        let edge = before.matching.as_ref().unwrap().config.edge;
        for params in [
            json!({"tool":"extrude_faces","faces":[rim],"displacement":[0,0,1]}),
            json!({"tool":"extrude_edges","edges":[edge],"displacement":[0,0,1]}),
        ] {
            assert!(s.execute("form.prototype.topology", &params).is_err());
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), old);
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        }
        assert!(s.execute("form.prototype.extrude_drag", &json!({"faces":[rim],"displacement":[0,0,1]})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), old);
        s.execute("form.prototype.topology", &json!({"tool":"extrude_faces","faces":[interior],"displacement":[0,0,0.5]})).unwrap();
        let after = s.forms_prototype.as_ref().unwrap().definition();
        let saved_hierarchy = serde_json::to_value(after.hierarchy.as_ref().unwrap()).unwrap();
        let operation = &saved_hierarchy["extrusions"][0];
        let cells: Vec<_> = operation["cells"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group.as_array().unwrap())
            .map(|cell| cell.as_u64().unwrap() as usize)
            .collect();
        let displacement = Vec3::Z * 0.5;
        let replay = solidcraft_geom::subdivision::stable_extrusion::faces(&before.cage, &cells, displacement).unwrap();
        assert_eq!(replay.cage.faces, after.cage.faces);
        assert!(replay.cap.iter().any(|(source, cap)| source != cap), "the fixture must have new cap controls");
        for (&source, &cap) in &replay.cap {
            assert!(
                after.cage.points[cap].dist(before.cage.points[source] + displacement) <= 1e-6,
                "the original extrusion must retain its displacement"
            );
        }
        let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let source_definition = |session: &Session| match &session.doc.feature(feature).unwrap().kind {
            crate::doc::FeatureKind::Form { definition, .. } => definition.as_ref().clone(),
            _ => panic!("expected the persisted Form source"),
        };
        let saved_source = source_definition(&s);
        assert!(s.model.result(feature).unwrap().error.is_none());
        assert_eq!(s.model.state().bodies.len(), 2);
        let package = s.design_container(solidcraft_io::DesignCompression::Fast, false, false).unwrap();
        let mut reopened = Session::new(solidcraft_io::read_design(&package).unwrap());
        assert_eq!(source_definition(&reopened), saved_source, "source reopening must be exact");
        reopened.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(source_definition(&reopened), saved_source, "resolving the edit must not rewrite persisted source");
        let restored = reopened.forms_prototype.as_ref().unwrap();
        assert_eq!(restored.matching.as_ref().unwrap().coordinates, before.matching.as_ref().unwrap().coordinates);
        assert_eq!(serde_json::to_value(restored.hierarchy.as_ref().unwrap()).unwrap()["extrusions"], saved_hierarchy["extrusions"]);
        assert_eq!(restored.hierarchy.as_ref().unwrap().leaves, after.hierarchy.as_ref().unwrap().leaves);
        assert!(reopened.execute("form.prototype.extrude_drag", &json!({"faces":[interior],"displacement":[0,0,2],"checkpoint":false})).is_err());
        reopened.execute("form.prototype.close", &json!({})).unwrap();

        if let crate::doc::FeatureKind::Box { height, .. } = &mut reopened.doc_mut().features[0].kind {
            *height = "1.5 mm".into();
        } else {
            panic!("expected Box support");
        }
        reopened.refresh();
        assert!(reopened.model.result(feature).unwrap().error.is_none());
        assert_eq!(source_definition(&reopened), saved_source, "support refresh must not rewrite persisted source");
        reopened.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(source_definition(&reopened), saved_source, "opening a resolved edit must leave its authored source exact");
        let form = reopened.forms_prototype.as_ref().unwrap();
        let ring = crate::doc::forms::match_openings(&form.cage, form.hierarchy.as_ref()).unwrap().remove(0);
        assert!(ring.iter().all(|&v| (form.surface.vertices[v].z - 0.5).abs() <= 0.001));
        assert_eq!(form.matching.as_ref().unwrap().coordinates, before.matching.as_ref().unwrap().coordinates);
        let resolved_hierarchy = serde_json::to_value(form.hierarchy.as_ref().unwrap()).unwrap();
        assert_eq!(resolved_hierarchy["extrusions"].as_array().unwrap().len(), saved_hierarchy["extrusions"].as_array().unwrap().len());
        for (resolved, authored) in resolved_hierarchy["extrusions"].as_array().unwrap().iter().zip(saved_hierarchy["extrusions"].as_array().unwrap())
        {
            for key in ["level", "cells", "edges", "first_vertex", "delta", "face_offsets", "edge_offsets"] {
                assert_eq!(resolved[key], authored[key], "resolved operation descriptor {key}");
            }
        }
        assert_eq!(form.hierarchy.as_ref().unwrap().leaves, after.hierarchy.as_ref().unwrap().leaves);
        assert_eq!(form.hierarchy.as_ref().unwrap().evaluate().unwrap(), form.cage);
        for (&source, &cap) in &replay.cap {
            assert!(form.cage.points[cap].dist(after.cage.points[cap]) <= 1e-6, "the support solve must retain unaffected free cap controls");
            if source != cap {
                assert!(
                    (form.cage.points[cap] - form.cage.points[source]).dist(after.cage.points[cap] - after.cage.points[source]) <= 1e-6,
                    "the wall must retain the authored source-to-cap displacement"
                );
            }
        }
        let state = match_state(&reopened).unwrap();
        let resolved = form.definition();
        let mut repeated = resolved.clone();
        for _ in 0..3 {
            let (next, report) = repeated.matching.as_ref().unwrap().recompute(&repeated, &state, 0.01).unwrap();
            assert!(report.position_mm <= 0.001 && report.tangent_deg <= 0.1);
            assert_eq!(next.cage.faces, resolved.cage.faces);
            assert_eq!(next.cage.points.len(), resolved.cage.points.len());
            for (actual, expected) in next.cage.points.iter().zip(&resolved.cage.points) {
                assert!(actual.dist(*expected) <= 1e-6, "repeated support recomputation must not accumulate geometric drift");
            }
            assert_eq!(next.hierarchy.as_ref().unwrap().evaluate().unwrap(), next.cage);
            repeated = next;
        }
        assert_eq!(source_definition(&reopened), saved_source, "pure recomputation must preserve the full persisted authoring source");
    }
    #[test]
    fn matching_a_trimmed_opening_keeps_the_hidden_subdivision_stencil() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":2,"width":2,"height":1,"corner":[-1,-1,-1],"operation":"new","body_name":"Support"})).unwrap();
        let body = s.model.state().bodies[0].clone();
        let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[6,6,1],"segments":[3,3,1],"levels":2})).unwrap();
        let cage = &s.forms_prototype.as_ref().unwrap().cage;
        let center = cage.faces.iter().position(|f| f.iter().all(|&v| cage.points[v].x.abs() < 1.01 && cage.points[v].y.abs() < 1.01)).unwrap();
        let hidden = (0..cage.faces.len()).filter(|&f| f != center).collect::<Vec<_>>();
        s.execute("form.prototype.refine", &json!({"faces":[center],"counts":[2,2]})).unwrap();
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":hidden})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let h = original.hierarchy.as_ref().unwrap();
        let rings = crate::doc::forms::match_openings(&original.cage, Some(h)).unwrap();
        assert_eq!(rings.len(), 1);
        let edge = [rings[0][0], rings[0][1]];
        assert!(crate::geom::subdivision::matching::opening(&original.cage, edge).is_err(), "the selected edge is an evaluator interior");
        for order in [0, 2] {
            let reply = s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":order})).unwrap();
            assert!(reply["match_report"]["position_mm"].as_f64().unwrap() <= 0.001);
            if order == 2 {
                assert!(reply["match_report"]["tangent_deg"].as_f64().unwrap() <= 0.1);
                assert!(reply["match_report"]["curvature_relative"].as_f64().unwrap() <= 0.01);
            }
            let matched = s.forms_prototype.as_ref().unwrap().definition();
            assert_eq!(matched.hierarchy.as_ref().unwrap().leaves, h.leaves);
            assert_eq!(matched.cage.faces, original.cage.faces);
            let (cad, report) = crate::doc::forms::convert(&matched, 0.01).unwrap();
            assert!(cad.is_surface());
            let bounds = cad.outline_bounds();
            assert!(bounds.min.dist(Vec3::new(-1.0, -1.0, 0.0)) < 0.002);
            assert!(bounds.max.dist(Vec3::new(1.0, 1.0, 0.0)) < 0.002);
            assert!(report.patch_roots.iter().all(|&id| h.leaves[id].active));
            s.execute("form.prototype.undo", &json!({})).unwrap();
        }
    }
    #[test]
    fn matching_respects_mirrored_controls_and_rejects_an_incompatible_support() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        for (name, x) in [("Support", -2), ("Offset", -1)] {
            s.execute("solid.box", &json!({"length":4,"width":6,"height":8,"corner":[x,-3,0],"operation":"new","body_name":name})).unwrap();
        }
        let state = s.model.state();
        let top = |name: &str| {
            let body = &state.body(name).unwrap().body;
            (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap()
        };
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":2})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let edge = opening(&original.cage);
        let reply = s.execute("form.prototype.match", &json!({"body":"Support","face":top("Support"),"edge":edge,"order":1})).unwrap();
        assert!(reply["match_report"]["position_mm"].as_f64().unwrap() <= 0.001);
        assert!(reply["match_report"]["tangent_deg"].as_f64().unwrap() <= 0.1);
        let matched = s.forms_prototype.as_ref().unwrap().definition();
        assert_eq!(matched.symmetry, original.symmetry);
        matched.symmetry.as_ref().unwrap().validate_reflection(&matched.cage).unwrap();
        assert!(s.execute("form.prototype.match", &json!({"body":"Offset","face":top("Offset"),"edge":edge,"order":1})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), matched);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), matched);
        let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let reopened = s.forms_prototype.as_ref().unwrap();
        assert_eq!(reopened.symmetry, matched.symmetry);
        reopened.symmetry.as_ref().unwrap().validate_reflection(&reopened.cage).unwrap();
        assert!(reopened.surface.mesh.positions.iter().all(|p| (p.z - 8.0).abs() < 0.002));
    }
    #[test]
    fn matching_support_picks_and_recomputation_use_the_authoring_component_frame() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        let support = s
            .execute("solid.box", &json!({"length":4,"width":6,"height":8,"corner":[78,17,0],"body_name":"Support","operation":"new"}))
            .unwrap()["feature"]
            .as_u64()
            .unwrap();
        let body = s.model.state().bodies[0].clone();
        let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let created = s.execute("component.create", &json!({"name":"Matched panel","grounded":false})).unwrap();
        let component = created["component"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence":created["occurrence"],"translate":[80,20,5],"angle":90,"axis":[0,0,1],"capture":true}))
            .unwrap();
        let frame = s.doc.component_transform(component);
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[6,4,1],"segments":[2,2,2],"levels":2})).unwrap();
        s.execute("form.prototype.transform", &json!({"matrix":frame})).unwrap();
        let edge = opening(&s.forms_prototype.as_ref().unwrap().cage);
        s.execute("form.prototype.match", &json!({"body":body.name,"face":top,"edge":edge,"order":1})).unwrap();
        let feature = s.execute("form.prototype.commit", &json!({"operation":"new","tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert_eq!(s.doc.feature(feature).unwrap().component, component);
        if let crate::doc::FeatureKind::Box { height, .. } = &mut s.doc_mut().features.iter_mut().find(|f| f.id == support).unwrap().kind {
            *height = "10 mm".into();
        } else {
            panic!("expected Box");
        }
        s.refresh();
        assert!(s.model.result(feature).unwrap().error.is_none(), "{:?}", s.model.result(feature).unwrap().error);
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert!(form.surface.mesh.positions.iter().all(|p| (p.z - 10.0).abs() < 0.002));
        assert!(form.surface.mesh.positions.iter().all(|p| p.x >= 77.998 && p.x <= 82.002 && p.y >= 16.998 && p.y <= 23.002));
    }
    fn opening(cage: &crate::geom::subdivision::Cage) -> [usize; 2] {
        let ring = cage.boundary_loops().unwrap().remove(0);
        [ring[0], ring[1]]
    }
    fn seam(cage: &crate::geom::subdivision::Cage) -> Vec<[usize; 2]> {
        let f = &cage.faces[0];
        f.iter().copied().zip(f.iter().copied().cycle().skip(1)).take(f.len()).map(|(a, b)| [a, b]).collect()
    }
    #[test]
    fn refined_collapse_uses_the_normal_tool_and_keeps_qualified_edits_atomic() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"plane","size":8,"spans":1,"levels":2})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,1]})).unwrap();
        s.execute("form.prototype.finish_options", &json!({"tolerance":"0.05 mm"})).unwrap();
        let source = s.forms_prototype.as_ref().unwrap().definition();
        let initial = source.hierarchy.as_ref().unwrap().layout().unwrap();
        let leaf = &initial.faces[1];
        let picks = [leaf[3], leaf[0]];
        let before = s.doc.to_json();
        let params = json!({"topology":{"tool":"weld","vertices":picks,"tolerance":0.05}});
        for apply in [false, true] {
            s.execute("form.prototype.tool.begin", &json!({})).unwrap();
            s.execute("form.prototype.tool.preview", &params).unwrap();
            let edited = s.forms_prototype.as_ref().unwrap();
            let site = edited.hierarchy.as_ref().unwrap().validated_boundary_collapse().unwrap().unwrap();
            assert_eq!(edited.logical.faces[site.face], site.boundary);
            assert_eq!(edited.logical.faces[site.retained_neighbor], initial.faces[site.retained_neighbor]);
            assert_eq!(edited.cage, source.cage);
            assert!(edited.selectable_control(site.point));
            assert!(picks.iter().all(|&id| !edited.selectable_control(id)));
            assert_eq!(s.doc.to_json(), before);
            let command = if apply { "form.prototype.tool.apply" } else { "form.prototype.tool.cancel" };
            s.execute(command, &json!({})).unwrap();
            if !apply {
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
            }
        }
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let neighbor = leaf[1];
        let local = form.cage.points[neighbor];
        let old = form.definition();
        let target = local + Vec3::Z * 0.5;
        let params = json!({"vertices":[neighbor],"positions":[[target.x,target.y,target.z]]});
        assert!(s.execute("form.prototype.set_vertices", &params).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), old);
        let prepared = s.prepare_form_transform(crate::forms_prototype::FormTargetSpace::Control, params).unwrap();
        assert!(prepared.compute().is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), old);
        let before_refusal = s.forms_prototype.as_ref().unwrap().inspect();
        let projection = json!({
            "tool":"flatten","points":"control","vertices":[neighbor,leaf[2]],
            "direction":[0,0,1],"origin":[0,0,0.5]
        });
        assert!(s.execute("form.prototype.project", &projection).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before_refusal);
        let target = local + Vec3::X * 0.125;
        let params = json!({"vertices":[neighbor],"positions":[[target.x,target.y,target.z]]});
        let prepared = s.prepare_form_transform(crate::forms_prototype::FormTargetSpace::Control, params).unwrap().compute().unwrap();
        assert!(s.accept_form_transform(prepared).unwrap().0);
        assert_ne!(s.forms_prototype.as_ref().unwrap().definition(), old);

        let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        assert_eq!(s.model.state().bodies[0].body.edge_count(), 6);
        let saved = s.doc.to_json();
        let mut reopened = Session::new(crate::doc::Document::from_json(&saved).unwrap());
        reopened.refresh();
        reopened.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().logical.faces[1].len(), 3);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), saved);
    }

    #[test]
    fn refined_unweld_previews_two_exact_skins_and_finishes_one_independent_split() {
        use crate::doc::forms::{Definition, Placement};
        use crate::doc::{Document, FeatureKind};
        use crate::geom::subdivision::{Cage, hierarchy::Hierarchy, primitives::Parameters};
        use solidcore_record::Vec3;
        let mut points = Vec::new();
        for z in [-5.0, 0.0, 5.0] {
            for (x, y) in [(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)] {
                points.push(Vec3::new(x, y, z));
            }
        }
        let mut faces = Vec::new();
        for ring in [0, 4] {
            for i in 0..4 {
                let a = ring + i;
                let b = ring + (i + 1) % 4;
                faces.push(vec![a, b, b + 4, a + 4]);
            }
        }
        faces.extend([vec![3, 2, 1, 0], vec![8, 9, 10, 11]]);
        let mut h =
            Hierarchy::new(Cage { points, faces, ..Default::default() }).unwrap().subdivide(&[8], [2, 2]).unwrap().subdivide(&[9], [4, 4]).unwrap();
        let cage = h.evaluate().unwrap();
        let centers: Vec<_> =
            h.controls.iter().filter(|&(&i, &birth)| birth == 1 && cage.points[i].x == 0. && cage.points[i].y == 0.).map(|(&id, _)| id).collect();
        assert_eq!(centers.len(), 2);
        h.add_deltas(&centers, &[Vec3::Z * 0.25, Vec3::Z * 0.35]).unwrap();
        let source = Definition {
            cage: h.evaluate().unwrap(),
            hierarchy: Some(h.clone()),
            placement: Placement { origin: Vec3::new(2., -3., 4.), ..Default::default() },
            parameters: Parameters { kind: "box".into(), dimensions: [8., 8., 10.], ..Default::default() },
            matching: None,
            pipe: None,
            profile: None,
            imported: None,
            symmetry: None,
        };

        let mut s = Session::new(Document::new("Refined Unweld"));
        s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(source, 2, None).unwrap());
        let first = s.execute("form.prototype.commit", &json!({"name":"Original","tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!(!s.model.state().bodies[0].body.is_surface());
        let original = s.doc.to_json();
        s.execute("form.edit", &json!({"feature":first})).unwrap();
        let source = s.forms_prototype.as_ref().unwrap().definition();
        let edges = [[4, 5], [5, 6], [6, 7], [7, 4]];
        assert!(s.execute("form.prototype.unweld", &json!({"edges":edges,"separate":false})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
        let preview = json!({"unweld":{"edges":edges,"name":"Second half"}});
        for apply in [false, true] {
            s.execute("form.prototype.tool.begin", &json!({})).unwrap();
            s.execute("form.prototype.tool.preview", &preview).unwrap();
            let edited = s.forms_prototype.as_ref().unwrap();
            assert_eq!(edited.cage, source.cage);
            assert!(edited.hierarchy.as_ref().unwrap().has_detached_material());
            assert_eq!(edited.parts.len(), 1);
            let other = edited.parts.values().next().unwrap();
            assert_eq!(other.definition.cage, source.cage);
            assert!(other.definition.hierarchy.as_ref().unwrap().has_detached_material());
            assert_eq!(s.doc.to_json(), original);
            let command = if apply { "form.prototype.tool.apply" } else { "form.prototype.tool.cancel" };
            s.execute(command, &json!({})).unwrap();
            if !apply {
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
                assert!(s.forms_prototype.as_ref().unwrap().parts.is_empty());
            }
        }
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source);
        assert!(s.forms_prototype.as_ref().unwrap().parts.is_empty());
        s.execute("form.prototype.redo", &json!({})).unwrap();
        let second = s.execute("form.prototype.commit", &json!({})).unwrap()["parts"][0].as_u64().unwrap();
        assert_ne!(first, second);
        let completed = s.doc.to_json();
        let reopened = Document::from_json(&completed).unwrap();
        let mut reopened = Session::new(reopened);
        reopened.refresh();
        let st = reopened.model.state();
        assert_eq!(st.bodies.len(), 2);
        assert!(st.bodies.iter().all(|body| body.body.is_surface() && crate::doc::kernel::validity(&body.body).ok()));
        for (id, count) in [(first, 8), (second, 20)] {
            let FeatureKind::Form { definition, tolerance, .. } = &reopened.doc.feature(id).unwrap().kind else { panic!("saved Form part") };
            assert_eq!(tolerance.as_str(), "0.05 mm");
            let half = definition.hierarchy.as_ref().unwrap();
            assert_eq!(half.details, source.hierarchy.as_ref().unwrap().details);
            assert_eq!(half.layout().unwrap().faces.iter().filter(|face| !face.is_empty()).count(), count);
            assert_eq!(st.bodies.iter().find(|body| body.feature == id).unwrap().body.face_count(), count);
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), original);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), completed);
    }

    #[test]
    fn unweld_and_draft_weld_stage_objects_and_restore_complete_editor_history() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let edges = seam(&original.cage);
        let document = serde_json::to_value(s.doc.as_ref()).unwrap();
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"unweld":{"edges":edges}})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().parts.len(), 1);
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert!(s.forms_prototype.as_ref().unwrap().parts.is_empty());
        s.execute("form.prototype.unweld", &json!({"edges":edges})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let part = *f.parts.keys().next().unwrap();
        let first = opening(&f.cage);
        let second = opening(&f.parts[&part].definition.cage);
        assert_eq!(f.inspect()["undo"], 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert!(s.forms_prototype.as_ref().unwrap().parts.is_empty());
        s.execute("form.prototype.redo", &json!({})).unwrap();
        s.execute("form.prototype.weld", &json!({"other":{"kind":"draft","part":part},"first_edge":first,"second_edge":second})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.cage, original.cage);
        assert!(f.parts.is_empty());
        assert_eq!(f.inspect()["undo"], 2);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().parts.len(), 1);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage, original.cage);
        s.execute("form.prototype.close", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
    }
    fn refined_weld_fixture(offset: f64) -> (Session, [u64; 2]) {
        use crate::doc::forms::{Definition, Placement};
        use crate::geom::subdivision::{Cage, hierarchy::Hierarchy, primitives::Parameters};
        use solidcore_record::Vec3;
        let mut s = Session::new(crate::doc::Document::new("Refined Weld"));
        let ids = [true, false].map(|lower| {
            let mut points = Vec::new();
            for z in [-5.0, 0.0] {
                for (x, y) in [(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)] {
                    points.push(Vec3::new(x, y, z));
                }
            }
            let mut faces = (0..4)
                .map(|a| {
                    let b = (a + 1) % 4;
                    vec![a, b, b + 4, a + 4]
                })
                .collect::<Vec<_>>();
            faces.push(if lower { vec![3, 2, 1, 0] } else { vec![4, 5, 6, 7] });
            let depth = if lower { 2 } else { 4 };
            let mut hierarchy = Hierarchy::new(Cage { points, faces, ..Default::default() }).unwrap().subdivide(&[4], [depth, depth]).unwrap();
            let cage = hierarchy.evaluate().unwrap();
            let center = hierarchy
                .controls
                .iter()
                .find(|&(&id, &level)| {
                    let p = cage.points[id];
                    level == 1 && p.x == 0.0 && p.y == 0.0
                })
                .map(|(&id, _)| id)
                .unwrap();
            hierarchy.add_deltas(&[center], &[Vec3::Z * 0.25]).unwrap();
            assert_ne!(hierarchy.evaluate().unwrap(), cage);
            let definition = Definition {
                placement: Placement { origin: if lower { Vec3::ZERO } else { Vec3::Z * (5.0 + offset) }, ..Default::default() },
                cage: hierarchy.evaluate().unwrap(),
                hierarchy: Some(hierarchy),
                parameters: Parameters { kind: "box".into(), dimensions: [8.0, 8.0, 5.0], ..Default::default() },
                symmetry: None,
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
            };
            s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 2, None).unwrap());
            s.execute("form.prototype.commit", &json!({"name": if lower {"Lower"} else {"Upper"}, "tolerance": 0.05})).unwrap()["feature"]
                .as_u64()
                .unwrap()
        });
        assert_eq!(s.model.state().bodies.len(), 2);
        (s, ids)
    }

    #[test]
    fn refined_weld_retains_both_finished_sources_through_save_edit_remove_and_undo() {
        use crate::doc::{FeatureKind, naming::named_face_point};
        let (mut s, [a, b]) = refined_weld_fixture(0.0);
        let before = s.doc.to_json();
        let lower = s.model.state().bodies.iter().find(|body| body.feature == a).unwrap().name.clone();
        let names = [a, b].map(|id| format!("F{id}:FormPatch1"));
        for name in &names {
            assert!(named_face_point(&s.model.state(), None, name).is_ok(), "{name}");
        }
        let args = json!({"features":[a,b],"name":"Seam"});
        let feature = s.execute("form.refined_weld", &args).unwrap()["feature"].as_u64().unwrap();
        let seam = s.doc.feature(feature).unwrap();
        let FeatureKind::Stitch { form_seam: Some(record), .. } = &seam.kind else { panic!("saved seam") };
        assert_eq!(record.sources(), [crate::doc::FeatureId(a), crate::doc::FeatureId(b)]);
        assert!(seam.kind.names_feature(a, "Lower") && seam.kind.names_feature(b, "Upper"));
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!(!s.model.state().body(&lower).unwrap().body.is_surface());
        for name in &names {
            assert!(named_face_point(&s.model.state(), Some(&lower), name).is_ok(), "{name}");
        }
        let welded = s.doc.to_json();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), welded);

        let mut reopened = Session::new(crate::doc::Document::from_json(&welded).unwrap());
        reopened.refresh();
        assert_eq!(reopened.model.state().bodies.len(), 1);
        for name in &names {
            assert!(named_face_point(&reopened.model.state(), Some(&lower), name).is_ok(), "{name}");
        }
        reopened.execute("form.edit", &json!({"feature":a})).unwrap();
        let form = reopened.forms_prototype.as_ref().unwrap();
        let hierarchy = form.hierarchy.as_ref().unwrap();
        let id = hierarchy.details[1][0].vertex;
        let point = form.cage.points[id];
        reopened.execute("form.prototype.set_vertices", &json!({"vertices":[id],"positions":[[point.x,point.y,point.z+0.01]]})).unwrap();
        reopened.execute("form.prototype.commit", &json!({})).unwrap();
        assert!(reopened.forms_prototype.is_none());
        assert_eq!(reopened.model.state().bodies.len(), 1);
        let edited = reopened.doc.to_json();
        assert_ne!(edited, welded);
        reopened.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(reopened.doc.to_json(), welded);
        reopened.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(reopened.doc.to_json(), edited);

        reopened.execute("form.refined_weld.remove", &json!({"feature":feature})).unwrap();
        assert!(reopened.doc.feature(feature).is_none());
        assert!(reopened.doc.feature(a).is_some() && reopened.doc.feature(b).is_some());
        assert_eq!(reopened.model.state().bodies.len(), 2);
        reopened.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(reopened.doc.to_json(), edited);
        reopened.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(reopened.model.state().bodies.len(), 2);
        let next = reopened.execute("form.refined_weld", &args).unwrap()["feature"].as_u64().unwrap();
        assert_ne!(next, feature, "retired seam identity cannot name a later seam");
    }

    #[test]
    fn refined_weld_refuses_disconnected_changed_or_consumed_inputs_atomically() {
        use solidcore_record::Vec3;
        let (mut disconnected, ids) = refined_weld_fixture(0.01);
        let args = json!({"features":ids,"edges":[[4,5],[0,1]]});
        let before = disconnected.doc.to_json();
        let auto_error = disconnected.execute("form.refined_weld", &json!({"features":ids})).unwrap_err().to_string();
        assert!(auto_error.contains("rims are not aligned"), "{auto_error}");
        let error = disconnected.execute("form.refined_weld", &args).unwrap_err().to_string();
        assert!(error.contains("do not coincide"), "{error}");
        assert_eq!(disconnected.doc.to_json(), before);
        assert_eq!(disconnected.model.state().bodies.len(), 2);

        let (mut s, [a, b]) = refined_weld_fixture(0.0);
        let args = json!({"features":[a,b],"edges":[[4,5],[0,1]]});
        let donor = s.model.state().bodies.iter().find(|body| body.feature == b).unwrap().name.clone();
        s.execute("solid.move", &json!({"bodies":[donor],"translate":[0,0,0.01]})).unwrap();
        let moved = s.doc.to_json();
        let error = s.execute("form.refined_weld", &args).unwrap_err().to_string();
        assert!(error.contains("source body changed"), "{error}");
        assert_eq!(s.doc.to_json(), moved);
        s.execute("edit.undo", &json!({})).unwrap();
        let joined = s.execute("form.refined_weld", &args).unwrap()["feature"].as_u64().unwrap();
        for rolled_back in [false, true] {
            if rolled_back {
                let before_seam = s.doc.feature_index(joined).unwrap();
                s.execute("timeline.rollback", &json!({"position":before_seam})).unwrap();
                assert_eq!(s.model.state().bodies.len(), 2);
            }
            let before = s.doc.to_json();
            s.execute("form.edit", &json!({"feature":b})).unwrap();
            let placement = crate::doc::rigid(Vec3::Z * 0.01, Vec3::ZERO, Vec3::Z, 0.0);
            s.execute("form.prototype.transform", &json!({"matrix":placement})).unwrap();
            let attempted = s.forms_prototype.as_ref().unwrap().definition();
            let error = s.execute("form.prototype.commit", &json!({})).unwrap_err().to_string();
            assert!(error.contains("would break") && error.contains("do not coincide"), "{error}");
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), attempted);
            s.execute("form.prototype.close", &json!({})).unwrap();
        }
        s.execute("timeline.rollback", &json!({"position":"end"})).unwrap();
        assert_eq!(s.model.state().bodies.len(), 1);

        let result = s.model.state().bodies.first().unwrap().name.clone();
        s.execute("solid.move", &json!({"bodies":[result],"translate":[1,0,0]})).unwrap();
        let consumed = s.doc.to_json();
        let error = s.execute("form.refined_weld.remove", &json!({"feature":joined})).unwrap_err().to_string();
        assert!(error.contains("dependency"), "{error}");
        assert_eq!(s.doc.to_json(), consumed);
    }

    #[test]
    fn saved_object_weld_cancel_preserves_incoming_links_and_finish_is_one_document_action() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
        let edges = seam(&s.forms_prototype.as_ref().unwrap().cage);
        s.execute("form.prototype.unweld", &json!({"edges":edges,"name":"Second half"})).unwrap();
        let finished = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        let first = finished["feature"].as_u64().unwrap();
        let second = finished["parts"][0].as_u64().unwrap();
        assert_eq!(s.doc.features.len(), 2);
        assert_eq!(s.model.state().bodies.len(), 2);
        assert!(s.model.state().bodies.iter().all(|b| b.body.is_surface()));
        let child_body = s.model.result(second).unwrap().state.bodies.iter().find(|b| b.feature == second).unwrap().name.clone();
        s.execute("solid.move", &json!({"bodies":[child_body],"translate":[1,0,0]})).unwrap();
        let first_body = s.model.result(first).unwrap().state.bodies.iter().find(|b| b.feature == first).unwrap().name.clone();
        s.execute("solid.scale", &json!({"bodies":[child_body],"factor":1.25})).unwrap();
        s.execute("solid.move", &json!({"bodies":[first_body,child_body],"translate":[0,1,0]})).unwrap();
        let children: Vec<_> = s.doc.features.iter().skip(2).map(|f| f.id).collect();
        assert_eq!(children.len(), 3);
        let document = serde_json::to_value(s.doc.as_ref()).unwrap();
        s.execute("form.edit", &json!({"feature":first})).unwrap();
        let edge_a = opening(&s.forms_prototype.as_ref().unwrap().cage);
        let crate::doc::FeatureKind::Form { definition, .. } = &s.doc.feature(second).unwrap().kind else { panic!("expected Form") };
        let edge_b = opening(&definition.cage);
        let params = json!({"other":{"kind":"feature","feature":second},"first_edge":edge_a,"second_edge":edge_b});
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"weld":params})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().consumes(second));
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert!(!s.forms_prototype.as_ref().unwrap().consumes(second));
        s.execute("form.prototype.weld", &params).unwrap();
        s.execute("form.prototype.close", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
        assert_eq!(s.doc.features.len(), 5);
        s.execute("form.edit", &json!({"feature":first})).unwrap();
        s.execute("form.prototype.weld", &params).unwrap();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert!(!s.forms_prototype.as_ref().unwrap().consumes(second));
        s.execute("form.prototype.redo", &json!({})).unwrap();
        s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        assert_eq!(s.doc.features.len(), 4);
        assert!(s.doc.feature(first).is_some() && s.doc.feature(second).is_none());
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!(!s.model.state().bodies[0].body.is_surface());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.features.len(), 5);
        assert!(s.doc.feature(second).is_some());
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.features.len(), 4);
        for &child in &children {
            assert_eq!(s.doc.feature(child).unwrap().kind.input_bodies(), vec![first_body.clone()]);
            assert!(s.model.result(child).unwrap().error.is_none());
        }
        let encoded = serde_json::to_vec(s.doc.as_ref()).unwrap();
        let restored: crate::doc::Document = serde_json::from_slice(&encoded).unwrap();
        let mut reopened = Session { doc: std::sync::Arc::new(restored), ..Default::default() };
        reopened.refresh();
        for &child in &children {
            assert_eq!(reopened.doc.feature(child).unwrap().kind.input_bodies(), vec![first_body.clone()]);
            assert!(reopened.model.result(child).unwrap().error.is_none());
        }
        assert_eq!(reopened.model.state().bodies.len(), 1);
        assert!(reopened.model.state().bodies[0].body.validity().is_empty());
    }
    #[test]
    fn weld_refuses_to_merge_distinct_body_pattern_inputs() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
        let edges = seam(&s.forms_prototype.as_ref().unwrap().cage);
        s.execute("form.prototype.unweld", &json!({"edges":edges})).unwrap();
        let finished = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        let first = finished["feature"].as_u64().unwrap();
        let second = finished["parts"][0].as_u64().unwrap();
        let state = s.model.state();
        let names = [first, second].map(|id| state.bodies.iter().find(|body| body.feature == id).unwrap().name.clone());
        s.execute("solid.pattern.rectangular", &json!({"bodies":names,"dir1":[1,0,0],"count1":2,"spacing1":20})).unwrap();
        s.execute("form.edit", &json!({"feature":first})).unwrap();
        let a = opening(&s.forms_prototype.as_ref().unwrap().cage);
        let crate::doc::FeatureKind::Form { definition, .. } = &s.doc.feature(second).unwrap().kind else { panic!("expected donor Form") };
        let b = opening(&definition.cage);
        s.execute("form.prototype.weld", &json!({"other":{"kind":"feature","feature":second},"first_edge":a,"second_edge":b})).unwrap();
        let document = s.doc.to_json();
        let draft = s.forms_prototype.as_ref().unwrap().inspect();
        let error = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap_err().to_string();
        assert!(error.contains("merge separate copy sources"), "{error}");
        assert_eq!(s.doc.to_json(), document);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), draft);
    }

    #[test]
    fn weld_refuses_body_position_and_named_vertex_consumers_without_changing_source_or_draft() {
        for vertex in [false, true] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
            let edges = seam(&s.forms_prototype.as_ref().unwrap().cage);
            s.execute("form.prototype.unweld", &json!({"edges":edges})).unwrap();
            let finished = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
            let first = finished["feature"].as_u64().unwrap();
            let second = finished["parts"][0].as_u64().unwrap();
            let body = s.model.result(second).unwrap().state.bodies.iter().find(|body| body.feature == second).unwrap().name.clone();
            let faces = s.execute("model.faces", &json!({"body":body})).unwrap();
            let point = faces["faces"][0]["point"].clone();
            let edges = s.execute("model.edges", &json!({"body":body})).unwrap();
            let edge = edges["edges"][0]["name"].as_str().unwrap().to_owned();
            let child = s.execute("face.delete", &json!({"body":body,"faces":[point],"heal":false})).unwrap()["feature"].as_u64().unwrap();
            assert!(s.model.result(child).unwrap().error.is_none());
            let feature = s.doc_mut().feature_mut(child).unwrap();
            feature.face_names.clear(); // The legal positional schema of older designs.
            if vertex {
                let at: solidcore_record::Vec3 = serde_json::from_value(point).unwrap();
                feature.kind = crate::doc::FeatureKind::ConstructionPoint {
                    def: crate::doc::construct::PointDef::At { point: crate::doc::construct::GeoRef::Vertex { at, edges: Some(vec![edge]) } },
                };
            }
            s.refresh();
            assert!(s.model.result(child).unwrap().error.is_none());
            s.execute("form.edit", &json!({"feature":first})).unwrap();
            let a = opening(&s.forms_prototype.as_ref().unwrap().cage);
            let crate::doc::FeatureKind::Form { definition, .. } = &s.doc.feature(second).unwrap().kind else { panic!("expected donor Form") };
            let b = opening(&definition.cage);
            s.execute("form.prototype.weld", &json!({"other":{"kind":"feature","feature":second},"first_edge":a,"second_edge":b})).unwrap();
            let document = serde_json::to_value(s.doc.as_ref()).unwrap();
            let draft = s.forms_prototype.as_ref().unwrap().inspect();
            let error = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap_err().to_string();
            assert!(error.contains("topology reference"), "{error}");
            assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), document);
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), draft);
            assert!(s.model.result(child).unwrap().error.is_none());
        }
    }
    #[test]
    fn whole_rigid_edits_keep_local_controls_and_partial_edits_use_world_coordinates() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let transform = crate::doc::rigid(Vec3::new(30.0, -10.0, 7.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
        let indices: Vec<_> = (0..original.cage.points.len()).collect();
        let targets: Vec<_> = original
            .cage
            .points
            .iter()
            .map(|&p| {
                let p = crate::doc::apply_point(&transform, p);
                [p.x, p.y, p.z]
            })
            .collect();
        s.execute("form.prototype.set_vertices", &json!({"vertices":indices,"positions":targets})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.cage, original.cage);
        assert!(!f.placement.is_identity());
        assert_eq!(f.inspect()["undo"], 1);
        for (p, q) in f.control_positions().iter().zip(&targets) {
            assert!(p.dist(Vec3::new(q[0], q[1], q[2])) < 1e-10);
        }
        let placed = f.definition();
        let target = f.surface.vertices[0] + Vec3::new(0.2, -0.1, 0.3);
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[0],"positions":[[target.x,target.y,target.z]]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!(f.surface.vertices[0].dist(target) < 1e-9);
        assert_eq!(f.placement, placed.placement);
        assert_ne!(f.cage, original.cage);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), placed);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), placed);
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let mut invalid = crate::doc::IDENTITY;
        invalid[0][0] = 2.0;
        assert!(s.execute("form.prototype.transform", &json!({"matrix":invalid})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
    }
    #[test]
    fn surface_pulling_uses_authored_work_budget_independently_of_cached_preview_density() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"levels":1})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().clone();
        let indices: Vec<_> = (0..8).collect();
        let targets: Vec<_> = indices
            .iter()
            .map(|&i| {
                let point = original.surface.vertices[i];
                [point.x, point.y, point.z + 0.1]
            })
            .collect();
        let args = json!({"vertices":indices,"positions":targets});
        s.execute("form.prototype.set_surface_vertices", &args).unwrap();
        let expected = s.forms_prototype.as_ref().unwrap().definition();
        let mut dense = original.clone();
        let triangle = dense.surface.mesh.triangles[0];
        let face = dense.surface.mesh.tri_face[0];
        // Duplicate coincident display triangles without changing authored
        // geometry or the solve. The old display-count gate refused this edit.
        let surface = std::sync::Arc::make_mut(&mut dense.surface);
        surface.mesh.triangles.resize(500_001, triangle);
        surface.mesh.tri_face.resize(500_001, face);
        s.forms_prototype = Some(dense);
        s.execute("form.prototype.set_surface_vertices", &args).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), expected);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original.definition());
        s.execute("form.prototype.create", &json!({"segments":[4,4,4],"levels":4})).unwrap();
        let expensive = s.forms_prototype.as_mut().unwrap();
        let source = expensive.definition();
        std::sync::Arc::make_mut(&mut expensive.surface).mesh.triangles.clear();
        let indices: Vec<_> = (0..85).collect();
        let targets: Vec<_> = indices
            .iter()
            .map(|&i| {
                let point = expensive.surface.vertices[i];
                [point.x, point.y, point.z + 0.1]
            })
            .collect();
        let error = s.execute("form.prototype.set_surface_vertices", &json!({"vertices":indices,"positions":targets})).unwrap_err();
        assert!(error.to_string().contains("surface solve budget"), "{error}");
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), source, "an empty display cache cannot bypass the evaluator work limit");
    }
    #[test]
    fn whole_scaling_preserves_large_hierarchies_details_and_one_gesture_history() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"levels":1})).unwrap();
        let faces: Vec<_> = (0..24).collect();
        s.execute("form.prototype.refine", &json!({"faces":faces,"counts":[4,4]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let vertex = *f.hierarchy.as_ref().unwrap().controls.iter().find(|(_, level)| **level == 2).unwrap().0;
        let p = f.surface.vertices[vertex] + Vec3::Z * 0.3;
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let original = f.definition();
        let old_undo = f.inspect()["undo"].as_u64().unwrap();
        let indices: Vec<_> = f.logical.faces.iter().flatten().copied().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        assert!(indices.len() > 256);
        let pivot = Vec3::new(8.0, -4.0, 3.0);
        let affine = |p: Vec3| {
            let q = p - pivot;
            pivot + Vec3::new(1.3 * q.x, 0.8 * q.y, 1.1 * q.z) + Vec3::new(4.0, 3.0, -2.0)
        };
        let original_world = f.control_positions();
        let targets: Vec<_> = indices.iter().map(|&i| affine(f.surface.vertices[i])).map(|p| [p.x, p.y, p.z]).collect();
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":indices,"positions":targets})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.hierarchy.as_ref().unwrap().leaves, original.hierarchy.as_ref().unwrap().leaves);
        assert_eq!(f.hierarchy.as_ref().unwrap().controls, original.hierarchy.as_ref().unwrap().controls);
        for (p, q) in f.control_positions().iter().zip(original_world) {
            assert!(p.dist(affine(q)) < 1e-8);
        }
        for (&i, p) in indices.iter().zip(&targets) {
            assert!(f.surface.vertices[i].dist(Vec3::new(p[0], p[1], p[2])) < 1e-8);
        }
        let targets: Vec<_> = indices.iter().map(|&i| f.surface.vertices[i] * 1.1).map(|p| [p.x, p.y, p.z]).collect();
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":indices,"positions":targets,"checkpoint":false})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], old_undo + 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        let scaled = s.forms_prototype.as_ref().unwrap().definition();
        let decoded: crate::doc::forms::Definition = serde_json::from_str(&serde_json::to_string(&scaled).unwrap()).unwrap();
        assert_eq!(decoded, scaled);
        assert_eq!(scaled.hierarchy.as_ref().unwrap().evaluate().unwrap(), scaled.cage);
    }
    #[test]
    fn whole_scale_handles_planar_forms_and_keeps_mirror_partners() {
        use solidcore_record::Vec3;
        for primitive in ["face", "box"] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"primitive":primitive,"segments":[1,1,1],"levels":1})).unwrap();
            s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let original = f.definition();
            let indices: Vec<_> = (0..f.cage.points.len()).collect();
            let world = f.control_positions();
            let targets: Vec<_> = world.iter().map(|p| *p * 1.6 + Vec3::new(10.0, -8.0, 2.0)).map(|p| [p.x, p.y, p.z]).collect();
            s.execute("form.prototype.set_vertices", &json!({"vertices":indices,"positions":targets})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            assert_eq!(f.symmetry.as_ref().unwrap().partners, original.symmetry.unwrap().partners);
            for (i, &j) in f.symmetry.as_ref().unwrap().partners.iter().enumerate() {
                assert!(f.symmetry.as_ref().unwrap().reflect(f.cage.points[i]).dist(f.cage.points[j]) < 1e-8);
            }
            for (p, q) in f.control_positions().iter().zip(&targets) {
                assert!(p.dist(Vec3::new(q[0], q[1], q[2])) < 1e-8);
            }
        }
    }
    #[test]
    fn a_form_in_a_placed_component_reopens_without_baking_or_double_placement() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        let created = s.execute("component.create", &json!({"name":"Placed Form","grounded":false})).unwrap();
        let component = created["component"].as_u64().unwrap();
        let occurrence = created["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence":occurrence,"translate":[80,20,5],"angle":90,"axis":[0,0,1],"capture":true})).unwrap();
        let frame = s.doc.component_transform(component);
        s.execute("form.prototype.create", &json!({"dimensions":[10,10,10],"segments":[1,1,1],"levels":1})).unwrap();
        let cage = s.forms_prototype.as_ref().unwrap().cage.clone();
        s.execute("form.prototype.transform", &json!({"matrix":frame})).unwrap();
        let world = s.forms_prototype.as_ref().unwrap().control_positions();
        let finished = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        let feature = finished["feature"].as_u64().unwrap();
        let f = s.doc.feature(feature).unwrap();
        assert_eq!(f.component, component);
        let crate::doc::FeatureKind::Form { definition, .. } = &f.kind else { panic!("expected Form") };
        assert_eq!(definition.cage, cage);
        assert!(definition.placement.matrix().iter().flatten().zip(crate::doc::IDENTITY.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-10));
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        for (a, b) in f.control_positions().iter().zip(&world) {
            assert!(a.dist(*b) < 1e-9);
        }
        let target = world[0] + Vec3::X * 0.25;
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[target.x,target.y,target.z]]})).unwrap();
        s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!(f.control_positions()[0].dist(target) < 1e-9);
        for (i, p) in f.control_positions().iter().enumerate().skip(1) {
            assert!(p.dist(world[i]) < 1e-9);
        }
    }
    #[test]
    fn rigid_surface_motion_preserves_hierarchical_details_and_local_symmetry() {
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,1]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let original = f.definition();
        let indices: Vec<_> = f.logical.faces.iter().flatten().copied().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        let transform = crate::doc::rigid(Vec3::new(20.0, 8.0, 3.0), Vec3::ZERO, Vec3::Y, 0.6);
        let positions: Vec<_> = indices
            .iter()
            .map(|&i| {
                let p = crate::doc::apply_point(&transform, f.surface.vertices[i]);
                [p.x, p.y, p.z]
            })
            .collect();
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":indices,"positions":positions})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.cage, original.cage);
        assert_eq!(f.hierarchy, original.hierarchy);
        assert_eq!(f.symmetry, original.symmetry);
        assert!(!f.placement.is_identity());
        for (&i, p) in indices.iter().zip(positions) {
            assert!(f.surface.vertices[i].dist(Vec3::new(p[0], p[1], p[2])) < 1e-9);
        }
        let vertex = *f.hierarchy.as_ref().unwrap().controls.iter().find(|(_, level)| **level == 1).unwrap().0;
        let target = f.surface.vertices[vertex] + f.placement.vector(Vec3::Z * 0.15);
        let placement = f.placement.clone();
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[vertex],"positions":[[target.x,target.y,target.z]]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!(f.surface.vertices[vertex].dist(target) < 1e-8);
        assert_eq!(f.placement, placement);
        assert_ne!(f.hierarchy, original.hierarchy);
        f.symmetry.as_ref().unwrap().validate(&f.cage).unwrap();
    }
    #[test]
    fn preview_updates_start_from_the_original_and_cancel_restores_redo_history() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","levels":2})).unwrap();
        let p = s.forms_prototype.as_ref().unwrap().cage.points[0];
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z+1.0]]})).unwrap();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["redo"], 1);
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        for distance in [1.0, 3.0] {
            s.execute("form.prototype.tool.preview", &json!({"topology":{"tool":"thicken","distance":distance,"sharp":true}})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            assert_eq!(f.cage.points.len(), original.cage.points.len() * 2);
            assert_eq!(f.inspect()["undo"], 1);
            assert_eq!(f.cage.points.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max), distance);
        }
        let good = s.forms_prototype.as_ref().unwrap().inspect();
        assert!(s.execute("form.prototype.tool.preview", &json!({"topology":{"tool":"thicken","distance":0.0}})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), good);
        assert!(s.execute("form.prototype.commit", &json!({})).is_err());
        assert!(s.execute("form.prototype.create", &json!({})).is_err());
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.definition(), original);
        assert_eq!((f.inspect()["undo"].as_u64(), f.inspect()["redo"].as_u64()), (Some(0), Some(1)));
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"topology":{"tool":"thicken","distance":2.0}})).unwrap();
        s.execute("form.prototype.tool.apply", &json!({})).unwrap();
        assert!(!s.forms_prototype.as_ref().unwrap().has_tool_preview());
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert!(s.doc.features.is_empty());
    }
    #[test]
    fn computed_form_actions_adopt_the_edit_and_cannot_overwrite_a_newer_form() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face"})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let prepared =
            s.scratch().compute_commands(&[("form.prototype.set_vertices".into(), json!({"vertices":[0],"positions":[[-21,-20,1]]}))], None).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        s.accept_commands(prepared).unwrap();
        assert_ne!(s.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], 1);
        let stale =
            s.scratch().compute_commands(&[("form.prototype.set_vertices".into(), json!({"vertices":[0],"positions":[[-22,-20,2]]}))], None).unwrap();
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-23,-20,3]]})).unwrap();
        let current = s.forms_prototype.as_ref().unwrap().inspect();
        assert!(!stale.is_current(&s));
        assert!(s.accept_commands(stale).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), current);
    }
    #[test]
    fn bridge_materializes_local_details_and_undo_restores_the_hierarchy() {
        use crate::geom::subdivision::{hierarchy::Hierarchy, primitives::Parameters};
        let parameters = Parameters { kind: "face".into(), segments: [1; 3], dimensions: [10.0; 3], ..Default::default() };
        let mut cage = parameters.build().unwrap();
        for f in &mut cage.faces {
            f.reverse();
        }
        let other = Parameters { segments: [2; 3], ..parameters.clone() }.build().unwrap();
        let offset = cage.points.len();
        cage.points.extend(other.points.iter().map(|p| *p + solidcore_record::Vec3::Z * 4.0));
        cage.faces.extend(other.faces.iter().map(|f| f.iter().map(|v| v + offset).collect()));
        let mut h = Hierarchy::new(cage).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let control = *h.controls.iter().find(|(_, level)| **level == 1).unwrap().0;
        h.add_deltas(&[control], &[solidcore_record::Vec3::Z * 0.2]).unwrap();
        let (material, remap) = h.materialize().unwrap();
        let loops = material.boundary_loops().unwrap();
        let seeds: Vec<_> = loops.iter().map(|l| *remap.iter().find(|(_, new)| **new == l[0]).unwrap().0).collect();
        let definition = crate::doc::forms::Definition {
            placement: Default::default(),
            cage: h.evaluate().unwrap(),
            parameters,
            symmetry: None,
            hierarchy: Some(h),
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
        };
        let mut s = Session {
            forms_prototype: Some(crate::forms_prototype::Form::from_definition(definition.clone(), 1, None).unwrap()),
            ..Default::default()
        };
        s.execute("form.prototype.topology", &json!({"tool":"bridge","vertices":seeds})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!(f.hierarchy.is_none());
        assert_eq!(f.cage.points, material.points);
        assert!(f.cage.boundary_loops().unwrap().is_empty());
        assert_eq!(f.inspect()["undo"], 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), definition);
    }
    #[test]
    fn enabling_symmetry_refuses_unpaired_creases_without_changing_history() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"box","segments":[1,1,1],"levels":0})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let edge = form.cage.edges().unwrap().into_iter().find(|edge| edge.iter().all(|&id| form.cage.points[id].x > 0.0)).unwrap();
        s.execute("form.prototype.topology", &json!({"tool":"crease","edges":[edge],"distance":2.0})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let error = s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap_err();
        assert!(error.to_string().contains("creases must be mirrored"));
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
    }

    #[test]
    fn mirrored_local_refinement_pulls_deletion_and_base_creases_preserve_symmetry() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"levels":2})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,1]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.logical.faces.iter().filter(|f| !f.is_empty()).count(), 26);
        let h = f.hierarchy.as_ref().unwrap();
        let id = *h.controls.iter().find(|(id, level)| **level == 1 && f.cage.points[**id].x > 1.0).unwrap().0;
        let partner = f.symmetry.as_ref().unwrap().partners[id];
        let target = f.surface.vertices[id] + solidcore_record::Vec3::new(0.4, 0.2, 0.3);
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[id],"positions":[[target.x,target.y,target.z]]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!(f.surface.vertices[id].dist(target) < 1e-8);
        assert!(f.surface.vertices[partner].dist(f.symmetry.as_ref().unwrap().reflect(target)) < 1e-8);
        let before = f.definition();
        let edge = f.hierarchy.as_ref().unwrap().base.edges().unwrap()[0];
        s.execute("form.prototype.topology", &json!({"tool":"crease","edges":[edge],"distance":1.5})).unwrap();
        assert!(!s.forms_prototype.as_ref().unwrap().hierarchy.as_ref().unwrap().base.edge_creases.is_empty());
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        let f = s.forms_prototype.as_ref().unwrap();
        let face = f.logical.faces.iter().position(|f| !f.is_empty()).unwrap();
        let deleted = f.mirror_face_selection(&[face]).unwrap().len();
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[face]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.logical.faces.iter().filter(|f| !f.is_empty()).count(), 26 - deleted);
        let saved = f.definition();
        assert!(crate::forms_prototype::Form::from_definition(saved, 2, None).is_ok());
        let snapshot = f.inspect();
        let local = f.hierarchy.as_ref().unwrap().dissolved_edges.len();
        assert_eq!(local, 0);
        assert!(s.execute("form.prototype.topology", &json!({"tool":"crease","edges":[[id,partner]],"distance":2.0})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), snapshot);
    }

    #[test]
    fn refined_fill_restores_a_placed_mirrored_sculpt_as_one_undoable_cad_solid() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"box","dimensions":[12,10,8],"segments":[1,1,1],"levels":0})).unwrap();
        s.execute("form.prototype.finish_options", &json!({"tolerance":"0.05 mm"})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":2})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[4,4]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let h = f.hierarchy.as_ref().unwrap();
        let removed: Vec<_> = h
            .leaves
            .iter()
            .enumerate()
            .filter_map(|(id, leaf)| {
                let [_, _, x, y] = leaf.domain;
                (leaf.active && leaf.root == 0 && x <= 8 && y <= 8).then_some(id)
            })
            .collect();
        assert_eq!(removed.len(), 4);
        let centre = h.leaf_corners().unwrap()[removed[0]].unwrap()[2];
        assert_eq!(h.controls.get(&centre), Some(&2));
        let target = f.control_positions()[centre] + Vec3::Z * 0.125;
        s.execute("form.prototype.set_vertices", &json!({"vertices":[centre],"positions":[[target.x,target.y,target.z]]})).unwrap();
        let transform = crate::doc::rigid(Vec3::new(12.0, -4.0, 9.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.7);
        s.execute("form.prototype.transform", &json!({"matrix":transform})).unwrap();
        let intact = s.forms_prototype.as_ref().unwrap().definition();
        assert!(intact.hierarchy.as_ref().unwrap().details[2].iter().any(|detail| detail.vertex == centre && detail.delta.len() > 0.1));
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":removed})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let hole = f.definition();
        let h = hole.hierarchy.as_ref().unwrap();
        assert!(!h.controls.contains_key(&centre));
        assert_eq!(hole.cage, intact.cage);
        assert_eq!(h.details, intact.hierarchy.as_ref().unwrap().details);
        let rims = crate::doc::forms::match_openings(&hole.cage, Some(h)).unwrap();
        assert_eq!(rims.len(), 2);
        let rim = rims.iter().find(|rim| rim.iter().all(|&v| hole.cage.points[v].z > 0.0)).unwrap();
        assert_eq!(rim.len(), 8);
        let rim_edges: std::collections::BTreeSet<_> =
            rim.iter().copied().zip(rim.iter().copied().cycle().skip(1)).take(rim.len()).map(|(a, b)| (a.min(b), a.max(b))).collect();
        let edge = h
            .layout()
            .unwrap()
            .paths
            .into_iter()
            .find(|(_, path)| path.windows(2).all(|pair| rim_edges.contains(&(pair[0].min(pair[1]), pair[0].max(pair[1])))))
            .unwrap()
            .0;
        let edge = [edge[1], edge[0]];
        let undo = f.inspect()["undo"].as_u64().unwrap();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        for mode in ["single", "minimal", "reduced_star"] {
            assert!(s.execute("form.prototype.topology", &json!({"tool":"fill_hole","edges":[edge],"fill_mode":mode})).is_err());
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
        }
        s.execute("form.prototype.topology", &json!({"tool":"fill_hole","edges":[edge]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.definition(), intact, "restored leaf IDs, sculpt, mirror and placement are the original source");
        assert_eq!(f.inspect()["undo"].as_u64(), Some(undo + 1));
        let (body, report) = crate::doc::forms::convert(&f.definition(), 0.05).unwrap();
        assert!(report.solid && !body.is_mesh() && !body.is_surface());
        assert!(body.validity().is_empty());
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), hole);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), intact);
        let saved = crate::recovery::restore_bytes(&crate::recovery::capture_bytes(&s).unwrap(), None).unwrap();
        assert_eq!(saved.forms_prototype.as_ref().unwrap().definition(), intact);
    }
    #[test]
    fn fresh_refined_cap_preserves_restore_policy_and_commits_one_tool_checkpoint() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":0})).unwrap();
        let edge: [usize; 2] = s.forms_prototype.as_ref().unwrap().logical.faces[10][..2].try_into().unwrap();
        let new_cap = json!({"tool":"fill_hole","edges":[edge],"fill_mode":"new_cap","tolerance":0.05});
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[10]})).unwrap();
        let base = s.forms_prototype.as_ref().unwrap().definition();
        assert!(s.execute("form.prototype.topology", &new_cap).unwrap_err().to_string().contains("requires a refined Form"));
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), base);
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let face = before.hierarchy.as_ref().unwrap().leaves.len();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let document = s.doc.clone();
        let automatic = s.execute("form.prototype.topology", &json!({"tool":"fill_hole","edges":[edge]})).unwrap_err().to_string();
        assert!(automatic.contains("original free boundary") && automatic.contains("New cap"), "{automatic}");
        let invalid = json!({"tool":"fill_hole","edges":[edge,[edge[1],edge[0]]],"fill_mode":"new_cap"});
        for request in [
            invalid.clone(),
            json!({"tool":"fill_hole","vertices":[usize::MAX],"fill_mode":"new_cap"}),
            json!({"tool":"fill_hole","edges":[edge],"faces":[0],"fill_mode":"new_cap"}),
        ] {
            assert!(s.execute("form.prototype.topology", &request).is_err());
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
        }

        let mut notched = s.scratch();
        notched.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[1]})).unwrap();
        let notch = notched.forms_prototype.as_ref().unwrap().definition();
        let h = notch.hierarchy.as_ref().unwrap();
        let rings = crate::doc::forms::match_openings(&notch.cage, Some(h)).unwrap();
        let outer = rings.iter().max_by_key(|rim| rim.len()).unwrap();
        let vertex = *outer.iter().find(|id| h.controls.contains_key(id)).unwrap();
        let unchanged = crate::recovery::capture_bytes(&notched).unwrap();
        let error = notched.execute("form.prototype.topology", &json!({"tool":"fill_hole","vertices":[vertex]})).unwrap_err().to_string();
        assert!(error.contains("mix original free edges and deleted material"), "{error}");
        assert!(!error.contains("Choose New cap"));
        assert_eq!(crate::recovery::capture_bytes(&notched).unwrap(), unchanged);

        let mut mirrored = Session::default();
        mirrored.execute("form.prototype.create", &json!({"primitive":"face","segments":[3,3,1],"levels":0})).unwrap();
        mirrored.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        mirrored.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let unchanged = crate::recovery::capture_bytes(&mirrored).unwrap();
        let error = mirrored.execute("form.prototype.topology", &new_cap).unwrap_err().to_string();
        assert!(error.contains("without Match or symmetry"), "{error}");
        assert_eq!(crate::recovery::capture_bytes(&mirrored).unwrap(), unchanged);

        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        assert!(s.execute("form.prototype.tool.preview", &json!({"topology":invalid})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        s.execute("form.prototype.tool.preview", &json!({"topology":new_cap})).unwrap();
        let preview = s.forms_prototype.as_ref().unwrap();
        let authored = preview.definition();
        assert_eq!(authored.cage, before.cage);
        assert_eq!(authored.hierarchy.as_ref().unwrap().quad_cap_face(), Some(face));
        assert_eq!(preview.logical.faces[face].len(), 4);
        assert_eq!(preview.inspect()["undo"], undo + 1);
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        assert_eq!(s.doc.as_ref(), document.as_ref());

        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &json!({"topology":new_cap})).unwrap();
        s.execute("form.prototype.tool.apply", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), authored);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], undo + 1);
        assert_eq!(s.doc.as_ref(), document.as_ref());
        let accepted = crate::recovery::capture_bytes(&s).unwrap();
        for (command, request) in [
            ("form.prototype.topology", json!({"tool":"delete_faces","faces":[face,1]})),
            ("form.prototype.topology", json!({"tool":"bridge","edges":[edge]})),
            ("form.prototype.symmetry", json!({"axis":0})),
            ("form.prototype.extrude_drag", json!({"faces":[face],"displacement":[0,0,1]})),
        ] {
            assert!(s.execute(command, &request).is_err(), "{command}");
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), accepted, "{command}");
        }
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), authored);
        let id = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        let saved = solidcraft_io::write_design(s.doc.as_ref());
        let mut reopened = Session::new(solidcraft_io::read_design(&saved).unwrap());
        assert!(reopened.model.result(id).unwrap().error.is_none());
        reopened.execute("form.edit", &json!({"feature":id})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), authored);
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().logical.faces[face].len(), 4);

        let old_name = format!("F{id}:FormPatch{}", face + 1);
        let old = reopened.model.state();
        let cap_index = crate::doc::naming::face_names(&old.bodies[0]).iter().position(|name| name == &old_name).unwrap();
        assert!(crate::doc::naming::named_face_point(&old, None, &old_name).is_ok());
        let ordinal = format!("F{id}:FormPiece{}", cap_index + 1);
        assert!(crate::doc::naming::named_face_point(&old, None, &ordinal).is_err());
        let snapshot = crate::recovery::capture_bytes(&reopened).unwrap();
        let undo = reopened.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        for invalid in [
            json!({"tool":"delete_faces","faces":[face,1],"tolerance":0.05}),
            json!({"tool":"delete_faces","faces":[face],"edges":[edge],"tolerance":0.05}),
            json!({"tool":"delete_faces","faces":[face],"tolerance":0.0}),
        ] {
            assert!(reopened.execute("form.prototype.topology", &invalid).is_err());
            assert_eq!(crate::recovery::capture_bytes(&reopened).unwrap(), snapshot);
        }
        let delete = json!({"tool":"delete_faces","faces":[face],"tolerance":0.05});
        reopened.execute("form.prototype.topology", &delete).unwrap();
        let form = reopened.forms_prototype.as_ref().unwrap();
        let removed = form.definition();
        let hierarchy = removed.hierarchy.as_ref().unwrap();
        assert_eq!(hierarchy.quad_cap_face(), None);
        assert!(hierarchy.has_quad_cap_history());
        assert_eq!(form.inspect()["undo"], undo + 1);
        assert!(form.logical.faces[face].is_empty());
        assert_eq!(removed.cage, authored.cage);
        assert_eq!(removed.placement, authored.placement);
        assert_eq!(hierarchy.controls, authored.hierarchy.as_ref().unwrap().controls);
        assert_eq!(hierarchy.details, authored.hierarchy.as_ref().unwrap().details);
        let (_, old_rim) = authored.hierarchy.as_ref().unwrap().validated_quad_cap().unwrap().unwrap();
        assert_eq!(hierarchy.quad_cap_rim(&[edge], &[]).unwrap(), old_rim);
        assert!(reopened.execute("form.prototype.topology", &delete).is_err());
        assert!(reopened.execute("form.prototype.topology", &json!({"tool":"bridge","edges":[edge]})).is_err());
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), removed);
        reopened.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), authored);
        reopened.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), removed);
        assert_eq!(reopened.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"], id);
        let opened_skin = reopened.model.state();
        assert!(opened_skin.bodies[0].body.is_surface());
        assert!(crate::doc::naming::named_face_point(&opened_skin, None, &old_name).is_err());

        let bytes = solidcraft_io::write_design(reopened.doc.as_ref());
        let mut reopened = Session::new(solidcraft_io::read_design(&bytes).unwrap());
        reopened.execute("form.edit", &json!({"feature":id})).unwrap();
        assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), removed);
        reopened.execute("form.prototype.refine", &json!({"faces":[3],"counts":[4,4]})).unwrap();
        let next = reopened.forms_prototype.as_ref().unwrap().hierarchy.as_ref().unwrap().leaves.len();
        assert!(next > face + 1);
        reopened.execute("form.prototype.topology", &new_cap).unwrap();
        let form = reopened.forms_prototype.as_ref().unwrap();
        assert_eq!(form.hierarchy.as_ref().unwrap().quad_cap_face(), Some(next));
        assert!(form.logical.faces[face].is_empty());
        assert_eq!(form.logical.faces[next].len(), 4);
        assert_eq!(reopened.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"], id);
        let recapped = reopened.model.state();
        assert!(crate::doc::naming::named_face_point(&recapped, None, &old_name).is_err());
        assert!(crate::doc::naming::named_face_point(&recapped, None, &format!("F{id}:FormPatch{}", next + 1)).is_ok());
    }

    #[test]
    fn refined_erase_and_fill_previews_a_new_owned_cap_and_applies_one_checkpoint() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[10],"counts":[2,2]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let hierarchy = before.hierarchy.as_ref().unwrap();
        let faces: Vec<_> = hierarchy.leaves.iter().enumerate().filter_map(|(id, leaf)| (leaf.active && leaf.root == 10).then_some(id)).collect();
        assert_eq!(faces.len(), 4);
        let cap = hierarchy.leaves.len();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let original = s.doc.clone();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        for invalid in [
            json!({"tool":"erase_and_fill","faces":[0],"tolerance":0.05}),
            json!({"tool":"erase_and_fill","faces":faces,"vertices":[0],"tolerance":0.05}),
            json!({"tool":"erase_and_fill","faces":faces,"tolerance":1.1}),
        ] {
            assert!(s.execute("form.prototype.topology", &invalid).is_err());
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
        }
        let request = json!({"topology":{"tool":"erase_and_fill","faces":faces,"tolerance":0.05}});
        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &request).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let replacement = form.definition();
        assert_eq!(replacement.cage, before.cage);
        assert_eq!(replacement.hierarchy.as_ref().unwrap().quad_cap_face(), Some(cap));
        assert_eq!(form.logical.faces[cap].len(), 4);
        assert!(faces.iter().all(|&id| form.logical.faces[id].is_empty()));
        assert_eq!(form.inspect()["undo"], undo + 1);
        s.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        assert_eq!(s.doc.as_ref(), original.as_ref());

        s.execute("form.prototype.tool.begin", &json!({})).unwrap();
        s.execute("form.prototype.tool.preview", &request).unwrap();
        s.execute("form.prototype.tool.apply", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), replacement);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], undo + 1);
        assert_eq!(s.doc.as_ref(), original.as_ref());
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), replacement);
        let restored = crate::recovery::restore_bytes(&crate::recovery::capture_bytes(&s).unwrap(), None).unwrap();
        assert_eq!(restored.forms_prototype.as_ref().unwrap().definition(), replacement);
    }

    #[test]
    fn a_hierarchical_crease_preserves_the_exact_ridge_in_cad_and_undo() {
        use crate::geom::subdivision::{hierarchy::Hierarchy, primitives::Parameters};
        use std::collections::BTreeSet;

        let parameters = Parameters { kind: "face".into(), dimensions: [20.0, 20.0, 1.0], segments: [1; 3], ..Default::default() };
        let mut h = Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[0], [2, 1]).unwrap();
        let mut roof = h.evaluate().unwrap();
        for point in &mut roof.points {
            point.z = 8.0 - 0.8 * point.x.abs();
        }
        h = h.with_evaluator(&roof).unwrap();
        let layout = h.layout().unwrap();
        let (seam, path) = layout.paths.iter().find(|(_, p)| p.iter().all(|&v| roof.points[v].x == 0.0)).unwrap();
        assert_eq!(path.len(), 3);
        assert!(!h.controls.contains_key(&path[1]), "the evaluator midpoint is not a selectable point");
        let before = crate::doc::forms::Definition {
            cage: h.evaluate().unwrap(),
            hierarchy: Some(h.clone()),
            parameters,
            imported: None,
            matching: None,
            pipe: None,
            placement: Default::default(),
            profile: None,
            symmetry: None,
        };
        let mut s =
            Session { forms_prototype: Some(crate::forms_prototype::Form::from_definition(before.clone(), 1, None).unwrap()), ..Default::default() };
        assert!(s.forms_prototype.as_ref().unwrap().surface.vertices[path[1]].z < 8.0);
        let sharp = json!({"tool":"crease","edges":[seam],"distance":10});
        let applied = s.execute("form.prototype.topology", &sharp).unwrap();
        assert_eq!(applied["undo"], 1);
        let after = s.forms_prototype.as_ref().unwrap().definition();
        let hierarchy = after.hierarchy.as_ref().unwrap();
        assert_eq!(hierarchy.base, h.base);
        assert_eq!(hierarchy.details, h.details);
        assert_eq!(hierarchy.leaves, h.leaves);
        assert_eq!(hierarchy.controls, h.controls);
        assert_eq!(hierarchy.layout().unwrap().faces, layout.faces);
        assert_eq!(after.cage.faces, before.cage.faces);
        assert_eq!(after.cage.points, before.cage.points);
        let expected: BTreeSet<_> = path.windows(2).map(|p| [p[0].min(p[1]), p[0].max(p[1])]).collect();
        assert_eq!(after.cage.edge_creases.iter().map(|c| c.edge).collect::<BTreeSet<_>>(), expected);
        assert!(after.cage.edge_creases.iter().all(|c| c.sharpness == 10.0));
        assert_eq!(s.execute("form.prototype.topology", &sharp).unwrap()["undo"], 1);
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let error = s.execute("form.prototype.topology", &json!({"tool":"uncrease","vertices":[path[1]]})).unwrap_err();
        assert!(error.to_string().contains("active logical point"), "{error}");
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);

        assert!(hierarchy.fit_root_patches(0.01).unwrap_err().to_string().contains("local sharpness"));
        let (body, report) = crate::doc::forms::convert(&after, 0.01).unwrap();
        assert!(body.is_surface() && !body.is_mesh());
        assert!(!report.trimmed_roots);
        assert_eq!(report.patch_roots.iter().copied().collect::<BTreeSet<_>>(), BTreeSet::from([1, 2]));
        let native: Vec<_> = body
            .record()
            .edges
            .iter()
            .enumerate()
            .filter_map(|(id, edge)| {
                let [a, b] = edge.uses.as_slice() else { return None };
                (report.patch_roots[a.face as usize] != report.patch_roots[b.face as usize]).then_some(id)
            })
            .collect();
        assert!(!native.is_empty(), "the ridge must be a shared native edge between the two logical faces");
        for edge in crate::kernel::edge_geometry(&body, &native).unwrap() {
            for t in [0.0, 0.5, 1.0] {
                let (point, tangent) = edge.at(t).unwrap();
                assert!(point.x.abs() < 1e-7 && (point.z - 8.0).abs() < 1e-7);
                assert!(tangent.x.abs() < 1e-7 && tangent.z.abs() < 1e-7);
            }
        }

        assert_eq!(s.execute("form.prototype.topology", &json!({"tool":"uncrease","edges":[seam]})).unwrap()["undo"], 2);
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn hierarchical_crease_mirrors_evaluator_segments_and_active_points() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[30,20,1],"segments":[3,1,1],"levels":1})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,1]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let h = before.hierarchy.as_ref().unwrap();
        let layout = h.layout().unwrap();
        let (seam, path) = layout.paths.iter().find(|(_, p)| p.iter().all(|&v| (before.cage.points[v].x + 10.0).abs() < 1e-10)).unwrap();
        assert_eq!(path.len(), 3);
        let point = seam[0];
        let partners = &before.symmetry.as_ref().unwrap().partners;
        assert_ne!(partners[point], point);
        s.execute("form.prototype.topology", &json!({"tool":"crease","edges":[seam],"vertices":[point],"distance":2.5})).unwrap();
        let after = s.forms_prototype.as_ref().unwrap().definition();
        assert_eq!(after.symmetry, before.symmetry);
        after.symmetry.as_ref().unwrap().validate_reflection(&after.cage).unwrap();
        assert_eq!(after.cage.edge_creases.len(), 4);
        assert_eq!(after.cage.vertex_creases.len(), 2);
        assert!(after.cage.edge_creases.iter().all(|c| c.sharpness == 2.5));
        assert!(after.cage.vertex_creases.iter().all(|c| c.sharpness == 2.5));
        s.execute("form.prototype.topology", &json!({"tool":"uncrease","edges":[seam],"vertices":[point]})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn hierarchical_crease_refuses_match_without_changing_source_or_history() {
        let (mut s, _, _) = matched_extrusion_case(1, true);
        let form = s.forms_prototype.as_ref().unwrap();
        let h = form.hierarchy.as_ref().unwrap();
        let (edge, _) = h.layout().unwrap().paths.into_iter().find(|(e, _)| e.iter().any(|&v| v >= h.base.points.len())).unwrap();
        let before = form.inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let error = s.execute("form.prototype.topology", &json!({"tool":"crease","edges":[edge],"distance":3})).unwrap_err();
        assert!(error.to_string().contains("detach Match"), "{error}");
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
    }

    #[test]
    fn logical_topology_keeps_the_evaluator_and_is_one_atomic_history_step() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().definition();
        let edge = before.cage.edges().unwrap()[0];
        s.execute("form.prototype.topology", &json!({"tool":"dissolve","edges":[edge]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.cage, before.cage);
        assert_eq!(f.logical.faces.iter().filter(|f| !f.is_empty()).count(), 23);
        assert_eq!(f.inspect()["undo"], 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,1]})).unwrap();
        let refined = s.forms_prototype.as_ref().unwrap().definition();
        let triangles = s.forms_prototype.as_ref().unwrap().surface.mesh.triangles.len();
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[24]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.cage, refined.cage);
        assert!(f.surface.mesh.triangles.len() < triangles);
        let after = f.inspect();
        assert!(s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[usize::MAX]})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), after);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), refined);
    }
    fn nested_loop_fixture(primitive: &str) -> Session {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":primitive,"size":10,"segments":[1,1,1],"levels":0})).unwrap();
        for level in 1..=2 {
            let faces: Vec<_> = s
                .forms_prototype
                .as_ref()
                .unwrap()
                .logical
                .faces
                .iter()
                .enumerate()
                .filter_map(|(id, face)| (!face.is_empty()).then_some(id))
                .collect();
            s.execute("form.prototype.refine", &json!({"faces":faces,"counts":[2,2]})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let vertex = *f.hierarchy.as_ref().unwrap().controls.iter().find(|(_, authoring)| **authoring == level).unwrap().0;
            let p = f.cage.points[vertex] + solidcore_record::Vec3::new(0.03, -0.02, 0.04) * level as f64;
            s.execute("form.prototype.set_vertices", &json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]]})).unwrap();
            if level == 1 {
                s.execute("form.prototype.topology", &json!({"tool":"crease","vertices":[0],"distance":0.3})).unwrap();
            }
        }
        s
    }

    #[test]
    fn hierarchical_loop_preserves_nested_details_ids_exact_endpoints_history_and_source_reopen() {
        for primitive in ["box", "face"] {
            for fraction in [0.3, 0.5, 0.7] {
                let mut s = nested_loop_fixture(primitive);
                let f = s.forms_prototype.as_ref().unwrap();
                let face = f.logical.faces.iter().find(|face| !face.is_empty()).unwrap();
                let seed = [face[0], face[1]];
                let records = f.hierarchical_loop_records(seed, fraction).unwrap();
                assert_eq!(records.first().unwrap().0 == records.last().unwrap().0, primitive == "box");
                let before = f.definition();
                let old = before.hierarchy.as_ref().unwrap();
                assert!(old.details[1].iter().any(|detail| detail.delta.len() > 0.001));
                assert!(old.details[2].iter().any(|detail| detail.delta.len() > 0.001));
                assert!(!old.base.vertex_creases.is_empty());
                let depth = f.inspect()["undo"].as_u64().unwrap();
                let plan: Vec<_> = records.windows(2).map(|pair| old.insert_target(pair[0].0, pair[1].0).unwrap()).collect();
                let curves = crate::doc::forms::edge_curves(&before, 0.01, &records.iter().map(|record| record.0).collect::<Vec<_>>()).unwrap();
                let targets: Vec<_> = curves.iter().zip(&records).map(|(curve, record)| curve.at(record.1).unwrap()).collect();
                s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":fraction})).unwrap();
                let f = s.forms_prototype.as_ref().unwrap();
                let after = f.definition();
                let h = after.hierarchy.as_ref().unwrap();
                assert_eq!(f.inspect()["undo"].as_u64().unwrap(), depth + 1);
                assert_eq!(h.base, old.base);
                assert_eq!(h.sharpness, old.sharpness);
                for (&id, &level) in &old.controls {
                    assert_eq!(h.controls.get(&id), Some(&level));
                }
                for (level, details) in old.details.iter().enumerate() {
                    for detail in details {
                        assert!(h.details[level].contains(detail), "insertion must retain existing authored detail records");
                    }
                }
                for (id, leaf) in old.leaves.iter().enumerate() {
                    let mut expected = leaf.clone();
                    if plan.iter().any(|&(parent, _)| parent == id) {
                        expected.active = false;
                    }
                    assert_eq!(h.leaves[id], expected, "old leaf slots must not be reordered or overwritten");
                }
                let endpoints: std::collections::BTreeSet<_> =
                    plan.iter().flat_map(|&(parent, axis)| h.seam_endpoints(parent, axis).unwrap()).collect();
                let unique_edges: std::collections::BTreeSet<_> = records.iter().map(|record| record.0).collect();
                assert_eq!(endpoints.len(), unique_edges.len(), "the closed seed endpoint must be deduplicated");
                for &id in &endpoints {
                    assert!(!old.controls.contains_key(&id));
                }
                for target in targets {
                    assert!(endpoints.iter().any(|&id| f.surface.vertices[id].dist(target) <= 0.01));
                }
                s.execute("form.prototype.undo", &json!({})).unwrap();
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
                s.execute("form.prototype.redo", &json!({})).unwrap();
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
                let decoded: crate::doc::forms::Definition = serde_json::from_slice(&serde_json::to_vec(&after).unwrap()).unwrap();
                let reopened = crate::forms_prototype::Form::from_definition(decoded, 0, None).unwrap();
                assert_eq!(reopened.definition(), after);
            }
        }
    }

    #[test]
    fn hierarchical_loop_seed_orientation_and_open_strip_order_preserve_parameter_direction() {
        for primitive in ["box", "face"] {
            let mut s = nested_loop_fixture(primitive);
            let f = s.forms_prototype.as_ref().unwrap();
            let face = f.logical.faces.iter().find(|face| !face.is_empty()).unwrap();
            let seed = [face[0], face[1]];
            let before = f.definition();
            let forward = f.hierarchical_loop_records(seed, 0.3).unwrap();
            let reverse = f.hierarchical_loop_records([seed[1], seed[0]], 0.7).unwrap();
            assert_eq!(forward.len(), reverse.len());
            for (a, b) in forward.iter().zip(&reverse) {
                assert_eq!(a.0, b.0);
                assert!((a.1 - b.1).abs() < 1e-12);
            }
            // Pick an interior strip edge as well as the boundary seed: both
            // halves must concatenate without reversing the edge parameter.
            if primitive == "face" {
                let interior = forward[forward.len() / 2];
                let records = f.hierarchical_loop_records(interior.0, interior.1).unwrap();
                let a: std::collections::BTreeMap<_, _> = forward.iter().copied().collect();
                let b: std::collections::BTreeMap<_, _> = records.iter().copied().collect();
                assert_eq!(a.len(), b.len());
                for (edge, fraction) in a {
                    assert!((fraction - b[&edge]).abs() < 1e-12);
                }
            }
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.3})).unwrap();
            let a = s.forms_prototype.as_ref().unwrap().definition();
            let mut reversed =
                Session { forms_prototype: Some(crate::forms_prototype::Form::from_definition(before, 0, None).unwrap()), ..Default::default() };
            reversed.execute("form.prototype.insert_loop", &json!({"edge":[seed[1],seed[0]],"fraction":0.7})).unwrap();
            let b = reversed.forms_prototype.as_ref().unwrap().definition();
            assert_eq!(a.hierarchy.as_ref().unwrap().leaves, b.hierarchy.as_ref().unwrap().leaves);
            for (a, b) in a.cage.points.iter().zip(&b.cage.points) {
                assert!(a.dist(*b) < 1e-8);
            }
        }
    }

    #[test]
    fn hierarchical_loop_refusals_leave_source_history_and_recovery_untouched() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let face = s.forms_prototype.as_ref().unwrap().logical.faces.iter().find(|face| face.len() > 4).unwrap();
        let seed = [face[0], face[1]];
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let refusal = s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.3})).unwrap_err();
        assert!(refusal.to_string().contains("aligned dyadic"), "{refusal}");
        for parameters in [json!({"edge":[99999,99998]}), json!({"edge":seed,"fraction":0}), json!({"edge":seed,"fraction":1})] {
            assert!(s.execute("form.prototype.insert_loop", &parameters).is_err());
        }
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);

        let mut symmetric = Session::default();
        symmetric.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":0})).unwrap();
        symmetric.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        symmetric.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let face = symmetric.forms_prototype.as_ref().unwrap().logical.faces.iter().find(|face| face.len() > 4).unwrap();
        let seed = [face[0], face[1]];
        let before = symmetric.forms_prototype.as_ref().unwrap().inspect();
        let recovery = crate::recovery::capture_bytes(&symmetric).unwrap();
        let refusal = symmetric.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.3})).unwrap_err();
        assert!(refusal.to_string().contains("aligned dyadic"), "{refusal}");
        assert_eq!(symmetric.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&symmetric).unwrap(), recovery);
    }

    #[test]
    fn hierarchical_loop_target_budget_and_insert_point_public_record_limit_are_atomic() {
        use solidcore_record::Vec3;
        use solidcraft_geom::subdivision::{Cage, hierarchy::Hierarchy};
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let mut definition = s.forms_prototype.as_ref().unwrap().definition();
        // 256 regular quads require 257 distinct boundary-to-boundary targets.
        let cage = Cage {
            points: (0..=256).flat_map(|x| [Vec3::new(x as f64, 0.0, 0.0), Vec3::new(x as f64, 1.0, 0.0)]).collect(),
            faces: (0..256).map(|x| vec![2 * x, 2 * x + 2, 2 * x + 3, 2 * x + 1]).collect(),
            ..Default::default()
        };
        definition.hierarchy = Some(Hierarchy::new(cage.clone()).unwrap());
        definition.cage = cage;
        s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 0, None).unwrap());
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let refusal = s.execute("form.prototype.insert_loop", &json!({"edge":[0,1]})).unwrap_err();
        assert!(refusal.to_string().contains("256 unique"), "{refusal}");
        let too_many = vec![json!({"edge":[0,1],"fraction":0.5}); 33];
        let refusal = s.execute("form.prototype.insert_points", &json!({"points":too_many})).unwrap_err();
        assert!(refusal.to_string().contains("2..32"), "{refusal}");
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
    }

    #[test]
    fn hierarchical_loop_depth_budget_refuses_without_a_history_or_source_change() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[16,16]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let face = f.logical.faces.iter().find(|face| !face.is_empty()).unwrap();
        let seed = [face[0], face[1]];
        let before = f.inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let refusal = s.execute("form.prototype.insert_loop", &json!({"edge":seed})).unwrap_err();
        assert!(refusal.to_string().contains("depth budget"), "{refusal}");
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
    }

    #[test]
    fn hierarchical_loop_rebinds_matched_regular_strips_without_dropping_original_coordinates() {
        for order in [0, 1, 2] {
            let (mut s, _, _) = matched_extrusion_case(order, false);
            s.execute("form.prototype.refine", &json!({"faces":(0..16).collect::<Vec<_>>(),"counts":[2,2]})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let face = f.logical.faces.iter().find(|face| !face.is_empty()).unwrap();
            let seed = [face[0], face[1]];
            let records = f.hierarchical_loop_records(seed, 0.3).unwrap();
            let before = f.definition();
            let old = before.matching.as_ref().unwrap();
            let old_ring = crate::doc::forms::match_openings(&before.cage, before.hierarchy.as_ref()).unwrap().remove(0);
            let curves = crate::doc::forms::edge_curves(&before, 0.01, &records.iter().map(|record| record.0).collect::<Vec<_>>()).unwrap();
            let targets: Vec<_> = curves.iter().zip(&records).map(|(curve, record)| curve.at(record.1).unwrap()).collect();
            let plan: Vec<_> =
                records.windows(2).map(|pair| before.hierarchy.as_ref().unwrap().insert_target(pair[0].0, pair[1].0).unwrap()).collect();
            let depth = f.inspect()["undo"].as_u64().unwrap();
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.3})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let after = f.definition();
            let matching = f.matching.as_ref().unwrap();
            let new_ring = crate::doc::forms::match_openings(&f.cage, f.hierarchy.as_ref()).unwrap().remove(0);
            for (&vertex, coordinate) in old_ring.iter().zip(&old.coordinates) {
                let station = new_ring.iter().position(|&id| id == vertex).unwrap();
                assert_eq!(&matching.coordinates[station], coordinate, "G{order}: an original station must retain its saved native coordinate");
            }
            let h = f.hierarchy.as_ref().unwrap();
            let endpoints: std::collections::BTreeSet<_> = plan.iter().flat_map(|&(parent, axis)| h.seam_endpoints(parent, axis).unwrap()).collect();
            for target in targets {
                assert!(
                    endpoints.iter().any(|&id| f.surface.vertices[id].dist(target) <= 0.01),
                    "G{order}: Match must retain the original CAD insertion target"
                );
            }
            assert!(f.match_report.as_ref().unwrap().position_mm <= matching.config.tolerances.position_mm);
            assert_eq!(f.inspect()["undo"].as_u64().unwrap(), depth + 1);
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        }
    }

    fn symmetric_loop_fixture(axis: usize, nested: bool) -> Session {
        use crate::geom::subdivision::hierarchy::Hierarchy;
        use solidcore_record::Vec3;
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","size":10,"segments":[1,1,1],"levels":0})).unwrap();
        let mut definition = s.forms_prototype.as_ref().unwrap().definition();
        for point in &mut definition.cage.points {
            let (u, v) = (point.x, point.y);
            let mut coordinates = [0.0; 3];
            coordinates[axis] = u;
            coordinates[(axis + 1) % 3] = v;
            *point = Vec3::new(coordinates[0], coordinates[1], coordinates[2]);
        }
        definition.hierarchy = Some(Hierarchy::new(definition.cage.clone()).unwrap());
        s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 0, None).unwrap());
        s.execute("form.prototype.symmetry", &json!({"axis":axis})).unwrap();
        if nested {
            for level in 1..=2 {
                let ids: Vec<_> = s
                    .forms_prototype
                    .as_ref()
                    .unwrap()
                    .hierarchy
                    .as_ref()
                    .unwrap()
                    .leaves
                    .iter()
                    .enumerate()
                    .filter_map(|(id, leaf)| leaf.active.then_some(id))
                    .collect();
                s.execute("form.prototype.refine", &json!({"faces":ids,"counts":[2,2]})).unwrap();
                let f = s.forms_prototype.as_ref().unwrap();
                let vertex = *f.hierarchy.as_ref().unwrap().controls.iter().find(|(_, birth)| **birth == level).unwrap().0;
                let mut displacement = [0.0; 3];
                displacement[(axis + 2) % 3] = 0.025 * level as f64;
                let p = f.control_positions()[vertex] + Vec3::new(displacement[0], displacement[1], displacement[2]);
                s.execute("form.prototype.set_vertices", &json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]]})).unwrap();
            }
        }
        s
    }

    fn mirrored_loop_records(f: &crate::forms_prototype::Form, records: &[([usize; 2], f64)]) -> Vec<([usize; 2], f64)> {
        let symmetry = f.symmetry.as_ref().unwrap();
        records
            .iter()
            .map(|&(edge, fraction)| {
                let [a, b] = edge.map(|v| symmetry.partners[v]);
                ([a.min(b), a.max(b)], if a < b { fraction } else { 1.0 - fraction })
            })
            .collect()
    }

    #[test]
    fn hierarchical_loop_mirrors_disjoint_regular_strips_in_the_placed_local_frame() {
        use solidcore_record::Vec3;
        for axis in 0..3 {
            for placed in [false, true] {
                let mut s = symmetric_loop_fixture(axis, true);
                if placed {
                    let matrix = crate::doc::rigid(Vec3::new(20.0, -8.0, 3.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.6);
                    s.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
                }
                let f = s.forms_prototype.as_ref().unwrap();
                let h = f.hierarchy.as_ref().unwrap();
                let id = h.leaves.iter().position(|leaf| leaf.active && leaf.domain[0] == 0 && leaf.domain[1] == 0).unwrap();
                let face = &f.logical.faces[id];
                let seed = [face[0], face[1]];
                let original_records = f.hierarchical_loop_records(seed, 0.3).unwrap();
                let mirror = mirrored_loop_records(f, &original_records);
                let source_parents: std::collections::BTreeSet<_> =
                    original_records.windows(2).map(|p| h.insert_target(p[0].0, p[1].0).unwrap().0).collect();
                let mirror_parents: std::collections::BTreeSet<_> = mirror.windows(2).map(|p| h.insert_target(p[0].0, p[1].0).unwrap().0).collect();
                assert!(source_parents.is_disjoint(&mirror_parents), "fixture must exercise distinct mirrored strips");
                let records: Vec<_> = original_records.iter().chain(&mirror).copied().collect();
                let before = f.definition();
                let curves = crate::doc::forms::edge_curves(&before, 0.01, &records.iter().map(|record| record.0).collect::<Vec<_>>()).unwrap();
                let targets: Vec<_> = curves.iter().zip(&records).map(|(curve, record)| curve.at(record.1).unwrap()).collect();
                let undo = f.inspect()["undo"].as_u64().unwrap();
                s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.3})).unwrap();
                let f = s.forms_prototype.as_ref().unwrap();
                let after = f.definition();
                let old_h = before.hierarchy.as_ref().unwrap();
                let h = after.hierarchy.as_ref().unwrap();
                let symmetry = f.symmetry.as_ref().unwrap();
                let old_symmetry = before.symmetry.as_ref().unwrap();
                assert_eq!(&symmetry.partners[..old_symmetry.partners.len()], old_symmetry.partners.as_slice());
                assert_eq!(after.placement, before.placement);
                assert_eq!(h.base, old_h.base);
                assert_eq!(h.sharpness, old_h.sharpness);
                for (&id, &birth) in &old_h.controls {
                    assert_eq!(h.controls.get(&id), Some(&birth));
                }
                for (level, details) in old_h.details.iter().enumerate() {
                    for detail in details {
                        assert!(h.details[level].contains(detail));
                    }
                }
                let endpoints: std::collections::BTreeSet<_> = original_records
                    .windows(2)
                    .chain(mirror.windows(2))
                    .flat_map(|p| {
                        let (parent, axis) = old_h.insert_target(p[0].0, p[1].0).unwrap();
                        h.seam_endpoints(parent, axis).unwrap()
                    })
                    .collect();
                let unique: std::collections::BTreeSet<_> = records.iter().map(|record| record.0).collect();
                assert_eq!(endpoints.len(), unique.len());
                for &id in &endpoints {
                    assert!(!old_h.controls.contains_key(&id));
                    assert!(endpoints.contains(&symmetry.partners[id]));
                    assert_eq!(h.controls.get(&id), h.controls.get(&symmetry.partners[id]));
                }
                for target in targets {
                    assert!(endpoints.iter().any(|&id| f.surface.vertices[id].dist(target) <= 0.01));
                }
                symmetry.validate_reflection(&f.cage).unwrap();
                assert_eq!(f.inspect()["undo"].as_u64().unwrap(), undo + 1);
                s.execute("form.prototype.undo", &json!({})).unwrap();
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
                s.execute("form.prototype.redo", &json!({})).unwrap();
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
                let decoded = serde_json::from_slice(&serde_json::to_vec(&after).unwrap()).unwrap();
                assert_eq!(crate::forms_prototype::Form::from_definition(decoded, 0, None).unwrap().definition(), after);
            }
        }
    }

    #[test]
    fn hierarchical_loop_deduplicates_self_mirrors_and_refuses_distinct_source_stations_atomically() {
        for primitive in ["face", "box"] {
            let mut s = symmetric_loop_fixture(0, false);
            if primitive == "box" {
                s.execute("form.prototype.create", &json!({"primitive":"box","size":10,"segments":[1,1,1],"levels":0})).unwrap();
                let mut definition = s.forms_prototype.as_ref().unwrap().definition();
                definition.hierarchy = Some(crate::geom::subdivision::hierarchy::Hierarchy::new(definition.cage.clone()).unwrap());
                s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 0, None).unwrap());
                s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
            }
            let f = s.forms_prototype.as_ref().unwrap();
            let face = &f.logical.faces[0];
            let seed = [face[0], face[1]];
            let records = f.hierarchical_loop_records(seed, 0.5).unwrap();
            assert_eq!(records.first().unwrap().0 == records.last().unwrap().0, primitive == "box");
            let h = f.hierarchy.as_ref().unwrap();
            let plan: Vec<_> = records.windows(2).map(|p| h.insert_target(p[0].0, p[1].0).unwrap()).collect();
            let active = h.leaves.iter().filter(|leaf| leaf.active).count();
            let before = f.definition();
            let inspect = f.inspect();
            let recovery = crate::recovery::capture_bytes(&s).unwrap();
            let refusal = s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.3})).unwrap_err();
            assert!(refusal.to_string().contains("different stations"), "{refusal}");
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), inspect);
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
            let undo = inspect["undo"].as_u64().unwrap();
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.5})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let after = f.definition();
            let h = f.hierarchy.as_ref().unwrap();
            assert_eq!(h.leaves.iter().filter(|leaf| leaf.active).count(), active + plan.len(), "a self-mirror must refine each parent once");
            let endpoints: std::collections::BTreeSet<_> = plan.iter().flat_map(|&(parent, axis)| h.seam_endpoints(parent, axis).unwrap()).collect();
            for id in endpoints {
                assert_eq!(f.symmetry.as_ref().unwrap().partners[id], id);
                assert!(f.surface.vertices[id].x.abs() < 1e-8);
            }
            assert_eq!(f.inspect()["undo"].as_u64().unwrap(), undo + 1);
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        }
    }

    #[test]
    fn hierarchical_loop_refuses_cross_axis_mirror_overlap_without_partial_refinement() {
        use solidcore_record::Vec3;
        let mut s = symmetric_loop_fixture(0, false);
        let mut definition = s.forms_prototype.as_ref().unwrap().definition();
        // A diamond's X mirror exchanges its root U/V axes. Each strip is
        // individually regular, but the pair would split this parent twice.
        for p in &mut definition.cage.points {
            *p = Vec3::new(p.x - p.y, p.x + p.y, 0.0);
        }
        definition.symmetry = Some(crate::geom::subdivision::symmetry::Symmetry::new(&definition.cage, 0).unwrap());
        definition.hierarchy = Some(crate::geom::subdivision::hierarchy::Hierarchy::new(definition.cage.clone()).unwrap());
        s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 0, None).unwrap());
        let f = s.forms_prototype.as_ref().unwrap();
        let face = &f.logical.faces[0];
        let seed = [face[0], face[1]];
        let before = f.inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let refusal = s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.5})).unwrap_err();
        assert!(refusal.to_string().contains("different parent seams"), "{refusal}");
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
    }

    #[test]
    fn hierarchical_loop_mirrors_matched_targets_without_rebinding_old_native_stations() {
        use solidcore_record::Vec3;
        for order in [0, 1, 2] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length":8,"width":8,"height":1,"corner":[-4,-4,-1],"body_name":"Support"})).unwrap();
            let body = s.model.state().bodies[0].body.clone();
            let top = (0..body.face_count()).find(|&id| body.planar_face(id).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
            s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":2})).unwrap();
            s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
            let ring = crate::doc::forms::match_openings(&s.forms_prototype.as_ref().unwrap().cage, None).unwrap().remove(0);
            s.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":[ring[0],ring[1]],"order":order})).unwrap();
            s.execute("form.prototype.refine", &json!({"faces":(0..16).collect::<Vec<_>>(),"counts":[2,2]})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let h = f.hierarchy.as_ref().unwrap();
            let parent = h.leaves.iter().position(|leaf| leaf.active && leaf.root == 0 && leaf.domain[0] == 0 && leaf.domain[1] == 0).unwrap();
            let face = &f.logical.faces[parent];
            let seed = [face[0], face[1]];
            let records = f.hierarchical_loop_records(seed, 0.3).unwrap();
            let mirror = mirrored_loop_records(f, &records);
            let all: Vec<_> = records.iter().chain(&mirror).copied().collect();
            let before = f.definition();
            let original = before.matching.as_ref().unwrap();
            let old_ring = crate::doc::forms::match_openings(&before.cage, before.hierarchy.as_ref()).unwrap().remove(0);
            let curves = crate::doc::forms::edge_curves(&before, 0.01, &all.iter().map(|r| r.0).collect::<Vec<_>>()).unwrap();
            let targets: Vec<_> = curves.iter().zip(&all).map(|(curve, record)| curve.at(record.1).unwrap()).collect();
            let undo = f.inspect()["undo"].as_u64().unwrap();
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.3})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let after = f.definition();
            let matching = f.matching.as_ref().unwrap();
            let new_ring = crate::doc::forms::match_openings(&after.cage, after.hierarchy.as_ref()).unwrap().remove(0);
            assert_eq!(matching.source, original.source);
            assert_eq!(matching.binding, original.binding);
            for (&id, coordinate) in old_ring.iter().zip(&original.coordinates) {
                let station = new_ring.iter().position(|&v| v == id).unwrap();
                assert_eq!(&matching.coordinates[station], coordinate, "G{order}: old native station");
            }
            let h = f.hierarchy.as_ref().unwrap();
            let endpoints: std::collections::BTreeSet<_> = records
                .windows(2)
                .chain(mirror.windows(2))
                .flat_map(|p| {
                    let (parent, axis) = before.hierarchy.as_ref().unwrap().insert_target(p[0].0, p[1].0).unwrap();
                    h.seam_endpoints(parent, axis).unwrap()
                })
                .collect();
            for target in targets {
                assert!(endpoints.iter().any(|&id| f.surface.vertices[id].dist(target) <= 0.01), "G{order}: original CAD target");
            }
            let report = f.match_report.as_ref().unwrap();
            assert!(report.position_mm <= matching.config.tolerances.position_mm);
            if order >= 1 {
                assert!(report.tangent_deg <= matching.config.tolerances.tangent_deg);
            }
            if order == 2 {
                assert!(report.curvature_relative <= matching.config.tolerances.curvature_relative);
            }
            f.symmetry.as_ref().unwrap().validate_reflection(&f.cage).unwrap();
            assert_eq!(f.inspect()["undo"].as_u64().unwrap(), undo + 1);
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        }
    }

    #[test]
    fn hierarchical_loop_mirror_union_budget_counts_both_strips_without_partial_refinement() {
        use crate::geom::subdivision::{Cage, hierarchy::Hierarchy};
        use solidcore_record::Vec3;
        let mut s = symmetric_loop_fixture(0, false);
        let mut definition = s.forms_prototype.as_ref().unwrap().definition();
        let cage = Cage {
            points: (0..=128).flat_map(|y| (0..=2).map(move |x| Vec3::new(f64::from(x) - 1.0, f64::from(y), 0.0))).collect(),
            faces: (0..128).flat_map(|y| (0..2).map(move |x| vec![3 * y + x, 3 * y + x + 1, 3 * (y + 1) + x + 1, 3 * (y + 1) + x])).collect(),
            ..Default::default()
        };
        definition.symmetry = Some(crate::geom::subdivision::symmetry::Symmetry::new(&cage, 0).unwrap());
        definition.hierarchy = Some(Hierarchy::new(cage.clone()).unwrap());
        definition.cage = cage;
        s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 0, None).unwrap());
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        let refusal = s.execute("form.prototype.insert_loop", &json!({"edge":[0,1],"fraction":0.3})).unwrap_err();
        assert!(refusal.to_string().contains("256 unique"), "{refusal}");
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
    }

    fn aligned_loop_fixture() -> (Session, [usize; 2]) {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,4,1],"segments":[2,1,1],"levels":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[1],"counts":[2,2]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let vertex = *f.hierarchy.as_ref().unwrap().controls.iter().find(|(_, birth)| **birth == 1).unwrap().0;
        let p = f.control_positions()[vertex] + Vec3::new(0.0, 0.0, 0.08);
        s.execute("form.prototype.set_vertices", &json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]]})).unwrap();
        let sides = s.forms_prototype.as_ref().unwrap().hierarchy.as_ref().unwrap().parameter_sides().unwrap();
        let seed = sides.iter().find(|leaf| leaf.id == 0).unwrap().sides[3][0].edge;
        (s, seed)
    }

    #[test]
    fn insert_loop_contract_accepts_legacy_edges_and_requires_one_unambiguous_seed() {
        let legacy = serde_json::from_value::<params::InsertLoop>(json!({"edge":[2,1]})).unwrap();
        let params::InsertLoop::Edge(legacy) = legacy else { panic!("legacy edge shape must remain valid") };
        assert_eq!(legacy.edge, [2, 1]);
        assert_eq!(legacy.fraction, 0.5);
        let face = serde_json::from_value::<params::InsertLoop>(json!({"face":1,"axis":0,"tolerance":0.05})).unwrap();
        let params::InsertLoop::Face(face) = face else { panic!("face/axis must use its own seed") };
        assert_eq!((face.face, face.axis, face.tolerance), (1, 0, Some(0.05)));
        for invalid in [json!({"edge":[2,1],"face":1,"axis":0}), json!({"face":1,"axis":0,"fraction":0.5}), json!({"face":1})] {
            assert!(serde_json::from_value::<params::InsertLoop>(invalid).is_err());
        }
    }
    #[test]
    fn hierarchical_loop_aligned_t_continuation_reuses_fine_seams_and_preserves_history_and_placement() {
        for (placed, fraction) in [(false, 0.25), (true, 0.25), (false, 0.5), (true, 0.5)] {
            let (mut s, seed) = aligned_loop_fixture();
            if placed {
                let matrix = crate::doc::rigid(Vec3::new(12.0, -4.0, 9.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.7);
                s.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
            }
            let f = s.forms_prototype.as_ref().unwrap();
            let before = f.definition();
            let old = before.hierarchy.as_ref().unwrap();
            let plan = old.aligned_loop(seed, fraction).unwrap();
            assert_eq!(plan.existing_edges.len(), if fraction == 0.5 { 2 } else { 0 });
            let undo = f.inspect()["undo"].as_u64().unwrap();
            let curves =
                crate::doc::forms::edge_curves(&before, 0.01, &plan.points.iter().map(|point| point.source_edge).collect::<Vec<_>>()).unwrap();
            let targets: Vec<_> = curves.iter().zip(&plan.points).map(|(curve, point)| curve.at(point.fraction).unwrap()).collect();
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":fraction})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let after = f.definition();
            let h = after.hierarchy.as_ref().unwrap();
            assert_eq!(h.evaluate().unwrap(), old.evaluate().unwrap().refined(h.depth - old.depth).unwrap());
            assert_eq!(&h.details[..old.details.len()], old.details.as_slice());
            assert_eq!(after.placement, before.placement);
            assert_eq!(h.leaves.iter().filter(|leaf| leaf.active).count(), if fraction == 0.5 { 6 } else { 8 });
            let ids = h.aligned_loop_vertices(&plan.points).unwrap();
            for ((point, &id), target) in plan.points.iter().zip(&ids).zip(targets) {
                assert!(f.surface.vertices[id].dist(target) <= 0.01);
                if let Some(old) = point.existing {
                    assert_eq!(old, id);
                }
            }
            let layout = h.layout().unwrap();
            for edge in &plan.existing_edges {
                assert!(layout.paths.iter().any(|(other, _)| other == edge));
            }
            assert_eq!(f.inspect()["undo"].as_u64().unwrap(), undo + 1);
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
            let decoded = serde_json::from_slice(&serde_json::to_vec(&after).unwrap()).unwrap();
            assert_eq!(crate::forms_prototype::Form::from_definition(decoded, 0, None).unwrap().definition(), after);
        }
    }
    #[test]
    fn hierarchical_loop_aligned_mixed_depth_closed_and_mirrored_plans_do_not_balance_extra_faces() {
        for symmetric in [false, true] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"primitive":"box","segments":[1,1,1],"levels":0})).unwrap();
            if symmetric {
                s.execute("form.prototype.symmetry", &json!({"axis":2})).unwrap();
            }
            s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let before = f.definition();
            let h = before.hierarchy.as_ref().unwrap();
            let face = f.logical.faces.iter().find(|face| face.len() > 4).unwrap();
            let seed = [face[0], face[1]];
            let mut cuts = h.aligned_loop(seed, 0.5).unwrap().cuts;
            if let Some(symmetry) = &before.symmetry {
                cuts.extend(h.aligned_loop(seed.map(|id| symmetry.partners[id]), 0.5).unwrap().cuts);
            }
            let unique: std::collections::BTreeSet<_> = cuts.iter().map(|cut| (cut.parent, cut.axis, cut.coordinate)).collect();
            let active = h.leaves.iter().filter(|leaf| leaf.active).count();
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.5})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let h = f.hierarchy.as_ref().unwrap();
            assert_eq!(h.leaves.iter().filter(|leaf| leaf.active).count(), active + unique.len());
            assert_eq!(h.base, before.hierarchy.as_ref().unwrap().base);
            if let Some(symmetry) = &f.symmetry {
                symmetry.validate_reflection(&f.cage).unwrap();
            }
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        }
    }
    #[test]
    fn hierarchical_loop_aligned_mixed_depth_match_keeps_original_g0_g1_g2_native_stations() {
        for order in [0, 1, 2] {
            let (mut s, _, _) = matched_extrusion_case(order, false);
            s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let before = f.definition();
            let h = before.hierarchy.as_ref().unwrap();
            let sides = h.parameter_sides().unwrap();
            let coarse = sides.iter().find(|leaf| leaf.id == 1).unwrap();
            let seed = coarse.sides[1][0].edge;
            let plan = h.aligned_loop(seed, 0.5).unwrap();
            assert!(!plan.existing_edges.is_empty());
            let old = before.matching.as_ref().unwrap();
            let old_ring = crate::doc::forms::match_openings(&before.cage, Some(h)).unwrap().remove(0);
            let undo = f.inspect()["undo"].as_u64().unwrap();
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.5})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let after = f.definition();
            let matching = f.matching.as_ref().unwrap();
            let new_ring = crate::doc::forms::match_openings(&after.cage, after.hierarchy.as_ref()).unwrap().remove(0);
            assert_eq!(matching.source, old.source);
            assert_eq!(matching.binding, old.binding);
            for (&vertex, coordinate) in old_ring.iter().zip(&old.coordinates) {
                let index = new_ring.iter().position(|&id| id == vertex).unwrap();
                assert_eq!(&matching.coordinates[index], coordinate, "G{order}: reused rim controls keep their original native station");
            }
            let report = f.match_report.as_ref().unwrap();
            assert!(report.position_mm <= matching.config.tolerances.position_mm);
            if order > 0 {
                assert!(report.tangent_deg <= matching.config.tolerances.tangent_deg);
            }
            if order == 2 {
                assert!(report.curvature_relative <= matching.config.tolerances.curvature_relative);
            }
            assert_eq!(f.inspect()["undo"].as_u64().unwrap(), undo + 1);
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        }
    }

    fn aligned_match_seam_band(cage: &solidcraft_geom::subdivision::Cage, rim: &[usize], rows: usize) -> std::collections::BTreeSet<usize> {
        let edges = cage.edges().unwrap();
        let mut band: std::collections::BTreeSet<_> = rim.iter().copied().collect();
        for _ in 0..rows {
            let mut next = band.clone();
            for &[a, b] in &edges {
                if band.contains(&a) || band.contains(&b) {
                    next.extend([a, b]);
                }
            }
            band = next;
        }
        band
    }

    fn refine_aligned_match_and_sculpt(s: &mut Session, sculpt_face: usize, order: usize, amount: f64) -> [usize; 2] {
        assert!(sculpt_face > 1, "keep the original refinement root and adjacent coarse seed");
        let f = s.forms_prototype.as_ref().unwrap();
        // One subdivision step retains old IDs, appends edge points, then
        // appends face points in source face order. Refine an interior face
        // as well as root 0 so the sculpt uses a genuinely new control.
        let control = f.cage.points.len() + f.cage.edges().unwrap().len() + sculpt_face;
        s.execute("form.prototype.refine", &json!({"faces":[0,sculpt_face],"counts":[2,2]}))
            .unwrap_or_else(|error| panic!("G{order} initial mixed-depth refinement: {error}"));
        let f = s.forms_prototype.as_ref().unwrap();
        let h = f.hierarchy.as_ref().unwrap();
        assert_eq!(h.depth, 1);
        assert_eq!(h.controls.get(&control), Some(&1));
        let rim = crate::doc::forms::match_openings(&f.cage, Some(h)).unwrap().remove(0);
        assert_eq!(rim.len(), 32);
        assert!(!aligned_match_seam_band(&f.cage, &rim, 2).contains(&control), "the fine sculpt must lie beyond both inward seam rows");
        let rest = f.control_positions()[control];
        let evaluated_rest = f.surface.vertices[control];
        let old_detail = h.details[1].iter().find(|d| d.vertex == control).map_or(Vec3::ZERO, |d| d.delta);
        let target = rest + Vec3::Z * amount;
        s.execute("form.prototype.set_vertices", &json!({"vertices":[control],"positions":[[target.x,target.y,target.z]]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert!(f.control_positions()[control].dist(target) < 1e-8, "G{order}: the edit must first reach the fine control");
        let source = f.definition();
        let (resolved, report) = source
            .matching
            .as_ref()
            .unwrap()
            .recompute(&source, &match_state(s).unwrap(), 0.01)
            .unwrap_or_else(|error| panic!("G{order} sculpted baseline Match: {error}"));
        s.forms_prototype.as_mut().unwrap().refresh_match_preview(resolved, report).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let h = f.hierarchy.as_ref().unwrap();
        let detail = h.details[1].iter().find(|d| d.vertex == control).unwrap();
        assert!((detail.delta - old_detail).dist(Vec3::Z * amount) < 1e-8, "G{order}: retain the authored fine detail after Match");
        assert!(f.control_positions()[control].dist(target) < 1e-8, "G{order}: Match must retain the interior control target");
        assert!(f.surface.vertices[control].dist(evaluated_rest) > 0.001, "G{order}: the source baseline must retain a real evaluated fine sculpt");
        let sides = h.parameter_sides().unwrap();
        sides.iter().find(|leaf| leaf.id == 1).unwrap().sides[1][0].edge
    }

    fn curved_aligned_loop_match_fixture(order: usize) -> (Session, [usize; 2]) {
        use solidcraft_geom::bspline::NurbsSurface;
        let mut s = Session::default();
        // The pinned 5Ã—5 cage is a uniform cubic tensor spline. At each
        // endpoint reflect the first interior coefficient through the endpoint.
        // The extended interior cubic mask then reduces to the open boundary
        // mask on the rim and the pinned mask at corners. A polynomial through
        // the unextended control points is a different surface.
        let coordinates = [-6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0];
        let squares = [28.0, 16.0, 4.0, 0.0, 4.0, 16.0, 28.0];
        let points: Vec<_> =
            (0..7).flat_map(|v| (0..7).map(move |u| Vec3::new(coordinates[u], coordinates[v], 0.005 * squares[u] + 0.004 * squares[v]))).collect();
        let knots: Vec<_> = (-3..=7).map(|i| f64::from(i) / 4.0).collect();
        let patch = NurbsSurface::new([3, 3], [knots.clone(), knots], &points, &[1.0; 49]).unwrap();
        assert!(patch.eval(0.1, 0.2).unwrap().z > patch.eval(0.5, 0.5).unwrap().z + 0.05);
        let body = crate::kernel::nurbs_sheet(&patch).unwrap();
        let step = crate::kernel::step_export(&[&body], "SolidCraft").unwrap();
        let id = s
            .doc_mut()
            .add_feature(
                crate::doc::FeatureKind::Import { file: "original-cubic-support.step".into(), step: step.into(), components: vec![], skip: vec![] },
                Some("CurvedSupport"),
            )
            .unwrap();
        s.doc_mut().feature_mut(id).unwrap().body_names = vec!["CurvedSupport".into()];
        s.refresh();
        assert!(s.model.result(id).unwrap().error.is_none());
        assert!(
            s.model.state().body("CurvedSupport").unwrap().body.planar_face(0).is_none(),
            "the native source must remain curved after STEP roundtrip"
        );
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":1})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let ids: Vec<_> = (0..f.cage.points.len()).collect();
        let positions: Vec<_> = f.cage.points.iter().map(|p| [p.x, p.y, 0.005 * p.x * p.x + 0.004 * p.y * p.y]).collect();
        s.execute("form.prototype.set_vertices", &json!({"vertices":ids,"positions":positions})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let ring = crate::doc::forms::match_openings(&f.cage, None).unwrap().remove(0);
        s.execute("form.prototype.match", &json!({"body":"CurvedSupport","face":0,"edge":[ring[0],ring[1]],"order":order}))
            .unwrap_or_else(|error| panic!("G{order} initial cornered Match: {error}"));
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.cage.vertex_creases.iter().filter(|v| v.sharpness == 10.0 && ring.contains(&v.vertex)).count(), 4);
        // Root 5 is interior to the 4Ã—4 grid. Its fine face point is
        // three control edges from the nearest rim vertex.
        let seed = refine_aligned_match_and_sculpt(&mut s, 5, order, 0.025);
        (s, seed)
    }

    fn aligned_old_seam_view(before: &crate::doc::forms::Definition, after: &crate::doc::forms::Definition) -> crate::doc::forms::Definition {
        let mut view = after.clone();
        // The actual continued evaluator/Matching basis remains authoritative.
        // Restore only original logical regions so their exact old restrictions
        // can still be requested after insertion splits those visible edges.
        view.hierarchy.as_mut().unwrap().leaves = before.hierarchy.as_ref().unwrap().leaves.clone();
        assert_eq!(view.hierarchy.as_ref().unwrap().evaluate().unwrap(), after.cage);
        view
    }

    #[test]
    fn hierarchical_loop_aligned_quarter_sculpted_cornered_support_preserves_actual_old_cad_seams() {
        for order in [1, 2] {
            let (mut s, seed) = curved_aligned_loop_match_fixture(order);
            let f = s.forms_prototype.as_ref().unwrap();
            let before = f.definition();
            let old = before.hierarchy.as_ref().unwrap();
            let plan = old.aligned_loop(seed, 0.25).unwrap();
            let refined = old.split_at(&plan.cuts).unwrap();
            assert_eq!(old.depth, 1);
            assert_eq!(refined.depth, 2, "this fixture must raise the evaluator depth");
            let old_edges: Vec<_> = old.layout().unwrap().paths.iter().map(|(edge, _)| *edge).collect();
            let old_curves = crate::doc::forms::edge_curves(&before, 0.01, &old_edges).unwrap();
            let anchors = f.surface.vertices.clone();
            let undo = f.inspect()["undo"].as_u64().unwrap();
            let expected = if order == 2 {
                solidcraft_geom::subdivision::matching::refine_g2(&before.cage, 1).unwrap()
            } else {
                before.cage.refined(1).unwrap()
            };
            if order == 2 {
                let ordinary = before.cage.refined(1).unwrap();
                assert_eq!(
                    solidcraft_geom::subdivision::matching::quadratic_extension(&before.cage).unwrap(),
                    before.cage,
                    "a rectangular native rim exercises pinned-corner G2, not a ghost ring"
                );
                assert_eq!(expected, ordinary);
            }
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.25})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let after = f.definition();
            f.verify_aligned_continuation(&before).unwrap();
            let h = after.hierarchy.as_ref().unwrap();
            assert_eq!(&h.details[..old.details.len()], old.details.as_slice());
            for (&id, &birth) in &old.controls {
                assert_eq!(h.controls.get(&id), Some(&birth));
                assert!(f.surface.vertices[id].dist(anchors[id]) < 1e-7, "G{order}: original evaluated control {id}");
            }
            for (a, b) in after.cage.points.iter().zip(&expected.points) {
                assert!(a.dist(*b) < 1e-8);
            }
            // The independent seam oracle is fitted CAD from actual old and
            // Match-continued sources, never ordinary hierarchy refinement.
            let view = aligned_old_seam_view(&before, &after);
            let next_curves = crate::doc::forms::edge_curves(&view, 0.01, &old_edges).unwrap();
            for (a, b) in old_curves.iter().zip(&next_curves) {
                for index in 0..=40 {
                    let t = index as f64 / 40.0;
                    assert!(a.at(t).unwrap().dist(b.at(t).unwrap()) < 1e-8, "G{order}: original CAD restriction {:?} t={t}", a.edge);
                }
            }
            let old_matching = before.matching.as_ref().unwrap();
            let matching = after.matching.as_ref().unwrap();
            assert_eq!(matching.source, old_matching.source);
            assert_eq!(matching.binding, old_matching.binding);
            let old_ring = crate::doc::forms::match_openings(&before.cage, Some(old)).unwrap().remove(0);
            let ring = crate::doc::forms::match_openings(&after.cage, Some(h)).unwrap().remove(0);
            for (&id, coordinate) in old_ring.iter().zip(&old_matching.coordinates) {
                let at = ring.iter().position(|&other| other == id).unwrap();
                assert_eq!(&matching.coordinates[at], coordinate);
            }
            let report = f.match_report.as_ref().unwrap();
            assert!(report.position_mm <= matching.config.tolerances.position_mm);
            assert!(report.tangent_deg <= matching.config.tolerances.tangent_deg);
            if order == 2 {
                assert!(report.curvature_relative <= matching.config.tolerances.curvature_relative);
            }
            assert_eq!(f.inspect()["undo"].as_u64(), Some(undo + 1));
            let restored = crate::recovery::restore_bytes(&crate::recovery::capture_bytes(&s).unwrap(), None).unwrap();
            assert_eq!(restored.forms_prototype.as_ref().unwrap().definition(), after);
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        }
    }

    #[test]
    fn aligned_tensor_coefficient_certificate_covers_interior_without_sampling_and_keeps_rational_basis_exact() {
        use solidcraft_geom::bspline::NurbsSurface;
        let points: Vec<_> = (0..4).flat_map(|v| (0..4).map(move |u| Vec3::new(u as f64, v as f64, 0.0))).collect();
        let mut weights = vec![1.0; 16];
        weights[5] = 2.0;
        let knots = [vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]];
        let original = NurbsSurface::new([3, 3], knots.clone(), &points, &weights).unwrap();
        assert!(crate::forms_prototype::Form::aligned_surface_difference_bound(&original, &original).unwrap() < 1e-12);
        let mut changed = points.clone();
        changed[5].z = 0.02;
        let changed = NurbsSurface::new([3, 3], knots.clone(), &changed, &weights).unwrap();
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            assert_eq!(original.eval(u, v).unwrap(), changed.eval(u, v).unwrap());
        }
        let bound = crate::forms_prototype::Form::aligned_surface_difference_bound(&original, &changed).unwrap();
        assert!((0.02..0.020000000001).contains(&bound), "the coefficient certificate is uniform even when all corner witnesses agree");
        assert!(original.eval(0.4, 0.4).unwrap().dist(changed.eval(0.4, 0.4).unwrap()) > 0.001);
        let mut changed_weights = weights.clone();
        changed_weights[5] = f64::from_bits(changed_weights[5].to_bits() + 1);
        let changed_weight = NurbsSurface::new([3, 3], knots.clone(), &points, &changed_weights).unwrap();
        assert!(
            crate::forms_prototype::Form::aligned_surface_difference_bound(&original, &changed_weight).is_err(),
            "nearby rational weights are not the same basis"
        );
        let reparameterized = NurbsSurface::new([3, 3], [knots[0].iter().map(|u| u + 2.0).collect(), knots[1].clone()], &points, &weights).unwrap();
        assert!(crate::forms_prototype::Form::aligned_surface_difference_bound(&original, &reparameterized).is_err());
    }

    fn smooth_circular_aligned_match_fixture(order: usize) -> (Session, [usize; 2]) {
        use solidcraft_geom::subdivision::primitives::Parameters;
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius":1,"height":1,"body_name":"RoundSupport"})).unwrap();
        let body = s.model.state().body("RoundSupport").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&face| body.planar_face(face).is_some_and(|plane| plane.normal().dot(Vec3::Z) > 0.99)).unwrap();
        let parameters = Parameters { kind: "cylinder".into(), dimensions: [2.0; 3], segments: [2, 3, 1], ..Default::default() };
        let closed = parameters.build().unwrap();
        let removed: Vec<_> =
            closed.faces.iter().enumerate().filter_map(|(id, face)| face.iter().all(|&v| closed.points[v].z == 1.0).then_some(id)).collect();
        let cage = closed.delete_faces(&removed).unwrap();
        let ring = cage.boundary_loops().unwrap().remove(0);
        assert_eq!(ring.len(), 16);
        let definition = crate::doc::forms::Definition {
            cage,
            parameters,
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            symmetry: None,
            hierarchy: None,
            placement: Default::default(),
        };
        s.forms_prototype = Some(crate::forms_prototype::Form::from_definition(definition, 0, None).unwrap());
        s.execute("form.prototype.match", &json!({"body":"RoundSupport","face":top,"edge":[ring[0],ring[1]],"order":order})).unwrap();
        if order == 2 {
            let f = s.forms_prototype.as_ref().unwrap();
            assert!(
                solidcraft_geom::subdivision::matching::quadratic_extension(&f.cage).unwrap().points.len() > f.cage.points.len(),
                "the circular opening must really use ghost continuation"
            );
        }
        // Root 0 starts at the top opening. Its first non-rim child can
        // still be one of the controls that Match solves. Use the centre of
        // a separate coarse face entirely beyond the first inward row.
        let f = s.forms_prototype.as_ref().unwrap();
        let rim = crate::doc::forms::match_openings(&f.cage, None).unwrap().remove(0);
        let band = aligned_match_seam_band(&f.cage, &rim, 1);
        let sculpt_face = f.cage.faces.iter().enumerate().find(|(id, face)| *id > 1 && face.iter().all(|v| !band.contains(v))).unwrap().0;
        let seed = refine_aligned_match_and_sculpt(&mut s, sculpt_face, order, 0.015);
        (s, seed)
    }
    #[test]
    fn hierarchical_loop_aligned_smooth_circular_g2_quarter_preserves_real_continuation_and_old_cad_seams() {
        for order in [1, 2] {
            let (mut s, seed) = smooth_circular_aligned_match_fixture(order);
            let f = s.forms_prototype.as_ref().unwrap();
            let before = f.definition();
            let old = before.hierarchy.as_ref().unwrap();
            let plan = old.aligned_loop(seed, 0.25).unwrap();
            let proposed = old.split_at(&plan.cuts).unwrap();
            assert_eq!((old.depth, proposed.depth), (1, 2));
            let expected = if order == 2 {
                solidcraft_geom::subdivision::matching::refine_g2(&before.cage, 1).unwrap()
            } else {
                before.cage.refined(1).unwrap()
            };
            if order == 2 {
                assert!(expected.points.iter().zip(before.cage.refined(1).unwrap().points).any(|(a, b)| a.dist(b) > 1e-10));
            }
            let edges: Vec<_> = old.layout().unwrap().paths.iter().map(|(edge, _)| *edge).collect();
            let curves = crate::doc::forms::edge_curves(&before, 0.01, &edges).unwrap();
            let undo = f.inspect()["undo"].as_u64().unwrap();
            s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.25})).unwrap();
            let f = s.forms_prototype.as_ref().unwrap();
            let after = f.definition();
            f.verify_aligned_continuation(&before).unwrap();
            assert_eq!(&after.hierarchy.as_ref().unwrap().details[..old.details.len()], old.details.as_slice());
            assert_eq!(crate::doc::forms::match_openings(&after.cage, after.hierarchy.as_ref()).unwrap()[0].len(), 64);
            let view = aligned_old_seam_view(&before, &after);
            let actual = crate::doc::forms::edge_curves(&view, 0.01, &edges).unwrap();
            for (a, b) in curves.iter().zip(&actual) {
                for index in 0..=40 {
                    let t = index as f64 / 40.0;
                    let residual = a.at(t).unwrap().dist(b.at(t).unwrap());
                    assert!(residual < 1e-8, "smooth G{order}: original CAD restriction {:?} t={t}, residual {residual:.12} mm", a.edge);
                }
            }
            let old_matching = before.matching.as_ref().unwrap();
            let matching = after.matching.as_ref().unwrap();
            assert_eq!(matching.source, old_matching.source);
            assert_eq!(matching.binding, old_matching.binding);
            let old_ring = crate::doc::forms::match_openings(&before.cage, Some(old)).unwrap().remove(0);
            let ring = crate::doc::forms::match_openings(&after.cage, after.hierarchy.as_ref()).unwrap().remove(0);
            for (&id, coordinate) in old_ring.iter().zip(&old_matching.coordinates) {
                let at = ring.iter().position(|&v| v == id).unwrap();
                assert_eq!(&matching.coordinates[at], coordinate);
            }
            assert!(f.match_report.as_ref().unwrap().meets_tolerances(&matching.config));
            let restored = crate::recovery::restore_bytes(&crate::recovery::capture_bytes(&s).unwrap(), None).unwrap();
            let saved = restored.forms_prototype.as_ref().unwrap().definition();
            assert_eq!(saved, after);
            let state = match_state(&restored).unwrap();
            let (recomputed, report) = saved.matching.as_ref().unwrap().recompute(&saved, &state, 0.01).unwrap();
            assert_eq!(recomputed, saved, "unchanged support must retain the authored CAD chart after reopening");
            assert!(report.meets_tolerances(&saved.matching.as_ref().unwrap().config));
            if order == 1 {
                // An actual fine detail changes the current evaluator rather
                // than being hidden by the old fitted surface recipe.
                let mut sculpted = saved.clone();
                let hierarchy = sculpted.hierarchy.as_ref().unwrap();
                let rim = crate::doc::forms::match_openings(&sculpted.cage, Some(hierarchy)).unwrap().remove(0);
                let band = aligned_match_seam_band(&sculpted.cage, &rim, 2);
                let vertex = (before.cage.points.len()..sculpted.cage.points.len()).find(|id| !band.contains(id)).unwrap();
                sculpted.cage.points[vertex].z += 0.005;
                sculpted.hierarchy = Some(hierarchy.with_evaluator(&sculpted.cage).unwrap());
                let (updated, _) = sculpted.matching.as_ref().unwrap().recompute(&sculpted, &state, 0.01).unwrap();
                assert!(updated.cage.points[vertex].dist(sculpted.cage.points[vertex]) < 1e-8);
                let saved_match = serde_json::to_value(updated.matching.as_ref().unwrap()).unwrap();
                assert!(saved_match.get("continued_surface").is_none(), "later fine sculpt uses a newly qualified current evaluator fit");
            }
            assert_eq!(f.inspect()["undo"].as_u64(), Some(undo + 1));
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        }
    }

    #[test]
    fn hierarchical_loop_aligned_preservation_guard_rejects_an_unselected_old_control_correction() {
        let (s, seed) = aligned_loop_fixture();
        let f = s.forms_prototype.as_ref().unwrap();
        let before = f.definition();
        let h = before.hierarchy.as_ref().unwrap();
        let plan = h.aligned_loop(seed, 0.25).unwrap();
        let refined = h.split_at(&plan.cuts).unwrap();
        let mut after = before.clone();
        after.cage = refined.evaluate().unwrap();
        after.hierarchy = Some(refined);
        let candidate = crate::forms_prototype::Form::from_definition(after.clone(), 0, None).unwrap();
        candidate.verify_aligned_continuation(&before).unwrap();
        let inserted: std::collections::BTreeSet<_> =
            candidate.hierarchy.as_ref().unwrap().aligned_loop_vertices(&plan.points).unwrap().into_iter().collect();
        let old = *h.controls.keys().find(|id| !inserted.contains(id)).unwrap();
        after.cage.points[old] += Vec3::Z * 0.001;
        after.hierarchy = Some(after.hierarchy.as_ref().unwrap().with_evaluator(&after.cage).unwrap());
        let moved = crate::forms_prototype::Form::from_definition(after, 0, None).unwrap();
        assert!(moved.verify_aligned_continuation(&before).is_err(), "a passing target residual cannot hide another original control moving");
    }

    #[test]
    fn hierarchical_loop_aligned_conflicting_mirror_and_subgrid_fraction_refuse_atomically() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"box","segments":[1,1,1],"levels":0})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let seed = {
            let c = f.hierarchy.as_ref().unwrap().leaf_corners().unwrap()[4].unwrap();
            [c[0], c[1]]
        };
        assert!(f.logical.faces.iter().any(|face| face.len() > 4));
        let before = f.definition();
        let inspect = f.inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        for fraction in [0.25, 1.0 / 32.0, 0.0, 1.0] {
            assert!(s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":fraction})).is_err());
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), inspect);
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
        }
        // A refusal must not poison the next supported self-mirrored station.
        s.execute("form.prototype.insert_loop", &json!({"edge":seed,"fraction":0.5})).unwrap();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
    }
    fn insert_records(form: &crate::forms_prototype::Form, face: usize) -> (Value, usize, usize) {
        let h = form.hierarchy.clone().unwrap_or_else(|| solidcraft_geom::subdivision::hierarchy::Hierarchy::new(form.cage.clone()).unwrap());
        let f = &form.logical.faces[face];
        let edges: Vec<_> = f.iter().zip(f.iter().cycle().skip(1)).take(f.len()).map(|(&a, &b)| [a, b]).collect();
        for &first in &edges {
            for &second in &edges {
                if let Ok((parent, axis)) = h.insert_target(first, second) {
                    return (json!({"points":[{"edge":first,"fraction":0.3},{"edge":second,"fraction":0.65}]}), parent, axis);
                }
            }
        }
        panic!("fixture has no supported opposite-edge insertion route")
    }
    #[test]
    fn exact_insert_points_ignore_preview_quality_and_keep_reverse_parameter_undo_and_reopen() {
        use solidcore_record::Vec3;
        let mut outputs = Vec::new();
        for (levels, reverse) in [(0, false), (3, false), (3, true)] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":levels})).unwrap();
            let p = s.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::new(2.0, 0.0, 3.0);
            s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z]]})).unwrap();
            let before = s.forms_prototype.as_ref().unwrap().definition();
            let depth = s.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
            let (mut parameters, parent, axis) = insert_records(s.forms_prototype.as_ref().unwrap(), 0);
            let records: params::InsertPoints = serde_json::from_value(parameters.clone()).unwrap();
            let curves = crate::doc::forms::edge_curves(&before, 0.01, &records.points.iter().map(|p| p.edge).collect::<Vec<_>>()).unwrap();
            let targets: Vec<_> = curves.iter().zip(&records.points).map(|(curve, record)| curve.at(record.fraction).unwrap()).collect();
            if reverse {
                for record in parameters["points"].as_array_mut().unwrap() {
                    record["edge"].as_array_mut().unwrap().reverse();
                }
            }
            s.execute("form.prototype.insert_points", &parameters).unwrap();
            let form = s.forms_prototype.as_ref().unwrap();
            assert_eq!(form.inspect()["undo"].as_u64().unwrap(), depth + 1);
            let endpoints = form.hierarchy.as_ref().unwrap().seam_endpoints(parent, axis).unwrap();
            for target in targets {
                assert!(endpoints.iter().any(|&id| form.surface.vertices[id].dist(target) < 1e-8));
            }
            let after = form.definition();
            assert_eq!(after.cage.faces.len(), 24);
            assert_eq!(after.cage.points.len(), 26);
            assert_eq!(after.hierarchy.as_ref().unwrap().leaves.iter().filter(|leaf| leaf.active).count(), 7);
            outputs.push(after.clone());
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
            let feature = s.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
            assert!(s.model.result(feature).unwrap().error.is_none());
            let saved = solidcraft_io::write_design(s.doc.as_ref());
            let mut reopened = Session::new(solidcraft_io::read_design(&saved).unwrap());
            reopened.execute("form.edit", &json!({"feature":feature})).unwrap();
            assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), after);
        }
        for other in &outputs[1..] {
            assert_eq!(outputs[0].cage.faces, other.cage.faces);
            for (a, b) in outputs[0].cage.points.iter().zip(&other.cage.points) {
                assert!(a.dist(*b) < 1e-8, "preview quality changed authored insertion geometry");
            }
            assert_eq!(outputs[0].hierarchy.as_ref().unwrap().leaves, other.hierarchy.as_ref().unwrap().leaves);
        }
    }
    #[test]
    fn exact_insert_points_preserve_original_matched_basis_coordinates_and_fail_atomically() {
        for order in [0, 1, 2] {
            for refined in [false, true] {
                let (mut s, face, _) = matched_extrusion_case(order, refined);
                let before = s.forms_prototype.as_ref().unwrap().definition();
                let matching = before.matching.as_ref().unwrap();
                let coordinates = matching.coordinates.clone();
                let depth = s.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
                let (parameters, parent, axis) = insert_records(s.forms_prototype.as_ref().unwrap(), face);
                let records: params::InsertPoints = serde_json::from_value(parameters.clone()).unwrap();
                let original_curves =
                    crate::doc::forms::edge_curves(&before, 0.01, &records.points.iter().map(|p| p.edge).collect::<Vec<_>>()).unwrap();
                let targets: Vec<_> = original_curves.iter().zip(&records.points).map(|(curve, point)| curve.at(point.fraction).unwrap()).collect();
                s.execute("form.prototype.insert_points", &parameters).unwrap();
                let after = s.forms_prototype.as_ref().unwrap();
                assert_eq!(after.inspect()["undo"].as_u64().unwrap(), depth + 1);
                assert!(after.match_report.as_ref().unwrap().position_mm <= matching.config.tolerances.position_mm);
                let updated = after.matching.as_ref().unwrap();
                let endpoints = after.hierarchy.as_ref().unwrap().seam_endpoints(parent, axis).unwrap();
                for target in targets {
                    assert!(
                        endpoints.iter().any(|&id| after.surface.vertices[id].dist(target) <= 0.01),
                        "Match must retain the authored insertion endpoint within the CAD tolerance"
                    );
                }
                // Refinement can add stations, but original named curve coordinates remain.
                for coordinate in &coordinates {
                    assert!(updated.coordinates.contains(coordinate));
                }
                let valid = after.inspect();
                let recovery = crate::recovery::capture_bytes(&s).unwrap();
                assert!(
                    s.execute(
                        "form.prototype.insert_points",
                        &json!({"points":[{"edge":[99999,99998],"fraction":0.3},{"edge":[99997,99996],"fraction":0.65}]})
                    )
                    .is_err()
                );
                assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), valid);
                assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
                let after = s.forms_prototype.as_ref().unwrap().definition();
                s.execute("form.prototype.undo", &json!({})).unwrap();
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
                s.execute("form.prototype.redo", &json!({})).unwrap();
                assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
            }
        }
    }
    #[test]
    fn exact_insert_points_refuse_endpoint_nonfinite_and_malformed_records_atomically() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1]})).unwrap();
        let face = s.forms_prototype.as_ref().unwrap().cage.faces[0].clone();
        let first = [face[0], face[1]];
        let second = [face[2], face[3]];
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let recovery = crate::recovery::capture_bytes(&s).unwrap();
        for fraction in [0.0, 1.0, f64::NAN, f64::INFINITY, -0.1] {
            assert!(s.forms_prototype.as_mut().unwrap().insert_points(&[(first, fraction), (second, 0.65)], None, 0.01).is_err());
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
        }
        for points in [json!([]), json!([{"edge":first,"fraction":0.3}]), json!([{"edge":first,"fraction":0.3},{"edge":first,"fraction":0.65}])] {
            assert!(s.execute("form.prototype.insert_points", &json!({"points":points})).is_err());
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        }
        assert!(
            s.execute(
                "form.prototype.insert_points",
                &json!({"points":[{"edge":first,"fraction":0.3},{"edge":second,"fraction":0.65}],"tolerance":0.0})
            )
            .is_err()
        );
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        s.execute("form.prototype.topology", &json!({"tool":"dissolve","edges":[first]})).unwrap();
        let dissolved = s.forms_prototype.as_ref().unwrap().inspect();
        let form = s.forms_prototype.as_ref().unwrap();
        let ngon = form.logical.faces.iter().find(|face| face.len() > 4).unwrap();
        let first = [ngon[0], ngon[1]];
        let second = [ngon[2], ngon[3]];
        assert!(form.cage.faces.iter().all(|face| face.len() == 4), "logical dissolution retains the quad evaluator");
        let curves = crate::forms_prototype::insertion_curves(&s, 0.01, &[first, second]).unwrap();
        assert_eq!(curves.len(), 2);
        assert!(curves.iter().all(|curve| curve.at(0.3).unwrap().is_finite()));
        assert!(
            s.execute("form.prototype.insert_points", &json!({"points":[{"edge":first,"fraction":0.3},{"edge":second,"fraction":0.65}]})).is_err()
        );
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), dissolved, "ordinary n-gon source queries must not broaden Insert Point topology");
    }
    #[test]
    fn exact_insert_points_follow_support_recompute_before_insertion_and_after_saved_reopen() {
        let (mut s, face, _) = matched_extrusion_case(1, false);
        let support = s.model.state().bodies[0].feature;
        let original = s.forms_prototype.as_ref().unwrap().definition();
        let (parameters, _, _) = insert_records(s.forms_prototype.as_ref().unwrap(), face);
        let coordinates = original.matching.as_ref().unwrap().coordinates.clone();
        let set_height = |session: &mut Session, height: &str| {
            let feature = session.doc_mut().features.iter_mut().find(|feature| feature.id == support).unwrap();
            let crate::doc::FeatureKind::Box { height: stored, .. } = &mut feature.kind else { panic!("support box") };
            *stored = height.into();
            session.refresh();
        };
        set_height(&mut s, "1.2 mm");
        s.execute("form.prototype.insert_points", &parameters).unwrap();
        let inserted = s.forms_prototype.as_ref().unwrap().definition();
        for coordinate in &coordinates {
            assert!(inserted.matching.as_ref().unwrap().coordinates.contains(coordinate));
        }
        assert!(s.forms_prototype.as_ref().unwrap().match_report.as_ref().unwrap().position_mm <= 0.001);
        let feature = s.execute("form.prototype.commit", &json!({"operation":"new","tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        let mut s = Session::new(solidcraft_io::read_design(&solidcraft_io::write_design(s.doc.as_ref())).unwrap());
        set_height(&mut s, "1.4 mm");
        assert!(s.model.result(feature).unwrap().error.is_none());
        let body = s.model.state().bodies.iter().find(|body| body.feature == feature).unwrap().body.clone();
        let edges: std::collections::BTreeSet<_> = (0..body.face_count())
            .flat_map(|face| crate::kernel::face_boundaries(&body, face).unwrap().into_iter().flatten())
            .filter_map(|edge| edge.neighbor.is_none().then_some(edge.edge))
            .collect();
        for curve in crate::kernel::edge_geometry(&body, &edges.into_iter().collect::<Vec<_>>()).unwrap() {
            for t in [0.0, 0.3, 0.65, 1.0] {
                assert!((curve.at(t).unwrap().0.z - 0.4).abs() < 0.002);
            }
        }
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().hierarchy.is_some());
    }

    #[test]
    fn surface_extrusion_targets_evaluated_caps_absolutely_and_preserves_one_history_action() {
        use solidcore_record::Vec3;
        for (whole, hierarchical, placed) in [(false, false, false), (true, false, false), (false, true, false), (false, true, true)] {
            let mut s = Session::default();
            s.execute(
                "form.prototype.create",
                &json!({"primitive":"face","dimensions":[12,12,1],"segments":if whole {[1,1,1]} else {[2,2,1]},"levels":1}),
            )
            .unwrap();
            let face = if hierarchical {
                s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
                let form = s.forms_prototype.as_ref().unwrap();
                let id = form.hierarchy.as_ref().unwrap().base.faces.len();
                let vertex = form.logical.faces[id][0];
                let p = form.surface.vertices[vertex] + Vec3::Z * 0.03;
                s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]]})).unwrap();
                id
            } else {
                0
            };
            if placed {
                let matrix = crate::doc::rigid(Vec3::new(11.0, -7.0, 9.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.6);
                s.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
            }
            let form = s.forms_prototype.as_ref().unwrap();
            let before = form.definition();
            let recovery = crate::recovery::capture_bytes(&s).unwrap();
            let undo = form.inspect()["undo"].as_u64().unwrap();
            let anchors: Vec<_> = form.logical.faces[face].iter().map(|&id| form.surface.vertices[id]).collect();
            let delta = form.placement.vector(Vec3::new(0.2, -0.1, 1.0));
            assert!(
                s.execute("form.prototype.extrude_drag", &json!({"surface":true,"faces":[face],"displacement":[0,0,1],"checkpoint":false})).is_err()
            );
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
            for (checkpoint, distance) in [(true, 2.0), (false, 4.0)] {
                let d = delta * distance;
                s.execute(
                    "form.prototype.extrude_drag",
                    &json!({"surface":true,"faces":[face],"displacement":[d.x,d.y,d.z],"checkpoint":checkpoint}),
                )
                .unwrap();
                let form = s.forms_prototype.as_ref().unwrap();
                for (&id, &anchor) in form.logical.faces[face].iter().zip(&anchors) {
                    assert!(form.surface.vertices[id].dist(anchor + d) < 1e-7, "whole={whole}, hierarchy={hierarchical}, placement={placed}");
                }
                assert_eq!(form.cage.faces.len(), before.cage.faces.len() + 4);
                assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
                assert_eq!(form.placement, before.placement);
            }
            let after = s.forms_prototype.as_ref().unwrap().definition();
            let accepted = crate::recovery::capture_bytes(&s).unwrap();
            for params in [
                json!({"surface":true,"faces":[99999],"displacement":[0,0,5],"checkpoint":false}),
                json!({"faces":[face],"displacement":[0,0,5],"checkpoint":false}),
                json!({"surface":true,"faces":[face],"displacement":[0,0,0],"checkpoint":false}),
            ] {
                assert!(s.execute("form.prototype.extrude_drag", &params).is_err());
                assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), accepted);
            }
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
            s.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
            let decoded = serde_json::from_slice(&serde_json::to_vec(&after).unwrap()).unwrap();
            assert_eq!(crate::forms_prototype::Form::from_definition(decoded, 1, None).unwrap().definition(), after);
        }
        // Omission keeps the existing coefficient-space programmatic contract.
        let args: params::ExtrudeDrag = serde_json::from_value(json!({"faces":[0],"displacement":[0,0,2]})).unwrap();
        assert!(!args.surface);
        let mut raw = Session::default();
        raw.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":1})).unwrap();
        let before = raw.forms_prototype.as_ref().unwrap().cage.clone();
        raw.execute("form.prototype.extrude_drag", &json!({"faces":[0],"displacement":[0,0,2]})).unwrap();
        assert_eq!(raw.forms_prototype.as_ref().unwrap().cage, before.extrude_faces(&[0], Vec3::Z * 2.0).unwrap());
    }

    #[test]
    fn oblique_surface_extrusion_retains_coarse_box_cap_profile_and_converts_to_cad() {
        use solidcore_record::Vec3;
        use std::collections::BTreeSet;

        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"box","dimensions":[20,20,20],"segments":[1,1,1],"levels":2})).unwrap();
        let original = s.forms_prototype.as_ref().unwrap();
        let before = original.definition();
        let before_recovery = crate::recovery::capture_bytes(&s).unwrap();
        let evaluator = before.cage.refined(1).unwrap();
        let source_points = evaluator.limit_vertices().unwrap();
        // The first subdivision appends edge points and then face centres.
        // Cell slots 0..4 are the four children of the original top face.
        let centre = before.cage.points.len() + before.cage.edges().unwrap().len();
        let cap_grid: BTreeSet<_> = evaluator.faces[..4].iter().flatten().copied().collect();
        assert_eq!(cap_grid.len(), 9);
        assert!(cap_grid.contains(&centre));
        assert!(source_points[centre].dist(Vec3::new(0.0, 0.0, 680.0 / 81.0)) < 1e-12);

        // Actual displacement captured from the native Shift gesture.
        let target = Vec3::new(0.9044602749883381, 2.1739930145698736, 1.2695327395815355);
        let first = target + Vec3::Z * 0.4;
        let ghost = original.prepare_surface_extrusion_preview(&[0], &[], first, true).unwrap().preview(target).unwrap();
        let axis = original.prepare_surface_extrusion_preview_on_axis(&[0], &[], first, true, Vec3::Z).unwrap();
        let axis_preview = axis.preview(target).unwrap();
        assert_eq!(axis_preview.mesh.triangles, ghost.mesh.triangles);
        assert_eq!(axis_preview.mesh.tri_face, ghost.mesh.tri_face);
        assert!(axis_preview.mesh.positions.iter().zip(&ghost.mesh.positions).all(|(a, b)| a.dist(*b) < 1e-9));
        assert!(axis.preview(target + Vec3::X * 0.02).is_err());
        assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), before_recovery);
        let mut first_topology = None;
        for (checkpoint, displacement) in [(true, first), (false, target)] {
            s.execute(
                "form.prototype.extrude_drag",
                &json!({"surface":true,"faces":[0],"displacement":[displacement.x,displacement.y,displacement.z],"checkpoint":checkpoint}),
            )
            .unwrap();
            let form = s.forms_prototype.as_ref().unwrap();
            let hierarchy = form.hierarchy.as_ref().unwrap();
            assert_eq!(hierarchy.base, before.cage);
            assert_eq!(hierarchy.depth, 1);
            assert!(hierarchy.details.iter().all(Vec::is_empty), "the authored base and its original levels stay intact");
            assert_eq!(form.logical.faces.len(), 14);
            assert_eq!(form.cage.faces.len(), 32);
            assert!(!hierarchy.controls.contains_key(&centre), "the cap centre is not a new selectable control");
            assert_eq!(form.inspect()["undo"], 1);
            for (face, cells) in hierarchy.layout().unwrap().owners[..24].as_chunks::<4>().0.iter().enumerate() {
                assert!(cells.iter().all(|owner| *owner == Some(face)), "every original logical face retains its four cell owners");
                if face != 0 {
                    let corners = &before.cage.faces[face];
                    let retained: Vec<_> = form.logical.faces[face].iter().copied().filter(|id| corners.contains(id)).collect();
                    assert_eq!(&retained, corners, "neighbors retain their original boundary corner identities");
                }
            }
            let cap: BTreeSet<_> = form.cage.faces[..4].iter().flatten().copied().collect();
            for (id, &point) in evaluator.points.iter().enumerate() {
                if !cap.contains(&id) {
                    assert_eq!(form.cage.points[id], point, "existing wall-base and neighboring coefficients stay in place");
                }
            }
            for (old, new) in evaluator.faces[..4].iter().zip(&form.cage.faces[..4]) {
                for (&old, &new) in old.iter().zip(new) {
                    let expected = source_points[old] + displacement;
                    assert!(form.surface.vertices[new].dist(expected) < 1e-7, "retain every cap grid station, including its centre");
                    if !checkpoint {
                        assert!(ghost.vertices[new].dist(expected) < 1e-7, "the same affine cap response drives the ghost and authored continuation");
                    }
                }
            }
            if let Some(topology) = &first_topology {
                assert_eq!(&form.cage.faces, topology);
                assert_eq!(ghost.mesh.triangles, form.surface.mesh.triangles);
                assert_eq!(ghost.mesh.tri_face, form.surface.mesh.tri_face);
            } else {
                first_topology = Some(form.cage.faces.clone());
            }
        }

        let after = s.forms_prototype.as_ref().unwrap().definition();
        for tolerance in [0.05, 0.01] {
            let (body, report) = crate::doc::forms::convert(&after, tolerance).unwrap();
            assert!(report.solid && !body.is_surface() && !body.is_mesh());
            assert!(report.sampled_deviation_mm <= tolerance);
            let owners: BTreeSet<_> = report.patch_roots.into_iter().collect();
            assert_eq!(owners, (0..14).collect(), "CAD conversion keeps all original and new logical face owners");
        }
        s.execute("form.prototype.undo", &json!({})).unwrap();
        let restored = s.forms_prototype.as_ref().unwrap();
        assert_eq!(restored.definition(), before);
        assert!(restored.hierarchy.is_none(), "one Undo restores the original base cage");
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        let finished = s.execute("form.prototype.commit", &json!({"operation":"new","tolerance":0.01})).unwrap();
        let feature = finished["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().error.is_none());
        assert!(s.forms_prototype.is_none());
    }

    #[test]
    fn surface_boundary_extrusion_maps_translated_rim_to_original_evaluated_points() {
        use solidcore_record::Vec3;
        for hierarchical in [false, true] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[12,12,1],"segments":[2,2,1],"levels":1})).unwrap();
            if hierarchical {
                s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
            }
            let form = s.forms_prototype.as_ref().unwrap();
            let before = form.definition();
            let anchors = form.surface.vertices.clone();
            let ring = form.cage.boundary_loops().unwrap().remove(0);
            let rim: std::collections::BTreeSet<_> = ring.iter().zip(ring.iter().cycle().skip(1)).map(|(&a, &b)| [a.min(b), a.max(b)]).collect();
            let edge = if let Some(h) = &form.hierarchy {
                h.layout()
                    .unwrap()
                    .paths
                    .into_iter()
                    .find(|(_, path)| path.len() > 2 && path.windows(2).all(|p| rim.contains(&[p[0].min(p[1]), p[0].max(p[1])])))
                    .unwrap()
                    .0
            } else {
                *rim.first().unwrap()
            };
            let undo = form.inspect()["undo"].as_u64().unwrap();
            for (checkpoint, z) in [(true, 1.0), (false, 3.0)] {
                s.execute("form.prototype.extrude_drag", &json!({"surface":true,"edges":[edge],"displacement":[0,0,z],"checkpoint":checkpoint}))
                    .unwrap();
                let form = s.forms_prototype.as_ref().unwrap();
                for wall in form.cage.faces.iter().skip(before.cage.faces.len()) {
                    for (old, new) in [(wall[0], wall[3]), (wall[1], wall[2])] {
                        assert!(form.surface.vertices[new].dist(anchors[old] + Vec3::Z * z) < 1e-7);
                    }
                }
                assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
            }
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        }
    }
    #[test]
    fn surface_extrusion_keeps_mirrored_component_motion_and_projects_joined_seams() {
        use solidcore_record::Vec3;
        for (joined, edge_mode, placed) in [(false, false, false), (false, true, true), (true, false, true), (true, true, false)] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[16,12,1],"segments":[4,3,1],"levels":1})).unwrap();
            s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
            if placed {
                let matrix = crate::doc::rigid(Vec3::new(7.0, 3.0, -4.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.5);
                s.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
            }
            let form = s.forms_prototype.as_ref().unwrap();
            let before = form.definition();
            let undo = form.inspect()["undo"].as_u64().unwrap();
            let anchors = form.surface.vertices.clone();
            let face = form
                .logical
                .faces
                .iter()
                .position(|f| {
                    f.iter().all(|&id| form.cage.points[id].x >= if joined { 0.0 } else { 4.0 })
                        && (!joined || f.iter().any(|&id| form.cage.points[id].x == 0.0))
                })
                .unwrap();
            let ring = form.cage.boundary_loops().unwrap().remove(0);
            let edge = ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .find_map(|(&a, &b)| {
                    let x = [form.cage.points[a].x, form.cage.points[b].x];
                    (x.iter().all(|&x| x >= if joined { 0.0 } else { 4.0 }) && (!joined || x.contains(&0.0))).then_some([a, b])
                })
                .unwrap();
            let original_faces = form.logical.faces.clone();
            let selected = form.mirror_face_selection(&[face]).unwrap();
            for (checkpoint, z) in [(true, 1.0), (false, 3.0)] {
                let delta = before.placement.vector(Vec3::new(0.5, 0.0, z));
                s.execute("form.prototype.extrude_drag", &json!({"surface":true,"faces":if edge_mode {vec![]} else {vec![face]},"edges":if edge_mode {vec![edge]} else {vec![]},"displacement":[delta.x,delta.y,delta.z],"checkpoint":checkpoint})).unwrap();
                let form = s.forms_prototype.as_ref().unwrap();
                let assert_pair = |old: usize, new: usize| {
                    let x = if joined {
                        0.0
                    } else if before.cage.points[old].x > 0.0 {
                        0.5
                    } else {
                        -0.5
                    };
                    let expected = anchors[old] + before.placement.vector(Vec3::new(x, 0.0, z));
                    assert!(form.surface.vertices[new].dist(expected) < 1e-7, "joined={joined}, edge={edge_mode}, placed={placed}");
                };
                if edge_mode {
                    for wall in form.cage.faces.iter().skip(before.cage.faces.len()) {
                        assert_pair(wall[0], wall[3]);
                        assert_pair(wall[1], wall[2]);
                    }
                } else {
                    for &id in &selected {
                        for (&old, &new) in original_faces[id].iter().zip(&form.logical.faces[id]) {
                            assert_pair(old, new);
                        }
                    }
                }
                form.symmetry.as_ref().unwrap().validate_reflection(&form.cage).unwrap();
                assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
            }
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        }
    }
    #[test]
    fn surface_extrusion_qualifies_match_and_feedback_without_moving_original_native_stations() {
        use solidcore_record::Vec3;
        for order in [0, 1, 2] {
            let (mut s, face, _) = matched_extrusion_case(order, false);
            let form = s.forms_prototype.as_ref().unwrap();
            let before = form.definition();
            let recovery = crate::recovery::capture_bytes(&s).unwrap();
            let original_ids = form.logical.faces[face].clone();
            let anchors: Vec<_> = original_ids.iter().map(|&id| form.surface.vertices[id]).collect();
            let undo = form.inspect()["undo"].as_u64().unwrap();
            let feedback = form.prepare_surface_extrusion_preview(&[face], &[], Vec3::Z * 0.2, true).unwrap();
            let preview = feedback.preview(Vec3::Z * 0.8).unwrap();
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
            for (checkpoint, z) in [(true, 0.4), (false, 0.8)] {
                s.execute("form.prototype.extrude_drag", &json!({"surface":true,"faces":[face],"displacement":[0,0,z],"checkpoint":checkpoint}))
                    .unwrap();
                let form = s.forms_prototype.as_ref().unwrap();
                for (&id, &point) in form.logical.faces[face].iter().zip(&anchors) {
                    assert!(form.surface.vertices[id].dist(point + Vec3::Z * z) < 1e-7, "G{order}");
                    if !checkpoint {
                        assert!(preview.vertices[id].dist(point + Vec3::Z * z) < 1e-7, "G{order} ghost");
                    }
                }
                let matching = form.matching.as_ref().unwrap();
                let old = before.matching.as_ref().unwrap();
                assert_eq!(matching.coordinates, old.coordinates);
                assert_eq!(matching.source, old.source);
                assert_eq!(matching.binding, old.binding);
                assert_eq!(form.placement, before.placement);
                let report = form.match_report.as_ref().unwrap();
                assert!(report.position_mm <= matching.config.tolerances.position_mm);
                if order > 0 {
                    assert!(report.tangent_deg <= matching.config.tolerances.tangent_deg);
                }
                if order > 1 {
                    assert!(report.curvature_relative <= matching.config.tolerances.curvature_relative);
                }
                assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
            }
            s.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        }
    }
    #[test]
    fn surface_extrusion_feedback_follows_evaluated_targets_without_authoring_history() {
        use solidcore_record::Vec3;
        for placed in [false, true] {
            let mut s = Session::default();
            s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[12,12,1],"segments":[2,2,1],"levels":1})).unwrap();
            if placed {
                let matrix = crate::doc::rigid(Vec3::new(31.0, -17.0, 29.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.7);
                s.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
            }
            let form = s.forms_prototype.as_ref().unwrap();
            let recovery = crate::recovery::capture_bytes(&s).unwrap();
            let anchors: Vec<_> = form.logical.faces[0].iter().map(|&id| form.surface.vertices[id]).collect();
            let a = form.placement.vector(Vec3::Z * 0.2);
            let b = form.placement.vector(Vec3::new(0.3, -0.1, 2.0));
            let response = form.prepare_surface_extrusion_preview(&[0], &[], a, true).unwrap();
            let preview = response.preview(b).unwrap();
            assert_eq!(crate::recovery::capture_bytes(&s).unwrap(), recovery);
            s.execute("form.prototype.extrude_drag", &json!({"surface":true,"faces":[0],"displacement":[b.x,b.y,b.z]})).unwrap();
            let form = s.forms_prototype.as_ref().unwrap();
            for (&id, &point) in form.logical.faces[0].iter().zip(&anchors) {
                assert!(preview.vertices[id].dist(point + b) < 1e-7);
                assert!(form.surface.vertices[id].dist(preview.vertices[id]) < 1e-7);
            }
        }
    }
    #[test]
    fn local_refinement_insertion_detail_and_drag_extrusion_are_atomic() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().cage.clone();
        s.execute("form.prototype.insert_edge", &json!({"face":0})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        assert_eq!(f.logical.faces.iter().filter(|f| !f.is_empty()).count(), 7);
        assert_eq!(f.cage, before.refined(1).unwrap());
        let id = f.hierarchy.as_ref().unwrap().seam_endpoints(0, 0).unwrap()[0];
        let p = f.surface.vertices[id] + solidcore_record::Vec3::Z;
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[id],"positions":[[p.x,p.y,p.z]]})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().surface.vertices[id].dist(p) < 1e-8);
        let definition = s.forms_prototype.as_ref().unwrap().definition();
        let encoded = serde_json::to_string(&definition).unwrap();
        let restored: crate::doc::forms::Definition = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored, definition);
        s.execute("form.prototype.refine", &json!({"faces":[6],"counts":[2,2]})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().hierarchy.as_ref().unwrap().leaves[7].active);
        for _ in 0..3 {
            s.execute("form.prototype.undo", &json!({})).unwrap();
        }
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage, before);
        let f = s.forms_prototype.as_ref().unwrap().cage.faces[0].clone();
        let a = [f[0], f[1]];
        let b = [f[2], f[3]];
        s.execute("form.prototype.insert_points", &json!({"points":[{"edge":a,"fraction":0.3},{"edge":b,"fraction":0.65}]})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        for (checkpoint, z) in [(true, 1.0), (false, 3.0)] {
            s.execute("form.prototype.extrude_drag", &json!({"faces":[0],"displacement":[0,0,z],"checkpoint":checkpoint})).unwrap();
        }
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage.faces.len(), 10);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage, before);
    }
    #[test]
    fn hierarchical_face_drag_keeps_one_undo_step_and_reopens_from_design() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[10,10,1],"segments":[2,2,1],"levels":1})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().definition();
        let old = before.hierarchy.as_ref().unwrap();
        let cap = old.base.faces.len();
        let mut numeric = s.scratch();
        let reply = numeric.execute("form.prototype.topology", &json!({"tool":"extrude_faces","faces":[cap],"displacement":[0,0,3]})).unwrap();
        assert_eq!(reply["undo"], 2);
        assert!(numeric.execute("form.prototype.extrude_drag", &json!({"faces":[cap],"displacement":[0,0,4],"checkpoint":false})).is_err());
        for (checkpoint, z) in [(true, 1.0), (false, 3.0)] {
            s.execute("form.prototype.extrude_drag", &json!({"faces":[cap],"displacement":[0,0,z],"checkpoint":checkpoint})).unwrap();
        }
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.inspect()["undo"], 2);
        let after = form.definition();
        assert_eq!(numeric.forms_prototype.as_ref().unwrap().definition(), after);
        let hierarchy = after.hierarchy.as_ref().unwrap();
        assert_eq!(hierarchy.base, old.base);
        assert_eq!(after.cage.faces.len(), before.cage.faces.len() + 4);
        assert_eq!(hierarchy.leaves.len(), old.leaves.len() + 4);
        assert_eq!(form.logical.faces, hierarchy.layout().unwrap().faces);
        assert_eq!(after.cage, old.extrude_faces(&[cap], solidcore_record::Vec3::Z * 3.0).unwrap().evaluate().unwrap());
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        let finish = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        let id = finish["feature"].as_u64().unwrap();
        let saved = solidcraft_io::write_design(s.doc.as_ref());
        let reopened = solidcraft_io::read_design(&saved).unwrap();
        assert_eq!(reopened.feature(id).unwrap(), s.doc.feature(id).unwrap());
        let mut s = Session::new(reopened);
        s.execute("form.edit", &json!({"feature":id})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.definition(), after);
        assert_eq!(form.logical.faces, hierarchy.layout().unwrap().faces);
        assert!(s.model.result(id).unwrap().error.is_none());
    }

    #[test]
    fn hierarchical_boundary_drag_preserves_old_controls_and_reopens_from_design() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[10,10,1],"segments":[2,2,1],"levels":1})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let before = s.forms_prototype.as_ref().unwrap().definition();
        let old = before.hierarchy.as_ref().unwrap();
        let ring = before.cage.boundary_loops().unwrap().remove(0);
        let rim: std::collections::BTreeSet<_> =
            ring.iter().copied().zip(ring.iter().copied().cycle().skip(1)).take(ring.len()).map(|(a, b)| (a.min(b), a.max(b))).collect();
        let (edge, path) = old
            .layout()
            .unwrap()
            .paths
            .into_iter()
            .find(|(_, path)| path.len() > 2 && path.windows(2).all(|p| rim.contains(&(p[0].min(p[1]), p[0].max(p[1])))))
            .unwrap();
        let mut numeric = s.scratch();
        let reply = numeric.execute("form.prototype.topology", &json!({"tool":"extrude_edges","edges":[edge],"displacement":[0,0,3]})).unwrap();
        assert_eq!(reply["undo"], 2);
        assert!(numeric.execute("form.prototype.extrude_drag", &json!({"edges":[edge],"displacement":[0,0,4],"checkpoint":false})).is_err());
        for (checkpoint, z) in [(true, 1.0), (false, 3.0)] {
            s.execute("form.prototype.extrude_drag", &json!({"edges":[edge],"displacement":[0,0,z],"checkpoint":checkpoint})).unwrap();
        }
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.inspect()["undo"], 2);
        assert_eq!(form.extruded_edges().len(), path.len() - 1);
        let after = form.definition();
        assert_eq!(numeric.forms_prototype.as_ref().unwrap().definition(), after);
        let hierarchy = after.hierarchy.as_ref().unwrap();
        assert_eq!(hierarchy.base, old.base);
        assert_eq!(&after.cage.faces[..before.cage.faces.len()], before.cage.faces);
        assert_eq!(&after.cage.points[..before.cage.points.len()], before.cage.points);
        assert_eq!(hierarchy.leaves.len(), old.leaves.len() + path.len() - 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        let id = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        let saved = solidcraft_io::write_design(s.doc.as_ref());
        let reopened = solidcraft_io::read_design(&saved).unwrap();
        assert_eq!(reopened.feature(id).unwrap(), s.doc.feature(id).unwrap());
        let mut s = Session::new(reopened);
        s.execute("form.edit", &json!({"feature":id})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.definition(), after);
        assert_eq!(form.logical.faces, hierarchy.layout().unwrap().faces);
        assert!(s.model.result(id).unwrap().error.is_none());
    }

    #[test]
    fn symmetric_hierarchical_face_drag_reflects_the_picked_side_and_reopens() {
        use solidcore_record::Vec3;

        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[16,12,1],"segments":[4,3,1],"levels":1})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let root = form.logical.faces.iter().position(|f| f.iter().all(|&v| form.cage.points[v].x >= 4.0)).unwrap();
        let mut numeric = s.scratch();
        let base_faces = form.cage.faces.len();
        numeric.execute("form.prototype.topology", &json!({"tool":"extrude_faces","faces":[root],"displacement":[1.5,0,3]})).unwrap();
        let base = numeric.forms_prototype.as_ref().unwrap();
        assert_eq!(base.hierarchy.as_ref().unwrap().depth, 0);
        assert_eq!(base.cage.faces.len(), base_faces + 8);
        base.symmetry.as_ref().unwrap().validate_reflection(&base.cage).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[root],"counts":[2,2]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let root_count = before.hierarchy.as_ref().unwrap().base.faces.len();
        let seed = form
            .logical
            .faces
            .iter()
            .enumerate()
            .skip(root_count)
            .find_map(|(id, f)| f.iter().all(|&v| form.cage.points[v].x > 3.5).then_some(id))
            .unwrap();
        let depth = form.inspect()["undo"].as_u64().unwrap();
        let seam = form
            .logical
            .faces
            .iter()
            .position(|f| {
                !f.is_empty()
                    && f.iter().any(|&v| form.cage.points[v].x == 0.0)
                    && f.iter().any(|&v| form.cage.points[v].x > 0.0)
                    && f.iter().all(|&v| form.cage.points[v].x >= 0.0)
            })
            .unwrap();
        assert!(s.execute("form.prototype.extrude_drag", &json!({"faces":[seam],"displacement":[1,0,0]})).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        for (checkpoint, x, z) in [(true, 0.5, 1.0), (false, 1.5, 3.0)] {
            s.execute("form.prototype.extrude_drag", &json!({"faces":[seed],"displacement":[x,0,z],"checkpoint":checkpoint})).unwrap();
        }
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.inspect()["undo"], depth + 1);
        let after = form.definition();
        assert_eq!(after.cage.faces.len(), before.cage.faces.len() + 8);
        after.symmetry.as_ref().unwrap().validate_reflection(&after.cage).unwrap();
        assert_eq!(
            after.cage,
            before
                .hierarchy
                .as_ref()
                .unwrap()
                .extrude_symmetric_faces(&[seed], Vec3::new(1.5, 0.0, 3.0), before.symmetry.as_ref().unwrap())
                .unwrap()
                .evaluate()
                .unwrap()
        );
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        let id = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        let design = solidcraft_io::read_design(&solidcraft_io::write_design(s.doc.as_ref())).unwrap();
        let mut s = Session::new(design);
        s.execute("form.edit", &json!({"feature":id})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.definition(), after);
        assert!(s.model.result(id).unwrap().error.is_none());
    }

    #[test]
    fn mirrored_boundary_drag_preserves_reflection_and_reopens_from_design() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[16,12,1],"segments":[4,3,1],"levels":1})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let root = form.logical.faces.iter().position(|f| f.iter().all(|&v| form.cage.points[v].x >= 4.0)).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[root],"counts":[2,2]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let ring = before.cage.boundary_loops().unwrap().remove(0);
        let rim: std::collections::BTreeSet<_> = ring.iter().zip(ring.iter().cycle().skip(1)).map(|(&a, &b)| (a.min(b), a.max(b))).collect();
        let (edge, path) = before
            .hierarchy
            .as_ref()
            .unwrap()
            .layout()
            .unwrap()
            .paths
            .into_iter()
            .find(|(_, path)| {
                path.iter().all(|&v| before.cage.points[v].x >= 3.5) && path.windows(2).all(|p| rim.contains(&(p[0].min(p[1]), p[0].max(p[1]))))
            })
            .unwrap();
        let mut numeric = s.scratch();
        numeric.execute("form.prototype.topology", &json!({"tool":"extrude_edges","edges":[edge],"displacement":[1.5,0,3]})).unwrap();
        for (checkpoint, x, z) in [(true, 0.5, 1.0), (false, 1.5, 3.0)] {
            s.execute("form.prototype.extrude_drag", &json!({"edges":[edge],"displacement":[x,0,z],"checkpoint":checkpoint})).unwrap();
        }
        let form = s.forms_prototype.as_ref().unwrap();
        let after = form.definition();
        assert_eq!(numeric.forms_prototype.as_ref().unwrap().definition(), after);
        assert_eq!(form.inspect()["undo"], undo + 1);
        assert_eq!(form.extruded_edges().len(), 2 * (path.len() - 1));
        after.symmetry.as_ref().unwrap().validate_reflection(&after.cage).unwrap();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        let id = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        let design = solidcraft_io::read_design(&solidcraft_io::write_design(s.doc.as_ref())).unwrap();
        let mut s = Session::new(design);
        s.execute("form.edit", &json!({"feature":id})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), after);
        assert!(s.model.result(id).unwrap().error.is_none());
    }

    #[test]
    fn trimmed_edge_drag_preserves_stencil_edits_rim_and_finishes_after_reopen() {
        use solidcore_record::Vec3;

        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":1})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[5],"counts":[2,2]})).unwrap();
        let deleted = 16;
        let old_face = s.forms_prototype.as_ref().unwrap().logical.faces[deleted].clone();
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[deleted]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let h = before.hierarchy.as_ref().unwrap();
        let edge = old_face
            .iter()
            .copied()
            .zip(old_face.iter().copied().cycle().skip(1))
            .take(old_face.len())
            .find_map(|(a, b)| h.trim_attachments(&[[a, b]]).ok()?.into_iter().find(|t| t.root == 5).map(|t| t.edge))
            .unwrap();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let mut surface = s.scratch();
        surface.execute("form.prototype.extrude_drag", &json!({"surface":true,"edges":[edge],"displacement":[0,0,2]})).unwrap();
        let surface_form = surface.forms_prototype.as_ref().unwrap();
        let wall = surface_form.logical.faces.last().unwrap();
        for (old, new) in [(wall[0], wall[3]), (wall[1], wall[2])] {
            assert!(surface_form.surface.vertices[new].dist(form.surface.vertices[old] + Vec3::Z * 2.0) < 1e-7);
        }
        for (checkpoint, z) in [(true, 1.0), (false, 2.0)] {
            s.execute("form.prototype.extrude_drag", &json!({"edges":[edge],"displacement":[0,0,z],"checkpoint":checkpoint})).unwrap();
        }
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.inspect()["undo"], undo + 1);
        assert_eq!(form.cage, before.cage);
        let extruded = form.definition();
        let [front] = form.extruded_edges() else { panic!("missing translated trim rim") };
        let front = *front;
        let mut unsupported = s.scratch();
        let recovery = crate::recovery::capture_bytes(&unsupported).unwrap();
        let error = unsupported.execute("form.prototype.topology", &json!({"tool":"crease","edges":[front],"distance":10})).unwrap_err();
        assert!(error.to_string().contains("remove the derived trim wall"), "{error}");
        assert_eq!(crate::recovery::capture_bytes(&unsupported).unwrap(), recovery);
        let targets = front.map(|v| {
            let p = form.surface.vertices[v] + Vec3::Z;
            [p.x, p.y, p.z]
        });
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":front,"positions":targets})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert_eq!(form.cage, before.cage);
        let edited = form.definition();
        assert_eq!(front.map(|v| form.surface.vertices[v].z), [3.0; 2]);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), extruded);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), before);
        for _ in 0..2 {
            s.execute("form.prototype.redo", &json!({})).unwrap();
        }
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), edited);

        let mut placed = s.scratch();
        placed.execute("form.prototype.transform", &json!({"matrix":crate::doc::rigid(Vec3::Z * 99_996.0, Vec3::ZERO, Vec3::Z, 0.0)})).unwrap();
        let form = placed.forms_prototype.as_ref().unwrap();
        let points = form.control_positions();
        let ids: std::collections::BTreeSet<_> = form.logical.faces.iter().flatten().copied().collect();
        let targets: Vec<_> = ids
            .iter()
            .map(|&v| {
                let p = points[v];
                [p.x, p.y, 99_996.0 + 2.0 * (p.z - 99_996.0)]
            })
            .collect();
        let snapshot = form.inspect();
        assert!(placed.execute("form.prototype.set_vertices", &json!({"vertices":ids,"positions":targets})).is_err());
        assert_eq!(placed.forms_prototype.as_ref().unwrap().inspect(), snapshot);

        let (skin, _) = crate::doc::forms::convert(&before, 0.05).unwrap();
        let (wall, report) = crate::doc::forms::convert(&edited, 0.05).unwrap();
        assert!(report.trimmed_roots && !report.solid);
        assert_eq!(wall.face_count(), skin.face_count() + 1);
        assert_eq!(wall.edge_count(), skin.edge_count() + 3);
        assert_eq!(wall.shell_count(), skin.shell_count());
        let id = s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        let doc = solidcraft_io::read_design(&solidcraft_io::write_design(s.doc.as_ref())).unwrap();
        let mut s = Session::new(doc);
        s.execute("form.edit", &json!({"feature":id})).unwrap();
        let reopened = s.forms_prototype.as_ref().unwrap();
        assert_eq!(reopened.definition(), edited);
        assert!(front.iter().all(|&v| reopened.selectable_control(v)));
        assert!(s.model.result(id).unwrap().error.is_none());
    }

    #[test]
    fn projection_targets_the_surface_and_undoes_atomically() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let ids = form.cage.faces[0].clone();
        let center_z = ids.iter().map(|&i| form.surface.vertices[i].z).sum::<f64>() / ids.len() as f64;
        let before = form.inspect();
        s.execute("form.prototype.project", &json!({"tool":"flatten","vertices":ids,"direction":[0,0,1]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        assert!(ids.iter().all(|&i| (form.surface.vertices[i].z - center_z).abs() < 1e-8));
        assert_eq!(form.inspect()["undo"], 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["points"], before["points"]);
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        for bad in [
            json!({"tool":"straighten","vertices":[0,0]}),
            json!({"tool":"flatten","vertices":[0,1,999999]}),
            json!({"tool":"flatten","vertices":ids,"direction":[0,0,0]}),
        ] {
            assert!(s.execute("form.prototype.project", &bad).is_err());
            assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        }
        s.execute("form.prototype.project", &json!({"tool":"straighten","points":"control","vertices":[0,1],"direction":[1,0,0],"origin":[0,2,3]}))
            .unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        for i in [0, 1] {
            assert_eq!(form.cage.points[i].y, 2.0);
            assert_eq!(form.cage.points[i].z, 3.0);
        }
    }
    #[test]
    fn changing_a_primitive_preserves_the_previous_sculpt_in_undo() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({})).unwrap();
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-20,-20,30]]})).unwrap();
        let sculpt = s.forms_prototype.as_ref().unwrap().cage.clone();
        s.execute("form.prototype.create", &json!({"primitive":"torus"})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().parameters.kind, "torus");
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage, sculpt);
        s.execute("form.prototype.close", &json!({})).unwrap();
        s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        assert!(s.execute("form.prototype.create", &json!({})).is_err());
        assert!(s.forms_prototype.is_none());
        assert!(s.active_sketch.is_some());
    }
    #[test]
    fn an_unfinished_form_is_unsaved_and_cannot_be_silently_left_out_of_a_save() {
        let mut s = Session::default();
        assert!(!s.is_dirty());
        s.execute("form.prototype.create", &json!({})).unwrap();
        assert!(s.is_dirty());
        let error = s.execute("file.save_as", &json!({"path":"target/forms-save-must-not-be-written.solidcraft"})).unwrap_err();
        assert!(error.to_string().contains("finish or cancel"));
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let error = s.design_container(solidcraft_io::DesignCompression::Fast, false, false).unwrap_err();
        assert!(error.to_string().contains("finish or cancel"));
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        s.execute("form.prototype.close", &json!({})).unwrap();
        assert!(!s.is_dirty());
        assert!(s.design_container(solidcraft_io::DesignCompression::Fast, false, false).is_ok());
    }
    #[test]
    fn mirror_face_pull_handles_the_complete_cubes_limit_null_mode() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
        s.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let ids = f.cage.faces.iter().find(|face| face.iter().all(|&i| f.cage.points[i].x > 0.0)).unwrap().clone();
        let targets: Vec<_> = ids
            .iter()
            .map(|&i| {
                let p = f.surface.vertices[i];
                [p.x + 1.0, p.y, p.z]
            })
            .collect();
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":ids,"positions":targets})).unwrap();
        let f = s.forms_prototype.as_ref().unwrap();
        let symmetry = f.symmetry.as_ref().unwrap();
        for (&i, target) in ids.iter().zip(targets) {
            assert!(f.surface.vertices[i].dist(solidcore_record::Vec3::new(target[0], target[1], target[2])) < 1e-8);
        }
        for (i, &j) in symmetry.partners.iter().enumerate() {
            assert!(f.cage.points[i].dist(symmetry.reflect(f.cage.points[j])) < 1e-8);
        }
    }
    #[test]
    fn sculpted_face_fits_to_cad_at_default_tolerance_and_failed_fits_are_atomic() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let ids = form.cage.faces[0].clone();
        let points: Vec<_> = ids.iter().map(|&i| form.surface.vertices[i]).collect();
        let center = points.iter().copied().fold(solidcore_record::Vec3::ZERO, |a, b| a + b) / points.len() as f64;
        let positions: Vec<_> = points
            .iter()
            .map(|&p| {
                let q = p - center;
                let (sn, cs) = 30_f64.to_radians().sin_cos();
                let p = center + solidcore_record::Vec3::new(q.x * cs - q.y * sn, q.x * sn + q.y * cs, q.z);
                [p.x, p.y, p.z]
            })
            .collect();
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":ids,"positions":positions})).unwrap();
        let source = s.forms_prototype.as_ref().unwrap().inspect();
        let document = s.doc.clone();
        assert!(s.execute("form.prototype.commit", &json!({"tolerance":1e-7})).is_err());
        assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), source);
        let result = s.execute("form.prototype.commit", &json!({})).unwrap();
        let feature = result["feature"].as_u64().unwrap();
        let state = s.model.state();
        let body = &state.bodies[0].body;
        assert!(!body.is_mesh());
        assert!(!body.is_surface());
        let crate::doc::FeatureKind::Form { definition, .. } = &s.doc.features.iter().find(|f| f.id == feature).unwrap().kind else {
            panic!("lost Form source")
        };
        assert_eq!(definition.cage.faces.len(), 24);
        assert!(body.validity().is_empty());
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        let saved = serde_json::to_vec(&*s.doc).unwrap();
        s.execute("form.prototype.commit", &json!({})).unwrap();
        assert_eq!(serde_json::to_vec(&*s.doc).unwrap(), saved);
    }
    #[test]
    fn surface_targets_topology_and_saved_cad_are_editable() {
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
        let old = s.forms_prototype.as_ref().unwrap().surface.vertices[0];
        let target = old + solidcore_record::Vec3::new(1.0, 2.0, 0.0);
        s.execute("form.prototype.set_surface_vertices", &json!({"vertices":[0],"positions":[[target.x,target.y,target.z]]})).unwrap();
        assert!(s.forms_prototype.as_ref().unwrap().surface.vertices[0].dist(target) < 1e-8);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        let definition = s.forms_prototype.as_ref().unwrap().cage.clone();
        let reply = s.execute("form.prototype.commit", &json!({"name":"Rounded box","tolerance":0.05})).unwrap();
        let feature = reply["feature"].as_u64().unwrap();
        assert!(s.forms_prototype.is_none());
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!(!s.model.state().bodies[0].body.is_mesh());
        let data = serde_json::to_vec(&*s.doc).unwrap();
        let restored: crate::doc::Document = serde_json::from_slice(&data).unwrap();
        assert!(matches!(&restored.feature(feature).unwrap().kind,crate::doc::FeatureKind::Form{definition:d,..} if d.cage==definition));
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        s.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[0]})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage.faces.len(), 5);
        s.execute("form.prototype.close", &json!({})).unwrap();
        assert_eq!(serde_json::to_vec(&*s.doc).unwrap(), data);
        s.execute("form.edit", &json!({"feature":feature})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().cage, definition);
    }
    #[test]
    fn prepared_form_transforms_match_the_commands_and_preserve_live_document_state() {
        use crate::forms_prototype::FormTargetSpace;
        use solidcore_record::Vec3;
        use std::sync::Arc;

        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.redo.is_empty(), "the CAD redo is real document history");
        s.execute("form.prototype.create", &json!({"levels":1})).unwrap();
        s.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let form = s.forms_prototype.as_ref().unwrap();
        let vertex = *form.hierarchy.as_ref().unwrap().controls.iter().find(|(_, level)| **level == 1).unwrap().0;
        let start = form.surface.vertices[vertex];
        let original = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let snapshot = s.checkpoint();
        let doc = s.doc.clone();
        let model = s.model.state();
        let redo = s.redo.last().unwrap().doc.clone();
        let revision = s.revision;
        s.pending_moves.insert(999, crate::doc::rigid(Vec3::X * 3.0, Vec3::ZERO, Vec3::Z, 0.0));
        let moves = s.pending_moves.clone();
        let frame = |delta: Vec3, checkpoint| {
            let p = start + delta;
            json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]],"checkpoint":checkpoint})
        };

        let preview = s.forms_prototype.as_ref().unwrap().surface.clone();
        let shares = Arc::strong_count(&preview);
        let noop = s.prepare_form_transform(FormTargetSpace::Surface, frame(Vec3::ZERO, true)).unwrap();
        assert_eq!(Arc::strong_count(&preview), shares + 1, "capture pins the existing preview instead of cloning its mesh");
        let (changed, _) = s.accept_form_transform(noop.compute().unwrap()).unwrap();
        assert!(!changed);
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);

        let mut sync = s.scratch();
        for (delta, first) in [(Vec3::Z * 0.4, true), (Vec3::new(0.2, 0.0, 0.7), false)] {
            let p = frame(delta, first);
            let prepared = s.prepare_form_transform(FormTargetSpace::Surface, p.clone()).unwrap().compute().unwrap();
            let expected = sync.execute("form.prototype.set_surface_vertices", &p).unwrap();
            // A view selection can change while the worker runs. Keep the new
            // live selection, not a worker copy of it.
            let selection = vec![crate::Sel::Feature { id: 999 }];
            s.selection = selection.clone();
            let (changed, reply) = s.accept_form_transform(prepared).unwrap();
            assert!(changed);
            assert_eq!(reply, expected);
            assert_eq!(s.selection, selection);
            assert!(Arc::ptr_eq(&s.doc, &doc));
            assert!(Arc::ptr_eq(&s.model.state(), &model));
            assert!(Arc::ptr_eq(&s.redo.last().unwrap().doc, &redo));
            assert!(s.undo.is_empty());
            assert_eq!(s.pending_moves, moves);
            assert_eq!(s.revision, revision);
        }
        let form = s.forms_prototype.as_ref().unwrap();
        assert!(form.surface.vertices[vertex].dist(start + Vec3::new(0.2, 0.0, 0.7)) < 1e-8);
        let authored = form.definition();
        assert_eq!(form.inspect()["undo"], undo + 1);
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
        s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), authored);
        s.restore_checkpoint(snapshot).unwrap();
        assert_eq!(s.forms_prototype.as_ref().unwrap().definition(), original);
    }

    #[test]
    fn prepared_form_transforms_refuse_stale_sources_and_preserve_command_checks() {
        use crate::forms_prototype::FormTargetSpace;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let mut s = Session::default();
        s.execute("form.prototype.create", &json!({"levels":1})).unwrap();
        let params = json!({"vertices":[0],"positions":[[-20,-20,4]]});
        let before = s.forms_prototype.as_ref().unwrap().inspect();
        let checks = Arc::new(AtomicUsize::new(0));
        let host_checks = checks.clone();
        s.set_command_check(Some(Arc::new(move |id, p, live| {
            host_checks.fetch_add(1, Ordering::Relaxed);
            if id == "form.prototype.set_vertices" && p["vertices"] == json!([0]) && live.forms_prototype.is_some() {
                Ok(())
            } else {
                Err("unexpected command or source".into())
            }
        })));
        let job = s.prepare_form_transform(FormTargetSpace::Control, params.clone()).unwrap();
        assert_eq!(checks.load(Ordering::Relaxed), 1);
        let ready = job.compute().unwrap();
        assert_eq!(checks.load(Ordering::Relaxed), 1, "the worker does not repeat the host check on a copy");
        s.pending_moves.insert(99, crate::doc::IDENTITY);
        assert!(s.accept_form_transform(ready).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
        s.pending_moves.clear();

        let ready = s.prepare_form_transform(FormTargetSpace::Control, params.clone()).unwrap().compute().unwrap();
        s.active_occurrence_path = Some(vec![12]);
        assert!(s.accept_form_transform(ready).is_err(), "a moved authoring context invalidates the job");
        s.active_occurrence_path = None;

        s.param_check = crate::schema::ParamCheck::Strict;
        let malformed = json!({"vertices":[0],"positions":[[-20,-20,4]],"positons":[]});
        let mut sync = s.scratch();
        sync.set_command_check(None);
        let expected = sync.execute("form.prototype.set_vertices", &malformed).unwrap_err().to_string();
        let job = s.prepare_form_transform(FormTargetSpace::Control, malformed).unwrap();
        assert!(matches!(job.compute(), Err(e) if e.to_string() == expected));

        let ready = s.prepare_form_transform(FormTargetSpace::Control, params.clone()).unwrap().compute().unwrap();
        s.set_command_check(None);
        s.execute("form.prototype.close", &json!({})).unwrap();
        s.execute("form.prototype.create", &json!({"levels":1})).unwrap();
        s.set_command_check(Some(Arc::new(|_, _, _| Ok(()))));
        assert!(s.accept_form_transform(ready).is_err());
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before, "identical geometry from a new draft cannot accept old work");

        s.set_command_check(Some(Arc::new(|_, _, _| Err("denied by host".into()))));
        assert!(matches!(s.prepare_form_transform(FormTargetSpace::Control, params), Err(EngineError::Refused { .. })));
        assert_eq!(s.forms_prototype.as_ref().unwrap().inspect(), before);
    }

    #[test]
    fn drag_is_atomic_one_undo_and_never_changes_the_cad_document() {
        let mut s = Session::default();
        let doc = s.doc.clone();
        let rev = s.revision;
        s.execute("form.prototype.create", &json!({})).unwrap();
        let before = s.execute("form.prototype.inspect", &json!({})).unwrap();
        let p = before["points"][0].as_array().unwrap();
        let x = p[0].as_f64().unwrap();
        let y = p[1].as_f64().unwrap();
        let z = p[2].as_f64().unwrap();
        s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[x+5.,y,z]],"checkpoint":true})).unwrap();
        let after = s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[x+10.,y,z]],"checkpoint":false})).unwrap();
        assert_eq!(after["undo"], 1);
        assert!(s.execute("form.prototype.set_vertices", &json!({"vertices":[99999],"positions":[[1,2,3]]})).is_err());
        assert_eq!(s.execute("form.prototype.inspect", &json!({})).unwrap(), after);
        let undone = s.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(undone["points"], before["points"]);
        let redone = s.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(redone["points"], after["points"]);
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(s.revision, rev);
        assert!(s.undo.is_empty());
    }
}
