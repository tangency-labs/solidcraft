//! Copy and paste of sketch geometry (#1480). `sketch.copy` keeps the chosen curves and points
//! with the constraints and dimensions that lie wholly among them (a dimension keeps its
//! expression), plus `Equal` constraints referencing curves left behind in the same sketch;
//! `sketch.paste` adds an independent copy to the sketch being edited, at a point or offset from
//! the original, as one undo step, and selects it. Positional constraints that reach outside the
//! copied geometry (to the origin, to projected edges, to other curves) are left behind, as are
//! fixes, so the pasted geometry can be moved straight away. External `Equal` constraints are
//! dropped when pasting into another sketch or design, or if the referenced curve no longer
//! exists.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_doc::Document;
use solidcraft_geom::Aabb2;
use solidcraft_sketch::{ConstraintKind, CurveKind, LinkKind, LinkSource, Sketch};

use super::sketch::edit;
use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, string_list, vec2};
use crate::schema::sketch_clipboard::{Copied, CopyParams, CutParams, PasteParams, Pasted, ReplaceParams, Replaced};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Sel, Session};

const COPY_CONTRACT: TypedContract<CopyParams> = TypedContract::new();
const CUT_CONTRACT: TypedContract<CutParams> = TypedContract::new();
const PASTE_CONTRACT: TypedContract<PasteParams> = TypedContract::new();
const REPLACE_CONTRACT: TypedContract<ReplaceParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.copy", "Copy Sketch Entities", crate::checked!("sketch.copy", copy)).icon("copy")
        .noundo()
        .enabled(in_sketch)
        .contract(&COPY_CONTRACT)
        .output(output_schema_for::<Copied>)
        .params("entities?: [curve or point ids] (default: the selected sketch geometry); the constraints and dimensions among them (and `Equal` constraints to other curves in the same sketch) come along"),
    CommandSpec::new("sketch.cut", "Cut Sketch Entities", crate::checked!("sketch.cut", cut)).icon("cut")
        .enabled(in_sketch)
        .contract(&CUT_CONTRACT)
        .output(output_schema_for::<Copied>)
        .params("entities?: [curve or point ids] (default: the selected sketch geometry): copies them as sketch.copy does, then deletes them (one undo step)"),
    CommandSpec::new("sketch.paste", "Paste Sketch Entities", crate::checked!("sketch.paste", paste))
        .icon("paste")
        .enabled(in_sketch)
        .contract(&PASTE_CONTRACT)
        .output(output_schema_for::<Pasted>)
        .params(
        "at?: [x, y] (sketch point the copy's centre goes to) | translate?: [x, y]; without either, offset from the original in its own sketch and at the same coordinates in another; sketch?: target (default: the active sketch)",
    ),
    CommandSpec::new("sketch.replace", "Replace Sketch", crate::checked!("sketch.replace", replace))
        .icon("sketch")
        .contract(&REPLACE_CONTRACT)
        .output(output_schema_for::<Replaced>)
        .params(
        "sketch?: the sketch to replace (default: the active sketch); from: the sketch whose contents take its place (id or name); file?: a design file `from` is in (default: this design). The sketch keeps its plane, place in the timeline and the features built on it; its curves, points, constraints, dimensions and texts become a copy of `from`'s, at the same sketch coordinates (one undo step)",
    ),
];

/// A curve of the clipboard; its points are indices into [`SketchClip::points`].
#[derive(Clone, Debug)]
pub struct ClipCurve {
    pub kind: CurveKind,
    pub construction: bool,
    pub centerline: bool,
    pub reversed: bool,
    pub infinite: bool,
}

/// A constraint or dimension of the clipboard, over clipboard points and curves. A dimension
/// keeps the expression and unit of its parameter.
#[derive(Clone, Debug)]
pub struct ClipConstraint {
    pub kind: ConstraintKind,
    pub param: Option<(String, String)>,
    pub driven: bool,
    pub text: Option<Vec2>,
}

/// An `Equal` constraint between a clipboard curve and an uncopied curve left behind in
/// [`SketchClip::source_sketch`]. Re-attached on paste back into that sketch while the external
/// curve still exists by id; dropped on paste into another sketch or design.
#[derive(Clone, Debug)]
pub struct ClipExternalEqual {
    /// Index into [`SketchClip::curves`] for the copied curve.
    pub copied: usize,
    /// Id of the uncopied curve in [`SketchClip::source_sketch`].
    pub external_id: String,
    /// `true` when the external curve was `a` in `Equal { a, b }`, `false` when it was `b`.
    pub external_first: bool,
}

