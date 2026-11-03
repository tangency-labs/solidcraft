//! A bounded region's shells with the one that encloses the others first.
//!
//! Not upstream in SMLib: the C API's `PutEnclosingShellsFirst` (`smlib_flat.cpp`, after
//! `SmuConvert`), moved here from SolidCraft's flat-array import, as it is, for every caller that
//! builds a body from data (docs/layers.md §1).
//!
//! SMLib takes the first shell of every region but the infinite one as the shell that encloses
//! the region, the others lying inside it (`SmBrep::AssertValid` test 12). Data may list a
//! region's shells in any order, so the one that encloses all the others is put first, decided
//! by `SmShell::IsInnerShellOf` ([`is_inner_shell_of`]). Candidates are the closed face-use
//! shells whose bounding box can be computed, largest box first (a stable sort: equal boxes keep
//! the data's order). Only `SM_TRUE` counts as inside: `SM_FALSE` and `SM_UNSURE` rule the
//! candidate out, as the C API's `== TRUE`, and a region where no candidate is shown to enclose
//! the others keeps its order. An error with no upstream counterpart (an unported call) refuses,
//! as does a body needing more than [`MAX_CONTAINMENT_TESTS`] containment tests.

use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::query::face::p1::calculate_shell_bounding_box;
use crate::topology::brep::p1::regions;
use crate::topology::shell::{is_inner_shell_of, SM_TRUE};
use crate::topology::{Brep, ShellId};

const OP: &str = "topology::brep::enclosing::put_enclosing_shells_first";

/// The most `SmShell::IsInnerShellOf` calls one body takes; a body that would need more is
/// refused instead of ray casting without end.
pub const MAX_CONTAINMENT_TESTS: usize = 1 << 16;

/// Puts each bounded region's enclosing shell first in its list.
///
/// # Errors
///
/// `InvalidInput` past [`MAX_CONTAINMENT_TESTS`]; an unported bounding box; those of the
/// topology walk and of [`is_inner_shell_of`].
pub fn put_enclosing_shells_first(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    let infinite = b.infinite_region();
    let mut tests = 0usize;
    for r in regions(b, None)? {
        if Some(r) == infinite {
            continue;
        }
        let shells = b.region(r)?.shells.clone();
        if shells.len() < 2 {
            continue;
        }
        let mut candidates: Vec<(f64, ShellId)> = Vec::new();
        for &s in &shells {
            let rec = b.shell(s)?;
            if !rec.is_faceuse_shell() || !rec.is_closed(b, s)? {
                continue;
            }
            match calculate_shell_bounding_box(k, b, s, false) {
                Ok(bx) => {
                    let size = bx.size();
                    candidates.push((size.x * size.y * size.z, s));
                }
                Err(e) if e.is_unported() => return Err(e),
                // != SM_SUCCESS: not a candidate
                Err(_) => {}
            }
        }
        // stable: equal boxes keep the data's order
        candidates.sort_by(|a, c| c.0.partial_cmp(&a.0).unwrap_or(core::cmp::Ordering::Equal));
        for &(_, candidate) in &candidates {
            let mut encloses = true;
            for &s in &shells {
                if s == candidate {
                    continue;
                }
                tests += 1;
                if tests > MAX_CONTAINMENT_TESTS {
                    return Err(SolidError::invalid_input(
                        OP,
                        "too many shells to order (SmShell::IsInnerShellOf)",
                    ));
                }
                if is_inner_shell_of(k, b, s, Some(candidate))? != SM_TRUE {
                    encloses = false;
                    break;
                }
            }
            if encloses {
                // SmRegion::SetList(candidate): the circular list now starts at it
                let list = &mut b.region_mut(r)?.shells;
                if let Some(at) = list.iter().position(|&s| s == candidate) {
                    let (Some(tail), Some(head)) = (list.get(at..), list.get(..at)) else {
                        continue;
                    };
                    *list = tail.iter().chain(head).copied().collect();
                }
                break;
            }
        }
    }
    Ok(())
}
