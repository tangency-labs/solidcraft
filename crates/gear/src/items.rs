//! The Gears feature's items contract (GEAR_UI_SPEC.md §3, §6; gear-drivetrain-plan.md WP0),
//! engine-neutral: plain types the dialog, the engine and the planners share.
//!
//! A drivetrain is a list of [`GearItem`]s. Each item has a type ([`ItemKind`]), the geometry
//! picked into its slots ([`Thing`]s, in world coordinates) and one [`GearSettings`] per gear it
//! makes. The dialog's links add [`ItemLink`]s between gears (Mesh or Shaft) and [`ModuleBlock`]s
//! (gears sharing one module and, optionally, more settings: [`crate::blocks`]); a [`Drive`]
//! names the gear that is turned.
//!
//! - [`slots`] / [`assign`] / [`validate_slot`]: which geometry an item type takes, and every
//!   geometric check on a pick (refusals say what is wrong and what to do, §5).
//! - [`resolve_module`] (decision 2) and [`face_extent`] (decision 4).
//! - [`gear_geoms`]: each gear's place (centre, side-A axis, pitch radius) from its item's things.
//! - [`suggest_links`]: dashed suggestions from the drawing (decision 3); [`effective_links`]
//!   uses them as the links only when the graph has none.
//! - [`stages`]: the link graph split into stages, one per mesh (a gear may be in several,
//!   decision 8) plus an item's own pair or planetary set, and shared-shaft clusters (rigid
//!   groups); each stage names its gears and their geometry for the planners.
//! - [`to_drivetrain`]: the items mapped onto [`crate::drivetrain::read`]'s input, with the
//!   explicit meshes and shafts beside them.

use crate::axes::{Axis, Relation, add, cross, dot, len, mul, num, relate, sub, unit};
use crate::drivetrain::{Ask, Circle3, Item, Line3, Role, Thing as DtThing};
use crate::plan::{Kind, SidePreset};
use crate::sketch_rules::TOLERANCE;

/// A point or vector in world coordinates (mm).
pub type V = [f64; 3];

fn finite(a: V) -> bool {
    a.iter().all(|v| v.is_finite())
}

/// Directions this close (radians) are parallel (looser than [`relate`]'s: picked sketch
/// geometry).
pub const ANGLE_TOL: f64 = 1e-3;

/// How close (mm) two places must be to count as one for a gear of size `size` (a pitch
/// radius): 0.5% of it ([`TOLERANCE`]), at least 0.005 mm.
pub fn tol(size: f64) -> f64 {
    1e-6 + TOLERANCE * size.abs().max(1.0)
}

fn parallel(a: V, b: V) -> bool {
    len(cross(a, b)) <= ANGLE_TOL
}

/// The foot of `p` on the line through `o` along unit `u`.
fn foot(p: V, o: V, u: V) -> V {
    Axis { origin: o, dir: u }.foot(p)
}

fn dist_to_line(p: V, o: V, u: V) -> f64 {
    Axis { origin: o, dir: u }.distance(p)
}

/// How two axes lie: [`relate`] with picked-geometry tolerances (`tol` the distance under
/// which they touch, [`ANGLE_TOL`] for parallel).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lie {
    Coaxial,
    Parallel { distance: f64 },
    Meet { apex: V, angle: f64 },
    Skew { offset: f64, angle: f64, foot1: V, foot2: V },
}

fn lie(o1: V, u1: V, o2: V, u2: V, tol: f64) -> Lie {
    if parallel(u1, u2) {
        let distance = dist_to_line(o2, o1, u1);
        return if distance <= tol { Lie::Coaxial } else { Lie::Parallel { distance } };
    }
    match relate(&Axis { origin: o1, dir: u1 }, &Axis { origin: o2, dir: u2 }) {
        Relation::Intersecting { angle, apex } => Lie::Meet { apex, angle },
        Relation::Skew { angle, offset, foot1, foot2 } if offset <= tol => Lie::Meet { apex: mul(add(foot1, foot2), 0.5), angle },
        Relation::Skew { angle, offset, foot1, foot2 } => Lie::Skew { offset, angle, foot1, foot2 },
        // Not parallel by ANGLE_TOL, so not by relate's tighter test either.
        Relation::Coincident | Relation::Parallel { .. } => Lie::Coaxial,
    }
}

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

/// Where a picked thing came from: its sketch (none for model geometry) and its id there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThingIds {
    pub sketch: Option<u64>,
    pub entity: String,
}

/// One piece of picked geometry, in world coordinates (mm).
#[derive(Clone, Debug, PartialEq)]
pub enum Thing {
    /// A pitch circle: its centre, the normal to its plane (side A), its radius.
    Circle { centre: V, normal: V, radius: f64, ids: ThingIds },
    /// A line from `a` to `b` (a shaft, an axis, a rack's pitch line); `construction`: a
    /// sketch helper line.
    Line { a: V, b: V, construction: bool, ids: ThingIds },
    /// A point (a bearing seat on a shaft line).
    Point { p: V, ids: ThingIds },
}

impl Thing {
    pub fn circle(centre: V, normal: V, radius: f64) -> Thing {
        Thing::Circle { centre, normal, radius, ids: ThingIds::default() }
    }
    pub fn line(a: V, b: V) -> Thing {
        Thing::Line { a, b, construction: false, ids: ThingIds::default() }
    }
    pub fn point(p: V) -> Thing {
        Thing::Point { p, ids: ThingIds::default() }
    }
    /// The same thing, with its ids.
    pub fn with_ids(mut self, sketch: Option<u64>, entity: &str) -> Thing {
        let ids = match &mut self {
            Thing::Circle { ids, .. } | Thing::Line { ids, .. } | Thing::Point { ids, .. } => ids,
        };
        *ids = ThingIds { sketch, entity: entity.to_string() };
        self
    }
    pub fn ids(&self) -> &ThingIds {
        match self {
            Thing::Circle { ids, .. } | Thing::Line { ids, .. } | Thing::Point { ids, .. } => ids,
        }
    }
    pub fn slot_kind(&self) -> SlotKind {
        match self {
            Thing::Circle { .. } => SlotKind::Circle,
            Thing::Line { .. } => SlotKind::Line,
            Thing::Point { .. } => SlotKind::Point,
        }
    }
    /// Why it can make nothing (a zero radius, a line of no length, not finite).
    fn degenerate(&self) -> Option<&'static str> {
        match self {
            Thing::Circle { centre, normal, radius, .. } => {
                if !(radius.is_finite() && *radius > 0.0) || !finite(*centre) {
                    Some("this circle has no size: pick a circle with a radius")
                } else if unit(*normal).is_none() {
                    Some("this circle has no plane: pick a circle drawn in a sketch")
                } else {
                    None
                }
            }
            Thing::Line { a, b, .. } => {
                (!finite(*a) || !finite(*b) || unit(sub(*b, *a)).is_none()).then_some("this line has no length: pick a line with two different ends")
            }
            Thing::Point { p, .. } => (!finite(*p)).then_some("this point is not a place: pick another"),
        }
    }
    fn same(&self, other: &Thing) -> bool {
        let (a, b) = (self.ids(), other.ids());
        if !a.entity.is_empty() && !b.entity.is_empty() {
            return a == b;
        }
        self == other
    }
}

/// What an item can be (GEAR_UI_SPEC §3); [`ItemKind::name`] is `gear.create`'s item `type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Spur,
    Helical,
    Herringbone,
    /// A ring gear (teeth inside), optionally with its pinion.
    Internal,
    /// A rack (its pitch line), optionally with its pinion.
    Rack,
    /// Two or more gears on one shaft, one rigid body.
    Compound,
    /// Sun, ring and planets on one axis, with a generated carrier (decision 9).
    Planetary,
    /// A bevel pair (straight, spiral or chevron: [`BevelStyle`]).
    Bevel,
    Hypoid,
    Worm,
    CrossedHelical,
    /// A shaft with its bearings (no teeth).
    Shaft,
}

impl ItemKind {
    pub const ALL: &[ItemKind] = &[
        ItemKind::Spur,
        ItemKind::Helical,
        ItemKind::Herringbone,
        ItemKind::Internal,
        ItemKind::Rack,
        ItemKind::Compound,
        ItemKind::Planetary,
        ItemKind::Bevel,
        ItemKind::Hypoid,
        ItemKind::Worm,
        ItemKind::CrossedHelical,
        ItemKind::Shaft,
    ];
    /// Its name in `gear.create`'s items (as the dialog's `ITEM_TYPES`).
    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Spur => "spur",
            ItemKind::Helical => "helical",
            ItemKind::Herringbone => "herringbone",
            ItemKind::Internal => "internal",
            ItemKind::Rack => "rack",
            ItemKind::Compound => "compound",
            ItemKind::Planetary => "planetary",
            ItemKind::Bevel => "bevel",
            ItemKind::Hypoid => "hypoid",
            ItemKind::Worm => "worm",
            ItemKind::CrossedHelical => "crossed_helical",
            ItemKind::Shaft => "shaft",
        }
    }
    /// Its label for people.
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Spur => "Spur",
            ItemKind::Helical => "Helical",
            ItemKind::Herringbone => "Herringbone",
            ItemKind::Internal => "Ring Gear",
            ItemKind::Rack => "Rack",
            ItemKind::Compound => "Compound",
            ItemKind::Planetary => "Planetary",
            ItemKind::Bevel => "Bevel Pair",
            ItemKind::Hypoid => "Hypoid Pair",
            ItemKind::Worm => "Worm and Wheel",
            ItemKind::CrossedHelical => "Crossed Helical",
            ItemKind::Shaft => "Shaft",
        }
    }
    /// The kind named `s` ([`ItemKind::name`]; `spiral_bevel` is a bevel pair).
    pub fn parse(s: &str) -> Option<ItemKind> {
        if s == "spiral_bevel" || s == "chevron_bevel" {
            return Some(ItemKind::Bevel);
        }
        ItemKind::ALL.iter().copied().find(|k| k.name() == s)
    }
}

/// Size by module (teeth follow from the circle) or by teeth (the module follows).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeBy {
    #[default]
    Module,
    Teeth,
}

/// A helix hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BevelStyle {
    #[default]
    Straight,
    Spiral,
    /// The spiral changes hand at mid-face.
    Chevron,
}

impl BevelStyle {
    pub fn name(self) -> &'static str {
        match self {
            BevelStyle::Straight => "straight",
            BevelStyle::Spiral => "spiral",
            BevelStyle::Chevron => "chevron",
        }
    }
}

/// The face preset (decision 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FacePreset {
    #[default]
    Symmetric,
    AOnly,
    BOnly,
    Custom,
}

/// How far the teeth reach each side of the sketch plane along the gear's axis (side A: the
/// axis direction, [`GearGeom::axis`]); `None`: Auto.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceSides {
    pub preset: FacePreset,
    pub a: Option<f64>,
    pub b: Option<f64>,
}

impl FaceSides {
    /// The sides as the planners take them ([`SidePreset`], decision 4): a preset whose reaches
    /// are Auto stays that preset; reaches given make a custom split (negative ones count as 0).
    pub fn side_preset(&self) -> SidePreset {
        let pos = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
        match self.preset {
            FacePreset::Symmetric => self.a.or(self.b).map_or(SidePreset::Symmetric, |v| SidePreset::Custom { a: pos(v), b: pos(v) }),
            FacePreset::AOnly => self.a.map_or(SidePreset::AOnly, |a| SidePreset::Custom { a: pos(a), b: 0.0 }),
            FacePreset::BOnly => self.b.map_or(SidePreset::BOnly, |b| SidePreset::Custom { a: 0.0, b: pos(b) }),
            FacePreset::Custom => match (self.a, self.b) {
                (None, None) => SidePreset::Symmetric,
                (a, b) => SidePreset::Custom { a: pos(a.unwrap_or(0.0)), b: pos(b.unwrap_or(0.0)) },
            },
        }
    }
}

/// An item's id as the engine and the dialog name it when it has none: A, B, … Z, then A2 …
pub fn item_id(i: usize) -> String {
    let letter = char::from(b'A' + (i % 26) as u8);
    if i < 26 { letter.to_string() } else { format!("{letter}{}", i / 26 + 1) }
}

/// The index [`item_id`] names (`None` for another id).
pub fn item_number(id: &str) -> Option<usize> {
    let mut c = id.chars();
    let letter = c.next().filter(char::is_ascii_uppercase)?;
    let round: usize = c.as_str().parse().unwrap_or(1);
    Some(round.checked_sub(1)? * 26 + usize::from(letter as u8 - b'A'))
}

/// A gear's bore.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Bore {
    /// Fits its shaft.
    #[default]
    Auto,
    /// No hole.
    Solid,
    Diameter(f64),
}

