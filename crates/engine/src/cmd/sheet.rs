//! Sheet metal: rules, Flange (base, edge, contour), Path Flange, Hem, Unfold/Refold, Convert to Sheet Metal,
//! the flat pattern (outline, bend lines, bend table) and its DXF export.

use serde_json::{Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::sheet::{self, SheetBody, SheetRule};
use solidcraft_doc::{BodyName, SketchId};
use solidcraft_doc::{FacePick, Position};
use solidcraft_doc::{FeatureKind, FlangeEdge};
use solidcraft_geom::{Aabb2, Ring, Vec2, Vec3};

use super::CommandSpec;
use super::features::{add_feature, check_expr, feature_sketch, profiles, sketch_id};
use crate::params::{bad, bool_, expr, expr_problem, req_expr, str_, string_list, vec3};
use crate::schema::features::FeatureResult;
use crate::schema::sheet::{ConvertParams, FlangeParams, FoldParams, HemParams, PerforateParams, UnfoldParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const FLANGE_CONTRACT: TypedContract<FlangeParams> = TypedContract::new();
const PERFORATE_CONTRACT: TypedContract<PerforateParams> = TypedContract::new();
const HEM_CONTRACT: TypedContract<HemParams> = TypedContract::new();
const FOLD_CONTRACT: TypedContract<FoldParams> = TypedContract::new();
const UNFOLD_CONTRACT: TypedContract<UnfoldParams> = TypedContract::new();
const CONVERT_CONTRACT: TypedContract<ConvertParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sheet.flange", "Flange", crate::checked!("sheet.flange", flange))
        .at("SHEET METAL", "CREATE")
        .icon("flange")
        .contract(&FLANGE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(
        "base: sketch + profiles? (rule?, flip?) | contour: sketch + curves (open chain of lines) + distance (rule?, flip?, reverse?) | \
         edge: edges: [[x,y,z] on top/bottom edges, or {point: [x,y,z], angle?, height?} for an edge with its own angle and height] + \
         height (along the flange, to its far edge), angle? (default 90 deg), radius? (default: rule), \
         height_from?: outer|inner|tangent (where the height is measured from: the outer virtual sharp (default), \
         the inner virtual sharp, or past 90 deg the line square to the flange touching the bend's outside (tangent; up to 90 deg the outer sharp)), \
         position?: inside|outside|middle|material_outside|tangent (where the bend starts: the outer virtual sharp on the edge (default), \
         the bend starting at the edge (bend_outside is the same), half way, the inner virtual sharp on the edge, \
         or the bend's outer face tangent to the edge's face (below 90 deg the bend's end on that face)), flip?, body?, corner_relief_shape?: notch|round|rectangular|tear and corner_relief? (size across): \
         the relief at the corners these flanges make (default: the rule's); \
         then?: [{angle?, height, flip?}] further bends (at most 16), each a flange on the far edge of the one before, flip: true turns that bend the other way (edge only); \
         type?: base|edge|contour (default from the inputs); name?, body_name? (the new body). \
         Thickness, K-factor and bend radius come from the rule (sheet.manage_rules); passing them here is refused, \
         and so is the corner relief on a base or contour flange",
    ),
    CommandSpec::new("sheet.path_flange", "Path Flange", path_flange).at("SHEET METAL", "CREATE").icon("path_flange").params(
        "edge: [x,y,z] on a top or bottom edge of the sheet; sketch + curves: an open chain of sketch lines drawn on a plane across \
         that edge, from the edge out (each line one flange as long as the line, to the outer face from the virtual sharp; each \
         corner one bend, turning the way the lines turn); radius? (default: rule); body?. One feature, one body",
    ),
    CommandSpec::new("sheet.perforate", "Perforate", crate::checked!("sheet.perforate", perforate))
        .at("SHEET METAL", "CREATE")
        .icon("perforate")
        .contract(&PERFORATE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(
        "diameter, pitch: expr; margin?: expr (clear of edges, cut-outs and bends; default: the diameter); pattern?: staggered (60°, default) | square; body? - a perforation field over the whole flat sheet: holes in the flat pattern (perforations), the DXF (layer PERFORATION) and the measured volume, not cut into the B-rep (#732)",
    ),
    CommandSpec::new("sheet.hem", "Hem", crate::checked!("sheet.hem", hem))
        .at("SHEET METAL", "CREATE")
        .icon("hem")
        .contract(&HEM_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("edges: [[x,y,z] on sheet edges]; length (to the outer face); gap? (inner radius, default: rule); flip?; body?"),
    CommandSpec::new("sheet.fold", "Fold", crate::checked!("sheet.fold", fold))
        .at("SHEET METAL", "CREATE")
        .icon("fold")
        .contract(&FOLD_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(
        "line: sketch + curve (a sketch line on the sheet's base face; followed when the sketch changes) or points: [[x,y,z], [x,y,z]]; \
         angle? (default 90 deg); radius? (default: rule); position?: centerline|start|end|mould (where the line sits in the bend, default centerline); \
         fixed?: [x,y,z] on the side that stays (default: the larger side); flip? (fold the other way); body?",
    ),
    CommandSpec::new("sheet.unfold", "Unfold", crate::checked!("sheet.unfold", unfold))
        .at("SHEET METAL", "MODIFY")
        .icon("unfold")
        .contract(&UNFOLD_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body? (default: the last sheet body) — all bends"),
    CommandSpec::new("sheet.refold", "Refold", crate::checked!("sheet.refold", refold))
        .at("SHEET METAL", "MODIFY")
        .icon("refold")
        .contract(&UNFOLD_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body? (default: the last unfolded sheet)"),
    CommandSpec::new("sheet.convert", "Convert to Sheet Metal", crate::checked!("sheet.convert", convert))
        .at("SHEET METAL", "CREATE")
        .icon("convert_sheet")
        .contract(&CONVERT_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(
            "body; face: [x,y,z] on its large flat face (a plate of even thickness; recesses in one face up to half the thickness deep, \
             such as an engraved label, become pockets of the sheet), or that face's name from model.faces; rule?; \
             bends?: [[x,y,z] on sharp edges] (a solid or a shelled body: `face` is the fixed face and the flat faces reached from it \
             across these edges become the sheet; a shell keeps its thickness, a solid takes the rule's inward; \
             edges between two of those faces that are no bends are cut open with the rule's gap; \
             a point in the middle of a chamfer between two faces, or on a bend modelled with its radius (a cylinder between \
             two flat faces tangent to it, either skin), makes it one bend of the rule's radius at their virtual sharp; \
             the faces' holes become cut-outs and their engravings, up to half the thickness deep, pockets)",
        ),
    CommandSpec::new("sheet.manage_rules", "Sheet Metal Rules", rules).at("SHEET METAL", "MODIFY").icon("sheet_rules").params(
        "name (new or existing); thickness?, k_factor?, bend_radius?, relief_width?, relief_depth?, corner_relief?, hem_gap?, gap? (expressions; `Thickness` is the rule's; \
         gap: between flanges that meet at a corner, whose ends are closed toward each other at any angle); \
         corner_relief_shape?: notch|round|rectangular|tear (what is cut where two bends meet at a corner of the base: round and rectangular a relief \
         `corner_relief` across; tear none, each bend's end is cut straight from the corner to its flange's end; default notch); \
         angle_display?: bend|included (how the DXF bend notes read the bend angles: the bend angle, default, or the included angle \
         left between the bend's sides, 180° less it; the bend table gives both, and a flange's `angle` is always the bend angle); \
         unfold_method?: k_factor|deduction|allowance (what sets the K-factor every bend unfolds by: `k_factor`, default, or a press brake \
         chart's bend deduction or bend allowance of a 90° bend at the rule's thickness and bend_radius, given as unfold_value?: length \
         expression; the rule's values then carry the K-factor that gives exactly that, which may be above 1 but not below 0); \
         active?: bool; delete?: bool",
    ),
    CommandSpec::new("sheet.rules", "List Sheet Metal Rules", list_rules).icon("sheet_rules").noundo().doc_only().params("→ rules with their expressions and values, and the active one"),
    CommandSpec::new("sheet.flat_pattern", "Create Flat Pattern", flat_pattern)
        .at("SHEET METAL", "CREATE")
        .icon("flat")
        .noundo()
        .params(
            "body? → flat size, outline loops and cut-outs (flat coordinates, as point lists), cutout_shapes (the same cut-outs exactly, in the same order: {circle: {center, radius}} for a round hole, else {segments: [{line: [a, b]} | {arc: {center, radius, start_deg, sweep_deg}} | {polyline: [...]}]}), pockets (with their own cutout_shapes), bend lines and the bend table",
        ),
    CommandSpec::new("sheet.export_dxf", "Export Flat Pattern as DXF", export_dxf).icon("dxf")
        .noundo()
        .params(
            "body?, path?, curves?: exact|arcs (default exact: an ellipse as ELLIPSE, splines, text outlines and other Béziers as SPLINE; arcs: tangent arcs and lines within 0.001 mm, for machines that read only those), \
             text?: cut|etch (default cut: letters that text cut through the sheet are cut contours on layer TEXT; etch: single contours on layer ETCH and the sheet keeps its material there; text machined to a depth is always on ETCH), \
             bend_notes?: bool (default false: a note beside each bend line on layer BEND_NOTES, `B1 UP 90%%d R1`, its angle the included angle \
              when the body's rule has angle_display: included), note_height?: mm (default 3), \
             bend_lines?: bool (default true; false leaves BEND_UP and BEND_DOWN out, for a cutter that would cut them), \
             layers?: roles|single (default roles, the layers below; single: every entity on layer 0), \
             per_body?: bool (a file for every sheet body, `<path>-<body>.dxf`, or their DXF texts without a path → files: [{body, path | dxf, bends}]; not with `body`) \
             → DXF (AutoCAD 2000, mm): layers OUTLINE (the outer contour), CUT (what is cut out inside it), TEXT or ETCH (letters), BEND_UP and BEND_DOWN (dashed centre lines), BEND_NOTES, \
             POCKET_<BOTTOM|TOP|INTERNAL>_Z<from>_TO_<to>_MM (one per side and depth) and PERFORATION; inner contours come before the outline",
        ),
    CommandSpec::new("sheet.export_bend_table", "Export Bend Table", export_bend_table).icon("bendtable")
        .noundo()
        .params("body?, format?: csv|json (default csv), path? → bend table in mm and degrees, including rule and flat centre-line coordinates; angle_deg is the bend angle and included_angle_deg the angle left between the bend's sides (180° less it); a bend that cut-outs split has a row for each piece"),
];

fn points(p: &Value, k: &str, cmd: &str) -> Result<Vec<Vec3>> {
    sheet_points(p.get(k)).map_err(|why| bad(cmd, format!("`{k}` {why}")))
}

/// What [`points`] reads a list of sheet-edge picks as, without the design: 1…1000 points
/// `[x, y, z]`, `{x, y, z}` or `{point}`. The error follows the key's name.
pub(crate) fn sheet_points(v: Option<&Value>) -> std::result::Result<Vec<Vec3>, &'static str> {
    let a = v.and_then(Value::as_array).ok_or("must list points [x, y, z] on sheet edges")?;
    if a.is_empty() || a.len() > 1000 {
        return Err("must list 1…1000 points");
    }
    a.iter().map(|v| vec3(v).or_else(|| v.get("point").and_then(vec3)).ok_or("must contain [x, y, z] points")).collect()
}

/// The `edges` of an edge flange: points `[x, y, z]` on sheet edges, or `{point, angle?, height?}`
/// for an edge with its own angle and height (expressions, checked like the feature's).
/// Most bends `then` may add after an edge flange's own.
const MAX_THEN: usize = 16;

/// One further bend of an edge flange: its angle, its height and, if it has its own, which way
/// it turns (`flip`; the flange's own otherwise).
pub(crate) type ThenStep = (solidcraft_doc::Expr, solidcraft_doc::Expr, Option<bool>);

/// The further bends of an edge flange (`then`): each an angle and a height, made one after
/// the other, every one on the far edge of the flange before it.
fn then_steps(s: &Session, p: &Value, cmd: &str) -> Result<Vec<ThenStep>> {
    let steps = then_list(p.get("then")).map_err(|why| bad(cmd, why))?;
    for (i, (angle, height, _)) in steps.iter().enumerate() {
        check_expr(s, height, Kind::Length, cmd, &format!("then[{i}].height"))?;
        check_expr(s, angle, Kind::Angle, cmd, &format!("then[{i}].angle"))?;
    }
    Ok(steps)
}

/// What [`then_steps`] reads `then` as, without the design: up to [`MAX_THEN`] bends
/// `{angle?, height, flip?}` (the angle 90° when it is left out, null or not an expression).
/// Whether the expressions suit the design is checked by the handler.
pub(crate) fn then_list(v: Option<&Value>) -> std::result::Result<Vec<ThenStep>, String> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let list = "`then` must list further bends {angle?, height, flip?}: each on the far edge of the flange before it";
    let a = v.as_array().ok_or(list)?;
    if a.len() > MAX_THEN {
        return Err(format!("`then` may list at most {MAX_THEN} bends"));
    }
    let mut out = Vec::with_capacity(a.len());
    for (i, step) in a.iter().enumerate() {
        let o = step.as_object().ok_or_else(|| format!("then[{i}]: {list}"))?;
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "angle" | "height" | "flip")) {
            return Err(format!("then[{i}]: unknown key `{k}` (a bend takes angle, height and flip)"));
        }
        let flip = match o.get("flip").filter(|v| !v.is_null()) {
            Some(v) => Some(v.as_bool().ok_or_else(|| format!("then[{i}]: `flip` must be true or false"))?),
            None => None,
        };
        let height = expr(step, "height").ok_or_else(|| format!("then[{i}]: `height` is required"))?;
        let angle = expr(step, "angle").unwrap_or_else(|| "90 deg".into());
        out.push((angle, height, flip));
    }
    Ok(out)
}

/// Where to pick the far edges of the flanges feature `id` made: a point on each, for the flanges
/// of the next bend of a chain.
fn far_edge_picks(s: &Session, id: u64, cmd: &str) -> Result<Vec<FlangeEdge>> {
    let prefix = format!("F{id}.");
    let st = s.model.state();
    let picks: Vec<FlangeEdge> = st
        .sheets
        .iter()
        .flat_map(|sh| sh.flanges.iter().enumerate().filter(|(_, f)| f.tag.starts_with(&prefix)).filter_map(|(i, _)| sh.far_edge_pick(i)))
        .map(FlangeEdge::at)
        .collect();
    if picks.is_empty() {
        return Err(bad(cmd, "the flange has no far edge to bend again"));
    }
    Ok(picks)
}

fn flange_edges(s: &Session, p: &Value, cmd: &str) -> Result<Vec<FlangeEdge>> {
    let a = flange_edge_list(p.get("edges")).map_err(|why| bad(cmd, why))?;
    let mut edges = Vec::with_capacity(a.len());
    for (i, v) in a.iter().enumerate() {
        let edge = flange_edge(i, v).map_err(|why| bad(cmd, why))?;
        for (k, e, kind) in [("angle", &edge.angle, Kind::Angle), ("height", &edge.height, Kind::Length)] {
            if let Some(e) = e {
                check_expr(s, e, kind, cmd, &format!("edges[{i}].{k}"))?;
            }
        }
        edges.push(edge);
    }
    Ok(edges)
}

/// The message for `edges` that are not a list of points or edges.
const FLANGE_EDGE_LIST: &str = "`edges` must list points [x, y, z] on sheet edges, or {point, angle?, height?}";

/// An edge flange's `edges` as a list of 1…1000 items, without the design.
pub(crate) fn flange_edge_list(v: Option<&Value>) -> std::result::Result<&Vec<Value>, &'static str> {
    let a = v.and_then(Value::as_array).ok_or(FLANGE_EDGE_LIST)?;
    if a.is_empty() || a.len() > 1000 {
        return Err("`edges` must list 1…1000 points");
    }
    Ok(a)
}

/// Item `i` of an edge flange's `edges`, without the design: whether its expressions suit the
/// design is checked by the handler.
pub(crate) fn flange_edge(i: usize, v: &Value) -> std::result::Result<FlangeEdge, String> {
    const KEYS: [&str; 3] = ["point", "angle", "height"];
    // A bare point, as before: [x, y, z] or {x, y, z}.
    let Some(o) = v.as_object().filter(|o| KEYS.iter().any(|k| o.contains_key(*k))) else {
        return vec3(v).map(FlangeEdge::at).ok_or_else(|| format!("edges[{i}]: {FLANGE_EDGE_LIST}"));
    };
    if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!("edges[{i}]: unknown key `{k}` (an edge takes point, angle and height)"));
    }
    let point = o.get("point").and_then(vec3).ok_or_else(|| format!("edges[{i}]: `point` must be [x, y, z]"))?;
    let own = |k: &str| -> std::result::Result<Option<solidcraft_doc::Expr>, String> {
        if o.get(k).is_none_or(Value::is_null) {
            return Ok(None);
        }
        expr(v, k).map(Some).ok_or_else(|| format!("edges[{i}]: `{k}` {}", expr_problem(v.get(k))))
    };
    Ok(FlangeEdge { point: Position(point), angle: own("angle")?, height: own("height")? })
}

fn rule_param(s: &Session, p: &Value, cmd: &str) -> Result<Option<String>> {
    match str_(p, "rule") {
        Some(r) if s.doc.sheet.rules.iter().any(|x| x.name == r) || r == SheetRule::steel().name => Ok(Some(r.to_string())),
        Some(r) => Err(bad(cmd, format!("no sheet metal rule `{r}` (sheet.manage_rules makes one)"))),
        None => Ok(None),
    }
}

/// Sheet values a flange takes from its rule. Passing one to `sheet.flange` is refused rather
/// than ignored, so a script never gets a part of another thickness than it asked for.
const RULE_ONLY: [&str; 10] = [
    "thickness",
    "k_factor",
    "bend_radius",
    "relief_width",
    "relief_depth",
    "corner_relief",
    "corner_relief_shape",
    "hem_gap",
    "unfold_method",
    "unfold_value",
];

/// The two rule values an edge flange may set for itself: the corner relief.
const EDGE_RELIEF: [&str; 2] = ["corner_relief", "corner_relief_shape"];

fn flange(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.flange";
    let nested = p.get("base").filter(|b| b.is_object());
    let ty = match str_(p, "type") {
        Some(t) => t.to_ascii_lowercase(),
        None if p.get("edges").is_some() => "edge".into(),
        None if p.get("curves").is_some() => "contour".into(),
        None => "base".into(),
    };
    let refused = |k: &&&str| !(ty == "edge" && EDGE_RELIEF.contains(*k));
    if let Some(key) = RULE_ONLY.iter().filter(refused).find(|k| p.get(**k).is_some() || nested.is_some_and(|b| b.get(**k).is_some())) {
        return Err(bad(
            cmd,
            format!(
                "`{key}` is not a sheet.flange parameter: the sheet metal rule sets it. Make a rule with \
                 sheet.manage_rules {{name, {key}, …}} and pass its name as `rule`"
            ),
        ));
    }
    if ty != "edge" && p.get("then").is_some_and(|v| !v.is_null()) {
        return Err(bad(cmd, "`then` (further bends) is only for edge flanges"));
    }
    match flange_type(&ty).map_err(|why| bad(cmd, why))? {
        FlangeType::Base => {
            let sketch = feature_sketch(s, p, cmd)?;
            let rule = rule_param(s, p, cmd)?;
            add_feature(
                s,
                p,
                FeatureKind::SheetBase { sketch: SketchId(sketch), profiles: profiles(p, cmd)?, rule, flip: bool_(p, "flip").unwrap_or(false) },
            )
        }
        FlangeType::Contour => {
            let sketch = sketch_id(s, p.get("sketch"), cmd, "sketch")?;
            let curves = string_list(cmd, p, "curves")?;
            if curves.is_empty() || curves.len() > 1000 {
                return Err(bad(cmd, "`curves` must list the open chain's lines"));
            }
            let distance = req_expr(cmd, p, "distance")?;
            check_expr(s, &distance, Kind::Length, cmd, "distance")?;
            let rule = rule_param(s, p, cmd)?;
            add_feature(
                s,
                p,
                FeatureKind::SheetContour {
                    sketch: SketchId(sketch),
                    curves,
                    distance,
                    rule,
                    flip: bool_(p, "flip").unwrap_or(false),
                    reverse: bool_(p, "reverse").unwrap_or(false),
                },
            )
        }
        FlangeType::Edge => {
            let edges = flange_edges(s, p, cmd)?;
            let then = then_steps(s, p, cmd)?;
            let height = req_expr(cmd, p, "height")?;
            check_expr(s, &height, Kind::Length, cmd, "height")?;
            let angle = expr(p, "angle").unwrap_or_else(|| "90 deg".into());
            check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
            let radius = expr(p, "radius");
            if let Some(r) = &radius {
                check_expr(s, r, Kind::Length, cmd, "radius")?;
            }
            let position = flange_position(str_(p, "position").unwrap_or("inside"))
                .ok_or_else(|| bad(cmd, "`position` must be inside, outside, middle, material_outside or tangent"))?;
            // Where the heights are measured from; the outer sharp (the default) is saved as nothing.
            let height_from = match p.get("height_from").filter(|v| !v.is_null()) {
                Some(v) => v.as_str().and_then(sheet::HeightFrom::parse).ok_or_else(|| bad(cmd, "`height_from` must be outer, inner or tangent"))?,
                None => sheet::HeightFrom::Outer,
            };
            let height_from = (height_from != sheet::HeightFrom::Outer).then(|| height_from.name().to_string());
            if s.model.state().sheets.is_empty() {
                return Err(no_sheet_body(cmd));
            }
            let body = str_(p, "body").map(str::to_string);
            // The corner relief these flanges make, in place of the rule's; null is not given.
            let corner_relief_shape = match p.get("corner_relief_shape").filter(|v| !v.is_null()) {
                Some(v) => Some(
                    v.as_str()
                        .and_then(solidcraft_doc::sheet::ReliefShape::parse)
                        .map(|shape| shape.name().to_string())
                        .ok_or_else(|| bad(cmd, "`corner_relief_shape` must be notch, round, rectangular or tear"))?,
                ),
                None => None,
            };
            let corner_relief = expr(p, "corner_relief");
            if let Some(c) = &corner_relief {
                check_expr(s, c, Kind::Length, cmd, "corner_relief")?;
            }
            let flip = bool_(p, "flip").unwrap_or(false);
            let mut made = add_feature(
                s,
                p,
                FeatureKind::SheetFlange {
                    edges,
                    height,
                    angle,
                    radius: radius.clone(),
                    position: position.clone(),
                    flip,
                    body: body.clone().map(BodyName),
                    corner_relief_shape,
                    corner_relief,
                    height_from: height_from.clone(),
                },
            )?;
            // Each further bend stands on the far edge of the flange before it.
            for (angle, height, own_flip) in then {
                let id = made.get("feature").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "no feature to bend again"))?;
                let edges = far_edge_picks(s, id, cmd)?;
                made = add_feature(
                    s,
                    &json!({}),
                    FeatureKind::SheetFlange {
                        edges,
                        height,
                        angle,
                        radius: radius.clone(),
                        position: position.clone(),
                        flip: own_flip.unwrap_or(flip),
                        body: body.clone().map(BodyName),
                        corner_relief_shape: None,
                        corner_relief: None,
                        height_from: height_from.clone(),
                    },
                )?;
            }
            Ok(made)
        }
    }
}

