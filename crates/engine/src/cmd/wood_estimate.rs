//! Derived workshop time and cost, using the cutting plan and evaluated exact geometry.
use super::CommandSpec;
use crate::params::bad;
use crate::schema::cutting_plan::Layout;
use crate::schema::wood_estimate::{DesignSupplies, Estimate, EstimateParams, Line, Rates};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::{BomType, FeatureKind};
use std::collections::BTreeMap;

const ID: &str = "wood.estimate";
const CONTRACT: TypedContract<EstimateParams> = TypedContract::new();
pub const DEFAULT_RATES: &str = include_str!("../../../../assets/wood/rates.json");

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("wood.estimate", "Time and Cost Estimate", crate::checked!("wood.estimate", estimate))
        .at("SOLID", "INSPECT").icon("bom").noundo().contract(&CONTRACT).output(output_schema_for::<Estimate>)
        .params("library?, stock?, kerf?: 3.2, trim?: 10: cutlist.layout settings; rates?: shared JSON path (default assets/wood/rates.json); glue_ups?: 0 explicit workshop batches; finishing?: [{name,quantity,unit_price}] overrides design metadata or stock-library supplies; path?: .csv. Returns costs, work_minutes, complete, warnings and breakdown lines. Work time counts saw cuts from the plan, router edge lengths and glue-ups. Labour is separate and omitted when labour_per_hour is null. Stock and supply prices use one chosen currency. No design changes."),
];

