//! The hole stacks of bodies' records ([`hole_stacks`]): the places a rivet, bolt or pin can
//! pass through two or more touching bodies. The kernel does the work on the records' exact
//! faces ([`solidcore::eval::hole_stacks`]); the records reach it through
//! `solidcore-convert` once, the diameter and slot filters here only compare the numbers the
//! kernel returned, and nothing here computes.

use solidcore::eval::hole_stacks as k;
use solidcore_convert::record_vec3;
use solidcore_record::brep::Brep;
use solidcore_record::Vec3;

use super::query::kernel_record;
use crate::error::Error;

/// Most holes one call answers for; past them the caller narrows with fewer bodies or
/// diameter ranges.
const MAX_HOLES: usize = 100_000;

/// What shapes the stacks: the gaps and offsets that hold, and which bores count.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The largest gap between the layers of one stack, in mm.
    pub touching: f64,
    /// The largest offset between the axes of one line of holes, in mm.
    pub axis_tolerance: f64,
    /// Whether obround slots count.
    pub slots: bool,
    /// Diameter ranges in mm, each `min ≤ max`: only holes and slot widths inside a range
    /// count. Empty takes every size.
    pub diameters: Vec<(f64, f64)>,
}

/// An obround slot's shape across the axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// The overall length, end to end, in mm.
    pub length: f64,
    /// The unit direction of the length.
    pub along: Vec3,
}

/// One layer of a stack: the bores of one body that run together.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// The body, by its index into the records [`hole_stacks`] took.
    pub body: usize,
    /// The layer's narrowest diameter, in mm.
    pub d: f64,
    /// The extent from the stack's start, in mm.
    pub t: [f64; 2],
    /// The narrowest bore's slot shape.
    pub slot: Option<Slot>,
}

/// Bores that line up through two or more touching bodies.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    /// Unit, from the start into the stack.
    pub axis: Vec3,
    /// The point on the axis the first layer starts at.
    pub start: Vec3,
    /// The clamped thickness, in mm.
    pub grip: f64,
    /// The point on the axis the last layer ends at.
    pub end: Vec3,
    /// The stack's layers, from its start along `axis`.
    pub layers: Vec<Layer>,
}

/// A bore in no stack.
#[derive(Clone, Debug, PartialEq)]
pub struct Lone {
    /// The body, by its index into the records [`hole_stacks`] took.
    pub body: usize,
    /// The bore's entry point.
    pub at: Vec3,
    /// Unit axis, from the entry point into the bore.
    pub axis: Vec3,
    /// The bore's diameter, in mm.
    pub d: f64,
    /// The wall's extent, in mm.
    pub depth: f64,
    /// The slot's shape.
    pub slot: Option<Slot>,
}

/// The stacks among bodies' records, and the bores in none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stacks {
    /// The stacks, ordered by their start points.
    pub stacks: Vec<Stack>,
    /// The bores in no stack.
    pub lone: Vec<Lone>,
}

/// The hole stacks among bodies' records: every set of coaxial holes through two or more
/// touching bodies, and the holes in no stack. Each record's bores — its round holes and
/// obround slots — come from the kernel, read from the exact faces of the record, with the
/// wall extents a countersink or chamfer extends; a bore outside every range in `diameters`,
/// or a slot with `slots: false`, counts as not there.
///
/// # Errors
/// [`Error::Argument`] when the records hold more than [`MAX_HOLES`] holes between them, and
/// the kernel's refusal when it meets too many nearly-coaxial holes to compare: narrow the
/// call with fewer bodies or diameter ranges.
pub fn hole_stacks(records: &[&Brep], o: &Options) -> crate::Result<Stacks> {
    let mut bodies: Vec<Vec<k::Bore>> = Vec::new();
    let mut count = 0usize;
    for r in records {
        let mut bores = Vec::new();
        for b in k::bores(&kernel_record(r)).map_err(crate::error::status)? {
            if !o.slots && b.slot.is_some() {
                continue;
            }
            if !o.diameters.is_empty()
                && !o.diameters.iter().any(|&(lo, hi)| b.d >= lo && b.d <= hi)
            {
                continue;
            }
            bores.push(b);
        }
        count += bores.len();
        if count > MAX_HOLES {
            return Err(Error::Refused {
                operation: "hole_stacks",
                reason: "more than 100000 holes: pass fewer bodies or diameter ranges",
            });
        }
        bodies.push(bores);
    }
    let slices: Vec<&[k::Bore]> = bodies.iter().map(Vec::as_slice).collect();
    let out = k::stacks(
        &slices,
        &k::Options {
            touching: o.touching,
            axis_tolerance: o.axis_tolerance,
        },
    )
    .map_err(crate::error::status)?;
    let lone = out
        .lone
        .iter()
        .map(|h| Lone {
            body: h.body,
            at: record_vec3(h.at),
            axis: record_vec3(h.axis),
            d: h.d,
            depth: h.depth,
            slot: h.slot.map(slot),
        })
        .collect();
    Ok(Stacks {
        stacks: out.stacks.iter().map(|s| one_stack(s, &bodies)).collect(),
        lone,
    })
}

/// One kernel stack as this module's, with its layers' slot shapes mapped.
fn one_stack(s: &k::Stack, bodies: &[Vec<k::Bore>]) -> Stack {
    Stack {
        axis: record_vec3(s.axis),
        start: record_vec3(s.start),
        grip: s.grip,
        end: record_vec3(s.end),
        layers: s
            .layers
            .iter()
            .map(|l| Layer {
                body: l.body,
                d: l.d,
                t: l.t,
                slot: bodies
                    .get(l.body)
                    .and_then(|bs| bs.get(l.bore))
                    .and_then(|b| b.slot)
                    .map(slot),
            })
            .collect(),
    }
}

/// A kernel slot's shape as this module's.
fn slot(s: k::Slot) -> Slot {
    Slot {
        length: s.length,
        along: record_vec3(s.along),
    }
}
