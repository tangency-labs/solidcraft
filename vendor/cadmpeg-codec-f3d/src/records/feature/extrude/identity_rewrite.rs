// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue,
    DesignExtrudePrologueReference, DesignExtrudeStart, DesignExtrudeTargetOrdinal,
};

rewrite_native_scalar!(DesignExtrudeExtent);
rewrite_native_scalar!(DesignExtrudeOperation);
rewrite_native_scalar!(DesignExtrudePrologue);
rewrite_native_scalar!(DesignExtrudePrologueReference);
rewrite_native_scalar!(DesignExtrudeStart);
rewrite_native_scalar!(DesignExtrudeTargetOrdinal);
