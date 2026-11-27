//! Generators: an ordering code to a parametric part and back (`generator.list`,
//! `generator.parse`, `generator.code`, `generator.template`, `generator.insert`,
//! `generator.check`). The package is data and a sandboxed script; this file loads it, asks it
//! what a code means and inserts the package's design through [`Session::execute`], so the
//! commands' own schemas, checks and undo apply (docs/formats/generator.md, RFC 0017, #22331).

use serde_json::{Map, Value, json};

use super::CommandSpec;
use crate::generator::code_template::{Declared, Params, Template, TemplateError};
use crate::generator::{Package, Refusal, Resolved, installed};
use crate::params::bad;
use crate::schema::generator::{
    GeneratorCheckParams, GeneratorCheckResult, GeneratorCheckRow, GeneratorCodeParams, GeneratorCodeResult, GeneratorInsertParams,
    GeneratorInsertResult, GeneratorListParams, GeneratorListResult, GeneratorParseParams, GeneratorParseResult, GeneratorProvenance,
    GeneratorSummary, GeneratorTemplateParams, GeneratorTemplateResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const LIST_CONTRACT: TypedContract<GeneratorListParams> = TypedContract::new();
const PARSE_CONTRACT: TypedContract<GeneratorParseParams> = TypedContract::new();
const CODE_CONTRACT: TypedContract<GeneratorCodeParams> = TypedContract::new();
const TEMPLATE_CONTRACT: TypedContract<GeneratorTemplateParams> = TypedContract::new();
const INSERT_CONTRACT: TypedContract<GeneratorInsertParams> = TypedContract::new();
const CHECK_CONTRACT: TypedContract<GeneratorCheckParams> = TypedContract::new();

/// The placement keys of `component.insert_design` that `generator.insert` passes on.
const PLACEMENT_KEYS: [&str; 8] = ["parent", "translate", "axis", "angle", "origin", "matrix", "to_matrix", "tolerance"];

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("generator.list", "List Generators", crate::checked!("generator.list", list))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<GeneratorListResult>)
        .params("no parameters → generators: [{id, name, description, license, author, path, templates, prefixes}] installed for this user, skipped: folders that are not valid packages, with why"),
    CommandSpec::new("generator.parse", "Read Ordering Code", crate::checked!("generator.parse", parse))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&PARSE_CONTRACT)
        .output(output_schema_for::<GeneratorParseResult>)
        .params("generator: an installed package's id or its folder; code: the ordering code → ok, reason? (invalid | not_modelled | limit | error), message? (the generator's own words), generator, code (in the template's spelling), template?, fields?, configuration? (the row to select), parameters (name: expression), description?, provenance? ({row, source, page?, status}), warnings. A refused code is an answer, not an error. Never changes the design"),
    CommandSpec::new("generator.code", "Print Ordering Code", crate::checked!("generator.code", code))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&CODE_CONTRACT)
        .output(output_schema_for::<GeneratorCodeResult>)
        .params("generator: an installed package's id or its folder; parameters: the field values by name; template?: print with this template's pattern → code, template. A value the template cannot say (outside its list or range) is an error naming the field"),
    CommandSpec::new("generator.template", "Try Code Template", crate::checked!("generator.template", template))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&TEMPLATE_CONTRACT)
        .output(output_schema_for::<GeneratorTemplateResult>)
        .params("template: a field template such as MGN{size: 7|9|12|15}R{length: int 40..2000}; code?: a code to read, or parameters?: field values to print → parameters? or code?, prefixes"),
    CommandSpec::new("generator.insert", "Insert Part From Ordering Code", crate::checked!("generator.insert", insert))
        .begins_edit()
        .icon("component")
        .contract(&INSERT_CONTRACT)
        .output(output_schema_for::<GeneratorInsertResult>)
        .params("generator: an installed package's id or its folder; code?: the ordering code, or parameters?: field values as generator.parse gives them; placement?: {parent, translate, axis, angle, origin, matrix, to_matrix, tolerance} as component.insert_design takes them → generator, code, component, occurrence?, configuration?, parameters, description?, provenance?, warnings, bodies, volume_mm3?, bbox_mm? [x, y, z], faces?. A new component linked to the generator's parametric design, built with the code's configuration row and parameters, its part number the printed code; one undo step. component.set_link_parameters changes its values later. A refused code is an error and leaves the design as it was"),
    CommandSpec::new("generator.check", "Check Generator", crate::checked!("generator.check", check))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&CHECK_CONTRACT)
        .output(output_schema_for::<GeneratorCheckResult>)
        .params("generator: an installed package's id or its folder → generator, passed, results: [{code, ok, problems}]. Runs the package's declared [[example]] codes, each in a scratch design: it must read, print back as itself, insert and measure as declared (volume, bounding box, faces), and an invalid example must be refused; the open design is not changed"),
];

