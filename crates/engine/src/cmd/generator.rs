//! Generators: an ordering code to a part (`generator.parse`, `generator.build`,
//! `generator.check`, `generator.list`). The package is data; this file only loads it, asks it
//! for a plan and runs the plan's commands through [`Session::execute`], so the commands' own
//! schemas, checks and undo apply (docs/formats/generator-package.md, RFC 0017, #22331).

use std::path::Path;

use serde_json::{Value, json};

use super::CommandSpec;
use crate::generator::{Check, Generator, GeneratorError, Parsed, Plan, RowUsed, installed};
use crate::params::bad;
use crate::schema::generator::{
    GeneratorBuildParams, GeneratorBuildResult, GeneratorCheckParams, GeneratorCheckResult, GeneratorCheckRow, GeneratorListParams,
    GeneratorListResult, GeneratorParseParams, GeneratorParseResult, GeneratorRow, GeneratorSource, GeneratorSummary, GeneratorValue,
    InstalledGenerator,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const PARSE_CONTRACT: TypedContract<GeneratorParseParams> = TypedContract::new();
const BUILD_CONTRACT: TypedContract<GeneratorBuildParams> = TypedContract::new();
const CHECK_CONTRACT: TypedContract<GeneratorCheckParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<GeneratorListParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("generator.parse", "Read Ordering Code", crate::checked!("generator.parse", parse))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&PARSE_CONTRACT)
        .output(output_schema_for::<GeneratorParseResult>)
        .params("package?: a .generator.json path or an installed package's id; package_json?: the package as an object or JSON text; code: the ordering code → ok, reason? (invalid | not_modelled | error), message? (the package's own words), generator, code, fields, variables, rows (the table rows used, with source, page and status), commands (how many a build would run), component. A refused code is an answer, not an error. Never changes the design"),
    CommandSpec::new("generator.build", "Build Part From Ordering Code", crate::checked!("generator.build", build))
        .begins_edit()
        .icon("component")
        .contract(&BUILD_CONTRACT)
        .output(output_schema_for::<GeneratorBuildResult>)
        .params("package?: a .generator.json path or an installed package's id; package_json?: the package as an object or JSON text; code: the ordering code; save?: a path to save the design to afterwards → generator, code, component, commands, bodies, volume_mm3, bbox_mm [x, y, z], faces, fields, rows, saved. Makes a new component named after the code and runs the package's commands in it, as one undo step; a refused code or a failing step is an error and leaves the design as it was"),
    CommandSpec::new("generator.check", "Check Generator", crate::checked!("generator.check", check))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&CHECK_CONTRACT)
        .output(output_schema_for::<GeneratorCheckResult>)
        .params("package?: a .generator.json path or an installed package's id; package_json?: the package as an object or JSON text → generator, passed, results: [{code, ok, problems}]. Runs the package's own checks (volume, bounding box, faces, dimensions and refusals per code) and its declared examples, each in a scratch design; the open design is not changed"),
    CommandSpec::new("generator.list", "List Generators", crate::checked!("generator.list", list))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<GeneratorListResult>)
        .params("no parameters → generators: [{id, name, description, guarantee?, path}] installed for this user, skipped: files that are not valid packages, with why"),
];

fn params<P: serde::de::DeserializeOwned>(cmd: &str, p: &Value) -> Result<P> {
    let p = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    serde_json::from_value(p).map_err(|e| bad(cmd, e.to_string()))
}

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// A package file's text, never more than the package limit.
fn read_text(cmd: &str, path: &Path) -> Result<String> {
    installed::read_text(path).map_err(|e| bad(cmd, e))
}

fn load_text(cmd: &str, package: Option<&str>, package_json: Option<&Value>) -> Result<String> {
    match (package, package_json) {
        (Some(_), Some(_)) => Err(bad(cmd, "give `package` or `package_json`, not both")),
        (None, None) => Err(bad(cmd, "give `package` (a path or an installed id) or `package_json`")),
        (None, Some(Value::String(text))) => Ok(text.clone()),
        (None, Some(object @ Value::Object(_))) => serde_json::to_string(object).map_err(|e| bad(cmd, e.to_string())),
        (None, Some(_)) => Err(bad(cmd, "`package_json` must be an object or its JSON text")),
        (Some(name), None) => {
            let named_file = name.contains('/') || name.contains('\\') || name.ends_with(".json");
            if named_file {
                return read_text(cmd, Path::new(name));
            }
            if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                return Err(bad(cmd, format!("`{name}` is neither a package file nor an installed package id")));
            }
            let dir = installed::dir().ok_or_else(|| bad(cmd, "there is no folder for installed generators on this system"))?;
            read_text(cmd, &dir.join(format!("{name}.generator.json")))
        }
    }
}

