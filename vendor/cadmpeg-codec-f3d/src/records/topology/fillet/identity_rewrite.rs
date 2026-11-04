// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::HistoricalBinding;

rewrite_native_record!(HistoricalBinding, []; {kind, entity_ref, state_ids});
