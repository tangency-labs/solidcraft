//! Standard behaviours are registered here; file declarations alone do not offer a control.
use crate::{
    theme::Tokens,
    theme_files::{OptionDef, OptionValue, ThemeFile},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Availability {
    Available,
    /// Message-catalog id, translated at the presentation boundary.
    Unavailable(&'static str),
}

pub(crate) struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub availability: fn() -> Availability,
    pub apply: fn(&mut Tokens, &OptionValue),
}

pub(crate) const PROVIDERS: &[Provider] = &[
    Provider {
        id: "blur",
        label: "prefs.theme_option_blur",
        help: "prefs.theme_option_blur_help",
        availability: || Availability::Available,
        apply: apply_runtime_setting,
    },
    Provider {
        id: "opacity",
        label: "prefs.theme_option_opacity",
        help: "prefs.theme_option_opacity_help",
        availability: || Availability::Available,
        apply: apply_opacity,
    },
    Provider {
        id: "tab_colours",
        label: "prefs.theme_option_tab_colours",
        help: "prefs.theme_option_tab_colours_help",
        availability: || Availability::Available,
        apply: apply_runtime_setting,
    },
    Provider {
        id: "tab_fade_ms",
        label: "prefs.theme_option_tab_fade_ms",
        help: "prefs.theme_option_tab_fade_ms_help",
        availability: || Availability::Available,
        apply: apply_runtime_setting,
    },
];

/// Hardware/OS state is resolved per app, outside the theme token cache. An unavailable
/// ancestor disables its descendants in both the API and the UI.
pub(crate) fn availability(file: &ThemeFile, id: &str, providers: &[Provider], blur: Availability) -> Availability {
    let mut current = Some(id);
    for _ in 0..=crate::theme_files::MAX_OPTIONS {
        let Some(id) = current else { return Availability::Available };
        if let Some(p) = provider(id, providers) {
            let state = if id == "blur" && (p.availability)() == Availability::Available { blur } else { (p.availability)() };
            if state != Availability::Available {
                return state;
            }
        } else if standard(id) {
            return Availability::Unavailable("prefs.theme_options_unavailable");
        }
        current = file.options.iter().find(|o| o.id == id).and_then(|o| o.parent.as_deref());
    }
    Availability::Unavailable("prefs.theme_options_unavailable")
}

pub(crate) fn blur_radius(file: &ThemeFile, values: &BTreeMap<String, OptionValue>, blur: Availability) -> f32 {
    let definitions = offered(file, PROVIDERS);
    let Some(def) = definitions.iter().find(|o| o.id == "blur") else { return 0.0 };
    if availability(file, "blur", PROVIDERS, blur) != Availability::Available {
        return 0.0;
    }
    let mut parent = def.parent.as_deref();
    for _ in 0..crate::theme_files::MAX_OPTIONS {
        let Some(id) = parent else { break };
        if values.get(id) != Some(&OptionValue::Bool(true)) {
            return 0.0;
        }
        parent = definitions.iter().find(|o| o.id == id).and_then(|o| o.parent.as_deref());
    }
    match values.get("blur") {
        Some(OptionValue::Num(n)) if n.is_finite() && (0.0..=40.0).contains(n) => *n,
        _ => 0.0,
    }
}

/// All standard providers are registered; normalized declarations have validated ancestry.
/// This query is also used by cold UI/API compositions and does not order/allocate a table.
pub(crate) fn declares_blur(file: &ThemeFile) -> bool {
    !file.needs_newer() && file.options.iter().any(|o| o.id == "blur")
}

/// Tab providers affect resolution/animation rather than the base palette. Undeclared values
/// and inactive parents cannot enable them, even for a directly supplied theme.
pub(crate) fn tab_settings(file: &ThemeFile, values: &BTreeMap<String, OptionValue>) -> (bool, f32) {
    let offered = offered(file, PROVIDERS);
    let enabled = offered.iter().any(|o| o.id == "tab_colours") && values.get("tab_colours") == Some(&OptionValue::Bool(true));
    let duration = if enabled && offered.iter().any(|o| o.id == "tab_fade_ms") {
        match values.get("tab_fade_ms") {
            Some(OptionValue::Num(n)) if n.is_finite() => n.clamp(0.0, 600.0),
            _ => 0.0,
        }
    } else {
        0.0
    };
    (enabled, duration)
}

// Consumed by tab_settings or the blur-intent boundary; never store runtime state in tokens.
fn apply_runtime_setting(_: &mut Tokens, _: &OptionValue) {}

pub(crate) fn standard(id: &str) -> bool {
    matches!(id, "opacity" | "blur" | "tab_colours" | "tab_fade_ms")
}

pub(crate) fn provider<'a>(id: &str, providers: &'a [Provider]) -> Option<&'a Provider> {
    providers.iter().find(|p| p.id == id)
}

/// A child of an absent provider is absent too, even if it has a custom id.
pub(crate) fn offered<'a>(file: &'a ThemeFile, providers: &[Provider]) -> Vec<&'a OptionDef> {
    let ordered = crate::theme_files::ordered_options(file);
    ordered
        .iter()
        .copied()
        .filter(|option| {
            let mut current = Some(*option);
            for _ in 0..=crate::theme_files::MAX_OPTIONS {
                let Some(def) = current else { return true };
                if standard(&def.id) && provider(&def.id, providers).is_none() {
                    return false;
                }
                current = def.parent.as_ref().and_then(|id| ordered.iter().find(|o| &o.id == id).copied());
            }
            false
        })
        .collect()
}

pub(crate) fn apply(file: &ThemeFile, values: &BTreeMap<String, OptionValue>, tokens: &mut Tokens) {
    let offered = offered(file, PROVIDERS);
    for def in &offered {
        let mut parent = def.parent.as_deref();
        let mut active = true;
        for _ in 0..crate::theme_files::MAX_OPTIONS {
            let Some(id) = parent else { break };
            active &= values.get(id) == Some(&OptionValue::Bool(true));
            active &= provider(id, PROVIDERS).is_none_or(|p| (p.availability)() == Availability::Available);
            parent = offered.iter().find(|o| o.id == id).and_then(|o| o.parent.as_deref());
        }
        if let Some(provider) = provider(&def.id, PROVIDERS)
            && active
            && (provider.availability)() == Availability::Available
            && let Some(value) = values.get(&def.id)
        {
            (provider.apply)(tokens, value);
        }
    }
}

/// Adapt the option value; all colour math belongs to the theme boundary.
fn apply_opacity(t: &mut Tokens, value: &OptionValue) {
    if let OptionValue::Num(percent) = value {
        t.apply_panel_opacity(*percent);
    }
}
