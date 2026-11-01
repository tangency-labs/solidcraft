//! Compare Bodies: two versions of one part laid over each other. Both are measured, each
//! surface's distance to the other is sampled, and the places where only one of them has
//! material are found, measured and, on request, coloured. The places are found from the
//! surfaces ([`solidcraft_geom::compare`]), so bodies that share almost every face compare as
//! well as any; each place's volume, centre and box are then measured exactly, on the pieces of
//! the two cuts a − b and b − a that lie there, as `inspect.measure` measures a body.

use serde_json::{Value, json};
use solidcraft_geom::compare::{self, CompareOptions, Side};
use solidcraft_geom::{Aabb3, Vec3};

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Sel, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.compare", "Compare Bodies", compare_bodies).icon("compare").params(
    "a, b: body names (default: the two selected bodies, or the design's only two), in world placement; tolerance?: mm (0.001 to 10, default 0.01): \
     surfaces closer than this agree; spacing?: mm (default 0.5): the largest piece of a face sampled as one point, coarser for a body that would need \
     more than max_points? (default 8 million, up to 50 million) samples; max_regions? (default 50); color?: true | {only_in_a?, only_in_b?: \"#rrggbb\"} \
     (colours the faces that hold a region, red where only a has material and green where only b has: the faces mostly beyond the tolerance, else the \
     one with the largest share; an undo step) → a, b: volume_mm3, area_mm2, center_of_mass, bbox; difference: b − a; \
     deviation: a_to_b, b_to_a (max_mm, mean_mm, at, area_beyond_mm2); regions, largest first: side (only_in_a | only_in_b), volume_mm3, center and bbox (measured_by: \"boolean\": exact, the pieces of a − b or \
     b − a there, measured as inspect.measure does; \"not measured\": the cut could not be made, so volume_mm3 and center are null and \
     not_measured says why, never an estimate), area_mm2, max_deviation_mm, at, faces_a, faces_b (face indices, as model.faces lists them); identical; verdict; warnings \
     (a body whose surface does not enclose what it measures, lines left out, a cut that could not be made). The regions are found from the two \
     surfaces, so near-identical bodies compare too",
)];

/// Default colours: red for material only `a` has, green for material only `b` has.
const ONLY_IN_A: [u8; 3] = [0xe0, 0x31, 0x31];
const ONLY_IN_B: [u8; 3] = [0x2f, 0x9e, 0x44];

fn side_name(s: Side) -> &'static str {
    match s {
        Side::OnlyInA => "only_in_a",
        Side::OnlyInB => "only_in_b",
    }
}

/// A number parameter in `range`, `default` when absent.
fn number(cmd: &str, p: &Value, key: &str, default: f64, range: std::ops::RangeInclusive<f64>) -> Result<f64> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && range.contains(x))
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a number from {} to {}", range.start(), range.end()))),
    }
}

/// The two bodies: `a` and `b`, else the two selected bodies, else the design's only two.
fn pair(s: &Session, st: &solidcraft_doc::ModelState, p: &Value, cmd: &str) -> Result<(String, String)> {
    let given = |k: &str| -> Result<Option<String>> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(n)) => Ok(Some(n.clone())),
            Some(_) => Err(bad(cmd, format!("`{k}` must be a body name"))),
        }
    };
    let (a, b) = match (given("a")?, given("b")?) {
        (Some(a), Some(b)) => (a, b),
        (None, None) => {
            let selected: Vec<String> =
                s.selection.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect();
            let all: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
            match (selected.as_slice(), all.as_slice()) {
                ([a, b], _) | (_, [a, b]) => (a.clone(), b.clone()),
                _ => return Err(bad(cmd, "give `a` and `b` (or select two bodies)")),
            }
        }
        _ => return Err(bad(cmd, "give both `a` and `b`")),
    };
    for n in [&a, &b] {
        if st.body(n).is_none() {
            return Err(bad(cmd, format!("no body `{n}`")));
        }
    }
    if a == b {
        return Err(bad(cmd, "`a` and `b` must be two different bodies"));
    }
    Ok((a, b))
}

