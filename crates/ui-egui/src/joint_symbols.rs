//! The pictogram of each joint type in the viewport: a few flat strokes, the same size on screen
//! wherever the joint sits. A movement draws as what it does (a double arrow over a line slides
//! along it, a ring with a head turns) and a relation as its usual sign.
//!
//! Coordinates run from -1 to 1 with y up. The types with one direction of travel name it, and
//! their x runs along it on screen ([`along`]); the others stay upright.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::{Arc, Weak};

use egui::{Color32, Pos2, Shape, Stroke, Vec2, vec2};
use solidcraft_engine::Session;
use solidcraft_engine::doc::ModelState;
use solidcraft_engine::doc::joints::{self, Joint, JointKind};

use solidcraft_theme::Tokens;

type Line = Vec<[f32; 2]>;

/// Steps in an arc.
const ARC: usize = 24;

/// How large a dot is on a point the joint holds.
const POINT: f32 = 0.24;

thread_local! {
    /// The bright joints, for the design state they were found in, and the solve they came from.
    static BRIGHT: RefCell<Option<Bright>> = const { RefCell::new(None) };
}

/// The bright joints of one design state, and the solve they were picked from, with what that
/// solve holds for.
struct Bright {
    /// The design, by id and revision.
    state: (u64, u64),
    joints: Arc<BTreeSet<u64>>,
    /// What the freedom depends on ([`construction`]). `None`: the solve found errors, so it may
    /// have run on a model still being built, and the next design state solves again.
    construction: Option<u64>,
    /// The model geometry the joints' frames were found on. Moving parts keeps it; editing a
    /// feature makes a new one.
    geometry: Weak<ModelState>,
    /// How free each part is in the world, and each joined pair against each other.
    dof: Arc<BTreeMap<u64, usize>>,
    pairs: Arc<BTreeMap<(u64, u64), usize>>,
}

/// A hash of what decides how free the parts are: everything about the joints but their values
/// and limits, so dragging a part doesn't solve again for every frame. A limit only changes the
/// freedom where a motion link drives the value it stops, since the link is held to it there, so
/// only those limits count; [`bright_joints`] reads the others from the design each time.
fn construction(s: &Session) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for j in &s.doc.assembly.joints {
        format!("{} {:?} {:?} {:?} {} {} {} {}", j.id, j.kind, j.a, j.b, j.flip, j.offset, j.angle, j.suppressed).hash(&mut h);
    }
    format!("{:?}", s.doc.assembly.links).hash(&mut h);
    for l in &s.doc.assembly.links {
        let held = s.doc.assembly.joints.iter().find(|j| j.id == l.b).and_then(|j| j.limits.get(l.ib).copied().flatten());
        format!("{held:?}").hash(&mut h);
    }
    for o in &s.doc.occurrences {
        (o.id, o.parent, o.grounded).hash(&mut h);
    }
    h.finish()
}

/// The joints [`bright_joints`] picks in this design, from a solve made once for each
/// construction and model geometry.
pub(crate) fn bright(s: &Session) -> Arc<BTreeSet<u64>> {
    let state = (s.uid, s.revision);
    if let Some(joints) = BRIGHT.with(|b| b.borrow().as_ref().filter(|b| b.state == state).map(|b| b.joints.clone())) {
        return joints;
    }
    let built = construction(s);
    let model = s.model.state();
    let same = BRIGHT.with(|b| {
        b.borrow()
            .as_ref()
            .filter(|b| b.state.0 == state.0 && b.construction == Some(built) && b.geometry.upgrade().is_some_and(|g| Arc::ptr_eq(&g, &model)))
            .map(|b| (b.dof.clone(), b.pairs.clone()))
    });
    let (dof, pairs, construction) = same.map_or_else(
        || {
            let solved = joints::solve_holding_with_analysis(&s.doc, &model, &joints::Held::new(), joints::Analysis::PairFreedom);
            (Arc::new(solved.dof), Arc::new(solved.pair_dof), solved.errors.is_empty().then_some(built))
        },
        |(dof, pairs)| (dof, pairs, Some(built)),
    );
    let joints = Arc::new(bright_joints(&s.doc.assembly.joints, &dof, &pairs));
    let geometry = Arc::downgrade(&model);
    BRIGHT.with(|b| *b.borrow_mut() = Some(Bright { state, joints: joints.clone(), construction, geometry, dof, pairs }));
    joints
}