/// The three kinds of flange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlangeType {
    Base,
    Edge,
    Contour,
}

/// A flange's `type`, in any case.
pub(crate) fn flange_type(t: &str) -> std::result::Result<FlangeType, String> {
    match t.to_ascii_lowercase().as_str() {
        "base" => Ok(FlangeType::Base),
        "edge" => Ok(FlangeType::Edge),
        "contour" => Ok(FlangeType::Contour),
        other => Err(format!("unknown flange type `{other}` (base, edge or contour)")),
    }
}

/// An edge flange's `position` (in any case), lower-cased; the bend starting at the edge is
/// `outside`, whichever name asked for it (`bend_outside` is the same).
pub(crate) fn flange_position(t: &str) -> Option<String> {
    match t.to_ascii_lowercase().as_str() {
        "bend_outside" => Some("outside".into()),
        t @ ("inside" | "outside" | "middle" | "material_outside" | "tangent") => Some(t.into()),
        _ => None,
    }
}

/// A fold's `position` (in any case), lower-cased.
pub(crate) fn fold_position(t: &str) -> Option<String> {
    Some(t.to_ascii_lowercase()).filter(|t| matches!(t.as_str(), "centerline" | "start" | "end" | "mould" | "mold"))
}

/// A fold's `points`: the fold line's two ends. The error follows the key's name.
pub(crate) fn fold_points(v: &Value) -> std::result::Result<(Vec3, Vec3), &'static str> {
    let pts = v.as_array().filter(|a| a.len() == 2).ok_or("must be two points [x, y, z]")?;
    let a = pts.first().and_then(vec3).ok_or("must be two finite points [x, y, z]")?;
    let b = pts.get(1).and_then(vec3).ok_or("must be two finite points [x, y, z]")?;
    Ok((a, b))
}

