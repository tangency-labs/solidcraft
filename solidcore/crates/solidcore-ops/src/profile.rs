//! Planar profiles: line families clipped to a region (`solidcore::prism::profile::line_family`).
//! Validate the stored families, convert inputs field by field, and ask the kernel for
//! complete bounded marks. Named patterns and PAT parsing stay in the app.
use solidcore::prism::profile::line_family as kernel;
use solidcore_convert as convert;
use solidcore_record::profile::line_family::{Invalid, LineFamily};
use solidcore_record::{Region2, Seg2, Vec2};

pub use kernel::MAX_FAMILIES;

pub use solidcore_record::profile::line_family::Mark;

/// Clip each family's lines, in turn, to the region, under one work budget, or refuse
/// without partial geometry.
///
/// # Errors
/// Invalid families, more than [`MAX_FAMILIES`], unsupported boundaries, uncertain
/// arithmetic or exhausted budgets.
pub fn clip_line_family(families: &[LineFamily], region: &Region2) -> Result<Vec<Mark>, Invalid> {
    if families.len() > MAX_FAMILIES {
        return Err(Invalid("more than 64 line families"));
    }
    families.iter().try_for_each(LineFamily::validate)?;
    admit_region(region)?;
    let families: Vec<kernel::LineFamily> = families
        .iter()
        .map(|family| kernel::LineFamily {
            angle: family.angle,
            origin: convert::vec2(family.origin),
            delta_x: family.delta_x,
            delta_y: family.delta_y,
            dashes: family.dashes.clone(),
        })
        .collect();
    kernel::clip(&families, &convert::region2(region))
        .map_err(|error| Invalid(error.0))
        .map(|marks| {
            marks
                .into_iter()
                .map(|mark| match mark {
                    kernel::Mark::Line { a, b } => Mark::Line {
                        a: convert::record_vec2(a),
                        b: convert::record_vec2(b),
                    },
                    kernel::Mark::Dot { at } => Mark::Dot {
                        at: convert::record_vec2(at),
                    },
                })
                .collect()
        })
}

// Bound every vector before fieldwise conversion allocates. Seg2's cubic/conic
// controls and Bezier's control array have fixed sizes; there is no nested spline
// allocation. The kernel retains its geometry checks and their error ordering.
fn admit_region(region: &Region2) -> Result<(), Invalid> {
    let circular = matches!(region.outer.segs.first(), Some(Seg2::Arc { .. }));
    let holes = if circular {
        1
    } else {
        kernel::MAX_INPUT_PIECES / 3
    };
    if region.holes.len() > holes {
        return Err(Invalid(if circular {
            "not supported yet: multiple circular hatch holes"
        } else {
            "too many region holes"
        }));
    }
    let mut pieces = 0usize;
    for boundary in region.loops() {
        pieces = pieces
            .checked_add(boundary.segs.len())
            .filter(|n| *n <= kernel::MAX_INPUT_PIECES)
            .ok_or(Invalid("input piece budget exhausted"))?;
    }
    Ok(())
}

/// A dot's filled disk, when its physical pen diameter is representable at its centre.
#[must_use]
pub fn dot_region(at: Vec2, width: f64) -> Option<Region2> {
    kernel::dot_region(convert::vec2(at), width)
        .as_ref()
        .map(convert::record_region2)
}
