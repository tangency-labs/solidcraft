// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for body-reference-tail fields.

use super::{BodyTailReference, DesignBodyReferenceTail};
rewrite_native_record!(BodyTailReference, []; {offset, encoding});
rewrite_native_record!(DesignBodyReferenceTail, []; {id, record_index, byte_offset, serialization_form, first_count_offset, first_references, second_count_offset, second_references, extra_zero, trailing_form});
