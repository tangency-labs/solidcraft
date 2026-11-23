//! A whole drivetrain from the Gears feature's items (gear-drivetrain-plan.md WP1): every stage
//! planned by its own planner, placed where the drawing puts it, and merged into one [`Plan`]
//! with one assembly.
//!
//! [`plan_items`] is what the engine calls. It splits the items into [`crate::items::stages`] and plans
//! them in units:
//!
//! - **trains**: the gears on parallel shafts tied by Parallel / Internal meshes and Shaft links
//!   (compound gears) are one train each, sized by [`crate::sketch_rules::size_train`] through
//!   the drivetrain reader ([`dt::read_with`]) and planned by [`super::train`]. Tooth styles may
//!   differ between meshing groups (spur, helical, herringbone per gear; hands opposite across a
//!   mesh);
//! - **planetary sets**: sized as a train (sun, ring, planet), planned by the planetary planner
//!   with a generated carrier body that the planets ride on (decision 9);
//! - **pairs**: bevel (straight, spiral), hypoid, worm and crossed helical stages, each the set
//!   its shafts imply ([`crate::select::set_on_shafts`] through the reader); the module from
//!   [`crate::sketch_rules::size_stage`] where none is asked;
//! - **racks**: a rack and its pinion (the pitch line held by the pinion's profile shift,
//!   [`crate::sketch_rules::size_stage`]).
//!
//! A gear in several stages (decision 8: a bevel gear driving two pinions, a worm with two
//! wheels, a ring with several pinions) is planned once: a later stage's copy of it is dropped
//! and its new mate turned to mesh with the one kept ([`crate::mesh3d`]). Gears on one shaft
//! across units (Shaft links) are one rigid member ([`Plan::clusters`], [`super::solid`]). Bodies
//! of different stages that would sweep into each other are refused, naming both.

use std::f64::consts::{PI, TAU};

use crate::analysis::{Efficiencies, MeshKind, StepKind, classify, drive_table, plan_mesh_checks};
use crate::axes::{Axis, add, cross, dot, len, mul, num, square_to, sub, unit};
use crate::blocks::block_spec;
use crate::drivetrain::{self as dt, Outcome};
use crate::geom::{Outline, Region, V2};
use crate::items::{
    BevelStyle, Drive, FacePreset, GearGeom, GearItem, GearRef, GearReply, GearSettings, ItemKind, ItemLink, LinkKind, LinkReply, ModuleBlock,
    Source, Stage, StageFrom, StageKind, Suggestion, effective_links, gear_geoms, resolve_module, stages, suggest_links, to_drivetrain, unlinked,
    validate_item, who,
};
use crate::plan::{Assembly, AssemblyLink, AssemblyMember, Body, Frame, Kind, Op, Plan, PlanShaft, SetMember, Spec, Step, plan};
use crate::simple::Simple;
use crate::sketch_rules::{StageGear, auto_module, size_stage};

type V = [f64; 3];

/// A whole drivetrain planned from items ([`plan_items`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemPlan {
    /// Every stage's bodies in world coordinates, one [`SetMember`] per gear (a planetary set's
    /// every planet), its clusters, shafts and one assembly driven from the drive gear. Empty
    /// when [`SystemPlan::error`] is set.
    pub plan: Plan,
    /// One per gear of the items, in item and gear order.
    pub gears: Vec<GearReply>,
    /// One per link used (the request's, or the inferred ones when it has none).
    pub links: Vec<LinkReply>,
    /// Links the drawing suggests that the request doesn't have (drawn dashed).
    pub suggestions: Vec<Suggestion>,
    /// Each gear's member: an index into `plan.members` (its body: `plan.members[k].body`). A
    /// planetary set's planet gear is planet 1; its other planets are members of no gear.
    pub member_of: Vec<(GearRef, usize)>,
    pub notes: Vec<String>,
    /// Generated carriers: (planetary item, body).
    pub carriers: Vec<(usize, usize)>,
    /// The assembly member the drive turns (`plan.assembly` is rooted on it).
    pub driver: Option<usize>,
    /// Why nothing is built: a link or stage that is wrong, stages that collide (what is wrong
    /// and what to do). The replies still say per gear and per link what was found.
    pub error: Option<String>,
    /// Each mesh's real stage kind (worm, bevel, spiral bevel, hypoid, crossed helical, rack,
    /// internal, parallel), by the two bodies it joins: what the analysis takes it as
    /// ([`SystemPlan::efficiencies`]).
    pub mesh_tags: Vec<MeshTag>,
}

/// A mesh of a drivetrain of items tagged with the kind of stage it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshTag {
    /// The bodies it joins (indices into `plan.bodies`), either order.
    pub bodies: (usize, usize),
    pub kind: MeshKind,
    /// A worm's lead angle (degrees), as its stage was planned.
    pub lead_angle: Option<f64>,
}

impl SystemPlan {
    /// The analysis' efficiencies for `asm` (this plan's assembly, or one over the same bodies
    /// rooted elsewhere): each mesh link its stage's own kind, a worm its own lead angle (so a
    /// worm gets its lead-angle efficiency and self-locking check whichever end drives it).
    pub fn efficiencies(&self, asm: &Assembly) -> Efficiencies {
        let mut e = Efficiencies::default();
        let body = |m: usize| asm.members.get(m).map(|x| x.body);
        for (k, l) in asm.links.iter().enumerate() {
            if matches!(l, AssemblyLink::Rigid { .. }) {
                continue;
            }
            let (a, b) = l.members();
            let (Some(ba), Some(bb)) = (body(a), body(b)) else { continue };
            if let Some(t) = self.mesh_tags.iter().find(|t| t.bodies == (ba, bb) || t.bodies == (bb, ba)) {
                e.kinds.push((k, t.kind));
                if let Some(lead) = t.lead_angle {
                    e.worm_leads.push((k, lead));
                }
            }
        }
        e
    }
    /// Gear `r`'s member (an index into `plan.members`).
    pub fn member(&self, r: GearRef) -> Option<usize> {
        self.member_of.iter().find(|(g, _)| *g == r).map(|(_, m)| *m)
    }
    /// Gear `r`'s body (an index into `plan.bodies` and `plan.assembly.members`).
    pub fn body(&self, r: GearRef) -> Option<usize> {
        self.plan.members.get(self.member(r)?)?.body
    }
}

