// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignPersistentIdText, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
    SketchLinkSense,
};

rewrite_native_record!(DesignPersistentIdText, []; (field0));
rewrite_native_identity_record!(PersistentDesignLink; target, design_id, design_reference, ordinal);
rewrite_native_identity_record!(PersistentSubentityTag; target, selector, token, design_references, ordinal);
rewrite_native_identity_record!(SketchCurveLink; target, sketch_curve_id, ref_b, sense, role, closure);
rewrite_native_scalar!(SketchLinkSense);
