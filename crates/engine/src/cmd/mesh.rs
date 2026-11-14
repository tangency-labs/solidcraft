//! Parametric reconstruction of imported mechanical mesh bodies, and reading a mesh as the
//! surfaces it stands for.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::expr::Kind;
use solidcraft_kernel::{
    MAX_POINTS, MAX_SECTIONS, MIN_POINTS, Outline, Part as SlabPart, Prismatic, SilhouetteOptions, Silhouetted, SlabOptions, SlabStack, Surface,
    Symmetry, prismatic_plan, recognize_mesh, silhouettes, slab_plan,
};

use super::{
    CommandSpec,
    features::{add_feature, check_expr},
};
use crate::schema::mesh::{
    MeshAxis, MeshBore, MeshBoss, MeshCorner, MeshDeviation, MeshLoftAxis, MeshLoftParams, MeshLoftResult, MeshLoop, MeshMirror, MeshParameter,
    MeshPatch, MeshRebuildParams, MeshRebuildResult, MeshRecognizeParams, MeshRecognizeResult, MeshReconstructParams, MeshReconstructResult,
    MeshSlab, MeshStep, MeshUnused,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{
    Result, Sel, Session,
    params::{bad, bool_, expr, num, str_, vec3},
};

const RECOGNIZE_CONTRACT: TypedContract<MeshRecognizeParams> = TypedContract::new();
const REBUILD_CONTRACT: TypedContract<MeshRebuildParams> = TypedContract::new();
const RECONSTRUCT_CONTRACT: TypedContract<MeshReconstructParams> = TypedContract::new();
const LOFT_CONTRACT: TypedContract<MeshLoftParams> = TypedContract::new();

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
        .icon("mesh_body")
        .contract(&REBUILD_CONTRACT)
        .output(output_schema_for::<MeshRebuildResult>)
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.05 mm), groups?: as mesh.recognize takes them; axis?: [x,y,z] (the direction to extrude along; default: the direction with the most recognized area square to it); align?: degrees (default 2, between 0.0573 and 17.19: how far a surface's own axis may lean from the build axis and still count as along it or square to it, which on a scan has to cover the part's own skew); parameters?: bool (default true: name each size as a user parameter the features follow); prefix?: those parameters' name before `_thickness` and the rest (default: from the body name); plan_only?: bool (default false: report the plan and the commands and change nothing); name?: a name for the extrusion: build a prismatic part again from what its mesh was recognized as -- a sketch on one end face, an extrusion to the other, the corner rounds, and the holes and bosses its cylinders stand for -- as ordinary parametric features, leaving the mesh body alone. An outline that turns back on itself, such as an L, is refused rather than rebuilt as something smaller; what the rebuild does not account for comes back in `unused` to model by hand → body, axis, origin, x_dir, y_dir, thickness, outline: circle|polygon, radius, corners: [{at, radius}], bores: [{centre, diameter, through, depth, from_top}], bosses: [{centre, diameter, height}], accounted (the share of the mesh's area the features cover), unused: [{surface, area, reason}], parameters: [{name, expression}], steps: [{command, params}], features"),
    CommandSpec::new("mesh.reconstruct", "Reconstruct Scanned Mesh", crate::checked!("mesh.reconstruct", reconstruct))
        .icon("mesh_body")
        .contract(&RECONSTRUCT_CONTRACT)
        .output(output_schema_for::<MeshReconstructResult>)
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.25 mm, between 0.001 and 10 mm: the scan's own noise, and so how close a line or arc must come to count as fitting); grid?: length expression (default 0.1 mm, at most 10: round finished dimensions to this step; 0 leaves them as measured); symmetry?: auto|x|y|off (default auto); snap_angle?: degrees (default 15: square up an edge already within three degrees of a multiple); min_thickness?: length expression (default 0.6 mm: the thinnest layer kept); axis?: [x,y,z] (the direction to stack along; default: the direction the most facet area lies square to); parameters?: bool (default true: name each layer thickness and circle diameter as a user parameter); prefix?: those parameters' name before `_layer_1_thickness` (default: from the body name); plan_only?: bool (default false); name?: a name for the first extrusion: read a scanned or noisy mesh as the stack of extruded layers it was modelled from and build it again as ordinary parametric features -- a sketch per level whose outline is fitted with lines, arcs and circles, squared up, with matching dimensions equalised, mirrored features averaged and sizes rounded to a grid. Unlike mesh.rebuild it never needs a surface to certify, so it reads scans; it always has an answer, and `deviation` is how far that answer sits from the scan -> body, origin, axis, x_dir, y_dir, levels, size, slabs: [{from, to, loops: [{lines, arcs, circle, area, deviation, fitted}]}], mirrors: [{across, at, mismatch, paired}], triangles, area, deviation: {mean, p95, max, probes}, traced (outlines kept as polygons because nothing fitted), parameters, steps, features"),
    CommandSpec::new("mesh.loft", "Loft Mesh Silhouettes", crate::checked!("mesh.loft", loft))
        .icon("loft")
        .contract(&LOFT_CONTRACT)
        .output(output_schema_for::<MeshLoftResult>)
        .params("body?: mesh body name (default: the selected mesh body); axes?: [x|y|z|[x,y,z]] (default [\"z\"]: each direction is read and lofted on its own, then they are intersected, because one direction loses every concavity that faces along it); sections?: 2 to 50 cuts per direction (default 16); spacing?: length expression between cuts instead; points?: 12 to 500 per profile (default 60); margin?: length expression (default 1.5 mm: how far in from each tip to start cutting, never trimmed so far that nothing is left); hull?: bool (default false: take the convex hull at every cut whether or not the outline closes -- rougher, but impossible to trip up on a badly broken scan); plan_only?: bool (default false); name?: a name for the first loft: read a mesh as the silhouettes it casts along one or more directions and loft a solid through them. This is the path for a shape that is not a stack of layers -- a scan, a casting, a generated shape -- where mesh.reconstruct would force it into slabs and mesh.rebuild would refuse it. What comes out is not a dimensioned part: it is a solid with the shape's own silhouette, which booleans, measures, fillets and prints where the mesh could not. A cut that will not close into one outline falls back to the convex hull at that height and says so in `hulled`, because a hull loses every concavity there -> body, along: [{axis, x_dir, y_dir, heights, areas, hulled, reach, across}], triangles, area, points, bodies, steps, features"),
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

