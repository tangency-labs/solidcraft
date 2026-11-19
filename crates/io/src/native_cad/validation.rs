//! The file's own figures for its parts, checked against the bodies imported: a STEP file can
//! state each part's volume, surface area and centre (the CAx-IF geometric validation properties),
//! which cadmpeg reads into its neutral model's declared measures. Each one the bodies can be
//! measured for is compared with what they measure, and a part whose bodies differ from what its
//! file says is named in a warning, with both figures: a face left out, a hole not cut or a part
//! placed wrong shows there, where the import would otherwise look whole.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::declared_measures::{DeclaredMeasureProvenance, DeclaredQuantity, DeclaredTarget};
use cadmpeg_ir::document::CadIr;
use solidcraft_kernel::{ImportRecords, StepImport};

/// How far a volume or an area may be from the file's figure, as a fraction of it, beyond the
/// file's own stated error: validation properties are written to a few significant figures, and
/// SMLib integrates to 1e-8.
const RELATIVE: f64 = 1e-3;

/// Where a record body came from: its body in the neutral model, and the world placement of the
/// occurrence that placed it (the identity outside an assembly), so a centre the file states in its
/// part's coordinates is compared there.
#[derive(Clone)]
pub(super) struct Origin {
    pub body: String,
    pub world: [[f64; 4]; 4],
}

/// One record body as built and measured.
struct Measured {
    volume: f64,
    area: f64,
    shells: usize,
    /// Centre in the part's coordinates.
    centroid: [f64; 3],
    sheet: bool,
}

/// A warning for each validation property `ir` states that the bodies of `imp` (built from
/// `records`, whose bodies came from `origins`) do not match, and one for those that cannot be
/// checked because a body they cover was not imported.
pub(super) fn check(ir: &CadIr, records: &ImportRecords, origins: &[Origin], imp: &StepImport) -> Vec<String> {
    let stated: Vec<_> = ir.model.declared_measures.iter().filter(|m| m.provenance == DeclaredMeasureProvenance::ValidationProperty).collect();
    if stated.is_empty() {
        return Vec::new();
    }
    // Each original record body (not a copy) by its neutral-model body: copies are the same part
    // placed again, so a part is measured once.
    let mut by_body: HashMap<&str, Vec<usize>> = HashMap::new();
    for (k, (r, o)) in records.bodies.iter().zip(origins).enumerate() {
        if r.copy_of.is_none() {
            by_body.entry(o.body.as_str()).or_default().push(k);
        }
    }
    // The body each original record became (bodies a record made carry its index).
    let mut built: HashMap<usize, &solidcraft_kernel::ImportedBody> = HashMap::new();
    for b in &imp.bodies {
        built.entry(b.source_item as usize).or_insert(b);
    }
    let definitions: HashMap<&str, &cadmpeg_ir::products::ProductDefinition> =
        ir.model.product_definitions.iter().map(|d| (d.id.as_str(), d)).collect();
    let hidden: HashSet<&str> = ir.model.bodies.iter().filter(|b| b.visible == Some(false)).map(|b| b.id.as_str()).collect();
    // Definitions that place parts of their own (assemblies): the file means their figures as
    // totals over those parts where they are placed, which the neutral model does not compose, so
    // they are not compared (as cadmpeg's own measuring leaves them), rather than compared with
    // the assembly's own bodies alone.
    let assemblies = assemblies(ir);
    let mut cache: HashMap<usize, Option<Measured>> = HashMap::new();
    let mut warnings = Vec::new();
    for m in stated {
        let (bodies, name): (Vec<&str>, String) = match &m.target {
            DeclaredTarget::Definition { definition } if assemblies.contains(definition.as_str()) => continue,
            DeclaredTarget::Definition { definition } => match definitions.get(definition.as_str()) {
                Some(d) => (
                    d.bodies.iter().map(|b| b.as_str()).filter(|b| !hidden.contains(b)).collect(),
                    [&d.label, &d.source_name, &d.part_number].into_iter().flatten().find(|n| !n.trim().is_empty()).cloned().unwrap_or_default(),
                ),
                None => continue,
            },
            DeclaredTarget::Bodies { bodies } => (bodies.iter().map(|b| b.as_str()).collect(), String::new()),
            DeclaredTarget::Unresolved {} => continue,
        };
        let label = if name.is_empty() { "a part".to_string() } else { format!("part '{name}'") };
        let sheets = matches!(m.quantity, DeclaredQuantity::SheetArea { .. } | DeclaredQuantity::SheetCentroid { .. });
        let mut parts = Vec::new();
        let mut missing = false;
        for body in &bodies {
            // A body no record came of (its conversion was refused, or its placement) was not
            // imported: the total would leave it out.
            let Some(records_of) = by_body.get(body) else {
                missing = true;
                continue;
            };
            for &k in records_of {
                let measured = cache.entry(k).or_insert_with(|| measure(records, origins, &built, k));
                match measured {
                    Some(p) if p.sheet == sheets => parts.push(k),
                    Some(_) => {}
                    None => missing = true,
                }
            }
        }
        let what = quantity_name(&m.quantity);
        if missing {
            warnings.push(format!("{label}: its {what} the file states was not checked: some of its bodies were not imported"));
            continue;
        }
        if parts.is_empty() {
            continue;
        }
        let ps: Vec<&Measured> = parts.iter().filter_map(|k| cache.get(k).and_then(Option::as_ref)).collect();
        let error = m.source_error.map_or(0.0, |e| e.get());
        let total = |f: &dyn Fn(&Measured) -> f64| ps.iter().map(|p| f(p)).sum::<f64>();
        let scalar = |stated: f64, measured: f64, unit: &str| {
            let off = (measured - stated).abs();
            (off > error + RELATIVE * stated.abs()).then(|| {
                format!(
                    "{label}: the file states its {what} as {stated:.6} {unit}; it imported as {measured:.6} {unit} ({:.3}% off)",
                    100.0 * off / stated.abs().max(f64::MIN_POSITIVE)
                )
            })
        };
        let centre = |stated: [f64; 3], weight: &dyn Fn(&Measured) -> f64| {
            let w = total(weight);
            if !(w > 0.0) {
                return None;
            }
            let c: [f64; 3] = std::array::from_fn(|i| ps.iter().map(|p| weight(p) * p.centroid[i]).sum::<f64>() / w);
            let off = (0..3).map(|i| (c[i] - stated[i]).powi(2)).sum::<f64>().sqrt();
            // A part's size, to scale the allowance: the side of a cube of its volume, else the
            // side of a square of its area.
            let size = if w == total(&|p| p.volume) { w.cbrt() } else { w.sqrt() };
            (off > error + RELATIVE * size).then(|| {
                format!(
                    "{label}: the file states its {what} at ({:.4}, {:.4}, {:.4}) mm; it imported at ({:.4}, {:.4}, {:.4}) mm, {off:.4} mm away",
                    stated[0], stated[1], stated[2], c[0], c[1], c[2]
                )
            })
        };
        let warning = match &m.quantity {
            DeclaredQuantity::Volume { value } => scalar(value.get(), total(&|p| p.volume), "mm³"),
            DeclaredQuantity::SolidArea { value } | DeclaredQuantity::SheetArea { value } => scalar(value.get(), total(&|p| p.area), "mm²"),
            // The outer shells' area: the measured area is every shell's, so only bodies of one
            // shell are compared.
            DeclaredQuantity::WettedArea { value } => {
                if ps.iter().all(|p| p.shells <= 1) {
                    scalar(value.get(), total(&|p| p.area), "mm²")
                } else {
                    None
                }
            }
            DeclaredQuantity::SolidCentroid { value } => centre(value.coordinates().map(|c| c.get()), &|p| p.volume),
            DeclaredQuantity::SheetCentroid { value } => centre(value.coordinates().map(|c| c.get()), &|p| p.area),
        };
        warnings.extend(warning);
    }
    warnings
}

