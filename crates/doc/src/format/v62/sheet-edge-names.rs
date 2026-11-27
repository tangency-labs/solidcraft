//! A flange's or a hem's picked edges are saved by the model's names for them (#6748, #7990), as
//! every feature's edges are: `<face A>|<face B>`, the two faces that meet there in sorted order,
//! each named by the model after what made it. A design saved before keeps them by the sheet's own
//! names, `base:<line>:<side>` and `F<f>.<k>:<edge>:<side>`, which no edge of the model has: a
//! flange or hem that picked one fails, asking for the edge to be picked again. As one opens, each
//! such name of an edge the design says what made is the model's name for that edge:
//!
//! - `base:<line>:top` on the sheet of base flange `F<b>` is the edge where the panel's top meets
//!   its rim on the sketch line `<line>`: `F<b>:side:<line>|F<b>:top`, and `bottom` likewise.
//! - `F<f>.<k>:far:top` is the edge where the `k`th flange of edge flange or hem `F<f>` meets its
//!   far rim with its top. That flange is the one made on the feature's own `k`th edge where each
//!   edge up to it makes one flange (a hem's always, an edge flange's where none of them is split
//!   into segments), and it stands on that edge's rim, `<rim>`: the model's name for the face that
//!   meets the edge, read from the name the feature saved for it. The edge is
//!   `F<f>:far:<rim>|F<f>:top:<rim>`. `side0` and `side1` are the flange's end rims,
//!   `F<f>:end1:<rim>` and `F<f>:end0:<rim>`: the sheet ran an edge with its panel's material on
//!   the left seen from the top, and `side0` was at its start; a bend's anchor runs the edge the
//!   other way, and `end0` is at the anchor's start.
//! - The sheet a feature's names are on is its `body`, where a base flange alone keeps that name
//!   (`body_names`) and nothing converts it; else the one sheet before it, where one base flange
//!   makes every sheet before it (no other flange, conversion or recognition makes one, and no
//!   pattern, mirror or copying move copies a body before it). A copy made after it, as of a part
//!   mirrored once its flanges are on, made no sheet it picked an edge on.
//!
//! Every other name is left, and what picked it fails as it did: a name on a sheet the design does
//! not say; a converted sheet's flange (`F<c>.<k>`, numbered in the order the conversion reached
//! its faces), a fold's or a path flange's panel (numbered as their builds made them), and an edge
//! flange's past an edge split into segments (one flange for each run of segments with the same
//! values, which only evaluating them says); a flange's `bend`, which is no free edge; an edge
//! saved with no name; and an edge along a sketch line in line with another, which the sheet named
//! after the first of the two in the sketch, whichever the edge lay on. The last release warns
//! about the newer format number and opens the design; its sheets find none of these names and
//! take the edges at the picked points, saying so.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{DocError, Result};

/// The features that make a sheet.
const MAKERS: [&str; 5] = ["sheet_base", "sheet_contour", "sheet_convert", "sheet_recognize", "sheet_surface_convert"];

/// The features that make a sheet of a body they convert.
const CONVERSIONS: [&str; 3] = ["sheet_convert", "sheet_recognize", "sheet_surface_convert"];

/// How deep a flange stands on flanges before it, at most, whose names are followed.
const DEPTH: usize = 64;

/// Maximum total `body_names` entries inspected across all features.
const MAX_BODY_NAMES: usize = 200_000;
/// Maximum total `edge_names` entries inspected across all sheet flanges and hems.
const MAX_EDGE_NAMES: usize = 200_000;
/// Maximum total pairwise collinearity checks performed in one upgrade step.
const MAX_LINE_CHECKS: usize = 2_000_000;
/// Lines at least this long (mm) have angular collinearity window `2e-6 / L <= 2e-4` rad.
const LONG_LINE: f64 = 1e-2;
/// Period of the piecewise-linear half-circle angle parameter (`2 * sqrt(2)`).
const ANGLE_PERIOD: f64 = 2.0 * std::f64::consts::SQRT_2;
/// Angular search window covering `2e-4` rad with generous floating-point safety margin.
const ANGLE_MARGIN: f64 = 3e-4;

fn kind(f: &Value) -> Option<&str> {
    f.get("type").and_then(Value::as_str)
}

fn id(f: &Value) -> Option<u64> {
    f.get("id").and_then(Value::as_u64)
}

/// Whether feature `f` may copy a body, and a sheet body's sheet with it: a pattern, a mirror, a
/// move that copies.
fn copies(f: &Value) -> bool {
    matches!(kind(f), Some("pattern" | "mirror")) || (kind(f) == Some("move") && f.get("copy").and_then(Value::as_bool) == Some(true))
}

enum Keeper {
    One(usize),
    Many,
}

