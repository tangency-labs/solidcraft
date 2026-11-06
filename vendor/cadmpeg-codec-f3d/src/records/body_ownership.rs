// SPDX-License-Identifier: Apache-2.0
//! Component membership stored at the end of a Design Body container.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::bytes::take_reference;
use crate::design::presentation::BODY_PRESENTATION_TYPE_GUID;

pub(crate) const CONTAINER_GUID: &str = "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA";
pub(crate) const CONTAINER_BASE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
pub(crate) const COMPONENT_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
const TYPED_REFERENCE_BYTES: usize = 51;
const LOCAL_REFERENCE_BYTES: usize = 11;
const COUNT_BYTES: usize = std::mem::size_of::<u32>();

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BodyOwnershipReferences {
    InlineTyped {
        body: NativeBytes<[u8; TYPED_REFERENCE_BYTES]>,
        owner: NativeBytes<[u8; TYPED_REFERENCE_BYTES]>,
    },
    LocalSingle {
        body: NativeBytes<[u8; LOCAL_REFERENCE_BYTES]>,
        owner: NativeBytes<[u8; LOCAL_REFERENCE_BYTES]>,
    },
    Local {
        body: NativeBytes<[u8; LOCAL_REFERENCE_BYTES]>,
        repeated_count: u32,
        repeated_body: NativeBytes<[u8; LOCAL_REFERENCE_BYTES]>,
        body_word: u32,
        owner: NativeBytes<[u8; LOCAL_REFERENCE_BYTES]>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BodyOwnership {
    pub(crate) container_record_index: u64,
    pub(crate) byte_offset: u64,
    pub(crate) byte_end: u64,
    pub(crate) body_count: u32,
    pub(crate) body_ordinal: u32,
    pub(crate) references: BodyOwnershipReferences,
}

#[derive(Clone, Copy)]
enum Form {
    InlineTyped,
    LocalSingle,
    Local,
}

impl Form {
    fn reference_bytes(self) -> usize {
        match self {
            Self::InlineTyped => TYPED_REFERENCE_BYTES,
            Self::LocalSingle | Self::Local => LOCAL_REFERENCE_BYTES,
        }
    }
    fn entry_stride(self) -> usize {
        self.reference_bytes()
            + match self {
                Self::InlineTyped | Self::LocalSingle => 0,
                Self::Local => COUNT_BYTES,
            }
    }
    fn overhead(self) -> usize {
        self.reference_bytes()
            + COUNT_BYTES
                * match self {
                    Self::InlineTyped | Self::LocalSingle => 1,
                    Self::Local => 2,
                }
    }
    fn body_stride(self) -> usize {
        match self {
            Self::InlineTyped | Self::LocalSingle => self.reference_bytes(),
            Self::Local => LOCAL_REFERENCE_BYTES + self.entry_stride(),
        }
    }
}

impl BodyOwnership {
    pub(crate) fn component_for(&self, body: u64) -> Option<u64> {
        if self.body_ordinal >= self.body_count {
            return None;
        }
        let (form, body_reference, owner_reference) = match &self.references {
            BodyOwnershipReferences::InlineTyped { body, owner } => {
                (Form::InlineTyped, &body[..], &owner[..])
            }
            BodyOwnershipReferences::LocalSingle { body, owner } => {
                (Form::LocalSingle, &body[..], &owner[..])
            }
            BodyOwnershipReferences::Local {
                body,
                repeated_count,
                repeated_body,
                body_word: 1,
                owner,
            } if *repeated_count == self.body_count && body == repeated_body => {
                (Form::Local, &body[..], &owner[..])
            }
            BodyOwnershipReferences::Local { .. } => return None,
        };
        let extent = usize::try_from(self.byte_end.checked_sub(self.byte_offset)?).ok()?;
        let body_bytes = extent.checked_sub(form.overhead())?;
        let count = cadmpeg_core::decode::bounded_len(
            u64::from(self.body_count),
            form.body_stride(),
            body_bytes,
        )?;
        if body_bytes != count.checked_mul(form.body_stride())? {
            return None;
        }
        let mut at = 0;
        if local_reference(body_reference, &mut at, BODY_PRESENTATION_TYPE_GUID, form)? != body {
            return None;
        }
        let mut at = 0;
        local_reference(owner_reference, &mut at, COMPONENT_GUID, form)
    }
}

fn local_reference(bytes: &[u8], at: &mut usize, guid: &str, form: Form) -> Option<u64> {
    let before = *at;
    let reference = take_reference(bytes, at)?;
    let (target, actual) = reference.local()?;
    let correct_type = match (form, actual) {
        (Form::InlineTyped, Some(actual)) => actual.eq_ignore_ascii_case(guid),
        (Form::Local | Form::LocalSingle, None) => true,
        _ => false,
    };
    (target != 0 && correct_type && *at - before == form.reference_bytes()).then_some(target)
}

pub(crate) struct Membership<'a> {
    bytes: &'a [u8],
    pub(crate) component: u64,
    count: usize,
    form: Form,
    repeated: BTreeMap<u64, usize>,
}

impl Membership<'_> {
    pub(crate) fn proof(
        &self,
        container: u64,
        byte_offset: u64,
        ordinal: usize,
    ) -> Option<BodyOwnership> {
        if ordinal >= self.count {
            return None;
        }
        let start = COUNT_BYTES.checked_add(ordinal.checked_mul(self.form.reference_bytes())?)?;
        let first_end =
            COUNT_BYTES.checked_add(self.count.checked_mul(self.form.reference_bytes())?)?;
        let references = match self.form {
            Form::InlineTyped => BodyOwnershipReferences::InlineTyped {
                body: NativeBytes::new(
                    self.bytes
                        .get(start..start + TYPED_REFERENCE_BYTES)?
                        .try_into()
                        .ok()?,
                ),
                owner: NativeBytes::new(
                    self.bytes
                        .get(first_end..first_end + TYPED_REFERENCE_BYTES)?
                        .try_into()
                        .ok()?,
                ),
            },
            Form::LocalSingle => BodyOwnershipReferences::LocalSingle {
                body: NativeBytes::new(
                    self.bytes
                        .get(start..start + LOCAL_REFERENCE_BYTES)?
                        .try_into()
                        .ok()?,
                ),
                owner: NativeBytes::new(
                    self.bytes
                        .get(first_end..first_end + LOCAL_REFERENCE_BYTES)?
                        .try_into()
                        .ok()?,
                ),
            },
            Form::Local => {
                let mut body_at = start;
                let body = local_reference(
                    self.bytes,
                    &mut body_at,
                    BODY_PRESENTATION_TYPE_GUID,
                    self.form,
                )?;
                let repeated = *self.repeated.get(&body)?;
                let owner = first_end
                    .checked_add(COUNT_BYTES)?
                    .checked_add(self.count.checked_mul(self.form.entry_stride())?)?;
                BodyOwnershipReferences::Local {
                    body: NativeBytes::new(
                        self.bytes
                            .get(start..start + LOCAL_REFERENCE_BYTES)?
                            .try_into()
                            .ok()?,
                    ),
                    repeated_count: View::u32_le_at(self.bytes, first_end)?,
                    repeated_body: NativeBytes::new(
                        self.bytes
                            .get(repeated..repeated + LOCAL_REFERENCE_BYTES)?
                            .try_into()
                            .ok()?,
                    ),
                    body_word: View::u32_le_at(self.bytes, repeated + LOCAL_REFERENCE_BYTES)?,
                    owner: NativeBytes::new(
                        self.bytes
                            .get(owner..owner + LOCAL_REFERENCE_BYTES)?
                            .try_into()
                            .ok()?,
                    ),
                }
            }
        };
        Some(BodyOwnership {
            container_record_index: container,
            byte_offset,
            byte_end: byte_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(self.bytes.len()))?,
            body_count: u32::try_from(self.count).ok()?,
            body_ordinal: u32::try_from(ordinal).ok()?,
            references,
        })
    }
    pub(crate) fn bodies(&self) -> impl Iterator<Item = u64> + '_ {
        self.bytes
            .get(COUNT_BYTES..)
            .into_iter()
            .flat_map(|bytes| bytes.chunks_exact(self.form.reference_bytes()))
            .take(self.count)
            .filter_map(|bytes| {
                let mut at = 0;
                local_reference(bytes, &mut at, BODY_PRESENTATION_TYPE_GUID, self.form)
            })
    }
}

