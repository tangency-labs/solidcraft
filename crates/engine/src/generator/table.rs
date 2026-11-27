//! Parameter tables (docs/formats/generator-package.md, section 3): CSV text with typed columns
//! and optional provenance columns, loaded into rows with a unique key.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::expr::Value;
use super::{GeneratorError, MAX_COLUMNS, MAX_LIST_ITEMS, MAX_POINTS, MAX_ROWS, MAX_TEXT_CHARS, is_name};

/// A table as the package writes it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableDef {
    pub columns: BTreeMap<String, String>,
    pub key: Vec<String>,
    pub csv: String,
}

/// The provenance columns every table may have.
pub const STATUSES: [&str; 4] = ["published", "measured", "derived", "approximate"];

#[derive(Clone, Debug)]
pub struct Table {
    pub rows: Vec<BTreeMap<String, Value>>,
    key: Vec<String>,
    index: BTreeMap<Vec<String>, usize>,
}

fn bad<T>(table: &str, msg: impl Into<String>) -> Result<T, GeneratorError> {
    Err(GeneratorError::Package(format!("table `{table}`: {}", msg.into())))
}

/// Split CSV text into records. A cell may be quoted with `"` (`""` is a quote).
fn records(table: &str, csv: &str) -> Result<Vec<Vec<String>>, GeneratorError> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = csv.chars().peekable();
    while let Some(c) = chars.next() {
        if cell.chars().count() > MAX_TEXT_CHARS * 8 {
            return Err(GeneratorError::Limit { what: "characters in a table cell", limit: MAX_TEXT_CHARS * 8 });
        }
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut cell)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut cell));
                if row.len() > MAX_COLUMNS {
                    return Err(GeneratorError::Limit { what: "table columns", limit: MAX_COLUMNS });
                }
                if !(row.len() == 1 && row.first().is_some_and(|c| c.trim().is_empty())) {
                    out.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                if out.len() > MAX_ROWS + 1 {
                    return Err(GeneratorError::Limit { what: "table rows", limit: MAX_ROWS });
                }
            }
            other => cell.push(other),
        }
    }
    if quoted {
        return bad(table, "a quoted cell is not closed");
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        out.push(row);
    }
    if out.len() > MAX_ROWS + 1 {
        return Err(GeneratorError::Limit { what: "table rows", limit: MAX_ROWS });
    }
    Ok(out)
}

fn typed(table: &str, row: usize, column: &str, ty: &str, raw: &str) -> Result<Value, GeneratorError> {
    let at = |why: String| -> Result<Value, GeneratorError> { bad(table, format!("row {row}, column `{column}`: {why}")) };
    let raw = raw.trim();
    if ty == "text" && raw.chars().count() > MAX_TEXT_CHARS {
        return Err(GeneratorError::Limit { what: "characters in a table cell", limit: MAX_TEXT_CHARS });
    }
    match ty {
        "text" => Ok(Value::Text(raw.to_string())),
        "number" => match raw.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Value::Num(n)),
            _ => at(format!("`{raw}` is not a finite number")),
        },
        "list" => {
            let items: Vec<Value> = raw.split_whitespace().map(|s| Value::Text(s.to_string())).collect();
            if items.len() > MAX_LIST_ITEMS {
                return Err(GeneratorError::Limit { what: "items in a table list", limit: MAX_LIST_ITEMS });
            }
            Ok(Value::List(items))
        }
        "points" => {
            let mut points = Vec::new();
            for pair in raw.split(';').filter(|p| !p.trim().is_empty()) {
                if points.len() >= MAX_POINTS {
                    return Err(GeneratorError::Limit { what: "points in a table cell", limit: MAX_POINTS });
                }
                let nums: Vec<f64> = pair.split_whitespace().filter_map(|n| n.parse::<f64>().ok().filter(|x| x.is_finite())).collect();
                if nums.len() != 2 || pair.split_whitespace().count() != 2 {
                    return at(format!("`{}` is not an `x y` pair", pair.trim()));
                }
                let (Some(x), Some(y)) = (nums.first(), nums.get(1)) else { return at("a point needs x and y".into()) };
                points.push([*x, *y]);
            }
            Ok(Value::Points(points))
        }
        other => bad(table, format!("column `{column}` has the unknown type `{other}`")),
    }
}

impl Table {
    pub fn load(name: &str, def: &TableDef) -> Result<Table, GeneratorError> {
        if def.columns.len() > MAX_COLUMNS {
            return Err(GeneratorError::Limit { what: "table columns", limit: MAX_COLUMNS });
        }
        for column in def.columns.keys() {
            if !is_name(column) {
                return bad(name, format!("`{column}` is not a plain column name"));
            }
        }
        for k in &def.key {
            if !def.columns.contains_key(k) {
                return bad(name, format!("the key column `{k}` is not a column"));
            }
        }
        if def.key.is_empty() {
            return bad(name, "`key` must name at least one column");
        }
        let mut recs = records(name, &def.csv)?.into_iter();
        let Some(header) = recs.next() else { return bad(name, "the CSV has no header row") };
        let header: Vec<String> = header.iter().map(|h| h.trim().to_string()).collect();
        for h in &header {
            if !def.columns.contains_key(h) {
                return bad(name, format!("the header names `{h}`, which `columns` does not declare"));
            }
        }
        for c in def.columns.keys() {
            if !header.contains(c) {
                return bad(name, format!("column `{c}` is declared but the header does not have it"));
            }
        }
        let mut rows = Vec::new();
        let mut index = BTreeMap::new();
        for (i, rec) in recs.enumerate() {
            let line = i + 2;
            if rec.len() != header.len() {
                return bad(name, format!("row {line} has {} cells, the header has {}", rec.len(), header.len()));
            }
            let mut row = BTreeMap::new();
            for (h, cell) in header.iter().zip(&rec) {
                let ty = def.columns.get(h).map_or("text", String::as_str);
                row.insert(h.clone(), typed(name, line, h, ty, cell)?);
            }
            if let Some(Value::Text(status)) = row.get("status")
                && !status.is_empty()
                && !STATUSES.contains(&status.as_str())
            {
                return bad(name, format!("row {line}: status `{status}` is not one of published, measured, derived, approximate"));
            }
            let key: Vec<String> = def.key.iter().filter_map(|k| row.get(k).and_then(Value::key_string)).collect();
            if key.len() != def.key.len() {
                return bad(name, format!("row {line}: a key column has no usable value"));
            }
            if index.insert(key.clone(), rows.len()).is_some() {
                return bad(name, format!("row {line}: the key `{}` is repeated", key.join(" ")));
            }
            rows.push(row);
        }
        Ok(Table { rows, key: def.key.clone(), index })
    }

    /// The row for a key, in the order of the table's `key` columns.
    pub fn find(&self, key: &[String]) -> Option<&BTreeMap<String, Value>> {
        self.index.get(key).and_then(|i| self.rows.get(*i))
    }

    pub fn key_columns(&self) -> &[String] {
        &self.key
    }
}
