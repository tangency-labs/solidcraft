//! Lookup tables of a generator package (`tables/<name>.csv`, docs/formats/generator.md, section 5):
//! a header row and rows of cells, the first column being the key. A cell is a whole number, a
//! decimal or text; an empty cell is null. Everything is bounded.

use std::collections::BTreeMap;

use serde_json::Value as Json;

/// A table file, in bytes.
pub const MAX_TABLE_BYTES: usize = 256 * 1024;
pub const MAX_TABLE_ROWS: usize = 5000;
pub const MAX_TABLE_COLUMNS: usize = 64;
const MAX_CELL_CHARS: usize = 512;

#[derive(Clone, Debug)]
pub struct Table {
    pub columns: Vec<String>,
    rows: Vec<Vec<Json>>,
    index: BTreeMap<String, usize>,
}

/// Split CSV text into records. A cell may be quoted with `"` (`""` is a quote).
fn records(csv: &str) -> Result<Vec<Vec<String>>, String> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = csv.chars().peekable();
    while let Some(c) = chars.next() {
        if cell.chars().count() > MAX_CELL_CHARS {
            return Err(format!("a cell has more than {MAX_CELL_CHARS} characters"));
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
                if row.len() > MAX_TABLE_COLUMNS {
                    return Err(format!("more than {MAX_TABLE_COLUMNS} columns"));
                }
                if row.len() == 1 && row.first().is_some_and(|c| c.trim().is_empty()) {
                    row.clear();
                } else {
                    out.push(std::mem::take(&mut row));
                }
                if out.len() > MAX_TABLE_ROWS + 1 {
                    return Err(format!("more than {MAX_TABLE_ROWS} rows"));
                }
            }
            other => cell.push(other),
        }
    }
    if quoted {
        return Err("a quoted cell is not closed".into());
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        out.push(row);
    }
    if out.len() > MAX_TABLE_ROWS + 1 {
        return Err(format!("more than {MAX_TABLE_ROWS} rows"));
    }
    Ok(out)
}

fn typed(raw: &str) -> Json {
    let t = raw.trim();
    if t.is_empty() {
        return Json::Null;
    }
    let plain_int = t.strip_prefix('-').unwrap_or(t);
    if !plain_int.is_empty() && plain_int.chars().all(|c| c.is_ascii_digit()) && (plain_int == "0" || !plain_int.starts_with('0')) {
        if let Ok(n) = t.parse::<i64>() {
            return Json::from(n);
        }
    } else if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E' | '+'))
        && t.chars().any(|c| c.is_ascii_digit())
        && !t.starts_with('+')
        && let Ok(x) = t.parse::<f64>()
        && x.is_finite()
    {
        return Json::from(x);
    }
    Json::String(t.to_string())
}

/// A key cell as text: a string as it is, a number as written plainly.
pub fn key_text(v: &Json) -> Option<String> {
    match v {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Table {
    pub fn parse(csv: &str) -> Result<Table, String> {
        if csv.len() > MAX_TABLE_BYTES {
            return Err(format!("larger than {} KiB", MAX_TABLE_BYTES / 1024));
        }
        let mut recs = records(csv)?.into_iter();
        let Some(header) = recs.next() else { return Err("it has no header row".into()) };
        let columns: Vec<String> = header.iter().map(|h| h.trim().to_string()).collect();
        for (i, c) in columns.iter().enumerate() {
            if !super::code_template::is_name(c) {
                return Err(format!("`{c}` is not a plain column name"));
            }
            if columns.iter().take(i).any(|p| p == c) {
                return Err(format!("the column `{c}` is repeated"));
            }
        }
        let mut rows = Vec::new();
        let mut index = BTreeMap::new();
        for (i, rec) in recs.enumerate() {
            let line = i + 2;
            if rec.len() != columns.len() {
                return Err(format!("row {line} has {} cells, the header has {}", rec.len(), columns.len()));
            }
            let cells: Vec<Json> = rec.iter().map(|c| typed(c)).collect();
            let Some(key) = cells.first().and_then(key_text) else { return Err(format!("row {line}: the key cell is empty")) };
            if index.insert(key.to_lowercase(), rows.len()).is_some() {
                return Err(format!("row {line}: the key `{key}` is repeated"));
            }
            rows.push(cells);
        }
        Ok(Table { columns, rows, index })
    }

    /// The row for a key (case ignored), as its columns' values.
    pub fn row(&self, key: &str) -> Option<Vec<(&str, &Json)>> {
        let cells = self.rows.get(*self.index.get(&key.to_lowercase())?)?;
        Some(self.columns.iter().map(String::as_str).zip(cells.iter()).collect())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}
