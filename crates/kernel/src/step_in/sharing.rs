//! Exact source-graph matches. A digest only selects candidates; a bijection verifies topology.
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::p21::{Entity, Exchange, Param};
use super::source::SourceCopy;

const MAX_DEPTH: usize = 128;
const MAX_WORK: usize = 20_000_000;

#[derive(Debug, Default)]
pub(super) struct Sharing {
    pub copies: std::sync::Arc<[SourceCopy]>,
    pub canonical: HashMap<u64, u64>,
    repeated: HashSet<u64>,
}

impl Sharing {
    pub fn item(&self, item: u64) -> u64 {
        self.canonical.get(&item).copied().unwrap_or(item)
    }

    pub fn repeated(&self, item: u64) -> bool {
        self.repeated.contains(&item)
    }

    pub fn units(&mut self, planned: &[super::Planned]) {
        let mut units = HashMap::new();
        let mut ambiguous = HashSet::new();
        for p in planned {
            if let Some(old) = units.insert(p.item, p.units)
                && old != p.units
            {
                ambiguous.insert(p.item);
            }
        }
        self.canonical.retain(|item, source| !ambiguous.contains(item) && !ambiguous.contains(source) && units.get(item) == units.get(source));
        self.repeated.clear();
        for (item, source) in &self.canonical {
            self.repeated.insert(*item);
            self.repeated.insert(*source);
        }
    }

    pub fn face(&self, item: u64, face: u64) -> u64 {
        if !self.canonical.contains_key(&item) {
            return face;
        }
        let Some(copy) = self.copies.binary_search_by_key(&item, |c| c.item).ok().and_then(|i| self.copies.get(i)) else { return face };
        copy.entities.binary_search_by_key(&face, |p| p[1]).ok().and_then(|i| copy.entities.get(i)).map_or(face, |p| p[0])
    }
}

struct Fingerprints<'a> {
    ex: &'a Exchange,
    memo: HashMap<u64, Option<u64>>,
    path: Vec<u64>,
    work: usize,
    watch: Option<std::sync::Arc<dyn crate::watch::Watch>>,
}

impl Fingerprints<'_> {
    fn entity(&mut self, id: u64) -> Option<u64> {
        if let Some(hash) = self.memo.get(&id) {
            return *hash;
        }
        self.work += 1;
        if self.work > MAX_WORK
            || self.path.len() >= MAX_DEPTH
            || self.path.contains(&id)
            || (self.work.is_multiple_of(1024) && self.watch.as_ref().is_some_and(|w| w.cancelled()))
        {
            return None;
        }
        self.path.push(id);
        let result = self.record(id);
        self.path.pop();
        self.memo.insert(id, result);
        result
    }

    fn record(&mut self, id: u64) -> Option<u64> {
        let e = self.ex.get(id)?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        e.records.len().hash(&mut hash);
        for r in &e.records {
            r.name.hash(&mut hash);
            r.params.len().hash(&mut hash);
            for (i, p) in r.params.iter().enumerate() {
                if i == 0 && matches!(p, Param::Str(_)) {
                    0u8.hash(&mut hash);
                } else {
                    self.param(p, &mut hash, 0)?;
                }
            }
        }
        Some(hash.finish())
    }

    fn param(&mut self, p: &Param, hash: &mut impl Hasher, depth: usize) -> Option<()> {
        self.work += 1;
        if depth > 64 || self.work > MAX_WORK || (self.work.is_multiple_of(1024) && self.watch.as_ref().is_some_and(|w| w.cancelled())) {
            return None;
        }
        std::mem::discriminant(p).hash(hash);
        match p {
            Param::Ref(id) => self.entity(*id)?.hash(hash),
            Param::Int(v) => v.hash(hash),
            Param::Real(v) if v.is_finite() => v.to_bits().hash(hash),
            Param::Real(_) => return None,
            Param::Str(s) | Param::Enum(s) | Param::Binary(s) => s.hash(hash),
            Param::Typed(name, values) => {
                name.hash(hash);
                self.params(values, hash, depth + 1)?;
            }
            Param::List(values) => self.params(values, hash, depth + 1)?,
            Param::Null | Param::Derived => {}
        }
        Some(())
    }

    fn params(&mut self, values: &[Param], hash: &mut impl Hasher, depth: usize) -> Option<()> {
        values.len().hash(hash);
        for p in values {
            self.param(p, hash, depth)?;
        }
        Some(())
    }
}

#[derive(Default)]
struct Compare {
    forward: HashMap<u64, u64>,
    reverse: HashMap<u64, u64>,
    work: usize,
    watch: Option<std::sync::Arc<dyn crate::watch::Watch>>,
}