/// `text` with the drivetrain reader's "member N" named as `names[N - 1]`.
fn rename_members(text: &str, names: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(k) = rest.find("member ") {
        let (before, after) = rest.split_at(k);
        out.push_str(before);
        let digits: String = after["member ".len()..].chars().take_while(char::is_ascii_digit).collect();
        match digits.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| names.get(i)) {
            Some(name) if !digits.is_empty() => {
                out.push_str(name);
                rest = &after["member ".len() + digits.len()..];
            }
            _ => {
                out.push_str("member ");
                rest = &after["member ".len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `text` with a stage plan's "gear N" ("gears N and M") named as `names[N - 1]`, so a stage's
/// warnings name the items' gears.
fn gear_names(text: &str, names: &[String]) -> String {
    crate::sketch_rules::rename_gears(text, &|k| names.get(k).cloned().unwrap_or_else(|| format!("gear {}", k + 1)))
}

/// `text` (a set's plan, sizing or reader words) with the plan's "gear N" named `by_gear[N - 1]`
/// ([`gear_names`]) and the reader's "member N" `names[N - 1]` ([`rename_members`]): every
/// place a set's words leave its planner names its gears as the dialog does ("Gear F1").
pub(crate) fn named(text: &str, by_gear: &[String], names: &[String]) -> String {
    rename_members(&gear_names(text, by_gear), names)
}

/// A set's refusal [`named`]: said of the gear it names ("Gear F1: the teeth are too tall…"),
/// or, when it starts with none of them, of the set ("Gear F1 and Gear F2: …").
fn set_refusal(e: &str, by_gear: &[String], names: &[String]) -> String {
    let t = named(e, by_gear, names);
    if names.iter().chain(by_gear).any(|n| !n.is_empty() && t.starts_with(n.as_str())) || names.is_empty() {
        t
    } else {
        format!("{}: {t}", names.join(" and "))
    }
}

/// The plan's name for each of a reader's gears (its gear N: `order[N - 1]`, an index into
/// `names`).
fn by_order(order: &[usize], names: &[String]) -> Vec<String> {
    order.iter().enumerate().map(|(k, i)| names.get(*i).cloned().unwrap_or_else(|| format!("gear {}", k + 1))).collect()
}

/// A block's name as the dialog shows it on its own: "M1" is "Block 1".
fn block_name(b: &ModuleBlock) -> String {
    match b.id.strip_prefix('M').filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
        Some(n) => format!("Block {n}"),
        None => format!("Block {}", b.id),
    }
}

/// `text` with "Gear B1 sits on a shaft (Ø15)" (a planner's words for a gear one body with its
/// shaft, [`crate::hub::root_words`]) saying which shaft: "sits on Block 1's shaft S1 (Ø15)"
/// (a block's Make shaft), "sits on shaft S1 (Ø15)" (a Shaft item's bar).
fn shaft_named(text: &str, items: &[GearItem], blocks: &[ModuleBlock]) -> String {
    if !text.contains(" sits on a shaft (Ø") {
        return text.to_string();
    }
    let mut out = text.to_string();
    for (i, it) in items.iter().enumerate() {
        for k in 0..gear_geoms(it).len() {
            let r = GearRef::new(i, k);
            let pat = format!("{} sits on a shaft (Ø", who(items, r));
            if !out.contains(&pat) {
                continue;
            }
            let shaft = crate::items::link_geom(items, r).and_then(|g| {
                items
                    .iter()
                    .position(|s| s.kind == ItemKind::Shaft && crate::items::shaft_geom(s).is_some_and(|line| crate::items::on_shaft_line(&line, &g)))
            });
            let block = crate::blocks::block_of(blocks, i)
                .map(|b| &blocks[b])
                .filter(|b| b.settings.shaft == crate::items::ShaftMode::Make && b.settings.shaft_diameter.is_some_and(|d| d > 0.0));
            let desc = match (block, shaft) {
                (Some(b), Some(s)) => format!("{}'s shaft {}", block_name(b), items[s].id),
                (None, Some(s)) => format!("shaft {}", items[s].id),
                (Some(b), None) => format!("{}'s shaft", block_name(b)),
                (None, None) => continue,
            };
            out = out.replace(&pat, &format!("{} sits on {desc} (Ø", who(items, r)));
        }
    }
    out
}

/// The text after a refusal's last ": " (what to do), for a link's hint.
fn hint_of(e: &str) -> Option<String> {
    e.rsplit_once(": ").map(|(_, h)| h.to_string()).filter(|h| !h.is_empty())
}

/// One planned unit before merging: its plan (bodies in world coordinates, one assembly member
/// per body), which gear each of its members is, and each member's profile shift.
struct Unit {
    label: String,
    plan: Plan,
    refs: Vec<Option<GearRef>>,
    shifts: Vec<Option<f64>>,
    carrier: Option<(usize, usize)>,
    /// Its meshes' stage kinds (bodies local to `plan`), from [`tagged`].
    tags: Vec<MeshTag>,
    /// Its members' contact ratios (member local to `plan`; the least of its meshes').
    contact: Vec<(usize, f64)>,
    /// For people (the system's notes).
    notes: Vec<String>,
}

/// `u` with its meshes tagged by stage kind (`kind`, else per mesh what `spec` says: a train's
/// each mesh by its own gears' teeth), a worm's with the lead angle its plan worked out, and
/// its members' contact ratios from the plan's own checks ([`plan_mesh_checks`]).
fn tagged(mut u: Unit, spec: &Spec, kind: Option<MeshKind>) -> Unit {
    let e = Efficiencies::for_spec(spec);
    let p = &u.plan;
    let (mut tags, mut contact): (Vec<MeshTag>, Vec<(usize, f64)>) = (Vec::new(), Vec::new());
    let lead = p.facts.iter().find(|f| f.label == "lead angle").map(|f| f.value);
    for k in 0..p.assembly.links.len() {
        let (StepKind::Mesh(mut m), _, _) = classify(&p.assembly, k, &e) else { continue };
        let (a, b) = p.assembly.links[k].members();
        let (Some(ba), Some(bb)) = (p.assembly.members.get(a).map(|x| x.body), p.assembly.members.get(b).map(|x| x.body)) else { continue };
        if let Some(given) = kind {
            m = given;
        } else if let Some(t) = &spec.train
            && m != MeshKind::Internal
        {
            // The two gears meshing across these bodies.
            let pair = p
                .members
                .iter()
                .enumerate()
                .filter(|(_, x)| x.body == Some(ba))
                .flat_map(|(i, x)| x.meshes_with.iter().map(move |j| (i, *j)))
                .find(|(_, j)| p.members.get(*j).and_then(|x| x.body) == Some(bb));
            if let Some((i, j)) = pair
                && let (Some(gi), Some(gj)) = (t.gears.get(i), t.gears.get(j))
            {
                m = if gi.ring || gj.ring {
                    MeshKind::Internal
                } else if gi.herringbone || gj.herringbone {
                    MeshKind::Herringbone
                } else if gi.helix.abs() > 1e-9 || gj.helix.abs() > 1e-9 {
                    MeshKind::Helical
                } else {
                    MeshKind::Spur
                };
            }
        }
        tags.push(MeshTag { bodies: (ba, bb), kind: m, lead_angle: if m == MeshKind::Worm { lead } else { None } });
    }
    let member_on = |b: usize| p.members.iter().position(|x| x.body == Some(b));
    for c in plan_mesh_checks(spec, p) {
        let Some(cr) = c.contact_ratio else { continue };
        let ends = match c.gears {
            Some((ga, gb)) => [Some(ga), Some(gb)],
            None => [c.members.0, c.members.1].map(|m| p.assembly.members.get(m).and_then(|x| member_on(x.body))),
        };
        for k in ends.into_iter().flatten() {
            match contact.iter_mut().find(|(x, _)| *x == k) {
                Some(have) => have.1 = have.1.min(cr),
                None => contact.push((k, cr)),
            }
        }
    }
    (u.tags, u.contact) = (tags, contact);
    u
}

/// The drivetrain the items, links and module blocks describe, every stage planned and merged
/// into one plan whose assembly is driven from `drive` (its gear; default the first gear). `base`
/// gives what the items don't (pressure angle, backlash, tooth form, bores...).
///
/// `Err` only when the request can't be read (no items, an item incomplete or its picks wrong,
/// a drive gear that doesn't exist). Anything wrong with a link, a stage or the stages together
/// comes back as [`SystemPlan::error`] with an empty plan and the replies.
pub fn plan_items(items: &[GearItem], links: &[ItemLink], blocks: &[ModuleBlock], drive: Option<&Drive>, base: &Spec) -> Result<SystemPlan, String> {
    let _span = solidcraft_profile::span("gear", "plan items");
    if items.is_empty() {
        return Err("Add a gear: press New gear and click a pitch circle.".into());
    }
    for it in items {
        validate_item(it)?;
    }
    let geoms: Vec<Vec<GearGeom>> = items.iter().map(gear_geoms).collect();
    let exists = |r: GearRef| geoms.get(r.item).is_some_and(|g| r.gear < g.len());
    if let Some(d) = drive
        && !exists(GearRef::new(d.item, d.gear))
    {
        return Err(format!("the drive gear ({}) doesn't exist: make another gear the driver", who(items, GearRef::new(d.item, d.gear))));
    }
    let (used, inferred) = effective_links(items, links);
    // The suggestions not in use (with no links given, all of them: the dialog draws them dashed).
    let mut out = SystemPlan { suggestions: unlinked(suggest_links(items), if inferred { links } else { &used[..] }), ..SystemPlan::default() };
    if inferred && !used.is_empty() {
        out.notes.push("no links given: the gears mesh and share shafts as the drawing suggests".into());
    }
    // A Shaft item no link names carries the gears on its line: say so.
    if !inferred {
        for l in &used[links.len()..] {
            out.notes.push(format!("{} is in no link: it carries {}, on its line", who(items, l.a), who(items, l.b)));
        }
    }
    // A module block on Auto: one module for all its members' gears.
    let mut blocks = blocks.to_vec();
    for b in blocks.iter_mut().filter(|b| b.module.is_none()) {
        let ds: Vec<(f64, f64)> = b.members.iter().filter_map(|i| geoms.get(*i)).flatten().filter_map(|g| g.radius.map(|r| (2.0 * r, 1.0))).collect();
        if !ds.is_empty() {
            let (m, _, note) = auto_module(&ds, 8);
            b.module = Some(m);
            out.notes.push(format!("module block {}: module {}", b.id, num(m)));
            out.notes.extend(note);
        }
    }
    let st = stages(items, links);
    let mapped = to_drivetrain(items, links, &blocks);
    let mut err: Vec<Option<String>> = st.iter().map(|s| s.error.clone()).collect();
    check_owners(items, &st, &mut err);
    // Errors of gears in no stage (a lone gear whose train can't be sized).
    let mut loose: Vec<String> = Vec::new();
    let units = if err.iter().all(Option::is_none) {
        solidcraft_profile::scope("gear", "plan units", || plan_units(items, &st, &mapped, &blocks, base, &mut err, &mut loose))
    } else {
        Vec::new()
    };
    if err.iter().any(Option::is_some) || !loose.is_empty() {
        let err = err.into_iter().map(|e| e.map(|e| shaft_named(&e, items, &blocks))).collect();
        let loose = loose.iter().map(|e| shaft_named(e, items, &blocks)).collect();
        return Ok(refused(items, &st, &used, err, loose, out));
    }
    let merged = merge(&st, units, drive, &mut out);
    let mut sweeps = Sweeps::default();
    solidcraft_profile::scope("gear", "trim shafts", || trim_shafts(items, &mut out, &mut sweeps));
    if let Some(e) = solidcraft_profile::scope("gear", "collisions", || collisions(items, &out, &merged.body_units, &mut sweeps)) {
        out.error = Some(e);
    }
    replies(items, &st, &used, drive, &merged.shifts, &merged.contact, &mut out);
    if out.error.is_some() {
        let keep = std::mem::take(&mut out.gears);
        out.plan = Plan::default();
        out.member_of.clear();
        out.carriers.clear();
        out.driver = None;
        out.gears = keep;
    }
    Ok(out)
}

/// The reply when a stage is wrong: every stage error in one line, the links' errors.
fn refused(items: &[GearItem], st: &[Stage], used: &[ItemLink], err: Vec<Option<String>>, loose: Vec<String>, mut out: SystemPlan) -> SystemPlan {
    let mut seen: Vec<String> = Vec::new();
    for e in err.iter().flatten().chain(&loose) {
        if !seen.contains(e) {
            seen.push(e.clone());
        }
    }
    out.error = Some(seen.join("; "));
    let st: Vec<Stage> = st.iter().zip(err).map(|(s, e)| Stage { error: e, ..s.clone() }).collect();
    replies(items, &st, used, None, &[], &[], &mut out);
    out
}

/// What the merge keeps beside the plan for the checks after it: each body's units, each
/// member's profile shift.
struct Merged {
    body_units: Vec<Vec<usize>>,
    shifts: Vec<Option<f64>>,
    /// Each member's contact ratio.
    contact: Vec<Option<f64>>,
}

fn is_pair(k: StageKind) -> bool {
    matches!(k, StageKind::Bevel | StageKind::Hypoid | StageKind::Worm | StageKind::CrossedHelical)
}

/// The kinds of stage a gear is in, checked: one gear has one kind of teeth.
fn check_owners(items: &[GearItem], st: &[Stage], err: &mut [Option<String>]) {
    let kind_of = |r: GearRef| items.get(r.item).map(|it| it.kind);
    for (k, s) in st.iter().enumerate() {
        if err[k].is_some() || s.kind == StageKind::Shaft {
            continue;
        }
        for &r in &s.gears {
            let others: Vec<&Stage> =
                st.iter().enumerate().filter(|(j, o)| *j != k && o.kind != StageKind::Shaft && o.gears.contains(&r)).map(|(_, o)| o).collect();
            let e = match s.kind {
                StageKind::Parallel | StageKind::Internal => match kind_of(r) {
                    Some(ik @ (ItemKind::Bevel | ItemKind::Hypoid | ItemKind::Worm | ItemKind::CrossedHelical)) => Some(format!(
                        "{} is a {} gear: it meshes only at an angle to its mate; put a spur gear on its shaft (a Compound, or a Shaft link) to mesh on parallel shafts",
                        who(items, r),
                        ik.label().to_lowercase()
                    )),
                    Some(ItemKind::Planetary) if !matches!(s.from, StageFrom::Item(_)) => Some(format!(
                        "{} is in a planetary set: its gears mesh only within the set (not made yet otherwise); link another gear to its sun or ring by Shaft instead",
                        who(items, r)
                    )),
                    _ => others.iter().find(|o| is_pair(o.kind)).map(|o| {
                        format!(
                            "{} meshes on parallel shafts and also in a {} stage: one gear has one kind of teeth; put a second gear on its shaft (a Compound, or a Shaft link) for the other mesh",
                            who(items, r),
                            stage_name(o.kind)
                        )
                    }),
                },
                StageKind::Rack => others.iter().find(|o| o.kind != StageKind::Shaft).map(|o| {
                    if items.get(r.item).is_some_and(|it| it.kind == ItemKind::Rack && r.gear == 0) {
                        format!("{} meshes with two pinions: a rack with several pinions is not made yet; give each pinion its own rack", who(items, r))
                    } else {
                        format!(
                            "{} drives a rack and is in a {} stage too: a rack's pinion that meshes other gears is not made yet; put a second gear on the pinion's shaft (a Compound, or a Shaft link)",
                            who(items, r),
                            stage_name(o.kind)
                        )
                    }
                }),
                StageKind::Planetary => others.iter().find(|o| o.kind != StageKind::Planetary || o.from != s.from).map(|_| {
                    format!(
                        "{} is in a planetary set and meshes outside it: not made yet; link another gear to its sun or ring by Shaft instead",
                        who(items, r)
                    )
                }),
                k if is_pair(k) => others.iter().find(|o| o.kind != k && o.kind != StageKind::Parallel && o.kind != StageKind::Internal).map(|o| {
                    format!(
                        "{} is in a {} stage and a {} stage: one gear has one kind of teeth; put a second gear on its shaft (a Compound, or a Shaft link)",
                        who(items, r),
                        stage_name(k),
                        stage_name(o.kind)
                    )
                }),
                _ => None,
            };
            if let Some(e) = e {
                err[k].get_or_insert(e);
            }
        }
    }
}

fn stage_name(k: StageKind) -> &'static str {
    match k {
        StageKind::Parallel => "parallel",
        StageKind::Internal => "ring gear",
        StageKind::Rack => "rack",
        StageKind::Planetary => "planetary",
        StageKind::Bevel => "bevel",
        StageKind::Hypoid => "hypoid",
        StageKind::Worm => "worm",
        StageKind::CrossedHelical => "crossed helical",
        StageKind::Shaft => "shaft",
    }
}

/// The kind of mesh a stage of two gears is (what the analysis takes its link as); `None`: per
/// mesh, by its gears.
fn stage_mesh(s: &Stage) -> Option<MeshKind> {
    match s.kind {
        StageKind::Bevel if s.bevel_style == BevelStyle::Straight => Some(MeshKind::Bevel),
        StageKind::Bevel => Some(MeshKind::SpiralBevel),
        StageKind::Hypoid => Some(MeshKind::Hypoid),
        StageKind::Worm => Some(MeshKind::Worm),
        StageKind::Rack => Some(MeshKind::Rack),
        StageKind::CrossedHelical => Some(MeshKind::CrossedHelical),
        StageKind::Internal | StageKind::Parallel | StageKind::Planetary | StageKind::Shaft => None,
    }
}

/// The units in planning order: trains, planetary sets, racks, then the pairs (a gear met again
/// is reused). A unit that can't be planned sets its stages' errors.
fn plan_units(
    items: &[GearItem],
    st: &[Stage],
    mapped: &crate::items::Mapped,
    blocks: &[ModuleBlock],
    base: &Spec,
    err: &mut [Option<String>],
    loose: &mut Vec<String>,
) -> Vec<Unit> {
    let all: Vec<GearRef> = items.iter().enumerate().flat_map(|(i, it)| (0..gear_geoms(it).len()).map(move |g| GearRef::new(i, g))).collect();
    let owned_elsewhere =
        |r: GearRef| st.iter().any(|s| s.gears.contains(&r) && !matches!(s.kind, StageKind::Parallel | StageKind::Internal | StageKind::Shaft));
    let rack_bar = |r: GearRef| items.get(r.item).is_some_and(|it| it.kind == ItemKind::Rack && r.gear == 0);
    let in_train: Vec<GearRef> = all
        .iter()
        .copied()
        .filter(|r| {
            !rack_bar(*r)
                && (st.iter().any(|s| matches!(s.kind, StageKind::Parallel | StageKind::Internal) && s.gears.contains(r)) || !owned_elsewhere(*r))
        })
        .collect();
    // Trains: the train gears joined by parallel meshes and shared shafts.
    let mut group: Vec<usize> = (0..in_train.len()).collect();
    let find = |g: &[usize], mut x: usize| {
        while g[x] != x {
            x = g[x];
        }
        x
    };
    // Gears tied onto one shaft (Shaft stages: links, compounds), by gear in `in_train`.
    let mut tied: Vec<usize> = (0..in_train.len()).collect();
    for s in st.iter().filter(|s| s.kind == StageKind::Shaft) {
        let idx: Vec<usize> = s.gears.iter().filter_map(|r| in_train.iter().position(|g| g == r)).collect();
        for w in idx.windows(2) {
            let (a, b) = (find(&tied, w[0]), find(&tied, w[1]));
            if a != b {
                tied[a.max(b)] = a.min(b);
            }
        }
    }
    let axis_of = |x: usize| {
        let r = in_train[x];
        let g = items.get(r.item).and_then(|it| gear_geoms(it).get(r.gear).copied())?;
        g.radius?;
        Axis::new(g.centre, g.axis)
    };
    let axes: Vec<Option<Axis>> = (0..in_train.len()).map(axis_of).collect();
    // The meshes first, then the shared shafts; a shaft that would bring into one train two gears
    // on one axis that no shaft ties (one turning free on it: the train's reader would take them
    // as one compound) leaves the two trains apart, joined by the shaft across them as any
    // cluster across units is.
    let meshes = st.iter().filter(|s| matches!(s.kind, StageKind::Parallel | StageKind::Internal));
    for s in meshes.chain(st.iter().filter(|s| s.kind == StageKind::Shaft)) {
        let idx: Vec<usize> = s.gears.iter().filter_map(|r| in_train.iter().position(|g| g == r)).collect();
        for w in idx.windows(2) {
            let (a, b) = (find(&group, w[0]), find(&group, w[1]));
            if a == b {
                continue;
            }
            if s.kind == StageKind::Shaft {
                let (of_a, of_b): (Vec<usize>, Vec<usize>) =
                    (0..in_train.len()).filter(|x| [a, b].contains(&find(&group, *x))).partition(|x| find(&group, *x) == a);
                let loose = of_a
                    .iter()
                    .any(|x| of_b.iter().any(|y| find(&tied, *x) != find(&tied, *y) && axes[*x].zip(axes[*y]).is_some_and(|(p, q)| p.coaxial(&q))));
                if loose {
                    continue;
                }
            }
            group[a.max(b)] = a.min(b);
        }
    }
    let mut units = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    for x in 0..in_train.len() {
        let r = find(&group, x);
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    let stage_of = |r: GearRef| st.iter().position(|s| s.gears.contains(&r));
    let mut fail = |err: &mut [Option<String>], rs: &[GearRef], k: Option<usize>, e: String| {
        let at = k.or_else(|| rs.iter().find_map(|r| stage_of(*r)));
        match at {
            Some(k) => {
                err[k].get_or_insert(e);
            }
            None => loose.push(e),
        }
    };
    for r in roots {
        let gears: Vec<GearRef> = (0..in_train.len()).filter(|x| find(&group, *x) == r).map(|x| in_train[x]).collect();
        match train_unit(items, &gears, mapped, &block_spec(blocks, &gears, base)) {
            Ok(u) => units.push(u),
            Err(e) => fail(err, &gears, None, e),
        }
    }
    // The gears planned so far (module, teeth and body), which a later stage reuses.
    let mut planned: Vec<Planned> = Vec::new();
    let note = |planned: &mut Vec<Planned>, u: &Unit| {
        for (r, m) in u.refs.iter().zip(&u.plan.members) {
            if let (Some(r), Some(b)) = (r, m.body.and_then(|b| u.plan.bodies.get(b)))
                && !planned.iter().any(|p| p.gear == *r)
            {
                planned.push(Planned { gear: *r, module: m.module, teeth: m.teeth, pitch_d: m.pitch_diameter, body: b.clone() });
            }
        }
    };
    for u in &units {
        note(&mut planned, u);
    }
    for (k, s) in st.iter().enumerate() {
        let made = match s.kind {
            StageKind::Planetary => planetary_unit(items, s, blocks, &block_spec(blocks, &s.gears, base)),
            StageKind::Rack => rack_unit(items, s, blocks, &block_spec(blocks, &s.gears, base)),
            _ => continue,
        };
        match made {
            Ok(u) => units.push(u),
            Err(e) => fail(err, &s.gears, Some(k), e),
        }
    }
    for (k, s) in st.iter().enumerate().filter(|(_, s)| is_pair(s.kind)) {
        match pair_unit(items, s, mapped, &block_spec(blocks, &s.gears, base), &planned) {
            Ok(u) => {
                note(&mut planned, &u);
                units.push(u);
            }
            Err(e) => fail(err, &s.gears, Some(k), e),
        }
    }
    // Shaft items: each a bar along its line, joined to the gears its cluster holds.
    for (i, it) in items.iter().enumerate() {
        let Some(line) = crate::items::shaft_geom(it) else { continue };
        let me = GearRef::new(i, 0);
        let carried: Vec<GearRef> = st
            .iter()
            .filter(|s| s.kind == StageKind::Shaft && s.error.is_none() && s.gears.contains(&me))
            .flat_map(|s| s.gears.iter().copied())
            .filter(|r| items.get(r.item).is_some_and(|x| x.kind != ItemKind::Shaft))
            .collect();
        // The other Shaft items' lines in its cluster: a gear on one of them is that shaft's.
        let others: Vec<GearGeom> = st
            .iter()
            .filter(|s| s.kind == StageKind::Shaft && s.error.is_none() && s.gears.contains(&me))
            .flat_map(|s| s.gears.iter().copied())
            .filter(|r| r.item != i)
            .filter_map(|r| items.get(r.item).and_then(crate::items::shaft_geom))
            .collect();
        match shaft_unit(items, i, &line, &carried, &others, &planned, base) {
            Ok(u) => units.push(u),
            Err(e) => fail(err, &[me], None, e),
        }
    }
    units
}

/// A gear planned already: its module, teeth, pitch diameter and body (where it is).
struct Planned {
    gear: GearRef,
    module: f64,
    teeth: u32,
    pitch_d: f64,
    body: Body,
}

/// A Shaft item's bar: a round bar along its drawn line, from end to end, and on past an end to
/// a gear it carries on its axis there that no other Shaft item of its cluster (`others`, their
/// lines) holds (so a prop shaft reaches the pinion it drives); cut back later where another
/// body is in its way ([`trim_shafts`]). `carried`: the gears its cluster holds (joined to it by
/// the merge's clusters). Its diameter is the item's, else half the smallest root diameter of the
/// gears it carries (as a train's shaft). What it can't do is said in its warnings; `Err` when it
/// is wider than the tooth roots of a gear on its line (one body with it).
fn shaft_unit(
    items: &[GearItem],
    i: usize,
    line: &GearGeom,
    carried: &[GearRef],
    others: &[GearGeom],
    planned: &[Planned],
    base: &Spec,
) -> Result<Unit, String> {
    let it = &items[i];
    let me = GearRef::new(i, 0);
    let name = who(items, me);
    let (a, b) = line.line.unwrap_or((line.centre, add(line.centre, line.axis)));
    let u = line.axis;
    let length = len(sub(b, a));
    let mut p = Plan::default();
    // The gears it carries on its line (its cluster may hold gears on other shafts too).
    let on_line: Vec<GearRef> =
        carried.iter().copied().filter(|r| crate::items::link_geom(items, *r).is_some_and(|g| crate::items::on_shaft_line(line, &g))).collect();
    let sized = if on_line.is_empty() { carried } else { &on_line[..] };
    let roots: Vec<f64> =
        sized.iter().filter_map(|r| planned.iter().find(|x| x.gear == *r)).map(|x| x.pitch_d - 2.5 * x.module).filter(|d| *d > 0.0).collect();
    let d = it.shaft_diameter.filter(|d| *d > 0.0).or((base.bore.diameter > 0.0).then_some(base.bore.diameter)).unwrap_or_else(|| {
        match roots.iter().copied().reduce(f64::min) {
            Some(root) => (root.floor() / 2.0).max(1.0),
            None => (length / 16.0).clamp(2.0, 10.0),
        }
    });
    // A gear on its line is one body with it: the bar must stay inside the gear's tooth roots.
    for r in &on_line {
        let Some(x) = planned.iter().find(|x| x.gear == *r).filter(|x| x.module > 0.0) else { continue };
        let root = x.pitch_d - 2.5 * x.module;
        if d >= root - 0.05 * x.module {
            let bar = crate::hub::Bore { kind: crate::hub::BoreKind::Round, diameter: d, on_shaft: true, ..crate::hub::Bore::default() };
            return Err(format!("{}{}", who(items, *r), crate::hub::root_words(&bar, root, x.module)));
        }
    }
    // On past its ends to the gears it carries on its axis that no other shaft of its cluster holds.
    let (mut lo, mut hi) = (0.0f64, length);
    let mut reached: Vec<String> = Vec::new();
    for r in carried {
        let Some(g) = crate::items::link_geom(items, *r) else { continue };
        let coaxial = gear_on_line(u, &g, |c| Axis { origin: a, dir: u }.distance(c));
        if !coaxial || crate::items::on_shaft_line(line, &g) || others.iter().any(|o| crate::items::on_shaft_line(o, &g)) {
            continue;
        }
        // Into the middle of its body along the axis (a hypoid pinion's drawn place is where its
        // axis passes the ring's, past its teeth), else to its centre.
        let at = match planned.iter().find(|x| x.gear == *r) {
            Some(x) => {
                let (lo, hi) = body_span(&x.body, |q| dot(sub(q, a), u));
                0.5 * (lo + hi)
            }
            None => dot(sub(g.centre, a), u),
        };
        if at < lo || at > hi {
            (lo, hi) = (lo.min(at), hi.max(at));
            reached.push(format!("{} ({} mm past its line's end)", who(items, *r), num(if at < 0.0 { -at } else { at - length })));
        }
    }
    let length_drawn = length;
    let length = hi - lo;
    let frame = Frame { origin: a, x: square_to(u), z: u };
    let region = Region { outer: vec![Outline::circle(V2::ZERO, d / 2.0)], holes: vec![] };
    p.bodies.push(Body {
        name: format!("{name} Ø{}", num(d)),
        frame,
        steps: vec![Step::Prism { region, z0: lo, height: length, twist: 0.0, op: Op::New }],
    });
    p.assembly =
        Assembly { members: vec![AssemblyMember { body: 0, axis: Axis { origin: a, dir: u }, grounded: false, carrier: None }], links: Vec::new() };
    p.members = vec![SetMember {
        source: 0,
        module: 0.0,
        teeth: 0,
        pitch_diameter: 0.0,
        face_width: length,
        face_a: length,
        face_b: 0.0,
        ring: false,
        meshes_with: Vec::new(),
        cluster: None,
        body: Some(0),
    }];
    p.shafts.push(PlanShaft { body: 0, gears: vec![0], diameter: d, length });
    p.fact("diameter", d, crate::plan::Unit::Mm);
    p.fact("length", length, crate::plan::Unit::Mm);
    if carried.is_empty() {
        p.warn("carries no gear: it is made on its own and turns with nothing; draw its line along a gear's axis, or link it by Shaft to the gears it carries");
    }
    // Linked to gears its line doesn't reach (off its ends): the bar is made along its line all
    // the same and turns with them, but touches none of them.
    if !carried.is_empty() && on_line.is_empty() && reached.is_empty() {
        let near = carried
            .iter()
            .filter_map(|r| crate::items::link_geom(items, *r).map(|g| (*r, dot(sub(g.centre, a), u))))
            .filter(|(_, at)| *at < 0.0 || *at > length_drawn)
            .map(|(r, at)| (r, if at < 0.0 { -at } else { at - length_drawn }))
            .min_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((r, short)) = near {
            p.warn(format!(
                "its line reaches none of the gears it carries ({} is {} mm past its end): it is made along its line and turns with them; lengthen the line to reach the gear",
                who(items, r),
                num(short)
            ));
        }
    }
    let notes =
        if reached.is_empty() { Vec::new() } else { vec![format!("{name} runs on past its line to {}, which it carries", reached.join(" and "))] };
    Ok(Unit { label: name, plan: p, refs: vec![Some(me)], shifts: vec![None], carrier: None, tags: Vec::new(), contact: Vec::new(), notes })
}

/// The drivetrain reader's items for `gears` (each with its face sides), and the shaft lines and
/// bearings of their items (lines only when `lines`), with their names.
fn reader_items(
    items: &[GearItem],
    gears: &[GearRef],
    mapped: &crate::items::Mapped,
    lines: bool,
) -> Result<(Vec<dt::Item>, Vec<GearRef>, Vec<String>), String> {
    let mut its = Vec::new();
    let mut names = Vec::new();
    for &r in gears {
        let k =
            mapped.source.iter().position(|s| *s == Source::Gear(r)).ok_or_else(|| format!("{} can't be planned from its picks", who(items, r)))?;
        let mut it = mapped.items[k];
        it.ask.sides = items.get(r.item).map(|i| i.settings(r.gear).face.side_preset());
        its.push(it);
        names.push(who(items, r));
    }
    if lines {
        let owners: Vec<usize> = gears.iter().map(|r| r.item).collect();
        for (k, s) in mapped.source.iter().enumerate() {
            let (Source::Line { item, k: n } | Source::Point { item, k: n }) = *s else { continue };
            let on_axis = |it: &dt::Item| match it.thing {
                dt::Thing::Line(l) => gears
                    .iter()
                    .filter_map(|r| gear_geoms(&items[r.item]).get(r.gear).copied())
                    .any(|g| l.dir().is_some_and(|u| gear_on_line(u, &g, |c| l.distance(c)))),
                _ => true,
            };
            // (A Shaft item's line is a body of its own, [`shaft_unit`], joined to what it carries.)
            if owners.contains(&item) && on_axis(&mapped.items[k]) {
                its.push(mapped.items[k]);
                let what = if matches!(s, Source::Line { .. }) { "shaft line" } else { "bearing" };
                names.push(format!("{} {what} {}", items.get(item).map_or_else(String::new, GearItem::who), n + 1));
            }
        }
    }
    Ok((its, gears.to_vec(), names))
}

/// One train: gears on parallel shafts (see the module docs).
fn train_unit(items: &[GearItem], gears: &[GearRef], mapped: &crate::items::Mapped, base: &Spec) -> Result<Unit, String> {
    let (mut its, refs, names) = reader_items(items, gears, mapped, true)?;
    let has_lines = its.iter().any(|i| matches!(i.thing, dt::Thing::Line(_)));
    if has_lines {
        // A shaft only where one is drawn.
        for (k, r) in refs.iter().enumerate() {
            let g = gear_geoms(&items[r.item]).get(r.gear).copied();
            let drawn = its.iter().any(|i| match (i.thing, g) {
                (dt::Thing::Line(l), Some(g)) => l.dir().is_some_and(|u| gear_on_line(u, &g, |c| l.distance(c))),
                _ => false,
            });
            if !drawn {
                its[k].ask.shaft_body = Some(false);
            }
        }
    }
    // A ring's rim as its gear asks (else the planner's).
    let rim = gears
        .iter()
        .find_map(|r| items.get(r.item).filter(|i| i.kind == ItemKind::Internal).and_then(|i| i.settings(r.gear).rim))
        .filter(|v| *v > 0.0);
    let ringed;
    let base = match rim {
        Some(rim) => {
            ringed = Spec { rim, ..base.clone() };
            &ringed
        }
        None => base,
    };
    let o = dt::read_with(&its, None, &Simple::default(), base, has_lines);
    // (The plan's "gear N" is the reader's gear `o.gears[N - 1]`.)
    let plan_names = by_order(&o.gears, &names);
    if let Some(e) = &o.error {
        return Err(named(e, &plan_names, &names));
    }
    let solved = o.solved.as_ref().ok_or("the train could not be sized")?;
    let p = plan(&solved.spec).map_err(|e| named(&e, &plan_names, &names))?;
    let shifts: Vec<Option<f64>> = (0..p.members.len()).map(|k| solved.spec.train.as_ref().and_then(|t| t.gears.get(k)).map(|g| g.shift)).collect();
    let refs: Vec<Option<GearRef>> = o.gears.iter().map(|i| refs.get(*i).copied()).collect();
    let label = {
        let mut ids: Vec<String> = refs.iter().flatten().filter_map(|r| items.get(r.item).map(|i| i.id.clone())).collect();
        ids.dedup();
        format!("train {}", ids.join(", "))
    };
    let mut u = Unit { label, plan: p, refs, shifts, carrier: None, tags: Vec::new(), contact: Vec::new(), notes: Vec::new() };
    // (The plan's "gear N" is its member N, the train's gear N.)
    let by_gear: Vec<String> = u.refs.iter().enumerate().map(|(k, r)| r.map_or_else(|| format!("gear {}", k + 1), |r| who(items, r))).collect();
    u.plan.warnings = u.plan.warnings.iter().map(|w| named(w, &by_gear, &names)).collect();
    // A gear meshing nothing at a fixed module (decision 3): the closest tooth count at that
    // module, so its pitch circle is not the drawn one; say both.
    for &i in &o.gears {
        let (Some(m), Some(it)) = (o.members.get(i), its.get(i)) else { continue };
        let (dt::Thing::Circle(c), Some(module), Some(z), Some(d)) = (it.thing, it.ask.module, m.teeth, m.pitch_diameter) else { continue };
        if m.meshes_with.is_empty() && (d - 2.0 * c.r).abs() > 1e-6 {
            let name = names.get(i).cloned().unwrap_or_default();
            u.notes.push(format!("{name} meshes nothing: {z} teeth at module {} make it Ø{} (Ø{} drawn)", num(module), num(d), num(2.0 * c.r)));
        }
    }
    Ok(tagged(u, &solved.spec, None))
}

/// A bevel, hypoid, worm or crossed helical pair (see the module docs); `planned` the gears
/// planned already (their module and teeth), which this pair reuses.
fn pair_unit(items: &[GearItem], s: &Stage, mapped: &crate::items::Mapped, base: &Spec, planned: &[Planned]) -> Result<Unit, String> {
    let (mut its, refs, names) = reader_items(items, &s.gears, mapped, false)?;
    if its.len() != 2 {
        return Err(format!("{}: a pair needs two gears", names.join(" and ")));
    }
    let again: Vec<(usize, f64, u32)> =
        refs.iter().enumerate().filter_map(|(k, r)| planned.iter().find(|p| p.gear == *r).map(|p| (k, p.module, p.teeth))).collect();
    // The module: a gear planned already decides it; else what is asked; else decision 1's.
    let asked = its.iter().find_map(|i| i.ask.module);
    let module = match again.first() {
        Some(&(_, m, _)) => Some(m),
        None if asked.is_some() => None,
        None => {
            let kind = match s.kind {
                StageKind::Bevel => Kind::Bevel,
                StageKind::Hypoid => Kind::Hypoid,
                StageKind::Worm => Kind::Worm,
                _ => Kind::CrossedHelical,
            };
            let gs: Vec<StageGear> = s.geom.iter().filter_map(|g| g.radius).map(|r| StageGear::of(2.0 * r)).collect();
            let teeth = |k: usize| its.get(k).and_then(|i| i.ask.teeth);
            let gs: Vec<StageGear> = gs.iter().enumerate().map(|(k, g)| StageGear { teeth: teeth(k), ..*g }).collect();
            let shaft_angle = match (s.geom.first(), s.geom.get(1)) {
                (Some(a), Some(b)) => dot(a.axis, b.axis).clamp(-1.0, 1.0).acos().to_degrees(),
                _ => 90.0,
            };
            let probe = Spec { shaft_angle, ..base.clone() };
            size_stage(kind, &gs, None, &probe).ok().map(|z| z.module)
        }
    };
    if let Some(m) = module {
        for i in &mut its {
            i.ask.module = Some(m);
        }
    }
    if s.kind == StageKind::Bevel && s.bevel_style != BevelStyle::Straight {
        for i in &mut its {
            i.ask.role = Some(dt::Role::Gear(Kind::SpiralBevel));
        }
    }
    // What the item asks of the pair: a worm's thread starts, a spiral bevel's or hypoid's
    // spiral angle.
    let asked = |f: fn(&GearSettings) -> Option<f64>| {
        s.gears.iter().find_map(|r| items.get(r.item).and_then(|i| (0..i.gear_count().max(1)).find_map(|g| f(&i.settings(g)))))
    };
    // A hypoid's asked spiral angle sizes the set (its largest offset follows from it).
    let spiral_asked = asked(|g| g.spiral_angle);
    let hypoid_base;
    let read_base = match spiral_asked.filter(|a| s.kind == StageKind::Hypoid && a.is_finite() && a.abs() < 89.0) {
        Some(a) => {
            hypoid_base = Spec { spiral_angle: a, ..base.clone() };
            &hypoid_base
        }
        None => base,
    };
    let o = dt::read_with(&its, None, &Simple::default(), read_base, false);
    let e = o.error.clone().or_else(|| o.members.iter().find_map(|m| m.error.clone()));
    // (The plan's gear 1 is the reader's gear `o.gears[0]`, gear 2 the other.)
    let plan_names = if o.gears.len() == 2 { by_order(&o.gears, &names) } else { names.clone() };
    if let Some(e) = e {
        return Err(set_refusal(&e, &plan_names, &names));
    }
    let mut solved = o.solved.clone().ok_or_else(|| format!("{}: no set fits these shafts", names.join(" and ")))?;
    if s.kind == StageKind::Worm
        && let Some(z) = asked(|g| g.starts.map(f64::from)).filter(|z| *z >= 1.0)
    {
        solved.spec.starts = z as u32;
    }
    if matches!(s.kind, StageKind::Bevel | StageKind::Hypoid)
        && (s.bevel_style != BevelStyle::Straight || s.kind == StageKind::Hypoid)
        && let Some(a) = asked(|g| g.spiral_angle).filter(|a| a.is_finite() && a.abs() < 89.0)
    {
        solved.spec.spiral_angle = a;
    }
    // A chevron bevel pair: spiral teeth whose hand changes at mid-face.
    if s.kind == StageKind::Bevel && s.bevel_style == BevelStyle::Chevron {
        solved.spec.herringbone = true;
    }
    // A bevel pair's face left on Auto: at most a third of the cone distance (GEAR_UI_SPEC §3),
    // so the small ends aren't weak.
    let auto_face = s.gears.iter().all(|r| {
        items.get(r.item).is_none_or(|i| {
            let f = i.settings(r.gear).face;
            f.a.is_none() && f.b.is_none()
        })
    });
    if s.kind == StageKind::Bevel && auto_face {
        let sp = &mut solved.spec;
        let (z1, z2) = (f64::from(sp.teeth), f64::from(sp.teeth2));
        let sigma = sp.shaft_angle.to_radians();
        let d1 = sigma.sin().atan2(z2 / z1 + sigma.cos());
        let re = sp.module * z1 / (2.0 * d1.sin().max(1e-9));
        let third = (re / 3.0 * 10.0).floor() / 10.0;
        if third > 0.0 && sp.face_width > third {
            sp.face_width = third;
        }
    }
    let solved = &solved;
    let mut p = plan(&solved.spec).map_err(|e| set_refusal(&e, &plan_names, &names))?;
    // Which body is which gear: the one on its axis.
    let geom = |k: usize| s.geom.get(k).copied();
    let mut body_of = [None, None];
    for (k, slot) in body_of.iter_mut().enumerate() {
        let Some(g) = geom(k) else { continue };
        *slot = p.bodies.iter().position(|b| on_axis(&b.frame, g.centre, g.axis));
    }
    let [Some(b0), Some(b1)] = body_of else {
        return Err(format!("{}: the planned pair is not where the drawing puts it", names.join(" and ")));
    };
    // The plan's warnings name its gears by number, gear N its body N.
    if p.bodies.len() == 2 && b0 != b1 {
        let mut by_gear = vec![String::new(); 2];
        (by_gear[b0], by_gear[b1]) = (names[0].clone(), names[1].clone());
        p.warnings = p.warnings.iter().map(|w| gear_names(w, &by_gear)).collect();
    }
    let face = solved.spec.face_width;
    let mut members = Vec::new();
    for (k, b) in [(0usize, b0), (1, b1)] {
        let m = &o.members[k];
        // A worm's "teeth" are its thread starts, its pitch Ø its own.
        let worm = m.role == "worm";
        let sp = &solved.spec;
        members.push(SetMember {
            source: 0,
            module: m.module.unwrap_or(sp.module),
            teeth: if worm { sp.starts.max(1) } else { m.teeth.unwrap_or(0) },
            pitch_diameter: if worm && sp.worm_diameter > 0.0 { sp.worm_diameter } else { m.pitch_diameter.unwrap_or(0.0) },
            face_width: m.face_width.unwrap_or(face),
            face_a: m.face_width.unwrap_or(face),
            face_b: 0.0,
            ring: false,
            meshes_with: vec![1 - k],
            cluster: None,
            body: Some(b),
        });
    }
    // A gear planned already: the same module and teeth, or say what differs; then its new mate
    // turned with it onto the copy that is kept.
    if again.len() > 1 {
        return Err(format!(
            "{} are both meshed by other stages already: a mesh that closes a loop of stages of several kinds is not made yet; remove one link",
            names.join(" and ")
        ));
    }
    if let Some(&(k, m, z)) = again.first() {
        let got = &members[k];
        if (got.module - m).abs() > 1e-9 * m.max(1.0) || got.teeth != z {
            return Err(format!(
                "{} is planned with module {} and {} teeth, but this {} stage needs module {} and {} teeth: give its mates the same module, or draw it at a size both stages share",
                names[k],
                num(m),
                z,
                stage_name(s.kind),
                num(got.module),
                got.teeth
            ));
        }
        let kept = planned.iter().find(|p| p.gear == refs[k]).map(|p| &p.body).ok_or("no such gear")?;
        let (copy, mate) = if k == 0 { (b0, b1) } else { (b1, b0) };
        let c = &p.bodies[copy];
        let same_place =
            len(sub(c.frame.origin, kept.frame.origin)) <= 1e-6 * (1.0 + len(kept.frame.origin)) && dot(c.frame.z, kept.frame.z) > 1.0 - 1e-9;
        if c.steps != kept.steps || !same_place {
            return Err(format!(
                "{} meshes in two {} stages that need it cut differently (its teeth, cone or place differ between them): give both mates the same teeth and shaft angle, at the same place along it",
                names[k],
                stage_name(s.kind)
            ));
        }
        // The kept body is the copy turned δ about its axis: the mate turns with it.
        let y = cross(c.frame.z, c.frame.x);
        let delta = dot(kept.frame.x, y).atan2(dot(kept.frame.x, c.frame.x));
        // (Turns measured about each member's assembly axis, which may point against its frame's z.)
        let sign = |b: usize| p.assembly.members.get(b).map_or(1.0, |m| if dot(m.axis.dir, p.bodies[b].frame.z) < 0.0 { -1.0 } else { 1.0 });
        let ratio = p.assembly.ratio(copy, mate).unwrap_or(0.0) * sign(copy) * sign(mate);
        let f = &mut p.bodies[mate].frame;
        let turn = delta * ratio;
        let fy = cross(f.z, f.x);
        f.x = add(mul(f.x, turn.cos()), mul(fy, turn.sin()));
    }
    let label = names.iter().map(|n| n.split(" gear").next().unwrap_or_default().to_string()).collect::<Vec<_>>();
    let mut label = label;
    label.dedup();
    p.members = members;
    let refs = refs.iter().map(|r| Some(*r)).collect();
    // The duplicate of a gear planned already is dropped when merging; its member stays (refs).
    let u = Unit {
        label: format!("{} {}", stage_name(s.kind), label.join(", ")),
        plan: p,
        refs,
        shifts: vec![None, None],
        carrier: None,
        tags: Vec::new(),
        contact: Vec::new(),
        notes: Vec::new(),
    };
    Ok(tagged(u, &solved.spec, stage_mesh(s)))
}

/// Whether gear `g` sits on a line along `u`: parallel to it, its centre on it (`distance`: a
/// point's distance from the line).
fn gear_on_line(u: V, g: &GearGeom, distance: impl FnOnce(V) -> f64) -> bool {
    len(cross(u, g.axis)) < 1e-3 && distance(g.centre) <= crate::items::tol(g.radius.unwrap_or(1.0))
}

/// Where `body` reaches along a line: the least and most of `along` (a point's place on it)
/// over the corners of the body's box.
fn body_span(body: &Body, along: impl Fn(V) -> f64) -> (f64, f64) {
    let (lo, hi) = crate::mesh3d::Solid::new(body, 1.0).bounds();
    (0..8)
        .map(|k| along([if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] }]))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| (a.min(z), b.max(z)))
}

