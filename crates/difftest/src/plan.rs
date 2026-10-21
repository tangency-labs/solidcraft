//! Reads unit ids/statuses from `plan/units/<id>.toml`, one file per unit (format:
//! `plan/modules.schema.md`; the Python twin is `harness/plan/units.py`).

use std::fs;
use std::path::Path;

use crate::HarnessError;

/// One unit file (only the fields difftest needs).
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

/// Loads every `<root>/plan/units/*.toml`, sorted by id. Each file's `id` must equal
/// its file name.
pub fn load(root: &Path) -> Result<Vec<PlanUnit>, HarnessError> {
    let dir = root.join("plan").join("units");
    let rd =
        fs::read_dir(&dir).map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let err = |e: String| HarnessError::new(format!("{}: {e}", path.display()));
        let text = fs::read_to_string(&path).map_err(|e| err(e.to_string()))?;
        let unit = parse_unit(&text).map_err(err)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if unit.id != stem {
            return Err(err(format!(
                "id `{}` does not match the file name",
                unit.id
            )));
        }
        out.push(unit);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Parses the text of one unit file (top-level `id`, `status`, ...).
pub fn parse_unit(text: &str) -> Result<PlanUnit, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let id = v
        .get("id")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "missing id".to_string())?;
    let status = v
        .get("status")
        .and_then(toml::Value::as_str)
        .unwrap_or("todo");
    if !STATUSES.contains(&status) {
        return Err(format!(
            "unit {id}: unknown status `{status}` (expected one of {})",
            STATUSES.join(", ")
        ));
    }
    Ok(PlanUnit {
        id: id.to_string(),
        status: status.to_string(),
    })
}

/// Ids of units with `status = "green"` or `"partial"` (what `--green` runs), in
/// the order of `units` (by id, as [`load`] returns them).
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
        let u: Vec<PlanUnit> = [
            "id = \"a\"\nstatus = \"green\"\n",
            "id = \"b\"\nstatus = \"todo\"\n",
            "id = \"c\"\nstatus = \"partial\"\n# trailing comment\n",
        ]
        .iter()
        .map(|t| parse_unit(t).unwrap())
        .collect();
        assert_eq!(green_units(&u), vec!["a".to_string(), "c".to_string()]);
        assert!(parse_unit("id = \"x\"\nstatus = \"done\"\n").is_err());
        assert!(parse_unit("status = \"green\"\n").is_err());
    }

    #[test]
    fn loads_unit_dir_and_checks_file_names() {
        let dir = std::env::temp_dir().join(format!("difftest-plan-{}", std::process::id()));
        let units = dir.join("plan").join("units");
        fs::create_dir_all(&units).unwrap();
        fs::write(units.join("b.y.toml"), "id = \"b.y\"\nstatus = \"green\"\n").unwrap();
        fs::write(
            units.join("a.x.toml"),
            "id = \"a.x\"\nstatus = \"partial\"\n",
        )
        .unwrap();
        fs::write(units.join("README"), "not a unit").unwrap();
        let got = load(&dir).unwrap();
        assert_eq!(
            green_units(&got),
            vec!["a.x".to_string(), "b.y".to_string()]
        );
        fs::write(units.join("c.toml"), "id = \"other\"\n").unwrap();
        assert!(load(&dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
