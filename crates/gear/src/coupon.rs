//! A print test coupon: instead of a whole pair or planetary set, a small piece of each member
//! carrying a few of its real teeth where they mesh, to print and try for fit and backlash
//! before printing the set.
//!
//! - A member small enough gets a piece that reaches its centre: the teeth, two radial sides and
//!   a round boss with the member's bore (or a pin hole) on the gear's true centre. A planet
//!   gets teeth on both sides (towards the sun and towards the ring).
//! - A large member, and a ring, gets a block: the teeth, two sides and a flat reference edge
//!   square to the line of centres at a known distance from the gear's centre (a fact).
//! - A base plate carries a peg for every centre hole and a fence for every flat, all at the
//!   true centre distance: drop the pieces on and turn them.
//!
//! Everything is cut from the plan of the whole set (its preview outlines and bodies), so the
//! teeth are the real ones, at their real phase.
//!
//! The tooth tuning of [`crate::body`] (tip rounding, horizontal-expansion compensation, tip
//! relief, clearance, tip diameters, gear 2's backlash) is in the teeth, so the coupon tests it;
//! teeth that change along the face (elephant-foot relief, crowning) are cut from the gear's own
//! steps, as helical teeth are. Shafts, chamfers, hub hardware, bearing pockets and gear 2's own
//! face width and offset are left off (with a note when they were set), as hubs are.

use std::f64::consts::{PI, TAU};

use crate::axes::num;
use crate::geom::{Outline, Region, Seg, V2, wrap};
use crate::hub::{Bore, Chamfer, Hardware, Shaft};
use crate::involute::join_ends;
use crate::plan::{Body, Frame, Kind, Op, Plan, PreviewGear, Spec, Step, Unit};

/// Fewest and most teeth a coupon's window may be asked to hold.
pub const MIN_TEETH: u32 = 3;
pub const MAX_TEETH: u32 = 40;

/// A coupon is asked for and this kind of set has one.
pub fn applies(s: &Spec) -> bool {
    s.coupon && matches!(s.kind, Kind::Pair | Kind::Planetary)
}

/// How a piece is held at the true centre distance.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Locate {
    /// A hole on the gear's centre whose largest inscribed circle has this radius.
    Peg { radius: f64 },
    /// A flat edge square to `dir` at `at` along it from the gear's centre, from `lo` to `hi`
    /// across; the piece lies on the side of the flat that `inward` (±1, along `dir`) points to.
    Flat { dir: f64, at: f64, lo: f64, hi: f64, inward: f64 },
    /// Not held (a whole ring).
    Free,
}

struct Piece {
    /// The member's name in the preview ("gear 1", "sun", "ring"...).
    who: String,
    body_name: String,
    center: V2,
    /// The piece at z = 0, in the gear's own frame (a helical piece's end teeth are trimmed a
    /// little more where its sides cut across them).
    region: Region,
    steps: Vec<Step>,
    locate: Locate,
    teeth: u32,
    whole: bool,
}

/// One member to cut a piece from.
struct Source<'a> {
    preview: &'a PreviewGear,
    body: &'a Body,
    teeth: u32,
    /// Directions (from its centre) of the meshes to keep teeth for.
    dirs: Vec<f64>,
    ring: bool,
    bore: Option<&'a Bore>,
}

fn line(a: V2, b: V2) -> Seg {
    Seg::Line(a, b)
}

/// The arc about the origin from `a` to `b`, counter-clockwise.
fn arc(a: V2, b: V2) -> Seg {
    Seg::Arc { c: V2::ZERO, a, b, ccw: true }
}

fn rect(lo: V2, hi: V2) -> Outline {
    let (a, b, c, d) = (lo, V2::new(hi.x, lo.y), hi, V2::new(lo.x, hi.y));
    Outline { segs: vec![line(a, b), line(b, c), line(c, d), line(d, a)] }
}

/// An arc about the origin split at its middle.
fn halves(s: &Seg) -> Option<(Seg, Seg)> {
    let Seg::Arc { c, a, b, ccw } = s else { return None };
    let sweep = wrap((*b - *c).angle() - (*a - *c).angle());
    let mid = *c + (*a - *c).rot(sweep / 2.0);
    Some((Seg::Arc { c: *c, a: *a, b: mid, ccw: *ccw }, Seg::Arc { c: *c, a: mid, b: *b, ccw: *ccw }))
}

