// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignFixedChamferDistance, DesignFixedChamferParameters, DesignFixedExtrudeDistance,
    DesignFixedExtrudeParameters, DesignFixedExtrudeScalar, DesignFixedFilletGroup,
    DesignFixedFilletIntermediate, DesignFixedFilletLaw, DesignFixedFilletParameters,
    DesignFixedFilletScalar,
};

rewrite_native_record!(DesignFixedChamferDistance, []; {value, record_index, value_offset});
rewrite_native_enum!(DesignFixedChamferParameters, []; {
    EqualDistance {distance},
    TwoDistances {first, second},
});
rewrite_native_enum!(DesignFixedExtrudeDistance, []; {
    FixedScalar(field0),
    DistanceConstruction(field0),
});
rewrite_native_record!(DesignFixedExtrudeParameters, []; {along_distance, taper_angle});
rewrite_native_record!(DesignFixedExtrudeScalar<T>, [T]; {value, record_index, value_offset});
rewrite_native_record!(DesignFixedFilletGroup, []; {tangency_weight, law});
rewrite_native_record!(DesignFixedFilletIntermediate, []; {radius, parameter});
rewrite_native_enum!(DesignFixedFilletLaw, []; {
    Constant(field0),
    Variable {start, end, intermediate},
});
rewrite_native_record!(DesignFixedFilletParameters, []; {groups});
rewrite_native_record!(DesignFixedFilletScalar, []; {value, record_index, value_offset});
