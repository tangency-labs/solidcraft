//! The design compare (#5184, X1a): what changed from one design (`base`) to another (`head`):
//! features, parameters, components, joints and the design's other fields, with values before
//! and after. And its one projection, [`Touched`]: what a change edited, or added inside, that the
//! base already had, which conflict warnings, claims and role rules compare. Pure: no model
//! evaluation, geometry or I/O.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::expr::Kind;
use crate::joints::{Assembly, Joint};
use crate::{Component, Document, Feature, FeatureKind, FieldRole, Occurrence, ParamRow, ParamUser};

/// What changed from `base` to `head`.
#[derive(Clone, Debug, Default)]
pub struct DesignDiff {
    /// In `head`'s timeline order, then the removed ones in `base`'s.
    pub features: Vec<FeatureChange>,
    /// User parameters, sketch dimensions and texts, by name, in `head`'s order, then the removed
    /// ones. A feature's own inputs are on the feature.
    pub params: Vec<ParamChange>,
    /// Components and their placements: `head`'s order, then the removed ones.
    pub components: Vec<ComponentChange>,
    /// Joints whose definition changed: `head`'s order, then the removed ones.
    pub joints: Vec<JointChange>,
    /// The design's other fields that differ (`appearances`, `materials`...).
    pub other: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Added,
    Removed,
    /// What it is or does: a definition field, an input, an expression.
    Changed,
    /// Its place in the timeline, or a component's parent.
    Moved,
    Renamed,
    Suppressed,
    Unsuppressed,
    /// Only what solving writes back: a sketch solved again for something it uses that changed,
    /// a placement its joints moved.
    Resolved,
    /// Only its value: a parameter whose expression follows one that changed.
    Recomputed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeatureChange {
    pub id: u64,
    pub type_name: &'static str,
    /// The component it is in (`head`'s; `base`'s for a removed one).
    pub component: u64,
    /// The name in `base` and in `head`; empty where it has none.
    pub names: (String, String),
    pub kinds: BTreeSet<ChangeKind>,
    pub inputs: Vec<InputChange>,
    /// The other definition fields that differ (`operation`, `edges`, `sketch`...).
    pub fields: Vec<String>,
}

/// An input whose expression changed, came or went.
#[derive(Clone, Debug, PartialEq)]
pub struct InputChange {
    pub label: &'static str,
    /// Its parameter name (`d3`), `head`'s or else `base`'s.
    pub param: Option<String>,
    pub before: Option<InputValue>,
    pub after: Option<InputValue>,
    /// The unit of the values: `mm`, `deg`, or none.
    pub unit: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputValue {
    pub expression: String,
    /// What it evaluates to in that design, in the input's unit; `None` when it doesn't.
    pub value: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct ParamChange {
    pub name: String,
    /// Added, Removed, Changed (a renamed parameter is one removed and one added), or Recomputed.
    pub kind: ChangeKind,
    pub before: Option<ParamRow>,
    pub after: Option<ParamRow>,
    /// What in `base` uses it (`Document::param_users`): features, sketches and joints. A
    /// parameter that uses it is not listed: its value changes too, so it is its own change.
    pub used_by: Vec<Target>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentChange {
    pub id: u64,
    /// Its parent component (`head`'s; `base`'s for a removed one; 0 is the design).
    pub parent: u64,
    /// The name in `base` and in `head`; empty where it has none.
    pub names: (String, String),
    pub kinds: BTreeSet<ChangeKind>,
    /// What differs: `metadata`, `properties`, `identification_color`, `occurrences` (one came,
    /// went or changed), `placement` (one moved by a person).
    pub fields: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JointChange {
    pub id: u64,
    /// The name in `base` and in `head`; empty where it has none.
    pub names: (String, String),
    pub kinds: BTreeSet<ChangeKind>,
}

/// What changed from `base` to `head`. `base` must be the changeset's parent: items pair by id
/// with `base`'s, so an id `base` doesn't have is new in `head`.
///
/// Costs one `==` per unchanged feature, component, placement and joint. A changed feature is
/// copied without its heavy fields (a sketch's geometry, an import's STEP text and meshes),
/// which are compared where they are, and the copies are serialised. When parameters,
/// features or units differ, each design's parameters are evaluated once, and each changed
/// parameter's users are looked up once (`param_users`, a walk of `base`). The assembly is
/// copied twice when it differs. Neither design is cloned.
pub fn diff(base: &Document, head: &Document) -> DesignDiff {
    let values = Evaluated::of(base, head);
    let params = params(base, head, values.as_ref());
    // Features something that changed uses: a sketch among them solved again is `Resolved`.
    let followed: BTreeSet<u64> =
        params.iter().flat_map(|p| &p.used_by).filter_map(|t| if let Target::Feature(f) = t { Some(*f) } else { None }).collect();
    let assemblies = (base.assembly != head.assembly).then(|| (base.assembly.as_defined(), head.assembly.as_defined()));
    DesignDiff {
        features: features(base, head, values.as_ref(), &followed),
        params,
        components: components(base, head, assemblies.as_ref().is_none_or(|(a, b)| a == b)),
        joints: joints(assemblies.as_ref()),
        other: other(base, head, assemblies.as_ref()),
    }
}

type Values = BTreeMap<String, crate::expr::Value>;

/// Each design's parameter values and errors (`evaluation_values`), worked out once.
struct Evaluated {
    base: (Values, BTreeMap<String, String>),
    head: (Values, BTreeMap<String, String>),
}

impl Evaluated {
    /// `None` when nothing the values come from differs.
    fn of(base: &Document, head: &Document) -> Option<Evaluated> {
        let same = base.params == head.params && base.text_params == head.text_params && base.units == head.units && base.features == head.features;
        (!same).then(|| Evaluated { base: crate::eval::evaluation_values(base), head: crate::eval::evaluation_values(head) })
    }
}

/// Where the first item with each id is (a damaged design can repeat one).
fn first_of_each<T>(items: &[T], id: impl Fn(&T) -> u64) -> BTreeMap<u64, usize> {
    let mut at = BTreeMap::new();
    for (i, x) in items.iter().enumerate() {
        at.entry(id(x)).or_insert(i);
    }
    at
}

/// For each item of `items` that is the first with its id: the index of the first in `base`
/// with that id. A changeset's new items get ids above every id its parent has
/// ([`Document::next_item_id`]), so they never pair, even where two people's share one (#1017).
fn pair_by_id<T>(base: &[T], items: &[T], id: impl Fn(&T) -> u64) -> Vec<Option<usize>> {
    let (in_base, in_items) = (first_of_each(base, &id), first_of_each(items, &id));
    items.iter().enumerate().map(|(i, x)| in_base.get(&id(x)).copied().filter(|_| in_items.get(&id(x)) == Some(&i))).collect()
}

/// Features pair by id when they are of the same type.
fn features(base: &Document, head: &Document, values: Option<&Evaluated>, followed: &BTreeSet<u64>) -> Vec<FeatureChange> {
    let pairs: Vec<Option<(usize, &Feature)>> = pair_by_id(&base.features, &head.features, |f| f.id)
        .into_iter()
        .zip(&head.features)
        .map(|(j, h)| {
            j.and_then(|j| base.features.get(j).filter(|b| std::mem::discriminant(&b.kind) == std::mem::discriminant(&h.kind)).map(|b| (j, b)))
        })
        .collect();
    let paired: Vec<usize> = pairs.iter().flatten().map(|(j, _)| *j).collect();
    let moved = out_of_order(&paired);
    let kept: BTreeSet<usize> = paired.iter().copied().collect();
    let removed: Vec<FeatureChange> =
        base.features.iter().enumerate().filter(|(j, _)| !kept.contains(j)).map(|(_, b)| alone(b, ChangeKind::Removed)).collect();
    // Whether a feature before this one changed: what a sketch projects may have moved.
    let mut upstream = !removed.is_empty();
    let mut out = Vec::new();
    for (h, pair) in head.features.iter().zip(&pairs) {
        let change = match pair {
            Some((j, b)) => {
                let explained = followed.contains(&h.id) || (upstream && projects(h));
                feature_change(b, h, moved.contains(j), values, explained)
            }
            None => Some(alone(h, ChangeKind::Added)),
        };
        if let Some(change) = change {
            upstream |= change.kinds.iter().any(|k| *k != ChangeKind::Resolved);
            out.push(change);
        }
    }
    out.extend(removed);
    out
}

/// A sketch with points projected from other geometry.
fn projects(f: &Feature) -> bool {
    matches!(&f.kind, FeatureKind::Sketch { sketch, .. } if sketch.points.iter().any(|p| p.link.is_some()))
}

fn alone(f: &Feature, kind: ChangeKind) -> FeatureChange {
    let names = if kind == ChangeKind::Added { (String::new(), f.name.clone()) } else { (f.name.clone(), String::new()) };
    FeatureChange {
        id: f.id,
        type_name: f.kind.type_name(),
        component: f.component,
        names,
        kinds: BTreeSet::from([kind]),
        inputs: Vec::new(),
        fields: Vec::new(),
    }
}

/// The base positions that are not on one longest increasing run of `order`: the fewest
/// features whose moves explain the new order. O(n log n).
fn out_of_order(order: &[usize]) -> BTreeSet<usize> {
    // `tails[k]`: where in `order` the smallest last value of a run of length k + 1 is.
    let (mut tails, mut prev): (Vec<usize>, Vec<Option<usize>>) = (Vec::new(), Vec::with_capacity(order.len()));
    for (i, x) in order.iter().enumerate() {
        let k = tails.partition_point(|t| order.get(*t).is_some_and(|v| v < x));
        prev.push(k.checked_sub(1).and_then(|k| tails.get(k).copied()));
        match tails.get_mut(k) {
            Some(t) => *t = i,
            None => tails.push(i),
        }
    }
    let mut run = BTreeSet::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        run.insert(i);
        at = prev.get(i).copied().flatten();
    }
    order.iter().enumerate().filter(|(i, _)| !run.contains(i)).map(|(_, j)| *j).collect()
}

/// How feature `b` became `h`, or `None` when only what evaluation writes back differs. A sketch
/// solved again counts as `Resolved` when `explained` (something it uses changed) or its own
/// inputs changed; otherwise its moved points are a person's drag.
fn feature_change(b: &Feature, h: &Feature, moved: bool, values: Option<&Evaluated>, explained: bool) -> Option<FeatureChange> {
    let mut kinds = BTreeSet::new();
    if moved {
        kinds.insert(ChangeKind::Moved);
    }
    let (mut inputs, mut fields) = (Vec::new(), Vec::new());
    if b != h {
        if b.name != h.name {
            kinds.insert(ChangeKind::Renamed);
        }
        if b.suppressed != h.suppressed {
            kinds.insert(if h.suppressed { ChangeKind::Suppressed } else { ChangeKind::Unsuppressed });
        }
        inputs = input_changes(b, h, values);
        fields = field_changes(b, h);
        if fields.iter().any(|f| f == "sketch") && (explained || !inputs.is_empty()) && solved_again(b, h) {
            fields.retain(|f| f != "sketch");
            kinds.insert(ChangeKind::Resolved);
        }
        if !inputs.is_empty() || !fields.is_empty() {
            kinds.insert(ChangeKind::Changed);
        }
    }
    (!kinds.is_empty()).then(|| FeatureChange {
        id: h.id,
        type_name: h.kind.type_name(),
        component: h.component,
        names: (b.name.clone(), h.name.clone()),
        kinds,
        inputs,
        fields,
    })
}

fn solved_again(b: &Feature, h: &Feature) -> bool {
    matches!((&b.kind, &h.kind), (FeatureKind::Sketch { sketch: s, .. }, FeatureKind::Sketch { sketch: t, .. }) if crate::eval::solved_again(s, t))
}

/// Each label with how many of it came before: the second `Radius` is `("Radius", 1)`. Inputs
/// pair by these, so an optional input that comes or goes shifts no other.
fn numbered(labels: impl IntoIterator<Item = &'static str>) -> Vec<(&'static str, usize)> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    labels
        .into_iter()
        .map(|label| {
            let n = seen.entry(label).or_insert(0);
            *n += 1;
            (label, *n - 1)
        })
        .collect()
}

/// One of a feature's inputs (`FeatureKind::inputs`), keyed by [`numbered`].
struct KeyedInput<'a> {
    key: (&'static str, usize),
    expression: String,
    kind: Kind,
    param: Option<&'a String>,
}

fn keyed_inputs(f: &Feature) -> Vec<KeyedInput<'_>> {
    let inputs = f.kind.inputs();
    let keys = numbered(inputs.iter().map(|(label, ..)| *label));
    keys.into_iter()
        .zip(inputs)
        .enumerate()
        .map(|(i, (key, (_, expression, kind)))| KeyedInput { key, expression, kind, param: f.param_names.get(i).filter(|p| !p.is_empty()) })
        .collect()
}

/// An input's expression with its value in that design, in the input's unit.
fn input_value(input: &KeyedInput, values: Option<&Values>) -> InputValue {
    // Evaluated in mm or radians, shown in the unit as the parameters table shows it.
    let scale = crate::expr::unit_info(input.kind.default_unit()).map_or(1.0, |(_, s)| s);
    let value = values.and_then(|v| crate::eval::val(v, &input.expression, input.kind).ok());
    InputValue { expression: input.expression.clone(), value: value.map(|x| x / scale) }
}

/// The inputs whose expressions differ, in `h`'s order, then the ones only `b` has.
fn input_changes(b: &Feature, h: &Feature, values: Option<&Evaluated>) -> Vec<InputChange> {
    let (before, after) = (values.map(|v| &v.base.0), values.map(|v| &v.head.0));
    let was = keyed_inputs(b);
    let mut left: BTreeMap<_, &KeyedInput> = was.iter().map(|x| (x.key, x)).collect();
    let mut out = Vec::new();
    for now in keyed_inputs(h) {
        let then = left.remove(&now.key);
        if then.is_some_and(|w| w.expression == now.expression) {
            continue;
        }
        out.push(InputChange {
            label: now.key.0,
            param: now.param.or(then.and_then(|w| w.param)).cloned(),
            before: then.map(|w| input_value(w, before)),
            after: Some(input_value(&now, after)),
            unit: now.kind.default_unit(),
        });
    }
    for then in was.iter().filter(|x| left.contains_key(&x.key)) {
        let before = Some(input_value(then, before));
        out.push(InputChange { label: then.key.0, param: then.param.cloned(), before, after: None, unit: then.kind.default_unit() });
    }
    out
}

/// What an input only one side has is set to, so its field drops out of the comparison.
const ONE_SIDED: &str = "\u{1}input only one side has";

/// The definition fields (`Feature::field_role`) that differ from `b` to `h`, apart from the
/// inputs, which are reported as inputs: copies without the heavy fields, with `h`'s inputs put
/// into `b`'s and inputs only one side has taken out, are compared serialised; the heavy fields
/// are compared where they are. Derived fields count only where a person changed them.
fn field_changes(b: &Feature, h: &Feature) -> Vec<String> {
    let (mut x, mut y) = (b.without_heavy(), h.without_heavy());
    let theirs: BTreeMap<_, String> = keyed_inputs(h).into_iter().map(|i| (i.key, i.expression)).collect();
    let ours: BTreeSet<_> = keyed_inputs(b).into_iter().map(|i| i.key).collect();
    let keys = numbered(x.kind.inputs().into_iter().map(|(label, ..)| label));
    for (key, (_, e, _)) in keys.iter().zip(x.kind.inputs_mut()) {
        *e = theirs.get(key).map_or(ONE_SIDED, String::as_str).into();
    }
    let keys = numbered(y.kind.inputs().into_iter().map(|(label, ..)| label));
    for (key, (_, e, _)) in keys.iter().zip(y.kind.inputs_mut()) {
        if !ours.contains(key) {
            *e = ONE_SIDED.into();
        }
    }
    let (Ok(mut x), Ok(mut y)) = (serde_json::to_value(&x), serde_json::to_value(&y)) else {
        return vec!["definition".into()];
    };
    forget_one_sided(&mut x);
    forget_one_sided(&mut y);
    let (Value::Object(x), Value::Object(y)) = (x, y) else { return vec!["definition".into()] };
    let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
    let mut out: Vec<String> = keys
        .into_iter()
        .filter(|k| x.get(*k) != y.get(*k))
        .filter(|k| match h.field_role(k) {
            FieldRole::Identity | FieldRole::Bookkeeping | FieldRole::Heavy(_) => false,
            FieldRole::Derived => !h.only_written_back(b, k),
            FieldRole::Input => true,
        })
        .cloned()
        .collect();
    out.extend(b.heavy_differs(h).into_iter().map(String::from));
    out.sort();
    out
}

/// Removes the object entries set to [`ONE_SIDED`], at any depth.
fn forget_one_sided(v: &mut Value) {
    match v {
        Value::Object(o) => {
            o.retain(|_, x| x.as_str() != Some(ONE_SIDED));
            o.values_mut().for_each(forget_one_sided);
        }
        Value::Array(a) => a.iter_mut().for_each(forget_one_sided),
        _ => {}
    }
}

/// Parameters pair by name. Changed means its expression, unit, comment, text or source changed;
/// Recomputed, only its value (it follows one that changed).
fn params(base: &Document, head: &Document, values: Option<&Evaluated>) -> Vec<ParamChange> {
    let Some(values) = values else { return Vec::new() };
    let rows = |d: &Document, (vals, errs): &(Values, BTreeMap<String, String>)| -> Vec<ParamRow> {
        d.param_rows_in(vals, errs).into_iter().filter(|r| r.source != "feature").collect()
    };
    let before = rows(base, &values.base);
    let mut left: BTreeMap<&str, &ParamRow> = BTreeMap::new();
    for r in &before {
        left.entry(r.name.as_str()).or_insert(r);
    }
    // The feature each named input belongs to.
    let owners: BTreeMap<&str, u64> =
        base.features.iter().flat_map(|f| f.param_names.iter().filter(|n| !n.is_empty()).map(move |n| (n.as_str(), f.id))).collect();
    let used_by = |name: &str| -> Vec<Target> {
        let users = base.param_users(name).into_iter().filter_map(|u| match u {
            ParamUser::Feature { feature, .. } | ParamUser::Dimension { feature, .. } => Some(Target::Feature(feature)),
            ParamUser::Parameter { name } => owners.get(name.as_str()).map(|f| Target::Feature(*f)),
            ParamUser::Joint { joint, .. } => Some(Target::Joint(joint)),
            ParamUser::Configuration { .. } | ParamUser::Rule { .. } | ParamUser::Canvas { .. } => None,
        });
        users.collect::<BTreeSet<_>>().into_iter().collect()
    };
    let mut out = Vec::new();
    for r in rows(head, &values.head) {
        let (kind, was) = match left.remove(r.name.as_str()) {
            Some(b) if b.expression == r.expression && b.unit == r.unit && b.comment == r.comment && b.text == r.text && b.source == r.source => {
                if b.value == r.value {
                    continue;
                }
                (ChangeKind::Recomputed, Some(b.clone()))
            }
            Some(b) => (ChangeKind::Changed, Some(b.clone())),
            None => (ChangeKind::Added, None),
        };
        let used_by = if was.is_some() { used_by(&r.name) } else { Vec::new() };
        out.push(ParamChange { name: r.name.clone(), kind, before: was, after: Some(r), used_by });
    }
    for r in before.iter().filter(|r| left.contains_key(r.name.as_str())) {
        out.push(ParamChange { name: r.name.clone(), kind: ChangeKind::Removed, before: Some(r.clone()), after: None, used_by: used_by(&r.name) });
    }
    out
}

/// What a component's placements say: fields that differ, and whether solving moved one.
#[derive(Default)]
struct PlacementMarks {
    fields: BTreeSet<&'static str>,
    solved: bool,
}

/// Components pair by id, as do their placements. A placement that only moved is `Resolved`
/// when joints place it and they are defined as they were (`joints_kept`: the solve wrote it
/// back), else a person moved it.
fn components(base: &Document, head: &Document, joints_kept: bool) -> Vec<ComponentChange> {
    let mut marks: BTreeMap<u64, PlacementMarks> = BTreeMap::new();
    let placed = |d: &Document| first_of_each(&d.occurrences, |o| o.id);
    let (before, after) = (placed(base), placed(head));
    let jointed: BTreeSet<u64> = head.assembly.joints.iter().flat_map(|j| [j.a.occurrence, j.b.occurrence]).collect();
    for id in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        match (before.get(id).and_then(|i| base.occurrences.get(*i)), after.get(id).and_then(|i| head.occurrences.get(*i))) {
            (Some(b), Some(h)) if b == h => {}
            (Some(b), Some(h)) if Occurrence { transform: h.transform, ..b.clone() } == *h => {
                let mark = marks.entry(h.component).or_default();
                if joints_kept && jointed.contains(id) && !h.grounded {
                    mark.solved = true;
                } else {
                    mark.fields.insert("placement");
                }
            }
            (b, h) => {
                for o in [b, h].into_iter().flatten() {
                    marks.entry(o.component).or_default().fields.insert("occurrences");
                }
            }
        }
    }
    let (mut left, in_head) = (first_of_each(&base.components, |c| c.id), first_of_each(&head.components, |c| c.id));
    let mut out = Vec::new();
    for h in head.components.iter().enumerate().filter(|(i, h)| in_head.get(&h.id) == Some(i)).map(|(_, h)| h) {
        let b = left.remove(&h.id).and_then(|j| base.components.get(j));
        out.extend(component_change(h.id, b, Some(h), marks.remove(&h.id)));
    }
    for b in base.components.iter().enumerate().filter(|(j, b)| left.get(&b.id) == Some(j)).map(|(_, b)| b) {
        out.extend(component_change(b.id, Some(b), None, marks.remove(&b.id)));
    }
    // Placements of components neither design has (a damaged design).
    out.extend(marks.into_iter().filter_map(|(id, mark)| component_change(id, None, None, Some(mark))));
    out
}

/// How component `b` became `h`, with what its placements say, or `None` for no change.
fn component_change(id: u64, b: Option<&Component>, h: Option<&Component>, mark: Option<PlacementMarks>) -> Option<ComponentChange> {
    let PlacementMarks { mut fields, solved } = mark.unwrap_or_default();
    let mut kinds = BTreeSet::new();
    match (b, h) {
        (Some(b), Some(h)) => {
            let Component { id: _, name, parent, metadata, properties, identification_color, identification_color_index: _ } = b;
            if *name != h.name {
                kinds.insert(ChangeKind::Renamed);
            }
            if *parent != h.parent {
                kinds.insert(ChangeKind::Moved);
            }
            for (field, differs) in [
                ("metadata", *metadata != h.metadata),
                ("properties", *properties != h.properties),
                ("identification_color", *identification_color != h.identification_color),
            ] {
                if differs {
                    fields.insert(field);
                }
            }
        }
        (None, Some(_)) => {
            kinds.insert(ChangeKind::Added);
        }
        (Some(_), None) => {
            kinds.insert(ChangeKind::Removed);
        }
        (None, None) => {}
    }
    if solved {
        kinds.insert(ChangeKind::Resolved);
    }
    if !fields.is_empty() && !kinds.contains(&ChangeKind::Added) && !kinds.contains(&ChangeKind::Removed) {
        kinds.insert(ChangeKind::Changed);
    }
    let name = |c: Option<&Component>| c.map(|c| c.name.clone()).unwrap_or_default();
    let parent = h.or(b).map_or(0, |c| c.parent);
    (!kinds.is_empty()).then(|| ComponentChange { id, parent, names: (name(b), name(h)), kinds, fields: fields.into_iter().collect() })
}

/// Joints pair by id, as features do, and compare as defined ([`Joint::as_defined`]):
/// `assemblies` is both assemblies so, or `None` when the assemblies are the same.
fn joints(assemblies: Option<&(Assembly, Assembly)>) -> Vec<JointChange> {
    let Some((a, b)) = assemblies else { return Vec::new() };
    let pairs = pair_by_id(&a.joints, &b.joints, |j| j.id);
    let mut kept = BTreeSet::new();
    let mut out = Vec::new();
    for (h, pair) in b.joints.iter().zip(pairs) {
        let Some(was) = pair.and_then(|i| a.joints.get(i).map(|j| (i, j))) else {
            out.push(JointChange { id: h.id, names: (String::new(), h.name.clone()), kinds: BTreeSet::from([ChangeKind::Added]) });
            continue;
        };
        kept.insert(was.0);
        let j = was.1;
        let mut kinds = BTreeSet::new();
        if j.name != h.name {
            kinds.insert(ChangeKind::Renamed);
        }
        if j.suppressed != h.suppressed {
            kinds.insert(if h.suppressed { ChangeKind::Suppressed } else { ChangeKind::Unsuppressed });
        }
        if (Joint { name: h.name.clone(), suppressed: h.suppressed, ..j.clone() }) != *h {
            kinds.insert(ChangeKind::Changed);
        }
        if !kinds.is_empty() {
            out.push(JointChange { id: h.id, names: (j.name.clone(), h.name.clone()), kinds });
        }
    }
    let removed = a.joints.iter().enumerate().filter(|(i, _)| !kept.contains(i));
    out.extend(removed.map(|(_, j)| JointChange { id: j.id, names: (j.name.clone(), String::new()), kinds: BTreeSet::from([ChangeKind::Removed]) }));
    out
}

/// The design's other fields that differ. Every field of `Document` and of its assembly is named
/// here, so a field added later has to be decided on (as the engine's `same_but_features` does).
fn other(base: &Document, head: &Document, assemblies: Option<&(Assembly, Assembly)>) -> Vec<&'static str> {
    let Document {
        // Compared above.
        features: _,
        params: _,
        text_params: _,
        components: _,
        occurrences: _,
        // Bookkeeping: the counters, the format the file was saved in, and body names a rename freed.
        next_id: _,
        next_component_color_index: _,
        format: _,
        format_pending: _,
        reserved_body_names: _,
        // View state.
        marker: _,
        visibility: _,
        named_views: _,
        browser_order: _,
        browser_groups: _,
        timeline_groups: _,
        // Its joints above, the rest below, as defined.
        assembly: _,
        render,
        name,
        units,
        metadata,
        sheet,
        plastic,
        parts,
        links,
        configs,
        favorites,
        canvases,
        point_clouds,
        param_comments,
        materials,
        user_materials,
        appearances,
        body_components,
        body_offsets,
    } = base;
    let assembly = assemblies.is_some_and(|(a, b)| {
        let Assembly { joints: _, origins, links: motion_links, contact_sets, contact_enabled, contact_all, studies, traces, exploded } = a;
        *origins != b.origins
            || *motion_links != b.links
            || *contact_sets != b.contact_sets
            || *contact_enabled != b.contact_enabled
            || *contact_all != b.contact_all
            || *studies != b.studies
            || *traces != b.traces
            || *exploded != b.exploded
    });
    [
        ("render", *render != head.render),
        ("name", *name != head.name),
        ("units", *units != head.units),
        ("metadata", *metadata != head.metadata),
        ("assembly", assembly),
        ("sheet", *sheet != head.sheet),
        ("plastic", *plastic != head.plastic),
        ("parts", *parts != head.parts),
        ("links", *links != head.links),
        ("configs", *configs != head.configs),
        ("favorites", *favorites != head.favorites),
        ("canvases", *canvases != head.canvases),
        ("point_clouds", *point_clouds != head.point_clouds),
        ("param_comments", *param_comments != head.param_comments),
        ("materials", *materials != head.materials),
        ("user_materials", *user_materials != head.user_materials),
        ("appearances", *appearances != head.appearances),
        ("body_components", *body_components != head.body_components),
        ("body_offsets", *body_offsets != head.body_offsets),
    ]
    .into_iter()
    .filter_map(|(field, differs)| differs.then_some(field))
    .collect()
}

