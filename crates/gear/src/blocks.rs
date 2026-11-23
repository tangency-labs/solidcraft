//! Blocks: gears put under one block share its settings. A block ([`ModuleBlock`]) holds a
//! module (decision 2: [`crate::items::resolve_module`]) and, optionally, the rest of
//! [`BlockSettings`]: tooth chamfer, pressure angle, helix angle, face width, backlash and shaft
//! Ø. A member gear's own value stays as its override; where it gives none (Auto), the block's
//! applies.
//!
//! - [`with_blocks`]: the items with their blocks' helix, face and bore filled in;
//!   [`block_spec`]: a unit's spec with its block's pressure angle and backlash;
//!   [`chamfer_groups`]: which items each tooth chamfer goes on.
//! - [`block_shafts`]: a block with a shaft Ø gets one shaft per distinct axis its gears are on,
//!   over the gears' faces and a margin past them (or out to the bearing points on that axis);
//!   [`expand_blocks`] adds each as a Shaft item joined to the gears on its axis, so the shaft
//!   and its gears are one body (the planners' own Shaft machinery,
//!   [`crate::plan::system::plan_items`]). A gear on that axis that the request's links make
//!   turn at another speed than the shaft's gears (an idler, a gear driven back at another
//!   ratio) is not joined to it: it turns free on the shaft, bored [`FREE_FIT`] over its Ø.

use crate::axes::{Axis, add, cross, dot, len, mul, sub, unit};
use crate::items::{
    ANGLE_TOL, BevelStyle, BlockSettings, Bore, FacePreset, FaceSides, GearGeom, GearItem, GearRef, GearSettings, ItemKind, ItemLink, LinkKind,
    ModuleBlock, ShaftMode, Thing, face_extent, gear_geoms, resolve_module, tol,
};
use crate::plan::Spec;
use crate::sketch_rules::auto_module;

type V = [f64; 3];

fn positive(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

/// The block item `item` is in (an index into the blocks): the first that lists it.
pub fn block_of(blocks: &[ModuleBlock], item: usize) -> Option<usize> {
    blocks.iter().position(|b| b.members.contains(&item))
}

/// Whether a helix angle means something for an item of `kind` (gears on parallel shafts).
fn takes_helix(kind: ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone | ItemKind::Compound | ItemKind::Internal | ItemKind::Rack | ItemKind::Planetary
    )
}

/// Gear settings `s` (of an item of `kind`) with block `b`'s filled in where `s` gives none: its
/// helix angle and teeth (Helical or Herringbone: [`BlockSettings::herringbone`]), its face width
/// (half each side; an A only or B only face all on that side), and (Bore only) its Ø as the
/// bore. A gear on a made shaft gets no bore: it is one body with it.
///
/// A bevel pair takes the block's helix as its spiral ([`block_spiral`]); a hypoid keeps its own
/// (its spiral goes with its offset), a worm its lead.
pub fn block_settings(b: &BlockSettings, kind: ItemKind, s: &GearSettings) -> GearSettings {
    let mut out = *s;
    if let Some(h) = b.helix.filter(|h| h.is_finite())
        && s.helix.is_none()
        && takes_helix(kind)
    {
        out.helix = Some(h);
        out.v = s.v || b.herringbone;
    }
    if kind == ItemKind::Bevel {
        (out.bevel_style, out.spiral_angle) = block_spiral(b, s.bevel_style, s.spiral_angle);
    }
    if let Some(w) = positive(b.face_width)
        && s.face.a.is_none()
        && s.face.b.is_none()
    {
        out.face = match s.face.preset {
            FacePreset::AOnly => FaceSides { preset: FacePreset::AOnly, a: Some(w), b: None },
            FacePreset::BOnly => FaceSides { preset: FacePreset::BOnly, a: None, b: Some(w) },
            _ => FaceSides { preset: FacePreset::Symmetric, a: Some(w / 2.0), b: None },
        };
    }
    if let Some(d) = positive(b.shaft_diameter)
        && b.shaft == ShaftMode::BoreOnly
        && s.bore == Bore::Auto
    {
        out.bore = Bore::Diameter(d);
    }
    out
}