/// A hub: its diameter (`None`: Auto) and how far it reaches each side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hub {
    pub diameter: Option<f64>,
    pub length_a: f64,
    pub length_b: f64,
    /// Its Ø worked out when none is given or drawn (the dialog's Hub Ø left on auto): 1.6 × the
    /// bore, or half the root Ø. Off, a hub with no Ø only gives a drawn hub circle its lengths.
    pub auto: bool,
}

/// One gear's own settings (GEAR_UI_SPEC §3, §6); `None`: Auto.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GearSettings {
    pub size_by: SizeBy,
    pub module: Option<f64>,
    pub teeth: Option<u32>,
    pub face: FaceSides,
    /// Degrees; 0 straight.
    pub helix: Option<f64>,
    /// `None`: opposite its mate.
    pub hand: Option<Hand>,
    /// V (herringbone) teeth.
    pub v: bool,
    pub profile_shift: Option<f64>,
    pub bore: Bore,
    pub hub: Option<Hub>,
    /// Ring gear: rim thickness (Auto 2.5 m).
    pub rim: Option<f64>,
    /// Worm: thread starts; a throated wheel.
    pub starts: Option<u32>,
    pub throated: bool,
    /// Spiral bevel / hypoid: degrees at mid-face.
    pub spiral_angle: Option<f64>,
    pub bevel_style: BevelStyle,
}

/// One gear item.
#[derive(Clone, Debug, PartialEq)]
pub struct GearItem {
    /// Its id as the engine names it (A, B, …).
    pub id: String,
    pub kind: ItemKind,
    /// The picked geometry, in pick order.
    pub things: Vec<Thing>,
    /// One per gear ([`GearItem::settings`] fills the rest from the first).
    pub gears: Vec<GearSettings>,
    /// Planetary: the number of planets (`None`: as many as fit, at most 3) and whether its
    /// carrier is generated (decision 9).
    pub planets: Option<u32>,
    pub carrier: bool,
    /// Rack: back height (`None`: Auto).
    pub back_height: Option<f64>,
    /// Shaft: diameter (`None`: Auto) and a keyway.
    pub shaft_diameter: Option<f64>,
    pub keyway: bool,
}

impl GearItem {
    pub fn new(id: &str, kind: ItemKind) -> GearItem {
        GearItem {
            id: id.to_string(),
            kind,
            things: Vec::new(),
            gears: Vec::new(),
            planets: None,
            carrier: true,
            back_height: None,
            shaft_diameter: None,
            keyway: false,
        }
    }
    /// With these things.
    pub fn with(mut self, things: Vec<Thing>) -> GearItem {
        self.things = things;
        self
    }
    /// Gear `gear`'s settings: its own, else the first gear's (with no teeth), else defaults.
    pub fn settings(&self, gear: usize) -> GearSettings {
        match self.gears.get(gear) {
            Some(s) => *s,
            None => self.gears.first().map(|s| GearSettings { teeth: None, ..*s }).unwrap_or_default(),
        }
    }
    /// The gears it makes (see [`gear_count`]).
    pub fn gear_count(&self) -> usize {
        gear_count(self.kind, &self.things)
    }
    /// "Gear A" (as the dialog, the timeline and the bodies name it).
    pub fn who(&self) -> String {
        format!("Gear {}", self.id)
    }
}

/// A gear: its item (index into the items) and its place among the item's gears.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GearRef {
    pub item: usize,
    pub gear: usize,
}

impl GearRef {
    pub fn new(item: usize, gear: usize) -> GearRef {
        GearRef { item, gear }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkKind {
    /// The two gears' teeth mesh.
    Mesh,
    /// They share a shaft and turn together.
    Shaft,
}

impl LinkKind {
    pub fn name(self) -> &'static str {
        match self {
            LinkKind::Mesh => "mesh",
            LinkKind::Shaft => "shaft",
        }
    }
    pub fn parse(s: &str) -> Option<LinkKind> {
        match s {
            "mesh" => Some(LinkKind::Mesh),
            "shaft" => Some(LinkKind::Shaft),
            _ => None,
        }
    }
}

/// A graph link between two gears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemLink {
    pub a: GearRef,
    pub b: GearRef,
    pub kind: LinkKind,
}

impl ItemLink {
    /// Whether it joins `a` and `b` (either way round) with `kind`.
    pub fn joins(&self, a: GearRef, b: GearRef, kind: LinkKind) -> bool {
        self.kind == kind && ((self.a == a && self.b == b) || (self.a == b && self.b == a))
    }
}

/// Items whose gears share one module (`None`: Auto, one for all) and, optionally, the rest of
/// [`BlockSettings`] ([`crate::blocks`]: a member's own value wins, else the block's).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModuleBlock {
    pub id: String,
    pub module: Option<f64>,
    /// Indices into the items.
    pub members: Vec<usize>,
    pub settings: BlockSettings,
}

/// What a block gives every gear in it besides its module (each `None`: the gear's own, or
/// Auto). Lengths in mm, angles in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockSettings {
    /// A 45° tooth chamfer on every member gear that takes one (over the set's).
    pub tooth_chamfer: Option<f64>,
    pub pressure_angle: Option<f64>,
    /// The helix angle its gears on Auto take: on spur-type gears their helix, on a straight
    /// bevel pair its spiral ([`crate::blocks::block_settings`]).
    pub helix: Option<f64>,
    /// With [`BlockSettings::helix`]: V (herringbone) teeth; a bevel pair's spiral changes hand
    /// at mid-face (chevron).
    pub herringbone: bool,
    /// The whole face width, half each side of the sketch plane.
    pub face_width: Option<f64>,
    pub backlash: Option<f64>,
    /// The block's Ø, used as [`BlockSettings::shaft`] says: the shafts made, or the bores.
    pub shaft_diameter: Option<f64>,
    pub shaft: ShaftMode,
}

/// What a block's Ø makes. A **shaft** is a solid rod along an axis; a **bore** is the hole
/// through a gear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShaftMode {
    /// A shaft of the Ø along each axis the block's gears are on, each gear on it one body with
    /// it (no bore: they are one solid) ([`crate::blocks::block_shafts`]).
    #[default]
    Make,
    /// Every gear of the block bored to the Ø; no shaft is made (the rod is the user's own).
    BoreOnly,
}

/// The gear that is turned, and optionally how fast and how hard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drive {
    pub item: usize,
    pub gear: usize,
    pub rpm: Option<f64>,
    pub torque: Option<f64>,
}

/// The reply per gear (GEAR_UI_SPEC §6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GearReply {
    pub item: usize,
    pub gear: usize,
    pub teeth: u32,
    pub module: f64,
    pub pitch_d: f64,
    /// Its speed over the driver's.
    pub ratio: Option<f64>,
    pub rpm: Option<f64>,
    pub torque: Option<f64>,
    pub contact_ratio: Option<f64>,
    pub profile_shift: Option<f64>,
    pub warnings: Vec<String>,
}

/// The reply per link (GEAR_UI_SPEC §6).
#[derive(Clone, Debug, PartialEq)]
pub struct LinkReply {
    pub a: GearRef,
    pub b: GearRef,
    pub kind: LinkKind,
    pub ratio: Option<f64>,
    pub error: Option<String>,
    pub hint: Option<String>,
}

/// A link the drawing suggests (drawn dashed until accepted).
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub a: GearRef,
    pub b: GearRef,
    pub kind: LinkKind,
    /// Why, for the hover.
    pub why: String,
}

impl Suggestion {
    pub fn link(&self) -> ItemLink {
        ItemLink { a: self.a, b: self.b, kind: self.kind }
    }
}

// ---------------------------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------------------------

/// What a slot takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Circle,
    Line,
    Point,
}

impl SlotKind {
    pub fn name(self) -> &'static str {
        match self {
            SlotKind::Circle => "pitch circle",
            SlotKind::Line => "shaft line",
            SlotKind::Point => "bearing point",
        }
    }
}

/// One slot of an item type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub kind: SlotKind,
    /// Its component name in `gear.create` (pitch, pitch2, axis, axis2, bearing, sun, ring,
    /// planet …); a repeating slot numbers on (pitch3, planet2, bearing2 …).
    pub name: &'static str,
    /// What it is, for the status line ("wheel circle").
    pub what: &'static str,
    pub required: bool,
    /// It takes any number more (the last slot of a compound, planetary or shaft).
    pub repeats: bool,
}

const fn slot(kind: SlotKind, name: &'static str, what: &'static str, required: bool, repeats: bool) -> Slot {
    Slot { kind, name, what, required, repeats }
}

use SlotKind::{Circle as C, Line as L, Point as P};

/// A gear's optional bearing points: on its shaft line, for the housing cutout's bearing seats.
const SPUR: &[Slot] =
    &[slot(C, "pitch", "pitch circle", true, false), slot(L, "axis", "shaft line", false, false), slot(P, "bearing", "bearing point", false, true)];
const INTERNAL: &[Slot] = &[slot(C, "pitch", "ring's pitch circle", true, false), slot(C, "pitch2", "pinion's pitch circle", false, false)];
const RACK: &[Slot] = &[slot(L, "axis", "rack's pitch line", true, false), slot(C, "pitch", "pinion's pitch circle", false, false)];
const COMPOUND: &[Slot] = &[
    slot(L, "axis", "shaft line", true, false),
    slot(C, "pitch", "pitch circle", true, false),
    slot(C, "pitch2", "pitch circle", true, true),
    slot(P, "bearing", "bearing point", false, true),
];
const PLANETARY: &[Slot] = &[
    slot(C, "sun", "sun's pitch circle", true, false),
    slot(C, "ring", "ring's pitch circle", true, false),
    slot(C, "planet", "planet's pitch circle", false, true),
];
const BEVEL_CIRCLES: &[Slot] = &[slot(C, "pitch", "pitch circle 1", true, false), slot(C, "pitch2", "pitch circle 2", true, false)];
const BEVEL_LINES: &[Slot] =
    &[slot(L, "axis", "axis line 1", true, false), slot(L, "axis2", "axis line 2", true, false), slot(C, "pitch", "pitch circle", true, false)];
/// A bevel pair from one side view: the apex where the shafts meet, the two shaft lines from it,
/// and optionally the contact line between them (its angle sets the ratio, its length the cone
/// distance; without it the gears' teeth and module do).
const BEVEL_SIDE: &[Slot] = &[
    slot(P, "apex", "apex point (where the shafts meet)", true, false),
    slot(L, "axis", "shaft line 1", true, false),
    slot(L, "axis2", "shaft line 2", true, false),
    slot(L, "contact", "contact line", false, false),
];
/// A side view by its two shaft lines alone, meeting at a common end: that end is the apex.
const BEVEL_MEET: &[Slot] =
    &[slot(L, "axis", "shaft line 1", true, false), slot(L, "axis2", "shaft line 2", true, false), slot(L, "contact", "contact line", false, false)];
const HYPOID: &[Slot] = &[slot(C, "pitch", "ring's pitch circle", true, false), slot(L, "axis2", "pinion's axis line", true, false)];
const WORM: &[Slot] = &[slot(C, "pitch", "wheel's pitch circle", true, false), slot(L, "axis2", "worm's axis line", true, false)];
const CROSSED: &[Slot] = &[slot(C, "pitch", "pitch circle 1", true, false), slot(C, "pitch2", "pitch circle 2", true, false)];
const SHAFT: &[Slot] = &[slot(L, "axis", "shaft line", true, false), slot(P, "bearing", "bearing point", false, true)];

/// An item type's slot layouts: the first that fits the picks is used. A bevel pair has two:
/// two pitch circles, or two axis lines and one pitch circle.
pub fn slots(kind: ItemKind) -> &'static [&'static [Slot]] {
    match kind {
        ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone => &[SPUR],
        ItemKind::Internal => &[INTERNAL],
        ItemKind::Rack => &[RACK],
        ItemKind::Compound => &[COMPOUND],
        ItemKind::Planetary => &[PLANETARY],
        ItemKind::Bevel => &[BEVEL_CIRCLES, BEVEL_LINES, BEVEL_SIDE, BEVEL_MEET],
        ItemKind::Hypoid => &[HYPOID],
        ItemKind::Worm => &[WORM],
        ItemKind::CrossedHelical => &[CROSSED],
        ItemKind::Shaft => &[SHAFT],
    }
}

/// Which slot a thing fills: the layout (index into [`slots`]), the slot in it, and its
/// component name (numbered on for a repeating slot).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotIndex {
    pub layout: usize,
    pub slot: usize,
    pub name: String,
}

fn numbered(name: &str, n: usize) -> String {
    if n == 0 {
        return name.to_string();
    }
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let base: usize = name[stem.len()..].parse().unwrap_or(1);
    format!("{stem}{}", base + n)
}

