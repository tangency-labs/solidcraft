// SPDX-License-Identifier: Apache-2.0
//! Which documents a decode below full depth stops before design intent.

use crate::decode::cuts_design_intent;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::report::ReportScope;

fn scope(kind: DesignFeatureKind, record_index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
        kind,
        record_index,
    )
}

/// A standalone design whose scopes project no image stops before design
/// intent: nothing phase 2 writes reaches a geometry-depth document.
#[test]
fn a_standalone_design_without_image_scopes_cuts_design_intent() {
    assert!(cuts_design_intent(
        &ReportScope::Standalone,
        &[scope(DesignFeatureKind::Sketch, 23)],
        false,
    ));
}

/// Canvas and Decal scopes project their images into model assets, which a
/// geometry-depth document keeps.
#[test]
fn an_image_scope_keeps_design_intent() {
    for kind in [DesignFeatureKind::Canvas, DesignFeatureKind::Decal] {
        assert!(!cuts_design_intent(
            &ReportScope::Standalone,
            &[scope(DesignFeatureKind::Sketch, 23), scope(kind, 24)],
            false,
        ));
    }
}

/// A bodyless document without a body source classifies itself by the sketch
/// entities and canvas images design intent projects.
#[test]
fn a_bodyless_document_without_a_body_source_keeps_design_intent() {
    assert!(!cuts_design_intent(&ReportScope::Standalone, &[], true));
}
