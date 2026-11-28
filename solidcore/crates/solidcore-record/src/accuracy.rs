//! How exactly a drawn curve stands for its source: the accuracy a drawing or section carries
//! with each curve, as data. Scaling it with a view is ops' (`solidcore_ops::hlr::scaled`).

use serde::{Deserialize, Serialize};

/// How exactly a drawn curve stands for its source.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveAccuracy {
    /// An analytic representation without fitting; not a certificate about the source or
    /// topology.
    Analytic,
    /// A producer-established bound, not merely a requested fitting tolerance.
    Bounded {
        /// The bound (mm).
        max_deviation_mm: f64,
    },
    /// A deviation measured at samples only, with no bound between them. Zero remains sampled.
    Sampled {
        /// The largest deviation measured (mm).
        max_sample_deviation_mm: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::CurveAccuracy;

    /// The serde form: a `kind` tag in snake case, the fields by name, nothing else accepted.
    #[test]
    fn the_serde_form_is_tagged_by_kind() {
        let cases = [
            (CurveAccuracy::Analytic, r#"{"kind":"analytic"}"#),
            (
                CurveAccuracy::Bounded {
                    max_deviation_mm: 0.5,
                },
                r#"{"kind":"bounded","max_deviation_mm":0.5}"#,
            ),
            (
                CurveAccuracy::Sampled {
                    max_sample_deviation_mm: 0.0,
                },
                r#"{"kind":"sampled","max_sample_deviation_mm":0.0}"#,
            ),
        ];
        for (accuracy, json) in cases {
            assert_eq!(serde_json::to_string(&accuracy).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<CurveAccuracy>(json).unwrap(),
                accuracy
            );
        }
        for bad in [
            r#"{"kind":"exact"}"#,
            r#"{"kind":"bounded","max_deviation_mm":0.1,"exact":true}"#,
            r#"{"max_deviation_mm":0.1}"#,
        ] {
            assert!(serde_json::from_str::<CurveAccuracy>(bad).is_err(), "{bad}");
        }
    }
}
