//! Capability `effects@0.1`: a plugin offers an effect the host draws itself, such as the RGB wave
//! over the ribbon's icons. The plugin names the kind; the host owns the drawing and the settings,
//! and draws only while the plugin is on and its effect is chosen.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HostError, Result, invalid, plain, valid_id};

/// The most effects one plugin declares.
pub const MAX_EFFECTS: usize = 8;

/// The kinds this host draws. An effect of any other kind is refused at install.
pub const KINDS: &[&str] = &["hue-wave"];

/// One effect in a plugin's manifest (`"effects": [...]`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectDecl {
    /// Unique within the plugin; the app names it `<plugin-id>/<id>`.
    pub id: String,
    /// What a person sees on the plugin's card.
    pub name: String,
    /// What the host draws, one of [`KINDS`] (`"hue-wave"`).
    pub kind: String,
}

/// Checks a manifest's `effects` list against its capabilities: the capability is needed, at most
/// [`MAX_EFFECTS`], ids valid and distinct, names plain, and every kind one this host draws.
pub fn validate(effects: &[EffectDecl], capabilities: &[String]) -> Result<()> {
    if effects.is_empty() {
        return Ok(());
    }
    if !capabilities.iter().any(|capability| capability.starts_with("effects@")) {
        return Err(invalid("capabilities", "a plugin with effects needs the effects capability: \"capabilities\": [\"effects@0.1\"]"));
    }
    if effects.len() > MAX_EFFECTS {
        return Err(invalid("effects", format!("at most {MAX_EFFECTS} effects")));
    }
    let mut ids = BTreeSet::new();
    for effect in effects {
        let field = format!("effects.{}", effect.id);
        if !valid_id(&effect.id) || !ids.insert(&effect.id) {
            return Err(invalid("effects", "each effect needs a distinct lowercase id"));
        }
        if !plain(&effect.name, 80) {
            return Err(invalid(format!("{field}.name"), "requires 1..80 printable characters"));
        }
        if !KINDS.contains(&effect.kind.as_str()) {
            return Err(HostError::InvalidManifest {
                field: format!("{field}.kind"),
                reason: format!("needs effect kind {}; update SolidCraft or the plugin", effect.kind),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(id: &str, kind: &str) -> EffectDecl {
        EffectDecl { id: id.into(), name: "RGB wave".into(), kind: kind.into(), picture: None }
    }

    fn caps() -> Vec<String> {
        vec!["effects@0.1".into()]
    }

    #[test]
    fn a_hue_wave_effect_with_the_capability_is_accepted() {
        assert_eq!(validate(&[effect("rgb-wave", "hue-wave")], &caps()), Ok(()));
        assert_eq!(validate(&[], &[]), Ok(()));
    }

    #[test]
    fn effects_need_the_capability_and_the_rules_hold() {
        let one = [effect("rgb-wave", "hue-wave")];
        assert!(matches!(validate(&one, &[]), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
        let many: Vec<EffectDecl> = (0..9).map(|i| effect(&format!("e{i}"), "hue-wave")).collect();
        assert!(validate(&many, &caps()).is_err());
        assert!(validate(&[effect("a", "hue-wave"), effect("a", "hue-wave")], &caps()).is_err(), "ids are distinct");
        assert!(validate(&[effect("Bad Id", "hue-wave")], &caps()).is_err());
        let blank = EffectDecl { id: "ok".into(), name: " ".into(), kind: "hue-wave".into(), picture: None };
        assert!(validate(&[blank], &caps()).is_err());
    }

    #[test]
    fn an_unknown_kind_is_refused_naming_the_kind() {
        match validate(&[effect("glow", "neon-rain")], &caps()) {
            Err(HostError::InvalidManifest { field, reason }) => {
                assert_eq!(field, "effects.glow.kind");
                assert_eq!(reason, "needs effect kind neon-rain; update SolidCraft or the plugin");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn with_picture(kind: &str, picture: Option<&str>) -> EffectDecl {
        EffectDecl { picture: picture.map(str::to_string), ..effect("sky", kind) }
    }

    #[test]
    fn a_galaxy_effect_is_accepted_with_or_without_a_picture() {
        assert_eq!(validate(&[with_picture("galaxy", None)], &caps()), Ok(()));
        assert_eq!(validate(&[with_picture("galaxy", Some("sky/stars.jpg"))], &caps()), Ok(()));
        assert_eq!(validate(&[with_picture("galaxy", Some("stars.PNG"))], &caps()), Ok(()));
    }

    #[test]
    fn a_picture_path_stays_inside_the_package_and_names_an_image() {
        for bad in
            ["", "/stars.jpg", "../stars.jpg", "a/../stars.jpg", "a\\stars.jpg", "C:/stars.jpg", "stars.gif", "stars", "a//b.jpg", "./stars.jpg"]
        {
            match validate(&[with_picture("galaxy", Some(bad))], &caps()) {
                Err(HostError::InvalidManifest { field, .. }) => assert_eq!(field, "effects.sky.picture", "{bad:?}"),
                other => panic!("{bad:?}: expected a refusal, got {other:?}"),
            }
        }
    }

    #[test]
    fn only_a_kind_that_draws_a_picture_takes_one() {
        assert!(validate(&[with_picture("hue-wave", Some("stars.jpg"))], &caps()).is_err());
    }

    #[test]
    fn an_effect_without_a_picture_reads_and_writes_as_before() {
        let text = r#"{"id":"rgb-wave","name":"RGB wave","kind":"hue-wave"}"#;
        let parsed: EffectDecl = serde_json::from_str(text).unwrap();
        assert_eq!(parsed.picture, None);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), text);
    }
}