/// How a scan is read when a call does not say: a quarter of a millimetre of noise, dimensions
/// rounded to a tenth, edges squared to 15 degrees, and no layer thinner than six tenths.
const DEFAULT_TOLERANCE: &str = "0.25 mm";
const DEFAULT_GRID: &str = "0.1 mm";
const DEFAULT_SNAP: f64 = 15.;
const DEFAULT_MIN_THICKNESS: &str = "0.6 mm";

/// How a scan should be read, from what the call asked for.
fn reconstruct_options(s: &mut Session, p: &Value, cmd: &str) -> Result<SlabOptions> {
    let length = |s: &mut Session, field: &str, default: &str| -> Result<f64> {
        let text = expr(p, field).unwrap_or_else(|| default.into());
        check_expr(s, &text, Kind::Length, cmd, field)?;
        s.doc.eval(&text, Kind::Length).map_err(|err| bad(cmd, format!("{field}: {err}")))
    };
    let tolerance = length(s, "tolerance", DEFAULT_TOLERANCE)?;
    let grid = length(s, "grid", DEFAULT_GRID)?;
    let min_thickness = length(s, "min_thickness", DEFAULT_MIN_THICKNESS)?;
    let symmetry = match str_(p, "symmetry").unwrap_or("auto") {
        "auto" => Symmetry::Auto,
        "x" => Symmetry::AcrossX,
        "y" => Symmetry::AcrossY,
        "off" => Symmetry::Off,
        other => return Err(bad(cmd, format!("symmetry: auto, x, y or off, not `{other}`"))),
    };
    let axis = match p.get("axis").filter(|v| !v.is_null()) {
        None => None,
        Some(given) => Some(vec3(given).ok_or_else(|| bad(cmd, "axis: [x, y, z]"))?),
    };
    Ok(SlabOptions { tolerance, grid, symmetry, snap_angle: num(p, "snap_angle").unwrap_or(DEFAULT_SNAP), min_thickness, axis })
}

