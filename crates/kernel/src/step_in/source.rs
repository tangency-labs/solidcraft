//! Versioned STEP sources. Shared records retain original identities through explicit maps.
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::p21::{self, Entity, Exchange, Param, Record};
use super::{StepImport, StepReading};
use crate::{KernelError, Result};

/// An import's authoritative source; legacy designs serialize the text as before.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepSource {
    Text(Arc<StepText>),
    Shared(Arc<StepLibrary>),
}

/// Immutable legacy text with digests shared by every feature using that source.
#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepText {
    text: Arc<str>,
    #[serde(skip)]
    fingerprint: OnceLock<u64>,
    #[serde(skip)]
    digest: OnceLock<String>,
}
impl std::ops::Deref for StepText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}
impl PartialEq for StepText {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}
impl Eq for StepText {}

/// STEP records stored once, with copies described by original-to-source entity mappings.
/// This is source data, independent of a kernel build or an evaluated-geometry cache.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepLibrary {
    version: u32,
    data: String,
    copies: Arc<[SourceCopy]>,
    bytes: usize,
    sha256: String,
    #[serde(skip)]
    fingerprint: OnceLock<u64>,
}

impl PartialEq for StepLibrary {
    fn eq(&self, other: &Self) -> bool {
        (self.version, &self.data, &self.copies, self.bytes, &self.sha256) == (other.version, &other.data, &other.copies, other.bytes, &other.sha256)
    }
}
impl Eq for StepLibrary {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceCopy {
    pub item: u64,
    pub source: u64,
    /// Sorted by source id; each view has its own bijective topology mapping.
    pub entities: Vec<[u64; 2]>,
    pub labels: Vec<(u64, usize, String)>,
}

fn invalid(message: impl std::fmt::Display) -> KernelError {
    KernelError::Invalid(format!("shared STEP source: {message}"))
}

impl Default for StepSource {
    fn default() -> Self {
        "".into()
    }
}

impl From<String> for StepSource {
    fn from(text: String) -> Self {
        Arc::<str>::from(text).into()
    }
}

impl From<&str> for StepSource {
    fn from(text: &str) -> Self {
        Arc::<str>::from(text).into()
    }
}

impl From<Arc<str>> for StepSource {
    fn from(text: Arc<str>) -> Self {
        Self::Text(Arc::new(StepText { text, fingerprint: OnceLock::new(), digest: OnceLock::new() }))
    }
}

impl StepSource {
    /// Compact only verified copies; failure keeps the original source intact.
    pub fn compact(text: String, import: &Arc<StepImport>) -> Self {
        if import.sharing.copies.is_empty() {
            return text.into();
        }
        let remove = super::sharing::removable(&import.sharing.copies);
        if remove.is_empty() {
            return text.into();
        }
        let source = match without_entities(&text, &remove) {
            Ok(data) => {
                let mut library = StepLibrary {
                    version: 1,
                    data,
                    copies: import.sharing.copies.clone(),
                    bytes: text.len(),
                    sha256: String::new(),
                    fingerprint: OnceLock::new(),
                };
                library.sha256 = library.content_digest();
                let mut size = JsonSize(0);
                if serde_json::to_writer(&mut size, &library).is_ok() && size.0 < text.len() { Self::Shared(Arc::new(library)) } else { text.into() }
            }
            Err(_) => text.into(),
        };
        if matches!(source, Self::Shared(_)) {
            super::remember((source.fingerprint(), source.len()), import, source.len() <= super::KEEP_SMALL_BYTES);
        }
        source
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Text(text) if text.is_empty())
    }