/// Whether `frame`'s z lies along the axis through `c` along `u`.
fn on_axis(frame: &Frame, c: V, u: V) -> bool {
    let d = sub(frame.origin, c);
    let off = sub(d, mul(u, dot(d, u)));
    len(cross(frame.z, u)) < 1e-6 && len(off) < 1e-6 * (1.0 + len(d)) + 1e-6
}

/// A planetary set with its carrier (see the module docs).
fn planetary_unit(items: &[GearItem], s: &Stage, blocks: &[ModuleBlock], base: &Spec) -> Result<Unit, String> {
    let item_index = s.gears.first().map(|r| r.item).ok_or("a planetary set needs its gears")?;
    let item = items.get(item_index).ok_or("no such item")?;
    let refs = s.gears.clone();
    // (Its sizing's and plan's gear 1 is the sun, gear 2 the planet.)
    let sun_planet: Vec<String> = [0, 2].iter().map(|k| refs.get(*k).map_or_else(|| item.who(), |r| who(items, *r))).collect();
    let (Some(gs), Some(gr), Some(gp)) = (s.geom.first().copied(), s.geom.get(1).copied(), s.geom.get(2).copied()) else {
        return Err(format!("{}: a planetary set needs its sun, ring and planet", item.who()));
    };
    let (Some(rs), Some(rr), Some(rp)) = (gs.radius, gr.radius, gp.radius) else {
        return Err(format!("{}: draw the sun's and the ring's pitch circles", item.who()));
    };
    // Decision 1: the sun's settings (or its block) give the module, else the automatic one for
    // all three; the sun and planet held at the drawn centre distance by profile shift (the
    // planner shifts the ring to reach the planets).
    let set = item.settings(0);
    let helix = set.helix.unwrap_or(0.0).abs() * if set.hand == Some(crate::items::Hand::Left) { -1.0 } else { 1.0 };
    let factor = 1.0 / helix.to_radians().cos();
    let module = resolve_module(blocks, item_index, &set).module.or_else(|| {
        let ds = [(2.0 * rs, factor), (2.0 * rp, factor), (2.0 * rr, factor)];
        Some(auto_module(&ds, 8).0)
    });
    let teeth_of = |k: usize| (item.settings(k).size_by == crate::items::SizeBy::Teeth).then(|| item.settings(k).teeth).flatten();
    let pair = size_stage(
        Kind::Pair,
        &[StageGear { d: 2.0 * rs, factor, teeth: teeth_of(0) }, StageGear { d: 2.0 * rp, factor, teeth: teeth_of(2) }],
        module,
        base,
    )
    .map_err(|e| set_refusal(&e, &sun_planet, &[item.who()]))?;
    let m = pair.module;
    let (zs, zp) = (pair.teeth.first().copied().unwrap_or(3), pair.teeth.get(1).copied().unwrap_or(3));
    let (xs, xp) = (pair.shift.first().copied().unwrap_or(0.0), pair.shift.get(1).copied().unwrap_or(0.0));
    let zr = teeth_of(1).unwrap_or_else(|| ((2.0 * rr / (m * factor)).round() as u32).max(zp + 3));
    let a = apart(&gp, &gs).max(1e-9);
    let ra_p = m * (f64::from(zp) / 2.0 + 1.0 + xp);
    let clears = |n: u32| n == 1 || 2.0 * a * (PI / f64::from(n)).sin() > 2.0 * ra_p + 0.5 * m;
    let n = match item.planets {
        Some(n) if n >= 1 => n,
        _ => (1..=3).rev().filter(|n| clears(*n)).max_by_key(|n| ((zs + zr) % n == 0, *n)).unwrap_or(1),
    };
    let (fa, fb) = crate::items::face_extent(&set, m);
    let face = fa + fb;
    let z = gs.axis;
    let x = unit(sub(gp.centre, add(gs.centre, mul(z, dot(sub(gp.centre, gs.centre), z))))).unwrap_or_else(|| square_to(z));
    let frame = Frame { origin: sub(gs.centre, mul(z, fb)), x, z };
    // The planets turn on the carrier's pins: a bore for each.
    let root_p = m * (f64::from(zp) / 2.0 - 1.25 + xp);
    let pin = (0.35 * 2.0 * root_p).max(0.5 * m);
    let clearance = 0.2;
    let spec = Spec {
        kind: Kind::Planetary,
        module: m,
        teeth: zs,
        teeth2: zp,
        teeth3: zr,
        planets: n,
        helix,
        herringbone: false,
        shift: xs,
        shift2: xp,
        face_width: face,
        train: None,
        origin: [0.0; 3],
        placement: Some(frame),
        only: 0,
        bore2: if item.carrier {
            crate::hub::Bore { kind: crate::hub::BoreKind::Round, diameter: pin + clearance, ..crate::hub::Bore::default() }
        } else {
            base.bore2
        },
        clearance_body: None,
        ..base.clone()
    };
    let mut p = plan(&spec).map_err(|e| set_refusal(&e, &sun_planet, &[item.who()]))?;
    let nb = p.bodies.len();
    if nb != n as usize + 2 {
        return Err(format!("{}: the planetary set came out with {nb} bodies", item.who()));
    }
    let ring_body = nb - 1;
    let pd = |z: u32| m * f64::from(z) / spec.helix.to_radians().cos();
    let member = |z: u32, ring: bool, body: usize, meshes: Vec<usize>| SetMember {
        source: 0,
        module: m,
        teeth: z,
        pitch_diameter: pd(z),
        face_width: face,
        face_a: fa,
        face_b: fb,
        ring,
        meshes_with: meshes,
        cluster: None,
        body: Some(body),
    };
    // Members: sun, ring, planet 1 (the item's gears in order), then planets 2...
    let planets: Vec<usize> = (1..=n as usize).collect();
    let pm = |k: usize| k + 1;
    let mut members =
        vec![member(zs, false, 0, planets.iter().map(|k| pm(*k)).collect()), member(zr, true, ring_body, planets.iter().map(|k| pm(*k)).collect())];
    for &k in &planets {
        members.push(member(zp, false, k, vec![0, 1]));
    }
    let mut refs: Vec<Option<GearRef>> = vec![refs.first().copied(), refs.get(1).copied(), refs.get(2).copied()];
    refs.extend((2..=n as usize).map(|_| None));
    let mut shifts = vec![Some(xs), None, Some(xp)];
    shifts.extend((2..=n as usize).map(|_| Some(xp)));
    p.members = members;
    let mut carrier = None;
    if item.carrier {
        let gap = 0.5 * m;
        let thick = (2.5 * m).max(2.0);
        let root_s = m * (f64::from(zs) / 2.0 - 1.25 + xs);
        let inner = (0.8 * root_s).max(0.5);
        let outer = a + 0.5 * pin + 1.5 * m;
        let thetas: Vec<f64> = (0..n).map(|k| TAU * (f64::from(k) * f64::from(zs + zr) / f64::from(n)).round() / f64::from(zs + zr)).collect();
        let mut steps = vec![Step::Prism {
            region: Region { outer: vec![Outline::circle(V2::ZERO, outer)], holes: vec![Outline::circle(V2::ZERO, inner)] },
            z0: -gap - thick,
            height: thick,
            twist: 0.0,
            op: Op::New,
        }];
        for th in &thetas {
            let c = V2::polar(a, *th);
            steps.push(Step::Prism {
                region: Region { outer: vec![Outline::circle(c, pin / 2.0)], holes: vec![] },
                z0: -gap - thick * 0.5,
                height: face + gap + thick * 0.5,
                twist: 0.0,
                op: Op::Join,
            });
        }
        let body = p.bodies.len();
        p.bodies.push(Body { name: format!("Carrier ({} planets)", n), frame, steps });
        p.assembly.members.push(AssemblyMember { body, axis: Axis { origin: frame.origin, dir: z }, grounded: false, carrier: None });
        for k in 1..=n as usize {
            if let Some(am) = p.assembly.members.get_mut(k) {
                am.carrier = Some(body);
            }
        }
        carrier = Some((item_index, body));
    }
    Ok(tagged(
        Unit { label: format!("planetary {}", item.who()), plan: p, refs, shifts, carrier, tags: Vec::new(), contact: Vec::new(), notes: Vec::new() },
        &spec,
        None,
    ))
}

