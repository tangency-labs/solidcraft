//! Parametric reconstruction of imported mechanical mesh bodies, and reading a mesh as the
//! surfaces it stands for.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::expr::Kind;
use solidcraft_kernel::{Outline, Prismatic, Surface, prismatic_plan, recognize_mesh};

use super::{
    CommandSpec,
    features::{add_feature, check_expr},
};
use crate::schema::mesh::{
    MeshAxis, MeshBore, MeshBoss, MeshCorner, MeshParameter, MeshPatch, MeshRebuildParams, MeshRebuildResult, MeshRecognizeParams,
    MeshRecognizeResult, MeshStep, MeshUnused,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{
    Result, Sel, Session,
    params::{bad, bool_, expr, num, str_, vec3},
};

const RECOGNIZE_CONTRACT: TypedContract<MeshRecognizeParams> = TypedContract::new();
const REBUILD_CONTRACT: TypedContract<MeshRebuildParams> = TypedContract::new();

/// How far a surface's axis may lean from the build axis, in degrees, when a call does not say.
const DEFAULT_ALIGN: f64 = 2.;
/// The widest lean a call may ask for, and the narrowest: the kernel's own limits in degrees.
const ALIGN_RANGE: std::ops::RangeInclusive<f64> = 0.0573..=17.19;

/// Patches reported when a call does not say.
const DEFAULT_PATCHES: usize = 256;
/// Patches reported at most, whatever a call asks for.
const MAX_PATCHES: usize = 4096;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("mesh.convert", "Convert Mesh", convert)
        .icon("import")
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.05 mm): reconstruct a closed constant-section mechanical mesh as editable planar and cylindrical faces, with a parametric timeline feature in the mesh body's owning component (the active component must be that component or an ancestor; otherwise activate the named component first); the active component is unchanged; name?"),
    CommandSpec::new("mesh.recognize", "Recognize Mesh", crate::checked!("mesh.recognize", recognize))
        .icon("inspect")
        .noundo()
        .contract(&RECOGNIZE_CONTRACT)
        .output(output_schema_for::<MeshRecognizeResult>)
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.05 mm, between 0.000001 and 1 mm: how far the mesh may stray from a surface and still be certified as it, measured inside each triangle and not only at its corners, so a coarse mesh needs a larger tolerance to come back round rather than flat); groups?: [whole numbers], one per triangle in the body's own order (a proposal about which triangles make one face, as a neural segmenter gives: it decides only what is tried together, every surface is still fitted and certified, and a group that will not certify is split further); max_patches?: 1…4096 (default 256): read the mesh as the surfaces it stands for, changing nothing → body, triangles, vertices, closed, area, tolerance, found, freeform (the share of the area nothing analytic fit), patches: [{surface: plane|cylinder|cone|sphere|torus|freeform, triangles, area, deviation, origin, axis, radius, minor, half_angle (degrees), along}], axes: [{direction, along (walls), across (end caps)}]"),
    CommandSpec::new("mesh.rebuild", "Rebuild Mesh as Features", crate::checked!("mesh.rebuild", rebuild))
        .icon("import")
        .contract(&REBUILD_CONTRACT)
        .output(output_schema_for::<MeshRebuildResult>)
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.05 mm), groups?: as mesh.recognize takes them; axis?: [x,y,z] (the direction to extrude along; default: the direction with the most recognized area square to it); align?: degrees (default 2, between 0.0573 and 17.19: how far a surface's own axis may lean from the build axis and still count as along it or square to it, which on a scan has to cover the part's own skew); parameters?: bool (default true: name each size as a user parameter the features follow); prefix?: those parameters' name before `_thickness` and the rest (default: from the body name); plan_only?: bool (default false: report the plan and the commands and change nothing); name?: a name for the extrusion: build a prismatic part again from what its mesh was recognized as -- a sketch on one end face, an extrusion to the other, the corner rounds, and the holes and bosses its cylinders stand for -- as ordinary parametric features, leaving the mesh body alone. An outline that turns back on itself, such as an L, is refused rather than rebuilt as something smaller; what the rebuild does not account for comes back in `unused` to model by hand → body, axis, origin, x_dir, y_dir, thickness, outline: circle|polygon, radius, corners: [{at, radius}], bores: [{centre, diameter, through, depth, from_top}], bosses: [{centre, diameter, height}], accounted (the share of the mesh's area the features cover), unused: [{surface, area, reason}], parameters: [{name, expression}], steps: [{command, params}], features"),
];