/// Precomputed `O(1)` lookups over the design's feature list.
struct FeatureIndex<'a> {
    features: &'a [Value],
    /// Each non-empty body name kept uniquely by one `sheet_base` and converted by nothing.
    body_base: HashMap<&'a str, usize>,
    /// For each feature index `at`, the sole `sheet_base` before `at` when no other maker or
    /// copying feature precedes `at`.
    sole_base_before: Vec<Option<usize>>,
    /// First feature index for each feature id.
    first_by_id: HashMap<u64, usize>,
    /// First sketch feature index for each sketch id.
    sketches: HashMap<u64, usize>,
}

impl<'a> FeatureIndex<'a> {
    fn build(features: &'a [Value]) -> Result<Self> {
        let mut keepers: HashMap<&'a str, Keeper> = HashMap::new();
        let mut converted: HashSet<&'a str> = HashSet::new();
        let mut sole_base_before = Vec::with_capacity(features.len());
        let mut first_by_id = HashMap::with_capacity(features.len());
        let mut sketches = HashMap::new();
        let mut maker_count = 0usize;
        let mut first_maker: Option<usize> = None;
        let mut had_copy = false;
        let mut total_body_names = 0usize;

        for (idx, f) in features.iter().enumerate() {
            let sole =
                if maker_count == 1 && !had_copy { first_maker.filter(|&m| features.get(m).and_then(kind) == Some("sheet_base")) } else { None };
            sole_base_before.push(sole);

            if let Some(fid) = id(f) {
                first_by_id.entry(fid).or_insert(idx);
                if kind(f) == Some("sketch") {
                    sketches.entry(fid).or_insert(idx);
                }
            }
            if let Some(names) = f.get("body_names").and_then(Value::as_array) {
                total_body_names = total_body_names.saturating_add(names.len());
                if total_body_names > MAX_BODY_NAMES {
                    return Err(DocError::Invalid("too many body names in design".into()));
                }
                for name in names.iter().filter_map(Value::as_str) {
                    match keepers.entry(name) {
                        Entry::Vacant(entry) => {
                            entry.insert(Keeper::One(idx));
                        }
                        Entry::Occupied(mut entry) => {
                            if !matches!(entry.get(), Keeper::One(prev) if *prev == idx) {
                                entry.insert(Keeper::Many);
                            }
                        }
                    }
                }
            }
            if kind(f).is_some_and(|k| CONVERSIONS.contains(&k))
                && let Some(body) = f.get("body").and_then(Value::as_str)
            {
                converted.insert(body);
            }
            if kind(f).is_some_and(|k| MAKERS.contains(&k)) {
                maker_count += 1;
                if first_maker.is_none() {
                    first_maker = Some(idx);
                }
            }
            if copies(f) {
                had_copy = true;
            }
        }

        let mut body_base = HashMap::new();
        for (name, keeper) in keepers {
            if !name.is_empty()
                && !converted.contains(name)
                && let Keeper::One(idx) = keeper
                && features.get(idx).and_then(kind) == Some("sheet_base")
            {
                body_base.insert(name, idx);
            }
        }

        Ok(Self { features, body_base, sole_base_before, first_by_id, sketches })
    }

    /// The base flange whose sheet the edges feature `at` picked lie on, as the saved design says.
    fn base_of(&self, at: usize) -> Option<usize> {
        let f = self.features.get(at)?;
        if let Some(body) = f.get("body").and_then(Value::as_str).filter(|b| !b.is_empty())
            && let Some(&idx) = self.body_base.get(body)
        {
            return Some(idx);
        }
        self.sole_base_before.get(at).copied().flatten()
    }
}

/// How far `p` lies off the line through `s` and `e`.
fn off(p: [f64; 2], s: [f64; 2], e: [f64; 2]) -> f64 {
    let (tx, ty) = (e[0] - s[0], e[1] - s[1]);
    ((p[0] - s[0]) * ty - (p[1] - s[1]) * tx).abs() / tx.hypot(ty)
}

fn along(p: [f64; 2], q: [f64; 2], s: [f64; 2], e: [f64; 2]) -> bool {
    off(p, s, e) < 1e-6 && off(q, s, e) < 1e-6
}

/// Piecewise-linear angle parameter in `[0, 2*sqrt(2))` and signed origin offset for a segment of
/// length `len >= LONG_LINE`. Its derivative with respect to angle lies in `[sqrt(2)/2, 1]`.
fn line_key(a: [f64; 2], b: [f64; 2], len: f64) -> (f64, f64) {
    let (mut ux, mut uy) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
    if uy < 0.0 || (uy == 0.0 && ux < 0.0) {
        ux = -ux;
        uy = -uy;
    }
    let t = if ux >= uy {
        uy
    } else if -ux >= uy {
        ANGLE_PERIOD - uy
    } else {
        std::f64::consts::SQRT_2 - ux
    };
    let (nx, ny) = (-uy, ux);
    let d = nx * (0.5 * (a[0] + b[0])) + ny * (0.5 * (a[1] + b[1]));
    (t, d)
}

struct LongLine {
    t: f64,
    d: f64,
    idx: usize,
}

