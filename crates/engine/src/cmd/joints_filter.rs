//! Filters and summaries for `joint.list` and `joint.inspect`: an imported assembly holds each
//! placement with a rigid joint, hundreds of them, which bury the few joints that move.
//!
//! A filter judges a [`Row`]: its type, whether it is suppressed, the occurrences it ties and
//! whether it holds a component's contents together. A joint is such a row; so is a record that
//! holds a whole group rigid (type rigid, its members as the occurrences, holding contents), so
//! such a list can be filtered and counted the same way.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;
use solidcraft_doc::Document;
use solidcraft_doc::joints::{Joint, JointKind};

use super::occurrence_id;
use crate::params::{bad, bool_};
use crate::schema::joints::{JointSummary, MoreRigidGroups, OccurrenceDof, Omitted, RigidGroupSummary};
use crate::{Result, Session};

/// At most this many types in `kinds`.
const MAX_KINDS: usize = 64;
/// At most this many occurrences in `occurrences`.
const MAX_OCCURRENCES: usize = 10_000;
/// At most this many rows in a summary's `rigid_groups`; the rest are counted.
const MAX_GROUP_ROWS: usize = 20;

/// One row a filter judges.
pub(super) struct Row<'a> {
    pub kind: JointKind,
    pub suppressed: bool,
    /// The occurrences it ties: a joint's two ends.
    pub ends: &'a [u64],
    /// Whether it holds a component's contents together (an imported assembly's placements).
    pub holds_contents: bool,
}

/// A joint's ends as a [`Row`] gives them: its two occurrences and, for an origin on a later copy
/// of a sub-assembly, the occurrences below the copy down to the part it was picked on
/// ([`JointOrigin::inside`](solidcraft_doc::joints::JointOrigin::inside)), so naming that part finds it.
pub(super) fn ends(j: &Joint) -> Vec<u64> {
    let mut ends = vec![j.a.occurrence, j.b.occurrence];
    ends.extend(j.a.inside.iter().chain(&j.b.inside).copied());
    ends
}

/// Which rows a reply keeps, from the call's `kinds`, `exclude_rigid`, `exclude_rigid_groups`,
/// `moving_only` and `occurrences`. A row is kept when it passes every filter given.
#[derive(Default)]
pub(super) struct RowFilter {
    kinds: Option<Vec<JointKind>>,
    exclude_rigid: bool,
    exclude_groups: bool,
    moving_only: bool,
    /// The occurrences named and every occurrence placed inside them.
    occurrences: Option<BTreeSet<u64>>,
}

