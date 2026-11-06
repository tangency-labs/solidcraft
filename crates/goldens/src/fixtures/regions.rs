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
    /// Objects replaced by the infinite region of a B-rep of their own (the shim's
    /// `new SmBrep()`), each with the identity of that B-rep.
    infinite: BTreeMap<ObjectKey, (Brep, BrepId)>,
    /// B-reps made so far: every `new SmBrep()` is a distinct owner.
    breps_made: u32,
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
            infinite: BTreeMap::new(),
            breps_made: 0,
        })
    }

    /// `SmRegion::SetIsVoid` on the object's region record.
    pub(crate) fn set_is_void(&mut self, object: ObjectKey, value: i32) -> Result<()> {
        if let Some((brep, _)) = self.infinite.get_mut(&object) {
            let id = brep.infinite_region().ok_or_else(unregistered)?;
            brep.region_mut(id)?.set_is_void(value);
            return Ok(());
        }
        let id = self
            .objects
            .get(&object)
            .copied()
            .ok_or_else(unregistered)?;
        self.storage.region_mut(id)?.set_is_void(value);
        Ok(())
    }

    /// The object becomes the infinite region of a fresh B-rep (`new SmBrep()`),
    /// with the default (non-void) state, as the shim's replacement object has.
    pub(crate) fn make_infinite(&mut self, object: ObjectKey) -> Result<()> {
        if !self.objects.contains_key(&object) {
            return Err(unregistered());
        }
        let mut brep = Brep::new();
        let mut region = Region::new();
        region.in_brep = true;
        let id = brep.add_region_record(region)?;
        brep.set_infinite_region_record(Some(id));
        self.breps_made += 1;
        let identity = BrepId::from_index(self.breps_made);
        self.infinite.insert(object, (brep, identity));
        Ok(())
    }
}

fn unregistered() -> SolidError {
    SolidError::invalid_input("fixtures::regions", "object is not a registered region")
}

impl RegionLookup for RegionScene {
    fn resolve_region(&self, object: ObjectKey) -> Result<RegionRef<'_>> {
        if let Some((brep, brep_id)) = self.infinite.get(&object) {
            return Ok(RegionRef {
                brep,
                brep_id: *brep_id,
                region_id: brep.infinite_region().ok_or_else(unregistered)?,
            });
        }
        let region_id = self
            .objects
            .get(&object)
            .copied()
            .ok_or_else(unregistered)?;
        Ok(RegionRef {
            brep: &self.storage,
            brep_id: BrepId::from_index(0),
            region_id,
        })
    }
}
