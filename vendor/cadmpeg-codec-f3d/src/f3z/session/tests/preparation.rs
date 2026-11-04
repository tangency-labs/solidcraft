// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::hash::digest::Sha256Digest;

fn scans_of(scans: &[String], source: &[u8]) -> usize {
    let digest = Sha256Digest::digest(source);
    scans
        .iter()
        .filter(|scan| scan.as_str() == digest.as_str())
        .count()
}

#[test]
fn archive_session_open_scans_each_nested_member_once() {
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let leaf = crate::test_support::zip_test::f3d_with_deflated_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let unused = f3d_without_brep("part-design", "unused.f3d", &[]);
    let members = [
        ("root.f3d", root.as_slice()),
        ("leaf.f3d", leaf.as_slice()),
        ("unused.f3d", unused.as_slice()),
    ];
    for bytes in [
        f3z_archive("root.f3d", &members),
        crate::test_support::archive_session::deflated_archive("root.f3d", &members),
    ] {
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let (session, scans) =
            crate::test_support::scan_counts::track(|| F3dCodec.open_archive(&ctx, view).unwrap());
        assert_eq!(scans.len(), 4);
        for source in [&bytes, &root, &leaf, &unused] {
            assert_eq!(scans_of(&scans, source), 1);
        }
        assert_eq!(session.members()[0].references(), &["leaf.f3d"]);
        assert_eq!(session.members()[0].kind(), &F3zMemberKind::PlacementOnly);

        let (decoded, scans) =
            crate::test_support::scan_counts::track(|| session.decode_member("leaf.f3d").unwrap());
        assert!(decoded.decoded().unwrap().is_ok());
        assert_eq!(scans, [Sha256Digest::digest(&leaf).as_str().to_owned()]);
    }
}
