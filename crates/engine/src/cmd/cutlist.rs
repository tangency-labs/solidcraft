//! Stock cut parts, counted independently of their assembly poses.

use std::collections::BTreeMap;

use serde_json::Value;
use solidcraft_doc::BomType;
use solidcraft_geom::Vec3Geom as _;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::cutlist::{Component, CutList, CutListParams, Excluded, Format, GroupBy, Include, MaterialTotal, Row, Stock};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ID: &str = "cutlist.parts";
const CONTRACT: TypedContract<CutListParams> = TypedContract::new();
const MAX_PLACEMENTS: usize = 100_000;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cutlist.parts", "Cut List", crate::checked!("cutlist.parts", parts))
        .at("SOLID", "INSPECT")
        .icon("bom")
        .noundo()
        .contract(&CONTRACT)
        .output(output_schema_for::<CutList>)
        .params("include?: bodies (default, including loose bodies)|components; component?: id or name; group_by?: [material, stock]; path?: .csv or .json; format?: csv|json. Returns rows: [{item, name, count, material, stock: {kind, length, width, thickness, exact}, grain, bodies}], excluded, totals, total_occurrences. All sizes in mm to six decimal places. Grain is empty until assigned; equal stock with different grain stays separate. Stock follows planar faces and straight edges; curved stock is approximate. Purchased and reference subtrees never count; phantom and inseparable contents count."),
];