/// Fills one layout with `things` in order (each into the next open slot of its kind, extras
/// into a repeating slot of their kind); `None` when they don't fit.
fn fill(layout: usize, list: &[Slot], things: &[Thing]) -> Option<Vec<SlotIndex>> {
    let mut used = vec![0usize; list.len()];
    let mut out = Vec::new();
    for t in things {
        let k = t.slot_kind();
        let s =
            list.iter().enumerate().position(|(i, s)| s.kind == k && used[i] == 0).or_else(|| list.iter().rposition(|s| s.kind == k && s.repeats))?;
        out.push(SlotIndex { layout, slot: s, name: numbered(list[s].name, used[s]) });
        used[s] += 1;
    }
    Some(out)
}

/// Every thing's slot (role-based for a ring gear and a planetary set: the larger of the
/// first two circles is the ring), or why they don't fit the type.
/// The order the layouts of `kind` are tried in for `things`: as listed, except that a bevel
/// pair's two shaft lines meeting at a common end (no circle, no point) are a side view first.
fn layout_order(kind: ItemKind, things: &[Thing]) -> Vec<usize> {
    let n = slots(kind).len();
    if kind == ItemKind::Bevel && circles(things).is_empty() && points(things).is_empty() && lines_meet(things).is_some() {
        return std::iter::once(n - 1).chain(0..n - 1).collect();
    }
    (0..n).collect()
}

/// The first layout (in [`layout_order`]) the things fill: its index and the slots.
fn first_fit(kind: ItemKind, things: &[Thing]) -> Option<(usize, Vec<SlotIndex>)> {
    let layouts = slots(kind);
    layout_order(kind, things).into_iter().find_map(|i| fill(i, layouts[i], things).map(|f| (i, f)))
}

/// Where the first two lines meet, end to end (within the tolerance): a side view's apex.
fn lines_meet(things: &[Thing]) -> Option<V> {
    let (a, b) = (line_ends(things, 0)?, line_ends(things, 1)?);
    let scale = [a.0, a.1, b.0, b.1].iter().map(|p| len(*p)).fold(1.0f64, f64::max).max(len(sub(a.1, a.0))).max(len(sub(b.1, b.0)));
    [a.0, a.1].into_iter().find_map(|p| [b.0, b.1].into_iter().find(|q| len(sub(p, *q)) <= tol(scale)).map(|_| p))
}

/// A side view's apex: the point picked, else where its two shaft lines meet.
fn side_apex(things: &[Thing]) -> Option<V> {
    points(things).first().copied().or_else(|| lines_meet(things))
}

pub fn assign(kind: ItemKind, things: &[Thing]) -> Result<Vec<SlotIndex>, String> {
    let Some((_, mut out)) = first_fit(kind, things) else {
        return Err(no_room(kind, things));
    };
    let circles: Vec<usize> = (0..things.len()).filter(|i| matches!(things[*i], Thing::Circle { .. })).collect();
    if matches!(kind, ItemKind::Internal | ItemKind::Planetary)
        && let [i, j, ..] = circles[..]
    {
        // The larger is the ring, whichever was picked first.
        let (big, small) = if radius(&things[j]) > radius(&things[i]) { (j, i) } else { (i, j) };
        let (ring, other) = if kind == ItemKind::Internal { ((0, "pitch"), (1, "pitch2")) } else { ((1, "ring"), (0, "sun")) };
        (out[big].slot, out[big].name) = (ring.0, ring.1.into());
        (out[small].slot, out[small].name) = (other.0, other.1.into());
    }
    Ok(out)
}

fn radius(t: &Thing) -> f64 {
    match t {
        Thing::Circle { radius, .. } => *radius,
        _ => 0.0,
    }
}

/// Why the last of `things` has no slot.
fn no_room(kind: ItemKind, things: &[Thing]) -> String {
    let Some((last, before)) = things.split_last() else { return String::new() };
    let got = last.slot_kind();
    let label = kind.label();
    // What the picks so far still need, in the first layout they fit.
    let layouts = slots(kind);
    let fitting = first_fit(kind, before).map(|(i, f)| (layouts[i], f));
    let alt = if layouts.len() > 1 {
        " (or remove a pick to use another layout: two pitch circles; two axis lines and one pitch circle; or a side view: the apex point, two shaft lines and an optional contact line)"
    } else {
        ""
    };
    if let Some((list, filled)) = fitting {
        let taken: Vec<usize> = filled.iter().map(|s| s.slot).collect();
        if let Some(want) = list.iter().enumerate().find(|(i, s)| s.required && !taken.contains(i)) {
            return format!("this {label} needs a {}, not a {}: pick a {} next{alt}", want.1.what, got.name(), want.1.what);
        }
    }
    let what = match got {
        SlotKind::Point => "a bearing point goes on a shaft line: pick the gear's shaft line first, or make a Shaft gear".to_string(),
        _ => "press New gear for it".to_string(),
    };
    format!("this {label} has no room for another {}: {what}{alt}", got.name())
}

/// The gears an item of `kind` with `things` makes: one per circle picked for a gear or a
/// compound; two for a pair (and a ring gear or rack with its pinion); sun, ring and planet
/// for a planetary set (its planets are one gear, copied round); none for a shaft.
pub fn gear_count(kind: ItemKind, things: &[Thing]) -> usize {
    let circles = things.iter().filter(|t| matches!(t, Thing::Circle { .. })).count();
    match kind {
        ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone => 1,
        ItemKind::Internal => 1 + usize::from(circles >= 2),
        ItemKind::Rack => 1 + usize::from(circles >= 1),
        ItemKind::Compound => circles.max(2),
        ItemKind::Planetary => 3,
        ItemKind::Bevel | ItemKind::Hypoid | ItemKind::Worm | ItemKind::CrossedHelical => 2,
        ItemKind::Shaft => 0,
    }
}

/// The next slot the picks still need ("pitch circle 2"), `None` when complete.
pub fn next_slot(kind: ItemKind, things: &[Thing]) -> Option<&'static str> {
    let layouts = slots(kind);
    let (li, filled) = first_fit(kind, things)?;
    let taken: Vec<usize> = filled.iter().map(|s| s.slot).collect();
    layouts[li].iter().enumerate().find(|(i, s)| s.required && !taken.contains(i)).map(|(_, s)| s.what)
}

/// Whether `thing` can be picked into an item of `kind` holding `existing`: its slot, or why
/// not (what is wrong and what to do).
pub fn validate_slot(kind: ItemKind, existing: &[Thing], thing: &Thing) -> Result<SlotIndex, String> {
    if let Some(e) = thing.degenerate() {
        return Err(e.into());
    }
    if existing.iter().any(|t| t.same(thing)) {
        return Err("already picked into this item: pick another".into());
    }
    if let Thing::Line { construction: true, .. } = thing
        && matches!(kind, ItemKind::Shaft | ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone | ItemKind::Compound)
    {
        return Err("a construction line is a helper, not a shaft: pick a solid line, or turn this one off construction".into());
    }
    let mut all = existing.to_vec();
    all.push(thing.clone());
    let slots_now = assign(kind, &all)?;
    check_geometry(kind, &all)?;
    Ok(slots_now.last().cloned().unwrap_or(SlotIndex { layout: 0, slot: 0, name: String::new() }))
}

/// Whether an item is complete and its geometry fits its type.
pub fn validate_item(item: &GearItem) -> Result<(), String> {
    for t in &item.things {
        if let Some(e) = t.degenerate() {
            return Err(format!("{}: {e}", item.who()));
        }
    }
    assign(item.kind, &item.things).map_err(|e| format!("{}: {e}", item.who()))?;
    if let Some(want) = next_slot(item.kind, &item.things) {
        return Err(format!("{} still needs a {want}: click one in the view", item.who()));
    }
    check_geometry(item.kind, &item.things).map_err(|e| format!("{}: {e}", item.who()))?;
    if item.kind == ItemKind::Bevel && circles(&item.things).is_empty() && gear_geoms(item).len() < 2 {
        return Err(format!(
            "{}: a side view without a contact line takes its sizes from the gears: give both gears their teeth (Size by Teeth) and a module, or draw the contact line from the apex",
            item.who()
        ));
    }
    Ok(())
}

struct Circ {
    c: V,
    n: V,
    r: f64,
}

struct Ln {
    a: V,
    u: V,
}

fn circles(things: &[Thing]) -> Vec<Circ> {
    things
        .iter()
        .filter_map(|t| match t {
            Thing::Circle { centre, normal, radius, .. } => Some(Circ { c: *centre, n: unit(*normal)?, r: *radius }),
            _ => None,
        })
        .collect()
}

fn lines(things: &[Thing]) -> Vec<Ln> {
    things
        .iter()
        .filter_map(|t| match t {
            Thing::Line { a, b, .. } => Some(Ln { a: *a, u: unit(sub(*b, *a))? }),
            _ => None,
        })
        .collect()
}

fn points(things: &[Thing]) -> Vec<V> {
    things
        .iter()
        .filter_map(|t| match t {
            Thing::Point { p, .. } => Some(*p),
            _ => None,
        })
        .collect()
}

/// A circle on a shaft line: its normal along the line, its centre on it.
fn circle_on_line(c: &Circ, l: &Ln, what: &str) -> Result<(), String> {
    if !parallel(c.n, l.u) {
        let deg = len(cross(c.n, l.u)).asin().to_degrees();
        return Err(format!("the {what} is tilted {}° to the shaft line: draw it in a sketch plane square to the line", num(deg)));
    }
    let d = dist_to_line(c.c, l.a, l.u);
    if d > tol(c.r) {
        return Err(format!("the {what}'s centre is {} mm off the shaft line: put its centre on the line (Coincident)", num(d)));
    }
    Ok(())
}

/// The axial offset between two circles of parallel axes (along the first's normal).
fn axial(a: &Circ, b: &Circ) -> f64 {
    dot(sub(b.c, a.c), a.n)
}

