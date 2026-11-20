//! `solidcraft-cli check`: Check Drawing (`drawing.check`, #13054) on every drawing of a design, or
//! the one `--drawing` names, as one JSON report. The exit is 1 when anything is found, after the
//! report, as `oracle` fails after its report, so a script or a CI job can stop on it.

use serde_json::{Value, json};
use solidcraft_engine::Session;

/// The command's usage, from the same lines the top-level usage lists.
fn usage() -> String {
    crate::command_usage("check")
}

pub fn run(args: &[String]) -> Result<(), String> {
    crate::known_args("check", args, &["--drawing", "--sheet", "--json-out"], &[], true)?;
    let json_out = crate::json_out(args)?;
    let path = crate::positional(args).ok_or_else(usage)?;
    let mut s = crate::load(path)?;
    let report = check(&mut s, crate::flag(args, "--drawing"), crate::flag(args, "--sheet"))?;
    crate::print_json(&report, json_out)?;
    let found = report["findings"].as_u64().unwrap_or(0);
    let drawings = report["drawings"].as_array().into_iter().flatten();
    match drawings.filter(|d| d["check"]["findings"].as_array().is_some_and(|f| !f.is_empty())).count() {
        0 => Ok(()),
        with => Err(format!("{found} finding(s) in {with} drawing(s)")),
    }
}

/// `{drawings: [{id, name, check}], findings}`: each chosen drawing opened, checked (on `sheet`
/// when given) and closed in turn, with `drawing.check`'s reply unchanged, and the total.
fn check(s: &mut Session, drawing: Option<&str>, sheet: Option<&str>) -> Result<Value, String> {
    let mut exec = |cmd: &str, p: Value| s.execute(cmd, &p).map_err(|e| e.to_string());
    let listed = exec("drawing.list", json!({}))?;
    let all = listed["drawings"].as_array().map(Vec::as_slice).unwrap_or_default();
    let chosen = match drawing {
        Some(wanted) => vec![one(all, wanted)?],
        None => all.iter().collect(),
    };
    if sheet.is_some() && chosen.len() > 1 {
        return Err("--sheet needs --drawing when the design has more than one drawing: sheet names are per drawing".into());
    }
    let mut drawings = Vec::with_capacity(chosen.len());
    let mut found = 0;
    for d in chosen {
        exec("drawing.open", json!({"drawing": d["id"]}))?;
        let check = exec("drawing.check", sheet.map_or_else(|| json!({}), |sheet| json!({"sheet": sheet})))?;
        exec("drawing.close", json!({}))?;
        found += check["findings"].as_array().map_or(0, Vec::len);
        drawings.push(json!({"id": d["id"], "name": d["name"], "check": check}));
    }
    Ok(json!({"drawings": drawings, "findings": found}))
}

/// The listed drawing whose id or name is `wanted`. A name two drawings share is refused, naming
/// their ids, since checking one of them would leave the other unchecked without a word.
fn one<'a>(all: &'a [Value], wanted: &str) -> Result<&'a Value, String> {
    if let Some(d) = all.iter().find(|d| d["id"] == wanted) {
        return Ok(d);
    }
    match all.iter().filter(|d| d["name"] == wanted).collect::<Vec<_>>().as_slice() {
        [d] => Ok(*d),
        [] => Err(format!("the design has no drawing `{wanted}`, by id or by name (drawing.list)")),
        many => {
            let ids: Vec<&str> = many.iter().filter_map(|d| d["id"].as_str()).collect();
            Err(format!("{} drawings are called `{wanted}`: give one's id ({})", many.len(), ids.join(", ")))
        }
    }
}
