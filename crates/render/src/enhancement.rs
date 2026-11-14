//! Optional enhancement (reconstruction, denoising, appearance) provider capability model.
//! Metadata only, like [`crate::backend`]: no device is probed, no SDK is linked and no job is
//! started. A GPU host may later describe its own providers through this model.
//!
//! Two selection semantics exist, mirroring the strict backend resolver: an explicitly chosen
//! provider never falls back to another, while `Auto` may pick any eligible provider and always
//! says so in a warning.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::backend::{Availability, MAX_RENDER_ID_BYTES, ResolveError};

/// Maximum ASCII bytes in a provider identifier.
pub const MAX_ENHANCEMENT_PROVIDER_ID_BYTES: usize = MAX_RENDER_ID_BYTES;

/// The error shape of this model, kept separate from [`ResolveError`] so the two registries
/// cannot be confused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnhancementError {
    InvalidId,
    InvalidRegistry(&'static str),
    DuplicateProvider(crate::backend::BackendId),
    UnknownProvider(crate::backend::BackendId),
    /// An explicitly chosen provider cannot run. The saved choice is retained, never replaced.
    ProviderUnavailable {
        provider: crate::backend::BackendId,
        reason: String,
    },
    /// An explicitly chosen provider cannot run with the supplied inputs.
    MissingInputs {
        provider: crate::backend::BackendId,
        missing: Vec<EnhancementInput>,
    },
}

impl fmt::Display for EnhancementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId => write!(
                f,
                "enhancement provider identifier must be 1-64 ASCII bytes, start with a lowercase letter and contain only lowercase letters, digits, '.', '_' or '-'"
            ),
            Self::InvalidRegistry(reason) => write!(f, "invalid enhancement registry: {reason}"),
            Self::DuplicateProvider(id) => write!(f, "duplicate enhancement provider: {}", id.as_str()),
            Self::UnknownProvider(id) => write!(f, "unknown enhancement provider: {}", id.as_str()),
            Self::ProviderUnavailable { provider, reason } => {
                write!(f, "enhancement provider {} unavailable: {reason}", provider.as_str())
            }
            Self::MissingInputs { provider, missing } => {
                let names = missing.iter().map(|i| format!("{i:?}")).collect::<Vec<_>>().join(", ");
                write!(f, "enhancement provider {} is missing inputs: {names}", provider.as_str())
            }
        }
    }
}

impl std::error::Error for EnhancementError {}

/// What an enhancement provider can do. Listing a variant does not advertise support for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnhancementFeature {
    /// Temporal or spatial upscaling from a lower internal resolution.
    Upscaling,
    /// Antialiasing at native render resolution, without upscaling.
    NativeAa,
    /// Noise reduction of a traced or sampled signal.
    Denoising,
    /// Learned appearance changes applied to a rendered frame (an "AI presentation" style pass).
    Appearance,
}

/// A frame signal a provider consumes. Callers declare what they supply; a provider is eligible
/// only when everything it requires is present. Supplied-but-unused inputs are fine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnhancementInput {
    LinearHdrColor,
    Depth,
    MotionVectors,
    Jitter,
    History,
}

/// One enhancement provider as its host describes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnhancementProvider {
    pub id: crate::backend::BackendId,
    pub label: String,
    pub availability: Availability,
    pub features: Vec<EnhancementFeature>,
    /// Inputs this provider consumes. Eligibility requires all of them to be supplied.
    pub requires: Vec<EnhancementInput>,
}

/// The user's enhancement intent. Serialized so a document can persist the intended choice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnhancementSelection {
    Off,
    /// Pick any eligible provider; render unenhanced (with a warning) when none is eligible.
    Auto,
    /// One named provider. Never falls back: an ineligible choice is an error.
    Provider(crate::backend::BackendId),
}

/// A resolved enhancement pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedEnhancement {
    pub provider: crate::backend::BackendId,
    pub features: Vec<EnhancementFeature>,
}

/// The outcome of resolving an enhancement selection: an optional pass plus warnings that a UI
/// should show. An explicit choice produces warnings only with a successful pass; failures are
/// errors. `Auto` always explains what it did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct EnhancementOutcome {
    pub enhancement: Option<ResolvedEnhancement>,
    pub warnings: Vec<String>,
}

fn validate_registry(providers: &[EnhancementProvider]) -> Result<(), EnhancementError> {
    if providers.len() > 32 {
        return Err(EnhancementError::InvalidRegistry("too many providers (maximum 32)"));
    }
    let mut ids = BTreeSet::new();
    for provider in providers {
        if !ids.insert(&provider.id) {
            return Err(EnhancementError::DuplicateProvider(provider.id.clone()));
        }
        if provider.features.len() > 16 || provider.features.len() != provider.features.iter().collect::<BTreeSet<_>>().len() {
            return Err(EnhancementError::InvalidRegistry("duplicate or too many features"));
        }
        if provider.requires.len() > 16 || provider.requires.len() != provider.requires.iter().collect::<BTreeSet<_>>().len() {
            return Err(EnhancementError::InvalidRegistry("duplicate or too many required inputs"));
        }
    }
    Ok(())
}