fn load(cmd: &str, package: Option<&str>, package_json: Option<&Value>) -> Result<Generator> {
    let text = load_text(cmd, package, package_json)?;
    Generator::load(&text, env!("CARGO_PKG_VERSION")).map_err(|e| bad(cmd, e.to_string()))
}

fn summary(g: &Generator) -> GeneratorSummary {
    let m = &g.meta;
    GeneratorSummary {
        id: m.id.clone(),
        name: m.name.clone(),
        description: m.description.clone(),
        license: m.license.clone(),
        author: m.author.clone(),
        guarantee: m.guarantee.clone(),
        provenance: m.provenance.clone(),
        sources: m
            .sources
            .iter()
            .map(|(id, title, url, pages)| GeneratorSource { id: id.clone(), title: title.clone(), url: url.clone(), pages: pages.clone() })
            .collect(),
    }
}

fn values(list: &[(String, crate::generator::expr::Value)]) -> Vec<GeneratorValue> {
    list.iter().map(|(name, v)| GeneratorValue { name: name.clone(), value: v.to_json().unwrap_or(Value::Null) }).collect()
}

fn rows(list: &[RowUsed]) -> Vec<GeneratorRow> {
    list.iter()
        .map(|r| GeneratorRow {
            lookup: r.lookup.clone(),
            table: r.table.clone(),
            key: r.key.clone(),
            source: r.source.clone(),
            page: r.page.clone(),
            status: r.status.clone(),
            note: r.note.clone(),
        })
        .collect()
}

fn parse(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.parse";
    let q: GeneratorParseParams = params(cmd, p)?;
    let g = load(cmd, q.package.as_deref(), q.package_json.as_ref())?;
    let mut out = GeneratorParseResult {
        ok: false,
        reason: None,
        message: None,
        generator: summary(&g),
        code: q.code.trim().chars().take(128).collect(),
        fields: Vec::new(),
        variables: Vec::new(),
        rows: Vec::new(),
        commands: None,
        component: None,
        param_warnings: Vec::new(),
    };
    match g.plan(&q.code) {
        Ok(Plan { parsed, component, commands }) => {
            out.ok = true;
            out.code = parsed.code.clone();
            out.fields = values(&parsed.fields);
            out.variables = values(&parsed.vars);
            out.rows = rows(&parsed.rows);
            out.commands = Some(commands.len());
            out.component = Some(component);
        }
        Err(GeneratorError::Refused { kind, message }) => {
            out.reason = Some(kind.as_str().into());
            out.message = Some(message);
        }
        Err(e) => {
            out.reason = Some("error".into());
            out.message = Some(e.to_string());
        }
    }
    reply(cmd, out)
}

