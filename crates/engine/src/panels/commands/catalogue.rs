//! Small versioned storage seam for application preferences, not design files.
//! Length-prefixed records keep arbitrary recovery bytes exact and bound every
//! allocation before copying. Envelopes retain their foundation JSON limits.
use super::*;

const HEADER: &[u8] = b"SCPCAT01";

impl CatalogueState {
    /// Transform a private candidate, retaining exact unchanged records and all
    /// rejected recovery bytes. A failure never publishes a partially mapped set.
    pub fn map_envelopes(
        &self,
        registry: &PanelRegistry,
        mut map: impl FnMut(&[u8], LayoutEnvelope) -> PanelResult<LayoutEnvelope>,
    ) -> PanelResult<Self> {
        let mut next = self.clone();
        for record in next.builtins.values_mut().chain(next.users.values_mut()).chain(next.remembered.values_mut()) {
            let original = decode_retained_record(record.bytes(), registry)?;
            let mapped = map(record.bytes(), original.clone())?;
            if mapped.layout.workspace != original.layout.workspace {
                return Err(error(Rule::Schema, "catalogue mapper cannot change record workspace"));
            }
            if mapped != original {
                *record = StoredEnvelope::new(&mapped, record.rejected(), registry)?;
            }
        }
        next.validate()?;
        Ok(next)
    }
    /// Export all immutable registrations, mutable users and remembered full
    /// envelopes. No storage, live action, selection or Session is touched.
    pub fn export(&self) -> PanelResult<Vec<u8>> {
        self.validate()?;
        let mut writer = Writer(Vec::new());
        writer.put(HEADER)?;
        for presets in [&self.builtins, &self.users] {
            writer.count(presets.len())?;
            for (name, record) in presets {
                writer.bytes(name.as_bytes())?;
                writer.record(record)?;
            }
        }
        writer.count(self.workspaces.len())?;
        for descriptor in self.workspaces.values() {
            writer.bytes(descriptor.id.0.as_bytes())?;
            writer.bytes(descriptor.title_key.as_bytes())?;
            writer.bytes(descriptor.default_preset.as_bytes())?;
        }
        writer.count(self.remembered.len())?;
        for (workspace, record) in &self.remembered {
            writer.bytes(workspace.0.as_bytes())?;
            writer.record(record)?;
        }
        Ok(writer.0)
    }

    /// Build a private validated candidate. Counts, duplicate identities,
    /// names/IDs, envelope node/depth/byte bounds, rejected recovery and total
    /// budgets are checked before the result can be published by any catalogue.
    pub fn hydrate(bytes: &[u8], registry: &PanelRegistry) -> PanelResult<Self> {
        Ok(Self::hydrate_inner(bytes, registry, None)?.0)
    }

    /// Like `hydrate`, but one stored layout that cannot be read is dropped on its
    /// own instead of rejecting the whole snapshot. A dropped built-in or
    /// remembered layout is replaced by the same entry of `defaults` when it has
    /// one; a dropped user preset is gone. Framing, counts, duplicate identities
    /// and aggregate budgets remain hard errors. The bytes on disk are not touched.
    pub fn hydrate_lenient(bytes: &[u8], registry: &PanelRegistry, defaults: &Self) -> PanelResult<(Self, Vec<SkippedEntry>)> {
        Self::hydrate_inner(bytes, registry, Some(defaults))
    }

