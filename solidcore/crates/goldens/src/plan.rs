//! Reads unit ids/statuses from `plan/units/<id>.toml`, one file per unit (format:
//! `plan/modules.schema.md`; the Python twin is `harness/plan/units.py`).

use std::fs;
use std::path::Path;

use crate::HarnessError;

/// One unit file (only the fields the runners need).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanUnit {
    /// Unit id.
    pub id: String,
    /// One of [`STATUSES`].
    pub status: String,
}

/// Valid `status` values (`plan/modules.schema.md`).
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
    let status = match v.get("status") {
        None => "todo",
        Some(value) => value.as_str().ok_or_else(|| {
            format!(
                "unit {id}: status must be a string, found {}",
                value.type_str()
            )
        })?,
    };
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
        let statuses: Vec<(&str, &str)> = u
            .iter()
            .map(|u| (u.id.as_str(), u.status.as_str()))
            .collect();
        assert_eq!(statuses, [("a", "green"), ("b", "todo"), ("c", "partial")]);
        assert!(parse_unit("id = \"x\"\nstatus = \"done\"\n").is_err());
        assert!(parse_unit("status = \"green\"\n").is_err());
    }

    #[test]
    fn rejects_non_string_statuses() {
        for (status, kind) in [
            ("[\"green\"]", "array"),
            ("1", "integer"),
            ("true", "boolean"),
            ("{ value = \"green\" }", "table"),
        ] {
            let err =
                parse_unit(&format!("id = \"malformed.unit\"\nstatus = {status}\n")).unwrap_err();
            assert_eq!(
                err,
                format!("unit malformed.unit: status must be a string, found {kind}")
            );
        }
    }

    #[test]
    fn accepts_all_string_statuses() {
        for status in STATUSES {
            let unit = parse_unit(&format!("id = \"x\"\nstatus = \"{status}\"\n")).unwrap();
            assert_eq!(unit.id, "x");
            assert_eq!(unit.status, *status);
        }
    }

    #[test]
    fn missing_status_defaults_to_todo() {
        let unit = parse_unit("id = \"x\"\n").unwrap();
        assert_eq!(unit.status, "todo");
    }

    #[test]
    fn load_rejects_non_string_status_instead_of_omitting_unit() {
        let dir = std::env::temp_dir().join(format!(
            "goldens-plan-malformed-status-{}",
            std::process::id()
        ));
        let units = dir.join("plan").join("units");
        fs::create_dir_all(&units).unwrap();
        fs::write(units.join("a.toml"), "id = \"a\"\nstatus = \"green\"\n").unwrap();
        fs::write(units.join("b.toml"), "id = \"b\"\nstatus = [\"green\"]\n").unwrap();
        let err = load(&dir).unwrap_err().to_string();
        assert!(err.contains("b.toml"));
        assert!(err.contains("unit b: status must be a string, found array"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn loads_unit_dir_and_checks_file_names() {
        let dir = std::env::temp_dir().join(format!("goldens-plan-{}", std::process::id()));
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
        let ids: Vec<&str> = got.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, ["a.x", "b.y"]);
        fs::write(units.join("c.toml"), "id = \"other\"\n").unwrap();
        assert!(load(&dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
