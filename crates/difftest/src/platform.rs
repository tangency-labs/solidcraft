//! Platform requirements of the native reference, independent of Rust portability.
//!
//! Some pinned C++ operations convert out-of-range floats to integers (undefined
//! behavior). The Rust port deliberately reproduces the x86-64 reference on every
//! target. A different native instruction cannot act as its parity oracle (#448).

/// A documented architecture requirement of a unit's C++ reference.
#[derive(Debug, Clone, Copy)]
pub struct ReferencePlatform {
    /// Architecture name, using Rust's `std::env::consts::ARCH` spelling.
    pub arch: &'static str,
    /// Why other architectures cannot supply this unit's reference results.
    pub reason: &'static str,
}

impl ReferencePlatform {
    /// Reason to skip comparison on `arch`; `None` when it is supported.
    pub fn skip_reason(self, arch: &str) -> Option<&'static str> {
        (arch != self.arch).then_some(self.reason)
    }
}

/// Native voxel mapping (`SmObjsInVoxels::MapPointToVoxel`) depends on x86-64 float casts.
pub const X86_64_HASH_CASTS: ReferencePlatform = ReferencePlatform {
    arch: "x86_64",
    reason: "C++ float-to-ULONG conversions require x86-64 (UB-35, #448)",
};

/// Native twist-period reduction uses an out-of-range float-to-LONG conversion.
pub const X86_64_TWIST_CASTS: ReferencePlatform = ReferencePlatform {
    arch: "x86_64",
    reason: "C++ float-to-LONG conversion requires x86-64 (TwistVolume::EvaluateBoundingBoxSimple, #448)",
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    use crate::{Case, CaseContext, CaseSource, HarnessError, Output, RunOptions, UnitDiff};

    #[test]
    fn reference_requirements_are_architecture_specific() {
        for platform in [X86_64_HASH_CASTS, X86_64_TWIST_CASTS] {
            assert_eq!(platform.skip_reason("x86_64"), None);
            for arch in ["aarch64", "arm", "x86", "wasm32"] {
                assert_eq!(platform.skip_reason(arch), Some(platform.reason));
            }
        }
    }

    #[derive(Default)]
    struct Restricted {
        cases: Cell<usize>,
        rust: Cell<usize>,
    }

    impl UnitDiff for Restricted {
        fn id(&self) -> &'static str {
            "test.platform"
        }

        fn reference_platform(&self) -> Option<ReferencePlatform> {
            Some(ReferencePlatform {
                arch: "unsupported-test-architecture",
                reason: "test reference is unavailable",
            })
        }

        fn cases(&self, _: &CaseContext) -> Result<Vec<Case>, HarnessError> {
            self.cases.set(self.cases.get() + 1);
            Ok(vec![Case {
                unit: self.id().into(),
                name: "value".into(),
                source: CaseSource::Inline,
                inputs: serde_json::Value::Null,
            }])
        }

        fn run_cpp(&self, _: &Case) -> Result<Output, HarnessError> {
            panic!("unsupported C++ must never run")
        }

        fn run_rust(&self, _: &Case) -> Result<Output, HarnessError> {
            self.rust.set(self.rust.get() + 1);
            Ok(Output::new().scalar("value", 42.0))
        }
    }

    #[test]
    fn unsupported_comparisons_skip_before_fixture_construction_and_never_pass() {
        let unit = Restricted::default();
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        for perturb in [false, true] {
            let r = crate::run_unit(
                &unit,
                &ctx,
                &RunOptions {
                    perturb,
                    ..RunOptions::default()
                },
            );
            assert_eq!(r.status(), crate::UnitStatus::SkippedPlatform);
            assert!(!r.pass());
            assert!(r.cases.is_empty());
            assert_eq!((unit.cases.get(), unit.rust.get()), (0, 0));
            let report = crate::report::unit_report(&r);
            assert_eq!(report["status"], "skipped(platform)");
            assert_eq!(report["pass"], false);
            assert_eq!(report["cases_passed"], 0);
            assert_eq!(report["skipped_platform"], "test reference is unavailable");
            assert!(crate::report::summary_line(&r)
                .starts_with("SKIP test.platform skipped(platform):"));
            // A platform-only report must never replace a trusted CI baseline.
            assert!(crate::baseline::parse_baseline(&report.to_string()).is_err());
            let baseline = crate::baseline::Baseline {
                unit: unit.id().into(),
                status: Some("green".into()),
                seeds: Some(16),
                cases: vec![crate::baseline::BaselineCase {
                    name: "value".into(),
                    outcome: "pass".into(),
                    generated: false,
                    compared_values: true,
                }],
            };
            assert!(!crate::baseline::ratchet(&baseline, &r).is_empty());
        }
    }

    #[test]
    fn fuzz_skips_without_workers_but_rust_digests_still_run() {
        let unit = Restricted::default();
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let r = crate::fuzz::run_fuzz(
            &unit,
            &ctx,
            &crate::fuzz::FuzzOptions {
                cases: 10,
                out_dir: ctx.root.join("bin/platform-test-unused"),
                filter: None,
            },
        );
        assert!(r.skipped_platform.is_some());
        assert!(!r.pass());
        assert!(r.verdicts.is_empty());
        assert_eq!((unit.cases.get(), unit.rust.get()), (0, 0));
        assert!(crate::fuzz::summary_line(&r).starts_with("FUZZ-SKIP test.platform"));

        let digest = crate::runner::digest_unit(&unit, &ctx, &RunOptions::default(), false);
        assert!(digest.harness_error.is_none());
        assert_eq!(digest.lines.len(), 1);
        assert_eq!((unit.cases.get(), unit.rust.get()), (1, 1));
    }

    #[test]
    #[cfg(feature = "cpp")]
    fn registry_marks_only_the_documented_units() {
        let marked: Vec<_> = crate::registry()
            .iter()
            .filter_map(|u| {
                u.reference_platform().map(|p| {
                    assert_eq!(p.arch, "x86_64");
                    u.id()
                })
            })
            .collect();
        assert_eq!(
            marked,
            [
                "geom.hash_table",
                "heal.heal_data_p1",
                "heal.heal_data_p2",
                "heal.heal_data_p3",
                "surface.twist_volume"
            ]
        );
    }
}
