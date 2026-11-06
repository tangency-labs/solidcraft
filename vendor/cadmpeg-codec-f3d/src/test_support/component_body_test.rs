// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::{indexed_header, lp_ascii};

pub(crate) fn append_body_owners(
    bulk: &mut Vec<u8>,
    records: &mut Vec<(u64, u64)>,
    owners: &[u64],
) -> Vec<u64> {
    records.push((350, u64_from_index(bulk.len())));
    indexed_header(bulk, *b"261", 350);
    let mut containers = Vec::new();
    for (ordinal, owner) in owners.iter().enumerate() {
        let record = 501 + u32::try_from(ordinal).unwrap();
        containers.push(u64::from(record));
        records.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(bulk, *b"260", record);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        for (target, guid) in [
            (
                350_u64,
                crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            ),
            (*owner, crate::records::body_ownership::COMPONENT_GUID),
        ] {
            bulk.push(1);
            bulk.extend_from_slice(&target.to_le_bytes());
            lp_ascii(bulk, guid);
            bulk.extend_from_slice(&[0, 0]);
        }
    }
    containers
}

type TypeRegistration<'a> = (&'a str, &'a str, u32, &'a str, &'a [u64]);

pub(crate) fn container_type(containers: &[u64]) -> TypeRegistration<'_> {
    (
        crate::records::body_ownership::CONTAINER_GUID,
        crate::records::body_ownership::CONTAINER_BASE_GUID,
        4,
        "Body",
        containers,
    )
}

pub(crate) fn body_type() -> TypeRegistration<'static> {
    (
        crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
        crate::design::presentation::BODY_PRESENTATION_BASE_TYPE_GUID,
        15,
        "Body",
        &[350],
    )
}

/// Body-container record of [`history_only_design`].
#[derive(Clone, Copy)]
pub(crate) enum HistoryOnlyContainer<'a> {
    /// No registered Body container.
    Absent,
    /// A version-4 container that lists these Body entities.
    Lists(&'a [u64]),
    /// A registered version-4 container whose frame has no membership.
    Malformed,
    /// A container that lists these entities, registered at another version.
    OtherVersion(&'a [u64]),
    /// An admitted empty list and a second registered container with no membership.
    ListsAndMalformed,
}

/// Design with one body-map pair `(0, 350)` naming `blob`, an optional browser
/// node for entity 350, and one Body-container form.
pub(crate) fn history_only_design(
    blob: &str,
    hidden: Option<bool>,
    container: HistoryOnlyContainer<'_>,
) -> (Vec<u8>, Vec<u8>) {
    history_only_pairs_design(blob, &[(0, 350, hidden)], container)
}

/// Design with one body map naming `blob` and holding one pair per
/// `(selector, entity, hidden flag)` row. A row with no flag has no browser node.
pub(crate) fn history_only_pairs_design(
    blob: &str,
    pairs: &[(u64, u64, Option<bool>)],
    container: HistoryOnlyContainer<'_>,
) -> (Vec<u8>, Vec<u8>) {
    history_only_pairs_with_cluster(blob, pairs, container, false)
}

pub(crate) fn history_only_pairs_with_cluster(
    blob: &str,
    pairs: &[(u64, u64, Option<bool>)],
    container: HistoryOnlyContainer<'_>,
    bound_cluster: bool,
) -> (Vec<u8>, Vec<u8>) {
    history_only_pairs_with_consumer(blob, pairs, container, bound_cluster, None)
}

pub(crate) const BODY_CONSUMER_SCOPE: u32 = 710;

/// A scope that owns one whole-body selection of one browser node.
#[derive(Clone, Copy)]
pub(crate) struct BodyConsumer<'a> {
    pub(crate) scope_type_guid: &'a str,
    pub(crate) entity: u64,
    pub(crate) hidden: u8,
    pub(crate) flag: u8,
    /// Record that the selection's closing reference names.
    pub(crate) selection_owner: Option<u32>,
    /// The selected record is registered as a browser node.
    pub(crate) registered_node: bool,
}