/// A bevel pair's style and spiral angle in block `b` (its own `style` and `spiral`): a straight
/// pair with no spiral angle of its own (Auto) is spiral at the block's helix angle (chevron when
/// the block's teeth are herringbone); a spiral or chevron pair with none takes the block's
/// angle. A pair's own spiral angle (0: straight) stays, and a block helix of 0 changes nothing.
pub fn block_spiral(b: &BlockSettings, style: BevelStyle, spiral: Option<f64>) -> (BevelStyle, Option<f64>) {
    let Some(h) = b.helix.filter(|h| h.is_finite() && h.abs() > 1e-9) else { return (style, spiral) };
    if spiral.is_some() {
        return (style, spiral);
    }
    match style {
        BevelStyle::Straight if b.herringbone => (BevelStyle::Chevron, Some(h)),
        BevelStyle::Straight => (BevelStyle::Spiral, Some(h)),
        own => (own, Some(h)),
    }
}

/// The items with each member's gears given its block's settings ([`block_settings`]) and its
/// shaft Ø (a Shaft item's, a drawn shaft line's) where it has none.
pub fn with_blocks(items: &[GearItem], blocks: &[ModuleBlock]) -> Vec<GearItem> {
    items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let Some(k) = block_of(blocks, i) else { return it.clone() };
            let b = &blocks[k].settings;
            let mut out = it.clone();
            let n = it.gear_count().max(it.gears.len()).max(1);
            out.gears = (0..n).map(|g| block_settings(b, it.kind, &it.settings(g))).collect();
            // A bevel pair drawn as a side view is sized from its gears' teeth and module (no
            // circle gives its size): the block's module is theirs, or it is sized at the
            // stand-in module and its teeth can't be met at the block's.
            if crate::items::sized_by_settings(it.kind, &it.things)
                && let Some(m) = blocks[k].module.filter(|m| *m > 0.0 && m.is_finite())
            {
                for g in &mut out.gears {
                    g.module = Some(m);
                }
            }
            if out.shaft_diameter.is_none() && b.shaft == ShaftMode::Make {
                out.shaft_diameter = positive(b.shaft_diameter);
            }
            out
        })
        .collect()
}

/// `base` with the pressure angle and backlash of the block holding `gears` (the first of them
/// in a block), where it sets them.
pub fn block_spec(blocks: &[ModuleBlock], gears: &[GearRef], base: &Spec) -> Spec {
    let mut s = base.clone();
    if let Some(b) = gears.iter().find_map(|r| block_of(blocks, r.item)).map(|k| &blocks[k].settings) {
        if let Some(pa) = positive(b.pressure_angle) {
            s.pressure_angle = pa;
        }
        if let Some(bl) = b.backlash.filter(|v| v.is_finite() && *v >= 0.0) {
            s.backlash = bl;
        }
    }
    s
}

/// Which items each tooth chamfer goes on: each block's own (its members), and the set's
/// (`set`, mm) on every item in no block with a chamfer of its own. Sizes of 0 are left out.
pub fn chamfer_groups(count: usize, blocks: &[ModuleBlock], set: Option<f64>) -> Vec<(f64, Vec<usize>)> {
    let own = |i: usize| block_of(blocks, i).and_then(|k| positive(blocks[k].settings.tooth_chamfer));
    let mut out = Vec::new();
    if let Some(c) = positive(set) {
        let rest: Vec<usize> = (0..count).filter(|i| own(*i).is_none()).collect();
        if !rest.is_empty() {
            out.push((c, rest));
        }
    }
    for (k, b) in blocks.iter().enumerate() {
        let Some(c) = positive(b.settings.tooth_chamfer) else { continue };
        let members: Vec<usize> = (0..count).filter(|i| block_of(blocks, *i) == Some(k)).collect();
        if !members.is_empty() {
            out.push((c, members));
        }
    }
    out
}

/// The items with only those `keep` names left as they are, the others emptied (no geometry, so
/// no gears) with their places kept: a pass over every gear of a set (the tooth chamfer) then
/// reaches only those.
pub fn only_items(items: &[GearItem], keep: &[usize]) -> Vec<GearItem> {
    items.iter().enumerate().map(|(i, it)| if keep.contains(&i) { it.clone() } else { GearItem::new(&it.id, ItemKind::Spur) }).collect()
}