fn missing_inputs(provider: &EnhancementProvider, supplied: &[EnhancementInput]) -> Vec<EnhancementInput> {
    provider.requires.iter().copied().filter(|input| !supplied.contains(input)).collect()
}

/// Resolves the user's enhancement intent against a provider registry.
///
/// - `Off` resolves to `None`, always.
/// - An explicit provider that is unknown, unavailable or missing inputs is an error; it is never
///   silently replaced by another provider.
/// - `Auto` picks the lexicographically first eligible provider and records the choice in a
///   warning, or resolves to `None` with a warning when nothing is eligible.
pub fn resolve_enhancement(
    providers: &[EnhancementProvider],
    selection: &EnhancementSelection,
    supplied: &[EnhancementInput],
) -> Result<EnhancementOutcome, EnhancementError> {
    validate_registry(providers)?;
    let mut outcome = EnhancementOutcome::default();
    match selection {
        EnhancementSelection::Off => {}
        EnhancementSelection::Provider(id) => {
            let provider = providers.iter().find(|p| &p.id == id).ok_or_else(|| EnhancementError::UnknownProvider(id.clone()))?;
            if let Availability::Unavailable { reason } = &provider.availability {
                return Err(EnhancementError::ProviderUnavailable { provider: id.clone(), reason: reason.clone() });
            }
            let missing = missing_inputs(provider, supplied);
            if !missing.is_empty() {
                return Err(EnhancementError::MissingInputs { provider: id.clone(), missing });
            }
            let mut features = provider.features.clone();
            features.sort();
            outcome.enhancement = Some(ResolvedEnhancement { provider: id.clone(), features });
        }
        EnhancementSelection::Auto => {
            let mut eligible: Vec<_> =
                providers.iter().filter(|p| p.availability == Availability::Available && missing_inputs(p, supplied).is_empty()).collect();
            eligible.sort_by(|a, b| a.id.cmp(&b.id));
            match eligible.first() {
                Some(provider) => {
                    let mut features = provider.features.clone();
                    features.sort();
                    outcome.enhancement = Some(ResolvedEnhancement { provider: provider.id.clone(), features });
                    outcome.warnings.push(format!("enhancement provider {} selected automatically", provider.id.as_str()));
                }
                None => {
                    outcome.warnings.push("no eligible enhancement provider; rendering without enhancement".into());
                }
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, availability: Availability, features: &[EnhancementFeature], requires: &[EnhancementInput]) -> EnhancementProvider {
        EnhancementProvider {
            id: crate::backend::BackendId::new(id).unwrap(),
            label: id.into(),
            availability,
            features: features.into(),
            requires: requires.into(),
        }
    }

    fn all_inputs() -> Vec<EnhancementInput> {
        vec![
            EnhancementInput::LinearHdrColor,
            EnhancementInput::Depth,
            EnhancementInput::MotionVectors,
            EnhancementInput::Jitter,
            EnhancementInput::History,
        ]
    }

    #[test]
    fn off_resolves_to_none() {
        let providers = vec![provider("fsr", Availability::Available, &[EnhancementFeature::Upscaling], &all_inputs())];
        let outcome = resolve_enhancement(&providers, &EnhancementSelection::Off, &all_inputs()).unwrap();
        assert_eq!(outcome.enhancement, None);
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn explicit_eligible_provider_resolves_without_warnings() {
        let providers = vec![provider(
            "fsr",
            Availability::Available,
            &[EnhancementFeature::Upscaling, EnhancementFeature::NativeAa],
            &[EnhancementInput::LinearHdrColor, EnhancementInput::Depth],
        )];
        let outcome = resolve_enhancement(
            &providers,
            &EnhancementSelection::Provider(crate::backend::BackendId::new("fsr").unwrap()),
            &[EnhancementInput::LinearHdrColor],
        )
        .unwrap();
        assert!(outcome.warnings.is_empty());
        let resolved = outcome.enhancement.unwrap();
        assert_eq!(resolved.provider.as_str(), "fsr");
        assert_eq!(resolved.features, vec![EnhancementFeature::NativeAa, EnhancementFeature::Upscaling]);
    }

    #[test]
    fn explicit_provider_never_falls_back() {
        let fsr = crate::backend::BackendId::new("fsr").unwrap();
        let providers = vec![
            provider("fsr", Availability::Unavailable { reason: "no Vulkan device".into() }, &[EnhancementFeature::Upscaling], &[]),
            provider("dlss", Availability::Available, &[EnhancementFeature::Upscaling], &[]),
        ];
        let error = resolve_enhancement(&providers, &EnhancementSelection::Provider(fsr.clone()), &all_inputs()).unwrap_err();
        assert_eq!(error, EnhancementError::ProviderUnavailable { provider: fsr.clone(), reason: "no Vulkan device".into() });
        assert!(error.to_string().contains("no Vulkan device"));
    }

    #[test]
    fn explicit_provider_reports_missing_inputs() {
        let fsr = crate::backend::BackendId::new("fsr").unwrap();
        let providers = vec![provider(
            "fsr",
            Availability::Available,
            &[EnhancementFeature::Upscaling],
            &[EnhancementInput::MotionVectors, EnhancementInput::History],
        )];
        let error = resolve_enhancement(&providers, &EnhancementSelection::Provider(fsr), &[EnhancementInput::LinearHdrColor]).unwrap_err();
        assert_eq!(
            error,
            EnhancementError::MissingInputs {
                provider: crate::backend::BackendId::new("fsr").unwrap(),
                missing: vec![EnhancementInput::MotionVectors, EnhancementInput::History]
            }
        );
    }

    #[test]
    fn unknown_explicit_provider_is_an_error() {
        let providers = vec![provider("fsr", Availability::Available, &[EnhancementFeature::Upscaling], &[])];
        let error =
            resolve_enhancement(&providers, &EnhancementSelection::Provider(crate::backend::BackendId::new("dlss").unwrap()), &[]).unwrap_err();
        assert_eq!(error, EnhancementError::UnknownProvider(crate::backend::BackendId::new("dlss").unwrap()));
    }

    #[test]
    fn auto_selects_first_eligible_provider_and_warns() {
        let providers = vec![
            provider("dlss", Availability::Available, &[EnhancementFeature::Upscaling], &[]),
            provider("fsr", Availability::Available, &[EnhancementFeature::Upscaling, EnhancementFeature::Denoising], &[]),
        ];
        let outcome = resolve_enhancement(&providers, &EnhancementSelection::Auto, &all_inputs()).unwrap();
        let resolved = outcome.enhancement.unwrap();
        assert_eq!(resolved.provider.as_str(), "dlss");
        assert_eq!(outcome.warnings, vec!["enhancement provider dlss selected automatically"]);
    }

    #[test]
    fn auto_skips_unavailable_and_input_starved_providers() {
        let providers = vec![
            provider("dlss", Availability::Unavailable { reason: "no RTX".into() }, &[EnhancementFeature::Upscaling], &[]),
            provider("fsr", Availability::Available, &[EnhancementFeature::Upscaling], &[EnhancementInput::MotionVectors]),
        ];
        let outcome = resolve_enhancement(&providers, &EnhancementSelection::Auto, &[EnhancementInput::LinearHdrColor]).unwrap();
        let resolved = outcome.enhancement.unwrap();
        assert_eq!(resolved.provider.as_str(), "fsr");
        assert!(outcome.warnings[0].contains("selected automatically"));
    }

    #[test]
    fn auto_with_no_eligible_provider_renders_unenhanced_with_a_warning() {
        let providers = vec![provider("fsr", Availability::Unavailable { reason: "no device".into() }, &[EnhancementFeature::Upscaling], &[])];
        let outcome = resolve_enhancement(&providers, &EnhancementSelection::Auto, &all_inputs()).unwrap();
        assert_eq!(outcome.enhancement, None);
        assert!(outcome.warnings[0].contains("no eligible enhancement provider"));
    }

    #[test]
    fn duplicate_provider_is_rejected() {
        let providers = vec![provider("fsr", Availability::Available, &[], &[]), provider("fsr", Availability::Available, &[], &[])];
        assert_eq!(
            resolve_enhancement(&providers, &EnhancementSelection::Off, &[]).unwrap_err(),
            EnhancementError::DuplicateProvider(crate::backend::BackendId::new("fsr").unwrap())
        );
    }

    #[test]
    fn selection_serializes_snake_case_with_named_provider() {
        let json = serde_json::to_string(&EnhancementSelection::Provider(crate::backend::BackendId::new("fsr").unwrap())).unwrap();
        assert_eq!(json, r#"{"provider":"fsr"}"#);
        let back: EnhancementSelection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, EnhancementSelection::Provider(crate::backend::BackendId::new("fsr").unwrap()));
        assert_eq!(serde_json::to_string(&EnhancementSelection::Off).unwrap(), r#""off""#);
        assert_eq!(serde_json::to_string(&EnhancementSelection::Auto).unwrap(), r#""auto""#);
    }
}
