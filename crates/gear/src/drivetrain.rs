//! A drivetrain read from 3D geometry: a 3D sketch of the whole drivetrain, or a selection of
//! sketch and model geometry. Each thing read is a member:
//!
//! - a circle is a gear's pitch circle: its centre and the normal to its plane are the gear's
//!   axis (on a shaft line, when one is drawn along it);
//! - a line is a shaft, in any direction (a line asked to be a gear carries one, sized by its
//!   pitch diameter, or teeth and module, `along` it from its first end);
//! - a point lying on a shaft line is a bearing seat (elsewhere, a point is only a dot);
//! - circles on one shaft at different places along it are a cluster (compound gears, joined
//!   as one part); circles touching in one plane mesh.
//!
//! The things may come from several sketches on different planes (and 3D sketches), read
//! together in model space: a shaft line in one sketch, circles in sketches square to it.
//!
//! Gears on parallel shafts (any number) are a gear train ([`crate::sketch_rules::size_train`],
//! planned by [`crate::plan::train`]); two gears on shafts at an angle are the set their shafts
//! imply ([`crate::select::set_on_shafts`]: meeting a bevel pair, skew crossed helicals or a worm
//! and wheel). More gears with some at an angle (a train with a bevel or crossed stage) are
//! planned stage by stage by [`crate::plan::system`] and come back as [`Outcome::planned`].
//! Meshing gears share one tooth style; gears on one shaft may differ. Anything wrong is said
//! per member ([`Member::error`]), naming it.

use crate::axes::{Axis, Solved, add, cross, dot, len, mul, num, square_to, sub, unit};
use crate::geom::V2;
use crate::plan::{Frame, Kind, SidePreset, Spec};
use crate::select::{Ask as ShaftAsk, Shaft, set_on_shafts};
use crate::simple::Simple;
use crate::sketch_rules::{Circle, TOLERANCE, TrainCircle, is_style, size_train};

type V = [f64; 3];

/// A circle in space: centre, the unit normal to its plane, radius (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle3 {
    pub c: V,
    pub n: V,
    pub r: f64,
}

/// A straight line in space through `a` and `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line3 {
    pub a: V,
    pub b: V,
}

impl Line3 {
    pub fn dir(&self) -> Option<V> {
        unit(sub(self.b, self.a))
    }
    /// How far `p` is from the endless line.
    pub fn distance(&self, p: V) -> f64 {
        match self.dir() {
            Some(u) => {
                let q = sub(p, self.a);
                len(sub(q, mul(u, dot(q, u))))
            }
            None => len(sub(p, self.a)),
        }
    }
}

/// One thing read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Thing {
    Circle(Circle3),
    Line(Line3),
    Point(V),
}

/// What a member is asked to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A gear of this tooth style or family (spur, helical, herringbone, bevel, spiral bevel,
    /// worm, face gear, rack...).
    Gear(Kind),
    /// A worm's wheel.
    Wheel,
    /// A ring gear (teeth inside).
    Internal,
    Shaft,
    Bearing,
}

impl Role {
    /// Its name in `gear.create`'s `selection` and `members`.
    pub fn name(self) -> &'static str {
        match self {
            Role::Gear(Kind::Helical) => "helical",
            Role::Gear(Kind::Herringbone) => "herringbone",
            Role::Gear(Kind::Bevel) => "bevel",
            Role::Gear(Kind::SpiralBevel) => "spiral_bevel",
            Role::Gear(Kind::Worm) => "worm",
            Role::Gear(Kind::Rack) => "rack",
            Role::Gear(Kind::FaceGear) => "face",
            Role::Gear(_) => "spur",
            Role::Wheel => "wheel",
            Role::Internal => "internal",
            Role::Shaft => "shaft",
            Role::Bearing => "bearing",
        }
    }
    /// The role named `s` (as [`Role::name`]).
    pub fn parse(s: &str) -> Option<Role> {
        Some(match s {
            "spur" => Role::Gear(Kind::Spur),
            "helical" => Role::Gear(Kind::Helical),
            "herringbone" => Role::Gear(Kind::Herringbone),
            "bevel" => Role::Gear(Kind::Bevel),
            "spiral_bevel" => Role::Gear(Kind::SpiralBevel),
            "worm" => Role::Gear(Kind::Worm),
            "rack" => Role::Gear(Kind::Rack),
            "face" => Role::Gear(Kind::FaceGear),
            "wheel" => Role::Wheel,
            "internal" => Role::Internal,
            "shaft" => Role::Shaft,
            "bearing" => Role::Bearing,
            // Worked out from the geometry (as no role at all).
            "auto" => return None,
            _ => return None,
        })
    }
    fn is_gear(self) -> bool {
        !matches!(self, Role::Shaft | Role::Bearing)
    }
}