/// A perforation's `pattern`.
pub(crate) fn perforation_pattern(t: &str) -> std::result::Result<&str, &'static str> {
    match t {
        "staggered" | "square" => Ok(t),
        _ => Err("`pattern` is \"staggered\" (60°, default) or \"square\""),
    }
}

fn perforate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.perforate";
    let diameter = req_expr(cmd, p, "diameter")?;
    check_expr(s, &diameter, Kind::Length, cmd, "diameter")?;
    let pitch = req_expr(cmd, p, "pitch")?;
    check_expr(s, &pitch, Kind::Length, cmd, "pitch")?;
    let margin = expr(p, "margin");
    if let Some(m) = &margin {
        check_expr(s, m, Kind::Length, cmd, "margin")?;
    }
    let pattern = str_(p, "pattern").map(|t| perforation_pattern(t).map(str::to_string).map_err(|why| bad(cmd, why))).transpose()?;
    add_feature(s, p, FeatureKind::SheetPerforate { diameter, pitch, margin, pattern, body: str_(p, "body").map(BodyName::from) })
}

fn hem(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.hem";
    let edges = points(p, "edges", cmd)?;
    let length = req_expr(cmd, p, "length")?;
    check_expr(s, &length, Kind::Length, cmd, "length")?;
    let gap = expr(p, "gap");
    if let Some(g) = &gap {
        check_expr(s, g, Kind::Length, cmd, "gap")?;
    }
    add_feature(
        s,
        p,
        FeatureKind::SheetHem {
            edges: edges.into_iter().map(Position).collect(),
            length,
            gap,
            flip: bool_(p, "flip").unwrap_or(false),
            body: str_(p, "body").map(BodyName::from),
        },
    )
}