/// Sketch geometry copied with `sketch.copy`, in the sketch coordinates it was copied from.
#[derive(Clone, Debug, Default)]
pub struct SketchClip {
    /// The sketch it was copied from (`u64::MAX` after `sketch.cut` or for another design, so a
    /// paste does not offset from the original).
    pub sketch: u64,
    /// Original sketch feature id for re-attaching [`Self::external_equals`] (`None` for
    /// whole-sketch copies or copies from another design).
    pub source_sketch: Option<u64>,
    pub points: Vec<Vec2>,
    pub curves: Vec<ClipCurve>,
    pub constraints: Vec<ClipConstraint>,
    pub external_equals: Vec<ClipExternalEqual>,
    /// Text sources (their outlines are made again on paste). A path, when kept, is the index of
    /// a clipboard curve.
    pub texts: Vec<(LinkSource, Option<usize>)>,
    /// Lone points (not used by a copied curve), as indices into `points`.
    pub lone: Vec<usize>,
    /// Centre of the copied geometry's bounding box: what `at` places.
    pub center: Vec2,
    /// Size of the bounding box (the larger side), for the paste offset.
    pub extent: f64,
    /// Pastes made without a position since the copy (each lands one offset further on).
    pub pastes: u32,
    /// The clipboard point that stands for the sketch origin (in a whole-sketch copy): a paste
    /// ties it to the origin of the sketch it goes to rather than adding a point.
    pub origin: Option<usize>,
}

/// The id of a sketch's origin point.
const ORIGIN: &str = "origin";

impl SketchClip {
    /// This clipboard as another design pastes it (#9258): at the same sketch coordinates (its
    /// sketch ids mean nothing there), and with nothing that names the copied design's
    /// parameters. A dimension whose expression names a parameter keeps its value, in its unit
    /// (another design's parameter of the same name would be another thing); a text keeps its
    /// words, height and angle as numbers.
    pub fn for_another_design(&self) -> SketchClip {
        let mut clip = self.clone();
        clip.sketch = u64::MAX;
        clip.source_sketch = None;
        clip.external_equals.clear();
        clip.pastes = 0;
        for c in &mut clip.constraints {
            let Some((expr, unit)) = c.param.as_mut() else { continue };
            if solidcraft_doc::expr::references(expr).is_empty() {
                continue;
            }
            let v = c.kind.value().unwrap_or_default();
            if solidcraft_doc::expr::unit_info(unit).is_none() {
                *unit = if c.kind.is_angle() { "rad" } else { "mm" }.into();
            }
            let value = if c.kind.is_angle() { solidcraft_doc::expr::Value::angle(v) } else { solidcraft_doc::expr::Value::length(v) };
            let shown = solidcraft_doc::expr::value_in_unit(value, unit);
            *expr = format!("{}", if shown.is_finite() { shown } else { 0.0 });
        }
        for (src, _) in &mut clip.texts {
            if let LinkSource::Text { exprs, .. } = src {
                *exprs = Default::default();
            }
        }
        clip
    }
}

/// Curve and point ids named by `entities`, or the selected sketch geometry.
fn chosen(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    if p.get("entities").is_some() {
        return string_list(cmd, p, "entities");
    }
    Ok(s.selection
        .iter()
        .filter_map(|sel| match sel {
            Sel::SketchCurve { id, .. } | Sel::SketchPoint { id } => Some(id.clone()),
            _ => None,
        })
        .collect())
}

fn text_of(sk: &Sketch, ci: usize) -> Option<&str> {
    let link = sk.curves.get(ci)?.link.as_deref()?;
    sk.link(link).filter(|l| l.kind == LinkKind::Text).map(|l| l.id.as_str())
}

fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.copy";
    let id = sketch_store::target(s, p, cmd)?;
    let ids = chosen(s, p, cmd)?;
    if ids.is_empty() {
        return Err(bad(cmd, "select the sketch curves or points to copy"));
    }
    let sk = sketch_store::read(&s.doc, id)?;
    let (clip, out) = clip_of(&s.doc, sk, id, &ids, false, cmd)?;
    s.sketch_clipboard = Some(clip);
    reply(cmd, out)
}

