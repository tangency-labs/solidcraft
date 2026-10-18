//! Identification colours, independent of materials and appearances.

use crate::Document;

/// Fixed palette: never reorder it, since automatic colours and cycling depend on it.
pub const PALETTE: [[u8; 3]; 12] = [
    [209, 125, 114],
    [112, 163, 214],
    [144, 185, 113],
    [185, 142, 205],
    [218, 177, 93],
    [106, 184, 176],
    [216, 143, 181],
    [148, 159, 210],
    [192, 160, 121],
    [155, 190, 166],
    [215, 153, 104],
    [153, 175, 190],
];

/// A specified integer mixer keeps colours stable across platforms and Rust versions.
pub fn automatic_color(id: u64) -> [u8; 3] {
    let mut mixed = id.wrapping_add(0x9e3779b97f4a7c15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
    mixed ^= mixed >> 31;
    PALETTE.get((mixed % PALETTE.len() as u64) as usize).copied().unwrap_or([112, 163, 214])
}

pub fn next_color(color: [u8; 3]) -> [u8; 3] {
    let index = PALETTE.iter().position(|c| *c == color).map_or(0, |i| (i + 1) % PALETTE.len());
    PALETTE.get(index).copied().unwrap_or([112, 163, 214])
}

impl Document {
    pub fn component_identification_color(&self, id: u64) -> [u8; 3] {
        self.components.iter().find(|c| c.id == id).and_then(|c| c.identification_color).unwrap_or_else(|| automatic_color(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_components_default_to_stable_colors_and_overrides_round_trip() {
        let mut doc = Document::default();
        let id = doc.add_component(Some("Part"), 0).unwrap();
        let automatic = doc.component_identification_color(id);
        let mut legacy: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
        legacy["components"][0].as_object_mut().unwrap().remove("identification_color");
        legacy["format_pending"].as_array_mut().unwrap().retain(|name| name != "component-identification-colors");
        let restored = Document::from_json(&legacy.to_string()).unwrap();
        assert_eq!(restored.component_identification_color(id), automatic);
        assert_eq!(restored.components[0].identification_color, None);
        let override_color = next_color(automatic);
        assert_ne!(override_color, automatic);
        doc.components[0].identification_color = Some(override_color);
        doc.components[0].name = "Renamed".into();
        let saved = Document::from_json(&doc.to_json()).unwrap();
        assert_eq!(saved.component_identification_color(id), override_color);
        assert_eq!(saved.components[0].identification_color, Some(override_color));
    }

    #[test]
    fn cycling_visits_every_palette_color_and_wraps() {
        let original = automatic_color(123);
        let mut seen = std::collections::BTreeSet::new();
        let mut color = original;
        for _ in 0..PALETTE.len() {
            assert!(seen.insert(color));
            color = next_color(color);
        }
        assert_eq!(color, original);
        assert_eq!(automatic_color(1), [106, 184, 176]);
    }
}