/// The values of a joint with a stop on them.
fn limited_values(j: &Joint) -> usize {
    j.limits.iter().flatten().filter(|(lo, hi)| lo.is_finite() || hi.is_finite()).count()
}

/// Does the joint have a limit?
pub(crate) fn has_limit(j: &Joint) -> bool {
    limited_values(j) > 0
}

/// The joints that draw bright: those that let their parts move against each other. A rigid or
/// suppressed joint isn't one of them, and nor is a joint between parts the construction has
/// locked together, however they move as one on the joints further along. That is how it shows
/// what it has fully constrained.
///
/// `pairs` is how free each pair of joined parts is against each other. A pair over the solver's
/// caps isn't in it: `dof`, how free each part is in the world, then stands in, which can only
/// leave its joints bright, never wrongly dim.
///
/// The joints between two parts that still move are all bright, until limits stop as many values
/// as the pair has free: then the joints with the limits are what shapes the motion, and only they
/// are bright. Parts are compared in pairs, since a limit says nothing about the joints to a third
/// part.
pub(crate) fn bright_joints(joints: &[Joint], dof: &BTreeMap<u64, usize>, pairs: &BTreeMap<(u64, u64), usize>) -> BTreeSet<u64> {
    let mut stacks: BTreeMap<(u64, u64), Vec<&Joint>> = BTreeMap::new();
    for j in joints.iter().filter(|j| j.kind != JointKind::Rigid && !j.suppressed) {
        let (a, b) = (j.a.occurrence, j.b.occurrence);
        stacks.entry((a.min(b), a.max(b))).or_default().push(j);
    }
    let mut bright = BTreeSet::new();
    for ((a, b), stack) in stacks {
        let in_the_world = || [a, b].map(|o| dof.get(&o).copied().unwrap_or(6)).into_iter().max().unwrap_or(0);
        let free = pairs.get(&(a, b)).copied().unwrap_or_else(in_the_world);
        if free == 0 {
            continue;
        }
        let stopped: usize = stack.iter().map(|j| limited_values(j)).sum();
        bright.extend(stack.iter().filter(|j| stopped == 0 || stopped < free || has_limit(j)).map(|j| j.id));
    }
    bright
}

/// A joint type's pictogram.
#[derive(Debug, PartialEq)]
pub(crate) struct Symbol {
    pub lines: Vec<Line>,
    /// Filled dots: centre and radius.
    pub dots: Vec<([f32; 2], f32)>,
}

/// The direction in the joint's frame that a type's pictogram runs along.
pub(crate) fn along(kind: JointKind) -> Option<[f64; 3]> {
    match kind {
        JointKind::Slider | JointKind::Cylindrical | JointKind::PointOnLine => Some([0.0, 0.0, 1.0]),
        JointKind::PinSlot => Some([1.0, 0.0, 0.0]),
        _ => None,
    }
}

/// The pictogram of a joint type: what the joint lets move, or the relation it holds.
pub(crate) fn symbol(kind: JointKind) -> Symbol {
    let centre = [0.0, 0.0];
    let (lines, dots) = match kind {
        // Fixed, as a support is drawn: a line hatched on one side.
        JointKind::Rigid => {
            let hatch = [-0.45, 0.15, 0.75].map(|x| vec![[x, 0.3], [x - 0.4, -0.4]]);
            ([vec![vec![[-1.0, 0.3], [1.0, 0.3]]], hatch.to_vec()].concat(), Vec::new())
        }
        JointKind::Revolute => (turn(0.85), vec![(centre, 0.14)]),
        JointKind::Slider => ([slide(0.35), vec![vec![[-1.0, -0.45], [1.0, -0.45]]]].concat(), Vec::new()),
        // A slide over a turn: it does both.
        JointKind::Cylindrical => ([slide(0.55), turn_at([0.0, -0.42], 0.5)].concat(), Vec::new()),
        // A pin in a slot.
        JointKind::PinSlot => {
            let (c, w) = (0.55, 0.55);
            let sides = vec![vec![[-c, w], [c, w]], vec![[-c, -w], [c, -w]]];
            ([vec![arc([c, 0.0], w, -FRAC_PI_2, PI), arc([-c, 0.0], w, FRAC_PI_2, PI)], sides].concat(), vec![(centre, 0.28)])
        }
        JointKind::Planar => {
            let across: Vec<Line> = slide(0.0).iter().map(|line| line.iter().map(|&[x, y]| [y, x]).collect()).collect();
            ([slide(0.0), across].concat(), Vec::new())
        }
        // A ball in its socket, open to one side.
        JointKind::Ball => (vec![arc(centre, 0.9, 0.7, TAU - 1.4)], vec![(centre, 0.45)]),
        JointKind::Parallel => (vec![vec![[-0.7, -0.75], [-0.1, 0.75]], vec![[0.1, -0.75], [0.7, 0.75]]], Vec::new()),
        JointKind::Perpendicular => (vec![vec![[-0.9, -0.7], [0.9, -0.7]], vec![[0.0, -0.7], [0.0, 0.8]]], Vec::new()),
        JointKind::Angle => (vec![vec![[0.95, -0.7], [-0.85, -0.7], [0.45, 0.75]], arc([-0.85, -0.7], 0.95, 0.0, 0.84)], Vec::new()),
        JointKind::PointOnPlane => (vec![plate()], vec![(centre, POINT)]),
        JointKind::PointOnLine => (vec![vec![[-1.15, 0.0], [1.15, 0.0]]], vec![(centre, POINT)]),
        JointKind::LineOnPlane => (vec![plate(), vec![[-1.15, 0.0], [1.15, 0.0]]], Vec::new()),
    };
    Symbol { lines, dots }
}