fn membership_form(bytes: &[u8], form: Form) -> Option<Membership<'_>> {
    let raw_count = View::over_retained(bytes).u32_le()?;
    let body_bytes = bytes.len().checked_sub(form.overhead())?;
    let count =
        cadmpeg_core::decode::bounded_len(u64::from(raw_count), form.body_stride(), body_bytes)?;
    if body_bytes != count.checked_mul(form.body_stride())? {
        return None;
    }
    let mut at = COUNT_BYTES;
    let mut bodies = BTreeSet::new();
    for _ in 0..count {
        let body = local_reference(bytes, &mut at, BODY_PRESENTATION_TYPE_GUID, form)?;
        if !matches!(form, Form::InlineTyped) && !bodies.insert(body) {
            return None;
        }
    }
    let mut repeated = BTreeMap::new();
    if matches!(form, Form::Local) {
        if View::u32_le_at(bytes, at)? != raw_count {
            return None;
        }
        at += COUNT_BYTES;
        for _ in 0..count {
            let reference_at = at;
            let body = local_reference(bytes, &mut at, BODY_PRESENTATION_TYPE_GUID, form)?;
            if !bodies.contains(&body)
                || repeated.insert(body, reference_at).is_some()
                || View::u32_le_at(bytes, at)? != 1
            {
                return None;
            }
            at += COUNT_BYTES;
        }
    }
    let component = local_reference(bytes, &mut at, COMPONENT_GUID, form)?;
    (at == bytes.len()).then_some(Membership {
        bytes,
        component,
        count,
        form,
        repeated,
    })
}