    fn hydrate_inner(bytes: &[u8], registry: &PanelRegistry, lenient: Option<&Self>) -> PanelResult<(Self, Vec<SkippedEntry>)> {
        let mut skipped = Vec::new();
        if bytes.len() > MAX_CATALOGUE_SNAPSHOT_BYTES {
            return Err(error(Rule::CollectionLimit, "catalogue snapshot byte cap"));
        }
        let mut reader = Reader { bytes, at: 0 };
        if reader.take(HEADER.len())? != HEADER {
            return Err(error(Rule::Version, "unsupported catalogue snapshot"));
        }
        let mut next = Self::default();
        for builtin in [true, false] {
            let count = reader.count(MAX_USER_PRESETS)?;
            for _ in 0..count {
                let name = reader.text(256)?;
                if preset_name(&name)? != name {
                    return Err(error(Rule::Schema, "stored preset name is not normalised"));
                }
                if next.find_preset_name(&name).is_some() {
                    return Err(error(Rule::DuplicateId, "duplicate stored preset identity"));
                }
                let raw = reader.raw_record()?;
                let record = match StoredEnvelope::from_record(raw.0, raw.1, registry) {
                    Ok(record) => record,
                    Err(_) if lenient.is_some() => {
                        skipped.push(if builtin { SkippedEntry::Builtin(name) } else { SkippedEntry::User(name) });
                        continue;
                    }
                    Err(reason) => return Err(reason),
                };
                if builtin {
                    next.builtins.insert(name, record);
                } else {
                    next.users.insert(name, record);
                }
            }
        }
        for _ in 0..reader.count(MAX_WORKSPACES)? {
            let id = WorkspaceId::new(reader.text(MAX_ID_BYTES)?)?;
            let title_key = reader.text(MAX_ID_BYTES)?;
            valid_id(&title_key)?;
            let default_preset = reader.text(256)?;
            if next.workspaces.contains_key(&id) {
                return Err(error(Rule::DuplicateId, "duplicate stored workspace identity"));
            }
            next.workspaces.insert(id.clone(), WorkspaceDescriptor { id, title_key, default_preset });
        }
        for _ in 0..reader.count(MAX_WORKSPACES)? {
            let workspace = WorkspaceId::new(reader.text(MAX_ID_BYTES)?)?;
            if next.remembered.contains_key(&workspace) {
                return Err(error(Rule::DuplicateId, "duplicate remembered workspace identity"));
            }
            let raw = reader.raw_record()?;
            // A remembered record belongs to its key, unlike a reusable preset.
            let checked =
                StoredEnvelope::from_record(raw.0, raw.1, registry).and_then(|record| record.envelope(registry, &workspace).map(|_| record));
            match checked {
                Ok(record) => {
                    next.remembered.insert(workspace, record);
                }
                Err(_) if lenient.is_some() => skipped.push(SkippedEntry::Remembered(workspace)),
                Err(reason) => return Err(reason),
            }
        }
        if reader.at != bytes.len() {
            return Err(error(Rule::Schema, "trailing catalogue snapshot data"));
        }
        if let Some(defaults) = lenient {
            for entry in &skipped {
                match entry {
                    SkippedEntry::Builtin(name) => {
                        if let Some(record) = defaults.builtins.get(name) {
                            next.builtins.insert(name.clone(), record.clone());
                        }
                    }
                    SkippedEntry::Remembered(workspace) => {
                        if let Some(record) = defaults.remembered.get(workspace) {
                            next.remembered.insert(workspace.clone(), record.clone());
                        }
                    }
                    SkippedEntry::User(_) => {}
                }
            }
        }
        next.validate()?;
        Ok((next, skipped))
    }
}

struct Writer(Vec<u8>);
impl Writer {
    fn put(&mut self, bytes: &[u8]) -> PanelResult<()> {
        if self.0.len().saturating_add(bytes.len()) > MAX_CATALOGUE_SNAPSHOT_BYTES {
            return Err(error(Rule::CollectionLimit, "catalogue snapshot byte cap"));
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn count(&mut self, count: usize) -> PanelResult<()> {
        let count = u32::try_from(count).map_err(|_| error(Rule::CollectionLimit, "catalogue framing cap"))?;
        self.put(&count.to_le_bytes())
    }
    fn bytes(&mut self, bytes: &[u8]) -> PanelResult<()> {
        self.count(bytes.len())?;
        self.put(bytes)
    }
    fn record(&mut self, record: &StoredEnvelope) -> PanelResult<()> {
        self.bytes(record.bytes())?;
        match record.rejected() {
            Some(rejected) => {
                self.put(&[1])?;
                self.bytes(rejected)
            }
            None => self.put(&[0]),
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> PanelResult<&'a [u8]> {
        let end = self.at.checked_add(count).ok_or_else(|| error(Rule::CollectionLimit, "catalogue framing overflow"))?;
        let bytes = self.bytes.get(self.at..end).ok_or_else(|| error(Rule::Schema, "truncated catalogue snapshot"))?;
        self.at = end;
        Ok(bytes)
    }
    fn count(&mut self, max: usize) -> PanelResult<usize> {
        let raw = <[u8; 4]>::try_from(self.take(4)?).map_err(|_| error(Rule::Schema, "invalid catalogue length"))?;
        let count = usize::try_from(u32::from_le_bytes(raw)).map_err(|_| error(Rule::CollectionLimit, "catalogue length overflow"))?;
        if count > max {
            return Err(error(Rule::CollectionLimit, "catalogue record/count cap"));
        }
        Ok(count)
    }
    fn bytes(&mut self, max: usize) -> PanelResult<&'a [u8]> {
        let count = self.count(max)?;
        self.take(count)
    }
    fn text(&mut self, max: usize) -> PanelResult<String> {
        std::str::from_utf8(self.bytes(max)?).map(str::to_owned).map_err(|_| error(Rule::Schema, "invalid catalogue UTF-8"))
    }
    /// Framing only: the record's bytes and recovery bytes, undecoded.
    fn raw_record(&mut self) -> PanelResult<(&'a [u8], Option<&'a [u8]>)> {
        let bytes = self.bytes(MAX_ENVELOPE_BYTES)?;
        let rejected = match self.take(1)? {
            [0] => None,
            [1] => Some(self.bytes(MAX_ENVELOPE_BYTES)?),
            _ => return Err(error(Rule::Schema, "invalid catalogue recovery tag")),
        };
        Ok((bytes, rejected))
    }
}