/// What a change touched: a component (`Component(0)` is the design itself), a feature, a body,
/// a joint, a parameter, or one of the design's own fields (a name from [`DesignDiff::other`]).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Component(u64),
    Feature(u64),
    Body(BodyKey),
    Joint(u64),
    Parameter(String),
    Field(&'static str),
}

impl Target {
    /// The design itself.
    pub const DESIGN: Target = Target::Component(0);
    /// The design's units: every length is read in them, so changing them overlaps every edit.
    pub const UNITS: Target = Target::Field("units");
}

/// A body by its name and the feature that made it (names alone collide across branches, #5308).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyKey {
    pub name: String,
    pub feature: u64,
}

/// How a change touched a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// It changed, moved, was renamed, suppressed or removed, or something it uses changed.
    Edited,
    /// Something new was put inside it: a feature into its component, a component into its
    /// parent, a parameter or a joint into the design.
    AddedInside,
}

/// One touched target: its name in the base and after the change (empty for one removed), and
/// how it was touched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touch {
    pub names: (String, String),
    pub how: How,
}

/// The targets a change touched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Touched {
    pub targets: BTreeMap<Target, Touch>,
}

impl Touched {
    /// What `diff` touched that `base` has: what a person edited (and what uses a parameter that
    /// changed), and what something was added inside. Each of the design's own fields in `other`
    /// is edited under its name. What only solving wrote back touches nothing. O(n log n), plus a
    /// lookup of each name in `base`.
    pub fn of(diff: &DesignDiff, base: &Document) -> Touched {
        let by_person = |kinds: &BTreeSet<ChangeKind>| kinds.iter().any(|k| *k != ChangeKind::Resolved);
        let features: BTreeSet<u64> = base.features.iter().map(|f| f.id).collect();
        let components: BTreeSet<u64> = base.components.iter().map(|c| c.id).collect();
        let joints: BTreeSet<u64> = base.assembly.joints.iter().map(|j| j.id).collect();
        let params: BTreeSet<&str> = base.params.iter().map(|p| p.name.as_str()).chain(base.text_params.iter().map(|p| p.name.as_str())).collect();
        let has = |t: &Target| match t {
            Target::Component(c) => *c == 0 || components.contains(c),
            Target::Feature(f) => features.contains(f),
            Target::Joint(j) => joints.contains(j),
            Target::Parameter(p) => params.contains(p.as_str()),
            Target::Field(_) => true,
            Target::Body(_) => false,
        };
        let mut touched = Touched::default();
        let mut touch = |t: Target, names: Option<(String, String)>, how: How| {
            if has(&t) {
                let names = names.unwrap_or_else(|| base_names(base, &t));
                touched.touch(t, names, how);
            }
        };
        for f in &diff.features {
            if f.kinds.contains(&ChangeKind::Added) {
                touch(Target::Component(f.component), None, How::AddedInside);
            } else if by_person(&f.kinds) {
                touch(Target::Feature(f.id), Some(f.names.clone()), How::Edited);
            }
        }
        for c in &diff.components {
            if c.kinds.contains(&ChangeKind::Added) {
                touch(Target::Component(c.parent), None, How::AddedInside);
            } else if by_person(&c.kinds) {
                touch(Target::Component(c.id), Some(c.names.clone()), How::Edited);
            }
        }
        for j in &diff.joints {
            if j.kinds.contains(&ChangeKind::Added) {
                touch(Target::DESIGN, None, How::AddedInside);
            } else {
                touch(Target::Joint(j.id), Some(j.names.clone()), How::Edited);
            }
        }
        for p in &diff.params {
            match p.kind {
                ChangeKind::Added => touch(Target::DESIGN, None, How::AddedInside),
                ChangeKind::Changed | ChangeKind::Removed => {
                    let after = if p.kind == ChangeKind::Removed { String::new() } else { p.name.clone() };
                    touch(Target::Parameter(p.name.clone()), Some((p.name.clone(), after)), How::Edited);
                }
                _ => {}
            }
            for user in &p.used_by {
                touch(user.clone(), None, How::Edited);
            }
        }
        for &field in &diff.other {
            touch(Target::Field(field), None, How::Edited);
        }
        touched
    }