fn fold(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.fold";
    let (sketch, curve, a, b) = match p.get("points") {
        Some(v) => {
            let (a, b) = fold_points(v).map_err(|why| bad(cmd, format!("`points` {why}")))?;
            (None, None, a, b)
        }
        None => {
            let sk = sketch_id(s, p.get("sketch"), cmd, "sketch")?;
            let c = str_(p, "curve").ok_or_else(|| bad(cmd, "give `sketch` + `curve` (a sketch line) or `points`"))?.to_string();
            let st = s.model.state();
            let ss = st.sketch(sk).ok_or_else(|| bad(cmd, "no such sketch"))?;
            let ci = ss.sketch.curve_index(&c).ok_or_else(|| bad(cmd, format!("no curve `{c}` in the sketch")))?;
            let (a, b) = match ss.sketch.segs(ci).as_slice() {
                [solidcraft_geom::Seg2::Line { a, b }] => (ss.plane.to_world(*a), ss.plane.to_world(*b)),
                _ => return Err(bad(cmd, "the fold line must be a sketch line")),
            };
            (Some(sk), Some(c), a, b)
        }
    };
    if a.dist(b) < 1e-9 {
        return Err(bad(cmd, "the fold line has no length"));
    }
    let angle = expr(p, "angle").unwrap_or_else(|| "90 deg".into());
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    let radius = expr(p, "radius");
    if let Some(r) = &radius {
        check_expr(s, r, Kind::Length, cmd, "radius")?;
    }
    let position =
        fold_position(str_(p, "position").unwrap_or("centerline")).ok_or_else(|| bad(cmd, "`position` must be centerline, start, end or mould"))?;
    let fixed = match p.get("fixed") {
        Some(v) => Some(vec3(v).ok_or_else(|| bad(cmd, "`fixed` must be a point [x, y, z]"))?),
        None => None,
    };
    let body = str_(p, "body").map(str::to_string);
    let r = add_feature(
        s,
        p,
        FeatureKind::SheetFold {
            sketch: sketch.map(SketchId),
            curve,
            a: Position(a),
            b: Position(b),
            angle,
            radius,
            position,
            flip: bool_(p, "flip").unwrap_or(false),
            fixed: fixed.map(Position),
            body: body.map(BodyName),
        },
    )?;
    // A recompute keeps a stored fold whose fixed point no longer touches the sheet (with a
    // warning); a new fold refuses one, since it picks no side (#263).
    if let Some(f) = fixed {
        let id = r.get("feature").and_then(Value::as_u64);
        let off =
            id.and_then(|id| s.model.result(id)).and_then(|res| res.warning.as_deref()).is_some_and(|w| w.contains(sheet::FOLD_FIXED_OFF_SHEET));
        if off {
            return Err(bad(
                cmd,
                format!("the fixed point ({:.3}, {:.3}, {:.3}) is not on the sheet: pick a point on the face of the side that stays", f.x, f.y, f.z),
            ));
        }
    }
    Ok(r)
}