/// A shaft a block with a shaft Ø makes along one axis its gears are on.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockShaft {
    /// The block (an index into the blocks).
    pub block: usize,
    /// The block's gears on this axis.
    pub gears: Vec<GearRef>,
    /// Its ends (world): over the gears' faces and a margin past them (the shaft Ø, at least 3
    /// mm), or out to the bearing points picked on the axis.
    pub a: V,
    pub b: V,
    pub diameter: f64,
    /// The block's gears on this axis that the links make turn at another speed than `gears`:
    /// not joined to the shaft, each turning free on it in a bore [`FREE_FIT`] over its Ø.
    pub free: Vec<GearRef>,
    /// The item whose own shaft line is on this axis already (a Shaft item, or a gear's drawn
    /// shaft line): that shaft takes the block's Ø and no other is made.
    pub existing: Option<usize>,
}

impl BlockShaft {
    pub fn length(&self) -> f64 {
        len(sub(self.b, self.a))
    }
}

/// Whether a gear can sit on a block's shaft: not a ring, a rack's bar, a planetary set's (its
/// carrier holds its planets) or a Shaft item.
fn on_a_shaft(it: &GearItem, geo: &GearGeom) -> bool {
    !geo.ring && !geo.rack && !matches!(it.kind, ItemKind::Planetary | ItemKind::Shaft)
}

/// Whether the axis through `p` along `w` is the one through `o` along unit `u`.
fn coaxial(o: V, u: V, p: V, w: V, t: f64) -> bool {
    len(cross(u, w)) <= ANGLE_TOL && Axis { origin: o, dir: u }.distance(p) <= t
}

/// The item (other than a block's own) whose shaft line lies on the axis through `o` along `u`:
/// a Shaft item's line, or a parallel-axis gear's drawn shaft line.
fn line_on(items: &[GearItem], o: V, u: V, t: f64) -> Option<usize> {
    items.iter().position(|it| {
        matches!(it.kind, ItemKind::Shaft | ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone | ItemKind::Compound)
            && it.things.iter().any(|x| match x {
                Thing::Line { a, b, .. } => unit(sub(*b, *a)).is_some_and(|w| coaxial(o, u, *a, w, t)),
                _ => false,
            })
    })
}

/// For a bevel pair's gear `g`: which way along its axis (`geos[g].axis`) its pitch cone's apex
/// lies from its centre (+1 or -1), and how far from the centre toward the apex a shaft of
/// radius `r` on it may reach and still clear a shaft of the same radius on the mate's axis
/// (both running from the apex out: at a shaft angle θ of 90° or more the two ends must stand
/// r·cot(θ/2) off the apex, under 90° 2r / sin θ), less a running gap; 0 where even the centre
/// is too near. `None` for other gears, and for axes that don't meet (a hypoid's).
fn apex_side(it: &GearItem, geos: &[GearGeom], g: usize, r: f64) -> Option<(f64, f64)> {
    const GAP: f64 = 0.5;
    if it.kind != ItemKind::Bevel || geos.len() != 2 {
        return None;
    }
    let (me, mate) = (&geos[g], &geos[1 - g]);
    let (u, v) = (unit(me.axis)?, unit(mate.axis)?);
    let n = cross(u, v);
    let nn = dot(n, n);
    if nn <= ANGLE_TOL * ANGLE_TOL {
        return None;
    }
    // The point on my axis nearest the mate's (the apex), and how far apart the axes pass there.
    let w = sub(mate.centre, me.centre);
    let t = dot(cross(w, v), n) / nn;
    let gap = dot(w, n).abs() / nn.sqrt();
    if gap > tol(me.radius.unwrap_or(1.0)) || t.abs() <= 1e-9 {
        return None;
    }
    let apex = add(me.centre, mul(u, t));
    let theta = dot(unit(sub(me.centre, apex))?, unit(sub(mate.centre, apex))?).clamp(-1.0, 1.0).acos();
    let clear = if theta >= std::f64::consts::FRAC_PI_2 { r / (0.5 * theta).tan() } else { 2.0 * r / theta.sin().max(1e-6) } + GAP;
    Some((t.signum(), (t.abs() - clear).max(0.0)))
}

/// The shafts the blocks with a shaft Ø make: one per distinct axis their gears are on (gears
/// on one axis share it), over the gears' faces (at the block's module, or the Auto one its
/// gears' sizes give) and a margin past them, or out to the bearing points picked on that axis.
/// No links: every gear on an axis is on its shaft ([`block_shafts_linked`]).
pub fn block_shafts(items: &[GearItem], blocks: &[ModuleBlock]) -> Vec<BlockShaft> {
    block_shafts_linked(items, &[], blocks)
}