/// What is asked of one member besides its geometry (all optional).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Ask {
    pub role: Option<Role>,
    pub module: Option<f64>,
    pub teeth: Option<u32>,
    pub face_width: Option<f64>,
    /// A gear on a shaft line: its pitch diameter.
    pub pitch_diameter: Option<f64>,
    /// On a shaft line: where its gear or bearing sits, mm from its first end.
    pub along: Option<f64>,
    /// A gear: its helix angle (degrees; 0 straight) and V (herringbone) teeth. Its meshing
    /// partners get the opposite hand; a helix on a gear of no asked type makes it helical.
    pub helix: Option<f64>,
    pub herringbone: Option<bool>,
    /// A shaft (a line, or a gear's axis): build its shaft body (default yes) and its diameter
    /// (default from the smallest gear's root).
    pub shaft_body: Option<bool>,
    pub shaft_diameter: Option<f64>,
    /// A gear on parallel shafts: which side of its sketch plane its face extends along its axis
    /// (`None`: all on side A, as before sides existed; see [`SidePreset`]).
    pub sides: Option<SidePreset>,
}

/// One member read: its geometry and what is asked of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub thing: Thing,
    pub ask: Ask,
}

/// What a member became ([`read`]); `index` is its place in the items.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Member {
    pub index: usize,
    pub role: &'static str,
    pub module: Option<f64>,
    pub teeth: Option<u32>,
    pub pitch_diameter: Option<f64>,
    pub face_width: Option<f64>,
    pub meshes_with: Vec<usize>,
    pub shaft: Option<usize>,
    pub cluster: Option<usize>,
    pub error: Option<String>,
    /// Its role was worked out from the geometry (none, or `auto`, was asked).
    pub inferred: bool,
    /// A helical gear's hand ("RH", "LH"), and a gear's shaft angle to its mate's (deg) when
    /// they are at an angle.
    pub hand: Option<&'static str>,
    pub shaft_angle: Option<f64>,
}

/// What [`read`] made of the items.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Outcome {
    /// The set placed where the geometry is (none when a member is wrong).
    pub solved: Option<Solved>,
    /// One per item, in their order.
    pub members: Vec<Member>,
    /// The bearing seats (world points on their shafts), for the keep-out body's seats.
    pub seats: Vec<V>,
    pub notes: Vec<String>,
    /// Why there is no set: every member's error in one line, or one about the whole.
    pub error: Option<String>,
    /// The gears in the set's order (its train's gears, `Plan::members`): each one's item.
    pub gears: Vec<usize>,
    /// A drivetrain of stages of several kinds (a train with a bevel or crossed stage among
    /// more than two gears) has no one spec: its plan, made by
    /// [`crate::plan::system::plan_items`] (`solved` is `None`; `gears` in its members' order).
    pub planned: Option<crate::plan::Plan>,
}

/// Lines this close to parallel (radians) are parallel.
const PARALLEL: f64 = 1e-3;

fn parallel(a: V, b: V) -> bool {
    len(cross(a, b)) <= PARALLEL
}

/// A member's name for people: its place in what was read.
fn who(_items: &[Item], i: usize) -> String {
    format!("member {}", i + 1)
}

