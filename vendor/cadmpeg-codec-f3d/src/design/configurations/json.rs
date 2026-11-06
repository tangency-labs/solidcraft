// SPDX-License-Identifier: Apache-2.0
//! Parse configuration JSON, admitting each nested value against the decode depth limit.

use crate::design::json_value::ValueSeed;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::de::DeserializeSeed;
use serde_json::Value;

pub(super) fn parse_configuration_payload(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
) -> Result<Value, CodecError> {
    let mut refusal = None;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = ValueSeed {
        ctx,
        refusal: &mut refusal,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| {
        deserializer.end()?;
        Ok(value)
    });
    match parsed {
        Ok(value) => Ok(value),
        Err(error) => match refusal {
            Some(error) => Err(error),
            None => Err(CodecError::malformed(format_args!(
                "invalid F3D configuration JSON {entry_name}: {error}"
            ))),
        },
    }
}

#[cfg(test)]
mod tests;