fn external_equal(cmap: &BTreeMap<usize, usize>, sk: &Sketch, a: usize, b: usize) -> Option<ClipExternalEqual> {
    let (copied, ext_idx, external_first) = match (cmap.get(&a).copied(), cmap.get(&b).copied()) {
        (Some(copied), None) => (copied, b, false),
        (None, Some(copied)) => (copied, a, true),
        _ => return None,
    };
    let external_id = sk.curves.get(ext_idx)?.id.clone();
    Some(ClipExternalEqual { copied, external_id, external_first })
}

/// The clipboard copy of the entities `ids` of sketch `id` (`sk`, in `doc`), and what it holds.
/// `whole` is a copy of the whole sketch for [`replace`]: its fixes come along, its constraints to
/// the origin stay tied to the origin of the sketch it goes to, and an empty sketch is a copy too.
fn clip_of(doc: &Document, sk: &Sketch, id: u64, ids: &[String], whole: bool, cmd: &str) -> Result<(SketchClip, Copied)> {
    let (mut cs, mut ps, mut texts): (Vec<usize>, Vec<usize>, Vec<String>) = (Vec::new(), Vec::new(), Vec::new());
    for e in ids {
        if let Some(c) = sk.curve_index(e) {
            match text_of(sk, c) {
                Some(t) if !texts.iter().any(|x| x == t) => texts.push(t.to_string()),
                Some(_) => {}
                None if !cs.contains(&c) => cs.push(c),
                None => {}
            }
        } else if let Some(q) = sk.resolve_point(e) {
            match sk.points.get(q).and_then(|x| x.link.as_deref()).and_then(|l| sk.link(l)).filter(|l| l.kind == LinkKind::Text) {
                Some(l) if !texts.contains(&l.id) => texts.push(l.id.clone()),
                Some(_) => {}
                None if !ps.contains(&q) => ps.push(q),
                None => {}
            }
        } else {
            return Err(bad(cmd, format!("unknown sketch entity `{e}`")));
        }
    }
    let mut clip = SketchClip { sketch: id, source_sketch: (!whole).then_some(id), ..Default::default() };
    // Points: those of the copied curves, then the lone ones, each once.
    let mut pmap: BTreeMap<usize, usize> = BTreeMap::new();
    if whole && let Some(o) = sk.points.iter().position(|q| q.id == ORIGIN) {
        clip.points.push(Vec2::ZERO);
        clip.origin = Some(0);
        pmap.insert(o, 0);
    }
    let take = |q: usize, clip: &mut SketchClip, pmap: &mut BTreeMap<usize, usize>| -> usize {
        *pmap.entry(q).or_insert_with(|| {
            clip.points.push(sk.point(q).unwrap_or_default());
            clip.points.len() - 1
        })
    };
    let mut cmap: BTreeMap<usize, usize> = BTreeMap::new();
    for &c in &cs {
        let Some(cu) = sk.curves.get(c) else { continue };
        let mut kind = cu.kind.clone();
        let used: Vec<(usize, usize)> = kind.point_ids().into_iter().map(|q| (q, take(q, &mut clip, &mut pmap))).collect();
        kind.map_points(&|q| used.iter().find(|(o, _)| *o == q).map_or(q, |(_, n)| *n));
        cmap.insert(c, clip.curves.len());
        // Projected geometry comes in as ordinary geometry.
        clip.curves.push(ClipCurve { kind, construction: cu.construction, centerline: cu.centerline, reversed: cu.reversed, infinite: cu.infinite });
    }
    for &q in &ps {
        let known = pmap.contains_key(&q);
        let n = take(q, &mut clip, &mut pmap);
        if !known {
            clip.lone.push(n);
        }
    }
    for c in &sk.constraints {
        // Pattern or offset relations stay with the original, and so do the fixes of a part copy.
        if matches!(c.kind, ConstraintKind::PatternPoint { .. } | ConstraintKind::PatternRadius { .. } | ConstraintKind::OffsetDistance { .. })
            || (!whole && matches!(c.kind, ConstraintKind::Fix { .. }))
        {
            continue;
        }
        let mut kind = c.kind.clone();
        if !kind.remap(&|q| pmap.get(&q).copied(), &|k| cmap.get(&k).copied()) {
            if !whole
                && let ConstraintKind::Equal { a, b } = c.kind
                && let Some(eq) = external_equal(&cmap, sk, a, b)
            {
                clip.external_equals.push(eq);
            }
            continue;
        }
        let param = c.param.as_ref().map(|name| match doc.param(name) {
            Some(x) => (x.expr.clone(), x.unit.clone()),
            None => (format!("{}", kind.value().unwrap_or_default()), if kind.is_angle() { "rad".into() } else { "mm".into() }),
        });
        clip.constraints.push(ClipConstraint { kind, param, driven: c.driven, text: c.text });
    }
    for t in &texts {
        let Some(mut src) = sk.link(t).map(|l| l.source.clone()) else { continue };
        let LinkSource::Text { style, .. } = &mut src else { continue };
        // A text on a path copied with its path follows the copy; otherwise it is set straight.
        let path = style.path.as_deref().and_then(|pid| sk.curve_index(pid)).and_then(|pc| cmap.get(&pc).copied());
        if path.is_none() {
            style.path = None;
        }
        clip.texts.push((src, path));
    }
    // Bounds of everything copied, as drawn.
    let mut bounds = Aabb2::EMPTY;
    let mut add = |q: Vec2| bounds.add(q);
    let mut drawn: Vec<usize> = cs.clone();
    for t in &texts {
        drawn.extend(sk.link_curves(t));
    }
    for c in drawn {
        sk.polyline(c).into_iter().for_each(&mut add);
    }
    for &n in &clip.lone {
        if let Some(q) = clip.points.get(n) {
            add(*q);
        }
    }
    if !bounds.is_empty() {
        clip.center = bounds.center();
        let size = bounds.size();
        clip.extent = size.x.max(size.y);
    } else if !whole {
        return Err(bad(cmd, "nothing to copy"));
    }
    let out = Copied {
        curves: clip.curves.len() + texts.iter().map(|t| sk.link_curves(t).len()).sum::<usize>(),
        points: clip.lone.len(),
        texts: texts.len(),
        constraints: clip.constraints.len() + clip.external_equals.len(),
        center: [clip.center.x, clip.center.y],
        param_warnings: Vec::new(),
    };
    Ok((clip, out))
}

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn cut(s: &mut Session, p: &Value) -> Result<Value> {
    // `q["entities"] = …` below panics on anything but an object.
    if !(p.is_object() || p.is_null()) {
        return Err(bad("sketch.cut", "params must be an object"));
    }
    let entities = chosen(s, p, "sketch.cut")?;
    let mut q = if p.is_null() { json!({}) } else { p.clone() };
    q["entities"] = json!(entities);
    let out = copy(s, &q)?;
    let delete = super::find_command("sketch.delete").ok_or_else(|| bad("sketch.cut", "sketch.delete is missing"))?;
    let mut d = json!({ "entities": entities });
    if let Some(sk) = p.get("sketch") {
        d["sketch"] = sk.clone();
    }
    delete.run_nested(s, &d)?;
    s.selection.clear();
    // The cut geometry is gone: a paste back into this sketch goes where it was, not offset.
    if let Some(c) = s.sketch_clipboard.as_mut() {
        c.sketch = u64::MAX;
    }
    Ok(out)
}

