//! Pass layout: a weld body shown as its passes, round beads stacked in the joint, for a weld
//! whose "passes shown" is on ([`weld_passes`]). The layout is saved with the weld as passes per
//! layer, root first ([`crate::weld::WeldSettings::passes`]).
//!
//! SolidCraft's own, as the other welds in this crate are: every body is a kernel call that reads
//! the parts and the weld and never changes them; this module adds the passes' rules. For
//! display only: the weld's own body stays the one the weld table and the BOM measure.
//!
//! # The geometry
//!
//! The weld is read in its frame: the section in the frame's plane, `y` up out of the joint (the
//! way the weld is built up), `x` across it, the weld running along the frame's normal. Its
//! height is split into equal layers; each layer's width (where the weld is, at that layer) into
//! equal cells, one per pass. Each pass is the elliptic cylinder through its cell's corners (the
//! ellipse of semi-axes √2 times the cell's half sizes, about its middle), as long as the weld,
//! kept to the room outside the parts, less the passes laid after it. So every point of the weld
//! lies in a pass, the seams between passes are the later beads' round edges, and the cap passes
//! stand proud of the weld's face as real crowns do.
//!
//! # Not covered
//!
//! Welds along a straight joint only: the weld is taken as running straight along the frame's
//! normal over its whole length. Passes round a curved joint come later.

use std::f64::consts::{SQRT_2, TAU};

use solidcore_record::{Plane, Vec3};
use solidcraft_geom::PlaneGeom;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, BoolOp, KernelError, affine, box_solid, transform_matrix};

/// The most layers a layout may have, and the most passes in one layer.
pub const MAX_LAYERS: usize = 20;
pub const MAX_PASSES_PER_LAYER: u32 = 20;

/// Why a weld's passes are refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PassRefusal {
    /// Input the passes cannot take: a layout out of range, a weld or frame the kernel cannot read.
    #[error("{0}")]
    Invalid(&'static str),
    /// Pass `number` (counted from 1, root first) lies wholly under the passes laid after it.
    #[error("pass {number} lies wholly under the passes laid after it")]
    Covered { number: usize },
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, PassRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(PassRefusal::Invalid(why))
}

/// One pass: its layer (0 the root), its number (1 the root pass, counting layer by layer) and
/// its body, for display only.
#[derive(Debug, Clone)]
pub struct Pass {
    pub layer: usize,
    pub number: usize,
    pub body: Body,
}

/// Whether `layout` (passes per layer, root first) is one the passes take.
///
/// # Errors
///
/// [`PassRefusal::Invalid`] for no layers, more than [`MAX_LAYERS`], or a layer of no passes or
/// more than [`MAX_PASSES_PER_LAYER`].
pub fn check_layout(layout: &[u32]) -> Result<()> {
    if layout.is_empty() || layout.len() > MAX_LAYERS || layout.iter().any(|&m| m == 0 || m > MAX_PASSES_PER_LAYER) {
        return bad("a pass layout of no layers or too many, or a layer of no passes or too many");
    }
    Ok(())
}

/// The passes of the weld `weld` laid out as `layout` (passes per layer, root first), the weld
/// read in `frame` (see the module's notes), kept outside `parts`. The weld and the parts are
/// never changed.
///
/// # Errors
///
/// [`PassRefusal::Invalid`] for a layout out of range ([`check_layout`]), a frame the kernel
/// cannot place, a weld with no extent, a layer the weld does not reach, or parts that leave no
/// room round the weld; [`PassRefusal::Covered`]; the errors of the kernel calls otherwise.
pub fn weld_passes(weld: &Body, frame: &Plane, parts: &[&Body], layout: &[u32]) -> Result<Vec<Pass>> {
    check_layout(layout)?;
    // the frame as a map from its own coordinates into the model, and back
    let n = frame.normal();
    let row = |v: Vec3| [v.x, v.y, v.z, 0.0];
    let to_model = [row(frame.x), row(frame.y), row(n), [frame.origin.x, frame.origin.y, frame.origin.z, 1.0]];
    let to_frame = affine::inverse(&to_model)?;
    let w = transform_matrix(weld, to_frame)?;
    let bb = w.exact_bounds()?;
    let ([x0, y0, z0], [x1, y1, z1]) = ([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z]);
    let (dx, dy) = (x1 - x0, y1 - y0);
    if !(dx > 0.0 && dy > 0.0 && z1 > z0) {
        return bad("a weld with no extent in its frame");
    }
    // the room round the weld: a box well past it, less the parts
    let mut room = box_solid(Vec3::new(x0 - dx, y0 - dy, z0 - 1.0), Vec3::new(x1 + dx, y1 + dy, z1 + 1.0))?;
    for part in parts {
        let part = transform_matrix(part, to_frame)?;
        room = solidcraft_kernel::boolean(&room, &part, BoolOp::Cut)?.ok_or(PassRefusal::Invalid("parts that leave no room round the weld"))?;
    }
    // one round bar along the weld, of radius 1, scaled into each pass's ellipse
    let circle = [Curve::Arc { center: [0.0, 0.0, z0], axis: [0.0, 0.0, 1.0], reference: [1.0, 0.0, 0.0], radius: 1.0, start: 0.0, end: TAU }];
    let bar = profiles::extrude_curves(&circle, [0.0, 0.0, 1.0], z1 - z0, true)?;
    let height = dy / layout.len() as f64;
    let mut beads: Vec<(usize, Body)> = Vec::new();
    for (j, &m) in layout.iter().enumerate() {
        let (ya, yb) = (y0 + j as f64 * height, y0 + (j + 1) as f64 * height);
        let slab = box_solid(Vec3::new(x0 - 1.0, ya, z0 - 1.0), Vec3::new(x1 + 1.0, yb, z1 + 1.0))?;
        let layer = solidcraft_kernel::boolean(&w, &slab, BoolOp::Intersect)?.ok_or(PassRefusal::Invalid("a layer the weld does not reach"))?;
        let lb = layer.exact_bounds()?;
        let width = (lb.max.x - lb.min.x) / f64::from(m);
        for i in 0..m {
            let (hx, hy) = (width / 2.0, height / 2.0);
            let (cx, cy) = (lb.min.x + (f64::from(i) + 0.5) * width, (ya + yb) / 2.0);
            let ellipse = [[SQRT_2 * hx, 0.0, 0.0, 0.0], [0.0, SQRT_2 * hy, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [cx, cy, 0.0, 1.0]];
            let bead = solidcraft_kernel::boolean(&transform_matrix(&bar, ellipse)?, &room, BoolOp::Intersect)?;
            let number = beads.len() + 1;
            beads.push((j, bead.ok_or(PassRefusal::Covered { number })?));
        }
    }
    let mut passes = Vec::with_capacity(beads.len());
    for (k, (layer, bead)) in beads.iter().enumerate() {
        let mut body = bead.clone();
        // a bead reaches only into its own layer's later cells and the next layer's
        for (later_layer, later) in beads.iter().skip(k + 1) {
            if *later_layer > layer + 1 {
                break;
            }
            body = solidcraft_kernel::boolean(&body, later, BoolOp::Cut)?.ok_or(PassRefusal::Covered { number: k + 1 })?;
        }
        passes.push(Pass { layer: *layer, number: k + 1, body: transform_matrix(&body, to_model)? });
    }
    Ok(passes)
}

#[cfg(test)]
#[path = "weld_passes_tests.rs"]
mod tests;