/// The definitions whose occurrences place child occurrences.
fn assemblies(ir: &CadIr) -> HashSet<&str> {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};
    let by_id: HashMap<&str, &cadmpeg_ir::products::Occurrence> = ir.model.occurrences.iter().map(|o| (o.id.as_str(), o)).collect();
    ir.model
        .occurrences
        .iter()
        .filter_map(|o| match &o.parent {
            OccurrenceParent::Occurrence { occurrence } => by_id.get(occurrence.as_str()),
            OccurrenceParent::Root {} => None,
        })
        .filter_map(|parent| match &parent.prototype {
            PrototypeReference::Local { definition } => Some(definition.as_str()),
            _ => None,
        })
        .collect()
}

fn quantity_name(q: &DeclaredQuantity) -> &'static str {
    match q {
        DeclaredQuantity::Volume { .. } => "volume",
        DeclaredQuantity::SolidArea { .. } => "surface area",
        DeclaredQuantity::WettedArea { .. } => "outer surface area",
        DeclaredQuantity::SolidCentroid { .. } => "centre",
        DeclaredQuantity::SheetArea { .. } => "sheet area",
        DeclaredQuantity::SheetCentroid { .. } => "sheet centre",
    }
}

/// Record body `k` as built, measured, its centre taken back into its part's coordinates by its
/// occurrence's world placement; `None` when it was not imported or cannot be measured.
fn measure(records: &ImportRecords, origins: &[Origin], built: &HashMap<usize, &solidcraft_kernel::ImportedBody>, k: usize) -> Option<Measured> {
    let b = built.get(&k)?;
    let m = solidcraft_kernel::measure(&b.body).ok()?;
    let back = solidcraft_doc::mat_inverse(&origins.get(k)?.world)?;
    let c = solidcraft_doc::apply_point(&back, m.centroid);
    let centroid = [c.x, c.y, c.z];
    Some(Measured { volume: m.volume, area: m.area, shells: m.shells, centroid, sheet: records.bodies.get(k).is_some_and(|r| r.sheet) || !b.closed })
}