/// Indexed lines of one base sketch, built once and reused across all flanges on that sheet.
struct SketchLines {
    lines: Vec<(String, [f64; 2], [f64; 2])>,
    first_index: HashMap<String, usize>,
    long_lines: Vec<LongLine>,
    short_lines: Vec<usize>,
    max_coord: f64,
    alone_cache: HashMap<usize, bool>,
}

impl SketchLines {
    fn build(features: &[Value], sketch_idx: Option<usize>) -> Result<Self> {
        let Some(s) = sketch_idx.and_then(|i| features.get(i)).and_then(|f| f.get("sketch")) else {
            return Ok(Self {
                lines: Vec::new(),
                first_index: HashMap::new(),
                long_lines: Vec::new(),
                short_lines: Vec::new(),
                max_coord: 0.0,
                alone_cache: HashMap::new(),
            });
        };
        let points = s.get("points").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let curves = s.get("curves").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        if curves.len() > solidcraft_sketch::CEILING_CURVES {
            return Err(DocError::Invalid("damaged design: sketch too large".into()));
        }
        let at = |c: &Value, end: &str| -> Option<[f64; 2]> {
            let pos = points.get(usize::try_from(c.get(end)?.as_u64()?).ok()?)?.get("pos")?;
            Some([pos.get(0)?.as_f64()?, pos.get(1)?.as_f64()?])
        };
        let lines: Vec<(String, [f64; 2], [f64; 2])> = curves
            .iter()
            .filter(|c| kind(c) == Some("line") && c.get("construction").and_then(Value::as_bool) != Some(true))
            .filter_map(|c| Some((c.get("id")?.as_str()?.to_string(), at(c, "a")?, at(c, "b")?)))
            .collect();
        let mut first_index = HashMap::with_capacity(lines.len());
        let mut long_lines = Vec::with_capacity(lines.len());
        let mut short_lines = Vec::new();
        let mut max_coord = 0.0_f64;
        for (idx, (line_id, a, b)) in lines.iter().enumerate() {
            first_index.entry(line_id.clone()).or_insert(idx);
            let coords = [a[0], a[1], b[0], b[1]];
            let finite = coords.iter().all(|v| v.is_finite());
            if finite {
                max_coord = max_coord.max(a[0].abs() + a[1].abs()).max(b[0].abs() + b[1].abs());
            }
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            if finite && len >= LONG_LINE {
                let (t, d) = line_key(*a, *b, len);
                if t.is_finite() && d.is_finite() {
                    long_lines.push(LongLine { t, d, idx });
                    continue;
                }
            }
            short_lines.push(idx);
        }
        long_lines.sort_unstable_by(|x, y| x.t.total_cmp(&y.t).then_with(|| x.d.total_cmp(&y.d)));
        Ok(Self { lines, first_index, long_lines, short_lines, max_coord, alone_cache: HashMap::new() })
    }