/// The step a paste without a position moves by: a tenth of the copy's size, rounded up to 1, 2
/// or 5 times a power of ten (at least 1 mm).
fn paste_step(extent: f64) -> f64 {
    let want = (extent * 0.1).max(1.0);
    let mag = 10f64.powf(want.log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * mag).find(|x| *x >= want - 1e-9).unwrap_or(want)
}

fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.paste";
    // `p["sketch"] = …` below panics on anything but an object.
    if !(p.is_object() || p.is_null()) {
        return Err(bad(cmd, "params must be an object"));
    }
    let Some(clip) = s.sketch_clipboard.clone() else { return Err(bad(cmd, "nothing copied (sketch.copy first)")) };
    let target = sketch_store::target(s, p, cmd)?;
    let positioned = p.get("at").is_some() || p.get("translate").is_some();
    let t = if let Some(v) = p.get("at") {
        vec2(v).ok_or_else(|| bad(cmd, "`at` must be [x, y]"))? - clip.center
    } else if let Some(v) = p.get("translate") {
        vec2(v).ok_or_else(|| bad(cmd, "`translate` must be [x, y]"))?
    } else if target == clip.sketch {
        let d = paste_step(clip.extent) * f64::from(clip.pastes.saturating_add(1));
        Vec2::new(d, -d)
    } else {
        Vec2::ZERO
    };
    if !(t.x.is_finite() && t.y.is_finite()) {
        return Err(bad(cmd, "the paste position must be finite"));
    }
    let mut p = p.clone();
    p["sketch"] = json!(target);
    let same_sketch = clip.source_sketch == Some(target);
    let ((curves, lone, constraints), info) = edit(s, &p, cmd, false, |sk, doc| paste_clip(sk, doc, &clip, t, same_sketch))?;
    if !positioned
        && target == clip.sketch
        && let Some(c) = s.sketch_clipboard.as_mut()
    {
        c.pastes = c.pastes.saturating_add(1);
    }
    // The pasted geometry is selected, ready to move.
    if s.active_sketch == Some(target) {
        s.selection = curves
            .iter()
            .map(|id| Sel::SketchCurve { id: id.clone(), sketch: None })
            .chain(lone.iter().map(|id| Sel::SketchPoint { id: id.clone() }))
            .collect();
    }
    reply(cmd, Pasted { curves, points: lone, constraints, translate: [t.x, t.y], sketch: info, param_warnings: Vec::new() })
}

