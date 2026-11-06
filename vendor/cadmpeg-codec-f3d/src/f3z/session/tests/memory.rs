// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn archive_session_retained_estimate_for_skipped_member_is_id_length() {
    let root = f3d_without_brep("part-design", "root.f3d", &[]);
    let bytes = f3z_archive(
        "root.f3d",
        &[("root.f3d", &root), ("sheet.f2d", b"drawing")],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    assert_eq!(
        session
            .decode_member("sheet.f2d")
            .unwrap()
            .retained_bytes_estimate()
            .unwrap(),
        9
    );
}

#[test]
fn archive_session_member_result_outlives_source_and_releases_archive_ownership() {
    let member = {
        let root = f3d_without_brep("part-design", "root.f3d", &[]);
        let bytes = crate::test_support::archive_session::deflated_archive(
            "root.f3d",
            &[("root.f3d", &root)],
        );
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let session = F3dCodec.open_archive(&ctx, view).unwrap();
        assert!(!session.scan.retains_entry_payload("root.f3d"));
        let owners = session.scan.archive_owner_count();
        let member = session.decode_member("root.f3d").unwrap();
        assert_eq!(session.scan.archive_owner_count(), owners);
        assert!(!session.scan.retains_entry_payload("root.f3d"));
        member
    };
    assert!(member.decoded().unwrap().is_ok());
    let decoded = member.decoded().unwrap().unwrap();
    let body = &decoded.body;
    let payload = serde_json::to_vec(&(
        &decoded.ir,
        (
            &body.transfer,
            &*body.coverage,
            &body.losses,
            &body.notes,
            &body.transfer_ledger,
        ),
        &decoded.source_fidelity,
    ))
    .unwrap();
    let expected = u64::try_from(payload.len() + "root.f3d".len()).unwrap()
        + member.trace.retained_bytes_estimate();
    assert_eq!(member.retained_bytes_estimate().unwrap(), expected);
}

#[test]
fn archive_session_working_estimate_uses_largest_member_not_archive_sum() {
    let leaf = f3d_without_brep("part-design", "leaf.f3d", &[]);
    let larger = f3d_without_brep("assembly-design", "root.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let declared = |bytes: &[u8]| {
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
        let directory = cadmpeg_container::ArchiveSnapshot::new(&ctx, view).unwrap();
        u64::try_from(bytes.len()).unwrap()
            + directory
                .entries()
                .iter()
                .map(|entry| entry.uncompressed_size)
                .sum::<u64>()
    };
    let expected = declared(&leaf).max(declared(&larger));
    let bytes = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &larger),
            ("leaf.f3d", &leaf),
            ("extra.f3d", &leaf),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    assert_eq!(
        session.largest_member_working_bytes_estimate(),
        Some(expected)
    );
}