fn params<P: serde::de::DeserializeOwned>(cmd: &str, p: &Value) -> Result<P> {
    let p = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    serde_json::from_value(p).map_err(|e| bad(cmd, e.to_string()))
}

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn load(cmd: &str, generator: &str) -> Result<Package> {
    installed::load(generator).map_err(|e| bad(cmd, e))
}

fn to_map(p: Params) -> Map<String, Value> {
    p.into_iter().collect()
}

fn to_params(m: Map<String, Value>) -> Params {
    m.into_iter().collect()
}

fn provenance(r: &Resolved) -> Option<GeneratorProvenance> {
    let (name, row) = r.configuration.as_ref().zip(r.row.as_ref())?;
    Some(GeneratorProvenance { row: name.clone(), source: row.source.clone(), page: row.page.clone(), status: row.status.clone(), note: row.note.clone() })
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.list";
    let GeneratorListParams {} = params(cmd, p)?;
    let (found, skipped) = installed::list();
    let generators = found
        .iter()
        .map(|i| {
            let g = &i.package;
            GeneratorSummary {
                id: g.id.clone(),
                name: g.name.clone(),
                description: g.description.clone(),
                license: g.license.clone(),
                author: g.author.clone(),
                path: i.path.to_string_lossy().into_owned(),
                templates: g.templates.iter().map(|t| t.template.source().to_string()).collect(),
                prefixes: g.templates.iter().flat_map(|t| t.template.prefixes()).collect(),
            }
        })
        .collect();
    reply(cmd, GeneratorListResult { generators, skipped, param_warnings: Vec::new() })
}

fn parse(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.parse";
    let q: GeneratorParseParams = params(cmd, p)?;
    let g = load(cmd, &q.generator)?;
    let typed: String = q.code.trim().chars().take(128).collect();
    let out = match g.parse(&q.code) {
        Ok(r) => GeneratorParseResult {
            ok: true,
            reason: None,
            message: None,
            generator: g.id.clone(),
            code: r.code.clone(),
            template: g.templates.get(r.template).map(|t| t.template.source().to_string()),
            provenance: provenance(&r),
            fields: Some(to_map(r.fields)),
            configuration: r.configuration,
            parameters: r.parameters,
            description: r.description,
            warnings: r.warnings,
            param_warnings: Vec::new(),
        },
        Err(Refusal { kind, message }) => GeneratorParseResult {
            ok: false,
            reason: Some(kind.as_str().into()),
            message: Some(message),
            generator: g.id.clone(),
            code: typed,
            template: None,
            fields: None,
            configuration: None,
            parameters: Default::default(),
            description: None,
            provenance: None,
            warnings: Vec::new(),
            param_warnings: Vec::new(),
        },
    };
    reply(cmd, out)
}

fn code(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.code";
    let q: GeneratorCodeParams = params(cmd, p)?;
    let g = load(cmd, &q.generator)?;
    let (i, printed) = g.code_for(&to_params(q.parameters), q.template.as_deref()).map_err(|e| bad(cmd, e.to_string()))?;
    let template = g.templates.get(i).map(|t| t.template.source().to_string()).unwrap_or_default();
    reply(cmd, GeneratorCodeResult { code: printed, template, param_warnings: Vec::new() })
}

