// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::identity::Located;

#[test]
fn text_parameter_owner_stores_the_text_in_its_scalar_envelope() {
    let evaluated = Located {
        value: 0.0,
        offset: 320,
    };
    for text in ["AB", "X", "Hello world 123"] {
        let mut frame = parameter_owner_frame();
        frame.splice(39..48, text_envelope(text));
        let owner = parse_text_parameter_owner(&frame, text, evaluated, 100).expect("text owner");
        assert_eq!(
            owner.frame_length,
            u64::try_from(100 + 2 * text.len()).unwrap()
        );
        assert_eq!(owner.scope_record_index, 12);
        assert_eq!(owner.local_ordinal, 2);
        assert_eq!(owner.parameter_record_index, 45);
        assert_eq!(owner.companion_record_index, 46);
        assert_eq!(owner.variant, Some(1));
        assert_eq!(owner.evaluated_value.get(), 0.0);
        assert_eq!(owner.evaluated_value_offset.absolute(100), Some(320));
        assert!(parse_text_parameter_owner(&frame, "AC", evaluated, 100).is_none());
    }

    assert!(parse_text_parameter_owner(&parameter_owner_frame(), "AB", evaluated, 100).is_none());
    let mut marked = parameter_owner_frame();
    marked.splice(39..48, text_envelope("AB"));
    marked[39] = 1;
    assert!(parse_text_parameter_owner(&marked, "AB", evaluated, 100).is_none());

    let mut padded = parameter_owner_frame();
    let mut envelope = text_envelope("AB");
    envelope.push(0);
    padded.splice(39..48, envelope);
    assert!(parse_text_parameter_owner(&padded, "AB", evaluated, 100).is_none());
}