    /// Whether the sketch line `line` is one of `lines` and no other lies in line with it: the
    /// sheet named an edge of its base after the first line in the sketch whose line both its ends
    /// lay on (within 1e-6 mm), so beside another in line (the two sides of a notch) the name says
    /// neither.
    fn alone(&mut self, line: &str, work: &mut usize) -> Result<bool> {
        let Some(&idx) = self.first_index.get(line) else { return Ok(false) };
        if let Some(&cached) = self.alone_cache.get(&idx) {
            return Ok(cached);
        }
        let Some((_, a, b)) = self.lines.get(idx) else {
            return Ok(false);
        };
        let (a, b) = (*a, *b);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if !(len > 1e-9) {
            self.alone_cache.insert(idx, false);
            return Ok(false);
        }
        let lines = &self.lines;
        let check_other = |other_idx: usize, work: &mut usize| -> Result<bool> {
            let Some((other_id, p, q)) = lines.get(other_idx) else { return Ok(false) };
            if other_id == line {
                return Ok(false);
            }
            *work = work.saturating_add(1);
            if *work > MAX_LINE_CHECKS {
                return Err(DocError::Invalid("too many sheet edge comparisons during format upgrade".into()));
            }
            Ok(along(*p, *q, a, b) || along(a, b, *p, *q))
        };

        let finite = [a[0], a[1], b[0], b[1]].iter().all(|v| v.is_finite());
        let mut is_alone = true;
        if !finite || len < LONG_LINE {
            for other_idx in 0..self.lines.len() {
                if check_other(other_idx, work)? {
                    is_alone = false;
                    break;
                }
            }
        } else {
            let (t1, d1) = line_key(a, b, len);
            if !t1.is_finite() || !d1.is_finite() {
                for other_idx in 0..self.lines.len() {
                    if check_other(other_idx, work)? {
                        is_alone = false;
                        break;
                    }
                }
            } else {
                for &other_idx in &self.short_lines {
                    if check_other(other_idx, work)? {
                        is_alone = false;
                        break;
                    }
                }
                if is_alone {
                    let intervals = if t1 < ANGLE_MARGIN {
                        [(0.0, t1 + ANGLE_MARGIN), (ANGLE_PERIOD - (ANGLE_MARGIN - t1), ANGLE_PERIOD)]
                    } else if t1 + ANGLE_MARGIN >= ANGLE_PERIOD {
                        [(t1 - ANGLE_MARGIN, ANGLE_PERIOD), (0.0, (t1 + ANGLE_MARGIN - ANGLE_PERIOD).max(0.0))]
                    } else {
                        [(t1 - ANGLE_MARGIN, t1 + ANGLE_MARGIN), (1.0, 0.0)]
                    };
                    let d_margin = 1e-4 + 1e-12 * self.max_coord;
                    'intervals: for (t_lo, t_hi) in intervals {
                        if t_lo > t_hi {
                            continue;
                        }
                        let lo = self.long_lines.partition_point(|e| e.t < t_lo);
                        let hi = self.long_lines.partition_point(|e| e.t <= t_hi);
                        let slice = self.long_lines.get(lo..hi).unwrap_or(&[]);
                        if (t_lo..=t_hi).contains(&t1) {
                            let eq_lo = slice.partition_point(|e| e.t < t1);
                            let eq_hi = eq_lo + slice.get(eq_lo..).unwrap_or(&[]).partition_point(|e| e.t <= t1);
                            let before = slice.get(..eq_lo).unwrap_or(&[]);
                            let equal = slice.get(eq_lo..eq_hi).unwrap_or(&[]);
                            let after = slice.get(eq_hi..).unwrap_or(&[]);
                            let d_lo = equal.partition_point(|e| e.d < d1 - d_margin);
                            let d_hi = equal.partition_point(|e| e.d <= d1 + d_margin);
                            let near_d = equal.get(d_lo..d_hi).unwrap_or(&[]);
                            for entry in before.iter().chain(near_d).chain(after) {
                                if check_other(entry.idx, work)? {
                                    is_alone = false;
                                    break 'intervals;
                                }
                            }
                        } else {
                            for entry in slice {
                                if check_other(entry.idx, work)? {
                                    is_alone = false;
                                    break 'intervals;
                                }
                            }
                        }
                    }
                }
            }
        }
        self.alone_cache.insert(idx, is_alone);
        Ok(is_alone)
    }
}

/// The sheet of base flange `base` made from its sketch's `lines`, with the design's `features`
/// that bent it.
struct Sheet<'a, 'b> {
    index: &'a FeatureIndex<'a>,
    base: u64,
    lines: &'b mut SketchLines,
    first_split: &'b mut HashMap<usize, usize>,
    work: &'b mut usize,
}

impl Sheet<'_, '_> {
    /// The model's names for the two faces that meet at the edge the sheet named `name`, as the
    /// feature at `at` in the design saved it: the rim along it, and its panel's cap on its side.
    /// None where the saved design does not say which they are.
    fn faces(&mut self, name: &str, at: usize, depth: usize) -> Result<Option<(String, String)>> {
        let Some((edge, side)) = name.rsplit_once(':') else { return Ok(None) };
        if !matches!(side, "top" | "bottom") || depth > DEPTH {
            return Ok(None);
        }
        let b = self.base;
        if let Some(line) = edge.strip_prefix("base:") {
            return Ok(self.lines.alone(line, self.work)?.then(|| (format!("F{b}:side:{line}"), format!("F{b}:{side}"))));
        }
        let Some((tag, rim)) = edge.split_once(':') else { return Ok(None) };
        let Some((f, k)) = tag.strip_prefix('F').and_then(|rest| rest.split_once('.')) else { return Ok(None) };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
        if !digits(f) || !digits(k) {
            return Ok(None);
        }
        let (Ok(f), Ok(k)) = (f.parse::<u64>(), k.parse::<usize>()) else {
            return Ok(None);
        };
        // The flange's end rims are named from its bend's anchor, which runs the edge the other way.
        let rim = match rim {
            "far" => "far",
            "side0" => "end1",
            "side1" => "end0",
            _ => return Ok(None),
        };
        // The feature that made the flange, before the one that picked its edge, on the same sheet.
        let Some(made) = self.index.first_by_id.get(&f).copied().filter(|&idx| idx < at) else { return Ok(None) };
        let Some(feature) = self.index.features.get(made) else { return Ok(None) };
        if self.index.base_of(made).and_then(|idx| self.index.features.get(idx)).is_some_and(|x| id(x) != Some(b)) {
            return Ok(None);
        }
        // Its `k`th flange is the one made on its own `k`th edge where each edge up to it makes one.
        let Some(edges) = feature.get("edges").and_then(Value::as_array) else { return Ok(None) };
        if k >= edges.len() {
            return Ok(None);
        }
        let one_each = match kind(feature) {
            Some("sheet_hem") => true,
            Some("sheet_flange") => {
                let split = *self.first_split.entry(made).or_insert_with(|| {
                    edges.iter().position(|e| !e.get("segments").and_then(Value::as_array).is_none_or(Vec::is_empty)).unwrap_or(edges.len())
                });
                k < split
            }
            _ => false,
        };
        if !one_each {
            return Ok(None);
        }
        let Some(picked) = feature.get("edge_names").and_then(|n| n.get(k)).and_then(Value::as_str) else { return Ok(None) };
        let Some((on, _)) = self.faces(picked, made, depth + 1)? else { return Ok(None) };
        Ok(Some((format!("F{f}:{rim}:{on}"), format!("F{f}:{side}:{on}"))))
    }