fn colours(cmd: &str, p: &Value) -> Result<Option<[[u8; 3]; 2]>> {
    match p.get("color") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
        Some(Value::Bool(true)) => Ok(Some([ONLY_IN_A, ONLY_IN_B])),
        Some(Value::Object(o)) => {
            let pick = |k: &str, d: [u8; 3]| match o.get(k) {
                None | Some(Value::Null) => Ok(d),
                Some(v) => {
                    super::inspect::color_param(v).ok_or_else(|| bad(cmd, format!("`color.{k}` must be \"#rrggbb\" or [r, g, b] with values 0–255")))
                }
            };
            Ok(Some([pick("only_in_a", ONLY_IN_A)?, pick("only_in_b", ONLY_IN_B)?]))
        }
        Some(_) => Err(bad(cmd, "`color` must be true or {only_in_a, only_in_b}")),
    }
}

fn v3(v: &Value) -> Option<Vec3> {
    crate::params::vec3(v)
}

/// b − a of the two bodies' measures.
fn difference(ma: &Value, mb: &Value) -> Value {
    let f = |m: &Value, k: &str| m.get(k).and_then(Value::as_f64);
    let sub = |x: Option<f64>, y: Option<f64>| x.zip(y).map(|(x, y)| y - x);
    let (va, vb) = (f(ma, "volume_mm3"), f(mb, "volume_mm3"));
    let com = v3(&ma["center_of_mass"]).zip(v3(&mb["center_of_mass"])).map(|(a, b)| b - a);
    let corner = |k: &str| v3(&ma["bbox"][k]).zip(v3(&mb["bbox"][k])).map(|(a, b)| b - a);
    json!({
        "volume_mm3": sub(va, vb),
        "volume_percent": va.zip(vb).filter(|(a, _)| a.abs() > 0.0).map(|(a, b)| 100.0 * (b - a) / a),
        "area_mm2": sub(f(ma, "area_mm2"), f(mb, "area_mm2")),
        "center_of_mass": com,
        "center_of_mass_distance_mm": com.map(Vec3::len),
        "bbox_min": corner("min"),
        "bbox_max": corner("max"),
    })
}

/// The volume a closed mesh encloses (divergence theorem, about its box's centre).
fn enclosed_volume(m: &solidcraft_geom::Mesh) -> f64 {
    let o = m.bounds().center();
    m.triangles.iter().filter_map(|t| m.tri(t)).map(|[a, b, c]| (a - o).dot((b - o).cross(c - o)) / 6.0).sum()
}

/// What makes the comparison less certain: a body whose surface does not enclose what it
/// measures (open, or faces that failed).
fn warnings(bodies: [(&str, &Value, &solidcraft_geom::Mesh); 2], chord: f64) -> Vec<String> {
    let mut out = Vec::new();
    for (name, measure, mesh) in bodies {
        let (Some(v), area) = (measure["volume_mm3"].as_f64(), measure["area_mm2"].as_f64().unwrap_or(0.0)) else { continue };
        let enclosed = enclosed_volume(mesh);
        // A mesh within `chord` of the surface encloses the volume to within chord × area.
        if !((enclosed - v).abs() <= chord * area.abs() + 1e-6 * v.abs()) {
            out.push(format!(
                "`{name}`: its surface encloses {enclosed:.3} mm³ but the body measures {v:.3} mm³: it is open or has faces that failed, so its measures and the regions near the gap are unreliable"
            ));
        }
    }
    out
}

/// A region's volume, centre and box, measured exactly.
#[derive(Clone, Copy, Debug)]
struct Measured {
    volume: f64,
    center: Vec3,
    bbox: Aabb3,
}