impl RowFilter {
    /// The filter the call asks for (none: every row is kept).
    pub fn read(s: &Session, p: &Value, cmd: &str) -> Result<Self> {
        let kinds = match p.get("kinds") {
            None | Some(Value::Null) => None,
            Some(Value::Array(list)) if list.len() <= MAX_KINDS => Some(
                list.iter()
                    .map(|v| v.as_str().and_then(JointKind::parse).ok_or_else(|| bad(cmd, format!("`kinds`: {v} is not a joint type"))))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Some(_) => return Err(bad(cmd, format!("`kinds` must list at most {MAX_KINDS} joint types"))),
        };
        let occurrences = match p.get("occurrences") {
            None | Some(Value::Null) => None,
            Some(Value::Array(list)) if list.len() <= MAX_OCCURRENCES => {
                let named = list.iter().map(|v| occurrence_id(s, Some(v), cmd)).collect::<Result<BTreeSet<u64>>>()?;
                Some(with_contents(&s.doc, &named))
            }
            Some(_) => return Err(bad(cmd, format!("`occurrences` must list at most {MAX_OCCURRENCES} occurrences"))),
        };
        Ok(RowFilter {
            kinds,
            exclude_rigid: bool_(p, "exclude_rigid").unwrap_or(false),
            exclude_groups: bool_(p, "exclude_rigid_groups").unwrap_or(false),
            moving_only: bool_(p, "moving_only").unwrap_or(false),
            occurrences,
        })
    }

    /// Whether the call asked for any filter.
    pub fn any(&self) -> bool {
        self.kinds.is_some() || self.exclude_rigid || self.exclude_groups || self.moving_only || self.occurrences.is_some()
    }

    /// Whether [`Row::holds_contents`] matters to it, so worth working out.
    pub fn needs_groups(&self) -> bool {
        self.exclude_groups
    }

    pub fn keeps(&self, row: &Row) -> bool {
        let rigid = row.kind == JointKind::Rigid;
        self.kinds.as_ref().is_none_or(|k| k.contains(&row.kind))
            && !(self.exclude_rigid && rigid)
            && !(self.exclude_groups && rigid && row.holds_contents)
            && !(self.moving_only && (rigid || row.suppressed))
            && self.occurrences.as_ref().is_none_or(|o| row.ends.iter().any(|e| o.contains(e)))
    }

    /// The occurrences `dof` keeps, when `occurrences` narrows it.
    pub fn occurrence_set(&self) -> Option<&BTreeSet<u64>> {
        self.occurrences.as_ref()
    }
}

/// `named` and every occurrence placed inside one of them, at any depth. The root (0) stands
/// for itself only: everything is inside it.
fn with_contents(doc: &Document, named: &BTreeSet<u64>) -> BTreeSet<u64> {
    let components: BTreeSet<u64> =
        doc.occurrences.iter().filter(|o| o.id != 0 && named.contains(&o.id) && o.component != 0).map(|o| o.component).collect();
    let mut out = named.clone();
    if components.is_empty() {
        return out;
    }
    let parents: HashMap<u64, u64> = doc.components.iter().map(|c| (c.id, c.parent)).collect();
    for o in &doc.occurrences {
        let mut c = o.parent;
        for _ in 0..=doc.components.len() {
            if c == 0 {
                break;
            }
            if components.contains(&c) {
                out.insert(o.id);
                break;
            }
            match parents.get(&c) {
                Some(p) => c = *p,
                None => break,
            }
        }
    }
    out
}

/// For each rigid joint that holds a component's contents together, that component: the
/// innermost component (not the root) both its ends are inside, as `Session::contents_joints`
/// counts them.
pub(super) fn contents_groups(doc: &Document) -> HashMap<u64, u64> {
    let occ: HashMap<u64, (u64, u64)> = doc.occurrences.iter().map(|o| (o.id, (o.component, o.parent))).collect();
    let parents: HashMap<u64, u64> = doc.components.iter().map(|c| (c.id, c.parent)).collect();
    // The components an occurrence is inside, innermost first: its own, then where it sits and up.
    let chain = |o: u64| -> Vec<u64> {
        let Some((component, parent)) = occ.get(&o).copied() else { return Vec::new() };
        let mut out = vec![component];
        let mut c = parent;
        for _ in 0..=doc.components.len() {
            if c == 0 {
                break;
            }
            out.push(c);
            match parents.get(&c) {
                Some(p) => c = *p,
                None => break,
            }
        }
        out
    };
    let mut out = HashMap::new();
    for j in doc.assembly.joints.iter().filter(|j| j.kind == JointKind::Rigid) {
        let (a, b) = (chain(j.a.occurrence), chain(j.b.occurrence));
        // B is inside C when C is its own component or one it sits in (the same chain).
        if let Some(c) = a.iter().find(|c| **c != 0 && b.contains(c)) {
            out.insert(j.id, *c);
        }
    }
    out
}

/// A joint type as the replies name it.
pub(super) fn kind_name(kind: JointKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// Count a row a filter left out.
pub(super) fn omit(omitted: &mut Omitted, kind: JointKind) {
    omitted.joints += 1;
    *omitted.by_type.entry(kind_name(kind)).or_default() += 1;
}

/// Narrow `dof` to the occurrences `keep` names (when given) and, with `free_only`, to those
/// that can still move; count the rest.
pub(super) fn narrow_dof(dof: &mut Option<Vec<OccurrenceDof>>, keep: Option<&BTreeSet<u64>>, free_only: bool, omitted: &mut Omitted) {
    if let Some(d) = dof.as_mut() {
        let before = d.len();
        d.retain(|x| (!free_only || x.dof > 0) && keep.is_none_or(|k| k.contains(&x.occurrence)));
        omitted.dof += before - d.len();
    }
}

/// Every joint in a few numbers: by type, the suppressed ones, and the rigid joints holding
/// components' contents (`groups`, from [`contents_groups`]) one row per component.
pub(super) fn summary(doc: &Document, groups: &HashMap<u64, u64>) -> JointSummary {
    let joints = &doc.assembly.joints;
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    for j in joints {
        *by_type.entry(kind_name(j.kind)).or_default() += 1;
    }
    let mut per: BTreeMap<u64, (usize, BTreeSet<u64>)> = BTreeMap::new();
    for j in joints {
        if let Some(c) = groups.get(&j.id) {
            let e = per.entry(*c).or_default();
            e.0 += 1;
            e.1.insert(j.a.occurrence);
            e.1.insert(j.b.occurrence);
        }
    }
    let mut rigid_groups: Vec<RigidGroupSummary> = per
        .into_iter()
        .map(|(component, (n, members))| RigidGroupSummary {
            component,
            name: doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default(),
            joints: n,
            members: members.len(),
        })
        .collect();
    rigid_groups.sort_by(|a, b| b.joints.cmp(&a.joints).then(a.component.cmp(&b.component)));
    // An import wraps many parts in a component of their own: hundreds of rows of one joint.
    let rest = rigid_groups.split_off(rigid_groups.len().min(MAX_GROUP_ROWS));
    let more_rigid_groups = (!rest.is_empty()).then(|| MoreRigidGroups { groups: rest.len(), joints: rest.iter().map(|g| g.joints).sum() });
    JointSummary { joints: joints.len(), by_type, suppressed: joints.iter().filter(|j| j.suppressed).count(), rigid_groups, more_rigid_groups }
}
