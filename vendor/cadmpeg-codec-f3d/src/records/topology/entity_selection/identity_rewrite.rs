// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{DesignEntitySelectionFaceCandidate, SelectionSerializationForm};

rewrite_native_record!(DesignEntitySelectionFaceCandidate, []; {history_id, historical, face_slot});
rewrite_native_scalar!(SelectionSerializationForm);