/// How much bigger than its shaft a free-turning gear's bore is (mm, on the diameter): a
/// running clearance.
pub const FREE_FIT: f64 = 0.2;

/// [`block_shafts`] with the request's links: of the gears on one axis, those the links make
/// turn at another speed (or the other way) than the most of them are `free` on the shaft, not
/// on it. Gears the links don't relate to those stay on it.
pub fn block_shafts_linked(items: &[GearItem], links: &[ItemLink], blocks: &[ModuleBlock]) -> Vec<BlockShaft> {
    let items = with_blocks(items, blocks);
    let turning = turnings(&items, links);
    let mut out = Vec::new();
    for (k, b) in blocks.iter().enumerate() {
        let Some(d) = positive(b.settings.shaft_diameter).filter(|_| b.settings.shaft == ShaftMode::Make) else { continue };
        let mut members: Vec<usize> = b.members.iter().copied().filter(|i| block_of(blocks, *i) == Some(k) && *i < items.len()).collect();
        members.sort_unstable();
        members.dedup();
        // The module the faces are reached at where a gear gives none: the block's, else the
        // Auto one for its gears (as the planning works it out).
        let auto = || {
            let ds: Vec<(f64, f64)> = members.iter().flat_map(|i| gear_geoms(&items[*i])).filter_map(|g| g.radius.map(|r| (2.0 * r, 1.0))).collect();
            (!ds.is_empty()).then(|| auto_module(&ds, 8).0)
        };
        let block_module = positive(b.module).or_else(auto);
        // Each axis: a point on it, its direction, its gears, how far they reach along it, and
        // whether each end stops at a bevel gear's apex side (no margin past it there).
        let mut axes: Vec<(V, V, Vec<GearRef>, (f64, bool), (f64, bool))> = Vec::new();
        for &i in &members {
            let it = &items[i];
            let geos = gear_geoms(it);
            for (g, geo) in geos.iter().enumerate() {
                let Some(w) = unit(geo.axis).filter(|_| on_a_shaft(it, geo)) else { continue };
                let s = it.settings(g);
                let m = resolve_module(blocks, i, &s).module.or(block_module).unwrap_or(1.0);
                let (fa, fb) = face_extent(&s, m);
                let t = tol(geo.radius.unwrap_or(1.0));
                let at = match axes.iter().position(|(o, u, ..)| coaxial(*o, *u, geo.centre, w, t)) {
                    Some(x) => x,
                    None => {
                        axes.push((geo.centre, w, Vec::new(), (f64::INFINITY, false), (f64::NEG_INFINITY, false)));
                        axes.len() - 1
                    }
                };
                let (o, u, gears, lo, hi) = &mut axes[at];
                let c = dot(sub(geo.centre, *o), *u);
                let (mut ahead, mut behind) = if dot(w, *u) >= 0.0 { (fa, fb) } else { (fb, fa) };
                // A bevel gear's shaft stops on its apex side short of where its mate's shaft
                // crosses (run on, the two would cut each other back off both gears), with no
                // margin past it there. It runs on out of the gear's back.
                let near = apex_side(it, &geos, g, d / 2.0);
                let apex = near.map(|(s, _)| s * dot(w, *u));
                if let Some((_, room)) = near {
                    if apex.is_some_and(|s| s > 0.0) {
                        ahead = ahead.min(room);
                    } else {
                        behind = behind.min(room);
                    }
                }
                if c - behind < lo.0 {
                    *lo = (c - behind, apex.is_some_and(|s| s < 0.0));
                }
                if c + ahead > hi.0 {
                    *hi = (c + ahead, apex.is_some_and(|s| s > 0.0));
                }
                gears.push(GearRef::new(i, g));
            }
        }
        for (o, u, gears, lo, hi) in axes {
            let line = Axis { origin: o, dir: u };
            let bearings: Vec<f64> = items
                .iter()
                .flat_map(|it| it.things.iter())
                .filter_map(|t| match t {
                    Thing::Point { p, .. } if line.distance(*p) <= tol(d) => Some(dot(sub(*p, o), u)),
                    _ => None,
                })
                .collect();
            let ((lo, lo_apex), (hi, hi_apex)) = (lo, hi);
            let (lo, hi) = if bearings.is_empty() {
                let margin = d.max(3.0);
                (if lo_apex { lo } else { lo - margin }, if hi_apex { hi } else { hi + margin })
            } else {
                (bearings.iter().copied().fold(lo, f64::min), bearings.iter().copied().fold(hi, f64::max))
            };
            let (gears, free) = on_and_free(&gears, u, &items, &turning);
            out.push(BlockShaft {
                block: k,
                gears,
                free,
                a: add(o, mul(u, lo)),
                b: add(o, mul(u, hi)),
                diameter: d,
                existing: line_on(&items, o, u, tol(d)),
            });
        }
    }
    out
}