/// The pieces of `from` − `tool`, the material only `from` has, each measured as
/// `inspect.measure` measures a body (none when `tool` takes all of it).
fn cut_pieces(from: &solidcraft_kernel::Body, tool: &solidcraft_kernel::Body) -> std::result::Result<Vec<Measured>, String> {
    let rest = solidcraft_kernel::boolean(from, tool, solidcraft_kernel::BoolOp::Cut).map_err(|e| e.to_string())?;
    let Some(rest) = rest else { return Ok(Vec::new()) };
    rest.lumps()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|piece| {
            let m = solidcraft_kernel::measure_mass(piece).map_err(|e| e.to_string())?;
            Ok(Measured { volume: m.volume, center: m.centroid, bbox: m.bbox })
        })
        .collect()
}

/// How much of `piece` lies by `region`: the volume their boxes, each grown by `pad`, share
/// (0 when they do not meet). Grown, so a region found as a flat patch of surface still holds
/// the piece on it.
fn overlap(piece: &Aabb3, region: &Aabb3, pad: f64) -> f64 {
    let shared = piece.inflated(pad).intersection(&region.inflated(pad));
    if shared.is_empty() {
        return 0.0;
    }
    let d = shared.size();
    d.x * d.y * d.z
}

/// Each region's exact measure: the pieces of the cut on its side (a − b for material only `a`
/// has, b − a for material only `b` has), each given to the region of that side whose box holds
/// most of it, summed. A piece by no region is thinner than the tolerance or in a region not
/// listed. `None` for a region whose cut could not be made or that no piece fell to; the second
/// value says why, once per side.
fn exact_regions(
    a: &solidcraft_kernel::Body,
    b: &solidcraft_kernel::Body,
    c: &compare::Comparison,
    pad: f64,
) -> (Vec<Option<Measured>>, Vec<String>) {
    let sides = [Side::OnlyInA, Side::OnlyInB];
    let wanted: Vec<Side> = sides.into_iter().filter(|side| c.regions.iter().any(|r| r.side == *side)).collect();
    let cuts = solidcraft_kernel::map_on_threads(&wanted, 2, |side| match side {
        Side::OnlyInA => cut_pieces(a, b),
        Side::OnlyInB => cut_pieces(b, a),
    });
    // (volume, volume × centre, box) of the pieces each region holds.
    let mut sums: Vec<Option<(f64, Vec3, Aabb3)>> = vec![None; c.regions.len()];
    let mut failed = Vec::new();
    for (side, cut) in wanted.iter().zip(cuts) {
        let pieces = match cut {
            Some(Ok(pieces)) => pieces,
            Some(Err(e)) => {
                failed.push((*side, e));
                continue;
            }
            None => {
                failed.push((*side, "the cut failed".to_string()));
                continue;
            }
        };
        for p in pieces.iter().filter(|p| p.volume.is_finite() && p.center.is_finite() && p.volume > 0.0) {
            let best = c
                .regions
                .iter()
                .enumerate()
                .filter(|(_, r)| r.side == *side)
                .map(|(i, r)| (i, overlap(&p.bbox, &r.bbox, pad)))
                .filter(|(_, o)| *o > 0.0)
                .max_by(|x, y| x.1.total_cmp(&y.1));
            let Some((i, _)) = best else { continue };
            if let Some(slot) = sums.get_mut(i) {
                let (v, m, bb) = slot.get_or_insert((0.0, Vec3::ZERO, Aabb3::EMPTY));
                *v += p.volume;
                *m += p.center * p.volume;
                *bb = bb.union(&p.bbox);
            }
        }
    }
    let measured = sums.into_iter().map(|x| x.map(|(v, m, bbox)| Measured { volume: v, center: m / v, bbox })).collect::<Vec<_>>();
    let mut notes: Vec<String> = failed
        .iter()
        .map(|(side, e)| {
            let (from, tool) = if *side == Side::OnlyInA { ("a", "b") } else { ("b", "a") };
            format!("the cut {from} − {tool} could not be made ({e}): its regions' volumes are not measured")
        })
        .collect();
    let unmatched = c.regions.iter().zip(&measured).filter(|(r, m)| m.is_none() && !failed.iter().any(|(side, _)| *side == r.side)).count();
    if unmatched > 0 {
        notes.push(format!("{unmatched} region(s) held no piece of the cut: their volumes are not measured"));
    }
    (measured, notes)
}