/// An arc about `centre`, from `from` on through `sweep` radians.
fn arc(centre: [f32; 2], radius: f32, from: f32, sweep: f32) -> Line {
    let [cx, cy] = centre;
    (0..=ARC)
        .map(|k| {
            let a = from + sweep * k as f32 / ARC as f32;
            [cx + radius * a.cos(), cy + radius * a.sin()]
        })
        .collect()
}

/// A turn: an open ring with a head at its end.
fn turn(r: f32) -> Vec<Line> {
    turn_at([0.0, 0.0], r)
}

/// A turn about `centre`, its head in scale with its ring.
fn turn_at(centre: [f32; 2], r: f32) -> Vec<Line> {
    let k = r / 0.85;
    let (from, end) = (0.9, 0.9 + TAU - 1.3);
    let (cos, sin) = (end.cos(), end.sin());
    let tip = [centre[0] + r * cos, centre[1] + r * sin];
    // Back along the ring from the tip, and out to either side of it.
    let barb = |side: f32| [tip[0] + k * (0.45 * sin + side * 0.35 * cos), tip[1] + k * (-0.45 * cos + side * 0.35 * sin)];
    vec![arc(centre, r, from, end - from), vec![barb(1.0), tip, barb(-1.0)]]
}

/// A slide: a level line at height `y` with a head at both ends.
fn slide(y: f32) -> Vec<Line> {
    let head = |x: f32, back: f32| vec![[back, y + 0.35], [x, y], [back, y - 0.35]];
    vec![vec![[-1.0, y], [1.0, y]], head(-1.0, -0.6), head(1.0, 0.6)]
}

/// A plane seen at a slant.
fn plate() -> Line {
    vec![[-0.95, -0.55], [0.45, -0.55], [0.95, 0.55], [-0.45, 0.55], [-0.95, -0.55]]
}

/// The `joint_symbols` styles, as Preferences lists them; a style's value is its place here.
pub(crate) const STYLES: [&str; 7] = ["Ring and axis", "White on dark", "Solid accent", "Solid amber", "Outline", "Colour by type", "Dots"];

/// The ring and axis drawn in the model before badges, and the default ([`crate::dialogs_assembly::ring_shapes`]).
pub(crate) const RING_AND_AXIS: u8 = 0;
const SOLID_ACCENT: u8 = 2;
const SOLID_AMBER: u8 = 3;
/// The pictogram alone, with a thin halo to read on any part.
const OUTLINE: u8 = 4;
/// A fill and an outline for each family of types ([`Family`]).
const BY_TYPE: u8 = 5;
/// A small dot, bright where the joint lets its parts move; the badge shows once it is picked.
const DOTS: u8 = 6;

/// How far a style's badge reaches on screen, for the overlap test; `None` for the ring and axis,
/// which reaches as far as the joint's ring.
pub(crate) fn badge_reach(style: u8) -> Option<f32> {
    (style != RING_AND_AXIS).then(|| size(style).0)
}