fn path_flange(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.path_flange";
    let edge = p.get("edge").and_then(vec3).ok_or_else(|| bad(cmd, "`edge` must be a point [x, y, z] on a sheet edge"))?;
    let sketch = sketch_id(s, p.get("sketch"), cmd, "sketch")?;
    let curves = string_list(cmd, p, "curves")?;
    if curves.is_empty() || curves.len() > sheet::MAX_PATH_LINES {
        return Err(bad(cmd, format!("`curves` must list the path's lines (1…{})", sheet::MAX_PATH_LINES)));
    }
    let radius = expr(p, "radius");
    if let Some(r) = &radius {
        check_expr(s, r, Kind::Length, cmd, "radius")?;
    }
    add_feature(
        s,
        p,
        FeatureKind::SheetPath { edge: Position(edge), sketch: SketchId(sketch), curves, radius, body: str_(p, "body").map(BodyName::from) },
    )
}

fn unfold(s: &mut Session, p: &Value) -> Result<Value> {
    add_feature(s, p, FeatureKind::SheetUnfold { body: str_(p, "body").map(BodyName::from), refold: false })
}

fn refold(s: &mut Session, p: &Value) -> Result<Value> {
    add_feature(s, p, FeatureKind::SheetUnfold { body: str_(p, "body").map(BodyName::from), refold: true })
}

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.convert";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let face = p.get("face").ok_or_else(|| bad(cmd, "`face` must be a point [x, y, z] on the plate's face, or its name from model.faces"))?;
    let face = super::face::face_ref(s, face, Some(&body), cmd)?;
    // With bend edges, a solid or shelled body; without, a plate.
    let bends = match p.get("bends") {
        None | Some(Value::Null) => Vec::new(),
        Some(_) => points(p, "bends", cmd)?,
    };
    // Such a sheet's values are its rule's (a plate's thickness is the plate's own).
    if let Some(key) = RULE_ONLY.iter().find(|k| !bends.is_empty() && p.get(**k).is_some()) {
        return Err(bad(cmd, format!("`{key}` is not a sheet.convert parameter: the sheet metal rule sets it. Pass a rule's name as `rule`")));
    }
    let rule = rule_param(s, p, cmd)?;
    add_feature(
        s,
        p,
        FeatureKind::SheetConvert { body: BodyName(body), face: FacePick(face), rule, bends: bends.into_iter().map(Position).collect() },
    )
}

fn rules(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.manage_rules";
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    if bool_(p, "delete").unwrap_or(false) {
        let doc = s.doc_mut();
        let before = doc.sheet.rules.len();
        doc.sheet.rules.retain(|r| r.name != name);
        if doc.sheet.rules.len() == before {
            return Err(bad(cmd, format!("no rule `{name}`")));
        }
        if doc.sheet.active.as_deref() == Some(name.as_str()) {
            doc.sheet.active = None;
        }
        return Ok(json!({"deleted": name}));
    }
    let mut rule =
        s.doc.sheet.rules.iter().find(|r| r.name == name).cloned().unwrap_or_else(|| SheetRule { name: name.clone(), ..s.doc.sheet_rule(None) });
    for (k, slot) in rule.fields_mut() {
        if let Some(e) = expr(p, k) {
            *slot = e.into();
        }
    }
    if let Some(shape) = p.get("corner_relief_shape").filter(|v| !v.is_null()) {
        let shape = shape.as_str().and_then(solidcraft_doc::sheet::ReliefShape::parse);
        rule.corner_relief_shape = shape.ok_or_else(|| bad(cmd, "`corner_relief_shape` must be notch, round, rectangular or tear"))?.name().into();
    }
    if let Some(display) = p.get("angle_display").filter(|v| !v.is_null()) {
        let display = display.as_str().and_then(sheet::AngleDisplay::parse);
        rule.angle_display = display.ok_or_else(|| bad(cmd, "`angle_display` must be bend or included"))?.name().into();
    }
    if let Some(method) = p.get("unfold_method").filter(|v| !v.is_null()) {
        let method = method.as_str().and_then(sheet::UnfoldMethod::parse);
        rule.unfold_method = method.ok_or_else(|| bad(cmd, "`unfold_method` must be k_factor, deduction or allowance"))?.name().into();
    }
    let (vals, _) = s.doc.param_values();
    let values = s.doc.rule_values(&vals, &rule).map_err(|e| bad(cmd, e.to_string()))?;
    let doc = s.doc_mut();
    match doc.sheet.rules.iter_mut().find(|r| r.name == name) {
        Some(r) => *r = rule.clone(),
        None => {
            if doc.sheet.rules.len() >= 1000 {
                return Err(EngineError::Other("too many rules".into()));
            }
            doc.sheet.rules.push(rule.clone());
        }
    }
    if bool_(p, "active").unwrap_or(doc.sheet.active.is_none()) {
        doc.sheet.active = Some(name.clone());
    }
    s.refresh();
    let errors: Vec<Value> = s.model.results.iter().filter_map(|r| r.error.as_ref().map(|e| json!({"feature": r.name, "error": e}))).collect();
    Ok(json!({"rule": rule_json(&rule), "values": values, "errors": errors}))
}

/// A rule as the rule commands list it: as saved, and with its `angle_display`, `unfold_method`
/// and `unfold_value` also while they are the defaults a design leaves out.
fn rule_json(r: &SheetRule) -> Value {
    let mut v = json!(r);
    if let Value::Object(m) = &mut v {
        m.insert("angle_display".into(), json!(r.angle_display));
        m.insert("unfold_method".into(), json!(r.unfold_method));
        m.insert("unfold_value".into(), json!(r.unfold_value));
    }
    v
}

fn list_rules(s: &mut Session, _p: &Value) -> Result<Value> {
    let (vals, _) = s.doc.param_values();
    let mut all = s.doc.sheet.rules.clone();
    if all.is_empty() {
        all.push(SheetRule::steel());
    }
    let rules: Vec<Value> = all.iter().map(|r| json!({"rule": rule_json(r), "values": s.doc.rule_values(&vals, r).ok()})).collect();
    let active = s.doc.sheet_rule(None).name;
    Ok(json!({"rules": rules, "active": active}))
}

fn no_sheet_body(cmd: &str) -> EngineError {
    bad(
        cmd,
        "there is no sheet metal body: start one with Flange > Base, or turn a plate into sheet metal with Convert to Sheet Metal (pick its large flat face)",
    )
}