/// The gear an item is, where it is one by its geometry and size: a circle (not asked to be a
/// shaft or bearing), or a line given its gear's pitch diameter (or teeth and module).
fn sized_gear(it: &Item) -> Option<Circle3> {
    match it.thing {
        Thing::Circle(c) if !matches!(it.ask.role, Some(Role::Shaft | Role::Bearing)) => Some(Circle3 { n: unit(c.n)?, ..c }),
        Thing::Line(l) if !matches!(it.ask.role, Some(Role::Shaft | Role::Bearing)) => {
            let u = l.dir()?;
            let d = it.ask.pitch_diameter.or_else(|| Some(it.ask.module? * f64::from(it.ask.teeth?)))?;
            (d > 0.0 && d.is_finite()).then(|| Circle3 { c: add(l.a, mul(u, it.ask.along.unwrap_or(0.0))), n: u, r: d / 2.0 })
        }
        _ => None,
    }
}

/// Selected shafts given no size among gears on parallel shafts: each the shaft of a gear whose
/// pitch circle fills the spacing to the nearest gear (as [`crate::select`] fills a chain of
/// shafts), in that gear's plane, where that circle crosses no other gear's. A shaft that
/// carries a gear already, or is asked to be only a shaft, stays as it is. The notes say what
/// was filled ("member 2: Ø60 fills the spacing to member 1").
pub fn fill_unsized_shafts(items: &mut [Item]) -> Vec<String> {
    let gears: Vec<(usize, Circle3)> = items.iter().enumerate().filter_map(|(i, it)| Some((i, sized_gear(it)?))).collect();
    let mut notes = Vec::new();
    for (i, slot) in items.iter_mut().enumerate() {
        let it = *slot;
        let Thing::Line(l) = it.thing else { continue };
        if it.ask.role.is_some() || it.ask.pitch_diameter.is_some() || it.ask.teeth.is_some() {
            continue;
        }
        let Some(u) = l.dir() else { continue };
        let near: Vec<(usize, Circle3, f64)> = gears.iter().filter(|(_, c)| parallel(c.n, u)).map(|(j, c)| (*j, *c, l.distance(c.c))).collect();
        // A gear on it already: it is that gear's shaft.
        if near.iter().any(|(_, c, d)| *d <= 1e-6 + TOLERANCE * c.r) {
            continue;
        }
        let Some(&(j, c, d)) = near.iter().min_by(|a, b| a.2.total_cmp(&b.2)) else { continue };
        let r = d - c.r;
        if !(r > 0.0) || near.iter().any(|(_, o, e)| e - o.r < r - (1e-6 + TOLERANCE * r)) {
            continue;
        }
        slot.ask.pitch_diameter = Some(2.0 * r);
        if it.ask.along.is_none() {
            slot.ask.along = Some(dot(sub(c.c, l.a), u));
        }
        notes.push(format!("member {}: Ø{} fills the spacing to member {}", i + 1, num(2.0 * r), j + 1));
    }
    notes
}

/// Reads the items as one drivetrain (see the module docs). `style` is the tooth style of gears
/// asked nothing else (default spur); `settings` the set's own (module, face width, helix,
/// hand, `exact`); `base` the rest of the spec (bores, tooth form...).
pub fn read(items: &[Item], style: Option<Kind>, settings: &Simple, base: &Spec) -> Outcome {
    read_with(items, style, settings, base, true)
}