/// `mesh.reconstruct`: read a scan as the stack of layers it was modelled from, and build that
/// stack again as ordinary features.
///
/// Where `mesh.rebuild` starts from certified surfaces and refuses a mesh whose faces will not
/// certify, this starts from measurements and always has an answer, with a deviation attached
/// saying how far to trust it. That is what a scan needs: its faces never certify, because every
/// one of them is a few tenths of a millimetre off being a plane.
fn reconstruct(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.reconstruct";
    let options = reconstruct_options(s, p, cmd)?;
    let body = target(s, p, cmd)?;
    let plan = {
        let state = s.model.state();
        let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
        if !source.body.is_mesh() {
            return Err(bad(cmd, "select a mesh body"));
        }
        slab_plan(&source.body, &options)?
    };
    let mut names = Naming {
        on: bool_(p, "parameters").unwrap_or(true),
        prefix: identifier(str_(p, "prefix").unwrap_or(&body)),
        body: body.clone(),
        made: Vec::new(),
    };
    let steps = slab_steps(s, p, &plan, &mut names);
    let features = if bool_(p, "plan_only").unwrap_or(false) { Vec::new() } else { run_steps(s, &steps, cmd)? };
    let out = MeshReconstructResult {
        body,
        origin: at3(plan.origin),
        axis: at3(plan.axis),
        x_dir: at3(plan.x_dir),
        y_dir: at3(plan.y_dir),
        levels: plan.levels.iter().map(|z| round(*z)).collect(),
        size: [round(plan.size[0]), round(plan.size[1]), round(plan.size[2])],
        slabs: plan.slabs.iter().map(slab_reply).collect(),
        mirrors: plan
            .mirrors
            .iter()
            .map(|m| MeshMirror {
                across: if m.across_x { "x".to_owned() } else { "y".to_owned() },
                at: round(m.at),
                mismatch: round(m.mismatch),
                paired: m.paired,
            })
            .collect(),
        triangles: plan.triangles,
        area: round(plan.area),
        deviation: plan.deviation.map(|d| MeshDeviation { mean: round(d.mean), p95: round(d.p95), max: round(d.max), probes: d.probes }),
        traced: plan.slabs.iter().flat_map(|slab| slab.loops.iter()).filter(|outline| !outline.fitted).count(),
        parameters: names.made,
        steps,
        features,
    };
    serde_json::to_value(out).map_err(|err| crate::EngineError::Other(err.to_string()))
}

fn slab_reply(slab: &solidcraft_kernel::Slab) -> MeshSlab {
    MeshSlab {
        from: round(slab.from),
        to: round(slab.to),
        loops: slab
            .loops
            .iter()
            .map(|outline| MeshLoop {
                lines: outline.parts.iter().filter(|part| matches!(part, SlabPart::Line { .. })).count(),
                arcs: outline.parts.iter().filter(|part| matches!(part, SlabPart::Arc { .. })).count(),
                circle: outline.parts.iter().find_map(|part| match part {
                    SlabPart::Circle { radius, .. } => Some(round(*radius)),
                    _ => None,
                }),
                area: round(outline.area),
                deviation: round(outline.deviation),
                fitted: outline.fitted,
            })
            .collect(),
    }
}