impl Compare {
    fn entity(&mut self, ex: &Exchange, a: u64, b: u64, depth: usize) -> bool {
        self.work += 1;
        if depth > MAX_DEPTH || self.work > MAX_WORK || (self.work.is_multiple_of(1024) && self.watch.as_ref().is_some_and(|w| w.cancelled())) {
            return false;
        }
        if let Some(mapped) = self.forward.get(&a) {
            return *mapped == b;
        }
        if self.reverse.contains_key(&b) {
            return false;
        }
        let (Some(ae), Some(be)) = (ex.get(a), ex.get(b)) else { return false };
        self.forward.insert(a, b);
        self.reverse.insert(b, a);
        ae.records.len() == be.records.len()
            && ae.records.iter().zip(&be.records).all(|(a, b)| {
                a.name == b.name
                    && a.params.len() == b.params.len()
                    && a.params
                        .iter()
                        .zip(&b.params)
                        .enumerate()
                        .all(|(i, (a, b))| (i == 0 && matches!((a, b), (Param::Str(_), Param::Str(_)))) || self.param(ex, a, b, depth + 1))
            })
    }

    fn param(&mut self, ex: &Exchange, a: &Param, b: &Param, depth: usize) -> bool {
        self.work += 1;
        if depth > MAX_DEPTH || self.work > MAX_WORK || (self.work.is_multiple_of(1024) && self.watch.as_ref().is_some_and(|w| w.cancelled())) {
            return false;
        }
        match (a, b) {
            (Param::Ref(a), Param::Ref(b)) => self.entity(ex, *a, *b, depth + 1),
            (Param::List(a), Param::List(b)) => self.params(ex, a, b, depth + 1),
            (Param::Typed(an, a), Param::Typed(bn, b)) => an == bn && self.params(ex, a, b, depth + 1),
            (Param::Real(a), Param::Real(b)) => a.is_finite() && a.to_bits() == b.to_bits(),
            _ => a == b,
        }
    }

    fn params(&mut self, ex: &Exchange, a: &[Param], b: &[Param], depth: usize) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.param(ex, a, b, depth))
    }
}

pub(super) fn find(ex: &Exchange) -> Sharing {
    let mut roots: Vec<_> = ex.entities.iter().filter(|(_, e)| is_body(e)).map(|(id, _)| *id).collect();
    roots.sort_unstable();
    let mut hashes = Fingerprints { ex, memo: HashMap::new(), path: Vec::with_capacity(MAX_DEPTH), work: 0, watch: crate::watch::current() };
    let mut candidates: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut compare = Compare { watch: crate::watch::current(), ..Default::default() };
    let mut sharing = Sharing::default();
    let mut copies = Vec::new();
    let watch = crate::watch::current();
    for item in roots {
        if watch.as_ref().is_some_and(|w| w.cancelled()) || hashes.work > MAX_WORK || compare.work > MAX_WORK {
            break;
        }
        let Some(hash) = hashes.entity(item) else { continue };
        let bucket = candidates.entry(hash).or_default();
        let mut matched = false;
        for source in bucket.iter().copied() {
            compare.forward.clear();
            compare.reverse.clear();
            if !compare.entity(ex, item, source, 0) {
                continue;
            }
            let mut labels = Vec::new();
            for (&id, &canonical) in &compare.forward {
                if let (Some(a), Some(b)) = (ex.get(id), ex.get(canonical)) {
                    for (record, (a, b)) in a.records.iter().zip(&b.records).enumerate() {
                        if let (Some(Param::Str(a)), Some(Param::Str(b))) = (a.params.first(), b.params.first())
                            && a != b
                        {
                            labels.push((id, record, a.clone()));
                        }
                    }
                }
            }
            labels.sort_unstable_by_key(|(id, record, _)| (*id, *record));
            let mut entities: Vec<_> = compare.forward.iter().map(|(a, b)| [*a, *b]).collect();
            entities.sort_unstable_by_key(|p| p[1]);
            copies.push(SourceCopy { item, source, entities, labels });
            sharing.canonical.insert(item, source);
            matched = true;
            break;
        }
        if !matched {
            bucket.push(item);
        }
    }
    copies.sort_unstable_by_key(|c| c.item);
    sharing.copies = copies.into();
    sharing
}

pub(super) fn is_body(e: &Entity) -> bool {
    matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" | "FACETED_BREP" | "SHELL_BASED_SURFACE_MODEL")
}

pub(super) fn removable(copies: &[SourceCopy]) -> HashSet<u64> {
    let mut pinned: HashSet<_> = copies.iter().flat_map(|c| c.entities.iter().map(|p| p[1])).collect();
    let mut remove = HashSet::new();
    for id in copies.iter().flat_map(|c| c.entities.iter().map(|p| p[0])) {
        if !pinned.contains(&id) && !remove.insert(id) {
            // Keep a common subgraph's original record instead of restoring it per copy.
            remove.remove(&id);
            pinned.insert(id);
        }
    }
    remove
}