pub(crate) fn csv(rows: &[Row]) -> String {
    let quote = solidcraft_doc::csv::quoted;
    let mut out = ["item", "name", "count", "material", "length", "width", "thickness", "grain", "exact"].map(quote).join(",");
    out.push('\n');
    for r in rows {
        let fields = [
            r.item.to_string(),
            r.name.clone(),
            r.count.to_string(),
            r.material.clone(),
            r.stock.length.to_string(),
            r.stock.width.to_string(),
            r.stock.thickness.to_string(),
            r.grain.clone(),
            r.stock.exact.to_string(),
        ];
        out.push_str(&fields.iter().map(|s| quote(s)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

pub(super) fn parts(s: &mut Session, p: &Value) -> Result<Value> {
    let p: CutListParams = serde_json::from_value(p.clone()).map_err(|e| bad(ID, e.to_string()))?;
    if let Some(group) = &p.group_by
        && !(group.len() == 2 && group.iter().any(|g| matches!(g, GroupBy::Material)) && group.iter().any(|g| matches!(g, GroupBy::Stock)))
    {
        return Err(bad(ID, "group_by must contain material and stock"));
    }
    let root = match p.component {
        None => 0,
        Some(key) => {
            let key = match key {
                Component::Id(id) => id.to_string(),
                Component::Name(n) => n,
            };
            s.doc.resolve_component(&key)?.ok_or_else(|| bad(ID, format!("no component `{key}`")))?
        }
    };
    let mut counts: BTreeMap<u64, u64> = BTreeMap::new();
    let mut excluded: BTreeMap<u64, &'static str> = BTreeMap::new();
    let mut children: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for o in &s.doc.occurrences {
        children.entry(o.parent).or_default().push(o.component);
    }
    let types: BTreeMap<_, _> = s.doc.components.iter().map(|c| (c.id, c.metadata.bom_type())).collect();
    let mut stack = vec![(root, 0usize, None)];
    let mut visited = 0;
    while let Some((component, depth, inherited)) = stack.pop() {
        visited += 1;
        if depth > 64 || visited > MAX_PLACEMENTS {
            return Err(bad(ID, "cut list exceeds 64 assembly levels or 100000 placements"));
        }
        let reason = inherited.or(match types.get(&component).copied().unwrap_or_default() {
            BomType::Purchased => Some("purchased"),
            BomType::Reference => Some("reference"),
            _ => None,
        });
        if let Some(reason) = reason {
            excluded.insert(component, reason);
        } else {
            *counts.entry(component).or_default() += 1;
        }
        for child in children.get(&component).into_iter().flatten() {
            stack.push((*child, depth + 1, reason));
        }
    }
    let state = s.model.state();
    if state.bodies.len() > 10_000 {
        return Err(bad(ID, "cut list exceeds 10000 body definitions"));
    }
    let owners = s.doc.model_body_components(&state);
    let mut body_counts = BTreeMap::new();
    for owner in &owners {
        *body_counts.entry(*owner).or_insert(0usize) += 1;
    }
    let mut out = CutList::default();
    let mut grouped: BTreeMap<(String, [String; 3], String), (Row, BTreeMap<String, u64>)> = BTreeMap::new();
    for (body, owner) in state.bodies.iter().zip(owners) {
        let Some(&count) = counts.get(&owner) else {
            if let Some(reason) = excluded.get(&owner) {
                out.excluded.push(Excluded { body: body.name.clone(), reason: (*reason).into() });
            }
            continue;
        };
        if owner == 0 && matches!(p.include, Include::Components) {
            continue;
        }
        if body.body.is_mesh() || body.is_surface() {
            out.excluded.push(Excluded { body: body.name.clone(), reason: "stock size requires a solid B-rep body".into() });
            continue;
        }
        // One body that cannot be measured is listed as left out; it never fails the whole list.
        let size = match crate::kernel::stock_box(&body.body) {
            Ok(size) => size,
            Err(e) => {
                out.excluded.push(Excluded { body: body.name.clone(), reason: format!("stock size could not be measured: {e}") });
                continue;
            }
        };
        // Export six decimal places in mm, removing rigid-transform roundoff.
        let [length, width, thickness] = size.dimensions.map(|d| (d * 1e6).round() / 1e6);
        let component = s.doc.components.iter().find(|c| c.id == owner);
        let name = component.filter(|_| body_counts.get(&owner) == Some(&1)).map(|c| c.name.clone()).unwrap_or_else(|| body.name.clone());
        let material = s
            .doc
            .materials
            .get(body.definition_name())
            .map(|_| s.doc.body_material(body.definition_name()))
            .or_else(|| component.and_then(|c| c.properties.get("material")).cloned())
            .unwrap_or_default();
        let grain = match s.doc.grain_direction(body)? {
            None => String::new(),
            Some(direction) => size
                .axes
                .iter()
                .zip(["length", "width", "thickness"])
                .find(|(axis, _)| solidcore_record::Vec3::from(**axis).dot(direction).abs() > 1.0 - 1e-8)
                .map(|(_, name)| name)
                .unwrap_or("oblique")
                .to_string(),
        };
        let key = (material.clone(), [length, width, thickness].map(|d| format!("{d:.6}")), grain.clone());
        let (row, names) = grouped.entry(key).or_insert_with(|| {
            let row = Row {
                item: 0,
                name: String::new(),
                count: 0,
                material,
                stock: Stock { kind: size.kind.into(), length, width, thickness, exact: size.exact },
                grain,
                bodies: Vec::new(),
            };
            (row, BTreeMap::<String, u64>::new())
        });
        *names.entry(name).or_default() += count;
        row.count += count;
        row.stock.exact &= size.exact;
        row.bodies.push(body.name.clone());
    }
    // A line is one piece cut `count` times; different parts that share a material, stock size and grain
    // all stay named on it ("; " between them, a count after a name more than one piece holds).
    out.rows = grouped
        .into_values()
        .map(|(mut row, names)| {
            row.name = if names.len() == 1 {
                names.into_keys().next().unwrap_or_default()
            } else {
                names.into_iter().map(|(n, k)| if k == 1 { n } else { format!("{n} ({k})") }).collect::<Vec<_>>().join("; ")
            };
            row
        })
        .collect();
    out.rows.sort_by(|a, b| (&a.material, &a.name).cmp(&(&b.material, &b.name)));
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    for (i, row) in out.rows.iter_mut().enumerate() {
        row.item = i + 1;
        row.bodies.sort();
        *totals.entry(row.material.clone()).or_default() += row.count;
        out.total_occurrences += row.count;
    }
    out.totals = totals.into_iter().map(|(material, count)| MaterialTotal { material, count }).collect();
    out.excluded.sort_by(|a, b| a.body.cmp(&b.body));
    if let Some(path) = p.path {
        let format = match p.format {
            Some(Format::Csv) => "csv",
            Some(Format::Json) => "json",
            None if path.to_ascii_lowercase().ends_with(".csv") => "csv",
            None if path.to_ascii_lowercase().ends_with(".json") => "json",
            None => return Err(bad(ID, "path must end in .csv or .json, or give format")),
        };
        let text = if format == "csv" { csv(&out.rows) } else { serde_json::to_string_pretty(&out).map_err(|e| bad(ID, e.to_string()))? };
        solidcraft_io::vfs::write(&path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        out.path = Some(path);
        out.format = Some(format.into());
    }
    serde_json::to_value(out).map_err(|e| bad(ID, e.to_string()))
}

#[cfg(test)]
#[path = "cutlist_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