/// What a build made.
struct Built {
    component: String,
    commands: usize,
    bodies: Vec<String>,
    volume_mm3: Option<f64>,
    bbox_mm: Option<[f64; 3]>,
    faces: Option<u64>,
    parsed: Parsed,
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

/// Run a plan in `s`: a new component named after the code, then the commands in it.
fn run_build(s: &mut Session, cmd: &str, g: &Generator, code: &str) -> Result<Built> {
    let plan = g.plan(code).map_err(|e| bad(cmd, e.to_string()))?;
    let before = body_names(s)?;
    s.execute("component.create", &json!({"name": plan.component, "activate": true}))?;
    // The ordering code is the part number, so the bill of materials names the part as ordered.
    let _ = s.execute("component.metadata.set", &json!({"component": plan.component, "part_number": plan.parsed.code, "description": g.meta.name}));
    for c in &plan.commands {
        s.execute(&c.command, &c.params).map_err(|e| bad(cmd, format!("step {} (`{}`) failed: {e}", c.step + 1, c.command)))?;
    }
    // Leave the person where they were: the part is a component of its own.
    let _ = s.execute("component.activate.parent", &json!({}));
    let bodies: Vec<String> = body_names(s)?.into_iter().filter(|n| !before.contains(n)).collect();
    let (volume_mm3, bbox_mm, faces) = if bodies.is_empty() { (None, None, None) } else { measure(s, &bodies)? };
    Ok(Built { component: plan.component, commands: plan.commands.len(), bodies, volume_mm3, bbox_mm, faces, parsed: plan.parsed })
}

fn build(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.build";
    let q: GeneratorBuildParams = params(cmd, p)?;
    let g = load(cmd, q.package.as_deref(), q.package_json.as_ref())?;
    let built = run_build(s, cmd, &g, &q.code)?;
    let saved = match &q.save {
        Some(path) => {
            s.execute("file.save", &json!({"path": path}))?;
            Some(path.clone())
        }
        None => None,
    };
    reply(
        cmd,
        GeneratorBuildResult {
            generator: summary(&g),
            code: built.parsed.code.clone(),
            component: built.component,
            commands: built.commands,
            bodies: built.bodies,
            volume_mm3: built.volume_mm3,
            bbox_mm: built.bbox_mm,
            faces: built.faces,
            fields: values(&built.parsed.fields),
            rows: rows(&built.parsed.rows),
            saved,
            param_warnings: Vec::new(),
        },
    )
}

fn close(a: f64, b: f64, rel: f64, abs: f64) -> bool {
    (a - b).abs() <= abs.max(rel * b.abs())
}

fn run_check(g: &Generator, c: &Check) -> GeneratorCheckRow {
    let mut problems = Vec::new();
    if c.invalid {
        match g.plan(&c.code) {
            Err(GeneratorError::Refused { message, .. }) => {
                if let Some(text) = &c.message_contains
                    && !message.contains(text.as_str())
                {
                    problems.push(format!("refused with `{message}`, which does not contain `{text}`"));
                }
            }
            Err(e) => problems.push(format!("refused for another reason: {e}")),
            Ok(_) => problems.push("accepted, but the check says the code must be refused".into()),
        }
    } else {
        let mut scratch = Session::default();
        match run_build(&mut scratch, "generator.check", g, &c.code) {
            Err(e) => problems.push(e.to_string()),
            Ok(built) => {
                let rel = c.rel_tol.unwrap_or(1e-6);
                let abs = c.abs_tol.unwrap_or(1e-6);
                if let Some(want) = c.volume_mm3 {
                    match built.volume_mm3 {
                        Some(got) if close(got, want, rel, 0.0) => {}
                        got => problems.push(format!("volume {got:?} mm3, expected {want}")),
                    }
                }
                if let Some(want) = c.bbox_mm {
                    match built.bbox_mm {
                        Some(got) if got.iter().zip(want).all(|(a, b)| close(*a, b, 0.0, abs)) => {}
                        got => problems.push(format!("bounding box {got:?}, expected {want:?}")),
                    }
                }
                if let Some(want) = c.faces
                    && built.faces != Some(want)
                {
                    problems.push(format!("{:?} faces, expected {want}", built.faces));
                }
                for (name, want) in &c.dimensions {
                    match Generator::named_value(&built.parsed, name) {
                        Some(got) if close(got, *want, 0.0, 1e-9) => {}
                        got => problems.push(format!("dimension `{name}` is {got:?}, expected {want}")),
                    }
                }
            }
        }
    }
    GeneratorCheckRow { code: c.code.clone(), ok: problems.is_empty(), problems }
}

fn check(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.check";
    let q: GeneratorCheckParams = params(cmd, p)?;
    let g = load(cmd, q.package.as_deref(), q.package_json.as_ref())?;
    let mut results = Vec::new();
    for (code, positive) in &g.declared {
        let accepted = g.parse(code).is_ok();
        let problems = if accepted == *positive {
            Vec::new()
        } else {
            vec![format!(
                "declared as {}, but the grammar {}",
                if *positive { "an example" } else { "a non-example" },
                if accepted { "accepts it" } else { "refuses it" }
            )]
        };
        results.push(GeneratorCheckRow { code: code.clone(), ok: problems.is_empty(), problems });
    }
    for c in &g.checks {
        results.push(run_check(&g, c));
    }
    let passed = results.iter().all(|r| r.ok);
    reply(cmd, GeneratorCheckResult { generator: summary(&g), passed, results, param_warnings: Vec::new() })
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generator.list";
    let GeneratorListParams {} = params(cmd, p)?;
    let (found, skipped) = installed::list();
    let generators = found
        .iter()
        .map(|i| {
            let m = &i.generator.meta;
            InstalledGenerator {
                id: m.id.clone(),
                name: m.name.clone(),
                description: m.description.clone(),
                guarantee: m.guarantee.clone(),
                path: i.path.to_string_lossy().into_owned(),
            }
        })
        .collect();
    reply(cmd, GeneratorListResult { generators, skipped, param_warnings: Vec::new() })
}
