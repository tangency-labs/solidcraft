//! Drawing-owned, whole-profile sheet fills and explicit pattern snapshots (W38d).
//! `last_region` is an admitted retention snapshot, not a substituted identity:
//! when the complete outer-plus-holes boundary no longer resolves it is drawn
//! with lost status, and exports refuse it until the association is repaired.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use solidcore_ops::profile::MAX_FAMILIES;
use solidcore_record::profile::line_family::MAX_DASHES;
use solidcore_record::{Region2, Vec2};
use solidcraft_geom::paper::{Group, Item, ObjectKind, Scene};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{CurveKind, Profile, find_profiles_checked};

use super::hatch::{MAX_DESCRIPTION_CHARS, MAX_NAME_BYTES, Pattern};
use super::{Drawing, Owner, require};
use crate::profile_refs::{WHOLE_MAX_BOUNDARY_CURVES, WHOLE_MAX_HOLES, WHOLE_MAX_ID_BYTES, WHOLE_MAX_TOTAL_CURVES};
use crate::{DocError, Document, Result, WholeProfileBoundary, ids};

pub const MAX_SKETCH_FILLS: usize = 1000;
pub const MAX_PATTERN_SNAPSHOTS: usize = 1000;
/// Retained boundary pieces across every fill of a drawing, not per fill.
pub const MAX_FILL_REGION_PIECES: usize = 8192;
/// One refresh solves each affected filled sketch once, at most this many.
pub const MAX_FILLED_SKETCHES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSnapshot {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// Complete definition; duplicate names remain distinct by drawing-global id.
    pub pattern: Pattern,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FillPaint {
    Solid,
    Hatch {
        #[serde(with = "crate::ids")]
        pattern: u64,
        /// The physical pen diameter of lines and dots, explicitly authored.
        pen_width_mm: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SketchFill {
    #[serde(with = "crate::ids")]
    pub id: u64,
    #[serde(with = "crate::ids")]
    pub sketch: u64,
    pub boundary: WholeProfileBoundary,
    pub paint: FillPaint,
    /// Last complete admitted region, retained across loss and save/reopen.
    /// Its validity is checked independently of the live source association.
    pub last_region: Region2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillRegionStatus {
    Current,
    LostBoundary(String),
}

impl FillRegionStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::LostBoundary(_) => "lost_boundary",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedSketchFill {
    pub drawing: u64,
    pub sheet: u64,
    pub id: u64,
    pub sketch: u64,
    pub region: Region2,
    pub paint: FillPaint,
    pub status: FillRegionStatus,
}

#[derive(Clone, Debug)]
pub struct SheetFillProfile {
    pub boundary: WholeProfileBoundary,
    pub region: Region2,
}

#[derive(Clone, Debug)]
pub struct SheetFillProfiles {
    pub profiles: Vec<SheetFillProfile>,
    /// Profiles without certified complete source-curve coverage are not picks.
    pub uncertified: usize,
}

fn invalid(message: impl std::fmt::Display) -> DocError {
    DocError::Invalid(format!("sheet fill: {message}"))
}

fn named(drawing: &Drawing, fill: &SketchFill, message: impl std::fmt::Display) -> String {
    format!("drawing {} sketch {} fill {}: {message}", ids::encode(drawing.id), ids::encode(fill.sketch), ids::encode(fill.id))
}

fn pieces(region: &Region2) -> usize {
    region.loops().fold(0usize, |n, boundary| n.saturating_add(boundary.segs.len()))
}

fn check_region(region: &Region2) -> Result<()> {
    require(region.holes.len() <= WHOLE_MAX_HOLES, || format!("sheet fill: retained region exceeds {WHOLE_MAX_HOLES} holes"))?;
    require(pieces(region) <= MAX_FILL_REGION_PIECES, || format!("sheet fill: retained region exceeds {MAX_FILL_REGION_PIECES} pieces"))?;
    // Solid paint keeps every structurally valid exact paper segment. D6's
    // narrower hatch clipping domain is checked only during hatch expansion.
    let group = Group { object: 1, kind: ObjectKind::Fill, items: vec![Item::Fill { region: region.clone() }] };
    Scene::new(Vec2::new(1.0, 1.0), vec![group]).validate().map_err(invalid)
}

fn retained_budget(fills: &[Arc<SketchFill>]) -> Result<()> {
    let mut count = 0usize;
    for fill in fills {
        require(fill.last_region.holes.len() <= WHOLE_MAX_HOLES, || format!("sheet fill: retained region exceeds {WHOLE_MAX_HOLES} holes"))?;
        count = count
            .checked_add(pieces(&fill.last_region))
            .filter(|n| *n <= MAX_FILL_REGION_PIECES)
            .ok_or_else(|| invalid(format!("retained regions exceed {MAX_FILL_REGION_PIECES} boundary pieces")))?;
    }
    Ok(())
}

/// Raw array admission precedes typed Vec deserialization. Encoded bytes are
/// independently bounded by Drawing::from_json and the format reader.
pub(super) fn check_raw(value: &Value) -> Result<()> {
    for (key, limit) in [("fills", MAX_SKETCH_FILLS), ("patterns", MAX_PATTERN_SNAPSHOTS)] {
        if let Some(records) = value.get(key) {
            let records = records.as_array().ok_or_else(|| invalid(format!("`{key}` must be a list")))?;
            require(records.len() <= limit, || format!("sheet fill: raw `{key}` exceeds {limit} records"))?;
        }
    }
    check_raw_patterns(value.get("patterns").and_then(Value::as_array).into_iter().flatten())?;
    check_raw_fills(value.get("fills").and_then(Value::as_array).into_iter().flatten())
}

pub(super) fn check_raw_patterns<'a>(records: impl IntoIterator<Item = &'a Value>) -> Result<()> {
    for snapshot in records {
        if let Some(pattern) = snapshot.get("pattern") {
            if let Some(name) = pattern.get("name").and_then(Value::as_str) {
                require(name.len() <= MAX_NAME_BYTES, || "sheet fill: raw pattern name exceeds 31 bytes".into())?;
            }
            if let Some(description) = pattern.get("description").and_then(Value::as_str) {
                let limit = MAX_DESCRIPTION_CHARS;
                require(description.len() <= limit * 4 && description.chars().count() <= limit, || {
                    "sheet fill: raw pattern description exceeds 80 characters".into()
                })?;
            }
            if let Some(families) = pattern.get("families").and_then(Value::as_array) {
                require(families.len() <= MAX_FAMILIES, || "sheet fill: raw pattern families exceeds 64".into())?;
                for family in families {
                    if let Some(dashes) = family.get("dashes").and_then(Value::as_array) {
                        require(dashes.len() <= MAX_DASHES, || "sheet fill: raw pattern dashes exceeds 6".into())?;
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn check_raw_fills<'a>(records: impl IntoIterator<Item = &'a Value>) -> Result<()> {
    let mut count = 0usize;
    let mut curve_ids = 0usize;
    for fill in records {
        if let Some(boundary) = fill.get("boundary") {
            let holes = boundary.get("holes").and_then(Value::as_array);
            require(holes.is_none_or(|holes| holes.len() <= WHOLE_MAX_HOLES), || "sheet fill: raw boundary exceeds 128 holes".into())?;
            for ids in boundary.get("outer").into_iter().chain(holes.into_iter().flatten()) {
                if let Some(ids) = ids.as_array() {
                    require(ids.len() <= WHOLE_MAX_BOUNDARY_CURVES, || "sheet fill: raw boundary exceeds 512 curve ids".into())?;
                    curve_ids = curve_ids
                        .checked_add(ids.len())
                        .filter(|n| *n <= WHOLE_MAX_TOTAL_CURVES)
                        .ok_or_else(|| invalid("raw boundaries exceed 20000 total curve ids"))?;
                    for id in ids {
                        if let Some(id) = id.as_str() {
                            require(!id.is_empty() && id.len() <= WHOLE_MAX_ID_BYTES && !id.chars().any(char::is_control), || {
                                "sheet fill: raw boundary has an invalid curve id".into()
                            })?;
                        }
                    }
                }
            }
        }
        if let Some(region) = fill.get("last_region") {
            let holes = region.get("holes").and_then(Value::as_array);
            require(holes.is_none_or(|holes| holes.len() <= WHOLE_MAX_HOLES), || "sheet fill: raw retained region exceeds 128 holes".into())?;
            for boundary in region.get("outer").into_iter().chain(holes.into_iter().flatten()) {
                if let Some(segs) = boundary.get("segs").and_then(Value::as_array) {
                    count = count
                        .checked_add(segs.len())
                        .filter(|n| *n <= MAX_FILL_REGION_PIECES)
                        .ok_or_else(|| invalid(format!("raw retained regions exceed {MAX_FILL_REGION_PIECES} boundary pieces")))?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn check_drawing(drawing: &Drawing) -> Result<()> {
    require(drawing.fills.len() <= MAX_SKETCH_FILLS, || format!("sheet fill: more than {MAX_SKETCH_FILLS} fills"))?;
    require(drawing.patterns.len() <= MAX_PATTERN_SNAPSHOTS, || format!("sheet fill: more than {MAX_PATTERN_SNAPSHOTS} patterns"))?;
    let filled: BTreeSet<_> = drawing.fills.iter().map(|fill| fill.sketch).collect();
    require(filled.len() <= MAX_FILLED_SKETCHES, || format!("sheet fill: more than {MAX_FILLED_SKETCHES} filled sketches"))?;
    retained_budget(&drawing.fills)?;
    let mut curve_ids = 0usize;
    for fill in &drawing.fills {
        require(fill.boundary.holes.len() <= WHOLE_MAX_HOLES, || "sheet fill: boundary exceeds 128 holes".into())?;
        for boundary in std::iter::once(&fill.boundary.outer).chain(&fill.boundary.holes) {
            require(boundary.len() <= WHOLE_MAX_BOUNDARY_CURVES, || "sheet fill: boundary exceeds 512 curve ids".into())?;
            curve_ids = curve_ids
                .checked_add(boundary.len())
                .filter(|n| *n <= WHOLE_MAX_TOTAL_CURVES)
                .ok_or_else(|| invalid("boundaries exceed 20000 total curve ids"))?;
        }
    }
    for pattern in &drawing.patterns {
        pattern.pattern.validate().map_err(|e| invalid(format!("pattern {}: {e}", ids::encode(pattern.id))))?;
    }
    for fill in &drawing.fills {
        let owner = drawing.sheet_sketch(fill.sketch).map(|sketch| sketch.owner);
        require(owner.is_some(), || named(drawing, fill, "owning sheet sketch does not exist"))?;
        // Its region would go on paper unscaled and unplaced: model millimetres, not paper.
        require(!matches!(owner, Some(Owner::View(_))), || named(drawing, fill, "not supported yet: a fill on a view sketch"))?;
        crate::profile_refs::validate_whole(std::slice::from_ref(&fill.boundary))?;
        check_region(&fill.last_region).map_err(|e| invalid(named(drawing, fill, e)))?;
        if let FillPaint::Hatch { pattern, pen_width_mm } = &fill.paint {
            require(drawing.hatch_pattern(*pattern).is_some(), || named(drawing, fill, "pattern snapshot does not exist"))?;
            require(*pen_width_mm > 0.0 && *pen_width_mm <= solidcraft_geom::paper::MAX_LINE_MM, || {
                named(drawing, fill, "pen width must be finite, above zero and at most 10 paper mm")
            })?;
        }
    }
    Ok(())
}

pub(super) fn serialize_fills<S: Serializer>(records: &[Arc<SketchFill>], serializer: S) -> std::result::Result<S::Ok, S::Error> {
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|record| record.id);
    ordered.serialize(serializer)
}

pub(super) fn serialize_patterns<S: Serializer>(records: &[Arc<PatternSnapshot>], serializer: S) -> std::result::Result<S::Ok, S::Error> {
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|record| record.id);
    ordered.serialize(serializer)
}

impl Drawing {
    pub fn sheet_fill(&self, id: u64) -> Option<&Arc<SketchFill>> {
        self.fills.iter().find(|fill| fill.id == id)
    }

    pub fn hatch_pattern(&self, id: u64) -> Option<&Arc<PatternSnapshot>> {
        self.patterns.iter().find(|pattern| pattern.id == id)
    }

    /// Strict, complete profile search of this prospective drawing's own sketch.
    /// Never substitutes the design's previously published drawing record.
    fn fill_profiles(&self, design: &Document, sketch: u64) -> Result<Vec<Profile>> {
        let solved = self.solve_sheet_sketch(design, sketch)?;
        require(solved.sketch.equations.is_empty(), || {
            format!(
                "sheet fill: drawing {} sketch {} equation metadata is not supported by exact paper conversion",
                ids::encode(self.id),
                ids::encode(sketch)
            )
        })?;
        // The shared conversion helpers normalize these cases. Complete graph
        // coverage cannot make the resulting altered curve an exact boundary.
        for (index, curve) in solved.sketch.curves.iter().enumerate() {
            if curve.construction || curve.centerline || curve.infinite {
                continue;
            }
            let fail = |why: &str| invalid(format!("drawing {} sketch {} curve {}: {why}", ids::encode(self.id), ids::encode(sketch), curve.id));
            if let CurveKind::Conic { rho, .. } = curve.kind
                && 1.0 - rho < 1e-9
            {
                return Err(fail("conic rho is too close to one for exact paper conversion"));
            }
            if let CurveKind::Arc { c, a, b } = curve.kind {
                let point = |id| solved.sketch.point(id).ok_or_else(|| fail("arc references an unknown point"));
                let (center, start, end) = (point(c)?, point(a)?, point(b)?);
                if start != end {
                    let sweep = solidcore_ops::prism::counterclockwise_sweep(center, start, end)
                        .ok_or_else(|| fail("arc has no finite nonzero radial directions"))?;
                    if sweep <= 1e-12 {
                        return Err(fail("noncoincident arc sweep at or below 1e-12 radians cannot be converted exactly"));
                    }
                }
                if !solved.sketch.segs(index).last().is_some_and(|piece| piece.end().dist(end) <= solidcraft_geom::EPS) {
                    return Err(fail("analytic arc does not retain its authored end within the paper endpoint tolerance"));
                }
            }
        }
        let search = find_profiles_checked(&solved.sketch);
        require(!search.incomplete, || "sheet fill: profile search is incomplete; no boundary can be certified".into())?;
        let mut count = 0usize;
        for profile in &search.profiles {
            count = count
                .checked_add(pieces(&profile.region))
                .filter(|n| *n <= MAX_FILL_REGION_PIECES)
                .ok_or_else(|| invalid(format!("profile search exceeds {MAX_FILL_REGION_PIECES} boundary pieces")))?;
        }
        for profile in &search.profiles {
            check_region(&profile.region)?;
        }
        Ok(search.profiles)
    }

    pub fn sheet_fill_profiles(&self, design: &Document, sketch: u64) -> Result<SheetFillProfiles> {
        let profiles = self.fill_profiles(design, sketch)?;
        let mut certified = Vec::new();
        let mut uncertified = 0usize;
        for profile in profiles {
            if let Some((outer, holes)) = profile.whole_curve_boundaries() {
                certified.push(SheetFillProfile { boundary: WholeProfileBoundary { outer, holes }, region: profile.region });
            } else {
                uncertified += 1;
            }
        }
        Ok(SheetFillProfiles { profiles: certified, uncertified })
    }

    pub fn sheet_fill_region(&self, design: &Document, sketch: u64, boundary: &WholeProfileBoundary) -> Result<Region2> {
        crate::profile_refs::validate_whole(std::slice::from_ref(boundary))?;
        let profiles = self.fill_profiles(design, sketch)?;
        let index = crate::profile_refs::whole_profile_indices_in(&profiles, "sheet sketch", std::slice::from_ref(boundary))?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("complete boundary is absent"))?;
        let region = profiles.get(index).ok_or_else(|| invalid("complete boundary disappeared"))?.region.clone();
        check_region(&region)?;
        Ok(region)
    }

    pub fn resolve_sheet_fill(&self, design: &Document, id: u64) -> Result<ResolvedSketchFill> {
        self.validate()?;
        let fill = self.sheet_fill(id).ok_or_else(|| invalid(format!("no fill {}", ids::encode(id))))?;
        self.resolve_sheet_fill_regions(design, fill.sketch)?
            .into_iter()
            .find(|resolved| resolved.id == id)
            .ok_or_else(|| invalid("selected fill disappeared"))
    }

    /// One solve and complete profile search shared by every fill of this sketch.
    /// Consumers may cache this source/context-bound batch, expanding one paint
    /// at a time. Failed exact associations keep each fill's own retained region.
    pub fn resolve_sheet_fill_regions(&self, design: &Document, sketch: u64) -> Result<Vec<ResolvedSketchFill>> {
        self.validate()?;
        let record = self.sheet_sketch(sketch).ok_or_else(|| invalid("owning sketch disappeared"))?;
        let sheet = self.sketch_sheet(record).ok_or_else(|| invalid("owning sketch's owner disappeared"))?;
        let profiles = self.fill_profiles(design, sketch)?;
        let mut out = Vec::new();
        for fill in self.fills.iter().filter(|fill| fill.sketch == sketch) {
            let (region, status) =
                match crate::profile_refs::whole_profile_indices_in(&profiles, "sheet sketch", std::slice::from_ref(&fill.boundary)) {
                    Ok(indices) => {
                        let profile =
                            indices.first().and_then(|index| profiles.get(*index)).ok_or_else(|| invalid("complete boundary disappeared"))?;
                        (profile.region.clone(), FillRegionStatus::Current)
                    }
                    Err(error @ DocError::Unknown(_)) => (fill.last_region.clone(), FillRegionStatus::LostBoundary(named(self, fill, error))),
                    Err(error) => return Err(invalid(named(self, fill, error))),
                };
            out.push(ResolvedSketchFill { drawing: self.id, sheet, id: fill.id, sketch, region, paint: fill.paint.clone(), status });
        }
        Ok(out)
    }

    /// Atomically refresh retention snapshots for explicitly affected sketches.
    /// Each sketch is solved/searched once for all its fills. Failed associations
    /// retain their existing snapshot; no profile is guessed or repaired.
    pub fn refresh_fill_regions(&mut self, design: &Document, affected: &BTreeSet<u64>) -> Result<()> {
        self.validate()?;
        let selected: BTreeSet<_> = self.fills.iter().filter(|fill| affected.contains(&fill.sketch)).map(|fill| fill.sketch).collect();
        require(selected.len() <= MAX_FILLED_SKETCHES, || format!("sheet fill: refresh exceeds {MAX_FILLED_SKETCHES} sketches"))?;
        let mut profiles = BTreeMap::new();
        let mut searched_pieces = 0usize;
        for sketch in selected {
            let result = self.fill_profiles(design, sketch);
            if let Ok(found) = &result {
                for profile in found {
                    searched_pieces = searched_pieces
                        .checked_add(pieces(&profile.region))
                        .filter(|n| *n <= MAX_FILL_REGION_PIECES)
                        .ok_or_else(|| invalid(format!("refresh exceeds {MAX_FILL_REGION_PIECES} profile boundary pieces")))?;
                }
            }
            profiles.insert(sketch, result);
        }
        let mut next = self.fills.clone();
        for slot in &mut next {
            let Some(Ok(profiles)) = profiles.get(&slot.sketch) else { continue };
            let Ok(indices) = crate::profile_refs::whole_profile_indices_in(profiles, "sheet sketch", std::slice::from_ref(&slot.boundary)) else {
                continue;
            };
            let Some(profile) = indices.first().and_then(|index| profiles.get(*index)) else { continue };
            if check_region(&profile.region).is_ok() && slot.last_region != profile.region {
                let mut fill = (**slot).clone();
                fill.last_region = profile.region.clone();
                *slot = Arc::new(fill);
            }
        }
        retained_budget(&next)?;
        if let Some(file) = &self.newer_file {
            file.check_fill_budget(next.iter().map(AsRef::as_ref))?;
        }
        self.fills = next;
        Ok(())
    }

    /// Carry only matching previous fill identities across source changes, then
    /// resolve this prospective drawing against the prospective design. No
    /// saved Drawing is looked up through either Document.
    pub fn refresh_fill_regions_after_source_change(&mut self, before: &Drawing, before_design: &Document, after_design: &Document) -> Result<()> {
        require(self.id == before.id, || "sheet fill: previous drawing owner differs".into())?;
        let affected: BTreeSet<_> = self.fills.iter().map(|fill| fill.sketch).collect();
        let mut old = before.clone();
        old.refresh_fill_regions(before_design, &affected)?;
        let mut next = self.clone();
        for slot in &mut next.fills {
            if let Some(previous) = old.sheet_fill(slot.id)
                && previous.sketch == slot.sketch
                && previous.boundary == slot.boundary
                && previous.last_region != slot.last_region
            {
                let mut fill = (**slot).clone();
                fill.last_region = previous.last_region.clone();
                *slot = Arc::new(fill);
            }
        }
        next.refresh_fill_regions(after_design, &affected)?;
        *self = next;
        Ok(())
    }
}