fn sheet_of(s: &Session, p: &Value, cmd: &str) -> Result<SheetBody> {
    let st = s.model.state();
    match str_(p, "body") {
        Some(b) => st.sheets.iter().find(|x| x.body == b).cloned().ok_or_else(|| bad(cmd, format!("`{b}` is not a sheet metal body"))),
        None => st.sheets.last().cloned().ok_or_else(|| no_sheet_body(cmd)),
    }
}

/// The centre and radius of a loop that is one whole circle (often two semicircles).
fn loop_circle(lp: &solidcraft_geom::Loop2) -> Option<(Vec2, f64)> {
    use solidcraft_geom::Seg2;
    let Some(Seg2::Arc { center, radius, sweep, .. }) = lp.segs.first() else { return None };
    let total: f64 = lp.segs.iter().filter_map(|s| if let Seg2::Arc { sweep, .. } = s { Some(*sweep) } else { None }).sum();
    let circular = lp.segs.iter().all(
        |s| matches!(s, Seg2::Arc { center: c, radius: r, sweep: a, .. } if c.dist(*center) < 1e-9 && (*r - *radius).abs() < 1e-9 && a.signum() == sweep.signum()),
    );
    (circular && (total.abs() - std::f64::consts::TAU).abs() < 1e-9).then_some((*center, *radius))
}

/// A loop's exact shape for the flat pattern JSON: a circle, or its lines and arcs (other
/// curves as point lists at the flat pattern's 0.001 mm tolerance).
fn loop_shape(lp: &solidcraft_geom::Loop2) -> Value {
    use solidcraft_geom::Seg2;
    let pt = |v: Vec2| json!([v.x, v.y]);
    if let Some((center, radius)) = loop_circle(lp) {
        return json!({"circle": {"center": pt(center), "radius": radius}});
    }
    let segments: Vec<Value> = lp
        .segs
        .iter()
        .map(|seg| match *seg {
            Seg2::Line { a, b } => json!({"line": [pt(a), pt(b)]}),
            Seg2::Arc { center, radius, start, sweep } => {
                json!({"arc": {"center": pt(center), "radius": radius, "start_deg": start.to_degrees(), "sweep_deg": sweep.to_degrees()}})
            }
            ref other => json!({"polyline": other.polyline(1e-3).into_iter().map(pt).collect::<Vec<_>>()}),
        })
        .collect();
    json!({"segments": segments})
}

fn flat_json(sh: &SheetBody) -> Result<Value> {
    let regions = sh.flat_regions()?;
    let loops: Vec<Ring<Vec2>> = regions.iter().map(|r| r.outer.polyline(1e-3)).collect();
    let size = loops.iter().fold(Aabb2::EMPTY, |b, l| b.union(&l.bounds())).size();
    let (w, h) = (size.x, size.y);
    let pt = |v: &Vec2| json!([v.x, v.y]);
    let outline: Vec<Value> = loops.iter().map(|l| Value::Array(l.iter().map(pt).collect())).collect();
    let holes: Vec<Value> = regions.iter().flat_map(|r| &r.holes).map(|h| Value::Array(h.polyline(1e-3).iter().map(pt).collect())).collect();
    let hole_shapes: Vec<Value> = regions.iter().flat_map(|r| &r.holes).map(loop_shape).collect();
    let bends: Vec<Value> = sh
        .bends()
        .iter()
        .map(|b| {
            json!({"bend": b.index, "angle_deg": b.angle_deg, "included_angle_deg": b.included_angle_deg(), "radius": b.radius, "direction": if b.up { "up" } else { "down" },
                "allowance": b.allowance, "length": b.length, "start": pt(&b.start), "end": pt(&b.end)})
        })
        .collect();
    let pockets: Vec<Value> = sh
        .machining()?
        .iter()
        .map(|p| {
            let side = if p.z0 == 0.0 {
                "bottom"
            } else if p.z1 == sh.t {
                "top"
            } else {
                "internal"
            };
            json!({"outline": p.region.outer.polyline(1e-3).iter().map(pt).collect::<Vec<_>>(),
            "cutouts": p.region.holes.iter().map(|h| h.polyline(1e-3).iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "cutout_shapes": p.region.holes.iter().map(loop_shape).collect::<Vec<_>>(),
            "side": side, "depth_mm": p.z1-p.z0, "z0_mm": p.z0, "z1_mm": p.z1})
        })
        .collect();
    // Holes drilled along a panel into its rim: drilled after cutting, not in the outline.
    let bores: Vec<Value> = sh
        .bores
        .iter()
        .map(|b| json!({"start": [b.base.x, b.base.y, b.base.z], "axis": [b.axis.x, b.axis.y, b.axis.z], "diameter": b.radius * 2.0, "length": b.height.abs()}))
        .collect();
    let mut perforations = Vec::new();
    for p in &sh.perforations {
        let n = sh.perforation_holes(p)?.len();
        perforations.push(json!({"feature": p.feature, "diameter": p.diameter, "pitch": p.pitch, "margin": p.margin,
            "pattern": if p.square { "square" } else { "staggered" }, "count": n,
            "open_area_mm2": n as f64 * std::f64::consts::PI * (p.diameter / 2.0).powi(2)}));
    }
    let bend_zones: Vec<Value> = sh
        .bend_zones()
        .iter()
        .map(|z| {
            let pts = z.outer.polyline(1e-3);
            let f = |g: fn(&Vec2) -> f64, m: fn(f64, f64) -> f64, s: f64| pts.iter().map(g).fold(s, m);
            json!([f(|p| p.x, f64::min, f64::MAX), f(|p| p.x, f64::max, f64::MIN), f(|p| p.y, f64::min, f64::MAX), f(|p| p.y, f64::max, f64::MIN)])
        })
        .collect();
    let mut out = json!({"body": sh.body, "rule": sh.rule, "thickness": sh.t, "k_factor": sh.k, "flat_size_mm": [w, h, sh.t], "outline": outline, "cutouts": holes, "cutout_shapes": hole_shapes, "pockets": pockets, "bends": bends, "unfolded": sh.flat,
        "perforations": perforations, "bend_zones": bend_zones});
    if !bores.is_empty() {
        out["bores"] = Value::Array(bores);
    }
    Ok(out)
}

fn flat_pattern(s: &mut Session, p: &Value) -> Result<Value> {
    let sh = sheet_of(s, p, "sheet.flat_pattern")?;
    sh.flat_matches_body()?;
    flat_json(&sh)
}

/// How the flat DXF writes the curves that are neither lines nor circular arcs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DxfCurves {
    /// Exactly: an ellipse as ELLIPSE, Béziers (spline pieces, text outlines) and the other conics as SPLINE.
    #[default]
    Exact,
    /// As tangent arcs and lines within 0.001 mm, for machines that read only those.
    Arcs,
}

/// Where the letters that text cut through the sheet go in the flat DXF.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DxfText {
    /// Cut contours on layer TEXT: the sheet has holes in the shape of the letters.
    #[default]
    Cut,
    /// Single contours on layer ETCH: the sheet keeps its material there.
    Etch,
}

/// What `sheet.export_dxf` is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DxfOptions {
    pub curves: DxfCurves,
    pub text: DxfText,
    /// The height (mm) of the note beside each bend line, when it is to have one.
    pub bend_notes: Option<f64>,
    /// Leave the bend lines out (BEND_UP, BEND_DOWN), for a cutter that would cut them (#4314).
    pub no_bend_lines: bool,
    /// Everything on layer 0 instead of a layer for each role (#4314).
    pub one_layer: bool,
}

