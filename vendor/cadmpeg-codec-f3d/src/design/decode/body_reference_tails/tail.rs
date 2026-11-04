// SPDX-License-Identifier: Apache-2.0
//! Frame the independent reference runs after a whole-body operand's UUIDs.

use cadmpeg_core::decode::{u64_from_index, View};

use crate::bytes::take_reference;
use crate::records::identity::Located;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReferenceRunSpan {
    pub(super) count: Located<u32>,
    pub(super) start: u64,
    pub(super) end: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BodyReferenceTail {
    pub(super) form: Located<u32>,
    pub(super) first: ReferenceRunSpan,
    pub(super) second: ReferenceRunSpan,
    pub(super) extra_zero: Option<Located<u32>>,
    pub(super) trailing: Located<u32>,
    pub(super) end: u64,
}

fn word(view: &mut View<'_>) -> Option<Located<u32>> {
    let offset = u64_from_index(view.position());
    let value = view.u32_le()?;
    Some(Located { value, offset })
}

fn reference_run(view: &mut View<'_>) -> Option<ReferenceRunSpan> {
    let count = word(view)?;
    let start = u64_from_index(view.position());
    let bounded = view.counted(u64::from(count.value), 1)?;
    let mut cursor = view.position().checked_sub(view.start())?;
    for _ in 0..bounded.get() {
        take_reference(view.window(), &mut cursor)?;
    }
    view.seek(view.start().checked_add(cursor)?)?;
    Some(ReferenceRunSpan {
        count,
        start,
        end: u64_from_index(view.position()),
    })
}

pub(super) fn parse(mut view: View<'_>, allow_extra_zero: bool) -> Option<BodyReferenceTail> {
    let form = word(&mut view)?;
    let first = reference_run(&mut view)?;
    let second = reference_run(&mut view)?;
    let candidate = word(&mut view)?;
    let (extra_zero, trailing) = if candidate.value == 0 && allow_extra_zero {
        (Some(candidate), word(&mut view)?)
    } else {
        (None, candidate)
    };
    if !matches!(trailing.value, 8 | 9) || extra_zero.is_some() && trailing.value != 9 {
        return None;
    }
    Some(BodyReferenceTail {
        form,
        first,
        second,
        extra_zero,
        trailing,
        end: u64_from_index(view.position()),
    })
}

#[cfg(test)]
mod tests;