/// [`read`], with shafts built on the gears' axes (`shafts`; each shaft or gear may still turn
/// its own off) or not.
pub fn read_with(items: &[Item], style: Option<Kind>, settings: &Simple, base: &Spec, shafts: bool) -> Outcome {
    let n = items.len();
    let style = style.unwrap_or(Kind::Spur);
    let mut out = Outcome { members: (0..n).map(|i| Member { index: i, ..Member::default() }).collect(), ..Outcome::default() };
    let mut err: Vec<Option<String>> = vec![None; n];
    let fail = |err: &mut Vec<Option<String>>, i: usize, e: String| {
        if err[i].is_none() {
            err[i] = Some(e);
        }
    };
    // Geometry that makes nothing.
    for (i, it) in items.iter().enumerate() {
        let ok = match it.thing {
            Thing::Circle(c) => c.r > 0.0 && c.r.is_finite() && c.c.iter().all(|v| v.is_finite()) && unit(c.n).is_some(),
            Thing::Line(l) => l.dir().is_some() && l.a.iter().chain(l.b.iter()).all(|v| v.is_finite()),
            Thing::Point(p) => p.iter().all(|v| v.is_finite()),
        };
        if !ok {
            fail(&mut err, i, format!("{}: its geometry is degenerate (a zero radius or a line of no length)", who(items, i)));
        }
    }
    let lines: Vec<usize> = (0..n).filter(|i| matches!(items[*i].thing, Thing::Line(_)) && err[*i].is_none()).collect();
    let line = |i: usize| match items[i].thing {
        Thing::Line(l) => Some(l),
        _ => None,
    };
    // The roles: what is asked, else a circle is a gear of the style, a line a shaft, and a point
    // on a shaft line a bearing.
    let on_line = |p: V, size: f64| lines.iter().copied().find(|&k| line(k).is_some_and(|l| l.distance(p) <= 1e-6 + TOLERANCE * size.max(1.0) * 0.1));
    let mut role: Vec<Option<Role>> = vec![None; n];
    for (i, it) in items.iter().enumerate() {
        role[i] = Some(match (it.thing, it.ask.role) {
            (Thing::Point(_), Some(r)) if r.is_gear() => {
                fail(&mut err, i, format!("{}: a point is a bearing seat (on a shaft line), not a {}", who(items, i), r.name()));
                r
            }
            (Thing::Circle(_), Some(Role::Shaft | Role::Bearing)) => {
                fail(
                    &mut err,
                    i,
                    format!("{}: a circle is a gear's pitch circle; select its shaft line or a point on it for a shaft or bearing", who(items, i)),
                );
                Role::Gear(style)
            }
            (_, Some(r)) => r,
            (Thing::Circle(_), None) => Role::Gear(style),
            (Thing::Line(_), None) if it.ask.pitch_diameter.is_some() || it.ask.teeth.is_some() => Role::Gear(style),
            (Thing::Line(_), None) => Role::Shaft,
            (Thing::Point(_), None) => Role::Bearing,
        });
        out.members[i].role = role[i].map_or("", Role::name);
        out.members[i].inferred = it.ask.role.is_none();
    }
    // The gears: circles, and lines carrying a gear `along` them.
    let mut gears: Vec<(usize, Circle3)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if err[i].is_some() || !role[i].is_some_and(Role::is_gear) {
            continue;
        }
        match it.thing {
            Thing::Circle(c) => {
                let n = unit(c.n).unwrap_or([0.0, 0.0, 1.0]);
                let c = Circle3 { n, ..c };
                // The shaft line its axis lies along, if one is drawn (else its axis is its shaft:
                // a drivetrain may be drawn in several sketches, its shafts in only some).
                out.members[i].shaft = lines
                    .iter()
                    .copied()
                    .find(|&k| line(k).is_some_and(|l| l.dir().is_some_and(|u| parallel(u, n)) && l.distance(c.c) <= 1e-6 + TOLERANCE * c.r));
                gears.push((i, c));
            }
            Thing::Line(l) => {
                let u = l.dir().unwrap_or([0.0, 0.0, 1.0]);
                let cb = 1.0; // the pitch diameter is asked as it is
                let d = it.ask.pitch_diameter.or_else(|| Some(it.ask.module? * f64::from(it.ask.teeth?) / cb));
                match d {
                    Some(d) if d > 0.0 && d.is_finite() => {
                        let c = add(l.a, mul(u, it.ask.along.unwrap_or(0.0)));
                        out.members[i].shaft = Some(i);
                        gears.push((i, Circle3 { c, n: u, r: d / 2.0 }));
                    }
                    _ => fail(&mut err, i, format!("{}: give the gear on this shaft a pitch diameter (or teeth and a module)", who(items, i))),
                }
            }
            Thing::Point(_) => {}
        }
    }
    // Bearings: points on a shaft line, or `along` a shaft line asked to be a bearing.
    for (i, it) in items.iter().enumerate() {
        if err[i].is_some() || role[i] != Some(Role::Bearing) {
            continue;
        }
        match it.thing {
            Thing::Point(p) => match on_line(p, 1.0) {
                Some(k) => {
                    out.members[i].shaft = Some(k);
                    out.seats.push(p);
                }
                None if it.ask.role == Some(Role::Bearing) => {
                    fail(&mut err, i, format!("{}: a bearing must lie on a shaft line, and this point is on none", who(items, i)))
                }
                // A dot (an origin), not a bearing.
                None => out.members[i].role = "point",
            },
            Thing::Line(l) => {
                let u = l.dir().unwrap_or([0.0, 0.0, 1.0]);
                out.members[i].shaft = Some(i);
                out.seats.push(add(l.a, mul(u, it.ask.along.unwrap_or(0.0))));
            }
            Thing::Circle(_) => {}
        }
    }
    let finish = |mut out: Outcome, err: Vec<Option<String>>| -> Outcome {
        let mut seen: Vec<String> = Vec::new();
        for (m, e) in out.members.iter_mut().zip(err) {
            if let Some(e) = &e
                && !seen.contains(e)
            {
                seen.push(e.clone());
            }
            m.error = e;
        }
        if !seen.is_empty() && out.error.is_none() {
            out.error = Some(seen.join("; "));
        }
        if out.error.is_some() {
            out.solved = None;
        }
        out
    };
    if gears.is_empty() {
        if out.error.is_none() && err.iter().all(Option::is_none) {
            out.error =
                Some("nothing to make a gear of: draw a pitch circle (on its shaft line), or ask a shaft for a gear with its pitch diameter".into());
        }
        return finish(out, err);
    }
    // Every gear's shaft parallel to gear 1's: a train in the plane square to them.
    let n0 = gears[0].1.n;
    let all_parallel = gears.iter().all(|(_, c)| parallel(c.n, n0));
    out.gears = gears.iter().map(|g| g.0).collect();
    if all_parallel {
        train(items, &gears, &role, style, settings, base, shafts, &mut out, &mut err);
    } else if gears.len() == 2 {
        crossed(items, &gears, &role, style, settings, base, &mut out, &mut err);
    } else {
        // Stages of several kinds (parallel trains, bevel, crossed helical): the items' system
        // planner, each stage by its own planner, merged into one plan.
        crate::plan::system::read_mixed(items, &gears, &role, style, base, &mut out, &mut err);
    }
    finish(out, err)
}

