//! Fixture loading: `fixtures/<unit>/*.json` and upstream test files.
//!
//! Fixture file format (either form):
//!
//! ```json
//! { "cases": [ { "name": "...", "upstream_ref": "tests/...cpp:123", "inputs": { ... } } ] }
//! [ { "name": "...", "inputs": { ... } } ]
//! ```
//!
//! Inside `inputs`, these objects are expanded at load time:
//! * `{"$pts": "Points.pts"}` → `[[x,y,z], ...]` read from
//!   `upstream/TestFiles/pt_TestFiles/Points.pts`;
//! * `{"$upstream_file": "Boolean/a.smb"}` → absolute path string of that file
//!   (for runners whose C++ side reads the file directly);
//! * `{"$repo_file": "fixtures/<unit>/data/a.smb"}` → absolute path string of that
//!   file under `fixtures/` (an input generated for a case, read by both sides).

#[cfg(feature = "cpp")]
pub(crate) mod regions;
#[cfg(feature = "cpp")]
pub(crate) mod text_curve;

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::case::{Case, CaseSource};
use crate::HarnessError;

/// Directory holding upstream test inputs (licence allow-listed).
pub const UPSTREAM_TEST_FILES: &str = "upstream/TestFiles/pt_TestFiles";

/// Resolves and checks a file under [`UPSTREAM_TEST_FILES`].
pub fn upstream_test_file(root: &Path, rel: &str) -> Result<PathBuf, HarnessError> {
    if rel.split(['/', '\\']).any(|c| c == "..") {
        return Err(HarnessError::new(format!(
            "upstream file `{rel}`: `..` not allowed"
        )));
    }
    let p = root.join(UPSTREAM_TEST_FILES).join(rel);
    if p.is_file() {
        Ok(p)
    } else {
        Err(HarnessError::new(format!(
            "upstream test file not found: {}",
            p.display()
        )))
    }
}

/// Resolves and checks a file under the repository's `fixtures/` directory (`rel` is
/// relative to the repository root and starts with `fixtures/`).
pub fn repo_fixture_file(root: &Path, rel: &str) -> Result<PathBuf, HarnessError> {
    if rel.split(['/', '\\']).any(|c| c == "..") || !rel.starts_with("fixtures/") {
        return Err(HarnessError::new(format!(
            "repo file `{rel}`: must be under `fixtures/` without `..`"
        )));
    }
    let p = root.join(rel);
    if p.is_file() {
        Ok(p)
    } else {
        Err(HarnessError::new(format!(
            "repo fixture file not found: {}",
            p.display()
        )))
    }
}

/// Parses an upstream `.pts` file: first token is the point count, then
/// `x y z` triples (whitespace separated).
pub fn load_pts(path: &Path) -> Result<Vec<[f64; 3]>, HarnessError> {
    let text = fs::read_to_string(path)
        .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
    let mut toks = text.split_whitespace();
    let n: usize = toks
        .next()
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| HarnessError::new(format!("{}: missing point count", path.display())))?;
    let mut pts = Vec::with_capacity(n.min(1 << 20));
    for i in 0..n {
        let mut p = [0.0; 3];
        for c in &mut p {
            *c = toks
                .next()
                .and_then(|t| t.parse().ok())
                .ok_or_else(|| HarnessError::new(format!("{}: bad point {i}", path.display())))?;
        }
        pts.push(p);
    }
    Ok(pts)
}