fn check_geometry(kind: ItemKind, things: &[Thing]) -> Result<(), String> {
    let cs = circles(things);
    let ls = lines(things);
    let ps = points(things);
    match kind {
        ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone => {
            if let (Some(c), Some(l)) = (cs.first(), ls.first()) {
                circle_on_line(c, l, "pitch circle")?;
            }
            bearings_on_shaft(&ls, &ps)?;
        }
        ItemKind::Internal => {
            if let [a, b, ..] = &cs[..] {
                let (ring, pinion) = if b.r > a.r { (b, a) } else { (a, b) };
                if !parallel(ring.n, pinion.n) {
                    return Err("the pinion's axis is not parallel to the ring's: draw both circles in one sketch plane".into());
                }
                let gap = axial(ring, pinion).abs();
                if gap > tol(pinion.r) {
                    return Err(format!("the pinion is {} mm off the ring's plane: draw it in the ring's sketch plane", num(gap)));
                }
                if (ring.r - pinion.r).abs() <= tol(pinion.r) {
                    return Err("the two circles are the same size: a pinion inside a ring is smaller than it".into());
                }
                let d = len(sub(pinion.c, ring.c));
                let want = ring.r - pinion.r;
                if d + pinion.r > ring.r + tol(pinion.r) {
                    return Err(format!(
                        "the pinion is not inside the ring: move its centre to {} mm from the ring's centre (ring radius − pinion radius)",
                        num(want)
                    ));
                }
                if (d - want).abs() > tol(pinion.r) {
                    return Err(format!(
                        "the pinion doesn't touch the ring: its centre is {} mm from the ring's, it must be {} mm (ring radius − pinion radius); make it Tangent",
                        num(d),
                        num(want)
                    ));
                }
            }
        }
        ItemKind::Rack => {
            if let (Some(c), Some(l)) = (cs.first(), ls.first()) {
                rack_tangent(l, c)?;
            }
        }
        ItemKind::Compound => {
            bearings_on_shaft(&ls, &ps)?;
            if let Some(l) = ls.first() {
                for (k, c) in cs.iter().enumerate() {
                    circle_on_line(c, l, &format!("pitch circle {}", k + 1))?;
                }
            } else if let Some(first) = cs.first() {
                for c in &cs[1..] {
                    if !parallel(first.n, c.n) {
                        return Err(
                            "this circle is not parallel to the first: a compound's circles lie in parallel sketch planes, square to its shaft"
                                .into(),
                        );
                    }
                    let d = dist_to_line(c.c, first.c, first.n);
                    if d > tol(c.r.min(first.r)) {
                        return Err(format!(
                            "this circle's centre is {} mm off the first circle's axis: a compound's gears share one axis; put the centres on one line",
                            num(d)
                        ));
                    }
                }
            }
            for (i, a) in cs.iter().enumerate() {
                for b in &cs[i + 1..] {
                    if axial(a, b).abs() <= tol(a.r.min(b.r)) {
                        return Err("two circles sit at the same place on the shaft: a compound's gears lie in different sketch planes along it; draw this one in another plane".into());
                    }
                }
            }
        }
        ItemKind::Planetary => planetary_geometry(&cs)?,
        // A side view, in any order: checked whole once both shaft lines are there, else what is
        // there so far (an apex picked first is fine).
        ItemKind::Bevel if cs.is_empty() && !ps.is_empty() && ls.len() >= 2 => {
            bevel_side(things)?;
        }
        ItemKind::Bevel if cs.is_empty() && !ps.is_empty() => bevel_side_so_far(things)?,
        // Two shaft lines meeting end to end: a side view, its apex where they meet.
        ItemKind::Bevel if cs.is_empty() && lines_meet(things).is_some() => {
            bevel_side(things)?;
        }
        ItemKind::Bevel => bevel_geometry(&cs, &ls)?,
        ItemKind::Hypoid => {
            if let (Some(c), Some(l)) = (cs.first(), ls.first()) {
                match lie(c.c, c.n, l.a, l.u, tol(c.r)) {
                    Lie::Coaxial | Lie::Parallel { .. } => return Err("the pinion's axis is parallel to the ring's: a hypoid pinion's axis crosses the ring's at an angle (usually 90°)".into()),
                    Lie::Meet { .. } => return Err("the pinion's axis meets the ring's: that is a bevel pair; a hypoid pinion is offset, so move its line off the ring's axis (less than the ring's radius)".into()),
                    Lie::Skew { offset, .. } if offset >= c.r - tol(c.r) => {
                        return Err(format!(
                            "the pinion's axis is {} mm from the ring's, not less than the ring's pitch radius {} mm: move the line nearer the ring's axis",
                            num(offset),
                            num(c.r)
                        ));
                    }
                    Lie::Skew { .. } => {}
                }
            }
        }
        ItemKind::Worm => {
            if let (Some(c), Some(l)) = (cs.first(), ls.first()) {
                worm_geometry(c, l)?;
            }
        }
        ItemKind::CrossedHelical => {
            if let [a, b, ..] = &cs[..] {
                match lie(a.c, a.n, b.c, b.n, tol(a.r.min(b.r))) {
                    Lie::Coaxial | Lie::Parallel { .. } => {
                        return Err("these circles' axes are parallel: make two Spur/Helical items and link them by Mesh".into());
                    }
                    Lie::Meet { .. } => return Err("these circles' axes meet: that is a Bevel Pair; crossed helical shafts pass each other".into()),
                    Lie::Skew { offset, foot1, foot2, .. } => {
                        let t = tol(a.r.min(b.r));
                        if (offset - a.r - b.r).abs() > t {
                            return Err(format!(
                                "the circles don't touch: their axes are {} mm apart and their radii add up to {} mm; move one circle's shaft or resize a circle",
                                num(offset),
                                num(a.r + b.r)
                            ));
                        }
                        for (k, (c, f)) in [(a, foot1), (b, foot2)].into_iter().enumerate() {
                            let off = len(sub(c.c, f));
                            if off > t {
                                return Err(format!(
                                    "circle {} is {} mm along its axis from where the shafts pass closest: move it there",
                                    k + 1,
                                    num(off)
                                ));
                            }
                        }
                    }
                }
            }
        }
        ItemKind::Shaft => bearings_on_shaft(&ls, &ps)?,
    }
    Ok(())
}

/// Bearing points (a Shaft item's, or a gear's on its own shaft line) sit on the shaft line: the
/// line picked first, each point on it.
fn bearings_on_shaft(ls: &[Ln], ps: &[V]) -> Result<(), String> {
    if ps.is_empty() {
        return Ok(());
    }
    let Some(l) = ls.first() else { return Err("pick the shaft line first, then the bearing points on it".into()) };
    for p in ps {
        let d = dist_to_line(*p, l.a, l.u);
        if d > tol(1.0) {
            return Err(format!(
                "this point is {} mm off the shaft line: a bearing sits on the shaft; put the point on the line (Coincident)",
                num(d)
            ));
        }
    }
    Ok(())
}

/// Every bearing point picked (a Shaft item's, a gear's on its shaft line), in the world: the
/// housing cutout's bearing seats ([`crate::clearance::Seat::Point`]), read where they are now.
pub fn bearing_points(items: &[GearItem]) -> Vec<V> {
    items
        .iter()
        .filter(|it| matches!(it.kind, ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone | ItemKind::Compound | ItemKind::Shaft))
        .flat_map(|it| points(&it.things))
        .collect()
}

fn rack_tangent(l: &Ln, c: &Circ) -> Result<(), String> {
    if dot(l.u, c.n).abs() > ANGLE_TOL {
        return Err("the rack's pitch line is not in the pinion's plane: draw it in the pinion's sketch".into());
    }
    let off = dot(sub(l.a, c.c), c.n).abs();
    if off > tol(c.r) {
        return Err(format!("the rack's pitch line is {} mm off the pinion's plane: draw it in the pinion's sketch", num(off)));
    }
    let d = dist_to_line(c.c, l.a, l.u);
    if (d - c.r).abs() > tol(c.r) {
        return Err(format!(
            "the rack's pitch line doesn't touch the pinion's pitch circle: it is {} mm from the centre, the radius is {} mm; make it Tangent",
            num(d),
            num(c.r)
        ));
    }
    Ok(())
}

fn planetary_geometry(cs: &[Circ]) -> Result<(), String> {
    let [a, b, planets @ ..] = cs else { return Ok(()) };
    if !parallel(a.n, b.n) {
        return Err("the sun and ring are not parallel: draw them in one sketch plane".into());
    }
    let t = tol(a.r.min(b.r));
    let d = len(sub(a.c, b.c));
    if d > t {
        return Err(format!(
            "the sun and ring are not concentric: their centres are {} mm apart; put both centres on one point (Concentric)",
            num(d)
        ));
    }
    if (a.r - b.r).abs() <= t {
        return Err("the sun and ring are the same size: the ring is the larger circle, the sun the smaller".into());
    }
    let (sun, ring) = if a.r < b.r { (a, b) } else { (b, a) };
    let rp = (ring.r - sun.r) / 2.0;
    for p in planets {
        if !parallel(p.n, sun.n) || axial(sun, p).abs() > t {
            return Err("a planet lies in the sun's sketch plane: draw it there".into());
        }
        if (p.r - rp).abs() > t {
            return Err(format!("this planet's radius is {} mm: between this sun and ring it must be {} mm ((ring − sun) / 2)", num(p.r), num(rp)));
        }
        let dc = len(sub(p.c, sun.c));
        if (dc - (sun.r + rp)).abs() > t {
            return Err(format!(
                "this planet doesn't touch the sun and ring: its centre is {} mm from the sun's, it must be {} mm; make it Tangent to both",
                num(dc),
                num(sun.r + rp)
            ));
        }
    }
    Ok(())
}

/// The cone distance of a circle around an axis through `apex`.
fn cone(c: &Circ, apex: V) -> f64 {
    (len(sub(c.c, apex)).powi(2) + c.r * c.r).sqrt()
}