/// Gears on parallel shafts: a train (see [`read`]).
#[allow(clippy::too_many_arguments)]
fn train(
    items: &[Item],
    gears: &[(usize, Circle3)],
    role: &[Option<Role>],
    style: Kind,
    settings: &Simple,
    base: &Spec,
    shafts: bool,
    out: &mut Outcome,
    err: &mut [Option<String>],
) {
    // Each gear's tooth style (its own); meshing gears share one (checked after the sizing finds
    // the meshes), gears on one shaft may differ.
    let mut chosen: Option<(usize, Kind)> = None;
    let mut styles: Vec<Option<Kind>> = vec![None; items.len()];
    for &(i, _) in gears {
        let k = match role[i] {
            Some(Role::Gear(k)) if is_style(k) => k,
            Some(Role::Gear(k)) => {
                err[i].get_or_insert_with(|| {
                    format!("{}: a {} needs its shaft at an angle to its mate's; these shafts are parallel", who(items, i), Role::Gear(k).name())
                });
                continue;
            }
            Some(Role::Wheel) => {
                err[i].get_or_insert_with(|| format!("{}: a worm wheel needs a worm on a shaft square to its own", who(items, i)));
                continue;
            }
            _ => continue,
        };
        styles[i] = Some(k);
        if chosen.is_none() {
            chosen = Some((i, k));
        }
    }
    let style = chosen.map_or(style, |c| c.1);
    // A gear asked to be helical or herringbone where the set's style is another: the set's helix
    // angle (as the sizing takes it), its V teeth its own.
    let helix_default = settings.helix.filter(|h| *h != 0.0).map(f64::abs).unwrap_or(if base.helix != 0.0 { base.helix.abs() } else { 20.0 });
    // The plane square to the shafts through gear 1's centre; each circle's place in it and
    // along the shafts.
    let flip = |c: &Circle3| if dot(c.n, gears[0].1.n) < 0.0 { mul(c.n, -1.0) } else { c.n };
    let z = gears[0].1.n;
    let x = square_to(z);
    let origin = gears[0].1.c;
    let frame = Frame { origin, x, z };
    let y = frame.y();
    let circles: Vec<TrainCircle> = gears
        .iter()
        .map(|(i, c)| {
            let _ = flip(c);
            let q = sub(c.c, origin);
            let a = &items[*i].ask;
            // Its shaft line, if drawn: the shaft runs between its ends; the line asks too.
            let line = out.members[*i].shaft.map(|k| (k, items[k]));
            let span = line.and_then(|(_, it)| match it.thing {
                Thing::Line(l) => Some((dot(sub(l.a, origin), z), dot(sub(l.b, origin), z))),
                _ => None,
            });
            let line_ask = line.map(|(_, it)| it.ask).unwrap_or_default();
            let on = shafts && a.shaft_body != Some(false) && line_ask.shaft_body != Some(false);
            TrainCircle {
                circle: Circle { c: V2::new(dot(q, x), dot(q, y)), r: c.r },
                z: Some(dot(q, z)),
                module: a.module,
                teeth: a.teeth,
                face_width: a.face_width,
                ring: (role[*i] == Some(Role::Internal)).then_some(true),
                shaft: on,
                shaft_span: span,
                shaft_diameter: a.shaft_diameter.or(line_ask.shaft_diameter),
                helix: a.helix.or(match role[*i] {
                    Some(Role::Gear(Kind::Spur)) if a.role.is_some() => Some(0.0),
                    Some(Role::Gear(k @ (Kind::Helical | Kind::Herringbone))) if a.role.is_some() && k != style => Some(helix_default),
                    _ => None,
                }),
                herringbone: a.herringbone.or(match role[*i] {
                    Some(Role::Gear(Kind::Herringbone)) if a.role.is_some() => Some(true),
                    Some(Role::Gear(Kind::Spur | Kind::Helical)) if a.role.is_some() && style == Kind::Herringbone => Some(false),
                    _ => None,
                }),
                // Side A along the gear's own axis: turned round where it points against the
                // train's z.
                sides: a.sides.map(|s| if dot(c.n, z) < 0.0 { s.swapped() } else { s }),
            }
        })
        .collect();
    let sized = size_train(&circles, style, settings, base);
    out.notes.extend(sized.notes.iter().map(|n| renamed(n, items, gears)));
    for (k, (i, _)) in gears.iter().enumerate() {
        let m = &sized.members[k];
        let mem = &mut out.members[*i];
        if m.teeth > 0 {
            (mem.module, mem.teeth, mem.pitch_diameter, mem.face_width) = (Some(m.module), Some(m.teeth), Some(m.pitch_diameter), Some(m.face_width));
        }
        mem.meshes_with = m.meshes_with.iter().map(|j| gears[*j].0).collect();
        mem.cluster = m.cluster;
        if let Some(e) = &sized.errors[k] {
            err[*i].get_or_insert_with(|| renamed(e, items, gears));
        }
    }
    // Meshing gears share one tooth style (gears on one shaft may differ).
    for (k, (i, _)) in gears.iter().enumerate() {
        for &j in &sized.members[k].meshes_with {
            let Some(&(jj, _)) = gears.get(j) else { continue };
            if let (Some(a), Some(b)) = (styles[*i], styles[jj])
                && a != b
            {
                err[*i].get_or_insert_with(|| {
                    format!(
                        "{} is {} but {} it meshes with is {}: meshing gears share one tooth style",
                        who(items, *i),
                        Role::Gear(a).name(),
                        who(items, jj),
                        Role::Gear(b).name()
                    )
                });
            }
        }
    }
    if let (Some(e), true) = (&sized.error, sized.errors.iter().all(Option::is_none)) {
        out.error = Some(renamed(e, items, gears));
    }
    if let Some(mut spec) = sized.spec {
        // Each gear's resolved type: helical where its helix isn't 0, V teeth herringbone.
        if let Some(t) = &spec.train {
            for (k, (i, _)) in gears.iter().enumerate() {
                if let Some(g) = t.gears.get(k).filter(|g| g.helix != 0.0) {
                    if !g.ring {
                        out.members[*i].role = if g.herringbone { "herringbone" } else { "helical" };
                    }
                    out.members[*i].hand = Some(if g.helix > 0.0 { "RH" } else { "LH" });
                }
            }
        }
        spec.origin = [0.0; 3];
        spec.placement = Some(frame);
        out.solved = Some(Solved { spec, relation: None, frames: [frame; 2], facts: Vec::new(), notes: Vec::new(), derived: Default::default() });
    }
}

