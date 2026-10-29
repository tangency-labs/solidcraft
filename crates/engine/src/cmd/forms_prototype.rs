//! Editable Forms sessions, surface edits and persistent CAD feature creation.
use super::CommandSpec;
use crate::schema::{TypedContract, forms_prototype as params, output_schema_for};
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};

mod blend;
mod pipe;

const CREATE: TypedContract<params::Create> = TypedContract::new();
const PARAMETERS: TypedContract<params::Parameters> = TypedContract::new();
const PROJECT: TypedContract<params::Project> = TypedContract::new();
const SET: TypedContract<params::SetVertices> = TypedContract::new();
const QUALITY: TypedContract<params::Quality> = TypedContract::new();
const EMPTY: TypedContract<params::Empty> = TypedContract::new();
const COMMIT: TypedContract<params::Commit> = TypedContract::new();
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
        .params("body, face: existing CAD source; segments?: [U,V] (2,2); continuity?:0|1. Edit interior guide points while retaining the source rim; Finish replaces that face in its solid."),
    CommandSpec::new("form.prototype.match","Match Form Boundary",crate::checked!("form.prototype.match",matching)).icon("edit")
        .noundo().doc_only().contract(&MATCH).output(output_schema_for::<params::Reply>)
        .params("body and exactly one of face or closed edges; edge: a free Form boundary edge; order?:0,1,2. Match exact CAD support with persistent references. Tolerances: position_mm?.001, tangent_deg?.1, curvature_relative?.01; tolerance?: fitted CAD mm (.01)."),
    CommandSpec::new("form.prototype.unweld","Unweld Form",crate::checked!("form.prototype.unweld",unweld)).icon("edit")
        .noundo().doc_only().contract(&UNWELD).output(output_schema_for::<params::Reply>)
        .params("edges: separating interior seam; separate?: true; name?. Stage independent open Forms, or keep their disjoint cages together. Finish creates the staged objects; Cancel restores the document unchanged."),
    CommandSpec::new("form.prototype.weld","Weld Forms",crate::checked!("form.prototype.weld",weld)).icon("edit")
        .noundo().doc_only().contract(&WELD).output(output_schema_for::<params::Reply>)
        .params("other: {kind:draft,part} or {kind:feature,feature}; first_edge,second_edge. Identify equal free openings, opposite winding, midpoint controls, maximum sharpness. The second object is consumed only on Finish; Cancel and undo restore it."),
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
        .params("edge: [a,b]; fraction?: 0..1 (0.5). Insert a complete quad edge ring in a base cage, preserving source sharpness."),
    CommandSpec::new("form.prototype.refine", "Refine Form Faces", crate::checked!("form.prototype.refine", refine)).icon("edit")
        .noundo().doc_only().contract(&REFINE).output(output_schema_for::<params::Reply>)
        .params("faces: active logical IDs; counts?: [U,V], powers of two 1..16. Shape-preserving local leaves over nested uniform evaluation and sparse hierarchical detail."),
    CommandSpec::new("form.prototype.insert_edge", "Insert Form Edge", crate::checked!("form.prototype.insert_edge", insert_edge)).icon("edit")
        .noundo().doc_only().contract(&INSERT_EDGE).output(output_schema_for::<params::Reply>)
        .params("face: active logical ID; axis?: 0 (split U) or 1 (split V). Insert a local dyadic seam without changing the limit surface."),
    CommandSpec::new("form.prototype.insert_points", "Insert Form Points", crate::checked!("form.prototype.insert_points", insert_points)).icon("edit")
        .noundo().doc_only().contract(&INSERT_POINTS).output(output_schema_for::<params::Reply>)
        .params("points: [{edge:[a,b],fraction:0..1}], ordered chain on opposite logical face sides. Fractions start at the lower vertex ID. Exposes dyadic seams and moves their surface endpoints to the requested positions."),
    CommandSpec::new("form.prototype.extrude_drag", "Extrude Form by Dragging", crate::checked!("form.prototype.extrude_drag", extrude_drag)).icon("edit")
        .noundo().doc_only().contract(&EXTRUDE_DRAG).output(output_schema_for::<params::Reply>)
        .params("faces? or boundary edges?; displacement:[x,y,z] mm; checkpoint?:true. Later gesture frames use false and displacement from the original source. One undo step."),
    CommandSpec::new("form.prototype.project", "Straighten or Flatten Form", crate::checked!("form.prototype.project", project)).icon("edit")
        .noundo().doc_only().contract(&PROJECT).output(output_schema_for::<params::Reply>)
        .params("tool: straighten|flatten; vertices: distinct indices; points?: surface|control; direction?: line direction or plane normal; origin?: mm. Omitted direction uses least-squares fitting; omitted origin uses the centroid. One undoable edit, no reference association."),
    CommandSpec::new("form.prototype.symmetry", "Form Symmetry", crate::checked!("form.prototype.symmetry", symmetry)).icon("mirror")
        .noundo().doc_only().contract(&SYMMETRY).output(output_schema_for::<params::Reply>)
        .params("axis?: 0 (YZ), 1 (XZ), 2 (XY), or null to disable. Stable mirrored point partners are saved with the Form."),
    CommandSpec::new("form.prototype.parameters", "Form Parameters", crate::checked!("form.prototype.parameters", parameters)).icon("params")
        .noundo().doc_only().contract(&PARAMETERS).output(output_schema_for::<params::Reply>)
        .params("dimensions?: [x,y,z] mm; segments?: [x,y,z]; radii?: [major,minor] or [outer,inner]; path?: [[x,y,z]] mm. Dimension changes preserve sculpted controls. Topology changes currently require an unedited primitive."),
    CommandSpec::new("form.prototype.topology", "Edit Form Topology", crate::checked!("form.prototype.topology", topology)).icon("edit")
        .noundo().doc_only().contract(&TOPOLOGY).output(output_schema_for::<params::Reply>)
        .params("tool: extrude_faces|extrude_edges|delete_faces|dissolve|fill_hole|bridge|weld|unweld|thicken|subdivide; faces?, edges?, vertices?, displacement?, distance?. Atomic validated topology edit."),
    CommandSpec::new("form.prototype.commit","Finish Form",crate::checked!("form.prototype.commit",commit)).icon("finish").contract(&COMMIT).output(output_schema_for::<Value>)
        .params("tolerance?: mm (0.01); operation?: new|join|cut|intersect; targets?: body names; name?. Fit and sew CAD patches and save the editable Form feature."),
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
    CommandSpec::new("form.prototype.inspect", "Inspect Form Prototype", crate::checked!("form.prototype.inspect", inspect)).icon("inspect")
        .noundo()
        .doc_only()
        .contract(&EMPTY)
        .output(output_schema_for::<params::Reply>),
    CommandSpec::new("form.prototype.close", "Close Form Prototype", crate::checked!("form.prototype.close", close)).icon("close")
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
    if f.tool_original.is_some() {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    Ok(f)
}
fn raw_form(s: &mut Session) -> Result<&mut crate::forms_prototype::Form> {
    s.forms_prototype.as_mut().ok_or_else(|| EngineError::Other("create a Form first".into()))
}
fn tool_begin(s: &mut Session, _: &Value) -> Result<Value> {
    let f = form(s)?;
    f.tool_original = Some(std::sync::Arc::new(f.clone()));
    Ok(f.inspect())
}
fn tool_preview(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::ToolPreview = parse(p)?;
    let f = raw_form(s)?;
    let source = f.tool_original.clone().ok_or_else(|| EngineError::Other("start a Form tool preview first".into()))?;
    let revision = f.revision;
    let mut scratch = s.scratch();
    scratch.forms_prototype = Some(source.as_ref().clone());
    type Preview = (&'static str, bool, fn(&mut Session, &Value) -> Result<Value>);
    let operations: [Preview; 7] = [
        ("pipe", args.pipe.is_some(), pipe::create),
        ("surface", args.surface.is_some(), surface),
        ("matching", args.matching.is_some(), matching),
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
    let mut next = scratch.forms_prototype.take().ok_or_else(|| EngineError::Other("tool preview did not produce a Form".into()))?;
    next.tool_original = Some(source);
    next.revision = revision.wrapping_add(1);
    let reply = next.inspect();
    s.forms_prototype = Some(next);
    Ok(reply)
}
fn tool_apply(s: &mut Session, _: &Value) -> Result<Value> {
    let f = raw_form(s)?;
    if f.tool_original.take().is_none() {
        return Err(EngineError::Other("no Form tool preview is open".into()));
    }
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
    if s.forms_prototype.as_ref().is_some_and(|f| f.tool_original.is_some()) {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
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
        path: p.path.unwrap_or_default().into_iter().map(|p| crate::geom::Vec3::new(p[0], p[1], p[2])).collect(),
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
fn match_state(s: &Session) -> crate::doc::ModelState {
    let before = s.forms_prototype.as_ref().and_then(|f| f.editing_feature).map_or_else(|| s.model.state(), |id| s.model.state_before(id));
    crate::doc::world_state(&s.doc, &before)
}
fn surface(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Surface = parse(p)?;
    if s.active_sketch.is_some() {
        return Err(EngineError::Other("finish the sketch before creating a Form Surface".into()));
    }
    if s.forms_prototype.as_ref().is_some_and(|f| f.tool_original.is_some()) {
        return Err(EngineError::Other("apply or cancel the active Form tool first".into()));
    }
    let state = match_state(s);
    let body = state.body(&args.body).ok_or_else(|| EngineError::Other("the Form Surface source body is missing".into()))?;
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
    let args: params::Match = parse(p)?;
    let definition = form(s)?.definition();
    let state = match_state(s);
    let body = state.body(&args.body).ok_or_else(|| EngineError::Other("Match support body is missing or follows the edited Form".into()))?;
    let source = match (args.face, args.edges) {
        (Some(face), None) if !args.wire_from_face => crate::doc::forms::SupportRef::face(body, face)?,
        (Some(face), None) => {
            crate::kernel::face_geometry(&body.body, face)?;
            let mesh = body.mesh();
            let edges = mesh.edge_faces.iter().enumerate().filter_map(|(i, faces)| faces.contains(&(face as u32)).then_some(i)).collect::<Vec<_>>();
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
    let form = form(s)?;
    form.apply_matching(definition, report)?;
    Ok(form.inspect())
}
fn parameters(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Parameters = parse(p)?;
    let form = form(s)?;
    let mut parameters = form.parameters.clone();
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
        parameters.path = value.into_iter().map(|p| crate::geom::Vec3::new(p[0], p[1], p[2])).collect();
    }
    form.set_parameters(parameters)?;
    Ok(form.inspect())
}
fn refine(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Refine = parse(p)?;
    let f = form(s)?;
    f.refine(&args.faces, args.counts)?;
    Ok(f.inspect())
}
fn insert_edge(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::InsertEdge = parse(p)?;
    if args.axis > 1 {
        return Err(EngineError::Other("insert axis must be U or V".into()));
    }
    let f = form(s)?;
    f.refine(&[args.face], if args.axis == 0 { [2, 1] } else { [1, 2] })?;
    Ok(f.inspect())
}
fn insert_points(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::InsertPoints = parse(p)?;
    let f = form(s)?;
    let points: Vec<_> = args.points.into_iter().map(|p| (p.edge, p.fraction)).collect();
    f.insert_points(&points)?;
    Ok(f.inspect())
}
fn insert_loop(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::InsertLoop = parse(p)?;
    let f = form(s)?;
    f.insert_loop(args.edge, args.fraction)?;
    Ok(f.inspect())
}
fn extrude_drag(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::ExtrudeDrag = parse(p)?;
    let f = form(s)?;
    f.extrude_drag(
        &args.faces,
        &args.edges,
        crate::geom::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]),
        args.checkpoint,
    )?;
    Ok(f.inspect())
}
fn project(s: &mut Session, p: &Value) -> Result<Value> {
    use crate::geom::subdivision::projection::{Projection, project};
    let args: params::Project = parse(p)?;
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
    let vector = |p: [f64; 3]| crate::geom::Vec3::new(p[0], p[1], p[2]);
    let positions: Vec<_> = project(&points, tool, args.direction.map(vector), args.origin.map(vector))
        .map_err(|e| EngineError::Other(e.to_string()))?
        .into_iter()
        .map(|p| [p.x, p.y, p.z])
        .collect();
    match args.points {
        params::ProjectionPoints::Control => form.set(&args.vertices, &positions, true)?,
        params::ProjectionPoints::Surface => form.set_surface(&args.vertices, &positions, true)?,
    }
    Ok(form.inspect())
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
    let mut definition = form.definition();
    pipe::recompute(s, &mut definition, form.authoring_component, form.editing_feature)?;
    if let Some(matching) = &definition.matching {
        definition = matching.recompute(&definition, &match_state(s), args.tolerance.unwrap_or(0.01))?.0;
    }
    if let Some(profile) = &definition.profile {
        let (profile, cage) = profile.recompute(&definition, &match_state(s))?;
        definition.profile = Some(Box::new(profile));
        definition.cage = cage;
    }
    if let Some(feature) = form.editing_feature.and_then(|id| s.doc.feature(id)) {
        let frame = if feature.component == s.active_component { s.active_component_frame() } else { s.doc.component_transform(feature.component) };
        let inverse = crate::doc::mat_inverse(&frame).ok_or_else(|| EngineError::Other("the Form component frame is not invertible".into()))?;
        definition.placement = crate::doc::forms::Placement::from_matrix(&crate::doc::mat_mul(&inverse, &definition.placement.matrix()))?;
        if let Some(matching) = &mut definition.matching {
            matching.source.transform_picks(&inverse);
        }
        if let Some(profile) = &mut definition.profile {
            profile.source.transform_picks(&inverse);
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
    let parts = form.parts.clone();
    let imported_name = form.imported.as_ref().map(|i| i.object.clone());
    let consumed = form.consumed.clone();
    for (&id, saved) in &consumed {
        if s.doc.feature(id) != Some(saved.as_ref()) {
            return Err(EngineError::Other("a Form selected for Weld changed; undo the weld and select it again".into()));
        }
    }
    let kind = crate::doc::FeatureKind::Form {
        definition: Box::new(definition),
        tolerance: args
            .tolerance
            .map(|v| format!("{v} mm").into())
            .unwrap_or_else(|| previous.as_ref().map_or_else(|| "0.01 mm".into(), |p| p.0.clone())),
        operation,
        targets: if p.get("targets").is_none() {
            previous.map(|p| p.2).unwrap_or_default()
        } else {
            args.targets.into_iter().map(Into::into).collect()
        },
    };
    for &id in consumed.keys() {
        s.doc_mut().delete_feature(id)?;
    }
    let mut result = if let Some(id) = editing_feature {
        let feature =
            s.doc_mut().features.iter_mut().find(|f| f.id == id).ok_or_else(|| EngineError::Other("the edited Form feature was removed".into()))?;
        feature.kind = kind;
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
            tolerance: args.tolerance.map_or_else(|| "0.01 mm".into(), |v| format!("{v} mm").into()),
            operation: crate::doc::Operation::NewBody,
            targets: Vec::new(),
        };
        let child = super::features::add_feature(s, &json!({"name":part.name}), kind)?;
        made.push(child.get("feature").cloned().ok_or_else(|| EngineError::Other("new Form did not return its identity".into()))?);
    }
    if !made.is_empty() {
        result["parts"] = Value::Array(made);
    }
    s.forms_prototype = None;
    Ok(result)
}
fn topology(s: &mut Session, p: &Value) -> Result<Value> {
    let mut args: params::Topology = parse(p)?;
    let form = form(s)?;
    if form.profile.is_some() {
        return Err(EngineError::Other("Form Surface keeps its fixed source boundary and guide grid".into()));
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
        form.refine(&args.faces, [2, 2])?;
        return Ok(form.inspect());
    }
    if form.hierarchy.is_some() && matches!(args.tool.as_str(), "crease" | "uncrease") {
        form.hierarchical_creases(&args.edges, &args.vertices, if args.tool == "crease" { args.distance } else { 0.0 })?;
        return Ok(form.inspect());
    }
    if args.tool == "delete_faces" && form.hierarchy.is_some() {
        form.delete_logical_faces(&args.faces)?;
        return Ok(form.inspect());
    }
    if args.tool == "dissolve" {
        form.dissolve_logical_edges(&args.edges)?;
        return Ok(form.inspect());
    }
    if args.tool == "bridge" {
        form.bridge_boundaries(&args.edges, &args.vertices)?;
        return Ok(form.inspect());
    }
    let c = &form.cage;
    let d = form.local_vector(crate::geom::Vec3::new(args.displacement[0], args.displacement[1], args.displacement[2]));
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
    form.set_symmetry(args.axis)?;
    Ok(form.inspect())
}
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::Edit = parse(p)?;
    if s.active_sketch.is_some() {
        return Err(EngineError::Other("finish the active sketch before editing a Form".into()));
    }
    if let Some(form) = &s.forms_prototype {
        if form.editing_feature == Some(args.feature) {
            return Ok(form.inspect());
        }
        return Err(EngineError::Other("finish or cancel the active Form before editing another feature".into()));
    }
    let feature = s.doc.features.iter().find(|f| f.id == args.feature).ok_or_else(|| EngineError::Other("no such feature".into()))?;
    let crate::doc::FeatureKind::Form { definition, .. } = &feature.kind else { return Err(EngineError::Other("select a Form feature".into())) };
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
    if let Some(matching) = &definition.matching {
        let before = s.model.state_before(feature.id);
        let state = crate::doc::state_in_frame(&s.doc, &before, feature.component);
        definition = matching.recompute(&definition, &state, 0.01)?.0;
    }
    if let Some(profile) = &definition.profile {
        let before = s.model.state_before(feature.id);
        let state = crate::doc::state_in_frame(&s.doc, &before, feature.component);
        let (profile, cage) = profile.recompute(&definition, &state)?;
        definition.profile = Some(Box::new(profile));
        definition.cage = cage;
    }
    let authoring_component = feature.component;
    let authoring_path = if s.active_component == authoring_component { s.active_occurrence_path.clone() } else { None };
    let frame = if s.active_component == authoring_component { s.active_component_frame() } else { s.doc.component_transform(authoring_component) };
    definition.placement = crate::doc::forms::Placement::from_matrix(&crate::doc::mat_mul(&frame, &definition.placement.matrix()))?;
    if let Some(matching) = &mut definition.matching {
        matching.source.transform_picks(&frame);
    }
    if let Some(profile) = &mut definition.profile {
        profile.source.transform_picks(&frame);
    }
    if let Some(pipe) = &mut definition.pipe {
        pipe.source.transform_picks(&frame);
    }
    let mut form = crate::forms_prototype::Form::from_definition(definition, levels, Some(feature.id))?;
    form.pipe_segments = pipe_segments;
    form.authoring_component = authoring_component;
    form.authoring_path = authoring_path;
    let reply = form.inspect();
    s.forms_prototype = Some(form);
    Ok(reply)
}
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let p: params::SetVertices = parse(p)?;
    let f = form(s)?;
    f.set(&p.vertices, &p.positions, p.checkpoint)?;
    Ok(f.inspect())
}
fn transform(s: &mut Session, p: &Value) -> Result<Value> {
    let p: params::Transform = parse(p)?;
    let f = form(s)?;
    f.transform(&p.matrix, p.checkpoint)?;
    Ok(f.inspect())
}
fn set_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let p: params::SetVertices = parse(p)?;
    let f = form(s)?;
    f.set_surface(&p.vertices, &p.positions, p.checkpoint)?;
    Ok(f.inspect())
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
    let f = form(s)?;
    f.unweld_parts(&args.edges, args.separate, args.name.as_deref().unwrap_or("Unwelded Form"))?;
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
    let f = form(s)?;
    f.history(false)?;
    Ok(f.inspect())
}
fn redo(s: &mut Session, _: &Value) -> Result<Value> {
    let f = form(s)?;
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
    fn blender_surface_export_evaluates_refinement_matching_profiles_and_staged_forms_without_finishing() {
        use crate::geom::Vec3;
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
        use crate::geom::{Vec3, subdivision::primitives::Parameters};
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
        use crate::geom::Vec3;
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
        use crate::geom::Vec3;
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
        use crate::geom::Vec3;
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
        use crate::geom::Vec3;
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
    fn a_cut_with_coincident_form_walls_preserves_the_base_until_the_kernel_can_trim_them() {
        use crate::geom::Vec3;
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
        let feature = s.execute("form.prototype.commit", &json!({"operation":"cut","tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        assert!(s.model.result(feature).unwrap().warning.is_some());
        let state = s.model.state();
        assert_eq!(state.bodies.len(), 2);
        assert!(state.body("Support").unwrap().body.same_shape(&base.body));
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
    fn associative_match_preview_cancel_history_and_upstream_recompute() {
        use crate::geom::Vec3;
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
    fn matching_support_picks_and_recomputation_use_the_authoring_component_frame() {
        use crate::geom::Vec3;
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
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.features.len(), 2);
        s.execute("form.edit", &json!({"feature":first})).unwrap();
        s.execute("form.prototype.weld", &params).unwrap();
        s.execute("form.prototype.undo", &json!({})).unwrap();
        assert!(!s.forms_prototype.as_ref().unwrap().consumes(second));
        s.execute("form.prototype.redo", &json!({})).unwrap();
        s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        assert_eq!(s.doc.features.len(), 1);
        assert!(s.doc.feature(first).is_some() && s.doc.feature(second).is_none());
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!(!s.model.state().bodies[0].body.is_surface());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.features.len(), 2);
        assert!(s.doc.feature(second).is_some());
    }
    #[test]
    fn whole_rigid_edits_keep_local_controls_and_partial_edits_use_world_coordinates() {
        use crate::geom::Vec3;
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
    fn whole_scaling_preserves_large_hierarchies_details_and_one_gesture_history() {
        use crate::geom::Vec3;
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
        use crate::geom::Vec3;
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
        use crate::geom::Vec3;
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
        use crate::geom::Vec3;
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
        cage.points.extend(other.points.iter().map(|p| *p + crate::geom::Vec3::Z * 4.0));
        cage.faces.extend(other.faces.iter().map(|f| f.iter().map(|v| v + offset).collect()));
        let mut h = Hierarchy::new(cage).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let control = *h.controls.iter().find(|(_, level)| **level == 1).unwrap().0;
        h.add_deltas(&[control], &[crate::geom::Vec3::Z * 0.2]).unwrap();
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
        let target = f.surface.vertices[id] + crate::geom::Vec3::new(0.4, 0.2, 0.3);
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
        let p = f.surface.vertices[id] + crate::geom::Vec3::Z;
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
        assert!(s.forms_prototype.is_some());
        s.execute("form.prototype.close", &json!({})).unwrap();
        assert!(!s.is_dirty());
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
            assert!(f.surface.vertices[i].dist(crate::geom::Vec3::new(target[0], target[1], target[2])) < 1e-8);
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
        let center = points.iter().copied().fold(crate::geom::Vec3::ZERO, |a, b| a + b) / points.len() as f64;
        let positions: Vec<_> = points
            .iter()
            .map(|&p| {
                let q = p - center;
                let (sn, cs) = 30_f64.to_radians().sin_cos();
                let p = center + crate::geom::Vec3::new(q.x * cs - q.y * sn, q.x * sn + q.y * cs, q.z);
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
        let target = old + crate::geom::Vec3::new(1.0, 2.0, 0.0);
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