    /// Original source size, also the bound on expanding its shared records.
    pub fn len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Shared(source) => source.bytes,
        }
    }

    /// Shared ownership identity, without comparing an unchanged large payload.
    pub fn same_source(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => Arc::ptr_eq(a, b),
            (Self::Shared(a), Self::Shared(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub fn fingerprint(&self) -> u64 {
        match self {
            Self::Text(text) => *text.fingerprint.get_or_init(|| {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                text.text.hash(&mut h);
                h.finish()
            }),
            Self::Shared(source) => *source.fingerprint.get_or_init(|| {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (source.version, &source.data, &source.copies, source.bytes, &source.sha256).hash(&mut h);
                h.finish()
            }),
        }
    }

    /// The authoritative source certificate, stable across save/reopen.
    pub fn payload_digest(&self) -> String {
        match self {
            Self::Text(text) => text.digest.get_or_init(|| digest(text.as_bytes())).clone(),
            Self::Shared(source) => source.sha256.clone(),
        }
    }

    pub fn import_shared(&self) -> Result<Arc<StepImport>> {
        let key = (self.fingerprint(), self.len());
        if let Ok(cache) = super::RECENT.lock()
            && let Some(import) = cache.iter().filter(|r| r.key == key).find_map(|r| r.import.upgrade())
        {
            return Ok(import);
        }
        let reading = match self {
            Self::Text(text) => StepReading::new(text)?,
            Self::Shared(source) => StepReading::with_exchange(source.exchange()?, key, false, super::parallel::workers())?,
        };
        reading.finish_shared()
    }

    /// Prepare selected source placements only; an independent part never rebuilds its assembly.
    pub fn import_only(&self, indices: &[u64]) -> Result<Arc<StepImport>> {
        if indices.is_empty() || indices.len() > 10_000 || indices.windows(2).any(|p| p[0] >= p[1]) {
            return Err(invalid("body selection must be a sorted, nonempty set within the body budget"));
        }
        let full = (self.fingerprint(), self.len());
        let mut h = std::collections::hash_map::DefaultHasher::new();
        full.hash(&mut h);
        indices.hash(&mut h);
        let key = (h.finish(), self.len());
        let cached = super::RECENT.lock().ok().and_then(|cache| {
            cache.iter().filter(|r| r.key == key).find_map(|r| r.import.upgrade().map(|i| (i, true, r.key))).or_else(|| {
                cache.iter().filter(|r| r.source == full).find_map(|r| {
                    let import = r.import.upgrade()?;
                    indices
                        .iter()
                        .all(|index| import.bodies.binary_search_by_key(index, |b| b.planned_index as u64).is_ok())
                        .then_some((import, false, r.key))
                })
            })
        });
        if let Some((import, selected, cached_key)) = cached {
            if selected {
                return Ok(import);
            }
            let selected: HashSet<_> = indices.iter().copied().collect();
            let view = Arc::new(StepImport {
                bodies: import.bodies.iter().filter(|b| selected.contains(&(b.planned_index as u64))).cloned().collect(),
                tree: Vec::new(),
                warnings: import.warnings.clone(),
                schema: import.schema.clone(),
                system: import.system.clone(),
                preprocessor: import.preprocessor.clone(),
                description: import.description.clone(),
                sharing: import.sharing.clone(),
            });
            if view.bodies.len() != selected.len() {
                return Err(invalid("body selection is outside the source"));
            }
            // Touch the superset as well: a sequence of independent copies can keep borrowing it
            // instead of evicting it with its own small views after four copies.
            super::remember_selection(cached_key, full, &import, self.len() <= super::KEEP_SMALL_BYTES);
            super::remember_selection(key, full, &view, self.len() <= super::KEEP_SMALL_BYTES);
            return Ok(view);
        }
        let mut reading = match self {
            Self::Text(text) => StepReading::new(text)?,
            Self::Shared(source) => StepReading::with_exchange(source.exchange()?, key, false, super::parallel::workers())?,
        };
        reading.key = key;
        reading.rd.selection = Some(indices.iter().map(|i| usize::try_from(*i).map_err(invalid)).collect::<Result<_>>()?);
        if reading.rd.selection.as_ref().is_some_and(|s| s.is_empty() || s.iter().any(|i| *i >= reading.rd.plan.len())) {
            return Err(invalid("body selection is outside the source"));
        }
        let import = reading.finish_shared()?;
        super::remember_selection(key, full, &import, self.len() <= super::KEEP_SMALL_BYTES);
        Ok(import)
    }

    /// Expand explicitly for interchange callers; evaluation reads the library directly.
    pub fn expanded_text(&self) -> Result<std::borrow::Cow<'_, str>> {
        match self {
            Self::Text(text) => Ok(std::borrow::Cow::Borrowed(text)),
            Self::Shared(source) => {
                let ex = source.exchange()?;
                let mut out = String::with_capacity(source.data.len());
                let data = ex.data_start.ok_or_else(|| invalid("missing DATA section"))?;
                out.push_str(source.data.get(..data).ok_or_else(|| invalid("DATA section"))?);
                out.push('\n');
                let mut ids: Vec<_> = ex.entities.keys().copied().collect();
                ids.sort_unstable();
                for id in ids {
                    let e = ex.get(id).ok_or_else(|| invalid("missing entity"))?;
                    write!(&mut out, "#{id}=").map_err(invalid)?;
                    if e.is_complex() {
                        out.push('(')
                    }
                    for r in &e.records {
                        out.push_str(&r.name);
                        out.push('(');
                        write_params(&mut out, &r.params)?;
                        out.push(')');
                    }
                    if e.is_complex() {
                        out.push(')')
                    }
                    out.push_str(";\n");
                    if out.len() > p21::MAX_BYTES {
                        return Err(invalid("expanded source too large"));
                    }
                }
                out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
                Ok(std::borrow::Cow::Owned(out))
            }
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(bytes) {
        let _ = write!(out, "{b:02x}");
    }
    out
}

impl StepLibrary {
    fn content_digest(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.version.to_le_bytes());
        h.update((self.bytes as u64).to_le_bytes());
        h.update((self.data.len() as u64).to_le_bytes());
        h.update(self.data.as_bytes());
        h.update((self.copies.len() as u64).to_le_bytes());
        for copy in self.copies.iter() {
            h.update(copy.item.to_le_bytes());
            h.update(copy.source.to_le_bytes());
            h.update((copy.entities.len() as u64).to_le_bytes());
            for [id, source] in &copy.entities {
                h.update(id.to_le_bytes());
                h.update(source.to_le_bytes());
            }
            h.update((copy.labels.len() as u64).to_le_bytes());
            for (id, record, label) in &copy.labels {
                h.update(id.to_le_bytes());
                h.update((*record as u64).to_le_bytes());
                h.update((label.len() as u64).to_le_bytes());
                h.update(label.as_bytes());
            }
        }
        let mut out = String::with_capacity(64);
        for b in h.finalize() {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    fn exchange(&self) -> Result<Arc<Exchange>> {
        if self.version != 1
            || self.bytes > p21::MAX_BYTES
            || self.bytes < self.data.len()
            || self.data.len() > p21::MAX_BYTES
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.copies.len() > 10_000
            || self.copies.is_empty()
        {
            return Err(invalid("version or source budget"));
        }
        if self.content_digest() != self.sha256 {
            return Err(invalid("source certificate mismatch"));
        }
        let mut ex = p21::parse(&self.data).map_err(invalid)?;
        if !ex.duplicates.is_empty() {
            return Err(invalid("duplicate entity ids"));
        }
        let mut reverse = HashMap::new();
        let mut original = HashSet::new();
        let mut restored = HashMap::new();
        let mut costs = HashMap::new();
        let mut bytes = self.data.len();
        let mut work = 0usize;
        let mut expanded_work = 0usize;
        let mut compared_bytes = 0usize;
        let watch = crate::watch::current();
        for (copy_index, copy) in self.copies.iter().enumerate() {
            if watch.as_ref().is_some_and(|w| w.cancelled()) {
                return Err(crate::watch::cancelled_error("STEP source"));
            }
            work = work.saturating_add(copy.entities.len()).saturating_add(copy.labels.len());
            if work > p21::MAX_ENTITIES || ex.entities.len().saturating_add(restored.len()) > p21::MAX_ENTITIES {
                return Err(invalid("entity mapping budget"));
            }
            if copy.entities.windows(2).any(|p| p[0][1] >= p[1][1])
                || copy.labels.windows(2).any(|p| (p[0].0, p[0].1) >= (p[1].0, p[1].1))
                || copy.labels.iter().any(|(_, _, label)| label.len() > 1 << 20)
            {
                return Err(invalid("unordered or excessive copy mapping"));
            }
            reverse.clear();
            original.clear();
            for (i, [id, source]) in copy.entities.iter().enumerate() {
                if i.is_multiple_of(1024) && watch.as_ref().is_some_and(|w| w.cancelled()) {
                    return Err(crate::watch::cancelled_error("STEP source"));
                }
                if *id == 0 || *source == 0 || reverse.insert(*source, *id).is_some() || !original.insert(*id) {
                    return Err(invalid("entity mapping is not bijective"));
                }
            }
            if reverse.get(&copy.source) != Some(&copy.item) {
                return Err(invalid("copy root disagrees with its mapping"));
            }
            for [id, source] in &copy.entities {
                if watch.as_ref().is_some_and(|w| w.cancelled()) {
                    return Err(crate::watch::cancelled_error("STEP source"));
                }
                if ex.entities.contains_key(id) {
                    continue;
                }
                let source_entity = ex.get(*source).ok_or_else(|| invalid("missing canonical entity"))?;
                let cost = match costs.get(source) {
                    Some(cost) => *cost,
                    None => {
                        let cost = CopyCost::entity(source_entity, watch.as_deref())?;
                        costs.insert(*source, cost);
                        cost
                    }
                };
                expanded_work = expanded_work.saturating_add(cost.work);
                if expanded_work > p21::MAX_ENTITIES {
                    return Err(invalid("expanded parameter budget"));
                }
                let labels = copy_labels(copy, *id);
                let mut size = cost.bytes;
                for (_, record, label) in labels {
                    let old = source_entity
                        .records
                        .get(*record)
                        .and_then(|r| r.params.first())
                        .and_then(Param::as_str)
                        .ok_or_else(|| invalid("copy label is not a string"))?;
                    size = size.saturating_sub(old.len()).saturating_add(label.len());
                }
                if let Some((_, old_source, old_copy)) = restored.get(id) {
                    let old_copy = self.copies.get(*old_copy).ok_or_else(|| invalid("copy provenance"))?;
                    if source == old_source && labels == copy_labels(old_copy, *id) {
                        // Older libraries may repeat a shared subgraph's mapping. The source
                        // values are identical; compare only the references that placing it changes.
                        let mut visited = 0usize;
                        for record in &source_entity.records {
                            if !same_refs(&record.params, &reverse, old_copy, watch.as_deref(), &mut visited)? {
                                return Err(invalid("conflicting copied entity"));
                            }
                        }
                        continue;
                    }
                    compared_bytes =
                        compared_bytes.checked_add(size).filter(|n| *n <= p21::MAX_BYTES).ok_or_else(|| invalid("copy comparison budget"))?;
                } else {
                    bytes = bytes.checked_add(size).filter(|n| *n <= self.bytes).ok_or_else(|| invalid("expanded source budget"))?;
                }
                let mut records = Vec::with_capacity(source_entity.records.len());
                let mut labels = labels.iter().peekable();
                for (i, record) in source_entity.records.iter().enumerate() {
                    let mut params = match labels.next_if(|label| label.1 == i) {
                        Some((_, _, label)) => {
                            // Do not clone a potentially large canonical label only to discard it.
                            let mut params = Vec::with_capacity(record.params.len());
                            params.push(Param::Str(label.clone()));
                            params.extend(record.params.iter().skip(1).cloned());
                            params
                        }
                        None => record.params.clone(),
                    };
                    remap(&mut params, &reverse, 0)?;
                    records.push(Record { name: record.name.clone(), params });
                }
                let entity = Entity { records };
                if let Some((previous, _, _)) = restored.get(id) {
                    if previous != &entity {
                        return Err(invalid("conflicting copied entity"));
                    }
                } else {
                    restored.insert(*id, (entity, *source, copy_index));
                }
            }
        }
        if ex.entities.len().saturating_add(restored.len()) > p21::MAX_ENTITIES {
            return Err(invalid("entity budget"));
        }
        ex.entities.extend(restored.into_iter().map(|(id, (entity, _, _))| (id, entity)));
        Ok(Arc::new(ex))
    }
}

fn copy_labels(copy: &SourceCopy, id: u64) -> &[(u64, usize, String)] {
    let first = copy.labels.partition_point(|(entity, _, _)| *entity < id);
    let last = copy.labels.partition_point(|(entity, _, _)| *entity <= id);
    copy.labels.get(first..last).unwrap_or_default()
}

fn same_refs(
    params: &[Param],
    ids: &HashMap<u64, u64>,
    previous: &SourceCopy,
    watch: Option<&dyn crate::watch::Watch>,
    visited: &mut usize,
) -> Result<bool> {
    for p in params {
        *visited += 1;
        if visited.is_multiple_of(1024) && watch.is_some_and(|w| w.cancelled()) {
            return Err(crate::watch::cancelled_error("STEP source"));
        }
        match p {
            Param::Ref(id) => {
                let old = previous.entities.binary_search_by_key(id, |p| p[1]).ok().and_then(|i| previous.entities.get(i)).map(|p| &p[0]);
                if old.is_none() || old != ids.get(id) {
                    return Ok(false);
                }
            }
            Param::List(values) | Param::Typed(_, values) if !same_refs(values, ids, previous, watch, visited)? => return Ok(false),
            _ => {}
        }
    }
    Ok(true)
}

/// A lower bound on the original STEP token bytes, independent of numeric formatting and
/// entity-id width. Count parameter work too, so a tiny list cannot expand into unlimited
/// parsed nodes. Memoized per canonical entity before its first allocation as a copy.
#[derive(Clone, Copy, Default)]
struct CopyCost {
    bytes: usize,
    work: usize,
}

impl CopyCost {
    fn entity(entity: &Entity, watch: Option<&dyn crate::watch::Watch>) -> Result<Self> {
        let mut cost = Self::default();
        for record in &entity.records {
            cost.add(record.name.len(), watch)?;
            cost.params(&record.params, 0, watch)?;
        }
        Ok(cost)
    }

    fn add(&mut self, bytes: usize, watch: Option<&dyn crate::watch::Watch>) -> Result<()> {
        self.bytes = self.bytes.checked_add(bytes).filter(|n| *n <= p21::MAX_BYTES).ok_or_else(|| invalid("expanded source budget"))?;
        self.work = self.work.saturating_add(1);
        if self.work > p21::MAX_ENTITIES {
            return Err(invalid("expanded parameter budget"));
        }
        if self.work.is_multiple_of(1024) && watch.is_some_and(|w| w.cancelled()) {
            return Err(crate::watch::cancelled_error("STEP source"));
        }
        Ok(())
    }

    fn params(&mut self, params: &[Param], depth: usize, watch: Option<&dyn crate::watch::Watch>) -> Result<()> {
        if depth > 64 {
            return Err(invalid("parameter nesting"));
        }
        for p in params {
            let text = match p {
                Param::Str(s) | Param::Enum(s) | Param::Binary(s) | Param::Typed(s, _) => s.len(),
                _ => 0,
            };
            self.add(text.saturating_add(1), watch)?;
            if let Param::List(values) | Param::Typed(_, values) = p {
                self.params(values, depth + 1, watch)?;
            }
        }
        Ok(())
    }
}

fn remap(params: &mut [Param], ids: &HashMap<u64, u64>, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(invalid("parameter nesting"));
    }
    for p in params {
        match p {
            Param::Ref(id) => *id = *ids.get(id).ok_or_else(|| invalid("incomplete entity mapping"))?,
            Param::List(v) | Param::Typed(_, v) => remap(v, ids, depth + 1)?,
            _ => {}
        }
    }
    Ok(())
}

fn write_params(out: &mut String, params: &[Param]) -> Result<()> {
    for (i, p) in params.iter().enumerate() {
        if i != 0 {
            out.push(',')
        }
        match p {
            Param::Int(v) => write!(out, "{v}").map_err(invalid)?,
            Param::Real(v) if v.is_finite() => write!(out, "{v:?}").map_err(invalid)?,
            Param::Real(_) => return Err(invalid("non-finite number")),
            Param::Ref(id) => write!(out, "#{id}").map_err(invalid)?,
            Param::Str(v) => {
                out.push('\'');
                for c in v.chars() {
                    if c == '\'' {
                        out.push('\'')
                    }
                    if c == '\\' {
                        out.push('\\')
                    }
                    out.push(c);
                }
                out.push('\'');
            }
            Param::Enum(v) => {
                out.push('.');
                out.push_str(v);
                out.push('.');
            }
            Param::Binary(v) => {
                out.push('"');
                out.push_str(v);
                out.push('"');
            }
            Param::List(v) => {
                out.push('(');
                write_params(out, v)?;
                out.push(')');
            }
            Param::Typed(name, v) => {
                out.push_str(name);
                out.push('(');
                write_params(out, v)?;
                out.push(')');
            }
            Param::Null => out.push('$'),
            Param::Derived => out.push('*'),
        }
    }
    Ok(())
}

fn without_entities(text: &str, remove: &HashSet<u64>) -> Result<String> {
    let mut length = 0;
    kept_chunks(text, remove, |chunk| length += chunk.len())?;
    let mut out = String::with_capacity(length);
    kept_chunks(text, remove, |chunk| out.push_str(chunk))?;
    Ok(out)
}

fn kept_chunks(text: &str, remove: &HashSet<u64>, mut keep: impl FnMut(&str)) -> Result<()> {
    let bytes = text.as_bytes();
    let watch = crate::watch::current();
    let (mut i, mut kept) = (0usize, 0usize);
    let mut quote = None;
    let mut start = None;
    while let Some(&c) = bytes.get(i) {
        if i.is_multiple_of(65536) && watch.as_ref().is_some_and(|w| w.cancelled()) {
            return Err(crate::watch::cancelled_error("STEP source"));
        }
        if let Some(q) = quote {
            if c == q {
                if bytes.get(i + 1) == Some(&q) {
                    i += 2;
                    continue;
                }
                quote = None;
            }
        } else if c == b'\'' || c == b'"' {
            quote = Some(c);
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = bytes.get(i + 2..).and_then(|s| s.windows(2).position(|p| p == b"*/")).ok_or_else(|| invalid("unterminated comment"))?;
            i += end + 4;
            continue;
        } else if c == b'#' && start.is_none() {
            let mut end = i + 1;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            let id = text.get(i + 1..end).and_then(|s| s.parse::<u64>().ok());
            while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
                end += 1;
            }
            if bytes.get(end) == Some(&b'=') && id.is_some_and(|id| remove.contains(&id)) {
                start = Some(i);
            }
        } else if c == b';'
            && let Some(start) = start.take()
        {
            keep(text.get(kept..start).ok_or_else(|| invalid("record boundary"))?);
            kept = i + 1;
        }
        i += 1;
    }
    if quote.is_some() || start.is_some() {
        return Err(invalid("incomplete record"));
    }
    keep(text.get(kept..).ok_or_else(|| invalid("source boundary"))?);
    Ok(())
}

struct JsonSize(usize);
impl std::io::Write for JsonSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.checked_add(bytes.len()).filter(|n| *n <= p21::MAX_BYTES).ok_or_else(|| std::io::Error::other("STEP source budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_certificate_does_not_allow_copies_to_exceed_the_source_budget() {
        let data = format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;\n\
             #1=DESCRIPTIVE_REPRESENTATION_ITEM('Description','{}');\nENDSEC;END-ISO-10303-21;",
            "x".repeat(4096),
        );
        let copies: Arc<[_]> = (2..18).map(|item| SourceCopy { item, source: 1, entities: vec![[item, 1]], labels: Vec::new() }).collect();
        let mut library =
            StepLibrary { version: 1, bytes: data.len() + copies.len() * 100, data, copies, sha256: String::new(), fingerprint: OnceLock::new() };
        library.sha256 = library.content_digest();
        let source = StepSource::Shared(Arc::new(library));
        assert!(source.expanded_text().unwrap_err().to_string().contains("expanded source budget"));
    }

    #[test]
    fn older_libraries_allow_overlapping_subgraphs_but_not_conflicting_references() {
        let data = format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;\n\
             #1=REPRESENTATION('Definition',(#2),$);\n\
             #2=DESCRIPTIVE_REPRESENTATION_ITEM('Description','{}');\nENDSEC;END-ISO-10303-21;",
            "x".repeat(4096),
        );
        let mut library = StepLibrary {
            version: 1,
            bytes: data.len() * 2 + 200,
            data,
            copies: vec![
                SourceCopy { item: 3, source: 1, entities: vec![[3, 1], [4, 2]], labels: Vec::new() },
                SourceCopy { item: 5, source: 1, entities: vec![[5, 1], [4, 2]], labels: Vec::new() },
            ]
            .into(),
            sha256: String::new(),
            fingerprint: OnceLock::new(),
        };
        library.sha256 = library.content_digest();
        let exchange = library.exchange().unwrap();
        assert_eq!(exchange.entities.len(), 5);
        for id in [3, 5] {
            assert_eq!(exchange.get(id).unwrap().params().get(1), Some(&Param::List(vec![Param::Ref(4)])));
        }
        Arc::make_mut(&mut library.copies)[1] = SourceCopy { item: 3, source: 1, entities: vec![[3, 1], [5, 2]], labels: Vec::new() };
        library.sha256 = library.content_digest();
        assert!(library.exchange().unwrap_err().to_string().contains("conflicting copied entity"));
    }
}
