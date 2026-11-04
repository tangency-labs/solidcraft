// SPDX-License-Identifier: Apache-2.0
use super::parse_configuration_payload;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn refuse(bytes: &[u8], policy: DecodePolicy, dimension: ResourceDimension, operation: &str) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(parse_configuration_payload(&ctx, "table.dsgcfg", bytes),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == dimension && failure.operation == operation),
        "{operation}"
    );
}

#[test]
fn configuration_json_nested_values_refuse_depth_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 2;
    refuse(
        b"[[null]]",
        policy,
        ResourceDimension::RecursionDepth,
        "f3d configuration JSON depth",
    );
}

#[test]
fn configuration_json_values_and_diagnostics_match_serde() {
    for text in [
        "null",
        "true",
        "false",
        "-0.0",
        "-9223372036854775808",
        "18446744073709551615",
        "1.25e-200",
        "[{},[null,true,false],\"\\u00e9\"]",
        "{\"a\":1,\"a\":2,\"\\u00e9\":\"\\ud83d\\ude00\"}",
        "{\"$serde_json::private::RawValue\":\"[true,null]\"}",
        "{\"$serde_json::private::RawValue\":\"[\"}",
        "{\"$serde_json::private::RawValue\":\"null true\"}",
        "{\"$serde_json::private::RawValue\":false}",
        "{\"$serde_json::private::RawValue\":\"null\",\"x\":1}",
        "{\"a\":0,\"$serde_json::private::RawValue\":\"null\"}",
        "[",
        "{",
        "[true,]",
        "{\"a\":}",
        "null true",
        "1e400",
    ] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let actual = parse_configuration_payload(&ctx, "table.dsgcfg", text.as_bytes());
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(expected) => {
                let actual = actual.unwrap();
                assert_eq!(actual, expected, "{text}");
                assert_eq!(
                    serde_json::to_vec(&actual).unwrap(),
                    serde_json::to_vec(&expected).unwrap()
                );
            }
            Err(expected) => assert!(
                matches!(actual, Err(CodecError::Malformed(message))
                if message == format!("invalid F3D configuration JSON table.dsgcfg: {expected}")),
                "{text}"
            ),
        }
    }
}
