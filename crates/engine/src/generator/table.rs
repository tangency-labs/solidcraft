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
pub const STATUSES: [&str; 5] = ["published", "labelled", "measured", "derived", "approximate"];

/// One row: its values by column, and what it says about where they come from.
#[derive(Clone, Debug)]
pub struct Row {
    pub values: BTreeMap<String, Value>,
    /// The status that holds for the columns the row does not list (`published`, `labelled`,
    /// `measured`, `derived` or `approximate`), if it names one.
    pub status: Option<String>,
    /// The columns with a status of their own.
    pub column_status: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub rows: Vec<Row>,
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
        "number?" if raw.is_empty() => Ok(Value::None),
        "number?" => match raw.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Value::Num(n)),
            _ => at(format!("`{raw}` is not a finite number (or leave the cell empty)")),
        },
        "numbers" => {
            let mut items = Vec::new();
            for n in raw.split_whitespace() {
                if items.len() >= MAX_LIST_ITEMS {
                    return Err(GeneratorError::Limit { what: "items in a table list", limit: MAX_LIST_ITEMS });
                }
                match n.parse::<f64>() {
                    Ok(x) if x.is_finite() => items.push(Value::Num(x)),
                    _ => return at(format!("`{n}` is not a finite number")),
                }
            }
            Ok(Value::List(items))
        }
        "tuples" => {
            let mut groups = Vec::new();
            for group in raw.split(';').filter(|g| !g.trim().is_empty()) {
                if groups.len() >= MAX_LIST_ITEMS {
                    return Err(GeneratorError::Limit { what: "items in a table list", limit: MAX_LIST_ITEMS });
                }
                let mut nums = Vec::new();
                for n in group.split_whitespace() {
                    match n.parse::<f64>() {
                        Ok(x) if x.is_finite() && nums.len() < 16 => nums.push(Value::Num(x)),
                        _ => return at(format!("`{}` is not a group of 1 to 16 finite numbers", group.trim())),
                    }
                }
                groups.push(Value::List(nums));
            }
            Ok(Value::List(groups))
        }
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

/// A row's `status` cell: one status for the whole row (`measured`), or a status for some columns
/// and one for the rest: `measured:d0,floor_half;approximate:tip_round;published`.
fn parse_status(
    table: &str,
    row: usize,
    raw: &str,
    columns: &BTreeMap<String, String>,
) -> Result<(Option<String>, BTreeMap<String, String>), GeneratorError> {
    let mut default = None;
    let mut listed = BTreeMap::new();
    for part in raw.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (status, cols) = match part.split_once(':') {
            Some((s, c)) => (s.trim(), Some(c)),
            None => (part, None),
        };
        if !STATUSES.contains(&status) {
            return bad(table, format!("row {row}: status `{status}` is not one of published, labelled, measured, derived, approximate"));
        }
        match cols {
            None => default = Some(status.to_string()),
            Some(list) => {
                for col in list.split(',').map(str::trim).filter(|c| !c.is_empty()) {
                    if !columns.contains_key(col) {
                        return bad(table, format!("row {row}: status names the column `{col}`, which the table does not have"));
                    }
                    listed.insert(col.to_string(), status.to_string());
                }
            }
        }
    }
    Ok((default, listed))
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
            let (status, column_status) = match row.get("status") {
                Some(Value::Text(raw)) => parse_status(name, line, raw, &def.columns)?,
                _ => (None, BTreeMap::new()),
            };
            let key: Vec<String> = def.key.iter().filter_map(|k| row.get(k).and_then(Value::key_string)).collect();
            if key.len() != def.key.len() {
                return bad(name, format!("row {line}: a key column has no usable value"));
            }
            if index.insert(key.clone(), rows.len()).is_some() {
                return bad(name, format!("row {line}: the key `{}` is repeated", key.join(" ")));
            }
            rows.push(Row { values: row, status, column_status });
        }
        Ok(Table { rows, key: def.key.clone(), index })
    }

    /// The row for a key, in the order of the table's `key` columns.
    pub fn find(&self, key: &[String]) -> Option<&Row> {
        self.index.get(key).and_then(|i| self.rows.get(*i))
    }

    pub fn key_columns(&self) -> &[String] {
        &self.key
    }
}