/// The mesh body to convert: `body`, else the one selected body.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<String> {
    if let Some(name) = str_(p, "body").filter(|b| !b.trim().is_empty()) {
        return Ok(name.to_owned());
    }
    let selected: Vec<&str> = s.selection.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.as_str()) } else { None }).collect();
    match selected.as_slice() {
        [name] => Ok((*name).to_owned()),
        [] => Err(bad(cmd, "select a mesh body")),
        _ => Err(bad(cmd, "select one mesh body")),
    }
}

/// Entries a proposed grouping may have: one per triangle, and the kernel takes no larger mesh.
const MAX_GROUPS: usize = 2_000_000;

/// `mesh.recognize`: read the mesh as the surfaces it stands for. Nothing is changed, so there is
/// no feature and no undo step; a design is rebuilt from the reply with the ordinary commands.
fn recognize(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.recognize";
    let reported = match num(p, "max_patches") {
        None => DEFAULT_PATCHES,
        Some(n) if (1. ..=MAX_PATCHES as f64).contains(&n) && n.fract() == 0. => n as usize,
        Some(_) => return Err(bad(cmd, format!("max_patches: a whole number between 1 and {MAX_PATCHES}"))),
    };
    let (name, found) = recognized(s, p, cmd)?;
    let out = MeshRecognizeResult {
        body: name,
        triangles: found.triangles,
        vertices: found.vertices,
        closed: found.closed,
        area: found.area,
        tolerance: found.tolerance,
        found: found.patches.len(),
        freeform: found.freeform_share(),
        patches: found.patches.iter().take(reported).map(patch_reply).collect(),
        axes: found
            .axes
            .iter()
            .map(|a| MeshAxis { direction: [a.direction.x, a.direction.y, a.direction.z], along: a.along, across: a.across })
            .collect(),
    };
    serde_json::to_value(out).map_err(|err| crate::EngineError::Other(err.to_string()))
}

/// The proposed grouping: one whole number per triangle, or nothing.
fn groups_of(p: &Value, cmd: &str) -> Result<Option<Vec<u32>>> {
    let Some(value) = p.get("groups").filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(list) = value.as_array() else {
        return Err(bad(cmd, "groups: a list of whole numbers, one per triangle"));
    };
    if list.len() > MAX_GROUPS {
        return Err(bad(cmd, format!("groups: {} entries, more than the {MAX_GROUPS} this reads", list.len())));
    }
    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        let group =
            entry.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(cmd, "groups: a list of whole numbers, none of them negative"))?;
        out.push(group);
    }
    Ok(Some(out))
}

fn patch_reply(patch: &solidcraft_kernel::Patch) -> MeshPatch {
    let at = |v: solidcore_record::Vec3| [v.x, v.y, v.z];
    let mut out = MeshPatch {
        surface: patch.surface.name().to_owned(),
        triangles: patch.triangles,
        area: patch.area,
        deviation: patch.deviation,
        origin: None,
        axis: None,
        radius: None,
        minor: None,
        half_angle: None,
        along: patch.along,
        outward: patch.outward,
    };
    match patch.surface {
        Surface::Plane { origin, normal } => (out.origin, out.axis) = (Some(at(origin)), Some(at(normal))),
        Surface::Cylinder { origin, axis, radius } => {
            (out.origin, out.axis, out.radius) = (Some(at(origin)), Some(at(axis)), Some(radius));
        }
        Surface::Cone { apex, axis, half_angle } => {
            (out.origin, out.axis, out.half_angle) = (Some(at(apex)), Some(at(axis)), Some(half_angle.to_degrees()));
        }
        Surface::Sphere { centre, radius } => (out.origin, out.radius) = (Some(at(centre)), Some(radius)),
        Surface::Torus { centre, axis, major, minor } => {
            (out.origin, out.axis, out.radius, out.minor) = (Some(at(centre)), Some(at(axis)), Some(major), Some(minor));
        }
        Surface::Freeform => {}
    }
    out
}