/// Run a rebuild's steps as the one thing the user asked for.
///
/// Every step is a command of its own and records an undo step of its own, so those are folded
/// back into the single step this command's own call records. A step that fails leaves the design
/// as it was, as any failed command does.
fn run_steps(s: &mut Session, steps: &[MeshStep], cmd: &str) -> Result<Vec<String>> {
    let start = s.undo.len();
    let mut features = Vec::new();
    for (at, step) in steps.iter().enumerate() {
        let out = s.execute(&step.command, &step.params).map_err(|err| bad(cmd, format!("step {at} of the rebuild, `{}`: {err}", step.command)))?;
        if out.get("feature").is_some()
            && let Some(name) = out.get("name").and_then(Value::as_str)
        {
            features.push(name.to_owned());
        }
    }
    if s.undo.len() >= start {
        s.undo.truncate(start);
    }
    Ok(features)
}

/// The commands that build the stack, in order: the parameters first, so everything after them
/// can follow them, then a sketch and an extrusion for each layer.
fn slab_steps(s: &Session, p: &Value, plan: &SlabStack, names: &mut Naming) -> Vec<MeshStep> {
    let step = |command: &str, params: Value| MeshStep { command: command.to_owned(), params };
    // The new body is named here because the mesh it was read from stands in the same space.
    let solid = format!("{} rebuilt", names.body);
    let mut build = Vec::new();
    for (layer, slab) in plan.slabs.iter().enumerate() {
        let origin = plan.origin + plan.axis * slab.from;
        build.push(step("sketch.create", json!({"plane": {"origin": at3(origin), "x_dir": at3(plan.x_dir), "y_dir": at3(plan.y_dir)}})));
        for (which, outline) in slab.loops.iter().enumerate() {
            draw_loop(s, outline, layer, which, names, &mut build);
        }
        build.push(step("sketch.finish", json!({})));
        let thickness = names.size(s, &format!("layer_{}_thickness", layer + 1), slab.to - slab.from);
        // The first layer makes the body and the rest join onto it, so a stepped part comes out as
        // one solid with a feature per level, which is how someone would have modelled it.
        let mut extrude = match layer {
            0 => json!({"distance": thickness, "operation": "new", "body_name": solid}),
            _ => json!({"distance": thickness, "operation": "join", "targets": [&solid]}),
        };
        if layer == 0
            && let (Some(name), Some(fields)) = (str_(p, "name"), extrude.as_object_mut())
        {
            fields.insert("name".into(), name.into());
        }
        build.push(step("solid.extrude", extrude));
    }
    let mut steps: Vec<MeshStep> =
        names.made.iter().map(|made| step("parameters.add", json!({"name": made.name, "expression": made.expression}))).collect();
    steps.extend(build);
    steps
}

/// Draw one outline into the sketch being built.
///
/// A run of lines goes in as one call, the way someone drawing it would, and an arc interrupts the
/// run and starts a new one. Only a circle's diameter becomes a parameter: a fitted outline's
/// corners came off the scan and carry no dimension to hold them, so a parameter there would push
/// walls about instead of resizing anything.
fn draw_loop(s: &Session, outline: &solidcraft_kernel::Loop, layer: usize, which: usize, names: &mut Naming, build: &mut Vec<MeshStep>) {
    let step = |command: &str, params: Value| MeshStep { command: command.to_owned(), params };
    let at = |p: [f64; 2]| [fixed(p[0]), fixed(p[1])];
    let circle = |centre: [f64; 2], radius: f64, names: &mut Naming, build: &mut Vec<MeshStep>| {
        let diameter = names.size(s, &format!("layer_{}_circle_{}_dia", layer + 1, which + 1), radius * 2.);
        build.push(step("sketch.circle.center", json!({"center": at(centre), "diameter": diameter})));
    };
    if let [SlabPart::Circle { centre, radius }] = outline.parts.as_slice() {
        circle(*centre, *radius, names, build);
        return;
    }
    // All lines: one closed run, which is both fewer commands and a sketch that reads as a shape.
    if outline.parts.iter().all(|part| matches!(part, SlabPart::Line { .. })) {
        let points: Vec<[String; 2]> = outline
            .parts
            .iter()
            .filter_map(|part| match part {
                SlabPart::Line { from, .. } => Some(at(*from)),
                _ => None,
            })
            .collect();
        if points.len() >= 3 {
            build.push(step("sketch.line", json!({"points": points, "closed": true})));
        }
        return;
    }
    let mut run: Vec<[String; 2]> = Vec::new();
    for part in &outline.parts {
        match part {
            SlabPart::Line { from, to } => {
                if run.is_empty() {
                    run.push(at(*from));
                }
                run.push(at(*to));
            }
            SlabPart::Arc { from, to, centre, ccw, .. } => {
                if run.len() >= 2 {
                    build.push(step("sketch.line", json!({"points": std::mem::take(&mut run)})));
                }
                run.clear();
                build.push(step("sketch.arc.center_point", json!({"center": at(*centre), "start": at(*from), "end": at(*to), "clockwise": !ccw})));
            }
            SlabPart::Circle { centre, radius } => circle(*centre, *radius, names, build),
        }
    }
    if run.len() >= 2 {
        build.push(step("sketch.line", json!({"points": run})));
    }
}