/// How far gear `a`'s centre is from `b`'s axis.
fn apart(a: &GearGeom, b: &GearGeom) -> f64 {
    let d = sub(a.centre, b.centre);
    len(sub(d, mul(b.axis, dot(d, b.axis))))
}

/// A rack and its pinion (see the module docs).
fn rack_unit(items: &[GearItem], s: &Stage, blocks: &[ModuleBlock], base: &Spec) -> Result<Unit, String> {
    let (Some(&rr), Some(&pr)) = (s.gears.first(), s.gears.get(1)) else { return Err("a rack needs its pinion".into()) };
    let (rr, pr, rg, pg) = match (s.geom.first().copied(), s.geom.get(1).copied()) {
        (Some(a), Some(b)) if a.rack => (rr, pr, a, b),
        (Some(a), Some(b)) => (pr, rr, b, a),
        _ => return Err("a rack needs its pinion".into()),
    };
    let names = [who(items, rr), who(items, pr)];
    let ((la, lb), r) = match (rg.line, pg.radius) {
        (Some(l), Some(r)) => (l, r),
        _ => return Err(format!("{}: draw the rack's pitch line and the pinion's pitch circle", names[0])),
    };
    let u = unit(sub(lb, la)).ok_or("the rack's pitch line has no length")?;
    let settings = items.get(pr.item).map(|i| i.settings(pr.gear)).unwrap_or_default();
    let module = resolve_module(blocks, pr.item, &settings).module;
    let helix = settings.helix.unwrap_or(0.0).abs();
    let factor = 1.0 / helix.to_radians().cos();
    let sized = size_stage(
        Kind::Rack,
        &[StageGear { d: 2.0 * r, factor, teeth: if settings.size_by == crate::items::SizeBy::Teeth { settings.teeth } else { None } }],
        module,
        base,
    )
    .map_err(|e| set_refusal(&e, std::slice::from_ref(&names[1]), &[names[1].clone(), names[0].clone()]))?;
    let (m, z) = (sized.module, sized.teeth.first().copied().unwrap_or(3));
    let x = sized.shift.first().copied().unwrap_or(0.0);
    let (fa, fb) = crate::items::face_extent(&settings, m);
    let face = fa + fb;
    let axis = pg.axis;
    let foot = add(la, mul(u, dot(sub(pg.centre, la), u)));
    let toward = unit(sub(foot, pg.centre)).ok_or_else(|| format!("{}: the pinion's centre is on the rack's pitch line", names[1]))?;
    let pinion_frame = Frame { origin: sub(pg.centre, mul(axis, fb)), x: toward, z: axis };
    let style = if helix == 0.0 { Kind::Spur } else { Kind::Helical };
    let pinion = Spec {
        kind: style,
        module: m,
        teeth: z,
        shift: x,
        helix,
        face_width: face,
        train: None,
        origin: [0.0; 3],
        placement: Some(pinion_frame),
        only: 0,
        clearance_body: None,
        ..base.clone()
    };
    let mut p = plan(&pinion).map_err(|e| set_refusal(&e, std::slice::from_ref(&names[1]), std::slice::from_ref(&names[1])))?;
    // The rack: teeth along its line, pointing at the pinion, a space where the pinion's tooth 0
    // points.
    let pitch = PI * m * factor;
    let length = len(sub(lb, la));
    let count = ((length / pitch).round() as u32).max(3);
    let at = dot(sub(foot, la), u);
    let k = (at / pitch).round().clamp(1.0, f64::from(count - 1));
    let start = sub(foot, mul(u, k * pitch));
    // y = z × x must point at the pinion (−toward).
    let (rz, origin) =
        if dot(cross(axis, u), toward) < 0.0 { (axis, sub(start, mul(axis, fb))) } else { (mul(axis, -1.0), add(start, mul(axis, fa))) };
    let rack = Spec {
        kind: Kind::Rack,
        module: m,
        teeth: z,
        shift: 0.0,
        helix: 0.0,
        rack_teeth: count,
        face_width: face,
        train: None,
        origin: [0.0; 3],
        placement: Some(Frame { origin, x: u, z: rz }),
        only: 0,
        clearance_body: None,
        // Its back height as the rack item asks (else the planner's).
        rack_height: items.get(rr.item).and_then(|i| i.back_height).filter(|v| *v > 0.0).unwrap_or(base.rack_height),
        ..base.clone()
    };
    if helix != 0.0 {
        p.warn(format!("{}: a helical pinion's rack is built with straight teeth (not made yet): use a spur pinion on a rack", names[1]));
    }
    let rp = plan(&rack).map_err(|e| set_refusal(&e, std::slice::from_ref(&names[0]), std::slice::from_ref(&names[0])))?;
    let rack_body = p.bodies.len();
    p.bodies.extend(rp.bodies);
    p.facts.extend(rp.facts);
    // The pinion turning about its axis slides the rack along `u` (or back).
    let sign = if dot(cross(axis, toward), u) >= 0.0 { 1.0 } else { -1.0 };
    p.assembly = Assembly {
        members: vec![
            AssemblyMember { body: 0, axis: Axis { origin: pinion_frame.origin, dir: axis }, grounded: false, carrier: None },
            AssemblyMember { body: rack_body, axis: Axis { origin, dir: mul(u, sign) }, grounded: false, carrier: None },
        ],
        links: vec![AssemblyLink::RackPinion { pinion: 0, rack: 1, pitch_radius: m * f64::from(z) * factor / 2.0 }],
    };
    let pd = m * f64::from(z) * factor;
    p.members = vec![
        SetMember {
            source: 0,
            module: m,
            teeth: 0,
            pitch_diameter: 0.0,
            face_width: face,
            face_a: fa,
            face_b: fb,
            ring: false,
            meshes_with: vec![1],
            cluster: None,
            body: Some(rack_body),
        },
        SetMember {
            source: 0,
            module: m,
            teeth: z,
            pitch_diameter: pd,
            face_width: face,
            face_a: fa,
            face_b: fb,
            ring: false,
            meshes_with: vec![0],
            cluster: None,
            body: Some(0),
        },
    ];
    let u = Unit {
        label: format!("rack {}", names[0]),
        plan: p,
        refs: vec![Some(rr), Some(pr)],
        shifts: vec![None, Some(x)],
        carrier: None,
        tags: Vec::new(),
        contact: Vec::new(),
        notes: Vec::new(),
    };
    Ok(tagged(u, &pinion, Some(MeshKind::Rack)))
}