/// The id of the set's `n`-th block shaft (from 1, across its blocks): "S2"; its body is
/// "Shaft S2".
pub fn shaft_id(n: usize) -> String {
    format!("S{n}")
}

/// Each of `shafts`' ids ([`shaft_id`], numbered in order), `None` for one that is a shaft line
/// already there ([`BlockShaft::existing`]).
pub fn shaft_ids(shafts: &[BlockShaft]) -> Vec<Option<String>> {
    let mut n = 0;
    shafts
        .iter()
        .map(|s| {
            s.existing.is_none().then(|| {
                n += 1;
                shaft_id(n)
            })
        })
        .collect()
}

/// The items and links with the blocks applied, for planning: every member given its block's
/// settings ([`with_blocks`]), and each [`block_shafts`] shaft made: a Shaft item along it (after
/// the items, so their indices stay) joined to the gears on its axis (one body), or the shaft
/// line already there given the block's Ø.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Expanded {
    pub items: Vec<GearItem>,
    pub links: Vec<ItemLink>,
    pub shafts: Vec<BlockShaft>,
    /// The Shaft items added (indices into `items`, the same order as the shafts without an
    /// `existing` line).
    pub added: Vec<usize>,
}

/// See [`Expanded`]. A request with links of its own gets a Shaft link from each new shaft to
/// each gear on its axis; one with none infers them as for any Shaft item (the gears on its line).
pub fn expand_blocks(items: &[GearItem], links: &[ItemLink], blocks: &[ModuleBlock]) -> Expanded {
    let shafts = block_shafts_linked(items, links, blocks);
    let mut out = Expanded { items: with_blocks(items, blocks), links: links.to_vec(), shafts: Vec::new(), added: Vec::new() };
    let given = !links.is_empty();
    for (s, id) in shafts.iter().zip(shaft_ids(&shafts)) {
        // A free-turning gear's bore (where the gear left it on Auto) clears the shaft.
        for r in &s.free {
            if items.get(r.item).is_some_and(|it| it.settings(r.gear).bore == Bore::Auto) {
                let it = &mut out.items[r.item];
                while it.gears.len() <= r.gear {
                    let next = it.settings(it.gears.len());
                    it.gears.push(next);
                }
                it.gears[r.gear].bore = Bore::Diameter(s.diameter + FREE_FIT);
            }
        }
        match s.existing {
            Some(e) => {
                let it = &mut out.items[e];
                if it.shaft_diameter.is_none() {
                    it.shaft_diameter = Some(s.diameter);
                }
                // A Shaft item's line runs over the block's gears on its axis (a line drawn
                // short of a gear, a bevel's back face, would leave a gap), and carries them.
                if it.kind == ItemKind::Shaft {
                    reach_over(it, s.a, s.b);
                }
                if given {
                    let from = GearRef::new(e, 0);
                    for r in s.gears.iter().filter(|r| r.item != e) {
                        if !out.links.iter().any(|l| l.kind == LinkKind::Shaft && ((l.a == from && l.b == *r) || (l.b == from && l.a == *r))) {
                            out.links.push(ItemLink { a: from, b: *r, kind: LinkKind::Shaft });
                        }
                    }
                }
            }
            None => {
                let mut it = GearItem::new(&id.unwrap_or_default(), ItemKind::Shaft).with(vec![Thing::line(s.a, s.b)]);
                it.shaft_diameter = Some(s.diameter);
                let at = out.items.len();
                out.items.push(it);
                out.added.push(at);
                if given {
                    out.links.extend(s.gears.iter().map(|r| ItemLink { a: GearRef::new(at, 0), b: *r, kind: LinkKind::Shaft }));
                }
            }
        }
    }
    out.shafts = shafts;
    out
}