fn template(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.template";
    let q: GeneratorTemplateParams = params(cmd, p)?;
    let t = Template::parse(&q.template, &Declared::new()).map_err(|e| bad(cmd, e.to_string()))?;
    let prefixes = t.prefixes();
    let out = match (q.code, q.parameters) {
        (Some(c), None) => match t.match_code(&c) {
            Ok(Some(found)) => GeneratorTemplateResult { parameters: Some(to_map(found)), code: None, prefixes, param_warnings: Vec::new() },
            Ok(None) => return Err(bad(cmd, format!("`{}` does not fit the template", c.trim().chars().take(40).collect::<String>()))),
            Err(e) => return Err(bad(cmd, e.to_string())),
        },
        (None, Some(m)) => {
            let printed = t.print(&to_params(m)).map_err(|e: TemplateError| bad(cmd, e.to_string()))?;
            GeneratorTemplateResult { parameters: None, code: Some(printed), prefixes, param_warnings: Vec::new() }
        }
        _ => return Err(bad(cmd, "give `code` or `parameters`, not both")),
    };
    reply(cmd, out)
}

fn body_names(s: &mut Session) -> Result<Vec<String>> {
    let v = s.execute("inspect.measure", &json!({"names_only": true}))?;
    Ok(v.get("bodies")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|b| b.get("name").and_then(Value::as_str)).map(str::to_string).collect())
        .unwrap_or_default())
}

fn triple(v: Option<&Value>) -> Option<[f64; 3]> {
    let a = v?.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}

/// The volume, bounding-box size and face count of the named bodies.
fn measure(s: &mut Session, names: &[String]) -> Result<(Option<f64>, Option<[f64; 3]>, Option<u64>)> {
    let v = s.execute("inspect.measure", &json!({"bodies": names}))?;
    let total = v.get("total");
    let volume = total.and_then(|t| t.get("volume_mm3")).and_then(Value::as_f64);
    let faces = total.and_then(|t| t.get("faces")).and_then(Value::as_u64);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for b in v.get("bodies").and_then(Value::as_array).into_iter().flatten() {
        let bbox = b.get("bbox");
        if let (Some(min), Some(max)) = (triple(bbox.and_then(|x| x.get("min"))), triple(bbox.and_then(|x| x.get("max")))) {
            any = true;
            for (k, (a, b)) in min.iter().zip(max.iter()).enumerate() {
                if let (Some(l), Some(h)) = (lo.get_mut(k), hi.get_mut(k)) {
                    *l = l.min(*a);
                    *h = h.max(*b);
                }
            }
        }
    }
    let size = any.then(|| [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]);
    Ok((volume, size, faces))
}

/// What an insertion made.
struct Inserted {
    component: u64,
    occurrence: Option<u64>,
    bodies: Vec<String>,
    volume_mm3: Option<f64>,
    bbox_mm: Option<[f64; 3]>,
    faces: Option<u64>,
}

/// Insert the package's design for `r` in `s`, name and describe the component from the code.
fn run_insert(s: &mut Session, cmd: &str, g: &Package, r: &Resolved, placement: Option<&Map<String, Value>>) -> Result<Inserted> {
    let before = body_names(s)?;
    let mut call = Map::new();
    call.insert("path".into(), json!(g.model.to_string_lossy()));
    call.insert("name".into(), json!(r.code));
    if let Some(row) = &r.configuration {
        call.insert("configuration".into(), json!(row));
    }
    if !r.parameters.is_empty() {
        call.insert("parameters".into(), json!(r.parameters));
    }
    if let Some(place) = placement {
        for (k, v) in place {
            if !PLACEMENT_KEYS.contains(&k.as_str()) {
                return Err(bad(cmd, format!("`placement.{k}` is not one of {}", PLACEMENT_KEYS.join(", "))));
            }
            call.insert(k.clone(), v.clone());
        }
    }
    let made = s.execute("component.insert_design", &Value::Object(call))?;
    let component = made.get("component").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "the part was not inserted"))?;
    let occurrence = made.get("occurrence").and_then(Value::as_u64);
    // The ordering code is the part number, so the bill of materials names the part as ordered.
    let description = r.description.clone().unwrap_or_else(|| g.name.clone());
    let _ = s.execute("component.metadata.set", &json!({"component": component, "part_number": r.code, "description": description}));
    let bodies: Vec<String> = body_names(s)?.into_iter().filter(|n| !before.contains(n)).collect();
    let (volume_mm3, bbox_mm, faces) = if bodies.is_empty() { (None, None, None) } else { measure(s, &bodies)? };
    Ok(Inserted { component, occurrence, bodies, volume_mm3, bbox_mm, faces })
}