    /// The model's name for the edge the sheet named `name`, as the feature at `at` saved it.
    fn renamed(&mut self, name: &str, at: usize) -> Result<Option<String>> {
        let Some((rim, cap)) = self.faces(name, at, 0)? else { return Ok(None) };
        let mut two = [rim, cap];
        two.sort();
        Ok(Some(two.join("|")))
    }
}

pub fn step(v: &mut Value) -> Result<()> {
    let Some(features) = v.get("features").and_then(Value::as_array) else { return Ok(()) };
    if features.len() > crate::document::MAX_FEATURES {
        return Err(DocError::Invalid("document too large".into()));
    }
    let has_sheet_bends = features.iter().any(|f| matches!(kind(f), Some("sheet_flange" | "sheet_hem")) && f.get("edge_names").is_some());
    if !has_sheet_bends {
        return Ok(());
    }
    let index = FeatureIndex::build(features)?;
    let mut sketch_cache: HashMap<u64, SketchLines> = HashMap::new();
    let mut first_split: HashMap<usize, usize> = HashMap::new();
    let mut work = 0usize;
    let mut total_edge_names = 0usize;
    // Every name is read as the design saved it before any is rewritten: a flange's name follows
    // the name saved for the edge of the flange it stands on.
    let mut renamed: Vec<(usize, usize, String)> = Vec::new();
    for (at, f) in features.iter().enumerate() {
        if !matches!(kind(f), Some("sheet_flange" | "sheet_hem")) {
            continue;
        }
        let Some(names) = f.get("edge_names").and_then(Value::as_array) else { continue };
        total_edge_names = total_edge_names.saturating_add(names.len());
        if total_edge_names > MAX_EDGE_NAMES {
            return Err(DocError::Invalid("too many sheet edge names during format upgrade".into()));
        }
        let Some(base_idx) = index.base_of(at) else { continue };
        let Some(base) = features.get(base_idx) else { continue };
        let (Some(b), Some(sketch)) = (id(base), base.get("sketch").and_then(Value::as_u64)) else { continue };
        let lines = match sketch_cache.entry(sketch) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(SketchLines::build(features, index.sketches.get(&sketch).copied())?),
        };
        let mut sheet = Sheet { index: &index, base: b, lines, first_split: &mut first_split, work: &mut work };
        for (k, name) in names.iter().enumerate() {
            if let Some(n) = name.as_str()
                && let Some(new) = sheet.renamed(n, at)?
            {
                renamed.push((at, k, new));
            }
        }
    }
    if let Some(Value::Array(features)) = v.get_mut("features") {
        for (at, k, new) in renamed {
            if let Some(slot) = features.get_mut(at).and_then(|f| f.get_mut("edge_names")).and_then(|e| e.get_mut(k)) {
                *slot = Value::String(new);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::{Value, json};

    /// A sketch feature of straight lines, each `(id, start, end, construction)`, between points
    /// saved where the lines end.
    fn sketch(id: u64, lines: &[(&str, [f64; 2], [f64; 2], bool)]) -> Value {
        let mut points = vec![json!({"id": "origin", "pos": [0.0, 0.0], "fixed": true})];
        let mut curves = Vec::new();
        for (line, a, b, construction) in lines {
            points.push(json!({"id": format!("{line}.start"), "pos": a, "fixed": false}));
            points.push(json!({"id": format!("{line}.end"), "pos": b, "fixed": false}));
            curves.push(json!({"id": line, "type": "line", "a": points.len() - 2, "b": points.len() - 1, "construction": construction}));
        }
        json!({"id": id, "name": format!("Sketch{id}"), "type": "sketch", "plane": {"type": "origin", "name": "XY"}, "sketch": {"points": points, "curves": curves, "constraints": []}})
    }

    /// A 100 x 60 rectangle from the origin, its lines `l1` along the front (y = 0) to `l4`.
    const RECTANGLE: [(&str, [f64; 2], [f64; 2], bool); 4] = [
        ("l1", [0.0, 0.0], [100.0, 0.0], false),
        ("l2", [100.0, 0.0], [100.0, 60.0], false),
        ("l3", [100.0, 60.0], [0.0, 60.0], false),
        ("l4", [0.0, 60.0], [0.0, 0.0], false),
    ];

    /// The `edge_names` of each feature once the step has run on a design of `features`.
    fn upgraded(features: Value) -> Vec<Value> {
        let mut v = json!({"features": features});
        super::step(&mut v).unwrap();
        v["features"].as_array().unwrap().iter().map(|f| f["edge_names"].clone()).collect()
    }

    #[test]
    fn a_flanges_and_a_hems_edges_take_the_models_names() {
        let mut lines = RECTANGLE.to_vec();
        lines.push(("l5", [0.0, 30.0], [100.0, 30.0], true));
        let split = json!({"point": [0, 30, 1], "segments": [{"from": 0, "to": 0.5}, {"from": 0.5, "to": 1, "height": "5"}]});
        let names = upgraded(json!([
            sketch(1, &lines),
            {"id": 2, "name": "BaseFlange1", "type": "sheet_base", "sketch": 1, "profiles": {"type": "all"}},
            {"id": 3, "name": "EdgeFlange1", "type": "sheet_flange", "edges": [[50, 0, 1], [100, 30, 0]], "edge_names": ["base:l1:top", "base:l2:bottom"]},
            {"id": 4, "name": "Hem1", "type": "sheet_hem", "edges": [[50, 1, 20], [100, 0, 10], [100, 60, 10]],
             "edge_names": ["F3.0:far:top", "F3.1:side0:bottom", "F3.1:side1:top"]},
            {"id": 5, "name": "EdgeFlange2", "type": "sheet_flange", "edges": [[50, -3, 15]], "edge_names": ["F4.0:far:bottom"]},
            {"id": 6, "name": "EdgeFlange3", "type": "sheet_flange", "edges": [split, [50, 60, 1]], "edge_names": ["base:l4:top", "base:l3:top"]},
            {"id": 7, "name": "Hem2", "type": "sheet_hem", "edges": [[0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0]],
             "edge_names": ["F6.1:far:top", "F6.0:far:top", "F3.0:bend:top", "F9.0:far:top", "base:l5:top", "base:l9:top", "", "F2.0:far:top"]},
            {"id": 8, "name": "Fillet1", "type": "fillet", "edge_names": ["base:l1:top"]},
            {"id": 9, "name": "EdgeFlange4", "type": "sheet_flange", "edges": [[50, 0, 0]], "edge_names": ["base:l1:bottom"]},
        ]));
        // The base flange's top and bottom where they meet its rims on the sketch's lines.
        assert_eq!(names[2], json!(["F2:side:l1|F2:top", "F2:bottom|F2:side:l2"]));
        assert_eq!(names[8], json!(["F2:bottom|F2:side:l1"]));
        // Flange F3.0 is the one on F3's first edge, the base's rim on l1, F3.1 the one on its
        // second, the rim on l2: the first's far rim with its top, and the second's ends, `side0`
        // the model's `end1`, `side1` its `end0`.
        assert_eq!(
            names[3],
            json!(["F3:far:F2:side:l1|F3:top:F2:side:l1", "F3:bottom:F2:side:l2|F3:end1:F2:side:l2", "F3:end0:F2:side:l2|F3:top:F2:side:l2"])
        );
        // A flange on the hem's far edge: the hem stands on F3's far rim, as its own name says.
        assert_eq!(names[4], json!(["F4:bottom:F3:far:F2:side:l1|F4:far:F3:far:F2:side:l1"]));
        // A split edge's own name is the edge's.
        assert_eq!(names[5], json!(["F2:side:l4|F2:top", "F2:side:l3|F2:top"]));
        // Left: F6's flanges from its split edge on (it makes one for each run of equal segments),
        // a bend, a flange of a feature after the hem, a construction line, a line the sketch does
        // not have, no name, and a flange of the base flange, which bends none; and a fillet's
        // edges, which are no sheet's.
        assert_eq!(
            names[6],
            json!(["F6.1:far:top", "F6.0:far:top", "F3.0:bend:top", "F9.0:far:top", "base:l5:top", "base:l9:top", "", "F2.0:far:top"])
        );
        assert_eq!(names[7], json!(["base:l1:top"]));
        assert_eq!(names[0], Value::Null);
    }

    #[test]
    fn names_on_a_sheet_the_design_does_not_say_or_along_a_notch_are_left() {
        let base = |id: u64, sketch: u64, body: Option<&str>| {
            let mut f = json!({"id": id, "name": format!("BaseFlange{id}"), "type": "sheet_base", "sketch": sketch, "profiles": {"type": "all"}});
            if let Some(body) = body {
                f["body_names"] = json!([body]);
            }
            f
        };
        let flange = |id: u64, body: Option<&str>, name: &str| {
            let mut f = json!({"id": id, "name": format!("EdgeFlange{id}"), "type": "sheet_flange", "edges": [[50, 0, 1]], "edge_names": [name]});
            if let Some(body) = body {
                f["body"] = json!(body);
            }
            f
        };
        // Two sheets: a flange on the body whose name only base flange 2 keeps is on F2's sheet, and
        // so is the flange on that flange; one that names no body, or a body whose name no feature
        // keeps (base flange 4's), could be on either.
        let names = upgraded(json!([
            sketch(1, &RECTANGLE),
            base(2, 1, Some("Plate")),
            sketch(3, &RECTANGLE),
            base(4, 3, None),
            flange(5, Some("Plate"), "base:l1:top"),
            flange(6, None, "base:l1:top"),
            flange(7, Some("Body2"), "base:l1:top"),
            flange(8, Some("Plate"), "F5.0:far:top"),
        ]));
        assert_eq!(names[4], json!(["F2:side:l1|F2:top"]));
        assert_eq!(names[5], json!(["base:l1:top"]));
        assert_eq!(names[6], json!(["base:l1:top"]));
        assert_eq!(names[7], json!(["F5:far:F2:side:l1|F5:top:F2:side:l1"]));
        // One base flange, and a mirror that may have copied its body: the body's name says the
        // sheet, no name does not.
        let names = upgraded(json!([
            sketch(1, &RECTANGLE),
            base(2, 1, Some("Plate")),
            {"id": 3, "name": "Mirror1", "type": "mirror", "bodies": ["Plate"]},
            flange(4, None, "base:l1:top"),
            flange(5, Some("Plate"), "base:l1:top"),
        ]));
        assert_eq!((&names[3], &names[4]), (&json!(["base:l1:top"]), &json!(["F2:side:l1|F2:top"])));
        // A part mirrored once a flange is on (`kc/3353-mirrored-sheet-body-keeps-its-flat-pattern`):
        // the flange before the mirror was picked on the one sheet there was; one after it, on
        // either copy.
        let names = upgraded(json!([
            sketch(1, &RECTANGLE),
            base(2, 1, Some("Plate")),
            flange(3, None, "base:l1:top"),
            {"id": 4, "name": "Mirror1", "type": "mirror", "bodies": ["Plate"], "body_names": ["Plate (2)"]},
            flange(5, None, "base:l3:top"),
            flange(6, None, "F3.0:far:top"),
        ]));
        assert_eq!((&names[2], &names[4]), (&json!(["F2:side:l1|F2:top"]), &json!(["base:l3:top"])));
        assert_eq!(names[5], json!(["F3.0:far:top"]));
        // A front edge notched between x = 40 and 60: l1 and l5 lie in line, and the sheet named
        // the edges along both after l1. The notch's own sides and the others are each alone.
        let notched = [
            ("l1", [0.0, 0.0], [40.0, 0.0], false),
            ("l2", [40.0, 0.0], [40.0, 20.0], false),
            ("l3", [40.0, 20.0], [60.0, 20.0], false),
            ("l4", [60.0, 20.0], [60.0, 0.0], false),
            ("l5", [60.0, 0.0], [100.0, 0.0], false),
            ("l6", [100.0, 0.0], [100.0, 60.0], false),
            ("l7", [100.0, 60.0], [0.0, 60.0], false),
            ("l8", [0.0, 60.0], [0.0, 0.0], false),
        ];
        let names = upgraded(json!([
            sketch(1, &notched),
            base(2, 1, None),
            {"id": 3, "name": "EdgeFlange3", "type": "sheet_flange", "edges": [[80, 0, 1], [100, 30, 1], [50, 20, 1]],
             "edge_names": ["base:l1:top", "base:l6:top", "base:l3:top"]},
        ]));
        assert_eq!(names[2], json!(["base:l1:top", "F2:side:l6|F2:top", "F2:side:l3|F2:top"]));
        // A converted sheet: its flanges were numbered in the order the conversion reached its
        // faces, and the design's one sheet is no base flange's.
        let names = upgraded(json!([
            {"id": 1, "name": "Box1", "type": "box", "body_names": ["Tray"]},
            {"id": 2, "name": "C1", "type": "sheet_convert", "body": "Tray", "face": [50, 30, 0], "bends": [[50, 0, 0]], "edge_names": ["F1:-y|F1:-z"]},
            flange(3, Some("Tray"), "F2.0:far:top"),
            flange(4, None, "F2.0:far:top"),
        ]));
        assert_eq!(names[1], json!(["F1:-y|F1:-z"]), "a conversion's bend edges are its body's");
        assert_eq!((&names[2], &names[3]), (&json!(["F2.0:far:top"]), &json!(["F2.0:far:top"])));
    }

    #[test]
    fn indexed_collinearity_matches_brute_force_across_angles_and_large_parallel_sketches_run_fast() {
        // Brute-force reference matching the original algorithm.
        let ref_alone = |lines: &[(String, [f64; 2], [f64; 2])], line: &str| -> bool {
            let Some((_, a, b)) = lines.iter().find(|(id, _, _)| id == line) else { return false };
            (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-9
                && lines.iter().filter(|(id, _, _)| id != line).all(|(_, p, q)| !super::along(*p, *q, *a, *b) && !super::along(*a, *b, *p, *q))
        };
        // Angles covering all sectors, wrap-around near 0/pi, far coordinates, collinear pairs and tiny lines.
        let spec: Vec<(String, [f64; 2], [f64; 2], bool)> = vec![
            ("h1".into(), [0.0, 0.0], [10.0, 0.0], false),
            ("h2".into(), [20.0, 0.0], [30.0, 0.0], false),
            ("h_alone".into(), [0.0, 5.0], [10.0, 5.0], false),
            ("v1".into(), [0.0, 0.0], [0.0, 10.0], false),
            ("v2".into(), [0.0, 30.0], [0.0, 40.0], false),
            ("v_alone".into(), [5.0, 0.0], [5.0, 10.0], false),
            ("diag1".into(), [0.0, 0.0], [10.0, 10.0], false),
            ("diag2".into(), [20.0, 20.0], [30.0, 30.0], false),
            ("diag_alone".into(), [1.0, 0.0], [11.0, 10.0], false),
            ("wrap1".into(), [0.0, 100.0], [100.0, 100.0 + 1e-8], false),
            ("wrap2".into(), [200.0, 100.0], [300.0, 100.0 - 1e-8], false),
            ("far1".into(), [1e8, 50.0], [1e8 + 10.0, 50.0], false),
            ("far2".into(), [1e8 + 20.0, 50.0], [1e8 + 30.0, 50.0], false),
            ("far_alone".into(), [1e8, 55.0], [1e8 + 10.0, 55.0], false),
            ("tiny".into(), [0.0, 5.0], [1e-4, 5.0 + 1e-7], false),
            ("near_tiny_alone".into(), [0.0, 25.0], [10.0, 25.0], false),
        ];
        let refs: Vec<(&str, [f64; 2], [f64; 2], bool)> = spec.iter().map(|(id, a, b, c)| (id.as_str(), *a, *b, *c)).collect();
        let raw_lines: Vec<(String, [f64; 2], [f64; 2])> = spec.iter().map(|(id, a, b, _)| (id.clone(), *a, *b)).collect();
        let sk = sketch(1, &refs);
        let features = vec![sk];
        let mut indexed = super::SketchLines::build(&features, Some(0)).unwrap();
        let mut work = 0usize;
        for (id, _, _, _) in &spec {
            assert_eq!(indexed.alone(id, &mut work).unwrap(), ref_alone(&raw_lines, id), "mismatch on `{id}`");
        }

        // Large sketch of 5,000 parallel non-collinear lines + 500 flanges picking distinct lines.
        let parallel: Vec<(String, [f64; 2], [f64; 2], bool)> =
            (0..5_000).map(|i| (format!("p{i}"), [0.0, i as f64], [100.0, i as f64], false)).collect();
        let parallel_refs: Vec<(&str, [f64; 2], [f64; 2], bool)> = parallel.iter().map(|(id, a, b, c)| (id.as_str(), *a, *b, *c)).collect();
        let mut feat_list =
            vec![sketch(1, &parallel_refs), json!({"id": 2, "name": "Base", "type": "sheet_base", "sketch": 1, "profiles": {"type": "all"}})];
        for i in 0..500usize {
            feat_list.push(json!({
                "id": 3 + i as u64,
                "name": format!("F{i}"),
                "type": "sheet_flange",
                "edges": [[50, i, 1]],
                "edge_names": [format!("base:p{i}:top")],
            }));
        }
        let mut v = json!({"features": feat_list});
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][2]["edge_names"], json!(["F2:side:p0|F2:top"]));
        assert_eq!(v["features"][501]["edge_names"], json!(["F2:side:p499|F2:top"]));

        // Adversarial input exceeding MAX_EDGE_NAMES is rejected cleanly.
        let too_many_edges = vec![json!("base:p0:top"); super::MAX_EDGE_NAMES + 1];
        let mut bloated = json!({"features": [
            sketch(1, &RECTANGLE),
            {"id": 2, "type": "sheet_base", "sketch": 1},
            {"id": 3, "type": "sheet_flange", "edges": [[0, 0, 0]], "edge_names": too_many_edges},
        ]});
        assert!(super::step(&mut bloated).is_err());

        // Adversarial tiny-segment cluster exceeding MAX_LINE_CHECKS is rejected cleanly.
        let tiny_lines: Vec<(String, [f64; 2], [f64; 2], bool)> =
            (0..2_500).map(|i| (format!("t{i}"), [0.0, i as f64], [1e-4, i as f64], false)).collect();
        let tiny_refs: Vec<(&str, [f64; 2], [f64; 2], bool)> = tiny_lines.iter().map(|(id, a, b, c)| (id.as_str(), *a, *b, *c)).collect();
        let edge_names: Vec<Value> = (0..1_000).map(|i| json!(format!("base:t{i}:top"))).collect();
        let mut adversarial = json!({"features": [
            sketch(1, &tiny_refs),
            {"id": 2, "type": "sheet_base", "sketch": 1},
            {"id": 3, "type": "sheet_flange", "edges": [[0, 0, 0]], "edge_names": edge_names},
        ]});
        let err = super::step(&mut adversarial).unwrap_err().to_string();
        assert!(err.contains("sheet edge comparisons"), "{err}");
    }
}