/// The sizing's words name gears by their place among the gears ("gear 3"): named as the
/// members they are ("member 5").
fn renamed(text: &str, items: &[Item], gears: &[(usize, Circle3)]) -> String {
    crate::sketch_rules::rename_gears(text, &|k| gears.get(k).map_or_else(|| format!("gear {}", k + 1), |g| who(items, g.0)))
}

/// Two gears on shafts at an angle: the set their shafts imply (see [`read`]).
#[allow(clippy::too_many_arguments)]
fn crossed(
    items: &[Item],
    gears: &[(usize, Circle3)],
    role: &[Option<Role>],
    style: Kind,
    settings: &Simple,
    base: &Spec,
    out: &mut Outcome,
    err: &mut [Option<String>],
) {
    let (mut g1, mut g2) = (gears[0], gears[1]);
    // A family asked (bevel, worm...), from either gear's role; a worm's wheel goes first.
    let kind_of = |i: usize| match role[i] {
        Some(Role::Gear(k)) if !is_style(k) => Some(k),
        Some(Role::Wheel) => Some(Kind::Worm),
        _ => None,
    };
    let family = kind_of(g1.0).or(kind_of(g2.0));
    if role[g1.0] == Some(Role::Gear(Kind::Worm)) || role[g2.0] == Some(Role::Wheel) {
        std::mem::swap(&mut g1, &mut g2);
    }
    // A hypoid set is its pinion (the smaller) and then its ring gear.
    if family == Some(Kind::Hypoid) && g1.1.r > g2.1.r {
        std::mem::swap(&mut g1, &mut g2);
    }
    out.gears = vec![g1.0, g2.0];
    if let (Some(a), Some(b)) = (kind_of(g1.0), kind_of(g2.0))
        && a != b
    {
        err[g2.0].get_or_insert_with(|| {
            format!("{} is a {} but {} is a {}: a pair is one kind", who(items, g2.0), Role::Gear(b).name(), who(items, g1.0), Role::Gear(a).name())
        });
        return;
    }
    let axis = |c: &Circle3| Axis::new(c.c, c.n).unwrap_or(Axis { origin: c.c, dir: [0.0, 0.0, 1.0] });
    let (a1, a2) = (axis(&g1.1), axis(&g2.1));
    // The pitch circles must touch: a bevel pair's at the same distance from the apex, crossed
    // gears' shafts their radii apart.
    match crate::axes::relate(&a1, &a2) {
        crate::axes::Relation::Intersecting { apex, .. } => {
            let cone = |c: &Circle3| (len(sub(c.c, apex)).powi(2) + c.r * c.r).sqrt();
            let (r1, r2) = (cone(&g1.1), cone(&g2.1));
            if (r1 - r2).abs() > 1e-6 + TOLERANCE * r1.min(r2) && family != Some(Kind::FaceGear) {
                err[g2.0].get_or_insert_with(|| {
                    format!(
                        "{} and {} don't touch: a bevel pair's pitch circles meet at one point, the same distance from where the shafts meet ({} and {} mm)",
                        who(items, g1.0),
                        who(items, g2.0),
                        num(r1),
                        num(r2)
                    )
                });
                return;
            }
        }
        crate::axes::Relation::Skew { offset, .. }
            if !matches!(family, Some(Kind::Worm | Kind::Hypoid)) && (offset - g1.1.r - g2.1.r).abs() > 1e-6 + TOLERANCE * g1.1.r.min(g2.1.r) =>
        {
            err[g2.0].get_or_insert_with(|| {
                format!(
                    "{} and {} don't touch: their shafts are {} mm apart and their radii add up to {} mm",
                    who(items, g1.0),
                    who(items, g2.0),
                    num(offset),
                    num(g1.1.r + g2.1.r)
                )
            });
            return;
        }
        _ => {}
    }
    let ask = |i: usize| items[i].ask;
    let modules: Vec<f64> = [g1.0, g2.0].iter().filter_map(|i| ask(*i).module).collect();
    if modules.len() == 2 && (modules[0] - modules[1]).abs() > 1e-9 {
        err[g2.0].get_or_insert_with(|| {
            format!(
                "{} asks module {} but {} it meshes with asks {}: meshing gears share one module",
                who(items, g2.0),
                num(modules[1]),
                who(items, g1.0),
                num(modules[0])
            )
        });
        return;
    }
    let shafts = [
        Shaft { axis: a1, pitch_diameter: Some(2.0 * g1.1.r), teeth: ask(g1.0).teeth },
        Shaft { axis: a2, pitch_diameter: Some(2.0 * g2.1.r), teeth: ask(g2.0).teeth },
    ];
    let helical = matches!(role[g1.0], Some(Role::Gear(Kind::Helical | Kind::Herringbone))) || matches!(style, Kind::Helical | Kind::Herringbone);
    let kind = family.or(helical.then_some(Kind::Helical)).or(Some(style));
    let sa = ShaftAsk {
        kind,
        module: modules.first().copied().or(settings.module),
        ratio: None,
        along: 0.0,
        face_width: ask(g1.0).face_width.or(settings.face_width),
        helix: settings.helix,
        left_hand: settings.left_hand == Some(true),
    };
    match set_on_shafts(&shafts, &sa, base) {
        Ok(solved) => {
            let s = &solved.spec;
            for (i, z) in [(g1.0, s.teeth), (g2.0, s.teeth2)] {
                let m = &mut out.members[i];
                (m.module, m.teeth, m.pitch_diameter, m.face_width) = (Some(s.module), Some(z), Some(s.module * f64::from(z)), Some(s.face_width));
            }
            out.members[g1.0].meshes_with = vec![g2.0];
            out.members[g2.0].meshes_with = vec![g1.0];
            for i in [g1.0, g2.0] {
                out.members[i].shaft_angle = Some(s.shaft_angle);
                out.members[i].role = Role::Gear(s.kind).name();
            }
            if s.kind == Kind::Worm {
                out.members[g1.0].role = "wheel";
                out.members[g2.0].role = "worm";
            }
            out.notes.extend(solved.notes.iter().cloned());
            out.solved = Some(solved);
        }
        Err(e) => {
            err[g2.0].get_or_insert_with(|| format!("{} and {}: {e}", who(items, g1.0), who(items, g2.0)));
        }
    }
}