fn resolve_for_insert(cmd: &str, g: &Package, code: Option<&str>, fields: Option<Map<String, Value>>) -> Result<Resolved> {
    let refused = |r: Refusal| bad(cmd, r.message);
    match (code, fields) {
        (Some(c), None) => g.parse(c).map_err(refused),
        (None, Some(m)) => {
            let fields = to_params(m);
            let (i, _) = g.code_for(&fields, None).map_err(|e| bad(cmd, e.to_string()))?;
            g.resolve_fields(i, fields).map_err(refused)
        }
        _ => Err(bad(cmd, "give `code` or `parameters`, not both")),
    }
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.insert";
    let q: GeneratorInsertParams = params(cmd, p)?;
    let g = load(cmd, &q.generator)?;
    let r = resolve_for_insert(cmd, &g, q.code.as_deref(), q.parameters)?;
    let made = run_insert(s, cmd, &g, &r, q.placement.as_ref())?;
    reply(
        cmd,
        GeneratorInsertResult {
            generator: g.id.clone(),
            code: r.code.clone(),
            component: made.component,
            occurrence: made.occurrence,
            provenance: provenance(&r),
            configuration: r.configuration,
            parameters: r.parameters,
            description: r.description,
            warnings: r.warnings,
            bodies: made.bodies,
            volume_mm3: made.volume_mm3,
            bbox_mm: made.bbox_mm,
            faces: made.faces,
            param_warnings: Vec::new(),
        },
    )
}

fn close(a: f64, b: f64, rel: f64, abs: f64) -> bool {
    (a - b).abs() <= abs.max(rel * b.abs())
}

fn check_example(g: &Package, e: &crate::generator::package::Example) -> GeneratorCheckRow {
    let mut problems = Vec::new();
    if e.invalid {
        match g.parse(&e.code) {
            Err(Refusal { message, .. }) => {
                if let Some(text) = &e.message_contains
                    && !message.contains(text.as_str())
                {
                    problems.push(format!("refused with `{message}`, which does not contain `{text}`"));
                }
            }
            Ok(_) => problems.push("accepted, but the example must be refused".into()),
        }
        return GeneratorCheckRow { code: e.code.clone(), ok: problems.is_empty(), problems };
    }
    let resolved = match g.parse(&e.code) {
        Ok(r) => r,
        Err(Refusal { message, .. }) => {
            problems.push(format!("refused: {message}"));
            return GeneratorCheckRow { code: e.code.clone(), ok: false, problems };
        }
    };
    // The code printed back reads as the same fields.
    match g.read_code(&resolved.code) {
        Ok((_, again)) => {
            if again != resolved.fields && g.templates.get(resolved.template).is_some_and(|t| t.set.is_empty()) {
                problems.push(format!("`{}` printed back as `{}`, which reads differently", e.code, resolved.code));
            }
        }
        Err(r) => problems.push(format!("`{}` printed back as `{}`, which is refused: {}", e.code, resolved.code, r.message)),
    }
    let mut scratch = Session::default();
    match run_insert(&mut scratch, "generator.check", g, &resolved, None) {
        Err(err) => problems.push(err.to_string()),
        Ok(made) => {
            if let Some(want) = e.volume_mm3 {
                match made.volume_mm3 {
                    Some(got) if close(got, want, 1e-6, 0.0) => {}
                    got => problems.push(format!("volume {got:?} mm3, expected {want}")),
                }
            }
            if let Some(want) = e.bbox_mm {
                match made.bbox_mm {
                    Some(got) if got.iter().zip(want).all(|(a, b)| close(*a, b, 0.0, 1e-6)) => {}
                    got => problems.push(format!("bounding box {got:?}, expected {want:?}")),
                }
            }
            if let Some(want) = e.faces
                && made.faces != Some(want)
            {
                problems.push(format!("{:?} faces, expected {want}", made.faces));
            }
        }
    }
    GeneratorCheckRow { code: e.code.clone(), ok: problems.is_empty(), problems }
}

fn check(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.check";
    let q: GeneratorCheckParams = params(cmd, p)?;
    let g = load(cmd, &q.generator)?;
    let results: Vec<GeneratorCheckRow> = g.examples.iter().map(|e| check_example(&g, e)).collect();
    let passed = results.iter().all(|r| r.ok);
    reply(cmd, GeneratorCheckResult { generator: g.id.clone(), passed, results, param_warnings: Vec::new() })
}