/// A badge's radius and the half size of its pictogram, unpicked. A dot has no pictogram.
fn size(style: u8) -> (f32, f32) {
    if style == DOTS { (5.0, 0.0) } else { (13.0, 8.5) }
}

/// What the joint types do, for the [`BY_TYPE`] style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    /// Rigid: nothing moves.
    Fixed,
    /// A turn, a slide or both.
    Motion,
    /// Orientations only: parallel, perpendicular, an angle.
    Relation,
    /// A point or line held on a plane or line.
    Contact,
}

/// The family a joint type belongs to, which the [`BY_TYPE`] style colours and outlines.
pub(crate) fn family(kind: JointKind) -> Family {
    match kind {
        JointKind::Rigid => Family::Fixed,
        JointKind::Revolute | JointKind::Slider | JointKind::Cylindrical | JointKind::PinSlot | JointKind::Planar | JointKind::Ball => Family::Motion,
        JointKind::Parallel | JointKind::Perpendicular | JointKind::Angle => Family::Relation,
        JointKind::PointOnPlane | JointKind::PointOnLine | JointKind::LineOnPlane => Family::Contact,
    }
}

/// The outline of a badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outline {
    Circle,
    Square,
    Diamond,
    Hexagon,
}

/// The corners of a badge's outline about `c`, reaching about as far as a circle of `radius`.
fn corners(outline: Outline, c: Pos2, radius: f32) -> Vec<Pos2> {
    let ring = |n: usize, r: f32, turn: f32| (0..n).map(|k| c + egui::Vec2::angled(turn + TAU * k as f32 / n as f32) * r).collect();
    match outline {
        Outline::Circle => Vec::new(),
        Outline::Square => ring(4, radius * 1.2, PI / 4.0),
        Outline::Diamond => ring(4, radius * 1.3, 0.0),
        Outline::Hexagon => ring(6, radius * 1.1, 0.0),
    }
}

/// How a badge draws for its joint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mood {
    /// Lets its parts move ([`bright_joints`]), or every joint while Highlight moving joints is off.
    Bright,
    /// Its parts can't move against each other.
    Quiet,
    Suppressed,
    Picked,
}

/// How a joint's badge looks: its outline, size, fill, and the width and colour of its rim and
/// symbol, with a halo under the symbol where there is no fill to carry it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Badge {
    pub outline: Outline,
    pub radius: f32,
    /// Half the pictogram's size; 0 draws none (a dot).
    pub half: f32,
    pub fill: Color32,
    pub ring: Stroke,
    pub ink: Stroke,
    pub halo: Option<Stroke>,
    /// The limit mark's colour.
    pub mark: Color32,
    /// Drawn larger and filled, with a second ring around it.
    pub picked: bool,
}