/// A run of `n` teeth out of a gear's outline, counter-clockwise, centred as near `dir` as its
/// teeth allow.
struct Window {
    segs: Vec<Seg>,
    start: V2,
    end: V2,
    teeth: u32,
}

/// The teeth of `o` (`z` equal teeth, as [`crate::involute::teeth_outline`] lays them out) cut
/// into a window of `n` teeth (one more when that centres it on `dir`), from the middle of one
/// root land to the middle of another.
fn window(o: &Outline, z: u32, n: u32, dir: f64) -> Option<Window> {
    let per = o.segs.len() / z as usize;
    if per == 0 || o.segs.len() != per * z as usize || o.segs.iter().any(|s| matches!(s, Seg::Circle(..))) {
        return None;
    }
    let pitch = TAU / f64::from(z);
    // Each tooth ends with the root land to the next, unless the fillets meet.
    let last = &o.segs[per - 1];
    let root = o.segs[0].start().len();
    let land = per >= 2
        && matches!(last, Seg::Arc { c, a, b, .. } if c.len() < 1e-9 * root && (a.len() - root).abs() < 1e-9 * root && (b.len() - root).abs() < 1e-9 * root);
    // Where a window starting before tooth 0 begins.
    let begin0 = if land { halves(&o.segs[o.segs.len() - 1])?.1.start() } else { o.segs[0].start() };
    let pick = |n: u32| -> (i64, f64) {
        let i = ((dir - f64::from(n) * pitch / 2.0 - begin0.angle()) / pitch).round();
        let off = wrap(begin0.angle() + i * pitch + f64::from(n) * pitch / 2.0 - dir);
        (i as i64, off.abs())
    };
    let (mut n, (mut i, off)) = (n, pick(n));
    if off > 0.25 * pitch && n + 1 < z {
        n += 1;
        i = pick(n).0;
    }
    let total = o.segs.len() as i64;
    let at = |k: i64| &o.segs[k.rem_euclid(total) as usize];
    let first = i * per as i64;
    let end = first + i64::from(n) * per as i64;
    let mut segs = Vec::new();
    if land {
        segs.push(halves(at(first - 1))?.1);
        segs.extend((first..end - 1).map(|k| at(k).clone()));
        segs.push(halves(at(end - 1))?.0);
    } else {
        segs.extend((first..end).map(|k| at(k).clone()));
    }
    let (start, end) = (segs[0].start(), segs[segs.len() - 1].end());
    Some(Window { segs, start, end, teeth: n })
}

