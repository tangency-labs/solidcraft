//! Offset a regularized source without aliasing the untouched outer operand.
//!
//! Private notifications carry the source's attributes through construction. Only
//! the finished cavity is registered in the caller's reserved result namespace.
//! Geometry owners are rebound from their proven private identities. No recursion.
//!
//! Upstream: none (regularized inner offsets).

use std::collections::BTreeMap;

use super::attributes;
use super::p2::{
    configured_shell_executive, OffsetNotifications, ShellOptions, SolidOffsetOptions,
};
use crate::error::{EntityRef, Result};
use crate::geom::attribute::AttributeStore;
use crate::kernel::Kernel;
use crate::topology::brep::p3;
use crate::topology::{Brep, BrepId, FaceId, NamingScheme};

pub(super) fn shell_offset(
    kernel: &mut Kernel,
    source: &mut Brep,
    options: ShellOptions,
    open: &[FaceId],
    caller: &mut OffsetNotifications<'_>,
) -> Result<Brep> {
    let source_owners: BTreeMap<_, _> = attributes::live_entities(source)
        .into_iter()
        .map(|entity| (entity, source.owner_identity(entity)))
        .collect();
    p3::rebind_geometry_owners(
        source,
        |entity| {
            source_owners
                .get(&entity)
                .copied()
                .unwrap_or_else(|| attributes::original_key(entity))
        },
        attributes::original_key,
    )?;
    source.set_naming(NamingScheme::Caller);
    let saved_key = source.key();
    source.set_key(attributes::original_key(EntityRef::Brep(BrepId::from_index(0))).key);
    let mut records = attributes::simple_records(source);
    let mut store = AttributeStore::default();
    let mut private = OffsetNotifications {
        records: &mut records,
        attributes: &mut store,
        original_key_of: &attributes::original_key,
        result_key_of: &attributes::result_key,
        original_curve_ref: &attributes::original_curve,
        result_curve_ref: &attributes::result_curve,
        boolean_source_lists: &attributes::source_lists,
        result_brep: attributes::result_key(EntityRef::Brep(BrepId::from_index(0))),
        merge_identities: Some(&attributes::SimpleIdentities),
    };
    let outcome = (|| {
        attributes::seed_attributes(source, private.original_key_of, private.attributes)?;
        let mut executive = configured_shell_executive(source, options, open);
        executive.use_creator_tangency_tol(kernel);
        executive.do_solid_offset(
            kernel,
            Some(&mut *source),
            SolidOffsetOptions::default(),
            &mut private,
        )
    })();
    source.set_key(saved_key);
    let mut cavity = outcome?;
    attributes::write_back_attributes(&mut cavity, &attributes::result_key, &store)?;
    attributes::record_result_owners(&mut cavity, &attributes::result_key)?;
    let owners: BTreeMap<_, _> = attributes::live_entities(&cavity)
        .into_iter()
        .map(|entity| (entity, cavity.owner_identity(entity)))
        .collect();
    p3::rebind_geometry_owners(
        &mut cavity,
        |entity| {
            owners
                .get(&entity)
                .copied()
                .unwrap_or_else(|| attributes::result_key(entity))
        },
        caller.result_key_of,
    )?;
    cavity.set_naming(NamingScheme::Caller);
    cavity.set_key(caller.result_brep.key);
    p3::seed_records(caller.records, &cavity, caller.result_key_of);
    attributes::seed_attributes(&cavity, caller.result_key_of, caller.attributes)?;
    Ok(cavity)
}