/// The layer of a role: its AutoCAD colour index (1 red, 2 yellow, 3 green, 4 cyan, 5 blue,
/// 6 magenta, 7 white or black, 8 grey) and, for the bend lines, a dashed line. The pocket layers
/// and PERFORATION are green.
fn dxf_layer(name: &str) -> solidcraft_io::DxfLayer {
    use solidcraft_io::DxfLayer;
    match name {
        "OUTLINE" => DxfLayer::new(name, 7),
        "CUT" => DxfLayer::new(name, 4),
        "TEXT" => DxfLayer::new(name, 2),
        "ETCH" => DxfLayer::new(name, 6),
        "BEND_UP" => DxfLayer::new(name, 1).dashed(),
        "BEND_DOWN" => DxfLayer::new(name, 5).dashed(),
        "BEND_NOTES" => DxfLayer::new(name, 8),
        _ => DxfLayer::new(name, 3),
    }
}

/// A number in a bend note: at most three decimals, none when they are zeros (`90`, `2.5`).
fn compact(v: f64) -> String {
    let t = format!("{v:.3}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-0" { "0".into() } else { t.into() }
}

/// The note beside a bend line, `B1 UP 90%%d R2.5` (number, direction, angle as `display` reads
/// it, inner radius): as high as `height`, centred half a height beside the middle of the line,
/// parallel to it and readable from below or from the right. (`%%d` is AutoCAD's degree sign.)
fn bend_note(b: &solidcraft_doc::sheet::BendInfo, height: f64, display: sheet::AngleDisplay) -> solidcraft_io::DxfShape {
    let along = (b.end - b.start).normalized().unwrap_or(Vec2::X);
    let mut angle = along.angle().to_degrees();
    if angle > 90.0 + 1e-9 {
        angle -= 180.0;
    } else if angle <= -90.0 + 1e-9 {
        angle += 180.0;
    }
    let beside = Vec2::from_angle(angle.to_radians()).perp();
    solidcraft_io::DxfShape::Text {
        at: (b.start + b.end) * 0.5 + beside * (height * 0.5),
        height,
        angle_deg: angle,
        text: format!("B{} {} {}%%d R{}", b.index, if b.up { "UP" } else { "DOWN" }, compact(display.of(b.angle_deg)), compact(b.radius)),
    }
}

/// A DXF (AutoCAD 2000, mm) of the flat pattern on layers by role: OUTLINE (the outer contour of
/// each piece), CUT (what is cut out inside it, and the loose pieces of a cut), TEXT or ETCH
/// (letters), BEND_UP and BEND_DOWN (dashed centre lines), BEND_NOTES (when asked for), one layer
/// per pocket depth and PERFORATION. Inner contours are written before the outline, so a program
/// that follows the file's order cuts inside-out. Free-form curves as `o.curves` says.
pub fn flat_dxf(sh: &SheetBody, o: &DxfOptions) -> Result<String> {
    use solidcraft_io::{DxfEntity, DxfShape};
    let regions = sh.flat_regions()?;
    let pockets = sh.machining()?;
    let letters = sh.letters();
    let pocket_layer = |p: &solidcraft_doc::sheet::SheetPocket| {
        let side = if p.z0 == 0.0 {
            "BOTTOM"
        } else if p.z1 == sh.t {
            "TOP"
        } else {
            "INTERNAL"
        };
        format!("POCKET_{side}_Z{:.6}_TO_{:.6}_MM", p.z0, p.z1)
    };
    let perforation: Vec<(Vec2, f64)> = sh
        .perforations
        .iter()
        .map(|p| sh.perforation_holes(p).map(|h| h.into_iter().map(|c| (c, p.diameter / 2.0)).collect::<Vec<_>>()))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    // A piece inside another piece's hole (a letter's counter, the middle of a ring cut) is no
    // outline of the sheet but a loose piece the cut leaves.
    let nested: Vec<bool> = regions
        .iter()
        .enumerate()
        .map(|(j, r)| {
            let probe = r.outer.segs.first().map(solidcraft_geom::Seg2::mid);
            probe.is_some_and(|p| regions.iter().enumerate().any(|(i, other)| i != j && other.outer.contains(p)))
        })
        .collect();
    // The loops, in the order they are written: inner contours, markings, pockets, the outline.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Pass {
        Cut,
        Etch,
        Pocket,
        Outline,
    }
    let mut loops: Vec<(Pass, &solidcraft_geom::Loop2, String)> = Vec::new();
    for (j, r) in regions.iter().enumerate() {
        for (k, lp) in std::iter::once(&r.outer).chain(&r.holes).enumerate() {
            let (pass, layer) = if k == 0 && !nested.get(j).copied().unwrap_or(false) {
                (Pass::Outline, "OUTLINE")
            } else if letters.is_letter(lp) {
                match o.text {
                    DxfText::Cut => (Pass::Cut, "TEXT"),
                    DxfText::Etch => (Pass::Etch, "ETCH"),
                }
            } else {
                (Pass::Cut, "CUT")
            };
            loops.push((pass, lp, layer.to_string()));
        }
    }
    for p in &pockets {
        let layer = pocket_layer(p);
        for lp in std::iter::once(&p.region.outer).chain(&p.region.holes) {
            // Text machined to a depth is a marking, whatever its depth.
            if letters.is_letter(lp) {
                loops.push((Pass::Etch, lp, "ETCH".to_string()));
            } else {
                loops.push((Pass::Pocket, lp, layer.clone()));
            }
        }
    }
    loops.sort_by_key(|(pass, ..)| *pass);
    let mut entities = Vec::new();
    for (_, h, layer) in loops {
        // A circle is often represented by two semicircles. Keep the whole analytic
        // boundary as one CIRCLE rather than approximating outer boundaries with chords.
        if let Some((center, radius)) = loop_circle(h) {
            entities.push(DxfEntity { layer, shape: DxfShape::Circle { c: center, r: radius } });
            continue;
        }
        let fitted;
        let h = match o.curves {
            DxfCurves::Arcs if !h.is_line_arc() => {
                fitted = h
                    .fit_arcs(solidcraft_kernel::PLANAR_FIT_TOL)
                    .ok_or_else(|| EngineError::Other("a flat-pattern curve could not be written as arcs".into()))?;
                &fitted
            }
            _ => h,
        };
        // A whole ellipse is one ELLIPSE; every other segment is an entity of its own.
        entities.extend(solidcraft_io::loop_shapes(h).into_iter().map(|shape| DxfEntity { layer: layer.clone(), shape }));
    }
    for b in sh.bends() {
        if !o.no_bend_lines {
            entities.push(DxfEntity { layer: if b.up { "BEND_UP" } else { "BEND_DOWN" }.into(), shape: DxfShape::Line(b.start, b.end) });
        }
        if let Some(height) = o.bend_notes {
            entities.push(DxfEntity { layer: "BEND_NOTES".into(), shape: bend_note(&b, height, sh.angle_display) });
        }
    }
    for (c, r) in perforation {
        entities.push(DxfEntity { layer: "PERFORATION".into(), shape: DxfShape::Circle { c, r } });
    }
    // One layer: every entity on layer 0, which every drawing has.
    if o.one_layer {
        for e in &mut entities {
            e.layer = "0".into();
        }
        return Ok(solidcraft_io::write_dxf_drawing(&[], &entities)?);
    }
    // The four layers of every flat pattern, and those the rest is on.
    let mut names: std::collections::BTreeSet<String> = ["OUTLINE", "CUT", "BEND_UP", "BEND_DOWN"].map(String::from).into();
    names.extend(entities.iter().map(|e| e.layer.clone()));
    let layers: Vec<_> = names.iter().map(|name| dxf_layer(name)).collect();
    Ok(solidcraft_io::write_dxf_drawing(&layers, &entities)?)
}

