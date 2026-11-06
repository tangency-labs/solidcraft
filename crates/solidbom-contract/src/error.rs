//! Errors a contract value can carry, and the one place geometry becomes a decimal.

use rust_decimal::Decimal;
use rust_decimal::prelude::FromStr;

/// What can be wrong with a value received from another program.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    /// The value was written by a different contract version.
    #[error("unsupported SolidBOM schema version {found}; this build reads version {expected}")]
    SchemaVersion { found: u32, expected: u32 },
    /// A geometric number is NaN, infinite, or too large for a decimal.
    #[error("`{field}` is not a finite decimal number")]
    NonFinite { field: String },
    /// A field does not satisfy its rule.
    #[error("`{field}`: {reason}")]
    Invalid { field: String, reason: String },
}

impl ContractError {
    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Invalid { field: field.into(), reason: reason.into() }
    }
}

/// A contract operation's result.
pub type Result<T> = std::result::Result<T, ContractError>;

/// Convert one finite geometric `f64` to a decimal through its shortest round-trip text.
///
/// The conversion happens once, here: later arithmetic never starts from a displayed rounding.
pub fn decimal_from_f64(value: f64, field: &str) -> Result<Decimal> {
    if !value.is_finite() {
        return Err(ContractError::NonFinite { field: field.to_string() });
    }
    let text = value.to_string();
    let parsed = if text.contains(['e', 'E']) { Decimal::from_scientific(&text) } else { Decimal::from_str(&text) };
    parsed.map_err(|_| ContractError::NonFinite { field: field.to_string() })
}

/// A SHA-256 digest as the contract spells it: 64 lower-case hexadecimal characters.
pub fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Check a SHA-256 digest, naming the field in the error.
pub fn check_sha256(text: &str, field: &str) -> Result<()> {
    if is_sha256(text) { Ok(()) } else { Err(ContractError::invalid(field, "expected 64 lower-case hexadecimal characters")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_geometry_number_keeps_its_shortest_round_trip_text() {
        assert_eq!(decimal_from_f64(2400.0, "volume").unwrap().to_string(), "2400");
        assert_eq!(decimal_from_f64(2.7, "density").unwrap().to_string(), "2.7");
        assert_eq!(decimal_from_f64(0.1 + 0.2, "sum").unwrap().to_string(), "0.30000000000000004");
    }

    #[test]
    fn a_non_finite_geometry_number_is_refused() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(decimal_from_f64(value, "volume"), Err(ContractError::NonFinite { .. })));
        }
    }

    #[test]
    fn a_sha256_digest_is_64_lower_case_hexadecimal_characters() {
        assert!(is_sha256(&"a".repeat(64)));
        assert!(!is_sha256(&"A".repeat(64)));
        assert!(!is_sha256(&"a".repeat(63)));
        assert!(!is_sha256(&"g".repeat(64)));
    }
}