    /// Adds `target`; an edit outranks an addition inside.
    fn touch(&mut self, target: Target, names: (String, String), how: How) {
        let entry = self.targets.entry(target).or_insert(Touch { names: names.clone(), how });
        if how == How::Edited && entry.how == How::AddedInside {
            *entry = Touch { names, how };
        }
    }

    /// My edited targets that overlap their edited ones: the same target, or one holding the
    /// other. A component holds the components and features inside it (`doc`'s component tree),
    /// and a feature the bodies it made. A design field overlaps only an edit of the same field,
    /// except the units, which overlap everything the other side touched, additions included.
    /// Other additions inside never overlap. Costs
    /// O(n log n) in the targets and `doc`'s features, plus one walk of the component tree per
    /// component target.
    pub fn overlaps(&self, theirs: &Touched, doc: &Document) -> Vec<Target> {
        // The units overlap whatever the other side touched, additions too: what is added is read in them.
        if theirs.targets.contains_key(&Target::UNITS) {
            return self.targets.keys().cloned().collect();
        }
        let my_units = self.targets.contains_key(&Target::UNITS) && !theirs.targets.is_empty();
        let edited =
            |t: &Touched| -> BTreeSet<Target> { t.targets.iter().filter(|(_, touch)| touch.how == How::Edited).map(|(t, _)| t.clone()).collect() };
        let (mine, theirs) = (edited(self), edited(theirs));
        let at = first_of_each(&doc.features, |f| f.id);
        // The component a target is, or sits in.
        let home = |t: &Target| match t {
            Target::Component(c) => Some(*c),
            Target::Feature(f) | Target::Body(BodyKey { feature: f, .. }) => at.get(f).and_then(|i| doc.features.get(*i)).map(|f| f.component),
            Target::Joint(_) | Target::Parameter(_) | Target::Field(_) => None,
        };
        let tree = doc.component_tree();
        let their_trees: Vec<BTreeSet<u64>> =
            theirs.iter().filter_map(|t| if let Target::Component(c) = t { Some(tree.within(*c)) } else { None }).collect();
        let their_homes: BTreeSet<u64> = theirs.iter().filter_map(home).collect();
        let their_bodies: BTreeSet<u64> = theirs.iter().filter_map(|t| if let Target::Body(k) = t { Some(k.feature) } else { None }).collect();
        mine.into_iter()
            .filter(|t| {
                (*t == Target::UNITS && my_units)
                    || theirs.contains(t)
                    || home(t).is_some_and(|h| their_trees.iter().any(|inside| inside.contains(&h)))
                    || match t {
                        Target::Component(c) => !tree.within(*c).is_disjoint(&their_homes),
                        Target::Feature(f) => their_bodies.contains(f),
                        Target::Body(k) => theirs.contains(&Target::Feature(k.feature)),
                        Target::Joint(_) | Target::Parameter(_) | Target::Field(_) => false,
                    }
            })
            .collect()
    }
}

/// A target's name in `base`, twice: for targets touched without a name of their own.
fn base_names(base: &Document, t: &Target) -> (String, String) {
    let name = match t {
        Target::Component(0) => Some(base.name.clone()),
        Target::Component(c) => base.components.iter().find(|x| x.id == *c).map(|x| x.name.clone()),
        Target::Feature(f) => base.feature(*f).map(|x| x.name.clone()),
        Target::Joint(j) => base.assembly.joints.iter().find(|x| x.id == *j).map(|x| x.name.clone()),
        Target::Body(k) => Some(k.name.clone()),
        Target::Parameter(p) => Some(p.clone()),
        Target::Field(f) => Some((*f).to_owned()),
    };
    let name = name.unwrap_or_default();
    (name.clone(), name)
}

#[cfg(test)]
#[path = "design_diff_tests.rs"]
mod tests;