/// The named mesh body read as the surfaces it stands for: what both `mesh.recognize` and
/// `mesh.rebuild` start from, so that a rebuild takes a tolerance and a proposed grouping exactly
/// as recognition does.
fn recognized(s: &mut Session, p: &Value, cmd: &str) -> Result<(String, solidcraft_kernel::Recognition)> {
    let text = expr(p, "tolerance").unwrap_or_else(|| "0.05 mm".into());
    check_expr(s, &text, Kind::Length, cmd, "tolerance")?;
    let tolerance = s.doc.eval(&text, Kind::Length).map_err(|err| bad(cmd, format!("tolerance: {err}")))?;
    let groups = groups_of(p, cmd)?;
    let state = s.model.state();
    let name = target(s, p, cmd)?;
    let source = state.body(&name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
    if !source.body.is_mesh() {
        return Err(bad(cmd, "select a mesh body"));
    }
    let found = recognize_mesh(&source.body, tolerance, groups.as_deref())?;
    Ok((name, found))
}

/// `mesh.rebuild`: build a prismatic part again as ordinary features. The recognized surfaces say
/// what the part is; the ordinary commands build it, so what comes out is a timeline to edit and
/// not another frozen body. The mesh body itself is left alone.
fn rebuild(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.rebuild";
    let (body, found) = recognized(s, p, cmd)?;
    let axis = match p.get("axis").filter(|v| !v.is_null()) {
        None => None,
        Some(given) => Some(vec3(given).ok_or_else(|| bad(cmd, "axis: [x, y, z]"))?),
    };
    let align = match num(p, "align") {
        None => DEFAULT_ALIGN,
        Some(degrees) if ALIGN_RANGE.contains(&degrees) => degrees,
        Some(_) => return Err(bad(cmd, format!("align: degrees between {} and {}", ALIGN_RANGE.start(), ALIGN_RANGE.end()))),
    };
    let plan = prismatic_plan(&found, axis, align.to_radians())?;
    let mut names = Naming {
        on: bool_(p, "parameters").unwrap_or(true),
        prefix: identifier(str_(p, "prefix").unwrap_or(&body)),
        body: body.clone(),
        made: Vec::new(),
    };
    let steps = plan_steps(s, p, &plan, &mut names);
    let mut features = Vec::new();
    if !bool_(p, "plan_only").unwrap_or(false) {
        // Every step is a command of its own and records an undo step of its own. The rebuild is
        // one thing the user asked for, so those are folded back into the single step this
        // command's own call records. A step that fails leaves the design as it was, as any
        // failed command does.
        let start = s.undo.len();
        let mut solid: Option<String> = None;
        for (at, step) in steps.iter().enumerate() {
            let mut params = step.params.clone();
            // A body name already taken is given another, so the steps after the extrusion follow
            // the body it made rather than the name they asked for.
            if let (Some(solid), Some(fields)) = (&solid, params.as_object_mut())
                && fields.contains_key("targets")
            {
                fields.insert("targets".into(), json!([solid]));
            }
            let out = s.execute(&step.command, &params).map_err(|err| bad(cmd, format!("step {at} of the rebuild, `{}`: {err}", step.command)))?;
            if let Some(made) = out.get("created_bodies").and_then(Value::as_array).and_then(|b| b.first()).and_then(Value::as_str) {
                solid = Some(made.to_owned());
            }
            if out.get("feature").is_some()
                && let Some(name) = out.get("name").and_then(Value::as_str)
            {
                features.push(name.to_owned());
            }
        }
        if s.undo.len() >= start {
            s.undo.truncate(start);
        }
    }
    let at2 = |v: [f64; 2]| [round(v[0]), round(v[1])];
    let out = MeshRebuildResult {
        body,
        axis: at3(plan.axis),
        origin: at3(plan.origin),
        x_dir: at3(plan.x_dir),
        y_dir: at3(plan.y_dir),
        thickness: round(plan.thickness),
        outline: match plan.outline {
            Outline::Circle { .. } => "circle".into(),
            Outline::Polygon(_) => "polygon".into(),
        },
        radius: match plan.outline {
            Outline::Circle { radius, .. } => Some(round(radius)),
            Outline::Polygon(_) => None,
        },
        corners: match &plan.outline {
            Outline::Circle { .. } => Vec::new(),
            Outline::Polygon(corners) => corners.iter().map(|c| MeshCorner { at: at2(c.at), radius: round(c.radius) }).collect(),
        },
        bores: plan
            .bores
            .iter()
            .map(|b| MeshBore {
                centre: at2(b.centre),
                diameter: round(b.radius * 2.),
                through: b.through,
                depth: round(b.depth),
                from_top: b.from_top,
                at: round(b.at),
            })
            .collect(),
        bosses: plan.bosses.iter().map(|b| MeshBoss { centre: at2(b.centre), diameter: round(b.radius * 2.), height: round(b.height) }).collect(),
        accounted: plan.accounted,
        unused: plan.unused.iter().map(|u| MeshUnused { surface: u.surface.into(), area: u.area, reason: u.reason.into() }).collect(),
        parameters: names.made,
        steps,
        features,
    };
    serde_json::to_value(out).map_err(|err| crate::EngineError::Other(err.to_string()))
}

/// The commands that build the plan, in order: the parameters first, so everything after them can
/// follow them.
fn plan_steps(s: &Session, p: &Value, plan: &Prismatic, names: &mut Naming) -> Vec<MeshStep> {
    let step = |command: &str, params: Value| MeshStep { command: command.to_owned(), params };
    let thickness = names.size(s, "thickness", plan.thickness);
    let mut build = vec![step("sketch.create", json!({"plane": {"origin": at3(plan.origin), "x_dir": at3(plan.x_dir), "y_dir": at3(plan.y_dir)}}))];
    match &plan.outline {
        Outline::Circle { centre, radius } => {
            let diameter = names.size(s, "diameter", radius * 2.);
            build.push(step("sketch.circle.center", json!({"center": [fixed(centre[0]), fixed(centre[1])], "diameter": diameter})));
        }
        Outline::Polygon(corners) => {
            // Each coordinate goes in as an expression, which fixes the point where it lands: the
            // outline came off the mesh and is not for the solver to move, and a corner radius
            // changed afterwards then rounds the corner instead of pushing the walls outwards.
            let points: Vec<[String; 2]> = corners.iter().map(|c| [fixed(c.at[0]), fixed(c.at[1])]).collect();
            build.push(step("sketch.line", json!({"points": points, "closed": true})));
            // A closed run of points makes l1 from the first to the second and so on, so the
            // corner at point k is the end of line k, and the first point is the end of the last.
            let mut alike: Vec<(f64, Vec<String>)> = Vec::new();
            for (k, corner) in corners.iter().enumerate() {
                if !(corner.radius > 0.) {
                    continue;
                }
                let at = format!("l{}.end", if k == 0 { corners.len() } else { k });
                // Corners whose radii agree to within a micron are one round, and are rounded by
                // one call: a fitted radius carries the mesh's own last digits.
                match alike.iter_mut().find(|(radius, _)| (*radius - corner.radius).abs() <= 1e-3) {
                    Some((_, same)) => same.push(at),
                    None => alike.push((corner.radius, vec![at])),
                }
            }
            // A corner radius stays a number and not a parameter. The outline is a run of lines
            // the mesh gave and carries no dimensions to hold it, so a radius changed afterwards
            // would have the solver push the walls out instead of rounding the corner; the
            // thickness, the bores and the bosses never touch the sketch, so those are safe.
            for (radius, at) in &alike {
                build.push(step("sketch.fillet", json!({"points": at, "radius": mm(*radius)})));
            }
        }
    }
    build.push(step("sketch.finish", json!({})));
    // The new body is named here because the mesh it was read from stands in the same space: a
    // hole drilled without saying whose face it is for would go through the mesh body as well.
    let solid = format!("{} rebuilt", names.body);
    let mut extrude = json!({"distance": thickness, "operation": "new", "body_name": solid});
    if let (Some(name), Some(fields)) = (str_(p, "name"), extrude.as_object_mut()) {
        fields.insert("name".into(), name.into());
    }
    build.push(step("solid.extrude", extrude));
    // Where a point sits along the axis is an expression over the thickness, not the number it
    // came out at: a boss stands on the far end face and a hole is drilled into it, so both have
    // to move when the part is made thicker.
    let on_axis = |u: f64, v: f64, height: &str| {
        let flat = plan.origin + plan.x_dir * u + plan.y_dir * v;
        let each = |base: f64, axis: f64| match axis.abs() < 1e-9 {
            true => mm(base),
            false => format!("{} + ({:.6}) * ({height})", mm(base), axis),
        };
        [each(flat.x, plan.axis.x), each(flat.y, plan.axis.y), each(flat.z, plan.axis.z)]
    };
    // The bosses are built before the bores: a hole under a boss is drilled through both, and
    // drilling first would only have the boss fill it in again.
    let mut standing: Vec<([f64; 2], f64, String)> = Vec::new();
    for (which, boss) in plan.bosses.iter().enumerate() {
        let diameter = names.size(s, &format!("boss_{}_dia", which + 1), boss.radius * 2.);
        let height = names.size(s, &format!("boss_{}_height", which + 1), boss.height);
        let base = on_axis(boss.centre[0], boss.centre[1], &thickness);
        build.push(step(
            "solid.cylinder",
            json!({"base": base, "axis": at3(plan.axis), "diameter": diameter, "height": &height, "operation": "join", "targets": [&solid]}),
        ));
        standing.push((boss.centre, boss.radius, height));
    }
    for (which, bore) in plan.bores.iter().enumerate() {
        let diameter = names.size(s, &format!("bore_{}_dia", which + 1), bore.radius * 2.);
        // The face it is drilled into: the far end, raised by the boss standing over it.
        let over =
            standing.iter().find(|(centre, radius, _)| (centre[0] - bore.centre[0]).hypot(centre[1] - bore.centre[1]) + bore.radius <= radius + 1e-3);
        let height = match (bore.from_top, over) {
            (false, _) => "0".to_owned(),
            (true, None) => thickness.clone(),
            (true, Some((_, _, boss))) => format!("{thickness} + {boss}"),
        };
        let at = on_axis(bore.centre[0], bore.centre[1], &height);
        let mut params = json!({"position": at, "diameter": diameter, "targets": [&solid]});
        if !bore.through
            && let Some(fields) = params.as_object_mut()
        {
            fields.insert("depth".into(), names.size(s, &format!("bore_{}_depth", which + 1), bore.depth).into());
        }
        build.push(step("solid.hole", params));
    }
    let mut steps: Vec<MeshStep> =
        names.made.iter().map(|made| step("parameters.add", json!({"name": made.name, "expression": made.expression}))).collect();
    steps.extend(build);
    steps
}

/// Names for the sizes the rebuild found, so that each feature follows a parameter instead of a
/// number and the part can be changed afterwards.
struct Naming {
    on: bool,
    prefix: String,
    /// The mesh body being rebuilt, which the new solid is named after.
    body: String,
    made: Vec<MeshParameter>,
}

impl Naming {
    /// What a feature should use for one size: a new parameter's name, or the length itself where
    /// the caller asked for no parameters, or the names are all taken.
    fn size(&mut self, s: &Session, what: &str, value: f64) -> String {
        if !self.on {
            return mm(value);
        }
        let base = format!("{}_{what}", self.prefix);
        let mut name = base.clone();
        for next in 2..100 {
            if s.doc.param(&name).is_none() && !self.made.iter().any(|made| made.name == name) {
                self.made.push(MeshParameter { name: name.clone(), expression: mm(value) });
                return name;
            }
            name = format!("{base}_{next}");
        }
        mm(value)
    }
}

/// A length as an expression a command takes, to a tenth of a micrometre.
fn mm(value: f64) -> String {
    let text = format!("{value:.4}");
    format!("{} mm", text.trim_end_matches('0').trim_end_matches('.'))
}

/// A name a parameter may have: lower case letters, digits and underscores, starting with a
/// letter, from whatever the caller or the body was called.
fn identifier(text: &str) -> String {
    let mut out = String::new();
    for letter in text.chars().take(64) {
        if letter.is_ascii_alphanumeric() {
            out.push(letter.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
        if out.len() >= 24 {
            break;
        }
    }
    let out = out.trim_matches('_');
    match out.chars().next() {
        Some(first) if first.is_ascii_alphabetic() => out.to_owned(),
        _ => format!("part_{out}").trim_end_matches('_').to_owned(),
    }
}

/// A sketch coordinate as an expression, which is what fixes the point it places.
fn fixed(value: f64) -> String {
    format!("{:.4}", round(value))
}

fn round(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}

fn at3(v: Vec3) -> [f64; 3] {
    [round(v.x), round(v.y), round(v.z)]
}

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.convert";
    let state = s.model.state();
    let body = target(s, p, cmd)?;
    let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    if !source.body.is_mesh() {
        return Err(bad(cmd, "select a mesh body"));
    }
    let owner = s.doc.model_body_component(source);
    if !s.doc.component_within(owner, s.active_component) {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first")));
    }
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| "0.05 mm".into());
    check_expr(s, &tolerance, Kind::Length, cmd, "tolerance")?;
    // Conversion edits the component definition, independent of its occurrence placement.
    let active_component = std::mem::replace(&mut s.active_component, owner);
    let active_occurrence_path = s.active_occurrence_path.take();
    let result = add_feature(s, p, FeatureKind::MeshConvert { body: body.into(), tolerance });
    s.active_component = active_component;
    s.active_occurrence_path = active_occurrence_path;
    result
}
