// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignMirrorConstruction, DesignMirrorScopeTolerance, DesignMirrorToleranceMarker,
    DesignMirrorToleranceSource,
};

rewrite_native_scalar!(DesignMirrorConstruction);
rewrite_native_scalar!(DesignMirrorScopeTolerance);
rewrite_native_scalar!(DesignMirrorToleranceMarker);
rewrite_native_scalar!(DesignMirrorToleranceSource);