/// What a `sheet.export_dxf` call asks for; a value it cannot use is refused.
fn dxf_options(p: &Value, cmd: &str) -> Result<DxfOptions> {
    let curves = match str_(p, "curves").map(str::to_ascii_lowercase).as_deref() {
        None | Some("exact") => DxfCurves::Exact,
        Some("arcs") => DxfCurves::Arcs,
        Some(_) => return Err(bad(cmd, "`curves` must be exact or arcs")),
    };
    let text = match p.get("text").filter(|v| !v.is_null()) {
        None => DxfText::Cut,
        Some(v) => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("cut") => DxfText::Cut,
            Some("etch") => DxfText::Etch,
            _ => return Err(bad(cmd, "`text` must be cut or etch")),
        },
    };
    let height = match p.get("note_height").filter(|v| !v.is_null()) {
        None => 3.0,
        Some(v) => {
            v.as_f64().filter(|h| (0.1..=100.0).contains(h)).ok_or_else(|| bad(cmd, "`note_height` must be a number of mm from 0.1 to 100"))?
        }
    };
    let bend_notes = match p.get("bend_notes").filter(|v| !v.is_null()) {
        None | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(height),
        Some(_) => return Err(bad(cmd, "`bend_notes` must be true or false")),
    };
    let no_bend_lines = match p.get("bend_lines").filter(|v| !v.is_null()) {
        None | Some(Value::Bool(true)) => false,
        Some(Value::Bool(false)) => true,
        Some(_) => return Err(bad(cmd, "`bend_lines` must be true or false")),
    };
    let one_layer = match p.get("layers").filter(|v| !v.is_null()) {
        None => false,
        Some(v) => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("roles") => false,
            Some("single") => true,
            _ => return Err(bad(cmd, "`layers` must be roles or single")),
        },
    };
    Ok(DxfOptions { curves, text, bend_notes, no_bend_lines, one_layer })
}

/// The files of an export of every sheet body: `path` with `-<body>` before its extension
/// (`.dxf` when it has none). Characters a file name cannot hold become `_`, and a name another
/// body's file already has (in any case) gets `_2`, `_3`…
fn body_paths(path: &str, bodies: &[String]) -> Vec<String> {
    let p = std::path::Path::new(path);
    let stem = p.file_stem().map_or_else(|| "flat".to_string(), |s| s.to_string_lossy().to_string());
    let ext = p.extension().map_or_else(|| "dxf".to_string(), |e| e.to_string_lossy().to_string());
    let mut taken: Vec<String> = Vec::new();
    bodies
        .iter()
        .map(|body| {
            let clean: String = body.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
            let clean = clean.trim().trim_end_matches(['.', ' ']);
            let base = if clean.is_empty() { "body" } else { clean };
            let mut name = base.to_string();
            let mut n = 2;
            while taken.contains(&name.to_lowercase()) {
                name = format!("{base}_{n}");
                n += 1;
            }
            taken.push(name.to_lowercase());
            p.with_file_name(format!("{stem}-{name}.{ext}")).to_string_lossy().to_string()
        })
        .collect()
}

fn export_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.export_dxf";
    let options = dxf_options(p, cmd)?;
    let per_body = match p.get("per_body").filter(|v| !v.is_null()) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`per_body` must be true or false")),
    };
    let path = str_(p, "path").filter(|x| !x.trim().is_empty());
    if per_body {
        if p.get("body").is_some_and(|b| !b.is_null()) {
            return Err(bad(cmd, "`per_body` writes every sheet body: leave `body` out"));
        }
        let sheets = &s.model.state().sheets;
        if sheets.is_empty() {
            return Err(no_sheet_body(cmd));
        }
        // Every file is made before the first is written.
        let texts = sheets
            .iter()
            .map(|sh| {
                sh.flat_matches_body()?;
                flat_dxf(sh, &options)
            })
            .collect::<Result<Vec<_>>>()?;
        let paths = path.map(|path| body_paths(path, &sheets.iter().map(|sh| sh.body.clone()).collect::<Vec<_>>()));
        let mut files = Vec::new();
        for (i, (sh, text)) in sheets.iter().zip(&texts).enumerate() {
            let bends = sh.bends().len();
            if let Some(file) = paths.as_ref().and_then(|paths| paths.get(i)) {
                solidcraft_io::vfs::write(file, text.as_bytes()).map_err(|e| EngineError::Other(format!("{file}: {e}")))?;
                files.push(json!({"body": sh.body, "path": file, "bends": bends}));
            } else {
                files.push(json!({"body": sh.body, "dxf": text, "bends": bends}));
            }
        }
        return Ok(json!({"files": files}));
    }
    let sh = sheet_of(s, p, cmd)?;
    sh.flat_matches_body()?;
    let text = flat_dxf(&sh, &options)?;
    if let Some(path) = path {
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        return Ok(json!({"path": path, "bends": sh.bends().len()}));
    }
    Ok(json!({"dxf": text, "bends": sh.bends().len()}))
}

fn export_bend_table(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.export_bend_table";
    let format = str_(p, "format").unwrap_or("csv").to_ascii_lowercase();
    if !matches!(format.as_str(), "csv" | "json") {
        return Err(bad(cmd, "`format` must be csv or json"));
    }
    let sh = sheet_of(s, p, cmd)?;
    sh.flat_matches_body()?;
    let table = flat_json(&sh)?;
    let text = if format == "json" {
        serde_json::to_string_pretty(&table).map_err(|e| EngineError::Other(e.to_string()))?
    } else {
        // Always quote text fields: body/rule names may include commas, quotes or newlines.
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut csv =
            "body,rule,thickness_mm,k_factor,bend,angle_deg,direction,radius_mm,allowance_mm,length_mm,start_x_mm,start_y_mm,end_x_mm,end_y_mm,included_angle_deg\r\n"
                .to_string();
        for b in sh.bends() {
            csv.push_str(&format!(
                "{},{},{:.9},{:.9},{},{:.9},{},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9}\r\n",
                quote(&sh.body),
                quote(&sh.rule),
                sh.t,
                sh.k,
                b.index,
                b.angle_deg,
                if b.up { "up" } else { "down" },
                b.radius,
                b.allowance,
                b.length,
                b.start.x,
                b.start.y,
                b.end.x,
                b.end.y,
                b.included_angle_deg()
            ));
        }
        csv
    };
    if let Some(path) = str_(p, "path").filter(|x| !x.trim().is_empty()) {
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        return Ok(json!({"path": path, "format": format, "bends": sh.bends().len()}));
    }
    Ok(json!({"format": format, "text": text, "bends": sh.bends().len()}))
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sheet_batch_cuts_tests.rs"]
mod batch_cuts_tests;

#[cfg(test)]
#[path = "sheet_convert_tests.rs"]
mod convert_tests;

#[cfg(test)]
#[path = "sheet_dxf_tests.rs"]
mod dxf_tests;

#[cfg(test)]
#[path = "sheet_face_names_tests.rs"]
mod face_names_tests;
