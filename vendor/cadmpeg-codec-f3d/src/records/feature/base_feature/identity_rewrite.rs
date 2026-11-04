// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignBaseFeatureBodyReferenceForm, DesignBaseFeatureCompactMode,
    DesignBaseFeatureConstruction, DesignBaseFeatureEntry, DesignBaseFeatureResultBody,
    DesignBaseFeatureResults, DesignLegacyBaseFeatureBody,
};

rewrite_native_scalar!(DesignBaseFeatureBodyReferenceForm);
rewrite_native_scalar!(DesignBaseFeatureCompactMode);
rewrite_native_enum!(DesignBaseFeatureConstruction, []; {
    ResultBodies {bodies, metadata_record, metadata_record_offset, metadata_field},
    BodyBasedOnFaces {body, parameter_body_record, parameter_body_record_offset, auxiliary_record, auxiliary_record_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces_offset},
    LegacyBodyBasedOnFaces {form, scope_reference, scope_reference_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces_offset},
    BodySnapshot {bodies, related_guids, related_guid_offsets, linkage_record, linkage_record_offset, auxiliary_record, auxiliary_record_offset},
    TimelineOwnedBodies {parameter_bodies, bodies, owner_record, owner_reference_offset},
});
rewrite_native_record!(DesignBaseFeatureEntry<T>, [T]; {value, offset, field});
rewrite_native_scalar!(DesignBaseFeatureResultBody);
rewrite_native_enum!(DesignBaseFeatureResults, []; {
    WithoutRepeatedFields(field0),
    WithRepeatedFields {first, rest},
});
rewrite_native_scalar!(DesignLegacyBaseFeatureBody);