/// The feature id of the sketch `key` (an id or a name, as [`sketch_store::target`] reads them)
/// in `doc`.
fn sketch_in(doc: &Document, key: &Value, cmd: &str) -> Result<u64> {
    let (name, found) = match key {
        Value::Number(n) => {
            let id = n.as_u64().ok_or_else(|| bad(cmd, "`from` must be a sketch id or name"))?;
            (id.to_string(), doc.feature(id))
        }
        Value::String(x) => (x.clone(), doc.find_feature(x)),
        _ => return Err(bad(cmd, "`from` must be a sketch id or name")),
    };
    let f = found.ok_or_else(|| bad(cmd, format!("no sketch `{name}`")))?;
    if !matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. }) {
        return Err(bad(cmd, format!("`{name}` is not a sketch")));
    }
    Ok(f.id)
}

/// A copy of every curve, point, constraint, dimension and text of sketch `id` in `doc`.
fn whole_clip(doc: &Document, id: u64, cmd: &str) -> Result<SketchClip> {
    let sk = sketch_store::read(doc, id)?;
    let ids: Vec<String> = sk.curves.iter().map(|c| c.id.clone()).chain(sk.points.iter().filter(|q| q.id != ORIGIN).map(|q| q.id.clone())).collect();
    Ok(clip_of(doc, sk, id, &ids, true, cmd)?.0)
}

/// `sketch.replace` (#11125): the sketch keeps its feature (plane, place in the timeline, name,
/// display options and what is built on it) and takes a copy of another sketch's contents, as a
/// whole-sketch copy and paste would. Projected geometry comes as ordinary geometry; patterns,
/// offsets and formula curves come as the geometry they made. The dimension parameters the old
/// contents had, when nothing else uses them, go with them.
fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.replace";
    if !(p.is_object() || p.is_null()) {
        return Err(bad(cmd, "params must be an object"));
    }
    let target = sketch_store::target(s, p, cmd)?;
    let from = p.get("from").filter(|v| !v.is_null()).ok_or_else(|| bad(cmd, "`from`: the sketch to take the contents of"))?;
    let file = match p.get("file").filter(|v| !v.is_null()) {
        None => None,
        Some(Value::String(f)) if !f.trim().is_empty() => Some(f.clone()),
        Some(_) => return Err(bad(cmd, "`file` must be the path of a design file")),
    };
    let clip = match &file {
        Some(path) => {
            let (other, _) =
                solidcraft_io::read_design_file(path, None::<fn(solidcraft_io::DesignPreview)>).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
            let source = sketch_in(&other, from, cmd)?;
            whole_clip(&other, source, cmd)?.for_another_design()
        }
        None => {
            let source = sketch_in(&s.doc, from, cmd)?;
            if source == target {
                return Err(bad(cmd, "a sketch can't be replaced by itself"));
            }
            whole_clip(&s.doc, source, cmd)?
        }
    };
    let ((curves, points, constraints), info) = edit(s, &json!({ "sketch": target }), cmd, false, |sk, doc| {
        let old: Vec<String> = sk.constraints.iter().filter_map(|c| c.param.clone()).collect();
        // Emptied, but its id counters kept: the new contents' ids carry on from the old ones, so
        // a reference to an old curve or point (a sweep path, a profile picked by its curves)
        // finds nothing and says so, rather than taking a new, unrelated curve that happens to get
        // the same id.
        let mut fresh = sk.clone();
        fresh.points.retain(|q| q.id == ORIGIN);
        fresh.curves.clear();
        fresh.constraints.clear();
        fresh.patterns.clear();
        fresh.links.clear();
        fresh.offsets.clear();
        fresh.freeform_offsets.clear();
        fresh.wires.clear();
        fresh.equations.clear();
        // The old contents' own dimension parameters go with them; one something else uses stays.
        sketch_store::write(doc, target, fresh.clone())?;
        for name in &old {
            if doc.param(name).is_some_and(|x| x.model) {
                let _ = doc.remove_param(name);
            }
        }
        *sk = fresh;
        paste_clip(sk, doc, &clip, Vec2::ZERO, false)
    })?;
    if s.active_sketch == Some(target) {
        s.selection.clear();
    }
    let source = match from {
        Value::String(x) => x.clone(),
        other => other.to_string(),
    };
    reply(cmd, Replaced { sketch_id: target, from: source, file, curves, points, constraints, sketch: info, param_warnings: Vec::new() })
}

