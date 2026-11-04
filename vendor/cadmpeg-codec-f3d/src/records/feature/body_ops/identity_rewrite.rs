// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{DesignCopiedBody, DesignCopyPasteBodiesOperation, DesignScaleOperation};

rewrite_native_scalar!(DesignCopiedBody);
rewrite_native_record!(DesignCopyPasteBodiesOperation, []; {bodies, body_group_record_index, body_group_class_tag, body_group_byte_offset, relation_record_index, relation_class_tag, relation_byte_offset});
rewrite_native_scalar!(DesignScaleOperation);
