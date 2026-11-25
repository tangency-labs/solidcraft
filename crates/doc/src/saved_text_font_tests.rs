//! A saved text whose font this computer lacks (#19230): it keeps its outlines, set in the
//! built-in font, and the sketch says which font stood in.

use crate::{Document, ModelState};
use solidcore_record::Plane;
use solidcraft_sketch::{LinkSource, Sketch, find_profiles};
use std::collections::BTreeMap;

/// A sketch as a design saves it: one text link and no curves yet (they are rebuilt on open).
fn saved(font: &str, bold: bool) -> Sketch {
    let json = format!(
        r#"{{"points": [], "curves": [], "constraints": [], "links": [{{"id": "j1", "kind": "text", "source": {{"type": "text", "text": "ABC", "at": [0, 0], "height": 6, "angle": 0,
            "font": "{font}", "style": {{"bold": {bold}}}}}}}]}}"#
    );
    serde_json::from_str(&json).unwrap()
}

fn refresh(sk: &mut Sketch) -> Vec<String> {
    crate::project::refresh_links(&Document::default(), &BTreeMap::new(), &ModelState::default(), &Plane::XY, sk).unwrap()
}

#[test]
fn a_saved_text_in_a_font_that_is_not_installed_keeps_its_outlines_in_the_built_in_font() {
    let mut sk = saved("NoSuchFamily", true);
    let warnings = refresh(&mut sk);
    assert!(!sk.curves.is_empty(), "the text lost its outlines: {warnings:?}");
    assert!(!find_profiles(&sk).is_empty(), "no closed profiles to extrude");
    assert!(sk.links.iter().all(|l| !l.lost), "the text was marked lost");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("NoSuchFamily") && warnings[0].contains("built-in font"), "{warnings:?}");
    // The text still names its own font, to be set in it where it is installed.
    assert!(matches!(&sk.links[0].source, LinkSource::Text { font: Some(f), style, .. } if f == "NoSuchFamily" && style.bold));
}

#[test]
fn a_saved_bold_text_in_the_built_in_font_falls_back_to_its_regular_face() {
    let mut sk = saved("", true);
    let warnings = refresh(&mut sk);
    assert!(!sk.curves.is_empty(), "the text lost its outlines: {warnings:?}");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("no bold face") && warnings[0].contains("regular face"), "{warnings:?}");
    // In a font it has, nothing to say.
    let mut plain = saved("", false);
    assert_eq!(refresh(&mut plain), Vec::<String>::new());
    assert!(!plain.curves.is_empty());
}