fn bevel_geometry(cs: &[Circ], ls: &[Ln]) -> Result<(), String> {
    let axes_meet = |o1: V, u1: V, o2: V, u2: V, t: f64, what: &str| -> Result<V, String> {
        match lie(o1, u1, o2, u2, t) {
            Lie::Coaxial | Lie::Parallel { .. } => {
                Err(format!("{what} are parallel: a bevel pair's shafts meet at an angle (parallel shafts are a spur pair)"))
            }
            Lie::Skew { offset, .. } => Err(format!(
                "{what} don't meet: they pass {} mm apart; make them meet, or use a Hypoid or Crossed Helical pair for offset shafts",
                num(offset)
            )),
            Lie::Meet { apex, .. } => Ok(apex),
        }
    };
    match (cs, ls) {
        ([a, b, ..], []) => {
            let t = tol(a.r.min(b.r));
            let apex = axes_meet(a.c, a.n, b.c, b.n, t, "these circles' axes")?;
            let (r1, r2) = (cone(a, apex), cone(b, apex));
            if (r1 - r2).abs() > t {
                return Err(format!(
                    "the pitch circles don't touch: a bevel pair's circles meet at one point, the same distance from where the shafts meet ({} and {} mm); move or resize one",
                    num(r1),
                    num(r2)
                ));
            }
            for c in [a, b] {
                if len(sub(c.c, apex)) <= t {
                    return Err("a circle is centred where the shafts meet: move it along its shaft".into());
                }
            }
        }
        (_, [l1, l2, ..]) => {
            let apex = axes_meet(l1.a, l1.u, l2.a, l2.u, tol(1.0), "the axis lines")?;
            if let Some(c) = cs.first() {
                let on = |l: &Ln| parallel(c.n, l.u) && dist_to_line(c.c, l.a, l.u) <= tol(c.r);
                if !on(l1) && !on(l2) {
                    return Err("the pitch circle is not on either axis line: draw it square to one of them, centred on it".into());
                }
                if len(sub(c.c, apex)) <= tol(c.r) {
                    return Err("the pitch circle is centred where the shafts meet: move it along its shaft".into());
                }
            }
        }
        ([c], [l]) => {
            if parallel(c.n, l.u) {
                if dist_to_line(c.c, l.a, l.u) > tol(c.r) {
                    return Err("this line is parallel to the circle's axis but not through its centre: pick the circle's own axis line, or the other gear's axis (meeting it at an angle)".into());
                }
            } else {
                axes_meet(c.c, c.n, l.a, l.u, tol(c.r), "the circle's axis and this line")?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn worm_geometry(c: &Circ, l: &Ln) -> Result<(), String> {
    let t = tol(c.r);
    match lie(c.c, c.n, l.a, l.u, t) {
        Lie::Coaxial | Lie::Parallel { .. } => {
            Err("the worm's axis is parallel to the wheel's: a worm crosses the wheel square to its axis, beside its pitch circle".into())
        }
        Lie::Meet { .. } => Err(format!(
            "the worm's axis passes through the wheel's axis: move it out to the wheel's radius plus the worm's pitch radius (more than {} mm from the wheel's axis)",
            num(c.r)
        )),
        Lie::Skew { offset, angle, foot1, .. } => {
            if (angle - 90.0).abs() > 0.5 {
                return Err(format!(
                    "the worm's axis crosses the wheel's at {}°: a worm crosses square (90°); draw its line in the wheel's plane, tangent-wise",
                    num(angle)
                ));
            }
            let off = len(sub(foot1, c.c));
            if off > t {
                return Err(format!("the worm's line is {} mm off the wheel's plane along its axis: draw it in the wheel's sketch plane", num(off)));
            }
            if offset <= c.r + t {
                return Err(format!(
                    "the worm's line is {} mm from the wheel's axis, inside the wheel's pitch radius {} mm: move it out to the wheel's radius plus the worm's pitch radius",
                    num(offset),
                    num(c.r)
                ));
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Module and face
// ---------------------------------------------------------------------------------------------

/// Where a gear's module comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleFrom {
    /// Its Module Block (index into the blocks) decides (decision 2), its module or Auto.
    Block(usize),
    /// Its own module.
    Own,
    /// Sized by teeth: the module follows from the circle (d / z).
    Teeth,
    /// Auto: a standard module giving the smallest gear ≥ 17 teeth.
    Auto,
}

/// A gear's module as decided (`module` `None`: worked out by the sizing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModuleChoice {
    pub module: Option<f64>,
    pub from: ModuleFrom,
}

/// The module of a gear of item `item` with `settings` (decision 2): its Module Block's wins
/// (a block with no module: one Auto module for all its members), else its own module when
/// sized by module, else none (from its teeth, or Auto).
pub fn resolve_module(blocks: &[ModuleBlock], item: usize, settings: &GearSettings) -> ModuleChoice {
    if let Some((k, b)) = blocks.iter().enumerate().find(|(_, b)| b.members.contains(&item)) {
        return ModuleChoice { module: b.module.filter(|m| *m > 0.0 && m.is_finite()), from: ModuleFrom::Block(k) };
    }
    match settings.size_by {
        SizeBy::Module => match settings.module.filter(|m| *m > 0.0 && m.is_finite()) {
            Some(m) => ModuleChoice { module: Some(m), from: ModuleFrom::Own },
            None => ModuleChoice { module: None, from: ModuleFrom::Auto },
        },
        SizeBy::Teeth if settings.teeth.is_some() => ModuleChoice { module: None, from: ModuleFrom::Teeth },
        SizeBy::Teeth => ModuleChoice { module: None, from: ModuleFrom::Auto },
    }
}

/// How far a gear's teeth reach on side A and side B of its sketch plane (decision 4), for
/// `module`: Symmetric a each side (Auto 5 m each: ≈ 10 m in all); A only / B only all on one
/// side (Auto 10 m); Custom a and b (both Auto: 5 m each; one given: the other 0). Negative
/// values count as 0.
pub fn face_extent(settings: &GearSettings, module: f64) -> (f64, f64) {
    let f = settings.face;
    let m = if module.is_finite() && module > 0.0 { module } else { 1.0 };
    let pos = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
    match f.preset {
        FacePreset::Symmetric => {
            let s = pos(f.a.or(f.b).unwrap_or(5.0 * m));
            (s, s)
        }
        FacePreset::AOnly => (pos(f.a.unwrap_or(10.0 * m)), 0.0),
        FacePreset::BOnly => (0.0, pos(f.b.unwrap_or(10.0 * m))),
        FacePreset::Custom => match (f.a, f.b) {
            (None, None) => (5.0 * m, 5.0 * m),
            (a, b) => (pos(a.unwrap_or(0.0)), pos(b.unwrap_or(0.0))),
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Gear geometry
// ---------------------------------------------------------------------------------------------

/// Where a gear is: its pitch circle's centre, its axis (side A, decision 4: the circle's
/// normal, or the shaft line's direction from its first point when one is picked), its pitch
/// radius (`None` where the drawing doesn't give it: a rack, a hypoid pinion), whether it is a
/// ring (teeth inside), and a rack's pitch line or a worm / pinion axis line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GearGeom {
    pub centre: V,
    pub axis: V,
    pub radius: Option<f64>,
    pub ring: bool,
    pub rack: bool,
    pub line: Option<(V, V)>,
}

impl GearGeom {
    fn circle(c: &Circ) -> GearGeom {
        GearGeom { centre: c.c, axis: c.n, radius: Some(c.r), ring: false, rack: false, line: None }
    }
}

fn line_ends(things: &[Thing], k: usize) -> Option<(V, V)> {
    things
        .iter()
        .filter_map(|t| match t {
            Thing::Line { a, b, .. } => Some((*a, *b)),
            _ => None,
        })
        .nth(k)
}

/// Whether no circle or line gives the gears of an item made of `things` their size: a bevel pair
/// drawn as a side view without a contact line. Its gears then take a module and their teeth both
/// (sized by their teeth, the module given as well); every other layout ties one to the other.
pub fn sized_by_settings(kind: ItemKind, things: &[Thing]) -> bool {
    kind == ItemKind::Bevel && circles(things).is_empty() && lines(things).len() == 2 && side_apex(things).is_some()
}

/// Each gear's place (one per [`gear_count`], fewer while the item is incomplete).
pub fn gear_geoms(item: &GearItem) -> Vec<GearGeom> {
    let cs = circles(&item.things);
    let ls = lines(&item.things);
    let along = |g: GearGeom, l: Option<&Ln>| match l {
        Some(l) if parallel(g.axis, l.u) => GearGeom { axis: l.u, ..g },
        _ => g,
    };
    match item.kind {
        ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone => {
            cs.first().map(|c| along(GearGeom::circle(c), ls.first())).into_iter().collect()
        }
        ItemKind::Internal => {
            let mut g: Vec<GearGeom> = cs.iter().take(2).map(GearGeom::circle).collect();
            if g.len() == 2 && g[1].radius > g[0].radius {
                g.swap(0, 1);
            }
            if let Some(r) = g.first_mut() {
                r.ring = true;
            }
            g
        }
        ItemKind::Rack => {
            let Some(l) = ls.first() else { return Vec::new() };
            let (a, b) = line_ends(&item.things, 0).unwrap_or((l.a, l.a));
            let pinion = cs.first().map(GearGeom::circle);
            let centre = pinion.map_or(mul(add(a, b), 0.5), |p| foot(p.centre, l.a, l.u));
            let rack = GearGeom { centre, axis: l.u, radius: None, ring: false, rack: true, line: Some((a, b)) };
            std::iter::once(rack).chain(pinion).collect()
        }
        ItemKind::Compound => cs.iter().map(|c| along(GearGeom::circle(c), ls.first())).collect(),
        ItemKind::Planetary => {
            let [a, b, rest @ ..] = &cs[..] else { return cs.iter().map(GearGeom::circle).collect() };
            let (sun, ring) = if a.r < b.r { (a, b) } else { (b, a) };
            let rp = (ring.r - sun.r) / 2.0;
            let planet = match rest.first() {
                Some(p) => GearGeom::circle(p),
                None => {
                    let x = crate::axes::square_to(sun.n);
                    GearGeom { centre: add(sun.c, mul(x, sun.r + rp)), axis: sun.n, radius: Some(rp), ring: false, rack: false, line: None }
                }
            };
            vec![GearGeom::circle(sun), GearGeom { ring: true, ..GearGeom::circle(ring) }, planet]
        }
        ItemKind::Bevel if cs.is_empty() && side_apex(&item.things).is_some() => bevel_side_gears(item),
        ItemKind::Bevel => match (&cs[..], &ls[..]) {
            (_, [l1, l2, ..]) => {
                let Some(c) = cs.first() else { return Vec::new() };
                let (on, other, k) = if parallel(c.n, l1.u) && dist_to_line(c.c, l1.a, l1.u) <= tol(c.r) { (l1, l2, 1) } else { (l2, l1, 0) };
                let g1 = GearGeom::circle(&Circ { c: c.c, n: on.u, r: c.r });
                let ends = line_ends(&item.things, k).unwrap_or((other.a, other.a));
                match bevel_mate(c, other, ends) {
                    Some(g2) => vec![g1, g2],
                    None => vec![g1],
                }
            }
            _ => cs.iter().take(2).map(GearGeom::circle).collect(),
        },
        ItemKind::Hypoid | ItemKind::Worm => {
            let Some(c) = cs.first() else { return Vec::new() };
            let wheel = GearGeom::circle(c);
            let Some(l) = ls.first() else { return vec![wheel] };
            let ends = line_ends(&item.things, 0).unwrap_or((l.a, l.a));
            let (centre, offset) = match lie(c.c, c.n, l.a, l.u, tol(c.r)) {
                Lie::Skew { offset, foot2, .. } => (foot2, offset),
                _ => (foot(c.c, l.a, l.u), dist_to_line(c.c, l.a, l.u)),
            };
            let radius = (item.kind == ItemKind::Worm && offset > c.r).then_some(offset - c.r);
            vec![wheel, GearGeom { centre, axis: l.u, radius, ring: false, rack: false, line: Some(ends) }]
        }
        ItemKind::CrossedHelical => cs.iter().take(2).map(GearGeom::circle).collect(),
        ItemKind::Shaft => Vec::new(),
    }
}

/// A Shaft item's line as a place links can name (`{item, gear: 0}`): from its first end along
/// it, no radius, the line's ends. `None` for any other item, or a Shaft item with no line yet.
pub fn shaft_geom(item: &GearItem) -> Option<GearGeom> {
    if item.kind != ItemKind::Shaft {
        return None;
    }
    let (a, b) = line_ends(&item.things, 0)?;
    let u = unit(sub(b, a))?;
    Some(GearGeom { centre: a, axis: u, radius: None, ring: false, rack: false, line: Some((a, b)) })
}

/// Where the gear (or Shaft item, gear 0) a link names is: [`gear_geoms`], or a Shaft item's
/// [`shaft_geom`].
pub fn link_geom(items: &[GearItem], r: GearRef) -> Option<GearGeom> {
    let it = items.get(r.item)?;
    match it.kind {
        ItemKind::Shaft => shaft_geom(it).filter(|_| r.gear == 0),
        _ => gear_geoms(it).get(r.gear).copied(),
    }
}

/// Whether a gear at `g` lies on the shaft line `s` ([`shaft_geom`]): on its axis, its centre
/// within the line's ends (give or take its own radius).
pub fn on_shaft_line(s: &GearGeom, g: &GearGeom) -> bool {
    let Some((a, b)) = s.line else { return false };
    let t = tol(g.radius.unwrap_or(1.0));
    if !matches!(lie(s.centre, s.axis, g.centre, g.axis, t), Lie::Coaxial) {
        return false;
    }
    let at = dot(sub(g.centre, a), s.axis);
    let reach = g.radius.unwrap_or(0.0).max(t);
    at >= -reach && at <= len(sub(b, a)) + reach
}

/// A bevel pair drawn as a side view ([`BEVEL_SIDE`]), read: the apex, each shaft's direction
/// away from it (toward its line's far end) with the line's ends, and the contact line's
/// direction and length when one is drawn. Checks that both shaft lines run through the apex
/// at an angle and that the contact line starts at the apex between them.
struct SideView {
    apex: V,
    shafts: [(V, (V, V)); 2],
    contact: Option<(V, f64)>,
}

/// A side view picked so far (the apex and fewer than two lines): each line runs through the apex.
fn bevel_side_so_far(things: &[Thing]) -> Result<(), String> {
    let Some(apex) = points(things).first().copied() else { return Ok(()) };
    for k in 0..2 {
        let Some((a, b)) = line_ends(things, k) else { break };
        let (near, far) = if len(sub(a, apex)) <= len(sub(b, apex)) { (a, b) } else { (b, a) };
        let Some(u) = unit(sub(far, near)) else { return Err("a shaft line has no length".into()) };
        let off = dist_to_line(apex, near, u);
        if off > tol(len(sub(far, apex)).max(1.0)) {
            return Err(format!("this line doesn't run through the apex point ({} mm off): draw the shaft lines from the apex", num(off)));
        }
    }
    Ok(())
}

fn bevel_side(things: &[Thing]) -> Result<SideView, String> {
    let apex = side_apex(things).ok_or("pick the apex point, where the shafts meet")?;
    let ends: Vec<(V, V)> = (0..3).filter_map(|k| line_ends(things, k)).collect();
    if ends.len() < 2 {
        return Err("pick the two shaft lines from the apex".into());
    }
    // A line's direction away from the apex, its length from it, and how far it passes it.
    let away = |(a, b): (V, V)| -> Option<(V, f64, f64)> {
        let (near, far) = if len(sub(a, apex)) <= len(sub(b, apex)) { (a, b) } else { (b, a) };
        let u = unit(sub(far, near))?;
        Some((u, len(sub(far, apex)), dist_to_line(apex, near, u)))
    };
    let scale = ends.iter().map(|(a, b)| len(sub(*a, apex)).max(len(sub(*b, apex)))).fold(1.0f64, f64::max);
    let t = tol(scale);
    let mut shafts = [([0.0; 3], ([0.0; 3], [0.0; 3])); 2];
    for (k, e) in ends.iter().take(2).enumerate() {
        let (u, _, off) = away(*e).ok_or("a shaft line has no length")?;
        if off > t {
            return Err(format!(
                "shaft line {} doesn't run through the apex point ({} mm off): draw both shaft lines from the apex",
                k + 1,
                num(off)
            ));
        }
        shafts[k] = (u, *e);
    }
    let (u1, u2) = (shafts[0].0, shafts[1].0);
    if parallel(u1, u2) {
        return Err("the shaft lines are parallel: a bevel pair's shafts meet at an angle (parallel shafts are a spur pair)".into());
    }
    let contact = match ends.get(2) {
        Some(e) => {
            let (c, r, off) = away(*e).ok_or("the contact line has no length")?;
            let near = len(sub(e.0, apex)).min(len(sub(e.1, apex)));
            if off > t || near > t {
                return Err("the contact line must start at the apex point: draw it from the apex, between the two shaft lines".into());
            }
            let angle = |a: V, b: V| dot(a, b).clamp(-1.0, 1.0).acos();
            let (d1, d2, sigma) = (angle(u1, c), angle(u2, c), angle(u1, u2));
            if (d1 + d2 - sigma).abs() > 1e-3 || d1 < 1e-3 || d2 < 1e-3 {
                return Err("the contact line must lie between the two shaft lines, in their plane: its angle to each sets the ratio".into());
            }
            Some((c, r))
        }
        None => None,
    };
    Ok(SideView { apex, shafts, contact })
}

/// A side view with no contact line and no tooth counts set: each gear's teeth, and the module
/// when none is set.
pub const SIDE_VIEW_TEETH: u32 = 20;
pub const SIDE_VIEW_MODULE: f64 = 2.0;

/// The two bevel gears of a side view: the cone angles from the contact line (else from the
/// gears' tooth counts against the shaft angle), the cone distance from the contact line's
/// length (else from the module and teeth). Fewer than two while that can't be worked out.
fn bevel_side_gears(item: &GearItem) -> Vec<GearGeom> {
    let Ok(v) = bevel_side(&item.things) else { return Vec::new() };
    let [(u1, e1), (u2, e2)] = v.shafts;
    let angle = |a: V, b: V| dot(a, b).clamp(-1.0, 1.0).acos();
    let sigma = angle(u1, u2);
    let (d1, d2, cone) = match v.contact {
        Some((c, r)) => (angle(u1, c), angle(u2, c), r),
        None => {
            let (s1, s2) = (item.settings(0), item.settings(1));
            // Teeth and module not set yet: [`SIDE_VIEW_TEETH`] each at [`SIDE_VIEW_MODULE`], so
            // the pair builds at once (the dialog shows them; set them to size it).
            let (z1, z2) = (s1.teeth.filter(|z| *z > 0).unwrap_or(SIDE_VIEW_TEETH), s2.teeth.filter(|z| *z > 0).unwrap_or(SIDE_VIEW_TEETH));
            let m = s1.module.or(s2.module).filter(|m| *m > 0.0 && m.is_finite()).unwrap_or(SIDE_VIEW_MODULE);
            let d1 = sigma.sin().atan2(f64::from(z2) / f64::from(z1) + sigma.cos());
            let r1 = m * f64::from(z1) / 2.0;
            (d1, sigma - d1, r1 / d1.sin().max(1e-12))
        }
    };
    let gear = |u: V, d: f64, ends: (V, V)| GearGeom {
        centre: add(v.apex, mul(u, cone * d.cos())),
        axis: u,
        radius: Some(cone * d.sin()),
        ring: false,
        rack: false,
        line: Some(ends),
    };
    vec![gear(u1, d1, e1), gear(u2, d2, e2)]
}

/// A bevel gear's mate on axis line `other` (`ends` its picked ends): the circle around the
/// line through the pitch point of `c` that lies toward the line's picked side.
fn bevel_mate(c: &Circ, other: &Ln, ends: (V, V)) -> Option<GearGeom> {
    let Lie::Meet { apex, .. } = lie(c.c, c.n, other.a, other.u, tol(c.r)) else { return None };
    let mid = mul(add(ends.0, ends.1), 0.5);
    let side = if dot(sub(mid, apex), other.u) < 0.0 { -1.0 } else { 1.0 };
    let s2 = mul(other.u, side);
    let w = unit(sub(s2, mul(c.n, dot(s2, c.n))))?;
    let p = add(c.c, mul(w, c.r));
    let c2 = add(apex, mul(s2, dot(sub(p, apex), s2)));
    let r2 = len(sub(p, c2));
    (r2 > 1e-9).then_some(GearGeom { centre: c2, axis: other.u, radius: Some(r2), ring: false, rack: false, line: Some(ends) })
}

// ---------------------------------------------------------------------------------------------
// Suggestions, stages
// ---------------------------------------------------------------------------------------------

/// Links the drawing suggests between gears of different items (decision 3): circles touching
/// in one plane (outside, or inside a ring) and a rack line touching a circle → Mesh; gears on
/// one axis → Shaft. Links already in the graph are not removed here: see [`unlinked`].
pub fn suggest_links(items: &[GearItem]) -> Vec<Suggestion> {
    let geoms: Vec<Vec<GearGeom>> = items.iter().map(gear_geoms).collect();
    // Gears already joined on one shaft by the suggestions so far (a union–find by gear): a
    // third gear on a shaft is suggested to one of the others, not to each of them, so three
    // gears on one axis get two shaft links, not three.
    let at: Vec<(usize, usize)> = geoms.iter().enumerate().flat_map(|(i, g)| (0..g.len()).map(move |k| (i, k))).collect();
    let mut root: Vec<usize> = (0..at.len()).collect();
    let index = |i: usize, g: usize| at.iter().position(|p| *p == (i, g)).unwrap_or(0);
    let mut out = Vec::new();
    for (i, gi) in geoms.iter().enumerate() {
        for (j, gj) in geoms.iter().enumerate().skip(i + 1) {
            for (ga, a) in gi.iter().enumerate() {
                for (gb, b) in gj.iter().enumerate() {
                    if let Some((kind, why)) = suggest(a, b) {
                        if kind == LinkKind::Shaft {
                            let (x, y) = (find(&mut root, index(i, ga)), find(&mut root, index(j, gb)));
                            if x == y {
                                continue;
                            }
                            root[y] = x;
                        }
                        out.push(Suggestion { a: GearRef::new(i, ga), b: GearRef::new(j, gb), kind, why });
                    }
                }
            }
        }
    }
    // A Shaft item carries the gears on its line: a Shaft link to one gear of each set of gears
    // already on one shaft (the gears on its line are on one axis, so usually just the first).
    for (s, it) in items.iter().enumerate() {
        let Some(sg) = shaft_geom(it) else { continue };
        let mut joined: Vec<usize> = Vec::new();
        for (j, gj) in geoms.iter().enumerate() {
            for (gb, b) in gj.iter().enumerate() {
                if j == s || !on_shaft_line(&sg, b) {
                    continue;
                }
                let x = find(&mut root, index(j, gb));
                if joined.contains(&x) {
                    continue;
                }
                joined.push(x);
                let at = dot(sub(b.centre, sg.centre), sg.axis);
                out.push(Suggestion {
                    a: GearRef::new(s, 0),
                    b: GearRef::new(j, gb),
                    kind: LinkKind::Shaft,
                    why: format!("on the shaft line, {} mm along it", num(at)),
                });
            }
        }
    }
    out
}

/// The Shaft links a request's own links leave out (decision 3 for Shaft items): a Shaft item
/// that no link names carries the gears on its line, as [`suggest_links`] suggests. A request
/// with no links at all uses every suggestion ([`effective_links`]).
pub fn implicit_shaft_links(items: &[GearItem], links: &[ItemLink]) -> Vec<ItemLink> {
    let named = |i: usize| links.iter().any(|l| l.a.item == i || l.b.item == i);
    suggest_links(items)
        .iter()
        .filter(|s| {
            s.kind == LinkKind::Shaft && [s.a, s.b].iter().any(|r| items.get(r.item).is_some_and(|it| it.kind == ItemKind::Shaft) && !named(r.item))
        })
        .map(Suggestion::link)
        .collect()
}

/// A gear's details picked as circles (`gear.create` component slots `bore` and `hub`, beside
/// its item's slots): the slot name and what it is, for a checklist.
pub const DETAIL_SLOTS: [(&str, &str); 2] = [("bore", "bore circle"), ("hub", "hub circle")];

/// Whether an item's gears take a bore and a hub circle: the parallel-axis gears and a
/// compound's.
pub fn takes_details(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone | ItemKind::Compound)
}

/// Which of `item`'s gears circle `t` can be a bore or hub circle for: drawn concentric with
/// that gear's pitch circle (same centre and plane) and inside it. `None`: none of them.
pub fn detail_gear(item: &GearItem, t: &Thing) -> Option<usize> {
    let Thing::Circle { centre, normal, radius, .. } = t else { return None };
    if !takes_details(item.kind) {
        return None;
    }
    gear_geoms(item)
        .iter()
        .position(|g| g.radius.is_some_and(|r| !g.ring && *radius < r - tol(r) && len(sub(g.centre, *centre)) <= tol(r) && parallel(g.axis, *normal)))
}

/// The set gear `x` is in, of the shaft sets in `root` (a union–find).
fn find(root: &mut [usize], mut x: usize) -> usize {
    while root[x] != x {
        root[x] = root[root[x]];
        x = root[x];
    }
    x
}

/// The suggestions not already in `links`.
pub fn unlinked(suggestions: Vec<Suggestion>, links: &[ItemLink]) -> Vec<Suggestion> {
    suggestions.into_iter().filter(|s| !links.iter().any(|l| l.joins(s.a, s.b, s.kind))).collect()
}

fn suggest(a: &GearGeom, b: &GearGeom) -> Option<(LinkKind, String)> {
    if a.rack || b.rack {
        let (rack, other) = if a.rack { (a, b) } else { (b, a) };
        let (r, (p, q)) = (other.radius?, rack.line?);
        let l = Ln { a: p, u: unit(sub(q, p))? };
        // Touching within the drawn line, and not a ring.
        let at = dot(sub(other.centre, p), l.u);
        if other.ring || at < -tol(r) || at > len(sub(q, p)) + tol(r) {
            return None;
        }
        return rack_tangent(&l, &Circ { c: other.centre, n: other.axis, r })
            .is_ok()
            .then(|| (LinkKind::Mesh, "the rack's pitch line touches this pitch circle".to_string()));
    }
    // Gears of any kind on one axis line (a bevel's, a pinion's or a worm's axis too) share it.
    let t = tol([a.radius, b.radius].into_iter().flatten().reduce(f64::min).unwrap_or(1.0));
    if matches!(lie(a.centre, a.axis, b.centre, b.axis, t), Lie::Coaxial) {
        let gap = dot(sub(b.centre, a.centre), a.axis).abs();
        return (gap > t && !a.ring && !b.ring).then(|| (LinkKind::Shaft, format!("on one axis, {} mm apart along it", num(gap))));
    }
    let (ra, rb) = (a.radius?, b.radius?);
    let t = tol(ra.min(rb));
    if a.line.is_some() || b.line.is_some() {
        // A worm or pinion axis (no circle drawn): nothing to see.
        return None;
    }
    match lie(a.centre, a.axis, b.centre, b.axis, t) {
        Lie::Coaxial => None,
        Lie::Parallel { distance } => {
            let gap = dot(sub(b.centre, a.centre), a.axis).abs();
            if gap > t {
                return None;
            }
            if !a.ring && !b.ring && (distance - ra - rb).abs() <= t {
                return Some((LinkKind::Mesh, "the pitch circles touch".into()));
            }
            let inner = if a.ring && rb < ra { true } else { b.ring && ra < rb };
            (inner && (distance - (ra - rb).abs()).abs() <= t).then(|| (LinkKind::Mesh, "the pitch circle touches the ring from inside".to_string()))
        }
        _ => None,
    }
}

/// The links a request uses (decision 3): its own, or when it has none, the suggestions
/// (`true`: inferred).
pub fn effective_links(items: &[GearItem], links: &[ItemLink]) -> (Vec<ItemLink>, bool) {
    if links.is_empty() {
        let s: Vec<ItemLink> = suggest_links(items).iter().map(Suggestion::link).collect();
        (s, true)
    } else {
        // The request's own links in their order (the replies follow it), then a Shaft item's
        // that no link names.
        let mut all = links.to_vec();
        all.extend(implicit_shaft_links(items, links));
        (all, false)
    }
}

/// A stage's kind: which planner makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    /// External gears on parallel shafts (spur, helical, herringbone).
    Parallel,
    /// A gear inside a ring.
    Internal,
    Rack,
    /// Sun, ring and planet(s), with the carrier (decision 9).
    Planetary,
    /// Straight, spiral or chevron bevel ([`Stage::bevel_style`]).
    Bevel,
    Hypoid,
    Worm,
    CrossedHelical,
    /// Gears on one shaft turning together (a compound's gears, Shaft links): a rigid group.
    Shaft,
}

impl StageKind {
    /// The plan kind its planner makes ([`Stage::style`] tells spur, helical or herringbone).
    pub fn plan_kind(self) -> Option<Kind> {
        Some(match self {
            StageKind::Parallel => Kind::Pair,
            StageKind::Internal => Kind::Internal,
            StageKind::Rack => Kind::Rack,
            StageKind::Planetary => Kind::Planetary,
            StageKind::Bevel => Kind::Bevel,
            StageKind::Hypoid => Kind::Hypoid,
            StageKind::Worm => Kind::Worm,
            StageKind::CrossedHelical => Kind::CrossedHelical,
            StageKind::Shaft => return None,
        })
    }
}

/// Where a stage came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageFrom {
    /// An item's own pair or set (bevel, hypoid, worm, crossed helical, ring + pinion, rack +
    /// pinion, planetary).
    Item(usize),
    /// A Mesh link (index into the links used).
    Link(usize),
    /// A shaft cluster: these Shaft links (indices) and compound items.
    Cluster { links: Vec<usize>, items: Vec<usize> },
}

/// One stage of the drivetrain: a mesh (two gears), a planetary set (sun, ring, planet), or a
/// rigid group of gears on one shaft. A gear may be in several stages (decision 8).
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub kind: StageKind,
    /// Its gears in order: a mesh's two as linked (an item's pair in the item's order: ring /
    /// wheel first); a planetary set sun, ring, planet.
    pub gears: Vec<GearRef>,
    /// Their places, as [`gear_geoms`] (the axes come from the items' things).
    pub geom: Vec<GearGeom>,
    pub from: StageFrom,
    /// Tooth style for a parallel, internal or rack stage: Spur, Helical or Herringbone; for a
    /// bevel: Bevel or SpiralBevel (chevron: SpiralBevel with [`Stage::bevel_style`]).
    pub style: Kind,
    pub bevel_style: BevelStyle,
    /// A planetary set: its planets (`None`: Auto) and whether its carrier is generated.
    pub planets: Option<u32>,
    pub carrier: bool,
    /// What is wrong with it (what and what to do), for the link's reply.
    pub error: Option<String>,
}

/// A gear for people, as the dialog names it: "Gear A" for an item of one gear, "Gear W worm",
/// "Gear B ring" for a pair's named parts, "Gear C2" for a compound's second.
pub(crate) fn who(items: &[GearItem], r: GearRef) -> String {
    match items.get(r.item) {
        Some(it) if it.kind == ItemKind::Shaft => format!("Shaft {}", it.id),
        Some(it) if it.gear_count() <= 1 => it.who(),
        Some(it) => match part_name(it.kind, r.gear) {
            Some(part) => format!("{} {part}", it.who()),
            None => format!("{}{}", it.who(), r.gear + 1),
        },
        None => format!("Gear {}{}", item_id(r.item), r.gear + 1),
    }
}

/// The name of a pair's gear number (the dialog's): a worm item's "wheel" and "worm", a ring's
/// "ring" and "pinion"; None for a compound's layers (numbered).
pub fn part_name(kind: ItemKind, gear: usize) -> Option<&'static str> {
    let names: &[&str] = match kind {
        ItemKind::Planetary => &["sun", "ring", "planet"],
        ItemKind::Worm => &["wheel", "worm"],
        ItemKind::Hypoid => &["ring", "pinion"],
        ItemKind::Internal => &["ring", "pinion"],
        ItemKind::Rack => &["rack", "pinion"],
        _ => &[],
    };
    names.get(gear).copied()
}

/// Two gears on one circle: the same centre, radius and plane.
fn same_circle(a: &GearGeom, b: &GearGeom) -> bool {
    match (a.radius, b.radius) {
        (Some(r1), Some(r2)) => (r1 - r2).abs() <= tol(r1.max(r2)) && len(sub(a.centre, b.centre)) <= tol(r1.max(r2)),
        _ => false,
    }
}

/// The entity id of the circle a gear was picked from (its pitch circle), when it was one pick.
fn pitch_thing(items: &[GearItem], r: GearRef) -> Option<String> {
    let it = items.get(r.item)?;
    let g = gear_geoms(it).get(r.gear).copied()?;
    let rad = g.radius?;
    it.things.iter().find_map(|t| match t {
        Thing::Circle { centre, radius, .. } if (radius - rad).abs() <= tol(rad) && len(sub(*centre, g.centre)) <= tol(rad) => {
            let e = t.ids().entity.clone();
            (!e.is_empty()).then_some(e)
        }
        _ => None,
    })
}

fn style_of(items: &[GearItem], gears: &[GearRef]) -> Kind {
    let mut style = Kind::Spur;
    for r in gears {
        let Some(it) = items.get(r.item) else { continue };
        let s = it.settings(r.gear);
        if it.kind == ItemKind::Herringbone || s.v {
            return Kind::Herringbone;
        }
        if it.kind == ItemKind::Helical || s.helix.is_some_and(|h| h != 0.0) {
            style = Kind::Helical;
        }
    }
    style
}

fn bevel_style_of(items: &[GearItem], gears: &[GearRef]) -> BevelStyle {
    gears
        .iter()
        .filter_map(|r| items.get(r.item).filter(|it| it.kind == ItemKind::Bevel).map(|it| it.settings(0).bevel_style))
        .next()
        .unwrap_or_default()
}

/// The stages of `items` joined by `links` (decision 8): each item's own pair or set, one stage
/// per Mesh link, and one per shaft cluster (Shaft links and compound items, joined). When
/// `links` is empty the suggestions are used (decision 3, [`effective_links`]).
pub fn stages(items: &[GearItem], links: &[ItemLink]) -> Vec<Stage> {
    let (links, _) = effective_links(items, links);
    let geoms: Vec<Vec<GearGeom>> = items.iter().map(gear_geoms).collect();
    let shafts: Vec<Option<GearGeom>> = items.iter().map(shaft_geom).collect();
    // A gear's place, or a Shaft item's line (its gear 0).
    let geom =
        |r: GearRef| geoms.get(r.item).and_then(|g| g.get(r.gear)).copied().or_else(|| shafts.get(r.item).copied().flatten().filter(|_| r.gear == 0));
    let is_shaft = |r: GearRef| items.get(r.item).is_some_and(|it| it.kind == ItemKind::Shaft);
    let mut out = Vec::new();
    let base = |kind, gears: Vec<GearRef>, from| {
        let g: Vec<GearGeom> = gears.iter().filter_map(|r| geom(*r)).collect();
        let bs = bevel_style_of(items, &gears);
        let style = match kind {
            StageKind::Bevel if bs != BevelStyle::Straight => Kind::SpiralBevel,
            StageKind::Bevel => Kind::Bevel,
            StageKind::Parallel | StageKind::Internal | StageKind::Rack | StageKind::Shaft => style_of(items, &gears),
            k => k.plan_kind().unwrap_or(Kind::Spur),
        };
        Stage { kind, gears, geom: g, from, style, bevel_style: bs, planets: None, carrier: false, error: None }
    };
    // Each item's own.
    for (i, it) in items.iter().enumerate() {
        let n = geoms[i].len();
        let two = vec![GearRef::new(i, 0), GearRef::new(i, 1)];
        let kind = match it.kind {
            ItemKind::Internal if n >= 2 => StageKind::Internal,
            ItemKind::Rack if n >= 2 => StageKind::Rack,
            ItemKind::Bevel if n >= 2 => StageKind::Bevel,
            ItemKind::Hypoid if n >= 2 => StageKind::Hypoid,
            ItemKind::Worm if n >= 2 => StageKind::Worm,
            ItemKind::CrossedHelical if n >= 2 => StageKind::CrossedHelical,
            ItemKind::Planetary if n >= 3 => {
                let mut s = base(StageKind::Planetary, vec![GearRef::new(i, 0), GearRef::new(i, 1), GearRef::new(i, 2)], StageFrom::Item(i));
                (s.planets, s.carrier) = (it.planets, it.carrier);
                out.push(s);
                continue;
            }
            _ => continue,
        };
        out.push(base(kind, two, StageFrom::Item(i)));
    }
    // One per Mesh link.
    for (k, l) in links.iter().enumerate().filter(|(_, l)| l.kind == LinkKind::Mesh) {
        let covered = out.iter().any(|s| matches!(s.from, StageFrom::Item(_)) && s.gears.contains(&l.a) && s.gears.contains(&l.b));
        if covered {
            continue;
        }
        let (kind, error) = match (geom(l.a), geom(l.b)) {
            _ if l.a == l.b => (StageKind::Parallel, Some(format!("{} is linked to itself: remove the link", who(items, l.a)))),
            _ if is_shaft(l.a) || is_shaft(l.b) => {
                let s = if is_shaft(l.a) { l.a } else { l.b };
                (StageKind::Parallel, Some(format!("{} has no teeth, so it can't mesh: link it by Shaft to the gears it carries", who(items, s))))
            }
            (Some(a), Some(b)) => classify(items, l.a, l.b, &a, &b),
            (ga, _) => {
                let bad = if ga.is_none() { l.a } else { l.b };
                (StageKind::Parallel, Some(format!("{} doesn't exist (yet): finish its item or remove the link", who(items, bad))))
            }
        };
        let mut s = base(kind, vec![l.a, l.b], StageFrom::Link(k));
        s.error = error;
        out.push(s);
    }
    // Shaft clusters: union of Shaft links and compound items' gears.
    // (A Shaft item is one more member, its gear 0: links through it join one rigid cluster.)
    let mut all: Vec<GearRef> = geoms.iter().enumerate().flat_map(|(i, g)| (0..g.len()).map(move |k| GearRef::new(i, k))).collect();
    all.extend(shafts.iter().enumerate().filter(|(_, s)| s.is_some()).map(|(i, _)| GearRef::new(i, 0)));
    let mut parent: Vec<usize> = (0..all.len()).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let idx = |r: GearRef| all.iter().position(|g| *g == r);
    let join = |p: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (root(p, a), root(p, b));
        if ra != rb {
            p[ra.max(rb)] = ra.min(rb);
        }
    };
    let mut shaft_links: Vec<(usize, usize)> = Vec::new();
    let mut errors: Vec<(usize, String)> = Vec::new();
    // Shaft links that join nothing (a gear that isn't there, a gear to itself): their own stage,
    // so the link's reply says what is wrong.
    let mut shaft_errors: Vec<(usize, String)> = Vec::new();
    for (k, l) in links.iter().enumerate().filter(|(_, l)| l.kind == LinkKind::Shaft) {
        match (idx(l.a), idx(l.b)) {
            (Some(a), Some(b)) if a != b => {
                join(&mut parent, a, b);
                shaft_links.push((k, a));
                // Both gears are there (found above), so both have their geometry.
                let (Some(ga), Some(gb)) = (geom(l.a), geom(l.b)) else { continue };
                let t = tol([ga.radius, gb.radius].into_iter().flatten().reduce(f64::min).unwrap_or(1.0));
                if !matches!(lie(ga.centre, ga.axis, gb.centre, gb.axis, t), Lie::Coaxial) {
                    let e = if is_shaft(l.a) || is_shaft(l.b) {
                        let (s, g) = if is_shaft(l.a) { (l.a, l.b) } else { (l.b, l.a) };
                        format!(
                            "{} is not on the line of {}: a shaft carries only the gears on its line; draw the line along the gear's axis, or remove the link",
                            who(items, g),
                            who(items, s)
                        )
                    } else {
                        format!(
                            "{} and {} share a shaft but are not on one axis: put their centres on one line, square to both circles, or remove the link",
                            who(items, l.a),
                            who(items, l.b)
                        )
                    };
                    errors.push((a, e));
                }
            }
            (Some(_), Some(_)) => {
                shaft_errors.push((k, format!("{} is linked to itself: remove the link", who(items, l.a))));
            }
            (a, _) => {
                // Never dropped quietly: a link to a gear that isn't there is said.
                let bad = if a.is_none() { l.a } else { l.b };
                let e = if is_shaft(bad) {
                    format!("{} is linked as gear {}: a shaft is linked as gear 0 (it has no gears of its own)", who(items, bad), bad.gear)
                } else {
                    format!("{} doesn't exist (yet): finish its item or remove the link", who(items, bad))
                };
                shaft_errors.push((k, e));
            }
        }
    }
    let mut compound_of: Vec<(usize, usize)> = Vec::new();
    for (i, _) in items.iter().enumerate().filter(|(_, it)| it.kind == ItemKind::Compound) {
        let first = idx(GearRef::new(i, 0));
        for g in 1..geoms[i].len() {
            if let (Some(a), Some(b)) = (first, idx(GearRef::new(i, g))) {
                join(&mut parent, a, b);
            }
        }
        if let Some(a) = first {
            compound_of.push((i, a));
        }
    }
    let mut roots: Vec<usize> = Vec::new();
    for x in 0..all.len() {
        let r = root(&mut parent, x);
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    for r in roots {
        let members: Vec<GearRef> = (0..all.len()).filter(|x| root(&mut parent, *x) == r).map(|x| all[x]).collect();
        if members.len() < 2 {
            continue;
        }
        let links_in: Vec<usize> = shaft_links.iter().filter(|(_, a)| root(&mut parent, *a) == r).map(|(k, _)| *k).collect();
        let items_in: Vec<usize> = compound_of.iter().filter(|(_, a)| root(&mut parent, *a) == r).map(|(i, _)| *i).collect();
        let mut s = base(StageKind::Shaft, members, StageFrom::Cluster { links: links_in, items: items_in });
        s.error = errors.iter().find(|(a, _)| root(&mut parent, *a) == r).map(|(_, e)| e.clone());
        out.push(s);
    }
    for (k, e) in shaft_errors {
        let l = links[k];
        let gears: Vec<GearRef> = [l.a, l.b].into_iter().filter(|r| geom(*r).is_some()).collect();
        let mut s = base(StageKind::Shaft, gears, StageFrom::Cluster { links: vec![k], items: Vec::new() });
        s.error = Some(e);
        out.push(s);
    }
    out
}

/// A Mesh link's stage kind from the two gears' geometry, and what is wrong with it.
fn classify(items: &[GearItem], ra: GearRef, rb: GearRef, a: &GearGeom, b: &GearGeom) -> (StageKind, Option<String>) {
    let names = || format!("{} and {}", who(items, ra), who(items, rb));
    let kind_of = |r: GearRef| items.get(r.item).map(|it| it.kind);
    if a.rack || b.rack {
        let (rack, other) = if a.rack { (a, b) } else { (b, a) };
        let err = match (other.radius, rack.line.and_then(|(p, q)| Some(Ln { a: p, u: unit(sub(q, p))? }))) {
            (Some(r), Some(l)) => rack_tangent(&l, &Circ { c: other.centre, n: other.axis, r }).err().map(|e| format!("{}: {e}", names())),
            _ => None,
        };
        return (StageKind::Rack, err);
    }
    let t = tol(a.radius.unwrap_or(1.0).min(b.radius.unwrap_or(1.0)));
    match lie(a.centre, a.axis, b.centre, b.axis, t) {
        Lie::Coaxial if same_circle(a, b) => {
            // One circle picked for two gears.
            let (first, second) = if (ra.item, ra.gear) <= (rb.item, rb.gear) { (ra, rb) } else { (rb, ra) };
            let curve = pitch_thing(items, first).map_or(String::new(), |e| format!(" (curve {e})"));
            (
                StageKind::Parallel,
                Some(format!("{} uses the same pitch circle as {}{curve}: pick another circle", who(items, second), who(items, first))),
            )
        }
        Lie::Coaxial => (StageKind::Parallel, Some(format!("{} are on one axis, so they can't mesh: link them by Shaft instead", names()))),
        Lie::Parallel { distance } => {
            let (Some(r1), Some(r2)) = (a.radius, b.radius) else { return (StageKind::Parallel, None) };
            if a.ring || b.ring {
                let want = (r1 - r2).abs();
                let err = ((distance - want).abs() > t).then(|| {
                    format!("{} don't touch: their centres are {} mm apart, a gear inside a ring needs ring radius − gear radius = {} mm; move one circle or remove the link", names(), num(distance), num(want))
                });
                (StageKind::Internal, err)
            } else {
                let gap = distance - r1 - r2;
                let err = (gap.abs() > t).then(|| {
                    let what = if gap < 0.0 { format!("overlap by {} mm", num(-gap)) } else { format!("are {} mm apart", num(gap)) };
                    format!(
                        "{} {what}: their centres are {} mm apart and their radii add up to {} mm; make the circles Tangent or remove the link",
                        names(),
                        num(distance),
                        num(r1 + r2)
                    )
                });
                (StageKind::Parallel, err)
            }
        }
        Lie::Meet { apex, .. } => {
            let err = match (a.radius, b.radius) {
                (Some(r1), Some(r2)) => {
                    let (c1, c2) = (cone(&Circ { c: a.centre, n: a.axis, r: r1 }, apex), cone(&Circ { c: b.centre, n: b.axis, r: r2 }, apex));
                    ((c1 - c2).abs() > t).then(|| {
                        format!(
                            "{} don't touch: as a bevel pair their circles must be the same distance from where the shafts meet ({} and {} mm)",
                            names(),
                            num(c1),
                            num(c2)
                        )
                    })
                }
                _ => None,
            };
            (StageKind::Bevel, err)
        }
        Lie::Skew { offset, .. } => {
            if kind_of(ra) == Some(ItemKind::Worm) || kind_of(rb) == Some(ItemKind::Worm) {
                return (StageKind::Worm, None);
            }
            if kind_of(ra) == Some(ItemKind::Hypoid) || kind_of(rb) == Some(ItemKind::Hypoid) {
                return (StageKind::Hypoid, None);
            }
            let err = match (a.radius, b.radius) {
                (Some(r1), Some(r2)) if (offset - r1 - r2).abs() > t => Some(format!(
                    "{} don't touch: their shafts are {} mm apart and their radii add up to {} mm; move one or remove the link",
                    names(),
                    num(offset),
                    num(r1 + r2)
                )),
                _ => None,
            };
            (StageKind::CrossedHelical, err)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Mapping onto the drivetrain reader
// ---------------------------------------------------------------------------------------------

/// What a drivetrain item came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Gear(GearRef),
    /// An item's `k`-th line used as a shaft.
    Line {
        item: usize,
        k: usize,
    },
    /// An item's `k`-th point (a bearing).
    Point {
        item: usize,
        k: usize,
    },
}

/// The items as [`crate::drivetrain::read`]'s input, with what each came from and the meshes
/// and shared shafts the links ask for (indices into `items`). The reader still infers meshes
/// from tangency; WP1 plans from [`stages`] and these explicit links.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapped {
    pub items: Vec<Item>,
    pub source: Vec<Source>,
    /// Mesh and Shaft links (an item's own pair's mesh included), as indices into `items`.
    pub meshes: Vec<(usize, usize)>,
    pub shafts: Vec<(usize, usize)>,
    /// Each gear's face sides (a, b) when they are set without Auto (else Auto, from the
    /// module), per item (`None` for shafts and bearings). The reader takes their sum as the
    /// face width; WP2 places them each side of the sketch plane.
    pub faces: Vec<Option<(f64, f64)>>,
    /// The links were inferred (the request had none, decision 3).
    pub inferred: bool,
    pub notes: Vec<String>,
    /// What the reader can't take (a rack, a hypoid pinion with no size): plan it by its stage.
    pub errors: Vec<String>,
}

/// Maps `items`, `links` and `blocks` onto the drivetrain reader's input (see [`Mapped`]).
pub fn to_drivetrain(items: &[GearItem], links: &[ItemLink], blocks: &[ModuleBlock]) -> Mapped {
    let (links, inferred) = effective_links(items, links);
    let mut out = Mapped { inferred, ..Mapped::default() };
    for (i, it) in items.iter().enumerate() {
        let geoms = gear_geoms(it);
        let first = out.items.len();
        let ask_of = |g: usize| {
            let s = it.settings(g);
            let m = resolve_module(blocks, i, &s);
            Ask {
                role: None,
                module: m.module,
                teeth: if s.size_by == SizeBy::Teeth { s.teeth } else { None },
                face_width: None,
                pitch_diameter: None,
                along: None,
                helix: s.helix,
                herringbone: s.v.then_some(true),
                shaft_body: None,
                shaft_diameter: None,
                sides: None,
            }
        };
        let face_of = |g: usize| {
            let s = it.settings(g);
            let f = s.face;
            let given = match f.preset {
                FacePreset::Symmetric => f.a.or(f.b).is_some(),
                FacePreset::AOnly => f.a.is_some(),
                FacePreset::BOnly => f.b.is_some(),
                FacePreset::Custom => f.a.is_some() || f.b.is_some(),
            };
            given.then(|| face_extent(&s, 1.0))
        };
        let style_role = |g: usize| {
            let s = it.settings(g);
            match it.kind {
                ItemKind::Herringbone => Role::Gear(Kind::Herringbone),
                ItemKind::Helical => Role::Gear(Kind::Helical),
                _ if s.v => Role::Gear(Kind::Herringbone),
                _ if s.helix.is_some_and(|h| h != 0.0) => Role::Gear(Kind::Helical),
                _ => Role::Gear(Kind::Spur),
            }
        };
        let push = |out: &mut Mapped, thing: DtThing, mut ask: Ask, role: Role, src: Source, face: Option<(f64, f64)>| {
            ask.role = Some(role);
            if let Some((a, b)) = face {
                ask.face_width = Some(a + b);
            }
            out.items.push(Item { thing, ask });
            out.source.push(src);
            out.faces.push(face);
            out.items.len() - 1
        };
        let circle = |g: &GearGeom| DtThing::Circle(Circle3 { c: g.centre, n: g.axis, r: g.radius.unwrap_or(0.0) });
        match it.kind {
            ItemKind::Shaft => {}
            ItemKind::Rack => {
                out.errors.push(format!("{}: the drivetrain reader makes no racks: plan it by its stage", it.who()));
                if let Some(p) = geoms.get(1) {
                    push(&mut out, circle(p), ask_of(1), style_role(1), Source::Gear(GearRef::new(i, 1)), face_of(1));
                }
            }
            ItemKind::Hypoid | ItemKind::Worm => {
                let (Some(w), Some(p)) = (geoms.first(), geoms.get(1)) else { continue };
                let wheel_role = if it.kind == ItemKind::Worm { Role::Wheel } else { Role::Gear(Kind::Hypoid) };
                push(&mut out, circle(w), ask_of(0), wheel_role, Source::Gear(GearRef::new(i, 0)), face_of(0));
                let (a, b) = p.line.unwrap_or((p.centre, add(p.centre, p.axis)));
                let mut ask = ask_of(1);
                ask.along = Some(dot(sub(p.centre, a), p.axis));
                ask.pitch_diameter = match (it.kind, p.radius) {
                    (_, Some(r)) => Some(2.0 * r),
                    _ => {
                        let (z1, z2) = (it.settings(0).teeth, it.settings(1).teeth);
                        let d = w.radius.unwrap_or(0.0) * 2.0;
                        Some(match (z1, z2) {
                            (Some(z1), Some(z2)) if z1 > 0 => d * f64::from(z2) / f64::from(z1),
                            _ => {
                                out.notes.push(format!(
                                    "{}: the pinion's size is not drawn: taken as a quarter of the ring's (4:1); give both tooth counts to set it",
                                    it.who()
                                ));
                                d / 4.0
                            }
                        })
                    }
                };
                let role = if it.kind == ItemKind::Worm { Role::Gear(Kind::Worm) } else { Role::Gear(Kind::Hypoid) };
                push(&mut out, DtThing::Line(Line3 { a, b }), ask, role, Source::Gear(GearRef::new(i, 1)), face_of(1));
            }
            _ => {
                for (g, geo) in geoms.iter().enumerate() {
                    let role = match it.kind {
                        ItemKind::Internal | ItemKind::Planetary if geo.ring => Role::Internal,
                        ItemKind::Bevel => {
                            Role::Gear(if it.settings(0).bevel_style == BevelStyle::Straight { Kind::Bevel } else { Kind::SpiralBevel })
                        }
                        ItemKind::CrossedHelical => Role::Gear(Kind::CrossedHelical),
                        _ => style_role(g),
                    };
                    push(&mut out, circle(geo), ask_of(g), role, Source::Gear(GearRef::new(i, g)), face_of(g));
                }
                if it.kind == ItemKind::Planetary {
                    out.notes.push(format!("{}: read as sun, one planet and ring; its other planets and carrier come from its stage", it.who()));
                }
            }
        }
        // Its shaft lines and bearings (not a pair's axis lines, which carry gears).
        if matches!(it.kind, ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone | ItemKind::Compound | ItemKind::Shaft) {
            for (k, t) in it.things.iter().filter(|t| matches!(t, Thing::Line { .. })).enumerate() {
                if let Thing::Line { a, b, .. } = t {
                    let ask = Ask { shaft_diameter: it.shaft_diameter, ..Ask::default() };
                    push(&mut out, DtThing::Line(Line3 { a: *a, b: *b }), ask, Role::Shaft, Source::Line { item: i, k }, None);
                }
            }
            for (k, t) in it.things.iter().filter(|t| matches!(t, Thing::Point { .. })).enumerate() {
                if let Thing::Point { p, .. } = t {
                    push(&mut out, DtThing::Point(*p), Ask::default(), Role::Bearing, Source::Point { item: i, k }, None);
                }
            }
        }
        // An item's own mesh(es) and a compound's shaft.
        let at = |out: &Mapped, g: usize| (first..out.items.len()).find(|k| out.source[*k] == Source::Gear(GearRef::new(i, g)));
        match it.kind {
            ItemKind::Compound => {
                for g in 1..geoms.len() {
                    if let (Some(a), Some(b)) = (at(&out, 0), at(&out, g)) {
                        out.shafts.push((a, b));
                    }
                }
            }
            ItemKind::Planetary => {
                for (x, y) in [(0, 2), (2, 1)] {
                    if let (Some(a), Some(b)) = (at(&out, x), at(&out, y)) {
                        out.meshes.push((a, b));
                    }
                }
            }
            _ => {
                if let (Some(a), Some(b)) = (at(&out, 0), at(&out, 1)) {
                    out.meshes.push((a, b));
                }
            }
        }
    }
    let find = |out: &Mapped, r: GearRef| out.source.iter().position(|s| *s == Source::Gear(r));
    for l in &links {
        if let (Some(a), Some(b)) = (find(&out, l.a), find(&out, l.b)) {
            let list = if l.kind == LinkKind::Mesh { &mut out.meshes } else { &mut out.shafts };
            if a != b && !list.iter().any(|p| *p == (a, b) || *p == (b, a)) {
                list.push((a, b));
            }
        }
    }
    out
}
