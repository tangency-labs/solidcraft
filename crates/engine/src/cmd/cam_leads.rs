//! The leads, entries, linking, tabs and finishing passes of `cam.job`'s `contour` and `pocket`
//! operations: the job's keys read into the CAM crate's options, and the check that a link at
//! the retract height clears the part (by dropping the cutter onto it along the link).

use solidcore_record::Vec2;
use solidcraft_cam::{ContourOptions, Entry, Lead, Linking, PocketOptions, Surface, TabLayout, Tabs, Tool, drop_z};
use solidcraft_geom::prelude::*;

use super::Ctx;
use crate::Result;
use crate::schema::cam::{ContourOp, ENTRY_DEG, EntrySpec, LeadSpec, LinkingSpec, PocketOp, TabsSpec};

/// Default steepness of the ramps over a tab, degrees.
pub(super) const TAB_RAMP_DEG: f64 = 30.0;
/// Default lead sweep, degrees.
const LEAD_SWEEP_DEG: f64 = 90.0;
/// Default helix radius, as a share of the tool's radius.
const HELIX_SHARE: f64 = 0.9;
/// Default finishing stepover, as a share of the tool's diameter.
pub(super) const FINISH_SHARE: f64 = 0.05;
/// How far the part must stay below a link at the retract height, mm.
const LINK_MARGIN: f64 = 1.0;
/// Most points a link is checked at; a longer link goes up to the clearance height.
const MAX_LINK_SAMPLES: f64 = 4000.0;

fn entry(ctx: &Ctx<'_>, op: &str, e: EntrySpec, tool: &Tool) -> Result<Entry> {
    Ok(match e {
        EntrySpec::Plunge => Entry::Plunge,
        EntrySpec::Ramp { angle_deg } => Entry::Ramp { angle: ctx.degrees(op, angle_deg.unwrap_or(ENTRY_DEG), "`entry.angle_deg`")? },
        EntrySpec::Helix { radius, angle_deg } => Entry::Helix {
            radius: radius.unwrap_or(tool.radius() * HELIX_SHARE),
            angle: ctx.degrees(op, angle_deg.unwrap_or(ENTRY_DEG), "`entry.angle_deg`")?,
        },
    })
}

fn linking(l: Option<LinkingSpec>) -> Linking {
    match l {
        None | Some(LinkingSpec::Clearance) => Linking::Clearance,
        Some(LinkingSpec::StayDown) => Linking::StayDown,
    }
}

fn lead(ctx: &Ctx<'_>, op: &str, l: LeadSpec) -> Result<Lead> {
    Ok(Lead { radius: l.radius, sweep: ctx.degrees(op, l.sweep_deg.unwrap_or(LEAD_SWEEP_DEG), "`lead.sweep_deg`")?, line: l.line.unwrap_or(0.0) })
}

fn tabs(ctx: &Ctx<'_>, op: &str, t: TabsSpec) -> Result<Tabs> {
    let layout = match (t.count, t.spacing) {
        (Some(n), None) => TabLayout::Count(n as usize),
        (None, Some(s)) => TabLayout::Spacing(s),
        (Some(_), Some(_)) => return Err(ctx.bad(op, "give `tabs.count` or `tabs.spacing`, not both")),
        (None, None) => return Err(ctx.bad(op, "give `tabs.count` or `tabs.spacing`")),
    };
    Ok(Tabs {
        layout,
        width: t.width,
        height: t.height,
        ramp_angle: ctx.degrees(op, t.ramp_angle_deg.unwrap_or(TAB_RAMP_DEG), "`tabs.ramp_angle_deg`")?,
    })
}

/// A contour's options from its job keys; the CAM crate checks their values.
pub(super) fn contour_options(ctx: &Ctx<'_>, op: &str, o: &ContourOp, tool: &Tool) -> Result<ContourOptions> {
    Ok(ContourOptions {
        lead: o.lead.map(|l| lead(ctx, op, l)).transpose()?,
        entry: o.entry.map(|e| entry(ctx, op, e, tool)).transpose()?,
        tabs: o.tabs.map(|t| tabs(ctx, op, t)).transpose()?,
        finish_passes: o.finish_passes.unwrap_or(0) as usize,
        finish_stepover: o.finish_stepover.unwrap_or(tool.diameter * FINISH_SHARE),
        spring_pass: o.spring_pass.unwrap_or(false),
        linking: linking(o.linking),
        vertical_lead: o.vertical_lead,
    })
}

/// A pocket's options from its job keys. Without `entry` it goes in as its `ramp_angle_deg`
/// says (0: a plunge).
pub(super) fn pocket_options(ctx: &Ctx<'_>, op: &str, o: &PocketOp, tool: &Tool) -> Result<PocketOptions> {
    let entry = o.entry.map(|e| entry(ctx, op, e, tool)).transpose()?;
    Ok(PocketOptions { entry, linking: linking(o.linking) })
}

/// Whether a rapid from `a` to `b` with the tool's tip at `z` clears the part (`surface`, in the
/// operation's frame) by [`LINK_MARGIN`]: the cutter dropped onto the part every half radius
/// along the link stops that far below it.
pub(super) fn clear_above<'a>(surface: &'a Surface, tool: &Tool) -> impl Fn(Vec2, Vec2, f64) -> bool + 'a {
    let cutter = tool.cutter().ok();
    let step = (tool.radius() * 0.5).max(0.05);
    move |a, b, z| {
        let Some(cutter) = &cutter else { return false };
        let n = (a.dist(b) / step).ceil();
        if !(n <= MAX_LINK_SAMPLES) || !z.is_finite() {
            return false;
        }
        let n = (n as usize).max(1);
        (0..=n).all(|i| {
            let p = a.lerp(b, i as f64 / n as f64);
            drop_z(surface, cutter, p.x, p.y).is_none_or(|top| top <= z - LINK_MARGIN)
        })
    }
}
