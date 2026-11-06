// SPDX-License-Identifier: Apache-2.0
//! Rebinding of member-owned locations before archive composition.

use cadmpeg_core::decode::DecodeSpaceRebinding;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;

pub(super) fn rebind_member(
    decoded: &mut Result<Decoded, CodecError>,
    spaces: &DecodeSpaceRebinding,
) -> bool {
    if spaces.is_identity() {
        return true;
    }
    match decoded {
        Ok(decoded) => decoded
            .body
            .losses
            .iter()
            .map(|loss| loss.message.as_str())
            .chain(decoded.body.notes.iter().map(String::as_str))
            .all(|message| !has_space_location(message)),
        Err(CodecError::Truncated { location, .. }) => {
            spaces.rebind(&mut location.space);
            true
        }
        Err(error) => !has_space_location(&error.to_string()),
    }
}

fn has_space_location(message: &str) -> bool {
    message
        .split("space ")
        .skip(1)
        .any(|tail| tail.as_bytes().first().is_some_and(u8::is_ascii_digit))
        || message.contains("SpaceId(")
}
