//! Owned layers and entity metadata for partially built DXF files.
//!
//! Generic geometry is carried unchanged and is not validated or serialized by
//! insertion. Concrete DxfShape methods provide conservative bounds and positive
//! transforms in declared supported subsets; text bounds need a layout provider.
//! Existing flat/current-view producers use explicitly bounded compatibility
//! constructors; ordinary files keep strict metadata and geometry caps.
//!
//! Layer declarations currently admit ASCII only. They trim ASCII whitespace,
//! replace restricted characters, and refuse normalized/case collisions.
//! Names admit at most 4096 raw bytes and 255 normalized characters; files hold
//! at most 1024 layers and one million entities. These are resource limits,
//! not a promise of complete DXF import support. Geometry owns its own budget.
//! Standard physical lineweights use the 24 public DXF codes, with exact
//! midpoint ties choosing the heavier weight. Absent and symbolic entity
//! weights stay distinct. Layer zero is initialized by new() or with_layer_zero()
//! and stays immutable after construction. Unicode names remain unsupported.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use solidcore_record::Vec2;

use super::DxfLinetype;
use crate::{IoError, Result};

mod bulges;
mod geometry;
pub use geometry::DxfGeometryBounds;

const STANDARD_WEIGHTS: [u16; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];
const MAX_NAME_CHARS: usize = 255;
// A resource bound on the RAW input, independent of the format-name length
// after trimming. It does not change the normalized format-name bound.
const MAX_RAW_NAME_BYTES: usize = 4096;
const MAX_LAYERS: usize = 1024;
const MAX_ENTITIES: usize = 1_000_000;
static NEXT_OWNER: AtomicUsize = AtomicUsize::new(1);

fn invalid(message: &str) -> IoError {
    IoError::Invalid(message.into())
}

/// A standard physical weight, in hundredths of a millimetre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardWeight(u16);

impl StandardWeight {
    pub fn from_code(code: u16) -> Result<Self> {
        if STANDARD_WEIGHTS.contains(&code) { Ok(Self(code)) } else { Err(invalid("not a standard DXF lineweight")) }
    }

    /// Nearest rational standard weight; exact midpoint ties choose the heavier.
    /// Existing drawing writers are unchanged until their adapters use this model.
    pub fn from_mm(mm: f64) -> Result<Self> {
        if !mm.is_finite() || !(0.0..=2.11).contains(&mm) {
            return Err(invalid("DXF lineweight must be finite and between 0 and 2.11 mm"));
        }
        for pair in STANDARD_WEIGHTS.windows(2) {
            let [low, high] = pair else { return Err(invalid("invalid standard lineweight table")) };
            let midpoint_sum = f64::from(*low + *high);
            // Compare exact 200*mm to the integer midpoint_sum. When its
            // rounded product equals that integer, FMA preserves the sign of
            // the residual. At these nonzero integer midpoints the residual
            // is far above subnormal underflow. No tolerance or mm clamping.
            let scaled = mm * 200.0;
            if scaled < midpoint_sum || (scaled == midpoint_sum && mm.mul_add(200.0, -scaled) < 0.0) {
                return Ok(Self(*low));
            }
        }
        Ok(Self(211))
    }

    pub fn code(self) -> u16 {
        self.0
    }
}

/// Symbols are not negative physical weights. None on Entity means the group
/// was omitted; Some(ByLayer) is an explicitly authored -1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityWeight {
    ByLayer,
    ByBlock,
    Default,
    Explicit(StandardWeight),
}

impl EntityWeight {
    pub fn group370(self) -> i16 {
        match self {
            Self::ByLayer => -1,
            Self::ByBlock => -2,
            Self::Default => -3,
            Self::Explicit(value) => value.code() as i16,
        }
    }
}

/// These are in-memory ownership handles, NOT serialized DXF group-5 handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerId {
    owner: usize,
    index: usize,
    source: bool,
}

#[derive(Debug)]
pub struct Layer {
    original: String,
    name: String,
    color: i16,
    linetype: DxfLinetype,
    /// None keeps the unresolved default in the owned model. R2000 writes the
    /// layer sentinel -3, never a silently chosen physical lineweight.
    weight: Option<StandardWeight>,
}

impl Layer {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn color(&self) -> i16 {
        self.color
    }
    pub fn linetype(&self) -> DxfLinetype {
        self.linetype
    }
    pub fn weight(&self) -> Option<StandardWeight> {
        self.weight
    }
}

/// The original text-record kind, independent of its decoded geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DxfTextRecordKind {
    Text,
    MText,
}

/// Source fields needed by the existing sketch-import projection. A typed TEXT
/// may use its second alignment anchor and decoded contents, while the original
/// import contract uses its first anchor and trimmed raw value (TEXT's group 1;
/// MTEXT's group 3 chunks and group 1 tail, joined). These fields retain actual
/// DXF data; no alternate Geom2 or whole parse is cached.
#[derive(Debug, PartialEq)]
pub struct DxfTextSource {
    kind: DxfTextRecordKind,
    first_anchor_mm: Option<Vec2>,
    raw_group1: Option<String>,
}

impl DxfTextSource {
    pub fn kind(&self) -> DxfTextRecordKind {
        self.kind
    }
    pub fn first_anchor_mm(&self) -> Option<Vec2> {
        self.first_anchor_mm
    }
    pub fn raw_group1(&self) -> Option<&str> {
        self.raw_group1.as_deref()
    }

    pub(crate) fn parsed_owned(kind: DxfTextRecordKind, first_anchor_mm: Option<Vec2>, raw_group1: Option<String>) -> Result<Self> {
        if first_anchor_mm.is_some_and(|p| !p.is_finite()) || raw_group1.as_ref().is_some_and(|s| s.len() > crate::MAX_DRAWING_BYTES) {
            return Err(invalid("invalid or oversized DXF text-source fields"));
        }
        Ok(Self { kind, first_anchor_mm, raw_group1 })
    }

    pub(super) fn transformed(&self, translation: Vec2, scale: f64) -> Result<Self> {
        let first_anchor_mm = self.first_anchor_mm.map(|p| solidcraft_kernel::services::import_geometry::transform_anchor(p, scale, translation));
        let raw_group1 = self
            .raw_group1
            .as_deref()
            .map(|text| -> Result<String> {
                let mut copied = String::new();
                copied.try_reserve_exact(text.len()).map_err(|_| invalid("DXF text-source allocation failed"))?;
                copied.push_str(text);
                Ok(copied)
            })
            .transpose()?;
        Self::parsed_owned(self.kind, first_anchor_mm, raw_group1)
    }
}

#[derive(Debug)]
pub struct Entity<G> {
    layer: LayerId,
    weight: Option<EntityWeight>,
    geometry: G,
    source_text: Option<DxfTextSource>,
}

impl<G> Entity<G> {
    pub fn layer(&self) -> LayerId {
        self.layer
    }
    pub fn weight(&self) -> Option<EntityWeight> {
        self.weight
    }
    pub fn geometry(&self) -> &G {
        &self.geometry
    }
    pub fn source_text(&self) -> Option<&DxfTextSource> {
        self.source_text.as_ref()
    }
}

/// A source BLOCK definition owned by the same DxfFile as its references.
/// Entity order and the original base point are retained; expansion is a
/// consumer projection, never precomputed alternate geometry.
#[derive(Debug)]
pub struct DxfSourceBlock<G> {
    base: Vec2,
    entities: Vec<Entity<G>>,
}

impl<G> DxfSourceBlock<G> {
    pub fn base(&self) -> Vec2 {
        self.base
    }
    pub fn entities(&self) -> &[Entity<G>] {
        &self.entities
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdmissionProfile {
    Owned,
    SketchImport,
}

/// No Clone: cloning must not accidentally duplicate ownership. A future copy
/// operation must mint a new owner and remap its handles explicitly.
#[derive(Debug)]
pub struct DxfFile<G> {
    owner: usize,
    profile: AdmissionProfile,
    source_work: usize,
    source_layers: Vec<String>,
    source_blocks: HashMap<String, DxfSourceBlock<G>>,
    layers: Vec<Layer>,
    entities: Vec<Entity<G>>,
    max_layers: usize,
    max_entities: usize,
    pub(super) compatibility: Option<super::export_compatibility::Budget>,
}

fn owner_from(counter: &AtomicUsize) -> Result<usize> {
    counter.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1)).map_err(|_| invalid("DXF file ownership identities exhausted"))
}

fn owned(text: &str) -> Result<String> {
    let mut out = String::new();
    out.try_reserve(text.len()).map_err(|_| invalid("DXF name allocation failed"))?;
    out.push_str(text);
    Ok(out)
}

fn ascii_name_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

fn name_of(input: &str) -> Result<String> {
    if input.len() > MAX_RAW_NAME_BYTES {
        return Err(invalid("DXF raw layer-name input exceeds resource byte limit"));
    }
    let input = input.trim_matches(ascii_name_space);
    // Inspect at most one scalar beyond the bound before any allocation.
    // At most255 Unicode scalars imply at most1020 UTF-8 bytes. The length
    // rule alone does not establish Unicode symbol-equivalence semantics.
    if input.chars().take(MAX_NAME_CHARS + 1).count() > MAX_NAME_CHARS {
        return Err(invalid("DXF layer name exceeds 255 Unicode scalar values"));
    }
    if input.is_empty() {
        return owned("0");
    }
    let mut name = String::new();
    name.try_reserve(input.len()).map_err(|_| invalid("DXF name allocation failed"))?;
    for c in input.chars() {
        let forbidden = c.is_control() || matches!(c, '<' | '>' | '/' | '\\' | '"' | '\'' | ':' | ';' | '?' | '*' | '|' | '=');
        name.push(if forbidden { '_' } else { c });
    }
    Ok(name)
}

impl<G> DxfFile<G> {
    pub fn new() -> Result<Self> {
        Self::with_limits(MAX_LAYERS, MAX_ENTITIES)
    }

    /// Construct a new file with the imported style of its first layer, `0`.
    /// This is initialization, not a setter: the style is immutable after construction.
    /// Colors use the same indexed range as [`Self::declare_layer`]; a missing
    /// physical weight remains absent rather than becoming an explicit zero.
    /// Every successful construction mints fresh file-scoped ownership handles.
    pub fn with_layer_zero(color: i16, linetype: DxfLinetype, weight: Option<StandardWeight>) -> Result<Self> {
        Self::with_layer_zero_limits(color, linetype, weight, MAX_LAYERS, MAX_ENTITIES)
    }

    fn with_limits(max_layers: usize, max_entities: usize) -> Result<Self> {
        Self::with_layer_zero_limits(7, DxfLinetype::Continuous, None, max_layers, max_entities)
    }