/// Shaft item `it`'s line (its first) lengthened, along itself, to reach over `a`..`b` (points on
/// its axis) too.
fn reach_over(it: &mut GearItem, a: V, b: V) {
    let Some(Thing::Line { a: la, b: lb, .. }) = it.things.iter_mut().find(|t| matches!(t, Thing::Line { .. })) else { return };
    let Some(u) = unit(sub(*lb, *la)) else { return };
    let o = *la;
    let ts = [0.0, dot(sub(*lb, o), u), dot(sub(a, o), u), dot(sub(b, o), u)];
    let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    *la = add(o, mul(u, lo));
    *lb = add(o, mul(u, hi));
}

/// Each gear's turning as far as the links tell it: the group of gears the links (and each
/// item's own gears: a compound's on one shaft, a pair's meshing) tie together, and its speed
/// about its own axis (`gear_geoms`' direction, signed) per unit of the group's first gear's. A
/// gear tied to none is a group of its own; a tie whose speeds can't be worked out (a rack, a
/// gear of no radius) ties nothing. Gears tied rigidly (a compound's layers, a Shaft link) always
/// get their speeds from each other, so a loop whose meshes don't quite agree (a power split
/// drawn with circles a little off) can't part them.
fn turnings(items: &[GearItem], links: &[ItemLink]) -> Vec<(GearRef, usize, f64)> {
    let geoms: Vec<Vec<GearGeom>> = items.iter().map(gear_geoms).collect();
    let refs: Vec<GearRef> = geoms.iter().enumerate().flat_map(|(i, g)| (0..g.len()).map(move |k| GearRef::new(i, k))).collect();
    let at = |r: GearRef| refs.iter().position(|x| *x == r);
    let geo = |r: GearRef| geoms.get(r.item).and_then(|g| g.get(r.gear));
    // Every tie: the two gears, the second's speed per unit of the first's, and whether it is
    // rigid (one shaft).
    let mut ties: Vec<(usize, usize, f64, bool)> = Vec::new();
    let mut tie = |a: GearRef, b: GearRef, kind: LinkKind| {
        let (Some(x), Some(y), Some(ga), Some(gb)) = (at(a), at(b), geo(a), geo(b)) else { return };
        let ratio = match kind {
            LinkKind::Shaft => unit(ga.axis).zip(unit(gb.axis)).map(|(u, w)| dot(u, w)),
            LinkKind::Mesh => mesh_ratio(ga, gb),
        };
        if let Some(k) = ratio.filter(|k| k.is_finite() && k.abs() > 1e-12) {
            ties.push((x, y, k, kind == LinkKind::Shaft));
        }
    };
    for (i, it) in items.iter().enumerate() {
        let n = geoms[i].len();
        if n < 2 || matches!(it.kind, ItemKind::Planetary | ItemKind::Shaft) {
            continue;
        }
        let kind = if it.kind == ItemKind::Compound { LinkKind::Shaft } else { LinkKind::Mesh };
        for g in 1..n {
            tie(GearRef::new(i, 0), GearRef::new(i, g), kind);
        }
    }
    for l in links {
        tie(l.a, l.b, l.kind);
    }
    // Each group from its first gear, the speeds carried along the ties (a loop's last tie, which
    // a set that can turn agrees with, changes nothing).
    let mut speed: Vec<Option<(usize, f64)>> = vec![None; refs.len()];
    for start in 0..refs.len() {
        if speed[start].is_some() {
            continue;
        }
        let mut todo = Vec::new();
        set_rigidly(&mut speed, &ties, start, (start, 1.0), &mut todo);
        while let Some(x) = todo.pop() {
            let Some((group, sx)) = speed[x] else { continue };
            for &(a, b, k, _) in &ties {
                let (next, s) = if a == x {
                    (b, sx * k)
                } else if b == x {
                    (a, sx / k)
                } else {
                    continue;
                };
                if speed[next].is_none() {
                    set_rigidly(&mut speed, &ties, next, (group, s), &mut todo);
                }
            }
        }
    }
    refs.iter().zip(speed).filter_map(|(r, s)| s.map(|(g, v)| (*r, g, v))).collect()
}