pub(crate) fn membership(bytes: &[u8]) -> Option<Membership<'_>> {
    membership_form(bytes, Form::InlineTyped)
        .or_else(|| membership_form(bytes, Form::Local))
        .or_else(|| membership_form(bytes, Form::LocalSingle))
}

fn terminal_inline_membership(bytes: &[u8]) -> Option<usize> {
    let owner_at = bytes.len().checked_sub(TYPED_REFERENCE_BYTES)?;
    let mut at = owner_at;
    local_reference(bytes, &mut at, COMPONENT_GUID, Form::InlineTyped)?;
    let mut first = owner_at;
    while let Some(previous) = first.checked_sub(TYPED_REFERENCE_BYTES) {
        let mut cursor = previous;
        if local_reference(
            bytes,
            &mut cursor,
            BODY_PRESENTATION_TYPE_GUID,
            Form::InlineTyped,
        )
        .is_none()
            || cursor != first
        {
            break;
        }
        first = previous;
    }
    let start = first.checked_sub(COUNT_BYTES)?;
    membership_form(bytes.get(start..)?, Form::InlineTyped).map(|_| start)
}

fn terminal_local_membership(bytes: &[u8]) -> Option<usize> {
    let owner_at = bytes.len().checked_sub(LOCAL_REFERENCE_BYTES)?;
    let mut at = owner_at;
    local_reference(bytes, &mut at, COMPONENT_GUID, Form::Local)?;
    let mut first = owner_at;
    let mut found = None;
    loop {
        if let Some(repeated_count_at) = first.checked_sub(COUNT_BYTES) {
            if let Some(count) = View::u32_le_at(bytes, repeated_count_at).and_then(|count| {
                cadmpeg_core::decode::bounded_len(
                    u64::from(count),
                    LOCAL_REFERENCE_BYTES,
                    repeated_count_at,
                )
            }) {
                if let Some(start) = count
                    .checked_mul(LOCAL_REFERENCE_BYTES)
                    .and_then(|span| span.checked_add(COUNT_BYTES))
                    .and_then(|span| repeated_count_at.checked_sub(span))
                {
                    if membership_form(bytes.get(start..)?, Form::Local).is_some()
                        && found.replace(start).is_some()
                    {
                        return None;
                    }
                }
            }
        }
        let Some(previous) = first.checked_sub(Form::Local.entry_stride()) else {
            break;
        };
        let mut cursor = previous;
        if local_reference(bytes, &mut cursor, BODY_PRESENTATION_TYPE_GUID, Form::Local).is_none()
            || View::u32_le_at(bytes, cursor) != Some(1)
        {
            break;
        }
        first = previous;
    }
    found
}

fn terminal_single_membership(bytes: &[u8]) -> Option<usize> {
    let owner_at = bytes.len().checked_sub(LOCAL_REFERENCE_BYTES)?;
    let mut at = owner_at;
    local_reference(bytes, &mut at, COMPONENT_GUID, Form::LocalSingle)?;
    let mut first = owner_at;
    while let Some(previous) = first.checked_sub(LOCAL_REFERENCE_BYTES) {
        let mut cursor = previous;
        if local_reference(
            bytes,
            &mut cursor,
            BODY_PRESENTATION_TYPE_GUID,
            Form::LocalSingle,
        )
        .is_none()
        {
            break;
        }
        first = previous;
    }
    let start = first.checked_sub(COUNT_BYTES)?;
    membership_form(bytes.get(start..)?, Form::LocalSingle).map(|_| start)
}

pub(crate) fn terminal_membership(bytes: &[u8]) -> Option<usize> {
    if let Some(start) = terminal_inline_membership(bytes) {
        return Some(start);
    }
    let single = terminal_single_membership(bytes);
    if single.is_some_and(|start| View::u32_le_at(bytes, start).is_some_and(|count| count != 0)) {
        return single;
    }
    terminal_local_membership(bytes).or(single)
}

#[cfg(test)]
mod tests;
