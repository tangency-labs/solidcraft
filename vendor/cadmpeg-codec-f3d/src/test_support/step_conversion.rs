// SPDX-License-Identifier: Apache-2.0
//! STEP conversion along the path `cadmpeg convert --to step` takes.
//!
//! The CLI decodes in one session, validates the neutral model and the F3D
//! native namespace in a second session sized from the input, plans the
//! inherited STEP target, refuses error findings the output would carry, and
//! then writes the plan.

use cadmpeg_codec_step::StepCodec;
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::validate::validate_neutral_with_source_fidelity_for_decode;
use cadmpeg_ir::CadIr;

use crate::F3dCodec;

/// A completed conversion.
pub(crate) struct StepConversion {
    pub(crate) ir: CadIr,
    pub(crate) decode: DecodeReport,
    pub(crate) step: Vec<u8>,
}

/// Convert an `.f3d` archive to STEP under the desktop profile, panicking with
/// the refusal the CLI would report.
pub(crate) fn convert_to_step(bytes: &[u8]) -> StepConversion {
    let policy = DecodePolicy::default();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("acquire the archive");
    let decoded = F3dCodec.decode_with_context(&ctx, root);
    ctx.finish_session().expect("the decode session closes");
    let (ir, decode, fidelity) = decoded
        .unwrap_or_else(|error| panic!("decode refused: {error:?}"))
        .into_parts();

    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, u64_from_index(bytes.len()))
        .expect("validation session");
    let mut validation = validate_neutral_with_source_fidelity_for_decode(
        &ctx,
        &ir,
        &fidelity,
        decode.losses.clone(),
    )
    .expect("neutral validation");
    validation.findings.extend(
        F3dCodec
            .validate_native(&ctx, &ir, None)
            .expect("native validation"),
    );
    ctx.finish_session().expect("the validation session closes");

    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("plan the STEP export");
    let carried = plan.carried_errors(&ir, &validation);
    assert!(carried.is_empty(), "the export carries errors: {carried:?}");
    let mut step = Vec::new();
    plan.write_to(&mut step).expect("write the STEP export");
    StepConversion { ir, decode, step }
}