/// The ids of what a paste added: curves (those of texts included), lone points, constraints.
type PastedIds = (Vec<String>, Vec<String>, Vec<String>);

/// Add a copy of `clip`, moved by `t`, to `sk`. When `same_sketch` is `true`, `Equal`
/// constraints referencing curves left behind in the source sketch are re-attached where those
/// curves still exist by id.
fn paste_clip(sk: &mut Sketch, doc: &mut Document, clip: &SketchClip, t: Vec2, same_sketch: bool) -> Result<PastedIds> {
    if sk.curves.len().saturating_add(clip.curves.len()) > solidcraft_sketch::limits::max_curves() {
        return Err(solidcraft_sketch::SketchError::TooLarge.into());
    }
    let mut pts = Vec::with_capacity(clip.points.len());
    let origin = sk.points.iter().position(|q| q.id == ORIGIN);
    for (i, q) in clip.points.iter().enumerate() {
        match origin.filter(|_| clip.origin == Some(i)) {
            Some(o) => pts.push(o),
            None => pts.push(sk.add_point(*q + t, None)?),
        }
    }
    let mut made = Vec::with_capacity(clip.curves.len());
    for c in &clip.curves {
        let mut kind = c.kind.clone();
        kind.map_points(&|q| pts.get(q).copied().unwrap_or(usize::MAX));
        let i = sk.add_curve(kind, None)?;
        if let Some(cu) = sk.curves.get_mut(i) {
            cu.construction = c.construction;
            cu.centerline = c.centerline;
            cu.reversed = c.reversed;
            cu.infinite = c.infinite;
        }
        made.push(i);
    }
    let mut cons = Vec::new();
    for c in &clip.constraints {
        let mut kind = c.kind.clone();
        if !kind.remap(&|q| pts.get(q).copied(), &|k| made.get(k).copied()) {
            continue;
        }
        // Each pasted dimension gets its own parameter, with the same expression.
        let param = c.param.as_ref().map(|(e, u)| doc.new_model_param(e, u));
        let id = sk.add_constraint(kind, param)?;
        if let Some(nc) = sk.constraints.last_mut() {
            nc.driven = c.driven;
            nc.text = c.text;
        }
        cons.push(id);
    }
    if same_sketch {
        for eq in &clip.external_equals {
            let Some(&copied_idx) = made.get(eq.copied) else { continue };
            let Some(ext_idx) = sk.curve_index(&eq.external_id) else { continue };
            let (a, b) = if eq.external_first { (ext_idx, copied_idx) } else { (copied_idx, ext_idx) };
            cons.push(sk.add_constraint(ConstraintKind::Equal { a, b }, None)?);
        }
    }
    let mut curve_ids: Vec<String> = made.iter().filter_map(|i| sk.curves.get(*i).map(|c| c.id.clone())).collect();
    for (src, path) in &clip.texts {
        let mut src = src.clone();
        let LinkSource::Text { at, style, exprs, .. } = &mut src else { continue };
        style.path = path.and_then(|k| made.get(k)).and_then(|i| sk.curves.get(*i)).map(|c| c.id.clone());
        *at += t;
        // The copy sits where it was pasted: its place no longer follows the original's expressions.
        exprs.x = None;
        exprs.y = None;
        let geom = sk.text_source_geometry(&src)?;
        let link = sk.add_link(LinkKind::Text, src, &geom)?;
        curve_ids.extend(sk.link_curves(&link).into_iter().filter_map(|i| sk.curves.get(i).map(|c| c.id.clone())));
    }
    let lone: Vec<String> = clip.lone.iter().filter_map(|n| pts.get(*n)).filter_map(|i| sk.points.get(*i).map(|q| q.id.clone())).collect();
    Ok((curve_ids, lone, cons))
}

#[cfg(test)]
#[path = "sketch_clipboard_tests.rs"]
mod tests;
