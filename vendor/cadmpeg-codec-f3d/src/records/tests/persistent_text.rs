// SPDX-License-Identifier: Apache-2.0

use crate::records::sketch_links::{
    DesignPersistentIdText, PersistentDesignLink, PersistentSubentityTag,
};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::{BodyId, FaceId};

#[test]
fn persistent_design_text_preserves_decimal_spelling_without_an_integer_bound() {
    for text in ["0", "000301", "18446744073709551616000000000000000000"] {
        let design_id = DesignPersistentIdText::try_from(text.to_owned()).unwrap();
        assert_eq!(design_id.as_str(), text);
        let link = PersistentDesignLink {
            id: "link".into(),
            target: AttributeTarget::Body(BodyId::mint("test:model:body#1").unwrap()),
            design_id,
            design_reference: 1,
            ordinal: 0,
        };
        let wire = serde_json::to_value(&link).unwrap();
        assert_eq!(wire["design_id"], text);
        assert_eq!(
            serde_json::from_value::<PersistentDesignLink>(wire.clone()).unwrap(),
            link
        );
        for invalid in ["", "-1", "+1", " 1", "1 ", "1.0", "a", "１２"] {
            assert!(DesignPersistentIdText::try_from(invalid.to_owned()).is_err());
            let mut invalid_wire = wire.clone();
            invalid_wire["design_id"] = invalid.into();
            assert!(serde_json::from_value::<PersistentDesignLink>(invalid_wire)
                .unwrap_err()
                .to_string()
                .contains("design_id"));
        }
    }
}

#[test]
fn persistent_subentity_tokens_require_content_and_preserve_non_numeric_text() {
    assert!(NonBlankString::new("").is_none());
    assert!(NonBlankString::new(" ").is_none());
    for text in ["-1", "0003", " a ", "named-token", "面"] {
        let tag = PersistentSubentityTag {
            id: "tag".into(),
            target: AttributeTarget::Face(FaceId::mint("test:model:face#1").unwrap()),
            selector: 1,
            token: NonBlankString::new(text).unwrap(),
            design_references: vec![],
            ordinal: 0,
        };
        let wire = serde_json::to_value(&tag).unwrap();
        assert_eq!(wire["token"], text);
        assert_eq!(
            serde_json::from_value::<PersistentSubentityTag>(wire.clone()).unwrap(),
            tag
        );
        let mut invalid = wire;
        invalid["token"] = "".into();
        assert!(serde_json::from_value::<PersistentSubentityTag>(invalid)
            .unwrap_err()
            .to_string()
            .contains("token"));
    }
}
