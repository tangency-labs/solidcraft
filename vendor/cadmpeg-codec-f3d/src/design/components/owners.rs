// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_ir::transform::Transform;

use super::LocalComponentSources;

pub(crate) fn placement(
    sources: &LocalComponentSources<'_>,
    stream: &str,
    mut owner: u64,
) -> Option<Transform> {
    let components = sources.components.get(stream)?;
    let root = components.roots.get(&components.root_component?)?;
    let mut visited = BTreeSet::new();
    let mut world = Transform::identity();
    loop {
        if !visited.insert(owner)
            || components
                .roots
                .values()
                .filter(|definition| definition.root_entity == owner)
                .count()
                != 1
        {
            return None;
        }
        if owner == root.root_entity {
            return Some(world);
        }
        let mut carriers = sources.occurrences.iter().filter(|carrier| {
            crate::ids::native_stream(&carrier.id) == Some(stream)
                && components
                    .roots
                    .get(&carrier.component_record_index)
                    .is_some_and(|definition| definition.root_entity == owner)
        });
        let carrier = carriers.next()?;
        if carriers.next().is_some()
            || sources
                .occurrences
                .iter()
                .filter(|other| {
                    crate::ids::native_stream(&other.id) == Some(stream)
                        && other
                            .occurrence_guid
                            .as_str()
                            .eq_ignore_ascii_case(carrier.occurrence_guid.as_str())
                })
                .count()
                != 1
        {
            return None;
        }
        let local = super::neutral_transform(
            carrier
                .transform()
                .map_or(super::IDENTITY, |frame| frame.value.into()),
        )
        .ok()?;
        world = local.compose(world).ok()?;
        owner = carrier.owner_entity;
    }
}