/// The largest radius of an outline about the origin, and the smallest (the nearest point of
/// its sides, not only of its corners).
fn radii(o: &Outline, tol: f64) -> (f64, f64) {
    let p = crate::mesh2d::polygon(o, tol);
    let far = p.iter().fold(0.0, |hi: f64, q| hi.max(q.len()));
    let near = p.windows(2).fold(p.first().map_or(0.0, |q| q.len()), |lo: f64, w| {
        let d = w[1] - w[0];
        let l2 = d.dot(d);
        let t = if l2 > 0.0 { (-w[0].dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        lo.min((w[0] + d * t).len())
    });
    (far, near)
}

/// Windows joined through a round boss of radius `boss` about the centre: each window, a
/// radial side in, round the boss to the next window's side, and out.
fn through_centre(windows: &[(Vec<Seg>, V2, V2)], boss: f64) -> Outline {
    let mut segs = Vec::new();
    for (k, (run, _, end)) in windows.iter().enumerate() {
        let next = windows[(k + 1) % windows.len()].1;
        let (a, b) = (V2::polar(boss, end.angle()), V2::polar(boss, next.angle()));
        segs.extend(run.iter().cloned());
        segs.push(line(*end, a));
        segs.push(arc(a, b));
        segs.push(line(b, next));
    }
    join_ends(&mut segs);
    Outline { segs }
}

/// A window closed by two sides along `dir` and a flat square to it at `at` from the centre.
fn flat_backed(run: &[Seg], start: V2, end: V2, dir: f64, at: f64) -> Outline {
    let (u, v) = (V2::polar(1.0, dir), V2::polar(1.0, dir + PI / 2.0));
    let (f1, f0) = (u * at + v * end.dot(v), u * at + v * start.dot(v));
    let mut segs: Vec<Seg> = run.to_vec();
    segs.extend([line(end, f1), line(f1, f0), line(f0, start)]);
    join_ends(&mut segs);
    Outline { segs }
}

/// A ring's window (its tooth spaces, running counter-clockwise on the inside) closed outwards
/// by its two sides, carried on from the centre, and a flat at `at`.
fn ring_block(run: &[Seg], start: V2, end: V2, dir: f64, at: f64) -> Outline {
    let u = V2::polar(1.0, dir);
    let (g0, g1) = (start * (at / start.dot(u)), end * (at / end.dot(u)));
    let mut segs = vec![line(g0, g1), line(g1, end)];
    segs.extend(run.iter().rev().map(Seg::reversed));
    segs.push(line(start, g0));
    join_ends(&mut segs);
    Outline { segs }
}

fn prism(outer: Vec<Outline>, z0: f64, height: f64, op: Op) -> Step {
    Step::Prism { region: Region { outer, holes: vec![] }, z0, height, twist: 0.0, op }
}

fn twisted(body: &Body) -> bool {
    body.steps.iter().any(|s| matches!(s, Step::Prism { twist, .. } if *twist != 0.0))
}

/// Teeth that aren't one straight prism of the outline: twisted, lofted (crowning) or stacked
/// (elephant-foot relief). Such pieces are cut from the gear's own steps.
fn shaped(body: &Body) -> bool {
    twisted(body)
        || body.steps.iter().any(|s| matches!(s, Step::Loft { .. } | Step::Taper { .. }))
        || body.steps.iter().filter(|s| matches!(s, Step::Prism { op: Op::New | Op::Join, .. })).count() > 1
}

/// The spec of the whole set a coupon is cut from: every member, no hubs, lightening holes,
/// shafts, chamfers, hardware or gear 2 face width and offset (the tooth tuning stays).
fn plain_set(s: &Spec) -> Spec {
    Spec {
        coupon: false,
        only: 0,
        holes: 0,
        hub_diameter: 0.0,
        hub_length: 0.0,
        face_width2: 0.0,
        face_offset2: 0.0,
        tooth_chamfer_front: Chamfer::default(),
        tooth_chamfer_back: Chamfer::default(),
        bore_chamfer: 0.0,
        hub_chamfer: 0.0,
        shaft_front: Shaft::default(),
        shaft_back: Shaft::default(),
        shaft2_front: Shaft::default(),
        shaft2_back: Shaft::default(),
        hardware: Hardware::default(),
        hardware2: Hardware::default(),
        ..s.clone()
    }
}

/// Whether the spec sets any of the body features a coupon leaves off (hubs and lightening
/// holes aside, which a coupon never had).
fn drops_body_features(s: &Spec) -> bool {
    let p = plain_set(s);
    let hw = |h: &Hardware| {
        h.set_screw > 0.0 || h.nut_width > 0.0 || h.insert_diameter > 0.0 || h.clamp_slit > 0.0 || h.clamp_screw > 0.0 || h.bearing_od > 0.0
    };
    [&s.shaft_front, &s.shaft_back, &s.shaft2_front, &s.shaft2_back].iter().any(|sh| sh.exists())
        || hw(&s.hardware)
        || hw(&s.hardware2)
        || s.tooth_chamfer_front.size > 0.0
        || s.tooth_chamfer_back.size > 0.0
        || s.bore_chamfer > 0.0
        || s.hub_chamfer > 0.0
        || s.face_width2 != p.face_width2
        || s.face_offset2 != p.face_offset2
}

/// The piece of one member.
fn piece(s: &Spec, src: &Source, n: u32) -> Piece {
    let m = s.module;
    let face = s.face_width;
    let g = src.preview;
    let z = src.teeth;
    let teeth_of = if src.ring { &g.region.holes[0] } else { &g.region.outer[0] };
    let (far, near) = radii(teeth_of, 1e-3 * m);
    // (Helical, crowned or elephant-foot teeth: cut from the gear's own steps.)
    let helical = shaped(src.body);
    let cut = |o: &Outline| prism(vec![o.clone()], -1.0, face + 2.0, Op::Cut);
    // The centre hole: the member's own bore, or a pin hole.
    let bore = src.bore.and_then(|b| b.outline().map(|o| (o, b.reach())));
    let pin = (0.25 * near).min(1.5);
    let hole_reach = bore.as_ref().map_or(pin, |b| b.1);
    let hole = bore.as_ref().map_or_else(|| Outline::circle(V2::ZERO, pin), |b| b.0.clone());
    let inscribed = bore.as_ref().map_or(pin, |b| radii(&b.0, 1e-3 * m).1);
    let name = |what: &str| if s.name.trim().is_empty() { format!("Coupon {what}") } else { format!("{} coupon {what}", s.name.trim()) };
    let whole = || -> Piece {
        let mut steps = src.body.steps.clone();
        if bore.is_none() && !src.ring {
            steps.push(cut(&hole));
        }
        Piece {
            who: g.name.clone(),
            body_name: name(&format!("{} z{z}", g.name)),
            center: g.center,
            region: if src.ring { g.region.clone() } else { Region { outer: vec![g.region.outer[0].clone()], holes: vec![hole.clone()] } },
            steps,
            locate: if src.ring { Locate::Free } else { Locate::Peg { radius: inscribed } },
            teeth: z,
            whole: true,
        }
    };
    if src.ring {
        // A block of the ring: never the whole of it.
        let n = n.min(z / 3).max(1);
        let Some(w) = window(teeth_of, z, n, src.dirs[0]) else { return whole() };
        let dir = src.dirs[0];
        let u = V2::polar(1.0, dir);
        if !(w.start.dot(u) > 0.2 * near && w.end.dot(u) > 0.2 * near) {
            return whole();
        }
        let at = far + 3.0 * m;
        let outline = ring_block(&w.segs, w.start, w.end, dir, at);
        let v = V2::polar(1.0, dir + PI / 2.0);
        let (g0, g1) = (w.start * (at / w.start.dot(u)), w.end * (at / w.end.dot(u)));
        let steps = if helical {
            // The whole ring, kept where a block with a plain arc for teeth covers it.
            let inner = near - m;
            let (a, b) = (w.start * (inner / w.start.len()), w.end * (inner / w.end.len()));
            let mask = ring_block(&[arc(a, b)], a, b, dir, at);
            let mut steps = src.body.steps.clone();
            steps.push(prism(vec![mask], -1.0, face + 2.0, Op::Intersect));
            steps
        } else {
            vec![prism(vec![outline.clone()], 0.0, face, Op::New)]
        };
        return Piece {
            who: g.name.clone(),
            body_name: name(&format!("{} z{z}", g.name)),
            center: g.center,
            region: Region { outer: vec![outline], holes: vec![] },
            steps,
            locate: Locate::Flat { dir, at, lo: g0.dot(v), hi: g1.dot(v), inward: -1.0 },
            teeth: w.teeth,
            whole: false,
        };
    }
    let sides = src.dirs.len() as u32;
    // Through the centre when the gear is small (or has teeth to show on two sides); the whole
    // gear when the windows would take most of it anyway.
    let centred = sides > 1 || near <= 16.0 * m;
    if centred {
        if 10 * (n + 1) * sides > 6 * z {
            return whole();
        }
        let windows: Option<Vec<Window>> = src.dirs.iter().map(|d| window(teeth_of, z, n, *d)).collect();
        let Some(windows) = windows else { return whole() };
        let boss = (hole_reach + 1.5 * m).min(0.5 * (hole_reach + near));
        let runs: Vec<(Vec<Seg>, V2, V2)> = windows.iter().map(|w| (w.segs.clone(), w.start, w.end)).collect();
        let outline = through_centre(&runs, boss);
        let steps = if helical {
            let out = far + m;
            let arcs: Vec<(Vec<Seg>, V2, V2)> = windows
                .iter()
                .map(|w| {
                    let (a, b) = (w.start * (out / w.start.len()), w.end * (out / w.end.len()));
                    (vec![arc(a, b)], a, b)
                })
                .collect();
            let mut steps = src.body.steps.clone();
            steps.push(prism(vec![through_centre(&arcs, boss)], -1.0, face + 2.0, Op::Intersect));
            if bore.is_none() {
                steps.push(cut(&hole));
            }
            steps
        } else {
            vec![prism(vec![outline.clone()], 0.0, face, Op::New), cut(&hole)]
        };
        return Piece {
            who: g.name.clone(),
            body_name: name(&format!("{} z{z}", g.name)),
            center: g.center,
            region: Region { outer: vec![outline], holes: vec![hole.clone()] },
            steps,
            locate: Locate::Peg { radius: inscribed },
            teeth: windows[0].teeth,
            whole: false,
        };
    }
    // A block with a flat back.
    let dir = src.dirs[0];
    let n = n.min(z / 3).max(1);
    let Some(w) = window(teeth_of, z, n, dir) else { return whole() };
    let (u, v) = (V2::polar(1.0, dir), V2::polar(1.0, dir + PI / 2.0));
    let at = w.start.dot(u).min(w.end.dot(u)) - 3.0 * m;
    let outline = flat_backed(&w.segs, w.start, w.end, dir, at);
    let steps = if helical {
        // The whole gear (without its bore), kept inside the block's rectangle: flat to beyond
        // the tips, side to side.
        let (y0, y1, out) = (w.start.dot(v), w.end.dot(v), far + m);
        let c = [u * at + v * y0, u * out + v * y0, u * out + v * y1, u * at + v * y1];
        let mask = Outline { segs: (0..4).map(|i| line(c[i], c[(i + 1) % 4])).collect() };
        let mut steps: Vec<Step> = src.body.steps.iter().filter(|st| !matches!(st, Step::Prism { op: Op::Cut, .. })).cloned().collect();
        steps.push(prism(vec![mask], -1.0, face + 2.0, Op::Intersect));
        steps
    } else {
        vec![prism(vec![outline.clone()], 0.0, face, Op::New)]
    };
    Piece {
        who: g.name.clone(),
        body_name: name(&format!("{} z{z}", g.name)),
        center: g.center,
        region: Region { outer: vec![outline], holes: vec![] },
        steps,
        locate: Locate::Flat { dir, at, lo: w.start.dot(v), hi: w.end.dot(v), inward: 1.0 },
        teeth: w.teeth,
        whole: false,
    }
}

/// Where a piece's reference (its centre hole, or its flat) is along `u` in the set's plane.
fn reference(p: &Piece, u: V2) -> (f64, &'static str) {
    match p.locate {
        Locate::Peg { .. } => (p.center.dot(u), "centre"),
        Locate::Flat { dir, at, .. } => (p.center.dot(u) + at * V2::polar(1.0, dir).dot(u), "flat"),
        Locate::Free => (p.center.dot(u), "centre"),
    }
}

/// The coupon of a pair or planetary set (see the module notes).
pub(crate) fn plan(s: &Spec) -> Result<Plan, String> {
    if !(MIN_TEETH..=MAX_TEETH).contains(&s.coupon_teeth) {
        return Err(format!("coupon teeth must be between {MIN_TEETH} and {MAX_TEETH}"));
    }
    // The whole set, plain: every member, no hubs, lightening holes, shafts, chamfers or
    // hardware (see `plain_set`).
    let full = crate::plan::plan(&plain_set(s))?;
    let pv = &full.preview;
    let toward = |a: usize, b: usize| (pv[b].center - pv[a].center).angle();
    let sources: Vec<Source> = match s.kind {
        Kind::Pair if pv.len() == 2 && full.bodies.len() == 2 => vec![
            Source { preview: &pv[0], body: &full.bodies[0], teeth: s.teeth, dirs: vec![toward(0, 1)], ring: false, bore: Some(&s.bore) },
            Source { preview: &pv[1], body: &full.bodies[1], teeth: s.teeth2, dirs: vec![toward(1, 0)], ring: false, bore: Some(&s.bore2) },
        ],
        Kind::Planetary if pv.len() >= 3 && full.bodies.len() == pv.len() => {
            let last = pv.len() - 1;
            let ring_teeth = if s.teeth3 > 0 { s.teeth3 } else { s.teeth + 2 * s.teeth2 };
            let out = toward(0, 1);
            vec![
                Source { preview: &pv[0], body: &full.bodies[0], teeth: s.teeth, dirs: vec![out], ring: false, bore: Some(&s.bore) },
                Source { preview: &pv[1], body: &full.bodies[1], teeth: s.teeth2, dirs: vec![out + PI, out], ring: false, bore: Some(&s.bore2) },
                Source { preview: &pv[last], body: &full.bodies[last], teeth: ring_teeth, dirs: vec![out], ring: true, bore: None },
            ]
        }
        _ => return Err("a test coupon needs every member of the set".into()),
    };
    let m = s.module;
    let face = s.face_width;
    let pieces: Vec<Piece> = sources.iter().map(|src| piece(s, src, s.coupon_teeth)).collect();
    let mut p = Plan::default();
    p.facts = full.facts.clone();
    p.warnings = full.warnings.clone();
    if drops_body_features(s) {
        p.warn("the test coupon leaves off shafts, chamfers, hub hardware, bearing pockets and gear 2's own face width; the teeth keep their tuning");
    }
    p.preview_note = "test coupon: a few teeth of each member, at the true centre distance".into();
    let origin = |c: V2| Frame::at(s.origin).shifted([c.x, c.y, 0.0]);
    for (k, pc) in pieces.iter().enumerate() {
        if pc.whole {
            p.fact(format!("coupon {}: the whole gear, teeth", pc.who), f64::from(pc.teeth), Unit::None);
        } else {
            p.fact(format!("coupon {}: teeth", pc.who), f64::from(pc.teeth), Unit::None);
        }
        match pc.locate {
            Locate::Peg { radius } => p.fact(format!("coupon {}: centre hole (inscribed diameter)", pc.who), 2.0 * radius, Unit::Mm),
            Locate::Flat { at, .. } => p.fact(format!("coupon {}: flat edge to the gear's centre", pc.who), at, Unit::Mm),
            Locate::Free => {}
        }
        if s.only == 0 || usize::from(s.only) == k + 1 {
            p.bodies.push(Body { name: pc.body_name.clone(), frame: origin(pc.center), steps: pc.steps.clone() });
        }
        let src = sources[k].preview;
        p.preview.push(PreviewGear { region: pc.region.clone(), ..src.clone() });
    }
    // The size to check each mesh by: between the two pieces' references along the line of
    // centres, with the pieces at the true centre distance.
    for w in pieces.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let u = if a.center.dist(b.center) > 1e-9 * m { (b.center - a.center).norm() } else { V2::new(1.0, 0.0) };
        let ((xa, ka), (xb, kb)) = (reference(a, u), reference(b, u));
        p.fact(format!("coupon: {} {ka} to {} {kb}", a.who, b.who), (xb - xa).abs(), Unit::Mm);
    }
    // The base: a plate under everything, a peg in every centre hole, a fence behind every flat.
    if s.only == 0 {
        let clearance = |r: f64| (0.05 * r).min(0.1);
        let mut pegs = Vec::new();
        let mut fences = Vec::new();
        let (mut lo, mut hi) = (V2::new(f64::INFINITY, f64::INFINITY), V2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        let mut cover = |q: V2| {
            lo = V2::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = V2::new(hi.x.max(q.x), hi.y.max(q.y));
        };
        for pc in &pieces {
            for q in crate::mesh2d::polygon(&pc.region.outer[0], 1e-2 * m) {
                cover(pc.center + q);
            }
            match pc.locate {
                Locate::Peg { radius } => {
                    let r = radius - clearance(radius);
                    pegs.push(Outline::circle(pc.center, r));
                    p.fact(format!("coupon base: {} peg diameter", pc.who), 2.0 * r, Unit::Mm);
                }
                Locate::Flat { dir, at, lo: y0, hi: y1, inward } => {
                    // Behind the flat, 2.5 modules thick.
                    let (u, v) = (V2::polar(1.0, dir), V2::polar(1.0, dir + PI / 2.0));
                    let back = at - inward * 2.5 * m;
                    let corners = [u * at + v * y0, u * at + v * y1, u * back + v * y1, u * back + v * y0];
                    // Counter-clockwise whichever side the fence is on.
                    let c: Vec<V2> = if inward > 0.0 {
                        corners.iter().map(|q| pc.center + *q).collect()
                    } else {
                        corners.iter().rev().map(|q| pc.center + *q).collect()
                    };
                    for q in &c {
                        cover(*q);
                    }
                    fences.push(Outline { segs: (0..4).map(|i| line(c[i], c[(i + 1) % 4])).collect() });
                }
                Locate::Free => {}
            }
        }
        let margin = V2::new(m, m);
        let thick = (1.5 * m).clamp(2.0f64.min(face), 6.0f64.max(0.2 * face));
        let mut steps = vec![prism(vec![rect(lo - margin, hi + margin)], -thick, thick, Op::New)];
        if !pegs.is_empty() {
            steps.push(prism(pegs, 0.0, face, Op::Join));
        }
        if !fences.is_empty() {
            steps.push(prism(fences, 0.0, face, Op::Join));
        }
        p.fact("coupon base: thickness", thick, Unit::Mm);
        let base = if s.name.trim().is_empty() { "Coupon base".to_string() } else { format!("{} coupon base", s.name.trim()) };
        p.bodies.push(Body { name: base, frame: Frame::at(s.origin), steps });
    }
    if pieces.iter().all(|pc| pc.whole) {
        p.warn(format!("the gears are too small to cut a {}-tooth coupon from: the coupon is the whole set", num(f64::from(s.coupon_teeth))));
    }
    if p.bodies.is_empty() {
        return Err("nothing to build with these choices".into());
    }
    p.meshes = crate::mesh2d::topology(s, &p.preview);
    Ok(p)
}
