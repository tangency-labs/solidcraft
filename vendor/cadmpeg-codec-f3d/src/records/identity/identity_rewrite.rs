// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignAffineTransform, DesignEntityId, DesignSecondaryIdentity, Located, MaybeRecordedValue,
    RecordedValue, ReferenceRun, ReferenceRunData,
};

rewrite_native_scalar!(DesignAffineTransform);
rewrite_native_record!(DesignEntityId, []; {text, suffix});
rewrite_native_record!(DesignSecondaryIdentity<Id>, [Id]; {identity, curve_identity});
rewrite_native_record!(Located<T, O>, [T, O]; {value, offset});
rewrite_native_enum!(MaybeRecordedValue<T>, [T]; {
    Located(field0),
    Unlocated(field0),
});
rewrite_native_record!(RecordedValue<T>, [T]; {value, offset});
rewrite_native_record!(ReferenceRun<T, O>, [T, O]; (field0));
rewrite_native_enum!(ReferenceRunData<T, O>, [T, O]; {
    Unlocated(field0),
    Located(field0),
});