fn deviation_json(d: &compare::Deviation) -> Value {
    json!({"max_mm": d.max, "mean_mm": d.mean, "at": d.at, "area_beyond_mm2": d.beyond_area, "area_mm2": d.area, "points": d.points})
}

fn compare_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.compare";
    let tolerance = number(cmd, p, "tolerance", 0.01, 0.001..=10.0)?;
    let spacing = number(cmd, p, "spacing", 0.5, 0.01..=1000.0)?;
    let max_points = number(cmd, p, "max_points", 8e6, 1e4..=5e7)? as usize;
    let max_regions = number(cmd, p, "max_regions", 50.0, 1.0..=1000.0)? as usize;
    let colors = colours(cmd, p)?;
    let st = s.world_state()?;
    let (na, nb) = pair(s, &st, p, cmd)?;
    let (Some(ba), Some(bb)) = (st.body(&na), st.body(&nb)) else { return Err(bad(cmd, "no such body")) };
    // Meshed within a quarter of the tolerance (the kernel goes no finer than a millionth of a
    // body's size), so two meshes of one surface agree well within it.
    let chord = tolerance / 4.0;
    let mut done = solidcraft_kernel::map_on_threads(&[ba, bb], 2, |b| {
        (super::inspect::measure_json(b), b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(chord)))
    })
    .into_iter()
    .zip([&na, &nb]);
    let mut next = || -> Result<(Value, solidcraft_geom::Mesh)> {
        let Some((r, name)) = done.next() else { return Err(bad(cmd, "no such body")) };
        let (m, mesh) = r.ok_or_else(|| bad(cmd, format!("`{name}`: measuring it failed")))?;
        let mesh = mesh.map_err(|e| bad(cmd, format!("`{name}`: {e}")))?;
        if mesh.triangles.is_empty() {
            return Err(bad(cmd, format!("`{name}` has no surface")));
        }
        Ok((m, mesh))
    };
    let ((measure_a, mesh_a), (measure_b, mesh_b)) = (next()?, next()?);
    let options = CompareOptions { tolerance, spacing, max_points, max_regions, ..CompareOptions::default() };
    let each = |n: usize, job: &(dyn Fn(usize) + Sync)| {
        let jobs: Vec<usize> = (0..n).collect();
        solidcraft_kernel::for_each_on_threads(&jobs, 16, |i| job(*i));
    };
    let c = compare::compare(&mesh_a, &mesh_b, &options, &each);
    // The regions' exact volumes, from the cuts of two bodies that enclose volumes.
    let (exact, exact_notes) = if c.regions.is_empty() {
        (Vec::new(), Vec::new())
    } else if super::inspect::encloses_volume(&ba.body) && super::inspect::encloses_volume(&bb.body) {
        exact_regions(&ba.body, &bb.body, &c, c.spacing.max(tolerance))
    } else {
        (vec![None; c.regions.len()], vec!["a body encloses no volume, so the regions' volumes are not measured".to_string()])
    };
    // A region's exact volume, centre and box, or none: never the lines' estimate in its place.
    let measured = |i: usize| exact.get(i).copied().flatten();
    // Largest first: by the exact volume, then (unmeasured) by area.
    let mut order: Vec<usize> = (0..c.regions.len()).collect();
    let key = |i: usize| (measured(i).map(|m| m.volume), c.regions.get(i).map_or(0.0, |r| r.area_a + r.area_b));
    order.sort_by(|&x, &y| match (key(x).0, key(y).0) {
        (Some(vx), Some(vy)) => vy.total_cmp(&vx),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => key(y).1.total_cmp(&key(x).1),
    });
    let reason = |i: usize| -> String {
        let side = c.regions.get(i).map(|r| r.side);
        let mine = |n: &&String| side.is_some_and(|s| n.starts_with(if s == Side::OnlyInA { "the cut a" } else { "the cut b" }));
        exact_notes.iter().find(mine).or_else(|| exact_notes.last()).cloned().unwrap_or_else(|| "the exact cut gave no piece here".to_string())
    };
    let axis = |k: usize| ["X", "Y", "Z"].get(k).copied().unwrap_or("?");
    let regions: Vec<Value> = order
        .iter()
        .enumerate()
        .filter_map(|(i, &k)| c.regions.get(k).map(|r| (i, k, r)))
        .map(|(i, k, r)| {
            let m = measured(k);
            json!({
                "index": i + 1,
                "side": side_name(r.side),
                "material_of": if r.side == Side::OnlyInA { &na } else { &nb },
                "volume_mm3": m.map(|m| m.volume),
                "measured_by": if m.is_some() { "boolean" } else { "not measured" },
                "not_measured": if m.is_some() { Value::Null } else { json!(reason(k)) },
                "area_mm2": r.area_a + r.area_b,
                "area_a_mm2": r.area_a,
                "area_b_mm2": r.area_b,
                "max_deviation_mm": r.max_deviation,
                "at": r.at,
                "center": m.map(|m| m.center),
                "bbox": m.map(|m| json!({"min": m.bbox.min, "max": m.bbox.max})).unwrap_or_else(|| json!({"min": r.bbox.min, "max": r.bbox.max})),
                "faces_a": r.faces_a.iter().map(|x| x.0).collect::<Vec<u32>>(),
                "faces_b": r.faces_b.iter().map(|x| x.0).collect::<Vec<u32>>(),
                "resolution": {"along": axis(r.lines.axis), "line_spacing_mm": r.lines.spacing, "lines": r.lines.count, "skipped_lines": r.lines.skipped},
            })
        })
        .collect();
    // A side's total, or none when one of its regions is not measured.
    let total = |side: Side| {
        let rs: Vec<Option<f64>> =
            (0..c.regions.len()).filter(|&i| c.regions.get(i).is_some_and(|r| r.side == side)).map(|i| measured(i).map(|m| m.volume)).collect();
        // `+ 0.0`: an empty sum is -0.0.
        (rs.len(), rs.iter().copied().sum::<Option<f64>>().map(|v| v + 0.0))
    };
    let ((ca, va), (cb, vb)) = (total(Side::OnlyInA), total(Side::OnlyInB));
    let worst = c.a_to_b.max.max(c.b_to_a.max);
    let identical = c.regions.is_empty() && c.unlisted == 0;
    let mut warnings = warnings([(na.as_str(), &measure_a, &mesh_a), (nb.as_str(), &measure_b, &mesh_b)], chord);
    warnings.extend(exact_notes);
    let mut verdict = if identical {
        format!("identical within {tolerance} mm (largest deviation {worst:.4} mm)")
    } else {
        let more = if c.unlisted > 0 { format!(" (and {} more, not measured)", c.unlisted) } else { String::new() };
        format!(
            "{} {} by more than {tolerance} mm: {ca} with material only in `{na}` ({}), {cb} only in `{nb}` ({}){more}",
            c.regions.len(),
            if c.regions.len() == 1 { "region differs" } else { "regions differ" },
            va.map_or("volume not measured".to_string(), |v| format!("{v:.3} mm³")),
            vb.map_or("volume not measured".to_string(), |v| format!("{v:.3} mm³")),
        )
    };
    if !warnings.is_empty() {
        verdict += &format!(" ({} warning{})", warnings.len(), if warnings.len() == 1 { "" } else { "s" });
    }
    let mut out = json!({
        "a": measure_a,
        "b": measure_b,
        "difference": difference(&measure_a, &measure_b),
        "tolerance_mm": tolerance,
        "deviation": {"a_to_b": deviation_json(&c.a_to_b), "b_to_a": deviation_json(&c.b_to_a), "max_mm": worst},
        "identical": identical,
        "verdict": verdict,
        "regions": regions,
        "only_in_a": {"regions": ca, "volume_mm3": va},
        "only_in_b": {"regions": cb, "volume_mm3": vb},
        "unlisted_regions": c.unlisted,
        "warnings": warnings,
        "sampling": {"spacing_mm": c.spacing, "chord_mm": chord, "triangles_a": mesh_a.triangles.len(), "triangles_b": mesh_b.triangles.len()},
    });
    if let Some(colors) = colors {
        out["colored_faces"] = json!(color_regions(s, &c, [&na, &nb], colors)?);
    }
    Ok(out)
}