pub(crate) fn history_only_pairs_with_consumer(
    blob: &str,
    pairs: &[(u64, u64, Option<bool>)],
    container: HistoryOnlyContainer<'_>,
    bound_cluster: bool,
    consumer: Option<BodyConsumer<'_>>,
) -> (Vec<u8>, Vec<u8>) {
    const COMPONENT: u64 = 40;
    const NODE: u32 = 700;
    const SCOPE: u32 = BODY_CONSUMER_SCOPE;
    const SELECTION: u32 = 720;
    fn local(bulk: &mut Vec<u8>, target: u64) {
        bulk.push(1);
        bulk.extend_from_slice(&target.to_le_bytes());
        bulk.extend_from_slice(&[0, 0]);
    }
    let mut bulk = Vec::new();
    let mut records = Vec::new();
    records.push((900, u64_from_index(bulk.len())));
    indexed_header(&mut bulk, *b"256", 900);
    bulk.extend_from_slice(&[0; 10]);
    bulk.extend_from_slice(&u32::try_from(pairs.len()).unwrap().to_le_bytes());
    for (selector, entity, _) in pairs {
        bulk.extend_from_slice(&selector.to_le_bytes());
        bulk.extend_from_slice(&entity.to_le_bytes());
    }
    bulk.extend_from_slice(&1793_u64.to_le_bytes());
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    super::lp_utf16(&mut bulk, blob);
    let mut nodes = Vec::new();
    for (ordinal, (_, entity, hidden)) in (0_u32..).zip(pairs) {
        let Some(hidden) = hidden else {
            continue;
        };
        let record = 100 + ordinal;
        nodes.push(u64::from(record));
        records.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"257", record);
        bulk.extend_from_slice(&[0; 10]);
        super::lp_utf16(&mut bulk, &format!("11111111-2222-8333-A444-{record:012}"));
        bulk.push(u8::from(*hidden));
        bulk.extend_from_slice(&[1, 1]);
        bulk.extend_from_slice(&entity.to_le_bytes());
    }
    let mut component_entities = vec![COMPONENT];
    if let Some(consumer) = consumer {
        let class_tag = if consumer.registered_node {
            nodes.push(u64::from(NODE));
            *b"257"
        } else {
            component_entities.push(u64::from(NODE));
            *b"259"
        };
        records.push((u64::from(NODE), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, class_tag, NODE);
        bulk.extend_from_slice(&[0; 10]);
        super::lp_utf16(&mut bulk, "11111111-2222-8333-A444-000000000700");
        bulk.extend_from_slice(&[consumer.hidden, consumer.flag]);
        local(&mut bulk, consumer.entity);
        local(&mut bulk, u64::from(SCOPE));
    }
    let entities = pairs
        .iter()
        .map(|(_, entity, _)| *entity)
        .collect::<Vec<_>>();
    for entity in &entities {
        records.push((*entity, u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"258", u32::try_from(*entity).unwrap());
    }
    let mut frame = |bulk: &mut Vec<u8>, class_tag: [u8; 3], record: u32, tail: Option<&[u64]>| {
        records.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(bulk, class_tag, record);
        let Some(bodies) = tail else {
            bulk.extend_from_slice(&[7; 9]);
            return;
        };
        bulk.extend_from_slice(&u32::try_from(bodies.len()).unwrap().to_le_bytes());
        let references = bodies
            .iter()
            .map(|body| {
                (
                    *body,
                    crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                )
            })
            .chain([(COMPONENT, crate::records::body_ownership::COMPONENT_GUID)]);
        for (target, guid) in references {
            bulk.push(1);
            bulk.extend_from_slice(&target.to_le_bytes());
            lp_ascii(bulk, guid);
            bulk.extend_from_slice(&[0, 0]);
        }
    };
    let mut containers = Vec::new();
    let mut other_version = Vec::new();
    match container {
        HistoryOnlyContainer::Absent => {}
        HistoryOnlyContainer::Lists(bodies) => {
            containers.push(501);
            frame(&mut bulk, *b"260", 501, Some(bodies));
        }
        HistoryOnlyContainer::Malformed => {
            containers.push(501);
            frame(&mut bulk, *b"260", 501, None);
        }
        HistoryOnlyContainer::OtherVersion(bodies) => {
            other_version.push(501);
            frame(&mut bulk, *b"261", 501, Some(bodies));
        }
        HistoryOnlyContainer::ListsAndMalformed => {
            containers.extend([501, 502]);
            frame(&mut bulk, *b"260", 501, Some(&[]));
            frame(&mut bulk, *b"260", 502, None);
        }
    }
    let mut types = vec![
        (
            crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
            crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
            crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
            "Body",
            &[900][..],
        ),
        (
            crate::design::presentation::BROWSER_NODE_TYPE_GUID,
            crate::design::presentation::BROWSER_NODE_BASE_TYPE_GUID,
            crate::design::presentation::BROWSER_NODE_TYPE_VERSION,
            "Fusion",
            &nodes,
        ),
        (
            body_type().0,
            body_type().1,
            body_type().2,
            body_type().3,
            &entities,
        ),
        (
            crate::records::body_ownership::COMPONENT_GUID,
            crate::records::body_ownership::CONTAINER_BASE_GUID,
            1,
            "Body",
            &component_entities,
        ),
        container_type(&containers),
    ];
    if !other_version.is_empty() {
        let mut registration = container_type(&other_version);
        registration.2 = 3;
        types.push(registration);
    }
    if bound_cluster {
        records.push((300, u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"261", 300);
        bulk.push(1);
        bulk.extend_from_slice(&301_u64.to_le_bytes());
        bulk.extend_from_slice(&[0; 2]);
        super::lp_utf16(&mut bulk, "00000000-0000-4000-8000-000000000300");
        types.extend([
            (
                crate::records::body_ownership::COMPONENT_GUID,
                "",
                2,
                "Component",
                &[300][..],
            ),
            (
                "00000000-0000-4000-8000-000000000011",
                "21F379C8-CAFD-4985-B461-767673A4C502",
                2,
                "Component",
                &[301][..],
            ),
        ]);
    }
    let scope_registration = [u64::from(SCOPE)];
    let selection_registration = [u64::from(SELECTION)];
    if let Some(consumer) = consumer {
        let class_tag = |ordinal: usize| -> [u8; 3] {
            (256 + u32::try_from(ordinal).unwrap())
                .to_string()
                .into_bytes()
                .try_into()
                .unwrap()
        };
        records.push((u64::from(SCOPE), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, class_tag(types.len()), SCOPE);
        bulk.extend_from_slice(&[0; 9]);
        types.push((
            consumer.scope_type_guid,
            "2EDC2536-D95B-4469-8ACA-DC07124D9C43",
            1,
            "Fusion",
            &scope_registration,
        ));
        let class = class_tag(types.len());
        types.push((
            "9716F783-676D-42E0-93F9-EBF273E7C035",
            "6144EA59-3ACA-4172-9F14-A6531E94D69B",
            1,
            "Fusion",
            &selection_registration,
        ));
        records.push((u64::from(SELECTION), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, class, SELECTION);
        bulk.extend_from_slice(&[0; 10]);
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        local(&mut bulk, u64::from(SELECTION) + 3);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        super::lp_utf16(&mut bulk, "11111111-2222-3333-4444-555555555555");
        super::lp_utf16(&mut bulk, "66666666-7777-8888-9999-AAAAAAAAAAAA");
        for word in [2_u32, 0, 0, 1] {
            bulk.extend_from_slice(&word.to_le_bytes());
        }
        local(&mut bulk, u64::from(NODE));
        bulk.extend_from_slice(&9_u32.to_le_bytes());
        if let Some(owner) = consumer.selection_owner {
            local(&mut bulk, u64::from(owner));
        }
    }
    let meta = super::streams_test::design_metastream_with_records(&types, &records);
    (bulk, meta)
}