fn estimate(s: &mut Session, value: &Value) -> Result<Value> {
    let p: EstimateParams = serde_json::from_value(value.clone()).map_err(|e| bad(ID, e.to_string()))?;
    if p.glue_ups > 100_000 || p.path.as_ref().is_some_and(|path| !path.to_ascii_lowercase().ends_with(".csv")) {
        return Err(bad(ID, "glue_ups is limited to 100000; export path must end in .csv"));
    }
    let bytes = p.rates.as_deref().map(|path| solidcraft_io::vfs::read(path, 1 << 20).map_err(|e| bad(ID, e.to_string()))).transpose()?;
    let rates: Rates = serde_json::from_slice(bytes.as_deref().unwrap_or(DEFAULT_RATES.as_bytes())).map_err(|e| bad(ID, e.to_string()))?;
    let valid_rate = |v: f64| v.is_finite() && (0.0..=1e6).contains(&v);
    if ![rates.saw_cut_minutes, rates.router_minutes_per_metre, rates.glue_up_minutes].into_iter().all(valid_rate)
        || rates.labour_per_hour.is_some_and(|v| !valid_rate(v))
    {
        return Err(bad(ID, "time and labour rates must be finite, nonnegative and at most 1000000"));
    }
    let library = super::cutting_plan::library(p.library.as_deref())?;
    let design_supplies = if p.finishing.is_none() {
        s.doc
            .metadata
            .get("io.solidcraft.wood.estimate")
            .map(|v| serde_json::from_value::<DesignSupplies>(v.clone()).map_err(|e| bad(ID, e.to_string())))
            .transpose()?
    } else {
        None
    };
    let finishing = p.finishing.as_ref().or_else(|| design_supplies.as_ref().map(|s| &s.finishing)).unwrap_or(&library.finishing);
    if finishing.len() > 128
        || finishing.iter().any(|s| {
            s.name.trim().is_empty()
                || s.name.len() > 1024
                || [s.quantity, s.unit_price].into_iter().any(|v| !v.is_finite() || !(0.0..=1e12).contains(&v))
        })
    {
        return Err(bad(ID, "at most 128 named finishing supplies; quantities and prices must be finite, nonnegative and at most 1e12"));
    }
    let plan: Layout =
        serde_json::from_value(s.execute("cutlist.layout", &json!({"library":p.library,"stock":p.stock,"kerf":p.kerf,"trim":p.trim}))?)
            .map_err(|e| bad(ID, e.to_string()))?;
    let mut out = Estimate { complete: true, ..Estimate::default() };
    for purchase in &plan.to_buy {
        let cost = purchase.price.map(|v| v * purchase.count as f64);
        out.stock_cost += cost.unwrap_or(0.0);
        if cost.is_none() {
            out.warnings.push(format!("Missing stock price: {}", purchase.stock));
        }
        out.lines.push(Line {
            category: "material".into(),
            item: format!("{} / {}", purchase.stock, purchase.material),
            quantity: purchase.count as f64,
            unit_minutes: 0.0,
            minutes: 0.0,
            unit_cost: purchase.price,
            cost,
        });
    }
    for supply in finishing {
        let cost = supply.quantity * supply.unit_price;
        out.finishing_cost += cost;
        out.lines.push(Line {
            category: "finishing".into(),
            item: supply.name.clone(),
            quantity: supply.quantity,
            unit_minutes: 0.0,
            minutes: 0.0,
            unit_cost: Some(supply.unit_price),
            cost: Some(cost),
        });
    }
    for unplaced in &plan.unplaced {
        out.warnings.push(format!("Unplaced piece {}: {}", unplaced.mark, unplaced.reason));
    }
    for excluded in &plan.excluded {
        if !matches!(excluded.reason.as_str(), "purchased" | "reference") {
            out.warnings.push(format!("Excluded body {}: {}", excluded.body, excluded.reason));
        }
    }
    let cuts = plan.sheets.iter().map(|s| s.cuts.len()).sum::<usize>();
    out.lines.push(Line {
        category: "saw_cut".into(),
        item: "saw_cut".into(),
        quantity: cuts as f64,
        unit_minutes: rates.saw_cut_minutes,
        minutes: cuts as f64 * rates.saw_cut_minutes,
        unit_cost: None,
        cost: None,
    });

    // Count workshop operations once per physical occurrence, with the cut list's BOM exclusions.
    let mut counts = BTreeMap::<u64, u64>::new();
    let mut children = BTreeMap::<u64, Vec<u64>>::new();
    for occurrence in &s.doc.occurrences {
        children.entry(occurrence.parent).or_default().push(occurrence.component);
    }
    let types: BTreeMap<_, _> = s.doc.components.iter().map(|c| (c.id, c.metadata.bom_type())).collect();
    let mut stack = vec![(0, 0)];
    let mut visited = 0;
    while let Some((id, depth)) = stack.pop() {
        visited += 1;
        if depth > 64 || visited > 100_000 {
            return Err(bad(ID, "estimate exceeds 64 assembly levels or 100000 placements"));
        }
        if matches!(types.get(&id), Some(BomType::Purchased | BomType::Reference)) {
            continue;
        }
        *counts.entry(id).or_default() += 1;
        for child in children.get(&id).into_iter().flatten() {
            stack.push((*child, depth + 1));
        }
    }
    if s.doc.features.len() > 100_000 {
        return Err(bad(ID, "estimate exceeds 100000 features"));
    }
    for feature in &s.doc.features {
        if feature.suppressed {
            continue;
        }
        let Some(&copies) = counts.get(&feature.component) else { continue };
        // Router edge shapes take time by their exact routed length; `none` has no work.
        if !matches!(&feature.kind, FeatureKind::WoodEdgeProfile { profile, .. } if profile != "none") {
            continue;
        }
        let Some(result) = s.model.result(feature.id).filter(|r| !r.skipped && !r.stale && r.error.is_none()) else {
            out.warnings.push(format!("Feature has no current result: {}", feature.name));
            continue;
        };
        let lengths = result.state.routed_lengths.get(&feature.id).ok_or_else(|| bad(ID, "evaluated router lengths are missing"))?;
        let quantity = lengths.iter().sum::<f64>() / 1000.0 * copies as f64;
        out.lines.push(Line {
            category: "router".into(),
            item: format!("router: {}", feature.name),
            quantity,
            unit_minutes: rates.router_minutes_per_metre,
            minutes: quantity * rates.router_minutes_per_metre,
            unit_cost: None,
            cost: None,
        });
    }
    out.lines.push(Line {
        category: "glue_up".into(),
        item: "glue_up".into(),
        quantity: f64::from(p.glue_ups),
        unit_minutes: rates.glue_up_minutes,
        minutes: f64::from(p.glue_ups) * rates.glue_up_minutes,
        unit_cost: None,
        cost: None,
    });
    out.work_minutes = out.lines.iter().map(|l| l.minutes).sum();
    out.labour_cost = rates.labour_per_hour.map(|v| v * out.work_minutes / 60.0);
    if let Some(cost) = out.labour_cost {
        out.lines.push(Line {
            category: "labour".into(),
            item: "labour".into(),
            quantity: out.work_minutes / 60.0,
            unit_minutes: 0.0,
            minutes: 0.0,
            unit_cost: rates.labour_per_hour,
            cost: Some(cost),
        });
    }
    out.material_cost = out.stock_cost + out.finishing_cost;
    out.total_cost = out.material_cost + out.labour_cost.unwrap_or(0.0);
    out.complete = out.warnings.is_empty();
    if ![out.total_cost, out.work_minutes].into_iter().all(f64::is_finite) {
        return Err(bad(ID, "estimate totals exceed numeric limits"));
    }
    if let Some(path) = p.path {
        let mut csv = String::from("category,item,quantity,unit_minutes,minutes,unit_cost,cost\n");
        for line in &out.lines {
            let optional = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
            csv.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                solidcraft_doc::csv::quoted(&line.category),
                solidcraft_doc::csv::quoted(&line.item),
                line.quantity,
                line.unit_minutes,
                line.minutes,
                optional(line.unit_cost),
                optional(line.cost)
            ));
        }
        for (item, minutes, cost) in [
            ("stock_cost", 0.0, Some(out.stock_cost)),
            ("material_cost", 0.0, Some(out.material_cost)),
            ("finishing_cost", 0.0, Some(out.finishing_cost)),
            ("labour_cost", 0.0, out.labour_cost),
            ("total", out.work_minutes, Some(out.total_cost)),
        ] {
            csv.push_str(&format!("total,{item},,,{},,{}\n", minutes, cost.map(|v| v.to_string()).unwrap_or_default()));
        }
        for warning in &out.warnings {
            csv.push_str(&format!("warning,{},,,,,\n", solidcraft_doc::csv::quoted(warning)));
        }
        solidcraft_io::vfs::check(&path).map_err(|e| EngineError::Other(e.to_string()))?;
        #[cfg(not(target_arch = "wasm32"))]
        solidcraft_io::safe_write::write_atomic(std::path::Path::new(&path), csv.as_bytes(), false).map_err(|e| EngineError::Other(e.to_string()))?;
        #[cfg(target_arch = "wasm32")]
        solidcraft_io::vfs::write(&path, csv.as_bytes()).map_err(|e| EngineError::Other(e.to_string()))?;
    }
    serde_json::to_value(out).map_err(|e| bad(ID, e.to_string()))
}

#[cfg(test)]
#[path = "wood_estimate_tests.rs"]
mod tests;