/// The units merged into one plan (see the module docs).
fn merge(st: &[Stage], units: Vec<Unit>, drive: Option<&Drive>, out: &mut SystemPlan) -> Merged {
    let mut p = Plan::default();
    let mut asm: Vec<AssemblyMember> = Vec::new();
    let mut edges: Vec<AssemblyLink> = Vec::new();
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut shifts: Vec<Option<f64>> = Vec::new();
    // Each body's units (for the collision check), kept beside the plan.
    let mut body_units: Vec<Vec<usize>> = Vec::new();
    // Each member's contact ratio (the least of its meshes').
    let mut contact: Vec<Option<f64>> = Vec::new();
    let stage_count = units.len();
    for (ui, u) in units.into_iter().enumerate() {
        let lp = u.plan;
        // Bodies: a gear met before keeps its body; the copy is dropped.
        let mut body_map: Vec<Option<usize>> = vec![None; lp.bodies.len()];
        let mut member_map: Vec<Option<usize>> = vec![None; lp.members.len()];
        for (k, m) in lp.members.iter().enumerate() {
            if let Some(r) = u.refs.get(k).copied().flatten()
                && let Some(&(_, g)) = out.member_of.iter().find(|(x, _)| *x == r)
            {
                member_map[k] = Some(g);
                if let (Some(b), Some(gb)) = (m.body, p.members.get(g).and_then(|m| m.body)) {
                    body_map[b] = Some(gb);
                    if let Some(bu) = body_units.get_mut(gb) {
                        bu.push(ui);
                    }
                }
            }
        }
        for (b, body) in lp.bodies.into_iter().enumerate() {
            if body_map[b].is_none() {
                body_map[b] = Some(p.bodies.len());
                let am = lp.assembly.members.iter().find(|m| m.body == b).cloned();
                asm.push(AssemblyMember {
                    body: p.bodies.len(),
                    ..am.unwrap_or(AssemblyMember {
                        body: 0,
                        axis: Axis { origin: body.frame.origin, dir: body.frame.z },
                        grounded: false,
                        carrier: None,
                    })
                });
                p.bodies.push(body);
                body_units.push(vec![ui]);
            }
        }
        // Carriers point at the merged bodies.
        for m in lp.assembly.members.iter() {
            if let (Some(gb), Some(c)) = (body_map.get(m.body).copied().flatten(), m.carrier)
                && let Some(gc) =
                    lp.assembly.members.get(c).and_then(|cm| body_map.get(cm.body).copied().flatten()).or_else(|| body_map.get(c).copied().flatten())
                && let Some(a) = asm.get_mut(gb)
            {
                a.carrier = Some(gc);
            }
        }
        for (k, m) in lp.members.iter().enumerate() {
            if member_map[k].is_none() {
                member_map[k] = Some(p.members.len());
                p.members.push(SetMember {
                    body: m.body.and_then(|b| body_map.get(b).copied().flatten()),
                    meshes_with: Vec::new(),
                    cluster: None,
                    ..m.clone()
                });
                shifts.push(u.shifts.get(k).copied().flatten());
                if let Some(r) = u.refs.get(k).copied().flatten() {
                    out.member_of.push((r, p.members.len() - 1));
                }
            }
        }
        let gm = |k: usize| member_map.get(k).copied().flatten();
        for (k, m) in lp.members.iter().enumerate() {
            let Some(g) = gm(k) else { continue };
            for j in m.meshes_with.iter().filter_map(|j| gm(*j)) {
                if j != g && !p.members[g].meshes_with.contains(&j) {
                    p.members[g].meshes_with.push(j);
                }
            }
        }
        clusters.extend(lp.clusters.iter().map(|c| c.iter().filter_map(|k| gm(*k)).collect::<Vec<_>>()));
        for sh in &lp.shafts {
            if let Some(b) = body_map.get(sh.body).copied().flatten() {
                p.shafts.push(PlanShaft { body: b, gears: sh.gears.iter().filter_map(|k| gm(*k)).collect(), ..sh.clone() });
            }
        }
        for l in &lp.assembly.links {
            let local = |m: usize| lp.assembly.members.get(m).and_then(|am| body_map.get(am.body).copied().flatten());
            let (a, b) = l.members();
            let (Some(ga), Some(gb)) = (local(a), local(b)) else { continue };
            edges.push(match l.clone() {
                AssemblyLink::Gear { ra, rb, internal, reverse, .. } => AssemblyLink::Gear { a: ga, b: gb, ra, rb, internal, reverse },
                AssemblyLink::RackPinion { pitch_radius, .. } => AssemblyLink::RackPinion { pinion: ga, rack: gb, pitch_radius },
                AssemblyLink::Worm { starts, wheel_teeth, .. } => AssemblyLink::Worm { worm: ga, wheel: gb, starts, wheel_teeth },
                AssemblyLink::Rigid { .. } => AssemblyLink::Rigid { a: ga, b: gb },
            });
        }
        out.notes.extend(u.notes.iter().cloned());
        for t in &u.tags {
            if let (Some(a), Some(b)) = (body_map.get(t.bodies.0).copied().flatten(), body_map.get(t.bodies.1).copied().flatten())
                && !out.mesh_tags.iter().any(|x| x.bodies == (a, b) || x.bodies == (b, a))
            {
                out.mesh_tags.push(MeshTag { bodies: (a, b), ..*t });
            }
        }
        for &(k, cr) in &u.contact {
            if let Some(g) = gm(k) {
                if contact.len() <= g {
                    contact.resize(g + 1, None);
                }
                contact[g] = Some(contact[g].map_or(cr, |have: f64| have.min(cr)));
            }
        }
        for c in lp.contacts {
            if let (Some(a), Some(b)) = (gm(c.a), gm(c.b)) {
                p.contacts.push(crate::plan::Contact { a, b, ..c });
            }
        }
        for sh in lp.shapes {
            if let Some(m) = gm(sh.member) {
                p.shapes.push(crate::plan::MemberShape { member: m, ..sh });
            }
        }
        for f in lp.facts {
            p.facts.push(crate::plan::Fact { label: format!("{}: {}", u.label, f.label), ..f });
        }
        for w in lp.warnings {
            p.warn(format!("{}: {w}", u.label));
        }
        if let Some((item, b)) = u.carrier
            && let Some(gb) = body_map.get(b).copied().flatten()
        {
            out.carriers.push((item, gb));
        }
    }
    // Shaft links across units: one rigid member.
    for s in st.iter().filter(|s| s.kind == StageKind::Shaft) {
        let ms: Vec<usize> = s.gears.iter().filter_map(|r| out.member_of.iter().find(|(g, _)| g == r).map(|(_, m)| *m)).collect();
        clusters.push(ms);
    }
    let n = p.members.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let root = |p: &[usize], mut x: usize| {
        while p[x] != x {
            x = p[x];
        }
        x
    };
    for c in &clusters {
        for w in c.windows(2) {
            if w[0] < n && w[1] < n {
                let (a, b) = (root(&parent, w[0]), root(&parent, w[1]));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for x in 0..n {
        let r = root(&parent, x);
        match groups.iter_mut().find(|g| g.first().is_some_and(|f| root(&parent, *f) == r)) {
            Some(g) => g.push(x),
            None => groups.push(vec![x]),
        }
    }
    for g in groups.into_iter().filter(|g| g.len() > 1) {
        let k = p.clusters.len();
        for &x in &g {
            p.members[x].cluster = Some(k);
        }
        if let Some(first) = g.first().and_then(|f| p.members[*f].body) {
            for b in g.iter().skip(1).filter_map(|x| p.members[*x].body) {
                if b != first {
                    edges.push(AssemblyLink::Rigid { a: first, b });
                }
            }
        }
        p.clusters.push(g);
    }
    same_way_round(&mut asm, &mut edges);
    // One assembly driven from the drive gear.
    let driver = drive.map(|d| GearRef::new(d.item, d.gear)).or_else(|| out.member_of.first().map(|m| m.0));
    let root_body =
        driver.and_then(|r| out.member_of.iter().find(|(g, _)| *g == r)).and_then(|(_, m)| p.members.get(*m)).and_then(|m| m.body).unwrap_or(0);
    let (a, issues) = Assembly::from_graph(asm, &edges, root_body);
    p.assembly = a;
    p.assembly_issues = issues;
    out.driver = (!p.bodies.is_empty()).then_some(root_body);
    out.plan = p;
    // The bodies the feature makes: one per rigid member (a cluster's gears are one body).
    let made = super::solid::members(&out.plan).len();
    out.notes.push(format!(
        "{made} {} from {stage_count} {}",
        if made == 1 { "body" } else { "bodies" },
        if stage_count == 1 { "stage" } else { "stages" }
    ));
    contact.resize(out.plan.members.len(), None);
    Merged { body_units, shifts, contact }
}

/// The bodies of each rigid member turned about one axis direction: a rigid link turns its two
/// members the same amount about their own axes, so two bodies on one shaft whose axes point
/// opposite ways (a bevel gear's toward its apex, the pinion it carries toward its own crossing)
/// would turn opposite ways. Each such body's axis is turned round to its member's first body's,
/// and each gear link at it turns the other way about the new axis (`reverse`). A body whose
/// worm or rack link can't be turned round is kept as it is, and its member taken from it.
fn same_way_round(asm: &mut [AssemblyMember], edges: &mut [AssemblyLink]) {
    let n = asm.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for e in edges.iter() {
        if let AssemblyLink::Rigid { a, b } = *e
            && a < n
            && b < n
        {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    // Bodies whose links can only turn one way about their axis: a worm, a rack's pinion.
    let fixed: Vec<bool> = (0..n)
        .map(|k| {
            edges.iter().any(|e| {
                matches!(e, AssemblyLink::Worm { .. } | AssemblyLink::RackPinion { .. }) && {
                    let (a, b) = e.members();
                    a == k || b == k
                }
            })
        })
        .collect();
    let mut flipped = vec![false; n];
    let mut seen: Vec<usize> = Vec::new();
    for k in 0..n {
        let r = root(&mut parent, k);
        if seen.contains(&r) {
            continue;
        }
        seen.push(r);
        let group: Vec<usize> = (0..n).filter(|x| root(&mut parent, *x) == r).collect();
        if group.len() < 2 {
            continue;
        }
        let reference = group.iter().copied().find(|x| fixed[*x]).unwrap_or(group[0]);
        let d0 = asm[reference].axis.dir;
        for &x in &group {
            if x != reference && !fixed[x] && dot(asm[x].axis.dir, d0) < 0.0 {
                asm[x].axis.dir = mul(asm[x].axis.dir, -1.0);
                flipped[x] = true;
            }
        }
    }
    for e in edges.iter_mut() {
        if let AssemblyLink::Gear { a, b, reverse, .. } = e
            && flipped.get(*a).copied().unwrap_or(false) != flipped.get(*b).copied().unwrap_or(false)
        {
            *reverse = !*reverse;
        }
    }
}

/// A body's swept volume about an axis: per band along it, the least and most radius its
/// material reaches.
struct Sweep {
    axis: Axis,
    z0: f64,
    dz: f64,
    bands: Vec<(f64, f64)>,
    size: f64,
}

impl Sweep {
    fn of(body: &Body, axis: Axis, hollow: bool) -> Option<Sweep> {
        let _span = solidcraft_profile::span("gear", "sweep");
        let probe = crate::mesh3d::Solid::new(body, 1.0);
        let size = probe.radius.max(probe.z.1 - probe.z.0).max(1e-3);
        let h = (size / 40.0).max(0.05);
        let solid = crate::mesh3d::Solid::new(body, h);
        let pts: Vec<V> = solid.material_points(h).into_iter().map(|q| solid.world(q)).collect();
        let rz = |p: V| {
            let d = sub(p, axis.origin);
            let z = dot(d, axis.dir);
            (len(sub(d, mul(axis.dir, z))), z)
        };
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for p in &pts {
            let (_, z) = rz(*p);
            lo = lo.min(z);
            hi = hi.max(z);
        }
        if pts.is_empty() || hi < lo {
            return None;
        }
        let nb = (((hi - lo) / h).ceil() as usize).clamp(1, 200);
        let dz = ((hi - lo) / nb as f64).max(1e-9);
        let mut bands = vec![(f64::MAX, f64::MIN); nb];
        for p in &pts {
            let (r, z) = rz(*p);
            let k = (((z - lo) / dz) as usize).min(nb - 1);
            bands[k] = (bands[k].0.min(r), bands[k].1.max(r));
        }
        // Bands with no points take their neighbours'.
        for k in 0..nb {
            if bands[k].1 < bands[k].0 {
                let near = (1..nb)
                    .find_map(|d| [k.checked_sub(d), (k + d < nb).then_some(k + d)].into_iter().flatten().find(|j| bands[*j].1 >= bands[*j].0));
                bands[k] = near.map_or((0.0, 0.0), |j| bands[j]);
            }
        }
        if !hollow {
            for b in &mut bands {
                b.0 = 0.0;
            }
        }
        Some(Sweep { axis, z0: lo, dz, bands, size })
    }
    /// Whether world point `p` is inside the sweep by more than `eps`.
    fn holds(&self, p: V, eps: f64) -> bool {
        let d = sub(p, self.axis.origin);
        let z = dot(d, self.axis.dir);
        let top = self.z0 + self.dz * self.bands.len() as f64;
        if z < self.z0 + eps || z > top - eps {
            return false;
        }
        let k = (((z - self.z0) / self.dz) as usize).min(self.bands.len() - 1);
        let r = len(sub(d, mul(self.axis.dir, z)));
        let (a, b) = self.bands[k];
        r < b - eps && (a <= 0.0 || r > a + eps)
    }
    /// A point of this sweep inside `other`'s, sampled `h` apart.
    fn meets(&self, other: &Sweep, eps: f64) -> Option<V> {
        let _span = solidcraft_profile::span("gear", "sweeps meet");
        let h = (self.size.min(other.size) / 12.0).max(0.1);
        let u = self.axis.dir;
        let e1 = square_to(u);
        let e2 = cross(u, e1);
        for (k, (a, b)) in self.bands.iter().enumerate() {
            let z = self.z0 + self.dz * (k as f64 + 0.5);
            let centre = add(self.axis.origin, mul(u, z));
            let mut r = *a;
            loop {
                let n = ((TAU * r / h).ceil() as usize).clamp(1, 2000);
                for i in 0..n {
                    let t = TAU * i as f64 / n as f64;
                    let q = add(centre, add(mul(e1, r * t.cos()), mul(e2, r * t.sin())));
                    if other.holds(q, eps) {
                        return Some(q);
                    }
                }
                if r >= *b {
                    break;
                }
                r = (r + h).min(*b);
            }
        }
        None
    }
}

/// The volume each body of a plan sweeps as it turns ([`body_sweep`]), worked out once each.
#[derive(Default)]
struct Sweeps(Vec<Option<Option<Sweep>>>);

impl Sweeps {
    fn get(&mut self, p: &Plan, b: usize) -> Option<&Sweep> {
        if self.0.len() < p.bodies.len() {
            self.0.resize_with(p.bodies.len(), || None);
        }
        if self.0[b].is_none() {
            self.0[b] = Some(body_sweep(p, b));
        }
        self.0[b].as_ref().and_then(Option::as_ref)
    }
    /// Body `b` changed: its sweep is worked out again when next asked for.
    fn forget(&mut self, b: usize) {
        if let Some(s) = self.0.get_mut(b) {
            *s = None;
        }
    }
}

/// The volume body `b` sweeps as it turns: about its assembly axis (a planet about its carrier's,
/// hollow like a ring's); a rack slides, so its own body.
fn body_sweep(p: &Plan, b: usize) -> Option<Sweep> {
    let carrier_of = |b: usize| p.assembly.members.get(b).and_then(|m| m.carrier);
    let axis = match carrier_of(b).and_then(|c| p.assembly.members.get(c)) {
        Some(c) => c.axis,
        None => p.assembly.members.get(b).map_or(Axis { origin: p.bodies[b].frame.origin, dir: p.bodies[b].frame.z }, |m| m.axis),
    };
    let ring = p.members.iter().any(|m| m.body == Some(b) && m.ring);
    let rack = p.assembly.links.iter().any(|l| matches!(l, AssemblyLink::RackPinion { rack, .. } if *rack == b));
    if rack {
        // A rack slides: its envelope is its body (a sweep about its own z).
        let f = p.bodies[b].frame;
        return Sweep::of(&p.bodies[b], Axis { origin: f.origin, dir: f.z }, false);
    }
    Sweep::of(&p.bodies[b], axis, ring || carrier_of(b).is_some())
}

/// The bodies one rigid member with body `b` (one body, or a cluster's), `b` among them.
fn rigid_with(p: &Plan, b: usize) -> Vec<usize> {
    let clusters: Vec<usize> = p.members.iter().filter(|m| m.body == Some(b)).filter_map(|m| m.cluster).collect();
    let mut out = vec![b];
    for m in &p.members {
        if let (Some(c), Some(x)) = (m.cluster, m.body)
            && clusters.contains(&c)
            && !out.contains(&x)
        {
            out.push(x);
        }
    }
    out
}

/// Every shaft cut back where another body (not one rigid member with it) is in its way: a
/// Shaft item's bar runs along its line as far as it goes, and a gear's shaft along its drawn
/// line, but each stops short of a gear it doesn't carry (on its line, or a bevel's mate where
/// two shafts meet). The stretch between the gears it carries is never cut: what is in the way
/// there is a collision, said by [`collisions`]. Each cut is said in the notes.
fn trim_shafts(items: &[GearItem], out: &mut SystemPlan, sweeps: &mut Sweeps) {
    // Clear of a body in the way by this much (mm); the sweeps' own tolerance.
    const GAP: f64 = 0.5;
    let eps = 0.05;
    let mut cuts: Vec<(usize, usize, f64, f64, String)> = Vec::new();
    {
        let p = &out.plan;
        // Each body's bounds, worked out once (not once per shaft).
        let bounds: Vec<([f64; 3], [f64; 3])> = p.bodies.iter().map(|b| crate::mesh3d::Solid::new(b, 1.0).bounds()).collect();
        let is_shaft = |r: &GearRef| items.get(r.item).is_some_and(|it| it.kind == ItemKind::Shaft);
        let on_body = |b: usize, shaft: bool| {
            out.member_of.iter().find(|(r, m)| is_shaft(r) == shaft && p.members.get(*m).and_then(|x| x.body) == Some(b)).map(|(r, _)| *r)
        };
        let name = |b: usize| {
            on_body(b, false)
                .or_else(|| on_body(b, true))
                .map_or_else(|| p.bodies.get(b).map_or_else(String::new, |x| x.name.clone()), |r| who(items, r))
        };
        for (k, sh) in p.shafts.iter().enumerate() {
            let Some(body) = p.bodies.get(sh.body) else { continue };
            let r = sh.diameter / 2.0;
            // Its bar: a prism of the shaft's circle about the body's z, the shaft's length.
            let bar = body.steps.iter().position(|s| match s {
                Step::Prism { region, height, twist, .. } => {
                    *twist == 0.0
                        && (height - sh.length).abs() <= 1e-6
                        && region.holes.is_empty()
                        && matches!(&region.outer[..], [o] if matches!(&o.segs[..], [crate::geom::Seg::Circle(c, cr)] if c.len() <= 1e-9 && (cr - r).abs() <= 1e-9))
                }
                _ => false,
            });
            let Some(j) = bar else { continue };
            let Step::Prism { z0, height, .. } = body.steps[j] else { continue };
            let axis = Axis { origin: body.frame.origin, dir: body.frame.z };
            let rigid = rigid_with(p, sh.body);
            // The gears it carries on its axis: each the stretch along it that the shaft must
            // reach (a gear on the shaft's own body its centre; another body as far as it goes
            // along the axis, so the shaft may stop inside it, short of what lies beyond).
            let along = |q: V| dot(sub(q, axis.origin), axis.dir);
            let anchors: Vec<(f64, f64)> = out
                .member_of
                .iter()
                .filter(|(g, m)| !is_shaft(g) && p.members.get(*m).and_then(|x| x.body).is_some_and(|x| rigid.contains(&x)))
                .filter_map(|(g, m)| Some((crate::items::link_geom(items, *g)?, p.members.get(*m)?.body?)))
                .filter(|(g, _)| gear_on_line(axis.dir, g, |c| axis.distance(c)))
                .map(|(g, b)| {
                    let c = along(g.centre);
                    if b == sh.body {
                        return (c, c);
                    }
                    let (lo, hi) = body_span(&p.bodies[b], along);
                    (lo.min(c), hi.max(c))
                })
                .filter_map(|(a, b)| {
                    let (a, b) = (a.max(z0), b.min(z0 + height));
                    (a <= b).then_some((a, b))
                })
                .collect();
            // Which stretches along it another body takes (the first body found in each).
            let n = ((height / 0.25).ceil() as usize).clamp(8, 400);
            let dz = height / n as f64;
            let mut blocked: Vec<Option<usize>> = vec![None; n];
            let at = |z: f64| add(axis.origin, mul(axis.dir, z));
            let e1 = square_to(axis.dir);
            let e2 = cross(axis.dir, e1);
            let (ends0, ends1) = (at(z0), at(z0 + height));
            let lo: V = std::array::from_fn(|i| ends0[i].min(ends1[i]) - r);
            let hi: V = std::array::from_fn(|i| ends0[i].max(ends1[i]) + r);
            for (o, &ob) in bounds.iter().enumerate().take(p.bodies.len()) {
                if rigid.contains(&o) {
                    continue;
                }
                let orbits = p.assembly.members.get(o).is_some_and(|m| m.carrier.is_some());
                if !orbits && (0..3).any(|i| hi[i] < ob.0[i] - 1.0 || ob.1[i] < lo[i] - 1.0) {
                    continue;
                }
                let Some(sw) = sweeps.get(p, o) else { continue };
                for (i, slot) in blocked.iter_mut().enumerate() {
                    if slot.is_some() {
                        continue;
                    }
                    let c = at(z0 + dz * (i as f64 + 0.5));
                    let ring = (0..12).flat_map(|q| {
                        let t = TAU * f64::from(q) / 12.0;
                        [0.5 * r, r].map(|rr| add(c, add(mul(e1, rr * t.cos()), mul(e2, rr * t.sin()))))
                    });
                    if std::iter::once(c).chain(ring).any(|q| sw.holds(q, eps)) {
                        *slot = Some(o);
                    }
                }
            }
            if blocked.iter().all(Option::is_none) {
                continue;
            }
            // The free runs of bands; the one kept reaches the most gears it carries (then the
            // longest).
            let mut runs: Vec<(usize, usize)> = Vec::new();
            let mut start = None;
            // One more, blocked, step past the last band closes a run that reaches the end.
            for (i, free) in blocked.iter().map(Option::is_none).chain([false]).enumerate() {
                match (free, start) {
                    (true, None) => start = Some(i),
                    (false, Some(s)) => {
                        runs.push((s, i - 1));
                        start = None;
                    }
                    _ => {}
                }
            }
            let reaches = |(a, b): (usize, usize)| {
                anchors.iter().filter(|(lo, hi)| *hi >= z0 + dz * a as f64 - 1e-9 && *lo <= z0 + dz * (b + 1) as f64 + 1e-9).count()
            };
            let Some((from_band, to_band)) = runs.iter().copied().max_by(|x, y| reaches(*x).cmp(&reaches(*y)).then((x.1 - x.0).cmp(&(y.1 - y.0))))
            else {
                continue;
            };
            if reaches((from_band, to_band)) < anchors.len() && anchors.iter().any(|(lo, hi)| lo == hi) {
                // A gear on the shaft's own body is in the way's stretch: a collision, said there.
                continue;
            }
            let below = (0..from_band).rev().find(|i| blocked[*i].is_some());
            let above = (to_band + 1..n).find(|i| blocked[*i].is_some());
            let from = below.map_or(z0, |i| z0 + dz * (i + 1) as f64 + GAP);
            let to = above.map_or(z0 + height, |i| z0 + dz * i as f64 - GAP);
            if to - from < 1e-3 || (below.is_none() && above.is_none()) {
                continue;
            }
            let mut by: Vec<String> = [below, above].into_iter().flatten().filter_map(|i| blocked[i]).map(name).collect();
            by.dedup();
            let shaft = on_body(sh.body, true).map_or_else(|| format!("the shaft of {}", name(sh.body)), |g| who(items, g));
            cuts.push((
                k,
                j,
                from,
                to,
                format!("{shaft} is cut back to {} mm (of {} mm) to stay clear of {}", num(to - from), num(height), by.join(" and ")),
            ));
        }
    }
    for (k, j, from, to, note) in cuts {
        let b = out.plan.shafts[k].body;
        if let Some(Step::Prism { z0, height, .. }) = out.plan.bodies[b].steps.get_mut(j) {
            (*z0, *height) = (from, to - from);
        }
        out.plan.shafts[k].length = to - from;
        sweeps.forget(b);
        out.notes.push(note);
    }
}

/// Bodies of different stages that would sweep into each other (not meshing, not one rigid
/// member): what collides and what to do.
fn collisions(items: &[GearItem], out: &SystemPlan, units: &[Vec<usize>], sweeps: &mut Sweeps) -> Option<String> {
    let p = &out.plan;
    let gear_of = |b: usize| out.member_of.iter().find(|(_, m)| p.members.get(*m).and_then(|m| m.body) == Some(b)).map(|(r, _)| *r);
    let name = |b: usize| gear_of(b).map_or_else(|| p.bodies.get(b).map_or_else(String::new, |x| x.name.clone()), |r| who(items, r));
    let members_on = |b: usize| (0..p.members.len()).filter(move |k| p.members[*k].body == Some(b));
    let meshing = |a: usize, b: usize| members_on(a).any(|x| members_on(b).any(|y| p.members[x].meshes_with.contains(&y)));
    let rigid = |a: usize, b: usize| {
        members_on(a).any(|x| members_on(b).any(|y| p.members[x].cluster.is_some() && p.members[x].cluster == p.members[y].cluster))
    };
    let carrier_of = |b: usize| p.assembly.members.get(b).and_then(|m| m.carrier);
    let eps = 0.05;
    // Each body's bounds, worked out once (not once per pair).
    let bounds: Vec<([f64; 3], [f64; 3])> = p.bodies.iter().map(|b| crate::mesh3d::Solid::new(b, 1.0).bounds()).collect();
    for a in 0..p.bodies.len() {
        for b in a + 1..p.bodies.len() {
            let (ua, ub) = (units.get(a).cloned().unwrap_or_default(), units.get(b).cloned().unwrap_or_default());
            if ua.iter().any(|u| ub.contains(u)) || meshing(a, b) || rigid(a, b) {
                continue;
            }
            let (ba, bb) = (bounds[a], bounds[b]);
            if (0..3).any(|i| ba.1[i] < bb.0[i] - 1.0 || bb.1[i] < ba.0[i] - 1.0) && carrier_of(a).is_none() && carrier_of(b).is_none() {
                continue;
            }
            if sweeps.get(p, a).is_none() || sweeps.get(p, b).is_none() {
                continue;
            }
            let (Some(Some(sa)), Some(Some(sb))) = (&sweeps.0[a], &sweeps.0[b]) else { continue };
            if let Some(q) = sa.meets(sb, eps).or_else(|| sb.meets(sa, eps)) {
                return Some(format!(
                    "{} and {} collide (near {}, {}, {}): they are in different stages and turn into each other; move one (along its shaft or apart), make it smaller, or link them if they share a shaft",
                    name(a),
                    name(b),
                    num(q[0]),
                    num(q[1]),
                    num(q[2])
                ));
            }
        }
    }
    None
}

/// The per-gear and per-link replies.
fn replies(
    items: &[GearItem],
    st: &[Stage],
    used: &[ItemLink],
    drive: Option<&Drive>,
    shifts: &[Option<f64>],
    contact: &[Option<f64>],
    out: &mut SystemPlan,
) {
    let p = &out.plan;
    let body = |r: GearRef| out.member_of.iter().find(|(g, _)| *g == r).and_then(|(_, m)| p.members.get(*m)).and_then(|m| m.body);
    let root = out.driver;
    let states = match (drive.and_then(|d| d.rpm), root) {
        (Some(rpm), Some(r)) if !p.assembly.members.is_empty() => {
            drive_table(&p.assembly, r, rpm, drive.and_then(|d| d.torque).unwrap_or(0.0), &out.efficiencies(&p.assembly))
        }
        _ => Vec::new(),
    };
    let mut gears = Vec::new();
    for (i, it) in items.iter().enumerate() {
        for g in 0..gear_geoms(it).len() {
            let r = GearRef::new(i, g);
            let mut reply = GearReply { item: i, gear: g, ..GearReply::default() };
            if let Some((_, k)) = out.member_of.iter().find(|(x, _)| *x == r)
                && let Some(m) = p.members.get(*k)
            {
                (reply.teeth, reply.module, reply.pitch_d) = (m.teeth, m.module, m.pitch_diameter);
                reply.profile_shift = shifts.get(*k).copied().flatten();
                reply.contact_ratio = contact.get(*k).copied().flatten();
                if let (Some(b), Some(rt)) = (m.body, root) {
                    reply.ratio = p.assembly.ratio(rt, b);
                    if let Some(s) = states.get(b).filter(|s| s.reached) {
                        reply.rpm = Some(s.rpm);
                        reply.torque = drive.and_then(|d| d.torque).map(|_| s.torque);
                        reply.warnings.extend(s.note.clone());
                    }
                }
            }
            gears.push(reply);
        }
    }
    out.gears = gears;
    let mut links = Vec::new();
    for (k, l) in used.iter().enumerate() {
        let stage = st.iter().find(|s| match &s.from {
            StageFrom::Link(j) => *j == k,
            StageFrom::Cluster { links, .. } => links.contains(&k),
            StageFrom::Item(_) => s.gears.contains(&l.a) && s.gears.contains(&l.b) && l.kind == LinkKind::Mesh,
        });
        let error = stage.and_then(|s| s.error.clone());
        let ratio = match (body(l.a), body(l.b)) {
            (Some(a), Some(b)) if error.is_none() => p.assembly.ratio(a, b),
            _ => None,
        };
        links.push(LinkReply { a: l.a, b: l.b, kind: l.kind, ratio, hint: error.as_deref().and_then(hint_of), error });
    }
    out.links = links;
}

/// The drivetrain reader's way in for a set of stages of several kinds (more than two gears, not
/// all on parallel shafts): the circles made items, the meshes found from where they touch, the
/// items planned by [`plan_items`] and the answer put back per member.
pub(crate) fn read_mixed(
    items: &[dt::Item],
    gears: &[(usize, dt::Circle3)],
    role: &[Option<dt::Role>],
    style: Kind,
    base: &Spec,
    out: &mut Outcome,
    err: &mut [Option<String>],
) {
    use crate::items::Thing;
    let mut gi: Vec<GearItem> = Vec::new();
    let mut of_item: Vec<usize> = Vec::new();
    for &(i, c) in gears {
        let kind = match role.get(i).copied().flatten() {
            Some(dt::Role::Gear(Kind::Helical)) => ItemKind::Helical,
            Some(dt::Role::Gear(Kind::Herringbone)) => ItemKind::Herringbone,
            Some(dt::Role::Gear(Kind::Spur | Kind::Bevel | Kind::SpiralBevel)) => ItemKind::Spur,
            Some(dt::Role::Internal) => ItemKind::Internal,
            Some(r) => {
                if let Some(e) = err.get_mut(i) {
                    e.get_or_insert(format!("member {}: a {} among more than two gears not all on parallel shafts is not made yet: use the Gears feature's items (a Worm or Rack item)", i + 1, r.name()));
                }
                continue;
            }
            None => match style {
                Kind::Helical => ItemKind::Helical,
                Kind::Herringbone => ItemKind::Herringbone,
                _ => ItemKind::Spur,
            },
        };
        if !matches!(items.get(i).map(|x| x.thing), Some(dt::Thing::Circle(_))) {
            if let Some(e) = err.get_mut(i) {
                e.get_or_insert(format!(
                    "member {}: a gear on a shaft line among stages of several kinds is not made yet: draw its pitch circle",
                    i + 1
                ));
            }
            continue;
        }
        let a = items[i].ask;
        let mut item = GearItem::new(&(i + 1).to_string(), kind).with(vec![Thing::circle(c.c, c.n, c.r)]);
        let mut s = GearSettings { module: a.module, teeth: a.teeth, helix: a.helix, v: a.herringbone == Some(true), ..GearSettings::default() };
        if a.teeth.is_some() && a.module.is_none() {
            s.size_by = crate::items::SizeBy::Teeth;
        }
        if let Some(f) = a.face_width {
            s.face = crate::items::FaceSides { preset: FacePreset::AOnly, a: Some(f), b: None };
        } else {
            s.face.preset = FacePreset::AOnly;
        }
        item.gears = vec![s];
        gi.push(item);
        of_item.push(i);
    }
    if err.iter().any(Option::is_some) {
        return;
    }
    // The meshes: as the drawing suggests on parallel shafts, and circles whose shafts meet at
    // one cone distance (bevel) or cross their radii apart (crossed helical).
    let mut links: Vec<ItemLink> = suggest_links(&gi).iter().map(Suggestion::link).collect();
    for a in 0..gi.len() {
        for b in a + 1..gi.len() {
            let (Some(ga), Some(gb)) = (gear_geoms(&gi[a]).first().copied(), gear_geoms(&gi[b]).first().copied()) else { continue };
            let (Some(ra), Some(rb)) = (ga.radius, gb.radius) else { continue };
            if len(cross(ga.axis, gb.axis)) <= 1e-3 {
                continue;
            }
            let t = crate::items::tol(ra.min(rb));
            let touch = match crate::axes::relate(&Axis { origin: ga.centre, dir: ga.axis }, &Axis { origin: gb.centre, dir: gb.axis }) {
                crate::axes::Relation::Intersecting { apex, .. } => {
                    let cone = |c: V, r: f64| (len(sub(c, apex)).powi(2) + r * r).sqrt();
                    (cone(ga.centre, ra) - cone(gb.centre, rb)).abs() <= t
                }
                crate::axes::Relation::Skew { offset, .. } => (offset - ra - rb).abs() <= t,
                _ => false,
            };
            if touch {
                links.push(ItemLink { a: GearRef::new(a, 0), b: GearRef::new(b, 0), kind: LinkKind::Mesh });
            }
        }
    }
    let names: Vec<String> = gi.iter().map(|g| g.who()).collect();
    let back = |text: &str| {
        let mut t = text.to_string();
        for (k, n) in names.iter().enumerate().rev() {
            t = t.replace(&format!("{n} gear 1"), &format!("member {}", of_item[k] + 1));
        }
        t
    };
    match plan_items(&gi, &links, &[], None, base) {
        Err(e) => out.error = Some(back(&e)),
        Ok(sp) if sp.error.is_some() => out.error = sp.error.as_deref().map(back),
        Ok(sp) => {
            out.gears = Vec::new();
            for (r, k) in &sp.member_of {
                let Some(&i) = of_item.get(r.item) else { continue };
                let Some(m) = sp.plan.members.get(*k) else { continue };
                let mem = &mut out.members[i];
                (mem.module, mem.teeth, mem.pitch_diameter, mem.face_width) =
                    (Some(m.module), Some(m.teeth), Some(m.pitch_diameter), Some(m.face_width));
                mem.cluster = m.cluster;
                mem.meshes_with = m
                    .meshes_with
                    .iter()
                    .filter_map(|j| sp.member_of.iter().find(|(_, x)| x == j).and_then(|(g, _)| of_item.get(g.item).copied()))
                    .collect();
                out.gears.push(i);
            }
            out.notes.extend(sp.notes.iter().map(|n| back(n)));
            out.notes.push("stages of several kinds: each planned by its own planner and merged into one plan".into());
            out.planned = Some(sp.plan);
        }
    }
}