/// How a shape is read as silhouettes when a call does not say.
const DEFAULT_SECTIONS: usize = 16;
const DEFAULT_PROFILE_POINTS: usize = 60;
const DEFAULT_MARGIN: &str = "1.5 mm";

/// The directions to read along, from what the call asked for.
fn loft_axes(p: &Value, cmd: &str) -> Result<Vec<Vec3>> {
    let Some(given) = p.get("axes").filter(|v| !v.is_null()) else { return Ok(vec![Vec3::Z]) };
    let list = given.as_array().ok_or_else(|| bad(cmd, "axes: a list of x, y, z or [x, y, z]"))?;
    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        let axis = match entry.as_str() {
            Some("x" | "X") => Vec3::X,
            Some("y" | "Y") => Vec3::Y,
            Some("z" | "Z") => Vec3::Z,
            Some(other) => return Err(bad(cmd, format!("axes: x, y, z or [x, y, z], not `{other}`"))),
            None => vec3(entry).ok_or_else(|| bad(cmd, "axes: x, y, z or [x, y, z]"))?,
        };
        out.push(axis);
    }
    Ok(out)
}

/// How a shape should be read, from what the call asked for.
fn loft_options(s: &mut Session, p: &Value, cmd: &str) -> Result<SilhouetteOptions> {
    let length = |s: &mut Session, field: &str, default: &str| -> Result<f64> {
        let text = expr(p, field).unwrap_or_else(|| default.into());
        check_expr(s, &text, Kind::Length, cmd, field)?;
        s.doc.eval(&text, Kind::Length).map_err(|err| bad(cmd, format!("{field}: {err}")))
    };
    let whole = |field: &str, default: usize, range: std::ops::RangeInclusive<usize>| -> Result<usize> {
        match num(p, field) {
            None => Ok(default),
            Some(n) if n.fract() == 0. && n >= 0. && range.contains(&(n as usize)) => Ok(n as usize),
            Some(_) => Err(bad(cmd, format!("{field}: a whole number between {} and {}", range.start(), range.end()))),
        }
    };
    let spacing = match expr(p, "spacing") {
        None => None,
        Some(_) => Some(length(s, "spacing", "5 mm")?),
    };
    Ok(SilhouetteOptions {
        axes: loft_axes(p, cmd)?,
        spacing,
        count: whole("sections", DEFAULT_SECTIONS, 2..=MAX_SECTIONS)?,
        points: whole("points", DEFAULT_PROFILE_POINTS, MIN_POINTS..=MAX_POINTS)?,
        margin: length(s, "margin", DEFAULT_MARGIN)?,
        hull: bool_(p, "hull").unwrap_or(false),
    })
}