/// A face is coloured for a region when at least this share of it deviates there.
const MOSTLY: f64 = 0.5;

/// The faces to colour for a region, as (body: 0 = a, 1 = b; face): those mostly beyond the
/// tolerance (a small difference on a large face would colour all of it), or, when it has none,
/// the one with the largest share.
fn faces_to_color(c: &compare::Comparison, r: &compare::Region) -> Vec<(usize, u32)> {
    let share = |m: usize, &(f, beyond): &(u32, f64)| {
        let all = c.face_area.get(m).and_then(|x| x.get(&f)).copied().unwrap_or(0.0);
        (m, f, if all > 0.0 { beyond / all } else { 0.0 })
    };
    let all: Vec<(usize, u32, f64)> = r.faces_a.iter().map(|x| share(0, x)).chain(r.faces_b.iter().map(|x| share(1, x))).collect();
    let mostly: Vec<(usize, u32)> = all.iter().filter(|x| x.2 >= MOSTLY).map(|x| (x.0, x.1)).collect();
    if !mostly.is_empty() {
        return mostly;
    }
    all.iter().max_by(|x, y| x.2.total_cmp(&y.2)).map(|x| vec![(x.0, x.1)]).unwrap_or_default()
}

/// Give the faces holding each region the colour of its side (a face holding several takes the
/// largest one's); returns how many face appearances were set.
fn color_regions(s: &mut Session, c: &compare::Comparison, names: [&String; 2], colors: [[u8; 3]; 2]) -> Result<usize> {
    use solidcraft_doc::appearance::{FaceLook, Look};
    let st = s.model.state();
    // Where each face of the two bodies is, as the design file names and finds it. A body placed
    // by an occurrence is coloured where it is defined, if at all.
    let found: Vec<_> =
        names.iter().map(|n| st.body(n).map(|b| (solidcraft_doc::naming::face_spots(b), solidcraft_doc::naming::face_names(b)))).collect();
    let mut picks: Vec<(String, Vec3, Option<String>, Look)> = Vec::new();
    // Smallest region first, so the largest one's colour is the one that stays.
    for r in c.regions.iter().rev() {
        let colour = if r.side == Side::OnlyInA { colors[0] } else { colors[1] };
        let mut look = Look::custom(colour, 1.0);
        look.name = if r.side == Side::OnlyInA { "Only in a".into() } else { "Only in b".into() };
        for (m, f) in faces_to_color(c, r) {
            let (Some(Some((spots, face_names))), Some(name)) = (found.get(m), names.get(m)) else { continue };
            let Some(spot) = spots.get(f as usize).copied().flatten() else { continue };
            picks.retain(|(b, pt, _, _)| !(b == *name && pt.dist(spot.point) < 1e-9));
            picks.push(((*name).clone(), spot.point, face_names.get(f as usize).cloned(), look.clone()));
        }
    }
    if picks.is_empty() {
        return Ok(0);
    }
    let a = &mut s.doc_mut().appearances;
    let mut n = 0;
    for (body, point, name, look) in picks {
        a.faces.retain(|f| !(f.body == body && f.point.dist(point) < 1e-9));
        if a.faces.len() >= 100_000 {
            return Err(bad("inspect.compare", "too many face appearances"));
        }
        a.faces.push(FaceLook { body, point, look, name, strict_name: false, certificate: None });
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;
