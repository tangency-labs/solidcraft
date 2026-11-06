// SPDX-License-Identifier: Apache-2.0
//! Agreement of the header-only history index with full history decoding.

use cadmpeg_asm::kernel_header::RefWidth;

use crate::test_support::tokens_test::{t_end, t_ident, t_long, t_ref};

/// One `delta_state` stream whose single board carries one change from `old`
/// to `new`.
fn one_change_history(old: i64, new: i64) -> Vec<u8> {
    let mut bytes = Vec::new();
    t_end(&mut bytes);
    t_ident(&mut bytes, "delta_state");
    for value in [2, 1, 0] {
        t_long(&mut bytes, value);
    }
    for reference in [-1, -1, 0, -1, 0] {
        t_ref(&mut bytes, reference);
    }
    bytes.push(0x0b);
    t_long(&mut bytes, 1);
    t_ref(&mut bytes, 0);
    t_long(&mut bytes, 2);
    t_long(&mut bytes, 1);
    t_ref(&mut bytes, old);
    t_ref(&mut bytes, new);
    t_long(&mut bytes, 0);
    t_long(&mut bytes, 0);
    t_end(&mut bytes);
    t_ident(&mut bytes, "End-of-ASM-History-Section");
    t_end(&mut bytes);
    bytes
}

fn decodes(bytes: &[u8]) -> bool {
    crate::test_support::with_decode_context(|ctx| {
        let mut remaining_retained_bytes = u64::MAX;
        crate::history::decode(
            ctx,
            bytes,
            "history",
            RefWidth::Eight,
            &mut remaining_retained_bytes,
            &cadmpeg_core::decode::WorkBudget::new(
                crate::history::binding::MAX_HISTORY_BINDING_WORK,
            ),
        )
        .expect("decode the history stream")
        .is_some()
    })
}

#[test]
fn header_index_drops_a_stream_whose_bulletin_board_full_decode_drops() {
    let framed = one_change_history(-1, 1);
    assert!(decodes(&framed));
    assert!(crate::history::decode_header_index(&framed, "history", RefWidth::Eight).is_some());

    let malformed = one_change_history(-1, -1);
    assert!(!decodes(&malformed));
    assert!(crate::history::decode_header_index(&malformed, "history", RefWidth::Eight).is_none());
}
