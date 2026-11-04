// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{BodyVisibility, DesignBodyBinding, DesignBulkStreamPath};
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;

rewrite_native_record!(BodyVisibility, []; {id, body, stream, byte_offset, asm_body_key_offset, asm_body_key, entity_suffix, visible});
rewrite_native_record!(DesignBodyBinding, []; {id, stream, pair_count, pair_ordinal, asm_body_key, asm_body_key_offset, entity_suffix, blob_name, blob_name_offset, body});

impl RewriteIdentities for DesignBulkStreamPath {
    fn rewrite_native_value<
        RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
    >(
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Ok(())
    }

    fn visit_identity_references(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Ok(())
    }

    fn rewrite_identities<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let text = self.0.rewrite_identities(ctx, map)?;
        Self::try_from(text.into_string()).map_err(cadmpeg_core::CodecError::malformed)
    }
}
