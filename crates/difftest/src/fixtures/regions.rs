//! Typed counterparts of the genuine detached `SmRegion` objects in attribute fixtures.

use solidcore::geom::attribute::ObjectKey;
use solidcore::topology::shell::{RegionLookup, RegionRef};
use solidcore::topology::{Brep, BrepId, Region, RegionId};
use solidcore::{Result, SolidError};
use std::collections::BTreeMap;

/// Live region records and only an object-key-to-arena-key identity mapping.
/// The native fixture constructs `SmRegion()` without an owner; no region flags
/// are inferred from attribute contents or cached in this mapping.
pub(crate) struct RegionScene {
    storage: Brep,
    objects: BTreeMap<ObjectKey, RegionId>,
}

impl RegionScene {
    /// Construct counterparts of the fixture's native detached region objects.
    pub(crate) fn orphan_objects(objects: impl IntoIterator<Item = ObjectKey>) -> Result<Self> {
        let mut storage = Brep::new();
        let mut mapping = BTreeMap::new();
        for object in objects {
            if mapping.len() >= 64 || mapping.contains_key(&object) {
                return Err(SolidError::invalid_input(
                    "fixtures::regions",
                    "invalid region object registry",
                ));
            }
            let id = storage.add_region_record(Region::new())?;
            mapping.insert(object, id);
        }
        Ok(Self {
            storage,
            objects: mapping,
        })
    }
}

impl RegionLookup for RegionScene {
    fn resolve_region(&self, object: ObjectKey) -> Result<RegionRef<'_>> {
        let region_id = self.objects.get(&object).copied().ok_or_else(|| {
            SolidError::invalid_input("fixtures::regions", "object is not a registered region")
        })?;
        Ok(RegionRef {
            brep: &self.storage,
            brep_id: BrepId::from_index(0),
            region_id,
        })
    }
}