/// Loads every `*.json` in `fixtures/<unit>/`, sorted by file name. A missing
/// directory yields no cases. Case names must be unique within the unit.
pub fn load_fixture_dir(root: &Path, unit: &str) -> Result<Vec<Case>, HarnessError> {
    let dir = root.join("fixtures").join(unit);
    let Ok(rd) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    let mut cases: Vec<Case> = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&f).map_err(|e| HarnessError::new(format!("{rel}: {e}")))?;
        let json: Value =
            serde_json::from_str(&text).map_err(|e| HarnessError::new(format!("{rel}: {e}")))?;
        let list = match &json {
            Value::Array(a) => a.clone(),
            Value::Object(o) => match o.get("cases") {
                Some(Value::Array(a)) => a.clone(),
                _ => return Err(HarnessError::new(format!("{rel}: expected `cases` array"))),
            },
            _ => {
                return Err(HarnessError::new(format!(
                    "{rel}: expected object or array"
                )))
            }
        };
        for (index, entry) in list.iter().enumerate() {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| HarnessError::new(format!("{rel}#{index}: missing `name`")))?
                .to_string();
            if cases.iter().any(|c| c.name == name) {
                return Err(HarnessError::new(format!(
                    "{rel}#{index}: duplicate case name `{name}`"
                )));
            }
            let upstream_ref = entry
                .get("upstream_ref")
                .and_then(Value::as_str)
                .map(str::to_string);
            let raw = entry.get("inputs").cloned().unwrap_or(Value::Null);
            let inputs =
                expand(root, raw).map_err(|e| HarnessError::new(format!("{rel}#{index}: {e}")))?;
            cases.push(Case {
                unit: unit.to_string(),
                name,
                source: CaseSource::Fixture {
                    path: rel.clone(),
                    index,
                    upstream_ref,
                },
                inputs,
            });
        }
    }
    Ok(cases)
}

/// Expands the `$pts` / `$upstream_file` / `$repo_file` objects of a case's `inputs`, as
/// [`load_fixture_dir`] does (for case files kept outside `fixtures/`).
pub fn expand_inputs(root: &Path, inputs: Value) -> Result<Value, HarnessError> {
    expand(root, inputs)
}

fn expand(root: &Path, v: Value) -> Result<Value, HarnessError> {
    match v {
        Value::Object(o) => {
            if o.len() == 1 {
                if let Some(Value::String(rel)) = o.get("$pts") {
                    let pts = load_pts(&upstream_test_file(root, rel)?)?;
                    return Ok(Value::Array(
                        pts.iter()
                            .map(|p| Value::Array(p.iter().map(|&x| Value::from(x)).collect()))
                            .collect(),
                    ));
                }
                if let Some(Value::String(rel)) = o.get("$upstream_file") {
                    let p = upstream_test_file(root, rel)?;
                    return Ok(Value::String(p.to_string_lossy().into_owned()));
                }
                if let Some(Value::String(rel)) = o.get("$repo_file") {
                    let p = repo_fixture_file(root, rel)?;
                    return Ok(Value::String(p.to_string_lossy().into_owned()));
                }
            }
            let mut out = serde_json::Map::new();
            for (k, x) in o {
                out.insert(k, expand(root, x)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(a) => Ok(Value::Array(
            a.into_iter()
                .map(|x| expand(root, x))
                .collect::<Result<_, _>>()?,
        )),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pts_loader() {
        let root = crate::case::repo_root();
        let Ok(p) = upstream_test_file(&root, "FitCrvPoints.pts") else {
            return; // upstream checkout absent
        };
        let pts = load_pts(&p).unwrap();
        assert_eq!(pts.len(), 12);
        assert_eq!(pts[0], [-11.0, 2.0, 0.0]);
    }

    #[test]
    fn repo_file_key() {
        let root = crate::case::repo_root();
        let rel = "fixtures/offset.offset_executive_p2/data/issue1528_plate.smb";
        let v = expand(&root, serde_json::json!({ "path": { "$repo_file": rel } })).unwrap();
        assert_eq!(
            v["path"],
            Value::String(root.join(rel).to_string_lossy().into_owned())
        );
        for bad in [
            "fixtures/../Cargo.toml",
            "Cargo.toml",
            "fixtures/offset.offset_executive_p2/data/absent.smb",
        ] {
            assert!(
                expand(&root, serde_json::json!({ "$repo_file": bad })).is_err(),
                "{bad}"
            );
        }
    }
}