/// The badge of a joint of `kind` in `style`; `picked` is the colour a picked joint fills with.
pub(crate) fn badge(style: u8, kind: JointKind, mood: Mood, picked: Color32, t: &Tokens) -> Badge {
    let (radius, half) = size(style);
    let outline = if style == BY_TYPE {
        match family(kind) {
            Family::Fixed => Outline::Square,
            Family::Motion => Outline::Circle,
            Family::Relation => Outline::Diamond,
            Family::Contact => Outline::Hexagon,
        }
    } else {
        Outline::Circle
    };
    let plain =
        |fill: Color32, ring: Stroke, ink: Stroke| Badge { outline, radius, half, fill, ring, ink, halo: None, mark: t.warning, picked: false };
    let look = match mood {
        Mood::Picked => {
            // A dot shows its badge once picked, so the pick says what it is.
            let (radius, half) = size(BY_TYPE);
            return Badge {
                radius: radius + 4.0,
                half: half + 2.5,
                ring: Stroke::new(1.0, picked),
                ink: Stroke::new(2.0, t.panel),
                picked: true,
                ..plain(picked, Stroke::NONE, Stroke::NONE)
            };
        }
        Mood::Suppressed if style == DOTS => plain(t.text_dim.gamma_multiply(0.4), Stroke::new(1.5, t.panel.gamma_multiply(0.6)), Stroke::NONE),
        Mood::Quiet if style == DOTS => plain(t.text_dim, Stroke::new(1.5, t.panel), Stroke::NONE),
        Mood::Suppressed => {
            let dim = t.text_dim.gamma_multiply(0.5);
            plain(t.panel.gamma_multiply(0.85), Stroke::new(1.0, dim), Stroke::new(1.5, dim))
        }
        Mood::Quiet => plain(t.panel.gamma_multiply(0.85), Stroke::new(1.0, t.text_dim.gamma_multiply(0.6)), Stroke::new(1.5, t.text_dim)),
        Mood::Bright => match style {
            SOLID_ACCENT => plain(t.accent, Stroke::new(1.0, t.on_accent), Stroke::new(2.0, t.on_accent)),
            SOLID_AMBER => Badge { mark: t.text, ..plain(t.warning, Stroke::new(1.0, t.panel), Stroke::new(2.0, t.panel)) },
            OUTLINE => plain(Color32::TRANSPARENT, Stroke::NONE, Stroke::new(2.0, t.accent)),
            BY_TYPE => {
                let fill = match family(kind) {
                    Family::Fixed => t.text_dim,
                    Family::Motion => t.accent,
                    Family::Relation => t.sketch_accent,
                    Family::Contact => t.surface_accent,
                };
                let ink = if light(fill) { t.tab_on_fill_dark } else { t.on_accent };
                plain(fill, Stroke::new(1.0, ink), Stroke::new(2.0, ink))
            }
            DOTS => plain(t.accent, Stroke::new(1.5, t.panel), Stroke::NONE),
            _ => plain(t.panel, Stroke::new(2.0, t.accent), Stroke::new(2.0, t.text)),
        },
    };
    if style == OUTLINE {
        // No disc: the symbol alone, on a halo of the panel colour.
        return Badge {
            fill: Color32::TRANSPARENT,
            ring: Stroke::NONE,
            halo: Some(Stroke::new(look.ink.width + 3.0, t.panel.gamma_multiply(0.8))),
            ..look
        };
    }
    look
}

/// Does dark ink read better than light on `fill`?
fn light(fill: Color32) -> bool {
    let [r, g, b, _] = fill.to_array();
    0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b) > 150.0
}

/// A badge at `c` for a joint of `kind`: `x` is the screen direction its pictogram runs along
/// ([`along`]), `limit` puts a mark on its rim (or rings a dot), and `faded` draws it faint,
/// where it lies on another.
pub(crate) fn badge_shapes(c: Pos2, x: Vec2, kind: JointKind, look: &Badge, limit: bool, faded: bool, t: &Tokens) -> Vec<Shape> {
    let fade = |col: Color32| if faded { col.gamma_multiply(0.3) } else { col };
    let stroke = |s: Stroke| Stroke::new(s.width, fade(s.color));
    let disc = |radius: f32, fill: Color32, rim: Stroke| match corners(look.outline, c, radius) {
        points if points.is_empty() => Shape::Circle(egui::epaint::CircleShape { center: c, radius, fill, stroke: rim }),
        points => Shape::convex_polygon(points, fill, rim),
    };
    let mut out = Vec::new();
    if look.picked {
        out.push(disc(look.radius + 3.5, Color32::TRANSPARENT, Stroke::new(2.0, fade(t.text))));
    }
    let dot = look.half == 0.0;
    let ring = if limit && dot { Stroke::new(2.0, look.mark) } else { look.ring };
    out.push(disc(look.radius, fade(look.fill), stroke(ring)));
    if dot {
        return out;
    }
    // y to the upper side of x.
    let y = vec2(x.y, -x.x);
    let place = |p: [f32; 2]| c + (x * p[0] + y * p[1]) * look.half;
    let symbol = symbol(kind);
    for ink in look.halo.into_iter().chain([look.ink]) {
        let wide = if Some(ink) == look.halo { ink.width / 2.0 } else { 0.0 };
        for line in &symbol.lines {
            out.push(Shape::line(line.iter().copied().map(place).collect(), stroke(ink)));
        }
        for (centre, r) in &symbol.dots {
            out.push(Shape::circle_filled(place(*centre), r * look.half + wide, fade(ink.color)));
        }
    }
    if limit {
        let r = (look.radius * 0.3).clamp(3.0, 4.0);
        out.push(Shape::Circle(egui::epaint::CircleShape {
            center: c + vec2(0.7, -0.7) * look.radius,
            radius: r,
            fill: fade(look.mark),
            stroke: Stroke::new(1.5, fade(t.panel)),
        }));
    }
    out
}

#[cfg(test)]
#[path = "joint_symbol_sheet.rs"]
mod joint_symbol_sheet;
