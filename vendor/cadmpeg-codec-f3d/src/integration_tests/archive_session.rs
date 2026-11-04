// SPDX-License-Identifier: Apache-2.0

use crate::test_support::archive_session::{assert_equal, decode};
use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, ResourceDimension};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions, DecodeResult, FormatId};
use std::io::Cursor;

fn archive() -> Vec<u8> {
    let leaf = f3d_with_smbh(&synthetic_geometry_smbh());
    let child = f3d_without_brep("assembly-design", "child.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[
            ("child.f3d", XREF_ROLE),
            ("leaf.f3d", XREF_ROLE),
            ("child.f3d", XREF_ROLE),
        ],
    );
    f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("leaf.f3d", &leaf),
            ("child.f3d", &child),
            ("unreadable.f3d", b"invalid"),
            ("drawing.f2d", b"drawing"),
        ],
    )
}

fn traversal_decode(bytes: &[u8], options: &DecodeOptions) -> Result<DecodeResult, DecodeFailure> {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::read_root(
        &mut Cursor::new(bytes),
        &arena,
        &options.policy,
        options.depth,
    )?;
    let session = F3dCodec.open_archive(&ctx, root)?;
    let result = session
        .compose_in_traversal_for_test(&ctx)
        .map_err(DecodeFailure::from)
        .and_then(|decoded| DecodeResult::from_decoded(&ctx, decoded, FormatId::new("f3d")));
    ctx.finish_session()?;
    result
}

#[test]
fn archive_session_parallel_decode_matches_traversal_and_arrival_orders() {
    let bytes = archive();
    let options = DecodeOptions::default();
    let baseline = traversal_decode(&bytes, &options);
    let result = decode(&mut Cursor::new(&bytes), &options);
    assert_equal(&baseline, &result);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &options.policy).unwrap();
    let session = F3dCodec.open_archive(&ctx, root).unwrap();
    session.require_original_spaces_for_test(true);
    assert_equal(
        &baseline,
        &session.compose(
            session
                .members()
                .iter()
                .map(|member| session.decode_member(member.id()).unwrap()),
        ),
    );
    let before = session.composition_reexecutions();
    session.require_original_spaces_for_test(false);
    let members = std::thread::scope(|scope| {
        let handles = session
            .members()
            .iter()
            .rev()
            .map(|member| {
                let session = &session;
                scope.spawn(move || session.decode_member(member.id()).unwrap())
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .rev()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_equal(&baseline, &session.compose(members));
    assert_eq!(session.composition_reexecutions(), before);
    for order in 0..3 {
        let mut ids = session
            .members()
            .iter()
            .map(crate::F3zMember::id)
            .collect::<Vec<_>>();
        if order == 1 {
            ids.reverse();
        } else if order == 2 {
            ids.rotate_left(1);
            for index in (1..ids.len()).step_by(2) {
                ids.swap(index - 1, index);
            }
        }
        let members = ids
            .into_iter()
            .map(|id| session.decode_member(id).unwrap())
            .collect::<Vec<_>>();
        assert_equal(&baseline, &session.compose(members));
        assert_eq!(session.composition_reexecutions(), before);
    }
    println!("existing archive: original-space reexecutions={before}, relocated reexecutions=0");
}

#[test]
fn archive_session_refuses_missing_duplicate_and_foreign_results() {
    let bytes = archive();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodeOptions::default().policy).unwrap();
    let session = F3dCodec.open_archive(&ctx, root).unwrap();
    let foreign = F3dCodec.open_archive(&ctx, root).unwrap();
    assert!(session.compose(Vec::new()).is_err());
    assert!(session
        .compose([
            session.decode_member("root.f3d").unwrap(),
            session.decode_member("root.f3d").unwrap()
        ])
        .is_err());
    assert!(session
        .compose(
            foreign
                .members()
                .iter()
                .map(|member| foreign.decode_member(member.id()).unwrap())
        )
        .is_err());
}

#[test]
fn archive_session_limits_match_direct_traversal_refusal_details() {
    let bytes = archive();
    for dimension in [
        ResourceDimension::DecompressedBytes,
        ResourceDimension::RetainedBytes,
        ResourceDimension::Entities,
        ResourceDimension::RecursionDepth,
    ] {
        for ceiling in [0, 1, 7, 31, 127, 511, 2047, 8191, 32767, u64::MAX] {
            let mut options = DecodeOptions::default();
            match dimension {
                ResourceDimension::DecompressedBytes => {
                    options.policy.limits.max_decompressed_bytes_total = ceiling;
                }
                ResourceDimension::RetainedBytes => {
                    options.policy.limits.max_retained_bytes = ceiling;
                }
                ResourceDimension::Entities => options.policy.limits.max_entities = ceiling,
                ResourceDimension::RecursionDepth => {
                    options.policy.limits.max_recursion_depth = ceiling;
                }
                ResourceDimension::InputBytes => unreachable!(),
            }
            assert_equal(
                &traversal_decode(&bytes, &options),
                &F3dCodec.decode(&mut Cursor::new(&bytes), &options),
            );
        }
    }
}

#[test]
fn archive_session_repeated_member_refuses_at_the_original_entity_charge() {
    let bytes = archive();
    let mut options = DecodeOptions {
        policy: cadmpeg_core::decode::DecodePolicy {
            limits: cadmpeg_core::decode::ResourceLimits {
                max_entities: u64::MAX,
                ..cadmpeg_core::decode::ResourceLimits::default()
            },
            ..cadmpeg_core::decode::DecodePolicy::default()
        },
        ..DecodeOptions::default()
    };
    let operation = "F3Z repeated member occurrences and joints";
    let error = cadmpeg_core::decode::test_support::at_charge(
        ResourceDimension::Entities,
        operation,
        0,
        || traversal_decode(&bytes, &options),
    )
    .unwrap_err();
    let DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = error else {
        panic!("expected repeated-member resource refusal")
    };
    assert_eq!(limit.operation, operation);
    options.policy.limits.max_entities = limit.limit;
    let baseline = traversal_decode(&bytes, &options);
    assert_equal(&baseline, &decode(&mut Cursor::new(&bytes), &options));
}

#[test]
fn archive_session_depth_and_strict_policy_match_direct_traversal() {
    let bytes = archive();
    for depth in [
        cadmpeg_core::decode::DecodeDepth::Full,
        cadmpeg_core::decode::DecodeDepth::Geometry,
        cadmpeg_core::decode::DecodeDepth::Container,
    ] {
        for mode in [
            cadmpeg_core::decode::DecodeMode::Salvage,
            cadmpeg_core::decode::DecodeMode::Strict,
        ] {
            let options = DecodeOptions {
                depth,
                policy: cadmpeg_core::decode::DecodePolicy {
                    mode,
                    ..cadmpeg_core::decode::DecodePolicy::default()
                },
            };
            assert_equal(
                &traversal_decode(&bytes, &options),
                &decode(&mut Cursor::new(&bytes), &options),
            );
        }
    }
}

mod memory;

mod preparation;