/// Gear `x`'s speed (its group's) set, and at once every gear rigidly tied to it (on one shaft
/// with it, through rigid ties only) from it; each is queued in `todo`.
fn set_rigidly(speed: &mut [Option<(usize, f64)>], ties: &[(usize, usize, f64, bool)], x: usize, at: (usize, f64), todo: &mut Vec<usize>) {
    speed[x] = Some(at);
    todo.push(x);
    let mut stack = vec![x];
    while let Some(y) = stack.pop() {
        let Some((group, sy)) = speed[y] else { continue };
        for &(a, b, k, _) in ties.iter().filter(|t| t.3) {
            let (next, s) = if a == y {
                (b, sy * k)
            } else if b == y {
                (a, sy / k)
            } else {
                continue;
            };
            if speed[next].is_none() {
                speed[next] = Some((group, s));
                todo.push(next);
                stack.push(next);
            }
        }
    }
}

/// Gear `b`'s speed about its axis per unit of `a`'s about its own, the two meshing: their pitch
/// circles' speeds agree where they meet (the point of `a`'s nearest `b`'s). `None` without
/// both radii.
fn mesh_ratio(a: &GearGeom, b: &GearGeom) -> Option<f64> {
    let (ra, rb) = (a.radius?, b.radius?);
    let (ua, ub) = (unit(a.axis)?, unit(b.axis)?);
    // Two directions square to a's axis, in its pitch circle's plane.
    let pick = if ua[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let e1 = unit(cross(ua, pick))?;
    let e2 = cross(ua, e1);
    let on_a = |t: f64| add(a.centre, add(mul(e1, ra * t.cos()), mul(e2, ra * t.sin())));
    let off_b = |t: f64| {
        let d = sub(on_a(t), b.centre);
        let along = dot(d, ub);
        let radial = len(sub(d, mul(ub, along)));
        along * along + (radial - rb) * (radial - rb)
    };
    // The nearest point: the best of 720, narrowed down about it.
    let n = 720u32;
    let step = std::f64::consts::TAU / f64::from(n);
    let mut t = (0..n).map(|k| f64::from(k) * step).min_by(|x, y| off_b(*x).total_cmp(&off_b(*y)))?;
    let mut h = step;
    for _ in 0..40 {
        h /= 2.0;
        t = [t - h, t, t + h].into_iter().min_by(|x, y| off_b(*x).total_cmp(&off_b(*y)))?;
    }
    let p = on_a(t);
    let (va, vb) = (cross(ua, sub(p, a.centre)), cross(ub, sub(p, b.centre)));
    let vv = dot(vb, vb);
    (vv > 1e-18).then(|| dot(va, vb) / vv)
}

/// An axis's gears (`u` its direction) parted into those on its shaft and those free on it: the
/// gears the links make turn alike about `u` are one lot; the biggest lot (the first of equal
/// ones) is on the shaft, with every gear the links don't relate to it; the others of its group
/// turn free.
fn on_and_free(gears: &[GearRef], u: V, items: &[GearItem], turning: &[(GearRef, usize, f64)]) -> (Vec<GearRef>, Vec<GearRef>) {
    // A gear's group and its speed about `u`.
    let about = |r: GearRef| -> Option<(usize, f64)> {
        let (_, group, s) = turning.iter().find(|(x, ..)| *x == r)?;
        let w = unit(gear_geoms(items.get(r.item)?).get(r.gear)?.axis)?;
        Some((*group, s * dot(w, u)))
    };
    // (Within a part in a thousand: drawn circles in a loop can be a little off.)
    let alike = |a: (usize, f64), b: (usize, f64)| a.0 == b.0 && (a.1 - b.1).abs() <= 1e-3 * a.1.abs().max(b.1.abs()).max(1e-12);
    let mut lots: Vec<((usize, f64), usize)> = Vec::new();
    for r in gears {
        let Some(t) = about(*r) else { continue };
        match lots.iter_mut().find(|(k, _)| alike(*k, t)) {
            Some((_, count)) => *count += 1,
            None => lots.push((t, 1)),
        }
    }
    let mut kept: Option<((usize, f64), usize)> = None;
    for lot in &lots {
        if kept.is_none_or(|k| lot.1 > k.1) {
            kept = Some(*lot);
        }
    }
    let Some((kept, _)) = kept else { return (gears.to_vec(), Vec::new()) };
    let free: Vec<GearRef> = gears.iter().copied().filter(|r| about(*r).is_some_and(|t| t.0 == kept.0 && !alike(t, kept))).collect();
    (gears.iter().copied().filter(|r| !free.contains(r)).collect(), free)
}
