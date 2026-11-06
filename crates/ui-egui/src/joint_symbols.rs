//! The pictogram of each joint type in the viewport: a few flat strokes, the same size on screen
//! wherever the joint sits. A movement draws as what it does (a double arrow over a line slides
//! along it, a ring with a head turns) and a relation as its usual sign.
//!
//! Coordinates run from -1 to 1 with y up. The types with one direction of travel name it, and
//! their x runs along it on screen ([`along`]); the others stay upright.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

use solidcraft_engine::Session;
use solidcraft_engine::doc::joints::{self, Joint, JointKind};

type Line = Vec<[f32; 2]>;

/// Steps in an arc.
const ARC: usize = 24;

/// How large a dot is on a point the joint holds.
const POINT: f32 = 0.24;

thread_local! {
    /// The bright joints, for the design state they were found in.
    static BRIGHT: RefCell<Option<Bright>> = const { RefCell::new(None) };
}

/// A solve's answer, with what it holds for.
struct Bright {
    /// The design, by id and revision.
    state: (u64, u64),
    /// What the freedom depends on ([`construction`]). `None`: the solve found errors, so it may
    /// have run on a model still being built, and the next design state solves again.
    construction: Option<u64>,
    joints: Arc<BTreeSet<u64>>,
}

/// A hash of what decides how free the parts are: everything about the joints but their values,
/// so dragging a part doesn't solve again for every frame.
fn construction(s: &Session) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for j in &s.doc.assembly.joints {
        format!("{} {:?} {:?} {:?} {} {} {} {}", j.id, j.kind, j.a, j.b, j.flip, j.offset, j.angle, j.suppressed).hash(&mut h);
    }
    format!("{:?}", s.doc.assembly.links).hash(&mut h);
    for o in &s.doc.occurrences {
        (o.id, o.parent, o.grounded).hash(&mut h);
    }
    h.finish()
}

/// The joints [`bright_joints`] picks in this design, solved once for each construction.
pub(crate) fn bright(s: &Session) -> Arc<BTreeSet<u64>> {
    let state = (s.uid, s.revision);
    if let Some(joints) = BRIGHT.with(|b| b.borrow().as_ref().filter(|b| b.state == state).map(|b| b.joints.clone())) {
        return joints;
    }
    let built = construction(s);
    let same = BRIGHT.with(|b| b.borrow().as_ref().filter(|b| b.state.0 == state.0 && b.construction == Some(built)).map(|b| b.joints.clone()));
    let (joints, construction) = same.map_or_else(
        || {
            let solved = joints::solve_holding_with_analysis(&s.doc, &s.model.state(), &joints::Held::new(), joints::Analysis::PairFreedom);
            (Arc::new(bright_joints(&s.doc.assembly.joints, &solved.dof, &solved.pair_dof)), solved.errors.is_empty().then_some(built))
        },
        |joints| (joints, Some(built)),
    );
    BRIGHT.with(|b| *b.borrow_mut() = Some(Bright { state, construction, joints: joints.clone() }));
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
        JointKind::Cylindrical => ([turn(0.95), slide(0.0)].concat(), Vec::new()),
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
    let (from, end) = (0.9, 0.9 + TAU - 1.3);
    let (cos, sin) = (end.cos(), end.sin());
    let tip = [r * cos, r * sin];
    // Back along the ring from the tip, and out to either side of it.
    let barb = |side: f32| [tip[0] + 0.45 * sin + side * 0.35 * cos, tip[1] - 0.45 * cos + side * 0.35 * sin];
    vec![arc([0.0, 0.0], r, from, end - from), vec![barb(1.0), tip, barb(-1.0)]]
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
