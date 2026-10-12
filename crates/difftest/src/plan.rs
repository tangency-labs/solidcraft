//! Reads unit ids/statuses from `plan/modules.toml` (format: `plan/modules.schema.md`).

use std::fs;
use std::path::Path;

use crate::HarnessError;

/// One `[[unit]]` entry (only the fields difftest needs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanUnit {
    /// Unit id.
    pub id: String,
    /// One of [`STATUSES`].
    pub status: String,
}

/// Valid `status` values (`plan/modules.schema.md`). `partial` = every
/// non-deferred difftest case passes and at least one is deferred (AGENTS.md 4a).
pub const STATUSES: &[&str] = &[
    "todo",
    "claimed",
    "partial",
    "green",
    "blocked",
    "out_of_scope",
];

/// Parses `<root>/plan/modules.toml`.
pub fn load(root: &Path) -> Result<Vec<PlanUnit>, HarnessError> {
    let path = root.join("plan").join("modules.toml");
    let text = fs::read_to_string(&path)
        .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
    parse(&text).map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))
}

/// Parses modules.toml text.
pub fn parse(text: &str) -> Result<Vec<PlanUnit>, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let units = v
        .get("unit")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, u) in units.iter().enumerate() {
        let id = u
            .get("id")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("unit #{i}: missing id"))?;
        let status = u
            .get("status")
            .and_then(toml::Value::as_str)
            .unwrap_or("todo");
        if !STATUSES.contains(&status) {
            return Err(format!(
                "unit {id}: unknown status `{status}` (expected one of {})",
                STATUSES.join(", ")
            ));
        }
        out.push(PlanUnit {
            id: id.to_string(),
            status: status.to_string(),
        });
    }
    Ok(out)
}

/// Ids of units with `status = "green"` or `"partial"` (what `--green` runs), in
/// file order.
pub fn green_units(units: &[PlanUnit]) -> Vec<String> {
    units
        .iter()
        .filter(|u| u.status == "green" || u.status == "partial")
        .map(|u| u.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_green() {
        let t =
            "[[unit]]\nid = \"a\"\nstatus = \"green\"\n[[unit]]\nid = \"b\"\nstatus = \"todo\"\n\
                 [[unit]]\nid = \"c\"\nstatus = \"partial\"\n";
        let u = parse(t).unwrap();
        assert_eq!(green_units(&u), vec!["a".to_string(), "c".to_string()]);
        assert!(parse("[[unit]]\nid = \"x\"\nstatus = \"done\"\n").is_err());
    }
}