    fn with_layer_zero_limits(
        color: i16,
        linetype: DxfLinetype,
        weight: Option<StandardWeight>,
        max_layers: usize,
        max_entities: usize,
    ) -> Result<Self> {
        if !(1..=255).contains(&color) {
            return Err(invalid("DXF layer color must be an index from 1 to 255"));
        }
        if max_layers == 0 {
            return Err(invalid("DXF file requires layer 0"));
        }
        let owner = owner_from(&NEXT_OWNER)?;
        let mut layers = Vec::new();
        layers.try_reserve(1).map_err(|_| invalid("DXF layer allocation failed"))?;
        layers.push(Layer { original: owned("0")?, name: owned("0")?, color, linetype, weight });
        Ok(Self {
            owner,
            profile: AdmissionProfile::Owned,
            source_work: 0,
            source_layers: Vec::new(),
            source_blocks: HashMap::new(),
            layers,
            entities: Vec::new(),
            max_layers: max_layers.min(MAX_LAYERS),
            max_entities: max_entities.min(MAX_ENTITIES),
            compatibility: None,
        })
    }

    pub(super) fn with_export_budget(budget: super::export_compatibility::Budget) -> Result<Self> {
        let mut file = Self::new()?;
        file.max_layers = budget.layers;
        file.max_entities = budget.entities;
        file.compatibility = Some(budget);
        Ok(file)
    }

    /// Named compatibility admission for the existing sketch-import domain.
    /// It carries source names without inventing declared layer styles. The
    /// shared parser already enforces 64MiB input and 200,000 records/section;
    /// projection cumulatively enforces 800,000 visits/cells/payload copies and
    /// 200,000 outputs. Public builders cannot mutate the admitted source.
    pub(crate) fn for_sketch_import() -> Result<Self> {
        Ok(Self {
            owner: owner_from(&NEXT_OWNER)?,
            profile: AdmissionProfile::SketchImport,
            source_work: 0,
            source_layers: Vec::new(),
            source_blocks: HashMap::new(),
            layers: Vec::new(),
            entities: Vec::new(),
            max_layers: crate::sketch2d::MAX_ENTITIES.checked_mul(2).ok_or_else(|| invalid("DXF source layer count overflow"))?,
            max_entities: crate::sketch2d::MAX_ENTITIES,
            compatibility: None,
        })
    }

    pub(crate) fn source_work(&self) -> usize {
        self.source_work
    }
    pub(crate) fn set_source_work(&mut self, work: usize) {
        self.source_work = work;
    }

    pub fn is_sketch_import(&self) -> bool {
        self.profile == AdmissionProfile::SketchImport
    }

    pub fn source_layer_name(&self, id: LayerId) -> Result<&str> {
        if id.owner != self.owner {
            return Err(invalid("layer belongs to a different DXF file"));
        }
        if id.source {
            self.source_layers.get(id.index).map(String::as_str).ok_or_else(|| invalid("unknown DXF source layer handle"))
        } else {
            self.layer(id).map(Layer::name)
        }
    }

    pub fn source_block(&self, name: &str) -> Option<&DxfSourceBlock<G>> {
        self.source_blocks.get(name)
    }
    pub fn source_blocks(&self) -> impl Iterator<Item = (&str, &DxfSourceBlock<G>)> {
        self.source_blocks.iter().map(|(name, block)| (name.as_str(), block))
    }

    pub(crate) fn import_layer_reference(&mut self, name: &str) -> Result<LayerId> {
        if !self.is_sketch_import() {
            return Err(invalid("source references require SketchImport admission"));
        }
        if name.len() > crate::MAX_DRAWING_BYTES || self.source_layers.len() >= self.max_layers {
            return Err(invalid("DXF source layer reference budget exceeded"));
        }
        // Each source record owns one actual reference. No quadratic name
        // deduplication and no guessed Unicode equivalence or missing style.
        let name = owned(name)?;
        self.source_layers.try_reserve(1).map_err(|_| invalid("DXF source layer allocation failed"))?;
        let index = self.source_layers.len();
        self.source_layers.push(name);
        Ok(LayerId { owner: self.owner, index, source: true })
    }

    pub(crate) fn import_record(&self, layer: LayerId, geometry: G, source_text: Option<DxfTextSource>) -> Result<Entity<G>> {
        if !self.is_sketch_import() || !layer.source {
            return Err(invalid("source record requires its SketchImport layer"));
        }
        self.source_layer_name(layer)?;
        Ok(Entity { layer, weight: None, geometry, source_text })
    }

    pub(crate) fn push_import_record(&mut self, entity: Entity<G>) -> Result<()> {
        if !self.is_sketch_import() || self.entities.len() >= self.max_entities {
            return Err(invalid("DXF source entity count limit exceeded"));
        }
        self.source_layer_name(entity.layer)?;
        self.entities.try_reserve(1).map_err(|_| invalid("DXF source entity allocation failed"))?;
        self.entities.push(entity);
        Ok(())
    }

    pub(crate) fn insert_source_block(&mut self, name: String, base: Vec2, entities: Vec<Entity<G>>) -> Result<()> {
        if !self.is_sketch_import() || !base.is_finite() || entities.len() > crate::sketch2d::MAX_ENTITIES {
            return Err(invalid("invalid DXF source block"));
        }
        if name.len() > crate::MAX_DRAWING_BYTES {
            return Err(invalid("DXF source block name exceeds input budget"));
        }
        for entity in &entities {
            self.source_layer_name(entity.layer)?;
        }
        self.source_blocks.try_reserve(1).map_err(|_| invalid("DXF source block allocation failed"))?;
        // The existing import contract resolves the last definition of a name.
        self.source_blocks.insert(name, DxfSourceBlock { base, entities });
        Ok(())
    }