/// `mesh.loft`: read a shape as the silhouettes it casts along one or more directions, and loft a
/// solid through them.
///
/// This is the path for a mesh that is not a stack of layers and never will be: a scanned handle,
/// a cast housing, a shape a generator produced. `mesh.reconstruct` would force it into slabs and
/// report a large deviation for its trouble; `mesh.rebuild` would refuse it outright. What comes
/// out here is not a dimensioned part and does not pretend to be one. It is a solid with the
/// shape's own silhouette, which booleans, measures, fillets and prints where the mesh could not.
fn loft(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.loft";
    let options = loft_options(s, p, cmd)?;
    let body = target(s, p, cmd)?;
    let found = {
        let state = s.model.state();
        let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
        if !source.body.is_mesh() {
            return Err(bad(cmd, "select a mesh body"));
        }
        silhouettes(&source.body, &options)?
    };
    let prefix = identifier(str_(p, "prefix").unwrap_or(&body));
    let (steps, bodies) = loft_steps(p, &found, &body, &prefix);
    let features = if bool_(p, "plan_only").unwrap_or(false) { Vec::new() } else { run_steps(s, &steps, cmd)? };
    let out = MeshLoftResult {
        body,
        along: found
            .along
            .iter()
            .map(|along| MeshLoftAxis {
                axis: at3(along.axis),
                x_dir: at3(along.x_dir),
                y_dir: at3(along.y_dir),
                heights: along.sections.iter().map(|section| round(section.at)).collect(),
                areas: along.sections.iter().map(|section| round(section.area)).collect(),
                hulled: along.hulled,
                reach: [round(along.reach[0]), round(along.reach[1])],
                across: {
                    let [u, v] = solidcraft_kernel::across(along);
                    [round(u), round(v)]
                },
            })
            .collect(),
        triangles: found.triangles,
        area: round(found.area),
        points: options.points,
        bodies,
        steps,
        features,
    };
    serde_json::to_value(out).map_err(|err| crate::EngineError::Other(err.to_string()))
}

/// The commands that build the lofts, in order: every section's sketch, then the loft through
/// them, then — where more than one direction was read — the intersection that keeps only what
/// all of them agreed was material.
fn loft_steps(p: &Value, found: &Silhouetted, body: &str, prefix: &str) -> (Vec<MeshStep>, Vec<String>) {
    let step = |command: &str, params: Value| MeshStep { command: command.to_owned(), params };
    let mut steps = Vec::new();
    let mut bodies = Vec::new();
    for (which, along) in found.along.iter().enumerate() {
        let made = match which {
            0 => format!("{body} lofted"),
            _ => format!("{body} along {}", which + 1),
        };
        let mut sections = Vec::new();
        for (at, section) in along.sections.iter().enumerate() {
            // Each section's sketch is named, because a loft has to name the sketches it runs
            // through and cannot fall back on whichever one was drawn last.
            let sketch = format!("{prefix}_{}_{}", which + 1, at + 1);
            let origin = along.axis * section.at;
            steps.push(step(
                "sketch.create",
                json!({"plane": {"origin": at3(origin), "x_dir": at3(along.x_dir), "y_dir": at3(along.y_dir)}, "name": sketch}),
            ));
            let points: Vec<[String; 2]> = section.points.iter().map(|q| [fixed(q[0]), fixed(q[1])]).collect();
            steps.push(step("sketch.spline.fit_point", json!({"points": points, "periodic": true})));
            steps.push(step("sketch.finish", json!({})));
            sections.push(json!({"sketch": sketch}));
        }
        let mut loft = json!({"sections": sections, "operation": "new", "body_name": made});
        if which == 0
            && let (Some(name), Some(fields)) = (str_(p, "name"), loft.as_object_mut())
        {
            fields.insert("name".into(), name.into());
        }
        steps.push(step("solid.loft", loft));
        bodies.push(made);
    }
    // What every direction agreed was material is the shape; what only one of them claimed is a
    // concavity another direction could see and it could not.
    if let Some((first, rest)) = bodies.split_first()
        && !rest.is_empty()
    {
        steps.push(step("solid.combine", json!({"targets": [first], "tools": rest, "operation": "intersect"})));
    }
    (steps, bodies)
}