    /// The owned-file layer-zero handle. SketchImport has unresolved source
    /// references instead of a declared zero style; use each entity reference
    /// with source_layer_name there. Normal layer/style lookup refuses it.
    pub fn default_layer(&self) -> LayerId {
        LayerId { owner: self.owner, index: 0, source: false }
    }
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }
    pub fn entities(&self) -> &[Entity<G>] {
        &self.entities
    }

    pub fn layer(&self, id: LayerId) -> Result<&Layer> {
        if id.owner != self.owner {
            return Err(invalid("layer belongs to a different DXF file"));
        }
        if id.source {
            return Err(invalid("DXF source layer has no resolved declared style"));
        }
        self.layers.get(id.index).ok_or_else(|| invalid("unknown DXF layer handle"))
    }

    /// Where a declared layer sits in [`Self::layers`].
    pub(super) fn layer_index(&self, id: LayerId) -> Result<usize> {
        self.layer(id).map(|_| id.index)
    }

    /// The frozen legacy fixtures keep exact names and first declaration wins.
    /// This private seam cannot initialize or relax an ordinary/producer file.
    #[cfg(any(test, feature = "dxf-legacy-baseline-tests"))]
    pub(super) fn declare_legacy_baseline_layer(&mut self, name: &str, color: i16, linetype: DxfLinetype) -> Result<LayerId> {
        let budget = self
            .compatibility
            .filter(|budget| budget.is_legacy_baseline())
            .ok_or_else(|| invalid("exact fixture layers require private legacy baseline admission"))?;
        budget.layer_name(name, color, linetype, false)?;
        if let Some(index) = self.layers.iter().position(|layer| layer.name == name) {
            return Ok(LayerId { owner: self.owner, index, source: false });
        }
        if self.layers.len() >= self.max_layers {
            return Err(invalid("DXF layer count limit exceeded"));
        }
        let original = owned(name)?;
        let name = owned(name)?;
        self.layers.try_reserve(1).map_err(|_| invalid("DXF layer allocation failed"))?;
        let index = self.layers.len();
        self.layers.push(Layer { original, name, color, linetype, weight: None });
        Ok(LayerId { owner: self.owner, index, source: false })
    }

    pub fn declare_layer(&mut self, original: &str, color: i16, linetype: DxfLinetype, weight: Option<StandardWeight>) -> Result<LayerId> {
        if self.is_sketch_import() {
            return Err(invalid("SketchImport source metadata is immutable; convert explicitly to an owned file"));
        }
        if original.len() > MAX_RAW_NAME_BYTES {
            return Err(invalid("DXF raw layer-name input exceeds resource byte limit"));
        }
        // Provisional ASCII-only container: do not claim general Unicode
        // collision safety from eq_ignore_ascii_case. The character-bound
        // helper is Unicode-aware, but a reader adapter needs an agreed
        // canonical symbol comparison before it may admit Unicode layers.
        if !original.is_ascii() {
            return Err(invalid("not supported yet: non-ASCII DXF layer names"));
        }
        let name = if let Some(budget) = self.compatibility {
            budget.layer_name(original, color, linetype, weight.is_some())?;
            owned(original)?
        } else {
            name_of(original)?
        };
        if !(1..=255).contains(&color) {
            return Err(invalid("DXF layer color must be an index from 1 to 255"));
        }
        if let Some((index, existing)) = self.layers.iter().enumerate().find(|(_, layer)| layer.name.eq_ignore_ascii_case(&name)) {
            // Blank and whitespace-surrounded0 explicitly alias built-in0.
            // Other distinct raw declarations that normalize to one symbol
            // refuse rather than adding a second or silently merging styles.
            let same_input = original == existing.original || name == "0";
            if same_input && existing.color == color && existing.linetype == linetype && existing.weight == weight {
                return Ok(LayerId { owner: self.owner, index, source: false });
            }
            return Err(invalid("DXF layer declaration collides with an existing symbol"));
        }
        if self.layers.len() >= self.max_layers {
            return Err(invalid("DXF layer count limit exceeded"));
        }
        let original = owned(original)?;
        self.layers.try_reserve(1).map_err(|_| invalid("DXF layer allocation failed"))?;
        let index = self.layers.len();
        self.layers.push(Layer { original, name, color, linetype, weight });
        Ok(LayerId { owner: self.owner, index, source: false })
    }

    /// G is carried unchanged. A future adapter validates geometric values.
    pub fn push(&mut self, layer: LayerId, geometry: G, weight: Option<EntityWeight>) -> Result<usize> {
        self.push_with_source(layer, geometry, weight, None)
    }

    pub(crate) fn push_with_source(
        &mut self,
        layer: LayerId,
        geometry: G,
        weight: Option<EntityWeight>,
        source_text: Option<DxfTextSource>,
    ) -> Result<usize> {
        if self.is_sketch_import() {
            return Err(invalid("SketchImport source records is immutable; convert explicitly to an owned file"));
        }
        self.layer(layer)?;
        if self.entities.len() >= self.max_entities {
            return Err(invalid("DXF entity count limit exceeded"));
        }
        self.entities.try_reserve(1).map_err(|_| invalid("DXF entity allocation failed"))?;
        let index = self.entities.len();
        self.entities.push(Entity { layer, weight, geometry, source_text });
        Ok(index)
    }

    /// ByBlock and Default deliberately remain symbolic, rather than inventing
    /// a parent block or the consuming application's default numeric thickness.
    pub fn effective_weight(&self, index: usize) -> Result<EntityWeight> {
        let entity = self.entities.get(index).ok_or_else(|| invalid("unknown DXF entity index"))?;
        match entity.weight {
            None | Some(EntityWeight::ByLayer) => Ok(self.layer(entity.layer)?.weight.map_or(EntityWeight::Default, EntityWeight::Explicit)),
            Some(weight) => Ok(weight),
        }
    }

    pub fn effective_linetype(&self, index: usize) -> Result<DxfLinetype> {
        let entity = self.entities.get(index).ok_or_else(|| invalid("unknown DXF entity index"))?;
        Ok(self.layer(entity.layer)?.linetype)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_own_ordered_geometry_without_inventing_a_weight() {
        let mut file = DxfFile::new().unwrap();
        let cut = file.declare_layer("CUT", 1, DxfLinetype::Continuous, None).unwrap();
        let hidden = file.declare_layer("HIDDEN", 8, DxfLinetype::Hidden, None).unwrap();
        file.push(cut, ((0, 0), (10, 5)), None).unwrap();
        file.push(hidden, ((3, 4), (5, 4)), None).unwrap();
        assert_eq!(file.layers().iter().map(Layer::name).collect::<Vec<_>>(), ["0", "CUT", "HIDDEN"]);
        assert_eq!(file.entities()[0].geometry(), &((0, 0), (10, 5)));
        assert_eq!(file.entities()[0].layer(), cut);
        assert_eq!(file.entities()[0].weight(), None);
        assert_eq!(file.effective_linetype(1).unwrap(), DxfLinetype::Hidden);
        assert_eq!(file.effective_weight(0).unwrap(), EntityWeight::Default);
    }

    #[test]
    fn handles_from_another_file_never_select_its_same_numbered_layer() {
        let first = DxfFile::<()>::new().unwrap();
        let mut second = DxfFile::new().unwrap();
        assert!(second.push(first.default_layer(), (), None).is_err());
        assert!(second.entities().is_empty());
        second.push(second.default_layer(), (), None).unwrap();
    }

    #[test]
    fn names_are_cleaned_once_and_collisions_refuse_without_mutation() {
        let mut file = DxfFile::<()>::new().unwrap();
        let id = file.declare_layer("CUT\nA", 3, DxfLinetype::Continuous, None).unwrap();
        assert_eq!(file.layer(id).unwrap().name(), "CUT_A");
        assert_eq!(file.declare_layer("CUT\nA", 3, DxfLinetype::Continuous, None).unwrap(), id);
        for name in ["CUT?A", "cut_a", "CUT_A"] {
            assert!(file.declare_layer(name, 3, DxfLinetype::Continuous, None).is_err());
        }
        assert_eq!(file.layers().len(), 2);
        assert_eq!(file.declare_layer("", 7, DxfLinetype::Continuous, None).unwrap(), file.default_layer());
        assert!(file.declare_layer(&"x".repeat(256), 7, DxfLinetype::Continuous, None).is_err());
        assert!(file.declare_layer("bad color", 256, DxfLinetype::Continuous, None).is_err());
        assert_eq!(file.layers().len(), 2);
    }

    #[test]
    fn a_layer_cannot_be_redeclared_with_different_style() {
        let mut file = DxfFile::<()>::new().unwrap();
        file.declare_layer("ETCH", 4, DxfLinetype::Dashed, None).unwrap();
        assert!(file.declare_layer("ETCH", 4, DxfLinetype::Continuous, None).is_err());
        assert_eq!(file.layers().len(), 2);
        assert_eq!(file.layers()[1].color(), 4);
    }

    #[test]
    fn name_length_counts_scalars_with_a_bounded_utf8_allocation() {
        assert_eq!(name_of(&"漢".repeat(86)).unwrap().chars().count(), 86);
        assert_eq!(name_of(&"漢".repeat(255)).unwrap().chars().count(), 255);
        assert_eq!(name_of(&"😀".repeat(255)).unwrap().len(), 1020);
        assert!(name_of(&"漢".repeat(256)).is_err());
        let mut file = DxfFile::<()>::new().unwrap();
        file.declare_layer(&"A".repeat(255), 7, DxfLinetype::Continuous, None).unwrap();
        assert!(file.declare_layer(&"A".repeat(256), 7, DxfLinetype::Continuous, None).is_err());
        assert_eq!(file.layers().len(), 2);
    }

    #[test]
    fn unicode_symbol_equivalence_is_refused_instead_of_assumed() {
        let mut file = DxfFile::<()>::new().unwrap();
        for name in ["É", "é", "漢", "Aé", "E\u{301}"] {
            let error = file.declare_layer(name, 7, DxfLinetype::Continuous, None).unwrap_err();
            assert!(error.to_string().contains("not supported yet: non-ASCII"));
        }
        assert_eq!(file.layers().len(), 1);
        assert!(file.entities().is_empty());
    }

    #[test]
    fn importing_a_different_layer_zero_style_remains_explicitly_pending() {
        let mut file = DxfFile::<()>::new().unwrap();
        assert!(file.declare_layer("0", 3, DxfLinetype::Dashed, None).is_err());
        let zero = file.layer(file.default_layer()).unwrap();
        assert_eq!(zero.color(), 7);
        assert_eq!(zero.linetype(), DxfLinetype::Continuous);
        assert_eq!(zero.weight(), None);
    }

    #[test]
    fn trimming_precedes_ascii_symbol_collision_detection() {
        let mut file = DxfFile::<()>::new().unwrap();
        file.declare_layer("CUT", 7, DxfLinetype::Continuous, None).unwrap();
        for (name, expected) in [(" CUT ", "CUT"), ("\tCUT\r\n", "CUT"), ("\x0bCUT\x0c", "CUT"), (" cut ", "cut")] {
            assert_eq!(name_of(name).unwrap(), expected);
            assert!(file.declare_layer(name, 7, DxfLinetype::Continuous, None).is_err());
        }
        assert_eq!(file.layers().len(), 2);
    }

    #[test]
    fn blanks_and_trimmed_layer_zero_use_the_existing_zero_handle() {
        let mut file = DxfFile::<()>::new().unwrap();
        for name in ["", " \t\r\n\x0b\x0c ", " 0 ", "\t0\r\n"] {
            assert_eq!(file.declare_layer(name, 7, DxfLinetype::Continuous, None).unwrap(), file.default_layer());
        }
        assert_eq!(file.layers().len(), 1);
        assert!(file.declare_layer(" 0 ", 3, DxfLinetype::Continuous, None).is_err());
    }

    #[test]
    fn normalized_255_character_limit_is_independent_of_raw_padding() {
        let mut file = DxfFile::<()>::new().unwrap();
        let name = "A".repeat(255);
        let padded = format!(" \t{name}\r\n ");
        let id = file.declare_layer(&padded, 7, DxfLinetype::Continuous, None).unwrap();
        assert_eq!(file.layer(id).unwrap().name(), name);
        assert!(name_of(&format!(" {} ", "A".repeat(256))).is_err());
        assert_eq!(name_of(&format!(" {} ", "漢".repeat(255))).unwrap().chars().count(), 255);
        assert_eq!(file.layers().len(), 2);
    }

    #[test]
    fn raw_resource_limit_is_separate_from_normalized_format_length() {
        let boundary = format!("{}{}", " ".repeat(3841), "A".repeat(255));
        assert_eq!(boundary.len(), 4096);
        assert_eq!(name_of(&boundary).unwrap().len(), 255);
        assert!(name_of(&format!(" {boundary}")).unwrap_err().to_string().contains("resource byte limit"));
        assert!(name_of(&" ".repeat(4097)).is_err());
    }

    #[test]
    fn physical_weights_keep_standard_values_and_nearest_boundaries() {
        let table = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];
        for code in table {
            let w = StandardWeight::from_code(code).unwrap();
            assert_eq!(w.code(), code);
            assert_eq!(StandardWeight::from_mm(f64::from(code) / 100.0).unwrap(), w);
        }
        // 0.375 is the EXACT binary midpoint between 0.35 and 0.40.
        assert_eq!(StandardWeight::from_mm(0.375_f64.next_down()).unwrap().code(), 35);
        assert_eq!(StandardWeight::from_mm(0.375).unwrap().code(), 40);
        assert_eq!(StandardWeight::from_mm(0.375_f64.next_up()).unwrap().code(), 40);
        // Stored 0.025 is slightly ABOVE the rational first midpoint.
        assert_eq!(StandardWeight::from_mm(0.025_f64.next_down()).unwrap().code(), 0);
        assert_eq!(StandardWeight::from_mm(0.025).unwrap().code(), 5);
        assert_eq!(StandardWeight::from_mm(0.01).unwrap().code(), 0);
    }

    #[test]
    fn invalid_physical_weights_are_refused_not_clamped_or_symbols() {
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.11_f64.next_up()] {
            assert!(StandardWeight::from_mm(value).is_err());
        }
        assert!(StandardWeight::from_code(6).is_err());
        assert_eq!(StandardWeight::from_mm(-0.0).unwrap().code(), 0);
        assert_eq!(StandardWeight::from_mm(2.11).unwrap().code(), 211);
    }

    #[test]
    fn omitted_and_symbolic_entity_weights_remain_distinct() {
        let mut file = DxfFile::new().unwrap();
        let fifty = StandardWeight::from_code(50).unwrap();
        let cut = file.declare_layer("CUT", 1, DxfLinetype::Continuous, Some(fifty)).unwrap();
        for weight in [
            None,
            Some(EntityWeight::ByLayer),
            Some(EntityWeight::ByBlock),
            Some(EntityWeight::Default),
            Some(EntityWeight::Explicit(StandardWeight::from_code(0).unwrap())),
        ] {
            file.push(cut, (), weight).unwrap();
        }
        assert_eq!(file.entities()[0].weight(), None);
        assert_eq!(file.entities()[1].weight(), Some(EntityWeight::ByLayer));
        assert_eq!(file.effective_weight(0).unwrap(), EntityWeight::Explicit(fifty));
        assert_eq!(file.effective_weight(1).unwrap(), EntityWeight::Explicit(fifty));
        assert_eq!(file.effective_weight(2).unwrap(), EntityWeight::ByBlock);
        assert_eq!(file.effective_weight(3).unwrap(), EntityWeight::Default);
        assert_eq!(file.effective_weight(4).unwrap(), EntityWeight::Explicit(StandardWeight::from_code(0).unwrap()));
        assert_eq!(EntityWeight::ByLayer.group370(), -1);
        assert_eq!(EntityWeight::ByBlock.group370(), -2);
        assert_eq!(EntityWeight::Default.group370(), -3);
        assert_eq!(EntityWeight::Explicit(fifty).group370(), 50);
        assert!(file.effective_weight(5).is_err());
    }

    #[test]
    fn cumulative_limits_refuse_the_next_insert_without_a_partial_entity() {
        let mut file = DxfFile::with_limits(2, 1).unwrap();
        file.declare_layer("A", 7, DxfLinetype::Continuous, None).unwrap();
        assert!(file.declare_layer("B", 7, DxfLinetype::Continuous, None).is_err());
        file.push(file.default_layer(), (), None).unwrap();
        assert!(file.push(file.default_layer(), (), None).is_err());
        assert_eq!(file.layers().len(), 2);
        assert_eq!(file.entities().len(), 1);
    }

    #[test]
    fn ownership_counter_never_wraps_or_reuses_an_identity() {
        let counter = AtomicUsize::new(usize::MAX - 1);
        assert_eq!(owner_from(&counter).unwrap(), usize::MAX - 1);
        assert!(owner_from(&counter).is_err());
        assert!(owner_from(&counter).is_err());
    }

    // Round trips exercise the real writer and layered reader, not group-code test parsers.
    use crate::DxfShape;
    use solidcore_record::Vec2;

    fn read_written_file(file: &DxfFile<DxfShape>) -> DxfFile<DxfShape> {
        crate::read_dxf_file(&crate::write_dxf_file(file).unwrap()).unwrap()
    }

    #[test]
    fn writer_reader_keep_mixed_layer_declarations_entity_order_and_file_scoped_handles() {
        let mut source = DxfFile::new().unwrap();
        let cut = source.declare_layer("CUT", 1, DxfLinetype::Continuous, Some(StandardWeight::from_code(50).unwrap())).unwrap();
        let hidden = source.declare_layer("HIDDEN", 8, DxfLinetype::Hidden, Some(StandardWeight::from_code(0).unwrap())).unwrap();
        source.declare_layer("UNUSED", 3, DxfLinetype::Dashed, None).unwrap();
        source.push(cut, DxfShape::Line(Vec2::new(-2.0, 1.5), Vec2::new(3.0, 1.5)), None).unwrap();
        source.push(source.default_layer(), DxfShape::Circle { c: Vec2::new(4.0, -1.0), r: 0.5 }, Some(EntityWeight::ByBlock)).unwrap();
        source.push(hidden, DxfShape::Arc { c: Vec2::new(-3.0, 2.0), r: 2.0, a0: 350.0, a1: 10.0 }, Some(EntityWeight::ByLayer)).unwrap();
        source.push(cut, DxfShape::Line(Vec2::new(7.0, 0.0), Vec2::new(7.0, 4.0)), Some(EntityWeight::Default)).unwrap();

        let recovered = read_written_file(&source);
        let layers: Vec<_> = recovered
            .layers()
            .iter()
            .map(|layer| (layer.name(), layer.color(), layer.linetype(), layer.weight().map(StandardWeight::code)))
            .collect();
        assert_eq!(
            layers,
            [
                ("0", 7, DxfLinetype::Continuous, None),
                ("CUT", 1, DxfLinetype::Continuous, Some(50)),
                ("HIDDEN", 8, DxfLinetype::Hidden, Some(0)),
                ("UNUSED", 3, DxfLinetype::Dashed, None),
            ]
        );
        let names: Vec<_> = recovered.entities().iter().map(|entity| recovered.layer(entity.layer()).unwrap().name()).collect();
        assert_eq!(names, ["CUT", "0", "HIDDEN", "CUT"]);
        assert_eq!(recovered.entities().len(), 4);
        assert_eq!(recovered.entities()[0].geometry(), &DxfShape::Line(Vec2::new(-2.0, 1.5), Vec2::new(3.0, 1.5)));
        assert_eq!(recovered.entities()[1].geometry(), &DxfShape::Circle { c: Vec2::new(4.0, -1.0), r: 0.5 });
        assert_eq!(recovered.entities()[2].geometry(), &DxfShape::Arc { c: Vec2::new(-3.0, 2.0), r: 2.0, a0: 350.0, a1: 10.0 });
        assert_eq!(recovered.entities()[3].geometry(), &DxfShape::Line(Vec2::new(7.0, 0.0), Vec2::new(7.0, 4.0)));
        assert_eq!(
            recovered.entities().iter().map(Entity::weight).collect::<Vec<_>>(),
            [None, Some(EntityWeight::ByBlock), Some(EntityWeight::ByLayer), Some(EntityWeight::Default)]
        );
        // Imported handles belong to the new file; compare resolved metadata, not owner IDs.
        assert!(recovered.layer(cut).is_err());
        assert!(recovered.layer(source.default_layer()).is_err());
        assert!(source.layer(recovered.default_layer()).is_err());
    }

    #[test]
    fn writer_reader_keep_omission_symbols_and_all_physical_weights_in_each_linetype() {
        let mut weights = vec![None, Some(EntityWeight::ByLayer), Some(EntityWeight::ByBlock), Some(EntityWeight::Default)];
        // Independent explicit public code list: not the model's internal lookup table.
        for code in [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211] {
            weights.push(Some(EntityWeight::Explicit(StandardWeight::from_code(code).unwrap())));
        }
        let styles = [
            ("MEASURED", 1, DxfLinetype::Continuous, Some(50)),
            ("OMITTED", 3, DxfLinetype::Dashed, None),
            ("ZERO_WEIGHT", 8, DxfLinetype::Hidden, Some(0)),
        ];
        let mut source = DxfFile::new().unwrap();
        for (name, color, linetype, weight) in styles {
            let layer = source.declare_layer(name, color, linetype, weight.map(|code| StandardWeight::from_code(code).unwrap())).unwrap();
            for weight in &weights {
                source.push(layer, DxfShape::Line(Vec2::new(0.5, 0.75), Vec2::new(1.25, -0.5)), *weight).unwrap();
            }
        }
        let recovered = read_written_file(&source);
        assert_eq!(recovered.layers().len(), 4);
        assert_eq!(recovered.entities().len(), 84);
        for (style_index, (name, color, linetype, layer_weight)) in styles.iter().enumerate() {
            for (weight_index, expected_weight) in weights.iter().enumerate() {
                let index = style_index * 28 + weight_index;
                let entity = &recovered.entities()[index];
                let layer = recovered.layer(entity.layer()).unwrap();
                assert_eq!(
                    (layer.name(), layer.color(), layer.linetype(), layer.weight().map(StandardWeight::code)),
                    (*name, *color, *linetype, *layer_weight)
                );
                assert_eq!(entity.weight(), *expected_weight, "style={name} weight ordinal={weight_index}");
                assert_eq!(entity.geometry(), &DxfShape::Line(Vec2::new(0.5, 0.75), Vec2::new(1.25, -0.5)));
            }
        }
    }

    #[test]
    fn writer_reader_keep_exact_supported_curves_on_all_three_layer_styles() {
        for (name, linetype) in [("SOLID", DxfLinetype::Continuous), ("DASH", DxfLinetype::Dashed), ("HIDE", DxfLinetype::Hidden)] {
            let mut source = DxfFile::new().unwrap();
            let layer = source.declare_layer(name, 5, linetype, None).unwrap();
            for shape in [
                DxfShape::Line(Vec2::new(-1.25, 2.5), Vec2::new(3.75, -4.0)),
                DxfShape::Circle { c: Vec2::new(8.0, -2.0), r: 1.25 },
                DxfShape::Arc { c: Vec2::new(2.0, 3.0), r: 0.75, a0: 45.0, a1: 135.0 },
                // Open straight chain, with an intentional repeated vertex retained.
                DxfShape::Polyline { points: vec![Vec2::new(-2.0, 0.0), Vec2::new(0.0, 1.5), Vec2::new(0.0, 1.5), Vec2::new(4.0, -0.5)] },
            ] {
                source.push(layer, shape, Some(EntityWeight::ByLayer)).unwrap();
            }
            let recovered = read_written_file(&source);
            assert_eq!(recovered.entities().len(), 4);
            assert_eq!(recovered.entities()[0].geometry(), &DxfShape::Line(Vec2::new(-1.25, 2.5), Vec2::new(3.75, -4.0)));
            assert_eq!(recovered.entities()[1].geometry(), &DxfShape::Circle { c: Vec2::new(8.0, -2.0), r: 1.25 });
            assert_eq!(recovered.entities()[2].geometry(), &DxfShape::Arc { c: Vec2::new(2.0, 3.0), r: 0.75, a0: 45.0, a1: 135.0 });
            assert_eq!(
                recovered.entities()[3].geometry(),
                &DxfShape::Polyline { points: vec![Vec2::new(-2.0, 0.0), Vec2::new(0.0, 1.5), Vec2::new(0.0, 1.5), Vec2::new(4.0, -0.5)] }
            );
            for entity in recovered.entities() {
                let layer = recovered.layer(entity.layer()).unwrap();
                assert_eq!((layer.name(), layer.linetype()), (name, linetype));
                assert_eq!(entity.weight(), Some(EntityWeight::ByLayer));
            }
        }
    }

    #[test]
    fn writer_reader_keep_degree_nine_nonunit_knots_controls_and_optional_rational_weights() {
        for linetype in [DxfLinetype::Continuous, DxfLinetype::Dashed, DxfLinetype::Hidden] {
            let mut source = DxfFile::new().unwrap();
            let layer = source.declare_layer("CURVE", 2, linetype, Some(StandardWeight::from_code(35).unwrap())).unwrap();
            for weights in [None, Some(vec![1.0; 10]), Some(vec![0.5, 1.0, 2.0, 4.0, 0.25, 0.5, 1.0, 2.0, 4.0, 1.0])] {
                source
                    .push(
                        layer,
                        DxfShape::Spline {
                            degree: 9,
                            knots: vec![-2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0],
                            ctrl: vec![
                                Vec2::new(0.0, 0.0),
                                Vec2::new(1.0, 2.0),
                                Vec2::new(2.0, 4.0),
                                Vec2::new(3.0, 8.0),
                                Vec2::new(4.0, 4.0),
                                Vec2::new(5.0, 2.0),
                                Vec2::new(6.0, 1.0),
                                Vec2::new(7.0, 0.0),
                                Vec2::new(8.0, -1.0),
                                Vec2::new(9.0, -2.0),
                            ],
                            weights,
                        },
                        Some(EntityWeight::Explicit(StandardWeight::from_code(0).unwrap())),
                    )
                    .unwrap();
            }
            let recovered = read_written_file(&source);
            assert_eq!(recovered.entities().len(), 3);
            for (index, entity) in recovered.entities().iter().enumerate() {
                let DxfShape::Spline { degree, knots, ctrl, weights } = entity.geometry() else { panic!("expected degree-nine spline") };
                assert_eq!(*degree, 9);
                assert_eq!(
                    ctrl,
                    &[
                        Vec2::new(0.0, 0.0),
                        Vec2::new(1.0, 2.0),
                        Vec2::new(2.0, 4.0),
                        Vec2::new(3.0, 8.0),
                        Vec2::new(4.0, 4.0),
                        Vec2::new(5.0, 2.0),
                        Vec2::new(6.0, 1.0),
                        Vec2::new(7.0, 0.0),
                        Vec2::new(8.0, -1.0),
                        Vec2::new(9.0, -2.0)
                    ]
                );
                assert_eq!(knots, &[-2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0]);
                match index {
                    0 => assert_eq!(weights, &None),
                    1 => assert_eq!(weights.as_deref(), Some([1.0; 10].as_slice())),
                    2 => assert_eq!(weights.as_deref(), Some([0.5, 1.0, 2.0, 4.0, 0.25, 0.5, 1.0, 2.0, 4.0, 1.0].as_slice())),
                    _ => panic!("unexpected extra spline"),
                }
                let layer = recovered.layer(entity.layer()).unwrap();
                assert_eq!((layer.name(), layer.linetype(), layer.weight().map(StandardWeight::code)), ("CURVE", linetype, Some(35)));
                assert_eq!(entity.weight(), Some(EntityWeight::Explicit(StandardWeight::from_code(0).unwrap())));
            }
        }
    }

    #[test]
    fn writer_reader_keep_degree_nine_interior_knots_without_normalizing_the_domain() {
        let mut source = DxfFile::new().unwrap();
        source
            .push(
                source.default_layer(),
                DxfShape::Spline {
                    degree: 9,
                    knots: vec![
                        -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -0.5, 2.5, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0,
                    ],
                    ctrl: vec![
                        Vec2::new(0.0, 0.0),
                        Vec2::new(1.0, 1.0),
                        Vec2::new(2.0, 4.0),
                        Vec2::new(3.0, 1.0),
                        Vec2::new(4.0, 0.0),
                        Vec2::new(5.0, -1.0),
                        Vec2::new(6.0, -2.0),
                        Vec2::new(7.0, -1.0),
                        Vec2::new(8.0, 0.0),
                        Vec2::new(9.0, 1.0),
                        Vec2::new(10.0, 2.0),
                        Vec2::new(11.0, 0.0),
                    ],
                    weights: Some(vec![1.0, 0.5, 2.0, 1.0, 0.25, 4.0, 1.0, 2.0, 0.5, 1.0, 4.0, 1.0]),
                },
                Some(EntityWeight::Default),
            )
            .unwrap();
        let recovered = read_written_file(&source);
        assert_eq!(recovered.entities().len(), 1);
        let entity = &recovered.entities()[0];
        let DxfShape::Spline { degree, knots, ctrl, weights } = entity.geometry() else { panic!("expected degree-nine spline") };
        assert_eq!(*degree, 9);
        assert_eq!(knots, &[-2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -0.5, 2.5, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0]);
        assert_eq!(
            ctrl,
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(2.0, 4.0),
                Vec2::new(3.0, 1.0),
                Vec2::new(4.0, 0.0),
                Vec2::new(5.0, -1.0),
                Vec2::new(6.0, -2.0),
                Vec2::new(7.0, -1.0),
                Vec2::new(8.0, 0.0),
                Vec2::new(9.0, 1.0),
                Vec2::new(10.0, 2.0),
                Vec2::new(11.0, 0.0)
            ]
        );
        assert_eq!(weights.as_deref(), Some([1.0, 0.5, 2.0, 1.0, 0.25, 4.0, 1.0, 2.0, 0.5, 1.0, 4.0, 1.0].as_slice()));
        assert_eq!(recovered.layer(entity.layer()).unwrap().name(), "0");
        assert_eq!(entity.weight(), Some(EntityWeight::Default));
    }

    // DEPENDENT CASE ONLY: apply after W1's real DxfShape::Point(Vec2) and writer
    // support plus Kernel's real POINT read_dxf_file branch are integrated.
    // No fake variant/API, ignore attribute or active-source compile claim.
    #[test]
    fn writer_reader_keep_point_geometry_layer_and_stored_weight() {
        let mut source = DxfFile::new().unwrap();
        let points = source.declare_layer("DATUM", 4, DxfLinetype::Hidden, Some(StandardWeight::from_code(50).unwrap())).unwrap();
        source.push(points, DxfShape::Point(Vec2::new(-0.5, 1.25)), Some(EntityWeight::ByBlock)).unwrap();
        source.push(source.default_layer(), DxfShape::Point(Vec2::new(8.0, -2.0)), None).unwrap();
        let recovered = read_written_file(&source);
        assert_eq!(recovered.entities().len(), 2);
        assert_eq!(recovered.entities()[0].geometry(), &DxfShape::Point(Vec2::new(-0.5, 1.25)));
        assert_eq!(recovered.entities()[1].geometry(), &DxfShape::Point(Vec2::new(8.0, -2.0)));
        let layer = recovered.layer(recovered.entities()[0].layer()).unwrap();
        assert_eq!(
            (layer.name(), layer.color(), layer.linetype(), layer.weight().map(StandardWeight::code)),
            ("DATUM", 4, DxfLinetype::Hidden, Some(50))
        );
        assert_eq!(recovered.entities()[0].weight(), Some(EntityWeight::ByBlock));
        assert_eq!(recovered.layer(recovered.entities()[1].layer()).unwrap().name(), "0");
        assert_eq!(recovered.entities()[1].weight(), None);
        assert!(recovered.layer(points).is_err());
    }

    #[test]
    fn layer_zero_default_constructor_keeps_new_metadata_and_writer_bytes() {
        let mut original = DxfFile::new().unwrap();
        let mut imported = DxfFile::with_layer_zero(7, DxfLinetype::Continuous, None).unwrap();
        for file in [&original, &imported] {
            assert_eq!(file.layers().len(), 1);
            assert!(file.entities().is_empty());
            let zero = file.layer(file.default_layer()).unwrap();
            assert_eq!((zero.name(), zero.color(), zero.linetype(), zero.weight()), ("0", 7, DxfLinetype::Continuous, None));
        }
        assert_eq!(crate::write_dxf_file(&original).unwrap(), crate::write_dxf_file(&imported).unwrap());
        original.push(original.default_layer(), DxfShape::Line(Vec2::new(-1.0, 0.5), Vec2::new(2.0, 0.5)), None).unwrap();
        imported.push(imported.default_layer(), DxfShape::Line(Vec2::new(-1.0, 0.5), Vec2::new(2.0, 0.5)), None).unwrap();
        assert_eq!(original.effective_weight(0).unwrap(), EntityWeight::Default);
        assert_eq!(imported.effective_weight(0).unwrap(), EntityWeight::Default);
        assert_eq!(crate::write_dxf_file(&original).unwrap(), crate::write_dxf_file(&imported).unwrap());
        assert!(original.layer(imported.default_layer()).is_err());
        assert!(imported.layer(original.default_layer()).is_err());
    }

    #[test]
    fn imported_layer_zero_is_first_and_immutable_through_every_zero_alias() {
        let zero_weight = Some(StandardWeight::from_code(0).unwrap());
        let mut file = DxfFile::with_layer_zero(3, DxfLinetype::Hidden, zero_weight).unwrap();
        let zero = file.default_layer();
        assert!(file.entities().is_empty());
        let cut = file.declare_layer("CUT", 1, DxfLinetype::Dashed, None).unwrap();
        file.push(zero, (), None).unwrap();
        for alias in ["0", " 0 ", "\t0\r\n", "", " \t\r\n\x0b\x0c "] {
            assert_eq!(file.declare_layer(alias, 3, DxfLinetype::Hidden, zero_weight).unwrap(), zero);
            assert!(file.declare_layer(alias, 7, DxfLinetype::Hidden, zero_weight).is_err());
            assert!(file.declare_layer(alias, 3, DxfLinetype::Continuous, zero_weight).is_err());
            assert!(file.declare_layer(alias, 3, DxfLinetype::Hidden, None).is_err());
        }
        assert_eq!(file.layers().iter().map(Layer::name).collect::<Vec<_>>(), ["0", "CUT"]);
        let layer = file.layer(zero).unwrap();
        assert_eq!((layer.color(), layer.linetype(), layer.weight().map(StandardWeight::code)), (3, DxfLinetype::Hidden, Some(0)));
        assert_eq!(file.layer(cut).unwrap().name(), "CUT");
        assert_eq!(file.entities().len(), 1);
        assert_eq!(file.entities()[0].layer(), zero);
    }

    #[test]
    fn imported_zero_inheritance_keeps_omission_symbols_and_physical_zero_distinct() {
        for code in [None, Some(0), Some(50)] {
            let layer_weight = code.map(|value| StandardWeight::from_code(value).unwrap());
            let mut file = DxfFile::with_layer_zero(5, DxfLinetype::Dashed, layer_weight).unwrap();
            for weight in [
                None,
                Some(EntityWeight::ByLayer),
                Some(EntityWeight::ByBlock),
                Some(EntityWeight::Default),
                Some(EntityWeight::Explicit(StandardWeight::from_code(13).unwrap())),
            ] {
                file.push(file.default_layer(), (), weight).unwrap();
            }
            let inherited = match code {
                None => EntityWeight::Default,
                Some(0) => EntityWeight::Explicit(StandardWeight::from_code(0).unwrap()),
                Some(50) => EntityWeight::Explicit(StandardWeight::from_code(50).unwrap()),
                _ => panic!("unexpected fixture weight"),
            };
            assert_eq!(file.layer(file.default_layer()).unwrap().weight().map(StandardWeight::code), code);
            assert_eq!(file.entities()[0].weight(), None);
            assert_eq!(file.entities()[1].weight(), Some(EntityWeight::ByLayer));
            assert_eq!(file.effective_weight(0).unwrap(), inherited);
            assert_eq!(file.effective_weight(1).unwrap(), inherited);
            assert_eq!(file.effective_weight(2).unwrap(), EntityWeight::ByBlock);
            assert_eq!(file.effective_weight(3).unwrap(), EntityWeight::Default);
            assert_eq!(file.effective_weight(4).unwrap(), EntityWeight::Explicit(StandardWeight::from_code(13).unwrap()));
            assert_eq!(file.effective_linetype(0).unwrap(), DxfLinetype::Dashed);
        }
    }

    #[test]
    fn imported_layer_zero_uses_validated_colors_and_standard_weight_types() {
        for color in [i16::MIN, -1, 0, 256, i16::MAX] {
            let error = DxfFile::<()>::with_layer_zero(color, DxfLinetype::Continuous, None).unwrap_err();
            assert!(error.to_string().contains("color must be an index from 1 to 255"));
        }
        for color in [1, 255] {
            let file = DxfFile::<()>::with_layer_zero(color, DxfLinetype::Hidden, Some(StandardWeight::from_code(211).unwrap())).unwrap();
            let zero = file.layer(file.default_layer()).unwrap();
            assert_eq!((zero.color(), zero.linetype(), zero.weight().map(StandardWeight::code)), (color, DxfLinetype::Hidden, Some(211)));
        }
        assert!(StandardWeight::from_code(6).is_err());
    }

    #[test]
    fn imported_zero_handles_are_fresh_and_foreign_insertion_is_atomic() {
        let first = DxfFile::<()>::with_layer_zero(1, DxfLinetype::Dashed, None).unwrap();
        let mut second = DxfFile::with_layer_zero(1, DxfLinetype::Dashed, None).unwrap();
        let mut ordinary = DxfFile::new().unwrap();
        assert!(second.push(first.default_layer(), (), None).is_err());
        assert!(ordinary.push(second.default_layer(), (), None).is_err());
        assert!(second.push(ordinary.default_layer(), (), None).is_err());
        assert!(second.entities().is_empty());
        assert!(ordinary.entities().is_empty());
        second.push(second.default_layer(), (), None).unwrap();
        assert_eq!(second.entities().len(), 1);
        assert_eq!(second.layers().len(), 1);
        assert_eq!(second.layer(second.entities()[0].layer()).unwrap().name(), "0");
    }

    #[test]
    fn imported_zero_constructor_keeps_fallible_resource_limits() {
        assert!(DxfFile::<()>::with_layer_zero_limits(1, DxfLinetype::Dashed, None, 0, 1).is_err());
        let mut empty = DxfFile::with_layer_zero_limits(1, DxfLinetype::Dashed, None, 1, 0).unwrap();
        assert!(empty.push(empty.default_layer(), (), None).is_err());
        assert!(empty.entities().is_empty());
        let mut file = DxfFile::with_layer_zero_limits(2, DxfLinetype::Hidden, None, 2, 1).unwrap();
        file.declare_layer("CUT", 1, DxfLinetype::Continuous, None).unwrap();
        assert!(file.declare_layer("EXCESS", 1, DxfLinetype::Continuous, None).is_err());
        file.push(file.default_layer(), (), None).unwrap();
        assert!(file.push(file.default_layer(), (), None).is_err());
        assert_eq!((file.layers().len(), file.entities().len()), (2, 1));
        let bounded = DxfFile::<()>::with_layer_zero_limits(7, DxfLinetype::Continuous, None, usize::MAX, usize::MAX).unwrap();
        assert_eq!((bounded.max_layers, bounded.max_entities), (1024, 1_000_000));
    }

    // Additive X2 builder/writer coverage; original X3 reader oracles above stay unchanged.
    type X2WireRecord<'a> = (&'a str, Vec<(i32, &'a str)>);

    fn x2_wire_records(output: &str) -> Vec<X2WireRecord<'_>> {
        let mut lines = output.lines();
        let mut pairs = Vec::new();
        while let Some(code) = lines.next() {
            pairs.push((code.trim().parse::<i32>().unwrap(), lines.next().unwrap()));
        }
        pairs
            .iter()
            .enumerate()
            .filter_map(|(index, (code, kind))| {
                if *code != 0 {
                    return None;
                }
                let tail = &pairs[index + 1..];
                let end = tail.iter().position(|(code, _)| *code == 0).unwrap_or(tail.len());
                Some((*kind, tail[..end].to_vec()))
            })
            .collect()
    }

    fn x2_wire_entities(output: &str) -> Vec<X2WireRecord<'_>> {
        // Out pads numeric group codes. Select its section from the existing
        // tokenized records without changing any literal field expectations.
        let records = x2_wire_records(output);
        let start = records
            .iter()
            .position(|(kind, fields)| *kind == "SECTION" && x2_wire_values(fields, 2).as_slice() == ["ENTITIES"])
            .expect("Out includes an ENTITIES section");
        let end = records[start + 1..].iter().position(|(kind, _)| *kind == "ENDSEC").expect("Out closes its ENTITIES section");
        records.into_iter().skip(start + 1).take(end).collect()
    }

    fn x2_wire_values<'a>(record: &[(i32, &'a str)], code: i32) -> Vec<&'a str> {
        record.iter().filter_map(|(field, value)| (*field == code).then_some(*value)).collect()
    }

    fn x2_wire_fields(record: &[(i32, &str)], fields: &[(i32, &[&str])]) {
        for (code, expected) in fields {
            assert_eq!(x2_wire_values(record, *code).as_slice(), *expected, "wire group{code}");
        }
    }

    #[test]
    fn owned_writer_keeps_mixed_layer_declarations_entity_order_and_file_scoped_handles() {
        let mut source = DxfFile::new().unwrap();
        let cut = source.declare_layer("CUT", 1, DxfLinetype::Continuous, Some(StandardWeight::from_code(50).unwrap())).unwrap();
        let hidden = source.declare_layer("HIDDEN", 8, DxfLinetype::Hidden, Some(StandardWeight::from_code(0).unwrap())).unwrap();
        source.declare_layer("UNUSED", 3, DxfLinetype::Dashed, None).unwrap();
        source.push(cut, DxfShape::Line(Vec2::new(-2.0, 1.5), Vec2::new(3.0, 1.5)), None).unwrap();
        source.push(source.default_layer(), DxfShape::Circle { c: Vec2::new(4.0, -1.0), r: 0.5 }, Some(EntityWeight::ByBlock)).unwrap();
        source.push(hidden, DxfShape::Arc { c: Vec2::new(-3.0, 2.0), r: 2.0, a0: 350.0, a1: 10.0 }, Some(EntityWeight::ByLayer)).unwrap();
        source.push(cut, DxfShape::Line(Vec2::new(7.0, 0.0), Vec2::new(7.0, 4.0)), Some(EntityWeight::Default)).unwrap();

        let written = crate::write_dxf_file(&source).unwrap();
        let records = x2_wire_records(&written);
        let layers: Vec<_> = records.iter().filter(|(kind, _)| *kind == "LAYER").collect();
        assert_eq!(layers.len(), 4);
        for ((_, record), (name, color, kind, weight)) in layers.iter().zip([
            ("0", "7", "Continuous", None),
            ("CUT", "1", "Continuous", Some("50")),
            ("HIDDEN", "8", "HIDDEN", Some("0")),
            ("UNUSED", "3", "DASHED", None),
        ]) {
            x2_wire_fields(record, &[(2, &[name]), (62, &[color]), (6, &[kind])]);
            assert_eq!(x2_wire_values(record, 370), [weight.unwrap_or("-3")]);
        }
        assert_eq!(source.layers().iter().map(|layer| layer.weight().map(StandardWeight::code)).collect::<Vec<_>>(), [None, Some(50), Some(0), None]);
        let entities = x2_wire_entities(&written);
        assert_eq!(entities.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(), ["LINE", "CIRCLE", "ARC", "LINE"]);
        for ((_, record), (layer, weight)) in entities.iter().zip([("CUT", None), ("0", Some("-2")), ("HIDDEN", Some("-1")), ("CUT", Some("-3"))]) {
            x2_wire_fields(record, &[(8, &[layer])]);
            assert_eq!(x2_wire_values(record, 370), weight.into_iter().collect::<Vec<_>>());
        }
        x2_wire_fields(&entities[0].1, &[(10, &["-2.0"]), (20, &["1.5"]), (11, &["3.0"]), (21, &["1.5"])]);
        x2_wire_fields(&entities[1].1, &[(10, &["4.0"]), (20, &["-1.0"]), (40, &["0.5"])]);
        x2_wire_fields(&entities[2].1, &[(10, &["-3.0"]), (20, &["2.0"]), (40, &["2.0"]), (50, &["350.0"]), (51, &["10.0"])]);
        x2_wire_fields(&entities[3].1, &[(10, &["7.0"]), (20, &["0.0"]), (11, &["7.0"]), (21, &["4.0"])]);
        let copied = source.translated_scaled(Vec2::ZERO, 1.0).unwrap();
        let layers: Vec<_> =
            source.layers().iter().map(|layer| (layer.name(), layer.color(), layer.linetype(), layer.weight().map(StandardWeight::code))).collect();
        assert_eq!(
            layers,
            [
                ("0", 7, DxfLinetype::Continuous, None),
                ("CUT", 1, DxfLinetype::Continuous, Some(50)),
                ("HIDDEN", 8, DxfLinetype::Hidden, Some(0)),
                ("UNUSED", 3, DxfLinetype::Dashed, None),
            ]
        );
        let names: Vec<_> = source.entities().iter().map(|entity| source.layer(entity.layer()).unwrap().name()).collect();
        assert_eq!(names, ["CUT", "0", "HIDDEN", "CUT"]);
        assert_eq!(source.entities().len(), 4);
        assert_eq!(source.entities()[0].geometry(), &DxfShape::Line(Vec2::new(-2.0, 1.5), Vec2::new(3.0, 1.5)));
        assert_eq!(source.entities()[1].geometry(), &DxfShape::Circle { c: Vec2::new(4.0, -1.0), r: 0.5 });
        assert_eq!(source.entities()[2].geometry(), &DxfShape::Arc { c: Vec2::new(-3.0, 2.0), r: 2.0, a0: 350.0, a1: 10.0 });
        assert_eq!(source.entities()[3].geometry(), &DxfShape::Line(Vec2::new(7.0, 0.0), Vec2::new(7.0, 4.0)));
        assert_eq!(
            source.entities().iter().map(Entity::weight).collect::<Vec<_>>(),
            [None, Some(EntityWeight::ByBlock), Some(EntityWeight::ByLayer), Some(EntityWeight::Default)]
        );
        // The actual X2 copy API mints fresh ownership; this is not a reader round trip.
        assert!(copied.layer(cut).is_err());
        assert!(copied.layer(source.default_layer()).is_err());
        assert!(source.layer(copied.default_layer()).is_err());
    }
    #[test]
    fn owned_writer_keeps_omission_symbols_and_all_physical_weights_in_each_linetype() {
        let mut weights = vec![None, Some(EntityWeight::ByLayer), Some(EntityWeight::ByBlock), Some(EntityWeight::Default)];
        // Independent explicit public code list: not the model's internal lookup table.
        for code in [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211] {
            weights.push(Some(EntityWeight::Explicit(StandardWeight::from_code(code).unwrap())));
        }
        let styles = [
            ("MEASURED", 1, DxfLinetype::Continuous, Some(50)),
            ("OMITTED", 3, DxfLinetype::Dashed, None),
            ("ZERO_WEIGHT", 8, DxfLinetype::Hidden, Some(0)),
        ];
        let mut source = DxfFile::new().unwrap();
        for (name, color, linetype, weight) in styles {
            let layer = source.declare_layer(name, color, linetype, weight.map(|code| StandardWeight::from_code(code).unwrap())).unwrap();
            for weight in &weights {
                source.push(layer, DxfShape::Line(Vec2::new(0.5, 0.75), Vec2::new(1.25, -0.5)), *weight).unwrap();
            }
        }
        let written = crate::write_dxf_file(&source).unwrap();
        let entities = x2_wire_entities(&written);
        assert_eq!(entities.len(), 84);
        let records = x2_wire_records(&written);
        let layers: Vec<_> = records.iter().filter(|(kind, _)| *kind == "LAYER").collect();
        assert_eq!(layers.len(), 4);
        let wire_weights = [
            None,
            Some("-1"),
            Some("-2"),
            Some("-3"),
            Some("0"),
            Some("5"),
            Some("9"),
            Some("13"),
            Some("15"),
            Some("18"),
            Some("20"),
            Some("25"),
            Some("30"),
            Some("35"),
            Some("40"),
            Some("50"),
            Some("53"),
            Some("60"),
            Some("70"),
            Some("80"),
            Some("90"),
            Some("100"),
            Some("106"),
            Some("120"),
            Some("140"),
            Some("158"),
            Some("200"),
            Some("211"),
        ];
        for (style_index, (name, color, kind, weight)) in
            [("MEASURED", "1", "Continuous", Some("50")), ("OMITTED", "3", "DASHED", None), ("ZERO_WEIGHT", "8", "HIDDEN", Some("0"))]
                .into_iter()
                .enumerate()
        {
            x2_wire_fields(&layers[style_index + 1].1, &[(2, &[name]), (62, &[color]), (6, &[kind])]);
            assert_eq!(x2_wire_values(&layers[style_index + 1].1, 370), [weight.unwrap_or("-3")]);
            assert_eq!(source.layers()[style_index + 1].weight().map(StandardWeight::code), styles[style_index].3);
            for (index, expected_weight) in wire_weights.iter().enumerate() {
                let (entity, record) = &entities[style_index * 28 + index];
                assert_eq!(*entity, "LINE");
                x2_wire_fields(record, &[(8, &[name]), (10, &["0.5"]), (20, &["0.75"]), (11, &["1.25"]), (21, &["-0.5"])]);
                assert_eq!(x2_wire_values(record, 370), expected_weight.iter().copied().collect::<Vec<_>>());
            }
        }
        assert_eq!(source.layers().len(), 4);
        assert_eq!(source.entities().len(), 84);
        for (style_index, (name, color, linetype, layer_weight)) in styles.iter().enumerate() {
            for (weight_index, expected_weight) in weights.iter().enumerate() {
                let index = style_index * 28 + weight_index;
                let entity = &source.entities()[index];
                let layer = source.layer(entity.layer()).unwrap();
                assert_eq!(
                    (layer.name(), layer.color(), layer.linetype(), layer.weight().map(StandardWeight::code)),
                    (*name, *color, *linetype, *layer_weight)
                );
                assert_eq!(entity.weight(), *expected_weight, "style={name} weight ordinal={weight_index}");
                assert_eq!(entity.geometry(), &DxfShape::Line(Vec2::new(0.5, 0.75), Vec2::new(1.25, -0.5)));
            }
        }
    }
    #[test]
    fn owned_writer_keeps_exact_supported_curves_on_all_three_layer_styles() {
        for (name, linetype) in [("SOLID", DxfLinetype::Continuous), ("DASH", DxfLinetype::Dashed), ("HIDE", DxfLinetype::Hidden)] {
            let mut source = DxfFile::new().unwrap();
            let layer = source.declare_layer(name, 5, linetype, None).unwrap();
            for shape in [
                DxfShape::Line(Vec2::new(-1.25, 2.5), Vec2::new(3.75, -4.0)),
                DxfShape::Circle { c: Vec2::new(8.0, -2.0), r: 1.25 },
                DxfShape::Arc { c: Vec2::new(2.0, 3.0), r: 0.75, a0: 45.0, a1: 135.0 },
                // Open straight chain, with an intentional repeated vertex retained.
                DxfShape::Polyline { points: vec![Vec2::new(-2.0, 0.0), Vec2::new(0.0, 1.5), Vec2::new(0.0, 1.5), Vec2::new(4.0, -0.5)] },
            ] {
                source.push(layer, shape, Some(EntityWeight::ByLayer)).unwrap();
            }
            let written = crate::write_dxf_file(&source).unwrap();
            let entities = x2_wire_entities(&written);
            assert_eq!(entities.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(), ["LINE", "CIRCLE", "ARC", "LWPOLYLINE"]);
            let wire_kind = match name {
                "SOLID" => "Continuous",
                "DASH" => "DASHED",
                "HIDE" => "HIDDEN",
                _ => panic!("unexpected fixture"),
            };
            let records = x2_wire_records(&written);
            let layer = records.iter().find(|(kind, record)| *kind == "LAYER" && x2_wire_values(record, 2) == [name]).unwrap();
            x2_wire_fields(&layer.1, &[(6, &[wire_kind]), (62, &["5"])]);
            for (_, record) in &entities {
                x2_wire_fields(record, &[(8, &[name]), (370, &["-1"])]);
            }
            x2_wire_fields(&entities[0].1, &[(10, &["-1.25"]), (20, &["2.5"]), (11, &["3.75"]), (21, &["-4.0"])]);
            x2_wire_fields(&entities[1].1, &[(10, &["8.0"]), (20, &["-2.0"]), (40, &["1.25"])]);
            x2_wire_fields(&entities[2].1, &[(10, &["2.0"]), (20, &["3.0"]), (40, &["0.75"]), (50, &["45.0"]), (51, &["135.0"])]);
            x2_wire_fields(
                &entities[3].1,
                &[(90, &["4"]), (70, &["128"]), (10, &["-2.0", "0.0", "0.0", "4.0"]), (20, &["0.0", "1.5", "1.5", "-0.5"])],
            );
            assert_eq!(source.entities().len(), 4);
            assert_eq!(source.entities()[0].geometry(), &DxfShape::Line(Vec2::new(-1.25, 2.5), Vec2::new(3.75, -4.0)));
            assert_eq!(source.entities()[1].geometry(), &DxfShape::Circle { c: Vec2::new(8.0, -2.0), r: 1.25 });
            assert_eq!(source.entities()[2].geometry(), &DxfShape::Arc { c: Vec2::new(2.0, 3.0), r: 0.75, a0: 45.0, a1: 135.0 });
            assert_eq!(
                source.entities()[3].geometry(),
                &DxfShape::Polyline { points: vec![Vec2::new(-2.0, 0.0), Vec2::new(0.0, 1.5), Vec2::new(0.0, 1.5), Vec2::new(4.0, -0.5)] }
            );
            for entity in source.entities() {
                let layer = source.layer(entity.layer()).unwrap();
                assert_eq!((layer.name(), layer.linetype()), (name, linetype));
                assert_eq!(entity.weight(), Some(EntityWeight::ByLayer));
            }
        }
    }
    #[test]
    fn owned_writer_keeps_degree_nine_nonunit_knots_controls_and_optional_rational_weights() {
        for linetype in [DxfLinetype::Continuous, DxfLinetype::Dashed, DxfLinetype::Hidden] {
            let mut source = DxfFile::new().unwrap();
            let layer = source.declare_layer("CURVE", 2, linetype, Some(StandardWeight::from_code(35).unwrap())).unwrap();
            for weights in [None, Some(vec![1.0; 10]), Some(vec![0.5, 1.0, 2.0, 4.0, 0.25, 0.5, 1.0, 2.0, 4.0, 1.0])] {
                source
                    .push(
                        layer,
                        DxfShape::Spline {
                            degree: 9,
                            knots: vec![-2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0],
                            ctrl: vec![
                                Vec2::new(0.0, 0.0),
                                Vec2::new(1.0, 2.0),
                                Vec2::new(2.0, 4.0),
                                Vec2::new(3.0, 8.0),
                                Vec2::new(4.0, 4.0),
                                Vec2::new(5.0, 2.0),
                                Vec2::new(6.0, 1.0),
                                Vec2::new(7.0, 0.0),
                                Vec2::new(8.0, -1.0),
                                Vec2::new(9.0, -2.0),
                            ],
                            weights,
                        },
                        Some(EntityWeight::Explicit(StandardWeight::from_code(0).unwrap())),
                    )
                    .unwrap();
            }
            let written = crate::write_dxf_file(&source).unwrap();
            let entities = x2_wire_entities(&written);
            assert_eq!(entities.len(), 3);
            for (index, (kind, record)) in entities.iter().enumerate() {
                assert_eq!(*kind, "SPLINE");
                x2_wire_fields(
                    record,
                    &[
                        (8, &["CURVE"]),
                        (370, &["0"]),
                        (71, &["9"]),
                        (72, &["20"]),
                        (73, &["10"]),
                        (74, &["0"]),
                        (10, &["0.0", "1.0", "2.0", "3.0", "4.0", "5.0", "6.0", "7.0", "8.0", "9.0"]),
                        (20, &["0.0", "2.0", "4.0", "8.0", "4.0", "2.0", "1.0", "0.0", "-1.0", "-2.0"]),
                        (
                            40,
                            &[
                                "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "6.0", "6.0", "6.0", "6.0", "6.0",
                                "6.0", "6.0", "6.0", "6.0", "6.0",
                            ],
                        ),
                    ],
                );
                let expected: &[&str] = match index {
                    0 => &[],
                    1 => &["1.0"; 10],
                    2 => &["0.5", "1.0", "2.0", "4.0", "0.25", "0.5", "1.0", "2.0", "4.0", "1.0"],
                    _ => panic!("unexpected fixture"),
                };
                assert_eq!(x2_wire_values(record, 41).as_slice(), expected);
                assert_eq!(x2_wire_values(record, 70), if index == 0 { vec!["8"] } else { vec!["12"] });
            }
            assert_eq!(source.entities().len(), 3);
            for (index, entity) in source.entities().iter().enumerate() {
                let DxfShape::Spline { degree, knots, ctrl, weights } = entity.geometry() else { panic!("expected degree-nine spline") };
                assert_eq!(*degree, 9);
                assert_eq!(
                    ctrl,
                    &[
                        Vec2::new(0.0, 0.0),
                        Vec2::new(1.0, 2.0),
                        Vec2::new(2.0, 4.0),
                        Vec2::new(3.0, 8.0),
                        Vec2::new(4.0, 4.0),
                        Vec2::new(5.0, 2.0),
                        Vec2::new(6.0, 1.0),
                        Vec2::new(7.0, 0.0),
                        Vec2::new(8.0, -1.0),
                        Vec2::new(9.0, -2.0)
                    ]
                );
                assert_eq!(knots, &[-2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0]);
                match index {
                    0 => assert_eq!(weights, &None),
                    1 => assert_eq!(weights.as_deref(), Some([1.0; 10].as_slice())),
                    2 => assert_eq!(weights.as_deref(), Some([0.5, 1.0, 2.0, 4.0, 0.25, 0.5, 1.0, 2.0, 4.0, 1.0].as_slice())),
                    _ => panic!("unexpected extra spline"),
                }
                let layer = source.layer(entity.layer()).unwrap();
                assert_eq!((layer.name(), layer.linetype(), layer.weight().map(StandardWeight::code)), ("CURVE", linetype, Some(35)));
                assert_eq!(entity.weight(), Some(EntityWeight::Explicit(StandardWeight::from_code(0).unwrap())));
            }
        }
    }
    #[test]
    fn owned_writer_keeps_degree_nine_interior_knots_without_normalizing_the_domain() {
        let mut source = DxfFile::new().unwrap();
        source
            .push(
                source.default_layer(),
                DxfShape::Spline {
                    degree: 9,
                    knots: vec![
                        -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -0.5, 2.5, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0,
                    ],
                    ctrl: vec![
                        Vec2::new(0.0, 0.0),
                        Vec2::new(1.0, 1.0),
                        Vec2::new(2.0, 4.0),
                        Vec2::new(3.0, 1.0),
                        Vec2::new(4.0, 0.0),
                        Vec2::new(5.0, -1.0),
                        Vec2::new(6.0, -2.0),
                        Vec2::new(7.0, -1.0),
                        Vec2::new(8.0, 0.0),
                        Vec2::new(9.0, 1.0),
                        Vec2::new(10.0, 2.0),
                        Vec2::new(11.0, 0.0),
                    ],
                    weights: Some(vec![1.0, 0.5, 2.0, 1.0, 0.25, 4.0, 1.0, 2.0, 0.5, 1.0, 4.0, 1.0]),
                },
                Some(EntityWeight::Default),
            )
            .unwrap();
        let written = crate::write_dxf_file(&source).unwrap();
        let entities = x2_wire_entities(&written);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0, "SPLINE");
        x2_wire_fields(
            &entities[0].1,
            &[
                (8, &["0"]),
                (370, &["-3"]),
                (70, &["12"]),
                (71, &["9"]),
                (72, &["22"]),
                (73, &["12"]),
                (10, &["0.0", "1.0", "2.0", "3.0", "4.0", "5.0", "6.0", "7.0", "8.0", "9.0", "10.0", "11.0"]),
                (20, &["0.0", "1.0", "4.0", "1.0", "0.0", "-1.0", "-2.0", "-1.0", "0.0", "1.0", "2.0", "0.0"]),
                (
                    40,
                    &[
                        "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-2.0", "-0.5", "2.5", "6.0", "6.0", "6.0", "6.0",
                        "6.0", "6.0", "6.0", "6.0", "6.0", "6.0",
                    ],
                ),
                (41, &["1.0", "0.5", "2.0", "1.0", "0.25", "4.0", "1.0", "2.0", "0.5", "1.0", "4.0", "1.0"]),
            ],
        );
        assert_eq!(source.entities().len(), 1);
        let entity = &source.entities()[0];
        let DxfShape::Spline { degree, knots, ctrl, weights } = entity.geometry() else { panic!("expected degree-nine spline") };
        assert_eq!(*degree, 9);
        assert_eq!(knots, &[-2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0, -0.5, 2.5, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0]);
        assert_eq!(
            ctrl,
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(2.0, 4.0),
                Vec2::new(3.0, 1.0),
                Vec2::new(4.0, 0.0),
                Vec2::new(5.0, -1.0),
                Vec2::new(6.0, -2.0),
                Vec2::new(7.0, -1.0),
                Vec2::new(8.0, 0.0),
                Vec2::new(9.0, 1.0),
                Vec2::new(10.0, 2.0),
                Vec2::new(11.0, 0.0)
            ]
        );
        assert_eq!(weights.as_deref(), Some([1.0, 0.5, 2.0, 1.0, 0.25, 4.0, 1.0, 2.0, 0.5, 1.0, 4.0, 1.0].as_slice()));
        assert_eq!(source.layer(entity.layer()).unwrap().name(), "0");
        assert_eq!(entity.weight(), Some(EntityWeight::Default));
    }
    #[test]
    fn owned_writer_keeps_point_geometry_layer_and_stored_weight() {
        let mut source = DxfFile::new().unwrap();
        let points = source.declare_layer("DATUM", 4, DxfLinetype::Hidden, Some(StandardWeight::from_code(50).unwrap())).unwrap();
        source.push(points, DxfShape::Point(Vec2::new(-0.5, 1.25)), Some(EntityWeight::ByBlock)).unwrap();
        source.push(source.default_layer(), DxfShape::Point(Vec2::new(8.0, -2.0)), None).unwrap();
        let written = crate::write_dxf_file(&source).unwrap();
        let entities = x2_wire_entities(&written);
        assert_eq!(entities.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(), ["POINT", "POINT"]);
        x2_wire_fields(&entities[0].1, &[(8, &["DATUM"]), (370, &["-2"]), (10, &["-0.5"]), (20, &["1.25"]), (30, &["0.0"])]);
        x2_wire_fields(&entities[1].1, &[(8, &["0"]), (10, &["8.0"]), (20, &["-2.0"]), (30, &["0.0"])]);
        assert!(x2_wire_values(&entities[1].1, 370).is_empty());
        let records = x2_wire_records(&written);
        let layer = records.iter().find(|(kind, record)| *kind == "LAYER" && x2_wire_values(record, 2) == ["DATUM"]).unwrap();
        x2_wire_fields(&layer.1, &[(62, &["4"]), (6, &["HIDDEN"]), (370, &["50"])]);
        let copied = source.translated_scaled(Vec2::ZERO, 1.0).unwrap();
        assert_eq!(source.entities().len(), 2);
        assert_eq!(source.entities()[0].geometry(), &DxfShape::Point(Vec2::new(-0.5, 1.25)));
        assert_eq!(source.entities()[1].geometry(), &DxfShape::Point(Vec2::new(8.0, -2.0)));
        let layer = source.layer(source.entities()[0].layer()).unwrap();
        assert_eq!(
            (layer.name(), layer.color(), layer.linetype(), layer.weight().map(StandardWeight::code)),
            ("DATUM", 4, DxfLinetype::Hidden, Some(50))
        );
        assert_eq!(source.entities()[0].weight(), Some(EntityWeight::ByBlock));
        assert_eq!(source.layer(source.entities()[1].layer()).unwrap().name(), "0");
        assert_eq!(source.entities()[1].weight(), None);
        assert!(copied.layer(points).is_err());
    }

    #[test]
    fn legacy_baseline_exact_layer_admission_keeps_its_caps_and_cannot_relax_real_files() {
        let mut ordinary = DxfFile::<DxfShape>::new().unwrap();
        assert!(
            ordinary.declare_legacy_baseline_layer("中", 3, DxfLinetype::Continuous).unwrap_err().to_string().contains("private legacy baseline")
        );
        assert!(ordinary.declare_layer("中", 3, DxfLinetype::Continuous, None).is_err());
        ordinary.declare_layer("cut", 1, DxfLinetype::Continuous, None).unwrap();
        assert!(ordinary.declare_layer("CUT", 2, DxfLinetype::Continuous, None).is_err());
        for mut producer in [DxfFile::for_flat_pattern_export(0, 0, 0).unwrap(), DxfFile::for_current_view_export(0, 0, 0).unwrap()] {
            assert!(
                producer.declare_legacy_baseline_layer("中", 3, DxfLinetype::Continuous).unwrap_err().to_string().contains("private legacy baseline")
            );
            assert_eq!(producer.layers().len(), 1);
        }
        let mut baseline = super::super::export_compatibility::baseline_file().unwrap();
        let first = baseline.declare_legacy_baseline_layer("中", 3, DxfLinetype::Continuous).unwrap();
        assert_eq!(baseline.declare_legacy_baseline_layer("中", 5, DxfLinetype::Dashed).unwrap(), first);
        assert_eq!(baseline.layer(first).unwrap().color(), 3);
        assert_eq!(baseline.layer(first).unwrap().linetype(), DxfLinetype::Continuous);
        let lower = baseline.declare_legacy_baseline_layer("cut", 1, DxfLinetype::Continuous).unwrap();
        let upper = baseline.declare_legacy_baseline_layer("CUT", 2, DxfLinetype::Continuous).unwrap();
        assert_ne!(lower, upper);
        for color in [0, -1, 256] {
            let before = baseline.layers().len();
            assert!(baseline.declare_legacy_baseline_layer("bad", color, DxfLinetype::Continuous).is_err());
            assert_eq!(baseline.layers().len(), before);
        }
        baseline.declare_legacy_baseline_layer(&"é".repeat(350), 3, DxfLinetype::Continuous).unwrap();
        let before = baseline.layers().len();
        assert!(baseline.declare_legacy_baseline_layer(&"é".repeat(351), 3, DxfLinetype::Continuous).is_err());
        assert_eq!(baseline.layers().len(), before, "the existing 700-byte bound is measured before allocation");
        while baseline.layers().len() < 1024 {
            baseline.declare_legacy_baseline_layer(&format!("B{}", baseline.layers().len()), 7, DxfLinetype::Continuous).unwrap();
        }
        assert!(baseline.declare_legacy_baseline_layer("OVER", 7, DxfLinetype::Continuous).is_err());
        assert_eq!(baseline.layers().len(), 1024);
        assert!(baseline.entities().is_empty());
    }
}
